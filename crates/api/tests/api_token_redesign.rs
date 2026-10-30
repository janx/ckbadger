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
                metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
                chain_metadata: None,
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
    batch.put_token_transfers_count(&TOKEN, 4);
    batch.commit().unwrap();
    // The rows the indexer writes for these transfers: one transaction a day,
    // the mint on 10-01, the burn on 10-08 (`TokenDailyDelta` supply facts).
    for (date, row) in [
        (
            20261001,
            TokenDailyDelta {
                owned_capacity_delta: 1000,
                owned_knowledge_delta: 600,
                tx_count: 1,
                transfer_tx_count: 1,
                minted: 100u128.into(),
                mint_count: 1,
                live_cells_delta: 1,
                ..Default::default()
            },
        ),
        (
            20261002,
            TokenDailyDelta {
                tx_count: 1,
                transfer_tx_count: 1,
                ..Default::default()
            },
        ),
        (
            20261008,
            TokenDailyDelta {
                tx_count: 1,
                transfer_tx_count: 1,
                burned: 10u128.into(),
                burn_count: 1,
                ..Default::default()
            },
        ),
    ] {
        store.put_token_daily_delta(&TOKEN, date, &row).unwrap();
    }
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

/// IV-27 (I4): the history is the token's persisted days — supply from
/// `minted - burned`, weekly counts of transactions (not transfer legs) from
/// `tx_count`, mint / burn counts from the rows, the transfer count from the
/// persisted counter — over complete indexed calendar days. The supply-event
/// list is its own paged route.
#[tokio::test]
async fn token_history_reads_its_persisted_token_days() {
    let app = create_router(test_config(fixture())).await;
    let (status, body) =
        response_json(&app, &format!("/tokens/0x{}/history", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], "indexed_chain_time");
    assert_eq!(body["completeThrough"], "2026-10-11");
    assert_eq!(body["transfersCount"], 4);
    assert_eq!(body["mintCount"], 1);
    assert_eq!(body["burnCount"], 1);
    let daily = body["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 11);
    assert_eq!(daily[0]["supply"], "100");
    assert_eq!(daily[6]["supply"], "100", "10-07, before the burn");
    assert_eq!(daily[7]["supply"], "90", "10-08, the burn");
    assert_eq!(daily.last().unwrap()["capacity"], "1000");
    assert_eq!(daily.last().unwrap()["supply"], "90");
    assert_eq!(
        body["weeklyTransactions"],
        serde_json::json!([
            {"weekStart": "2026-09-28", "transactionsCount": 2},
            {"weekStart": "2026-10-05", "transactionsCount": 1},
        ]),
        "{body}"
    );
    assert!(
        body.get("weekly").is_none() && body.get("supplyEvents").is_none(),
        "{body}"
    );
}

/// The supply-event list (moved off `/history`): every mint and burn, newest
/// first, paged, with the exact supply after each, and `total` = the
/// persisted record count.
#[tokio::test]
async fn token_supply_events_page_newest_first_with_exact_supply_after() {
    let app = create_router(test_config(fixture())).await;
    let path = format!("/tokens/0x{}/supply-events", hex::encode(TOKEN));
    let (status, first) = response_json(&app, &format!("{path}?limit=1")).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["total"], 2, "{first}");
    assert_eq!(first["data"].as_array().unwrap().len(), 1);
    assert_eq!(first["data"][0]["kind"], "burn");
    assert_eq!(first["data"][0]["blockNumber"], 2);
    assert_eq!(first["data"][0]["amount"], "10");
    assert_eq!(first["data"][0]["supplyAfter"], "90");
    assert_eq!(first["nextCursor"], "2:0");
    let (status, second) = response_json(&app, &format!("{path}?limit=1&cursor=2:0")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["data"][0]["kind"], "mint");
    assert_eq!(second["data"][0]["blockNumber"], 0);
    assert_eq!(second["data"][0]["supplyAfter"], "100");
    assert_eq!(
        second["data"][0]["lockHash"],
        format!("0x{}", "32".repeat(32))
    );
    assert_eq!(second["nextCursor"], serde_json::Value::Null);
    let (_, all) = response_json(&app, &path).await;
    assert_eq!(all["data"].as_array().unwrap().len(), 2, "{all}");
}

/// A day row that disagrees with the day's own mint / burn records is a
/// write-path bug: a 500 naming the day.
#[tokio::test]
async fn token_supply_events_reject_a_day_row_that_disagrees_with_its_records() {
    let store = fixture();
    store
        .put_token_daily_delta(
            &TOKEN,
            20261008,
            &TokenDailyDelta {
                tx_count: 1,
                burned: 10u128.into(),
                burn_count: 2,
                ..Default::default()
            },
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = response_json(
        &app,
        &format!("/tokens/0x{}/supply-events", hex::encode(TOKEN)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("disagree with its row") && message.contains("date=2026-10-08"),
        "{body}"
    );
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

/// IV-27: `cellsCount` is the token's persisted live cell count — the running
/// sum of its daily rows' `live_cells_delta` — on the detail and on every
/// `/tokens` row alike. A day that would take the count below zero is a 500
/// naming the token and the day. The type index is not read per request:
/// comparing it with this count is `ckbadger verify`'s
/// `token_live_cells_consistency`, so a stale index row changes nothing here.
#[tokio::test]
async fn token_cells_count_is_the_persisted_daily_count() {
    let store = fixture();
    let mut batch = StoreBatch::new(&store);
    // A stale index row (no live marker, no payload): never read per request.
    batch.put_cell_by_type(&TOKEN, 2, &[0x91; 32], 0);
    batch.commit().unwrap();
    store
        .put_token_daily_delta(
            &TOKEN,
            20261012,
            &TokenDailyDelta {
                tx_count: 1,
                live_cells_delta: 2,
                ..Default::default()
            },
        )
        .unwrap();
    let app = create_router(test_config(store.clone())).await;
    let detail = format!("/tokens/0x{}", hex::encode(TOKEN));
    let (status, body) = response_json(&app, &detail).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cellsCount"], 3);
    let (status, list) = response_json(&app, "/tokens").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["data"][0]["cellsCount"], 3);

    store
        .put_token_daily_delta(
            &TOKEN,
            20261013,
            &TokenDailyDelta {
                tx_count: 1,
                live_cells_delta: -4,
                ..Default::default()
            },
        )
        .unwrap();
    for path in [detail.as_str(), "/tokens"] {
        let (status, body) = response_json(&app, path).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{path}: {body}");
        let message = body["message"].as_str().unwrap();
        assert!(
            message.contains(&format!("type_hash=0x{}", hex::encode(TOKEN)))
                && message.contains("date=20261013"),
            "{path}: {body}"
        );
    }
}

/// IV-27: the token detail and every `/tokens` row read the persisted count:
/// a bounded number of point reads and the token's daily rows, never the type
/// index, a live marker or a payload. Live (2026-10-30 full-chain store) the
/// per-request cell walk cost USDI's 785 cells 0.28–0.45 s, RGB++'s 19,632
/// cells 5.3 s, and `/tokens?limit=20` 18 s.
#[tokio::test]
async fn token_cells_count_reads_no_cells() {
    const CELLS: u16 = 1_500;
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
        udt_amount: Some(1),
        data_hash: None,
    };
    let mut batch = StoreBatch::new(&store);
    for i in 0..CELLS {
        let mut tx = [0x90u8; 32];
        tx[..2].copy_from_slice(&i.to_be_bytes());
        batch.put_cell(&tx, 0, &cell, 2);
        batch.put_cell_by_type(&TOKEN, 2, &tx, 0);
    }
    batch.commit().unwrap();
    // The fixture's first day holds one live cell; this day the other 1,499.
    store
        .put_token_daily_delta(
            &TOKEN,
            20261012,
            &TokenDailyDelta {
                tx_count: 1,
                live_cells_delta: i64::from(CELLS) - 1,
                ..Default::default()
            },
        )
        .unwrap();
    let app = create_router(test_config(store.clone())).await;

    for path in [
        format!("/tokens/0x{}", hex::encode(TOKEN)),
        "/tokens".into(),
    ] {
        store.reset_read_call_counters();
        let (status, body) = response_json(&app, &path).await;
        let (get_cf, multi_get) = store.read_call_counts();
        let cell_rows =
            ["cell_by_type", "live_cells", "cells"].map(|cf| (cf, store.iterated_rows(cf)));
        eprintln!("{path}: get_cf={get_cf} multi_get={multi_get} iterated={cell_rows:?}");
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let count = if path == "/tokens" {
            &body["data"][0]["cellsCount"]
        } else {
            &body["cellsCount"]
        };
        assert_eq!(count, CELLS, "{path}");
        assert_eq!(
            cell_rows,
            [("cell_by_type", 0), ("live_cells", 0), ("cells", 0)],
            "{path} walked cells"
        );
        assert_eq!(multi_get, 0, "{path}: cell payload batches");
        assert!(
            get_cf <= 20,
            "{path}: {get_cf} point reads for {CELLS} cells"
        );
        assert!(
            store.iterated_rows("stats_token") <= 10,
            "{path}: daily rows read"
        );
    }
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
    // The history's chain clock (the complete-day boundary search) is the
    // first reader of block 1's header; either reader names the block.
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("missing") && message.contains("header") && message.contains("block=1"),
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
    // The indexer dates a token day by chain time, so the burn's row is
    // 10-09's, not the raw timestamp's 10-08.
    let burn_day = store
        .get_token_daily_delta(&TOKEN, 20261008)
        .unwrap()
        .unwrap();
    store
        .delete_cf(
            store.cf_stats_token(),
            &ckbadger_store::keys::encode_token_daily_key(&TOKEN, 20261008),
        )
        .unwrap();
    store
        .put_token_daily_delta(&TOKEN, 20261009, &burn_day)
        .unwrap();
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
    // The mint's day carries supply but no capacity for the cells holding it.
    store
        .put_token_daily_delta(
            &TOKEN,
            20261001,
            &TokenDailyDelta {
                tx_count: 1,
                minted: 100u128.into(),
                mint_count: 1,
                ..Default::default()
            },
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
async fn token_history_accepts_same_day_mint_burn_with_zero_net_capacity() {
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
    // The day keeps its row although its capacity nets to zero (W3): two
    // transactions, the mint and the burn; no holders remain after the burn.
    store
        .put_token_daily_delta(
            &TOKEN,
            20261001,
            &TokenDailyDelta {
                tx_count: 2,
                minted: 100u128.into(),
                mint_count: 1,
                burned: 100u128.into(),
                burn_count: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) =
        response_json(&app, &format!("/tokens/0x{}/history", hex::encode(TOKEN))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["mintCount"].clone(), body["burnCount"].clone()),
        (1.into(), 1.into())
    );
    assert_eq!(
        body["daily"],
        serde_json::json!([{
            "date":"2026-10-01", "supply":"0", "capacity":"0", "occupiedCapacity":"0"
        }])
    );
    let (status, events) = response_json(
        &app,
        &format!("/tokens/0x{}/supply-events", hex::encode(TOKEN)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let after: Vec<_> = events["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap(),
                e["supplyAfter"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(after, [("burn", "0"), ("mint", "100")]);
}

/// A token whose history spans `blocks` (at most 40) blocks, four per UTC+8
/// day from 2026-10-01: block 1 mints 1,000 to `0x32…`, every later block
/// moves one unit to `0x33…`. The indexed chain is 48 blocks long whatever
/// `blocks` is, so two fixtures differ only in their history. The token's
/// daily rows are the ones the indexer writes for it (one transaction per
/// transfer block, the mint's supply and capacity on 10-01).
fn long_history_fixture(blocks: i64) -> Arc<CkbadgerStore> {
    assert!(blocks <= 40);
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
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: blocks,
                metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
                chain_metadata: None,
            },
        )
        .unwrap();
    let start = utc_ms("2026-09-30T16:00:00Z");
    let tip = 48;
    let headers: Vec<(i64, i64)> = (0..=tip)
        .map(|block| (block, start + block * 6 * 3_600_000))
        .collect();
    seed_block_headers_at(&store, &headers);
    let mut batch = StoreBatch::new(&store);
    let mut days: std::collections::BTreeMap<u32, TokenDailyDelta> = Default::default();
    for block in 1..=blocks {
        let hash = long_history_tx(block);
        let timestamp = store.get_block_header(block).unwrap().unwrap().timestamp;
        batch.put_tx_hash_map(&hash, block, 1);
        batch.put_tx_index(
            block,
            1,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp,
                inputs_count: 1,
                outputs_count: 1,
                fee: 10,
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
                tx_hash: hash.to_vec(),
                block_number: block,
                from_lock_hash: (block > 1).then(|| vec![0x32; 32]),
                to_lock_hash: vec![if block == 1 { 0x32 } else { 0x33 }; 32],
                amount: if block == 1 { 1_000 } else { 1 },
                is_mint: block == 1,
                is_burn: false,
                timestamp,
            },
        );
        let day = 20261001 + u32::try_from(block / 4).unwrap();
        let row = days.entry(day).or_default();
        row.tx_count += 1;
        if block == 1 {
            row.owned_capacity_delta = 1_000;
            row.owned_knowledge_delta = 600;
            row.minted = 1_000u128.into();
            row.mint_count = 1;
        }
    }
    let moved = u128::try_from(blocks - 1).unwrap();
    batch.put_token_holder(&TOKEN, &[0x32; 32], 1_000 - moved);
    batch.put_token_holder(&TOKEN, &[0x33; 32], moved);
    batch.put_token_transfers_count(&TOKEN, blocks);
    batch.commit().unwrap();
    for (day, row) in days {
        store.put_token_daily_delta(&TOKEN, day, &row).unwrap();
    }
    store
}

fn long_history_tx(block: i64) -> [u8; 32] {
    let mut hash = [0x5a; 32];
    hash[..8].copy_from_slice(&block.to_be_bytes());
    hash
}

/// Overwrite the token's transfer rows of blocks `from..=to` with bytes no
/// reader can decode: a route that reads any of them fails.
fn corrupt_transfer_rows(store: &CkbadgerStore, from: i64, to: i64) {
    for block in from..=to {
        store
            .put_cf(
                store.cf_token_transfers(),
                &ckbadger_store::keys::encode_token_transfer_key(&TOKEN, block, 0),
                b"not a transfer record",
            )
            .unwrap();
    }
}

/// IV-27 (I4): `/history` walked every transfer of the token and read block
/// headers for them (4–13 s for USDI, linear in history for Seal). It now
/// reads no transfer row at all — every row is corrupt here and the history
/// is still exact — and its point reads do not grow from 5 to 10 days of
/// history: no per-block header and no per-day read.
#[tokio::test]
async fn token_history_reads_no_transfer_and_no_per_block_header() {
    let path = format!("/tokens/0x{}/history", hex::encode(TOKEN));
    let mut reads = Vec::new();
    for blocks in [20, 40] {
        let store = long_history_fixture(blocks);
        corrupt_transfer_rows(&store, 1, blocks);
        let app = router_from_state(test_app_state(test_config(store.clone())));
        store.reset_read_call_counters();
        let (status, body) = response_json(&app, &path).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["transfersCount"], blocks, "{body}");
        assert_eq!(body["mintCount"], 1, "{body}");
        assert_eq!(body["daily"].as_array().unwrap().len(), 12, "10-01 … 10-12");
        assert_eq!(body["daily"][11]["supply"], "1000", "{body}");
        let weekly_total: u64 = body["weeklyTransactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["transactionsCount"].as_u64().unwrap())
            .sum();
        // Every transfer block is one transaction in a complete week.
        assert_eq!(weekly_total, u64::try_from(blocks).unwrap(), "{body}");
        reads.push((blocks, store.read_call_counts()));
    }
    assert_eq!(
        reads[0].1, reads[1].1,
        "history point reads grew with its days: {reads:?}"
    );
}

/// Only the tip day is "live": it is the indexer's running row for that day,
/// read like every other row. A transaction later on the tip day changes the
/// counts and the anchor, never a complete day.
#[tokio::test]
async fn token_history_tip_day_reads_its_running_row() {
    let store = long_history_fixture(40);
    let app = router_from_state(test_app_state(test_config(store.clone())));
    let path = format!("/tokens/0x{}/history", hex::encode(TOKEN));
    let (status, first) = response_json(&app, &path).await;
    assert_eq!(status, StatusCode::OK, "{first}");

    // What the indexer writes for one more transfer on the tip day.
    let tip = store.get_sync_tip_block().unwrap().unwrap();
    let block = tip.0 + 1;
    let mut header = tip.1.clone();
    header.timestamp += 60_000;
    header.chain_time_ms += 60_000;
    header.hash = vec![0xee; 32];
    let mut batch = StoreBatch::new(&store);
    batch.put_block_header(block, &header);
    batch.put_token_holder(&TOKEN, &[0x32; 32], 1_000 - 40);
    batch.put_token_holder(&TOKEN, &[0x33; 32], 40);
    batch.put_token_transfers_count(&TOKEN, 41);
    batch.commit().unwrap();
    store
        .update_sync_status(|status| status.tip_block_number = block)
        .unwrap();
    store
        .put_token_daily_delta(
            &TOKEN,
            20261013,
            &TokenDailyDelta {
                tx_count: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let (status, second) = response_json(&app, &path).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["transfersCount"], 41, "{second}");
    assert_eq!(second["anchor"]["blockNumber"], block);
    assert_eq!(second["completeThrough"], first["completeThrough"]);
    assert_eq!(second["daily"], first["daily"]);
    assert_eq!(second["weeklyTransactions"], first["weeklyTransactions"]);
}

/// The supply-event list reads transfers only inside the days whose row
/// records a mint or burn: every other day's transfer rows are corrupt here.
#[tokio::test]
async fn token_supply_events_read_transfers_only_on_mint_or_burn_days() {
    let store = long_history_fixture(40);
    corrupt_transfer_rows(&store, 4, 40);
    let app = router_from_state(test_app_state(test_config(store)));
    let (status, body) = response_json(
        &app,
        &format!("/tokens/0x{}/supply-events", hex::encode(TOKEN)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1, "{body}");
    assert_eq!(body["data"][0]["kind"], "mint");
    assert_eq!(body["data"][0]["blockNumber"], 1);
    assert_eq!(body["data"][0]["supplyAfter"], "1000");
}

/// IV-27: a Transactions-tab row summed its output capacity with one point
/// read per output. The row reads its outputs in one batch, so a page's point
/// reads do not grow with how many outputs its transactions have.
#[tokio::test]
async fn token_transactions_rows_read_outputs_in_one_batch() {
    let mut reads = Vec::new();
    for outputs in [2i16, 12] {
        let store = fixture();
        let mut batch = StoreBatch::new(&store);
        for block in 0..3 {
            let hash = [0x81 + block as u8; 32];
            let mut entry = store.get_tx_index(block, 2).unwrap().unwrap();
            entry.outputs_count = outputs;
            batch.put_tx_index(block, 2, &entry);
            for index in 0..outputs {
                batch.put_cell_payload_by_outpoint(
                    &hash,
                    index,
                    &LiveCellInfo {
                        capacity: 100,
                        occupied_capacity: 61,
                        lock_script_hash: vec![0x32; 32],
                        lock_code_hash: vec![0x33; 32],
                        lock_hash_type: 1,
                        lock_args: vec![],
                        type_script_hash: None,
                        type_code_hash: None,
                        type_hash_type: None,
                        type_args: None,
                        data_size: 0,
                        udt_amount: None,
                        data_hash: None,
                    },
                );
            }
        }
        batch.commit().unwrap();
        let app = router_from_state(test_app_state(test_config(store.clone())));
        store.reset_read_call_counters();
        let (status, body) = response_json(
            &app,
            &format!("/tokens/0x{}/transactions?limit=3", hex::encode(TOKEN)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["data"][0]["outputCapacity"],
            (100 * i64::from(outputs)).to_string(),
            "{body}"
        );
        reads.push((outputs, store.read_call_counts()));
    }
    assert_eq!(
        reads[0].1 .0, reads[1].1 .0,
        "get_cf point reads grew with outputs per transaction: {reads:?}"
    );
}

// ── I5: the token tabs page at the transfer index ───────────────────────

/// `n` transfer transactions of [`TOKEN`], one per block from block 1 at
/// tx index 1 (one leg each, the last block's tx has two legs), with the
/// daily rows the indexer writes for them (`transfer_tx_count` per day).
fn tab_fixture(n: i64) -> Arc<CkbadgerStore> {
    let store = test_store();
    let info = fixture().get_token(&TOKEN).unwrap().unwrap();
    store.put_token_direct(&TOKEN, &info).unwrap();
    let start = utc_ms("2026-09-30T16:00:00Z");
    let headers: Vec<(i64, i64)> = (0..=n).map(|b| (b, start + b * 60_000)).collect();
    seed_block_headers_at(&store, &headers);
    let mut batch = StoreBatch::new(&store);
    let mut days: std::collections::BTreeMap<u32, TokenDailyDelta> = Default::default();
    for block in 1..=n {
        let header = store.get_block_header(block).unwrap().unwrap();
        let hash = long_history_tx(block);
        batch.put_tx_hash_map(&hash, block, 1);
        batch.put_tx_index(
            block,
            1,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp: header.timestamp,
                inputs_count: 1,
                outputs_count: 0,
                fee: 10,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_tx_actions(&make_test_tx_actions(
            &[0x32; 32],
            &hash,
            &header.hash,
            block,
            1,
            0,
            0,
        ));
        for sequence in 0..if block == n { 2 } else { 1 } {
            batch.put_token_transfer(
                &TOKEN,
                block,
                sequence,
                &TokenTransferRecord {
                    tx_hash: hash.to_vec(),
                    block_number: block,
                    from_lock_hash: Some(vec![0x32; 32]),
                    to_lock_hash: vec![0x33; 32],
                    amount: 1,
                    is_mint: false,
                    is_burn: false,
                    timestamp: header.timestamp,
                },
            );
        }
        let day = ckbadger_store::keys::timestamp_ms_to_date(header.chain_time_ms);
        let row = days.entry(day).or_default();
        row.tx_count += 1;
        row.transfer_tx_count += 1;
    }
    batch.commit().unwrap();
    for (day, row) in days {
        store.put_token_daily_delta(&TOKEN, day, &row).unwrap();
    }
    store
}

/// I5 (the ID-COL-1 pattern for tokens): the Activities / Transactions tabs
/// built the token's whole transaction scope for every page (a full transfer
/// walk). A page now reads the transfer index at its cursor, so its reads do
/// not grow with the token's history, and `total` is the persisted count.
#[tokio::test]
async fn token_tab_page_reads_do_not_grow_with_history() {
    for route in ["activity-feed", "transactions"] {
        let mut reads = Vec::new();
        for n in [30, 2_100] {
            let store = tab_fixture(n);
            let app = router_from_state(test_app_state(test_config(store.clone())));
            store.reset_read_call_counters();
            let (status, page) = response_json(
                &app,
                &format!("/tokens/0x{}/{route}?limit=10", hex::encode(TOKEN)),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{page}");
            assert_eq!(page["total"], n, "{route}: total is the persisted count");
            assert_eq!(page["data"].as_array().unwrap().len(), 10);
            reads.push((n, store.read_call_counts()));
        }
        assert_eq!(
            reads[0].1, reads[1].1,
            "{route}: page-1 reads grew with history: {reads:?}"
        );
    }
}

/// Walking the cursor visits every transfer transaction once, newest first,
/// with no gap across pages (the two-leg transaction included once).
#[tokio::test]
async fn token_tab_cursor_walks_every_transaction_once() {
    let store = tab_fixture(30);
    let app = router_from_state(test_app_state(test_config(store)));
    for (route, key) in [("activity-feed", "txHash"), ("transactions", "hash")] {
        let mut uri = format!("/tokens/0x{}/{route}?limit=7", hex::encode(TOKEN));
        let mut seen = Vec::new();
        loop {
            let (status, page) = response_json(&app, &uri).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            assert_eq!(page["total"], 30, "{page}");
            for row in page["data"].as_array().unwrap() {
                seen.push(row[key].as_str().unwrap().to_string());
            }
            match page["nextCursor"].as_str() {
                Some(cursor) => {
                    uri = format!(
                        "/tokens/0x{}/{route}?limit=7&cursor={cursor}",
                        hex::encode(TOKEN)
                    )
                }
                None => break,
            }
        }
        let expected: Vec<String> = (1..=30)
            .rev()
            .map(|block| format!("0x{}", hex::encode(long_history_tx(block))))
            .collect();
        assert_eq!(seen, expected, "{route}");
    }
}
