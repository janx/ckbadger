use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::SporeEntry;

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
        // Clusters are stored as SporeEntry with cluster-specific fields
        let existing = self.store.get_spore(&cluster.cluster_id)?;
        let entry = SporeEntry {
            cluster_id: None, // This IS a cluster, not a spore in a cluster
            content_type: Some("cluster".to_string()),
            content_length: None,
            owner_lock_hash: Some(cluster.owner_lock_hash.clone()),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            name: cluster.name.clone(),
            description: cluster.description.clone(),
        };
        batch.put_spore(&cluster.cluster_id, &entry);
        Ok(())
    }

    pub fn insert_spore_cell(
        &self,
        spore: &ParsedSporeCell,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_spore(&spore.spore_id)?;
        let entry = SporeEntry {
            cluster_id: spore.cluster_id.clone(),
            content_type: Some(spore.content_type.clone()),
            content_length: Some(spore.content.len() as i64),
            owner_lock_hash: Some(spore.owner_lock_hash.clone()),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            name: None,
            description: None,
        };
        batch.put_spore(&spore.spore_id, &entry);
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
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_spore(spore_id, &entry);
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
