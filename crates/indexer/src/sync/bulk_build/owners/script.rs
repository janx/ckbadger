use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::{
    CkbadgerStore, ScriptDailyDelta, ScriptReferenceInfo, CF_SCRIPT_REFERENCE_INFO, CF_STATS_SCRIPT,
};
use rustc_hash::FxHashMap;

use super::{BulkReducer, ReducerContext};
use crate::db::writer::{
    apply_generic_type_code, build_script_reference_rollup_state_with_lineage, reference_binary,
    GenericTypeCodes, TypeIdCodeChange,
};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{CellFacts, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct ScriptOwner {
    /// Usage by the script's pair (code hash, hash type): the one script usage
    /// the bulk build keeps (lane rd/api-lock-names; a second, lumped copy by
    /// the code hash alone summed every hash type's cells into one
    /// `script_info` row and is gone).
    reference_infos: FxHashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
    type_id_reference_live_versions: FxHashMap<Vec<u8>, FxHashMap<Vec<u8>, u32>>,
    type_id_reference_versions: FxHashMap<Vec<u8>, Option<Vec<u8>>>,
    /// Type hash -> binary -> its live code cells under that type script, for
    /// every type script that is not Type ID (lane rd/api-lock-names, the
    /// lead's ruling): what a generic type reference runs, counted by the one
    /// family rule at finalize. Only code cells enter it.
    generic_type_live_versions: FxHashMap<Vec<u8>, std::collections::BTreeMap<Vec<u8>, u32>>,
    type_id_lineage: FxHashMap<(Vec<u8>, Vec<u8>), Vec<i64>>,
    daily_deltas: FxHashMap<(Vec<u8>, u8, bool, u32), ScriptDailyDelta>,
}

enum TypeIdReferenceResolution {
    Retired,
    Resolved(Vec<u8>),
    Ambiguous,
}

impl ScriptOwner {
    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.reference_infos,
            |(reference_hash, _hash_type), info| {
                crate::sync::bulk_build::accounting::bytes_vec_bytes(reference_hash)
                    + std::mem::size_of::<u8>() as u64
                    + crate::sync::bulk_build::accounting::serialized_bytes(info)
            },
        ) + crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.type_id_reference_live_versions,
            |reference_hash, version_counts| {
                crate::sync::bulk_build::accounting::bytes_vec_bytes(reference_hash)
                    + crate::sync::bulk_build::accounting::hash_map_bytes(
                        version_counts,
                        |version_hash, count| {
                            crate::sync::bulk_build::accounting::bytes_vec_bytes(version_hash)
                                + std::mem::size_of_val(count) as u64
                        },
                    )
            },
        ) + crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.type_id_reference_versions,
            |reference_hash, version_hash| {
                crate::sync::bulk_build::accounting::bytes_vec_bytes(reference_hash)
                    + version_hash
                        .as_ref()
                        .map(crate::sync::bulk_build::accounting::bytes_vec_bytes)
                        .unwrap_or(0)
            },
        ) + crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.generic_type_live_versions,
            |reference_hash, version_counts| {
                crate::sync::bulk_build::accounting::bytes_vec_bytes(reference_hash)
                    + version_counts
                        .keys()
                        .map(|version_hash| {
                            crate::sync::bulk_build::accounting::bytes_vec_bytes(version_hash)
                                + std::mem::size_of::<u32>() as u64
                        })
                        .sum::<u64>()
            },
        ) + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.type_id_lineage)
            + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.daily_deltas)
    }

    fn apply_type_id_reference_live_version_delta(
        &mut self,
        reference_hash: &[u8],
        version_hash: &[u8],
        delta: i32,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        let reference_key = reference_hash.to_vec();
        let (resolution, should_remove_live_counts) = {
            let version_counts = self
                .type_id_reference_live_versions
                .entry(reference_key.clone())
                .or_default();
            match delta {
                1 => {
                    let count = version_counts.entry(version_hash.to_vec()).or_insert(0);
                    *count = count.checked_add(1).ok_or_else(|| {
                        anyhow!(
                            "type reference live version count overflow: reference_hash=0x{}, version_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                            hex::encode(reference_hash),
                            hex::encode(version_hash),
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index
                        )
                    })?;
                }
                -1 => {
                    let count = version_counts.get_mut(version_hash).ok_or_else(|| {
                        anyhow!(
                            "missing type reference live version while consuming code cell: reference_hash=0x{}, version_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                            hex::encode(reference_hash),
                            hex::encode(version_hash),
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index
                        )
                    })?;
                    *count = count.checked_sub(1).ok_or_else(|| {
                        anyhow!(
                            "type reference live version count underflow: reference_hash=0x{}, version_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                            hex::encode(reference_hash),
                            hex::encode(version_hash),
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index
                        )
                    })?;
                    if *count == 0 {
                        version_counts.remove(version_hash);
                    }
                }
                _ => {
                    bail!(
                        "unsupported type reference live version delta: reference_hash=0x{}, version_hash=0x{}, delta={}, block={}, tx=0x{}, tx_index={}",
                        hex::encode(reference_hash),
                        hex::encode(version_hash),
                        delta,
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index
                    );
                }
            }

            let resolution = match version_counts.len() {
                0 => TypeIdReferenceResolution::Retired,
                1 => TypeIdReferenceResolution::Resolved(
                    version_counts
                        .keys()
                        .next()
                        .expect("single live version count must exist")
                        .clone(),
                ),
                _ => TypeIdReferenceResolution::Ambiguous,
            };
            (resolution, version_counts.is_empty())
        };

        if should_remove_live_counts {
            self.type_id_reference_live_versions.remove(&reference_key);
        }

        match resolution {
            TypeIdReferenceResolution::Resolved(version_hash) => {
                self.type_id_reference_versions
                    .insert(reference_key, Some(version_hash));
            }
            TypeIdReferenceResolution::Retired | TypeIdReferenceResolution::Ambiguous => {
                self.type_id_reference_versions.insert(reference_key, None);
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn record_daily_delta(
        &mut self,
        code_hash: &[u8],
        hash_type: u8,
        is_type: bool,
        date_yyyymmdd: u32,
        owned_capacity_delta: i128,
        owned_knowledge_delta: i128,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        if owned_capacity_delta == 0 && owned_knowledge_delta == 0 {
            return Ok(());
        }

        // The kind is formatted only if the arithmetic below fails.
        let kind = if is_type { "type" } else { "lock" };
        let entry = self
            .daily_deltas
            .entry((code_hash.to_vec(), hash_type, is_type, date_yyyymmdd))
            .or_default();
        entry.owned_capacity_delta = checked_signed_i128(
            code_hash,
            format_args!("{kind}(hash_type={hash_type})"),
            "daily owned_capacity_delta",
            entry.owned_capacity_delta,
            owned_capacity_delta,
            tx,
        )?;
        entry.owned_knowledge_delta = checked_signed_i128(
            code_hash,
            format_args!("{kind}(hash_type={hash_type})"),
            "daily owned_knowledge_delta",
            entry.owned_knowledge_delta,
            owned_knowledge_delta,
            tx,
        )?;
        Ok(())
    }

    fn build_sealed_rows(&self) -> Vec<MaterializedRow> {
        let mut daily_keys = self.daily_deltas.keys().collect::<Vec<_>>();
        daily_keys.sort();

        daily_keys
            .into_iter()
            .filter_map(|(code_hash, hash_type, is_type, date)| {
                let delta = self
                    .daily_deltas
                    .get(&(code_hash.clone(), *hash_type, *is_type, *date))
                    .expect("sorted script daily key must exist");
                (delta.owned_capacity_delta != 0 || delta.owned_knowledge_delta != 0).then_some(
                    MaterializedRow::new(
                        CF_STATS_SCRIPT,
                        keys::encode_script_daily_key(code_hash, *hash_type, *is_type, *date)
                            .to_vec(),
                        bincode::serialize(delta)
                            .expect("script daily delta serialization must succeed"),
                    ),
                )
            })
            .collect::<Vec<_>>()
    }

    fn build_snapshot_rows(&self, domain_store: &CkbadgerStore) -> Result<Vec<MaterializedRow>> {
        let mut reference_keys: Vec<&(Vec<u8>, u8)> = self.reference_infos.keys().collect();
        reference_keys.sort();
        let mut all_rows = reference_keys
            .into_iter()
            .map(|(reference_hash, hash_type)| {
                let info = self
                    .reference_infos
                    .get(&(reference_hash.clone(), *hash_type))
                    .expect("sorted script reference key must exist in script owner");
                Ok(MaterializedRow::new(
                    CF_SCRIPT_REFERENCE_INFO,
                    keys::encode_script_reference_key(*hash_type, reference_hash).to_vec(),
                    bincode::serialize(info)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let mut mapping_keys = self.reference_infos.keys().cloned().collect::<Vec<_>>();
        mapping_keys.extend(
            self.type_id_reference_versions
                .keys()
                .cloned()
                .map(|reference_hash| (reference_hash, 1)),
        );
        mapping_keys.sort();
        mapping_keys.dedup();
        let mut reference_mappings = Vec::with_capacity(mapping_keys.len());
        for (reference_hash, hash_type) in mapping_keys {
            let version_hash = reference_binary(&reference_hash, hash_type, || {
                Ok(self
                    .type_id_reference_versions
                    .get(&reference_hash)
                    .cloned()
                    .flatten())
            })?;
            reference_mappings.push(((reference_hash, hash_type), version_hash));
        }
        for ((reference_hash, hash_type), version_hash) in &reference_mappings {
            if let Some(version_hash) = version_hash {
                all_rows.push(MaterializedRow::new(
                    ckbadger_store::CF_SCRIPT_REFERENCE_TO_VERSION,
                    keys::encode_script_reference_key(*hash_type, reference_hash).to_vec(),
                    version_hash.clone(),
                ));
            }
        }

        // Build rollup state from in-memory data rather than reading from
        // the store.  This allows build_snapshot_rows to produce correct
        // version/family rows without requiring intermediate writes.
        let mut reference_info_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo> = self
            .reference_infos
            .iter()
            .map(|((h, ht), info)| ((h.clone(), *ht), info.clone()))
            .collect();
        for ((reference_hash, hash_type), _) in &reference_mappings {
            reference_info_map
                .entry((reference_hash.clone(), *hash_type))
                .or_insert_with(|| ScriptReferenceInfo {
                    reference_hash: reference_hash.clone(),
                    hash_type: *hash_type,
                    ..Default::default()
                });
        }
        let mut lineage = self
            .type_id_lineage
            .iter()
            .flat_map(|((reference_hash, version_hash), blocks)| {
                blocks.iter().map(|block| TypeIdCodeChange {
                    reference_hash: reference_hash.clone(),
                    version_hash: Some(version_hash.clone()),
                    block_number: *block,
                })
            })
            .collect::<Vec<_>>();
        lineage.sort_by(|a, b| {
            (a.block_number, &a.reference_hash, &a.version_hash).cmp(&(
                b.block_number,
                &b.reference_hash,
                &b.version_hash,
            ))
        });
        // A type reference with no Type ID binding runs what its live code
        // cells hold (the lead's ruling), counted by the one family rule.
        let mut generic = GenericTypeCodes::new();
        for ((reference_hash, hash_type), binding) in &reference_mappings {
            if *hash_type == 1
                && binding.is_none()
                && !self.type_id_reference_versions.contains_key(reference_hash)
            {
                if let Some(live) = self.generic_type_live_versions.get(reference_hash) {
                    apply_generic_type_code(&mut generic, reference_hash, live);
                }
            }
        }
        let rollups = build_script_reference_rollup_state_with_lineage(
            domain_store,
            reference_mappings,
            &generic,
            reference_info_map,
            &lineage,
        )?;

        // The labels each version row had before the rollup: those of the
        // rows it changed are reported, every other row kept its labels.
        let labels_before: HashMap<&[u8], &Option<crate::db::writer::VersionLabels>> = rollups
            .changed_versions
            .iter()
            .map(|(hash, labels)| (hash.as_slice(), labels))
            .collect();
        let mut versions: Vec<(Vec<u8>, ckbadger_store::ScriptVersionInfo)> =
            rollups.versions.into_iter().collect();
        versions.sort_by(|left, right| left.0.cmp(&right.0));
        for (version_hash, info) in versions {
            let (prev_family, prev_name) = match labels_before.get(version_hash.as_slice()) {
                Some(Some(labels)) => (labels.family_id.clone(), labels.name.clone()),
                Some(None) => (None, None),
                None => (info.family_id.clone(), info.name.clone()),
            };
            // Same as the live writer: a family/name rebind must tombstone the
            // previous index key. Final snapshot puts only; deletes are staged
            // as empty-value rows on a delete CF path via materializer helpers.
            let prev_family = prev_family.as_deref();
            let new_family = info.family_id.as_deref();
            if let (Some(old), Some(new)) = (prev_family, new_family) {
                if old != new {
                    all_rows.push(MaterializedRow::delete(
                        ckbadger_store::CF_SCRIPT_VERSIONS_BY_FAMILY,
                        keys::encode_script_version_by_family_key(old, &version_hash),
                    ));
                }
            } else if let (Some(old), None) = (prev_family, new_family) {
                all_rows.push(MaterializedRow::delete(
                    ckbadger_store::CF_SCRIPT_VERSIONS_BY_FAMILY,
                    keys::encode_script_version_by_family_key(old, &version_hash),
                ));
            }
            if let Some(family) = new_family {
                all_rows.push(MaterializedRow::new(
                    ckbadger_store::CF_SCRIPT_VERSIONS_BY_FAMILY,
                    keys::encode_script_version_by_family_key(family, &version_hash),
                    Vec::new(),
                ));
            }
            let prev_name = prev_name.as_deref();
            let new_name = info.name.as_deref();
            if let (Some(old), Some(new)) = (prev_name, new_name) {
                if old != new {
                    all_rows.push(MaterializedRow::delete(
                        ckbadger_store::CF_SCRIPT_VERSIONS_BY_LABEL,
                        keys::encode_script_version_by_label_key(old, &version_hash),
                    ));
                }
            } else if let (Some(old), None) = (prev_name, new_name) {
                all_rows.push(MaterializedRow::delete(
                    ckbadger_store::CF_SCRIPT_VERSIONS_BY_LABEL,
                    keys::encode_script_version_by_label_key(old, &version_hash),
                ));
            }
            if let Some(name) = new_name {
                all_rows.push(MaterializedRow::new(
                    ckbadger_store::CF_SCRIPT_VERSIONS_BY_LABEL,
                    keys::encode_script_version_by_label_key(name, &version_hash),
                    Vec::new(),
                ));
            }
            all_rows.push(MaterializedRow::new(
                ckbadger_store::CF_SCRIPT_VERSIONS,
                version_hash,
                bincode::serialize(&info)?,
            ));
        }

        let mut families: Vec<(String, ckbadger_store::ScriptFamilyInfo)> =
            rollups.families.into_iter().collect();
        families.sort_by(|left, right| left.0.cmp(&right.0));
        for (family_id, info) in families {
            all_rows.push(MaterializedRow::new(
                ckbadger_store::CF_SCRIPT_FAMILIES,
                family_id.into_bytes(),
                bincode::serialize(&info)?,
            ));
        }

        Ok(all_rows)
    }

    #[cfg(test)]
    pub(crate) fn build_final_rows(
        &self,
        domain_store: &CkbadgerStore,
    ) -> Result<super::super::materialize::OwnerFinalRows> {
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: self.build_sealed_rows(),
            snapshot_rows: self.build_snapshot_rows(domain_store)?,
        })
    }
}

impl BulkReducer for ScriptOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let mut reference_deltas: FxHashMap<(Vec<u8>, u8), ScriptReferenceDelta> =
            FxHashMap::default();

        for input in &tx.resolved_inputs {
            apply_input_reference_deltas(input, ctx, &mut reference_deltas, tx)?;
            let native_type_id = input
                .type_code_hash_id
                .zip(input.type_hash_type)
                .is_some_and(|(hash, kind)| is_native_type_id(ctx, hash, kind));
            // A consumed code cell under a type script that is not Type ID
            // leaves what that generic type reference runs (its binary is
            // among the live ones only if it is code).
            if !native_type_id && !self.generic_type_live_versions.is_empty() {
                if let (Some(reference), Some(data_hash)) =
                    (input.type_script_hash_id, input.data_hash)
                {
                    let reference = ctx.resolve_identity(reference);
                    if let Some(live) = self.generic_type_live_versions.get_mut(reference) {
                        if let Some(count) = live.get_mut(data_hash.as_slice()) {
                            *count -= 1;
                            if *count == 0 {
                                live.remove(data_hash.as_slice());
                            }
                        }
                    }
                }
            }
            if native_type_id {
                let (Some(reference), Some(data_hash)) =
                    (input.type_script_hash_id, input.data_hash)
                else {
                    bail!("native TYPE_ID input lacks type hash or data hash: block={} tx=0x{} input=0x{}:{}", tx.block_number, hex::encode(tx.tx_hash), hex::encode(input.outpoint.tx_hash), input.outpoint.index);
                };
                let reference = ctx.resolve_identity(reference);
                if self
                    .type_id_reference_live_versions
                    .get(reference)
                    .is_some_and(|versions| versions.contains_key(data_hash.as_slice()))
                {
                    self.apply_type_id_reference_live_version_delta(reference, &data_hash, -1, tx)?;
                }
            }
        }
        for cell in tx.cells.iter() {
            apply_output_reference_deltas(cell, ctx, &mut reference_deltas, tx)?;
            let native_type_id = cell
                .type_code_hash_id
                .zip(cell.type_hash_type)
                .is_some_and(|(hash, kind)| is_native_type_id(ctx, hash, kind));
            if native_type_id && cell.type_script_hash_id.is_none() {
                bail!(
                    "type-id output lacks type hash at block {}",
                    tx.block_number
                );
            }
            // A code cell under a type script that is not Type ID: what that
            // generic type reference runs.
            if !native_type_id && ckbadger_store::keys::is_code_cell(&cell.data) {
                if let Some(reference) = cell.type_script_hash_id {
                    let reference = ctx.resolve_identity(reference);
                    let data_hash = cell.data_hash.ok_or_else(|| {
                        anyhow!(
                            "typed code output lacks data hash: block={} tx=0x{}",
                            tx.block_number,
                            hex::encode(tx.tx_hash)
                        )
                    })?;
                    let count = self
                        .generic_type_live_versions
                        .entry(reference.to_vec())
                        .or_default()
                        .entry(data_hash.to_vec())
                        .or_insert(0);
                    *count = count.checked_add(1).ok_or_else(|| {
                        anyhow!(
                            "generic type reference live code count overflow: reference=0x{} block={}",
                            hex::encode(reference),
                            tx.block_number
                        )
                    })?;
                }
            }
            if native_type_id && ckbadger_store::keys::is_code_cell(&cell.data) {
                let reference = cell.type_script_hash_id.ok_or_else(|| {
                    anyhow!(
                        "TYPE_ID ELF output lacks type hash at block {}",
                        tx.block_number
                    )
                })?;
                let reference = ctx.resolve_identity(reference);
                let data_hash = cell.data_hash.ok_or_else(|| {
                    anyhow!(
                        "TYPE_ID ELF output lacks data hash: block={} tx=0x{}",
                        tx.block_number,
                        hex::encode(tx.tx_hash)
                    )
                })?;
                self.type_id_lineage
                    .entry((reference.to_vec(), data_hash.to_vec()))
                    .or_default()
                    .push(tx.block_number);
                self.apply_type_id_reference_live_version_delta(reference, &data_hash, 1, tx)?;
            }
        }

        let date_yyyymmdd = keys::timestamp_ms_to_date(tx.chain_time_ms);
        for ((reference_hash, hash_type), delta) in reference_deltas {
            let info = self
                .reference_infos
                .entry((reference_hash.clone(), hash_type))
                .or_insert_with(|| ScriptReferenceInfo {
                    reference_hash: reference_hash.clone(),
                    hash_type,
                    ..Default::default()
                });
            apply_reference_delta(info, &reference_hash, hash_type, &delta, tx)?;

            // Daily deltas are recorded per reference form: the same code_hash
            // bytes used with different hash_types are distinct references
            // with independent capacity timelines (matches the live pipeline).
            self.record_daily_delta(
                &reference_hash,
                hash_type,
                false,
                date_yyyymmdd,
                delta.lock_owned_capacity_delta,
                delta.lock_owned_knowledge_delta,
                tx,
            )?;
            self.record_daily_delta(
                &reference_hash,
                hash_type,
                true,
                date_yyyymmdd,
                delta.type_owned_capacity_delta,
                delta.type_owned_knowledge_delta,
                tx,
            )?;
        }

        Ok(())
    }

    fn flush_sealed(&mut self, materializer: &mut Materializer<'_>) -> Result<()> {
        let rows = self.build_sealed_rows();
        if rows.is_empty() {
            return Ok(());
        }
        materializer.stream_sealed_aggregate_rows(&rows)
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        let rows = self.build_snapshot_rows(materializer.domain_store())?;
        materializer.materialize_final_snapshot(&rows)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ScriptReferenceDelta {
    lock_cells_delta: i64,
    lock_live_cells_delta: i64,
    lock_capacity_delta: i128,
    lock_owned_capacity_delta: i128,
    lock_used_capacity_delta: i128,
    lock_owned_knowledge_delta: i128,
    type_cells_delta: i64,
    type_live_cells_delta: i64,
    type_capacity_delta: i128,
    type_owned_capacity_delta: i128,
    type_used_capacity_delta: i128,
    type_owned_knowledge_delta: i128,
}

fn is_native_type_id(
    ctx: &ReducerContext<'_>,
    code_hash_id: crate::sync::types::InternId,
    hash_type: i16,
) -> bool {
    hash_type == 1
        && crate::parser::script::is_type_id(ctx.resolve_identity(code_hash_id), hash_type)
}

fn parse_hash_type_u8(
    hash_type: i16,
    script_kind: &str,
    tx: &ResolvedTxFacts<'_>,
    code_hash_id: crate::sync::types::InternId,
) -> Result<u8> {
    match hash_type {
        0 | 1 | 2 | 4 => Ok(hash_type as u8),
        _ => Err(anyhow!(
            "invalid {} script hash_type: code_hash_id={}, hash_type={}, block={}, tx=0x{}, tx_index={}, expected_one_of=[0,1,2,4]",
            script_kind,
            code_hash_id.as_usize(),
            hash_type,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )),
    }
}

fn apply_input_reference_deltas(
    input: &ResolvedInputFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut FxHashMap<(Vec<u8>, u8), ScriptReferenceDelta>,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    let lock_hash_type =
        parse_hash_type_u8(input.lock_hash_type, "lock", tx, input.lock_code_hash_id)?;
    let lock_reference_hash = ctx.resolve_identity(input.lock_code_hash_id).to_vec();
    let lock_delta = deltas
        .entry((lock_reference_hash.clone(), lock_hash_type))
        .or_default();
    lock_delta.lock_live_cells_delta -= 1;
    lock_delta.lock_owned_capacity_delta -= i128::from(input.capacity);
    lock_delta.lock_owned_knowledge_delta -= i128::from(input.occupied_capacity);

    if let Some(type_code_hash_id) = input.type_code_hash_id {
        let type_hash_type = input.type_hash_type.ok_or_else(|| {
            anyhow!(
                "missing type hash_type for resolved typed input: block={}, tx=0x{}, tx_index={}, outpoint=0x{}:{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(input.outpoint.tx_hash),
                input.outpoint.index
            )
        })?;
        let type_hash_type = parse_hash_type_u8(type_hash_type, "type", tx, type_code_hash_id)?;
        let type_reference_hash = ctx.resolve_identity(type_code_hash_id).to_vec();
        let type_delta = deltas
            .entry((type_reference_hash.clone(), type_hash_type))
            .or_default();
        type_delta.type_live_cells_delta -= 1;
        type_delta.type_owned_capacity_delta -= i128::from(input.capacity);
        type_delta.type_owned_knowledge_delta -= i128::from(input.occupied_capacity);
    }

    Ok(())
}

fn apply_output_reference_deltas(
    cell: &CellFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut FxHashMap<(Vec<u8>, u8), ScriptReferenceDelta>,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    let lock_hash_type =
        parse_hash_type_u8(cell.lock_hash_type, "lock", tx, cell.lock_code_hash_id)?;
    let lock_reference_hash = ctx.resolve_identity(cell.lock_code_hash_id).to_vec();
    let lock_delta = deltas
        .entry((lock_reference_hash.clone(), lock_hash_type))
        .or_default();
    lock_delta.lock_cells_delta += 1;
    lock_delta.lock_live_cells_delta += 1;
    lock_delta.lock_capacity_delta += i128::from(cell.capacity);
    lock_delta.lock_owned_capacity_delta += i128::from(cell.capacity);
    lock_delta.lock_used_capacity_delta += i128::from(cell.occupied_capacity);
    lock_delta.lock_owned_knowledge_delta += i128::from(cell.occupied_capacity);

    if let Some(type_code_hash_id) = cell.type_code_hash_id {
        let type_hash_type = cell.type_hash_type.ok_or_else(|| {
            anyhow!(
                "missing type hash_type for typed output: block={}, tx=0x{}, tx_index={}, outpoint=0x{}:{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(cell.outpoint.tx_hash),
                cell.outpoint.index
            )
        })?;
        let type_hash_type = parse_hash_type_u8(type_hash_type, "type", tx, type_code_hash_id)?;
        let type_reference_hash = ctx.resolve_identity(type_code_hash_id).to_vec();
        let type_delta = deltas
            .entry((type_reference_hash.clone(), type_hash_type))
            .or_default();
        type_delta.type_cells_delta += 1;
        type_delta.type_live_cells_delta += 1;
        type_delta.type_capacity_delta += i128::from(cell.capacity);
        type_delta.type_owned_capacity_delta += i128::from(cell.capacity);
        type_delta.type_used_capacity_delta += i128::from(cell.occupied_capacity);
        type_delta.type_owned_knowledge_delta += i128::from(cell.occupied_capacity);
    }

    Ok(())
}

fn apply_reference_delta(
    info: &mut ScriptReferenceInfo,
    reference_hash: &[u8],
    hash_type: u8,
    delta: &ScriptReferenceDelta,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    info.lock_cells_count = checked_next_reference_i64(
        reference_hash,
        hash_type,
        "lock cells_count",
        info.lock_cells_count,
        delta.lock_cells_delta,
        tx,
    )?;
    info.lock_live_cells_count = checked_next_reference_i64(
        reference_hash,
        hash_type,
        "lock live_cells_count",
        info.lock_live_cells_count,
        delta.lock_live_cells_delta,
        tx,
    )?;
    info.lock_capacity_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "lock capacity_sum",
        info.lock_capacity_sum,
        delta.lock_capacity_delta,
        tx,
    )?;
    info.lock_owned_capacity_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "lock owned_capacity_sum",
        info.lock_owned_capacity_sum,
        delta.lock_owned_capacity_delta,
        tx,
    )?;
    info.lock_used_capacity_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "lock used_capacity_sum",
        info.lock_used_capacity_sum,
        delta.lock_used_capacity_delta,
        tx,
    )?;
    info.lock_owned_knowledge_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "lock owned_knowledge_sum",
        info.lock_owned_knowledge_sum,
        delta.lock_owned_knowledge_delta,
        tx,
    )?;

    info.type_cells_count = checked_next_reference_i64(
        reference_hash,
        hash_type,
        "type cells_count",
        info.type_cells_count,
        delta.type_cells_delta,
        tx,
    )?;
    info.type_live_cells_count = checked_next_reference_i64(
        reference_hash,
        hash_type,
        "type live_cells_count",
        info.type_live_cells_count,
        delta.type_live_cells_delta,
        tx,
    )?;
    info.type_capacity_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "type capacity_sum",
        info.type_capacity_sum,
        delta.type_capacity_delta,
        tx,
    )?;
    info.type_owned_capacity_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "type owned_capacity_sum",
        info.type_owned_capacity_sum,
        delta.type_owned_capacity_delta,
        tx,
    )?;
    info.type_used_capacity_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "type used_capacity_sum",
        info.type_used_capacity_sum,
        delta.type_used_capacity_delta,
        tx,
    )?;
    info.type_owned_knowledge_sum = checked_next_reference_i128(
        reference_hash,
        hash_type,
        "type owned_knowledge_sum",
        info.type_owned_knowledge_sum,
        delta.type_owned_knowledge_delta,
        tx,
    )?;

    if info.lock_used_capacity_sum > info.lock_capacity_sum {
        bail!(
            "script reference lock used capacity exceeds total: reference_hash=0x{}, hash_type={}, lock_used_capacity_sum={}, lock_capacity_sum={}",
            hex::encode(reference_hash),
            hash_type,
            info.lock_used_capacity_sum,
            info.lock_capacity_sum
        );
    }
    if info.lock_owned_knowledge_sum > info.lock_owned_capacity_sum {
        bail!(
            "script reference lock owned knowledge exceeds owned capacity: reference_hash=0x{}, hash_type={}, lock_owned_knowledge_sum={}, lock_owned_capacity_sum={}",
            hex::encode(reference_hash),
            hash_type,
            info.lock_owned_knowledge_sum,
            info.lock_owned_capacity_sum
        );
    }
    if info.type_used_capacity_sum > info.type_capacity_sum {
        bail!(
            "script reference type used capacity exceeds total: reference_hash=0x{}, hash_type={}, type_used_capacity_sum={}, type_capacity_sum={}",
            hex::encode(reference_hash),
            hash_type,
            info.type_used_capacity_sum,
            info.type_capacity_sum
        );
    }
    if info.type_owned_knowledge_sum > info.type_owned_capacity_sum {
        bail!(
            "script reference type owned knowledge exceeds owned capacity: reference_hash=0x{}, hash_type={}, type_owned_knowledge_sum={}, type_owned_capacity_sum={}",
            hex::encode(reference_hash),
            hash_type,
            info.type_owned_knowledge_sum,
            info.type_owned_capacity_sum
        );
    }

    Ok(())
}

fn checked_next_reference_i64(
    reference_hash: &[u8],
    hash_type: u8,
    metric: &str,
    current: i64,
    delta: i64,
    tx: &ResolvedTxFacts<'_>,
) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "script reference {} overflow: reference_hash=0x{}, hash_type={}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "script reference {} underflow: reference_hash=0x{}, hash_type={}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    Ok(next)
}

fn checked_next_reference_i128(
    reference_hash: &[u8],
    hash_type: u8,
    metric: &str,
    current: i128,
    delta: i128,
    tx: &ResolvedTxFacts<'_>,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "script reference {} overflow: reference_hash=0x{}, hash_type={}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "script reference {} underflow: reference_hash=0x{}, hash_type={}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(reference_hash),
            hash_type,
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    Ok(next)
}

fn checked_signed_i128(
    code_hash: &[u8],
    script_kind: impl std::fmt::Display,
    metric: &str,
    current: i128,
    delta: i128,
    tx: &ResolvedTxFacts<'_>,
) -> Result<i128> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "script {} {} overflow: code_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            script_kind,
            metric,
            hex::encode(code_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })
}

pub(crate) fn materialize_script_reference_infos_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<HashMap<(Vec<u8>, u8), ScriptReferenceInfo>> {
    let interner = IdentityInterner::default();
    let (arena, _) = build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let frozen = interner.snapshot_for_reads();
    let ctx = ReducerContext::new(&frozen);
    let mut owner = ScriptOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    Ok(owner.reference_infos.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{CellFacts, OutPointKey, ResolvedInputFacts};
    use crate::sync::types::InternId;
    use ckbadger_store::{ScriptFamilyInfo, ScriptReferenceInfo, ScriptVersionInfo};

    /// A daily script delta that overflows names the script kind with its
    /// hash_type, the metric, code hash, operands, block, tx and tx_index
    /// exactly as below, for both kinds and both metrics. The kind is
    /// formatted only when the arithmetic fails.
    #[test]
    fn script_daily_delta_overflow_names_kind_hash_type_and_tx() {
        let tx = ResolvedTxFacts {
            tx_hash: [0xcd; 32],
            block_number: 777,
            block_hash: [0x01; 32],
            timestamp_ms: 1_700_000_000_000,
            chain_time_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 5,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: Vec::new().into(),
        };
        let code_hash = [0x5c; 32];
        for (is_type, kind) in [(false, "lock"), (true, "type")] {
            let mut capacity_owner = ScriptOwner::default();
            capacity_owner
                .record_daily_delta(&code_hash, 2, is_type, 20240102, i128::MAX, 0, &tx)
                .expect("first capacity delta fits");
            let capacity_error = capacity_owner
                .record_daily_delta(&code_hash, 2, is_type, 20240102, 1, 0, &tx)
                .expect_err("capacity delta must overflow");
            assert_eq!(
                capacity_error.to_string(),
                format!(
                    "script {kind}(hash_type=2) daily owned_capacity_delta overflow: code_hash=0x{}, current={}, delta=1, block=777, tx=0x{}, tx_index=5",
                    "5c".repeat(32),
                    i128::MAX,
                    "cd".repeat(32)
                )
            );

            let mut knowledge_owner = ScriptOwner::default();
            knowledge_owner
                .record_daily_delta(&code_hash, 4, is_type, 20240102, 0, i128::MIN, &tx)
                .expect("first knowledge delta fits");
            let knowledge_error = knowledge_owner
                .record_daily_delta(&code_hash, 4, is_type, 20240102, 0, -1, &tx)
                .expect_err("knowledge delta must overflow");
            assert_eq!(
                knowledge_error.to_string(),
                format!(
                    "script {kind}(hash_type=4) daily owned_knowledge_delta overflow: code_hash=0x{}, current={}, delta=-1, block=777, tx=0x{}, tx_index=5",
                    "5c".repeat(32),
                    i128::MIN,
                    "cd".repeat(32)
                )
            );
        }
    }

    #[test]
    fn script_owner_reduces_lock_and_type_live_usage() {
        let lock_code_hash = vec![0x11; 32];
        let type_code_hash = vec![0x22; 32];
        let interner = IdentityInterner::default();
        interner.intern_bytes(vec![0xaa; 32]);
        let lock_code_hash_id = interner.intern_bytes(lock_code_hash.clone());
        interner.intern_bytes(vec![0xab; 20]);
        interner.intern_bytes(vec![0xbb; 32]);
        interner.intern_bytes(vec![0xcc; 20]);
        interner.intern_bytes(vec![0xdd; 32]);
        let type_code_hash_id = interner.intern_bytes(type_code_hash.clone());
        interner.intern_bytes(vec![0xee; 32]);
        interner.intern_bytes(vec![0xf0; 32]);
        interner.intern_bytes(vec![0xff; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x31; 32],
            block_number: 100,
            block_hash: [0x02; 32],
            timestamp_ms: 1_700_000_000_000,
            chain_time_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x31; 32], 0),
                    created_at_block: 100,
                    created_by_block_dao_ar: 1,
                    capacity: 100_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id,
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x31; 32], 1),
                    created_at_block: 100,
                    created_by_block_dao_ar: 1,
                    capacity: 200_00000000,
                    lock_script_hash_id: InternId::new(3),
                    lock_code_hash_id,
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(4),
                    type_script_hash_id: Some(InternId::new(5)),
                    type_code_hash_id: Some(type_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(7)),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: Some(42),
                    semantic_tag: crate::sync::CellSemanticTag::Sudt,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };
        let tx1 = ResolvedTxFacts {
            tx_hash: [0x32; 32],
            block_number: 100,
            block_hash: [0x02; 32],
            timestamp_ms: 1_700_000_000_001,
            chain_time_ms: 1_700_000_000_001,
            block_dao_ar: 1,
            tx_index: 1,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x31; 32], 1),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: Some(42),
                lock_script_hash_id: InternId::new(3),
                lock_code_hash_id,
                lock_hash_type: 1,
                lock_args_id: InternId::new(4),
                type_script_hash_id: Some(InternId::new(5)),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(7)),
                semantic_tag: crate::sync::CellSemanticTag::Sudt,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x32; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 80_00000000,
                lock_script_hash_id: InternId::new(8),
                lock_code_hash_id,
                lock_hash_type: 1,
                lock_args_id: InternId::new(9),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };

        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply tx0");
        owner.apply_tx(&tx1, &ctx).expect("apply tx1");

        // Every cell here is by `type`: each script's usage is its pair's row.
        let lock_info = owner
            .reference_infos
            .get(&(lock_code_hash.clone(), 1))
            .expect("lock info");
        assert_eq!(lock_info.lock_cells_count, 3);
        assert_eq!(lock_info.lock_live_cells_count, 2);
        assert_eq!(lock_info.lock_capacity_sum, 380_00000000);
        assert_eq!(lock_info.lock_owned_capacity_sum, 180_00000000);
        assert_eq!(lock_info.lock_used_capacity_sum, 264_00000000);
        assert_eq!(lock_info.lock_owned_knowledge_sum, 122_00000000);

        let type_info = owner
            .reference_infos
            .get(&(type_code_hash.clone(), 1))
            .expect("type info");
        assert_eq!(type_info.type_cells_count, 1);
        assert_eq!(type_info.type_live_cells_count, 0);
        assert_eq!(type_info.type_capacity_sum, 200_00000000);
        assert_eq!(type_info.type_owned_capacity_sum, 0);
        assert_eq!(type_info.type_used_capacity_sum, 142_00000000);
        assert_eq!(type_info.type_owned_knowledge_sum, 0);
    }

    /// Lane rd/api-lock-names: the bulk build neither reads nor writes
    /// `script_info` (the registry's labels, label import's alone): a corrupt
    /// label row is no business of the bulk finalize, which leaves it exactly
    /// as it found it. Its readers (label import's preflight, the API) fail
    /// on it naming the pair. (Before, the finalize read the row to carry its
    /// labels into the lumped usage row, and failed on it rebuild-required.)
    #[test]
    fn script_owner_materialize_final_leaves_a_script_info_row_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();
        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();
        let key = keys::encode_script_info_key(&[0x6d; 32], 1);
        domain_store
            .put_cf(domain_store.cf_script_info(), &key, b"garbage")
            .unwrap();
        let mut owner = ScriptOwner::default();
        owner.reference_infos.insert(
            (vec![0x6d; 32], 1),
            ScriptReferenceInfo {
                reference_hash: vec![0x6d; 32],
                hash_type: 1,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                ..Default::default()
            },
        );
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();
        assert_eq!(
            domain_store
                .get_cf(domain_store.cf_script_info(), &key)
                .unwrap()
                .as_deref(),
            Some(b"garbage".as_slice())
        );
    }

    /// Lane rd/api-lock-names: the label row label import wrote for a pair
    /// (name, deprecated flag, description) is exactly as it was after the
    /// bulk build: the build writes the pair's usage to its reference row and
    /// no label. (Before, the finalize rewrote the row with the lumped usage,
    /// carried the label fields over and set the hash type to the last
    /// cell's.)
    #[test]
    fn script_owner_materialize_final_preserves_existing_deprecated_flag() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let code_hash = vec![0x44; 32];
        let label = ckbadger_store::ScriptInfo {
            name: "PW Lock".to_string(),
            deprecated: true,
            description: Some("deprecated ethereum-compatible lock".to_string()),
            ..Default::default()
        };
        domain_store
            .put_script_info_direct(&code_hash, 1, &label)
            .unwrap();

        let mut owner = ScriptOwner::default();
        owner.reference_infos.insert(
            (code_hash.clone(), 1),
            ScriptReferenceInfo {
                reference_hash: code_hash.clone(),
                hash_type: 1,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 100_00000000,
                lock_owned_capacity_sum: 100_00000000,
                lock_used_capacity_sum: 61_00000000,
                lock_owned_knowledge_sum: 61_00000000,
                ..Default::default()
            },
        );

        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        assert_eq!(
            domain_store.list_script_infos().unwrap(),
            vec![((code_hash.clone(), 1), label)]
        );
        let usage = domain_store
            .get_script_reference_info(1, &code_hash)
            .unwrap()
            .unwrap();
        assert_eq!(usage.lock_cells_count, 1);
        assert_eq!(usage.lock_live_cells_count, 1);
    }

    #[test]
    fn parse_hash_type_u8_rejects_unsupported_value() {
        let tx = ResolvedTxFacts {
            tx_hash: [0x31; 32],
            block_number: 100,
            block_hash: [0x02; 32],
            timestamp_ms: 1_700_000_000_000,
            chain_time_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: Vec::new().into(),
        };

        let err = parse_hash_type_u8(3, "lock", &tx, InternId::new(0)).unwrap_err();
        assert!(err.to_string().contains("expected_one_of=[0,1,2,4]"));
    }

    #[test]
    fn script_owner_materialize_final_persists_distinct_reference_hash_types() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let reference_hash = vec![0x55; 32];
        let interner = IdentityInterner::default();
        interner.intern_bytes(vec![0xaa; 32]);
        let code_hash_id = interner.intern_bytes(reference_hash.clone());
        interner.intern_bytes(vec![0xab; 20]);
        interner.intern_bytes(vec![0xbb; 32]);
        interner.intern_bytes(vec![0xac; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let tx = ResolvedTxFacts {
            tx_hash: [0x41; 32],
            block_number: 200,
            block_hash: [0x09; 32],
            timestamp_ms: 1_700_000_100_000,
            chain_time_ms: 1_700_000_100_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x41; 32], 0),
                    created_at_block: 200,
                    created_by_block_dao_ar: 1,
                    capacity: 100_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: code_hash_id,
                    lock_hash_type: 0,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x41; 32], 1),
                    created_at_block: 200,
                    created_by_block_dao_ar: 1,
                    capacity: 150_00000000,
                    lock_script_hash_id: InternId::new(3),
                    lock_code_hash_id: code_hash_id,
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(4),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 71_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };

        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx, &ctx).unwrap();

        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        let data_hash_info = domain_store
            .get_script_reference_info(0, &reference_hash)
            .unwrap();
        assert_eq!(
            data_hash_info,
            Some(ScriptReferenceInfo {
                reference_hash: reference_hash.clone(),
                hash_type: 0,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 100_00000000,
                lock_owned_capacity_sum: 100_00000000,
                lock_used_capacity_sum: 61_00000000,
                lock_owned_knowledge_sum: 61_00000000,
                type_cells_count: 0,
                type_live_cells_count: 0,
                type_capacity_sum: 0,
                type_owned_capacity_sum: 0,
                type_used_capacity_sum: 0,
                type_owned_knowledge_sum: 0,
            })
        );

        let type_hash_info = domain_store
            .get_script_reference_info(1, &reference_hash)
            .unwrap();
        assert_eq!(
            type_hash_info,
            Some(ScriptReferenceInfo {
                reference_hash: reference_hash.clone(),
                hash_type: 1,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                lock_capacity_sum: 150_00000000,
                lock_owned_capacity_sum: 150_00000000,
                lock_used_capacity_sum: 71_00000000,
                lock_owned_knowledge_sum: 71_00000000,
                type_cells_count: 0,
                type_live_cells_count: 0,
                type_capacity_sum: 0,
                type_owned_capacity_sum: 0,
                type_used_capacity_sum: 0,
                type_owned_knowledge_sum: 0,
            })
        );
    }

    #[test]
    fn script_owner_materialize_final_rolls_reference_stats_into_version_and_family() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let version_hash = vec![0x66; 32];
        let family_id = "family/test-script";
        domain_store
            .put_script_version(
                &version_hash,
                &ScriptVersionInfo {
                    version_hash: version_hash.clone(),
                    family_id: Some(family_id.to_string()),
                    name: Some("Test Script".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        domain_store
            .put_script_family_direct(
                family_id,
                &ScriptFamilyInfo {
                    family_id: family_id.to_string(),
                    name: "Test Script".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();

        let unrelated_lock_hash = vec![0x77; 32];
        let interner = IdentityInterner::default();
        interner.intern_bytes(vec![0xaa; 32]);
        let version_hash_id = interner.intern_bytes(version_hash.clone());
        interner.intern_bytes(vec![0xab; 20]);
        interner.intern_bytes(vec![0xbb; 32]);
        interner.intern_bytes(vec![0xac; 20]);
        interner.intern_bytes(vec![0xdd; 32]);
        let unrelated_lock_hash_id = interner.intern_bytes(unrelated_lock_hash);
        interner.intern_bytes(vec![0xde; 20]);
        interner.intern_bytes(vec![0xee; 32]);
        interner.intern_bytes(vec![0xef; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let tx = ResolvedTxFacts {
            tx_hash: [0x51; 32],
            block_number: 300,
            block_hash: [0x0a; 32],
            timestamp_ms: 1_700_000_200_000,
            chain_time_ms: 1_700_000_200_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x51; 32], 0),
                    created_at_block: 300,
                    created_by_block_dao_ar: 1,
                    capacity: 100_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: version_hash_id,
                    lock_hash_type: 0,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x51; 32], 1),
                    created_at_block: 300,
                    created_by_block_dao_ar: 1,
                    capacity: 200_00000000,
                    lock_script_hash_id: InternId::new(3),
                    lock_code_hash_id: unrelated_lock_hash_id,
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(4),
                    type_script_hash_id: Some(InternId::new(6)),
                    type_code_hash_id: Some(version_hash_id),
                    type_hash_type: Some(2),
                    type_args_id: Some(InternId::new(7)),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };

        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx, &ctx).unwrap();

        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        assert_eq!(
            domain_store
                .get_script_reference_version_hash(0, &version_hash)
                .unwrap(),
            Some(version_hash.clone())
        );
        assert_eq!(
            domain_store
                .get_script_reference_version_hash(2, &version_hash)
                .unwrap(),
            Some(version_hash.clone())
        );

        let version = domain_store
            .get_script_version(&version_hash)
            .unwrap()
            .expect("version should exist");
        assert_eq!(version.family_id.as_deref(), Some(family_id));
        assert_eq!(version.name.as_deref(), Some("Test Script"));
        assert_eq!(version.lock_cells_count, 1);
        assert_eq!(version.lock_live_cells_count, 1);
        assert_eq!(version.lock_capacity_sum, 100_00000000);
        assert_eq!(version.lock_owned_capacity_sum, 100_00000000);
        assert_eq!(version.lock_used_capacity_sum, 61_00000000);
        assert_eq!(version.lock_owned_knowledge_sum, 61_00000000);
        assert_eq!(version.type_cells_count, 1);
        assert_eq!(version.type_live_cells_count, 1);
        assert_eq!(version.type_capacity_sum, 200_00000000);
        assert_eq!(version.type_owned_capacity_sum, 200_00000000);
        assert_eq!(version.type_used_capacity_sum, 142_00000000);
        assert_eq!(version.type_owned_knowledge_sum, 142_00000000);

        let family = domain_store
            .get_script_family(family_id)
            .unwrap()
            .expect("family should exist");
        assert_eq!(family.versions_count, 1);
        assert_eq!(family.live_cells_count, 2);
        assert_eq!(family.cells_count, 2);
        assert_eq!(family.owned_capacity_sum, 300_00000000);
        assert_eq!(family.owned_knowledge_sum, 203_00000000);
    }

    #[test]
    fn script_owner_materialize_final_keeps_type_reference_mapping_without_live_code_cells() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let reference_hash = vec![0x88; 32];
        let version_hash = vec![0x99; 32];
        let family_id = "family/consumed-script";

        domain_store
            .put_script_version(
                &version_hash,
                &ScriptVersionInfo {
                    version_hash: version_hash.clone(),
                    family_id: Some(family_id.to_string()),
                    name: Some("Consumed Script".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        domain_store
            .put_script_family_direct(
                family_id,
                &ScriptFamilyInfo {
                    family_id: family_id.to_string(),
                    name: "Consumed Script".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();

        let mut owner = ScriptOwner::default();
        owner.reference_infos.insert(
            (reference_hash.clone(), 1),
            ScriptReferenceInfo {
                reference_hash: reference_hash.clone(),
                hash_type: 1,
                lock_cells_count: 3,
                lock_live_cells_count: 0,
                lock_capacity_sum: 300_00000000,
                lock_owned_capacity_sum: 0,
                lock_used_capacity_sum: 183_00000000,
                lock_owned_knowledge_sum: 0,
                type_cells_count: 0,
                type_live_cells_count: 0,
                type_capacity_sum: 0,
                type_owned_capacity_sum: 0,
                type_used_capacity_sum: 0,
                type_owned_knowledge_sum: 0,
            },
        );
        owner
            .type_id_reference_versions
            .insert(reference_hash.clone(), Some(version_hash.clone()));

        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        let resolved = domain_store
            .get_script_reference_version_hash(1, &reference_hash)
            .unwrap();
        assert_eq!(resolved, Some(version_hash.clone()));

        let version = domain_store
            .get_script_version(&version_hash)
            .unwrap()
            .unwrap();
        assert_eq!(version.lock_cells_count, 3);
        assert_eq!(version.lock_live_cells_count, 0);
        assert_eq!(version.lock_capacity_sum, 300_00000000);
        assert_eq!(version.lock_used_capacity_sum, 183_00000000);

        let family = domain_store.get_script_family(family_id).unwrap().unwrap();
        assert_eq!(family.versions_count, 1);
        assert_eq!(family.live_cells_count, 0);
        assert_eq!(family.cells_count, 3);
        assert_eq!(family.owned_capacity_sum, 0);
        assert_eq!(family.owned_knowledge_sum, 0);
    }

    #[test]
    fn script_owner_lineage_rebind_deletes_old_indexes_in_final_snapshot() {
        // Label import ran first and filed the binary under a data-hash family;
        // bulk then observes a registered Type ID deploying it. The final
        // snapshot must tombstone the old index keys through the production
        // bounded sink, adopt the Type ID identity, and leave CF_CELLS alone.
        let dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(dir.path().join("domain")).unwrap();
        let append_store = CkbadgerStore::open_append_only(dir.path().join("append-only")).unwrap();
        let binary = vec![0x42; 32];
        let registered = vec![0x52; 32];
        domain_store
            .put_script_version(
                &binary,
                &ScriptVersionInfo {
                    version_hash: binary.clone(),
                    family_id: Some("metadata-family".into()),
                    name: Some("From Data Hash".into()),
                    registered_references: vec![(binary.clone(), 2)],
                    canonical_reference_hash: Some(binary.clone()),
                    canonical_hash_type: Some(2),
                    deprecated: true,
                    ..Default::default()
                },
            )
            .unwrap();
        domain_store
            .put_script_version_by_family_direct("metadata-family", &binary)
            .unwrap();
        domain_store
            .insert_script_version_by_label("From Data Hash", &binary)
            .unwrap();
        domain_store
            .put_script_version(
                &[0x99; 32],
                &ScriptVersionInfo {
                    version_hash: vec![0x99; 32],
                    family_id: Some("template-family".into()),
                    name: Some("Template Family".into()),
                    registered_references: vec![(registered.clone(), 1)],
                    canonical_reference_hash: Some(registered.clone()),
                    canonical_hash_type: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        for (family_id, name) in [
            ("template-family", "Template Family"),
            ("metadata-family", "From Data Hash"),
        ] {
            domain_store
                .put_script_family_direct(
                    family_id,
                    &ScriptFamilyInfo {
                        family_id: family_id.into(),
                        name: name.into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }

        let interner = IdentityInterner::default();
        let lock_script_hash_id = interner.intern_bytes(vec![0x01; 32]);
        let lock_code_hash_id = interner.intern_bytes(vec![0x02; 32]);
        let lock_args_id = interner.intern_bytes(vec![0x03; 20]);
        let type_script_hash_id = interner.intern_bytes(registered.clone());
        let type_code_hash_id =
            interner.intern_bytes(ckbadger_store::keys::TYPE_ID_CODE_HASH.to_vec());
        let type_args_id = interner.intern_bytes(vec![0x05; 32]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let tx = ResolvedTxFacts {
            tx_hash: [0x61; 32],
            block_number: 400,
            block_hash: [0x11; 32],
            timestamp_ms: 1_700_000_300_000,
            chain_time_ms: 1_700_000_300_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x61; 32], 0),
                created_at_block: 400,
                created_by_block_dao_ar: 1,
                capacity: 120_00000000,
                lock_script_hash_id,
                lock_code_hash_id,
                lock_hash_type: 1,
                lock_args_id,
                type_script_hash_id: Some(type_script_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                occupied_capacity: 80_00000000,
                data_size: 4,
                data: b"\x7fELF".to_vec(),
                data_hash: Some(binary.as_slice().try_into().unwrap()),
                udt_amount: None,
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };
        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx, &ctx).unwrap();
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        let version = domain_store.get_script_version(&binary).unwrap().unwrap();
        assert_eq!(version.family_id.as_deref(), Some("template-family"));
        assert_eq!(version.name.as_deref(), Some("Template Family"));
        assert!(version.registered_references.is_empty());
        assert_eq!(
            (
                version.canonical_reference_hash.as_deref(),
                version.canonical_hash_type
            ),
            (Some(registered.as_slice()), Some(1))
        );
        assert!(!version.deprecated);
        assert!(
            !domain_store
                .list_script_version_hashes_by_family("metadata-family")
                .unwrap()
                .contains(&binary),
            "bulk final snapshot must delete the old by-family key"
        );
        assert!(domain_store
            .list_script_version_hashes_by_family("template-family")
            .unwrap()
            .contains(&binary));
        assert!(
            !domain_store
                .list_script_version_hashes_by_label("From Data Hash")
                .unwrap()
                .contains(&binary),
            "bulk final snapshot must delete the old by-label key"
        );
        assert!(domain_store
            .list_script_version_hashes_by_label("Template Family")
            .unwrap()
            .contains(&binary));
        assert!(
            append_store
                .iterator_cf(append_store.cf_cells(), rocksdb::IteratorMode::Start)
                .next()
                .is_none(),
            "script finalization must not touch the append-only store"
        );
    }

    #[test]
    fn script_owner_binds_only_unused_native_type_id_elf_deployments() {
        for native_type_id in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let domain_store = CkbadgerStore::open_domain(dir.path().join("domain")).unwrap();
            let append_store =
                CkbadgerStore::open_append_only(dir.path().join("append-only")).unwrap();
            let reference_hash = vec![if native_type_id { 0x81 } else { 0x82 }; 32];
            let version_hash = vec![0x91; 32];
            let observed_hash = if native_type_id {
                [0x91; 32]
            } else {
                [0x92; 32]
            };
            let family_id = "family/unused-deployment";
            domain_store
                .put_script_version(
                    &version_hash,
                    &ScriptVersionInfo {
                        version_hash: version_hash.clone(),
                        family_id: Some(family_id.to_string()),
                        registered_references: if native_type_id {
                            Vec::new()
                        } else {
                            vec![(reference_hash.clone(), 1)]
                        },
                        ..Default::default()
                    },
                )
                .unwrap();
            domain_store
                .put_script_family_direct(
                    family_id,
                    &ScriptFamilyInfo {
                        family_id: family_id.to_string(),
                        ..Default::default()
                    },
                )
                .unwrap();
            let interner = IdentityInterner::default();
            let lock_script_hash_id = interner.intern_bytes(vec![0x01; 32]);
            let lock_code_hash_id = interner.intern_bytes(vec![0x02; 32]);
            let lock_args_id = interner.intern_bytes(vec![0x03; 20]);
            let type_script_hash_id = interner.intern_bytes(reference_hash.clone());
            let type_code_hash_id = interner.intern_bytes(if native_type_id {
                ckbadger_store::keys::TYPE_ID_CODE_HASH.to_vec()
            } else {
                vec![0x04; 32]
            });
            let type_args_id = interner.intern_bytes(vec![0x05; 20]);
            let frozen = interner.snapshot_for_reads();
            let ctx = ReducerContext::new(&frozen);
            let tx = ResolvedTxFacts {
                tx_hash: [0x61; 32],
                block_number: 400,
                block_hash: [0x11; 32],
                timestamp_ms: 1_700_000_300_000,
                chain_time_ms: 1_700_000_300_000,
                block_dao_ar: 1,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: Vec::new(),
                cells: vec![CellFacts {
                    outpoint: OutPointKey::new([0x61; 32], 0),
                    created_at_block: 400,
                    created_by_block_dao_ar: 1,
                    capacity: 120_00000000,
                    lock_script_hash_id,
                    lock_code_hash_id,
                    lock_hash_type: 1,
                    lock_args_id,
                    type_script_hash_id: Some(type_script_hash_id),
                    type_code_hash_id: Some(type_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(type_args_id),
                    occupied_capacity: 80_00000000,
                    data_size: 4,
                    data: b"\x7fELF".to_vec(),
                    data_hash: Some(observed_hash),
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                }]
                .into(),
            };
            let mut owner = ScriptOwner::default();
            owner.apply_tx(&tx, &ctx).unwrap();
            assert!(
                !owner
                    .reference_infos
                    .contains_key(&(reference_hash.clone(), 1)),
                "deployment has no users yet"
            );
            let mut materializer = Materializer::new(&domain_store, &append_store);
            owner.materialize_final(&mut materializer).unwrap();
            let _ = materializer.finish();
            assert_eq!(
                domain_store
                    .get_script_reference_version_hash(1, &reference_hash)
                    .unwrap(),
                native_type_id.then(|| version_hash.clone()),
                "unexpected binding for native_type_id={native_type_id}"
            );
            let version = domain_store
                .get_script_version(&version_hash)
                .unwrap()
                .unwrap();
            assert_eq!(version.family_id.as_deref(), Some(family_id));
            assert_eq!(
                version.type_id_deployments,
                if native_type_id {
                    vec![(reference_hash.clone(), 400)]
                } else {
                    Vec::new()
                }
            );
            assert_eq!(
                domain_store
                    .get_script_family(family_id)
                    .unwrap()
                    .unwrap()
                    .versions_count,
                1
            );
            if !native_type_id {
                assert!(
                    domain_store
                        .get_script_version(&observed_hash)
                        .unwrap()
                        .is_none(),
                    "an unrecognized generic binary must not create a version row"
                );
                assert!(
                    owner.type_id_lineage.is_empty(),
                    "generic reference is not native Type ID lineage"
                );
                assert!(owner.type_id_reference_versions.is_empty());
                assert!(owner.type_id_reference_live_versions.is_empty());

                let mut usage_tx = tx.clone();
                usage_tx.tx_hash = [0x62; 32];
                usage_tx.block_number = 401;
                usage_tx.cells = vec![CellFacts {
                    outpoint: OutPointKey::new([0x62; 32], 0),
                    created_at_block: 401,
                    created_by_block_dao_ar: 1,
                    capacity: 150_00000000,
                    lock_script_hash_id,
                    lock_code_hash_id: type_script_hash_id,
                    lock_hash_type: 1,
                    lock_args_id,
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                }]
                .into();
                owner.apply_tx(&usage_tx, &ctx).unwrap();
                let mut materializer = Materializer::new(&domain_store, &append_store);
                owner.materialize_final(&mut materializer).unwrap();
                let _ = materializer.finish();
                assert_eq!(
                    domain_store
                        .get_script_version(&version_hash)
                        .unwrap()
                        .unwrap()
                        .lock_cells_count,
                    0,
                    "a registered generic type reference runs the foreign binary its live code \
                     cell holds, not the registered version"
                );
                assert_eq!(
                    domain_store
                        .get_script_family(family_id)
                        .unwrap()
                        .unwrap()
                        .cells_count,
                    0,
                    "generic type reference usage must stay outside the family"
                );
                // the lead's ruling (lane rd/api-lock-names): what it runs is
                // the one live binary, outside the registry: its usage is that
                // binary's, counted in no family
                assert_eq!(
                    domain_store
                        .get_script_version(&observed_hash)
                        .unwrap()
                        .unwrap()
                        .lock_cells_count,
                    1
                );
            }
            assert_eq!(
                append_store
                    .iterator_cf(append_store.cf_cells(), rocksdb::IteratorMode::Start)
                    .count(),
                0,
                "script snapshot must not write immutable cell payloads"
            );
        }
    }

    #[test]
    fn script_owner_materialize_final_rebinds_type_reference_to_remaining_live_version() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let reference_hash = vec![0x91; 32];
        let live_version_hash = vec![0xb2; 32];
        let registered_version_hash = vec![0xa1; 32];
        let family_id = "family/rebound-script";

        domain_store
            .put_script_version(
                &registered_version_hash,
                &ScriptVersionInfo {
                    version_hash: registered_version_hash.clone(),
                    canonical_reference_hash: Some(reference_hash.clone()),
                    registered_references: vec![(reference_hash.clone(), 1)],
                    canonical_hash_type: Some(1),
                    family_id: Some(family_id.to_string()),
                    name: Some("Rebound Script".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        domain_store
            .put_script_family_direct(
                family_id,
                &ScriptFamilyInfo {
                    family_id: family_id.to_string(),
                    name: "Rebound Script".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();

        let interner = IdentityInterner::default();
        interner.intern_bytes(vec![0x01; 32]);
        interner.intern_bytes(vec![0x02; 32]);
        interner.intern_bytes(vec![0x03; 20]);
        let reference_hash_id = interner.intern_bytes(reference_hash.clone());
        interner.intern_bytes(ckbadger_store::keys::TYPE_ID_CODE_HASH.to_vec());
        interner.intern_bytes(vec![0x05; 20]);
        interner.intern_bytes(vec![0x06; 32]);
        interner.intern_bytes(vec![0x07; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let initial_version_hash = [0xa1; 32];
        let final_version_hash = [0xb2; 32];

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x61; 32],
            block_number: 400,
            block_hash: [0x11; 32],
            timestamp_ms: 1_700_000_300_000,
            chain_time_ms: 1_700_000_300_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x61; 32], 0),
                    created_at_block: 400,
                    created_by_block_dao_ar: 1,
                    capacity: 120_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: InternId::new(1),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: Some(reference_hash_id),
                    type_code_hash_id: Some(InternId::new(4)),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(5)),
                    occupied_capacity: 80_00000000,
                    data_size: 0,
                    data: b"\x7fELF".to_vec(),
                    data_hash: Some(initial_version_hash),
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x61; 32], 1),
                    created_at_block: 400,
                    created_by_block_dao_ar: 1,
                    capacity: 150_00000000,
                    lock_script_hash_id: InternId::new(6),
                    lock_code_hash_id: reference_hash_id,
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(7),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x62; 32],
            block_number: 401,
            block_hash: [0x12; 32],
            timestamp_ms: 1_700_000_400_000,
            chain_time_ms: 1_700_000_400_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x61; 32], 0),
                created_at_block: 400,
                created_by_block_dao_ar: 1,
                capacity: 120_00000000,
                occupied_capacity: 80_00000000,
                data_size: 0,
                data_hash: Some(initial_version_hash),
                udt_amount: None,
                lock_script_hash_id: InternId::new(0),
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(reference_hash_id),
                type_code_hash_id: Some(InternId::new(4)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(5)),
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x62; 32], 0),
                created_at_block: 401,
                created_by_block_dao_ar: 1,
                capacity: 120_00000000,
                lock_script_hash_id: InternId::new(0),
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(reference_hash_id),
                type_code_hash_id: Some(InternId::new(4)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(5)),
                occupied_capacity: 80_00000000,
                data_size: 0,
                data: b"\x7fELF".to_vec(),
                data_hash: Some(final_version_hash),
                udt_amount: None,
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };

        let mut state_only = tx0.clone();
        state_only.cells.to_mut()[0].data.clear();
        let mut state_owner = ScriptOwner::default();
        state_owner.apply_tx(&state_only, &ctx).unwrap();
        assert!(
            state_owner.type_id_lineage.is_empty(),
            "mutable non-ELF state is not code history"
        );
        assert!(state_owner.type_id_reference_live_versions.is_empty());
        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx0, &ctx).unwrap();
        owner.apply_tx(&tx1, &ctx).unwrap();

        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        let resolved = domain_store
            .get_script_reference_version_hash(1, &reference_hash)
            .unwrap();
        assert_eq!(resolved, Some(live_version_hash.clone()));

        let version = domain_store
            .get_script_version(&live_version_hash)
            .unwrap()
            .unwrap();
        assert_eq!(version.lock_cells_count, 1);
        assert_eq!(version.lock_live_cells_count, 1);
        assert_eq!(version.lock_capacity_sum, 150_00000000);
        assert_eq!(version.lock_owned_capacity_sum, 150_00000000);

        let family = domain_store.get_script_family(family_id).unwrap().unwrap();
        assert_eq!(family.versions_count, 2);
        assert_eq!(version.family_id.as_deref(), Some(family_id));
        assert_eq!(
            version.type_id_deployments,
            vec![(reference_hash.clone(), 401)]
        );
        let old = domain_store
            .get_script_version(&registered_version_hash)
            .unwrap()
            .unwrap();
        assert_eq!(old.type_id_deployments, vec![(reference_hash.clone(), 400)]);
        assert_eq!(old.lock_live_cells_count, 0);
        assert_eq!(
            domain_store
                .list_script_version_hashes_by_family(family_id)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(family.cells_count, 1);
        assert_eq!(family.live_cells_count, 1);

        // Real mainnet block 11,993,376 replaces an ELF Type ID with zero bytes.
        // Retirement preserves versions while removing the active executable.
        let mut retired = tx1.clone();
        retired.tx_hash = [0x63; 32];
        retired.block_number = 402;
        retired.resolved_inputs[0].outpoint = OutPointKey::new(tx1.tx_hash, 0);
        retired.resolved_inputs[0].data_hash = Some(final_version_hash);
        retired.cells.to_mut()[0].outpoint = OutPointKey::new(retired.tx_hash, 0);
        retired.cells.to_mut()[0].data = vec![0; 31_560];
        retired.cells.to_mut()[0].data_hash = Some([0xc3; 32]);
        owner.apply_tx(&retired, &ctx).unwrap();
        assert_eq!(
            owner.type_id_reference_versions.get(&reference_hash),
            Some(&None)
        );
        assert!(!owner
            .type_id_reference_live_versions
            .contains_key(&reference_hash));
        let rows = owner.build_final_rows(&domain_store).unwrap().snapshot_rows;
        assert!(!rows.iter().any(|row| row.cf_name
            == ckbadger_store::CF_SCRIPT_REFERENCE_TO_VERSION
            && row.key == keys::encode_script_reference_key(1, &reference_hash)));
        for hash in [&registered_version_hash, &live_version_hash] {
            let row = rows
                .iter()
                .find(|row| row.cf_name == ckbadger_store::CF_SCRIPT_VERSIONS && &row.key == hash)
                .unwrap();
            let info: ScriptVersionInfo = bincode::deserialize(&row.value).unwrap();
            assert_eq!(info.lock_live_cells_count, 0);
            assert_eq!(info.type_id_deployments.len(), 1);
            assert_eq!(info.family_id.as_deref(), Some(family_id));
        }

        // Production bulk materializes this mapping CF exactly once, at
        // finalize, into a fresh store. Labels seed metadata, never bindings.
        let retired_store = CkbadgerStore::open_domain(dir.path().join("retired-domain")).unwrap();
        retired_store
            .put_script_version(&registered_version_hash, &old)
            .unwrap();
        retired_store
            .put_script_family_direct(family_id, &family)
            .unwrap();
        assert_eq!(
            retired_store
                .get_script_reference_version_hash(1, &reference_hash)
                .unwrap(),
            None
        );
        let mut retired_materializer = Materializer::new(&retired_store, &append_store);
        owner.materialize_final(&mut retired_materializer).unwrap();
        let _ = retired_materializer.finish();
        assert_eq!(
            retired_store
                .get_script_reference_version_hash(1, &reference_hash)
                .unwrap(),
            None
        );
        assert_eq!(
            retired_store
                .get_script_family(family_id)
                .unwrap()
                .unwrap()
                .versions_count,
            2
        );
        let retired_family = retired_store.get_script_family(family_id).unwrap().unwrap();
        assert_eq!(
            retired_family.live_cells_count, 1,
            "retired code does not erase the live family user"
        );
        assert_eq!(retired_family.cells_count, 1);

        let mut reactivated = retired.clone();
        reactivated.tx_hash = [0x64; 32];
        reactivated.block_number = 403;
        reactivated.resolved_inputs[0].outpoint = OutPointKey::new(retired.tx_hash, 0);
        reactivated.resolved_inputs[0].data_hash = Some([0xc3; 32]);
        reactivated.cells.to_mut()[0].outpoint = OutPointKey::new(reactivated.tx_hash, 0);
        reactivated.cells.to_mut()[0].data = b"\x7fELFreactivated".to_vec();
        reactivated.cells.to_mut()[0].data_hash = Some([0xd4; 32]);
        owner.apply_tx(&reactivated, &ctx).unwrap();
        assert_eq!(
            owner.type_id_reference_versions.get(&reference_hash),
            Some(&Some(vec![0xd4; 32]))
        );
        let rows = owner.build_final_rows(&domain_store).unwrap().snapshot_rows;
        let row = rows
            .iter()
            .find(|row| {
                row.cf_name == ckbadger_store::CF_SCRIPT_VERSIONS && row.key == vec![0xd4; 32]
            })
            .unwrap();
        let info: ScriptVersionInfo = bincode::deserialize(&row.value).unwrap();
        assert_eq!(info.family_id.as_deref(), Some(family_id));
        assert_eq!(
            info.type_id_deployments,
            vec![(reference_hash.clone(), 403)]
        );
        assert_eq!(info.lock_live_cells_count, 1);
        assert!(!rows
            .iter()
            .any(|row| row.cf_name == ckbadger_store::CF_SCRIPT_VERSIONS
                && row.key == vec![0xc3; 32]));
        let mut burned = reactivated.clone();
        burned.tx_hash = [0x65; 32];
        burned.block_number = 404;
        burned.resolved_inputs[0].outpoint = OutPointKey::new(reactivated.tx_hash, 0);
        burned.resolved_inputs[0].data_hash = Some([0xd4; 32]);
        burned.cells = Vec::new().into();
        owner.apply_tx(&burned, &ctx).unwrap();
        assert_eq!(
            owner.type_id_reference_versions.get(&reference_hash),
            Some(&None)
        );
        assert_eq!(owner.type_id_lineage.len(), 3);
        assert_eq!(
            append_store
                .iterator_cf(append_store.cf_cells(), rocksdb::IteratorMode::Start)
                .count(),
            0
        );
    }

    #[test]
    fn script_owner_materialize_final_leaves_type_reference_ambiguous_with_multi_live_versions() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let reference_hash = vec![0x93; 32];
        let version_a = vec![0xa3; 32];
        let version_b = vec![0xb3; 32];
        let family_id = "family/ambiguous-script";

        for version_hash in [&version_a, &version_b] {
            domain_store
                .put_script_version(
                    version_hash,
                    &ScriptVersionInfo {
                        version_hash: version_hash.clone(),
                        family_id: Some(family_id.to_string()),
                        name: Some("Ambiguous Script".to_string()),
                        registered_references: vec![(reference_hash.clone(), 1)],
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        domain_store
            .put_script_family_direct(
                family_id,
                &ScriptFamilyInfo {
                    family_id: family_id.to_string(),
                    name: "Ambiguous Script".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();

        let interner = IdentityInterner::default();
        interner.intern_bytes(vec![0x11; 32]);
        interner.intern_bytes(vec![0x12; 32]);
        interner.intern_bytes(vec![0x13; 20]);
        let reference_hash_id = interner.intern_bytes(reference_hash.clone());
        interner.intern_bytes(ckbadger_store::keys::TYPE_ID_CODE_HASH.to_vec());
        interner.intern_bytes(vec![0x15; 20]);
        interner.intern_bytes(vec![0x16; 32]);
        interner.intern_bytes(vec![0x17; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x71; 32],
            block_number: 500,
            block_hash: [0x21; 32],
            timestamp_ms: 1_700_000_500_000,
            chain_time_ms: 1_700_000_500_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x71; 32], 0),
                    created_at_block: 500,
                    created_by_block_dao_ar: 1,
                    capacity: 120_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: InternId::new(1),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: Some(reference_hash_id),
                    type_code_hash_id: Some(InternId::new(4)),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(5)),
                    occupied_capacity: 80_00000000,
                    data_size: 0,
                    data: b"\x7fELF".to_vec(),
                    data_hash: Some([0xa3; 32]),
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x71; 32], 1),
                    created_at_block: 500,
                    created_by_block_dao_ar: 1,
                    capacity: 150_00000000,
                    lock_script_hash_id: InternId::new(6),
                    lock_code_hash_id: reference_hash_id,
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(7),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: crate::sync::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x72; 32],
            block_number: 501,
            block_hash: [0x22; 32],
            timestamp_ms: 1_700_000_600_000,
            chain_time_ms: 1_700_000_600_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x72; 32], 0),
                created_at_block: 501,
                created_by_block_dao_ar: 1,
                capacity: 120_00000000,
                lock_script_hash_id: InternId::new(0),
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(reference_hash_id),
                type_code_hash_id: Some(InternId::new(4)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(5)),
                occupied_capacity: 80_00000000,
                data_size: 0,
                data: b"\x7fELF".to_vec(),
                data_hash: Some([0xb3; 32]),
                udt_amount: None,
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };

        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx0, &ctx).unwrap();
        owner.apply_tx(&tx1, &ctx).unwrap();

        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        assert_eq!(
            domain_store
                .get_script_reference_version_hash(1, &reference_hash)
                .unwrap(),
            None
        );

        let version_a_info = domain_store
            .get_script_version(&version_a)
            .unwrap()
            .unwrap();
        assert_eq!(version_a_info.lock_cells_count, 0);
        assert_eq!(version_a_info.lock_live_cells_count, 0);

        let version_b_info = domain_store
            .get_script_version(&version_b)
            .unwrap()
            .unwrap();
        assert_eq!(version_b_info.lock_cells_count, 0);
        assert_eq!(version_b_info.lock_live_cells_count, 0);

        let family = domain_store.get_script_family(family_id).unwrap().unwrap();
        assert_eq!(family.versions_count, 2);
        // Ambiguous executable resolution does not erase the observed family:
        // both code candidates establish the same family for this reference.
        // Its one user must not be counted once per historical version.
        assert_eq!(family.cells_count, 1);
        assert_eq!(family.live_cells_count, 1);
        assert_eq!(family.owned_capacity_sum, 150_00000000);
        assert_eq!(family.owned_knowledge_sum, 61_00000000);
    }

    /// Lane rd/api-lock-names (domain store): CKB names a script by the pair
    /// (code hash, hash type). The bulk owner summed every cell of a code hash
    /// into one `script_info` row and gave it the last cell's hash type: over
    /// a (secp, type) lock cell and a (secp, data) lock cell, the row label
    /// import wrote as "Default Lock" by `type` came out by `data` with both
    /// cells (RAN before the fix: `name=Some("Default Lock") hash_type=0
    /// lock_cells_count=2 lock_capacity_sum=25000000000`). The bulk build
    /// writes no `script_info` row now (the row is the registry's label of one
    /// pair, label import's alone), and each pair's usage is its own
    /// `script_reference_info` row.
    /// The lead's ruling (lane rd/api-lock-names), on the bulk path: a generic
    /// (non Type ID) type reference is counted in a family by the binaries
    /// its live code cells hold, by the one rule (`family_of_form`): two
    /// versions of one family, in it; a binary outside the registry beside a
    /// registered one, in none. The bulk build counted a generic type
    /// reference in no family at all.
    #[test]
    fn bulk_build_counts_a_generic_type_reference_by_its_live_code_cells() {
        let family_live_cells = |binaries: &[[u8; 32]]| -> i64 {
            let dir = tempfile::tempdir().unwrap();
            let domain_path = dir.path().join("domain");
            let append_path = dir.path().join("append-only");
            std::fs::create_dir_all(&domain_path).unwrap();
            std::fs::create_dir_all(&append_path).unwrap();
            let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
            let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();
            domain_store
                .put_script_family_direct(
                    "pair",
                    &ckbadger_store::ScriptFamilyInfo {
                        family_id: "pair".into(),
                        name: "Pair Family".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            for version in [[0x61u8; 32], [0x62; 32]] {
                domain_store
                    .put_script_version(
                        &version,
                        &ckbadger_store::ScriptVersionInfo {
                            version_hash: version.to_vec(),
                            name: Some("Pair Family".into()),
                            family_id: Some("pair".into()),
                            registered_references: vec![(version.to_vec(), 0)],
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }

            let deployment_code = vec![0x13u8; 32];
            let deployment_args = vec![0x5eu8; 4];
            let reference = crate::parser::ScriptParser::compute_script_hash_raw(
                &deployment_code,
                1,
                &deployment_args,
            );
            let interner = IdentityInterner::default();
            let reference_id = interner.intern_bytes(reference.clone());
            let code_id = interner.intern_bytes(deployment_code);
            let args_id = interner.intern_bytes(deployment_args);
            let other_lock_id = interner.intern_bytes(vec![0x9b; 32]);
            let lock_hash_id = interner.intern_bytes(vec![0xaa; 32]);
            let lock_args_id = interner.intern_bytes(vec![0xab; 20]);
            let frozen = interner.snapshot_for_reads();
            let ctx = ReducerContext::new(&frozen);
            let base = |index: u32| CellFacts {
                outpoint: OutPointKey::new([0x41; 32], index),
                created_at_block: 200,
                created_by_block_dao_ar: 1,
                capacity: 150_00000000,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: other_lock_id,
                lock_hash_type: 1,
                lock_args_id,
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            };
            // one code cell per binary under the deployment type script
            let mut cells: Vec<CellFacts> = binaries
                .iter()
                .enumerate()
                .map(|(index, binary)| CellFacts {
                    type_script_hash_id: Some(reference_id),
                    type_code_hash_id: Some(code_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(args_id),
                    data: b"\x7fELF\x02\x01\x01rest".to_vec(),
                    data_size: 11,
                    data_hash: Some(*binary),
                    ..base(index as u32)
                })
                .collect();
            // a cell locked by the reference: the pair's usage
            cells.push(CellFacts {
                lock_code_hash_id: reference_id,
                ..base(binaries.len() as u32)
            });
            let tx = ResolvedTxFacts {
                tx_hash: [0x41; 32],
                block_number: 200,
                block_hash: [0x09; 32],
                timestamp_ms: 1_700_000_100_000,
                chain_time_ms: 1_700_000_100_000,
                block_dao_ar: 1,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: Vec::new(),
                cells: cells.into(),
            };
            let mut owner = ScriptOwner::default();
            owner.apply_tx(&tx, &ctx).unwrap();
            let mut materializer = Materializer::new(&domain_store, &append_store);
            owner.materialize_final(&mut materializer).unwrap();
            let _ = materializer.finish();
            domain_store
                .get_script_family("pair")
                .unwrap()
                .unwrap()
                .live_cells_count
        };
        assert_eq!(
            family_live_cells(&[[0x61; 32], [0x62; 32]]),
            1,
            "two versions of one family: counted in it"
        );
        assert_eq!(
            family_live_cells(&[[0x61; 32]]),
            1,
            "one binary: its family"
        );
        assert_eq!(
            family_live_cells(&[[0x61; 32], [0x6f; 32]]),
            0,
            "a binary outside the registry beside a registered one: no family"
        );
    }

    #[test]
    fn bulk_build_writes_no_script_info_row_and_counts_each_pair_apart() {
        let dir = tempfile::tempdir().unwrap();
        let domain_path = dir.path().join("domain");
        let append_path = dir.path().join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();
        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let secp = hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
            .unwrap();
        let interner = IdentityInterner::default();
        interner.intern_bytes(vec![0xaa; 32]);
        let code_hash_id = interner.intern_bytes(secp.clone());
        interner.intern_bytes(vec![0xab; 20]);
        interner.intern_bytes(vec![0xbb; 32]);
        interner.intern_bytes(vec![0xac; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let cell =
            |index: u32, hash_type: i16, lock_hash: usize, args: usize, capacity: i64| CellFacts {
                outpoint: OutPointKey::new([0x41; 32], index),
                created_at_block: 200,
                created_by_block_dao_ar: 1,
                capacity,
                lock_script_hash_id: InternId::new(lock_hash),
                lock_code_hash_id: code_hash_id,
                lock_hash_type: hash_type,
                lock_args_id: InternId::new(args),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: crate::sync::CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            };
        let tx = ResolvedTxFacts {
            tx_hash: [0x41; 32],
            block_number: 200,
            block_hash: [0x09; 32],
            timestamp_ms: 1_700_000_100_000,
            chain_time_ms: 1_700_000_100_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            // (secp, type) then (secp, data): two different locks.
            cells: vec![
                cell(0, 1, 0, 2, 150_00000000),
                cell(1, 0, 3, 4, 100_00000000),
            ]
            .into(),
        };
        let mut owner = ScriptOwner::default();
        owner.apply_tx(&tx, &ctx).unwrap();
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer).unwrap();
        let _ = materializer.finish();

        let rows = domain_store
            .iterator_cf(domain_store.cf_script_info(), rocksdb::IteratorMode::Start)
            .count();
        assert_eq!(rows, 0, "the bulk build writes no script_info row");
        let by_type = domain_store
            .get_script_reference_info(1, &secp)
            .unwrap()
            .unwrap();
        assert_eq!(
            (by_type.lock_cells_count, by_type.lock_capacity_sum),
            (1, 150_00000000)
        );
        let by_data = domain_store
            .get_script_reference_info(0, &secp)
            .unwrap()
            .unwrap();
        assert_eq!(
            (by_data.lock_cells_count, by_data.lock_capacity_sum),
            (1, 100_00000000)
        );
    }
}
