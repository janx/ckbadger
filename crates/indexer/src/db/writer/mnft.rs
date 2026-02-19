use anyhow::Result;
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{NftEntry, NftExtra, NftStandard, NftTypeIndex};

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
            standard: NftStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(issuer.owner_lock_hash.clone()),
            name: issuer.name.clone(),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            extra: NftExtra::MnftIssuer {
                class_count: issuer.class_count,
                set_count: issuer.set_count,
                info: issuer.info.clone(),
            },
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
            standard: NftStandard::MnftClass,
            collection_id: Some(class.issuer_id.clone()),
            token_id: None,
            owner_lock_hash: Some(class.owner_lock_hash.clone()),
            name: class.name.clone(),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            extra: NftExtra::MnftClass {
                description: class.description.clone(),
                renderer: class.renderer.clone(),
                total: class.total,
                issued: class.issued,
                configure: class.configure,
            },
        };
        batch.put_nft(&class.class_id, &entry);

        // Create/update NFT collection aggregate
        let mut agg = self
            .store
            .get_nft_collection_aggregate(&class.class_id)?
            .unwrap_or_default();
        agg.name = class.name.clone();
        agg.standard = NftStandard::MnftClass;
        batch.put_nft_collection_aggregate(&class.class_id, &agg);

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
        timestamp_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_nft(&token.token_id)?;
        let entry = NftEntry {
            standard: NftStandard::MnftToken,
            collection_id: Some(token.class_id.clone()),
            token_id: Some(token.token_id.clone()),
            owner_lock_hash: Some(token.owner_lock_hash.clone()),
            name: None,
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            extra: NftExtra::MnftToken {
                token_index: token.token_index,
                characteristic: token.characteristic.clone(),
                configure: token.configure,
                state: token.state,
            },
        };
        batch.put_nft(&token.token_id, &entry);

        // Update collection aggregate if this is a new token
        if existing.is_none() {
            let mut agg = self
                .store
                .get_nft_collection_aggregate(&token.class_id)?
                .unwrap_or_default();
            agg.total_count += 1;
            agg.live_count += 1;
            batch.put_nft_collection_aggregate(&token.class_id, &agg);
        } else {
            // Re-insert (transfer) — increment hourly bucket for 24h tracking
            let hour_bucket = timestamp_ms / 3_600_000;
            let key = ckbadger_store::keys::encode_nft_hourly_key(&token.class_id, hour_bucket);
            let current = match self.store.get_cf(self.store.cf_stats(), &key)? {
                Some(v) if v.len() == 8 => i64::from_le_bytes(v[..8].try_into().unwrap()),
                _ => 0,
            };
            batch.put_nft_hourly_transfer(&token.class_id, hour_bucket, current + 1);
        }

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
            let collection_id = entry.collection_id.clone();
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_nft(token_id, &entry);

            // Decrement collection's live_count
            if let Some(ref cid) = collection_id {
                let mut agg = self
                    .store
                    .get_nft_collection_aggregate(cid)?
                    .unwrap_or_default();
                agg.live_count -= 1;
                batch.put_nft_collection_aggregate(cid, &agg);
            }
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

    pub fn update_nft_type_index_batch(
        &self,
        changes: &HashMap<Vec<u8>, NftTypeIndex>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for (type_script_hash, index) in changes {
            batch.put_nft_type_index(type_script_hash, index);
        }
        Ok(())
    }

    pub fn update_nft_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i64, i64)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for ((collection_id, date), (capacity_delta, occupied_delta)) in changes {
            if *capacity_delta == 0 && *occupied_delta == 0 {
                continue;
            }
            let mut current = self
                .store
                .get_nft_daily_delta(collection_id, *date)?
                .unwrap_or_default();
            current.live_capacity_delta += *capacity_delta;
            current.live_occupied_capacity_delta += *occupied_delta;
            batch.put_nft_daily_delta(collection_id, *date, &current);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::BatchWriter;
    use ckbadger_store::store::CkbadgerStore;
    use std::sync::Arc;

    #[test]
    fn test_update_nft_type_index_and_daily_deltas_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

        let type_script_hash = vec![0x11; 32];
        let collection_id = vec![0x22; 24];
        let date = 20260219;

        let mut batch = StoreBatch::new(writer.store());
        let mut index_changes = HashMap::new();
        index_changes.insert(
            type_script_hash.clone(),
            NftTypeIndex {
                collection_id: collection_id.clone(),
            },
        );
        writer
            .update_nft_type_index_batch(&index_changes, &mut batch)
            .unwrap();

        let mut daily_changes = HashMap::new();
        daily_changes.insert((collection_id.clone(), date), (100, 61));
        writer
            .update_nft_daily_deltas_batch(&daily_changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let idx = writer
            .store()
            .get_nft_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(idx.collection_id, collection_id);

        let daily = writer
            .store()
            .get_nft_daily_delta(&[0x22; 24], date)
            .unwrap()
            .unwrap();
        assert_eq!(daily.live_capacity_delta, 100);
        assert_eq!(daily.live_occupied_capacity_delta, 61);
    }
}
