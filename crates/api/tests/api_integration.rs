use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use ckbadger_api::{create_router, AppConfig};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    ClusterAggregate, ClusterDailyDelta, DobEntry, DobExtra, DobStandard, ScriptDailyDelta,
    ScriptInfo, SporeDailyDelta, TokenDailyDelta, TokenInfo,
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

    let response = app.oneshot(request).await.unwrap();
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

    let response = app.oneshot(request).await.unwrap();
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
