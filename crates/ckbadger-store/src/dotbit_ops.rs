//! DotBit-specific store operations.

use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;

pub(crate) type DotbitLiveOutpoint = (Vec<u8>, i16);
pub(crate) type DotbitLiveOutpointMap = HashMap<Vec<u8>, DotbitLiveOutpoint>;
pub(crate) type DotbitOutpointLookup = (Vec<u8>, i16, Vec<u8>);

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_dotbit_account_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_dotbit_account_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats_object(), &key)? {
            Some(value) if value.is_empty() => anyhow::bail!(
                "empty dotbit account id for outpoint: tx_hash=0x{}, output_index={}",
                bytes_to_hex(tx_hash),
                output_index
            ),
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    pub fn get_dotbit_account_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<Vec<DotbitOutpointLookup>> {
        let cf = self.cf_stats_object();
        let keys: Vec<[u8; keys::DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_dotbit_account_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            let (tx_hash, idx) = outpoints[i];
            match value_result {
                Ok(Some(value)) => {
                    if value.is_empty() {
                        return Err(anyhow::anyhow!(
                            "empty dotbit account id in get_dotbit_account_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}",
                            bytes_to_hex(tx_hash),
                            idx
                        ));
                    }
                    results.push((tx_hash.to_vec(), idx, value));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_dotbit_account_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}, error={}",
                        bytes_to_hex(tx_hash),
                        idx,
                        e
                    ));
                }
            }
        }
        Ok(results)
    }

    /// Resolve live dotbit account outpoints by account IDs.
    ///
    /// Scans dotbit outpoint index in `stats` and validates liveness via `live_cells`.
    /// Returns account_id -> (tx_hash, output_index) for accounts that currently have
    /// a live outpoint.
    ///
    /// Uses the `DOTBIT_OUTPOINT_BY_ACCOUNT_ID` reverse index for O(k) lookups per
    /// account (where k is the number of historical outpoints for that account),
    /// instead of scanning all dotbit outpoints.
    pub fn get_live_dotbit_outpoints_by_account_ids(
        &self,
        account_ids: &[Vec<u8>],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<DotbitLiveOutpointMap> {
        if account_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut resolved: DotbitLiveOutpointMap = HashMap::with_capacity(account_ids.len());

        for account_id in account_ids {
            let prefix = keys::encode_dotbit_outpoint_by_account_id_prefix(account_id);
            let iter = self.prefix_iterator_cf(self.cf_stats_object(), &prefix);

            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate stats_object in get_live_dotbit_outpoints_by_account_ids: account_id=0x{}, error={}",
                        bytes_to_hex(account_id),
                        e
                    )
                })?;
                if !key.starts_with(&prefix) {
                    break;
                }
                if key.len() != keys::DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE {
                    anyhow::bail!(
                        "invalid dotbit outpoint_by_account_id key length: expected {}, got {}, account_id=0x{}",
                        keys::DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE,
                        key.len(),
                        bytes_to_hex(account_id)
                    );
                }

                let (tx_hash, output_index) = keys::decode_dotbit_outpoint_by_account_id_key(&key);
                if self
                    .get_cell(&tx_hash, output_index, cells_store)?
                    .is_none()
                {
                    continue;
                }

                if let Some((existing_tx_hash, existing_output_index)) =
                    resolved.get(account_id.as_slice())
                {
                    if existing_tx_hash != &tx_hash || *existing_output_index != output_index {
                        anyhow::bail!(
                            "multiple live dotbit outpoints for account_id=0x{}: first=0x{}-{}, second=0x{}-{}",
                            bytes_to_hex(account_id),
                            bytes_to_hex(existing_tx_hash),
                            existing_output_index,
                            bytes_to_hex(&tx_hash),
                            output_index
                        );
                    }
                } else {
                    resolved.insert(account_id.clone(), (tx_hash, output_index));
                }

                // Found a live outpoint for this account — move to next account
                break;
            }
        }

        Ok(resolved)
    }

    /// List all historical .bit account outpoints recorded for an account ID.
    ///
    /// Uses the `DOTBIT_OUTPOINT_BY_ACCOUNT_ID` reverse index for efficient
    /// per-account lookup instead of scanning all dotbit outpoints.
    pub fn list_dotbit_account_outpoints_by_account_id(
        &self,
        account_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
        let prefix = keys::encode_dotbit_outpoint_by_account_id_prefix(account_id);
        let iter = self.prefix_iterator_cf(self.cf_stats_object(), &prefix);
        let mut outpoints = Vec::new();

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_object in list_dotbit_account_outpoints_by_account_id: account_id=0x{}, error={}",
                    bytes_to_hex(account_id),
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE {
                anyhow::bail!(
                    "invalid dotbit outpoint_by_account_id key length: expected {}, got {}, account_id=0x{}",
                    keys::DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE,
                    key.len(),
                    bytes_to_hex(account_id)
                );
            }

            outpoints.push(keys::decode_dotbit_outpoint_by_account_id_key(&key));
        }

        Ok(outpoints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_dotbit_outpoint_roundtrip_and_batch_lookup() {
        let (_dir, store) = test_store();
        let tx_b = [0xC2u8; 32];
        let dotbit_account_id = [0x51u8; 20];

        let mut batch = StoreBatch::new(&store);
        batch.put_dotbit_account_outpoint(&tx_b, 5, &dotbit_account_id);
        batch.commit().unwrap();

        let dotbit_id = store
            .get_dotbit_account_id_by_outpoint(&tx_b, 5)
            .unwrap()
            .unwrap();
        assert_eq!(dotbit_id, dotbit_account_id.to_vec());

        let dotbit_outpoints: Vec<(&[u8], i16)> = vec![(&tx_b, 5), (&tx_b, 8)];
        let dotbit_results = store
            .get_dotbit_account_ids_by_outpoints_batch(&dotbit_outpoints)
            .unwrap();
        assert_eq!(dotbit_results.len(), 1);
        assert_eq!(dotbit_results[0].0, tx_b.to_vec());
        assert_eq!(dotbit_results[0].1, 5);
        assert_eq!(dotbit_results[0].2, dotbit_account_id.to_vec());
    }

    #[test]
    fn test_get_dotbit_account_ids_by_outpoints_batch_fails_on_empty_value() {
        let (_dir, store) = test_store();
        let tx_b = [0xC2u8; 32];
        let key = keys::encode_dotbit_account_outpoint_key(&tx_b, 5);
        store.put_cf(store.cf_stats_object(), &key, b"").unwrap();

        let dotbit_outpoints: Vec<(&[u8], i16)> = vec![(&tx_b, 5)];
        let err = store
            .get_dotbit_account_ids_by_outpoints_batch(&dotbit_outpoints)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("empty dotbit account id in get_dotbit_account_ids_by_outpoints_batch"));
    }

    #[test]
    fn test_get_live_dotbit_outpoints_by_account_ids_prefers_live_cells() {
        let (_dir, store) = test_store();
        let account_id = vec![0x61u8; 20];
        let old_tx = vec![0x71u8; 32];
        let live_tx = vec![0x72u8; 32];
        let old_idx = 1i16;
        let live_idx = 2i16;

        let mut batch = StoreBatch::new(&store);
        // Historical outpoint (no live cell now)
        batch.put_dotbit_account_outpoint(&old_tx, old_idx, &account_id);
        batch.put_dotbit_outpoint_by_account_id(&account_id, &old_tx, old_idx);
        // Current outpoint with a live cell
        batch.put_dotbit_account_outpoint(&live_tx, live_idx, &account_id);
        batch.put_dotbit_outpoint_by_account_id(&account_id, &live_tx, live_idx);
        batch.put_cell(
            &live_tx,
            live_idx,
            &crate::types::LiveCellInfo {
                capacity: 100_00000000,
                lock_script_hash: vec![0x01; 32],
                lock_code_hash: vec![0x02; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(vec![0x03; 32]),
                type_code_hash: Some(vec![0x04; 32]),
                type_hash_type: Some(1),
                type_args: Some(account_id.clone()),
                data_size: 0,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: None,
            },
            10,
        );
        batch.commit().unwrap();

        let outpoints = store
            .get_live_dotbit_outpoints_by_account_ids(std::slice::from_ref(&account_id), &store)
            .unwrap();
        let (tx_hash, output_index) = outpoints.get(&account_id).unwrap();
        assert_eq!(tx_hash, &live_tx);
        assert_eq!(*output_index, live_idx);
    }

    #[test]
    fn test_list_dotbit_account_outpoints_by_account_id_returns_all_matches() {
        let (_dir, store) = test_store();
        let account_id = vec![0x61u8; 20];
        let tx_a = vec![0x71u8; 32];
        let tx_b = vec![0x72u8; 32];
        let tx_other = vec![0x73u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_dotbit_account_outpoint(&tx_a, 4, &account_id);
        batch.put_dotbit_outpoint_by_account_id(&account_id, &tx_a, 4);
        batch.put_dotbit_account_outpoint(&tx_b, 5, &account_id);
        batch.put_dotbit_outpoint_by_account_id(&account_id, &tx_b, 5);
        batch.put_dotbit_account_outpoint(&tx_other, 6, &[0x44u8; 20]);
        batch.put_dotbit_outpoint_by_account_id(&[0x44u8; 20], &tx_other, 6);
        batch.commit().unwrap();

        let mut outpoints = store
            .list_dotbit_account_outpoints_by_account_id(&account_id)
            .unwrap();
        outpoints.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        assert_eq!(outpoints.len(), 2);
        assert_eq!(outpoints[0], (tx_a, 4));
        assert_eq!(outpoints[1], (tx_b, 5));
    }
}
