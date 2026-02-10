use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::NftEntry;

use crate::parser::mnft::{ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken};

use super::BatchWriter;

impl BatchWriter {
    pub fn insert_mnft_issuer(
        &self,
        issuer: &ParsedMnftIssuer,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_nft(&issuer.issuer_id)?;
        let entry = NftEntry {
            standard: "mnft_issuer".to_string(),
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(issuer.owner_lock_hash.clone()),
            name: issuer.name.clone(),
            metadata: issuer.info.as_ref().map(|i| {
                serde_json::json!({
                    "class_count": issuer.class_count,
                    "set_count": issuer.set_count,
                    "info": hex::encode(i),
                })
            }),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
        };
        batch.put_nft(&issuer.issuer_id, &entry);
        let _ = tx_hash; // Used for provenance tracking in PG, not needed in RocksDB
        Ok(())
    }

    pub fn consume_mnft_issuer(
        &self,
        issuer_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if let Some(mut entry) = self.store.get_nft(issuer_id)? {
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_nft(issuer_id, &entry);
        }
        Ok(())
    }

    pub fn insert_mnft_class(
        &self,
        class: &ParsedMnftClass,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_nft(&class.class_id)?;
        let entry = NftEntry {
            standard: "mnft_class".to_string(),
            collection_id: Some(class.issuer_id.clone()),
            token_id: None,
            owner_lock_hash: Some(class.owner_lock_hash.clone()),
            name: class.name.clone(),
            metadata: Some(serde_json::json!({
                "description": class.description,
                "renderer": class.renderer,
                "total": class.total,
                "issued": class.issued,
                "configure": class.configure,
            })),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
        };
        batch.put_nft(&class.class_id, &entry);
        let _ = tx_hash;
        Ok(())
    }

    pub fn consume_mnft_class(
        &self,
        class_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if let Some(mut entry) = self.store.get_nft(class_id)? {
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_nft(class_id, &entry);
        }
        Ok(())
    }

    pub fn get_mnft_class_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    pub fn insert_mnft_token(
        &self,
        token: &ParsedMnftToken,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_nft(&token.token_id)?;
        let entry = NftEntry {
            standard: "mnft".to_string(),
            collection_id: Some(token.class_id.clone()),
            token_id: Some(token.token_id.clone()),
            owner_lock_hash: Some(token.owner_lock_hash.clone()),
            name: None,
            metadata: Some(serde_json::json!({
                "token_index": token.token_index,
                "characteristic": hex::encode(&token.characteristic),
                "configure": token.configure,
                "state": token.state,
            })),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
        };
        batch.put_nft(&token.token_id, &entry);
        let _ = tx_hash;
        Ok(())
    }

    pub fn consume_mnft_token(
        &self,
        token_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if let Some(mut entry) = self.store.get_nft(token_id)? {
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_nft(token_id, &entry);
        }
        Ok(())
    }

    pub fn get_mnft_token_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        // In RocksDB model, token_id comes from type script args during parsing.
        Ok(None)
    }

    /// Batch lookup: find token_ids for multiple outpoints.
    /// In the RocksDB model, token_id comes from type script args during parsing.
    pub fn get_mnft_token_ids_by_outpoints_batch(
        &self,
        _tx_hashes: &[Vec<u8>],
        _output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        Ok(Vec::new())
    }
}
