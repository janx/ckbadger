use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use ckbadger_api::{create_router, AppConfig};
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
