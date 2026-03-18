use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::{
    CkbadgerStore, ScriptDailyDelta, ScriptInfo, CF_SCRIPT_INFO, CF_STATS_SCRIPT,
};

use super::{BulkReducer, ReducerContext};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{CellFacts, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct ScriptOwner {
    infos: HashMap<Vec<u8>, ScriptInfo>,
    daily_deltas: HashMap<(Vec<u8>, bool, u32), ScriptDailyDelta>,
}

impl ScriptOwner {
    #[cfg(test)]
    pub(crate) fn infos(&self) -> &HashMap<Vec<u8>, ScriptInfo> {
        &self.infos
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::hash_map_bytes(&self.infos, |code_hash, info| {
            crate::sync::bulk_build::accounting::bytes_vec_bytes(code_hash)
                + crate::sync::bulk_build::accounting::serialized_bytes(info)
        }) + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.daily_deltas)
    }

    fn record_daily_delta(
        &mut self,
        code_hash: &[u8],
        is_type: bool,
        date_yyyymmdd: u32,
        live_capacity_delta: i128,
        live_used_delta: i128,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        if live_capacity_delta == 0 && live_used_delta == 0 {
            return Ok(());
        }

        let entry = self
            .daily_deltas
            .entry((code_hash.to_vec(), is_type, date_yyyymmdd))
            .or_default();
        entry.live_capacity_delta = checked_signed_i128(
            code_hash,
            if is_type { "type" } else { "lock" },
            "daily live_capacity_delta",
            entry.live_capacity_delta,
            live_capacity_delta,
            tx,
        )?;
        entry.live_used_capacity_delta = checked_signed_i128(
            code_hash,
            if is_type { "type" } else { "lock" },
            "daily live_used_capacity_delta",
            entry.live_used_capacity_delta,
            live_used_delta,
            tx,
        )?;
        Ok(())
    }
}

impl BulkReducer for ScriptOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let mut deltas: HashMap<Vec<u8>, ScriptDelta> = HashMap::new();

        for input in &tx.resolved_inputs {
            apply_input_deltas(input, ctx, &mut deltas, tx)?;
        }
        for cell in tx.cells.iter() {
            apply_output_deltas(cell, ctx, &mut deltas, tx)?;
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

                if info.hash_type != delta.hash_type {
                    bail!(
                        "script reducer hash_type mismatch: code_hash=0x{}, existing_hash_type={}, incoming_hash_type={}, block={}, tx=0x{}, tx_index={}",
                        hex::encode(&code_hash),
                        info.hash_type,
                        delta.hash_type,
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index
                    );
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
                delta.lock_live_capacity_delta,
                delta.lock_live_used_capacity_delta,
                tx,
            )?;
            self.record_daily_delta(
                &code_hash,
                true,
                date_yyyymmdd,
                delta.type_live_capacity_delta,
                delta.type_live_used_capacity_delta,
                tx,
            )?;
        }

        Ok(())
    }

    fn flush_sealed(&mut self, materializer: &mut Materializer<'_>) -> Result<()> {
        let mut daily_keys = self.daily_deltas.keys().collect::<Vec<_>>();
        daily_keys.sort();

        let rows = daily_keys
            .into_iter()
            .filter_map(|(code_hash, is_type, date)| {
                let delta = self
                    .daily_deltas
                    .get(&(code_hash.clone(), *is_type, *date))
                    .expect("sorted script daily key must exist");
                (delta.live_capacity_delta != 0 || delta.live_used_capacity_delta != 0).then_some(
                    MaterializedRow::new(
                        CF_STATS_SCRIPT,
                        keys::encode_script_daily_key(code_hash, *is_type, *date).to_vec(),
                        bincode::serialize(delta)
                            .expect("script daily delta serialization must succeed"),
                    ),
                )
            })
            .collect::<Vec<_>>();

        if rows.is_empty() {
            return Ok(());
        }

        materializer.stream_sealed_aggregate_rows(&rows)
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        let mut code_hashes: Vec<&Vec<u8>> = self.infos.keys().collect();
        code_hashes.sort();

        let rows = code_hashes
            .into_iter()
            .map(|code_hash| {
                let info = self
                    .infos
                    .get(code_hash)
                    .expect("sorted code hash must exist in script owner");
                Ok(MaterializedRow::new(
                    CF_SCRIPT_INFO,
                    code_hash.clone(),
                    bincode::serialize(info)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        materializer.materialize_final_snapshot(&rows)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ScriptDelta {
    hash_type: u8,
    lock_cells_delta: i64,
    lock_live_cells_delta: i64,
    lock_capacity_delta: i128,
    lock_live_capacity_delta: i128,
    lock_used_capacity_delta: i128,
    lock_live_used_capacity_delta: i128,
    type_cells_delta: i64,
    type_live_cells_delta: i64,
    type_capacity_delta: i128,
    type_live_capacity_delta: i128,
    type_used_capacity_delta: i128,
    type_live_used_capacity_delta: i128,
}

fn apply_input_deltas(
    input: &ResolvedInputFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut HashMap<Vec<u8>, ScriptDelta>,
    tx: &ResolvedTxFacts<'_>,
) -> Result<()> {
    let lock_code_hash = input.lock_code_hash_id;
    let lock_delta = deltas
        .entry(ctx.resolve_identity(lock_code_hash).to_vec())
        .or_default();
    set_or_confirm_hash_type(lock_delta, input.lock_hash_type, "lock", tx, lock_code_hash)?;
    lock_delta.lock_live_cells_delta -= 1;
    lock_delta.lock_live_capacity_delta -= i128::from(input.capacity);
    lock_delta.lock_live_used_capacity_delta -= i128::from(input.occupied_capacity);

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
        type_delta.type_live_capacity_delta -= i128::from(input.capacity);
        type_delta.type_live_used_capacity_delta -= i128::from(input.occupied_capacity);
    }

    Ok(())
}

fn apply_output_deltas(
    cell: &CellFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut HashMap<Vec<u8>, ScriptDelta>,
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
    lock_delta.lock_live_capacity_delta += i128::from(cell.capacity);
    lock_delta.lock_used_capacity_delta += i128::from(cell.occupied_capacity);
    lock_delta.lock_live_used_capacity_delta += i128::from(cell.occupied_capacity);

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
        type_delta.type_live_capacity_delta += i128::from(cell.capacity);
        type_delta.type_used_capacity_delta += i128::from(cell.occupied_capacity);
        type_delta.type_live_used_capacity_delta += i128::from(cell.occupied_capacity);
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
    info.lock_live_capacity_sum = checked_next_i128(
        code_hash,
        "lock",
        "live_capacity_sum",
        info.lock_live_capacity_sum,
        delta.lock_live_capacity_delta,
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
    info.lock_live_used_capacity_sum = checked_next_i128(
        code_hash,
        "lock",
        "live_used_capacity_sum",
        info.lock_live_used_capacity_sum,
        delta.lock_live_used_capacity_delta,
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
    if info.lock_live_used_capacity_sum > info.lock_live_capacity_sum {
        bail!(
            "script lock live used capacity exceeds total: code_hash=0x{}, live_used_capacity_sum={}, live_capacity_sum={}",
            hex::encode(code_hash),
            info.lock_live_used_capacity_sum,
            info.lock_live_capacity_sum
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
    info.type_live_capacity_sum = checked_next_i128(
        code_hash,
        "type",
        "live_capacity_sum",
        info.type_live_capacity_sum,
        delta.type_live_capacity_delta,
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
    info.type_live_used_capacity_sum = checked_next_i128(
        code_hash,
        "type",
        "live_used_capacity_sum",
        info.type_live_used_capacity_sum,
        delta.type_live_used_capacity_delta,
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
    if info.type_live_used_capacity_sum > info.type_live_capacity_sum {
        bail!(
            "script type live used capacity exceeds total: code_hash=0x{}, live_used_capacity_sum={}, live_capacity_sum={}",
            hex::encode(code_hash),
            info.type_live_used_capacity_sum,
            info.type_live_capacity_sum
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
    let next_hash_type = u8::try_from(hash_type).map_err(|_| {
        anyhow!(
            "invalid {} script hash_type: code_hash_id={}, hash_type={}, block={}, tx=0x{}, tx_index={}",
            script_kind,
            code_hash_id.as_usize(),
            hash_type,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;

    if delta == &ScriptDelta::default() {
        delta.hash_type = next_hash_type;
        return Ok(());
    }

    if delta.hash_type != next_hash_type {
        bail!(
            "script reducer delta hash_type mismatch: code_hash_id={}, existing_hash_type={}, incoming_hash_type={}, block={}, tx=0x{}, tx_index={}",
            code_hash_id.as_usize(),
            delta.hash_type,
            next_hash_type,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }

    Ok(())
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
    let mut interner = IdentityInterner::default();
    let arena = build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let ctx = ReducerContext::new(&interner);
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{CellFacts, OutPointKey, ResolvedInputFacts};
    use crate::sync::types::InternId;

    #[test]
    fn script_owner_reduces_lock_and_type_live_usage() {
        let lock_code_hash = vec![0x11; 32];
        let type_code_hash = vec![0x22; 32];
        let mut interner = IdentityInterner::default();
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
        let ctx = ReducerContext::new(&interner);

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
        assert_eq!(lock_info.lock_live_capacity_sum, 180_00000000);
        assert_eq!(lock_info.lock_used_capacity_sum, 264_00000000);
        assert_eq!(lock_info.lock_live_used_capacity_sum, 122_00000000);

        let type_info = owner.infos().get(&type_code_hash).expect("type info");
        assert_eq!(type_info.type_cells_count, 1);
        assert_eq!(type_info.type_live_cells_count, 0);
        assert_eq!(type_info.type_capacity_sum, 200_00000000);
        assert_eq!(type_info.type_live_capacity_sum, 0);
        assert_eq!(type_info.type_used_capacity_sum, 142_00000000);
        assert_eq!(type_info.type_live_used_capacity_sum, 0);
    }
}
