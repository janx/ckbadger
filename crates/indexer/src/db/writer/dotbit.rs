use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{NftCollectionAggregate, NftEntry, NftExtra, NftStandard};

use crate::parser::dotbit::ParsedDotbitAccount;

use super::BatchWriter;

/// Sentinel collection key for DotBit accounts (which have no collection_id).
/// 32-byte key: "dotbit_collection_______________" (padded to 32 bytes).
const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";

impl BatchWriter {
    pub fn insert_dotbit_account(
        &self,
        account: &ParsedDotbitAccount,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let account_name = format!("0x{}", hex::encode(&account.account_id));
        let existing = self.store.get_nft(&account.account_id)?;

        let entry = NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(account.account_id.clone()),
            owner_lock_hash: Some(account.owner_lock_hash.clone()),
            name: Some(account_name),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            extra: NftExtra::DotBit {
                expired_at: account.expired_at,
            },
        };
        batch.put_nft(&account.account_id, &entry);

        // Update collection aggregate if this is a new account
        if existing.is_none() {
            let mut agg = self
                .store
                .get_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)?
                .unwrap_or_else(|| NftCollectionAggregate {
                    name: Some(".bit".to_string()),
                    standard: NftStandard::DotBit,
                    ..Default::default()
                });
            agg.total_count += 1;
            agg.live_count += 1;
            batch.put_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION, &agg);
        } else {
            // Re-insert (transfer) — increment hourly bucket for 24h tracking
            let hour_bucket = timestamp_ms / 3_600_000;
            let key = ckbadger_store::keys::encode_nft_hourly_key(
                &DOTBIT_SENTINEL_COLLECTION,
                hour_bucket,
            );
            let current = match self.store.get_cf(self.store.cf_stats(), &key)? {
                Some(v) if v.len() == 8 => i64::from_le_bytes(v[..8].try_into().unwrap()),
                _ => 0,
            };
            batch.put_nft_hourly_transfer(&DOTBIT_SENTINEL_COLLECTION, hour_bucket, current + 1);
        }
        batch.put_dotbit_account_outpoint(tx_hash, output_index, &account.account_id);
        Ok(())
    }

    pub fn consume_dotbit_account(
        &self,
        account_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if let Some(mut entry) = self.store.get_nft(account_id)? {
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_nft(account_id, &entry);

            // Decrement collection's live_count
            let mut agg = self
                .store
                .get_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)?
                .unwrap_or_default();
            agg.live_count -= 1;
            batch.put_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION, &agg);
        }
        Ok(())
    }

    pub fn get_dotbit_account_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        self.store
            .get_dotbit_account_id_by_outpoint(tx_hash, output_index)
    }

    /// Batch lookup: find account_ids for multiple outpoints.
    pub fn get_dotbit_account_ids_by_outpoints_batch(
        &self,
        tx_hashes: &[Vec<u8>],
        output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        let outpoints: Vec<(&[u8], i16)> = tx_hashes
            .iter()
            .zip(output_indices.iter())
            .map(|(hash, idx)| (hash.as_slice(), *idx))
            .collect();
        Ok(self
            .store
            .get_dotbit_account_ids_by_outpoints_batch(&outpoints))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::BatchWriter;
    use ckbadger_store::store::CkbadgerStore;
    use std::sync::Arc;

    #[test]
    fn test_dotbit_outpoint_lookups_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

        let account = ParsedDotbitAccount {
            account_id: vec![0x11; 20],
            type_script_hash: vec![0x21; 32],
            next_account_id: None,
            expired_at: None,
            owner_lock_hash: vec![0x31; 32],
        };
        let tx_hash = vec![0x41; 32];

        let mut batch = StoreBatch::new(writer.store());
        writer
            .insert_dotbit_account(&account, &tx_hash, 6, 1, 0, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let loaded = writer
            .get_dotbit_account_id_by_outpoint(&tx_hash, 6)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, account.account_id);

        let batch_loaded = writer
            .get_dotbit_account_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[6])
            .unwrap();
        assert_eq!(batch_loaded.len(), 1);
        assert_eq!(batch_loaded[0].0, tx_hash);
        assert_eq!(batch_loaded[0].1, 6);
    }
}
