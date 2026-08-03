#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_is_multiple_of)]

use std::collections::HashMap;
use std::sync::Arc;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{UndoLogEntry, UndoLogStoreTarget};
use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;
use crate::sync::types::UndoSeqScope;
use crate::sync::undo::next_undo_seq;

#[derive(Clone)]
pub struct BatchWriter {
    pub(super) store: Arc<CkbadgerStore>,
    pub(super) append_only_store: Arc<CkbadgerStore>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
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
        undo_seq: &mut HashMap<i64, u64>,
    ) {
        if self.store.is_bulk_sync_mode() {
            return;
        }
        let seq = next_undo_seq(undo_seq, block_number, UndoSeqScope::Object);
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

/// Guard for identity item ids that must be recorded in the spore-outpoint
/// reverse index (`SPORE_OUTPOINT_BY_ID`).
///
/// Identity item ids are the type-script args verbatim and the identity store
/// itself is width-agnostic (`CF_IDENTITY_DATA` and `identity_by_collection`
/// both key on the raw id). The spore-outpoint reverse index, however, encodes
/// a FIXED 32-byte id component, and it is what backs the per-item lifecycle
/// (`/assets/identities/*/items/{id}/activities`).
///
/// Live testnet did:ckb holds real cells with 20-byte args (31 of 421 as of the
/// 2026-08-01 audit), which that index cannot represent. Rather than silently
/// dropping their lifecycle rows (the endpoint would return an empty history
/// for a real item) or letting the key encoder's assert abort the process, this
/// fails fast with the exact context needed to act on it. Widening the index
/// key is a deliberate store-schema decision, not something to patch around
/// here.
pub(crate) fn ensure_outpoint_indexable_item_id(
    item_id: &[u8],
    protocol: &str,
    tx_hash: &[u8],
    output_index: i16,
) -> anyhow::Result<()> {
    const OUTPOINT_INDEX_ID_LEN: usize = 32;
    if item_id.len() != OUTPOINT_INDEX_ID_LEN {
        anyhow::bail!(
            "{protocol} item id width is not representable in the spore-outpoint reverse index: \
             item_id=0x{}, actual_len={}, required_len={}, tx=0x{}, output_index={} — \
             the identity store keys ids verbatim, but SPORE_OUTPOINT_BY_ID encodes a fixed \
             32-byte id; widening that key is a pending store-schema decision",
            hex::encode(item_id),
            item_id.len(),
            OUTPOINT_INDEX_ID_LEN,
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
pub use dao::{DaoWithdrawalContext, DaoWithdrawalContextTrait};
pub use reorg::ReorgResult;
pub use statistics::calculate_knowledge_size;
pub(crate) use statistics::DaoSnapshotBoundary;
pub use statistics::DaoSnapshotInput;
