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

/// `expected` item deltas as an address row states them (the third read's Lb SHOULD 2): each with
/// its movement in the whole transaction beside it, in order.
fn with_movements(expected: &serde_json::Value, movements: &[&str]) -> serde_json::Value {
    let items = expected
        .as_array()
        .expect("expected item deltas are an array");
    assert_eq!(items.len(), movements.len(), "one movement per item delta");
    serde_json::Value::Array(
        items
            .iter()
            .zip(movements)
            .map(|(item, movement)| {
                let mut item = item.clone();
                item["movement"] = serde_json::json!(movement);
                item
            })
            .collect(),
    )
}

/// The third read's Lb SHOULD 2, ruled: an address's row carries only that address's deltas, so
/// its page headed a burn "Token sent" and folded it with plain sends, and a mint with transfers in.
/// The address route states each of the row's items' movement in the whole transaction, from its
/// net over every party as every other feed's head computes it: `mint` where it rose, `burn`
/// where it fell, `transfer` where it only changed hands (one rule for every kind of item: an
/// object and an identity by their ids, as a token by its type script hash).
#[tokio::test]
async fn address_rows_state_each_items_movement_in_the_whole_transaction() {
    use ckbadger_store::types::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId};
    let lock_hash = vec![0x31; 32];
    let token = |magnitude: u128, negative: bool| ItemDelta {
        item_id: vec![0xa7; 32],
        kind: ItemKind::Token,
        magnitude,
        negative,
    };
    let party = |id: u8, items: Vec<ItemDelta>| ParticipantDelta {
        id: ParticipantId::Lock([id; 32]),
        ckb_delta: 0,
        used_delta: 0,
        item_deltas: items,
        tags: 0,
        roles: 0,
    };
    for (parties, movement) in [
        // the address's 100 leave and no party gains them
        (vec![party(0x31, vec![token(100, true)])], "burn"),
        // its 50 go to another party
        (
            vec![
                party(0x31, vec![token(50, true)]),
                party(0x32, vec![token(50, false)]),
            ],
            "transfer",
        ),
        // 10 arrive and no party gave them
        (vec![party(0x31, vec![token(10, false)])], "mint"),
    ] {
        let mut actions = make_test_tx_actions(&lock_hash, &[0xab; 32], &[0xbc; 32], 88, 1, 100, 0);
        actions.participants = parties;
        let json = address_activities_json(&lock_hash, &actions).await;
        assert_eq!(
            json["data"][0]["itemDeltas"][0]["movement"], movement,
            "{json}"
        );
    }
}

/// The automated security read of the movement (the lead's finding on a3c52252): a token amount
/// is a u128 on chain, and anyone can mint an xUDT of 2^127 units or more. The movement converted
/// each delta into an i128 and summed it there, failing the whole page for an amount valid chain
/// data holds. The amounts received and sent are summed apart, exactly, whatever their size, and
/// every party's address page answers: a transfer of u128::MAX units, a mint of 2^127 units, two
/// parties each receiving u128::MAX with no sender (a sum past a u128), and 2^128 received against
/// 5 sent, which only the sum's carry tells apart from a burn.
#[tokio::test]
async fn address_rows_state_the_movement_of_any_amount_a_transaction_holds() {
    use ckbadger_store::types::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId};
    let token = |magnitude: u128, negative: bool| ItemDelta {
        item_id: vec![0xa7; 32],
        kind: ItemKind::Token,
        magnitude,
        negative,
    };
    let party = |id: u8, items: Vec<ItemDelta>| ParticipantDelta {
        id: ParticipantId::Lock([id; 32]),
        ckb_delta: 0,
        used_delta: 0,
        item_deltas: items,
        tags: 0,
        roles: 0,
    };
    let cases: [(Vec<ParticipantDelta>, &str); 4] = [
        (
            vec![
                party(0x41, vec![token(u128::MAX, true)]),
                party(0x42, vec![token(u128::MAX, false)]),
            ],
            "transfer",
        ),
        (vec![party(0x41, vec![token(1u128 << 127, false)])], "mint"),
        (
            vec![
                party(0x41, vec![token(u128::MAX, false)]),
                party(0x42, vec![token(u128::MAX, false)]),
            ],
            "mint",
        ),
        // 2^128 received (its low bits 0, carried once) and 5 sent: only the carry says a mint
        (
            vec![
                party(0x41, vec![token(u128::MAX, false)]),
                party(0x42, vec![token(1, false)]),
                party(0x43, vec![token(5, true)]),
            ],
            "mint",
        ),
    ];
    for (parties, movement) in cases {
        let locks: Vec<Vec<u8>> = parties
            .iter()
            .map(|p| match p.id {
                ParticipantId::Lock(hash) => hash.to_vec(),
                _ => unreachable!("the seed's parties are locks"),
            })
            .collect();
        for lock_hash in locks {
            let mut actions =
                make_test_tx_actions(&lock_hash, &[0xab; 32], &[0xbc; 32], 88, 1, 100, 0);
            actions.participants = parties.clone();
            let (status, json) = address_activities_response(&lock_hash, &actions).await;
            assert_eq!(status, StatusCode::OK, "{json}");
            assert_eq!(
                json["data"][0]["itemDeltas"][0]["movement"], movement,
                "{json}"
            );
        }
    }
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
    // the third read's T SHOULD-5: a called lock's registry name is `lockScriptName` on the wire,
    // as on a party (null here: the test store imports no script names); the bare `scriptName` it
    // was is gone. At the merge with resync/all, the hash type beside the name is
    // `lockScriptNameBy` (null without a name), the pair of fields every name site carries
    assert_eq!(
        calls[1].get("lockScriptName"),
        Some(&serde_json::Value::Null),
        "{json}"
    );
    assert_eq!(
        calls[1].get("lockScriptNameBy"),
        Some(&serde_json::Value::Null),
        "{json}"
    );
    assert_eq!(calls[1].get("scriptName"), None, "{json}");
    assert_eq!(calls[1].get("scriptNameBy"), None, "{json}");
}

/// resync fix round S2 (logic S2): a lock call is decoded and named by its pair. The Default
/// Lock's code hash by `data` and JoyID's by `data` run no registered code (anyone can lock a
/// cell under them; no deployed binary has that data hash), so their args decode nothing and
/// name nothing: no `decoded`, no `decodeError` naming "Default Lock" or "JoyID". The same code
/// hashes by their registered pair still decode.
#[tokio::test]
async fn test_activity_lock_call_is_decoded_only_by_its_registered_pair() {
    let lock_hash = vec![0x26; 32];
    let mut actions = make_test_tx_actions(&lock_hash, &[0xac; 32], &[0xbd; 32], 88, 1, 100, 0);
    let default_lock =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let joyid =
        hex::decode("d00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323").unwrap();
    let call =
        |code_hash: &[u8], hash_type: i16, args: Vec<u8>| ckbadger_store::types::LockCallEntry {
            lock_code_hash: code_hash.to_vec(),
            lock_hash_type: hash_type,
            lock_args: args,
        };
    actions.lock_calls = vec![
        call(&default_lock, 0, vec![0; 20]),
        call(&joyid, 0, vec![0; 23]),
        call(&default_lock, 1, vec![0; 20]),
        call(&joyid, 1, vec![0; 23]),
    ];
    let json = address_activities_json(&lock_hash, &actions).await;
    let calls = &json["data"][0]["lockCalls"];
    for (index, what) in [(0, "Default Lock by data"), (1, "JoyID by data")] {
        assert_eq!(
            (calls[index].get("decoded"), calls[index].get("decodeError")),
            (
                Some(&serde_json::Value::Null),
                Some(&serde_json::Value::Null)
            ),
            "{what}: a pair that runs no registered code decodes nothing: {json}"
        );
    }
    assert_eq!(
        calls[2]["decoded"]["lock"], "secp256k1_blake160",
        "the Default Lock by type decodes: {json}"
    );
    assert_eq!(
        calls[3]["decodeError"],
        "JoyID: args are 23 bytes, layout subType(2) + pubkeyHash(20) expects 22",
        "JoyID by type states why: {json}"
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
        recognized_type_codes: vec![],
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
    seed_script_label(core_store.as_ref(), &type_code_hash, 1, "RGB++ Lock");
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
        recognized_type_codes: vec![],
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
    seed_script_label(core_store.as_ref(), &type_code_hash, 1, "RGB++ Lock");
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
        recognized_type_codes: vec![],
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
    // the .cell name's cell, as the indexer records it
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
        .clone()
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
    // re-pinned on purpose (lane feed, ruling 14 of gate P): the address row draws this party
    // alone and names every party by its short id, the other one too, and sends no other party in
    // full (it sent `participants`, the other party with its address); the other party's address
    // is on the transaction's own activity, which draws every party
    assert!(rows[0].get("participants").is_none(), "{json:?}");
    assert_eq!(rows[0]["participantCount"], 2);
    assert_eq!(
        rows[0]["partyIds"],
        serde_json::json!([
            [format!("0x{}", hex::encode(&other[..20])), 0],
            [format!("0x{}", hex::encode(prefix)), TAG_IDENTITY],
        ])
    );
    let (status, own) = get_json(
        &app,
        &format!("/transactions/0x{}/activity", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{own}");
    let party = &own["participants"][0];
    assert_eq!(party["lockHash"], format!("0x{}", hex::encode(&other)));
    assert!(party["address"].is_string(), "{own}");
    assert!(party["lockHashPrefix"].is_null());
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
        recognized_type_codes: vec![],
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

    // Re-pinned on purpose (lane feed, ruling 14 of gate P): the address row names the other
    // parties by their short ids and sends none in full, so it reads no other party's lock; the
    // named party is resolved where a row draws it, the global feed (two parties: both drawn).
    let feed_uri = format!("/addresses/0x{}/activities", hex::encode(&lock_hash));
    let named_id = serde_json::json!([format!("0x{}", hex::encode(named_prefix)), 0]);
    let named = |json: &serde_json::Value| json["data"][0]["participants"][1].clone();

    // Unresolved: no lock script starts with this prefix.
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let (status, feed) = get_json(&app, &feed_uri).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(feed["data"][0]["partyIds"][1], named_id, "{feed}");
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let participant = named(&json);
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
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let participant = named(&json);
    assert_eq!(
        participant["lockHash"],
        format!("0x{}", hex::encode(resolved_hash))
    );
    assert!(participant["address"].as_str().unwrap().starts_with("ck"));

    // Ambiguous: a second lock script with the same prefix is an error, never a guess, where the
    // party is drawn; the address row, which names it by its prefix alone, reads neither script.
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
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json["message"].as_str().unwrap().contains("ambiguous"),
        "{json}"
    );
    let (status, feed) = get_json(&app, &feed_uri).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(feed["data"][0]["partyIds"][1], named_id, "{feed}");
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
        recognized_type_codes: vec![],
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
        recognized_type_codes: vec![],
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

/// Every participant states `sent`, the indexer's senders bit (`TAG_INPUT`):
/// true exactly for a party whose lock is an input's. The global activity's
/// parties, an address activity's own party (`sent` beside its `tags`) and
/// every party's tags in `partyIds` (the bit itself) all read it from the one
/// place, the participant's tags; a party a protocol only named never sent.
#[tokio::test]
async fn every_participant_states_whether_it_sent() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_INPUT, TAG_TOKEN};
    let store = test_store();
    let payer = [0x42u8; 32];
    let payee = [0x11u8; 32];
    let named = [0x99u8; 20];
    let tx_hash = vec![0xaa; 32];
    let party = |id, ckb_delta, tags| ParticipantDelta {
        id,
        ckb_delta,
        used_delta: 0,
        item_deltas: vec![],
        tags,
        roles: 0,
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
        recognized_type_codes: vec![],
        participants: vec![
            party(ParticipantId::Lock(payer), -100, TAG_INPUT),
            party(ParticipantId::Lock(payee), 100, TAG_TOKEN),
            party(ParticipantId::LockPrefix(named), 0, 0),
        ],
    };
    seed_named_tx(&store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(
        &payer,
        10,
        0,
        &AddrTxValue::new(-100, true, false, TAG_INPUT),
    );
    batch.put_addr_tx(
        &payee,
        10,
        0,
        &AddrTxValue::new(100, false, true, TAG_TOKEN),
    );
    batch.commit().unwrap();
    sync_address_tx_counters(&store, &payer);
    sync_address_tx_counters(&store, &payee);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let sent: Vec<_> = json["data"][0]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["sent"].clone())
        .collect();
    assert_eq!(
        sent,
        vec![
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::json!(false)
        ],
        "{json}"
    );
    // every party by its short id (lane feed), the senders bit in its tags:
    // the bit a folded run counts its senders by, drawn or not
    let senders_bit = |row: &serde_json::Value| -> Vec<bool> {
        row["partyIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| pair[1].as_u64().unwrap() & u64::from(TAG_INPUT) != 0)
            .collect()
    };
    assert_eq!(
        senders_bit(&json["data"][0]),
        vec![true, false, false],
        "{json}"
    );

    // Re-pinned on purpose at the merge of lane feed (ruling 14 of gate P): an
    // address's row sends no other party in full, only every party by its short
    // id with its tags in `partyIds`; the other parties' `sent` is read there,
    // as the senders bit (it was each other party's `sent` in `participants`)
    for (lock, own) in [(payer, true), (payee, false)] {
        let (status, json) = get_json(
            &app,
            &format!("/addresses/0x{}/activities", hex::encode(lock)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let row = &json["data"][0];
        assert_eq!(row["sent"], serde_json::json!(own), "{json}");
        assert!(row.get("participants").is_none(), "{json}");
        assert_eq!(senders_bit(row), vec![true, false, false], "{json}");
    }
}

/// resync fix round T2 (tests report SHOULD-2): `sent` is the senders bit, never
/// the sign of the CKB change. A DAO withdrawal's sender gains its compensation:
/// its lock is an input's (`TAG_INPUT`) and its CKB rose (+50). It sent, on the
/// global row and on its own address row; the party whose CKB fell without an
/// input (-50, no bit) did not.
#[tokio::test]
async fn a_sender_whose_ckb_rose_still_sent() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_DAO, TAG_INPUT};
    let store = test_store();
    let withdrawer = [0x43u8; 32];
    let other = [0x12u8; 32];
    let tx_hash = vec![0xab; 32];
    let party = |id, ckb_delta, tags| ParticipantDelta {
        id,
        ckb_delta,
        used_delta: 0,
        item_deltas: vec![],
        tags,
        roles: 0,
    };
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
        recognized_type_codes: vec![],
        participants: vec![
            party(ParticipantId::Lock(withdrawer), 50, TAG_INPUT | TAG_DAO),
            party(ParticipantId::Lock(other), -50, 0),
        ],
    };
    seed_named_tx(&store, 11, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(
        &withdrawer,
        11,
        0,
        &AddrTxValue::new(50, true, true, TAG_INPUT | TAG_DAO),
    );
    batch.put_addr_tx(&other, 11, 0, &AddrTxValue::new(-50, false, false, 0));
    batch.commit().unwrap();
    sync_address_tx_counters(&store, &withdrawer);
    sync_address_tx_counters(&store, &other);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let sent: Vec<_> = json["data"][0]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["sent"].clone())
        .collect();
    assert_eq!(
        sent,
        vec![serde_json::json!(true), serde_json::json!(false)],
        "the withdrawer sent though its CKB rose; the other did not though its fell: {json}"
    );

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(withdrawer)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["sent"], serde_json::json!(true), "{json}");
}

/// A party whose lock script the store does not know is reported by its lock
/// hash with no address — never with the hex lock hash posing as an address.
/// Re-pinned on purpose (lane feed, ruling 14 of gate P): read where the party
/// is drawn, the transaction's own activity; the address row names the other
/// party by its short id and sends it in full no more.
#[tokio::test]
async fn unknown_lock_participant_is_unresolved_not_a_fabricated_address() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let other = [0x11u8; 32];
    seed_two_party_tx(&store, &lock_hash, &other);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/activity", hex::encode([0xaa; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let party = &json["participants"][1];
    assert_eq!(party["lockHash"], format!("0x{}", hex::encode(other)));
    assert!(party["address"].is_null(), "{json}");
}

/// A lock script the store holds but cannot encode (a hash_type no chain
/// allows) is store corruption, reported with the lock — not rendered as a
/// guess or dropped. Re-pinned on purpose (lane feed, ruling 14 of gate P):
/// read where the party is drawn, the transaction's own activity; the address
/// row reads no other party's lock any more, and answers.
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
        &format!("/transactions/0x{}/activity", hex::encode([0xaa; 32])),
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
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["data"][0]["partyIds"][1],
        serde_json::json!([format!("0x{}", hex::encode(&other[..20])), 0])
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
    // re-pinned on purpose (lane feed, ruling 14 of gate P): the address row names the recipient
    // by its short id and sends no other party in full; the recipient's address, encoded from the
    // record, is on the pool's own row, which draws both parties
    assert_eq!(
        row["partyIds"][1],
        serde_json::json!([format!("0x{}", hex::encode(&recipient[..20])), 0]),
        "{json}"
    );
    let (status, pool) = get_json(&app, "/transactions/pool").await;
    assert_eq!(status, StatusCode::OK, "{pool}");
    let recipient_hex = format!("0x{}", hex::encode(recipient));
    let party = pool["data"][0]["activity"]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["lockHash"] == recipient_hex)
        .unwrap_or_else(|| panic!("the recipient is a party of the pool row: {pool}"));
    assert_eq!(party["address"], expected, "{pool}");
}

// ── 2.6b: identity item deltas name their standard ──────────────────────

/// Every identity item delta carries its standard, as the builder recorded
/// it, in the standard's one wire value (`IdentityStandard::asset_standard`):
/// the frontend links the item to its page with it. Re-pinned on purpose (the
/// P3 follow-up): the four identities have their cells in the item outpoint
/// index, as the indexer records them; they still have no identity row, which
/// the standard needs none of.
#[tokio::test]
async fn identity_item_deltas_name_their_standard() {
    use ckbadger_store::types::{
        ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TypeCode, TAG_IDENTITY,
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
        // each identity's type script, as the indexer records the codes that ran (C7)
        recognized_type_codes: [0xd0u8, 0xd1, 0xd2, 0xd3]
            .iter()
            .map(|code| TypeCode {
                code_hash: [*code; 32],
                hash_type: 1,
            })
            .collect(),
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
    // the third read's T SHOULD-2, carried to C7: each identity's code is stated, on the address
    // row and the global row alike; it was read from each identity's recorded cell per request,
    // and is the row's now (`recognizedTypes`, no cell read)
    let codes = |row: &serde_json::Value| -> Vec<String> {
        row["recognizedTypes"]
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
        recognized_type_codes: vec![],
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
    // re-pinned on purpose (the third read's Lb SHOULD 2): the address row states each item's
    // movement in the whole transaction; the one party gains the spore and loses the token
    assert_eq!(
        json["data"][0]["itemDeltas"],
        with_movements(&expected, &["mint", "burn"]),
        "address feed"
    );

    let (status, global) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{global}");
    // re-pinned on purpose (lane feed, ruling 14 of gate P): every party's item states its
    // movement in the whole transaction, as an address's row does
    assert_eq!(
        global["data"][0]["participants"][0]["itemDeltas"],
        with_movements(&expected, &["mint", "burn"]),
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
        recognized_type_codes: vec![],
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

    // re-pinned on purpose (lane feed, ruling 14 of gate P): every party's item states its
    // movement in the whole transaction, on every response that draws the party
    let minted = with_movements(&expected, &["mint"]);
    for uri in ["/activities?filter=object&limit=20", "/activities?limit=20"] {
        let (status, json) = get_json(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {json}");
        assert_eq!(
            json["data"][0]["participants"][0]["itemDeltas"], minted,
            "{uri}"
        );
    }
    let (status, json) = get_json(&app, "/transactions?activity_type=object&limit=20").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["data"][0]["activity"]["participants"][0]["itemDeltas"], minted,
        "transactions?activity_type=object"
    );
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    // re-pinned on purpose (the third read's Lb SHOULD 2): the cluster is created, a mint
    assert_eq!(
        json["data"][0]["itemDeltas"],
        with_movements(&expected, &["mint"]),
        "address feed"
    );
    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/activity", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["participants"][0]["itemDeltas"], minted,
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
        recognized_type_codes: vec![],
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
    // re-pinned on purpose (the third read's Lb SHOULD 2): each item's movement in the whole
    // pending transaction, its one party gaining the new spore and losing the indexed one
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
                "movement": "mint",
            },
            {
                "kind": "object",
                "objectId": format!("0x{}", hex::encode(&spore)),
                "delta": -1,
                "collection": {"id": format!("0x{}", hex::encode(&cluster)), "name": "Nervape"},
                "class": "object",
                "composition": "btc_ckb",
                "role": "spore",
                "movement": "burn",
            },
        ]),
        "{json}"
    );
}

// ── integration: a spent never-seen lock is encoded from the record ──────

/// A pending transaction may spend a cell whose lock exists so far only as the
/// output of another pending transaction, so no store row knows it. The mirror
/// keeps that lock script on the record (`input_locks`); a row that draws the
/// sender (the pool's own row) must encode it from there exactly as it does for
/// the record's own outputs — never `null`, never a store-derived guess.
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
    // re-pinned on purpose (lane feed, ruling 14 of gate P): the address row names the sender by
    // its short id and sends no other party in full; the sender's address, encoded from the
    // record's spent input, is on the pool's own row, which draws both parties
    let sender_id = serde_json::json!([format!("0x{}", hex::encode(&sender[..20])), 0]);
    assert!(
        row["partyIds"].as_array().unwrap().contains(&sender_id),
        "{json}"
    );
    let (status, pool) = get_json(&app, "/transactions/pool").await;
    assert_eq!(status, StatusCode::OK, "{pool}");
    let sender_hex = format!("0x{}", hex::encode(sender));
    let party = pool["data"][0]["activity"]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["lockHash"] == sender_hex)
        .unwrap_or_else(|| panic!("the sender is a party of the pool row: {pool}"));
    assert_eq!(party["address"], expected, "{pool}");
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
    batch.commit().unwrap();
    seed_script_label(store.as_ref(), &code_hash, 1, "JoyID");
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

/// Lane rd/api-lock-names: a party's lock is named by its pair (code hash,
/// hash type). A lock on the default lock's code hash by `data` is another
/// script, and the default lock's name does not reach it.
#[tokio::test]
async fn by_pair_activity_participants_name_a_lock_by_its_pair() {
    let store = test_store();
    let by_type = [0x12; 32];
    let by_data = [0x13; 32];
    seed_committed_activity(&store, &by_type, &[0xa1; 32], 42, 1, 800, 0);
    seed_committed_activity(&store, &by_data, &[0xa2; 32], 42, 2, -900, 0);
    seed_default_lock_by_type_and_by_data(&store, &by_type, &by_data);
    let app = create_router(test_config(store)).await;

    let activity = |tx: &str| format!("/transactions/0x{}/activity", tx.repeat(32));
    let (status, json) = get_json(&app, &activity("a1")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["participants"][0]["lockKind"], "Default Lock",
        "{json}"
    );
    let (status, json) = get_json(&app, &activity("a2")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["participants"][0]["lockHash"],
        format!("0x{}", hex::encode(by_data)),
        "{json}"
    );
    assert!(
        json["participants"][0]["lockKind"].is_null(),
        "the default lock's name does not reach the lock by data: {json}"
    );
}

// ---------------------------------------------------------------------------
// The fold's code (the page-phase review's MUST-1, the P3 follow-up)
// ---------------------------------------------------------------------------

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

/// MUST-1 of the page-phase review (the P3 follow-up), re-pinned on purpose for C7
/// (resync/type-codes): the fold compares what was done plus the script code, so an activity states
/// the code of the type scripts that ran. It was `itemTypes`, resolved per request for the items
/// that changed hands (a token's row, an object's or identity's recorded cell, an m-NFT token's
/// creating output, the DAO for a DAO step), and a recognised script that moved nothing stated no
/// code. Now every activity states `recognizedTypes`: the codes its row records
/// (`recognized_type_codes`, written by the indexer from the transaction's own cells), each
/// (code hash, hash type) once, named by its pair. So an sUDT and an xUDT transfer, a Spore and
/// an m-NFT transfer, a .bit Cell and a did:ckb transfer each name a different code, a row may
/// name two, and a row whose script moved nothing names its code too.
#[tokio::test]
async fn recognized_types_are_the_codes_the_row_records() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TxActions, TypeCode};

    let store = test_store();
    let dao: [u8; 32] =
        hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e")
            .unwrap()
            .try_into()
            .unwrap();
    let code = |code_hash: [u8; 32], hash_type: i16| TypeCode {
        code_hash,
        hash_type,
    };
    // (block, recorded codes): one row each, newest first in the response
    let rows: Vec<(u8, Vec<TypeCode>)> = vec![
        (1, vec![code([0x5e; 32], 1)]),
        (2, vec![code([0x50; 32], 2)]),
        (3, vec![code([0x4a; 32], 1), code([0x5e; 32], 1)]),
        (4, vec![code(dao, 1)]),
        (5, vec![]),
    ];
    for (n, codes) in &rows {
        let tx_hash = vec![0xe0 + n; 32];
        let block = 100 + i64::from(*n);
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
            recognized_type_codes: codes.clone(),
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0x40 + n; 32]),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
        };
        seed_named_tx(&store, block, &tx_hash, &actions);
    }
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let stated: Vec<Vec<(String, String)>> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            assert!(row.get("itemTypes").is_none(), "{row}");
            assert!(row.get("itemTypesUnread").is_none(), "{row}");
            row["recognizedTypes"]
                .as_array()
                .unwrap_or_else(|| panic!("every row states its recognised types: {row}"))
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
    let named =
        |c: [u8; 32], hash_type: &str| (format!("0x{}", hex::encode(c)), hash_type.to_string());
    assert_eq!(
        stated,
        vec![
            vec![],
            vec![named(dao, "type")],
            vec![named([0x4a; 32], "type"), named([0x5e; 32], "type")],
            vec![named([0x50; 32], "data1")],
            vec![named([0x5e; 32], "type")],
        ],
        "{json}"
    );
}

/// C7 (resync/type-codes), re-pinned on purpose: the codes are the row's alone. The item-type
/// resolver read per request what the stores held for each item that moved, and a row whose item
/// they could not place (a token with no token row) stated no code and said why
/// (`itemTypesUnread`), standing alone in a feed; a crafted output (an item's id under another
/// code) had to be kept out of what it read. Nothing is read per item now: a row whose token has
/// no token row states its recorded code, and a token row naming another code changes nothing,
/// on the global feed and on an address's own feed alike.
#[tokio::test]
async fn recognized_types_are_read_from_the_row_alone() {
    use ckbadger_store::types::{
        ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TxActions, TypeCode,
    };

    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    // a token row that would name another code than the one the row records
    batch.put_token(&[0xa1; 32], &token_row([0x77; 32], "sudt"));
    batch.commit().unwrap();
    for (n, token) in [(9u8, [0xc4u8; 32]), (8, [0xa1; 32])] {
        let tx_hash = vec![0xe0 + n; 32];
        let block = 100 + i64::from(n);
        let lock = [0x40 + n; 32];
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
            recognized_type_codes: vec![TypeCode {
                code_hash: [0x5e; 32],
                hash_type: 1,
            }],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock(lock),
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
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_addr_tx(&lock, block, 0, &AddrTxValue::new(0, true, true, 0));
        batch.commit().unwrap();
        sync_address_tx_counters(&store, &lock);
    }
    let app = create_router(test_config(store)).await;
    let recorded = serde_json::json!([{
        "typeCodeHash": format!("0x{}", "5e".repeat(32)),
        "typeHashType": "type",
        "scriptName": null,
        "scriptNameBy": null,
    }]);
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    for row in json["data"].as_array().unwrap() {
        assert_eq!(row["recognizedTypes"], recorded, "{row}");
    }
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode([0x49u8; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["recognizedTypes"], recorded, "{json}");
}

/// Lane rd/api-lock-names, at the merge of fix/readability, carried to C7 at the merge of
/// resync/all into resync/type-codes: an activity's `recognizedTypes` names each code by the code
/// its pair runs (`utils::ScriptNamer`), as every other site does, with the hash type beside the
/// name where the pair is not the registry's reference for that code (`scriptNameBy`); C7 named the
/// code hash alone (`cached_script_name`), and `itemTypes` before it was named the same way as
/// here. A Nervos DAO step's code is the DAO's Type ID by `type`: "Nervos DAO" where the indexer
/// bound that Type ID to its binary, no name where nothing runs under it. A family registered by
/// its binary (`data`) and bound to a Type ID: the binary is the family by its own reference, the
/// Type ID by `type` the family "by type".
#[tokio::test]
async fn recognized_types_name_each_code_by_the_code_its_pair_runs() {
    use ckbadger_store::types::TypeCode;

    let dao: [u8; 32] = hex::decode(&ckbadger_indexer::parser::dao::DAO_CODE_HASH[2..])
        .unwrap()
        .try_into()
        .unwrap();
    for bound in [true, false] {
        let store = test_store();
        run_label_import_bundled(store.as_ref(), "mainnet").unwrap();
        if bound {
            observe_registered_type_deployments(store.as_ref());
        }
        let tx_hash = [0x71u8; 32];
        let mut actions = make_test_tx_actions(&[0x24; 32], &tx_hash, &[0xb7; 32], 300, 0, 100, 0);
        actions.protocol_actions =
            vec![ProtocolAction::new("dao", "deposit", serde_json::json!({}))];
        actions.recognized_type_codes = vec![TypeCode {
            code_hash: dao,
            hash_type: 1,
        }];
        seed_named_tx(&store, 300, &tx_hash, &actions);
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/activities").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(
            json["data"][0]["recognizedTypes"],
            serde_json::json!([{
                "typeCodeHash": ckbadger_indexer::parser::dao::DAO_CODE_HASH,
                "typeHashType": "type",
                "scriptName": if bound { serde_json::json!("Nervos DAO") } else { serde_json::Value::Null },
                "scriptNameBy": null,
            }]),
            "bound={bound}: {json}"
        );
    }

    let (binary, type_id) = ([0x6bu8; 32], [0x6cu8; 32]);
    let store = test_store();
    seed_family_run_by_its_type_id(&store, &binary, &type_id, "Fam");
    let tx_hash = [0x72u8; 32];
    let mut actions = make_test_tx_actions(&[0x25; 32], &tx_hash, &[0xb8; 32], 301, 0, 100, 0);
    actions.recognized_type_codes = vec![
        TypeCode {
            code_hash: binary,
            hash_type: 0,
        },
        TypeCode {
            code_hash: type_id,
            hash_type: 1,
        },
    ];
    seed_named_tx(&store, 301, &tx_hash, &actions);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["data"][0]["recognizedTypes"],
        serde_json::json!([
            {
                "typeCodeHash": format!("0x{}", hex::encode(binary)),
                "typeHashType": "data",
                "scriptName": "Fam",
                "scriptNameBy": null,
            },
            {
                "typeCodeHash": format!("0x{}", hex::encode(type_id)),
                "typeHashType": "type",
                "scriptName": "Fam",
                "scriptNameBy": "type",
            },
        ]),
        "{json}"
    );
}

// ── Lane feed, ruling 14 of gate P: a row states how many parties it has, sends in full only the
//    parties it draws, and names every party by its short id ──────────────────────────────────

/// How many parties the many-party row holds: more than a Godwoken row's 600 to 770.
const MANY_PARTIES: usize = 720;

/// The lock hash of party `i` of the many-party row: its first two bytes are `i`, so the parties
/// sort by `i` (the indexer's order) and each has its own 20-byte short id.
fn many_party_lock(i: usize) -> [u8; 32] {
    let mut hash = [0x5a; 32];
    hash[0] = u8::try_from(i >> 8).unwrap();
    hash[1] = u8::try_from(i & 0xff).unwrap();
    hash
}

/// The many-party row's parties: the five largest CKB changes are 400 (+5,000 CKB) and 100 to 103
/// (−1,000 CKB and a little more each); 104 is the sixth largest (−900 CKB); 7, 101 and 650 moved a
/// token (101 is also one of the five largest); every other party changed by a few shannons.
fn many_party_actions(tx_hash: &[u8]) -> TxActions {
    use ckbadger_store::types::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TAG_TOKEN};
    let token = |magnitude: u128, negative: bool| ItemDelta {
        item_id: vec![0x7e; 32],
        kind: ItemKind::Token,
        magnitude,
        negative,
    };
    let participants = (0..MANY_PARTIES)
        .map(|i| {
            let shannons = i128::try_from(i).unwrap();
            let ckb_delta: i128 = match i {
                400 => 500_000_000_000,
                100..=103 => -100_000_000_000 - shannons,
                104 => -90_000_000_000,
                _ => 1_000 + shannons,
            };
            let item_deltas = match i {
                7 => vec![token(50, true)],
                101 => vec![token(10, true)],
                650 => vec![token(60, false)],
                _ => vec![],
            };
            ParticipantDelta {
                id: ParticipantId::Lock(many_party_lock(i)),
                ckb_delta,
                used_delta: 0,
                tags: if item_deltas.is_empty() { 0 } else { TAG_TOKEN },
                item_deltas,
                roles: 0,
            }
        })
        .collect();
    TxActions {
        tx_hash: tx_hash.to_vec(),
        block_hash: vec![0xb7; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        recognized_type_codes: vec![],
        participants,
    }
}

/// `["0x<the first 20 bytes of the lock hash>", tags]`: a party's short id as the route sends it.
fn short_id(lock: &[u8; 32], tags: u16) -> serde_json::Value {
    serde_json::json!([format!("0x{}", hex::encode(&lock[..20])), tags])
}

/// Ruling 14 of gate P (the owner, 2026-11-20): `/activities?filter=token` answered 8.25 MB for one
/// page of 50, every field of each of a Godwoken row's 600 to 770 parties (about 680 bytes a
/// party). A row now states its exact `participantCount`; sends in full only the parties a row
/// draws, by one stated rule (the five largest CKB changes, the fee payer, every party whose items
/// moved; a row of at most six parties draws them all), in the transaction's order, each once; and
/// names EVERY party by its short id with its tags, which the page's fold counts distinct parties
/// from. The serialized row stays within 52 bytes a party and 8 KB, which the old shape (every
/// party in full) does not.
#[tokio::test]
async fn a_many_party_row_states_its_count_draws_by_the_rule_and_names_every_party() {
    let store = test_store();
    let tx_hash = vec![0xa7; 32];
    let actions = many_party_actions(&tx_hash);
    seed_named_tx(&store, 10, &tx_hash, &actions);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];

    assert_eq!(row["participantCount"], MANY_PARTIES, "participantCount");
    // every party, by its short id and its tags, in the transaction's order
    let expected_ids: Vec<serde_json::Value> = actions
        .participants
        .iter()
        .enumerate()
        .map(|(i, p)| short_id(&many_party_lock(i), p.tags))
        .collect();
    assert_eq!(row["partyIds"], serde_json::Value::Array(expected_ids));

    // the drawn set: the five largest (100, 101, 102, 103, 400; not the sixth, 104), in the
    // transaction's order. Re-pinned on purpose (lane feed, part 4): a party is drawn for moving
    // an item no more (7 and 650 were; 101 is drawn as one of the five), the row states its items
    // once instead
    let drawn: Vec<String> = row["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["lockHash"].as_str().unwrap().to_string())
        .collect();
    let expected_drawn: Vec<String> = [100, 101, 102, 103, 400]
        .iter()
        .map(|i| format!("0x{}", hex::encode(many_party_lock(*i))))
        .collect();
    assert_eq!(drawn, expected_drawn);
    // no party paid the fee alone: many changed
    assert_eq!(row["feePayer"], serde_json::Value::Null);
    // a drawn party's own item carries its movement in the whole transaction (−50 −10 +60)
    let own = &row["participants"][1]["itemDeltas"][0];
    assert_eq!(own["movement"], "transfer", "{own}");
    // and the row states the token once: it changed hands, 60 of it, among three parties
    assert_eq!(
        row["items"],
        serde_json::json!([{
            "kind": "token",
            "typeScriptHash": format!("0x{}", "7e".repeat(32)),
            "symbol": null,
            "decimals": null,
            "movement": "transfer",
            "amount": "60",
            "parties": 3,
        }])
    );

    let row_bytes = serde_json::to_string(row).unwrap().len();
    let id_bytes = serde_json::to_string(&row["partyIds"]).unwrap().len();
    assert!(
        id_bytes <= 52 * MANY_PARTIES,
        "the id list takes {id_bytes} bytes for {MANY_PARTIES} parties"
    );
    assert!(
        row_bytes <= 52 * MANY_PARTIES + 8 * 1024,
        "the row takes {row_bytes} bytes for {MANY_PARTIES} parties"
    );
}

/// The same activity on the transaction's own page draws every party (that page lists each one's
/// change), with the same count and ids; an address's feed states the count, every party's id and
/// the fee payer beside the address's own changes, and sends no other party in full: its row
/// draws the address's party alone (the route's `participants` of the other parties is gone).
#[tokio::test]
async fn the_transaction_page_draws_every_party_and_an_address_row_names_them_by_id() {
    let store = test_store();
    let tx_hash = vec![0xa8; 32];
    let actions = many_party_actions(&tx_hash);
    seed_named_tx(&store, 10, &tx_hash, &actions);
    let focus = many_party_lock(600);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(&focus, 10, 0, &AddrTxValue::new(1_600, true, true, 0));
    batch.commit().unwrap();
    sync_address_tx_counters(&store, &focus);
    let app = create_router(test_config(store)).await;

    let own_uri = format!("/transactions/0x{}/activity", hex::encode(&tx_hash));
    let (status, own) = get_json(&app, &own_uri).await;
    assert_eq!(status, StatusCode::OK, "{own}");
    assert_eq!(own["participantCount"], MANY_PARTIES, "participantCount");
    assert_eq!(own["participants"].as_array().unwrap().len(), MANY_PARTIES);
    assert_eq!(own["partyIds"].as_array().unwrap().len(), MANY_PARTIES);

    let feed_uri = format!("/addresses/0x{}/activities", hex::encode(focus));
    let (status, feed) = get_json(&app, &feed_uri).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    let row = &feed["data"][0];
    assert_eq!(row["ckbDelta"], "1600", "ckbDelta");
    assert_eq!(row["participantCount"], MANY_PARTIES, "participantCount");
    assert_eq!(row["partyIds"].as_array().unwrap().len(), MANY_PARTIES);
    assert_eq!(row["partyIds"][600], short_id(&focus, 0));
    assert_eq!(row["feePayer"], serde_json::Value::Null);
    assert!(
        row.get("participants").is_none(),
        "the address row sends no other party in full"
    );
}

/// The fee payer (the party whose only movement is the fee): stated by the route from every party,
/// and drawn. Only the payer's CKB changed and nothing else moved: a row of two parties names it;
/// a row of eight draws it once though it is also the largest change, beside the four next
/// largest (all zero: the earliest four). A party that received, an item moved, and a Nervos DAO
/// withdrawal (whose compensation enters) leave no fee payer.
#[tokio::test]
async fn the_route_states_the_fee_payer_from_every_party_and_draws_it() {
    use ckbadger_store::types::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId};

    let party = |i: u8, ckb_delta: i128| ParticipantDelta {
        id: ParticipantId::Lock([0x10 + i; 32]),
        ckb_delta,
        used_delta: 0,
        item_deltas: vec![],
        tags: 0,
        roles: 0,
    };
    let row = |n: u8, participants: Vec<ParticipantDelta>| TxActions {
        tx_hash: vec![0xe0 + n; 32],
        block_hash: vec![0xc0 + n; 32],
        block_number: 100 + i64::from(n),
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        recognized_type_codes: vec![],
        participants,
    };
    let lock_of = |i: u8| format!("0x{}", hex::encode([0x10 + i; 32]));

    let two = row(1, vec![party(0, 0), party(1, -1_000)]);
    let eight = row(
        2,
        (0..8)
            .map(|i| party(i, if i == 6 { -2_000 } else { 0 }))
            .collect(),
    );
    let received = row(3, vec![party(0, -1_000), party(1, 500)]);
    let mut moved = row(4, vec![party(0, -1_000), party(1, 0)]);
    moved.participants[1].item_deltas = vec![ItemDelta {
        item_id: vec![0x7e; 32],
        kind: ItemKind::Token,
        magnitude: 5,
        negative: false,
    }];
    let mut withdrawal = row(5, vec![party(0, -1_000)]);
    withdrawal.protocol_actions = vec![ProtocolAction::new(
        "dao",
        "withdraw_complete",
        serde_json::json!({}),
    )];

    let store = test_store();
    for actions in [&two, &eight, &received, &moved, &withdrawal] {
        seed_named_tx(&store, actions.block_number, &actions.tx_hash, actions);
    }
    let app = create_router(test_config(store)).await;
    let own =
        |actions: &TxActions| format!("/transactions/0x{}/activity", hex::encode(&actions.tx_hash));
    let (_, json) = get_json(&app, &own(&two)).await;
    assert_eq!(json["feePayer"], lock_of(1), "{json}");
    let (_, json) = get_json(&app, "/activities?limit=10").await;
    let eight_hash = format!("0x{}", hex::encode(&eight.tx_hash));
    let eight_row = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["txHash"] == eight_hash)
        .unwrap();
    assert_eq!(eight_row["feePayer"], lock_of(6), "{eight_row}");
    let drawn: Vec<&str> = eight_row["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["lockHash"].as_str().unwrap())
        .collect();
    assert_eq!(
        drawn,
        [lock_of(0), lock_of(1), lock_of(2), lock_of(3), lock_of(6)]
    );
    for none in [&received, &moved, &withdrawal] {
        let (_, json) = get_json(&app, &own(none)).await;
        assert_eq!(json["feePayer"], serde_json::Value::Null, "{json}");
    }
}

/// Two fee rules, two questions (the lead's F3 on lane feed). The route's `feePayer` names the one
/// party whose line is the fee exactly (a feed row says "fee" on it): a protocol step is allowed,
/// so a Nervos DAO deposit from its owner's own address names the owner, whose one change is the
/// fee. The transaction page's `onlyFeeMoved` (frontend/lib/tx-flows.ts) asks whether nothing but
/// the fee moved: no protocol step, and the fee paid by one party or several, so two parties that
/// paid the fee together are "only the fee" and name no fee payer here. The page's answer on the
/// same two transactions is held in frontend/__tests__/lib/tx-flows.test.ts.
#[tokio::test]
async fn the_fee_payer_names_a_depositor_and_none_of_two_payers() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};

    let party = |i: u8, ckb_delta: i128| ParticipantDelta {
        id: ParticipantId::Lock([0x20 + i; 32]),
        ckb_delta,
        used_delta: 0,
        item_deltas: vec![],
        tags: 0,
        roles: 0,
    };
    let row = |n: u8, participants: Vec<ParticipantDelta>| TxActions {
        tx_hash: vec![0xf0 + n; 32],
        block_hash: vec![0xd0 + n; 32],
        block_number: 200 + i64::from(n),
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        recognized_type_codes: vec![],
        participants,
    };
    let mut deposit = row(1, vec![party(0, -1_000)]);
    deposit.protocol_actions = vec![ProtocolAction::new("dao", "deposit", serde_json::json!({}))];
    let shared = row(2, vec![party(0, -600), party(1, -400)]);

    let store = test_store();
    for actions in [&deposit, &shared] {
        seed_named_tx(&store, actions.block_number, &actions.tx_hash, actions);
    }
    let app = create_router(test_config(store)).await;
    let own =
        |actions: &TxActions| format!("/transactions/0x{}/activity", hex::encode(&actions.tx_hash));
    let (_, json) = get_json(&app, &own(&deposit)).await;
    assert_eq!(
        json["feePayer"],
        format!("0x{}", hex::encode([0x20; 32])),
        "{json}"
    );
    let (_, json) = get_json(&app, &own(&shared)).await;
    assert_eq!(json["feePayer"], serde_json::Value::Null, "{json}");
}

// ── Lane feed, part 4 (the owner's "做", 2026-11-20): a row states its items once and draws no
//    party for moving one ───────────────────────────────────────────────────────────────────

/// How many parties the item-mover row holds, and how many of them moved token T.
const MOVER_PARTIES: usize = 600;
const T_MOVERS: usize = 550;

/// A party of the item-mover row: its lock hash names it by `i` in its first two bytes.
fn mover_lock(i: usize) -> [u8; 32] {
    let mut hash = [0x3c; 32];
    hash[0] = u8::try_from(i >> 8).unwrap();
    hash[1] = u8::try_from(i & 0xff).unwrap();
    hash
}

/// The item-mover row: 600 parties. 0..275 send 10 of token T and 275..550 receive 10 (a transfer
/// among 550 parties); 560 receives 100 of M that no one gave (a mint); 561 sends 30 of B and 562
/// receives 10 (a burn of 20, 30 moved out); 570 sends 5 of T2 from one cell and takes 3 back in
/// another (−2 net) and 571 receives 2 (a transfer of 2, the 3 it paid itself changing no holder);
/// the five largest CKB changes are 595 to 599; every other party changed a few shannons.
fn mover_actions(tx_hash: &[u8]) -> TxActions {
    use ckbadger_store::types::{ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TAG_TOKEN};
    let token = |id: u8, magnitude: u128, negative: bool| ItemDelta {
        item_id: vec![id; 32],
        kind: ItemKind::Token,
        magnitude,
        negative,
    };
    let participants = (0..MOVER_PARTIES)
        .map(|i| {
            let item_deltas = match i {
                0..=274 => vec![token(0x71, 10, true)],
                275..=549 => vec![token(0x71, 10, false)],
                560 => vec![token(0x72, 100, false)],
                561 => vec![token(0x73, 30, true)],
                562 => vec![token(0x73, 10, false)],
                570 => vec![token(0x74, 5, true), token(0x74, 3, false)],
                571 => vec![token(0x74, 2, false)],
                _ => vec![],
            };
            ParticipantDelta {
                id: ParticipantId::Lock(mover_lock(i)),
                ckb_delta: if i >= 595 {
                    -1_000_000_000_000 - i128::try_from(i).unwrap()
                } else {
                    100 + i128::try_from(i).unwrap()
                },
                used_delta: 0,
                tags: if item_deltas.is_empty() { 0 } else { TAG_TOKEN },
                item_deltas,
                roles: 0,
            }
        })
        .collect();
    TxActions {
        tx_hash: tx_hash.to_vec(),
        block_hash: vec![0xb8; 32],
        block_number: 11,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        recognized_type_codes: vec![],
        participants,
    }
}

/// Part 4 of lane feed: on Godwoken rows hundreds of parties' items move, and a row drew every
/// one of them in full (the live token page: 2,054 parties drawn for 50 rows, about 1.4 MB). A row
/// now draws the five largest CKB changes and the fee payer, nothing else, and states the
/// transaction's items once, each computed from every party: what it is, its movement, the token
/// amount moved (the larger of what holders received and what they gave, each party's change
/// netted first) and how many parties' holdings of it changed. The transaction's own activity
/// still draws every party; the address feed states the same items.
#[tokio::test]
async fn a_row_states_its_items_once_and_draws_no_party_for_moving_one() {
    let store = test_store();
    let tx_hash = vec![0xa9; 32];
    let actions = mover_actions(&tx_hash);
    seed_named_tx(&store, 11, &tx_hash, &actions);
    let focus = mover_lock(300);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(&focus, 11, 0, &AddrTxValue::new(400, true, true, 0));
    batch.commit().unwrap();
    sync_address_tx_counters(&store, &focus);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    let drawn: Vec<String> = row["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["lockHash"].as_str().unwrap().to_string())
        .collect();
    let largest: Vec<String> = (595..600)
        .map(|i| format!("0x{}", hex::encode(mover_lock(i))))
        .collect();
    assert_eq!(drawn, largest, "the five largest CKB changes alone");

    let token = |id: u8, movement: &str, amount: &str, parties: usize| {
        serde_json::json!({
            "kind": "token",
            "typeScriptHash": format!("0x{}", hex::encode([id; 32])),
            "symbol": null,
            "decimals": null,
            "movement": movement,
            "amount": amount,
            "parties": parties,
        })
    };
    let items = serde_json::json!([
        token(0x71, "transfer", "2750", T_MOVERS),
        token(0x72, "mint", "100", 1),
        token(0x73, "burn", "30", 2),
        token(0x74, "transfer", "2", 2),
    ]);
    assert_eq!(row["items"], items, "the row's items, once each");

    let row_bytes = serde_json::to_string(row).unwrap().len();
    assert!(
        row_bytes <= 52 * MOVER_PARTIES + 8 * 1024,
        "the row takes {row_bytes} bytes for {MOVER_PARTIES} parties"
    );

    let own_uri = format!("/transactions/0x{}/activity", hex::encode(&tx_hash));
    let (_, own) = get_json(&app, &own_uri).await;
    assert_eq!(own["participants"].as_array().unwrap().len(), MOVER_PARTIES);
    assert_eq!(own["items"], items);

    let feed_uri = format!("/addresses/0x{}/activities", hex::encode(focus));
    let (status, feed) = get_json(&app, &feed_uri).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(feed["data"][0]["items"], items);
}
