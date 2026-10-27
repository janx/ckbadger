mod common;
use common::*;

async fn mock_indexed_node_tip(config: &mut AppConfig) -> MockServer {
    let tip = config.store.get_sync_tip_block().unwrap().unwrap().0;
    let rpc = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"method":"get_tip_block_number"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "result": format!("0x{tip:x}")
        })))
        .mount(&rpc)
        .await;
    config.ckb_rpc_url = rpc.uri();
    rpc
}

#[tokio::test]
async fn network_stats_does_not_report_synced_when_node_tip_rpc_fails() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000)]);
    let mut header = store.get_block_header(0).unwrap().unwrap();
    header.compact_target = 0x190d_f964;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.commit().unwrap();
    let rpc = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"method":"get_tip_block_number"}),
        ))
        .respond_with(ResponseTemplate::new(503))
        .mount(&rpc)
        .await;
    let mut config = test_config(store);
    config.ckb_rpc_url = rpc.uri();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/statistics/network").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("failed to read CKB node tip"));
}

#[tokio::test]
async fn network_transaction_window_uses_effective_time_when_raw_tip_moves_backward() {
    let store = test_store();
    let raw_earlier_ms = 1_700_337_600_000i64; // exact UTC hour
    let effective_ms = raw_earlier_ms + 5_400_000; // next hour + 30 minutes
    seed_block_headers_at(
        &store,
        &[
            (0, raw_earlier_ms),
            (1, effective_ms),
            (2, raw_earlier_ms + 1_800_000),
        ],
    );
    let mut batch = StoreBatch::new(store.as_ref());
    // The tip's raw timestamp moves back an hour, so its effective time stays
    // at the prior block's 21:30 prefix maximum.
    for (number, chain_time_ms) in [(0, raw_earlier_ms), (1, effective_ms), (2, effective_ms)] {
        let mut header = store.get_block_header(number).unwrap().unwrap();
        header.compact_target = 0x190d_f964;
        header.chain_time_ms = chain_time_ms;
        batch.put_block_header(number, &header);
    }
    batch.commit().unwrap();
    for (hour_ms, count) in [(raw_earlier_ms, 13), (raw_earlier_ms + 3_600_000, 7)] {
        store
            .put_hourly_stats(
                &chrono::DateTime::from_timestamp_millis(hour_ms)
                    .unwrap()
                    .format("%Y%m%d%H")
                    .to_string(),
                &HourlyStats {
                    hour: hour_ms / 1000,
                    blocks_count: 1,
                    transactions_count: count,
                    cells_created: 0,
                    cells_consumed: 0,
                    capacity_transferred: 0,
                },
            )
            .unwrap();
    }
    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/statistics/network").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["transactionsPerDay"], "320");
}

#[tokio::test]
async fn test_network_stats_reports_initializing_before_genesis() {
    let store = test_store();
    let mut config = test_config(store);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "initializing");
    assert!(json["message"].as_str().unwrap().contains("block header"));
}

#[tokio::test]
async fn test_network_stats_rejects_zero_difficulty_for_existing_tip() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000)]);
    let mut header = store.get_block_header(0).unwrap().unwrap();
    header.compact_target = 0;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.commit().unwrap();
    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/statistics/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("zero difficulty"));
}

#[tokio::test]
async fn test_network_stats_reports_unknown_averages_with_one_header() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000)]);
    let mut header = store.get_block_header(0).unwrap().unwrap();
    header.compact_target = 0x190df964;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.commit().unwrap();
    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/statistics/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(json["avgBlockTime"].is_null(), "{json}");
    assert!(json["estimatedEpochTime"].is_null(), "{json}");
    assert!(json["tps"].is_null(), "{json}");
    assert!(json["transactionsPerMinute"].is_null(), "{json}");
    assert!(json["transactionsPerDay"].is_null(), "{json}");
}

#[tokio::test]
async fn test_network_stats_rejects_corrupt_sync_progress() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000)]);
    let mut header = store.get_block_header(0).unwrap().unwrap();
    header.compact_target = 0x190df964;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.commit().unwrap();
    store.put_sync_progress(b"not-json").unwrap();
    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/statistics/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("invalid sync progress"));
}

#[tokio::test]
async fn test_network_stats_allows_zero_chain_time_span_without_a_rate() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000), (1, 1_700_000_000_000)]);
    let mut batch = StoreBatch::new(store.as_ref());
    for number in 0..=1 {
        let mut header = store.get_block_header(number).unwrap().unwrap();
        header.compact_target = 0x190df964;
        batch.put_block_header(number, &header);
    }
    batch.commit().unwrap();
    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/statistics/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(json["avgBlockTime"], "0.00s");
    assert_eq!(json["estimatedEpochTime"], "0s");
    assert!(json["hashRate"].is_null());
}

#[tokio::test]
async fn test_network_stats_rejects_invalid_tip_timestamp() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, i64::MAX)]);
    let mut header = store.get_block_header(0).unwrap().unwrap();
    header.compact_target = 0x190df964;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.commit().unwrap();
    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/statistics/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        String::from_utf8_lossy(&body)
            .contains("invalid timestamp on sync tip block 0: 9223372036854775807 ms"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn test_knowledge_size_chart_rejects_missing_complete_day_dao_snapshot() {
    let store = test_store();
    seed_genesis_baseline(&store);
    store
        .put_daily_stats(
            "20260101",
            &DailyStats {
                knowledge_size: Some(100),
                ..Default::default()
            },
        )
        .unwrap();
    seed_tip_block_on_utc8_day(&store, 40, "2026-01-03");
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/charts/knowledge-size")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body)
        .contains("missing DAO daily snapshot for complete knowledge-size date 2026-01-01"));
}

#[tokio::test]
async fn test_uncle_rate_rejects_zero_block_daily_row() {
    let store = test_store();
    store
        .put_daily_block_stats(
            "20260101",
            &DailyBlockStats {
                difficulty_sum: 0,
                block_count: 0,
                total_uncles: 1,
            },
        )
        .unwrap();
    seed_tip_block_on_utc8_day(&store, 40, "2026-01-03");
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/charts/uncle-rate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body)
        .contains("daily block stats for 20260101 has invalid block_count=0"));
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
    seed_reorg_event_of_kind(
        &store,
        detected_at * 1000,
        100,
        60,
        0xD1,
        ckbadger_store::types::ReorgEventKind::Deep,
    );

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
    assert_eq!(json_recent["reorg"]["eventType"], "deep");
    assert_eq!(json_recent["deepFork"]["detected"], true);
    assert_eq!(json_recent["deepFork"]["detectedAt"], expected_detected_at);

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

fn recent_block_header(number: i64, ts_ms: i64) -> CachedBlockHeader {
    CachedBlockHeader {
        hash: vec![(number % 251) as u8; 32],
        parent_hash: vec![0u8; 32],
        timestamp: ts_ms,
        epoch_number: 1,
        epoch_index: 0,
        epoch_length: 1800,
        dao: vec![0; 32],
        transactions_count: 2,
        uncles_count: 0,
        proposals_count: 0,
        compact_target: 0x1a08a97e,
        miner_lock_hash: None,
        cycles: None,
        chain_time_ms: ts_ms,
    }
}

/// Regression (C4): the 24h window must never be silently truncated by a
/// fixed fetch cap. Node-proven: 2026-07-30 (UTC+8) had 10,141 blocks inside
/// 24h, while the old implementation fetched a single 10,000-block page and
/// filtered, silently dropping the oldest in-window blocks.
#[tokio::test]
async fn test_recent_blocks_covers_full_24h_window_beyond_10000_blocks() {
    let store = test_store();

    let tip_ms: i64 = 1_800_000_000_000;
    let cutoff_ms = tip_ms - 24 * 3600 * 1000;
    let in_window = 10_001i64;
    // 10,000 gaps at 8s span ~22.2h — every seeded recent block sits inside
    // the 24h window.
    let oldest_in_window_ms = tip_ms - (in_window - 1) * 8_000;
    assert!(oldest_in_window_ms > cutoff_ms);

    let mut batch = StoreBatch::new(store.as_ref());
    // Blocks 3..=10_003 are inside the window (10_001 blocks, tip = 10_003)…
    for i in 0..in_window {
        let number = 3 + i;
        batch.put_block_header(
            number,
            &recent_block_header(number, oldest_in_window_ms + i * 8_000),
        );
    }
    // …blocks 0..=2 are at/before the cutoff and must be excluded.
    for number in 0..3i64 {
        batch.put_block_header(
            number,
            &recent_block_header(number, cutoff_ms - (3 - number) * 8_000),
        );
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/recent-blocks")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let blocks = json["blocks"].as_array().unwrap();

    assert_eq!(
        blocks.len(),
        in_window as usize,
        "every block inside the 24h window must be returned"
    );
    // Ascending order: first is the oldest in-window block, last is the tip.
    assert_eq!(
        blocks[0]["timestamp"].as_i64().unwrap(),
        oldest_in_window_ms
    );
    assert_eq!(
        blocks.last().unwrap()["timestamp"].as_i64().unwrap(),
        tip_ms
    );
    assert!(
        blocks
            .iter()
            .all(|b| b["timestamp"].as_i64().unwrap() > cutoff_ms),
        "blocks at/before the cutoff must be excluded"
    );
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
        // The only block: the whole median-time window.
        0,
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
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: now_ms,
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
    // Regression (F2/F3/F6/F7):
    //  - difficulty = current-epoch difficulty from the tip compact_target
    //    (mainnet vector 0x190df964 → 1,320,058,941,807,520,729 = "1.32 E"),
    //    not a daily average;
    //  - avgBlockTime = windowed average with ms precision (two 8s gaps →
    //    "8.00s"), not a hardcoded 10.00s;
    //  - hashRate = the window's summed per-block work over its span in true
    //    H/s ("165.01 PH/s"; equal to difficulty / avg seconds here because
    //    the seeded window is uniform-difficulty), not the 1000×-understated
    //    per-millisecond figure;
    //  - transactionsPerDay = the rolling last-24h rate normalized to a day
    //    (count / window_secs * 86400) from the same hourly-bucket window as
    //    tps/perMinute — not today+yesterday calendar days, and not the raw
    //    quantized-window sum.
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    for (i, block_num) in (198i64..=200).enumerate() {
        core_batch.put_block_header(
            block_num,
            &CachedBlockHeader {
                hash: vec![0x20 + i as u8; 32],
                parent_hash: vec![0u8; 32],
                timestamp: now_ms - (200 - block_num) * 8_000,
                epoch_number: 42,
                epoch_index: 10,
                epoch_length: 1800,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0x190d_f964,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: now_ms - (200 - block_num) * 8_000,
            },
        );
    }
    core_batch.commit().unwrap();

    // Two hourly buckets inside the trailing 24h window: 120 + 80 = 200.
    // One stale bucket outside the window that must NOT be counted.
    let hour_secs = 3600;
    let now_hour = (now_ms / 1000) / hour_secs * hour_secs;
    for (offset_hours, txs) in [(0i64, 120), (1, 80), (30, 999)] {
        let bucket_start = now_hour - offset_hours * hour_secs;
        core_store
            .put_hourly_stats(
                &chrono::DateTime::from_timestamp(bucket_start, 0)
                    .unwrap()
                    .format("%Y%m%d%H")
                    .to_string(),
                &HourlyStats {
                    hour: bucket_start,
                    blocks_count: 1,
                    transactions_count: txs,
                    cells_created: 0,
                    cells_consumed: 0,
                    capacity_transferred: 0,
                },
            )
            .unwrap();
    }

    let mut config = test_config_with_append_only(core_store, append_only_store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // perDay replicates the handler's exact normalization: 200 txs over the
    // window from the oldest included bucket start (now_hour - 3600) to the
    // tip timestamp, scaled to 86400s. The stale 999-tx bucket stays excluded
    // (a raw or wider window would inflate this far beyond the expectation).
    let window_ms = now_ms - (now_hour - 3600) * 1000;
    let numerator = 200i128 * 86_400 * 1000;
    let expected_per_day =
        ((numerator + i128::from(window_ms) / 2) / i128::from(window_ms)).to_string();
    assert_eq!(json["transactionsPerDay"], expected_per_day);
    assert_eq!(json["difficulty"], "1.32 E");
    assert_eq!(json["avgBlockTime"], "8.00s");
    // 1,320,058,941,807,520,729 hashes-per-block / 8s = 165.01 PH/s.
    assert_eq!(json["hashRate"], "165.01 PH/s");
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
            proposals_count: 0,
            compact_target: 0x190df964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: now_ms,
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
                difficulty_sum: 100_000_000,
                block_count: 100,
                total_uncles: 5,
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
        frozen_phase1_compensation: 0,
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

    // Genesis baseline supplies the burnt amount used in circulating-supply math.
    // Seed the mainnet 8.4B burn so the hero-metric assertion below reflects it.
    core_store
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance: 3_500_000_000_000_000_000,
            burnt: 840_000_000_000_000_000,
            virtual_occupied: 0,
        })
        .unwrap();

    let mut config = test_config_with_append_only(core_store, append_only_store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // knowledge_size = occupied_capacity (U) − virtual_occupied, and this
    // baseline declares no burn adjustment.
    assert_eq!(json["knowledgeSize"], "1000000000000000000");
    // circulating_supply = DAO C - genesis burnt - DAO S (all unissued secondary).
    let expected_circulating: i128 =
        3_500_000_000_000_000_000 - 840_000_000_000_000_000 - 10_000_000_000_000_000;
    assert_eq!(json["circulatingSupply"], expected_circulating.to_string());
    // dao_locked = total_deposited
    assert_eq!(json["daoLocked"], "50000000000000000");
}

/// Regression: circulating supply must derive `burnt` from the seeded genesis
/// baseline, NOT from the hardcoded mainnet 8.4B constant. Seeding a distinct
/// (testnet-style) burn proves the value flows through `state.genesis_baseline()`.
#[tokio::test]
async fn test_network_stats_circulating_supply_uses_seeded_genesis_baseline() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();

    // Minimal block header so fetch_network_stats_from_db succeeds.
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x33; 32],
            parent_hash: vec![0u8; 32],
            timestamp: now_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x190df964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: now_ms,
        },
    );
    core_batch.commit().unwrap();

    // Distinct, non-mainnet burn (1.23M CKB) so the assertion fails if the old
    // hardcoded 8.4B constant is used instead of the baseline.
    let seeded_burnt: i128 = 123_000_000_000_000;
    let total_issuance: i128 = 3_500_000_000_000_000_000;
    let cum_treasury: i128 = 2_000_000_000_000_000;
    let secondary_pool: i128 = 10_000_000_000_000_000;
    let total_deposited: i128 = 50_000_000_000_000_000;

    let snapshot = DaoDailySnapshot {
        date: "2026-03-10".to_string(),
        total_deposited,
        depositors_count: 100,
        new_deposits: 5,
        withdrawals: 2,
        compensation: 100_000_000_000_000,
        cumulative_deposit_amount: 60_000_000_000_000_000,
        total_issuance,
        secondary_pool,
        occupied_capacity: 1_000_000_000_000_000_000,
        cum_miner_secondary: 5_000_000_000_000_000,
        cum_dao_compensation: 3_000_000_000_000_000,
        cum_treasury,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
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

    core_store
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance,
            burnt: seeded_burnt,
            virtual_occupied: 0,
        })
        .unwrap();

    let mut config = test_config_with_append_only(core_store, append_only_store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let expected_circulating: i128 = total_issuance - seeded_burnt - secondary_pool;
    assert_eq!(json["circulatingSupply"], expected_circulating.to_string());
}

/// The circulating-supply hero metric must still refuse to invent a number when
/// the genesis baseline has not been derived yet, but "the indexer has not
/// written block 0 yet" is a startup state, not a server fault: it reports 503
/// `initializing` (the contract the SPA's initializing UX keys on), never a 500.
#[tokio::test]
async fn test_network_stats_reports_initializing_when_baseline_missing() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x44; 32],
            parent_hash: vec![0u8; 32],
            timestamp: now_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x190df964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: now_ms,
        },
    );
    core_batch.commit().unwrap();

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
        frozen_phase1_compensation: 0,
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

    // Intentionally do NOT seed the genesis baseline.
    let mut config = test_config_with_append_only(core_store, append_only_store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "initializing");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("genesis baseline"),
        "message should name the missing state: {json}"
    );
}

#[tokio::test]
async fn test_daily_block_charts_read_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    core_store
        .put_daily_block_stats(
            "20260101",
            &DailyBlockStats {
                difficulty_sum: 100_000_000,
                block_count: 100,
                total_uncles: 2,
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            "20260102",
            &DailyBlockStats {
                difficulty_sum: 240_000_000,
                block_count: 120,
                total_uncles: 3,
            },
        )
        .unwrap();
    // The hash-rate divisor comes from DailyStats (the copy both the bulk and
    // the live writer maintain), so every daily-block row needs its sibling.
    for (date, blocks) in [("20260101", 100), ("20260102", 120)] {
        core_store
            .put_daily_stats(
                date,
                &DailyStats {
                    blocks_count: blocks,
                    block_time_sum_ms: blocks as i64 * 10_000,
                    block_time_count: blocks,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    // 2026-01-02 holds the indexed tip block: in progress, not charted.
    seed_tip_block_on_utc8_day(&core_store, 220, "2026-01-02");

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

    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET).unwrap();
    let latest_complete_date =
        chrono::Utc::now().with_timezone(&utc8).date_naive() - chrono::Duration::days(1);
    let included_date = latest_complete_date.format("%Y%m%d").to_string();
    let excluded_date = (latest_complete_date - chrono::Duration::days(7))
        .format("%Y%m%d")
        .to_string();

    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = hex::decode("8211f1b938a107cd53b6302cc752a6fc3965638d").unwrap();
    let miner_hash = compute_script_hash(&code_hash, 1, &args);
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_lock_script(
        &miner_hash,
        &ckbadger_store::LockScriptEntry {
            code_hash,
            hash_type: 1,
            args,
        },
    );
    batch.commit().unwrap();

    core_store
        .put_miner_stats(
            &included_date,
            &miner_hash,
            &MinerStats {
                miner_lock_hash: miner_hash.clone(),
                blocks_count: 10,
                last_block_number: 99,
            },
        )
        .unwrap();
    core_store
        .put_miner_stats(
            &excluded_date,
            &[0x77; 32],
            &MinerStats {
                miner_lock_hash: vec![0x77; 32],
                blocks_count: 90,
                last_block_number: 1,
            },
        )
        .unwrap();
    // The indexed tip is today, so its last complete day is yesterday.
    seed_tip_block_on_utc8_day(
        &core_store,
        50,
        &(latest_complete_date + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string(),
    );

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
    assert_eq!(json["windowDays"], 7);
    assert_eq!(
        json["fromDate"],
        (latest_complete_date - chrono::Duration::days(6))
            .format("%Y-%m-%d")
            .to_string()
    );
    assert_eq!(
        json["toDate"],
        latest_complete_date.format("%Y-%m-%d").to_string()
    );
    assert_eq!(
        json["data"][0]["minerLockHash"],
        format!("0x{}", hex::encode(&miner_hash))
    );
    assert!(json["data"][0]["address"]
        .as_str()
        .unwrap()
        .starts_with("ckb1"));
}

/// Regression (STATS-014, review B): the miner distribution's "last 7 complete
/// days" came from the wall clock. An indexer that lags the chain (here by
/// years) then reported an empty window of days it has not indexed, while
/// its own complete days went unshown. The window ends at the chain's last
/// complete day by the median-time rule.
#[tokio::test]
async fn test_miner_distribution_window_ends_at_the_chain_s_last_complete_day() {
    let store = test_store();
    for (date, blocks) in [("20240115", 10), ("20240108", 90)] {
        store
            .put_miner_stats(
                date,
                &[0x66; 32],
                &MinerStats {
                    miner_lock_hash: vec![0x66; 32],
                    blocks_count: blocks,
                    last_block_number: 1,
                },
            )
            .unwrap();
    }
    seed_tip_block_on_utc8_day(&store, 50, "2024-01-16");
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/charts/miner-address-distribution").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["fromDate"], "2024-01-09", "{json}");
    assert_eq!(json["toDate"], "2024-01-15", "{json}");
    assert_eq!(json["totalBlocks"], 10, "{json}");
}

#[tokio::test]
async fn test_secondary_issuance_chart_exposes_exact_verifier_values() {
    const CKB: i128 = 100_000_000;

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let snapshot = DaoDailySnapshot {
        date: "2026-08-07".to_string(),
        total_deposited: 0,
        depositors_count: 0,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 10 * CKB,
        cumulative_deposit_amount: 0,
        total_issuance: 0,
        secondary_pool: 50 * CKB,
        occupied_capacity: 0,
        cum_miner_secondary: 40 * CKB,
        cum_dao_compensation: 30 * CKB,
        cum_treasury: 30 * CKB,
        unclaimed_compensation: 20 * CKB,
        frozen_phase1_compensation: 5 * CKB,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20260807",
    );
    core_store
        .put_cf(
            core_store.cf_stats_dao(),
            &snapshot_key,
            &bincode::serialize(&snapshot).unwrap(),
        )
        .unwrap();
    seed_tip_block_on_utc8_day(&core_store, 1, "2026-08-08");

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/charts/secondary-issuance")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["protocolTotalShannons"], "10000000000");
    assert_eq!(json["data"][0]["phase1CompensationShannons"], "500000000");
    assert_eq!(json["data"][0]["values"]["mining"], "40");
    assert_eq!(json["data"][0]["values"]["compensation"], "30");
    assert_eq!(json["data"][0]["values"]["burnt"], "30");
}

#[tokio::test]
async fn test_inflation_rate_uses_exact_trailing_year_dao_snapshots() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET).unwrap();
    let end_date = chrono::Utc::now().with_timezone(&utc8).date_naive() - chrono::Duration::days(1);
    let incomplete_tip_date = end_date + chrono::Duration::days(1);
    let start_date = end_date - chrono::Duration::days(365);

    let snapshot = |date: chrono::NaiveDate,
                    total_issuance: i128,
                    secondary_pool: i128,
                    claimed_compensation: i128,
                    cum_miner_secondary: i128,
                    cum_dao_compensation: i128,
                    cum_treasury: i128,
                    unclaimed_compensation: i128| DaoDailySnapshot {
        date: date.format("%Y-%m-%d").to_string(),
        total_deposited: 0,
        depositors_count: 0,
        new_deposits: 0,
        withdrawals: 0,
        compensation: claimed_compensation,
        cumulative_deposit_amount: 0,
        total_issuance,
        secondary_pool,
        occupied_capacity: 0,
        cum_miner_secondary,
        cum_dao_compensation,
        cum_treasury,
        unclaimed_compensation,
        frozen_phase1_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };

    // C grows by 10%; exact cumulative secondary issuance
    // (miner + S + claimed) grows by 2%, leaving 8% primary dilution.
    // The chart must use that exact formula rather than summing the stacked
    // components: `cum_dao_compensation` already contains `unclaimed`, which is
    // exactly the part of S that `cum_treasury` excludes.
    let mut rows = Vec::new();
    for day_offset in 0..=365_i64 {
        let scale = i128::from(day_offset);
        let total_issuance = 1_000_000_000 + 100_000_000 * scale / 365;
        let cum_miner = 40_000_000 + 10_000_000 * scale / 365;
        let claimed = 10_000_000 + 5_000_000 * scale / 365;
        let secondary_pool = 50_000_000 + 5_000_000 * scale / 365;
        let unclaimed = 20_000_000 + 10_000_000 * scale / 365;
        let cum_dao = claimed + unclaimed;
        let treasury = secondary_pool - unclaimed;
        rows.push(snapshot(
            start_date + chrono::Duration::days(day_offset),
            total_issuance,
            secondary_pool,
            claimed,
            cum_miner,
            cum_dao,
            treasury,
            unclaimed,
        ));
    }
    rows.push(snapshot(
        incomplete_tip_date,
        1_200_000_000,
        60_000_000,
        20_000_000,
        60_000_000,
        55_000_000,
        25_000_000,
        35_000_000,
    ));

    for row in rows {
        let date_key = row.date.replace('-', "");
        let key = ckbadger_store::keys::encode_stats_key(
            ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
            date_key.as_bytes(),
        );
        core_store
            .put_cf(
                core_store.cf_stats_dao(),
                &key,
                &bincode::serialize(&row).unwrap(),
            )
            .unwrap();
    }
    let tip_timestamp = incomplete_tip_date
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(utc8)
        .single()
        .unwrap()
        .timestamp_millis();
    let mut batch = StoreBatch::new(core_store.as_ref());
    // The only block: the whole median-time window, at noon of the tip day.
    batch.put_block_header(
        0,
        &CachedBlockHeader {
            hash: vec![0x55; 32],
            parent_hash: vec![0x44; 32],
            timestamp: tip_timestamp,
            epoch_number: 0,
            epoch_index: 1,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: tip_timestamp,
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/charts/inflation-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], end_date.format("%Y-%m-%d").to_string());
    assert_eq!(data[0]["value"], "10.0000");
    assert_eq!(data[0]["value2"], "8.0000");
}

#[tokio::test]
async fn test_inflation_rate_forward_fills_testnet_genesis_blockless_days() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET).unwrap();
    let genesis_date = chrono::NaiveDate::from_ymd_opt(2020, 5, 12).unwrap();
    let first_mined_date = chrono::NaiveDate::from_ymd_opt(2020, 5, 22).unwrap();
    let last_complete_date = chrono::NaiveDate::from_ymd_opt(2021, 5, 22).unwrap();
    let incomplete_tip_date = last_complete_date + chrono::Duration::days(1);

    let snapshot = |date: chrono::NaiveDate, total_issuance: i128| DaoDailySnapshot {
        date: date.format("%Y-%m-%d").to_string(),
        total_deposited: 0,
        depositors_count: 0,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 0,
        total_issuance,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };

    let mut rows = vec![snapshot(genesis_date, 1_000_000_000)];
    let mut date = first_mined_date;
    while date <= incomplete_tip_date {
        let elapsed_days = date.signed_duration_since(genesis_date).num_days();
        rows.push(snapshot(
            date,
            1_000_000_000 + i128::from(elapsed_days) * 1_000_000,
        ));
        date += chrono::Duration::days(1);
    }

    for row in rows {
        let date_key = row.date.replace('-', "");
        let key = ckbadger_store::keys::encode_stats_key(
            ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
            date_key.as_bytes(),
        );
        core_store
            .put_cf(
                core_store.cf_stats_dao(),
                &key,
                &bincode::serialize(&row).unwrap(),
            )
            .unwrap();
    }

    let tip_timestamp = incomplete_tip_date
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(utc8)
        .single()
        .unwrap()
        .timestamp_millis();
    let mut batch = StoreBatch::new(core_store.as_ref());
    // Genesis, the first mined block, then 37 one-second-apart headers ending
    // at the tip. Their effective chain times are monotonic, so the tip's
    // `stats_day` marks the current incomplete day for this chart.
    let mut headers = vec![
        (0, 1_589_276_230_000), // testnet genesis: 2020-05-12 17:37:10 UTC+8
        (1, 1_590_137_711_584), // first mined block: 2020-05-22 16:55:11.584 UTC+8
    ];
    headers.extend((2..=38).map(|n| (n, tip_timestamp - (38 - n) * 1_000)));
    for (number, timestamp) in headers {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                parent_hash: vec![number.saturating_sub(1) as u8; 32],
                timestamp,
                epoch_number: 0,
                epoch_index: number as i32,
                epoch_length: 1800,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: timestamp,
            },
        );
    }
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/charts/inflation-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 11);
    assert_eq!(data.first().unwrap()["date"], "2021-05-12");
    assert_eq!(data.last().unwrap()["date"], "2021-05-22");
    assert!(data.iter().all(|point| point["value"] == point["value2"]));
}

#[tokio::test]
async fn test_inflation_rate_rejects_missing_snapshot_on_block_bearing_day() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET).unwrap();
    let first_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
    let missing_date = first_date + chrono::Duration::days(1);
    let next_snapshot_date = missing_date + chrono::Duration::days(1);
    let incomplete_tip_date = next_snapshot_date + chrono::Duration::days(1);

    for (date, total_issuance) in [
        (first_date, 1_000_000_000),
        (next_snapshot_date, 1_001_000_000),
        (incomplete_tip_date, 1_002_000_000),
    ] {
        let row = DaoDailySnapshot {
            date: date.format("%Y-%m-%d").to_string(),
            total_deposited: 0,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unclaimed_compensation: 0,
            frozen_phase1_compensation: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
        };
        let date_key = row.date.replace('-', "");
        let key = ckbadger_store::keys::encode_stats_key(
            ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
            date_key.as_bytes(),
        );
        core_store
            .put_cf(
                core_store.cf_stats_dao(),
                &key,
                &bincode::serialize(&row).unwrap(),
            )
            .unwrap();
    }

    let timestamp = |date: chrono::NaiveDate| {
        date.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(utc8)
            .single()
            .unwrap()
            .timestamp_millis()
    };
    let mut batch = StoreBatch::new(core_store.as_ref());
    for (number, date) in [
        (0, first_date),
        (1, missing_date),
        (2, next_snapshot_date),
        (3, incomplete_tip_date),
    ] {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                parent_hash: vec![number.saturating_sub(1) as u8; 32],
                timestamp: timestamp(date),
                epoch_number: 0,
                epoch_index: number as i32,
                epoch_length: 1800,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: timestamp(date),
            },
        );
    }
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/charts/inflation-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing_date=2026-02-16, first_block=1"));
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
    // A complete header, DAO snapshot and genesis baseline are all required
    // for the asset percentages and Common Knowledge Size.
    seed_knowledge_size_fixture(&store, 519_967_746_700_000_000);

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
                max_supply: None,
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
        json["totalLiveCapacityCkb"].is_string(),
        "totalLiveCapacityCkb should be a string"
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

/// Regression (C3): the capacity breakdown must be a share of TOTAL LIVE
/// CAPACITY (C − S from the tip header's DAO field), not of the snapshot
/// knowledge size. The old denominator counted occupied bytes only while the
/// dao numerator was full deposit capacity (mostly free capacity), producing
/// a self-contradictory 161% DAO share whose violation the `other` clamp
/// silently masked.
#[tokio::test]
async fn test_asset_ecosystem_breakdown_is_share_of_total_live_capacity() {
    let store = test_store();

    // Tip header DAO field with realistic mainnet magnitudes:
    // C = 47.9B CKB total issuance, S = 0.3B CKB unissued secondary
    // ⇒ total live capacity C − S = 47.6B CKB.
    let total_issuance_c: u64 = 4_790_000_000_000_000_000;
    let unissued_secondary_s: u64 = 30_000_000_000_000_000;
    let mut dao_field = vec![0u8; 32];
    dao_field[0..8].copy_from_slice(&total_issuance_c.to_le_bytes());
    dao_field[16..24].copy_from_slice(&unissued_secondary_s.to_le_bytes());

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x66; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_800_000_000_000,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: dao_field,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_800_000_000_000,
        },
    );
    batch.commit().unwrap();

    // DAO snapshot: 8.37B CKB deposited but only 5.2B CKB knowledge size —
    // deposits are mostly free capacity, so deposited > knowledge is the
    // structural norm, and the old knowledge denominator showed dao = 160.96%.
    let snapshot = DaoDailySnapshot {
        date: "2026-07-30".to_string(),
        total_deposited: 837_000_000_000_000_000,
        depositors_count: 100,
        new_deposits: 5,
        withdrawals: 2,
        compensation: 0,
        cumulative_deposit_amount: 0,
        total_issuance: total_issuance_c as i128,
        secondary_pool: unissued_secondary_s as i128,
        occupied_capacity: 520_000_000_000_000_000,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20260730",
    );
    store
        .put_cf(
            store.cf_stats_dao(),
            &snapshot_key,
            &bincode::serialize(&snapshot).unwrap(),
        )
        .unwrap();

    // One token (500 CKB owned capacity) so the tokens share is non-zero.
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
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 10,
            },
        )
        .unwrap();
    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_token_holder(&[0xAA; 32], &[0x01; 32], 500_000);
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

    // The standalone knowledge stat is U − virtual_occupied, so the mainnet
    // baseline must be present.
    seed_genesis_baseline(&store);

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

    // The denominator is exposed alongside the standalone knowledge stat.
    assert_eq!(json["totalLiveCapacityCkb"], "47600000000");
    // 520_000_000_000_000_000 (U) − 504_000_000_000_000_000 (virtual occupied)
    assert_eq!(json["totalKnowledgeSizeCkb"], "160000000");

    let breakdown = json["capacityBreakdown"].as_array().unwrap();
    let pct_of = |category: &str| -> f64 {
        breakdown
            .iter()
            .find(|c| c["category"] == category)
            .unwrap_or_else(|| panic!("missing category {category}"))["percentage"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };

    // dao = 8.37B / 47.6B = 17.58% — a share of live capacity, never >100%.
    let dao_pct = pct_of("dao");
    assert!(
        (15.0..18.0).contains(&dao_pct),
        "dao share of live capacity must be ~17.58%, got {dao_pct}"
    );

    let pct_sum: f64 = ["dao", "tokens", "objects", "other"]
        .iter()
        .map(|c| pct_of(c))
        .sum();
    assert!(
        (99.9..=100.01).contains(&pct_sum),
        "category shares must partition live capacity, got {pct_sum}"
    );
}

// ============================================================
// Exact activity summary: chain-time window and cross-hour address dedup
// ============================================================

#[tokio::test]
async fn test_activity_summary_24h_uses_exact_chain_window_and_deduplicates_addresses() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TxActions};
    use ckbadger_store::StoreBatch;

    let store = test_store();
    let tip_time = 1_700_000_000_000i64;
    let cutoff = tip_time - 86_400_000;
    let mut batch = StoreBatch::new(&store);
    for (number, chain_time, raw_time) in [
        (0, cutoff - 1, cutoff - 1),
        (1, cutoff, cutoff),
        // Raw header time is older, but monotone effective chain time is in-window.
        (2, cutoff + 1, cutoff - 1),
        (3, tip_time, tip_time),
    ] {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                parent_hash: vec![0; 32],
                timestamp: raw_time,
                chain_time_ms: chain_time,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
    }
    for (number, participants) in [
        (0, vec![(0xDD, 300)]),
        (1, vec![(0xCC, 200)]),
        (2, vec![(0xAA, 100)]),
        (3, vec![(0xAA, -50), (0xBB, 25)]),
    ] {
        batch.put_tx_actions(&TxActions {
            tx_hash: vec![number as u8 + 0x10; 32],
            block_hash: vec![number as u8; 32],
            block_number: number,
            tx_index: 1,
            timestamp: tip_time,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: participants
                .into_iter()
                .map(|(hash, ckb_delta)| ParticipantDelta {
                    id: ParticipantId::Lock([hash; 32]),
                    ckb_delta,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: 0,
                    roles: 0,
                })
                .collect(),
        });
    }
    // Cross the 512-row page boundary. Every extra transaction reuses A, so
    // a paginated scan must count transactions while deduplicating addresses.
    for tx_index in 2..=521i32 {
        batch.put_tx_actions(&TxActions {
            tx_hash: vec![tx_index as u8; 32],
            block_hash: vec![2; 32],
            block_number: 2,
            tx_index,
            timestamp: tip_time,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0xAA; 32]),
                ckb_delta: 1,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
        });
    }
    batch.commit().unwrap();
    for hour in 0..24 {
        store
            .put_hourly_activity_stats(
                &format!("20231115{hour:02}"),
                &ckbadger_store::DailyActivityStats {
                    transfer_count: 1,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let old_started = std::time::Instant::now();
    let old_rows = store
        .list_hourly_activity_stats_since("2023111500")
        .unwrap();
    let old_elapsed = old_started.elapsed();
    assert_eq!(old_rows.len(), 24);
    let app = create_router(test_config(store.clone())).await;
    let fresh_started = std::time::Instant::now();
    let (status, json) = get_json(&app, "/stats/activity-summary-24h").await;
    let fresh_elapsed = fresh_started.elapsed();
    let cached_started = std::time::Instant::now();
    let (cached_status, cached) = get_json(&app, "/stats/activity-summary-24h").await;
    let cached_elapsed = cached_started.elapsed();
    assert_eq!(cached_status, StatusCode::OK);
    assert_eq!(cached, json);
    eprintln!(
        "activity24h fixture: old 24-bucket read={}us, exact fresh 524-tx/4-block response={}us, tip-hash cache hit={}us",
        old_elapsed.as_micros(), fresh_elapsed.as_micros(), cached_elapsed.as_micros()
    );
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["transferCount"], 522,
        "all pages in (cutoff,tip] count"
    );
    assert_eq!(
        json["uniqueAddressCount"], 2,
        "same lock across hours counts once"
    );
    assert_eq!(json["activityCount"], 522);
    assert_eq!(json["totalCkbMoved"], "645");
    assert_eq!(json["hoursCovered"], 2);

    // A shallow reorg can replace the same-number tip before the TTL expires.
    // Its new hash must force a fresh window scan and address set.
    let mut new_tip = store.get_block_header(3).unwrap().unwrap();
    new_tip.hash = vec![0xF3; 32];
    let mut new_tx = store.get_tx_actions(3, 1, &[0x13; 32]).unwrap().unwrap();
    new_tx.block_hash = new_tip.hash.clone();
    new_tx.participants.push(ParticipantDelta {
        id: ParticipantId::Lock([0xEE; 32]),
        ckb_delta: 1,
        used_delta: 0,
        item_deltas: vec![],
        tags: 0,
        roles: 0,
    });
    let mut reorg_batch = StoreBatch::new(&store);
    reorg_batch.put_block_header(3, &new_tip);
    reorg_batch.put_tx_actions(&new_tx);
    reorg_batch.commit().unwrap();
    let (status, after_reorg) = get_json(&app, "/stats/activity-summary-24h").await;
    assert_eq!(status, StatusCode::OK, "{after_reorg}");
    assert_eq!(after_reorg["uniqueAddressCount"], 3);
    assert_eq!(after_reorg["totalCkbMoved"], "646");
}

// ============================================================
// B3 regression: transactionsPerDay window normalization and
// tx-stats currentDay natural-day semantics
// ============================================================

#[tokio::test]
async fn test_network_stats_transactions_per_day_matches_window_rate() {
    // transactionsPerDay must be normalized over the same rolling window that
    // tps/transactionsPerMinute use (count / window_secs * 86400), not the raw
    // bucket sum: the window spans 23h between bucket-start quantization
    // edges, so the raw sum self-contradicts perMinute by ~4% (and far more
    // while the window is still filling after a rebuild).
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    // Fixed reference: tip block exactly at a UTC hour boundary.
    let tip_secs: i64 = 1_700_337_600; // 2023-11-18T20:00:00Z
    let tip_ms = tip_secs * 1000;

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x42; 32],
            parent_hash: vec![0u8; 32],
            timestamp: tip_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x190d_f964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: tip_ms,
        },
    );
    core_batch.commit().unwrap();

    // 24 hourly buckets: the tip hour plus the 23 before it. The rolling
    // window spans from the oldest bucket start to the reference timestamp:
    // 23h = 82,800s. Total count = 24 * 60 = 1440.
    for i in 0..24i64 {
        let bucket_start = tip_secs - i * 3600;
        core_store
            .put_hourly_stats(
                &chrono::DateTime::from_timestamp(bucket_start, 0)
                    .unwrap()
                    .format("%Y%m%d%H")
                    .to_string(),
                &HourlyStats {
                    hour: bucket_start,
                    blocks_count: 1,
                    transactions_count: 60,
                    cells_created: 0,
                    cells_consumed: 0,
                    capacity_transferred: 0,
                },
            )
            .unwrap();
    }

    let mut config = test_config_with_append_only(core_store, append_only_store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // round(1440 / 82800 * 86400) = round(1502.6) = 1503 — NOT the raw 1440.
    assert_eq!(json["transactionsPerDay"], "1503");

    // Internal consistency: perDay ≈ perMinute × 1440 within perMinute's
    // one-decimal formatting granularity (0.05 × 1440 = 72) plus rounding.
    let per_minute: f64 = json["transactionsPerMinute"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let per_day: f64 = json["transactionsPerDay"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (per_day - per_minute * 1440.0).abs() <= 0.05 * 1440.0 + 0.5,
        "perDay ({}) must be consistent with perMinute ({}) × 1440",
        per_day,
        per_minute
    );
}

#[tokio::test]
async fn test_tx_stats_current_day_reports_natural_day_so_far() {
    // currentDay reports the natural (UTC+8) day so far — the same daily
    // bucket series that dailyData charts — while the rolling normalized
    // per-day rate lives in /statistics/network transactionsPerDay. It must
    // NOT be a third semantic (raw rolling 24h hourly sum).
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let this_hour = now.timestamp() / 3600 * 3600;
    let date_str = ckbadger_common::block_date(now)
        .format("%Y%m%d")
        .to_string();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        // The only block: the whole median-time window.
        0,
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
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: now_ms,
        },
    );
    core_batch.commit().unwrap();

    // An hourly bucket inside the trailing 24h whose sum (77) differs from
    // today's daily bucket (456): currentDay must report the daily bucket.
    core_store
        .put_hourly_stats(
            &chrono::DateTime::from_timestamp(this_hour, 0)
                .unwrap()
                .format("%Y%m%d%H")
                .to_string(),
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
                blocks_count: 10,
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
    assert_eq!(
        json["currentDay"], 456,
        "currentDay must be today's natural-day bucket, not a rolling hourly sum"
    );
}

fn seed_tx_stats_buckets(store: &CkbadgerStore, hours: &[(&str, i32)], days: &[(&str, i32)]) {
    for &(start, txs) in hours {
        let hour = utc_ms(start) / 1000;
        store
            .put_hourly_stats(
                &chrono::DateTime::from_timestamp(hour, 0)
                    .unwrap()
                    .format("%Y%m%d%H")
                    .to_string(),
                &HourlyStats {
                    hour,
                    blocks_count: 1,
                    transactions_count: txs,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    for &(date, txs) in days {
        store
            .put_daily_stats(
                date,
                &DailyStats {
                    blocks_count: 1,
                    transactions_count: txs,
                    ..Default::default()
                },
            )
            .unwrap();
    }
}

/// The `/statistics/tx-stats` contract (see `TxStatsResponse`): every bucket
/// says whether it is `complete`, by canonical monotone chain time. A tip
/// dated 14:59:58Z after a parent at 15:00:02Z cannot reopen the 14:00 hour.
#[tokio::test]
async fn test_tx_stats_marks_buckets_complete_by_chain_time_across_an_hour() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &[
            (0, utc_ms("2026-10-23T14:59:50Z")),
            (1, utc_ms("2026-10-23T15:00:02Z")),
            (2, utc_ms("2026-10-23T14:59:58Z")),
        ],
    );
    seed_tx_stats_buckets(
        &store,
        &[
            ("2026-10-23T13:00:00Z", 710),
            ("2026-10-23T14:00:00Z", 720),
            ("2026-10-23T15:00:00Z", 2),
        ],
        &[("20261022", 16_000), ("20261023", 17_000)],
    );
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/statistics/tx-stats").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        serde_json::json!({
            "currentHour": 2,
            "currentDay": 17000,
            "hourlyData": [
                { "label": "13:00", "value": 710, "complete": true },
                { "label": "14:00", "value": 720, "complete": true },
                { "label": "15:00", "value": 2, "complete": false }
            ],
            "dailyData": [
                { "label": "10/22", "value": 16000, "complete": true },
                { "label": "10/23", "value": 17000, "complete": false }
            ]
        })
    );
}

/// Across midnight UTC+8, the parent's later clock closes 10-23 even when the
/// tip's raw timestamp goes backward.
#[tokio::test]
async fn test_tx_stats_marks_buckets_complete_by_chain_time_across_midnight() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &[
            (0, utc_ms("2026-10-23T15:59:50Z")),
            (1, utc_ms("2026-10-23T16:00:02Z")),
            (2, utc_ms("2026-10-23T15:59:58Z")),
        ],
    );
    seed_tx_stats_buckets(
        &store,
        &[
            ("2026-10-23T14:00:00Z", 720),
            ("2026-10-23T15:00:00Z", 700),
            ("2026-10-23T16:00:00Z", 3),
        ],
        &[("20261022", 16_000), ("20261023", 17_000), ("20261024", 3)],
    );
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/statistics/tx-stats").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        serde_json::json!({
            "currentHour": 3,
            "currentDay": 3,
            "hourlyData": [
                { "label": "14:00", "value": 720, "complete": true },
                { "label": "15:00", "value": 700, "complete": true },
                { "label": "16:00", "value": 3, "complete": false }
            ],
            "dailyData": [
                { "label": "10/22", "value": 16000, "complete": true },
                { "label": "10/23", "value": 17000, "complete": true },
                { "label": "10/24", "value": 3, "complete": false }
            ]
        })
    );
}

// ---------------------------------------------------------------------------
// Daily charts stop at the day before the indexed tip block's day (STATS-013)
// ---------------------------------------------------------------------------

/// Regression: `/stats/daily-activities` served the tip block's partial day
/// as its last point (20261024 on the live stack), so the homepage's daily
/// activity bars ended on a drop. `days=N` is the last N complete days.
#[tokio::test]
async fn test_daily_activities_exclude_the_in_progress_tip_day() {
    let store = test_store();
    for (date, transfers) in [("20240114", 30), ("20240115", 40), ("20240116", 5)] {
        store
            .put_daily_activity_stats(
                date,
                &ckbadger_store::types::DailyActivityStats {
                    transfer_count: transfers,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/stats/daily-activities?days=0").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let days: Vec<(&str, u64)> = json
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["date"].as_str().unwrap(),
                d["transferCount"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(days, vec![("20240114", 30), ("20240115", 40)], "{json}");

    let (status, json) = get_json(&app, "/stats/daily-activities?days=1").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json.as_array().unwrap().len(), 1, "{json}");
    assert_eq!(json[0]["date"], "20240115", "the last complete day: {json}");
}

fn knowledge_size_day(knowledge_size: i128) -> DailyStats {
    DailyStats {
        blocks_count: 1,
        knowledge_size: Some(knowledge_size),
        ..Default::default()
    }
}

#[tokio::test]
async fn knowledge_chart_fills_only_verified_blockless_days() {
    let store = test_store();
    seed_sparse_daily_chart_history(&store);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/charts/knowledge-size").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let expected: Vec<_> = (12..=24)
        .map(|day| {
            let (value, utilization) = if day < 22 {
                ("100.00000001", "11.4025")
            } else {
                ("110.00000002", "12.5000")
            };
            serde_json::json!({
                "date": format!("2020-05-{day:02}"),
                "value": value,
                "value2": utilization,
            })
        })
        .collect();
    assert_eq!(json["data"], serde_json::json!(expected));
}

#[tokio::test]
async fn knowledge_chart_rejects_block_bearing_gaps_including_trailing_days() {
    for (missing_day, blocks, first_block) in [
        (
            "2020-05-15",
            vec![
                (0, 1_589_276_230_000),
                (1, utc_ms("2020-05-15T04:00:00Z")),
                (2, 1_590_137_711_584),
                (3, utc_ms("2020-05-25T04:00:00Z")),
            ],
            1,
        ),
        (
            "2020-05-24",
            vec![
                (0, 1_589_276_230_000),
                (1, 1_590_137_711_584),
                (2, utc_ms("2020-05-24T04:00:00Z")),
                (3, utc_ms("2020-05-25T04:00:00Z")),
            ],
            2,
        ),
    ] {
        let store = test_store();
        seed_sparse_daily_chart_history(&store);
        seed_block_headers_at(&store, &blocks);
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/charts/knowledge-size").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
        let message = json["message"].as_str().unwrap();
        assert!(message.contains(missing_day), "{json}");
        assert!(
            message.contains(&format!("first_block={first_block}")),
            "{json}"
        );
    }
}

#[tokio::test]
async fn turnover_chart_rejects_complete_day_without_knowledge_size() {
    let store = test_store();
    store
        .put_daily_stats(
            "20240115",
            &DailyStats {
                blocks_count: 1,
                used_capacity_consumed: 5,
                knowledge_size: None,
                ..Default::default()
            },
        )
        .unwrap();
    seed_block_headers_at(&store, &[(0, 1_705_363_200_000), (1, 1_705_449_600_000)]);
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(&app, "/charts/capacity-turnover-ratio").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("missing knowledge_size in daily_stats for 20240115"),
        "{body}"
    );
}

/// Regression: the knowledge-size chart judged "today" by the wall clock. An
/// indexer that lags the chain — catching up after a restart — still has the
/// tip block's day only partly indexed after midnight has passed, and that day
/// was charted as complete. The reference is the indexed tip, not the clock.
#[tokio::test]
async fn test_knowledge_size_chart_excludes_a_lagging_tip_day() {
    let store = test_store();
    seed_genesis_baseline(&store);
    store
        .put_daily_stats("20240115", &knowledge_size_day(100_000_000_000))
        .unwrap();
    store
        .put_daily_stats("20240116", &knowledge_size_day(100_100_000_000))
        .unwrap();
    // A complete knowledge-size row must have the matching DAO supply row.
    // The incomplete tip day is deliberately left without one.
    let snapshot = DaoDailySnapshot {
        date: "2024-01-15".to_string(),
        total_deposited: 0,
        depositors_count: 0,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 0,
        total_issuance: 1_000_000_000_000_000_000,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20240115",
    );
    store
        .put_cf(
            store.cf_stats_dao(),
            &snapshot_key,
            &bincode::serialize(&snapshot).unwrap(),
        )
        .unwrap();
    // The tip block is years before the wall-clock day.
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/charts/knowledge-size").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let dates: Vec<&str> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["date"].as_str().unwrap())
        .collect();
    assert_eq!(dates, vec!["2024-01-15"], "{json}");
}

/// The composition chart shares the knowledge-size chart's day rule (both used
/// the wall clock); a lagging tip day must be absent from both.
#[tokio::test]
async fn test_knowledge_composition_chart_excludes_a_lagging_tip_day() {
    let store = test_store();
    store
        .put_daily_stats("20240115", &knowledge_size_day(100_000_000_000))
        .unwrap();
    store
        .put_daily_stats("20240116", &knowledge_size_day(100_100_000_000))
        .unwrap();
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let state = test_app_state(test_config(store));
    // No typed scripts: the whole knowledge size is plain transfer cells.
    state.mem_cache.set(
        ckbadger_api::warmup::CACHE_KEY_SCRIPTS_ALL,
        &Vec::<(Vec<u8>, ckbadger_store::ScriptInfo)>::new(),
        std::time::Duration::from_secs(60),
    );
    let app = router_from_state(state);

    let (status, json) = get_json(&app, "/charts/common-knowledge-composition").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let dates: Vec<&str> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["date"].as_str().unwrap())
        .collect();
    assert_eq!(dates, vec!["2024-01-15"], "{json}");
}

// ---------------------------------------------------------------------------
// Common Knowledge Size: one definition on every surface
// ---------------------------------------------------------------------------

/// Build a DAO daily snapshot whose only interesting field is the DAO header
/// `U` (`occupied_capacity`), plus a tip header so the network/asset handlers
/// have a chain to read.
fn seed_knowledge_size_fixture(store: &Arc<CkbadgerStore>, occupied_capacity: i128) {
    let total_issuance_c: u64 = 4_790_000_000_000_000_000;
    let unissued_secondary_s: u64 = 30_000_000_000_000_000;
    let mut dao_field = vec![0u8; 32];
    dao_field[0..8].copy_from_slice(&total_issuance_c.to_le_bytes());
    dao_field[16..24].copy_from_slice(&unissued_secondary_s.to_le_bytes());

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x77; 32],
            parent_hash: vec![0u8; 32],
            timestamp: chrono::Utc::now().timestamp_millis(),
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: dao_field,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x190df964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: chrono::Utc::now().timestamp_millis(),
        },
    );
    batch.commit().unwrap();

    let snapshot = DaoDailySnapshot {
        date: "2026-07-30".to_string(),
        total_deposited: 837_000_000_000_000_000,
        depositors_count: 100,
        new_deposits: 5,
        withdrawals: 2,
        compensation: 0,
        cumulative_deposit_amount: 0,
        total_issuance: total_issuance_c as i128,
        secondary_pool: unissued_secondary_s as i128,
        occupied_capacity,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20260730",
    );
    store
        .put_cf(
            store.cf_stats_dao(),
            &snapshot_key,
            &bincode::serialize(&snapshot).unwrap(),
        )
        .unwrap();
    seed_genesis_baseline(store);
}

/// Regression: `/statistics/network` must report Common Knowledge Size the way
/// CLAUDE.md and `docs/DAO_CALCULATIONS.md` §8 define it — DAO header `U` minus
/// the network's genesis-derived `virtual_occupied` — the same quantity
/// `/charts/knowledge-size` plots. Raw `U` was 32.6× larger on mainnet.
#[tokio::test]
async fn test_network_stats_knowledge_size_subtracts_genesis_virtual_occupied() {
    let store = test_store();
    // Real mainnet tip U at the time this bug was found.
    seed_knowledge_size_fixture(&store, 519_967_746_700_000_000);

    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // 519_967_746_700_000_000 (U) − 504_000_000_000_000_000 (virtual occupied)
    assert_eq!(
        json["knowledgeSize"], "15967746700000000",
        "hero Knowledge Size must equal the knowledge-size chart's definition"
    );
}

/// Regression: `/statistics/asset-ecosystem` shares the single Common Knowledge
/// Size path with `/statistics/network` and `/charts/knowledge-size`.
#[tokio::test]
async fn test_asset_ecosystem_knowledge_size_subtracts_genesis_virtual_occupied() {
    let store = test_store();
    seed_knowledge_size_fixture(&store, 519_967_746_700_000_000);

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

    assert_eq!(
        json["totalKnowledgeSizeCkb"], "159677467",
        "asset-ecosystem knowledge size must be U − virtual_occupied, in CKB"
    );
}

/// Fail fast (never clamp) when `U` is below the network's virtual occupied
/// capacity: that is a broken snapshot or a wrong baseline, not a zero.
#[tokio::test]
async fn test_network_stats_fails_fast_when_knowledge_size_is_negative() {
    let store = test_store();
    // U below the seeded mainnet virtual_occupied (504e15) is impossible on a
    // synced mainnet store.
    seed_knowledge_size_fixture(&store, 1_000_000_000_000_000);

    let mut config = test_config(store);
    let _rpc = mock_indexed_node_tip(&mut config).await;
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("2026-07-30")
            && message.contains("1000000000000000")
            && message.contains("504000000000000000"),
        "error must name the date, U and virtual_occupied: {json}"
    );
}

// ---------------------------------------------------------------------------
// Daily hash rate: divide the day's work by the time actually spent mining it
// ---------------------------------------------------------------------------

/// Regression: mainnet's genesis day is partial — the chain started 05:09:50
/// UTC+8, so its 598 blocks were mined in 67_712_964 ms, not 86_400_000 ms.
/// The stored `DailyStats.block_time_sum_ms` is that exact span (it sums every
/// `ts(b) − ts(b−1)` gap for blocks on the day), so the hash rate is
/// `Σdifficulty / (block_time_sum_ms / 1000)` = 73_466_099_633_870 H/s — the
/// official explorer's 73_466_099_633.87, which it serves in H/ms. The
/// full-day divisor understated it by 21.6%, and the millisecond divisor
/// served H/ms under an H/s label (STATS-012).
///
/// `DailyStats` is the only place inter-block time is stored; `DailyBlockStats`
/// supplies the difficulty and block count that form the numerator.
#[tokio::test]
async fn test_hash_rate_chart_divides_by_the_day_s_actual_mined_span() {
    let store = test_store();

    store
        .put_daily_block_stats(
            "20191116",
            &DailyBlockStats {
                difficulty_sum: 4_974_607_359_728_662_734,
                block_count: 598,
                total_uncles: 0,
            },
        )
        .unwrap();
    store
        .put_daily_stats(
            "20191116",
            &DailyStats {
                blocks_count: 598,
                block_time_sum_ms: 67_712_964,
                block_time_count: 597,
                ..Default::default()
            },
        )
        .unwrap();
    // A later day so the genesis day is not the excluded incomplete max date.
    store
        .put_daily_block_stats(
            "20191117",
            &DailyBlockStats {
                difficulty_sum: 5_880_000_000_000_000_000,
                block_count: 700,
                total_uncles: 0,
            },
        )
        .unwrap();
    store
        .put_daily_stats(
            "20191117",
            &DailyStats {
                blocks_count: 700,
                block_time_sum_ms: 86_400_000,
                block_time_count: 700,
                ..Default::default()
            },
        )
        .unwrap();
    seed_tip_block_on_utc8_day(&store, 1_298, "2019-11-17");

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hash-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        1,
        "the incomplete latest day is excluded: {json}"
    );
    assert_eq!(data[0]["value"], "73466099633870");
}

/// Regression: testnet's genesis day contains only block 0. Block 1 was mined
/// ten days later, and its gap is correctly attributed to block 1's UTC+8 day,
/// so the genesis day has no mined span and therefore no definable hash rate.
/// The chart must omit that one point without inventing a zero/full-day value,
/// while retaining later complete days that do have an exact span.
#[tokio::test]
async fn test_hash_rate_chart_omits_single_block_testnet_genesis_day() {
    let store = test_store();
    let genesis_timestamp_ms = chrono::DateTime::parse_from_rfc3339("2020-05-12T09:37:10Z")
        .unwrap()
        .timestamp_millis();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &recent_block_header(0, genesis_timestamp_ms));
    batch.commit().unwrap();

    for (date, block_count, block_time_sum_ms, block_time_count) in [
        ("20200512", 1, 0, 0),
        ("20200522", 2, 20_000, 2),
        // The indexed tip block's (incomplete) day: present only so 20200522
        // is chart-eligible.
        ("20200523", 1, 10_000, 1),
    ] {
        store
            .put_daily_block_stats(
                date,
                &DailyBlockStats {
                    difficulty_sum: 1_000_000u128 * block_count as u128,
                    block_count,
                    total_uncles: 0,
                },
            )
            .unwrap();
        store
            .put_daily_stats(
                date,
                &DailyStats {
                    blocks_count: block_count,
                    block_time_sum_ms,
                    block_time_count,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    // Blocks 1..=4 on 2020-05-23; the window with genesis has its median there,
    // so 2020-05-22 is complete. (The tip helper would overwrite block 0.)
    let tip_ms = utc_ms("2020-05-23T04:00:00Z");
    seed_block_headers_at(
        &store,
        &(1..=4)
            .map(|n| (n, tip_ms - (4 - n) * 1_000))
            .collect::<Vec<_>>(),
    );

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/charts/hash-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["data"],
        serde_json::json!([{
            "date": "2020-05-22",
            // 2 blocks of difficulty 1_000_000 over a 20 s span.
            "value": "100000",
            "value2": null
        }]),
        "the undefined genesis day must be absent without hiding later exact points: {json}"
    );
}

/// Fail fast when a day has blocks but no accumulated inter-block time: the
/// divisor is missing, and inventing 86_400_000 ms is exactly the bug above.
#[tokio::test]
async fn test_hash_rate_chart_fails_fast_without_block_time_sum() {
    let store = test_store();

    store
        .put_daily_block_stats(
            "20260101",
            &DailyBlockStats {
                difficulty_sum: 100_000_000,
                block_count: 100,
                total_uncles: 0,
            },
        )
        .unwrap();
    store
        .put_daily_stats(
            "20260101",
            &DailyStats {
                blocks_count: 100,
                block_time_sum_ms: 0,
                block_time_count: 0,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_daily_block_stats(
            "20260102",
            &DailyBlockStats {
                difficulty_sum: 100_000_000,
                block_count: 100,
                total_uncles: 0,
            },
        )
        .unwrap();
    seed_tip_block_on_utc8_day(&store, 200, "2026-01-02");

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hash-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("20260101") && message.contains("block_time_sum_ms"),
        "error must name the date and the missing divisor: {json}"
    );
}

/// Fail fast when the day's `DailyStats` row is missing entirely: both rows are
/// written in the same batch, so a hole is upstream corruption.
#[tokio::test]
async fn test_hash_rate_chart_fails_fast_when_daily_stats_row_missing() {
    let store = test_store();

    store
        .put_daily_block_stats(
            "20260101",
            &DailyBlockStats {
                difficulty_sum: 100_000_000,
                block_count: 100,
                total_uncles: 0,
            },
        )
        .unwrap();
    store
        .put_daily_block_stats(
            "20260102",
            &DailyBlockStats {
                difficulty_sum: 100_000_000,
                block_count: 100,
                total_uncles: 0,
            },
        )
        .unwrap();
    seed_tip_block_on_utc8_day(&store, 200, "2026-01-02");

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hash-rate")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("20260101"),
        "error must name the date whose daily stats row is missing: {json}"
    );
}

/// Persist one reorg-event history record exactly as the indexer writer does.
fn seed_reorg_event(
    store: &Arc<CkbadgerStore>,
    detected_at_ms: i64,
    fork_point: i64,
    depth: i32,
    seq: u8,
) {
    seed_reorg_event_of_kind(
        store,
        detected_at_ms,
        fork_point,
        depth,
        seq,
        ckbadger_store::types::ReorgEventKind::Automatic,
    );
}

fn seed_reorg_event_of_kind(
    store: &Arc<CkbadgerStore>,
    detected_at_ms: i64,
    fork_point: i64,
    depth: i32,
    seq: u8,
    kind: ckbadger_store::types::ReorgEventKind,
) {
    let event = ReorgEvent {
        detected_at: detected_at_ms / 1000,
        kind,
        fork_point,
        fork_point_hash: vec![seq; 32],
        old_tip: fork_point + i64::from(depth),
        old_tip_hash: vec![0x11; 32],
        new_tip: fork_point + i64::from(depth) + 1,
        new_tip_hash: vec![0x22; 32],
        depth,
        orphaned_blocks: i64::from(depth),
        orphaned_txs: i64::from(depth) * 2,
        reverted_by_event: None,
        reverses_events: Vec::new(),
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(detected_at_ms, &[seq; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&event).unwrap(),
        )
        .unwrap();
}

/// Every automatic reorg is persisted under a `reorg:<ts_ms>:<uuid>` history
/// key. `/forks` reporting `total: 0` minutes after "Reorg completed" is the
/// endpoint lying about what the store holds.
#[tokio::test]
async fn test_forks_lists_persisted_reorg_events() {
    let store = test_store();
    seed_reorg_event(&store, 1_754_000_000_000, 17_100, 3, 0xA1);
    seed_reorg_event(&store, 1_754_000_600_000, 17_140, 5, 0xB2);

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/forks")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2, "persisted reorg history must be listed");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    // Newest first.
    assert_eq!(data[0]["forkPointNumber"], 17_140);
    assert_eq!(data[0]["depth"], 5);
    assert_eq!(data[1]["forkPointNumber"], 17_100);
}

/// A shallow reorg is resolved automatically, so `deepFork.detected` stays
/// false — but it did happen and `/forks/recent` must say so.
#[tokio::test]
async fn test_forks_recent_surfaces_persisted_shallow_reorg() {
    let store = test_store();
    let detected_at_ms = chrono::Utc::now().timestamp_millis() - 60_000;
    seed_reorg_event(&store, detected_at_ms, 17_200, 4, 0xC3);

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/forks/recent")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["hasRecentReorg"], true);
    assert_eq!(json["reorg"]["forkPointNumber"], 17_200);
    assert_eq!(json["reorg"]["depth"], 4);
    assert_eq!(
        json["deepFork"]["detected"], false,
        "a shallow reorg must not be reported as a deep fork"
    );
}

// ---------------------------------------------------------------------------
// CKBytes allocation: circulation split into exact, non-overlapping parts
// ---------------------------------------------------------------------------

fn dao_field(c: u64, s: u64, u: u64) -> Vec<u8> {
    let mut dao = vec![0u8; 32];
    dao[0..8].copy_from_slice(&c.to_le_bytes());
    dao[16..24].copy_from_slice(&s.to_le_bytes());
    dao[24..32].copy_from_slice(&u.to_le_bytes());
    dao
}

/// DAO header C, S and U of the allocation fixture's blocks 10 and 11.
const BLOCK_10_CSU: (i128, i128, i128) = (
    4_790_000_000_000_000_000,
    30_000_000_000_000_000,
    519_967_746_700_000_000,
);
const BLOCK_11_CSU: (i128, i128, i128) = (
    4_790_000_001_000_000_000,
    30_000_000_100_000_000,
    519_967_746_800_000_000,
);

/// The DAO aggregate as the indexer writes it at `tip_block_number` (10 or
/// 11), carrying that block's C, S and U.
fn seed_dao_latest(
    store: &CkbadgerStore,
    tip_block_number: i64,
    dao_capacity: i128,
    dao_free: i128,
) {
    let (tip_total_issuance, tip_secondary_pool, tip_occupied_capacity) = match tip_block_number {
        10 => BLOCK_10_CSU,
        11 => BLOCK_11_CSU,
        other => panic!("no C/S/U fixture for block {other}"),
    };
    let latest = ckbadger_store::DaoLatestStatistics {
        tip_block_number,
        total_deposited: dao_capacity,
        total_depositors: 1,
        active_deposits: 1,
        deposit_cells_count: 1,
        withdrawal_request_cells_count: 0,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: Some("1 days".to_string()),
        estimated_apc: "2.00".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
        pending_withdrawal_capacity: 0,
        dao_capacity,
        dao_free_capacity: dao_free,
        tip_total_issuance,
        tip_secondary_pool,
        tip_occupied_capacity,
        rolling_24h: None,
    };
    store
        .put_stats_key(
            &ckbadger_store::keys::encode_stats_key(
                ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
                b"latest",
            ),
            &bincode::serialize(&latest).unwrap(),
        )
        .unwrap();
}

/// Seed blocks 10 and 11 (the sync tip) with different DAO fields; block 10
/// carries mainnet-shaped C, S and U.
fn seed_allocation_headers(store: &Arc<CkbadgerStore>) {
    let mut batch = StoreBatch::new(store.as_ref());
    let mut header = recent_block_header(10, 1_700_000_000_000);
    header.dao = dao_field(
        4_790_000_000_000_000_000,
        30_000_000_000_000_000,
        519_967_746_700_000_000,
    );
    batch.put_block_header(10, &header);
    let mut later = recent_block_header(11, 1_700_000_010_000);
    later.dao = dao_field(
        4_790_000_001_000_000_000,
        30_000_000_100_000_000,
        519_967_746_800_000_000,
    );
    batch.put_block_header(11, &later);
    // Block 11's 10-second gap was persisted by the indexer. Rollback must
    // subtract the actual histogram row instead of inventing missing state.
    batch.put_stats(
        &ckbadger_store::keys::encode_stats_key(
            ckbadger_store::keys::STATS_PREFIX_BLOCK_TIME_DIST,
            &10i32.to_be_bytes(),
        ),
        &1i32.to_le_bytes(),
    );
    batch.commit().unwrap();
}

/// BE-4: the homepage CKBytes bar must partition circulation exactly.
/// Knowledge (`U − virtual_occupied`) already holds every live cell's occupied
/// capacity — the DAO cells' included — so the DAO part is the DAO cells'
/// FREE capacity, and Free is the rest. Every term is read at ONE block: the
/// block the indexer computed the DAO aggregate at (10 here, while the tip is
/// already 11), so the three parts always sum to the circulation they split.
#[tokio::test]
async fn test_ckbytes_allocation_partitions_circulation_at_one_block() {
    let store = test_store();
    seed_genesis_baseline(&store);
    seed_allocation_headers(&store);
    // 22,378 live DAO cells of 102 CKB occupied each.
    seed_dao_latest(
        &store,
        10,
        812_889_561_643_799_700,
        812_889_561_643_799_700 - 22_378 * 102_00000000,
    );
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/statistics/ckbytes-allocation").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        serde_json::json!({
            "blockNumber": 10,
            // C − genesis burnt − S
            "circulating": "3920000000000000000",
            // U − genesis virtual_occupied
            "knowledge": "15967746700000000",
            "daoFree": "812661306043799700",
            // circulating − knowledge − daoFree
            "free": "3091370947256200300",
            "daoCapacity": "812889561643799700"
        })
    );
}

/// The parts can never exceed the whole: a DAO free capacity larger than
/// what circulation leaves after Knowledge is corruption, not a zero Free.
#[tokio::test]
async fn test_ckbytes_allocation_fails_fast_when_the_parts_exceed_circulation() {
    let store = test_store();
    seed_genesis_baseline(&store);
    seed_allocation_headers(&store);
    seed_dao_latest(
        &store,
        10,
        4_000_000_000_000_000_000,
        3_910_000_000_000_000_000,
    );
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/statistics/ckbytes-allocation").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("block 10")
            && message.contains("3920000000000000000")
            && message.contains("3910000000000000000"),
        "error must name the block and the terms: {json}"
    );
}

/// The DAO aggregate is indexer-written: absent before the first block it is
/// `initializing`; absent at a synced tip it is a 500 (API-006 semantics).
#[tokio::test]
async fn test_ckbytes_allocation_needs_the_dao_aggregate() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let app = create_router(test_config(store.clone())).await;
    let (status, json) = get_json(&app, "/statistics/ckbytes-allocation").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");

    seed_allocation_headers(&store);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/statistics/ckbytes-allocation").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("dao_latest_stats"),
        "{json}"
    );
}

/// Regression (STATS-015, review A): rollback deletes the headers above the
/// fork point but deliberately keeps the DAO aggregate row until the indexer
/// refreshes it (API-006). The allocation read C, S and U from the header of
/// the aggregate's block, so after every reorg it answered 500 ("no stored
/// header") until the refresh landed. It must answer from the aggregate alone:
/// the pre-rollback split, labelled with the block it describes.
#[tokio::test]
async fn test_ckbytes_allocation_answers_right_after_a_rollback() {
    let store = test_store();
    seed_genesis_baseline(&store);
    seed_allocation_headers(&store);
    // The aggregate was computed at block 11, the tip before the reorg.
    seed_dao_latest(&store, 11, 812_889_561_643_799_700, 812_661_306_043_799_700);
    store.rollback_to_block(10).unwrap();
    assert!(store.get_block_header(11).unwrap().is_none());
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/statistics/ckbytes-allocation").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["blockNumber"], 11, "{json}");
    // Block 11's C − burnt − S and U − virtual_occupied.
    assert_eq!(json["circulating"], "3920000000900000000", "{json}");
    assert_eq!(json["knowledge"], "15967746800000000", "{json}");
}

#[tokio::test]
async fn redesign_fork_detail_fails_when_node_evidence_is_unavailable() {
    let store = test_store();
    let id = 1_754_000_000_000;
    seed_reorg_event(&store, id, 17_100, 3, 0xA1);
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body["message"].as_str().unwrap().contains("CKB RocksDB"));
}

#[tokio::test]
async fn redesign_fork_detail_reads_retained_orphan_blocks_and_transactions() {
    use ckb_types::{
        core::{BlockBuilder, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let parent = [0x33; 32];
    let tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(123456u64.pack())
                .build(),
        )
        .output_data(Vec::<u8>::new().pack())
        .build();
    let tx_hash = tx.hash();
    let block = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(ckb_types::core::EpochNumberWithFraction::new(1, 0, 1000).pack())
        .parent_hash(packed::Byte32::new(parent))
        .timestamp(1700000000000u64.pack())
        .transaction(tx)
        .build();
    let block_hash = block.hash();
    let adopted11 = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(ckb_types::core::EpochNumberWithFraction::new(1, 0, 1000).pack())
        .parent_hash(packed::Byte32::new(parent))
        .timestamp(1700000000100u64.pack())
        .build();
    let adopted12 = BlockBuilder::default()
        .number(12u64.pack())
        .epoch(ckb_types::core::EpochNumberWithFraction::new(1, 1, 1000).pack())
        .parent_hash(adopted11.hash())
        .timestamp(1700000000200u64.pack())
        .build();
    let adopted_hash = adopted12.hash();
    let chain = seed_ckb_chain(&[block, adopted11, adopted12]);
    let id = 1754000000000;
    let event = ReorgEvent {
        detected_at: id / 1000,
        kind: ckbadger_store::types::ReorgEventKind::Automatic,
        fork_point: 10,
        fork_point_hash: parent.to_vec(),
        old_tip: 11,
        old_tip_hash: block_hash.as_slice().to_vec(),
        new_tip: 12,
        new_tip_hash: adopted_hash.as_slice().to_vec(),
        depth: 1,
        orphaned_blocks: 1,
        orphaned_txs: 1,
        reverted_by_event: None,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&event).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["orphanedBlocks"][0]["hash"],
        format!("0x{}", hex::encode(block_hash.as_slice()))
    );
    assert_eq!(
        json["orphanedTransactions"][0]["hash"],
        format!("0x{}", hex::encode(tx_hash.as_slice()))
    );
    assert_eq!(json["orphanedTransactions"][0]["totalCapacity"], "123456");
}
