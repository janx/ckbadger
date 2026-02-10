use anyhow::Result;
use chrono::{DateTime, Utc};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::DaoDepositCacheEntry;
use std::collections::{HashMap, HashSet};

use crate::parser::{ParsedDaoDeposit, ParsedDaoWithdrawRequest};

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
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        compensation: None,
    }
}

fn dao_cache_entry_to_row(
    tx_hash: Vec<u8>,
    output_index: i16,
    entry: DaoDepositCacheEntry,
) -> (i64, Vec<u8>, i16, String, i64, i16) {
    (
        0,
        tx_hash,
        output_index,
        entry.capacity.to_string(),
        entry.deposit_block_number,
        entry.status,
    )
}

fn dedup_tx_hashes<'a>(tx_hashes: &[&'a [u8]]) -> Vec<&'a [u8]> {
    let mut seen = std::collections::HashSet::new();
    tx_hashes
        .iter()
        .filter(|h| seen.insert(**h))
        .copied()
        .collect()
}

pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)];
    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)];
    fn block_number(&self) -> i64;
    fn consuming_tx_hash(&self) -> &[u8];
    fn timestamp(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Default)]
pub struct SecondaryIssuanceBreakdown {
    pub secondary_issuance: i64,
    pub miner_secondary: i64,
    pub dao_compensation: i64,
    pub burnt: i64,
}

fn extract_ar_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
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

    pub fn insert_dao_deposit(
        &self,
        deposit: &ParsedDaoDeposit,
        block_number: i64,
        _timestamp: DateTime<Utc>,
        deposit_ar: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let entry = build_dao_cache_entry(deposit, block_number, deposit_ar);
        let outpoint_key = keys::encode_outpoint(&deposit.tx_hash, deposit.output_index as i16);
        batch.put_dao_deposit(&outpoint_key, &entry);
        Ok(())
    }

    pub fn update_dao_withdraw_request(
        &self,
        request: &ParsedDaoWithdrawRequest,
        block_number: i64,
        _timestamp: DateTime<Utc>,
        withdraw_ar: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let outpoint_key = keys::encode_outpoint(
            &request.original_tx_hash,
            request.original_output_index as i16,
        );
        if let Some(value) = self
            .store
            .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
        {
            if let Ok(mut entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                entry.status = 1;
                entry.withdraw_request_block = Some(block_number);
                entry.withdraw_request_tx = Some(request.tx_hash.clone());
                entry.withdraw_request_ar = Some(withdraw_ar);
                batch.put_dao_deposit(&outpoint_key, &entry);
                // Update the withdraw_tx -> outpoint index
                batch.put_dao_by_withdraw_tx(&request.tx_hash, &outpoint_key);
            }
        }
        Ok(())
    }

    pub fn complete_dao_withdrawal(
        &self,
        withdraw_request_tx_hash: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        _timestamp: DateTime<Utc>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        // Get deposits linked via withdraw_request_tx
        if let Some(outpoint_key) = self
            .store
            .get_cf(self.store.cf_dao_by_withdraw_tx(), withdraw_request_tx_hash)?
        {
            if let Some(value) = self
                .store
                .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
            {
                if let Ok(mut entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                    let request_block = entry.withdraw_request_block.unwrap_or(block_number);
                    let compensation = self
                        .calculate_dao_compensation(
                            entry.capacity,
                            entry.deposit_block_number,
                            request_block,
                        )?
                        .unwrap_or(0);

                    entry.status = 2;
                    entry.withdraw_block = Some(block_number);
                    entry.withdraw_tx = Some(tx_hash.to_vec());
                    entry.compensation = Some(compensation);
                    batch.put_dao_deposit(&outpoint_key, &entry);
                }
            }
        }
        Ok(())
    }

    pub fn find_consumed_dao_deposits(
        &self,
        inputs: &[(&[u8], i32)],
    ) -> Result<Vec<(i64, Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::new();
        let mut seen_keys: HashSet<(Vec<u8>, i16)> = HashSet::new();

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| *h).collect();

        // Check direct deposits (tx_hash, output_index)
        for (tx_hash, output_index) in inputs {
            let outpoint_key = keys::encode_outpoint(tx_hash, *output_index as i16);
            if let Some(value) = self
                .store
                .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
            {
                if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                    let key = (tx_hash.to_vec(), *output_index as i16);
                    seen_keys.insert(key);
                    results.push(dao_cache_entry_to_row(
                        tx_hash.to_vec(),
                        *output_index as i16,
                        entry,
                    ));
                }
            }
        }

        // Check by withdraw_request_tx (Phase 2 withdrawals)
        let unique_tx_hashes = dedup_tx_hashes(&tx_hashes);
        for tx_hash in unique_tx_hashes {
            if let Some(outpoint_key) = self
                .store
                .get_cf(self.store.cf_dao_by_withdraw_tx(), tx_hash)?
            {
                if let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                {
                    if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                        if entry.status == 1 {
                            let (orig_tx, orig_idx) = keys::decode_outpoint(&outpoint_key);
                            let key = (orig_tx.clone(), orig_idx);
                            if seen_keys.insert(key) {
                                results.push(dao_cache_entry_to_row(orig_tx, orig_idx, entry));
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    pub fn process_dao_withdrawals(
        &self,
        consumed_dao_deposits: &[(i64, Vec<u8>, i16, String, i64, i16)],
        new_dao_outputs: &[(Vec<u8>, i16, Vec<u8>, i64, u64)],
        block_number: i64,
        consuming_tx_hash: &[u8],
        _timestamp: DateTime<Utc>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for (
            _deposit_id,
            original_tx_hash,
            original_output_index,
            capacity_str,
            deposit_block,
            status,
        ) in consumed_dao_deposits
        {
            let capacity: i64 = capacity_str.parse().unwrap_or(0);
            let outpoint_key = keys::encode_outpoint(original_tx_hash, *original_output_index);

            if *status == 0 {
                // Phase 1: deposit -> withdraw_request
                let matching_output = new_dao_outputs
                    .iter()
                    .find(|(_, _, _, cap, _)| *cap == capacity);

                if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                    if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        if let Ok(mut entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value)
                        {
                            entry.status = 1;
                            entry.withdraw_request_block = Some(block_number);
                            entry.withdraw_request_tx = Some(new_tx_hash.clone());
                            batch.put_dao_deposit(&outpoint_key, &entry);
                            batch.put_dao_by_withdraw_tx(new_tx_hash, &outpoint_key);
                        }
                    }
                }
            } else if *status == 1 {
                // Phase 2: withdraw_request -> withdrawal complete
                let request_block = if let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                {
                    bincode::deserialize::<DaoDepositCacheEntry>(&value)
                        .ok()
                        .and_then(|e| e.withdraw_request_block)
                        .unwrap_or(block_number)
                } else {
                    block_number
                };

                let compensation =
                    self.calculate_dao_compensation(capacity, *deposit_block, request_block)?;

                if let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                {
                    if let Ok(mut entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                        entry.status = 2;
                        entry.withdraw_block = Some(block_number);
                        entry.withdraw_tx = Some(consuming_tx_hash.to_vec());
                        entry.compensation = Some(compensation.unwrap_or(0));
                        batch.put_dao_deposit(&outpoint_key, &entry);
                    }
                }
            }
        }
        Ok(())
    }

    fn calculate_dao_compensation(
        &self,
        capacity: i64,
        deposit_block: i64,
        withdraw_request_block: i64,
    ) -> Result<Option<i64>> {
        let deposit_dao = self.get_block_dao_field(deposit_block)?;
        let withdraw_dao = self.get_block_dao_field(withdraw_request_block)?;

        match (deposit_dao, withdraw_dao) {
            (Some(d), Some(w)) => {
                let ar_deposit = extract_ar_from_dao(&d).unwrap_or(1);
                let ar_withdraw = extract_ar_from_dao(&w).unwrap_or(1);

                if ar_deposit == 0 {
                    return Ok(Some(0));
                }

                let capacity_u128 = capacity as u128;
                let free_capacity = capacity_u128.saturating_sub(DAO_OCCUPIED_CAPACITY as u128);
                let compensation = (free_capacity * ar_withdraw as u128 / ar_deposit as u128)
                    .saturating_sub(free_capacity);

                Ok(Some(compensation as i64))
            }
            _ => Ok(None),
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
            let outpoint_key = keys::encode_outpoint(&deposit.tx_hash, deposit.output_index as i16);
            batch.put_dao_deposit(&outpoint_key, &entry);
        }

        Ok(())
    }

    pub fn find_consumed_dao_deposits_batch(
        &self,
        inputs: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result_map: HashMap<(Vec<u8>, i16), (i64, Vec<u8>, i16, String, i64, i16)> =
            HashMap::new();

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| *h).collect();

        // Direct deposit lookups
        for (tx_hash, output_index) in inputs {
            let outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
            if let Some(value) = self
                .store
                .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
            {
                if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                    result_map.insert(
                        (tx_hash.to_vec(), *output_index),
                        dao_cache_entry_to_row(tx_hash.to_vec(), *output_index, entry),
                    );
                }
            }
        }

        // Withdraw request TX lookups
        let unique_tx_hashes = dedup_tx_hashes(&tx_hashes);
        for tx_hash in unique_tx_hashes {
            if let Some(outpoint_key) = self
                .store
                .get_cf(self.store.cf_dao_by_withdraw_tx(), tx_hash)?
            {
                if let Some(value) = self
                    .store
                    .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                {
                    if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                        if entry.status == 1 {
                            let (orig_tx, orig_idx) = keys::decode_outpoint(&outpoint_key);
                            let key = (tx_hash.to_vec(), 0i16);
                            result_map.entry(key).or_insert_with(|| {
                                dao_cache_entry_to_row(orig_tx, orig_idx, entry)
                            });
                        }
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
            for (_, _, _, _, deposit_block, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    all_blocks.insert(*deposit_block);
                }
            }
        }

        // Also collect request blocks from the store
        for ctx in contexts {
            for (_, tx_hash, output_index, _, _, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    let outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
                    if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                            if let Some(block) = entry.withdraw_request_block {
                                all_blocks.insert(block);
                            }
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
            for (
                _deposit_id,
                original_tx_hash,
                original_output_index,
                capacity_str,
                deposit_block,
                status,
            ) in ctx.consumed_deposits()
            {
                let capacity: i64 = capacity_str.parse().unwrap_or(0);
                let outpoint_key = keys::encode_outpoint(original_tx_hash, *original_output_index);

                if *status == 0 {
                    let matching_output = ctx
                        .new_dao_outputs()
                        .iter()
                        .find(|(_, _, _, cap, _)| *cap == capacity);

                    if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                        if let Some(value) = self
                            .store
                            .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                        {
                            if let Ok(mut entry) =
                                bincode::deserialize::<DaoDepositCacheEntry>(&value)
                            {
                                entry.status = 1;
                                entry.withdraw_request_block = Some(ctx.block_number());
                                entry.withdraw_request_tx = Some(new_tx_hash.clone());
                                batch.put_dao_deposit(&outpoint_key, &entry);
                                batch.put_dao_by_withdraw_tx(new_tx_hash, &outpoint_key);
                            }
                        }
                    }
                } else if *status == 1 {
                    let request_block: i64 = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        bincode::deserialize::<DaoDepositCacheEntry>(&value)
                            .ok()
                            .and_then(|e| e.withdraw_request_block)
                            .unwrap_or(ctx.block_number())
                    } else {
                        ctx.block_number()
                    };

                    let compensation = if let (Some(dep_dao), Some(req_dao)) = (
                        dao_fields.get(deposit_block),
                        dao_fields.get(&request_block),
                    ) {
                        let ar_deposit = extract_ar_from_dao(dep_dao).unwrap_or(1);
                        let ar_withdraw = extract_ar_from_dao(req_dao).unwrap_or(1);
                        if ar_deposit > 0 {
                            let cap_u128 = capacity as u128;
                            let free = cap_u128.saturating_sub(DAO_OCCUPIED_CAPACITY as u128);
                            Some(
                                ((free * ar_withdraw as u128 / ar_deposit as u128)
                                    .saturating_sub(free)) as i64,
                            )
                        } else {
                            Some(0)
                        }
                    } else {
                        None
                    };

                    if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        if let Ok(mut entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value)
                        {
                            entry.status = 2;
                            entry.withdraw_block = Some(ctx.block_number());
                            entry.withdraw_tx = Some(ctx.consuming_tx_hash().to_vec());
                            entry.compensation = Some(compensation.unwrap_or(0));
                            batch.put_dao_deposit(&outpoint_key, &entry);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn accumulate_secondary_issuance(
        &self,
        breakdown: &SecondaryIssuanceBreakdown,
        block_number: i64,
        _block_timestamp: DateTime<Utc>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let issuance = ckbadger_store::types::SecondaryIssuance {
            miner_reward: breakdown.miner_secondary,
            dao_reward: breakdown.dao_compensation,
            treasury: breakdown.burnt,
        };
        batch.put_block_issuance(block_number, &issuance);
        Ok(())
    }

    pub fn recalculate_dao_extended_statistics(&self, _current_block: i64) -> Result<()> {
        // DAO extended statistics recalculation is deferred to task-runner
        // since it requires complex aggregation across all deposits
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(entry.withdraw_request_block.is_none());
        assert!(entry.withdraw_request_ar.is_none());
        assert!(entry.withdraw_block.is_none());
        assert!(entry.withdraw_tx.is_none());
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
            withdraw_request_block: Some(88),
            withdraw_request_ar: Some(456),
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };
        let (id, tx_hash, output_index, capacity_str, deposit_block, status) =
            dao_cache_entry_to_row(vec![0xaa; 32], 3, entry);

        assert_eq!(id, 0);
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
}
