use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    AddressBalance, ScriptDailyDelta, ScriptFamilyInfo, ScriptReferenceInfo, ScriptVersionInfo,
};

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

fn clear_script_version_usage(info: &mut ScriptVersionInfo) {
    info.lock_cells_count = 0;
    info.lock_live_cells_count = 0;
    info.lock_capacity_sum = 0;
    info.lock_owned_capacity_sum = 0;
    info.lock_used_capacity_sum = 0;
    info.lock_owned_knowledge_sum = 0;
    info.type_cells_count = 0;
    info.type_live_cells_count = 0;
    info.type_capacity_sum = 0;
    info.type_owned_capacity_sum = 0;
    info.type_used_capacity_sum = 0;
    info.type_owned_knowledge_sum = 0;
}

fn clear_script_family_usage(info: &mut ScriptFamilyInfo) {
    info.deprecated = true;
    info.versions_count = 0;
    info.live_cells_count = 0;
    info.cells_count = 0;
    info.lock_cells_count = 0;
    info.type_cells_count = 0;
    info.owned_capacity_sum = 0;
    info.owned_knowledge_sum = 0;
}

fn checked_add_version_i64(
    version_hash: &[u8],
    metric: &str,
    current: i64,
    delta: i64,
) -> Result<i64> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script version {} overflow: version_hash=0x{}, current={}, delta={}",
            metric,
            hex::encode(version_hash),
            current,
            delta
        )
    })
}

fn checked_add_version_i128(
    version_hash: &[u8],
    metric: &str,
    current: i128,
    delta: i128,
) -> Result<i128> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script version {} overflow: version_hash=0x{}, current={}, delta={}",
            metric,
            hex::encode(version_hash),
            current,
            delta
        )
    })
}

fn checked_add_family_i64(family_id: &str, metric: &str, current: i64, delta: i64) -> Result<i64> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script family {} overflow: family_id={}, current={}, delta={}",
            metric,
            family_id,
            current,
            delta
        )
    })
}

fn checked_add_family_i128(
    family_id: &str,
    metric: &str,
    current: i128,
    delta: i128,
) -> Result<i128> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "script family {} overflow: family_id={}, current={}, delta={}",
            metric,
            family_id,
            current,
            delta
        )
    })
}

fn resolve_reference_version_hash(
    store: &ckbadger_store::CkbadgerStore,
    append_only_store: &ckbadger_store::CkbadgerStore,
    reference_hash: &[u8],
    hash_type: u8,
) -> Result<Option<Vec<u8>>> {
    match hash_type {
        0 | 2 | 4 => Ok(Some(reference_hash.to_vec())),
        1 => Ok(
            resolve_type_reference_live_versions(store, append_only_store, reference_hash)?
                .into_iter()
                .next(),
        ),
        _ => bail!(
            "unsupported script reference hash_type while rebuilding rollups: reference_hash=0x{}, hash_type={}, expected_one_of=[0,1,2,4]",
            hex::encode(reference_hash),
            hash_type
        ),
    }
}

fn resolve_type_reference_live_versions(
    store: &ckbadger_store::CkbadgerStore,
    append_only_store: &ckbadger_store::CkbadgerStore,
    reference_hash: &[u8],
) -> Result<Vec<Vec<u8>>> {
    let mut seen = HashSet::new();
    let mut versions = Vec::new();
    for (tx_hash, output_index, cell) in
        store.list_cells_by_type(reference_hash, usize::MAX, None, append_only_store)?
    {
        let version_hash = cell.cell.data_hash.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "live type-referenced code cell missing data_hash while rebuilding reference rollups: reference_hash=0x{}, outpoint=0x{}:{}",
                hex::encode(reference_hash),
                hex::encode(&tx_hash),
                output_index
            )
        })?;
        if seen.insert(version_hash.clone()) {
            versions.push(version_hash);
        }
    }
    versions.sort();
    Ok(versions)
}

#[derive(Debug, Default)]
pub(crate) struct ScriptReferenceRollupState {
    pub(crate) reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)>,
    pub(crate) versions: Vec<(Vec<u8>, ScriptVersionInfo)>,
    pub(crate) families: Vec<(String, ScriptFamilyInfo)>,
}

pub(crate) fn build_script_reference_rollup_state(
    store: &ckbadger_store::CkbadgerStore,
    mut reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)>,
    reference_info_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
) -> Result<ScriptReferenceRollupState> {
    let existing_versions = store.list_script_versions()?;
    let existing_families = store.list_script_families()?;

    let mut version_map: HashMap<Vec<u8>, ScriptVersionInfo> =
        existing_versions.into_iter().collect();
    for version in version_map.values_mut() {
        clear_script_version_usage(version);
    }

    for ((reference_hash, hash_type), resolved_version) in &reference_mappings {
        let Some(reference_info) = reference_info_map.get(&(reference_hash.clone(), *hash_type))
        else {
            bail!(
                "missing script reference info while building rollups: reference_hash=0x{}, hash_type={}",
                hex::encode(reference_hash),
                hash_type
            );
        };

        let Some(version_hash) = resolved_version else {
            continue;
        };
        let version =
            version_map
                .entry(version_hash.clone())
                .or_insert_with(|| ScriptVersionInfo {
                    version_hash: version_hash.clone(),
                    ..Default::default()
                });
        version.version_hash = version_hash.clone();
        version.lock_cells_count = checked_add_version_i64(
            version_hash,
            "lock_cells_count",
            version.lock_cells_count,
            reference_info.lock_cells_count,
        )?;
        version.lock_live_cells_count = checked_add_version_i64(
            version_hash,
            "lock_live_cells_count",
            version.lock_live_cells_count,
            reference_info.lock_live_cells_count,
        )?;
        version.lock_capacity_sum = checked_add_version_i128(
            version_hash,
            "lock_capacity_sum",
            version.lock_capacity_sum,
            reference_info.lock_capacity_sum,
        )?;
        version.lock_owned_capacity_sum = checked_add_version_i128(
            version_hash,
            "lock_owned_capacity_sum",
            version.lock_owned_capacity_sum,
            reference_info.lock_owned_capacity_sum,
        )?;
        version.lock_used_capacity_sum = checked_add_version_i128(
            version_hash,
            "lock_used_capacity_sum",
            version.lock_used_capacity_sum,
            reference_info.lock_used_capacity_sum,
        )?;
        version.lock_owned_knowledge_sum = checked_add_version_i128(
            version_hash,
            "lock_owned_knowledge_sum",
            version.lock_owned_knowledge_sum,
            reference_info.lock_owned_knowledge_sum,
        )?;
        version.type_cells_count = checked_add_version_i64(
            version_hash,
            "type_cells_count",
            version.type_cells_count,
            reference_info.type_cells_count,
        )?;
        version.type_live_cells_count = checked_add_version_i64(
            version_hash,
            "type_live_cells_count",
            version.type_live_cells_count,
            reference_info.type_live_cells_count,
        )?;
        version.type_capacity_sum = checked_add_version_i128(
            version_hash,
            "type_capacity_sum",
            version.type_capacity_sum,
            reference_info.type_capacity_sum,
        )?;
        version.type_owned_capacity_sum = checked_add_version_i128(
            version_hash,
            "type_owned_capacity_sum",
            version.type_owned_capacity_sum,
            reference_info.type_owned_capacity_sum,
        )?;
        version.type_used_capacity_sum = checked_add_version_i128(
            version_hash,
            "type_used_capacity_sum",
            version.type_used_capacity_sum,
            reference_info.type_used_capacity_sum,
        )?;
        version.type_owned_knowledge_sum = checked_add_version_i128(
            version_hash,
            "type_owned_knowledge_sum",
            version.type_owned_knowledge_sum,
            reference_info.type_owned_knowledge_sum,
        )?;
    }

    let mut family_map: HashMap<String, ScriptFamilyInfo> = existing_families.into_iter().collect();
    for family in family_map.values_mut() {
        clear_script_family_usage(family);
    }

    for version in version_map.values() {
        let Some(family_id) = version.family_id.as_deref() else {
            continue;
        };
        let family = family_map
            .entry(family_id.to_string())
            .or_insert_with(|| ScriptFamilyInfo {
                family_id: family_id.to_string(),
                deprecated: true,
                ..Default::default()
            });
        family.family_id = family_id.to_string();
        if !version.deprecated {
            family.deprecated = false;
        }
        family.versions_count =
            checked_add_family_i64(family_id, "versions_count", family.versions_count, 1)?;
        family.live_cells_count = checked_add_family_i64(
            family_id,
            "live_cells_count",
            family.live_cells_count,
            version.lock_live_cells_count + version.type_live_cells_count,
        )?;
        family.cells_count = checked_add_family_i64(
            family_id,
            "cells_count",
            family.cells_count,
            version.lock_cells_count + version.type_cells_count,
        )?;
        family.lock_cells_count = checked_add_family_i64(
            family_id,
            "lock_cells_count",
            family.lock_cells_count,
            version.lock_cells_count,
        )?;
        family.type_cells_count = checked_add_family_i64(
            family_id,
            "type_cells_count",
            family.type_cells_count,
            version.type_cells_count,
        )?;
        family.owned_capacity_sum = checked_add_family_i128(
            family_id,
            "owned_capacity_sum",
            family.owned_capacity_sum,
            version.lock_owned_capacity_sum + version.type_owned_capacity_sum,
        )?;
        family.owned_knowledge_sum = checked_add_family_i128(
            family_id,
            "owned_knowledge_sum",
            family.owned_knowledge_sum,
            version.lock_owned_knowledge_sum + version.type_owned_knowledge_sum,
        )?;
    }

    let mut versions = version_map.into_iter().collect::<Vec<_>>();
    versions.sort_by(|left, right| left.0.cmp(&right.0));

    let mut families = family_map.into_iter().collect::<Vec<_>>();
    families.sort_by(|left, right| left.0.cmp(&right.0));

    reference_mappings.sort_by(|left, right| left.0.cmp(&right.0));

    Ok(ScriptReferenceRollupState {
        reference_mappings,
        versions,
        families,
    })
}

pub(crate) fn collect_current_script_reference_rollup_state(
    store: &ckbadger_store::CkbadgerStore,
    append_only_store: &ckbadger_store::CkbadgerStore,
) -> Result<ScriptReferenceRollupState> {
    let all_infos = store.list_script_reference_infos()?;
    let reference_mappings = all_infos
        .iter()
        .map(|((reference_hash, hash_type), _info)| {
            let version_hash = if *hash_type == 1 {
                let live_versions =
                    resolve_type_reference_live_versions(store, append_only_store, reference_hash)?;
                match live_versions.len() {
                    0 => store.get_script_reference_version_hash(1, reference_hash)?,
                    1 => Some(live_versions[0].clone()),
                    _ => None,
                }
            } else {
                resolve_reference_version_hash(
                    store,
                    append_only_store,
                    reference_hash,
                    *hash_type,
                )?
            };
            Ok(((reference_hash.clone(), *hash_type), version_hash))
        })
        .collect::<Result<Vec<_>>>()?;
    let reference_info_map = all_infos.into_iter().collect();
    build_script_reference_rollup_state(store, reference_mappings, reference_info_map)
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
                    let next_balance =
                        bal.balance
                            .checked_add(delta.balance_delta)
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                            "address balance overflow: lock_hash=0x{}, balance={}, delta={}",
                            hex::encode(lock_hash),
                            bal.balance,
                            delta.balance_delta
                        )
                            })?;
                    if next_balance < 0 {
                        bail!(
                            "address balance underflow: lock_hash=0x{}, balance={}, delta={}",
                            hex::encode(lock_hash),
                            bal.balance,
                            delta.balance_delta
                        );
                    }
                    let next_used = bal.used_capacity.checked_add(delta.used_delta).ok_or_else(|| {
                        anyhow::anyhow!(
                            "address used capacity overflow: lock_hash=0x{}, used_capacity={}, delta={}",
                            hex::encode(lock_hash),
                            bal.used_capacity,
                            delta.used_delta
                        )
                    })?;
                    if next_used < 0 {
                        bail!(
                            "address used capacity underflow: lock_hash=0x{}, used_capacity={}, delta={}",
                            hex::encode(lock_hash),
                            bal.used_capacity,
                            delta.used_delta
                        );
                    }
                    let next_live_cells = bal.live_cells_count.checked_add(delta.live_delta).ok_or_else(|| {
                        anyhow::anyhow!(
                            "address live_cells_count overflow: lock_hash=0x{}, live_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.live_cells_count,
                            delta.live_delta
                        )
                    })?;
                    if next_live_cells < 0 {
                        bail!(
                            "address live_cells_count underflow: lock_hash=0x{}, live_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.live_cells_count,
                            delta.live_delta
                        );
                    }
                    let next_total_cells = bal.total_cells_count.checked_add(delta.total_delta as i64).ok_or_else(|| {
                        anyhow::anyhow!(
                            "address total_cells_count overflow: lock_hash=0x{}, total_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.total_cells_count,
                            delta.total_delta
                        )
                    })?;
                    if next_total_cells < 0 {
                        bail!(
                            "address total_cells_count underflow: lock_hash=0x{}, total_cells_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.total_cells_count,
                            delta.total_delta
                        );
                    }
                    let next_txs_count =
                        bal.txs_count.checked_add(delta.tx_delta).ok_or_else(|| {
                            anyhow::anyhow!(
                            "address txs_count overflow: lock_hash=0x{}, txs_count={}, delta={}",
                            hex::encode(lock_hash),
                            bal.txs_count,
                            delta.tx_delta
                        )
                        })?;
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
        changes: &HashMap<(Vec<u8>, u8, bool), (i64, i64, i128, i128, i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<HashMap<(Vec<u8>, u8), ScriptReferenceInfo>> {
        if changes.is_empty() {
            return Ok(HashMap::new());
        }

        let mut updated_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo> =
            HashMap::with_capacity(existing.len());

        for (
            (reference_hash, hash_type, is_type),
            (
                cells_delta,
                live_delta,
                capacity_delta,
                owned_cap_delta,
                used_delta,
                owned_knowledge_delta,
            ),
        ) in changes
        {
            let key = (reference_hash.clone(), *hash_type);
            let existing_info = existing.get(&key).and_then(|o| o.clone());
            if existing_info.is_none()
                && (*cells_delta < 0
                    || *live_delta < 0
                    || *capacity_delta < 0
                    || *owned_cap_delta < 0
                    || *used_delta < 0
                    || *owned_knowledge_delta < 0)
            {
                bail!(
                    "script reference delta underflow for unseen reference: reference_hash=0x{}, hash_type={}, is_type={}, cells_delta={}, live_delta={}, capacity_delta={}, owned_capacity_delta={}, used_delta={}, owned_knowledge_delta={}",
                    hex::encode(reference_hash),
                    hash_type,
                    is_type,
                    cells_delta,
                    live_delta,
                    capacity_delta,
                    owned_cap_delta,
                    used_delta,
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

            if *is_type {
                let next_cells_count = checked_next_script_reference_metric_i64(
                    reference_hash,
                    *hash_type,
                    "type cells_count",
                    info.type_cells_count,
                    *cells_delta,
                )?;
                let next_live_cells_count = checked_next_script_reference_metric_i64(
                    reference_hash,
                    *hash_type,
                    "type live_cells_count",
                    info.type_live_cells_count,
                    *live_delta,
                )?;
                let next_capacity_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "type capacity_sum",
                    info.type_capacity_sum,
                    *capacity_delta,
                )?;
                let next_owned_capacity_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "type owned_capacity_sum",
                    info.type_owned_capacity_sum,
                    *owned_cap_delta,
                )?;
                let next_used_capacity_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "type used_capacity_sum",
                    info.type_used_capacity_sum,
                    *used_delta,
                )?;
                let next_owned_knowledge_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "type owned_knowledge_sum",
                    info.type_owned_knowledge_sum,
                    *owned_knowledge_delta,
                )?;

                if next_used_capacity_sum > next_capacity_sum {
                    bail!(
                        "script reference type used capacity exceeds total: reference_hash=0x{}, hash_type={}, type_used_capacity_sum={}, type_capacity_sum={}",
                        hex::encode(reference_hash),
                        hash_type,
                        next_used_capacity_sum,
                        next_capacity_sum
                    );
                }
                if next_owned_knowledge_sum > next_owned_capacity_sum {
                    bail!(
                        "script reference type owned knowledge exceeds owned capacity: reference_hash=0x{}, hash_type={}, type_owned_knowledge_sum={}, type_owned_capacity_sum={}",
                        hex::encode(reference_hash),
                        hash_type,
                        next_owned_knowledge_sum,
                        next_owned_capacity_sum
                    );
                }

                info.type_cells_count = next_cells_count;
                info.type_live_cells_count = next_live_cells_count;
                info.type_capacity_sum = next_capacity_sum;
                info.type_owned_capacity_sum = next_owned_capacity_sum;
                info.type_used_capacity_sum = next_used_capacity_sum;
                info.type_owned_knowledge_sum = next_owned_knowledge_sum;
            } else {
                let next_cells_count = checked_next_script_reference_metric_i64(
                    reference_hash,
                    *hash_type,
                    "lock cells_count",
                    info.lock_cells_count,
                    *cells_delta,
                )?;
                let next_live_cells_count = checked_next_script_reference_metric_i64(
                    reference_hash,
                    *hash_type,
                    "lock live_cells_count",
                    info.lock_live_cells_count,
                    *live_delta,
                )?;
                let next_capacity_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "lock capacity_sum",
                    info.lock_capacity_sum,
                    *capacity_delta,
                )?;
                let next_owned_capacity_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "lock owned_capacity_sum",
                    info.lock_owned_capacity_sum,
                    *owned_cap_delta,
                )?;
                let next_used_capacity_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "lock used_capacity_sum",
                    info.lock_used_capacity_sum,
                    *used_delta,
                )?;
                let next_owned_knowledge_sum = checked_next_script_reference_metric_i128(
                    reference_hash,
                    *hash_type,
                    "lock owned_knowledge_sum",
                    info.lock_owned_knowledge_sum,
                    *owned_knowledge_delta,
                )?;

                if next_used_capacity_sum > next_capacity_sum {
                    bail!(
                        "script reference lock used capacity exceeds total: reference_hash=0x{}, hash_type={}, lock_used_capacity_sum={}, lock_capacity_sum={}",
                        hex::encode(reference_hash),
                        hash_type,
                        next_used_capacity_sum,
                        next_capacity_sum
                    );
                }
                if next_owned_knowledge_sum > next_owned_capacity_sum {
                    bail!(
                        "script reference lock owned knowledge exceeds owned capacity: reference_hash=0x{}, hash_type={}, lock_owned_knowledge_sum={}, lock_owned_capacity_sum={}",
                        hex::encode(reference_hash),
                        hash_type,
                        next_owned_knowledge_sum,
                        next_owned_capacity_sum
                    );
                }

                info.lock_cells_count = next_cells_count;
                info.lock_live_cells_count = next_live_cells_count;
                info.lock_capacity_sum = next_capacity_sum;
                info.lock_owned_capacity_sum = next_owned_capacity_sum;
                info.lock_used_capacity_sum = next_used_capacity_sum;
                info.lock_owned_knowledge_sum = next_owned_knowledge_sum;
            }
        }

        for ((reference_hash, hash_type), info) in &updated_map {
            batch.put_script_reference_info(*hash_type, reference_hash, info);
        }

        Ok(updated_map)
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
        changes: &HashMap<(Vec<u8>, u8, bool), (i64, i64, i128, i128, i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<HashMap<(Vec<u8>, u8), ScriptReferenceInfo>> {
        if changes.is_empty() {
            return Ok(HashMap::new());
        }

        let mut seen = HashSet::new();
        let references: Vec<(Vec<u8>, u8)> = changes
            .keys()
            .filter_map(|(reference_hash, hash_type, _is_type)| {
                let key = (reference_hash.clone(), *hash_type);
                seen.insert(key.clone()).then_some(key)
            })
            .collect();
        let existing = self.read_script_reference_info(&references)?;
        self.apply_script_reference_usage_deltas(&existing, changes, batch)
    }

    pub fn refresh_script_reference_rollups(&self) -> Result<()> {
        let rollups = collect_current_script_reference_rollup_state(
            self.store.as_ref(),
            self.append_only_store.as_ref(),
        )?;

        let mut batch = StoreBatch::new(self.store.as_ref());
        for ((reference_hash, hash_type), version_hash) in rollups.reference_mappings {
            if let Some(version_hash) = version_hash {
                batch.put_script_reference_to_version(hash_type, &reference_hash, &version_hash);
            } else {
                batch.delete_script_reference_to_version(hash_type, &reference_hash);
            }
        }
        for (version_hash, info) in rollups.versions {
            batch.put_script_version(&version_hash, &info);
        }
        for (family_id, info) in rollups.families {
            batch.put_script_family(&family_id, &info);
        }

        if batch.is_empty() {
            return Ok(());
        }
        batch.commit()
    }

    pub fn materialize_script_versions_and_families(
        &self,
        updated_references: &HashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        // Build complete reference info map: committed state + batch-pending updates.
        // This ensures the rollup reflects post-delta state for references modified
        // in the current batch, rather than reading stale pre-commit values.
        let mut reference_info_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo> = self
            .store
            .list_script_reference_infos()?
            .into_iter()
            .collect();
        for (key, info) in updated_references {
            reference_info_map.insert(key.clone(), info.clone());
        }

        let reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)> = reference_info_map
            .keys()
            .map(|(reference_hash, hash_type)| {
                let version_hash = self
                    .store
                    .get_script_reference_version_hash(*hash_type, reference_hash)?;
                Ok(((reference_hash.clone(), *hash_type), version_hash))
            })
            .collect::<Result<Vec<_>>>()?;

        let rollups = build_script_reference_rollup_state(
            self.store.as_ref(),
            reference_mappings,
            reference_info_map,
        )?;

        for ((reference_hash, hash_type), version_hash) in rollups.reference_mappings {
            if let Some(version_hash) = version_hash {
                batch.put_script_reference_to_version(hash_type, &reference_hash, &version_hash);
            } else {
                batch.delete_script_reference_to_version(hash_type, &reference_hash);
            }
        }
        for (version_hash, info) in rollups.versions {
            batch.put_script_version(&version_hash, &info);
        }
        for (family_id, info) in rollups.families {
            batch.put_script_family(&family_id, &info);
        }

        Ok(())
    }

    pub fn refresh_type_script_reference_version_mappings(
        &self,
        reference_hashes: &[Vec<u8>],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        for reference_hash in reference_hashes {
            let live_versions = resolve_type_reference_live_versions(
                self.store.as_ref(),
                self.append_only_store.as_ref(),
                reference_hash,
            )?;
            let version_hash = match live_versions.len() {
                0 => self
                    .store
                    .get_script_reference_version_hash(1, reference_hash)?,
                1 => Some(live_versions[0].clone()),
                _ => None,
            };
            if let Some(version_hash) = version_hash {
                batch.put_script_reference_to_version(1, reference_hash, &version_hash);
            } else {
                batch.delete_script_reference_to_version(1, reference_hash);
            }
        }
        Ok(())
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
                lock_cells_count: 2,
                lock_live_cells_count: 2,
                lock_capacity_sum: 200,
                lock_owned_capacity_sum: 200,
                lock_used_capacity_sum: 120,
                lock_owned_knowledge_sum: 120,
                type_cells_count: 0,
                type_live_cells_count: 0,
                type_capacity_sum: 0,
                type_owned_capacity_sum: 0,
                type_used_capacity_sum: 0,
                type_owned_knowledge_sum: 0,
            }),
        );
        existing.insert((reference_hash.clone(), 1u8), None);

        let mut changes = HashMap::new();
        changes.insert(
            (reference_hash.clone(), 0u8, false),
            (1, 1, 100, 100, 61, 61),
        );
        changes.insert(
            (reference_hash.clone(), 1u8, true),
            (3, 2, 300, 300, 183, 183),
        );

        let mut batch = StoreBatch::new(&store);
        writer
            .apply_script_reference_usage_deltas(&existing, &changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let data_hash_info = store
            .get_script_reference_info(0, &reference_hash)
            .unwrap()
            .unwrap();
        assert_eq!(data_hash_info.lock_cells_count, 3);
        assert_eq!(data_hash_info.lock_live_cells_count, 3);
        assert_eq!(data_hash_info.lock_capacity_sum, 300);
        assert_eq!(data_hash_info.lock_owned_capacity_sum, 300);
        assert_eq!(data_hash_info.lock_used_capacity_sum, 181);
        assert_eq!(data_hash_info.lock_owned_knowledge_sum, 181);
        assert_eq!(data_hash_info.type_cells_count, 0);

        let type_hash_info = store
            .get_script_reference_info(1, &reference_hash)
            .unwrap()
            .unwrap();
        assert_eq!(type_hash_info.type_cells_count, 3);
        assert_eq!(type_hash_info.type_live_cells_count, 2);
        assert_eq!(type_hash_info.type_capacity_sum, 300);
        assert_eq!(type_hash_info.type_owned_capacity_sum, 300);
        assert_eq!(type_hash_info.type_used_capacity_sum, 183);
        assert_eq!(type_hash_info.type_owned_knowledge_sum, 183);
        assert_eq!(type_hash_info.lock_cells_count, 0);
    }

    #[test]
    fn test_materialize_script_versions_and_families_rolls_up_reference_records_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let family_id = "default-lock";
        let version_hash = vec![0x41; 32];
        let type_reference_hash = vec![0x51; 32];
        let data_reference_hash = vec![0x61; 32];

        store
            .put_script_family_direct(
                family_id,
                &ckbadger_store::ScriptFamilyInfo {
                    family_id: family_id.to_string(),
                    name: "Default Lock".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_version(
                &version_hash,
                &ckbadger_store::ScriptVersionInfo {
                    version_hash: version_hash.clone(),
                    family_id: Some(family_id.to_string()),
                    canonical_reference_hash: Some(type_reference_hash.clone()),
                    canonical_hash_type: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_reference_info_direct(
                1,
                &type_reference_hash,
                &ScriptReferenceInfo {
                    reference_hash: type_reference_hash.clone(),
                    hash_type: 1,
                    lock_cells_count: 1,
                    lock_live_cells_count: 1,
                    lock_capacity_sum: 100,
                    lock_owned_capacity_sum: 100,
                    lock_used_capacity_sum: 61,
                    lock_owned_knowledge_sum: 61,
                    type_cells_count: 2,
                    type_live_cells_count: 1,
                    type_capacity_sum: 200,
                    type_owned_capacity_sum: 90,
                    type_used_capacity_sum: 122,
                    type_owned_knowledge_sum: 55,
                },
            )
            .unwrap();
        store
            .put_script_reference_info_direct(
                0,
                &data_reference_hash,
                &ScriptReferenceInfo {
                    reference_hash: data_reference_hash.clone(),
                    hash_type: 0,
                    lock_cells_count: 3,
                    lock_live_cells_count: 2,
                    lock_capacity_sum: 300,
                    lock_owned_capacity_sum: 180,
                    lock_used_capacity_sum: 183,
                    lock_owned_knowledge_sum: 110,
                    type_cells_count: 4,
                    type_live_cells_count: 3,
                    type_capacity_sum: 400,
                    type_owned_capacity_sum: 270,
                    type_used_capacity_sum: 244,
                    type_owned_knowledge_sum: 166,
                },
            )
            .unwrap();
        store
            .put_script_reference_to_version_direct(1, &type_reference_hash, &version_hash)
            .unwrap();
        store
            .put_script_reference_to_version_direct(0, &data_reference_hash, &version_hash)
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families(&HashMap::new(), &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let version = store.get_script_version(&version_hash).unwrap().unwrap();
        assert_eq!(version.lock_cells_count, 4);
        assert_eq!(version.lock_live_cells_count, 3);
        assert_eq!(version.lock_capacity_sum, 400);
        assert_eq!(version.lock_owned_capacity_sum, 280);
        assert_eq!(version.lock_used_capacity_sum, 244);
        assert_eq!(version.lock_owned_knowledge_sum, 171);
        assert_eq!(version.type_cells_count, 6);
        assert_eq!(version.type_live_cells_count, 4);
        assert_eq!(version.type_capacity_sum, 600);
        assert_eq!(version.type_owned_capacity_sum, 360);
        assert_eq!(version.type_used_capacity_sum, 366);
        assert_eq!(version.type_owned_knowledge_sum, 221);

        let family = store.get_script_family(family_id).unwrap().unwrap();
        assert_eq!(family.versions_count, 1);
        assert_eq!(family.cells_count, 10);
        assert_eq!(family.live_cells_count, 7);
        assert_eq!(family.owned_capacity_sum, 640);
        assert_eq!(family.owned_knowledge_sum, 392);
    }

    #[test]
    fn test_refresh_type_script_reference_version_mapping_replaces_stale_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let reference_hash = vec![0x71; 32];
        let stale_version = vec![0x81; 32];
        let live_version = vec![0x91; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &stale_version)
            .unwrap();

        let mut seed_batch = StoreBatch::new(&store);
        seed_batch.put_cell(
            &[0x45; 32],
            0,
            &ckbadger_store::types::LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(live_version.clone()),
            },
            10,
        );
        seed_batch.put_cell_by_type(&reference_hash, 10, &[0x45; 32], 0);
        seed_batch.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        writer
            .refresh_type_script_reference_version_mappings(
                std::slice::from_ref(&reference_hash),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let resolved = store
            .get_script_reference_version_hash(1, &reference_hash)
            .unwrap();
        assert_eq!(resolved, Some(live_version));
    }

    #[test]
    fn test_refresh_type_script_reference_version_mapping_preserves_existing_value_without_live_cells(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let reference_hash = vec![0x72; 32];
        let persisted_version = vec![0x82; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &persisted_version)
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        writer
            .refresh_type_script_reference_version_mappings(
                std::slice::from_ref(&reference_hash),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let resolved = store
            .get_script_reference_version_hash(1, &reference_hash)
            .unwrap();
        assert_eq!(resolved, Some(persisted_version));
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
