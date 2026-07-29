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
