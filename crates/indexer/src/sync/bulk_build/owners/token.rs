use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::{
    CkbadgerStore, TokenDailyDelta, TokenInfo, CF_ADDR_TOKENS_BY_BALANCE, CF_STATS_TOKEN,
    CF_TOKENS, CF_TOKEN_HOLDERS, CF_TOKEN_HOLDERS_BY_BALANCE,
};
use rocksdb::IteratorMode;
use rustc_hash::FxHashMap;
use serde::Serialize;

use super::{BulkReducer, ReducerContext};
use crate::parser::{ParsedUdtCell, UdtParser};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{
    CellFacts, CellSemanticTag, ResolvedInputFacts, ResolvedTxFacts,
};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct TokenOwner {
    tokens: FxHashMap<Vec<u8>, TokenAccum>,
    /// On-chain max_supply observations collected from omnilock supply info cells.
    max_supply_observations: FxHashMap<Vec<u8>, i128>,
}

impl TokenOwner {
    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::hash_map_bytes(&self.tokens, |type_hash, token| {
            crate::sync::bulk_build::accounting::bytes_vec_bytes(type_hash)
                + token.estimated_bytes()
        }) + crate::sync::bulk_build::accounting::hash_map_bytes(
            &self.max_supply_observations,
            |k, _| crate::sync::bulk_build::accounting::bytes_vec_bytes(k) + 16,
        )
    }

    fn observe_max_supply_from_output(&mut self, cell: &CellFacts, ctx: &ReducerContext<'_>) {
        let lock_code_hash = ctx.resolve_identity(cell.lock_code_hash_id);
        if !crate::sync::token_helpers::is_omnilock_code_hash(lock_code_hash) {
            return;
        }
        let lock_args = ctx.resolve_identity(cell.lock_args_id);
        let Some(supply_info_type_hash) =
            crate::sync::token_helpers::extract_omnilock_supply_info_type_hash(lock_args)
        else {
            return;
        };
        let Some(type_hash_id) = cell.type_script_hash_id else {
            return;
        };
        let type_hash = ctx.resolve_identity(type_hash_id);
        if type_hash != supply_info_type_hash {
            return;
        }
        let Some((max_supply, token_type_hash)) =
            crate::sync::token_helpers::parse_omnilock_supply_info_cell_data(&cell.data)
        else {
            return;
        };
        self.max_supply_observations
            .insert(token_type_hash.to_vec(), max_supply);
    }
}

impl BulkReducer for TokenOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let input_views = tx
            .resolved_inputs
            .iter()
            .filter_map(|input| TokenCellView::from_input(input, ctx, tx).transpose())
            .collect::<Result<Vec<_>>>()?;
        let output_views = tx
            .cells
            .iter()
            .filter_map(|cell| TokenCellView::from_output(cell, ctx, tx).transpose())
            .collect::<Result<Vec<_>>>()?;

        for view in &input_views {
            let token = self
                .tokens
                .entry(view.type_hash.clone())
                .or_insert_with(|| TokenAccum::from_view(view, tx.block_number));
            token.apply_input(view, tx)?;
        }

        for view in &output_views {
            let token = self
                .tokens
                .entry(view.type_hash.clone())
                .or_insert_with(|| TokenAccum::from_view(view, tx.block_number));
            token.apply_output(view, tx)?;
        }

        let parsed_inputs = input_views
            .iter()
            .map(TokenCellView::to_parsed_udt_cell)
            .collect::<Vec<_>>();
        let parsed_outputs = output_views
            .iter()
            .map(TokenCellView::to_parsed_udt_cell)
            .collect::<Vec<_>>();
        let hour_bucket = tx.timestamp_ms / 3_600_000;

        // Collect max_supply observations from omnilock supply info output cells
        for cell in tx.cells.iter() {
            self.observe_max_supply_from_output(cell, ctx);
        }

        for transfer in UdtParser::build_transfers_from_cells(&parsed_inputs, &parsed_outputs) {
            let token = self
                .tokens
                .get_mut(&transfer.type_script_hash)
                .ok_or_else(|| {
                    anyhow!(
                        "token transfer missing live token accumulator in bulk reducer: type_hash=0x{} block={} tx=0x{} tx_index={}",
                        hex::encode(&transfer.type_script_hash),
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index
                    )
                })?;
            token.record_transfer(hour_bucket, &transfer.type_script_hash, tx)?;
        }

        Ok(())
    }

    fn flush_sealed(&mut self, materializer: &mut Materializer<'_>) -> Result<()> {
        let mut rows = Vec::new();
        let mut type_hashes: Vec<&Vec<u8>> = self.tokens.keys().collect();
        type_hashes.sort();

        for type_hash in type_hashes {
            let token = self
                .tokens
                .get(type_hash)
                .expect("sorted token type hash must exist");
            rows.push(MaterializedRow::new(
                CF_STATS_TOKEN,
                keys::encode_token_transfers_key(type_hash),
                token.transfers_count.to_le_bytes().to_vec(),
            ));

            let mut hour_buckets = token.hourly_transfers.iter().collect::<Vec<_>>();
            hour_buckets.sort_by_key(|(hour_bucket, _)| *hour_bucket);
            for (hour_bucket, count) in hour_buckets {
                rows.push(MaterializedRow::new(
                    CF_STATS_TOKEN,
                    keys::encode_token_hourly_key(type_hash, *hour_bucket),
                    count.to_le_bytes().to_vec(),
                ));
            }

            let mut daily_dates = token.daily_deltas.iter().collect::<Vec<_>>();
            daily_dates.sort_by_key(|(date, _)| *date);
            for (date, delta) in daily_dates {
                if delta.owned_capacity_delta == 0 && delta.owned_knowledge_delta == 0 {
                    continue;
                }
                rows.push(MaterializedRow::new(
                    CF_STATS_TOKEN,
                    keys::encode_token_daily_key(type_hash, *date).to_vec(),
                    bincode::serialize(delta)?,
                ));
            }
        }

        if rows.is_empty() {
            return Ok(());
        }

        materializer.stream_sealed_aggregate_rows(&rows)
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        let store = materializer.domain_store();
        let all_type_hashes: Vec<Vec<u8>> = self.tokens.keys().cloned().collect();
        let existing_tokens: FxHashMap<Vec<u8>, TokenInfo> = store
            .get_tokens_batch(&all_type_hashes)?
            .into_iter()
            .filter_map(|(k, v)| v.map(|info| (k, info)))
            .collect();

        let mut rows = Vec::new();
        let mut type_hashes: Vec<&Vec<u8>> = self.tokens.keys().collect();
        type_hashes.sort();

        for type_hash in type_hashes {
            let token = self
                .tokens
                .get(type_hash)
                .expect("sorted token type hash must exist");
            let mut info = token.to_info(type_hash.clone());

            // Apply on-chain max_supply observations (omnilock supply info cells)
            if let Some(&observed) = self.max_supply_observations.get(type_hash) {
                info.max_supply = Some(observed);
            }

            // Preserve label fields from existing store data (written by label import)
            if let Some(existing) = existing_tokens.get(type_hash) {
                if info.name.is_none() {
                    info.name = existing.name.clone();
                }
                if info.symbol.is_none() {
                    info.symbol = existing.symbol.clone();
                }
                if info.decimals.is_none() {
                    info.decimals = existing.decimals;
                }
                if info.icon_url.is_none() {
                    info.icon_url = existing.icon_url.clone();
                }
                if info.description.is_none() {
                    info.description = existing.description.clone();
                }
                if info.max_supply.is_none() {
                    info.max_supply = existing.max_supply;
                }
            }

            rows.push(MaterializedRow::new(
                CF_TOKENS,
                type_hash.clone(),
                bincode::serialize(&info)?,
            ));

            let mut holders: Vec<(&Vec<u8>, &i128)> = token
                .holders
                .iter()
                .filter(|(_, balance)| **balance > 0)
                .collect();
            holders.sort_by(|(lock_a, _), (lock_b, _)| lock_a.cmp(lock_b));

            for (lock_hash, balance) in holders {
                rows.push(MaterializedRow::new(
                    CF_TOKEN_HOLDERS,
                    keys::encode_token_holder_key(type_hash, lock_hash).to_vec(),
                    balance.to_le_bytes().to_vec(),
                ));
                rows.push(MaterializedRow::new(
                    CF_TOKEN_HOLDERS_BY_BALANCE,
                    keys::encode_token_holder_balance_key(type_hash, *balance, lock_hash).to_vec(),
                    Vec::new(),
                ));
                rows.push(MaterializedRow::new(
                    CF_ADDR_TOKENS_BY_BALANCE,
                    keys::encode_addr_token_balance_key(lock_hash, *balance, type_hash).to_vec(),
                    Vec::new(),
                ));
            }
        }

        materializer.materialize_final_snapshot(&rows)
    }
}

#[derive(Debug, Clone, Serialize)]
struct TokenAccum {
    type_code_hash: Vec<u8>,
    hash_type: u8,
    type_args: Vec<u8>,
    standard: &'static str,
    first_seen_block: i64,
    live_supply: i128,
    holders: FxHashMap<Vec<u8>, i128>,
    transfers_count: i64,
    hourly_transfers: FxHashMap<i64, i64>,
    daily_deltas: FxHashMap<u32, TokenDailyDelta>,
}

impl TokenAccum {
    fn estimated_bytes(&self) -> u64 {
        std::mem::size_of::<Self>() as u64
            + crate::sync::bulk_build::accounting::bytes_vec_bytes(&self.type_code_hash)
            + crate::sync::bulk_build::accounting::bytes_vec_bytes(&self.type_args)
            + self.standard.len() as u64
            + crate::sync::bulk_build::accounting::hash_map_bytes(
                &self.holders,
                |lock_hash, amount| {
                    crate::sync::bulk_build::accounting::bytes_vec_bytes(lock_hash)
                        + std::mem::size_of_val(amount) as u64
                },
            )
            + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.hourly_transfers)
            + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.daily_deltas)
    }

    fn from_view(view: &TokenCellView, first_seen_block: i64) -> Self {
        Self {
            type_code_hash: view.type_code_hash.clone(),
            hash_type: view.type_hash_type,
            type_args: view.type_args.clone(),
            standard: view.standard,
            first_seen_block,
            live_supply: 0,
            holders: FxHashMap::default(),
            transfers_count: 0,
            hourly_transfers: FxHashMap::default(),
            daily_deltas: FxHashMap::default(),
        }
    }

    fn apply_input(&mut self, view: &TokenCellView, tx: &ResolvedTxFacts<'_>) -> Result<()> {
        self.ensure_metadata(view, tx)?;
        self.live_supply = checked_next_i128(
            self.live_supply,
            -(view.amount as i128),
            "token live_supply",
            &view.type_hash,
            tx,
        )?;

        let current = *self.holders.get(&view.lock_hash).unwrap_or(&0);
        let next = checked_next_i128(
            current,
            -(view.amount as i128),
            "token holder balance",
            &view.type_hash,
            tx,
        )?;
        if next == 0 {
            self.holders.remove(&view.lock_hash);
        } else {
            self.holders.insert(view.lock_hash.clone(), next);
        }

        self.record_daily_delta(
            keys::timestamp_ms_to_date(tx.timestamp_ms),
            -i128::from(view.capacity),
            -i128::from(view.occupied_capacity),
            &view.type_hash,
            tx,
        )?;

        Ok(())
    }

    fn apply_output(&mut self, view: &TokenCellView, tx: &ResolvedTxFacts<'_>) -> Result<()> {
        self.ensure_metadata(view, tx)?;
        if tx.block_number < self.first_seen_block {
            self.first_seen_block = tx.block_number;
        }
        self.live_supply = checked_next_i128(
            self.live_supply,
            view.amount as i128,
            "token live_supply",
            &view.type_hash,
            tx,
        )?;

        let current = *self.holders.get(&view.lock_hash).unwrap_or(&0);
        let next = checked_next_i128(
            current,
            view.amount as i128,
            "token holder balance",
            &view.type_hash,
            tx,
        )?;
        self.holders.insert(view.lock_hash.clone(), next);

        self.record_daily_delta(
            keys::timestamp_ms_to_date(tx.timestamp_ms),
            i128::from(view.capacity),
            i128::from(view.occupied_capacity),
            &view.type_hash,
            tx,
        )?;
        Ok(())
    }

    fn record_transfer(
        &mut self,
        hour_bucket: i64,
        type_hash: &[u8],
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        self.transfers_count = checked_next_i64(
            self.transfers_count,
            1,
            "token transfers_count",
            type_hash,
            tx,
        )?;

        let current_hourly = *self.hourly_transfers.get(&hour_bucket).unwrap_or(&0);
        let next_hourly =
            checked_next_i64(current_hourly, 1, "token hourly transfers", type_hash, tx)?;
        self.hourly_transfers.insert(hour_bucket, next_hourly);
        Ok(())
    }

    fn record_daily_delta(
        &mut self,
        date_yyyymmdd: u32,
        owned_capacity_delta: i128,
        owned_knowledge_delta: i128,
        type_hash: &[u8],
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<()> {
        if owned_capacity_delta == 0 && owned_knowledge_delta == 0 {
            return Ok(());
        }

        let entry = self.daily_deltas.entry(date_yyyymmdd).or_default();
        entry.owned_capacity_delta = checked_signed_i128(
            entry.owned_capacity_delta,
            owned_capacity_delta,
            "token daily owned_capacity_delta",
            type_hash,
            tx,
        )?;
        entry.owned_knowledge_delta = checked_signed_i128(
            entry.owned_knowledge_delta,
            owned_knowledge_delta,
            "token daily owned_knowledge_delta",
            type_hash,
            tx,
        )?;
        Ok(())
    }

    fn ensure_metadata(&self, view: &TokenCellView, tx: &ResolvedTxFacts<'_>) -> Result<()> {
        if self.type_code_hash != view.type_code_hash
            || self.hash_type != view.type_hash_type
            || self.type_args != view.type_args
            || self.standard != view.standard
        {
            bail!(
                "token reducer metadata mismatch: type_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                hex::encode(&view.type_hash),
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            );
        }
        Ok(())
    }

    fn to_info(&self, _type_hash: Vec<u8>) -> TokenInfo {
        TokenInfo {
            type_code_hash: self.type_code_hash.clone(),
            hash_type: self.hash_type,
            type_args: self.type_args.clone(),
            standard: self.standard.to_string(),
            name: None,
            symbol: None,
            decimals: None,
            total_supply: Some(self.live_supply),
            max_supply: None,
            holders_count: i64::try_from(self.holders.len()).expect("holders count exceeds i64"),
            first_seen_block: self.first_seen_block,
            icon_url: None,
            description: None,
            transfers_count: self.transfers_count,
        }
    }
}

#[derive(Debug, Clone)]
struct TokenCellView {
    type_hash: Vec<u8>,
    type_code_hash: Vec<u8>,
    type_hash_type: u8,
    type_args: Vec<u8>,
    lock_hash: Vec<u8>,
    capacity: i64,
    occupied_capacity: i64,
    amount: u128,
    standard: &'static str,
}

impl TokenCellView {
    fn from_output(
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<Option<Self>> {
        Self::from_parts(
            cell.semantic_tag,
            cell.type_script_hash_id,
            cell.type_code_hash_id,
            cell.type_hash_type,
            cell.type_args_id,
            cell.lock_script_hash_id,
            cell.capacity,
            cell.occupied_capacity,
            cell.udt_amount,
            ctx,
            tx,
            format!(
                "output outpoint=0x{}:{}",
                hex::encode(cell.outpoint.tx_hash),
                cell.outpoint.index
            ),
        )
    }

    fn from_input(
        input: &ResolvedInputFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<Option<Self>> {
        Self::from_parts(
            input.semantic_tag,
            input.type_script_hash_id,
            input.type_code_hash_id,
            input.type_hash_type,
            input.type_args_id,
            input.lock_script_hash_id,
            input.capacity,
            input.occupied_capacity,
            input.udt_amount,
            ctx,
            tx,
            format!(
                "input outpoint=0x{}:{}",
                hex::encode(input.outpoint.tx_hash),
                input.outpoint.index
            ),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        semantic_tag: CellSemanticTag,
        type_script_hash_id: Option<crate::sync::types::InternId>,
        type_code_hash_id: Option<crate::sync::types::InternId>,
        type_hash_type: Option<i16>,
        type_args_id: Option<crate::sync::types::InternId>,
        lock_script_hash_id: crate::sync::types::InternId,
        capacity: i64,
        occupied_capacity: i64,
        udt_amount: Option<u128>,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
        location: String,
    ) -> Result<Option<Self>> {
        let standard = match semantic_tag {
            CellSemanticTag::Sudt => "sudt",
            CellSemanticTag::Xudt => "xudt",
            _ => return Ok(None),
        };

        let Some(amount) = udt_amount else {
            if matches!(semantic_tag, CellSemanticTag::Xudt) {
                return Ok(None);
            }
            bail!(
                "missing UDT amount for fungible token cell: block={}, tx=0x{}, tx_index={}, {}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                location
            );
        };

        let type_hash = ctx
            .resolve_identity(type_script_hash_id.ok_or_else(|| {
                anyhow!(
                    "missing type_script_hash_id for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?)
            .to_vec();
        let type_code_hash = ctx
            .resolve_identity(type_code_hash_id.ok_or_else(|| {
                anyhow!(
                    "missing type_code_hash_id for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?)
            .to_vec();
        let type_hash_type = type_hash_type.ok_or_else(|| {
            anyhow!(
                "missing type_hash_type for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                location
            )
        })?;
        let type_args = ctx
            .resolve_identity(type_args_id.ok_or_else(|| {
                anyhow!(
                    "missing type_args_id for UDT cell: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?)
            .to_vec();
        let lock_hash = ctx.resolve_identity(lock_script_hash_id).to_vec();
        let type_hash_type = u8::try_from(type_hash_type).map_err(|_| {
            anyhow!(
                "UDT cell type_hash_type out of u8 range: type_hash=0x{}, hash_type={}, block={}, tx=0x{}, tx_index={}, {}",
                hex::encode(&type_hash),
                type_hash_type,
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                location
            )
        })?;

        Ok(Some(Self {
            type_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            lock_hash,
            capacity,
            occupied_capacity,
            amount,
            standard,
        }))
    }

    fn to_parsed_udt_cell(&self) -> ParsedUdtCell {
        ParsedUdtCell {
            type_script_hash: self.type_hash.clone(),
            type_code_hash: self.type_code_hash.clone(),
            type_hash_type: i16::from(self.type_hash_type),
            type_args: self.type_args.clone(),
            lock_script_hash: self.lock_hash.clone(),
            amount: self.amount,
            standard: UdtParser::is_udt_code_hash_bytes(
                &self.type_code_hash,
                i16::from(self.type_hash_type),
            )
            .expect("token cell view must carry a recognized UDT standard"),
        }
    }
}

fn checked_next_i128(
    current: i128,
    delta: i128,
    metric: &str,
    type_hash: &[u8],
    tx: &ResolvedTxFacts<'_>,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: type_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: type_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
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
    current: i64,
    delta: i64,
    metric: &str,
    type_hash: &[u8],
    tx: &ResolvedTxFacts<'_>,
) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: type_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: type_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
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
    current: i128,
    delta: i128,
    metric: &str,
    type_hash: &[u8],
    tx: &ResolvedTxFacts<'_>,
) -> Result<i128> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: type_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(type_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct TokenStateSnapshot {
    pub tokens: HashMap<Vec<u8>, TokenInfo>,
    pub token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>>,
    pub addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>>,
    pub token_transfer_counts: HashMap<Vec<u8>, i64>,
    pub token_hourly_transfers: HashMap<Vec<u8>, HashMap<i64, i64>>,
    pub token_daily_deltas: HashMap<Vec<u8>, HashMap<u32, TokenDailyDelta>>,
}

#[doc(hidden)]
pub(crate) fn materialize_token_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<TokenStateSnapshot> {
    let interner = IdentityInterner::default();
    let arena = build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let frozen = interner.snapshot_for_reads();
    let ctx = ReducerContext::new(&frozen);
    let mut owner = TokenOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = super::super::unique_temp_test_dir("bulk-build-token-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let sst_domain = root.join("domain-sst-tmp");
        let sst_append = root.join("append-sst-tmp");
        std::fs::create_dir_all(&sst_domain)?;
        std::fs::create_dir_all(&sst_append)?;
        let mut materializer =
            Materializer::new(&domain_store, &append_store, sst_domain, sst_append);
        owner.flush_sealed(&mut materializer)?;
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();

        let tokens = domain_store
            .list_tokens()?
            .into_iter()
            .collect::<HashMap<_, _>>();

        let mut token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
        let mut token_transfer_counts = HashMap::new();
        let mut token_hourly_transfers = HashMap::new();
        let mut token_daily_deltas = HashMap::new();
        for type_hash in tokens.keys() {
            let holders = domain_store
                .list_token_holders(type_hash, usize::MAX)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            token_holders.insert(type_hash.clone(), holders);

            token_transfer_counts.insert(
                type_hash.clone(),
                domain_store.get_token_transfers_count(type_hash)?,
            );

            let prefix = keys::encode_token_hourly_prefix(type_hash);
            let iter = domain_store.prefix_iterator_cf(domain_store.cf_stats_token(), &prefix);
            let mut hourly = HashMap::new();
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow!(
                        "failed to iterate stats_token hourly rows in token snapshot helper: type_hash=0x{}, error={}",
                        hex::encode(type_hash),
                        e
                    )
                })?;
                if !key.starts_with(prefix.as_slice()) {
                    break;
                }
                if key.len() != 41 {
                    bail!(
                        "invalid token hourly key length in token snapshot helper: type_hash=0x{}, len={}",
                        hex::encode(type_hash),
                        key.len()
                    );
                }
                if value.len() != 8 {
                    bail!(
                        "invalid token hourly value length in token snapshot helper: type_hash=0x{}, len={}",
                        hex::encode(type_hash),
                        value.len()
                    );
                }
                let hour_bucket = i64::from_be_bytes(
                    key[33..41]
                        .try_into()
                        .expect("hour bucket slice length must be 8"),
                );
                let count = i64::from_le_bytes(
                    value[..8]
                        .try_into()
                        .expect("hourly transfer value length must be 8"),
                );
                hourly.insert(hour_bucket, count);
            }
            if !hourly.is_empty() {
                token_hourly_transfers.insert(type_hash.clone(), hourly);
            }

            let daily_deltas = domain_store
                .list_token_daily_deltas(type_hash)?
                .into_iter()
                .collect::<HashMap<_, _>>();
            if !daily_deltas.is_empty() {
                token_daily_deltas.insert(type_hash.clone(), daily_deltas);
            }
        }

        let mut addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
        let iter = domain_store.iterator_cf(
            domain_store.cf_addr_tokens_by_balance(),
            IteratorMode::Start,
        );
        for item in iter {
            let (key, value) = item?;
            if !value.is_empty() {
                bail!(
                    "addr_tokens_by_balance value must be empty in token snapshot helper: value_len={}",
                    value.len()
                );
            }
            let (lock_hash, balance, type_hash) = keys::decode_addr_token_balance_key(&key);
            addr_tokens
                .entry(lock_hash)
                .or_default()
                .insert(type_hash, balance);
        }

        TokenStateSnapshot {
            tokens,
            token_holders,
            addr_tokens,
            token_transfer_counts,
            token_hourly_transfers,
            token_daily_deltas,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{CellFacts, OutPointKey, ResolvedInputFacts};
    use crate::sync::types::InternId;

    #[test]
    fn token_owner_reduces_live_supply_and_holder_removal() {
        let interner = IdentityInterner::default();
        let lock_a = interner.intern_bytes(vec![0xaa; 32]);
        let lock_b = interner.intern_bytes(vec![0xbb; 32]);
        let type_hash_id = interner.intern_bytes(vec![0xcc; 32]);
        let type_code_hash_id =
            interner.intern_bytes(hex::decode(&crate::parser::udt::SUDT_CODE_HASH[2..]).unwrap());
        let type_args_id = interner.intern_bytes(vec![0x12; 32]);
        // Pad interner to cover all InternIds used in test fixtures (up to 99)
        for i in interner.len()..=100 {
            interner.intern_bytes(vec![0xf0, (i >> 8) as u8, i as u8]);
        }
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x41; 32],
            block_number: 100,
            block_hash: [0x03; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x41; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(99),
                lock_hash_type: 1,
                lock_args_id: InternId::new(98),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                occupied_capacity: 142_00000000,
                data_size: 16,
                data: Vec::new(),
                data_hash: None,
                udt_amount: Some(1000),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };
        let tx1 = ResolvedTxFacts {
            tx_hash: [0x42; 32],
            block_number: 100,
            block_hash: [0x03; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 1,
            tx_index: 1,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x41; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                data_size: 0,
                udt_amount: Some(1000),
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(99),
                lock_hash_type: 1,
                lock_args_id: InternId::new(98),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x42; 32], 0),
                    created_at_block: 100,
                    created_by_block_dao_ar: 1,
                    capacity: 200_00000000,
                    lock_script_hash_id: lock_b,
                    lock_code_hash_id: InternId::new(97),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(96),
                    type_script_hash_id: Some(type_hash_id),
                    type_code_hash_id: Some(type_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(type_args_id),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: Some(600),
                    semantic_tag: CellSemanticTag::Sudt,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x42; 32], 1),
                    created_at_block: 100,
                    created_by_block_dao_ar: 1,
                    capacity: 200_00000000,
                    lock_script_hash_id: lock_a,
                    lock_code_hash_id: InternId::new(95),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(94),
                    type_script_hash_id: Some(type_hash_id),
                    type_code_hash_id: Some(type_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(type_args_id),
                    occupied_capacity: 142_00000000,
                    data_size: 16,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: Some(400),
                    semantic_tag: CellSemanticTag::Sudt,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };
        let tx2 = ResolvedTxFacts {
            tx_hash: [0x43; 32],
            block_number: 100,
            block_hash: [0x03; 32],
            timestamp_ms: 1_700_000_000_002,
            block_dao_ar: 1,
            tx_index: 2,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x42; 32], 1),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                data_size: 0,
                udt_amount: Some(400),
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(95),
                lock_hash_type: 1,
                lock_args_id: InternId::new(94),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x43; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                lock_script_hash_id: lock_b,
                lock_code_hash_id: InternId::new(93),
                lock_hash_type: 1,
                lock_args_id: InternId::new(92),
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(type_args_id),
                occupied_capacity: 142_00000000,
                data_size: 16,
                data: Vec::new(),
                data_hash: None,
                udt_amount: Some(400),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };

        let mut owner = TokenOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply tx0");
        owner.apply_tx(&tx1, &ctx).expect("apply tx1");
        owner.apply_tx(&tx2, &ctx).expect("apply tx2");

        let token = owner.tokens.get(&vec![0xcc; 32]).expect("token");
        assert_eq!(token.live_supply, 1000);
        assert_eq!(token.holders.len(), 1);
        assert_eq!(token.holders.get(&vec![0xbb; 32]), Some(&1000));
        assert!(!token.holders.contains_key(&vec![0xaa; 32]));
    }

    #[test]
    fn token_owner_rejects_type_hash_type_out_of_u8_range() {
        let interner = IdentityInterner::default();
        let lock_hash_id = interner.intern_bytes(vec![0xaa; 32]);
        let type_hash_id = interner.intern_bytes(vec![0xbb; 32]);
        let type_code_hash_id =
            interner.intern_bytes(hex::decode(&crate::parser::udt::SUDT_CODE_HASH[2..]).unwrap());
        let type_args_id = interner.intern_bytes(vec![0x11; 32]);
        let lock_code_hash_id = interner.intern_bytes(vec![0x22; 32]);
        let lock_args_id = interner.intern_bytes(vec![0x33; 20]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);

        let tx = ResolvedTxFacts {
            tx_hash: [0x55; 32],
            block_number: 123,
            block_hash: [0x66; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x55; 32], 0),
                created_at_block: 123,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id,
                lock_hash_type: 1,
                lock_args_id,
                type_script_hash_id: Some(type_hash_id),
                type_code_hash_id: Some(type_code_hash_id),
                type_hash_type: Some(i16::from(u8::MAX) + 1),
                type_args_id: Some(type_args_id),
                occupied_capacity: 142_00000000,
                data_size: 16,
                data: Vec::new(),
                data_hash: None,
                udt_amount: Some(1000),
                semantic_tag: CellSemanticTag::Sudt,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };

        let err = TokenOwner::default()
            .apply_tx(&tx, &ctx)
            .expect_err("invalid hash_type should fail fast");
        assert!(err.to_string().contains("out of u8 range"));
        assert!(err.to_string().contains("type_hash"));
    }
}
