use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use ckbadger_api::{create_router, AppConfig};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    CachedBlockHeader, ClusterAggregate, ClusterDailyDelta, DobEntry, DobExtra, DobStandard,
    LiveCellInfo, NftCollectionAggregate, NftDailyDelta, NftStandard, ScriptDailyDelta, ScriptInfo,
    SporeDailyDelta, TokenDailyDelta, TokenInfo,
};
use ckbadger_store::CkbadgerStore;

fn test_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap())
}

fn test_config(store: Arc<CkbadgerStore>) -> AppConfig {
    AppConfig {
        store,
        redis_url: None,
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_network: "mainnet".to_string(),
        rate_limit_per_second: Some(1000),
        rate_limit_burst: Some(2000),
        start_background_tasks: false,
        ckb_data_path: None,
    }
}

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

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
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

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["blocks"].as_array().unwrap().is_empty());
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

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_search_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/search?q=0x1234")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

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
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_tokens_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/tokens")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

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
async fn test_new_capacity_charts_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/charts/cell-age-vs-occupied-capacity",
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
async fn test_most_utilized_scripts_chart_ranks_by_occupied_and_capacity() {
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
                lock_live_capacity_sum: 500,
                lock_live_occupied_capacity_sum: 300,
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
                type_live_capacity_sum: 700,
                type_live_occupied_capacity_sum: 500,
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
                lock_live_capacity_sum: 800,
                lock_live_occupied_capacity_sum: 200,
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
                lock_live_capacity_sum: 600,
                lock_live_occupied_capacity_sum: 550,
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
                live_capacity_delta: 500,
                live_occupied_capacity_delta: 300,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a2,
            true,
            20240101,
            &ScriptDailyDelta {
                live_capacity_delta: 700,
                live_occupied_capacity_delta: 500,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240101,
            &ScriptDailyDelta {
                live_capacity_delta: 800,
                live_occupied_capacity_delta: 200,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_unknown,
            false,
            20240101,
            &ScriptDailyDelta {
                live_capacity_delta: 600,
                live_occupied_capacity_delta: 550,
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

    assert_eq!(json["title"], "Most Utilized Scripts");
    let occupied_share = &json["occupiedShare"];
    let occupied_series = occupied_share["series"].as_array().unwrap();
    assert_eq!(occupied_series.len(), 4);
    assert_eq!(occupied_series[0]["label"], "Script A");
    assert_eq!(
        occupied_series[1]["label"],
        format!("0x{}", hex::encode(&code_hash_unknown))
    );
    assert_eq!(occupied_series[2]["label"], "Script B");
    assert_eq!(occupied_series[3]["label"], "Others");

    let occupied_data = occupied_share["data"].as_array().unwrap();
    assert_eq!(occupied_data.len(), 1);
    assert_eq!(occupied_data[0]["date"], "2024-01-01");
    assert_eq!(occupied_data[0]["values"]["top0"], "800");
    assert_eq!(occupied_data[0]["values"]["top1"], "550");
    assert_eq!(occupied_data[0]["values"]["top2"], "200");
    assert_eq!(occupied_data[0]["values"]["others"], "0");

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
                total_supply: Some(1000),
                holders_count: 10,
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
                live_capacity_delta: 300,
                live_occupied_capacity_delta: 250,
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
                total_supply: Some(1000),
                holders_count: 11,
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
                live_capacity_delta: 900,
                live_occupied_capacity_delta: 100,
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
        },
    );
    batch.put_nft_collection_aggregate(
        &nft_collection_id,
        &NftCollectionAggregate {
            name: Some("NFT Collection".to_string()),
            standard: NftStandard::MnftClass,
            total_count: 6,
            live_count: 6,
        },
    );
    batch.commit().unwrap();

    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240101,
            &ClusterDailyDelta {
                live_capacity_delta: 500,
                live_occupied_capacity_delta: 400,
            },
        )
        .unwrap();
    store
        .put_nft_daily_delta(
            &nft_collection_id,
            20240101,
            &NftDailyDelta {
                live_capacity_delta: 700,
                live_occupied_capacity_delta: 600,
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

    assert_eq!(json["title"], "Most Utilized Assets");
    let occupied_share = &json["occupiedShare"];
    let occupied_series = occupied_share["series"].as_array().unwrap();
    assert_eq!(occupied_series[0]["label"], "NFT Collection (nft)");
    assert_eq!(occupied_series[1]["label"], "DOB Cluster (dob)");
    assert_eq!(occupied_series[2]["label"], "A (token)");
    assert_eq!(occupied_series[3]["label"], "B (token)");
    assert_eq!(occupied_series[4]["label"], "Others");

    let occupied_data = occupied_share["data"].as_array().unwrap();
    assert_eq!(occupied_data[0]["date"], "2024-01-01");
    assert_eq!(occupied_data[0]["values"]["top0"], "600");
    assert_eq!(occupied_data[0]["values"]["top1"], "400");
    assert_eq!(occupied_data[0]["values"]["top2"], "250");
    assert_eq!(occupied_data[0]["values"]["top3"], "100");
    assert_eq!(occupied_data[0]["values"]["others"], "0");

    let capacity_share = &json["capacityShare"];
    let capacity_series = capacity_share["series"].as_array().unwrap();
    assert_eq!(capacity_series[0]["label"], "B (token)");
    assert_eq!(capacity_series[1]["label"], "NFT Collection (nft)");
    assert_eq!(capacity_series[2]["label"], "DOB Cluster (dob)");
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

    let mut batch = StoreBatch::new(store.as_ref());
    for (number, ts_ms) in [(0i64, 0i64), (1, 1_000), (2, 3_000)] {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                timestamp: ts_ms,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
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

    let point_1s = data.iter().find(|point| point["date"] == "1.0").unwrap();
    let point_2s = data.iter().find(|point| point["date"] == "2.0").unwrap();
    assert_eq!(point_1s["value"], "50.000");
    assert_eq!(point_2s["value"], "50.000");
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
async fn test_transaction_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let hash = "0x".to_string() + &"ab".repeat(32);
    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{}", hash))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_scripts_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_scripts_list_supports_cursor_pagination() {
    let store = test_store();

    for (code_byte, name) in [
        (0x01u8, "A_SCRIPT"),
        (0x02u8, "B_SCRIPT"),
        (0x03u8, "C_SCRIPT"),
    ] {
        let code_hash = vec![code_byte; 32];
        store
            .put_script_info_direct(
                &code_hash,
                &ScriptInfo {
                    code_hash: code_hash.clone(),
                    hash_type: 1,
                    name: Some(name.to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page1 = json["data"].as_array().unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0]["name"], "A_SCRIPT");
    assert_eq!(page1[1]["name"], "B_SCRIPT");
    assert_eq!(page1[0]["liveCapacitySum"], "0");
    assert_eq!(page1[0]["liveOccupiedCapacitySum"], "0");
    assert_eq!(json["total"], 3);
    assert_eq!(json["limit"], 2);
    assert_eq!(json["hasMore"], true);
    assert_eq!(json["nextCursor"], "2");

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&cursor=2")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page2 = json["data"].as_array().unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0]["name"], "C_SCRIPT");
    assert_eq!(json["total"], 3);
    assert_eq!(json["limit"], 2);
    assert_eq!(json["hasMore"], false);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_scripts_list_sorts_before_cursor_pagination() {
    let store = test_store();

    for (code_byte, name, live_capacity_sum) in [
        (0x01u8, "A_SCRIPT", 10i64),
        (0x02u8, "B_SCRIPT", 30i64),
        (0x03u8, "C_SCRIPT", 20i64),
    ] {
        let code_hash = vec![code_byte; 32];
        store
            .put_script_info_direct(
                &code_hash,
                &ScriptInfo {
                    code_hash: code_hash.clone(),
                    hash_type: 1,
                    name: Some(name.to_string()),
                    lock_live_capacity_sum: live_capacity_sum,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&sort_key=capacity&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page1 = json["data"].as_array().unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0]["name"], "B_SCRIPT");
    assert_eq!(page1[1]["name"], "C_SCRIPT");
    assert_eq!(page1[0]["liveCapacitySum"], "30");
    assert_eq!(page1[1]["liveCapacitySum"], "20");
    assert_eq!(json["nextCursor"], "2");
    assert_eq!(json["hasMore"], true);

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&cursor=2&sort_key=capacity&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page2 = json["data"].as_array().unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0]["name"], "A_SCRIPT");
    assert_eq!(page2[0]["liveCapacitySum"], "10");
    assert_eq!(json["hasMore"], false);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_scripts_list_keeps_unknown_entries_distinct() {
    let store = test_store();

    for code_byte in [0x11u8, 0x22u8] {
        let code_hash = vec![code_byte; 32];
        store
            .put_script_info_direct(
                &code_hash,
                &ScriptInfo {
                    code_hash: code_hash.clone(),
                    hash_type: 1,
                    name: None,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["total"], 2);
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["name"], "Unknown");
    assert_eq!(data[1]["name"], "Unknown");
    assert_ne!(data[0]["codeHash"], data[1]["codeHash"]);
}

#[tokio::test]
async fn test_get_script_returns_deployments_sorted_by_deployed_at() {
    let store = test_store();
    let name = "SECP256K1_BLAKE160".to_string();

    let older_code_hash = vec![0x11; 32];
    let newer_code_hash = vec![0x22; 32];
    let older_tx_hash = vec![0xaa; 32];
    let newer_tx_hash = vec![0xbb; 32];

    store
        .put_script_info_direct(
            &older_code_hash,
            &ScriptInfo {
                code_hash: older_code_hash.clone(),
                hash_type: 1, // type
                name: Some(name.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &newer_code_hash,
            &ScriptInfo {
                code_hash: newer_code_hash.clone(),
                hash_type: 1, // type
                name: Some(name.clone()),
                ..Default::default()
            },
        )
        .unwrap();

    let older_block = 100i64;
    let newer_block = 200i64;
    let older_timestamp = 1_700_000_000_000i64;
    let newer_timestamp = 1_700_100_000_000i64;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        older_block,
        &CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: older_timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_block_header(
        newer_block,
        &CachedBlockHeader {
            hash: vec![0x02; 32],
            timestamp: newer_timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_cell(
        &older_tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: older_block,
            lock_script_hash: vec![0x10; 32],
            lock_code_hash: vec![0x20; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(older_code_hash.clone()),
            type_code_hash: Some(vec![0x30; 32]),
            data_size: 0,
            occupied_capacity: 61_00000000,
        },
    );
    batch.put_cell_by_type(&older_code_hash, older_block, &older_tx_hash, 0);
    batch.put_cell(
        &newer_tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: newer_block,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x21; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(newer_code_hash.clone()),
            type_code_hash: Some(vec![0x31; 32]),
            data_size: 0,
            occupied_capacity: 61_00000000,
        },
    );
    batch.put_cell_by_type(&newer_code_hash, newer_block, &newer_tx_hash, 1);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json.as_array().unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0]["codeHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&newer_code_hash)))
    );
    assert_eq!(items[0]["deployedAt"], newer_timestamp);
    assert_eq!(
        items[1]["codeHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&older_code_hash)))
    );
    assert_eq!(items[1]["deployedAt"], older_timestamp);
}

#[tokio::test]
async fn test_script_occupation_chart_aggregates_deployments() {
    let store = test_store();

    let code_hash_a = vec![0x11; 32];
    let code_hash_b = vec![0x22; 32];
    let name = "SECP256K1_BLAKE160".to_string();

    store
        .put_script_info_direct(
            &code_hash_a,
            &ScriptInfo {
                code_hash: code_hash_a.clone(),
                name: Some(name.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_b,
            &ScriptInfo {
                code_hash: code_hash_b.clone(),
                name: Some(name.clone()),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .put_script_daily_delta(
            &code_hash_a,
            false,
            20240115,
            &ScriptDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a,
            false,
            20240116,
            &ScriptDailyDelta {
                live_capacity_delta: -20,
                live_occupied_capacity_delta: -10,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240115,
            &ScriptDailyDelta {
                live_capacity_delta: 50,
                live_occupied_capacity_delta: 30,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240116,
            &ScriptDailyDelta {
                live_capacity_delta: 10,
                live_occupied_capacity_delta: 5,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160/charts/occupation")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["title"], "SECP256K1_BLAKE160 Capacity Occupation");
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["occupied"], "90");
    assert_eq!(data[0]["values"]["unoccupied"], "60");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["occupied"], "85");
    assert_eq!(data[1]["values"]["unoccupied"], "55");

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160/charts/occupation?from=2024-01-16&to=2024-01-16")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["occupied"], "85");
    assert_eq!(data[0]["values"]["unoccupied"], "55");
}

#[tokio::test]
async fn test_script_occupation_chart_by_code_hash_with_kind_filter() {
    let store = test_store();
    let code_hash = vec![0x33; 32];
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    store
        .put_script_daily_delta(
            &code_hash,
            false,
            20240115,
            &ScriptDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 40,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash,
            true,
            20240115,
            &ScriptDailyDelta {
                live_capacity_delta: 80,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/charts/occupation?code_hash={}&script_kind=lock",
            code_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["values"]["occupied"], "40");
    assert_eq!(data[0]["values"]["unoccupied"], "60");
}

#[tokio::test]
async fn test_token_occupation_chart_returns_cumulative_series() {
    let store = test_store();
    let type_hash = vec![0x44; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Test Token".to_string()),
                symbol: Some("TEST".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &type_hash,
            20240115,
            &TokenDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &type_hash,
            20240116,
            &TokenDailyDelta {
                live_capacity_delta: -20,
                live_occupied_capacity_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/occupation",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["title"], "TEST Capacity Occupation");
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["occupied"], "60");
    assert_eq!(data[0]["values"]["unoccupied"], "40");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["occupied"], "50");
    assert_eq!(data[1]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/occupation?from=2024-01-16&to=2024-01-16",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["occupied"], "50");
    assert_eq!(data[0]["values"]["unoccupied"], "30");
}

#[tokio::test]
async fn test_token_occupation_chart_rejects_invalid_date_range() {
    let store = test_store();
    let type_hash = vec![0x45; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Test Token".to_string()),
                symbol: Some("TEST".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/occupation?from=2024-01-31&to=2024-01-01",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_spore_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/spore/nfts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_cluster_occupation_chart_and_cluster_capacity_fields() {
    let store = test_store();
    let cluster_id = [0x42u8; 32];
    let cluster_id_hex = format!("0x{}", hex::encode(cluster_id));

    let cluster_entry = DobEntry {
        standard: DobStandard::SporeCluster,
        collection_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("Test Cluster".to_string()),
        description: None,
        is_live: true,
        created_at_block: 123,
        created_at_tx: vec![0x22; 32],
        extra: DobExtra::SporeCluster,
    };
    store.put_spore_direct(&cluster_id, &cluster_entry).unwrap();
    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240115,
            &ClusterDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();
    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240116,
            &ClusterDailyDelta {
                live_capacity_delta: -20,
                live_occupied_capacity_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/{}/charts/occupation",
            cluster_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Test Cluster Capacity Occupation");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["values"]["occupied"], "60");
    assert_eq!(data[0]["values"]["unoccupied"], "40");
    assert_eq!(data[1]["values"]["occupied"], "50");
    assert_eq!(data[1]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/{}/charts/occupation?from=2024-01-16&to=2024-01-16",
            cluster_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["occupied"], "50");
    assert_eq!(data[0]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!("/api/v1/spore/clusters/{}", cluster_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["liveCapacity"], "80");
    assert_eq!(json["liveOccupiedCapacity"], "50");
}

#[tokio::test]
async fn test_spore_occupation_chart_and_spore_capacity_fields() {
    let store = test_store();
    let spore_id = [0x77u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = DobEntry {
        standard: DobStandard::Spore,
        collection_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: DobExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 1024,
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();
    store
        .put_spore_daily_delta(
            &spore_id,
            20240115,
            &SporeDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 61,
            },
        )
        .unwrap();
    store
        .put_spore_daily_delta(
            &spore_id,
            20240116,
            &SporeDailyDelta {
                live_capacity_delta: -20,
                live_occupied_capacity_delta: -11,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/nfts/{}/charts/occupation",
            spore_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Spore Capacity Occupation");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[1]["values"]["occupied"], "50");
    assert_eq!(data[1]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/nfts/{}/charts/occupation?from=2024-01-16&to=2024-01-16",
            spore_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["occupied"], "50");
    assert_eq!(data[0]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!("/api/v1/spore/nfts/{}", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["liveCapacity"], "80");
    assert_eq!(json["liveOccupiedCapacity"], "50");
}

#[tokio::test]
async fn test_assets_dob_uses_cluster_entry_name_when_aggregate_name_missing() {
    let store = test_store();

    let cluster_id = [0x42u8; 32];
    let cluster_entry = DobEntry {
        standard: DobStandard::SporeCluster,
        collection_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("Recovered Cluster Name".to_string()),
        description: Some("desc".to_string()),
        is_live: true,
        created_at_block: 123,
        created_at_tx: vec![0x22; 32],
        extra: DobExtra::SporeCluster,
    };
    store.put_spore_direct(&cluster_id, &cluster_entry).unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: None,
            description: None,
            total_count: 3,
            live_count: 3,
            owner_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["name"], "Recovered Cluster Name");
}

#[tokio::test]
async fn test_assets_nft_collection_occupation_chart_and_capacity_fields() {
    let store = test_store();
    let collection_id = [0x24u8; 24];
    let collection_id_hex = format!("0x{}", hex::encode(collection_id));

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &collection_id,
        &NftCollectionAggregate {
            name: Some("Test NFT Collection".to_string()),
            standard: NftStandard::MnftToken,
            total_count: 100,
            live_count: 60,
        },
    );
    batch.commit().unwrap();

    store
        .put_nft_daily_delta(
            &collection_id,
            20240115,
            &NftDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();
    store
        .put_nft_daily_delta(
            &collection_id,
            20240116,
            &NftDailyDelta {
                live_capacity_delta: -20,
                live_occupied_capacity_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/{}/charts/occupation",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Test NFT Collection Capacity Occupation");
    assert_eq!(json["data"][1]["values"]["occupied"], "50");
    assert_eq!(json["data"][1]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/{}/charts/occupation?from=2024-01-16&to=2024-01-16",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["date"], "2024-01-16");
    assert_eq!(json["data"][0]["values"]["occupied"], "50");
    assert_eq!(json["data"][0]["values"]["unoccupied"], "30");

    let request = Request::builder()
        .uri(format!("/api/v1/assets/nfts/{}", collection_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["liveCapacity"], "80");
    assert_eq!(json["liveOccupiedCapacity"], "50");
}

#[tokio::test]
async fn test_assets_nft_collection_accepts_dotbit_alias() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &collection_id,
        &NftCollectionAggregate {
            name: None,
            standard: NftStandard::DotBit,
            total_count: 200,
            live_count: 120,
        },
    );
    batch.commit().unwrap();

    store
        .put_nft_daily_delta(
            &collection_id,
            20240115,
            &NftDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/charts/occupation")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], ".bit Capacity Occupation");
    assert_eq!(json["data"][0]["values"]["occupied"], "60");
    assert_eq!(json["data"][0]["values"]["unoccupied"], "40");

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "dotbit");
    assert_eq!(json["name"], ".bit");
    assert_eq!(json["liveCapacity"], "100");
    assert_eq!(json["liveOccupiedCapacity"], "60");

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/DOTBIT/charts/occupation")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/%2Ebit")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "dotbit");
    assert_eq!(json["name"], ".bit");
}

#[tokio::test]
async fn test_assets_nft_list_uses_dotbit_display_name_when_aggregate_name_missing() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &collection_id,
        &NftCollectionAggregate {
            name: None,
            standard: NftStandard::DotBit,
            total_count: 20,
            live_count: 12,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"][0]["name"], ".bit");
    assert_eq!(json["data"][0]["standard"], "dotbit");
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
