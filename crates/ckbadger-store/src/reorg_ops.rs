//! Rollback: the block journal (`docs/prompts/REORG_HANDLING.md`).
//!
//! Every domain column family has one `RollbackClass` (`CF_SPECS`):
//! - `Journal`: a live batch journals, the first time block N writes a key,
//!   the key's value at the end of block N-1 (`block_overlay::BlockOverlay`);
//!   rollback writes those values back;
//! - `RangeByBlock`: the key opens with the block number (`activities`: the
//!   block number descending); rollback deletes the blocks' key ranges;
//! - `Never`: not rolled back.
//!
//! [`CkbadgerStore::rollback_to_block`] takes the blocks from the highest
//! stored header down to `target + 1` and stages, into ONE atomic write, the
//! range deletes of their `RangeByBlock` rows and their journal entries'
//! previous values, block by block from the top, each block's entries in
//! reverse sequence order. There is no other stage: nothing is reversed from
//! deleted rows, rebuilt from survivors, or found by scanning a column family.
//!
//! Every row of `reorg_undo_log_by_block` is a journal row
//! (`UndoSeqScope::Journal`); a row of any other scope is an error.
//!
//! Store boundary: domain store only. `cells` (append-only) is `Never`.

use rocksdb::{Direction, IteratorMode, WriteBatch};
use tracing::info;

use crate::batch::StoreBatch;
use crate::block_overlay::{decode_journal_value, is_outside_journal};
use crate::bytes_to_hex;
use crate::keys::{self, UndoSeqScope};
use crate::store::{
    cf_spec, CkbadgerStore, RollbackClass, CF_LIVE_CELLS, CF_REORG_UNDO_LOG_BY_BLOCK, CF_SPECS,
    CF_TX_ACTIONS,
};

/// What one rollback removed and restored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RollbackResult {
    /// Block headers deleted (the rolled-back blocks that had a header).
    pub blocks_removed: u64,
    /// `tx_index` rows deleted.
    pub txs_removed: u64,
    /// `live_cells` journal entries without a previous value: a live marker a
    /// rolled-back block created, deleted.
    pub cells_removed: u64,
    /// `live_cells` journal entries with a previous value: a live marker a
    /// rolled-back block consumed, written back.
    pub cells_restored: u64,
    /// Journal entries written back.
    pub journal_entries: u64,
}

/// One block journal entry: a `Journal`-class key and its value at the end of
/// the previous block (`None`: the key did not exist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub block: i64,
    pub seq: u64,
    pub cf: &'static str,
    pub key: Vec<u8>,
    pub previous: Option<Vec<u8>>,
}

/// One row of `reorg_undo_log_by_block`, decoded as the journal entry it
/// holds. A malformed key, a row of a scope other than `Journal`, an
/// undecodable journal row, or a journal row that names a column family that
/// is not `Journal`-class is an error.
fn decode_undo_row(key: &[u8], value: &[u8]) -> anyhow::Result<JournalEntry> {
    if key.len() != keys::REORG_UNDO_LOG_KEY_SIZE {
        anyhow::bail!(
            "journal: undo-log key of {} bytes (expected {}): 0x{}",
            key.len(),
            keys::REORG_UNDO_LOG_KEY_SIZE,
            bytes_to_hex(key)
        );
    }
    let (block, seq) = keys::decode_reorg_undo_log_key(key);
    if !UndoSeqScope::Journal.owns(seq) {
        anyhow::bail!(
            "journal: undo-log row at block {block} seq {seq:#x} is not a journal row (scope bits {:#06x}); \
             the per-writer undo scopes no longer exist",
            seq >> keys::UNDO_SEQ_SCOPE_SHIFT
        );
    }
    let entry = decode_journal_value(value).map_err(|e| {
        anyhow::anyhow!("journal: undecodable entry at block {block} seq {seq:#x}: {e}")
    })?;
    let cf_name = entry.cf;
    let spec = cf_spec(cf_name).ok_or_else(|| {
        anyhow::anyhow!(
            "journal: entry at block {block} seq {seq:#x} names unknown CF {cf_name} (key 0x{})",
            bytes_to_hex(entry.key)
        )
    })?;
    if spec.rollback_class != RollbackClass::Journal {
        anyhow::bail!(
            "journal: entry at block {block} seq {seq:#x} names CF {cf_name}, which is {:?}, not Journal (key 0x{})",
            spec.rollback_class,
            bytes_to_hex(entry.key)
        );
    }
    if is_outside_journal(cf_name, entry.key) {
        anyhow::bail!(
            "journal: entry at block {block} seq {seq:#x} restores cf={cf_name} key 0x{}, which is written outside the journal",
            bytes_to_hex(entry.key)
        );
    }
    Ok(JournalEntry {
        block,
        seq,
        cf: spec.name,
        key: entry.key.to_vec(),
        previous: entry.previous.map(<[u8]>::to_vec),
    })
}

/// The `activities` key range of blocks `first..=last`: its keys open with
/// `i64::MAX - block` big-endian, so the highest block's rows come first and
/// the range ends right after the lowest block's (for block 0 the end is
/// `2^63`, one past `i64::MAX`).
fn activities_range(first: i64, last: i64) -> ([u8; 8], [u8; 8]) {
    let start = keys::encode_desc_block_num(last);
    let end = ((i64::MAX - first) as u64 + 1).to_be_bytes();
    (start, end)
}

impl CkbadgerStore {
    /// The highest block with a stored header (`-1` on an empty store).
    fn highest_header_block(&self) -> anyhow::Result<i64> {
        match self
            .iterator_cf(self.cf_block_headers(), IteratorMode::End)
            .next()
        {
            None => Ok(-1),
            Some(item) => {
                let (key, _) =
                    item.map_err(|e| anyhow::anyhow!("rollback: reading the last header: {e}"))?;
                if key.len() != keys::BLOCK_NUM_KEY_SIZE {
                    anyhow::bail!(
                        "rollback: block header key of {} bytes: 0x{}",
                        key.len(),
                        bytes_to_hex(&key)
                    );
                }
                Ok(keys::decode_block_num(&key))
            }
        }
    }

    /// Every journal entry of blocks `first..`, in key order (ascending block,
    /// ascending sequence).
    fn journal_entries_from(&self, first: i64) -> anyhow::Result<Vec<JournalEntry>> {
        let start = keys::encode_block_num(first);
        let mut entries = Vec::new();
        for item in self.iterator_cf(
            self.cf_reorg_undo_log_by_block(),
            IteratorMode::From(&start, Direction::Forward),
        ) {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("journal: reading blocks from {first}: {e}"))?;
            entries.push(decode_undo_row(&key, &value)?);
        }
        Ok(entries)
    }

    /// The journal entries of `block`, in sequence order.
    pub fn journal_entries(&self, block: i64) -> anyhow::Result<Vec<JournalEntry>> {
        let start = keys::encode_block_num(block);
        let mut entries = Vec::new();
        for item in self.iterator_cf(
            self.cf_reorg_undo_log_by_block(),
            IteratorMode::From(&start, Direction::Forward),
        ) {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("journal: reading block {block}: {e}"))?;
            if key.len() == keys::REORG_UNDO_LOG_KEY_SIZE
                && keys::decode_reorg_undo_log_key(&key).0 != block
            {
                break;
            }
            entries.push(decode_undo_row(&key, &value)?);
        }
        Ok(entries)
    }

    /// Whether `reorg_undo_log_by_block` holds a row of a block above `block`.
    pub fn has_journal_after(&self, block: i64) -> anyhow::Result<bool> {
        if block < -1 {
            anyhow::bail!("journal: invalid probe block {block} (expected >= -1)");
        }
        let start = keys::encode_block_num(block + 1);
        match self
            .iterator_cf(
                self.cf_reorg_undo_log_by_block(),
                IteratorMode::From(&start, Direction::Forward),
            )
            .next()
        {
            None => Ok(false),
            Some(item) => {
                let (key, _) =
                    item.map_err(|e| anyhow::anyhow!("journal: probing above block {block}: {e}"))?;
                if key.len() != keys::REORG_UNDO_LOG_KEY_SIZE {
                    anyhow::bail!(
                        "journal: undo-log key of {} bytes (expected {}) above block {block}: 0x{}",
                        key.len(),
                        keys::REORG_UNDO_LOG_KEY_SIZE,
                        bytes_to_hex(&key)
                    );
                }
                Ok(true)
            }
        }
    }

    /// Stage into `batch` the deletion of every journal row of blocks
    /// `from_block + 1 ..= to_block` (they fall behind the coverage floor
    /// `to_block`, below which no rollback reaches): one range delete.
    /// Nothing is staged when the floor does not move up.
    pub fn prune_journal_below(
        &self,
        batch: &mut StoreBatch,
        from_block: i64,
        to_block: i64,
    ) -> anyhow::Result<()> {
        if from_block < -1 {
            anyhow::bail!("journal prune: invalid lower floor {from_block} (expected >= -1)");
        }
        if to_block <= from_block {
            return Ok(());
        }
        batch.delete_range_raw_cf_by_name(
            CF_REORG_UNDO_LOG_BY_BLOCK,
            &keys::encode_block_num(from_block + 1),
            &keys::encode_block_num(to_block + 1),
        )
    }

    /// Stage into `batch` the deletion of every journal entry above
    /// `rollback_to` that restores a row of one of `cf_names`: the CFs of
    /// derived aggregates a rollback resets (their rows are cleared in the
    /// same write, so their previous values must not be written back).
    /// Returns the number of entries staged for deletion.
    pub fn stage_delete_journal_above(
        &self,
        batch: &mut StoreBatch,
        cf_names: &[&str],
        rollback_to: i64,
    ) -> anyhow::Result<u64> {
        if rollback_to < -1 {
            anyhow::bail!("journal: invalid rollback target {rollback_to} (expected >= -1)");
        }
        if cf_names.is_empty() {
            return Ok(0);
        }
        let mut deleted = 0;
        for entry in self.journal_entries_from(rollback_to + 1)? {
            if cf_names.contains(&entry.cf) {
                batch.delete_reorg_undo_log_key(&keys::encode_reorg_undo_log_key(
                    entry.block,
                    entry.seq,
                ));
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Refuse a `RangeByBlock` row of a block above `tip` (the highest
    /// header): no rollback deletes it, and no live write puts it there.
    fn ensure_no_range_rows_above(&self, target: i64, tip: i64) -> anyhow::Result<()> {
        for spec in CF_SPECS
            .iter()
            .filter(|spec| spec.rollback_class == RollbackClass::RangeByBlock)
        {
            let cf = self.cf(spec.name);
            let row = if spec.name == CF_TX_ACTIONS {
                // Block-descending keys: the first row is the highest block.
                self.iterator_cf(cf, IteratorMode::Start).next()
            } else {
                let above = keys::encode_block_num(tip + 1);
                self.iterator_cf(cf, IteratorMode::From(&above, Direction::Forward))
                    .next()
            };
            let Some(row) = row else {
                continue;
            };
            let (key, _) = row
                .map_err(|e| anyhow::anyhow!("rollback to {target}: reading {}: {e}", spec.name))?;
            if key.len() < keys::BLOCK_NUM_KEY_SIZE {
                anyhow::bail!(
                    "rollback to {target}: {} key of {} bytes: 0x{}",
                    spec.name,
                    key.len(),
                    bytes_to_hex(&key)
                );
            }
            let block = if spec.name == CF_TX_ACTIONS {
                keys::decode_stored_tx_actions_key(&key)
                    .map_err(|e| anyhow::anyhow!("rollback to {target}: {e:#}"))?
                    .0
            } else {
                keys::decode_block_num(&key)
            };
            if block > tip {
                anyhow::bail!(
                    "rollback to {target}: {} row of block {block} above the tip {tip}: 0x{}",
                    spec.name,
                    bytes_to_hex(&key)
                );
            }
        }
        Ok(())
    }

    /// Roll the domain store back to the end of block `target` (`-1`: before
    /// genesis), in one atomic write: the `RangeByBlock` rows of blocks
    /// `target + 1 ..= tip` (tip = the highest stored header) are range
    /// deleted, and their journal entries written back from the top block
    /// down, each block's entries in reverse sequence order, so the last write
    /// of a key is its value at the end of `target`.
    ///
    /// The core proves its own preconditions, and on any failure writes
    /// nothing: a target in `-1 ..= tip`; no `RangeByBlock` row above the tip
    /// (checked even when `target == tip`, which is when startup cleanup
    /// runs); a target not below the journal coverage floor; and at least one
    /// journal row for every block it removes (each live block journals at
    /// least its `block_hash_index` creation, so a block without one was
    /// pruned or never journaled). Every journal row must be a decodable
    /// `Journal`-scope row naming a `Journal`-class CF.
    pub fn rollback_to_block(&self, target: i64) -> anyhow::Result<RollbackResult> {
        if target < -1 {
            anyhow::bail!("rollback: invalid target {target} (expected >= -1)");
        }
        let tip = self.highest_header_block()?;
        if target > tip {
            anyhow::bail!("rollback: target {target} is above the tip {tip} (highest header)");
        }
        self.ensure_no_range_rows_above(target, tip)?;
        let mut result = RollbackResult::default();
        if target == tip {
            return Ok(result);
        }
        if let Some(contract) = self.get_entity_stats_undo_contract()? {
            if target < contract.coverage_floor_block {
                anyhow::bail!(
                    "rollback to {target}: below the journal coverage floor {} (journal rows at or \
                     below it are pruned; floor last advanced at block {})",
                    contract.coverage_floor_block,
                    contract.updated_at_block
                );
            }
        }
        let first = target + 1;
        let first_key = keys::encode_block_num(first);
        let end_key = keys::encode_block_num(tip + 1);

        for item in self.iterator_cf(
            self.cf_block_headers(),
            IteratorMode::From(&first_key, Direction::Forward),
        ) {
            item.map_err(|e| anyhow::anyhow!("rollback to {target}: reading headers: {e}"))?;
            result.blocks_removed += 1;
        }
        for item in self.iterator_cf(
            self.cf_tx_index(),
            IteratorMode::From(&first_key, Direction::Forward),
        ) {
            item.map_err(|e| anyhow::anyhow!("rollback to {target}: reading tx_index: {e}"))?;
            result.txs_removed += 1;
        }
        let entries = self.journal_entries_from(first)?;
        let journaled: std::collections::BTreeSet<i64> =
            entries.iter().map(|entry| entry.block).collect();
        if let Some(missing) = (first..=tip).find(|block| !journaled.contains(block)) {
            anyhow::bail!(
                "rollback to {target}: block {missing} has no journal row (pruned or never \
                 journaled), so nothing restores what it wrote; tip {tip}"
            );
        }

        let mut batch = WriteBatch::default();
        // Every `RangeByBlock` CF loses the rolled-back blocks' key range:
        // ascending block keys, except activities' descending ones.
        let (activities_start, activities_end) = activities_range(first, tip);
        for spec in CF_SPECS
            .iter()
            .filter(|spec| spec.rollback_class == RollbackClass::RangeByBlock)
        {
            if spec.name == CF_TX_ACTIONS {
                batch.delete_range_cf(self.cf(spec.name), &activities_start, &activities_end);
            } else {
                batch.delete_range_cf(self.cf(spec.name), &first_key, &end_key);
            }
        }
        // Key order is (block, seq) ascending: reversed, it is the blocks from
        // the top down, each in reverse sequence order.
        for entry in entries.iter().rev() {
            if entry.cf == CF_LIVE_CELLS {
                match entry.previous {
                    Some(_) => result.cells_restored += 1,
                    None => result.cells_removed += 1,
                }
            }
            let cf = self.cf(entry.cf);
            match &entry.previous {
                Some(value) => batch.put_cf(cf, &entry.key, value),
                None => batch.delete_cf(cf, &entry.key),
            }
            result.journal_entries += 1;
        }
        self.write_rollback_batch(batch)?;
        info!(
            target,
            tip,
            blocks_removed = result.blocks_removed,
            txs_removed = result.txs_removed,
            journal_entries = result.journal_entries,
            cells_removed = result.cells_removed,
            cells_restored = result.cells_restored,
            "Rollback committed"
        );
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::*;
    use crate::block_overlay::{journal_seq, SharedBlockOverlay};
    use crate::store::{
        CF_BLOCK_HEADERS, CF_SYNC_META, CF_TX_ACTIONS, CF_TX_GROUP_CYCLES, CF_TX_INDEX,
    };
    use crate::DOMAIN_CFS;

    /// A hand-written journal row (`UndoSeqScope::Journal`, local `seq`).
    fn put_journal_row(
        batch: &mut StoreBatch,
        block: i64,
        seq: u64,
        cf: &str,
        key: &[u8],
        previous: Option<&[u8]>,
    ) {
        batch
            .put_raw_cf_by_name(
                CF_REORG_UNDO_LOG_BY_BLOCK,
                &keys::encode_reorg_undo_log_key(block, UndoSeqScope::Journal.seq_base() | seq),
                &crate::block_overlay::encode_journal_value(cf, key, previous).unwrap(),
            )
            .unwrap();
    }

    fn open() -> (CkbadgerStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (CkbadgerStore::open_domain(dir.path()).unwrap(), dir)
    }

    type Rows = BTreeMap<(&'static str, Vec<u8>), Vec<u8>>;
    /// One block's writes: (cf, key, value or delete).
    type BlockWrites<'a> = (i64, Vec<(&'a str, &'a [u8], Option<&'a [u8]>)>);
    type Corruption = (&'static str, Box<dyn Fn(&CkbadgerStore)>, &'static str);

    /// Every row of every domain CF.
    fn rows(store: &CkbadgerStore) -> Rows {
        let mut rows = Rows::new();
        for &cf in DOMAIN_CFS {
            let start: [u8; 0] = [];
            for item in
                store.iterator_cf(store.cf(cf), IteratorMode::From(&start, Direction::Forward))
            {
                let (key, value) = item.unwrap();
                rows.insert((cf, key.to_vec()), value.to_vec());
            }
        }
        rows
    }

    /// The rows of the block-numbered CFs a block writes: its header, two
    /// tx_index rows with their lazy cycles records and one `activities` row.
    fn block_rows(batch: &mut StoreBatch, block: i64) {
        batch
            .put_raw_cf_by_name(CF_BLOCK_HEADERS, &keys::encode_block_num(block), b"header")
            .unwrap();
        for tx in 0..2i32 {
            let key = keys::encode_tx_slot_key(block, tx);
            batch.put_raw_cf_by_name(CF_TX_INDEX, &key, b"tx").unwrap();
            batch
                .put_raw_cf_by_name(CF_TX_GROUP_CYCLES, &key, b"cycles")
                .unwrap();
        }
        batch
            .put_raw_cf_by_name(
                CF_TX_ACTIONS,
                &keys::encode_tx_actions_key(block, 0),
                b"activity",
            )
            .unwrap();
    }

    /// One live batch over `blocks`: each block's rows and `writes` (cf, key,
    /// value or delete) in its scope, journaled, committed.
    fn live_batch(store: &CkbadgerStore, blocks: &[BlockWrites<'_>]) {
        let overlay = SharedBlockOverlay::new(store).unwrap();
        let mut batch = StoreBatch::journaled(store, &overlay, 0);
        for (block, writes) in blocks {
            let mut at = batch.at(*block);
            block_rows(&mut at, *block);
            for (cf, key, value) in writes {
                match value {
                    Some(value) => at.put_raw_cf_by_name(cf, key, value).unwrap(),
                    None => at.delete_raw_cf_by_name(cf, key).unwrap(),
                }
            }
        }
        let mut counters = HashMap::new();
        batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();
    }

    fn meta(store: &CkbadgerStore, key: &[u8]) -> Option<Vec<u8>> {
        store.get_cf(store.cf_sync_meta(), key).unwrap()
    }

    /// Blocks 0..=2 in one batch, 3..=5 in another; `a` is written at 1, 2
    /// (twice), 4; `b` created at 3 and deleted at 5; `c` deleted at 4.
    fn chain(store: &CkbadgerStore) {
        live_batch(
            store,
            &[
                (0, vec![(CF_SYNC_META, b"c", Some(b"c0"))]),
                (1, vec![(CF_SYNC_META, b"a", Some(b"a1"))]),
                (
                    2,
                    vec![
                        (CF_SYNC_META, b"a", Some(b"a2")),
                        (CF_SYNC_META, b"a", Some(b"a2'")),
                    ],
                ),
            ],
        );
        live_batch(
            store,
            &[
                (3, vec![(CF_SYNC_META, b"b", Some(b"b3"))]),
                (
                    4,
                    vec![
                        (CF_SYNC_META, b"a", Some(b"a4")),
                        (CF_SYNC_META, b"c", None),
                        (CF_LIVE_CELLS, b"cell-old", None),
                        (CF_LIVE_CELLS, b"cell-new", Some(b"live")),
                    ],
                ),
                (5, vec![(CF_SYNC_META, b"b", None)]),
            ],
        );
    }

    /// Roll a copy of the chain back to each block of it and compare with a
    /// store that only wrote up to that block, with the same batches cut there.
    #[test]
    fn rollback_restores_every_block_end_inside_and_across_batches() {
        for target in (-1..=5).rev() {
            let (store, _dir) = open();
            // `cell-old` was live before the chain.
            store
                .put_cf(store.cf(CF_LIVE_CELLS), b"cell-old", b"live")
                .unwrap();
            chain(&store);
            store.rollback_to_block(target).unwrap();

            let (reference, _ref_dir) = open();
            reference
                .put_cf(reference.cf(CF_LIVE_CELLS), b"cell-old", b"live")
                .unwrap();
            if target >= 0 {
                let first: Vec<_> = [
                    (0, vec![(CF_SYNC_META, &b"c"[..], Some(&b"c0"[..]))]),
                    (1, vec![(CF_SYNC_META, &b"a"[..], Some(&b"a1"[..]))]),
                    (2, vec![(CF_SYNC_META, &b"a"[..], Some(&b"a2'"[..]))]),
                ]
                .into_iter()
                .filter(|(block, _)| *block <= target)
                .collect();
                live_batch(&reference, &first);
            }
            if target >= 3 {
                let second: Vec<_> = [
                    (3, vec![(CF_SYNC_META, &b"b"[..], Some(&b"b3"[..]))]),
                    (
                        4,
                        vec![
                            (CF_SYNC_META, &b"a"[..], Some(&b"a4"[..])),
                            (CF_SYNC_META, &b"c"[..], None),
                            (CF_LIVE_CELLS, &b"cell-old"[..], None),
                            (CF_LIVE_CELLS, &b"cell-new"[..], Some(&b"live"[..])),
                        ],
                    ),
                    (5, vec![(CF_SYNC_META, &b"b"[..], None)]),
                ]
                .into_iter()
                .filter(|(block, _)| *block <= target)
                .collect();
                live_batch(&reference, &second);
            }
            assert_eq!(rows(&store), rows(&reference), "rollback to {target}");
        }
    }

    #[test]
    fn replay_order_inside_a_block_is_reverse_sequence_and_across_blocks_top_down() {
        let (store, _dir) = open();
        chain(&store);
        // Two hand-written journal rows of one key in one block: the
        // lower sequence holds the older value and must be written last.
        let mut batch = StoreBatch::new(&store);
        block_rows(&mut batch, 6);
        for (seq, previous) in [(0u64, &b"a4"[..]), (1, &b"stale"[..])] {
            put_journal_row(&mut batch, 6, seq, CF_SYNC_META, b"a", Some(previous));
        }
        batch.put_raw_cf_by_name(CF_SYNC_META, b"a", b"a6").unwrap();
        batch.commit().unwrap();

        store.rollback_to_block(5).unwrap();
        assert_eq!(meta(&store, b"a"), Some(b"a4".to_vec()));
        // Across blocks: `a` journaled at 1, 2, 4; rolling back to 1 must end
        // on block 2's entry (the value at the end of block 1).
        store.rollback_to_block(1).unwrap();
        assert_eq!(meta(&store, b"a"), Some(b"a1".to_vec()));
        assert_eq!(meta(&store, b"b"), None);
        assert_eq!(meta(&store, b"c"), Some(b"c0".to_vec()));
    }

    #[test]
    fn every_range_by_block_cf_loses_exactly_the_rolled_back_blocks() {
        let (store, _dir) = open();
        chain(&store);
        let result = store.rollback_to_block(2).unwrap();
        assert_eq!(result.blocks_removed, 3);
        assert_eq!(result.txs_removed, 6);
        let rows = rows(&store);
        let blocks_of = |cf: &str| -> Vec<i64> {
            let mut blocks: Vec<i64> = rows
                .keys()
                .filter(|(row_cf, _)| *row_cf == cf)
                .map(|(_, key)| {
                    if cf == CF_TX_ACTIONS {
                        keys::decode_stored_tx_actions_key(key).unwrap().0
                    } else {
                        keys::decode_block_num(key)
                    }
                })
                .collect();
            blocks.sort();
            blocks.dedup();
            blocks
        };
        assert_eq!(blocks_of(CF_BLOCK_HEADERS), vec![0, 1, 2]);
        assert_eq!(blocks_of(CF_TX_INDEX), vec![0, 1, 2]);
        assert_eq!(blocks_of(CF_TX_GROUP_CYCLES), vec![0, 1, 2]);
        assert_eq!(blocks_of(CF_TX_ACTIONS), vec![0, 1, 2], "descending keys");
        let undo_blocks = blocks_of(CF_REORG_UNDO_LOG_BY_BLOCK);
        assert!(
            undo_blocks.iter().all(|block| *block <= 2),
            "{undo_blocks:?}"
        );

        // Down to genesis: block 0's activities range ends at 2^63.
        store.rollback_to_block(-1).unwrap();
        let rows = self::rows(&store);
        for cf in CF_SPECS
            .iter()
            .filter(|spec| spec.rollback_class == RollbackClass::RangeByBlock)
            .map(|spec| spec.name)
        {
            assert!(
                rows.keys().all(|(row_cf, _)| *row_cf != cf),
                "{cf} keeps rows after a rollback to -1"
            );
        }
    }

    #[test]
    fn rollback_counts_live_cell_entries_and_journal_entries() {
        let (store, _dir) = open();
        store
            .put_cf(store.cf(CF_LIVE_CELLS), b"cell-old", b"live")
            .unwrap();
        chain(&store);
        let result = store.rollback_to_block(3).unwrap();
        assert_eq!(result.cells_removed, 1, "cell-new created at 4");
        assert_eq!(result.cells_restored, 1, "cell-old consumed at 4");
        // Block 4: a, c, cell-old, cell-new, the header's hash index is not
        // written (raw header); block 5: b.
        assert_eq!(result.journal_entries, 5);
        assert_eq!(
            store.get_cf(store.cf(CF_LIVE_CELLS), b"cell-old").unwrap(),
            Some(b"live".to_vec())
        );
        assert_eq!(
            store.get_cf(store.cf(CF_LIVE_CELLS), b"cell-new").unwrap(),
            None
        );
    }

    /// The per-writer undo scopes are gone: a row with any other scope bits
    /// is not a row this store writes, and reading it is an error (the
    /// rollback's case is in `rollback_errors_leave_the_store_untouched`).
    #[test]
    fn a_row_of_another_scope_is_refused_by_every_journal_read() {
        let (store, _dir) = open();
        chain(&store);
        store
            .put_cf(
                store.cf_reorg_undo_log_by_block(),
                &keys::encode_reorg_undo_log_key(5, 0x0003 << keys::UNDO_SEQ_SCOPE_SHIFT),
                b"per-writer row",
            )
            .unwrap();
        let error = store.journal_entries(5).unwrap_err().to_string();
        assert!(
            error.contains("block 5") && error.contains("not a journal row (scope bits 0x0003)"),
            "{error}"
        );
        let mut batch = StoreBatch::new(&store);
        let error = store
            .stage_delete_journal_above(&mut batch, &[crate::store::CF_DAO_LIVE_AGG], 4)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a journal row"), "{error}");
    }

    #[test]
    fn rollback_errors_leave_the_store_untouched() {
        let corruptions: Vec<Corruption> = vec![
            (
                "undecodable",
                Box::new(|store| {
                    store
                        .put_cf(
                            store.cf_reorg_undo_log_by_block(),
                            &keys::encode_reorg_undo_log_key(
                                4,
                                UndoSeqScope::Journal.seq_base() | 99,
                            ),
                            b"\xff\xff",
                        )
                        .unwrap()
                }),
                "undecodable entry at block 4",
            ),
            (
                "per-writer scope",
                Box::new(|store| {
                    store
                        .put_cf(
                            store.cf_reorg_undo_log_by_block(),
                            &keys::encode_reorg_undo_log_key(
                                4,
                                0x0004 << keys::UNDO_SEQ_SCOPE_SHIFT,
                            ),
                            b"entity stats scope row",
                        )
                        .unwrap()
                }),
                "undo-log row at block 4 seq 0x4000000000000 is not a journal row",
            ),
            (
                "the coverage contract",
                Box::new(|store| {
                    let mut batch = StoreBatch::new(store);
                    put_journal_row(
                        &mut batch,
                        4,
                        97,
                        CF_SYNC_META,
                        keys::sync_meta_keys::ENTITY_STATS_UNDO_CONTRACT,
                        Some(b"older floor"),
                    );
                    batch.commit().unwrap();
                }),
                "which is written outside the journal",
            ),
            (
                "non-Journal CF",
                Box::new(|store| {
                    let mut batch = StoreBatch::new(store);
                    put_journal_row(&mut batch, 4, 99, CF_TX_INDEX, b"k", None);
                    batch.commit().unwrap();
                }),
                "which is RangeByBlock, not Journal",
            ),
            (
                "append-only CF",
                Box::new(|store| {
                    // The append-only `cells` payloads are never rolled back.
                    let mut batch = StoreBatch::new(store);
                    put_journal_row(&mut batch, 4, 98, crate::store::CF_CELLS, b"k", None);
                    batch.commit().unwrap();
                }),
                "names CF cells, which is Never, not Journal",
            ),
            (
                "unknown CF",
                Box::new(|store| {
                    let mut batch = StoreBatch::new(store);
                    put_journal_row(&mut batch, 5, 99, "no_such_cf", b"k", None);
                    batch.commit().unwrap();
                }),
                "names unknown CF no_such_cf",
            ),
            (
                "journal above the tip",
                Box::new(|store| {
                    let mut batch = StoreBatch::new(store);
                    put_journal_row(&mut batch, 9, 0, CF_SYNC_META, b"k", None);
                    batch.commit().unwrap();
                }),
                "reorg_undo_log_by_block row of block 9 above the tip 5",
            ),
            (
                "target below the coverage floor",
                Box::new(|store| {
                    store
                        .put_entity_stats_undo_contract(&crate::types::EntityStatsUndoContract {
                            version: crate::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
                            coverage_floor_block: 3,
                            updated_at_block: 5,
                        })
                        .unwrap()
                }),
                "below the journal coverage floor 3",
            ),
            (
                "a block with no journal row",
                Box::new(|store| {
                    // Block 4's journal rows gone (pruned or never written).
                    let first = keys::encode_block_num(4);
                    let block_4: Vec<Vec<u8>> = store
                        .iterator_cf(
                            store.cf_reorg_undo_log_by_block(),
                            IteratorMode::From(&first, Direction::Forward),
                        )
                        .map(|item| item.unwrap().0.to_vec())
                        .take_while(|key| key.starts_with(&first))
                        .collect();
                    assert!(!block_4.is_empty());
                    for key in block_4 {
                        store
                            .delete_cf(store.cf_reorg_undo_log_by_block(), &key)
                            .unwrap();
                    }
                }),
                "block 4 has no journal row",
            ),
            (
                "a lazy cycles record above the tip",
                Box::new(|store| {
                    store
                        .put_cf(
                            store.cf(CF_TX_GROUP_CYCLES),
                            &keys::encode_tx_slot_key(7, 1),
                            b"cycles",
                        )
                        .unwrap()
                }),
                "tx_group_cycles row of block 7 above the tip 5",
            ),
        ];
        for (name, corrupt, expected) in corruptions {
            let (store, _dir) = open();
            chain(&store);
            corrupt(&store);
            let before = rows(&store);
            let error = store.rollback_to_block(2).unwrap_err().to_string();
            assert!(error.contains(expected), "{name}: {error}");
            assert_eq!(
                rows(&store),
                before,
                "{name}: a failed rollback wrote nothing"
            );
        }
    }

    /// Rows above the tip are refused even when there is nothing to roll
    /// back (target = tip): startup cleanup runs exactly then.
    #[test]
    fn rows_above_the_tip_are_refused_when_the_target_is_the_tip() {
        for (cf, key, block) in [
            (CF_TX_INDEX, keys::encode_tx_slot_key(8, 0), 8),
            (CF_TX_GROUP_CYCLES, keys::encode_tx_slot_key(8, 1), 8),
            (
                CF_REORG_UNDO_LOG_BY_BLOCK,
                keys::encode_reorg_undo_log_key(9, UndoSeqScope::Journal.seq_base()).to_vec(),
                9,
            ),
            (CF_TX_ACTIONS, keys::encode_tx_actions_key(7, 0).to_vec(), 7),
        ] {
            let (store, _dir) = open();
            chain(&store);
            store.put_cf(store.cf(cf), &key, b"row").unwrap();
            let before = rows(&store);
            let error = store.rollback_to_block(5).unwrap_err().to_string();
            assert!(
                error.contains(&format!("{cf} row of block {block} above the tip 5")),
                "{cf}: {error}"
            );
            assert_eq!(rows(&store), before, "{cf}: nothing written");
        }
    }

    #[test]
    fn rollback_targets_outside_the_stored_chain_are_errors() {
        let (store, _dir) = open();
        chain(&store);
        let before = rows(&store);
        let error = store.rollback_to_block(-2).unwrap_err().to_string();
        assert!(error.contains("invalid target -2"), "{error}");
        let error = store.rollback_to_block(6).unwrap_err().to_string();
        assert!(error.contains("target 6 is above the tip 5"), "{error}");
        assert_eq!(
            store.rollback_to_block(5).unwrap(),
            RollbackResult::default()
        );
        assert_eq!(rows(&store), before);

        let (empty, _dir) = open();
        assert_eq!(
            empty.rollback_to_block(-1).unwrap(),
            RollbackResult::default()
        );
        assert!(empty.rollback_to_block(0).is_err());
    }

    #[test]
    fn journal_entries_and_has_journal_after() {
        let (store, _dir) = open();
        store
            .put_cf(store.cf(CF_LIVE_CELLS), b"cell-old", b"live")
            .unwrap();
        chain(&store);
        let block_4 = store.journal_entries(4).unwrap();
        assert!(block_4.windows(2).all(|pair| pair[0].seq < pair[1].seq));
        let keys_4: Vec<(&str, &[u8])> = block_4
            .iter()
            .map(|entry| (entry.cf, entry.key.as_slice()))
            .collect();
        assert_eq!(keys_4.len(), 4, "{keys_4:?}");
        assert!(keys_4.contains(&(CF_SYNC_META, &b"c"[..])));
        let c = block_4.iter().find(|entry| entry.key == b"c").unwrap();
        assert_eq!(c.previous, Some(b"c0".to_vec()));
        assert!(store.journal_entries(7).unwrap().is_empty());

        assert!(store.has_journal_after(4).unwrap());
        assert!(!store.has_journal_after(5).unwrap());
        assert!(store.has_journal_after(-1).unwrap());
        assert!(store.has_journal_after(-2).is_err());
        store
            .put_cf(store.cf_reorg_undo_log_by_block(), b"short", b"")
            .unwrap();
        // `short` (0x73..) sorts after every key of a small block: the probe
        // above block 5 reaches it.
        let error = store.has_journal_after(5).unwrap_err().to_string();
        assert!(error.contains("undo-log key of 5 bytes"), "{error}");
    }

    #[test]
    fn prune_is_one_range_delete_of_every_row_below_the_floor() {
        let (store, _dir) = open();
        let mut batch = StoreBatch::new(&store);
        for block in 1..=10i64 {
            for seq in 0..3u64 {
                put_journal_row(&mut batch, block, seq, CF_SYNC_META, b"k", None);
            }
        }
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        store.prune_journal_below(&mut batch, 0, 4).unwrap();
        assert_eq!(batch.len(), 1, "one range delete");
        batch.commit().unwrap();
        let blocks: Vec<i64> = rows(&store)
            .keys()
            .map(|(_, key)| keys::decode_reorg_undo_log_key(key).0)
            .collect();
        assert_eq!(blocks.len(), 18);
        assert!(blocks.iter().all(|block| *block >= 5), "{blocks:?}");

        let mut batch = StoreBatch::new(&store);
        store.prune_journal_below(&mut batch, 4, 4).unwrap();
        store.prune_journal_below(&mut batch, 4, 3).unwrap();
        assert!(
            batch.is_empty(),
            "a floor that does not move up prunes nothing"
        );
        assert!(store.prune_journal_below(&mut batch, -2, 3).is_err());
    }

    #[test]
    fn stage_delete_journal_above_drops_only_the_named_cfs_above_the_target() {
        let (store, _dir) = open();
        live_batch(
            &store,
            &[
                (0, vec![(crate::store::CF_DAO_LIVE_AGG, b"x", Some(b"x0"))]),
                (
                    1,
                    vec![
                        (crate::store::CF_DAO_LIVE_AGG, b"x", Some(b"x1")),
                        (CF_SYNC_META, b"m", Some(b"m1")),
                    ],
                ),
            ],
        );
        let mut batch = StoreBatch::new(&store);
        let dropped = store
            .stage_delete_journal_above(&mut batch, &[crate::store::CF_DAO_LIVE_AGG], 0)
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(dropped, 1);
        let cfs: Vec<&str> = store
            .journal_entries(1)
            .unwrap()
            .into_iter()
            .map(|entry| entry.cf)
            .collect();
        assert_eq!(cfs, vec![CF_SYNC_META]);
        assert_eq!(
            store.journal_entries(0).unwrap().len(),
            1,
            "block 0 is kept"
        );
    }
}
