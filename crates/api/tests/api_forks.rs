mod common;
use ckbadger_store::types::ReorgEventKind;
use common::*;

fn emit_fixture(name: &str, json: &serde_json::Value) {
    if let Ok(dir) = std::env::var("CKBADGER_FORK_FIXTURE_DIR") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), serde_json::to_vec_pretty(json).unwrap()).unwrap();
    }
}

fn utc8_ms(day: &str) -> i64 {
    use chrono::TimeZone;
    chrono::FixedOffset::east_opt(8 * 3600)
        .unwrap()
        .from_local_datetime(
            &chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
        )
        .single()
        .unwrap()
        .timestamp_millis()
}

fn event(store: &CkbadgerStore, id: i64, kind: ReorgEventKind) {
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind,
        fork_point: 1,
        fork_point_hash: vec![1; 32],
        old_tip: 2,
        old_tip_hash: vec![2; 32],
        new_tip: 3,
        new_tip_hash: vec![3; 32],
        depth: 1,
        orphaned_blocks: 1,
        orphaned_txs: 0,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[id as u8; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
}

#[tokio::test]
async fn recent_fork_reports_persisted_superseding_tip_hash() {
    let store = test_store();
    let id = 1_700_000_000_000i64;
    event(&store, id, ReorgEventKind::Automatic);
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[id as u8; 16]).unwrap();
    let bytes = store
        .get_cf(store.cf_sync_meta(), key.as_bytes())
        .unwrap()
        .unwrap();
    let mut record: ReorgEvent = bincode::deserialize(&bytes).unwrap();
    record.superseded_by_tip_hash = Some(vec![0xdd; 32]);
    record.reconcile_interrupted = true;
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/forks/recent").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["reorg"]["reconcileInterrupted"], true);
    assert_eq!(
        json["reorg"]["supersededByTipHash"],
        format!("0x{}", "dd".repeat(32))
    );
}

#[tokio::test]
async fn overview_separates_canonical_uncles_from_node_switches_on_complete_days() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &[
            (0, utc8_ms("2026-10-20")),
            (1, utc8_ms("2026-10-21")),
            (2, utc8_ms("2026-10-23")),
        ],
    );
    store
        .put_daily_block_stats(
            "20261020",
            &DailyBlockStats {
                difficulty_sum: 1,
                block_count: 1,
                total_uncles: 2,
            },
        )
        .unwrap();
    store
        .put_daily_block_stats(
            "20261021",
            &DailyBlockStats {
                difficulty_sum: 1,
                block_count: 1,
                total_uncles: 3,
            },
        )
        .unwrap();
    store
        .put_daily_block_stats(
            "20261023",
            &DailyBlockStats {
                difficulty_sum: 1,
                block_count: 1,
                total_uncles: 9,
            },
        )
        .unwrap();
    event(&store, utc8_ms("2026-10-21"), ReorgEventKind::Automatic);
    event(&store, utc8_ms("2026-10-21") + 1000, ReorgEventKind::Deep);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/forks/overview?days=3").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["throughDate"], "2026-10-22");
    assert_eq!(
        json["days"],
        serde_json::json!([
            { "date": "2026-10-20", "canonicalBlocks": 1, "uncles": 2, "nodeSwitches": 0 },
            { "date": "2026-10-21", "canonicalBlocks": 1, "uncles": 3, "nodeSwitches": 1 },
            { "date": "2026-10-22", "canonicalBlocks": 0, "uncles": 0, "nodeSwitches": 0 },
        ])
    );
    assert_eq!(json["unclesInWindow"], 5);
    assert_eq!(json["nodeSwitchesInWindow"], 1);
    assert_eq!(json["nodeSwitchesAllTime"], 1);
    emit_fixture("fork-overview.json", &json);
}

#[tokio::test]
async fn overview_fails_if_a_block_bearing_day_has_no_daily_stats() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &[
            (0, utc8_ms("2026-10-20")),
            (1, utc8_ms("2026-10-21")),
            (2, utc8_ms("2026-10-22")),
        ],
    );
    store
        .put_daily_block_stats(
            "20261020",
            &DailyBlockStats {
                difficulty_sum: 1,
                block_count: 1,
                total_uncles: 0,
            },
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/forks/overview?days=2").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing daily block stats on block-bearing fork day 2026-10-21"));
}

#[tokio::test]
async fn deep_detection_reports_recorded_tips_without_claiming_a_rollback() {
    let store = test_store();
    let id = 1_754_000_000_000i64;
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind: ReorgEventKind::Deep,
        fork_point: 10,
        fork_point_hash: vec![0x11; 32],
        old_tip: 12,
        old_tip_hash: vec![0x22; 32],
        new_tip: 13,
        new_tip_hash: vec![0x33; 32],
        depth: 3,
        orphaned_blocks: 0,
        orphaned_txs: 0,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, detail) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["event"]["eventType"], "deep");
    assert_eq!(detail["event"]["previewStatus"], "no_old_branch");
    assert_eq!(detail["event"]["orphanedBlocksCount"], 0);
    assert_eq!(detail["orphanedBlocks"], serde_json::json!([]));
    assert_eq!(detail["adoptedBranch"], serde_json::json!([]));
    emit_fixture("fork-deep-detail.json", &detail);
}

#[tokio::test]
async fn detail_resolves_two_old_blocks_from_one_canonical_uncle_pass() {
    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction},
        packed,
        prelude::*,
    };
    let store = test_store();
    let fork_hash = [0x33; 32];
    let epoch = EpochNumberWithFraction::new(1, 0, 1800);
    let old11 = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_010u64.pack())
        .build();
    let old12 = BlockBuilder::default()
        .number(12u64.pack())
        .epoch(epoch.pack())
        .parent_hash(old11.hash())
        .timestamp(1_700_000_001_010u64.pack())
        .build();
    let canonical11 = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_000u64.pack())
        .build();
    let canonical12 = BlockBuilder::default()
        .number(12u64.pack())
        .epoch(epoch.pack())
        .parent_hash(canonical11.hash())
        .timestamp(1_700_000_001_000u64.pack())
        .build();
    let canonical13 = BlockBuilder::default()
        .number(13u64.pack())
        .epoch(epoch.pack())
        .parent_hash(canonical12.hash())
        .timestamp(1_700_000_002_000u64.pack())
        .uncle(old11.as_uncle())
        .uncle(old12.as_uncle())
        .build();
    let chain = seed_ckb_chain(&[
        old11.clone(),
        old12.clone(),
        canonical11.clone(),
        canonical12.clone(),
        canonical13.clone(),
    ]);
    seed_block_headers_at(
        &store,
        &[
            (0, 1_700_000_000_000),
            (10, 1_700_000_000_000),
            (11, 1_700_000_000_000),
            (12, 1_700_000_001_000),
            (13, 1_700_000_002_000),
        ],
    );
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, hash, uncles) in [
        (10, fork_hash.to_vec(), 0),
        (11, canonical11.hash().as_slice().to_vec(), 0),
        (12, canonical12.hash().as_slice().to_vec(), 0),
        (13, canonical13.hash().as_slice().to_vec(), 2),
    ] {
        let mut header = store.get_block_header(number).unwrap().unwrap();
        header.hash = hash;
        header.uncles_count = uncles;
        header.transactions_count = 0;
        batch.put_block_header(number, &header);
    }
    batch.commit().unwrap();
    let id = 1_754_000_000_000i64;
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind: ReorgEventKind::Automatic,
        fork_point: 10,
        fork_point_hash: fork_hash.to_vec(),
        old_tip: 12,
        old_tip_hash: old12.hash().as_slice().to_vec(),
        new_tip: 13,
        new_tip_hash: canonical13.hash().as_slice().to_vec(),
        depth: 2,
        orphaned_blocks: 2,
        orphaned_txs: 0,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, detail) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["orphanedBlocks"].as_array().unwrap().len(), 2);
    assert_eq!(detail["orphanedBlocks"][0]["number"], 11);
    assert_eq!(detail["orphanedBlocks"][1]["number"], 12);
    assert_eq!(detail["orphanedBlocks"][0]["fate"], "included_as_uncle");
    assert_eq!(detail["orphanedBlocks"][1]["fate"], "included_as_uncle");
    assert_eq!(detail["orphanedBlocks"][0]["uncleIn"]["distance"], 2);
    assert_eq!(detail["orphanedBlocks"][1]["uncleIn"]["distance"], 1);
    assert_eq!(detail["adoptedBranch"].as_array().unwrap().len(), 3);
    emit_fixture("fork-multi-uncle-detail.json", &detail);
}

#[tokio::test]
async fn detail_proves_uncle_inclusion_competing_gap_and_later_tx_commit() {
    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let fork_hash = [0x33; 32];
    let epoch = EpochNumberWithFraction::new(1, 0, 1800);
    let cellbase = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(1u64.pack())
                .build(),
        )
        .output_data(Vec::<u8>::new().pack())
        .build();
    let tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(1234u64.pack())
                .build(),
        )
        .output_data(Vec::<u8>::new().pack())
        .build();
    let orphan = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_050u64.pack())
        .transaction(cellbase.clone())
        .transaction(tx.clone())
        .build();
    let canonical11 = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_000u64.pack())
        .transaction(cellbase.clone())
        .build();
    let canonical12 = BlockBuilder::default()
        .number(12u64.pack())
        .epoch(epoch.pack())
        .parent_hash(canonical11.hash())
        .timestamp(1_700_000_001_000u64.pack())
        .transaction(cellbase)
        .transaction(tx.clone())
        .uncle(orphan.as_uncle())
        .build();
    let chain = seed_ckb_chain(&[orphan.clone(), canonical11.clone(), canonical12.clone()]);
    seed_block_headers_at(
        &store,
        &[
            (0, 1_700_000_000_000),
            (10, 1_700_000_000_000),
            (11, 1_700_000_000_000),
            (12, 1_700_000_001_000),
        ],
    );
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, hash, uncles, tx_count) in [
        (10, fork_hash.to_vec(), 0, 1),
        (11, canonical11.hash().as_slice().to_vec(), 0, 1),
        (12, canonical12.hash().as_slice().to_vec(), 1, 2),
    ] {
        let mut header = store.get_block_header(number).unwrap().unwrap();
        header.hash = hash;
        header.uncles_count = uncles;
        header.transactions_count = tx_count;
        batch.put_block_header(number, &header);
    }
    let tx_hash: [u8; 32] = tx.hash().unpack();
    batch.put_tx_hash_map(&tx_hash, 12, 1);
    batch.put_tx_index(
        12,
        1,
        &TxIndexEntry {
            tx_hash,
            is_cellbase: false,
            timestamp: 1_700_000_001_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let id = 1_754_000_000_000i64;
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind: ReorgEventKind::Automatic,
        fork_point: 10,
        fork_point_hash: fork_hash.to_vec(),
        old_tip: 11,
        old_tip_hash: orphan.hash().as_slice().to_vec(),
        new_tip: 12,
        new_tip_hash: canonical12.hash().as_slice().to_vec(),
        depth: 1,
        orphaned_blocks: 1,
        orphaned_txs: 2,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["orphanedBlocks"][0]["fate"], "included_as_uncle");
    assert_eq!(json["orphanedBlocks"][0]["uncleIn"]["blockNumber"], 12);
    assert_eq!(
        json["orphanedBlocks"][0]["competingBlock"]["hash"],
        format!("0x{}", hex::encode(canonical11.hash().as_slice()))
    );
    assert_eq!(json["orphanedBlocks"][0]["timestampGapMs"], 50);
    assert_eq!(json["orphanedTransactions"][1]["fate"], "committed_later");
    assert_eq!(
        json["orphanedTransactions"][1]["committedAt"]["blockNumber"],
        12
    );
    assert_eq!(json["canonicalBranch"].as_array().unwrap().len(), 2);
    assert_eq!(json["adoptedBranch"].as_array().unwrap().len(), 2);
    assert_eq!(
        json["adoptedBranch"][0]["hash"],
        format!("0x{}", hex::encode(canonical11.hash().as_slice()))
    );
    emit_fixture("fork-uncle-detail.json", &json);
    let (list_status, list) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(list_status, StatusCode::OK, "{list}");
    assert_eq!(list["data"][0]["previewStatus"], "available");
    assert_eq!(list["data"][0]["preview"]["fate"], "included_as_uncle");
    assert_eq!(list["data"][0]["preview"]["uncleInBlock"], 12);
    assert_eq!(list["data"][0]["preview"]["committedLaterTransactions"], 1);
    emit_fixture("fork-list.json", &list);
}

#[tokio::test]
async fn detail_keeps_historical_drop_but_marks_reentered_block_canonical_now() {
    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction},
        packed,
        prelude::*,
    };
    let store = test_store();
    let fork_hash = [0x33; 32];
    let block = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_000u64.pack())
        .build();
    let adopted = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_100u64.pack())
        .build();
    let chain = seed_ckb_chain(&[adopted.clone(), block.clone()]);
    seed_block_headers_at(
        &store,
        &[
            (0, 1_700_000_000_000),
            (10, 1_700_000_000_000),
            (11, 1_700_000_000_000),
        ],
    );
    let mut header = store.get_block_header(11).unwrap().unwrap();
    header.hash = block.hash().as_slice().to_vec();
    header.transactions_count = 0;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(11, &header);
    batch.commit().unwrap();
    let id = 1_754_000_000_000i64;
    let reversing_id = id + 1000;
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind: ReorgEventKind::Automatic,
        fork_point: 10,
        fork_point_hash: fork_hash.to_vec(),
        old_tip: 11,
        old_tip_hash: block.hash().as_slice().to_vec(),
        new_tip: 11,
        new_tip_hash: adopted.hash().as_slice().to_vec(),
        depth: 1,
        orphaned_blocks: 1,
        orphaned_txs: 0,
        reverted_by_event: Some(
            ckbadger_store::keys::encode_reorg_event_key(reversing_id, &[0x22; 16]).unwrap(),
        ),
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["event"]["revertedByEvent"], reversing_id);
    assert_eq!(json["orphanedBlocks"][0]["fate"], "canonical_now");
    assert_eq!(
        json["orphanedBlocks"][0]["uncleIn"],
        serde_json::Value::Null
    );
    assert_eq!(
        json["orphanedBlocks"][0]["competingBlock"],
        serde_json::Value::Null
    );
    assert_eq!(
        json["canonicalBranch"][0]["hash"],
        json["orphanedBlocks"][0]["hash"]
    );
    assert_eq!(
        json["adoptedBranch"][0]["hash"],
        format!("0x{}", hex::encode(adopted.hash().as_slice()))
    );
    assert_ne!(
        json["adoptedBranch"][0]["hash"],
        json["canonicalBranch"][0]["hash"]
    );
    emit_fixture("fork-reentry-detail.json", &json);
}

#[tokio::test]
async fn detail_fails_if_persisted_adopted_tip_is_not_retained_by_the_node() {
    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction},
        packed,
        prelude::*,
    };
    let store = test_store();
    let fork_hash = [0x33; 32];
    let old = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .build();
    let chain = seed_ckb_chain(std::slice::from_ref(&old));
    let id = 1_754_000_000_000i64;
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind: ReorgEventKind::Automatic,
        fork_point: 10,
        fork_point_hash: fork_hash.to_vec(),
        old_tip: 11,
        old_tip_hash: old.hash().as_slice().to_vec(),
        new_tip: 11,
        new_tip_hash: vec![0x44; 32],
        depth: 1,
        orphaned_blocks: 1,
        orphaned_txs: 0,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("missing adopted block 11"),
        "{json}"
    );
}

#[tokio::test]
async fn detail_reports_epoch_closed_without_uncle_or_tx_commit() {
    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let fork_hash = [0x33; 32];
    let epoch1 = EpochNumberWithFraction::new(1, 0, 1800);
    let epoch2 = EpochNumberWithFraction::new(2, 0, 1800);
    let cellbase = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(1u64.pack())
                .build(),
        )
        .output_data(Vec::<u8>::new().pack())
        .build();
    let tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(77u64.pack())
                .build(),
        )
        .output_data(Vec::<u8>::new().pack())
        .build();
    let orphan = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch1.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_010u64.pack())
        .transaction(cellbase.clone())
        .transaction(tx)
        .build();
    let canonical11 = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch1.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_000u64.pack())
        .transaction(cellbase.clone())
        .build();
    let canonical12 = BlockBuilder::default()
        .number(12u64.pack())
        .epoch(epoch2.pack())
        .parent_hash(canonical11.hash())
        .timestamp(1_700_000_001_000u64.pack())
        .transaction(cellbase)
        .build();
    let chain = seed_ckb_chain(&[orphan.clone(), canonical11.clone(), canonical12.clone()]);
    seed_block_headers_at(
        &store,
        &[
            (0, 1_700_000_000_000),
            (10, 1_700_000_000_000),
            (11, 1_700_000_000_000),
            (12, 1_700_000_001_000),
        ],
    );
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, hash, epoch_number) in [
        (10, fork_hash.to_vec(), 1),
        (11, canonical11.hash().as_slice().to_vec(), 1),
        (12, canonical12.hash().as_slice().to_vec(), 2),
    ] {
        let mut header = store.get_block_header(number).unwrap().unwrap();
        header.hash = hash;
        header.epoch_number = epoch_number;
        batch.put_block_header(number, &header);
    }
    batch.commit().unwrap();
    let id = 1_754_000_000_000i64;
    let record = ReorgEvent {
        detected_at: id / 1000,
        kind: ReorgEventKind::Automatic,
        fork_point: 10,
        fork_point_hash: fork_hash.to_vec(),
        old_tip: 11,
        old_tip_hash: orphan.hash().as_slice().to_vec(),
        new_tip: 12,
        new_tip_hash: canonical12.hash().as_slice().to_vec(),
        depth: 1,
        orphaned_blocks: 1,
        orphaned_txs: 2,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    };
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[0x11; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(&app, &format!("/forks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["orphanedBlocks"][0]["fate"], "not_included_as_uncle");
    assert_eq!(
        json["orphanedTransactions"][1]["fate"],
        "not_committed_as_of_tip"
    );
    assert_eq!(json["canonicalTipNumber"], 12);
    emit_fixture("fork-unincluded-detail.json", &json);
}
