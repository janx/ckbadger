use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{DobEntry, DobExtra, DobStandard};

use crate::parser::{ParsedClusterCell, ParsedSporeCell};

use super::BatchWriter;

impl BatchWriter {
    pub fn insert_spore_cluster(
        &self,
        cluster: &ParsedClusterCell,
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_spore(&cluster.cluster_id)?;
        let entry = DobEntry {
            standard: DobStandard::SporeCluster,
            collection_id: None, // This IS a cluster, not a spore in a cluster
            owner_lock_hash: Some(cluster.owner_lock_hash.clone()),
            name: cluster.name.clone(),
            description: cluster.description.clone(),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: DobExtra::SporeCluster,
        };
        batch.put_spore(&cluster.cluster_id, &entry);

        // Update cluster aggregate with name/description
        let mut agg = self
            .store
            .get_cluster_aggregate(&cluster.cluster_id)?
            .unwrap_or_default();
        agg.name = cluster.name.clone();
        agg.description = cluster.description.clone();
        batch.put_cluster_aggregate(&cluster.cluster_id, &agg);

        Ok(())
    }

    pub fn insert_spore_cell(
        &self,
        spore: &ParsedSporeCell,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_spore(&spore.spore_id)?;
        let entry = DobEntry {
            standard: DobStandard::Spore,
            collection_id: spore.cluster_id.clone(),
            owner_lock_hash: Some(spore.owner_lock_hash.clone()),
            name: None,
            description: None,
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: DobExtra::Spore {
                content_type: spore.content_type.clone(),
                content_length: spore.content.len() as i64,
            },
        };
        batch.put_spore(&spore.spore_id, &entry);

        // Write spore-by-cluster secondary index
        if let Some(ref cluster_id) = spore.cluster_id {
            batch.put_spore_by_cluster(cluster_id, &spore.spore_id);

            // Update cluster aggregate
            let mut agg = self
                .store
                .get_cluster_aggregate(cluster_id)?
                .unwrap_or_default();

            if existing.is_none() {
                // New spore: increment counts
                agg.total_count += 1;
                agg.live_count += 1;
            } else {
                // Re-insert (transfer) — increment hourly bucket for 24h tracking
                let hour_bucket = timestamp_ms / 3_600_000;
                let key = ckbadger_store::keys::encode_spore_hourly_key(cluster_id, hour_bucket);
                let current = match self.store.get_cf(self.store.cf_stats(), &key)? {
                    Some(v) if v.len() == 8 => i64::from_le_bytes(v[..8].try_into().unwrap()),
                    _ => 0,
                };
                batch.put_spore_hourly_transfer(cluster_id, hour_bucket, current + 1);
            }

            // Track owner changes
            let old_owner = existing.as_ref().and_then(|e| e.owner_lock_hash.clone());
            let new_owner = Some(spore.owner_lock_hash.clone());

            if old_owner != new_owner {
                // Decrement old owner's count
                if let Some(ref old_lock) = old_owner {
                    let old_count = self.store.get_cluster_owner_count(cluster_id, old_lock)?;
                    let new_count = old_count - 1;
                    if new_count <= 0 {
                        batch.delete_cluster_owner(cluster_id, old_lock);
                        agg.owner_count -= 1;
                    } else {
                        batch.put_cluster_owner_count(cluster_id, old_lock, new_count);
                    }
                }

                // Increment new owner's count
                if let Some(ref new_lock) = new_owner {
                    let cur_count = self.store.get_cluster_owner_count(cluster_id, new_lock)?;
                    if cur_count == 0 {
                        agg.owner_count += 1;
                    }
                    batch.put_cluster_owner_count(cluster_id, new_lock, cur_count + 1);
                }
            }

            batch.put_cluster_aggregate(cluster_id, &agg);
        }

        Ok(())
    }

    pub fn insert_spore_content(&self, _spore_id: &[u8], _content: &[u8]) -> Result<()> {
        // Spore content is large binary data. We don't store it in the indexer store —
        // it can be fetched from the CKB node's RocksDB when needed.
        Ok(())
    }

    pub fn consume_spore(
        &self,
        spore_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if let Some(mut entry) = self.store.get_spore(spore_id)? {
            let old_owner = entry.owner_lock_hash.clone();
            let cluster_id = entry.collection_id.clone();

            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_spore(spore_id, &entry);

            // Update cluster aggregate
            if let Some(ref cid) = cluster_id {
                let mut agg = self.store.get_cluster_aggregate(cid)?.unwrap_or_default();
                agg.live_count -= 1;

                // Decrement old owner's count
                if let Some(ref old_lock) = old_owner {
                    let old_count = self.store.get_cluster_owner_count(cid, old_lock)?;
                    let new_count = old_count - 1;
                    if new_count <= 0 {
                        batch.delete_cluster_owner(cid, old_lock);
                        agg.owner_count -= 1;
                    } else {
                        batch.put_cluster_owner_count(cid, old_lock, new_count);
                    }
                }

                batch.put_cluster_aggregate(cid, &agg);
            }
        }
        Ok(())
    }

    pub fn get_spore_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        // In the PG schema, spore_cells tracked tx_hash + output_index.
        // In RocksDB, spores are keyed by spore_id (from type_args).
        // The caller should already have the spore_id from the type script args.
        // This method is kept for API compatibility but returns None —
        // callers should use the spore_id directly from the parsed output.
        Ok(None)
    }

    /// Batch lookup: find spore_ids for multiple outpoints.
    /// In the RocksDB model, spore_id comes from type script args during parsing,
    /// not from a separate outpoint index. Returns empty — callers should use
    /// parsed data directly.
    pub fn get_spore_ids_by_outpoints_batch(
        &self,
        _tx_hashes: &[Vec<u8>],
        _output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        Ok(Vec::new())
    }
}
