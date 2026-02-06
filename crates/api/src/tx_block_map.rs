use std::collections::HashMap;

use anyhow::Result;

use crate::db::DbPool;

pub async fn get_block_number_by_tx_hash(_pool: &DbPool, _tx_hash: &[u8]) -> Result<Option<i64>> {
    Ok(None)
}

pub async fn get_block_numbers_by_tx_hashes(
    _pool: &DbPool,
    _tx_hashes: &[Vec<u8>],
) -> Result<HashMap<Vec<u8>, i64>> {
    Ok(HashMap::new())
}
