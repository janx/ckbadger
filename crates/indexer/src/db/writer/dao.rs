use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::DaoDepositCacheEntry;
use std::collections::{HashMap, HashSet};

pub(crate) use ckbadger_common::dao::calculate_dao_compensation_from_ar;

use crate::parser::ParsedDaoDeposit;
use crate::sync::dao_helpers::DaoConsumedRow;

use super::BatchWriter;

fn build_dao_cache_entry(
    deposit: &ParsedDaoDeposit,
    block_number: i64,
    deposit_ar: i64,
    deposit_timestamp: i64,
) -> DaoDepositCacheEntry {
    DaoDepositCacheEntry {
        capacity: deposit.capacity,
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

pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[DaoConsumedRow];
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
    pub consumed_deposits: Vec<DaoConsumedRow>,
    pub new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
    pub tx_inputs: Vec<(Vec<u8>, i16)>,
    pub candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)>,
    pub block_number: i64,
    pub consuming_tx_hash: Vec<u8>,
    pub timestamp: DateTime<Utc>,
}

impl DaoWithdrawalContextTrait for DaoWithdrawalContext {
    fn consumed_deposits(&self) -> &[DaoConsumedRow] {
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

    pub fn insert_dao_deposits_batch(
        &self,
        deposits: &[(ParsedDaoDeposit, i64, DateTime<Utc>, i64)],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if deposits.is_empty() {
            return Ok(());
        }

        for (deposit, block_number, timestamp, ar) in deposits {
            let entry =
                build_dao_cache_entry(deposit, *block_number, *ar, timestamp.timestamp_millis());
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
    ) -> Result<HashMap<(Vec<u8>, i16), DaoConsumedRow>> {
        if inputs.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result_map: HashMap<(Vec<u8>, i16), DaoConsumedRow> = HashMap::new();

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
        pending_deposits: &mut HashMap<[u8; 34], DaoDepositCacheEntry>,
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
            for row in ctx.consumed_deposits() {
                if row.status == 0 {
                    // Phase-1 withdraw request: need AR at the request block
                    all_blocks.insert(ctx.block_number());
                } else if row.status == 1 {
                    all_blocks.insert(row.deposit_block);
                }
            }
        }

        // Also collect request blocks from the store (or pending deposits)
        for ctx in contexts {
            for row in ctx.consumed_deposits() {
                if row.status == 1 {
                    let outpoint_key = keys::encode_outpoint(&row.tx_hash, row.output_index);
                    let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        Some(
                            bincode::deserialize::<DaoDepositCacheEntry>(&value).map_err(|e| {
                                anyhow!(
                                    "failed to deserialize DAO deposit: outpoint=0x{}:{}, error={}",
                                    hex::encode(&row.tx_hash),
                                    row.output_index,
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
            for row in ctx.consumed_deposits() {
                let original_tx_hash = &row.tx_hash;
                let original_output_index = row.output_index;
                let capacity_str = &row.capacity_str;
                let deposit_block = row.deposit_block;
                let status = row.status;
                let capacity: i64 = capacity_str.parse().map_err(|e| {
                    anyhow!(
                        "invalid DAO capacity string: value='{}', error={}",
                        capacity_str,
                        e
                    )
                })?;
                let outpoint_key = keys::encode_outpoint(original_tx_hash, original_output_index);

                if status == 0 {
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
                    entry.withdraw_request_ar = dao_fields
                        .get(&ctx.block_number())
                        .and_then(|dao| extract_ar_from_dao(dao))
                        .map(i64::try_from)
                        .transpose()
                        .map_err(|_| {
                            anyhow!(
                                "DAO withdraw request AR exceeds i64 range: block={}, consuming_tx=0x{}, deposit_outpoint=0x{}:{}",
                                ctx.block_number(),
                                hex::encode(ctx.consuming_tx_hash()),
                                hex::encode(original_tx_hash),
                                original_output_index
                            )
                        })?;
                    batch.put_dao_deposit(&outpoint_key, &entry);
                    batch.put_dao_by_withdraw_tx(new_tx_hash, *new_output_index, &outpoint_key);
                    // Propagate phase1 update to pending map so a hypothetical
                    // same-batch phase2 lookup sees status=1 with request fields.
                    pending_deposits.insert(outpoint_key, entry);
                } else if status == 1 {
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
                        let dep_dao = dao_fields.get(&deposit_block).ok_or_else(|| {
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
        consumed: Vec<DaoConsumedRow>,
        new_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
        block_num: i64,
        consuming_tx: Vec<u8>,
    }

    impl DaoWithdrawalContextTrait for BatchCtx {
        fn consumed_deposits(&self) -> &[DaoConsumedRow] {
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
            parent_hash: vec![0u8; 32],
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
    fn test_calculate_dao_compensation_from_ar_errors_on_zero_deposit_ar() {
        let err = calculate_dao_compensation_from_ar(200_00000000, 0, 100).unwrap_err();
        assert!(err.to_string().contains("zero deposit AR"));
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
                deposit_timestamp: 0,
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx_hash,
                output_index: deposit_output_index,
                capacity_str: "not-a-number".to_string(),
                deposit_block,
                status: 0,
                lock_script_hash: vec![0xBB; 32],
            }],
            new_outputs: vec![],
            block_num: 200,
            consuming_tx: vec![0xCC; 32],
        };

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
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
                deposit_timestamp: 0,
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx_hash,
                output_index: deposit_output_index,
                capacity_str: "10000000000".to_string(),
                deposit_block: 100,
                status: 1,
                lock_script_hash: vec![0xBB; 32],
            }],
            new_outputs: vec![],
            block_num: 120,
            consuming_tx: vec![0xEE; 32],
        };

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
            .unwrap_err();
        assert!(err.to_string().contains("below occupied"));
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
            deposit_timestamp: 0,
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
            consumed: Vec<DaoConsumedRow>,
            new_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
            block_num: i64,
            consuming_tx: Vec<u8>,
        }
        impl DaoWithdrawalContextTrait for TestCtx {
            fn consumed_deposits(&self) -> &[DaoConsumedRow] {
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx_hash.clone(),
                output_index: deposit_output_index,
                capacity_str: deposit_capacity.to_string(),
                deposit_block,
                status: 0, // Phase 1 withdrawal
                lock_script_hash: vec![0xBB; 32],
            }],
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
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
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
                deposit_timestamp: 0,
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx_hash,
                output_index: deposit_output_index,
                capacity_str: "50000000000".to_string(),
                deposit_block: 100,
                status: 1,
                lock_script_hash: vec![0xBB; 32],
            }],
            new_outputs: vec![],
            block_num: 200,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
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
                deposit_timestamp: 0,
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx_hash,
                output_index: deposit_output_index,
                capacity_str: capacity.to_string(),
                deposit_block: 100,
                status: 0,
                lock_script_hash: vec![0xBB; 32],
            }],
            new_outputs: vec![
                (vec![0xC1; 32], 0, vec![0xBB; 32], capacity, 100),
                (vec![0xC2; 32], 1, vec![0xBB; 32], capacity, 100),
            ],
            block_num: 200,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
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
                deposit_timestamp: 0,
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx,
                output_index: deposit_output_index,
                capacity_str: capacity.to_string(),
                deposit_block: 100,
                status: 0,
                lock_script_hash: vec![0xBB; 32],
            }],
            new_outputs: vec![
                (withdraw_tx.clone(), 0, vec![0xBB; 32], capacity, 101),
                (withdraw_tx.clone(), 1, vec![0xBB; 32], capacity, 100),
            ],
            block_num: 200,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
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
                deposit_timestamp: 0,
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
            consumed: vec![DaoConsumedRow {
                tx_hash: deposit_tx,
                output_index: deposit_output_index,
                capacity_str: capacity.to_string(),
                deposit_block: 5668752,
                status: 0,
                lock_script_hash: vec![0x33; 32],
            }],
            new_outputs: vec![(withdraw_tx.clone(), 0, vec![0x44; 32], capacity, 5668752)],
            block_num: 5733774,
            consuming_tx: vec![0xDD; 32],
        };

        let mut batch = StoreBatch::new(&store);
        writer
            .process_dao_withdrawals_batch(&[ctx], &mut batch, &mut pending_deposits)
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
