//! Exact rolling activity, retained only in API memory. Each canonical extension
//! reads its new transactions once. A changed cached-tip hash rebuilds the window.
use anyhow::{anyhow, ensure, Result};
use ckbadger_common::stats_bucket::{DAY_MS, HOUR_MS};
use ckbadger_store::types::ParticipantId;
use ckbadger_store::{CachedBlockHeader, CkbadgerStore, ClassifiedTxActions, DailyActivityStats};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

#[derive(Default)]
pub struct ActivityWindowCache(Mutex<Option<ActivityWindow>>);

#[derive(Default)]
struct ActivityWindow {
    tip: Option<(i64, Vec<u8>)>,
    blocks: BTreeMap<i64, BlockActivity>,
    stats: DailyActivityStats,
    addresses: HashMap<[u8; 32], u32>,
    hours: HashMap<i64, u32>,
}

struct BlockActivity {
    chain_time_ms: i64,
    stats: DailyActivityStats,
    addresses: HashSet<[u8; 32]>,
}

impl ActivityWindowCache {
    /// The window's activity and the UTC hours it covers. Call in a blocking
    /// task while the request's read view is pinned. Its non-cellbase
    /// transactions are `kind_counts.total()`: each is in exactly one kind.
    pub fn summary(
        &self,
        store: &CkbadgerStore,
        tip: Option<(i64, CachedBlockHeader)>,
    ) -> Result<(DailyActivityStats, u32)> {
        let mut slot = self
            .0
            .lock()
            .map_err(|_| anyhow!("activity window lock poisoned"))?;
        // An error discards any partly updated memory; it never serves stale totals.
        let mut window = slot.take().unwrap_or_default();
        let Some((tip_number, tip_header)) = tip else {
            return Ok((DailyActivityStats::default(), 0));
        };
        if let Some((number, hash)) = &window.tip {
            let extends = if *number > tip_number {
                false // A rollback shortened the canonical chain.
            } else {
                let header = store.get_block_header(*number)?.ok_or_else(|| {
                    anyhow!(
                    "missing cached activity tip header: block={number}, current_tip={tip_number}"
                )
                })?;
                header.hash == *hash
            };
            if !extends {
                window = ActivityWindow::default();
            }
        }
        window.advance(store, tip_number, &tip_header)?;
        let answer = (window.stats.clone(), u32::try_from(window.hours.len())?);
        *slot = Some(window);
        Ok(answer)
    }
}

impl ActivityWindow {
    fn advance(
        &mut self,
        store: &CkbadgerStore,
        tip_number: i64,
        tip: &CachedBlockHeader,
    ) -> Result<()> {
        if self
            .tip
            .as_ref()
            .is_some_and(|(number, hash)| *number == tip_number && *hash == tip.hash)
        {
            return Ok(());
        }
        let cutoff = tip
            .chain_time_ms
            .checked_sub(DAY_MS)
            .ok_or_else(|| anyhow!("activity 24h cutoff underflow: tip={tip_number}"))?;
        let old_tip = self.tip.as_ref().map(|(number, _)| *number);
        let mut additions = BTreeMap::<i64, BlockActivity>::new();
        let mut cursor = None;
        const PAGE_SIZE: usize = 512;
        'pages: loop {
            // The page's headers come with it: its completion read them.
            let (page, headers) = store.list_tx_actions_recent_page(PAGE_SIZE, cursor, old_tip)?;
            if page.is_empty() {
                break;
            }
            for tx in &page {
                ensure!(
                    tx.block_number <= tip_number,
                    "activity row above sync tip: tx=0x{} block={} tip={tip_number}",
                    hex::encode(&tx.tx_hash),
                    tx.block_number
                );
                let header = headers.get(&tx.block_number).ok_or_else(|| {
                    anyhow!(
                        "missing block header for activity row: tx=0x{} block={}",
                        hex::encode(&tx.tx_hash),
                        tx.block_number
                    )
                })?;
                if header.chain_time_ms <= cutoff {
                    break 'pages;
                }
                let block = additions
                    .entry(tx.block_number)
                    .or_insert_with(|| BlockActivity {
                        chain_time_ms: header.chain_time_ms,
                        stats: DailyActivityStats::default(),
                        addresses: HashSet::new(),
                    });
                // The one classification the filter lists the transaction by.
                block
                    .stats
                    .accumulate_from_tx_actions(ClassifiedTxActions::classify(tx));
                if !tx.is_cellbase {
                    for participant in &tx.participants {
                        if let ParticipantId::Lock(hash) = participant.id {
                            block.addresses.insert(hash);
                        }
                    }
                }
            }
            let last = page.last().expect("nonempty page");
            cursor = Some((last.block_number, last.tx_index));
            if page.len() < PAGE_SIZE {
                break;
            }
        }
        for (number, block) in additions {
            self.apply(&block, true)?;
            ensure!(
                self.blocks.insert(number, block).is_none(),
                "duplicate rolling activity block {number}"
            );
        }
        while self
            .blocks
            .first_key_value()
            .is_some_and(|(_, block)| block.chain_time_ms <= cutoff)
        {
            let (_, block) = self.blocks.pop_first().expect("checked nonempty window");
            self.apply(&block, false)?;
        }
        self.stats.unique_address_count = u32::try_from(self.addresses.len())?;
        self.tip = Some((tip_number, tip.hash.clone()));
        Ok(())
    }

    fn apply(&mut self, block: &BlockActivity, add: bool) -> Result<()> {
        self.stats.kind_counts = if add {
            self.stats.kind_counts.checked_add(&block.stats.kind_counts)
        } else {
            self.stats.kind_counts.checked_sub(&block.stats.kind_counts)
        }
        .map_err(|e| {
            anyhow!(
                "rolling activity {e} at chain_time_ms={}",
                block.chain_time_ms
            )
        })?;
        macro_rules! counters {
            ($($field:ident),+ $(,)?) => { $(
                self.stats.$field = if add { self.stats.$field.checked_add(block.stats.$field) }
                    else { self.stats.$field.checked_sub(block.stats.$field) }
                    .ok_or_else(|| anyhow!("rolling activity {} overflow/underflow at chain_time_ms={}", stringify!($field), block.chain_time_ms))?;
            )+ };
        }
        counters!(coinbase_count, total_ckb_moved);
        for (key, count) in &block.stats.script_counts {
            update_count(&mut self.stats.script_counts, key.clone(), *count, add)?;
        }
        for (key, count) in &block.stats.protocol_action_counts {
            update_count(
                &mut self.stats.protocol_action_counts,
                key.clone(),
                *count,
                add,
            )?;
        }
        for address in &block.addresses {
            update_refcount(&mut self.addresses, *address, add)?;
        }
        update_refcount(
            &mut self.hours,
            block.chain_time_ms.div_euclid(HOUR_MS),
            add,
        )?;
        Ok(())
    }
}

fn update_count(map: &mut BTreeMap<String, u32>, key: String, delta: u32, add: bool) -> Result<()> {
    let count = map.entry(key.clone()).or_default();
    *count = if add {
        count.checked_add(delta)
    } else {
        count.checked_sub(delta)
    }
    .ok_or_else(|| anyhow!("rolling activity count overflow/underflow: key={key} delta={delta}"))?;
    if *count == 0 {
        map.remove(&key);
    }
    Ok(())
}

fn update_refcount<K: Eq + std::hash::Hash + Copy>(
    map: &mut HashMap<K, u32>,
    key: K,
    add: bool,
) -> Result<()> {
    let count = map.entry(key).or_default();
    *count = if add {
        count.checked_add(1)
    } else {
        count.checked_sub(1)
    }
    .ok_or_else(|| anyhow!("rolling activity reference count overflow/underflow"))?;
    if *count == 0 {
        map.remove(&key);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::{
        types::{ParticipantDelta, TxActions},
        StoreBatch,
    };
    use std::time::Instant;

    fn seed(store: &CkbadgerStore, number: i64, time: i64, hash_byte: u8, transactions: i32) {
        let mut batch = StoreBatch::new(store);
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![hash_byte; 32],
                timestamp: time,
                chain_time_ms: time,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: transactions,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
        for index in 0..transactions {
            // The transaction the row's position resolves to.
            batch.put_tx_index(
                number,
                index,
                &ckbadger_store::types::TxIndexEntry {
                    tx_hash: [index as u8; 32],
                    is_cellbase: index == 0,
                    timestamp: time,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: 0,
                    tx_size: 1,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
            batch.put_tx_actions(&TxActions {
                block_number: number,
                block_hash: vec![hash_byte; 32],
                tx_hash: vec![index as u8; 32],
                tx_index: index,
                timestamp: time,
                is_cellbase: index == 0,
                protocol_actions: vec![],
                type_calls: vec![],
                lock_calls: vec![],
                participants: vec![ParticipantDelta {
                    id: ParticipantId::Lock([hash_byte % 3; 32]),
                    ckb_delta: 7,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: 0,
                    roles: 0,
                }],
            });
        }
        batch.commit().unwrap();
    }

    #[test]
    fn extension_eviction_and_reorg_equal_fresh_exact_windows() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        for number in 0..128 {
            seed(&store, number, DAY_MS + number * 1000, number as u8, 32);
        }
        let cache = ActivityWindowCache::default();
        let started = Instant::now();
        let initial = cache
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        let rebuild_us = started.elapsed().as_micros();
        assert_eq!(initial.0.kind_counts.ckb, 128 * 31);
        assert_eq!(initial.0.kind_counts.total(), 128 * 31);
        assert_eq!(initial.0.coinbase_count, 128);
        assert_eq!(initial.0.unique_address_count, 3);
        seed(&store, 128, DAY_MS + 128_000, 128, 32);
        let started = Instant::now();
        let extended = cache
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        let extension_us = started.elapsed().as_micros();
        assert_eq!(extended.0.kind_counts.ckb, 129 * 31);
        assert_eq!(extended.0.kind_counts.total(), 129 * 31);
        eprintln!("activity window synthetic: rebuild 4096 rows={rebuild_us}us; extension 32 rows={extension_us}us");
        // A window jump evicts whole blocks at the exact inclusive lower boundary.
        seed(&store, 129, 2 * DAY_MS + 127_000, 129, 32);
        let evicted = cache
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        assert_eq!(evicted.0.kind_counts.ckb, 2 * 31);
        assert_eq!(evicted.0.kind_counts.total(), 2 * 31);
        assert_eq!(evicted.0.total_ckb_moved, 2 * 31 * 7);
        assert_eq!(evicted.0.unique_address_count, 2);
        // Same-height reorg changes a participant and lowers time, reopening the window.
        seed(&store, 129, DAY_MS + 129_000, 130, 32);
        let reorg = cache
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        let fresh = ActivityWindowCache::default()
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        assert_eq!(
            serde_json::to_value(&reorg).unwrap(),
            serde_json::to_value(&fresh).unwrap()
        );
        assert_eq!(reorg.0.kind_counts.ckb, 130 * 31);
        assert_eq!(reorg.0.kind_counts.total(), 130 * 31);
        assert_eq!(reorg.0.unique_address_count, 3);
        assert_eq!(
            cache.summary(&store, None).unwrap().0.kind_counts.total(),
            0
        );
        assert!(cache.0.lock().unwrap().is_none());
    }

    #[test]
    fn eviction_removes_expired_script_and_protocol_counts() {
        use ckbadger_store::types::{LockCallEntry, ProtocolAction};
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let cache = ActivityWindowCache::default();
        for (number, action, script) in [(0, "deposit", 0xAA), (1, "withdraw_request", 0xBB)] {
            seed(&store, number, DAY_MS * (number + 1), number as u8, 2);
            let mut tx = store.get_tx_actions(number, 1, &[1; 32]).unwrap().unwrap();
            tx.protocol_actions = vec![ProtocolAction::new("dao", action, serde_json::json!({}))];
            tx.lock_calls = vec![LockCallEntry {
                lock_code_hash: vec![script; 32],
                lock_hash_type: 1,
                lock_args: vec![],
            }];
            let mut batch = StoreBatch::new(&store);
            batch.put_tx_actions(&tx);
            batch.commit().unwrap();
            let (stats, _) = cache
                .summary(&store, store.get_sync_tip_block().unwrap())
                .unwrap();
            assert_eq!(stats.coinbase_count, 1);
            // The window holds this block only; its transaction is a DAO one.
            assert_eq!(
                stats.kind_counts,
                ckbadger_store::ActivityKindCounts {
                    dao: 1,
                    ..Default::default()
                }
            );
            assert_eq!(stats.total_ckb_moved, 7);
            assert_eq!(
                stats.script_counts,
                BTreeMap::from([(hex::encode([script; 32]), 1)])
            );
            assert_eq!(
                stats.protocol_action_counts,
                BTreeMap::from([(format!("dao:{action}"), 1)])
            );
        }
    }

    /// A block as the indexer leaves it: its header with `transactions`
    /// transactions, a `tx_index` row for each (position 0 the cellbase), and
    /// an `activities` row for every transaction but the cellbase, which
    /// production never writes.
    fn seed_production_block(
        store: &CkbadgerStore,
        number: i64,
        time: i64,
        hash_byte: u8,
        transactions: i32,
    ) {
        let mut batch = StoreBatch::new(store);
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![hash_byte; 32],
                timestamp: time,
                chain_time_ms: time,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: transactions,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
        for index in 0..transactions {
            batch.put_tx_index(
                number,
                index,
                &ckbadger_store::types::TxIndexEntry {
                    tx_hash: [hash_byte.wrapping_add(index as u8); 32],
                    is_cellbase: index == 0,
                    timestamp: time,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: 0,
                    tx_size: 1,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
            if index > 0 {
                batch.put_tx_actions(&TxActions {
                    block_number: number,
                    block_hash: vec![hash_byte; 32],
                    tx_hash: vec![hash_byte.wrapping_add(index as u8); 32],
                    tx_index: index,
                    timestamp: time,
                    is_cellbase: false,
                    protocol_actions: vec![],
                    type_calls: vec![],
                    lock_calls: vec![],
                    participants: vec![ParticipantDelta {
                        id: ParticipantId::Lock([hash_byte; 32]),
                        ckb_delta: 7,
                        used_delta: 0,
                        item_deltas: vec![],
                        tags: 0,
                        roles: 0,
                    }],
                });
            }
        }
        batch.commit().unwrap();
    }

    /// F-6 follow-up: the window's cellbases. A cellbase has no `activities`
    /// row, so the window counts it from the canonical block that holds it,
    /// one per block — blocks with only their cellbase included — and the
    /// window's kinds plus its cellbases are its blocks' transactions. A
    /// block leaving the window takes its cellbase with it, and a reorg's
    /// rebuilt window equals a fresh one.
    #[test]
    fn the_window_counts_one_cellbase_per_block_and_kinds_plus_cellbases_are_its_transactions() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let summary = |cache: &ActivityWindowCache| {
            cache
                .summary(&store, store.get_sync_tip_block().unwrap())
                .unwrap()
                .0
        };
        // (block, its transactions with the cellbase); blocks 1 and 3 hold
        // only their cellbase.
        let blocks = [(0i64, 3i32), (1, 1), (2, 4), (3, 1), (4, 2), (5, 2)];
        for (number, transactions) in blocks {
            seed_production_block(
                &store,
                number,
                DAY_MS + number * 1000,
                number as u8 * 16,
                transactions,
            );
        }
        let cache = ActivityWindowCache::default();
        let window = summary(&cache);
        assert_eq!(
            window.coinbase_count, 6,
            "one cellbase per block of the window"
        );
        assert_eq!(
            window.kind_counts.total() + u64::from(window.coinbase_count),
            blocks.iter().map(|(_, t)| *t as u64).sum::<u64>(),
            "kinds plus cellbases are the window's transactions"
        );

        // Block 6 moves the cutoff past blocks 0..=3: their cellbases leave.
        seed_production_block(&store, 6, 2 * DAY_MS + 3_500, 0x60, 3);
        let window = summary(&cache);
        assert_eq!(window.coinbase_count, 3, "blocks 4, 5 and 6");
        assert_eq!(
            window.kind_counts.total() + u64::from(window.coinbase_count),
            2 + 2 + 3
        );

        // A same-height reorg: block 6 replaced (a new hash, an earlier time
        // that reopens the window to block 3): the rebuilt window is a fresh one.
        seed_production_block(&store, 6, 2 * DAY_MS + 2_500, 0x61, 3);
        let reorged = summary(&cache);
        assert_eq!(reorged, summary(&ActivityWindowCache::default()));
        assert_eq!(reorged.coinbase_count, 4, "blocks 3, 4, 5 and the new 6");
        assert_eq!(
            reorged.kind_counts.total() + u64::from(reorged.coinbase_count),
            1 + 2 + 2 + 3
        );
    }

    #[test]
    fn invalid_new_row_is_an_error_and_discards_partial_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        seed(&store, 0, DAY_MS, 1, 2);
        let cache = ActivityWindowCache::default();
        cache
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        seed(&store, 1, DAY_MS + 1000, 2, 2);
        // The new block's second row points at a position with no transaction.
        store
            .delete_cf(
                store.cf_tx_index(),
                &ckbadger_store::keys::encode_tx_slot_key(1, 1),
            )
            .unwrap();
        let error = cache
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("activities points at block=1, tx_idx=1, which has no tx_index row"),
            "{error}"
        );
        assert!(cache.0.lock().unwrap().is_none());
    }
    /// Each transaction of the window is counted once, in the kind the global
    /// filter lists it under: a token and object transfer that also calls an
    /// unrecognized type script is a token transaction; a CKB-only Fiber
    /// channel open (F-6) is a protocol one. The kinds sum to the window's
    /// non-cellbase transactions.
    #[test]
    fn every_window_transaction_is_counted_once_in_its_filter_kind() {
        use ckbadger_store::types::{ProtocolAction, TypeCallEntry, TAG_OBJECT, TAG_TOKEN};
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        seed(&store, 1, DAY_MS, 1, 3);
        let mut mixed = store.get_tx_actions(1, 1, &[1; 32]).unwrap().unwrap();
        mixed.participants[0].tags = TAG_TOKEN | TAG_OBJECT;
        mixed.type_calls.push(TypeCallEntry {
            type_code_hash: vec![0xAA; 32],
            type_hash_type: 1,
            type_args: vec![],
        });
        let mut protocol = store.get_tx_actions(1, 2, &[2; 32]).unwrap().unwrap();
        protocol
            .protocol_actions
            .push(ProtocolAction::new("fiber", "open", serde_json::json!({})));
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_actions(&mixed);
        batch.put_tx_actions(&protocol);
        batch.commit().unwrap();
        let summary = ActivityWindowCache::default()
            .summary(&store, store.get_sync_tip_block().unwrap())
            .unwrap();
        assert_eq!(
            summary.0.kind_counts,
            ckbadger_store::ActivityKindCounts {
                token: 1,
                protocol: 1,
                ..Default::default()
            }
        );
        assert_eq!(summary.0.kind_counts.total(), 2);
        assert_eq!(summary.0.coinbase_count, 1);
        for tx in [&mixed, &protocol] {
            let kind = ckbadger_store::classify_global_bucket(tx).unwrap();
            assert_eq!(summary.0.kind_counts.get(kind), 1, "{}", kind.as_filter());
        }
    }
}
