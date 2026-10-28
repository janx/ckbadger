use anyhow::{anyhow, ensure, Context, Result};
use chrono::Utc;
use tracing::info;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys::{self, sync_meta_keys};
use ckbadger_store::types::{DeepForkInfo, ReorgEvent, ReorgEventKind};
use ckbadger_store::CkbadgerStore;

use super::{BatchWriter, ContractRequirement};

/// Build the history key for an event detected now.
///
/// The millisecond field orders the history and the uuid keeps two events
/// detected in the same millisecond distinct.
fn next_reorg_event_key() -> Result<String> {
    keys::encode_reorg_event_key(
        Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4().as_bytes(),
    )
}

impl BatchWriter {
    /// Persist interruption before any canonical mutation. A surviving pending
    /// marker therefore proves that no later rollback has even begun.
    pub(crate) fn begin_rollback(&self) -> Result<()> {
        let Some(event_key) = self.store.get_cf(
            self.store.cf_sync_meta(),
            sync_meta_keys::REORG_RECONCILE_PENDING,
        )?
        else {
            return Ok(());
        };
        keys::decode_reorg_event_key(&event_key)?;
        let key = std::str::from_utf8(&event_key)?;
        let bytes = self
            .store
            .get_cf(self.store.cf_sync_meta(), &event_key)?
            .ok_or_else(|| anyhow!("pending reorg event is missing before rollback: key={key}"))?;
        let mut event: ReorgEvent = bincode::deserialize(&bytes)
            .with_context(|| format!("invalid pending reorg event before rollback: key={key}"))?;
        ensure!(
            event.kind == ReorgEventKind::Automatic,
            "pending reconciliation is not an automatic fork before rollback: key={key}"
        );
        event.reconcile_interrupted = true;
        let mut batch = StoreBatch::new(&self.store);
        batch.put_sync_meta(&event_key, &bincode::serialize(&event)?);
        batch.delete_sync_meta(sync_meta_keys::REORG_RECONCILE_PENDING);
        batch.commit()?;
        Ok(())
    }

    /// Reconcile the latest automatic switch after replay reaches its observed
    /// new-tip height and verifies the retained fork point. The observed tip
    /// may have been superseded before replay. A marker makes the normal live
    /// path one point read and survives a restart between rollback and replay.
    pub(crate) fn reconcile_reorg_events(&self) -> Result<()> {
        let Some(event_key) = self.store.get_cf(
            self.store.cf_sync_meta(),
            sync_meta_keys::REORG_RECONCILE_PENDING,
        )?
        else {
            return Ok(());
        };
        keys::decode_reorg_event_key(&event_key)?;
        let event_key_str = std::str::from_utf8(&event_key)?;
        let bytes = self
            .store
            .get_cf(self.store.cf_sync_meta(), &event_key)?
            .ok_or_else(|| anyhow!("pending reorg event is missing: key={event_key_str}"))?;
        let mut current: ReorgEvent = bincode::deserialize(&bytes)?;
        if current.kind != ReorgEventKind::Automatic {
            return Err(anyhow!(
                "pending reconciliation is not an automatic fork: key={event_key_str}"
            ));
        }
        let (tip, _) = self.store.get_sync_tip()?;
        if tip < current.new_tip {
            return Ok(());
        }
        let fork_header = self.store.get_block_header(current.fork_point)?
            .ok_or_else(|| anyhow!("canonical fork point header missing during fork reconciliation: event={event_key_str} block={} tip={tip} expected=0x{}", current.fork_point, hex::encode(&current.fork_point_hash)))?;
        ensure!(
            fork_header.hash == current.fork_point_hash,
            "fork point hash mismatch during fork reconciliation: event={event_key_str} block={} expected=0x{} actual=0x{}",
            current.fork_point,
            hex::encode(&current.fork_point_hash),
            hex::encode(&fork_header.hash),
        );
        let winning_header = self.store.get_block_header(current.new_tip)?
            .ok_or_else(|| anyhow!("canonical header missing during fork reconciliation: event={event_key_str} block={} tip={tip}", current.new_tip))?;
        if winning_header.hash != current.new_tip_hash {
            // The observed new tip was replaced before replay, but the fork
            // point and restored old tips still prove reversal links.
            info!(
                "Reorg observation superseded before reconciliation: event={} block={} observed=0x{} canonical=0x{}",
                event_key_str,
                current.new_tip,
                hex::encode(&current.new_tip_hash),
                hex::encode(&winning_header.hash),
            );
            if let Some(previous) = &current.superseded_by_tip_hash {
                ensure!(
                    previous == &winning_header.hash,
                    "superseded tip hash changed during fork reconciliation: event={event_key_str} block={} recorded=0x{} actual=0x{}",
                    current.new_tip,
                    hex::encode(previous),
                    hex::encode(&winning_header.hash),
                );
            }
            current.superseded_by_tip_hash = Some(winning_header.hash.clone());
        } else {
            ensure!(
                current.superseded_by_tip_hash.is_none(),
                "superseded tip hash exists but observed tip is canonical during fork reconciliation: event={event_key_str} block={} observed=0x{}",
                current.new_tip,
                hex::encode(&current.new_tip_hash),
            );
        }
        let mut batch = StoreBatch::new(&self.store);
        let mut complete = true;
        // Fork events are observations, intentionally retained across rollback.
        // Compare actual canonical ancestry, not equal-height tip guesses.
        for mut prior in self.store.list_reorg_events(usize::MAX, None)? {
            if prior.key == event_key_str
                || prior.event.kind != ReorgEventKind::Automatic
                || prior.event.reverted_by_event.is_some()
                || prior.event.old_tip <= current.fork_point
            {
                continue;
            }
            if prior.event.old_tip > tip {
                complete = false;
                continue;
            }
            let header = self.store.get_block_header(prior.event.old_tip)?
                .ok_or_else(|| anyhow!("canonical header missing while reconciling abandoned fork: event={} block={} tip={tip}", prior.key, prior.event.old_tip))?;
            if header.hash != prior.event.old_tip_hash {
                continue;
            }
            prior.event.reverted_by_event = Some(event_key_str.to_string());
            if !current.reverses_events.contains(&prior.key) {
                current.reverses_events.push(prior.key.clone());
            }
            batch.put_sync_meta(prior.key.as_bytes(), &bincode::serialize(&prior.event)?);
        }
        current.reverses_events.sort();
        let next_bytes = bincode::serialize(&current)?;
        if next_bytes != bytes {
            batch.put_sync_meta(&event_key, &next_bytes);
        }
        if complete {
            batch.delete_sync_meta(sync_meta_keys::REORG_RECONCILE_PENDING);
        }
        if !batch.is_empty() {
            batch.commit()?;
        }
        Ok(())
    }

    pub fn record_deep_fork(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        db_tip: i64,
        db_tip_hash: &[u8],
        chain_tip: i64,
        chain_tip_hash: &[u8],
        depth: i64,
    ) -> Result<()> {
        // Store the reorg event
        let depth_i32 = i32::try_from(depth)
            .map_err(|_| anyhow!("reorg depth exceeds i32 range: depth={}", depth))?;
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            kind: ReorgEventKind::Deep,
            fork_point,
            fork_point_hash: fork_hash.to_vec(),
            old_tip: db_tip,
            old_tip_hash: db_tip_hash.to_vec(),
            new_tip: chain_tip,
            new_tip_hash: chain_tip_hash.to_vec(),
            depth: depth_i32,
            // A deep fork pauses sync instead of rolling back, so nothing is
            // orphaned yet.
            orphaned_blocks: 0,
            orphaned_txs: 0,
            reverted_by_event: None,
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let event_key = next_reorg_event_key()?;
        let event_bytes = bincode::serialize(&event)?;
        let mut status = self.store.get_sync_status()?;
        status.deep_fork_detected = true;
        status.deep_fork_info = Some(DeepForkInfo {
            db_tip,
            db_tip_hash: db_tip_hash.to_vec(),
            chain_tip,
            chain_tip_hash: chain_tip_hash.to_vec(),
            depth: depth_i32,
            fork_point,
        });
        let status_bytes = bincode::serialize(&status)?;

        let mut batch = StoreBatch::new(self.store.as_ref());
        batch.put_sync_meta(event_key.as_bytes(), &event_bytes);
        batch.put_sync_meta(sync_meta_keys::SYNC_STATUS, &status_bytes);
        // The local node has already reported that this tip is no longer
        // canonical. Keep the API unavailable until an operator resolves the
        // deep fork instead of serving a stale summary under a canonical label.
        batch.delete_live_cell_summary_current()?;
        batch.commit()?;

        Ok(())
    }

    pub async fn execute_reorg(
        &self,
        append_store: &CkbadgerStore,
        fork_point: i64,
        fork_hash: &[u8],
        old_tip: i64,
        old_tip_hash: &[u8],
        new_tip: i64,
        new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        let depth = i32::try_from(old_tip - fork_point).map_err(|_| {
            anyhow!(
                "reorg depth exceeds i32 range: old_tip={}, fork_point={}",
                old_tip,
                fork_point
            )
        })?;

        // Validate the retained recovery source before undo-log replay. The
        // old summary stays verifiable by hash during this first phase;
        // rollback_to_block withdraws it before staging canonical cell changes.
        let summary_initialized = self.store.is_live_cell_summary_initialized()?;
        let current_summary = self.store.get_live_cell_summary()?;
        if summary_initialized && current_summary.is_none() {
            return Err(anyhow!(
                "initialized live-cell summary is missing before reorg: old_tip={} old_tip_hash=0x{}",
                old_tip,
                hex::encode(old_tip_hash),
            ));
        }
        if !summary_initialized && current_summary.is_some() {
            return Err(anyhow!(
                "live-cell summary current record exists without initialized marker before reorg"
            ));
        }
        if let Some(current) = current_summary {
            let history = self
                .store
                .get_live_cell_summary_at(current.tip_block_number)?
                .ok_or_else(|| {
                    anyhow!(
                        "missing live-cell summary history before reorg: block={} hash=0x{}",
                        current.tip_block_number,
                        hex::encode(current.tip_block_hash),
                    )
                })?;
            if history != current
                || current.tip_block_number != old_tip
                || current.tip_block_hash.as_slice() != old_tip_hash
            {
                return Err(anyhow!(
                    "live-cell summary does not match reorg source tip: current={:?} history={:?} old_tip={} old_tip_hash=0x{}",
                    current,
                    history,
                    old_tip,
                    hex::encode(old_tip_hash),
                ));
            }
        }
        // Entity daily/hourly stats are restored ONLY by the undo log, and the
        // undo log is pruned behind a coverage floor. A fork point below that
        // floor cannot be undone — fail with the numbers rather than replay a
        // rollback that would silently leave those buckets wrong.
        self.ensure_rollback_within_entity_stats_coverage(
            fork_point,
            ContractRequirement::Required,
            "reorg fork point",
        )?;

        // Revert domain mutations from undo-log first so that entity data
        // (Spore, mNFT, dotbit) is restored to pre-fork state before the
        // multi-stage rollback rebuilds aggregates from it.
        // Extract TxContext entries before the undo log deletes them, so the
        // subsequent cell rollback can use targeted lookups instead of full
        // CF scans.
        self.begin_rollback()?;
        let undo_result = self.store.rollback_via_undo_log(append_store, fork_point)?;
        // Domain rollback for canonical mutable state (cells, blocks, stats,
        // aggregates rebuilt from now-correct entity data).
        let rollback = self.store.rollback_to_block_with_tx_contexts(
            fork_point,
            Some(append_store),
            undo_result.tx_contexts,
        )?;
        // Flush both stores after the rollback. The original reason — avoiding
        // O(N log N) VectorRep sorts on the un-flushed memtable during the
        // refresh reads below — no longer applies: a live process is the only
        // one that reorgs, and it now always opens with a skiplist (P3.1).
        // The flush stays for durability and cross-store consistency: the
        // rolled-back tip of BOTH stores must be on disk before the refresh
        // reads re-derive aggregates from it and before an API secondary can
        // catch up to a half-rolled-back view.
        self.store.flush_all_memtables()?;
        append_store.flush_all_memtables()?;
        // Re-derive script version/family rollups from the corrected reference info.
        self.refresh_script_reference_rollups()?;
        // Advance the DAO singleton stats (latest stats + top depositors) to the
        // post-rollback tip. Rollback deliberately leaves the pre-rollback row
        // in place — it is never deleted — so the read path always has a value
        // and this refresh overwrites it rather than filling a gap.
        self.refresh_latest_dao_statistics()?;

        // Record reorg event and clear deep fork flag in one sync_meta batch.
        let orphaned_blocks = i64::try_from(rollback.blocks_removed).map_err(|_| {
            anyhow!(
                "rolled-back block count exceeds i64 range: blocks_removed={}",
                rollback.blocks_removed
            )
        })?;
        let orphaned_txs = i64::try_from(rollback.txs_removed).map_err(|_| {
            anyhow!(
                "rolled-back transaction count exceeds i64 range: txs_removed={}",
                rollback.txs_removed
            )
        })?;
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            kind: ReorgEventKind::Automatic,
            fork_point,
            fork_point_hash: fork_hash.to_vec(),
            old_tip,
            old_tip_hash: old_tip_hash.to_vec(),
            new_tip,
            new_tip_hash: new_tip_hash.to_vec(),
            depth,
            orphaned_blocks,
            orphaned_txs,
            reverted_by_event: None,
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let event_key = next_reorg_event_key()?;
        let event_bytes = bincode::serialize(&event)?;
        let mut status = self.store.get_sync_status()?;
        status.deep_fork_detected = false;
        status.deep_fork_info = None;
        let status_bytes = bincode::serialize(&status)?;
        let mut batch = StoreBatch::new(self.store.as_ref());
        batch.put_sync_meta(event_key.as_bytes(), &event_bytes);
        batch.put_sync_meta(
            sync_meta_keys::REORG_RECONCILE_PENDING,
            event_key.as_bytes(),
        );
        batch.put_sync_meta(sync_meta_keys::SYNC_STATUS, &status_bytes);
        batch.commit()?;

        // Update cache
        if let Some(cache) = &self.cache_invalidator {
            let hash_hex = format!("0x{}", hex::encode(fork_hash));
            cache
                .update_sync_status(|status| {
                    status.tip_block_number = fork_point;
                    status.tip_block_hash = hash_hex;
                    status.last_synced_at = Utc::now().timestamp();
                })
                .await;
        }

        info!(
            "Reorg completed: fork_point={}, depth={}, orphaned_blocks={}, orphaned_txs={}",
            fork_point, depth, orphaned_blocks, orphaned_txs
        );

        Ok(ReorgResult {
            depth,
            orphaned_blocks: i32::try_from(orphaned_blocks).map_err(|_| {
                anyhow!(
                    "rolled-back block count exceeds i32 range: orphaned_blocks={}",
                    orphaned_blocks
                )
            })?,
        })
    }
}

pub struct ReorgResult {
    pub depth: i32,
    pub orphaned_blocks: i32,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ckbadger_store::keys;
    use ckbadger_store::store::CkbadgerStore;
    use ckbadger_store::types::{LiveCellSummary, ReorgEventKind, SyncStatus};
    use ckbadger_store::StoreBatch;

    use crate::db::writer::BatchWriter;

    use super::next_reorg_event_key;

    #[test]
    fn superseded_winning_tip_clears_pending_observation_without_reversal_links() {
        use ckbadger_store::types::{CachedBlockHeader, ReorgEvent};
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), append);
        let key = keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
        let event = ReorgEvent {
            detected_at: 1,
            kind: ReorgEventKind::Automatic,
            fork_point: 9,
            fork_point_hash: vec![9; 32],
            old_tip: 10,
            old_tip_hash: vec![0xaa; 32],
            new_tip: 11,
            new_tip_hash: vec![0xbb; 32],
            depth: 1,
            orphaned_blocks: 1,
            orphaned_txs: 3,
            reverted_by_event: None,
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_sync_meta(key.as_bytes(), &bincode::serialize(&event).unwrap());
        batch.put_sync_meta(
            keys::sync_meta_keys::REORG_RECONCILE_PENDING,
            key.as_bytes(),
        );
        for (height, hash) in [(9, vec![9; 32]), (11, vec![0xcc; 32])] {
            batch.put_block_header(
                height,
                &CachedBlockHeader {
                    hash,
                    parent_hash: vec![9; 32],
                    timestamp: 0,
                    epoch_number: 1,
                    epoch_index: 0,
                    epoch_length: 100,
                    dao: vec![0; 32],
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                    chain_time_ms: 0,
                },
            );
        }
        batch.commit().unwrap();
        store.update_sync_tip(11, &[0xcc; 32], 0).unwrap();

        writer.reconcile_reorg_events().unwrap();

        assert!(store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_RECONCILE_PENDING
            )
            .unwrap()
            .is_none());
        let retained = store.get_reorg_event(1000).unwrap().unwrap().event;
        assert_eq!(retained.new_tip_hash, event.new_tip_hash);
        assert_eq!(retained.superseded_by_tip_hash, Some(vec![0xcc; 32]));
        assert!(retained.reverses_events.is_empty());
        assert!(retained.reverted_by_event.is_none());
        writer.reconcile_reorg_events().unwrap();
        let repeated = store.get_reorg_event(1000).unwrap().unwrap().event;
        assert_eq!(repeated.new_tip_hash, event.new_tip_hash);
        assert_eq!(
            repeated.superseded_by_tip_hash,
            retained.superseded_by_tip_hash
        );
    }

    #[test]
    fn superseded_observation_still_links_restored_old_tip() {
        use ckbadger_store::types::{CachedBlockHeader, ReorgEvent};
        // Each sequence starts at height 7. Derive fork points from actual
        // shared ancestry so the fixture cannot claim an impossible switch.
        fn observation(old: &[u8], new: &[u8], detected_at: i64) -> ReorgEvent {
            let shared = old.iter().zip(new).take_while(|(a, b)| a == b).count();
            assert!(shared > 0 && shared < old.len() && shared < new.len());
            ReorgEvent {
                detected_at,
                kind: ReorgEventKind::Automatic,
                fork_point: 7 + shared as i64 - 1,
                fork_point_hash: vec![old[shared - 1]; 32],
                old_tip: 7 + old.len() as i64 - 1,
                old_tip_hash: vec![*old.last().unwrap(); 32],
                new_tip: 7 + new.len() as i64 - 1,
                new_tip_hash: vec![*new.last().unwrap(); 32],
                depth: (old.len() - shared) as i32,
                orphaned_blocks: (old.len() - shared) as i64,
                orphaned_txs: 0,
                reverted_by_event: None,
                superseded_by_tip_hash: None,
                reconcile_interrupted: false,
                reverses_events: Vec::new(),
            }
        }
        let a = [7, 8, 0xa9, 0xa0];
        let b = [7, 8, 0xb9, 0xb0, 0xb1];
        let c = [7, 0xc8, 0xc9, 0xc0, 0xc1];
        let observed_a = [7, 8, 0xa9, 0xa0, 0xa1, 0xa2];
        let canonical_a = [7, 8, 0xa9, 0xa0, 0xd1, 0xd2];
        for deeper in [false, true] {
            let domain_dir = tempfile::tempdir().unwrap();
            let append_dir = tempfile::tempdir().unwrap();
            let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
            let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
            let writer = BatchWriter::new(store.clone(), append.clone());
            let first_key = keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
            let second_key = keys::encode_reorg_event_key(2000, &[2; 16]).unwrap();
            let first = observation(&a, &b, 1);
            let second = observation(if deeper { &c } else { &b }, &observed_a, 2);
            assert_eq!(first.fork_point, 8);
            assert_eq!(second.fork_point, if deeper { 7 } else { 8 });
            let mut batch = StoreBatch::new(&store);
            batch.put_sync_meta(first_key.as_bytes(), &bincode::serialize(&first).unwrap());
            if deeper {
                // A later switch B -> C at height 7 precedes C -> A. Without
                // this intervening branch, f1 < f0 while restoring 10a would
                // contradict the ancestry of the two original branches.
                let middle_key = keys::encode_reorg_event_key(1500, &[3; 16]).unwrap();
                batch.put_sync_meta(
                    middle_key.as_bytes(),
                    &bincode::serialize(&observation(&b, &c, 1)).unwrap(),
                );
            }
            batch.put_sync_meta(second_key.as_bytes(), &bincode::serialize(&second).unwrap());
            batch.put_sync_meta(
                keys::sync_meta_keys::REORG_RECONCILE_PENDING,
                second_key.as_bytes(),
            );
            let mut parent = vec![6; 32];
            for (index, hash) in canonical_a.iter().enumerate() {
                batch.put_block_header(
                    7 + index as i64,
                    &CachedBlockHeader {
                        hash: vec![*hash; 32],
                        parent_hash: parent,
                        timestamp: 0,
                        epoch_number: 1,
                        epoch_index: 0,
                        epoch_length: 100,
                        dao: vec![0; 32],
                        transactions_count: 1,
                        uncles_count: 0,
                        proposals_count: 0,
                        compact_target: 0,
                        miner_lock_hash: None,
                        cycles: None,
                        chain_time_ms: 0,
                    },
                );
                parent = vec![*hash; 32];
            }
            batch.commit().unwrap();
            store.update_sync_tip(12, &[0xd2; 32], 0).unwrap();
            let append_key = [7; 36];
            append
                .put_cf(append.cf_cells(), &append_key, b"immutable")
                .unwrap();
            writer.reconcile_reorg_events().unwrap();
            let first = store.get_reorg_event(1000).unwrap().unwrap().event;
            let second = store.get_reorg_event(2000).unwrap().unwrap().event;
            assert_eq!(first.reverted_by_event, Some(second_key));
            assert_eq!(second.reverses_events, vec![first_key]);
            assert_eq!(second.superseded_by_tip_hash, Some(vec![0xd2; 32]));
            assert_eq!(
                append
                    .get_cf(append.cf_cells(), &append_key)
                    .unwrap()
                    .unwrap(),
                b"immutable"
            );
            assert_eq!(
                append
                    .iterator_cf(append.cf_cells(), rocksdb::IteratorMode::Start)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn fork_point_mismatch_preserves_pending_reconciliation() {
        use ckbadger_store::types::{CachedBlockHeader, ReorgEvent};
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let writer = BatchWriter::new(
            store.clone(),
            Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap()),
        );
        let key = keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
        let event = ReorgEvent {
            detected_at: 1,
            kind: ReorgEventKind::Automatic,
            fork_point: 9,
            fork_point_hash: vec![9; 32],
            old_tip: 10,
            old_tip_hash: vec![0xaa; 32],
            new_tip: 11,
            new_tip_hash: vec![0xcc; 32],
            depth: 1,
            orphaned_blocks: 1,
            orphaned_txs: 0,
            reverted_by_event: None,
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let header = |hash| CachedBlockHeader {
            hash,
            parent_hash: vec![8; 32],
            timestamp: 0,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 100,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 0,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_sync_meta(key.as_bytes(), &bincode::serialize(&event).unwrap());
        batch.put_sync_meta(
            keys::sync_meta_keys::REORG_RECONCILE_PENDING,
            key.as_bytes(),
        );
        batch.put_block_header(9, &header(vec![0xee; 32]));
        batch.put_block_header(11, &header(vec![0xcc; 32]));
        batch.commit().unwrap();
        store.update_sync_tip(11, &[0xcc; 32], 0).unwrap();

        let error = writer.reconcile_reorg_events().unwrap_err();
        assert!(error.to_string().contains("fork point"), "{error:#}");
        assert!(store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_RECONCILE_PENDING
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn changed_superseding_hash_rejects_reconciliation_and_keeps_pending() {
        use ckbadger_store::types::{CachedBlockHeader, ReorgEvent};
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let writer = BatchWriter::new(
            store.clone(),
            Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap()),
        );
        let key = keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
        let prior_key = keys::encode_reorg_event_key(500, &[2; 16]).unwrap();
        let event = ReorgEvent {
            detected_at: 1,
            kind: ReorgEventKind::Automatic,
            fork_point: 9,
            fork_point_hash: vec![9; 32],
            old_tip: 10,
            old_tip_hash: vec![0xaa; 32],
            new_tip: 11,
            new_tip_hash: vec![0xbb; 32],
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            depth: 1,
            orphaned_blocks: 1,
            orphaned_txs: 0,
            reverted_by_event: None,
            reverses_events: Vec::new(),
        };
        let prior = ReorgEvent {
            old_tip: 12,
            ..event.clone()
        };
        let header = |hash| CachedBlockHeader {
            hash,
            parent_hash: vec![9; 32],
            timestamp: 0,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 100,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 0,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_sync_meta(key.as_bytes(), &bincode::serialize(&event).unwrap());
        batch.put_sync_meta(prior_key.as_bytes(), &bincode::serialize(&prior).unwrap());
        batch.put_sync_meta(
            keys::sync_meta_keys::REORG_RECONCILE_PENDING,
            key.as_bytes(),
        );
        batch.put_block_header(9, &header(vec![9; 32]));
        batch.put_block_header(11, &header(vec![0xcc; 32]));
        batch.commit().unwrap();
        store.update_sync_tip(11, &[0xcc; 32], 0).unwrap();

        writer.reconcile_reorg_events().unwrap();
        assert_eq!(
            store
                .get_reorg_event(1000)
                .unwrap()
                .unwrap()
                .event
                .superseded_by_tip_hash,
            Some(vec![0xcc; 32])
        );
        assert!(store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_RECONCILE_PENDING
            )
            .unwrap()
            .is_some());

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(11, &header(vec![0xdd; 32]));
        batch.commit().unwrap();
        let error = writer.reconcile_reorg_events().unwrap_err();
        assert!(
            error.to_string().contains("superseded tip hash changed"),
            "{error:#}"
        );
        assert!(store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_RECONCILE_PENDING
            )
            .unwrap()
            .is_some());
        assert_eq!(
            store
                .get_reorg_event(1000)
                .unwrap()
                .unwrap()
                .event
                .superseded_by_tip_hash,
            Some(vec![0xcc; 32])
        );
    }

    #[test]
    fn pending_reorg_still_rejects_missing_canonical_header() {
        use ckbadger_store::types::ReorgEvent;
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let writer = BatchWriter::new(
            store.clone(),
            Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap()),
        );
        let key = keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
        let event = ReorgEvent {
            detected_at: 1,
            kind: ReorgEventKind::Automatic,
            fork_point: 9,
            fork_point_hash: vec![9; 32],
            old_tip: 10,
            old_tip_hash: vec![0xaa; 32],
            new_tip: 11,
            new_tip_hash: vec![0xbb; 32],
            depth: 1,
            orphaned_blocks: 1,
            orphaned_txs: 3,
            reverted_by_event: None,
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_sync_meta(key.as_bytes(), &bincode::serialize(&event).unwrap());
        batch.put_sync_meta(
            keys::sync_meta_keys::REORG_RECONCILE_PENDING,
            key.as_bytes(),
        );
        batch.put_block_header(
            9,
            &ckbadger_store::types::CachedBlockHeader {
                hash: vec![9; 32],
                parent_hash: vec![8; 32],
                timestamp: 0,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 100,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: 0,
            },
        );
        batch.commit().unwrap();
        store.update_sync_tip(11, &[0xcc; 32], 0).unwrap();

        let error = writer.reconcile_reorg_events().unwrap_err();
        assert!(
            error.to_string().contains("canonical header missing"),
            "{error:#}"
        );
        assert!(store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_RECONCILE_PENDING
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn switchback_links_reversal_after_canonical_replay_with_extended_tip() {
        use ckbadger_store::types::{CachedBlockHeader, ReorgEvent};
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), append.clone());
        let first_key = keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
        let reversal_key = keys::encode_reorg_event_key(2000, &[2; 16]).unwrap();
        let unrelated_key = keys::encode_reorg_event_key(500, &[3; 16]).unwrap();
        let deep_key = keys::encode_reorg_event_key(750, &[4; 16]).unwrap();
        let first = ReorgEvent {
            detected_at: 1,
            kind: ReorgEventKind::Automatic,
            fork_point: 9,
            fork_point_hash: vec![9; 32],
            old_tip: 10,
            old_tip_hash: vec![0xaa; 32],
            new_tip: 11,
            new_tip_hash: vec![0xbb; 32],
            depth: 1,
            orphaned_blocks: 1,
            orphaned_txs: 3,
            reverted_by_event: None,
            superseded_by_tip_hash: None,
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let reversal = ReorgEvent {
            detected_at: 2,
            old_tip: 11,
            old_tip_hash: vec![0xbb; 32],
            new_tip: 12,
            new_tip_hash: vec![0xcc; 32],
            depth: 2,
            ..first.clone()
        };
        let unrelated = ReorgEvent {
            old_tip_hash: vec![0xdd; 32],
            ..first.clone()
        };
        let deep = ReorgEvent {
            kind: ReorgEventKind::Deep,
            ..first.clone()
        };
        let mut batch = StoreBatch::new(&store);
        for (key, event) in [
            (&first_key, &first),
            (&reversal_key, &reversal),
            (&unrelated_key, &unrelated),
            (&deep_key, &deep),
        ] {
            batch.put_sync_meta(key.as_bytes(), &bincode::serialize(event).unwrap());
        }
        batch.put_sync_meta(b"reorg_reconcile_pending", reversal_key.as_bytes());
        for (height, hash) in [(9, vec![9; 32]), (10, vec![0xaa; 32]), (12, vec![0xcc; 32])] {
            batch.put_block_header(
                height,
                &CachedBlockHeader {
                    hash,
                    parent_hash: vec![9; 32],
                    timestamp: 0,
                    epoch_number: 1,
                    epoch_index: 0,
                    epoch_length: 100,
                    dao: vec![0; 32],
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                    chain_time_ms: 0,
                },
            );
        }
        batch.commit().unwrap();
        store.update_sync_tip(11, &[0xbb; 32], 0).unwrap();
        writer.reconcile_reorg_events().unwrap();
        assert!(store
            .get_reorg_event(1000)
            .unwrap()
            .unwrap()
            .event
            .reverted_by_event
            .is_none());
        assert!(store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_RECONCILE_PENDING
            )
            .unwrap()
            .is_some());
        store.update_sync_tip(12, &[0xcc; 32], 0).unwrap();
        let immutable_key = [7; 36];
        append
            .put_cf(append.cf_cells(), &immutable_key, b"immutable cell payload")
            .unwrap();
        writer.reconcile_reorg_events().unwrap();
        let first_after = store.get_reorg_event(1000).unwrap().unwrap();
        let reversal_after = store.get_reorg_event(2000).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&first_after.event).unwrap()["reverted_by_event"],
            reversal_key
        );
        assert_eq!(
            serde_json::to_value(&reversal_after.event).unwrap()["reverses_events"],
            serde_json::json!([first_key])
        );
        assert!(store
            .get_reorg_event(750)
            .unwrap()
            .unwrap()
            .event
            .reverted_by_event
            .is_none());
        assert_eq!(first_after.event.old_tip_hash, first.old_tip_hash);
        assert_eq!(first_after.event.orphaned_txs, 3);
        assert!(
            serde_json::to_value(store.get_reorg_event(500).unwrap().unwrap().event).unwrap()
                ["reverted_by_event"]
                .is_null()
        );
        assert!(store
            .get_cf(store.cf_sync_meta(), b"reorg_reconcile_pending")
            .unwrap()
            .is_none());
        writer.reconcile_reorg_events().unwrap();
        assert_eq!(
            serde_json::to_value(store.get_reorg_event(2000).unwrap().unwrap().event).unwrap()
                ["reverses_events"],
            serde_json::json!([first_key])
        );
        assert_eq!(
            append
                .get_cf(append.cf_cells(), &immutable_key)
                .unwrap()
                .unwrap(),
            b"immutable cell payload"
        );
    }

    fn reorg_event_count(store: &CkbadgerStore) -> usize {
        store
            .iterator_cf(store.cf_sync_meta(), rocksdb::IteratorMode::Start)
            .flatten()
            .filter(|(key, _)| key.starts_with(keys::REORG_EVENT_KEY_PREFIX))
            .count()
    }

    #[test]
    fn test_next_reorg_event_key_is_unique() {
        let first = next_reorg_event_key().unwrap();
        let second = next_reorg_event_key().unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with("reorg:"));
        assert!(second.starts_with("reorg:"));
    }

    #[test]
    fn test_next_reorg_event_key_contains_uuid_segment() {
        let key = next_reorg_event_key().unwrap();
        assert!(key.starts_with("reorg:"));
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "expected format reorg:timestamp:uuid");
        assert_eq!(
            parts[1].len(),
            keys::REORG_EVENT_MS_DIGITS,
            "millisecond field must be fixed width so key order is chronological"
        );
        assert!(parts[1].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(parts[2].len(), 32, "UUID segment should be 32 hex chars");
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Fixed-width millisecond fields mean lexicographic key order is
    /// chronological order, which is what the history listing pages on.
    #[test]
    fn test_reorg_event_keys_sort_chronologically() {
        let early = keys::encode_reorg_event_key(999_999_999_999, &[0xFF; 16]).unwrap();
        let late = keys::encode_reorg_event_key(1_700_000_000_000, &[0x00; 16]).unwrap();
        assert!(early < late, "{early} should sort before {late}");
    }

    #[test]
    fn test_next_reorg_event_key_no_collision_across_simulated_restarts() {
        let key1 = next_reorg_event_key().unwrap();
        let key2 = next_reorg_event_key().unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_record_deep_fork_writes_event_and_sync_status_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let summary = LiveCellSummary {
            tip_block_number: 120,
            tip_block_hash: [0x11; 32],
            dao: 1,
            typed_non_dao: 0,
            plain: 2,
            data_bearing: 1,
        };
        let mut seed = StoreBatch::new(store.as_ref());
        seed.put_live_cell_summary_snapshots(&[summary]).unwrap();
        seed.commit().unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 120,
                tip_block_hash: vec![0x11; 32],
                total_cells_created: 4,
                total_cells_consumed: 1,
                ..Default::default()
            })
            .unwrap();

        writer
            .record_deep_fork(100, &[0x33; 32], 120, &[0x11; 32], 130, &[0x22; 32], 20)
            .unwrap();

        assert_eq!(reorg_event_count(store.as_ref()), 1);
        let latest = store
            .get_latest_reorg_event()
            .unwrap()
            .expect("latest reorg event should exist");
        assert_eq!(latest.event.kind, ReorgEventKind::Deep);
        assert!(!latest.event.reconcile_interrupted);
        assert_eq!(latest.event.fork_point, 100);
        assert_eq!(latest.event.fork_point_hash, vec![0x33; 32]);
        assert_eq!(latest.event.old_tip, 120);
        assert_eq!(latest.event.old_tip_hash, vec![0x11; 32]);
        assert_eq!(latest.event.new_tip, 130);
        assert_eq!(latest.event.new_tip_hash, vec![0x22; 32]);
        assert_eq!(latest.event.depth, 20);
        assert_eq!(latest.event.orphaned_blocks, 0);

        let status = store.get_sync_status().unwrap();
        assert!(status.deep_fork_detected);
        let info = status.deep_fork_info.unwrap();
        assert_eq!(info.fork_point, 100);
        assert_eq!(info.db_tip, 120);
        assert_eq!(info.chain_tip, 130);
        assert_eq!(info.depth, 20);
        assert_eq!(store.get_live_cell_summary().unwrap(), None);
        assert_eq!(store.get_live_cell_summary_at(120).unwrap(), Some(summary));
    }

    #[tokio::test]
    async fn test_execute_reorg_does_not_persist_event_when_rollback_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let key = keys::encode_block_num(1);
        store
            .put_cf(store.cf_block_headers(), &key, b"invalid-header-payload")
            .unwrap();

        let result = writer
            .execute_reorg(store.as_ref(), 0, &[0xAA; 32], 1, &[], 1, &[])
            .await;
        assert!(result.is_err());
        assert_eq!(reorg_event_count(store.as_ref()), 0);
    }
}
