use anyhow::{anyhow, bail, Result};
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

fn calculate_dao_compensation_from_ar(
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
                    let compensation = self.calculate_dao_compensation(
                        entry.capacity,
                        entry.deposit_block_number,
                        request_block,
                    )?;

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
            let capacity: i64 = capacity_str.parse().map_err(|e| {
                anyhow!(
                    "invalid DAO capacity string: value='{}', error={}",
                    capacity_str,
                    e
                )
            })?;
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

                let mut entry = entry;
                entry.status = 2;
                entry.withdraw_block = Some(block_number);
                entry.withdraw_tx = Some(consuming_tx_hash.to_vec());
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
                let ar_deposit = extract_ar_from_dao(&d).unwrap_or(1);
                let ar_withdraw = extract_ar_from_dao(&w).unwrap_or(1);
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
            for (_, _, _, _, deposit_block, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    all_blocks.insert(*deposit_block);
                }
            }
        }

        // Also collect request blocks from the store (or pending deposits)
        for ctx in contexts {
            for (_, tx_hash, output_index, _, _, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    let outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
                    let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        bincode::deserialize::<DaoDepositCacheEntry>(&value).ok()
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
            for (
                _deposit_id,
                original_tx_hash,
                original_output_index,
                capacity_str,
                deposit_block,
                status,
            ) in ctx.consumed_deposits()
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
                    let matching_output = ctx
                        .new_dao_outputs()
                        .iter()
                        .find(|(_, _, _, cap, _)| *cap == capacity);

                    if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                        let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                            .store
                            .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                        {
                            bincode::deserialize::<DaoDepositCacheEntry>(&value).ok()
                        } else {
                            // Fall back to pending (same-batch) deposits not yet committed
                            pending_deposits.get(&outpoint_key).cloned()
                        };

                        if let Some(mut entry) = maybe_entry {
                            entry.status = 1;
                            entry.withdraw_request_block = Some(ctx.block_number());
                            entry.withdraw_request_tx = Some(new_tx_hash.clone());
                            batch.put_dao_deposit(&outpoint_key, &entry);
                            batch.put_dao_by_withdraw_tx(new_tx_hash, &outpoint_key);
                        }
                    }
                } else if *status == 1 {
                    let maybe_entry: Option<DaoDepositCacheEntry> = if let Some(value) = self
                        .store
                        .get_cf(self.store.cf_dao_deposits(), &outpoint_key)?
                    {
                        bincode::deserialize::<DaoDepositCacheEntry>(&value).ok()
                    } else {
                        pending_deposits.get(&outpoint_key).cloned()
                    };

                    let request_block: i64 = maybe_entry
                        .as_ref()
                        .and_then(|e| e.withdraw_request_block)
                        .unwrap_or(ctx.block_number());

                    if let Some(mut entry) = maybe_entry {
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
                        let ar_deposit = extract_ar_from_dao(dep_dao).unwrap_or(1);
                        let ar_withdraw = extract_ar_from_dao(req_dao).unwrap_or(1);
                        let compensation =
                            calculate_dao_compensation_from_ar(capacity, ar_deposit, ar_withdraw)?;
                        entry.status = 2;
                        entry.withdraw_block = Some(ctx.block_number());
                        entry.withdraw_tx = Some(ctx.consuming_tx_hash().to_vec());
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
        // No-op placeholder: full DAO extended statistics recalculation is not implemented
        // in incremental writer path yet.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::CachedBlockHeader;

    #[derive(Clone)]
    struct BatchCtx {
        consumed: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
        new_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
        block_num: i64,
        consuming_tx: Vec<u8>,
    }

    impl DaoWithdrawalContextTrait for BatchCtx {
        fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)] {
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone());

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
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                compensation: None,
            },
        );

        let ctx = BatchCtx {
            consumed: vec![(
                0,
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone());

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
                withdraw_request_block: Some(110),
                withdraw_request_ar: Some(11),
                withdraw_block: None,
                withdraw_tx: None,
                compensation: None,
            },
        );

        let ctx = BatchCtx {
            consumed: vec![(
                0,
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone());

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
            withdraw_request_block: None,
            withdraw_request_ar: Some(11),
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(&outpoint_key, &entry);
        batch.commit().unwrap();

        let consumed = vec![(
            0,
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone());

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
            withdraw_request_block: Some(110),
            withdraw_request_ar: Some(11),
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(&outpoint_key, &entry);
        batch.commit().unwrap();

        let consumed = vec![(
            0,
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = super::super::BatchWriter::new(store.clone());

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
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };

        let mut pending_deposits: HashMap<[u8; 34], DaoDepositCacheEntry> = HashMap::new();
        pending_deposits.insert(outpoint_key, pending_entry);

        // The Phase 1 withdrawal tx consumes the deposit and creates a new DAO cell
        let withdraw_tx_hash = vec![0xCC; 32];
        let withdraw_block: i64 = 200;

        #[derive(Clone)]
        struct TestCtx {
            consumed: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
            new_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
            block_num: i64,
            consuming_tx: Vec<u8>,
        }
        impl DaoWithdrawalContextTrait for TestCtx {
            fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)] {
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
                0,
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

        // Also verify the withdraw_tx -> outpoint index was written
        let reverse_lookup = store
            .get_cf(store.cf_dao_by_withdraw_tx(), &withdraw_tx_hash)
            .unwrap();
        assert!(
            reverse_lookup.is_some(),
            "dao_by_withdraw_tx index should be written"
        );
    }
}
