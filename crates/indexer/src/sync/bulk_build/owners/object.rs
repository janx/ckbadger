use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::store::{CF_IDENTITY_BY_COLLECTION, CF_STATS_IDENTITY};
use ckbadger_store::types::{
    ClusterAggregate, IdentityCollectionAggregate, IdentityEntry, IdentityExtra, IdentityStandard,
    ObjectEntry, ObjectExtra, ObjectStandard, SporeTypeIndex, StorageDependencyTier,
    DID_CKB_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::{
    CkbadgerStore, CF_CLUSTER_AGG, CF_IDENTITY_AGG, CF_IDENTITY_DATA, CF_SPORE_BY_CLUSTER,
    CF_SPORE_DATA, CF_STATS_SPORE,
};
use rocksdb::IteratorMode;

use super::{BulkReducer, ReducerContext};
use crate::parser::analyze_spore_media_profile;
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{
    CellFacts, CellProtocolFacts, ResolvedInputFacts, ResolvedTxFacts,
};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct ObjectOwner {
    spore_entries: BTreeMap<Vec<u8>, ObjectEntry>,
    identities: BTreeMap<Vec<u8>, IdentityEntry>,
    cluster_aggs: BTreeMap<Vec<u8>, ClusterAggregate>,
    did_agg: Option<IdentityCollectionAggregate>,
    identity_by_collection: BTreeSet<Vec<u8>>,
    spore_by_cluster: BTreeSet<Vec<u8>>,
    stats_spore_rows: BTreeMap<Vec<u8>, Vec<u8>>,
    did_owner_counts: BTreeMap<Vec<u8>, i64>,
    cluster_owner_counts: BTreeMap<(Vec<u8>, Vec<u8>), i64>,
}

impl BulkReducer for ObjectOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts, ctx: &ReducerContext<'_>) -> Result<()> {
        for input in &tx.resolved_inputs {
            self.apply_input(input)?;
        }

        for cell in &tx.cells {
            self.apply_output(cell, ctx, tx)?;
        }

        Ok(())
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        let sealed_rows: Vec<MaterializedRow> = self
            .stats_spore_rows
            .iter()
            .map(|(key, value)| MaterializedRow::new(CF_STATS_SPORE, key.clone(), value.clone()))
            .collect();
        if !sealed_rows.is_empty() {
            materializer.stream_sealed_aggregate_rows(&sealed_rows)?;
        }

        let mut final_rows = Vec::new();
        for (id, entry) in &self.spore_entries {
            final_rows.push(MaterializedRow::new(
                CF_SPORE_DATA,
                id.clone(),
                bincode::serialize(entry)?,
            ));
        }
        for key in &self.spore_by_cluster {
            final_rows.push(MaterializedRow::new(
                CF_SPORE_BY_CLUSTER,
                key.clone(),
                Vec::new(),
            ));
        }
        for (id, entry) in &self.identities {
            final_rows.push(MaterializedRow::new(
                CF_IDENTITY_DATA,
                id.clone(),
                bincode::serialize(entry)?,
            ));
        }
        for key in &self.identity_by_collection {
            final_rows.push(MaterializedRow::new(
                CF_IDENTITY_BY_COLLECTION,
                key.clone(),
                Vec::new(),
            ));
        }
        if let Some(agg) = &self.did_agg {
            final_rows.push(MaterializedRow::new(
                CF_IDENTITY_AGG,
                DID_CKB_SENTINEL_COLLECTION.to_vec(),
                bincode::serialize(agg)?,
            ));
        }
        for (cluster_id, agg) in &self.cluster_aggs {
            final_rows.push(MaterializedRow::new(
                CF_CLUSTER_AGG,
                cluster_id.clone(),
                bincode::serialize(agg)?,
            ));
        }
        for (lock_hash, count) in &self.did_owner_counts {
            if *count <= 0 {
                continue;
            }
            final_rows.push(MaterializedRow::new(
                CF_STATS_IDENTITY,
                keys::encode_identity_owner_key(&DID_CKB_SENTINEL_COLLECTION, lock_hash).to_vec(),
                count.to_le_bytes().to_vec(),
            ));
        }

        materializer.materialize_final_snapshot(&final_rows)
    }
}

impl ObjectOwner {
    pub(crate) fn apply_identity_activity_count_deltas(
        &mut self,
        deltas: &HashMap<Vec<u8>, i64>,
    ) -> Result<()> {
        for (collection_id, delta) in deltas {
            if *delta == 0 {
                continue;
            }
            if collection_id.as_slice() != DID_CKB_SENTINEL_COLLECTION {
                bail!(
                    "object owner only supports did:ckb identity activity deltas for now: collection_id=0x{} delta={}",
                    hex::encode(collection_id),
                    delta
                );
            }

            let agg = self.did_agg.get_or_insert_with(|| IdentityCollectionAggregate {
                name: Some("did:ckb".to_string()),
                standard: IdentityStandard::DidCkb,
                ..IdentityCollectionAggregate::default()
            });
            agg.activities_count = checked_next_i64(
                agg.activities_count,
                *delta,
                "did:ckb activities_count",
                collection_id,
                0,
            )?;
        }

        Ok(())
    }

    fn apply_input(&mut self, input: &ResolvedInputFacts) -> Result<()> {
        let Some(protocol) = input.protocol_facts.as_ref() else {
            return Ok(());
        };

        match protocol {
            CellProtocolFacts::Spore(spore) if spore.is_did => {
                self.consume_did(spore.spore_id.as_slice())
            }
            CellProtocolFacts::Spore(spore) => self.consume_spore(spore.spore_id.as_slice()),
            CellProtocolFacts::Cluster(_) => bail!(
                "object owner does not support consumed cluster cells yet: outpoint=0x{}:{}",
                hex::encode(input.outpoint.tx_hash),
                input.outpoint.index
            ),
            CellProtocolFacts::MnftIssuer(_)
            | CellProtocolFacts::MnftClass(_)
            | CellProtocolFacts::MnftToken(_)
            | CellProtocolFacts::Dotbit(_) => bail!(
                "object owner received unsupported protocol facts on input: outpoint=0x{}:{} protocol={:?}",
                hex::encode(input.outpoint.tx_hash),
                input.outpoint.index,
                protocol
            ),
        }
    }

    fn apply_output(
        &mut self,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
    ) -> Result<()> {
        let Some(protocol) = cell.protocol_facts.as_ref() else {
            return Ok(());
        };

        match protocol {
            CellProtocolFacts::Cluster(cluster) => self.insert_cluster(cluster, cell, ctx, tx),
            CellProtocolFacts::Spore(spore) if spore.is_did => {
                self.insert_did(spore, cell, ctx, tx)
            }
            CellProtocolFacts::Spore(spore) => self.insert_spore(spore, cell, ctx, tx),
            CellProtocolFacts::MnftIssuer(_)
            | CellProtocolFacts::MnftClass(_)
            | CellProtocolFacts::MnftToken(_)
            | CellProtocolFacts::Dotbit(_) => bail!(
                "object owner received unsupported protocol facts on output: outpoint=0x{}:{} protocol={:?}",
                hex::encode(cell.outpoint.tx_hash),
                cell.outpoint.index,
                protocol
            ),
        }
    }

    fn insert_cluster(
        &mut self,
        cluster: &crate::sync::bulk_build::facts::ClusterProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
    ) -> Result<()> {
        let cluster_id = cluster.cluster_id.to_vec();
        let existing = self.spore_entries.get(&cluster_id);
        self.spore_entries.insert(
            cluster_id.clone(),
            ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(ctx.resolve_identity(cell.lock_script_hash_id).to_vec()),
                name: cluster.name.clone(),
                description: cluster.description.clone(),
                is_live: true,
                created_at_block: existing
                    .map(|entry| entry.created_at_block)
                    .unwrap_or(tx.block_number),
                created_at_tx: existing
                    .map(|entry| entry.created_at_tx.clone())
                    .unwrap_or_else(|| tx.tx_hash.to_vec()),
                extra: ObjectExtra::SporeCluster,
            },
        );

        let agg = self.cluster_aggs.entry(cluster_id).or_default();
        agg.name = cluster.name.clone();
        agg.description = cluster.description.clone();
        Ok(())
    }

    fn insert_did(
        &mut self,
        did: &crate::sync::bulk_build::facts::SporeProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
    ) -> Result<()> {
        let did_id = did.spore_id.to_vec();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.identities.get(&did_id).cloned();
        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing.as_ref().and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };

        self.identities.insert(
            did_id.clone(),
            IdentityEntry {
                standard: IdentityStandard::DidCkb,
                owner_lock_hash: Some(owner_lock.clone()),
                name: None,
                is_live: true,
                created_at_block: existing
                    .as_ref()
                    .map(|entry| entry.created_at_block)
                    .unwrap_or(tx.block_number),
                created_at_tx: existing
                    .as_ref()
                    .map(|entry| entry.created_at_tx.clone())
                    .unwrap_or_else(|| tx.tx_hash.to_vec()),
                extra: IdentityExtra::DidCkb,
            },
        );

        if existing.is_none() {
            self.identity_by_collection.insert(
                keys::encode_identity_by_collection_key(&DID_CKB_SENTINEL_COLLECTION, &did_id)
                    .to_vec(),
            );
        }

        let did_owner_counts = &mut self.did_owner_counts;
        let agg = self.did_agg.get_or_insert_with(|| IdentityCollectionAggregate {
            name: Some("did:ckb".to_string()),
            standard: IdentityStandard::DidCkb,
            ..IdentityCollectionAggregate::default()
        });
        if existing.is_none() {
            agg.total_count = checked_next_i64(
                agg.total_count,
                1,
                "did:ckb total_count",
                &did_id,
                tx.block_number,
            )?;
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                "did:ckb live_count",
                &did_id,
                tx.block_number,
            )?;
        } else if !was_live {
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                "did:ckb live_count reactivate",
                &did_id,
                tx.block_number,
            )?;
        }

        Self::apply_did_owner_transition(
            did_owner_counts,
            old_owner.as_deref(),
            Some(owner_lock.as_slice()),
            agg,
        )?;
        self.insert_spore_outpoint_rows(&did_id, cell)?;
        Ok(())
    }

    fn insert_spore(
        &mut self,
        spore: &crate::sync::bulk_build::facts::SporeProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts,
    ) -> Result<()> {
        let spore_id = spore.spore_id.to_vec();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.spore_entries.get(&spore_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|entry| entry.standard == ObjectStandard::SporeCluster)
        {
            bail!(
                "spore id collides with cluster entry: spore_id=0x{} block={}",
                hex::encode(&spore_id),
                tx.block_number
            );
        }

        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing.as_ref().and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };
        let old_cluster = existing.as_ref().and_then(|entry| entry.collection_id.clone());
        let old_tier = existing
            .as_ref()
            .map(Self::spore_media_tier)
            .unwrap_or(StorageDependencyTier::Unknown);

        let cluster_id = spore
            .cluster_id
            .map(|value| value.to_vec())
            .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
        let cluster_description = if spore.cluster_id.is_some() {
            self.spore_entries
                .get(cluster_id.as_slice())
                .ok_or_else(|| {
                    anyhow!(
                        "missing cluster entry before spore media analysis: cluster_id=0x{}, spore_id=0x{}, block={}",
                        hex::encode(&cluster_id),
                        hex::encode(&spore_id),
                        tx.block_number
                    )
                })?
                .description
                .clone()
        } else {
            None
        };
        let media_profile = analyze_spore_media_profile(
            &spore.content_type,
            &spore.content,
            cluster_description.as_deref(),
        );
        let new_tier = media_profile.tier;

        self.spore_entries.insert(
            spore_id.clone(),
            ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(cluster_id.clone()),
                token_id: None,
                owner_lock_hash: Some(owner_lock.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: existing
                    .as_ref()
                    .map(|entry| entry.created_at_block)
                    .unwrap_or(tx.block_number),
                created_at_tx: existing
                    .as_ref()
                    .map(|entry| entry.created_at_tx.clone())
                    .unwrap_or_else(|| tx.tx_hash.to_vec()),
                extra: ObjectExtra::Spore {
                    content_type: spore.content_type.clone(),
                    content_length: i64::try_from(spore.content.len())
                        .expect("spore content length exceeds i64"),
                    media_profile,
                },
            },
        );

        if old_cluster.as_ref() != Some(&cluster_id) {
            if let Some(old_cluster_id) = old_cluster.as_ref() {
                self.spore_by_cluster.remove(
                    keys::encode_spore_by_cluster_key(old_cluster_id, &spore_id).as_slice(),
                );
                if was_live {
                    let old_agg = self.cluster_aggs.get_mut(old_cluster_id).ok_or_else(|| {
                        anyhow!(
                            "missing old cluster aggregate during spore move: cluster_id=0x{}, spore_id=0x{}",
                            hex::encode(old_cluster_id),
                            hex::encode(&spore_id)
                        )
                    })?;
                    old_agg.live_count = checked_next_i64(
                        old_agg.live_count,
                        -1,
                        "cluster live_count move old",
                        &spore_id,
                        tx.block_number,
                    )?;
                    Self::adjust_cluster_tier_count(
                        old_agg,
                        old_tier,
                        -1,
                        old_cluster_id,
                        &spore_id,
                    )?;
                    Self::apply_cluster_owner_transition(
                        &mut self.cluster_owner_counts,
                        &mut self.stats_spore_rows,
                        old_cluster_id,
                        old_owner.as_deref(),
                        None,
                        old_agg,
                    )?;
                }
            }
        }

        self.spore_by_cluster.insert(
            keys::encode_spore_by_cluster_key(&cluster_id, &spore_id).to_vec(),
        );

        let agg = self.cluster_aggs.entry(cluster_id.clone()).or_default();
        if existing.is_none() {
            agg.total_count = checked_next_i64(
                agg.total_count,
                1,
                "cluster total_count insert",
                &spore_id,
                tx.block_number,
            )?;
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                "cluster live_count insert",
                &spore_id,
                tx.block_number,
            )?;
            Self::adjust_cluster_tier_count(agg, new_tier, 1, &cluster_id, &spore_id)?;
            Self::apply_cluster_owner_transition(
                &mut self.cluster_owner_counts,
                &mut self.stats_spore_rows,
                &cluster_id,
                None,
                Some(owner_lock.as_slice()),
                agg,
            )?;
        } else if !was_live || old_cluster.as_ref() != Some(&cluster_id) {
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                "cluster live_count reactivate",
                &spore_id,
                tx.block_number,
            )?;
            Self::adjust_cluster_tier_count(agg, new_tier, 1, &cluster_id, &spore_id)?;
            Self::apply_cluster_owner_transition(
                &mut self.cluster_owner_counts,
                &mut self.stats_spore_rows,
                &cluster_id,
                None,
                Some(owner_lock.as_slice()),
                agg,
            )?;
        } else {
            if old_tier != new_tier {
                Self::adjust_cluster_tier_count(agg, old_tier, -1, &cluster_id, &spore_id)?;
                Self::adjust_cluster_tier_count(agg, new_tier, 1, &cluster_id, &spore_id)?;
            }
            Self::apply_cluster_owner_transition(
                &mut self.cluster_owner_counts,
                &mut self.stats_spore_rows,
                &cluster_id,
                old_owner.as_deref(),
                Some(owner_lock.as_slice()),
                agg,
            )?;
        }

        self.insert_spore_outpoint_rows(&spore_id, cell)?;
        let type_script_hash = cell
            .type_script_hash_id
            .map(|id| ctx.resolve_identity(id).to_vec())
            .ok_or_else(|| {
                anyhow!(
                    "missing type_script_hash for spore output: spore_id=0x{}, outpoint=0x{}:{}",
                    hex::encode(&spore_id),
                    hex::encode(cell.outpoint.tx_hash),
                    cell.outpoint.index
                )
            })?;
        let index = SporeTypeIndex {
            spore_id: spore_id.clone(),
            cluster_id: spore.cluster_id.map(|value| value.to_vec()),
        };
        self.stats_spore_rows.insert(
            keys::encode_spore_type_index_key(&type_script_hash).to_vec(),
            bincode::serialize(&index)?,
        );
        Ok(())
    }

    fn consume_did(&mut self, did_id: &[u8]) -> Result<()> {
        let entry = self.identities.get_mut(did_id).ok_or_else(|| {
            anyhow!(
                "missing did:ckb identity during consume: did_id=0x{}",
                hex::encode(did_id)
            )
        })?;
        if !entry.is_live {
            bail!("did:ckb identity already consumed: did_id=0x{}", hex::encode(did_id));
        }
        let old_owner = entry.owner_lock_hash.clone();
        entry.is_live = false;
        entry.owner_lock_hash = None;

        let did_owner_counts = &mut self.did_owner_counts;
        let agg = self.did_agg.get_or_insert_with(|| IdentityCollectionAggregate {
            name: Some("did:ckb".to_string()),
            standard: IdentityStandard::DidCkb,
            ..IdentityCollectionAggregate::default()
        });
        agg.live_count = checked_next_i64(agg.live_count, -1, "did:ckb live_count consume", did_id, 0)?;
        Self::apply_did_owner_transition(did_owner_counts, old_owner.as_deref(), None, agg)
    }

    fn consume_spore(&mut self, spore_id: &[u8]) -> Result<()> {
        let entry = self.spore_entries.get_mut(spore_id).ok_or_else(|| {
            anyhow!("missing spore during consume: spore_id=0x{}", hex::encode(spore_id))
        })?;
        if entry.standard != ObjectStandard::Spore {
            bail!(
                "consume_spore expected spore entry, found {:?}: spore_id=0x{}",
                entry.standard,
                hex::encode(spore_id)
            );
        }
        if !entry.is_live {
            bail!("spore already consumed: spore_id=0x{}", hex::encode(spore_id));
        }

        let old_owner = entry.owner_lock_hash.clone();
        let cluster_id = entry.collection_id.clone().ok_or_else(|| {
            anyhow!(
                "live spore missing collection_id during consume: spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;
        let old_tier = Self::spore_media_tier(entry);
        entry.is_live = false;
        entry.owner_lock_hash = None;

        let agg = self.cluster_aggs.get_mut(cluster_id.as_slice()).ok_or_else(|| {
            anyhow!(
                "missing cluster aggregate during spore consume: cluster_id=0x{}, spore_id=0x{}",
                hex::encode(&cluster_id),
                hex::encode(spore_id)
            )
        })?;
        agg.live_count = checked_next_i64(
            agg.live_count,
            -1,
            "cluster live_count consume",
            spore_id,
            0,
        )?;
        Self::adjust_cluster_tier_count(agg, old_tier, -1, &cluster_id, spore_id)?;
        Self::apply_cluster_owner_transition(
            &mut self.cluster_owner_counts,
            &mut self.stats_spore_rows,
            &cluster_id,
            old_owner.as_deref(),
            None,
            agg,
        )
    }

    fn apply_did_owner_transition(
        did_owner_counts: &mut BTreeMap<Vec<u8>, i64>,
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut IdentityCollectionAggregate,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_owner) = old_owner {
            let current = *did_owner_counts.get(old_owner).unwrap_or(&0);
            if current <= 0 {
                bail!(
                    "did:ckb owner count underflow: lock_hash=0x{}, current={}",
                    hex::encode(old_owner),
                    current
                );
            }
            if current == 1 {
                did_owner_counts.remove(old_owner);
                agg.holders_count = checked_next_i64(
                    agg.holders_count,
                    -1,
                    "did:ckb holders_count remove",
                    old_owner,
                    0,
                )?;
            } else {
                did_owner_counts.insert(old_owner.to_vec(), current - 1);
            }
        }

        if let Some(new_owner) = new_owner {
            let current = *did_owner_counts.get(new_owner).unwrap_or(&0);
            if current == 0 {
                agg.holders_count = checked_next_i64(
                    agg.holders_count,
                    1,
                    "did:ckb holders_count add",
                    new_owner,
                    0,
                )?;
            }
            did_owner_counts.insert(new_owner.to_vec(), current + 1);
        }

        Ok(())
    }

    fn apply_cluster_owner_transition(
        cluster_owner_counts: &mut BTreeMap<(Vec<u8>, Vec<u8>), i64>,
        stats_spore_rows: &mut BTreeMap<Vec<u8>, Vec<u8>>,
        cluster_id: &[u8],
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut ClusterAggregate,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_owner) = old_owner {
            let key = (cluster_id.to_vec(), old_owner.to_vec());
            let current = *cluster_owner_counts.get(&key).unwrap_or(&0);
            if current <= 0 {
                bail!(
                    "cluster owner count underflow: cluster_id=0x{}, lock_hash=0x{}, current={}",
                    hex::encode(cluster_id),
                    hex::encode(old_owner),
                    current
                );
            }
            if current == 1 {
                cluster_owner_counts.remove(&key);
                stats_spore_rows
                    .remove(keys::encode_cluster_owner_key(cluster_id, old_owner).as_slice());
                agg.owner_count = checked_next_i64(
                    agg.owner_count,
                    -1,
                    "cluster owner_count remove",
                    cluster_id,
                    0,
                )?;
            } else {
                let next = current - 1;
                cluster_owner_counts.insert(key, next);
                stats_spore_rows.insert(
                    keys::encode_cluster_owner_key(cluster_id, old_owner).to_vec(),
                    next.to_le_bytes().to_vec(),
                );
            }
        }

        if let Some(new_owner) = new_owner {
            let key = (cluster_id.to_vec(), new_owner.to_vec());
            let current = *cluster_owner_counts.get(&key).unwrap_or(&0);
            if current == 0 {
                agg.owner_count = checked_next_i64(
                    agg.owner_count,
                    1,
                    "cluster owner_count add",
                    cluster_id,
                    0,
                )?;
            }
            let next = current + 1;
            cluster_owner_counts.insert(key, next);
            stats_spore_rows.insert(
                keys::encode_cluster_owner_key(cluster_id, new_owner).to_vec(),
                next.to_le_bytes().to_vec(),
            );
        }

        Ok(())
    }

    fn insert_spore_outpoint_rows(&mut self, spore_id: &[u8], cell: &CellFacts) -> Result<()> {
        let output_index = i16::try_from(cell.outpoint.index).map_err(|_| {
            anyhow!(
                "spore outpoint index exceeds i16: outpoint=0x{}:{}",
                hex::encode(cell.outpoint.tx_hash),
                cell.outpoint.index
            )
        })?;
        self.stats_spore_rows.insert(
            keys::encode_spore_outpoint_key(&cell.outpoint.tx_hash, output_index).to_vec(),
            spore_id.to_vec(),
        );
        self.stats_spore_rows.insert(
            keys::encode_spore_outpoint_by_id_key(spore_id, &cell.outpoint.tx_hash, output_index)
                .to_vec(),
            Vec::new(),
        );
        Ok(())
    }

    fn spore_media_tier(entry: &ObjectEntry) -> StorageDependencyTier {
        match &entry.extra {
            ObjectExtra::Spore { media_profile, .. } => media_profile.tier,
            _ => StorageDependencyTier::Unknown,
        }
    }

    fn adjust_cluster_tier_count(
        agg: &mut ClusterAggregate,
        tier: StorageDependencyTier,
        delta: i64,
        cluster_id: &[u8],
        spore_id: &[u8],
    ) -> Result<()> {
        let slot = match tier {
            StorageDependencyTier::FullyOnchain => &mut agg.fully_onchain_count,
            StorageDependencyTier::DecentralizedExternal => &mut agg.decentralized_external_count,
            StorageDependencyTier::CentralizedDependent => &mut agg.centralized_dependent_count,
            StorageDependencyTier::Unknown => &mut agg.unknown_count,
        };
        let next = slot.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "cluster tier count overflow: cluster_id=0x{}, spore_id=0x{}, tier={}, current={}, delta={}",
                hex::encode(cluster_id),
                hex::encode(spore_id),
                tier.as_str(),
                *slot,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "cluster tier count underflow: cluster_id=0x{}, spore_id=0x{}, tier={}, current={}, delta={}",
                hex::encode(cluster_id),
                hex::encode(spore_id),
                tier.as_str(),
                *slot,
                delta
            );
        }
        *slot = next;
        Ok(())
    }
}

fn checked_next_i64(current: i64, delta: i64, label: &str, key: &[u8], block_number: i64) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: key=0x{}, current={}, delta={}, block={}",
            label,
            hex::encode(key),
            current,
            delta,
            block_number
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: key=0x{}, current={}, delta={}, block={}",
            label,
            hex::encode(key),
            current,
            delta,
            block_number
        );
    }
    Ok(next)
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct ObjectStateSnapshot {
    pub spores: HashMap<Vec<u8>, ObjectEntry>,
    pub identities: HashMap<Vec<u8>, IdentityEntry>,
    pub cluster_aggs: HashMap<Vec<u8>, ClusterAggregate>,
    pub did_agg: Option<IdentityCollectionAggregate>,
    pub identities_by_collection: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    pub spores_by_cluster: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    pub did_owner_counts: HashMap<Vec<u8>, i64>,
    pub cluster_owner_counts: HashMap<Vec<u8>, HashMap<Vec<u8>, i64>>,
    pub spore_outpoints: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>>,
    pub spore_type_indexes: HashMap<Vec<u8>, SporeTypeIndex>,
}

#[doc(hidden)]
pub(crate) fn materialize_object_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<ObjectStateSnapshot> {
    let mut interner = IdentityInterner::default();
    let arena = build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let ctx = ReducerContext::new(&interner);
    let mut owner = ObjectOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = unique_temp_test_dir("bulk-build-object-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();

        let spores = domain_store
            .list_spores(usize::MAX)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let identities = domain_store
            .list_identities(usize::MAX)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let cluster_aggs = domain_store
            .list_cluster_aggregates()?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let did_agg =
            domain_store.get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)?;

        let mut identities_by_collection = HashMap::new();
        let mut did_ids =
            domain_store.list_identity_ids_by_collection(&DID_CKB_SENTINEL_COLLECTION, None, usize::MAX)?;
        did_ids.sort();
        if !did_ids.is_empty() {
            identities_by_collection.insert(DID_CKB_SENTINEL_COLLECTION.to_vec(), did_ids);
        }

        let mut spores_by_cluster = HashMap::new();
        let mut cluster_owner_counts = HashMap::new();
        for cluster_id in cluster_aggs.keys() {
            let mut members = domain_store
                .list_spores_by_cluster(cluster_id, usize::MAX)?
                .into_iter()
                .map(|(spore_id, _entry)| spore_id)
                .collect::<Vec<_>>();
            members.sort();
            spores_by_cluster.insert(cluster_id.clone(), members);

            let owners = domain_store
                .list_cluster_owner_counts(cluster_id)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            cluster_owner_counts.insert(cluster_id.clone(), owners);
        }

        let did_owner_counts = domain_store
            .list_identity_owner_counts(&DID_CKB_SENTINEL_COLLECTION)?
            .into_iter()
            .collect::<HashMap<_, _>>();

        let mut spore_outpoints = HashMap::new();
        for (spore_id, entry) in &spores {
            if entry.standard != ObjectStandard::Spore {
                continue;
            }
            let mut outpoints = domain_store.list_spore_outpoints_by_spore_id(spore_id)?;
            outpoints.sort();
            spore_outpoints.insert(spore_id.clone(), outpoints);
        }

        let mut spore_type_indexes = HashMap::new();
        let iter = domain_store.iterator_cf(domain_store.cf_stats_spore(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item?;
            if key.len() != keys::SPORE_TYPE_INDEX_KEY_SIZE
                || key[0] != keys::STATS_PREFIX_SPORE_TYPE_INDEX
            {
                continue;
            }
            let type_hash = key[1..33].to_vec();
            let index: SporeTypeIndex = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize SporeTypeIndex in object snapshot helper: type_hash=0x{}, error={}",
                    hex::encode(&type_hash),
                    e
                )
            })?;
            spore_type_indexes.insert(type_hash, index);
        }

        ObjectStateSnapshot {
            spores,
            identities,
            cluster_aggs,
            did_agg,
            identities_by_collection,
            spores_by_cluster,
            did_owner_counts,
            cluster_owner_counts,
            spore_outpoints,
            spore_type_indexes,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn unique_temp_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ckbadger-{}-{}-{}",
        prefix,
        std::process::id(),
        nanos
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{
        CellFacts, CellProtocolFacts, CellSemanticTag, ClusterProtocolFacts, OutPointKey,
        ResolvedInputFacts, ResolvedTxFacts, SporeProtocolFacts,
    };
    use crate::sync::types::InternId;

    #[test]
    fn object_owner_materializes_spore_transfer_and_did_burn_without_db_reads() {
        let mut interner = crate::sync::bulk_build::interner::IdentityInterner::default();
        let lock_a = interner.intern_bytes(vec![0xaa; 32]);
        let lock_b = interner.intern_bytes(vec![0xbb; 32]);
        let lock_c = interner.intern_bytes(vec![0xcc; 32]);
        let spore_type_hash = interner.intern_bytes(vec![0x91; 32]);
        let did_type_hash = interner.intern_bytes(vec![0x92; 32]);
        let ctx = ReducerContext::new(&interner);

        let cluster_id = [0x11; 32];
        let spore_id = [0x22; 32];
        let did_id = [0x33; 32];

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x01; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 0,
            tx_index: 0,
            is_cellbase: false,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x01; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(10),
                lock_hash_type: 1,
                lock_args_id: InternId::new(11),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Cluster,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Cluster(ClusterProtocolFacts {
                    cluster_id,
                    name: Some("Genesis Cluster".to_string()),
                    description: Some("{\"dob\":{\"ver\":1}}".to_string()),
                })),
            }],
        };

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x02; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 0,
            tx_index: 1,
            is_cellbase: false,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x02; 32], 0),
                    created_at_block: 100,
                    capacity: 200_00000000,
                    lock_script_hash_id: lock_a,
                    lock_code_hash_id: InternId::new(12),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(13),
                    type_script_hash_id: Some(spore_type_hash),
                    type_code_hash_id: Some(InternId::new(14)),
                    type_hash_type: Some(2),
                    type_args_id: Some(InternId::new(15)),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: b"spore-content".to_vec(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Spore,
                    dao_state: None,
                    protocol_facts: Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                        spore_id,
                        is_did: false,
                        content_type: "image/png".to_string(),
                        content: b"spore-content".to_vec(),
                        cluster_id: Some(cluster_id),
                    })),
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x02; 32], 1),
                    created_at_block: 100,
                    capacity: 150_00000000,
                    lock_script_hash_id: lock_c,
                    lock_code_hash_id: InternId::new(16),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(17),
                    type_script_hash_id: Some(did_type_hash),
                    type_code_hash_id: Some(InternId::new(18)),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(19)),
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Spore,
                    dao_state: None,
                    protocol_facts: Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                        spore_id: did_id,
                        is_did: true,
                        content_type: String::new(),
                        content: Vec::new(),
                        cluster_id: None,
                    })),
                },
            ],
        };

        let tx2 = ResolvedTxFacts {
            tx_hash: [0x03; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_002,
            block_dao_ar: 0,
            tx_index: 2,
            is_cellbase: false,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x02; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                data_size: 16,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(12),
                lock_hash_type: 1,
                lock_args_id: InternId::new(13),
                type_script_hash_id: Some(spore_type_hash),
                type_code_hash_id: Some(InternId::new(14)),
                type_hash_type: Some(2),
                type_args_id: Some(InternId::new(15)),
                semantic_tag: CellSemanticTag::Spore,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                    spore_id,
                    is_did: false,
                    content_type: "image/png".to_string(),
                    content: b"spore-content".to_vec(),
                    cluster_id: Some(cluster_id),
                })),
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x03; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                lock_script_hash_id: lock_b,
                lock_code_hash_id: InternId::new(20),
                lock_hash_type: 1,
                lock_args_id: InternId::new(21),
                type_script_hash_id: Some(spore_type_hash),
                type_code_hash_id: Some(InternId::new(14)),
                type_hash_type: Some(2),
                type_args_id: Some(InternId::new(15)),
                occupied_capacity: 142_00000000,
                data_size: 16,
                data: b"spore-content".to_vec(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Spore,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                    spore_id,
                    is_did: false,
                    content_type: "image/png".to_string(),
                    content: b"spore-content".to_vec(),
                    cluster_id: Some(cluster_id),
                })),
            }],
        };

        let tx3 = ResolvedTxFacts {
            tx_hash: [0x04; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_003,
            block_dao_ar: 0,
            tx_index: 3,
            is_cellbase: false,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x02; 32], 1),
                created_at_block: 100,
                capacity: 150_00000000,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_c,
                lock_code_hash_id: InternId::new(16),
                lock_hash_type: 1,
                lock_args_id: InternId::new(17),
                type_script_hash_id: Some(did_type_hash),
                type_code_hash_id: Some(InternId::new(18)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(19)),
                semantic_tag: CellSemanticTag::Spore,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                    spore_id: did_id,
                    is_did: true,
                    content_type: String::new(),
                    content: Vec::new(),
                    cluster_id: None,
                })),
            }],
            cells: vec![],
        };

        let mut owner = ObjectOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply cluster");
        owner.apply_tx(&tx1, &ctx).expect("apply create");
        owner.apply_tx(&tx2, &ctx).expect("apply transfer");
        owner.apply_tx(&tx3, &ctx).expect("apply did consume");

        let root = unique_temp_test_dir("bulk-build-object-owner");
        std::fs::create_dir_all(&root).expect("root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("domain");
        std::fs::create_dir_all(&append_path).expect("append");

        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("domain store");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("append store");
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner
            .materialize_final(&mut materializer)
            .expect("materialize object owner");

        let stored_spore = domain_store
            .get_spore(&spore_id)
            .expect("get spore")
            .expect("stored spore");
        assert!(stored_spore.is_live);
        assert_eq!(stored_spore.owner_lock_hash, Some(vec![0xbb; 32]));
        assert_eq!(stored_spore.collection_id, Some(cluster_id.to_vec()));
        assert_eq!(stored_spore.created_at_block, 100);
        assert_eq!(stored_spore.created_at_tx, vec![0x02; 32]);

        let stored_cluster = domain_store
            .get_spore(&cluster_id)
            .expect("get cluster")
            .expect("stored cluster");
        assert_eq!(stored_cluster.standard, ObjectStandard::SporeCluster);
        assert_eq!(stored_cluster.description.as_deref(), Some("{\"dob\":{\"ver\":1}}"));

        let cluster_agg = domain_store
            .get_cluster_aggregate(&cluster_id)
            .expect("cluster agg")
            .expect("cluster agg exists");
        assert_eq!(cluster_agg.total_count, 1);
        assert_eq!(cluster_agg.live_count, 1);
        assert_eq!(cluster_agg.owner_count, 1);

        let cluster_members = domain_store
            .list_spores_by_cluster(&cluster_id, 10)
            .expect("cluster members");
        assert_eq!(cluster_members.len(), 1);
        assert_eq!(cluster_members[0].0, spore_id.to_vec());

        assert_eq!(
            domain_store
                .get_cluster_owner_count(&cluster_id, &[0xaa; 32])
                .expect("owner count a"),
            0
        );
        assert_eq!(
            domain_store
                .get_cluster_owner_count(&cluster_id, &[0xbb; 32])
                .expect("owner count b"),
            1
        );

        assert_eq!(
            domain_store
                .get_spore_id_by_outpoint(&[0x02; 32], 0)
                .expect("first outpoint")
                .expect("first outpoint value"),
            spore_id.to_vec()
        );
        assert_eq!(
            domain_store
                .get_spore_id_by_outpoint(&[0x03; 32], 0)
                .expect("second outpoint")
                .expect("second outpoint value"),
            spore_id.to_vec()
        );
        let type_index = domain_store
            .get_spore_type_index(&[0x91; 32])
            .expect("spore type index")
            .expect("spore type index exists");
        assert_eq!(type_index.spore_id, spore_id.to_vec());
        assert_eq!(type_index.cluster_id, Some(cluster_id.to_vec()));

        let did_entry = domain_store
            .get_identity(&did_id)
            .expect("get did")
            .expect("did entry");
        assert!(!did_entry.is_live);
        assert!(did_entry.owner_lock_hash.is_none());
        let did_agg = domain_store
            .get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)
            .expect("did agg")
            .expect("did agg exists");
        assert_eq!(did_agg.total_count, 1);
        assert_eq!(did_agg.live_count, 0);
        assert_eq!(did_agg.holders_count, 0);
        let did_ids = domain_store
            .list_identity_ids_by_collection(&DID_CKB_SENTINEL_COLLECTION, None, 10)
            .expect("did ids");
        assert_eq!(did_ids, vec![did_id.to_vec()]);
        assert_eq!(
            domain_store
                .get_identity_owner_count(&DID_CKB_SENTINEL_COLLECTION, &[0xcc; 32])
                .expect("did owner count"),
            0
        );

        drop(materializer);
        drop(append_store);
        drop(domain_store);
        let _ = std::fs::remove_dir_all(&root);
    }
}
