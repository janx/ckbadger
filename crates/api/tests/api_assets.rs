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
