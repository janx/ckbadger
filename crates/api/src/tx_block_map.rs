use sqlx::PgPool;
use std::collections::HashMap;

/// Look up block_number for a single transaction hash.
/// Returns None if tx_hash not found in tx_block_map (fallback needed).
pub async fn get_block_number_for_tx(
    pool: &PgPool,
    tx_hash: &[u8],
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT block_number FROM tx_block_map WHERE tx_hash = $1")
        .bind(tx_hash)
        .fetch_optional(pool)
        .await
}

/// Look up block_numbers for multiple transaction hashes.
/// Returns a HashMap mapping tx_hash -> block_number for found entries.
pub async fn get_block_numbers_for_txs(
    pool: &PgPool,
    tx_hashes: &[Vec<u8>],
) -> Result<HashMap<Vec<u8>, i64>, sqlx::Error> {
    if tx_hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Vec<u8>, i64)> =
        sqlx::query_as("SELECT tx_hash, block_number FROM tx_block_map WHERE tx_hash = ANY($1)")
            .bind(tx_hashes)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}
