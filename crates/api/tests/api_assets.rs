mod common;
use common::*;

#[tokio::test]
async fn test_assets_nft_includes_spore_cluster_name_when_aggregate_name_missing() {
    let store = test_store();

    let cluster_id = [0x42u8; 32];
    let cluster_entry = ObjectEntry {
        standard: ObjectStandard::SporeCluster,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("Recovered Cluster Name".to_string()),
        description: Some("desc".to_string()),
        is_live: true,
        created_at_block: 123,
        created_at_tx: vec![0x22; 32],
        extra: ObjectExtra::SporeCluster,
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
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=object")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["name"], "Recovered Cluster Name");
    assert_eq!(json["data"][0]["assetType"], "object");
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
                    max_supply: None,
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
                    owned_capacity_delta: 100,
                    owned_knowledge_delta: 50,
                },
            )
            .unwrap();
    }

    store
        .put_spore_direct(
            &spore_cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x11; 32]),
                name: Some("Spore Filter Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0x22; 32],
                extra: ObjectExtra::SporeCluster,
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
            ..Default::default()
        },
    );
    batch.put_mnft_collection_aggregate(
        &dotbit_collection_id,
        &MnftCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: ObjectStandard::default(),
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
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
        .uri("/api/v1/assets?type=object&standard=spore")
        .body(Body::empty())
        .unwrap();
    let nft_response = app.oneshot(nft_request).await.unwrap();
    assert_eq!(nft_response.status(), StatusCode::OK);
    let nft_body = nft_response.into_body().collect().await.unwrap().to_bytes();
    let nft_json: serde_json::Value = serde_json::from_slice(&nft_body).unwrap();
    assert_eq!(nft_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(nft_json["data"][0]["standard"], "spore");
    assert_eq!(nft_json["data"][0]["assetType"], "object");
}

#[tokio::test]
async fn test_assets_list_supports_composition_tier_filter_and_onchain_ratio_sort() {
    let store = test_store();
    let cluster_onchain = [0x81u8; 32];
    let cluster_centralized = [0x82u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_onchain,
        &ClusterAggregate {
            name: Some("Onchain Cluster".to_string()),
            description: None,
            total_count: 5,
            live_count: 5,
            owner_count: 2,
            btc_ckb_count: 0,
            pure_ckb_count: 5,
            decentralized_mixture_count: 0,
            centralized_mixture_count: 0,
            unknown_count: 0,
            ..Default::default()
        },
    );
    batch.put_cluster_aggregate(
        &cluster_centralized,
        &ClusterAggregate {
            name: Some("Centralized Cluster".to_string()),
            description: None,
            total_count: 4,
            live_count: 4,
            owner_count: 2,
            btc_ckb_count: 0,
            pure_ckb_count: 0,
            decentralized_mixture_count: 0,
            centralized_mixture_count: 4,
            unknown_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=object&composition_tier=pure_ckb")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Onchain Cluster");
    assert_eq!(rows[0]["compositionTier"], "pure_ckb");

    let request = Request::builder()
        .uri("/api/v1/assets?type=object&composition_tier=centralized_mixture")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Centralized Cluster");
    assert_eq!(rows[0]["compositionTier"], "centralized_mixture");

    let request = Request::builder()
        .uri("/api/v1/assets?type=object&sort_key=onchain_ratio&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "Onchain Cluster");
    assert_eq!(rows[1]["name"], "Centralized Cluster");
}

#[tokio::test]
async fn test_assets_list_includes_did_ckb_collection_under_nft_type() {
    let store = test_store();
    let did_collection_id = *b"did_ckb_collection______________";

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_mnft_collection_aggregate(
        &did_collection_id,
        &MnftCollectionAggregate {
            name: Some("did:ckb".to_string()),
            standard: ObjectStandard::default(),
            total_count: 2,
            live_count: 2,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=identity&standard=did:ckb")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["assetType"], "identity");
    assert_eq!(rows[0]["standard"], "did_ckb");
    assert_eq!(rows[0]["name"], "did:ckb");
}

#[tokio::test]
async fn test_nft_collection_items_supports_did_ckb_collection_from_spore_data() {
    let store = test_store();
    let did_collection_id = *b"did_ckb_collection______________";
    let did_id = [0xD3u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &did_id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("did:alice.ckb".to_string()),
            is_live: true,
            created_at_block: 321,
            created_at_tx: vec![0x22; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.put_identity_collection_aggregate(
        &did_collection_id,
        &IdentityCollectionAggregate {
            name: Some("did:ckb".to_string()),
            standard: IdentityStandard::DidCkb,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity_by_collection(&did_collection_id, &did_id);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/0x{}/items",
            hex::encode(did_collection_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["standard"], "did_ckb");
    assert_eq!(rows[0]["name"], "did:alice.ckb");
    assert_eq!(rows[0]["isLive"], true);
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
                max_supply: None,
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
                max_supply: None,
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
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_b,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 300,
                owned_knowledge_delta: 120,
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
    assert_eq!(json["data"][0]["ownedCapacity"], "300");
    assert_eq!(json["data"][0]["ownedKnowledge"], "120");

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
async fn test_assets_supply_sort_orders_aggregate_beyond_u128() {
    // Sorting uses the exact aggregate domain, including values beyond u128.
    let store = test_store();
    let token_small = [0x51u8; 32];
    let token_huge = [0x52u8; 32];

    let amount = 200u128 << 120;

    store
        .put_token_direct(
            &token_small,
            &TokenInfo {
                type_code_hash: vec![0xAA; 32],
                hash_type: 1,
                type_args: vec![0x01; 20],
                standard: "xudt".to_string(),
                name: Some("Small Supply".to_string()),
                symbol: Some("SMALL".to_string()),
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
        .put_token_direct(
            &token_huge,
            &TokenInfo {
                type_code_hash: vec![0xBB; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("Huge Supply".to_string()),
                symbol: Some("HUGE".to_string()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&token_small, &[0x01; 32], 1_000u128);
    batch.put_token_holder(&token_huge, &[0x02; 32], amount);
    batch.put_token_holder(&token_huge, &[0x03; 32], amount);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    // Descending by supply: the > u128::MAX token must come first.
    let request = Request::builder()
        .uri("/api/v1/assets?type=token&sort_key=supply&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["data"][0]["id"],
        format!("0x{}", hex::encode(token_huge))
    );
    assert_eq!(
        json["data"][0]["totalSupply"],
        "531691198313966349161522824112137830400"
    );
    assert_eq!(
        json["data"][1]["id"],
        format!("0x{}", hex::encode(token_small))
    );

    // Ascending by supply: the small token must come first.
    let request = Request::builder()
        .uri("/api/v1/assets?type=token&sort_key=supply&sort_direction=asc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["data"][0]["id"],
        format!("0x{}", hex::encode(token_small))
    );
    assert_eq!(
        json["data"][1]["id"],
        format!("0x{}", hex::encode(token_huge))
    );
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
                    max_supply: None,
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
                owned_capacity_delta: 200,
                owned_knowledge_delta: 100,
            },
        )
        .unwrap();

    // Broken history: used exceeds capacity; API must fail fast instead of masking.
    store
        .put_token_daily_delta(
            &broken_token,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 120,
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
    assert!(message.contains("asset cache warmup failed"));
    assert!(message.contains("invalid token daily deltas during warmup"));
    assert!(message.contains(&format!("type_hash=0x{}", hex::encode(broken_token))));
}

#[tokio::test]
async fn test_assets_nft_collection_capacity_chart_and_capacity_fields() {
    let store = test_store();
    let collection_id = [0x24u8; 24];
    let collection_id_hex = format!("0x{}", hex::encode(collection_id));

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_mnft_collection_aggregate(
        &collection_id,
        &MnftCollectionAggregate {
            name: Some("Test NFT Collection".to_string()),
            standard: ObjectStandard::MnftToken,
            total_count: 100,
            live_count: 60,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    store
        .put_mnft_daily_delta(
            &collection_id,
            20240115,
            &MnftDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_mnft_daily_delta(
            &collection_id,
            20240117,
            &MnftDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/{}/charts/capacity-history",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Test NFT Collection Capacity History");
    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][1]["values"]["used"], "60");
    assert_eq!(json["data"][1]["values"]["unused"], "40");
    assert_eq!(json["data"][2]["values"]["used"], "50");
    assert_eq!(json["data"][2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
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
    assert_eq!(json["data"][0]["values"]["used"], "60");
    assert_eq!(json["data"][0]["values"]["unused"], "40");

    let request = Request::builder()
        .uri(format!("/api/v1/assets/objects/{}", collection_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["ownedCapacity"], "80");
    assert_eq!(json["ownedKnowledge"], "50");
}

#[tokio::test]
async fn test_assets_nft_collection_accepts_dotbit_alias() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: None,
            standard: IdentityStandard::DotBit,
            total_count: 200,
            live_count: 120,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.commit().unwrap();

    store
        .put_mnft_daily_delta(
            &collection_id,
            20240115,
            &MnftDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], ".bit Capacity History");
    assert_eq!(json["data"][0]["values"]["used"], "60");
    assert_eq!(json["data"][0]["values"]["unused"], "40");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "dotbit");
    assert_eq!(json["name"], ".bit");
    assert_eq!(json["ownedCapacity"], "100");
    assert_eq!(json["ownedKnowledge"], "60");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/DOTBIT/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let request = Request::builder()
        .uri("/api/v1/assets/objects/%2Ebit")
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
async fn test_assets_nft_collection_detail_uses_preaggregated_counts() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 200,
            live_count: 120,
            holders_count: 77,
            activities_count: 6_543,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["holdersCount"], 77);
    assert_eq!(json["activitiesCount"], 6543);
}

#[tokio::test]
async fn test_assets_nft_collection_detail_enriches_mnft_class_metadata() {
    let store = test_store();
    let issuer_id = [0x21u8; 20];
    let class_id = [0x31u8; 24];

    let mut batch = StoreBatch::new(store.as_ref());

    // Insert issuer ObjectEntry with MnftIssuer extra
    batch.put_mnft(
        &issuer_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x01; 32]),
            name: Some("Issuer-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 90,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftIssuer {
                class_count: 2,
                set_count: 3,
                info: Some(br#"{"name":"Issuer-A"}"#.to_vec()),
            },
        },
    );

    // Insert class ObjectEntry with MnftClass extra
    batch.put_mnft(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x02; 32]),
            name: Some("Class-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 95,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 500,
                issued: 128,
                configure: 9,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );

    // Insert MnftCollectionAggregate (required for get_object_collection to find it)
    batch.put_mnft_collection_aggregate(
        &class_id,
        &MnftCollectionAggregate {
            name: Some("Class-A".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 50,
            live_count: 40,
            holders_count: 12,
            activities_count: 30,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    // Hit the endpoint
    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/0x{}",
            hex::encode(class_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify base collection fields
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["name"], "Class-A");
    assert_eq!(json["totalCount"], 50);
    assert_eq!(json["liveCount"], 40);
    assert_eq!(json["holdersCount"], 12);
    assert_eq!(json["activitiesCount"], 30);

    // Verify enriched class metadata
    assert_eq!(json["classDetail"]["name"], "Class-A");
    assert_eq!(json["classDetail"]["description"], "Class description");
    assert_eq!(json["classDetail"]["renderer"], "renderer:v1");
    assert_eq!(json["classDetail"]["total"], 500);
    assert_eq!(json["classDetail"]["issued"], 128);
    assert_eq!(json["classDetail"]["configure"], 9);
    assert_eq!(
        json["classDetail"]["classId"],
        format!("0x{}", hex::encode(class_id))
    );
    assert_eq!(
        json["classDetail"]["issuerId"],
        format!("0x{}", hex::encode(issuer_id))
    );

    // Verify enriched issuer metadata
    assert_eq!(json["issuerDetail"]["name"], "Issuer-A");
    assert_eq!(json["issuerDetail"]["classCount"], 2);
    assert_eq!(json["issuerDetail"]["setCount"], 3);
    assert_eq!(
        json["issuerDetail"]["issuerId"],
        format!("0x{}", hex::encode(issuer_id))
    );

    // Verify created_at_block and owner_lock_hash
    assert_eq!(json["createdAtBlock"], 95);
    let owner_hash = json["ownerLockHash"].as_str().unwrap();
    assert!(owner_hash.starts_with("0x"));
    assert_eq!(owner_hash, format!("0x{}", hex::encode(vec![0x02u8; 32])));
}

#[tokio::test]
async fn test_assets_nft_collection_accepts_did_ckb_aliases() {
    let store = test_store();
    let collection_id = b"did_ckb_collection______________".to_vec();
    let did_id = [0xA5u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &did_id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![0x21; 32]),
            name: Some("did:alice.ckb".to_string()),
            is_live: true,
            created_at_block: 888,
            created_at_tx: vec![0x33; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: None,
            standard: IdentityStandard::DidCkb,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity_by_collection(&collection_id, &did_id);
    batch.commit().unwrap();

    store
        .put_mnft_daily_delta(
            &collection_id,
            20240115,
            &MnftDailyDelta {
                owned_capacity_delta: 120,
                owned_knowledge_delta: 70,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/did:ckb")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "did_ckb");
    assert_eq!(json["name"], "did:ckb");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/did_ckb/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "did:alice.ckb");
    assert_eq!(json["data"][0]["standard"], "did_ckb");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/did%3Ackb/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "did:ckb Capacity History");
    assert_eq!(json["data"][0]["values"]["used"], "70");
    assert_eq!(json["data"][0]["values"]["unused"], "50");
}

#[tokio::test]
async fn test_assets_did_ckb_item_detail_supports_20_byte_item_ids() {
    let store = test_store();
    // Real live-testnet did:ckb item id (type-script args verbatim, 20 bytes):
    // cell 0x1d43c10b...:0. 31 of 421 live testnet did:ckb cells carry
    // 20-byte args, so the detail route must accept non-32-byte item ids.
    let did_id = hex::decode("00ee044b93fab31c060417d159f9678b7cc154d4").unwrap();
    assert_eq!(did_id.len(), 20);

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &did_id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: None,
            is_live: true,
            created_at_block: 21_080_336,
            created_at_tx: vec![0x91; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}",
            hex::encode(&did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["nftId"], format!("0x{}", hex::encode(&did_id)));
    assert_eq!(json["standard"], "did_ckb");
    assert_eq!(json["isLive"], true);
    assert_eq!(json["createdAtBlock"], 21_080_336);
}

/// The item lifecycle feed resolves through the outpoint reverse index, whose
/// id component is variable-width. A real 20-byte did:ckb item id must produce
/// its real mint/transfer history, not an empty feed.
#[tokio::test]
async fn test_assets_did_ckb_20_byte_item_activities_resolve() {
    let store = test_store();
    // Real live-testnet did:ckb item id (type-script args verbatim, 20 bytes).
    let did_id = hex::decode("00ee044b93fab31c060417d159f9678b7cc154d4").unwrap();
    assert_eq!(did_id.len(), 20);
    let mint_tx = vec![0x93; 32];
    let transfer_tx = vec![0x94; 32];

    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_identity(
            &did_id,
            &IdentityEntry {
                standard: IdentityStandard::DidCkb,
                owner_lock_hash: Some(vec![0x31; 32]),
                name: None,
                is_live: true,
                created_at_block: 300,
                created_at_tx: mint_tx.clone(),
                extra: IdentityExtra::DidCkb,
            },
        );
        batch.commit().unwrap();
    }

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_spore_outpoint(&mint_tx, 0, &did_id);
    batch.put_spore_outpoint(&transfer_tx, 0, &did_id);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 521_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![0x61; 22],
            type_script_hash: Some(vec![0x71; 32]),
            type_code_hash: Some(vec![0x81; 32]),
            type_hash_type: Some(1),
            type_args: Some(did_id.clone()),
            data_size: 205,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        300,
        400,
        Some(&transfer_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 300, 0);
    batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_753_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 400, 0);
    batch.put_tx_index(
        400,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_753_000_100,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}/activities?limit=20",
            hex::encode(&did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["data"].as_array().unwrap().len(),
        2,
        "20-byte item id must resolve its lifecycle rows"
    );
    assert_eq!(json["data"][0]["blockNumber"], 400);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 300);
    assert_eq!(json["data"][1]["actions"][0], "mint");
}

#[tokio::test]
async fn test_assets_did_ckb_item_detail_and_activities() {
    let store = test_store();
    let did_id = [0xB7u8; 32];
    let mint_tx = vec![0x91; 32];
    let transfer_tx = vec![0x92; 32];

    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_identity(
            &did_id,
            &IdentityEntry {
                standard: IdentityStandard::DidCkb,
                owner_lock_hash: Some(vec![0x31; 32]),
                name: Some("did:alice.ckb".to_string()),
                is_live: true,
                created_at_block: 100,
                created_at_tx: mint_tx.clone(),
                extra: IdentityExtra::DidCkb,
            },
        );
        batch.commit().unwrap();
    }

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_spore_outpoint(&mint_tx, 0, &did_id);
    batch.put_spore_outpoint(&transfer_tx, 0, &did_id);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![0x61; 20],
            type_script_hash: Some(vec![0x71; 32]),
            type_code_hash: Some(vec![0x81; 32]),
            type_hash_type: Some(1),
            type_args: Some(did_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}",
            hex::encode(did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["name"], "did:alice.ckb");
    assert_eq!(json["standard"], "did_ckb");
    assert_eq!(json["isLive"], true);
    assert_eq!(json["txHash"], serde_json::Value::Null);
    assert_eq!(json["outputIndex"], serde_json::Value::Null);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}/activities?limit=20",
            hex::encode(did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["blockNumber"], 200);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 100);
    assert_eq!(json["data"][1]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
}

#[tokio::test]
async fn test_assets_nft_list_uses_dotbit_display_name_when_aggregate_name_missing() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_mnft_collection_aggregate(
        &collection_id,
        &MnftCollectionAggregate {
            name: None,
            standard: ObjectStandard::default(),
            total_count: 20,
            live_count: 12,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=identity")
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
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 2,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_a,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_b,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: Some("bob.bit".to_string()),
            is_live: false,
            created_at_block: 101,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_900_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_a);
    batch.put_identity_by_collection(&collection_id, &nft_b);
    batch.put_cell(
        &nft_a_tx_hash,
        nft_a_output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(nft_a_type_hash.clone()),
            type_code_hash: Some(dotbit_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(nft_a.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
    );
    batch.put_dotbit_account_outpoint(&nft_a_tx_hash, nft_a_output_index, &nft_a);
    batch.put_dotbit_outpoint_by_account_id(&nft_a, &nft_a_tx_hash, nft_a_output_index);
    batch.put_cell_by_type(&nft_a_type_hash, 100, &nft_a_tx_hash, nft_a_output_index);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=1")
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
            "/api/v1/assets/objects/dotbit/items?limit=1&cursor={cursor}"
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
        .uri("/api/v1/assets/objects/dotbit/items?limit=20&search=alice")
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
        .uri("/api/v1/assets/objects/dotbit/items?limit=20&status=live")
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
        .uri("/api/v1/assets/objects/dotbit/items?limit=20&status=recycled")
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
            "/api/v1/assets/identities/dotbit/items/0x{}",
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

/// The `.bit Cell` item activities route had no coverage at all, so nothing
/// pinned that it resolves the identity, enforces the standard, or orders and
/// filters the lifecycle actions the way its dotbit/did siblings do.
#[tokio::test]
async fn test_assets_bit_cell_item_activities() {
    let store = test_store();
    let identity_id =
        hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab36345847129d0cc4135d1ffd4").unwrap();
    let account_id = hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab3").unwrap();
    let mint_tx = vec![0xc1; 32];
    let transfer_tx = vec![0xc2; 32];

    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_identity(
            &identity_id,
            &IdentityEntry {
                standard: IdentityStandard::BitCell,
                owner_lock_hash: Some(vec![0x31; 32]),
                name: Some("20240507.bit".to_string()),
                is_live: true,
                created_at_block: 100,
                created_at_tx: mint_tx.clone(),
                extra: IdentityExtra::BitCell {
                    account_id,
                    expired_at: 1_778_140_699,
                },
            },
        );
        batch.commit().unwrap();
    }

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_spore_outpoint(&mint_tx, 0, &identity_id);
    batch.put_spore_outpoint(&transfer_tx, 0, &identity_id);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x41; 32],
            lock_hash_type: 1,
            lock_args: vec![0x51; 20],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x71; 32]),
            type_hash_type: Some(1),
            type_args: Some(identity_id.clone()),
            data_size: 72,
            occupied_capacity: 158_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    // Newest lifecycle action first, each carrying its own block.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/bit-cell/items/0x{}/activities?limit=20",
            hex::encode(&identity_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["blockNumber"], 200);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&transfer_tx))
    );
    assert_eq!(json["data"][1]["blockNumber"], 100);
    assert_eq!(json["data"][1]["actions"][0], "mint");
    assert_eq!(
        json["data"][1]["txHash"],
        format!("0x{}", hex::encode(&mint_tx))
    );

    // The action filter narrows to a single lifecycle step.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/bit-cell/items/0x{}/activities?limit=20&action=mint",
            hex::encode(&identity_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "mint");
    assert_eq!(json["data"][0]["blockNumber"], 100);

    // An unknown identity is a 404, not an empty page.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/bit-cell/items/0x{}/activities",
            hex::encode([0xEEu8; 32])
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "not_found");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains(".bit Cell identity not found"));
}

#[tokio::test]
async fn test_assets_bit_cell_collection_and_detail_keep_independent_identity() {
    let store = test_store();
    let collection_id = b"bit_cell_collection_____________".to_vec();
    let identity_id =
        hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab36345847129d0cc4135d1ffd4").unwrap();
    let account_id = hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab3").unwrap();
    let tx_hash = vec![0xc1; 32];
    let output_index = 1_i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit Cell".to_string()),
            standard: IdentityStandard::BitCell,
            total_count: 1,
            live_count: 1,
            holders_count: 1,
            activities_count: 1,
        },
    );
    batch.put_identity(
        &identity_id,
        &IdentityEntry {
            standard: IdentityStandard::BitCell,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("20240507.bit".to_string()),
            is_live: true,
            created_at_block: 13_184_726,
            created_at_tx: tx_hash.clone(),
            extra: IdentityExtra::BitCell {
                account_id,
                expired_at: 1_778_140_699,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &identity_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x41; 32],
            lock_hash_type: 1,
            lock_args: vec![0x51; 20],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x71; 32]),
            type_hash_type: Some(1),
            type_args: Some(Vec::new()),
            data_size: 72,
            occupied_capacity: 158_00000000,
            udt_amount: None,
            data_hash: None,
        },
        13_184_726,
    );
    batch.put_spore_outpoint(&tx_hash, output_index, &identity_id);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/identities/bit_cell/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(
        json["data"][0]["nftId"],
        format!("0x{}", hex::encode(&identity_id))
    );
    assert_eq!(json["data"][0]["standard"], "bit_cell");
    assert_eq!(json["data"][0]["name"], "20240507.bit");
    assert_eq!(json["data"][0]["expiredAt"], 1_778_140_699u64);
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&tx_hash))
    );
    assert_eq!(json["data"][0]["outputIndex"], output_index);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/bit-cell/items/0x{}",
            hex::encode(&identity_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "bit_cell");
    assert_eq!(json["name"], "20240507.bit");
    assert_eq!(json["txHash"], format!("0x{}", hex::encode(tx_hash)));
}

#[tokio::test]
async fn test_assets_nft_collection_items_dotbit_requires_outpoint_index_even_with_live_cell() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let dotbit_code_hash =
        hex::decode("4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918").unwrap();
    let nft_id = [0x66u8; 20];
    let nft_type_hash = compute_script_hash(&dotbit_code_hash, 1, &nft_id);
    let tx_hash = vec![0xabu8; 32];
    let output_index = 3i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("indexed.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(nft_type_hash.clone()),
            type_code_hash: Some(dotbit_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(nft_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
    );
    batch.put_cell_by_type(&nft_type_hash, 100, &tx_hash, output_index);
    // Intentionally no put_dotbit_account_outpoint(...): live cell exists but index is required.
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20")
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
async fn test_assets_nft_collection_items_dotbit_live_missing_outpoint_fails_fast() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let nft_id = [0x67u8; 20];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("broken.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_id);
    // Intentionally no outpoint index and no fallback-resolvable live cell.
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20")
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
    batch.put_mnft_collection_aggregate(
        &class_id,
        &MnftCollectionAggregate {
            name: Some("Genesis Class".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.put_mnft(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("Genesis Class".to_string()),
            description: None,
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 1000,
                issued: 1,
                configure: 7,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );
    batch.put_mnft(
        &token_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(vec![0x22; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 101,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftToken {
                token_index: 1,
                characteristic: vec![1, 2, 3, 4, 5, 6, 7, 8],
                configure: 3,
                state: 1,
            },
        },
    );
    batch.put_mnft_by_collection(&class_id, &token_id);
    batch.put_mnft_token_outpoint(&tx_hash, output_index, &token_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x62; 32]),
            type_hash_type: Some(1),
            type_args: Some(token_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        101,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/{}/items?limit=20",
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
async fn test_assets_nft_collection_holders_supports_pagination() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let nft_a = [0x81u8; 20];
    let nft_b = [0x82u8; 20];
    let nft_c = [0x83u8; 20];
    let nft_d = [0x84u8; 20];
    let owner_a = vec![0x11u8; 32];
    let owner_b = vec![0x22u8; 32];
    let owner_c = vec![0x33u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 4,
            live_count: 3,
            holders_count: 2,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_a,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_a.clone()),
            name: Some("alpha.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_b,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_a.clone()),
            name: Some("beta.bit".to_string()),
            is_live: true,
            created_at_block: 101,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_001),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_c,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_b.clone()),
            name: Some("gamma.bit".to_string()),
            is_live: true,
            created_at_block: 102,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_002),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_d,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_c),
            name: Some("dead.bit".to_string()),
            is_live: false,
            created_at_block: 103,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_003),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_a);
    batch.put_identity_by_collection(&collection_id, &nft_b);
    batch.put_identity_by_collection(&collection_id, &nft_c);
    batch.put_identity_by_collection(&collection_id, &nft_d);
    batch.put_identity_owner_count(&collection_id, &owner_a, 2);
    batch.put_identity_owner_count(&collection_id, &owner_b, 1);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/holders?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_a))
    );
    assert_eq!(json["data"][0]["itemCount"], 2);
    let next_cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/dotbit/holders?limit=1&cursor={next_cursor}"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_b))
    );
    assert_eq!(json["data"][0]["itemCount"], 1);
}

#[tokio::test]
async fn test_assets_nft_collection_activities_supports_action_filter() {
    let (core_store, append_only_store) = split_test_stores();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let account_id = [0x91u8; 20];
    let mint_tx = vec![0xa1; 32];
    let transfer_tx = vec![0xa2; 32];
    let burn_tx = vec![0xa3; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 0,
            holders_count: 0,
            activities_count: 0,
        },
    );
    core_batch.put_identity(
        &account_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: Some("burned.bit".to_string()),
            is_live: false,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    core_batch.put_identity_by_collection(&collection_id, &account_id);
    core_batch.put_dotbit_account_outpoint(&mint_tx, 0, &account_id);
    core_batch.put_dotbit_outpoint_by_account_id(&account_id, &mint_tx, 0);
    core_batch.put_dotbit_account_outpoint(&transfer_tx, 0, &account_id);
    core_batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx, 0);
    core_batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x41; 32],
            lock_hash_type: 1,
            lock_args: vec![0x51; 20],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x62; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    core_batch.put_consumed_cell_with_consumer(
        &transfer_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x32; 32],
            lock_code_hash: vec![0x42; 32],
            lock_hash_type: 1,
            lock_args: vec![0x52; 20],
            type_script_hash: Some(vec![0x63; 32]),
            type_code_hash: Some(vec![0x64; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        200,
        300,
        Some(&burn_tx),
    );
    core_batch.put_tx_hash_map(&mint_tx, 100, 0);
    core_batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_tx_hash_map(&transfer_tx, 200, 0);
    core_batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_tx_hash_map(&burn_tx, 300, 0);
    core_batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 1,
            outputs_count: 0,
            fee: 0,
            tx_size: 160,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0xB1; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_100,
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
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0xB2; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_200,
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
    core_batch.put_block_header(
        300,
        &CachedBlockHeader {
            hash: vec![0xB3; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_300,
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
    core_batch.commit().unwrap();

    let mut append_batch = StoreBatch::new(append_only_store.as_ref());
    append_batch.put_identity_collection_activity(
        &collection_id,
        100,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: mint_tx.clone(),
            block_hash: vec![0xB1; 32],
            timestamp_ms: 1_700_000_100,
            actions: vec![AssetAction::Mint],
        },
    );
    append_batch.put_identity_collection_activity(
        &collection_id,
        200,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: transfer_tx.clone(),
            block_hash: vec![0xB2; 32],
            timestamp_ms: 1_700_000_200,
            actions: vec![AssetAction::Transfer],
        },
    );
    append_batch.put_identity_collection_activity(
        &collection_id,
        300,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: burn_tx.clone(),
            block_hash: vec![0xB3; 32],
            timestamp_ms: 1_700_000_300,
            actions: vec![AssetAction::Burn],
        },
    );
    append_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/activities?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["data"][0]["actions"][0], "burn");
    assert_eq!(json["data"][1]["blockNumber"], 200);
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["blockNumber"], 100);
    assert_eq!(json["data"][2]["actions"][0], "mint");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/activities?limit=20&action=burn")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "burn");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/activities?action=invalid")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
    batch.put_mnft(
        &issuer_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x01; 32]),
            name: Some("Issuer-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 90,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftIssuer {
                class_count: 2,
                set_count: 3,
                info: Some(br#"{"name":"Issuer-A"}"#.to_vec()),
            },
        },
    );
    batch.put_mnft(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x02; 32]),
            name: Some("Class-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 95,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 500,
                issued: 128,
                configure: 9,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );
    batch.put_mnft(
        &token_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(vec![0x03; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 120,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftToken {
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
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x32; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x33; 32]),
            type_code_hash: Some(vec![0x34; 32]),
            type_hash_type: Some(1),
            type_args: Some(token_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        120,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}",
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
    let previous_owner_lock_hash = vec![0x66u8; 32];
    let mint_tx = vec![0x93; 32];
    let transfer_tx = vec![0x91; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_mnft(
        &token_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(owner_lock_hash.clone()),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 120,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftToken {
                token_index: 128,
                characteristic: vec![0xaa; 8],
                configure: 5,
                state: 2,
            },
        },
    );
    batch.put_mnft_token_outpoint(&mint_tx, 0, &token_id);
    batch.put_mnft_token_outpoint(&transfer_tx, 0, &token_id);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: previous_owner_lock_hash,
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: Some(vec![0x44; 32]),
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(token_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        300,
        Some(&transfer_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 300, 0);
    batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}/activities?limit=20",
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
            "/api/v1/assets/objects/items/0x{}/activities?limit=1",
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
            "/api/v1/assets/objects/items/0x{}/activities?limit=1&cursor={}",
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
            "/api/v1/assets/objects/items/0x{}/activities?limit=20&action=transfer",
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
            "/api/v1/assets/objects/items/0x{}/activities?action=invalid",
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
    let owner_a = vec![0x88u8; 32];
    let owner_b = vec![0x77u8; 32];
    let owner_c = vec![0x66u8; 32];
    let mint_tx = vec![0xa2; 32];
    let transfer_tx_1 = vec![0xa1; 32];
    let transfer_tx_2 = vec![0xa4; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &account_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_c.clone()),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 120,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_dotbit_account_outpoint(&mint_tx, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &mint_tx, 0);
    batch.put_dotbit_account_outpoint(&transfer_tx_1, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx_1, 0);
    batch.put_dotbit_account_outpoint(&transfer_tx_2, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx_2, 0);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_a,
            lock_code_hash: vec![0x31; 32],
            lock_hash_type: 1,
            lock_args: vec![0x32; 20],
            type_script_hash: Some(vec![0x33; 32]),
            type_code_hash: Some(vec![0x34; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        300,
        320,
        Some(&transfer_tx_1),
    );
    batch.put_consumed_cell_with_consumer(
        &transfer_tx_1,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_b,
            lock_code_hash: vec![0x41; 32],
            lock_hash_type: 1,
            lock_args: vec![0x42; 20],
            type_script_hash: Some(vec![0x43; 32]),
            type_code_hash: Some(vec![0x44; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        320,
        340,
        Some(&transfer_tx_2),
    );
    batch.put_tx_hash_map(&mint_tx, 300, 0);
    batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx_1, 320, 0);
    batch.put_tx_index(
        320,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_320,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx_2, 340, 0);
    batch.put_tx_index(
        340,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_340,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=20",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 340);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 320);
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["blockNumber"], 300);
    assert_eq!(json["data"][2]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=1",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 340);
    assert_eq!(json["hasMore"], true);
    let next_cursor = json["nextCursor"]
        .as_str()
        .expect("next cursor for dotbit activities");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=1&cursor={}",
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
    assert_eq!(json["data"][0]["blockNumber"], 320);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
}

#[tokio::test]
async fn test_assets_nft_item_activities_dotbit_recycled_has_burn_history() {
    let store = test_store();
    let account_id = [0x31u8; 20];
    let owner_a = vec![0x21u8; 32];
    let owner_b = vec![0x22u8; 32];
    let mint_tx = vec![0xb1; 32];
    let transfer_tx = vec![0xb2; 32];
    let burn_tx = vec![0xb3; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &account_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: Some("recycled.bit".to_string()),
            is_live: false,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_dotbit_account_outpoint(&mint_tx, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &mint_tx, 0);
    batch.put_dotbit_account_outpoint(&transfer_tx, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx, 0);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_a,
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![0x52; 20],
            type_script_hash: Some(vec![0x53; 32]),
            type_code_hash: Some(vec![0x54; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    batch.put_consumed_cell_with_consumer(
        &transfer_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_b,
            lock_code_hash: vec![0x61; 32],
            lock_hash_type: 1,
            lock_args: vec![0x62; 20],
            type_script_hash: Some(vec![0x63; 32]),
            type_code_hash: Some(vec![0x64; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        200,
        260,
        Some(&burn_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&burn_tx, 260, 0);
    batch.put_tx_index(
        260,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_260,
            inputs_count: 1,
            outputs_count: 0,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=20",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 260);
    assert_eq!(json["data"][0]["actions"][0], "burn");
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["actions"][0], "mint");
}

// ---------------------------------------------------------------------------
// `/assets` cursor strictness — the silent-page-1 family.
// ---------------------------------------------------------------------------

/// Seed two tokens so `/assets` has a stable two-row, one-page-per-row list.
fn seed_two_assets_for_cursor_tests() -> (Arc<CkbadgerStore>, [u8; 32], [u8; 32]) {
    let store = test_store();
    let token_a = [0x11u8; 32];
    let token_b = [0x22u8; 32];

    for (id, code_hash, arg, name, symbol) in [
        (&token_a, 0xAAu8, 0x01u8, "Alpha Token", "ALPHA"),
        (&token_b, 0xBBu8, 0x02u8, "Beta Token", "BETA"),
    ] {
        store
            .put_token_direct(
                id,
                &TokenInfo {
                    type_code_hash: vec![code_hash; 32],
                    hash_type: 1,
                    type_args: vec![arg; 20],
                    standard: "xudt".to_string(),
                    name: Some(name.to_string()),
                    symbol: Some(symbol.to_string()),
                    decimals: Some(8),
                    max_supply: None,
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
            &token_a,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_b,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 300,
                owned_knowledge_delta: 120,
            },
        )
        .unwrap();

    (store, token_a, token_b)
}

/// A cursor `/assets` cannot parse must be a 400, not page 1 again.
///
/// `parse_asset_cursor` returned `Option` and the caller consumed it as nested
/// `if let Some(..)`, so an unparseable cursor and a cursor naming no row both
/// fell through to "skip nothing". Verified live: `?limit=2&cursor=zzzz`
/// answered 200 with rows byte-identical to page 1, so a client paging with a
/// corrupted cursor loops on page 1 forever instead of learning it broke —
/// while `/tokens?limit=2&cursor=zzzz` correctly answered 400.
#[tokio::test]
async fn test_assets_reject_unparseable_cursor_instead_of_reserving_page_one() {
    let (store, _, _) = seed_two_assets_for_cursor_tests();
    let config = test_config(store);
    let app = create_router(config).await;

    let long_hex = format!("0x{}", "ab".repeat(600));
    for cursor in [
        "zzzz",                         // no separator at all
        "-1",                           // no separator, numeric
        "-1:0",                         // separator, but not an asset type
        "99999999999999999999999999:0", // separator, but not an asset type
        &long_hex,                      // 1200 hex chars, no separator
        "token:",                       // known type, empty id
        "spore:0xdeadbeef",             // unknown asset type
    ] {
        let (status, body) = get_json(&app, &format!("/assets?limit=2&cursor={cursor}")).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "GET /api/v1/assets?cursor={cursor} must reject an unparseable cursor with 400 \
             instead of silently re-serving page 1, got {status} body={body}"
        );
    }
}

/// A well-formed cursor naming a row that is not in the current result set is
/// also a 400 — same answer `/tokens` already gives (`tokens.rs` maps a missing
/// `position(..)` to `bad_request("Invalid token cursor")`). Resuming at page 1
/// would hand the client rows it has already seen and never terminate.
#[tokio::test]
async fn test_assets_reject_well_formed_cursor_that_names_no_row() {
    let (store, _, _) = seed_two_assets_for_cursor_tests();
    let config = test_config(store);
    let app = create_router(config).await;

    let unknown = format!("token:0x{}", "de".repeat(32));
    let (status, body) = get_json(&app, &format!("/assets?limit=2&cursor={unknown}")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "GET /api/v1/assets?cursor={unknown} names no row in the result set and must be \
         rejected rather than restarting pagination, got {status} body={body}"
    );
}

/// The strictness must not break the cursor the endpoint itself emits, and
/// `?cursor=` must stay page 1 like every other list route.
#[tokio::test]
async fn test_assets_round_trip_their_own_cursor_and_treat_empty_as_page_one() {
    let (store, token_a, token_b) = seed_two_assets_for_cursor_tests();
    let config = test_config(store);
    let app = create_router(config).await;

    let (status, page1) = get_json(&app, "/assets?type=token&limit=1&cursor=").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an empty cursor must mean page 1, got {status} body={page1}"
    );
    assert_eq!(
        page1["data"][0]["id"],
        format!("0x{}", hex::encode(token_b))
    );

    let next = page1["nextCursor"]
        .as_str()
        .expect("page 1 has a next cursor");
    let (status, page2) =
        get_json(&app, &format!("/assets?type=token&limit=1&cursor={next}")).await;
    assert_eq!(status, StatusCode::OK, "body={page2}");
    assert_eq!(
        page2["data"][0]["id"],
        format!("0x{}", hex::encode(token_a))
    );
    assert!(page2["nextCursor"].is_null());
}

/// Regression (Bug: spore-cluster ids accepted with permanently empty rows):
/// /assets/objects/{collection_id}/items resolves spore clusters for its
/// aggregate/total (CF_CLUSTER_AGG fallback) but sourced rows only from
/// list_mnft_ids_by_collection, so a spore cluster returned correct totals
/// with zero rows forever (live mainnet: "Chinese Mahjong"
/// 0xc22de62b3933f741e203714a189a2f468779e384fa33307fb9902d11aa648080,
/// total 138 / live 98 / recycled 40, data always []). Rows must come from
/// the same source as /spore/clusters/{id}/spores and agree with totals.
#[tokio::test]
async fn test_object_collection_items_serves_spore_cluster_rows_matching_totals() {
    let store = test_store();
    let cluster_id = [0xC2u8; 32];
    let cluster_id_hex = format!("0x{}", hex::encode(cluster_id));

    store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x11; 32]),
                name: Some("Test Mahjong".to_string()),
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0x10; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            total_count: 3,
            live_count: 2,
            owner_count: 2,
            pure_ckb_count: 2,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    // Three member spores: two live, one melted; distinct creation blocks so
    // the (created_at_block DESC, id ASC) order is observable.
    for (id_byte, block, is_live, name) in [
        (0xA1u8, 300i64, true, "tile-one"),
        (0xA2, 250, false, "tile-two"),
        (0xA3, 200, true, "tile-three"),
    ] {
        store
            .put_spore_direct(
                &[id_byte; 32],
                &ObjectEntry {
                    standard: ObjectStandard::Spore,
                    collection_id: Some(cluster_id.to_vec()),
                    token_id: None,
                    owner_lock_hash: Some(vec![id_byte; 32]),
                    name: Some(name.to_string()),
                    description: None,
                    is_live,
                    created_at_block: block,
                    created_at_tx: vec![id_byte ^ 0xFF; 32],
                    extra: ObjectExtra::Spore {
                        content_type: "dob/0".to_string(),
                        content_length: 8,
                        media_profile: SporeMediaProfile {
                            tier: CompositionTier::PureCkb,
                            sources: vec![],
                            issues: vec![],
                        },
                    },
                },
            )
            .unwrap();
    }

    let app = create_router(test_config(store)).await;

    // All items: rows must exist and agree with the aggregate total.
    let (status, json) = get_json(
        &app,
        &format!("/assets/objects/{cluster_id_hex}/items?limit=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["total"], 3);
    let rows = json["data"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        3,
        "spore-cluster rows must match the reported total, got {json}"
    );
    // Same order as /spore/clusters/{id}/spores: created_at_block DESC, id ASC.
    assert_eq!(rows[0]["nftId"], format!("0x{}", hex::encode([0xA1u8; 32])));
    assert_eq!(rows[1]["nftId"], format!("0x{}", hex::encode([0xA2u8; 32])));
    assert_eq!(rows[2]["nftId"], format!("0x{}", hex::encode([0xA3u8; 32])));
    assert_eq!(rows[0]["standard"], "spore");
    assert_eq!(rows[0]["name"], "tile-one");
    assert_eq!(rows[0]["isLive"], true);
    assert_eq!(rows[1]["isLive"], false);
    assert_eq!(
        rows[0]["txHash"],
        format!("0x{}", hex::encode([0xA1u8 ^ 0xFF; 32])),
        "spore rows carry the creation tx like the spore endpoints"
    );

    // Status filters: rows and totals stay in agreement.
    let (status, live) = get_json(
        &app,
        &format!("/assets/objects/{cluster_id_hex}/items?limit=20&status=live"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{live}");
    assert_eq!(live["total"], 2);
    assert_eq!(live["data"].as_array().unwrap().len(), 2);

    let (status, recycled) = get_json(
        &app,
        &format!("/assets/objects/{cluster_id_hex}/items?limit=20&status=recycled"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recycled}");
    assert_eq!(recycled["total"], 1);
    assert_eq!(recycled["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        recycled["data"][0]["nftId"],
        format!("0x{}", hex::encode([0xA2u8; 32]))
    );

    // Search narrows rows (count-less response like the mNFT branch).
    let (status, searched) = get_json(
        &app,
        &format!("/assets/objects/{cluster_id_hex}/items?limit=20&search=tile-three"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{searched}");
    assert_eq!(searched["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        searched["data"][0]["nftId"],
        format!("0x{}", hex::encode([0xA3u8; 32]))
    );

    // Pagination walks every row exactly once via the returned cursor.
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let path = match &cursor {
            Some(c) => format!("/assets/objects/{cluster_id_hex}/items?limit=1&cursor={c}"),
            None => format!("/assets/objects/{cluster_id_hex}/items?limit=1"),
        };
        let (status, page) = get_json(&app, &path).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        for row in page["data"].as_array().unwrap() {
            seen.push(row["nftId"].as_str().unwrap().to_string());
        }
        match page["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }
    assert_eq!(
        seen,
        vec![
            format!("0x{}", hex::encode([0xA1u8; 32])),
            format!("0x{}", hex::encode([0xA2u8; 32])),
            format!("0x{}", hex::encode([0xA3u8; 32])),
        ],
        "cursor pagination must walk all spore rows exactly once"
    );

    // A malformed cursor on the spore branch is a 400, never an empty page 1.
    let (status, _) = get_json(
        &app,
        &format!("/assets/objects/{cluster_id_hex}/items?limit=1&cursor=abc"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Task 5.1 — `/assets?type=token` after a REAL shallow fork.
///
/// Companion to `test_get_token_reports_pre_fork_capacity_after_real_writer_rollback`
/// on the warmup-cache path: the asset list's `ownedCapacity` comes from
/// `accumulate_owned_capacity` over the same `TOKEN_DAILY` rows, so a rollback
/// that deletes the cutoff day's bucket silently reports a token's whole
/// pre-fork history as zero. Both tokens' rows are written by the production
/// `BatchWriter` per-block writer, never by `put_token_daily_delta`.
///
/// `test_assets_list_token_errors_when_daily_deltas_invalid` stays as the
/// fail-fast counterpart: a genuinely invalid series must still 500.
#[tokio::test]
async fn test_assets_list_token_capacity_after_real_writer_rollback() {
    let store = test_store();
    let surviving = [0x71u8; 32];
    let orphan_only = [0x72u8; 32];

    for (hash, name, symbol) in [
        (surviving, "Surviving Token", "SRV"),
        (orphan_only, "Orphan Only Token", "ORP"),
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
                    max_supply: None,
                    first_seen_block: 1,
                    icon_url: None,
                    description: None,
                    transfers_count: 1,
                },
            )
            .unwrap();
    }

    let writer = BatchWriter::new(store.clone(), store.clone());

    // Blocks 1..=3: only the surviving token moves.
    for block in 1..=3i64 {
        commit_token_daily_blocks(
            &writer,
            &store,
            &[(block, surviving, 10_000_000_000, 6_100_000_000)],
        );
    }
    // Orphan block 4: a row the fork created from nothing, plus more of the
    // surviving token on the same UTC+8 day.
    commit_token_daily_blocks(
        &writer,
        &store,
        &[
            (4, surviving, 25_000_000_000, 13_000_000_000),
            (4, orphan_only, 9_000_000_000, 4_200_000_000),
        ],
    );

    rollback_entity_stats(&store, 3);

    let config = test_config(store);
    let app = create_router(config).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/assets?type=token&sort_key=capacity&sort_direction=desc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);

    let row_of = |hash: [u8; 32]| -> &serde_json::Value {
        let id = format!("0x{}", hex::encode(hash));
        rows.iter()
            .find(|row| row["id"] == serde_json::Value::String(id.clone()))
            .unwrap_or_else(|| panic!("asset row for {id} missing"))
    };

    assert_eq!(
        row_of(surviving)["ownedCapacity"],
        "30000000000",
        "the three blocks the fork never touched must survive the rollback"
    );
    assert_eq!(row_of(surviving)["ownedKnowledge"], "18300000000");
    assert_eq!(
        row_of(orphan_only)["ownedCapacity"],
        "0",
        "a row only the orphan ever created must be gone"
    );
    assert_eq!(row_of(orphan_only)["ownedKnowledge"], "0");
}

// ── `.cell` (DotCell) identity ────────────────────────────────────────────
//
// Ownership is a 20-byte lock-hash prefix in the cell's data, so every one of
// these asserts what the chain says and what the API could resolve from it —
// never a fabricated lock hash or address.

const DOTCELL_ACCOUNT_CODE_HASH_MAINNET: &str =
    "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54";
const DOTCELL_ACCOUNT_LOCK_CODE_HASH_MAINNET: &str =
    "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab";
const DOTCELL_SALE_LOCK_CODE_HASH_TESTNET: &str =
    "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4";
const DOTCELL_NAMESPACE_MAINNET: &str = "0xb4f4302965b7d6421481a520ee7eb5971a5e808c";
/// `blake2b("support")[..20]`, the real mainnet name id.
const SUPPORT_ID: &str = "0x62d71147ac82b83c8531126cacb0d2f072bfd94a";
/// The mainnet owner of `support.cell` — a 20-byte prefix, as stored on chain.
const SUPPORT_OWNER20: &str = "0x57d926a44d83fc13b21ce037b1e31f4223e3c867";
/// The full secp lock hash that prefix resolves to.
const SUPPORT_OWNER_LOCK_HASH: &str =
    "0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d";
const DOTCELL_EXPIRY: u64 = 1_821_507_678;

fn hex20(hex: &str) -> [u8; 20] {
    hex::decode(hex.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap()
}

fn hex32(hex: &str) -> [u8; 32] {
    hex::decode(hex.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap()
}

fn dotcell_extra(label: &str, owner20: [u8; 20], manager20: [u8; 20]) -> IdentityExtra {
    IdentityExtra::DotCell {
        label: label.to_string(),
        namespace_args: hex20(DOTCELL_NAMESPACE_MAINNET),
        layout_version: 3,
        expired_at: DOTCELL_EXPIRY,
        owner_hash20: owner20,
        manager_hash20: manager20,
        next_id: [0x65; 20],
        records_hash: [0x72; 32],
        records: Vec::new(),
        parent_id: None,
    }
}

/// Seed one `.cell` name, its collection rows and the tip header the expiry
/// state is measured against.
fn seed_dotcell_name(
    store: &Arc<CkbadgerStore>,
    id: [u8; 20],
    label: &str,
    owner20: [u8; 20],
    extra: IdentityExtra,
    is_live: bool,
    tip_timestamp_ms: i64,
) {
    let create_tx = vec![0xD1; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &id,
        &IdentityEntry {
            standard: IdentityStandard::DotCell,
            owner_lock_hash: None,
            name: Some(format!("{label}.cell")),
            is_live,
            created_at_block: 20_518_306,
            created_at_tx: create_tx.clone(),
            extra,
        },
    );
    batch.put_identity_by_collection(&ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION, &id);
    batch.put_identity_collection_aggregate(
        &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
        &IdentityCollectionAggregate {
            standard: IdentityStandard::DotCell,
            name: Some(".cell".to_string()),
            total_count: 1,
            live_count: if is_live { 1 } else { 0 },
            holders_count: if is_live { 1 } else { 0 },
            activities_count: 1,
        },
    );
    if is_live {
        batch.put_dotcell_name_by_owner(&owner20, &id);
        batch.put_identity_owner20_count(
            &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
            &owner20,
            1,
        );
        batch.put_spore_outpoint(&create_tx, 1, &id);
        let name_cell = LiveCellInfo {
            capacity: 240_00000000,
            lock_script_hash: vec![0x0A; 32],
            lock_code_hash: hex::decode(
                DOTCELL_ACCOUNT_LOCK_CODE_HASH_MAINNET.trim_start_matches("0x"),
            )
            .unwrap(),
            lock_hash_type: 1,
            lock_args: Vec::new(),
            type_script_hash: Some(vec![0x0B; 32]),
            type_code_hash: Some(
                hex::decode(DOTCELL_ACCOUNT_CODE_HASH_MAINNET.trim_start_matches("0x")).unwrap(),
            ),
            type_hash_type: Some(1),
            type_args: Some(hex20(DOTCELL_NAMESPACE_MAINNET).to_vec()),
            data_size: 105,
            occupied_capacity: 200_00000000,
            udt_amount: None,
            data_hash: None,
        };
        batch.put_cell_payload_by_outpoint(&create_tx, 1, &name_cell);
        batch.put_live_cell_marker_by_outpoint(&create_tx, 1, 20_518_306);
    }
    batch.put_block_header(
        20_518_306,
        &CachedBlockHeader {
            hash: vec![0xB0; 32],
            parent_hash: vec![0xB1; 32],
            timestamp: tip_timestamp_ms,
            epoch_number: 10,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0u8; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.commit().unwrap();
}

/// Make the owner prefix resolvable to a real secp lock.
fn seed_owner_lock(store: &Arc<CkbadgerStore>) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &hex32(SUPPORT_OWNER_LOCK_HASH),
        &ckbadger_store::types::LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0xE1; 20],
        },
    );
    batch.commit().unwrap();
}

async fn dotcell_get(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

#[tokio::test]
async fn test_assets_identities_dotcell_collection_aliases() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    let config = test_config(store);
    let app = create_router(config).await;

    for alias in [
        "dotcell",
        ".cell",
        &format!(
            "0x{}",
            hex::encode(ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION)
        ),
    ] {
        let (status, body) = dotcell_get(
            app.clone(),
            &format!("/api/v1/assets/identities/{}", alias.replace('.', "%2E")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "alias {alias}: {body}");
        assert_eq!(body["standard"], "dotcell", "alias {alias}");
        assert_eq!(body["liveCount"], 1);
    }
}

#[tokio::test]
async fn test_assets_dotcell_item_detail_by_id_and_by_name() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    seed_owner_lock(&store);
    let config = test_config(store);
    let app = create_router(config).await;

    let mut seen: Vec<serde_json::Value> = Vec::new();
    for reference in [SUPPORT_ID, "support", "support.cell"] {
        let (status, body) = dotcell_get(
            app.clone(),
            &format!("/api/v1/assets/identities/dotcell/items/{reference}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reference}: {body}");
        seen.push(body.clone());
    }
    assert_eq!(seen[0], seen[1], "a bare label resolves to the same name");
    assert_eq!(seen[1], seen[2], "the .cell suffix is optional");

    let item = &seen[0];
    assert_eq!(item["identityId"], SUPPORT_ID);
    assert_eq!(item["label"], "support");
    assert_eq!(item["name"], "support.cell");
    assert_eq!(item["isLive"], true);
    assert_eq!(item["expiredAt"], DOTCELL_EXPIRY);
    assert_eq!(item["state"], "active");
    assert_eq!(item["graceEndsAt"], DOTCELL_EXPIRY + 2_592_000);
    assert_eq!(item["owner"]["hashPrefix"], SUPPORT_OWNER20);
    assert_eq!(item["owner"]["lockHash"], SUPPORT_OWNER_LOCK_HASH);
    assert!(
        item["owner"]["address"]
            .as_str()
            .unwrap()
            .starts_with("ckb"),
        "{item}"
    );
    assert_eq!(item["manager"]["hashPrefix"], SUPPORT_OWNER20);
    assert_eq!(item["records"].as_array().unwrap().len(), 0);
    assert_eq!(item["parent"], serde_json::Value::Null);
    assert_eq!(item["children"].as_array().unwrap().len(), 0);
    assert_eq!(item["liveOutPoint"]["index"], 1);
    assert_eq!(item["sale"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_assets_dotcell_item_state_grace_free_and_recycled() {
    for (tip_secs, expected) in [
        (DOTCELL_EXPIRY - 10, "active"),
        (DOTCELL_EXPIRY + 10 * 86_400, "grace"),
        (DOTCELL_EXPIRY + 40 * 86_400, "free"),
    ] {
        let store = test_store();
        seed_dotcell_name(
            &store,
            hex20(SUPPORT_ID),
            "support",
            hex20(SUPPORT_OWNER20),
            dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
            true,
            (tip_secs * 1000) as i64,
        );
        let config = test_config(store);
        let app = create_router(config).await;
        let (status, body) =
            dotcell_get(app, "/api/v1/assets/identities/dotcell/items/support").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["state"], expected, "tip={tip_secs}");
    }

    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        false,
        1_700_000_000_000,
    );
    let config = test_config(store);
    let app = create_router(config).await;
    let (_, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items/support").await;
    assert_eq!(body["state"], "recycled");
    assert_eq!(body["liveOutPoint"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_assets_dotcell_item_sale_state_resolves_through_sale_lock() {
    let store = test_store();
    // The Sale Lock instance: args = seller lock hash ‖ price (u64 LE).
    let seller32 = hex32(SUPPORT_OWNER_LOCK_HASH);
    let mut sale_args = seller32.to_vec();
    sale_args.extend_from_slice(&10_000_000_000u64.to_le_bytes());
    let sale_lock = ckbadger_store::types::LockScriptEntry {
        code_hash: hex::decode(DOTCELL_SALE_LOCK_CODE_HASH_TESTNET.trim_start_matches("0x"))
            .unwrap(),
        hash_type: 1,
        args: sale_args,
    };
    let sale_lock_hash = compute_script_hash(&sale_lock.code_hash, 1, &sale_lock.args);
    let sale20: [u8; 20] = sale_lock_hash[..20].try_into().unwrap();

    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        sale20,
        dotcell_extra("support", sale20, sale20),
        true,
        1_700_000_000_000,
    );
    seed_owner_lock(&store);
    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_lock_script(&sale_lock_hash, &sale_lock);
        batch.commit().unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items/support").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let sale = &body["sale"];
    assert_eq!(sale["priceShannons"], "10000000000");
    assert_eq!(sale["seller"]["lockHash"], SUPPORT_OWNER_LOCK_HASH);
    assert!(sale["seller"]["address"]
        .as_str()
        .unwrap()
        .starts_with("ckb"));
    assert_eq!(
        body["owner"]["hashPrefix"],
        format!("0x{}", hex::encode(sale20)),
        "the chain says a Sale Lock instance owns the name while it is listed"
    );
}

/// A listed name whose Sale Lock args do not decode is an error, not a name
/// quietly reported as not for sale.
#[tokio::test]
async fn test_assets_dotcell_malformed_sale_lock_args_is_500() {
    let store = test_store();
    let sale_lock = ckbadger_store::types::LockScriptEntry {
        code_hash: hex::decode(DOTCELL_SALE_LOCK_CODE_HASH_TESTNET.trim_start_matches("0x"))
            .unwrap(),
        hash_type: 1,
        // 39 bytes: one short of `seller32 ‖ price u64 LE`.
        args: vec![0x11; 39],
    };
    let sale_lock_hash = compute_script_hash(&sale_lock.code_hash, 1, &sale_lock.args);
    let sale20: [u8; 20] = sale_lock_hash[..20].try_into().unwrap();

    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        sale20,
        dotcell_extra("support", sale20, sale20),
        true,
        1_700_000_000_000,
    );
    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_lock_script(&sale_lock_hash, &sale_lock);
        batch.commit().unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items/support").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("sale lock args"),
        "{body}"
    );
}

/// An owner prefix that resolves to an ordinary lock is simply not for sale —
/// the two cases are distinguished by what the lock IS, not by a read failing.
#[tokio::test]
async fn test_assets_dotcell_owner_on_an_ordinary_lock_is_not_a_sale() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    seed_owner_lock(&store);
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items/support").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sale"], serde_json::Value::Null);
    assert_eq!(body["owner"]["lockHash"], SUPPORT_OWNER_LOCK_HASH);
}

#[tokio::test]
async fn test_assets_dotcell_unresolved_owner_prefix_is_reported_not_fabricated() {
    let store = test_store();
    let unknown = hex20("0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019");
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        unknown,
        dotcell_extra("support", unknown, unknown),
        true,
        1_700_000_000_000,
    );
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items/support").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["owner"]["hashPrefix"],
        "0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019"
    );
    assert_eq!(body["owner"]["lockHash"], serde_json::Value::Null);
    assert_eq!(body["owner"]["address"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_assets_dotcell_records_decode_ckb_address_values() {
    let store = test_store();
    let extra = IdentityExtra::DotCell {
        label: "maria".to_string(),
        namespace_args: hex20(DOTCELL_NAMESPACE_MAINNET),
        layout_version: 3,
        expired_at: DOTCELL_EXPIRY,
        owner_hash20: hex20(SUPPORT_OWNER20),
        manager_hash20: hex20(SUPPORT_OWNER20),
        next_id: [0x65; 20],
        records_hash: [0x3b; 32],
        records: vec![
            ckbadger_store::types::DotCellRecord {
                key: "address.309".to_string(),
                label: String::new(),
                value: b"ckt1qrfrwcdnvssswdwpn3s9v8fp87emat306ctjwsm3nmlkjg8qyza2cqgqq9x75zu4l7gld606r6eyd00m4lzy3zkxkq4nywzu".to_vec(),
                ttl: 300,
            },
            ckbadger_store::types::DotCellRecord {
                key: "profile.email".to_string(),
                label: String::new(),
                value: b"maria@example.com".to_vec(),
                ttl: 300,
            },
        ],
        parent_id: None,
    };
    let maria_id = hex20("0x2224948f63975a7a0741139cd5d2a45b9fb02c03");
    seed_dotcell_name(
        &store,
        maria_id,
        "maria",
        hex20(SUPPORT_OWNER20),
        extra,
        true,
        1_700_000_000_000,
    );
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items/maria").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["key"], "address.309");
    assert_eq!(records[0]["ttl"], 300);
    assert_eq!(
        records[0]["valueUtf8"],
        "ckt1qrfrwcdnvssswdwpn3s9v8fp87emat306ctjwsm3nmlkjg8qyza2cqgqq9x75zu4l7gld606r6eyd00m4lzy3zkxkq4nywzu"
    );
    assert_eq!(
        records[0]["decodedAddress"]["address"],
        "ckt1qrfrwcdnvssswdwpn3s9v8fp87emat306ctjwsm3nmlkjg8qyza2cqgqq9x75zu4l7gld606r6eyd00m4lzy3zkxkq4nywzu"
    );
    assert!(records[0]["decodedAddress"]["lockHash"]
        .as_str()
        .unwrap()
        .starts_with("0x"));
    assert_eq!(records[1]["decodedAddress"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_assets_dotcell_holders_are_owner20() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    seed_owner_lock(&store);
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/holders").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["ownerHashPrefix"], SUPPORT_OWNER20);
    assert_eq!(rows[0]["itemCount"], 1);
    assert_eq!(
        rows[0]["lockScriptHash"], SUPPORT_OWNER_LOCK_HASH,
        "a resolvable prefix still reports the full hash"
    );
}

#[tokio::test]
async fn test_assets_dotcell_children_of_parent() {
    let store = test_store();
    let parent_id = hex20("0x4144e782dfaadeeb07625e11e4b6de717893aacb");
    let child_id = hex20("0xbb008a3e9045554d5b1b609c072b59b404320f9f");
    seed_dotcell_name(
        &store,
        parent_id,
        "v3-first-name",
        hex20(SUPPORT_OWNER20),
        dotcell_extra(
            "v3-first-name",
            hex20(SUPPORT_OWNER20),
            hex20(SUPPORT_OWNER20),
        ),
        true,
        1_700_000_000_000,
    );
    {
        let extra = dotcell_extra(
            "shop.v3-first-name",
            hex20(SUPPORT_OWNER20),
            hex20(SUPPORT_OWNER20),
        );
        let extra = match extra {
            IdentityExtra::DotCell {
                label,
                namespace_args,
                layout_version,
                expired_at,
                owner_hash20,
                manager_hash20,
                next_id,
                records_hash,
                records,
                ..
            } => IdentityExtra::DotCell {
                label,
                namespace_args,
                layout_version,
                expired_at,
                owner_hash20,
                manager_hash20,
                next_id,
                records_hash,
                records,
                parent_id: Some(parent_id),
            },
            other => other,
        };
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_identity(
            &child_id,
            &IdentityEntry {
                standard: IdentityStandard::DotCell,
                owner_lock_hash: None,
                name: Some("shop.v3-first-name.cell".to_string()),
                is_live: true,
                created_at_block: 20_518_307,
                created_at_tx: vec![0xD2; 32],
                extra,
            },
        );
        batch.put_identity_by_collection(&ckbadger_store::keys::pad_id_32(&parent_id), &child_id);
        batch.put_identity_by_collection(
            &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
            &child_id,
        );
        batch.commit().unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(
        app.clone(),
        "/api/v1/assets/identities/dotcell/items/v3-first-name",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let children = body["children"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["label"], "shop.v3-first-name");
    assert_eq!(children[0]["name"], "shop.v3-first-name.cell");

    let (_, child) = dotcell_get(
        app,
        "/api/v1/assets/identities/dotcell/items/shop.v3-first-name",
    )
    .await;
    assert_eq!(child["parent"]["label"], "v3-first-name");
}

#[tokio::test]
async fn test_assets_dotcell_ring_endpoint() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_dotcell_ring(
            &hex20(DOTCELL_NAMESPACE_MAINNET),
            &ckbadger_store::types::DotCellRingRoot {
                root_tx_hash: vec![0xA0; 32],
                root_output_index: 0,
                first_id: hex20(SUPPORT_ID),
                created_at_block: 20_515_882,
            },
        );
        batch.commit().unwrap();
    }
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/ring").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["namespaceArgs"], DOTCELL_NAMESPACE_MAINNET);
    assert_eq!(body["firstId"], SUPPORT_ID);
    assert_eq!(body["rootOutPoint"]["index"], 0);
    assert_eq!(body["liveCount"], 1);
}

#[tokio::test]
async fn test_assets_dotcell_item_not_found_and_bad_name() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, _) = dotcell_get(
        app.clone(),
        "/api/v1/assets/identities/dotcell/items/nosuchname",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = dotcell_get(
        app,
        "/api/v1/assets/identities/dotcell/items/Not%20A%20Name%21",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_assets_dotcell_items_listing() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    let config = test_config(store);
    let app = create_router(config).await;
    let (status, body) = dotcell_get(app, "/api/v1/assets/identities/dotcell/items").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["nftId"], SUPPORT_ID);
    assert_eq!(rows[0]["name"], "support.cell");
    assert_eq!(rows[0]["standard"], "dotcell");
    assert_eq!(rows[0]["expiredAt"], DOTCELL_EXPIRY);
    assert_eq!(
        rows[0]["ownerLockHash"],
        serde_json::Value::Null,
        "the chain gives a 20-byte prefix, so there is no lock hash to report here"
    );
    assert_eq!(rows[0]["outputIndex"], 1);
}

/// `.cell` collection activities are written to the identity CF by both
/// indexer paths; the collection feed must read them from there. The sentinel
/// was once missing from the identity set, so this feed read the object CF and
/// was always empty while the detail reported `activitiesCount > 0`.
#[tokio::test]
async fn dotcell_collection_activities_are_served_from_the_identity_cf() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    let register_tx = vec![0xC7; 32];
    let block_hash = vec![0xB7; 32];
    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_tx_hash_map(&register_tx, 700, 0);
        batch.put_tx_index(
            700,
            0,
            &TxIndexEntry {
                is_cellbase: false,
                timestamp: 1_700_000_700,
                inputs_count: 1,
                outputs_count: 2,
                fee: 0,
                tx_size: 400,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_block_header(
            700,
            &CachedBlockHeader {
                hash: block_hash.clone(),
                parent_hash: vec![0u8; 32],
                timestamp: 1_700_000_700,
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
        batch.put_identity_collection_activity(
            &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
            700,
            0,
            &ObjectCollectionActivityEntry {
                tx_hash: register_tx.clone(),
                block_hash,
                timestamp_ms: 1_700_000_700,
                actions: vec![AssetAction::Mint],
            },
        );
        batch.commit().unwrap();
    }
    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/assets/identities/dotcell/activities",
        "/api/v1/assets/objects/dotcell/activities",
    ] {
        let (status, body) = dotcell_get(app.clone(), uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        let rows = body["data"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{uri}: {body}");
        assert_eq!(
            rows[0]["txHash"],
            format!("0x{}", hex::encode(&register_tx))
        );
        assert_eq!(rows[0]["blockNumber"], 700);
        assert_eq!(rows[0]["actions"][0], "mint");
    }
}

/// The `.cell` collection page's capacity chart asks for
/// `/assets/objects/dotcell/charts/capacity-history`; the alias must resolve to
/// the `.cell` sentinel and the chart read its daily rows.
#[tokio::test]
async fn dotcell_capacity_history_chart_resolves_the_alias() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    store
        .put_mnft_daily_delta(
            &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
            20240115,
            &MnftDailyDelta {
                owned_capacity_delta: 240_00000000,
                owned_knowledge_delta: 200_00000000,
            },
        )
        .unwrap();
    let config = test_config(store);
    let app = create_router(config).await;

    for alias in ["dotcell", "%2Ecell", "DOTCELL"] {
        let (status, body) = dotcell_get(
            app.clone(),
            &format!("/api/v1/assets/objects/{alias}/charts/capacity-history"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{alias}: {body}");
        assert_eq!(body["title"], ".cell Capacity History", "{alias}");
        assert_eq!(body["data"][0]["date"], "2024-01-15", "{alias}");
        assert_eq!(body["data"][0]["values"]["used"], "20000000000", "{alias}");
        assert_eq!(body["data"][0]["values"]["unused"], "4000000000", "{alias}");
    }
}

/// A malformed `.cell` holder row (an owner segment that is not a 20-byte
/// prefix padded with zeros) is a store invariant violation the API reports as
/// a 500 naming the row. It must never reach an `assert!` inside the handler:
/// the release profile aborts on panic, so that took the whole API down.
#[tokio::test]
async fn dotcell_malformed_holder_row_is_a_500_not_an_abort() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    {
        // A full 32-byte lock hash where the chain only ever gives 20 bytes.
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_identity_owner_count(
            &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
            &[0x77; 32],
            5,
        );
        batch.commit().unwrap();
    }
    let config = test_config(store);
    let app = create_router(config).await;

    let (status, body) =
        dotcell_get(app.clone(), "/api/v1/assets/identities/dotcell/holders").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains(&hex::encode(
            ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION
        )),
        "names the collection: {message}"
    );
    assert!(
        message.contains(&"77".repeat(32)),
        "names the segment: {message}"
    );

    // The process is still serving.
    let (status, _) = dotcell_get(app, "/api/v1/assets/identities/dotcell").await;
    assert_eq!(status, StatusCode::OK);
}

/// Search lowercases a `.cell` label, so the item endpoint must too: the link
/// a search hit produces, and any name typed with capitals, resolves.
#[tokio::test]
async fn dotcell_item_ref_is_case_insensitive() {
    let store = test_store();
    seed_dotcell_name(
        &store,
        hex20(SUPPORT_ID),
        "support",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("support", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    {
        // The registration transaction, so the per-item feed can place it.
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_tx_hash_map(&[0xD1; 32], 20_518_306, 0);
        batch.put_tx_index(
            20_518_306,
            0,
            &TxIndexEntry {
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 2,
                fee: 0,
                tx_size: 400,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.commit().unwrap();
    }
    let config = test_config(store);
    let app = create_router(config).await;

    for reference in ["SUPPORT.cell", "Support", "support.CELL"] {
        let (status, body) = dotcell_get(
            app.clone(),
            &format!("/api/v1/assets/identities/dotcell/items/{reference}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reference}: {body}");
        assert_eq!(body["identityId"], SUPPORT_ID, "{reference}");
        let (status, body) = dotcell_get(
            app.clone(),
            &format!("/api/v1/assets/identities/dotcell/items/{reference}/activities"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reference} activities: {body}");
    }
}

/// Seed one `.cell` sub-name under `parent_id`, with its parent→child row.
fn seed_dotcell_child(
    store: &Arc<CkbadgerStore>,
    parent_id: [u8; 20],
    label: &str,
    is_live: bool,
) -> [u8; 20] {
    let child_id = ckbadger_store::types::derive_dotcell_id(label);
    let extra = match dotcell_extra(label, hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)) {
        IdentityExtra::DotCell {
            label,
            namespace_args,
            layout_version,
            expired_at,
            owner_hash20,
            manager_hash20,
            next_id,
            records_hash,
            records,
            ..
        } => IdentityExtra::DotCell {
            label,
            namespace_args,
            layout_version,
            expired_at,
            owner_hash20,
            manager_hash20,
            next_id,
            records_hash,
            records,
            parent_id: Some(parent_id),
        },
        other => other,
    };
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &child_id,
        &IdentityEntry {
            standard: IdentityStandard::DotCell,
            owner_lock_hash: None,
            name: Some(format!("{label}.cell")),
            is_live,
            created_at_block: 20_518_307,
            created_at_tx: vec![0xD2; 32],
            extra,
        },
    );
    batch.put_identity_by_collection(&ckbadger_store::keys::pad_id_32(&parent_id), &child_id);
    batch.put_identity_by_collection(
        &ckbadger_store::types::DOTCELL_SENTINEL_COLLECTION,
        &child_id,
    );
    batch.commit().unwrap();
    child_id
}

/// The parent→child index keeps every sub-name ever registered (a recycled one
/// included); the API lists the live ones, a page at a time, and says when
/// there are more — never a silent cut at 200.
#[tokio::test]
async fn dotcell_children_are_live_only_and_paged() {
    let store = test_store();
    let parent_id = ckbadger_store::types::derive_dotcell_id("alice");
    seed_dotcell_name(
        &store,
        parent_id,
        "alice",
        hex20(SUPPORT_OWNER20),
        dotcell_extra("alice", hex20(SUPPORT_OWNER20), hex20(SUPPORT_OWNER20)),
        true,
        1_700_000_000_000,
    );
    let recycled = seed_dotcell_child(&store, parent_id, "gone.alice", false);
    let mut live: Vec<String> = (0..201)
        .map(|i| {
            let id = seed_dotcell_child(&store, parent_id, &format!("s{i}.alice"), true);
            format!("0x{}", hex::encode(id))
        })
        .collect();
    live.sort();
    let recycled_hex = format!("0x{}", hex::encode(recycled));

    let config = test_config(store);
    let app = create_router(config).await;

    // The detail carries the first page and says there is more.
    let (status, body) =
        dotcell_get(app.clone(), "/api/v1/assets/identities/dotcell/items/alice").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let children = body["children"].as_array().unwrap();
    assert_eq!(children.len(), 50);
    assert_eq!(body["childrenHasMore"], true);
    assert_eq!(body["childrenNextCursor"], children[49]["identityId"]);

    // The children endpoint pages the rest: 200, then the last one.
    let (status, first) = dotcell_get(
        app.clone(),
        "/api/v1/assets/identities/dotcell/items/alice/children?limit=200",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first_rows = first["data"].as_array().unwrap();
    assert_eq!(first_rows.len(), 200);
    assert_eq!(first["hasMore"], true);
    let cursor = first["nextCursor"].as_str().unwrap().to_string();

    let (status, second) = dotcell_get(
        app.clone(),
        &format!(
            "/api/v1/assets/identities/dotcell/items/alice/children?limit=200&cursor={cursor}"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let second_rows = second["data"].as_array().unwrap();
    assert_eq!(second_rows.len(), 1);
    assert_eq!(second["hasMore"], false);
    assert_eq!(second["nextCursor"], serde_json::Value::Null);

    let mut seen: Vec<String> = first_rows
        .iter()
        .chain(second_rows)
        .map(|row| row["identityId"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !seen.contains(&recycled_hex),
        "a recycled sub-name is not a child"
    );
    seen.sort();
    assert_eq!(seen, live, "every live child exactly once");

    // The limit is bounded, and a malformed cursor is the caller's error.
    let (status, capped) = dotcell_get(
        app.clone(),
        "/api/v1/assets/identities/dotcell/items/alice/children?limit=5000",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(capped["data"].as_array().unwrap().len(), 200);
    let (status, _) = dotcell_get(
        app,
        "/api/v1/assets/identities/dotcell/items/alice/children?cursor=0x1234",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
