//! Integration test: verify that a partial-day rollback correctly repairs
//! the cutoff-date DAO daily snapshot.
//!
//! Scenario: two blocks on 2026-04-08 each contain one DAO deposit.
//! Pre-reorg snapshot reflects both. Rollback one block. After rollback,
//! the snapshot should reflect ONLY the surviving block's deposit — NOT
//! yesterday's snapshot value (which is what the buggy code produces).

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{CachedBlockHeader, DaoDailySnapshot, DaoDepositCacheEntry};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

/// Build a 32-byte DAO field (C | AR | S | U), each as little-endian u64.
fn dao_field(c: u64, ar: u64, s: u64, u: u64) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    v[0..8].copy_from_slice(&c.to_le_bytes());
    v[8..16].copy_from_slice(&ar.to_le_bytes());
    v[16..24].copy_from_slice(&s.to_le_bytes());
    v[24..32].copy_from_slice(&u.to_le_bytes());
    v
}

fn outpoint_bytes(tx_hash: &[u8], output_index: i16) -> Vec<u8> {
    let mut k = Vec::with_capacity(34);
    k.extend_from_slice(tx_hash);
    k.extend_from_slice(&output_index.to_be_bytes());
    k
}

/// Partial-day rollback: blocks 100 and 101 are both on 2026-04-08.
/// Each creates one DAO deposit from a distinct lock.
/// Rollback to block 100 (drops block 101).
/// Verify snapshot for 2026-04-08 reflects only block 100's deposit.
#[test]
fn test_partial_day_rollback_recomputes_dao_snapshot() {
    let store = setup_store();

    // 2026-04-08 00:00 UTC+8 = 2026-04-07 16:00 UTC = 1775577600000 ms
    let day_0408_t1_ms: i64 = 1_775_577_600_000 + 10_000;
    let day_0408_t2_ms: i64 = 1_775_577_600_000 + 20_000;

    let mut batch = StoreBatch::new(&store);

    // Block 99: the last block of 2026-04-07 (yesterday). Has the baseline
    // DAO field used to populate yesterday's snapshot.
    let day_0407_end_ms: i64 = 1_775_577_600_000 - 1000;
    batch.put_block_header(
        99,
        &CachedBlockHeader {
            hash: vec![0x63; 32],
            parent_hash: vec![0x62; 32],
            timestamp: day_0407_end_ms,
            epoch_number: 1,
            epoch_index: 99,
            epoch_length: 1800,
            dao: dao_field(
                1_000_000_000_000_000_000,
                10_000_000_000_000_000,
                0,
                100_000_000_000_000_000,
            ),
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        },
    );

    // Block 100: 2026-04-08 first block with deposit #1
    batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0x64; 32],
            parent_hash: vec![0x63; 32],
            timestamp: day_0408_t1_ms,
            epoch_number: 2,
            epoch_index: 0,
            epoch_length: 1800,
            dao: dao_field(
                1_100_000_000_000_000_000, // C
                10_050_000_000_000_000,    // AR
                5_000_000,                 // S
                110_000_000_000_000_000,   // U
            ),
            transactions_count: 2,
            uncles_count: 0,
            cycles: None,
        },
    );

    // Block 101: 2026-04-08 second block with deposit #2
    batch.put_block_header(
        101,
        &CachedBlockHeader {
            hash: vec![0x65; 32],
            parent_hash: vec![0x64; 32],
            timestamp: day_0408_t2_ms,
            epoch_number: 2,
            epoch_index: 1,
            epoch_length: 1800,
            dao: dao_field(
                1_200_000_000_000_000_000, // C
                10_100_000_000_000_000,    // AR
                10_000_000,                // S
                120_000_000_000_000_000,   // U
            ),
            transactions_count: 2,
            uncles_count: 0,
            cycles: None,
        },
    );

    // Yesterday's (2026-04-07) snapshot — the recompute starting point.
    let y_snap = DaoDailySnapshot {
        date: "2026-04-07".to_string(),
        total_deposited: 0,
        depositors_count: 0,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 0,
        total_issuance: 1_000_000_000_000_000_000,
        secondary_pool: 0,
        occupied_capacity: 100_000_000_000_000_000,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unmade_dao_interests: 0,
        unclaimed_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: Some(0),
    };
    let y_key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260407");
    batch.put_stats(&y_key, &bincode::serialize(&y_snap).unwrap());

    // Pre-reorg 2026-04-08 snapshot (both deposits counted).
    let pre_snap = DaoDailySnapshot {
        date: "2026-04-08".to_string(),
        total_deposited: 200_00000000, // 200 CKB (2 x 100 CKB deposits)
        depositors_count: 2,
        new_deposits: 2,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 200_00000000,
        total_issuance: 1_200_000_000_000_000_000,
        secondary_pool: 10_000_000,
        occupied_capacity: 120_000_000_000_000_000,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unmade_dao_interests: 0,
        unclaimed_compensation: 0,
        cumulative_depositors: 2,
        daily_depositor_addresses: 2,
        protocol_deposited: Some(200_00000000),
    };
    let s_key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260408");
    batch.put_stats(&s_key, &bincode::serialize(&pre_snap).unwrap());

    // Two DAO deposit entries — one per block, with distinct lock hashes.
    let tx_hash_100: Vec<u8> = vec![0xA0; 32];
    let tx_hash_101: Vec<u8> = vec![0xA1; 32];
    let outpoint_100 = outpoint_bytes(&tx_hash_100, 0);
    let outpoint_101 = outpoint_bytes(&tx_hash_101, 0);

    let entry_100 = DaoDepositCacheEntry {
        capacity: 100_00000000,
        deposit_block_number: 100,
        deposit_timestamp: day_0408_t1_ms,
        lock_script_hash: vec![0xB0; 32],
        deposit_ar: 10_050_000_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };
    let entry_101 = DaoDepositCacheEntry {
        capacity: 100_00000000,
        deposit_block_number: 101,
        deposit_timestamp: day_0408_t2_ms,
        lock_script_hash: vec![0xB1; 32], // distinct lock hash
        deposit_ar: 10_100_000_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };
    batch.put_dao_deposit(&outpoint_100, &entry_100);
    batch.put_dao_deposit(&outpoint_101, &entry_101);
    batch.commit().unwrap();

    // Seed sync_status: tip = 101
    store
        .update_sync_status(|s| {
            s.tip_block_number = 101;
            s.tip_block_hash = vec![0x65; 32];
        })
        .unwrap();

    // ACT: partial-day rollback to block 100 (drops block 101 only).
    store.rollback_to_block(100).unwrap();

    // ASSERT: snapshot for 2026-04-08 should now reflect block 100's deposit
    // ONLY (one deposit of 100 CKB).
    let raw = store
        .get_stats_key(&s_key)
        .expect("get_stats_key succeeded")
        .expect("snapshot for 2026-04-08 must exist after rollback repair");
    let repaired: DaoDailySnapshot = bincode::deserialize(&raw).unwrap();

    assert_eq!(
        repaired.total_deposited,
        100_00000000,
        "total_deposited must reflect the single surviving deposit, got {}",
        repaired.total_deposited
    );
    assert_eq!(repaired.new_deposits, 1, "new_deposits");
    assert_eq!(repaired.depositors_count, 1, "depositors_count");
    assert_eq!(
        repaired.cumulative_deposit_amount,
        100_00000000,
        "cumulative_deposit_amount"
    );
    assert_eq!(
        repaired.daily_depositor_addresses, 1,
        "daily_depositor_addresses"
    );
    assert_eq!(
        repaired.protocol_deposited,
        Some(100_00000000),
        "protocol_deposited"
    );

    // DAO C/S/U must be re-read from block 100's DAO header.
    assert_eq!(
        repaired.total_issuance,
        1_100_000_000_000_000_000,
        "total_issuance must match block 100 DAO field C"
    );
    assert_eq!(
        repaired.occupied_capacity,
        110_000_000_000_000_000,
        "occupied_capacity must match block 100 DAO field U"
    );
    assert_eq!(
        repaired.secondary_pool, 5_000_000,
        "secondary_pool must match block 100 DAO field S"
    );
}

/// Cross-day rollback: block 100 on 2026-04-07 end-of-day, blocks 101 and 102
/// on 2026-04-08 each with one deposit. Rollback to block 100 — drops ALL of
/// 2026-04-08.
/// After rollback, the 2026-04-08 snapshot should be rebuilt by the recompute
/// stage from yesterday's baseline with zero new-day deltas (because no blocks
/// from 2026-04-08 survive). Semantically equivalent to "day didn't happen".
#[test]
fn test_cross_day_rollback_rebuilds_cutoff_date_snapshot() {
    let store = setup_store();

    // 2026-04-08 00:00 UTC+8 = 1_775_577_600_000 ms
    let day_0407_late_ms: i64 = 1_775_577_600_000 - 5_000; // ~5s before midnight UTC+8
    let day_0408_t1_ms: i64 = 1_775_577_600_000 + 10_000;
    let day_0408_t2_ms: i64 = 1_775_577_600_000 + 20_000;

    let mut batch = StoreBatch::new(&store);

    // Block 100: last block of 2026-04-07.
    batch.put_block_header(100, &CachedBlockHeader {
        hash: vec![0x64; 32],
        parent_hash: vec![0x63; 32],
        timestamp: day_0407_late_ms,
        epoch_number: 1,
        epoch_index: 99,
        epoch_length: 1800,
        dao: dao_field(1_000_000_000_000_000_000, 10_000_000_000_000_000, 0, 100_000_000_000_000_000),
        transactions_count: 1,
        uncles_count: 0,
        cycles: None,
    });
    // Block 101: 2026-04-08 first block with deposit #1.
    batch.put_block_header(101, &CachedBlockHeader {
        hash: vec![0x65; 32],
        parent_hash: vec![0x64; 32],
        timestamp: day_0408_t1_ms,
        epoch_number: 2,
        epoch_index: 0,
        epoch_length: 1800,
        dao: dao_field(1_100_000_000_000_000_000, 10_050_000_000_000_000, 5_000_000, 110_000_000_000_000_000),
        transactions_count: 2,
        uncles_count: 0,
        cycles: None,
    });
    // Block 102: 2026-04-08 second block with deposit #2.
    batch.put_block_header(102, &CachedBlockHeader {
        hash: vec![0x66; 32],
        parent_hash: vec![0x65; 32],
        timestamp: day_0408_t2_ms,
        epoch_number: 2,
        epoch_index: 1,
        epoch_length: 1800,
        dao: dao_field(1_200_000_000_000_000_000, 10_100_000_000_000_000, 10_000_000, 120_000_000_000_000_000),
        transactions_count: 2,
        uncles_count: 0,
        cycles: None,
    });

    // Yesterday's (2026-04-07) snapshot — zero baseline.
    let y_snap = DaoDailySnapshot {
        date: "2026-04-07".to_string(),
        total_deposited: 0,
        depositors_count: 0,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 0,
        total_issuance: 1_000_000_000_000_000_000,
        secondary_pool: 0,
        occupied_capacity: 100_000_000_000_000_000,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unmade_dao_interests: 0,
        unclaimed_compensation: 0,
        cumulative_depositors: 0,
        daily_depositor_addresses: 0,
        protocol_deposited: Some(0),
    };
    let y_key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260407");
    batch.put_stats(&y_key, &bincode::serialize(&y_snap).unwrap());

    // Pre-reorg 2026-04-08 snapshot — BOTH deposits counted (this is the value
    // that needs to disappear after rolling back the entire day).
    let pre_snap = DaoDailySnapshot {
        date: "2026-04-08".to_string(),
        total_deposited: 200_00000000,
        depositors_count: 2,
        new_deposits: 2,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 200_00000000,
        total_issuance: 1_200_000_000_000_000_000,
        secondary_pool: 10_000_000,
        occupied_capacity: 120_000_000_000_000_000,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unmade_dao_interests: 0,
        unclaimed_compensation: 0,
        cumulative_depositors: 2,
        daily_depositor_addresses: 2,
        protocol_deposited: Some(200_00000000),
    };
    let s_key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260408");
    batch.put_stats(&s_key, &bincode::serialize(&pre_snap).unwrap());

    // Two DAO deposit entries on blocks 101 and 102.
    let entry_101 = DaoDepositCacheEntry {
        capacity: 100_00000000,
        deposit_block_number: 101,
        deposit_timestamp: day_0408_t1_ms,
        lock_script_hash: vec![0xB0; 32],
        deposit_ar: 10_050_000_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };
    let entry_102 = DaoDepositCacheEntry {
        capacity: 100_00000000,
        deposit_block_number: 102,
        deposit_timestamp: day_0408_t2_ms,
        lock_script_hash: vec![0xB1; 32],
        deposit_ar: 10_100_000_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };
    batch.put_dao_deposit(&outpoint_bytes(&vec![0xA1; 32], 0), &entry_101);
    batch.put_dao_deposit(&outpoint_bytes(&vec![0xA2; 32], 0), &entry_102);
    batch.commit().unwrap();

    store.update_sync_status(|s| {
        s.tip_block_number = 102;
        s.tip_block_hash = vec![0x66; 32];
    }).unwrap();

    // ACT: rollback to block 100 — drops ALL of 2026-04-08.
    store.rollback_to_block(100).unwrap();

    // ASSERT: after recompute, the 2026-04-08 snapshot should show zero
    // day-deltas (no blocks survive on that date). Accept either "absent"
    // (snapshot deleted because no blocks remain on the day) or "present
    // with all day-deltas == 0".
    match store.get_stats_key(&s_key).unwrap() {
        None => {
            // Acceptable: snapshot deleted, no blocks on the day to rebuild from.
        }
        Some(raw) => {
            let snap: DaoDailySnapshot = bincode::deserialize(&raw).unwrap();
            assert_eq!(
                snap.total_deposited, 0,
                "total_deposited must be 0 after full-day rollback, got {}",
                snap.total_deposited
            );
            assert_eq!(snap.new_deposits, 0, "new_deposits must be 0");
            assert_eq!(snap.cumulative_deposit_amount, 0, "cumulative_deposit_amount must be 0");
            assert_eq!(snap.depositors_count, 0, "depositors_count must be 0");
            assert_eq!(snap.withdrawals, 0, "withdrawals must be 0");
            assert_eq!(snap.protocol_deposited, Some(0), "protocol_deposited must be 0");
        }
    }
}
