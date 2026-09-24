//! "Does this address exist?" — asked in one place.
//!
//! An address is a derived semantic concept: a lock script that owns cells and
//! appears in transactions (`docs/prompts/DATA_DESIGN.md`). Whether ckbadger
//! knows anything about one has two independent sources — the chain view the
//! indexer wrote, and the node's transaction pool, which holds transitions
//! consensus has not confirmed. Both are consulted here, and the two are never
//! summed: chain presence is reported as cells, pool presence as pending
//! transactions.

use ckbadger_store::CkbadgerStore;

use crate::pool::PoolMirror;

/// What ckbadger knows about an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressPresence {
    /// The chain has this address: it owns or has owned cells.
    OnChain { cells: i64, txs: i64 },
    /// Nothing on chain, but the node's pool holds transactions involving it —
    /// typically a brand-new address receiving its first payment.
    PoolOnly { pending: usize },
    /// Neither source knows it. The address is still valid and its page still
    /// renders; it simply has no history yet.
    None,
}

impl AddressPresence {
    /// Whether any source knows this address.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// The search-result label. Pool presence is reported next to the (zero)
    /// cell count, never folded into it.
    pub fn label(&self) -> String {
        match self {
            Self::OnChain { cells, .. } => format!("Address ({cells} cells)"),
            Self::PoolOnly { pending } => format!("Address (0 cells, {pending} pending)"),
            Self::None => "Address (no on-chain activity)".to_string(),
        }
    }
}

/// Resolve an address's presence from the chain view and the pool mirror.
///
/// Blocking: reads the store. Call it inside `spawn_blocking`.
pub fn address_presence(
    store: &CkbadgerStore,
    mirror: &PoolMirror,
    lock_hash: &[u8],
) -> anyhow::Result<AddressPresence> {
    if let Some(balance) = store.get_addr_balance(lock_hash)? {
        if balance.total_cells_count > 0 || balance.txs_count > 0 || balance.balance > 0 {
            return Ok(AddressPresence::OnChain {
                cells: balance.total_cells_count,
                txs: balance.txs_count,
            });
        }
    }

    // Pool presence only from a mirror that can currently see the node: an
    // unhealthy snapshot keeps the records it last observed, and "n pending"
    // read off a node we cannot reach is the claim its flag exists to stop.
    if mirror.enabled() {
        let snapshot = mirror.load();
        if snapshot.status.healthy {
            let pending = snapshot.pending_count_for_lock(lock_hash);
            if pending > 0 {
                return Ok(AddressPresence::PoolOnly { pending });
            }
        }
    }

    Ok(AddressPresence::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_labels_report_chain_and_pool_presence_separately() {
        assert_eq!(
            AddressPresence::OnChain { cells: 3, txs: 5 }.label(),
            "Address (3 cells)"
        );
        assert_eq!(
            AddressPresence::PoolOnly { pending: 1 }.label(),
            "Address (0 cells, 1 pending)"
        );
        assert_eq!(
            AddressPresence::None.label(),
            "Address (no on-chain activity)"
        );
    }

    #[test]
    fn test_only_none_is_unknown() {
        assert!(AddressPresence::OnChain { cells: 0, txs: 1 }.is_known());
        assert!(AddressPresence::PoolOnly { pending: 2 }.is_known());
        assert!(!AddressPresence::None.is_known());
    }

    /// A snapshot from a mirror that cannot reach the node still holds the
    /// records it last observed — but "1 pending" read off a dead node is
    /// exactly the claim the unhealthy flag exists to prevent.
    #[test]
    fn unhealthy_mirror_never_claims_pending_presence() {
        use crate::pool::{
            Interpretation, MirrorStatus, PoolEntryMeta, PoolParticipant, PoolSnapshot, PoolStatus,
            PoolTxRecord,
        };
        use ckbadger_store::types::{AddrTxValue, ParticipantId};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0xAA; 32];
        let record = Arc::new(PoolTxRecord {
            tx_hash: [0x01; 32],
            pool_status: PoolStatus::Pending,
            entry: PoolEntryMeta {
                fee: 1_000,
                size: 500,
                cycles: 1,
                ancestors_count: 0,
                time_added_to_pool_ms: 1_700_000_000_000,
            },
            outputs: vec![],
            actions: None,
            participants: vec![PoolParticipant {
                id: ParticipantId::Lock(lock),
                addr_tx: AddrTxValue::new(100, false, true, 0),
            }],
            inputs_count: 1,
            outputs_count: 1,
            semantic_tags: 0,
            is_cellbase: false,
            interpretation: Interpretation::Complete,
            first_seen_ms: 1_700_000_000_000,
            last_seen_ms: 1_700_000_000_000,
        });
        let mirror = PoolMirror::new(true);
        let snapshot = |healthy: bool| {
            PoolSnapshot::from_records(
                vec![record.clone()],
                MirrorStatus {
                    enabled: true,
                    healthy,
                    ..MirrorStatus::default()
                },
            )
        };

        mirror.publish(snapshot(true));
        assert_eq!(
            address_presence(&store, &mirror, &lock).unwrap(),
            AddressPresence::PoolOnly { pending: 1 }
        );

        mirror.publish(snapshot(false));
        assert_eq!(
            address_presence(&store, &mirror, &lock).unwrap(),
            AddressPresence::None,
            "an unhealthy mirror must not claim pool presence"
        );
    }

    #[test]
    fn test_a_disabled_mirror_contributes_no_presence() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mirror = PoolMirror::disabled();
        assert_eq!(
            address_presence(&store, &mirror, &[0xAA; 32]).unwrap(),
            AddressPresence::None
        );
    }
}
