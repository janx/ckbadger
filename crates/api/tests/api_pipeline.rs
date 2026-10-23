//! `GET /api/v1/pipeline` — the transaction-pipeline snapshot.
//!
//! Every fixture is a contiguous chain seeded into BOTH stores exactly the way
//! production reads them: ckbadger block headers + tx index rows (the store
//! view) and a CKB-node-format RocksDB holding the real blocks (proposal zones,
//! uncles, transaction hashes). The node RPC answers `get_block_economic_state`
//! for the cellbase reward targets and the tx pool, from wiremock.

mod common;

use common::*;
use std::collections::BTreeMap;

use ckb_types::bytes::Bytes;
use ckb_types::core::{BlockBuilder, BlockView, EpochNumberWithFraction, TransactionView};
use ckb_types::packed::{self, ProposalShortId};
use ckb_types::prelude::*;

// ---------------------------------------------------------------------------
// Fixture chain
// ---------------------------------------------------------------------------

/// A non-cellbase fixture transaction with the values the store records for it.
#[derive(Clone)]
struct FixtureTx {
    tx: TransactionView,
    fee: i64,
    cycles: Option<i64>,
}

/// A distinct transaction whose molecule grows with `extra_bytes` (the size
/// lives in `raw` so it is part of the hash and of the serialized size).
fn fixture_tx(seed: u32, extra_bytes: usize) -> FixtureTx {
    let mut dep = [0u8; 32];
    dep[..4].copy_from_slice(&seed.to_le_bytes());
    dep[31] = 0xA5;
    let tx = ckb_types::core::TransactionBuilder::default()
        .header_dep(packed::Byte32::new(dep))
        .output(packed::CellOutput::new_builder().build())
        .output_data(Bytes::from(vec![0x5Au8; extra_bytes]).pack())
        .build();
    FixtureTx {
        tx,
        fee: 1_000 + i64::from(seed),
        cycles: Some(1_000_000 + i64::from(seed)),
    }
}

fn short_id(tx: &FixtureTx) -> ProposalShortId {
    tx.tx.proposal_short_id()
}

fn hex32(hash: [u8; 32]) -> String {
    format!("0x{}", hex::encode(hash))
}

fn tx_hash(tx: &FixtureTx) -> String {
    let hash: [u8; 32] = tx.tx.hash().unpack();
    hex32(hash)
}

fn short_id_hex(id: &ProposalShortId) -> String {
    format!("0x{}", hex::encode(id.as_slice()))
}

/// What one block of the fixture chain carries besides its cellbase.
#[derive(Clone, Default)]
struct BlockSpec {
    proposals: Vec<ProposalShortId>,
    /// One entry per embedded uncle: that uncle's proposal zone.
    uncles: Vec<Vec<ProposalShortId>>,
    txs: Vec<FixtureTx>,
}

/// Blocks `0..=tip`, each with a cellbase at index 0.
struct FixtureChain {
    blocks: Vec<BlockView>,
    specs: BTreeMap<u64, BlockSpec>,
}

impl FixtureChain {
    fn build(tip: u64, mut specs: BTreeMap<u64, BlockSpec>) -> Self {
        let epoch = EpochNumberWithFraction::new(1, 0, 1800);
        let mut blocks: Vec<BlockView> = Vec::new();
        let mut parent = packed::Byte32::zero();
        for number in 0..=tip {
            let spec = specs.entry(number).or_default().clone();
            let cellbase = ckb_types::core::TransactionBuilder::default()
                .input(packed::CellInput::new_cellbase_input(number))
                .output(packed::CellOutput::new_builder().build())
                .output_data(Bytes::new().pack())
                .witness(Bytes::from(vec![0u8; 8]).pack())
                .build();
            let mut builder = BlockBuilder::default()
                .number(number.pack())
                .epoch(epoch.pack())
                .parent_hash(parent.clone())
                .timestamp((1_700_000_000_000u64 + number * 10_000).pack())
                .transaction(cellbase);
            for tx in &spec.txs {
                builder = builder.transaction(tx.tx.clone());
            }
            for id in &spec.proposals {
                builder = builder.proposal(id.clone());
            }
            for (i, uncle_zone) in spec.uncles.iter().enumerate() {
                let mut uncle = BlockBuilder::default()
                    .number((number - 1).pack())
                    .epoch(epoch.pack())
                    .nonce((0xBEEF_0000u128 + i as u128).pack());
                for id in uncle_zone {
                    uncle = uncle.proposal(id.clone());
                }
                builder = builder.uncle(uncle.build().as_uncle());
            }
            let block = builder.build();
            parent = block.hash();
            blocks.push(block);
        }
        Self { blocks, specs }
    }

    fn tip(&self) -> u64 {
        (self.blocks.len() - 1) as u64
    }

    fn hash(&self, number: u64) -> [u8; 32] {
        self.blocks[number as usize].hash().unpack()
    }

    fn hash_hex(&self, number: u64) -> String {
        hex32(self.hash(number))
    }

    fn uncle_hash_hex(&self, number: u64, index: usize) -> String {
        let hash: [u8; 32] = self.blocks[number as usize]
            .uncles()
            .get(index)
            .expect("fixture uncle")
            .hash()
            .unpack();
        hex32(hash)
    }

    fn cellbase_hash_hex(&self, number: u64) -> String {
        let hash: [u8; 32] = self.blocks[number as usize].transactions()[0]
            .hash()
            .unpack();
        hex32(hash)
    }
}

/// The serialized size CKB counts for a transaction: molecule + 4.
fn size_in_block(tx: &TransactionView) -> i64 {
    tx.data().as_slice().len() as i64 + 4
}

/// What the store side of the fixture should look like. Each knob breaks one
/// invariant on purpose; the default is a coherent chain.
#[derive(Default)]
struct StoreSeed {
    /// Blocks whose store `parent_hash` is corrupted (C5).
    broken_parent: Option<u64>,
    /// Block whose tx index row 0 is stored as a non-cellbase (C3).
    no_cellbase: Option<u64>,
}

fn seed_store(chain: &FixtureChain, seed: &StoreSeed) -> Arc<CkbadgerStore> {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    for block in &chain.blocks {
        let number = block.number();
        let hash: [u8; 32] = block.hash().unpack();
        let mut parent_hash: [u8; 32] = block.parent_hash().unpack();
        if seed.broken_parent == Some(number) {
            parent_hash = [0xEE; 32];
        }
        let spec = &chain.specs[&number];
        let header_cycles: Option<i64> = spec.txs.iter().map(|tx| tx.cycles).sum::<Option<i64>>();
        batch.put_block_header(
            number as i64,
            &CachedBlockHeader {
                hash: hash.to_vec(),
                parent_hash: parent_hash.to_vec(),
                timestamp: block.timestamp() as i64,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 1800,
                dao: vec![0; 32],
                transactions_count: block.transactions().len() as i32,
                uncles_count: block.uncles().data().len() as i32,
                proposals_count: block.data().proposals().len() as i32,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: header_cycles,
            },
        );
        for (index, tx) in block.transactions().iter().enumerate() {
            let tx_hash: [u8; 32] = tx.hash().unpack();
            let (fee, cycles) = if index == 0 {
                (0, None)
            } else {
                let fixture = &spec.txs[index - 1];
                (fixture.fee, fixture.cycles)
            };
            batch.put_tx_hash_map(&tx_hash, number as i64, index as i32);
            batch.put_tx_index(
                number as i64,
                index as i32,
                &TxIndexEntry {
                    is_cellbase: index == 0 && seed.no_cellbase != Some(number),
                    timestamp: block.timestamp() as i64,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee,
                    tx_size: tx.data().as_slice().len() as i32,
                    cycles,
                    semantic_tags: 0,
                },
            );
        }
    }
    batch.commit().unwrap();
    let tip_hash = chain.hash(chain.tip());
    let tip = chain.tip() as i64;
    store
        .update_sync_status(|s| {
            s.tip_block_number = tip;
            s.tip_block_hash = tip_hash.to_vec();
        })
        .unwrap();
    store
}

/// `get_block_economic_state` for every block the fixture's cellbases pay:
/// block `X - 11` is finalized at `X` (mainnet window: farthest 10).
async fn mount_reward_targets(server: &MockServer, chain: &FixtureChain) {
    let pairs: Vec<(String, String)> = (12..=chain.tip())
        .map(|x| (chain.hash_hex(x - 11), chain.hash_hex(x)))
        .collect();
    let pairs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(target, finalized_at)| (target.as_str(), finalized_at.as_str()))
        .collect();
    mount_block_economic_state_rpc(server, &pairs).await;
}

struct PipelineApp {
    app: axum::Router,
    server: MockServer,
    _chain_db: TestCkbChain,
}

/// Seed both stores from `chain`, point the node RPC at a wiremock that
/// answers the reward targets and an empty tx pool at the chain's tip, and
/// mount the production router.
async fn pipeline_app(chain: &FixtureChain, seed: StoreSeed) -> PipelineApp {
    let empty_pool = PoolRpc {
        tip_number: chain.tip(),
        tip_hash: chain.hash_hex(chain.tip()),
        pending: vec![],
        proposed: vec![],
    };
    pipeline_app_with(chain, seed, &chain.blocks, true, Some(empty_pool)).await
}

async fn pipeline_app_with(
    chain: &FixtureChain,
    seed: StoreSeed,
    node_blocks: &[BlockView],
    mount_rewards: bool,
    pool: Option<PoolRpc>,
) -> PipelineApp {
    let store = seed_store(chain, &seed);
    let chain_db = seed_ckb_chain(node_blocks);
    let server = MockServer::start().await;
    if mount_rewards {
        mount_reward_targets(&server, chain).await;
    }
    if let Some(pool) = pool {
        mount_tx_pool_rpc_at(&server, &pool).await;
    }
    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain_db.path.clone(),
        Some(chain_db.cleanup.clone()),
    );
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    PipelineApp {
        app,
        server,
        _chain_db: chain_db,
    }
}

fn block_json(json: &serde_json::Value, number: u64) -> &serde_json::Value {
    json["blocks"]
        .as_array()
        .expect("blocks array")
        .iter()
        .find(|b| b["number"] == number)
        .unwrap_or_else(|| panic!("block {number} in snapshot: {json}"))
}

fn proposal_json<'a>(block: &'a serde_json::Value, id: &ProposalShortId) -> &'a serde_json::Value {
    let id = short_id_hex(id);
    block["proposals"]
        .as_array()
        .expect("proposals array")
        .iter()
        .find(|p| p["shortId"] == id.as_str())
        .unwrap_or_else(|| panic!("proposal {id} in block {block}"))
}

fn tx_json<'a>(block: &'a serde_json::Value, hash: &str) -> &'a serde_json::Value {
    block["txs"]
        .as_array()
        .expect("txs array")
        .iter()
        .find(|t| t["hash"] == hash)
        .unwrap_or_else(|| panic!("tx {hash} in block {block}"))
}

fn specs(entries: Vec<(u64, BlockSpec)>) -> BTreeMap<u64, BlockSpec> {
    entries.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Chain section
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_chain_blocks_and_proposal_states() {
    let committed_close = fixture_tx(1, 40);
    let committed_far = fixture_tx(2, 400);
    let small = fixture_tx(3, 0);
    let expired_id = ProposalShortId::new([0x11; 10]);
    let waiting_id = ProposalShortId::new([0x22; 10]);
    let early_id = ProposalShortId::new([0x33; 10]);

    let chain = FixtureChain::build(
        13,
        specs(vec![
            (
                2,
                BlockSpec {
                    proposals: vec![expired_id.clone()],
                    ..Default::default()
                },
            ),
            (
                3,
                BlockSpec {
                    proposals: vec![short_id(&committed_close), short_id(&small)],
                    ..Default::default()
                },
            ),
            (
                4,
                BlockSpec {
                    proposals: vec![short_id(&committed_far)],
                    ..Default::default()
                },
            ),
            (
                5,
                BlockSpec {
                    txs: vec![committed_close.clone(), small.clone()],
                    ..Default::default()
                },
            ),
            (
                11,
                BlockSpec {
                    txs: vec![committed_far.clone()],
                    ..Default::default()
                },
            ),
            (
                12,
                BlockSpec {
                    proposals: vec![waiting_id.clone()],
                    ..Default::default()
                },
            ),
            (
                13,
                BlockSpec {
                    proposals: vec![early_id.clone()],
                    ..Default::default()
                },
            ),
        ]),
    );
    let fixture = pipeline_app(&chain, StoreSeed::default()).await;

    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    // The consensus the snapshot was derived under travels with it.
    assert_eq!(json["consensus"]["proposalWindowClosest"], 2);
    assert_eq!(json["consensus"]["proposalWindowFarthest"], 10);
    assert_eq!(json["consensus"]["maxBlockBytes"], 597_000);
    assert_eq!(json["consensus"]["maxBlockCycles"], 3_500_000_000u64);
    assert_eq!(json["consensus"]["maxBlockProposals"], 1500);
    assert_eq!(json["tip"]["number"], 13);
    assert_eq!(json["tip"]["hash"], chain.hash_hex(13));

    // Newest first. Genesis is not a pipeline block (not mined, proposes and
    // commits nothing), so a 14-block chain shows 13 cards.
    let numbers: Vec<u64> = json["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["number"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, (1..=13).rev().collect::<Vec<u64>>());
    let b13 = block_json(&json, 13);
    assert_eq!(b13["hash"], chain.hash_hex(13));
    assert_eq!(b13["parentHash"], chain.hash_hex(12));

    // The four proposal states, each on the block that carries it.
    let expired = proposal_json(block_json(&json, 2), &expired_id);
    assert_eq!(expired["state"], "expired");
    assert_eq!(expired["txHash"], serde_json::Value::Null);
    assert_eq!(expired["committedBlock"], serde_json::Value::Null);
    assert_eq!(expired["uncleIndex"], serde_json::Value::Null);

    let close = proposal_json(block_json(&json, 3), &short_id(&committed_close));
    assert_eq!(close["state"], "committed");
    assert_eq!(close["txHash"], tx_hash(&committed_close));
    assert_eq!(close["committedBlock"], 5);

    let far = proposal_json(block_json(&json, 4), &short_id(&committed_far));
    assert_eq!(far["state"], "committed");
    assert_eq!(far["committedBlock"], 11);

    let waiting = proposal_json(block_json(&json, 12), &waiting_id);
    assert_eq!(waiting["state"], "waiting");
    let early = proposal_json(block_json(&json, 13), &early_id);
    assert_eq!(early["state"], "early");

    // Committed transactions and their proposal attribution.
    let b5 = block_json(&json, 5);
    assert_eq!(b5["txCount"], 3, "cellbase counts: {b5}");
    let close_tx = tx_json(b5, &tx_hash(&committed_close));
    assert_eq!(close_tx["proposedIn"]["block"], 3);
    assert_eq!(
        close_tx["proposedIn"]["uncleIndex"],
        serde_json::Value::Null
    );
    assert_eq!(close_tx["size"], size_in_block(&committed_close.tx));
    assert_eq!(close_tx["fee"], committed_close.fee.to_string());
    assert_eq!(
        close_tx["feeRate"],
        (committed_close.fee * 1000 / size_in_block(&committed_close.tx)).to_string()
    );
    assert_eq!(close_tx["cycles"], committed_close.cycles.unwrap());
    assert_eq!(close_tx["cyclesStatus"], "done");
    // Bytes = Σ (molecule + 4) over every transaction, cellbase included.
    let block5 = &chain.blocks[5];
    let expected_bytes: i64 = block5.transactions().iter().map(size_in_block).sum();
    assert_eq!(b5["bytes"], expected_bytes);
    assert_eq!(b5["cellbase"]["hash"], chain.cellbase_hash_hex(5));
    assert_eq!(
        b5["cellbase"]["size"],
        size_in_block(&block5.transactions()[0])
    );
    // Largest first.
    let sizes: Vec<i64> = b5["txs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["size"].as_i64().unwrap())
        .collect();
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
    assert_eq!(b5["omittedTxs"], 0);
    assert_eq!(
        b5["cycles"],
        committed_close.cycles.unwrap() + small.cycles.unwrap()
    );
    assert_eq!(b5["cyclesPending"], false);
    assert_eq!(b5["uncles"], serde_json::json!([]));

    let b11 = block_json(&json, 11);
    assert_eq!(
        tx_json(b11, &tx_hash(&committed_far))["proposedIn"]["block"],
        4
    );
}

#[tokio::test]
async fn test_pipeline_uncle_proposals_count_at_including_block() {
    let via_uncle = fixture_tx(7, 10);
    let own = ProposalShortId::new([0x44; 10]);
    let uncle_only = ProposalShortId::new([0x55; 10]);

    let chain = FixtureChain::build(
        12,
        specs(vec![
            (
                6,
                BlockSpec {
                    proposals: vec![own.clone()],
                    uncles: vec![vec![short_id(&via_uncle), uncle_only.clone()]],
                    ..Default::default()
                },
            ),
            (
                8,
                BlockSpec {
                    txs: vec![via_uncle.clone()],
                    ..Default::default()
                },
            ),
        ]),
    );
    let fixture = pipeline_app(&chain, StoreSeed::default()).await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    let b6 = block_json(&json, 6);
    assert_eq!(
        b6["uncles"],
        serde_json::json!([{ "number": 5, "hash": chain.uncle_hash_hex(6, 0) }])
    );
    // Zone order: the block's own proposals first, then its uncles'.
    let ids: Vec<&str> = b6["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["shortId"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            short_id_hex(&own),
            short_id_hex(&short_id(&via_uncle)),
            short_id_hex(&uncle_only)
        ]
    );
    assert_eq!(
        proposal_json(b6, &own)["uncleIndex"],
        serde_json::Value::Null
    );
    let carried = proposal_json(b6, &short_id(&via_uncle));
    assert_eq!(carried["uncleIndex"], 0);
    assert_eq!(carried["state"], "committed");
    assert_eq!(carried["committedBlock"], 8);
    assert_eq!(proposal_json(b6, &uncle_only)["uncleIndex"], 0);

    // The uncle-borne proposal counts at the block that embeds the uncle (6),
    // not at the uncle's own height (5).
    let committed = tx_json(block_json(&json, 8), &tx_hash(&via_uncle));
    assert_eq!(committed["proposedIn"]["block"], 6);
    assert_eq!(committed["proposedIn"]["uncleIndex"], 0);
}

#[tokio::test]
async fn test_pipeline_committed_tx_proposed_in_earliest_in_window() {
    let twice = fixture_tx(9, 0);
    let chain = FixtureChain::build(
        8,
        specs(vec![
            (
                2,
                BlockSpec {
                    proposals: vec![short_id(&twice)],
                    ..Default::default()
                },
            ),
            (
                4,
                BlockSpec {
                    proposals: vec![short_id(&twice)],
                    ..Default::default()
                },
            ),
            (
                6,
                BlockSpec {
                    txs: vec![twice.clone()],
                    ..Default::default()
                },
            ),
        ]),
    );
    let fixture = pipeline_app(&chain, StoreSeed::default()).await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    let committed = tx_json(block_json(&json, 6), &tx_hash(&twice));
    assert_eq!(
        committed["proposedIn"]["block"], 2,
        "the earliest proposal in [c - farthest, c - closest] is the one the proposal reward pays"
    );
}

#[tokio::test]
async fn test_pipeline_cellbase_pays_finalized_block() {
    let chain = FixtureChain::build(13, BTreeMap::new());
    let fixture = pipeline_app(&chain, StoreSeed::default()).await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    for x in [12u64, 13] {
        assert_eq!(
            block_json(&json, x)["cellbase"]["pays"],
            serde_json::json!({ "number": x - 11, "hash": chain.hash_hex(x - 11) }),
            "block {x}'s cellbase pays block {}",
            x - 11
        );
    }
    // Blocks 1..=11 pay nothing: their target would be genesis or earlier.
    for x in 1u64..=11 {
        assert_eq!(
            block_json(&json, x)["cellbase"]["pays"],
            serde_json::Value::Null,
            "block {x}"
        );
    }
}

#[tokio::test]
async fn test_pipeline_cellbase_target_finalized_elsewhere_fails() {
    let chain = FixtureChain::build(13, BTreeMap::new());
    // The node says block 2 was finalized at block 12, not 13 (C4).
    let server_pairs = [
        (chain.hash_hex(1), chain.hash_hex(12)),
        (chain.hash_hex(2), chain.hash_hex(12)),
    ];
    let pairs: Vec<(&str, &str)> = server_pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let fixture = pipeline_app_with(
        &chain,
        StoreSeed::default(),
        &chain.blocks,
        false,
        Some(PoolRpc {
            tip_number: chain.tip(),
            tip_hash: chain.hash_hex(chain.tip()),
            pending: vec![],
            proposed: vec![],
        }),
    )
    .await;
    mount_block_economic_state_rpc(&fixture.server, &pairs).await;

    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("C4"), "{message}");
    assert!(message.contains(&chain.hash_hex(13)), "{message}");
    assert!(message.contains(&chain.hash_hex(2)), "{message}");
}

#[tokio::test]
async fn test_pipeline_committed_tx_without_proposal_fails() {
    let unproposed = fixture_tx(21, 0);
    let chain = FixtureChain::build(
        9,
        specs(vec![(
            7,
            BlockSpec {
                txs: vec![unproposed.clone()],
                ..Default::default()
            },
        )]),
    );
    let fixture = pipeline_app(&chain, StoreSeed::default()).await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("C2"), "{message}");
    assert!(message.contains("block 7"), "{message}");
    assert!(message.contains(&tx_hash(&unproposed)), "{message}");
}

#[tokio::test]
async fn test_pipeline_caps_block_txs() {
    // 299 transactions plus the cellbase: 300 in the block.
    let txs: Vec<FixtureTx> = (0..299u32)
        .map(|i| fixture_tx(100 + i, (i % 7) as usize * 50))
        .collect();
    let chain = FixtureChain::build(
        4,
        specs(vec![
            (
                1,
                BlockSpec {
                    proposals: txs.iter().map(short_id).collect(),
                    ..Default::default()
                },
            ),
            (
                3,
                BlockSpec {
                    txs: txs.clone(),
                    ..Default::default()
                },
            ),
        ]),
    );
    let fixture = pipeline_app(&chain, StoreSeed::default()).await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    let b3 = block_json(&json, 3);
    assert_eq!(b3["txCount"], 300);
    assert_eq!(b3["txs"].as_array().unwrap().len(), 238);
    assert_eq!(b3["omittedTxs"], 61);
    // The cap keeps the largest transactions, largest first.
    let served: Vec<i64> = b3["txs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["size"].as_i64().unwrap())
        .collect();
    let mut expected: Vec<i64> = txs.iter().map(|t| size_in_block(&t.tx)).collect();
    expected.sort_unstable_by(|a, b| b.cmp(a));
    expected.truncate(238);
    assert_eq!(served, expected);
    // Bytes still cover every transaction, kept or not.
    let expected_bytes: i64 = chain.blocks[3]
        .transactions()
        .iter()
        .map(size_in_block)
        .sum();
    assert_eq!(b3["bytes"], expected_bytes);
}

#[tokio::test]
async fn test_pipeline_requires_the_node_db() {
    let chain = FixtureChain::build(3, BTreeMap::new());
    let store = seed_store(&chain, &StoreSeed::default());
    // `test_app_state` opens no CKB node reader.
    let app = create_router_without_warmup(test_config(store));
    let (status, json) = get_json(&app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    assert!(
        json["message"].as_str().unwrap().contains("ckb_db_path"),
        "{json}"
    );
}

#[tokio::test]
async fn test_pipeline_block_missing_from_node_fails() {
    let chain = FixtureChain::build(6, BTreeMap::new());
    // The node DB lacks block 4 although the store indexed it (C1).
    let node_blocks: Vec<BlockView> = chain
        .blocks
        .iter()
        .filter(|b| b.number() != 4)
        .cloned()
        .collect();
    let fixture = pipeline_app_with(&chain, StoreSeed::default(), &node_blocks, true, None).await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("C1"), "{message}");
    assert!(message.contains("block 4"), "{message}");
    assert!(message.contains(&chain.hash_hex(4)), "{message}");
}

#[tokio::test]
async fn test_pipeline_block_without_cellbase_fails() {
    let chain = FixtureChain::build(5, BTreeMap::new());
    let fixture = pipeline_app(
        &chain,
        StoreSeed {
            no_cellbase: Some(3),
            ..Default::default()
        },
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("C3"), "{message}");
    assert!(message.contains("block 3"), "{message}");
}

#[tokio::test]
async fn test_pipeline_broken_parent_link_fails() {
    let chain = FixtureChain::build(5, BTreeMap::new());
    let fixture = pipeline_app(
        &chain,
        StoreSeed {
            broken_parent: Some(4),
            ..Default::default()
        },
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("C5"), "{message}");
    assert!(message.contains("block 4"), "{message}");
}

// ---------------------------------------------------------------------------
// Pool section, NEXT prediction, tip consistency
// ---------------------------------------------------------------------------

/// A pool transaction hash whose proposal short id is `short_id`.
fn pool_hash(short_id: [u8; 10], tail: u8) -> [u8; 32] {
    let mut hash = [tail; 32];
    hash[..10].copy_from_slice(&short_id);
    hash
}

fn pool_entry(hash: [u8; 32], size: u64, fee: u64, cycles: u64, arrived_at_ms: u64) -> PoolRpcTx {
    PoolRpcTx {
        hash: hex32(hash),
        size,
        fee,
        cycles,
        timestamp_ms: arrived_at_ms,
    }
}

fn pool_at_tip(chain: &FixtureChain, pending: Vec<PoolRpcTx>, proposed: Vec<PoolRpcTx>) -> PoolRpc {
    PoolRpc {
        tip_number: chain.tip(),
        tip_hash: chain.hash_hex(chain.tip()),
        pending,
        proposed,
    }
}

fn pool_tx_json(list: &serde_json::Value, hash: [u8; 32]) -> &serde_json::Value {
    let hash = hex32(hash);
    list.as_array()
        .expect("pool list")
        .iter()
        .find(|t| t["hash"] == hash.as_str())
        .unwrap_or_else(|| panic!("pool tx {hash} in {list}"))
}

#[tokio::test]
async fn test_pipeline_pool_tip_mismatch() {
    let chain = FixtureChain::build(13, BTreeMap::new());
    let stray = pool_entry(pool_hash([0xA9; 10], 1), 500, 1_000, 10, 1);
    // Behind by one block; and the same height on another fork.
    for (number, hash) in [(12u64, chain.hash_hex(12)), (13, hex32([0x99; 32]))] {
        let fixture = pipeline_app_with(
            &chain,
            StoreSeed::default(),
            &chain.blocks,
            true,
            Some(PoolRpc {
                tip_number: number,
                tip_hash: hash.clone(),
                pending: vec![stray.clone()],
                proposed: vec![],
            }),
        )
        .await;
        let (status, json) = get_json(&fixture.app, "/pipeline").await;
        assert_eq!(status, StatusCode::OK, "got {json}");
        assert_eq!(
            json["status"], "poolTipMismatch",
            "pool tip {number} {hash}"
        );
        assert_eq!(
            json["poolTip"],
            serde_json::json!({ "number": number, "hash": hash })
        );
        assert_eq!(json["tip"]["number"], 13);
        assert_eq!(json["pool"], serde_json::Value::Null);
        assert_eq!(json["next"], serde_json::Value::Null);
        assert_eq!(
            json["blocks"].as_array().unwrap().len(),
            13,
            "the chain is still served"
        );
    }
}

#[tokio::test]
async fn test_pipeline_pool_stage_from_chain_zones() {
    let gap_id = [0xA1; 10];
    let set_id = [0xA2; 10];
    let uncle_id = [0xA3; 10];
    let fresh_id = [0xA4; 10];
    let chain = FixtureChain::build(
        20,
        specs(vec![
            (
                17,
                BlockSpec {
                    proposals: vec![ProposalShortId::new(set_id)],
                    ..Default::default()
                },
            ),
            (
                18,
                BlockSpec {
                    uncles: vec![vec![ProposalShortId::new(uncle_id)]],
                    ..Default::default()
                },
            ),
            (
                20,
                BlockSpec {
                    proposals: vec![ProposalShortId::new(gap_id)],
                    ..Default::default()
                },
            ),
        ]),
    );
    let gap = pool_entry(pool_hash(gap_id, 1), 400, 4_000, 100, 10);
    let set = pool_entry(pool_hash(set_id, 2), 500, 2_000, 200, 20);
    let uncle = pool_entry(pool_hash(uncle_id, 3), 600, 1_200, 300, 30);
    let fresh = pool_entry(pool_hash(fresh_id, 4), 700, 7_000, 400, 40);
    // The node lists the tip-proposed tx as pending (its "gap"); the chain's
    // zones say it is proposed, just too recent for the next block.
    let pool = pool_at_tip(
        &chain,
        vec![gap.clone(), fresh.clone()],
        vec![set.clone(), uncle.clone()],
    );
    let fixture = pipeline_app_with(
        &chain,
        StoreSeed::default(),
        &chain.blocks,
        true,
        Some(pool),
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["poolTip"], json["tip"]);

    let pool = &json["pool"];
    assert_eq!(pool["pendingTotal"], 1);
    assert_eq!(pool["proposedTotal"], 3);
    let pending = pool_tx_json(&pool["pending"], pool_hash(fresh_id, 4));
    assert_eq!(pending["proposedIn"], serde_json::Value::Null);
    assert_eq!(pending["size"], 700);
    assert_eq!(pending["fee"], "7000");
    assert_eq!(pending["feeRate"], (7_000u64 * 1000 / 700).to_string());
    assert_eq!(pending["cycles"], 400);
    assert_eq!(pending["arrivedAt"], 40);

    // Newest proposal first.
    let order: Vec<&str> = pool["proposed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["hash"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        vec![gap.hash.as_str(), uncle.hash.as_str(), set.hash.as_str()]
    );
    assert_eq!(
        pool_tx_json(&pool["proposed"], pool_hash(gap_id, 1))["proposedIn"],
        serde_json::json!({ "block": 20, "uncleIndex": null })
    );
    assert_eq!(
        pool_tx_json(&pool["proposed"], pool_hash(set_id, 2))["proposedIn"],
        serde_json::json!({ "block": 17, "uncleIndex": null })
    );
    assert_eq!(
        pool_tx_json(&pool["proposed"], pool_hash(uncle_id, 3))["proposedIn"],
        serde_json::json!({ "block": 18, "uncleIndex": 0 })
    );

    // The proposal each staged pool tx points at names it.
    let early = proposal_json(block_json(&json, 20), &ProposalShortId::new(gap_id));
    assert_eq!(early["state"], "early");
    assert_eq!(early["txHash"], gap.hash);
    let waiting = proposal_json(block_json(&json, 17), &ProposalShortId::new(set_id));
    assert_eq!(waiting["state"], "waiting");
    assert_eq!(waiting["txHash"], set.hash);
    let carried = proposal_json(block_json(&json, 18), &ProposalShortId::new(uncle_id));
    assert_eq!(carried["uncleIndex"], 0);
    assert_eq!(carried["txHash"], uncle.hash);

    // NEXT may commit only what the window allows: not the gap tx.
    let next: Vec<&str> = json["next"]["txs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["hash"].as_str().unwrap())
        .collect();
    assert_eq!(next.len(), 2, "{json}");
    assert!(next.contains(&set.hash.as_str()) && next.contains(&uncle.hash.as_str()));
    assert_eq!(json["next"]["proposals"], 1, "one unproposed tx to propose");
}

#[tokio::test]
async fn test_pipeline_pool_invariant_p1() {
    let chain = FixtureChain::build(13, BTreeMap::new());
    let unproposed = pool_entry(pool_hash([0xB7; 10], 7), 500, 1_000, 10, 1);
    let pool = pool_at_tip(&chain, vec![], vec![unproposed.clone()]);
    let fixture = pipeline_app_with(
        &chain,
        StoreSeed::default(),
        &chain.blocks,
        true,
        Some(pool),
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("P1"), "{message}");
    assert!(message.contains(&unproposed.hash), "{message}");
    assert!(message.contains("tip 13"), "{message}");
}

/// A node without `[block_assembler]` never moves pool entries out of pending
/// (ckb v0.209 `tx-pool/src/process.rs` `_update_tx_pool_for_reorg`, `mine_mode`), so
/// its pending label says nothing about proposals. The stage comes from the chain.
/// Live mainnet regression: a 500 "P2: the node lists .. as pending" on a non-mining node.
#[tokio::test]
async fn test_pipeline_pool_stage_ignores_non_mining_pending_label() {
    let id = [0xB8; 10];
    let chain = FixtureChain::build(
        13,
        specs(vec![(
            10,
            BlockSpec {
                proposals: vec![ProposalShortId::new(id)],
                ..Default::default()
            },
        )]),
    );
    // Proposed at tip - 3, committable — and the non-mining node still calls it pending.
    let tx = pool_entry(pool_hash(id, 8), 500, 1_000, 10, 1);
    let pool = pool_at_tip(&chain, vec![tx.clone()], vec![]);
    let fixture = pipeline_app_with(
        &chain,
        StoreSeed::default(),
        &chain.blocks,
        true,
        Some(pool),
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(
        json["pool"]["pending"].as_array().unwrap().len(),
        0,
        "{json}"
    );
    let proposed = json["pool"]["proposed"].as_array().unwrap();
    assert_eq!(proposed.len(), 1, "{json}");
    assert_eq!(proposed[0]["hash"], tx.hash);
    assert_eq!(proposed[0]["proposedIn"]["block"], 10);
    let next: Vec<&str> = json["next"]["txs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["hash"].as_str().unwrap())
        .collect();
    assert_eq!(next, vec![tx.hash.as_str()], "the next block may commit it");
}

#[tokio::test]
async fn test_pipeline_next_prediction() {
    let ids: Vec<[u8; 10]> = (0..5u8).map(|i| [0xD0 + i; 10]).collect();
    let gap_id = [0xDA; 10];
    let chain = FixtureChain::build(
        20,
        specs(vec![
            (
                18,
                BlockSpec {
                    proposals: ids.iter().map(|id| ProposalShortId::new(*id)).collect(),
                    ..Default::default()
                },
            ),
            (
                20,
                BlockSpec {
                    proposals: vec![ProposalShortId::new(gap_id)],
                    ..Default::default()
                },
            ),
        ]),
    );
    // feeRate = fee * 1000 / size.
    let best = pool_entry(pool_hash(ids[0], 1), 300_000, 300_000_000, 1_000, 50); // 1_000_000
    let too_big = pool_entry(pool_hash(ids[1], 2), 300_000, 240_000_000, 1_000, 10); // 800_000, no room left
    let early_arrival = pool_entry(pool_hash(ids[2], 3), 2_000, 1_000_000, 2_000, 5); // 500_000
    let late_arrival = pool_entry(pool_hash(ids[3], 4), 1_000, 500_000, 3_000, 60); // 500_000
    let cycle_heavy = pool_entry(pool_hash(ids[4], 5), 500, 100_000, 3_500_000_000, 1); // 200_000, cycles overflow
    let gap = pool_entry(pool_hash(gap_id, 6), 100, 1_000_000, 10, 1); // best rate, but too early
    let fresh = pool_entry(pool_hash([0xDF; 10], 7), 100, 1_000_000, 10, 1);
    let pool = pool_at_tip(
        &chain,
        vec![gap.clone(), fresh.clone()],
        vec![
            best.clone(),
            too_big.clone(),
            early_arrival.clone(),
            late_arrival.clone(),
            cycle_heavy.clone(),
        ],
    );
    let fixture = pipeline_app_with(
        &chain,
        StoreSeed::default(),
        &chain.blocks,
        true,
        Some(pool),
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    let next = &json["next"];
    assert_eq!(
        next["txs"],
        serde_json::json!([
            { "hash": best.hash, "size": 300_000 },
            { "hash": early_arrival.hash, "size": 2_000 },
            { "hash": late_arrival.hash, "size": 1_000 },
        ]),
        "greedy by fee rate within maxBlockBytes / maxBlockCycles, served largest first"
    );
    assert_eq!(next["omittedTxs"], 0);
    assert_eq!(next["bytes"], 303_000);
    assert_eq!(next["cycles"], 6_000);
    // min(unproposed pool txs, maxBlockProposals): the gap tx is already proposed.
    assert_eq!(next["proposals"], 1);
}

#[tokio::test]
async fn test_pipeline_pool_caps_and_totals() {
    let proposed_ids: Vec<[u8; 10]> = (0..210u16)
        .map(|i| {
            let mut id = [0xC0; 10];
            id[1..3].copy_from_slice(&i.to_be_bytes());
            id
        })
        .collect();
    let chain = FixtureChain::build(
        20,
        specs(vec![(
            19,
            BlockSpec {
                proposals: proposed_ids
                    .iter()
                    .map(|id| ProposalShortId::new(*id))
                    .collect(),
                ..Default::default()
            },
        )]),
    );
    let pending: Vec<PoolRpcTx> = (0..150u16)
        .map(|i| {
            let mut id = [0xE0; 10];
            id[1..3].copy_from_slice(&i.to_be_bytes());
            pool_entry(pool_hash(id, 1), 300, 1_000, 10, u64::from(i))
        })
        .collect();
    let proposed: Vec<PoolRpcTx> = proposed_ids
        .iter()
        .enumerate()
        .map(|(i, id)| pool_entry(pool_hash(*id, 2), 300, 1_000, 10, i as u64))
        .collect();
    let pool = pool_at_tip(&chain, pending, proposed);
    let fixture = pipeline_app_with(
        &chain,
        StoreSeed::default(),
        &chain.blocks,
        true,
        Some(pool),
    )
    .await;
    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");

    let pool = &json["pool"];
    assert_eq!(pool["pending"].as_array().unwrap().len(), 136);
    assert_eq!(pool["pendingTotal"], 150);
    assert_eq!(pool["proposed"].as_array().unwrap().len(), 204);
    assert_eq!(pool["proposedTotal"], 210);
    // The oldest pending arrivals are the ones kept.
    assert_eq!(pool["pending"][0]["arrivedAt"], 0);
    assert_eq!(pool["pending"][135]["arrivedAt"], 135);
    // Truncation is the grids' only: NEXT still sees every committable tx.
    assert_eq!(json["next"]["txs"].as_array().unwrap().len(), 210);
}

/// The node's tx-pool sizes an entry as `tx.data().serialized_size_in_block()`
/// (molecule + 4) — the convention the store's committed sizes are served in.
/// Served side by side, the same transaction must report one size whether it
/// sits in the pool or in a block. The node-side convention is asserted here
/// from fixture bytes; it is to be confirmed against a live node (plan Task 12).
#[tokio::test]
async fn test_pool_size_matches_committed_size_convention() {
    let tx = fixture_tx(77, 123);
    let node_pool_size = tx.tx.data().serialized_size_in_block() as u64;

    // Committed in block 5 (proposed in 3).
    let committed_chain = FixtureChain::build(
        6,
        specs(vec![
            (
                3,
                BlockSpec {
                    proposals: vec![short_id(&tx)],
                    ..Default::default()
                },
            ),
            (
                5,
                BlockSpec {
                    txs: vec![tx.clone()],
                    ..Default::default()
                },
            ),
        ]),
    );
    let committed = pipeline_app(&committed_chain, StoreSeed::default()).await;
    let (status, json) = get_json(&committed.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let committed_size = tx_json(block_json(&json, 5), &tx_hash(&tx))["size"].clone();

    // In the pool, proposed in block 5 of another chain at tip 6.
    let pool_chain = FixtureChain::build(
        6,
        specs(vec![(
            5,
            BlockSpec {
                proposals: vec![short_id(&tx)],
                ..Default::default()
            },
        )]),
    );
    let hash: [u8; 32] = tx.tx.hash().unpack();
    let pool = pool_at_tip(
        &pool_chain,
        vec![],
        vec![pool_entry(hash, node_pool_size, 1_000, 10, 1)],
    );
    let pooled = pipeline_app_with(
        &pool_chain,
        StoreSeed::default(),
        &pool_chain.blocks,
        true,
        Some(pool),
    )
    .await;
    let (status, json) = get_json(&pooled.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let pooled_size = pool_tx_json(&json["pool"]["proposed"], hash)["size"].clone();

    assert_eq!(pooled_size, committed_size);
    assert_eq!(committed_size, size_in_block(&tx.tx));
}

/// The node's tip moved between the two `tx_pool_info` reads that bracket the
/// verbose pool: the pool may belong to either tip, so it is not served
/// against the chain the snapshot shows.
#[tokio::test]
async fn test_pipeline_pool_read_straddling_a_block_is_a_mismatch() {
    let chain = FixtureChain::build(13, BTreeMap::new());
    let fixture = pipeline_app_with(&chain, StoreSeed::default(), &chain.blocks, true, None).await;
    let next_tip = hex32([0x14; 32]);
    let info = |number: u64, hash: &str| {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tip_hash": hash,
                "tip_number": format!("0x{number:x}"),
                "last_txs_updated_at": "0x1",
                "pending": "0x0",
                "proposed": "0x0",
                "orphan": "0x0"
            }
        }))
    };
    // First read: ckbadger's tip. Every later read: the next block.
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "tx_pool_info" }),
        ))
        .respond_with(info(13, &chain.hash_hex(13)))
        .up_to_n_times(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "tx_pool_info" }),
        ))
        .respond_with(info(14, &next_tip))
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "get_raw_tx_pool" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "pending": {}, "proposed": {} }
        })))
        .mount(&fixture.server)
        .await;

    let (status, json) = get_json(&fixture.app, "/pipeline").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["status"], "poolTipMismatch");
    assert_eq!(
        json["poolTip"],
        serde_json::json!({ "number": 14, "hash": next_tip })
    );
    assert_eq!(json["pool"], serde_json::Value::Null);
    assert_eq!(json["next"], serde_json::Value::Null);
}
