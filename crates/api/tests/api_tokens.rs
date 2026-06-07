mod common;
use common::*;

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
async fn test_get_token_includes_maximum_supply() {
    let store = test_store();
    let type_hash = vec![0x77; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let holder_lock = [0x11; 32];

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

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &holder_lock, 500_00000000);
    batch.put_token_holder_by_balance(&type_hash, &holder_lock, 500_00000000);
    batch.put_addr_token_by_balance(&holder_lock, &type_hash, 500_00000000);
    batch.commit().unwrap();

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
    assert_eq!(json["totalCommonKnowledgeSize"], serde_json::Value::Null);
    assert_eq!(json["maximumSupply"], "100000000000");
    assert_eq!(json["maximumSupplyStatus"], "limited");
}

#[tokio::test]
async fn test_get_token_returns_store_backed_detail_when_filtered_from_warmup_cache() {
    let store = test_store();
    let type_hash = vec![0x78; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let holder_lock = [0x12; 32];

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: None,
                symbol: None,
                decimals: Some(8),
                total_supply: Some(500_00000000),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: Some("Store-backed token detail".to_string()),
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &holder_lock, 500_00000000);
    batch.put_token_holder_by_balance(&type_hash, &holder_lock, 500_00000000);
    batch.put_addr_token_by_balance(&holder_lock, &type_hash, 500_00000000);
    batch.commit().unwrap();

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
    assert_eq!(json["typeScriptHash"], type_hash_hex);
    assert!(json["name"].is_null());
    assert!(json["symbol"].is_null());
    assert_eq!(json["description"], "Store-backed token detail");
    assert_eq!(json["totalSupply"], "50000000000");
}

#[tokio::test]
async fn test_get_token_derives_stats_from_holder_and_stats_cfs_when_token_row_is_placeholder() {
    let store = test_store();
    let type_hash = vec![0x7b; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 2,
                type_args: vec![0x66; 32],
                standard: "xudt".to_string(),
                name: Some("Placeholder Label".to_string()),
                symbol: Some("PLH".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: Some("logo.png".to_string()),
                description: Some("label metadata only".to_string()),
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &[0x01; 32], 200);
    batch.put_token_holder(&type_hash, &[0x02; 32], 100);
    batch.put_token_holder_by_balance(&type_hash, &[0x01; 32], 200);
    batch.put_token_holder_by_balance(&type_hash, &[0x02; 32], 100);
    batch.put_addr_token_by_balance(&[0x01; 32], &type_hash, 200);
    batch.put_addr_token_by_balance(&[0x02; 32], &type_hash, 100);
    batch.put_token_transfers_count(&type_hash, 7);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let detail_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tokens/{type_hash_hex}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_body = detail_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let detail_json: serde_json::Value = serde_json::from_slice(&detail_body).unwrap();
    assert_eq!(detail_json["name"], "Placeholder Label");
    assert_eq!(detail_json["totalSupply"], "300");
    assert_eq!(detail_json["holdersCount"], 2);
    assert_eq!(detail_json["transfersCount"], 7);

    let holders_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tokens/{type_hash_hex}/holders?limit=10"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(holders_response.status(), StatusCode::OK);
    let holders_body = holders_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let holders_json: serde_json::Value = serde_json::from_slice(&holders_body).unwrap();
    assert_eq!(holders_json["total"], 2);
    assert_eq!(holders_json["data"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_get_token_holders_preserves_equal_balance_pagination() {
    let store = test_store();
    let type_hash = vec![0x79; 32];
    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Paged Holders".to_string()),
                symbol: Some("PH".to_string()),
                decimals: Some(8),
                total_supply: Some(300),
                max_supply: None,
                holders_count: 3,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &[0x01; 32], 100);
    batch.put_token_holder(&type_hash, &[0x02; 32], 100);
    batch.put_token_holder(&type_hash, &[0x03; 32], 50);
    batch.put_token_holder_by_balance(&type_hash, &[0x01; 32], 100);
    batch.put_token_holder_by_balance(&type_hash, &[0x02; 32], 100);
    batch.put_token_holder_by_balance(&type_hash, &[0x03; 32], 50);
    batch.put_addr_token_by_balance(&[0x01; 32], &type_hash, 100);
    batch.put_addr_token_by_balance(&[0x02; 32], &type_hash, 100);
    batch.put_addr_token_by_balance(&[0x03; 32], &type_hash, 50);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    let first_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tokens/{type_hash_hex}/holders?limit=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    let next_cursor = first_json["nextCursor"]
        .as_str()
        .expect("first page should have next cursor")
        .to_string();
    assert_eq!(
        first_json["data"][0]["lockScriptHash"],
        format!("0x{}", "01".repeat(32))
    );

    let second_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/tokens/{type_hash_hex}/holders?limit=1&cursor={next_cursor}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(
        second_json["data"][0]["lockScriptHash"],
        format!("0x{}", "02".repeat(32))
    );
}

#[tokio::test]
async fn test_get_address_tokens_uses_store_backed_pagination_without_warmup_cache() {
    let store = test_store();
    let lock_hash = vec![0x88; 32];
    let token_a = vec![0x81; 32];
    let token_b = vec![0x82; 32];

    store
        .put_token_direct(
            &token_a,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Alpha".to_string()),
                symbol: Some("ALP".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 1,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_direct(
            &token_b,
            &TokenInfo {
                type_code_hash: vec![0x56; 32],
                hash_type: 1,
                type_args: vec![0x67; 20],
                standard: "sudt".to_string(),
                name: Some("Beta".to_string()),
                symbol: Some("BET".to_string()),
                decimals: Some(4),
                total_supply: Some(300),
                max_supply: None,
                holders_count: 1,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&token_a, &lock_hash, 200);
    batch.put_token_holder(&token_b, &lock_hash, 100);
    batch.put_token_holder_by_balance(&token_a, &lock_hash, 200);
    batch.put_token_holder_by_balance(&token_b, &lock_hash, 100);
    batch.put_addr_token_by_balance(&lock_hash, &token_a, 200);
    batch.put_addr_token_by_balance(&lock_hash, &token_b, 100);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let lock_hash_hex = format!("0x{}", hex::encode(&lock_hash));

    let first_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/addresses/{lock_hash_hex}/tokens?limit=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(
        first_json["data"][0]["typeScriptHash"],
        format!("0x{}", hex::encode(&token_a))
    );
    let next_cursor = first_json["nextCursor"]
        .as_str()
        .expect("first page should have next cursor")
        .to_string();

    let second_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/{lock_hash_hex}/tokens?limit=1&cursor={next_cursor}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(
        second_json["data"][0]["typeScriptHash"],
        format!("0x{}", hex::encode(&token_b))
    );
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
async fn test_token_capacity_chart_returns_cumulative_series() {
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
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &type_hash,
            20240117,
            &TokenDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["title"], "TEST Capacity History");
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["used"], "60");
    assert_eq!(data[1]["values"]["unused"], "40");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["used"], "50");
    assert_eq!(data[2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
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
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
}

#[tokio::test]
async fn test_token_capacity_chart_reads_daily_deltas_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let type_hash = vec![0x64; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    core_store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Derived Delta Token".to_string()),
                symbol: Some("DDT".to_string()),
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

    core_store
        .put_token_daily_delta(
            &type_hash,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
}

#[tokio::test]
async fn test_token_capacity_chart_rejects_invalid_date_range() {
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
            "/api/v1/tokens/{}/charts/capacity-history?from=2024-01-31&to=2024-01-01",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
