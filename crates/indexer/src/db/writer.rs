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

/// The bytes an undo entry restores for a row that exists.
///
/// A serialization failure is an error, never "the row did not exist":
/// recorded as `None`, it would make the rollback DELETE a row that had a
/// value — the same class a failed pre-image READ was fixed for (a5241239).
pub(crate) fn undo_pre_image<T: serde::Serialize + ?Sized>(
    value: &T,
    what: &str,
    id: &[u8],
    block_number: i64,
) -> Result<Vec<u8>> {
    bincode::serialize(value).map_err(|e| {
        anyhow::anyhow!(
            "failed to serialize the {what} undo pre-image: id=0x{}, block={}, {e}",
            hex::encode(id),
            block_number
        )
    })
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
    /// the identical undo key `(block, UndoSeqScope::Object.seq_base() | 0)` and the second
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

    /// Plan Task 5.5 (IDX-008 class): a writer that mints its own
    /// `SharedUndoSeq::default()` numbers its undo entries from 0 beside the
    /// batch's counter, so two such writes in one block collide and the second
    /// pre-image overwrites the first. Only `write_parsed_batch` creates the
    /// counter; writer modules take it. Every occurrence in a writer module
    /// must therefore sit in test code, i.e. after the module's first
    /// `#[cfg(test)] mod` (test modules close each file).
    #[test]
    fn no_writer_module_mints_its_own_undo_sequence() {
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
            let production = &src[..test_code_starts];
            assert!(
                !production.contains("SharedUndoSeq::default()"),
                "{name} mints its own undo sequence outside test code"
            );
        }
    }

    /// A value that cannot be serialized fails the write with its entity and
    /// block, instead of becoming a `None` pre-image that tells rollback the
    /// row did not exist.
    #[test]
    fn undo_pre_image_serialization_failure_is_an_error_with_context() {
        struct Poisoned;
        impl serde::Serialize for Poisoned {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("poisoned value"))
            }
        }
        let err = super::undo_pre_image(&Poisoned, "mNFT token", &[0xAB; 4], 777).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mNFT token undo pre-image"), "{msg}");
        assert!(msg.contains("id=0xabababab"), "{msg}");
        assert!(msg.contains("block=777"), "{msg}");
        assert!(msg.contains("poisoned value"), "{msg}");

        assert_eq!(
            super::undo_pre_image(&42u32, "x", &[1], 1).unwrap(),
            bincode::serialize(&42u32).unwrap()
        );
    }

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

    /// Plan Task 5.3: rollback replays a block's undo entries scope-major, which
    /// is exact only while every key the block mutates is recorded by ONE scope.
    /// One block written by the DotBit, Object and EntityStats scopes at once
    /// (a .bit registration, which also bumps the .bit hourly bucket, beside an
    /// mNFT issuer) must keep them disjoint.
    #[test]
    fn one_block_across_dotbit_object_and_entity_stats_scopes_records_disjoint_keys() {
        use crate::parser::dotbit::{ParsedDotbitAccount, ParsedDotbitAccountOutput};
        use crate::sync::types::UndoSeqScope;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        const BLOCK: i64 = 5_151;
        let mut batch = StoreBatch::new(store.as_ref());
        let batch_undo_seq = SharedUndoSeq::default();
        let entity_stats = SharedEntityStatsOverlay::new();
        let mut dotbit_state =
            writer.new_dotbit_batch_state(entity_stats.clone(), batch_undo_seq.clone());
        let mut mnft_state =
            writer.new_mnft_batch_state(entity_stats.clone(), batch_undo_seq.clone());

        writer
            .insert_dotbit_account_with_state(
                &ParsedDotbitAccountOutput {
                    output_index: 0,
                    account: ParsedDotbitAccount {
                        account_id: vec![0x61; 20],
                        account: Some("scopes.bit".to_string()),
                        type_script_hash: vec![0x62; 32],
                        next_account_id: None,
                        expired_at: Some(1_900_000_000),
                        registered_at: Some(1_700_000_000),
                        status: Some(0),
                        owner_lock_hash: vec![0x63; 32],
                    },
                },
                &[0xD1; 32],
                BLOCK,
                1_700_000_000_000,
                &mut batch,
                &mut dotbit_state,
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
        // An mNFT class transfer bumps its hourly bucket through the shared
        // entity-stats overlay: the EntityStats scope.
        entity_stats
            .mutate_hourly(
                store.as_ref(),
                &mut batch,
                &batch_undo_seq,
                BLOCK,
                &ckbadger_store::keys::encode_object_hourly_key(&[0x55; 24], 472_222),
                1,
                &|| "task 5.3 object hourly".to_string(),
            )
            .unwrap();
        entity_stats.stage_final(&mut batch).unwrap();
        batch.commit().unwrap();

        let start = ckbadger_store::keys::encode_reorg_undo_log_key(BLOCK, 0);
        let mut scopes = std::collections::BTreeSet::new();
        for item in store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, _) = item.unwrap();
            let (block, seq) = ckbadger_store::keys::decode_reorg_undo_log_key(&key);
            if block != BLOCK {
                break;
            }
            scopes.insert(seq);
        }
        for scope in [
            UndoSeqScope::DotBit,
            UndoSeqScope::Object,
            UndoSeqScope::EntityStats,
        ] {
            assert!(
                scopes.iter().any(|seq| scope.owns(*seq)),
                "the block must exercise the {scope:?} scope"
            );
        }
        assert!(
            !scopes.iter().any(|seq| UndoSeqScope::TxContext.owns(*seq)),
            "writer-level fixture: no tx contexts"
        );
        assert_eq!(
            crate::sync::undo::undo_scope_overlaps(store.as_ref(), BLOCK, BLOCK).unwrap(),
            Vec::new()
        );
    }
}
