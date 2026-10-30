use crate::common::*;

/// Node-backed block routes now require actual serialized evidence. These
/// fixtures retain each test's canonical hashes and economics while seeding
/// the node's independent read-only database with matching transactions.
fn attach_block_evidence(mut config: AppConfig, numbers: &[i64]) -> AppConfig {
    use ckb_types::{core, packed, prelude::*};
    let mut blocks = Vec::new();
    for &number in numbers {
        let mut header = config.store.get_block_header(number).unwrap().unwrap();
        if header.transactions_count == 0 {
            header.transactions_count = 1;
        }
        let lock = packed::Script::new_builder()
            .code_hash(packed::Byte32::new([0x99; 32]))
            .hash_type(1u8.into())
            .args(ckb_types::bytes::Bytes::from(vec![0x11; 20]).pack())
            .build();
        let witness = packed::CellbaseWitness::new_builder()
            .lock(lock)
            .message(ckb_types::bytes::Bytes::from_static(b"ckb 0.210.0").pack())
            .build();
        let cellbase = core::TransactionBuilder::default()
            .input(packed::CellInput::new_cellbase_input(number as u64))
            .output(packed::CellOutput::new_builder().build())
            .output_data(ckb_types::bytes::Bytes::new().pack())
            .witness(witness.as_bytes().pack())
            .build();
        let mut transactions = vec![cellbase];
        for index in 1..header.transactions_count {
            transactions.push(
                core::TransactionBuilder::default()
                    .header_dep(packed::Byte32::new([index as u8; 32]))
                    .build(),
            );
        }
        let mut batch = StoreBatch::new(&config.store);
        if number > 0 && config.store.get_block_header(number - 1).unwrap().is_none() {
            let mut parent = header.clone();
            parent.hash = header.parent_hash.clone();
            parent.timestamp -= 10_000;
            parent.chain_time_ms = parent.timestamp;
            batch.put_block_header(number - 1, &parent);
        }
        for (index, tx) in transactions.iter().enumerate() {
            batch.put_tx_index(
                number,
                index as i32,
                &TxIndexEntry {
                    tx_hash: tx.hash().unpack(),
                    is_cellbase: index == 0,
                    timestamp: header.timestamp,
                    inputs_count: tx.inputs().len() as i16,
                    outputs_count: tx.outputs().len() as i16,
                    fee: if index == 0 { 0 } else { 12345 },
                    tx_size: tx.data().as_slice().len() as i32,
                    cycles: if index == 0 { None } else { Some(1234567) },
                    semantic_tags: 0,
                },
            );
        }
        batch.put_block_header(number, &header);
        batch.commit().unwrap();
        let proposals = (0..header.proposals_count)
            .map(|i| packed::ProposalShortId::new([i as u8; 10]))
            .collect();
        let uncles = (0..header.uncles_count)
            .map(|i| {
                core::BlockBuilder::default()
                    .number((number as u64).pack())
                    .epoch(core::EpochNumberWithFraction::new(1, 0, 1800).pack())
                    .nonce((i as u128).pack())
                    .build()
                    .as_uncle()
            })
            .collect();
        let block = core::BlockBuilder::default()
            .set_proposals(proposals)
            .set_uncles(uncles)
            .number((number as u64).pack())
            .timestamp((header.timestamp as u64).pack())
            .epoch(
                core::EpochNumberWithFraction::new(
                    header.epoch_number as u64,
                    header.epoch_index as u64,
                    header.epoch_length as u64,
                )
                .pack(),
            )
            .set_transactions(transactions)
            .build()
            .fake_hash(packed::Byte32::from_slice(&header.hash).unwrap());
        blocks.push(block);
    }
    let chain = seed_ckb_chain(&blocks);
    config.ckb_db_path = chain.path;
    config.ckb_db_cleanup = Some(chain.cleanup);
    config
}

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
async fn block_list_requires_node_evidence_instead_of_zero_header_fields() {
    let store = test_store();
    seed_proposer_header(&store, 440, &[0x44; 32]);
    let app = create_router_without_warmup(test_config(store));
    let (status, json) = get_json(&app, "/blocks").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("ckb_db_path"),
        "{json}"
    );
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
    let config = attach_block_evidence(config, &[8_775_638]);
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

    let server = MockServer::start().await;
    mount_block_economic_state_not_finalized(&server).await;
    let mut config = test_config_with_append_only(core_store, append_only_store);
    config.ckb_rpc_url = server.uri();
    let config = attach_block_evidence(config, &[8_775_638, 8_775_639]);
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
async fn block_detail_requires_node_evidence_even_when_reward_is_indexed() {
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
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("ckb_db_path"),
        "{json}"
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
    let config = attach_block_evidence(config, &[500]);
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
    let config = attach_block_evidence(config, &[500]);
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
    let config = attach_block_evidence(config, &[500]);
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

#[tokio::test]
async fn block_list_and_detail_share_exact_evidence_and_reward_breakdown() {
    let store = test_store();
    seed_proposer_header(&store, 500, &[0x50; 32]);
    let mut header = store.get_block_header(500).unwrap().unwrap();
    header.transactions_count = 2;
    let mut batch = StoreBatch::new(&store);
    batch.put_block_header(500, &header);
    batch.commit().unwrap();
    let finalizing = build_finalizing_block(511);
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
    config.ckb_rpc_url = server.uri();
    let app = create_router(attach_block_evidence(config, &[500])).await;
    let (status, detail) = get_json(&app, "/blocks/500").await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let (status, page) = get_json(&app, "/blocks?cursor=501&limit=1").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["data"][0], detail);
    assert_eq!(detail["totalFees"], "12345");
    assert_eq!(detail["cycles"], "1234567");
    assert_eq!(detail["cyclesStatus"], "complete");
    assert_eq!(
        detail["size"].as_u64().unwrap(),
        detail["userTransactionsSize"].as_u64().unwrap() + detail["overheadSize"].as_u64().unwrap()
    );
    assert_eq!(detail["transactionSizes"].as_array().unwrap().len(), 2);
    assert_eq!(
        detail["userTransactionsSize"],
        detail["transactionSizes"][1]
    );
    assert!(detail["minerAddress"].as_str().unwrap().starts_with("ckb1"));
    let reward: u128 = ["primary", "secondary", "committed", "proposal"]
        .into_iter()
        .map(|field| {
            detail["rewardBreakdown"][field]
                .as_str()
                .unwrap()
                .parse::<u128>()
                .unwrap()
        })
        .sum();
    assert_eq!(detail["miningReward"], reward.to_string());
    assert_eq!(detail["rewardPaidAtBlock"], 511);
    assert_eq!(detail["rewardFinalizesAt"], 511);
    assert_eq!(detail["maxBlockBytes"], 597_000);
    assert_eq!(detail["maxBlockCycles"], "3500000000");
    assert_eq!(detail["minerMessageText"], "ckb 0.210.0");
    assert_eq!(detail["confirmations"], 12);
    assert_eq!(detail["parentTimestamp"], "2023-11-14T22:13:10+00:00");
    assert_eq!(detail["rawHeader"]["dao"], format!("0x{}", "00".repeat(32)));
}

#[tokio::test]
async fn block_cycles_do_not_present_partial_or_failed_execution_as_a_total() {
    for (marker, expected_status) in [(None, "pending"), (Some(-1), "failed")] {
        let store = test_store();
        seed_proposer_header(&store, 500, &[0x50; 32]);
        let mut batch = StoreBatch::new(&store);
        for (index, cycles) in [(0, Some(100)), (1, marker)] {
            batch.put_tx_index(
                500,
                index,
                &TxIndexEntry {
                    tx_hash: [index as u8; 32],
                    is_cellbase: false,
                    timestamp: 1_700_000_000_000,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: 100,
                    tx_size: 100,
                    cycles,
                    semantic_tags: 0,
                },
            );
        }
        batch.commit().unwrap();
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/blocks/500/fee-stats").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(
            json["totalCycles"].is_null(),
            "partial cycles are not a block total: {json}"
        );
        assert_eq!(json["cyclesStatus"], expected_status);
        assert_eq!(json["cyclesPending"], expected_status == "pending");
    }
}

#[tokio::test]
async fn block_response_rejects_index_counts_that_disagree_with_node_evidence() {
    let store = test_store();
    seed_proposer_header(&store, 500, &[0x50; 32]);
    let config = attach_block_evidence(test_config(store.clone()), &[500]);
    let mut header = store.get_block_header(500).unwrap().unwrap();
    header.proposals_count += 1;
    let mut batch = StoreBatch::new(&store);
    batch.put_block_header(500, &header);
    batch.commit().unwrap();
    let app = create_router(config).await;
    let (status, error) = get_json(&app, "/blocks/500").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("proposal count mismatch"),
        "{error}"
    );
}

#[tokio::test]
async fn r3_deep_fork_blocks_do_not_dispatch_cycles_above_fork_point() {
    for endpoint in ["", "/fee-stats"] {
        let store = test_store();
        seed_proposer_header(&store, 101, &[0x51; 32]);
        let mut header = store.get_block_header(101).unwrap().unwrap();
        header.transactions_count = 2;
        header.cycles = None;
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(101, &header);
        batch.commit().unwrap();
        let requests = tempfile::tempdir().unwrap();
        let mut config = attach_block_evidence(test_config(store.clone()), &[101]);
        let mut tx = store.get_tx_index(101, 1).unwrap().unwrap();
        tx.cycles = None;
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_index(101, 1, &tx);
        batch.commit().unwrap();
        seed_deep_fork_pause(&store, 100);
        config.cycles_request_dir = Some(requests.path().to_path_buf());
        // Hermetic node RPC: the reward lookup must not depend on a live
        // localhost:8114 node.
        let server = MockServer::start().await;
        mount_block_economic_state_not_finalized(&server).await;
        config.ckb_rpc_url = server.uri();
        let app = create_router(config).await;
        let (status, body) = get_json(&app, &format!("/blocks/101{endpoint}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(
            std::fs::read_dir(requests.path()).unwrap().count(),
            0,
            "{endpoint}"
        );
    }
}

// ---------------------------------------------------------------------------
// /blocks/{id}/uncles/{index}: an uncle's full header as its including block
// embeds it — uncles have no `/blocks/{hash}` page.
// ---------------------------------------------------------------------------

struct UncleFixture {
    app: axum::Router,
    including_hash: [u8; 32],
    uncle_hash: [u8; 32],
    uncle_parent: [u8; 32],
    canonical_439: [u8; 32],
    proposal: ckb_types::packed::ProposalShortId,
}

async fn uncle_fixture(seed_canonical_height: bool) -> UncleFixture {
    uncle_fixture_with_rival_parent(seed_canonical_height, [0u8; 32]).await
}

/// The uncle fixture with the canonical block at the uncle's height built on
/// `rival_parent` (the uncle's own parent is `0x37…`).
async fn uncle_fixture_with_rival_parent(
    seed_canonical_height: bool,
    rival_parent: [u8; 32],
) -> UncleFixture {
    use ckb_types::{core, packed, prelude::*};
    let mut dao = Vec::new();
    for word in [
        3_360_000_000_123_456_789u64,
        10_000_000_000_000_001,
        12_345,
        (1u64 << 60) + 7,
    ] {
        dao.extend(word.to_le_bytes());
    }
    let proposal = packed::ProposalShortId::new([0x6c; 10]);
    let uncle_parent = [0x37; 32];
    let uncle = core::BlockBuilder::default()
        .number(439u64.pack())
        .parent_hash(packed::Byte32::new(uncle_parent))
        .timestamp(1_700_000_000_500u64.pack())
        .epoch(core::EpochNumberWithFraction::new(1, 5, 1800).pack())
        .compact_target(0x1a08a97eu32.pack())
        .nonce(0x0123_4567_89ab_cdef_0011_2233_4455_6677u128.pack())
        .dao(packed::Byte32::from_slice(&dao).unwrap())
        .proposal(proposal.clone())
        .build()
        .as_uncle();
    let uncle_hash: [u8; 32] = uncle.hash().unpack();
    let including = core::BlockBuilder::default()
        .number(440u64.pack())
        .epoch(core::EpochNumberWithFraction::new(1, 6, 1800).pack())
        .uncle(uncle)
        .build();
    let including_hash: [u8; 32] = including.hash().unpack();
    let canonical_439 = [0x39; 32];
    let store = test_store();
    let header = |hash: &[u8], uncles_count: i32| CachedBlockHeader {
        hash: hash.to_vec(),
        parent_hash: rival_parent.to_vec(),
        timestamp: 1_700_000_000_000,
        epoch_number: 1,
        epoch_index: 0,
        epoch_length: 1800,
        dao: vec![0; 32],
        transactions_count: 0,
        uncles_count,
        proposals_count: 0,
        compact_target: 0,
        miner_lock_hash: None,
        cycles: None,
        chain_time_ms: 1_700_000_000_000,
    };
    let mut batch = StoreBatch::new(store.as_ref());
    if seed_canonical_height {
        batch.put_block_header(439, &header(&canonical_439, 0));
    }
    batch.put_block_header(440, &header(&including_hash, 1));
    batch.commit().unwrap();
    let chain = seed_ckb_chain(&[including]);
    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    UncleFixture {
        app: create_router(config).await,
        including_hash,
        uncle_hash,
        uncle_parent,
        canonical_439,
        proposal,
    }
}

#[tokio::test]
async fn block_uncle_detail_serves_the_embedded_header_and_its_canonical_rival() {
    use ckb_types::prelude::*;
    let fixture = uncle_fixture(true).await;
    let hex = |bytes: &[u8]| format!("0x{}", hex::encode(bytes));
    let (status, json) = get_json(&fixture.app, "/blocks/440/uncles/0").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["number"], 439);
    assert_eq!(json["hash"], hex(&fixture.uncle_hash));
    assert_eq!(json["parentHash"], hex(&fixture.uncle_parent));
    assert_eq!(
        json["timestamp"],
        chrono::DateTime::from_timestamp_millis(1_700_000_000_500)
            .unwrap()
            .to_rfc3339()
    );
    assert_eq!(json["epoch"], "5/1800");
    assert_eq!(json["epochNumber"], 1);
    assert_eq!(json["epochIndex"], 5);
    assert_eq!(json["epochLength"], 1800);
    assert_eq!(json["compactTarget"], "0x1a08a97e");
    assert_eq!(
        json["difficulty"],
        ckb_types::utilities::compact_to_difficulty(0x1a08a97e).to_string()
    );
    assert_eq!(json["nonce"], "0x123456789abcdef0011223344556677");
    assert_eq!(json["version"], 0);
    assert_eq!(
        json["dao"],
        serde_json::json!({
            "raw": json["dao"]["raw"].clone(),
            "c": "3360000000123456789",
            "ar": "10000000000000001",
            "s": "12345",
            "u": ((1u64 << 60) + 7).to_string(),
        })
    );
    assert_eq!(json["dao"]["raw"].as_str().unwrap().len(), 66);
    assert_eq!(
        json["proposalIds"],
        serde_json::json!([hex(fixture.proposal.as_slice())])
    );
    for field in ["transactionsRoot", "proposalsHash", "extraHash"] {
        assert_eq!(json[field].as_str().unwrap().len(), 66, "{field}: {json}");
    }
    assert_eq!(
        json["includedBy"],
        serde_json::json!({"number": 440, "hash": hex(&fixture.including_hash), "index": 0})
    );
    assert_eq!(
        json["canonicalAtHeight"],
        serde_json::json!({"number": 439, "hash": hex(&fixture.canonical_439)})
    );

    // The including block by hash resolves the same uncle.
    let (status, by_hash) = get_json(
        &fixture.app,
        &format!("/blocks/{}/uncles/0", hex(&fixture.including_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_hash}");
    assert_eq!(by_hash, json);
}

/// BA-43: an uncle page says which canonical block it competed with —
/// "same parent as #439" — and how long after that sibling it was mined.
#[tokio::test]
async fn block_uncle_detail_names_its_sibling_and_the_time_after_it() {
    let fixture = uncle_fixture_with_rival_parent(true, [0x37; 32]).await;
    let (status, json) = get_json(&fixture.app, "/blocks/440/uncles/0").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["sameParentAs"], 439, "{json}");
    assert_eq!(json["msAfterSibling"], 500, "{json}");

    // A canonical block at the height on another parent is no sibling.
    let fixture = uncle_fixture(true).await;
    let (status, json) = get_json(&fixture.app, "/blocks/440/uncles/0").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["sameParentAs"].is_null(), "{json}");
    assert!(json["msAfterSibling"].is_null(), "{json}");
}

#[tokio::test]
async fn block_uncle_detail_rejects_unknown_uncles_and_non_canonical_blocks() {
    let fixture = uncle_fixture(true).await;
    let (status, json) = get_json(&fixture.app, "/blocks/440/uncles/1").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("no uncle at index 1"),
        "{json}"
    );
    // An uncle is not a canonical block, and is not an including block either.
    let uncle = format!("0x{}", hex::encode(fixture.uncle_hash));
    let (status, json) = get_json(&fixture.app, &format!("/blocks/{uncle}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    let (status, json) = get_json(&fixture.app, &format!("/blocks/{uncle}/uncles/0")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    let (status, json) = get_json(&fixture.app, "/blocks/441/uncles/0").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    let (status, json) = get_json(&fixture.app, "/blocks/440/uncles/-1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}

#[tokio::test]
async fn block_uncle_detail_requires_the_canonical_block_at_its_height() {
    let fixture = uncle_fixture(false).await;
    let (status, json) = get_json(&fixture.app, "/blocks/440/uncles/0").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("block 440")
            && message.contains("uncle 0")
            && message.contains("no canonical block indexed at uncle height 439"),
        "{message}"
    );
}
