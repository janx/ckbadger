mod common;
use common::*;

#[tokio::test]
async fn test_get_cell_returns_occupied_capacity_breakdown() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];
    let output_index = 1i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: Some(vec![0x44; 32]),
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0xaa, 0xbb]),
            data_size: 42,
            occupied_capacity: 138_00000000,
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
            "/api/v1/cells/0x{}/{}",
            hex::encode(&tx_hash),
            output_index
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json["commonKnowledgeSize"],
        serde_json::Value::from(138_00000000i64)
    );
    assert_eq!(json["type"]["args"], serde_json::Value::from("0xaabb"));
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["capacityFieldBytes"],
        serde_json::Value::from(8)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["lockScriptBytes"],
        serde_json::Value::from(53)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["typeScriptBytes"],
        serde_json::Value::from(35)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["dataBytes"],
        serde_json::Value::from(42)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["totalBytes"],
        serde_json::Value::from(138)
    );
}

#[tokio::test]
async fn test_dead_cell_exposes_consumer_metadata_in_cell_and_graph() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];
    let output_index = 0i16;
    let consumed_by_tx = vec![0xcd; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_consumed_cell_with_consumer(
        &tx_hash,
        output_index,
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
        456,
        Some(&consumed_by_tx),
    );
    // graph route requires creating tx location to exist
    batch.put_tx_hash_map(&tx_hash, 123, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
    let consumed_by_tx_hex = format!("0x{}", hex::encode(&consumed_by_tx));

    let cell_request = Request::builder()
        .uri(format!("/api/v1/cells/{}/{}", tx_hash_hex, output_index))
        .body(Body::empty())
        .unwrap();
    let cell_response = app.clone().oneshot(cell_request).await.unwrap();
    assert_eq!(cell_response.status(), StatusCode::OK);
    let cell_body = cell_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let cell_json: serde_json::Value = serde_json::from_slice(&cell_body).unwrap();
    assert_eq!(cell_json["status"], "dead");
    assert_eq!(cell_json["consumedAtBlock"], serde_json::Value::from(456));
    assert_eq!(
        cell_json["consumedByTx"],
        serde_json::Value::from(consumed_by_tx_hex.clone())
    );

    let graph_request = Request::builder()
        .uri(format!(
            "/api/v1/graph/cell/{}/{}?depth=1",
            tx_hash_hex, output_index
        ))
        .body(Body::empty())
        .unwrap();
    let graph_response = app.oneshot(graph_request).await.unwrap();
    assert_eq!(graph_response.status(), StatusCode::OK);
    let graph_body = graph_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let graph_json: serde_json::Value = serde_json::from_slice(&graph_body).unwrap();

    let links = graph_json["links"].as_array().unwrap();
    assert!(links.iter().any(|link| {
        link["linkType"] == "consumed_by" && link["source"] == format!("cell-{}-{}", tx_hash_hex, 0)
    }));

    let nodes = graph_json["nodes"].as_array().unwrap();
    assert!(nodes
        .iter()
        .any(|node| node["data"]["hash"] == consumed_by_tx_hex));
}

// ---------------------------------------------------------------------------
// cells/by-script: per-form indexes and script_kind=both pagination
// ---------------------------------------------------------------------------

/// Insert a live cell plus its cell-by-code index rows, mirroring the indexer
/// write path so `cells/by-script` reads the same key shape production writes.
#[allow(clippy::too_many_arguments)]
fn insert_by_script_cell(
    store: &Arc<CkbadgerStore>,
    tx_hash: &[u8],
    created_at_block: i64,
    lock_code_hash: &[u8],
    lock_hash_type: i16,
    type_code_hash: Option<&[u8]>,
    type_hash_type: Option<i16>,
) {
    let cell = LiveCellInfo {
        capacity: 100_00000000,
        lock_script_hash: vec![tx_hash[0]; 32],
        lock_code_hash: lock_code_hash.to_vec(),
        lock_hash_type,
        lock_args: vec![],
        type_script_hash: type_code_hash.map(|_| vec![tx_hash[0].wrapping_add(1); 32]),
        type_code_hash: type_code_hash.map(|h| h.to_vec()),
        type_hash_type,
        type_args: type_code_hash.map(|_| vec![]),
        data_size: 0,
        occupied_capacity: 61_00000000,
        udt_amount: None,
        data_hash: None,
    };

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(tx_hash, 0, &cell, created_at_block);
    batch.put_cell_by_lock_code(
        lock_code_hash,
        lock_hash_type as u8,
        created_at_block,
        tx_hash,
        0,
    );
    if let (Some(type_code_hash), Some(type_hash_type)) = (type_code_hash, type_hash_type) {
        batch.put_cell_by_type_code(
            type_code_hash,
            type_hash_type as u8,
            created_at_block,
            tx_hash,
            0,
        );
    }
    batch.commit().unwrap();
}

async fn fetch_by_script(
    app: &axum::Router,
    code_hash: &[u8],
    hash_type: &str,
    script_kind: &str,
    limit: usize,
    cursor: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut uri = format!(
        "/api/v1/cells/by-script?code_hash=0x{}&hash_type={}&script_kind={}&limit={}",
        hex::encode(code_hash),
        hash_type,
        script_kind,
        limit
    );
    if let Some(cursor) = cursor {
        uri.push_str(&format!("&cursor={}", cursor));
    }
    let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, json)
}

#[tokio::test]
async fn test_cells_by_script_sparse_form_is_isolated_from_dense_sibling() {
    // Same code_hash bytes used by a dense `type` form (8 live cells) and a
    // sparse `data` form (1 live cell). Each query must read only its own
    // form's index range: rows, total, and cursor semantics all per-form.
    let store = test_store();
    seed_genesis_baseline(&store);

    let code_hash = vec![0x9b; 32];

    for i in 0..8u8 {
        insert_by_script_cell(
            &store,
            &[0x10 + i; 32],
            100 + i as i64,
            &code_hash,
            1,
            None,
            None,
        );
    }
    insert_by_script_cell(&store, &[0xd1; 32], 200, &code_hash, 0, None, None);

    store
        .put_script_reference_info_direct(
            1,
            &code_hash,
            &ScriptReferenceInfo {
                reference_hash: code_hash.clone(),
                hash_type: 1,
                lock_cells_count: 8,
                lock_live_cells_count: 8,
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
                ..Default::default()
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    // Sparse form: one row, one total — the dense sibling never leaks in.
    let (status, json) = fetch_by_script(&app, &code_hash, "data", "lock", 20, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total"], 1);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode([0xd1; 32])));
    assert_eq!(json["hasMore"], false);
    assert!(json["nextCursor"].is_null());

    // Dense form: exact per-form pagination, every row exactly once.
    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let (status, json) =
            fetch_by_script(&app, &code_hash, "type", "lock", 3, cursor.as_deref()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["total"], 8);
        for cell in json["data"].as_array().unwrap() {
            seen.push(cell["txHash"].as_str().unwrap().to_string());
            assert_eq!(cell["matchedScriptKind"], "lock");
        }
        match json["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }
    let expected: Vec<String> = (0..8u8)
        .map(|i| format!("0x{}", hex::encode([0x10 + i; 32])))
        .collect();
    assert_eq!(
        seen, expected,
        "dense form paginates exactly, no duplicates"
    );
}

#[tokio::test]
async fn test_cells_by_script_both_paginates_the_full_lock_type_union() {
    // script_kind=both must enumerate the deduplicated union of the lock-form
    // and type-form cells across pages: lock rows first, then type-only rows,
    // with a phase-composite cursor. The cell matching on both sides appears
    // exactly once, and `total` is omitted because the deduplicated count is
    // not available from the per-form counters.
    let store = test_store();
    seed_genesis_baseline(&store);

    let code_hash = vec![0x9b; 32];
    let other_code_hash = vec![0x33; 32];

    // Lock-form cells.
    let lock_only: Vec<[u8; 32]> = (0..3u8).map(|i| [0x41 + i; 32]).collect();
    for (i, tx_hash) in lock_only.iter().enumerate() {
        insert_by_script_cell(&store, tx_hash, 100 + i as i64, &code_hash, 1, None, None);
    }
    // Cell matching on BOTH sides — must not be emitted twice.
    let both_tx = [0x51; 32];
    insert_by_script_cell(
        &store,
        &both_tx,
        110,
        &code_hash,
        1,
        Some(&code_hash),
        Some(1),
    );
    // Type-only cells (different lock code hash).
    let type_only: Vec<[u8; 32]> = (0..2u8).map(|i| [0x61 + i; 32]).collect();
    for (i, tx_hash) in type_only.iter().enumerate() {
        insert_by_script_cell(
            &store,
            tx_hash,
            120 + i as i64,
            &other_code_hash,
            1,
            Some(&code_hash),
            Some(1),
        );
    }

    store
        .put_script_reference_info_direct(
            1,
            &code_hash,
            &ScriptReferenceInfo {
                reference_hash: code_hash.clone(),
                hash_type: 1,
                lock_cells_count: 4,
                lock_live_cells_count: 4,
                type_cells_count: 3,
                type_live_cells_count: 3,
                ..Default::default()
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let (status, json) =
            fetch_by_script(&app, &code_hash, "type", "both", 2, cursor.as_deref()).await;
        assert_eq!(status, StatusCode::OK, "both-mode cursor must be accepted");
        assert!(
            json.get("total").is_none() || json["total"].is_null(),
            "script_kind=both omits total: {json}"
        );
        for cell in json["data"].as_array().unwrap() {
            seen.push(cell["txHash"].as_str().unwrap().to_string());
        }
        pages += 1;
        assert!(pages < 10, "pagination must terminate");
        match json["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => {
                assert_eq!(json["hasMore"], false);
                break;
            }
        }
    }

    let mut deduped = seen.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(
        deduped.len(),
        seen.len(),
        "no row is returned twice: {seen:?}"
    );

    let mut expected: Vec<String> = lock_only
        .iter()
        .chain(std::iter::once(&both_tx))
        .chain(type_only.iter())
        .map(|tx_hash| format!("0x{}", hex::encode(tx_hash)))
        .collect();
    expected.sort();
    assert_eq!(
        deduped, expected,
        "both mode enumerates the full lock/type union"
    );
}

#[tokio::test]
async fn test_cells_by_script_rejects_cursors_from_another_form() {
    // A cursor is a key inside one (code_hash, hash_type) form. Replaying it
    // against a different form would silently page the wrong range.
    let store = test_store();
    seed_genesis_baseline(&store);

    let code_hash = vec![0x9b; 32];
    for i in 0..3u8 {
        insert_by_script_cell(
            &store,
            &[0x71 + i; 32],
            100 + i as i64,
            &code_hash,
            1,
            None,
            None,
        );
    }
    store
        .put_script_reference_info_direct(
            1,
            &code_hash,
            &ScriptReferenceInfo {
                reference_hash: code_hash.clone(),
                hash_type: 1,
                lock_cells_count: 3,
                lock_live_cells_count: 3,
                ..Default::default()
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let (status, json) = fetch_by_script(&app, &code_hash, "type", "lock", 1, None).await;
    assert_eq!(status, StatusCode::OK);
    let cursor = json["nextCursor"].as_str().unwrap().to_string();

    // Same cursor, different hash_type form.
    let (status, _) = fetch_by_script(&app, &code_hash, "data", "lock", 1, Some(&cursor)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Composite cursor is required in both mode.
    let (status, _) = fetch_by_script(&app, &code_hash, "type", "both", 1, Some(&cursor)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // ...and a phase-composite cursor is accepted.
    let (status, _) = fetch_by_script(
        &app,
        &code_hash,
        "type",
        "both",
        1,
        Some(&format!("lock:{}", cursor)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// R4-E bug 2: /addresses/{lock_hash} must resolve its lock script through
// CF_LOCK_SCRIPTS (`get_lock_script`), the same single path every other handler
// uses. It used to derive the script from one *live* cell, so any fully spent
// address (completed DAO withdrawers, emptied wallets) reported
// `address: null` and no `lockScript` even though the script is stored.
// ---------------------------------------------------------------------------

/// secp256k1_blake160_sighash_all code hash + args with an externally verified
/// mainnet address (see `crates/api/src/utils/address.rs` tests).
fn known_secp_lock() -> (Vec<u8>, Vec<u8>, &'static str) {
    (
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap(),
        hex::decode("b39bbc0b3673c7d36450bc14cfcdad2d559c6c64").unwrap(),
        "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4",
    )
}

#[tokio::test]
async fn test_get_address_resolves_lock_script_with_zero_live_cells() {
    let store = test_store();
    let (code_hash, args, expected_address) = known_secp_lock();
    let lock_hash = compute_script_hash(&code_hash, 1, &args);

    let mut batch = StoreBatch::new(store.as_ref());
    // Address with history but every cell spent: no live cell, lock script stored.
    batch.put_lock_script(
        &lock_hash,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: code_hash.clone(),
            hash_type: 1,
            args: args.clone(),
        },
    );
    batch.put_addr_balance(
        &lock_hash,
        &ckbadger_store::types::AddressBalance {
            balance: 0,
            used_capacity: 0,
            live_cells_count: 0,
            total_cells_count: 3,
            txs_count: 3,
            first_seen_block: 10,
            first_seen_tx: vec![0x01; 32],
            last_activity_block: 90,
            last_activity_tx: vec![0x02; 32],
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/addresses/0x{}", hex::encode(&lock_hash))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["liveCellsCount"], 0);
    assert_eq!(json["transactionsCount"], 3);
    assert_eq!(
        json["address"],
        serde_json::Value::String(expected_address.to_string()),
        "fully spent address must still resolve from CF_LOCK_SCRIPTS, got {}",
        json["address"]
    );
    assert_eq!(
        json["lockScript"]["codeHash"],
        format!("0x{}", hex::encode(&code_hash))
    );
    assert_eq!(json["lockScript"]["hashType"], "type");
    assert_eq!(
        json["lockScript"]["args"],
        format!("0x{}", hex::encode(&args))
    );
}

#[tokio::test]
async fn test_get_address_keeps_exact_hash_type_for_live_address() {
    // Control: an address that still has live cells resolves identically, and the
    // stored hash_type (data1 == 2) is reported exactly, never a canonical guess.
    let store = test_store();
    let code_hash = vec![0x9c; 32];
    let args = vec![0x44; 20];
    let lock_hash = compute_script_hash(&code_hash, 2, &args);
    let expected_address =
        ckbadger_common::script_to_address(&code_hash, 2, &args, "mainnet").unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &lock_hash,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: code_hash.clone(),
            hash_type: 2,
            args: args.clone(),
        },
    );
    batch.put_cell(
        &[0x51; 32],
        0,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: lock_hash.clone(),
            lock_code_hash: code_hash.clone(),
            lock_hash_type: 2,
            lock_args: args.clone(),
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        77,
    );
    batch.put_cell_by_lock(&lock_hash, 77, &[0x51; 32], 0);
    batch.put_addr_balance(
        &lock_hash,
        &ckbadger_store::types::AddressBalance {
            balance: 200_00000000,
            used_capacity: 61_00000000,
            live_cells_count: 1,
            total_cells_count: 1,
            txs_count: 1,
            first_seen_block: 77,
            first_seen_tx: vec![0x51; 32],
            last_activity_block: 77,
            last_activity_tx: vec![0x51; 32],
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/addresses/0x{}", hex::encode(&lock_hash))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["liveCellsCount"], 1);
    assert_eq!(json["balance"], "20000000000");
    assert_eq!(
        json["address"],
        serde_json::Value::String(expected_address),
        "live address must resolve, got {}",
        json["address"]
    );
    assert_eq!(json["lockScript"]["hashType"], "data1");
}

#[tokio::test]
async fn test_get_address_fails_fast_on_invalid_stored_hash_type() {
    // 0/1/2/4 are the only hash_type values CKB consensus allows. Anything else in
    // CF_LOCK_SCRIPTS is a store corruption: report it instead of rendering a guess.
    let store = test_store();
    let code_hash = vec![0x8d; 32];
    let args = vec![0x66; 20];
    let lock_hash = vec![0xBE; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &lock_hash,
        &ckbadger_store::types::LockScriptEntry {
            code_hash,
            hash_type: 3,
            args,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/addresses/0x{}", hex::encode(&lock_hash))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&hex::encode(&lock_hash)) && message.contains("hash_type"),
        "error must name the lock hash and the bad hash_type, got {message}"
    );
}

// ---------------------------------------------------------------------------
// R4-G item 2: the `/cells/{tx_hash}/{index}` detail handler carried the same
// class of silent guard 288730bb removed from `get_address` in this module — a
// local `_ => "data"` hash_type fallback, `type_hash_type.unwrap_or(1)`, and
// `script_to_address(...).ok()`. Each rendered a plausible guess over corrupt
// stored state instead of reporting it.
// ---------------------------------------------------------------------------

/// A live cell whose fields the individual tests perturb.
fn cell_with(lock_hash_type: i16, type_hash_type: Option<i16>) -> LiveCellInfo {
    LiveCellInfo {
        capacity: 100_00000000,
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: vec![0x22; 32],
        lock_hash_type,
        lock_args: vec![0x33; 20],
        type_script_hash: Some(vec![0x44; 32]),
        type_code_hash: Some(vec![0x55; 32]),
        type_hash_type,
        type_args: Some(vec![0xaa, 0xbb]),
        data_size: 42,
        occupied_capacity: 138_00000000,
        udt_amount: None,
        data_hash: None,
    }
}

async fn get_cell_json(tx_hash: &[u8], cell: LiveCellInfo) -> (StatusCode, serde_json::Value) {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(tx_hash, 1, &cell, 123);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    get_json(&app, &format!("/cells/0x{}/1", hex::encode(tx_hash))).await
}

#[tokio::test]
async fn test_get_cell_fails_fast_on_invalid_stored_lock_hash_type() {
    // 0/1/2/4 are the only hash_type values CKB consensus allows; 3 in the store
    // is corruption. It used to render as "data" — a valid-looking lie.
    let tx_hash = vec![0xc1; 32];
    let (status, json) = get_cell_json(&tx_hash, cell_with(3, Some(1))).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&hex::encode(&tx_hash)) && message.contains("hash_type"),
        "error must name the outpoint and the bad hash_type, got {message}"
    );
}

#[tokio::test]
async fn test_get_cell_fails_fast_on_invalid_stored_type_hash_type() {
    let tx_hash = vec![0xc2; 32];
    let (status, json) = get_cell_json(&tx_hash, cell_with(1, Some(3))).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&hex::encode(&tx_hash)) && message.contains("hash_type"),
        "error must name the outpoint and the bad hash_type, got {message}"
    );
}

#[tokio::test]
async fn test_get_cell_fails_fast_on_missing_type_hash_type() {
    // A cell with a type script always has a hash_type on chain. `unwrap_or(1)`
    // silently rendered "type" for a cell the indexer stored incompletely.
    let tx_hash = vec![0xc3; 32];
    let (status, json) = get_cell_json(&tx_hash, cell_with(1, None)).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&hex::encode(&tx_hash)) && message.contains("hash_type"),
        "error must name the outpoint and the missing hash_type, got {message}"
    );
}

#[tokio::test]
async fn test_get_cell_fails_fast_when_address_cannot_be_encoded() {
    // `script_to_address(...).ok()` turned an unencodable lock into `address:
    // null`, indistinguishable from a cell that legitimately has no address —
    // there is no such cell.
    let tx_hash = vec![0xc4; 32];
    let mut cell = cell_with(1, Some(1));
    cell.lock_code_hash = vec![0x22; 31]; // RFC-0021 requires exactly 32 bytes

    let (status, json) = get_cell_json(&tx_hash, cell).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "got {json}");
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&hex::encode(&tx_hash)) && message.contains("address"),
        "error must name the outpoint and the encoding failure, got {message}"
    );
}

#[tokio::test]
async fn test_get_cell_keeps_exact_hash_types() {
    // Control (passes on both revisions): every valid hash_type renders as
    // itself, never collapsed to the "data" fallback.
    let tx_hash = vec![0xc5; 32];
    let (status, json) = get_cell_json(&tx_hash, cell_with(2, Some(4))).await;

    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["lock"]["hashType"], "data1");
    assert_eq!(json["type"]["hashType"], "data2");
    assert!(
        json["address"]
            .as_str()
            .unwrap_or_default()
            .starts_with("ckb1"),
        "a live cell always has an encodable address, got {}",
        json["address"]
    );
}
