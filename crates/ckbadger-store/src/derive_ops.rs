//! Derived aggregates: persisted rows the indexer derives from rows that
//! already exist, after bulk sync hands off to live sync, and then maintains
//! on the live path only (round 3, lane L).
//!
//! Bulk build never writes these column families (they are `BulkDisabled`),
//! so a derivation costs bulk sync nothing.
//!
//! Every derived aggregate is a **signed sum**: each source row contributes a
//! fixed vector of integers to one or more keys, and a key's value is the sum
//! of the contributions of every source row. That one shape is what makes the
//! derivation exact while live sync runs alongside it: the rebuild adds the
//! contributions of the rows in a RocksDB snapshot taken at tip `H`, and every
//! live block above `H` adds `f(new) - f(old)` for each source row it changes.
//! An intermediate value can go negative (a deposit made at or below `H` and
//! withdrawn above it is subtracted before the rebuild reaches it); the
//! invariants of each aggregate are checked when the derivation completes and
//! on every later live write. A key whose components are all zero has no row.
//!
//! State lives in `sync_meta` under `derive:<name>` (`DeriveMeta`). This module
//! is the store's single definition of each aggregate's name, column family,
//! version and key layout; the indexer computes the rows, the API reads them.

use rocksdb::{IteratorMode, Snapshot};
use serde::{Deserialize, Serialize};

use crate::batch::StoreBatch;
use crate::keys;
use crate::store::{
    CkbadgerStore, KvResult, CF_BIT_CELL_BY_ACCOUNT, CF_DAO_LIVE_AGG,
    CF_OBJECT_COLLECTION_ACTIVITY_COUNT,
};
use crate::types::CachedBlockHeader;

/// Identity of one derived aggregate: its name (also the `derive:<name>` meta
/// key and the `derive:<name>` background task), the one column family it
/// owns, and the version of its row definition. Bumping `version` makes the
/// indexer clear the column family and derive it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedAggregateId {
    pub name: &'static str,
    pub cf_name: &'static str,
    pub version: u32,
}

/// L1: rows over the live NervosDAO cells (phase 0 and phase 1).
pub const DAO_LIVE_AGG: DerivedAggregateId = DerivedAggregateId {
    name: "dao_live_agg",
    cf_name: CF_DAO_LIVE_AGG,
    version: 1,
};

/// L2: `object_collection_activities` rows per padded collection id.
pub const OBJECT_COLLECTION_ACTIVITY_COUNT: DerivedAggregateId = DerivedAggregateId {
    name: "object_collection_activity_count",
    cf_name: CF_OBJECT_COLLECTION_ACTIVITY_COUNT,
    version: 1,
};

/// L3: live `.bit Cell` items per `.bit` account id.
pub const BIT_CELL_BY_ACCOUNT: DerivedAggregateId = DerivedAggregateId {
    name: "bit_cell_by_account",
    cf_name: CF_BIT_CELL_BY_ACCOUNT,
    version: 1,
};

/// Every derived column family. Nothing else may be written through the
/// derived-row helpers below.
pub const DERIVED_CFS: &[&str] = &[
    CF_DAO_LIVE_AGG,
    CF_OBJECT_COLLECTION_ACTIVITY_COUNT,
    CF_BIT_CELL_BY_ACCOUNT,
];

/// Key layout of `dao_live_agg` (L1). Values are signed component vectors;
/// see each kind for its components.
pub mod dao_live_agg {
    /// `[0x00]` → `[phase0 capacity, phase0 cells, phase1 capacity, phase1 cells]`.
    pub const KIND_GLOBAL: u8 = 0x00;
    /// `[0x01] + lock_hash(32)` → `[phase0 capacity, phase0 cells]` of one depositor.
    pub const KIND_DEPOSITOR: u8 = 0x01;
    /// `[0x02] + deposit epoch(16)` → `[phase0 capacity, phase0 cells]` deposited at that
    /// exact epoch fraction (what `/dao/evidence` buckets into its 30 nominal days).
    pub const KIND_PHASE0_EPOCH: u8 = 0x02;
    /// `[0x03] + deposit epoch(16) + request epoch(16)` → `[phase1 capacity, phase1 cells]`
    /// (what `/dao/evidence` splits into ready / waiting by epoch lock).
    pub const KIND_PHASE1_EPOCHS: u8 = 0x03;

    pub const EPOCH_KEY_LEN: usize = 16;

    /// One exact epoch position `number + index/length` of a block header.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct EpochPoint {
        pub number: i64,
        pub index: i64,
        pub length: i64,
    }

    impl EpochPoint {
        pub fn new(number: i64, index: i64, length: i64) -> anyhow::Result<Self> {
            anyhow::ensure!(
                number >= 0
                    && length > 0
                    && index >= 0
                    && index < length
                    && number <= i64::from(u32::MAX)
                    && length <= i64::from(u32::MAX),
                "invalid DAO epoch {number}+{index}/{length}"
            );
            Ok(Self {
                number,
                index,
                length,
            })
        }

        /// `number` (u64 BE) + `index` (u32 BE) + `length` (u32 BE).
        pub fn encode(&self) -> [u8; EPOCH_KEY_LEN] {
            let mut out = [0u8; EPOCH_KEY_LEN];
            out[..8].copy_from_slice(&(self.number as u64).to_be_bytes());
            out[8..12].copy_from_slice(&(self.index as u32).to_be_bytes());
            out[12..16].copy_from_slice(&(self.length as u32).to_be_bytes());
            out
        }

        pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
            anyhow::ensure!(
                bytes.len() == EPOCH_KEY_LEN,
                "DAO epoch key must be {EPOCH_KEY_LEN} bytes, got {}",
                bytes.len()
            );
            let number = u64::from_be_bytes(bytes[..8].try_into().expect("8 bytes"));
            let index = u32::from_be_bytes(bytes[8..12].try_into().expect("4 bytes"));
            let length = u32::from_be_bytes(bytes[12..16].try_into().expect("4 bytes"));
            Self::new(
                i64::try_from(number)
                    .map_err(|_| anyhow::anyhow!("DAO epoch number overflows i64"))?,
                i64::from(index),
                i64::from(length),
            )
        }
    }

    pub fn global_key() -> Vec<u8> {
        vec![KIND_GLOBAL]
    }

    pub fn depositor_key(lock_hash: &[u8]) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            lock_hash.len() == 32,
            "DAO depositor lock hash must be 32 bytes, got {}",
            lock_hash.len()
        );
        let mut key = Vec::with_capacity(33);
        key.push(KIND_DEPOSITOR);
        key.extend_from_slice(lock_hash);
        Ok(key)
    }

    pub fn phase0_epoch_key(deposit: EpochPoint) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + EPOCH_KEY_LEN);
        key.push(KIND_PHASE0_EPOCH);
        key.extend_from_slice(&deposit.encode());
        key
    }

    pub fn phase1_epochs_key(deposit: EpochPoint, request: EpochPoint) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + 2 * EPOCH_KEY_LEN);
        key.push(KIND_PHASE1_EPOCHS);
        key.extend_from_slice(&deposit.encode());
        key.extend_from_slice(&request.encode());
        key
    }
}

/// Upper bound passed to `delete_range` when a derivation clears its column
/// family: longer than any derived key, so it sorts after every one of them.
const DERIVED_CLEAR_END: [u8; 128] = [0xFF; 128];

/// Where one aggregate's derivation stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeriveState {
    /// Rows are absent or garbage; nothing reads them and live sync does not
    /// maintain them.
    NotStarted,
    /// A rebuild is in flight over a snapshot taken at `derived_from_tip`.
    /// Live blocks above it are applied as signed deltas meanwhile. `cursor`
    /// is the last source key folded (progress only: an interrupted rebuild
    /// lost its snapshot and starts over).
    Running {
        derived_from_tip: i64,
        cursor: Option<Vec<u8>>,
        rows_done: u64,
    },
    /// The rows equal a from-scratch derivation at the sync tip. They became
    /// exact at `completed_at_tip`: undo entries recorded at or below it were
    /// recorded while rebuild chunks still wrote, so a rollback below it
    /// restarts the derivation instead of replaying them.
    Complete {
        derived_from_tip: i64,
        completed_at_tip: i64,
        rows_done: u64,
    },
    /// The rebuild or a live write found an invariant violation. The rows are
    /// not served; the error is. The indexer retries once per process start.
    Failed { error: String },
}

/// The persisted derivation state of one aggregate (`sync_meta`
/// `derive:<name>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeriveMeta {
    pub version: u32,
    pub state: DeriveState,
    /// Increments on every start, so a scheduler can tell its own rebuild from
    /// one an earlier process (or a rollback) abandoned.
    pub run: u64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

impl DeriveMeta {
    pub fn not_started(version: u32, run: u64) -> Self {
        Self {
            version,
            state: DeriveState::NotStarted,
            run,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    /// Whether live sync maintains this aggregate's rows.
    pub fn is_maintained(&self, id: &DerivedAggregateId) -> bool {
        self.version == id.version
            && matches!(
                self.state,
                DeriveState::Running { .. } | DeriveState::Complete { .. }
            )
    }

    /// Whether the rows may be served.
    pub fn is_complete(&self, id: &DerivedAggregateId) -> bool {
        self.version == id.version && matches!(self.state, DeriveState::Complete { .. })
    }

    pub fn state_name(&self) -> &'static str {
        match self.state {
            DeriveState::NotStarted => "not_started",
            DeriveState::Running { .. } => "running",
            DeriveState::Complete { .. } => "complete",
            DeriveState::Failed { .. } => "failed",
        }
    }

    pub fn rows_done(&self) -> Option<u64> {
        match self.state {
            DeriveState::Running { rows_done, .. } | DeriveState::Complete { rows_done, .. } => {
                Some(rows_done)
            }
            DeriveState::NotStarted | DeriveState::Failed { .. } => None,
        }
    }
}

/// A derived row's value: a fixed vector of signed components, 16 bytes
/// little-endian each.
pub fn encode_derived_value(components: &[i128]) -> Vec<u8> {
    let mut out = Vec::with_capacity(components.len() * 16);
    for component in components {
        out.extend_from_slice(&component.to_le_bytes());
    }
    out
}

pub fn decode_derived_value(bytes: &[u8]) -> anyhow::Result<Vec<i128>> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(16),
        "derived row value is not a vector of 16-byte components: len={}",
        bytes.len()
    );
    Ok(bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|chunk| i128::from_le_bytes(*chunk))
        .collect())
}

fn ensure_derived_cf(cf_name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        DERIVED_CFS.contains(&cf_name),
        "'{cf_name}' is not a derived aggregate column family"
    );
    Ok(())
}

impl CkbadgerStore {
    pub fn get_derive_meta(&self, name: &str) -> anyhow::Result<Option<DeriveMeta>> {
        let key = keys::encode_derive_meta_key(name);
        match self.get_cf(self.cf_sync_meta(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!("failed to decode derive meta for aggregate '{name}': {e}")
            })?)),
            None => Ok(None),
        }
    }

    /// One derived row, decoded.
    pub fn get_derived_value(
        &self,
        cf_name: &str,
        key: &[u8],
    ) -> anyhow::Result<Option<Vec<i128>>> {
        ensure_derived_cf(cf_name)?;
        self.get_cf(self.cf(cf_name), key)?
            .map(|value| {
                decode_derived_value(&value).map_err(|e| {
                    anyhow::anyhow!("{cf_name} key=0x{}: {e}", crate::bytes_to_hex(key))
                })
            })
            .transpose()
    }

    /// Every derived row whose key starts with `prefix`, in key order.
    pub fn scan_derived_prefix<F>(
        &self,
        cf_name: &str,
        prefix: &[u8],
        mut visit: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], Vec<i128>) -> anyhow::Result<()>,
    {
        ensure_derived_cf(cf_name)?;
        let iter = self.iterator_cf(
            self.cf(cf_name),
            IteratorMode::From(prefix, rocksdb::Direction::Forward),
        );
        for item in iter {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("failed to iterate {cf_name}: {e}"))?;
            if !key.starts_with(prefix) {
                break;
            }
            let value = decode_derived_value(&value).map_err(|e| {
                anyhow::anyhow!("{cf_name} key=0x{}: {e}", crate::bytes_to_hex(&key))
            })?;
            visit(&key, value)?;
        }
        Ok(())
    }

    /// A read-only RocksDB snapshot of this primary store, for a derivation's
    /// rebuild. A secondary cannot snapshot.
    pub fn read_snapshot(&self) -> anyhow::Result<ReadSnapshot<'_>> {
        anyhow::ensure!(
            !self.is_secondary(),
            "a derivation snapshot needs the primary store; secondaries cannot snapshot"
        );
        Ok(ReadSnapshot {
            store: self,
            snapshot: self.snapshot(),
        })
    }
}

impl StoreBatch<'_> {
    pub fn put_derive_meta(&mut self, name: &str, meta: &DeriveMeta) {
        let value = bincode::serialize(meta).expect("serialize DeriveMeta");
        self.put_sync_meta(&keys::encode_derive_meta_key(name), &value);
    }

    /// Set (`Some`) or remove (`None`, an all-zero sum) one derived row.
    pub fn put_derived_value(
        &mut self,
        cf_name: &str,
        key: &[u8],
        value: Option<&[i128]>,
    ) -> anyhow::Result<()> {
        ensure_derived_cf(cf_name)?;
        match value {
            Some(components) => {
                self.put_raw_cf_by_name(cf_name, key, &encode_derived_value(components))
            }
            None => self.delete_raw_cf_by_name(cf_name, key),
        }
    }

    /// Remove every row of one derived column family.
    pub fn clear_derived_cf(&mut self, cf_name: &str) -> anyhow::Result<()> {
        ensure_derived_cf(cf_name)?;
        self.delete_range_raw_cf_by_name(cf_name, &[], &DERIVED_CLEAR_END)
    }
}

/// One RocksDB snapshot of the primary domain store: what a derivation's
/// rebuild reads its source rows through, so live blocks committed after it
/// are excluded however long the rebuild runs.
pub struct ReadSnapshot<'a> {
    store: &'a CkbadgerStore,
    snapshot: Snapshot<'a>,
}

impl<'a> ReadSnapshot<'a> {
    pub fn store(&self) -> &'a CkbadgerStore {
        self.store
    }

    pub fn get(&self, cf_name: &str, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        let cf = self
            .store
            .cf_handle_by_name(cf_name)
            .ok_or_else(|| anyhow::anyhow!("CF '{cf_name}' not found for snapshot read"))?;
        self.snapshot
            .get_cf(cf, key)
            .map_err(|e| anyhow::anyhow!("snapshot read failed: cf={cf_name}, error={e}"))
    }

    /// Rows of `cf_name` from `start` (inclusive), in key order.
    pub fn iter_from(
        &self,
        cf_name: &str,
        start: &[u8],
    ) -> anyhow::Result<impl Iterator<Item = KvResult> + '_> {
        let cf = self
            .store
            .cf_handle_by_name(cf_name)
            .ok_or_else(|| anyhow::anyhow!("CF '{cf_name}' not found for snapshot iteration"))?;
        Ok(self
            .snapshot
            .iterator_cf(cf, IteratorMode::From(start, rocksdb::Direction::Forward)))
    }

    pub fn get_block_header(&self, block_number: i64) -> anyhow::Result<Option<CachedBlockHeader>> {
        self.get(
            crate::store::CF_BLOCK_HEADERS,
            &keys::encode_block_num(block_number),
        )?
        .map(|value| {
            bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!("cannot decode snapshot block header at block {block_number}: {e}")
            })
        })
        .transpose()
    }

    /// The highest committed block in this snapshot (`None` on an empty store).
    pub fn tip(&self) -> anyhow::Result<Option<(i64, CachedBlockHeader)>> {
        let cf = self.store.cf_block_headers();
        for item in self.snapshot.iterator_cf(cf, IteratorMode::End) {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("failed to iterate snapshot headers: {e}"))?;
            if key.len() == 8 {
                let number = keys::decode_block_num(&key);
                let header = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!("cannot decode snapshot tip header at block {number}: {e}")
                })?;
                return Ok(Some((number, header)));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dao_epoch_keys_round_trip_and_reject_invalid_fractions() {
        use dao_live_agg::EpochPoint;
        let point = EpochPoint::new(185, 2, 4).unwrap();
        assert_eq!(EpochPoint::decode(&point.encode()).unwrap(), point);
        assert!(EpochPoint::new(1, 4, 4).is_err());
        assert!(EpochPoint::new(1, 0, 0).is_err());
        assert!(EpochPoint::new(-1, 0, 1).is_err());
        let key = dao_live_agg::phase1_epochs_key(point, EpochPoint::new(200, 0, 1).unwrap());
        assert_eq!(key.len(), 33);
        assert_eq!(key[0], dao_live_agg::KIND_PHASE1_EPOCHS);
        assert!(dao_live_agg::depositor_key(&[0; 31]).is_err());
    }

    #[test]
    fn derived_values_round_trip_and_reject_ragged_bytes() {
        let value = vec![-5i128, 0, i128::MAX];
        assert_eq!(
            decode_derived_value(&encode_derived_value(&value)).unwrap(),
            value
        );
        assert!(decode_derived_value(&[]).is_err());
        assert!(decode_derived_value(&[0u8; 17]).is_err());
    }

    #[test]
    fn derived_row_helpers_refuse_a_non_derived_cf() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        let err = batch
            .put_derived_value(crate::store::CF_DAO_DEPOSITS, b"k", Some(&[1]))
            .unwrap_err();
        assert!(err.to_string().contains("not a derived"), "{err}");
        assert!(batch.clear_derived_cf(crate::store::CF_CELLS).is_err());
    }

    #[test]
    fn clear_removes_every_row_and_meta_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        for key in [vec![0x00], vec![0xFF; 52], vec![0x01; 33]] {
            batch
                .put_derived_value(CF_DAO_LIVE_AGG, &key, Some(&[1, 2]))
                .unwrap();
        }
        let meta = DeriveMeta {
            version: 3,
            state: DeriveState::Running {
                derived_from_tip: 7,
                cursor: Some(vec![1, 2]),
                rows_done: 9,
            },
            run: 2,
            started_at_ms: Some(1),
            finished_at_ms: None,
        };
        batch.put_derive_meta("dao_live_agg", &meta);
        batch.commit().unwrap();
        assert_eq!(store.get_derive_meta("dao_live_agg").unwrap(), Some(meta));
        assert_eq!(
            store
                .get_derived_value(CF_DAO_LIVE_AGG, &[0xFF; 52])
                .unwrap(),
            Some(vec![1, 2])
        );

        let mut batch = StoreBatch::new(&store);
        batch.clear_derived_cf(CF_DAO_LIVE_AGG).unwrap();
        batch.commit().unwrap();
        let mut left = 0;
        store
            .scan_derived_prefix(CF_DAO_LIVE_AGG, &[], |_, _| {
                left += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_snapshot_excludes_writes_committed_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch
            .put_derived_value(CF_DAO_LIVE_AGG, b"a", Some(&[1]))
            .unwrap();
        batch.commit().unwrap();
        let snapshot = store.read_snapshot().unwrap();
        let mut batch = StoreBatch::new(&store);
        batch
            .put_derived_value(CF_DAO_LIVE_AGG, b"b", Some(&[2]))
            .unwrap();
        batch.commit().unwrap();
        let keys: Vec<Vec<u8>> = snapshot
            .iter_from(CF_DAO_LIVE_AGG, &[])
            .unwrap()
            .map(|item| item.unwrap().0.to_vec())
            .collect();
        assert_eq!(keys, vec![b"a".to_vec()]);
        assert!(snapshot.tip().unwrap().is_none());
    }
}
