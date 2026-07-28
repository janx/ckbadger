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
