use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{NftEntry, NftExtra, NftStandard};

use crate::parser::dotbit::ParsedDotbitAccount;

use super::BatchWriter;

impl BatchWriter {
    pub fn insert_dotbit_account(
        &self,
        account: &ParsedDotbitAccount,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
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
        let _ = tx_hash;
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
        }
        Ok(())
    }

    pub fn get_dotbit_account_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        // In RocksDB model, account_id comes from type script args during parsing.
        Ok(None)
    }

    /// Batch lookup: find account_ids for multiple outpoints.
    /// In the RocksDB model, account_id comes from type script args during parsing.
    pub fn get_dotbit_account_ids_by_outpoints_batch(
        &self,
        _tx_hashes: &[Vec<u8>],
        _output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        Ok(Vec::new())
    }
}
