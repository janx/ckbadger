#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use anyhow::{anyhow, bail};
use ckbadger_store::keys;
use ckbadger_store::types::{
    decode_live_cell_marker, CachedBlockHeader, ConsumedCellMeta, DID_CKB_SENTINEL_COLLECTION,
    LiveCellInfo, ObjectStandard, SporeTypeIndex, TokenTransferRecord, TxActivityBundle,
    TxIndexEntry,
};
use ckbadger_store::{
    AddressBalance, CkbadgerStore, ScriptInfo, CF_ACTIVITIES, CF_ADDR_TXS,
    CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS, CF_CELL_BY_DATA_HASH, CF_CELL_BY_LOCK,
    CF_CELL_BY_LOCK_CODE, CF_CELL_BY_TYPE, CF_CELL_BY_TYPE_CODE, CF_CELLS,
    CF_CONSUMED_CELLS, CF_LIVE_CELLS, CF_TX_HASH_MAP, CF_TX_INDEX,
};
use ckbadger_store::store::CF_TOKEN_TRANSFERS;
use rocksdb::IteratorMode;
use tracing::info;

use super::indexer::Indexer;
use crate::parser::cell::ParsedCell;
use crate::parser::{ParsedUdtCell, ScriptParser, UdtParser, UdtStandard};
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
    pub activity_bundles: HashMap<Vec<u8>, TxActivityBundle>,
    pub cell_payloads: HashMap<Vec<u8>, LiveCellInfo>,
    pub live_cells: HashMap<Vec<u8>, i64>,
    pub consumed_cells: HashMap<Vec<u8>, ConsumedCellMeta>,
    pub cell_by_lock: HashSet<Vec<u8>>,
    pub cell_by_type: HashSet<Vec<u8>>,
    pub cell_by_lock_code: HashSet<Vec<u8>>,
    pub cell_by_type_code: HashSet<Vec<u8>>,
    pub cell_by_data_hash: HashSet<Vec<u8>>,
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
    let history_rows = build_history_rows(&arena, &resolved, &interner, true)?;
    let final_snapshot_rows = build_final_snapshot_rows(&sequencer, &interner)?;

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
        materializer.materialize_final_snapshot(&final_snapshot_rows)?;
        owners.materialize_all(&mut materializer)?;
        let report = materializer.finish();

        let core = collect_core_owner_state_snapshot(&domain_store)?;
        let (block_headers, block_numbers_by_hash, txs_by_hash, activity_bundles) =
            collect_history_snapshot(&domain_store)?;
        let (
            cell_payloads,
            live_cells,
            consumed_cells,
            cell_by_lock,
            cell_by_type,
            cell_by_lock_code,
            cell_by_type_code,
            cell_by_data_hash,
        ) = collect_cell_snapshot(&domain_store, &append_store)?;

        BulkArtifactSnapshot {
            report,
            block_headers,
            block_numbers_by_hash,
            txs_by_hash,
            activity_bundles,
            cell_payloads,
            live_cells,
            consumed_cells,
            cell_by_lock,
            cell_by_type,
            cell_by_lock_code,
            cell_by_type_code,
            cell_by_data_hash,
            core,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn build_history_rows(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
    is_mainnet: bool,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows =
        Vec::with_capacity(
            arena.blocks.len() * 2 + arena.txs.len() * 2 + arena.cells.len() * 2 + arena.txs.len(),
        );

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

        let mut touched_lock_hash_ids = HashSet::new();
        for output in &resolved_tx.cells {
            touched_lock_hash_ids.insert(output.lock_script_hash_id);
        }
        for input in &resolved_tx.resolved_inputs {
            touched_lock_hash_ids.insert(input.lock_script_hash_id);
        }
        for lock_hash_id in touched_lock_hash_ids {
            rows.push(materialize::MaterializedRow::new(
                CF_ADDR_TXS,
                keys::encode_addr_tx_key(
                    interner.resolve_bytes(lock_hash_id),
                    tx.block_number,
                    tx.tx_index,
                    &tx.hash,
                ),
                Vec::new(),
            ));
        }

        if tx.is_cellbase {
            continue;
        }

        for input in &resolved_tx.resolved_inputs {
            rows.push(materialize::MaterializedRow::new(
                CF_CONSUMED_CELLS,
                keys::encode_outpoint(&input.outpoint.tx_hash, resolved_input_outpoint_index_i16(input)?)
                    .to_vec(),
                bincode::serialize(&ConsumedCellMeta {
                    created_at_block: input.created_at_block,
                    consumed_at_block: tx.block_number,
                    consumed_by_tx: Some(tx.hash.to_vec()),
                })?,
            ));
        }
    }

    rows.extend(build_token_transfer_rows(resolved, interner)?);
    rows.extend(build_activity_rows(arena, resolved, interner, is_mainnet)?);

    for cell in &arena.cells {
        let outpoint_key =
            keys::encode_outpoint(&cell.outpoint.tx_hash, cell_outpoint_index_i16(cell)?).to_vec();
        rows.push(materialize::MaterializedRow::new(
            CF_CELLS,
            outpoint_key,
            bincode::serialize(&cell_facts_to_live_cell_info(cell, interner))?,
        ));

        if let Some(data_hash) = &cell.data_hash {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_DATA_HASH,
                keys::encode_cell_index_key(
                    data_hash,
                    cell.created_at_block,
                    &cell.outpoint.tx_hash,
                    cell_outpoint_index_i16(cell)?,
                ),
                Vec::new(),
            ));
        }
    }

    Ok(rows)
}

fn build_activity_rows(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
    is_mainnet: bool,
) -> Result<Vec<materialize::MaterializedRow>> {
    let block_ar_by_number: HashMap<i64, u64> = arena
        .blocks
        .iter()
        .map(|block| {
            let ar = crate::db::writer::dao::extract_ar_from_dao(&block.dao).ok_or_else(|| {
                anyhow!(
                    "missing AR in block DAO field while building bulk activities: block={}",
                    block.number
                )
            })?;
            Ok((block.number, ar))
        })
        .collect::<Result<_>>()?;
    let detectors = build_activity_protocol_detectors(resolved, interner, is_mainnet)?;
    let token_info_cache = HashMap::new();
    let mut rows = Vec::new();

    for block in &arena.blocks {
        let txs = &arena.txs[block.tx_range.clone()];
        let resolved_txs = &resolved[block.tx_range.clone()];
        if txs.len() != resolved_txs.len() {
            bail!(
                "bulk build activity tx count mismatch within block: block={} facts_txs={} resolved_txs={}",
                block.number,
                txs.len(),
                resolved_txs.len()
            );
        }

        let mut block_inputs = Vec::with_capacity(txs.len());
        let mut block_outputs = Vec::with_capacity(txs.len());
        for (tx, resolved_tx) in txs.iter().zip(resolved_txs) {
            if tx.hash != resolved_tx.tx_hash
                || tx.block_number != resolved_tx.block_number
                || tx.tx_index != resolved_tx.tx_index
            {
                bail!(
                    "bulk build activity tx alignment mismatch: facts_tx=0x{} facts_block={} facts_tx_index={} resolved_tx=0x{} resolved_block={} resolved_tx_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    tx.tx_index,
                    hex::encode(resolved_tx.tx_hash),
                    resolved_tx.block_number,
                    resolved_tx.tx_index
                );
            }

            block_outputs.push(
                resolved_tx
                    .cells
                    .iter()
                    .map(|cell| parsed_cell_from_facts(cell, interner))
                    .collect::<Result<Vec<_>>>()?,
            );
            block_inputs.push(
                resolved_tx
                .resolved_inputs
                .iter()
                .map(|input| activity_input_view_from_resolved_input(input, interner, &block_ar_by_number))
                .collect::<Result<Vec<_>>>()?,
            );
        }

        let tx_views = txs
            .iter()
            .zip(block_inputs.into_iter())
            .zip(block_outputs.iter())
            .map(|((tx, inputs), outputs)| crate::db::writer::activities::TxView {
                tx_hash: &tx.hash,
                block_hash: &tx.block_hash,
                tx_index: tx.tx_index,
                block_number: tx.block_number,
                timestamp: tx.timestamp_ms,
                is_cellbase: tx.is_cellbase,
                inputs,
                outputs,
                witnesses: &[],
            })
            .collect::<Vec<_>>();

        let bundles = crate::db::writer::activities::build_activity_bundles_for_block_with_detectors(
            &tx_views,
            &token_info_cache,
            &detectors,
        )?;
        for bundle in bundles {
            rows.push(materialize::MaterializedRow::new(
                CF_ACTIVITIES,
                keys::encode_tx_activity_bundle_key(
                    bundle.block_number,
                    bundle.tx_index,
                    &bundle.tx_hash,
                ),
                bincode::serialize(&bundle)?,
            ));
        }
    }

    Ok(rows)
}

fn build_token_transfer_rows(
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::new();
    let mut transfer_idx: HashMap<(Vec<u8>, i64), i32> = HashMap::new();

    for tx in resolved {
        let input_udts = tx
            .resolved_inputs
            .iter()
            .filter_map(|input| parsed_udt_cell_from_input(input, interner, tx).transpose())
            .collect::<Result<Vec<_>>>()?;
        let output_udts = tx
            .cells
            .iter()
            .filter_map(|cell| parsed_udt_cell_from_output(cell, interner, tx).transpose())
            .collect::<Result<Vec<_>>>()?;

        for transfer in UdtParser::build_transfers_from_cells(&input_udts, &output_udts) {
            let idx = transfer_idx
                .entry((transfer.type_script_hash.clone(), tx.block_number))
                .or_insert(0);
            let record = TokenTransferRecord {
                tx_hash: tx.tx_hash.to_vec(),
                block_number: tx.block_number,
                from_lock_hash: transfer.from_lock_hash.clone(),
                to_lock_hash: transfer.to_lock_hash.clone(),
                amount: transfer.amount,
                is_mint: transfer.is_mint,
                is_burn: transfer.is_burn,
                timestamp: tx.timestamp_ms,
            };
            rows.push(materialize::MaterializedRow::new(
                CF_TOKEN_TRANSFERS,
                keys::encode_token_transfer_key(&transfer.type_script_hash, tx.block_number, *idx),
                bincode::serialize(&record)?,
            ));
            *idx = idx.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "token transfer index overflow in bulk build history rows: type_hash=0x{} block={}",
                    hex::encode(&transfer.type_script_hash),
                    tx.block_number
                )
            })?;
        }
    }

    Ok(rows)
}

fn build_activity_protocol_detectors(
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
    is_mainnet: bool,
) -> Result<Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>>> {
    let mut lock_code_hashes = HashSet::new();
    let mut type_code_hashes = HashSet::new();

    for tx in resolved {
        for input in &tx.resolved_inputs {
            lock_code_hashes.insert(activity_code_hash(
                interner,
                input.lock_code_hash_id,
                "input lock_code_hash",
                tx,
            )?);
            if let Some(type_code_hash_id) = input.type_code_hash_id {
                type_code_hashes.insert(activity_code_hash(
                    interner,
                    type_code_hash_id,
                    "input type_code_hash",
                    tx,
                )?);
            }
        }

        for cell in &tx.cells {
            lock_code_hashes.insert(activity_code_hash(
                interner,
                cell.lock_code_hash_id,
                "output lock_code_hash",
                tx,
            )?);
            if let Some(type_code_hash_id) = cell.type_code_hash_id {
                type_code_hashes.insert(activity_code_hash(
                    interner,
                    type_code_hash_id,
                    "output type_code_hash",
                    tx,
                )?);
            }
        }
    }

    Ok(vec![
        Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
            is_mainnet,
        )) as Box<dyn crate::db::writer::activities::ProtocolDetector>,
        Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
            is_mainnet,
        )),
        Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
            is_mainnet,
        )),
        Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
            is_mainnet,
        )),
    ]
    .into_iter()
    .filter(|detector| detector.might_apply_batch(&lock_code_hashes, &type_code_hashes))
    .collect())
}

fn activity_code_hash(
    interner: &interner::IdentityInterner,
    id: crate::sync::types::InternId,
    label: &str,
    tx: &facts::ResolvedTxFacts,
) -> Result<[u8; 32]> {
    interner.resolve_bytes(id).try_into().map_err(|_| {
        anyhow!(
            "invalid {} length while building bulk activities: tx=0x{} block={} tx_index={} len={}",
            label,
            hex::encode(tx.tx_hash),
            tx.block_number,
            tx.tx_index,
            interner.resolve_bytes(id).len()
        )
    })
}

fn activity_input_view_from_resolved_input(
    input: &facts::ResolvedInputFacts,
    interner: &interner::IdentityInterner,
    block_ar_by_number: &HashMap<i64, u64>,
) -> Result<crate::db::writer::activities::InputCellView> {
    let (is_dao_withdraw_request, dao_compensation) = match input.dao_state {
        Some(facts::DaoCellState::WithdrawRequest { deposit_block_number }) => {
            let deposit_ar = *block_ar_by_number.get(&deposit_block_number).ok_or_else(|| {
                anyhow!(
                    "missing deposit block AR while building bulk DAO activity input: deposit_block={} outpoint=0x{}:{}",
                    deposit_block_number,
                    hex::encode(input.outpoint.tx_hash),
                    input.outpoint.index
                )
            })?;
            let request_ar = *block_ar_by_number.get(&input.created_at_block).ok_or_else(|| {
                anyhow!(
                    "missing withdraw-request block AR while building bulk DAO activity input: request_block={} outpoint=0x{}:{}",
                    input.created_at_block,
                    hex::encode(input.outpoint.tx_hash),
                    input.outpoint.index
                )
            })?;
            let compensation = crate::db::writer::dao::calculate_dao_compensation_from_ar(
                input.capacity,
                deposit_ar,
                request_ar,
            )?;
            (true, Some(compensation))
        }
        _ => (false, None),
    };

    Ok(crate::db::writer::activities::InputCellView {
        lock_script_hash: interner.resolve_bytes(input.lock_script_hash_id).to_vec(),
        lock_code_hash: interner.resolve_bytes(input.lock_code_hash_id).to_vec(),
        lock_hash_type: input.lock_hash_type,
        lock_args: interner.resolve_bytes(input.lock_args_id).to_vec(),
        capacity: input.capacity,
        occupied_capacity: input.occupied_capacity,
        type_code_hash: input
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: input.type_hash_type,
        type_script_hash: input
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_args: input.type_args_id.map(|id| interner.resolve_bytes(id).to_vec()),
        udt_amount: input.udt_amount,
        data: Vec::new(),
        is_dao_withdraw_request,
        dao_compensation,
    })
}

fn parsed_cell_from_facts(
    cell: &facts::CellFacts,
    interner: &interner::IdentityInterner,
) -> Result<ParsedCell> {
    if usize::try_from(cell.data_size).ok() != Some(cell.data.len()) {
        bail!(
            "bulk build cell data size mismatch while building activities: outpoint=0x{}:{} data_size={} actual_len={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index,
            cell.data_size,
            cell.data.len()
        );
    }

    Ok(ParsedCell {
        capacity: cell.capacity,
        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id).to_vec(),
        lock_hash_type: cell.lock_hash_type,
        lock_args: interner.resolve_bytes(cell.lock_args_id).to_vec(),
        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id).to_vec(),
        type_code_hash: cell
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: cell.type_hash_type,
        type_args: cell.type_args_id.map(|id| interner.resolve_bytes(id).to_vec()),
        type_script_hash: cell
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        data_hash: cell
            .data_hash
            .clone()
            .unwrap_or_else(|| ScriptParser::compute_data_hash(&cell.data)),
        data_size: cell.data_size,
        data: cell.data.clone(),
    })
}

fn parsed_udt_cell_from_output(
    cell: &facts::CellFacts,
    interner: &interner::IdentityInterner,
    tx: &facts::ResolvedTxFacts,
) -> Result<Option<ParsedUdtCell>> {
    parsed_udt_cell_from_parts(
        cell.semantic_tag,
        cell.type_script_hash_id,
        cell.type_code_hash_id,
        cell.type_hash_type,
        cell.type_args_id,
        cell.lock_script_hash_id,
        cell.udt_amount,
        interner,
        &format!(
            "output outpoint=0x{}:{} block={} tx=0x{} tx_index={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        ),
    )
}

fn parsed_udt_cell_from_input(
    input: &facts::ResolvedInputFacts,
    interner: &interner::IdentityInterner,
    tx: &facts::ResolvedTxFacts,
) -> Result<Option<ParsedUdtCell>> {
    parsed_udt_cell_from_parts(
        input.semantic_tag,
        input.type_script_hash_id,
        input.type_code_hash_id,
        input.type_hash_type,
        input.type_args_id,
        input.lock_script_hash_id,
        input.udt_amount,
        interner,
        &format!(
            "input outpoint=0x{}:{} block={} tx=0x{} tx_index={}",
            hex::encode(input.outpoint.tx_hash),
            input.outpoint.index,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        ),
    )
}

fn parsed_udt_cell_from_parts(
    semantic_tag: facts::CellSemanticTag,
    type_script_hash_id: Option<crate::sync::types::InternId>,
    type_code_hash_id: Option<crate::sync::types::InternId>,
    type_hash_type: Option<i16>,
    type_args_id: Option<crate::sync::types::InternId>,
    lock_script_hash_id: crate::sync::types::InternId,
    udt_amount: Option<u128>,
    interner: &interner::IdentityInterner,
    context: &str,
) -> Result<Option<ParsedUdtCell>> {
    let Some(standard) = udt_standard_for_semantic_tag(semantic_tag) else {
        return Ok(None);
    };

    let type_script_hash_id = type_script_hash_id.ok_or_else(|| {
        anyhow!(
            "missing type_script_hash_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_code_hash_id = type_code_hash_id.ok_or_else(|| {
        anyhow!(
            "missing type_code_hash_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_hash_type = type_hash_type.ok_or_else(|| {
        anyhow!(
            "missing type_hash_type for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_args_id = type_args_id.ok_or_else(|| {
        anyhow!(
            "missing type_args_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let amount = udt_amount.ok_or_else(|| {
        anyhow!(
            "missing udt_amount for bulk build token transfer cell: {}",
            context
        )
    })?;

    Ok(Some(ParsedUdtCell {
        type_script_hash: interner.resolve_bytes(type_script_hash_id).to_vec(),
        type_code_hash: interner.resolve_bytes(type_code_hash_id).to_vec(),
        type_hash_type,
        type_args: interner.resolve_bytes(type_args_id).to_vec(),
        lock_script_hash: interner.resolve_bytes(lock_script_hash_id).to_vec(),
        amount,
        standard,
    }))
}

fn udt_standard_for_semantic_tag(semantic_tag: facts::CellSemanticTag) -> Option<UdtStandard> {
    match semantic_tag {
        facts::CellSemanticTag::Sudt => Some(UdtStandard::Sudt),
        facts::CellSemanticTag::Xudt => Some(UdtStandard::Xudt),
        _ => None,
    }
}

fn build_final_snapshot_rows(
    sequencer: &sequencer::BulkSequencer,
    interner: &interner::IdentityInterner,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::with_capacity(sequencer.live_count() * 5);

    for slot in sequencer.live_slots() {
        let outpoint_index = live_slot_outpoint_index_i16(slot)?;
        rows.push(materialize::MaterializedRow::new(
            CF_LIVE_CELLS,
            keys::encode_outpoint(&slot.outpoint.tx_hash, outpoint_index).to_vec(),
            slot.created_at_block.to_le_bytes().to_vec(),
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_script_hash_id),
                slot.created_at_block,
                &slot.outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK_CODE,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_code_hash_id),
                slot.created_at_block,
                &slot.outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ));
        if let Some(type_script_hash_id) = slot.type_script_hash_id {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_script_hash_id),
                    slot.created_at_block,
                    &slot.outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ));
        }
        if let Some(type_code_hash_id) = slot.type_code_hash_id {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE_CODE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_code_hash_id),
                    slot.created_at_block,
                    &slot.outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ));
        }
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

fn cell_facts_to_live_cell_info(
    cell: &facts::CellFacts,
    interner: &interner::IdentityInterner,
) -> LiveCellInfo {
    LiveCellInfo {
        capacity: cell.capacity,
        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id).to_vec(),
        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id).to_vec(),
        lock_hash_type: cell.lock_hash_type,
        lock_args: interner.resolve_bytes(cell.lock_args_id).to_vec(),
        type_script_hash: cell
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_code_hash: cell
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: cell.type_hash_type,
        type_args: cell.type_args_id.map(|id| interner.resolve_bytes(id).to_vec()),
        data_size: cell.data_size,
        occupied_capacity: cell.occupied_capacity,
        udt_amount: cell.udt_amount,
        data_hash: cell.data_hash.clone(),
    }
}

fn cell_outpoint_index_i16(cell: &facts::CellFacts) -> Result<i16> {
    i16::try_from(cell.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build cell outpoint index exceeds i16 while materializing cells: tx=0x{} output_index={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index
        )
    })
}

fn live_slot_outpoint_index_i16(slot: &live_cells::LiveCellSlot) -> Result<i16> {
    i16::try_from(slot.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build live outpoint index exceeds i16 while materializing live cells: tx=0x{} output_index={}",
            hex::encode(slot.outpoint.tx_hash),
            slot.outpoint.index
        )
    })
}

fn resolved_input_outpoint_index_i16(input: &facts::ResolvedInputFacts) -> Result<i16> {
    i16::try_from(input.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build consumed outpoint index exceeds i16 while materializing consumed cells: tx=0x{} output_index={}",
            hex::encode(input.outpoint.tx_hash),
            input.outpoint.index
        )
    })
}

fn collect_history_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<i64, CachedBlockHeader>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
    HashMap<Vec<u8>, TxActivityBundle>,
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

    let mut activity_bundles = HashMap::new();
    let activity_iter = domain_store.iterator_cf(domain_store.cf_activities(), IteratorMode::Start);
    for item in activity_iter {
        let (key, value) = item?;
        let bundle: TxActivityBundle = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize TxActivityBundle in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        activity_bundles.insert(key.to_vec(), bundle);
    }

    Ok((block_headers, block_numbers_by_hash, txs_by_hash, activity_bundles))
}

fn collect_cell_snapshot(
    domain_store: &CkbadgerStore,
    append_store: &CkbadgerStore,
) -> Result<(
    HashMap<Vec<u8>, LiveCellInfo>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, ConsumedCellMeta>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
)> {
    let mut cell_payloads = HashMap::new();
    let cell_iter = append_store.iterator_cf(append_store.cf_cells(), IteratorMode::Start);
    for item in cell_iter {
        let (key, value) = item?;
        let cell: LiveCellInfo = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize LiveCellInfo in bulk artifact snapshot helper: outpoint=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        cell_payloads.insert(key.to_vec(), cell);
    }

    let mut live_cells = HashMap::new();
    let live_iter = domain_store.iterator_cf(domain_store.cf_live_cells(), IteratorMode::Start);
    for item in live_iter {
        let (key, value) = item?;
        let created_at_block = decode_live_cell_marker(&value).ok_or_else(|| {
            anyhow!(
                "invalid live cell marker value in bulk artifact snapshot helper: outpoint=0x{} value_len={}",
                hex::encode(&key),
                value.len()
            )
        })?;
        live_cells.insert(key.to_vec(), created_at_block);
    }

    let mut consumed_cells = HashMap::new();
    let consumed_iter = domain_store.iterator_cf(domain_store.cf_consumed_cells(), IteratorMode::Start);
    for item in consumed_iter {
        let (key, value) = item?;
        let consumed: ConsumedCellMeta = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize ConsumedCellMeta in bulk artifact snapshot helper: outpoint=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        consumed_cells.insert(key.to_vec(), consumed);
    }

    let cell_by_lock = collect_index_keys(domain_store.iterator_cf(
        domain_store.cf_cell_by_lock(),
        IteratorMode::Start,
    ))?;
    let cell_by_type = collect_index_keys(domain_store.iterator_cf(
        domain_store.cf_cell_by_type(),
        IteratorMode::Start,
    ))?;
    let cell_by_lock_code = collect_index_keys(domain_store.iterator_cf(
        domain_store.cf_cell_by_lock_code(),
        IteratorMode::Start,
    ))?;
    let cell_by_type_code = collect_index_keys(domain_store.iterator_cf(
        domain_store.cf_cell_by_type_code(),
        IteratorMode::Start,
    ))?;
    let cell_by_data_hash = collect_index_keys(domain_store.iterator_cf(
        domain_store.cf_cell_by_data_hash(),
        IteratorMode::Start,
    ))?;

    Ok((
        cell_payloads,
        live_cells,
        consumed_cells,
        cell_by_lock,
        cell_by_type,
        cell_by_lock_code,
        cell_by_type_code,
        cell_by_data_hash,
    ))
}

fn collect_index_keys<I>(iter: I) -> Result<HashSet<Vec<u8>>>
where
    I: IntoIterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>>,
{
    let mut keys = HashSet::new();
    for item in iter {
        let (key, value) = item?;
        if !value.is_empty() {
            bail!(
                "cell index value must be empty in bulk artifact snapshot helper: key=0x{} value_len={}",
                hex::encode(&key),
                value.len()
            );
        }
        keys.insert(key.to_vec());
    }
    Ok(keys)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::udt::SUDT_CODE_HASH;
    use crate::parser::ScriptParser;
    use crate::rpc::{
        BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
        TransactionView,
    };
    use ckbadger_store::store::CF_TOKEN_TRANSFERS;
    use ckbadger_store::types::{TokenTransferRecord, TxActivityBundle};
    use ckbadger_store::{keys, CF_ACTIVITIES, CF_ADDR_TXS};

    fn fixture_lock_script(args_hex: &str) -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: args_hex.to_string(),
        }
    }

    fn fixture_header(number: u64, hash_byte: u8) -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: "0x18c7b3b2b00".to_string(),
            number: format!("0x{number:x}"),
            epoch: "0x7080006000028".to_string(),
            parent_hash: format!("0x{}", "11".repeat(32)),
            transactions_root: format!("0x{}", "22".repeat(32)),
            proposals_hash: format!("0x{}", "33".repeat(32)),
            extra_hash: format!("0x{}", "44".repeat(32)),
            dao: format!("0x{}", "00".repeat(32)),
            nonce: "0x1".to_string(),
            hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
        }
    }

    fn bulk_build_addr_tx_fixture() -> BlockResponseWithCycles {
        let lock_a_args = format!("0x{}", "01".repeat(20));
        let lock_b_args = format!("0x{}", "02".repeat(20));
        let create_tx = TransactionView {
            hash: format!("0x{}", "aa".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let split_tx = TransactionView {
            hash: format!("0x{}", "bb".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_a_args),
                    type_: None,
                },
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_b_args),
                    type_: None,
                },
            ],
            outputs_data: vec!["0x".to_string(), "0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_888, 0x99),
                uncles: vec![],
                transactions: vec![create_tx, split_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn fixture_sudt_type_script() -> Script {
        Script {
            code_hash: SUDT_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", "11".repeat(20)),
        }
    }

    fn u128_data_hex(amount: u128) -> String {
        format!("0x{}", hex::encode(amount.to_le_bytes()))
    }

    fn bulk_build_token_transfer_fixture() -> BlockResponseWithCycles {
        let lock_a_args = format!("0x{}", "01".repeat(20));
        let lock_b_args = format!("0x{}", "02".repeat(20));
        let sudt_type = fixture_sudt_type_script();
        let create_tx = TransactionView {
            hash: format!("0x{}", "c1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: Some(sudt_type.clone()),
            }],
            outputs_data: vec![u128_data_hex(200)],
            witnesses: vec!["0x".to_string()],
        };

        let split_tx = TransactionView {
            hash: format!("0x{}", "d2".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_a_args),
                    type_: Some(sudt_type.clone()),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_b_args),
                    type_: Some(sudt_type),
                },
            ],
            outputs_data: vec![u128_data_hex(100), u128_data_hex(100)],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_889, 0x9a),
                uncles: vec![],
                transactions: vec![create_tx, split_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    #[test]
    fn build_history_rows_materializes_addr_txs_for_unique_touched_locks() {
        let block = bulk_build_addr_tx_fixture();
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let addr_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .into_iter()
            .filter(|row| row.cf_name == CF_ADDR_TXS)
            .collect();

        let expected = [
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 0, &create_tx_hash),
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 1, &split_tx_hash),
            keys::encode_addr_tx_key(&lock_b_hash, 14_000_888, 1, &split_tx_hash),
        ];

        assert_eq!(addr_rows.len(), expected.len());
        let actual_keys: HashSet<Vec<u8>> = addr_rows.iter().map(|row| row.key.clone()).collect();
        assert_eq!(actual_keys.len(), expected.len());
        for key in expected {
            assert!(actual_keys.contains(&key));
        }
    }

    #[test]
    fn build_history_rows_materializes_token_transfer_records_in_tx_order() {
        let block = bulk_build_token_transfer_fixture();
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));
        let type_hash = ScriptParser::compute_script_hash(&fixture_sudt_type_script());
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let token_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .into_iter()
            .filter(|row| row.cf_name == CF_TOKEN_TRANSFERS)
            .collect();

        assert_eq!(token_rows.len(), 2);
        let token_records: HashMap<Vec<u8>, TokenTransferRecord> = token_rows
            .into_iter()
            .map(|row| {
                (
                    row.key,
                    bincode::deserialize(&row.value).expect("deserialize token transfer"),
                )
            })
            .collect();

        let mint_key = keys::encode_token_transfer_key(&type_hash, 14_000_889, 0);
        let transfer_key = keys::encode_token_transfer_key(&type_hash, 14_000_889, 1);
        let mint = token_records.get(&mint_key).expect("mint transfer");
        assert_eq!(mint.tx_hash, create_tx_hash);
        assert_eq!(mint.block_number, 14_000_889);
        assert_eq!(mint.from_lock_hash, None);
        assert_eq!(mint.to_lock_hash, lock_a_hash);
        assert_eq!(mint.amount, 200);
        assert!(mint.is_mint);
        assert!(!mint.is_burn);

        let transfer = token_records.get(&transfer_key).expect("split transfer");
        assert_eq!(transfer.tx_hash, split_tx_hash);
        assert_eq!(transfer.block_number, 14_000_889);
        assert_eq!(transfer.from_lock_hash, Some(lock_a_hash));
        assert_eq!(transfer.to_lock_hash, lock_b_hash);
        assert_eq!(transfer.amount, 100);
        assert!(!transfer.is_mint);
        assert!(!transfer.is_burn);
    }

    #[test]
    fn build_history_rows_materializes_ckb_activity_bundles_in_tx_order() {
        let block = bulk_build_addr_tx_fixture();
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let activity_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .into_iter()
            .filter(|row| row.cf_name == CF_ACTIVITIES)
            .collect();

        assert_eq!(activity_rows.len(), 2);
        let activity_bundles: HashMap<Vec<u8>, TxActivityBundle> = activity_rows
            .into_iter()
            .map(|row| {
                (
                    row.key,
                    bincode::deserialize(&row.value).expect("deserialize tx activity bundle"),
                )
            })
            .collect();

        let create_key = keys::encode_tx_activity_bundle_key(14_000_888, 0, &create_tx_hash);
        let split_key = keys::encode_tx_activity_bundle_key(14_000_888, 1, &split_tx_hash);
        let create_bundle = activity_bundles.get(&create_key).expect("cellbase bundle");
        assert_eq!(create_bundle.tx_hash, create_tx_hash);
        assert!(create_bundle.is_cellbase);
        assert_eq!(create_bundle.owners.len(), 1);

        let split_bundle = activity_bundles.get(&split_key).expect("split bundle");
        assert_eq!(split_bundle.tx_hash, split_tx_hash);
        assert!(!split_bundle.is_cellbase);
        assert_eq!(split_bundle.owners.len(), 2);

        let owner_a = split_bundle
            .owners
            .iter()
            .find(|owner| owner.lock_hash == lock_a_hash)
            .expect("owner a");
        assert_eq!(owner_a.ckb_delta, -100_00000000);
        assert!(owner_a.asset_changes.is_empty());
        assert_eq!(owner_a.peers, vec![lock_b_hash.clone()]);

        let owner_b = split_bundle
            .owners
            .iter()
            .find(|owner| owner.lock_hash == lock_b_hash)
            .expect("owner b");
        assert_eq!(owner_b.ckb_delta, 100_00000000);
        assert!(owner_b.asset_changes.is_empty());
        assert_eq!(owner_b.peers, vec![lock_a_hash]);
    }
}
