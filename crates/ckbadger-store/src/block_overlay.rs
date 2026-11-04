//! The block journal's write side: one overlay per live batch over every
//! `Journal`-class domain column family (`CfSpec::rollback_class`).
//!
//! A live batch spans many blocks and a shallow fork can land on any of them,
//! so rolling back to block N must restore every key to its value at the end
//! of block N. The overlay keeps, for every key the batch touches, its value
//! as of the end of the last block that wrote it, and records a journal entry
//! `KeyMutation { cf, key, previous }` the first time a block writes the key:
//! the value at the end of the previous block (`None` when the key did not
//! exist). Three properties, each pinned by a test below (they are the ones
//! `EntityStatsOverlay` established for the entity stats buckets):
//!
//! - **Per block, not per batch.** The pre-image of block N is the value at
//!   the end of block N-1, which may itself have been written by this batch.
//! - **Once per (block, key).** A key written three times in one block gets
//!   one entry, holding its pre-block value.
//! - **One write per key.** Every key is staged into the commit once, with its
//!   final value, by [`BlockOverlay::finish`].
//!
//! Blocks must reach a key in ascending order; a write for an older block than
//! the key's last one is an error, never a silently wrong pre-image.
//!
//! Pre-images of keys the batch did not touch before are the committed values;
//! they are read in one `multi_get` per column family when the batch finishes
//! (or earlier, cached, when a writer asks for [`BlockOverlay::current`]).
//!
//! Bulk sync never journals (`docs/prompts/BULK_SYNC.md` rule 12): an overlay
//! opened on a store in bulk mode stages final values and records nothing.
//!
//! Store boundary: domain store only. `cells` is `RollbackClass::Never` and
//! never reaches an overlay.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use rocksdb::WriteBatch;

use crate::keys::{self, UndoSeqScope, UNDO_SEQ_LOCAL_MAX};
use crate::store::{cf_spec, CkbadgerStore, RollbackClass};
use crate::types::{UndoLogEntry, UndoLogStoreTarget};

type CfKey = (&'static str, Vec<u8>);

/// A key's state inside the batch.
#[derive(Debug, Clone)]
struct KeyState {
    /// The block whose end this value is; `None` for a write without a block
    /// (see [`BlockOverlay::write`]).
    last_block: Option<i64>,
    value: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
enum Previous {
    /// The committed value, resolved when the batch finishes.
    Committed,
    Known(Option<Vec<u8>>),
}

#[derive(Debug)]
struct PendingEntry {
    block: i64,
    cf: &'static str,
    key: Vec<u8>,
    previous: Previous,
}

/// What [`BlockOverlay::finish`] staged.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JournalStats {
    /// Keys staged into the commit (each once).
    pub keys_written: u64,
    /// Journal entries staged.
    pub entries: u64,
    /// Encoded bytes of the staged journal rows (keys + values).
    pub bytes: u64,
}

/// One live batch's overlay over the `Journal`-class domain CFs.
#[derive(Debug, Default)]
pub struct BlockOverlay {
    journaling: bool,
    keys: HashMap<CfKey, KeyState>,
    /// Committed values read so far (pre-batch), by `current` or `prefetch`.
    committed: HashMap<CfKey, Option<Vec<u8>>>,
    entries: Vec<PendingEntry>,
    /// Keys written without a block: their journal is incomplete, so no
    /// entry is staged for them. Allowed only while the writers are being
    /// moved onto block scopes (store refactor lane B, B-3).
    unattributed: HashSet<CfKey>,
    unattributed_cfs: BTreeSet<&'static str>,
    /// Which sub-batch first wrote each key, to catch a key written from two
    /// sub-batches whose merge order could disagree with program order.
    origin: HashMap<CfKey, u32>,
    finished: bool,
}

impl BlockOverlay {
    /// An overlay for one batch on `store`. On a store in bulk-sync mode it
    /// records no journal (bulk never rolls back).
    pub fn new(store: &CkbadgerStore) -> Self {
        Self {
            journaling: !store.is_bulk_sync_mode(),
            ..Self::default()
        }
    }

    pub fn is_journaling(&self) -> bool {
        self.journaling
    }

    /// The column family's spec must be `Journal`; anything else reaching the
    /// overlay is a routing bug.
    fn journal_cf(cf: &str) -> Result<&'static str> {
        let spec =
            cf_spec(cf).ok_or_else(|| anyhow!("block overlay: unknown column family {cf}"))?;
        if spec.rollback_class != RollbackClass::Journal {
            bail!(
                "block overlay: column family {cf} is {:?}, not Journal",
                spec.rollback_class
            );
        }
        Ok(spec.name)
    }

    /// The current value of `key`: this batch's latest write, else the
    /// committed value (read once, cached).
    pub fn current(
        &mut self,
        store: &CkbadgerStore,
        cf: &str,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let cf = Self::journal_cf(cf)?;
        let cf_key = (cf, key.to_vec());
        if let Some(state) = self.keys.get(&cf_key) {
            return Ok(state.value.clone());
        }
        if let Some(committed) = self.committed.get(&cf_key) {
            return Ok(committed.clone());
        }
        let loaded = store
            .get_cf(store.cf(cf), key)
            .map_err(|e| anyhow!("block overlay read cf={cf} key=0x{}: {e}", hex::encode(key)))?;
        self.committed.insert(cf_key, loaded.clone());
        Ok(loaded)
    }

    /// Warm the committed values of `keys` with one `multi_get`. A key this
    /// batch already wrote keeps its newer value.
    pub fn prefetch(&mut self, store: &CkbadgerStore, cf: &str, keys: &[Vec<u8>]) -> Result<()> {
        let cf = Self::journal_cf(cf)?;
        let missing: Vec<&Vec<u8>> = keys
            .iter()
            .filter(|key| {
                let cf_key = (cf, (*key).clone());
                !self.keys.contains_key(&cf_key) && !self.committed.contains_key(&cf_key)
            })
            .collect();
        self.read_committed(store, cf, &missing)
    }

    fn read_committed(
        &mut self,
        store: &CkbadgerStore,
        cf: &'static str,
        keys: &[&Vec<u8>],
    ) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let handle = store.cf(cf);
        let results = store.multi_get_cf(keys.iter().map(|key| (handle, key.as_slice())).collect());
        for (key, result) in keys.iter().zip(results) {
            let value = result.map_err(|e| {
                anyhow!(
                    "block overlay prefetch cf={cf} key=0x{}: {e}",
                    hex::encode(key)
                )
            })?;
            self.committed.insert((cf, (*key).clone()), value);
        }
        Ok(())
    }

    /// Set `key` to `value` (`None` deletes it) as part of `block`.
    ///
    /// `block = None` is a write no block scope claimed: the key is staged
    /// but carries no journal entry in this batch (transitional, B-3).
    /// `origin` names the sub-batch the write came from.
    pub fn write(
        &mut self,
        block: Option<i64>,
        cf: &str,
        key: &[u8],
        value: Option<Vec<u8>>,
        origin: u32,
    ) -> Result<()> {
        if self.finished {
            bail!(
                "block overlay: write after finish: cf={cf} key=0x{}",
                hex::encode(key)
            );
        }
        let cf = Self::journal_cf(cf)?;
        let cf_key = (cf, key.to_vec());
        match self.origin.get(&cf_key) {
            Some(first) if *first != origin => bail!(
                "block overlay: cf={cf} key=0x{} is written from two sub-batches ({first} and {origin}); \
                 their merge order would decide the committed value",
                hex::encode(key)
            ),
            Some(_) => {}
            None => {
                self.origin.insert(cf_key.clone(), origin);
            }
        }
        let Some(block) = block else {
            // Transitional diagnostic (B-3): name the first unscoped write.
            if std::env::var_os("CKBADGER_JOURNAL_STRICT").is_some() {
                bail!(
                    "block overlay: write without a block scope: cf={cf} key=0x{}\n{}",
                    hex::encode(key),
                    std::backtrace::Backtrace::force_capture()
                );
            }
            self.unattributed_cfs.insert(cf);
            self.unattributed.insert(cf_key.clone());
            let state = self.keys.entry(cf_key).or_insert(KeyState {
                last_block: None,
                value: None,
            });
            state.value = value;
            return Ok(());
        };
        match self.keys.get_mut(&cf_key) {
            None => {
                if self.journaling {
                    let previous = match self.committed.get(&cf_key) {
                        Some(committed) => Previous::Known(committed.clone()),
                        None => Previous::Committed,
                    };
                    self.entries.push(PendingEntry {
                        block,
                        cf,
                        key: key.to_vec(),
                        previous,
                    });
                }
                self.keys.insert(
                    cf_key,
                    KeyState {
                        last_block: Some(block),
                        value,
                    },
                );
            }
            Some(state) => {
                match state.last_block {
                    Some(last) if block < last => bail!(
                        "block overlay requires ascending block order per key: cf={cf} key=0x{} \
                         last_block={last} block={block}",
                        hex::encode(key)
                    ),
                    Some(last) if block == last => {}
                    _ => {
                        if self.journaling && !self.unattributed.contains(&cf_key) {
                            self.entries.push(PendingEntry {
                                block,
                                cf,
                                key: key.to_vec(),
                                previous: Previous::Known(state.value.clone()),
                            });
                        }
                        state.last_block = Some(block);
                    }
                }
                state.value = value;
            }
        }
        Ok(())
    }

    /// Column families that received a write without a block scope in this
    /// batch (transitional, B-3).
    pub fn unattributed_cfs(&self) -> &BTreeSet<&'static str> {
        &self.unattributed_cfs
    }

    /// Keys this batch has written (each is staged once).
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Stage every key once with its final value, and every journal entry
    /// under `UndoSeqScope::Journal`, into `batch` (a domain-store batch).
    /// `next_seq` hands out the block-local sequence numbers shared with the
    /// batch's other undo writers.
    pub fn finish(
        &mut self,
        store: &CkbadgerStore,
        batch: &mut WriteBatch,
        next_seq: &mut dyn FnMut(i64) -> Result<u64>,
    ) -> Result<JournalStats> {
        if self.finished {
            bail!("block overlay finished twice");
        }
        // Committed pre-images still unknown, one multi_get per CF.
        let mut unresolved: HashMap<&'static str, Vec<Vec<u8>>> = HashMap::new();
        for entry in &self.entries {
            if matches!(entry.previous, Previous::Committed)
                && !self.committed.contains_key(&(entry.cf, entry.key.clone()))
            {
                unresolved
                    .entry(entry.cf)
                    .or_default()
                    .push(entry.key.clone());
            }
        }
        for (cf, keys) in unresolved {
            let refs: Vec<&Vec<u8>> = keys.iter().collect();
            self.read_committed(store, cf, &refs)?;
        }

        // Transitional diagnostic (B-3): list the unscoped writes by CF and
        // first key byte.
        if let Some(path) = std::env::var_os("CKBADGER_JOURNAL_UNATTRIBUTED_LOG") {
            use std::io::Write;
            let mut seen: BTreeSet<(&str, u8)> = BTreeSet::new();
            for (cf, key) in &self.unattributed {
                seen.insert((cf, key.first().copied().unwrap_or(0)));
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            for (cf, first) in seen {
                writeln!(file, "{cf} {first:#04x}")?;
            }
        }
        let mut stats = JournalStats::default();
        let mut cf_keys: Vec<(&CfKey, &KeyState)> = self.keys.iter().collect();
        cf_keys.sort_by(|a, b| a.0.cmp(b.0));
        for ((cf, key), state) in cf_keys {
            let handle = store.cf(cf);
            match &state.value {
                Some(value) => batch.put_cf(handle, key, value),
                None => batch.delete_cf(handle, key),
            }
            stats.keys_written += 1;
        }

        let undo_cf = store.cf_reorg_undo_log_by_block();
        for entry in &self.entries {
            if self.unattributed.contains(&(entry.cf, entry.key.clone())) {
                continue;
            }
            let previous_value = match &entry.previous {
                Previous::Known(previous) => previous.clone(),
                Previous::Committed => self
                    .committed
                    .get(&(entry.cf, entry.key.clone()))
                    .cloned()
                    .ok_or_else(|| {
                        anyhow!(
                            "block overlay: committed pre-image not resolved: cf={} key=0x{}",
                            entry.cf,
                            hex::encode(&entry.key)
                        )
                    })?,
            };
            let seq = next_seq(entry.block)?;
            let row_key = keys::encode_reorg_undo_log_key(entry.block, seq);
            let row_value = bincode::serialize(&UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: entry.cf.to_string(),
                key: entry.key.clone(),
                previous_value,
            })?;
            stats.entries += 1;
            stats.bytes += (row_key.len() + row_value.len()) as u64;
            batch.put_cf(undo_cf, row_key, &row_value);
        }
        self.finished = true;
        Ok(stats)
    }
}

/// The one [`BlockOverlay`] of a live batch, shared by every sub-batch and
/// writer of it. `Arc<Mutex<_>>`: the batch write future must stay `Send`.
#[derive(Clone, Debug, Default)]
pub struct SharedBlockOverlay(Arc<Mutex<BlockOverlay>>);

impl SharedBlockOverlay {
    pub fn new(store: &CkbadgerStore) -> Self {
        Self(Arc::new(Mutex::new(BlockOverlay::new(store))))
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut BlockOverlay) -> R) -> R {
        let mut guard = self.0.lock().expect("block overlay mutex poisoned");
        f(&mut guard)
    }
}

/// The sequence-number allocator `finish` needs, over a block-local counter
/// map shared with the batch's other undo writers.
pub fn journal_seq(counters: &mut HashMap<i64, u64>, block: i64) -> Result<u64> {
    let local = counters.entry(block).or_insert(0);
    if *local > UNDO_SEQ_LOCAL_MAX {
        bail!("undo seq local counter overflow: block={block}");
    }
    let seq = UndoSeqScope::Journal.seq_base() | *local;
    *local += 1;
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::decode_reorg_undo_log_key;
    use crate::store::{
        CF_ADDR_BALANCE, CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS, CF_LIVE_CELLS, CF_SYNC_META,
    };
    use crate::StoreBatch;
    use tempfile::TempDir;

    fn store() -> (CkbadgerStore, TempDir) {
        let dir = TempDir::new().unwrap();
        (CkbadgerStore::open_domain(dir.path()).unwrap(), dir)
    }

    /// Every journal row of `store`, as `(block, cf, key, previous)` in key
    /// order.
    type JournalRow = (i64, String, Vec<u8>, Option<Vec<u8>>);

    fn journal_rows(store: &CkbadgerStore) -> Vec<JournalRow> {
        store
            .iterator_cf(
                store.cf_reorg_undo_log_by_block(),
                rocksdb::IteratorMode::Start,
            )
            .map(|row| {
                let (key, value) = row.unwrap();
                let (block, seq) = decode_reorg_undo_log_key(&key);
                assert!(UndoSeqScope::Journal.owns(seq));
                match bincode::deserialize::<UndoLogEntry>(&value).unwrap() {
                    UndoLogEntry::KeyMutation {
                        cf_name,
                        key,
                        previous_value,
                        ..
                    } => (block, cf_name, key, previous_value),
                    other => panic!("unexpected journal row {other:?}"),
                }
            })
            .collect()
    }

    fn commit(store: &CkbadgerStore, overlay: &SharedBlockOverlay) -> JournalStats {
        let mut batch = StoreBatch::journaled(store, overlay, 0);
        let mut counters = HashMap::new();
        let stats = batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();
        stats
    }

    fn put(overlay: &SharedBlockOverlay, block: i64, key: &[u8], value: &[u8]) {
        overlay
            .with(|o| o.write(Some(block), CF_ADDR_BALANCE, key, Some(value.to_vec()), 0))
            .unwrap();
    }

    /// The pre-image of block N is the value at the end of block N-1 — which
    /// this very batch may have written — and a block writes a key's entry
    /// once however often it writes the key.
    #[test]
    fn pre_images_are_per_block_and_once_per_block_key() {
        let (store, _dir) = store();
        store.put_cf(store.cf_addr_balance(), b"k", b"v0").unwrap();
        let overlay = SharedBlockOverlay::new(&store);
        put(&overlay, 10, b"k", b"v10a");
        put(&overlay, 10, b"k", b"v10b");
        put(&overlay, 12, b"k", b"v12");
        put(&overlay, 15, b"k", b"v15a");
        put(&overlay, 15, b"k", b"v15b");
        put(&overlay, 12, b"new", b"n12");
        let stats = commit(&store, &overlay);

        assert_eq!(
            journal_rows(&store),
            vec![
                (
                    10,
                    CF_ADDR_BALANCE.into(),
                    b"k".to_vec(),
                    Some(b"v0".to_vec())
                ),
                (
                    12,
                    CF_ADDR_BALANCE.into(),
                    b"k".to_vec(),
                    Some(b"v10b".to_vec())
                ),
                (12, CF_ADDR_BALANCE.into(), b"new".to_vec(), None),
                (
                    15,
                    CF_ADDR_BALANCE.into(),
                    b"k".to_vec(),
                    Some(b"v12".to_vec())
                ),
            ]
        );
        assert_eq!(stats.entries, 4);
        assert_eq!(stats.keys_written, 2, "each key is staged once");
        assert_eq!(
            store.get_cf(store.cf_addr_balance(), b"k").unwrap(),
            Some(b"v15b".to_vec())
        );
    }

    /// A block older than the key's last one would record a pre-image from
    /// the future.
    #[test]
    fn a_descending_block_for_a_key_is_an_error() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store);
        put(&overlay, 12, b"k", b"a");
        let err = overlay
            .with(|o| o.write(Some(11), CF_ADDR_BALANCE, b"k", Some(b"b".to_vec()), 0))
            .unwrap_err();
        assert!(err.to_string().contains("ascending"), "{err:#}");
    }

    /// A delete records the value it removes; a key created and deleted in
    /// one batch still journals both blocks.
    #[test]
    fn deletes_and_creations_journal_their_previous_values() {
        let (store, _dir) = store();
        store.put_cf(store.cf_live_cells(), b"old", b"m").unwrap();
        let overlay = SharedBlockOverlay::new(&store);
        overlay
            .with(|o| o.write(Some(5), CF_LIVE_CELLS, b"old", None, 0))
            .unwrap();
        overlay
            .with(|o| o.write(Some(5), CF_LIVE_CELLS, b"fresh", Some(b"m5".to_vec()), 0))
            .unwrap();
        overlay
            .with(|o| o.write(Some(7), CF_LIVE_CELLS, b"fresh", None, 0))
            .unwrap();
        commit(&store, &overlay);
        assert_eq!(
            journal_rows(&store),
            vec![
                (
                    5,
                    CF_LIVE_CELLS.into(),
                    b"old".to_vec(),
                    Some(b"m".to_vec())
                ),
                (5, CF_LIVE_CELLS.into(), b"fresh".to_vec(), None),
                (
                    7,
                    CF_LIVE_CELLS.into(),
                    b"fresh".to_vec(),
                    Some(b"m5".to_vec())
                ),
            ]
        );
        assert_eq!(store.get_cf(store.cf_live_cells(), b"old").unwrap(), None);
        assert_eq!(store.get_cf(store.cf_live_cells(), b"fresh").unwrap(), None);
    }

    /// Only `Journal`-class CFs reach the overlay.
    #[test]
    fn a_non_journal_cf_is_refused() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store);
        let err = overlay
            .with(|o| o.write(Some(1), CF_BLOCK_HEADERS, b"k", Some(vec![1]), 0))
            .unwrap_err();
        assert!(err.to_string().contains("RangeByBlock"), "{err:#}");
    }

    /// Bulk sync records no journal (BULK_SYNC.md rule 12): a bulk-mode
    /// write stages its values and zero journal rows.
    #[test]
    fn a_bulk_mode_write_produces_zero_journal_rows() {
        let (store, _dir) = store();
        store.set_bulk_sync_mode(true);
        let overlay = SharedBlockOverlay::new(&store);
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        batch.at(3).put_sync_meta(b"key", b"value");
        let mut counters = HashMap::new();
        let stats = batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(stats.entries, 0);
        assert!(journal_rows(&store).is_empty());
        assert_eq!(
            store.get_cf(store.cf_sync_meta(), b"key").unwrap(),
            Some(b"value".to_vec())
        );
    }

    /// A journaled StoreBatch sends Journal-class writes through the overlay
    /// and every other CF straight to its WriteBatch; it refuses to commit
    /// before the overlay is staged.
    #[test]
    fn a_journaled_batch_routes_by_rollback_class_and_must_be_staged() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store);
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        batch.at(4).put_sync_meta(b"meta", b"x");
        batch.at(4).put_block_header(
            4,
            &crate::types::CachedBlockHeader {
                hash: vec![4; 32],
                parent_hash: vec![3; 32],
                timestamp: 0,
                epoch_number: 0,
                epoch_index: 4,
                epoch_length: 10,
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
        let err = batch.commit().unwrap_err();
        assert!(err.to_string().contains("stage_journal"), "{err:#}");

        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        let mut counters = HashMap::new();
        batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            journal_rows(&store),
            vec![
                (4, CF_SYNC_META.into(), b"meta".to_vec(), None),
                (4, CF_BLOCK_HASH_INDEX.into(), vec![4; 32], None),
            ],
            "the header itself (RangeByBlock) is not journaled; its hash index is"
        );
        assert!(
            store.get_block_header(4).unwrap().is_none(),
            "the first batch never committed"
        );
    }

    /// A key two sub-batches write would take its committed value from their
    /// merge order, not from the order of the writes.
    #[test]
    fn a_key_written_from_two_sub_batches_fails_the_commit() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store);
        let mut first = StoreBatch::journaled(&store, &overlay, 1);
        let mut second = StoreBatch::journaled(&store, &overlay, 2);
        first.at(1).put_sync_meta(b"k", b"a");
        second.at(1).put_sync_meta(b"k", b"b");
        first.merge_from(second);
        let mut counters = HashMap::new();
        let err = first
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap_err();
        assert!(err.to_string().contains("two sub-batches"), "{err:#}");
    }

    /// A write no block scope claimed is staged but journals nothing, and
    /// its CF is reported (transitional, B-3).
    #[test]
    fn an_unattributed_write_is_staged_without_journal_entries() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store);
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        batch.put_sync_meta(b"k", b"v");
        let mut counters = HashMap::new();
        batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();
        assert!(journal_rows(&store).is_empty());
        assert_eq!(
            store.get_cf(store.cf_sync_meta(), b"k").unwrap(),
            Some(b"v".to_vec())
        );
        assert_eq!(
            overlay.with(|o| o.unattributed_cfs().iter().copied().collect::<Vec<_>>()),
            vec![CF_SYNC_META]
        );
    }
}
