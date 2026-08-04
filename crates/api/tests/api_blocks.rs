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
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
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
    let expected_rate = 1234.0f64 * 1000.0 / 226.0;
    for field in ["avgFeeRate", "minFeeRate", "maxFeeRate"] {
        let got = json[field].as_f64().unwrap();
        assert!(
            (got - expected_rate).abs() < 1e-9,
            "{field} must divide by serialized_size_in_block: got {got}, expected {expected_rate}"
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
