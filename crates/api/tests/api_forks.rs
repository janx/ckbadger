use crate::common::*;
use ckbadger_store::types::ReorgEventKind;

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
    // D7: a deep detection dropped nothing, so it has no detail address; the
    // list row carries every recorded fact and says the orphan was not observed.
    let (status, list) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let row = &list["data"][0];
    assert_eq!(row["eventType"], "deep");
    assert_eq!(row["previewStatus"], "no_old_branch");
    assert_eq!(row["orphanedBlocksCount"], 0);
    assert_eq!(row["oldTipNumber"], 12);
    assert_eq!(row["firstOrphanStatus"], "not_observed");
    assert_eq!(row["firstOrphanHash"], serde_json::Value::Null);
    assert_eq!(
        status_of(&app, &format!("/forks/{id}")).await,
        StatusCode::NOT_FOUND
    );
    emit_fixture("fork-deep-row.json", row);
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
    let (status, detail) = get_json(&app, &d7(10, old11.hash().as_slice())).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["event"]["id"], id);
    assert_eq!(detail["event"]["firstOrphanStatus"], "traced");
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
    let (status, json) = get_json(&app, &d7(10, orphan.hash().as_slice())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["event"]["id"], id);
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
    // The later switch back: it dropped `adopted` and restored `block`.
    let mut reversing = record.clone();
    reversing.detected_at = reversing_id / 1000;
    reversing.old_tip_hash = adopted.hash().as_slice().to_vec();
    reversing.new_tip_hash = block.hash().as_slice().to_vec();
    reversing.reverted_by_event = None;
    reversing.reverses_events = vec![key.clone()];
    put_event(&store, reversing_id, 0x22, &reversing);
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(&app, &d7(10, block.hash().as_slice())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["event"]["revertedByEvent"],
        serde_json::json!({ "forkPointNumber": 10,
            "firstOrphanHash": format!("0x{}", hex::encode(adopted.hash().as_slice())),
            "firstOrphanStatus": "recorded" })
    );
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
    let (status, json) = get_json(&app, &d7(10, old.hash().as_slice())).await;
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
    let (status, json) = get_json(&app, &d7(10, orphan.hash().as_slice())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["event"]["id"], id);
    assert_eq!(json["orphanedBlocks"][0]["fate"], "not_included_as_uncle");
    assert_eq!(
        json["orphanedTransactions"][1]["fate"],
        "not_committed_as_of_tip"
    );
    assert_eq!(json["canonicalTipNumber"], 12);
    emit_fixture("fork-unincluded-detail.json", &json);
}

// ---- Lane R2b-b: D7 fork addresses, miner addresses, node switch record ----

fn put_event(store: &CkbadgerStore, id: i64, uuid: u8, record: &ReorgEvent) -> String {
    let key = ckbadger_store::keys::encode_reorg_event_key(id, &[uuid; 16]).unwrap();
    store
        .put_cf(
            store.cf_sync_meta(),
            key.as_bytes(),
            &bincode::serialize(record).unwrap(),
        )
        .unwrap();
    key
}

fn automatic(fork_point: i64, old_tip: i64, old_tip_hash: [u8; 32]) -> ReorgEvent {
    ReorgEvent {
        detected_at: 1_754_000_000,
        kind: ReorgEventKind::Automatic,
        fork_point,
        fork_point_hash: vec![0x33; 32],
        old_tip,
        old_tip_hash: old_tip_hash.to_vec(),
        new_tip: old_tip + 1,
        new_tip_hash: vec![0x44; 32],
        depth: (old_tip - fork_point) as i32,
        orphaned_blocks: old_tip - fork_point,
        orphaned_txs: old_tip - fork_point,
        reverted_by_event: None,
        superseded_by_tip_hash: None,
        reconcile_interrupted: false,
        reverses_events: vec![],
    }
}

fn config_without_node_reader(store: Arc<CkbadgerStore>) -> ckbadger_api::AppConfig {
    let mut config = test_config(store);
    config.ckb_db_path = "/nonexistent/ckbadger-r2b-b-forks".into();
    config
}

async fn status_of(app: &axum::Router, path: &str) -> StatusCode {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .uri(format!("/api/v1{path}"))
        .body(axum::body::Body::empty())
        .unwrap();
    app.clone().oneshot(request).await.unwrap().status()
}

/// D7 path of a block hash: `/forks/<fork_point>/<first 8 hex digits>`.
fn d7(fork_point: i64, hash: &[u8]) -> String {
    format!("/forks/{fork_point}/{}", &hex::encode(hash)[..8])
}

/// A cellbase whose witness names `args` as the miner (secp256k1 blake160 lock).
fn cellbase_paying(args: [u8; 20]) -> ckb_types::core::TransactionView {
    use ckb_types::{bytes::Bytes, core::TransactionBuilder, packed, prelude::*};
    let lock = packed::Script::new_builder()
        .code_hash(
            packed::Byte32::from_slice(
                &hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                    .unwrap(),
            )
            .unwrap(),
        )
        .hash_type(ckb_types::core::ScriptHashType::Type.into())
        .args(Bytes::from(args.to_vec()).pack())
        .build();
    let witness = packed::CellbaseWitness::new_builder()
        .lock(lock)
        .message(Bytes::from_static(b"r2b-b").pack())
        .build();
    TransactionBuilder::default()
        .input(packed::CellInput::new_cellbase_input(11))
        .output(
            packed::CellOutput::new_builder()
                .capacity(1u64.pack())
                .build(),
        )
        .output_data(Vec::<u8>::new().pack())
        .witness(witness.as_bytes().pack())
        .build()
}

fn secp_address(args: [u8; 20]) -> String {
    ckbadger_api::utils::script_to_address(
        &hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap(),
        1,
        &args,
        "mainnet",
    )
    .unwrap()
}

#[tokio::test]
async fn d7_route_names_one_event_by_fork_point_and_first_orphan() {
    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction},
        packed,
        prelude::*,
    };
    let store = test_store();
    let fork_hash = [0x33; 32];
    let epoch = EpochNumberWithFraction::new(1, 0, 1800);
    let orphan = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_050u64.pack())
        .transaction(cellbase_paying([0xaa; 20]))
        .build();
    let canonical11 = BlockBuilder::default()
        .number(11u64.pack())
        .epoch(epoch.pack())
        .parent_hash(packed::Byte32::new(fork_hash))
        .timestamp(1_700_000_000_000u64.pack())
        .transaction(cellbase_paying([0xbb; 20]))
        .build();
    let canonical12 = BlockBuilder::default()
        .number(12u64.pack())
        .epoch(epoch.pack())
        .parent_hash(canonical11.hash())
        .timestamp(1_700_000_001_000u64.pack())
        .transaction(cellbase_paying([0xbb; 20]))
        .uncle(orphan.as_uncle())
        .build();
    let chain = seed_ckb_chain(&[orphan.clone(), canonical11.clone(), canonical12.clone()]);
    // Every height up to the tip, so the overview's chain-time search can run.
    let heights: Vec<(i64, i64)> = (0..=12)
        .map(|n| {
            (
                n,
                if n == 12 {
                    1_700_000_001_000
                } else {
                    1_700_000_000_000
                },
            )
        })
        .collect();
    seed_block_headers_at(&store, &heights);
    let miner_bb = {
        let tx = cellbase_paying([0xbb; 20]);
        let witness = tx.witnesses().get(0).unwrap().raw_data();
        let witness = packed::CellbaseWitness::from_slice(&witness).unwrap();
        witness.lock().calc_script_hash().as_slice().to_vec()
    };
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, hash, uncles) in [
        (10, fork_hash.to_vec(), 0),
        (11, canonical11.hash().as_slice().to_vec(), 0),
        (12, canonical12.hash().as_slice().to_vec(), 1),
    ] {
        let mut header = store.get_block_header(number).unwrap().unwrap();
        header.hash = hash;
        header.uncles_count = uncles;
        header.transactions_count = 1;
        header.miner_lock_hash = (number > 10).then(|| miner_bb.clone());
        batch.put_block_header(number, &header);
    }
    batch.commit().unwrap();
    let id = 1_754_000_000_000i64;
    let mut record = automatic(10, 11, orphan.hash().unpack());
    record.new_tip = 12;
    record.new_tip_hash = canonical12.hash().as_slice().to_vec();
    record.orphaned_txs = 1;
    put_event(&store, id, 0x11, &record);
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let orphan_hex = format!("0x{}", hex::encode(orphan.hash().as_slice()));

    // D7: the detail lives at the fork point and the first dropped block's prefix.
    let (status, json) = get_json(&app, &d7(10, orphan.hash().as_slice())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["event"]["forkPointNumber"], 10);
    assert_eq!(json["event"]["firstOrphanHash"], orphan_hex);
    assert_eq!(json["event"]["firstOrphanStatus"], "recorded");
    assert_eq!(json["orphanedBlocks"][0]["hash"], orphan_hex);
    // Item 12: miner addresses beside every miner lock hash, from the cellbase witness.
    assert_eq!(
        json["orphanedBlocks"][0]["minerAddress"],
        secp_address([0xaa; 20])
    );
    assert_eq!(
        json["orphanedBlocks"][0]["competingBlock"]["minerAddress"],
        secp_address([0xbb; 20])
    );
    assert_eq!(
        json["canonicalBranch"][0]["minerAddress"],
        secp_address([0xbb; 20])
    );
    assert_eq!(
        json["adoptedBranch"][1]["minerAddress"],
        secp_address([0xbb; 20])
    );

    // The millisecond route is gone (D7: removed, not redirected).
    assert_eq!(
        status_of(&app, &format!("/forks/{id}")).await,
        StatusCode::NOT_FOUND
    );
    // An address that names no event is a 404; a malformed one is a 400.
    let (status, json) = get_json(&app, "/forks/10/00000000").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    for bad in [
        "/forks/10/ABCDEF12",
        "/forks/10/abc",
        "/forks/-1/abcdef12",
        "/forks/010/abcdef12",
    ] {
        let (status, json) = get_json(&app, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {json}");
    }

    // The list row carries the same address facts without a detail request.
    let (status, list) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["data"][0]["firstOrphanHash"], orphan_hex);
    assert_eq!(list["data"][0]["firstOrphanStatus"], "recorded");
    assert_eq!(
        list["data"][0]["preview"]["oldMinerAddress"],
        secp_address([0xaa; 20])
    );
    assert_eq!(
        list["data"][0]["preview"]["competingMinerAddress"],
        secp_address([0xbb; 20])
    );

    // Item 13: the overview's node record reads the same branch evidence.
    let (status, overview) = get_json(&app, "/forks/overview?days=1").await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    let record = &overview["nodeRecord"];
    assert_eq!(record["evidenceStatus"], "available", "{overview}");
    assert_eq!(record["orphanFates"]["eventsWithEvidence"], 1);
    assert_eq!(record["orphanFates"]["blocks"]["includedAsUncle"], 1);
    assert_eq!(
        record["orphanFates"]["blocks"]["uncleDistances"],
        serde_json::json!([{ "distance": 1, "count": 1 }])
    );
    assert_eq!(record["orphanFates"]["blocks"]["canonicalNow"], 0);
    assert_eq!(record["orphanFates"]["competing"]["compared"], 1);
    assert_eq!(record["orphanFates"]["competing"]["sameMiner"], 0);
    assert_eq!(record["orphanFates"]["competing"]["gapMedianMs"], "50");
}

#[tokio::test]
async fn first_orphan_is_recorded_for_one_block_switches_without_the_node_reader() {
    let store = test_store();
    let one = put_event(
        &store,
        1_754_000_000_000,
        0x11,
        &automatic(10, 11, [0xa1; 32]),
    );
    let two = put_event(
        &store,
        1_754_000_001_000,
        0x12,
        &automatic(20, 22, [0xa2; 32]),
    );
    let mut deep = automatic(30, 33, [0xa3; 32]);
    deep.kind = ReorgEventKind::Deep;
    deep.orphaned_blocks = 0;
    deep.orphaned_txs = 0;
    put_event(&store, 1_754_000_002_000, 0x13, &deep);
    let app = create_router(config_without_node_reader(store)).await;
    let (status, list) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let rows = list["data"].as_array().unwrap();
    // Newest first: deep, two-block, one-block.
    assert_eq!(rows[0]["firstOrphanStatus"], "not_observed");
    assert_eq!(rows[0]["firstOrphanHash"], serde_json::Value::Null);
    assert_eq!(rows[1]["firstOrphanStatus"], "needs_node_reader");
    assert_eq!(rows[1]["firstOrphanHash"], serde_json::Value::Null);
    assert_eq!(rows[2]["firstOrphanStatus"], "recorded");
    assert_eq!(rows[2]["firstOrphanHash"], format!("0x{}", "a1".repeat(32)));
    let _ = (one, two);
}

#[tokio::test]
async fn first_orphan_of_a_two_block_switch_is_traced_on_the_node_and_addressable() {
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
    let chain = seed_ckb_chain(&[old11.clone(), old12.clone()]);
    let mut record = automatic(10, 12, old12.hash().unpack());
    record.fork_point_hash = fork_hash.to_vec();
    put_event(&store, 1_754_000_000_000, 0x11, &record);
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, list) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["data"][0]["firstOrphanStatus"], "traced");
    assert_eq!(
        list["data"][0]["firstOrphanHash"],
        format!("0x{}", hex::encode(old11.hash().as_slice()))
    );
    // The old tip's prefix does not address the event: only the first dropped block does.
    let (status, json) = get_json(&app, &d7(10, old12.hash().as_slice())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

#[tokio::test]
async fn d7_route_refuses_an_address_that_names_two_events() {
    let store = test_store();
    let first = put_event(
        &store,
        1_754_000_000_000,
        0x11,
        &automatic(10, 11, [0xa1; 32]),
    );
    let again = put_event(
        &store,
        1_754_000_005_000,
        0x12,
        &automatic(10, 11, [0xa1; 32]),
    );
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, &d7(10, &[0xa1; 32])).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains(&first) && message.contains(&again),
        "{json}"
    );
}

#[tokio::test]
async fn reversal_links_are_d7_addresses_not_millisecond_ids() {
    let store = test_store();
    let reversing_key =
        ckbadger_store::keys::encode_reorg_event_key(1_754_000_005_000, &[0x12; 16]).unwrap();
    let mut first = automatic(10, 11, [0xa1; 32]);
    first.reverted_by_event = Some(reversing_key.clone());
    let first_key = put_event(&store, 1_754_000_000_000, 0x11, &first);
    let mut reversing = automatic(10, 11, [0xb1; 32]);
    reversing.reverses_events = vec![first_key];
    put_event(&store, 1_754_000_005_000, 0x12, &reversing);
    let app = create_router(test_config(store)).await;
    let (status, list) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list["data"][1]["revertedByEvent"],
        serde_json::json!({ "forkPointNumber": 10, "firstOrphanHash": format!("0x{}", "b1".repeat(32)),
            "firstOrphanStatus": "recorded" })
    );
    assert_eq!(
        list["data"][0]["reversesEvents"],
        serde_json::json!([{ "forkPointNumber": 10, "firstOrphanHash": format!("0x{}", "a1".repeat(32)),
            "firstOrphanStatus": "recorded" }])
    );
    assert_eq!(list["data"][0]["revertedByEvent"], serde_json::Value::Null);
}

#[tokio::test]
async fn overview_reports_the_node_switch_record_over_all_events() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &[(0, utc8_ms("2026-10-20")), (1, utc8_ms("2026-10-22"))],
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
    store
        .update_sync_status(|status| {
            status.bulk_sync_completed_at = Some(utc8_ms("2026-10-20") / 1000);
            status.bulk_sync_completed_block = Some(0);
        })
        .unwrap();
    let mut one = automatic(10, 11, [0xa1; 32]);
    one.orphaned_txs = 3;
    let mut two = automatic(20, 22, [0xa2; 32]);
    two.orphaned_txs = 4;
    put_event(&store, utc8_ms("2026-10-21"), 0x11, &one);
    put_event(&store, utc8_ms("2026-10-21") + 1000, 0x12, &two);
    let mut deep = automatic(30, 33, [0xa3; 32]);
    deep.kind = ReorgEventKind::Deep;
    deep.orphaned_blocks = 0;
    deep.orphaned_txs = 0;
    put_event(&store, utc8_ms("2026-10-21") + 2000, 0x13, &deep);
    let app = create_router(config_without_node_reader(store)).await;
    let (status, json) = get_json(&app, "/forks/overview?days=3").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let record = &json["nodeRecord"];
    assert_eq!(
        record["observedSince"], "2026-10-20T04:00:00+00:00",
        "{json}"
    );
    assert_eq!(record["observedSinceBlock"], 0);
    assert_eq!(record["firstSwitchDetectedAt"], "2026-10-21T04:00:00+00:00");
    assert_eq!(json["nodeSwitchesAllTime"], 2);
    assert_eq!(record["switchesDeeperThanOneBlock"], 1);
    assert_eq!(record["deepDetections"], 1);
    assert_eq!(record["orphanedBlocks"], 3);
    assert_eq!(record["orphanedTransactions"], 7);
    // Fates need the node's copy of each old branch.
    assert_eq!(record["evidenceStatus"], "node_reader_unavailable");
    assert_eq!(record["orphanFates"], serde_json::Value::Null);
}

#[tokio::test]
async fn fork_history_is_read_once_per_tip_and_fresh_while_reconcile_is_pending() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000), (1, 1_700_000_001_000)]);
    put_event(
        &store,
        1_754_000_000_000,
        0x11,
        &automatic(10, 11, [0xa1; 32]),
    );
    let app = create_router(config_without_node_reader(store.clone())).await;
    let (status, first) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["total"], 1);

    // Production writes an event row only together with the reconcile marker
    // or the deep-fork flag. A row slipped in without either, at the same tip,
    // is invisible: the history was read from the store once for this tip.
    put_event(
        &store,
        1_754_000_001_000,
        0x12,
        &automatic(20, 21, [0xa2; 32]),
    );
    let (_, second) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(second["total"], 1, "{second}");
    let (_, recent) = get_json(&app, "/forks/recent").await;
    assert_eq!(recent["reorg"]["forkPointNumber"], 10, "{recent}");

    // The tip moves: the history is read again.
    seed_block_headers_at(&store, &[(2, 1_700_000_002_000)]);
    let (_, third) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(third["total"], 2, "{third}");

    // While a reconcile is pending, every request reads the history afresh.
    let pending = put_event(
        &store,
        1_754_000_002_000,
        0x13,
        &automatic(30, 31, [0xa3; 32]),
    );
    store
        .put_cf(
            store.cf_sync_meta(),
            ckbadger_store::keys::sync_meta_keys::REORG_RECONCILE_PENDING,
            pending.as_bytes(),
        )
        .unwrap();
    let (_, fourth) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(fourth["total"], 3, "{fourth}");
    put_event(
        &store,
        1_754_000_003_000,
        0x14,
        &automatic(40, 41, [0xa4; 32]),
    );
    let (_, fifth) = get_json(&app, "/forks?limit=10").await;
    assert_eq!(fifth["total"], 4, "{fifth}");
}
