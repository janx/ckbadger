use anyhow::{bail, Result};
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{AddressBalance, ScriptDailyDelta, ScriptReferenceInfo};

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

fn checked_next_script_reference_metric_i64(
    reference_hash: &[u8],
    hash_type: u8,
    metric: &str,
    current: i64,
    delta: i64,
) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script reference {} overflow: reference_hash=0x{}, hash_type={}, current={}, delta={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta
        )
    })?;
    if next < 0 {
        bail!(
            "script reference {} underflow: reference_hash=0x{}, hash_type={}, current={}, delta={}, next={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta,
            next
        );
    }
    Ok(next)
}

fn checked_next_script_reference_metric_i128(
    reference_hash: &[u8],
    hash_type: u8,
    metric: &str,
    current: i128,
    delta: i128,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script reference {} overflow: reference_hash=0x{}, hash_type={}, current={}, delta={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta
        )
    })?;
    if next < 0 {
        bail!(
            "script reference {} underflow: reference_hash=0x{}, hash_type={}, current={}, delta={}, next={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta,
            next
        );
    }
    Ok(next)
}

fn overlay_script_metadata(
    mut base: ckbadger_store::types::ScriptInfo,
    fresh: &ckbadger_store::types::ScriptInfo,
) -> ckbadger_store::types::ScriptInfo {
    base.hash_type = fresh.hash_type;
    base.name = fresh.name.clone();
    base.deprecated = fresh.deprecated;
    base.category = fresh.category.clone();
    base.website = fresh.website.clone();
    base.description = fresh.description.clone();
    // Note: dep_type_hash, dep_data_hash, code_cell_tx_hash, code_cell_output_index
    // are no longer overlaid here. Label import no longer writes these correctness
    // fields; code cell resolution uses script_references/script_versions CFs instead.
    base
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
        changes: &HashMap<Vec<u8>, crate::sync::types::AddressBalanceDelta>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        for (lock_hash, delta) in changes {
            let prev = existing.get(lock_hash).and_then(|o| o.as_ref());

            let updated = match prev {
                Some(bal) => {
                    let mut bal = bal.clone();
                    let next_balance = bal.balance + delta.balance_delta;
                    if next_balance < 0 {
                        bail!(
                            "address balance underflow: lock_hash=0x{}, balance={}, delta={}",
                            hex::encode(lock_hash),
                            bal.balance,
                            delta.balance_delta
                        );
                    }
                    let next_used = bal.used_capacity + delta.used_delta;
                    if next_used < 0 {
                        bail!(
                            "address used capacity underflow: lock_hash=0x{}, used_capacity={}, delta={}",
                            hex::encode(lock_hash),
                            bal.used_capacity,
                            delta.used_delta
                        );
                    }
                    let next_live_cells = bal.live_cells_count + delta.live_delta;
                    if next_live_cells < 0 {
                        bail!(
                            "address live_cells_count underflow: lock_hash=0x{}, live_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.live_cells_count,
                            delta.live_delta
                        );
                    }
                    let next_total_cells = bal.total_cells_count + delta.total_delta as i64;
                    if next_total_cells < 0 {
                        bail!(
                            "address total_cells_count underflow: lock_hash=0x{}, total_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.total_cells_count,
                            delta.total_delta
                        );
                    }
                    let next_txs_count = bal.txs_count + delta.tx_delta;
                    if next_txs_count < 0 {
                        bail!(
                            "address txs_count underflow: lock_hash=0x{}, txs_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.txs_count,
                            delta.tx_delta
                        );
                    }
                    bal.balance = next_balance;
                    bal.used_capacity = next_used;
                    bal.live_cells_count = next_live_cells;
                    bal.total_cells_count = next_total_cells;
                    bal.txs_count = next_txs_count;
                    bal.last_activity_block = delta.last_activity_block;
                    bal.last_activity_tx = delta.last_activity_tx.clone();
                    bal
                }
                None => {
                    if delta.balance_delta < 0
                        || delta.used_delta < 0
                        || delta.live_delta < 0
                        || delta.total_delta < 0
                        || delta.tx_delta < 0
                    {
                        bail!(
                            "address delta underflow for unseen address: lock_hash=0x{}, balance_delta={}, used_delta={}, live_delta={}, total_delta={}, tx_delta={}",
                            hex::encode(lock_hash),
                            delta.balance_delta,
                            delta.used_delta,
                            delta.live_delta,
                            delta.total_delta,
                            delta.tx_delta
                        );
                    }
                    AddressBalance {
                        balance: delta.balance_delta,
                        used_capacity: delta.used_delta,
                        live_cells_count: delta.live_delta,
                        total_cells_count: delta.total_delta as i64,
                        txs_count: delta.tx_delta,
                        first_seen_block: delta.first_seen_block,
                        first_seen_tx: delta.first_seen_tx.clone(),
                        last_activity_block: delta.last_activity_block,
                        last_activity_tx: delta.last_activity_tx.clone(),
                    }
                }
            };

            batch.put_addr_balance(lock_hash, &updated);
        }

        Ok(())
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
                owned_cap_delta,
                used_delta,
                owned_knowledge_delta,
            ),
        ) in changes
        {
            let existing_info = existing.get(code_hash).and_then(|o| o.clone());
            if existing_info.is_none()
                && (*cells_delta < 0
                    || *live_delta < 0
                    || *cap_delta < 0
                    || *owned_cap_delta < 0
                    || *used_delta < 0
                    || *owned_knowledge_delta < 0)
            {
                bail!(
                    "script delta underflow for unseen code_hash: code_hash=0x{}, is_type={}, cells_delta={}, live_delta={}, capacity_delta={}, owned_capacity_delta={}, used_delta={}, owned_knowledge_delta={}",
                    hex::encode(code_hash),
                    is_type,
                    cells_delta,
                    live_delta,
                    cap_delta,
                    owned_cap_delta,
                    used_delta,
                    owned_knowledge_delta
                );
            }

            let info = updated_map.entry(code_hash).or_insert_with(|| {
                let mut info = existing_info.unwrap_or_else(|| ckbadger_store::types::ScriptInfo {
                    code_hash: code_hash.clone(),
                    ..Default::default()
                });
                if let Ok(Some(fresh)) = self.store.get_script_info(code_hash) {
                    info = overlay_script_metadata(info, &fresh);
                }
                info
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
                let next_type_owned_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "owned_capacity_sum",
                    info.type_owned_capacity_sum,
                    *owned_cap_delta,
                )?;
                let next_type_used_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "used_capacity_sum",
                    info.type_used_capacity_sum,
                    *used_delta,
                )?;
                let next_type_owned_knowledge_sum = checked_next_script_metric_i128(
                    code_hash,
                    "type",
                    "owned_knowledge_sum",
                    info.type_owned_knowledge_sum,
                    *owned_knowledge_delta,
                )?;

                if next_type_used_capacity_sum > next_type_capacity_sum {
                    bail!(
                        "script type used capacity exceeds total: code_hash=0x{}, used_capacity_sum={}, capacity_sum={}",
                        hex::encode(code_hash),
                        next_type_used_capacity_sum,
                        next_type_capacity_sum
                    );
                }
                if next_type_owned_knowledge_sum > next_type_owned_capacity_sum {
                    bail!(
                        "script type owned knowledge exceeds owned capacity: code_hash=0x{}, owned_knowledge_sum={}, owned_capacity_sum={}",
                        hex::encode(code_hash),
                        next_type_owned_knowledge_sum,
                        next_type_owned_capacity_sum
                    );
                }

                info.type_cells_count = next_type_cells_count;
                info.type_live_cells_count = next_type_live_cells_count;
                info.type_capacity_sum = next_type_capacity_sum;
                info.type_owned_capacity_sum = next_type_owned_capacity_sum;
                info.type_used_capacity_sum = next_type_used_capacity_sum;
                info.type_owned_knowledge_sum = next_type_owned_knowledge_sum;
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
                let next_lock_owned_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "owned_capacity_sum",
                    info.lock_owned_capacity_sum,
                    *owned_cap_delta,
                )?;
                let next_lock_used_capacity_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "used_capacity_sum",
                    info.lock_used_capacity_sum,
                    *used_delta,
                )?;
                let next_lock_owned_knowledge_sum = checked_next_script_metric_i128(
                    code_hash,
                    "lock",
                    "owned_knowledge_sum",
                    info.lock_owned_knowledge_sum,
                    *owned_knowledge_delta,
                )?;

                if next_lock_used_capacity_sum > next_lock_capacity_sum {
                    bail!(
                        "script lock used capacity exceeds total: code_hash=0x{}, used_capacity_sum={}, capacity_sum={}",
                        hex::encode(code_hash),
                        next_lock_used_capacity_sum,
                        next_lock_capacity_sum
                    );
                }
                if next_lock_owned_knowledge_sum > next_lock_owned_capacity_sum {
                    bail!(
                        "script lock owned knowledge exceeds owned capacity: code_hash=0x{}, owned_knowledge_sum={}, owned_capacity_sum={}",
                        hex::encode(code_hash),
                        next_lock_owned_knowledge_sum,
                        next_lock_owned_capacity_sum
                    );
                }

                info.lock_cells_count = next_lock_cells_count;
                info.lock_live_cells_count = next_lock_live_cells_count;
                info.lock_capacity_sum = next_lock_capacity_sum;
                info.lock_owned_capacity_sum = next_lock_owned_capacity_sum;
                info.lock_used_capacity_sum = next_lock_used_capacity_sum;
                info.lock_owned_knowledge_sum = next_lock_owned_knowledge_sum;
            }
        }

        for (code_hash, info) in &updated_map {
            batch.put_script_info(code_hash, info);
        }

        Ok(())
    }

    pub fn read_script_reference_info(
        &self,
        references: &[(Vec<u8>, u8)],
    ) -> Result<HashMap<(Vec<u8>, u8), Option<ScriptReferenceInfo>>> {
        if references.is_empty() {
            return Ok(HashMap::new());
        }

        let encoded_keys: Vec<Vec<u8>> = references
            .iter()
            .map(|(reference_hash, hash_type)| {
                keys::encode_script_reference_key(*hash_type, reference_hash).to_vec()
            })
            .collect();
        let cf_keys: Vec<_> = encoded_keys
            .iter()
            .map(|key| (self.store.cf_script_reference_info(), key.as_slice()))
            .collect();
        let results = self.store.multi_get_cf(cf_keys);

        let mut map = HashMap::with_capacity(references.len());
        for (((reference_hash, hash_type), key), res) in references
            .iter()
            .zip(encoded_keys.iter())
            .zip(results.into_iter())
        {
            let existing: Option<ScriptReferenceInfo> = match res {
                Ok(Some(value)) => Some(bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script reference info: key=0x{}, hash_type={}, reference_hash=0x{}, error={}",
                        hex::encode(key),
                        hash_type,
                        hex::encode(reference_hash),
                        e
                    )
                })?),
                Ok(None) => None,
                Err(e) => {
                    bail!(
                        "failed to read script reference info: hash_type={}, reference_hash=0x{}, error={}",
                        hash_type,
                        hex::encode(reference_hash),
                        e
                    );
                }
            };
            map.insert((reference_hash.clone(), *hash_type), existing);
        }

        Ok(map)
    }

    pub fn apply_script_reference_usage_deltas(
        &self,
        existing: &HashMap<(Vec<u8>, u8), Option<ScriptReferenceInfo>>,
        changes: &HashMap<(Vec<u8>, u8), (i64, i64, i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut updated_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo> =
            HashMap::with_capacity(existing.len());

        for (
            (reference_hash, hash_type),
            (cells_delta, live_delta, owned_cap_delta, owned_knowledge_delta),
        ) in changes
        {
            let key = (reference_hash.clone(), *hash_type);
            let existing_info = existing.get(&key).and_then(|o| o.clone());
            if existing_info.is_none()
                && (*cells_delta < 0
                    || *live_delta < 0
                    || *owned_cap_delta < 0
                    || *owned_knowledge_delta < 0)
            {
                bail!(
                    "script reference delta underflow for unseen reference: reference_hash=0x{}, hash_type={}, cells_delta={}, live_delta={}, owned_capacity_delta={}, owned_knowledge_delta={}",
                    hex::encode(reference_hash),
                    hash_type,
                    cells_delta,
                    live_delta,
                    owned_cap_delta,
                    owned_knowledge_delta
                );
            }

            let info = updated_map.entry(key).or_insert_with(|| {
                existing_info.unwrap_or_else(|| ScriptReferenceInfo {
                    reference_hash: reference_hash.clone(),
                    hash_type: *hash_type,
                    ..Default::default()
                })
            });

            let next_cells_count = checked_next_script_reference_metric_i64(
                reference_hash,
                *hash_type,
                "cells_count",
                info.cells_count,
                *cells_delta,
            )?;
            let next_live_cells_count = checked_next_script_reference_metric_i64(
                reference_hash,
                *hash_type,
                "live_cells_count",
                info.live_cells_count,
                *live_delta,
            )?;
            let next_owned_capacity_sum = checked_next_script_reference_metric_i128(
                reference_hash,
                *hash_type,
                "owned_capacity_sum",
                info.owned_capacity_sum,
                *owned_cap_delta,
            )?;
            let next_owned_knowledge_sum = checked_next_script_reference_metric_i128(
                reference_hash,
                *hash_type,
                "owned_knowledge_sum",
                info.owned_knowledge_sum,
                *owned_knowledge_delta,
            )?;

            if next_owned_knowledge_sum > next_owned_capacity_sum {
                bail!(
                    "script reference owned knowledge exceeds owned capacity: reference_hash=0x{}, hash_type={}, owned_knowledge_sum={}, owned_capacity_sum={}",
                    hex::encode(reference_hash),
                    hash_type,
                    next_owned_knowledge_sum,
                    next_owned_capacity_sum
                );
            }

            info.cells_count = next_cells_count;
            info.live_cells_count = next_live_cells_count;
            info.owned_capacity_sum = next_owned_capacity_sum;
            info.owned_knowledge_sum = next_owned_knowledge_sum;
        }

        for ((reference_hash, hash_type), info) in &updated_map {
            batch.put_script_reference_info(*hash_type, reference_hash, info);
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

    pub fn update_script_reference_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u8), (i64, i64, i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let references: Vec<(Vec<u8>, u8)> = changes
            .keys()
            .map(|(reference_hash, hash_type)| (reference_hash.clone(), *hash_type))
            .collect();
        let existing = self.read_script_reference_info(&references)?;
        self.apply_script_reference_usage_deltas(&existing, changes, batch)
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
        for ((code_hash, is_type, date_yyyymmdd), (owned_cap_delta, owned_knowledge_delta)) in
            changes
        {
            if *owned_cap_delta == 0 && *owned_knowledge_delta == 0 {
                continue;
            }
            keyed_changes.push((
                keys::encode_script_daily_key(code_hash, *is_type, *date_yyyymmdd).to_vec(),
                *owned_cap_delta,
                *owned_knowledge_delta,
            ));
        }

        if keyed_changes.is_empty() {
            return Ok(());
        }

        let cf_keys: Vec<_> = keyed_changes
            .iter()
            .map(|(key, _, _)| {
                let cf = self.store.cf_for_stats_key(key)?;
                Ok((cf, key.as_slice()))
            })
            .collect::<Result<Vec<_>>>()?;
        let existing_results = self.store.multi_get_cf(cf_keys);

        for ((key, owned_cap_delta, owned_knowledge_delta), existing_res) in
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
            existing.owned_capacity_delta = existing
                .owned_capacity_delta
                .checked_add(owned_cap_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "script daily capacity delta overflow: key=0x{}, current={}, delta={}",
                        hex::encode(&key),
                        existing.owned_capacity_delta,
                        owned_cap_delta
                    )
                })?;
            existing.owned_knowledge_delta = existing
                .owned_knowledge_delta
                .checked_add(owned_knowledge_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "script daily used delta overflow: key=0x{}, current={}, delta={}",
                        hex::encode(&key),
                        existing.owned_knowledge_delta,
                        owned_knowledge_delta
                    )
                })?;
            if existing.owned_capacity_delta == 0 && existing.owned_knowledge_delta == 0 {
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

    use ckbadger_store::{CkbadgerStore, ScriptInfo, ScriptReferenceInfo};

    #[test]
    fn test_update_script_daily_deltas_batch_accumulates_and_deletes_zero_net() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
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
        assert_eq!(delta.owned_capacity_delta, 80);
        assert_eq!(delta.owned_knowledge_delta, 50);

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
    fn test_apply_script_usage_deltas_rejects_owned_capacity_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let code_hash = vec![0xAA; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: 100,
            lock_owned_capacity_sum: 100,
            lock_used_capacity_sum: 60,
            lock_owned_knowledge_sum: 60,
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
            .contains("script lock owned_capacity_sum underflow"));
    }

    #[test]
    fn test_apply_script_usage_deltas_rejects_owned_knowledge_over_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let code_hash = vec![0xBB; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            type_cells_count: 1,
            type_live_cells_count: 1,
            type_capacity_sum: 200,
            type_owned_capacity_sum: 200,
            type_used_capacity_sum: 100,
            type_owned_knowledge_sum: 100,
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
            .contains("script type owned knowledge exceeds owned capacity"));
    }

    #[test]
    fn test_apply_script_usage_deltas_allows_capacity_sum_above_i64_max() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let code_hash = vec![0xCC; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: i64::MAX as i128,
            lock_owned_capacity_sum: i64::MAX as i128,
            lock_used_capacity_sum: i64::MAX as i128 - 100,
            lock_owned_knowledge_sum: i64::MAX as i128 - 100,
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
        assert_eq!(updated.lock_owned_capacity_sum, i64::MAX as i128 + 500);
        assert_eq!(updated.lock_used_capacity_sum, i64::MAX as i128 + 300);
        assert_eq!(updated.lock_owned_knowledge_sum, i64::MAX as i128 + 300);
    }

    #[test]
    fn test_apply_script_usage_deltas_accepts_capacity_delta_exceeding_i64() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let code_hash = vec![0xDD; 32];
        let existing_info = ScriptInfo {
            code_hash: code_hash.clone(),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: 0,
            lock_owned_capacity_sum: 0,
            lock_used_capacity_sum: 0,
            lock_owned_knowledge_sum: 0,
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
        assert_eq!(updated.lock_owned_capacity_sum, huge_delta);
    }

    #[test]
    fn test_apply_script_usage_deltas_preserves_fresh_script_metadata_when_snapshot_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let code_hash = vec![0x9b; 32];
        let latest = ScriptInfo {
            code_hash: code_hash.clone(),
            hash_type: 1,
            name: Some("Default Lock".to_string()),
            deprecated: true,
            description: Some("mainnet default lock".to_string()),
            ..Default::default()
        };
        store.put_script_info_direct(&code_hash, &latest).unwrap();

        let existing = HashMap::new();
        let mut changes = HashMap::new();
        changes.insert((code_hash.clone(), false), (1, 1, 100, 100, 61, 61));

        let mut batch = StoreBatch::new(&store);
        writer
            .apply_script_usage_deltas(&existing, &changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let updated = store.get_script_info(&code_hash).unwrap().unwrap();
        assert_eq!(updated.hash_type, 1);
        assert_eq!(updated.name.as_deref(), Some("Default Lock"));
        assert!(updated.deprecated);
        assert_eq!(updated.description.as_deref(), Some("mainnet default lock"));
        // Correctness fields (dep_type_hash, dep_data_hash, code_cell_tx_hash,
        // code_cell_output_index) are NOT overlaid from existing records. Label import
        // no longer writes these; code cell resolution uses script_references/versions CFs.
        assert_eq!(updated.dep_type_hash, None);
        assert_eq!(updated.dep_data_hash, None);
        assert_eq!(updated.lock_cells_count, 1);
        assert_eq!(updated.lock_live_cells_count, 1);
        assert_eq!(updated.lock_capacity_sum, 100);
        assert_eq!(updated.lock_owned_capacity_sum, 100);
        assert_eq!(updated.lock_used_capacity_sum, 61);
        assert_eq!(updated.lock_owned_knowledge_sum, 61);
    }

    #[test]
    fn test_update_script_reference_usage_batch_persists_distinct_hash_types() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let reference_hash = vec![0x7a; 32];
        let mut existing = HashMap::new();
        existing.insert(
            (reference_hash.clone(), 0u8),
            Some(ScriptReferenceInfo {
                reference_hash: reference_hash.clone(),
                hash_type: 0,
                live_cells_count: 2,
                cells_count: 2,
                owned_capacity_sum: 200,
                owned_knowledge_sum: 120,
            }),
        );
        existing.insert((reference_hash.clone(), 1u8), None);

        let mut changes = HashMap::new();
        changes.insert((reference_hash.clone(), 0u8), (1, 1, 100, 61));
        changes.insert((reference_hash.clone(), 1u8), (3, 2, 300, 183));

        let mut batch = StoreBatch::new(&store);
        writer
            .apply_script_reference_usage_deltas(&existing, &changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let data_hash_info = store
            .get_script_reference_info(0, &reference_hash)
            .unwrap()
            .unwrap();
        assert_eq!(data_hash_info.cells_count, 3);
        assert_eq!(data_hash_info.live_cells_count, 3);
        assert_eq!(data_hash_info.owned_capacity_sum, 300);
        assert_eq!(data_hash_info.owned_knowledge_sum, 181);

        let type_hash_info = store
            .get_script_reference_info(1, &reference_hash)
            .unwrap()
            .unwrap();
        assert_eq!(type_hash_info.cells_count, 3);
        assert_eq!(type_hash_info.live_cells_count, 2);
        assert_eq!(type_hash_info.owned_capacity_sum, 300);
        assert_eq!(type_hash_info.owned_knowledge_sum, 183);
    }

    #[test]
    fn test_read_script_info_errors_on_deserialize_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
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
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

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
