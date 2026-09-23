//! `POST /api/v1/verify/entity-statistics` — the bounded, read-only export the
//! verifier compares chain-derived expectations against.
//!
//! Everything it returns must come from one read pin, as raw stored values, with
//! the sync state that makes them meaningful. It never computes a new user
//! statistic, never calls the node, and never writes.

mod common;

use common::*;

const ROUTE: &str = "/api/v1/verify/entity-statistics";

fn token_hash(byte: u8) -> Vec<u8> {
    vec![byte; 32]
}

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Seed a synced tip so the export has an anchor to pin itself to.
fn seed_anchor(store: &Arc<CkbadgerStore>, tip: i64, hash: &[u8]) {
    store
        .set_sync_status(&ckbadger_store::types::SyncStatus {
            tip_block_number: tip,
            tip_block_hash: hash.to_vec(),
            total_transactions: 0,
            total_cells_created: 0,
            total_cells_consumed: 0,
            last_synced_at: 0,
            sync_started_at: None,
            sync_started_block: 0,
            sync_ema_rate: None,
            bulk_sync_completed_at: None,
            bulk_sync_completed_block: None,
            deep_fork_detected: false,
            deep_fork_info: None,
        })
        .unwrap();
}

fn seed_token(store: &Arc<CkbadgerStore>, type_hash: &[u8], rows: &[(u32, i128, i128)]) {
    let mut batch = StoreBatch::new(store);
    batch.put_token(
        type_hash,
        &TokenInfo {
            type_code_hash: vec![0xa1; 32],
            hash_type: 1,
            type_args: vec![0x01],
            standard: "xudt".to_string(),
            name: Some("Fixture".to_string()),
            symbol: Some("FIX".to_string()),
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        },
    );
    batch.commit().unwrap();
    for (date, capacity, knowledge) in rows {
        store
            .put_token_daily_delta(
                type_hash,
                *date,
                &TokenDailyDelta {
                    owned_capacity_delta: *capacity,
                    owned_knowledge_delta: *knowledge,
                },
            )
            .unwrap();
    }
}

async fn post(app: axum::Router, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(ROUTE)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "response body must be JSON ({e}): {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, json)
}

#[tokio::test]
async fn exports_raw_daily_deltas_as_exact_decimal_strings() {
    let store = test_store();
    let hash = token_hash(0xaa);
    seed_anchor(&store, 1_234, &token_hash(0x01));
    // i128 values well past f64's exact-integer range: a float round-trip would
    // change the last digits, which is exactly what this export must not do.
    seed_token(
        &store,
        &hash,
        &[
            (20260912, 232_171_655_021_955, -14_400_000_000),
            (
                20260913,
                -1,
                170_141_183_460_469_231_731_687_303_715_884_105_i128,
            ),
        ],
    );

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&hash) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["anchor"]["blockNumber"], 1_234);
    assert_eq!(body["anchor"]["blockHash"], hex0x(&token_hash(0x01)));
    assert_eq!(body["complete"], true);

    let entity = &body["entities"][0];
    assert_eq!(entity["kind"], "token");
    assert_eq!(entity["id"], hex0x(&hash));
    assert_eq!(entity["present"], true);
    assert_eq!(entity["rowCount"], 2);
    assert_eq!(entity["complete"], true);

    let daily = entity["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 2);
    assert_eq!(daily[0]["date"], 20260912);
    assert_eq!(daily[0]["capacityDelta"], "232171655021955");
    assert_eq!(daily[0]["knowledgeDelta"], "-14400000000");
    assert_eq!(daily[1]["date"], 20260913);
    assert_eq!(daily[1]["capacityDelta"], "-1");
    assert_eq!(
        daily[1]["knowledgeDelta"],
        "170141183460469231731687303715884105"
    );
}

#[tokio::test]
async fn an_entity_with_no_index_row_reports_present_false() {
    let store = test_store();
    seed_anchor(&store, 10, &token_hash(0x02));

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&token_hash(0xbb)) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let entity = &body["entities"][0];
    assert_eq!(entity["present"], false);
    assert_eq!(entity["rowCount"], 0);
    assert_eq!(
        entity["complete"], true,
        "an absent entity is fully exported: the verifier decides whether absence is a failure"
    );
    assert!(entity["daily"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_row_budget_below_the_stored_rows_reports_incomplete_instead_of_truncating_silently() {
    let store = test_store();
    let hash = token_hash(0xcc);
    seed_anchor(&store, 10, &token_hash(0x03));
    seed_token(
        &store,
        &hash,
        &[(20260101, 1, 1), (20260102, 2, 2), (20260103, 3, 3)],
    );

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({
            "entities": [{ "kind": "token", "id": hex0x(&hash) }],
            "maxDailyRows": 2
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let entity = &body["entities"][0];
    assert_eq!(entity["rowCount"], 3, "rowCount is what the store holds");
    assert_eq!(entity["daily"].as_array().unwrap().len(), 2);
    assert_eq!(entity["complete"], false);
    assert_eq!(
        body["complete"], false,
        "a truncated entity makes the whole export incomplete"
    );
}

/// A fresh router over a fresh anchored store, so each limit case is isolated.
fn anchored_router(anchor_byte: u8) -> axum::Router {
    let store = test_store();
    seed_anchor(&store, 10, &token_hash(anchor_byte));
    create_router_without_warmup(test_config(store))
}

#[tokio::test]
async fn a_request_over_the_server_limits_is_rejected() {
    let too_many: Vec<serde_json::Value> = (0..17u8)
        .map(|i| serde_json::json!({ "kind": "token", "id": hex0x(&token_hash(i)) }))
        .collect();
    let (status, body) = post(
        anchored_router(0x04),
        serde_json::json!({ "entities": too_many }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("16"), "{body:?}");

    let (status, body) = post(
        anchored_router(0x04),
        serde_json::json!({
            "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }],
            "maxDailyRows": 8193
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["message"].as_str().unwrap().contains("8192"),
        "{body:?}"
    );

    let (status, _) = post(anchored_router(0x04), serde_json::json!({ "entities": [] })).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an empty selection verifies nothing"
    );

    let (status, body) = post(
        anchored_router(0x04),
        serde_json::json!({ "entities": [{ "kind": "spore", "id": hex0x(&token_hash(1)) }] }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an unsupported family must be refused, never answered with an empty export"
    );
    assert!(
        body["message"].as_str().unwrap().contains("token"),
        "{body:?}"
    );
}

#[tokio::test]
async fn an_expected_anchor_that_no_longer_matches_is_rejected_with_the_actual_anchor() {
    let store = test_store();
    seed_anchor(&store, 500, &token_hash(0x05));

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({
            "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }],
            "expectedAnchor": { "blockNumber": 500, "blockHash": hex0x(&token_hash(0x99)) }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = body["message"].as_str().unwrap();
    assert!(message.contains(&hex0x(&token_hash(0x05))), "{message}");
    assert!(message.contains("500"), "{message}");
}

#[tokio::test]
async fn a_matching_expected_anchor_is_accepted() {
    let store = test_store();
    seed_anchor(&store, 500, &token_hash(0x05));

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({
            "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }],
            "expectedAnchor": { "blockNumber": 500, "blockHash": hex0x(&token_hash(0x05)) }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["anchor"]["blockNumber"], 500);
}

#[tokio::test]
async fn a_rollback_cleanup_in_progress_withholds_the_numbers() {
    let store = test_store();
    let hash = token_hash(0xdd);
    seed_anchor(&store, 42, &token_hash(0x06));
    seed_token(&store, &hash, &[(20260101, 7, 7)]);
    store.set_rollback_cleanup_in_progress(true).unwrap();

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&hash) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["rollbackCleanupInProgress"], true);
    assert_eq!(body["complete"], false);
    let entity = &body["entities"][0];
    assert_eq!(entity["complete"], false);
    assert!(
        entity["daily"].as_array().unwrap().is_empty(),
        "a mid-rollback view must not be published as raw statistics"
    );
}

#[tokio::test]
async fn an_unfinished_bulk_session_withholds_the_numbers() {
    let store = test_store();
    let hash = token_hash(0xde);
    seed_anchor(&store, 42, &token_hash(0x07));
    seed_token(&store, &hash, &[(20260101, 7, 7)]);
    store
        .set_bulk_build_session_marker(Some(&ckbadger_store::types::BulkBuildSessionMarker {
            run_id: "run".to_string(),
            started_at: 1,
            start_block: 0,
        }))
        .unwrap();

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&hash) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["bulkSessionInProgress"], true);
    assert_eq!(body["complete"], false);
    assert!(body["entities"][0]["daily"].as_array().unwrap().is_empty());
}

/// Until Phase 2 writes the coverage contract and the hourly retention
/// boundary, the export must say it has no evidence rather than imply one.
#[tokio::test]
async fn phase2_state_fields_report_absence_of_evidence() {
    let store = test_store();
    seed_anchor(&store, 42, &token_hash(0x08));

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body["state"]["entityStatsUndoContract"].is_null(),
        "no contract is written yet; null is the honest answer"
    );
    assert_eq!(body["state"]["hourlyRetention"], "unknown");
    assert_eq!(body["state"]["liveCellSummaryInitialized"], false);
    assert_eq!(body["state"]["deepForkDetected"], false);
}

/// The endpoint reads; it must leave the store byte-identical.
#[tokio::test]
async fn the_export_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    let hash = token_hash(0xef);
    seed_anchor(&store, 99, &token_hash(0x09));
    seed_token(&store, &hash, &[(20260101, 5, 5), (20260102, -5, -5)]);
    store.flush_all_memtables().unwrap();

    let files_before = store_files(dir.path());
    let rows_before = store.list_token_daily_deltas(&hash).unwrap();

    let app = create_router_without_warmup(test_config(store.clone()));
    let (status, _) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&hash) }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        store_files(dir.path()),
        files_before,
        "the export must not create or grow any store file"
    );
    let rows_after = store.list_token_daily_deltas(&hash).unwrap();
    assert_eq!(rows_before.len(), rows_after.len());
    for (before, after) in rows_before.iter().zip(rows_after.iter()) {
        assert_eq!(before.0, after.0);
        assert_eq!(before.1.owned_capacity_delta, after.1.owned_capacity_delta);
        assert_eq!(
            before.1.owned_knowledge_delta,
            after.1.owned_knowledge_delta
        );
    }
}

/// Every data file under the store root, with its size. RocksDB's textual LOG
/// is excluded: it records reads as well as writes.
fn store_files(root: &std::path::Path) -> Vec<(String, u64)> {
    fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, u64)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if path.is_dir() {
                walk(&path, &rel, out);
            } else if !name.starts_with("LOG") {
                out.push((rel, entry.metadata().unwrap().len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out.sort();
    out
}
