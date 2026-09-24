//! One resolver from an uncommitted transaction to the cells it spends and
//! creates.
//!
//! Shared by the tx-pool mirror (which then runs the indexer's activity
//! interpreter over the result) and by the `/tx/{hash}` pending branch, so a
//! transaction's inputs are resolved exactly one way no matter which endpoint
//! asked.
//!
//! Resolution order is a DEFINITION, not a retry ladder:
//!   1. an output of a transaction the mirror's snapshot holds (a chained
//!      unconfirmed spend whose parent the mirror has already resolved);
//!   2. otherwise `outputs[index]` / `outputs_data[index]` of the parent
//!      transaction as the node returns it from `get_transaction(prev_tx_hash)`,
//!      each parent fetched once per call.
//!
//! Step 2 holds for every input of every uncommitted transaction: the node
//! answers `get_transaction` for committed AND pool transactions, and a
//! previous output is what its creating transaction says it is whether or not
//! it has been spent since. That is what lets `/tx/{hash}` serve a transaction
//! the node committed a moment ago (its inputs are spent by then), and lets a
//! chained pool spend resolve with the mirror switched off.
//!
//! A parent the node does not know leaves the input unresolved; the caller
//! retries (the mirror on its next poll, `/tx` with a 503). It is never filled
//! in from somewhere else, and never defaulted to zero. A node that fails to
//! answer is an error: "the node did not answer" and "the node does not know
//! this transaction" are different facts.
//!
//! A spent Nervos DAO withdraw-request cell additionally carries the exact
//! compensation completing its withdrawal pays (RFC-0023), priced from the
//! deposit block's and the request block's accumulated rates — two more
//! header reads per distinct block.
//!
//! Resolving through the node rather than the local store is deliberate: the
//! node holds the primitive truth, the store's `LiveCellInfo` carries no `data`
//! bytes (which DAO / `.bit` / UDT interpretation needs), and this path keeps
//! working while the indexer is still in bulk sync.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;

use ckb_store_reader::RpcTransactionView;
use ckbadger_indexer::db::{InputCellView, OutputCellView, TxView};
use ckbadger_indexer::parser::dao::{DaoParser, DaoState};
use ckbadger_indexer::parser::udt::UdtParser;
use ckbadger_indexer::parser::DotCellParser;
use ckbadger_store::types::DotCellNameData;
use futures::StreamExt;

use super::source::{
    parse_hash_type, parse_hex_bytes, parse_hex_hash32, parse_hex_u64, NodeHeader, NodeTxStatus,
    PoolSource,
};
use crate::utils::address::compute_script_hash;

/// A previous output, identified the way CKB identifies one.
pub type OutPointKey = ([u8; 32], u32);

/// How many node calls one resolution (or one mirror step) keeps in flight.
pub const TX_FETCH_CONCURRENCY: usize = 16;

/// Run `fetch` for every key with at most [`TX_FETCH_CONCURRENCY`] in flight,
/// returning the results in key order.
pub(crate) async fn fetch_bounded<K, T, F, Fut>(keys: Vec<K>, fetch: F) -> Vec<(K, T)>
where
    K: Copy,
    F: Fn(K) -> Fut,
    Fut: Future<Output = T>,
{
    futures::stream::iter(keys)
        .map(|key| {
            let pending = fetch(key);
            async move { (key, pending.await) }
        })
        .buffered(TX_FETCH_CONCURRENCY)
        .collect()
        .await
}

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
    /// The `.cell` name this cell carries, when its type script is the Cells
    /// Account script.
    ///
    /// A `.cell` name's ownership lives in the cell's DATA, and the classifier
    /// learns a consumed name's previous state only from
    /// `InputCellView.dotcell`. The mirror has the node's resolved previous
    /// output, data included, so it parses it here with the SAME parser the
    /// indexer uses — leaving it empty would make every touch of an existing
    /// name read as a brand-new registration.
    pub dotcell: Option<DotCellNameData>,
    /// For a Nervos DAO withdraw-request cell spent as an input: the exact
    /// compensation completing its withdrawal pays, from the deposit and
    /// request headers' accumulated rates and THIS cell's occupied capacity
    /// (RFC-0023 counts the withdrawing cell's). Set only by
    /// [`resolve_previous_outputs`]; `None` for every other cell.
    pub dao_compensation: Option<i64>,
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

        let (type_code_hash, type_hash_type, type_args, type_script_hash, udt_amount, dotcell) =
            match type_script {
                Some((code_hash, hash_type, args)) => {
                    let script_hash = compute_script_hash(&code_hash, hash_type as u8, &args);
                    let udt_amount = UdtParser::is_udt_code_hash_bytes(&code_hash, hash_type)
                        .and_then(|_| UdtParser::parse_amount(&data));
                    // A Cells Account cell whose data does not decode is not a
                    // cell to interpret loosely: say so rather than report the
                    // transaction as if the name were absent.
                    let dotcell = if DotCellParser::is_account_type_script(&code_hash) {
                        Some(
                            DotCellParser::parse_name_data(&data)
                                .map_err(|e| format!("pool .cell name cell: {e}"))?,
                        )
                    } else {
                        None
                    };
                    (
                        Some(code_hash),
                        Some(hash_type),
                        Some(args),
                        Some(script_hash),
                        udt_amount,
                        dotcell,
                    )
                }
                None => (None, None, None, None, None, None),
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
            dotcell,
            dao_compensation: None,
        })
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
    /// Outpoints whose cell could not be resolved. A cellbase's pseudo-input
    /// spends no cell, so it is never one of them.
    pub fn unresolved_inputs(&self) -> Vec<OutPointKey> {
        self.inputs
            .iter()
            .filter(|input| input.cell.is_none() && !is_cellbase_outpoint(&input.previous_tx_hash))
            .map(|input| (input.previous_tx_hash, input.previous_output_index))
            .collect()
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
                // The mirror holds the real input data (the parent transaction
                // carries it in `outputs_data`), so `.bit Cell` identity IDs are parsed from it
                // exactly as they are for outputs. No pre-parsed override.
                bit_cell_identity_id: None,
                // `.cell` is the exception: the classifier reads a consumed
                // name's previous state from this field, never from the data.
                dotcell: cell.dotcell.as_ref(),
                data: &cell.data,
                // Same two fields live sync fills from its DAO map: the
                // resolver priced every spent withdraw-request cell, so the
                // builder emits `dao:withdraw_complete` with the exact
                // compensation (and refuses a request cell without one).
                is_dao_withdraw_request: cell.is_dao_withdraw_request(),
                dao_compensation: cell.dao_compensation,
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
/// Returns the map of what could be resolved. An outpoint whose parent the node
/// does not know is simply absent; an RPC failure is an error.
pub async fn resolve_previous_outputs(
    source: &dyn PoolSource,
    tx: &RpcTransactionView,
    pool_parents: &dyn PoolParentCells,
) -> Result<HashMap<OutPointKey, ResolvedCell>, String> {
    let outpoints = input_outpoints(tx)?;
    let mut resolved: HashMap<OutPointKey, ResolvedCell> = HashMap::with_capacity(outpoints.len());
    // Step 2's work list: each parent once, with every index spent from it.
    let mut from_node: BTreeMap<[u8; 32], BTreeSet<u32>> = BTreeMap::new();

    for ((tx_hash, index), _since) in outpoints {
        if is_cellbase_outpoint(&tx_hash) || resolved.contains_key(&(tx_hash, index)) {
            continue;
        }
        // 1) a parent the mirror's snapshot holds
        if let Some(cell) = pool_parents.output_cell(&tx_hash, index) {
            resolved.insert((tx_hash, index), cell);
            continue;
        }
        from_node.entry(tx_hash).or_default().insert(index);
    }

    // 2) the parent transaction, as the node returns it
    let parents: Vec<[u8; 32]> = from_node.keys().copied().collect();
    let lookups = fetch_bounded(parents, |hash| async move {
        source.get_transaction(&hash).await
    })
    .await;
    // Withdraw-request inputs, keyed to the block that committed their request.
    let mut request_blocks: HashMap<OutPointKey, [u8; 32]> = HashMap::new();
    for (parent_hash, lookup) in lookups {
        let context = || format!("previous transaction 0x{}", hex::encode(parent_hash));
        let Some(lookup) = lookup.map_err(|e| format!("{}: {e}", context()))? else {
            continue;
        };
        let parent_tx = match (&lookup.transaction, lookup.status) {
            (Some(parent_tx), _) => parent_tx,
            // The node does not know it (or has dropped it): the input stays
            // unresolved.
            (None, NodeTxStatus::Unknown | NodeTxStatus::Rejected) => continue,
            (None, status) => {
                return Err(format!(
                    "{}: node reported {status:?} without a transaction body",
                    context()
                ))
            }
        };
        for index in &from_node[&parent_hash] {
            let cell = output_cell(parent_tx, *index)
                .map_err(|e| format!("{}, output {index}: {e}", context()))?;
            if cell.is_dao_withdraw_request() {
                match (lookup.status, lookup.block_hash) {
                    (NodeTxStatus::Committed, Some(block_hash)) => {
                        request_blocks.insert((parent_hash, *index), block_hash);
                    }
                    (status, _) => {
                        return Err(format!(
                            "{}, output {index}: a DAO withdraw-request cell whose request is \
                             {status:?}, not committed; its withdrawal cannot complete yet",
                            context()
                        ))
                    }
                }
            }
            resolved.insert((parent_hash, *index), cell);
        }
    }

    price_dao_withdrawals(source, &mut resolved, &request_blocks).await?;
    Ok(resolved)
}

/// Attach the exact compensation to every spent DAO withdraw-request cell.
///
/// The same arithmetic live sync runs (`calculate_dao_compensation_from_ar`),
/// fed the same inputs: the request cell's capacity and exact occupied
/// capacity, `AR_deposit` from the block its data names, `AR_withdraw` from the
/// block that committed the request.
async fn price_dao_withdrawals(
    source: &dyn PoolSource,
    resolved: &mut HashMap<OutPointKey, ResolvedCell>,
    request_blocks: &HashMap<OutPointKey, [u8; 32]>,
) -> Result<(), String> {
    let mut withdrawals: Vec<(OutPointKey, [u8; 32], u64)> = Vec::new();
    for (outpoint, cell) in resolved.iter() {
        if !cell.is_dao_withdraw_request() {
            continue;
        }
        let describe = || {
            format!(
                "DAO withdraw-request input 0x{}:{}",
                hex::encode(outpoint.0),
                outpoint.1
            )
        };
        let Some(request_block) = request_blocks.get(outpoint) else {
            return Err(format!(
                "{} is an output of a transaction still in the pool; its withdrawal cannot \
                 complete before the request is committed",
                describe()
            ));
        };
        let deposit_block = DaoParser::parse_deposit_block_number(&cell.data)
            .ok_or_else(|| format!("{} carries no deposit block number", describe()))?;
        withdrawals.push((*outpoint, *request_block, deposit_block));
    }
    if withdrawals.is_empty() {
        return Ok(());
    }

    let mut request_hashes: Vec<[u8; 32]> = withdrawals.iter().map(|(_, hash, _)| *hash).collect();
    request_hashes.sort_unstable();
    request_hashes.dedup();
    let mut deposit_numbers: Vec<u64> = withdrawals.iter().map(|(_, _, number)| *number).collect();
    deposit_numbers.sort_unstable();
    deposit_numbers.dedup();

    let mut ar_by_hash: HashMap<[u8; 32], u64> = HashMap::new();
    for (hash, header) in fetch_bounded(request_hashes, |hash| async move {
        source.get_header(&hash).await
    })
    .await
    {
        let label = format!("DAO request block 0x{}", hex::encode(hash));
        ar_by_hash.insert(hash, accumulated_rate(header, &label)?);
    }
    let mut ar_by_number: HashMap<u64, u64> = HashMap::new();
    for (number, header) in fetch_bounded(deposit_numbers, |number| async move {
        source.get_header_by_number(number).await
    })
    .await
    {
        let label = format!("DAO deposit block #{number}");
        ar_by_number.insert(number, accumulated_rate(header, &label)?);
    }

    for (outpoint, request_block, deposit_block) in withdrawals {
        let cell = resolved
            .get_mut(&outpoint)
            .expect("withdrawals are collected from the resolved map");
        let compensation = ckbadger_common::dao::calculate_dao_compensation_from_ar(
            cell.capacity,
            cell.occupied_capacity,
            ar_by_number[&deposit_block],
            ar_by_hash[&request_block],
        )
        .map_err(|e| {
            format!(
                "DAO compensation for input 0x{}:{} (deposit #{deposit_block}, request 0x{}): {e}",
                hex::encode(outpoint.0),
                outpoint.1,
                hex::encode(request_block)
            )
        })?;
        cell.dao_compensation = Some(compensation);
    }
    Ok(())
}

fn accumulated_rate(
    header: Result<Option<NodeHeader>, String>,
    label: &str,
) -> Result<u64, String> {
    let header = header
        .map_err(|e| format!("{label}: {e}"))?
        .ok_or_else(|| format!("{label}: the node does not know this header"))?;
    DaoParser::extract_ar_from_dao_field(&header.dao)
        .ok_or_else(|| format!("{label}: DAO field carries no accumulated rate"))
}

/// `outputs[index]` of `tx`, with its data, as a resolved cell. The ONE parse of
/// an RPC output: [`resolve_pool_tx`] builds a transaction's own outputs through
/// it, and step 2 builds a parent's.
fn output_cell(tx: &RpcTransactionView, index: u32) -> Result<ResolvedCell, String> {
    let position = index as usize;
    let (Some(output), Some(data_hex)) = (tx.outputs.get(position), tx.outputs_data.get(position))
    else {
        return Err(format!(
            "output {index} out of range ({} outputs, {} outputs_data)",
            tx.outputs.len(),
            tx.outputs_data.len()
        ));
    };
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

    let outputs = (0..tx.outputs.len())
        .map(|position| {
            let index = u32::try_from(position)
                .map_err(|_| format!("transaction {} has more than u32::MAX outputs", tx.hash))?;
            output_cell(tx, index)
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
