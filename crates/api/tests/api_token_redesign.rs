use crate::common::*;
use ckbadger_store::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TokenTransferRecord};

const TOKEN: [u8; 32] = [0x71; 32];

async fn response_json(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1{path}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

fn fixture() -> Arc<CkbadgerStore> {
    let store = test_store();
    store
        .put_token_direct(
            &TOKEN,
            &TokenInfo {
                type_code_hash: vec![0x72; 32],
                hash_type: 1,
                type_args: vec![0x73; 32],
                standard: "sudt".into(),
                name: Some("Token".into()),
                symbol: Some("TOK".into()),
                decimals: Some(0),
                max_supply: None,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 4,
            },
        )
        .unwrap();
    seed_block_headers_at(
        &store,
        &[
            (0, utc_ms("2026-10-01T00:00:00Z")),
            (1, utc_ms("2026-10-02T00:00:00Z")),
            (2, utc_ms("2026-10-08T00:00:00Z")),
            (3, utc_ms("2026-10-12T00:00:00Z")),
        ],
    );
    let mut batch = StoreBatch::new(&store);
    for block in 0..3 {
        let hash = [0x81 + block as u8; 32];
        let timestamp = store.get_block_header(block).unwrap().unwrap().timestamp;
        batch.put_tx_hash_map(&hash, block, 2);
        batch.put_tx_index(
            block,
            2,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp,
                inputs_count: 1,
                outputs_count: 0,
                fee: 10,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_tx_actions(&TxActions {
            tx_hash: hash.to_vec(),
            block_hash: vec![block as u8; 32],
            block_number: block,
            tx_index: 2,
            timestamp,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![
                ParticipantDelta {
                    id: ParticipantId::Lock([0x31; 32]),
                    ckb_delta: -100,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: 0,
                    roles: 0,
                },
                ParticipantDelta {
                    id: ParticipantId::Lock([0x32; 32]),
                    ckb_delta: 90,
                    used_delta: 0,
                    item_deltas: vec![ItemDelta {
                        item_id: TOKEN.to_vec(),
                        kind: ItemKind::Token,
                        magnitude: 10,
                        negative: false,
                    }],
                    tags: 0,
                    roles: 0,
                },
            ],
        });
        for seq in 0..if block == 1 { 2 } else { 1 } {
            batch.put_token_transfer(
                &TOKEN,
                block,
                seq,
                &TokenTransferRecord {
                    tx_hash: hash.to_vec(),
                    block_number: block,
                    from_lock_hash: if block == 0 {
                        None
                    } else {
                        Some(vec![0x31; 32])
                    },
                    to_lock_hash: vec![0x32; 32],
                    amount: if block == 0 { 100 } else { 10 },
                    is_mint: block == 0,
                    is_burn: block == 2,
                    timestamp,
                },
            );
        }
    }
    batch.put_token_holder(&TOKEN, &[0x32; 32], 90);
    batch.put_token_holder_by_balance(&TOKEN, &[0x32; 32], 90);
    batch.commit().unwrap();
    store
        .put_token_daily_delta(
            &TOKEN,
            20261001,
            &TokenDailyDelta {
                owned_capacity_delta: 1000,
                owned_knowledge_delta: 600,
                tx_count: 1,
            },
        )
        .unwrap();
    store
}

#[tokio::test]
async fn asset_feeds_deduplicate_transactions_and_preserve_all_participants() {
    let app = create_router(test_config(fixture())).await;
    let path = format!("/tokens/0x{}", hex::encode(TOKEN));
    let (status, body) = response_json(&app, &format!("{path}/activity-feed?limit=2")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 3, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert_eq!(body["data"][1]["txIndex"], 2);
    assert_eq!(body["data"][1]["participants"].as_array().unwrap().len(), 2);
    let cursor = body["nextCursor"].as_str().unwrap();
    let (status, last) = response_json(
        &app,
        &format!("{path}/transactions?limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{last}");
    assert_eq!(last["data"].as_array().unwrap().len(), 1);
    assert_eq!(last["data"][0]["value"], "90");
    assert_eq!(last["data"][0]["cycles"], 0);
    assert_eq!(last["data"][0]["cyclesStatus"], "complete");
}

#[tokio::test]
async fn token_history_uses_all_transfers_and_complete_indexed_calendar_days() {
    let app = create_router(test_config(fixture())).await;
    let (status, body) =
        response_json(&app, &format!("/tokens/0x{}/history", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], "indexed_chain_time");
    assert_eq!(body["completeThrough"], "2026-10-11");
    assert_eq!(body["transfersCount"], 4);
    assert_eq!(body["mintCount"], 1);
    assert_eq!(body["burnCount"], 1);
    assert_eq!(body["supplyEvents"][1]["supplyAfter"], "90");
    let daily = body["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 11);
    assert_eq!(daily.last().unwrap()["capacity"], "1000");
    assert_eq!(daily.last().unwrap()["supply"], "90");
    assert_eq!(body["weekly"][0]["transfersCount"], 3);
    assert_eq!(body["weekly"][1]["transfersCount"], 1);
}

#[tokio::test]
async fn token_distribution_is_complete_not_a_top_holder_page() {
    let store = fixture();
    let mut batch = StoreBatch::new(&store);
    for i in 100..210u8 {
        batch.put_token_holder(&TOKEN, &[i; 32], 1);
        batch.put_token_holder_by_balance(&TOKEN, &[i; 32], 1);
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = response_json(
        &app,
        &format!("/tokens/0x{}/distribution", hex::encode(TOKEN)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Distribution includes the original 90-unit owner and all 110 additional owners.
    assert_eq!(body["holdersCount"], 111);
    assert_eq!(body["totalSupply"], "200");
    assert_eq!(body["nextNineBalance"], "9");
    assert_eq!(body["remainingBalance"], "101");
    assert_eq!(body["buckets"][1]["holdersCount"], 110);
}

#[tokio::test]
async fn token_live_cell_count_includes_zero_amount_and_rejects_stale_type_index() {
    let store = fixture();
    let cell = LiveCellInfo {
        capacity: 100,
        occupied_capacity: 80,
        lock_script_hash: vec![0x32; 32],
        lock_code_hash: vec![0x33; 32],
        lock_hash_type: 1,
        lock_args: vec![],
        type_script_hash: Some(TOKEN.to_vec()),
        type_code_hash: Some(vec![0x72; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x73; 32]),
        data_size: 16,
        udt_amount: Some(0),
        data_hash: None,
    };
    let mut batch = StoreBatch::new(&store);
    batch.put_cell(&[0x90; 32], 0, &cell, 2);
    batch.put_cell_by_type(&TOKEN, 2, &[0x90; 32], 0);
    batch.commit().unwrap();
    let app = create_router(test_config(store.clone())).await;
    let (status, body) = response_json(&app, &format!("/tokens/0x{}", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cellsCount"], 1);
    let (_, list) = response_json(&app, "/tokens").await;
    assert_eq!(list["data"][0]["cellsCount"], 1);
    let mut batch = StoreBatch::new(&store);
    batch.put_cell_by_type(&TOKEN, 2, &[0x91; 32], 0);
    batch.commit().unwrap();
    let (status, body) = response_json(&app, &format!("/tokens/0x{}", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("missing live marker"),
        "{body}"
    );
}

#[tokio::test]
async fn token_history_rejects_missing_headers_and_supply_disagreement() {
    let store = fixture();
    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&TOKEN, &[0x32; 32], 91);
    batch.commit().unwrap();
    let app = create_router(test_config(store.clone())).await;
    let path = format!("/tokens/0x{}/history", hex::encode(TOKEN));
    let (status, body) = response_json(&app, &path).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("disagrees with current holders"),
        "{body}"
    );
    store
        .delete_cf(
            store.cf_block_headers(),
            &ckbadger_store::keys::encode_block_num(1),
        )
        .unwrap();
    let (status, body) = response_json(&app, &path).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("missing header block=1"),
        "{body}"
    );
}

#[tokio::test]
async fn asset_feed_rejects_missing_actions_and_foreign_cursor() {
    let store = fixture();
    let app = create_router(test_config(store.clone())).await;
    let path = format!("/tokens/0x{}/activity-feed", hex::encode(TOKEN));
    let (status, body) = response_json(&app, &format!("{path}?cursor=2:0")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let key = ckbadger_store::keys::encode_tx_actions_key(2, 2, &[0x83; 32]);
    store.delete_cf(store.cf_tx_actions(), &key).unwrap();
    let (status, body) = response_json(&app, &path).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("missing actions block=2"),
        "{body}"
    );
}

#[tokio::test]
async fn token_history_dates_follow_chain_time_after_raw_timestamp_regression() {
    let store = fixture();
    let mut header = store.get_block_header(2).unwrap().unwrap();
    header.chain_time_ms = utc_ms("2026-10-09T00:00:00Z");
    let mut batch = StoreBatch::new(&store);
    batch.put_block_header(2, &header);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (_, body) = response_json(&app, &format!("/tokens/0x{}/history", hex::encode(TOKEN))).await;
    assert_eq!(body["daily"][7]["date"], "2026-10-08");
    assert_eq!(body["daily"][7]["supply"], "100");
    assert_eq!(body["daily"][8]["supply"], "90");
}

#[tokio::test]
async fn token_authority_and_same_symbol_are_evidence_based() {
    let store = fixture();
    let mut other = store.get_token(&TOKEN).unwrap().unwrap();
    other.standard = "xudt_compatible".into();
    store.put_token_direct(&[0x72; 32], &other).unwrap();
    let app = create_router(test_config(store)).await;
    let (_, known) = response_json(&app, &format!("/tokens/0x{}", hex::encode(TOKEN))).await;
    assert_eq!(
        known["issuer"]["lockHash"],
        format!("0x{}", hex::encode([0x73; 32]))
    );
    assert_eq!(
        known["sameSymbolTokens"][0]["typeScriptHash"],
        format!("0x{}", hex::encode([0x72; 32]))
    );
    let (_, custom) = response_json(&app, &format!("/tokens/0x{}", hex::encode([0x72; 32]))).await;
    assert_eq!(custom["issuer"], serde_json::Value::Null);
    assert_eq!(custom["maximumSupplyStatus"], "unknown");
}

#[tokio::test]
async fn real_token_router_producer_contract() {
    let store = fixture();
    let mut batch = StoreBatch::new(&store);
    batch.put_cell(
        &[0x83; 32],
        0,
        &LiveCellInfo {
            capacity: 1000,
            occupied_capacity: 600,
            lock_script_hash: vec![0x32; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(TOKEN.to_vec()),
            type_code_hash: Some(vec![0x72; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x73; 32]),
            data_size: 16,
            udt_amount: Some(90),
            data_hash: None,
        },
        2,
    );
    batch.put_cell_by_type(&TOKEN, 2, &[0x83; 32], 0);
    let mut tx = store.get_tx_index(2, 2).unwrap().unwrap();
    tx.outputs_count = 1;
    batch.put_tx_index(2, 2, &tx);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let mut artifact = serde_json::Map::new();
    for (name, suffix) in [
        ("token", ""),
        ("history", "/history"),
        ("distribution", "/distribution"),
        ("holders", "/holders"),
        ("activityFeed", "/activity-feed"),
        ("transactions", "/transactions"),
    ] {
        let (status, body) =
            response_json(&app, &format!("/tokens/0x{}{suffix}", hex::encode(TOKEN))).await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");
        artifact.insert(name.into(), body);
    }
    assert_eq!(artifact["token"]["cellsCount"], 1);
    assert_eq!(artifact["holders"]["totalSupply"], "90");
    assert_eq!(artifact["holders"]["anchor"]["blockNumber"], 3);
    assert_eq!(
        artifact["transactions"]["data"][0]["outputCapacity"],
        "1000"
    );
    if let Ok(directory) = std::env::var("CKBADGER_CONTRACT_OUTPUT") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("token-producer.json"),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn distribution_preserves_wide_balances_and_explicit_unknown_units() {
    let store = fixture();
    let mut info = store.get_token(&TOKEN).unwrap().unwrap();
    info.decimals = None;
    store.put_token_direct(&TOKEN, &info).unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&TOKEN, &[0xd1; 32], u128::MAX);
    batch.put_token_holder(&TOKEN, &[0xd2; 32], u128::MAX);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = response_json(
        &app,
        &format!("/tokens/0x{}/distribution", hex::encode(TOKEN)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["unit"], "raw");
    assert!(body["decimals"].is_null());
    assert_eq!(
        body["totalSupply"],
        "680564733841876926926749214863536423000"
    );
    assert_eq!(body["largestHolderBalance"], u128::MAX.to_string());
    assert_eq!(body["buckets"][6]["holdersCount"], 2);
    assert_eq!(
        body["buckets"][6]["balance"],
        "680564733841876926926749214863536422910"
    );
}

#[tokio::test]
async fn token_history_rejects_missing_capacity_evidence_for_existing_transfers() {
    let store = fixture();
    store
        .delete_cf(
            store.cf_stats_token(),
            &ckbadger_store::keys::encode_token_daily_key(&TOKEN, 20261001),
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) =
        response_json(&app, &format!("/tokens/0x{}/history", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body.to_string().contains("missing capacity history"));
    assert!(body.to_string().contains(&hex::encode(TOKEN)));
}

#[tokio::test]
async fn token_history_accepts_same_day_mint_burn_with_deleted_zero_net_capacity_row() {
    let info = fixture().get_token(&TOKEN).unwrap().unwrap();
    let store = test_store();
    store.put_token_direct(&TOKEN, &info).unwrap();
    let timestamp = utc_ms("2026-10-01T00:00:00Z");
    seed_block_headers_at(
        &store,
        &[
            (0, timestamp),
            (1, timestamp + 1),
            (2, utc_ms("2026-10-02T00:00:00Z")),
        ],
    );
    let mut batch = StoreBatch::new(&store);
    for block in 0..2 {
        let tx_hash = [0xe1 + block as u8; 32];
        batch.put_tx_hash_map(&tx_hash, block, 1);
        batch.put_tx_index(
            block,
            1,
            &TxIndexEntry {
                tx_hash,
                timestamp: timestamp + block,
                is_cellbase: false,
                inputs_count: 1,
                outputs_count: 0,
                fee: 0,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_token_transfer(
            &TOKEN,
            block,
            0,
            &TokenTransferRecord {
                tx_hash: tx_hash.to_vec(),
                block_number: block,
                from_lock_hash: if block == 0 {
                    None
                } else {
                    Some(vec![0xe3; 32])
                },
                to_lock_hash: vec![0xe3; 32],
                amount: 100,
                is_mint: block == 0,
                is_burn: block == 1,
                timestamp: timestamp + block,
            },
        );
    }
    batch.commit().unwrap();
    // The owning daily writer removes a date's row when its net capacity and
    // occupied deltas both become zero; no holders remain after this burn.
    assert!(store.list_token_daily_deltas(&TOKEN).unwrap().is_empty());
    let app = create_router(test_config(store)).await;
    let (status, body) =
        response_json(&app, &format!("/tokens/0x{}/history", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["supplyEvents"].as_array().unwrap().len(), 2);
    assert_eq!(
        body["daily"],
        serde_json::json!([{
            "date":"2026-10-01", "supply":"0", "capacity":"0", "occupiedCapacity":"0"
        }])
    );
}
