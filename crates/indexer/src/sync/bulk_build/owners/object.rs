use std::collections::{BTreeMap, BTreeSet, HashMap};

use rustc_hash::FxHashMap;

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::store::{
    CF_IDENTITY_BY_COLLECTION, CF_OBJECT_BY_COLLECTION, CF_STATS_IDENTITY,
};
use ckbadger_store::types::{
    ClusterAggregate, ClusterDailyDelta, CompositionTier, IdentityCollectionAggregate,
    IdentityEntry, IdentityExtra, IdentityStandard, ObjectCollectionAggregate, ObjectDailyDelta,
    ObjectEntry, ObjectExtra, ObjectStandard, ObjectTypeIndex, SporeDailyDelta, SporeTypeIndex,
    DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::{
    CkbadgerStore, CF_CLUSTER_AGG, CF_IDENTITY_AGG, CF_IDENTITY_DATA, CF_OBJECT_COLLECTION_AGG,
    CF_OBJECT_DATA, CF_SPORE_BY_CLUSTER, CF_SPORE_DATA, CF_STATS_OBJECT, CF_STATS_SPORE,
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
    mnft_entries: BTreeMap<Vec<u8>, ObjectEntry>,
    identities: BTreeMap<Vec<u8>, IdentityEntry>,
    cluster_aggs: BTreeMap<Vec<u8>, ClusterAggregate>,
    object_collection_aggs: BTreeMap<Vec<u8>, ObjectCollectionAggregate>,
    did_agg: Option<IdentityCollectionAggregate>,
    dotbit_agg: Option<IdentityCollectionAggregate>,
    identity_by_collection: BTreeSet<Vec<u8>>,
    spore_by_cluster: BTreeSet<Vec<u8>>,
    stats_spore_rows: BTreeMap<Vec<u8>, Vec<u8>>,
    mnft_by_collection: BTreeSet<Vec<u8>>,
    mnft_owner_counts: BTreeMap<(Vec<u8>, Vec<u8>), i64>,
    mnft_class_outpoints: BTreeMap<Vec<u8>, Vec<u8>>,
    mnft_token_outpoints: BTreeMap<Vec<u8>, Vec<u8>>,
    mnft_type_indexes: BTreeMap<Vec<u8>, ObjectTypeIndex>,
    mnft_hourly_transfers: BTreeMap<Vec<u8>, i64>,
    did_owner_counts: BTreeMap<Vec<u8>, i64>,
    dotbit_owner_counts: BTreeMap<Vec<u8>, i64>,
    dotbit_outpoints: BTreeMap<Vec<u8>, Vec<u8>>,
    dotbit_outpoints_by_account: BTreeSet<Vec<u8>>,
    dotbit_hourly_transfers: BTreeMap<Vec<u8>, i64>,
    cluster_owner_counts: BTreeMap<(Vec<u8>, Vec<u8>), i64>,
    /// Daily capacity deltas keyed by (collection_id, date_yyyymmdd).
    /// Mirrors `object_daily_changes` in the live pipeline.
    object_daily_deltas: BTreeMap<(Vec<u8>, u32), (i128, i128)>,
    spore_daily_deltas: BTreeMap<(Vec<u8>, u32), (i128, i128)>,
    cluster_daily_deltas: BTreeMap<(Vec<u8>, u32), (i128, i128)>,
}

impl BulkReducer for ObjectOwner {
    fn flush_sealed(&mut self, materializer: &mut Materializer<'_>) -> Result<()> {
        let mut sealed_rows: Vec<MaterializedRow> = self
            .stats_spore_rows
            .iter()
            .map(|(key, value)| MaterializedRow::new(CF_STATS_SPORE, key.clone(), value.clone()))
            .collect();
        sealed_rows.extend(
            self.mnft_class_outpoints.iter().map(|(key, value)| {
                MaterializedRow::new(CF_STATS_OBJECT, key.clone(), value.clone())
            }),
        );
        sealed_rows.extend(
            self.mnft_token_outpoints.iter().map(|(key, value)| {
                MaterializedRow::new(CF_STATS_OBJECT, key.clone(), value.clone())
            }),
        );
        sealed_rows.extend(self.mnft_type_indexes.iter().map(|(type_hash, index)| {
            MaterializedRow::new(
                CF_STATS_OBJECT,
                keys::encode_nft_type_index_key(type_hash).to_vec(),
                bincode::serialize(index).expect("object type index serialization must succeed"),
            )
        }));
        sealed_rows.extend(self.mnft_hourly_transfers.iter().map(|(key, count)| {
            MaterializedRow::new(CF_STATS_OBJECT, key.clone(), count.to_le_bytes().to_vec())
        }));
        sealed_rows.extend(
            self.dotbit_outpoints.iter().map(|(key, value)| {
                MaterializedRow::new(CF_STATS_OBJECT, key.clone(), value.clone())
            }),
        );
        sealed_rows.extend(
            self.dotbit_outpoints_by_account
                .iter()
                .map(|key| MaterializedRow::new(CF_STATS_OBJECT, key.clone(), Vec::new())),
        );
        sealed_rows.extend(self.dotbit_hourly_transfers.iter().map(|(key, count)| {
            MaterializedRow::new(CF_STATS_OBJECT, key.clone(), count.to_le_bytes().to_vec())
        }));
        sealed_rows.extend(
            self.object_daily_deltas
                .iter()
                .filter(|(_, (cap, know))| *cap != 0 || *know != 0)
                .map(|((collection_id, date), (cap_delta, know_delta))| {
                    MaterializedRow::new(
                        CF_STATS_OBJECT,
                        keys::encode_nft_daily_key(collection_id, *date).to_vec(),
                        bincode::serialize(&ObjectDailyDelta {
                            owned_capacity_delta: *cap_delta,
                            owned_knowledge_delta: *know_delta,
                        })
                        .expect("serialize ObjectDailyDelta"),
                    )
                }),
        );
        sealed_rows.extend(
            self.spore_daily_deltas
                .iter()
                .filter(|(_, (cap, know))| *cap != 0 || *know != 0)
                .map(|((spore_id, date), (cap_delta, know_delta))| {
                    MaterializedRow::new(
                        CF_STATS_SPORE,
                        keys::encode_spore_daily_key(spore_id, *date).to_vec(),
                        bincode::serialize(&SporeDailyDelta {
                            owned_capacity_delta: *cap_delta,
                            owned_knowledge_delta: *know_delta,
                        })
                        .expect("serialize SporeDailyDelta"),
                    )
                }),
        );
        sealed_rows.extend(
            self.cluster_daily_deltas
                .iter()
                .filter(|(_, (cap, know))| *cap != 0 || *know != 0)
                .map(|((cluster_id, date), (cap_delta, know_delta))| {
                    MaterializedRow::new(
                        CF_STATS_SPORE,
                        keys::encode_cluster_daily_key(cluster_id, *date).to_vec(),
                        bincode::serialize(&ClusterDailyDelta {
                            owned_capacity_delta: *cap_delta,
                            owned_knowledge_delta: *know_delta,
                        })
                        .expect("serialize ClusterDailyDelta"),
                    )
                }),
        );
        sealed_rows.extend(
            self.mnft_owner_counts
                .iter()
                .filter(|(_, count)| **count > 0)
                .map(|((class_id, lock_hash), count)| {
                    MaterializedRow::new(
                        CF_STATS_OBJECT,
                        keys::encode_nft_collection_owner_key(class_id, lock_hash).to_vec(),
                        count.to_le_bytes().to_vec(),
                    )
                }),
        );
        if !sealed_rows.is_empty() {
            materializer.stream_sealed_aggregate_rows(&sealed_rows)?;
        }
        Ok(())
    }

    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let date_yyyymmdd = keys::timestamp_ms_to_date(tx.timestamp_ms);
        let mnft_tokens_consumed_in_tx = tx
            .resolved_inputs
            .iter()
            .filter_map(|input| match input.protocol_facts.as_ref() {
                Some(CellProtocolFacts::MnftToken(token)) => Some(token.token_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let dotbit_accounts_consumed_in_tx = tx
            .resolved_inputs
            .iter()
            .filter_map(|input| match input.protocol_facts.as_ref() {
                Some(CellProtocolFacts::Dotbit(dotbit)) => Some((
                    dotbit.account_id.to_vec(),
                    ctx.resolve_identity(input.lock_script_hash_id).to_vec(),
                )),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        for input in &tx.resolved_inputs {
            if let Some(collection_id) =
                classify_nft_collection_from_protocol(&input.protocol_facts)
            {
                let entry = self
                    .object_daily_deltas
                    .entry((collection_id, date_yyyymmdd))
                    .or_insert((0, 0));
                entry.0 -= i128::from(input.capacity);
                entry.1 -= i128::from(input.occupied_capacity);
            }
            if let Some(CellProtocolFacts::Spore(spore)) = input.protocol_facts.as_ref() {
                if !spore.is_did {
                    self.record_spore_daily_delta(
                        spore.spore_id.as_slice(),
                        spore.cluster_id.as_ref().map(|id| id.as_slice()),
                        date_yyyymmdd,
                        -i128::from(input.capacity),
                        -i128::from(input.occupied_capacity),
                    );
                }
            }
            self.apply_input(input)?;
        }

        for cell in tx.cells.iter() {
            if let Some(collection_id) = classify_nft_collection_from_protocol(&cell.protocol_facts)
            {
                let entry = self
                    .object_daily_deltas
                    .entry((collection_id, date_yyyymmdd))
                    .or_insert((0, 0));
                entry.0 += i128::from(cell.capacity);
                entry.1 += i128::from(cell.occupied_capacity);
            }
            if let Some(CellProtocolFacts::Spore(spore)) = cell.protocol_facts.as_ref() {
                if !spore.is_did {
                    self.record_spore_daily_delta(
                        spore.spore_id.as_slice(),
                        spore.cluster_id.as_ref().map(|id| id.as_slice()),
                        date_yyyymmdd,
                        i128::from(cell.capacity),
                        i128::from(cell.occupied_capacity),
                    );
                }
            }
            self.apply_output(
                cell,
                ctx,
                tx,
                &mnft_tokens_consumed_in_tx,
                &dotbit_accounts_consumed_in_tx,
            )?;
        }

        Ok(())
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
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
        for (id, entry) in &self.mnft_entries {
            final_rows.push(MaterializedRow::new(
                CF_OBJECT_DATA,
                id.clone(),
                bincode::serialize(entry)?,
            ));
        }
        for key in &self.mnft_by_collection {
            final_rows.push(MaterializedRow::new(
                CF_OBJECT_BY_COLLECTION,
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
        if let Some(agg) = &self.dotbit_agg {
            final_rows.push(MaterializedRow::new(
                CF_IDENTITY_AGG,
                DOTBIT_SENTINEL_COLLECTION.to_vec(),
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
        for (class_id, agg) in &self.object_collection_aggs {
            final_rows.push(MaterializedRow::new(
                CF_OBJECT_COLLECTION_AGG,
                class_id.clone(),
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
        for (lock_hash, count) in &self.dotbit_owner_counts {
            if *count <= 0 {
                continue;
            }
            final_rows.push(MaterializedRow::new(
                CF_STATS_IDENTITY,
                keys::encode_identity_owner_key(&DOTBIT_SENTINEL_COLLECTION, lock_hash).to_vec(),
                count.to_le_bytes().to_vec(),
            ));
        }

        materializer.materialize_final_snapshot(&final_rows)
    }
}

impl ObjectOwner {
    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::btree_map_serialized_bytes(&self.spore_entries)
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(&self.mnft_entries)
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(&self.identities)
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(&self.cluster_aggs)
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.object_collection_aggs,
            )
            + self
                .did_agg
                .as_ref()
                .map_or(0, crate::sync::bulk_build::accounting::serialized_bytes)
            + self
                .dotbit_agg
                .as_ref()
                .map_or(0, crate::sync::bulk_build::accounting::serialized_bytes)
            + crate::sync::bulk_build::accounting::btree_set_serialized_bytes(
                &self.identity_by_collection,
            )
            + crate::sync::bulk_build::accounting::btree_set_serialized_bytes(
                &self.spore_by_cluster,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.stats_spore_rows,
            )
            + crate::sync::bulk_build::accounting::btree_set_serialized_bytes(
                &self.mnft_by_collection,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.mnft_owner_counts,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.mnft_class_outpoints,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.mnft_token_outpoints,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.mnft_type_indexes,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.mnft_hourly_transfers,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.did_owner_counts,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.dotbit_owner_counts,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.dotbit_outpoints,
            )
            + crate::sync::bulk_build::accounting::btree_set_serialized_bytes(
                &self.dotbit_outpoints_by_account,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.dotbit_hourly_transfers,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.cluster_owner_counts,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.object_daily_deltas,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.spore_daily_deltas,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.cluster_daily_deltas,
            )
    }

    pub(crate) fn apply_identity_activity_count_deltas(
        &mut self,
        deltas: &FxHashMap<Vec<u8>, i64>,
    ) -> Result<()> {
        for (collection_id, delta) in deltas {
            if *delta == 0 {
                continue;
            }
            let (label, agg) = match collection_id.as_slice() {
                x if x == DID_CKB_SENTINEL_COLLECTION => (
                    "did:ckb",
                    self.did_agg
                        .get_or_insert_with(|| IdentityCollectionAggregate {
                            name: Some("did:ckb".to_string()),
                            standard: IdentityStandard::DidCkb,
                            ..IdentityCollectionAggregate::default()
                        }),
                ),
                x if x == DOTBIT_SENTINEL_COLLECTION => (
                    ".bit",
                    self.dotbit_agg
                        .get_or_insert_with(|| IdentityCollectionAggregate {
                            name: Some(".bit".to_string()),
                            standard: IdentityStandard::DotBit,
                            ..IdentityCollectionAggregate::default()
                        }),
                ),
                _ => {
                    bail!(
                        "unsupported identity activity delta in object owner: collection_id=0x{} delta={}",
                        hex::encode(collection_id),
                        delta
                    );
                }
            };

            agg.activities_count = checked_next_i64(
                agg.activities_count,
                *delta,
                &format!("{} activities_count", label),
                collection_id,
                0,
            )?;
        }

        Ok(())
    }

    pub(crate) fn apply_object_activity_count_deltas(
        &mut self,
        deltas: &FxHashMap<Vec<u8>, i64>,
    ) -> Result<()> {
        for (collection_id, delta) in deltas {
            if *delta == 0 {
                continue;
            }
            let agg = match self
                .object_collection_aggs
                .get_mut(collection_id.as_slice())
            {
                Some(agg) => agg,
                None => {
                    // Spore cluster activities share the same activity CF but their
                    // counts live in ClusterAggregate, not ObjectCollectionAggregate.
                    // Skip silently if the collection_id belongs to a cluster.
                    if self.cluster_aggs.contains_key(collection_id.as_slice()) {
                        continue;
                    }
                    bail!(
                        "missing object collection aggregate while applying activity_count delta in bulk build: collection_id=0x{} delta={}",
                        hex::encode(collection_id),
                        delta
                    );
                }
            };

            agg.activities_count = checked_next_i64(
                agg.activities_count,
                *delta,
                "object collection activities_count",
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
            CellProtocolFacts::MnftIssuer(issuer) => self.consume_mnft_issuer(&issuer.issuer_id),
            CellProtocolFacts::MnftClass(class) => self.consume_mnft_class(&class.class_id),
            CellProtocolFacts::MnftToken(token) => self.consume_mnft_token(&token.token_id),
            CellProtocolFacts::Cluster(cluster) => self.consume_cluster(&cluster.cluster_id),
            CellProtocolFacts::Dotbit(dotbit) => self.consume_dotbit(&dotbit.account_id),
        }
    }

    fn apply_output(
        &mut self,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
        mnft_tokens_consumed_in_tx: &BTreeSet<Vec<u8>>,
        dotbit_accounts_consumed_in_tx: &BTreeMap<Vec<u8>, Vec<u8>>,
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
            CellProtocolFacts::MnftIssuer(issuer) => self.insert_mnft_issuer(issuer, cell, ctx, tx),
            CellProtocolFacts::MnftClass(class) => self.insert_mnft_class(class, cell, ctx, tx),
            CellProtocolFacts::MnftToken(token) => self.insert_mnft_token(
                token,
                cell,
                ctx,
                tx,
                mnft_tokens_consumed_in_tx.contains(token.token_id.as_slice()),
            ),
            CellProtocolFacts::Dotbit(dotbit) => self.insert_dotbit(
                dotbit,
                cell,
                ctx,
                tx,
                dotbit_accounts_consumed_in_tx
                    .get(dotbit.account_id.as_slice())
                    .map(Vec::as_slice),
            ),
        }
    }

    fn record_spore_daily_delta(
        &mut self,
        spore_id: &[u8],
        cluster_id: Option<&[u8]>,
        date_yyyymmdd: u32,
        capacity_delta: i128,
        occupied_delta: i128,
    ) {
        let spore_entry = self
            .spore_daily_deltas
            .entry((spore_id.to_vec(), date_yyyymmdd))
            .or_insert((0, 0));
        spore_entry.0 += capacity_delta;
        spore_entry.1 += occupied_delta;

        let effective_cluster_id = cluster_id
            .map(|id| id.to_vec())
            .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
        let cluster_entry = self
            .cluster_daily_deltas
            .entry((effective_cluster_id, date_yyyymmdd))
            .or_insert((0, 0));
        cluster_entry.0 += capacity_delta;
        cluster_entry.1 += occupied_delta;
    }

    fn insert_cluster(
        &mut self,
        cluster: &crate::sync::bulk_build::facts::ClusterProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
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
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        let did_id = did.spore_id.to_vec();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.identities.get(&did_id).cloned();
        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
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
        let agg = self
            .did_agg
            .get_or_insert_with(|| IdentityCollectionAggregate {
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
        tx: &ResolvedTxFacts<'_>,
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
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };
        let old_cluster = existing
            .as_ref()
            .and_then(|entry| entry.collection_id.clone());
        let old_tier = existing
            .as_ref()
            .map(Self::spore_media_tier)
            .unwrap_or(CompositionTier::Unknown);

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
            true, // skip DOB decode during bulk sync; backfilled by background worker
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

        self.spore_by_cluster
            .insert(keys::encode_spore_by_cluster_key(&cluster_id, &spore_id).to_vec());

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

    fn insert_mnft_issuer(
        &mut self,
        issuer: &crate::sync::bulk_build::facts::MnftIssuerProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        let issuer_id = issuer.issuer_id.to_vec();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.mnft_entries.get(&issuer_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|entry| entry.standard != ObjectStandard::MnftIssuer)
        {
            bail!(
                "mNFT issuer id collides with non-issuer object entry: issuer_id=0x{} block={}",
                hex::encode(&issuer_id),
                tx.block_number
            );
        }

        self.mnft_entries.insert(
            issuer_id,
            ObjectEntry {
                standard: ObjectStandard::MnftIssuer,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(owner_lock),
                name: issuer.name.clone(),
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
                extra: ObjectExtra::MnftIssuer {
                    class_count: issuer.class_count,
                    set_count: issuer.set_count,
                    info: issuer.info.clone(),
                },
            },
        );
        Ok(())
    }

    fn insert_mnft_class(
        &mut self,
        class: &crate::sync::bulk_build::facts::MnftClassProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        let class_id = class.class_id.clone();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.mnft_entries.get(&class_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|entry| entry.standard != ObjectStandard::MnftClass)
        {
            bail!(
                "mNFT class id collides with non-class object entry: class_id=0x{} block={}",
                hex::encode(&class_id),
                tx.block_number
            );
        }
        if let Some(existing_collection_id) = existing
            .as_ref()
            .and_then(|entry| entry.collection_id.as_ref())
        {
            if existing_collection_id.as_slice() != class.issuer_id {
                bail!(
                    "mNFT class issuer changed across writes: class_id=0x{} old_issuer=0x{} new_issuer=0x{} block={}",
                    hex::encode(&class_id),
                    hex::encode(existing_collection_id),
                    hex::encode(class.issuer_id),
                    tx.block_number
                );
            }
        }

        self.mnft_entries.insert(
            class_id.clone(),
            ObjectEntry {
                standard: ObjectStandard::MnftClass,
                collection_id: Some(class.issuer_id.to_vec()),
                token_id: None,
                owner_lock_hash: Some(owner_lock),
                name: class.name.clone(),
                description: class.description.clone(),
                is_live: true,
                created_at_block: existing
                    .as_ref()
                    .map(|entry| entry.created_at_block)
                    .unwrap_or(tx.block_number),
                created_at_tx: existing
                    .as_ref()
                    .map(|entry| entry.created_at_tx.clone())
                    .unwrap_or_else(|| tx.tx_hash.to_vec()),
                extra: ObjectExtra::MnftClass {
                    description: class.description.clone(),
                    renderer: class.renderer.clone(),
                    total: class.total,
                    issued: class.issued,
                    configure: class.configure,
                    composition_tier: crate::parser::media_source::analyze_renderer_tier(
                        class.renderer.as_deref(),
                    ),
                },
            },
        );

        let new_tier =
            crate::parser::media_source::analyze_renderer_tier(class.renderer.as_deref());
        let old_tier = existing.as_ref().and_then(|e| match &e.extra {
            ObjectExtra::MnftClass {
                composition_tier, ..
            } => Some(*composition_tier),
            _ => None,
        });

        let agg = self
            .object_collection_aggs
            .entry(class_id.clone())
            .or_default();
        agg.name = class.name.clone();
        agg.standard = ObjectStandard::MnftClass;
        // If renderer changed, recompute tier counts: shift all live tokens from old tier to new
        if let Some(old) = old_tier {
            if old != new_tier && agg.live_count > 0 {
                let count = agg.live_count;
                Self::adjust_object_collection_tier_count(agg, old, -count, &class_id, &class_id)?;
                Self::adjust_object_collection_tier_count(
                    agg, new_tier, count, &class_id, &class_id,
                )?;
            }
        }

        let output_index = Self::as_i16_outpoint_index(&cell.outpoint, "mNFT class")?;
        self.mnft_class_outpoints.insert(
            keys::encode_mnft_class_outpoint_key(&cell.outpoint.tx_hash, output_index).to_vec(),
            class_id,
        );
        Ok(())
    }

    fn insert_mnft_token(
        &mut self,
        token: &crate::sync::bulk_build::facts::MnftTokenProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
        consumed_in_same_tx: bool,
    ) -> Result<()> {
        let token_id = token.token_id.clone();
        let class_id = token.class_id.clone();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.mnft_entries.get(&token_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|entry| entry.standard != ObjectStandard::MnftToken)
        {
            bail!(
                "mNFT token id collides with non-token object entry: token_id=0x{} block={}",
                hex::encode(&token_id),
                tx.block_number
            );
        }
        if let Some(existing_collection_id) = existing
            .as_ref()
            .and_then(|entry| entry.collection_id.as_ref())
        {
            if existing_collection_id != &class_id {
                bail!(
                    "mNFT token class changed across writes: token_id=0x{} old_class=0x{} new_class=0x{} block={}",
                    hex::encode(&token_id),
                    hex::encode(existing_collection_id),
                    hex::encode(&class_id),
                    tx.block_number
                );
            }
        }

        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };
        if was_live && old_owner.is_none() {
            bail!(
                "mNFT live token missing owner_lock_hash during transfer: class_id=0x{}, token_id=0x{}",
                hex::encode(&class_id),
                hex::encode(&token_id)
            );
        }

        self.mnft_entries.insert(
            token_id.clone(),
            ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(token_id.clone()),
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
                extra: ObjectExtra::MnftToken {
                    token_index: token.token_index,
                    characteristic: token.characteristic.clone(),
                    configure: token.configure,
                    state: token.state,
                },
            },
        );

        self.mnft_by_collection
            .insert(keys::encode_nft_by_collection_key(&class_id, &token_id));

        let token_tier = self.resolve_mnft_token_tier(&class_id);
        let agg = self
            .object_collection_aggs
            .entry(class_id.clone())
            .or_default();
        agg.standard = ObjectStandard::MnftClass;
        if existing.is_none() {
            agg.total_count = checked_next_i64(
                agg.total_count,
                1,
                "mnft collection total_count insert",
                &token_id,
                tx.block_number,
            )?;
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                "mnft collection live_count insert",
                &token_id,
                tx.block_number,
            )?;
            Self::adjust_object_collection_tier_count(agg, token_tier, 1, &class_id, &token_id)?;
            Self::apply_mnft_owner_transition(
                &mut self.mnft_owner_counts,
                &class_id,
                None,
                Some(owner_lock.as_slice()),
                agg,
            )?;
        } else if !was_live {
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                "mnft collection live_count reactivate",
                &token_id,
                tx.block_number,
            )?;
            Self::adjust_object_collection_tier_count(agg, token_tier, 1, &class_id, &token_id)?;
            Self::apply_mnft_owner_transition(
                &mut self.mnft_owner_counts,
                &class_id,
                None,
                Some(owner_lock.as_slice()),
                agg,
            )?;
            if consumed_in_same_tx {
                self.increment_mnft_hourly_transfer(
                    &class_id,
                    &token_id,
                    tx.timestamp_ms,
                    tx.block_number,
                )?;
            }
        } else {
            Self::apply_mnft_owner_transition(
                &mut self.mnft_owner_counts,
                &class_id,
                old_owner.as_deref(),
                Some(owner_lock.as_slice()),
                agg,
            )?;
            if old_owner.as_deref() != Some(owner_lock.as_slice()) {
                self.increment_mnft_hourly_transfer(
                    &class_id,
                    &token_id,
                    tx.timestamp_ms,
                    tx.block_number,
                )?;
            }
        }

        let type_script_hash = cell
            .type_script_hash_id
            .map(|id| ctx.resolve_identity(id).to_vec())
            .ok_or_else(|| {
                anyhow!(
                    "missing type_script_hash for mNFT token output: token_id=0x{}, outpoint=0x{}:{}",
                    hex::encode(&token_id),
                    hex::encode(cell.outpoint.tx_hash),
                    cell.outpoint.index
                )
            })?;
        self.insert_mnft_type_index(type_script_hash, &class_id, &token_id, tx.block_number)?;

        let output_index = Self::as_i16_outpoint_index(&cell.outpoint, "mNFT token")?;
        self.mnft_token_outpoints.insert(
            keys::encode_mnft_token_outpoint_key(&cell.outpoint.tx_hash, output_index).to_vec(),
            token_id,
        );
        Ok(())
    }

    fn insert_dotbit(
        &mut self,
        dotbit: &crate::sync::bulk_build::facts::DotbitProtocolFacts,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
        consumed_in_same_tx_owner: Option<&[u8]>,
    ) -> Result<()> {
        let account_id = dotbit.account_id.to_vec();
        let owner_lock = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
        let existing = self.identities.get(&account_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|entry| entry.standard != IdentityStandard::DotBit)
        {
            bail!(
                ".bit account id collides with non-dotbit identity entry: account_id=0x{} block={}",
                hex::encode(&account_id),
                tx.block_number
            );
        }

        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };
        if was_live && old_owner.is_none() {
            bail!(
                ".bit live identity missing owner_lock_hash during transfer: account_id=0x{} block={}",
                hex::encode(&account_id),
                tx.block_number
            );
        }

        let account_name = dotbit
            .account
            .clone()
            .unwrap_or_else(|| format!("0x{}", hex::encode(&account_id)));

        self.identities.insert(
            account_id.clone(),
            IdentityEntry {
                standard: IdentityStandard::DotBit,
                owner_lock_hash: Some(owner_lock.clone()),
                name: Some(account_name),
                is_live: true,
                created_at_block: existing
                    .as_ref()
                    .map(|entry| entry.created_at_block)
                    .unwrap_or(tx.block_number),
                created_at_tx: existing
                    .as_ref()
                    .map(|entry| entry.created_at_tx.clone())
                    .unwrap_or_else(|| tx.tx_hash.to_vec()),
                extra: IdentityExtra::DotBit {
                    expired_at: dotbit.expired_at,
                    registered_at: dotbit.registered_at,
                    status: dotbit.status,
                },
            },
        );

        if existing.is_none() {
            self.identity_by_collection.insert(
                keys::encode_identity_by_collection_key(&DOTBIT_SENTINEL_COLLECTION, &account_id)
                    .to_vec(),
            );
        }

        let dotbit_owner_counts = &mut self.dotbit_owner_counts;
        let agg = self
            .dotbit_agg
            .get_or_insert_with(|| IdentityCollectionAggregate {
                name: Some(".bit".to_string()),
                standard: IdentityStandard::DotBit,
                ..IdentityCollectionAggregate::default()
            });
        if existing.is_none() {
            agg.total_count = checked_next_i64(
                agg.total_count,
                1,
                ".bit total_count",
                &account_id,
                tx.block_number,
            )?;
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                ".bit live_count",
                &account_id,
                tx.block_number,
            )?;
        } else if !was_live {
            agg.live_count = checked_next_i64(
                agg.live_count,
                1,
                ".bit live_count reactivate",
                &account_id,
                tx.block_number,
            )?;
        }

        Self::apply_dotbit_owner_transition(
            dotbit_owner_counts,
            old_owner.as_deref(),
            Some(owner_lock.as_slice()),
            agg,
        )?;
        let transfer_from_owner = if was_live {
            old_owner.as_deref()
        } else {
            consumed_in_same_tx_owner
        };
        if transfer_from_owner.is_some() && transfer_from_owner != Some(owner_lock.as_slice()) {
            self.increment_dotbit_hourly_transfer(&account_id, tx.timestamp_ms, tx.block_number)?;
        }
        self.insert_dotbit_outpoint_rows(&account_id, cell)?;
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
            bail!(
                "did:ckb identity already consumed: did_id=0x{}",
                hex::encode(did_id)
            );
        }
        let old_owner = entry.owner_lock_hash.clone();
        entry.is_live = false;
        entry.owner_lock_hash = None;

        let did_owner_counts = &mut self.did_owner_counts;
        let agg = self
            .did_agg
            .get_or_insert_with(|| IdentityCollectionAggregate {
                name: Some("did:ckb".to_string()),
                standard: IdentityStandard::DidCkb,
                ..IdentityCollectionAggregate::default()
            });
        agg.live_count =
            checked_next_i64(agg.live_count, -1, "did:ckb live_count consume", did_id, 0)?;
        Self::apply_did_owner_transition(did_owner_counts, old_owner.as_deref(), None, agg)
    }

    fn consume_spore(&mut self, spore_id: &[u8]) -> Result<()> {
        let entry = self.spore_entries.get_mut(spore_id).ok_or_else(|| {
            anyhow!(
                "missing spore during consume: spore_id=0x{}",
                hex::encode(spore_id)
            )
        })?;
        if entry.standard != ObjectStandard::Spore {
            bail!(
                "consume_spore expected spore entry, found {:?}: spore_id=0x{}",
                entry.standard,
                hex::encode(spore_id)
            );
        }
        if !entry.is_live {
            bail!(
                "spore already consumed: spore_id=0x{}",
                hex::encode(spore_id)
            );
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

        let agg = self
            .cluster_aggs
            .get_mut(cluster_id.as_slice())
            .ok_or_else(|| {
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

    fn consume_cluster(&mut self, cluster_id: &[u8; 32]) -> Result<()> {
        let entry = self
            .spore_entries
            .get_mut(cluster_id.as_slice())
            .ok_or_else(|| {
                anyhow!(
                    "missing cluster during consume: cluster_id=0x{}",
                    hex::encode(cluster_id)
                )
            })?;
        if entry.standard != ObjectStandard::SporeCluster {
            bail!(
                "consume_cluster expected cluster entry, found {:?}: cluster_id=0x{}",
                entry.standard,
                hex::encode(cluster_id)
            );
        }
        if !entry.is_live {
            bail!(
                "cluster already consumed: cluster_id=0x{}",
                hex::encode(cluster_id)
            );
        }

        entry.is_live = false;
        entry.owner_lock_hash = None;
        Ok(())
    }

    fn consume_mnft_issuer(&mut self, issuer_id: &[u8; 20]) -> Result<()> {
        let entry = self
            .mnft_entries
            .get_mut(issuer_id.as_slice())
            .ok_or_else(|| {
                anyhow!(
                    "missing mNFT issuer during consume: issuer_id=0x{}",
                    hex::encode(issuer_id)
                )
            })?;
        if entry.standard != ObjectStandard::MnftIssuer {
            bail!(
                "consume_mnft_issuer expected issuer entry, found {:?}: issuer_id=0x{}",
                entry.standard,
                hex::encode(issuer_id)
            );
        }
        if !entry.is_live {
            bail!(
                "mNFT issuer already consumed: issuer_id=0x{}",
                hex::encode(issuer_id)
            );
        }
        entry.is_live = false;
        entry.owner_lock_hash = None;
        Ok(())
    }

    fn consume_mnft_class(&mut self, class_id: &[u8]) -> Result<()> {
        let entry = self.mnft_entries.get_mut(class_id).ok_or_else(|| {
            anyhow!(
                "missing mNFT class during consume: class_id=0x{}",
                hex::encode(class_id)
            )
        })?;
        if entry.standard != ObjectStandard::MnftClass {
            bail!(
                "consume_mnft_class expected class entry, found {:?}: class_id=0x{}",
                entry.standard,
                hex::encode(class_id)
            );
        }
        if !entry.is_live {
            bail!(
                "mNFT class already consumed: class_id=0x{}",
                hex::encode(class_id)
            );
        }
        entry.is_live = false;
        entry.owner_lock_hash = None;
        Ok(())
    }

    fn consume_mnft_token(&mut self, token_id: &[u8]) -> Result<()> {
        let entry = self.mnft_entries.get_mut(token_id).ok_or_else(|| {
            anyhow!(
                "missing mNFT token during consume: token_id=0x{}",
                hex::encode(token_id)
            )
        })?;
        if entry.standard != ObjectStandard::MnftToken {
            bail!(
                "consume_mnft_token expected token entry, found {:?}: token_id=0x{}",
                entry.standard,
                hex::encode(token_id)
            );
        }
        if !entry.is_live {
            bail!(
                "mNFT token already consumed: token_id=0x{}",
                hex::encode(token_id)
            );
        }
        let old_owner = entry.owner_lock_hash.clone().ok_or_else(|| {
            anyhow!(
                "mNFT live token missing owner_lock_hash during consume: token_id=0x{}",
                hex::encode(token_id)
            )
        })?;
        let class_id = entry.collection_id.clone().ok_or_else(|| {
            anyhow!(
                "mNFT token missing class_id during consume: token_id=0x{}",
                hex::encode(token_id)
            )
        })?;
        entry.is_live = false;
        entry.owner_lock_hash = None;

        let token_tier = self.resolve_mnft_token_tier(&class_id);
        let agg = self
            .object_collection_aggs
            .get_mut(class_id.as_slice())
            .ok_or_else(|| {
                anyhow!(
                    "mNFT collection aggregate missing during consume: class_id=0x{}, token_id=0x{}",
                    hex::encode(&class_id),
                    hex::encode(token_id)
                )
            })?;
        agg.live_count = checked_next_i64(
            agg.live_count,
            -1,
            "mnft collection live_count consume",
            token_id,
            0,
        )?;
        Self::adjust_object_collection_tier_count(agg, token_tier, -1, &class_id, token_id)?;
        Self::apply_mnft_owner_transition(
            &mut self.mnft_owner_counts,
            &class_id,
            Some(old_owner.as_slice()),
            None,
            agg,
        )
    }

    fn consume_dotbit(&mut self, account_id: &[u8; 20]) -> Result<()> {
        let entry = self
            .identities
            .get_mut(account_id.as_slice())
            .ok_or_else(|| {
                anyhow!(
                    "missing .bit account during consume: account_id=0x{}",
                    hex::encode(account_id)
                )
            })?;
        if entry.standard != IdentityStandard::DotBit {
            bail!(
                "consume_dotbit expected dotbit identity entry, found {:?}: account_id=0x{}",
                entry.standard,
                hex::encode(account_id)
            );
        }
        if !entry.is_live {
            bail!(
                ".bit account already consumed: account_id=0x{}",
                hex::encode(account_id)
            );
        }
        let old_owner = entry.owner_lock_hash.clone().ok_or_else(|| {
            anyhow!(
                ".bit live account missing owner_lock_hash during consume: account_id=0x{}",
                hex::encode(account_id)
            )
        })?;
        entry.is_live = false;
        entry.owner_lock_hash = None;

        let dotbit_owner_counts = &mut self.dotbit_owner_counts;
        let agg = self
            .dotbit_agg
            .get_or_insert_with(|| IdentityCollectionAggregate {
                name: Some(".bit".to_string()),
                standard: IdentityStandard::DotBit,
                ..IdentityCollectionAggregate::default()
            });
        agg.live_count =
            checked_next_i64(agg.live_count, -1, ".bit live_count consume", account_id, 0)?;
        Self::apply_dotbit_owner_transition(
            dotbit_owner_counts,
            Some(old_owner.as_slice()),
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

    fn apply_dotbit_owner_transition(
        dotbit_owner_counts: &mut BTreeMap<Vec<u8>, i64>,
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut IdentityCollectionAggregate,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_owner) = old_owner {
            let current = *dotbit_owner_counts.get(old_owner).unwrap_or(&0);
            if current <= 0 {
                bail!(
                    ".bit owner count underflow: lock_hash=0x{}, current={}",
                    hex::encode(old_owner),
                    current
                );
            }
            if current == 1 {
                dotbit_owner_counts.remove(old_owner);
                agg.holders_count = checked_next_i64(
                    agg.holders_count,
                    -1,
                    ".bit holders_count remove",
                    old_owner,
                    0,
                )?;
            } else {
                dotbit_owner_counts.insert(old_owner.to_vec(), current - 1);
            }
        }

        if let Some(new_owner) = new_owner {
            let current = *dotbit_owner_counts.get(new_owner).unwrap_or(&0);
            if current == 0 {
                agg.holders_count =
                    checked_next_i64(agg.holders_count, 1, ".bit holders_count add", new_owner, 0)?;
            }
            dotbit_owner_counts.insert(new_owner.to_vec(), current + 1);
        }

        Ok(())
    }

    fn apply_mnft_owner_transition(
        mnft_owner_counts: &mut BTreeMap<(Vec<u8>, Vec<u8>), i64>,
        class_id: &[u8],
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut ObjectCollectionAggregate,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_owner) = old_owner {
            let key = (class_id.to_vec(), old_owner.to_vec());
            let current = *mnft_owner_counts.get(&key).unwrap_or(&0);
            if current <= 0 {
                bail!(
                    "mnft owner count underflow: class_id=0x{}, lock_hash=0x{}, current={}",
                    hex::encode(class_id),
                    hex::encode(old_owner),
                    current
                );
            }
            if current == 1 {
                mnft_owner_counts.remove(&key);
                agg.holders_count = checked_next_i64(
                    agg.holders_count,
                    -1,
                    "mnft holders_count remove",
                    class_id,
                    0,
                )?;
            } else {
                mnft_owner_counts.insert(key, current - 1);
            }
        }

        if let Some(new_owner) = new_owner {
            let key = (class_id.to_vec(), new_owner.to_vec());
            let current = *mnft_owner_counts.get(&key).unwrap_or(&0);
            if current == 0 {
                agg.holders_count =
                    checked_next_i64(agg.holders_count, 1, "mnft holders_count add", class_id, 0)?;
            }
            let next = checked_next_i64(current, 1, "mnft owner count add", class_id, 0)?;
            mnft_owner_counts.insert(key, next);
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
                agg.owner_count =
                    checked_next_i64(agg.owner_count, 1, "cluster owner_count add", cluster_id, 0)?;
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

    fn insert_mnft_type_index(
        &mut self,
        type_hash: Vec<u8>,
        class_id: &[u8],
        token_id: &[u8],
        block_number: i64,
    ) -> Result<()> {
        if let Some(existing) = self.mnft_type_indexes.get(type_hash.as_slice()) {
            if existing.collection_id != class_id {
                bail!(
                    "mNFT type index changed collection_id: type_hash=0x{}, old_class=0x{}, new_class=0x{}, token_id=0x{}, block={}",
                    hex::encode(&type_hash),
                    hex::encode(&existing.collection_id),
                    hex::encode(class_id),
                    hex::encode(token_id),
                    block_number
                );
            }
        } else {
            self.mnft_type_indexes.insert(
                type_hash,
                ObjectTypeIndex {
                    collection_id: class_id.to_vec(),
                },
            );
        }
        Ok(())
    }

    fn increment_mnft_hourly_transfer(
        &mut self,
        class_id: &[u8],
        token_id: &[u8],
        timestamp_ms: i64,
        block_number: i64,
    ) -> Result<()> {
        let hour_bucket = timestamp_ms / 3_600_000;
        let key = keys::encode_nft_hourly_key(class_id, hour_bucket).to_vec();
        let current = *self.mnft_hourly_transfers.get(key.as_slice()).unwrap_or(&0);
        let next = checked_next_i64(current, 1, "mnft hourly transfer", token_id, block_number)?;
        self.mnft_hourly_transfers.insert(key, next);
        Ok(())
    }

    fn increment_dotbit_hourly_transfer(
        &mut self,
        account_id: &[u8],
        timestamp_ms: i64,
        block_number: i64,
    ) -> Result<()> {
        let hour_bucket = timestamp_ms / 3_600_000;
        let key = keys::encode_nft_hourly_key(&DOTBIT_SENTINEL_COLLECTION, hour_bucket).to_vec();
        let current = *self
            .dotbit_hourly_transfers
            .get(key.as_slice())
            .unwrap_or(&0);
        let next = checked_next_i64(current, 1, ".bit hourly transfer", account_id, block_number)?;
        self.dotbit_hourly_transfers.insert(key, next);
        Ok(())
    }

    fn as_i16_outpoint_index(
        outpoint: &crate::sync::bulk_build::facts::OutPointKey,
        label: &str,
    ) -> Result<i16> {
        i16::try_from(outpoint.index).map_err(|_| {
            anyhow!(
                "{} outpoint index exceeds i16: outpoint=0x{}:{}",
                label,
                hex::encode(outpoint.tx_hash),
                outpoint.index
            )
        })
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

    fn insert_dotbit_outpoint_rows(&mut self, account_id: &[u8], cell: &CellFacts) -> Result<()> {
        let output_index = Self::as_i16_outpoint_index(&cell.outpoint, ".bit")?;
        self.dotbit_outpoints.insert(
            keys::encode_dotbit_account_outpoint_key(&cell.outpoint.tx_hash, output_index).to_vec(),
            account_id.to_vec(),
        );
        self.dotbit_outpoints_by_account.insert(
            keys::encode_dotbit_outpoint_by_account_id_key(
                account_id,
                &cell.outpoint.tx_hash,
                output_index,
            )
            .to_vec(),
        );
        Ok(())
    }

    fn spore_media_tier(entry: &ObjectEntry) -> CompositionTier {
        match &entry.extra {
            ObjectExtra::Spore { media_profile, .. } => media_profile.tier,
            _ => CompositionTier::Unknown,
        }
    }

    fn adjust_cluster_tier_count(
        agg: &mut ClusterAggregate,
        tier: CompositionTier,
        delta: i64,
        cluster_id: &[u8],
        spore_id: &[u8],
    ) -> Result<()> {
        let slot = match tier {
            CompositionTier::PureCkb => &mut agg.pure_ckb_count,
            CompositionTier::BtcCkb => &mut agg.btc_ckb_count,
            CompositionTier::DecentralizedMixture => &mut agg.decentralized_mixture_count,
            CompositionTier::CentralizedMixture => &mut agg.centralized_mixture_count,
            CompositionTier::Unknown => &mut agg.unknown_count,
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

    fn mnft_class_tier(entry: &ObjectEntry) -> CompositionTier {
        match &entry.extra {
            ObjectExtra::MnftClass {
                composition_tier, ..
            } => *composition_tier,
            _ => CompositionTier::Unknown,
        }
    }

    fn resolve_mnft_token_tier(&self, class_id: &[u8]) -> CompositionTier {
        self.mnft_entries
            .get(class_id)
            .map(Self::mnft_class_tier)
            .unwrap_or(CompositionTier::Unknown)
    }

    fn adjust_object_collection_tier_count(
        agg: &mut ObjectCollectionAggregate,
        tier: CompositionTier,
        delta: i64,
        collection_id: &[u8],
        token_id: &[u8],
    ) -> Result<()> {
        let slot = match tier {
            CompositionTier::PureCkb => &mut agg.pure_ckb_count,
            CompositionTier::BtcCkb => &mut agg.btc_ckb_count,
            CompositionTier::DecentralizedMixture => &mut agg.decentralized_mixture_count,
            CompositionTier::CentralizedMixture => &mut agg.centralized_mixture_count,
            CompositionTier::Unknown => &mut agg.unknown_count,
        };
        let next = slot.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "mnft collection tier count overflow: collection_id=0x{}, token_id=0x{}, tier={}, current={}, delta={}",
                hex::encode(collection_id),
                hex::encode(token_id),
                tier.as_str(),
                *slot,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "mnft collection tier count underflow: collection_id=0x{}, token_id=0x{}, tier={}, current={}, delta={}",
                hex::encode(collection_id),
                hex::encode(token_id),
                tier.as_str(),
                *slot,
                delta
            );
        }
        *slot = next;
        Ok(())
    }
}

/// Classify a cell's protocol facts into an NFT collection ID for daily delta tracking.
/// Mirrors `classify_nft_collection_id` from `dao_helpers.rs` but operates on parsed protocol
/// facts rather than raw code_hash/type_args.
fn classify_nft_collection_from_protocol(
    protocol_facts: &Option<CellProtocolFacts>,
) -> Option<Vec<u8>> {
    match protocol_facts.as_ref()? {
        CellProtocolFacts::MnftToken(token) => Some(token.class_id.clone()),
        CellProtocolFacts::Dotbit(_) => Some(DOTBIT_SENTINEL_COLLECTION.to_vec()),
        CellProtocolFacts::Spore(spore) if spore.is_did => {
            Some(DID_CKB_SENTINEL_COLLECTION.to_vec())
        }
        _ => None,
    }
}

fn checked_next_i64(
    current: i64,
    delta: i64,
    label: &str,
    key: &[u8],
    block_number: i64,
) -> Result<i64> {
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
    pub objects: HashMap<Vec<u8>, ObjectEntry>,
    pub identities: HashMap<Vec<u8>, IdentityEntry>,
    pub cluster_aggs: HashMap<Vec<u8>, ClusterAggregate>,
    pub object_collection_aggs: HashMap<Vec<u8>, ObjectCollectionAggregate>,
    pub did_agg: Option<IdentityCollectionAggregate>,
    pub identities_by_collection: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    pub spores_by_cluster: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    pub objects_by_collection: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    pub did_owner_counts: HashMap<Vec<u8>, i64>,
    pub cluster_owner_counts: HashMap<Vec<u8>, HashMap<Vec<u8>, i64>>,
    pub object_owner_counts: HashMap<Vec<u8>, HashMap<Vec<u8>, i64>>,
    pub spore_outpoints: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>>,
    pub spore_type_indexes: HashMap<Vec<u8>, SporeTypeIndex>,
    pub object_type_indexes: HashMap<Vec<u8>, ObjectTypeIndex>,
    pub mnft_class_outpoints: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>>,
    pub mnft_token_outpoints: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>>,
    pub object_hourly_transfers: HashMap<Vec<u8>, HashMap<i64, i64>>,
    pub spore_daily_deltas: HashMap<Vec<u8>, HashMap<u32, SporeDailyDelta>>,
    pub cluster_daily_deltas: HashMap<Vec<u8>, HashMap<u32, ClusterDailyDelta>>,
}

#[doc(hidden)]
pub(crate) fn materialize_object_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<ObjectStateSnapshot> {
    let interner = IdentityInterner::default();
    let (arena, _) = build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let frozen = interner.snapshot_for_reads();
    let ctx = ReducerContext::new(&frozen);
    let mut owner = ObjectOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = super::super::unique_temp_test_dir("bulk-build-object-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.flush_sealed(&mut materializer)?;
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();

        let spores = domain_store
            .list_spores(usize::MAX)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let objects = domain_store
            .list_objects(usize::MAX)?
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
        let object_collection_aggs = domain_store
            .list_object_collection_aggregates()?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let did_agg =
            domain_store.get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)?;

        let mut identities_by_collection = HashMap::new();
        let mut did_ids = domain_store.list_identity_ids_by_collection(
            &DID_CKB_SENTINEL_COLLECTION,
            None,
            usize::MAX,
        )?;
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

        let mut objects_by_collection = HashMap::new();
        let mut object_owner_counts = HashMap::new();
        let mut object_hourly_transfers = HashMap::new();
        for collection_id in object_collection_aggs.keys() {
            let mut members =
                domain_store.list_object_ids_by_collection(collection_id, None, usize::MAX)?;
            members.sort();
            if !members.is_empty() {
                objects_by_collection.insert(collection_id.clone(), members);
            }

            let owners = domain_store
                .list_object_collection_owner_counts(collection_id)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            if !owners.is_empty() {
                object_owner_counts.insert(collection_id.clone(), owners);
            }

            let prefix = keys::encode_nft_hourly_prefix(collection_id);
            let iter = domain_store.prefix_iterator_cf(domain_store.cf_stats_object(), &prefix);
            let mut hourly = HashMap::new();
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow!(
                        "failed to iterate stats_object hourly rows in object snapshot helper: collection_id=0x{}, error={}",
                        hex::encode(collection_id),
                        e
                    )
                })?;
                if !key.starts_with(prefix.as_slice()) {
                    break;
                }
                if key.len() != 41 {
                    bail!(
                        "invalid mNFT hourly transfer key length in object snapshot helper: collection_id=0x{}, len={}",
                        hex::encode(collection_id),
                        key.len()
                    );
                }
                if value.len() != 8 {
                    bail!(
                        "invalid mNFT hourly transfer value length in object snapshot helper: collection_id=0x{}, len={}",
                        hex::encode(collection_id),
                        value.len()
                    );
                }
                let hour_bucket = i64::from_be_bytes(
                    key[33..41]
                        .try_into()
                        .expect("hour bucket slice length must be 8"),
                );
                let count = i64::from_le_bytes(
                    value[..8]
                        .try_into()
                        .expect("hourly transfer value length must be 8"),
                );
                hourly.insert(hour_bucket, count);
            }
            if !hourly.is_empty() {
                object_hourly_transfers.insert(collection_id.clone(), hourly);
            }
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

        let mut spore_daily_deltas = HashMap::new();
        for (spore_id, entry) in &spores {
            if entry.standard != ObjectStandard::Spore {
                continue;
            }
            let rows = domain_store
                .list_spore_daily_deltas(spore_id)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            if !rows.is_empty() {
                spore_daily_deltas.insert(spore_id.clone(), rows);
            }
        }

        let mut cluster_daily_deltas = HashMap::new();
        let mut cluster_daily_ids = cluster_aggs.keys().cloned().collect::<Vec<_>>();
        if spores
            .values()
            .any(|entry| entry.standard == ObjectStandard::Spore && entry.collection_id.is_none())
        {
            cluster_daily_ids.push(SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
        }
        for cluster_id in cluster_daily_ids {
            let rows = domain_store
                .list_cluster_daily_deltas(&cluster_id)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            if !rows.is_empty() {
                cluster_daily_deltas.insert(cluster_id, rows);
            }
        }

        let mut spore_type_indexes = HashMap::new();
        let mut object_type_indexes = HashMap::new();
        let mut mnft_class_outpoints: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>> = HashMap::new();
        let mut mnft_token_outpoints: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>> = HashMap::new();
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

        for (object_id, entry) in &objects {
            if entry.standard != ObjectStandard::MnftToken {
                continue;
            }
            let mut outpoints = domain_store.list_mnft_token_outpoints_by_token_id(object_id)?;
            outpoints.sort();
            if !outpoints.is_empty() {
                mnft_token_outpoints.insert(object_id.clone(), outpoints);
            }
        }

        let mut stats_object_iter =
            domain_store.iterator_cf(domain_store.cf_stats_object(), IteratorMode::Start);
        for item in &mut stats_object_iter {
            let (key, value) = item?;
            match key.first().copied() {
                Some(keys::STATS_PREFIX_NFT_TYPE_INDEX) => {
                    if key.len() != keys::NFT_TYPE_INDEX_KEY_SIZE {
                        bail!(
                            "invalid object type index key length in object snapshot helper: len={}",
                            key.len()
                        );
                    }
                    let type_hash = key[1..33].to_vec();
                    let index: ObjectTypeIndex =
                        bincode::deserialize(&value).map_err(|e| {
                            anyhow!(
                                "failed to deserialize ObjectTypeIndex in object snapshot helper: type_hash=0x{}, error={}",
                                hex::encode(&type_hash),
                                e
                            )
                        })?;
                    object_type_indexes.insert(type_hash, index);
                }
                Some(keys::STATS_PREFIX_MNFT_CLASS_OUTPOINT) => {
                    if key.len() != keys::MNFT_CLASS_OUTPOINT_KEY_SIZE {
                        bail!(
                            "invalid mNFT class outpoint key length in object snapshot helper: len={}",
                            key.len()
                        );
                    }
                    if value.is_empty() {
                        bail!("empty mNFT class outpoint value in object snapshot helper");
                    }
                    let outpoint = keys::decode_outpoint(&key[1..35]);
                    mnft_class_outpoints
                        .entry(value.to_vec())
                        .or_default()
                        .push(outpoint);
                }
                _ => {}
            }
        }
        for outpoints in mnft_class_outpoints.values_mut() {
            outpoints.sort();
        }

        ObjectStateSnapshot {
            spores,
            objects,
            identities,
            cluster_aggs,
            object_collection_aggs,
            did_agg,
            identities_by_collection,
            spores_by_cluster,
            objects_by_collection,
            did_owner_counts,
            cluster_owner_counts,
            object_owner_counts,
            spore_outpoints,
            spore_type_indexes,
            object_type_indexes,
            mnft_class_outpoints,
            mnft_token_outpoints,
            object_hourly_transfers,
            spore_daily_deltas,
            cluster_daily_deltas,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{
        CellFacts, CellProtocolFacts, CellSemanticTag, ClusterProtocolFacts, DotbitProtocolFacts,
        MnftClassProtocolFacts, MnftIssuerProtocolFacts, MnftTokenProtocolFacts, OutPointKey,
        ResolvedInputFacts, ResolvedTxFacts, SporeProtocolFacts,
    };
    use crate::sync::bulk_build::unique_temp_test_dir;
    use crate::sync::types::InternId;

    macro_rules! cell_facts {
        ($($body:tt)*) => {
            CellFacts {
                created_by_block_dao_ar: 0,
                $($body)*
            }
        };
    }

    macro_rules! resolved_input_facts {
        ($($body:tt)*) => {
            ResolvedInputFacts {
                created_by_block_dao_ar: 0,
                data_size: 0,
                data_hash: None,
                dao_compensation_ars: None,
                $($body)*
            }
        };
    }

    #[test]
    fn object_owner_materializes_spore_transfer_and_did_burn_without_db_reads() {
        let interner = crate::sync::bulk_build::interner::IdentityInterner::default();
        let lock_a = interner.intern_bytes(vec![0xaa; 32]);
        let lock_b = interner.intern_bytes(vec![0xbb; 32]);
        let lock_c = interner.intern_bytes(vec![0xcc; 32]);
        let spore_type_hash = interner.intern_bytes(vec![0x91; 32]);
        let did_type_hash = interner.intern_bytes(vec![0x92; 32]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

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
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![cell_facts! {
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
            }]
            .into(),
        };

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x02; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 0,
            tx_index: 1,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                cell_facts! {
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
                cell_facts! {
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
            ]
            .into(),
        };

        let tx2 = ResolvedTxFacts {
            tx_hash: [0x03; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_002,
            block_dao_ar: 0,
            tx_index: 2,
            dotbit_action: None,
            resolved_inputs: vec![resolved_input_facts! {
                outpoint: OutPointKey::new([0x02; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
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
            cells: vec![cell_facts! {
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
            }]
            .into(),
        };

        let tx3 = ResolvedTxFacts {
            tx_hash: [0x04; 32],
            block_number: 100,
            block_hash: [0x80; 32],
            timestamp_ms: 1_700_000_000_003,
            block_dao_ar: 0,
            tx_index: 3,
            dotbit_action: None,
            resolved_inputs: vec![resolved_input_facts! {
                outpoint: OutPointKey::new([0x02; 32], 1),
                created_at_block: 100,
                capacity: 150_00000000,
                occupied_capacity: 61_00000000,
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
            cells: Vec::new().into(),
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
            .flush_sealed(&mut materializer)
            .expect("flush sealed object owner");
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
        assert_eq!(
            stored_cluster.description.as_deref(),
            Some("{\"dob\":{\"ver\":1}}")
        );

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

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn object_owner_materializes_mnft_transfer_and_consume_without_db_reads() {
        let interner = crate::sync::bulk_build::interner::IdentityInterner::default();
        let owner_a = interner.intern_bytes(vec![0xaa; 32]);
        let owner_b = interner.intern_bytes(vec![0xbb; 32]);
        let issuer_type_hash = interner.intern_bytes(vec![0x71; 32]);
        let class_type_hash = interner.intern_bytes(vec![0x72; 32]);
        let token_type_hash = interner.intern_bytes(vec![0x73; 32]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let issuer_id = [0x11; 20];
        let mut class_id = issuer_id.to_vec();
        class_id.extend_from_slice(&7u32.to_le_bytes());
        let mut token_id = class_id.clone();
        token_id.extend_from_slice(&9u32.to_le_bytes());

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x21; 32],
            block_number: 200,
            block_hash: [0x90; 32],
            timestamp_ms: 1_700_000_100_000,
            block_dao_ar: 0,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                cell_facts! {
                    outpoint: OutPointKey::new([0x21; 32], 0),
                    created_at_block: 200,
                    capacity: 250_00000000,
                    lock_script_hash_id: owner_a,
                    lock_code_hash_id: InternId::new(101),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(102),
                    type_script_hash_id: Some(issuer_type_hash),
                    type_code_hash_id: Some(InternId::new(103)),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(104)),
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Mnft,
                    dao_state: None,
                    protocol_facts: Some(CellProtocolFacts::MnftIssuer(MnftIssuerProtocolFacts {
                        issuer_id,
                        name: Some("Test Issuer".to_string()),
                        info: Some(b"{\"name\":\"Test Issuer\"}".to_vec()),
                        class_count: 1,
                        set_count: 0,
                    })),
                },
                cell_facts! {
                    outpoint: OutPointKey::new([0x21; 32], 1),
                    created_at_block: 200,
                    capacity: 260_00000000,
                    lock_script_hash_id: owner_a,
                    lock_code_hash_id: InternId::new(105),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(106),
                    type_script_hash_id: Some(class_type_hash),
                    type_code_hash_id: Some(InternId::new(107)),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(108)),
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Mnft,
                    dao_state: None,
                    protocol_facts: Some(CellProtocolFacts::MnftClass(MnftClassProtocolFacts {
                        class_id: class_id.clone(),
                        issuer_id,
                        name: Some("Genesis Class".to_string()),
                        description: Some("class description".to_string()),
                        renderer: Some("renderer".to_string()),
                        total: 100,
                        issued: 1,
                        configure: 3,
                    })),
                },
                cell_facts! {
                    outpoint: OutPointKey::new([0x21; 32], 2),
                    created_at_block: 200,
                    capacity: 270_00000000,
                    lock_script_hash_id: owner_a,
                    lock_code_hash_id: InternId::new(109),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(110),
                    type_script_hash_id: Some(token_type_hash),
                    type_code_hash_id: Some(InternId::new(111)),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(112)),
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Mnft,
                    dao_state: None,
                    protocol_facts: Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                        token_id: token_id.clone(),
                        class_id: class_id.clone(),
                        token_index: 9,
                        characteristic: vec![1, 2, 3, 4],
                        configure: 1,
                        state: 0,
                    })),
                },
            ]
            .into(),
        };

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x22; 32],
            block_number: 201,
            block_hash: [0x91; 32],
            timestamp_ms: 1_700_000_200_000,
            block_dao_ar: 0,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: vec![resolved_input_facts! {
                outpoint: OutPointKey::new([0x21; 32], 2),
                created_at_block: 200,
                capacity: 270_00000000,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                lock_script_hash_id: owner_a,
                lock_code_hash_id: InternId::new(109),
                lock_hash_type: 1,
                lock_args_id: InternId::new(110),
                type_script_hash_id: Some(token_type_hash),
                type_code_hash_id: Some(InternId::new(111)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(112)),
                semantic_tag: CellSemanticTag::Mnft,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id: token_id.clone(),
                    class_id: class_id.clone(),
                    token_index: 9,
                    characteristic: vec![1, 2, 3, 4],
                    configure: 1,
                    state: 0,
                })),
            }],
            cells: vec![cell_facts! {
                outpoint: OutPointKey::new([0x22; 32], 0),
                created_at_block: 201,
                capacity: 270_00000000,
                lock_script_hash_id: owner_b,
                lock_code_hash_id: InternId::new(113),
                lock_hash_type: 1,
                lock_args_id: InternId::new(114),
                type_script_hash_id: Some(token_type_hash),
                type_code_hash_id: Some(InternId::new(111)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(112)),
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Mnft,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id: token_id.clone(),
                    class_id: class_id.clone(),
                    token_index: 9,
                    characteristic: vec![1, 2, 3, 4],
                    configure: 1,
                    state: 0,
                })),
            }]
            .into(),
        };

        let tx2 = ResolvedTxFacts {
            tx_hash: [0x23; 32],
            block_number: 202,
            block_hash: [0x92; 32],
            timestamp_ms: 1_700_000_260_000,
            block_dao_ar: 0,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: vec![resolved_input_facts! {
                outpoint: OutPointKey::new([0x22; 32], 0),
                created_at_block: 201,
                capacity: 270_00000000,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                lock_script_hash_id: owner_b,
                lock_code_hash_id: InternId::new(113),
                lock_hash_type: 1,
                lock_args_id: InternId::new(114),
                type_script_hash_id: Some(token_type_hash),
                type_code_hash_id: Some(InternId::new(111)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(112)),
                semantic_tag: CellSemanticTag::Mnft,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id: token_id.clone(),
                    class_id: class_id.clone(),
                    token_index: 9,
                    characteristic: vec![1, 2, 3, 4],
                    configure: 1,
                    state: 0,
                })),
            }],
            cells: Vec::new().into(),
        };

        let mut owner = ObjectOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply create");
        owner.apply_tx(&tx1, &ctx).expect("apply transfer");
        owner.apply_tx(&tx2, &ctx).expect("apply consume");

        let root = unique_temp_test_dir("bulk-build-object-owner-mnft");
        std::fs::create_dir_all(&root).expect("root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("domain");
        std::fs::create_dir_all(&append_path).expect("append");

        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("domain store");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("append store");
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner
            .flush_sealed(&mut materializer)
            .expect("flush sealed object owner");
        owner
            .materialize_final(&mut materializer)
            .expect("materialize object owner");

        let issuer_entry = domain_store
            .get_object(&issuer_id)
            .expect("get issuer")
            .expect("issuer entry");
        assert_eq!(issuer_entry.standard, ObjectStandard::MnftIssuer);
        assert!(issuer_entry.is_live);
        assert_eq!(issuer_entry.owner_lock_hash, Some(vec![0xaa; 32]));

        let class_entry = domain_store
            .get_object(&class_id)
            .expect("get class")
            .expect("class entry");
        assert_eq!(class_entry.standard, ObjectStandard::MnftClass);
        assert!(class_entry.is_live);
        assert_eq!(class_entry.collection_id, Some(issuer_id.to_vec()));

        let token_entry = domain_store
            .get_object(&token_id)
            .expect("get token")
            .expect("token entry");
        assert_eq!(token_entry.standard, ObjectStandard::MnftToken);
        assert!(!token_entry.is_live);
        assert!(token_entry.owner_lock_hash.is_none());
        assert_eq!(token_entry.created_at_block, 200);
        assert_eq!(token_entry.created_at_tx, vec![0x21; 32]);

        let class_agg = domain_store
            .get_object_collection_aggregate(&class_id)
            .expect("class agg")
            .expect("class agg exists");
        assert_eq!(class_agg.standard, ObjectStandard::MnftClass);
        assert_eq!(class_agg.name.as_deref(), Some("Genesis Class"));
        assert_eq!(class_agg.total_count, 1);
        assert_eq!(class_agg.live_count, 0);
        assert_eq!(class_agg.holders_count, 0);

        let class_members = domain_store
            .list_object_ids_by_collection(&class_id, None, 10)
            .expect("class members");
        assert_eq!(class_members, vec![token_id.clone()]);

        assert_eq!(
            domain_store
                .get_object_collection_owner_count(&class_id, &[0xaa; 32])
                .expect("owner count a"),
            0
        );
        assert_eq!(
            domain_store
                .get_object_collection_owner_count(&class_id, &[0xbb; 32])
                .expect("owner count b"),
            0
        );

        let type_index = domain_store
            .get_object_type_index(&[0x73; 32])
            .expect("object type index")
            .expect("object type index exists");
        assert_eq!(type_index.collection_id, class_id);

        let create_outpoint = domain_store
            .get_mnft_class_id_by_outpoint(&[0x21; 32], 1)
            .expect("class outpoint")
            .expect("class outpoint exists");
        assert_eq!(create_outpoint, class_id);
        let first_token_outpoint = domain_store
            .get_mnft_token_id_by_outpoint(&[0x21; 32], 2)
            .expect("first token outpoint")
            .expect("first token outpoint exists");
        assert_eq!(first_token_outpoint, token_id);
        let second_token_outpoint = domain_store
            .get_mnft_token_id_by_outpoint(&[0x22; 32], 0)
            .expect("second token outpoint")
            .expect("second token outpoint exists");
        assert_eq!(second_token_outpoint, token_id);

        let token_outpoints = domain_store
            .list_mnft_token_outpoints_by_token_id(&token_id)
            .expect("token outpoints");
        assert_eq!(token_outpoints.len(), 2);

        let hour_bucket = tx1.timestamp_ms / 3_600_000;
        let hourly_key = keys::encode_nft_hourly_key(&class_id, hour_bucket);
        let hourly_value = domain_store
            .get_stats_key(&hourly_key)
            .expect("stats lookup")
            .expect("hourly stats");
        assert_eq!(
            i64::from_le_bytes(hourly_value[..8].try_into().expect("hourly bytes")),
            1
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn object_owner_materializes_dotbit_transfer_and_consume_without_db_reads() {
        let interner = crate::sync::bulk_build::interner::IdentityInterner::default();
        let owner_a = interner.intern_bytes(vec![0xa1; 32]);
        let owner_b = interner.intern_bytes(vec![0xb2; 32]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let account_id = [0x51; 20];

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x31; 32],
            block_number: 300,
            block_hash: [0xa0; 32],
            timestamp_ms: 1_700_100_000_000,
            block_dao_ar: 0,
            tx_index: 0,
            dotbit_action: Some("confirm_proposal".to_string()),
            resolved_inputs: Vec::new(),
            cells: vec![cell_facts! {
                outpoint: OutPointKey::new([0x31; 32], 0),
                created_at_block: 300,
                capacity: 200_00000000,
                lock_script_hash_id: owner_a,
                lock_code_hash_id: InternId::new(201),
                lock_hash_type: 1,
                lock_args_id: InternId::new(202),
                type_script_hash_id: Some(InternId::new(203)),
                type_code_hash_id: Some(InternId::new(204)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(205)),
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some("alice.bit".to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }]
            .into(),
        };

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x32; 32],
            block_number: 301,
            block_hash: [0xa1; 32],
            timestamp_ms: 1_700_100_360_000,
            block_dao_ar: 0,
            tx_index: 0,
            dotbit_action: Some("transfer_account".to_string()),
            resolved_inputs: vec![resolved_input_facts! {
                outpoint: OutPointKey::new([0x31; 32], 0),
                created_at_block: 300,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                lock_script_hash_id: owner_a,
                lock_code_hash_id: InternId::new(201),
                lock_hash_type: 1,
                lock_args_id: InternId::new(202),
                type_script_hash_id: Some(InternId::new(203)),
                type_code_hash_id: Some(InternId::new(204)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(205)),
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some("alice.bit".to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }],
            cells: vec![cell_facts! {
                outpoint: OutPointKey::new([0x32; 32], 0),
                created_at_block: 301,
                capacity: 200_00000000,
                lock_script_hash_id: owner_b,
                lock_code_hash_id: InternId::new(206),
                lock_hash_type: 1,
                lock_args_id: InternId::new(207),
                type_script_hash_id: Some(InternId::new(208)),
                type_code_hash_id: Some(InternId::new(209)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(210)),
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some("alice.bit".to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }]
            .into(),
        };

        let tx2 = ResolvedTxFacts {
            tx_hash: [0x33; 32],
            block_number: 302,
            block_hash: [0xa2; 32],
            timestamp_ms: 1_700_100_720_000,
            block_dao_ar: 0,
            tx_index: 0,
            dotbit_action: Some("recycle_expired_account".to_string()),
            resolved_inputs: vec![resolved_input_facts! {
                outpoint: OutPointKey::new([0x32; 32], 0),
                created_at_block: 301,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                lock_script_hash_id: owner_b,
                lock_code_hash_id: InternId::new(206),
                lock_hash_type: 1,
                lock_args_id: InternId::new(207),
                type_script_hash_id: Some(InternId::new(208)),
                type_code_hash_id: Some(InternId::new(209)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(210)),
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some("alice.bit".to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }],
            cells: Vec::new().into(),
        };

        let mut owner = ObjectOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply create");
        owner.apply_tx(&tx1, &ctx).expect("apply transfer");
        owner.apply_tx(&tx2, &ctx).expect("apply consume");
        owner
            .apply_identity_activity_count_deltas(&FxHashMap::from_iter([(
                ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION.to_vec(),
                1,
            )]))
            .expect("apply dotbit activity delta");

        let root = unique_temp_test_dir("bulk-build-object-owner-dotbit");
        std::fs::create_dir_all(&root).expect("root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("domain");
        std::fs::create_dir_all(&append_path).expect("append");

        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("domain store");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("append store");
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner
            .flush_sealed(&mut materializer)
            .expect("flush sealed object owner");
        owner
            .materialize_final(&mut materializer)
            .expect("materialize object owner");

        let entry = domain_store
            .get_identity(&account_id)
            .expect("get identity")
            .expect("identity exists");
        assert_eq!(entry.standard, IdentityStandard::DotBit);
        assert_eq!(entry.name.as_deref(), Some("alice.bit"));
        assert!(!entry.is_live);
        assert!(entry.owner_lock_hash.is_none());
        assert_eq!(entry.created_at_block, 300);
        assert_eq!(entry.created_at_tx, vec![0x31; 32]);
        assert!(matches!(
            entry.extra,
            IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: Some(1_700_000_000),
                status: Some(0),
            }
        ));

        let agg = domain_store
            .get_identity_collection_aggregate(&ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION)
            .expect("dotbit agg")
            .expect("dotbit agg exists");
        assert_eq!(agg.standard, IdentityStandard::DotBit);
        assert_eq!(agg.name.as_deref(), Some(".bit"));
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);
        assert_eq!(agg.activities_count, 1);

        let account_ids = domain_store
            .list_identity_ids_by_collection(
                &ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION,
                None,
                10,
            )
            .expect("identity ids");
        assert_eq!(account_ids, vec![account_id.to_vec()]);

        assert_eq!(
            domain_store
                .get_identity_owner_count(
                    &ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION,
                    &[0xa1; 32],
                )
                .expect("owner count a"),
            0
        );
        assert_eq!(
            domain_store
                .get_identity_owner_count(
                    &ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION,
                    &[0xb2; 32],
                )
                .expect("owner count b"),
            0
        );

        let first_outpoint = domain_store
            .get_dotbit_account_id_by_outpoint(&[0x31; 32], 0)
            .expect("first outpoint")
            .expect("first outpoint exists");
        assert_eq!(first_outpoint, account_id.to_vec());
        let second_outpoint = domain_store
            .get_dotbit_account_id_by_outpoint(&[0x32; 32], 0)
            .expect("second outpoint")
            .expect("second outpoint exists");
        assert_eq!(second_outpoint, account_id.to_vec());

        let outpoints = domain_store
            .list_dotbit_account_outpoints_by_account_id(&account_id)
            .expect("dotbit outpoints");
        assert_eq!(
            outpoints,
            vec![(vec![0x31; 32], 0_i16), (vec![0x32; 32], 0_i16)]
        );

        let hour_bucket = tx1.timestamp_ms / 3_600_000;
        let hourly_key = keys::encode_nft_hourly_key(
            &ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION,
            hour_bucket,
        );
        let hourly_value = domain_store
            .get_stats_key(&hourly_key)
            .expect("stats lookup")
            .expect("hourly stats");
        assert_eq!(
            i64::from_le_bytes(hourly_value[..8].try_into().expect("hourly bytes")),
            1
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
