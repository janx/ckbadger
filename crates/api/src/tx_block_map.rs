use ckbadger_store::CkbadgerStore;
use std::collections::HashMap;

/// Look up block_number for a single transaction hash.
pub fn get_block_number_for_tx(
    store: &CkbadgerStore,
    tx_hash: &[u8],
) -> anyhow::Result<Option<i64>> {
    Ok(store
        .get_tx_location(tx_hash)?
        .map(|(block_num, _)| block_num))
}

/// Look up block_numbers for multiple transaction hashes.
pub fn get_block_numbers_for_txs(
    store: &CkbadgerStore,
    tx_hashes: &[Vec<u8>],
) -> anyhow::Result<HashMap<Vec<u8>, i64>> {
    let mut result = HashMap::with_capacity(tx_hashes.len());
    for tx_hash in tx_hashes {
        if let Some((block_num, _)) = store.get_tx_location(tx_hash)? {
            result.insert(tx_hash.clone(), block_num);
        }
    }
    Ok(result)
}
