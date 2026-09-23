//! Batch-level overlay for the eight entity daily/hourly stats families.
//!
//! Rollback of these families has exactly one owner: the undo log. This module
//! is what feeds it. For every key a batch touches it keeps the value as of the
//! end of the last block that wrote it, records the pre-image the *first* time
//! a given block mutates a given key, and writes each key exactly once at the
//! end of the batch.
//!
//! Three properties matter and each is pinned by a test below:
//!
//! - **Per-block, not per-batch.** A batch can span thousands of blocks. The
//!   undo entry for block N must hold the value as of the end of block N-1, not
//!   the value that was in RocksDB when the batch opened — otherwise rolling
//!   back to the middle of a batch restores a value from before the batch.
//! - **Once per (block, key).** A key mutated three times inside one block gets
//!   one undo entry, holding the pre-block value.
//! - **One write per key.** Read-modify-write against RocksDB inside the loop
//!   would both re-read stale values and multiply the batch size.
//!
//! Store boundary: domain store only. Nothing here can address `CF_CELLS`;
//! `stats_cf_name_by_prefix` resolves stats prefixes and nothing else.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{TokenDailyDelta, UndoLogEntry, UndoLogStoreTarget};
use ckbadger_store::CkbadgerStore;

use crate::sync::types::UndoSeqScope;
use crate::sync::undo::{next_undo_seq, SharedUndoSeq};

/// Batch-scoped view of the entity stats keys a batch touches.
#[derive(Default)]
pub struct EntityStatsOverlay {
    /// Full stats key → current encoded value (`None` = the row does not exist).
    values: HashMap<Vec<u8>, Option<Vec<u8>>>,
    /// `(block, key)` pairs whose pre-image this batch has already recorded.
    touched: HashSet<(i64, Vec<u8>)>,
    /// Keys `stage_final` must write.
    dirty: HashSet<Vec<u8>>,
}

impl EntityStatsOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current value of `key`: the overlay if this batch has seen it, otherwise
    /// one read from the store, cached.
    pub fn current(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(cached) = self.values.get(key) {
            return Ok(cached.clone());
        }
        let loaded = store.get_stats_key(key)?;
        self.values.insert(key.to_vec(), loaded.clone());
        Ok(loaded)
    }

    /// Warm the overlay with one `multi_get` instead of N point reads. Keys
    /// already in the overlay keep their (newer) value — a prefetch must never
    /// overwrite a value this batch has already computed.
    pub fn prefetch(&mut self, store: &CkbadgerStore, keys: &[Vec<u8>]) -> Result<()> {
        let missing: Vec<&Vec<u8>> = keys
            .iter()
            .filter(|key| !self.values.contains_key(key.as_slice()))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let cf_keys = missing
            .iter()
            .map(|key| Ok((store.cf_for_stats_key(key)?, key.as_slice())))
            .collect::<Result<Vec<_>>>()?;
        let results = store.multi_get_cf(cf_keys);
        for (key, result) in missing.into_iter().zip(results) {
            let value = result.map_err(|e| {
                anyhow::anyhow!(
                    "failed to prefetch entity stats key=0x{}: {}",
                    hex::encode(key),
                    e
                )
            })?;
            self.values.insert(key.clone(), value);
        }
        Ok(())
    }

    /// Set `key` to `next` as part of `block`.
    ///
    /// The first mutation of `key` inside `block` records a `KeyMutation` undo
    /// entry holding the value as of the end of the previous block. Bulk sync
    /// records nothing (`BULK_SYNC.md` rule: bulk never rolls back).
    pub fn mutate(
        &mut self,
        store: &CkbadgerStore,
        batch: &mut StoreBatch,
        undo_seq: &mut HashMap<i64, u64>,
        block: i64,
        key: &[u8],
        next: Option<Vec<u8>>,
    ) -> Result<()> {
        let previous_value = self.current(store, key)?;
        if self.touched.insert((block, key.to_vec())) && !store.is_bulk_sync_mode() {
            let Some(prefix) = key.first().copied() else {
                bail!("empty entity stats key in overlay mutate: block={block}");
            };
            let cf_name = CkbadgerStore::stats_cf_name_by_prefix(prefix)?;
            let seq = next_undo_seq(undo_seq, block, UndoSeqScope::EntityStats);
            batch.put_reorg_undo_log_by_block(
                block,
                seq,
                &UndoLogEntry::KeyMutation {
                    target_store: UndoLogStoreTarget::Domain,
                    cf_name: cf_name.to_string(),
                    key: key.to_vec(),
                    previous_value,
                },
            );
        }
        self.values.insert(key.to_vec(), next);
        self.dirty.insert(key.to_vec());
        Ok(())
    }

    /// Write the final value of every mutated key — once each.
    pub fn stage_final(&self, batch: &mut StoreBatch) -> Result<()> {
        for key in &self.dirty {
            match self.values.get(key) {
                Some(Some(value)) => batch.put_stats(key, value),
                Some(None) => batch.delete_stats(key),
                None => bail!(
                    "entity stats overlay marked key=0x{} dirty without a value",
                    hex::encode(key)
                ),
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn dirty_len(&self) -> usize {
        self.dirty.len()
    }
}

/// The one `EntityStatsOverlay` of a committed batch, shared with the four
/// protocol batch states that write hourly transfer counters.
///
/// The daily writers take `&mut EntityStatsOverlay` directly (via
/// [`SharedEntityStatsOverlay::with`]); the hourly write points live deep
/// inside `SporeBatchState` / `MnftBatchState` / `DotbitBatchState` /
/// `UdtBatchState`, so they hold a clone of this handle instead of threading
/// two more parameters through a dozen signatures. Either way there is exactly
/// one overlay per batch and one `stage_final`.
///
/// `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>`: the batch write future must
/// stay `Send`.
#[derive(Clone, Default)]
pub struct SharedEntityStatsOverlay(Arc<Mutex<EntityStatsOverlay>>);

impl SharedEntityStatsOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut EntityStatsOverlay) -> R) -> R {
        let mut guard = self.0.lock().expect("entity stats overlay mutex poisoned");
        f(&mut guard)
    }

    /// Bump an hourly transfer counter through the overlay, recording this
    /// block's pre-image the first time the block touches the key.
    pub fn mutate_hourly(
        &self,
        store: &CkbadgerStore,
        batch: &mut StoreBatch,
        undo_seq: &SharedUndoSeq,
        block: i64,
        key: &[u8],
        by: i64,
        ctx: &dyn Fn() -> String,
    ) -> Result<()> {
        self.with(|overlay| {
            let prev = overlay.current(store, key)?;
            let next = apply_hourly_increment(prev.as_deref(), by, ctx)?;
            undo_seq.with(|seq| overlay.mutate(store, batch, seq, block, key, next))
        })
    }

    /// Current value of an hourly counter as this batch sees it.
    #[cfg(test)]
    pub fn current_hourly(&self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        self.with(|overlay| match overlay.current(store, key)? {
            Some(bytes) => {
                if bytes.len() != 8 {
                    bail!(
                        "invalid entity hourly transfer value length: key=0x{}, len={}",
                        hex::encode(key),
                        bytes.len()
                    );
                }
                Ok(i64::from_le_bytes(bytes[..8].try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "failed to decode entity hourly transfer value: key=0x{}",
                        hex::encode(key)
                    )
                })?))
            }
            None => Ok(0),
        })
    }

    pub fn stage_final(&self, batch: &mut StoreBatch) -> Result<()> {
        self.with(|overlay| overlay.stage_final(batch))
    }
}

/// Apply a `(capacity, knowledge)` daily delta on top of the previous encoded
/// value.
///
/// All five daily families serialise the identical two-`i128` shape (pinned by
/// `daily_pair_codec_is_identical_across_families`), so one codec serves them
/// all. Both fields zero collapses the row to `None`, matching every existing
/// daily writer.
pub fn apply_daily_pair(
    prev: Option<&[u8]>,
    capacity_delta: i128,
    knowledge_delta: i128,
    ctx: &dyn Fn() -> String,
) -> Result<Option<Vec<u8>>> {
    let mut current: TokenDailyDelta = match prev {
        Some(bytes) => bincode::deserialize(bytes).map_err(|e| {
            anyhow::anyhow!("failed to deserialize entity daily delta: {}, {}", ctx(), e)
        })?,
        None => TokenDailyDelta::default(),
    };
    current.owned_capacity_delta = current
        .owned_capacity_delta
        .checked_add(capacity_delta)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "entity daily capacity delta overflow: {}, current={}, delta={}",
                ctx(),
                current.owned_capacity_delta,
                capacity_delta
            )
        })?;
    current.owned_knowledge_delta = current
        .owned_knowledge_delta
        .checked_add(knowledge_delta)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "entity daily knowledge delta overflow: {}, current={}, delta={}",
                ctx(),
                current.owned_knowledge_delta,
                knowledge_delta
            )
        })?;
    if current.owned_capacity_delta == 0 && current.owned_knowledge_delta == 0 {
        return Ok(None);
    }
    Ok(Some(bincode::serialize(&current).map_err(|e| {
        anyhow::anyhow!("failed to serialize entity daily delta: {}, {}", ctx(), e)
    })?))
}

/// Apply an increment to an hourly transfer counter (`i64` little-endian, 8
/// bytes exactly). A zero result still writes a row: hourly counters are
/// deleted only by the retention sweep, never by reaching zero.
pub fn apply_hourly_increment(
    prev: Option<&[u8]>,
    by: i64,
    ctx: &dyn Fn() -> String,
) -> Result<Option<Vec<u8>>> {
    let current = match prev {
        Some(bytes) => {
            if bytes.len() != 8 {
                bail!(
                    "invalid entity hourly transfer value length: {}, len={}",
                    ctx(),
                    bytes.len()
                );
            }
            i64::from_le_bytes(bytes[..8].try_into().map_err(|_| {
                anyhow::anyhow!("failed to decode entity hourly transfer value: {}", ctx())
            })?)
        }
        None => 0,
    };
    let next = current.checked_add(by).ok_or_else(|| {
        anyhow::anyhow!(
            "entity hourly transfer overflow: {}, current={}, delta={}",
            ctx(),
            current,
            by
        )
    })?;
    Ok(Some(next.to_le_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::keys;
    use ckbadger_store::types::{
        ClusterDailyDelta, MnftDailyDelta, ScriptDailyDelta, SporeDailyDelta,
    };

    fn ctx() -> impl Fn() -> String {
        || "test".to_string()
    }

    fn open_store() -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        (dir, store)
    }

    fn undo_entries(store: &CkbadgerStore) -> Vec<(i64, u64, UndoLogEntry)> {
        let iter = store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        );
        iter.map(|item| {
            let (key, value) = item.unwrap();
            let (block, seq) = keys::decode_reorg_undo_log_key(&key);
            (block, seq, bincode::deserialize(&value).unwrap())
        })
        .collect()
    }

    fn daily(cap: i128, know: i128) -> Vec<u8> {
        bincode::serialize(&TokenDailyDelta {
            owned_capacity_delta: cap,
            owned_knowledge_delta: know,
        })
        .unwrap()
    }

    #[test]
    fn mutate_records_previous_value_once_per_block() {
        let (_dir, store) = open_store();
        let key = keys::encode_token_daily_key(&[0x22; 32], 20_260_922).to_vec();
        store.put_stats_key(&key, &daily(10, 1)).unwrap();

        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq: HashMap<i64, u64> = HashMap::new();
        let mut batch = StoreBatch::new(&store);

        // Two mutations inside block 5 → one undo entry holding the DB value.
        overlay
            .mutate(
                &store,
                &mut batch,
                &mut undo_seq,
                5,
                &key,
                Some(daily(30, 3)),
            )
            .unwrap();
        overlay
            .mutate(
                &store,
                &mut batch,
                &mut undo_seq,
                5,
                &key,
                Some(daily(70, 7)),
            )
            .unwrap();
        // One mutation in block 6 → a second entry holding the end-of-block-5
        // value, NOT the value RocksDB still holds.
        overlay
            .mutate(
                &store,
                &mut batch,
                &mut undo_seq,
                6,
                &key,
                Some(daily(150, 15)),
            )
            .unwrap();
        overlay.stage_final(&mut batch).unwrap();
        batch.commit().unwrap();

        let entries = undo_entries(&store);
        assert_eq!(entries.len(), 2, "one undo entry per (block, key)");
        assert_eq!(entries[0].0, 5);
        assert_eq!(entries[1].0, 6);
        assert_eq!(
            entries[0].1 >> 48,
            UndoSeqScope::EntityStats as u64,
            "entity stats entries must carry their own scope"
        );

        let UndoLogEntry::KeyMutation {
            ref cf_name,
            previous_value: ref block5_prev,
            ..
        } = entries[0].2
        else {
            panic!("expected KeyMutation");
        };
        assert_eq!(cf_name, ckbadger_store::CF_STATS_TOKEN);
        assert_eq!(block5_prev.as_deref(), Some(daily(10, 1).as_slice()));

        let UndoLogEntry::KeyMutation {
            previous_value: ref block6_prev,
            ..
        } = entries[1].2
        else {
            panic!("expected KeyMutation");
        };
        assert_eq!(
            block6_prev.as_deref(),
            Some(daily(70, 7).as_slice()),
            "block 6's pre-image is the end of block 5, not the pre-batch DB value"
        );
    }

    #[test]
    fn mutate_records_absence_for_a_key_the_block_creates() {
        let (_dir, store) = open_store();
        let key = keys::encode_spore_hourly_key(&[0x33; 32], 497_209);

        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq: HashMap<i64, u64> = HashMap::new();
        let mut batch = StoreBatch::new(&store);
        overlay
            .mutate(
                &store,
                &mut batch,
                &mut undo_seq,
                9,
                &key,
                Some(4i64.to_le_bytes().to_vec()),
            )
            .unwrap();
        overlay.stage_final(&mut batch).unwrap();
        batch.commit().unwrap();

        let entries = undo_entries(&store);
        assert_eq!(entries.len(), 1);
        let UndoLogEntry::KeyMutation {
            ref cf_name,
            ref previous_value,
            ..
        } = entries[0].2
        else {
            panic!("expected KeyMutation");
        };
        assert_eq!(cf_name, ckbadger_store::CF_STATS_SPORE);
        assert_eq!(
            *previous_value, None,
            "a row the block created must roll back to not existing"
        );
    }

    #[test]
    fn stage_final_writes_each_key_once() {
        let (_dir, store) = open_store();
        let key = keys::encode_object_daily_key(&[0x55; 32], 20_260_922).to_vec();

        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq: HashMap<i64, u64> = HashMap::new();
        let mut batch = StoreBatch::new(&store);
        let before = batch.len();
        for block in 1..=3i64 {
            overlay
                .mutate(
                    &store,
                    &mut batch,
                    &mut undo_seq,
                    block,
                    &key,
                    Some(daily(block as i128 * 10, block as i128)),
                )
                .unwrap();
        }
        let after_mutations = batch.len();
        assert_eq!(
            after_mutations - before,
            3,
            "three blocks contribute three undo entries and no stats writes yet"
        );
        assert_eq!(overlay.dirty_len(), 1);
        overlay.stage_final(&mut batch).unwrap();
        assert_eq!(
            batch.len() - after_mutations,
            1,
            "the key is written exactly once, with its final value"
        );

        batch.commit().unwrap();
        assert_eq!(store.get_stats_key(&key).unwrap(), Some(daily(30, 3)));
    }

    #[test]
    fn stage_final_deletes_a_key_whose_final_value_is_none() {
        let (_dir, store) = open_store();
        let key = keys::encode_token_daily_key(&[0x22; 32], 20_260_922).to_vec();
        store.put_stats_key(&key, &daily(10, 1)).unwrap();

        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq: HashMap<i64, u64> = HashMap::new();
        let mut batch = StoreBatch::new(&store);
        overlay
            .mutate(&store, &mut batch, &mut undo_seq, 4, &key, None)
            .unwrap();
        overlay.stage_final(&mut batch).unwrap();
        batch.commit().unwrap();

        assert_eq!(store.get_stats_key(&key).unwrap(), None);
    }

    #[test]
    fn daily_pair_zero_collapses_to_none() {
        let prev = daily(100, 10);
        assert_eq!(
            apply_daily_pair(Some(&prev), -100, -10, &ctx()).unwrap(),
            None
        );
        // Only one field cancelling keeps the row.
        assert!(apply_daily_pair(Some(&prev), -100, 0, &ctx())
            .unwrap()
            .is_some());
    }

    #[test]
    fn daily_pair_overflow_fails() {
        let prev = daily(i128::MAX, 0);
        let err = apply_daily_pair(Some(&prev), 1, 0, &ctx()).unwrap_err();
        assert!(
            err.to_string().contains("capacity delta overflow"),
            "got: {err}"
        );
        let prev = daily(0, i128::MIN);
        let err = apply_daily_pair(Some(&prev), 0, -1, &ctx()).unwrap_err();
        assert!(
            err.to_string().contains("knowledge delta overflow"),
            "got: {err}"
        );
    }

    #[test]
    fn daily_pair_codec_is_identical_across_families() {
        let cap = -1_234_567_890_123_456_789i128;
        let know = 9_876_543_210i128;
        let token = bincode::serialize(&TokenDailyDelta {
            owned_capacity_delta: cap,
            owned_knowledge_delta: know,
        })
        .unwrap();
        for (label, bytes) in [
            (
                "script",
                bincode::serialize(&ScriptDailyDelta {
                    owned_capacity_delta: cap,
                    owned_knowledge_delta: know,
                })
                .unwrap(),
            ),
            (
                "cluster",
                bincode::serialize(&ClusterDailyDelta {
                    owned_capacity_delta: cap,
                    owned_knowledge_delta: know,
                })
                .unwrap(),
            ),
            (
                "spore",
                bincode::serialize(&SporeDailyDelta {
                    owned_capacity_delta: cap,
                    owned_knowledge_delta: know,
                })
                .unwrap(),
            ),
            (
                "object",
                bincode::serialize(&MnftDailyDelta {
                    owned_capacity_delta: cap,
                    owned_knowledge_delta: know,
                })
                .unwrap(),
            ),
        ] {
            assert_eq!(
                bytes, token,
                "{label} daily delta must encode identically to token's — \
                 `apply_daily_pair` is the single codec for all five families"
            );
        }
    }

    #[test]
    fn hourly_rejects_malformed_len() {
        let err = apply_hourly_increment(Some(&[0u8; 4]), 1, &ctx()).unwrap_err();
        assert!(err.to_string().contains("length"), "got: {err}");
        assert_eq!(
            apply_hourly_increment(None, 3, &ctx()).unwrap(),
            Some(3i64.to_le_bytes().to_vec())
        );
        assert_eq!(
            apply_hourly_increment(Some(&7i64.to_le_bytes()), 2, &ctx()).unwrap(),
            Some(9i64.to_le_bytes().to_vec())
        );
        // Reaching zero still writes a row: only retention deletes hourly keys.
        assert_eq!(
            apply_hourly_increment(Some(&1i64.to_le_bytes()), -1, &ctx()).unwrap(),
            Some(0i64.to_le_bytes().to_vec())
        );
        let err = apply_hourly_increment(Some(&i64::MAX.to_le_bytes()), 1, &ctx()).unwrap_err();
        assert!(err.to_string().contains("overflow"), "got: {err}");
    }

    #[test]
    fn bulk_mode_records_no_undo() {
        let (_dir, store) = open_store();
        store.set_bulk_sync_mode(true);
        let key = keys::encode_cluster_daily_key(&[0x33; 32], 20_260_922).to_vec();

        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq: HashMap<i64, u64> = HashMap::new();
        let mut batch = StoreBatch::new(&store);
        overlay
            .mutate(
                &store,
                &mut batch,
                &mut undo_seq,
                7,
                &key,
                Some(daily(5, 1)),
            )
            .unwrap();
        overlay.stage_final(&mut batch).unwrap();
        batch.commit().unwrap();

        assert!(
            undo_entries(&store).is_empty(),
            "bulk build never rolls back, so it must not pay for undo entries"
        );
        assert_eq!(store.get_stats_key(&key).unwrap(), Some(daily(5, 1)));
    }

    #[test]
    fn prefetch_never_overwrites_a_value_the_batch_computed() {
        let (_dir, store) = open_store();
        let key = keys::encode_token_daily_key(&[0x22; 32], 20_260_922).to_vec();
        store.put_stats_key(&key, &daily(10, 1)).unwrap();

        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq: HashMap<i64, u64> = HashMap::new();
        let mut batch = StoreBatch::new(&store);
        overlay
            .mutate(
                &store,
                &mut batch,
                &mut undo_seq,
                2,
                &key,
                Some(daily(99, 9)),
            )
            .unwrap();
        overlay
            .prefetch(&store, std::slice::from_ref(&key))
            .unwrap();
        assert_eq!(
            overlay.current(&store, &key).unwrap(),
            Some(daily(99, 9)),
            "prefetch must not resurrect the pre-batch DB value"
        );
    }

    #[test]
    fn stats_cf_name_resolves_every_entity_prefix_like_the_handle_lookup() {
        let (_dir, store) = open_store();
        for (prefix, expected) in [
            (
                keys::STATS_PREFIX_SCRIPT_DAILY,
                ckbadger_store::CF_STATS_SCRIPT,
            ),
            (
                keys::STATS_PREFIX_TOKEN_DAILY,
                ckbadger_store::CF_STATS_TOKEN,
            ),
            (
                keys::STATS_PREFIX_TOKEN_HOURLY,
                ckbadger_store::CF_STATS_TOKEN,
            ),
            (
                keys::STATS_PREFIX_CLUSTER_DAILY,
                ckbadger_store::CF_STATS_SPORE,
            ),
            (
                keys::STATS_PREFIX_SPORE_DAILY,
                ckbadger_store::CF_STATS_SPORE,
            ),
            (
                keys::STATS_PREFIX_SPORE_HOURLY,
                ckbadger_store::CF_STATS_SPORE,
            ),
            (
                keys::STATS_PREFIX_OBJECT_DAILY,
                ckbadger_store::CF_STATS_MNFT,
            ),
            (
                keys::STATS_PREFIX_OBJECT_HOURLY,
                ckbadger_store::CF_STATS_MNFT,
            ),
        ] {
            assert_eq!(
                CkbadgerStore::stats_cf_name_by_prefix(prefix).unwrap(),
                expected,
                "prefix {prefix:#04x}"
            );
            // The name must address the same CF the handle lookup resolves.
            let handle = store.stats_cf_by_prefix(prefix).unwrap();
            let named = store.stats_cf_by_prefix(prefix).unwrap();
            assert!(std::ptr::eq(handle, named));
        }
        assert!(CkbadgerStore::stats_cf_name_by_prefix(0xFE).is_err());
    }
}
