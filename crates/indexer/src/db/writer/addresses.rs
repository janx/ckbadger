use anyhow::Result;
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::AddressBalance;

use super::BatchWriter;

impl BatchWriter {
    pub fn read_address_balances(
        &self,
        lock_hashes: &[&Vec<u8>],
    ) -> Result<HashMap<Vec<u8>, Option<AddressBalance>>> {
        if lock_hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let cf_keys: Vec<_> = lock_hashes
            .iter()
            .map(|k| (self.store.cf_addr_balance(), k.as_slice()))
            .collect();
        let results = self.store.multi_get_cf(cf_keys);

        let mut map = HashMap::with_capacity(lock_hashes.len());
        for (res, lock_hash) in results.into_iter().zip(lock_hashes.iter()) {
            let existing: Option<AddressBalance> = match res {
                Ok(Some(value)) => bincode::deserialize(&value).ok(),
                _ => None,
            };
            map.insert((*lock_hash).clone(), existing);
        }

        Ok(map)
    }

    pub fn apply_address_balance_deltas(
        &self,
        existing: &HashMap<Vec<u8>, Option<AddressBalance>>,
        changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        for (lock_hash, (balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash)) in
            changes
        {
            let prev = existing.get(lock_hash).and_then(|o| o.as_ref());

            let updated = match prev {
                Some(bal) => {
                    let mut bal = bal.clone();
                    bal.balance += *balance_delta as i128;
                    bal.live_cells_count = (bal.live_cells_count + *live_delta).max(0);
                    bal.total_cells_count += *total_delta as i64;
                    bal.txs_count += tx_delta;
                    bal.last_activity_block = *block_num;
                    bal.last_activity_tx = tx_hash.to_vec();
                    bal
                }
                None => AddressBalance {
                    balance: *balance_delta as i128,
                    live_cells_count: (*live_delta).max(0),
                    total_cells_count: (*total_delta).max(0) as i64,
                    txs_count: *tx_delta,
                    first_seen_block: *block_num,
                    first_seen_tx: tx_hash.to_vec(),
                    last_activity_block: *block_num,
                    last_activity_tx: tx_hash.to_vec(),
                },
            };

            batch.put_addr_balance(lock_hash, &updated);
        }

        Ok(())
    }

    pub fn update_address_balances_batch(
        &self,
        changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let keys_vec: Vec<&Vec<u8>> = changes.keys().collect();
        let existing = self.read_address_balances(&keys_vec)?;
        self.apply_address_balance_deltas(&existing, changes, batch)
    }

    pub fn read_script_info(
        &self,
        code_hashes: &[&Vec<u8>],
    ) -> Result<HashMap<Vec<u8>, Option<ckbadger_store::types::ScriptInfo>>> {
        if code_hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let cf_keys: Vec<_> = code_hashes
            .iter()
            .map(|k| (self.store.cf_script_info(), k.as_slice()))
            .collect();
        let results = self.store.multi_get_cf(cf_keys);

        let mut map = HashMap::with_capacity(code_hashes.len());
        for (res, code_hash) in results.into_iter().zip(code_hashes.iter()) {
            let existing: Option<ckbadger_store::types::ScriptInfo> = match res {
                Ok(Some(value)) => bincode::deserialize(&value).ok(),
                _ => None,
            };
            map.insert((*code_hash).clone(), existing);
        }

        Ok(map)
    }

    pub fn apply_script_usage_deltas(
        &self,
        existing: &HashMap<Vec<u8>, Option<ckbadger_store::types::ScriptInfo>>,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut updated_map: HashMap<&Vec<u8>, ckbadger_store::types::ScriptInfo> =
            HashMap::with_capacity(existing.len());

        for ((code_hash, is_type), (cells_delta, live_delta, cap_delta, live_cap_delta)) in changes
        {
            let info = updated_map.entry(code_hash).or_insert_with(|| {
                existing
                    .get(code_hash)
                    .and_then(|o| o.clone())
                    .unwrap_or_else(|| ckbadger_store::types::ScriptInfo {
                        code_hash: code_hash.clone(),
                        ..Default::default()
                    })
            });

            if *is_type {
                info.type_cells_count += cells_delta;
                info.type_live_cells_count += live_delta;
                info.type_capacity_sum += cap_delta;
                info.type_live_capacity_sum += live_cap_delta;
            } else {
                info.lock_cells_count += cells_delta;
                info.lock_live_cells_count += live_delta;
                info.lock_capacity_sum += cap_delta;
                info.lock_live_capacity_sum += live_cap_delta;
            }
        }

        for (code_hash, info) in &updated_map {
            batch.put_script_info(code_hash, info);
        }

        Ok(())
    }

    pub fn update_script_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let unique_code_hashes: Vec<Vec<u8>> = {
            let mut seen = std::collections::HashSet::new();
            changes
                .keys()
                .filter_map(|(code_hash, _)| {
                    if seen.insert(code_hash.clone()) {
                        Some(code_hash.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        let refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();
        let existing = self.read_script_info(&refs)?;
        self.apply_script_usage_deltas(&existing, changes, batch)
    }
}
