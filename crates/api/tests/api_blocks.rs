mod common;
use common::*;

#[tokio::test]
async fn test_blocks_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn block_fee_stats_preserve_rates_above_ieee_safe_integer() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        7,
        &CachedBlockHeader {
            hash: vec![0x77; 32],
            parent_hash: vec![0; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1e083126,
            miner_lock_hash: None,
            cycles: Some(100),
            chain_time_ms: 1_700_000_000_000,
        },
    );
    for (index, fee) in [(0, 9_007_199_254_740_993i64), (1, 1i64)] {
        batch.put_tx_index(
            7,
            index,
            &TxIndexEntry {
                tx_hash: [index as u8; 32],
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 1,
                fee,
                tx_size: 996,
                cycles: Some(50),
                semantic_tags: 0,
            },
        );
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/blocks/7/fee-stats").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["maxFeeRate"], "9007199254740993.00");
    assert_eq!(json["minFeeRate"], "1.00");
    assert_eq!(json["avgFeeRate"], "4503599627370497.00");
}

#[tokio::test]
async fn block_fee_stats_without_fee_sample_are_null() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        8,
        &CachedBlockHeader {
            hash: vec![0x88; 32],
            parent_hash: vec![0; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1e083126,
            miner_lock_hash: None,
            cycles: Some(0),
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_index(
        8,
        0,
        &TxIndexEntry {
            tx_hash: [0x88; 32],
            is_cellbase: true,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/blocks/8/fee-stats").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["avgFeeRate"].is_null());
    assert!(json["minFeeRate"].is_null());
    assert!(json["maxFeeRate"].is_null());
}

#[tokio::test]
async fn test_get_block_includes_hardfork_activation() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .put_epoch_stats(
            5414,
            &EpochStats {
                epoch_number: 5414,
                start_block: 8_775_638,
                end_block: None,
                blocks_count: 1800,
                length: 1800,
                start_timestamp: chrono::Utc::now(),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_block_header(
        8_775_638,
        &CachedBlockHeader {
            hash: vec![0x11; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 5414,
            epoch_index: 7,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();

    // The detail asks the node for the block's reward; this fixture block is
    // not finalized there.
    let server = MockServer::start().await;
    mount_block_economic_state_not_finalized(&server).await;
    let mut config = test_config_with_append_only(core_store, append_only_store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/blocks/8775638")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["number"], 8_775_638);
    assert_eq!(json["hardforkActivation"]["id"], "mirana-2021");
    assert_eq!(json["hardforkActivation"]["shortName"], "Mirana");
    assert_eq!(json["hardforkActivation"]["activationEpoch"], 5414);
    assert_eq!(
        json["hardforkActivation"]["resources"][0]["label"],
        "CKB2021"
    );
    assert_eq!(
        json["hardforkActivation"]["resources"][0]["url"],
        "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0037-ckb2021/0037-ckb2021.md"
    );
}

#[tokio::test]
async fn test_blocks_list_includes_hardfork_activation() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .put_epoch_stats(
            5414,
            &EpochStats {
                epoch_number: 5414,
                start_block: 8_775_638,
                end_block: None,
                blocks_count: 1800,
                length: 1800,
                start_timestamp: chrono::Utc::now(),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_block_header(
        8_775_639,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 5414,
            epoch_index: 8,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_010_000,
        },
    );
    batch.put_block_header(
        8_775_638,
        &CachedBlockHeader {
            hash: vec![0x11; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 5414,
            epoch_index: 7,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/blocks?limit=2")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().expect("block rows");
    assert_eq!(rows.len(), 2);

    let activation_row = rows
        .iter()
        .find(|row| row["number"].as_i64() == Some(8_775_638))
        .expect("activation block row");
    assert_eq!(activation_row["hardforkActivation"]["id"], "mirana-2021");
    assert_eq!(
        activation_row["hardforkActivation"]["shortName"],
        serde_json::Value::from("Mirana")
    );
    assert_eq!(
        activation_row["hardforkActivation"]["resources"][0]["label"],
        serde_json::Value::from("CKB2021")
    );

    let normal_row = rows
        .iter()
        .find(|row| row["number"].as_i64() == Some(8_775_639))
        .expect("non-activation block row");
    assert_eq!(normal_row["hardforkActivation"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_block_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks/999999999")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_block_fee_stats_uses_serialized_size_in_block_denominator() {
    let store = test_store();
    // Seeds block 321 with one non-cellbase tx: fee=1234, molecule size=222.
    insert_committed_transaction(&store, &[0xabu8; 32]);
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks/321/fee-stats")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["blockNumber"], 321);
    assert_eq!(json["transactionCount"], 1);
    // Sizes and fee rates are served on ONE convention: the serialized size in
    // block (molecule + 4), matching the node, explorer, and wallet. Reporting
    // molecule sizes here while dividing by molecule + 4 made `totalSize`
    // disagree with the summed per-tx `txSize` and made every client-side
    // `fee / size` disagree with the served fee rate.
    assert_eq!(json["totalSize"], 226);
    for field in ["avgFeeRate", "minFeeRate", "maxFeeRate"] {
        assert_eq!(
            json[field], "5460.18",
            "{field} must use serialized_size_in_block"
        );
    }
}

// ---------------------------------------------------------------------------
// Audited bug (2026-08-01 night, agent C extremes): /blocks/{id}/proposals
// hardcoded committedTxHash/committedBlockNumber to null while the sibling
// /graph/proposals/{block_number} endpoint resolved the very same commitments
// by scanning the NC-Max commit window (+2..+10). Same fact, two endpoints,
// one lied (mainnet block 11,988,763: 1,500 proposals, 1,410 committed, all
// reported null). Both endpoints must resolve through ONE shared helper.
// ---------------------------------------------------------------------------

/// Distinct fixture transactions (the distinguishing field must live in `raw`:
/// a tx hash covers only `raw`, so witness-only variation would collapse the
/// proposal short ids).
fn proposal_fixture_txs(count: u8) -> Vec<ckb_types::core::TransactionView> {
    (0..count)
        .map(|i| {
            ckb_types::core::TransactionBuilder::default()
                .header_dep(ckb_types::packed::Byte32::new([i; 32]))
                .build()
        })
        .collect()
}

fn build_proposal_block(
    number: u64,
    proposals: &[ckb_types::packed::ProposalShortId],
    transactions: &[ckb_types::core::TransactionView],
) -> ckb_types::core::BlockView {
    use ckb_types::prelude::*;
    let epoch = ckb_types::core::EpochNumberWithFraction::new(1, 0, 1800);
    let mut builder = ckb_types::core::BlockBuilder::default()
        .number(number.pack())
        .epoch(epoch.pack());
    for id in proposals {
        builder = builder.proposal(id.clone());
    }
    for tx in transactions {
        builder = builder.transaction(tx.clone());
    }
    builder.build()
}

#[tokio::test]
async fn test_block_proposals_resolve_commitments_via_shared_window() {
    use ckb_types::prelude::*;

    let txs = proposal_fixture_txs(2);
    let (tx_a, tx_b) = (&txs[0], &txs[1]);
    // A short id no transaction in the window matches: honest null (the window
    // beyond the seeded blocks simply does not exist yet).
    let unknown_id = ckb_types::packed::ProposalShortId::new([0x5A; 10]);

    let proposer = build_proposal_block(
        440,
        &[
            tx_a.proposal_short_id(),
            tx_b.proposal_short_id(),
            unknown_id,
        ],
        &[],
    );
    // tx_a commits at the window's close edge (+2), tx_b mid-window (+5).
    let commit_close = build_proposal_block(442, &[], std::slice::from_ref(tx_a));
    let commit_mid = build_proposal_block(445, &[], std::slice::from_ref(tx_b));

    let store = test_store();
    let proposer_hash: [u8; 32] = proposer.hash().unpack();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        440,
        &CachedBlockHeader {
            hash: proposer_hash.to_vec(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 0,
            uncles_count: 0,
            proposals_count: 3,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();

    let chain = seed_ckb_chain(&[proposer, commit_close, commit_mid]);
    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(&app, "/blocks/440/proposals").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let proposals = json.as_array().expect("proposals array");
    assert_eq!(proposals.len(), 3);

    let tx_a_hash: [u8; 32] = tx_a.hash().unpack();
    let tx_b_hash: [u8; 32] = tx_b.hash().unpack();

    assert_eq!(proposals[0]["proposalIndex"], 0);
    assert_eq!(
        proposals[0]["proposalId"],
        format!("0x{}", hex::encode(tx_a.proposal_short_id().as_slice()))
    );
    assert_eq!(
        proposals[0]["committedTxHash"],
        format!("0x{}", hex::encode(tx_a_hash)),
        "the commit-window resolution the graph endpoint already performs must populate this field, got {json}"
    );
    assert_eq!(proposals[0]["committedBlockNumber"], 442);

    assert_eq!(proposals[1]["proposalIndex"], 1);
    assert_eq!(
        proposals[1]["committedTxHash"],
        format!("0x{}", hex::encode(tx_b_hash))
    );
    assert_eq!(proposals[1]["committedBlockNumber"], 445);

    assert_eq!(proposals[2]["proposalIndex"], 2);
    assert_eq!(
        proposals[2]["proposalId"],
        format!("0x{}", hex::encode([0x5A; 10]))
    );
    assert_eq!(
        proposals[2]["committedTxHash"],
        serde_json::Value::Null,
        "a proposal with no committing tx within the available window stays null — honest data, not a fallback"
    );
    assert_eq!(
        proposals[2]["committedBlockNumber"],
        serde_json::Value::Null
    );
}

// ---------------------------------------------------------------------------
// /blocks/{id}/proposals must serve the block's WHOLE proposal zone — its own
// proposals plus those of the uncles it embeds, exactly what CKB consensus
// (`BlockView::union_proposal_ids`) and `/graph/proposals` count — and must
// fail fast instead of answering `[]` when it cannot read the zone.
// ---------------------------------------------------------------------------

fn build_block_with_uncle(
    number: u64,
    proposals: &[ckb_types::packed::ProposalShortId],
    uncle_number: u64,
    uncle_proposals: &[ckb_types::packed::ProposalShortId],
) -> ckb_types::core::BlockView {
    use ckb_types::prelude::*;
    let epoch = ckb_types::core::EpochNumberWithFraction::new(1, 0, 1800);
    let mut uncle = ckb_types::core::BlockBuilder::default()
        .number(uncle_number.pack())
        .epoch(epoch.pack());
    for id in uncle_proposals {
        uncle = uncle.proposal(id.clone());
    }
    let mut builder = ckb_types::core::BlockBuilder::default()
        .number(number.pack())
        .epoch(epoch.pack())
        .uncle(uncle.build().as_uncle());
    for id in proposals {
        builder = builder.proposal(id.clone());
    }
    builder.build()
}

fn seed_proposer_header(store: &Arc<CkbadgerStore>, number: i64, hash: &[u8]) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        number,
        &CachedBlockHeader {
            hash: hash.to_vec(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 0,
            uncles_count: 1,
            proposals_count: 1,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();
}

#[tokio::test]
async fn test_block_proposals_include_uncle_proposals() {
    use ckb_types::prelude::*;

    let txs = proposal_fixture_txs(2);
    let (tx_own, tx_uncle) = (&txs[0], &txs[1]);
    let uncle_unknown = ckb_types::packed::ProposalShortId::new([0x6B; 10]);

    let proposer = build_block_with_uncle(
        440,
        &[tx_own.proposal_short_id()],
        439,
        &[tx_uncle.proposal_short_id(), uncle_unknown],
    );
    let uncle_hash: [u8; 32] = proposer.uncles().get(0).unwrap().hash().unpack();
    let commit = build_proposal_block(443, &[], &[tx_own.clone(), tx_uncle.clone()]);

    let store = test_store();
    let proposer_hash: [u8; 32] = proposer.hash().unpack();
    seed_proposer_header(&store, 440, &proposer_hash);

    let chain = seed_ckb_chain(&[proposer, commit]);
    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(&app, "/blocks/440/proposals").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let proposals = json.as_array().expect("proposals array");
    assert_eq!(
        proposals.len(),
        3,
        "the zone is the block's own proposal plus the uncle's two, got {json}"
    );

    assert_eq!(proposals[0]["proposalIndex"], 0);
    assert_eq!(proposals[0]["uncle"], serde_json::Value::Null);
    assert_eq!(proposals[0]["committedBlockNumber"], 443);

    let uncle =
        serde_json::json!({ "number": 439, "hash": format!("0x{}", hex::encode(uncle_hash)) });
    assert_eq!(proposals[1]["proposalIndex"], 1);
    assert_eq!(
        proposals[1]["proposalId"],
        format!("0x{}", hex::encode(tx_uncle.proposal_short_id().as_slice()))
    );
    assert_eq!(proposals[1]["uncle"], uncle);
    let tx_uncle_hash: [u8; 32] = tx_uncle.hash().unpack();
    assert_eq!(
        proposals[1]["committedTxHash"],
        format!("0x{}", hex::encode(tx_uncle_hash))
    );
    assert_eq!(proposals[1]["committedBlockNumber"], 443);

    assert_eq!(proposals[2]["proposalIndex"], 2);
    assert_eq!(proposals[2]["uncle"], uncle);
    assert_eq!(proposals[2]["committedTxHash"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_block_proposals_fail_without_chain_reader() {
    let store = test_store();
    seed_proposer_header(&store, 440, &[0x44; 32]);
    // `test_app_state` opens no CKB node reader.
    let app = create_router_without_warmup(test_config(store));

    let (status, json) = get_json(&app, "/blocks/440/proposals").await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "without the node DB the proposals are unknown, not empty, got {json}"
    );
    assert!(
        json["message"].as_str().unwrap().contains("ckb_db_path"),
        "{json}"
    );
}

#[tokio::test]
async fn test_block_proposals_fail_when_node_lacks_block() {
    let store = test_store();
    seed_proposer_header(&store, 440, &[0x44; 32]);
    // An (empty) node DB is opened, but it does not hold block 440.
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/blocks/440/proposals").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("440"), "{message}");
    assert!(
        message.contains(&format!("0x{}", hex::encode([0x44u8; 32]))),
        "{message}"
    );
}

#[tokio::test]
async fn test_block_proposals_unknown_block_is_not_found() {
    let app = create_router(test_config(test_store())).await;
    let (status, json) = get_json(&app, "/blocks/440/proposals").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "got {json}");
}

// ---------------------------------------------------------------------------
// The block detail's mining reward used to swallow every failure into
// `miningRewardTxHash: null`: a malformed `finalized_at`
// (`hex::decode(..).ok()?`), a wrong-length one (`return None`), no node
// reader (`ckb_store?`). A malformed node answer is an error. A valid finalizing
// block not yet indexed is pending; once indexed its cellbase is resolved solely
// through the domain index, independent of the optional node reader.
// ---------------------------------------------------------------------------

/// A block whose cellbase (index 0) pays an earlier block's reward.
fn build_finalizing_block(number: u64) -> ckb_types::core::BlockView {
    use ckb_types::prelude::*;
    let cellbase = ckb_types::core::TransactionBuilder::default()
        .input(ckb_types::packed::CellInput::new_cellbase_input(number))
        .output(ckb_types::packed::CellOutput::new_builder().build())
        .output_data(ckb_types::bytes::Bytes::new().pack())
        .build();
    build_proposal_block(number, &[], &[cellbase])
}

fn seed_finalizing_index(store: &Arc<CkbadgerStore>, block: &ckb_types::core::BlockView) {
    use ckb_types::prelude::*;
    let hash: [u8; 32] = block.hash().unpack();
    seed_proposer_header(store, block.number() as i64, &hash);
    let mut batch = ckbadger_store::StoreBatch::new(store);
    batch.put_tx_index(
        block.number() as i64,
        0,
        &ckbadger_store::TxIndexEntry {
            tx_hash: block.transactions()[0].hash().unpack(),
            is_cellbase: true,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
}

#[tokio::test]
async fn mining_reward_uses_indexed_cellbase_without_optional_node_reader() {
    let finalizing = build_finalizing_block(511);
    let store = test_store();
    seed_proposer_header(&store, 500, &[0x50; 32]);
    seed_finalizing_index(&store, &finalizing);
    let server = MockServer::start().await;
    mount_block_economic_state_rpc(
        &server,
        &[(
            &format!("0x{}", hex::encode([0x50; 32])),
            &format!("0x{}", hex::encode(finalizing.hash().raw_data())),
        )],
    )
    .await;
    let mut config = test_config(store);
    config.ckb_db_path = "/nonexistent/ckbadger-review-node-db".into();
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/blocks/500").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["miningRewardTxHash"],
        format!(
            "0x{}",
            hex::encode(finalizing.transactions()[0].hash().raw_data())
        )
    );
}

#[tokio::test]
async fn test_mining_reward_names_the_finalizing_cellbase() {
    use ckb_types::prelude::*;

    let finalizing = build_finalizing_block(511);
    let finalizing_hash: [u8; 32] = finalizing.hash().unpack();
    let cellbase_hash: [u8; 32] = finalizing.transactions()[0].hash().unpack();

    let store = test_store();
    seed_proposer_header(&store, 500, &[0x50; 32]);
    seed_finalizing_index(&store, &finalizing);
    let chain = seed_ckb_chain(&[finalizing]);
    let server = MockServer::start().await;
    mount_block_economic_state_rpc(
        &server,
        &[(
            &format!("0x{}", hex::encode([0x50u8; 32])),
            &format!("0x{}", hex::encode(finalizing_hash)),
        )],
    )
    .await;
    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, "/blocks/500").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    // primary + secondary + committed + proposal of the fixture reward.
    assert_eq!(
        json["miningReward"],
        BLOCK_ECONOMIC_STATE_REWARD_TOTAL.to_string()
    );
    assert_eq!(
        json["miningRewardTxHash"],
        format!("0x{}", hex::encode(cellbase_hash))
    );
}

#[tokio::test]
async fn test_mining_reward_bad_finalized_at_is_error() {
    let store = test_store();
    seed_proposer_header(&store, 500, &[0x50; 32]);
    let server = MockServer::start().await;
    // A 2-byte `finalized_at` names no block.
    mount_block_economic_state_rpc(
        &server,
        &[(&format!("0x{}", hex::encode([0x50u8; 32])), "0x1234")],
    )
    .await;
    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, "/blocks/500").await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "an unusable node answer must not become `miningRewardTxHash: null`, got {json}"
    );
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("finalized_at"), "{message}");
    assert!(message.contains("0x1234"), "{message}");
}

#[tokio::test]
async fn test_mining_reward_waits_until_finalizing_block_is_indexed() {
    let store = test_store();
    seed_proposer_header(&store, 500, &[0x50; 32]);
    let server = MockServer::start().await;
    let finalizing = build_finalizing_block(511);
    let finalized_at = format!("0x{}", hex::encode(finalizing.hash().raw_data()));
    mount_block_economic_state_rpc(
        &server,
        &[(&format!("0x{}", hex::encode([0x50u8; 32])), &finalized_at)],
    )
    .await;
    // Neither the node reader nor the domain index has reached the paying block.
    let mut config = test_config(store.clone());
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, "/blocks/500").await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert!(json["miningReward"].is_null());
    assert!(json["miningRewardTxHash"].is_null());
    // Pending answers must not be cached across indexer progress.
    seed_finalizing_index(&store, &finalizing);
    let (status, indexed) = get_json(&app, "/blocks/500").await;
    assert_eq!(status, StatusCode::OK, "{indexed}");
    assert_eq!(
        indexed["miningRewardTxHash"],
        format!(
            "0x{}",
            hex::encode(finalizing.transactions()[0].hash().raw_data())
        )
    );
}
