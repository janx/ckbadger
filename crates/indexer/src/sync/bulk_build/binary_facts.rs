//! Binary-native CKB block facts extractor.
//!
//! Reads `ckb_types::core::BlockView` directly into `FactsArena` structs,
//! bypassing the hex string roundtrip of the RPC-based path
//! (`block_view_to_rpc()` → `parse_single_block()`).
//!
//! The output is byte-identical to `parse_single_block()` in `pipeline.rs`.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{anyhow, Result};
use ckb_types::prelude::*;
use tracing::warn;

use crate::parser::cell::ParsedCell;
use crate::parser::dao::{DaoParser, DaoState};
use crate::parser::dotbit::{DotbitParser, DotbitWitnessBundle};
use crate::parser::mnft::MnftParser;
use crate::parser::script::ScriptParser;
use crate::parser::spore::SporeParser;
use crate::parser::udt::{UdtParser, UdtStandard};
use crate::sync::dao_helpers::occupied_capacity_shannons_i64;

use super::facts::{
    BlockFacts, CellFacts, CellProtocolFacts, CellSemanticTag, ClusterProtocolFacts, DaoCellState,
    DotbitProtocolFacts, MnftClassProtocolFacts, MnftIssuerProtocolFacts, MnftTokenProtocolFacts,
    OutPointKey, SporeProtocolFacts, TxFacts,
};
use super::interner::IdentityInterner;

use crate::sync::helpers::checked_usize_to_i16;
use crate::sync::token_helpers::parse_parsed_cell_udt_amount;

// ---------------------------------------------------------------------------
// Public container
// ---------------------------------------------------------------------------

/// A CKB block read directly from the node's RocksDB (molecule types),
/// paired with optional per-tx cycle counts.
pub(crate) struct RawCkbBlock {
    pub block: ckb_types::core::BlockView,
    pub cycles: Vec<Option<u64>>,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Parse a binary `RawCkbBlock` into the same `(BlockFacts, Vec<TxFacts>, Vec<CellFacts>)`
/// triple that `parse_single_block()` produces from hex RPC data.
///
/// Output ranges start at 0 and are remapped by the caller.
pub(crate) fn parse_block_to_facts(
    raw: &RawCkbBlock,
    interner: &IdentityInterner,
) -> Result<(BlockFacts, Vec<TxFacts>, Vec<CellFacts>)> {
    let header = raw.block.header();

    // -- Block-level fields --------------------------------------------------

    // core::HeaderView provides unpacked accessors directly (no .raw() needed).
    let block_number_u64: u64 = header.number();
    let block_number = i64::try_from(block_number_u64).map_err(|_| {
        anyhow!(
            "binary facts block number exceeds i64 range: {}",
            block_number_u64
        )
    })?;

    let block_hash: [u8; 32] = header.hash().unpack();

    let timestamp_ms_u64: u64 = header.timestamp();
    let timestamp_ms = i64::try_from(timestamp_ms_u64).map_err(|_| {
        anyhow!(
            "binary facts timestamp exceeds i64 range: block={} ts={}",
            block_number,
            timestamp_ms_u64
        )
    })?;

    let epoch = header.epoch();
    let epoch_number = i64::try_from(epoch.number()).map_err(|_| {
        anyhow!(
            "binary facts epoch number exceeds i64 range: block={} epoch={}",
            block_number,
            epoch.number()
        )
    })?;
    let epoch_index = i32::try_from(epoch.index()).map_err(|_| {
        anyhow!(
            "binary facts epoch index exceeds i32 range: block={} index={}",
            block_number,
            epoch.index()
        )
    })?;
    let epoch_length = i32::try_from(epoch.length()).map_err(|_| {
        anyhow!(
            "binary facts epoch length exceeds i32 range: block={} length={}",
            block_number,
            epoch.length()
        )
    })?;

    let dao: [u8; 32] = header.dao().unpack();
    let compact_target: u32 = header.compact_target();
    let uncles_count = i32::try_from(raw.block.uncle_hashes().len()).map_err(|_| {
        anyhow!(
            "binary facts uncles count exceeds i32 range: block={} uncles={}",
            block_number,
            raw.block.uncle_hashes().len()
        )
    })?;

    let block_dao_ar = DaoParser::extract_ar_from_dao_field(&dao).ok_or_else(|| {
        anyhow!(
            "failed to extract block DAO AR in binary facts: block={}, dao_len={}",
            block_number,
            dao.len()
        )
    })?;

    let transactions = raw.block.transactions();
    let transactions_count = i32::try_from(transactions.len()).map_err(|_| {
        anyhow!(
            "binary facts transactions count exceeds i32 range: block={} count={}",
            block_number,
            transactions.len()
        )
    })?;

    // -- Per-tx iteration ----------------------------------------------------

    let mut local_txs = Vec::with_capacity(transactions.len());
    let mut local_cells = Vec::new();

    for (tx_position, tx) in transactions.iter().enumerate() {
        let tx_hash: [u8; 32] = tx.hash().unpack();
        let tx_data = tx.data();
        let raw_tx = tx_data.raw();
        let inputs = raw_tx.inputs();
        let outputs = raw_tx.outputs();
        let outputs_data = raw_tx.outputs_data();

        // Validate outputs == outputs_data
        if outputs.len() != outputs_data.len() {
            return Err(anyhow!(
                "binary facts outputs/outputs_data length mismatch: block={} tx=0x{} outputs={} outputs_data={}",
                block_number,
                hex::encode(tx_hash),
                outputs.len(),
                outputs_data.len()
            ));
        }

        let tx_index = i32::try_from(tx_position).map_err(|_| {
            anyhow!(
                "binary facts tx index exceeds i32 range: block={} tx_position={}",
                block_number,
                tx_position
            )
        })?;

        let is_cellbase = tx_position == 0;

        let inputs_count_usize = inputs.len();
        let inputs_count = i16::try_from(inputs_count_usize).map_err(|_| {
            anyhow!(
                "binary facts inputs_count exceeds i16 range: block={} tx=0x{} tx_index={} inputs_count={}",
                block_number,
                hex::encode(tx_hash),
                tx_index,
                inputs_count_usize
            )
        })?;

        let outputs_count_usize = outputs.len();
        let outputs_count = i16::try_from(outputs_count_usize).map_err(|_| {
            anyhow!(
                "binary facts outputs_count exceeds i16 range: block={} tx=0x{} tx_index={} outputs_count={}",
                block_number,
                hex::encode(tx_hash),
                tx_index,
                outputs_count_usize
            )
        })?;

        let tx_size = i32::try_from(tx_data.as_slice().len()).map_err(|_| {
            anyhow!(
                "binary facts tx_size exceeds i32 range: block={} tx=0x{} size={}",
                block_number,
                hex::encode(tx_hash),
                tx_data.as_slice().len()
            )
        })?;

        let cycles = parse_binary_tx_cycles(raw, tx_position, block_number, &tx_hash)?;

        // -- Witness handling (for DotBit) -----------------------------------

        let witnesses = tx_data.witnesses();
        let witness_bundle = parse_binary_dotbit_witnesses(&witnesses);

        // -- Input outpoints -------------------------------------------------

        let input_outpoints = if is_cellbase {
            Vec::new()
        } else {
            inputs
                .into_iter()
                .map(|input| {
                    let prev_out = input.previous_output();
                    let prev_tx_hash: [u8; 32] = prev_out.tx_hash().unpack();
                    let prev_index: u32 = prev_out.index().unpack();
                    Ok(OutPointKey::new(prev_tx_hash, prev_index))
                })
                .collect::<Result<Vec<_>>>()?
        };

        // -- Outputs → CellFacts --------------------------------------------

        let output_start = local_cells.len();

        for output_index in 0..outputs.len() {
            let output = outputs.get(output_index).unwrap();
            let data_bytes = outputs_data.get(output_index).unwrap().raw_data();
            let data: Vec<u8> = data_bytes.to_vec();

            let output_index_i16 =
                checked_usize_to_i16(output_index, "binary facts arena output index")?;

            // -- Lock script -------------------------------------------------

            let lock = output.lock();
            let lock_code_hash_bytes: [u8; 32] = lock.code_hash().unpack();
            let lock_hash_type_byte: u8 = lock.hash_type().into();
            let lock_hash_type = lock_hash_type_byte as i16;
            let lock_args_bytes: Vec<u8> = lock.args().raw_data().to_vec();
            let lock_script_hash: [u8; 32] = lock.calc_script_hash().unpack();

            // -- Type script (optional) --------------------------------------

            let type_opt = output.type_().to_opt();
            let (type_code_hash_bytes, type_hash_type, type_args_bytes, type_script_hash) =
                if let Some(ref type_script) = type_opt {
                    let code_hash: [u8; 32] = type_script.code_hash().unpack();
                    let ht_byte: u8 = type_script.hash_type().into();
                    let args: Vec<u8> = type_script.args().raw_data().to_vec();
                    let script_hash: [u8; 32] = type_script.calc_script_hash().unpack();
                    (
                        Some(code_hash),
                        Some(ht_byte as i16),
                        Some(args),
                        Some(script_hash),
                    )
                } else {
                    (None, None, None, None)
                };

            // -- Capacity ----------------------------------------------------

            let capacity_u64: u64 = output.capacity().unpack();
            let capacity = i64::try_from(capacity_u64).map_err(|_| {
                anyhow!(
                    "binary facts capacity exceeds i64 range: block={} tx=0x{} output_index={} capacity={}",
                    block_number,
                    hex::encode(tx_hash),
                    output_index,
                    capacity_u64
                )
            })?;

            // -- Data hash ---------------------------------------------------

            let data_hash = ScriptParser::compute_data_hash(&data);

            // -- Data size ---------------------------------------------------

            let data_size = i32::try_from(data.len()).map_err(|_| {
                anyhow!(
                    "binary facts data size exceeds i32 range: block={} tx=0x{} output_index={} data_len={}",
                    block_number,
                    hex::encode(tx_hash),
                    output_index,
                    data.len()
                )
            })?;

            // -- Build ParsedCell for classification/protocol parsing ---------

            let parsed_cell = ParsedCell {
                capacity,
                lock_code_hash: lock_code_hash_bytes.to_vec(),
                lock_hash_type,
                lock_args: lock_args_bytes.clone(),
                lock_script_hash: lock_script_hash.to_vec(),
                type_code_hash: type_code_hash_bytes.map(|b| b.to_vec()),
                type_hash_type,
                type_args: type_args_bytes.clone(),
                type_script_hash: type_script_hash.map(|b| b.to_vec()),
                data_hash,
                data_size,
                data: data.clone(),
            };

            // -- Semantic tag ------------------------------------------------

            let semantic_tag = classify_bulk_cell_semantic_tag(&parsed_cell);

            // -- Occupied capacity -------------------------------------------

            let occupied_capacity = occupied_capacity_shannons_i64(
                lock_args_bytes.len(),
                type_args_bytes.as_ref().map(|a| a.len()),
                data_size,
            );

            // -- Interned identities -----------------------------------------

            let lock_script_hash_id = interner.intern_bytes(lock_script_hash.to_vec());
            let lock_code_hash_id = interner.intern_bytes(lock_code_hash_bytes.to_vec());
            let lock_args_id = interner.intern_bytes(lock_args_bytes);
            let type_script_hash_id = type_script_hash.map(|v| interner.intern_bytes(v.to_vec()));
            let type_code_hash_id = type_code_hash_bytes.map(|v| interner.intern_bytes(v.to_vec()));
            let type_args_id = type_args_bytes.map(|v| interner.intern_bytes(v));

            // -- UDT amount --------------------------------------------------

            let udt_amount =
                parse_parsed_cell_udt_amount(&parsed_cell, &tx_hash, output_index_i16, None)?;

            // -- DAO state ---------------------------------------------------

            let dao_state = parse_binary_dao_cell_state(
                &parsed_cell,
                semantic_tag,
                &tx_hash,
                output_index_i16,
            )?;

            // -- Protocol facts ----------------------------------------------

            let protocol_facts = parse_binary_protocol_facts(
                &parsed_cell,
                semantic_tag,
                &witness_bundle,
                &tx_hash,
                output_index_i16,
            )?;

            local_cells.push(CellFacts {
                outpoint: OutPointKey::new(
                    tx_hash,
                    u32::try_from(output_index).unwrap_or_else(|_| {
                        panic!(
                            "binary facts arena output index {} exceeds u32::MAX",
                            output_index
                        )
                    }),
                ),
                created_at_block: block_number,
                created_by_block_dao_ar: block_dao_ar,
                capacity,
                lock_script_hash_id,
                lock_code_hash_id,
                lock_hash_type,
                lock_args_id,
                type_script_hash_id,
                type_code_hash_id,
                type_hash_type,
                type_args_id,
                occupied_capacity,
                data_size,
                data,
                data_hash: Some(data_hash),
                udt_amount,
                semantic_tag,
                dao_state,
                protocol_facts,
            });
        }

        local_txs.push(TxFacts {
            hash: tx_hash,
            block_number,
            block_hash,
            timestamp_ms,
            block_dao_ar,
            tx_index,
            is_cellbase,
            inputs_count,
            outputs_count,
            tx_size,
            cycles,
            dotbit_action: witness_bundle.action.clone(),
            input_outpoints,
            output_range: output_start..local_cells.len(),
        });
    }

    let block_facts = BlockFacts {
        number: block_number,
        hash: block_hash,
        timestamp_ms,
        epoch_number,
        epoch_index,
        epoch_length,
        dao,
        compact_target,
        uncles_count,
        transactions_count,
        // Placeholder tx_range; remapped in the merge phase.
        tx_range: 0..local_txs.len(),
    };

    Ok((block_facts, local_txs, local_cells))
}

// ---------------------------------------------------------------------------
// Cycles extraction
// ---------------------------------------------------------------------------

fn parse_binary_tx_cycles(
    raw: &RawCkbBlock,
    tx_position: usize,
    block_number: i64,
    tx_hash: &[u8; 32],
) -> Result<Option<i64>> {
    if raw.cycles.is_empty() {
        return Ok(None);
    }

    // Cellbase (tx_position 0) has no cycles
    if tx_position == 0 {
        return Ok(None);
    }

    let tx_count = raw.block.transactions().len();
    let expected_len = tx_count.saturating_sub(1);
    if raw.cycles.len() != expected_len {
        return Err(anyhow!(
            "binary facts cycles length mismatch: block={} tx_count={} expected_cycles={} actual_cycles={}",
            block_number,
            tx_count,
            expected_len,
            raw.cycles.len()
        ));
    }

    // cycles[0] corresponds to tx_position 1 (first non-cellbase tx)
    let cycles_index = tx_position - 1;
    let raw_cycles = raw.cycles.get(cycles_index).ok_or_else(|| {
        anyhow!(
            "binary facts cycles missing tx position: block={} tx=0x{} tx_position={} cycles_index={} cycles_count={}",
            block_number,
            hex::encode(tx_hash),
            tx_position,
            cycles_index,
            raw.cycles.len()
        )
    })?;

    match raw_cycles {
        Some(c) => {
            let cycles_i64 = i64::try_from(*c).map_err(|_| {
                anyhow!(
                    "binary facts cycles exceed i64 range: block={} tx=0x{} tx_position={} cycles={}",
                    block_number,
                    hex::encode(tx_hash),
                    tx_position,
                    c
                )
            })?;
            Ok(Some(cycles_i64))
        }
        None => Ok(Some(0)),
    }
}

// ---------------------------------------------------------------------------
// Witness handling (DotBit)
// ---------------------------------------------------------------------------

/// DAS witness magic bytes: b"das" = [0x64, 0x61, 0x73]
const DAS_MAGIC: &[u8; 3] = b"das";
const DAS_WITNESS_HEADER_LEN: usize = 7; // "das"(3) + action_data_type(4)

/// Check if a binary witness starts with the DAS magic prefix.
fn witness_has_das_binary_prefix(witness_bytes: &[u8]) -> bool {
    witness_bytes.len() > DAS_WITNESS_HEADER_LEN && witness_bytes.starts_with(DAS_MAGIC)
}

/// Parse DotBit witness bundle from binary witnesses.
///
/// The existing `parse_dotbit_witness_bundle` expects hex strings, so we convert
/// witness bytes to hex only for witnesses that contain the DAS prefix. This keeps
/// the hot path (non-DotBit witnesses, which are the vast majority) allocation-free.
fn parse_binary_dotbit_witnesses(witnesses: &ckb_types::packed::BytesVec) -> DotbitWitnessBundle {
    // Quick check: does any witness have the DAS prefix in binary?
    let mut has_das = false;
    for i in 0..witnesses.len() {
        let w = witnesses.get(i).unwrap();
        let raw = w.raw_data();
        if witness_has_das_binary_prefix(&raw) {
            has_das = true;
            break;
        }
    }

    if !has_das {
        return DotbitWitnessBundle::default();
    }

    // Convert witnesses to hex strings and delegate to the existing parser.
    let hex_witnesses: Vec<String> = (0..witnesses.len())
        .map(|i| {
            let w = witnesses.get(i).unwrap();
            format!("0x{}", hex::encode(w.raw_data()))
        })
        .collect();

    crate::parser::dotbit::parse_dotbit_witness_bundle(&hex_witnesses)
}

// ---------------------------------------------------------------------------
// O(1) semantic tag lookup table
// ---------------------------------------------------------------------------

/// Raw code_hash tag before hash_type refinement.
#[derive(Debug, Clone, Copy)]
enum CodeHashTag {
    Dao,
    Sudt,      // requires hash_type == 1
    XudtData1, // requires hash_type == 2
    XudtType,  // requires hash_type == 1
    Dotbit,
    MnftIssuer,
    MnftClass,
    MnftToken,
    SporeNft,
    SporeDid,
    Cluster,
}

fn parse_code_hash_32(hex_str: &str) -> [u8; 32] {
    let bytes = crate::rpc::parse_hex_to_bytes(hex_str);
    bytes.try_into().expect("code hash must be 32 bytes")
}

static CODE_HASH_TAG_MAP: LazyLock<HashMap<[u8; 32], CodeHashTag>> = LazyLock::new(|| {
    use crate::parser::dao::DAO_CODE_HASH;
    use crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID;
    use crate::parser::mnft::{MNFT_CLASS_CODE_HASH, MNFT_ISSUER_CODE_HASH, MNFT_TOKEN_CODE_HASH};
    use crate::parser::spore::{
        CLUSTER_CODE_HASH_MAINNET_V2, CLUSTER_CODE_HASH_TESTNET_V1, CLUSTER_CODE_HASH_TESTNET_V2,
        SPORE_CODE_HASH_MAINNET_DID, SPORE_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_TESTNET_V1,
        SPORE_CODE_HASH_TESTNET_V2,
    };
    use crate::parser::udt::{SUDT_CODE_HASH, XUDT_CODE_HASH_DATA1, XUDT_CODE_HASH_TYPE};

    let mut m = HashMap::with_capacity(16);
    m.insert(parse_code_hash_32(DAO_CODE_HASH), CodeHashTag::Dao);
    m.insert(parse_code_hash_32(SUDT_CODE_HASH), CodeHashTag::Sudt);
    m.insert(
        parse_code_hash_32(XUDT_CODE_HASH_DATA1),
        CodeHashTag::XudtData1,
    );
    m.insert(
        parse_code_hash_32(XUDT_CODE_HASH_TYPE),
        CodeHashTag::XudtType,
    );
    m.insert(
        parse_code_hash_32(DOTBIT_ACCOUNT_CELL_TYPE_ID),
        CodeHashTag::Dotbit,
    );
    m.insert(
        parse_code_hash_32(MNFT_ISSUER_CODE_HASH),
        CodeHashTag::MnftIssuer,
    );
    m.insert(
        parse_code_hash_32(MNFT_CLASS_CODE_HASH),
        CodeHashTag::MnftClass,
    );
    m.insert(
        parse_code_hash_32(MNFT_TOKEN_CODE_HASH),
        CodeHashTag::MnftToken,
    );
    m.insert(
        parse_code_hash_32(SPORE_CODE_HASH_MAINNET_V2),
        CodeHashTag::SporeNft,
    );
    m.insert(
        parse_code_hash_32(SPORE_CODE_HASH_MAINNET_DID),
        CodeHashTag::SporeDid,
    );
    m.insert(
        parse_code_hash_32(SPORE_CODE_HASH_TESTNET_V2),
        CodeHashTag::SporeNft,
    );
    m.insert(
        parse_code_hash_32(SPORE_CODE_HASH_TESTNET_V1),
        CodeHashTag::SporeNft,
    );
    m.insert(
        parse_code_hash_32(CLUSTER_CODE_HASH_MAINNET_V2),
        CodeHashTag::Cluster,
    );
    m.insert(
        parse_code_hash_32(CLUSTER_CODE_HASH_TESTNET_V2),
        CodeHashTag::Cluster,
    );
    m.insert(
        parse_code_hash_32(CLUSTER_CODE_HASH_TESTNET_V1),
        CodeHashTag::Cluster,
    );
    m
});

/// Classify a cell's semantic tag from raw code_hash bytes and hash_type.
/// O(1) HashMap lookup replaces the sequential if-chain.
#[allow(dead_code)] // Will be wired in Task 2
fn classify_semantic_tag_from_code_hash(
    type_code_hash: Option<&[u8; 32]>,
    type_hash_type: Option<i16>,
) -> CellSemanticTag {
    let Some(code_hash) = type_code_hash else {
        return CellSemanticTag::Plain;
    };
    let Some(&raw_tag) = CODE_HASH_TAG_MAP.get(code_hash) else {
        return CellSemanticTag::Plain;
    };
    let hash_type = type_hash_type.unwrap_or(-1);
    match raw_tag {
        // DAO does not enforce hash_type — matches existing behavior in classify_bulk_cell_semantic_tag
        CodeHashTag::Dao => CellSemanticTag::Dao,
        CodeHashTag::Sudt if hash_type == 1 => CellSemanticTag::Sudt,
        CodeHashTag::XudtData1 if hash_type == 2 => CellSemanticTag::Xudt,
        CodeHashTag::XudtType if hash_type == 1 => CellSemanticTag::Xudt,
        CodeHashTag::Dotbit => CellSemanticTag::Dotbit,
        CodeHashTag::MnftIssuer | CodeHashTag::MnftClass | CodeHashTag::MnftToken => {
            CellSemanticTag::Mnft
        }
        CodeHashTag::SporeNft | CodeHashTag::SporeDid => CellSemanticTag::Spore,
        CodeHashTag::Cluster => CellSemanticTag::Cluster,
        // hash_type mismatch for UDT entries
        _ => CellSemanticTag::Plain,
    }
}

// ---------------------------------------------------------------------------
// Cell semantic classification (identical to pipeline.rs classify_bulk_cell_semantic_tag)
// ---------------------------------------------------------------------------

fn classify_bulk_cell_semantic_tag(cell: &ParsedCell) -> CellSemanticTag {
    let Some(type_code_hash) = cell.type_code_hash.as_deref() else {
        return CellSemanticTag::Plain;
    };

    if DaoParser::is_dao_code_hash(type_code_hash) {
        return CellSemanticTag::Dao;
    }

    if let Some(hash_type) = cell.type_hash_type {
        if let Some(standard) = UdtParser::is_udt_code_hash_bytes(type_code_hash, hash_type) {
            return match standard {
                UdtStandard::Sudt => CellSemanticTag::Sudt,
                UdtStandard::Xudt => CellSemanticTag::Xudt,
            };
        }
    }

    if DotbitParser::is_account_cell_type_script(type_code_hash) {
        return CellSemanticTag::Dotbit;
    }

    if MnftParser::is_issuer_type_script(type_code_hash)
        || MnftParser::is_class_type_script(type_code_hash)
        || MnftParser::is_token_type_script(type_code_hash)
    {
        return CellSemanticTag::Mnft;
    }

    if SporeParser::is_cluster_type_script(type_code_hash) {
        return CellSemanticTag::Cluster;
    }

    if SporeParser::is_spore_type_script(type_code_hash) {
        return CellSemanticTag::Spore;
    }

    CellSemanticTag::Plain
}

// ---------------------------------------------------------------------------
// DAO cell state (identical to pipeline.rs parse_bulk_dao_cell_state)
// ---------------------------------------------------------------------------

fn parse_binary_dao_cell_state(
    cell: &ParsedCell,
    semantic_tag: CellSemanticTag,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<DaoCellState>> {
    if !matches!(semantic_tag, CellSemanticTag::Dao) {
        return Ok(None);
    }

    let state = DaoParser::parse_dao_state(&cell.data).ok_or_else(|| {
        anyhow!(
            "invalid DAO cell data in binary facts: tx=0x{}, output_index={}, data_len={}",
            hex::encode(tx_hash),
            output_index,
            cell.data.len()
        )
    })?;

    Ok(Some(match state {
        DaoState::Deposit => DaoCellState::Deposit,
        DaoState::WithdrawRequest => {
            let deposit_block_number =
                DaoParser::parse_deposit_block_number(&cell.data).ok_or_else(|| {
                    anyhow!(
                        "missing DAO deposit block number in withdraw request: tx=0x{}, output_index={}, data_len={}",
                        hex::encode(tx_hash),
                        output_index,
                        cell.data.len()
                    )
                })?;
            DaoCellState::WithdrawRequest {
                deposit_block_number: i64::try_from(deposit_block_number).map_err(|_| {
                    anyhow!(
                        "DAO deposit block number exceeds i64 range in binary facts: tx=0x{}, output_index={}, deposit_block_number={}",
                        hex::encode(tx_hash),
                        output_index,
                        deposit_block_number
                    )
                })?,
            }
        }
    }))
}

// ---------------------------------------------------------------------------
// Protocol ID helpers (identical to pipeline.rs)
// ---------------------------------------------------------------------------

fn parse_fixed_protocol_id<const N: usize>(
    bytes: &[u8],
    label: &str,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<[u8; N]> {
    bytes.try_into().map_err(|_| {
        anyhow!(
            "invalid {} length in binary facts: tx=0x{}, output_index={}, expected={}, actual={}",
            label,
            hex::encode(tx_hash),
            output_index,
            N,
            bytes.len()
        )
    })
}

fn parse_optional_fixed_protocol_id<const N: usize>(
    bytes: Option<&Vec<u8>>,
    label: &str,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<[u8; N]>> {
    bytes
        .map(|value| parse_fixed_protocol_id::<N>(value, label, tx_hash, output_index))
        .transpose()
}

// ---------------------------------------------------------------------------
// Protocol facts (identical to pipeline.rs parse_bulk_protocol_facts)
// ---------------------------------------------------------------------------

fn parse_binary_protocol_facts(
    cell: &ParsedCell,
    semantic_tag: CellSemanticTag,
    witness_bundle: &DotbitWitnessBundle,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<CellProtocolFacts>> {
    match semantic_tag {
        CellSemanticTag::Plain
        | CellSemanticTag::Dao
        | CellSemanticTag::Sudt
        | CellSemanticTag::Xudt => Ok(None),
        CellSemanticTag::Spore => {
            let spore = SporeParser::parse_spore_parsed_cell(cell).ok_or_else(|| {
                anyhow!(
                    "failed to parse Spore cell semantics in binary facts: tx=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index
                )
            })?;
            Ok(Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                spore_id: parse_fixed_protocol_id::<32>(
                    &spore.spore_id,
                    "spore_id",
                    tx_hash,
                    output_index,
                )?,
                is_did: spore.is_did,
                content_type: spore.content_type,
                content: spore.content,
                cluster_id: parse_optional_fixed_protocol_id::<32>(
                    spore.cluster_id.as_ref(),
                    "spore cluster_id",
                    tx_hash,
                    output_index,
                )?,
            })))
        }
        CellSemanticTag::Cluster => {
            let cluster = SporeParser::parse_cluster_parsed_cell(cell).ok_or_else(|| {
                anyhow!(
                    "failed to parse Cluster cell semantics in binary facts: tx=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index
                )
            })?;
            Ok(Some(CellProtocolFacts::Cluster(ClusterProtocolFacts {
                cluster_id: parse_fixed_protocol_id::<32>(
                    &cluster.cluster_id,
                    "cluster_id",
                    tx_hash,
                    output_index,
                )?,
                name: cluster.name,
                description: cluster.description,
            })))
        }
        CellSemanticTag::Mnft => {
            if let Some(issuer) = MnftParser::parse_issuer_parsed_cell(cell) {
                return Ok(Some(CellProtocolFacts::MnftIssuer(
                    MnftIssuerProtocolFacts {
                        issuer_id: parse_fixed_protocol_id::<20>(
                            &issuer.issuer_id,
                            "mnft issuer_id",
                            tx_hash,
                            output_index,
                        )?,
                        name: issuer.name,
                        info: issuer.info,
                        class_count: issuer.class_count,
                        set_count: issuer.set_count,
                    },
                )));
            }

            if let Some(class) = MnftParser::parse_class_parsed_cell(cell) {
                return Ok(Some(CellProtocolFacts::MnftClass(MnftClassProtocolFacts {
                    class_id: class.class_id,
                    issuer_id: parse_fixed_protocol_id::<20>(
                        &class.issuer_id,
                        "mnft class issuer_id",
                        tx_hash,
                        output_index,
                    )?,
                    name: class.name,
                    description: class.description,
                    renderer: class.renderer,
                    total: class.total,
                    issued: class.issued,
                    configure: class.configure,
                })));
            }

            if let Some(token) = MnftParser::parse_token_parsed_cell(cell) {
                return Ok(Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id: token.token_id,
                    class_id: token.class_id,
                    token_index: token.token_index,
                    characteristic: token.characteristic,
                    configure: token.configure,
                    state: token.state,
                })));
            }

            Err(anyhow!(
                "failed to parse mNFT cell semantics in binary facts: tx=0x{}, output_index={}",
                hex::encode(tx_hash),
                output_index
            ))
        }
        CellSemanticTag::Dotbit => {
            let Some(mut account) = DotbitParser::parse_account_parsed_cell(cell) else {
                // Some on-chain DotBit AccountCells have minimal/edge-case data
                // (e.g. data < 52 bytes, all-zero account IDs) that the parser
                // cannot extract a valid account from. This is external data, not
                // an invariant violation -- skip the cell rather than crashing the
                // entire bulk sync.
                warn!(
                    tx = hex::encode(tx_hash),
                    output_index,
                    data_len = cell.data.len(),
                    "skipping unparseable DotBit AccountCell in binary facts"
                );
                return Ok(None);
            };

            if let Some(data) = witness_bundle.accounts.get(account.account_id.as_slice()) {
                account.account = data.name.clone();
                account.registered_at = data.registered_at;
                account.status = data.status;
            }

            if account.account.is_none() {
                // DAS witness may lack account name for some historical or
                // edge-case transactions. Skip rather than crash bulk sync.
                warn!(
                    tx = hex::encode(tx_hash),
                    output_index,
                    account_id = hex::encode(&account.account_id),
                    "skipping DotBit cell: account name missing in DAS witness"
                );
                return Ok(None);
            }

            Ok(Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                account_id: parse_fixed_protocol_id::<20>(
                    &account.account_id,
                    "dotbit account_id",
                    tx_hash,
                    output_index,
                )?,
                account: account.account,
                next_account_id: parse_optional_fixed_protocol_id::<20>(
                    account.next_account_id.as_ref(),
                    "dotbit next_account_id",
                    tx_hash,
                    output_index,
                )?,
                expired_at: account.expired_at,
                registered_at: account.registered_at,
                status: account.status,
            })))
        }
    }
}

// ---------------------------------------------------------------------------
// Arena builder
// ---------------------------------------------------------------------------

/// Build a FactsArena from raw CKB blocks using binary-native parsing.
pub(crate) fn build_bulk_facts_arena_from_raw_blocks(
    blocks: &[RawCkbBlock],
    interner: &IdentityInterner,
) -> Result<super::facts::FactsArena> {
    use rayon::prelude::*;

    #[allow(clippy::type_complexity)]
    let per_block_results: Vec<Result<(BlockFacts, Vec<TxFacts>, Vec<CellFacts>)>> = blocks
        .par_iter()
        .map(|raw| parse_block_to_facts(raw, interner))
        .collect();

    let mut arena = super::facts::FactsArena::default();
    for result in per_block_results {
        let (block_facts, txs, cells) = result?;
        let tx_start = arena.txs.len();
        let cell_start = arena.cells.len();

        for mut tx in txs {
            tx.output_range =
                (cell_start + tx.output_range.start)..(cell_start + tx.output_range.end);
            arena.txs.push(tx);
        }
        arena.cells.extend(cells);

        let tx_end = arena.txs.len();
        let mut block = block_facts;
        block.tx_range = tx_start..tx_end;
        arena.blocks.push(block);
    }
    Ok(arena)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn witness_has_das_binary_prefix_detects_das() {
        // "das" + 4 bytes action_data_type + at least 1 more byte
        let mut witness = b"das".to_vec();
        witness.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        witness.push(0xFF);
        assert!(witness_has_das_binary_prefix(&witness));
    }

    #[test]
    fn witness_has_das_binary_prefix_rejects_short() {
        let witness = b"das\x00\x00\x00\x00";
        assert!(!witness_has_das_binary_prefix(witness));
    }

    #[test]
    fn witness_has_das_binary_prefix_rejects_non_das() {
        let witness = b"xyz\x00\x00\x00\x00\xFF";
        assert!(!witness_has_das_binary_prefix(witness));
    }

    #[test]
    fn classify_plain_cell_without_type_script() {
        let cell = ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            lock_script_hash: vec![1; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: [0; 32],
            data_size: 0,
            data: vec![],
        };
        assert_eq!(
            classify_bulk_cell_semantic_tag(&cell),
            CellSemanticTag::Plain
        );
    }

    #[test]
    fn classify_dao_cell() {
        let dao_code_hash =
            hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e")
                .unwrap();
        let cell = ParsedCell {
            capacity: 102_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            lock_script_hash: vec![1; 32],
            type_code_hash: Some(dao_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![2; 32]),
            data_hash: [0; 32],
            data_size: 8,
            data: vec![0; 8],
        };
        assert_eq!(classify_bulk_cell_semantic_tag(&cell), CellSemanticTag::Dao);
    }

    #[test]
    fn parse_binary_dao_cell_state_recognizes_deposit() {
        let dao_code_hash =
            hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e")
                .unwrap();
        let cell = ParsedCell {
            capacity: 102_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            lock_script_hash: vec![1; 32],
            type_code_hash: Some(dao_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![2; 32]),
            data_hash: [0; 32],
            data_size: 8,
            data: vec![0; 8],
        };

        let state =
            parse_binary_dao_cell_state(&cell, CellSemanticTag::Dao, &[0xaa; 32], 0).unwrap();
        assert_eq!(state, Some(DaoCellState::Deposit));
    }

    #[test]
    fn parse_binary_dao_cell_state_recognizes_withdraw_request() {
        let dao_code_hash =
            hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e")
                .unwrap();
        let cell = ParsedCell {
            capacity: 102_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            lock_script_hash: vec![1; 32],
            type_code_hash: Some(dao_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![2; 32]),
            data_hash: [0; 32],
            data_size: 8,
            data: 123u64.to_le_bytes().to_vec(),
        };

        let state =
            parse_binary_dao_cell_state(&cell, CellSemanticTag::Dao, &[0xaa; 32], 0).unwrap();
        assert_eq!(
            state,
            Some(DaoCellState::WithdrawRequest {
                deposit_block_number: 123
            })
        );
    }

    #[test]
    fn parse_binary_dao_cell_state_skips_non_dao() {
        let cell = ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            lock_script_hash: vec![1; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: [0; 32],
            data_size: 0,
            data: vec![],
        };

        let state =
            parse_binary_dao_cell_state(&cell, CellSemanticTag::Plain, &[0xaa; 32], 0).unwrap();
        assert_eq!(state, None);
    }

    #[test]
    fn parse_fixed_protocol_id_correct_length() {
        let bytes = [0xAB; 32];
        let result: [u8; 32] = parse_fixed_protocol_id(&bytes, "test_id", &[0; 32], 0).unwrap();
        assert_eq!(result, bytes);
    }

    #[test]
    fn parse_fixed_protocol_id_wrong_length_fails() {
        let bytes = [0xAB; 16];
        let result = parse_fixed_protocol_id::<32>(&bytes, "test_id", &[0; 32], 0);
        assert!(result.is_err());
    }

    #[test]
    fn parse_optional_fixed_protocol_id_none() {
        let result = parse_optional_fixed_protocol_id::<32>(None, "test_id", &[0; 32], 0).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_optional_fixed_protocol_id_some_correct() {
        let bytes = vec![0xCD; 20];
        let result =
            parse_optional_fixed_protocol_id::<20>(Some(&bytes), "test_id", &[0; 32], 0).unwrap();
        assert_eq!(result, Some([0xCD; 20]));
    }

    // -----------------------------------------------------------------------
    // classify_semantic_tag_from_code_hash tests
    // -----------------------------------------------------------------------

    #[test]
    fn classify_from_code_hash_plain_without_type() {
        assert_eq!(
            classify_semantic_tag_from_code_hash(None, None),
            CellSemanticTag::Plain,
        );
    }

    #[test]
    fn classify_from_code_hash_dao() {
        let hash = parse_code_hash_32(crate::parser::dao::DAO_CODE_HASH);
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Dao,
        );
    }

    #[test]
    fn classify_from_code_hash_sudt() {
        let hash = parse_code_hash_32(crate::parser::udt::SUDT_CODE_HASH);
        // hash_type 1 → Sudt
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Sudt,
        );
        // hash_type 0 → Plain (mismatch)
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(0)),
            CellSemanticTag::Plain,
        );
    }

    #[test]
    fn classify_from_code_hash_xudt_data1() {
        let hash = parse_code_hash_32(crate::parser::udt::XUDT_CODE_HASH_DATA1);
        // hash_type 2 → Xudt
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(2)),
            CellSemanticTag::Xudt,
        );
        // hash_type 1 → Plain (mismatch)
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Plain,
        );
    }

    #[test]
    fn classify_from_code_hash_xudt_type() {
        let hash = parse_code_hash_32(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Xudt,
        );
    }

    #[test]
    fn classify_from_code_hash_spore_mainnet_v2() {
        let hash = parse_code_hash_32(crate::parser::spore::SPORE_CODE_HASH_MAINNET_V2);
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Spore,
        );
    }

    #[test]
    fn classify_from_code_hash_spore_did() {
        let hash = parse_code_hash_32(
            "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33",
        );
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Spore,
        );
    }

    #[test]
    fn classify_from_code_hash_cluster() {
        let hash = parse_code_hash_32(crate::parser::spore::CLUSTER_CODE_HASH_MAINNET_V2);
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Cluster,
        );
    }

    #[test]
    fn classify_from_code_hash_dotbit() {
        let hash = parse_code_hash_32(
            "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918",
        );
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Dotbit,
        );
    }

    #[test]
    fn classify_from_code_hash_mnft_issuer() {
        let hash = parse_code_hash_32(
            "0x24b04faf80ded836efc05247778eec4ec02548dab6e2012c0107374aa3f68b81",
        );
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Mnft,
        );
    }

    #[test]
    fn classify_from_code_hash_unknown_type_is_plain() {
        let hash = [0xFF; 32];
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Plain,
        );
    }
}
