//! Address balance operations.

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{AddressBalance, LiveCellInfo};

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

impl CkbadgerStore {
    pub fn get_addr_balance(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        match self.get_cf(self.cf_addr_balance(), lock_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_addr_balance_direct(
        &self,
        lock_hash: &[u8],
        balance: &AddressBalance,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(balance)?;
        self.put_cf(self.cf_addr_balance(), lock_hash, &value)
    }

    /// List top addresses by balance (full scan, sorted).
    pub fn top_addresses(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, AddressBalance)>> {
        let iter = self.iterator_cf(self.cf_addr_balance(), IteratorMode::Start);

        let mut all: Vec<(Vec<u8>, AddressBalance)> = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate addr_balance in top_addresses: {}", e)
            })?;
            let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize address balance in top_addresses: lock_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            all.push((key.to_vec(), balance));
        }

        all.sort_by(|a, b| b.1.balance.cmp(&a.1.balance));
        all.truncate(limit);
        Ok(all)
    }

    /// List transactions for an address (newest first, reverse scan).
    pub fn list_addr_txs_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_addr_txs_recent expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        // Seek to the cursor position or end of prefix range, then iterate backwards
        let upper_key = match cursor {
            Some((block_num, tx_idx)) => {
                // Seek to cursor position; skip it in the loop
                crate::keys::encode_addr_tx_key(lock_hash, block_num, tx_idx)
            }
            None => {
                let mut key = Vec::with_capacity(44);
                key.extend_from_slice(lock_hash);
                key.extend_from_slice(&[0xFF; 12]); // max block_num(8) + max tx_idx(4)
                key
            }
        };

        let iter = self.iterator_cf(
            self.cf_addr_txs(),
            rocksdb::IteratorMode::From(&upper_key, rocksdb::Direction::Reverse),
        );

        let mut results = Vec::new();
        let mut skip_first = cursor.is_some();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate addr_txs in list_addr_txs_recent: {}", e)
            })?;
            if !key.starts_with(lock_hash) {
                break;
            }
            if key.len() == 44 {
                let block_num = crate::keys::decode_block_num(&key[32..40]);
                let tx_idx = crate::keys::decode_tx_idx(&key[40..44]);
                if skip_first {
                    skip_first = false;
                    if Some((block_num, tx_idx)) == cursor {
                        continue;
                    }
                }
                results.push((block_num, tx_idx, value.to_vec()));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Rebuild addr_balance from live_cells.
    ///
    /// This heals historical drift caused by previously skipped input-cell consumption.
    /// Uses live_cells as the source of truth for:
    /// - balance
    /// - occupied_capacity
    /// - live_cells_count
    /// - txs_count (from addr_txs index)
    ///
    /// Other fields are re-initialized conservatively.
    pub fn rebuild_addr_balances_from_live_cells(&self) -> anyhow::Result<usize> {
        use std::collections::HashMap;

        struct Agg {
            balance: i128,
            occupied_capacity: i128,
            live_cells_count: i32,
            txs_count: i64,
            first_seen_block: i64,
            first_seen_tx: Vec<u8>,
            last_activity_block: i64,
            last_activity_tx: Vec<u8>,
        }

        let mut agg_by_lock: HashMap<Vec<u8>, Agg> = HashMap::new();
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate live_cells in rebuild_addr_balances_from_live_cells: {}",
                    e
                )
            })?;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: LiveCellInfo = self
                .get_cell_by_outpoint_key(&key)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "failed to load canonical cell during addr balance rebuild: outpoint=0x{}, error={}",
                        bytes_to_hex(&key),
                        e
                    )
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing canonical cell for live marker during addr balance rebuild: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
            let tx_hash = key[..32].to_vec();

            let entry = agg_by_lock
                .entry(info.lock_script_hash.clone())
                .or_insert_with(|| Agg {
                    balance: 0,
                    occupied_capacity: 0,
                    live_cells_count: 0,
                    txs_count: 0,
                    first_seen_block: info.created_at_block,
                    first_seen_tx: tx_hash.clone(),
                    last_activity_block: info.created_at_block,
                    last_activity_tx: tx_hash.clone(),
                });

            entry.balance += info.capacity as i128;
            entry.occupied_capacity += info.occupied_capacity as i128;
            entry.live_cells_count += 1;

            if info.created_at_block < entry.first_seen_block {
                entry.first_seen_block = info.created_at_block;
                entry.first_seen_tx = tx_hash.clone();
            }
            if info.created_at_block > entry.last_activity_block {
                entry.last_activity_block = info.created_at_block;
                entry.last_activity_tx = tx_hash;
            }
        }

        // Rebuild tx count from addr_txs index.
        // Count only addresses that still have live cells after rebuild.
        let iter = self.iterator_cf(self.cf_addr_txs(), rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate addr_txs in rebuild_addr_balances_from_live_cells: {}",
                    e
                )
            })?;
            // Key: lock_hash(32) + block_num(8) + tx_idx(4) = 44
            if key.len() != 44 {
                continue;
            }
            if let Some(entry) = agg_by_lock.get_mut(&key[..32]) {
                entry.txs_count += 1;
            }
        }

        // Clear existing addr_balance CF
        let mut clear_batch = rocksdb::WriteBatch::default();
        let mut cleared = 0usize;
        let iter = self.iterator_cf(self.cf_addr_balance(), IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate addr_balance for clearing in rebuild_addr_balances_from_live_cells: {}",
                    e
                )
            })?;
            clear_batch.delete_cf(self.cf_addr_balance(), &key);
            cleared += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if cleared % 20_000 == 0 {
                self.write_batch(std::mem::take(&mut clear_batch))?;
                clear_batch = rocksdb::WriteBatch::default();
            }
        }
        if !clear_batch.is_empty() {
            self.write_batch(clear_batch)?;
        }

        // Write rebuilt balances
        let mut write_batch = crate::batch::StoreBatch::new(self);
        let mut written = 0usize;
        for (lock_hash, agg) in agg_by_lock {
            if agg.balance < 0 || agg.occupied_capacity < 0 || agg.live_cells_count < 0 {
                anyhow::bail!(
                    "rebuild addr_balance negative aggregate: lock_hash={:?}, balance={}, occupied_capacity={}, live_cells_count={}",
                    lock_hash,
                    agg.balance,
                    agg.occupied_capacity,
                    agg.live_cells_count
                );
            }
            let balance = AddressBalance {
                balance: agg.balance,
                occupied_capacity: agg.occupied_capacity,
                live_cells_count: agg.live_cells_count,
                total_cells_count: i64::from(agg.live_cells_count),
                txs_count: agg.txs_count,
                first_seen_block: agg.first_seen_block,
                first_seen_tx: agg.first_seen_tx,
                last_activity_block: agg.last_activity_block,
                last_activity_tx: agg.last_activity_tx,
            };
            write_batch.put_addr_balance(&lock_hash, &balance);
            written += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if written % 20_000 == 0 {
                write_batch.commit()?;
                write_batch = crate::batch::StoreBatch::new(self);
            }
        }
        #[allow(clippy::manual_is_multiple_of)]
        if written % 20_000 != 0 {
            write_batch.commit()?;
        }

        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::tempdir;

    fn make_cell(
        lock_hash: Vec<u8>,
        created_at: i64,
        capacity: i64,
        occupied: i64,
    ) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            created_at_block: created_at,
            lock_script_hash: lock_hash,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: occupied,
            udt_amount: None,
        }
    }

    #[test]
    fn test_rebuild_addr_balances_from_live_cells() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 120));
        batch.put_cell(&[0x02; 32], 0, &make_cell(lock_a.clone(), 11, 300, 320));
        batch.put_cell(&[0x03; 32], 0, &make_cell(lock_b.clone(), 12, 50, 60));
        batch.commit().unwrap();

        let rebuilt = store.rebuild_addr_balances_from_live_cells().unwrap();
        assert_eq!(rebuilt, 2);

        let a = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(a.balance, 400);
        assert_eq!(a.occupied_capacity, 440);
        assert_eq!(a.live_cells_count, 2);

        let b = store.get_addr_balance(&lock_b).unwrap().unwrap();
        assert_eq!(b.balance, 50);
        assert_eq!(b.occupied_capacity, 60);
        assert_eq!(b.live_cells_count, 1);
    }

    #[test]
    fn test_rebuild_addr_balances_restores_txs_count_from_addr_txs() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 120));
        batch.put_cell(&[0x02; 32], 0, &make_cell(lock_a.clone(), 11, 300, 320));
        batch.put_cell(&[0x03; 32], 0, &make_cell(lock_b.clone(), 12, 50, 60));
        batch.put_addr_tx(&lock_a, 10, 0, &[0x11; 32]);
        batch.put_addr_tx(&lock_a, 11, 1, &[0x22; 32]);
        batch.put_addr_tx(&lock_b, 12, 0, &[0x33; 32]);
        // This address has no live cells; rebuild should not materialize addr_balance for it.
        batch.put_addr_tx(&[0xCC; 32], 9, 0, &[0x44; 32]);
        batch.commit().unwrap();

        store.rebuild_addr_balances_from_live_cells().unwrap();

        let a = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(a.txs_count, 2);

        let b = store.get_addr_balance(&lock_b).unwrap().unwrap();
        assert_eq!(b.txs_count, 1);

        assert!(store.get_addr_balance(&[0xCC; 32]).unwrap().is_none());
    }

    #[test]
    fn test_rebuild_addr_balances_errors_on_negative_aggregate() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, -1, -1));
        batch.commit().unwrap();

        let err = store.rebuild_addr_balances_from_live_cells().unwrap_err();
        assert!(err.to_string().contains("negative aggregate"));
    }

    #[test]
    fn test_rebuild_addr_balances_fails_when_live_marker_missing_canonical_cell() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0x11; 32], 0);
        store.put_cf(store.cf_live_cells(), &outpoint, b"").unwrap();

        let err = store.rebuild_addr_balances_from_live_cells().unwrap_err();
        assert!(err
            .to_string()
            .contains("missing canonical cell for live marker during addr balance rebuild"));
    }

    #[test]
    fn test_top_addresses_fails_on_invalid_payload() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_hash = [0xAA; 32];
        store
            .put_cf(
                store.cf_addr_balance(),
                &lock_hash,
                b"invalid-address-balance",
            )
            .unwrap();

        let err = store.top_addresses(10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize address balance in top_addresses"));
    }

    #[test]
    fn test_list_addr_txs_recent_rejects_non_32_byte_lock_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let err = store
            .list_addr_txs_recent(&[0xAA; 31], 10, None)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("list_addr_txs_recent expects 32-byte lock_hash"));
    }

    #[test]
    fn test_list_addr_txs_recent_limit_zero_returns_empty() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 0, &[0x11; 32]);
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 0, None).unwrap();
        assert!(rows.is_empty());
    }
}
