use anyhow::{anyhow, bail, Context as _, Result};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{DaoDepositCacheEntry, PositionedCellInfo};
use ckbadger_store::{dao_frozen_request_compensation, CkbadgerStore};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) use ckbadger_common::dao::calculate_dao_compensation_from_ar;

use crate::parser::{DaoParser, ParsedDaoDeposit};
use crate::sync::dao_helpers::{DaoConsumedMap, DaoConsumedRow};
use crate::sync::types::TxData;

use super::BatchWriter;

fn build_dao_cache_entry(
    deposit: &ParsedDaoDeposit,
    block_number: i64,
    deposit_ar: i64,
    deposit_timestamp: i64,
) -> DaoDepositCacheEntry {
    DaoDepositCacheEntry {
        capacity: deposit.capacity,
        occupied_capacity: deposit.occupied_capacity,
        deposit_block_number: block_number,
        deposit_timestamp,
        lock_script_hash: deposit.lock_script_hash.clone(),
        deposit_ar,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_request_occupied_capacity: None,
        withdraw_to_output_index: None,
        compensation: None,
    }
}

fn dao_cache_entry_to_row(
    tx_hash: Vec<u8>,
    output_index: i16,
    entry: DaoDepositCacheEntry,
) -> DaoConsumedRow {
    DaoConsumedRow {
        tx_hash,
        output_index,
        capacity_str: entry.capacity.to_string(),
        deposit_block: entry.deposit_block_number,
        status: entry.status,
        lock_script_hash: entry.lock_script_hash,
    }
}

/// A DAO cell created by the consuming transaction with a deposit block
/// number in its data: the phase-1 withdraw request of the deposit spent at
/// the same index (RFC-0023). Its lock is free and plays no part in pairing.
#[derive(Debug, Clone)]
pub struct NewDaoOutput {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub capacity: i64,
    /// Exact occupied capacity of this withdraw-request cell.
    ///
    /// RFC-0023 derives `counted_capacity` from the WITHDRAWING cell, so this
    /// — not the original deposit cell's occupied capacity — is what the
    /// deposit's compensation must be computed from once phase-1 happens.
    pub occupied_capacity: i64,
    pub deposit_block: u64,
}

/// Every NervosDAO lifecycle transition one live batch makes, resolved once,
/// in chain order.
///
/// A live batch spans up to 5,000 blocks, so a deposit, its withdraw request
/// and its withdrawal completion can all fall inside ONE batch. (They can never
/// share a block — a completion header-deps its request block — but nothing
/// stops them sharing a batch.) Each DAO input is therefore resolved against
/// the committed store shadowed by every transition this batch has already
/// applied (read-your-writes), the way the bulk reducer resolves against its
/// in-memory state. A DAO-typed input that resolves to no lifecycle entry is an
/// invariant violation and fails the batch with its block/tx/outpoint.
///
/// This is the live path's single owner of DAO lifecycle facts: the phase-2
/// fee correction, the activity input views, the daily DAO deltas, the
/// pre-commit snapshot overlays and the staged `dao_deposits` /
/// `dao_by_withdraw_tx` rows all read what it resolved. Before it existed each
/// of them re-resolved inputs against committed state only, so a completion
/// whose request was written earlier in the same batch was silently dropped by
/// all of them at once (POSTMORTEM DAO-032).
#[derive(Debug, Default)]
pub(crate) struct LiveDaoBatch {
    /// Every DAO cell the batch consumes, keyed by the consumed outpoint, with
    /// its lifecycle status when it was spent (0 = deposit, 1 = withdraw
    /// request) and the original deposit it belongs to.
    consumed: DaoConsumedMap,
    /// Claimed compensation of every withdraw request the batch consumes,
    /// keyed by the consumed request outpoint.
    compensations: HashMap<(Vec<u8>, i16), i64>,
    /// Post-batch entry of every deposit the batch touched, by deposit outpoint.
    entries: BTreeMap<[u8; 34], DaoDepositCacheEntry>,
    /// Deposits this batch created or moved to phase 1. Their post-batch entry
    /// is the pre-commit snapshot overlay; every other touched deposit is a
    /// completion of a request committed by an earlier batch.
    lifecycle_in_batch: HashSet<[u8; 34]>,
    /// `dao_by_withdraw_tx` rows to stage: request outpoint → deposit outpoint.
    request_rows: Vec<(Vec<u8>, i16, [u8; 34])>,
    /// Withdraw requests this batch created and has not yet consumed.
    open_requests: HashMap<[u8; 34], [u8; 34]>,
    /// Withdraw-request outpoints this batch consumed.
    spent_requests: HashSet<[u8; 34]>,
    /// Deposit cells this batch created.
    deposits_created: u64,
    /// Withdraw-request cells this batch consumed by a completion.
    withdrawals_completed: u64,
}

/// The outputs of one transaction as the DAO lifecycle sees them.
struct DaoTxOutputs {
    /// Per output index: the DAO cell carrying a deposit block number there,
    /// if any. RFC-0023 puts the withdraw request for the deposit at input
    /// index `i` at output index `i`, so this is indexed, never searched.
    requests: Vec<Option<NewDaoOutput>>,
    /// Non-DAO outputs, as `(output_index, lock_hash)`: withdraw-to candidates.
    withdraw_to_candidates: Vec<(i16, Vec<u8>)>,
}

impl LiveDaoBatch {
    /// Resolve every DAO transition of `txs` (the batch, in chain order).
    ///
    /// `block_ars` holds the DAO accumulated rate of every block in the batch;
    /// `input_cell` resolves any input outpoint to its cell, committed or
    /// created earlier in the batch.
    pub(crate) fn resolve<'c, F>(
        store: &CkbadgerStore,
        txs: &[TxData],
        block_ars: &HashMap<i64, i64>,
        input_cell: F,
    ) -> Result<Self>
    where
        F: Fn(&(Vec<u8>, i16)) -> Option<&'c PositionedCellInfo>,
    {
        let mut batch = Self::default();
        for tx in txs.iter().filter(|tx| !tx.is_cellbase) {
            batch
                .apply_tx(store, tx, block_ars, &input_cell)
                .with_context(|| {
                    format!(
                        "resolving DAO lifecycle: block={}, tx=0x{}",
                        tx.block_number,
                        hex::encode(tx.hash)
                    )
                })?;
        }
        Ok(batch)
    }

    /// Every DAO cell the batch consumes, keyed by the consumed outpoint.
    pub(crate) fn consumed(&self) -> &DaoConsumedMap {
        &self.consumed
    }

    /// Claimed compensation per consumed withdraw-request outpoint.
    pub(crate) fn compensations(&self) -> &HashMap<(Vec<u8>, i16), i64> {
        &self.compensations
    }

    /// The withdraw-request outpoints this batch consumes (phase-2 inputs).
    pub(crate) fn withdraw_request_outpoints(&self) -> HashSet<(Vec<u8>, i16)> {
        self.compensations.keys().cloned().collect()
    }

    /// The net change this batch makes to the number of live DAO cells: a
    /// deposit adds one, a withdraw request swaps one for one, a completion
    /// removes one.
    pub(crate) fn live_dao_cell_delta(&self) -> i128 {
        i128::from(self.deposits_created) - i128::from(self.withdrawals_completed)
    }

    /// Stage the resolved lifecycle into the batch's domain write batch.
    pub(crate) fn stage(&self, batch: &mut StoreBatch) {
        for (deposit_key, entry) in &self.entries {
            batch.put_dao_deposit(deposit_key, entry);
        }
        for (request_tx, request_index, deposit_key) in &self.request_rows {
            batch.put_dao_by_withdraw_tx(request_tx, *request_index, deposit_key);
        }
    }

    /// Post-batch entries of the deposits this batch created or moved to
    /// phase 1 — including any of them it also completed.
    pub(crate) fn staged_entries(&self) -> HashMap<Vec<u8>, DaoDepositCacheEntry> {
        self.entries
            .iter()
            .filter(|(key, _)| self.lifecycle_in_batch.contains(*key))
            .map(|(key, entry)| (key.to_vec(), entry.clone()))
            .collect()
    }

    /// Completions of requests committed by an earlier batch, as
    /// `deposit outpoint → (withdraw block, withdraw tx)`.
    pub(crate) fn staged_completions(&self) -> Result<HashMap<Vec<u8>, (i64, Vec<u8>)>> {
        self.entries
            .iter()
            .filter(|(key, _)| !self.lifecycle_in_batch.contains(*key))
            .map(|(key, entry)| {
                let (tx_hash, output_index) = keys::decode_outpoint(key);
                match (entry.status, entry.withdraw_block, entry.withdraw_tx.as_ref()) {
                    (2, Some(block), Some(tx)) => Ok((key.to_vec(), (block, tx.clone()))),
                    _ => Err(anyhow!(
                        "DAO deposit touched by the batch without being created, requested or completed by it: deposit=0x{}:{}, status={}",
                        hex::encode(tx_hash),
                        output_index,
                        entry.status
                    )),
                }
            })
            .collect()
    }

    /// The deposit's current entry: this batch's own, else the committed one.
    fn lookup_deposit(
        &self,
        store: &CkbadgerStore,
        deposit_key: &[u8; 34],
    ) -> Result<Option<DaoDepositCacheEntry>> {
        if let Some(entry) = self.entries.get(deposit_key) {
            return Ok(Some(entry.clone()));
        }
        let Some(raw) = store.get_cf(store.cf_dao_deposits(), deposit_key)? else {
            return Ok(None);
        };
        let entry = bincode::deserialize(&raw).map_err(|e| {
            let (tx_hash, output_index) = keys::decode_outpoint(deposit_key);
            anyhow!(
                "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                hex::encode(tx_hash),
                output_index,
                e
            )
        })?;
        Ok(Some(entry))
    }

    /// The deposit a withdraw-request outpoint belongs to: a request this
    /// batch created, else the committed `dao_by_withdraw_tx` row.
    fn lookup_request(
        &self,
        store: &CkbadgerStore,
        request_key: &[u8; 34],
    ) -> Result<Option<[u8; 34]>> {
        if self.spent_requests.contains(request_key) {
            let (tx_hash, output_index) = keys::decode_outpoint(request_key);
            bail!(
                "DAO withdraw request consumed twice in one batch: request=0x{}:{}",
                hex::encode(tx_hash),
                output_index
            );
        }
        if let Some(deposit_key) = self.open_requests.get(request_key) {
            return Ok(Some(*deposit_key));
        }
        let Some(raw) = store.get_cf(store.cf_dao_by_withdraw_tx(), request_key)? else {
            return Ok(None);
        };
        let deposit_key: [u8; 34] = raw.as_slice().try_into().map_err(|_| {
            let (tx_hash, output_index) = keys::decode_outpoint(request_key);
            anyhow!(
                "dao_by_withdraw_tx row is not a deposit outpoint key: request=0x{}:{}, value_len={}",
                hex::encode(tx_hash),
                output_index,
                raw.len()
            )
        })?;
        Ok(Some(deposit_key))
    }

    fn apply_tx<'c, F>(
        &mut self,
        store: &CkbadgerStore,
        tx: &TxData,
        block_ars: &HashMap<i64, i64>,
        input_cell: &F,
    ) -> Result<()>
    where
        F: Fn(&(Vec<u8>, i16)) -> Option<&'c PositionedCellInfo>,
    {
        let block_ar = *block_ars
            .get(&tx.block_number)
            .ok_or_else(|| anyhow!("missing DAO AR for live batch block {}", tx.block_number))?;
        let outputs = dao_tx_outputs(tx)?;

        for (input_index, input) in tx.inputs.iter().enumerate() {
            let output_index = i16::try_from(input.previous_output_index).map_err(|_| {
                anyhow!(
                    "DAO input index exceeds i16 range: previous_output_index={}",
                    input.previous_output_index
                )
            })?;
            let outpoint = (input.previous_tx_hash.to_vec(), output_index);
            let cell = input_cell(&outpoint).ok_or_else(|| {
                anyhow!(
                    "input cell unresolved while resolving DAO lifecycle: outpoint=0x{}:{}",
                    hex::encode(input.previous_tx_hash),
                    output_index
                )
            })?;
            if !cell
                .cell
                .type_code_hash
                .as_deref()
                .is_some_and(DaoParser::is_dao_code_hash)
            {
                continue;
            }

            let outpoint_key = keys::encode_outpoint(&outpoint.0, outpoint.1);
            let as_deposit = self.lookup_deposit(store, &outpoint_key)?;
            let as_request = self.lookup_request(store, &outpoint_key)?;
            match (as_deposit, as_request) {
                (Some(entry), None) => self.apply_withdraw_request(
                    tx,
                    block_ar,
                    input_index,
                    outpoint,
                    outpoint_key,
                    entry,
                    &outputs,
                )?,
                (None, Some(deposit_key)) => self.apply_withdraw_completion(
                    store,
                    tx,
                    outpoint,
                    outpoint_key,
                    deposit_key,
                    &outputs,
                )?,
                (None, None) => bail!(
                    "DAO-typed input resolves to no lifecycle entry: it is neither a recorded deposit nor a recorded withdraw request: outpoint=0x{}:{}",
                    hex::encode(&outpoint.0),
                    outpoint.1
                ),
                (Some(_), Some(deposit_key)) => {
                    let (deposit_tx, deposit_index) = keys::decode_outpoint(&deposit_key);
                    bail!(
                        "DAO input recorded both as a deposit and as the withdraw request of deposit 0x{}:{}: outpoint=0x{}:{}",
                        hex::encode(deposit_tx),
                        deposit_index,
                        hex::encode(&outpoint.0),
                        outpoint.1
                    )
                }
            }
        }

        // Outputs after inputs, as the chain applies them: a transaction can
        // never spend a cell it creates.
        for deposit in DaoParser::parse_deposits_from_cells(&tx.hash, &tx.cells)? {
            let output_index = i16::try_from(deposit.output_index).map_err(|_| {
                anyhow!(
                    "DAO deposit output_index exceeds i16 range: output_index={}",
                    deposit.output_index
                )
            })?;
            let deposit_key = keys::encode_outpoint(&deposit.tx_hash, output_index);
            if self.lookup_deposit(store, &deposit_key)?.is_some() {
                bail!(
                    "duplicate DAO deposit outpoint: deposit=0x{}:{}",
                    hex::encode(&deposit.tx_hash),
                    output_index
                );
            }
            self.entries.insert(
                deposit_key,
                build_dao_cache_entry(
                    &deposit,
                    tx.block_number,
                    block_ar,
                    tx.timestamp.timestamp_millis(),
                ),
            );
            self.lifecycle_in_batch.insert(deposit_key);
            self.deposits_created += 1;
        }
        Ok(())
    }

    /// Phase 1: the deposit `deposit_outpoint`, spent as input
    /// `input_index` of `tx`, becomes the withdraw request at output
    /// `input_index` (RFC-0023; the request's lock is free).
    #[allow(clippy::too_many_arguments)]
    fn apply_withdraw_request(
        &mut self,
        tx: &TxData,
        block_ar: i64,
        input_index: usize,
        deposit_outpoint: (Vec<u8>, i16),
        deposit_key: [u8; 34],
        mut entry: DaoDepositCacheEntry,
        outputs: &DaoTxOutputs,
    ) -> Result<()> {
        if entry.status != 0 {
            bail!(
                "DAO deposit consumed while not an active deposit: deposit=0x{}:{}, status={}",
                hex::encode(&deposit_outpoint.0),
                deposit_outpoint.1,
                entry.status
            );
        }
        let request_output = outputs
            .requests
            .get(input_index)
            .and_then(Option::as_ref)
            .ok_or_else(|| {
                anyhow!(
                    "DAO deposit spent as input {} has no withdraw request at output {}: deposit=0x{}:{}, deposit_block={}",
                    input_index,
                    input_index,
                    hex::encode(&deposit_outpoint.0),
                    deposit_outpoint.1,
                    entry.deposit_block_number,
                )
            })?;
        if request_output.capacity != entry.capacity
            || i64::try_from(request_output.deposit_block).ok() != Some(entry.deposit_block_number)
        {
            bail!(
                "withdraw request at output {} is not the request of the deposit at input {}: deposit=0x{}:{} (capacity={}, deposit_block={}), request capacity={}, request deposit_block={}",
                input_index,
                input_index,
                hex::encode(&deposit_outpoint.0),
                deposit_outpoint.1,
                entry.capacity,
                entry.deposit_block_number,
                request_output.capacity,
                request_output.deposit_block,
            );
        }

        self.consumed.insert(
            deposit_outpoint.clone(),
            dao_cache_entry_to_row(deposit_outpoint.0, deposit_outpoint.1, entry.clone()),
        );

        entry.status = 1;
        entry.withdraw_request_block = Some(tx.block_number);
        entry.withdraw_request_tx = Some(request_output.tx_hash.clone());
        entry.withdraw_request_output_index = Some(request_output.output_index);
        entry.withdraw_request_ar = Some(block_ar);
        // RFC-0023 computes compensation from the WITHDRAWING cell, so persist
        // this request cell's exact occupied capacity — it differs from the
        // deposit cell's whenever the request changes lock script.
        entry.withdraw_request_occupied_capacity = Some(request_output.occupied_capacity);

        let request_key =
            keys::encode_outpoint(&request_output.tx_hash, request_output.output_index);
        if self
            .open_requests
            .insert(request_key, deposit_key)
            .is_some()
        {
            bail!(
                "DAO withdraw-request output claimed twice: request=0x{}:{}",
                hex::encode(&request_output.tx_hash),
                request_output.output_index
            );
        }
        self.request_rows.push((
            request_output.tx_hash.clone(),
            request_output.output_index,
            deposit_key,
        ));
        self.entries.insert(deposit_key, entry);
        self.lifecycle_in_batch.insert(deposit_key);
        Ok(())
    }

    /// Phase 2: the withdraw request `request_outpoint` of deposit
    /// `deposit_key` is consumed by `tx`.
    fn apply_withdraw_completion(
        &mut self,
        store: &CkbadgerStore,
        tx: &TxData,
        request_outpoint: (Vec<u8>, i16),
        request_key: [u8; 34],
        deposit_key: [u8; 34],
        outputs: &DaoTxOutputs,
    ) -> Result<()> {
        let (deposit_tx, deposit_index) = keys::decode_outpoint(&deposit_key);
        let mut entry = self.lookup_deposit(store, &deposit_key)?.ok_or_else(|| {
            anyhow!(
                "withdraw request points at a missing DAO deposit: request=0x{}:{}, deposit=0x{}:{}",
                hex::encode(&request_outpoint.0),
                request_outpoint.1,
                hex::encode(&deposit_tx),
                deposit_index
            )
        })?;
        if entry.status != 1 {
            bail!(
                "DAO withdraw request consumed while its deposit is not pending withdrawal: request=0x{}:{}, deposit=0x{}:{}, status={}",
                hex::encode(&request_outpoint.0),
                request_outpoint.1,
                hex::encode(&deposit_tx),
                deposit_index,
                entry.status
            );
        }
        match entry.withdraw_request_block {
            Some(request_block) if request_block < tx.block_number => {}
            request_block => bail!(
                "DAO withdraw request block missing or not before its completion: request=0x{}:{}, deposit=0x{}:{}, request_block={:?}",
                hex::encode(&request_outpoint.0),
                request_outpoint.1,
                hex::encode(&deposit_tx),
                deposit_index,
                request_block
            ),
        }
        if entry.withdraw_request_tx.as_deref() != Some(request_outpoint.0.as_slice())
            || entry.withdraw_request_output_index != Some(request_outpoint.1)
        {
            bail!(
                "withdraw-request index disagrees with the deposit's recorded request: request=0x{}:{}, deposit=0x{}:{}, recorded_request={:?}:{:?}",
                hex::encode(&request_outpoint.0),
                request_outpoint.1,
                hex::encode(&deposit_tx),
                deposit_index,
                entry.withdraw_request_tx.as_ref().map(hex::encode),
                entry.withdraw_request_output_index
            );
        }
        // The one definition of the claimed amount, frozen at the request AR
        // over the request cell's counted capacity (RFC-0023).
        let compensation = dao_frozen_request_compensation(&entry).with_context(|| {
            format!(
                "DAO withdrawal completion: request=0x{}:{}, deposit=0x{}:{}",
                hex::encode(&request_outpoint.0),
                request_outpoint.1,
                hex::encode(&deposit_tx),
                deposit_index
            )
        })?;

        self.consumed.insert(
            request_outpoint.clone(),
            dao_cache_entry_to_row(deposit_tx, deposit_index, entry.clone()),
        );
        self.compensations.insert(request_outpoint, compensation);

        entry.status = 2;
        entry.withdraw_block = Some(tx.block_number);
        entry.withdraw_tx = Some(tx.hash.to_vec());
        entry.withdraw_to_output_index = infer_withdraw_to_output_index_from_outputs(
            &outputs.withdraw_to_candidates,
            &entry.lock_script_hash,
        );
        entry.compensation = Some(compensation);
        self.open_requests.remove(&request_key);
        self.spent_requests.insert(request_key);
        self.entries.insert(deposit_key, entry);
        self.withdrawals_completed += 1;
        Ok(())
    }
}

fn dao_tx_outputs(tx: &TxData) -> Result<DaoTxOutputs> {
    let mut requests = Vec::with_capacity(tx.cells.len());
    let mut withdraw_to_candidates = Vec::new();
    for (idx, cell) in tx.cells.iter().enumerate() {
        let output_index = i16::try_from(idx)
            .map_err(|_| anyhow!("DAO output index exceeds i16 range: output_index={}", idx))?;
        let is_dao = cell
            .type_code_hash
            .as_deref()
            .is_some_and(DaoParser::is_dao_code_hash);
        if !is_dao || cell.data_size != 8 {
            withdraw_to_candidates.push((output_index, cell.lock_script_hash.clone()));
            requests.push(None);
            continue;
        }
        let Some(deposit_block) = DaoParser::parse_deposit_block_number(&cell.data) else {
            requests.push(None);
            continue;
        };
        let data_size = usize::try_from(cell.data_size).map_err(|_| {
            anyhow!(
                "negative DAO cell data size: output_index={}, data_size={}",
                idx,
                cell.data_size
            )
        })?;
        requests.push(Some(NewDaoOutput {
            tx_hash: tx.hash.to_vec(),
            output_index,
            capacity: cell.capacity,
            // RFC-0023 computes compensation from the withdrawing cell, so the
            // request cell's own occupied capacity is what phase-2 must use.
            occupied_capacity: ckbadger_common::dao::occupied_capacity_shannons(
                data_size,
                cell.lock_args.len(),
                cell.type_args.as_ref().map(Vec::len),
            )?,
            deposit_block,
        }));
    }
    Ok(DaoTxOutputs {
        requests,
        withdraw_to_candidates,
    })
}

fn infer_withdraw_to_output_index_from_outputs(
    candidate_outputs: &[(i16, Vec<u8>)],
    lock_script_hash: &[u8],
) -> Option<i16> {
    if candidate_outputs.is_empty() {
        return None;
    }

    let mut same_lock = candidate_outputs
        .iter()
        .filter_map(|(output_index, output_lock_hash)| {
            (output_lock_hash.as_slice() == lock_script_hash).then_some(*output_index)
        });
    if let Some(first) = same_lock.next() {
        if same_lock.next().is_none() {
            return Some(first);
        }
        return None;
    }

    if candidate_outputs.len() == 1 {
        return Some(candidate_outputs[0].0);
    }

    None
}

impl BatchWriter {
    pub fn get_block_dao_field(&self, block_number: i64) -> Result<Option<Vec<u8>>> {
        if let Some(header) = self.store.get_block_header(block_number)? {
            if !header.dao.is_empty() {
                return Ok(Some(header.dao));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dedup_tx_hashes<'a>(tx_hashes: &[&'a [u8]]) -> Vec<&'a [u8]> {
        let mut seen = std::collections::HashSet::new();
        tx_hashes
            .iter()
            .filter(|h| seen.insert(**h))
            .copied()
            .collect()
    }

    #[test]
    fn test_build_dao_cache_entry_sets_defaults() {
        let deposit = ParsedDaoDeposit {
            tx_hash: vec![0x11; 32],
            output_index: 7,
            lock_script_hash: vec![0x22; 32],
            capacity: 123_456,
            occupied_capacity: 45_678,
        };
        let entry = build_dao_cache_entry(&deposit, 42, 9876, 0);

        assert_eq!(entry.capacity, deposit.capacity);
        assert_eq!(entry.deposit_block_number, 42);
        assert_eq!(entry.lock_script_hash, deposit.lock_script_hash);
        assert_eq!(entry.deposit_ar, 9876);
        assert_eq!(entry.status, 0);
        assert!(entry.withdraw_request_tx.is_none());
        assert!(entry.withdraw_request_output_index.is_none());
        assert!(entry.withdraw_request_block.is_none());
        assert!(entry.withdraw_request_ar.is_none());
        assert!(entry.withdraw_block.is_none());
        assert!(entry.withdraw_tx.is_none());
        assert!(entry.withdraw_to_output_index.is_none());
        assert!(entry.compensation.is_none());
    }

    #[test]
    fn test_dao_cache_entry_to_row_maps_fields() {
        let entry = DaoDepositCacheEntry {
            capacity: 999,
            occupied_capacity: 111,
            deposit_block_number: 77,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x33; 32],
            deposit_ar: 123,
            status: 1,
            withdraw_request_tx: Some(vec![0x44; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(88),
            withdraw_request_ar: Some(456),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: Some(111),
            withdraw_to_output_index: None,
            compensation: None,
        };
        let row = dao_cache_entry_to_row(vec![0xaa; 32], 3, entry);

        assert_eq!(row.tx_hash, vec![0xaa; 32]);
        assert_eq!(row.output_index, 3);
        assert_eq!(row.capacity_str, "999");
        assert_eq!(row.deposit_block, 77);
        assert_eq!(row.status, 1);
        assert_eq!(row.lock_script_hash, vec![0x33; 32]);
    }

    #[test]
    fn test_dedup_tx_hashes_removes_duplicates() {
        let h1 = vec![0xaa; 32];
        let h2 = vec![0xbb; 32];
        let input: Vec<&[u8]> = vec![&h1, &h2, &h1, &h2, &h1];

        let result = dedup_tx_hashes(&input);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], h1.as_slice());
        assert_eq!(result[1], h2.as_slice());
    }

    #[test]
    fn test_dedup_tx_hashes_preserves_order() {
        let h1 = vec![0x01; 32];
        let h2 = vec![0x02; 32];
        let h3 = vec![0x03; 32];
        let input: Vec<&[u8]> = vec![&h3, &h1, &h2, &h3, &h1];

        let result = dedup_tx_hashes(&input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], h3.as_slice());
        assert_eq!(result[1], h1.as_slice());
        assert_eq!(result[2], h2.as_slice());
    }

    #[test]
    fn test_dedup_tx_hashes_empty_input() {
        let input: Vec<&[u8]> = vec![];
        assert!(dedup_tx_hashes(&input).is_empty());
    }

    #[test]
    fn test_dedup_tx_hashes_all_same() {
        let h = vec![0xff; 32];
        let input: Vec<&[u8]> = vec![&h, &h, &h, &h];

        let result = dedup_tx_hashes(&input);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_dedup_tx_hashes_all_unique() {
        let hashes: Vec<Vec<u8>> = (0..5u8).map(|i| vec![i; 32]).collect();
        let input: Vec<&[u8]> = hashes.iter().map(|h| h.as_slice()).collect();

        let result = dedup_tx_hashes(&input);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_dedup_realistic_batch_reduction() {
        let hashes: Vec<Vec<u8>> = (0..100u8).map(|i| vec![i; 32]).collect();
        let mut input: Vec<&[u8]> = Vec::new();
        for h in &hashes {
            input.push(h.as_slice());
            input.push(h.as_slice());
            input.push(h.as_slice());
        }
        assert_eq!(input.len(), 300);

        let result = dedup_tx_hashes(&input);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn test_infer_withdraw_to_output_index_prefers_unique_same_lock() {
        let candidate_outputs = vec![
            (0, vec![0x10; 32]),
            (2, vec![0x20; 32]),
            (4, vec![0x30; 32]),
        ];

        let idx = infer_withdraw_to_output_index_from_outputs(&candidate_outputs, &[0x20; 32]);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_infer_withdraw_to_output_index_returns_single_candidate_when_unambiguous() {
        let candidate_outputs = vec![(3, vec![0x10; 32])];
        let idx = infer_withdraw_to_output_index_from_outputs(&candidate_outputs, &[0x99; 32]);
        assert_eq!(idx, Some(3));
    }

    #[test]
    fn test_infer_withdraw_to_output_index_returns_none_when_ambiguous() {
        let candidate_outputs = vec![(1, vec![0x10; 32]), (2, vec![0x11; 32])];
        let idx = infer_withdraw_to_output_index_from_outputs(&candidate_outputs, &[0x99; 32]);
        assert!(idx.is_none());
    }

    #[test]
    fn test_calculate_dao_compensation_from_ar_errors_on_capacity_below_occupied() {
        let err =
            calculate_dao_compensation_from_ar(100_00000000, 102_00000000, 100, 110).unwrap_err();
        assert!(err.to_string().contains("below occupied capacity"));
    }

    #[test]
    fn test_calculate_dao_compensation_from_ar_errors_on_ar_underflow() {
        let err =
            calculate_dao_compensation_from_ar(200_00000000, 102_00000000, 100, 90).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_calculate_dao_compensation_from_ar_errors_on_zero_deposit_ar() {
        let err =
            calculate_dao_compensation_from_ar(200_00000000, 102_00000000, 0, 100).unwrap_err();
        assert!(err.to_string().contains("zero deposit AR"));
    }

    // ── LiveDaoBatch ───────────────────────────────────────────────────

    use crate::parser::cell::ParsedCell;
    use crate::parser::transaction::ParsedInput;
    use ckbadger_store::types::LiveCellInfo;

    const AR_DEPOSIT: i64 = 10_000_000_000_000_000;
    const AR_REQUEST: i64 = 10_100_000_000_000_000;
    const AR_COMPLETION: i64 = 10_200_000_000_000_000;
    /// 1000 CKB, standard 102 CKB occupied: 898 CKB earn 1% between the
    /// deposit and request ARs.
    const DEPOSIT_CAPACITY: i64 = 1000_00000000;
    const COMPENSATION: i64 = 8_98000000;
    const LOCK: [u8; 32] = [0x71; 32];

    fn dao_cell(capacity: i64, lock: &[u8], data: [u8; 8]) -> ParsedCell {
        ParsedCell {
            capacity,
            lock_code_hash: vec![0x9b; 32],
            lock_hash_type: 1,
            // 20-byte args: the standard secp DAO cell is 102 CKB occupied.
            lock_args: vec![0x01; 20],
            lock_script_hash: lock.to_vec(),
            type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                crate::parser::dao::DAO_CODE_HASH,
            )),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![0xcc; 32]),
            data_hash: [0u8; 32],
            data_size: 8,
            data: data.to_vec(),
        }
    }

    fn deposit_cell(capacity: i64, lock: &[u8]) -> ParsedCell {
        dao_cell(capacity, lock, [0u8; 8])
    }

    fn request_cell(capacity: i64, lock: &[u8], deposit_block: u64) -> ParsedCell {
        dao_cell(capacity, lock, deposit_block.to_le_bytes())
    }

    fn plain_cell(capacity: i64, lock: &[u8]) -> ParsedCell {
        ParsedCell {
            capacity,
            lock_code_hash: vec![0x9b; 32],
            lock_hash_type: 1,
            lock_args: vec![0x01; 20],
            lock_script_hash: lock.to_vec(),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: [0u8; 32],
            data_size: 0,
            data: vec![],
        }
    }

    fn tx(
        hash: u8,
        block_number: i64,
        inputs: &[([u8; 32], i32)],
        cells: Vec<ParsedCell>,
    ) -> TxData {
        TxData {
            hash: [hash; 32],
            block_number,
            tx_index: 1,
            inputs_count: i16::try_from(inputs.len()).unwrap(),
            outputs_count: i16::try_from(cells.len()).unwrap(),
            is_cellbase: false,
            inputs: inputs
                .iter()
                .map(|(previous_tx_hash, previous_output_index)| ParsedInput {
                    previous_tx_hash: *previous_tx_hash,
                    previous_output_index: *previous_output_index,
                    since: 0,
                })
                .collect(),
            cells,
            witnesses: vec![],
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + block_number)
                .unwrap(),
            semantic_tags: 0,
        }
    }

    fn cell_info(cell: &ParsedCell, created_at_block: i64) -> PositionedCellInfo {
        PositionedCellInfo::new(
            LiveCellInfo {
                capacity: cell.capacity,
                lock_script_hash: cell.lock_script_hash.clone(),
                lock_code_hash: cell.lock_code_hash.clone(),
                lock_hash_type: cell.lock_hash_type,
                lock_args: cell.lock_args.clone(),
                type_script_hash: cell.type_script_hash.clone(),
                type_code_hash: cell.type_code_hash.clone(),
                type_hash_type: cell.type_hash_type,
                type_args: cell.type_args.clone(),
                data_size: cell.data_size,
                occupied_capacity: 102_00000000,
                udt_amount: None,
                data_hash: Some(cell.data_hash.to_vec()),
            },
            created_at_block,
        )
    }

    fn ars(blocks: &[(i64, i64)]) -> HashMap<i64, i64> {
        blocks.iter().copied().collect()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: CkbadgerStore,
        /// Committed cells the batch may spend.
        committed: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = CkbadgerStore::open_domain(dir.path()).unwrap();
            Self {
                _dir: dir,
                store,
                committed: HashMap::new(),
            }
        }

        fn commit_cell(&mut self, outpoint: ([u8; 32], i16), cell: &ParsedCell, block: i64) {
            self.committed
                .insert((outpoint.0.to_vec(), outpoint.1), cell_info(cell, block));
        }

        fn commit_entry(&self, deposit: ([u8; 32], i16), entry: &DaoDepositCacheEntry) {
            let mut batch = StoreBatch::new(&self.store);
            let key = keys::encode_outpoint(&deposit.0, deposit.1);
            batch.put_dao_deposit(&key, entry);
            if let (Some(request_tx), Some(request_index)) = (
                entry.withdraw_request_tx.as_ref(),
                entry.withdraw_request_output_index,
            ) {
                batch.put_dao_by_withdraw_tx(request_tx, request_index, &key);
            }
            batch.commit().unwrap();
        }

        /// Resolve `txs` with every input visible: committed cells plus the
        /// outputs of `txs` themselves (the live `batch_cell_infos`).
        fn resolve(&self, txs: &[TxData], block_ars: &HashMap<i64, i64>) -> Result<LiveDaoBatch> {
            let mut batch_cells = HashMap::new();
            for tx in txs {
                // Outputs past the i16 range are never spendable inputs here;
                // the resolver itself rejects them.
                for (idx, cell) in tx.cells.iter().enumerate() {
                    let Ok(output_index) = i16::try_from(idx) else {
                        break;
                    };
                    batch_cells.insert(
                        (tx.hash.to_vec(), output_index),
                        cell_info(cell, tx.block_number),
                    );
                }
            }
            LiveDaoBatch::resolve(&self.store, txs, block_ars, |key| {
                self.committed.get(key).or_else(|| batch_cells.get(key))
            })
        }

        fn stored(&self, deposit: ([u8; 32], i16)) -> DaoDepositCacheEntry {
            let key = keys::encode_outpoint(&deposit.0, deposit.1);
            bincode::deserialize(
                &self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &key)
                    .unwrap()
                    .expect("deposit entry"),
            )
            .unwrap()
        }
    }

    fn active_entry(deposit_block: i64) -> DaoDepositCacheEntry {
        DaoDepositCacheEntry {
            capacity: DEPOSIT_CAPACITY,
            occupied_capacity: 102_00000000,
            deposit_block_number: deposit_block,
            deposit_timestamp: 1_700_000_000_000,
            lock_script_hash: LOCK.to_vec(),
            deposit_ar: AR_DEPOSIT,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
            withdraw_to_output_index: None,
            compensation: None,
        }
    }

    fn requested_entry(
        deposit_block: i64,
        request: ([u8; 32], i16),
        request_block: i64,
    ) -> DaoDepositCacheEntry {
        DaoDepositCacheEntry {
            status: 1,
            withdraw_request_tx: Some(request.0.to_vec()),
            withdraw_request_output_index: Some(request.1),
            withdraw_request_block: Some(request_block),
            withdraw_request_ar: Some(AR_REQUEST),
            withdraw_request_occupied_capacity: Some(102_00000000),
            ..active_entry(deposit_block)
        }
    }

    const DEPOSIT: ([u8; 32], i16) = ([0xd1; 32], 0);
    const REQUEST: ([u8; 32], i16) = ([0xd2; 32], 0);

    fn request_tx(block: i64) -> TxData {
        tx(
            0xd2,
            block,
            &[(DEPOSIT.0, 0)],
            vec![request_cell(DEPOSIT_CAPACITY, &LOCK, 101)],
        )
    }

    fn completion_tx(block: i64) -> TxData {
        tx(
            0xd3,
            block,
            &[(REQUEST.0, 0)],
            vec![plain_cell(DEPOSIT_CAPACITY + COMPENSATION - 527, &LOCK)],
        )
    }

    /// The 2026-10-23 incident shape: the deposit is committed; its withdraw
    /// request and the completion that spends that request are both in the
    /// batch. The completion must resolve through the request this batch
    /// created, not only through committed `dao_by_withdraw_tx` rows.
    #[test]
    fn test_live_dao_batch_completes_request_made_earlier_in_the_batch() {
        let mut fx = Fixture::new();
        fx.commit_cell(DEPOSIT, &deposit_cell(DEPOSIT_CAPACITY, &LOCK), 101);
        fx.commit_entry(DEPOSIT, &active_entry(101));

        let resolved = fx
            .resolve(
                &[request_tx(102), completion_tx(103)],
                &ars(&[(102, AR_REQUEST), (103, AR_COMPLETION)]),
            )
            .unwrap();

        let deposit_row = &resolved.consumed()[&(DEPOSIT.0.to_vec(), 0)];
        assert_eq!(deposit_row.status, 0);
        let request_row = &resolved.consumed()[&(REQUEST.0.to_vec(), 0)];
        assert_eq!(request_row.status, 1);
        assert_eq!(request_row.tx_hash, DEPOSIT.0.to_vec());
        assert_eq!(request_row.output_index, DEPOSIT.1);
        assert_eq!(
            resolved.compensations()[&(REQUEST.0.to_vec(), 0)],
            COMPENSATION
        );
        assert_eq!(
            resolved.withdraw_request_outpoints(),
            HashSet::from([(REQUEST.0.to_vec(), 0)])
        );

        let staged = resolved.staged_entries();
        let entry = &staged[&keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1).to_vec()];
        assert_eq!(entry.status, 2);
        assert_eq!(entry.withdraw_request_block, Some(102));
        assert_eq!(entry.withdraw_request_ar, Some(AR_REQUEST));
        assert_eq!(entry.withdraw_block, Some(103));
        assert_eq!(entry.withdraw_tx, Some(vec![0xd3; 32]));
        assert_eq!(entry.withdraw_to_output_index, Some(0));
        assert_eq!(entry.compensation, Some(COMPENSATION));
        assert!(
            resolved.staged_completions().unwrap().is_empty(),
            "a deposit whose phase 1 is in the batch is staged as an entry, never also as a completion"
        );

        let mut batch = StoreBatch::new(&fx.store);
        resolved.stage(&mut batch);
        batch.commit().unwrap();
        assert_eq!(fx.stored(DEPOSIT), *entry);
        assert_eq!(
            fx.store
                .get_dao_deposit_by_withdraw_tx(&REQUEST.0, REQUEST.1)
                .unwrap(),
            Some(keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1).to_vec())
        );
    }

    /// Deposit, withdraw request and completion all inside one batch.
    #[test]
    fn test_live_dao_batch_resolves_whole_lifecycle_in_one_batch() {
        let fx = Fixture::new();
        let deposit_tx = tx(
            0xd1,
            101,
            &[],
            vec![deposit_cell(DEPOSIT_CAPACITY, &LOCK), plain_cell(1, &LOCK)],
        );
        let resolved = fx
            .resolve(
                &[deposit_tx, request_tx(102), completion_tx(103)],
                &ars(&[(101, AR_DEPOSIT), (102, AR_REQUEST), (103, AR_COMPLETION)]),
            )
            .unwrap();

        let staged = resolved.staged_entries();
        assert_eq!(staged.len(), 1);
        let entry = &staged[&keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1).to_vec()];
        assert_eq!(entry.status, 2);
        assert_eq!(entry.deposit_block_number, 101);
        assert_eq!(entry.deposit_ar, AR_DEPOSIT);
        assert_eq!(entry.compensation, Some(COMPENSATION));
        assert!(resolved.staged_completions().unwrap().is_empty());
        assert_eq!(resolved.consumed().len(), 2);
        assert_eq!(
            resolved.live_dao_cell_delta(),
            0,
            "a deposit created and fully withdrawn in one batch nets zero live DAO cells"
        );
    }

    /// A completion of a request committed by an earlier batch stays a
    /// staged completion: the snapshot overlay projects it from the
    /// committed phase-1 entry.
    #[test]
    fn test_live_dao_batch_completion_of_committed_request_is_a_staged_completion() {
        let mut fx = Fixture::new();
        fx.commit_cell(REQUEST, &request_cell(DEPOSIT_CAPACITY, &LOCK, 101), 102);
        fx.commit_entry(DEPOSIT, &requested_entry(101, REQUEST, 102));

        let resolved = fx
            .resolve(&[completion_tx(103)], &ars(&[(103, AR_COMPLETION)]))
            .unwrap();

        assert!(resolved.staged_entries().is_empty());
        assert_eq!(
            resolved.staged_completions().unwrap(),
            HashMap::from([(
                keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1).to_vec(),
                (103, vec![0xd3; 32])
            )])
        );
        assert_eq!(
            resolved.compensations()[&(REQUEST.0.to_vec(), 0)],
            COMPENSATION
        );
        assert_eq!(resolved.live_dao_cell_delta(), -1);
        let mut batch = StoreBatch::new(&fx.store);
        resolved.stage(&mut batch);
        batch.commit().unwrap();
        assert_eq!(fx.stored(DEPOSIT).status, 2);
    }

    /// A DAO-typed input with neither a deposit entry nor a request row is a
    /// lost lifecycle fact. It must stop the batch, never be skipped.
    #[test]
    fn test_live_dao_batch_fails_on_dao_input_without_lifecycle_entry() {
        let mut fx = Fixture::new();
        fx.commit_cell(REQUEST, &request_cell(DEPOSIT_CAPACITY, &LOCK, 101), 102);

        let err = fx
            .resolve(&[completion_tx(103)], &ars(&[(103, AR_COMPLETION)]))
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("resolves to no lifecycle entry"), "{msg}");
        assert!(msg.contains("block=103"), "{msg}");
        assert!(msg.contains(&hex::encode([0xd3; 32])), "{msg}");
    }

    #[test]
    fn test_live_dao_batch_rejects_request_row_that_disagrees_with_deposit() {
        let mut fx = Fixture::new();
        let stray = ([0xee; 32], 0i16);
        fx.commit_cell(stray, &request_cell(DEPOSIT_CAPACITY, &LOCK, 101), 102);
        fx.commit_entry(DEPOSIT, &requested_entry(101, REQUEST, 102));
        let mut batch = StoreBatch::new(&fx.store);
        batch.put_dao_by_withdraw_tx(
            &stray.0,
            stray.1,
            &keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1),
        );
        batch.commit().unwrap();

        let completion = tx(0xd3, 103, &[(stray.0, 0)], vec![plain_cell(1, &LOCK)]);
        let err = fx
            .resolve(&[completion], &ars(&[(103, AR_COMPLETION)]))
            .unwrap_err();
        assert!(format!("{err:#}").contains("disagrees"), "{err:#}");
    }

    #[test]
    fn test_live_dao_batch_rejects_completing_a_completed_deposit() {
        let mut fx = Fixture::new();
        fx.commit_cell(REQUEST, &request_cell(DEPOSIT_CAPACITY, &LOCK, 101), 102);
        fx.commit_entry(
            DEPOSIT,
            &DaoDepositCacheEntry {
                status: 2,
                withdraw_block: Some(103),
                withdraw_tx: Some(vec![0xd3; 32]),
                compensation: Some(COMPENSATION),
                ..requested_entry(101, REQUEST, 102)
            },
        );

        let err = fx
            .resolve(&[completion_tx(104)], &ars(&[(104, AR_COMPLETION)]))
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("not pending withdrawal"),
            "{err:#}"
        );
    }

    #[test]
    fn test_live_dao_batch_rejects_completion_without_request_block() {
        let mut fx = Fixture::new();
        fx.commit_cell(REQUEST, &request_cell(DEPOSIT_CAPACITY, &LOCK, 101), 102);
        fx.commit_entry(
            DEPOSIT,
            &DaoDepositCacheEntry {
                withdraw_request_block: None,
                ..requested_entry(101, REQUEST, 102)
            },
        );

        let err = fx
            .resolve(&[completion_tx(103)], &ars(&[(103, AR_COMPLETION)]))
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("request block missing"),
            "{err:#}"
        );
    }

    #[test]
    fn test_live_dao_batch_completion_errors_when_capacity_below_occupied() {
        let mut fx = Fixture::new();
        fx.commit_cell(REQUEST, &request_cell(100_00000000, &LOCK, 101), 102);
        fx.commit_entry(
            DEPOSIT,
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                occupied_capacity: 0,
                ..requested_entry(101, REQUEST, 102)
            },
        );

        let err = fx
            .resolve(&[completion_tx(103)], &ars(&[(103, AR_COMPLETION)]))
            .unwrap_err();
        assert!(format!("{err:#}").contains("below occupied"), "{err:#}");
    }

    /// The request AR is the request block's own AR, read from the batch —
    /// that block's header is not committed yet.
    #[test]
    fn test_live_dao_batch_request_records_its_block_ar() {
        let mut fx = Fixture::new();
        fx.commit_cell(DEPOSIT, &deposit_cell(DEPOSIT_CAPACITY, &LOCK), 101);
        fx.commit_entry(DEPOSIT, &active_entry(101));

        let resolved = fx
            .resolve(&[request_tx(102)], &ars(&[(102, AR_REQUEST)]))
            .unwrap();
        let staged = resolved.staged_entries();
        let entry = &staged[&keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1).to_vec()];
        assert_eq!(entry.status, 1);
        assert_eq!(entry.withdraw_request_block, Some(102));
        assert_eq!(entry.withdraw_request_ar, Some(AR_REQUEST));
        assert_eq!(entry.withdraw_request_occupied_capacity, Some(102_00000000));
        assert!(resolved.compensations().is_empty());
    }

    #[test]
    fn test_live_dao_batch_fails_without_block_ar() {
        let mut fx = Fixture::new();
        fx.commit_cell(DEPOSIT, &deposit_cell(DEPOSIT_CAPACITY, &LOCK), 101);
        fx.commit_entry(DEPOSIT, &active_entry(101));

        let err = fx.resolve(&[request_tx(102)], &HashMap::new()).unwrap_err();
        assert!(format!("{err:#}").contains("missing DAO AR"), "{err:#}");
    }

    /// RFC-0023: the withdraw request for the deposit at input index `i` is
    /// the output at index `i`; its lock is free. Two same-block,
    /// same-capacity deposits whose requests swap locks must still pair by
    /// position — any other pairing hands each deposit the other's request
    /// outpoint and request occupied capacity, so the completion of one
    /// would complete the other at the wrong compensation.
    #[test]
    fn test_live_dao_batch_pairs_each_deposit_with_the_output_at_its_input_index() {
        let mut fx = Fixture::new();
        let lock_a = [0xa1u8; 32];
        let lock_b = [0xb2u8; 32];
        let deposit_a = ([0xd1; 32], 0i16);
        let deposit_b = ([0xd1; 32], 1i16);
        fx.commit_cell(deposit_a, &deposit_cell(DEPOSIT_CAPACITY, &lock_a), 101);
        fx.commit_cell(deposit_b, &deposit_cell(DEPOSIT_CAPACITY, &lock_b), 101);
        fx.commit_entry(
            deposit_a,
            &DaoDepositCacheEntry {
                lock_script_hash: lock_a.to_vec(),
                ..active_entry(101)
            },
        );
        fx.commit_entry(
            deposit_b,
            &DaoDepositCacheEntry {
                lock_script_hash: lock_b.to_vec(),
                ..active_entry(101)
            },
        );

        // Output 0 is A's request, but under B's lock with 32-byte args
        // (114 CKB occupied); output 1 is B's request under A's lock (102).
        let mut request_for_a = request_cell(DEPOSIT_CAPACITY, &lock_b, 101);
        request_for_a.lock_args = vec![0x02; 32];
        let request_for_b = request_cell(DEPOSIT_CAPACITY, &lock_a, 101);
        let request = tx(
            0xd2,
            102,
            &[(deposit_a.0, 0), (deposit_b.0, 1)],
            vec![request_for_a, request_for_b],
        );
        let resolved = fx.resolve(&[request], &ars(&[(102, AR_REQUEST)])).unwrap();
        let staged = resolved.staged_entries();

        let a = &staged[&keys::encode_outpoint(&deposit_a.0, deposit_a.1).to_vec()];
        assert_eq!(a.withdraw_request_output_index, Some(0), "{a:?}");
        assert_eq!(a.withdraw_request_occupied_capacity, Some(114_00000000));
        let b = &staged[&keys::encode_outpoint(&deposit_b.0, deposit_b.1).to_vec()];
        assert_eq!(b.withdraw_request_output_index, Some(1), "{b:?}");
        assert_eq!(b.withdraw_request_occupied_capacity, Some(102_00000000));
    }

    /// The output at the deposit's input index must be that deposit's
    /// withdraw request: DAO-typed, the same capacity, the deposit's own
    /// block in its data. Anything else is a transaction the DAO script
    /// rejects, so it stops the batch instead of being paired elsewhere.
    #[test]
    fn test_live_dao_batch_phase1_rejects_an_output_that_is_not_the_deposits_request() {
        let mut fx = Fixture::new();
        fx.commit_cell(DEPOSIT, &deposit_cell(DEPOSIT_CAPACITY, &LOCK), 101);
        fx.commit_entry(DEPOSIT, &active_entry(101));
        let resolve = |cells: Vec<ParsedCell>| {
            fx.resolve(
                &[tx(0xd2, 102, &[(DEPOSIT.0, 0)], cells)],
                &ars(&[(102, AR_REQUEST)]),
            )
        };

        // The right request exists, but at output 1, not output 0.
        let err = resolve(vec![
            request_cell(DEPOSIT_CAPACITY, &LOCK, 99),
            request_cell(DEPOSIT_CAPACITY, &LOCK, 101),
        ])
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("is not the request of the deposit at input 0"),
            "{err:#}"
        );
        let err = resolve(vec![request_cell(DEPOSIT_CAPACITY - 1, &LOCK, 101)]).unwrap_err();
        assert!(
            format!("{err:#}").contains("is not the request of the deposit at input 0"),
            "{err:#}"
        );
        let err = resolve(vec![plain_cell(DEPOSIT_CAPACITY, &LOCK)]).unwrap_err();
        assert!(
            format!("{err:#}").contains("has no withdraw request at output 0"),
            "{err:#}"
        );
    }

    #[test]
    fn test_live_dao_batch_phase1_allows_output_lock_change() {
        let mut fx = Fixture::new();
        fx.commit_cell(DEPOSIT, &deposit_cell(DEPOSIT_CAPACITY, &LOCK), 101);
        fx.commit_entry(DEPOSIT, &active_entry(101));

        let request = tx(
            0xd2,
            102,
            &[(DEPOSIT.0, 0)],
            vec![request_cell(DEPOSIT_CAPACITY, &[0x44; 32], 101)],
        );
        let resolved = fx.resolve(&[request], &ars(&[(102, AR_REQUEST)])).unwrap();
        let staged = resolved.staged_entries();
        let entry = &staged[&keys::encode_outpoint(&DEPOSIT.0, DEPOSIT.1).to_vec()];
        assert_eq!(entry.status, 1);
        assert_eq!(entry.withdraw_request_tx, Some(REQUEST.0.to_vec()));
        assert_eq!(entry.withdraw_request_output_index, Some(0));
        // The deposit keeps its own lock; only the request cell changed it.
        assert_eq!(entry.lock_script_hash, LOCK.to_vec());
    }

    #[test]
    fn test_live_dao_batch_errors_on_output_index_overflow() {
        let fx = Fixture::new();
        // One output past the i16 range, the last of them a deposit.
        let mut deposit = tx(0xd1, 101, &[], vec![]);
        deposit.cells = vec![plain_cell(1, &LOCK); usize::try_from(i16::MAX).unwrap() + 1];
        deposit.cells.push(deposit_cell(DEPOSIT_CAPACITY, &LOCK));
        let err = fx
            .resolve(&[deposit], &ars(&[(101, AR_DEPOSIT)]))
            .unwrap_err();
        assert!(format!("{err:#}").contains("exceeds i16 range"), "{err:#}");
    }
}
