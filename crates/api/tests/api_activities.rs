use crate::common::*;

#[tokio::test]
async fn test_address_activities_reads_from_store() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x22; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xba; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&tx_hash, 10, 0);
    core_batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            timestamp: 1_700_000_000_000,
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
            chain_time_ms: 1_700_000_000_000,
        },
    );
    let actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    core_batch.put_tx_actions(&actions);
    core_batch.put_addr_tx(&lock_hash, 10, 0, &AddrTxValue::new(0, false, true, 0));
    core_batch.commit().unwrap();
    sync_address_tx_counters(&core_store, &lock_hash);
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
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

/// Seed one committed activity (block 88, tx index 1) for `lock_hash` and
/// return the JSON of that address's activity list.
async fn address_activities_json(
    lock_hash: &[u8],
    actions: &ckbadger_store::types::TxActions,
) -> serde_json::Value {
    let (status, json) = address_activities_response(lock_hash, actions).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

/// [`address_activities_json`] without the status assertion.
async fn address_activities_response(
    lock_hash: &[u8],
    actions: &ckbadger_store::types::TxActions,
) -> (StatusCode, serde_json::Value) {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_tx_hash_map(&actions.tx_hash, 88, 1);
    batch.put_tx_index(
        88,
        1,
        &TxIndexEntry {
            tx_hash: actions.tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_123,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        88,
        &CachedBlockHeader {
            hash: actions.block_hash.clone(),
            timestamp: 1_700_000_123,
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
            chain_time_ms: 1_700_000_123,
        },
    );
    batch.put_tx_actions(actions);
    batch.put_addr_tx(lock_hash, 88, 1, &AddrTxValue::new(0, false, true, 0));
    batch.commit().unwrap();
    sync_address_tx_counters(&core_store, lock_hash);
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 88;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn test_address_activities_returns_protocol_metadata() {
    let lock_hash = vec![0x24; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];

    let mut actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 88, 1, 100, 0);
    actions.protocol_actions = vec![ProtocolAction::new(
        "stablepp",
        "deposit",
        serde_json::json!({
            "hasIntent": true,
            "vaultCount": 2,
        }),
    )];

    let json = address_activities_json(&lock_hash, &actions).await;
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["protocolActions"][0]["protocol"],
        "stablepp"
    );
    assert_eq!(
        json["data"][0]["protocolActions"][0]["metadata"]["hasIntent"],
        true
    );
    // Every integer in metadata is an exact decimal string (BE-5 contract).
    assert_eq!(
        json["data"][0]["protocolActions"][0]["metadata"]["vaultCount"],
        "2"
    );
}

/// Each protocol action carries its display name from the bundled registry
/// (`docs/metadata/protocols.toml`), so no client keeps its own copy of the
/// table. DAO actions come from the activity builder, not a detector, and are
/// named from the same source.
#[tokio::test]
async fn test_activity_protocol_actions_carry_registry_display_names() {
    let lock_hash = vec![0x24; 32];
    let mut actions = make_test_tx_actions(&lock_hash, &[0xaa; 32], &[0xbb; 32], 88, 1, 100, 0);
    actions.protocol_actions = vec![
        ProtocolAction::new("fiber", "channel_open", serde_json::json!({})),
        ProtocolAction::new("dao", "deposit", serde_json::json!({})),
        ProtocolAction::new("rgbpp", "transfer", serde_json::json!({})),
        ProtocolAction::new("stablepp", "deposit", serde_json::json!({})),
        ProtocolAction::new("utxoswap", "swap_submitted", serde_json::json!({})),
        ProtocolAction::new("dotcell", "register", serde_json::json!({})),
    ];

    let json = address_activities_json(&lock_hash, &actions).await;
    let names: Vec<(&str, &str)> = json["data"][0]["protocolActions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["protocol"].as_str().unwrap(),
                a["protocolName"].as_str().unwrap_or("<missing>"),
            )
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ("fiber", "Fiber"),
            ("dao", "Nervos DAO"),
            ("rgbpp", "RGB++"),
            ("stablepp", "Stable++"),
            ("utxoswap", "UTXOSwap"),
            ("dotcell", ".cell"),
        ],
        "{json}"
    );
}

/// W2: the `dotbit` actions the indexer persists (one per account, each with
/// `metadata.accountId`) project through the registry with their display
/// name and the account id intact, on the address feed and on the global
/// Identity bucket, where every `.bit` transaction is indexed.
#[tokio::test]
async fn dotbit_protocol_actions_project_with_their_registry_name() {
    let lock_hash = vec![0x25; 32];
    let account_id = "0x2d2ed79a32f078c511108765a384a183e14990e9";
    let mut actions = make_test_tx_actions(&lock_hash, &[0xab; 32], &[0xbc; 32], 88, 1, 0, 0);
    actions.protocol_actions = vec![ProtocolAction::new(
        "dotbit",
        "renew_account",
        serde_json::json!({ "accountId": account_id }),
    )];
    for participant in &mut actions.participants {
        participant.tags |= ckbadger_store::types::TAG_PROTOCOL;
    }

    let json = address_activities_json(&lock_hash, &actions).await;
    let action = &json["data"][0]["protocolActions"][0];
    assert_eq!(action["protocol"], "dotbit", "{json}");
    assert_eq!(action["protocolName"], ".bit", "{json}");
    assert_eq!(action["action"], "renew_account", "{json}");
    assert_eq!(action["metadata"]["accountId"], account_id, "{json}");

    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    put_tx_actions_indexed(&mut batch, &actions);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, global) = fetch_json(app, "/api/v1/activities?filter=identity&limit=10").await;
    assert_eq!(status, StatusCode::OK, "{global}");
    let rows = global["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 1, "{global}");
    assert_eq!(
        rows[0]["protocolActions"][0]["protocolName"], ".bit",
        "{global}"
    );
}

/// A protocol id with no registry entry is an invariant violation — the
/// response fails naming it rather than showing the raw id.
#[tokio::test]
async fn test_activity_protocol_action_without_a_registry_name_is_an_error() {
    let lock_hash = vec![0x24; 32];
    let mut actions = make_test_tx_actions(&lock_hash, &[0xaa; 32], &[0xbb; 32], 88, 1, 100, 0);
    actions.protocol_actions = vec![ProtocolAction::new(
        "unlisted",
        "transfer",
        serde_json::json!({}),
    )];

    let (status, json) = address_activities_response(&lock_hash, &actions).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("unlisted") && message.contains("protocols.toml"),
        "{json}"
    );
}

/// Regression (API-020): a browser parses every JSON number as a double, so an
/// integer above 2^53 — a DAO cell of more than ~90M CKB, a Fiber commitment's
/// since-encoded delay — was rounded before the frontend saw it. Free-form
/// JSON the API passes through (protocol-action metadata, decoded lock args)
/// carries every integer as an exact decimal string, whatever its size.
#[tokio::test]
async fn test_activity_free_form_json_carries_integers_as_exact_strings() {
    let lock_hash = vec![0x24; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];

    let mut actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 88, 1, 100, 0);
    // 2^53 + 1 and 2^53 + 3 shannons: not representable as a double.
    actions.protocol_actions = vec![ProtocolAction::new(
        "dao",
        "withdraw_complete",
        serde_json::json!({
            "capacity": 9_007_199_254_740_993_i64,
            "compensation": 9_007_199_254_740_995_i64,
            "lockHash": "24",
            "nested": [{ "depositBlock": 19_695_550 }],
        }),
    )];
    // Fiber commitment lock args: pubkey hash, delay_epoch (u64 LE; a relative
    // epoch `since` with its flag bits set), version (u64 BE).
    let mut args = vec![0x11; 20];
    args.extend_from_slice(&0xa001_0000_0000_0006u64.to_le_bytes());
    args.extend_from_slice(&7u64.to_be_bytes());
    actions.lock_calls = vec![ckbadger_store::types::LockCallEntry {
        lock_code_hash: hex::decode(
            "2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c",
        )
        .unwrap(),
        lock_hash_type: 1,
        lock_args: args,
    }];

    let json = address_activities_json(&lock_hash, &actions).await;
    let metadata = &json["data"][0]["protocolActions"][0]["metadata"];
    assert_eq!(metadata["capacity"], "9007199254740993", "{json}");
    assert_eq!(metadata["compensation"], "9007199254740995", "{json}");
    assert_eq!(metadata["lockHash"], "24", "{json}");
    assert_eq!(metadata["nested"][0]["depositBlock"], "19695550", "{json}");
    let decoded = &json["data"][0]["lockCalls"][0]["decoded"];
    assert_eq!(decoded["delayEpoch"], "11529496521045180422", "{json}");
    assert_eq!(decoded["version"], "7", "{json}");
    assert_eq!(decoded["protocol"], "fiber", "{json}");
}

/// R-8 ruling: an activity lock call whose args do not fit the lock's
/// registry layout is served (200) with `decoded: null` and a `decodeError`
/// naming the script and the length, the same rule as the transaction detail.
#[tokio::test]
async fn test_activity_lock_call_states_why_its_args_do_not_decode() {
    let lock_hash = vec![0x25; 32];
    let mut actions = make_test_tx_actions(&lock_hash, &[0xab; 32], &[0xbc; 32], 88, 1, 100, 0);
    actions.lock_calls = vec![
        ckbadger_store::types::LockCallEntry {
            // JoyID (mainnet): a real cell carries 23-byte args.
            lock_code_hash: hex::decode(
                "d00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            )
            .unwrap(),
            lock_hash_type: 1,
            lock_args: vec![0; 23],
        },
        ckbadger_store::types::LockCallEntry {
            lock_code_hash: hex::decode(
                "d00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            )
            .unwrap(),
            lock_hash_type: 1,
            lock_args: [vec![0x00, 0x01], vec![0x5a; 20]].concat(),
        },
    ];
    let json = address_activities_json(&lock_hash, &actions).await;
    let calls = &json["data"][0]["lockCalls"];
    assert_eq!(
        calls[0].get("decoded"),
        Some(&serde_json::Value::Null),
        "{json}"
    );
    assert_eq!(
        calls[0]["decodeError"],
        "JoyID: args are 23 bytes, layout subType(2) + pubkeyHash(20) expects 22",
        "{json}"
    );
    assert_eq!(calls[1]["decoded"]["subType"], "0x0001", "{json}");
    assert_eq!(
        calls[1].get("decodeError"),
        Some(&serde_json::Value::Null),
        "{json}"
    );
}

#[tokio::test]
async fn test_address_activities_rejects_unknown_filter() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities?filter=tok",
            "11".repeat(32)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("invalid activity filter"));
}

#[tokio::test]
async fn test_address_activities_return_type_calls_and_support_type_call_filter() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x12; 32];
    let tx_hash = vec![0x34; 32];
    let block_hash = vec![0x56; 32];
    let type_code_hash = vec![0x78; 32];
    let type_args = vec![0x9A; 20];
    let expected_script_hash = format!(
        "0x{}",
        hex::encode(compute_script_hash(&type_code_hash, 1, &type_args))
    );

    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_TYPE_CALL};
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 88,
        tx_index: 0,
        timestamp: 1_700_000_888,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![TypeCallEntry {
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
        }],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::lock(&lock_hash).unwrap(),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL,
            roles: 0,
        }],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_actions(&actions);
    // AddrTxValue.tags must mirror the participant's tags so filtered scans
    // hit the entry (list_activities pre-filters on AddrTxValue.tags).
    core_batch.put_addr_tx(
        &lock_hash,
        88,
        0,
        &AddrTxValue::new(0, false, true, TAG_TYPE_CALL),
    );
    core_batch.put_tx_hash_map(&tx_hash, 88, 0);
    core_batch.put_tx_index(
        88,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_888_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 120,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_block_header(
        88,
        &CachedBlockHeader {
            hash: block_hash,
            timestamp: 1_700_000_888_000,
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
            chain_time_ms: 1_700_000_888_000,
        },
    );
    core_batch.put_script_info(
        &type_code_hash,
        &ScriptInfo {
            code_hash: type_code_hash.clone(),
            hash_type: 1,
            name: Some("RGB++ Lock".to_string()),
            ..Default::default()
        },
    );
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities?filter=type_call",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["itemDeltas"].as_array().unwrap().len(), 0);
    let type_calls = data[0]["typeCalls"].as_array().unwrap();
    assert_eq!(type_calls.len(), 1);
    assert_eq!(
        type_calls[0]["typeCodeHash"],
        format!("0x{}", hex::encode(&type_code_hash))
    );
    assert_eq!(type_calls[0]["typeHashType"], "type");
    assert_eq!(
        type_calls[0]["typeArgs"],
        format!("0x{}", hex::encode(&type_args))
    );
    assert_eq!(type_calls[0]["scriptHash"], expected_script_hash);
    assert_eq!(type_calls[0]["scriptName"], "RGB++ Lock");
    let lock_calls = data[0]["lockCalls"].as_array().unwrap();
    assert_eq!(lock_calls.len(), 0);
}

#[tokio::test]
async fn test_latest_activities_return_type_calls() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_DAO, TAG_TYPE_CALL};
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let tx_hash = vec![0x68; 32];
    let block_hash = vec![0x79; 32];
    let type_code_hash = vec![0x46; 32];
    let type_args = vec![0x57; 20];
    let expected_script_hash = format!(
        "0x{}",
        hex::encode(compute_script_hash(&type_code_hash, 1, &type_args))
    );

    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 99,
        tx_index: 1,
        timestamp: 1_700_000_999,
        is_cellbase: false,
        protocol_actions: vec![ProtocolAction::new(
            "dao",
            "deposit",
            serde_json::json!({"capacity": 102_00000000i64}),
        )],
        type_calls: vec![TypeCallEntry {
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
        }],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock([0x13; 32]),
            ckb_delta: -30000,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL | TAG_DAO,
            roles: 0,
        }],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_script_info(
        &type_code_hash,
        &ScriptInfo {
            code_hash: type_code_hash.clone(),
            hash_type: 1,
            name: Some("RGB++ Lock".to_string()),
            ..Default::default()
        },
    );
    core_batch.put_block_header(
        99,
        &CachedBlockHeader {
            hash: block_hash,
            timestamp: 1_700_000_999,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_999,
        },
    );
    core_batch.put_tx_hash_map(&tx_hash, 99, 1);
    core_batch.put_tx_index(
        99,
        1,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_999,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_tx_actions(&actions);
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/activities/latest?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let type_calls = items[0]["typeCalls"].as_array().unwrap();
    assert_eq!(type_calls.len(), 1);
    assert_eq!(type_calls[0]["typeHashType"], "type");
    assert_eq!(type_calls[0]["scriptHash"], expected_script_hash);
    assert_eq!(type_calls[0]["scriptName"], "RGB++ Lock");
    let lock_calls = items[0]["lockCalls"].as_array().unwrap();
    assert_eq!(lock_calls.len(), 0);
}

// Global activities cursor pagination test removed: owner-level pagination replaced with TX-level
// in the TxActions model. The /api/v1/activities endpoint now returns TX-level items.

#[tokio::test]
async fn test_global_activities_basic() {
    let store = test_store();
    let tx_hash = vec![0x91; 32];
    let block_hash = vec![0xA1; 32];

    let actions = make_test_tx_actions(&[0x11; 32], &tx_hash, &block_hash, 200, 0, 111, 0);

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: block_hash,
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
            chain_time_ms: 1_700_000_200,
        },
    );
    batch.put_tx_hash_map(&tx_hash, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
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
    batch.put_tx_actions(&actions);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;

    let request = Request::builder()
        .uri("/api/v1/activities?limit=10")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().expect("data array");
    assert!(!data.is_empty());
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

#[tokio::test]
async fn test_address_transactions_reads_from_derived_store() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x33; 32];
    let tx_hash = vec![0xab; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&tx_hash, 10, 0);
    core_batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 2,
            fee: 1000,
            tx_size: 120,
            cycles: Some(10_000),
            semantic_tags: 0,
        },
    );
    // A canonical transaction's block has a header; the list reads its time.
    core_batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xba; 32],
            timestamp: 1_700_000_000_000,
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
            chain_time_ms: 1_700_000_000_000,
        },
    );
    core_batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 0);

    let mut derived_batch = StoreBatch::new(append_only_store.as_ref());
    derived_batch.put_addr_tx(&lock_hash, 10, 0, &AddrTxValue::new(0, false, true, 0));
    derived_batch.commit().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

// ---------------------------------------------------------------------------
// R4-G item 3 / W5: `/addresses/{addr}/activities` once declared `total` from
// `AddressBalance.txs_count` — the address's TRANSACTION count. Activities are
// a different set: cellbase rows are deliberately never persisted in
// CF_TX_ACTIONS. Measured on mainnet (block 12000000's cellbase-output
// address): 4,727,769 transactions against 1,682 activities. W5 persists
// `AddressBalance.cellbase_txs_count`, so the exact activity count is
// `address_tx_count - cellbase_txs_count` and the unfiltered feed declares it.
// ---------------------------------------------------------------------------

/// Seed one canonical activity for `lock_hash` plus an `AddressBalance` whose
/// `txs_count` may far exceed it (the shape a miner address has: many
/// cellbase transactions, few activities) — `cellbase_txs_count` of them
/// cellbases.
fn seed_activity_with_txs_count(
    store: &Arc<CkbadgerStore>,
    lock_hash: &[u8],
    txs_count: i64,
    cellbase_txs_count: i64,
) -> Vec<u8> {
    let tx_hash = vec![0xa1; 32];
    let block_hash = vec![0xb1; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 10, 0);
    batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            timestamp: 1_700_000_000_000,
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
            chain_time_ms: 1_700_000_000_000,
        },
    );
    let actions = make_test_tx_actions(lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    batch.put_tx_actions(&actions);
    batch.put_addr_tx(lock_hash, 10, 0, &AddrTxValue::new(0, false, true, 0));
    batch.commit().unwrap();

    store
        .put_addr_balance_direct(
            lock_hash,
            &ckbadger_store::types::AddressBalance {
                balance: 100,
                used_capacity: 0,
                live_cells_count: 1,
                total_cells_count: 1,
                txs_count,
                cellbase_txs_count,
                first_seen_block: 10,
                first_seen_tx_idx: 0,
                last_activity_block: 10,
                last_activity_tx_idx: 0,
            },
        )
        .unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    tx_hash
}

#[tokio::test]
async fn test_address_activities_total_counts_activities_not_cellbase_transactions() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x71; 32];
    // 4_727_769 transactions, 4_727_768 of them cellbases: exactly 1 activity.
    seed_activity_with_txs_count(&core_store, &lock_hash, 4_727_769, 4_727_768);

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(&lock_hash)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["hasMore"], false);
    assert_eq!(
        json["total"], 1,
        "total is the activity count (transactions minus cellbases), which the page reaches, got {json}"
    );
}

#[tokio::test]
async fn test_address_activities_total_contradicting_page_one_is_an_error() {
    // An address with an activity row but no `AddressBalance` row (a state no
    // forward writer produces) once reported `total: 0` through
    // `.ok().flatten().map(...).unwrap_or(0)` — a silent default-zero next to
    // a non-empty page. The total is now read for every unfiltered page, and a
    // total below page one's committed rows is an error naming the address.
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x72; 32];
    let tx_hash = vec![0xa1; 32];
    let block_hash = vec![0xb1; 32];

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 10, 0);
    batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            timestamp: 1_700_000_000_000,
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
            chain_time_ms: 1_700_000_000_000,
        },
    );
    let actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    batch.put_tx_actions(&actions);
    batch.put_addr_tx(&lock_hash, 10, 0, &AddrTxValue::new(0, false, true, 0));
    batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(&lock_hash)),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains(&hex::encode(&lock_hash))
            && message.contains("activity total 0 disagrees with page one: 1 committed rows"),
        "the error names the address and both counts, got {json}"
    );
}

#[tokio::test]
async fn test_address_activities_filtered_page_still_omits_total() {
    // Control (passes on both revisions): the filtered branch already used
    // `without_total`. Both branches now agree.
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x73; 32];
    seed_activity_with_txs_count(&core_store, &lock_hash, 4_727_769, 4_727_768);

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/addresses/0x{}/activities?filter=ckb",
            hex::encode(&lock_hash)
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json.get("total"), None);
}

/// W5 (BA-35): the declared total is exactly what paging the feed to
/// exhaustion enumerates — three activities of a lock that also received two
/// cellbases (their `addr_txs` rows have no `TxActions` row, as the indexer
/// writes them), with the `AddressBalance` the forward writers produce.
#[tokio::test]
async fn test_address_activities_total_is_reached_by_paging_to_exhaustion() {
    let store = test_store();
    let lock_hash = [0x74u8; 32];
    for (block, tx_hash) in [(20, [0xc1u8; 32]), (22, [0xc2u8; 32]), (24, [0xc3u8; 32])] {
        seed_committed_activity(&store, &lock_hash, &tx_hash, block, 1, 100, 0);
    }
    let mut batch = StoreBatch::new(store.as_ref());
    for (block, tx_hash) in [(21, [0xd1u8; 32]), (23, [0xd2u8; 32])] {
        batch.put_tx_hash_map(&tx_hash, block, 0);
        batch.put_tx_index(
            block,
            0,
            &TxIndexEntry {
                tx_hash,
                is_cellbase: true,
                timestamp: 1_700_000_000_000 + block,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_addr_tx(&lock_hash, block, 0, &AddrTxValue::new(100, false, true, 0));
    }
    batch.put_addr_balance(
        &lock_hash,
        &ckbadger_store::types::AddressBalance {
            txs_count: 5,
            cellbase_txs_count: 2,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let base = format!("/addresses/0x{}/activities?limit=2", hex::encode(lock_hash));
    let (status, first) = get_json(&app, &base).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["total"], 3, "{first}");
    assert_eq!(first["hasMore"], true);
    let cursor = first["nextCursor"].as_str().unwrap().to_string();
    let (status, second) = get_json(&app, &format!("{base}&cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["total"], 3, "every page declares the same total");
    assert_eq!(second["hasMore"], false);
    let enumerated =
        first["data"].as_array().unwrap().len() + second["data"].as_array().unwrap().len();
    assert_eq!(
        enumerated, 3,
        "paging enumerates exactly the declared total"
    );
}

// ---------------------------------------------------------------------------
// Audited bug (2026-08-01 night, agent E): all-uppercase bech32m addresses are
// legal per the bech32 case rules, but every address entry point routed them
// into the hex-hash branch (the activities handler even had its own inline
// lowercase-only prefix check) and answered a misleading 400 about hex hashes.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_address_activities_accepts_uppercase_address() {
    let (core_store, append_only_store) = split_test_stores();

    // Mainnet burn lock (secp sighash, args = 20 zero bytes); its canonical
    // bech32m encoding is the audit vector below.
    let burn_address = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq5m759c";
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let lock_hash = compute_script_hash(&code_hash, 1, &[0u8; 20]);
    seed_activity_with_txs_count(&core_store, &lock_hash, 1, 0);

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;

    let (status_lower, lower) =
        get_json(&app, &format!("/addresses/{burn_address}/activities")).await;
    assert_eq!(status_lower, StatusCode::OK, "got {lower}");
    assert_eq!(lower["data"].as_array().unwrap().len(), 1);

    let (status_upper, upper) = get_json(
        &app,
        &format!("/addresses/{}/activities", burn_address.to_uppercase()),
    )
    .await;
    assert_eq!(
        status_upper,
        StatusCode::OK,
        "uppercase bech32m must route to the address branch, got {upper}"
    );
    assert_eq!(
        upper, lower,
        "uppercase input must enumerate the identical activities"
    );
}

// ---------------------------------------------------------------------------
// Tx-pool rows on page one
//
// A pool transaction can only land in a FUTURE block, so it is later than every
// committed transaction in canonical order. That makes pool rows a page-one-only
// segment: above the committed rows, never inside a cursor.
// ---------------------------------------------------------------------------

const POOL_LOCK_HASH: [u8; 32] = [0x41; 32];

#[tokio::test]
async fn test_address_activities_page_one_puts_pool_rows_above_committed_rows() {
    let store = test_store();
    seed_committed_activity(&store, &POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "pool row then committed row: {json:?}");

    let pool_row = &rows[0];
    assert_eq!(pool_row["txHash"], format!("0x{}", "f1".repeat(32)));
    assert_eq!(pool_row["blockNumber"], serde_json::Value::Null);
    assert_eq!(pool_row["txIndex"], serde_json::Value::Null);
    assert_eq!(pool_row["timestamp"], serde_json::Value::Null);
    assert_eq!(pool_row["poolStatus"], "pending");
    assert!(pool_row["timeAddedToPool"].as_str().is_some());
    assert_eq!(pool_row["interpretation"]["status"], "complete");
    assert_eq!(pool_row["ckbDelta"], "-500");

    let committed_row = &rows[1];
    assert_eq!(committed_row["blockNumber"], 10);
    assert_eq!(committed_row["poolStatus"], serde_json::Value::Null);

    let pool = &json["pool"];
    assert_eq!(pool["enabled"], true);
    assert_eq!(pool["healthy"], true);
    assert_eq!(pool["count"], 1);
    assert_eq!(pool["pendingCkbDelta"], "-500");
    assert_eq!(pool["truncated"], false);
}

#[tokio::test]
async fn test_address_activities_cursor_page_contains_no_pool_rows() {
    let store = test_store();
    seed_committed_activity(&store, &POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities?cursor=999:0",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert!(
        rows.iter().all(|row| row["poolStatus"].is_null()),
        "cursor pages must stay stable while the pool churns: {json:?}"
    );
    assert!(
        json["pool"].is_null(),
        "the pool summary belongs to page one only"
    );
}

#[tokio::test]
async fn test_address_activities_filter_applies_to_pool_rows() {
    use ckbadger_store::types::TAG_TOKEN;

    let store = test_store();
    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record_with(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
            TAG_TOKEN,
            ckbadger_api::pool::Interpretation::Complete,
        )]));
    let app = create_router_with_state(state).await;

    for (filter, expected) in [("token", 1usize), ("dao", 0), ("all", 1)] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/addresses/0x{}/activities?filter={filter}",
                        hex::encode(POOL_LOCK_HASH)
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
            json["data"].as_array().unwrap().len(),
            expected,
            "filter={filter} must apply to pool rows through the same tag bitmap"
        );
        assert_eq!(json["pool"]["count"], expected);
    }
}

#[tokio::test]
async fn test_address_activities_omit_pool_rows_the_store_already_has() {
    let store = test_store();
    let tx_hash = [0xc1; 32];
    seed_committed_activity(&store, &POOL_LOCK_HASH, &tx_hash, 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    // The mirror still holds the record while it waits for the local index;
    // this request's store view already has the transaction.
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &tx_hash,
            &POOL_LOCK_HASH,
            100,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::CommittedAwaitingIndex {
                block_number: 10,
                block_hash: [0xba; 32],
            },
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "committed wins, exactly once: {json:?}");
    assert_eq!(rows[0]["blockNumber"], 10);
    assert_eq!(json["pool"]["count"], 0);
}

#[tokio::test]
async fn test_address_activities_report_an_unhealthy_mirror_and_still_serve_committed_rows() {
    let store = test_store();
    seed_committed_activity(&store, &POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    state.pool_mirror.publish(unhealthy_pool_snapshot());
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["pool"]["enabled"], true);
    assert_eq!(
        json["pool"]["healthy"], false,
        "an unreachable node must never read as an empty pool"
    );
}

#[tokio::test]
async fn test_address_activities_report_a_disabled_mirror() {
    let store = test_store();
    let mut config = test_config(store);
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["pool"]["enabled"], false);
    assert_eq!(json["pool"]["healthy"], false);
    assert_eq!(json["pool"]["count"], 0);
}

// ── Phase 1a: protocol-named participants ────────────────────────────────

/// Seed one block header + tx index + `TxActions` at `(block, 0)`.
fn seed_named_tx(
    store: &std::sync::Arc<CkbadgerStore>,
    block_number: i64,
    tx_hash: &[u8],
    actions: &ckbadger_store::types::TxActions,
) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(tx_hash, block_number, 0);
    batch.put_tx_index(
        block_number,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        block_number,
        &CachedBlockHeader {
            hash: actions.block_hash.clone(),
            timestamp: 1_700_000_000_000,
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
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_actions(actions);
    batch.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = s.tip_block_number.max(block_number);
        })
        .unwrap();
}

#[tokio::test]
async fn test_address_activities_include_named_participation_with_zero_ckb_delta() {
    use ckbadger_store::types::{
        participant_roles, AddrTxValue, ItemDelta, ItemKind, ParticipantDelta, ParticipantId,
        TAG_IDENTITY,
    };

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x42u8; 32];
    let other = vec![0x11u8; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];
    let mut prefix = [0u8; 20];
    prefix.copy_from_slice(&lock_hash[..20]);

    let actions = ckbadger_store::types::TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&other).unwrap(),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![ItemDelta {
                    item_id: vec![0xEE; 20],
                    kind: ItemKind::Identity(IdentityStandard::DotCell),
                    magnitude: 1,
                    negative: false,
                }],
                tags: TAG_IDENTITY,
                roles: participant_roles::OWNER_TO,
            },
        ],
    };
    seed_named_tx(&core_store, 10, &tx_hash, &actions);
    // the .cell name's cell (`itemTypes` reads its type script there)
    let mut cells = StoreBatch::new(append_only_store.as_ref());
    cells
        .put_cell_payload_by_outpoint(&tx_hash, 0, &typed_cell([0xd3; 32], &[0x5c; 20]))
        .unwrap();
    cells.commit().unwrap();
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_spore_outpoint(&tx_hash, 0, &[0xEE; 20]);
    batch.put_addr_tx_by_prefix(
        &prefix,
        10,
        0,
        &AddrTxValue::new(0, false, false, TAG_IDENTITY),
    );
    // `other` holds a cell in this transaction, so the chain view knows its
    // lock script (CF_LOCK_SCRIPTS is written at every cell creation).
    batch.put_lock_script(
        &other,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0x11; 20],
        },
    );
    batch.commit().unwrap();
    sync_address_tx_counters(&core_store, &lock_hash);

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(&lock_hash)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{json:?}");
    assert_eq!(rows[0]["ckbDelta"], "0");
    assert_eq!(rows[0]["usedDelta"], "0");
    assert_eq!(rows[0]["itemDeltas"][0]["kind"], "identity");
    assert_eq!(rows[0]["itemDeltas"][0]["delta"], 1);
    assert_eq!(rows[0]["itemDeltas"][0]["standard"], "dotcell");
    assert_eq!(rows[0]["roles"][0], "owner_to");
    let participants = rows[0]["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 1, "only the other party: {json:?}");
    assert_eq!(
        participants[0]["lockHash"],
        format!("0x{}", hex::encode(&other))
    );
    assert!(participants[0]["address"].is_string());
    assert!(participants[0]["lockHashPrefix"].is_null());
}

#[tokio::test]
async fn test_address_activities_other_participants_carry_prefix_and_resolution() {
    use ckbadger_store::types::{
        participant_roles, AddrTxValue, LockScriptEntry, ParticipantDelta, ParticipantId,
    };

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x42u8; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];
    let named_prefix = [0x99u8; 20];

    let actions = ckbadger_store::types::TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&lock_hash).unwrap(),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(named_prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: participant_roles::OWNER_TO,
            },
        ],
    };
    seed_named_tx(&core_store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_addr_tx(&lock_hash, 10, 0, &AddrTxValue::new(-100, true, false, 0));
    batch.commit().unwrap();
    sync_address_tx_counters(&core_store, &lock_hash);

    let uri = format!("/api/v1/addresses/0x{}/activities", hex::encode(&lock_hash));

    // Unresolved: no lock script starts with this prefix.
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let participant = &json["data"][0]["participants"][0];
    assert!(participant["address"].is_null(), "{json:?}");
    assert!(participant["lockHash"].is_null());
    assert_eq!(
        participant["lockHashPrefix"],
        format!("0x{}", hex::encode(named_prefix))
    );
    assert_eq!(participant["roles"][0], "owner_to");

    // Resolved: one lock script carries that prefix.
    let mut resolved_hash = [0x99u8; 32];
    resolved_hash[31] = 0x01;
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_lock_script(
        &resolved_hash,
        &LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0x22; 20],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let participant = &json["data"][0]["participants"][0];
    assert_eq!(
        participant["lockHash"],
        format!("0x{}", hex::encode(resolved_hash))
    );
    assert!(participant["address"].as_str().unwrap().starts_with("ck"));

    // Ambiguous: a second lock script with the same prefix is an error, never a guess.
    let mut second_hash = [0x99u8; 32];
    second_hash[31] = 0x02;
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_lock_script(
        &second_hash,
        &LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0x33; 20],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        String::from_utf8_lossy(&body).contains("ambiguous"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn test_global_activities_expose_participant_ids_and_roles() {
    use ckbadger_store::types::{participant_roles, ParticipantDelta, ParticipantId};

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x42u8; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];
    let named_prefix = [0x99u8; 20];

    let actions = ckbadger_store::types::TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&lock_hash).unwrap(),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(named_prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: participant_roles::OWNER_TO | participant_roles::MANAGER_TO,
            },
        ],
    };
    seed_named_tx(&core_store, 10, &tx_hash, &actions);

    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/activities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let participants = json["data"][0]["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 2, "{json:?}");
    assert_eq!(
        participants[0]["lockHash"],
        format!("0x{}", hex::encode(&lock_hash))
    );
    assert!(participants[0]["lockHashPrefix"].is_null());
    assert_eq!(participants[0]["roles"].as_array().unwrap().len(), 0);
    assert!(participants[1]["lockHash"].is_null());
    assert_eq!(
        participants[1]["lockHashPrefix"],
        format!("0x{}", hex::encode(named_prefix))
    );
    assert_eq!(participants[1]["roles"][0], "owner_to");
    assert_eq!(participants[1]["roles"][1], "manager_to");
}

// ── Task 2.8: timestamps propagate, one pool segment, pendingSummary ─────

async fn fetch_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// The pool summary's `lastPolledAt` is the mirror's own clock. A value that
/// cannot be rendered is a mirror invariant violation, reported with the value
/// — not a `null` that reads as "never polled".
#[tokio::test]
async fn pool_summary_unrenderable_last_polled_at_is_a_500() {
    let store = test_store();
    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(ckbadger_api::pool::PoolSnapshot::from_records(
            vec![Arc::new(make_test_pool_record(
                &[0xf1; 32],
                &POOL_LOCK_HASH,
                -500,
                1_700_000_500_000,
                ckbadger_api::pool::PoolStatus::Pending,
            ))],
            ckbadger_api::pool::MirrorStatus {
                enabled: true,
                healthy: true,
                last_polled_at_ms: Some(i64::MAX),
                ..Default::default()
            },
        ));
    let app = create_router_with_state(state).await;

    for uri in [
        format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(POOL_LOCK_HASH)
        ),
        format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(POOL_LOCK_HASH)
        ),
    ] {
        let (status, json) = fetch_json(app.clone(), &uri).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}: {json}");
        let message = json["message"].as_str().unwrap();
        assert!(message.contains("last_polled_at"), "{uri}: {message}");
        assert!(message.contains(&i64::MAX.to_string()), "{uri}: {message}");
    }
}

/// A canonical committed row whose block header is missing is store
/// corruption: the transaction list reports it with the block, instead of
/// serving the row with an empty timestamp.
#[tokio::test]
async fn address_transactions_missing_block_header_is_a_500_naming_the_block() {
    let store = test_store();
    let lock_hash = [0x4au8; 32];
    let tx_hash = [0x5bu8; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 4242, 0);
    batch.put_tx_index(
        4242,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 100,
            tx_size: 300,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_addr_tx(&lock_hash, 4242, 0, &AddrTxValue::new(-100, true, false, 0));
    batch.commit().unwrap();
    let mut config = test_config(store);
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;

    let (status, json) = fetch_json(
        app,
        &format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(lock_hash)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("4242"), "{message}");
    assert!(message.contains("header"), "{message}");
}

/// The address detail's `pendingSummary` counts every pool row of the address
/// — the same segment the lists serve on page one, committed-wins included —
/// and no activity filter reaches it, so the header cannot disagree with
/// itself between tabs.
#[tokio::test]
async fn address_pending_summary_is_independent_of_the_activity_filter() {
    use ckbadger_store::types::TAG_TOKEN;

    let store = test_store();
    let committed_tx = [0xc1; 32];
    seed_committed_activity(&store, &POOL_LOCK_HASH, &committed_tx, 10, 0, 100, 0);
    let state = test_app_state(test_config(store));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![
        make_test_pool_record_with(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
            TAG_TOKEN,
            ckbadger_api::pool::Interpretation::Complete,
        ),
        make_test_pool_record(
            &[0xf2; 32],
            &POOL_LOCK_HASH,
            300,
            1_700_000_600_000,
            ckbadger_api::pool::PoolStatus::Proposed,
        ),
        // Already indexed: served by the committed segment, counted nowhere here.
        make_test_pool_record(
            &committed_tx,
            &POOL_LOCK_HASH,
            100,
            1_700_000_400_000,
            ckbadger_api::pool::PoolStatus::CommittedAwaitingIndex {
                block_number: 10,
                block_hash: [0xba; 32],
            },
        ),
    ]));
    let app = create_router_with_state(state).await;
    let addr = format!("/api/v1/addresses/0x{}", hex::encode(POOL_LOCK_HASH));

    let (status, detail) = fetch_json(app.clone(), &addr).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(
        detail["pendingSummary"],
        serde_json::json!({ "txCount": 2, "capacityDelta": "-200" })
    );

    // A filter on a list narrows the list's own `pool` summary …
    let (_, filtered) = fetch_json(app.clone(), &format!("{addr}/activities?filter=token")).await;
    assert_eq!(filtered["pool"]["count"], 1);
    assert_eq!(filtered["pool"]["pendingCkbDelta"], "-500");
    // … the transaction list agrees with the unfiltered segment …
    let (_, txs) = fetch_json(app.clone(), &format!("{addr}/transactions")).await;
    assert_eq!(txs["pool"]["count"], 2);
    assert_eq!(txs["pool"]["pendingCkbDelta"], "-200");
    // … and the detail's summary is the same whatever the page asks for.
    let (_, again) = fetch_json(app, &format!("{addr}?filter=token")).await;
    assert_eq!(again["pendingSummary"], detail["pendingSummary"]);
}

/// No pool view, no pending summary: the key is present and `null`, never a
/// `0` that reads as "nothing pending".
#[tokio::test]
async fn address_pending_summary_is_null_without_a_healthy_mirror() {
    let addr = format!("/api/v1/addresses/0x{}", hex::encode(POOL_LOCK_HASH));

    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(unhealthy_pool_snapshot());
    let app = create_router_with_state(state).await;
    let (status, json) = fetch_json(app, &addr).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(
        json.as_object().unwrap().contains_key("pendingSummary"),
        "{json}"
    );
    assert!(json["pendingSummary"].is_null(), "{json}");

    let mut config = test_config(test_store());
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;
    let (status, json) = fetch_json(app, &addr).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(
        json.as_object().unwrap().contains_key("pendingSummary"),
        "{json}"
    );
    assert!(json["pendingSummary"].is_null(), "{json}");

    // A healthy mirror with nothing for this address is a real zero.
    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![]));
    let app = create_router_with_state(state).await;
    let (_, json) = fetch_json(app, &addr).await;
    assert_eq!(
        json["pendingSummary"],
        serde_json::json!({ "txCount": 0, "capacityDelta": "0" })
    );
}

/// The detail response is cached; the pending summary is not — a pool
/// transaction that arrives after the first request shows on the next one.
#[tokio::test]
async fn address_pending_summary_is_not_served_from_the_detail_cache() {
    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![]));
    let app = create_router_with_state(state.clone()).await;
    let addr = format!("/api/v1/addresses/0x{}", hex::encode(POOL_LOCK_HASH));

    let (_, first) = fetch_json(app.clone(), &addr).await;
    assert_eq!(first["pendingSummary"]["txCount"], 0);

    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            700,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let (_, second) = fetch_json(app, &addr).await;
    assert_eq!(
        second["pendingSummary"],
        serde_json::json!({ "txCount": 1, "capacityDelta": "700" })
    );
}

// ── 5.9: one lock → address resolver ─────────────────────────────────────

/// Seed one committed tx at block 10 in which `lock_hash` pays `other`.
fn seed_two_party_tx(store: &Arc<CkbadgerStore>, lock_hash: &[u8; 32], other: &[u8; 32]) {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};
    let tx_hash = vec![0xaa; 32];
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbb; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::Lock(*lock_hash),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::Lock(*other),
                ckb_delta: 100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
        ],
    };
    seed_named_tx(store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(lock_hash, 10, 0, &AddrTxValue::new(-100, true, false, 0));
    batch.commit().unwrap();
    sync_address_tx_counters(store, lock_hash);
}

/// A party whose lock script the store does not know is reported by its lock
/// hash with no address — never with the hex lock hash posing as an address.
#[tokio::test]
async fn unknown_lock_participant_is_unresolved_not_a_fabricated_address() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let other = [0x11u8; 32];
    seed_two_party_tx(&store, &lock_hash, &other);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let party = &json["data"][0]["participants"][0];
    assert_eq!(party["lockHash"], format!("0x{}", hex::encode(other)));
    assert!(party["address"].is_null(), "{json}");
}

/// A lock script the store holds but cannot encode (a hash_type no chain
/// allows) is store corruption, reported with the lock — not rendered as a
/// guess or dropped.
#[tokio::test]
async fn corrupt_lock_script_entry_is_a_500_naming_the_lock() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let other = [0x11u8; 32];
    seed_two_party_tx(&store, &lock_hash, &other);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &other,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: vec![0x9b; 32],
            hash_type: 7,
            args: vec![0x11; 20],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains(&hex::encode(other)),
        "{json}"
    );
}

/// A pending transaction paying a lock no committed cell has ever used: the
/// recipient's address comes from the script the pool transaction itself
/// carries, the same encoder the chain view uses, not from a store lookup
/// that cannot know the lock yet.
#[tokio::test]
async fn pending_tx_paying_a_never_seen_lock_shows_its_real_address() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};

    let secp =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = vec![0x5e; 20];
    let cell = ckbadger_api::pool::ResolvedCell::new(
        100_000_000_000,
        secp.clone(),
        1,
        args.clone(),
        None,
        vec![],
    )
    .unwrap();
    let recipient: [u8; 32] = cell.lock_script_hash.clone().try_into().unwrap();
    let expected =
        ckbadger_api::utils::address::script_to_address(&secp, 1, &args, "mainnet").unwrap();

    let mut record = make_test_pool_record(
        &[0xf7; 32],
        &POOL_LOCK_HASH,
        -100_000_000_000,
        1_700_000_500_000,
        ckbadger_api::pool::PoolStatus::Pending,
    );
    record.outputs = vec![cell];
    record
        .actions
        .as_mut()
        .unwrap()
        .participants
        .push(ParticipantDelta {
            id: ParticipantId::Lock(recipient),
            ckb_delta: 100_000_000_000,
            used_delta: 0,
            item_deltas: vec![],
            tags: 0,
            roles: 0,
        });

    let mut config = test_config(test_store());
    config.ckb_network = "mainnet".to_string();
    let state = test_app_state(config);
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(POOL_LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    assert_eq!(row["poolStatus"], "pending", "{json}");
    let party = &row["participants"][0];
    assert_eq!(party["lockHash"], format!("0x{}", hex::encode(recipient)));
    assert_eq!(party["address"], expected, "{json}");
}

// ── 2.6b: identity item deltas name their standard ──────────────────────

/// Every identity item delta carries its standard, as the builder recorded
/// it, in the standard's one wire value (`IdentityStandard::asset_standard`):
/// the frontend links the item to its page with it. Re-pinned on purpose (the
/// P3 follow-up, `itemTypes`): the four identities have their cells in the
/// item outpoint index now, where an activity's item types are read; they
/// still have no identity row, which the standard needs none of.
#[tokio::test]
async fn identity_item_deltas_name_their_standard() {
    use ckbadger_store::types::{
        ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TAG_IDENTITY,
    };

    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let tx_hash = vec![0xab; 32];
    let identity = |standard, byte| ItemDelta {
        item_id: vec![byte; 20],
        kind: ItemKind::Identity(standard),
        magnitude: 1,
        negative: false,
    };
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbb; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock(lock_hash),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![
                identity(IdentityStandard::DotBit, 0x01),
                identity(IdentityStandard::BitCell, 0x02),
                identity(IdentityStandard::DidCkb, 0x03),
                identity(IdentityStandard::DotCell, 0x04),
            ],
            tags: TAG_IDENTITY,
            roles: 0,
        }],
    };
    seed_named_tx(&store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    let created: [u8; 32] = tx_hash.as_slice().try_into().unwrap();
    batch.put_dotbit_outpoint_by_account_id(
        &[0x01; 20],
        &created,
        0,
        ckbadger_store::types::AssetAction::Mint,
    );
    for (index, byte) in [(1i16, 0x02u8), (2, 0x03), (3, 0x04)] {
        batch.put_spore_outpoint(&created, index, &[byte; 20]);
    }
    for (index, code) in [(0i16, 0xd0u8), (1, 0xd1), (2, 0xd2), (3, 0xd3)] {
        batch
            .put_cell_payload_by_outpoint(&created, index, &typed_cell([code; 32], &[]))
            .unwrap();
    }
    batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &AddrTxValue::new(0, true, true, TAG_IDENTITY),
    );
    batch.commit().unwrap();
    sync_address_tx_counters(&store, &lock_hash);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let deltas = json["data"][0]["itemDeltas"].as_array().unwrap();
    let standards: Vec<&str> = deltas
        .iter()
        .map(|d| d["standard"].as_str().unwrap())
        .collect();
    assert_eq!(
        standards,
        ["dotbit", "bit_cell", "did_ckb", "dotcell"],
        "{json}"
    );

    let (_, global) = get_json(&app, "/activities").await;
    assert_eq!(
        global["data"][0]["participants"][0]["itemDeltas"][3]["standard"], "dotcell",
        "{global}"
    );
    // the third read's T SHOULD-2: each identity's code is its recorded cell's, the .bit account
    // read through its own index (0xd0), the .bit Cell, did:ckb and `.cell` through the shared
    // id space's (0xd1, 0xd2, 0xd3), on the address row and the global row alike
    let codes = |row: &serde_json::Value| -> Vec<String> {
        row["itemTypes"]
            .as_array()
            .unwrap_or_else(|| panic!("the row states its item types: {row}"))
            .iter()
            .map(|t| t["typeCodeHash"].as_str().unwrap().to_string())
            .collect()
    };
    let expected: Vec<String> = [0xd0u8, 0xd1, 0xd2, 0xd3]
        .iter()
        .map(|c| format!("0x{}", hex::encode([*c; 32])))
        .collect();
    assert_eq!(codes(&json["data"][0]), expected, "{json}");
    assert_eq!(codes(&global["data"][0]), expected, "{global}");
}

/// A pending `.cell` registration: the name is not in the store yet, and the
/// pool row still names its standard — carried by the item delta the mirror's
/// builder produced, not looked up.
#[tokio::test]
async fn pending_dotcell_registration_item_delta_carries_its_standard() {
    use ckbadger_store::types::{ItemDelta, ItemKind, TAG_IDENTITY};

    let mut record = make_test_pool_record_with(
        &[0xf9; 32],
        &POOL_LOCK_HASH,
        -24_000_000_000,
        1_700_000_500_000,
        ckbadger_api::pool::PoolStatus::Pending,
        TAG_IDENTITY,
        ckbadger_api::pool::Interpretation::Complete,
    );
    let new_name = ckbadger_store::types::derive_dotcell_id("brandnew");
    record.actions.as_mut().unwrap().participants[0]
        .item_deltas
        .push(ItemDelta {
            item_id: new_name.to_vec(),
            kind: ItemKind::Identity(IdentityStandard::DotCell),
            magnitude: 1,
            negative: false,
        });

    let state = test_app_state(test_config(test_store()));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(POOL_LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    assert_eq!(row["poolStatus"], "pending", "{json}");
    assert_eq!(row["itemDeltas"][0]["kind"], "identity");
    assert_eq!(
        row["itemDeltas"][0]["identityId"],
        format!("0x{}", hex::encode(new_name))
    );
    assert_eq!(row["itemDeltas"][0]["standard"], "dotcell", "{json}");
}

// ── R12: object item deltas carry collection / class / composition ───────

/// Seed a "Nervape" Spore cluster, one of its spores (BTC+CKB media) and an
/// m-NFT token of class `0x3c…`; returns `(cluster, spore, mnft_token)`.
fn seed_named_cluster_spore_and_mnft(store: &Arc<CkbadgerStore>) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    use ckbadger_store::types::{
        CompositionTier, ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile,
    };
    let cluster = vec![0x86; 32];
    let spore = vec![0x51; 32];
    let token = [vec![0x3c; 24], vec![0, 0, 0, 7]].concat();
    let entry = |standard, collection_id: Option<Vec<u8>>, name: Option<&str>, extra| ObjectEntry {
        standard,
        collection_id,
        token_id: None,
        owner_lock_hash: None,
        name: name.map(str::to_string),
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: vec![0; 32],
        extra,
    };
    let mut batch = StoreBatch::new(store.as_ref());
    // their creating transaction (`created_at_tx` 0x00…), each one's cell with its id as args
    seed_creating_tx(
        &mut batch,
        2,
        9,
        [0; 32],
        [0x4a; 32],
        &[&cluster, &spore, &token],
    );
    batch.put_spore(
        &cluster,
        &entry(
            ObjectStandard::SporeCluster,
            None,
            Some("Nervape"),
            ObjectExtra::SporeCluster,
        ),
    );
    batch.put_spore(
        &spore,
        &entry(
            ObjectStandard::Spore,
            Some(cluster.clone()),
            None,
            ObjectExtra::Spore {
                content_type: "application/vnd.cell.dob@v1".into(),
                content_length: 10,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::BtcCkb,
                    ..Default::default()
                },
            },
        ),
    );
    batch.put_mnft(
        &token,
        &entry(
            ObjectStandard::MnftToken,
            Some(token[..24].to_vec()),
            None,
            ObjectExtra::MnftToken {
                token_index: 7,
                characteristic: vec![0; 8],
                configure: 0,
                state: 0,
            },
        ),
    );
    batch.commit().unwrap();
    (cluster, spore, token)
}

fn object_delta(id: &[u8], negative: bool) -> ckbadger_store::types::ItemDelta {
    ckbadger_store::types::ItemDelta {
        item_id: id.to_vec(),
        kind: ckbadger_store::types::ItemKind::Object,
        magnitude: 1,
        negative,
    }
}

/// R12 (TX-12, TX-29): every object item delta names its collection, asset
/// class and composition tier — on the address feed, the global feed and a
/// transaction's activity alike (one seam, `convert_item_delta`). A spore of a
/// named cluster carries the cluster's id and immutable name and its media
/// tier; an m-NFT token carries only its class id (the class row's name and
/// tier are mutable, not evidence about this item).
#[tokio::test]
async fn object_item_deltas_name_collection_class_and_composition() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_OBJECT};

    let store = test_store();
    let (cluster, spore, token) = seed_named_cluster_spore_and_mnft(&store);
    let lock_hash = [0x42u8; 32];
    let tx_hash = vec![0xac; 32];
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbb; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock(lock_hash),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![object_delta(&spore, false), object_delta(&token, true)],
            tags: TAG_OBJECT,
            roles: 0,
        }],
    };
    seed_named_tx(&store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &AddrTxValue::new(0, true, true, TAG_OBJECT),
    );
    batch.commit().unwrap();
    sync_address_tx_counters(&store, &lock_hash);
    let app = create_router(test_config(store)).await;

    let expected = serde_json::json!([
        {
            "kind": "object",
            "objectId": format!("0x{}", hex::encode(&spore)),
            "delta": 1,
            "collection": {"id": format!("0x{}", hex::encode(&cluster)), "name": "Nervape"},
            "class": "object",
            "composition": "btc_ckb",
            "role": "spore",
        },
        {
            "kind": "object",
            "objectId": format!("0x{}", hex::encode(&token)),
            "delta": -1,
            "collection": {"id": format!("0x{}", "3c".repeat(24)), "name": null},
            "class": "object",
            "composition": null,
            "role": "mnft_token",
        },
    ]);

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["itemDeltas"], expected, "address feed");

    let (status, global) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{global}");
    assert_eq!(
        global["data"][0]["participants"][0]["itemDeltas"], expected,
        "global feed"
    );
}

/// The activity writer emits object deltas for every cell of the Spore and
/// Cluster type scripts and the m-NFT token type script
/// (`emit_object_item_deltas` over `spore_*` / `nft_*`), so a committed
/// cluster-creation transaction carries a `+1` delta naming the CLUSTER. That
/// is a legitimate chain state: the delta describes the cluster by its own
/// facts (its own id and immutable name, no composition), on the `object`
/// filter of both global listings, the unfiltered feed, the address feed and
/// the transaction's own activity — mainnet tx at block 20,584,2xx answered
/// 500 "names a SporeCluster row, not a spore" on all of them.
#[tokio::test]
async fn cluster_creation_object_delta_names_the_cluster_itself() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_OBJECT};

    let store = test_store();
    let (cluster, _, _) = seed_named_cluster_spore_and_mnft(&store);
    let lock_hash = [0x44u8; 32];
    let tx_hash = vec![0xae; 32];
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbd; 32],
        block_number: 12,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock(lock_hash),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![object_delta(&cluster, false)],
            tags: TAG_OBJECT,
            roles: 0,
        }],
    };
    seed_named_tx(&store, 12, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(
        &lock_hash,
        12,
        0,
        &AddrTxValue::new(0, true, true, TAG_OBJECT),
    );
    // The transaction list sums each listed tx's output payloads.
    batch.put_cell(
        &tx_hash,
        0,
        &ckbadger_store::types::LiveCellInfo {
            capacity: 300_00000000,
            lock_code_hash: [0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            udt_amount: None,
            data_hash: None,
        },
        12,
    );
    batch.commit().unwrap();
    // The address counters the forward writers keep beside its `addr_txs`
    // rows (W5): the unfiltered address feed checks its `total` against them.
    sync_address_tx_counters(&store, &lock_hash);
    let app = create_router(test_config(store)).await;

    let expected = serde_json::json!([{
        "kind": "object",
        "objectId": format!("0x{}", hex::encode(&cluster)),
        "delta": 1,
        "collection": {"id": format!("0x{}", hex::encode(&cluster)), "name": "Nervape"},
        "class": "object",
        "composition": null,
        "role": "cluster",
    }]);

    for uri in ["/activities?filter=object&limit=20", "/activities?limit=20"] {
        let (status, json) = get_json(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {json}");
        assert_eq!(
            json["data"][0]["participants"][0]["itemDeltas"], expected,
            "{uri}"
        );
    }
    let (status, json) = get_json(&app, "/transactions?activity_type=object&limit=20").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["data"][0]["activity"]["participants"][0]["itemDeltas"], expected,
        "transactions?activity_type=object"
    );
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["itemDeltas"], expected, "address feed");
    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/activity", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["participants"][0]["itemDeltas"], expected,
        "transaction activity: {json}"
    );
}

/// A committed object delta naming no indexed spore or m-NFT token is store
/// corruption (the indexer writes the object row in the delta's batch): a 500
/// naming the object, never a delta without its collection.
#[tokio::test]
async fn committed_object_delta_naming_no_indexed_object_is_a_500() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_OBJECT};

    let store = test_store();
    let lock_hash = [0x43u8; 32];
    let tx_hash = vec![0xad; 32];
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbc; 32],
        block_number: 11,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock(lock_hash),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![object_delta(&[0x99; 32], false)],
            tags: TAG_OBJECT,
            roles: 0,
        }],
    };
    seed_named_tx(&store, 11, &tx_hash, &actions);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains(&format!("0x{}", "99".repeat(32))),
        "{json}"
    );
}

/// A pending spore mint: the pool row's object delta names a spore no store
/// row has yet (its creating transaction is the pending one). Its collection
/// and composition are not known until the indexer writes the spore row, so
/// they are `null` — while a pending transfer of an indexed spore names them.
#[tokio::test]
async fn pending_object_delta_names_what_the_store_knows() {
    use ckbadger_store::types::TAG_OBJECT;

    let store = test_store();
    let (cluster, spore, _) = seed_named_cluster_spore_and_mnft(&store);
    let pending_spore = vec![0x77; 32];
    let mut record = make_test_pool_record_with(
        &[0xf7; 32],
        &POOL_LOCK_HASH,
        -24_000_000_000,
        1_700_000_500_000,
        ckbadger_api::pool::PoolStatus::Pending,
        TAG_OBJECT,
        ckbadger_api::pool::Interpretation::Complete,
    );
    record.actions.as_mut().unwrap().participants[0].item_deltas = vec![
        object_delta(&pending_spore, false),
        object_delta(&spore, true),
    ];

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(POOL_LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    assert_eq!(row["poolStatus"], "pending", "{json}");
    assert_eq!(
        row["itemDeltas"],
        serde_json::json!([
            {
                "kind": "object",
                "objectId": format!("0x{}", hex::encode(&pending_spore)),
                "delta": 1,
                "collection": null,
                "class": "object",
                "composition": null,
                "role": null,
            },
            {
                "kind": "object",
                "objectId": format!("0x{}", hex::encode(&spore)),
                "delta": -1,
                "collection": {"id": format!("0x{}", hex::encode(&cluster)), "name": "Nervape"},
                "class": "object",
                "composition": "btc_ckb",
                "role": "spore",
            },
        ]),
        "{json}"
    );
}

// ── integration: a spent never-seen lock is encoded from the record ──────

/// A pending transaction may spend a cell whose lock exists so far only as the
/// output of another pending transaction, so no store row knows it. The mirror
/// keeps that lock script on the record (`input_locks`); the address page must
/// encode the sender from it exactly as it does for the record's own outputs —
/// never `null`, never a store-derived guess.
#[tokio::test]
async fn pending_tx_spending_a_never_seen_lock_shows_its_real_address() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};

    let secp =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = vec![0x6d; 20];
    let sender: [u8; 32] = ckbadger_api::utils::address::compute_script_hash(&secp, 1, &args)
        .try_into()
        .unwrap();
    let expected =
        ckbadger_api::utils::address::script_to_address(&secp, 1, &args, "mainnet").unwrap();

    let mut record = make_test_pool_record(
        &[0xf8; 32],
        &POOL_LOCK_HASH,
        100_000_000_000,
        1_700_000_600_000,
        ckbadger_api::pool::PoolStatus::Pending,
    );
    record.input_locks = vec![ckbadger_api::pool::PoolLockScript {
        lock_hash: sender,
        code_hash: secp.clone(),
        hash_type: 1,
        args: args.clone(),
    }];
    record
        .actions
        .as_mut()
        .unwrap()
        .participants
        .push(ParticipantDelta {
            id: ParticipantId::Lock(sender),
            ckb_delta: -100_000_000_000,
            used_delta: 0,
            item_deltas: vec![],
            tags: 0,
            roles: 0,
        });

    let mut config = test_config(test_store());
    config.ckb_network = "mainnet".to_string();
    let state = test_app_state(config);
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(POOL_LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    assert_eq!(row["poolStatus"], "pending", "{json}");
    let sender_hex = format!("0x{}", hex::encode(sender));
    let party = row["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["lockHash"] == sender_hex)
        .unwrap_or_else(|| panic!("the sender is a party of the row: {json}"));
    assert_eq!(party["address"], expected, "{json}");
}

#[tokio::test]
async fn redesign_block_feed_and_direct_activity_share_exact_participants() {
    let store = test_store();
    let lock = [0x12; 32];
    seed_committed_activity(&store, &lock, &[0xa1; 32], 42, 1, 800, 0);
    seed_committed_activity(&store, &lock, &[0xa2; 32], 42, 2, -900, 0);
    seed_committed_activity(&store, &lock, &[0xa3; 32], 43, 1, 100, 0);
    let code_hash = vec![0x65; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &lock,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: code_hash.clone(),
            hash_type: 1,
            args: vec![0x11; 20],
        },
    );
    batch.put_script_info(
        &code_hash,
        &ScriptInfo {
            code_hash: code_hash.clone(),
            hash_type: 1,
            name: Some("JoyID".into()),
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, first) = get_json(&app, "/activities?blockNumber=42&limit=1").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["data"][0]["txIndex"], 2);
    assert_eq!(first["data"][0]["participants"][0]["lockKind"], "JoyID");
    let (status, direct) = get_json(
        &app,
        &format!("/transactions/0x{}/activity", "a2".repeat(32)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{direct}");
    assert_eq!(direct, first["data"][0]);
    let (status, second) = get_json(
        &app,
        &format!(
            "/activities?blockNumber=42&limit=1&cursor={}",
            first["nextCursor"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["data"][0]["txIndex"], 1);
    assert!(second["nextCursor"].is_null());
    let (status, _) = get_json(&app, "/activities?blockNumber=42&cursor=43:1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn block_and_direct_cellbase_activity_derive_the_exact_paid_outputs() {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let lock = packed::Script::new_builder()
        .code_hash(packed::Byte32::new([0x11; 32]))
        .hash_type(1u8.into())
        .args(Bytes::from(vec![0x22; 20]).pack())
        .build();
    let tx = TransactionBuilder::default()
        .input(packed::CellInput::new_cellbase_input(100))
        .witness(
            packed::CellbaseWitness::new_builder()
                .lock(lock.clone())
                .build()
                .as_bytes()
                .pack(),
        )
        .output(
            packed::CellOutput::new_builder()
                .capacity(12_300_000_000u64.pack())
                .lock(lock.clone())
                .build(),
        )
        .output_data(Bytes::new().pack())
        .build();
    assert!(tx.is_cellbase());
    let hash: [u8; 32] = tx.hash().unpack();
    let block = BlockBuilder::default()
        .number(100u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1000).pack())
        .transaction(tx)
        .build();
    let block_hash: [u8; 32] = block.hash().unpack();
    let chain = seed_ckb_chain(&[block]);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: block_hash.to_vec(),
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
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_hash_map(&hash, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            tx_hash: hash,
            is_cellbase: true,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_lock_script(
        lock.calc_script_hash().as_slice(),
        &ckbadger_store::types::LockScriptEntry {
            code_hash: vec![0x11; 32],
            hash_type: 1,
            args: vec![0x22; 20],
        },
    );
    batch.commit().unwrap();
    assert!(store.get_tx_actions(100, 0, &hash).unwrap().is_none());
    let unavailable = create_router(test_config(store.clone())).await;
    let (status, missing) = get_json(&unavailable, "/activities?blockNumber=100").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{missing}");
    assert!(missing["message"].as_str().unwrap().contains("CKB RocksDB"));
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, body) = get_json(&app, "/activities?blockNumber=100").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"].as_array().unwrap().len(),
        1,
        "cellbase reward is part of block activity"
    );
    let row = &body["data"][0];
    assert_eq!(row["isCellbase"], true);
    assert_eq!(row["participants"][0]["ckbDelta"], "12300000000");
    assert_eq!(row["participants"][0]["usedDelta"], "6100000000");
    let (status, direct) = get_json(
        &app,
        &format!("/transactions/0x{}/activity", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{direct}");
    assert_eq!(direct, *row);
    let (_, global) = get_json(&app, "/activities").await;
    assert!(
        global["data"].as_array().unwrap().is_empty(),
        "global activity semantics still exclude cellbase"
    );
}

// ── R0: global bucket filters read CF_TX_ACTIONS_BY_BUCKET ──────────────

/// `count` one-transaction blocks from `first_block` up, each tx at index 1.
/// Every block whose offset is a multiple of `object_every` carries an object
/// item tag; the rest are plain CKB transfers. Returns the object blocks.
fn seed_sparse_object_chain(
    store: &Arc<CkbadgerStore>,
    first_block: i64,
    count: i64,
    object_every: i64,
) -> Vec<i64> {
    use ckbadger_store::types::TAG_OBJECT;
    let mut object_blocks = Vec::new();
    let mut batch = StoreBatch::new(store.as_ref());
    for offset in 0..count {
        let block = first_block + offset;
        let mut tx_hash = [0u8; 32];
        tx_hash[..8].copy_from_slice(&block.to_be_bytes());
        tx_hash[31] = 0x0b;
        let block_hash = vec![(block % 251) as u8; 32];
        let is_object = offset % object_every == 0;
        if is_object {
            object_blocks.push(block);
            // The transaction list sums each listed tx's output payloads.
            batch.put_cell(
                &tx_hash,
                0,
                &ckbadger_store::types::LiveCellInfo {
                    capacity: 100_00000000,
                    lock_code_hash: [0x22; 32],
                    lock_hash_type: 1,
                    lock_args: vec![0x33; 20],
                    type_code_hash: None,
                    type_hash_type: None,
                    type_args: None,
                    data_size: 0,
                    udt_amount: None,
                    data_hash: None,
                },
                block,
            );
        }
        let tags = if is_object { TAG_OBJECT } else { 0 };
        batch.put_block_header(
            block,
            &CachedBlockHeader {
                hash: block_hash.clone(),
                timestamp: 1_700_000_000_000 + block,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 2,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: 1_700_000_000_000 + block,
            },
        );
        batch.put_tx_hash_map(&tx_hash, block, 1);
        batch.put_tx_index(
            block,
            1,
            &TxIndexEntry {
                tx_hash,
                is_cellbase: false,
                timestamp: 1_700_000_000_000 + block,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_tx_actions(&make_test_tx_actions(
            &[0x11; 32],
            &tx_hash,
            &block_hash,
            block,
            1,
            7,
            tags,
        ));
    }
    batch.commit().unwrap();
    store
        .update_sync_status(|status| {
            status.tip_block_number = first_block + count - 1;
        })
        .unwrap();
    object_blocks.reverse();
    object_blocks
}

/// Pages a cursor-paginated JSON listing until it runs out; returns every
/// row's `blockNumber`, and how many pages it took.
async fn page_block_numbers(app: &axum::Router, first_uri: &str) -> (Vec<i64>, usize) {
    let mut blocks = Vec::new();
    let mut uri = first_uri.to_string();
    let mut pages = 0;
    loop {
        let (status, json) = fetch_json(app.clone(), &uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {json}");
        pages += 1;
        for row in json["data"].as_array().expect("data array") {
            let block = row["blockNumber"]
                .as_i64()
                .or_else(|| row["blockNumber"].as_str().and_then(|s| s.parse().ok()))
                .expect("blockNumber");
            blocks.push(block);
        }
        match json["nextCursor"].as_str() {
            Some(cursor) => uri = format!("{first_uri}&cursor={cursor}"),
            None => break,
        }
        assert!(pages < 100, "runaway paging on {first_uri}");
    }
    (blocks, pages)
}

/// TX-24 / TX-28 / DAO-19: a sparse bucket (3 object txs among 3,000) pages
/// with no duplicates and no gaps on both listings, and a page costs reads in
/// proportion to its size — not to the transactions between bucket rows.
#[tokio::test]
async fn global_bucket_filter_pages_a_sparse_bucket_with_bounded_reads() {
    let store = test_store();
    let expected = seed_sparse_object_chain(&store, 1_000, 3_000, 1_000);
    assert_eq!(expected, vec![3_000, 2_000, 1_000]);
    let app = create_router(test_config(store.clone())).await;

    for first_uri in [
        "/api/v1/activities?filter=object&limit=2",
        "/api/v1/transactions?activity_type=object&limit=2",
    ] {
        store.reset_read_call_counters();
        let started = std::time::Instant::now();
        let (blocks, pages) = page_block_numbers(&app, first_uri).await;
        let elapsed = started.elapsed();
        let (get_cf, multi_get) = store.read_call_counts();
        eprintln!(
            "{first_uri}: {pages} pages, {} rows, get_cf={get_cf}, multi_get={multi_get}, elapsed={elapsed:?}",
            blocks.len()
        );
        assert_eq!(blocks, expected, "{first_uri}: no duplicates, no gaps");
        assert_eq!(pages, 2, "{first_uri}: limit 2 over 3 rows");
        // 3 rows served over 2 pages. The pre-index scan issued two point reads
        // (tx location + tx index) for every one of the ~3,000 rows it walked.
        assert!(
            get_cf + multi_get <= 40,
            "{first_uri}: {get_cf} get_cf + {multi_get} multi_get for 3 rows"
        );
    }
}

/// The bucket path loads only the `TxActions` rows its bucket names: the
/// rows of every other bucket are unreadable here and the page still serves.
#[tokio::test]
async fn global_bucket_filter_never_reads_other_buckets_tx_actions() {
    let store = test_store();
    let expected = seed_sparse_object_chain(&store, 50, 600, 200);
    for block in 50..650 {
        if expected.contains(&block) {
            continue;
        }
        store
            .put_cf(
                store.cf_tx_actions(),
                &ckbadger_store::keys::encode_tx_actions_key(block, 1),
                b"not a TxActions payload",
            )
            .unwrap();
    }
    let app = create_router(test_config(store)).await;
    let (blocks, _) = page_block_numbers(&app, "/api/v1/activities?filter=object&limit=2").await;
    assert_eq!(blocks, expected);
    let (blocks, _) =
        page_block_numbers(&app, "/api/v1/transactions?activity_type=object&limit=2").await;
    assert_eq!(blocks, expected);
}

/// A bucket row whose `TxActions` row is gone is an index invariant
/// violation: the listing fails naming the position, it does not skip it.
#[tokio::test]
async fn global_bucket_row_without_tx_actions_is_a_500_naming_the_position() {
    let store = test_store();
    seed_sparse_object_chain(&store, 10, 3, 1);
    store
        .put_cf(
            store.cf_tx_actions_by_bucket(),
            &ckbadger_store::keys::encode_tx_actions_by_bucket_key(
                ckbadger_store::GlobalBucket::Object,
                20,
                4,
            ),
            b"",
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = fetch_json(app, "/api/v1/activities?filter=object&limit=10").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json.to_string();
    assert!(
        message.contains("bucket=object, block_num=20, tx_idx=4"),
        "{message}"
    );
}

/// BA-14: the block page lists a block's activities in block order. A
/// block-scoped feed reads ascending with `order=asc` (cursors page forward);
/// the default stays newest first, and `order` without `blockNumber` or with
/// another value is refused.
#[tokio::test]
async fn block_feed_reads_in_block_order_when_asked() {
    let store = test_store();
    let lock = [0x12; 32];
    for (hash, index) in [([0xa1; 32], 1), ([0xa2; 32], 2), ([0xa3; 32], 3)] {
        seed_committed_activity(&store, &lock, &hash, 42, index, 100, 0);
    }
    seed_committed_activity(&store, &lock, &[0xb1; 32], 43, 1, 100, 0);
    let app = create_router(test_config(store)).await;
    let indexes = |json: &serde_json::Value| -> Vec<i64> {
        json["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["txIndex"].as_i64().unwrap())
            .collect()
    };

    let (status, first) = get_json(&app, "/activities?blockNumber=42&order=asc&limit=2").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(indexes(&first), vec![1, 2], "{first}");
    assert_eq!(first["nextCursor"], "42:2", "{first}");
    let (status, second) = get_json(
        &app,
        "/activities?blockNumber=42&order=asc&limit=2&cursor=42:2",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(indexes(&second), vec![3], "{second}");
    assert!(second["nextCursor"].is_null(), "{second}");

    let (status, default) = get_json(&app, "/activities?blockNumber=42").await;
    assert_eq!(status, StatusCode::OK, "{default}");
    assert_eq!(indexes(&default), vec![3, 2, 1]);
    let (status, explicit) = get_json(&app, "/activities?blockNumber=42&order=desc").await;
    assert_eq!(status, StatusCode::OK, "{explicit}");
    assert_eq!(indexes(&explicit), vec![3, 2, 1]);

    let (status, json) = get_json(&app, "/activities?order=asc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    let (status, json) = get_json(&app, "/activities?blockNumber=42&order=sideways").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}

/// BA-35: the address page's single "Script" chip is `filter=script` — the
/// address's activities that call a type script OR a lock script, read from
/// the participant tag bitmap like every other address filter.
#[tokio::test]
async fn address_script_filter_is_type_calls_or_lock_calls() {
    use ckbadger_store::types::{TAG_LOCK_CALL, TAG_TOKEN, TAG_TYPE_CALL};
    let store = test_store();
    let lock = [0x31; 32];
    seed_committed_activity(&store, &lock, &[0xc1; 32], 50, 1, -10, TAG_TYPE_CALL);
    seed_committed_activity(&store, &lock, &[0xc2; 32], 51, 1, -10, TAG_LOCK_CALL);
    seed_committed_activity(&store, &lock, &[0xc3; 32], 52, 1, -10, 0);
    seed_committed_activity(&store, &lock, &[0xc4; 32], 53, 1, -10, TAG_TOKEN);
    seed_committed_activity(
        &store,
        &lock,
        &[0xc5; 32],
        54,
        1,
        -10,
        TAG_TYPE_CALL | TAG_LOCK_CALL,
    );
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/addresses/0x{}/activities?filter=script",
            hex::encode(lock)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let blocks: Vec<i64> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["blockNumber"].as_i64().unwrap())
        .collect();
    assert_eq!(blocks, vec![54, 51, 50], "{json}");
}

/// Review P1-1 / 27: one block's activities read the block header once and
/// the page's `activities` rows with one multi_get, whatever the number of
/// rows; no `tx_index` row is read by key.
#[tokio::test]
async fn block_activities_read_the_header_once_and_the_rows_with_one_multi_get() {
    let store = test_store();
    for index in 1..=12i32 {
        let mut tx = [0x5a; 32];
        tx[..4].copy_from_slice(&index.to_be_bytes());
        seed_committed_activity(&store, &[0x11; 32], &tx, 30, index, 100, 0);
    }
    let app = create_router(test_config(store.clone())).await;
    store.reset_read_call_counters();
    let (status, json) = get_json(&app, "/activities?blockNumber=30&limit=10").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"].as_array().unwrap().len(), 10);
    assert_eq!(store.point_reads(ckbadger_store::CF_BLOCK_HEADERS), 1);
    assert_eq!(store.point_reads(ckbadger_store::CF_TX_INDEX), 0);
    assert_eq!(store.multi_gets(ckbadger_store::CF_TX_ACTIONS), 1);
    assert_eq!(store.multi_gets(ckbadger_store::CF_TX_INDEX), 0);
}

// ---------------------------------------------------------------------------
// The fold's code (the page-phase review's MUST-1, the P3 follow-up)
// ---------------------------------------------------------------------------

/// An object's row, made by `created_at_tx`.
fn object_row(
    standard: ckbadger_store::types::ObjectStandard,
    extra: ckbadger_store::types::ObjectExtra,
    collection_id: Option<Vec<u8>>,
    created_at_tx: &[u8],
) -> ckbadger_store::types::ObjectEntry {
    ckbadger_store::types::ObjectEntry {
        standard,
        collection_id,
        token_id: None,
        owner_lock_hash: None,
        name: None,
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: created_at_tx.to_vec(),
        extra,
    }
}

/// A token's row: its type script.
fn token_row(code: [u8; 32], standard: &str) -> ckbadger_store::types::TokenInfo {
    ckbadger_store::types::TokenInfo {
        type_code_hash: code.to_vec(),
        hash_type: 1,
        type_args: vec![0x66; 32],
        standard: standard.to_string(),
        name: None,
        symbol: None,
        decimals: None,
        max_supply: None,
        first_seen_block: 0,
        icon_url: None,
        description: None,
        transfers_count: 0,
        metadata_source: None,
        chain_metadata: None,
    }
}

/// MUST-1 of the page-phase review (the P3 follow-up): the fold compares what was done plus the
/// script code, and the API sent no code for a token, object or identity row: `typeCalls` holds
/// only the type scripts the indexer does not recognise, and an item delta carries no code hash.
/// `itemTypes` names, for a committed activity, the type script of every item it moved by its
/// code hash and hash type (never its args), each code once, as the indexer classified the item:
/// a token's row; a spore's and an identity's cell the indexer recorded (the item outpoint
/// index); an m-NFT token's creating output the indexer recorded as the token; and the Nervos
/// DAO's for a DAO step. So an sUDT and an xUDT transfer, a Spore and an m-NFT transfer, a .bit
/// Cell and a did:ckb transfer each name a different code.
#[tokio::test]
async fn item_types_name_every_items_type_script_by_its_code() {
    use ckbadger_store::types::{
        IdentityStandard, ItemDelta, ItemKind, ObjectExtra, ObjectStandard, ParticipantDelta,
        ParticipantId, ProtocolAction, TxActions,
    };

    let store = test_store();
    let (sudt, xudt) = ([0x5e; 32], [0x50; 32]);
    let (spore_code, mnft_code) = ([0x4a; 32], [0x2b; 32]);
    let (bit_cell_code, did_code) = ([0x1c; 32], [0xcf; 32]);
    let (token_a, token_b) = ([0xa1u8; 32], [0xb1u8; 32]);
    let spore = vec![0x51; 32];
    let mnft = [vec![0x3c; 24], vec![0, 0, 0, 7]].concat();
    let (bit_cell, did) = (vec![0xbc; 32], vec![0xdd; 20]);
    let (spore_mint, mnft_mint, bit_cell_tx, did_tx) =
        ([0x71u8; 32], [0x72u8; 32], [0x73u8; 32], [0x74u8; 32]);

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_token(&token_a, &token_row(sudt, "sudt"));
    batch.put_token(&token_b, &token_row(xudt, "xudt"));
    batch.put_spore(
        &spore,
        &object_row(
            ObjectStandard::Spore,
            ObjectExtra::Spore {
                content_type: "image/png".into(),
                content_length: 10,
                media_profile: Default::default(),
            },
            Some(ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION.to_vec()),
            &spore_mint,
        ),
    );
    batch.put_mnft(
        &mnft,
        &object_row(
            ObjectStandard::MnftToken,
            ObjectExtra::MnftToken {
                token_index: 7,
                characteristic: vec![0; 8],
                configure: 0,
                state: 0,
            },
            Some(mnft[..24].to_vec()),
            &mnft_mint,
        ),
    );
    // the m-NFT token's creating transaction, its output 1 recorded as the token
    let other: &[u8] = &[0x01];
    seed_creating_tx(&mut batch, 2, 0, mnft_mint, mnft_code, &[other, &mnft]);
    batch.put_mnft_token_outpoint(&mnft_mint, 1, &mnft);
    // the spore's and the identities' cells, in the item outpoint index
    for (tx, code, id) in [
        (spore_mint, spore_code, &spore),
        (bit_cell_tx, bit_cell_code, &bit_cell),
        (did_tx, did_code, &did),
    ] {
        batch.put_spore_outpoint(&tx, 0, id);
        batch
            .put_cell_payload_by_outpoint(&tx, 0, &typed_cell(code, &[0x02; 32]))
            .unwrap();
    }
    batch.commit().unwrap();

    let item = |id: &[u8], kind| ItemDelta {
        item_id: id.to_vec(),
        kind,
        magnitude: 1,
        negative: false,
    };
    let rows: Vec<(u8, Vec<ItemDelta>, Vec<ProtocolAction>)> = vec![
        (1, vec![item(&token_a, ItemKind::Token)], vec![]),
        (2, vec![item(&token_b, ItemKind::Token)], vec![]),
        (3, vec![item(&spore, ItemKind::Object)], vec![]),
        (4, vec![item(&mnft, ItemKind::Object)], vec![]),
        (
            5,
            vec![item(
                &bit_cell,
                ItemKind::Identity(IdentityStandard::BitCell),
            )],
            vec![],
        ),
        (
            6,
            vec![item(&did, ItemKind::Identity(IdentityStandard::DidCkb))],
            vec![],
        ),
        (
            7,
            vec![],
            vec![ProtocolAction::new(
                "dao",
                "deposit",
                serde_json::json!({"capacity": "10200000000"}),
            )],
        ),
    ];
    for (n, items, protocol_actions) in rows {
        let tx_hash = vec![0xe0 + n; 32];
        let block = 100 + i64::from(n);
        let actions = TxActions {
            tx_hash: tx_hash.clone(),
            block_hash: vec![0xb0 + n; 32],
            block_number: block,
            tx_index: 0,
            timestamp: 1_700_000_000_000,
            is_cellbase: false,
            protocol_actions,
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0x40 + n; 32]),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: items,
                tags: 0,
                roles: 0,
            }],
        };
        seed_named_tx(&store, block, &tx_hash, &actions);
    }
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let codes: Vec<Vec<(String, String)>> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row["itemTypes"]
                .as_array()
                .unwrap_or_else(|| panic!("a committed row states its item types: {row}"))
                .iter()
                .map(|t| {
                    (
                        t["typeCodeHash"].as_str().unwrap().to_string(),
                        t["typeHashType"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        })
        .collect();
    let code = |c: [u8; 32]| vec![(format!("0x{}", hex::encode(c)), "type".to_string())];
    assert_eq!(
        codes,
        vec![
            // newest first: the DAO deposit, then the identities, the objects, the tokens
            vec![(
                "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e".to_string(),
                "type".to_string()
            )],
            code(did_code),
            code(bit_cell_code),
            code(mnft_code),
            code(spore_code),
            code(xudt),
            code(sudt),
        ],
        "{json}"
    );
}

/// The lead's ruling on the security read of the item types (the P3 follow-up): an item's type
/// script is what the indexer classified, never a match on args among a transaction's outputs, as
/// anyone can write a second output whose args equal an object's id under any code. A cluster's
/// creating transaction carrying a crafted second output (another code, the cluster's id as args)
/// and an m-NFT token's carrying one the indexer did not record as the token: the page answers,
/// the cluster's row names the registry's cluster code, the token's row the recorded cell's code.
#[tokio::test]
async fn item_types_name_what_the_indexer_classified_not_a_crafted_output() {
    use ckbadger_store::types::{
        ItemDelta, ItemKind, ObjectExtra, ObjectStandard, ParticipantDelta, ParticipantId,
        TxActions,
    };

    let store = test_store();
    // the mainnet Spore Cluster code (docs/metadata/scripts/spore-cluster.toml), data1
    let cluster_code: [u8; 32] =
        hex::decode("7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075")
            .unwrap()
            .try_into()
            .unwrap();
    let crafted = [0xee; 32];
    let (cluster, mnft) = (vec![0x86; 32], [vec![0x3c; 24], vec![0, 0, 0, 9]].concat());
    let (cluster_mint, mnft_mint) = ([0x61u8; 32], [0x62u8; 32]);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_spore(
        &cluster,
        &object_row(
            ObjectStandard::SporeCluster,
            ObjectExtra::SporeCluster,
            None,
            &cluster_mint,
        ),
    );
    batch.put_mnft(
        &mnft,
        &object_row(
            ObjectStandard::MnftToken,
            ObjectExtra::MnftToken {
                token_index: 9,
                characteristic: vec![0; 8],
                configure: 0,
                state: 0,
            },
            Some(mnft[..24].to_vec()),
            &mnft_mint,
        ),
    );
    // each creating transaction: the crafted output first, then the real one, both with the id
    seed_creating_tx(
        &mut batch,
        1,
        0,
        cluster_mint,
        crafted,
        &[&cluster, &cluster],
    );
    let mut real = typed_cell(cluster_code, &cluster);
    real.type_hash_type = Some(2); // data1
    batch
        .put_cell_payload_by_outpoint(&cluster_mint, 1, &real)
        .unwrap();
    seed_creating_tx(&mut batch, 2, 0, mnft_mint, crafted, &[&mnft, &mnft]);
    batch
        .put_cell_payload_by_outpoint(&mnft_mint, 1, &typed_cell([0x2b; 32], &mnft))
        .unwrap();
    batch.put_mnft_token_outpoint(&mnft_mint, 1, &mnft);
    batch.commit().unwrap();

    for (n, id) in [(1u8, &cluster), (2, &mnft)] {
        let tx_hash = vec![0xd0 + n; 32];
        let block = 200 + i64::from(n);
        let actions = TxActions {
            tx_hash: tx_hash.clone(),
            block_hash: vec![0xc0 + n; 32],
            block_number: block,
            tx_index: 0,
            timestamp: 1_700_000_000_000,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0x30 + n; 32]),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![ItemDelta {
                    item_id: id.clone(),
                    kind: ItemKind::Object,
                    magnitude: 1,
                    negative: false,
                }],
                tags: 0,
                roles: 0,
            }],
        };
        seed_named_tx(&store, block, &tx_hash, &actions);
    }
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let named: Vec<serde_json::Value> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["itemTypes"].clone())
        .collect();
    assert_eq!(
        named,
        vec![
            serde_json::json!([{
                "typeCodeHash": format!("0x{}", "2b".repeat(32)),
                "typeHashType": "type",
                "scriptName": null,
            }]),
            serde_json::json!([{
                "typeCodeHash": format!("0x{}", hex::encode(cluster_code)),
                "typeHashType": "data1",
                "scriptName": null,
            }]),
        ],
        "{json}"
    );
}

/// One activity's object row, `n`th of a seed, moving `items` for one party.
fn items_row(n: u8, items: Vec<ckbadger_store::types::ItemDelta>) -> TxActions {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};
    TxActions {
        tx_hash: vec![0xd0 + n; 32],
        block_hash: vec![0xc0 + n; 32],
        block_number: 300 + i64::from(n),
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock([0x30 + n; 32]),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: items,
            tags: 0,
            roles: 0,
        }],
    }
}

fn object_item(id: &[u8]) -> ckbadger_store::types::ItemDelta {
    ckbadger_store::types::ItemDelta {
        item_id: id.to_vec(),
        kind: ckbadger_store::types::ItemKind::Object,
        magnitude: 1,
        negative: false,
    }
}

/// The third read's T SHOULD-2: the resolver's unread branches, each a case. Where the stores
/// cannot say an item's code, the row states no item types and says why, the page answers:
/// a spore whose row has no recorded cell; a spore whose recorded cell's payload has no type
/// script; a cluster whose creating transaction has two outputs under two different registered
/// Cluster codes (the two testnet deployments, docs/metadata/scripts/spore-cluster.toml) with its
/// id as args, which `the_one` refuses to choose between.
#[tokio::test]
async fn item_types_unread_where_the_stores_hold_no_one_code() {
    use ckbadger_store::types::{ObjectExtra, ObjectStandard};

    let store = test_store();
    let spore_extra = || ObjectExtra::Spore {
        content_type: "image/png".into(),
        content_length: 10,
        media_profile: Default::default(),
    };
    let (unrecorded, untyped, cluster) = (vec![0x81; 32], vec![0x82; 32], vec![0x83; 32]);
    let (untyped_mint, cluster_mint) = ([0x92u8; 32], [0x93u8; 32]);
    let code = |hex_code: &str| -> [u8; 32] { hex::decode(hex_code).unwrap().try_into().unwrap() };
    let (cluster_v1, cluster_v2) = (
        code("598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080"),
        code("0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058"),
    );
    let mut batch = StoreBatch::new(store.as_ref());
    let sole = Some(ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
    batch.put_spore(
        &unrecorded,
        &object_row(
            ObjectStandard::Spore,
            spore_extra(),
            sole.clone(),
            &[0x91; 32],
        ),
    );
    batch.put_spore(
        &untyped,
        &object_row(ObjectStandard::Spore, spore_extra(), sole, &untyped_mint),
    );
    batch.put_spore_outpoint(&untyped_mint, 0, &untyped);
    let mut no_type = typed_cell([0x4a; 32], &untyped);
    no_type.type_code_hash = None;
    no_type.type_hash_type = None;
    no_type.type_args = None;
    batch
        .put_cell_payload_by_outpoint(&untyped_mint, 0, &no_type)
        .unwrap();
    batch.put_spore(
        &cluster,
        &object_row(
            ObjectStandard::SporeCluster,
            ObjectExtra::SporeCluster,
            None,
            &cluster_mint,
        ),
    );
    seed_creating_tx(
        &mut batch,
        3,
        0,
        cluster_mint,
        cluster_v1,
        &[&cluster, &cluster],
    );
    batch
        .put_cell_payload_by_outpoint(&cluster_mint, 1, &typed_cell(cluster_v2, &cluster))
        .unwrap();
    batch.commit().unwrap();
    for (n, id) in [(1u8, &unrecorded), (2, &untyped), (3, &cluster)] {
        let actions = items_row(n, vec![object_item(id)]);
        seed_named_tx(
            &store,
            actions.block_number,
            &actions.tx_hash.clone(),
            &actions,
        );
    }
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let rows = json["data"].as_array().unwrap();
    // newest first: the cluster, the untyped spore, the unrecorded spore
    let whys: Vec<&str> = rows
        .iter()
        .map(|row| {
            assert_eq!(row["itemTypes"], serde_json::Value::Null, "{row}");
            row["itemTypesUnread"].as_str().unwrap()
        })
        .collect();
    assert!(
        whys[0].contains("2 cluster cell in its creating transactions under different codes"),
        "{}",
        whys[0]
    );
    assert!(whys[1].contains("has no type script"), "{}", whys[1]);
    assert!(whys[2].contains("no cell recorded for it"), "{}", whys[2]);
}

/// The third read's La S1: an m-NFT batch's item types cost K × N reads, one multi_get over the
/// creating transaction's N outputs PER token. Its recorded token ids are read once per creating
/// transaction, beside its payloads. Here one activity moves K tokens of one creating transaction
/// of N = K + 1 outputs: the object outpoint index is read by one multi_get whatever K is (it
/// was K). For a batch of 1,000 tokens in a 1,000-output transaction that is 1,000 id reads and
/// 1,000 payload reads, 2,001 reads with the transaction's own (it was 10^6 id reads).
#[tokio::test]
async fn item_types_read_a_creating_transactions_token_ids_once() {
    use ckbadger_store::types::{ObjectExtra, ObjectStandard};

    let mut reads = Vec::new();
    for k in [1u8, 5] {
        let store = test_store();
        let mint = [0x62u8; 32];
        let tokens: Vec<Vec<u8>> = (0..k)
            .map(|i| [vec![0x3c; 24], vec![0, 0, 0, i]].concat())
            .collect();
        let mut batch = StoreBatch::new(store.as_ref());
        let other: &[u8] = &[0x01];
        let ids: Vec<&[u8]> = std::iter::once(other)
            .chain(tokens.iter().map(Vec::as_slice))
            .collect();
        seed_creating_tx(&mut batch, 2, 0, mint, [0x2b; 32], &ids);
        for (i, token) in tokens.iter().enumerate() {
            batch.put_mnft(
                token,
                &object_row(
                    ObjectStandard::MnftToken,
                    ObjectExtra::MnftToken {
                        token_index: u32::try_from(i).unwrap(),
                        characteristic: vec![0; 8],
                        configure: 0,
                        state: 0,
                    },
                    Some(token[..24].to_vec()),
                    &mint,
                ),
            );
            batch.put_mnft_token_outpoint(&mint, i16::try_from(i + 1).unwrap(), token);
        }
        batch.commit().unwrap();
        let actions = items_row(1, tokens.iter().map(|t| object_item(t)).collect());
        seed_named_tx(
            &store,
            actions.block_number,
            &actions.tx_hash.clone(),
            &actions,
        );
        let app = create_router(test_config(store.clone())).await;
        store.reset_read_call_counters();
        let (status, json) = get_json(&app, "/activities").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(
            json["data"][0]["itemTypes"],
            serde_json::json!([{
                "typeCodeHash": format!("0x{}", "2b".repeat(32)),
                "typeHashType": "type",
                "scriptName": null,
            }]),
            "{json}"
        );
        reads.push((k, store.multi_gets(ckbadger_store::CF_OBJECT_OUTPOINTS)));
    }
    assert_eq!(
        reads[0].1, reads[1].1,
        "token id reads grew with the batch: {reads:?}"
    );
}

/// The third read's La S1: an item's first recorded cell was a list of its whole out point
/// history (anyone can lengthen it with cheap self-transfers); it is one seek. The rows the
/// item outpoint index yields for one activity do not grow with the spore's history. First is by
/// key, the lowest transaction hash (the third read's La N2).
#[tokio::test]
async fn item_types_read_an_items_first_cell_by_one_seek() {
    use ckbadger_store::types::{ObjectExtra, ObjectStandard};

    let mut reads = Vec::new();
    for history in [2u8, 40] {
        let store = test_store();
        let spore = vec![0x51; 32];
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_spore(
            &spore,
            &object_row(
                ObjectStandard::Spore,
                ObjectExtra::Spore {
                    content_type: "image/png".into(),
                    content_length: 10,
                    media_profile: Default::default(),
                },
                Some(ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION.to_vec()),
                &[0x10; 32],
            ),
        );
        for h in 0..history {
            let tx = [0x10 + h; 32];
            batch.put_spore_outpoint(&tx, 0, &spore);
            batch
                .put_cell_payload_by_outpoint(&tx, 0, &typed_cell([0x4a; 32], &spore))
                .unwrap();
        }
        batch.commit().unwrap();
        let actions = items_row(1, vec![object_item(&spore)]);
        seed_named_tx(
            &store,
            actions.block_number,
            &actions.tx_hash.clone(),
            &actions,
        );
        let app = create_router(test_config(store.clone())).await;
        store.reset_read_call_counters();
        let (status, json) = get_json(&app, "/activities").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(
            json["data"][0]["itemTypes"][0]["typeCodeHash"],
            format!("0x{}", "4a".repeat(32)),
            "{json}"
        );
        reads.push((
            history,
            store.iterated_rows(ckbadger_store::CF_OBJECT_OUTPOINT_BY_ID),
        ));
    }
    assert_eq!(
        reads[0].1, reads[1].1,
        "the first cell's read grew with history: {reads:?}"
    );
}

/// Where the stores cannot say an item's script (here a token item with no token row), that part
/// of its row fails alone: the row states no item types and says why, so the feed folds it with no
/// other row; the page and its other rows answer.
#[tokio::test]
async fn item_types_a_row_the_stores_cannot_place_stands_alone() {
    use ckbadger_store::types::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TxActions};

    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_token(&[0xa1; 32], &token_row([0x5e; 32], "sudt"));
    batch.commit().unwrap();
    for (n, token) in [(9u8, [0xc4u8; 32]), (8, [0xa1; 32])] {
        let tx_hash = vec![0xe0 + n; 32];
        let block = 100 + i64::from(n);
        let actions = TxActions {
            tx_hash: tx_hash.clone(),
            block_hash: vec![0xb0 + n; 32],
            block_number: block,
            tx_index: 0,
            timestamp: 1_700_000_000_000,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0x40 + n; 32]),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![ItemDelta {
                    item_id: token.to_vec(),
                    kind: ItemKind::Token,
                    magnitude: 1,
                    negative: false,
                }],
                tags: 0,
                roles: 0,
            }],
        };
        seed_named_tx(&store, block, &tx_hash, &actions);
    }
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let rows = json["data"].as_array().unwrap();
    // newest first: the token with no row, then the sUDT one
    assert_eq!(rows[0]["itemTypes"], serde_json::Value::Null, "{json}");
    let why = rows[0]["itemTypesUnread"].as_str().unwrap();
    assert!(
        why.contains(&"c4".repeat(32)) && why.contains(&"e9".repeat(32)),
        "{why}"
    );
    assert!(why.contains("no token row"), "{why}");
    assert_eq!(
        rows[1]["itemTypesUnread"],
        serde_json::Value::Null,
        "{json}"
    );
    assert_eq!(
        rows[1]["itemTypes"][0]["typeCodeHash"],
        format!("0x{}", "5e".repeat(32)),
        "{json}"
    );
}
