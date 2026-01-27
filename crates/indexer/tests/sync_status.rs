use chrono::{TimeZone, Utc};
use sqlx::PgPool;

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::block::ParsedBlock;
use ckbadger_indexer::MIGRATOR;

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_sync_tip_and_block_hash(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let (tip, hash) = writer.get_sync_tip().await.unwrap();
    assert_eq!(tip, 0);
    assert!(hash.is_none());

    let expected_hash = vec![0x11u8; 32];
    sqlx::query("UPDATE sync_status SET tip_block_number = $1, tip_block_hash = $2 WHERE id = 1")
        .bind(42i64)
        .bind(expected_hash.clone())
        .execute(&pool)
        .await
        .unwrap();

    let (tip, hash) = writer.get_sync_tip().await.unwrap();
    assert_eq!(tip, 42);
    assert_eq!(hash, Some(expected_hash));
    assert!(!writer.has_unresolved_deep_fork().await.unwrap());

    let block = ParsedBlock {
        number: 7,
        hash: vec![0xAAu8; 32],
        parent_hash: vec![0xBBu8; 32],
        timestamp: Utc.timestamp_opt(1_704_067_200, 0).single().unwrap(),
        version: 0,
        compact_target: 0,
        transactions_count: 0,
        proposals_count: 0,
        uncles_count: 0,
        epoch_number: 0,
        epoch_index: 0,
        epoch_length: 0,
        dao: vec![0u8; 32],
        nonce: vec![0u8; 8],
        extra_hash: vec![0u8; 32],
        proposals_hash: vec![0u8; 32],
        transactions_root: vec![0u8; 32],
        uncles_hash: vec![0u8; 32],
        proposals: vec![],
    };

    writer.insert_block(&block, 0).await.unwrap();

    let fetched_hash = writer.get_block_hash_at_height(7).await.unwrap();
    assert_eq!(fetched_hash, Some(vec![0xAAu8; 32]));
    assert!(writer.get_block_hash_at_height(999).await.unwrap().is_none());
}
