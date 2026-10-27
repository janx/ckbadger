use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    AddressBalance, ScriptFamilyInfo, ScriptReferenceInfo, ScriptVersionInfo,
};

use super::entity_stats::{apply_daily_pair, EntityStatsOverlay};
use crate::sync::types::{EntityDailyChanges, ScriptDailyKey};

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

#[derive(Debug, Clone)]
pub(crate) struct TypeIdCodeChange {
    pub reference_hash: Vec<u8>,
    /// None retires the currently bound executable; historical versions remain.
    pub version_hash: Option<Vec<u8>>,
    pub block_number: i64,
}

/// Bind only observed ELF code to a registered type-id's family. Registry
/// binaries are labels; the chain supplies upgrades, including unknown ones.
fn apply_type_id_lineage(
    versions: &mut HashMap<Vec<u8>, ScriptVersionInfo>,
    templates: &HashMap<Vec<u8>, ScriptVersionInfo>,
    observed: &TypeIdCodeChange,
) -> Result<()> {
    let Some(version_hash) = &observed.version_hash else {
        return Ok(());
    };
    let template = templates.get(&observed.reference_hash);
    let version = versions
        .entry(version_hash.clone())
        .or_insert_with(|| ScriptVersionInfo {
            version_hash: version_hash.clone(),
            ..Default::default()
        });
    if let Some(template) = template {
        if let Some(family) = &version.family_id {
            if Some(family) != template.family_id.as_ref() {
                bail!("type-id binary belongs to a different registered family: block={} reference=0x{} binary=0x{} expected={:?} actual={family}",
                    observed.block_number, hex::encode(&observed.reference_hash), hex::encode(version_hash), template.family_id);
            }
        }
        if version.family_id.is_none() {
            version.family_id = template.family_id.clone();
            version.name = template.name.clone();
            version.category = template.category.clone();
            version.website = template.website.clone();
            version.description = template.description.clone();
        }
    }
    // Imported canonical metadata is not changed by arbitrary code copies.
    // A newly discovered registered-lineage version may display that anchor,
    // but it does not acquire an imported registration of its own.
    if template.is_some() && version.canonical_reference_hash.is_none() {
        version.canonical_reference_hash = Some(observed.reference_hash.clone());
        version.canonical_hash_type = Some(1);
    }
    let deployment = (observed.reference_hash.clone(), observed.block_number);
    if !version.type_id_deployments.contains(&deployment) {
        version.type_id_deployments.push(deployment);
    }
    Ok(())
}

fn type_id_family_templates(
    versions: &HashMap<Vec<u8>, ScriptVersionInfo>,
) -> Result<HashMap<Vec<u8>, ScriptVersionInfo>> {
    let mut templates: HashMap<Vec<u8>, ScriptVersionInfo> = HashMap::new();
    let mut ordered: Vec<_> = versions.iter().collect();
    ordered.sort_by_key(|(hash, _)| *hash);
    for (_, version) in ordered {
        if version.family_id.is_none() {
            continue;
        }
        for (reference, kind) in &version.registered_references {
            if *kind != 1 {
                continue;
            }
            if let Some(previous) = templates.get(reference) {
                if previous.family_id != version.family_id {
                    bail!("type-id reference registered in conflicting families: reference=0x{} families={:?}/{:?}", hex::encode(reference), previous.family_id, version.family_id);
                }
            } else {
                templates.insert(reference.clone(), version.clone());
            }
        }
    }
    Ok(templates)
}

#[derive(Debug, Default)]
pub(crate) struct ScriptReferenceRollupState {
    pub(crate) reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)>,
    pub(crate) versions: Vec<(Vec<u8>, ScriptVersionInfo)>,
    pub(crate) families: Vec<(String, ScriptFamilyInfo)>,
    /// The committed rows this rollup was computed FROM, so a caller can stage
    /// only the rows whose value actually changed. Read once here rather than
    /// re-read per row at write time.
    pub(crate) existing_versions: HashMap<Vec<u8>, ScriptVersionInfo>,
    pub(crate) existing_families: HashMap<String, ScriptFamilyInfo>,
    pub(crate) lineage_undo: Vec<(i64, &'static str, Vec<u8>, Option<Vec<u8>>)>,
}

pub(crate) fn build_script_reference_rollup_state(
    store: &ckbadger_store::CkbadgerStore,
    reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)>,
    reference_info_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
) -> Result<ScriptReferenceRollupState> {
    build_script_reference_rollup_state_with_lineage(
        store,
        reference_mappings,
        reference_info_map,
        &[],
    )
}

pub(crate) fn build_script_reference_rollup_state_with_lineage(
    store: &ckbadger_store::CkbadgerStore,
    mut reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)>,
    reference_info_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
    lineage: &[TypeIdCodeChange],
) -> Result<ScriptReferenceRollupState> {
    let existing_versions: HashMap<Vec<u8>, ScriptVersionInfo> =
        store.list_script_versions()?.into_iter().collect();
    let existing_families: HashMap<String, ScriptFamilyInfo> =
        store.list_script_families()?.into_iter().collect();

    let mut version_map: HashMap<Vec<u8>, ScriptVersionInfo> = existing_versions.clone();
    let templates = type_id_family_templates(&existing_versions)?;
    let mut lineage_undo = Vec::new();
    let mut lineage_mappings: HashMap<Vec<u8>, Option<Vec<u8>>> = HashMap::new();
    for observation in lineage {
        let mapping_key =
            keys::encode_script_reference_key(1, &observation.reference_hash).to_vec();
        let previous_mapping = match lineage_mappings.get(&observation.reference_hash) {
            Some(value) => value.clone(),
            None => store.get_script_reference_version_hash(1, &observation.reference_hash)?,
        };
        lineage_undo.push((
            observation.block_number,
            ckbadger_store::CF_SCRIPT_REFERENCE_TO_VERSION,
            mapping_key,
            previous_mapping,
        ));
        lineage_mappings.insert(
            observation.reference_hash.clone(),
            observation.version_hash.clone(),
        );

        let Some(version_hash) = &observation.version_hash else {
            continue;
        };
        let previous = version_map
            .get(version_hash)
            .map(bincode::serialize)
            .transpose()?;
        lineage_undo.push((
            observation.block_number,
            ckbadger_store::CF_SCRIPT_VERSIONS,
            version_hash.clone(),
            previous,
        ));
        let previous_family = version_map
            .get(version_hash)
            .and_then(|info| info.family_id.clone());
        apply_type_id_lineage(&mut version_map, &templates, observation)?;
        let updated = &version_map[version_hash];
        if updated.family_id != previous_family {
            if let Some(family) = &updated.family_id {
                let key = keys::encode_script_version_by_family_key(family, version_hash);
                lineage_undo.push((
                    observation.block_number,
                    ckbadger_store::CF_SCRIPT_VERSIONS_BY_FAMILY,
                    key.clone(),
                    store.get_cf(store.cf_script_versions_by_family(), &key)?,
                ));
            }
            if let Some(name) = &updated.name {
                let key = keys::encode_script_version_by_label_key(name, version_hash);
                lineage_undo.push((
                    observation.block_number,
                    ckbadger_store::CF_SCRIPT_VERSIONS_BY_LABEL,
                    key.clone(),
                    store.get_cf(store.cf_script_versions_by_label(), &key)?,
                ));
            }
        }
    }

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
        if *hash_type == 1 {
            if let Some(template) = templates.get(reference_hash) {
                let family = version_map
                    .get(version_hash)
                    .and_then(|version| version.family_id.as_ref());
                if family != template.family_id.as_ref() {
                    bail!("registered type-id resolved outside its observed binary family: reference=0x{} binary=0x{} family={:?}", hex::encode(reference_hash), hex::encode(version_hash), template.family_id);
                }
            }
        }
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

    let mut family_map: HashMap<String, ScriptFamilyInfo> = existing_families.clone();
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
    }

    // Family membership is stable provenance; a current binary mapping may be
    // absent after retirement. Sum each canonical reference form once instead
    // of summing version counters, which describe current binary resolution.
    let membership = ckbadger_store::ScriptFamilyMembershipIndex::new(version_map.values())?;
    let mut family_usage: HashMap<String, ckbadger_store::ScriptFamilyUsage> = HashMap::new();
    for ((reference, hash_type), current) in &reference_mappings {
        if let Some(family) =
            membership.resolve_family(reference, *hash_type, current.as_deref())?
        {
            let info = reference_info_map
                .get(&(reference.clone(), *hash_type))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing family reference info: family={} reference=0x{} hash_type={}",
                        family,
                        hex::encode(reference),
                        hash_type
                    )
                })?;
            family_usage
                .entry(family.to_owned())
                .or_default()
                .add_reference(info)?;
        }
    }
    for (family_id, usage) in family_usage {
        let family = family_map.get_mut(&family_id).ok_or_else(|| {
            anyhow::anyhow!("missing family metadata for observed membership: family={family_id}")
        })?;
        usage.apply_to(family);
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
        existing_versions,
        existing_families,
        lineage_undo,
    })
}

pub(crate) fn collect_current_script_reference_rollup_state(
    store: &ckbadger_store::CkbadgerStore,
) -> Result<ScriptReferenceRollupState> {
    // Canonical bindings are maintained by chain transitions and restored by
    // chronological undo. Rebuilding usage must never reinterpret a retired
    // Type ID's current non-executable payload as a new binary version.
    let all_infos = store.list_script_reference_infos()?;
    let reference_mappings = all_infos.iter().map(|((reference_hash, hash_type), _)| {
        let version_hash = match hash_type {
            0 | 2 | 4 => Some(reference_hash.clone()),
            1 => store.get_script_reference_version_hash(*hash_type, reference_hash)?,
            _ => bail!("unsupported script reference hash_type while rebuilding rollups: reference_hash=0x{}, hash_type={}", hex::encode(reference_hash), hash_type),
        };
        Ok(((reference_hash.clone(), *hash_type), version_hash))
    }).collect::<Result<Vec<_>>>()?;
    build_script_reference_rollup_state(store, reference_mappings, all_infos.into_iter().collect())
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
        for (((reference_hash, hash_type), key), res) in
            references.iter().zip(encoded_keys.iter()).zip(results)
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
        let rollups = collect_current_script_reference_rollup_state(self.store.as_ref())?;

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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn materialize_script_versions_and_families_with_chain(
        &self,
        updated_references: &HashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
        txs: &[crate::sync::types::TxData],
        input_cells: &HashMap<(Vec<u8>, i16), ckbadger_store::PositionedCellInfo>,
        batch_cells: &HashMap<(Vec<u8>, i16), ckbadger_store::PositionedCellInfo>,
        undo_seq: &crate::sync::undo::SharedUndoSeq,
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

        let mut lineage = Vec::new();
        let mut chain_mappings: HashMap<Vec<u8>, Option<Vec<u8>>> = HashMap::new();
        for tx in txs {
            if !tx.is_cellbase {
                for input in &tx.inputs {
                    let index = i16::try_from(input.previous_output_index).map_err(|_| anyhow::anyhow!(
                        "type-id input index exceeds storage range: block={} tx=0x{} input=0x{}:{}",
                        tx.block_number, hex::encode(tx.hash), hex::encode(input.previous_tx_hash), input.previous_output_index))?;
                    let key = (input.previous_tx_hash.to_vec(), index);
                    let cell = batch_cells.get(&key).or_else(|| input_cells.get(&key)).ok_or_else(|| anyhow::anyhow!(
                        "missing prefetched input for script lineage: block={} tx=0x{} input=0x{}:{}",
                        tx.block_number, hex::encode(tx.hash), hex::encode(input.previous_tx_hash), index))?;
                    if !cell
                        .type_code_hash
                        .as_deref()
                        .zip(cell.type_hash_type)
                        .is_some_and(|(hash, kind)| {
                            kind == 1 && crate::parser::script::is_type_id(hash, 1)
                        })
                    {
                        continue;
                    }
                    let reference = cell.type_script_hash.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "type-id input lacks script hash: block={} tx=0x{}",
                            tx.block_number,
                            hex::encode(tx.hash)
                        )
                    })?;
                    let current = match chain_mappings.get(reference) {
                        Some(value) => value.clone(),
                        None => self.store.get_script_reference_version_hash(1, reference)?,
                    };
                    if current.is_some() && cell.data_hash.is_none() {
                        bail!("active type-id input lacks data hash: reference=0x{} block={} tx=0x{} input=0x{}:{}", hex::encode(reference), tx.block_number, hex::encode(tx.hash), hex::encode(input.previous_tx_hash), index);
                    }
                    if current.is_some() && current == cell.data_hash {
                        lineage.push(TypeIdCodeChange {
                            reference_hash: reference.clone(),
                            version_hash: None,
                            block_number: tx.block_number,
                        });
                        chain_mappings.insert(reference.clone(), None);
                    }
                }
            }
            for cell in &tx.cells {
                if !cell
                    .type_code_hash
                    .as_deref()
                    .zip(cell.type_hash_type)
                    .is_some_and(|(hash, kind)| crate::parser::script::is_type_id(hash, kind))
                    || !cell.data.starts_with(b"\x7fELF")
                {
                    continue;
                }
                let reference = cell.type_script_hash.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "type-id output lacks script hash: block={} tx=0x{}",
                        tx.block_number,
                        hex::encode(tx.hash)
                    )
                })?;
                lineage.push(TypeIdCodeChange {
                    reference_hash: reference.clone(),
                    version_hash: Some(cell.data_hash.to_vec()),
                    block_number: tx.block_number,
                });
                chain_mappings.insert(reference.clone(), Some(cell.data_hash.to_vec()));
            }
        }
        let reference_mappings: Vec<((Vec<u8>, u8), Option<Vec<u8>>)> = reference_info_map
            .keys()
            .map(|(reference_hash, hash_type)| {
                let version_hash = if *hash_type == 1 && chain_mappings.contains_key(reference_hash)
                {
                    chain_mappings.get(reference_hash).cloned().flatten()
                } else {
                    self.store
                        .get_script_reference_version_hash(*hash_type, reference_hash)?
                };
                Ok(((reference_hash.clone(), *hash_type), version_hash))
            })
            .collect::<Result<Vec<_>>>()?;

        let rollups = build_script_reference_rollup_state_with_lineage(
            self.store.as_ref(),
            reference_mappings,
            reference_info_map,
            &lineage,
        )?;

        for (block, cf, key, previous) in &rollups.lineage_undo {
            self.record_object_undo(batch, *block, cf, key, previous.clone(), undo_seq);
        }
        for (reference, hash) in &chain_mappings {
            if let Some(hash) = hash {
                batch.put_script_reference_to_version(1, reference, hash);
            } else {
                batch.delete_script_reference_to_version(1, reference);
            }
        }
        for (version_hash, info) in rollups.versions {
            if rollups.existing_versions.get(&version_hash) == Some(&info) {
                continue;
            }
            batch.put_script_version(&version_hash, &info);
            let previous = rollups.existing_versions.get(&version_hash);
            if let Some(family) = &info.family_id {
                if previous.and_then(|info| info.family_id.as_ref()) != Some(family) {
                    batch.put_script_version_by_family(family, &version_hash);
                }
            }
            if let Some(name) = &info.name {
                if previous.and_then(|info| info.name.as_ref()) != Some(name) {
                    batch.put_script_version_by_label(name, &version_hash);
                }
            }
        }
        for (family_id, info) in rollups.families {
            if rollups.existing_families.get(&family_id) == Some(&info) {
                continue;
            }
            batch.put_script_family(&family_id, &info);
        }

        Ok(())
    }

    /// Per-block script daily capacity/knowledge deltas.
    ///
    /// Every mutation goes through the batch overlay, which records the value
    /// as of the end of the previous block the first time a given block touches
    /// a given key. That undo entry is the ONLY thing that restores these rows
    /// after a shallow fork — `should_delete_stats_for_replay` no longer deletes
    /// them (Task 2.4).
    pub fn update_script_daily_deltas_batch(
        &self,
        changes: &EntityDailyChanges<ScriptDailyKey>,
        overlay: &mut EntityStatsOverlay,
        undo_seq: &mut HashMap<i64, u64>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        // One `multi_get` for every key this family touches, instead of a point
        // read per key inside the loop. The overlay keeps whatever a previous
        // writer already computed, so warming it can never resurrect a stale
        // pre-batch value (`prefetch_never_overwrites_a_value_the_batch_computed`).
        let mut prefetch_keys: Vec<Vec<u8>> = Vec::new();
        for (_, map) in changes.by_block() {
            for (key_parts, (capacity_delta, knowledge_delta)) in map {
                if *capacity_delta == 0 && *knowledge_delta == 0 {
                    continue;
                }
                let (code_hash, hash_type, is_type, date_yyyymmdd) = key_parts;
                prefetch_keys.push(
                    keys::encode_script_daily_key(code_hash, *hash_type, *is_type, *date_yyyymmdd)
                        .to_vec(),
                );
            }
        }
        prefetch_keys.sort_unstable();
        prefetch_keys.dedup();
        overlay.prefetch(self.store.as_ref(), &prefetch_keys)?;

        for (block, map) in changes.by_block() {
            for (key_parts, (capacity_delta, knowledge_delta)) in map {
                if *capacity_delta == 0 && *knowledge_delta == 0 {
                    continue;
                }
                let (code_hash, hash_type, is_type, date_yyyymmdd) = key_parts;
                let key =
                    keys::encode_script_daily_key(code_hash, *hash_type, *is_type, *date_yyyymmdd)
                        .to_vec();
                let ctx = || {
                    format!(
                        "script_daily code_hash=0x{} hash_type={} is_type={} date={}",
                        hex::encode(code_hash),
                        hash_type,
                        is_type,
                        date_yyyymmdd
                    )
                };
                let prev = overlay.current(self.store.as_ref(), &key)?;
                let next =
                    apply_daily_pair(prev.as_deref(), *capacity_delta, *knowledge_delta, &ctx)?;
                overlay.mutate(self.store.as_ref(), batch, undo_seq, *block, &key, next)?;
            }
        }
        Ok(())
    }

    /// Test-only adapter: run a flat `(entity, delta)` map through the real
    /// per-block writer as a single block, then stage the overlay.
    ///
    /// The value semantics these callers assert (accumulate, delete on net
    /// zero, fail on a corrupt existing row) are unchanged by Task 2.3; the
    /// per-block undo semantics are covered by `entity_stats`' own unit tests
    /// and by `tests/entity_stats_rollback.rs`.
    #[cfg(test)]
    pub(crate) fn update_script_daily_deltas_batch_flat_for_test(
        &self,
        changes: &HashMap<(Vec<u8>, u8, bool, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut by_block = EntityDailyChanges::new();
        for (key, (capacity_delta, knowledge_delta)) in changes {
            by_block.add(1, key.clone(), *capacity_delta, *knowledge_delta)?;
        }
        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq = HashMap::new();
        self.update_script_daily_deltas_batch(&by_block, &mut overlay, &mut undo_seq, batch)?;
        overlay.stage_final(batch)
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
        first.insert((code_hash.clone(), 1u8, false, date), (100i128, 60i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch_flat_for_test(&first, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut second = HashMap::new();
        second.insert((code_hash.clone(), 1u8, false, date), (-20i128, -10i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch_flat_for_test(&second, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_script_daily_delta(&code_hash, 1, false, date)
            .unwrap()
            .unwrap();
        assert_eq!(delta.owned_capacity_delta, 80);
        assert_eq!(delta.owned_knowledge_delta, 50);

        let mut third = HashMap::new();
        third.insert((code_hash.clone(), 1u8, false, date), (-80i128, -50i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch_flat_for_test(&third, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_script_daily_delta(&code_hash, 1, false, date)
            .unwrap();
        assert!(delta.is_none());
    }

    #[test]
    fn test_update_script_daily_deltas_batch_keeps_hash_type_forms_separate() {
        // Regression (B8 root cause): deltas of two hash_type forms sharing
        // the same code_hash bytes must land in independent rows.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let code_hash = vec![0x9B; 32];
        let date = 20240115u32;

        let mut changes = HashMap::new();
        changes.insert((code_hash.clone(), 1u8, false, date), (500i128, 300i128));
        changes.insert((code_hash.clone(), 0u8, false, date), (98i128, 98i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_script_daily_deltas_batch_flat_for_test(&changes, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let type_form = store
            .get_script_daily_delta(&code_hash, 1, false, date)
            .unwrap()
            .unwrap();
        assert_eq!(type_form.owned_capacity_delta, 500);
        assert_eq!(type_form.owned_knowledge_delta, 300);

        let data_form = store
            .get_script_daily_delta(&code_hash, 0, false, date)
            .unwrap()
            .unwrap();
        assert_eq!(data_form.owned_capacity_delta, 98);
        assert_eq!(data_form.owned_knowledge_delta, 98);
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
    fn persisted_mainnet_mnft_lineage_survives_bundled_reimport() {
        assert_persisted_mainnet_lineage_survives_bundled_reimport(
            "m-nft-issuer",
            "b59879b6ea6fff985223117fa499ce84f8cfb028c4ffdfdf5d3ec19e905a11ed",
        );
    }

    #[test]
    fn persisted_mainnet_joyid_lineage_survives_bundled_reimport() {
        assert_persisted_mainnet_lineage_survives_bundled_reimport(
            "joyid",
            "d23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
        );
    }

    fn script_startup_rows(
        store: &CkbadgerStore,
    ) -> std::collections::BTreeMap<(String, Vec<u8>), Vec<u8>> {
        [
            ckbadger_store::CF_SCRIPT_VERSIONS,
            ckbadger_store::CF_SCRIPT_VERSIONS_BY_FAMILY,
            ckbadger_store::CF_SCRIPT_VERSIONS_BY_LABEL,
            ckbadger_store::CF_SCRIPT_FAMILIES,
            ckbadger_store::CF_SCRIPT_REFERENCE_INFO,
            ckbadger_store::CF_SCRIPT_REFERENCE_TO_VERSION,
        ]
        .into_iter()
        .flat_map(|cf| {
            store
                .iterator_cf(store.cf(cf), rocksdb::IteratorMode::Start)
                .map(move |row| {
                    let (key, value) = row.unwrap();
                    ((cf.to_owned(), key.to_vec()), value.to_vec())
                })
        })
        .collect()
    }

    fn assert_persisted_mainnet_lineage_survives_bundled_reimport(
        family: &str,
        foreign_reference_hex: &str,
    ) {
        // These compact cases were verified read-only with get_block_by_number
        // on 2026-10-27: native Type ID script hashes and ELF data hashes were
        // recomputed from the node's actual bytes. Keep block/tx/outpoint/type
        // provenance in the fixture; do not fabricate ELF bytes for these hashes.
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/native_typeid_lineage_mainnet_startup_cases.json"
        )))
        .unwrap();
        let case = cases.iter().find(|case| case["family"] == family).unwrap();
        let decode = |field: &str| {
            hex::decode(case[field].as_str().unwrap().trim_start_matches("0x")).unwrap()
        };
        let reference = decode("reference_hash");
        let binary = decode("binary_hash");
        let block = case["block_number"].as_i64().unwrap();
        let script: crate::rpc::Script =
            serde_json::from_value(case["type_script"].clone()).unwrap();
        assert_eq!(
            crate::parser::ScriptParser::compute_script_hash(&script),
            reference
        );
        let foreign_reference = hex::decode(foreign_reference_hex).unwrap();

        let tuples: Vec<(String, String, i64)> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/native_typeid_lineage_mainnet_0_12m.json"
        )))
        .unwrap();
        assert_eq!(tuples.len(), 471);
        let mut observations: Vec<_> = tuples
            .into_iter()
            .map(|(reference, binary, block_number)| TypeIdCodeChange {
                reference_hash: hex::decode(reference).unwrap(),
                version_hash: Some(hex::decode(binary).unwrap()),
                block_number,
            })
            .collect();
        let joyid = cases.iter().find(|case| case["family"] == "joyid").unwrap();
        observations.push(TypeIdCodeChange {
            reference_hash: hex::decode(
                joyid["reference_hash"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("0x"),
            )
            .unwrap(),
            version_hash: Some(
                hex::decode(
                    joyid["binary_hash"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("0x"),
                )
                .unwrap(),
            ),
            block_number: joyid["block_number"].as_i64().unwrap(),
        });
        assert!(observations.iter().any(|observation| {
            observation.reference_hash == reference
                && observation.version_hash.as_ref() == Some(&binary)
                && observation.block_number == block
        }));
        let current_binary = observations
            .iter()
            .rev()
            .find(|observation| observation.reference_hash == reference)
            .unwrap()
            .version_hash
            .clone()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append");
        let payload_key = keys::encode_outpoint(&[0x76; 32], 0);
        let (expected_rows, expected_version, expected_payload) = {
            let store = Arc::new(CkbadgerStore::open_domain(&domain_path).unwrap());
            let append = Arc::new(CkbadgerStore::open_append_only(&append_path).unwrap());
            let imported =
                crate::label_import::run_label_import_bundled(&store, "mainnet").unwrap();
            assert!(imported.errors.is_empty(), "{:?}", imported.errors);
            assert!(store.get_script_version(&binary).unwrap().is_none());
            let writer = BatchWriter::new(store.clone(), append.clone());

            // Synthetic nonzero usage exercises every rollup field, independently
            // of the exact chain deployment observations. Only domain is mutable.
            let changes = HashMap::from([
                ((reference.clone(), 1, false), (9, 7, 900, 700, 549, 427)),
                ((reference.clone(), 1, true), (5, 3, 500, 300, 305, 183)),
            ]);
            let mut batch = StoreBatch::new(&store);
            let infos = writer
                .update_script_reference_usage_batch(&changes, &mut batch)
                .unwrap();
            let rollup = build_script_reference_rollup_state_with_lineage(
                &store,
                vec![((reference.clone(), 1), Some(current_binary.clone()))],
                infos,
                &observations,
            )
            .unwrap();
            // Persist the shared rollup exactly as bulk final materialization:
            // version/family/index rows, and only the observed current binding.
            // There is no undo on the bulk path and no chain replay on reopen.
            for ((reference, kind), binary) in rollup.reference_mappings {
                batch.put_script_reference_to_version(kind, &reference, binary.as_ref().unwrap());
            }
            for (hash, version) in rollup.versions {
                batch.put_script_version(&hash, &version);
                if let Some(family) = &version.family_id {
                    batch.put_script_version_by_family(family, &hash);
                }
                if let Some(name) = &version.name {
                    batch.put_script_version_by_label(name, &hash);
                }
            }
            for (family, info) in rollup.families {
                batch.put_script_family(&family, &info);
            }
            batch.commit().unwrap();
            writer.refresh_script_reference_rollups().unwrap();
            let version = store.get_script_version(&binary).unwrap().unwrap();
            assert_eq!(version.family_id.as_deref(), Some(family));
            assert_eq!(version.canonical_reference_hash, Some(reference.clone()));
            assert_eq!(version.canonical_hash_type, Some(1));
            assert_eq!(
                version.type_id_deployments,
                vec![(reference.clone(), block)]
            );
            assert!(version.registered_references.is_empty());
            let info = store.get_script_family(family).unwrap().unwrap();
            assert_eq!((info.cells_count, info.live_cells_count), (14, 10));
            assert_eq!(
                (info.owned_capacity_sum, info.owned_knowledge_sum),
                (1000, 610)
            );

            // An unrelated immutable payload guards the store boundary. The
            // metadata import and rollup must never update/delete append-only CF_CELLS.
            let mut payload = StoreBatch::new(&append);
            payload.put_cell_payload(
                &payload_key,
                &ckbadger_store::LiveCellInfo {
                    capacity: 10000,
                    lock_script_hash: vec![0x11; 32],
                    lock_code_hash: vec![0x22; 32],
                    lock_hash_type: 1,
                    lock_args: vec![],
                    type_script_hash: None,
                    type_code_hash: None,
                    type_hash_type: None,
                    type_args: None,
                    data_size: 0,
                    occupied_capacity: 41,
                    udt_amount: None,
                    data_hash: None,
                },
            );
            payload.commit().unwrap();
            (
                script_startup_rows(&store),
                version,
                append
                    .get_cf(append.cf_cells(), &payload_key)
                    .unwrap()
                    .unwrap(),
            )
        }; // All writer/store handles drop here: no in-memory lineage survives.

        for startup in 1..=2 {
            let store = Arc::new(CkbadgerStore::open_domain(&domain_path).unwrap());
            let append = Arc::new(CkbadgerStore::open_append_only(&append_path).unwrap());
            assert_eq!(
                script_startup_rows(&store),
                expected_rows,
                "before startup {startup}"
            );
            let imported =
                crate::label_import::run_label_import_bundled(&store, "mainnet").unwrap();
            assert!(imported.errors.is_empty(), "{:?}", imported.errors);
            // Capture importer evidence before startup refresh can mask damage.
            let imported_version = store.get_script_version(&binary).unwrap().unwrap();
            let family_key = keys::encode_script_version_by_family_key(family, &binary);
            let label_key = keys::encode_script_version_by_label_key(
                expected_version.name.as_deref().unwrap(),
                &binary,
            );
            let family_index = store
                .get_cf(store.cf_script_versions_by_family(), &family_key)
                .unwrap();
            let label_index = store
                .get_cf(store.cf_script_versions_by_label(), &label_key)
                .unwrap();
            let writer = BatchWriter::new(store.clone(), append.clone());
            // This is the actual unconditional startup entry point in writer/sync.rs.
            let refreshed = writer.refresh_script_reference_rollups();
            assert_eq!(
                imported_version, expected_version,
                "{family} lost persisted lineage metadata during startup {startup}; refresh={refreshed:?}"
            );
            assert_eq!(
                family_index,
                Some(vec![]),
                "{family} family index after import"
            );
            assert_eq!(
                label_index,
                Some(vec![]),
                "{family} label index after import"
            );
            refreshed.unwrap();
            assert_eq!(
                script_startup_rows(&store),
                expected_rows,
                "after startup {startup}"
            );
            let versions = store.list_script_versions().unwrap();
            assert!(
                versions.iter().all(|(_, version)| !version
                    .registered_references
                    .iter()
                    .any(|(anchor, _)| anchor == &foreign_reference)),
                "{family} imported a testnet anchor on mainnet"
            );
            let membership = ckbadger_store::ScriptFamilyMembershipIndex::new(
                versions.iter().map(|(_, version)| version),
            )
            .unwrap();
            assert_eq!(
                membership
                    .resolve_family(&reference, 1, Some(&current_binary))
                    .unwrap(),
                Some(family)
            );
            assert_eq!(
                append.get_cf(append.cf_cells(), &payload_key).unwrap(),
                Some(expected_payload.clone())
            );
        }
    }

    #[test]
    fn real_mainnet_type_id_deployments_preserve_imported_family_boundaries() {
        // Exact native Type ID ELF outputs at heights 0..=12,000,000, collected
        // read-only from the node. Each tuple is (type hash, binary hash, block).
        let tuples: Vec<(String, String, i64)> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/native_typeid_lineage_mainnet_0_12m.json"
        )))
        .unwrap();
        assert_eq!(tuples.len(), 471);
        let observations: Vec<_> = tuples
            .into_iter()
            .map(|(reference, binary, block)| TypeIdCodeChange {
                reference_hash: hex::decode(reference).unwrap(),
                version_hash: Some(hex::decode(binary).unwrap()),
                block_number: block,
            })
            .collect();
        assert!(observations
            .windows(2)
            .all(|pair| pair[0].block_number <= pair[1].block_number));
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let result = crate::label_import::run_label_import_bundled(&store, "mainnet").unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let imported: HashMap<_, _> = store.list_script_versions().unwrap().into_iter().collect();
        let rollup = build_script_reference_rollup_state_with_lineage(
            &store,
            vec![],
            HashMap::new(),
            &observations,
        )
        .unwrap();
        let versions: HashMap<_, _> = rollup.versions.into_iter().collect();
        let membership =
            ckbadger_store::ScriptFamilyMembershipIndex::new(versions.values()).unwrap();
        let copy = hex::decode("e305abb02155db4393497ac45377808b7210bee5a0fe9b28f3e793d35714fec2")
            .unwrap();
        assert!(versions.values().all(|version| !version
            .registered_references
            .iter()
            .any(|(reference, _)| reference == &copy)));
        let mut copied_families = std::collections::BTreeSet::new();
        let mut copy_observations = 0;
        for observation in &observations {
            let hash = observation.version_hash.as_ref().unwrap();
            let version = &versions[hash];
            assert!(version
                .type_id_deployments
                .contains(&(observation.reference_hash.clone(), observation.block_number)));
            if observation.reference_hash == copy {
                copy_observations += 1;
                assert_eq!(
                    membership.resolve_family(&copy, 1, Some(hash)).unwrap(),
                    version.family_id.as_deref()
                );
                if let Some(family) = &version.family_id {
                    copied_families.insert(family.as_str());
                }
            }
        }
        assert_eq!(copy_observations, 6);
        assert_eq!(
            copied_families,
            std::collections::BTreeSet::from(["bit-account", "bit-lock"])
        );
        assert_eq!(
            membership.resolve_family(&copy, 1, None).unwrap(),
            None,
            "unregistered copy has no permanent family after retirement"
        );
        for (hash, version) in &versions {
            match imported.get(hash) {
                Some(seed) => {
                    assert_eq!(version.registered_references, seed.registered_references);
                    assert_eq!(
                        version.canonical_reference_hash,
                        seed.canonical_reference_hash
                    );
                    assert_eq!(version.canonical_hash_type, seed.canonical_hash_type);
                }
                None => assert!(
                    version.registered_references.is_empty(),
                    "chain discovery must never invent registration"
                ),
            }
        }
    }

    #[test]
    fn unregistered_type_id_copy_preserves_imported_canonical_reference() {
        let version_hash = vec![0x41; 32];
        let registered = vec![0x51; 32];
        let copy = vec![0xe3; 32];
        let original = ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some("family".into()),
            registered_references: vec![(registered.clone(), 1)],
            canonical_reference_hash: Some(registered.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        };
        let mut versions = HashMap::from([(version_hash.clone(), original)]);
        let templates = super::type_id_family_templates(&versions).unwrap();
        super::apply_type_id_lineage(
            &mut versions,
            &templates,
            &super::TypeIdCodeChange {
                reference_hash: copy.clone(),
                version_hash: Some(version_hash.clone()),
                block_number: 4_872_142,
            },
        )
        .unwrap();
        let result = &versions[&version_hash];
        assert_eq!(result.canonical_reference_hash, Some(registered.clone()));
        assert_eq!(result.registered_references, vec![(registered, 1)]);
        assert_eq!(result.type_id_deployments, vec![(copy.clone(), 4_872_142)]);
        assert!(!super::type_id_family_templates(&versions)
            .unwrap()
            .contains_key(&copy));
    }

    #[test]
    fn live_type_id_upgrade_adds_family_version_inline_and_undo_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path().join("domain")).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(dir.path().join("append")).unwrap());
        let writer = BatchWriter::new(store.clone(), append.clone());
        let reference = vec![0x71; 32];
        let old_hash = vec![0x81; 32];
        store
            .put_script_version(
                &old_hash,
                &ScriptVersionInfo {
                    version_hash: old_hash.clone(),
                    family_id: Some("cells-account".into()),
                    name: Some("Cells Account".into()),
                    canonical_reference_hash: Some(reference.clone()),
                    registered_references: vec![(reference.clone(), 1)],
                    canonical_hash_type: Some(1),
                    type_id_deployments: vec![(reference.clone(), 10)],
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_reference_to_version_direct(1, &reference, &old_hash)
            .unwrap();
        let mut seed = StoreBatch::new(&store);
        seed.put_script_reference_info(
            1,
            &reference,
            &ScriptReferenceInfo {
                reference_hash: reference.clone(),
                hash_type: 1,
                lock_live_cells_count: 7,
                lock_cells_count: 9,
                type_live_cells_count: 72,
                type_cells_count: 245,
                ..Default::default()
            },
        );
        seed.commit().unwrap();
        let data = b"\x7fELFupgrade".to_vec();
        let new_hash = crate::parser::ScriptParser::compute_data_hash(&data);
        let cell = crate::parser::cell::ParsedCell {
            capacity: 10000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            lock_script_hash: vec![0x22; 32],
            type_code_hash: Some(hex::decode(crate::parser::script::TYPE_ID_CODE_HASH).unwrap()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x33; 32]),
            type_script_hash: Some(reference.clone()),
            data_hash: new_hash,
            data_size: data.len() as i32,
            data,
        };
        let tx = crate::sync::types::TxData {
            hash: [0x44; 32],
            block_number: 20,
            tx_index: 1,
            inputs_count: 0,
            outputs_count: 1,
            is_cellbase: false,
            inputs: vec![],
            cells: vec![cell],
            witnesses: vec![],
            total_input_capacity: 0,
            total_output_capacity: 10000,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            semantic_tags: 0,
        };
        let mut batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                std::slice::from_ref(&tx),
                &HashMap::new(),
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();
        let version = store.get_script_version(&new_hash).unwrap().unwrap();
        assert_eq!(version.family_id.as_deref(), Some("cells-account"));
        assert_eq!(version.type_id_deployments, vec![(reference.clone(), 20)]);
        assert_eq!(version.type_live_cells_count, 72);
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .versions_count,
            2
        );
        assert!(store
            .list_script_version_hashes_by_family("cells-account")
            .unwrap()
            .contains(&new_hash.to_vec()));
        let input_cell = ckbadger_store::PositionedCellInfo::new(
            ckbadger_store::LiveCellInfo {
                capacity: 10000,
                lock_script_hash: vec![0x22; 32],
                lock_code_hash: vec![0x11; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference.clone()),
                type_code_hash: tx.cells[0].type_code_hash.clone(),
                type_hash_type: Some(1),
                type_args: Some(vec![0x33; 32]),
                data_size: tx.cells[0].data_size,
                occupied_capacity: 0,
                udt_amount: None,
                data_hash: Some(new_hash.to_vec()),
            },
            20,
        );
        let payload_key = keys::encode_outpoint(&tx.hash, 0);
        let payload_before = input_cell.cell.clone();
        let mut payload_batch = StoreBatch::new(&append);
        payload_batch.put_cell_payload(&payload_key, &payload_before);
        payload_batch.commit().unwrap();
        let mut input_cells = HashMap::from([((tx.hash.to_vec(), 0), input_cell.clone())]);
        let mut retired = tx;
        retired.inputs = vec![crate::parser::transaction::ParsedInput {
            previous_tx_hash: retired.hash,
            previous_output_index: 0,
            since: 0,
        }];
        retired.inputs_count = 1;
        retired.hash = [0x45; 32];
        retired.block_number = 21;
        retired.cells[0].data = vec![0; 31_560];
        retired.cells[0].data_hash =
            crate::parser::ScriptParser::compute_data_hash(&retired.cells[0].data);
        retired.cells[0].data_size = 31_560;
        // Consuming a different payload must never retire the active binding.
        let mut inactive = input_cells.clone();
        inactive
            .get_mut(&(vec![0x44; 32], 0))
            .unwrap()
            .cell
            .data_hash = Some(vec![0xee; 32]);
        let mut inactive_batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                std::slice::from_ref(&retired),
                &inactive,
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut inactive_batch,
            )
            .unwrap();
        inactive_batch.commit().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(new_hash.to_vec())
        );
        let mut batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                std::slice::from_ref(&retired),
                &input_cells,
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            None
        );
        assert!(store.get_script_version(&new_hash).unwrap().is_some());
        writer.refresh_script_reference_rollups().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            None
        );
        let retired_info = store.get_script_version(&new_hash).unwrap().unwrap();
        assert_eq!(retired_info.lock_live_cells_count, 0);
        assert_eq!(retired_info.type_live_cells_count, 0);
        assert_eq!(
            retired_info.type_id_deployments,
            vec![(reference.clone(), 20)]
        );
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .versions_count,
            2
        );
        let family = store.get_script_family("cells-account").unwrap().unwrap();
        assert_eq!(
            family.live_cells_count, 79,
            "retirement cannot erase still-live family users"
        );
        assert_eq!(
            family.cells_count, 254,
            "retirement cannot erase cumulative family users"
        );
        let non_elf_hash = retired.cells[0].data_hash;
        let mut non_elf_input = input_cell.clone();
        non_elf_input.cell.data_hash = Some(non_elf_hash.to_vec());
        non_elf_input.cell.data_size = 31_560;
        input_cells.insert((retired.hash.to_vec(), 0), non_elf_input);
        let previous_hash = retired.hash;
        retired.hash = [0x46; 32];
        retired.block_number = 22;
        retired.inputs[0].previous_tx_hash = previous_hash;
        retired.cells[0].data = b"\x7fELFreactivated".to_vec();
        retired.cells[0].data_size = retired.cells[0].data.len() as i32;
        let reactivated_hash =
            crate::parser::ScriptParser::compute_data_hash(&retired.cells[0].data);
        retired.cells[0].data_hash = reactivated_hash;
        let mut batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                std::slice::from_ref(&retired),
                &input_cells,
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(reactivated_hash.to_vec())
        );
        let reactivated = store
            .get_script_version(&reactivated_hash)
            .unwrap()
            .unwrap();
        assert_eq!(reactivated.family_id.as_deref(), Some("cells-account"));
        assert_eq!(
            reactivated.type_id_deployments,
            vec![(reference.clone(), 22)]
        );
        assert_eq!(
            (
                reactivated.lock_live_cells_count,
                reactivated.type_live_cells_count
            ),
            (7, 72)
        );
        assert!(store.get_script_version(&non_elf_hash).unwrap().is_none());
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .versions_count,
            3
        );
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .live_cells_count,
            79
        );
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .cells_count,
            254
        );
        let mut active_input = input_cell;
        active_input.cell.data_hash = Some(reactivated_hash.to_vec());
        // Exercise the same-batch prefetched input map too.
        let batch_cells = HashMap::from([((retired.hash.to_vec(), 0), active_input)]);
        let previous_hash = retired.hash;
        retired.hash = [0x47; 32];
        retired.block_number = 23;
        retired.inputs[0].previous_tx_hash = previous_hash;
        retired.cells.clear();
        retired.outputs_count = 0;
        let mut batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                std::slice::from_ref(&retired),
                &HashMap::new(),
                &batch_cells,
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .versions_count,
            3
        );
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .live_cells_count,
            79
        );
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .cells_count,
            254
        );
        store.rollback_via_undo_log(&append, 22).unwrap();
        writer.refresh_script_reference_rollups().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(reactivated_hash.to_vec())
        );
        assert_eq!(
            store
                .get_script_version(&reactivated_hash)
                .unwrap()
                .unwrap()
                .type_live_cells_count,
            72
        );
        store.rollback_via_undo_log(&append, 21).unwrap();
        writer.refresh_script_reference_rollups().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            None
        );
        assert!(store
            .get_script_version(&reactivated_hash)
            .unwrap()
            .is_none());
        assert!(!store
            .list_script_version_hashes_by_family("cells-account")
            .unwrap()
            .contains(&reactivated_hash.to_vec()));
        assert_eq!(
            store
                .get_script_family("cells-account")
                .unwrap()
                .unwrap()
                .versions_count,
            2
        );
        store.rollback_via_undo_log(&append, 20).unwrap();
        writer.refresh_script_reference_rollups().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(new_hash.to_vec())
        );
        store.rollback_via_undo_log(&append, 19).unwrap();
        writer.refresh_script_reference_rollups().unwrap();
        assert!(store.get_script_version(&new_hash).unwrap().is_none());
        assert!(!store
            .list_script_version_hashes_by_family("cells-account")
            .unwrap()
            .contains(&new_hash.to_vec()));
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(old_hash)
        );
        assert_eq!(
            append
                .iterator_cf(append.cf_cells(), rocksdb::IteratorMode::Start)
                .count(),
            1
        );
        assert_eq!(
            append.get_cell_by_outpoint_key(&payload_key).unwrap(),
            Some(payload_before)
        );
    }

    #[test]
    fn live_type_id_changes_in_one_batch_undo_each_canonical_binding() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path().join("domain")).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(dir.path().join("append")).unwrap());
        let writer = BatchWriter::new(store.clone(), append.clone());
        let reference = vec![0x71; 32];
        let old_hash = vec![0x81; 32];
        let new_data = b"\x7fELFnew".to_vec();
        let new_hash = crate::parser::ScriptParser::compute_data_hash(&new_data);
        let mut seed = StoreBatch::new(&store);
        seed.put_script_version(
            &old_hash,
            &ScriptVersionInfo {
                version_hash: old_hash.clone(),
                canonical_reference_hash: Some(reference.clone()),
                canonical_hash_type: Some(1),
                family_id: Some("family".into()),
                name: Some("Family".into()),
                registered_references: vec![(reference.clone(), 1)],
                type_id_deployments: vec![(reference.clone(), 10)],
                ..Default::default()
            },
        );
        seed.put_script_reference_to_version(1, &reference, &old_hash);
        seed.put_script_reference_info(
            1,
            &reference,
            &ScriptReferenceInfo {
                reference_hash: reference.clone(),
                hash_type: 1,
                lock_cells_count: 3,
                lock_live_cells_count: 2,
                type_cells_count: 5,
                type_live_cells_count: 4,
                ..Default::default()
            },
        );
        seed.commit().unwrap();
        let mut previous = ckbadger_store::LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x12; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference.clone()),
            type_code_hash: Some(hex::decode(crate::parser::script::TYPE_ID_CODE_HASH).unwrap()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x33; 32]),
            data_size: 10,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: Some(old_hash.clone()),
        };
        let mut inputs = HashMap::new();
        let mut txs = Vec::new();
        for block in 21u8..=24 {
            inputs.insert(
                (vec![block - 1; 32], 0),
                ckbadger_store::PositionedCellInfo::new(previous.clone(), i64::from(block - 1)),
            );
            let data = if block % 2 == 0 {
                new_data.clone()
            } else {
                vec![0; 128]
            };
            let hash = crate::parser::ScriptParser::compute_data_hash(&data);
            let cell = crate::parser::cell::ParsedCell {
                capacity: 10000,
                lock_script_hash: previous.lock_script_hash.clone(),
                lock_code_hash: previous.lock_code_hash.clone(),
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference.clone()),
                type_code_hash: previous.type_code_hash.clone(),
                type_hash_type: Some(1),
                type_args: previous.type_args.clone(),
                data_hash: hash,
                data_size: data.len() as i32,
                data,
            };
            previous.data_hash = Some(hash.to_vec());
            previous.data_size = cell.data_size;
            txs.push(crate::sync::types::TxData {
                hash: [block; 32],
                block_number: i64::from(block),
                tx_index: 1,
                inputs_count: 1,
                outputs_count: 1,
                is_cellbase: false,
                inputs: vec![crate::parser::transaction::ParsedInput {
                    previous_tx_hash: [block - 1; 32],
                    previous_output_index: 0,
                    since: 0,
                }],
                cells: vec![cell],
                witnesses: vec![],
                total_input_capacity: 10000,
                total_output_capacity: 10000,
                fee: 0,
                tx_size: 0,
                cycles: None,
                timestamp: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
                semantic_tags: 0,
            });
        }
        let mut batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                &txs,
                &HashMap::new(),
                &inputs,
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(new_hash.to_vec())
        );
        assert_eq!(
            store
                .get_script_version(&new_hash)
                .unwrap()
                .unwrap()
                .type_id_deployments,
            vec![(reference.clone(), 22), (reference.clone(), 24)]
        );
        for (fork, expected) in [
            (23, None),
            (22, Some(new_hash.to_vec())),
            (21, None),
            (20, Some(old_hash.clone())),
        ] {
            store.rollback_via_undo_log(&append, fork).unwrap();
            writer.refresh_script_reference_rollups().unwrap();
            assert_eq!(
                store
                    .get_script_reference_version_hash(1, &reference)
                    .unwrap(),
                expected,
                "fork={fork}"
            );
            let family = store.get_script_family("family").unwrap().unwrap();
            assert_eq!(
                family.live_cells_count, 6,
                "retired/active family users at fork={fork}"
            );
            assert_eq!(
                family.cells_count, 8,
                "one reference form must count once at fork={fork}"
            );
            let new = store.get_script_version(&new_hash).unwrap();
            if fork >= 22 {
                let info = new.unwrap();
                assert_eq!(info.type_id_deployments, vec![(reference.clone(), 22)]);
                assert_eq!(info.family_id.as_deref(), Some("family"));
                assert_eq!(info.type_live_cells_count, if fork == 22 { 4 } else { 0 });
            } else {
                assert!(new.is_none());
                assert!(!store
                    .list_script_version_hashes_by_family("family")
                    .unwrap()
                    .contains(&new_hash.to_vec()));
            }
            assert_eq!(
                store
                    .get_script_version(&old_hash)
                    .unwrap()
                    .unwrap()
                    .type_id_deployments,
                vec![(reference.clone(), 10)]
            );
        }
        assert_eq!(
            append
                .iterator_cf(append.cf_cells(), rocksdb::IteratorMode::Start)
                .count(),
            0
        );
    }

    #[test]
    fn registered_type_reference_cannot_resolve_to_an_unbound_binary() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let reference = vec![0x71; 32];
        store
            .put_script_version(
                &[0x81; 32],
                &ScriptVersionInfo {
                    version_hash: vec![0x81; 32],
                    family_id: Some("cells-account".into()),
                    canonical_reference_hash: Some(reference.clone()),
                    registered_references: vec![(reference.clone(), 1)],
                    canonical_hash_type: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        let info = ScriptReferenceInfo {
            reference_hash: reference.clone(),
            hash_type: 1,
            type_live_cells_count: 72,
            type_cells_count: 245,
            ..Default::default()
        };
        let result = build_script_reference_rollup_state(
            &store,
            vec![((reference.clone(), 1), Some(vec![0x91; 32]))],
            HashMap::from([((reference, 1), info)]),
        );
        assert!(
            result.is_err(),
            "a registered type-id must not lose its family when the code cell upgrades"
        );
    }

    #[test]
    fn test_materialize_script_versions_and_families_rolls_up_reference_records_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let family_id = "default-lock";
        let version_hash = vec![0x41; 32];
        let type_reference_hash = vec![0x51; 32];
        let data_reference_hash = version_hash.clone();

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
                    registered_references: vec![(type_reference_hash.clone(), 1)],
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
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                &[],
                &HashMap::new(),
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut batch,
            )
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

    /// P3.4: the rollup is recomputed every batch, but only rows whose bytes
    /// changed may be written.
    ///
    /// Before this, every live block rewrote the whole rollup: 1,903 reference
    /// mappings + 1,556 versions + 56 families (~700 KB) on testnet, which by
    /// itself filled the `script_versions` write buffer every ~5.6 blocks and
    /// drove the atomic-flush storm.
    #[test]
    fn materialize_writes_only_changed_rollup_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let family_id = "default-lock";
        let version_hash = vec![0x41; 32];
        let type_reference_hash = vec![0x51; 32];
        let data_reference_hash = version_hash.clone();

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
                    registered_references: vec![(type_reference_hash.clone(), 1)],
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
                    type_cells_count: 2,
                    type_live_cells_count: 1,
                    type_capacity_sum: 200,
                    type_owned_capacity_sum: 90,
                    type_used_capacity_sum: 122,
                    type_owned_knowledge_sum: 55,
                    ..Default::default()
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
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_reference_to_version_direct(1, &type_reference_hash, &version_hash)
            .unwrap();
        store
            .put_script_reference_to_version_direct(0, &data_reference_hash, &version_hash)
            .unwrap();

        // Baseline: the committed rollup now matches the reference info.
        writer.refresh_script_reference_rollups().unwrap();

        // Nothing changed -> nothing staged. No early return on empty input:
        // the rollup is still recomputed, it just finds no differences.
        store.reset_read_call_counters();
        let mut unchanged_batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &HashMap::new(),
                &[],
                &HashMap::new(),
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut unchanged_batch,
            )
            .unwrap();
        assert_eq!(
            unchanged_batch.len(),
            0,
            "an unchanged rollup must stage zero writes"
        );
        // Read budget: exactly ONE `script_reference_to_version` point read per
        // reference, to resolve the mapping the rollup is computed from. The
        // fixture has two references, so two point reads and nothing else —
        // reading each mapping a second time to "diff" it against the value it
        // was just read from is ~1,903 extra reads per block on testnet, inside
        // the commit window. The CF listings (`list_script_reference_infos`,
        // `list_script_versions`, `list_script_families`) are iterator scans and
        // do not count here.
        let (get_cf_calls, multi_get_cf_calls) = store.read_call_counts();
        assert_eq!(
            (get_cf_calls, multi_get_cf_calls),
            (2, 0),
            "the rollup must resolve each reference mapping exactly once"
        );

        let baseline_version = store.get_script_version(&version_hash).unwrap().unwrap();
        assert_eq!(baseline_version.type_capacity_sum, 200);
        let baseline_family = store.get_script_family(family_id).unwrap().unwrap();
        // A known data-hash reference retains family provenance even when no
        // executable cell currently resolves its version mapping.
        assert_eq!(baseline_family.owned_capacity_sum, 270);

        // One reference changes -> only the version and family rows it feeds.
        // The reference->version mappings are unchanged and must not be
        // rewritten.
        let mut changed = store
            .get_script_reference_info(1, &type_reference_hash)
            .unwrap()
            .unwrap();
        changed.type_capacity_sum += 100;
        changed.type_owned_capacity_sum += 100;
        changed.type_owned_knowledge_sum += 40;
        let mut updated_references = HashMap::new();
        updated_references.insert((type_reference_hash.clone(), 1u8), changed);

        let mut changed_batch = StoreBatch::new(&store);
        writer
            .materialize_script_versions_and_families_with_chain(
                &updated_references,
                &[],
                &HashMap::new(),
                &HashMap::new(),
                &crate::sync::undo::SharedUndoSeq::default(),
                &mut changed_batch,
            )
            .unwrap();
        assert_eq!(
            changed_batch.len(),
            2,
            "only the affected version and family rows may be staged"
        );
        changed_batch.commit().unwrap();

        let version = store.get_script_version(&version_hash).unwrap().unwrap();
        assert_eq!(version.type_capacity_sum, 300);
        assert_eq!(version.type_owned_capacity_sum, 190);
        assert_eq!(version.type_owned_knowledge_sum, 95);
        let family = store.get_script_family(family_id).unwrap().unwrap();
        assert_eq!(family.owned_capacity_sum, 370);
        assert_eq!(family.owned_knowledge_sum, 205);
    }

    #[test]
    fn refresh_rollups_uses_canonical_mapping_without_scanning_typed_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let reference = vec![0x71; 32];
        let version = vec![0x81; 32];
        let non_elf = vec![0x91; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_script_reference_info(
            1,
            &reference,
            &ScriptReferenceInfo {
                reference_hash: reference.clone(),
                hash_type: 1,
                lock_cells_count: 2,
                lock_live_cells_count: 1,
                type_cells_count: 3,
                type_live_cells_count: 2,
                ..Default::default()
            },
        );
        batch.put_script_version(
            &version,
            &ScriptVersionInfo {
                version_hash: version.clone(),
                canonical_reference_hash: Some(reference.clone()),
                canonical_hash_type: Some(1),
                type_id_deployments: vec![(reference.clone(), 10)],
                ..Default::default()
            },
        );
        batch.put_cell(
            &[0x45; 32],
            0,
            &ckbadger_store::LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference.clone()),
                type_code_hash: Some(
                    hex::decode(crate::parser::script::TYPE_ID_CODE_HASH).unwrap(),
                ),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 31_560,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(non_elf.clone()),
            },
            11,
        );
        batch.put_cell_by_type(&reference, 11, &[0x45; 32], 0);
        batch.commit().unwrap();
        let payload_before = store
            .get_cell_by_outpoint_key(&keys::encode_outpoint(&[0x45; 32], 0))
            .unwrap();
        writer.refresh_script_reference_rollups().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            None
        );
        assert!(store.get_script_version(&non_elf).unwrap().is_none());
        let history = store.get_script_version(&version).unwrap().unwrap();
        assert_eq!(history.type_id_deployments, vec![(reference.clone(), 10)]);
        assert_eq!(history.lock_live_cells_count, 0);
        assert_eq!(history.type_live_cells_count, 0);
        assert_eq!(
            store
                .get_cell_by_outpoint_key(&keys::encode_outpoint(&[0x45; 32], 0))
                .unwrap(),
            payload_before
        );

        // An undo-restored canonical binding remains authoritative even when
        // there is no matching current payload in this isolated rollup fixture.
        store
            .put_script_reference_to_version_direct(1, &reference, &version)
            .unwrap();
        writer.refresh_script_reference_rollups().unwrap();
        assert_eq!(
            store
                .get_script_reference_version_hash(1, &reference)
                .unwrap(),
            Some(version.clone())
        );
        let restored = store.get_script_version(&version).unwrap().unwrap();
        assert_eq!(restored.lock_live_cells_count, 1);
        assert_eq!(restored.type_live_cells_count, 2);
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
