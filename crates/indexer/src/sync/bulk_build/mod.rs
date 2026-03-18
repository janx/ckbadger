#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use anyhow::{anyhow, bail};
use ckbadger_store::keys;
use ckbadger_store::types::{DID_CKB_SENTINEL_COLLECTION, ObjectStandard, SporeTypeIndex};
use ckbadger_store::{AddressBalance, CkbadgerStore, ScriptInfo};
use rocksdb::IteratorMode;
use tracing::info;

use super::indexer::Indexer;
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::owners::BulkReducer;

pub(crate) mod facts;
pub(crate) mod interner;
pub(crate) mod live_cells;
pub(crate) mod materialize;
pub(crate) mod owners;
pub(crate) mod sequencer;

#[derive(Default)]
pub(crate) struct BulkBuildEngine;

impl BulkBuildEngine {
    pub(crate) async fn run(indexer: &Indexer) -> Result<()> {
        // Temporary routing seam: startup bulk sync now has an explicit build-engine
        // entrypoint, while the underlying execution still delegates to the existing
        // pipeline until reducers/materialization land in later tasks.
        info!(
            run_id = %indexer.run_id,
            "Bulk build engine route selected; delegating to pipeline until build engine materialization is implemented"
        );
        indexer.run_pipeline().await
    }
}

#[derive(Default)]
struct CoreOwners {
    address: owners::address::AddressOwner,
    script: owners::script::ScriptOwner,
    token: owners::token::TokenOwner,
    dao: owners::dao::DaoOwner,
    object: owners::object::ObjectOwner,
}

impl CoreOwners {
    fn apply_tx(
        &mut self,
        tx: &facts::ResolvedTxFacts,
        ctx: &owners::ReducerContext<'_>,
    ) -> Result<()> {
        self.address.apply_tx(tx, ctx)?;
        self.script.apply_tx(tx, ctx)?;
        self.token.apply_tx(tx, ctx)?;
        self.dao.apply_tx(tx, ctx)?;
        self.object.apply_tx(tx, ctx)?;
        Ok(())
    }

    fn materialize_all(&mut self, materializer: &mut materialize::Materializer<'_>) -> Result<()> {
        self.address.flush_sealed(materializer)?;
        self.script.flush_sealed(materializer)?;
        self.token.flush_sealed(materializer)?;
        self.dao.flush_sealed(materializer)?;
        self.object.flush_sealed(materializer)?;

        self.address.materialize_final(materializer)?;
        self.script.materialize_final(materializer)?;
        self.token.materialize_final(materializer)?;
        self.dao.materialize_final(materializer)?;
        self.object.materialize_final(materializer)?;
        Ok(())
    }
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct CoreOwnerStateSnapshot {
    pub address_balances: HashMap<Vec<u8>, AddressBalance>,
    pub script_infos: HashMap<Vec<u8>, ScriptInfo>,
    pub token_state: owners::token::TokenStateSnapshot,
    pub dao_state: owners::dao::DaoStateSnapshot,
    pub object_state: owners::object::ObjectStateSnapshot,
}

#[doc(hidden)]
pub(crate) fn materialize_core_owner_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<CoreOwnerStateSnapshot> {
    let mut interner = interner::IdentityInterner::default();
    let arena = crate::sync::pipeline::build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    let resolved = sequencer::BulkSequencer::default().resolve(&arena)?;
    let ctx = owners::ReducerContext::new(&interner);
    let mut owners = CoreOwners::default();

    for tx in &resolved {
        owners.apply_tx(tx, &ctx)?;
    }

    let root = unique_temp_test_dir("bulk-build-core-owners");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = materialize::Materializer::new(&domain_store, &append_store);
        owners.materialize_all(&mut materializer)?;
        let _ = materializer.finish();

        let mut address_balances = HashMap::new();
        let addr_iter = domain_store.iterator_cf(domain_store.cf_addr_balance(), IteratorMode::Start);
        for item in addr_iter {
            let (key, value) = item?;
            let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize AddressBalance in core owner snapshot helper: lock_hash=0x{}, error={}",
                    hex::encode(&key),
                    e
                )
            })?;
            address_balances.insert(key.to_vec(), balance);
        }

        let script_infos = domain_store
            .list_script_infos()?
            .into_iter()
            .collect::<HashMap<_, _>>();

        let tokens = domain_store
            .list_tokens()?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let mut token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
        for type_hash in tokens.keys() {
            let holders = domain_store
                .list_token_holders(type_hash, usize::MAX)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            token_holders.insert(type_hash.clone(), holders);
        }
        let mut addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
        let addr_tokens_iter = domain_store.iterator_cf(
            domain_store.cf_addr_tokens_by_balance(),
            IteratorMode::Start,
        );
        for item in addr_tokens_iter {
            let (key, value) = item?;
            if !value.is_empty() {
                bail!(
                    "addr_tokens_by_balance value must be empty in core owner snapshot helper: value_len={}",
                    value.len()
                );
            }
            let (lock_hash, balance, type_hash) = keys::decode_addr_token_balance_key(&key);
            addr_tokens
                .entry(lock_hash)
                .or_default()
                .insert(type_hash, balance);
        }
        let token_state = owners::token::TokenStateSnapshot {
            tokens,
            token_holders,
            addr_tokens,
        };

        let deposits = domain_store
            .list_dao_deposits()?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let page_limit = deposits.len().max(1);
        let mut withdraw_lookup: HashMap<Vec<u8>, HashMap<i16, Vec<u8>>> = HashMap::new();
        for (outpoint_key, entry) in &deposits {
            if let (Some(request_tx), Some(request_output_index)) = (
                entry.withdraw_request_tx.as_ref(),
                entry.withdraw_request_output_index,
            ) {
                let linked = domain_store
                    .get_dao_deposit_by_withdraw_tx(request_tx, request_output_index)?
                    .ok_or_else(|| {
                        anyhow!(
                            "dao_by_withdraw_tx missing in core owner snapshot helper: request_tx=0x{}, output_index={}",
                            hex::encode(request_tx),
                            request_output_index
                        )
                    })?;
                withdraw_lookup
                    .entry(request_tx.clone())
                    .or_default()
                    .insert(request_output_index, linked.clone());
                if linked != *outpoint_key {
                    bail!(
                        "dao_by_withdraw_tx mismatch in core owner snapshot helper: request_tx=0x{}, output_index={}",
                        hex::encode(request_tx),
                        request_output_index
                    );
                }
            }
        }
        let mut by_status = HashMap::new();
        for status in [0i16, 1, 2] {
            let outpoints = domain_store
                .list_dao_deposits_by_status_paginated(status, page_limit, None)?
                .into_iter()
                .map(|(outpoint, _entry)| outpoint)
                .collect::<Vec<_>>();
            by_status.insert(status, outpoints);
        }
        let mut by_lock: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        for (outpoint_key, entry) in &deposits {
            let rows = domain_store
                .list_dao_deposits_by_lock_paginated(&entry.lock_script_hash, page_limit, None)?
                .into_iter()
                .map(|(outpoint, _entry)| outpoint)
                .collect::<Vec<_>>();
            if !rows.iter().any(|row| row == outpoint_key) {
                bail!(
                    "dao_by_lock_block missing outpoint in core owner snapshot helper: outpoint=0x{}",
                    hex::encode(outpoint_key)
                );
            }
            by_lock.insert(entry.lock_script_hash.clone(), rows);
        }
        let dao_state = owners::dao::DaoStateSnapshot {
            deposits,
            withdraw_lookup,
            by_status,
            by_lock,
        };

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
        let stats_spore_iter = domain_store.iterator_cf(domain_store.cf_stats_spore(), IteratorMode::Start);
        for item in stats_spore_iter {
            let (key, value) = item?;
            if key.len() != keys::SPORE_TYPE_INDEX_KEY_SIZE
                || key[0] != keys::STATS_PREFIX_SPORE_TYPE_INDEX
            {
                continue;
            }
            let type_hash = key[1..33].to_vec();
            let index: SporeTypeIndex = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize SporeTypeIndex in core owner snapshot helper: type_hash=0x{}, error={}",
                    hex::encode(&type_hash),
                    e
                )
            })?;
            spore_type_indexes.insert(type_hash, index);
        }
        let object_state = owners::object::ObjectStateSnapshot {
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
        };

        CoreOwnerStateSnapshot {
            address_balances,
            script_infos,
            token_state,
            dao_state,
            object_state,
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
