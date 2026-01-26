#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;
use std::collections::HashMap;

async fn get_address_balance(pool: &PgPool, lock_hash: &[u8]) -> Option<(String, i32, i64)> {
    sqlx::query_as::<_, (String, i32, i64)>(
        "SELECT balance::TEXT, live_cells_count, transactions_count FROM address_balances WHERE lock_script_hash = $1",
    )
    .bind(lock_hash)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_single_address_single_tx(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash = vec![0x01u8; 32];
    let tx_hash = vec![0xAAu8; 32];

    let mut changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = HashMap::new();
    changes.insert(
        lock_hash.clone(),
        (
            1000_00000000, // balance_change: +1000 CKB
            1,             // live_cells_change: +1
            1,             // total_cells_change: +1
            1,             // tx_count: 1 transaction
            100,           // block_number
            &tx_hash,      // tx_hash
        ),
    );

    writer
        .update_address_balances_batch(&changes)
        .await
        .unwrap();

    let (balance, live_cells, tx_count) = get_address_balance(&pool, &lock_hash).await.unwrap();
    assert_eq!(balance, "100000000000");
    assert_eq!(live_cells, 1);
    assert_eq!(tx_count, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_address_in_multiple_txs_same_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash = vec![0x02u8; 32];
    let tx_hash = vec![0xBBu8; 32];

    // Simulate: address appears in 3 transactions in one batch
    // - Tx1: receives 100 CKB (1 cell created)
    // - Tx2: receives 200 CKB (1 cell created)
    // - Tx3: sends 50 CKB (1 cell consumed, 1 cell created as change)
    // Net: balance +250, live_cells +2 (3 created - 1 consumed), tx_count = 3
    let mut changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = HashMap::new();
    changes.insert(
        lock_hash.clone(),
        (
            250_00000000, // balance_change: +250 CKB
            2,            // live_cells_change: +2 (3 created - 1 consumed)
            3,            // total_cells_change: +3 (all created cells)
            3,            // tx_count: 3 transactions
            100,          // block_number
            &tx_hash,     // tx_hash
        ),
    );

    writer
        .update_address_balances_batch(&changes)
        .await
        .unwrap();

    let (balance, live_cells, tx_count) = get_address_balance(&pool, &lock_hash).await.unwrap();
    assert_eq!(balance, "25000000000");
    assert_eq!(live_cells, 2);
    assert_eq!(tx_count, 3); // This was the bug: it would have been 1 before the fix
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_multiple_batches_accumulate_correctly(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash = vec![0x03u8; 32];
    let tx_hash1 = vec![0xCCu8; 32];
    let tx_hash2 = vec![0xDDu8; 32];

    // Batch 1: address in 2 transactions
    let mut changes1: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = HashMap::new();
    changes1.insert(
        lock_hash.clone(),
        (
            100_00000000, // +100 CKB
            2,            // +2 live cells
            2,            // +2 total cells
            2,            // 2 transactions
            100,
            &tx_hash1,
        ),
    );
    writer
        .update_address_balances_batch(&changes1)
        .await
        .unwrap();

    // Batch 2: address in 3 more transactions
    let mut changes2: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = HashMap::new();
    changes2.insert(
        lock_hash.clone(),
        (
            -50_00000000, // -50 CKB (spent some)
            -1,           // -1 live cell (consumed 2, created 1)
            1,            // +1 total cell
            3,            // 3 transactions
            200,
            &tx_hash2,
        ),
    );
    writer
        .update_address_balances_batch(&changes2)
        .await
        .unwrap();

    let (balance, live_cells, tx_count) = get_address_balance(&pool, &lock_hash).await.unwrap();
    assert_eq!(balance, "5000000000"); // 100 - 50 = 50 CKB
    assert_eq!(live_cells, 1); // 2 - 1 = 1
    assert_eq!(tx_count, 5); // 2 + 3 = 5 transactions total
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_live_cells_cannot_go_negative(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash = vec![0x04u8; 32];
    let tx_hash = vec![0xEEu8; 32];

    // Edge case: try to consume more cells than exist
    // The SQL uses GREATEST(0, ...) to prevent negative values
    let mut changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = HashMap::new();
    changes.insert(
        lock_hash.clone(),
        (
            -100_00000000, // balance can go negative (debt)
            -5,            // try to go negative on live_cells
            0,
            1,
            100,
            &tx_hash,
        ),
    );

    writer
        .update_address_balances_batch(&changes)
        .await
        .unwrap();

    let (balance, live_cells, tx_count) = get_address_balance(&pool, &lock_hash).await.unwrap();
    assert_eq!(balance, "-10000000000"); // balance can be negative
    assert_eq!(live_cells, 0); // live_cells clamped to 0
    assert_eq!(tx_count, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_multiple_addresses_in_same_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash_a = vec![0x05u8; 32];
    let lock_hash_b = vec![0x06u8; 32];
    let tx_hash = vec![0xFFu8; 32];

    // Two addresses in same batch, each with different tx counts
    let mut changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = HashMap::new();
    changes.insert(
        lock_hash_a.clone(),
        (
            500_00000000, // Address A: +500 CKB
            3,            // +3 live cells
            3,            // +3 total cells
            2,            // appeared in 2 transactions
            100,
            &tx_hash,
        ),
    );
    changes.insert(
        lock_hash_b.clone(),
        (
            -200_00000000, // Address B: -200 CKB (sent to A)
            -1,            // -1 live cell
            1,             // +1 total cell (change output)
            1,             // appeared in 1 transaction
            100,
            &tx_hash,
        ),
    );

    writer
        .update_address_balances_batch(&changes)
        .await
        .unwrap();

    let (balance_a, live_a, tx_a) = get_address_balance(&pool, &lock_hash_a).await.unwrap();
    assert_eq!(balance_a, "50000000000");
    assert_eq!(live_a, 3);
    assert_eq!(tx_a, 2);

    let (balance_b, live_b, tx_b) = get_address_balance(&pool, &lock_hash_b).await.unwrap();
    assert_eq!(balance_b, "-20000000000");
    assert_eq!(live_b, 0); // clamped from -1
    assert_eq!(tx_b, 1);
}
