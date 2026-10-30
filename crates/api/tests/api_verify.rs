//! `POST /api/v1/verify/entity-statistics` — the bounded, read-only export the
//! verifier compares chain-derived expectations against.
//!
//! Everything it returns must come from one read pin, as raw stored values, with
//! the sync state that makes them meaningful. It never computes a new user
//! statistic, never calls the node, and never writes.

use crate::common::*;

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

/// Write one family's retention state through the only path the writer uses.
fn seed_retention(store: &Arc<CkbadgerStore>, state: &ckbadger_store::types::HourlyRetentionState) {
    let mut batch = StoreBatch::new(store);
    batch.put_hourly_retention_state(state);
    batch.commit().unwrap();
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
                    tx_count: 1,
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
    // 2^53 + 1 is the smallest integer f64 cannot represent: a float round-trip
    // turns it back into 2^53, which is exactly what this export must not do.
    // The series is otherwise a valid live-capacity history, so the index's own
    // accumulation of it succeeds.
    seed_token(
        &store,
        &hash,
        &[
            (20260912, 9_007_199_254_740_993, 12_345),
            (20260913, -1, -1),
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
    assert_eq!(body["calendar"], serde_json::Value::Null);
    assert!(body.as_object().unwrap().contains_key("calendar"));

    let entity = &body["entities"][0];
    assert_eq!(entity["kind"], "token");
    assert_eq!(entity["id"], hex0x(&hash));
    assert_eq!(entity["present"], true);
    assert_eq!(entity["rowCount"], 2);
    assert_eq!(entity["complete"], true);

    // The index's own current totals, so the verifier can compare them against
    // the chain's live set rather than against a restatement of these rows.
    // The type script the selector names, from the same pinned read. The
    // verifier needs it to query chain history and must not have to ask an
    // endpoint that also computes aggregates.
    assert_eq!(entity["typeScript"]["codeHash"], hex0x(&[0xa1; 32]));
    assert_eq!(entity["typeScript"]["hashType"], "type");
    assert_eq!(entity["typeScript"]["args"], "0x01");

    assert_eq!(entity["currentCapacity"], "9007199254740992");
    assert_eq!(entity["currentKnowledge"], "12344");

    let daily = entity["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 2);
    assert_eq!(daily[0]["date"], 20260912);
    assert_eq!(daily[0]["capacityDelta"], "9007199254740993");
    assert_eq!(daily[0]["knowledgeDelta"], "12345");
    assert_eq!(daily[1]["date"], 20260913);
    assert_eq!(daily[1]["capacityDelta"], "-1");
    assert_eq!(daily[1]["knowledgeDelta"], "-1");
}

/// A corrupt row series makes the checked accumulation fail. That is evidence
/// of exactly the bug this endpoint exists to expose, so it is reported — with
/// the daily rows still exported, so the verifier can name the offending day —
/// rather than collapsing the request into a 500.
#[tokio::test]
async fn an_accumulation_that_cannot_be_computed_is_reported_not_a_server_error() {
    let store = test_store();
    let hash = token_hash(0xb7);
    seed_anchor(&store, 10, &token_hash(0x0b));
    // Consuming more than was ever created: live capacity would go negative.
    seed_token(&store, &hash, &[(20260101, 100, 50), (20260102, -300, -50)]);

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&hash) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let entity = &body["entities"][0];
    assert!(entity["currentCapacity"].is_null());
    assert!(entity["currentError"]
        .as_str()
        .unwrap_or_default()
        .contains("underflow"));
    assert_eq!(
        entity["typeScript"]["hashType"], "type",
        "identity survives a corrupt row series; only the totals do not"
    );
    assert_eq!(
        entity["daily"].as_array().unwrap().len(),
        2,
        "the rows that prove the corruption must still be exported"
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
    assert_eq!(entity["currentCapacity"], "0");
    assert_eq!(entity["currentKnowledge"], "0");
    assert!(
        entity["typeScript"].is_null(),
        "an absent entity has no script to publish"
    );
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
        entity["currentCapacity"], "6",
        "the current total accumulates every stored row, not just the returned page"
    );
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

/// A pinned anchor the store has moved past is the chain moving, not a
/// malformed request. Nothing is exported, and the actual anchor comes back
/// structurally so a client can re-pin without parsing prose.
#[tokio::test]
async fn an_expected_anchor_that_no_longer_matches_withholds_the_export() {
    let store = test_store();
    let hash = token_hash(0xa5);
    seed_anchor(&store, 500, &token_hash(0x05));
    seed_token(&store, &hash, &[(20260101, 11, 7)]);

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({
            "entities": [{ "kind": "token", "id": hex0x(&hash) }],
            "expectedAnchor": { "blockNumber": 499, "blockHash": hex0x(&token_hash(0x99)) }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["complete"], false);
    assert_eq!(body["anchorMismatch"]["expected"]["blockNumber"], 499);
    assert_eq!(
        body["anchorMismatch"]["expected"]["blockHash"],
        hex0x(&token_hash(0x99))
    );
    assert_eq!(body["anchorMismatch"]["actual"]["blockNumber"], 500);
    assert_eq!(
        body["anchorMismatch"]["actual"]["blockHash"],
        hex0x(&token_hash(0x05))
    );
    assert_eq!(body["anchor"]["blockNumber"], 500);

    let entity = &body["entities"][0];
    assert_eq!(entity["complete"], false);
    assert!(
        entity["daily"].as_array().unwrap().is_empty(),
        "rows from this view must not be handed out against another anchor"
    );
    assert!(entity["currentCapacity"].is_null());
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
    assert!(
        body["anchorMismatch"].is_null(),
        "a matching anchor is not a mismatch"
    );
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
        entity["currentCapacity"].is_null(),
        "a withheld export publishes no totals either"
    );
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

/// A store that has never written the coverage contract or a retention round
/// must say it has no evidence rather than imply one. Absence is reported per
/// family, because retention runs per family.
#[tokio::test]
async fn absent_contract_and_retention_rows_report_absence_of_evidence() {
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
    assert_eq!(
        body["state"]["hourlyRetention"],
        serde_json::json!({ "token": "unknown", "mnft": "unknown" }),
        "a family with no row is unknown, not a zero boundary"
    );
    assert_eq!(body["state"]["liveCellSummaryInitialized"], false);
    assert_eq!(body["state"]["deepForkDetected"], false);
}

/// The stored coverage contract is published field for field, including the
/// tip it was last advanced at: a floor without the block it was written at
/// cannot be told from a stale one.
#[tokio::test]
async fn the_undo_contract_is_published_with_every_stored_field() {
    let store = test_store();
    seed_anchor(&store, 9_001, &token_hash(0x11));
    store
        .put_entity_stats_undo_contract(&ckbadger_store::types::EntityStatsUndoContract {
            version: 1,
            coverage_floor_block: 8_800,
            updated_at_block: 9_000,
        })
        .unwrap();

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["state"]["entityStatsUndoContract"],
        serde_json::json!({
            "version": 1,
            "coverageFloorBlock": 8_800,
            "updatedAtBlock": 9_000,
        })
    );
}

/// A round that reached the end of its family is the only state whose
/// `executedCutoffHour` is a boundary a reader may trust.
#[tokio::test]
async fn a_completed_retention_round_is_authoritative() {
    let store = test_store();
    seed_anchor(&store, 500, &token_hash(0x12));
    seed_retention(
        &store,
        &ckbadger_store::types::HourlyRetentionState {
            policy_version: 1,
            family: ckbadger_store::types::HourlyRetentionFamily::Token,
            executed_cutoff_hour: 480_000,
            round_in_progress_cutoff_hour: None,
            cursor: None,
            round_started_at: 1_700_000_000_000,
            round_completed_at: Some(1_700_000_060_000),
        },
    );

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["state"]["hourlyRetention"]["token"],
        serde_json::json!({
            "authoritative": true,
            "policyVersion": 1,
            "executedCutoffHour": 480_000,
            "roundInProgressCutoffHour": null,
            "cursor": null,
            "roundStartedAt": 1_700_000_000_000i64,
            "roundCompletedAt": 1_700_000_060_000i64,
        })
    );
}

/// A round that stopped at a cursor has deleted only the keys before it, so the
/// family is reported in full — and reported as not a boundary.
///
/// The two families are read independently: `token` being half-swept says
/// nothing about `mnft`, and a single collapsed verdict would hide exactly the
/// family a verifier has to look at.
#[tokio::test]
async fn an_interrupted_retention_round_is_reported_but_not_authoritative() {
    let store = test_store();
    seed_anchor(&store, 501, &token_hash(0x13));
    seed_retention(
        &store,
        &ckbadger_store::types::HourlyRetentionState {
            policy_version: 1,
            family: ckbadger_store::types::HourlyRetentionFamily::Mnft,
            executed_cutoff_hour: 470_000,
            round_in_progress_cutoff_hour: Some(480_000),
            cursor: Some(vec![0xde, 0xad, 0xbe, 0xef]),
            round_started_at: 1_700_000_000_000,
            round_completed_at: None,
        },
    );

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["state"]["hourlyRetention"]["mnft"],
        serde_json::json!({
            "authoritative": false,
            "policyVersion": 1,
            "executedCutoffHour": 470_000,
            "roundInProgressCutoffHour": 480_000,
            "cursor": "0xdeadbeef",
            "roundStartedAt": 1_700_000_000_000i64,
            "roundCompletedAt": null,
        })
    );
    assert_eq!(
        body["state"]["hourlyRetention"]["token"], "unknown",
        "the other family has no row and must stay unknown, not inherit this one"
    );
}

/// Two families with different histories are reported as two answers.
#[tokio::test]
async fn each_family_reports_its_own_retention_state() {
    let store = test_store();
    seed_anchor(&store, 502, &token_hash(0x14));
    seed_retention(
        &store,
        &ckbadger_store::types::HourlyRetentionState {
            policy_version: 1,
            family: ckbadger_store::types::HourlyRetentionFamily::Token,
            executed_cutoff_hour: 480_000,
            round_in_progress_cutoff_hour: None,
            cursor: None,
            round_started_at: 1_700_000_000_000,
            round_completed_at: Some(1_700_000_060_000),
        },
    );
    seed_retention(
        &store,
        &ckbadger_store::types::HourlyRetentionState {
            policy_version: 1,
            family: ckbadger_store::types::HourlyRetentionFamily::Mnft,
            executed_cutoff_hour: 460_000,
            round_in_progress_cutoff_hour: Some(480_000),
            cursor: Some(vec![0x01]),
            round_started_at: 1_700_000_010_000,
            round_completed_at: None,
        },
    );

    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&token_hash(1)) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["state"]["hourlyRetention"]["token"]["authoritative"],
        true
    );
    assert_eq!(
        body["state"]["hourlyRetention"]["token"]["executedCutoffHour"],
        480_000
    );
    assert_eq!(
        body["state"]["hourlyRetention"]["mnft"]["authoritative"],
        false
    );
    assert_eq!(
        body["state"]["hourlyRetention"]["mnft"]["executedCutoffHour"],
        460_000
    );
}

/// The export must read the pinned store, never the background warmup cache.
///
/// The public asset endpoints serve cached aggregates that were computed under
/// a *different* read view. Labelling those with this request's anchor would
/// hand the verifier numbers and an anchor that never coexisted — the exact
/// incoherence the read pin exists to prevent.
#[tokio::test]
async fn the_export_ignores_the_warmup_cache() {
    let store = test_store();
    let hash = token_hash(0xc9);
    seed_anchor(&store, 7_000, &token_hash(0x0c));
    seed_token(&store, &hash, &[(20260101, 100, 50), (20260102, 40, 20)]);

    let state = test_app_state(test_config(store));
    // Warm the caches the way a running API would, then poison the token entry
    // so a handler that consults it cannot accidentally agree with the store.
    dispatch_initial_warmup(state.clone(), false).await;
    assert!(
        state.token_cache.load().is_some(),
        "the cache must be populated, or this test proves nothing"
    );
    state
        .token_cache
        .store(std::sync::Arc::new(Some(vec![poisoned_token_entry(
            &hex0x(&hash),
        )])));

    let app = axum::Router::new()
        .nest("/api/v1", api_routes())
        .layer(axum::middleware::from_fn(ckbadger_api::pin_read_view))
        .with_state(state);

    let (status, body) = post(
        app,
        serde_json::json!({ "entities": [{ "kind": "token", "id": hex0x(&hash) }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let entity = &body["entities"][0];
    assert_eq!(
        entity["currentCapacity"], "140",
        "the current total must come from the pinned store, not the cache"
    );
    assert_eq!(entity["currentKnowledge"], "70");
    assert_eq!(entity["rowCount"], 2);
    let daily = entity["daily"].as_array().unwrap();
    assert_eq!(daily[0]["capacityDelta"], "100");
    assert_eq!(daily[1]["capacityDelta"], "40");
}

/// A cache entry claiming wildly different totals for the same token.
fn poisoned_token_entry(id: &str) -> ckbadger_api::warmup::CachedAssetEntry {
    ckbadger_api::warmup::CachedAssetEntry {
        id: id.to_string(),
        asset_type: "token".to_string(),
        standard: "xudt".to_string(),
        name: Some("Stale".to_string()),
        symbol: Some("STALE".to_string()),
        icon_url: None,
        holders_count: 0,
        transfers_count: 0,
        live_count: None,
        activities_count: None,
        transfers_24h: 0,
        decimals: Some(8),
        total_supply: None,
        maximum_supply: None,
        content_type: None,
        content_size: None,
        cluster_id: None,
        cluster_name: None,
        owned_capacity: Some("999999999".to_string()),
        owned_knowledge: Some("888888888".to_string()),
        composition_tier: None,
        onchain_ratio: None,
        onchain_count: None,
        type_code_hash: Some(hex0x(&[0xa1; 32])),
        type_hash_type: Some("type".to_string()),
        type_args: Some("0x01".to_string()),
        description: None,
    }
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

fn calendar_request() -> serde_json::Value {
    serde_json::json!({
        "entities": [{ "kind": "token", "id": hex0x(&token_hash(0xce)) }],
        "includeCalendar": true
    })
}

fn seed_calendar_headers(store: &Arc<CkbadgerStore>, blocks: &[(i64, i64)]) {
    seed_block_headers_at(store, blocks);
    let tip = blocks.iter().map(|(number, _)| *number).max().unwrap();
    let header = store.get_block_header(tip).unwrap().unwrap();
    seed_anchor(store, tip, &header.hash);
}

#[tokio::test]
async fn indexed_calendar_preserves_quiet_midnight_and_raw_time_regression() {
    let store = test_store();
    seed_calendar_headers(
        &store,
        &[
            (0, utc_ms("2026-01-01T15:59:50Z")),
            // This block has no token activity, but crosses the calendar boundary.
            (1, utc_ms("2026-01-01T16:00:02Z")),
            (2, utc_ms("2026-01-01T15:59:58Z")),
            // January 3 has no blocks; it must not receive a fabricated interval.
            (3, utc_ms("2026-01-03T16:00:00Z")),
            (4, utc_ms("2026-01-03T15:59:58Z")),
        ],
    );
    seed_token(
        &store,
        &token_hash(0xce),
        &[(20260101, 10, 5), (20260104, -10, -5)],
    );
    let app = create_router_without_warmup(test_config(store));
    let (status, body) = post(app, calendar_request()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["calendar"],
        serde_json::json!({
            "source": "indexed_chain_time", "independentlyVerified": false, "complete": true,
            "buckets": [
                { "date": 20260101, "startBlock": 0, "endBlock": 0 },
                { "date": 20260102, "startBlock": 1, "endBlock": 2 },
                { "date": 20260104, "startBlock": 3, "endBlock": 4 }
            ]
        })
    );
    assert_eq!(body["complete"], true);
    assert_eq!(body["entities"][0]["daily"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn indexed_calendar_missing_genesis_or_search_header_is_an_error() {
    for missing in [0, 1] {
        let store = test_store();
        seed_calendar_headers(
            &store,
            &[
                (0, utc_ms("2026-01-01T15:59:50Z")),
                (1, utc_ms("2026-01-01T16:00:02Z")),
                (2, utc_ms("2026-01-02T16:00:02Z")),
            ],
        );
        store
            .delete_cf(
                store.cf_block_headers(),
                &ckbadger_store::keys::encode_block_num(missing),
            )
            .unwrap();
        let (status, body) = post(
            create_router_without_warmup(test_config(store)),
            calendar_request(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "missing={missing}: {body}"
        );
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains(&format!("block={missing}")),
            "{body}"
        );
    }
}

#[tokio::test]
async fn indexed_calendar_requires_header_tip_and_hash_to_match_sync_anchor() {
    for wrong_height in [false, true] {
        let store = test_store();
        seed_calendar_headers(&store, &[(0, utc_ms("2026-01-01T00:00:00Z"))]);
        seed_anchor(&store, if wrong_height { 1 } else { 0 }, &token_hash(0xf0));
        let (status, body) = post(
            create_router_without_warmup(test_config(store)),
            calendar_request(),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("calendar anchor"),
            "{body}"
        );
    }
}

#[tokio::test]
async fn indexed_calendar_rejects_negative_anchor_and_inconsistent_boundary_dates() {
    let store = test_store();
    seed_anchor(&store, -1, &token_hash(0x01));
    let (status, body) = post(
        create_router_without_warmup(test_config(store)),
        calendar_request(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("block=-1"),
        "{body}"
    );

    let store = test_store();
    seed_calendar_headers(
        &store,
        &[
            (0, utc_ms("2026-01-01T00:00:00Z")),
            (1, utc_ms("2026-01-03T00:00:00Z")),
            (2, utc_ms("2026-01-03T00:00:01Z")),
        ],
    );
    let mut corrupt = store.get_block_header(2).unwrap().unwrap();
    corrupt.chain_time_ms = utc_ms("2026-01-02T00:00:00Z");
    let mut batch = StoreBatch::new(&store);
    batch.put_block_header(2, &corrupt);
    batch.commit().unwrap();
    let (status, body) = post(
        create_router_without_warmup(test_config(store)),
        calendar_request(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("calendar"),
        "{body}"
    );
}

#[tokio::test]
async fn indexed_calendar_cap_marks_both_calendar_and_export_incomplete() {
    let store = test_store();
    let base = utc_ms("2020-01-01T00:00:00Z");
    let blocks: Vec<_> = (0..=8192)
        .map(|number| (number, base + number * 86_400_000))
        .collect();
    seed_calendar_headers(&store, &blocks);
    let (status, body) = post(
        create_router_without_warmup(test_config(store)),
        calendar_request(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["calendar"]["buckets"].as_array().unwrap().len(), 8192);
    assert_eq!(body["calendar"]["complete"], false);
    assert_eq!(body["complete"], false);
    assert_eq!(body["calendar"]["buckets"][8191]["endBlock"], 8191);
}

#[tokio::test]
async fn indexed_calendar_withheld_state_does_not_read_incomplete_headers() {
    for mismatch in [false, true] {
        let store = test_store();
        // Deliberately no headers: withheld state must not try to build a calendar.
        seed_anchor(&store, 10, &token_hash(0x10));
        let mut request = calendar_request();
        if mismatch {
            request["expectedAnchor"] =
                serde_json::json!({ "blockNumber": 9, "blockHash": hex0x(&token_hash(9)) });
        } else {
            store.set_rollback_cleanup_in_progress(true).unwrap();
        }
        let (status, body) = post(create_router_without_warmup(test_config(store)), request).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["calendar"], serde_json::Value::Null);
        assert!(body.as_object().unwrap().contains_key("calendar"));
        assert_eq!(body["complete"], false);
    }
}

#[tokio::test]
async fn indexed_calendar_and_entities_share_the_secondary_view_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let primary_path = directory.path().join("primary");
    let secondary_path = directory.path().join("secondary");
    let primary = Arc::new(CkbadgerStore::open_test_unified(&primary_path).unwrap());
    seed_calendar_headers(&primary, &[(0, utc_ms("2026-01-01T00:00:00Z"))]);
    seed_token(&primary, &token_hash(0xce), &[(20260101, 10, 5)]);
    primary.flush_all_memtables().unwrap();
    let secondary = Arc::new(
        CkbadgerStore::open_test_unified_secondary(&primary_path, &secondary_path).unwrap(),
    );
    seed_calendar_headers(&primary, &[(1, utc_ms("2026-01-02T00:00:00Z"))]);
    primary
        .put_token_daily_delta(
            &token_hash(0xce),
            20260102,
            &TokenDailyDelta {
                owned_capacity_delta: 20,
                owned_knowledge_delta: 10,
                tx_count: 1,
            },
        )
        .unwrap();
    primary.flush_all_memtables().unwrap();
    let before = store_files(&primary_path);
    let app = create_router_without_warmup(test_config(secondary.clone()));
    let (status, body) = post(app.clone(), calendar_request()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["anchor"]["blockNumber"], 0);
    assert_eq!(
        body["calendar"]["buckets"],
        serde_json::json!([{ "date": 20260101, "startBlock": 0, "endBlock": 0 }])
    );
    assert_eq!(body["entities"][0]["currentCapacity"], "10");
    assert_eq!(store_files(&primary_path), before);

    secondary.refresh().unwrap();
    let (status, body) = post(app, calendar_request()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["anchor"]["blockNumber"], 1);
    assert_eq!(body["calendar"]["buckets"].as_array().unwrap().len(), 2);
    assert_eq!(body["entities"][0]["currentCapacity"], "30");
    assert_eq!(store_files(&primary_path), before);
}

#[tokio::test]
async fn indexed_calendar_shares_the_response_byte_budget_with_daily_rows() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(directory.path()).unwrap());
    let base = utc_ms("2020-01-01T00:00:00Z");
    let blocks: Vec<_> = (0..8192)
        .map(|number| (number, base + number * 86_400_000))
        .collect();
    seed_calendar_headers(&store, &blocks);
    let mut entities = Vec::new();
    for byte in 1..=5 {
        let hash = token_hash(byte);
        seed_token(&store, &hash, &[]);
        for (_, time) in &blocks {
            let date: u32 = chrono::DateTime::from_timestamp_millis(*time)
                .unwrap()
                .format("%Y%m%d")
                .to_string()
                .parse()
                .unwrap();
            store
                .put_token_daily_delta(
                    &hash,
                    date,
                    &TokenDailyDelta {
                        owned_capacity_delta: 10i128.pow(33),
                        owned_knowledge_delta: 5 * 10i128.pow(32),
                        tx_count: 1,
                    },
                )
                .unwrap();
        }
        entities.push(serde_json::json!({ "kind": "token", "id": hex0x(&hash) }));
    }
    let app = create_router_without_warmup(test_config(store));
    let request = serde_json::json!({ "entities": entities });
    let (status, without_calendar) = post(app.clone(), request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{without_calendar}");
    let mut request_with_calendar = request;
    request_with_calendar["includeCalendar"] = true.into();
    let (status, with_calendar) = post(app, request_with_calendar).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(with_calendar["calendar"]["complete"], true);
    assert_eq!(
        with_calendar["calendar"]["buckets"]
            .as_array()
            .unwrap()
            .len(),
        8192
    );
    assert_eq!(with_calendar["complete"], false);
    let rows = |body: &serde_json::Value| -> usize {
        body["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| entity["daily"].as_array().unwrap().len())
            .sum()
    };
    assert!(
        rows(&with_calendar) < rows(&without_calendar),
        "calendar must consume the same budget as daily rows"
    );
    assert!(serde_json::to_vec(&with_calendar).unwrap().len() <= 4 * 1024 * 1024);
}
