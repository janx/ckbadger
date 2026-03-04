//! DAO operations.

use rocksdb::{IteratorMode, Snapshot};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::DaoDepositCacheEntry;

const DAO_BY_BLOCK_OUTPOINT_OFFSET: usize = 8;
const DAO_BY_STATUS_OUTPOINT_OFFSET: usize = 10;
const DAO_BY_LOCK_OUTPOINT_OFFSET: usize = 40;

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

#[cfg(test)]
type DaoStatusPaginationHook = Box<dyn Fn(&CkbadgerStore, &[u8]) + Send + Sync + 'static>;

#[cfg(test)]
fn dao_status_pagination_hook_cell() -> &'static Mutex<Option<DaoStatusPaginationHook>> {
    static CELL: OnceLock<Mutex<Option<DaoStatusPaginationHook>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn set_dao_status_pagination_hook(hook: Option<DaoStatusPaginationHook>) {
    let mut guard = dao_status_pagination_hook_cell()
        .lock()
        .expect("dao_status_pagination_hook lock poisoned");
    *guard = hook;
}

#[cfg(test)]
fn run_dao_status_pagination_hook(store: &CkbadgerStore, outpoint_key: &[u8]) {
    let hook = {
        let mut guard = dao_status_pagination_hook_cell()
            .lock()
            .expect("dao_status_pagination_hook lock poisoned");
        guard.take()
    };
    if let Some(hook) = hook {
        hook(store, outpoint_key);
    }
}

impl CkbadgerStore {
    fn load_dao_entry_for_index(
        &self,
        outpoint_key: &[u8],
        index_name: &str,
        index_key: &[u8],
    ) -> anyhow::Result<DaoDepositCacheEntry> {
        match self.get_dao_deposit(outpoint_key)? {
            Some(entry) => Ok(entry),
            None => anyhow::bail!(
                "stale {} index points to missing dao deposit: index_key=0x{}, outpoint_key=0x{}",
                index_name,
                bytes_to_hex(index_key),
                bytes_to_hex(outpoint_key)
            ),
        }
    }

    fn load_dao_entry_for_index_from_snapshot(
        &self,
        snapshot: &Snapshot<'_>,
        outpoint_key: &[u8],
        index_name: &str,
        index_key: &[u8],
    ) -> anyhow::Result<DaoDepositCacheEntry> {
        match snapshot.get_cf(self.cf_dao_deposits(), outpoint_key)? {
            Some(value) => Ok(bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao deposit entry while reading {} index: index_key=0x{}, outpoint_key=0x{}, error={}",
                    index_name,
                    bytes_to_hex(index_key),
                    bytes_to_hex(outpoint_key),
                    e
                )
            })?),
            None => anyhow::bail!(
                "stale {} index points to missing dao deposit: index_key=0x{}, outpoint_key=0x{}",
                index_name,
                bytes_to_hex(index_key),
                bytes_to_hex(outpoint_key)
            ),
        }
    }

    fn delete_dao_secondary_indexes_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key);
        let by_lock_key = keys::encode_dao_by_lock_block_key(
            &entry.lock_script_hash,
            entry.deposit_block_number,
            outpoint_key,
        );
        let by_status_key = keys::encode_dao_by_status_block_key(
            entry.status,
            entry.deposit_block_number,
            outpoint_key,
        );

        self.delete_cf(self.cf_dao_by_block(), &by_block_key)?;
        self.delete_cf(self.cf_dao_by_lock_block(), &by_lock_key)?;
        self.delete_cf(self.cf_dao_by_status_block(), &by_status_key)?;
        Ok(())
    }

    fn put_dao_secondary_indexes_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            outpoint_key.len() == keys::OUTPOINT_KEY_SIZE,
            "put_dao_secondary_indexes_direct expected outpoint {} bytes, got {}",
            keys::OUTPOINT_KEY_SIZE,
            outpoint_key.len()
        );
        anyhow::ensure!(
            entry.lock_script_hash.len() == 32,
            "put_dao_secondary_indexes_direct expected lock hash 32 bytes, got {}",
            entry.lock_script_hash.len()
        );

        let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key);
        let by_lock_key = keys::encode_dao_by_lock_block_key(
            &entry.lock_script_hash,
            entry.deposit_block_number,
            outpoint_key,
        );
        let by_status_key = keys::encode_dao_by_status_block_key(
            entry.status,
            entry.deposit_block_number,
            outpoint_key,
        );

        self.put_cf(self.cf_dao_by_block(), &by_block_key, &[])?;
        self.put_cf(self.cf_dao_by_lock_block(), &by_lock_key, &[])?;
        self.put_cf(self.cf_dao_by_status_block(), &by_status_key, &[])?;
        Ok(())
    }

    pub fn get_dao_deposit(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<DaoDepositCacheEntry>> {
        match self.get_cf(self.cf_dao_deposits(), outpoint_key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_dao_deposit_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        if let Some(existing) = self.get_dao_deposit(outpoint_key)? {
            self.delete_dao_secondary_indexes_direct(outpoint_key, &existing)?;
        }

        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_dao_deposits(), outpoint_key, &value)?;
        self.put_dao_secondary_indexes_direct(outpoint_key, entry)
    }

    pub fn get_dao_deposit_by_withdraw_tx(
        &self,
        withdraw_tx_hash: &[u8],
        withdraw_output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_outpoint(withdraw_tx_hash, withdraw_output_index);
        self.get_cf(self.cf_dao_by_withdraw_tx(), &key)
    }

    /// Scan all DAO deposits (prefix scan) without materializing the full table in memory.
    pub fn scan_dao_deposits<F>(&self, mut visitor: F) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], &DaoDepositCacheEntry) -> anyhow::Result<()>,
    {
        let iter = self.iterator_cf(self.cf_dao_deposits(), IteratorMode::Start);

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate dao_deposits in scan_dao_deposits: {}", e)
            })?;
            let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao deposit entry in scan_dao_deposits: outpoint_key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            visitor(&key, &entry)?;
        }
        Ok(())
    }

    pub fn scan_dao_deposits_by_status<F>(&self, status: i16, mut visitor: F) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], &DaoDepositCacheEntry) -> anyhow::Result<()>,
    {
        let prefix = keys::encode_dao_by_status_prefix(status);
        let iter = self.prefix_iterator_cf(self.cf_dao_by_status_block(), &prefix);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_status_block in scan_dao_deposits_by_status: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            anyhow::ensure!(
                key.len() == keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                "invalid dao_by_status_block key length: expected {}, got {}",
                keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                key.len()
            );

            let indexed_status = i16::from_be_bytes(
                key[..2]
                    .try_into()
                    .expect("status key prefix should be exactly 2 bytes"),
            );
            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[2..10]
                        .try_into()
                        .expect("status key block section should be exactly 8 bytes"),
                );
            let outpoint_key = &key[DAO_BY_STATUS_OUTPOINT_OFFSET
                ..DAO_BY_STATUS_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];

            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_status_block", &key)?;
            anyhow::ensure!(
                indexed_status == status,
                "dao_by_status_block prefix mismatch: expected status={}, got status={}, index_key=0x{}",
                status,
                indexed_status,
                bytes_to_hex(&key)
            );
            anyhow::ensure!(
                entry.status == status,
                "dao_by_status_block stale status: expected={}, actual={}, outpoint_key=0x{}",
                status,
                entry.status,
                bytes_to_hex(outpoint_key)
            );
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_status_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            visitor(outpoint_key, &entry)?;
        }
        Ok(())
    }

    pub fn scan_dao_deposits_by_lock<F>(
        &self,
        lock_hash: &[u8],
        mut visitor: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], &DaoDepositCacheEntry) -> anyhow::Result<()>,
    {
        anyhow::ensure!(
            lock_hash.len() == 32,
            "scan_dao_deposits_by_lock expected lock hash 32 bytes, got {}",
            lock_hash.len()
        );

        let prefix = keys::encode_dao_by_lock_prefix(lock_hash);
        let iter = self.prefix_iterator_cf(self.cf_dao_by_lock_block(), &prefix);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_lock_block in scan_dao_deposits_by_lock: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            anyhow::ensure!(
                key.len() == keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                "invalid dao_by_lock_block key length: expected {}, got {}",
                keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                key.len()
            );

            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[32..40]
                        .try_into()
                        .expect("lock index block section should be exactly 8 bytes"),
                );
            let outpoint_key = &key[DAO_BY_LOCK_OUTPOINT_OFFSET
                ..DAO_BY_LOCK_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];

            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_lock_block", &key)?;
            anyhow::ensure!(
                entry.lock_script_hash.as_slice() == prefix.as_slice(),
                "dao_by_lock_block stale lock hash: expected=0x{}, actual=0x{}, outpoint_key=0x{}",
                bytes_to_hex(&prefix),
                bytes_to_hex(&entry.lock_script_hash),
                bytes_to_hex(outpoint_key)
            );
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_lock_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            visitor(outpoint_key, &entry)?;
        }
        Ok(())
    }

    pub fn list_dao_deposits_paginated(
        &self,
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut rows = Vec::with_capacity(limit);
        if let Some(cursor_key) = cursor_key_exclusive {
            anyhow::ensure!(
                cursor_key.len() == keys::DAO_BY_BLOCK_KEY_SIZE,
                "invalid dao_by_block cursor key length: expected {}, got {}",
                keys::DAO_BY_BLOCK_KEY_SIZE,
                cursor_key.len()
            );
        }
        let iter_mode = if let Some(cursor_key) = cursor_key_exclusive {
            IteratorMode::From(cursor_key, rocksdb::Direction::Forward)
        } else {
            IteratorMode::Start
        };
        let iter = self.iterator_cf(self.cf_dao_by_block(), iter_mode);

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate dao_by_block in pagination: {}", e)
            })?;
            anyhow::ensure!(
                key.len() == keys::DAO_BY_BLOCK_KEY_SIZE,
                "invalid dao_by_block key length: expected {}, got {}",
                keys::DAO_BY_BLOCK_KEY_SIZE,
                key.len()
            );
            if let Some(cursor_key) = cursor_key_exclusive {
                if key.as_ref() == cursor_key {
                    continue;
                }
            }

            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[..8]
                        .try_into()
                        .expect("block index prefix should be exactly 8 bytes"),
                );

            let outpoint_key = &key[DAO_BY_BLOCK_OUTPOINT_OFFSET
                ..DAO_BY_BLOCK_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];
            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_block", &key)?;
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            rows.push((outpoint_key.to_vec(), entry));
            if rows.len() >= limit {
                break;
            }
        }

        Ok(rows)
    }

    pub fn list_dao_deposits_by_status_paginated(
        &self,
        status: i16,
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut rows = Vec::with_capacity(limit);
        let prefix = keys::encode_dao_by_status_prefix(status);
        if let Some(cursor_key) = cursor_key_exclusive {
            anyhow::ensure!(
                cursor_key.len() == keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                "invalid dao_by_status_block cursor key length: expected {}, got {}",
                keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                cursor_key.len()
            );
            anyhow::ensure!(
                cursor_key.starts_with(&prefix),
                "dao_by_status_block cursor does not match status prefix: status={}",
                status
            );
        }
        let start_key = cursor_key_exclusive.unwrap_or(prefix.as_slice());
        let snapshot = self.snapshot();
        let iter = snapshot.iterator_cf(
            self.cf_dao_by_status_block(),
            IteratorMode::From(start_key, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_status_block in pagination: status={}, error={}",
                    status,
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            anyhow::ensure!(
                key.len() == keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                "invalid dao_by_status_block key length: expected {}, got {}",
                keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                key.len()
            );
            if let Some(cursor_key) = cursor_key_exclusive {
                if key.as_ref() == cursor_key {
                    continue;
                }
            }

            let indexed_status = i16::from_be_bytes(
                key[..2]
                    .try_into()
                    .expect("status key prefix should be exactly 2 bytes"),
            );
            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[2..10]
                        .try_into()
                        .expect("status key block section should be exactly 8 bytes"),
                );

            let outpoint_key = &key[DAO_BY_STATUS_OUTPOINT_OFFSET
                ..DAO_BY_STATUS_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];
            #[cfg(test)]
            run_dao_status_pagination_hook(self, outpoint_key);
            let entry = self.load_dao_entry_for_index_from_snapshot(
                &snapshot,
                outpoint_key,
                "dao_by_status_block",
                &key,
            )?;
            anyhow::ensure!(
                indexed_status == status,
                "dao_by_status_block prefix mismatch: expected status={}, got status={}, index_key=0x{}",
                status,
                indexed_status,
                bytes_to_hex(&key)
            );
            anyhow::ensure!(
                entry.status == status,
                "dao_by_status_block stale status: expected={}, actual={}, outpoint_key=0x{}",
                status,
                entry.status,
                bytes_to_hex(outpoint_key)
            );
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_status_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            rows.push((outpoint_key.to_vec(), entry));
            if rows.len() >= limit {
                break;
            }
        }

        Ok(rows)
    }

    pub fn list_dao_deposits_by_lock_paginated(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        anyhow::ensure!(
            lock_hash.len() == 32,
            "list_dao_deposits_by_lock_paginated expected lock hash 32 bytes, got {}",
            lock_hash.len()
        );

        let mut rows = Vec::with_capacity(limit);
        let prefix = keys::encode_dao_by_lock_prefix(lock_hash);
        if let Some(cursor_key) = cursor_key_exclusive {
            anyhow::ensure!(
                cursor_key.len() == keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                "invalid dao_by_lock_block cursor key length: expected {}, got {}",
                keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                cursor_key.len()
            );
            anyhow::ensure!(
                cursor_key.starts_with(&prefix),
                "dao_by_lock_block cursor does not match lock prefix: lock_hash=0x{}",
                bytes_to_hex(&prefix)
            );
        }
        let start_key = cursor_key_exclusive.unwrap_or(prefix.as_slice());
        let iter = self.iterator_cf(
            self.cf_dao_by_lock_block(),
            IteratorMode::From(start_key, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_lock_block in pagination: lock_hash=0x{}, error={}",
                    bytes_to_hex(&prefix),
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            anyhow::ensure!(
                key.len() == keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                "invalid dao_by_lock_block key length: expected {}, got {}",
                keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                key.len()
            );
            if let Some(cursor_key) = cursor_key_exclusive {
                if key.as_ref() == cursor_key {
                    continue;
                }
            }

            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[32..40]
                        .try_into()
                        .expect("lock index block section should be exactly 8 bytes"),
                );

            let outpoint_key = &key[DAO_BY_LOCK_OUTPOINT_OFFSET
                ..DAO_BY_LOCK_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];
            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_lock_block", &key)?;
            anyhow::ensure!(
                entry.lock_script_hash.as_slice() == prefix.as_slice(),
                "dao_by_lock_block stale lock hash: expected=0x{}, actual=0x{}, outpoint_key=0x{}",
                bytes_to_hex(&prefix),
                bytes_to_hex(&entry.lock_script_hash),
                bytes_to_hex(outpoint_key)
            );
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_lock_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            rows.push((outpoint_key.to_vec(), entry));
            if rows.len() >= limit {
                break;
            }
        }

        Ok(rows)
    }

    /// List all DAO deposits (prefix scan).
    pub fn list_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut results = Vec::new();
        self.scan_dao_deposits(|key, entry| {
            results.push((key.to_vec(), entry.clone()));
            Ok(())
        })?;
        Ok(results)
    }

    /// List active (status=0) DAO deposits.
    pub fn list_active_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut rows = Vec::new();
        self.scan_dao_deposits_by_status(0, |outpoint_key, entry| {
            rows.push((outpoint_key.to_vec(), entry.clone()));
            Ok(())
        })?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use anyhow::Context;
    use tempfile::TempDir;

    #[test]
    fn test_list_dao_deposits_fails_on_invalid_payload() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);
        store
            .put_cf(store.cf_dao_deposits(), &outpoint, b"invalid-dao-deposit")
            .unwrap();

        let err = store.list_dao_deposits().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize dao deposit entry in scan_dao_deposits"));
    }

    #[test]
    fn test_scan_dao_deposits_visits_rows() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);
        let entry = DaoDepositCacheEntry {
            capacity: 42,
            deposit_block_number: 100,
            lock_script_hash: vec![0xCD; 32],
            deposit_ar: 1,
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
        store.put_dao_deposit_direct(&outpoint, &entry).unwrap();

        let mut visited = Vec::new();
        store
            .scan_dao_deposits(|key, value| {
                visited.push((key.to_vec(), value.clone()));
                Ok(())
            })
            .unwrap();

        assert_eq!(visited.len(), 1);
        assert_eq!(visited[0].0, outpoint);
        assert_eq!(visited[0].1, entry);
    }

    #[test]
    fn test_list_dao_deposits_by_status_paginated_descending() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (tx, status, block) in [
            ([0xA1; 32], 0i16, 30i64),
            ([0xA2; 32], 1, 20),
            ([0xA3; 32], 0, 10),
        ] {
            batch.put_dao_deposit(
                &keys::encode_outpoint(&tx, 0),
                &DaoDepositCacheEntry {
                    capacity: 100,
                    deposit_block_number: block,
                    lock_script_hash: vec![0x11; 32],
                    deposit_ar: 1,
                    status,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
        }
        batch.commit().unwrap();

        let first_page = store
            .list_dao_deposits_by_status_paginated(0, 2, None)
            .unwrap();
        assert_eq!(first_page.len(), 2);
        assert_eq!(first_page[0].1.deposit_block_number, 30);
        assert_eq!(first_page[1].1.deposit_block_number, 10);

        let cursor = keys::encode_dao_by_status_block_key(
            0,
            first_page[1].1.deposit_block_number,
            &first_page[1].0,
        );
        let second_page = store
            .list_dao_deposits_by_status_paginated(0, 2, Some(&cursor))
            .unwrap();
        assert!(second_page.is_empty());
    }

    #[test]
    fn test_list_dao_deposits_by_status_paginated_survives_mid_scan_entry_update() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xA9; 32], 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &outpoint,
            &DaoDepositCacheEntry {
                capacity: 100,
                deposit_block_number: 30,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        // Simulate a concurrent status transition after index key is read but before
        // dao_deposits row is loaded.
        let outpoint_for_hook = outpoint;
        set_dao_status_pagination_hook(Some(Box::new(move |store, outpoint_key| {
            if outpoint_key != outpoint_for_hook.as_slice() {
                return;
            }
            let mut entry = store
                .get_dao_deposit(outpoint_key)
                .expect("load dao deposit in hook")
                .expect("dao deposit missing in hook");
            entry.status = 1;
            entry.withdraw_request_block = Some(31);
            entry.withdraw_request_tx = Some(vec![0xBB; 32]);
            entry.withdraw_request_output_index = Some(0);
            store
                .put_dao_deposit_direct(outpoint_key, &entry)
                .expect("update dao deposit in hook");
        })));

        let rows = store.list_dao_deposits_by_status_paginated(0, 10, None);
        set_dao_status_pagination_hook(None);

        let rows = rows.expect("status pagination should remain consistent under mid-scan update");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, outpoint);
        assert_eq!(rows[0].1.status, 0);

        let current = store.get_dao_deposit(&outpoint).unwrap().unwrap();
        assert_eq!(current.status, 1, "hook must have updated canonical row");
    }

    #[test]
    fn test_scan_dao_deposits_by_lock_visits_only_target_lock() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock_a = vec![0x11; 32];
        let lock_b = vec![0x22; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA1; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                deposit_block_number: 10,
                lock_script_hash: lock_a.clone(),
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA2; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                deposit_block_number: 20,
                lock_script_hash: lock_b,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        let mut blocks = Vec::new();
        store
            .scan_dao_deposits_by_lock(&lock_a, |_, entry| {
                blocks.push(entry.deposit_block_number);
                Ok(())
            })
            .unwrap();

        assert_eq!(blocks, vec![10]);
    }

    #[test]
    fn test_put_dao_deposit_direct_replaces_status_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);

        let mut entry = DaoDepositCacheEntry {
            capacity: 42,
            deposit_block_number: 100,
            lock_script_hash: vec![0xCD; 32],
            deposit_ar: 1,
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
        store.put_dao_deposit_direct(&outpoint, &entry).unwrap();

        entry.status = 1;
        entry.withdraw_request_tx = Some(vec![0xEE; 32]);
        entry.withdraw_request_output_index = Some(0);
        entry.withdraw_request_block = Some(120);
        store.put_dao_deposit_direct(&outpoint, &entry).unwrap();

        let status0 = store
            .list_dao_deposits_by_status_paginated(0, 10, None)
            .unwrap();
        assert!(status0.is_empty());
        let status1 = store
            .list_dao_deposits_by_status_paginated(1, 10, None)
            .unwrap();
        assert_eq!(status1.len(), 1);
        assert_eq!(status1[0].0, outpoint);
    }

    #[test]
    fn test_list_active_dao_deposits_uses_status_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA1; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                deposit_block_number: 10,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA2; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                deposit_block_number: 20,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0xBB; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(15),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(30),
                withdraw_tx: Some(vec![0xCC; 32]),
                withdraw_to_output_index: Some(0),
                compensation: Some(5),
            },
        );
        batch.commit().unwrap();

        let active = store.list_active_dao_deposits().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].1.status, 0);
    }

    #[test]
    fn test_list_dao_deposits_by_lock_paginated_descending() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let lock = vec![0x44; 32];
        let mut batch = StoreBatch::new(&store);
        for (tx, block) in [([0xA1; 32], 30i64), ([0xA2; 32], 20), ([0xA3; 32], 10)] {
            batch.put_dao_deposit(
                &keys::encode_outpoint(&tx, 0),
                &DaoDepositCacheEntry {
                    capacity: 100,
                    deposit_block_number: block,
                    lock_script_hash: lock.clone(),
                    deposit_ar: 1,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
        }
        batch.commit().unwrap();

        let first = store
            .list_dao_deposits_by_lock_paginated(&lock, 2, None)
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].1.deposit_block_number, 30);
        assert_eq!(first[1].1.deposit_block_number, 20);

        let cursor =
            keys::encode_dao_by_lock_block_key(&lock, first[1].1.deposit_block_number, &first[1].0);
        let second = store
            .list_dao_deposits_by_lock_paginated(&lock, 2, Some(&cursor))
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].1.deposit_block_number, 10);
    }

    #[test]
    fn test_scan_dao_deposits_by_lock_rejects_non_32_byte_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store
            .scan_dao_deposits_by_lock(&[0x11; 33], |_, _| Ok(()))
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("scan_dao_deposits_by_lock expected lock hash 32 bytes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_list_dao_deposits_by_lock_paginated_rejects_non_32_byte_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store
            .list_dao_deposits_by_lock_paginated(&[0x11; 33], 1, None)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("list_dao_deposits_by_lock_paginated expected lock hash 32 bytes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_list_dao_deposits_paginated_cursor_handles_same_block_multiple_rows() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (tx, output_index, block) in [
            ([0xD1; 32], 0i16, 30i64),
            ([0xD2; 32], 1i16, 30i64),
            ([0xD3; 32], 2i16, 30i64),
            ([0xD4; 32], 0i16, 20i64),
        ] {
            batch.put_dao_deposit(
                &keys::encode_outpoint(&tx, output_index),
                &DaoDepositCacheEntry {
                    capacity: 100,
                    deposit_block_number: block,
                    lock_script_hash: vec![0x55; 32],
                    deposit_ar: 1,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
        }
        batch.commit().unwrap();

        let first_page = store.list_dao_deposits_paginated(2, None).unwrap();
        assert_eq!(first_page.len(), 2);
        assert_eq!(first_page[0].1.deposit_block_number, 30);
        assert_eq!(first_page[1].1.deposit_block_number, 30);

        let cursor =
            keys::encode_dao_by_block_key(first_page[1].1.deposit_block_number, &first_page[1].0);
        let second_page = store.list_dao_deposits_paginated(2, Some(&cursor)).unwrap();
        assert_eq!(second_page.len(), 2);
        assert_eq!(second_page[0].1.deposit_block_number, 30);
        assert_eq!(second_page[1].1.deposit_block_number, 20);
    }

    #[test]
    fn test_list_dao_deposits_paginated_errors_on_stale_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let outpoint = keys::encode_outpoint(&[0xAA; 32], 0);
        let index_key = keys::encode_dao_by_block_key(100, &outpoint);
        store
            .put_cf(store.cf_dao_by_block(), &index_key, &[])
            .context("seed stale dao_by_block index")
            .unwrap();

        let err = store.list_dao_deposits_paginated(10, None).unwrap_err();
        assert!(err.to_string().contains("stale dao_by_block index"));
    }
}
