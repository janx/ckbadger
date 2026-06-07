mod common;
use common::*;

#[tokio::test]
async fn test_network_stats_returns_ok() {
    let store = test_store();
    let mut config = test_config(store);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_hardforks_endpoint_returns_default_timeline() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/hardforks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["network"], "mainnet");
    assert_eq!(json["tipEpoch"], 0);
    assert_eq!(json["tipBlock"], 0);
    assert!(json["events"].as_array().unwrap().len() >= 2);
    assert_eq!(json["events"][0]["status"], "upcoming");
    assert_eq!(json["events"][1]["status"], "upcoming");
}

#[tokio::test]
async fn test_hardforks_endpoint_marks_activated_and_fills_activation_block() {
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
        19_000_000,
        &CachedBlockHeader {
            hash: vec![0xaa; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 13_000,
            epoch_index: 100,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/hardforks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["tipEpoch"], 13_000);
    assert_eq!(json["tipBlock"], 19_000_000);
    assert_eq!(json["events"][0]["id"], "mirana-2021");
    assert_eq!(json["events"][0]["status"], "activated");
    assert_eq!(json["events"][0]["activationBlock"], 8_775_638);
    assert_eq!(json["events"][1]["id"], "meepo-2024");
    assert_eq!(json["events"][1]["status"], "activated");
}

#[tokio::test]
async fn test_hardforks_endpoint_rejects_unknown_network() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/hardforks?network=devnet")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_forks_recent_fails_on_deep_fork_invariant_violation() {
    let store = test_store();
    store
        .update_sync_status(|s| {
            s.deep_fork_detected = true;
            s.deep_fork_info = None;
        })
        .unwrap();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/forks/recent")
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
        .contains("deep_fork_detected=true but deep_fork_info is missing"));
}

#[tokio::test]
async fn test_forks_uses_persisted_reorg_detected_at_timestamp() {
    let store = test_store();
    store
        .set_deep_fork(DeepForkInfo {
            db_tip: 100,
            db_tip_hash: vec![0x11; 32],
            chain_tip: 160,
            chain_tip_hash: vec![0x22; 32],
            depth: 60,
            fork_point: 100,
        })
        .unwrap();

    let detected_at = 1_700_000_123i64;
    let event = ReorgEvent {
        detected_at,
        rollback_from: 101,
        rollback_to: 100,
        depth: 60,
    };
    store
        .put_cf(
            store.cf_sync_meta(),
            ckbadger_store::keys::sync_meta_keys::REORG_LATEST_EVENT,
            &bincode::serialize(&event).unwrap(),
        )
        .unwrap();

    let expected_detected_at = chrono::DateTime::<chrono::Utc>::from_timestamp(detected_at, 0)
        .unwrap()
        .to_rfc3339();

    let config = test_config(store);
    let app = create_router(config).await;

    let request_recent = Request::builder()
        .uri("/api/v1/forks/recent")
        .body(Body::empty())
        .unwrap();
    let response_recent = app.clone().oneshot(request_recent).await.unwrap();
    assert_eq!(response_recent.status(), StatusCode::OK);
    let body_recent = response_recent
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json_recent: serde_json::Value = serde_json::from_slice(&body_recent).unwrap();
    assert_eq!(json_recent["reorg"]["detectedAt"], expected_detected_at);

    let request_list = Request::builder()
        .uri("/api/v1/forks")
        .body(Body::empty())
        .unwrap();
    let response_list = app.oneshot(request_list).await.unwrap();
    assert_eq!(response_list.status(), StatusCode::OK);
    let body_list = response_list
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json_list: serde_json::Value = serde_json::from_slice(&body_list).unwrap();
    assert_eq!(json_list["data"][0]["detectedAt"], expected_detected_at);
}

#[tokio::test]
async fn test_recent_blocks_endpoint_empty_db() {
    let store = test_store();
    let mut config = test_config(store);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/recent-blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["blocks"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_tx_stats_reads_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let this_hour = now.timestamp() - 60;
    let date = ckbadger_common::block_date(now);
    let date_str = date.format("%Y%m%d").to_string();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0x10; 32],
            parent_hash: vec![0u8; 32],
            timestamp: now_ms,
            epoch_number: 1,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        },
    );
    core_batch.commit().unwrap();

    core_store
        .put_hourly_stats(
            &this_hour.to_string(),
            &HourlyStats {
                hour: this_hour,
                blocks_count: 1,
                transactions_count: 77,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &date_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 456,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                block_time_sum_ms: 0,
                block_time_count: 0,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/tx-stats")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["currentHour"], 77);
    assert!(!json["dailyData"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_epoch_time_charts_read_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let start = chrono::Utc::now() - chrono::Duration::hours(4);
    let end = chrono::Utc::now();

    core_store.put_epoch_time_dist(240, 3).unwrap();
    core_store
        .put_epoch_stats(
            12,
            &EpochStats {
                epoch_number: 12,
                start_block: 1,
                end_block: Some(100),
                blocks_count: 100,
                length: 1800,
                start_timestamp: start,
                end_timestamp: Some(end),
                transactions_count: 200,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let dist_request = Request::builder()
        .uri("/api/v1/charts/epoch-time-distribution")
        .body(Body::empty())
        .unwrap();
    let dist_response = app.clone().oneshot(dist_request).await.unwrap();
    assert_eq!(dist_response.status(), StatusCode::OK);
    let dist_body = dist_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let dist_json: serde_json::Value = serde_json::from_slice(&dist_body).unwrap();
    let dist_data = dist_json["data"].as_array().unwrap();
    assert!(dist_data
        .iter()
        .any(|point| point["date"] == "4.00" && point["value"] == "3"));

    let length_request = Request::builder()
        .uri("/api/v1/charts/epoch-time-length")
        .body(Body::empty())
        .unwrap();
    let length_response = app.clone().oneshot(length_request).await.unwrap();
    assert_eq!(length_response.status(), StatusCode::OK);
    let length_body = length_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let length_json: serde_json::Value = serde_json::from_slice(&length_body).unwrap();
    let length_data = length_json["data"].as_array().unwrap();
    assert_eq!(length_data.len(), 1);
    assert_eq!(length_data[0]["value2"], "100");
}

#[tokio::test]
async fn test_network_stats_reads_derived_statistics() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let today = ckbadger_common::block_date(now);
    let yesterday = today - chrono::Duration::days(1);
    let today_str = today.format("%Y%m%d").to_string();
    let yesterday_str = yesterday.format("%Y%m%d").to_string();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0u8; 32],
            timestamp: now_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        },
    );
    core_batch.commit().unwrap();

    core_store
        .put_epoch_stats(
            42,
            &EpochStats {
                epoch_number: 42,
                start_block: 1,
                end_block: None,
                blocks_count: 11,
                length: 1800,
                start_timestamp: now - chrono::Duration::seconds(110),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &today_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 120,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                block_time_sum_ms: 0,
                block_time_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &yesterday_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 80,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                block_time_sum_ms: 0,
                block_time_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            &today_str,
            &DailyBlockStats {
                avg_difficulty: 1_000_000.0,
                block_count: 100,
                total_uncles: 5,
                block_time_sum_ms: 100 * 10_000,
                block_time_count: 100,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["transactionsPerDay"], "200");
}

#[tokio::test]
async fn test_network_stats_includes_hero_metrics_from_dao_snapshot() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let today = ckbadger_common::block_date(now);
    let yesterday = today - chrono::Duration::days(1);
    let today_str = today.format("%Y%m%d").to_string();
    let yesterday_str = yesterday.format("%Y%m%d").to_string();

    // Minimal block header so fetch_network_stats_from_db succeeds
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0u8; 32],
            timestamp: now_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        },
    );
    core_batch.commit().unwrap();

    core_store
        .put_epoch_stats(
            42,
            &EpochStats {
                epoch_number: 42,
                start_block: 1,
                end_block: None,
                blocks_count: 11,
                length: 1800,
                start_timestamp: now - chrono::Duration::seconds(110),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &today_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 10,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                block_time_sum_ms: 0,
                block_time_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &yesterday_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 5,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                block_time_sum_ms: 0,
                block_time_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            &today_str,
            &DailyBlockStats {
                avg_difficulty: 1_000_000.0,
                block_count: 100,
                total_uncles: 5,
                block_time_sum_ms: 100 * 10_000,
                block_time_count: 100,
            },
        )
        .unwrap();

    // Write a DAO daily snapshot with known hero metric values
    let snapshot = DaoDailySnapshot {
        date: "2026-03-10".to_string(),
        total_deposited: 50_000_000_000_000_000,
        depositors_count: 100,
        new_deposits: 5,
        withdrawals: 2,
        compensation: 100_000_000_000_000,
        cumulative_deposit_amount: 60_000_000_000_000_000,
        total_issuance: 3_500_000_000_000_000_000,
        secondary_pool: 10_000_000_000_000_000,
        occupied_capacity: 1_000_000_000_000_000_000,
        cum_miner_secondary: 5_000_000_000_000_000,
        cum_dao_compensation: 3_000_000_000_000_000,
        cum_treasury: 2_000_000_000_000_000,
        unclaimed_compensation: 0,
        unmade_dao_interests: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20260310",
    );
    let snapshot_value = bincode::serialize(&snapshot).unwrap();
    core_store
        .put_cf(core_store.cf_stats_dao(), &snapshot_key, &snapshot_value)
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // knowledge_size = occupied_capacity
    assert_eq!(json["knowledgeSize"], "1000000000000000000");
    // circulating_supply = total_issuance - (GENESIS_BURNT + cum_treasury) - total_deposited
    let expected_circulating: i128 = 3_500_000_000_000_000_000
        - (840_000_000_000_000_000 + 2_000_000_000_000_000)
        - 50_000_000_000_000_000;
    assert_eq!(json["circulatingSupply"], expected_circulating.to_string());
    // dao_locked = total_deposited
    assert_eq!(json["daoLocked"], "50000000000000000");
}

#[tokio::test]
async fn test_daily_block_charts_read_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    core_store
        .put_daily_block_stats(
            "20260101",
            &DailyBlockStats {
                avg_difficulty: 1_000_000.0,
                block_count: 100,
                total_uncles: 2,
                block_time_sum_ms: 100 * 10_000,
                block_time_count: 100,
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            "20260102",
            &DailyBlockStats {
                avg_difficulty: 2_000_000.0,
                block_count: 120,
                total_uncles: 3,
                block_time_sum_ms: 120 * 10_000,
                block_time_count: 120,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    for path in [
        "/api/v1/charts/hash-rate",
        "/api/v1/charts/difficulty",
        "/api/v1/charts/uncle-rate",
    ] {
        let request = Request::builder().uri(path).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!json["data"].as_array().unwrap().is_empty(), "path={path}");
    }
}

#[tokio::test]
async fn test_miner_distribution_reads_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let miner_hash = vec![0x66; 32];
    core_store
        .put_miner_stats(
            "20260101",
            &miner_hash,
            &MinerStats {
                miner_lock_hash: miner_hash.clone(),
                blocks_count: 10,
                last_block_number: 99,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/miner-address-distribution")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalBlocks"], 10);
    assert_eq!(json["data"][0]["blocksMined"], 10);
}

#[tokio::test]
async fn test_hodl_wave_chart_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hodl-wave")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "CKB HODL Wave");
    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["series"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn test_hodl_wave_chart_with_data() {
    let store = test_store();

    // Insert test HODL wave data
    store
        .put_hodl_wave(
            "20240115",
            &ckbadger_store::types::DailyHodlWave {
                band_24h: 100_00000000,
                band_1d_1w: 200_00000000,
                band_1w_1m: 300_00000000,
                band_1m_3m: 400_00000000,
                band_3m_6m: 500_00000000,
                band_6m_1y: 600_00000000,
                band_1y_3y: 700_00000000,
                band_gt_3y: 800_00000000,
                holder_count: 42_000,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hodl-wave")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["title"], "CKB HODL Wave");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-15");
    // Verify holderCount is present
    assert_eq!(data[0]["values"]["holderCount"], "42000");
    // Verify percentage values are present (all should be > 0)
    let v24h: f64 = data[0]["values"]["24h"].as_str().unwrap().parse().unwrap();
    assert!(v24h > 0.0 && v24h < 100.0);
    // Series should have 8 entries
    assert_eq!(json["series"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn test_asset_ecosystem_returns_expected_structure() {
    let store = test_store();

    // Seed one token so the warmup cache populates CACHE_KEY_ASSETS_TOKEN.
    store
        .put_token_direct(
            &[0xAA; 32],
            &TokenInfo {
                type_code_hash: vec![0x01; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("TestToken".to_string()),
                symbol: Some("TT".to_string()),
                decimals: Some(8),
                total_supply: Some(1_000_000),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 10,
            },
        )
        .unwrap();
    // Seed holder entries in CF_TOKEN_HOLDERS so the live scan finds them.
    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_token_holder(&[0xAA; 32], &[0x01; 32], 500_000);
        batch.put_token_holder(&[0xAA; 32], &[0x02; 32], 500_000);
        batch.commit().unwrap();
    }
    store
        .put_token_daily_delta(
            &[0xAA; 32],
            20240101,
            &TokenDailyDelta {
                owned_capacity_delta: 500_00000000,
                owned_knowledge_delta: 300_00000000,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/asset-ecosystem")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify top-level structure (response is the struct directly, not wrapped in "data")
    assert!(json["topTokens"].is_array(), "topTokens should be an array");
    assert!(
        json["capacityBreakdown"].is_array(),
        "capacityBreakdown should be an array"
    );
    assert!(
        json["totalKnowledgeSizeCkb"].is_string(),
        "totalKnowledgeSizeCkb should be a string"
    );

    // Verify seeded token appears in topTokens
    let top_tokens = json["topTokens"].as_array().unwrap();
    assert_eq!(top_tokens.len(), 1);
    assert_eq!(top_tokens[0]["name"], "TestToken");
    assert_eq!(top_tokens[0]["symbol"], "TT");
    assert_eq!(top_tokens[0]["holdersCount"], 2);

    // Verify capacity breakdown has the expected categories
    let breakdown = json["capacityBreakdown"].as_array().unwrap();
    let categories: Vec<&str> = breakdown
        .iter()
        .map(|c| c["category"].as_str().unwrap())
        .collect();
    assert_eq!(categories, vec!["dao", "tokens", "objects", "other"]);
}
