use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

use crate::parser::{
    analyze_spore_media_profile, ParsedBitCell, ParsedClusterCell, ParsedDidCkbCell,
    ParsedSporeCell,
};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::store::{CF_IDENTITY_DATA, CF_SPORE_DATA};
use ckbadger_store::types::{
    ClusterAggregate, CompositionTier, IdentityCollectionAggregate, IdentityEntry, IdentityExtra,
    IdentityStandard, ObjectEntry, ObjectExtra, ObjectStandard, SporeTypeIndex,
};
use ckbadger_store::types::{
    BIT_CELL_SENTINEL_COLLECTION, DID_CKB_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;

#[cfg(test)]
use ckbadger_store::types::SporeMediaProfile;

use super::BatchWriter;

#[derive(Default)]
pub(crate) struct SporeBatchState {
    spores: HashMap<Vec<u8>, Option<ObjectEntry>>,
    identities: HashMap<Vec<u8>, Option<IdentityEntry>>,
    cluster_aggs: HashMap<Vec<u8>, ClusterAggregate>,
    cluster_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    identity_aggs: HashMap<Vec<u8>, IdentityCollectionAggregate>,
    identity_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    spore_hourly_transfers: HashMap<Vec<u8>, i64>,
    spore_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>>,
    undo_seq_by_block: HashMap<i64, u64>,
}

impl SporeBatchState {
    fn get_spore(&mut self, store: &CkbadgerStore, spore_id: &[u8]) -> Result<Option<ObjectEntry>> {
        if let Some(cached) = self.spores.get(spore_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_spore(spore_id)?;
        self.spores.insert(spore_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_spore(&mut self, spore_id: &[u8], entry: ObjectEntry) {
        self.spores.insert(spore_id.to_vec(), Some(entry));
    }

    fn get_identity(
        &mut self,
        store: &CkbadgerStore,
        identity_id: &[u8],
    ) -> Result<Option<IdentityEntry>> {
        if let Some(cached) = self.identities.get(identity_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_identity(identity_id)?;
        self.identities.insert(identity_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_identity(&mut self, identity_id: &[u8], entry: IdentityEntry) {
        self.identities.insert(identity_id.to_vec(), Some(entry));
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

    fn get_identity_agg(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
    ) -> Result<IdentityCollectionAggregate> {
        if let Some(cached) = self.identity_aggs.get(collection_id) {
            return Ok(cached.clone());
        }
        let loaded = store
            .get_identity_collection_aggregate(collection_id)?
            .unwrap_or_default();
        self.identity_aggs
            .insert(collection_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_identity_agg(
        &mut self,
        collection_id: &[u8],
        agg: IdentityCollectionAggregate,
        batch: &mut StoreBatch,
    ) {
        batch.put_identity_collection_aggregate(collection_id, &agg);
        self.identity_aggs.insert(collection_id.to_vec(), agg);
    }

    fn get_identity_owner_count(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
        lock_hash: &[u8],
    ) -> Result<i64> {
        let key = (collection_id.to_vec(), lock_hash.to_vec());
        if let Some(cached) = self.identity_owner_counts.get(&key) {
            return Ok(*cached);
        }
        let loaded = store.get_identity_owner_count(collection_id, lock_hash)?;
        self.identity_owner_counts.insert(key, loaded);
        Ok(loaded)
    }

    fn put_identity_owner_count(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        count: i64,
        batch: &mut StoreBatch,
    ) {
        batch.put_identity_owner_count(collection_id, lock_hash, count);
        self.identity_owner_counts
            .insert((collection_id.to_vec(), lock_hash.to_vec()), count);
    }

    fn delete_identity_owner(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        batch: &mut StoreBatch,
    ) {
        batch.delete_identity_owner(collection_id, lock_hash);
        self.identity_owner_counts
            .insert((collection_id.to_vec(), lock_hash.to_vec()), 0);
    }

    pub(crate) fn pending_identity_aggs(&self) -> &HashMap<Vec<u8>, IdentityCollectionAggregate> {
        &self.identity_aggs
    }

    pub(crate) fn extend_pending_cluster_ids(&self, target: &mut HashSet<Vec<u8>>) {
        target.extend(self.cluster_aggs.keys().cloned());
    }

    fn get_spore_hourly_transfer(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        if let Some(cached) = self.spore_hourly_transfers.get(key) {
            return Ok(*cached);
        }
        let loaded = match store.get_stats_key(key)? {
            Some(v) => {
                if v.len() != 8 {
                    bail!(
                        "invalid Spore hourly transfer value length in stats CF: key=0x{}, len={}",
                        hex::encode(key),
                        v.len()
                    );
                }
                i64::from_le_bytes(v[..8].try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "failed to decode Spore hourly transfer value as i64: key=0x{}",
                        hex::encode(key)
                    )
                })?)
            }
            None => 0,
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

    fn apply_identity_owner_transition(
        &self,
        collection_id: &[u8],
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut IdentityCollectionAggregate,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_lock) = old_owner {
            let old_count =
                state.get_identity_owner_count(self.store.as_ref(), collection_id, old_lock)?;
            if old_count <= 0 {
                bail!(
                    "identity owner count underflow: collection_id=0x{}, lock_hash=0x{}, owner_count={}",
                    hex::encode(collection_id),
                    hex::encode(old_lock),
                    old_count
                );
            } else if old_count == 1 {
                if agg.holders_count <= 0 {
                    bail!(
                        "identity aggregate holders_count underflow: collection_id=0x{}, holders_count={}",
                        hex::encode(collection_id),
                        agg.holders_count
                    );
                }
                state.delete_identity_owner(collection_id, old_lock, batch);
                agg.holders_count -= 1;
            } else {
                state.put_identity_owner_count(collection_id, old_lock, old_count - 1, batch);
            }
        }

        if let Some(new_lock) = new_owner {
            let cur_count =
                state.get_identity_owner_count(self.store.as_ref(), collection_id, new_lock)?;
            if cur_count == 0 {
                agg.holders_count = agg
                    .holders_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("identity aggregate holders_count overflow"))?;
            }
            let next = cur_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("identity owner count overflow"))?;
            state.put_identity_owner_count(collection_id, new_lock, next, batch);
        }

        Ok(())
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

    fn spore_media_tier(entry: &ObjectEntry) -> CompositionTier {
        match &entry.extra {
            ObjectExtra::Spore { media_profile, .. } => media_profile.tier,
            _ => CompositionTier::Unknown,
        }
    }

    fn adjust_cluster_tier_count(
        &self,
        cluster_id: &[u8],
        agg: &mut ClusterAggregate,
        tier: CompositionTier,
        delta: i64,
        context: &str,
    ) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let slot = match tier {
            CompositionTier::PureCkb => &mut agg.pure_ckb_count,
            CompositionTier::BtcCkb => &mut agg.btc_ckb_count,
            CompositionTier::DecentralizedMixture => &mut agg.decentralized_mixture_count,
            CompositionTier::CentralizedMixture => &mut agg.centralized_mixture_count,
            CompositionTier::Unknown => &mut agg.unknown_count,
        };
        let next = slot.checked_add(delta).ok_or_else(|| {
            anyhow::anyhow!(
                "cluster tier count overflow: cluster_id=0x{}, tier={}, current={}, delta={}, context={}",
                hex::encode(cluster_id),
                tier.as_str(),
                *slot,
                delta,
                context
            )
        })?;
        if next < 0 {
            bail!(
                "cluster tier count underflow: cluster_id=0x{}, tier={}, current={}, delta={}, context={}",
                hex::encode(cluster_id),
                tier.as_str(),
                *slot,
                delta,
                context
            );
        }
        *slot = next;
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
        self.record_object_undo(
            batch,
            block_number,
            CF_SPORE_DATA,
            &cluster.cluster_id,
            existing.as_ref().and_then(|e| bincode::serialize(e).ok()),
            &mut state.undo_seq_by_block,
        );
        let entry = ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None, // This IS a cluster, not a spore in a cluster
            token_id: None,
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
            extra: ObjectExtra::SporeCluster,
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

    /// Insert a did:ckb identity cell (live sync path).
    ///
    /// did:ckb entries are written to the identity store, not the spore/object
    /// store. The item id is the type-script args verbatim and is NOT
    /// fixed-width on chain (live testnet holds both 32-byte and 20-byte ids).
    pub(crate) fn insert_did_ckb_cell(
        &self,
        did: &ParsedDidCkbCell,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        // Ids are stored verbatim at their natural width; only widths outside
        // the indexable range are rejected. Checked before any write so a
        // rejected cell leaves no partial state. Same guard in bulk.
        super::ensure_outpoint_indexable_item_id(&did.did_id, "did:ckb", tx_hash, output_index)?;

        let existing = state.get_identity(self.store.as_ref(), &did.did_id)?;
        self.record_object_undo(
            batch,
            block_number,
            CF_IDENTITY_DATA,
            &did.did_id,
            existing.as_ref().and_then(|e| bincode::serialize(e).ok()),
            &mut state.undo_seq_by_block,
        );
        let was_live = existing.as_ref().is_some_and(|e| e.is_live);
        let old_owner = if was_live {
            existing.as_ref().and_then(|e| e.owner_lock_hash.clone())
        } else {
            None
        };
        let identity = IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(did.owner_lock_hash.clone()),
            name: None,
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: IdentityExtra::DidCkb,
        };
        batch.put_identity(&did.did_id, &identity);
        state.put_identity(&did.did_id, identity);
        batch.put_spore_outpoint(tx_hash, output_index, &did.did_id);
        state.put_spore_outpoint(tx_hash, output_index, &did.did_id);

        // Update identity collection aggregate
        let cid = &DID_CKB_SENTINEL_COLLECTION;
        let mut agg = state.get_identity_agg(self.store.as_ref(), cid)?;
        if agg.standard == IdentityStandard::default() && agg.total_count == 0 {
            agg.standard = IdentityStandard::DidCkb;
            agg.name = Some("did:ckb".to_string());
        }
        if existing.is_none() {
            // New identity — add to identity collection index
            batch.put_identity_by_collection(cid, &did.did_id);
            agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "did:ckb identity total_count overflow: did_id=0x{}",
                    hex::encode(&did.did_id)
                )
            })?;
            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "did:ckb identity live_count overflow: did_id=0x{}",
                    hex::encode(&did.did_id)
                )
            })?;
        } else if !was_live {
            // Re-activate consumed identity
            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "did:ckb identity live_count overflow on reactivate: did_id=0x{}",
                    hex::encode(&did.did_id)
                )
            })?;
        }
        let owner_from = if was_live { old_owner.as_deref() } else { None };
        self.apply_identity_owner_transition(
            cid,
            owner_from,
            Some(did.owner_lock_hash.as_slice()),
            &mut agg,
            batch,
            state,
        )?;
        state.put_identity_agg(cid, agg, batch);
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
        // The cluster_id comes out of the spore cell's molecule `Bytes` data, which
        // carries no width guarantee of its own. Bulk sync validates it via
        // `parse_optional_fixed_protocol_id::<32>`; the live path did not, so the
        // two modes disagreed on the same cell — and once it reaches a key encoder
        // a wrong width either panics without context or truncates into another
        // cluster's key range. Same check, same context, both modes.
        if let Some(cluster_id) = spore.cluster_id.as_deref() {
            if cluster_id.len() != 32 {
                bail!(
                    "invalid spore cluster_id length: tx=0x{}, output_index={}, spore_id=0x{}, expected=32, actual={}",
                    hex::encode(tx_hash),
                    output_index,
                    hex::encode(&spore.spore_id),
                    cluster_id.len()
                );
            }
        }

        let existing = state.get_spore(self.store.as_ref(), &spore.spore_id)?;
        self.record_object_undo(
            batch,
            block_number,
            CF_SPORE_DATA,
            &spore.spore_id,
            existing.as_ref().and_then(|e| bincode::serialize(e).ok()),
            &mut state.undo_seq_by_block,
        );
        let was_live = existing.as_ref().is_some_and(|e| e.is_live);
        let old_live_tier = if was_live {
            existing
                .as_ref()
                .map(Self::spore_media_tier)
                .unwrap_or(CompositionTier::Unknown)
        } else {
            CompositionTier::Unknown
        };
        let old_cluster = existing.as_ref().and_then(|e| e.collection_id.clone());
        let old_owner = if was_live {
            existing.as_ref().and_then(|e| e.owner_lock_hash.clone())
        } else {
            None
        };
        let new_cluster = spore.cluster_id.clone();
        let effective_cluster = new_cluster
            .clone()
            .or_else(|| Some(SOLE_SPORES_SENTINEL_COLLECTION.to_vec()));
        let media_profile = if let Some(precomputed) = &spore.media_profile {
            precomputed.clone()
        } else {
            let cluster_description = if let Some(cluster_id) = new_cluster.as_ref() {
                state
                    .get_spore(self.store.as_ref(), cluster_id)?
                    .and_then(|entry| {
                        if entry.standard == ObjectStandard::SporeCluster {
                            entry.description
                        } else {
                            None
                        }
                    })
            } else {
                None
            };
            analyze_spore_media_profile(
                &spore.content_type,
                &spore.content,
                cluster_description.as_deref(),
                false,
            )
        };
        let new_live_tier = media_profile.tier;
        let entry = ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: effective_cluster.clone(),
            token_id: None,
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
            extra: ObjectExtra::Spore {
                content_type: spore.content_type.clone(),
                content_length: spore.content.len() as i64,
                media_profile,
            },
        };
        batch.put_spore(&spore.spore_id, &entry);
        state.put_spore(&spore.spore_id, entry);
        batch.put_spore_outpoint(tx_hash, output_index, &spore.spore_id);
        state.put_spore_outpoint(tx_hash, output_index, &spore.spore_id);

        if old_cluster != effective_cluster {
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
                    self.adjust_cluster_tier_count(
                        old_cluster_id,
                        &mut old_agg,
                        old_live_tier,
                        -1,
                        "cluster move old cluster",
                    )?;
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
        if let Some(ref cluster_id) = effective_cluster {
            if !(was_live && old_cluster.as_ref() == Some(cluster_id)) {
                batch.put_spore_by_cluster(cluster_id, &spore.spore_id);
            }

            // Update cluster aggregate
            let mut agg = state.get_cluster_aggregate(self.store.as_ref(), cluster_id)?;

            if existing.is_none() {
                // New spore: increment counts
                agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster total_count overflow while inserting spore: cluster_id=0x{}, spore_id=0x{}, current={}",
                        hex::encode(cluster_id),
                        hex::encode(&spore.spore_id),
                        agg.total_count
                    )
                })?;
                agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster live_count overflow while inserting spore: cluster_id=0x{}, spore_id=0x{}, current={}",
                        hex::encode(cluster_id),
                        hex::encode(&spore.spore_id),
                        agg.live_count
                    )
                })?;
                self.adjust_cluster_tier_count(
                    cluster_id,
                    &mut agg,
                    new_live_tier,
                    1,
                    "insert new spore",
                )?;
            } else if !was_live || old_cluster.as_ref() != Some(cluster_id) {
                // Re-activate consumed spore or move a live spore to another cluster.
                agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster live_count overflow while reactivating/moving spore: cluster_id=0x{}, spore_id=0x{}, current={}",
                        hex::encode(cluster_id),
                        hex::encode(&spore.spore_id),
                        agg.live_count
                    )
                })?;
                self.adjust_cluster_tier_count(
                    cluster_id,
                    &mut agg,
                    new_live_tier,
                    1,
                    "reactivate or move spore",
                )?;
            } else {
                // Re-insert (transfer) — increment hourly bucket for 24h tracking
                let hour_bucket = timestamp_ms / 3_600_000;
                let key = ckbadger_store::keys::encode_spore_hourly_key(cluster_id, hour_bucket);
                let current = state.get_spore_hourly_transfer(self.store.as_ref(), &key)?;
                let next = current.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "spore hourly transfer overflow: cluster_id=0x{}, hour_bucket={}, current={}, spore_id=0x{}",
                        hex::encode(cluster_id),
                        hour_bucket,
                        current,
                        hex::encode(&spore.spore_id)
                    )
                })?;
                batch.put_spore_hourly_transfer(cluster_id, hour_bucket, next);
                state.put_spore_hourly_transfer(key, next);
                if old_live_tier != new_live_tier {
                    self.adjust_cluster_tier_count(
                        cluster_id,
                        &mut agg,
                        old_live_tier,
                        -1,
                        "in-place spore media tier update",
                    )?;
                    self.adjust_cluster_tier_count(
                        cluster_id,
                        &mut agg,
                        new_live_tier,
                        1,
                        "in-place spore media tier update",
                    )?;
                }
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

    pub(crate) fn insert_bit_cell(
        &self,
        bit_cell: &ParsedBitCell,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        let identity_id = bit_cell.identity_id.as_slice();
        let existing = state.get_identity(self.store.as_ref(), identity_id)?;
        if let Some(existing) = existing.as_ref() {
            if existing.standard != IdentityStandard::BitCell {
                bail!(
                    ".bit Cell identity ID collides with another identity standard: identity_id=0x{}, existing_standard={}",
                    hex::encode(identity_id),
                    existing.standard.as_str()
                );
            }
        }

        self.record_object_undo(
            batch,
            block_number,
            CF_IDENTITY_DATA,
            identity_id,
            existing
                .as_ref()
                .and_then(|entry| bincode::serialize(entry).ok()),
            &mut state.undo_seq_by_block,
        );
        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };
        let identity = IdentityEntry {
            standard: IdentityStandard::BitCell,
            owner_lock_hash: Some(bit_cell.owner_lock_hash.clone()),
            name: Some(bit_cell.account.clone()),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|entry| entry.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|entry| entry.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: IdentityExtra::BitCell {
                account_id: bit_cell.account_id.clone(),
                expired_at: bit_cell.expired_at,
            },
        };
        batch.put_identity(identity_id, &identity);
        state.put_identity(identity_id, identity);
        batch.put_spore_outpoint(tx_hash, output_index, identity_id);
        state.put_spore_outpoint(tx_hash, output_index, identity_id);

        let collection_id = &BIT_CELL_SENTINEL_COLLECTION;
        let mut aggregate = state.get_identity_agg(self.store.as_ref(), collection_id)?;
        if aggregate.standard == IdentityStandard::default() && aggregate.total_count == 0 {
            aggregate.standard = IdentityStandard::BitCell;
            aggregate.name = Some(".bit Cell".to_string());
        }
        if existing.is_none() {
            batch.put_identity_by_collection(collection_id, identity_id);
            aggregate.total_count = aggregate.total_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    ".bit Cell identity total_count overflow: identity_id=0x{}",
                    hex::encode(identity_id)
                )
            })?;
            aggregate.live_count = aggregate.live_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    ".bit Cell identity live_count overflow: identity_id=0x{}",
                    hex::encode(identity_id)
                )
            })?;
        } else if !was_live {
            aggregate.live_count = aggregate.live_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    ".bit Cell identity live_count overflow on reactivate: identity_id=0x{}",
                    hex::encode(identity_id)
                )
            })?;
        }
        self.apply_identity_owner_transition(
            collection_id,
            if was_live { old_owner.as_deref() } else { None },
            Some(bit_cell.owner_lock_hash.as_slice()),
            &mut aggregate,
            batch,
            state,
        )?;
        state.put_identity_agg(collection_id, aggregate, batch);
        Ok(())
    }

    /// Consume a spore/object or did:ckb identity.
    ///
    /// For did:ckb entries: marks the identity as consumed in the identity store.
    /// Returns `None` (identities have no collection hierarchy).
    ///
    /// For regular spores: returns the `collection_id` (cluster_id or sentinel)
    /// if consumed, or `None` if entry not found.
    /// Bails on double-consume (identity or spore already consumed).
    pub(crate) fn consume_spore(
        &self,
        spore_id: &[u8],
        block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<Option<Vec<u8>>> {
        // Check identity store first (`.bit Cell` or future did:ckb).
        if let Some(mut identity) = state.get_identity(self.store.as_ref(), spore_id)? {
            if !identity.is_live {
                bail!(
                    "consume_spore: identity already consumed: spore_id=0x{}",
                    hex::encode(spore_id)
                );
            }
            self.record_object_undo(
                batch,
                block_number,
                CF_IDENTITY_DATA,
                spore_id,
                bincode::serialize(&identity).ok(),
                &mut state.undo_seq_by_block,
            );
            let old_owner = identity.owner_lock_hash.clone();
            let identity_standard = identity.standard;
            identity.is_live = false;
            identity.owner_lock_hash = None;
            batch.put_identity(spore_id, &identity);
            state.put_identity(spore_id, identity);

            // Update the aggregate belonging to the stored identity standard.
            let cid = identity_standard.sentinel_collection_id();
            let mut agg = state.get_identity_agg(self.store.as_ref(), cid)?;
            if agg.live_count <= 0 {
                bail!(
                    "identity live_count underflow on consume: standard={}, identity_id=0x{}, live_count={}",
                    identity_standard.as_str(),
                    hex::encode(spore_id),
                    agg.live_count
                );
            }
            agg.live_count -= 1;
            self.apply_identity_owner_transition(
                cid,
                old_owner.as_deref(),
                None,
                &mut agg,
                batch,
                state,
            )?;
            state.put_identity_agg(cid, agg, batch);
            return Ok(None);
        }

        // Check object store (regular spore)
        if let Some(mut entry) = state.get_spore(self.store.as_ref(), spore_id)? {
            if !entry.is_live {
                bail!(
                    "consume_spore: spore already consumed: spore_id=0x{}",
                    hex::encode(spore_id)
                );
            }
            self.record_object_undo(
                batch,
                block_number,
                CF_SPORE_DATA,
                spore_id,
                bincode::serialize(&entry).ok(),
                &mut state.undo_seq_by_block,
            );

            let old_owner = entry.owner_lock_hash.clone();
            let cluster_id = entry.collection_id.clone();
            let old_tier = Self::spore_media_tier(&entry);

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
                self.adjust_cluster_tier_count(cid, &mut agg, old_tier, -1, "consume spore")?;
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
            return Ok(cluster_id);
        }
        Ok(None)
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
        for ((spore_id, date), (capacity_delta, used_delta)) in changes {
            if *capacity_delta == 0 && *used_delta == 0 {
                continue;
            }
            let mut current = self
                .store
                .get_spore_daily_delta(spore_id, *date)?
                .unwrap_or_default();
            current.owned_capacity_delta = current
                .owned_capacity_delta
                .checked_add(*capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "spore daily capacity delta overflow: spore_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(spore_id),
                        date,
                        current.owned_capacity_delta,
                        capacity_delta
                    )
                })?;
            current.owned_knowledge_delta = current
                .owned_knowledge_delta
                .checked_add(*used_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "spore daily used delta overflow: spore_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(spore_id),
                        date,
                        current.owned_knowledge_delta,
                        used_delta
                    )
                })?;
            if current.owned_capacity_delta == 0 && current.owned_knowledge_delta == 0 {
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
        for ((cluster_id, date), (capacity_delta, used_delta)) in changes {
            if *capacity_delta == 0 && *used_delta == 0 {
                continue;
            }
            let mut current = self
                .store
                .get_cluster_daily_delta(cluster_id, *date)?
                .unwrap_or_default();
            current.owned_capacity_delta = current
                .owned_capacity_delta
                .checked_add(*capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster daily capacity delta overflow: cluster_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(cluster_id),
                        date,
                        current.owned_capacity_delta,
                        capacity_delta
                    )
                })?;
            current.owned_knowledge_delta = current
                .owned_knowledge_delta
                .checked_add(*used_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cluster daily used delta overflow: cluster_id=0x{}, date={}, current={}, delta={}",
                        hex::encode(cluster_id),
                        date,
                        current.owned_knowledge_delta,
                        used_delta
                    )
                })?;
            if current.owned_capacity_delta == 0 && current.owned_knowledge_delta == 0 {
                let key = keys::encode_cluster_daily_key(cluster_id, *date);
                batch.delete_stats(&key);
            } else {
                batch.put_cluster_daily_delta(cluster_id, *date, &current);
            }
        }
        Ok(())
    }

    /// Apply cumulative capacity deltas from cluster_daily_changes to cluster aggregates.
    /// Called after spore insert/consume so the aggregate cache is warm.
    pub(crate) fn apply_cluster_capacity_deltas(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        let mut per_cluster: HashMap<Vec<u8>, (i128, i128)> = HashMap::new();
        for ((cluster_id, _date), (cap, know)) in changes {
            let e = per_cluster.entry(cluster_id.clone()).or_insert((0, 0));
            e.0 += cap;
            e.1 += know;
        }
        for (cluster_id, (cap_delta, know_delta)) in &per_cluster {
            let mut agg = state.get_cluster_aggregate(self.store.as_ref(), cluster_id)?;
            agg.owned_capacity += cap_delta;
            agg.owned_knowledge += know_delta;
            state.put_cluster_aggregate(cluster_id, agg, batch);
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
        self.store.get_spore_ids_by_outpoints_batch(&outpoints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::BatchWriter;
    use ckbadger_store::store::CkbadgerStore;
    use std::sync::Arc;

    fn make_spore_entry(cluster_id: &[u8], owner_lock: &[u8]) -> ObjectEntry {
        ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(cluster_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(owner_lock.to_vec()),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0xAA; 32],
            extra: ObjectExtra::Spore {
                content_type: "image/png".to_string(),
                content_length: 4,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::PureCkb,
                    sources: Vec::new(),
                    issues: Vec::new(),
                },
            },
        }
    }

    fn make_parsed_spore(spore_id: &[u8], cluster_id: &[u8], owner_lock: &[u8]) -> ParsedSporeCell {
        ParsedSporeCell {
            spore_id: spore_id.to_vec(),
            type_script_hash: vec![0x99; 32],
            content_type: "image/png".to_string(),
            content: vec![0x89, 0x50, 0x4e, 0x47],
            cluster_id: Some(cluster_id.to_vec()),
            owner_lock_hash: owner_lock.to_vec(),
            media_profile: None,
        }
    }

    fn make_parsed_spore_no_cluster(spore_id: &[u8], owner_lock: &[u8]) -> ParsedSporeCell {
        ParsedSporeCell {
            spore_id: spore_id.to_vec(),
            type_script_hash: vec![0x99; 32],
            content_type: "image/png".to_string(),
            content: vec![0x89, 0x50, 0x4e, 0x47],
            cluster_id: None,
            owner_lock_hash: owner_lock.to_vec(),
            media_profile: None,
        }
    }

    fn make_parsed_did(did_id: &[u8], owner_lock: &[u8]) -> ParsedDidCkbCell {
        ParsedDidCkbCell {
            did_id: did_id.to_vec(),
            owner_lock_hash: owner_lock.to_vec(),
        }
    }

    #[test]
    fn test_update_spore_daily_and_cluster_daily_deltas_batch_accumulates_and_deletes_zero_net() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

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
        assert_eq!(spore.owned_capacity_delta, 80);
        assert_eq!(spore.owned_knowledge_delta, 50);

        let cluster = writer
            .store()
            .get_cluster_daily_delta(&cluster_id, date)
            .unwrap()
            .unwrap();
        assert_eq!(cluster.owned_capacity_delta, 800);
        assert_eq!(cluster.owned_knowledge_delta, 500);

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
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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
                    pure_ckb_count: 1,
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
        assert_eq!(agg.pure_ckb_count, 1);
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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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
                    pure_ckb_count: 1,
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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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
                    pure_ckb_count: 1,
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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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
                    pure_ckb_count: 1,
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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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
                    pure_ckb_count: 1,
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
        assert_eq!(old_agg.pure_ckb_count, 0);
        assert_eq!(new_agg.live_count, 1);
        assert_eq!(new_agg.owner_count, 1);
        assert_eq!(new_agg.pure_ckb_count, 1);
    }

    /// A spore's `cluster_id` is a molecule `Bytes` field with no intrinsic
    /// width guarantee. Bulk sync rejects a wrong width with tx/output context
    /// (`parse_optional_fixed_protocol_id::<32>`); the live path used to pass it
    /// straight to the key encoders, where it either truncated into another
    /// cluster's range or panicked without saying which cell caused it.
    #[test]
    fn test_insert_spore_cell_rejects_cluster_id_of_the_wrong_width() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let spore_id = vec![0x91; 32];
        let owner = vec![0xA1; 32];
        let tx_hash = vec![0xB1; 32];

        for bad_width in [16usize, 31, 33, 64] {
            let cluster_id = vec![0x81; bad_width];
            let mut batch = StoreBatch::new(writer.store());
            let mut state = writer.new_spore_batch_state();
            let err = writer
                .insert_spore_cell(
                    &make_parsed_spore(&spore_id, &cluster_id, &owner),
                    &tx_hash,
                    3,
                    20,
                    14_400_000,
                    &mut batch,
                    &mut state,
                )
                .expect_err(&format!(
                    "a {bad_width}-byte cluster_id must be rejected, not written"
                ));
            let msg = err.to_string();
            assert!(msg.contains("cluster_id"), "{msg}");
            assert!(msg.contains(&format!("actual={bad_width}")), "{msg}");
            // Locating the offending cell must not require a debugger.
            assert!(msg.contains(&hex::encode(&tx_hash)), "{msg}");
            assert!(msg.contains("output_index=3"), "{msg}");
        }
    }

    #[test]
    fn test_insert_spore_cell_populates_outpoint_lookup_and_batch_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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

    #[test]
    fn test_insert_did_ckb_cell_writes_identity_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let did_id = vec![0xD1; 32];
        let owner = vec![0xE1; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&did_id, &owner),
                &[0x10; 32],
                0,
                200,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        // Verify identity was written to identity store
        let identity = store
            .get_identity(&did_id)
            .unwrap()
            .expect("identity exists");
        assert_eq!(identity.standard, IdentityStandard::DidCkb);
        assert!(identity.is_live);
        assert_eq!(identity.owner_lock_hash, Some(owner));
        assert!(matches!(identity.extra, IdentityExtra::DidCkb));
    }

    #[test]
    fn test_real_testnet_did_ckb_cell_roundtrips_identity_store() {
        use crate::parser::test_helpers::real_did_ckb;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        // Real audited testnet cell 0x00290adc…:0 (block 18082860).
        let (output, _data_hex) = real_did_ckb::cell_32();
        let parsed = crate::parser::did_ckb::DidCkbParser::parse_did_cell(&output)
            .expect("real did:ckb output must be classified for the identity write path");
        let tx_hash = crate::rpc::parse_hex_to_bytes(real_did_ckb::CELL_32_TX_HASH);
        let item_id = crate::rpc::parse_hex_to_bytes(real_did_ckb::CELL_32_ARGS);

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(&parsed, &tx_hash, 0, 18_082_860, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        // Read back exactly the way the API detail route does (get_identity by
        // the args item id).
        let identity = store
            .get_identity(&item_id)
            .unwrap()
            .expect("identity entry must exist under the args item id");
        assert_eq!(identity.standard, IdentityStandard::DidCkb);
        assert!(identity.is_live);
        assert!(matches!(identity.extra, IdentityExtra::DidCkb));
        let expected_owner = crate::parser::script::ScriptParser::compute_script_hash(&output.lock);
        assert_eq!(identity.owner_lock_hash, Some(expected_owner));
        assert_eq!(identity.created_at_block, 18_082_860);
        assert_eq!(identity.created_at_tx, tx_hash);

        // Collection aggregate (drives /assets?type=identity).
        let agg = store
            .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
            .unwrap()
            .expect("did:ckb aggregate");
        assert_eq!(agg.standard, IdentityStandard::DidCkb);
        assert_eq!(agg.name.as_deref(), Some("did:ckb"));
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 1);
        assert_eq!(agg.holders_count, 1);

        // Items listing index (drives /assets/identities/did_ckb/items).
        let ids = store
            .list_identity_ids_by_collection(&DID_CKB_SENTINEL_COLLECTION, None, 10)
            .unwrap();
        assert_eq!(ids, vec![item_id.clone()]);

        // Outpoint → id mapping used by the live consume path.
        let mapped = writer
            .get_spore_id_by_outpoint(&tx_hash, 0)
            .unwrap()
            .expect("outpoint mapping");
        assert_eq!(mapped, item_id);
    }

    /// Real live-testnet did:ckb cells carry 20-byte type-script args (31 of
    /// 421 as of the 2026-08-01 audit). Ids are stored verbatim at their
    /// natural width, so those cells index end-to-end — including the outpoint
    /// reverse index that backs the item's `/activities` route.
    #[test]
    fn test_real_testnet_did_ckb_20_byte_item_id_roundtrips_identity_store() {
        use crate::parser::test_helpers::real_did_ckb;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        // Real testnet cell 0x1d43c10b…:0 with a 20-byte args item id.
        let (output, _data_hex) = real_did_ckb::cell_20();
        let parsed = crate::parser::did_ckb::DidCkbParser::parse_did_cell(&output)
            .expect("real 20-byte-args did:ckb output must be classified");
        let tx_hash = crate::rpc::parse_hex_to_bytes(real_did_ckb::CELL_20_TX_HASH);
        let item_id = crate::rpc::parse_hex_to_bytes(real_did_ckb::CELL_20_ARGS);
        assert_eq!(item_id.len(), 20);

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(&parsed, &tx_hash, 0, 21_080_336, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        let identity = store
            .get_identity(&item_id)
            .unwrap()
            .expect("20-byte item id must be persisted verbatim");
        assert_eq!(identity.standard, IdentityStandard::DidCkb);
        assert!(identity.is_live);
        assert_eq!(identity.created_at_block, 21_080_336);

        let ids = store
            .list_identity_ids_by_collection(&DID_CKB_SENTINEL_COLLECTION, None, 10)
            .unwrap();
        assert_eq!(ids, vec![item_id.clone()]);

        // Forward outpoint map (live consume resolves the item through this)
        // returns the 20-byte id verbatim, not a truncated/absent value.
        assert_eq!(
            writer.get_spore_id_by_outpoint(&tx_hash, 0).unwrap(),
            Some(item_id.clone())
        );
        // Reverse index backs `/assets/identities/did/items/{id}/activities`.
        assert_eq!(
            store.list_spore_outpoints_by_spore_id(&item_id).unwrap(),
            vec![(tx_hash.clone(), 0)]
        );
    }

    /// The width guard now rejects only what genuinely cannot be indexed: an
    /// id outside 1..=32 bytes (empty would collapse identities onto one key;
    /// longer than 32 could never be queried through the API's item-id cap).
    #[test]
    fn test_did_ckb_item_id_outside_indexable_width_fails_fast_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let tx_hash = vec![0x7A; 32];

        for bad_id in [Vec::new(), vec![0x01; 33]] {
            let mut batch = StoreBatch::new(writer.store());
            let mut state = writer.new_spore_batch_state();
            let err = writer
                .insert_did_ckb_cell(
                    &make_parsed_did(&bad_id, &[0xE1; 32]),
                    &tx_hash,
                    0,
                    500,
                    &mut batch,
                    &mut state,
                )
                .expect_err("unindexable item id width must fail fast");
            let message = err.to_string();
            assert!(
                message.contains("item id width is not indexable"),
                "error must name the constraint: {message}"
            );
            assert!(
                message.contains(&hex::encode(&tx_hash)),
                "error must locate the cell: {message}"
            );
            batch.commit().unwrap();

            // Rejected before any write: no partial identity state.
            assert!(store.get_identity(&bad_id).unwrap().is_none());
            assert!(store
                .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn test_get_spore_hourly_transfer_errors_on_invalid_existing_value_length() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let mut state = writer.new_spore_batch_state();

        let cluster_id = vec![0xCC; 32];
        let key = ckbadger_store::keys::encode_spore_hourly_key(&cluster_id, 5);
        let mut seed = StoreBatch::new(writer.store());
        seed.put_stats(&key, &[1, 2, 3, 4, 5]);
        seed.commit().unwrap();

        let err = state
            .get_spore_hourly_transfer(writer.store(), &key)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid Spore hourly transfer value length"));
    }

    #[test]
    fn test_consume_did_ckb_cell_marks_identity_consumed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let did_id = vec![0xF1; 32];
        let owner = vec![0xA2; 32];

        {
            let mut batch = StoreBatch::new(writer.store());
            let mut state = writer.new_spore_batch_state();
            writer
                .insert_did_ckb_cell(
                    &make_parsed_did(&did_id, &owner),
                    &[0x22; 32],
                    0,
                    300,
                    &mut batch,
                    &mut state,
                )
                .unwrap();
            batch.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let result = writer
            .consume_spore(&did_id, 301, &[0x23; 32], &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        // Identities return None (no collection hierarchy)
        assert!(result.is_none());

        let identity = store
            .get_identity(&did_id)
            .unwrap()
            .expect("identity exists");
        assert!(!identity.is_live);
        assert!(identity.owner_lock_hash.is_none());
    }

    #[test]
    fn test_insert_spore_cell_errors_on_cluster_total_count_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let cluster_id = vec![0xB1; 32];
        let spore_id = vec![0xB2; 32];
        let owner = vec![0xB3; 32];

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_cluster_aggregate(
                &cluster_id,
                &ClusterAggregate {
                    total_count: i64::MAX,
                    live_count: 0,
                    ..Default::default()
                },
            );
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let err = writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &cluster_id, &owner),
                &[0x31; 32],
                0,
                400,
                14_400_000,
                &mut batch,
                &mut state,
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("cluster total_count overflow"),
            "expected overflow error, got: {}",
            err
        );
    }

    #[test]
    fn test_insert_spore_cell_errors_on_spore_hourly_transfer_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let cluster_id = vec![0xC1; 32];
        let spore_id = vec![0xC2; 32];
        let owner = vec![0xC3; 32];
        let hour_bucket = 8i64;
        let timestamp_ms = hour_bucket * 3_600_000;

        {
            let mut seed = StoreBatch::new(&store);
            seed.put_spore(&spore_id, &make_spore_entry(&cluster_id, &owner));
            seed.put_cluster_aggregate(
                &cluster_id,
                &ClusterAggregate {
                    total_count: 1,
                    live_count: 1,
                    owner_count: 1,
                    pure_ckb_count: 1,
                    ..Default::default()
                },
            );
            seed.put_cluster_owner_count(&cluster_id, &owner, 1);
            seed.put_spore_hourly_transfer(&cluster_id, hour_bucket, i64::MAX);
            seed.commit().unwrap();
        }

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let err = writer
            .insert_spore_cell(
                &make_parsed_spore(&spore_id, &cluster_id, &owner),
                &[0x41; 32],
                0,
                500,
                timestamp_ms,
                &mut batch,
                &mut state,
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("spore hourly transfer overflow"),
            "expected hourly overflow error, got: {}",
            err
        );
    }

    #[test]
    fn test_insert_did_ckb_updates_identity_collection_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let spore_id_a = vec![0x01; 32];
        let spore_id_b = vec![0x02; 32];
        let owner_a = vec![0xA1; 32];
        let owner_b = vec![0xB2; 32];
        let tx_hash_a = vec![0xF1; 32];
        let tx_hash_b = vec![0xF2; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&spore_id_a, &owner_a),
                &tx_hash_a,
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&spore_id_b, &owner_b),
                &tx_hash_b,
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
            .unwrap()
            .expect("identity aggregate should exist");
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 2);
        assert_eq!(agg.holders_count, 2);
        assert_eq!(agg.standard, IdentityStandard::DidCkb);
    }

    #[test]
    fn test_consume_did_ckb_decrements_identity_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let spore_id = vec![0x01; 32];
        let owner = vec![0xA1; 32];
        let tx_hash = vec![0xF1; 32];

        // Insert
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&spore_id, &owner),
                &tx_hash,
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        // Consume
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        let result = writer
            .consume_spore(&spore_id, 200, &[0xFF; 32], &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        // consume_spore returns None for identities (no collection hierarchy)
        assert!(result.is_none());

        let agg = store
            .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
            .unwrap()
            .expect("identity aggregate should exist");
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);
    }

    #[test]
    fn test_did_ckb_same_owner_two_identities_holders_count_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let owner = vec![0xA1; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&[0x01; 32], &owner),
                &[0xF1; 32],
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&[0x02; 32], &owner),
                &[0xF2; 32],
                1,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 2);
        assert_eq!(agg.holders_count, 1, "same owner should count as 1 holder");
    }

    #[test]
    fn test_reactivate_did_ckb_increments_live_count_not_total() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let spore_id = vec![0x01; 32];
        let owner = vec![0xA1; 32];

        // Insert
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&spore_id, &owner),
                &[0xF1; 32],
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        // Consume
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .consume_spore(&spore_id, 200, &[0xFF; 32], &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        // Reactivate
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .insert_did_ckb_cell(
                &make_parsed_did(&spore_id, &owner),
                &[0xF3; 32],
                0,
                300,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(
            agg.total_count, 1,
            "reactivation should not increment total"
        );
        assert_eq!(agg.live_count, 1);
        assert_eq!(agg.holders_count, 1);
    }

    #[test]
    fn test_insert_clusterless_spore_uses_sole_spores_sentinel() {
        use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();

        let spore_id = [0x11u8; 32];
        let owner = [0x22u8; 32];
        let tx_hash = [0x33u8; 32];

        let spore = make_parsed_spore_no_cluster(&spore_id, &owner);
        writer
            .insert_spore_cell(&spore, &tx_hash, 0, 100, 100_000, &mut batch, &mut state)
            .unwrap();

        // Verify the spore was stored with the sentinel collection_id
        let cached = state
            .spores
            .get(spore_id.as_slice())
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(
            cached.collection_id.as_deref(),
            Some(SOLE_SPORES_SENTINEL_COLLECTION.as_slice()),
            "clusterless spore must get SOLE_SPORES_SENTINEL_COLLECTION"
        );

        // Verify cluster aggregate was updated
        let agg = state
            .cluster_aggs
            .get(SOLE_SPORES_SENTINEL_COLLECTION.as_slice())
            .expect("sentinel aggregate must exist");
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 1);
        assert_eq!(agg.owner_count, 1);
    }

    #[test]
    fn test_consume_clusterless_spore_returns_sentinel_and_decrements() {
        use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();

        let spore_id = [0x11u8; 32];
        let owner = [0x22u8; 32];
        let tx_hash = [0x33u8; 32];

        // Insert a clusterless spore
        let spore = make_parsed_spore_no_cluster(&spore_id, &owner);
        writer
            .insert_spore_cell(&spore, &tx_hash, 0, 100, 100_000, &mut batch, &mut state)
            .unwrap();

        // Consume it
        let result = writer
            .consume_spore(&spore_id, 101, &[0x44u8; 32], &mut batch, &mut state)
            .unwrap();

        // Should return the sentinel collection_id
        assert_eq!(
            result.as_deref(),
            Some(SOLE_SPORES_SENTINEL_COLLECTION.as_slice()),
            "consuming a clusterless spore must return SOLE_SPORES_SENTINEL_COLLECTION"
        );

        // Aggregate: total_count stays 1, live_count decremented to 0
        let agg = state
            .cluster_aggs
            .get(SOLE_SPORES_SENTINEL_COLLECTION.as_slice())
            .expect("sentinel aggregate must exist");
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.owner_count, 0);
    }

    #[test]
    fn test_apply_cluster_capacity_deltas_updates_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let cluster_a = vec![0x11u8; 32];
        let cluster_b = vec![0x22u8; 32];

        // Seed cluster_a with existing aggregate
        {
            let mut batch = StoreBatch::new(writer.store());
            batch.put_cluster_aggregate(
                &cluster_a,
                &ClusterAggregate {
                    total_count: 5,
                    live_count: 3,
                    ..Default::default()
                },
            );
            batch.commit().unwrap();
        }

        // Apply capacity deltas across two dates for cluster_a, one for cluster_b
        let mut changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
        changes.insert((cluster_a.clone(), 20260101), (1000, 400));
        changes.insert((cluster_a.clone(), 20260102), (500, 200));
        changes.insert((cluster_b.clone(), 20260101), (300, 100));

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer.new_spore_batch_state();
        writer
            .apply_cluster_capacity_deltas(&changes, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        let agg_a = store.get_cluster_aggregate(&cluster_a).unwrap().unwrap();
        assert_eq!(agg_a.owned_capacity, 1500);
        assert_eq!(agg_a.owned_knowledge, 600);
        assert_eq!(agg_a.total_count, 5); // unchanged
        assert_eq!(agg_a.live_count, 3); // unchanged

        let agg_b = store.get_cluster_aggregate(&cluster_b).unwrap().unwrap();
        assert_eq!(agg_b.owned_capacity, 300);
        assert_eq!(agg_b.owned_knowledge, 100);
    }
}
