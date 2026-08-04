mod common;
use common::*;

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
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
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
async fn test_search_pending_transaction_hash_returns_transaction_result() {
    let store = test_store();
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/search?q={hash}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["resultType"], "transaction");
    assert_eq!(results[0]["id"], hash);
    assert_eq!(results[0]["url"], format!("/tx/{hash}"));
    assert_eq!(results[0]["label"], "Pending Transaction");
    assert_eq!(results[0]["matchKind"], "exact_hash");
    assert_eq!(json["ambiguous"], false);
}

#[tokio::test]
async fn test_search_name_matches_script_token_and_cluster_assets() {
    let store = test_store();

    let script_hash = vec![0x31; 32];
    let token_hash = vec![0x32; 32];
    let popular_token_hash = vec![0x34; 32];
    let cluster_id = vec![0x33; 32];
    let popular_cluster_id = vec![0x35; 32];

    store
        .put_script_version(
            &script_hash,
            &ScriptVersionInfo {
                version_hash: script_hash.clone(),
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
                max_supply: None,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_direct(
            &popular_token_hash,
            &TokenInfo {
                type_code_hash: vec![0x45; 32],
                hash_type: 1,
                type_args: vec![0x56; 32],
                standard: "xudt".to_string(),
                name: Some("Alpha Popular Token".to_string()),
                symbol: Some("ALPHA2".to_string()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_token_holder(&token_hash, &[0x61; 32], 100u128);
    batch.put_token_holder(&popular_token_hash, &[0x62; 32], 100u128);
    batch.put_token_holder(&popular_token_hash, &[0x63; 32], 100u128);
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: Some("Alpha Cluster".to_string()),
            description: None,
            total_count: 10,
            live_count: 8,
            owner_count: 2,
            ..Default::default()
        },
    );
    batch.put_cluster_aggregate(
        &popular_cluster_id,
        &ClusterAggregate {
            name: Some("Alpha Popular Cluster".to_string()),
            description: None,
            total_count: 20,
            live_count: 16,
            owner_count: 4,
            ..Default::default()
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
    let expected_script_url = "/scripts/Alpha%20Lock".to_string();
    let expected_token_url = format!("/tokens/0x{}", hex::encode(&token_hash));
    let expected_cluster_url = format!("/clusters/0x{}", hex::encode(&cluster_id));
    let expected_token_order = [
        format!("0x{}", hex::encode(&popular_token_hash)),
        format!("0x{}", hex::encode(&token_hash)),
    ];
    let expected_cluster_order = [
        format!("0x{}", hex::encode(&popular_cluster_id)),
        format!("0x{}", hex::encode(&cluster_id)),
    ];

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

    let token_order: Vec<_> = rows
        .iter()
        .filter(|row| row["resultType"] == "token")
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(token_order, expected_token_order);

    let cluster_order: Vec<_> = rows
        .iter()
        .filter(|row| row["resultType"] == "cluster")
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(cluster_order, expected_cluster_order);
}

#[tokio::test]
async fn test_search_exact_spore_hash_falls_back_to_cluster_name() {
    // A spore cell carries no name of its own (ObjectEntry.name is always None for
    // spores). The detail page shows the owning cluster's name, so the global search
    // dropdown must use the same single naming path instead of showing "Unnamed spore".
    let store = test_store();
    let cluster_id = [0xc1u8; 32];
    let spore_id = [0xa6u8; 32];

    store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x11; 32]),
                name: Some("Cosmic Repository".to_string()),
                description: None,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x22; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    store
        .put_spore_direct(
            &spore_id,
            &ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(cluster_id.to_vec()),
                token_id: None,
                owner_lock_hash: Some(vec![0x33; 32]),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 2,
                created_at_tx: vec![0x44; 32],
                extra: ObjectExtra::Spore {
                    content_type: "image/png".to_string(),
                    content_length: 3623,
                    media_profile: SporeMediaProfile::default(),
                },
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let spore_id_hex = format!("0x{}", hex::encode(spore_id));
    let request = Request::builder()
        .uri(format!("/api/v1/search?q={}", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["results"].as_array().unwrap();

    let spore_row = rows
        .iter()
        .find(|row| row["resultType"].as_str() == Some("spore"))
        .expect("expected a spore search result");

    let short = format!(
        "{}...{}",
        &spore_id_hex[..6],
        &spore_id_hex[spore_id_hex.len() - 4..]
    );
    assert_eq!(
        spore_row["label"].as_str().unwrap(),
        format!("Cosmic Repository#{}", short)
    );
    assert!(!spore_row["label"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("unnamed"));
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
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
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
async fn test_search_exact_script_hash_uses_reference_version_resolution() {
    let store = test_store();
    let reference_hash = vec![0x93; 32];

    store
        .put_script_info_direct(
            &reference_hash,
            &ScriptInfo {
                code_hash: reference_hash.clone(),
                hash_type: 0,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &reference_hash,
        &ScriptVersionInfo {
            version_hash: reference_hash.clone(),
            name: Some("SearchableScript".to_string()),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let request = Request::builder()
        .uri(format!("/api/v1/search?q={}", reference_hash_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let script_result = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["resultType"] == "script")
        .expect("script result");
    assert_eq!(
        script_result["url"],
        format!("/script/{}", reference_hash_hex)
    );
    assert_eq!(script_result["label"], "Script SearchableScript");
}
