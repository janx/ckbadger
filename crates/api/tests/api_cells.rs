mod common;
use common::*;

#[tokio::test]
async fn test_live_cell_summary_returns_exact_constant_size_projection() {
    let store = test_store();
    let summary = ckbadger_store::LiveCellSummary {
        tip_block_number: 12_345_678,
        tip_block_hash: [0xAB; 32],
        dao: 182_341,
        typed_non_dao: 2_639_044,
        plain: 5_600_552,
        data_bearing: 2_810_388,
    };
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_live_cell_summary_snapshots(&[summary]).unwrap();
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/cells/live-summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "tip": {
                "block": 12_345_678,
                "hash": format!("0x{}", "ab".repeat(32)),
            },
            "liveCells": 8_421_937,
            "classes": {
                "dao": 182_341,
                "typedNonDao": 2_639_044,
                "plain": 5_600_552,
            },
            "dataBearing": 2_810_388,
        })
    );
}

#[tokio::test]
async fn test_live_cell_summary_is_503_until_indexer_publishes_snapshot() {
    let store = test_store();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/cells/live-summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "initializing");
}

#[tokio::test]
async fn test_live_cell_summary_corruption_fails_instead_of_returning_defaults() {
    let store = test_store();
    store
        .put_cf(
            store.cf_sync_meta(),
            ckbadger_store::keys::sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT,
            b"corrupt",
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/cells/live-summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"].as_str().unwrap().contains("value length"));
}

#[tokio::test]
async fn test_live_cell_summary_missing_after_initialization_is_not_masked_as_warmup() {
    let store = test_store();
    let summary = ckbadger_store::LiveCellSummary {
        tip_block_number: 7,
        tip_block_hash: [0x07; 32],
        dao: 1,
        typed_non_dao: 1,
        plain: 1,
        data_bearing: 1,
    };
    let mut seed = StoreBatch::new(store.as_ref());
    seed.put_live_cell_summary_snapshots(&[summary]).unwrap();
    seed.commit().unwrap();
    store
        .set_sync_status(&ckbadger_store::SyncStatus {
            tip_block_number: 7,
            tip_block_hash: summary.tip_block_hash.to_vec(),
            total_cells_created: 3,
            ..Default::default()
        })
        .unwrap();
    let mut corrupt = StoreBatch::new(store.as_ref());
    corrupt.delete_live_cell_summary_current().unwrap();
    corrupt.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/cells/live-summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing outside reconciliation"));
}

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

/// `.cell` Cells Account type script, mainnet (docs/metadata/scripts/dotcell-account.toml;
/// `ACCOUNT_TYPE_CODE_HASH_MAINNET` in the indexer's dotcell fixtures).
const DOTCELL_ACCOUNT_TYPE_MAINNET: &str =
    "d96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54";
/// `.cell` Cells Account Lock, mainnet (`ACCOUNT_LOCK_CODE_HASH_MAINNET`).
const DOTCELL_ACCOUNT_LOCK_MAINNET: &str =
    "9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab";
/// The mainnet namespace args every name cell's type script carries
/// (`NAMESPACE_ARGS_MAINNET`).
const DOTCELL_NAMESPACE_ARGS_MAINNET: &str = "b4f4302965b7d6421481a520ee7eb5971a5e808c";

fn dotcell_name_cell_info(data_size: i32) -> LiveCellInfo {
    let lock_code_hash = hex::decode(DOTCELL_ACCOUNT_LOCK_MAINNET).unwrap();
    let type_code_hash = hex::decode(DOTCELL_ACCOUNT_TYPE_MAINNET).unwrap();
    let type_args = hex::decode(DOTCELL_NAMESPACE_ARGS_MAINNET).unwrap();
    LiveCellInfo {
        capacity: 240_00000000,
        lock_script_hash: compute_script_hash(&lock_code_hash, 1, &[]),
        lock_code_hash,
        lock_hash_type: 1,
        lock_args: vec![],
        type_script_hash: Some(compute_script_hash(&type_code_hash, 1, &type_args)),
        type_code_hash: Some(type_code_hash),
        type_hash_type: Some(1),
        type_args: Some(type_args),
        data_size,
        occupied_capacity: 0,
        udt_amount: None,
        data_hash: None,
    }
}

/// The cell detail names each script's registry protocol by slug, so the
/// frontend never compares code hashes to recognise `.cell` / did:ckb cells.
#[tokio::test]
async fn test_cell_detail_exposes_registry_protocol_slugs() {
    let store = test_store();
    let tx_hash = vec![0xc1; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(&tx_hash, 0, &dotcell_name_cell_info(105), 123);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, &format!("/cells/0x{}/0", hex::encode(&tx_hash))).await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["protocolScript"]["lock"], "dotcell-account-lock");
    assert_eq!(json["protocolScript"]["type"], "dotcell-account");
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

// ---------------------------------------------------------------------------
// Audited bugs (2026-08-01 night, agent E `format_probe_mainnet.txt`):
// `/addresses/{addr}` accepted RFC-invalid full addresses carrying the legacy
// Bech32 checksum (RFC-0021 mandates Bech32m for the 0x00 format), served full
// stats, and echoed the invalid input string back as `address`; all-uppercase
// bech32m (legal per spec case rules) fell into the hex-hash branch and got a
// misleading 400. The response `address` must always be the canonical
// lowercase encoding of the lock script on the serving network — never an
// input echo.
// ---------------------------------------------------------------------------

/// Mainnet burn lock (secp sighash, hash_type `type`, args = 20 zero bytes):
/// canonical bech32m form, the same payload under the WRONG legacy bech32
/// checksum, and the same payload under the testnet HRP (all audit vectors).
const BURN_BECH32M: &str = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq5m759c";
const BURN_WRONG_BECH32_CHECKSUM: &str = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqp8wcq6";
const BURN_BECH32M_CKT: &str = "ckt1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6f4m0q";

/// Seed the burn lock's script + balance so the endpoint has real stats to
/// serve (the audited live probe reported balance 35,417,100,000,000 and 11
/// live cells; values mirrored here so a wrongly accepted request would
/// observably return them).
fn seed_burn_lock(store: &Arc<CkbadgerStore>) -> Vec<u8> {
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = vec![0u8; 20];
    let lock_hash = compute_script_hash(&code_hash, 1, &args);

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &lock_hash,
        &ckbadger_store::types::LockScriptEntry {
            code_hash,
            hash_type: 1,
            args,
        },
    );
    batch.put_addr_balance(
        &lock_hash,
        &ckbadger_store::types::AddressBalance {
            balance: 35_417_100_000_000,
            used_capacity: 1_100_000_000_000,
            live_cells_count: 11,
            total_cells_count: 20,
            txs_count: 9,
            first_seen_block: 1,
            first_seen_tx: vec![0x01; 32],
            last_activity_block: 5,
            last_activity_tx: vec![0x02; 32],
        },
    );
    batch.commit().unwrap();
    lock_hash
}

#[tokio::test]
async fn test_get_address_rejects_full_address_with_bech32_checksum() {
    let store = test_store();
    seed_burn_lock(&store);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, &format!("/addresses/{BURN_WRONG_BECH32_CHECKSUM}")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an RFC-invalid checksum must be rejected, not served (the audited bug answered 200 with full stats), got {json}"
    );
    let message = json["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("bech32m"),
        "error must name the checksum requirement, got: {message}"
    );
}

#[tokio::test]
async fn test_get_address_uppercase_equals_lowercase_and_canonicalizes() {
    let store = test_store();
    let lock_hash = seed_burn_lock(&store);
    let app = create_router(test_config(store)).await;

    let (status_lower, lower) = get_json(&app, &format!("/addresses/{BURN_BECH32M}")).await;
    assert_eq!(status_lower, StatusCode::OK, "got {lower}");
    assert_eq!(lower["address"], BURN_BECH32M);
    assert_eq!(lower["balance"], "35417100000000");
    assert_eq!(lower["liveCellsCount"], 11);
    assert_eq!(lower["transactionsCount"], 9);
    assert_eq!(
        lower["lockScriptHash"],
        format!("0x{}", hex::encode(&lock_hash))
    );

    let (status_upper, upper) =
        get_json(&app, &format!("/addresses/{}", BURN_BECH32M.to_uppercase())).await;
    assert_eq!(
        status_upper,
        StatusCode::OK,
        "all-uppercase bech32m is a legal encoding and must resolve, got {upper}"
    );
    assert_eq!(
        upper, lower,
        "uppercase input must yield the identical response, canonical lowercase `address` included"
    );
}

#[tokio::test]
async fn test_get_address_returns_canonical_network_encoding_not_input_echo() {
    // A valid bech32m encoding under the testnet HRP resolves the same lock
    // hash (the HRP is not part of the script), but the response `address`
    // must be the canonical encoding for the SERVING network (mainnet here),
    // never the raw input echoed back.
    let store = test_store();
    seed_burn_lock(&store);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, &format!("/addresses/{BURN_BECH32M_CKT}")).await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["balance"], "35417100000000");
    assert_eq!(
        json["address"], BURN_BECH32M,
        "`address` is the canonical lock-script encoding on the serving network, not an input echo"
    );
}

#[tokio::test]
async fn test_get_address_unindexed_uppercase_reports_canonical_form() {
    // Never-indexed address: no stored lock script, but the input itself
    // decodes to the lock script, so the canonical lowercase form is still
    // reported (with honest zero stats and no guessed `lockScript`).
    let store = test_store();
    let app = create_router(test_config(store)).await;

    let (status, json) =
        get_json(&app, &format!("/addresses/{}", BURN_BECH32M.to_uppercase())).await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["balance"], "0");
    assert_eq!(json["liveCellsCount"], 0);
    assert_eq!(
        json["address"], BURN_BECH32M,
        "canonical form comes from the decoded lock script even with no stored entry"
    );
    assert_eq!(json["lockScript"], serde_json::Value::Null);
}

/// The address page must report the lock script's deprecated flag from the
/// store, not a hardcoded `false`. Vector: the old testnet Anyone-Can-Pay
/// deployment 0x86a1c698... is marked `deprecated = true` in
/// docs/metadata/scripts/anyone-can-pay-lock.toml, which label import persists
/// on the reference's ScriptInfo row.
#[tokio::test]
async fn test_get_address_lock_script_info_reports_deprecated_version() {
    let store = test_store();
    let code_hash =
        hex::decode("86a1c6987a4acbe1a887cca4c9dd2ac9fcb07405bbeda51b861b18bbf7492c4b").unwrap();
    let args = vec![0x44; 20];
    let lock_hash = compute_script_hash(&code_hash, 1, &args);

    store
        .put_script_info_direct(
            &code_hash,
            &ckbadger_store::types::ScriptInfo {
                code_hash: code_hash.clone(),
                hash_type: 1,
                name: Some("Anyone-Can-Pay Lock".to_string()),
                deprecated: true,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
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
            balance: 100_00000000,
            used_capacity: 61_00000000,
            live_cells_count: 1,
            total_cells_count: 1,
            txs_count: 1,
            first_seen_block: 10,
            first_seen_tx: vec![0x01; 32],
            last_activity_block: 20,
            last_activity_tx: vec![0x02; 32],
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/addresses/0x{}", hex::encode(&lock_hash))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["lockScriptInfo"]["name"], "Anyone-Can-Pay Lock");
    assert_eq!(
        json["lockScriptInfo"]["deprecated"], true,
        "store marks this script version deprecated; the address response must reflect it"
    );
}

// ---------------------------------------------------------------------------
// Tx-pool rows on the address transaction list
// ---------------------------------------------------------------------------

const TX_POOL_LOCK_HASH: [u8; 32] = [0x51; 32];

fn seed_pool_address_balance(store: &Arc<CkbadgerStore>, lock_hash: &[u8], txs_count: i64) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_balance(
        lock_hash,
        &ckbadger_store::types::AddressBalance {
            balance: 10_000_000_000,
            used_capacity: 6_100_000_000,
            live_cells_count: 1,
            total_cells_count: 1,
            txs_count,
            first_seen_block: 1,
            first_seen_tx: vec![0x01; 32],
            last_activity_block: 10,
            last_activity_tx: vec![0x02; 32],
        },
    );
    batch.commit().unwrap();
}

#[tokio::test]
async fn test_address_transactions_page_one_puts_pool_rows_above_committed_rows() {
    use ckbadger_store::types::semantic_tags;

    let store = test_store();
    seed_committed_activity(&store, &TX_POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);
    seed_pool_address_balance(&store, &TX_POOL_LOCK_HASH, 1);

    let state = test_app_state(test_config(store));
    let mut record = make_test_pool_record(
        &[0xf1; 32],
        &TX_POOL_LOCK_HASH,
        -500,
        1_700_000_500_000,
        ckbadger_api::pool::PoolStatus::Pending,
    );
    record.semantic_tags = semantic_tags::DAO;
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/transactions",
                    hex::encode(TX_POOL_LOCK_HASH)
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
    assert_eq!(pool_row["timestamp"], serde_json::Value::Null);
    assert_eq!(pool_row["poolStatus"], "pending");
    assert!(pool_row["timeAddedToPool"].as_str().is_some());
    // capacityChange and txType come from the same AddrTxValue constructor the
    // indexer uses for committed rows.
    assert_eq!(pool_row["capacityChange"], "-500");
    assert_eq!(pool_row["txType"], "sent");
    // fee / size / cycles are the node's own pool-entry values.
    assert_eq!(pool_row["fee"], "1000");
    assert_eq!(pool_row["txSize"], 500);
    assert_eq!(pool_row["cycles"], 200000);
    // scriptLabels come from the same semantic-tag derivation the tx_index
    // writer uses.
    assert_eq!(pool_row["scriptLabels"][0], "NervosDAO");

    assert_eq!(rows[1]["blockNumber"], 10);
    assert_eq!(
        json["total"], 1,
        "total stays the committed count; pool rows are reported beside it"
    );
    assert_eq!(json["pool"]["count"], 1);
    assert_eq!(json["pool"]["pendingCkbDelta"], "-500");
    assert_eq!(json["pool"]["healthy"], true);
}

#[tokio::test]
async fn test_address_transactions_cursor_page_contains_no_pool_rows() {
    let store = test_store();
    seed_committed_activity(&store, &TX_POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);
    seed_pool_address_balance(&store, &TX_POOL_LOCK_HASH, 1);

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &TX_POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/transactions?cursor=999:0",
                    hex::encode(TX_POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["poolStatus"].is_null()),
        "cursor pages carry committed rows only: {json:?}"
    );
    assert!(json["pool"].is_null());
}

#[tokio::test]
async fn test_address_transactions_omit_pool_rows_the_store_already_has() {
    let store = test_store();
    let tx_hash = [0xc1; 32];
    seed_committed_activity(&store, &TX_POOL_LOCK_HASH, &tx_hash, 10, 0, 100, 0);
    seed_pool_address_balance(&store, &TX_POOL_LOCK_HASH, 1);

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &tx_hash,
            &TX_POOL_LOCK_HASH,
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
                    "/api/v1/addresses/0x{}/transactions",
                    hex::encode(TX_POOL_LOCK_HASH)
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

// ── Phase 1a: protocol-named participants ────────────────────────────────

#[tokio::test]
async fn test_address_transactions_count_includes_prefix_participations() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_balance(
        &lock_hash,
        &ckbadger_store::types::AddressBalance {
            txs_count: 3,
            ..Default::default()
        },
    );
    batch.put_addr_prefix_stats(
        &lock_hash[..20],
        &ckbadger_store::types::AddrPrefixStats { txs_count: 2 },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/addresses/0x{}", hex::encode(lock_hash)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["transactionsCount"], 5, "{json:?}");
}

#[tokio::test]
async fn test_address_transactions_list_merges_prefix_rows_with_named_tx_type() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let named_tx = [0xf7u8; 32];

    // A committed cell participation at block 10.
    seed_committed_activity(&store, &lock_hash, &[0xc1; 32], 10, 0, 100, 0);
    // A protocol-named participation at block 11, in the prefix index only.
    let mut batch = StoreBatch::new(store.as_ref());
    // `seed_committed_activity` writes no addr_balance; the cell participation
    // it seeded is counted here so `total` can be checked as a real sum.
    batch.put_addr_balance(
        &lock_hash,
        &ckbadger_store::types::AddressBalance {
            txs_count: 1,
            ..Default::default()
        },
    );
    batch.put_tx_hash_map(&named_tx, 11, 0);
    batch.put_tx_index(
        11,
        0,
        &ckbadger_store::types::TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_011,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 150,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        11,
        &ckbadger_store::CachedBlockHeader {
            hash: vec![0xbc; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_011,
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
    batch.put_addr_tx_by_prefix(
        &lock_hash[..20],
        11,
        0,
        &named_tx,
        &ckbadger_store::types::AddrTxValue::new(
            0,
            false,
            false,
            ckbadger_store::types::TAG_IDENTITY,
        ),
    );
    batch.put_addr_prefix_stats(
        &lock_hash[..20],
        &ckbadger_store::types::AddrPrefixStats { txs_count: 1 },
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|s| s.tip_block_number = 11)
        .unwrap();

    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/transactions",
                    hex::encode(lock_hash)
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
    assert_eq!(rows.len(), 2, "{json:?}");
    assert_eq!(rows[0]["blockNumber"], 11, "descending by block");
    assert_eq!(rows[0]["txType"], "named");
    assert_eq!(rows[0]["capacityChange"], "0");
    assert_eq!(rows[1]["blockNumber"], 10);
    assert_eq!(json["total"], 2, "cell + named participations");
}

#[tokio::test]
async fn test_prefix_transactions_endpoint_lists_named_participations() {
    let store = test_store();
    let prefix = [0x99u8; 20];
    let mut batch = StoreBatch::new(store.as_ref());
    for (block, tx_byte) in [(10i64, 0xa1u8), (11, 0xa2)] {
        batch.put_addr_tx_by_prefix(
            &prefix,
            block,
            0,
            &[tx_byte; 32],
            &ckbadger_store::types::AddrTxValue::new(
                0,
                false,
                false,
                ckbadger_store::types::TAG_IDENTITY,
            ),
        );
    }
    // Another prefix's row must not leak into this one's page.
    batch.put_addr_tx_by_prefix(
        &[0x11u8; 20],
        12,
        0,
        &[0xa3; 32],
        &ckbadger_store::types::AddrTxValue::new(0, false, false, 0),
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/prefix/0x{}/transactions",
                    hex::encode(prefix)
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
    assert_eq!(rows.len(), 2, "{json:?}");
    assert_eq!(rows[0]["blockNumber"], 11, "descending by block");
    assert_eq!(rows[0]["txType"], "named");
    assert_eq!(rows[0]["capacityChange"], "0");
    assert_eq!(rows[1]["blockNumber"], 10);

    // A prefix of the wrong width is a 400, never a widened scan.
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/addresses/prefix/0x9999/transactions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// `.cell` names an address owns are found by prefix-seeking its own lock hash.
#[tokio::test]
async fn test_address_dotcell_names_lists_names_owned_by_prefix() {
    let store = test_store();
    let lock_hash: [u8; 32] =
        hex::decode("57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d")
            .unwrap()
            .try_into()
            .unwrap();
    let owner20: [u8; 20] = lock_hash[..20].try_into().unwrap();
    let support_id: [u8; 20] = hex::decode("62d71147ac82b83c8531126cacb0d2f072bfd94a")
        .unwrap()
        .try_into()
        .unwrap();
    let other_id: [u8; 20] = hex::decode("a8d5f7507b9f3d30090253a741c1c80cb0cb121c")
        .unwrap()
        .try_into()
        .unwrap();

    {
        let mut batch = StoreBatch::new(store.as_ref());
        for (id, label, owner) in [
            (support_id, "support", owner20),
            (other_id, "abuse", [0xAAu8; 20]),
        ] {
            batch.put_identity(
                &id,
                &IdentityEntry {
                    standard: IdentityStandard::DotCell,
                    owner_lock_hash: None,
                    name: Some(format!("{label}.cell")),
                    is_live: true,
                    created_at_block: 20_518_306,
                    created_at_tx: vec![0xD1; 32],
                    extra: IdentityExtra::DotCell {
                        label: label.to_string(),
                        namespace_args: [0xb4; 20],
                        layout_version: 3,
                        expired_at: 1_821_507_678,
                        owner_hash20: owner,
                        manager_hash20: owner,
                        next_id: [0x65; 20],
                        records_hash: [0x72; 32],
                        records: Vec::new(),
                        parent_id: None,
                    },
                },
            );
            batch.put_dotcell_name_by_owner(&owner, &id);
        }
        batch.commit().unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/dotcell-names",
                    hex::encode(lock_hash)
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
    assert_eq!(rows.len(), 1, "only this address's names: {json}");
    assert_eq!(rows[0]["label"], "support");
    assert_eq!(rows[0]["name"], "support.cell");
    assert_eq!(
        rows[0]["identityId"],
        "0x62d71147ac82b83c8531126cacb0d2f072bfd94a"
    );
    assert_eq!(rows[0]["expiredAt"], 1_821_507_678u64);
}
