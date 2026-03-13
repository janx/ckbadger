use anyhow::{bail, Result};
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    ObjectCollectionAggregate, ObjectEntry, ObjectExtra, ObjectStandard, ObjectTypeIndex,
};
use ckbadger_store::CkbadgerStore;

use crate::parser::mnft::{ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken};

use super::BatchWriter;

#[derive(Default)]
pub(crate) struct MnftBatchState {
    tokens: HashMap<Vec<u8>, Option<ObjectEntry>>,
    collection_aggs: HashMap<Vec<u8>, Option<ObjectCollectionAggregate>>,
    collection_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    hourly_transfers: HashMap<Vec<u8>, i64>,
}

impl MnftBatchState {
    fn get_token(&mut self, store: &CkbadgerStore, token_id: &[u8]) -> Result<Option<ObjectEntry>> {
        if let Some(cached) = self.tokens.get(token_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_object(token_id)?;
        self.tokens.insert(token_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_token(&mut self, token_id: &[u8], entry: ObjectEntry) {
        self.tokens.insert(token_id.to_vec(), Some(entry));
    }

    fn get_collection_aggregate(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
    ) -> Result<Option<ObjectCollectionAggregate>> {
        if let Some(cached) = self.collection_aggs.get(collection_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_object_collection_aggregate(collection_id)?;
        self.collection_aggs
            .insert(collection_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_collection_aggregate(
        &mut self,
        collection_id: &[u8],
        agg: ObjectCollectionAggregate,
        batch: &mut StoreBatch,
    ) {
        batch.put_object_collection_aggregate(collection_id, &agg);
        self.collection_aggs
            .insert(collection_id.to_vec(), Some(agg));
    }

    pub(crate) fn extend_pending_collection_aggregates(
        &self,
        target: &mut HashMap<Vec<u8>, ObjectCollectionAggregate>,
    ) {
        for (collection_id, agg) in &self.collection_aggs {
            if let Some(agg) = agg {
                target.insert(collection_id.clone(), agg.clone());
            }
        }
    }

    fn get_collection_owner_count(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
        lock_hash: &[u8],
    ) -> Result<i64> {
        let key = (collection_id.to_vec(), lock_hash.to_vec());
        if let Some(cached) = self.collection_owner_counts.get(&key) {
            return Ok(*cached);
        }
        let loaded = store.get_object_collection_owner_count(collection_id, lock_hash)?;
        self.collection_owner_counts.insert(key, loaded);
        Ok(loaded)
    }

    fn put_collection_owner_count(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        count: i64,
        batch: &mut StoreBatch,
    ) {
        batch.put_object_collection_owner_count(collection_id, lock_hash, count);
        self.collection_owner_counts
            .insert((collection_id.to_vec(), lock_hash.to_vec()), count);
    }

    fn delete_collection_owner(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        batch: &mut StoreBatch,
    ) {
        batch.delete_object_collection_owner(collection_id, lock_hash);
        self.collection_owner_counts
            .insert((collection_id.to_vec(), lock_hash.to_vec()), 0);
    }

    fn get_hourly_transfer(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        if let Some(cached) = self.hourly_transfers.get(key) {
            return Ok(*cached);
        }
        let loaded = match store.get_stats_key(key)? {
            Some(v) => {
                if v.len() != 8 {
                    bail!(
                        "invalid mNFT hourly transfer value length in stats CF: key=0x{}, len={}",
                        hex::encode(key),
                        v.len()
                    );
                }
                i64::from_le_bytes(v[..8].try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "failed to decode mNFT hourly transfer value as i64: key=0x{}",
                        hex::encode(key)
                    )
                })?)
            }
            None => 0,
        };
        self.hourly_transfers.insert(key.to_vec(), loaded);
        Ok(loaded)
    }

    fn put_hourly_transfer(&mut self, key: Vec<u8>, count: i64) {
        self.hourly_transfers.insert(key, count);
    }
}

impl BatchWriter {
    pub(crate) fn new_mnft_batch_state(&self) -> MnftBatchState {
        MnftBatchState::default()
    }

    fn apply_mnft_owner_transition(
        &self,
        collection_id: &[u8],
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut ObjectCollectionAggregate,
        batch: &mut StoreBatch,
        state: &mut MnftBatchState,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_lock) = old_owner {
            let old_count =
                state.get_collection_owner_count(self.store.as_ref(), collection_id, old_lock)?;
            if old_count <= 0 {
                bail!(
                    "mnft owner count underflow: class_id=0x{}, lock_hash=0x{}, owner_count={}",
                    hex::encode(collection_id),
                    hex::encode(old_lock),
                    old_count
                );
            } else if old_count == 1 {
                if agg.holders_count <= 0 {
                    bail!(
                        "mnft collection holders_count underflow: class_id=0x{}, holders_count={}",
                        hex::encode(collection_id),
                        agg.holders_count
                    );
                }
                state.delete_collection_owner(collection_id, old_lock, batch);
                agg.holders_count -= 1;
            } else {
                state.put_collection_owner_count(collection_id, old_lock, old_count - 1, batch);
            }
        }

        if let Some(new_lock) = new_owner {
            let current =
                state.get_collection_owner_count(self.store.as_ref(), collection_id, new_lock)?;
            if current == 0 {
                agg.holders_count = agg.holders_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "mnft collection holders_count overflow: class_id=0x{}, lock_hash=0x{}",
                        hex::encode(collection_id),
                        hex::encode(new_lock)
                    )
                })?;
            }
            let next = current.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "mnft owner count overflow: class_id=0x{}, lock_hash=0x{}, current={}",
                    hex::encode(collection_id),
                    hex::encode(new_lock),
                    current
                )
            })?;
            state.put_collection_owner_count(collection_id, new_lock, next, batch);
        }

        Ok(())
    }

    pub fn insert_mnft_issuer(
        &self,
        issuer: &ParsedMnftIssuer,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_object(&issuer.issuer_id)?;
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(issuer.owner_lock_hash.clone()),
            name: issuer.name.clone(),
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
            extra: ObjectExtra::MnftIssuer {
                class_count: issuer.class_count,
                set_count: issuer.set_count,
                info: issuer.info.clone(),
            },
        };
        batch.put_object(&issuer.issuer_id, &entry);
        Ok(())
    }

    pub fn insert_mnft_class(
        &self,
        class: &ParsedMnftClass,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut state = self.new_mnft_batch_state();
        self.insert_mnft_class_with_state(
            class,
            tx_hash,
            output_index,
            block_number,
            batch,
            &mut state,
        )
    }

    pub(crate) fn insert_mnft_class_with_state(
        &self,
        class: &ParsedMnftClass,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
        state: &mut MnftBatchState,
    ) -> Result<()> {
        let existing = state.get_token(self.store.as_ref(), &class.class_id)?;
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(class.issuer_id.clone()),
            token_id: None,
            owner_lock_hash: Some(class.owner_lock_hash.clone()),
            name: class.name.clone(),
            description: class.description.clone(),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: ObjectExtra::MnftClass {
                description: class.description.clone(),
                renderer: class.renderer.clone(),
                total: class.total,
                issued: class.issued,
                configure: class.configure,
            },
        };
        batch.put_object(&class.class_id, &entry);
        state.put_token(&class.class_id, entry);

        // Create/update object collection aggregate
        let mut agg = state
            .get_collection_aggregate(self.store.as_ref(), &class.class_id)?
            .unwrap_or_default();
        agg.name = class.name.clone();
        agg.standard = ObjectStandard::MnftClass;
        state.put_collection_aggregate(&class.class_id, agg, batch);
        batch.put_mnft_class_outpoint(tx_hash, output_index, &class.class_id);
        Ok(())
    }

    pub fn get_mnft_class_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        self.store
            .get_mnft_class_id_by_outpoint(tx_hash, output_index)
    }

    pub fn insert_mnft_token(
        &self,
        token: &ParsedMnftToken,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut state = self.new_mnft_batch_state();
        self.insert_mnft_token_with_state(
            token,
            tx_hash,
            output_index,
            block_number,
            timestamp_ms,
            batch,
            &mut state,
        )
    }

    pub(crate) fn insert_mnft_token_with_state(
        &self,
        token: &ParsedMnftToken,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
        state: &mut MnftBatchState,
    ) -> Result<()> {
        let existing = state.get_token(self.store.as_ref(), &token.token_id)?;
        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(token.class_id.clone()),
            token_id: Some(token.token_id.clone()),
            owner_lock_hash: Some(token.owner_lock_hash.clone()),
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
            extra: ObjectExtra::MnftToken {
                token_index: token.token_index,
                characteristic: token.characteristic.clone(),
                configure: token.configure,
                state: token.state,
            },
        };
        batch.put_object(&token.token_id, &entry);
        state.put_token(&token.token_id, entry);
        let should_upsert_collection_index = !existing
            .as_ref()
            .is_some_and(|e| e.is_live && e.collection_id.as_ref() == Some(&token.class_id));
        if should_upsert_collection_index {
            batch.put_object_by_collection(&token.class_id, &token.token_id);
        }

        // Update collection aggregate if this is a new token
        if existing.is_none() {
            let mut agg = state
                .get_collection_aggregate(self.store.as_ref(), &token.class_id)?
                .unwrap_or_default();
            agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "mnft collection total_count overflow: class_id=0x{}, token_id=0x{}, total_count={}",
                    hex::encode(&token.class_id),
                    hex::encode(&token.token_id),
                    agg.total_count
                )
            })?;
            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "mnft collection live_count overflow: class_id=0x{}, token_id=0x{}, live_count={}",
                    hex::encode(&token.class_id),
                    hex::encode(&token.token_id),
                    agg.live_count
                )
            })?;
            self.apply_mnft_owner_transition(
                &token.class_id,
                None,
                Some(token.owner_lock_hash.as_slice()),
                &mut agg,
                batch,
                state,
            )?;
            state.put_collection_aggregate(&token.class_id, agg, batch);
        } else if !was_live {
            let Some(mut agg) =
                state.get_collection_aggregate(self.store.as_ref(), &token.class_id)?
            else {
                bail!(
                    "mnft collection aggregate missing while re-activating token: class_id=0x{}, token_id=0x{}",
                    hex::encode(&token.class_id),
                    hex::encode(&token.token_id)
                );
            };
            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "mnft collection live_count overflow while re-activating token: class_id=0x{}, token_id=0x{}, live_count={}",
                    hex::encode(&token.class_id),
                    hex::encode(&token.token_id),
                    agg.live_count
                )
            })?;
            self.apply_mnft_owner_transition(
                &token.class_id,
                None,
                Some(token.owner_lock_hash.as_slice()),
                &mut agg,
                batch,
                state,
            )?;
            state.put_collection_aggregate(&token.class_id, agg, batch);
        } else {
            if old_owner.is_none() {
                bail!(
                    "mnft live token missing owner_lock_hash during transfer: class_id=0x{}, token_id=0x{}",
                    hex::encode(&token.class_id),
                    hex::encode(&token.token_id)
                );
            }
            let Some(mut agg) =
                state.get_collection_aggregate(self.store.as_ref(), &token.class_id)?
            else {
                bail!(
                    "mnft collection aggregate missing while transferring token: class_id=0x{}, token_id=0x{}",
                    hex::encode(&token.class_id),
                    hex::encode(&token.token_id)
                );
            };
            self.apply_mnft_owner_transition(
                &token.class_id,
                old_owner.as_deref(),
                Some(token.owner_lock_hash.as_slice()),
                &mut agg,
                batch,
                state,
            )?;
            state.put_collection_aggregate(&token.class_id, agg, batch);

            // Re-insert (transfer) — increment hourly bucket for 24h tracking
            let hour_bucket = timestamp_ms / 3_600_000;
            let key = ckbadger_store::keys::encode_nft_hourly_key(&token.class_id, hour_bucket);
            let current = state.get_hourly_transfer(self.store.as_ref(), &key)?;
            let next = current.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "mnft hourly transfer overflow: class_id=0x{}, hour_bucket={}, current={}, token_id=0x{}",
                    hex::encode(&token.class_id),
                    hour_bucket,
                    current,
                    hex::encode(&token.token_id)
                )
            })?;
            batch.put_object_hourly_transfer(&token.class_id, hour_bucket, next);
            state.put_hourly_transfer(key, next);
        }
        batch.put_mnft_token_outpoint(tx_hash, output_index, &token.token_id);
        Ok(())
    }

    /// Consume an mNFT token. Returns the collection_id (class_id) if consumed.
    pub fn consume_mnft_token(
        &self,
        token_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<Option<Vec<u8>>> {
        let mut state = self.new_mnft_batch_state();
        self.consume_mnft_token_with_state(token_id, block_number, tx_hash, batch, &mut state)
    }

    pub(crate) fn consume_mnft_token_with_state(
        &self,
        token_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
        state: &mut MnftBatchState,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(mut entry) = state.get_token(self.store.as_ref(), token_id)? {
            if !entry.is_live {
                bail!(
                    "mnft token already consumed: token_id=0x{}, block={}, tx=0x{}",
                    hex::encode(token_id),
                    block_number,
                    hex::encode(tx_hash)
                );
            }
            let collection_id = entry.collection_id.clone();
            let old_owner = entry.owner_lock_hash.clone();
            if old_owner.is_none() {
                bail!(
                    "mnft live token missing owner_lock_hash during consume: token_id=0x{}, block={}, tx=0x{}",
                    hex::encode(token_id),
                    block_number,
                    hex::encode(tx_hash)
                );
            }
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_object(token_id, &entry);
            state.put_token(token_id, entry);

            // Decrement collection's live_count
            if let Some(ref cid) = collection_id {
                let Some(mut agg) = state.get_collection_aggregate(self.store.as_ref(), cid)?
                else {
                    bail!(
                        "mnft collection aggregate missing: class_id=0x{}, token_id=0x{}, block={}, tx=0x{}",
                        hex::encode(cid),
                        hex::encode(token_id),
                        block_number,
                        hex::encode(tx_hash)
                    );
                };
                if agg.live_count <= 0 {
                    bail!(
                        "mnft collection live_count underflow: class_id=0x{}, live_count={}, block={}, tx=0x{}",
                        hex::encode(cid),
                        agg.live_count,
                        block_number,
                        hex::encode(tx_hash)
                    );
                }
                agg.live_count -= 1;
                self.apply_mnft_owner_transition(
                    cid,
                    old_owner.as_deref(),
                    None,
                    &mut agg,
                    batch,
                    state,
                )?;
                state.put_collection_aggregate(cid, agg, batch);
            } else {
                bail!(
                    "mnft token missing class_id: token_id=0x{}, block={}, tx=0x{}",
                    hex::encode(token_id),
                    block_number,
                    hex::encode(tx_hash)
                );
            }
            return Ok(collection_id);
        }
        Ok(None)
    }

    pub fn get_mnft_token_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        self.store
            .get_mnft_token_id_by_outpoint(tx_hash, output_index)
    }

    /// Batch lookup: find token_ids for multiple outpoints.
    pub fn get_mnft_token_ids_by_outpoints_batch(
        &self,
        tx_hashes: &[Vec<u8>],
        output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        let outpoints: Vec<(&[u8], i16)> = tx_hashes
            .iter()
            .zip(output_indices.iter())
            .map(|(hash, idx)| (hash.as_slice(), *idx))
            .collect();
        self.store.get_mnft_token_ids_by_outpoints_batch(&outpoints)
    }

    pub fn update_object_type_index_batch(
        &self,
        changes: &HashMap<Vec<u8>, ObjectTypeIndex>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for (type_script_hash, index) in changes {
            batch.put_object_type_index(type_script_hash, index);
        }
        Ok(())
    }

    pub fn update_object_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for ((collection_id, date), (capacity_delta, used_delta)) in changes {
            if *capacity_delta == 0 && *used_delta == 0 {
                continue;
            }
            let mut current = self
                .store
                .get_object_daily_delta(collection_id, *date)?
                .unwrap_or_default();
            current.live_capacity_delta = current
                .live_capacity_delta
                .checked_add(*capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "object daily capacity delta overflow: collection_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(collection_id),
                        date,
                        current.live_capacity_delta,
                        capacity_delta
                    )
                })?;
            current.live_used_capacity_delta = current
                .live_used_capacity_delta
                .checked_add(*used_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "object daily used delta overflow: collection_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(collection_id),
                        date,
                        current.live_used_capacity_delta,
                        used_delta
                    )
                })?;
            if current.live_capacity_delta == 0 && current.live_used_capacity_delta == 0 {
                let key = keys::encode_nft_daily_key(collection_id, *date);
                batch.delete_stats(&key);
            } else {
                batch.put_object_daily_delta(collection_id, *date, &current);
            }
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

    fn sample_class() -> ParsedMnftClass {
        ParsedMnftClass {
            class_id: vec![0x11; 24],
            type_script_hash: vec![0x21; 32],
            issuer_id: vec![0x31; 20],
            name: Some("Class".to_string()),
            description: None,
            renderer: None,
            total: 0,
            issued: 0,
            configure: 0,
            owner_lock_hash: vec![0x41; 32],
        }
    }

    fn sample_token(token_byte: u8, class_id: Vec<u8>, owner_byte: u8) -> ParsedMnftToken {
        ParsedMnftToken {
            token_id: vec![token_byte; 28],
            type_script_hash: vec![0x22; 32],
            class_id,
            token_index: u32::from(token_byte),
            characteristic: vec![],
            configure: 0,
            state: 0,
            owner_lock_hash: vec![owner_byte; 32],
        }
    }

    #[test]
    fn test_update_object_type_index_and_daily_deltas_batch_and_delete_zero_net() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_script_hash = vec![0x11; 32];
        let collection_id = vec![0x22; 24];
        let date = 20260219;

        let mut batch = StoreBatch::new(writer.store());
        let mut index_changes = HashMap::new();
        index_changes.insert(
            type_script_hash.clone(),
            ObjectTypeIndex {
                collection_id: collection_id.clone(),
            },
        );
        writer
            .update_object_type_index_batch(&index_changes, &mut batch)
            .unwrap();

        let mut daily_changes = HashMap::new();
        daily_changes.insert((collection_id.clone(), date), (100, 61));
        writer
            .update_object_daily_deltas_batch(&daily_changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let idx = writer
            .store()
            .get_object_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(idx.collection_id, collection_id);

        let daily = writer
            .store()
            .get_object_daily_delta(&[0x22; 24], date)
            .unwrap()
            .unwrap();
        assert_eq!(daily.live_capacity_delta, 100);
        assert_eq!(daily.live_used_capacity_delta, 61);

        let mut batch = StoreBatch::new(writer.store());
        let mut daily_changes = HashMap::new();
        daily_changes.insert((collection_id.clone(), date), (-100, -61));
        writer
            .update_object_daily_deltas_batch(&daily_changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let daily = writer
            .store()
            .get_object_daily_delta(&collection_id, date)
            .unwrap();
        assert!(daily.is_none());
    }

    #[test]
    fn test_mnft_outpoint_lookups_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token = sample_token(0x12, class.class_id.clone(), 0x42);
        let tx_hash = vec![0x51; 32];

        let mut batch = StoreBatch::new(writer.store());
        writer
            .insert_mnft_class(&class, &tx_hash, 7, 1, &mut batch)
            .unwrap();
        writer
            .insert_mnft_token(&token, &tx_hash, 8, 1, 0, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let loaded_class = writer
            .get_mnft_class_id_by_outpoint(&tx_hash, 7)
            .unwrap()
            .unwrap();
        let loaded_token = writer
            .get_mnft_token_id_by_outpoint(&tx_hash, 8)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_class, class.class_id);
        assert_eq!(loaded_token, token.token_id);

        let batch_loaded = writer
            .get_mnft_token_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[8])
            .unwrap();
        assert_eq!(batch_loaded.len(), 1);
        assert_eq!(batch_loaded[0].0, tx_hash);
        assert_eq!(batch_loaded[0].1, 8);

        let collection_ids = writer
            .store()
            .list_object_ids_by_collection(&class.class_id, None, 10)
            .unwrap();
        assert_eq!(collection_ids, vec![token.token_id]);
    }

    #[test]
    fn test_insert_mnft_tokens_with_state_accumulates_collection_counts_in_same_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token_a = sample_token(0x12, class.class_id.clone(), 0x42);
        let token_b = sample_token(0x13, class.class_id.clone(), 0x43);
        let tx_hash = vec![0x51; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_mnft_batch_state();
        writer
            .insert_mnft_class(&class, &tx_hash, 7, 1, &mut batch)
            .unwrap();
        writer
            .insert_mnft_token_with_state(&token_a, &tx_hash, 8, 1, 0, &mut batch, &mut state)
            .unwrap();
        writer
            .insert_mnft_token_with_state(&token_b, &tx_hash, 9, 1, 0, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        let agg = writer
            .store()
            .get_object_collection_aggregate(&class.class_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 2);
        assert_eq!(agg.holders_count, 2);
    }

    #[test]
    fn test_insert_mnft_token_with_state_accumulates_hourly_transfers_in_same_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token = sample_token(0x12, class.class_id.clone(), 0x42);
        let tx_hash = vec![0x51; 32];
        let hour_bucket = 3_600_000_i64 / 3_600_000;

        let mut seed = StoreBatch::new(writer.store());
        writer
            .insert_mnft_class(&class, &tx_hash, 7, 1, &mut seed)
            .unwrap();
        writer
            .insert_mnft_token(&token, &tx_hash, 8, 1, 0, &mut seed)
            .unwrap();
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_mnft_batch_state();
        let transfer_a = sample_token(0x12, class.class_id.clone(), 0x55);
        let transfer_b = sample_token(0x12, class.class_id.clone(), 0x66);
        writer
            .insert_mnft_token_with_state(
                &transfer_a,
                &tx_hash,
                8,
                2,
                3_600_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_mnft_token_with_state(
                &transfer_b,
                &tx_hash,
                8,
                3,
                3_600_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let key = ckbadger_store::keys::encode_nft_hourly_key(&class.class_id, hour_bucket);
        let value = writer.store().get_stats_key(&key).unwrap().unwrap();
        let transfer_count = i64::from_le_bytes(value[..8].try_into().unwrap());
        assert_eq!(transfer_count, 2);
    }

    #[test]
    fn test_insert_mnft_class_with_state_preserves_inflight_collection_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token_a = sample_token(0x12, class.class_id.clone(), 0x42);
        let token_b = sample_token(0x13, class.class_id.clone(), 0x43);
        let tx_hash = vec![0x51; 32];

        // Seed class with one token so the DB aggregate starts at 1.
        let mut seed = StoreBatch::new(writer.store());
        let mut seed_state = writer.new_mnft_batch_state();
        writer
            .insert_mnft_class_with_state(&class, &tx_hash, 7, 1, &mut seed, &mut seed_state)
            .unwrap();
        writer
            .insert_mnft_token_with_state(&token_a, &tx_hash, 8, 1, 0, &mut seed, &mut seed_state)
            .unwrap();
        seed.commit().unwrap();

        // In one uncommitted batch, add a new token then re-write class metadata.
        // Class upsert must not clobber collection counts already updated in this batch.
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_mnft_batch_state();
        writer
            .insert_mnft_token_with_state(&token_b, &tx_hash, 9, 2, 0, &mut batch, &mut state)
            .unwrap();
        writer
            .insert_mnft_class_with_state(&class, &tx_hash, 7, 2, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        let agg = writer
            .store()
            .get_object_collection_aggregate(&class.class_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 2);
        assert_eq!(agg.holders_count, 2);
    }

    #[test]
    fn test_get_hourly_transfer_errors_on_invalid_existing_value_length() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());
        let mut state = writer.new_mnft_batch_state();

        let collection_id = vec![0x88; 24];
        let key = ckbadger_store::keys::encode_nft_hourly_key(&collection_id, 1);
        let mut seed = StoreBatch::new(writer.store());
        seed.put_stats(&key, &[1, 2, 3, 4]);
        seed.commit().unwrap();

        let err = state.get_hourly_transfer(writer.store(), &key).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid mNFT hourly transfer value length"));
    }

    #[test]
    fn test_consume_mnft_tokens_with_state_decrements_live_count_in_same_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token_a = sample_token(0x12, class.class_id.clone(), 0x42);
        let token_b = sample_token(0x13, class.class_id.clone(), 0x43);
        let tx_hash = vec![0x51; 32];

        let mut seed = StoreBatch::new(writer.store());
        let mut seed_state = writer.new_mnft_batch_state();
        writer
            .insert_mnft_class(&class, &tx_hash, 7, 1, &mut seed)
            .unwrap();
        writer
            .insert_mnft_token_with_state(&token_a, &tx_hash, 8, 1, 0, &mut seed, &mut seed_state)
            .unwrap();
        writer
            .insert_mnft_token_with_state(&token_b, &tx_hash, 9, 1, 0, &mut seed, &mut seed_state)
            .unwrap();
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_mnft_batch_state();
        writer
            .consume_mnft_token_with_state(&token_a.token_id, 2, &tx_hash, &mut batch, &mut state)
            .unwrap();
        writer
            .consume_mnft_token_with_state(&token_b.token_id, 2, &tx_hash, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        let agg = writer
            .store()
            .get_object_collection_aggregate(&class.class_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);
    }

    #[test]
    fn test_consume_mnft_token_errors_on_live_count_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token = sample_token(0x12, class.class_id.clone(), 0x42);
        let tx_hash = vec![0x51; 32];

        let mut batch = StoreBatch::new(writer.store());
        writer
            .insert_mnft_class(&class, &tx_hash, 7, 1, &mut batch)
            .unwrap();
        writer
            .insert_mnft_token(&token, &tx_hash, 8, 1, 0, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut agg = writer
            .store()
            .get_object_collection_aggregate(&class.class_id)
            .unwrap()
            .unwrap();
        agg.live_count = 0;
        let mut batch = StoreBatch::new(writer.store());
        batch.put_object_collection_aggregate(&class.class_id, &agg);
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let err = writer
            .consume_mnft_token(&token.token_id, 2, &tx_hash, &mut batch)
            .unwrap_err();
        assert!(err.to_string().contains("live_count underflow"));
    }

    #[test]
    fn test_consume_mnft_token_errors_on_double_consume() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let class = sample_class();
        let token = sample_token(0x12, class.class_id.clone(), 0x42);
        let tx_hash = vec![0x51; 32];

        let mut batch = StoreBatch::new(writer.store());
        writer
            .insert_mnft_class(&class, &tx_hash, 7, 1, &mut batch)
            .unwrap();
        writer
            .insert_mnft_token(&token, &tx_hash, 8, 1, 0, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        writer
            .consume_mnft_token(&token.token_id, 2, &tx_hash, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let err = writer
            .consume_mnft_token(&token.token_id, 3, &tx_hash, &mut batch)
            .unwrap_err();
        assert!(err.to_string().contains("already consumed"));
    }
}
