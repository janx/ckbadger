#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use anyhow::{anyhow, bail};
use ckbadger_store::keys;
use ckbadger_store::types::{
    CachedBlockHeader, DID_CKB_SENTINEL_COLLECTION, ObjectStandard, SporeTypeIndex, TxIndexEntry,
};
use ckbadger_store::{
    AddressBalance, CkbadgerStore, ScriptInfo, CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS,
    CF_TX_HASH_MAP, CF_TX_INDEX,
};
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
#[derive(Debug, Default, Clone)]
pub struct BulkArtifactSnapshot {
    pub report: materialize::MaterializationReport,
    pub block_headers: HashMap<i64, CachedBlockHeader>,
    pub block_numbers_by_hash: HashMap<Vec<u8>, i64>,
    pub txs_by_hash: HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
    pub core: CoreOwnerStateSnapshot,
}

#[doc(hidden)]
pub(crate) fn materialize_core_owner_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<CoreOwnerStateSnapshot> {
    Ok(materialize_bulk_artifacts_for_test(blocks)?.core)
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_artifacts_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<BulkArtifactSnapshot> {
    let mut interner = interner::IdentityInterner::default();
    let arena = crate::sync::pipeline::build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    let mut sequencer = sequencer::BulkSequencer::default();
    let resolved = sequencer.resolve(&arena)?;
    let ctx = owners::ReducerContext::new(&interner);
    let mut owners = CoreOwners::default();
    let history_rows = build_history_rows(&arena, &resolved)?;

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
        materializer.stream_history_rows(&history_rows)?;
        owners.materialize_all(&mut materializer)?;
        let report = materializer.finish();

        let core = collect_core_owner_state_snapshot(&domain_store)?;
        let (block_headers, block_numbers_by_hash, txs_by_hash) =
            collect_history_snapshot(&domain_store)?;

        BulkArtifactSnapshot {
            report,
            block_headers,
            block_numbers_by_hash,
            txs_by_hash,
            core,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn build_history_rows(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts],
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::with_capacity(arena.blocks.len() * 2 + arena.txs.len() * 2);

    for block in &arena.blocks {
        let header = CachedBlockHeader {
            hash: block.hash.to_vec(),
            timestamp: block.timestamp_ms,
            epoch_number: block.epoch_number,
            epoch_index: block.epoch_index,
            epoch_length: block.epoch_length,
            dao: block.dao.clone(),
            transactions_count: block.transactions_count,
        };
        rows.push(materialize::MaterializedRow::new(
            CF_BLOCK_HEADERS,
            keys::encode_block_num(block.number).to_vec(),
            bincode::serialize(&header)?,
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_BLOCK_HASH_INDEX,
            block.hash.to_vec(),
            block.number.to_le_bytes().to_vec(),
        ));
    }

    if arena.txs.len() != resolved.len() {
        bail!(
            "bulk build history tx count mismatch: facts_txs={} resolved_txs={}",
            arena.txs.len(),
            resolved.len()
        );
    }

    for (tx, resolved_tx) in arena.txs.iter().zip(resolved) {
        if tx.hash != resolved_tx.tx_hash
            || tx.block_number != resolved_tx.block_number
            || tx.tx_index != resolved_tx.tx_index
        {
            bail!(
                "bulk build history tx alignment mismatch: facts_tx=0x{} facts_block={} facts_tx_index={} resolved_tx=0x{} resolved_block={} resolved_tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                hex::encode(resolved_tx.tx_hash),
                resolved_tx.block_number,
                resolved_tx.tx_index
            );
        }

        let entry = TxIndexEntry {
            is_cellbase: tx.is_cellbase,
            timestamp: tx.timestamp_ms,
            inputs_count: tx.inputs_count,
            outputs_count: tx.outputs_count,
            fee: resolved_tx_fee(tx, resolved_tx)?,
            tx_size: tx.tx_size,
            cycles: tx.cycles,
        };
        let tx_location = keys::encode_composite(&[
            &keys::encode_block_num(tx.block_number),
            &keys::encode_tx_idx(tx.tx_index),
        ]);
        rows.push(materialize::MaterializedRow::new(
            CF_TX_INDEX,
            tx_location.to_vec(),
            bincode::serialize(&entry)?,
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_TX_HASH_MAP,
            tx.hash.to_vec(),
            tx_location.to_vec(),
        ));
    }

    Ok(rows)
}

fn resolved_tx_fee(tx: &facts::TxFacts, resolved_tx: &facts::ResolvedTxFacts) -> Result<i64> {
    if tx.is_cellbase {
        return Ok(0);
    }

    let total_input_capacity =
        resolved_tx
            .resolved_inputs
            .iter()
            .try_fold(0i64, |acc, input| {
                acc.checked_add(input.capacity).ok_or_else(|| {
                    anyhow!(
                        "bulk build input capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                        hex::encode(tx.hash),
                        tx.block_number,
                        tx.tx_index
                    )
                })
            })?;
    let total_output_capacity = resolved_tx.cells.iter().try_fold(0i64, |acc, cell| {
        acc.checked_add(cell.capacity).ok_or_else(|| {
            anyhow!(
                "bulk build output capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index
            )
        })
    })?;

    total_input_capacity
        .checked_sub(total_output_capacity)
        .ok_or_else(|| {
            anyhow!(
                "bulk build negative fee while materializing tx index: tx=0x{} block={} tx_index={} inputs={} outputs={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                total_input_capacity,
                total_output_capacity
            )
        })
}

fn collect_history_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<i64, CachedBlockHeader>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
)> {
    let mut block_headers = HashMap::new();
    let mut block_numbers_by_hash = HashMap::new();
    let block_iter = domain_store.iterator_cf(domain_store.cf_block_headers(), IteratorMode::Start);
    for item in block_iter {
        let (key, value) = item?;
        if key.len() != 8 {
            bail!(
                "invalid block_headers key length in bulk artifact snapshot helper: key_len={}",
                key.len()
            );
        }
        let block_number = keys::decode_block_num(&key);
        let header: CachedBlockHeader = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize CachedBlockHeader in bulk artifact snapshot helper: block_number={} error={}",
                block_number,
                e
            )
        })?;
        let indexed_block_number = domain_store
            .get_block_number_by_hash(&header.hash)?
            .ok_or_else(|| {
                anyhow!(
                    "block_hash_index missing in bulk artifact snapshot helper: block_number={} hash=0x{}",
                    block_number,
                    hex::encode(&header.hash)
                )
            })?;
        if indexed_block_number != block_number {
            bail!(
                "block_hash_index mismatch in bulk artifact snapshot helper: block_number={} indexed_block_number={} hash=0x{}",
                block_number,
                indexed_block_number,
                hex::encode(&header.hash)
            );
        }
        block_numbers_by_hash.insert(header.hash.clone(), indexed_block_number);
        block_headers.insert(block_number, header);
    }

    let mut txs_by_hash = HashMap::new();
    let tx_iter = domain_store.iterator_cf(domain_store.cf_tx_hash_map(), IteratorMode::Start);
    for item in tx_iter {
        let (tx_hash, _value) = item?;
        let tx_entry = domain_store
            .get_tx_by_hash(&tx_hash)?
            .ok_or_else(|| {
                anyhow!(
                    "tx index missing in bulk artifact snapshot helper: tx_hash=0x{}",
                    hex::encode(&tx_hash)
                )
            })?;
        txs_by_hash.insert(tx_hash.to_vec(), tx_entry);
    }

    Ok((block_headers, block_numbers_by_hash, txs_by_hash))
}

fn collect_core_owner_state_snapshot(domain_store: &CkbadgerStore) -> Result<CoreOwnerStateSnapshot> {
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
    let did_agg = domain_store.get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)?;
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
        if key.len() != keys::SPORE_TYPE_INDEX_KEY_SIZE || key[0] != keys::STATS_PREFIX_SPORE_TYPE_INDEX
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

    Ok(CoreOwnerStateSnapshot {
        address_balances,
        script_infos,
        token_state,
        dao_state,
        object_state,
    })
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
