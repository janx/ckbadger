use crate::common::*;

fn write_dao_fixture(name: &str, value: &serde_json::Value) {
    if let Ok(dir) = std::env::var("D6_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/{name}.json"),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn dao_calculator_annualizes_using_header_chain_time_not_block_distance() {
    const YEAR_MS: i64 = 31_557_600_000;
    async fn apc_for_span(span_ms: i64) -> serde_json::Value {
        let store = test_store();
        let mut batch = StoreBatch::new(store.as_ref());
        for (number, ar, chain_time_ms) in [
            (10, 1u64, 1_700_000_000_000),
            (20, 2u64, 1_700_000_000_000 + span_ms),
        ] {
            let mut dao = vec![0u8; 32];
            dao[8..16].copy_from_slice(&ar.to_le_bytes());
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0u8; 32],
                    timestamp: 1_700_000_000_000,
                    epoch_number: 0,
                    epoch_index: 0,
                    epoch_length: 1,
                    dao,
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                    chain_time_ms,
                },
            );
        }
        batch.commit().unwrap();
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(
            &app,
            "/dao/calculator?capacity=20000000000&deposit_block=10&withdraw_block=20",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        json
    }

    let year = apc_for_span(YEAR_MS).await;
    let half_year = apc_for_span(YEAR_MS / 2).await;
    let zero = apc_for_span(0).await;
    assert_eq!(year["apc"], "100.00%");
    assert_eq!(half_year["apc"], "200.00%");
    assert!(zero["apc"].is_null());
}

/// An empty DB has no `dao_latest_stats` singleton yet. That is a startup state
/// that resolves itself, so it must report 503 `initializing` (the contract the
/// SPA retries behind its banner) instead of a fault or a fabricated zero row.
#[tokio::test]
async fn test_dao_stats_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "initializing");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("dao_latest_stats not written yet"));
}

fn dao_evidence_header(
    number: u8,
    epoch: (i64, i32, i32),
    raw_ms: i64,
    chain_ms: i64,
    ar: u64,
) -> CachedBlockHeader {
    let mut dao = vec![0u8; 32];
    dao[..8].copy_from_slice(&1_000_000_000_000_000u64.to_le_bytes());
    dao[8..16].copy_from_slice(&ar.to_le_bytes());
    dao[16..24].copy_from_slice(&500_000_000u64.to_le_bytes());
    dao[24..32].copy_from_slice(&100_000_000u64.to_le_bytes());
    CachedBlockHeader {
        hash: vec![number; 32],
        parent_hash: vec![number.saturating_sub(1); 32],
        timestamp: raw_ms,
        epoch_number: epoch.0,
        epoch_index: epoch.1,
        epoch_length: epoch.2,
        dao,
        transactions_count: 1,
        uncles_count: 0,
        proposals_count: 0,
        compact_target: 0,
        miner_lock_hash: None,
        cycles: None,
        chain_time_ms: chain_ms,
    }
}

fn dao_evidence_entry(capacity: i64, deposit_block: i64, lock: u8) -> DaoDepositCacheEntry {
    DaoDepositCacheEntry {
        capacity,
        occupied_capacity: 102_00000000,
        deposit_block_number: deposit_block,
        deposit_timestamp: 0,
        deposit_chain_time_ms: 0,
        lock_script_hash: vec![lock; 32],
        deposit_ar: 10_000_000_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_request_occupied_capacity: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    }
}

/// The producer fixture covers exact phase boundaries, changed epoch lengths,
/// a raw timestamp rollback, both readiness states, owner denominator and AR.
fn dao_evidence_fixture() -> Arc<CkbadgerStore> {
    use ckbadger_store::LockScriptEntry;

    let store = test_store();
    let year = 365 * 86_400_000i64;
    let epochs = [
        (0, 0, 1),
        (5, 1, 2),
        (10, 1, 2),
        (100, 1, 4),
        (185, 1, 4),
        (185, 2, 4),
    ];
    let times = [0, 1_000, 2_000, year, year + 1_000, year + 2_000];
    let mut batch = StoreBatch::new(store.as_ref());
    for number in 0..6u8 {
        let raw = if number == 4 {
            year - 500
        } else {
            times[number as usize]
        };
        let ar = if number <= 2 {
            10_000_000_000_000_000
        } else if number == 3 {
            10_100_000_000_000_000
        } else {
            10_200_000_000_000_000
        };
        batch.put_block_header(
            i64::from(number),
            &dao_evidence_header(
                number,
                epochs[number as usize],
                raw,
                times[number as usize],
                ar,
            ),
        );
    }
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    for (id, deposit_block, capacity, lock, request_block) in [
        (0xA1, 1, 200_00000000i64, 0x11, None),
        (0xA2, 2, 300_00000000i64, 0x22, None),
        (0xA3, 1, 400_00000000i64, 0x33, Some(3)),
        (0xA4, 2, 500_00000000i64, 0x44, Some(4)),
    ] {
        let mut entry = dao_evidence_entry(capacity, deposit_block, lock);
        entry.deposit_timestamp = times[deposit_block as usize];
        entry.deposit_chain_time_ms = times[deposit_block as usize];
        let args = vec![lock; 20];
        let lock_hash = compute_script_hash(&code_hash, 1, &args);
        entry.lock_script_hash = lock_hash.clone();
        batch.put_lock_script(
            &lock_hash,
            &LockScriptEntry {
                code_hash: code_hash.clone(),
                hash_type: 1,
                args,
            },
        );
        if let Some(block) = request_block {
            entry.status = 1;
            entry.withdraw_request_block = Some(block);
            entry.withdraw_request_tx = Some(vec![id + 1; 32]);
            entry.withdraw_request_output_index = Some(0);
            entry.withdraw_request_ar = Some(if block == 3 {
                10_100_000_000_000_000
            } else {
                10_200_000_000_000_000
            });
            entry.withdraw_request_occupied_capacity = Some(102_00000000);
        }
        batch.put_dao_deposit(&ckbadger_store::keys::encode_outpoint(&[id; 32], 0), &entry);
    }
    batch.commit().unwrap();
    store
        .update_sync_status(|status| status.tip_block_number = 5)
        .unwrap();
    store
}

#[tokio::test]
async fn dao_evidence_partitions_all_live_cells_and_exports_contract() {
    let app = create_router(test_config(dao_evidence_fixture())).await;
    let (status, json) = get_json(&app, "/dao/evidence").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["tipEpoch"]["display"], "185+2/4");
    assert_eq!(json["phase0"]["capacity"], "50000000000");
    assert_eq!(json["phase0"]["count"], 2);
    assert_eq!(json["phase1"]["capacity"], "90000000000");
    assert_eq!(json["phase1ReadyByEpochLock"]["capacity"], "40000000000");
    assert_eq!(json["phase1WaitingForEpochLock"]["capacity"], "50000000000");
    assert_eq!(json["windows"][0]["capacity"], "30000000000");
    assert_eq!(json["windows"][29]["capacity"], "20000000000");
    assert_eq!(json["concentration"]["currentDepositors"], 2);
    assert_eq!(json["concentration"]["top1SharePercent"], "60.00");
    assert_eq!(json["ar"]["year365"]["anchorBlockNumber"], 2);
    assert_eq!(json["header"]["ar"], "10200000000000000");
    write_dao_fixture("dao-evidence", &json);
}

/// DAO-17: `/dao/evidence` scanned every live DAO row on every request
/// (2.7–4.8 s). It is computed once per canonical tip (number and hash) and
/// served from API memory until the tip moves; nothing is written.
#[tokio::test]
async fn dao_evidence_is_memoized_by_tip() {
    let store = dao_evidence_fixture();
    let app = router_from_state(test_app_state(test_config(store.clone())));
    store.reset_read_call_counters();
    let (status, cold) = get_json(&app, "/dao/evidence").await;
    assert_eq!(status, StatusCode::OK, "{cold}");
    let cold_reads = store.read_call_counts();
    store.reset_read_call_counters();
    let (_, warm) = get_json(&app, "/dao/evidence").await;
    let warm_reads = store.read_call_counts();
    assert_eq!(cold, warm);
    assert_ne!(cold_reads, (0, 0));
    assert_eq!(
        warm_reads,
        (0, 0),
        "the same tip must be served from memory: cold={cold_reads:?}"
    );

    // A new tip is a miss: the evidence is recomputed for it.
    let mut header = store.get_block_header(5).unwrap().unwrap();
    header.hash = vec![0x66; 32];
    header.timestamp += 1_000;
    header.chain_time_ms += 1_000;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(6, &header);
    batch.commit().unwrap();
    store.reset_read_call_counters();
    let (status, next) = get_json(&app, "/dao/evidence").await;
    assert_eq!(status, StatusCode::OK, "{next}");
    assert_eq!(next["tipBlockNumber"], 6);
    assert_ne!(store.read_call_counts(), (0, 0));
}

#[tokio::test]
async fn dao_evidence_represents_genesis_with_no_history_or_deposits() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        0,
        &dao_evidence_header(0, (0, 0, 1), 1_000, 1_000, 10_000_000_000_000_000),
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|status| status.tip_block_number = 0)
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/evidence").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["phase0"]["capacity"], "0");
    assert_eq!(json["phase1"]["count"], 0);
    assert_eq!(json["concentration"]["currentDepositors"], 0);
    assert!(json["concentration"]["top1SharePercent"].is_null());
    assert!(json["ar"]["year365"].is_null());
    assert_eq!(json["windows"].as_array().unwrap().len(), 30);
}

#[tokio::test]
async fn dao_evidence_fails_on_missing_canonical_deposit_header() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        0,
        &dao_evidence_header(0, (0, 0, 1), 0, 0, 10_000_000_000_000_000),
    );
    batch.put_block_header(
        2,
        &dao_evidence_header(2, (2, 0, 1), 2_000, 2_000, 10_000_000_000_000_000),
    );
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0xA1; 32], 0),
        &dao_evidence_entry(200_00000000, 1, 0x11),
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|status| status.tip_block_number = 2)
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/evidence").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("missing canonical DAO header at block 1"),
        "{json}"
    );
}

#[tokio::test]
async fn dao_evidence_rejects_contradictory_lifecycle_markers_with_outpoint_context() {
    for defect in [
        "phase1 missing tx",
        "phase1 missing AR",
        "phase0 completion",
    ] {
        let store = test_store();
        let mut batch = StoreBatch::new(store.as_ref());
        for (number, epoch) in [(0, (0, 0, 1)), (1, (5, 1, 2)), (2, (5, 3, 4))] {
            batch.put_block_header(
                number,
                &dao_evidence_header(
                    number as u8,
                    epoch,
                    number * 1_000,
                    number * 1_000,
                    10_000_000_000_000_000,
                ),
            );
        }
        let mut entry = dao_evidence_entry(200_00000000, 1, 0x11);
        match defect {
            "phase1 missing tx" | "phase1 missing AR" => {
                entry.status = 1;
                entry.withdraw_request_block = Some(2);
                entry.withdraw_request_tx = Some(vec![0xB1; 32]);
                entry.withdraw_request_output_index = Some(0);
                entry.withdraw_request_ar = Some(10_000_000_000_000_000);
                entry.withdraw_request_occupied_capacity = Some(102_00000000);
                if defect == "phase1 missing tx" {
                    entry.withdraw_request_tx = None;
                } else {
                    entry.withdraw_request_ar = None;
                }
            }
            "phase0 completion" => entry.withdraw_tx = Some(vec![0xC1; 32]),
            _ => unreachable!(),
        }
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&[0xA1; 32], 0),
            &entry,
        );
        batch.commit().unwrap();
        store
            .update_sync_status(|status| status.tip_block_number = 2)
            .unwrap();
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/dao/evidence").await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{defect}: {json}"
        );
        let message = json["message"].as_str().unwrap();
        assert!(
            message.contains("a1a1a1") && message.contains("invalid DAO lifecycle"),
            "{defect}: {message}"
        );
    }
}

#[tokio::test]
async fn dao_evidence_rejects_owner_script_payload_under_wrong_hash() {
    use ckbadger_store::LockScriptEntry;
    let store = test_store();
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let lock_hash = compute_script_hash(&code_hash, 1, &[0x11; 20]);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        0,
        &dao_evidence_header(0, (0, 0, 1), 0, 0, 10_000_000_000_000_000),
    );
    batch.put_block_header(
        1,
        &dao_evidence_header(1, (1, 0, 1), 1_000, 1_000, 10_000_000_000_000_000),
    );
    let mut entry = dao_evidence_entry(200_00000000, 1, 0x11);
    entry.deposit_timestamp = 1_000;
    entry.deposit_chain_time_ms = 1_000;
    entry.lock_script_hash = lock_hash.clone();
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0xA1; 32], 0),
        &entry,
    );
    // The key is the hash of args 0x11, but the persisted value has args 0x22.
    batch.put_lock_script(
        &lock_hash,
        &LockScriptEntry {
            code_hash,
            hash_type: 1,
            args: vec![0x22; 20],
        },
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|status| status.tip_block_number = 1)
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/evidence").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("does not hash to indexed owner"),
        "{json}"
    );
}

#[tokio::test]
async fn test_dao_stats_serves_the_indexer_singleton() {
    let store = test_store();
    store
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance: 3_360_000_000_000_000_000,
            burnt: 123_456_789_000_000_001,
            virtual_occupied: 0,
        })
        .unwrap();

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xAA; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let latest = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 10,
        total_deposited: 123_00000000,
        total_depositors: 7,
        active_deposits: 9,
        deposit_cells_count: 9,
        withdrawal_request_cells_count: 0,
        total_compensation_paid: 11_00000000,
        unclaimed_compensation: 13_00000000,
        average_deposit_days: Some("950 days".to_string()),
        estimated_apc: "2.74".to_string(),
        mining_reward: 17_00000000,
        deposit_compensation: 19_00000000,
        burnt: 23_00000000,
        pending_withdrawal_capacity: 5_00000000,
        dao_capacity: 128_00000000,
        dao_free_capacity: 26_00000000,
        depositor_allocation_weight: 0,
        tip_total_issuance: 0,
        tip_secondary_pool: 0,
        tip_occupied_capacity: 0,
        rolling_24h: Some(ckbadger_store::types::DaoRolling24h {
            baseline_block_number: 9,
            cutoff_chain_time_ms: 1_699_913_600_000,
            deposited_change: -3_00000000,
            depositors_change: 2,
            claimed_compensation_change: 5_00000000,
            unclaimed_compensation_change: -7_00000000,
        }),
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let value = bincode::serialize(&latest).unwrap();
    store.put_stats_key(&key, &value).unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalDeposited"], "12300000000");
    assert_eq!(json["totalDepositors"], 7);
    assert_eq!(json["activeDeposits"], 9);
    assert_eq!(json["depositCellsCount"], 9);
    assert_eq!(json["withdrawalRequestCellsCount"], 0);
    assert_eq!(json["daoCapacity"], "12800000000");
    assert_eq!(json["daoFreeCapacity"], "2600000000");
    assert_eq!(json["depositorAllocationWeight"], "0");
    assert_eq!(json["tipTotalIssuance"], "0");
    assert_eq!(json["tipProtocolOccupiedCapacity"], "0");
    assert_eq!(json["rolling24hBaselineBlockNumber"], 9);
    assert_eq!(json["rolling24hCutoffChainTimeMs"], "1699913600000");
    assert_eq!(json["estimatedApc"], "2.74");
    assert_eq!(json["tipBlockNumber"], 10);
    assert_eq!(json["genesisBaselineBurnt"], "123456789000000001");
    assert_eq!(json["depositChange24h"], "-3");
    assert_eq!(json["depositorsChange24h"], 2);
    assert_eq!(json["claimedCompensationChange24h"], "5");
    assert_eq!(json["unclaimedCompensationChange24h"], "-7");
}

/// The singleton trails the sync tip by at most one batch commit, so a tip
/// mismatch is normal operation, not a reason to recompute the same numbers a
/// second way. The response serves the singleton and states the block it was
/// computed at.
#[tokio::test]
async fn test_dao_stats_serves_singleton_and_reports_its_as_of_block() {
    let store = test_store();
    seed_genesis_baseline(&store);

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xBB; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20231115",
    );
    batch.put_stats(
        &snapshot_key,
        &bincode::serialize(&DaoDailySnapshot {
            date: "2023-11-15".to_string(),
            total_deposited: 0,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance: 1,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unclaimed_compensation: 0,
            frozen_phase1_compensation: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: Some(0),
        })
        .unwrap(),
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let stale = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 9,
        total_deposited: 999_00000000,
        total_depositors: 999,
        active_deposits: 999,
        deposit_cells_count: 999,
        withdrawal_request_cells_count: 0,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: Some("999 days".to_string()),
        estimated_apc: "9.99".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
        pending_withdrawal_capacity: 0,
        dao_capacity: 0,
        dao_free_capacity: 0,
        depositor_allocation_weight: 0,
        tip_total_issuance: 0,
        tip_secondary_pool: 0,
        tip_occupied_capacity: 0,
        rolling_24h: None,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let value = bincode::serialize(&stale).unwrap();
    store.put_stats_key(&key, &value).unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["totalDeposited"], "99900000000");
    assert_eq!(json["totalDepositors"], 999);
    assert!(json["depositChange24h"].is_null());
    assert!(json["depositorsChange24h"].is_null());
    assert_eq!(
        json["tipBlockNumber"], 9,
        "the response must state the block the singleton was computed at"
    );
}

/// The indexer writes `dao_latest_stats` after every batch commit and after
/// every reorg rollback, so its absence at a synced tip is an invariant
/// violation. It must fail loudly with the tip it was observed at rather than
/// quietly recomputing the numbers a second way (which is what hid the
/// post-reorg singleton gap).
#[tokio::test]
async fn test_dao_stats_fails_fast_when_singleton_missing_at_synced_tip() {
    let store = test_store();
    seed_genesis_baseline(&store);

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xCC; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    // Deliberately no dao_latest_stats singleton at a synced tip.
    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("missing dao_latest_stats at sync tip block 10"),
        "message should name the missing singleton and the tip: {json}"
    );
}

/// `/dao/statistics` reads the singleton on every request and never caches it,
/// so a refreshed singleton (for example the one the indexer writes right after
/// a reorg rollback) is served immediately instead of behind a TTL.
#[tokio::test]
async fn test_dao_stats_follows_singleton_without_stale_cache() {
    let store = test_store();
    seed_genesis_baseline(&store);
    seed_tip_header(&store, 10);

    let mut latest = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 10,
        total_deposited: 200_00000000,
        total_depositors: 1,
        active_deposits: 1,
        deposit_cells_count: 1,
        withdrawal_request_cells_count: 0,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: Some("1 days".to_string()),
        estimated_apc: "2.00".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
        pending_withdrawal_capacity: 0,
        dao_capacity: 0,
        dao_free_capacity: 0,
        depositor_allocation_weight: 0,
        tip_total_issuance: 0,
        tip_secondary_pool: 0,
        tip_occupied_capacity: 0,
        rolling_24h: None,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    store
        .put_stats_key(&key, &bincode::serialize(&latest).unwrap())
        .unwrap();

    let app = create_router(test_config(store.clone())).await;
    let (status, first) = get_json(&app, "/dao/statistics").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["totalDeposited"], "20000000000");
    assert_eq!(first["tipBlockNumber"], 10);

    // The indexer advances the singleton (as it does after every batch commit
    // and after every rollback).
    latest.tip_block_number = 11;
    latest.total_deposited = 500_00000000;
    store
        .put_stats_key(&key, &bincode::serialize(&latest).unwrap())
        .unwrap();

    let (status, second) = get_json(&app, "/dao/statistics").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["totalDeposited"], "50000000000");
    assert_eq!(second["tipBlockNumber"], 11);
}

#[tokio::test]
async fn test_total_deposit_chart_recomputes_after_initial_empty_response() {
    let store = test_store();
    let config = test_config(store.clone());
    let app = create_router(config).await;

    let first_request = Request::builder()
        .uri("/api/v1/dao/charts/total-deposit")
        .body(Body::empty())
        .unwrap();
    let first_response = app.clone().oneshot(first_request).await.unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(first_json["data"], serde_json::json!([]));

    let snapshot = DaoDailySnapshot {
        date: "2024-01-15".to_string(),
        total_deposited: 123_00000000,
        depositors_count: 7,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 123_00000000,
        total_issuance: 0,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: 7,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20240115",
    );
    let value = bincode::serialize(&snapshot).unwrap();
    store.put_cf(store.cf_stats_dao(), &key, &value).unwrap();
    // The indexed tip is on the next day, so 2024-01-15 is complete.
    seed_tip_block_on_utc8_day(&store, 2, "2024-01-16");

    let second_request = Request::builder()
        .uri("/api/v1/dao/charts/total-deposit")
        .body(Body::empty())
        .unwrap();
    let second_response = app.oneshot(second_request).await.unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    let second_data = second_json["data"].as_array().unwrap();
    assert_eq!(second_data.len(), 1);
    assert_eq!(second_data[0]["date"], "2024-01-15");
    assert_eq!(second_data[0]["value"], "123");
    assert_eq!(second_data[0]["value2"], "7");
}

fn total_deposit_snapshot(date: &str, total_deposited: i128, depositors: i64) -> DaoDailySnapshot {
    DaoDailySnapshot {
        date: date.to_string(),
        total_deposited,
        depositors_count: depositors,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: total_deposited,
        total_issuance: 0,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: depositors,
        daily_depositor_addresses: 0,
        protocol_deposited: None,
    }
}

/// A depositor may have withdrawn all of their cells while remaining in the
/// historical ever-depositor count. The chart secondary line says *current*.
#[tokio::test]
async fn total_deposit_chart_uses_current_depositors_not_ever_depositors() {
    let store = test_store();
    let mut snapshot = total_deposit_snapshot("2024-01-15", 123_00000000, 7);
    snapshot.cumulative_depositors = 70;
    put_total_deposit_snapshot(&store, &snapshot);
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/charts/total-deposit").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["value2"], "7");
    assert_eq!(json["y2AxisLabel"], "Current depositors");
}

/// The Explorer comparison is cumulative, while the total-deposit board's
/// secondary line is intentionally current owners. They must remain distinct.
#[tokio::test]
async fn ever_depositors_chart_uses_cumulative_not_current_depositors() {
    let store = test_store();
    let mut first = total_deposit_snapshot("2024-01-15", 123_00000000, 7);
    first.cumulative_depositors = 70;
    put_total_deposit_snapshot(&store, &first);
    let mut second = total_deposit_snapshot("2024-01-16", 100_00000000, 6);
    second.cumulative_depositors = 71;
    put_total_deposit_snapshot(&store, &second);
    let mut partial = total_deposit_snapshot("2024-01-17", 105_00000000, 8);
    partial.cumulative_depositors = 72;
    put_total_deposit_snapshot(&store, &partial);
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-17");
    let app = create_router(test_config(store)).await;

    let (status, ever) = get_json(&app, "/dao/charts/ever-depositors").await;
    assert_eq!(status, StatusCode::OK, "{ever}");
    assert_eq!(
        ever["data"],
        serde_json::json!([
            {"date": "2024-01-15", "value": "70", "value2": null},
            {"date": "2024-01-16", "value": "71", "value2": null}
        ])
    );
    assert_eq!(ever["yAxisLabel"], "Ever-deposited locks");
    write_dao_fixture("dao-ever-depositors-api-producer", &ever);

    let (status, current) = get_json(&app, "/dao/charts/total-deposit").await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["data"][1]["value2"], "6");
}

#[tokio::test]
async fn ever_depositors_chart_rejects_decreasing_persisted_count() {
    let store = test_store();
    let mut first = total_deposit_snapshot("2024-01-15", 123_00000000, 7);
    first.cumulative_depositors = 70;
    put_total_deposit_snapshot(&store, &first);
    let mut second = total_deposit_snapshot("2024-01-16", 100_00000000, 6);
    second.cumulative_depositors = 69;
    put_total_deposit_snapshot(&store, &second);
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-17");
    let app = create_router(test_config(store)).await;

    let (status, response) = get_json(&app, "/dao/charts/ever-depositors").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{response}");
    assert!(response["message"].as_str().unwrap().contains("2024-01-16"));
}

#[tokio::test]
async fn ever_depositors_chart_rejects_missing_ever_count_below_current() {
    let store = test_store();
    let mut snapshot = total_deposit_snapshot("2024-01-15", 123_00000000, 7);
    // A legacy/defaulted cumulative field must not become a plausible zero.
    snapshot.cumulative_depositors = 0;
    put_total_deposit_snapshot(&store, &snapshot);
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let app = create_router(test_config(store)).await;

    let (status, response) = get_json(&app, "/dao/charts/ever-depositors").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{response}");
    let message = response["message"].as_str().unwrap();
    assert!(message.contains("2024-01-15"), "{message}");
    assert!(message.contains("current=7"), "{message}");
}

#[tokio::test]
async fn ever_depositors_chart_carries_only_certified_blockless_days() {
    let store = test_store();
    seed_sparse_daily_chart_history(&store);
    let app = create_router(test_config(store)).await;

    let (status, response) = get_json(&app, "/dao/charts/ever-depositors").await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let expected: Vec<_> = (12..=24)
        .map(|day| {
            serde_json::json!({
                "date": format!("2020-05-{day:02}"),
                "value": if day < 22 { "7" } else { "8" },
                "value2": null
            })
        })
        .collect();
    assert_eq!(response["data"], serde_json::json!(expected));
}

fn put_total_deposit_snapshot(store: &CkbadgerStore, snapshot: &DaoDailySnapshot) {
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        snapshot.date.replace('-', "").as_bytes(),
    );
    store
        .put_cf(
            store.cf_stats_dao(),
            &key,
            &bincode::serialize(snapshot).unwrap(),
        )
        .unwrap();
}

#[tokio::test]
async fn total_deposit_chart_fills_only_verified_blockless_days() {
    let store = test_store();
    seed_sparse_daily_chart_history(&store);
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/charts/total-deposit").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let expected: Vec<_> = (12..=24)
        .map(|day| {
            let (value, depositors) = if day < 22 {
                ("123.00000001", "7")
            } else {
                ("120.00000002", "8")
            };
            serde_json::json!({
                "date": format!("2020-05-{day:02}"),
                "value": value,
                "value2": depositors,
            })
        })
        .collect();
    assert_eq!(json["data"], serde_json::json!(expected));
}

#[tokio::test]
async fn total_deposit_chart_rejects_block_bearing_gaps_including_trailing_days() {
    for (missing_day, blocks, first_block) in [
        (
            "2020-05-15",
            vec![
                (0, 1_589_276_230_000),
                (1, utc_ms("2020-05-15T04:00:00Z")),
                (2, 1_590_137_711_584),
                (3, utc_ms("2020-05-25T04:00:00Z")),
            ],
            1,
        ),
        (
            "2020-05-24",
            vec![
                (0, 1_589_276_230_000),
                (1, 1_590_137_711_584),
                (2, utc_ms("2020-05-24T04:00:00Z")),
                (3, utc_ms("2020-05-25T04:00:00Z")),
            ],
            2,
        ),
    ] {
        let store = test_store();
        seed_sparse_daily_chart_history(&store);
        seed_block_headers_at(&store, &blocks);
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(&app, "/dao/charts/total-deposit").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
        let message = json["message"].as_str().unwrap();
        assert!(message.contains(missing_day), "{json}");
        assert!(
            message.contains(&format!("first_block={first_block}")),
            "{json}"
        );
    }
}

/// Regression (STATS-013): the snapshot of the indexed tip block's day is a
/// running value, so the daily total-deposit chart must stop at the day before
/// it — the homepage and the DAO card derive daily changes from consecutive
/// points, and the last one was a partial day (2026-10-24 on the live stack).
#[tokio::test]
async fn test_total_deposit_chart_excludes_the_in_progress_tip_day() {
    let store = test_store();
    put_total_deposit_snapshot(
        &store,
        &total_deposit_snapshot("2024-01-15", 123_00000000, 7),
    );
    put_total_deposit_snapshot(
        &store,
        &total_deposit_snapshot("2024-01-16", 100_00000000, 8),
    );
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(&app, "/dao/charts/total-deposit").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["data"],
        serde_json::json!([{ "date": "2024-01-15", "value": "123", "value2": "7" }]),
        "the tip block's day is in progress and must not be charted: {json}"
    );
}

/// Regression (STATS-014): the daily-deposit, daily-depositors and
/// circulation-ratio charts charted the partial current day. They follow the
/// same median-time rule as total-deposit now.
#[tokio::test]
async fn test_dao_daily_charts_exclude_in_progress_days() {
    let store = test_store();
    seed_genesis_baseline(&store);
    for (date, cumulative, deposits) in [
        ("2024-01-15", 100_00000000, 1),
        ("2024-01-16", 300_00000000, 3),
    ] {
        let mut snapshot = total_deposit_snapshot(date, cumulative, deposits);
        snapshot.new_deposits = deposits;
        snapshot.daily_depositor_addresses = deposits;
        snapshot.total_issuance = 3_400_000_000_000_000_000;
        put_total_deposit_snapshot(&store, &snapshot);
    }
    seed_tip_block_on_utc8_day(&store, 10, "2024-01-16");
    let app = create_router(test_config(store)).await;

    for path in [
        "/dao/charts/daily-deposit",
        "/dao/charts/daily-depositors",
        "/dao/charts/circulation-ratio",
    ] {
        let (status, json) = get_json(&app, path).await;
        assert_eq!(status, StatusCode::OK, "{path}: {json}");
        let dates: Vec<&str> = json["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| point["date"].as_str().unwrap())
            .collect();
        assert_eq!(dates, vec!["2024-01-15"], "{path}: {json}");
    }
}

#[tokio::test]
async fn test_dao_deposits_cursor_pagination_descending() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xA1; 32], 0i16, 30i64),
        (vec![0xA2; 32], 0i16, 20i64),
        (vec![0xA3; 32], 0i16, 10i64),
    ];
    for (tx_hash, output_index, block_number) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, output_index),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                deposit_chain_time_ms: 0,
                lock_script_hash: vec![0x22; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
    let next_cursor = json["nextCursor"].as_str().unwrap();
    assert!(next_cursor.starts_with("0x"));
    assert_eq!(json["hasMore"], true);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=2&cursor={}",
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 10);
    assert!(json["nextCursor"].is_null());
    assert_eq!(json["hasMore"], false);
}

#[tokio::test]
async fn test_dao_deposit_listing_rejects_missing_deposit_header() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0xA1; 32], 0),
        &DaoDepositCacheEntry {
            capacity: 100_00000000,
            occupied_capacity: 50_00000000,
            deposit_block_number: 30,
            deposit_timestamp: 0,
            deposit_chain_time_ms: 0,
            lock_script_hash: vec![0x22; 32],
            deposit_ar: 1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
            withdraw_to_output_index: None,
            compensation: None,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/dao/deposits?limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("missing deposit header"));
}

#[tokio::test]
async fn test_dao_deposits_cursor_pagination_keeps_same_block_rows() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xD1; 32], 0i16, 30i64),
        (vec![0xD2; 32], 1i16, 30i64),
        (vec![0xD3; 32], 2i16, 30i64),
        (vec![0xD4; 32], 0i16, 20i64),
    ];
    for (tx_hash, output_index, block_number) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, output_index),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                deposit_chain_time_ms: 0,
                lock_script_hash: vec![0x33; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 30);
    let next_cursor = json["nextCursor"].as_str().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=2&cursor={}",
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
}

#[tokio::test]
async fn test_dao_deposits_status_filter_uses_descending_order() {
    let store = test_store();
    let lock_a = vec![0x11; 32];
    let lock_b = vec![0x22; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xB1; 32], 30i64, 1i16, lock_a.clone()),
        (vec![0xB2; 32], 20i64, 1i16, lock_b.clone()),
        (vec![0xB3; 32], 10i64, 0i16, lock_a.clone()),
    ];
    for (tx_hash, block_number, status, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                deposit_chain_time_ms: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=10&status=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
}

/// A phase-1 withdraw request has already frozen its compensation at the
/// request AR — the value is deterministic and the aggregates
/// (`/dao/statistics`, the secondary-issuance chart) are computed from it. The
/// deposit listing must serve that same number instead of `null`.
#[tokio::test]
async fn test_dao_deposits_serve_frozen_compensation_for_pending_withdraw_requests() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    // free = 202 - 102 = 100 CKB; compensation = 100 * 200/100 - 100 = 100 CKB.
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0xC1; 32], 0),
        &DaoDepositCacheEntry {
            capacity: 202_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 10,
            deposit_timestamp: 0,
            deposit_chain_time_ms: 0,
            lock_script_hash: vec![0x11; 32],
            deposit_ar: 100,
            status: 1,
            withdraw_request_tx: Some(vec![0xD1; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(20),
            withdraw_request_ar: Some(200),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: Some(102_00000000),
            withdraw_to_output_index: None,
            compensation: None,
        },
    );
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=10&status=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "withdrawing");
    assert_eq!(
        rows[0]["compensation"], "10000000000",
        "pending withdraw requests must serve their frozen compensation"
    );
}

#[tokio::test]
async fn test_dao_deposits_status_cursor_mismatch_returns_bad_request() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xE1; 32], 30i64, 0i16, vec![0x11; 32]),
        (vec![0xE2; 32], 20i64, 0i16, vec![0x22; 32]),
        (vec![0xE3; 32], 10i64, 1i16, vec![0x33; 32]),
    ];
    for (tx_hash, block_number, status, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                deposit_chain_time_ms: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=1&status=0")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=1&status=1&cursor={}",
            cursor
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
        .contains("Invalid dao deposits cursor"));
}

#[tokio::test]
async fn test_dao_deposits_by_lock_hash_cursor_pagination() {
    let store = test_store();
    let lock_a = vec![0x33; 32];
    let lock_b = vec![0x44; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xC1; 32], 30i64, lock_a.clone()),
        (vec![0xC2; 32], 20i64, lock_a.clone()),
        (vec![0xC3; 32], 10i64, lock_b),
    ];
    for (tx_hash, block_number, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                deposit_chain_time_ms: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1",
            hex::encode(&lock_a)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    let next_cursor = json["nextCursor"].as_str().unwrap();
    assert!(next_cursor.starts_with("0x"));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1&cursor={}",
            hex::encode(&lock_a),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 20);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_dao_deposits_by_lock_hash_cursor_mismatch_returns_bad_request() {
    let store = test_store();
    let lock_a = vec![0x55; 32];
    let lock_b = vec![0x66; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xF1; 32], 30i64, lock_a.clone()),
        (vec![0xF2; 32], 20i64, lock_b.clone()),
        (vec![0xF3; 32], 10i64, lock_b.clone()),
    ];
    for (tx_hash, block_number, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 50_00000000,
                deposit_block_number: block_number,
                deposit_timestamp: 0,
                deposit_chain_time_ms: 0,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(10, 0), (20, 0), (30, 0)]);

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1",
            hex::encode(&lock_b)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1&cursor={}",
            hex::encode(&lock_a),
            cursor
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
        .contains("Invalid dao deposits by address cursor"));
}

#[tokio::test]
async fn test_top_depositors_resolves_address_from_lock_script() {
    use ckbadger_store::{DaoTopDepositorEntry, DaoTopDepositors, LockScriptEntry, StoreBatch};

    let store = test_store();

    // secp256k1_blake160 (hash_type=type) with 20-byte args.
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = vec![0x11u8; 20];
    let lock_hash = vec![0xABu8; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &lock_hash,
        &LockScriptEntry {
            code_hash: code_hash.clone(),
            hash_type: 1,
            args: args.clone(),
        },
    );
    batch.commit().unwrap();

    store
        .put_dao_top_depositors(&DaoTopDepositors {
            tip_block_number: 100,
            depositors: vec![DaoTopDepositorEntry {
                lock_script_hash: lock_hash.clone(),
                total_capacity: 100_000_000_000,
                deposit_count: 2,
                total_deposit_ms: 172_800_000,
            }],
        })
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["tipBlockNumber"], 100,
        "the leaderboard must state the block it was built at"
    );

    let expected =
        ckbadger_common::script_to_address(&code_hash, 1, &args, "mainnet").expect("encode");
    assert!(expected.starts_with("ckb1"));
    let dep = &json["depositors"][0];
    assert_eq!(
        dep["lockScriptHash"],
        serde_json::json!(format!("0x{}", hex::encode(&lock_hash)))
    );
    assert_eq!(dep["address"], serde_json::json!(expected));
}

#[tokio::test]
async fn test_top_depositors_address_null_when_lock_script_unknown() {
    use ckbadger_store::{DaoTopDepositorEntry, DaoTopDepositors};

    let store = test_store();
    store
        .put_dao_top_depositors(&DaoTopDepositors {
            tip_block_number: 100,
            depositors: vec![DaoTopDepositorEntry {
                lock_script_hash: vec![0xCDu8; 32],
                total_capacity: 42,
                deposit_count: 1,
                total_deposit_ms: 0,
            }],
        })
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["depositors"][0]["address"].is_null());
}

/// Seed a block header so `get_sync_tip_block()` reports a committed tip.
fn seed_tip_header(store: &Arc<CkbadgerStore>, block_number: i64) {
    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        block_number,
        &CachedBlockHeader {
            hash: vec![0xA5; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();
}

fn seed_dao_daily_snapshot(
    store: &Arc<CkbadgerStore>,
    date: &str,
    cumulative_deposit_amount: i128,
    new_deposits: i64,
) {
    let key_date = date.replace('-', "");
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        key_date.as_bytes(),
    );
    let snapshot = DaoDailySnapshot {
        date: date.to_string(),
        total_deposited: cumulative_deposit_amount,
        depositors_count: new_deposits,
        new_deposits,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount,
        total_issuance: 1,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
        frozen_phase1_compensation: 0,
        cumulative_depositors: new_deposits,
        daily_depositor_addresses: new_deposits,
        protocol_deposited: Some(cumulative_deposit_amount),
    };
    store
        .put_stats_key(&key, &bincode::serialize(&snapshot).unwrap())
        .unwrap();
}

/// The DAO singletons are written by the indexer after every batch commit and
/// after every reorg rollback. Their absence at a synced tip is an invariant
/// violation, not an empty leaderboard: masking it with a default-empty value
/// is exactly what made a ~6s store gap look like 35-40s of "no depositors".
#[tokio::test]
async fn test_top_depositors_fails_fast_when_singleton_missing_at_synced_tip() {
    let store = test_store();
    seed_tip_header(&store, 10);

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["error"], "internal_error");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("dao_top_depositors") && message.contains("10"),
        "error must name the missing singleton and the tip block: {message}"
    );
}

/// Before the indexer has committed anything there is no singleton yet. That is
/// a startup state that resolves itself, so it must be the explicit
/// `initializing` state rather than a 500 or a fabricated empty list.
#[tokio::test]
async fn test_top_depositors_reports_initializing_before_first_block() {
    let store = test_store();

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/top-depositors").await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json["error"], "initializing");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("dao_top_depositors"));
}

/// The daily chart is the day-over-day delta of a cumulative series, so the
/// first snapshot day's delta is "cumulative total minus nothing". Deriving it
/// from `windows(2)` silently drops launch day, hiding the genesis day's
/// deposits from the chart entirely.
#[tokio::test]
async fn test_daily_deposit_chart_includes_first_snapshot_day() {
    let store = test_store();
    seed_dao_daily_snapshot(&store, "2019-11-16", 3_715_755_618_324_833, 33);
    seed_dao_daily_snapshot(&store, "2019-11-17", 3_815_755_618_324_833, 77);
    // The indexed tip is on the next day, so both snapshot days are complete.
    seed_tip_block_on_utc8_day(&store, 1_300, "2019-11-18");

    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/charts/daily-deposit").await;
    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2, "genesis day must be present, got {:?}", data);
    assert_eq!(data[0]["date"], "2019-11-16");
    assert_eq!(data[0]["value"], "37157556.18324833");
    assert_eq!(data[0]["value2"], "33");
    assert_eq!(data[1]["date"], "2019-11-17");
    assert_eq!(data[1]["value"], "1000000");
    assert_eq!(data[1]["value2"], "44");
}

// ---------------------------------------------------------------------------
// Lane R2b-a · DAO page read-path additions (DAO-8, DAO-10, DAO-12, DAO-16).
// ---------------------------------------------------------------------------

/// A DAO statistics singleton at `tip` whose rolling-24h baseline is
/// `baseline`, with S = unclaimed + burnt exactly.
fn r2b_dao_latest(tip: i64, baseline: Option<i64>) -> ckbadger_store::DaoLatestStatistics {
    ckbadger_store::DaoLatestStatistics {
        tip_block_number: tip,
        total_deposited: 200_00000000,
        total_depositors: 1,
        active_deposits: 1,
        deposit_cells_count: 1,
        withdrawal_request_cells_count: 0,
        total_compensation_paid: 0,
        unclaimed_compensation: 13_00000000,
        average_deposit_days: Some("1 days".to_string()),
        estimated_apc: "2.00".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 29_00000000,
        pending_withdrawal_capacity: 0,
        dao_capacity: 0,
        dao_free_capacity: 0,
        depositor_allocation_weight: 0,
        tip_total_issuance: 0,
        tip_secondary_pool: 42_00000000,
        tip_occupied_capacity: 0,
        rolling_24h: baseline.map(
            |baseline_block_number| ckbadger_store::types::DaoRolling24h {
                baseline_block_number,
                cutoff_chain_time_ms: 1_699_913_600_000,
                deposited_change: 0,
                depositors_change: 0,
                claimed_compensation_change: 0,
                unclaimed_compensation_change: 0,
            },
        ),
    }
}

fn r2b_put_dao_latest(store: &Arc<CkbadgerStore>, latest: &ckbadger_store::DaoLatestStatistics) {
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    store
        .put_stats_key(&key, &bincode::serialize(latest).unwrap())
        .unwrap();
}

/// A DAO transaction at `block:index` carrying `actions` (`"deposit"` /
/// `"withdraw_request"` / `"withdraw_complete"`), indexed in the DAO bucket.
fn r2b_put_dao_tx(store: &Arc<CkbadgerStore>, block: i64, index: i32, actions: &[&str]) {
    let tx_hash = {
        let mut hash = [0xd0u8; 32];
        hash[..8].copy_from_slice(&block.to_be_bytes());
        hash[8..12].copy_from_slice(&index.to_be_bytes());
        hash
    };
    let mut tx = make_test_tx_actions(&[0x51; 32], &tx_hash, &[0xbb; 32], block, index, -1, 0);
    tx.protocol_actions = actions
        .iter()
        .map(|action| {
            ckbadger_store::types::ProtocolAction::new(
                "dao",
                *action,
                serde_json::json!({"capacity": 10_000_000_000i64, "lockHash": "51".repeat(32)}),
            )
        })
        .collect();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_actions(&tx);
    batch.commit().unwrap();
}

/// DAO-10 / DAO-16: the statistics say how many deposits went in and how
/// many withdrawal requests came out over the same rolling 24 h their
/// deltas cover — blocks `(baseline, tip]` of the singleton — and carry the
/// singleton's own S, so "S = still owed + burnt" is stated from one row at
/// one block.
#[tokio::test]
async fn dao_statistics_count_24h_deposits_and_requests_and_carry_their_own_s() {
    let store = test_store();
    seed_genesis_baseline(&store);
    seed_tip_header(&store, 21);
    r2b_put_dao_latest(&store, &r2b_dao_latest(20, Some(17)));
    r2b_put_dao_tx(&store, 17, 1, &["deposit"]); // the baseline block: before the window
    r2b_put_dao_tx(&store, 18, 1, &["deposit", "deposit"]);
    r2b_put_dao_tx(&store, 19, 2, &["withdraw_request", "deposit"]);
    r2b_put_dao_tx(&store, 19, 3, &["withdraw_complete"]);
    r2b_put_dao_tx(&store, 21, 1, &["deposit"]); // past the singleton's block
    let app = create_router(test_config(store.clone())).await;
    let (status, json) = get_json(&app, "/dao/statistics").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["deposits24h"], 3, "{json}");
    assert_eq!(json["withdrawalRequests24h"], 1, "{json}");
    assert_eq!(json["tipSecondaryPool"], "4200000000", "{json}");
    assert_eq!(
        json["tipSecondaryPool"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap(),
        json["unclaimedCompensation"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap()
            + json["burnt"].as_str().unwrap().parse::<i128>().unwrap()
    );

    // No complete 24 h baseline yet: no window, no counts.
    r2b_put_dao_latest(&store, &r2b_dao_latest(20, None));
    let (status, json) = get_json(&app, "/dao/statistics").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["deposits24h"].is_null(), "{json}");
    assert!(json["withdrawalRequests24h"].is_null(), "{json}");
}

/// DAO-12: every current depositor, ranked by phase-0 capacity, paged — the
/// same ranking `/dao/evidence`'s concentration is built from, at the same
/// tip, with the tip as its anchor.
#[tokio::test]
async fn dao_depositors_page_the_whole_ranking_at_the_evidence_tip() {
    let app = create_router(test_config(dao_evidence_fixture())).await;
    let (status, first) = get_json(&app, "/dao/depositors?limit=1").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first["anchor"],
        serde_json::json!({"blockNumber": 5, "blockHash": format!("0x{}", "05".repeat(32))}),
        "{first}"
    );
    assert_eq!(first["total"], 2, "{first}");
    let row = &first["data"][0];
    assert_eq!(row["rank"], 1, "{first}");
    assert_eq!(row["capacity"], "30000000000");
    assert_eq!(row["depositCount"], 1);
    assert_eq!(row["sharePercent"], "60.00");
    assert!(
        row["address"].as_str().unwrap().starts_with("ckb1"),
        "{first}"
    );
    let cursor = first["nextCursor"]
        .as_str()
        .expect("second page")
        .to_string();
    let (status, second) =
        get_json(&app, &format!("/dao/depositors?limit=1&cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["data"][0]["rank"], 2, "{second}");
    assert_eq!(second["data"][0]["capacity"], "20000000000");
    assert!(second["nextCursor"].is_null(), "{second}");
    // The evidence's concentration is the same ranking.
    let (_, evidence) = get_json(&app, "/dao/evidence").await;
    assert_eq!(
        evidence["concentration"]["owners"][0]["lockScriptHash"],
        row["lockScriptHash"]
    );
    // A cursor from another tip's ranking does not page this one.
    let (status, json) = get_json(&app, "/dao/depositors?limit=1&cursor=4:1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}

/// DAO-8: the accumulated rate at the end of every complete UTC+8 day — the
/// last block at or before the day's end, exactly as its header holds it —
/// from genesis through the last complete day.
#[tokio::test]
async fn dao_ar_history_is_the_header_ar_at_each_complete_day_end() {
    let store = test_store();
    let hour = 3_600_000i64;
    let day = 24 * hour;
    let start = 1_700_000_000_000i64; // 2023-11-15 06:13:20 UTC+8
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, at, ar) in [
        (0u8, start, 10_000_000_000_000_000u64),
        (1, start + hour, 10_001_000_000_000_000),
        (2, start + day, 10_002_000_000_000_000),
        (3, start + 3 * day, 10_003_000_000_000_000),
        (4, start + 4 * day, 10_004_000_000_000_000),
    ] {
        batch.put_block_header(
            i64::from(number),
            &dao_evidence_header(number, (0, i32::from(number), 1800), at, at, ar),
        );
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/dao/ar-history").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    // The series' anchor is the block the newest complete day closes at.
    assert_eq!(
        json["anchor"],
        serde_json::json!({"blockNumber": 3, "blockHash": format!("0x{}", "03".repeat(32))})
    );
    let days: Vec<(String, i64, String)> = json["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["date"].as_str().unwrap().to_string(),
                d["blockNumber"].as_i64().unwrap(),
                d["arIndex"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        days,
        vec![
            ("2023-11-15".into(), 1, "1.0001000000000000".into()),
            ("2023-11-16".into(), 2, "1.0002000000000000".into()),
            // No block that day: the rate at its end is block 2's.
            ("2023-11-17".into(), 2, "1.0002000000000000".into()),
            ("2023-11-18".into(), 3, "1.0003000000000000".into()),
        ],
        "{json}"
    );
    assert_eq!(json["days"][0]["ar"], "10001000000000000");
}

/// DAO-8: the calculator states the occupied capacity it assumed (a
/// standard 102-byte secp256k1 DAO cell) and the free capacity compensation
/// accrues on.
#[tokio::test]
async fn dao_calculator_states_the_free_capacity_it_assumed() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, ar) in [
        (10u8, 10_000_000_000_000_000u64),
        (20, 10_100_000_000_000_000),
    ] {
        batch.put_block_header(
            i64::from(number),
            &dao_evidence_header(number, (0, 0, 1), 1_700_000_000_000, 1_700_000_000_000, ar),
        );
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        "/dao/calculator?capacity=1000000000000&deposit_block=10&withdraw_block=20",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["occupiedCapacity"], "10200000000", "{json}");
    assert_eq!(json["freeCapacity"], "989800000000", "{json}");
    // compensation = free × (AR_w / AR_d − 1) = 989_800_000_000 × 1%.
    assert_eq!(json["estimatedCompensation"], "9898000000", "{json}");
}
