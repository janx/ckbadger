use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Serialize;
// ── UndoSeqScope constants & enum ──────────────────────────────────────

pub(crate) const UNDO_SEQ_SCOPE_SHIFT: u32 = 48;
pub(crate) const UNDO_SEQ_LOCAL_MAX: u64 = (1u64 << UNDO_SEQ_SCOPE_SHIFT) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct InternId(u32);

impl InternId {
    pub(crate) fn new(index: usize) -> Self {
        Self(
            u32::try_from(index)
                .unwrap_or_else(|_| panic!("intern id overflow: index {index} exceeds u32::MAX")),
        )
    }

    pub(crate) const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum UndoSeqScope {
    TxContext = 0x0001,
    DotBit = 0x0002,
    Object = 0x0003,
    /// Entity daily/hourly stats buckets (`SCRIPT_DAILY`, `TOKEN_DAILY`,
    /// `CLUSTER_DAILY`, `SPORE_DAILY`, `OBJECT_DAILY`, `TOKEN_HOURLY`,
    /// `SPORE_HOURLY`, `OBJECT_HOURLY`). Its own scope so the retention window
    /// can prune exactly these entries without touching the other three.
    EntityStats = 0x0004,
    /// Per-prefix participation counters (`CF_ADDR_PREFIX_STATS`). Rollback
    /// restores them ONLY by replaying these pre-images — stage 8c deletes the
    /// prefix rows but never re-derives the counter from them.
    AddrPrefixStats = 0x0005,
}

// ── Sync / Reorg action enums ──────────────────────────────────────────

pub(crate) enum ReorgAction {
    Handled,
    DeepForkPaused,
}

/// Per-tx .bit activity data for direct collection activity writes.
pub(crate) struct DotbitTxActivityData {
    pub(crate) das_action: Option<String>,
    pub(crate) created_account_ids: HashSet<Vec<u8>>,
    pub(crate) consumed_account_ids: HashSet<Vec<u8>>,
    pub(crate) block_number: i64,
    pub(crate) block_hash: Vec<u8>,
    pub(crate) tx_idx: i32,
    pub(crate) timestamp_ms: i64,
}

// ── XUDT extension ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct XudtExtensionScript {
    pub(crate) args: Vec<u8>,
}

// ── Cell caches ────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct CachedCellInfo {
    pub(crate) capacity: i64,
    pub(crate) created_at_block: i64,
    pub(crate) lock_script_hash: Vec<u8>,
    pub(crate) lock_code_hash: Vec<u8>,
    pub(crate) lock_hash_type: i16,
    pub(crate) lock_args: Vec<u8>,
    pub(crate) type_script_hash: Option<Vec<u8>>,
    pub(crate) type_code_hash: Option<Vec<u8>>,
    pub(crate) type_hash_type: Option<i16>,
    pub(crate) type_args: Option<Vec<u8>>,
    pub(crate) data_size: i32,
    pub(crate) occupied_capacity: i64,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) data_hash: Option<Vec<u8>>,
}

#[derive(Clone)]
pub(crate) struct CachedUdtCellInfo {
    pub(crate) type_script_hash: Vec<u8>,
    pub(crate) type_code_hash: Vec<u8>,
    pub(crate) type_hash_type: i16,
    pub(crate) type_args: Vec<u8>,
    pub(crate) lock_script_hash: Vec<u8>,
    pub(crate) amount: u128,
    pub(crate) standard: String,
}

// ── Transaction data ───────────────────────────────────────────────────

pub(crate) struct TxData {
    pub(crate) hash: [u8; 32],
    pub(crate) block_number: i64,
    pub(crate) tx_index: i32,
    pub(crate) inputs_count: i16,
    pub(crate) outputs_count: i16,
    pub(crate) is_cellbase: bool,
    pub(crate) inputs: Vec<crate::parser::transaction::ParsedInput>,
    pub(crate) cells: Vec<crate::parser::cell::ParsedCell>,
    pub(crate) witnesses: Vec<String>,
    pub(crate) outputs_data: Vec<String>,
    pub(crate) total_input_capacity: i64,
    pub(crate) total_output_capacity: i64,
    pub(crate) fee: i64,
    pub(crate) tx_size: i32,
    pub(crate) cycles: Option<i64>,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) semantic_tags: u16,
}

// ── Address balance delta (accumulated per-batch) ─────────────────────

/// Per-address accumulated changes within a single pipeline batch.
///
/// Tracks both first-seen and last-activity tx references so that new
/// addresses get correct `first_seen_*` values even when the batch
/// contains multiple transactions touching the same address.
#[derive(Debug, Clone)]
pub struct AddressBalanceDelta {
    pub balance_delta: i128,
    pub live_delta: i32,
    pub total_delta: i32,
    pub tx_delta: i64,
    pub used_delta: i128,
    /// Block number of the first transaction touching this address in the batch.
    pub first_seen_block: i64,
    /// Tx hash of the first transaction touching this address in the batch.
    pub first_seen_tx: Vec<u8>,
    /// Block number of the last transaction touching this address in the batch.
    pub last_activity_block: i64,
    /// Tx hash of the last transaction touching this address in the batch.
    pub last_activity_tx: Vec<u8>,
}

// ── Batch write metrics ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BatchWriteMetrics {
    pub(crate) write_ms: f64,
    /// The writer's own pre-batch CPU phase (`t_precompute`). Feeds the health
    /// monitor's `precompute_ms`; before P3.2 that slot was fed a field that
    /// was always 0.
    pub(crate) precompute_ms: f64,
    pub(crate) finalize_ms: f64,
    // ── commit window, split into five non-overlapping parts ────────────
    /// Batch merge + address-balance prefetch + HODL and cell-distribution
    /// tracker preparation (both stage their whole state into the batch).
    pub(crate) commit_prepare_ms: f64,
    /// `materialize_script_versions_and_families`.
    pub(crate) script_rollup_ms: f64,
    /// Append-only cell payload commit **including** its WAL fsync.
    pub(crate) append_only_commit_synced_ms: f64,
    /// The atomic domain batch commit.
    pub(crate) domain_commit_ms: f64,
    /// The whole window, from the first merge to the domain commit returning.
    pub(crate) commit_phase_total_ms: f64,
    /// Bytes the HODL and cell-distribution trackers serialized into
    /// `sync_meta` in this batch. Both write their WHOLE state every batch, so
    /// this is the fixed per-batch cost of the "tracker state matches the
    /// committed tip" recovery rule.
    pub(crate) tracker_state_bytes: usize,
    pub(crate) txs: u64,
    pub(crate) cells: u64,
    pub(crate) inputs: u64,
}

impl BatchWriteMetrics {
    /// The wide commit window under its historical name. Exactly
    /// [`Self::commit_phase_total_ms`] — one stored value, so `db_commit_ms` in
    /// logs and CSVs cannot drift from the split that explains it.
    pub(crate) fn commit_ms(&self) -> f64 {
        self.commit_phase_total_ms
    }
}

// ── Unresolved outpoint probe summaries ────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedLocalProbeSummary {
    pub(crate) sampled: usize,
    pub(crate) live_hits: usize,
    pub(crate) consumed_hits: usize,
    pub(crate) tx_location_hits: usize,
    pub(crate) missing_everywhere: usize,
    pub(crate) store_errors: usize,
    pub(crate) sample_details: Vec<String>,
}

impl UnresolvedLocalProbeSummary {
    pub(crate) fn format_for_log(&self) -> String {
        format!(
            "sampled={} live_hits={} consumed_hits={} tx_location_hits={} missing_everywhere={} store_errors={} sample=[{}]",
            self.sampled,
            self.live_hits,
            self.consumed_hits,
            self.tx_location_hits,
            self.missing_everywhere,
            self.store_errors,
            self.sample_details.join(", ")
        )
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedRpcProbeSummary {
    pub(crate) sampled_tx_hashes: usize,
    pub(crate) committed: usize,
    pub(crate) pending: usize,
    pub(crate) proposed: usize,
    pub(crate) rejected: usize,
    pub(crate) unknown_status: usize,
    pub(crate) rpc_null: usize,
    pub(crate) rpc_errors: usize,
    pub(crate) sample_details: Vec<String>,
}

impl UnresolvedRpcProbeSummary {
    pub(crate) fn format_for_log(&self) -> String {
        format!(
            "sampled_tx_hashes={} committed={} pending={} proposed={} rejected={} unknown_status={} rpc_null={} rpc_errors={} sample=[{}]",
            self.sampled_tx_hashes,
            self.committed,
            self.pending,
            self.proposed,
            self.rejected,
            self.unknown_status,
            self.rpc_null,
            self.rpc_errors,
            self.sample_details.join(", ")
        )
    }
}

// ── Per-block entity daily deltas ──────────────────────────────────────

/// `SCRIPT_DAILY` identity: `(code_hash, hash_type, is_type, date_yyyymmdd)`.
pub type ScriptDailyKey = (Vec<u8>, u8, bool, u32);
/// `TOKEN_DAILY` / `CLUSTER_DAILY` / `SPORE_DAILY` / `OBJECT_DAILY` identity:
/// `(entity_id, date_yyyymmdd)`.
pub type EntityDateKey = (Vec<u8>, u32);

/// Entity daily deltas accumulated **per block**, in ascending block order.
///
/// A committed batch can span thousands of blocks. Flattening every block's
/// contribution into one `HashMap<(entity, date), (i128, i128)>` — which is
/// what the parser used to hand the writer — destroys the block identity, and
/// with it any possibility of recording what a key's value was at the end of
/// block N. A shallow fork landing inside a batch then has nothing to restore.
///
/// Fail fast on out-of-order blocks: the parser walks `all_tx_data`, which is
/// built block by block in ascending order. A block going backwards means that
/// invariant broke upstream, and silently re-opening an earlier block's map
/// would attribute its deltas to the wrong undo entry.
/// One block's contributions: entity key → `(capacity_delta, knowledge_delta)`.
pub type EntityDailyBlockMap<K> = std::collections::HashMap<K, (i128, i128)>;

#[derive(Debug, Clone)]
pub struct EntityDailyChanges<K: Eq + std::hash::Hash> {
    by_block: Vec<(i64, EntityDailyBlockMap<K>)>,
}

impl<K: Eq + std::hash::Hash> Default for EntityDailyChanges<K> {
    fn default() -> Self {
        Self {
            by_block: Vec::new(),
        }
    }
}

impl<K: Eq + std::hash::Hash + Clone> EntityDailyChanges<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one contribution to `key` within `block`.
    pub fn add(
        &mut self,
        block: i64,
        key: K,
        capacity_delta: i128,
        knowledge_delta: i128,
    ) -> anyhow::Result<()> {
        match self.by_block.last() {
            Some((last, _)) if *last > block => {
                anyhow::bail!(
                    "entity daily changes must accumulate in ascending block order: \
                     last_block={last}, block={block}"
                );
            }
            Some((last, _)) if *last == block => {}
            _ => self.by_block.push((block, EntityDailyBlockMap::new())),
        }
        let entry = self
            .by_block
            .last_mut()
            .expect("by_block is non-empty after the push above")
            .1
            .entry(key)
            .or_insert((0, 0));
        entry.0 = entry.0.checked_add(capacity_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "entity daily capacity delta overflow in block {block}: current={}, delta={capacity_delta}",
                entry.0
            )
        })?;
        entry.1 = entry.1.checked_add(knowledge_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "entity daily knowledge delta overflow in block {block}: current={}, delta={knowledge_delta}",
                entry.1
            )
        })?;
        Ok(())
    }

    /// Blocks in ascending order with their per-block contributions.
    pub fn by_block(&self) -> &[(i64, EntityDailyBlockMap<K>)] {
        &self.by_block
    }

    /// Whole-batch totals, for downstream aggregates (cluster capacity, …)
    /// that are batch-scoped. Folded from the same per-block collection — never
    /// a second, independently accumulated copy.
    pub fn fold_total(&self) -> anyhow::Result<EntityDailyBlockMap<K>> {
        let mut total: EntityDailyBlockMap<K> = std::collections::HashMap::new();
        for (block, map) in &self.by_block {
            for (key, (cap, know)) in map {
                let entry = total.entry(key.clone()).or_insert((0, 0));
                entry.0 = entry.0.checked_add(*cap).ok_or_else(|| {
                    anyhow::anyhow!(
                        "entity daily capacity total overflow folding block {block}: current={}, delta={cap}",
                        entry.0
                    )
                })?;
                entry.1 = entry.1.checked_add(*know).ok_or_else(|| {
                    anyhow::anyhow!(
                        "entity daily knowledge total overflow folding block {block}: current={}, delta={know}",
                        entry.1
                    )
                })?;
            }
        }
        Ok(total)
    }

    pub fn is_empty(&self) -> bool {
        self.by_block.iter().all(|(_, map)| map.is_empty())
    }
}

#[cfg(test)]
mod entity_daily_changes_tests {
    use super::*;

    fn key(byte: u8) -> EntityDateKey {
        (vec![byte; 32], 20_260_922)
    }

    #[test]
    fn add_keeps_one_map_per_block_in_ascending_order() {
        let mut changes = EntityDailyChanges::<EntityDateKey>::new();
        changes.add(10, key(0x22), 100, 10).unwrap();
        changes.add(10, key(0x22), 50, 5).unwrap();
        changes.add(10, key(0x23), 7, 1).unwrap();
        changes.add(12, key(0x22), -30, -3).unwrap();

        let blocks = changes.by_block();
        assert_eq!(blocks.len(), 2, "one entry per block that contributed");
        assert_eq!(blocks[0].0, 10);
        assert_eq!(blocks[1].0, 12);
        assert_eq!(blocks[0].1.get(&key(0x22)), Some(&(150i128, 15i128)));
        assert_eq!(blocks[0].1.get(&key(0x23)), Some(&(7i128, 1i128)));
        assert_eq!(
            blocks[1].1.get(&key(0x22)),
            Some(&(-30i128, -3i128)),
            "block 12 holds only its own contribution, not a running total"
        );
    }

    #[test]
    fn add_rejects_a_block_going_backwards() {
        let mut changes = EntityDailyChanges::<EntityDateKey>::new();
        changes.add(10, key(0x22), 1, 1).unwrap();
        let err = changes.add(9, key(0x22), 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("ascending block order"),
            "got: {err}"
        );
    }

    #[test]
    fn add_detects_overflow_on_both_fields() {
        let mut changes = EntityDailyChanges::<EntityDateKey>::new();
        changes.add(1, key(0x22), i128::MAX, 0).unwrap();
        let err = changes.add(1, key(0x22), 1, 0).unwrap_err();
        assert!(
            err.to_string().contains("capacity delta overflow"),
            "got: {err}"
        );

        let mut changes = EntityDailyChanges::<EntityDateKey>::new();
        changes.add(1, key(0x22), 0, i128::MIN).unwrap();
        let err = changes.add(1, key(0x22), 0, -1).unwrap_err();
        assert!(
            err.to_string().contains("knowledge delta overflow"),
            "got: {err}"
        );
    }

    #[test]
    fn fold_total_sums_every_block() {
        let mut changes = EntityDailyChanges::<EntityDateKey>::new();
        changes.add(1, key(0x22), 100, 10).unwrap();
        changes.add(2, key(0x22), -30, -3).unwrap();
        changes.add(2, key(0x23), 5, 1).unwrap();
        let total = changes.fold_total().unwrap();
        assert_eq!(total.get(&key(0x22)), Some(&(70i128, 7i128)));
        assert_eq!(total.get(&key(0x23)), Some(&(5i128, 1i128)));
        assert_eq!(total.len(), 2);
    }

    #[test]
    fn is_empty_only_when_nothing_was_added() {
        let mut changes = EntityDailyChanges::<ScriptDailyKey>::new();
        assert!(changes.is_empty());
        changes
            .add(1, (vec![0x11; 32], 1, false, 20_260_922), 1, 1)
            .unwrap();
        assert!(!changes.is_empty());
    }
}
