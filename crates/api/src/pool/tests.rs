//! Mirror state-machine tests.
//!
//! Every transition the design defines is asserted here against a scripted
//! [`FakePoolSource`] and a real (empty) store, because the mirror's whole job
//! is to be right about *when* a transaction appears, changes status and
//! disappears.

use std::sync::Arc;

use ckb_store_reader::{RpcCellInput, RpcCellOutput, RpcOutPoint, RpcScript, RpcTransactionView};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::TxIndexEntry;
use ckbadger_store::CkbadgerStore;

use super::mirror::{PoolMirror, PoolRefresher, PoolRefresherConfig};
use super::resolve::{resolve_pool_tx, ResolvedCell};
use super::snapshot::{Interpretation, PartialReason, PoolStatus};
use super::source::{
    FakePoolSource, NodeLiveCell, NodeScript, NodeTxStatus, PoolEntryMeta, PoolTxLookup, RawTxPool,
    TxPoolInfo,
};

/// secp256k1-blake160, a standard lock: the activity builder records no
/// lock_call for it, keeping fixtures focused on positions.
const SECP_LOCK_CODE_HASH: &str =
    "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
const SUDT_CODE_HASH: &str = "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";

// Mainnet `.cell` (Cells) deployment, and the two states of `abuse.cell` in
// transaction 0x53a0519e06fc4aac3eca2606e12fc19849919a226af872f687dfb3209a0470fd
// at block 20,516,391 — a transfer from the operator's secp key to its JoyID.
// Fetched 2026-09-24 from a local mainnet node.
const DOTCELL_ACCOUNT_CODE_HASH: &str =
    "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54";
const DOTCELL_ACCOUNT_LOCK_CODE_HASH: &str =
    "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab";
const DOTCELL_NAMESPACE_ARGS: &str = "0xb4f4302965b7d6421481a520ee7eb5971a5e808c";
/// `abuse.cell` before the transfer: owner and manager `0x57d926a4…c867`.
const DOTCELL_ABUSE_BEFORE: &str = "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000c6c2916c0057d926a44d83fc13b21ce037b1e31f4223e3c86757d926a44d83fc13b21ce037b1e31f4223e3c8676162757365";
/// And after: owner and manager `0xac55d7da…8182`.
const DOTCELL_ABUSE_AFTER: &str = "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000c6c2916c00ac55d7dab2e9a4b85775a811bb4063e94cc98182ac55d7dab2e9a4b85775a811bb4063e94cc981826162757365";
const DOTCELL_ABUSE_OWNER_BEFORE: [u8; 20] = [
    0x57, 0xd9, 0x26, 0xa4, 0x4d, 0x83, 0xfc, 0x13, 0xb2, 0x1c, 0xe0, 0x37, 0xb1, 0xe3, 0x1f, 0x42,
    0x23, 0xe3, 0xc8, 0x67,
];
const DOTCELL_ABUSE_OWNER_AFTER: [u8; 20] = [
    0xac, 0x55, 0xd7, 0xda, 0xb2, 0xe9, 0xa4, 0xb8, 0x57, 0x75, 0xa8, 0x11, 0xbb, 0x40, 0x63, 0xe9,
    0x4c, 0xc9, 0x81, 0x82,
];

fn hex32(byte: u8) -> String {
    format!("0x{}", hex::encode([byte; 32]))
}

fn script(code_hash: &str, args_byte: u8) -> RpcScript {
    RpcScript {
        code_hash: code_hash.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode([args_byte; 20])),
    }
}

fn node_script(code_hash: &str, args_byte: u8) -> NodeScript {
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hex::decode(code_hash.trim_start_matches("0x")).unwrap());
    NodeScript {
        code_hash: bytes,
        hash_type: 1,
        args: vec![args_byte; 20],
    }
}

/// The lock hash the activity builder will key a participant by.
fn lock_hash(code_hash: &str, args_byte: u8) -> [u8; 32] {
    let code_hash_bytes = hex::decode(code_hash.trim_start_matches("0x")).unwrap();
    let hash = crate::utils::address::compute_script_hash(&code_hash_bytes, 1, &[args_byte; 20]);
    <[u8; 32]>::try_from(hash.as_slice()).unwrap()
}

struct TxBuilder {
    hash: [u8; 32],
    inputs: Vec<((String, u32), String)>,
    outputs: Vec<(u64, RpcScript, Option<RpcScript>, String)>,
}

impl TxBuilder {
    fn new(hash_byte: u8) -> Self {
        Self {
            hash: [hash_byte; 32],
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }

    fn input(mut self, prev_tx: &str, index: u32) -> Self {
        self.inputs
            .push(((prev_tx.to_string(), index), "0x0".into()));
        self
    }

    fn output(mut self, capacity: u64, lock_args: u8) -> Self {
        self.outputs.push((
            capacity,
            script(SECP_LOCK_CODE_HASH, lock_args),
            None,
            "0x".to_string(),
        ));
        self
    }

    /// A `.cell` name cell: Cells Account Lock (empty args) + Cells Account
    /// type script with the deployment's namespace args.
    fn dotcell_output(mut self, capacity: u64, data_hex: &str) -> Self {
        self.outputs.push((
            capacity,
            RpcScript {
                code_hash: DOTCELL_ACCOUNT_LOCK_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            },
            Some(RpcScript {
                code_hash: DOTCELL_ACCOUNT_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: DOTCELL_NAMESPACE_ARGS.to_string(),
            }),
            data_hex.to_string(),
        ));
        self
    }

    fn udt_output(mut self, capacity: u64, lock_args: u8, amount: u128) -> Self {
        self.outputs.push((
            capacity,
            script(SECP_LOCK_CODE_HASH, lock_args),
            Some(script(SUDT_CODE_HASH, 0x77)),
            format!("0x{}", hex::encode(amount.to_le_bytes())),
        ));
        self
    }

    fn build(&self) -> RpcTransactionView {
        RpcTransactionView {
            hash: format!("0x{}", hex::encode(self.hash)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: self
                .inputs
                .iter()
                .map(|((tx_hash, index), since)| RpcCellInput {
                    since: since.clone(),
                    previous_output: RpcOutPoint {
                        tx_hash: tx_hash.clone(),
                        index: format!("0x{index:x}"),
                    },
                })
                .collect(),
            outputs: self
                .outputs
                .iter()
                .map(|(capacity, lock, type_, _)| RpcCellOutput {
                    capacity: format!("0x{capacity:x}"),
                    lock: lock.clone(),
                    type_: type_.clone(),
                })
                .collect(),
            outputs_data: self
                .outputs
                .iter()
                .map(|(_, _, _, data)| data.clone())
                .collect(),
            witnesses: vec!["0x".to_string()],
        }
    }
}

fn entry(time_added_to_pool_ms: u64) -> PoolEntryMeta {
    PoolEntryMeta {
        fee: 1_000,
        size: 500,
        cycles: 200_000,
        ancestors_count: 0,
        time_added_to_pool_ms,
    }
}

fn pool_info(last_txs_updated_at: u64) -> TxPoolInfo {
    TxPoolInfo {
        tip_hash: [0x11; 32],
        tip_number: 1_000,
        last_txs_updated_at,
    }
}

fn lookup(tx: RpcTransactionView, status: NodeTxStatus) -> PoolTxLookup {
    PoolTxLookup {
        status,
        transaction: Some(tx),
        block_number: None,
        block_hash: None,
    }
}

fn test_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

fn refresher(
    source: Arc<FakePoolSource>,
    store: Arc<CkbadgerStore>,
    mirror: Arc<PoolMirror>,
    max_tracked_txs: usize,
) -> PoolRefresher {
    PoolRefresher::new(
        source,
        store,
        mirror,
        PoolRefresherConfig {
            max_tracked_txs,
            is_mainnet: true,
        },
    )
}

/// A funding cell the node reports as live, owned by `lock_args`.
fn live_cell(capacity: u64, lock_args: u8) -> NodeLiveCell {
    NodeLiveCell {
        capacity,
        lock: node_script(SECP_LOCK_CODE_HASH, lock_args),
        type_script: None,
        data: vec![],
    }
}

fn setup(max_tracked_txs: usize) -> (Arc<FakePoolSource>, Arc<PoolMirror>, PoolRefresher) {
    let source = Arc::new(FakePoolSource::new());
    let mirror = Arc::new(PoolMirror::new(true));
    let refresher = refresher(
        source.clone(),
        test_store(),
        mirror.clone(),
        max_tracked_txs,
    );
    (source, mirror, refresher)
}

#[tokio::test]
async fn test_new_pool_tx_is_indexed_by_participant_lock() {
    let (source, mirror, mut refresher) = setup(100);

    let sender = 0xAA;
    let receiver = 0xBB;
    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, receiver)
        .build();

    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, sender));
    source.set_transaction([0x01; 32], lookup(tx, NodeTxStatus::Pending));

    let outcome = refresher.refresh_once().await;
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.added, 1);

    let snapshot = mirror.load();
    assert!(snapshot.status.healthy);
    assert_eq!(snapshot.status.pending, 1);
    assert_eq!(snapshot.records.len(), 1);

    let receiver_rows = snapshot.records_for_lock(&lock_hash(SECP_LOCK_CODE_HASH, receiver));
    assert_eq!(
        receiver_rows.len(),
        1,
        "the receiving lock must find the pool transaction"
    );
    let record = &receiver_rows[0];
    assert_eq!(record.pool_status, PoolStatus::Pending);
    assert_eq!(record.interpretation, Interpretation::Complete);

    let actions = record
        .actions
        .as_ref()
        .expect("complete record has actions");
    let receiver_delta = actions
        .participants
        .iter()
        .find(|p| p.id.as_bytes() == lock_hash(SECP_LOCK_CODE_HASH, receiver))
        .expect("receiver is a participant");
    assert_eq!(receiver_delta.ckb_delta, 9_900_000_000);

    let sender_rows = snapshot.records_for_lock(&lock_hash(SECP_LOCK_CODE_HASH, sender));
    assert_eq!(sender_rows.len(), 1, "the sender sees it too");
    let sender_delta = actions
        .participants
        .iter()
        .find(|p| p.id.as_bytes() == lock_hash(SECP_LOCK_CODE_HASH, sender))
        .expect("sender is a participant");
    assert_eq!(sender_delta.ckb_delta, -10_000_000_000);

    // AddrTxValue comes from the same constructor the indexer uses.
    let sender_participant = record
        .participant(&lock_hash(SECP_LOCK_CODE_HASH, sender))
        .expect("sender participant");
    assert_eq!(sender_participant.addr_tx.tx_type_str(), "sent");
    assert_eq!(sender_participant.addr_tx.capacity_change, -10_000_000_000);
}

#[tokio::test]
async fn test_chained_child_resolves_its_parent_from_the_pool() {
    let (source, mirror, mut refresher) = setup(100);

    let parent = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    // The child spends the parent's output, which the node has no live cell for.
    let child = TxBuilder::new(0x02)
        .input(&hex32(0x01), 0)
        .output(9_800_000_000, 0xCC)
        .build();

    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![
            ([0x02; 32], entry(1_700_000_002_000)),
            ([0x01; 32], entry(1_700_000_001_000)),
        ],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_transaction([0x01; 32], lookup(parent, NodeTxStatus::Pending));
    source.set_transaction([0x02; 32], lookup(child, NodeTxStatus::Pending));

    let outcome = refresher.refresh_once().await;
    assert_eq!(outcome.error, None);

    let snapshot = mirror.load();
    let child_record = snapshot.records.get(&[0x02; 32]).expect("child tracked");
    assert_eq!(
        child_record.interpretation,
        Interpretation::Complete,
        "a chained unconfirmed spend must resolve from its pool parent"
    );
    let actions = child_record.actions.as_ref().unwrap();
    let spender = actions
        .participants
        .iter()
        .find(|p| p.id.as_bytes() == lock_hash(SECP_LOCK_CODE_HASH, 0xBB))
        .expect("the parent's receiver is the child's spender");
    assert_eq!(spender.ckb_delta, -9_900_000_000);

    // by_lock orders newest time_added_to_pool first.
    let rows = snapshot.records_for_lock(&lock_hash(SECP_LOCK_CODE_HASH, 0xBB));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].tx_hash, [0x02; 32]);
    assert_eq!(rows[1].tx_hash, [0x01; 32]);
}

#[tokio::test]
async fn test_unresolvable_input_is_partial_and_retried_to_completion() {
    let (source, mirror, mut refresher) = setup(100);

    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_transaction([0x01; 32], lookup(tx, NodeTxStatus::Pending));
    // No live cell scripted: the input cannot be resolved yet.

    refresher.refresh_once().await;
    let snapshot = mirror.load();
    let record = snapshot.records.get(&[0x01; 32]).expect("tracked anyway");
    assert_eq!(
        record.interpretation,
        Interpretation::Partial {
            reasons: vec![PartialReason::UnresolvedInput {
                tx_hash: [0xF0; 32],
                index: 0
            }]
        }
    );
    assert!(
        record.actions.is_none(),
        "a position derived from a partial input set would be wrong; none is published"
    );
    assert!(
        snapshot
            .records_for_lock(&lock_hash(SECP_LOCK_CODE_HASH, 0xBB))
            .is_empty(),
        "an uninterpretable record must not be attributed to any address"
    );
    assert_eq!(snapshot.status.partial, 1);

    // The node now reports the cell as live: same pool, retried, completed.
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    refresher.refresh_once().await;

    let snapshot = mirror.load();
    let record = snapshot.records.get(&[0x01; 32]).unwrap();
    assert_eq!(record.interpretation, Interpretation::Complete);
    assert_eq!(
        snapshot
            .records_for_lock(&lock_hash(SECP_LOCK_CODE_HASH, 0xBB))
            .len(),
        1
    );
    assert_eq!(snapshot.status.partial, 0);
}

#[tokio::test]
async fn test_committed_record_is_kept_until_the_store_has_indexed_it() {
    let source = Arc::new(FakePoolSource::new());
    let mirror = Arc::new(PoolMirror::new(true));
    let store = test_store();
    let mut refresher = refresher(source.clone(), store.clone(), mirror.clone(), 100);

    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_transaction([0x01; 32], lookup(tx.clone(), NodeTxStatus::Pending));
    refresher.refresh_once().await;
    assert_eq!(mirror.load().records.len(), 1);

    // The node commits it; the local store has not indexed it yet.
    source.set_info(pool_info(2));
    source.set_raw_pool(RawTxPool::default());
    source.set_transaction(
        [0x01; 32],
        PoolTxLookup {
            status: NodeTxStatus::Committed,
            transaction: Some(tx),
            block_number: Some(4_242),
            block_hash: Some([0x33; 32]),
        },
    );
    refresher.refresh_once().await;

    let snapshot = mirror.load();
    let record = snapshot.records.get(&[0x01; 32]).expect("still tracked");
    assert_eq!(
        record.pool_status,
        PoolStatus::CommittedAwaitingIndex {
            block_number: 4_242,
            block_hash: [0x33; 32]
        },
        "a committed transaction must not vanish before the local index has it"
    );
    assert_eq!(snapshot.status.awaiting_index, 1);
    assert_eq!(
        snapshot
            .records_for_lock(&lock_hash(SECP_LOCK_CODE_HASH, 0xBB))
            .len(),
        1
    );

    // The indexer catches up: the record is dropped.
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(&[0x01; 32], 4_242, 0);
    batch.put_tx_index(
        4_242,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1_000,
            tx_size: 500,
            cycles: Some(200_000),
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    source.set_info(pool_info(3));
    refresher.refresh_once().await;
    assert!(
        mirror.load().records.is_empty(),
        "once the store has the transaction the pool record is redundant"
    );
}

#[tokio::test]
async fn test_rejected_and_unknown_transactions_are_dropped() {
    for status in [NodeTxStatus::Rejected, NodeTxStatus::Unknown] {
        let (source, mirror, mut refresher) = setup(100);
        let tx = TxBuilder::new(0x01)
            .input(&hex32(0xF0), 0)
            .output(9_900_000_000, 0xBB)
            .build();
        source.set_info(pool_info(1));
        source.set_raw_pool(RawTxPool {
            pending: vec![([0x01; 32], entry(1_700_000_000_000))],
            proposed: vec![],
        });
        source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
        source.set_transaction([0x01; 32], lookup(tx.clone(), NodeTxStatus::Pending));
        refresher.refresh_once().await;
        assert_eq!(mirror.load().records.len(), 1);

        source.set_info(pool_info(2));
        source.set_raw_pool(RawTxPool::default());
        source.set_transaction([0x01; 32], lookup(tx.clone(), status));
        refresher.refresh_once().await;

        assert!(
            mirror.load().records.is_empty(),
            "a {status:?} transaction must be dropped, not shown as pending"
        );
    }
}

#[tokio::test]
async fn test_reorg_returns_a_committed_record_to_pending() {
    let (source, mirror, mut refresher) = setup(100);
    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_transaction([0x01; 32], lookup(tx.clone(), NodeTxStatus::Pending));
    refresher.refresh_once().await;

    // Committed, awaiting index.
    source.set_info(pool_info(2));
    source.set_raw_pool(RawTxPool::default());
    source.set_transaction(
        [0x01; 32],
        PoolTxLookup {
            status: NodeTxStatus::Committed,
            transaction: Some(tx.clone()),
            block_number: Some(4_242),
            block_hash: Some([0x33; 32]),
        },
    );
    refresher.refresh_once().await;
    assert!(matches!(
        mirror.load().records[&[0x01; 32]].pool_status,
        PoolStatus::CommittedAwaitingIndex { .. }
    ));

    // A reorg puts it back in the pool.
    source.set_info(pool_info(3));
    source.set_transaction([0x01; 32], lookup(tx, NodeTxStatus::Pending));
    refresher.refresh_once().await;

    let snapshot = mirror.load();
    assert_eq!(
        snapshot.records[&[0x01; 32]].pool_status,
        PoolStatus::Pending
    );
    assert_eq!(snapshot.status.awaiting_index, 0);
    assert_eq!(snapshot.status.pending, 1);
}

#[tokio::test]
async fn test_pending_becomes_proposed_without_rebuilding_the_record() {
    let (source, mirror, mut refresher) = setup(100);
    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_transaction([0x01; 32], lookup(tx, NodeTxStatus::Pending));
    refresher.refresh_once().await;
    let first_seen = mirror.load().records[&[0x01; 32]].first_seen_ms;

    source.set_info(pool_info(2));
    source.set_raw_pool(RawTxPool {
        pending: vec![],
        proposed: vec![([0x01; 32], entry(1_700_000_000_000))],
    });
    source.clear_calls();
    refresher.refresh_once().await;

    let snapshot = mirror.load();
    assert_eq!(
        snapshot.records[&[0x01; 32]].pool_status,
        PoolStatus::Proposed
    );
    assert_eq!(snapshot.status.proposed, 1);
    assert_eq!(snapshot.records[&[0x01; 32]].first_seen_ms, first_seen);
    assert!(
        !source
            .calls()
            .iter()
            .any(|call| call.starts_with("get_transaction")),
        "a status change must not re-fetch an unchanged transaction body: {:?}",
        source.calls()
    );
}

#[tokio::test]
async fn test_tracking_cap_keeps_the_newest_and_reports_truncation() {
    let (source, mirror, mut refresher) = setup(2);

    let mut pending = Vec::new();
    for index in 1u8..=3 {
        let tx = TxBuilder::new(index)
            .input(&hex32(0xF0 + index), 0)
            .output(9_900_000_000, 0xB0 + index)
            .build();
        source.set_live_cell(
            [0xF0 + index; 32],
            0,
            live_cell(10_000_000_000, 0xA0 + index),
        );
        source.set_transaction([index; 32], lookup(tx, NodeTxStatus::Pending));
        pending.push(([index; 32], entry(1_700_000_000_000 + index as u64 * 1_000)));
    }
    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending,
        proposed: vec![],
    });

    refresher.refresh_once().await;
    let snapshot = mirror.load();
    assert_eq!(snapshot.records.len(), 2);
    assert!(snapshot.status.truncated);
    assert!(
        snapshot.records.contains_key(&[3u8; 32]) && snapshot.records.contains_key(&[2u8; 32]),
        "the newest transactions by time_added_to_pool are the ones kept"
    );
}

#[tokio::test]
async fn test_unchanged_pool_costs_exactly_one_rpc() {
    let (source, _mirror, mut refresher) = setup(100);
    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    source.set_info(pool_info(7));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_transaction([0x01; 32], lookup(tx, NodeTxStatus::Pending));
    refresher.refresh_once().await;

    source.clear_calls();
    let outcome = refresher.refresh_once().await;

    assert!(outcome.skipped, "an unchanged pool needs no further work");
    assert_eq!(
        source.calls(),
        vec!["tx_pool_info".to_string()],
        "idle cost must be one cheap RPC"
    );
}

#[tokio::test]
async fn test_malformed_entry_is_reported_without_losing_other_records() {
    let (source, mirror, mut refresher) = setup(100);

    let good = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    // Outputs and outputs_data disagree: the node handed us something we cannot
    // interpret.
    let mut broken = TxBuilder::new(0x02)
        .input(&hex32(0xF1), 0)
        .output(9_900_000_000, 0xCC)
        .build();
    broken.outputs_data.clear();

    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![
            ([0x01; 32], entry(1_700_000_001_000)),
            ([0x02; 32], entry(1_700_000_002_000)),
        ],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_live_cell([0xF1; 32], 0, live_cell(10_000_000_000, 0xAB));
    source.set_transaction([0x01; 32], lookup(good, NodeTxStatus::Pending));
    source.set_transaction([0x02; 32], lookup(broken, NodeTxStatus::Pending));

    let outcome = refresher.refresh_once().await;
    assert_eq!(outcome.entry_errors, 1);
    assert_eq!(outcome.error, None, "one bad entry must not fail the round");

    let snapshot = mirror.load();
    assert!(snapshot.status.healthy);
    assert_eq!(snapshot.status.entry_errors.len(), 1);
    assert_eq!(snapshot.status.entry_errors[0].tx_hash, [0x02; 32]);
    assert!(
        snapshot.records.contains_key(&[0x01; 32]),
        "the readable transaction is still mirrored"
    );
    assert!(!snapshot.records.contains_key(&[0x02; 32]));
}

#[tokio::test]
async fn test_rpc_failure_publishes_an_unhealthy_snapshot_not_an_empty_pool() {
    let (source, mirror, mut refresher) = setup(100);
    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(9_900_000_000, 0xBB)
        .build();
    source.set_info(pool_info(1));
    source.set_raw_pool(RawTxPool {
        pending: vec![([0x01; 32], entry(1_700_000_000_000))],
        proposed: vec![],
    });
    source.set_live_cell([0xF0; 32], 0, live_cell(10_000_000_000, 0xAA));
    source.set_transaction([0x01; 32], lookup(tx, NodeTxStatus::Pending));
    refresher.refresh_once().await;

    source.set_info_error("connection refused");
    let outcome = refresher.refresh_once().await;
    assert_eq!(outcome.error.as_deref(), Some("connection refused"));

    let snapshot = mirror.load();
    assert!(!snapshot.status.healthy);
    assert_eq!(
        snapshot.status.last_error.as_deref(),
        Some("connection refused")
    );
    assert_eq!(
        snapshot.records.len(),
        1,
        "records last observed stay visible; the response says the view is stale"
    );
}

/// Single calculation path: one transaction interpreted through the live-sync
/// `TxView` shape (input data withheld, UDT amount from the store) and through
/// the pool resolver (input data from the node) must produce the SAME
/// `TxActions`. If these ever diverge, a transaction would read one way in the
/// pool and another once committed.
#[tokio::test]
async fn test_interpretation_parity_between_live_sync_and_pool_resolution() {
    use ckbadger_indexer::db::{
        build_tx_actions_with_production_detectors, InputCellView, OutputCellView, TxView,
    };

    let amount_in: u128 = 5_000;
    let amount_out: u128 = 5_000;
    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .udt_output(14_200_000_000, 0xBB, amount_out)
        .build();

    // --- Pool path: the node hands us the input cell with its data. ---
    let input_cell = ResolvedCell::new(
        14_300_000_000,
        hex::decode(SECP_LOCK_CODE_HASH.trim_start_matches("0x")).unwrap(),
        1,
        vec![0xAA; 20],
        Some((
            hex::decode(SUDT_CODE_HASH.trim_start_matches("0x")).unwrap(),
            1,
            vec![0x77; 20],
        )),
        amount_in.to_le_bytes().to_vec(),
    )
    .unwrap();
    assert_eq!(
        input_cell.udt_amount,
        Some(amount_in),
        "the resolver must read the UDT amount out of the node's cell data"
    );

    let mut previous_outputs = std::collections::HashMap::new();
    previous_outputs.insert(([0xF0; 32], 0u32), input_cell.clone());
    let resolved = resolve_pool_tx(&tx, &previous_outputs).unwrap();
    let zero_block_hash = [0u8; 32];
    let pool_view = resolved
        .tx_view(&zero_block_hash, 1_700_000_000_000)
        .expect("all inputs resolved");
    let pool_actions = build_tx_actions_with_production_detectors(&[pool_view], true).unwrap();

    // --- Live-sync path: input built from the store's LiveCellInfo, which has
    // no data bytes and carries the UDT amount as a stored field. ---
    let output = &resolved.outputs[0];
    let live_view = TxView {
        tx_hash: &resolved.tx_hash,
        block_hash: &zero_block_hash,
        tx_index: 0,
        block_number: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        inputs: vec![InputCellView {
            previous_tx_hash: &[0xF0; 32],
            previous_output_index: 0,
            lock_script_hash: &input_cell.lock_script_hash,
            lock_code_hash: &input_cell.lock_code_hash,
            lock_hash_type: input_cell.lock_hash_type,
            lock_args: &input_cell.lock_args,
            capacity: input_cell.capacity,
            occupied_capacity: input_cell.occupied_capacity,
            type_code_hash: input_cell.type_code_hash.as_deref(),
            type_hash_type: input_cell.type_hash_type,
            type_script_hash: input_cell.type_script_hash.as_deref(),
            type_args: input_cell.type_args.as_deref(),
            udt_amount: Some(amount_in),
            bit_cell_identity_id: None,
            dotcell: None,
            // Live sync retains no input cell data.
            data: &[],
            is_dao_withdraw_request: false,
            dao_compensation: None,
        }],
        outputs: vec![OutputCellView {
            capacity: output.capacity,
            lock_code_hash: &output.lock_code_hash,
            lock_hash_type: output.lock_hash_type,
            lock_args: &output.lock_args,
            lock_script_hash: &output.lock_script_hash,
            type_code_hash: output.type_code_hash.as_deref(),
            type_hash_type: output.type_hash_type,
            type_args: output.type_args.as_deref(),
            type_script_hash: output.type_script_hash.as_deref(),
            data_hash: &[0u8; 32],
            data_size: output.data.len() as i32,
            data: &output.data,
        }],
    };
    let live_actions = build_tx_actions_with_production_detectors(&[live_view], true).unwrap();

    assert_eq!(
        serde_json::to_value(&pool_actions).unwrap(),
        serde_json::to_value(&live_actions).unwrap(),
        "the pool resolver and the live-sync path must interpret the same transaction identically"
    );
}

/// A `.cell` transfer must read the same in the pool as it does once committed.
///
/// A `.cell` name's ownership lives in the cell's DATA, and the classifier
/// learns the previous owner only from `InputCellView.dotcell`. The mirror
/// holds the node's resolved previous output — data included — so it can fill
/// that field; leaving it empty turns every touch of an existing name into a
/// brand-new `register` with no `owner_from`, which is exactly what a user
/// watching the mempool must not be told.
#[tokio::test]
async fn test_dotcell_transfer_reads_the_same_in_the_pool_as_committed() {
    use ckbadger_indexer::db::{
        build_tx_actions_with_production_detectors, InputCellView, OutputCellView, TxView,
    };
    use ckbadger_indexer::parser::DotCellParser;
    use ckbadger_store::types::ParticipantId;

    let name_cell = ResolvedCell::new(
        24_000_000_000,
        hex::decode(DOTCELL_ACCOUNT_LOCK_CODE_HASH.trim_start_matches("0x")).unwrap(),
        1,
        Vec::new(),
        Some((
            hex::decode(DOTCELL_ACCOUNT_CODE_HASH.trim_start_matches("0x")).unwrap(),
            1,
            hex::decode(DOTCELL_NAMESPACE_ARGS.trim_start_matches("0x")).unwrap(),
        )),
        hex::decode(DOTCELL_ABUSE_BEFORE.trim_start_matches("0x")).unwrap(),
    )
    .unwrap();
    let funding_cell = ResolvedCell::new(
        8_774_800_000_000,
        hex::decode(SECP_LOCK_CODE_HASH.trim_start_matches("0x")).unwrap(),
        1,
        vec![0xAA; 20],
        None,
        Vec::new(),
    )
    .unwrap();

    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .input(&hex32(0xF1), 0)
        .dotcell_output(24_000_000_000, DOTCELL_ABUSE_AFTER)
        .output(8_774_700_000_000, 0xAA)
        .build();

    let mut previous_outputs = std::collections::HashMap::new();
    previous_outputs.insert(([0xF0; 32], 0u32), name_cell.clone());
    previous_outputs.insert(([0xF1; 32], 0u32), funding_cell.clone());
    let resolved = resolve_pool_tx(&tx, &previous_outputs).unwrap();
    let zero_block_hash = [0u8; 32];
    let pool_view = resolved
        .tx_view(&zero_block_hash, 1_700_000_000_000)
        .expect("all inputs resolved");
    let pool_actions = build_tx_actions_with_production_detectors(&[pool_view], true).unwrap();

    // --- Committed path: the input arrives without data, and the consumed
    // name's prior state comes from the identity entry the indexer read. ---
    let previous_name =
        DotCellParser::parse_name_data(&name_cell.data).expect("the real mainnet name cell");
    let name_out = &resolved.outputs[0];
    let change_out = &resolved.outputs[1];
    let live_view = TxView {
        tx_hash: &resolved.tx_hash,
        block_hash: &zero_block_hash,
        tx_index: 0,
        block_number: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        inputs: vec![
            InputCellView {
                previous_tx_hash: &[0xF0; 32],
                previous_output_index: 0,
                lock_script_hash: &name_cell.lock_script_hash,
                lock_code_hash: &name_cell.lock_code_hash,
                lock_hash_type: name_cell.lock_hash_type,
                lock_args: &name_cell.lock_args,
                capacity: name_cell.capacity,
                occupied_capacity: name_cell.occupied_capacity,
                type_code_hash: name_cell.type_code_hash.as_deref(),
                type_hash_type: name_cell.type_hash_type,
                type_script_hash: name_cell.type_script_hash.as_deref(),
                type_args: name_cell.type_args.as_deref(),
                udt_amount: None,
                bit_cell_identity_id: None,
                dotcell: Some(&previous_name),
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            },
            InputCellView {
                previous_tx_hash: &[0xF1; 32],
                previous_output_index: 0,
                lock_script_hash: &funding_cell.lock_script_hash,
                lock_code_hash: &funding_cell.lock_code_hash,
                lock_hash_type: funding_cell.lock_hash_type,
                lock_args: &funding_cell.lock_args,
                capacity: funding_cell.capacity,
                occupied_capacity: funding_cell.occupied_capacity,
                type_code_hash: None,
                type_hash_type: None,
                type_script_hash: None,
                type_args: None,
                udt_amount: None,
                bit_cell_identity_id: None,
                dotcell: None,
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            },
        ],
        outputs: vec![
            OutputCellView {
                capacity: name_out.capacity,
                lock_code_hash: &name_out.lock_code_hash,
                lock_hash_type: name_out.lock_hash_type,
                lock_args: &name_out.lock_args,
                lock_script_hash: &name_out.lock_script_hash,
                type_code_hash: name_out.type_code_hash.as_deref(),
                type_hash_type: name_out.type_hash_type,
                type_args: name_out.type_args.as_deref(),
                type_script_hash: name_out.type_script_hash.as_deref(),
                data_hash: &[0u8; 32],
                data_size: name_out.data.len() as i32,
                data: &name_out.data,
            },
            OutputCellView {
                capacity: change_out.capacity,
                lock_code_hash: &change_out.lock_code_hash,
                lock_hash_type: change_out.lock_hash_type,
                lock_args: &change_out.lock_args,
                lock_script_hash: &change_out.lock_script_hash,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: &[0u8; 32],
                data_size: 0,
                data: &[],
            },
        ],
    };
    let live_actions = build_tx_actions_with_production_detectors(&[live_view], true).unwrap();

    assert_eq!(
        serde_json::to_value(&pool_actions).unwrap(),
        serde_json::to_value(&live_actions).unwrap(),
        "a .cell transfer must interpret identically in the pool and once committed"
    );

    // And it must be a transfer, not a registration: the whole point is that
    // the previous owner is named.
    let dotcell_actions: Vec<_> = pool_actions[0]
        .protocol_actions
        .iter()
        .filter(|action| action.protocol == "dotcell")
        .collect();
    assert_eq!(dotcell_actions.len(), 1, "{dotcell_actions:?}");
    assert_eq!(dotcell_actions[0].action, "transfer");

    let from = pool_actions[0]
        .participants
        .iter()
        .find(|p| p.id == ParticipantId::LockPrefix(DOTCELL_ABUSE_OWNER_BEFORE))
        .expect("the previous owner must be named");
    assert_eq!(
        from.roles,
        ckbadger_store::types::participant_roles::OWNER_FROM
    );
    assert_eq!(from.item_deltas.len(), 1);
    assert!(from.item_deltas[0].negative);

    let to = pool_actions[0]
        .participants
        .iter()
        .find(|p| p.id == ParticipantId::LockPrefix(DOTCELL_ABUSE_OWNER_AFTER))
        .expect("the new owner must be named");
    assert_ne!(
        to.roles & ckbadger_store::types::participant_roles::OWNER_TO,
        0
    );
    assert!(!to.item_deltas[0].negative);
}

/// A pool transaction that completes a DAO withdrawal declares the one layer it
/// cannot compute, rather than reporting a compensation of zero.
#[tokio::test]
async fn test_dao_withdrawal_completion_declares_missing_compensation() {
    use ckbadger_indexer::parser::dao::DAO_CODE_HASH;

    let dao_withdraw_request_cell = ResolvedCell::new(
        20_400_000_000,
        hex::decode(SECP_LOCK_CODE_HASH.trim_start_matches("0x")).unwrap(),
        1,
        vec![0xAA; 20],
        Some((
            hex::decode(DAO_CODE_HASH.trim_start_matches("0x")).unwrap(),
            1,
            vec![],
        )),
        // Non-zero deposit block number: this is a phase-1 request cell.
        4_242u64.to_le_bytes().to_vec(),
    )
    .unwrap();
    assert!(dao_withdraw_request_cell.is_dao_withdraw_request());

    let tx = TxBuilder::new(0x01)
        .input(&hex32(0xF0), 0)
        .output(20_500_000_000, 0xAA)
        .build();
    let mut previous_outputs = std::collections::HashMap::new();
    previous_outputs.insert(([0xF0; 32], 0u32), dao_withdraw_request_cell);
    let resolved = resolve_pool_tx(&tx, &previous_outputs).unwrap();

    assert!(resolved.completes_dao_withdrawal());
    let zero = [0u8; 32];
    assert!(
        resolved.tx_view(&zero, 0).is_some(),
        "layers 1 and 2 are exact and must still be interpreted"
    );
}

/// Pool participants come from the same row derivation committed rows use.
///
/// The mirror used to re-derive `has_input`/`has_output` from its resolved
/// cells; that was a second definition of an `addr_txs` row and could drift
/// from the indexer's. Now it calls `participant_rows::addr_tx_rows`, so a
/// protocol-named party with no cell lands as a `named` row here exactly as it
/// does in the store.
#[test]
fn pool_participants_come_from_the_shared_row_derivation() {
    use ckbadger_indexer::db::ParticipantIo;
    use ckbadger_store::types::{
        participant_roles, ParticipantDelta, ParticipantId, TxActions, TAG_IDENTITY,
    };

    let sender = lock_hash(SECP_LOCK_CODE_HASH, 0xAA);
    let named_prefix = [0x77u8; 20];
    let actions = TxActions {
        tx_hash: vec![0x01; 32],
        block_hash: vec![0u8; 32],
        block_number: 0,
        tx_index: 0,
        timestamp: 0,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&sender).unwrap(),
                ckb_delta: -10_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(named_prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: TAG_IDENTITY,
                roles: participant_roles::OWNER_TO,
            },
        ],
    };
    let io = vec![
        ParticipantIo {
            has_inputs: true,
            has_outputs: false,
        },
        ParticipantIo {
            has_inputs: false,
            has_outputs: false,
        },
    ];

    let participants = super::mirror::participants_from(&actions, &io).expect("participants");
    assert_eq!(participants.len(), 2);
    assert_eq!(participants[0].id, ParticipantId::lock(&sender).unwrap());
    assert_eq!(participants[0].addr_tx.tx_type_str(), "sent");
    assert_eq!(participants[1].id, ParticipantId::LockPrefix(named_prefix));
    assert_eq!(participants[1].addr_tx.tx_type_str(), "named");
    assert_eq!(participants[1].addr_tx.capacity_change, 0);

    let record = super::snapshot::PoolTxRecord {
        tx_hash: [0x01; 32],
        pool_status: PoolStatus::Pending,
        entry: PoolEntryMeta {
            fee: 1,
            size: 1,
            cycles: 1,
            ancestors_count: 0,
            time_added_to_pool_ms: 0,
        },
        outputs: vec![],
        actions: Some(actions),
        participants,
        inputs_count: 1,
        outputs_count: 0,
        semantic_tags: 0,
        is_cellbase: false,
        interpretation: Interpretation::Complete,
        first_seen_ms: 0,
        last_seen_ms: 0,
    };

    // The matcher finds a party by either identity.
    let mut named_lock = [0x77u8; 32];
    named_lock[31] = 0x01;
    assert!(record.participant(&sender).is_some());
    assert!(
        record.participant(&named_lock).is_some(),
        "a 20-byte prefix must match any lock hash starting with it"
    );
}
