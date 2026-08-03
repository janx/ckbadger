mod common;
use common::*;

#[tokio::test]
async fn test_spore_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/spore/objects")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_cluster_capacity_chart_and_cluster_capacity_fields() {
    let store = test_store();
    let cluster_id = [0x42u8; 32];
    let cluster_id_hex = format!("0x{}", hex::encode(cluster_id));

    let cluster_entry = ObjectEntry {
        standard: ObjectStandard::SporeCluster,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("Test Cluster".to_string()),
        description: None,
        is_live: true,
        created_at_block: 123,
        created_at_tx: vec![0x22; 32],
        extra: ObjectExtra::SporeCluster,
    };
    store.put_spore_direct(&cluster_id, &cluster_entry).unwrap();
    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240115,
            &ClusterDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240117,
            &ClusterDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();

    // Write aggregate with cumulative totals (sum of daily deltas: 100-20=80, 60-10=50)
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            owned_capacity: 80,
            owned_knowledge: 50,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/{}/charts/capacity-history",
            cluster_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Test Cluster Capacity History");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
    assert_eq!(data[1]["values"]["used"], "60");
    assert_eq!(data[1]["values"]["unused"], "40");
    assert_eq!(data[2]["values"]["used"], "50");
    assert_eq!(data[2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
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
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");

    let request = Request::builder()
        .uri(format!("/api/v1/spore/clusters/{}", cluster_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ownedCapacity"], "80");
    assert_eq!(json["ownedKnowledge"], "50");
    assert_eq!(json["composition"]["tier"], "unknown");
}

#[tokio::test]
async fn test_spore_cluster_holders_supports_pagination() {
    let store = test_store();
    let cluster_id = [0x52u8; 32];
    let owner_a = [0x11u8; 32];
    let owner_b = [0x22u8; 32];

    store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(owner_a.to_vec()),
                name: Some("Holders Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 88,
                created_at_tx: vec![0x33; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_owner_count(&cluster_id, &owner_a, 3);
    batch.put_cluster_owner_count(&cluster_id, &owner_b, 1);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/holders?limit=1",
            hex::encode(cluster_id)
        ))
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
    assert_eq!(json["data"][0]["itemCount"], 3);
    let next_cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/holders?limit=1&cursor={}",
            hex::encode(cluster_id),
            next_cursor
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
async fn test_spore_cluster_activities_supports_action_filter() {
    let (core_store, append_only_store) = split_test_stores();
    let cluster_id = [0x62u8; 32];
    let mint_tx = vec![0x91; 32];
    let transfer_tx = vec![0x92; 32];
    let burn_tx = vec![0x93; 32];

    // Register cluster so existence check passes
    core_store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x21; 32]),
                name: Some("Activities Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 80,
                created_at_tx: vec![0x31; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    // Write pre-computed collection activities (the index the handler now reads)
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&mint_tx, 100, 0);
    core_batch.put_tx_hash_map(&transfer_tx, 200, 0);
    core_batch.put_tx_hash_map(&burn_tx, 300, 0);
    core_batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0xA1; 32],
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
            hash: vec![0xA2; 32],
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
            hash: vec![0xA3; 32],
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
    append_batch.put_object_collection_activity(
        &cluster_id,
        100,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: mint_tx.clone(),
            block_hash: vec![0xA1; 32],
            timestamp_ms: 1_700_000_100,
            actions: vec![AssetAction::Mint],
        },
    );
    append_batch.put_object_collection_activity(
        &cluster_id,
        200,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: transfer_tx.clone(),
            block_hash: vec![0xA2; 32],
            timestamp_ms: 1_700_000_200,
            actions: vec![AssetAction::Transfer],
        },
    );
    append_batch.put_object_collection_activity(
        &cluster_id,
        300,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: burn_tx.clone(),
            block_hash: vec![0xA3; 32],
            timestamp_ms: 1_700_000_300,
            actions: vec![AssetAction::Burn],
        },
    );
    append_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    // All activities — newest first
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/activities?limit=20",
            hex::encode(cluster_id)
        ))
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

    // Action filter
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/activities?limit=20&action=transfer",
            hex::encode(cluster_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");

    // Invalid action filter
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/activities?action=invalid",
            hex::encode(cluster_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_spore_capacity_chart_and_spore_capacity_fields() {
    let store = test_store();
    let spore_id = [0x77u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 1024,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();
    store
        .put_spore_daily_delta(
            &spore_id,
            20240115,
            &SporeDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 61,
            },
        )
        .unwrap();
    store
        .put_spore_daily_delta(
            &spore_id,
            20240117,
            &SporeDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -11,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/objects/{}/charts/capacity-history",
            spore_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Spore Capacity History");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[1]["values"]["used"], "61");
    assert_eq!(data[1]["values"]["unused"], "39");
    assert_eq!(data[2]["values"]["used"], "50");
    assert_eq!(data[2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/objects/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
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
    assert_eq!(data[0]["values"]["used"], "61");
    assert_eq!(data[0]["values"]["unused"], "39");

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ownedCapacity"], "80");
    assert_eq!(json["ownedKnowledge"], "50");
}

#[tokio::test]
async fn test_spore_decode_endpoint_returns_pending_without_cached_result() {
    let store = test_store();
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 128,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "pending");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "dob/0");
    assert_eq!(json["traits"], serde_json::json!([]));
    assert!(json["issues"].as_array().unwrap().iter().any(|issue| issue
        .as_str()
        .is_some_and(|s| s.contains("background worker"))));
}

#[tokio::test]
async fn test_spore_decode_endpoint_returns_decoded_from_cache() {
    let store = test_store();
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 128,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let decoded_entry = DobDecodedEntry {
        steps: vec![ckbadger_store::DobDecodedStep {
            step: 0,
            media_type: "image/svg+xml".to_string(),
            size: 29,
            hash: "abc123".to_string(),
            traits: vec![
                DobDecodedTrait {
                    name: "Background".to_string(),
                    value: "red".to_string(),
                },
                DobDecodedTrait {
                    name: "Level".to_string(),
                    value: "11".to_string(),
                },
            ],
        }],
        media_sources: vec![],
        decoded_at: 1700000000,
    };
    store
        .put_dob_decoded_direct(&spore_id, &decoded_entry)
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "decoded");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "dob/0");
    let traits = json["traits"].as_array().unwrap();
    assert_eq!(traits.len(), 2);
    assert_eq!(traits[0]["name"], "Background");
    assert_eq!(traits[0]["value"], "red");
    assert_eq!(traits[1]["name"], "Level");
    assert_eq!(traits[1]["value"], "11");
    let media = json["media"].as_array().unwrap();
    // 1 step output + render URL (SVG detected in media type)
    assert!(!media.is_empty());
    assert_eq!(media[0]["mediaType"], "image/svg+xml");
    assert_eq!(media[0]["size"], 29);
    assert_eq!(media[0]["hash"], "abc123");
    assert_eq!(media[0]["step"], 0);
    assert!(json["issues"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_decode_endpoint_reports_failed_with_reason() {
    use ckbadger_store::types::{DobDecodeFailure, DobDecodeFailureCategory};

    let store = test_store();
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    // A dob/0 spore whose decode was attempted and deterministically failed.
    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(vec![0x11; 32]),
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 128,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    // Record a persisted Failed outcome (mirrors the worker recording a
    // deterministic failure via StoreBatch::put_dob_decode_failure).
    let mut batch = StoreBatch::new(&store);
    batch.put_dob_decode_failure(
        &spore_id,
        &DobDecodeFailure {
            category: DobDecodeFailureCategory::ClusterMetadataInvalid,
            message: "cluster description is not valid JSON: expected value at line 1".to_string(),
            failed_at: 1_700_000_000,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "failed");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "dob/0");
    assert_eq!(json["traits"], serde_json::json!([]));
    assert_eq!(json["media"], serde_json::json!([]));
    // The recorded reason is surfaced verbatim as the sole issue.
    assert!(json["issues"][0]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[tokio::test]
async fn test_spore_decode_endpoint_non_dob_returns_pending_with_content_type() {
    let store = test_store();
    let spore_id = [0x66u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 500,
        created_at_tx: vec![0xCC; 32],
        extra: ObjectExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 4096,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "pending");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "image/png");
    assert_eq!(json["traits"], serde_json::json!([]));
}

#[tokio::test]
async fn test_spore_decode_endpoint_returns_not_found_for_missing_spore() {
    let store = test_store();
    let missing_id_hex = format!("0x{}", hex::encode([0x99u8; 32]));

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", missing_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

fn live_spore_entry(
    created_at_block: i64,
    owner: [u8; 32],
    cluster_id: Option<[u8; 32]>,
) -> ObjectEntry {
    ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: cluster_id.map(|c| c.to_vec()),
        token_id: None,
        owner_lock_hash: Some(owner.to_vec()),
        name: None,
        description: None,
        is_live: true,
        created_at_block,
        created_at_tx: vec![0x99; 32],
        extra: ObjectExtra::Spore {
            content_type: "text/plain".to_string(),
            content_length: 4,
            media_profile: SporeMediaProfile::default(),
        },
    }
}

fn live_cluster_entry(created_at_block: i64, owner: [u8; 32]) -> ObjectEntry {
    ObjectEntry {
        standard: ObjectStandard::SporeCluster,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(owner.to_vec()),
        name: Some("Walk Cluster".to_string()),
        description: None,
        is_live: true,
        created_at_block,
        created_at_tx: vec![0x98; 32],
        extra: ObjectExtra::SporeCluster,
    }
}

/// Follow `nextCursor` from `base` (which must already contain a query string)
/// until exhaustion, collecting `id_field` from every row.
async fn walk_paginated_ids(app: &axum::Router, base: &str, id_field: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..50 {
        let path = match &cursor {
            Some(c) => format!("{base}&cursor={c}"),
            None => base.to_string(),
        };
        let (status, json) = get_json(app, &path).await;
        assert_eq!(status, StatusCode::OK, "walk request {path} failed: {json}");
        for row in json["data"].as_array().expect("data array") {
            ids.push(row[id_field].as_str().expect("id string").to_string());
        }
        match json["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => return ids,
        }
    }
    panic!("pagination did not terminate within 50 pages");
}

fn assert_ids_exactly_once(ids: &[String], expected: &[String]) {
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "pagination returned duplicate rows: {ids:?}"
    );
    let expected_set: std::collections::HashSet<&String> = expected.iter().collect();
    let got_set: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(
        got_set, expected_set,
        "pagination lost or invented rows: got {ids:?}, expected {expected:?}"
    );
}

/// C5 regression: a block holding more spores than the page limit must still be
/// fully listable. The old numeric block cursor resumed with a strict
/// `created_at_block < cursor` comparison, irrecoverably skipping every
/// remaining entry of the cursor's block (5,806/37,291 mainnet spores lost).
#[tokio::test]
async fn test_spore_objects_pagination_walks_same_block_group_completely() {
    let store = test_store();
    let owner = [0xAA_u8; 32];
    // 7 spores in one block + 3 in neighboring blocks = 10 total.
    let mut expected = Vec::new();
    for b in 1u8..=7 {
        store
            .put_spore_direct(&[b; 32], &live_spore_entry(100, owner, None))
            .unwrap();
        expected.push(format!("0x{}", hex::encode([b; 32])));
    }
    for (b, block) in [(8u8, 99i64), (9, 98), (10, 97)] {
        store
            .put_spore_direct(&[b; 32], &live_spore_entry(block, owner, None))
            .unwrap();
        expected.push(format!("0x{}", hex::encode([b; 32])));
    }

    let app = create_router(test_config(store)).await;
    let ids = walk_paginated_ids(&app, "/spore/objects?limit=2", "sporeId").await;
    assert_ids_exactly_once(&ids, &expected);
}

/// C5 regression for `/spore/clusters/{id}/spores`: same strict-`<` defect.
#[tokio::test]
async fn test_spores_by_cluster_pagination_walks_same_block_group_completely() {
    let store = test_store();
    let owner = [0xAA_u8; 32];
    let cluster_id = [0xCC_u8; 32];
    store
        .put_spore_direct(&cluster_id, &live_cluster_entry(50, owner))
        .unwrap();

    let mut expected = Vec::new();
    for b in 1u8..=7 {
        store
            .put_spore_direct(&[b; 32], &live_spore_entry(100, owner, Some(cluster_id)))
            .unwrap();
        expected.push(format!("0x{}", hex::encode([b; 32])));
    }

    let app = create_router(test_config(store)).await;
    let base = format!(
        "/spore/clusters/0x{}/spores?limit=2",
        hex::encode(cluster_id)
    );
    let ids = walk_paginated_ids(&app, &base, "sporeId").await;
    assert_ids_exactly_once(&ids, &expected);
}

/// C5 regression for `/spore/clusters`: several clusters created in one block
/// must all be listable across pages.
#[tokio::test]
async fn test_spore_clusters_pagination_walks_same_block_group_completely() {
    let store = test_store();
    let owner = [0xAA_u8; 32];
    let mut expected = Vec::new();
    for b in 1u8..=5 {
        let cluster_id = [b; 32];
        store
            .put_spore_direct(&cluster_id, &live_cluster_entry(100, owner))
            .unwrap();
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_cluster_aggregate(
            &cluster_id,
            &ClusterAggregate {
                total_count: 1,
                live_count: 1,
                owner_count: 1,
                ..Default::default()
            },
        );
        batch.commit().unwrap();
        expected.push(format!("0x{}", hex::encode(cluster_id)));
    }

    let app = create_router(test_config(store)).await;
    let ids = walk_paginated_ids(&app, "/spore/clusters?limit=2", "clusterId").await;
    assert_ids_exactly_once(&ids, &expected);
}

/// The composite `{block}:{0x-id}` cursor is strictly validated on every spore
/// list endpoint; the legacy numeric-only block cursor is rejected too.
#[tokio::test]
async fn test_spore_list_endpoints_reject_malformed_cursors() {
    let store = test_store();
    let cluster_id = [0x11_u8; 32];
    store
        .put_spore_direct(&cluster_id, &live_cluster_entry(10, [0xAA; 32]))
        .unwrap();
    let app = create_router(test_config(store)).await;

    let endpoints = [
        "/spore/objects".to_string(),
        "/spore/clusters".to_string(),
        format!("/spore/clusters/0x{}/spores", hex::encode(cluster_id)),
        format!("/spore/owner/0x{}", hex::encode([0xAA_u8; 32])),
    ];
    let bad_cursors = [
        "abc",      // no separator
        "1:zz",     // id not 0x-prefixed hex
        "-3:0xab",  // negative block number
        "1:2:3",    // extra colon
        "100",      // legacy numeric-only cursor form (breaking change: rejected)
        "1:",       // empty id part
        ":0xab",    // empty block part
        "1:0x",     // empty hex after prefix
        "1:0xabcd", // id is not 32 bytes
    ];
    for endpoint in &endpoints {
        for cursor in &bad_cursors {
            let (status, json) = get_json(&app, &format!("{endpoint}?cursor={cursor}")).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "expected 400 for cursor {cursor:?} on {endpoint}, got {status}: {json}"
            );
        }
    }
}

/// A cursor pointing at the last row of a block resumes exactly at the next
/// entry — the first row of the following block, or the next id in-block.
#[tokio::test]
async fn test_spore_objects_cursor_resumes_exactly_after_block_boundary() {
    let store = test_store();
    let owner = [0xAA_u8; 32];
    store
        .put_spore_direct(&[0x01; 32], &live_spore_entry(100, owner, None))
        .unwrap();
    store
        .put_spore_direct(&[0x02; 32], &live_spore_entry(100, owner, None))
        .unwrap();
    store
        .put_spore_direct(&[0x03; 32], &live_spore_entry(99, owner, None))
        .unwrap();

    let app = create_router(test_config(store)).await;

    // Cursor at the last row of block 100 -> resume at block 99.
    let cursor = format!("100:0x{}", hex::encode([0x02_u8; 32]));
    let (status, json) = get_json(&app, &format!("/spore/objects?cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(
        data[0]["sporeId"],
        format!("0x{}", hex::encode([0x03_u8; 32]))
    );

    // Cursor mid-block -> resume at the next id within the same block.
    let cursor = format!("100:0x{}", hex::encode([0x01_u8; 32]));
    let (status, json) = get_json(&app, &format!("/spore/objects?cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(
        data[0]["sporeId"],
        format!("0x{}", hex::encode([0x02_u8; 32]))
    );
    assert_eq!(
        data[1]["sporeId"],
        format!("0x{}", hex::encode([0x03_u8; 32]))
    );
}

#[tokio::test]
async fn test_spore_objects_valid_cursor_on_empty_list_returns_empty_page() {
    let store = test_store();
    let app = create_router(test_config(store)).await;

    let cursor = format!("5:0x{}", hex::encode([0xFF_u8; 32]));
    let (status, json) = get_json(&app, &format!("/spore/objects?cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["data"].as_array().unwrap().is_empty());
    assert!(json["nextCursor"].is_null());
}

/// C6 regression: live CLUSTER cells must not appear as rows of the spore
/// objects/owner lists (their `/spore/objects/{id}` detail 404s — dead links).
/// The cluster must still be served by the cluster list and cluster detail.
#[tokio::test]
async fn test_spore_objects_and_owner_lists_exclude_cluster_cells() {
    let store = test_store();
    let owner = [0xAA_u8; 32];
    let cluster_id = [0xC1_u8; 32];
    let spore_id = [0x51_u8; 32];

    store
        .put_spore_direct(&cluster_id, &live_cluster_entry(100, owner))
        .unwrap();
    store
        .put_spore_direct(&spore_id, &live_spore_entry(101, owner, Some(cluster_id)))
        .unwrap();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            total_count: 1,
            live_count: 1,
            owner_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let spore_hex = format!("0x{}", hex::encode(spore_id));
    let cluster_hex = format!("0x{}", hex::encode(cluster_id));

    // Objects list: only the spore, never the cluster cell.
    let (status, json) = get_json(&app, "/spore/objects").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let data = json["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        1,
        "objects list must contain only the spore: {json}"
    );
    assert_eq!(data[0]["sporeId"], spore_hex);

    // Owner list: same exclusion (a cluster row there is the same dead link).
    let (status, json) = get_json(&app, &format!("/spore/owner/0x{}", hex::encode(owner))).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let data = json["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        1,
        "owner list must contain only the spore: {json}"
    );
    assert_eq!(data[0]["sporeId"], spore_hex);

    // The cluster is still served by the cluster surfaces.
    let (status, json) = get_json(&app, "/spore/clusters").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let clusters = json["data"].as_array().unwrap();
    assert!(
        clusters.iter().any(|c| c["clusterId"] == cluster_hex),
        "cluster missing from cluster list: {json}"
    );
    let (status, _json) = get_json(&app, &format!("/spore/clusters/{cluster_hex}")).await;
    assert_eq!(status, StatusCode::OK);
}

/// C7 regression: `sporesCount` on cluster detail and cluster list rows must be
/// the LIVE spore count, matching the live-based items list / holders /
/// composition next to it — not the ever-minted total including melted spores.
#[tokio::test]
async fn test_cluster_spores_count_is_live_count_on_detail_and_list() {
    let store = test_store();
    let cluster_id = [0xC7_u8; 32];
    let cluster_hex = format!("0x{}", hex::encode(cluster_id));

    store
        .put_spore_direct(&cluster_id, &live_cluster_entry(100, [0xAA; 32]))
        .unwrap();
    // 5 ever-minted, 2 live (3 melted) — the aggregate the indexer maintains
    // through the melt/consume path.
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            total_count: 5,
            live_count: 2,
            owner_count: 2,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, &format!("/spore/clusters/{cluster_hex}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["sporesCount"], 2,
        "detail sporesCount must be live count: {json}"
    );

    let (status, json) = get_json(&app, "/spore/clusters").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["clusterId"] == cluster_hex)
        .expect("cluster row present");
    assert_eq!(
        row["sporesCount"], 2,
        "list sporesCount must be live count: {json}"
    );
}

#[tokio::test]
async fn test_get_spore_returns_not_found_for_cluster_entry() {
    let store = test_store();
    let cluster_id = [0x44u8; 32];
    let cluster_id_hex = format!("0x{}", hex::encode(cluster_id));

    store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x11; 32]),
                name: Some("Test Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0x22; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    // GET /spore/objects/{cluster_id} should return 404
    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}", cluster_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // GET /spore/objects/{cluster_id}/activities should also return 404
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/objects/{}/activities",
            cluster_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
