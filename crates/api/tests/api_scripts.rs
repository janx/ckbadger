mod common;
use common::*;

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
async fn test_scripts_list_returns_warmup_pending_when_script_cache_missing() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router_without_warmup(config);

    let request = Request::builder()
        .uri("/api/v1/scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "warmup_pending");
    assert_eq!(
        json["message"],
        "script cache unavailable; warmup in progress"
    );
}

#[tokio::test]
async fn test_scripts_list_returns_default_lock_family_for_data1_reference() {
    let store = test_store();

    let family_id = "default-lock";
    let version_hash =
        hex::decode("709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649").unwrap();
    let canonical_type_reference =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let observed_data1_reference = version_hash.clone();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            description: Some("Default lock family".to_string()),
            versions_count: 1,
            live_cells_count: 10,
            cells_count: 14,
            owned_capacity_sum: 1_500,
            owned_knowledge_sum: 900,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            description: Some("Default lock family".to_string()),
            canonical_reference_hash: Some(canonical_type_reference.clone()),
            canonical_hash_type: Some(1),
            lock_live_cells_count: 10,
            lock_cells_count: 14,
            lock_owned_capacity_sum: 1_500,
            lock_owned_knowledge_sum: 900,
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &canonical_type_reference,
        &ScriptReferenceInfo {
            reference_hash: canonical_type_reference.clone(),
            hash_type: 1,
            lock_live_cells_count: 4,
            lock_cells_count: 6,
            lock_owned_capacity_sum: 700,
            lock_owned_knowledge_sum: 400,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(1, &canonical_type_reference, &version_hash);
    batch.put_script_reference_info(
        2,
        &observed_data1_reference,
        &ScriptReferenceInfo {
            reference_hash: observed_data1_reference.clone(),
            hash_type: 2,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(2, &observed_data1_reference, &version_hash);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;

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
    assert_eq!(data[0]["familyId"], "default-lock");
    assert_eq!(data[0]["name"], "Default Lock");
    assert_eq!(data[0]["liveCellsCount"], 10);
    assert_eq!(data[0]["cellsCount"], 14);
    assert_eq!(data[0]["ownedCapacitySum"], "1500");
    assert_eq!(data[0]["ownedKnowledgeSum"], "900");
    assert_eq!(data[0]["versionsCount"], 1);
}

#[tokio::test]
async fn test_scripts_list_supports_cursor_pagination() {
    let store = test_store();

    let mut batch = StoreBatch::new(store.as_ref());
    for (family_id, name) in [
        ("a-script", "A_SCRIPT"),
        ("b-script", "B_SCRIPT"),
        ("c-script", "C_SCRIPT"),
    ] {
        batch.put_script_family(
            family_id,
            &ScriptFamilyInfo {
                family_id: family_id.to_string(),
                name: name.to_string(),
                versions_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_family_by_name(name, family_id);
    }
    batch.commit().unwrap();

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
    assert_eq!(page1[0]["ownedCapacitySum"], "0");
    assert_eq!(page1[0]["ownedKnowledgeSum"], "0");
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

    let mut batch = StoreBatch::new(store.as_ref());
    for (family_id, name, owned_capacity_sum) in [
        ("a-script", "A_SCRIPT", 10i128),
        ("b-script", "B_SCRIPT", 30i128),
        ("c-script", "C_SCRIPT", 20i128),
    ] {
        batch.put_script_family(
            family_id,
            &ScriptFamilyInfo {
                family_id: family_id.to_string(),
                name: name.to_string(),
                owned_capacity_sum,
                versions_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_family_by_name(name, family_id);
    }
    batch.commit().unwrap();

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
    assert_eq!(page1[0]["ownedCapacitySum"], "30");
    assert_eq!(page1[1]["ownedCapacitySum"], "20");
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
    assert_eq!(page2[0]["ownedCapacitySum"], "10");
    assert_eq!(json["hasMore"], false);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_scripts_list_ignores_unlabeled_references_without_family_metadata() {
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
    assert_eq!(json["total"], 0);
    assert_eq!(data.len(), 0);
}

#[tokio::test]
async fn test_script_lookup_and_code_cell_resolve_data_reference() {
    let store = test_store();

    let version_hash = vec![0x70; 32];
    let code_cell_tx_hash = vec![0xe2; 32];
    let code_cell_output_index = 1i16;

    store
        .put_script_info_direct(
            &version_hash,
            &ScriptInfo {
                code_hash: version_hash.clone(),
                hash_type: 0,
                name: Some("Default Lock".to_string()),
                lock_cells_count: 10,
                lock_live_cells_count: 10,
                lock_capacity_sum: 1_000_000_000,
                lock_owned_capacity_sum: 1_000_000_000,
                lock_used_capacity_sum: 600_000_000,
                lock_owned_knowledge_sum: 600_000_000,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            name: Some("Default Lock".to_string()),
            category: Some("lock".to_string()),
            lock_cells_count: 10,
            lock_live_cells_count: 10,
            lock_capacity_sum: 1_000_000_000,
            lock_owned_capacity_sum: 1_000_000_000,
            lock_used_capacity_sum: 600_000_000,
            lock_owned_knowledge_sum: 600_000_000,
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_data_hash(
        &version_hash,
        123,
        &code_cell_tx_hash,
        code_cell_output_index,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            version_hash_hex
        )))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&version_hash_hex]["name"], "Default Lock");
    assert_eq!(json[&version_hash_hex]["codeHash"], version_hash_hex);
    assert_eq!(json[&version_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&version_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&version_hash_hex]["codeCellOutputIndex"], 1);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=data",
            version_hash_hex
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
async fn test_script_code_cells_resolve_unique_type_reference() {
    let store = test_store();

    let version_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let code_cell_tx_hash = vec![0xe2; 32];
    let code_cell_output_index = 1i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            name: Some("Default Lock".to_string()),
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_type(&type_hash, 123, &code_cell_tx_hash, code_cell_output_index);
    batch.put_cell_by_data_hash(
        &version_hash,
        123,
        &code_cell_tx_hash,
        code_cell_output_index,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=type",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["resolvedVersionHash"], version_hash_hex);
    assert_eq!(json["liveCount"], 1);
    assert_eq!(json["totalCount"], 1);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["codeCells"][0]["outputIndex"], 1);
    assert_eq!(json["codeCells"][0]["status"], "live");
    assert_eq!(json["codeCells"][0]["createdAtBlock"], 123);
}

#[tokio::test]
async fn test_script_lookup_and_code_cells_allow_unlabeled_resolved_type_reference() {
    let store = test_store();

    let version_hash = vec![0x51; 32];
    let type_hash = vec![0x61; 32];
    let code_cell_tx_hash = vec![0x71; 32];
    let code_cell_output_index = 2i16;

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(version_hash.clone()),
                lock_cells_count: 4,
                lock_live_cells_count: 2,
                lock_capacity_sum: 900,
                lock_owned_capacity_sum: 500,
                lock_used_capacity_sum: 700,
                lock_owned_knowledge_sum: 350,
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
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash.clone()),
        },
        234,
    );
    batch.put_cell_by_type(&type_hash, 234, &code_cell_tx_hash, code_cell_output_index);
    batch.put_cell_by_data_hash(
        &version_hash,
        234,
        &code_cell_tx_hash,
        code_cell_output_index,
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            type_hash_hex
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&type_hash_hex]["resolutionState"], "resolved");
    assert_eq!(json[&type_hash_hex]["name"], "Unknown");
    assert_eq!(json[&type_hash_hex]["codeHash"], version_hash_hex);
    assert_eq!(json[&type_hash_hex]["hashType"], "type");
    assert_eq!(json[&type_hash_hex]["deploymentTypeHash"], type_hash_hex);
    assert_eq!(json[&type_hash_hex]["deploymentDataHash"], version_hash_hex);
    assert_eq!(json[&type_hash_hex]["scriptKind"], "lock");
    assert_eq!(json[&type_hash_hex]["liveCellsCount"], 2);
    assert_eq!(json[&type_hash_hex]["ownedCapacitySum"], "500");
    assert_eq!(json[&type_hash_hex]["ownedKnowledgeSum"], "350");
    assert_eq!(
        json[&type_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&type_hash_hex]["codeCellOutputIndex"], 2);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=type",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["resolvedVersionHash"], version_hash_hex);
    assert_eq!(json["liveCount"], 1);
    assert_eq!(json["totalCount"], 1);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["codeCells"][0]["outputIndex"], 2);
    assert_eq!(json["codeCells"][0]["status"], "live");
}

#[tokio::test]
async fn test_scripts_list_merges_unknown_reference_into_known_deployment() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let family_id = "default-lock";

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.commit().unwrap();

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
    assert_eq!(data[0]["familyId"], family_id);
    assert_eq!(data[0]["name"], "Default Lock");
}

#[tokio::test]
async fn test_unknown_data_hash_script_resolves_code_cell_via_data_hash_index() {
    let store = test_store();

    let code_bytes = b"unknown-script-code-cell";
    let data_hash = compute_blake2b_data_hash(code_bytes);
    let code_cell_tx_hash = vec![0xcd; 32];
    let code_cell_output_index = 2i16;

    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                lock_live_cells_count: 3,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &data_hash,
        &ScriptVersionInfo {
            version_hash: data_hash.clone(),
            lock_live_cells_count: 3,
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: code_bytes.len() as i32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(data_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_data_hash(&data_hash, 123, &code_cell_tx_hash, code_cell_output_index);
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

    assert_eq!(json[&data_hash_hex]["name"], "Unknown");
    assert_eq!(json[&data_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&data_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&data_hash_hex]["codeCellOutputIndex"], 2);

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
    assert_eq!(json["outputIndex"], 2);
}

#[tokio::test]
async fn test_script_lookup_and_code_cells_resolve_unique_reference_without_hash_type() {
    let store = test_store();
    let reference_hash = vec![0x77; 32];
    let code_cell_tx_hash = vec![0xce; 32];

    store
        .put_script_info_direct(
            &reference_hash,
            &ScriptInfo {
                code_hash: reference_hash.clone(),
                hash_type: 0,
                lock_cells_count: 3,
                lock_live_cells_count: 1,
                lock_capacity_sum: 500,
                lock_owned_capacity_sum: 200,
                lock_used_capacity_sum: 500,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &reference_hash,
        &ScriptVersionInfo {
            version_hash: reference_hash.clone(),
            name: Some("UniqueScript".to_string()),
            category: Some("lock".to_string()),
            lock_cells_count: 3,
            lock_live_cells_count: 1,
            lock_capacity_sum: 500,
            lock_owned_capacity_sum: 200,
            lock_used_capacity_sum: 500,
            lock_owned_knowledge_sum: 200,
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(reference_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_data_hash(&reference_hash, 123, &code_cell_tx_hash, 1);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            reference_hash_hex
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json[&reference_hash_hex]["resolutionState"], "resolved");
    assert_eq!(json[&reference_hash_hex]["name"], "UniqueScript");
    assert_eq!(json[&reference_hash_hex]["codeHash"], reference_hash_hex);
    assert_eq!(json[&reference_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&reference_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}",
            reference_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["resolvedVersionHash"], reference_hash_hex);
    assert_eq!(json["liveCount"], 1);
    assert_eq!(json["totalCount"], 1);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);
}

#[tokio::test]
async fn test_script_lookup_and_code_cells_surface_type_reference_ambiguity() {
    let store = test_store();
    let reference_hash = vec![0x88; 32];
    let version_hash_a = vec![0xa1; 32];
    let version_hash_b = vec![0xb2; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &[0xd1; 32],
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash_a.clone()),
        },
        100,
    );
    batch.put_cell_by_type(&reference_hash, 100, &[0xd1; 32], 0);
    batch.put_cell(
        &[0xd2; 32],
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x44; 32],
            lock_code_hash: vec![0x55; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference_hash.clone()),
            type_code_hash: Some(vec![0x66; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash_b.clone()),
        },
        101,
    );
    batch.put_cell_by_type(&reference_hash, 101, &[0xd2; 32], 0);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let version_hash_a_hex = format!("0x{}", hex::encode(&version_hash_a));
    let version_hash_b_hex = format!("0x{}", hex::encode(&version_hash_b));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            reference_hash_hex
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json[&reference_hash_hex]["resolutionState"], "ambiguous");
    assert_eq!(
        json[&reference_hash_hex]["ambiguity"]["versionHashes"],
        serde_json::json!([version_hash_a_hex, version_hash_b_hex])
    );

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}",
            reference_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["codeCells"], serde_json::json!([]));
    assert_eq!(
        json["ambiguity"]["versionHashes"],
        serde_json::json!([version_hash_a_hex, version_hash_b_hex])
    );
}

#[tokio::test]
async fn test_cells_by_script_resolves_reference_hash_type_alias() {
    let store = test_store();
    // The cells-by-script listing reads the genesis baseline once per request
    // (fail-fast if absent) for burn-cell tagging.
    seed_genesis_baseline(&store);

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
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: type_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
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
    // The cells-by-script listing reads the genesis baseline once per request
    // (fail-fast if absent) for burn-cell tagging.
    seed_genesis_baseline(&store);

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
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: data_hash.clone(),
            lock_hash_type: 0,
            lock_args: vec![],
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
async fn test_get_script_returns_versions_sorted_by_deployed_at() {
    let store = test_store();
    let name = "SECP256K1_BLAKE160".to_string();
    let family_id = "secp256k1-blake160";

    let older_type_hash = vec![0x11; 32];
    let newer_type_hash = vec![0x22; 32];
    let older_version_hash = vec![0x33; 32];
    let newer_version_hash = vec![0x44; 32];
    let older_tx_hash = vec![0xaa; 32];
    let newer_earliest_tx_hash = vec![0xab; 32];
    let newer_tx_hash = vec![0xbb; 32];

    let older_block = 100i64;
    let newer_earliest_block = 150i64;
    let newer_block = 200i64;
    let older_timestamp = 1_700_000_000_000i64;
    let newer_earliest_timestamp = 1_700_050_000_000i64;
    let newer_timestamp = 1_700_100_000_000i64;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        older_block,
        &CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: older_timestamp,
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
    batch.put_block_header(
        newer_earliest_block,
        &CachedBlockHeader {
            hash: vec![0x03; 32],
            parent_hash: vec![0u8; 32],
            timestamp: newer_earliest_timestamp,
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
    batch.put_block_header(
        newer_block,
        &CachedBlockHeader {
            hash: vec![0x04; 32],
            parent_hash: vec![0u8; 32],
            timestamp: newer_timestamp,
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
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: name.clone(),
            versions_count: 2,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name(&name, family_id);
    batch.put_script_version_by_family(family_id, &older_version_hash);
    batch.put_script_version_by_family(family_id, &newer_version_hash);
    batch.put_script_version(
        &older_version_hash,
        &ScriptVersionInfo {
            version_hash: older_version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some(name.clone()),
            canonical_reference_hash: Some(older_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_version(
        &newer_version_hash,
        &ScriptVersionInfo {
            version_hash: newer_version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some(name.clone()),
            canonical_reference_hash: Some(newer_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &older_tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x10; 32],
            lock_code_hash: vec![0x20; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(older_type_hash.clone()),
            type_code_hash: Some(vec![0x30; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(older_version_hash.clone()),
        },
        older_block,
    );
    batch.put_cell_by_type(&older_type_hash, older_block, &older_tx_hash, 0);
    batch.put_cell_by_data_hash(&older_version_hash, older_block, &older_tx_hash, 0);
    batch.put_cell(
        &newer_earliest_tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x12; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(newer_type_hash.clone()),
            type_code_hash: Some(vec![0x32; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(newer_version_hash.clone()),
        },
        newer_earliest_block,
    );
    batch.put_cell_by_type(
        &newer_type_hash,
        newer_earliest_block,
        &newer_earliest_tx_hash,
        0,
    );
    batch.put_cell_by_data_hash(
        &newer_version_hash,
        newer_earliest_block,
        &newer_earliest_tx_hash,
        0,
    );
    batch.put_cell(
        &newer_tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x21; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(newer_type_hash.clone()),
            type_code_hash: Some(vec![0x31; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(newer_version_hash.clone()),
        },
        newer_block,
    );
    batch.put_cell_by_type(&newer_type_hash, newer_block, &newer_tx_hash, 1);
    batch.put_cell_by_data_hash(&newer_version_hash, newer_block, &newer_tx_hash, 1);
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
    let items = json["versions"].as_array().unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(json["familyId"], family_id);
    assert_eq!(json["name"], name);
    assert_eq!(
        items[0]["versionHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&newer_version_hash)))
    );
    assert_eq!(
        items[0]["canonicalReferenceHash"],
        format!("0x{}", hex::encode(&newer_type_hash))
    );
    assert_eq!(items[0]["canonicalHashType"], "type");
    assert_eq!(items[0]["deployedAt"], newer_earliest_timestamp);
    let newer_deployments = items[0]["deployments"].as_array().unwrap();
    assert_eq!(newer_deployments.len(), 2);
    assert_eq!(
        newer_deployments[0]["codeCellTxHash"],
        format!("0x{}", hex::encode(&newer_earliest_tx_hash))
    );
    assert_eq!(newer_deployments[0]["codeCellOutputIndex"], 0);
    assert_eq!(newer_deployments[0]["deployedAt"], newer_earliest_timestamp);
    assert_eq!(
        newer_deployments[0]["typeReferenceHash"],
        format!("0x{}", hex::encode(&newer_type_hash))
    );
    assert_eq!(
        newer_deployments[0]["dataReferenceHash"],
        format!("0x{}", hex::encode(&newer_version_hash))
    );
    assert_eq!(
        items[1]["versionHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&older_version_hash)))
    );
    assert_eq!(
        items[1]["canonicalReferenceHash"],
        format!("0x{}", hex::encode(&older_type_hash))
    );
    assert_eq!(items[1]["canonicalHashType"], "type");
    assert_eq!(items[1]["deployedAt"], older_timestamp);
}

#[tokio::test]
async fn test_get_script_includes_direct_version_hash_reference_without_mapping() {
    let store = test_store();
    let family_id = "default-lock";
    let version_hash = vec![0x70; 32];
    let canonical_type_hash = vec![0x9b; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            canonical_reference_hash: Some(canonical_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        2,
        &version_hash,
        &ScriptReferenceInfo {
            reference_hash: version_hash.clone(),
            hash_type: 2,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let references = json["versions"][0]["references"].as_array().unwrap();

    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0]["referenceHash"],
        format!("0x{}", hex::encode(&version_hash))
    );
    assert_eq!(references[0]["hashType"], "data1");
}

#[tokio::test]
async fn test_get_script_fails_when_relevant_canonical_reference_mapping_missing() {
    let store = test_store();
    let family_id = "default-lock";
    let version_hash = vec![0x70; 32];
    let canonical_type_hash = vec![0x9b; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            canonical_reference_hash: Some(canonical_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &canonical_type_hash,
        &ScriptReferenceInfo {
            reference_hash: canonical_type_hash.clone(),
            hash_type: 1,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing reference->version mapping"));
}

#[tokio::test]
async fn test_get_script_ignores_unrelated_unresolved_reference_info() {
    let store = test_store();
    let family_id = "default-lock";
    let version_hash = vec![0x70; 32];
    let canonical_type_hash = vec![0x9b; 32];
    let unrelated_reference = vec![0xaa; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            canonical_reference_hash: Some(canonical_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &unrelated_reference,
        &ScriptReferenceInfo {
            reference_hash: unrelated_reference.clone(),
            hash_type: 1,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let references = json["versions"][0]["references"].as_array().unwrap();
    assert_eq!(references.len(), 0);
}

#[tokio::test]
async fn test_get_script_usage_aggregates_family_versions() {
    let store = test_store();
    let family_id = "default-lock";
    let version_a = vec![0x11; 32];
    let version_b = vec![0x22; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 2,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_a);
    batch.put_script_version_by_family(family_id, &version_b);
    batch.put_script_version(
        &version_a,
        &ScriptVersionInfo {
            version_hash: version_a.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            category: Some("lock".to_string()),
            lock_cells_count: 4,
            lock_live_cells_count: 3,
            lock_capacity_sum: 500,
            lock_owned_capacity_sum: 300,
            lock_used_capacity_sum: 260,
            lock_owned_knowledge_sum: 180,
            ..Default::default()
        },
    );
    batch.put_script_version(
        &version_b,
        &ScriptVersionInfo {
            version_hash: version_b.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            category: Some("type".to_string()),
            type_cells_count: 5,
            type_live_cells_count: 2,
            type_capacity_sum: 700,
            type_owned_capacity_sum: 400,
            type_used_capacity_sum: 500,
            type_owned_knowledge_sum: 220,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    store
        .put_script_info_direct(
            &[0xaa; 32],
            &ScriptInfo {
                code_hash: vec![0xaa; 32],
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                lock_cells_count: 999,
                lock_live_cells_count: 999,
                lock_capacity_sum: 999_999,
                lock_owned_capacity_sum: 999_999,
                lock_used_capacity_sum: 999_999,
                lock_owned_knowledge_sum: 999_999,
                ..Default::default()
            },
        )
        .unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["name"], "Default Lock");
    assert_eq!(json["cellsCount"], 9);
    assert_eq!(json["liveCellsCount"], 5);
    assert_eq!(json["capacitySum"], "1200");
    assert_eq!(json["ownedCapacitySum"], "700");
    assert_eq!(json["commonKnowledgeSizeSum"], "760");
    assert_eq!(json["ownedKnowledgeSum"], "400");
    assert_eq!(
        json["byDeployment"][0]["codeHash"],
        format!("0x{}", hex::encode(&version_a))
    );
    assert_eq!(
        json["byDeployment"][1]["codeHash"],
        format!("0x{}", hex::encode(&version_b))
    );
}

#[tokio::test]
async fn test_get_script_usage_returns_not_found_for_unknown_family() {
    let store = test_store();
    let app = create_router(test_config(store)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/Unknown%20Family/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_script_capacity_chart_aggregates_deployments() {
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
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a,
            false,
            20240117,
            &ScriptDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 50,
                owned_knowledge_delta: 30,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240117,
            &ScriptDailyDelta {
                owned_capacity_delta: 10,
                owned_knowledge_delta: 5,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        300,
        &CachedBlockHeader {
            hash: vec![0x03; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_705_536_000_000,
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
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160/charts/capacity-history")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["title"], "SECP256K1_BLAKE160 Capacity History");
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "90");
    assert_eq!(data[0]["values"]["unused"], "60");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["used"], "90");
    assert_eq!(data[1]["values"]["unused"], "60");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["used"], "85");
    assert_eq!(data[2]["values"]["unused"], "55");

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160/charts/capacity-history?from=2024-01-16&to=2024-01-16")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["used"], "90");
    assert_eq!(data[0]["values"]["unused"], "60");
}

#[tokio::test]
async fn test_script_capacity_chart_by_code_hash_with_kind_filter() {
    let store = test_store();
    let code_hash = vec![0x33; 32];
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    store
        .put_script_daily_delta(
            &code_hash,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 40,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash,
            true,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 80,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0x04; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_705_363_200_000,
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
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/charts/capacity-history?code_hash={}&script_kind=lock",
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
    assert_eq!(data[0]["values"]["used"], "40");
    assert_eq!(data[0]["values"]["unused"], "60");
}

#[tokio::test]
async fn test_script_capacity_chart_by_code_hash_extends_to_latest_complete_ckb_day() {
    let store = test_store();
    let code_hash = vec![0x44; 32];
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    store
        .put_script_daily_delta(
            &code_hash,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 40,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_705_536_000_000,
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
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/charts/capacity-history?code_hash={}&script_kind=lock",
            code_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "40");
    assert_eq!(data[0]["values"]["unused"], "60");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["used"], "40");
    assert_eq!(data[1]["values"]["unused"], "60");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["used"], "40");
    assert_eq!(data[2]["values"]["unused"], "60");
}

#[tokio::test]
async fn test_scripts_list_reads_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let family_id = "core-only-script";
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "CoreOnlyScript".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    core_batch.put_script_family_by_name("CoreOnlyScript", family_id);
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
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
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["familyId"], family_id);
    assert_eq!(data[0]["name"], "CoreOnlyScript");
}

#[tokio::test]
async fn test_deprecated_script_labels_resolve_by_name_and_api_flag() {
    let store = test_store();
    run_label_import_bundled(store.as_ref(), "mainnet").unwrap();

    let app = create_router(test_config(store)).await;
    let pw_lock_data_hash = "0xd6a5a0edb152e88e8bbc702e164441cb3890fae35da672b408d28ca9a1bde3ee";

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            pw_lock_data_hash
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[pw_lock_data_hash]["name"], "PW Lock");
    assert_eq!(json[pw_lock_data_hash]["deprecated"], true);
    assert_eq!(json[pw_lock_data_hash]["resolutionState"], "resolved");

    let request = Request::builder()
        .uri("/api/v1/scripts/PW%20Lock")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json["versions"].as_array().unwrap();

    assert!(!items.is_empty());
    assert_eq!(json["name"], "PW Lock");
    assert_eq!(items[0]["name"], "PW Lock");
    assert_eq!(items[0]["deprecated"], true);
}

#[tokio::test]
async fn test_lookup_scripts_accepts_tx_hash_parameter() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"codeHashes":[],"txHash":"0x0000000000000000000000000000000000000000000000000000000000000000"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
