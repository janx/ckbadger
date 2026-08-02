mod common;
use common::*;

/// An empty DB has no `dao_latest_stats` singleton yet. That is a startup state
/// that resolves itself, so it must report 503 `initializing` (the contract the
/// SPA retries behind its banner) instead of a fault or a fabricated zero row.
#[tokio::test]
async fn test_dao_stats_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "initializing");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("dao_latest_stats not written yet"));
}

#[tokio::test]
async fn test_dao_stats_serves_the_indexer_singleton() {
    let store = test_store();

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xAA; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let latest = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 10,
        total_deposited: 123_00000000,
        total_depositors: 7,
        active_deposits: 9,
        total_compensation_paid: 11_00000000,
        unclaimed_compensation: 13_00000000,
        average_deposit_days: "950 days".to_string(),
        estimated_apc: "2.74".to_string(),
        mining_reward: 17_00000000,
        deposit_compensation: 19_00000000,
        burnt: 23_00000000,
        pending_withdrawal_capacity: 5_00000000,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let value = bincode::serialize(&latest).unwrap();
    store.put_stats_key(&key, &value).unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalDeposited"], "12300000000");
    assert_eq!(json["totalDepositors"], 7);
    assert_eq!(json["activeDeposits"], 9);
    assert_eq!(json["estimatedApc"], "2.74");
    assert_eq!(json["tipBlockNumber"], 10);
}

/// The singleton trails the sync tip by at most one batch commit, so a tip
/// mismatch is normal operation, not a reason to recompute the same numbers a
/// second way. The response serves the singleton and states the block it was
/// computed at.
#[tokio::test]
async fn test_dao_stats_serves_singleton_and_reports_its_as_of_block() {
    let store = test_store();
    seed_genesis_baseline(&store);

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xBB; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20231115",
    );
    batch.put_stats(
        &snapshot_key,
        &bincode::serialize(&DaoDailySnapshot {
            date: "2023-11-15".to_string(),
            total_deposited: 0,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance: 1,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: Some(0),
        })
        .unwrap(),
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let stale = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 9,
        total_deposited: 999_00000000,
        total_depositors: 999,
        active_deposits: 999,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: "999 days".to_string(),
        estimated_apc: "9.99".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
        pending_withdrawal_capacity: 0,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let value = bincode::serialize(&stale).unwrap();
    store.put_stats_key(&key, &value).unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["totalDeposited"], "99900000000");
    assert_eq!(json["totalDepositors"], 999);
    assert_eq!(
        json["tipBlockNumber"], 9,
        "the response must state the block the singleton was computed at"
    );
}

/// The indexer writes `dao_latest_stats` after every batch commit and after
/// every reorg rollback, so its absence at a synced tip is an invariant
/// violation. It must fail loudly with the tip it was observed at rather than
/// quietly recomputing the numbers a second way (which is what hid the
/// post-reorg singleton gap).
#[tokio::test]
async fn test_dao_stats_fails_fast_when_singleton_missing_at_synced_tip() {
    let store = test_store();
    seed_genesis_baseline(&store);

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xCC; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    // Deliberately no dao_latest_stats singleton at a synced tip.
    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("missing dao_latest_stats at sync tip block 10"),
        "message should name the missing singleton and the tip: {json}"
    );
}

/// `/dao/statistics` reads the singleton on every request and never caches it,
/// so a refreshed singleton (for example the one the indexer writes right after
/// a reorg rollback) is served immediately instead of behind a TTL.
#[tokio::test]
async fn test_dao_stats_follows_singleton_without_stale_cache() {
    let store = test_store();
    seed_tip_header(&store, 10);

    let mut latest = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 10,
        total_deposited: 200_00000000,
        total_depositors: 1,
        active_deposits: 1,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: "1 days".to_string(),
        estimated_apc: "2.00".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
        pending_withdrawal_capacity: 0,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    store
        .put_stats_key(&key, &bincode::serialize(&latest).unwrap())
        .unwrap();

    let app = create_router(test_config(store.clone())).await;
    let (status, first) = get_json(&app, "/dao/statistics").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["totalDeposited"], "20000000000");
    assert_eq!(first["tipBlockNumber"], 10);

    // The indexer advances the singleton (as it does after every batch commit
    // and after every rollback).
    latest.tip_block_number = 11;
    latest.total_deposited = 500_00000000;
    store
        .put_stats_key(&key, &bincode::serialize(&latest).unwrap())
        .unwrap();

    let (status, second) = get_json(&app, "/dao/statistics").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["totalDeposited"], "50000000000");
    assert_eq!(second["tipBlockNumber"], 11);
}

#[tokio::test]
async fn test_total_deposit_chart_recomputes_after_initial_empty_response() {
    let store = test_store();
    let config = test_config(store.clone());
    let app = create_router(config).await;

    let first_request = Request::builder()
        .uri("/api/v1/dao/charts/total-deposit")
        .body(Body::empty())
        .unwrap();
    let first_response = app.clone().oneshot(first_request).await.unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(first_json["data"], serde_json::json!([]));

    let snapshot = DaoDailySnapshot {
        date: "2024-01-15".to_string(),
        total_deposited: 123_00000000,
        depositors_count: 7,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 123_00000000,
        total_issuance: 0,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        unmade_dao_interests: 0,
        cumulative_depositors: 7,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20240115",
    );
    let value = bincode::serialize(&snapshot).unwrap();
    store.put_cf(store.cf_stats_dao(), &key, &value).unwrap();

    let second_request = Request::builder()
        .uri("/api/v1/dao/charts/total-deposit")
        .body(Body::empty())
        .unwrap();
    let second_response = app.oneshot(second_request).await.unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    let second_data = second_json["data"].as_array().unwrap();
    assert_eq!(second_data.len(), 1);
    assert_eq!(second_data[0]["date"], "2024-01-15");
    assert_eq!(second_data[0]["value"], "123");
    assert_eq!(second_data[0]["value2"], "7");
}

#[tokio::test]
async fn test_dao_deposits_cursor_pagination_descending() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xA1; 32], 0i16, 30i64),
        (vec![0xA2; 32], 0i16, 20i64),
        (vec![0xA3; 32], 0i16, 10i64),
    ];
    for (tx_hash, output_index, block_number) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, output_index),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x22; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
    let next_cursor = json["nextCursor"].as_str().unwrap();
    assert!(next_cursor.starts_with("0x"));
    assert_eq!(json["hasMore"], true);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=2&cursor={}",
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 10);
    assert!(json["nextCursor"].is_null());
    assert_eq!(json["hasMore"], false);
}

#[tokio::test]
async fn test_dao_deposits_cursor_pagination_keeps_same_block_rows() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xD1; 32], 0i16, 30i64),
        (vec![0xD2; 32], 1i16, 30i64),
        (vec![0xD3; 32], 2i16, 30i64),
        (vec![0xD4; 32], 0i16, 20i64),
    ];
    for (tx_hash, output_index, block_number) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, output_index),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x33; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 30);
    let next_cursor = json["nextCursor"].as_str().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=2&cursor={}",
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
}

#[tokio::test]
async fn test_dao_deposits_status_filter_uses_descending_order() {
    let store = test_store();
    let lock_a = vec![0x11; 32];
    let lock_b = vec![0x22; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xB1; 32], 30i64, 1i16, lock_a.clone()),
        (vec![0xB2; 32], 20i64, 1i16, lock_b.clone()),
        (vec![0xB3; 32], 10i64, 0i16, lock_a.clone()),
    ];
    for (tx_hash, block_number, status, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=10&status=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
}

#[tokio::test]
async fn test_dao_deposits_status_cursor_mismatch_returns_bad_request() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xE1; 32], 30i64, 0i16, vec![0x11; 32]),
        (vec![0xE2; 32], 20i64, 0i16, vec![0x22; 32]),
        (vec![0xE3; 32], 10i64, 1i16, vec![0x33; 32]),
    ];
    for (tx_hash, block_number, status, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=1&status=0")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=1&status=1&cursor={}",
            cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Invalid dao deposits cursor"));
}

#[tokio::test]
async fn test_dao_deposits_by_lock_hash_cursor_pagination() {
    let store = test_store();
    let lock_a = vec![0x33; 32];
    let lock_b = vec![0x44; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xC1; 32], 30i64, lock_a.clone()),
        (vec![0xC2; 32], 20i64, lock_a.clone()),
        (vec![0xC3; 32], 10i64, lock_b),
    ];
    for (tx_hash, block_number, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1",
            hex::encode(&lock_a)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    let next_cursor = json["nextCursor"].as_str().unwrap();
    assert!(next_cursor.starts_with("0x"));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1&cursor={}",
            hex::encode(&lock_a),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 20);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_dao_deposits_by_lock_hash_cursor_mismatch_returns_bad_request() {
    let store = test_store();
    let lock_a = vec![0x55; 32];
    let lock_b = vec![0x66; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xF1; 32], 30i64, lock_a.clone()),
        (vec![0xF2; 32], 20i64, lock_b.clone()),
        (vec![0xF3; 32], 10i64, lock_b.clone()),
    ];
    for (tx_hash, block_number, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1",
            hex::encode(&lock_b)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1&cursor={}",
            hex::encode(&lock_a),
            cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Invalid dao deposits by address cursor"));
}

#[tokio::test]
async fn test_top_depositors_resolves_address_from_lock_script() {
    use ckbadger_store::{DaoTopDepositorEntry, DaoTopDepositors, LockScriptEntry, StoreBatch};

    let store = test_store();

    // secp256k1_blake160 (hash_type=type) with 20-byte args.
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = vec![0x11u8; 20];
    let lock_hash = vec![0xABu8; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &lock_hash,
        &LockScriptEntry {
            code_hash: code_hash.clone(),
            hash_type: 1,
            args: args.clone(),
        },
    );
    batch.commit().unwrap();

    store
        .put_dao_top_depositors(&DaoTopDepositors {
            tip_block_number: 100,
            depositors: vec![DaoTopDepositorEntry {
                lock_script_hash: lock_hash.clone(),
                total_capacity: 100_000_000_000,
                deposit_count: 2,
                average_deposit_ms: 86_400_000.0,
            }],
        })
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["tipBlockNumber"], 100,
        "the leaderboard must state the block it was built at"
    );

    let expected =
        ckbadger_common::script_to_address(&code_hash, 1, &args, "mainnet").expect("encode");
    assert!(expected.starts_with("ckb1"));
    let dep = &json["depositors"][0];
    assert_eq!(
        dep["lockScriptHash"],
        serde_json::json!(format!("0x{}", hex::encode(&lock_hash)))
    );
    assert_eq!(dep["address"], serde_json::json!(expected));
}

#[tokio::test]
async fn test_top_depositors_address_null_when_lock_script_unknown() {
    use ckbadger_store::{DaoTopDepositorEntry, DaoTopDepositors};

    let store = test_store();
    store
        .put_dao_top_depositors(&DaoTopDepositors {
            tip_block_number: 100,
            depositors: vec![DaoTopDepositorEntry {
                lock_script_hash: vec![0xCDu8; 32],
                total_capacity: 42,
                deposit_count: 1,
                average_deposit_ms: 0.0,
            }],
        })
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["depositors"][0]["address"].is_null());
}

/// Seed a block header so `get_sync_tip_block()` reports a committed tip.
fn seed_tip_header(store: &Arc<CkbadgerStore>, block_number: i64) {
    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        block_number,
        &CachedBlockHeader {
            hash: vec![0xA5; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.commit().unwrap();
}

fn seed_dao_daily_snapshot(
    store: &Arc<CkbadgerStore>,
    date: &str,
    cumulative_deposit_amount: i128,
    new_deposits: i64,
) {
    let key_date = date.replace('-', "");
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        key_date.as_bytes(),
    );
    let snapshot = DaoDailySnapshot {
        date: date.to_string(),
        total_deposited: cumulative_deposit_amount,
        depositors_count: new_deposits,
        new_deposits,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount,
        total_issuance: 1,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        unmade_dao_interests: 0,
        cumulative_depositors: new_deposits,
        daily_depositor_addresses: new_deposits,
        protocol_deposited: Some(cumulative_deposit_amount),
    };
    store
        .put_stats_key(&key, &bincode::serialize(&snapshot).unwrap())
        .unwrap();
}

/// The DAO singletons are written by the indexer after every batch commit and
/// after every reorg rollback. Their absence at a synced tip is an invariant
/// violation, not an empty leaderboard: masking it with a default-empty value
/// is exactly what made a ~6s store gap look like 35-40s of "no depositors".
#[tokio::test]
async fn test_top_depositors_fails_fast_when_singleton_missing_at_synced_tip() {
    let store = test_store();
    seed_tip_header(&store, 10);

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["error"], "internal_error");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("dao_top_depositors") && message.contains("10"),
        "error must name the missing singleton and the tip block: {message}"
    );
}

/// Before the indexer has committed anything there is no singleton yet. That is
/// a startup state that resolves itself, so it must be the explicit
/// `initializing` state rather than a 500 or a fabricated empty list.
#[tokio::test]
async fn test_top_depositors_reports_initializing_before_first_block() {
    let store = test_store();

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json["error"], "initializing");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("dao_top_depositors"));
}

/// The daily chart is the day-over-day delta of a cumulative series, so the
/// first snapshot day's delta is "cumulative total minus nothing". Deriving it
/// from `windows(2)` silently drops launch day, hiding the genesis day's
/// deposits from the chart entirely.
#[tokio::test]
async fn test_daily_deposit_chart_includes_first_snapshot_day() {
    let store = test_store();
    seed_dao_daily_snapshot(&store, "2019-11-16", 3_715_755_618_324_833, 33);
    seed_dao_daily_snapshot(&store, "2019-11-17", 3_815_755_618_324_833, 77);

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/charts/daily-deposit").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2, "genesis day must be present, got {:?}", data);
    assert_eq!(data[0]["date"], "2019-11-16");
    assert_eq!(data[0]["value"], "37157556.18324833");
    assert_eq!(data[0]["value2"], "33");
    assert_eq!(data[1]["date"], "2019-11-17");
    assert_eq!(data[1]["value"], "1000000");
    assert_eq!(data[1]["value2"], "44");
}
