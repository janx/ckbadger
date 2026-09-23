//! One resolver from an uncommitted transaction to the cells it spends and
//! creates.
//!
//! Shared by the tx-pool mirror (which then runs the indexer's activity
//! interpreter over the result) and by the `/tx/{hash}` pending branch, so a
//! transaction's inputs are resolved exactly one way no matter which endpoint
//! asked.
//!
//! Resolution order is a DEFINITION, not a retry ladder:
//!   1. an output of another transaction that is itself in the pool (a chained
//!      unconfirmed spend — the node has no live cell for it yet);
//!   2. the node's `get_live_cell(out_point, with_data = true)`.
//!
//! A cell the node does not report as live means the transaction conflicts with
//! the chain, or its parent was committed a moment ago. The input then stays
//! unresolved and the caller retries on the next poll — it is never filled in
//! from somewhere else, and never defaulted to zero.
//!
//! Resolving through the node rather than the local store is deliberate: live
//! cells are primitive truth in the node's own database, the store's
//! `LiveCellInfo` carries no `data` bytes (which DAO / `.bit` / UDT
//! interpretation needs), and this path keeps working while the indexer is
//! still in bulk sync.

use std::collections::HashMap;

use ckb_store_reader::RpcTransactionView;
use ckbadger_indexer::db::{InputCellView, OutputCellView, TxView};
use ckbadger_indexer::parser::dao::{DaoParser, DaoState};
use ckbadger_indexer::parser::udt::UdtParser;

use super::source::{
    parse_hash_type, parse_hex_bytes, parse_hex_hash32, parse_hex_u64, NodeLiveCell, PoolSource,
};
use crate::utils::address::compute_script_hash;

/// A previous output, identified the way CKB identifies one.
pub type OutPointKey = ([u8; 32], u32);

/// A cell resolved into everything the activity interpreter and the API need,
/// owned so the borrowed `TxView` can be built from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCell {
    pub capacity: i64,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub lock_script_hash: Vec<u8>,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub data: Vec<u8>,
    /// Exact occupied capacity in shannons, through the one shared formula the
    /// indexer writes into `LiveCellInfo.occupied_capacity`.
    pub occupied_capacity: i64,
    /// UDT amount when this cell's type script is a UDT and its data carries a
    /// u128 amount. Mirrors the indexer's own rule: classify by
    /// (code_hash, hash_type), then parse; a short payload yields no amount
    /// rather than a zero.
    pub udt_amount: Option<u128>,
}

impl ResolvedCell {
    pub fn new(
        capacity: i64,
        lock_code_hash: Vec<u8>,
        lock_hash_type: i16,
        lock_args: Vec<u8>,
        type_script: Option<(Vec<u8>, i16, Vec<u8>)>,
        data: Vec<u8>,
    ) -> Result<Self, String> {
        let lock_script_hash =
            compute_script_hash(&lock_code_hash, lock_hash_type as u8, &lock_args);
        let occupied_capacity = ckbadger_common::dao::occupied_capacity_shannons(
            data.len(),
            lock_args.len(),
            type_script.as_ref().map(|(_, _, args)| args.len()),
        )
        .map_err(|e| format!("occupied capacity for pool cell: {e}"))?;

        let (type_code_hash, type_hash_type, type_args, type_script_hash, udt_amount) =
            match type_script {
                Some((code_hash, hash_type, args)) => {
                    let script_hash = compute_script_hash(&code_hash, hash_type as u8, &args);
                    let udt_amount = UdtParser::is_udt_code_hash_bytes(&code_hash, hash_type)
                        .and_then(|_| UdtParser::parse_amount(&data));
                    (
                        Some(code_hash),
                        Some(hash_type),
                        Some(args),
                        Some(script_hash),
                        udt_amount,
                    )
                }
                None => (None, None, None, None, None),
            };

        Ok(Self {
            capacity,
            lock_code_hash,
            lock_hash_type,
            lock_args,
            lock_script_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            type_script_hash,
            data,
            occupied_capacity,
            udt_amount,
        })
    }

    fn from_node_live_cell(cell: NodeLiveCell) -> Result<Self, String> {
        let capacity = i64::try_from(cell.capacity)
            .map_err(|_| format!("live cell capacity {} exceeds i64", cell.capacity))?;
        Self::new(
            capacity,
            cell.lock.code_hash.to_vec(),
            cell.lock.hash_type,
            cell.lock.args,
            cell.type_script
                .map(|script| (script.code_hash.to_vec(), script.hash_type, script.args)),
            cell.data,
        )
    }

    /// Whether this cell is a Nervos DAO withdraw *request* (phase 1 output).
    ///
    /// DAO cell data is the 8-byte deposit block number: all zero for a
    /// deposit, non-zero once a withdrawal has been requested.
    pub fn is_dao_withdraw_request(&self) -> bool {
        self.type_code_hash
            .as_deref()
            .is_some_and(DaoParser::is_dao_code_hash)
            && DaoParser::parse_dao_state(&self.data) == Some(DaoState::WithdrawRequest)
    }
}

/// Where step 1 of the resolution order looks: outputs of transactions that are
/// themselves in the pool.
pub trait PoolParentCells: Send + Sync {
    fn output_cell(&self, tx_hash: &[u8; 32], index: u32) -> Option<ResolvedCell>;
}

/// No pool parents — the `/tx/{hash}` branch when the mirror is off.
pub struct NoPoolParents;

impl PoolParentCells for NoPoolParents {
    fn output_cell(&self, _tx_hash: &[u8; 32], _index: u32) -> Option<ResolvedCell> {
        None
    }
}

impl PoolParentCells for HashMap<OutPointKey, ResolvedCell> {
    fn output_cell(&self, tx_hash: &[u8; 32], index: u32) -> Option<ResolvedCell> {
        self.get(&(*tx_hash, index)).cloned()
    }
}

/// One input of a pool transaction, with the cell it spends when that cell
/// could be resolved.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
    pub previous_tx_hash: [u8; 32],
    pub previous_output_index: u32,
    pub since: String,
    pub cell: Option<ResolvedCell>,
}

/// An uncommitted transaction with its inputs resolved as far as the node
/// allows.
#[derive(Debug, Clone)]
pub struct ResolvedPoolTx {
    pub tx_hash: [u8; 32],
    pub inputs: Vec<ResolvedInput>,
    pub outputs: Vec<ResolvedCell>,
    pub witnesses: Vec<String>,
    pub is_cellbase: bool,
}

impl ResolvedPoolTx {
    /// Outpoints whose cell the node could not resolve.
    pub fn unresolved_inputs(&self) -> Vec<OutPointKey> {
        self.inputs
            .iter()
            .filter(|input| input.cell.is_none())
            .map(|input| (input.previous_tx_hash, input.previous_output_index))
            .collect()
    }

    /// Whether any input spends a DAO withdraw-request cell, i.e. this is a
    /// withdrawal completion. Its compensation needs header AR arithmetic the
    /// mirror does not do yet, so such a record is reported as partially
    /// interpreted rather than given a made-up compensation.
    pub fn completes_dao_withdrawal(&self) -> bool {
        self.inputs.iter().any(|input| {
            input
                .cell
                .as_ref()
                .is_some_and(ResolvedCell::is_dao_withdraw_request)
        })
    }

    /// A `TxView` over this transaction for the indexer's activity interpreter.
    ///
    /// `None` when any input is unresolved: with a missing input the spender's
    /// CKB position cannot be computed, and a position that is merely
    /// *probably* right is worse than none.
    ///
    /// The block fields are provisional (`block_number = 0`, zero block hash,
    /// `tx_index = 0`); callers render from the record's pool status, never
    /// from these, and API rows built from pool records carry null chain
    /// positions.
    pub fn tx_view<'a>(
        &'a self,
        zero_block_hash: &'a [u8; 32],
        timestamp_ms: i64,
    ) -> Option<TxView<'a>> {
        let mut inputs = Vec::with_capacity(self.inputs.len());
        for input in &self.inputs {
            let cell = input.cell.as_ref()?;
            inputs.push(InputCellView {
                previous_tx_hash: &input.previous_tx_hash,
                previous_output_index: input.previous_output_index,
                lock_script_hash: &cell.lock_script_hash,
                lock_code_hash: &cell.lock_code_hash,
                lock_hash_type: cell.lock_hash_type,
                lock_args: &cell.lock_args,
                capacity: cell.capacity,
                occupied_capacity: cell.occupied_capacity,
                type_code_hash: cell.type_code_hash.as_deref(),
                type_hash_type: cell.type_hash_type,
                type_script_hash: cell.type_script_hash.as_deref(),
                type_args: cell.type_args.as_deref(),
                udt_amount: cell.udt_amount,
                // The mirror holds the real input data (the node returns it with
                // the live cell), so `.bit Cell` identity IDs are parsed from it
                // exactly as they are for outputs. No pre-parsed override.
                bit_cell_identity_id: None,
                dotcell: None,
                data: &cell.data,
                // Phase-2 DAO compensation is not derivable without header AR
                // arithmetic, and `classify_input` refuses a withdraw-request
                // input without one. Flagging it false omits the
                // `dao:withdraw_complete` action; the record declares that
                // omission as `DaoCompensationUnavailable` rather than
                // inventing a compensation figure.
                is_dao_withdraw_request: false,
                dao_compensation: None,
            });
        }

        let outputs = self
            .outputs
            .iter()
            .map(|cell| OutputCellView {
                capacity: cell.capacity,
                lock_code_hash: &cell.lock_code_hash,
                lock_hash_type: cell.lock_hash_type,
                lock_args: &cell.lock_args,
                lock_script_hash: &cell.lock_script_hash,
                type_code_hash: cell.type_code_hash.as_deref(),
                type_hash_type: cell.type_hash_type,
                type_args: cell.type_args.as_deref(),
                type_script_hash: cell.type_script_hash.as_deref(),
                data_hash: &[],
                data_size: cell.data.len() as i32,
                data: &cell.data,
            })
            .collect();

        Some(TxView {
            tx_hash: &self.tx_hash,
            block_hash: zero_block_hash,
            tx_index: 0,
            block_number: 0,
            timestamp: timestamp_ms,
            is_cellbase: self.is_cellbase,
            inputs,
            outputs,
        })
    }

    /// `semantic_tags` for this transaction, derived through the indexer's one
    /// shared classifier over every input and output type script — the same
    /// rule (outputs OR inputs) the live-sync tx_index writer applies.
    pub fn semantic_tags(&self) -> u16 {
        if self.is_cellbase {
            return 0;
        }
        let mut tags = 0u16;
        for cell in self
            .outputs
            .iter()
            .chain(self.inputs.iter().filter_map(|input| input.cell.as_ref()))
        {
            tags |= ckbadger_indexer::sync::classify_type_script_semantic_tag(
                cell.type_code_hash.as_deref(),
                cell.type_hash_type,
            )
            .to_bit();
        }
        tags
    }
}

/// The outpoints a transaction spends, in input order, skipping the cellbase
/// pseudo-input.
fn input_outpoints(tx: &RpcTransactionView) -> Result<Vec<(OutPointKey, String)>, String> {
    tx.inputs
        .iter()
        .map(|input| {
            let tx_hash = parse_hex_hash32(
                &input.previous_output.tx_hash,
                "input.previous_output.tx_hash",
            )?;
            let index = u32::try_from(parse_hex_u64(
                &input.previous_output.index,
                "input.previous_output.index",
            )?)
            .map_err(|_| {
                format!(
                    "input.previous_output.index '{}' exceeds u32",
                    input.previous_output.index
                )
            })?;
            Ok(((tx_hash, index), input.since.clone()))
        })
        .collect()
}

fn is_cellbase_outpoint(tx_hash: &[u8; 32]) -> bool {
    tx_hash.iter().all(|byte| *byte == 0)
}

/// Resolve every previous output of `tx`, in the fixed order defined above.
///
/// Returns the map of what could be resolved. An outpoint the node does not
/// report as live is simply absent; an RPC failure is an error, because
/// "the node did not answer" and "the cell is not live" are different facts
/// and must not collapse into one.
pub async fn resolve_previous_outputs(
    source: &dyn PoolSource,
    tx: &RpcTransactionView,
    pool_parents: &dyn PoolParentCells,
) -> Result<HashMap<OutPointKey, ResolvedCell>, String> {
    let outpoints = input_outpoints(tx)?;
    let mut resolved = HashMap::with_capacity(outpoints.len());

    for ((tx_hash, index), _since) in outpoints {
        if is_cellbase_outpoint(&tx_hash) || resolved.contains_key(&(tx_hash, index)) {
            continue;
        }

        // 1) a parent that is itself in the pool
        if let Some(cell) = pool_parents.output_cell(&tx_hash, index) {
            resolved.insert((tx_hash, index), cell);
            continue;
        }

        // 2) the node's live-cell set
        if let Some(cell) = source.get_live_cell(&tx_hash, index).await? {
            resolved.insert((tx_hash, index), ResolvedCell::from_node_live_cell(cell)?);
        }
    }

    Ok(resolved)
}

/// Build the owned cell view of a pool transaction from its RPC form plus the
/// previous outputs already resolved for it.
pub fn resolve_pool_tx(
    tx: &RpcTransactionView,
    previous_outputs: &HashMap<OutPointKey, ResolvedCell>,
) -> Result<ResolvedPoolTx, String> {
    if tx.outputs.len() != tx.outputs_data.len() {
        return Err(format!(
            "transaction {} has {} outputs but {} outputs_data entries",
            tx.hash,
            tx.outputs.len(),
            tx.outputs_data.len()
        ));
    }

    let tx_hash = parse_hex_hash32(&tx.hash, "transaction.hash")?;
    let outpoints = input_outpoints(tx)?;
    let is_cellbase = outpoints
        .first()
        .is_some_and(|((hash, _), _)| is_cellbase_outpoint(hash));

    let inputs = outpoints
        .into_iter()
        .map(
            |((previous_tx_hash, previous_output_index), since)| ResolvedInput {
                cell: if is_cellbase_outpoint(&previous_tx_hash) {
                    None
                } else {
                    previous_outputs
                        .get(&(previous_tx_hash, previous_output_index))
                        .cloned()
                },
                previous_tx_hash,
                previous_output_index,
                since,
            },
        )
        .collect::<Vec<_>>();

    let outputs = tx
        .outputs
        .iter()
        .zip(tx.outputs_data.iter())
        .enumerate()
        .map(|(index, (output, data_hex))| {
            let context = |field: &str| format!("output[{index}].{field}");
            let capacity = i64::try_from(parse_hex_u64(&output.capacity, &context("capacity"))?)
                .map_err(|_| format!("output[{index}].capacity exceeds i64"))?;
            let type_script = output
                .type_
                .as_ref()
                .map(|script| -> Result<_, String> {
                    Ok((
                        parse_hex_hash32(&script.code_hash, &context("type.code_hash"))?.to_vec(),
                        parse_hash_type(&script.hash_type)?,
                        parse_hex_bytes(&script.args, &context("type.args"))?,
                    ))
                })
                .transpose()?;
            ResolvedCell::new(
                capacity,
                parse_hex_hash32(&output.lock.code_hash, &context("lock.code_hash"))?.to_vec(),
                parse_hash_type(&output.lock.hash_type)?,
                parse_hex_bytes(&output.lock.args, &context("lock.args"))?,
                type_script,
                parse_hex_bytes(data_hex, &context("data"))?,
            )
        })
        .collect::<Result<Vec<_>, String>>()?;

    // A cellbase transaction can never be in the tx pool, but the same resolver
    // serves `/tx/{hash}`, which can be asked about one.
    Ok(ResolvedPoolTx {
        tx_hash,
        inputs,
        outputs,
        witnesses: tx.witnesses.clone(),
        is_cellbase,
    })
}
