use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::DaoDepositCacheEntry;
use std::collections::{HashMap, HashSet};

use crate::parser::ParsedDaoDeposit;

use super::BatchWriter;

const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;

fn build_dao_cache_entry(
    deposit: &ParsedDaoDeposit,
    block_number: i64,
    deposit_ar: i64,
) -> DaoDepositCacheEntry {
    DaoDepositCacheEntry {
        capacity: deposit.capacity,
        deposit_block_number: block_number,
        lock_script_hash: deposit.lock_script_hash.clone(),
        deposit_ar,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    }
}

fn dao_cache_entry_to_row(
    tx_hash: Vec<u8>,
    output_index: i16,
    entry: DaoDepositCacheEntry,
) -> (Vec<u8>, i16, String, i64, i16) {
    (
        tx_hash,
        output_index,
        entry.capacity.to_string(),
        entry.deposit_block_number,
        entry.status,
    )
}

pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[(Vec<u8>, i16, String, i64, i16)];
    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)];
    fn block_number(&self) -> i64;
    fn consuming_tx_hash(&self) -> &[u8];
    fn timestamp(&self) -> DateTime<Utc>;
    fn withdraw_to_output_index_for_lock(&self, _lock_script_hash: &[u8]) -> Option<i16> {
        None
    }
    fn infer_request_output_index(&self, _request_tx_hash: &[u8]) -> Option<i16> {
        None
    }
}

#[derive(Clone)]
pub struct DaoWithdrawalContext {
    pub consumed_deposits: Vec<(Vec<u8>, i16, String, i64, i16)>,
    pub new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
    pub tx_inputs: Vec<(Vec<u8>, i16)>,
    pub candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)>,
    pub block_number: i64,
    pub consuming_tx_hash: Vec<u8>,
    pub timestamp: DateTime<Utc>,
}

impl DaoWithdrawalContextTrait for DaoWithdrawalContext {
    fn consumed_deposits(&self) -> &[(Vec<u8>, i16, String, i64, i16)] {
        &self.consumed_deposits
    }

    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
        &self.new_dao_outputs
    }

    fn block_number(&self) -> i64 {
        self.block_number
    }

    fn consuming_tx_hash(&self) -> &[u8] {
        &self.consuming_tx_hash
    }

    fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    fn withdraw_to_output_index_for_lock(&self, lock_script_hash: &[u8]) -> Option<i16> {
        infer_withdraw_to_output_index_from_outputs(
            &self.candidate_withdraw_to_outputs,
            lock_script_hash,
        )
    }

    fn infer_request_output_index(&self, request_tx_hash: &[u8]) -> Option<i16> {
        infer_request_output_index_from_inputs(&self.tx_inputs, request_tx_hash)
    }
}

pub(crate) fn extract_ar_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

pub(crate) fn calculate_dao_compensation_from_ar(
    capacity: i64,
    ar_deposit: u64,
    ar_withdraw: u64,
) -> Result<i64> {
    if ar_deposit == 0 {
        return Ok(0);
    }

    let capacity_u128 = u128::try_from(capacity)
        .map_err(|_| anyhow!("DAO capacity is negative: capacity={}", capacity))?;
    let occupied = DAO_OCCUPIED_CAPACITY as u128;
    if capacity_u128 < occupied {
        bail!(
            "DAO capacity below occupied capacity: capacity={}, occupied={}",
            capacity,
            DAO_OCCUPIED_CAPACITY
        );
    }
    let free_capacity = capacity_u128 - occupied;
    let gross = free_capacity
        .checked_mul(ar_withdraw as u128)
        .ok_or_else(|| anyhow!("DAO compensation multiply overflow"))?
        / (ar_deposit as u128);
    let compensation_u128 = gross.checked_sub(free_capacity).ok_or_else(|| {
        anyhow!(
            "DAO compensation underflow: free_capacity={}, ar_deposit={}, ar_withdraw={}",
            free_capacity,
            ar_deposit,
            ar_withdraw
        )
    })?;
    i64::try_from(compensation_u128)
        .map_err(|_| anyhow!("DAO compensation exceeds i64: {}", compensation_u128))
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

fn select_phase1_output_for_deposit<'a>(
    new_dao_outputs: &'a [(Vec<u8>, i16, Vec<u8>, i64, u64)],
    consumed_output_indices: &HashSet<usize>,
    capacity: i64,
    deposit_block_number: i64,
) -> Result<Option<(usize, &'a (Vec<u8>, i16, Vec<u8>, i64, u64))>> {
    let deposit_block_u64 = u64::try_from(deposit_block_number).map_err(|_| {
        anyhow!(
            "invalid negative DAO deposit block number while matching phase-1 output: {}",
            deposit_block_number
        )
    })?;
    Ok(new_dao_outputs
        .iter()
        .enumerate()
        .filter(|(pos, (_, _, _, cap, output_deposit_block))| {
            *cap == capacity
                && *output_deposit_block == deposit_block_u64
                && !consumed_output_indices.contains(pos)
        })
        // Use output index as a deterministic tie-breaker when exact metadata repeats.
        .min_by_key(|(pos, (_, output_index, _, _, _))| (*output_index, *pos)))
}

fn infer_request_output_index_from_inputs(
    tx_inputs: &[(Vec<u8>, i16)],
    request_tx_hash: &[u8],
) -> Option<i16> {
    let mut matches = tx_inputs
        .iter()
        .filter_map(|(tx_hash, output_index)| {
            (tx_hash.as_slice() == request_tx_hash).then_some(*output_index)
        })
        .take(2);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
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

    pub fn find_consumed_dao_deposits(
        &self,
        inputs: &[(&[u8], i32)],
    ) -> Result<Vec<(Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::new();
        let mut seen_keys: HashSet<(Vec<u8>, i16)> = HashSet::new();

        // Check direct deposits (tx_hash, output_index)
        for (tx_hash, output_index_raw) in inputs {
            if *output_index_raw < 0 {
                bail!(
                    "negative DAO input output_index in find_consumed_dao_deposits: tx_hash=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index_raw
                );
            }
            let output_index = i16::try_from(*output_index_raw).map_err(|_| {
                anyhow!(
                    "DAO input output_index exceeds i16 range in find_consumed_dao_deposits: tx_hash=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index_raw
                )
            })?;
            let outpoint_key = keys::encode_outpoint(tx_hash, output_index);
            if let Some(value) = self
                .store
                .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
            {
                let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                    anyhow!(
                        "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                        hex::encode(tx_hash),
                        output_index_raw,
                        e
                    )
                })?;
                let key = (tx_hash.to_vec(), output_index);
                seen_keys.insert(key);
                results.push(dao_cache_entry_to_row(
                    tx_hash.to_vec(),
                    output_index,
                    entry,
                ));
            }
        }

        // Check by withdraw_request_tx outpoint (Phase 2 withdrawals).
        // Each input's (tx_hash, output_index) is the full outpoint of the
        // withdrawal request cell being consumed.
        for (tx_hash, output_index_raw) in inputs {
            if *output_index_raw < 0 {
                bail!(
                    "negative DAO input output_index in find_consumed_dao_deposits: tx_hash=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index_raw
                );
            }
            let output_index = i16::try_from(*output_index_raw).map_err(|_| {
                anyhow!(
                    "DAO input output_index exceeds i16 range in find_consumed_dao_deposits: tx_hash=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index_raw
                )
            })?;
            let withdraw_outpoint_key = keys::encode_outpoint(tx_hash, output_index);
            if let Some(deposit_outpoint_key) = self
                .store
                .get_cf(self.store.cf_dao_by_withdraw_tx(), &withdraw_outpoint_key)?
            {
                if let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &deposit_outpoint_key)?
                {
                    let entry: DaoDepositCacheEntry =
                        bincode::deserialize(&value).map_err(|e| {
                            let (orig_tx, orig_idx) = keys::decode_outpoint(&deposit_outpoint_key);
                            anyhow!(
                                "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                                hex::encode(orig_tx),
                                orig_idx,
                                e
                            )
                        })?;
                    if entry.status == 1 {
                        let (orig_tx, orig_idx) = keys::decode_outpoint(&deposit_outpoint_key);
                        let key = (orig_tx.clone(), orig_idx);
                        if seen_keys.insert(key) {
                            results.push(dao_cache_entry_to_row(orig_tx, orig_idx, entry));
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    pub fn process_dao_withdrawals(
        &self,
        consumed_dao_deposits: &[(Vec<u8>, i16, String, i64, i16)],
        new_dao_outputs: &[(Vec<u8>, i16, Vec<u8>, i64, u64)],
        candidate_withdraw_to_outputs: &[(i16, Vec<u8>)],
        tx_inputs: &[(Vec<u8>, i16)],
        block_number: i64,
        consuming_tx_hash: &[u8],
        _timestamp: DateTime<Utc>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut consumed_output_indices: HashSet<usize> = HashSet::new();
        for (original_tx_hash, original_output_index, capacity_str, deposit_block, status) in
            consumed_dao_deposits
        {
            let capacity: i64 = capacity_str.parse().map_err(|e| {
                anyhow!(
                    "invalid DAO capacity string: value='{}', error={}",
                    capacity_str,
                    e
                )
            })?;
            let outpoint_key = keys::encode_outpoint(original_tx_hash, *original_output_index);

            if *status == 0 {
                let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                else {
                    bail!(
                        "DAO deposit entry missing during withdrawal request: consuming_tx=0x{}, deposit_outpoint=0x{}:{}, capacity={}",
                        hex::encode(consuming_tx_hash),
                        hex::encode(original_tx_hash),
                        original_output_index,
                        capacity
                    );
                };
                let mut entry: DaoDepositCacheEntry =
                    bincode::deserialize(&value).map_err(|e| {
                        anyhow!(
                            "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                            hex::encode(original_tx_hash),
                            original_output_index,
                            e
                        )
                    })?;

                // Phase 1: deposit -> withdraw_request
                let (pos, (new_tx_hash, new_output_index, _, _, _)) =
                    select_phase1_output_for_deposit(
                        new_dao_outputs,
                        &consumed_output_indices,
                        capacity,
                        entry.deposit_block_number,
                    )
                    ?
                    .ok_or_else(|| {
                        anyhow!(
                            "DAO phase-1 output not found for consumed deposit: consuming_tx=0x{}, deposit_outpoint=0x{}:{}, capacity={}, deposit_block={}, lock_hash=0x{}",
                            hex::encode(consuming_tx_hash),
                            hex::encode(original_tx_hash),
                            original_output_index,
                            capacity,
                            entry.deposit_block_number,
                            hex::encode(&entry.lock_script_hash),
                        )
                    })?;
                consumed_output_indices.insert(pos);

                entry.status = 1;
                entry.withdraw_request_block = Some(block_number);
                entry.withdraw_request_tx = Some(new_tx_hash.clone());
                entry.withdraw_request_output_index = Some(*new_output_index);
                batch.put_dao_deposit(&outpoint_key, &entry);
                batch.put_dao_by_withdraw_tx(new_tx_hash, *new_output_index, &outpoint_key);
            } else if *status == 1 {
                // Phase 2: withdraw_request -> withdrawal complete
                let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                else {
                    bail!(
                        "DAO deposit entry missing during withdrawal completion: tx_hash=0x{}, output_index={}",
                        hex::encode(original_tx_hash),
                        original_output_index
                    );
                };
                let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                    anyhow!(
                        "failed to deserialize DAO deposit entry: tx_hash=0x{}, output_index={}, error={}",
                        hex::encode(original_tx_hash),
                        original_output_index,
                        e
                    )
                })?;
                let request_block = entry.withdraw_request_block.ok_or_else(|| {
                    anyhow!(
                        "withdraw request block missing for status=1 deposit: tx_hash=0x{}, output_index={}",
                        hex::encode(original_tx_hash),
                        original_output_index
                    )
                })?;

                let compensation =
                    self.calculate_dao_compensation(capacity, *deposit_block, request_block)?;

                let request_tx_hash = entry.withdraw_request_tx.as_ref().ok_or_else(|| {
                    anyhow!(
                        "withdraw request tx missing for status=1 deposit: tx_hash=0x{}, output_index={}",
                        hex::encode(original_tx_hash),
                        original_output_index
                    )
                })?;
                let request_output_index = if let Some(idx) = entry.withdraw_request_output_index {
                    idx
                } else {
                    infer_request_output_index_from_inputs(tx_inputs, request_tx_hash)
                            .ok_or_else(|| {
                                anyhow!(
                                    "withdraw request output index missing/ambiguous for status=1 deposit: tx_hash=0x{}, output_index={}, request_tx=0x{}",
                                    hex::encode(original_tx_hash),
                                    original_output_index,
                                    hex::encode(request_tx_hash)
                                )
                            })?
                };
                let withdraw_to_output_index = infer_withdraw_to_output_index_from_outputs(
                    candidate_withdraw_to_outputs,
                    &entry.lock_script_hash,
                );

                let mut entry = entry;
                entry.status = 2;
                entry.withdraw_block = Some(block_number);
                entry.withdraw_tx = Some(consuming_tx_hash.to_vec());
                entry.withdraw_request_output_index = Some(request_output_index);
                entry.withdraw_to_output_index = withdraw_to_output_index;
                entry.compensation = Some(compensation);
                batch.put_dao_deposit(&outpoint_key, &entry);
            }
        }
        Ok(())
    }

    fn calculate_dao_compensation(
        &self,
        capacity: i64,
        deposit_block: i64,
        withdraw_request_block: i64,
    ) -> Result<i64> {
        let deposit_dao = self.get_block_dao_field(deposit_block)?;
        let withdraw_dao = self.get_block_dao_field(withdraw_request_block)?;

        match (deposit_dao, withdraw_dao) {
            (Some(d), Some(w)) => {
                let ar_deposit = extract_ar_from_dao(&d).ok_or_else(|| {
                    anyhow!(
                        "failed to extract AR from DAO field: deposit_block={}, dao_len={}",
                        deposit_block,
                        d.len()
                    )
                })?;
                let ar_withdraw = extract_ar_from_dao(&w).ok_or_else(|| {
                    anyhow!(
                        "failed to extract AR from DAO field: request_block={}, dao_len={}",
                        withdraw_request_block,
                        w.len()
                    )
                })?;
                let compensation =
                    calculate_dao_compensation_from_ar(capacity, ar_deposit, ar_withdraw)?;
                Ok(compensation)
            }
            _ => bail!(
                "missing DAO field for compensation: capacity={}, deposit_block={}, request_block={}",
                capacity,
                deposit_block,
                withdraw_request_block
            ),
        }
    }

    pub fn insert_dao_deposits_batch(
        &self,
        deposits: &[(ParsedDaoDeposit, i64, DateTime<Utc>, i64)],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if deposits.is_empty() {
            return Ok(());
        }

        for (deposit, block_number, _timestamp, ar) in deposits {
            let entry = build_dao_cache_entry(deposit, *block_number, *ar);
            let output_index = i16::try_from(deposit.output_index).map_err(|_| {
                anyhow!(
                    "DAO deposit output_index exceeds i16 range while batching insert: tx_hash=0x{}, output_index={}",
                    hex::encode(&deposit.tx_hash),
                    deposit.output_index
                )
            })?;
            let outpoint_key = keys::encode_outpoint(&deposit.tx_hash, output_index);
            batch.put_dao_deposit(&outpoint_key, &entry);
        }

        Ok(())
    }

    pub fn find_consumed_dao_deposits_batch(
        &self,
        inputs: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result_map: HashMap<(Vec<u8>, i16), (Vec<u8>, i16, String, i64, i16)> =
            HashMap::new();

        // Direct deposit lookups
        for (tx_hash, output_index) in inputs {
            let outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
            if let Some(value) = self
                .store
                .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
            {
                let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                    anyhow!(
                        "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                        hex::encode(tx_hash),
                        output_index,
                        e
                    )
                })?;
                result_map.insert(
                    (tx_hash.to_vec(), *output_index),
                    dao_cache_entry_to_row(tx_hash.to_vec(), *output_index, entry),
                );
            }
        }

        // Withdraw request outpoint lookups (Phase 2 withdrawals).
        // Each input's (tx_hash, output_index) is the full outpoint of the
        // withdrawal request cell being consumed.
        for (tx_hash, output_index) in inputs {
            let withdraw_outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
            if let Some(deposit_outpoint_key) = self
                .store
                .get_cf(self.store.cf_dao_by_withdraw_tx(), &withdraw_outpoint_key)?
            {
                if let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &deposit_outpoint_key)?
                {
                    let entry: DaoDepositCacheEntry =
                        bincode::deserialize(&value).map_err(|e| {
                            let (orig_tx, orig_idx) = keys::decode_outpoint(&deposit_outpoint_key);
                            anyhow!(
                                "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                                hex::encode(orig_tx),
                                orig_idx,
                                e
                            )
                        })?;
                    if entry.status == 1 {
                        let (orig_tx, orig_idx) = keys::decode_outpoint(&deposit_outpoint_key);
                        let key = (tx_hash.to_vec(), *output_index);
                        result_map
                            .entry(key)
                            .or_insert_with(|| dao_cache_entry_to_row(orig_tx, orig_idx, entry));
                    }
                }
            }
        }

        Ok(result_map)
    }

    pub fn process_dao_withdrawals_batch<T>(
        &self,
        contexts: &[T],
        batch: &mut StoreBatch,
        pending_deposits: &HashMap<[u8; 34], DaoDepositCacheEntry>,
    ) -> Result<()>
    where
        T: DaoWithdrawalContextTrait,
    {
        if contexts.is_empty() {
            return Ok(());
        }

        // Pre-fetch DAO fields for all relevant blocks
        let mut all_blocks: HashSet<i64> = HashSet::new();
        for ctx in contexts {
            for (_, _, _, deposit_block, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    all_blocks.insert(*deposit_block);
                }
            }
        }

        // Also collect request blocks from the store (or pending deposits)
        for ctx in contexts {
            for (tx_hash, output_index, _, _, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    let outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
                    let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        Some(
                            bincode::deserialize::<DaoDepositCacheEntry>(&value).map_err(|e| {
                                anyhow!(
                                    "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                                    hex::encode(tx_hash),
                                    output_index,
                                    e
                                )
                            })?,
                        )
                    } else {
                        pending_deposits.get(&outpoint_key).cloned()
                    };
                    if let Some(entry) = maybe_entry {
                        if let Some(block) = entry.withdraw_request_block {
                            all_blocks.insert(block);
                        }
                    }
                }
            }
        }

        let dao_fields: HashMap<i64, Vec<u8>> = {
            let mut result = HashMap::new();
            let blocks: Vec<i64> = all_blocks.into_iter().collect();
            let cached = self.store.get_dao_fields_batch(&blocks)?;
            for (block_num, dao) in cached {
                result.insert(block_num, dao);
            }
            result
        };

        for ctx in contexts {
            let mut consumed_output_indices: HashSet<usize> = HashSet::new();
            for (original_tx_hash, original_output_index, capacity_str, deposit_block, status) in
                ctx.consumed_deposits()
            {
                let capacity: i64 = capacity_str.parse().map_err(|e| {
                    anyhow!(
                        "invalid DAO capacity string: value='{}', error={}",
                        capacity_str,
                        e
                    )
                })?;
                let outpoint_key = keys::encode_outpoint(original_tx_hash, *original_output_index);

                if *status == 0 {
                    let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        Some(
                            bincode::deserialize::<DaoDepositCacheEntry>(&value).map_err(|e| {
                                anyhow!(
                                    "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                                    hex::encode(original_tx_hash),
                                    original_output_index,
                                    e
                                )
                            })?,
                        )
                    } else {
                        // Fall back to pending (same-batch) deposits not yet committed
                        pending_deposits.get(&outpoint_key).cloned()
                    };
                    let Some(mut entry) = maybe_entry else {
                        bail!(
                            "DAO deposit entry missing during withdrawal request (batch): consuming_tx=0x{}, deposit_outpoint=0x{}:{}, capacity={}",
                            hex::encode(ctx.consuming_tx_hash()),
                            hex::encode(original_tx_hash),
                            original_output_index,
                            capacity
                        );
                    };
                    let (pos, (new_tx_hash, new_output_index, _, _, _)) =
                        select_phase1_output_for_deposit(
                            ctx.new_dao_outputs(),
                            &consumed_output_indices,
                            capacity,
                            entry.deposit_block_number,
                        )?
                        .ok_or_else(|| {
                            anyhow!(
                                "DAO phase-1 output not found for consumed deposit (batch): consuming_tx=0x{}, deposit_outpoint=0x{}:{}, capacity={}, deposit_block={}, lock_hash=0x{}",
                                hex::encode(ctx.consuming_tx_hash()),
                                hex::encode(original_tx_hash),
                                original_output_index,
                                capacity,
                                entry.deposit_block_number,
                                hex::encode(&entry.lock_script_hash),
                            )
                        })?;
                    consumed_output_indices.insert(pos);

                    entry.status = 1;
                    entry.withdraw_request_block = Some(ctx.block_number());
                    entry.withdraw_request_tx = Some(new_tx_hash.clone());
                    entry.withdraw_request_output_index = Some(*new_output_index);
                    batch.put_dao_deposit(&outpoint_key, &entry);
                    batch.put_dao_by_withdraw_tx(new_tx_hash, *new_output_index, &outpoint_key);
                } else if *status == 1 {
                    let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        Some(
                            bincode::deserialize::<DaoDepositCacheEntry>(&value).map_err(|e| {
                                anyhow!(
                                    "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                                    hex::encode(original_tx_hash),
                                    original_output_index,
                                    e
                                )
                            })?,
                        )
                    } else {
                        pending_deposits.get(&outpoint_key).cloned()
                    };

                    let request_block: i64 = maybe_entry
                        .as_ref()
                        .and_then(|e| e.withdraw_request_block)
                        .ok_or_else(|| {
                            anyhow!(
                                "withdraw_request_block missing for status=1 deposit: outpoint=0x{}:{}, block={}",
                                hex::encode(original_tx_hash),
                                original_output_index,
                                ctx.block_number()
                            )
                        })?;

                    if let Some(mut entry) = maybe_entry {
                        let request_tx_hash = entry.withdraw_request_tx.as_ref().ok_or_else(|| {
                            anyhow!(
                                "withdraw request tx missing for status=1 deposit: tx_hash=0x{}, output_index={}",
                                hex::encode(original_tx_hash),
                                original_output_index
                            )
                        })?;
                        let request_output_index = if let Some(idx) =
                            entry.withdraw_request_output_index
                        {
                            idx
                        } else {
                            ctx.infer_request_output_index(request_tx_hash).ok_or_else(|| {
                                    anyhow!(
                                        "withdraw request output index missing/ambiguous for status=1 deposit: tx_hash=0x{}, output_index={}, request_tx=0x{}",
                                        hex::encode(original_tx_hash),
                                        original_output_index,
                                        hex::encode(request_tx_hash)
                                    )
                                })?
                        };
                        let dep_dao = dao_fields.get(deposit_block).ok_or_else(|| {
                            anyhow!(
                                "missing DAO field for deposit block {} while completing DAO withdraw: block={}, consuming_tx=0x{}, deposit_outpoint=0x{}:{}",
                                deposit_block,
                                ctx.block_number(),
                                hex::encode(ctx.consuming_tx_hash()),
                                hex::encode(original_tx_hash),
                                original_output_index
                            )
                        })?;
                        let req_dao = dao_fields.get(&request_block).ok_or_else(|| {
                            anyhow!(
                                "missing DAO field for withdraw request block {} while completing DAO withdraw: block={}, consuming_tx=0x{}, deposit_outpoint=0x{}:{}, request_tx=0x{}",
                                request_block,
                                ctx.block_number(),
                                hex::encode(ctx.consuming_tx_hash()),
                                hex::encode(original_tx_hash),
                                original_output_index,
                                entry
                                    .withdraw_request_tx
                                    .as_ref()
                                    .map(hex::encode)
                                    .unwrap_or_else(|| "<missing>".to_string())
                            )
                        })?;
                        let ar_deposit = extract_ar_from_dao(dep_dao).ok_or_else(|| {
                            anyhow!(
                                "failed to extract AR from DAO field: deposit_block={}, dao_len={}",
                                deposit_block,
                                dep_dao.len()
                            )
                        })?;
                        let ar_withdraw = extract_ar_from_dao(req_dao).ok_or_else(|| {
                            anyhow!(
                                "failed to extract AR from DAO field: request_block={}, dao_len={}",
                                request_block,
                                req_dao.len()
                            )
                        })?;
                        let compensation =
                            calculate_dao_compensation_from_ar(capacity, ar_deposit, ar_withdraw)?;
                        let withdraw_to_output_index =
                            ctx.withdraw_to_output_index_for_lock(&entry.lock_script_hash);
                        entry.status = 2;
                        entry.withdraw_block = Some(ctx.block_number());
                        entry.withdraw_tx = Some(ctx.consuming_tx_hash().to_vec());
                        entry.withdraw_request_output_index = Some(request_output_index);
                        entry.withdraw_to_output_index = withdraw_to_output_index;
                        entry.compensation = Some(compensation);
                        batch.put_dao_deposit(&outpoint_key, &entry);
                    } else {
                        bail!(
                            "DAO deposit entry missing during withdrawal completion: tx_hash=0x{}, output_index={}",
                            hex::encode(original_tx_hash),
                            original_output_index
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::CachedBlockHeader;

    fn dedup_tx_hashes<'a>(tx_hashes: &[&'a [u8]]) -> Vec<&'a [u8]> {
        let mut seen = std::collections::HashSet::new();
        tx_hashes
            .iter()
            .filter(|h| seen.insert(**h))
            .copied()
            .collect()
    }

    #[derive(Clone)]
    struct BatchCtx {
        consumed: Vec<(Vec<u8>, i16, String, i64, i16)>,
        new_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
        block_num: i64,
        consuming_tx: Vec<u8>,
    }

    impl DaoWithdrawalContextTrait for BatchCtx {
        fn consumed_deposits(&self) -> &[(Vec<u8>, i16, String, i64, i16)] {
            &self.consumed
        }
        fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
            &self.new_outputs
        }
        fn block_number(&self) -> i64 {
            self.block_num
        }
        fn consuming_tx_hash(&self) -> &[u8] {
            &self.consuming_tx
        }
        fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
    }

    fn header_with_ar(ar: u64) -> CachedBlockHeader {
        let mut dao = vec![0u8; 32];
        dao[8..16].copy_from_slice(&ar.to_le_bytes());
        CachedBlockHeader {
            hash: vec![0x11; 32],
            timestamp: 1_704_067_200_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
        }
    }

    #[test]
    fn test_build_dao_cache_entry_sets_defaults() {
        let deposit = ParsedDaoDeposit {
            tx_hash: vec![0x11; 32],
            output_index: 7,
            lock_script_hash: vec![0x22; 32],
            capacity: 123_456,
        };
        let entry = build_dao_cache_entry(&deposit, 42, 9876);

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
            deposit_block_number: 77,
            lock_script_hash: vec![0x33; 32],
            deposit_ar: 123,
            status: 1,
            withdraw_request_tx: Some(vec![0x44; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(88),
            withdraw_request_ar: Some(456),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        let (tx_hash, output_index, capacity_str, deposit_block, status) =
            dao_cache_entry_to_row(vec![0xaa; 32], 3, entry);

        assert_eq!(tx_hash, vec![0xaa; 32]);
        assert_eq!(output_index, 3);
        assert_eq!(capacity_str, "999");
        assert_eq!(deposit_block, 77);
        assert_eq!(status, 1);
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
    fn test_infer_request_output_index_from_inputs_unique_match() {
        let tx_inputs = vec![
            (vec![0x10; 32], 0),
            (vec![0x20; 32], 4),
            (vec![0x30; 32], 1),
        ];
        let idx = infer_request_output_index_from_inputs(&tx_inputs, &[0x20; 32]);
        assert_eq!(idx, Some(4));
    }

    #[test]
    fn test_infer_request_output_index_from_inputs_ambiguous_returns_none() {
        let tx_inputs = vec![
            (vec![0x20; 32], 4),
            (vec![0x20; 32], 5),
            (vec![0x30; 32], 1),
        ];
        let idx = infer_request_output_index_from_inputs(&tx_inputs, &[0x20; 32]);
        assert!(idx.is_none());
    }

    #[test]
    fn test_calculate_dao_compensation_from_ar_errors_on_capacity_below_occupied() {
        let err = calculate_dao_compensation_from_ar(100_00000000, 100, 110).unwrap_err();
        assert!(err.to_string().contains("below occupied"));
    }

    #[test]
    fn test_calculate_dao_compensation_from_ar_errors_on_ar_underflow() {
        let err = calculate_dao_compensation_from_ar(200_00000000, 100, 90).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_process_dao_withdrawals_batch_errors_on_invalid_capacity_string() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx_hash = vec![0xAA; 32];
        let deposit_output_index: i16 = 0;
        let deposit_capacity: i64 = 500_00000000;
        let deposit_block: i64 = 100;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&deposit_tx_hash, deposit_output_index);
        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(
            outpoint_key,
            DaoDepositCacheEntry {
                capacity: deposit_capacity,
                deposit_block_number: deposit_block,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10000000000000000,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        let ctx = BatchCtx {
            consumed: vec![(
                deposit_tx_hash,
                deposit_output_index,
                "not-a-number".to_string(),
                deposit_block,
                0,
            )],
            new_outputs: vec![],
            block_num: 200,
            consuming_tx: vec![0xCC; 32],
        };

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap_err();
        assert!(err.to_string().contains("invalid DAO capacity string"));
    }

    #[test]
    fn test_process_dao_withdrawals_batch_errors_on_capacity_below_occupied() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(100, &header_with_ar(10));
        batch.put_block_header(110, &header_with_ar(11));
        batch.commit().unwrap();

        let deposit_tx_hash = vec![0xAA; 32];
        let deposit_output_index: i16 = 0;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&deposit_tx_hash, deposit_output_index);
        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(
            outpoint_key,
            DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: 100,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10,
                status: 1,
                withdraw_request_tx: Some(vec![0xDD; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(110),
                withdraw_request_ar: Some(11),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        let ctx = BatchCtx {
            consumed: vec![(
                deposit_tx_hash,
                deposit_output_index,
                "10000000000".to_string(),
                100,
                1,
            )],
            new_outputs: vec![],
            block_num: 120,
            consuming_tx: vec![0xEE; 32],
        };

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap_err();
        assert!(err.to_string().contains("below occupied"));
    }

    #[test]
    fn test_process_dao_withdrawals_errors_when_request_block_missing() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let original_tx_hash = vec![0xAA; 32];
        let original_output_index = 0i16;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&original_tx_hash, original_output_index);

        // status=1 but withdraw_request_block missing should be treated as inconsistent state.
        let entry = DaoDepositCacheEntry {
            capacity: 500_00000000,
            deposit_block_number: 100,
            lock_script_hash: vec![0xBB; 32],
            deposit_ar: 10,
            status: 1,
            withdraw_request_tx: Some(vec![0xCC; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: None,
            withdraw_request_ar: Some(11),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(&outpoint_key, &entry);
        batch.commit().unwrap();

        let consumed = vec![(
            original_tx_hash.clone(),
            original_output_index,
            "50000000000".to_string(),
            100,
            1,
        )];
        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals(
                &consumed,
                &[],
                &[],
                &[(vec![0xCC; 32], 0)],
                120,
                &[0xDD; 32],
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap_err();
        assert!(err.to_string().contains("withdraw request block missing"));
    }

    #[test]
    fn test_process_dao_withdrawals_errors_when_dao_field_missing_for_compensation() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let original_tx_hash = vec![0xAB; 32];
        let original_output_index = 1i16;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&original_tx_hash, original_output_index);

        let entry = DaoDepositCacheEntry {
            capacity: 500_00000000,
            deposit_block_number: 100,
            lock_script_hash: vec![0xBC; 32],
            deposit_ar: 10,
            status: 1,
            withdraw_request_tx: Some(vec![0xCD; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(110),
            withdraw_request_ar: Some(11),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(&outpoint_key, &entry);
        batch.commit().unwrap();

        let consumed = vec![(
            original_tx_hash.clone(),
            original_output_index,
            "50000000000".to_string(),
            100,
            1,
        )];
        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals(
                &consumed,
                &[],
                &[],
                &[(vec![0xCD; 32], 0)],
                120,
                &[0xDE; 32],
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("missing DAO field for compensation"));
    }

    #[test]
    fn test_process_dao_withdrawals_phase2_withdraw_to_uses_output_lock_analysis() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(100, &header_with_ar(10));
        batch.put_block_header(110, &header_with_ar(11));
        batch.commit().unwrap();

        let original_tx_hash = vec![0xAA; 32];
        let original_output_index = 1i16;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&original_tx_hash, original_output_index);
        let target_lock_hash = vec![0xBC; 32];
        let request_tx_hash = vec![0xCD; 32];
        let withdraw_tx_hash = vec![0xDE; 32];

        let entry = DaoDepositCacheEntry {
            capacity: 500_00000000,
            deposit_block_number: 100,
            lock_script_hash: target_lock_hash.clone(),
            deposit_ar: 10,
            status: 1,
            withdraw_request_tx: Some(request_tx_hash.clone()),
            withdraw_request_output_index: Some(4),
            withdraw_request_block: Some(110),
            withdraw_request_ar: Some(11),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(&outpoint_key, &entry);
        batch.commit().unwrap();

        let consumed = vec![(
            original_tx_hash.clone(),
            original_output_index,
            "50000000000".to_string(),
            100,
            1,
        )];
        let candidate_outputs = vec![
            (0, vec![0x11; 32]),
            (3, target_lock_hash),
            (5, vec![0x22; 32]),
        ];
        let tx_inputs = vec![(request_tx_hash, 4)];

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals(
                &consumed,
                &[],
                &candidate_outputs,
                &tx_inputs,
                120,
                &withdraw_tx_hash,
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stored = store
            .get_cf(store.cf_dao_deposits(), &outpoint_key)
            .unwrap()
            .unwrap();
        let updated: DaoDepositCacheEntry = bincode::deserialize(&stored).unwrap();

        assert_eq!(updated.status, 2);
        assert_eq!(updated.withdraw_tx, Some(withdraw_tx_hash));
        assert_eq!(updated.withdraw_to_output_index, Some(3));
    }

    /// Regression test: same-batch deposits must be updated to status=1
    /// when a Phase 1 withdrawal occurs within the same uncommitted batch.
    /// Previously, process_dao_withdrawals_batch only read from committed
    /// RocksDB, missing deposits that were written to the batch but not
    /// yet committed. This caused deposits to remain stuck at status=0.
    #[test]
    fn test_process_dao_withdrawals_batch_uses_pending_deposits() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        // A deposit that exists only in pending_deposits, NOT in the committed store
        let deposit_tx_hash = vec![0xAA; 32];
        let deposit_output_index: i16 = 0;
        let deposit_capacity: i64 = 500_00000000; // 500 CKB
        let deposit_block: i64 = 100;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&deposit_tx_hash, deposit_output_index);

        let pending_entry = DaoDepositCacheEntry {
            capacity: deposit_capacity,
            deposit_block_number: deposit_block,
            lock_script_hash: vec![0xBB; 32],
            deposit_ar: 10000000000000000,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };

        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(outpoint_key, pending_entry);

        // The Phase 1 withdrawal tx consumes the deposit and creates a new DAO cell
        let withdraw_tx_hash = vec![0xCC; 32];
        let withdraw_block: i64 = 200;

        #[derive(Clone)]
        struct TestCtx {
            consumed: Vec<(Vec<u8>, i16, String, i64, i16)>,
            new_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
            block_num: i64,
            consuming_tx: Vec<u8>,
        }
        impl DaoWithdrawalContextTrait for TestCtx {
            fn consumed_deposits(&self) -> &[(Vec<u8>, i16, String, i64, i16)] {
                &self.consumed
            }
            fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
                &self.new_outputs
            }
            fn block_number(&self) -> i64 {
                self.block_num
            }
            fn consuming_tx_hash(&self) -> &[u8] {
                &self.consuming_tx
            }
            fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
                chrono::Utc::now()
            }
        }

        let ctx = TestCtx {
            consumed: vec![(
                deposit_tx_hash.clone(),
                deposit_output_index,
                deposit_capacity.to_string(),
                deposit_block,
                0i16, // status = 0, Phase 1 withdrawal
            )],
            new_outputs: vec![(
                withdraw_tx_hash.clone(),
                0,
                vec![0xBB; 32],
                deposit_capacity,
                deposit_block as u64,
            )],
            block_num: withdraw_block,
            consuming_tx: withdraw_tx_hash.clone(),
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap();
        batch.commit().unwrap();

        // Verify the deposit was updated to status=1 in the store
        let stored = store
            .get_cf(store.cf_dao_deposits(), &outpoint_key)
            .unwrap()
            .expect("deposit should have been written");
        let entry: DaoDepositCacheEntry = bincode::deserialize(&stored).unwrap();
        assert_eq!(
            entry.status, 1,
            "deposit should be updated to status=1 (withdraw requested)"
        );
        assert_eq!(entry.withdraw_request_block, Some(withdraw_block));
        assert_eq!(entry.withdraw_request_tx, Some(withdraw_tx_hash.clone()));

        // Also verify the withdraw_tx outpoint -> deposit outpoint index was written
        let withdraw_outpoint_key = keys::encode_outpoint(&withdraw_tx_hash, 0);
        let reverse_lookup = store
            .get_cf(store.cf_dao_by_withdraw_tx(), &withdraw_outpoint_key)
            .unwrap();
        assert!(
            reverse_lookup.is_some(),
            "dao_by_withdraw_tx index should be written"
        );
    }

    #[test]
    fn test_process_dao_withdrawals_phase1_matches_same_capacity_outputs_deterministically() {
        // Two same-capacity outputs are legal; map deterministically by output index order.
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_a_tx = vec![0xA1; 32];
        let capacity: i64 = 500_00000000;

        let outpoint_a = ckbadger_store::keys::encode_outpoint(&deposit_a_tx, 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &outpoint_a,
            &DaoDepositCacheEntry {
                capacity,
                deposit_block_number: 100,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        let output_tx_1 = vec![0xC1; 32];
        let output_tx_2 = vec![0xC2; 32];
        let new_dao_outputs = vec![
            (output_tx_1.clone(), 0i16, vec![0xBB; 32], capacity, 100u64),
            (output_tx_2.clone(), 0i16, vec![0xBB; 32], capacity, 100u64),
        ];

        let consumed = vec![(
            deposit_a_tx.clone(),
            0i16,
            capacity.to_string(),
            100i64,
            0i16,
        )];

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals(
                &consumed,
                &new_dao_outputs,
                &[],
                &[],
                200,
                &[0xDD; 32],
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();
        let stored_a = store
            .get_cf(store.cf_dao_deposits(), &outpoint_a)
            .unwrap()
            .unwrap();
        let entry_a: DaoDepositCacheEntry = bincode::deserialize(&stored_a).unwrap();
        assert_eq!(entry_a.status, 1);
        assert_eq!(entry_a.withdraw_request_output_index, Some(0));
        assert_eq!(entry_a.withdraw_request_tx, Some(output_tx_1.clone()));

        let reverse_lookup = store
            .get_dao_deposit_by_withdraw_tx(&output_tx_1, 0)
            .unwrap();
        assert_eq!(reverse_lookup, Some(outpoint_a.to_vec()));
    }

    #[test]
    fn test_extract_ar_from_dao_errors_on_short_field() {
        let short_dao = vec![0u8; 8];
        assert!(extract_ar_from_dao(&short_dao).is_none());
    }

    #[test]
    fn test_process_dao_withdrawals_batch_errors_on_missing_request_block() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx_hash = vec![0xAA; 32];
        let deposit_output_index: i16 = 0;
        let outpoint_key =
            ckbadger_store::keys::encode_outpoint(&deposit_tx_hash, deposit_output_index);
        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(
            outpoint_key,
            DaoDepositCacheEntry {
                capacity: 500_00000000,
                deposit_block_number: 100,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10,
                status: 1,
                withdraw_request_tx: Some(vec![0xCC; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: None, // Missing!
                withdraw_request_ar: Some(11),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        let ctx = BatchCtx {
            consumed: vec![(
                deposit_tx_hash,
                deposit_output_index,
                "50000000000".to_string(),
                100,
                1,
            )],
            new_outputs: vec![],
            block_num: 200,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap_err();
        assert!(
            err.to_string().contains("withdraw_request_block missing"),
            "expected withdraw_request_block missing error, got: {}",
            err
        );
    }

    #[test]
    fn test_process_dao_withdrawals_batch_matches_same_capacity_outputs_deterministically() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx_hash = vec![0xA1; 32];
        let deposit_output_index: i16 = 0;
        let capacity: i64 = 500_00000000;
        let outpoint_key = keys::encode_outpoint(&deposit_tx_hash, deposit_output_index);

        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(
            outpoint_key,
            DaoDepositCacheEntry {
                capacity,
                deposit_block_number: 100,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        let ctx = BatchCtx {
            consumed: vec![(
                deposit_tx_hash,
                deposit_output_index,
                capacity.to_string(),
                100,
                0,
            )],
            new_outputs: vec![
                (vec![0xC1; 32], 0, vec![0xBB; 32], capacity, 100),
                (vec![0xC2; 32], 1, vec![0xBB; 32], capacity, 100),
            ],
            block_num: 200,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap();
        batch.commit().unwrap();

        let stored = store
            .get_cf(store.cf_dao_deposits(), &outpoint_key)
            .unwrap()
            .unwrap();
        let entry: DaoDepositCacheEntry = bincode::deserialize(&stored).unwrap();
        assert_eq!(entry.status, 1);
        assert_eq!(entry.withdraw_request_output_index, Some(0));

        let reverse_lookup = store
            .get_dao_deposit_by_withdraw_tx(&[0xC1; 32], 0)
            .unwrap();
        assert_eq!(reverse_lookup, Some(outpoint_key.to_vec()));
    }

    #[test]
    fn test_multi_deposit_same_withdraw_request_tx() {
        // Two deposits have their withdrawal requested in the SAME transaction.
        // Both should be tracked independently via dao_by_withdraw_tx keyed by
        // the full outpoint (tx_hash + output_index), not just tx_hash.
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_a_tx = vec![0xA1; 32];
        let deposit_b_tx = vec![0xA2; 32];
        let withdraw_request_tx = vec![0xBB; 32];
        let capacity_a: i64 = 200_00000000;
        let capacity_b: i64 = 300_00000000;
        let deposit_block: i64 = 100;

        // Seed two deposits
        let outpoint_a = keys::encode_outpoint(&deposit_a_tx, 0);
        let outpoint_b = keys::encode_outpoint(&deposit_b_tx, 0);
        let mut seed = StoreBatch::new(&store);
        seed.put_dao_deposit(
            &outpoint_a,
            &DaoDepositCacheEntry {
                capacity: capacity_a,
                deposit_block_number: deposit_block,
                lock_script_hash: vec![0xCC; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        seed.put_dao_deposit(
            &outpoint_b,
            &DaoDepositCacheEntry {
                capacity: capacity_b,
                deposit_block_number: deposit_block,
                lock_script_hash: vec![0xDD; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        seed.commit().unwrap();

        // Process withdrawal request: SAME tx_hash, different output indices (0 and 1)
        let new_dao_outputs = vec![
            (
                withdraw_request_tx.clone(),
                0i16,
                vec![0xCC; 32],
                capacity_a,
                deposit_block as u64,
            ),
            (
                withdraw_request_tx.clone(),
                1i16,
                vec![0xDD; 32],
                capacity_b,
                deposit_block as u64,
            ),
        ];
        let consumed = vec![
            (
                deposit_a_tx.clone(),
                0i16,
                capacity_a.to_string(),
                deposit_block,
                0i16,
            ),
            (
                deposit_b_tx.clone(),
                0i16,
                capacity_b.to_string(),
                deposit_block,
                0i16,
            ),
        ];

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals(
                &consumed,
                &new_dao_outputs,
                &[],
                &[],
                200,
                &[0x99; 32],
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        // Both deposits should now be status=1 with the same withdraw_request_tx
        let entry_a: DaoDepositCacheEntry = bincode::deserialize(
            &store
                .get_cf(store.cf_dao_deposits(), &outpoint_a)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let entry_b: DaoDepositCacheEntry = bincode::deserialize(
            &store
                .get_cf(store.cf_dao_deposits(), &outpoint_b)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(entry_a.status, 1);
        assert_eq!(entry_b.status, 1);
        assert_eq!(
            entry_a.withdraw_request_tx,
            Some(withdraw_request_tx.clone())
        );
        assert_eq!(
            entry_b.withdraw_request_tx,
            Some(withdraw_request_tx.clone())
        );
        assert_eq!(entry_a.withdraw_request_output_index, Some(0));
        assert_eq!(entry_b.withdraw_request_output_index, Some(1));

        // The dao_by_withdraw_tx CF should have TWO entries (one per output_index)
        let lookup_a = store
            .get_dao_deposit_by_withdraw_tx(&withdraw_request_tx, 0)
            .unwrap();
        let lookup_b = store
            .get_dao_deposit_by_withdraw_tx(&withdraw_request_tx, 1)
            .unwrap();
        assert_eq!(
            lookup_a.unwrap(),
            outpoint_a,
            "output_index=0 should map to deposit A"
        );
        assert_eq!(
            lookup_b.unwrap(),
            outpoint_b,
            "output_index=1 should map to deposit B"
        );

        // find_consumed_dao_deposits_batch should find BOTH deposits when
        // consuming both withdrawal request outputs
        let inputs: Vec<(&[u8], i16)> = vec![(&withdraw_request_tx, 0), (&withdraw_request_tx, 1)];
        let found = writer.find_consumed_dao_deposits_batch(&inputs).unwrap();
        assert_eq!(
            found.len(),
            2,
            "both deposits should be found via their distinct withdrawal request outpoints"
        );
    }

    #[test]
    fn test_find_consumed_dao_deposits_rejects_negative_output_index() {
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store);

        let tx_hash = [0xAB; 32];
        let err = writer
            .find_consumed_dao_deposits(&[(&tx_hash, -1)])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("negative DAO input output_index in find_consumed_dao_deposits"));
    }

    #[test]
    fn test_find_consumed_dao_deposits_rejects_output_index_over_i16() {
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store);

        let tx_hash = [0xCD; 32];
        let err = writer
            .find_consumed_dao_deposits(&[(&tx_hash, i16::MAX as i32 + 1)])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("DAO input output_index exceeds i16 range in find_consumed_dao_deposits"));
    }

    #[test]
    fn test_process_dao_withdrawals_phase1_uses_deposit_block_to_disambiguate_outputs() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx = vec![0xA1; 32];
        let capacity: i64 = 500_00000000;
        let outpoint = keys::encode_outpoint(&deposit_tx, 0);

        let mut seed = StoreBatch::new(&store);
        seed.put_dao_deposit(
            &outpoint,
            &DaoDepositCacheEntry {
                capacity,
                deposit_block_number: 100,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        seed.commit().unwrap();

        let withdraw_tx = vec![0xCC; 32];
        let new_dao_outputs = vec![
            // Same capacity/lock but wrong deposit block -> must be ignored.
            (withdraw_tx.clone(), 0i16, vec![0xBB; 32], capacity, 101u64),
            // Correct match.
            (withdraw_tx.clone(), 1i16, vec![0xBB; 32], capacity, 100u64),
        ];
        let consumed = vec![(deposit_tx.clone(), 0i16, capacity.to_string(), 100i64, 0i16)];

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals(
                &consumed,
                &new_dao_outputs,
                &[],
                &[],
                200,
                &[0xDD; 32],
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stored: DaoDepositCacheEntry = bincode::deserialize(
            &store
                .get_cf(store.cf_dao_deposits(), &outpoint)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.withdraw_request_output_index, Some(1));
        assert_eq!(stored.withdraw_request_tx, Some(withdraw_tx.clone()));
        assert!(store
            .get_dao_deposit_by_withdraw_tx(&withdraw_tx, 0)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_dao_deposit_by_withdraw_tx(&withdraw_tx, 1)
                .unwrap(),
            Some(outpoint.to_vec())
        );
    }

    #[test]
    fn test_process_dao_withdrawals_batch_phase1_uses_deposit_block_to_disambiguate_outputs() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx = vec![0xA5; 32];
        let deposit_output_index: i16 = 0;
        let capacity: i64 = 500_00000000;
        let outpoint = keys::encode_outpoint(&deposit_tx, deposit_output_index);

        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(
            outpoint,
            DaoDepositCacheEntry {
                capacity,
                deposit_block_number: 100,
                lock_script_hash: vec![0xBB; 32],
                deposit_ar: 10,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        let withdraw_tx = vec![0xC5; 32];
        let ctx = BatchCtx {
            consumed: vec![(
                deposit_tx,
                deposit_output_index,
                capacity.to_string(),
                100,
                0,
            )],
            new_outputs: vec![
                (withdraw_tx.clone(), 0, vec![0xBB; 32], capacity, 101),
                (withdraw_tx.clone(), 1, vec![0xBB; 32], capacity, 100),
            ],
            block_num: 200,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap();
        batch.commit().unwrap();

        let stored: DaoDepositCacheEntry = bincode::deserialize(
            &store
                .get_cf(store.cf_dao_deposits(), &outpoint)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.withdraw_request_output_index, Some(1));
        assert_eq!(stored.withdraw_request_tx, Some(withdraw_tx.clone()));
        assert!(store
            .get_dao_deposit_by_withdraw_tx(&withdraw_tx, 0)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_dao_deposit_by_withdraw_tx(&withdraw_tx, 1)
                .unwrap(),
            Some(outpoint.to_vec())
        );
    }

    #[test]
    fn test_process_dao_withdrawals_phase1_allows_output_lock_change() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx = vec![0xA7; 32];
        let capacity: i64 = 120_00000000;
        let outpoint = keys::encode_outpoint(&deposit_tx, 0);

        let mut seed = StoreBatch::new(&store);
        seed.put_dao_deposit(
            &outpoint,
            &DaoDepositCacheEntry {
                capacity,
                deposit_block_number: 5668752,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 10,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        seed.commit().unwrap();

        let withdraw_tx = vec![0xC7; 32];
        let consumed = vec![(deposit_tx, 0i16, capacity.to_string(), 5668752i64, 0i16)];
        let new_dao_outputs = vec![(
            withdraw_tx.clone(),
            0i16,
            vec![0x22; 32],
            capacity,
            5668752u64,
        )];

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals(
                &consumed,
                &new_dao_outputs,
                &[],
                &[],
                5733774,
                &[0xDD; 32],
                chrono::Utc::now(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stored: DaoDepositCacheEntry = bincode::deserialize(
            &store
                .get_cf(store.cf_dao_deposits(), &outpoint)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.status, 1);
        assert_eq!(stored.withdraw_request_tx, Some(withdraw_tx.clone()));
        assert_eq!(stored.withdraw_request_output_index, Some(0));
        assert_eq!(
            store
                .get_dao_deposit_by_withdraw_tx(&withdraw_tx, 0)
                .unwrap(),
            Some(outpoint.to_vec())
        );
    }

    #[test]
    fn test_process_dao_withdrawals_batch_phase1_allows_output_lock_change() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());

        let deposit_tx = vec![0xA8; 32];
        let deposit_output_index: i16 = 0;
        let capacity: i64 = 120_00000000;
        let outpoint = keys::encode_outpoint(&deposit_tx, deposit_output_index);

        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(
            outpoint,
            DaoDepositCacheEntry {
                capacity,
                deposit_block_number: 5668752,
                lock_script_hash: vec![0x33; 32],
                deposit_ar: 10,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        let withdraw_tx = vec![0xC8; 32];
        let ctx = BatchCtx {
            consumed: vec![(
                deposit_tx,
                deposit_output_index,
                capacity.to_string(),
                5668752,
                0,
            )],
            new_outputs: vec![(withdraw_tx.clone(), 0, vec![0x44; 32], capacity, 5668752)],
            block_num: 5733774,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &pending_deposits)
            .unwrap();
        batch.commit().unwrap();

        let stored: DaoDepositCacheEntry = bincode::deserialize(
            &store
                .get_cf(store.cf_dao_deposits(), &outpoint)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.status, 1);
        assert_eq!(stored.withdraw_request_tx, Some(withdraw_tx.clone()));
        assert_eq!(stored.withdraw_request_output_index, Some(0));
        assert_eq!(
            store
                .get_dao_deposit_by_withdraw_tx(&withdraw_tx, 0)
                .unwrap(),
            Some(outpoint.to_vec())
        );
    }

    #[test]
    fn test_insert_dao_deposits_batch_errors_on_output_index_overflow() {
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());
        let mut batch = StoreBatch::new(&store);

        let deposits = vec![(
            ParsedDaoDeposit {
                tx_hash: vec![0xAB; 32],
                output_index: i32::from(i16::MAX) + 1,
                lock_script_hash: vec![0xCD; 32],
                capacity: 123,
            },
            42_i64,
            chrono::Utc::now(),
            1_i64,
        )];

        let err = writer
            .insert_dao_deposits_batch(&deposits, &mut batch)
            .unwrap_err();
        assert!(err.to_string().contains("output_index exceeds i16 range"));
    }
}
