mod common;
use common::*;

fn cell(data_size: i32) -> LiveCellInfo {
    let occupied = i64::from(8 + 33 + data_size) * 100_000_000;
    LiveCellInfo {
        capacity: 20_000_000_000,
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: vec![0x22; 32],
        lock_hash_type: 1,
        lock_args: vec![],
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size,
        occupied_capacity: occupied,
        udt_amount: None,
        data_hash: None,
    }
}

fn seed(store: &CkbadgerStore, missing_output: bool, malformed: bool) {
    seed_block_headers_at(store, &[(0, 1000), (1, 2000), (2, 3000)]);
    let mut batch = StoreBatch::new(store);
    for block in 0..=2 {
        let tx = vec![block as u8 + 0x41; 32];
        batch.put_tx_hash_map(&tx, block, 0);
        batch.put_tx_index(
            block,
            0,
            &TxIndexEntry {
                tx_hash: tx.as_slice().try_into().unwrap(),
                is_cellbase: block == 0,
                timestamp: 1000 + block * 1000,
                inputs_count: if block == 0 { 0 } else { 1 },
                outputs_count: 1,
                fee: 0,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        if block == 0 {
            batch.put_consumed_cell_with_consumer(&tx, 0, &cell(0), 0, 1, Some(&[0x42; 32]));
        } else if block != 2 || !missing_output {
            let mut info = cell(10);
            if block == 2 && malformed {
                info.occupied_capacity += 100_000_000;
            }
            batch.put_cell(&tx, 0, &info, block);
        }
    }
    batch.commit().unwrap();
}

#[tokio::test]
async fn snapshot_reports_exact_cells_and_proven_tracked_fates() {
    let store = test_store();
    seed(&store, false, false);
    let app = create_router(test_config(store.clone())).await;
    let consumed = format!("0x{}-0", "41".repeat(32));
    let orphan = format!("0x{}-0", "ff".repeat(32));
    let (status, json) = get_json(
        &app,
        &format!("/vessels/snapshot?tracked={consumed},{orphan}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["anchorBlock"], 2);
    assert_eq!(json["anchorHash"], format!("0x{}", "02".repeat(32)));
    assert_eq!(json["firstSampledBlock"], 0);
    assert_eq!(json["examinedOutputs"], 3);
    assert_eq!(json["cells"].as_array().unwrap().len(), 2);
    assert_eq!(json["cells"][0]["capacityShannons"], "20000000000");
    assert_eq!(json["cells"][0]["occupiedShannons"], "5100000000");
    assert_eq!(json["cells"][0]["anatomy"]["totalBytes"], 51);
    assert_eq!(json["tracked"][0]["status"], "consumed");
    assert_eq!(
        json["tracked"][0]["consumingTxHash"],
        format!("0x{}", "42".repeat(32))
    );
    assert_eq!(json["tracked"][1]["status"], "notCanonical");
    if let Ok(dir) = std::env::var("D8_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/vessels-snapshot.json"),
            serde_json::to_vec_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn snapshot_fails_on_missing_canonical_output_or_bad_anatomy() {
    for (missing, malformed, expected) in [
        (true, false, "lacks exactly one canonical state marker"),
        (false, true, "invalid vessel cell anatomy"),
    ] {
        let store = test_store();
        seed(&store, missing, malformed);
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/vessels/snapshot").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(json.to_string().contains(expected), "{json}");
    }
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_000)]);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/vessels/snapshot").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json.to_string().contains("missing canonical transaction"),
        "{json}"
    );
    for (invalid, expected) in [
        (
            {
                let mut value = cell(0);
                value.data_size = -1;
                value.occupied_capacity = 4_000_000_000;
                value
            },
            "invalid vessel cell source sizes",
        ),
        (
            {
                let mut value = cell(0);
                value.type_code_hash = Some(vec![0x55; 32]);
                value
            },
            "incomplete vessel type script evidence",
        ),
    ] {
        let store = test_store();
        seed(&store, false, false);
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_cell(&[0x43; 32], 0, &invalid, 2);
        batch.commit().unwrap();
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/vessels/snapshot").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(json.to_string().contains(expected), "{json}");
    }
}

#[tokio::test]
async fn snapshot_rejects_duplicate_and_unbounded_tracking_queries() {
    let app = create_router(test_config(test_store())).await;
    let one = format!("0x{}-0", "aa".repeat(32));
    let (status, json) = get_json(&app, &format!("/vessels/snapshot?tracked={one},{one}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json.to_string().contains("duplicate tracked outpoint"));
    let (status, json) = get_json(
        &app,
        &format!("/vessels/snapshot?tracked={}", vec![one; 65].join(",")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json.to_string().contains("exceeds 64"));
}

#[tokio::test]
async fn snapshot_bounds_full_window_and_keeps_rotated_live_cell_authoritative() {
    let store = test_store();
    let blocks: Vec<_> = (0..12).map(|block| (block, 1_000 + block)).collect();
    seed_block_headers_at(&store, &blocks);
    let mut batch = StoreBatch::new(store.as_ref());
    for block in 0..12 {
        let mut header = store.get_block_header(block).unwrap().unwrap();
        header.transactions_count = 6;
        batch.put_block_header(block, &header);
        for tx_index in 0..6 {
            let mut tx = vec![0u8; 32];
            tx[0] = block as u8;
            tx[1] = tx_index as u8;
            batch.put_tx_hash_map(&tx, block, tx_index);
            batch.put_tx_index(
                block,
                tx_index,
                &TxIndexEntry {
                    tx_hash: tx.as_slice().try_into().unwrap(),
                    is_cellbase: block == 0 && tx_index == 0,
                    timestamp: 1_000 + block,
                    inputs_count: 1,
                    outputs_count: 4,
                    fee: 0,
                    tx_size: 100,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
            for output_index in 0..4 {
                batch.put_cell(&tx, output_index, &cell(0), block);
            }
        }
    }
    batch.commit().unwrap();
    let rotated = format!("0x{}-0", "00".repeat(32));
    let app = create_router(test_config(store.clone())).await;
    let (status, json) = get_json(&app, &format!("/vessels/snapshot?tracked={rotated}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["anchorBlock"], 11);
    assert_eq!(json["firstSampledBlock"], 0);
    assert_eq!(json["blocksInWindow"], 12);
    assert_eq!(json["examinedOutputs"], 288);
    assert_eq!(json["maxVisible"], 64);
    assert_eq!(json["cells"].as_array().unwrap().len(), 64);
    assert_eq!(
        json["cells"][0]["outpoint"],
        format!(
            "0x{}-0",
            hex::encode(
                [11u8, 0]
                    .iter()
                    .copied()
                    .chain(std::iter::repeat_n(0, 30))
                    .collect::<Vec<_>>()
            )
        )
    );
    assert_eq!(
        json["cells"][63]["outpoint"],
        format!(
            "0x{}-3",
            hex::encode(
                [9u8, 3]
                    .iter()
                    .copied()
                    .chain(std::iter::repeat_n(0, 30))
                    .collect::<Vec<_>>()
            )
        )
    );
    assert!(json["cells"]
        .as_array()
        .unwrap()
        .iter()
        .all(|cell| cell["outpoint"] != rotated));
    assert_eq!(json["tracked"][0]["status"], "live");
    assert_eq!(json["tracked"][0]["outpoint"], rotated);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_consumed_cell_meta(&[0u8; 32], 0, 0, 11, Some(&[0x99; 32]));
    batch.commit().unwrap();
    let (status, json) = get_json(&app, "/vessels/snapshot").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json.to_string()
            .contains("lacks exactly one canonical state marker"),
        "{json}"
    );
}
