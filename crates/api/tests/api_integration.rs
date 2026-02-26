use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use ckbadger_api::utils::address::compute_script_hash;
use ckbadger_api::{create_router, AppConfig};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    ActivityEntry, AssetAction, AssetChange, CachedBlockHeader, ClusterAggregate,
    ClusterDailyDelta, DobEntry, DobExtra, DobStandard, EpochStats, LiveCellInfo,
    NftCollectionAggregate, NftDailyDelta, NftEntry, NftExtra, NftStandard, ScriptDailyDelta,
    ScriptInfo, SporeDailyDelta, TokenDailyDelta, TokenInfo,
};
use ckbadger_store::CkbadgerStore;

fn test_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap())
}

fn test_config_with_derived(
    store: Arc<CkbadgerStore>,
    derived_store: Arc<CkbadgerStore>,
) -> AppConfig {
    AppConfig {
        derived_store,
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

fn test_config(store: Arc<CkbadgerStore>) -> AppConfig {
    test_config_with_derived(store.clone(), store)
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
    let store = test_store();
    store
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

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        19_000_000,
        &CachedBlockHeader {
            hash: vec![0xaa; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 13_000,
            epoch_index: 100,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

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
async fn test_blocks_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_block_includes_hardfork_activation() {
    let store = test_store();
    store
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

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        8_775_638,
        &CachedBlockHeader {
            hash: vec![0x11; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 5414,
            epoch_index: 7,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/blocks/8775638")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["number"], 8_775_638);
    assert_eq!(json["hardforkActivation"]["id"], "mirana-2021");
    assert_eq!(json["hardforkActivation"]["shortName"], "Mirana");
    assert_eq!(json["hardforkActivation"]["activationEpoch"], 5414);
    assert_eq!(
        json["hardforkActivation"]["resources"][0]["label"],
        "CKB2021"
    );
    assert_eq!(
        json["hardforkActivation"]["resources"][0]["url"],
        "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0037-ckb2021/0037-ckb2021.md"
    );
}

#[tokio::test]
async fn test_blocks_list_includes_hardfork_activation() {
    let store = test_store();
    store
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

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        8_775_639,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 5414,
            epoch_index: 8,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 2,
        },
    );
    batch.put_block_header(
        8_775_638,
        &CachedBlockHeader {
            hash: vec![0x11; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 5414,
            epoch_index: 7,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/blocks?limit=2")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().expect("block rows");
    assert_eq!(rows.len(), 2);

    let activation_row = rows
        .iter()
        .find(|row| row["number"].as_i64() == Some(8_775_638))
        .expect("activation block row");
    assert_eq!(activation_row["hardforkActivation"]["id"], "mirana-2021");
    assert_eq!(
        activation_row["hardforkActivation"]["shortName"],
        serde_json::Value::from("Mirana")
    );
    assert_eq!(
        activation_row["hardforkActivation"]["resources"][0]["label"],
        serde_json::Value::from("CKB2021")
    );

    let normal_row = rows
        .iter()
        .find(|row| row["number"].as_i64() == Some(8_775_639))
        .expect("non-activation block row");
    assert_eq!(normal_row["hardforkActivation"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_get_cell_returns_occupied_capacity_breakdown() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];
    let output_index = 1i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 123,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: Some(vec![0x44; 32]),
            type_code_hash: Some(vec![0x55; 32]),
            type_args: Some(vec![0xaa, 0xbb]),
            data_size: 42,
            occupied_capacity: 138_00000000,
            udt_amount: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/0x{}/{}",
            hex::encode(&tx_hash),
            output_index
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json["occupiedCapacity"],
        serde_json::Value::from(138_00000000i64)
    );
    assert_eq!(json["type"]["args"], serde_json::Value::from("0xaabb"));
    assert_eq!(
        json["occupiedCapacityBreakdown"]["capacityFieldBytes"],
        serde_json::Value::from(8)
    );
    assert_eq!(
        json["occupiedCapacityBreakdown"]["lockScriptBytes"],
        serde_json::Value::from(53)
    );
    assert_eq!(
        json["occupiedCapacityBreakdown"]["typeScriptBytes"],
        serde_json::Value::from(35)
    );
    assert_eq!(
        json["occupiedCapacityBreakdown"]["dataBytes"],
        serde_json::Value::from(42)
    );
    assert_eq!(
        json["occupiedCapacityBreakdown"]["totalBytes"],
        serde_json::Value::from(138)
    );
}

#[tokio::test]
async fn test_dead_cell_exposes_consumer_metadata_in_cell_and_graph() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];
    let output_index = 0i16;
    let consumed_by_tx = vec![0xcd; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_consumed_cell_with_consumer(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 123,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        },
        456,
        Some(&consumed_by_tx),
    );
    // graph route requires creating tx location to exist
    batch.put_tx_hash_map(&tx_hash, 123, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
    let consumed_by_tx_hex = format!("0x{}", hex::encode(&consumed_by_tx));

    let cell_request = Request::builder()
        .uri(format!("/api/v1/cells/{}/{}", tx_hash_hex, output_index))
        .body(Body::empty())
        .unwrap();
    let cell_response = app.clone().oneshot(cell_request).await.unwrap();
    assert_eq!(cell_response.status(), StatusCode::OK);
    let cell_body = cell_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let cell_json: serde_json::Value = serde_json::from_slice(&cell_body).unwrap();
    assert_eq!(cell_json["status"], "dead");
    assert_eq!(cell_json["consumedAtBlock"], serde_json::Value::from(456));
    assert_eq!(
        cell_json["consumedByTx"],
        serde_json::Value::from(consumed_by_tx_hex.clone())
    );

    let graph_request = Request::builder()
        .uri(format!(
            "/api/v1/graph/cell/{}/{}?depth=1",
            tx_hash_hex, output_index
        ))
        .body(Body::empty())
        .unwrap();
    let graph_response = app.oneshot(graph_request).await.unwrap();
    assert_eq!(graph_response.status(), StatusCode::OK);
    let graph_body = graph_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let graph_json: serde_json::Value = serde_json::from_slice(&graph_body).unwrap();

    let links = graph_json["links"].as_array().unwrap();
    assert!(links.iter().any(|link| {
        link["linkType"] == "consumed_by" && link["source"] == format!("cell-{}-{}", tx_hash_hex, 0)
    }));

    let nodes = graph_json["nodes"].as_array().unwrap();
    assert!(nodes
        .iter()
        .any(|node| node["data"]["hash"] == consumed_by_tx_hex));
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
async fn test_search_hash_without_0x_returns_ambiguous_block_and_transaction() {
    let store = test_store();
    let hash = vec![0xaa; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        123,
        &CachedBlockHeader {
            hash: hash.clone(),
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_tx_hash_map(&hash, 123, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/search?q={}", hex::encode(&hash)))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["normalizedQuery"],
        serde_json::Value::from(format!("0x{}", hex::encode(&hash)))
    );
    assert_eq!(json["ambiguous"], serde_json::Value::from(true));
    let result_types: Vec<_> = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["resultType"].as_str())
        .collect();
    assert!(result_types.contains(&"block"));
    assert!(result_types.contains(&"transaction"));
}

#[tokio::test]
async fn test_search_name_matches_script_token_and_cluster_assets() {
    let store = test_store();

    let script_hash = vec![0x31; 32];
    let token_hash = vec![0x32; 32];
    let cluster_id = vec![0x33; 32];

    store
        .put_script_info_direct(
            &script_hash,
            &ScriptInfo {
                code_hash: script_hash.clone(),
                hash_type: 1,
                name: Some("Alpha Lock".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .put_token_direct(
            &token_hash,
            &TokenInfo {
                type_code_hash: vec![0x44; 32],
                hash_type: 1,
                type_args: vec![0x55; 32],
                standard: "xudt".to_string(),
                name: Some("Alpha Token".to_string()),
                symbol: Some("ALPHA".to_string()),
                decimals: Some(8),
                total_supply: Some(1_000_000),
                max_supply: None,
                holders_count: 10,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: Some("Alpha Cluster".to_string()),
            description: None,
            total_count: 10,
            live_count: 8,
            owner_count: 2,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/search?q=alpha")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["results"].as_array().unwrap();
    let expected_script_url = format!("/script/0x{}", hex::encode(&script_hash));
    let expected_token_url = format!("/tokens/0x{}", hex::encode(&token_hash));
    let expected_cluster_url = format!("/clusters/0x{}", hex::encode(&cluster_id));

    assert!(rows.iter().any(|row| {
        row["resultType"].as_str() == Some("script")
            && row["url"].as_str() == Some(expected_script_url.as_str())
    }));
    assert!(rows.iter().any(|row| {
        row["resultType"].as_str() == Some("token")
            && row["url"].as_str() == Some(expected_token_url.as_str())
    }));
    assert!(rows.iter().any(|row| {
        row["resultType"].as_str() == Some("cluster")
            && row["url"].as_str() == Some(expected_cluster_url.as_str())
    }));
}

#[tokio::test]
async fn test_search_cell_prefix_supports_colon_and_hex_output_index() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 123,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/search?q=cell:{}:0x1",
            hex::encode(&tx_hash)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["normalizedQuery"],
        serde_json::Value::from(format!("0x{}-1", hex::encode(&tx_hash)))
    );
    assert_eq!(json["results"][0]["resultType"], "cell");
    assert_eq!(
        json["results"][0]["id"],
        serde_json::Value::from(format!("0x{}-1", hex::encode(&tx_hash)))
    );
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
async fn test_tokens_list_includes_maximum_supply_status() {
    let store = test_store();

    let mut xudt_plain_args = vec![0x11; 32];
    xudt_plain_args.extend_from_slice(&0u32.to_le_bytes());
    let mut xudt_ext_args = vec![0x22; 32];
    xudt_ext_args.extend_from_slice(&1u32.to_le_bytes());

    let fixtures = vec![
        (
            vec![0x01; 32],
            TokenInfo {
                type_code_hash: vec![0xA1; 32],
                hash_type: 1,
                type_args: xudt_plain_args.clone(),
                standard: "xudt".to_string(),
                name: Some("Limited XUDT".to_string()),
                symbol: Some("CAP".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: Some(1000),
                holders_count: 50,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
        (
            vec![0x02; 32],
            TokenInfo {
                type_code_hash: vec![0xA2; 32],
                hash_type: 1,
                type_args: xudt_plain_args,
                standard: "xudt".to_string(),
                name: Some("Plain XUDT".to_string()),
                symbol: Some("PX".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 40,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
        (
            vec![0x03; 32],
            TokenInfo {
                type_code_hash: vec![0xA3; 32],
                hash_type: 1,
                type_args: xudt_ext_args,
                standard: "xudt".to_string(),
                name: Some("Extended XUDT".to_string()),
                symbol: Some("EX".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 30,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
        (
            vec![0x04; 32],
            TokenInfo {
                type_code_hash: vec![0xA4; 32],
                hash_type: 1,
                type_args: vec![0x44; 20],
                standard: "sudt".to_string(),
                name: Some("Plain SUDT".to_string()),
                symbol: Some("SD".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 20,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
    ];

    for (type_hash, info) in fixtures {
        store.put_token_direct(&type_hash, &info).unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/tokens?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();

    let cap = rows
        .iter()
        .find(|row| row["symbol"] == "CAP")
        .expect("CAP token should exist");
    assert_eq!(cap["maximumSupply"], "1000");
    assert_eq!(cap["maximumSupplyStatus"], "limited");

    let px = rows
        .iter()
        .find(|row| row["symbol"] == "PX")
        .expect("PX token should exist");
    assert_eq!(px["maximumSupply"], serde_json::Value::Null);
    assert_eq!(px["maximumSupplyStatus"], "unlimited");

    let ex = rows
        .iter()
        .find(|row| row["symbol"] == "EX")
        .expect("EX token should exist");
    assert_eq!(ex["maximumSupply"], serde_json::Value::Null);
    assert_eq!(ex["maximumSupplyStatus"], "unknown");

    let sd = rows
        .iter()
        .find(|row| row["symbol"] == "SD")
        .expect("SD token should exist");
    assert_eq!(sd["maximumSupply"], serde_json::Value::Null);
    assert_eq!(sd["maximumSupplyStatus"], "unlimited");
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

    assert_eq!(json["title"], "Scripts Occupied & Total CKBytes");
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
                max_supply: None,
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
                max_supply: None,
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

    assert_eq!(json["title"], "Assets Occupied & Total CKBytes");
    let occupied_share = &json["occupiedShare"];
    let occupied_series = occupied_share["series"].as_array().unwrap();
    assert_eq!(occupied_series[0]["label"], "NFT Collection (nft)");
    assert_eq!(occupied_series[1]["label"], "DOB Cluster (nft)");
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
    assert_eq!(capacity_series[2]["label"], "DOB Cluster (nft)");
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
        (0x01u8, "A_SCRIPT", 10i128),
        (0x02u8, "B_SCRIPT", 30i128),
        (0x03u8, "C_SCRIPT", 20i128),
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
async fn test_script_lookup_and_code_cell_resolve_deployment_reference_alias() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let code_cell_tx_hash = vec![0xe2; 32];
    let code_cell_output_index = 1i16;

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(data_hash.clone()),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                lock_live_cells_count: 10,
                lock_live_capacity_sum: 1_000_000_000,
                lock_live_occupied_capacity_sum: 600_000_000,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 123,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_args: Some(vec![]),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        },
    );
    batch.put_cell_by_type(&type_hash, 123, &code_cell_tx_hash, code_cell_output_index);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let data_hash_hex = format!("0x{}", hex::encode(&data_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            data_hash_hex
        )))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&data_hash_hex]["name"], "Default Lock");
    assert_eq!(json[&data_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&data_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&data_hash_hex]["codeCellOutputIndex"], 1);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=data",
            data_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["outputIndex"], 1);
}

#[tokio::test]
async fn test_scripts_list_merges_unknown_reference_into_known_deployment() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(data_hash.clone()),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                name: None,
                ..Default::default()
            },
        )
        .unwrap();

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

    assert_eq!(json["total"], 1);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["name"], "Default Lock");
}

#[tokio::test]
async fn test_cells_by_script_resolves_reference_hash_type_alias() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let tx_hash = vec![0xab; 32];

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(data_hash.clone()),
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                name: None,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 123,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: type_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        },
    );
    batch.put_cell_by_lock_code(&type_hash, 123, &tx_hash, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash=0x{}&hash_type=type&script_kind=lock&limit=20",
            hex::encode(&data_hash)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();

    assert_eq!(json["total"], 1);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

#[tokio::test]
async fn test_cells_by_script_type_request_returns_empty_for_data_only_deployment() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let tx_hash = vec![0xab; 32];

    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 123,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: data_hash.clone(),
            lock_hash_type: 0,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        },
    );
    batch.put_cell_by_lock_code(&data_hash, 123, &tx_hash, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let data_hash_hex = format!("0x{}", hex::encode(&data_hash));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash={}&hash_type=data&script_kind=lock&limit=20",
            data_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash={}&hash_type=type&script_kind=lock&limit=20",
            data_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 0);
    assert_eq!(json["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_script_returns_deployments_sorted_by_deployed_at() {
    let store = test_store();
    let name = "SECP256K1_BLAKE160".to_string();

    let older_code_hash = vec![0x11; 32];
    let newer_code_hash = vec![0x22; 32];
    let older_data_hash = vec![0x33; 32];
    let newer_data_hash = vec![0x44; 32];
    let older_tx_hash = vec![0xaa; 32];
    let newer_tx_hash = vec![0xbb; 32];

    store
        .put_script_info_direct(
            &older_code_hash,
            &ScriptInfo {
                code_hash: older_code_hash.clone(),
                hash_type: 1, // type
                name: Some(name.clone()),
                dep_type_hash: Some(older_code_hash.clone()),
                dep_data_hash: Some(older_data_hash.clone()),
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
                dep_type_hash: Some(newer_code_hash.clone()),
                dep_data_hash: Some(newer_data_hash.clone()),
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
            type_args: Some(vec![]),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
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
            type_args: Some(vec![]),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
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
    assert_eq!(
        items[0]["typeHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&newer_code_hash)))
    );
    assert_eq!(
        items[0]["dataHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&newer_data_hash)))
    );
    assert_eq!(items[0]["deployedAt"], newer_timestamp);
    assert_eq!(
        items[1]["codeHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&older_code_hash)))
    );
    assert_eq!(
        items[1]["typeHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&older_code_hash)))
    );
    assert_eq!(
        items[1]["dataHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&older_data_hash)))
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
            20240117,
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
            20240117,
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
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["occupied"], "90");
    assert_eq!(data[0]["values"]["unoccupied"], "60");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["occupied"], "90");
    assert_eq!(data[1]["values"]["unoccupied"], "60");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["occupied"], "85");
    assert_eq!(data[2]["values"]["unoccupied"], "55");

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
    assert_eq!(data[0]["values"]["occupied"], "90");
    assert_eq!(data[0]["values"]["unoccupied"], "60");
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
async fn test_get_token_includes_maximum_supply() {
    let store = test_store();
    let type_hash = vec![0x77; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Cap Token".to_string()),
                symbol: Some("CAP".to_string()),
                decimals: Some(8),
                total_supply: Some(500_00000000),
                max_supply: Some(100_000_000_000),
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
        .uri(format!("/api/v1/tokens/{}", type_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalSupply"], "50000000000");
    assert_eq!(json["maximumSupply"], "100000000000");
    assert_eq!(json["maximumSupplyStatus"], "limited");
}

#[tokio::test]
async fn test_get_token_maximum_supply_status_without_cap() {
    let store = test_store();

    let sudt_hash = vec![0x71; 32];
    store
        .put_token_direct(
            &sudt_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "sudt".to_string(),
                name: Some("Plain sUDT".to_string()),
                symbol: Some("SUDT".to_string()),
                decimals: Some(8),
                total_supply: Some(123),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let xudt_hash = vec![0x72; 32];
    let mut xudt_type_args_with_extension = vec![0xAA; 32];
    xudt_type_args_with_extension.extend_from_slice(&1u32.to_le_bytes());
    store
        .put_token_direct(
            &xudt_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: xudt_type_args_with_extension,
                standard: "xudt".to_string(),
                name: Some("Extensible Token".to_string()),
                symbol: Some("XUDT".to_string()),
                decimals: Some(8),
                total_supply: Some(456),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let xudt_plain_hash = vec![0x73; 32];
    let mut xudt_plain_type_args = vec![0xBB; 32];
    xudt_plain_type_args.extend_from_slice(&0u32.to_le_bytes());
    store
        .put_token_direct(
            &xudt_plain_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: xudt_plain_type_args,
                standard: "xudt".to_string(),
                name: Some("Plain XUDT".to_string()),
                symbol: Some("PXUDT".to_string()),
                decimals: Some(8),
                total_supply: Some(789),
                max_supply: None,
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

    let sudt_request = Request::builder()
        .uri(format!("/api/v1/tokens/0x{}", hex::encode(&sudt_hash)))
        .body(Body::empty())
        .unwrap();
    let sudt_response = app.clone().oneshot(sudt_request).await.unwrap();
    assert_eq!(sudt_response.status(), StatusCode::OK);
    let sudt_body = sudt_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let sudt_json: serde_json::Value = serde_json::from_slice(&sudt_body).unwrap();
    assert_eq!(sudt_json["maximumSupply"], serde_json::Value::Null);
    assert_eq!(sudt_json["maximumSupplyStatus"], "unlimited");

    let xudt_request = Request::builder()
        .uri(format!("/api/v1/tokens/0x{}", hex::encode(&xudt_hash)))
        .body(Body::empty())
        .unwrap();
    let xudt_response = app.clone().oneshot(xudt_request).await.unwrap();
    assert_eq!(xudt_response.status(), StatusCode::OK);
    let xudt_body = xudt_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let xudt_json: serde_json::Value = serde_json::from_slice(&xudt_body).unwrap();
    assert_eq!(xudt_json["maximumSupply"], serde_json::Value::Null);
    assert_eq!(xudt_json["maximumSupplyStatus"], "unknown");

    let xudt_plain_request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/0x{}",
            hex::encode(&xudt_plain_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let xudt_plain_response = app.oneshot(xudt_plain_request).await.unwrap();
    assert_eq!(xudt_plain_response.status(), StatusCode::OK);
    let xudt_plain_body = xudt_plain_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let xudt_plain_json: serde_json::Value = serde_json::from_slice(&xudt_plain_body).unwrap();
    assert_eq!(xudt_plain_json["maximumSupply"], serde_json::Value::Null);
    assert_eq!(xudt_plain_json["maximumSupplyStatus"], "unlimited");
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
                max_supply: None,
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
            20240117,
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
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["occupied"], "60");
    assert_eq!(data[0]["values"]["unoccupied"], "40");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["occupied"], "60");
    assert_eq!(data[1]["values"]["unoccupied"], "40");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["occupied"], "50");
    assert_eq!(data[2]["values"]["unoccupied"], "30");

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
    assert_eq!(data[0]["values"]["occupied"], "60");
    assert_eq!(data[0]["values"]["unoccupied"], "40");
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
                max_supply: None,
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
            20240117,
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
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["values"]["occupied"], "60");
    assert_eq!(data[0]["values"]["unoccupied"], "40");
    assert_eq!(data[1]["values"]["occupied"], "60");
    assert_eq!(data[1]["values"]["unoccupied"], "40");
    assert_eq!(data[2]["values"]["occupied"], "50");
    assert_eq!(data[2]["values"]["unoccupied"], "30");

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
    assert_eq!(data[0]["values"]["occupied"], "60");
    assert_eq!(data[0]["values"]["unoccupied"], "40");

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
            20240117,
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
    assert_eq!(data.len(), 3);
    assert_eq!(data[1]["values"]["occupied"], "61");
    assert_eq!(data[1]["values"]["unoccupied"], "39");
    assert_eq!(data[2]["values"]["occupied"], "50");
    assert_eq!(data[2]["values"]["unoccupied"], "30");

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
    assert_eq!(data[0]["values"]["occupied"], "61");
    assert_eq!(data[0]["values"]["unoccupied"], "39");

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
async fn test_spore_decode_endpoint_returns_issues_without_ckb_direct_store() {
    let store = test_store();
    let cluster_id = [0x44u8; 32];
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let cluster_entry = DobEntry {
        standard: DobStandard::SporeCluster,
        collection_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("DOB Cluster".to_string()),
        description: Some(
            serde_json::json!({
                "dob": {
                    "ver": 0,
                    "pattern": [
                        {
                            "traitName": "Background",
                            "dobType": "String",
                            "dnaOffset": 0,
                            "dnaLength": 1,
                            "patternType": "options",
                            "traitArgs": ["red", "blue"]
                        }
                    ]
                }
            })
            .to_string(),
        ),
        is_live: true,
        created_at_block: 100,
        created_at_tx: vec![0x22; 32],
        extra: DobExtra::SporeCluster,
    };
    store.put_spore_direct(&cluster_id, &cluster_entry).unwrap();

    let spore_entry = DobEntry {
        standard: DobStandard::Spore,
        collection_id: Some(cluster_id.to_vec()),
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: DobExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 128,
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/nfts/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "dob/0");
    assert_eq!(json["traits"], serde_json::json!([]));
    assert!(json["issues"].as_array().unwrap().iter().any(|issue| issue
        .as_str()
        .is_some_and(|s| s.contains("Failed to load on-chain spore content"))));
}

#[tokio::test]
async fn test_assets_nft_includes_spore_cluster_name_when_aggregate_name_missing() {
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
        .uri("/api/v1/assets?type=nft")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["name"], "Recovered Cluster Name");
    assert_eq!(json["data"][0]["assetType"], "nft");
    assert_eq!(json["data"][0]["standard"], "spore");
}

#[tokio::test]
async fn test_assets_rejects_legacy_dob_type_filter() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_assets_list_supports_standard_filter_for_tokens_and_nfts() {
    let store = test_store();
    let token_xudt = [0x61u8; 32];
    let token_sudt = [0x62u8; 32];
    let spore_cluster_id = [0x71u8; 32];
    let dotbit_collection_id = b"dotbit_collection_______________".to_vec();

    for (type_hash, standard, symbol) in
        [(token_xudt, "xudt", "XUDT"), (token_sudt, "sudt", "SUDT")]
    {
        store
            .put_token_direct(
                &type_hash,
                &TokenInfo {
                    type_code_hash: vec![0xAA; 32],
                    hash_type: 1,
                    type_args: vec![0x01; 20],
                    standard: standard.to_string(),
                    name: Some(format!("{symbol} Token")),
                    symbol: Some(symbol.to_string()),
                    decimals: Some(8),
                    total_supply: Some(1000),
                    max_supply: None,
                    holders_count: 10,
                    first_seen_block: 1,
                    icon_url: None,
                    description: None,
                    transfers_count: 1,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                20240115,
                &TokenDailyDelta {
                    live_capacity_delta: 100,
                    live_occupied_capacity_delta: 50,
                },
            )
            .unwrap();
    }

    store
        .put_spore_direct(
            &spore_cluster_id,
            &DobEntry {
                standard: DobStandard::SporeCluster,
                collection_id: None,
                owner_lock_hash: Some(vec![0x11; 32]),
                name: Some("Spore Filter Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0x22; 32],
                extra: DobExtra::SporeCluster,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &spore_cluster_id,
        &ClusterAggregate {
            name: Some("Spore Filter Cluster".to_string()),
            description: None,
            total_count: 1,
            live_count: 1,
            owner_count: 1,
        },
    );
    batch.put_nft_collection_aggregate(
        &dotbit_collection_id,
        &NftCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: NftStandard::DotBit,
            total_count: 1,
            live_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let token_request = Request::builder()
        .uri("/api/v1/assets?type=token&standard=xudt")
        .body(Body::empty())
        .unwrap();
    let token_response = app.clone().oneshot(token_request).await.unwrap();
    assert_eq!(token_response.status(), StatusCode::OK);
    let token_body = token_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let token_json: serde_json::Value = serde_json::from_slice(&token_body).unwrap();
    assert_eq!(token_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(token_json["data"][0]["standard"], "xudt");
    assert_eq!(token_json["data"][0]["assetType"], "token");

    let nft_request = Request::builder()
        .uri("/api/v1/assets?type=nft&standard=spore")
        .body(Body::empty())
        .unwrap();
    let nft_response = app.oneshot(nft_request).await.unwrap();
    assert_eq!(nft_response.status(), StatusCode::OK);
    let nft_body = nft_response.into_body().collect().await.unwrap().to_bytes();
    let nft_json: serde_json::Value = serde_json::from_slice(&nft_body).unwrap();
    assert_eq!(nft_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(nft_json["data"][0]["standard"], "spore");
    assert_eq!(nft_json["data"][0]["assetType"], "nft");
}

#[tokio::test]
async fn test_assets_list_defaults_to_capacity_sort_and_supports_cursor_pagination() {
    let store = test_store();
    let token_a = [0x11u8; 32];
    let token_b = [0x22u8; 32];

    store
        .put_token_direct(
            &token_a,
            &TokenInfo {
                type_code_hash: vec![0xAA; 32],
                hash_type: 1,
                type_args: vec![0x01; 20],
                standard: "xudt".to_string(),
                name: Some("Alpha Token".to_string()),
                symbol: Some("ALPHA".to_string()),
                decimals: Some(8),
                total_supply: Some(1000),
                max_supply: None,
                holders_count: 10,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 1,
            },
        )
        .unwrap();
    store
        .put_token_direct(
            &token_b,
            &TokenInfo {
                type_code_hash: vec![0xBB; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("Beta Token".to_string()),
                symbol: Some("BETA".to_string()),
                decimals: Some(8),
                total_supply: Some(2000),
                max_supply: None,
                holders_count: 20,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 2,
            },
        )
        .unwrap();

    store
        .put_token_daily_delta(
            &token_a,
            20240115,
            &TokenDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_b,
            20240115,
            &TokenDailyDelta {
                live_capacity_delta: 300,
                live_occupied_capacity_delta: 120,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=token&limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["id"], format!("0x{}", hex::encode(token_b)));
    assert_eq!(json["data"][0]["liveCapacity"], "300");
    assert_eq!(json["data"][0]["liveOccupiedCapacity"], "120");

    let next_cursor = json["nextCursor"].as_str().unwrap();
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets?type=token&limit=1&sort_key=capacity&sort_direction=desc&cursor={next_cursor}"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["id"], format!("0x{}", hex::encode(token_a)));
    assert!(json["nextCursor"].is_null());

    let request = Request::builder()
        .uri("/api/v1/assets?type=token&sort_key=capacity&sort_direction=asc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["id"], format!("0x{}", hex::encode(token_a)));
}

#[tokio::test]
async fn test_assets_list_token_errors_when_daily_deltas_invalid() {
    let store = test_store();
    let healthy_token = [0x31u8; 32];
    let broken_token = [0x32u8; 32];

    for (hash, name, symbol) in [
        (healthy_token, "Healthy Token", "HLT"),
        (broken_token, "Broken Token", "BKT"),
    ] {
        store
            .put_token_direct(
                &hash,
                &TokenInfo {
                    type_code_hash: vec![0xAA; 32],
                    hash_type: 1,
                    type_args: vec![0x01; 20],
                    standard: "xudt".to_string(),
                    name: Some(name.to_string()),
                    symbol: Some(symbol.to_string()),
                    decimals: Some(8),
                    total_supply: Some(1000),
                    max_supply: None,
                    holders_count: 10,
                    first_seen_block: 1,
                    icon_url: None,
                    description: None,
                    transfers_count: 1,
                },
            )
            .unwrap();
    }

    store
        .put_token_daily_delta(
            &healthy_token,
            20240115,
            &TokenDailyDelta {
                live_capacity_delta: 200,
                live_occupied_capacity_delta: 100,
            },
        )
        .unwrap();

    // Broken history: occupied exceeds capacity; API must fail fast instead of masking.
    store
        .put_token_daily_delta(
            &broken_token,
            20240115,
            &TokenDailyDelta {
                live_capacity_delta: 100,
                live_occupied_capacity_delta: 120,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=token&sort_key=capacity&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("invalid token daily deltas"));
    assert!(message.contains(&hex::encode(broken_token)));
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
            20240117,
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
    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][1]["values"]["occupied"], "60");
    assert_eq!(json["data"][1]["values"]["unoccupied"], "40");
    assert_eq!(json["data"][2]["values"]["occupied"], "50");
    assert_eq!(json["data"][2]["values"]["unoccupied"], "30");

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
    assert_eq!(json["data"][0]["values"]["occupied"], "60");
    assert_eq!(json["data"][0]["values"]["unoccupied"], "40");

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
async fn test_assets_nft_collection_items_dotbit_human_readable_and_pagination() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let dotbit_code_hash =
        hex::decode("4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918").unwrap();
    let nft_a = [0x11u8; 20];
    let nft_b = [0x22u8; 20];
    let nft_a_type_hash = compute_script_hash(&dotbit_code_hash, 1, &nft_a);
    let nft_a_tx_hash = vec![0x9au8; 32];
    let nft_a_output_index = 2i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &collection_id,
        &NftCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: NftStandard::DotBit,
            total_count: 2,
            live_count: 1,
        },
    );
    batch.put_nft(
        &nft_a,
        &NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(nft_a.to_vec()),
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            extra: NftExtra::DotBit {
                expired_at: Some(1_800_000_000),
            },
        },
    );
    batch.put_nft(
        &nft_b,
        &NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(nft_b.to_vec()),
            owner_lock_hash: None,
            name: Some("bob.bit".to_string()),
            is_live: false,
            created_at_block: 101,
            extra: NftExtra::DotBit {
                expired_at: Some(1_900_000_000),
            },
        },
    );
    batch.put_nft_by_collection(&collection_id, &nft_a);
    batch.put_nft_by_collection(&collection_id, &nft_b);
    batch.put_cell(
        &nft_a_tx_hash,
        nft_a_output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            created_at_block: 100,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(nft_a_type_hash.clone()),
            type_code_hash: Some(dotbit_code_hash.clone()),
            type_args: Some(nft_a.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
        },
    );
    batch.put_dotbit_account_outpoint(&nft_a_tx_hash, nft_a_output_index, &nft_a);
    batch.put_cell_by_type(&nft_a_type_hash, 100, &nft_a_tx_hash, nft_a_output_index);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/items?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "alice.bit");
    assert_eq!(json["data"][0]["isLive"], true);
    assert_eq!(json["data"][0]["expiredAt"], 1_800_000_000u64);
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&nft_a_tx_hash))
    );
    assert_eq!(json["data"][0]["outputIndex"], nft_a_output_index);
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/dotbit/items?limit=1&cursor={cursor}"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "bob.bit");
    assert_eq!(json["data"][0]["isLive"], false);
    assert_eq!(json["data"][0]["txHash"], serde_json::Value::Null);
    assert_eq!(json["data"][0]["outputIndex"], serde_json::Value::Null);
    assert_eq!(json["nextCursor"], serde_json::Value::Null);

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/items?limit=20&search=alice")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "alice.bit");
    assert!(json.get("total").is_none());

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/items?limit=20&status=live")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "alice.bit");
    assert_eq!(json["data"][0]["isLive"], true);

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/items?limit=20&status=recycled")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "bob.bit");
    assert_eq!(json["data"][0]["isLive"], false);
    assert_eq!(json["data"][0]["txHash"], serde_json::Value::Null);
    assert_eq!(json["data"][0]["outputIndex"], serde_json::Value::Null);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/dotbit/items/0x{}",
            hex::encode(nft_a)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["name"], "alice.bit");
    assert_eq!(json["isLive"], true);
    assert_eq!(json["txHash"], format!("0x{}", hex::encode(&nft_a_tx_hash)));
    assert_eq!(json["outputIndex"], nft_a_output_index);
}

#[tokio::test]
async fn test_assets_nft_collection_items_dotbit_outpoint_fallback_without_index() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let dotbit_code_hash =
        hex::decode("4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918").unwrap();
    let nft_id = [0x66u8; 20];
    let nft_type_hash = compute_script_hash(&dotbit_code_hash, 1, &nft_id);
    let tx_hash = vec![0xabu8; 32];
    let output_index = 3i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &collection_id,
        &NftCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: NftStandard::DotBit,
            total_count: 1,
            live_count: 1,
        },
    );
    batch.put_nft(
        &nft_id,
        &NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(nft_id.to_vec()),
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("fallback.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            extra: NftExtra::DotBit {
                expired_at: Some(1_800_000_000),
            },
        },
    );
    batch.put_nft_by_collection(&collection_id, &nft_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            created_at_block: 100,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(nft_type_hash.clone()),
            type_code_hash: Some(dotbit_code_hash.clone()),
            type_args: Some(nft_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
        },
    );
    batch.put_cell_by_type(&nft_type_hash, 100, &tx_hash, output_index);
    // Intentionally no put_dotbit_account_outpoint(...) to verify fallback path.
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "fallback.bit");
    assert_eq!(json["data"][0]["isLive"], true);
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&tx_hash))
    );
    assert_eq!(json["data"][0]["outputIndex"], output_index);
}

#[tokio::test]
async fn test_assets_nft_collection_items_dotbit_live_missing_outpoint_fails_fast() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let nft_id = [0x67u8; 20];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &collection_id,
        &NftCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: NftStandard::DotBit,
            total_count: 1,
            live_count: 1,
        },
    );
    batch.put_nft(
        &nft_id,
        &NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(nft_id.to_vec()),
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("broken.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            extra: NftExtra::DotBit {
                expired_at: Some(1_800_000_000),
            },
        },
    );
    batch.put_nft_by_collection(&collection_id, &nft_id);
    // Intentionally no outpoint index and no fallback-resolvable live cell.
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/nfts/dotbit/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("live dotbit account missing outpoint index"));
}

#[tokio::test]
async fn test_assets_nft_collection_items_mnft_live_outpoint() {
    let store = test_store();
    let class_id = [0x24u8; 24];
    let issuer_id = [0x13u8; 20];
    let token_id = [0x42u8; 28];
    let tx_hash = vec![0x55u8; 32];
    let output_index = 6i16;
    let collection_id_hex = format!("0x{}", hex::encode(class_id));

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft_collection_aggregate(
        &class_id,
        &NftCollectionAggregate {
            name: Some("Genesis Class".to_string()),
            standard: NftStandard::MnftClass,
            total_count: 1,
            live_count: 1,
        },
    );
    batch.put_nft(
        &class_id,
        &NftEntry {
            standard: NftStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("Genesis Class".to_string()),
            is_live: true,
            created_at_block: 100,
            extra: NftExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 1000,
                issued: 1,
                configure: 7,
            },
        },
    );
    batch.put_nft(
        &token_id,
        &NftEntry {
            standard: NftStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(vec![0x22; 32]),
            name: None,
            is_live: true,
            created_at_block: 101,
            extra: NftExtra::MnftToken {
                token_index: 1,
                characteristic: vec![1, 2, 3, 4, 5, 6, 7, 8],
                configure: 3,
                state: 1,
            },
        },
    );
    batch.put_nft_by_collection(&class_id, &token_id);
    batch.put_mnft_token_outpoint(&tx_hash, output_index, &token_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            created_at_block: 101,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x62; 32]),
            type_args: Some(token_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/{}/items?limit=20",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&tx_hash))
    );
    assert_eq!(json["data"][0]["outputIndex"], output_index);
}

#[tokio::test]
async fn test_assets_nft_item_detail_mnft() {
    let store = test_store();
    let issuer_id = [0x21u8; 20];
    let class_id = [0x31u8; 24];
    let token_id = [0x41u8; 28];
    let tx_hash = vec![0x91u8; 32];
    let output_index = 4i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft(
        &issuer_id,
        &NftEntry {
            standard: NftStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x01; 32]),
            name: Some("Issuer-A".to_string()),
            is_live: true,
            created_at_block: 90,
            extra: NftExtra::MnftIssuer {
                class_count: 2,
                set_count: 3,
                info: Some(br#"{"name":"Issuer-A"}"#.to_vec()),
            },
        },
    );
    batch.put_nft(
        &class_id,
        &NftEntry {
            standard: NftStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x02; 32]),
            name: Some("Class-A".to_string()),
            is_live: true,
            created_at_block: 95,
            extra: NftExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 500,
                issued: 128,
                configure: 9,
            },
        },
    );
    batch.put_nft(
        &token_id,
        &NftEntry {
            standard: NftStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(vec![0x03; 32]),
            name: None,
            is_live: true,
            created_at_block: 120,
            extra: NftExtra::MnftToken {
                token_index: 128,
                characteristic: vec![0xaa; 8],
                configure: 5,
                state: 2,
            },
        },
    );
    batch.put_mnft_token_outpoint(&tx_hash, output_index, &token_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 300_00000000,
            created_at_block: 120,
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x32; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x33; 32]),
            type_code_hash: Some(vec![0x34; 32]),
            type_args: Some(token_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/items/0x{}",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["nftId"], format!("0x{}", hex::encode(token_id)));
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["tokenIndex"], 128);
    assert_eq!(json["state"], 2);
    assert_eq!(json["class"]["name"], "Class-A");
    assert_eq!(json["issuer"]["name"], "Issuer-A");
    assert_eq!(json["txHash"], format!("0x{}", hex::encode(&tx_hash)));
    assert_eq!(json["outputIndex"], output_index);
    assert_eq!(json["lifecycle"][0]["event"], "mint");
    assert_eq!(json["lifecycle"][1]["event"], "live");
}

#[tokio::test]
async fn test_assets_nft_item_activities_mnft() {
    let store = test_store();
    let class_id = [0x31u8; 24];
    let token_id = [0x41u8; 28];
    let owner_lock_hash = vec![0x77u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft(
        &token_id,
        &NftEntry {
            standard: NftStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(owner_lock_hash.clone()),
            name: None,
            is_live: true,
            created_at_block: 120,
            extra: NftExtra::MnftToken {
                token_index: 128,
                characteristic: vec![0xaa; 8],
                configure: 5,
                state: 2,
            },
        },
    );
    batch.put_activity(
        &owner_lock_hash,
        300,
        0,
        &ActivityEntry {
            tx_hash: vec![0x91; 32],
            block_number: 300,
            tx_index: 0,
            timestamp: 1_700_000_300,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![AssetChange::Nft {
                nft_id: token_id.to_vec(),
                standard: "m-nft".to_string(),
                action: AssetAction::Transfer,
            }],
            peers: vec![],
        },
    );
    batch.put_activity(
        &owner_lock_hash,
        200,
        0,
        &ActivityEntry {
            tx_hash: vec![0x92; 32],
            block_number: 200,
            tx_index: 0,
            timestamp: 1_700_000_200,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![AssetChange::Nft {
                nft_id: vec![0x55; 28],
                standard: "m-nft".to_string(),
                action: AssetAction::Transfer,
            }],
            peers: vec![],
        },
    );
    batch.put_activity(
        &owner_lock_hash,
        100,
        0,
        &ActivityEntry {
            tx_hash: vec![0x93; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_100,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![AssetChange::Nft {
                nft_id: token_id.to_vec(),
                standard: "m-nft".to_string(),
                action: AssetAction::Mint,
            }],
            peers: vec![],
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/items/0x{}/activities?limit=20",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 100);
    assert_eq!(json["data"][1]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/items/0x{}/activities?limit=1",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["hasMore"], true);
    let next_cursor = json["nextCursor"]
        .as_str()
        .expect("next cursor for mnft activities");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/items/0x{}/activities?limit=1&cursor={}",
            hex::encode(token_id),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 100);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/items/0x{}/activities?action=invalid",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_assets_nft_item_activities_dotbit() {
    let store = test_store();
    let account_id = [0x11u8; 20];
    let other_account_id = [0x22u8; 20];
    let owner_lock_hash = vec![0x88u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_nft(
        &account_id,
        &NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(account_id.to_vec()),
            owner_lock_hash: Some(owner_lock_hash.clone()),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 120,
            extra: NftExtra::DotBit {
                expired_at: Some(1_800_000_000),
            },
        },
    );
    batch.put_activity(
        &owner_lock_hash,
        320,
        0,
        &ActivityEntry {
            tx_hash: vec![0xa1; 32],
            block_number: 320,
            tx_index: 0,
            timestamp: 1_700_000_320,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![AssetChange::Nft {
                nft_id: account_id.to_vec(),
                standard: "dotbit".to_string(),
                action: AssetAction::Transfer,
            }],
            peers: vec![],
        },
    );
    batch.put_activity(
        &owner_lock_hash,
        300,
        0,
        &ActivityEntry {
            tx_hash: vec![0xa2; 32],
            block_number: 300,
            tx_index: 0,
            timestamp: 1_700_000_300,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![AssetChange::Nft {
                nft_id: account_id.to_vec(),
                standard: "dotbit".to_string(),
                action: AssetAction::Mint,
            }],
            peers: vec![],
        },
    );
    batch.put_activity(
        &owner_lock_hash,
        280,
        0,
        &ActivityEntry {
            tx_hash: vec![0xa3; 32],
            block_number: 280,
            tx_index: 0,
            timestamp: 1_700_000_280,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![AssetChange::Nft {
                nft_id: other_account_id.to_vec(),
                standard: "dotbit".to_string(),
                action: AssetAction::Transfer,
            }],
            peers: vec![],
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/dotbit/items/0x{}/activities?limit=20",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["blockNumber"], 320);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 300);
    assert_eq!(json["data"][1]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/dotbit/items/0x{}/activities?limit=1",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 320);
    assert_eq!(json["hasMore"], true);
    let next_cursor = json["nextCursor"]
        .as_str()
        .expect("next cursor for dotbit activities");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/dotbit/items/0x{}/activities?limit=1&cursor={}",
            hex::encode(account_id),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 300);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/nfts/dotbit/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
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
async fn test_address_activities_returns_503_when_derived_store_lags() {
    let core_store = test_store();
    let derived_store = test_store();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 100;
            s.derived_tip_block_number = 80;
        })
        .unwrap();

    let config = test_config_with_derived(core_store, derived_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            "11".repeat(32)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "derived_syncing");
}

#[tokio::test]
async fn test_address_activities_reads_from_derived_store() {
    let core_store = test_store();
    let derived_store = test_store();
    let lock_hash = vec![0x22; 32];
    let activity = ActivityEntry {
        tx_hash: vec![0xaa; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        ckb_delta: 100,
        occupied_delta: 50,
        is_cellbase: false,
        asset_changes: vec![],
        peers: vec![],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_activity(&lock_hash, 10, 0, &activity);
    core_batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
            s.derived_tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_derived(core_store.clone(), derived_store.clone());
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 0);

    let mut derived_batch = StoreBatch::new(derived_store.as_ref());
    derived_batch.put_activity(&lock_hash, 10, 0, &activity);
    derived_batch.commit().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
}
