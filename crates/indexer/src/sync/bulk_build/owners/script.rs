use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::{
    CkbadgerStore, ScriptDailyDelta, ScriptInfo, ScriptReferenceInfo, CF_SCRIPT_INFO,
    CF_SCRIPT_REFERENCE_INFO, CF_STATS_SCRIPT,
};
use rustc_hash::FxHashMap;

use super::{BulkReducer, ReducerContext};
use crate::db::writer::build_script_reference_rollup_state;
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{CellFacts, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct ScriptOwner {
    infos: FxHashMap<Vec<u8>, ScriptInfo>,
    reference_infos: FxHashMap<(Vec<u8>, u8), ScriptReferenceInfo>,
    type_reference_live_versions: FxHashMap<Vec<u8>, FxHashMap<Vec<u8>, u32>>,
    type_reference_versions: FxHashMap<Vec<u8>, Option<Vec<u8>>>,
    daily_deltas: FxHashMap<(Vec<u8>, bool, u32), ScriptDailyDelta>,
}

enum TypeReferenceResolution {
    Preserve,
    Resolved(Vec<u8>),
    Ambiguous,
}

impl ScriptOwner {
    #[cfg(test)]
    pub(crate) fn infos(&self) -> &FxHashMap<Vec<u8>, ScriptInfo> {
        &self.infos
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::hash_map_bytes(&self.infos, |code_hash, info| {
            crate::sync::bulk_build::accounting::bytes_vec_bytes(code_hash)
                + crate::sync::bulk_build::accounting::serialized_bytes(info)
        }) + crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.reference_infos,
            |(reference_hash, _hash_type), info| {
                crate::sync::bulk_build::accounting::bytes_vec_bytes(reference_hash)
                    + std::mem::size_of::<u8>() as u64
                    + crate::sync::bulk_build::accounting::serialized_bytes(info)
            },
        ) + crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.type_reference_live_versions,
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
            &self.type_reference_versions,
            |reference_hash, version_hash| {
                crate::sync::bulk_build::accounting::bytes_vec_bytes(reference_hash)
                    + version_hash
                        .as_ref()
                        .map(crate::sync::bulk_build::accounting::bytes_vec_bytes)
                        .unwrap_or(0)
            },
        ) + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.daily_deltas)
    }

    fn apply_type_reference_live_version_delta(
        &mut self,
        reference_hash: &[u8],
        version_hash: &[u8],
        delta: i32,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        let reference_key = reference_hash.to_vec();
        let (resolution, should_remove_live_counts) = {
            let version_counts = self
                .type_reference_live_versions
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
                0 => TypeReferenceResolution::Preserve,
                1 => TypeReferenceResolution::Resolved(
                    version_counts
                        .keys()
                        .next()
                        .expect("single live version count must exist")
                        .clone(),
                ),
                _ => TypeReferenceResolution::Ambiguous,
            };
            (resolution, version_counts.is_empty())
        };

        if should_remove_live_counts {
            self.type_reference_live_versions.remove(&reference_key);
            return Ok(());
        }

        match resolution {
            TypeReferenceResolution::Preserve => {}
            TypeReferenceResolution::Resolved(version_hash) => {
                self.type_reference_versions
                    .insert(reference_key, Some(version_hash));
            }
            TypeReferenceResolution::Ambiguous => {
                self.type_reference_versions.insert(reference_key, None);
            }
        }

        Ok(())
    }

    fn record_daily_delta(
        &mut self,
        code_hash: &[u8],
        is_type: bool,
        date_yyyymmdd: u32,
        owned_capacity_delta: i128,
        owned_knowledge_delta: i128,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        if owned_capacity_delta == 0 && owned_knowledge_delta == 0 {
            return Ok(());
        }

        let entry = self
            .daily_deltas
            .entry((code_hash.to_vec(), is_type, date_yyyymmdd))
            .or_default();
        entry.owned_capacity_delta = checked_signed_i128(
            code_hash,
            if is_type { "type" } else { "lock" },
            "daily owned_capacity_delta",
            entry.owned_capacity_delta,
            owned_capacity_delta,
            tx,
        )?;
        entry.owned_knowledge_delta = checked_signed_i128(
            code_hash,
            if is_type { "type" } else { "lock" },
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
            .filter_map(|(code_hash, is_type, date)| {
                let delta = self
                    .daily_deltas
                    .get(&(code_hash.clone(), *is_type, *date))
                    .expect("sorted script daily key must exist");
                (delta.owned_capacity_delta != 0 || delta.owned_knowledge_delta != 0).then_some(
                    MaterializedRow::new(
                        CF_STATS_SCRIPT,
                        keys::encode_script_daily_key(code_hash, *is_type, *date).to_vec(),
                        bincode::serialize(delta)
                            .expect("script daily delta serialization must succeed"),
                    ),
                )
            })
            .collect::<Vec<_>>()
    }

    fn build_snapshot_rows(
        &self,
        domain_store: &CkbadgerStore,
        _append_only_store: &CkbadgerStore,
    ) -> Result<Vec<MaterializedRow>> {
        let mut code_hashes: Vec<&Vec<u8>> = self.infos.keys().collect();
        code_hashes.sort();

        let mut all_rows = code_hashes
            .into_iter()
            .map(|code_hash| {
                let mut info = self
                    .infos
                    .get(code_hash)
                    .expect("sorted code hash must exist in script owner")
                    .clone();

                // Preserve label fields from existing store data (written by label import)
                if let Ok(Some(existing)) = domain_store.get_script_info(code_hash) {
                    if info.name.is_none() {
                        info.name = existing.name;
                    }
                    if !info.deprecated {
                        info.deprecated = existing.deprecated;
                    }
                    if info.category.is_none() {
                        info.category = existing.category;
                    }
                    if info.website.is_none() {
                        info.website = existing.website;
                    }
                    if info.description.is_none() {
                        info.description = existing.description;
                    }
                }

                Ok(MaterializedRow::new(
                    CF_SCRIPT_INFO,
                    code_hash.clone(),
                    bincode::serialize(&info)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let mut reference_keys: Vec<&(Vec<u8>, u8)> = self.reference_infos.keys().collect();
        reference_keys.sort();
        let reference_rows = reference_keys
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
        all_rows.extend(reference_rows);

        let mut mapping_keys = self.reference_infos.keys().cloned().collect::<Vec<_>>();
        mapping_keys.sort();
        let mut reference_mappings = Vec::with_capacity(self.reference_infos.len());
        for (reference_hash, hash_type) in mapping_keys {
            let version_hash = match hash_type {
                0 | 2 | 4 => Some(reference_hash.clone()),
                1 => self.type_reference_versions.get(&reference_hash).cloned().flatten(),
                _ => bail!(
                    "unsupported script reference hash_type during bulk-build materialization: reference_hash=0x{}, hash_type={}",
                    hex::encode(&reference_hash),
                    hash_type
                ),
            };
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
        let reference_info_map: HashMap<(Vec<u8>, u8), ScriptReferenceInfo> = self
            .reference_infos
            .iter()
            .map(|((h, ht), info)| ((h.clone(), *ht), info.clone()))
            .collect();
        let rollups = build_script_reference_rollup_state(
            domain_store,
            reference_mappings,
            reference_info_map,
        )?;

        for (version_hash, info) in rollups.versions {
            all_rows.push(MaterializedRow::new(
                ckbadger_store::CF_SCRIPT_VERSIONS,
                version_hash,
                bincode::serialize(&info)?,
            ));
        }

        for (family_id, info) in rollups.families {
            all_rows.push(MaterializedRow::new(
                ckbadger_store::CF_SCRIPT_FAMILIES,
                family_id.into_bytes(),
                bincode::serialize(&info)?,
            ));
        }

        Ok(all_rows)
    }

    pub(crate) fn build_final_rows(
        &self,
        domain_store: &CkbadgerStore,
        append_only_store: &CkbadgerStore,
    ) -> Result<super::super::materialize::OwnerFinalRows> {
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: self.build_sealed_rows(),
            snapshot_rows: self.build_snapshot_rows(domain_store, append_only_store)?,
        })
    }
}

impl BulkReducer for ScriptOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let mut deltas: FxHashMap<Vec<u8>, ScriptDelta> = FxHashMap::default();
        let mut reference_deltas: FxHashMap<(Vec<u8>, u8), ScriptReferenceDelta> =
            FxHashMap::default();

        for input in &tx.resolved_inputs {
            apply_input_deltas(input, ctx, &mut deltas, tx)?;
            apply_input_reference_deltas(input, ctx, &mut reference_deltas, tx)?;
            if let (Some(type_script_hash_id), Some(data_hash)) =
                (input.type_script_hash_id, input.data_hash)
            {
                self.apply_type_reference_live_version_delta(
                    ctx.resolve_identity(type_script_hash_id),
                    &data_hash,
                    -1,
                    tx,
                )?;
            }
        }
        for cell in tx.cells.iter() {
            apply_output_deltas(cell, ctx, &mut deltas, tx)?;
            apply_output_reference_deltas(cell, ctx, &mut reference_deltas, tx)?;
            if let (Some(type_script_hash_id), Some(data_hash)) =
                (cell.type_script_hash_id, cell.data_hash)
            {
                self.apply_type_reference_live_version_delta(
                    ctx.resolve_identity(type_script_hash_id),
                    &data_hash,
                    1,
                    tx,
                )?;
            }
        }

        let date_yyyymmdd = keys::timestamp_ms_to_date(tx.timestamp_ms);
        for (code_hash, delta) in deltas {
            {
                let info = self
                    .infos
                    .entry(code_hash.clone())
                    .or_insert_with(|| ScriptInfo {
                        code_hash: code_hash.clone(),
                        hash_type: delta.hash_type,
                        ..Default::default()
                    });

                // CKB allows the same code_hash to be referenced with different
                // hash_types (data=0, type=1, data1=2, data2=4). The pipeline
                // path handles this by overwriting; match that behavior here.
                if info.hash_type != delta.hash_type {
                    info.hash_type = delta.hash_type;
                }

                apply_lock_delta(info, &code_hash, &delta, tx)?;
                apply_type_delta(info, &code_hash, &delta, tx)?;
                info.cells_count = info
                    .lock_cells_count
                    .checked_add(info.type_cells_count)
                    .ok_or_else(|| {
                        anyhow!(
                            "script total cells_count overflow: code_hash=0x{}, lock_cells_count={}, type_cells_count={}",
                            hex::encode(&code_hash),
                            info.lock_cells_count,
                            info.type_cells_count
                        )
                    })?;
                info.capacity_used = info
                    .lock_capacity_sum
                    .checked_add(info.type_capacity_sum)
                    .ok_or_else(|| {
                        anyhow!(
                            "script total capacity_used overflow: code_hash=0x{}, lock_capacity_sum={}, type_capacity_sum={}",
                            hex::encode(&code_hash),
                            info.lock_capacity_sum,
                            info.type_capacity_sum
                        )
                    })?;
            }

            self.record_daily_delta(
                &code_hash,
                false,
                date_yyyymmdd,
                delta.lock_owned_capacity_delta,
                delta.lock_owned_knowledge_delta,
                tx,
            )?;
            self.record_daily_delta(
                &code_hash,
                true,
                date_yyyymmdd,
                delta.type_owned_capacity_delta,
                delta.type_owned_knowledge_delta,
                tx,
            )?;
        }

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
        let rows = self.build_snapshot_rows(
            materializer.domain_store(),
            materializer.append_only_store(),
        )?;
        materializer.materialize_final_snapshot(&rows)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ScriptDelta {
    hash_type: u8,
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

fn apply_input_deltas(
    input: &ResolvedInputFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut FxHashMap<Vec<u8>, ScriptDelta>,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    let lock_code_hash = input.lock_code_hash_id;
    let lock_delta = deltas
        .entry(ctx.resolve_identity(lock_code_hash).to_vec())
        .or_default();
    set_or_confirm_hash_type(lock_delta, input.lock_hash_type, "lock", tx, lock_code_hash)?;
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
        let type_delta = deltas
            .entry(ctx.resolve_identity(type_code_hash_id).to_vec())
            .or_default();
        set_or_confirm_hash_type(type_delta, type_hash_type, "type", tx, type_code_hash_id)?;
        type_delta.type_live_cells_delta -= 1;
        type_delta.type_owned_capacity_delta -= i128::from(input.capacity);
        type_delta.type_owned_knowledge_delta -= i128::from(input.occupied_capacity);
    }

    Ok(())
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

fn apply_output_deltas(
    cell: &CellFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut FxHashMap<Vec<u8>, ScriptDelta>,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    let lock_delta = deltas
        .entry(ctx.resolve_identity(cell.lock_code_hash_id).to_vec())
        .or_default();
    set_or_confirm_hash_type(
        lock_delta,
        cell.lock_hash_type,
        "lock",
        tx,
        cell.lock_code_hash_id,
    )?;
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
        let type_delta = deltas
            .entry(ctx.resolve_identity(type_code_hash_id).to_vec())
            .or_default();
        set_or_confirm_hash_type(type_delta, type_hash_type, "type", tx, type_code_hash_id)?;
        type_delta.type_cells_delta += 1;
        type_delta.type_live_cells_delta += 1;
        type_delta.type_capacity_delta += i128::from(cell.capacity);
        type_delta.type_owned_capacity_delta += i128::from(cell.capacity);
        type_delta.type_used_capacity_delta += i128::from(cell.occupied_capacity);
        type_delta.type_owned_knowledge_delta += i128::from(cell.occupied_capacity);
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

fn apply_lock_delta(
    info: &mut ScriptInfo,
    code_hash: &[u8],
    delta: &ScriptDelta,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    info.lock_cells_count = checked_next_i64(
        code_hash,
        "lock",
        "cells_count",
        info.lock_cells_count,
        delta.lock_cells_delta,
        tx,
    )?;
    info.lock_live_cells_count = checked_next_i64(
        code_hash,
        "lock",
        "live_cells_count",
        info.lock_live_cells_count,
        delta.lock_live_cells_delta,
        tx,
    )?;
    info.lock_capacity_sum = checked_next_i128(
        code_hash,
        "lock",
        "capacity_sum",
        info.lock_capacity_sum,
        delta.lock_capacity_delta,
        tx,
    )?;
    info.lock_owned_capacity_sum = checked_next_i128(
        code_hash,
        "lock",
        "owned_capacity_sum",
        info.lock_owned_capacity_sum,
        delta.lock_owned_capacity_delta,
        tx,
    )?;
    info.lock_used_capacity_sum = checked_next_i128(
        code_hash,
        "lock",
        "used_capacity_sum",
        info.lock_used_capacity_sum,
        delta.lock_used_capacity_delta,
        tx,
    )?;
    info.lock_owned_knowledge_sum = checked_next_i128(
        code_hash,
        "lock",
        "owned_knowledge_sum",
        info.lock_owned_knowledge_sum,
        delta.lock_owned_knowledge_delta,
        tx,
    )?;

    if info.lock_used_capacity_sum > info.lock_capacity_sum {
        bail!(
            "script lock used capacity exceeds total: code_hash=0x{}, used_capacity_sum={}, capacity_sum={}",
            hex::encode(code_hash),
            info.lock_used_capacity_sum,
            info.lock_capacity_sum
        );
    }
    if info.lock_owned_knowledge_sum > info.lock_owned_capacity_sum {
        bail!(
            "script lock owned knowledge exceeds total: code_hash=0x{}, owned_knowledge_sum={}, owned_capacity_sum={}",
            hex::encode(code_hash),
            info.lock_owned_knowledge_sum,
            info.lock_owned_capacity_sum
        );
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

fn apply_type_delta(
    info: &mut ScriptInfo,
    code_hash: &[u8],
    delta: &ScriptDelta,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    info.type_cells_count = checked_next_i64(
        code_hash,
        "type",
        "cells_count",
        info.type_cells_count,
        delta.type_cells_delta,
        tx,
    )?;
    info.type_live_cells_count = checked_next_i64(
        code_hash,
        "type",
        "live_cells_count",
        info.type_live_cells_count,
        delta.type_live_cells_delta,
        tx,
    )?;
    info.type_capacity_sum = checked_next_i128(
        code_hash,
        "type",
        "capacity_sum",
        info.type_capacity_sum,
        delta.type_capacity_delta,
        tx,
    )?;
    info.type_owned_capacity_sum = checked_next_i128(
        code_hash,
        "type",
        "owned_capacity_sum",
        info.type_owned_capacity_sum,
        delta.type_owned_capacity_delta,
        tx,
    )?;
    info.type_used_capacity_sum = checked_next_i128(
        code_hash,
        "type",
        "used_capacity_sum",
        info.type_used_capacity_sum,
        delta.type_used_capacity_delta,
        tx,
    )?;
    info.type_owned_knowledge_sum = checked_next_i128(
        code_hash,
        "type",
        "owned_knowledge_sum",
        info.type_owned_knowledge_sum,
        delta.type_owned_knowledge_delta,
        tx,
    )?;

    if info.type_used_capacity_sum > info.type_capacity_sum {
        bail!(
            "script type used capacity exceeds total: code_hash=0x{}, used_capacity_sum={}, capacity_sum={}",
            hex::encode(code_hash),
            info.type_used_capacity_sum,
            info.type_capacity_sum
        );
    }
    if info.type_owned_knowledge_sum > info.type_owned_capacity_sum {
        bail!(
            "script type owned knowledge exceeds total: code_hash=0x{}, owned_knowledge_sum={}, owned_capacity_sum={}",
            hex::encode(code_hash),
            info.type_owned_knowledge_sum,
            info.type_owned_capacity_sum
        );
    }

    Ok(())
}

fn set_or_confirm_hash_type(
    delta: &mut ScriptDelta,
    hash_type: i16,
    script_kind: &str,
    tx: &ResolvedTxFacts<'_>,
    code_hash_id: crate::sync::types::InternId,
) -> Result<()> {
    let next_hash_type = parse_hash_type_u8(hash_type, script_kind, tx, code_hash_id)?;

    if delta == &ScriptDelta::default() {
        delta.hash_type = next_hash_type;
        return Ok(());
    }

    // Same code_hash can appear with different hash_types within one tx
    // (e.g. a lock script used as data hash and a type script used as type hash
    // that happen to share the same code_hash). Accept latest, matching pipeline.
    if delta.hash_type != next_hash_type {
        delta.hash_type = next_hash_type;
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

fn checked_next_i64(
    code_hash: &[u8],
    script_kind: &str,
    metric: &str,
    current: i64,
    delta: i64,
    tx: &ResolvedTxFacts<'_>,
) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
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
    })?;
    if next < 0 {
        bail!(
            "script {} {} underflow: code_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            script_kind,
            metric,
            hex::encode(code_hash),
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

fn checked_next_i128(
    code_hash: &[u8],
    script_kind: &str,
    metric: &str,
    current: i128,
    delta: i128,
    tx: &ResolvedTxFacts<'_>,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
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
    })?;
    if next < 0 {
        bail!(
            "script {} {} underflow: code_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            script_kind,
            metric,
            hex::encode(code_hash),
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
    script_kind: &str,
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

#[doc(hidden)]
pub(crate) fn materialize_script_infos_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<HashMap<Vec<u8>, ScriptInfo>> {
    let interner = IdentityInterner::default();
    let (arena, _) = build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let frozen = interner.snapshot_for_reads();
    let ctx = ReducerContext::new(&frozen);
    let mut owner = ScriptOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = super::super::unique_temp_test_dir("bulk-build-script-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let infos = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();
        domain_store
            .list_script_infos()?
            .into_iter()
            .collect::<HashMap<_, _>>()
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(infos)
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

        let lock_info = owner.infos().get(&lock_code_hash).expect("lock info");
        assert_eq!(lock_info.lock_cells_count, 3);
        assert_eq!(lock_info.lock_live_cells_count, 2);
        assert_eq!(lock_info.lock_capacity_sum, 380_00000000);
        assert_eq!(lock_info.lock_owned_capacity_sum, 180_00000000);
        assert_eq!(lock_info.lock_used_capacity_sum, 264_00000000);
        assert_eq!(lock_info.lock_owned_knowledge_sum, 122_00000000);

        let type_info = owner.infos().get(&type_code_hash).expect("type info");
        assert_eq!(type_info.type_cells_count, 1);
        assert_eq!(type_info.type_live_cells_count, 0);
        assert_eq!(type_info.type_capacity_sum, 200_00000000);
        assert_eq!(type_info.type_owned_capacity_sum, 0);
        assert_eq!(type_info.type_used_capacity_sum, 142_00000000);
        assert_eq!(type_info.type_owned_knowledge_sum, 0);
    }

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
        domain_store
            .put_script_info_direct(
                &code_hash,
                &ScriptInfo {
                    code_hash: code_hash.clone(),
                    hash_type: 1,
                    name: Some("PW Lock".to_string()),
                    deprecated: true,
                    description: Some("deprecated ethereum-compatible lock".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let mut owner = ScriptOwner::default();
        owner.infos.insert(
            code_hash.clone(),
            ScriptInfo {
                code_hash: code_hash.clone(),
                hash_type: 1,
                cells_count: 1,
                capacity_used: 100_00000000,
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

        let updated = domain_store.get_script_info(&code_hash).unwrap().unwrap();
        assert_eq!(updated.name.as_deref(), Some("PW Lock"));
        assert!(updated.deprecated);
        assert_eq!(
            updated.description.as_deref(),
            Some("deprecated ethereum-compatible lock")
        );
        assert_eq!(updated.lock_cells_count, 1);
        assert_eq!(updated.lock_live_cells_count, 1);
    }

    #[test]
    fn parse_hash_type_u8_rejects_unsupported_value() {
        let tx = ResolvedTxFacts {
            tx_hash: [0x31; 32],
            block_number: 100,
            block_hash: [0x02; 32],
            timestamp_ms: 1_700_000_000_000,
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
            .type_reference_versions
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
        let family_id = "family/rebound-script";

        domain_store
            .put_script_version(
                &live_version_hash,
                &ScriptVersionInfo {
                    version_hash: live_version_hash.clone(),
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
        interner.intern_bytes(vec![0x04; 32]);
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
                    data: Vec::new(),
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
                data: Vec::new(),
                data_hash: Some(final_version_hash),
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
        assert_eq!(family.versions_count, 1);
        assert_eq!(family.cells_count, 1);
        assert_eq!(family.live_cells_count, 1);
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
        interner.intern_bytes(vec![0x14; 32]);
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
                    data: Vec::new(),
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
                data: Vec::new(),
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
        assert_eq!(family.cells_count, 0);
        assert_eq!(family.live_cells_count, 0);
        assert_eq!(family.owned_capacity_sum, 0);
        assert_eq!(family.owned_knowledge_sum, 0);
    }
}
