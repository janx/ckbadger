use anyhow::{bail, Result};
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{AddressBalance, ScriptDailyDelta};

use super::BatchWriter;

fn checked_next_script_metric_i64(
    code_hash: &[u8],
    script_kind: &str,
    metric: &str,
    current: i64,
    delta: i64,
) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script {} {} overflow: code_hash=0x{}, current={}, delta={}",
            script_kind,
            metric,
            hex::encode(code_hash),
            current,
            delta
        )
    })?;
    if next < 0 {
        bail!(
            "script {} {} underflow: code_hash=0x{}, current={}, delta={}, next={}",
            script_kind,
            metric,
            hex::encode(code_hash),
            current,
            delta,
            next
        );
    }
    Ok(next)
}

fn checked_next_script_metric_i128(
    code_hash: &[u8],
    script_kind: &str,
    metric: &str,
    current: i128,
    delta: i128,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script {} {} overflow: code_hash=0x{}, current={}, delta={}",
            script_kind,
            metric,
            hex::encode(code_hash),
            current,
            delta
        )
    })?;
    if next < 0 {
        bail!(
            "script {} {} underflow: code_hash=0x{}, current={}, delta={}, next={}",
            script_kind,
            metric,
            hex::encode(code_hash),
            current,
            delta,
            next
        );
    }
    Ok(next)
}

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
                Ok(Some(value)) => Some(bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize address balance: lock_hash=0x{}, error={}",
                        hex::encode(lock_hash),
                        e
                    )
                })?),
                Ok(None) => None,
                Err(e) => {
                    bail!(
                        "failed to read address balance: lock_hash=0x{}, error={}",
                        hex::encode(lock_hash),
                        e
                    );
                }
            };
            map.insert((*lock_hash).clone(), existing);
        }

        Ok(map)
    }

    pub fn apply_address_balance_deltas(
        &self,
        existing: &HashMap<Vec<u8>, Option<AddressBalance>>,
        changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        for (
            lock_hash,
            (balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash, occupied_delta),
        ) in changes
        {
            let prev = existing.get(lock_hash).and_then(|o| o.as_ref());

            let updated = match prev {
                Some(bal) => {
                    let mut bal = bal.clone();
                    let next_balance = bal.balance + *balance_delta;
                    if next_balance < 0 {
                        bail!(
                            "address balance underflow: lock_hash=0x{}, balance={}, delta={}",
                            hex::encode(lock_hash),
                            bal.balance,
                            balance_delta
                        );
                    }
                    let next_occupied = bal.occupied_capacity + *occupied_delta;
                    if next_occupied < 0 {
                        bail!(
                            "address occupied capacity underflow: lock_hash=0x{}, occupied_capacity={}, delta={}",
                            hex::encode(lock_hash),
                            bal.occupied_capacity,
                            occupied_delta
                        );
                    }
                    let next_live_cells = bal.live_cells_count + *live_delta;
                    if next_live_cells < 0 {
                        bail!(
                            "address live_cells_count underflow: lock_hash=0x{}, live_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.live_cells_count,
                            live_delta
                        );
                    }
                    let next_total_cells = bal.total_cells_count + *total_delta as i64;
                    if next_total_cells < 0 {
                        bail!(
                            "address total_cells_count underflow: lock_hash=0x{}, total_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.total_cells_count,
                            total_delta
                        );
                    }
                    let next_txs_count = bal.txs_count + tx_delta;
                    if next_txs_count < 0 {
                        bail!(
                            "address txs_count underflow: lock_hash=0x{}, txs_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.txs_count,
                            tx_delta
                        );
                    }
                    bal.balance = next_balance;
                    bal.occupied_capacity = next_occupied;
                    bal.live_cells_count = next_live_cells;
                    bal.total_cells_count = next_total_cells;
                    bal.txs_count = next_txs_count;
                    bal.last_activity_block = *block_num;
                    bal.last_activity_tx = tx_hash.to_vec();
                    bal
                }
                None => {
                    if *balance_delta < 0
                        || *occupied_delta < 0
                        || *live_delta < 0
                        || *total_delta < 0
                        || *tx_delta < 0
                    {
                        bail!(
                            "address delta underflow for unseen address: lock_hash=0x{}, balance_delta={}, occupied_delta={}, live_delta={}, total_delta={}, tx_delta={}",
                            hex::encode(lock_hash),
                            balance_delta,
                            occupied_delta,
                            live_delta,
                            total_delta,
                            tx_delta
                        );
                    }
                    AddressBalance {
                        balance: *balance_delta,
                        occupied_capacity: *occupied_delta,
                        live_cells_count: *live_delta,
                        total_cells_count: *total_delta as i64,
                        txs_count: *tx_delta,
                        first_seen_block: *block_num,
                        first_seen_tx: tx_hash.to_vec(),
                        last_activity_block: *block_num,
                        last_activity_tx: tx_hash.to_vec(),
                    }
                }
            };

            batch.put_addr_balance(lock_hash, &updated);
        }

        Ok(())
    }

    pub fn update_address_balances_batch(
        &self,
        changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)>,
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
                Ok(Some(value)) => Some(bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script info: code_hash=0x{}, error={}",
                        hex::encode(code_hash),
                        e
                    )
                })?),
                Ok(None) => None,
                Err(e) => {
                    bail!(
                        "failed to read script info: code_hash=0x{}, error={}",
                        hex::encode(code_hash),
                        e
                    );
                }
            };
            map.insert((*code_hash).clone(), existing);
        }

        Ok(map)
    }

    pub fn apply_script_usage_deltas(
        &self,
        existing: &HashMap<Vec<u8>, Option<ckbadger_store::types::ScriptInfo>>,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut updated_map: HashMap<&Vec<u8>, ckbadger_store::types::ScriptInfo> =
            HashMap::with_capacity(existing.len());

        for (
            (code_hash, is_type),
            (
                cells_delta,
                live_delta,
                cap_delta,
                live_cap_delta,
                occupied_delta,
                live_occupied_delta,
            ),
        ) in changes
        {
            let existing_info = existing.get(code_hash).and_then(|o| o.clone());
            if existing_info.is_none()
                && (*cells_delta < 0
                    || *live_delta < 0
                    || *cap_delta < 0
                    || *live_cap_delta < 0
                    || *occupied_delta < 0
                    || *live_occupied_delta < 0)
            {
                bail!(
                    "script delta underflow for unseen code_hash: code_hash=0x{}, is_type={}, cells_delta={}, live_delta={}, capacity_delta={}, live_capacity_delta={}, occupied_delta={}, live_occupied_delta={}",
                    hex::encode(code_hash),
                    is_type,
                    cells_delta,
                    live_delta,
                    cap_delta,
                    live_cap_delta,
                    occupied_delta,
                    live_occupied_delta
                );
            }

            let info = updated_map.entry(code_hash).or_insert_with(|| {
                existing_info.unwrap_or_else(|| ckbadger_store::types::ScriptInfo {
                    code_hash: code_hash.clone(),
                    ..Default::default()
                })
            });

            if *is_type {
                let next_type_cells_count = checked_next_script_metric_i64(
                    code_hash,
                    "type",
                    "cells_count",
                    info.type_cells_count,
                    *cells_delta,
                )?;
                let next_type_live_cells_count = checked_next_script_metric_i64(
                    code_hash,
                    "type",
                    "live_cells_count",
                    info.type_live_cells_count,
                    *live_delta,
                )?;
                let next_type_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "capacity_sum",
                    info.type_capacity_sum,
                    *cap_delta,
                )?;
                let next_type_live_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "live_capacity_sum",
                    info.type_live_capacity_sum,
                    *live_cap_delta,
                )?;
                let next_type_occupied_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "occupied_capacity_sum",
                    info.type_occupied_capacity_sum,
                    *occupied_delta,
                )?;
                let next_type_live_occupied_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "live_occupied_capacity_sum",
                    info.type_live_occupied_capacity_sum,
                    *live_occupied_delta,
                )?;

                if next_type_occupied_capacity_sum > next_type_capacity_sum {
                    bail!(
                        "script type occupied capacity exceeds total: code_hash=0x{}, occupied_capacity_sum={}, capacity_sum={}",
                        hex::encode(code_hash),
                        next_type_occupied_capacity_sum,
                        next_type_capacity_sum
                    );
                }
                if next_type_live_occupied_capacity_sum > next_type_live_capacity_sum {
                    bail!(
                        "script type live occupied capacity exceeds total: code_hash=0x{}, live_occupied_capacity_sum={}, live_capacity_sum={}",
                        hex::encode(code_hash),
                        next_type_live_occupied_capacity_sum,
                        next_type_live_capacity_sum
                    );
                }

                info.type_cells_count = next_type_cells_count;
                info.type_live_cells_count = next_type_live_cells_count;
                info.type_capacity_sum = next_type_capacity_sum;
                info.type_live_capacity_sum = next_type_live_capacity_sum;
                info.type_occupied_capacity_sum = next_type_occupied_capacity_sum;
                info.type_live_occupied_capacity_sum = next_type_live_occupied_capacity_sum;
            } else {
                let next_lock_cells_count = checked_next_script_metric_i64(
                    code_hash,
                    "lock",
                    "cells_count",
                    info.lock_cells_count,
                    *cells_delta,
                )?;
                let next_lock_live_cells_count = checked_next_script_metric_i64(
                    code_hash,
                    "lock",
                    "live_cells_count",
                    info.lock_live_cells_count,
                    *live_delta,
                )?;
                let next_lock_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "capacity_sum",
                    info.lock_capacity_sum,
                    *cap_delta,
                )?;
                let next_lock_live_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "live_capacity_sum",
                    info.lock_live_capacity_sum,
                    *live_cap_delta,
                )?;
                let next_lock_occupied_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "occupied_capacity_sum",
                    info.lock_occupied_capacity_sum,
                    *occupied_delta,
                )?;
                let next_lock_live_occupied_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "live_occupied_capacity_sum",
                    info.lock_live_occupied_capacity_sum,
                    *live_occupied_delta,
                )?;

                if next_lock_occupied_capacity_sum > next_lock_capacity_sum {
                    bail!(
                        "script lock occupied capacity exceeds total: code_hash=0x{}, occupied_capacity_sum={}, capacity_sum={}",
                        hex::encode(code_hash),
                        next_lock_occupied_capacity_sum,
                        next_lock_capacity_sum
                    );
                }
                if next_lock_live_occupied_capacity_sum > next_lock_live_capacity_sum {
                    bail!(
                        "script lock live occupied capacity exceeds total: code_hash=0x{}, live_occupied_capacity_sum={}, live_capacity_sum={}",
                        hex::encode(code_hash),
                        next_lock_live_occupied_capacity_sum,
                        next_lock_live_capacity_sum
                    );
                }

                info.lock_cells_count = next_lock_cells_count;
                info.lock_live_cells_count = next_lock_live_cells_count;
                info.lock_capacity_sum = next_lock_capacity_sum;
                info.lock_live_capacity_sum = next_lock_live_capacity_sum;
                info.lock_occupied_capacity_sum = next_lock_occupied_capacity_sum;
                info.lock_live_occupied_capacity_sum = next_lock_live_occupied_capacity_sum;
            }
        }

        for (code_hash, info) in &updated_map {
            batch.put_script_info(code_hash, info);
        }

        Ok(())
    }

    pub fn update_script_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>,
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

    pub fn update_script_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut keyed_changes: Vec<(Vec<u8>, i128, i128)> = Vec::with_capacity(changes.len());
        for ((code_hash, is_type, date_yyyymmdd), (live_cap_delta, live_occupied_delta)) in changes
        {
            if *live_cap_delta == 0 && *live_occupied_delta == 0 {
                continue;
            }
            keyed_changes.push((
                keys::encode_script_daily_key(code_hash, *is_type, *date_yyyymmdd).to_vec(),
                *live_cap_delta,
                *live_occupied_delta,
            ));
        }

        if keyed_changes.is_empty() {
            return Ok(());
        }

        let cf_keys: Vec<_> = keyed_changes
            .iter()
            .map(|(key, _, _)| (self.store.cf_stats(), key.as_slice()))
            .collect();
        let existing_results = self.store.multi_get_cf(cf_keys);

        for ((key, live_cap_delta, live_occupied_delta), existing_res) in
            keyed_changes.into_iter().zip(existing_results.into_iter())
        {
            let mut existing: ScriptDailyDelta = match existing_res {
                Ok(Some(value)) => bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script daily delta: key=0x{}, error={}",
                        hex::encode(&key),
                        e
                    )
                })?,
                Ok(None) => ScriptDailyDelta::default(),
                Err(e) => {
                    bail!(
                        "failed to read script daily delta: key=0x{}, error={}",
                        hex::encode(&key),
                        e
                    );
                }
            };
            existing.live_capacity_delta = existing
                .live_capacity_delta
                .checked_add(live_cap_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "script daily capacity delta overflow: key=0x{}, current={}, delta={}",
                        hex::encode(&key),
                        existing.live_capacity_delta,
                        live_cap_delta
                    )
                })?;
            existing.live_occupied_capacity_delta = existing
                .live_occupied_capacity_delta
                .checked_add(live_occupied_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "script daily occupied delta overflow: key=0x{}, current={}, delta={}",
                        hex::encode(&key),
                        existing.live_occupied_capacity_delta,
                        live_occupied_delta
                    )
                })?;
            if existing.live_capacity_delta == 0 && existing.live_occupied_capacity_delta == 0 {
                batch.delete_stats(&key);
            } else {
                let value = bincode::serialize(&existing)?;
                batch.put_stats(&key, &value);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::{CkbadgerStore, ScriptInfo};

    #[test]
    fn test_update_script_daily_deltas_batch_accumulates_and_deletes_zero_net() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        let code_hash = vec![0xBB; 32];
        let date = 20240115u32;

        let mut first = HashMap::new();
        first.insert((code_hash.clone(), false, date), (100i128, 60i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch(&first, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut second = HashMap::new();
        second.insert((code_hash.clone(), false, date), (-20i128, -10i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch(&second, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_script_daily_delta(&code_hash, false, date)
            .unwrap()
            .unwrap();
        assert_eq!(delta.live_capacity_delta, 80);
        assert_eq!(delta.live_occupied_capacity_delta, 50);

        let mut third = HashMap::new();
        third.insert((code_hash.clone(), false, date), (-80i128, -50i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch(&third, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_script_daily_delta(&code_hash, false, date)
            .unwrap();
        assert!(delta.is_none());
    }

    #[test]
    fn test_apply_script_usage_deltas_rejects_live_capacity_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let code_hash = vec![0xAA; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: 100,
            lock_live_capacity_sum: 100,
            lock_occupied_capacity_sum: 60,
            lock_live_occupied_capacity_sum: 60,
            ..Default::default()
        };

        let mut existing = HashMap::new();
        existing.insert(code_hash.clone(), Some(existing_info));

        let mut changes = HashMap::new();
        changes.insert((code_hash.clone(), false), (0, -1, 0, -200, 0, -120));

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .apply_script_usage_deltas(&existing, &changes, &mut batch)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("script lock live_capacity_sum underflow"));
    }

    #[test]
    fn test_apply_script_usage_deltas_rejects_live_occupied_over_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let code_hash = vec![0xBB; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            type_cells_count: 1,
            type_live_cells_count: 1,
            type_capacity_sum: 200,
            type_live_capacity_sum: 200,
            type_occupied_capacity_sum: 100,
            type_live_occupied_capacity_sum: 100,
            ..Default::default()
        };

        let mut existing = HashMap::new();
        existing.insert(code_hash.clone(), Some(existing_info));

        let mut changes = HashMap::new();
        changes.insert((code_hash.clone(), true), (0, 0, 0, 10, 0, 200));

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .apply_script_usage_deltas(&existing, &changes, &mut batch)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("script type live occupied capacity exceeds total"));
    }

    #[test]
    fn test_apply_script_usage_deltas_allows_capacity_sum_above_i64_max() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let code_hash = vec![0xCC; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: i64::MAX as i128,
            lock_live_capacity_sum: i64::MAX as i128,
            lock_occupied_capacity_sum: i64::MAX as i128 - 100,
            lock_live_occupied_capacity_sum: i64::MAX as i128 - 100,
            ..Default::default()
        };

        let mut existing = HashMap::new();
        existing.insert(code_hash.clone(), Some(existing_info));

        let mut changes = HashMap::new();
        changes.insert((code_hash.clone(), false), (0, 0, 500, 500, 400, 400));

        let mut batch = StoreBatch::new(&store);
        writer
            .apply_script_usage_deltas(&existing, &changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let updated = store.get_script_info(&code_hash).unwrap().unwrap();
        assert_eq!(updated.lock_capacity_sum, i64::MAX as i128 + 500);
        assert_eq!(updated.lock_live_capacity_sum, i64::MAX as i128 + 500);
        assert_eq!(updated.lock_occupied_capacity_sum, i64::MAX as i128 + 300);
        assert_eq!(
            updated.lock_live_occupied_capacity_sum,
            i64::MAX as i128 + 300
        );
    }

    #[test]
    fn test_apply_script_usage_deltas_accepts_capacity_delta_exceeding_i64() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let code_hash = vec![0xDD; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: 0,
            lock_live_capacity_sum: 0,
            lock_occupied_capacity_sum: 0,
            lock_live_occupied_capacity_sum: 0,
            ..Default::default()
        };

        let mut existing = HashMap::new();
        existing.insert(code_hash.clone(), Some(existing_info));

        let huge_delta = i128::from(i64::MAX) + 42;
        let mut changes = HashMap::new();
        changes.insert(
            (code_hash.clone(), false),
            (0, 0, huge_delta, huge_delta, 0, 0),
        );

        let mut batch = StoreBatch::new(&store);
        writer
            .apply_script_usage_deltas(&existing, &changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let updated = store.get_script_info(&code_hash).unwrap().unwrap();
        assert_eq!(updated.lock_capacity_sum, huge_delta);
        assert_eq!(updated.lock_live_capacity_sum, huge_delta);
    }

    #[test]
    fn test_read_script_info_errors_on_deserialize_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        let code_hash = vec![0xEE; 32];

        store
            .put_cf(store.cf_script_info(), &code_hash, &[0xFF, 0x00])
            .unwrap();

        let refs = vec![&code_hash];
        let err = writer.read_script_info(&refs).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize script info"));
    }

    #[test]
    fn test_read_address_balances_errors_on_deserialize_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        let lock_hash = vec![0xEF; 32];

        store
            .put_cf(store.cf_addr_balance(), &lock_hash, &[0xFF, 0x00])
            .unwrap();

        let refs = vec![&lock_hash];
        let err = writer.read_address_balances(&refs).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize address balance"));
    }

    #[test]
    fn test_apply_script_usage_deltas_rejects_negative_delta_for_unseen_script() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let code_hash = vec![0xAB; 32];
        let existing = HashMap::new();
        let mut changes = HashMap::new();
        changes.insert((code_hash.clone(), false), (0, -1, 0, -1, 0, -1));

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .apply_script_usage_deltas(&existing, &changes, &mut batch)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("script delta underflow for unseen code_hash"));
    }
}
