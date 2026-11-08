//! Store refactor lane C (pointer keys): the API responses that carry a hash a
//! domain row stops storing and the read side resolves from a `(block, tx_idx)`
//! pointer instead (`consumed_cells`, `addr_txs`, `activities`, collection
//! activities, `block_headers.parent_hash`, `addr_balance` first / last tx,
//! lazy cycles).
//!
//! One small chain — transfers, a Spore cluster with a Spore minted into it and
//! moved, a did:ckb minted and moved, a cell consumed in a later block, and a
//! two-block fork rolled back and replaced — is written through the indexer's
//! real live path (`live_test_support`), the node database is seeded with the
//! same blocks, and every response below must stay byte-identical to the JSON
//! committed under `tests/fixtures/layout_pins/` while the store layout under
//! it changes. A missing fixture fails the test and leaves the actual response
//! under the target's tmp directory for review; nothing here rewrites a
//! fixture.

use crate::common::*;
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView as RpcBlock, CellInput, CellOutput, HeaderView, OutPoint,
    Script, TransactionView,
};
use ckbadger_indexer::sync::live_test_support::{reorg_to, restart, write_live_batch};

const GENESIS_C: u64 = 3_360_000_000_000_000_000;
const SECONDARY_EPOCH_REWARD: u64 = 61_369_863_013_698;
const AR_BASE: u64 = 10_000_000_000_000_000;
const EPOCH_LENGTH: u64 = 1800;
/// `CellbaseWitness { lock: secp256k1 / args 0x8211…, message }` (mainnet
/// shape), so every block has a miner lock.
const CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";
const SECP_CODE_HASH: &str = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
const DID_CKB_CODE_HASH: &str =
    "0x4a06164dc34dccade5afe3e847a97b6db743e79f5477fa3295acf02849c5984a";
/// 2026-09-22 09:00:00 UTC+8.
const T0: u64 = 1_790_038_800_000;
const MINUTE: u64 = 60_000;
const FUND: u64 = 100_000;

const CLUSTER_ID: [u8; 32] = [0x11; 32];
const SPORE_ID: [u8; 32] = [0x12; 32];
const DID_ID: [u8; 32] = [0x16; 32];

fn hex32(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn lock(owner: u8) -> Script {
    Script {
        code_hash: SECP_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode([owner; 20])),
    }
}

fn lock_hash(owner: u8) -> String {
    let args = [owner; 20];
    format!(
        "0x{}",
        hex::encode(compute_script_hash(
            &hex::decode(&SECP_CODE_HASH[2..]).unwrap(),
            1,
            &args
        ))
    )
}

fn output(capacity_ckb: u64, lock: Script, type_: Option<Script>) -> CellOutput {
    CellOutput {
        capacity: format!("0x{:x}", capacity_ckb * 100_000_000),
        lock,
        type_,
    }
}

fn plain(capacity_ckb: u64, owner: u8) -> (CellOutput, String) {
    (output(capacity_ckb, lock(owner), None), "0x".to_string())
}

fn typed(capacity_ckb: u64, owner: u8, type_: Script, data: &[u8]) -> (CellOutput, String) {
    (
        output(capacity_ckb, lock(owner), Some(type_)),
        format!("0x{}", hex::encode(data)),
    )
}

/// The node's own hash of a transaction (the reader recomputes it from the
/// stored body, so the chain uses real hashes throughout).
fn real_tx_hash(tx: &TransactionView) -> [u8; 32] {
    use ckb_types::{packed, prelude::*};
    let json: ckb_jsonrpc_types::TransactionView =
        serde_json::from_value(serde_json::to_value(tx).unwrap()).unwrap();
    packed::Transaction::from(json.inner)
        .calc_tx_hash()
        .unpack()
}

fn jsonrpc_header(h: &HeaderView) -> ckb_jsonrpc_types::HeaderView {
    serde_json::from_value(serde_json::json!({
        "version": h.version,
        "compact_target": h.compact_target,
        "timestamp": h.timestamp,
        "number": h.number,
        "epoch": h.epoch,
        "parent_hash": h.parent_hash,
        "transactions_root": h.transactions_root,
        "proposals_hash": h.proposals_hash,
        "extra_hash": h.extra_hash,
        "dao": h.dao,
        "nonce": h.nonce,
        "hash": h.hash,
    }))
    .unwrap()
}

/// Builds the chain with real transaction and block hashes; transactions are
/// named by a tag and referenced by it.
#[derive(Default)]
struct Chain {
    txs: std::collections::HashMap<u16, [u8; 32]>,
    blocks: std::collections::HashMap<(u64, u8), [u8; 32]>,
}

impl Chain {
    fn tx_hash(&self, tag: u16) -> [u8; 32] {
        *self
            .txs
            .get(&tag)
            .unwrap_or_else(|| panic!("tx {tag:#06x} not built yet"))
    }

    fn block_hash(&self, number: u64, branch: u8) -> [u8; 32] {
        self.blocks[&(number, branch)]
    }

    fn input(&self, tag: u16, index: u32) -> CellInput {
        CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: hex32(self.tx_hash(tag)),
                index: format!("0x{index:x}"),
            },
        }
    }

    fn finish(&mut self, tag: u16, mut tx: TransactionView) -> TransactionView {
        let hash = real_tx_hash(&tx);
        tx.hash = hex32(hash);
        assert!(
            self.txs.insert(tag, hash).is_none(),
            "tag {tag:#06x} reused"
        );
        tx
    }

    fn tx(
        &mut self,
        tag: u16,
        inputs: Vec<CellInput>,
        outputs: Vec<(CellOutput, String)>,
    ) -> TransactionView {
        let (outputs, outputs_data): (Vec<_>, Vec<_>) = outputs.into_iter().unzip();
        let tx = TransactionView {
            hash: hex32([0; 32]),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            witnesses: vec!["0x".to_string(); inputs.len().max(1)],
            inputs,
            outputs,
            outputs_data,
        };
        self.finish(tag, tx)
    }

    /// `branch` 0 is the first branch, 1 the one that replaces blocks above
    /// 4 after the fork.
    fn block(
        &mut self,
        number: u64,
        branch: u8,
        txs: Vec<TransactionView>,
    ) -> BlockResponseWithCycles {
        let outputs: Vec<(CellOutput, String)> = if number == 0 {
            (0..8).map(|i| plain(FUND, 0x20 + i as u8)).collect()
        } else {
            vec![plain(1_000, 0xc0)]
        };
        let (outputs, outputs_data) = outputs.into_iter().unzip();
        let cellbase = TransactionView {
            hash: hex32([0; 32]),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: format!("0x{number:x}"),
                previous_output: OutPoint {
                    tx_hash: hex32([0; 32]),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs,
            outputs_data,
            witnesses: vec![CELLBASE_WITNESS.to_string()],
        };
        let cellbase = self.finish(0xc000 + 0x100 * branch as u16 + number as u16, cellbase);
        let mut transactions = vec![cellbase];
        transactions.extend(txs);

        let timestamp_ms = T0 + number * 10 * MINUTE;
        let mut dao = [0u8; 32];
        dao[0..8].copy_from_slice(&GENESIS_C.to_le_bytes());
        dao[8..16].copy_from_slice(&(AR_BASE + number * 1_000_000_000).to_le_bytes());
        dao[16..24].copy_from_slice(&10_000_000_000u64.to_le_bytes());
        dao[24..32].copy_from_slice(&100_000_000_000_000u64.to_le_bytes());
        let epoch = (EPOCH_LENGTH << 40) | (number << 24);
        let parent_hash = if number == 0 {
            [0; 32]
        } else {
            // The fork replaces blocks above 4: block 5's parent is block 4
            // of the first branch on either branch.
            self.block_hash(number - 1, if number - 1 <= 4 { 0 } else { branch })
        };
        let mut header = HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: format!("0x{timestamp_ms:x}"),
            number: format!("0x{number:x}"),
            epoch: format!("0x{epoch:x}"),
            parent_hash: hex32(parent_hash),
            transactions_root: hex32([0x22; 32]),
            proposals_hash: hex32([0x33; 32]),
            extra_hash: hex32([0x44; 32]),
            dao: format!("0x{}", hex::encode(dao)),
            // The two branches' block 5 / 6 differ only in their bodies (the
            // transactions root is not computed), so the nonce tells them apart.
            nonce: format!("0x{:x}", 1 + branch),
            hash: hex32([0; 32]),
        };
        let hash: [u8; 32] = {
            use ckb_types::{packed, prelude::*};
            packed::Header::from(jsonrpc_header(&header).inner)
                .calc_header_hash()
                .unpack()
        };
        header.hash = hex32(hash);
        self.blocks.insert((number, branch), hash);
        BlockResponseWithCycles {
            block: RpcBlock {
                header,
                uncles: vec![],
                transactions,
                proposals: vec![],
            },
            cycles: None,
        }
    }
}

fn molecule_bytes(raw: &[u8]) -> Vec<u8> {
    let mut out = (raw.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(raw);
    out
}

fn molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
    let head = 4 * (fields.len() + 1);
    let total = head + fields.iter().map(Vec::len).sum::<usize>();
    let mut out = (total as u32).to_le_bytes().to_vec();
    let mut offset = head;
    for field in fields {
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += field.len();
    }
    for field in fields {
        out.extend_from_slice(field);
    }
    out
}

fn cluster_type() -> Script {
    Script {
        code_hash: ckbadger_indexer::parser::spore::CLUSTER_CODE_HASH_MAINNET_V2.to_string(),
        hash_type: "data1".to_string(),
        args: hex32(CLUSTER_ID),
    }
}

fn spore_type() -> Script {
    Script {
        code_hash: ckbadger_indexer::parser::spore::SPORE_CODE_HASH_MAINNET_V2.to_string(),
        hash_type: "data1".to_string(),
        args: hex32(SPORE_ID),
    }
}

fn did_type() -> Script {
    Script {
        code_hash: DID_CKB_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: hex32(DID_ID),
    }
}

/// The genesis cellbase's output `index` (8 owners funded with `FUND`).
const GENESIS_CELLBASE: u16 = 0xc000;

struct Fixture {
    chain: Chain,
    /// Blocks 0..=6 of the first branch.
    first: Vec<BlockResponseWithCycles>,
    /// Blocks 0..=4 and the branch 5'..=6' that replaces 5..=6.
    winning: Vec<BlockResponseWithCycles>,
}

fn fixture() -> Fixture {
    let mut c = Chain::default();
    let cluster_data = molecule_table(&[
        molecule_bytes(b"Pins"),
        molecule_bytes(b"{\"dob\":{\"ver\":1}}"),
        Vec::new(),
    ]);
    let spore_data = molecule_table(&[
        molecule_bytes(b"text/plain"),
        molecule_bytes(b"pinned"),
        molecule_bytes(&CLUSTER_ID),
    ]);
    let genesis = c.block(0, 0, vec![]);
    let funding = |c: &Chain, index: u32| c.input(GENESIS_CELLBASE, index);
    // 1: a Spore cluster; 500 CKB to owner 0x31.
    let txs = vec![
        c.tx(
            0x0101,
            vec![funding(&c, 0)],
            vec![
                typed(1_000, 0x01, cluster_type(), &cluster_data),
                plain(FUND - 1_001, 0x20),
            ],
        ),
        c.tx(
            0x0102,
            vec![funding(&c, 1)],
            vec![plain(500, 0x31), plain(FUND - 501, 0x21)],
        ),
    ];
    let b1 = c.block(1, 0, txs);
    // 2: a Spore minted into the cluster; a did:ckb.
    let txs = vec![
        c.tx(
            0x0201,
            vec![funding(&c, 2)],
            vec![
                typed(1_000, 0x01, spore_type(), &spore_data),
                plain(FUND - 1_001, 0x22),
            ],
        ),
        c.tx(
            0x0202,
            vec![funding(&c, 3)],
            vec![
                typed(500, 0x06, did_type(), b"did"),
                plain(FUND - 501, 0x23),
            ],
        ),
    ];
    let b2 = c.block(2, 0, txs);
    // 3: the Spore and the did:ckb move to new owners.
    let txs = vec![
        c.tx(
            0x0301,
            vec![c.input(0x0201, 0)],
            vec![typed(999, 0x02, spore_type(), &spore_data)],
        ),
        c.tx(
            0x0302,
            vec![c.input(0x0202, 0)],
            vec![typed(499, 0x07, did_type(), b"did")],
        ),
    ];
    let b3 = c.block(3, 0, txs);
    // 4: owner 0x31's cell (created in block 1) is consumed.
    let txs = vec![c.tx(0x0401, vec![c.input(0x0102, 0)], vec![plain(499, 0x32)])];
    let b4 = c.block(4, 0, txs);
    let canonical = vec![genesis, b1, b2, b3, b4];
    // 5, 6: the branch the fork drops.
    let txs = vec![c.tx(0x0501, vec![c.input(0x0401, 0)], vec![plain(498, 0x33)])];
    let b5 = c.block(5, 0, txs);
    let b6 = c.block(6, 0, vec![]);
    // 5', 6': the branch that replaces it.
    let txs = vec![c.tx(0x0511, vec![c.input(0x0401, 0)], vec![plain(498, 0x34)])];
    let b5r = c.block(5, 1, txs);
    let b6r = c.block(6, 1, vec![]);
    let mut first = canonical.clone();
    first.extend([b5, b6]);
    let mut winning = canonical;
    winning.extend([b5r, b6r]);
    Fixture {
        chain: c,
        first,
        winning,
    }
}

/// The node database's view of a block.
fn node_block(block: &BlockResponseWithCycles) -> ckb_types::core::BlockView {
    use ckb_types::{core, packed, prelude::*};
    let header = jsonrpc_header(&block.block.header);
    let header_view = packed::Header::from(header.inner).into_view();
    assert_eq!(header_view.hash(), header.hash.pack());
    let transactions = block
        .block
        .transactions
        .iter()
        .map(|tx| {
            let json: ckb_jsonrpc_types::TransactionView =
                serde_json::from_value(serde_json::to_value(tx).unwrap()).unwrap();
            let view = packed::Transaction::from(json.inner).into_view();
            assert_eq!(view.hash(), json.hash.pack());
            view
        })
        .collect();
    core::BlockView::new_unchecked(
        header_view,
        packed::UncleBlockVecView::default().unpack(),
        transactions,
        packed::ProposalShortIdVec::default(),
    )
}

struct Pinned {
    app: axum::Router,
    chain: Chain,
    _rpc: MockServer,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    _chain: TestCkbChain,
}

/// The chain written through the live path (one block per batch), the fork
/// rolled back to block 4 and replaced, and an API over the result.
async fn pinned_app() -> Pinned {
    let domain_dir = tempfile::tempdir().unwrap();
    let append_dir = tempfile::tempdir().unwrap();
    let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
    let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
    domain
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance: GENESIS_C as i128,
            burnt: 0,
            virtual_occupied: 0,
        })
        .unwrap();
    domain
        .set_secondary_epoch_reward(SECONDARY_EPOCH_REWARD)
        .unwrap();
    let Fixture {
        chain: built,
        first,
        winning,
    } = fixture();
    let indexer = restart(domain.clone(), append.clone(), "mainnet").unwrap();
    for block in &first {
        write_live_batch(&indexer, vec![block.clone()])
            .await
            .unwrap();
    }
    reorg_to(&indexer, 4).await.unwrap();
    for block in &winning[5..] {
        write_live_batch(&indexer, vec![block.clone()])
            .await
            .unwrap();
    }
    drop(indexer);
    let node_blocks: Vec<_> = winning.iter().map(node_block).collect();
    let chain = seed_ckb_chain(&node_blocks);
    // Block pages ask the node for the block's economic state; it has not
    // finalized any of these blocks.
    let rpc = MockServer::start().await;
    mount_block_economic_state_not_finalized(&rpc).await;
    let app = create_router(AppConfig {
        ckb_rpc_url: rpc.uri(),
        ..test_config_with_ckb_db_path(
            domain,
            append,
            chain.path.clone(),
            Some(chain.cleanup.clone()),
        )
    })
    .await;
    Pinned {
        app,
        chain: built,
        _rpc: rpc,
        _dirs: (domain_dir, append_dir),
        _chain: chain,
    }
}

/// `response` must equal the committed fixture `name` byte for byte (as
/// pretty JSON).
fn assert_pinned(name: &str, status: StatusCode, response: &serde_json::Value) {
    let actual = format!("{}\n", serde_json::to_string_pretty(response).unwrap());
    assert_eq!(status, StatusCode::OK, "{name}: {actual}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/layout_pins")
        .join(format!("{name}.json"));
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("layout_pins")
            .join(format!("{name}.json"));
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &actual).unwrap();
        panic!(
            "no pinned response {}; the actual one is in {}",
            path.display(),
            out.display()
        )
    });
    if expected != actual {
        let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("layout_pins")
            .join(format!("{name}.actual.json"));
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &actual).unwrap();
        panic!(
            "{name} differs from its pin {}; the actual response is in {}",
            path.display(),
            out.display()
        );
    }
}

/// Every pinned route, as `(fixture name, path)`.
fn pinned_routes(chain: &Chain) -> Vec<(&'static str, String)> {
    let tx = |tag: u16| hex32(chain.tx_hash(tag));
    vec![
        // consumed_cells: the consumer of a spent cell.
        ("cell-consumed", format!("/cells/{}/0", tx(0x0102))),
        (
            "graph-cell-consumed",
            format!("/graph/cell/{}/0?depth=1", tx(0x0102)),
        ),
        (
            "transaction-detail-spent-output",
            format!("/transactions/{}/detail", tx(0x0102)),
        ),
        (
            "transaction-detail-consumer",
            format!("/transactions/{}/detail", tx(0x0401)),
        ),
        (
            "transaction-consumer",
            format!("/transactions/{}", tx(0x0401)),
        ),
        (
            "spore-cell-versions",
            format!("/spore/objects/{}/cell-versions", hex32(SPORE_ID)),
        ),
        (
            "did-cell-versions",
            format!(
                "/assets/identities/did/items/{}/cell-versions",
                hex32(DID_ID)
            ),
        ),
        // addr_txs, activities, addr_balance first / last transaction.
        ("address-0x31", format!("/addresses/{}", lock_hash(0x31))),
        (
            "address-0x31-transactions",
            format!("/addresses/{}/transactions", lock_hash(0x31)),
        ),
        (
            "address-0x31-activities",
            format!("/addresses/{}/activities", lock_hash(0x31)),
        ),
        ("address-0x01", format!("/addresses/{}", lock_hash(0x01))),
        (
            "address-0x01-activities",
            format!("/addresses/{}/activities", lock_hash(0x01)),
        ),
        (
            "address-0x34-transactions",
            format!("/addresses/{}/transactions", lock_hash(0x34)),
        ),
        ("activities", "/activities".to_string()),
        ("activities-latest", "/activities/latest".to_string()),
        ("transactions", "/transactions".to_string()),
        // Collection activities.
        (
            "cluster-activities",
            format!("/assets/objects/{}/activities", hex32(CLUSTER_ID)),
        ),
        (
            "did-collection-activities",
            format!(
                "/assets/identities/0x{}/activities",
                hex::encode(ckbadger_store::types::DID_CKB_SENTINEL_COLLECTION)
            ),
        ),
        // block_headers: parent hashes; the fork.
        ("block-4", "/blocks/4".to_string()),
        ("block-5", "/blocks/5".to_string()),
        (
            "block-5-by-hash",
            format!("/blocks/{}", hex32(chain.block_hash(5, 1))),
        ),
        ("blocks", "/blocks".to_string()),
        ("forks", "/forks".to_string()),
    ]
}

#[tokio::test]
async fn pointer_backed_responses_match_their_pins() {
    let pinned = pinned_app().await;
    let mut failures = Vec::new();
    for (name, path) in pinned_routes(&pinned.chain) {
        let (status, mut json) = get_json(&pinned.app, &path).await;
        if name == "forks" {
            // A reorg event is named by the wall-clock time it was detected
            // at; nothing else in it depends on the clock.
            for event in json["data"].as_array_mut().into_iter().flatten() {
                event["detectedAt"] = serde_json::json!("<detected at>");
                event["id"] = serde_json::json!("<event id>");
            }
        }
        if let Err(panic) = std::panic::catch_unwind(|| assert_pinned(name, status, &json)) {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("{name} ({path}): {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
