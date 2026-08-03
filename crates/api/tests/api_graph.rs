mod common;

use common::*;

// ---------------------------------------------------------------------------
// `/graph/proposals/{block_number}` must render the block's WHOLE proposal
// zone. CKB consensus counts the proposal zones of the uncles a block embeds,
// so a proposal borne by an uncle belongs to the main-chain block that embeds
// it — exactly the rule `/transactions/{hash}/lifecycle` settled on in
// 288730bb. Reading only `block.data().proposals()` dropped those proposals
// from the flow graph and undercounted `totalProposals`.
//
// The fixture reproduces the shape of mainnet block 19596440
// (0xa40ca5a66d10144a83f6494108ecb327ef6302590584b85074b44e751c70b2be): own
// proposal zone 0x9fd3b290a9560daa255d + 0x2fbd66af676621b0e1f9, plus the
// embedded uncle 19596438
// (0x0761f11409d71fa7b57c5207089ce01a6dd4740a982fea69c9b7c04e0815bab9) whose
// zone carries 0x7bee57b10784cafcdadf + 0xb7a46a33fedaa8d75a0a. All four
// commit two blocks later, in 19596442.
//
// The short ids themselves are derived from the fixture transactions rather
// than pasted from mainnet: `CkbChainReader::get_block` rebuilds the block
// through `packed::Block::into_view()`, which recomputes every transaction
// hash from its body, so a stored hash cannot be faked. The structure — which
// zone carries which id, and which block commits it — is what this endpoint
// gets wrong, and that is preserved exactly.
// ---------------------------------------------------------------------------

const SOURCE_BLOCK: u64 = 19_596_440;
const UNCLE_BLOCK: u64 = 19_596_438;
const COMMIT_BLOCK: u64 = 19_596_442;

/// Distinct fixture transactions for the commit block, index 0 standing in for
/// the cellbase (no proposal points at it).
///
/// The distinguishing field must live in `raw` — a transaction hash is the
/// hash of `raw` only, so varying witnesses alone yields five identical hashes
/// and therefore one collapsed proposal short id.
fn committed_transactions() -> Vec<ckb_types::core::TransactionView> {
    (0u8..5)
        .map(|i| {
            ckb_types::core::TransactionBuilder::default()
                .header_dep(ckb_types::packed::Byte32::new([i; 32]))
                .build()
        })
        .collect()
}

fn short_id_hex(tx: &ckb_types::core::TransactionView) -> String {
    use ckb_types::prelude::*;
    format!("0x{}", hex::encode(tx.proposal_short_id().as_slice()))
}

fn tx_hash_hex(tx: &ckb_types::core::TransactionView) -> String {
    use ckb_types::prelude::*;
    let hash: [u8; 32] = tx.hash().unpack();
    format!("0x{}", hex::encode(hash))
}

/// One block of the fixture chain: proposal zone, embedded uncles (each with
/// its own proposal zone), and committed transactions.
struct FixtureBlock {
    number: u64,
    proposals: Vec<ckb_types::packed::ProposalShortId>,
    uncles: Vec<(u64, Vec<ckb_types::packed::ProposalShortId>)>,
    transactions: Vec<ckb_types::core::TransactionView>,
}

fn build_fixture_block(spec: &FixtureBlock) -> ckb_types::core::BlockView {
    use ckb_types::core::{BlockBuilder, EpochNumberWithFraction};
    use ckb_types::prelude::*;

    // Non-genesis headers must carry a well-formed epoch (length > index).
    let epoch = EpochNumberWithFraction::new(1, 0, 1800);

    let mut uncle_views = Vec::new();
    for (uncle_number, uncle_proposals) in &spec.uncles {
        let mut uncle = BlockBuilder::default()
            .number(uncle_number.pack())
            .epoch(epoch.pack());
        for id in uncle_proposals {
            uncle = uncle.proposal(id.clone());
        }
        uncle_views.push(uncle.build().as_uncle());
    }

    let mut builder = BlockBuilder::default()
        .number(spec.number.pack())
        .epoch(epoch.pack());
    for id in &spec.proposals {
        builder = builder.proposal(id.clone());
    }
    for uncle in uncle_views {
        builder = builder.uncle(uncle);
    }
    for tx in &spec.transactions {
        builder = builder.transaction(tx.clone());
    }
    builder.build()
}

/// Seed the fixture blocks' ckbadger headers (the handler resolves the source
/// block there first) plus a CKB-node-format RocksDB holding the real blocks.
fn seed_proposal_fixture(specs: &[FixtureBlock]) -> (std::sync::Arc<CkbadgerStore>, TestCkbChain) {
    use ckb_types::prelude::*;

    let store = test_store();
    let blocks: Vec<ckb_types::core::BlockView> = specs.iter().map(build_fixture_block).collect();

    let mut batch = StoreBatch::new(store.as_ref());
    for block in &blocks {
        let hash: [u8; 32] = block.hash().unpack();
        batch.put_block_header(
            block.number() as i64,
            &CachedBlockHeader {
                hash: hash.to_vec(),
                parent_hash: vec![0u8; 32],
                timestamp: 1_700_000_000_000 + block.number() as i64,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 1800,
                dao: vec![0; 32],
                transactions_count: block.transactions().len() as i32,
                uncles_count: block.uncles().hashes().len() as i32,
                proposals_count: block.data().proposals().len() as i32,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
    }
    batch.commit().unwrap();

    let chain = seed_ckb_chain(&blocks);
    (store, chain)
}

async fn proposal_graph(specs: &[FixtureBlock], block_number: u64) -> serde_json::Value {
    let (store, chain) = seed_proposal_fixture(specs);
    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/graph/proposals/{block_number}")).await;
    assert_eq!(status, StatusCode::OK);
    json
}

fn proposal_nodes(json: &serde_json::Value) -> Vec<&serde_json::Value> {
    json["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter(|n| n["nodeType"] == "proposal")
        .collect()
}

fn proposal_node<'a>(json: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    proposal_nodes(json)
        .into_iter()
        .find(|n| n["data"]["proposalId"] == id)
        .unwrap_or_else(|| panic!("no proposal node for {id} in {json}"))
}

#[tokio::test]
async fn test_proposal_graph_honours_uncle_proposal_zone() {
    let txs = committed_transactions();
    // Mirrors mainnet: two ids proposed directly, two carried by the uncle.
    let main_zone = vec![txs[1].proposal_short_id(), txs[3].proposal_short_id()];
    let uncle_zone = vec![txs[2].proposal_short_id(), txs[4].proposal_short_id()];

    let specs = vec![
        FixtureBlock {
            number: SOURCE_BLOCK,
            proposals: main_zone,
            uncles: vec![(UNCLE_BLOCK, uncle_zone)],
            transactions: vec![],
        },
        FixtureBlock {
            number: COMMIT_BLOCK,
            proposals: vec![],
            uncles: vec![],
            transactions: txs.clone(),
        },
    ];
    let json = proposal_graph(&specs, SOURCE_BLOCK).await;

    assert_eq!(
        json["metadata"]["totalProposals"], 4,
        "the block's proposal zone is its own 2 ids plus the 2 its uncle carries, got {json}"
    );
    assert_eq!(json["metadata"]["committedCount"], 4);
    assert_eq!(proposal_nodes(&json).len(), 4);

    // Uncle-borne proposals are attributed to the embedding main-chain block,
    // with the uncle named explicitly.
    let uncle_borne = proposal_node(&json, &short_id_hex(&txs[2]));
    assert_eq!(uncle_borne["data"]["txHash"], tx_hash_hex(&txs[2]));
    assert_eq!(uncle_borne["data"]["commitBlock"], COMMIT_BLOCK);
    assert_eq!(
        uncle_borne["data"]["distance"],
        (COMMIT_BLOCK - SOURCE_BLOCK) as i64
    );
    assert_eq!(
        uncle_borne["data"]["proposedInUncle"]["blockNumber"],
        UNCLE_BLOCK
    );

    // Directly proposed ids carry no uncle attribution.
    assert_eq!(
        proposal_node(&json, &short_id_hex(&txs[1]))["data"]["proposedInUncle"],
        serde_json::Value::Null,
        "a directly proposed id must not name an uncle"
    );

    assert_eq!(
        json["nodes"][0]["data"]["proposalsCount"], 4,
        "the source-block node must count the whole zone"
    );

    // Every proposal links out of the source block and into the commit block.
    let links = json["links"].as_array().expect("links array");
    assert_eq!(
        links.iter().filter(|l| l["linkType"] == "proposes").count(),
        4
    );
    assert_eq!(
        links.iter().filter(|l| l["linkType"] == "commits").count(),
        4
    );
    let commit_node = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["nodeType"] == "commit_block")
        .expect("commit block node");
    assert_eq!(commit_node["data"]["committedCount"], 4);
}

#[tokio::test]
async fn test_proposal_graph_prefers_main_zone_over_uncle_for_same_id() {
    // A block may propose an id directly AND embed an uncle that proposes it.
    // The block's own zone owns the attribution, and the id renders once.
    let txs = committed_transactions();
    let id = txs[1].proposal_short_id();

    let specs = vec![
        FixtureBlock {
            number: SOURCE_BLOCK,
            proposals: vec![id.clone()],
            uncles: vec![(UNCLE_BLOCK, vec![id])],
            transactions: vec![],
        },
        FixtureBlock {
            number: COMMIT_BLOCK,
            proposals: vec![],
            uncles: vec![],
            transactions: txs.clone(),
        },
    ];
    let json = proposal_graph(&specs, SOURCE_BLOCK).await;

    assert_eq!(json["metadata"]["totalProposals"], 1);
    assert_eq!(proposal_nodes(&json).len(), 1);
    assert_eq!(
        proposal_node(&json, &short_id_hex(&txs[1]))["data"]["proposedInUncle"],
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn test_proposal_graph_dedups_ids_shared_by_two_uncles() {
    // Two embedded uncles can carry the same short id. It is one proposal in
    // the block's zone; the first uncle in embedding order names it.
    let txs = committed_transactions();
    let id = txs[2].proposal_short_id();

    let specs = vec![
        FixtureBlock {
            number: SOURCE_BLOCK,
            proposals: vec![],
            uncles: vec![(UNCLE_BLOCK, vec![id.clone()]), (UNCLE_BLOCK - 1, vec![id])],
            transactions: vec![],
        },
        FixtureBlock {
            number: COMMIT_BLOCK,
            proposals: vec![],
            uncles: vec![],
            transactions: txs.clone(),
        },
    ];
    let json = proposal_graph(&specs, SOURCE_BLOCK).await;

    assert_eq!(json["metadata"]["totalProposals"], 1);
    assert_eq!(proposal_nodes(&json).len(), 1);
    assert_eq!(
        proposal_node(&json, &short_id_hex(&txs[2]))["data"]["proposedInUncle"]["blockNumber"],
        UNCLE_BLOCK
    );
}

#[tokio::test]
async fn test_proposal_graph_without_uncles_is_unchanged() {
    // Control: the ordinary uncle-free case keeps reporting exactly the block's
    // own proposal zone, and passes on both revisions.
    let txs = committed_transactions();
    let specs = vec![
        FixtureBlock {
            number: SOURCE_BLOCK,
            proposals: vec![txs[1].proposal_short_id(), txs[3].proposal_short_id()],
            uncles: vec![],
            transactions: vec![],
        },
        FixtureBlock {
            number: COMMIT_BLOCK,
            proposals: vec![],
            uncles: vec![],
            transactions: txs.clone(),
        },
    ];
    let json = proposal_graph(&specs, SOURCE_BLOCK).await;

    assert_eq!(json["metadata"]["totalProposals"], 2);
    assert_eq!(json["metadata"]["committedCount"], 2);
    assert_eq!(proposal_nodes(&json).len(), 2);
}

// ---------------------------------------------------------------------------
// Commit-window resolution pin. This test passes BEFORE the resolution logic
// is extracted into the shared `resolve_committed_txs` helper (used by both
// /graph/proposals and /blocks/{id}/proposals) and MUST still pass after —
// proving the refactor preserves the exact matching semantics bit for bit:
//   - scan order: window blocks ascending (+2..=+10), transactions in-block
//     order, FIRST match wins (a later duplicate inclusion changes nothing);
//   - a missing window block is skipped, not an error (tip-adjacent windows);
//   - both window edges are exclusive fences: a matching tx at +1 or +11 does
//     not commit the proposal.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_proposal_graph_commit_window_semantics_pinned() {
    let txs = committed_transactions();
    let (tx_a, tx_b, tx_c, tx_d) = (&txs[1], &txs[2], &txs[3], &txs[4]);

    let specs = vec![
        FixtureBlock {
            number: 440,
            proposals: vec![
                tx_a.proposal_short_id(),
                tx_b.proposal_short_id(),
                tx_c.proposal_short_id(),
                tx_d.proposal_short_id(),
            ],
            uncles: vec![],
            transactions: vec![],
        },
        // +1: BEFORE the close edge (+2). tx_d's inclusion here must not count.
        FixtureBlock {
            number: 441,
            proposals: vec![],
            uncles: vec![],
            transactions: vec![tx_d.clone()],
        },
        // 442 is deliberately missing: the scan skips the gap.
        FixtureBlock {
            number: 443,
            proposals: vec![],
            uncles: vec![],
            transactions: vec![tx_a.clone(), tx_b.clone()],
        },
        // A duplicate inclusion of tx_a later in the window: first match wins.
        FixtureBlock {
            number: 449,
            proposals: vec![],
            uncles: vec![],
            transactions: vec![tx_a.clone()],
        },
        // +11: BEYOND the far edge (+10). tx_c stays uncommitted.
        FixtureBlock {
            number: 451,
            proposals: vec![],
            uncles: vec![],
            transactions: vec![tx_c.clone()],
        },
    ];
    let json = proposal_graph(&specs, 440).await;

    assert_eq!(json["metadata"]["totalProposals"], 4, "got {json}");
    assert_eq!(
        json["metadata"]["committedCount"], 2,
        "only tx_a and tx_b commit inside the +2..=+10 window, got {json}"
    );
    assert_eq!(
        json["metadata"]["commitmentWindow"]["earliestCommitBlock"],
        442
    );
    assert_eq!(
        json["metadata"]["commitmentWindow"]["latestCommitBlock"],
        450
    );

    let node_a = proposal_node(&json, &short_id_hex(tx_a));
    assert_eq!(node_a["data"]["txHash"], tx_hash_hex(tx_a));
    assert_eq!(
        node_a["data"]["commitBlock"], 443,
        "first match in scan order wins; the duplicate at 449 must not shift it"
    );
    assert_eq!(node_a["data"]["distance"], 3);

    let node_b = proposal_node(&json, &short_id_hex(tx_b));
    assert_eq!(node_b["data"]["txHash"], tx_hash_hex(tx_b));
    assert_eq!(node_b["data"]["commitBlock"], 443);

    // Uncommitted proposals contribute no proposal node at all.
    assert_eq!(proposal_nodes(&json).len(), 2);

    // Exactly one commit-block node (443), carrying both commitments.
    let commit_nodes: Vec<_> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["nodeType"] == "commit_block")
        .collect();
    assert_eq!(commit_nodes.len(), 1, "got {json}");
    assert_eq!(commit_nodes[0]["data"]["blockNumber"], 443);
    assert_eq!(commit_nodes[0]["data"]["committedCount"], 2);
}
