#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_is_multiple_of)]

use std::sync::Arc;

use anyhow::Result;

use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;

pub use dotcell_detector::{dotcell_asset_action, DOTCELL_RING_RELINK_ACTION};

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
pub(crate) mod dotbit_detector;
pub(crate) mod dotcell;
pub(crate) mod dotcell_detector;
pub mod entity_stats;
pub(crate) mod fiber;
pub(crate) mod fiber_detector;
pub mod hodl_wave;
mod mnft;
pub(crate) mod object_activity_acc;
pub mod participant_rows;
mod reorg;
pub(crate) mod rgbpp_detector;
mod spore;
pub(crate) mod stablepp_detector;
mod statistics;
mod sync;
pub(crate) mod udt;
pub(crate) mod utxoswap_detector;

pub use crate::sync::DaoConsumedRow;
pub(crate) use addresses::{
    build_script_reference_rollup_state_with_lineage, family_versions_counts, reference_binary,
    TypeIdCodeChange, VersionLabels,
};
pub(crate) use cells::is_cross_store_inconsistency;
pub use dao::DaoTransition;
pub(crate) use dao::LiveDaoBatch;
pub use reorg::ReorgResult;
pub use spore::IdentityTransition;
pub use statistics::calculate_knowledge_size;
pub(crate) use statistics::DaoSnapshotBoundary;
pub use statistics::FrozenActivityAddrSets;

/// Test support: the block journal a live batch committed (store refactor
/// lane B).
#[cfg(test)]
pub(crate) mod journal_test_support {
    use ckbadger_store::CkbadgerStore;

    /// A header for each of `blocks`, each written in its block's journaled
    /// scope (its `block_hash_index` row is journaled), so a fixture whose
    /// rows are journaled at those blocks can be rolled back
    /// (`CkbadgerStore::rollback_to_block` rolls back the blocks up to the
    /// highest stored header).
    pub(crate) fn journaled_headers(store: &CkbadgerStore, blocks: std::ops::RangeInclusive<i64>) {
        for block in blocks {
            let mut hash = vec![0x4b; 32];
            hash[..8].copy_from_slice(&block.to_be_bytes());
            let header = ckbadger_store::CachedBlockHeader {
                hash,
                parent_hash: vec![0; 32],
                timestamp: 1_700_000_000_000 + block * 1_000,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1_800,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: 1_700_000_000_000 + block * 1_000,
            };
            ckbadger_store::commit_journaled_block(store, block, |batch| {
                batch.put_block_header(block, &header)
            })
            .expect("journaled fixture header");
        }
    }

    /// Every block journal entry of `cf`, as `(block, key, previous value)`,
    /// sorted.
    pub(crate) fn journal_entries(
        store: &CkbadgerStore,
        cf: &str,
    ) -> Vec<(i64, Vec<u8>, Option<Vec<u8>>)> {
        let mut out = Vec::new();
        for item in store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        ) {
            let (key, value) = item.expect("iterate the undo log");
            let (block, seq) = ckbadger_store::keys::decode_reorg_undo_log_key(&key);
            if !ckbadger_store::keys::UndoSeqScope::Journal.owns(seq) {
                continue;
            }
            let entry = ckbadger_store::decode_journal_value(&value).expect("decode a journal row");
            if entry.cf == cf {
                out.push((
                    block,
                    entry.key.to_vec(),
                    entry.previous.map(<[u8]>::to_vec),
                ));
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod writer_module_tests {
    /// No writer module turns a failed pre-image serialization into an absent
    /// row (`bincode::serialize(..).ok()`) in production code.
    #[test]
    fn no_writer_module_drops_a_pre_image_serialization_error() {
        let modules: [(&str, &str); 24] = [
            ("activities.rs", include_str!("writer/activities.rs")),
            ("addresses.rs", include_str!("writer/addresses.rs")),
            (
                "cell_distribution.rs",
                include_str!("writer/cell_distribution.rs"),
            ),
            ("cells.rs", include_str!("writer/cells.rs")),
            ("chain.rs", include_str!("writer/chain.rs")),
            ("dao.rs", include_str!("writer/dao.rs")),
            ("dotbit.rs", include_str!("writer/dotbit.rs")),
            (
                "dotcell_detector.rs",
                include_str!("writer/dotcell_detector.rs"),
            ),
            ("dotcell.rs", include_str!("writer/dotcell.rs")),
            ("entity_stats.rs", include_str!("writer/entity_stats.rs")),
            (
                "fiber_detector.rs",
                include_str!("writer/fiber_detector.rs"),
            ),
            ("fiber.rs", include_str!("writer/fiber.rs")),
            ("hodl_wave.rs", include_str!("writer/hodl_wave.rs")),
            ("mnft.rs", include_str!("writer/mnft.rs")),
            (
                "object_activity_acc.rs",
                include_str!("writer/object_activity_acc.rs"),
            ),
            (
                "participant_rows.rs",
                include_str!("writer/participant_rows.rs"),
            ),
            ("reorg.rs", include_str!("writer/reorg.rs")),
            (
                "rgbpp_detector.rs",
                include_str!("writer/rgbpp_detector.rs"),
            ),
            ("spore.rs", include_str!("writer/spore.rs")),
            (
                "stablepp_detector.rs",
                include_str!("writer/stablepp_detector.rs"),
            ),
            ("statistics.rs", include_str!("writer/statistics.rs")),
            ("sync.rs", include_str!("writer/sync.rs")),
            ("udt.rs", include_str!("writer/udt.rs")),
            (
                "utxoswap_detector.rs",
                include_str!("writer/utxoswap_detector.rs"),
            ),
        ];
        for (name, src) in modules {
            let test_code_starts = src.find("#[cfg(test)]\nmod ").unwrap_or(src.len());
            for (i, line) in src[..test_code_starts].lines().enumerate() {
                assert!(
                    !(line.contains("serialize(") && line.contains(".ok()")),
                    "{name}:{} drops a serialization error: {}",
                    i + 1,
                    line.trim()
                );
            }
        }
    }
}

// Shared read contract: item and collection feeds classify the same persisted action.
