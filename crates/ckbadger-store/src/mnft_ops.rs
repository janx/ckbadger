//! mNFT-specific store operations.

use std::collections::{HashMap, HashSet};

use crate::keys;
use crate::store::CkbadgerStore;

pub(crate) type MnftLiveOutpoint = (Vec<u8>, i16);
pub(crate) type MnftLiveOutpointMap = HashMap<Vec<u8>, MnftLiveOutpoint>;
pub(crate) type MnftOutpointLookup = (Vec<u8>, i16, Vec<u8>);

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

impl CkbadgerStore {
    pub fn get_mnft_class_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_mnft_class_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats_nft(), &key)? {
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    pub fn get_mnft_token_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_mnft_token_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats_nft(), &key)? {
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    pub fn get_mnft_token_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<Vec<MnftOutpointLookup>> {
        let cf = self.cf_stats_nft();
        let keys: Vec<[u8; keys::MNFT_TOKEN_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_mnft_token_outpoint_key(tx_hash, *idx))
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
                            "empty mnft token id in get_mnft_token_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}",
                            bytes_to_hex(tx_hash),
                            idx
                        ));
                    }
                    results.push((tx_hash.to_vec(), idx, value));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_mnft_token_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}, error={}",
                        bytes_to_hex(tx_hash),
                        idx,
                        e
                    ));
                }
            }
        }
        Ok(results)
    }

    /// Resolve live mNFT token outpoints by token IDs.
    ///
    /// Scans mNFT token outpoint index in `stats` and validates liveness via `live_cells`.
    /// Returns token_id -> (tx_hash, output_index) for tokens that currently have
    /// a live outpoint.
    pub fn get_live_mnft_token_outpoints_by_token_ids(
        &self,
        token_ids: &[Vec<u8>],
    ) -> anyhow::Result<MnftLiveOutpointMap> {
        let targets: HashSet<Vec<u8>> = token_ids.iter().cloned().collect();
        if targets.is_empty() {
            return Ok(HashMap::new());
        }

        let prefix = [keys::STATS_PREFIX_MNFT_TOKEN_OUTPOINT];
        let iter = self.prefix_iterator_cf(self.cf_stats_nft(), &prefix);
        let mut resolved: MnftLiveOutpointMap = HashMap::with_capacity(targets.len());

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first() != Some(&keys::STATS_PREFIX_MNFT_TOKEN_OUTPOINT) {
                break;
            }
            if key.len() != keys::MNFT_TOKEN_OUTPOINT_KEY_SIZE {
                anyhow::bail!(
                    "invalid mnft token outpoint key length: expected {}, got {}",
                    keys::MNFT_TOKEN_OUTPOINT_KEY_SIZE,
                    key.len()
                );
            }
            if !targets.contains(value.as_ref()) {
                continue;
            }

            let (tx_hash, output_index) = keys::decode_outpoint(&key[1..35]);
            if self.get_cell(&tx_hash, output_index)?.is_none() {
                continue;
            }

            if let Some((existing_tx_hash, existing_output_index)) = resolved.get(value.as_ref()) {
                if existing_tx_hash != &tx_hash || *existing_output_index != output_index {
                    anyhow::bail!(
                        "multiple live mnft outpoints for token_id=0x{:x?}: first=0x{:x?}-{}, second=0x{:x?}-{}",
                        value.as_ref(),
                        existing_tx_hash,
                        existing_output_index,
                        tx_hash,
                        output_index
                    );
                }
            } else {
                resolved.insert(value.to_vec(), (tx_hash, output_index));
            }

            if resolved.len() == targets.len() {
                break;
            }
        }

        Ok(resolved)
    }

    /// List all historical mNFT token outpoints recorded for a token ID.
    pub fn list_mnft_token_outpoints_by_token_id(
        &self,
        token_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
        let prefix = [keys::STATS_PREFIX_MNFT_TOKEN_OUTPOINT];
        let iter = self.prefix_iterator_cf(self.cf_stats_nft(), &prefix);
        let mut outpoints = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first() != Some(&keys::STATS_PREFIX_MNFT_TOKEN_OUTPOINT) {
                break;
            }
            if key.len() != keys::MNFT_TOKEN_OUTPOINT_KEY_SIZE {
                anyhow::bail!(
                    "invalid mnft token outpoint key length: expected {}, got {}",
                    keys::MNFT_TOKEN_OUTPOINT_KEY_SIZE,
                    key.len()
                );
            }
            if value.as_ref() != token_id {
                continue;
            }

            outpoints.push(keys::decode_outpoint(&key[1..35]));
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
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_mnft_outpoint_roundtrip_and_batch_lookup() {
        let (_dir, store) = test_store();
        let tx_a = [0xC1u8; 32];
        let mnft_class_id = [0x31u8; 24];
        let mnft_token_id = [0x41u8; 28];

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_class_outpoint(&tx_a, 3, &mnft_class_id);
        batch.put_mnft_token_outpoint(&tx_a, 4, &mnft_token_id);
        batch.commit().unwrap();

        let class_id = store
            .get_mnft_class_id_by_outpoint(&tx_a, 3)
            .unwrap()
            .unwrap();
        let token_id = store
            .get_mnft_token_id_by_outpoint(&tx_a, 4)
            .unwrap()
            .unwrap();
        assert_eq!(class_id, mnft_class_id.to_vec());
        assert_eq!(token_id, mnft_token_id.to_vec());

        let mnft_outpoints: Vec<(&[u8], i16)> = vec![(&tx_a, 4), (&tx_a, 9)];
        let mnft_results = store
            .get_mnft_token_ids_by_outpoints_batch(&mnft_outpoints)
            .unwrap();
        assert_eq!(mnft_results.len(), 1);
        assert_eq!(mnft_results[0].0, tx_a.to_vec());
        assert_eq!(mnft_results[0].1, 4);
        assert_eq!(mnft_results[0].2, mnft_token_id.to_vec());
    }

    #[test]
    fn test_get_mnft_token_ids_by_outpoints_batch_fails_on_empty_value() {
        let (_dir, store) = test_store();
        let tx_a = [0xC1u8; 32];
        let key = keys::encode_mnft_token_outpoint_key(&tx_a, 4);
        store.put_cf(store.cf_stats_nft(), &key, b"").unwrap();

        let mnft_outpoints: Vec<(&[u8], i16)> = vec![(&tx_a, 4)];
        let err = store
            .get_mnft_token_ids_by_outpoints_batch(&mnft_outpoints)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("empty mnft token id in get_mnft_token_ids_by_outpoints_batch"));
    }

    #[test]
    fn test_get_live_mnft_token_outpoints_by_token_ids_prefers_live_cells() {
        let (_dir, store) = test_store();
        let token_id = vec![0x71u8; 28];
        let old_tx = vec![0x81u8; 32];
        let live_tx = vec![0x82u8; 32];
        let old_idx = 1i16;
        let live_idx = 2i16;

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_token_outpoint(&old_tx, old_idx, &token_id);
        batch.put_mnft_token_outpoint(&live_tx, live_idx, &token_id);
        batch.put_cell(
            &live_tx,
            live_idx,
            &crate::types::LiveCellInfo {
                capacity: 100_00000000,
                created_at_block: 10,
                lock_script_hash: vec![0x01; 32],
                lock_code_hash: vec![0x02; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(vec![0x03; 32]),
                type_code_hash: Some(vec![0x04; 32]),
                type_args: Some(token_id.clone()),
                data_size: 0,
                occupied_capacity: 61_00000000,
                udt_amount: None,
            },
        );
        batch.commit().unwrap();

        let outpoints = store
            .get_live_mnft_token_outpoints_by_token_ids(std::slice::from_ref(&token_id))
            .unwrap();
        let (tx_hash, output_index) = outpoints.get(&token_id).unwrap();
        assert_eq!(tx_hash, &live_tx);
        assert_eq!(*output_index, live_idx);
    }

    #[test]
    fn test_list_mnft_token_outpoints_by_token_id_returns_all_matches() {
        let (_dir, store) = test_store();
        let token_id = vec![0x91u8; 28];
        let tx_a = vec![0x81u8; 32];
        let tx_b = vec![0x82u8; 32];
        let tx_other = vec![0x83u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_token_outpoint(&tx_a, 1, &token_id);
        batch.put_mnft_token_outpoint(&tx_b, 2, &token_id);
        batch.put_mnft_token_outpoint(&tx_other, 3, &[0x55u8; 28]);
        batch.commit().unwrap();

        let mut outpoints = store
            .list_mnft_token_outpoints_by_token_id(&token_id)
            .unwrap();
        outpoints.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        assert_eq!(outpoints.len(), 2);
        assert_eq!(outpoints[0], (tx_a, 1));
        assert_eq!(outpoints[1], (tx_b, 2));
    }
}
