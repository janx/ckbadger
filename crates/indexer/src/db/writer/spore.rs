use anyhow::{bail, Result};
use std::collections::HashMap;

use crate::parser::{ParsedClusterCell, ParsedSporeCell};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{ClusterAggregate, DobEntry, DobExtra, DobStandard, SporeTypeIndex};
use ckbadger_store::CkbadgerStore;

use super::BatchWriter;

#[derive(Default)]
pub(crate) struct SporeBatchState {
    spores: HashMap<Vec<u8>, Option<DobEntry>>,
    cluster_aggs: HashMap<Vec<u8>, ClusterAggregate>,
    cluster_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    spore_hourly_transfers: HashMap<Vec<u8>, i64>,
    spore_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>>,
}

impl SporeBatchState {
    fn get_spore(&mut self, store: &CkbadgerStore, spore_id: &[u8]) -> Result<Option<DobEntry>> {
        if let Some(cached) = self.spores.get(spore_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_spore(spore_id)?;
        self.spores.insert(spore_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_spore(&mut self, spore_id: &[u8], entry: DobEntry) {
        self.spores.insert(spore_id.to_vec(), Some(entry));
    }

    fn get_cluster_aggregate(
        &mut self,
        store: &CkbadgerStore,
        cluster_id: &[u8],
    ) -> Result<ClusterAggregate> {
        if let Some(cached) = self.cluster_aggs.get(cluster_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_cluster_aggregate(cluster_id)?.unwrap_or_default();
        self.cluster_aggs
            .insert(cluster_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_cluster_aggregate(
        &mut self,
        cluster_id: &[u8],
        agg: ClusterAggregate,
        batch: &mut StoreBatch,
    ) {
        batch.put_cluster_aggregate(cluster_id, &agg);
        self.cluster_aggs.insert(cluster_id.to_vec(), agg);
    }

    fn get_cluster_owner_count(
        &mut self,
        store: &CkbadgerStore,
        cluster_id: &[u8],
        lock_hash: &[u8],
    ) -> Result<i64> {
        let key = (cluster_id.to_vec(), lock_hash.to_vec());
        if let Some(cached) = self.cluster_owner_counts.get(&key) {
            return Ok(*cached);
        }
        let loaded = store.get_cluster_owner_count(cluster_id, lock_hash)?;
        self.cluster_owner_counts.insert(key, loaded);
        Ok(loaded)
    }

    fn put_cluster_owner_count(
        &mut self,
        cluster_id: &[u8],
        lock_hash: &[u8],
        count: i64,
        batch: &mut StoreBatch,
    ) {
        batch.put_cluster_owner_count(cluster_id, lock_hash, count);
        self.cluster_owner_counts
            .insert((cluster_id.to_vec(), lock_hash.to_vec()), count);
    }

    fn delete_cluster_owner(
        &mut self,
        cluster_id: &[u8],
        lock_hash: &[u8],
        batch: &mut StoreBatch,
    ) {
        batch.delete_cluster_owner(cluster_id, lock_hash);
        self.cluster_owner_counts
            .insert((cluster_id.to_vec(), lock_hash.to_vec()), 0);
    }

    fn get_spore_hourly_transfer(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        if let Some(cached) = self.spore_hourly_transfers.get(key) {
            return Ok(*cached);
        }
        let loaded = match store.get_cf(store.cf_stats(), key)? {
            Some(v) if v.len() == 8 => i64::from_le_bytes(v[..8].try_into().unwrap()),
            _ => 0,
        };
        self.spore_hourly_transfers.insert(key.to_vec(), loaded);
        Ok(loaded)
    }

    fn put_spore_hourly_transfer(&mut self, key: Vec<u8>, count: i64) {
        self.spore_hourly_transfers.insert(key, count);
    }

    pub(crate) fn put_spore_outpoint(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        spore_id: &[u8],
    ) {
        self.spore_outpoints
            .insert((tx_hash.to_vec(), output_index), spore_id.to_vec());
    }

    pub(crate) fn get_cached_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Option<Vec<u8>> {
        self.spore_outpoints
            .get(&(tx_hash.to_vec(), output_index))
            .cloned()
    }
}

impl BatchWriter {
    pub(crate) fn new_spore_batch_state(&self) -> SporeBatchState {
        SporeBatchState::default()
    }

    fn apply_owner_transition(
        &self,
        cluster_id: &[u8],
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut ClusterAggregate,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_lock) = old_owner {
            let old_count =
                state.get_cluster_owner_count(self.store.as_ref(), cluster_id, old_lock)?;
            if old_count <= 0 {
                bail!(
                    "spore owner count underflow: cluster_id=0x{}, lock_hash=0x{}, owner_count={}",
                    hex::encode(cluster_id),
                    hex::encode(old_lock),
                    old_count
                );
            } else if old_count == 1 {
                if agg.owner_count <= 0 {
                    bail!(
                        "spore aggregate owner_count underflow: cluster_id=0x{}, owner_count={}",
                        hex::encode(cluster_id),
                        agg.owner_count
                    );
                }
                state.delete_cluster_owner(cluster_id, old_lock, batch);
                agg.owner_count -= 1;
            } else {
                state.put_cluster_owner_count(cluster_id, old_lock, old_count - 1, batch);
            }
        }

        if let Some(new_lock) = new_owner {
            let cur_count =
                state.get_cluster_owner_count(self.store.as_ref(), cluster_id, new_lock)?;
            if cur_count == 0 {
                agg.owner_count = agg
                    .owner_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("spore aggregate owner_count overflow"))?;
            }
            let next = cur_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("spore owner count overflow"))?;
            state.put_cluster_owner_count(cluster_id, new_lock, next, batch);
        }

        Ok(())
    }

    pub(crate) fn insert_spore_cluster(
        &self,
        cluster: &ParsedClusterCell,
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        let existing = state.get_spore(self.store.as_ref(), &cluster.cluster_id)?;
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
        state.put_spore(&cluster.cluster_id, entry);

        // Update cluster aggregate with name/description
        let mut agg = state.get_cluster_aggregate(self.store.as_ref(), &cluster.cluster_id)?;
        agg.name = cluster.name.clone();
        agg.description = cluster.description.clone();
        state.put_cluster_aggregate(&cluster.cluster_id, agg, batch);

        Ok(())
    }

    pub(crate) fn insert_spore_cell(
        &self,
        spore: &ParsedSporeCell,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        let existing = state.get_spore(self.store.as_ref(), &spore.spore_id)?;
        let was_live = existing.as_ref().is_some_and(|e| e.is_live);
        let old_cluster = existing.as_ref().and_then(|e| e.collection_id.clone());
        let old_owner = if was_live {
            existing.as_ref().and_then(|e| e.owner_lock_hash.clone())
        } else {
            None
        };
        let new_cluster = spore.cluster_id.clone();
        let entry = DobEntry {
            standard: DobStandard::Spore,
            collection_id: new_cluster.clone(),
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
        state.put_spore(&spore.spore_id, entry);
        batch.put_spore_outpoint(tx_hash, output_index, &spore.spore_id);
        state.put_spore_outpoint(tx_hash, output_index, &spore.spore_id);

        if old_cluster != new_cluster {
            if let Some(ref old_cluster_id) = old_cluster {
                batch.delete_spore_by_cluster(old_cluster_id, &spore.spore_id);

                if was_live {
                    let mut old_agg =
                        state.get_cluster_aggregate(self.store.as_ref(), old_cluster_id)?;
                    if old_agg.live_count <= 0 {
                        bail!(
                            "spore aggregate live_count underflow on cluster move: cluster_id=0x{}, live_count={}",
                            hex::encode(old_cluster_id),
                            old_agg.live_count
                        );
                    }
                    old_agg.live_count -= 1;
                    self.apply_owner_transition(
                        old_cluster_id,
                        old_owner.as_deref(),
                        None,
                        &mut old_agg,
                        batch,
                        state,
                    )?;
                    state.put_cluster_aggregate(old_cluster_id, old_agg, batch);
                }
            }
        }

        // Write spore-by-cluster secondary index and update target cluster aggregates.
        if let Some(ref cluster_id) = new_cluster {
            batch.put_spore_by_cluster(cluster_id, &spore.spore_id);

            // Update cluster aggregate
            let mut agg = state.get_cluster_aggregate(self.store.as_ref(), cluster_id)?;

            if existing.is_none() {
                // New spore: increment counts
                agg.total_count = agg.total_count.saturating_add(1);
                agg.live_count = agg.live_count.saturating_add(1);
            } else if !was_live || old_cluster.as_ref() != Some(cluster_id) {
                // Re-activate consumed spore or move a live spore to another cluster.
                agg.live_count = agg.live_count.saturating_add(1);
            } else {
                // Re-insert (transfer) — increment hourly bucket for 24h tracking
                let hour_bucket = timestamp_ms / 3_600_000;
                let key = ckbadger_store::keys::encode_spore_hourly_key(cluster_id, hour_bucket);
                let current = state.get_spore_hourly_transfer(self.store.as_ref(), &key)?;
                let next = current.saturating_add(1);
                batch.put_spore_hourly_transfer(cluster_id, hour_bucket, next);
                state.put_spore_hourly_transfer(key, next);
            }

            let owner_from = if was_live && old_cluster.as_ref() == Some(cluster_id) {
                old_owner.as_deref()
            } else {
                None
            };
            self.apply_owner_transition(
                cluster_id,
                owner_from,
                Some(spore.owner_lock_hash.as_slice()),
                &mut agg,
                batch,
                state,
            )?;
            state.put_cluster_aggregate(cluster_id, agg, batch);
        }

        Ok(())
    }

    pub fn insert_spore_content(&self, _spore_id: &[u8], _content: &[u8]) -> Result<()> {
        // Spore content is large binary data. We don't store it in the indexer store —
        // it can be fetched from the CKB node's RocksDB when needed.
        Ok(())
    }

    pub(crate) fn consume_spore(
        &self,
        spore_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        if let Some(mut entry) = state.get_spore(self.store.as_ref(), spore_id)? {
            if !entry.is_live {
                return Ok(());
            }

            let old_owner = entry.owner_lock_hash.clone();
            let cluster_id = entry.collection_id.clone();

            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_spore(spore_id, &entry);
            state.put_spore(spore_id, entry);

            // Update cluster aggregate
            if let Some(ref cid) = cluster_id {
                let mut agg = state.get_cluster_aggregate(self.store.as_ref(), cid)?;
                if agg.live_count <= 0 {
                    bail!(
                        "spore aggregate live_count underflow on consume: cluster_id=0x{}, live_count={}",
                        hex::encode(cid),
                        agg.live_count
                    );
                }
                agg.live_count -= 1;
                self.apply_owner_transition(
                    cid,
                    old_owner.as_deref(),
                    None,
                    &mut agg,
                    batch,
                    state,
                )?;
                state.put_cluster_aggregate(cid, agg, batch);
            }
        }
        Ok(())
    }

    pub fn update_spore_type_index_batch(
        &self,
        changes: &HashMap<Vec<u8>, SporeTypeIndex>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for (type_script_hash, index) in changes {
            batch.put_spore_type_index(type_script_hash, index);
        }
        Ok(())
    }

    pub fn update_spore_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for ((spore_id, date), (capacity_delta, occupied_delta)) in changes {
            if *capacity_delta == 0 && *occupied_delta == 0 {
                continue;
            }
            let mut current = self
                .store
                .get_spore_daily_delta(spore_id, *date)?
                .unwrap_or_default();
            current.live_capacity_delta = current
                .live_capacity_delta
                .checked_add(*capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "spore daily capacity delta overflow: spore_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(spore_id),
                        date,
                        current.live_capacity_delta,
                        capacity_delta
                    )
                })?;
            current.live_occupied_capacity_delta = current
                .live_occupied_capacity_delta
                .checked_add(*occupied_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "spore daily occupied delta overflow: spore_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(spore_id),
                        date,
                        current.live_occupied_capacity_delta,
                        occupied_delta
                    )
                })?;
            if current.live_capacity_delta == 0 && current.live_occupied_capacity_delta == 0 {
                let key = keys::encode_spore_daily_key(spore_id, *date);
                batch.delete_stats(&key);
            } else {
                batch.put_spore_daily_delta(spore_id, *date, &current);
            }
        }
        Ok(())
    }

    pub fn update_cluster_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for ((cluster_id, date), (capacity_delta, occupied_delta)) in changes {
            if *capacity_delta == 0 && *occupied_delta == 0 {
                continue;
            }
            let mut current = self
                .store
                .get_cluster_daily_delta(cluster_id, *date)?
                .unwrap_or_default();
            current.live_capacity_delta = current
                .live_capacity_delta
                .checked_add(*capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster daily capacity delta overflow: cluster_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(cluster_id),
                        date,
                        current.live_capacity_delta,
                        capacity_delta
                    )
                })?;
            current.live_occupied_capacity_delta = current
                .live_occupied_capacity_delta
                .checked_add(*occupied_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster daily occupied delta overflow: cluster_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(cluster_id),
                        date,
                        current.live_occupied_capacity_delta,
                        occupied_delta
                    )
                })?;
            if current.live_capacity_delta == 0 && current.live_occupied_capacity_delta == 0 {
                let key = keys::encode_cluster_daily_key(cluster_id, *date);
                batch.delete_stats(&key);
            } else {
                batch.put_cluster_daily_delta(cluster_id, *date, &current);
            }
        }
        Ok(())
    }

    pub fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        self.store.get_spore_id_by_outpoint(tx_hash, output_index)
    }

    /// Batch lookup: find spore_ids for multiple outpoints using persisted outpoint index.
    pub fn get_spore_ids_by_outpoints_batch(
        &self,
        tx_hashes: &[Vec<u8>],
        output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        let outpoints: Vec<(&[u8], i16)> = tx_hashes
            .iter()
            .zip(output_indices.iter())
            .map(|(hash, idx)| (hash.as_slice(), *idx))
            .collect();
        Ok(self.store.get_spore_ids_by_outpoints_batch(&outpoints))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::BatchWriter;
    use ckbadger_store::store::CkbadgerStore;
    use std::sync::Arc;

    fn make_spore_entry(cluster_id: &[u8], owner_lock: &[u8]) -> DobEntry {
        DobEntry {
            standard: DobStandard::Spore,
            collection_id: Some(cluster_id.to_vec()),
            owner_lock_hash: Some(owner_lock.to_vec()),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0xAA; 32],
            extra: DobExtra::Spore {
                content_type: "text/plain".to_string(),
                content_length: 4,
            },
        }
    }

    fn make_parsed_spore(spore_id: &[u8], cluster_id: &[u8], owner_lock: &[u8]) -> ParsedSporeCell {
        ParsedSporeCell {
            spore_id: spore_id.to_vec(),
            type_script_hash: vec![0x99; 32],
            content_type: "text/plain".to_string(),
            content: b"test".to_vec(),
            cluster_id: Some(cluster_id.to_vec()),
            owner_lock_hash: owner_lock.to_vec(),
        }
    }

    #[test]
    fn test_update_spore_daily_and_cluster_daily_deltas_batch_accumulates_and_deletes_zero_net() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

        let spore_id = vec![0x11; 32];
        let cluster_id = vec![0x22; 32];
        let date = 20260219;

        {
            let mut batch = StoreBatch::new(writer.store());
            let mut spore_changes = HashMap::new();
            spore_changes.insert((spore_id.clone(), date), (100, 61));
            writer
                .update_spore_daily_deltas_batch(&spore_changes, &mut batch)
                .unwrap();

            let mut cluster_changes = HashMap::new();
            cluster_changes.insert((cluster_id.clone(), date), (1000, 610));
            writer
                .update_cluster_daily_deltas_batch(&cluster_changes, &mut batch)
                .unwrap();
            batch.commit().unwrap();
        }

        {
            let mut batch = StoreBatch::new(writer.store());
            let mut spore_changes = HashMap::new();
            spore_changes.insert((spore_id.clone(), date), (-20, -11));
            writer
                .update_spore_daily_deltas_batch(&spore_changes, &mut batch)
                .unwrap();

            let mut cluster_changes = HashMap::new();
            cluster_changes.insert((cluster_id.clone(), date), (-200, -110));
            writer
                .update_cluster_daily_deltas_batch(&cluster_changes, &mut batch)
                .unwrap();
            batch.commit().unwrap();
        }

        let spore = writer
            .store()
            .get_spore_daily_delta(&spore_id, date)
            .unwrap()
            .unwrap();
        assert_eq!(spore.live_capacity_delta, 80);
        assert_eq!(spore.live_occupied_capacity_delta, 50);

        let cluster = writer
            .store()
            .get_cluster_daily_delta(&cluster_id, date)
            .unwrap()
            .unwrap();
        assert_eq!(cluster.live_capacity_delta, 800);
        assert_eq!(cluster.live_occupied_capacity_delta, 500);

        {
            let mut batch = StoreBatch::new(writer.store());
            let mut spore_changes = HashMap::new();
            spore_changes.insert((spore_id.clone(), date), (-80, -50));
            writer
                .update_spore_daily_deltas_batch(&spore_changes, &mut batch)
                .unwrap();

            let mut cluster_changes = HashMap::new();
            cluster_changes.insert((cluster_id.clone(), date), (-800, -500));
            writer
                .update_cluster_daily_deltas_batch(&cluster_changes, &mut batch)
                .unwrap();
            batch.commit().unwrap();
        }

        let spore = writer
            .store()
            .get_spore_daily_delta(&spore_id, date)
            .unwrap();
        assert!(spore.is_none());

        let cluster = writer
            .store()
            .get_cluster_daily_delta(&cluster_id, date)
            .unwrap();
        assert!(cluster.is_none());
    }

    #[test]
    fn test_update_spore_type_index_batch_writes_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

        let type_script_hash = vec![0x33; 32];
        let mut changes = HashMap::new();
        changes.insert(
            type_script_hash.clone(),
            SporeTypeIndex {
                spore_id: vec![0x44; 32],
                cluster_id: Some(vec![0x55; 32]),
            },
        );

        let mut batch = StoreBatch::new(writer.store());
        writer
            .update_spore_type_index_batch(&changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let loaded = writer
            .store()
            .get_spore_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.spore_id, vec![0x44; 32]);
        assert_eq!(loaded.cluster_id, Some(vec![0x55; 32]));
    }

    #[test]
    fn test_insert_spore_cell_keeps_owner_counts_consistent_for_multi_transfer_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let cluster_id = vec![0x11; 32];
        let spore_id = vec![0x22; 32];
        let owner_a = vec![0xA1; 32];
        let owner_b = vec![0xB2; 32];

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_spore(&spore_id, &make_spore_entry(&cluster_id, &owner_a));
            seed.put_cluster_aggregate(
                &cluster_id,
                &ClusterAggregate {
                    total_count: 1,
                    live_count: 1,
                    owner_count: 1,
                    ..Default::default()
                },
            );
            seed.put_cluster_owner_count(&cluster_id, &owner_a, 1);
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &cluster_id, &owner_b),
                &[0x01; 32],
                0,
                10,
                3_600_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &cluster_id, &owner_a),
                &[0x02; 32],
                0,
                10,
                3_600_001,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(agg.owner_count, 1);
        assert_eq!(agg.live_count, 1);
        assert_eq!(
            store
                .get_cluster_owner_count(&cluster_id, &owner_a)
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .get_cluster_owner_count(&cluster_id, &owner_b)
                .unwrap(),
            0
        );
    }

    #[test]
    fn test_insert_spore_cell_errors_on_owner_count_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let cluster_id = vec![0x33; 32];
        let spore_id = vec![0x44; 32];
        let owner_old = vec![0xC1; 32];
        let owner_new = vec![0xD2; 32];

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_spore(&spore_id, &make_spore_entry(&cluster_id, &owner_old));
            // Deliberately inconsistent seed: missing owner key but aggregate says 0.
            seed.put_cluster_aggregate(
                &cluster_id,
                &ClusterAggregate {
                    total_count: 1,
                    live_count: 1,
                    owner_count: 0,
                    ..Default::default()
                },
            );
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let err = writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &cluster_id, &owner_new),
                &[0x03; 32],
                0,
                11,
                7_200_000,
                &mut batch,
                &mut state,
            )
            .unwrap_err();
        assert!(err.to_string().contains("owner count underflow"));
    }

    #[test]
    fn test_insert_spore_cell_errors_on_live_count_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let old_cluster = vec![0x41; 32];
        let new_cluster = vec![0x42; 32];
        let spore_id = vec![0x43; 32];
        let owner_old = vec![0x44; 32];
        let owner_new = vec![0x45; 32];

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_spore(&spore_id, &make_spore_entry(&old_cluster, &owner_old));
            seed.put_cluster_aggregate(
                &old_cluster,
                &ClusterAggregate {
                    total_count: 1,
                    live_count: 0,
                    owner_count: 1,
                    ..Default::default()
                },
            );
            seed.put_cluster_owner_count(&old_cluster, &owner_old, 1);
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let err = writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &new_cluster, &owner_new),
                &[0x05; 32],
                0,
                12,
                10_800_000,
                &mut batch,
                &mut state,
            )
            .unwrap_err();
        assert!(err.to_string().contains("live_count underflow"));
    }

    #[test]
    fn test_consume_spore_errors_on_live_count_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let cluster_id = vec![0x51; 32];
        let spore_id = vec![0x61; 32];
        let owner = vec![0x71; 32];

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_spore(&spore_id, &make_spore_entry(&cluster_id, &owner));
            seed.put_cluster_aggregate(
                &cluster_id,
                &ClusterAggregate {
                    total_count: 1,
                    live_count: 0,
                    owner_count: 1,
                    ..Default::default()
                },
            );
            seed.put_cluster_owner_count(&cluster_id, &owner, 1);
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let err = writer
            .consume_spore(&spore_id, 100, &[0xAA; 32], &mut batch, &mut state)
            .unwrap_err();
        assert!(err.to_string().contains("live_count underflow"));
    }

    #[test]
    fn test_insert_spore_cell_cluster_move_updates_old_and_new_cluster_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let old_cluster = vec![0x51; 32];
        let new_cluster = vec![0x52; 32];
        let spore_id = vec![0x61; 32];
        let owner_old = vec![0x71; 32];
        let owner_new = vec![0x72; 32];

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_spore(&spore_id, &make_spore_entry(&old_cluster, &owner_old));
            seed.put_cluster_aggregate(
                &old_cluster,
                &ClusterAggregate {
                    total_count: 1,
                    live_count: 1,
                    owner_count: 1,
                    ..Default::default()
                },
            );
            seed.put_cluster_owner_count(&old_cluster, &owner_old, 1);
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &new_cluster, &owner_new),
                &[0x04; 32],
                0,
                12,
                10_800_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let old_agg = store.get_cluster_aggregate(&old_cluster).unwrap().unwrap();
        let new_agg = store.get_cluster_aggregate(&new_cluster).unwrap().unwrap();
        assert_eq!(old_agg.live_count, 0);
        assert_eq!(old_agg.owner_count, 0);
        assert_eq!(new_agg.live_count, 1);
        assert_eq!(new_agg.owner_count, 1);
    }

    #[test]
    fn test_insert_spore_cell_populates_outpoint_lookup_and_batch_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let cluster_id = vec![0x81; 32];
        let spore_id = vec![0x91; 32];
        let owner = vec![0xA1; 32];
        let tx_hash = vec![0xB1; 32];
        let output_index = 3i16;

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &cluster_id, &owner),
                &tx_hash,
                output_index,
                20,
                14_400_000,
                &mut batch,
                &mut state,
            )
            .unwrap();

        // Before commit, lookup is available from in-batch cache.
        let cached = state
            .get_cached_spore_id_by_outpoint(&tx_hash, output_index)
            .unwrap();
        assert_eq!(cached, spore_id);

        batch.commit().unwrap();

        // After commit, persistent outpoint index resolves through store.
        let loaded = writer
            .get_spore_id_by_outpoint(&tx_hash, output_index)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, spore_id);
    }
}
