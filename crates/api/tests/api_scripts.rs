use crate::common::*;

fn put_canonical_script_tx(
    batch: &mut StoreBatch<'_>,
    hash: &[u8],
    block: i64,
    index: i32,
    outputs: i16,
) {
    batch.put_tx_hash_map(hash, block, index);
    batch.put_tx_index(
        block,
        index,
        &TxIndexEntry {
            tx_hash: hash.try_into().expect("fixture tx hash width"),
            is_cellbase: false,
            timestamp: 1_704_153_601_000,
            inputs_count: 1,
            outputs_count: outputs,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        },
    );
}

fn set_script_fixture_block_tx_count(store: &CkbadgerStore, block: i64, count: i32) {
    let mut header = store
        .get_block_header(block)
        .expect("read fixture header")
        .expect("fixture block must exist");
    header.transactions_count = count;
    let mut batch = StoreBatch::new(store);
    batch.put_block_header(block, &header);
    batch.commit().expect("update fixture header");
}

#[tokio::test]
async fn test_multisig_never_signed_requires_complete_custodian_spend_evidence() {
    use ckbadger_store::types::AddressBalance;

    let store = test_store();
    let family_id = "multisig-evidence";
    let version = vec![0x91; 32];
    let refs: Vec<Vec<u8>> = (1..=3).map(|n| vec![n; 32]).collect();
    let multisig =
        hex::decode("5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8").unwrap();
    seed_block_headers_at(
        &store,
        &[
            (9, utc_ms("2024-01-02T00:00:01Z")),
            (10, utc_ms("2024-01-01T23:59:59Z")),
        ],
    );
    set_script_fixture_block_tx_count(&store, 10, 3);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Multisig Evidence".to_string(),
            versions_count: 1,
            lock_cells_count: 3,
            live_cells_count: 3,
            cells_count: 3,
            owned_capacity_sum: 300,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Multisig Evidence", family_id);
    batch.put_script_version_by_family(family_id, &version);
    batch.put_script_version(
        &version,
        &ScriptVersionInfo {
            version_hash: version.clone(),
            family_id: Some(family_id.to_string()),
            registered_references: refs.iter().map(|r| (r.clone(), 1)).collect(),
            type_id_deployments: refs.iter().map(|r| (r.clone(), 10)).collect(),
            canonical_reference_hash: Some(refs[0].clone()),
            canonical_hash_type: Some(1),
            lock_cells_count: 3,
            lock_live_cells_count: 3,
            lock_owned_capacity_sum: 300,
            ..Default::default()
        },
    );
    for (i, reference) in refs.iter().enumerate() {
        let tx = vec![0x20 + i as u8; 32];
        put_canonical_script_tx(&mut batch, &tx, 10, i as i32, 1);
        let lock_hash = vec![0x30 + i as u8; 32];
        batch.put_script_reference_info(
            1,
            reference,
            &ScriptReferenceInfo {
                reference_hash: reference.clone(),
                hash_type: 1,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 100,
                ..Default::default()
            },
        );
        batch.put_script_reference_to_version(1, reference, &version);
        batch.put_cell(
            &tx,
            0,
            &LiveCellInfo {
                capacity: 100_00000000,
                lock_script_hash: lock_hash.clone(),
                lock_code_hash: multisig.clone(),
                lock_hash_type: 1,
                lock_args: vec![0x45; 20],
                type_script_hash: Some(reference.clone()),
                type_code_hash: Some(vec![0x61; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 32,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: Some(version.clone()),
            },
            10,
        );
        batch.put_cell_by_type(reference, 10, &tx, 0);
        batch.put_cell_by_data_hash(&version, 10, &tx, 0);
        if i < 2 {
            batch.put_addr_balance(
                &lock_hash,
                &AddressBalance {
                    total_cells_count: if i == 0 { 1 } else { 2 },
                    live_cells_count: 1,
                    ..Default::default()
                },
            );
        }
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store.clone())).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Multisig%20Evidence")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let references = detail["references"].as_array().unwrap();
    assert_eq!(references.len(), 3);
    assert_eq!(references[0]["custody"]["witnessState"], "never_signed");
    assert_eq!(references[1]["custody"]["witnessState"], "unavailable");
    assert_eq!(references[2]["custody"]["witnessState"], "unavailable");
    assert_eq!(detail["history"].as_array().unwrap().len(), 3);
    if let Ok(dir) = std::env::var("CKBADGER_SCRIPT_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/multisig-evidence-detail.json"),
            serde_json::to_vec_pretty(&detail).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn test_type_reference_keeps_same_binary_multiple_holders_but_flags_different_binaries() {
    for different_binary in [false, true] {
        let store = test_store();
        let version = vec![0x81; 32];
        let reference = vec![0x82; 32];
        seed_block_headers_at(&store, &[(10, 1_700_000_000_000)]);
        set_script_fixture_block_tx_count(&store, 10, 2);
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_script_family(
            "mixed-holders",
            &ScriptFamilyInfo {
                family_id: "mixed-holders".to_string(),
                name: "Mixed Holders".to_string(),
                versions_count: 1,
                lock_cells_count: 2,
                live_cells_count: 2,
                cells_count: 2,
                owned_capacity_sum: 200,
                ..Default::default()
            },
        );
        batch.put_script_family_by_name("Mixed Holders", "mixed-holders");
        batch.put_script_version_by_family("mixed-holders", &version);
        batch.put_script_version(
            &version,
            &ScriptVersionInfo {
                version_hash: version.clone(),
                family_id: Some("mixed-holders".to_string()),
                registered_references: vec![(reference.clone(), 1)],
                type_id_deployments: Vec::new(),
                canonical_reference_hash: Some(reference.clone()),
                canonical_hash_type: Some(1),
                lock_cells_count: 2,
                lock_live_cells_count: 2,
                lock_owned_capacity_sum: 200,
                ..Default::default()
            },
        );
        batch.put_script_reference_info(
            1,
            &reference,
            &ScriptReferenceInfo {
                reference_hash: reference.clone(),
                hash_type: 1,
                lock_cells_count: 2,
                lock_live_cells_count: 2,
                lock_owned_capacity_sum: 200,
                ..Default::default()
            },
        );
        batch.put_script_reference_to_version(1, &reference, &version);
        for i in 0..2u8 {
            let tx = vec![0x91 + i; 32];
            put_canonical_script_tx(&mut batch, &tx, 10, i32::from(i), 1);
            batch.put_cell(
                &tx,
                0,
                &LiveCellInfo {
                    capacity: 100_00000000,
                    lock_script_hash: vec![0x31 + i; 32],
                    lock_code_hash: hex::decode(TEST_SECP_LOCK_CODE_HASH.trim_start_matches("0x"))
                        .unwrap(),
                    lock_hash_type: 1,
                    lock_args: vec![0x51 + i; 20],
                    type_script_hash: Some(reference.clone()),
                    type_code_hash: Some(vec![0x61; 32]),
                    type_hash_type: Some(1),
                    type_args: Some(vec![]),
                    data_size: 32,
                    occupied_capacity: 61_00000000,
                    udt_amount: None,
                    data_hash: Some(if different_binary && i == 1 {
                        vec![0x83; 32]
                    } else {
                        version.clone()
                    }),
                },
                10,
            );
            batch.put_cell_by_type(&reference, 10, &tx, 0);
            if !different_binary || i == 0 {
                batch.put_cell_by_data_hash(&version, 10, &tx, 0);
            }
        }
        batch.commit().unwrap();
        let app = create_router(test_config(store)).await;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/scripts/Mixed%20Holders")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let detail: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let custody = &detail["references"][0]["custody"];
        if different_binary {
            assert_eq!(custody["class"], "ambiguous");
            assert!(detail["references"][0]["currentVersionHash"].is_null());
            let response = app
                .oneshot(
                    Request::builder()
                        .uri(format!(
                            "/api/v1/scripts/code-cells?code_hash=0x{}&hash_type=type",
                            hex::encode(&reference)
                        ))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let code_cells: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert!(code_cells["resolvedVersionHash"].is_null());
            assert_eq!(
                code_cells["ambiguity"]["versionHashes"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        } else {
            assert_eq!(custody["class"], "mixed");
            assert_eq!(
                detail["references"][0]["currentVersionHash"],
                format!("0x{}", hex::encode(&version))
            );
            assert_eq!(custody["owners"].as_array().unwrap().len(), 2);
            assert_eq!(
                detail["versions"][0]["deployments"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
    }
}

#[tokio::test]
async fn d6_secp_lock_with_all_zero_args_is_unspendable_code_custody() {
    let store = test_store();
    let family_id = "class-evidence";
    let version = vec![0x31; 32];
    let type_ref = vec![0x42; 32];
    let outpoint = vec![0x53; 32];
    seed_block_headers_at(
        &store,
        &[
            (9, utc_ms("2024-01-02T00:00:01Z")),
            (10, utc_ms("2024-01-01T23:59:59Z")),
        ],
    );
    set_script_fixture_block_tx_count(&store, 10, 2);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Class Evidence".to_string(),
            versions_count: 1,
            lock_cells_count: 2,
            live_cells_count: 2,
            cells_count: 2,
            owned_capacity_sum: 900,
            owned_knowledge_sum: 100,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Class Evidence", family_id);
    batch.put_script_version_by_family(family_id, &version);
    batch.put_script_version(
        &version,
        &ScriptVersionInfo {
            version_hash: version.clone(),
            family_id: Some(family_id.to_string()),
            registered_references: vec![(version.clone(), 2), (type_ref.clone(), 1)],
            type_id_deployments: vec![(type_ref.clone(), 10)],
            canonical_reference_hash: Some(type_ref.clone()),
            canonical_hash_type: Some(1),
            lock_cells_count: 2,
            lock_live_cells_count: 2,
            lock_owned_capacity_sum: 900,
            lock_owned_knowledge_sum: 100,
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        2,
        &version,
        &ScriptReferenceInfo {
            reference_hash: version.clone(),
            hash_type: 2,
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_owned_capacity_sum: 500,
            lock_owned_knowledge_sum: 50,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(2, &version, &version);
    batch.put_script_reference_info(
        1,
        &type_ref,
        &ScriptReferenceInfo {
            reference_hash: type_ref.clone(),
            hash_type: 1,
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_owned_capacity_sum: 400,
            lock_owned_knowledge_sum: 50,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(1, &type_ref, &version);
    batch.put_cell(
        &outpoint,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x19; 32],
            lock_code_hash: hex::decode(TEST_SECP_LOCK_CODE_HASH.trim_start_matches("0x")).unwrap(),
            lock_hash_type: 1,
            lock_args: vec![0x00; 20],
            type_script_hash: Some(type_ref.clone()),
            type_code_hash: Some(vec![0x61; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version.clone()),
        },
        10,
    );
    batch.put_cell_by_type(&type_ref, 10, &outpoint, 0);
    put_canonical_script_tx(&mut batch, &outpoint, 10, 1, 1);
    batch.put_cell_by_data_hash(&version, 10, &outpoint, 0);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list: serde_json::Value =
        serde_json::from_slice(&list.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        list["data"][0]["codeClasses"],
        serde_json::json!([
            {"class":"fixed","lockCellsCount":1,"lockOwnedCapacitySum":"500","referenceCount":1},
            {"class":"unspendable","lockCellsCount":1,"lockOwnedCapacitySum":"400","referenceCount":1}
        ])
    );

    let (status, detail) = get_json(&app, "/scripts/Class%20Evidence").await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let custody = &detail["versions"][0]["deployments"][0]["custody"];
    // D6 / SC-L9: secp256k1 with no known key (20 zero bytes, the multisig v2
    // Type ID code cell's lock) can never sign, so the code is effectively immutable.
    assert_eq!(custody["class"], "unspendable", "{custody}");
    assert_eq!(custody["witnessState"], "not_applicable", "{custody}");
}

#[tokio::test]
async fn test_script_family_exposes_exact_reference_classes_and_chain_history() {
    let store = test_store();
    let family_id = "class-evidence";
    let version = vec![0x31; 32];
    let type_ref = vec![0x42; 32];
    let outpoint = vec![0x53; 32];
    seed_block_headers_at(
        &store,
        &[
            (9, utc_ms("2024-01-02T00:00:01Z")),
            (10, utc_ms("2024-01-01T23:59:59Z")),
        ],
    );
    set_script_fixture_block_tx_count(&store, 10, 2);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Class Evidence".to_string(),
            versions_count: 1,
            lock_cells_count: 2,
            live_cells_count: 2,
            cells_count: 2,
            owned_capacity_sum: 900,
            owned_knowledge_sum: 100,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Class Evidence", family_id);
    batch.put_script_version_by_family(family_id, &version);
    batch.put_script_version(
        &version,
        &ScriptVersionInfo {
            version_hash: version.clone(),
            family_id: Some(family_id.to_string()),
            registered_references: vec![(version.clone(), 2), (type_ref.clone(), 1)],
            type_id_deployments: vec![(type_ref.clone(), 10)],
            canonical_reference_hash: Some(type_ref.clone()),
            canonical_hash_type: Some(1),
            lock_cells_count: 2,
            lock_live_cells_count: 2,
            lock_owned_capacity_sum: 900,
            lock_owned_knowledge_sum: 100,
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        2,
        &version,
        &ScriptReferenceInfo {
            reference_hash: version.clone(),
            hash_type: 2,
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_owned_capacity_sum: 500,
            lock_owned_knowledge_sum: 50,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(2, &version, &version);
    batch.put_script_reference_info(
        1,
        &type_ref,
        &ScriptReferenceInfo {
            reference_hash: type_ref.clone(),
            hash_type: 1,
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_owned_capacity_sum: 400,
            lock_owned_knowledge_sum: 50,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(1, &type_ref, &version);
    batch.put_cell(
        &outpoint,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x19; 32],
            lock_code_hash: hex::decode(TEST_SECP_LOCK_CODE_HASH.trim_start_matches("0x")).unwrap(),
            lock_hash_type: 1,
            lock_args: vec![0x81; 20],
            type_script_hash: Some(type_ref.clone()),
            type_code_hash: Some(vec![0x61; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version.clone()),
        },
        10,
    );
    batch.put_cell_by_type(&type_ref, 10, &outpoint, 0);
    put_canonical_script_tx(&mut batch, &outpoint, 10, 1, 1);
    batch.put_cell_by_data_hash(&version, 10, &outpoint, 0);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list: serde_json::Value =
        serde_json::from_slice(&list.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        list["data"][0]["codeClasses"],
        serde_json::json!([
            {"class":"fixed","lockCellsCount":1,"lockOwnedCapacitySum":"500","referenceCount":1},
            {"class":"one_key","lockCellsCount":1,"lockOwnedCapacitySum":"400","referenceCount":1}
        ])
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Class%20Evidence")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(detail["references"].as_array().unwrap().len(), 2);
    assert_eq!(
        detail["versions"][0]["deployments"][0]["custody"]["class"],
        "one_key"
    );
    assert_eq!(detail["versions"][0]["deployments"][0]["hashType"], "type");
    assert_eq!(detail["history"].as_array().unwrap().len(), 2);
    assert_eq!(detail["history"][0]["blockNumber"], 10);
    assert_eq!(
        detail["history"][0]["timestamp"],
        utc_ms("2024-01-01T23:59:59Z")
    );
    assert_eq!(detail["history"][0]["indexedDay"], "2024-01-02");
    assert_eq!(
        detail["history"][0]["chainTimeMs"],
        utc_ms("2024-01-02T00:00:01Z")
    );
    if let Ok(dir) = std::env::var("CKBADGER_SCRIPT_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/class-evidence-list.json"),
            serde_json::to_vec_pretty(&list).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/class-evidence-detail.json"),
            serde_json::to_vec_pretty(&detail).unwrap(),
        )
        .unwrap();
    }
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

    let response = app.clone().oneshot(request).await.unwrap();
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
    for (i, (family_id, name)) in [
        ("a-script", "A_SCRIPT"),
        ("b-script", "B_SCRIPT"),
        ("c-script", "C_SCRIPT"),
    ]
    .into_iter()
    .enumerate()
    {
        let version = vec![i as u8 + 1; 32];
        batch.put_script_family(
            family_id,
            &ScriptFamilyInfo {
                family_id: family_id.to_string(),
                name: name.to_string(),
                versions_count: 1,
                lock_cells_count: if i < 2 { 1 } else { 0 },
                type_cells_count: if i == 2 { 1 } else { 0 },
                ..Default::default()
            },
        );
        batch.put_script_family_by_name(name, family_id);
        batch.put_script_version_by_family(family_id, &version);
        batch.put_script_version(
            &version,
            &ScriptVersionInfo {
                version_hash: version.clone(),
                family_id: Some(family_id.to_string()),
                registered_references: vec![(version.clone(), 2)],
                canonical_reference_hash: Some(version.clone()),
                canonical_hash_type: Some(2),
                ..Default::default()
            },
        );
        batch.put_script_reference_info(
            2,
            &version,
            &ScriptReferenceInfo {
                reference_hash: version.clone(),
                hash_type: 2,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 100,
                ..Default::default()
            },
        );
        batch.put_script_reference_to_version(2, &version, &version);
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
    assert_eq!(
        json["classSummary"],
        serde_json::json!([{
            "class":"fixed", "lockCellsCount":3, "lockOwnedCapacitySum":"300", "referenceCount":3
        }])
    );
    let page_one_summary = json["classSummary"].clone();
    assert_eq!(
        json["roleCounts"],
        serde_json::json!({"all":3,"locks":2,"types":1})
    );

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&cursor=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
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
    assert_eq!(json["classSummary"], page_one_summary);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts?role=type&limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let filtered: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["data"][0]["name"], "C_SCRIPT");
    assert_eq!(filtered["classSummary"][0]["referenceCount"], 1);
    assert_eq!(
        filtered["roleCounts"],
        serde_json::json!({"all":3,"locks":2,"types":1})
    );
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
async fn test_code_cell_endpoints_resolve_exact_form_when_hash_type_given() {
    // Mainnet secp scenario: the type form (0x9b..) maps to the real bytecode
    // version (0x70..) whose code cell exists, while a junk data-form
    // self-mapping exists for the same reference bytes with no code cell
    // carrying that data hash. With an explicit hash_type the endpoints must
    // resolve exactly that one form instead of the whole family.
    let store = test_store();

    let version_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let code_cell_tx_hash = vec![0xe2; 32];
    let code_cell_output_index = 1i16;

    store
        .put_script_reference_to_version_direct(1, &type_hash, &version_hash)
        .unwrap();
    // Garbage data-form self-mapping written from junk usage of the same bytes.
    store
        .put_script_reference_to_version_direct(0, &type_hash, &type_hash)
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            name: Some("SECP256K1_BLAKE160".to_string()),
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

    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    // hash_type=type resolves through the persisted type mapping to the real
    // code cell.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=type",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["outputIndex"], 1);

    // hash_type=data asks for the binary whose data hash IS 0x9b.. -- no such
    // code cell exists, so the exact form must NOT fall back to the family.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=data",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["txHash"], serde_json::Value::Null);
    assert_eq!(json["outputIndex"], serde_json::Value::Null);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=data",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["codeCells"], serde_json::json!([]));
    assert_eq!(json["totalCount"], 0);
    assert_eq!(json["resolvedVersionHash"], serde_json::Value::Null);

    // The bytecode hash queried as its own data form still resolves.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=data",
            version_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["resolvedVersionHash"], version_hash_hex);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);

    // Without hash_type the whole-reference family resolution is unchanged.
    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["txHash"], code_cell_tx_hash_hex);

    // Invalid hash_type values are a bad request on both endpoints.
    for uri in [
        format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=bogus",
            type_hash_hex
        ),
        format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=bogus",
            type_hash_hex
        ),
    ] {
        let request = Request::builder().uri(&uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "uri={uri} must reject invalid hash_type"
        );
    }
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
async fn test_cells_by_script_uses_exact_reference_form_without_alias() {
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
    // Per-form reference counters (production invariant: written for every
    // observed reference form; the endpoint total reads them).
    store
        .put_script_reference_info_direct(
            1,
            &type_hash,
            &ScriptReferenceInfo {
                reference_hash: type_hash.clone(),
                hash_type: 1,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 100_00000000,
                lock_owned_capacity_sum: 100_00000000,
                lock_used_capacity_sum: 61_00000000,
                lock_owned_knowledge_sum: 61_00000000,
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
            occupied_capacity: 41_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
    );
    batch.put_cell_by_lock_code(&type_hash, 1, 123, &tx_hash, 0);
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

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();

    assert_eq!(json["total"], 0);
    assert_eq!(data.len(), 0);

    let (status, exact) = get_json(
        &app,
        &format!(
            "/cells/by-script?code_hash=0x{}&hash_type=type&script_kind=lock&limit=20",
            hex::encode(&type_hash)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{exact}");
    assert_eq!(exact["total"], 1);
    assert_eq!(
        exact["data"][0]["txHash"],
        format!("0x{}", hex::encode(&tx_hash))
    );
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
    // Per-form reference counters (production invariant: written for every
    // observed reference form; the endpoint total reads them).
    store
        .put_script_reference_info_direct(
            0,
            &data_hash,
            &ScriptReferenceInfo {
                reference_hash: data_hash.clone(),
                hash_type: 0,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 100_00000000,
                lock_owned_capacity_sum: 100_00000000,
                lock_used_capacity_sum: 61_00000000,
                lock_owned_knowledge_sum: 61_00000000,
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
            occupied_capacity: 41_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
    );
    batch.put_cell_by_lock_code(&data_hash, 0, 123, &tx_hash, 0);
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
async fn test_cells_by_script_hash_type_filters_rows_strictly() {
    // Mixed-form fixture: two live cells share the same lock code_hash bytes
    // but use different hash_types (type vs data). The hash_type query
    // parameter must strictly filter rows, and total must match the filtered
    // row universe (per-form reference counters), not the cross-form counters.
    let store = test_store();
    seed_genesis_baseline(&store);

    let code_hash = vec![0x9b; 32];
    let tx_type_form = vec![0xa1; 32];
    let tx_data_form = vec![0xa2; 32];

    store
        .put_script_info_direct(
            &code_hash,
            &ScriptInfo {
                code_hash: code_hash.clone(),
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                lock_cells_count: 2,
                lock_live_cells_count: 2,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_reference_info_direct(
            1,
            &code_hash,
            &ScriptReferenceInfo {
                reference_hash: code_hash.clone(),
                hash_type: 1,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 100_00000000,
                lock_owned_capacity_sum: 100_00000000,
                lock_used_capacity_sum: 61_00000000,
                lock_owned_knowledge_sum: 61_00000000,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_reference_info_direct(
            0,
            &code_hash,
            &ScriptReferenceInfo {
                reference_hash: code_hash.clone(),
                hash_type: 0,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 61_00000000,
                lock_owned_capacity_sum: 61_00000000,
                lock_used_capacity_sum: 61_00000000,
                lock_owned_knowledge_sum: 61_00000000,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_type_form,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: code_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 41_00000000,
            udt_amount: None,
            data_hash: None,
        },
        10,
    );
    batch.put_cell_by_lock_code(&code_hash, 1, 10, &tx_type_form, 0);
    batch.put_cell(
        &tx_data_form,
        0,
        &LiveCellInfo {
            capacity: 61_00000000,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: code_hash.clone(),
            lock_hash_type: 0,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 41_00000000,
            udt_amount: None,
            data_hash: None,
        },
        11,
    );
    batch.put_cell_by_lock_code(&code_hash, 0, 11, &tx_data_form, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash={}&hash_type=type&script_kind=lock&limit=20",
            code_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        1,
        "hash_type=type must exclude the data-form cell"
    );
    assert_eq!(
        data[0]["txHash"],
        format!("0x{}", hex::encode(&tx_type_form))
    );
    assert_eq!(
        json["total"], 1,
        "total must match the filtered row universe"
    );

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash={}&hash_type=data&script_kind=lock&limit=20",
            code_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        1,
        "hash_type=data must exclude the type-form cell"
    );
    assert_eq!(
        data[0]["txHash"],
        format!("0x{}", hex::encode(&tx_data_form))
    );
    assert_eq!(
        json["total"], 1,
        "total must match the filtered row universe"
    );
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
            chain_time_ms: older_timestamp,
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
            chain_time_ms: newer_earliest_timestamp,
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
            chain_time_ms: newer_timestamp,
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
    put_canonical_script_tx(&mut batch, &older_tx_hash, older_block, 0, 3);
    put_canonical_script_tx(
        &mut batch,
        &newer_earliest_tx_hash,
        newer_earliest_block,
        0,
        3,
    );
    put_canonical_script_tx(&mut batch, &newer_tx_hash, newer_block, 0, 3);
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

async fn assert_registered_reference_missing_mapping(second_anchor: bool) {
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
            canonical_reference_hash: Some(if second_anchor {
                vec![0x9a; 32]
            } else {
                canonical_type_hash.clone()
            }),
            canonical_hash_type: Some(1),
            registered_references: if second_anchor {
                vec![(vec![0x9a; 32], 1), (canonical_type_hash.clone(), 1)]
            } else {
                vec![(canonical_type_hash.clone(), 1)]
            },
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
async fn test_get_script_fails_when_relevant_canonical_reference_mapping_missing() {
    assert_registered_reference_missing_mapping(false).await;
}

#[tokio::test]
async fn test_get_script_fails_when_second_registered_reference_mapping_missing() {
    assert_registered_reference_missing_mapping(true).await;
}

async fn assert_retired_family_usage_and_utilization(app: &axum::Router, active: bool) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Retired%20family/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let usage: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(usage["cellsCount"], 8);
    assert_eq!(usage["liveCellsCount"], 6);
    assert_eq!(usage["capacitySum"], "1000");
    assert_eq!(usage["commonKnowledgeSizeSum"], "600");
    assert_eq!(usage["ownedCapacitySum"], "800");
    assert_eq!(usage["ownedKnowledgeSum"], "500");
    let version_live: i64 = usage["byDeployment"]
        .as_array()
        .unwrap()
        .iter()
        .map(|version| version["liveCellsCount"].as_i64().unwrap())
        .sum();
    assert_eq!(version_live, if active { 6 } else { 0 });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/charts/most-utilized-scripts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let chart: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(chart["capacityShare"]["data"].as_array().unwrap().len(), 2);
    assert_eq!(chart["capacityShare"]["data"][0]["date"], "2024-01-15");
    assert_eq!(chart["capacityShare"]["data"][1]["date"], "2024-01-16");
    for (name, expected) in [
        ("capacityShare", ["1000", "800"]),
        ("usedShare", ["600", "500"]),
    ] {
        let share = &chart[name];
        let series = share["series"].as_array().unwrap();
        assert_eq!(
            series
                .iter()
                .filter(|s| s["label"] == "Retired family")
                .count(),
            1,
            "{chart}"
        );
        let family = series
            .iter()
            .find(|s| s["label"] == "Retired family")
            .unwrap();
        let key = family["key"].as_str().unwrap();
        assert_eq!(share["data"][0]["values"][key], expected[0]);
        assert_eq!(share["data"][1]["values"][key], expected[1]);
    }
}

async fn assert_type_id_history_without_current_binding(retired_payload: bool) {
    let store = test_store();
    // The complete-day chart walks the canonical height index to certify its
    // calendar boundary. A sparse header fixture is not an indexed chain.
    seed_block_headers_at(
        &store,
        &(0..=40)
            .map(|number| {
                let timestamp = if number == 40 {
                    utc_ms("2024-01-17T02:00:00Z")
                } else if number >= 31 {
                    utc_ms("2024-01-16T02:00:00Z") + (number - 31) * 1000
                } else {
                    utc_ms("2024-01-15T02:00:00Z") + number * 1000
                };
                (number, timestamp)
            })
            .collect::<Vec<_>>(),
    );
    set_script_fixture_block_tx_count(&store, 30, 2);
    set_script_fixture_block_tx_count(&store, 40, 2);
    let reference = vec![0x31; 32];
    let binary = vec![0x41; 32];
    let history = ScriptVersionInfo {
        version_hash: binary.clone(),
        family_id: Some("retired-family".to_string()),
        name: Some("Retired family".to_string()),
        canonical_reference_hash: Some(reference.clone()),
        canonical_hash_type: Some(1),
        type_id_deployments: vec![(reference.clone(), 10)],
        registered_references: vec![(reference.clone(), 1)],
        ..Default::default()
    };
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        "retired-family",
        &ScriptFamilyInfo {
            family_id: "retired-family".to_string(),
            name: "Retired family".to_string(),
            versions_count: 1,
            live_cells_count: 6,
            cells_count: 8,
            lock_cells_count: 8,
            owned_capacity_sum: 800,
            owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Retired family", "retired-family");
    batch.put_script_version(&binary, &history);
    batch.put_script_version_by_family("retired-family", &binary);
    batch.put_script_reference_info(
        1,
        &reference,
        &ScriptReferenceInfo {
            reference_hash: reference.clone(),
            hash_type: 1,
            // Cells using the retired code can remain live; these are not code cells.
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_capacity_sum: 1000,
            lock_owned_capacity_sum: 800,
            lock_used_capacity_sum: 600,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    if retired_payload {
        batch.put_cell(
            &[0x61; 32],
            0,
            &LiveCellInfo {
                type_script_hash: Some(reference.clone()),
                data_hash: Some(vec![0x51; 32]),
                data_size: 31_560,
                capacity: 3_200_000_000_000,
                occupied_capacity: 3_164_000_000_000,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some(
                    hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH).unwrap(),
                ),
                type_hash_type: Some(1),
                type_args: Some(vec![0x13; 32]),
                udt_amount: None,
            },
            20,
        );
        batch.put_cell_by_type(&reference, 20, &[0x61; 32], 0);
    }
    batch.commit().unwrap();
    store
        .put_script_daily_delta(
            &reference,
            1,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 1000,
                owned_knowledge_delta: 600,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &reference,
            1,
            false,
            20240116,
            &ScriptDailyDelta {
                owned_capacity_delta: -200,
                owned_knowledge_delta: -100,
            },
        )
        .unwrap();
    let app = create_router(test_config(store.clone())).await;
    for uri in [
        "/api/v1/scripts/Retired%20family".to_string(),
        "/api/v1/scripts/Retired%20family/charts/capacity-history?from=2024-01-15&to=2024-01-16"
            .to_string(),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            status,
            StatusCode::OK,
            "{uri}: {}",
            String::from_utf8_lossy(&body)
        );
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        if json.get("versions").is_some() {
            assert_eq!(
                json["versions"][0]["versionHash"],
                format!("0x{}", hex::encode(&binary))
            );
            assert_eq!(json["versions"][0]["references"], serde_json::json!([]));
            assert_eq!(json["liveCellsCount"], 6);
            assert_eq!(json["ownedCapacitySum"], "800");
            assert_eq!(json["ownedKnowledgeSum"], "500");
            assert_eq!(json["versions"][0]["liveCellsCount"], 0);
            assert_eq!(json["references"].as_array().unwrap().len(), 1);
            assert_eq!(
                json["references"][0]["currentVersionHash"],
                serde_json::Value::Null
            );
            assert_eq!(json["references"][0]["custody"]["class"], "retired");
        } else {
            assert_eq!(
                json["data"],
                serde_json::json!([
                    {"date": "2024-01-15", "values": {"used": "600", "unused": "400"}},
                    {"date": "2024-01-16", "values": {"used": "500", "unused": "300"}},
                ])
            );
        }
    }
    assert_retired_family_usage_and_utilization(&app, false).await;
    for suffix in ["", "&hash_type=type"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/scripts/code-cells?code_hash=0x{}{suffix}",
                        hex::encode(&reference)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["resolvedVersionHash"].is_null(), "{json}");
        assert_eq!(json["codeCells"], serde_json::json!([]));
    }
    assert_eq!(
        store
            .get_script_reference_version_hash(1, &reference)
            .unwrap(),
        None
    );
    assert_eq!(
        store.get_script_version(&binary).unwrap(),
        Some(history.clone())
    );

    // Reactivation adds a new observed binary and publishes its exact binding.
    // Historical provenance stays intact and only the new version owns usage.
    let active_binary = vec![0x71; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &active_binary,
        &ScriptVersionInfo {
            version_hash: active_binary.clone(),
            type_id_deployments: vec![(reference.clone(), 30)],
            registered_references: Vec::new(),
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_capacity_sum: 1000,
            lock_owned_capacity_sum: 800,
            lock_used_capacity_sum: 600,
            lock_owned_knowledge_sum: 500,
            ..history.clone()
        },
    );
    batch.put_script_version_by_family("retired-family", &active_binary);
    batch.put_script_reference_to_version(1, &reference, &active_binary);
    batch.delete_cell(&[0x61; 32], 0);
    let active_tx = vec![0x72; 32];
    batch.put_cell(
        &active_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            occupied_capacity: 61_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x12; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference.clone()),
            type_code_hash: Some(
                hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH).unwrap(),
            ),
            type_hash_type: Some(1),
            type_args: Some(vec![0x13; 32]),
            data_size: 32,
            data_hash: Some(active_binary.clone()),
            udt_amount: None,
        },
        30,
    );
    batch.put_cell_by_type(&reference, 30, &active_tx, 0);
    batch.put_cell_by_data_hash(&active_binary, 30, &active_tx, 0);
    put_canonical_script_tx(&mut batch, &active_tx, 30, 1, 1);
    batch.put_script_family(
        "retired-family",
        &ScriptFamilyInfo {
            family_id: "retired-family".to_string(),
            name: "Retired family".to_string(),
            versions_count: 2,
            live_cells_count: 6,
            cells_count: 8,
            lock_cells_count: 8,
            owned_capacity_sum: 800,
            owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    // Keep the pre-upgrade warmup cache: membership must use the same pinned
    // canonical metadata view as the newly published current binding.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Retired%20family")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let versions = json["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    let active = versions
        .iter()
        .find(|version| version["versionHash"] == format!("0x{}", hex::encode(&active_binary)))
        .unwrap();
    assert_eq!(
        active["references"][0]["referenceHash"],
        format!("0x{}", hex::encode(&reference))
    );
    assert_eq!(active["liveCellsCount"], 6);
    assert_retired_family_usage_and_utilization(&app, true).await;
    let response = app.clone().oneshot(Request::builder().uri(
        "/api/v1/scripts/Retired%20family/charts/capacity-history?from=2024-01-15&to=2024-01-16"
    ).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let chart: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        chart["data"],
        serde_json::json!([
            {"date": "2024-01-15", "values": {"used": "600", "unused": "400"}},
            {"date": "2024-01-16", "values": {"used": "500", "unused": "300"}},
        ])
    );
    for suffix in ["", "&hash_type=type"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/scripts/code-cells?code_hash=0x{}{suffix}",
                        hex::encode(&reference)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["resolvedVersionHash"],
            format!("0x{}", hex::encode(&active_binary))
        );
    }
    assert_eq!(
        store.get_script_version(&binary).unwrap(),
        Some(history.clone())
    );

    // Reusing an earlier binary updates a row already present in the warmup
    // cache. The family response must read its current counters, not that cache.
    let mut old_reactivated = store.get_script_version(&active_binary).unwrap().unwrap();
    old_reactivated.version_hash = binary.clone();
    old_reactivated.type_id_deployments = vec![(reference.clone(), 10), (reference.clone(), 40)];
    old_reactivated.registered_references = history.registered_references.clone();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(&binary, &old_reactivated);
    batch.put_script_version(
        &active_binary,
        &ScriptVersionInfo {
            version_hash: active_binary.clone(),
            type_id_deployments: vec![(reference.clone(), 30)],
            registered_references: Vec::new(),
            ..history
        },
    );
    batch.put_script_reference_to_version(1, &reference, &binary);
    batch.delete_cell(&[0x72; 32], 0);
    let consuming_tx = vec![0x74; 32];
    batch.put_consumed_cell_meta(&[0x72; 32], 0, 30, 40, Some(&consuming_tx));
    put_canonical_script_tx(&mut batch, &consuming_tx, 40, 0, 1);
    let reactivated_tx = vec![0x75; 32];
    batch.put_cell(
        &reactivated_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            occupied_capacity: 61_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x12; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference.clone()),
            type_code_hash: Some(
                hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH).unwrap(),
            ),
            type_hash_type: Some(1),
            type_args: Some(vec![0x13; 32]),
            data_size: 32,
            data_hash: Some(binary.clone()),
            udt_amount: None,
        },
        40,
    );
    batch.put_cell_by_type(&reference, 40, &reactivated_tx, 0);
    batch.put_cell_by_data_hash(&binary, 40, &reactivated_tx, 0);
    put_canonical_script_tx(&mut batch, &reactivated_tx, 40, 1, 1);
    batch.commit().unwrap();
    assert_retired_family_usage_and_utilization(&app, true).await;
    let mut inconsistent_family = store.get_script_family("retired-family").unwrap().unwrap();
    inconsistent_family.owned_capacity_sum += 1;
    store
        .put_script_family_direct("retired-family", &inconsistent_family)
        .unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Retired%20family/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("retired-family"),
        "{error}"
    );
}

#[tokio::test]
async fn test_type_id_retirement_family_and_current_code_endpoints() {
    assert_type_id_history_without_current_binding(true).await;
}

#[tokio::test]
async fn test_type_id_burn_family_and_current_code_endpoints() {
    assert_type_id_history_without_current_binding(false).await;
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
async fn test_get_script_usage_aggregates_family_references() {
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
            cells_count: 9,
            live_cells_count: 5,
            lock_cells_count: 4,
            type_cells_count: 5,
            owned_capacity_sum: 700,
            owned_knowledge_sum: 400,
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
    // Match the canonical reference facts from which the writer materializes
    // these version/family totals; unrelated ScriptInfo labels are not members.
    batch.put_script_reference_to_version(0, &version_a, &version_a);
    batch.put_script_reference_to_version(0, &version_b, &version_b);
    batch.put_script_reference_info(
        0,
        &version_a,
        &ScriptReferenceInfo {
            reference_hash: version_a.clone(),
            hash_type: 0,
            lock_cells_count: 4,
            lock_live_cells_count: 3,
            lock_capacity_sum: 500,
            lock_owned_capacity_sum: 300,
            lock_used_capacity_sum: 260,
            lock_owned_knowledge_sum: 180,
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        0,
        &version_b,
        &ScriptReferenceInfo {
            reference_hash: version_b.clone(),
            hash_type: 0,
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
    let version_a = vec![0xA1; 32];
    let version_b = vec![0xB1; 32];
    let name = "SECP256K1_BLAKE160".to_string();
    let family_id = "family:secp";

    // Family-shaped fixture: the {name} chart aggregates the family's member
    // reference forms (same membership computation as the usage counters).
    store
        .put_script_family_direct(
            family_id,
            &ScriptFamilyInfo {
                family_id: family_id.to_string(),
                name: name.clone(),
                versions_count: 2,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_family_name_direct(&name, family_id)
        .unwrap();
    for (version_hash, reference_hash) in [(&version_a, &code_hash_a), (&version_b, &code_hash_b)] {
        store
            .put_script_version(
                version_hash,
                &ScriptVersionInfo {
                    version_hash: (*version_hash).clone(),
                    family_id: Some(family_id.to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_version_by_family_direct(family_id, version_hash)
            .unwrap();
        store
            .put_script_reference_to_version_direct(1, reference_hash, version_hash)
            .unwrap();
        store
            .put_script_reference_info_direct(
                1,
                reference_hash,
                &ScriptReferenceInfo {
                    reference_hash: (*reference_hash).clone(),
                    hash_type: 1,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    store
        .put_script_daily_delta(
            &code_hash_a,
            1,
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
            1,
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
            1,
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
            1,
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
            chain_time_ms: 1_705_536_000_000,
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
async fn test_script_capacity_history_chart_aggregates_family_member_references() {
    // The family capacity chart must aggregate the SAME reference set the
    // usage counters are built from: every observed reference form resolving
    // to a family version — including members whose ScriptInfo label differs
    // from the family name (the USDI-inside-xUDT case).
    let store = test_store();

    let version_hash = vec![0x1c; 32];
    let r_main = vec![0x2a; 32];
    let r_alt = vec![0x2b; 32];
    seed_two_reference_script_family(
        &store,
        "xUDT",
        "family:xudt",
        &version_hash,
        &r_main,
        &r_alt,
        "USDI",
    );

    store
        .put_script_daily_delta(
            &r_main,
            1,
            true,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 200,
                owned_knowledge_delta: 120,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &r_alt,
            1,
            true,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
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
            chain_time_ms: 1_705_536_000_000,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/xUDT/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert!(!data.is_empty(), "family chart must have history");
    assert_eq!(data[0]["date"], "2024-01-15");
    let last = data.last().unwrap();
    // Family counters: owned 300 / knowledge 180 -> used 180, unused 120.
    // The chart end state must equal the family/usage counters exactly.
    assert_eq!(last["values"]["used"], "180");
    assert_eq!(last["values"]["unused"], "120");

    // Same-source lock: the usage endpoint totals must match the chart end state.
    let request = Request::builder()
        .uri("/api/v1/scripts/xUDT/usage")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let usage: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(usage["ownedCapacitySum"], "300");
    assert_eq!(usage["ownedKnowledgeSum"], "180");
}

#[tokio::test]
async fn test_script_capacity_chart_by_code_hash_with_kind_filter() {
    let store = test_store();
    let code_hash = vec![0x33; 32];
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    store
        .put_script_daily_delta(
            &code_hash,
            1,
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
            1,
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
            chain_time_ms: 1_705_363_200_000,
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
            1,
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
            // Raw tip time regressed, while its canonical chain day advanced.
            timestamp: 1_705_449_600_000,
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
            chain_time_ms: 1_705_536_000_000,
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

/// Two type-id deployments sharing one bytecode version (the RGB++ testnet
/// signet/testnet3 shape). Each lookup must serve the QUERIED reference's own
/// usage stats and its own deployment code cell — never the sibling
/// deployment's. Regression for the associated_code_hash single-slot redirect
/// that served the signet deployment's numbers for the testnet3 reference.
#[tokio::test]
async fn test_script_lookup_serves_queried_references_own_stats_for_shared_bytecode() {
    let store = test_store();

    // Shared bytecode version (data hash of the code cells).
    let version_hash = vec![0x7e; 32];
    // Two independent type-id deployments of the same binary.
    let ref_signet = vec![0xd0; 32];
    let ref_testnet3 = vec![0x61; 32];
    let signet_code_cell_tx = vec![0xaa; 32];
    let testnet3_code_cell_tx = vec![0xbb; 32];

    // Labeled version row, as label import writes it for the family.
    store
        .put_script_version(
            &version_hash,
            &ScriptVersionInfo {
                version_hash: version_hash.clone(),
                name: Some("RGB++".to_string()),
                description: Some("RGB++ Lock".to_string()),
                canonical_reference_hash: Some(ref_signet.clone()),
                canonical_hash_type: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

    // Both references resolve to the shared version.
    store
        .put_script_reference_to_version_direct(1, &ref_signet, &version_hash)
        .unwrap();
    store
        .put_script_reference_to_version_direct(1, &ref_testnet3, &version_hash)
        .unwrap();

    // Per-reference usage truth (audit numbers from the live testnet capture).
    store
        .put_script_info_direct(
            &ref_signet,
            &ScriptInfo {
                code_hash: ref_signet.clone(),
                hash_type: 1,
                name: Some("RGB++".to_string()),
                lock_cells_count: 1337,
                lock_live_cells_count: 680,
                lock_capacity_sum: 20_000_000_000_000,
                lock_owned_capacity_sum: 17_587_097_961_011,
                lock_used_capacity_sum: 12_000_000_000_000,
                lock_owned_knowledge_sum: 11_771_100_000_000,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &ref_testnet3,
            &ScriptInfo {
                code_hash: ref_testnet3.clone(),
                hash_type: 1,
                name: Some("RGB++".to_string()),
                lock_cells_count: 21009,
                lock_live_cells_count: 12486,
                lock_capacity_sum: 480_000_000_000_000,
                lock_owned_capacity_sum: 475_592_296_325_729,
                lock_used_capacity_sum: 280_000_000_000_000,
                lock_owned_knowledge_sum: 279_292_400_000_000,
                ..Default::default()
            },
        )
        .unwrap();

    // Each deployment's own code cell carries the same bytecode.
    let mut batch = StoreBatch::new(store.as_ref());
    for (tx_hash, type_hash, block) in [
        (&signet_code_cell_tx, &ref_signet, 100i64),
        (&testnet3_code_cell_tx, &ref_testnet3, 200i64),
    ] {
        batch.put_cell(
            tx_hash,
            0,
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
                data_size: 64,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: Some(version_hash.clone()),
            },
            block,
        );
        batch.put_cell_by_type(type_hash, block, tx_hash, 0);
        batch.put_cell_by_data_hash(&version_hash, block, tx_hash, 0);
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let ref_signet_hex = format!("0x{}", hex::encode(&ref_signet));
    let ref_testnet3_hex = format!("0x{}", hex::encode(&ref_testnet3));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}","{}"]}}"#,
            ref_signet_hex, ref_testnet3_hex
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // The testnet3 reference serves ITS OWN stats and ITS OWN code cell.
    assert_eq!(json[&ref_testnet3_hex]["name"], "RGB++");
    assert_eq!(json[&ref_testnet3_hex]["liveCellsCount"], 12486);
    assert_eq!(
        json[&ref_testnet3_hex]["ownedCapacitySum"],
        "475592296325729"
    );
    assert_eq!(
        json[&ref_testnet3_hex]["ownedKnowledgeSum"],
        "279292400000000"
    );
    assert_eq!(
        json[&ref_testnet3_hex]["codeCellTxHash"],
        format!("0x{}", hex::encode(&testnet3_code_cell_tx))
    );
    assert_eq!(json[&ref_testnet3_hex]["codeCellsTotal"], 1);
    assert_eq!(json[&ref_testnet3_hex]["codeCellsLiveCount"], 1);

    // The signet reference likewise serves only its own deployment.
    assert_eq!(json[&ref_signet_hex]["liveCellsCount"], 680);
    assert_eq!(json[&ref_signet_hex]["ownedCapacitySum"], "17587097961011");
    assert_eq!(json[&ref_signet_hex]["ownedKnowledgeSum"], "11771100000000");
    assert_eq!(
        json[&ref_signet_hex]["codeCellTxHash"],
        format!("0x{}", hex::encode(&signet_code_cell_tx))
    );
    assert_eq!(json[&ref_signet_hex]["codeCellsTotal"], 1);
}

/// An ambiguous type reference (two live bytecode versions) still describes
/// the queried reference itself: a deprecated flag recorded for the reference
/// must surface instead of a hardcoded `false`.
#[tokio::test]
async fn test_script_lookup_ambiguous_reference_reports_reference_deprecated() {
    let store = test_store();

    let reference_hash = vec![0x66; 32];
    let version_a = vec![0x67; 32];
    let version_b = vec![0x68; 32];

    store
        .put_script_info_direct(
            &reference_hash,
            &ScriptInfo {
                code_hash: reference_hash.clone(),
                hash_type: 1,
                name: Some("Old Deprecated Lock".to_string()),
                deprecated: true,
                lock_cells_count: 5,
                lock_live_cells_count: 2,
                lock_capacity_sum: 1_000,
                lock_owned_capacity_sum: 500,
                lock_used_capacity_sum: 800,
                lock_owned_knowledge_sum: 400,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    for (tx_hash, data_hash, block) in [
        (vec![0xa1; 32], &version_a, 10i64),
        (vec![0xa2; 32], &version_b, 11i64),
    ] {
        batch.put_cell(
            &tx_hash,
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
                data_size: 64,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: Some(data_hash.clone()),
            },
            block,
        );
        batch.put_cell_by_type(&reference_hash, block, &tx_hash, 0);
        batch.put_cell_by_data_hash(data_hash, block, &tx_hash, 0);
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            reference_hash_hex
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&reference_hash_hex]["resolutionState"], "ambiguous");
    assert_eq!(json[&reference_hash_hex]["name"], "Old Deprecated Lock");
    assert_eq!(
        json[&reference_hash_hex]["deprecated"], true,
        "the queried reference is marked deprecated in the store; the ambiguous fallback record must not hardcode false"
    );
    assert_eq!(
        json[&reference_hash_hex]["ambiguity"]["versionHashes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

async fn post_script_lookup(app: axum::Router, code_hashes: &[&str]) -> serde_json::Value {
    let body = serde_json::json!({ "codeHashes": code_hashes }).to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

/// The lookup carries the queried reference's own label description, so the
/// cell page can explain a protocol lock (e.g. the Cells Account Lock's empty
/// args) without a second request. A reference without one answers `null`,
/// present in the response, not an absent key.
#[tokio::test]
async fn test_script_lookup_serves_the_references_description() {
    let store = test_store();

    let version_hash = vec![0x7a; 32];
    let described = vec![0x7b; 32];
    let undescribed = vec![0x7c; 32];
    const DESCRIPTION: &str = "Lock a .cell name sits under. Always-success by design: \
        authority is decided by the Cells Account type script from the owner and manager \
        hashes in the cell.";

    store
        .put_script_version(
            &version_hash,
            &ScriptVersionInfo {
                version_hash: version_hash.clone(),
                name: Some("Cells Account Lock".to_string()),
                canonical_reference_hash: Some(described.clone()),
                canonical_hash_type: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    for reference in [&described, &undescribed] {
        store
            .put_script_reference_to_version_direct(1, reference, &version_hash)
            .unwrap();
    }
    store
        .put_script_info_direct(
            &described,
            &ScriptInfo {
                code_hash: described.clone(),
                hash_type: 1,
                name: Some("Cells Account Lock".to_string()),
                description: Some(DESCRIPTION.to_string()),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &undescribed,
            &ScriptInfo {
                code_hash: undescribed.clone(),
                hash_type: 1,
                name: Some("Cells Account Lock".to_string()),
                description: None,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    for (tx_hash, type_hash, block) in [
        (vec![0xb1; 32], &described, 100i64),
        (vec![0xb2; 32], &undescribed, 200i64),
    ] {
        batch.put_cell(
            &tx_hash,
            0,
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
                data_size: 64,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: Some(version_hash.clone()),
            },
            block,
        );
        batch.put_cell_by_type(type_hash, block, &tx_hash, 0);
        batch.put_cell_by_data_hash(&version_hash, block, &tx_hash, 0);
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let described_hex = format!("0x{}", hex::encode(&described));
    let undescribed_hex = format!("0x{}", hex::encode(&undescribed));
    let json = post_script_lookup(app, &[&described_hex, &undescribed_hex]).await;

    assert_eq!(json[&described_hex]["resolutionState"], "resolved");
    assert_eq!(json[&described_hex]["description"], DESCRIPTION);
    let undescribed_entry = json[&undescribed_hex]
        .as_object()
        .unwrap_or_else(|| panic!("lookup entry missing: {json}"));
    assert_eq!(
        undescribed_entry.get("description"),
        Some(&serde_json::Value::Null),
        "a reference without a description answers null: {json}"
    );
}

/// The ambiguous branch describes the queried reference too.
#[tokio::test]
async fn test_script_lookup_ambiguous_reference_serves_reference_description() {
    let store = test_store();

    let reference_hash = vec![0x6a; 32];
    let version_a = vec![0x6b; 32];
    let version_b = vec![0x6c; 32];

    store
        .put_script_info_direct(
            &reference_hash,
            &ScriptInfo {
                code_hash: reference_hash.clone(),
                hash_type: 1,
                name: Some("Described Lock".to_string()),
                description: Some("What this lock decides.".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    for (tx_hash, data_hash, block) in [
        (vec![0xc1; 32], &version_a, 10i64),
        (vec![0xc2; 32], &version_b, 11i64),
    ] {
        batch.put_cell(
            &tx_hash,
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
                data_size: 64,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: Some(data_hash.clone()),
            },
            block,
        );
        batch.put_cell_by_type(&reference_hash, block, &tx_hash, 0);
        batch.put_cell_by_data_hash(data_hash, block, &tx_hash, 0);
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let json = post_script_lookup(app, &[&reference_hash_hex]).await;

    assert_eq!(json[&reference_hash_hex]["resolutionState"], "ambiguous");
    assert_eq!(
        json[&reference_hash_hex]["description"],
        "What this lock decides."
    );
}

#[tokio::test]
async fn test_bundled_mainnet_usdi_deployment_belongs_to_xudt_family() {
    let store = test_store();
    let result = run_label_import_bundled(store.as_ref(), "mainnet").unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let reference =
        hex::decode("bfa35a9c38a676682b65ade8f02be164d48632281477e36f8dc2f41f79e56bfc").unwrap();
    let binary =
        hex::decode("50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95").unwrap();
    assert_eq!(
        store
            .get_script_info(&reference)
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("xUDT")
    );
    let version = store.get_script_version(&binary).unwrap().unwrap();
    assert_eq!(version.family_id.as_deref(), Some("xudt"));
    assert_eq!(version.canonical_reference_hash, Some(binary.clone()));
    assert_eq!(version.canonical_hash_type, Some(2));
    assert_eq!(
        version.registered_references,
        vec![(binary, 2), (reference, 1)]
    );
    assert!(store.get_script_family("usdi-asset").unwrap().is_none());
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/USDI%20Asset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_unregistered_type_id_copy_changes_current_family_without_permanent_membership() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &(0..=12)
            .map(|number| {
                let timestamp = if number == 12 {
                    utc_ms("2024-01-17T02:00:00Z")
                } else {
                    utc_ms("2024-01-15T02:00:00Z") + number * 1000
                };
                (number, timestamp)
            })
            .collect::<Vec<_>>(),
    );
    set_script_fixture_block_tx_count(&store, 10, 2);
    set_script_fixture_block_tx_count(&store, 11, 2);
    let reference = vec![0x31; 32];
    let binaries = [vec![0x41; 32], vec![0x51; 32]];
    let anchors = [vec![0x61; 32], vec![0x71; 32]];
    let families = ["First", "Second"];
    store
        .put_script_reference_info_direct(
            1,
            &reference,
            &ScriptReferenceInfo {
                reference_hash: reference.clone(),
                hash_type: 1,
                lock_live_cells_count: 6,
                lock_cells_count: 8,
                lock_capacity_sum: 1000,
                lock_used_capacity_sum: 600,
                lock_owned_capacity_sum: 800,
                lock_owned_knowledge_sum: 500,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &reference,
            1,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 800,
                owned_knowledge_delta: 500,
            },
        )
        .unwrap();
    for active in [Some(0), Some(1), None] {
        if let Some(index) = active {
            store
                .put_script_reference_to_version_direct(1, &reference, &binaries[index])
                .unwrap();
        } else {
            store
                .delete_script_reference_to_version_direct(1, &reference)
                .unwrap();
        }
        let mut batch = StoreBatch::new(store.as_ref());
        if active != Some(0) {
            let previous = if active == Some(1) { 0 } else { 1 };
            let previous_tx = vec![0x81 + previous as u8; 32];
            let consuming_tx = vec![0x91 + previous as u8; 32];
            batch.delete_cell(&previous_tx, 0);
            batch.put_consumed_cell_meta(
                &previous_tx,
                0,
                10 + previous as i64,
                11 + previous as i64,
                Some(&consuming_tx),
            );
            put_canonical_script_tx(&mut batch, &consuming_tx, 11 + previous as i64, 0, 1);
        }
        if let Some(index) = active {
            let tx = vec![0x81 + index as u8; 32];
            batch.put_cell(
                &tx,
                0,
                &LiveCellInfo {
                    capacity: 100_00000000,
                    occupied_capacity: 61_00000000,
                    lock_script_hash: vec![0x11; 32],
                    lock_code_hash: vec![0x12; 32],
                    lock_hash_type: 1,
                    lock_args: vec![],
                    type_script_hash: Some(reference.clone()),
                    type_code_hash: Some(
                        hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH).unwrap(),
                    ),
                    type_hash_type: Some(1),
                    type_args: Some(vec![0x13; 32]),
                    data_size: 32,
                    data_hash: Some(binaries[index].clone()),
                    udt_amount: None,
                },
                10 + index as i64,
            );
            batch.put_cell_by_type(&reference, 10 + index as i64, &tx, 0);
            batch.put_cell_by_data_hash(&binaries[index], 10 + index as i64, &tx, 0);
            put_canonical_script_tx(&mut batch, &tx, 10 + index as i64, 1, 1);
        }
        for (index, family) in families.iter().enumerate() {
            let owns = active == Some(index);
            batch.put_script_family(
                family,
                &ScriptFamilyInfo {
                    family_id: family.to_string(),
                    name: family.to_string(),
                    versions_count: 1,
                    live_cells_count: if owns { 6 } else { 0 },
                    cells_count: if owns { 8 } else { 0 },
                    lock_cells_count: if owns { 8 } else { 0 },
                    owned_capacity_sum: if owns { 800 } else { 0 },
                    owned_knowledge_sum: if owns { 500 } else { 0 },
                    ..Default::default()
                },
            );
            batch.put_script_family_by_name(family, family);
            batch.put_script_version_by_family(family, &binaries[index]);
            batch.put_script_version(
                &binaries[index],
                &ScriptVersionInfo {
                    version_hash: binaries[index].clone(),
                    family_id: Some(family.to_string()),
                    name: Some(family.to_string()),
                    canonical_reference_hash: Some(anchors[index].clone()),
                    canonical_hash_type: Some(1),
                    registered_references: vec![(anchors[index].clone(), 1)],
                    // The copy is a real TypeID deployment, but not a registered family anchor.
                    type_id_deployments: vec![(reference.clone(), 10 + index as i64)],
                    lock_live_cells_count: if owns { 6 } else { 0 },
                    lock_cells_count: if owns { 8 } else { 0 },
                    lock_capacity_sum: if owns { 1000 } else { 0 },
                    lock_used_capacity_sum: if owns { 600 } else { 0 },
                    lock_owned_capacity_sum: if owns { 800 } else { 0 },
                    lock_owned_knowledge_sum: if owns { 500 } else { 0 },
                    ..Default::default()
                },
            );
        }
        batch.commit().unwrap();
        let app = create_router(test_config(store.clone())).await;
        for (index, family) in families.iter().enumerate() {
            let owns = active == Some(index);
            for suffix in [
                "",
                "/usage",
                "/charts/capacity-history?from=2024-01-15&to=2024-01-15",
            ] {
                let uri = format!("/api/v1/scripts/{family}{suffix}");
                let response = app
                    .clone()
                    .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "{uri}: {}",
                    String::from_utf8_lossy(&bytes)
                );
                let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                if suffix.starts_with("/charts") {
                    if owns {
                        assert_eq!(
                            json["data"][0]["values"],
                            serde_json::json!({"used":"500","unused":"300"})
                        );
                    } else {
                        assert_eq!(json["data"], serde_json::json!([]));
                    }
                } else {
                    assert_eq!(json["liveCellsCount"], if owns { 6 } else { 0 });
                    assert_eq!(json["ownedCapacitySum"], if owns { "800" } else { "0" });
                    if suffix.is_empty() {
                        let refs = json["versions"][0]["references"].as_array().unwrap();
                        assert_eq!(refs.len(), usize::from(owns));
                        if owns {
                            assert_eq!(
                                refs[0]["referenceHash"],
                                format!("0x{}", hex::encode(&reference))
                            );
                        }
                    }
                }
            }
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/charts/most-utilized-scripts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["capacityShare"]["data"].as_array().unwrap().len(), 2);
        assert_eq!(json["capacityShare"]["data"][0]["date"], "2024-01-15");
        assert_eq!(json["capacityShare"]["data"][1]["date"], "2024-01-16");
        for (chart, expected) in [("capacityShare", 800), ("usedShare", 500)] {
            let total: i128 = json[chart]["data"][0]["values"]
                .as_object()
                .unwrap()
                .values()
                .map(|value| value.as_str().unwrap().parse::<i128>().unwrap())
                .sum();
            assert_eq!(
                total, expected,
                "retirement must preserve loose reference history: {json}"
            );
        }
        let series = json["capacityShare"]["series"].as_array().unwrap();
        for (index, family) in families.iter().enumerate() {
            assert_eq!(
                series.iter().filter(|row| row["label"] == *family).count(),
                usize::from(active == Some(index)),
                "{json}"
            );
        }
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/scripts/code-cells?code_hash=0x{}&hash_type=type",
                        hex::encode(&reference)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json["resolvedVersionHash"],
            serde_json::json!(active.map(|index| format!("0x{}", hex::encode(&binaries[index]))))
        );
    }
}

#[tokio::test]
async fn test_script_versions_order_same_block_by_canonical_tx_index() {
    let store = test_store();
    seed_block_headers_at(&store, &[(10, 1_704_153_601_000)]);
    set_script_fixture_block_tx_count(&store, 10, 2);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        "same-block-order",
        &ScriptFamilyInfo {
            family_id: "same-block-order".to_string(),
            name: "Same Block Order".to_string(),
            versions_count: 2,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Same Block Order", "same-block-order");
    for (hash_byte, tx_byte, tx_index) in [(0xf1u8, 0x31u8, 0i32), (0x11, 0x32, 1)] {
        let version = vec![hash_byte; 32];
        let tx = vec![tx_byte; 32];
        batch.put_script_version_by_family("same-block-order", &version);
        batch.put_script_version(
            &version,
            &ScriptVersionInfo {
                version_hash: version.clone(),
                family_id: Some("same-block-order".to_string()),
                registered_references: vec![(version.clone(), 0)],
                canonical_reference_hash: Some(version.clone()),
                canonical_hash_type: Some(0),
                ..Default::default()
            },
        );
        batch.put_script_reference_info(
            0,
            &version,
            &ScriptReferenceInfo {
                reference_hash: version.clone(),
                hash_type: 0,
                ..Default::default()
            },
        );
        batch.put_script_reference_to_version(0, &version, &version);
        batch.put_cell(
            &tx,
            0,
            &LiveCellInfo {
                capacity: 100_00000000,
                lock_script_hash: vec![0x21; 32],
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
                data_hash: Some(version.clone()),
            },
            10,
        );
        batch.put_cell_by_data_hash(&version, 10, &tx, 0);
        put_canonical_script_tx(&mut batch, &tx, 10, tx_index, 1);
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Same%20Block%20Order")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        detail["versions"][0]["versionHash"],
        format!("0x{}", hex::encode(vec![0x11; 32]))
    );
    assert_eq!(detail["versions"][0]["firstDeployedTxIndex"], 1);
    assert_eq!(detail["history"][0]["txIndex"], 0);
    assert_eq!(detail["history"][1]["txIndex"], 1);
}

async fn chronology_error_body_with_index(
    creation_location: Option<(i64, i32)>,
    consumption_location: Option<(i64, i32)>,
    consumed_at_block: i64,
    creation_indexed: bool,
    outputs_count: i16,
) -> (StatusCode, String) {
    let store = test_store();
    seed_block_headers_at(&store, &[(10, 1_704_153_601_000), (11, 1_704_153_602_000)]);
    set_script_fixture_block_tx_count(&store, 10, 2);
    let version = vec![0x71; 32];
    let tx = vec![0x72; 32];
    let consumer = vec![0x73; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        "chronology-error",
        &ScriptFamilyInfo {
            family_id: "chronology-error".to_string(),
            name: "Chronology Error".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Chronology Error", "chronology-error");
    batch.put_script_version_by_family("chronology-error", &version);
    batch.put_script_version(
        &version,
        &ScriptVersionInfo {
            version_hash: version.clone(),
            family_id: Some("chronology-error".to_string()),
            registered_references: vec![(version.clone(), 0)],
            canonical_reference_hash: Some(version.clone()),
            canonical_hash_type: Some(0),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        0,
        &version,
        &ScriptReferenceInfo {
            reference_hash: version.clone(),
            hash_type: 0,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(0, &version, &version);
    batch.put_consumed_cell_with_consumer(
        &tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x21; 32],
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
            data_hash: Some(version.clone()),
        },
        10,
        consumed_at_block,
        Some(&consumer),
    );
    batch.put_cell_by_data_hash(&version, 10, &tx, 0);
    if let Some((block, index)) = creation_location {
        if creation_indexed {
            put_canonical_script_tx(&mut batch, &tx, block, index, outputs_count);
        } else {
            batch.put_tx_hash_map(&tx, block, index);
        }
    }
    if let Some((block, index)) = consumption_location {
        put_canonical_script_tx(&mut batch, &consumer, block, index, 1);
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Chronology%20Error")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    (status, body)
}

async fn chronology_error_body(
    creation_location: Option<(i64, i32)>,
    consumption_location: Option<(i64, i32)>,
    consumed_at_block: i64,
) -> (StatusCode, String) {
    chronology_error_body_with_index(
        creation_location,
        consumption_location,
        consumed_at_block,
        true,
        1,
    )
    .await
}

#[tokio::test]
async fn test_script_history_requires_canonical_creation_and_consumption_locations() {
    for (creation, consumption, expected) in [
        (
            None,
            Some((11, 0)),
            "code cell 0x7272727272727272727272727272727272727272727272727272727272727272:0 at block 10 lacks canonical transaction location",
        ),
        (
            Some((10, 0)),
            None,
            "code cell consumption transaction",
        ),
    ] {
        let (status, body) = chronology_error_body(creation, consumption, 11).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert!(body.contains(expected), "{body}");
        assert!(body.contains("lacks canonical"), "{body}");
    }
}

#[tokio::test]
async fn test_script_history_rejects_consumer_not_after_creator() {
    let (status, body) = chronology_error_body(Some((10, 1)), Some((10, 0)), 10).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body.contains("does not follow creation"), "{body}");
}

#[tokio::test]
async fn test_script_history_rejects_missing_canonical_tx_index_and_output_bound() {
    let (status, body) = chronology_error_body_with_index(Some((10, 0)), None, 11, false, 1).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body.contains("tx_hash_map points to missing tx_index"),
        "{body}"
    );

    let (status, body) = chronology_error_body_with_index(Some((10, 0)), None, 11, true, 0).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body.contains("outside canonical transaction output count 0"),
        "{body}"
    );
}

#[tokio::test]
async fn test_native_type_id_rejects_persisted_binding_conflicting_with_live_code() {
    let store = test_store();
    let reference = vec![0x31; 32];
    let mapped_binary = vec![0x41; 32];
    let actual_binary = vec![0x42; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        "binding-conflict",
        &ScriptFamilyInfo {
            family_id: "binding-conflict".to_string(),
            name: "Binding Conflict".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Binding Conflict", "binding-conflict");
    batch.put_script_version_by_family("binding-conflict", &mapped_binary);
    batch.put_script_version(
        &mapped_binary,
        &ScriptVersionInfo {
            version_hash: mapped_binary.clone(),
            family_id: Some("binding-conflict".to_string()),
            type_id_deployments: vec![(reference.clone(), 10)],
            registered_references: vec![(reference.clone(), 1)],
            canonical_reference_hash: Some(reference.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &reference,
        &ScriptReferenceInfo {
            reference_hash: reference.clone(),
            hash_type: 1,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(1, &reference, &mapped_binary);
    batch.put_cell(
        &[0x61; 32],
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            occupied_capacity: 61_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x12; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference.clone()),
            type_code_hash: Some(
                hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH).unwrap(),
            ),
            type_hash_type: Some(1),
            type_args: Some(vec![0x13; 32]),
            data_size: 32,
            data_hash: Some(actual_binary),
            udt_amount: None,
        },
        10,
    );
    batch.put_cell_by_type(&reference, 10, &[0x61; 32], 0);
    batch.commit().unwrap();
    let app = create_router(test_config(store.clone())).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Binding%20Conflict")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains("conflicts with live native Type ID code"),
        "{body}"
    );
    let mut batch = StoreBatch::new(store.as_ref());
    batch.delete_cell(&[0x61; 32], 0);
    batch.commit().unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/scripts/Binding%20Conflict")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("has no live code cell"), "{body}");
}
