mod common;
use common::*;

#[tokio::test]
async fn test_charts_average_block_time_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/average-block-time")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_average_block_time_chart_recomputes_after_initial_empty_response() {
    let store = test_store();
    let config = test_config(store.clone());
    let app = create_router(config).await;

    let first_request = Request::builder()
        .uri("/api/v1/charts/average-block-time")
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

    store
        .put_daily_stats(
            "20240115",
            &DailyStats {
                block_time_sum_ms: 12_000,
                block_time_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let second_request = Request::builder()
        .uri("/api/v1/charts/average-block-time")
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
    assert_eq!(second_data[0]["date"], "20240115");
    assert_eq!(second_data[0]["value"], "12.00");
}

#[tokio::test]
async fn test_new_capacity_charts_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/charts/capacity-turnover-ratio",
        "/api/v1/charts/cell-size-distribution",
        "/api/v1/charts/address-cohort-retention",
        "/api/v1/charts/most-utilized-scripts",
        "/api/v1/charts/most-utilized-assets",
    ] {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "uri={uri}");
    }
}

#[tokio::test]
async fn test_removed_cell_age_chart_routes_return_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/charts/cell-age-vs-occupied-capacity",
        "/api/v1/charts/cell-age-vs-used-capacity",
    ] {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri={uri}");
    }
}

#[tokio::test]
async fn test_most_utilized_scripts_chart_ranks_by_used_and_capacity() {
    let store = test_store();

    let code_hash_a1 = vec![0x11; 32];
    let code_hash_a2 = vec![0x12; 32];
    let code_hash_b = vec![0x21; 32];
    let code_hash_unknown = vec![0x31; 32];

    store
        .put_script_info_direct(
            &code_hash_a1,
            &ScriptInfo {
                code_hash: code_hash_a1.clone(),
                name: Some("Script A".to_string()),
                lock_cells_count: 10,
                lock_live_cells_count: 8,
                lock_owned_capacity_sum: 500,
                lock_owned_knowledge_sum: 300,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_a2,
            &ScriptInfo {
                code_hash: code_hash_a2.clone(),
                name: Some("Script A".to_string()),
                type_cells_count: 6,
                type_live_cells_count: 5,
                type_owned_capacity_sum: 700,
                type_owned_knowledge_sum: 500,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_b,
            &ScriptInfo {
                code_hash: code_hash_b.clone(),
                name: Some("Script B".to_string()),
                lock_cells_count: 9,
                lock_live_cells_count: 7,
                lock_owned_capacity_sum: 800,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_unknown,
            &ScriptInfo {
                code_hash: code_hash_unknown.clone(),
                name: None,
                lock_cells_count: 4,
                lock_live_cells_count: 4,
                lock_owned_capacity_sum: 600,
                lock_owned_knowledge_sum: 550,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a1,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 500,
                owned_knowledge_delta: 300,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a2,
            true,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 700,
                owned_knowledge_delta: 500,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 800,
                owned_knowledge_delta: 200,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_unknown,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 600,
                owned_knowledge_delta: 550,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/most-utilized-scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["title"], "Scripts Used & Total CKBytes");
    let used_share = &json["usedShare"];
    let used_series = used_share["series"].as_array().unwrap();
    assert_eq!(used_series.len(), 4);
    assert_eq!(used_series[0]["label"], "Script A");
    assert_eq!(
        used_series[1]["label"],
        format!("0x{}", hex::encode(&code_hash_unknown))
    );
    assert_eq!(used_series[2]["label"], "Script B");
    assert_eq!(used_series[3]["label"], "Others");

    let used_data = used_share["data"].as_array().unwrap();
    assert_eq!(used_data.len(), 1);
    assert_eq!(used_data[0]["date"], "2024-01-01");
    assert_eq!(used_data[0]["values"]["top0"], "800");
    assert_eq!(used_data[0]["values"]["top1"], "550");
    assert_eq!(used_data[0]["values"]["top2"], "200");
    assert_eq!(used_data[0]["values"]["others"], "0");

    let capacity_share = &json["capacityShare"];
    let capacity_series = capacity_share["series"].as_array().unwrap();
    assert_eq!(capacity_series[0]["label"], "Script A");
    assert_eq!(capacity_series[1]["label"], "Script B");
    assert_eq!(
        capacity_series[2]["label"],
        format!("0x{}", hex::encode(&code_hash_unknown))
    );
    assert_eq!(capacity_series[3]["label"], "Others");

    let capacity_data = capacity_share["data"].as_array().unwrap();
    assert_eq!(capacity_data[0]["values"]["top0"], "1200");
    assert_eq!(capacity_data[0]["values"]["top1"], "800");
    assert_eq!(capacity_data[0]["values"]["top2"], "600");
    assert_eq!(capacity_data[0]["values"]["others"], "0");
}

#[tokio::test]
async fn test_most_utilized_assets_chart_ranks_mixed_asset_types() {
    let store = test_store();

    let token_a = vec![0x41; 32];
    let token_b = vec![0x42; 32];
    let cluster_id = vec![0x51; 32];
    let nft_collection_id = vec![0x61; 32];

    store
        .put_token_direct(
            &token_a,
            &TokenInfo {
                type_code_hash: vec![0x01; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("Token A".to_string()),
                symbol: Some("A".to_string()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_a,
            20240101,
            &TokenDailyDelta {
                owned_capacity_delta: 300,
                owned_knowledge_delta: 250,
            },
        )
        .unwrap();

    store
        .put_token_direct(
            &token_b,
            &TokenInfo {
                type_code_hash: vec![0x03; 32],
                hash_type: 1,
                type_args: vec![0x04; 20],
                standard: "xudt".to_string(),
                name: Some("Token B".to_string()),
                symbol: Some("B".to_string()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_b,
            20240101,
            &TokenDailyDelta {
                owned_capacity_delta: 900,
                owned_knowledge_delta: 100,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: Some("DOB Cluster".to_string()),
            description: None,
            total_count: 5,
            live_count: 5,
            owner_count: 3,
            ..Default::default()
        },
    );
    batch.put_mnft_collection_aggregate(
        &nft_collection_id,
        &MnftCollectionAggregate {
            name: Some("NFT Collection".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 6,
            live_count: 6,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240101,
            &ClusterDailyDelta {
                owned_capacity_delta: 500,
                owned_knowledge_delta: 400,
            },
        )
        .unwrap();
    store
        .put_mnft_daily_delta(
            &nft_collection_id,
            20240101,
            &MnftDailyDelta {
                owned_capacity_delta: 700,
                owned_knowledge_delta: 600,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/most-utilized-assets")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["title"], "Assets Used & Total CKBytes");
    let used_share = &json["usedShare"];
    let used_series = used_share["series"].as_array().unwrap();
    assert_eq!(used_series[0]["label"], "NFT Collection (object)");
    assert_eq!(used_series[1]["label"], "DOB Cluster (object)");
    assert_eq!(used_series[2]["label"], "A (token)");
    assert_eq!(used_series[3]["label"], "B (token)");
    assert_eq!(used_series[4]["label"], "Others");

    let used_data = used_share["data"].as_array().unwrap();
    assert_eq!(used_data[0]["date"], "2024-01-01");
    assert_eq!(used_data[0]["values"]["top0"], "600");
    assert_eq!(used_data[0]["values"]["top1"], "400");
    assert_eq!(used_data[0]["values"]["top2"], "250");
    assert_eq!(used_data[0]["values"]["top3"], "100");
    assert_eq!(used_data[0]["values"]["others"], "0");

    let capacity_share = &json["capacityShare"];
    let capacity_series = capacity_share["series"].as_array().unwrap();
    assert_eq!(capacity_series[0]["label"], "B (token)");
    assert_eq!(capacity_series[1]["label"], "NFT Collection (object)");
    assert_eq!(capacity_series[2]["label"], "DOB Cluster (object)");
    assert_eq!(capacity_series[3]["label"], "A (token)");
    assert_eq!(capacity_series[4]["label"], "Others");

    let capacity_data = capacity_share["data"].as_array().unwrap();
    assert_eq!(capacity_data[0]["values"]["top0"], "900");
    assert_eq!(capacity_data[0]["values"]["top1"], "700");
    assert_eq!(capacity_data[0]["values"]["top2"], "500");
    assert_eq!(capacity_data[0]["values"]["top3"], "300");
    assert_eq!(capacity_data[0]["values"]["others"], "0");
}

#[tokio::test]
async fn test_charts_block_time_distribution_with_data() {
    let store = test_store();

    // Blocks in epoch 0 (complete) + tip in epoch 1 so epoch 0 counts
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, ts_ms, epoch) in [
        (0i64, 0i64, 0i64),
        (1, 1_000, 0),
        (2, 3_000, 0),
        (3, 4_000, 1),
    ] {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                parent_hash: vec![0u8; 32],
                timestamp: ts_ms,
                epoch_number: epoch,
                epoch_index: 0,
                epoch_length: 3,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                cycles: None,
            },
        );
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/block-time-distribution")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 501);

    // Epoch 0 deltas: 0→1 = 1s, 1→2 = 2s
    let point_1s = data.iter().find(|point| point["date"] == "1.0").unwrap();
    let point_2s = data.iter().find(|point| point["date"] == "2.0").unwrap();
    assert_eq!(point_1s["value"], "50.000");
    assert_eq!(point_2s["value"], "50.000");
}
