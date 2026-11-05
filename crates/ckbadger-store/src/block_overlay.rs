//! The block journal's write side: one overlay per live batch over every
//! `Journal`-class domain column family (`CfSpec::rollback_class`).
//!
//! A live batch spans many blocks and a shallow fork can land on any of them,
//! so rolling back to block N must restore every key to its value at the end
//! of block N. The overlay keeps, for every key the batch touches, its value
//! as of the end of the last block that wrote it, and records a journal entry
//! `(cf, key, previous)` the first time a block writes the key:
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
//! A block whose writes leave a key at the value it found (an idempotent
//! rewrite such as a `lock_scripts` row of a known lock) journals nothing for
//! it: restoring that value would be a no-op.
//!
//! Blocks must reach a key in ascending order; a write for an older block than
//! the key's last one is an error, never a silently wrong pre-image. Every
//! write must name its block ([`crate::StoreBatch::at`]): a write without one
//! could not be journaled, so it fails the batch.
//!
//! One `Journal`-class key is written outside the journal: the coverage
//! contract (`sync_meta` `entity_stats_undo_contract`), which describes which
//! journal rows still exist ([`is_outside_journal`]). The overlay refuses it.
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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use rocksdb::WriteBatch;

use crate::keys::{self, UndoSeqScope, UNDO_SEQ_LOCAL_MAX};
use crate::store::{cf_spec, CkbadgerStore, RollbackClass};

type CfKey = (&'static str, Vec<u8>);

/// Whether `(cf, key)` is never journaled, on any batch: an
/// [`crate::store::OUTSIDE_JOURNAL`] entry marked `never_journaled` (the
/// coverage contract, the sync status's runtime part, telemetry, reorg
/// events, derive state, the DAO tip summary's singletons, ...). A journaled
/// batch stages such a write straight into its commit; the overlay refuses
/// it and the rollback refuses a journal row naming it.
pub fn is_outside_journal(cf: &str, key: &[u8]) -> bool {
    crate::store::outside_journal_entry(cf, key).is_some_and(|entry| entry.never_journaled)
}

/// A key's state inside the batch.
#[derive(Debug, Clone)]
struct KeyState {
    /// The block whose end this value is.
    last_block: i64,
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
    keys: HashMap<CfKey, KeyState>,
    /// Committed values read so far (pre-batch), by `current` or `prefetch`.
    committed: HashMap<CfKey, Option<Vec<u8>>>,
    entries: Vec<PendingEntry>,
    /// Which sub-batch first wrote each key, to catch a key written from two
    /// sub-batches whose merge order could disagree with program order.
    origin: HashMap<CfKey, u32>,
    finished: bool,
}

impl BlockOverlay {
    /// An overlay for one live batch on `store`. Bulk sync writes no journal
    /// and never builds an overlay: a store in bulk-sync mode is an error,
    /// never an overlay that silently stops journaling.
    pub fn new(store: &CkbadgerStore) -> Result<Self> {
        if store.is_bulk_sync_mode() {
            bail!(
                "block overlay on a store in bulk-sync mode: a journaled batch is live-only, and \
                 bulk writes no journal"
            );
        }
        Ok(Self::default())
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
    /// `block = None` is a write no block scope claimed ([`crate::StoreBatch::at`]):
    /// its previous value could not be journaled, so it is an error naming the
    /// CF and key, and the batch fails before anything is committed.
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
        if is_outside_journal(cf, key) {
            bail!(
                "block overlay: cf={cf} key=0x{} is written outside the block journal \
                 (StoreBatch::put_entity_stats_undo_contract), never through the overlay",
                hex::encode(key)
            );
        }
        let Some(block) = block else {
            bail!(
                "block overlay: a Journal-class write without a block scope (StoreBatch::at): \
                 cf={cf} key=0x{}",
                hex::encode(key)
            );
        };
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
        match self.keys.get_mut(&cf_key) {
            None => {
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
                self.keys.insert(
                    cf_key,
                    KeyState {
                        last_block: block,
                        value,
                    },
                );
            }
            Some(state) => {
                let last = state.last_block;
                if block < last {
                    bail!(
                        "block overlay requires ascending block order per key: cf={cf} key=0x{} \
                         last_block={last} block={block}",
                        hex::encode(key)
                    );
                }
                if block > last {
                    self.entries.push(PendingEntry {
                        block,
                        cf,
                        key: key.to_vec(),
                        previous: Previous::Known(state.value.clone()),
                    });
                    state.last_block = block;
                }
                state.value = value;
            }
        }
        Ok(())
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

        // Every entry's previous value, and whether its block changed the key
        // at all: a key's entries are in ascending block order, so the value
        // at the end of an entry's block is the previous value of the key's
        // next entry, or the key's final value after its last one. A block
        // that leaves the key as it found it (an idempotent rewrite, a delete
        // of a missing key) journals nothing: restoring the value is a no-op.
        let mut previous_values: Vec<Option<Option<Vec<u8>>>> = vec![None; self.entries.len()];
        let mut value_after: HashMap<CfKey, Option<Vec<u8>>> = HashMap::new();
        for (index, entry) in self.entries.iter().enumerate().rev() {
            let cf_key = (entry.cf, entry.key.clone());
            let previous_value = match &entry.previous {
                Previous::Known(previous) => previous.clone(),
                Previous::Committed => self.committed.get(&cf_key).cloned().ok_or_else(|| {
                    anyhow!(
                        "block overlay: committed pre-image not resolved: cf={} key=0x{}",
                        entry.cf,
                        hex::encode(&entry.key)
                    )
                })?,
            };
            let end_of_block = match value_after.get(&cf_key) {
                Some(value) => value.clone(),
                None => self
                    .keys
                    .get(&cf_key)
                    .ok_or_else(|| {
                        anyhow!(
                            "block overlay: journal entry for a key never written: cf={} key=0x{}",
                            entry.cf,
                            hex::encode(&entry.key)
                        )
                    })?
                    .value
                    .clone(),
            };
            if previous_value != end_of_block {
                previous_values[index] = Some(previous_value.clone());
            }
            value_after.insert(cf_key, previous_value);
        }

        let undo_cf = store.cf_reorg_undo_log_by_block();
        for (entry, previous_value) in self.entries.iter().zip(previous_values) {
            let Some(previous_value) = previous_value else {
                continue;
            };
            let seq = next_seq(entry.block)?;
            let row_key = keys::encode_reorg_undo_log_key(entry.block, seq);
            let row_value = encode_journal_value(entry.cf, &entry.key, previous_value.as_deref())?;
            stats.entries += 1;
            stats.bytes += (row_key.len() + row_value.len()) as u64;
            batch.put_cf(undo_cf, row_key, &row_value);
        }
        self.finished = true;
        Ok(stats)
    }
}

/// The value of one block journal row (`reorg_undo_log_by_block`, sequence
/// scope `Journal`): `[cf name length u8][cf name][key length u16 BE][key]
/// [tag u8: 0 = absent, 1 = present]` and, when present, `[value length u32
/// BE][value]`. One encode, one decode ([`decode_journal_value`]), shared by
/// the overlay that writes the rows and the rollback that reads them.
pub fn encode_journal_value(cf: &str, key: &[u8], previous: Option<&[u8]>) -> Result<Vec<u8>> {
    let cf_len = u8::try_from(cf.len())
        .map_err(|_| anyhow!("journal value: CF name of {} bytes: {cf}", cf.len()))?;
    let key_len = u16::try_from(key.len()).map_err(|_| {
        anyhow!(
            "journal value: key of {} bytes in cf={cf}: 0x{}",
            key.len(),
            hex::encode(key)
        )
    })?;
    let mut value = Vec::with_capacity(
        1 + cf.len() + 2 + key.len() + 1 + previous.map_or(0, |previous| 4 + previous.len()),
    );
    value.push(cf_len);
    value.extend_from_slice(cf.as_bytes());
    value.extend_from_slice(&key_len.to_be_bytes());
    value.extend_from_slice(key);
    match previous {
        None => value.push(0),
        Some(previous) => {
            let previous_len = u32::try_from(previous.len()).map_err(|_| {
                anyhow!(
                    "journal value: previous value of {} bytes in cf={cf} key=0x{}",
                    previous.len(),
                    hex::encode(key)
                )
            })?;
            value.push(1);
            value.extend_from_slice(&previous_len.to_be_bytes());
            value.extend_from_slice(previous);
        }
    }
    Ok(value)
}

/// One decoded block journal row: the CF name, the key and its previous
/// value, borrowed from the encoded row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalValue<'a> {
    pub cf: &'a str,
    pub key: &'a [u8],
    pub previous: Option<&'a [u8]>,
}

/// Decode a block journal row written by [`encode_journal_value`]; a short
/// row, trailing bytes, an unknown presence tag or a CF name that is not
/// UTF-8 is an error naming the offset.
pub fn decode_journal_value(value: &[u8]) -> Result<JournalValue<'_>> {
    fn take<'a>(value: &'a [u8], at: &mut usize, len: usize, what: &str) -> Result<&'a [u8]> {
        let end = at
            .checked_add(len)
            .filter(|end| *end <= value.len())
            .ok_or_else(|| {
                anyhow!(
                    "journal value: short row reading {what} ({len} bytes at offset {at} of {})",
                    value.len()
                )
            })?;
        let bytes = &value[*at..end];
        *at = end;
        Ok(bytes)
    }
    let mut at = 0;
    let cf_len = take(value, &mut at, 1, "the CF name length")?[0] as usize;
    let cf = std::str::from_utf8(take(value, &mut at, cf_len, "the CF name")?)
        .map_err(|e| anyhow!("journal value: CF name is not UTF-8: {e}"))?;
    let key_len = u16::from_be_bytes(
        take(value, &mut at, 2, "the key length")?
            .try_into()
            .expect("two bytes"),
    ) as usize;
    let key = take(value, &mut at, key_len, "the key")?;
    let previous = match take(value, &mut at, 1, "the presence tag")?[0] {
        0 => None,
        1 => {
            let len = u32::from_be_bytes(
                take(value, &mut at, 4, "the value length")?
                    .try_into()
                    .expect("four bytes"),
            ) as usize;
            Some(take(value, &mut at, len, "the value")?)
        }
        tag => bail!(
            "journal value: unknown presence tag {tag} at offset {} (cf={cf})",
            at - 1
        ),
    };
    if at != value.len() {
        bail!(
            "journal value: {} trailing bytes after the entry (cf={cf} key=0x{})",
            value.len() - at,
            hex::encode(key)
        );
    }
    Ok(JournalValue { cf, key, previous })
}

/// The one [`BlockOverlay`] of a live batch, shared by every sub-batch and
/// writer of it. `Arc<Mutex<_>>`: the batch write future must stay `Send`.
#[derive(Clone, Debug, Default)]
pub struct SharedBlockOverlay(Arc<Mutex<BlockOverlay>>);

impl SharedBlockOverlay {
    /// See [`BlockOverlay::new`]: an error on a store in bulk-sync mode.
    pub fn new(store: &CkbadgerStore) -> Result<Self> {
        Ok(Self(Arc::new(Mutex::new(BlockOverlay::new(store)?))))
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

/// Fixture support (tests and test harnesses of every crate): commit `write`
/// the way a live batch commits — a journaled [`crate::StoreBatch`] over a
/// fresh overlay, whose `Journal`-class writes `write` attributes to blocks
/// with [`crate::StoreBatch::at`], the journal staged (each block's sequence
/// numbers continue after the journal rows the store already holds for it)
/// and everything committed in one write. A fixture that seeds rows this way
/// can be rolled back by the journal like rows the live writer wrote.
#[doc(hidden)]
pub fn commit_journaled(
    store: &CkbadgerStore,
    write: impl FnOnce(&mut crate::StoreBatch),
) -> Result<JournalStats> {
    let overlay = SharedBlockOverlay::new(store)?;
    let mut batch = crate::StoreBatch::journaled(store, &overlay, 0);
    write(&mut batch);
    let stats = batch.stage_journal(&mut continuing_journal_seq(store))?;
    batch.commit()?;
    Ok(stats)
}

/// Fixture support: a sequence allocator whose numbers for a block continue
/// after the journal rows `store` already holds for it, for a block that more
/// than one batch writes (a live batch owns its blocks and starts at 0).
#[doc(hidden)]
pub fn continuing_journal_seq(store: &CkbadgerStore) -> impl FnMut(i64) -> Result<u64> + '_ {
    let mut counters: HashMap<i64, u64> = HashMap::new();
    move |block| {
        if let std::collections::hash_map::Entry::Vacant(slot) = counters.entry(block) {
            slot.insert(match store.journal_entries(block)?.last() {
                Some(entry) => (entry.seq & UNDO_SEQ_LOCAL_MAX) + 1,
                None => 0,
            });
        }
        journal_seq(&mut counters, block)
    }
}

/// [`commit_journaled`] with every write in `block`'s scope.
#[doc(hidden)]
pub fn commit_journaled_block(
    store: &CkbadgerStore,
    block: i64,
    write: impl FnOnce(&mut crate::StoreBatch),
) -> Result<JournalStats> {
    commit_journaled(store, |batch| write(&mut batch.at(block)))
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
                let entry = decode_journal_value(&value).unwrap();
                (
                    block,
                    entry.cf.to_string(),
                    entry.key.to_vec(),
                    entry.previous.map(<[u8]>::to_vec),
                )
            })
            .collect()
    }

    #[test]
    fn the_journal_value_codec_round_trips_and_is_exact() {
        for (cf, key, previous) in [
            ("addr_txs", &[0xAB; 76][..], None),
            ("sync_meta", &b"sync_status"[..], Some(&b"previous"[..])),
            ("live_cells", &b""[..], Some(&b""[..])),
        ] {
            let value = encode_journal_value(cf, key, previous).unwrap();
            assert_eq!(
                value.len(),
                1 + cf.len() + 2 + key.len() + 1 + previous.map_or(0, |p| 4 + p.len())
            );
            assert_eq!(
                decode_journal_value(&value).unwrap(),
                JournalValue { cf, key, previous }
            );
        }
        // The layout, byte by byte.
        assert_eq!(
            encode_journal_value("ab", b"k", Some(b"vv")).unwrap(),
            [&[2u8][..], b"ab", &[0, 1], b"k", &[1], &[0, 0, 0, 2], b"vv"].concat()
        );
        assert_eq!(
            encode_journal_value("ab", b"k", None).unwrap(),
            [&[2u8][..], b"ab", &[0, 1], b"k", &[0]].concat()
        );
    }

    #[test]
    fn the_journal_value_codec_rejects_malformed_rows() {
        let value = encode_journal_value("sync_meta", b"key", Some(b"value")).unwrap();
        for len in 0..value.len() {
            let error = decode_journal_value(&value[..len]).unwrap_err().to_string();
            assert!(error.contains("short row"), "truncated to {len}: {error}");
        }
        let trailing = [value.clone(), vec![0]].concat();
        let error = decode_journal_value(&trailing).unwrap_err().to_string();
        assert!(error.contains("1 trailing bytes"), "{error}");

        let mut unknown_tag = encode_journal_value("sync_meta", b"key", None).unwrap();
        *unknown_tag.last_mut().unwrap() = 7;
        let error = decode_journal_value(&unknown_tag).unwrap_err().to_string();
        assert!(error.contains("unknown presence tag 7"), "{error}");

        let not_utf8 = [&[1u8, 0xFF][..], &[0, 0], &[0]].concat();
        let error = decode_journal_value(&not_utf8).unwrap_err().to_string();
        assert!(error.contains("not UTF-8"), "{error}");

        assert!(encode_journal_value(&"x".repeat(256), b"k", None).is_err());
        assert!(encode_journal_value("sync_meta", &vec![0; 65_536], None).is_err());
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
        let overlay = SharedBlockOverlay::new(&store).unwrap();
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

    /// A block that leaves a key as it found it — an idempotent rewrite, a
    /// key created and deleted inside the block, a delete of a missing key —
    /// journals nothing (restoring the value would be a no-op); the blocks
    /// around it still journal the value at the end of the previous block.
    #[test]
    fn a_block_that_leaves_a_key_unchanged_journals_nothing() {
        let (store, _dir) = store();
        store.put_cf(store.cf_addr_balance(), b"k", b"v0").unwrap();
        let overlay = SharedBlockOverlay::new(&store).unwrap();
        put(&overlay, 5, b"k", b"v0");
        put(&overlay, 6, b"k", b"v1");
        put(&overlay, 7, b"k", b"tmp");
        put(&overlay, 7, b"k", b"v1");
        put(&overlay, 8, b"k", b"v2");
        put(&overlay, 8, b"brief", b"x");
        overlay
            .with(|o| o.write(Some(8), CF_ADDR_BALANCE, b"brief", None, 0))
            .unwrap();
        overlay
            .with(|o| o.write(Some(9), CF_ADDR_BALANCE, b"missing", None, 0))
            .unwrap();
        let stats = commit(&store, &overlay);
        assert_eq!(
            journal_rows(&store),
            vec![
                (
                    6,
                    CF_ADDR_BALANCE.into(),
                    b"k".to_vec(),
                    Some(b"v0".to_vec())
                ),
                (
                    8,
                    CF_ADDR_BALANCE.into(),
                    b"k".to_vec(),
                    Some(b"v1".to_vec())
                ),
            ]
        );
        assert_eq!(stats.entries, 2);
        assert_eq!(stats.keys_written, 3, "every written key is still staged");
        assert_eq!(
            store.get_cf(store.cf_addr_balance(), b"k").unwrap(),
            Some(b"v2".to_vec())
        );
    }

    /// A block older than the key's last one would record a pre-image from
    /// the future.
    #[test]
    fn a_descending_block_for_a_key_is_an_error() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store).unwrap();
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
        let overlay = SharedBlockOverlay::new(&store).unwrap();
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
        let overlay = SharedBlockOverlay::new(&store).unwrap();
        let err = overlay
            .with(|o| o.write(Some(1), CF_BLOCK_HEADERS, b"k", Some(vec![1]), 0))
            .unwrap_err();
        assert!(err.to_string().contains("RangeByBlock"), "{err:#}");
    }

    /// A journaled StoreBatch sends Journal-class writes through the overlay
    /// and every other CF straight to its WriteBatch; it refuses to commit
    /// before the overlay is staged.
    #[test]
    fn a_journaled_batch_routes_by_rollback_class_and_must_be_staged() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store).unwrap();
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
        let overlay = SharedBlockOverlay::new(&store).unwrap();
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

    /// A journaled batch reads a stats key through the overlay: the latest
    /// write of the live batch (from any sub-batch), else the committed
    /// value; an unjournaled batch reads the committed value only.
    #[test]
    fn get_stats_reads_the_overlay_of_a_journaled_batch() {
        let (store, _dir) = store();
        let key = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260210");
        store.put_stats_key(&key, b"committed").unwrap();
        let overlay = SharedBlockOverlay::new(&store).unwrap();
        let mut writer = StoreBatch::journaled(&store, &overlay, 1);
        let reader = StoreBatch::journaled(&store, &overlay, 2);
        assert_eq!(reader.get_stats(&key).unwrap(), Some(b"committed".to_vec()));
        writer.at(7).put_stats(&key, b"after block 7");
        assert_eq!(
            reader.get_stats(&key).unwrap(),
            Some(b"after block 7".to_vec()),
            "another sub-batch's write of the same live batch is visible"
        );
        writer.at(8).delete_stats(&key);
        assert_eq!(reader.get_stats(&key).unwrap(), None);
        let plain = StoreBatch::new(&store);
        assert_eq!(
            plain.get_stats(&key).unwrap(),
            Some(b"committed".to_vec()),
            "an unjournaled batch sees committed values only"
        );
        assert!(reader.get_stats(&[]).is_err());
    }

    /// The generic read of a journaled batch is the overlay's value for any
    /// `Journal`-class CF (the live batch's latest write from any sub-batch,
    /// else committed), several keys in their order; a non-Journal CF cannot
    /// be read back through it; an unjournaled batch reads committed values.
    #[test]
    fn get_current_reads_the_overlay_of_a_journaled_batch_for_any_journal_cf() {
        let (store, _dir) = store();
        store
            .put_cf(store.cf_sync_meta(), b"a", b"committed a")
            .unwrap();
        store
            .put_cf(store.cf_sync_meta(), b"b", b"committed b")
            .unwrap();
        let overlay = SharedBlockOverlay::new(&store).unwrap();
        let mut writer = StoreBatch::journaled(&store, &overlay, 1);
        let reader = StoreBatch::journaled(&store, &overlay, 2);
        writer.at(7).put_sync_meta(b"a", b"after block 7");
        writer.at(8).delete_sync_meta(b"b");
        writer.at(8).put_sync_meta(b"c", b"created at 8");
        assert_eq!(
            reader
                .get_current_many(
                    CF_SYNC_META,
                    &[b"c".to_vec(), b"a".to_vec(), b"b".to_vec(), b"d".to_vec()]
                )
                .unwrap(),
            vec![
                Some(b"created at 8".to_vec()),
                Some(b"after block 7".to_vec()),
                None,
                None
            ]
        );
        assert_eq!(
            reader.get_current(CF_SYNC_META, b"a").unwrap(),
            Some(b"after block 7".to_vec())
        );
        let err = reader.get_current(CF_BLOCK_HEADERS, b"k").unwrap_err();
        assert!(err.to_string().contains("not one"), "{err:#}");
        let plain = StoreBatch::new(&store);
        assert_eq!(
            plain
                .get_current_many(CF_SYNC_META, &[b"a".to_vec(), b"b".to_vec()])
                .unwrap(),
            vec![Some(b"committed a".to_vec()), Some(b"committed b".to_vec())],
            "an unjournaled batch sees committed values only"
        );
        assert!(plain.get_current("no_such_cf", b"k").is_err());
    }

    /// A `Journal`-class write no block scope claimed cannot be journaled:
    /// it fails the batch before anything is committed, naming the CF and the
    /// key.
    #[test]
    fn an_unscoped_journal_write_fails_the_batch() {
        let (store, _dir) = store();
        let overlay = SharedBlockOverlay::new(&store).unwrap();
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        batch.at(3).put_sync_meta(b"scoped", b"v");
        batch.put_sync_meta(b"unscoped", b"v");
        let mut counters = HashMap::new();
        let error = batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("without a block scope")
                && error.contains("cf=sync_meta")
                && error.contains(&format!("key=0x{}", hex::encode(b"unscoped"))),
            "{error}"
        );
        assert!(
            batch.commit().is_err(),
            "the batch refuses to commit once its overlay was not staged"
        );
        assert_eq!(store.get_cf(store.cf_sync_meta(), b"scoped").unwrap(), None);
        assert!(journal_rows(&store).is_empty());

        // The bare overlay refuses it too.
        let bare = SharedBlockOverlay::new(&store).unwrap();
        let error = bare
            .with(|o| o.write(None, CF_ADDR_BALANCE, b"k", Some(vec![1]), 0))
            .unwrap_err()
            .to_string();
        assert!(error.contains("without a block scope"), "{error}");
    }

    /// A store in bulk-sync mode journals nothing, so a journaled batch on it
    /// is an error, never an overlay that silently stops journaling.
    #[test]
    fn a_journaled_batch_on_a_bulk_mode_store_is_an_error() {
        let (store, _dir) = store();
        store.set_bulk_sync_mode(true);
        let error = SharedBlockOverlay::new(&store).unwrap_err().to_string();
        assert!(error.contains("bulk-sync mode"), "{error}");
        store.set_bulk_sync_mode(false);
        assert!(SharedBlockOverlay::new(&store).is_ok());
    }

    /// The coverage contract is written outside the journal: through a
    /// journaled batch it is staged into the commit directly, journals
    /// nothing in any block, and the overlay refuses it as a scoped write.
    #[test]
    fn the_coverage_contract_is_written_outside_the_journal() {
        let (store, _dir) = store();
        let contract = crate::types::EntityStatsUndoContract {
            version: crate::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 7,
            updated_at_block: 1_007,
        };
        let overlay = SharedBlockOverlay::new(&store).unwrap();
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        batch.at(9).put_sync_meta(b"other", b"x");
        batch.at(9).put_entity_stats_undo_contract(&contract);
        let mut counters = HashMap::new();
        let stats = batch
            .stage_journal(&mut |block| journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(stats.entries, 1, "only the other key is journaled");
        assert_eq!(
            journal_rows(&store),
            vec![(9, CF_SYNC_META.into(), b"other".to_vec(), None)]
        );
        assert_eq!(
            store.get_entity_stats_undo_contract().unwrap(),
            Some(contract)
        );

        let overlay = SharedBlockOverlay::new(&store).unwrap();
        let error = overlay
            .with(|o| {
                o.write(
                    Some(10),
                    CF_SYNC_META,
                    keys::sync_meta_keys::ENTITY_STATS_UNDO_CONTRACT,
                    Some(vec![0]),
                    0,
                )
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside the block journal"), "{error}");
        assert!(is_outside_journal(
            CF_SYNC_META,
            keys::sync_meta_keys::ENTITY_STATS_UNDO_CONTRACT
        ));
        assert!(!is_outside_journal(CF_SYNC_META, b"sync_status"));
    }
}
