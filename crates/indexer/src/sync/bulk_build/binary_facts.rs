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

use crate::parser::cell::ParsedCell;
use crate::parser::dao::{DaoParser, DaoState};
use crate::parser::dotbit::DotbitWitnessBundle;
use crate::parser::script::ScriptParser;
use crate::parser::udt::UdtParser;
use crate::sync::dao_helpers::occupied_capacity_shannons_i64;

use super::facts::{BlockFacts, CellFacts, CellSemanticTag, DaoCellState, OutPointKey, TxFacts};
use super::interner::IdentityInterner;

use crate::sync::helpers::checked_usize_to_i16;

// ---------------------------------------------------------------------------
// Timing breakdown
// ---------------------------------------------------------------------------

/// Per-batch timing breakdown for the Facts phase.
/// Returned alongside `FactsArena` to decompose `facts_ms` into parallel vs serial components.
#[derive(Debug, Default, Clone)]
pub(crate) struct FactsTimingBreakdown {
    /// Wall-clock time of the rayon par_iter phase (ms).
    pub par_iter_ms: f64,
    /// Wall-clock time of the serial arena merge phase (ms).
    pub merge_ms: f64,
    /// Sum of per-block parse times across all rayon threads (ms).
    /// `serial_equivalent_ms / par_iter_ms` = actual speedup ratio.
    pub serial_equivalent_ms: f64,
    /// Number of `intern_bytes` calls that took the Mutex slow path.
    pub intern_slow_path_count: u64,
    /// Total number of `intern_bytes` calls.
    pub intern_total_count: u64,
    /// Total cells parsed in this batch.
    pub cell_count: u64,
}

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
    let parent_hash: [u8; 32] = header.parent_hash().unpack();

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

            // -- Semantic tag (O(1) lookup from raw code_hash bytes) -----------

            let semantic_tag =
                classify_semantic_tag_from_code_hash(type_code_hash_bytes.as_ref(), type_hash_type);

            // -- Occupied capacity -------------------------------------------

            let occupied_capacity = occupied_capacity_shannons_i64(
                lock_args_bytes.len(),
                type_args_bytes.as_ref().map(|a| a.len()),
                data_size,
            );

            // -- UDT amount (directly from raw data bytes) -------------------

            let udt_amount =
                parse_binary_udt_amount(semantic_tag, &data, &tx_hash, output_index_i16)?;

            // -- DAO state (inlined from raw data bytes) ---------------------

            let dao_state = if matches!(semantic_tag, CellSemanticTag::Dao) {
                let state = DaoParser::parse_dao_state(&data).ok_or_else(|| {
                    anyhow!(
                        "invalid DAO cell data in binary facts: tx=0x{}, output_index={}, data_len={}",
                        hex::encode(tx_hash),
                        output_index_i16,
                        data.len()
                    )
                })?;
                Some(match state {
                    DaoState::Deposit => DaoCellState::Deposit,
                    DaoState::WithdrawRequest => {
                        let deposit_block_number =
                            DaoParser::parse_deposit_block_number(&data).ok_or_else(|| {
                                anyhow!(
                                    "missing DAO deposit block number in withdraw request: tx=0x{}, output_index={}, data_len={}",
                                    hex::encode(tx_hash),
                                    output_index_i16,
                                    data.len()
                                )
                            })?;
                        DaoCellState::WithdrawRequest {
                            deposit_block_number: i64::try_from(deposit_block_number).map_err(|_| {
                                anyhow!(
                                    "DAO deposit block number exceeds i64 range in binary facts: tx=0x{}, output_index={}, deposit_block_number={}",
                                    hex::encode(tx_hash),
                                    output_index_i16,
                                    deposit_block_number
                                )
                            })?,
                        }
                    }
                })
            } else {
                None
            };

            // -- Protocol facts (ParsedCell only for protocol cells) ---------

            let protocol_facts = match semantic_tag {
                CellSemanticTag::Spore
                | CellSemanticTag::Cluster
                | CellSemanticTag::Mnft
                | CellSemanticTag::Dotbit => {
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
                    parse_protocol_facts(
                        &parsed_cell,
                        semantic_tag,
                        &witness_bundle,
                        &tx_hash,
                        output_index_i16,
                    )?
                }
                _ => None,
            };

            // -- Interned identities (after protocol_facts to avoid ownership conflict) --

            let lock_script_hash_id = interner.intern_bytes(lock_script_hash.to_vec());
            let lock_code_hash_id = interner.intern_bytes(lock_code_hash_bytes.to_vec());
            let lock_args_id = interner.intern_bytes(lock_args_bytes);
            let type_script_hash_id = type_script_hash.map(|v| interner.intern_bytes(v.to_vec()));
            let type_code_hash_id = type_code_hash_bytes.map(|v| interner.intern_bytes(v.to_vec()));
            let type_args_id = type_args_bytes.map(|v| interner.intern_bytes(v));

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
        parent_hash,
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
        Some(c) if *c > 0 => {
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
        // CKB node returns 0 for pre-hardfork blocks where cycles weren't tracked.
        // Treat as unknown (None) so lazy calculation can fill it in.
        Some(_) | None => Ok(None),
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
// UDT amount extraction (directly from raw data bytes)
// ---------------------------------------------------------------------------

/// Extract UDT amount directly from raw cell data bytes,
/// using the already-determined semantic tag to avoid rebuilding ParsedCell.
///
/// Note: The existing `parse_parsed_cell_udt_amount` in token_helpers.rs accepts a
/// `standard_hint: Option<&str>` parameter. In the binary_facts call path, this is
/// always `None`, so we intentionally omit it here.
fn parse_binary_udt_amount(
    semantic_tag: CellSemanticTag,
    data: &[u8],
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<u128>> {
    match semantic_tag {
        CellSemanticTag::Sudt => {
            let amount = UdtParser::parse_amount(data).ok_or_else(|| {
                anyhow!(
                    "failed to parse sUDT amount in binary facts: tx=0x{}, output_index={}, data_len={}",
                    hex::encode(tx_hash),
                    output_index,
                    data.len()
                )
            })?;
            Ok(Some(amount))
        }
        CellSemanticTag::Xudt => Ok(UdtParser::parse_amount(data)),
        _ => Ok(None),
    }
}

use super::facts::parse_protocol_facts;

// ---------------------------------------------------------------------------
// Arena builder
// ---------------------------------------------------------------------------

/// Compute cell-weighted chunk boundaries from pre-computed per-block cell counts.
pub(crate) fn compute_cell_weighted_chunks_from_counts(
    block_count: usize,
    cell_counts: &[usize],
    total_cells: usize,
) -> Vec<(usize, usize)> {
    debug_assert_eq!(cell_counts.len(), block_count);
    if block_count == 0 {
        return vec![];
    }

    let num_threads = rayon::current_num_threads();
    let target_chunks = (2 * num_threads).max(1);

    if block_count <= target_chunks {
        return (0..block_count).map(|i| (i, i + 1)).collect();
    }

    if total_cells == 0 {
        let chunk_size = block_count.div_ceil(target_chunks);
        return (0..block_count)
            .step_by(chunk_size)
            .map(|start| (start, block_count.min(start + chunk_size)))
            .collect();
    }

    let cells_per_chunk = total_cells.div_ceil(target_chunks);
    let mut chunks = Vec::with_capacity(target_chunks);
    let mut start = 0;
    let mut current_cells = 0;

    for (i, &count) in cell_counts.iter().enumerate() {
        current_cells += count;
        if current_cells >= cells_per_chunk && chunks.len() < target_chunks - 1 {
            chunks.push((start, i + 1));
            start = i + 1;
            current_cells = 0;
        }
    }
    if start < block_count {
        chunks.push((start, block_count));
    }
    chunks
}

/// Compute cell-weighted chunk boundaries so that each rayon work unit gets
/// roughly equal work.  Target: `2 * num_cpus` chunks for good load
/// balancing via work-stealing, with each chunk containing enough cells
/// (1000+) to amortise the ~10-20 µs rayon scheduling overhead.
///
/// Returns a vec of `(start_idx, end_idx)` pairs (exclusive end) into `blocks`.
#[cfg(test)]
fn compute_cell_weighted_chunks(blocks: &[RawCkbBlock]) -> Vec<(usize, usize)> {
    let cell_counts: Vec<usize> = blocks
        .iter()
        .map(|b| {
            b.block
                .transactions()
                .iter()
                .map(|tx| tx.outputs().len())
                .sum()
        })
        .collect();
    let total: usize = cell_counts.iter().sum();
    compute_cell_weighted_chunks_from_counts(blocks.len(), &cell_counts, total)
}

/// Process blocks serially into a FactsArena. Used when total cell count
/// is below the parallelism threshold where rayon overhead exceeds benefit.
fn build_facts_arena_serial(
    blocks: &[RawCkbBlock],
    interner: &IdentityInterner,
) -> Result<(super::facts::FactsArena, u64)> {
    let mut arena = super::facts::FactsArena::default();
    let mut total_cells: u64 = 0;

    for raw in blocks {
        let (block_facts, txs, cells) = parse_block_to_facts(raw, interner)?;
        let tx_start = arena.txs.len();
        let cell_start = arena.cells.len();
        total_cells += cells.len() as u64;

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

    Ok((arena, total_cells))
}

/// Build a FactsArena from raw CKB blocks using binary-native parsing.
///
/// Blocks are split into cell-weighted chunks targeting `2 * num_cpus`
/// parallel work units.  Each chunk gets roughly equal cell count,
/// ensuring good load balancing regardless of block density.
pub(crate) fn build_bulk_facts_arena_from_raw_blocks(
    blocks: &[RawCkbBlock],
    interner: &IdentityInterner,
) -> Result<(super::facts::FactsArena, FactsTimingBreakdown)> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    // Single O(n) pre-scan: count cells per block from molecule headers.
    let cell_counts: Vec<usize> = blocks
        .iter()
        .map(|b| {
            b.block
                .transactions()
                .iter()
                .map(|tx| tx.outputs().len())
                .sum()
        })
        .collect();
    let total_cells_estimate: usize = cell_counts.iter().sum();

    // Serial fast-path: when total cells are small, rayon overhead exceeds
    // the parallelism benefit. Threshold from perf data: batches under ~50K
    // cells averaged 0.30x speedup (3.3x slowdown) with par_iter.
    if total_cells_estimate < 50_000 {
        let start = Instant::now();
        let (arena, cell_count) = build_facts_arena_serial(blocks, interner)?;
        let elapsed = start.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        let (intern_total, intern_slow) = interner.drain_counters();
        return Ok((
            arena,
            FactsTimingBreakdown {
                par_iter_ms: elapsed_ms,
                merge_ms: 0.0,
                serial_equivalent_ms: elapsed_ms,
                intern_slow_path_count: intern_slow,
                intern_total_count: intern_total,
                cell_count,
            },
        ));
    }

    let serial_equivalent_us = AtomicU64::new(0);
    let total_cells = AtomicU64::new(0);

    let par_start = Instant::now();

    // Cell-weighted chunks: each rayon work unit processes roughly equal
    // cell count.  This replaces the old fixed par_chunks(500) which gave
    // poor speedup (1.45x on 24 cores) because early-phase blocks have
    // very few cells, producing chunks with microseconds of useful work.
    let chunk_ranges =
        compute_cell_weighted_chunks_from_counts(blocks.len(), &cell_counts, total_cells_estimate);
    let chunk_results: Vec<Result<super::facts::FactsArena>> = chunk_ranges
        .par_iter()
        .map(|&(start, end)| {
            let chunk = &blocks[start..end];
            let chunk_start = Instant::now();
            let mut sub_arena = super::facts::FactsArena::default();
            let mut chunk_cells: u64 = 0;

            for raw in chunk {
                let (block_facts, txs, cells) = parse_block_to_facts(raw, interner)?;
                let tx_start = sub_arena.txs.len();
                let cell_start = sub_arena.cells.len();
                chunk_cells += cells.len() as u64;

                for mut tx in txs {
                    tx.output_range =
                        (cell_start + tx.output_range.start)..(cell_start + tx.output_range.end);
                    sub_arena.txs.push(tx);
                }
                sub_arena.cells.extend(cells);

                let tx_end = sub_arena.txs.len();
                let mut block = block_facts;
                block.tx_range = tx_start..tx_end;
                sub_arena.blocks.push(block);
            }

            serial_equivalent_us
                .fetch_add(chunk_start.elapsed().as_micros() as u64, Ordering::Relaxed);
            total_cells.fetch_add(chunk_cells, Ordering::Relaxed);
            Ok(sub_arena)
        })
        .collect();
    let par_elapsed = par_start.elapsed();

    // Merge sub-arenas sequentially, remapping tx/cell offsets.
    let merge_start = Instant::now();
    let mut arena = super::facts::FactsArena::default();
    for result in chunk_results {
        let sub = result?;
        let tx_offset = arena.txs.len();
        let cell_offset = arena.cells.len();

        // Remap block → tx ranges.
        for mut block in sub.blocks {
            block.tx_range = (tx_offset + block.tx_range.start)..(tx_offset + block.tx_range.end);
            arena.blocks.push(block);
        }

        // Remap tx → cell ranges.
        for mut tx in sub.txs {
            tx.output_range =
                (cell_offset + tx.output_range.start)..(cell_offset + tx.output_range.end);
            arena.txs.push(tx);
        }

        arena.cells.extend(sub.cells);
    }
    let merge_elapsed = merge_start.elapsed();

    let (intern_total, intern_slow) = interner.drain_counters();

    let breakdown = FactsTimingBreakdown {
        par_iter_ms: par_elapsed.as_secs_f64() * 1000.0,
        merge_ms: merge_elapsed.as_secs_f64() * 1000.0,
        serial_equivalent_ms: serial_equivalent_us.load(Ordering::Relaxed) as f64 / 1000.0,
        intern_slow_path_count: intern_slow,
        intern_total_count: intern_total,
        cell_count: total_cells.load(Ordering::Relaxed),
    };

    Ok((arena, breakdown))
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
        assert_eq!(
            classify_semantic_tag_from_code_hash(None, None),
            CellSemanticTag::Plain
        );
    }

    #[test]
    fn classify_dao_cell() {
        let dao_code_hash =
            hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e")
                .unwrap();
        let hash: [u8; 32] = dao_code_hash.try_into().unwrap();
        assert_eq!(
            classify_semantic_tag_from_code_hash(Some(&hash), Some(1)),
            CellSemanticTag::Dao
        );
    }

    // -----------------------------------------------------------------------
    // parse_binary_udt_amount tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_binary_udt_amount_sudt_valid() {
        let data = 42u128.to_le_bytes().to_vec();
        let result = parse_binary_udt_amount(CellSemanticTag::Sudt, &data, &[0xAA; 32], 0).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn parse_binary_udt_amount_xudt_valid() {
        let data = 99u128.to_le_bytes().to_vec();
        let result = parse_binary_udt_amount(CellSemanticTag::Xudt, &data, &[0xAA; 32], 0).unwrap();
        assert_eq!(result, Some(99));
    }

    #[test]
    fn parse_binary_udt_amount_xudt_short_data_returns_none() {
        let data = vec![0u8; 8];
        let result = parse_binary_udt_amount(CellSemanticTag::Xudt, &data, &[0xAA; 32], 0).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_binary_udt_amount_sudt_short_data_errors() {
        let data = vec![0u8; 8];
        let result = parse_binary_udt_amount(CellSemanticTag::Sudt, &data, &[0xAA; 32], 0);
        assert!(result.is_err());
    }

    #[test]
    fn parse_binary_udt_amount_plain_returns_none() {
        let result = parse_binary_udt_amount(CellSemanticTag::Plain, &[], &[0xAA; 32], 0).unwrap();
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // DAO state mapping tests (DaoParser → DaoCellState)
    // -----------------------------------------------------------------------

    #[test]
    fn dao_deposit_data_maps_to_deposit_state() {
        let data = vec![0u8; 8];
        let state = DaoParser::parse_dao_state(&data).unwrap();
        assert_eq!(state, DaoState::Deposit);
    }

    #[test]
    fn dao_withdraw_request_data_maps_to_withdraw_state() {
        let data = 123u64.to_le_bytes().to_vec();
        let state = DaoParser::parse_dao_state(&data).unwrap();
        assert_eq!(state, DaoState::WithdrawRequest);
        let block_num = DaoParser::parse_deposit_block_number(&data).unwrap();
        assert_eq!(block_num, 123);
    }

    #[test]
    fn dao_invalid_data_length_returns_none() {
        let data = vec![0u8; 4];
        assert!(DaoParser::parse_dao_state(&data).is_none());
    }

    #[test]
    fn parse_fixed_protocol_id_correct_length() {
        use crate::sync::bulk_build::facts::parse_fixed_protocol_id;
        let bytes = [0xAB; 32];
        let result: [u8; 32] = parse_fixed_protocol_id(&bytes, "test_id", &[0; 32], 0).unwrap();
        assert_eq!(result, bytes);
    }

    #[test]
    fn parse_fixed_protocol_id_wrong_length_fails() {
        use crate::sync::bulk_build::facts::parse_fixed_protocol_id;
        let bytes = [0xAB; 16];
        let result = parse_fixed_protocol_id::<32>(&bytes, "test_id", &[0; 32], 0);
        assert!(result.is_err());
    }

    #[test]
    fn parse_optional_fixed_protocol_id_none() {
        use crate::sync::bulk_build::facts::parse_optional_fixed_protocol_id;
        let result = parse_optional_fixed_protocol_id::<32>(None, "test_id", &[0; 32], 0).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_optional_fixed_protocol_id_some_correct() {
        use crate::sync::bulk_build::facts::parse_optional_fixed_protocol_id;
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

    // -----------------------------------------------------------------------
    // compute_cell_weighted_chunks tests
    // -----------------------------------------------------------------------

    /// Helper to build a minimal `RawCkbBlock` with a given number of outputs.
    fn make_raw_block_with_outputs(num_outputs: usize) -> RawCkbBlock {
        use ckb_types::packed;
        use ckb_types::prelude::*;

        let outputs: Vec<packed::CellOutput> = (0..num_outputs)
            .map(|_| {
                packed::CellOutput::new_builder()
                    .capacity(packed::Uint64::from_slice(&100u64.to_le_bytes()).unwrap())
                    .lock(
                        packed::Script::new_builder()
                            .code_hash(packed::Byte32::default())
                            .hash_type(packed::Byte::new(1))
                            .build(),
                    )
                    .build()
            })
            .collect();
        let outputs_data: Vec<packed::Bytes> =
            (0..num_outputs).map(|_| packed::Bytes::default()).collect();

        let tx = packed::Transaction::new_builder()
            .raw(
                packed::RawTransaction::new_builder()
                    .outputs(packed::CellOutputVec::new_builder().set(outputs).build())
                    .outputs_data(packed::BytesVec::new_builder().set(outputs_data).build())
                    .build(),
            )
            .build();

        let header = packed::Header::new_builder()
            .raw(packed::RawHeader::new_builder().build())
            .build();

        let block = packed::Block::new_builder()
            .header(header)
            .transactions(packed::TransactionVec::new_builder().push(tx).build())
            .build();

        RawCkbBlock {
            block: block.into_view(),
            cycles: vec![],
        }
    }

    #[test]
    fn cell_weighted_chunks_empty() {
        let chunks = compute_cell_weighted_chunks(&[]);
        assert!(chunks.is_empty());
    }

    #[test]
    fn cell_weighted_chunks_single_block() {
        let blocks = vec![make_raw_block_with_outputs(10)];
        let chunks = compute_cell_weighted_chunks(&blocks);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], (0, 1));
    }

    #[test]
    fn cell_weighted_chunks_covers_all_blocks() {
        // Many blocks with varying cell counts
        let blocks: Vec<RawCkbBlock> = (0..100)
            .map(|i| make_raw_block_with_outputs(if i < 50 { 1 } else { 20 }))
            .collect();
        let chunks = compute_cell_weighted_chunks(&blocks);

        // Every block is in exactly one chunk
        let mut covered = vec![false; blocks.len()];
        for &(start, end) in &chunks {
            assert!(start < end, "chunk must be non-empty");
            for slot in &mut covered[start..end] {
                assert!(!*slot, "block in multiple chunks");
                *slot = true;
            }
        }
        assert!(covered.iter().all(|&c| c), "every block must be covered");
    }

    #[test]
    fn cell_weighted_chunks_balances_cell_count() {
        // 50 blocks with 1 cell + 50 blocks with 100 cells = 5050 cells total
        let blocks: Vec<RawCkbBlock> = (0..100)
            .map(|i| make_raw_block_with_outputs(if i < 50 { 1 } else { 100 }))
            .collect();
        let chunks = compute_cell_weighted_chunks(&blocks);

        // Should have created multiple chunks
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        // Heavy blocks (50-99 with 100 cells each) should be spread across chunks,
        // not all in one chunk.  Check that no single chunk has more than 60%
        // of total cells.
        let total_cells: usize = 50 + 50 * 100;
        for &(start, end) in &chunks {
            let chunk_cells: usize = (start..end).map(|i| if i < 50 { 1 } else { 100 }).sum();
            assert!(
                chunk_cells <= total_cells * 60 / 100,
                "chunk ({start}..{end}) has {chunk_cells} cells ({:.0}% of {total_cells}), too unbalanced",
                chunk_cells as f64 / total_cells as f64 * 100.0,
            );
        }
    }

    #[test]
    fn cell_weighted_chunks_from_counts_matches_wrapper() {
        let blocks: Vec<RawCkbBlock> = (0..100)
            .map(|i| make_raw_block_with_outputs(if i < 50 { 1 } else { 20 }))
            .collect();
        let wrapper_result = compute_cell_weighted_chunks(&blocks);

        let cell_counts: Vec<usize> = blocks
            .iter()
            .map(|b| {
                b.block
                    .transactions()
                    .iter()
                    .map(|tx| tx.outputs().len())
                    .sum()
            })
            .collect();
        let total: usize = cell_counts.iter().sum();
        let direct_result =
            compute_cell_weighted_chunks_from_counts(blocks.len(), &cell_counts, total);

        assert_eq!(wrapper_result, direct_result);
    }

    #[test]
    fn cell_weighted_chunks_zero_cells() {
        // Blocks with no transactions (no cells)
        let blocks: Vec<RawCkbBlock> = (0..10)
            .map(|_| {
                use ckb_types::packed;
                use ckb_types::prelude::*;
                let header = packed::Header::new_builder()
                    .raw(packed::RawHeader::new_builder().build())
                    .build();
                let block = packed::Block::new_builder().header(header).build();
                RawCkbBlock {
                    block: block.into_view(),
                    cycles: vec![],
                }
            })
            .collect();
        let chunks = compute_cell_weighted_chunks(&blocks);

        // Should still partition all blocks
        let total_blocks: usize = chunks.iter().map(|&(s, e)| e - s).sum();
        assert_eq!(total_blocks, 10);
    }
}
