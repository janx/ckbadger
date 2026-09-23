#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_is_multiple_of)]

use std::sync::Arc;

use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{UndoLogEntry, UndoLogStoreTarget};
use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;
use crate::sync::types::UndoSeqScope;
use crate::sync::undo::SharedUndoSeq;

#[derive(Clone)]
pub struct BatchWriter {
    pub(super) store: Arc<CkbadgerStore>,
    pub(super) append_only_store: Arc<CkbadgerStore>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
}

/// Whether a rollback entry point may proceed on a store that has no
/// entity-stats coverage contract yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContractRequirement {
    /// Live reorg: startup already refused any non-fresh store without a
    /// contract, so its absence here is an invariant violation.
    Required,
    /// Startup / batch cleanup: a genuinely fresh store has not written its
    /// first contract yet, and has no entity rows to protect either.
    OptionalOnFreshStore,
}

impl BatchWriter {
    /// Refuse any rollback that reaches below the entity-stats coverage floor.
    ///
    /// The eight entity daily/hourly families are restored ONLY by the undo
    /// log, which is pruned behind that floor. Rolling back past it leaves
    /// those rows in place while blocks, cells and indexes are removed, so the
    /// re-sync adds the same deltas a second time — and nothing downstream can
    /// see it, because `find_first_invalid_token_daily_delta` only catches
    /// totals that go negative.
    pub(crate) fn ensure_rollback_within_entity_stats_coverage(
        &self,
        rollback_target: i64,
        requirement: ContractRequirement,
        context: &str,
    ) -> Result<()> {
        let contract = match (self.store.get_entity_stats_undo_contract()?, requirement) {
            (Some(contract), _) => contract,
            (None, ContractRequirement::OptionalOnFreshStore) => return Ok(()),
            (None, ContractRequirement::Required) => {
                return Err(anyhow::Error::new(
                    crate::lifecycle::RebuildRequiredError::new(format!(
                        "{context} {rollback_target}: chain store has no entity stats undo \
                         contract, so nothing states how far its entity daily/hourly stats can \
                         be rolled back"
                    )),
                ));
            }
        };
        if rollback_target < contract.coverage_floor_block {
            return Err(anyhow::Error::new(
                crate::lifecycle::RebuildRequiredError::new(format!(
                    "entity stats undo coverage floor {} is above {context} {}; the eight entity \
                     daily/hourly families cannot be restored that far back, and continuing would \
                     leave them counted twice after re-sync (contract version {}, floor last \
                     advanced at block {})",
                    contract.coverage_floor_block,
                    rollback_target,
                    contract.version,
                    contract.updated_at_block
                )),
            ));
        }
        Ok(())
    }
}

impl BatchWriter {
    pub fn new(store: Arc<CkbadgerStore>, append_only_store: Arc<CkbadgerStore>) -> Self {
        Self {
            store,
            append_only_store,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(
        store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
        cache_invalidator: CacheInvalidator,
    ) -> Self {
        Self {
            store,
            append_only_store,
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    pub fn store(&self) -> &Arc<CkbadgerStore> {
        &self.store
    }

    pub fn append_only_store(&self) -> &CkbadgerStore {
        &self.append_only_store
    }

    /// Record an undo log entry for an object/identity entity mutation.
    /// Captures the previous value so rollback can restore it.
    /// Skipped during bulk sync mode (no undo log needed).
    pub(crate) fn record_object_undo(
        &self,
        batch: &mut StoreBatch,
        block_number: i64,
        cf_name: &'static str,
        key: &[u8],
        previous_value: Option<Vec<u8>>,
        undo_seq: &SharedUndoSeq,
    ) {
        if self.store.is_bulk_sync_mode() {
            return;
        }
        let seq = undo_seq.next(block_number, UndoSeqScope::Object);
        batch.put_reorg_undo_log_by_block(
            block_number,
            seq,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: cf_name.to_string(),
                key: key.to_vec(),
                previous_value,
            },
        );
    }
}

/// Guard for identity item ids that are recorded in the spore-outpoint reverse
/// index (`SPORE_OUTPOINT_BY_ID`), which backs the per-item lifecycle feed
/// (`/assets/identities/*/items/{id}/activities`).
///
/// Item ids are the type-script args verbatim, and every id-keyed store
/// structure — `CF_IDENTITY_DATA`, `identity_by_collection` and the reverse
/// index — stores them at their natural width, so real did:ckb cells index
/// whether their args are 32 bytes (390 of 421 live testnet cells) or 20 bytes
/// (the remaining 31).
///
/// What genuinely cannot be indexed is an id outside `1..=32` bytes: a
/// zero-length id would collapse distinct identities onto one key, and the API
/// caps item ids at 32 bytes (`parse_asset_id_max32`), so a longer id would be
/// indexed but permanently unqueryable. Those fail fast here with locating
/// context rather than reaching the key encoder's process-aborting assert.
pub(crate) fn ensure_outpoint_indexable_item_id(
    item_id: &[u8],
    protocol: &str,
    tx_hash: &[u8],
    output_index: i16,
) -> anyhow::Result<()> {
    if item_id.is_empty() || item_id.len() > ckbadger_store::keys::SPORE_OUTPOINT_BY_ID_MAX_ID_LEN {
        anyhow::bail!(
            "{protocol} item id width is not indexable: item_id=0x{}, actual_len={}, \
             allowed=1..={}, tx=0x{}, output_index={}",
            hex::encode(item_id),
            item_id.len(),
            ckbadger_store::keys::SPORE_OUTPOINT_BY_ID_MAX_ID_LEN,
            hex::encode(tx_hash),
            output_index
        );
    }
    Ok(())
}

pub mod activities;
mod addresses;
pub(crate) mod cell_distribution;
pub(super) mod cells;
mod chain;
pub(crate) mod dao;
pub(crate) mod dotbit;
pub mod entity_stats;
pub(crate) mod fiber;
pub(crate) mod fiber_detector;
pub mod hodl_wave;
mod mnft;
pub(crate) mod object_activity_acc;
mod reorg;
pub(crate) mod rgbpp_detector;
mod spore;
pub(crate) mod stablepp_detector;
mod statistics;
mod sync;
pub(crate) mod udt;
pub(crate) mod utxoswap_detector;

pub use crate::sync::DaoConsumedRow;
pub(crate) use addresses::build_script_reference_rollup_state;
#[cfg(test)]
pub(crate) use addresses::collect_current_script_reference_rollup_state;
pub(crate) use cells::is_cross_store_inconsistency;
pub use dao::{DaoWithdrawalContext, DaoWithdrawalContextTrait};
pub use reorg::ReorgResult;
pub use statistics::calculate_knowledge_size;
pub(crate) use statistics::DaoSnapshotBoundary;
pub use statistics::DaoSnapshotInput;

#[cfg(test)]
mod undo_seq_tests {
    use std::sync::Arc;

    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::CkbadgerStore;

    use crate::parser::mnft::ParsedMnftIssuer;
    use crate::parser::spore::ParsedClusterCell;

    use crate::sync::undo::SharedUndoSeq;

    use super::entity_stats::SharedEntityStatsOverlay;
    use super::BatchWriter;

    /// Task 1.5: `SporeBatchState` and `MnftBatchState` each own a private
    /// `undo_seq_by_block` that starts at 0, and both hand it to
    /// `record_object_undo`, which stamps every entry with the same
    /// `UndoSeqScope::Object`. Two object writes in one block therefore compute
    /// the identical undo key `(block, (0x0003 << 48) | 0)` and the second
    /// silently overwrites the first inside the same `StoreBatch` — one
    /// entity's pre-image is lost before rollback ever runs.
    ///
    /// The undo log must hold one entry per `record_object_undo` call.
    #[test]
    fn object_scope_undo_seq_is_shared_across_entity_batch_states() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        const BLOCK: i64 = 4_242;
        let mut batch = StoreBatch::new(store.as_ref());
        // Exactly what `write_parsed_batch` does: ONE counter for the batch,
        // handed to every entity batch state.
        let batch_undo_seq = SharedUndoSeq::default();
        let mut spore_state =
            writer.new_spore_batch_state(SharedEntityStatsOverlay::new(), batch_undo_seq.clone());
        let mut mnft_state =
            writer.new_mnft_batch_state(SharedEntityStatsOverlay::new(), batch_undo_seq.clone());

        writer
            .insert_spore_cluster(
                &ParsedClusterCell {
                    cluster_id: vec![0x33; 32],
                    type_script_hash: vec![0x34; 32],
                    name: Some("cluster".to_string()),
                    description: Some("task 1.5".to_string()),
                    owner_lock_hash: vec![0x35; 32],
                },
                BLOCK,
                &[0xAA; 32],
                &mut batch,
                &mut spore_state,
            )
            .unwrap();
        writer
            .insert_mnft_issuer(
                &ParsedMnftIssuer {
                    issuer_id: vec![0x55; 20],
                    type_script_hash: vec![0x56; 32],
                    name: Some("issuer".to_string()),
                    info: None,
                    class_count: 0,
                    set_count: 0,
                    owner_lock_hash: vec![0x57; 32],
                },
                &[0xBB; 32],
                0,
                BLOCK,
                &mut batch,
                &mut mnft_state,
            )
            .unwrap();
        batch.commit().unwrap();

        let start = ckbadger_store::keys::encode_reorg_undo_log_key(BLOCK, 0);
        let mut entries = 0usize;
        let iter = store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        );
        for item in iter {
            let (key, _) = item.unwrap();
            let (block, _seq) = ckbadger_store::keys::decode_reorg_undo_log_key(&key);
            if block != BLOCK {
                break;
            }
            entries += 1;
        }

        assert_eq!(
            entries, 2,
            "two object writes in one block recorded {entries} undo entries; each \
             `record_object_undo` call must keep its own pre-image, but the two \
             `*BatchState`s number the `Object` scope independently from 0 and collide"
        );
    }
}
