mod common;
use common::*;

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
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing sync tip block while computing DAO statistics"));
}

#[tokio::test]
async fn test_dao_stats_uses_precomputed_latest_stats_when_tip_matches() {
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
}

#[tokio::test]
async fn test_dao_stats_ignores_stale_precomputed_latest_stats() {
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

    // No DAO deposits in DB, so fallback computation should be zero rather than stale 999.
    assert_eq!(json["totalDeposited"], "0");
    assert_eq!(json["totalDepositors"], 0);
}

/// While the indexer is still working through the first day there is no DAO
/// daily snapshot at the sync tip yet. That is a startup state, not a broken
/// one: it must report 503 `initializing` so the SPA retries behind its
/// initializing banner instead of showing a 500 that reads as a server fault.
#[tokio::test]
async fn test_dao_stats_reports_initializing_when_tip_snapshot_missing() {
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

    // Deliberately no dao_daily_snapshots row: the first day is still syncing.
    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "initializing");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("missing DAO daily snapshot"),
        "message should name the missing state and the tip: {json}"
    );
}

#[tokio::test]
async fn test_dao_stats_cached_response_is_stable_within_ttl() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let mut batch = StoreBatch::new(store.as_ref());

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
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
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20231115",
    );
    batch.put_stats(
        &snapshot_key,
        &bincode::serialize(&DaoDailySnapshot {
            date: "2023-11-15".to_string(),
            total_deposited: 200_00000000,
            depositors_count: 1,
            new_deposits: 1,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 200_00000000,
            total_issuance: 1,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 1,
            daily_depositor_addresses: 1,
            protocol_deposited: Some(200_00000000),
        })
        .unwrap(),
    );
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x11; 32], 0),
        &DaoDepositCacheEntry {
            capacity: 200_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 10,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x01; 32],
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
    batch.commit().unwrap();

    let app = create_router(test_config(store.clone())).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(first_json["totalDeposited"], "20000000000");

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x22; 32], 0),
        &DaoDepositCacheEntry {
            capacity: 300_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 10,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x02; 32],
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
    batch.commit().unwrap();

    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Expect cached response within TTL; without cache this would become 50000000000.
    assert_eq!(second_json["totalDeposited"], "20000000000");
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
