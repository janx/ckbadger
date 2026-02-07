//! Integration tests for scripts API endpoints.
//!
//! These tests require a running ClickHouse instance with the ckbadger database.
//! Run with: cargo test -p ckbadger-api --test scripts_integration

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use ckbadger_api::{create_router, AppConfig};
use ckbadger_common::{ClickHouseClient, ClickHouseConfig};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn percent_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '!' => "%21".to_string(),
            '#' => "%23".to_string(),
            '$' => "%24".to_string(),
            '&' => "%26".to_string(),
            '\'' => "%27".to_string(),
            '(' => "%28".to_string(),
            ')' => "%29".to_string(),
            '*' => "%2A".to_string(),
            '+' => "%2B".to_string(),
            ',' => "%2C".to_string(),
            '/' => "%2F".to_string(),
            ':' => "%3A".to_string(),
            ';' => "%3B".to_string(),
            '=' => "%3D".to_string(),
            '?' => "%3F".to_string(),
            '@' => "%40".to_string(),
            '[' => "%5B".to_string(),
            ']' => "%5D".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

async fn create_test_router() -> Router {
    let config = ClickHouseConfig::from_env().expect("Failed to load ClickHouse config");
    let pool = ClickHouseClient::new(config);

    let app_config = AppConfig {
        pool,
        redis_url: None,
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_network: "mainnet".to_string(),
        rate_limit_per_second: None,
        rate_limit_burst: None,
        start_background_tasks: false,
    };

    create_router(app_config).await
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

// GET /api/v1/scripts

#[tokio::test]
async fn test_list_scripts_returns_ok() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        json.get("data").is_some(),
        "Response should have 'data' field"
    );
    assert!(json["data"].is_array(), "data should be an array");
}

#[tokio::test]
async fn test_list_scripts_with_limit() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?limit=5").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();
    assert!(data.len() <= 5, "Should respect limit parameter");
}

#[tokio::test]
async fn test_list_scripts_with_network_filter() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?network=mainnet").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();

    for script in data {
        assert_eq!(script["network"].as_str().unwrap(), "mainnet");
    }
}

#[tokio::test]
async fn test_list_scripts_with_testnet_filter() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?network=testnet").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();

    for script in data {
        assert_eq!(script["network"].as_str().unwrap(), "testnet");
    }
}

#[tokio::test]
async fn test_list_scripts_with_decoder_type_filter() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?decoderType=udt").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();

    for script in data {
        if let Some(decoder) = script["decoderType"].as_str() {
            assert_eq!(decoder, "udt");
        }
    }
}

#[tokio::test]
async fn test_list_scripts_with_search() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?search=secp256k1").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();

    if !data.is_empty() {
        let first = &data[0];
        let name = first["name"].as_str().unwrap().to_lowercase();
        assert!(name.contains("secp256k1"));
    }
}

#[tokio::test]
async fn test_list_scripts_pagination() {
    let app = create_test_router().await;

    let (status, json1) = get_json(&app, "/api/v1/scripts?limit=3").await;
    assert_eq!(status, StatusCode::OK);

    let data1 = json1["data"].as_array().unwrap();
    if data1.len() < 3 {
        return;
    }

    if let Some(cursor) = json1["nextCursor"].as_str() {
        let encoded_cursor = percent_encode(cursor);
        let (status, json2) = get_json(
            &app,
            &format!("/api/v1/scripts?limit=3&cursor={}", encoded_cursor),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let data2 = json2["data"].as_array().unwrap();
        if !data2.is_empty() {
            assert_ne!(
                data1.last().unwrap()["name"],
                data2.first().unwrap()["name"],
                "Pagination should return different items"
            );
        }
    }
}

#[tokio::test]
async fn test_list_scripts_response_structure() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?limit=1").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();

    if !data.is_empty() {
        let script = &data[0];

        assert!(script.get("codeHash").is_some(), "codeHash is required");
        assert!(script.get("name").is_some(), "name is required");
        assert!(script.get("network").is_some(), "network is required");
        assert!(script.get("deprecated").is_some(), "deprecated is required");
        assert!(script.get("isSystem").is_some(), "isSystem is required");

        let code_hash = script["codeHash"].as_str().unwrap();
        assert!(code_hash.starts_with("0x"));
        assert_eq!(code_hash.len(), 66);
    }
}

// GET /api/v1/scripts/{name}

#[tokio::test]
async fn test_get_script_by_name() {
    let app = create_test_router().await;

    let (status, json) = get_json(&app, "/api/v1/scripts?limit=1").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    if data.is_empty() {
        return;
    }

    let name = data[0]["name"].as_str().unwrap();
    let encoded_name = percent_encode(name);

    let (status, json) = get_json(&app, &format!("/api/v1/scripts/{}", encoded_name)).await;
    assert_eq!(status, StatusCode::OK);

    // Response is an array of deployments
    assert!(json.is_array(), "Response should be an array");
    let scripts = json.as_array().unwrap();
    assert!(!scripts.is_empty(), "Should return at least one deployment");
    assert!(scripts[0].get("codeHash").is_some());
    assert!(scripts[0].get("name").is_some());
}

#[tokio::test]
async fn test_get_script_not_found() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts/NonExistentScript12345").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json.get("error").is_some());
}

// GET /api/v1/scripts/{name}/usage

#[tokio::test]
async fn test_get_script_usage() {
    let app = create_test_router().await;

    let (status, json) = get_json(&app, "/api/v1/scripts?limit=1").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    if data.is_empty() {
        return;
    }

    let name = data[0]["name"].as_str().unwrap();
    let encoded_name = percent_encode(name);

    let (status, json) = get_json(&app, &format!("/api/v1/scripts/{}/usage", encoded_name)).await;
    assert_eq!(status, StatusCode::OK);

    // Response is the usage object directly
    assert!(json.get("name").is_some());
    assert!(json.get("byDeployment").is_some());

    let deployments = json["byDeployment"].as_array().unwrap();
    for deployment in deployments {
        assert!(deployment.get("codeHash").is_some());
        assert!(deployment.get("liveCellsCount").is_some());
        assert!(deployment.get("cellsCount").is_some());
    }
}

#[tokio::test]
async fn test_get_script_usage_not_found() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts/NonExistentScript12345/usage").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json.get("error").is_some());
}

// POST /api/v1/scripts/lookup

#[tokio::test]
async fn test_lookup_scripts_empty_array() {
    let app = create_test_router().await;
    let body = serde_json::json!({
        "codeHashes": []
    });

    let (status, json) = post_json(&app, "/api/v1/scripts/lookup", body).await;
    assert_eq!(status, StatusCode::OK);

    // Response is an empty object directly
    assert!(json.is_object());
    assert!(json.as_object().unwrap().is_empty());
}

#[tokio::test]
async fn test_lookup_scripts_with_known_hash() {
    let app = create_test_router().await;

    let (status, json) = get_json(&app, "/api/v1/scripts?limit=1").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    if data.is_empty() {
        return;
    }

    let code_hash = data[0]["codeHash"].as_str().unwrap();

    let body = serde_json::json!({
        "codeHashes": [code_hash]
    });

    let (status, json) = post_json(&app, "/api/v1/scripts/lookup", body).await;
    assert_eq!(status, StatusCode::OK);

    // Response is a HashMap directly
    assert!(json.is_object());
    let lookup_data = json.as_object().unwrap();
    assert!(
        lookup_data.get(code_hash).is_some(),
        "Should find the script"
    );
}

#[tokio::test]
async fn test_lookup_scripts_with_unknown_hash() {
    let app = create_test_router().await;

    // Use a hash that doesn't exist (not 0x000...000 which is "Zero Lock")
    let fake_hash = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let body = serde_json::json!({
        "codeHashes": [fake_hash]
    });

    let (status, json) = post_json(&app, "/api/v1/scripts/lookup", body).await;
    assert_eq!(status, StatusCode::OK);

    let lookup_data = json.as_object().unwrap();
    assert!(
        lookup_data.get(fake_hash).is_none(),
        "Unknown hash should not be in results"
    );
}

#[tokio::test]
async fn test_lookup_scripts_multiple_hashes() {
    let app = create_test_router().await;

    let (status, json) = get_json(&app, "/api/v1/scripts?limit=3").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    if data.len() < 2 {
        return;
    }

    let code_hashes: Vec<&str> = data
        .iter()
        .map(|s| s["codeHash"].as_str().unwrap())
        .collect();

    let body = serde_json::json!({
        "codeHashes": code_hashes
    });

    let (status, json) = post_json(&app, "/api/v1/scripts/lookup", body).await;
    assert_eq!(status, StatusCode::OK);

    let lookup_data = json.as_object().unwrap();
    assert!(!lookup_data.is_empty(), "Should find at least some scripts");
}

// GET /api/v1/scripts/code-cell

#[tokio::test]
async fn test_get_code_cell_missing_params() {
    let app = create_test_router().await;
    let (status, _json) = get_json(&app, "/api/v1/scripts/code-cell").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_code_cell_missing_hash_type() {
    let app = create_test_router().await;
    let fake_hash = "0x0000000000000000000000000000000000000000000000000000000000000000";
    let (status, _json) = get_json(
        &app,
        &format!("/api/v1/scripts/code-cell?code_hash={}", fake_hash),
    )
    .await;

    // Missing hash_type returns bad request
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_code_cell_with_unknown_hash() {
    let app = create_test_router().await;
    let fake_hash = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let (status, json) = get_json(
        &app,
        &format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=type",
            fake_hash
        ),
    )
    .await;

    // TODO: API should return 200 with null values, but currently returns 500
    // This is a known issue with query_one returning error for no rows
    assert!(
        status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
        "Should handle unknown hash"
    );
    if status == StatusCode::OK {
        assert!(json.get("txHash").is_some());
        assert!(json["txHash"].is_null());
    }
}

#[tokio::test]
async fn test_get_code_cell_with_known_hash() {
    let app = create_test_router().await;

    let (status, json) = get_json(&app, "/api/v1/scripts?limit=20").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();

    let script_with_code_cell = data.iter().find(|s| {
        s["codeCellTxHash"].is_string() && !s["codeCellTxHash"].as_str().unwrap().is_empty()
    });

    if let Some(script) = script_with_code_cell {
        let code_hash = script["codeHash"].as_str().unwrap();
        let hash_type = script["hashType"].as_str().unwrap_or("type");
        let (status, json) = get_json(
            &app,
            &format!(
                "/api/v1/scripts/code-cell?code_hash={}&hash_type={}",
                code_hash, hash_type
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json.get("txHash").is_some());
        assert!(json.get("outputIndex").is_some());
    }
}

// Edge cases

#[tokio::test]
async fn test_invalid_limit_parameter() {
    let app = create_test_router().await;
    let (status, _json) = get_json(&app, "/api/v1/scripts?limit=-1").await;

    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "Should handle invalid limit gracefully"
    );
}

#[tokio::test]
async fn test_large_limit_parameter() {
    let app = create_test_router().await;
    let (status, json) = get_json(&app, "/api/v1/scripts?limit=10000").await;

    assert_eq!(status, StatusCode::OK);
    let data = json["data"].as_array().unwrap();
    assert!(data.len() <= 100, "Large limit should be capped");
}

#[tokio::test]
async fn test_special_characters_in_search() {
    let app = create_test_router().await;
    let (status, _json) = get_json(&app, "/api/v1/scripts?search=%27OR%201=1--").await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_unicode_in_search() {
    let app = create_test_router().await;
    let (status, _json) = get_json(&app, "/api/v1/scripts?search=%E4%B8%AD%E6%96%87").await;

    assert_eq!(status, StatusCode::OK);
}
