use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tracing::warn;

use ckbadger_common::TokenBalance;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{TokenInfo, TokenTransferRecord};

use crate::parser::ParsedUdtTransfer;
use crate::sync::token_helpers::{MaxSupplyObservation, OnchainTokenInfoTimeline};

use ckbadger_store::keys;

use super::entity_stats::{apply_token_daily, EntityStatsOverlay, SharedEntityStatsOverlay};
use crate::sync::types::{EntityDateKey, TokenDailyChanges};
use crate::sync::undo::SharedUndoSeq;

use super::BatchWriter;

/// Batch state for UDT writes.
///
/// The hourly transfer counters are no longer cached here: they live in the
/// batch-wide `EntityStatsOverlay`, which is also what records their per-block
/// undo pre-images. A private cache beside the overlay would be a second source
/// of "the current value".
pub(crate) struct UdtBatchState {
    stats: SharedEntityStatsOverlay,
    undo_seq: SharedUndoSeq,
}

impl BatchWriter {
    pub(crate) fn new_udt_batch_state(
        &self,
        stats: SharedEntityStatsOverlay,
        undo_seq: SharedUndoSeq,
    ) -> UdtBatchState {
        UdtBatchState { stats, undo_seq }
    }

    /// Per-block token daily capacity/knowledge deltas and transaction counts.
    ///
    /// Every mutation goes through the batch overlay, which records the value
    /// as of the end of the previous block the first time a given block touches
    /// a given key. That undo entry is the ONLY thing that restores these rows
    /// after a shallow fork — `should_delete_stats_for_replay` no longer deletes
    /// them (Task 2.4). Every contribution carries at least one transaction,
    /// so every one is written, including a net-zero capacity change.
    pub fn update_token_daily_deltas_batch(
        &self,
        changes: &TokenDailyChanges<EntityDateKey>,
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
            for (type_hash, date_yyyymmdd) in map.keys() {
                prefetch_keys
                    .push(keys::encode_token_daily_key(type_hash, *date_yyyymmdd).to_vec());
            }
        }
        prefetch_keys.sort_unstable();
        prefetch_keys.dedup();
        overlay.prefetch(self.store.as_ref(), &prefetch_keys)?;

        for (block, map) in changes.by_block() {
            for ((type_hash, date_yyyymmdd), contribution) in map {
                let key = keys::encode_token_daily_key(type_hash, *date_yyyymmdd).to_vec();
                let ctx = || {
                    format!(
                        "token_daily type_hash=0x{} date={} block={}",
                        hex::encode(type_hash),
                        date_yyyymmdd,
                        block
                    )
                };
                if contribution.tx_count == 0 {
                    anyhow::bail!("token daily contribution without a transaction: {}", ctx());
                }
                let prev = overlay.current(self.store.as_ref(), &key)?;
                let next = apply_token_daily(prev.as_deref(), contribution, &ctx)?;
                overlay.mutate(self.store.as_ref(), batch, undo_seq, *block, &key, next)?;
            }
        }
        Ok(())
    }

    /// Test-only adapter: run a flat `(entity, delta)` map through the real
    /// per-block writer as a single block — one transaction per entry — then
    /// stage the overlay.
    ///
    /// The value semantics these callers assert (accumulate, fail on a corrupt
    /// existing row) are covered here; the per-block undo semantics by
    /// `entity_stats`' own unit tests and `tests/entity_stats_rollback.rs`.
    #[cfg(test)]
    pub(crate) fn update_token_daily_deltas_batch_flat_for_test(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut by_block = TokenDailyChanges::new();
        for (key, (capacity_delta, knowledge_delta)) in changes {
            by_block.add_tx(1, key.clone(), *capacity_delta, *knowledge_delta, 0)?;
        }
        let mut overlay = EntityStatsOverlay::new();
        let mut undo_seq = HashMap::new();
        self.update_token_daily_deltas_batch(&by_block, &mut overlay, &mut undo_seq, batch)?;
        overlay.stage_final(batch)
    }

    /// Look up UDT cell info for multiple outpoints.
    /// Returns (type_script_hash, type_code_hash, type_hash_type, type_args, lock_script_hash, amount, standard).
    ///
    /// `script_hashes` holds each input's script hashes as the parser derived
    /// them once for the batch; they are read from there, not derived again.
    pub fn get_udt_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        script_hashes: &crate::sync::types::InputScriptHashes,
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String)>>
    {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        // UDT cells are stored in live_cells CF with their cell info.
        // We need to look up the cell info and extract UDT-specific data from the type script.
        // However, live_cells don't store the full UDT amount — that's token-specific.
        // For the RocksDB path, UDT cell data is tracked via the tokens/token_holders CFs,
        // and the cell info in live_cells contains the type_script_hash for lookup.
        //
        // Since the caller needs this for transfer detection (to know what token is being
        // transferred when an input is consumed), we look up the cell info from the store.
        let mut result = HashMap::with_capacity(outpoints.len());

        for &(tx_hash, output_index) in outpoints {
            // UDT transfer inputs must come from pre-batch live state.
            // Do not fall back to consumed_cells here: historical consumed entries
            // can reintroduce already-spent cells and produce false negative deltas.
            let cell_info = self
                .store
                .get_cell(tx_hash, output_index, &self.append_only_store)?;

            if let Some(info) = cell_info {
                let (lock_script_hash, type_script_hash) = script_hashes
                    .get(&(tx_hash.to_vec(), output_index))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "UDT input has no derived script hashes: outpoint=0x{}:{}",
                            hex::encode(tx_hash),
                            output_index
                        )
                    })?;
                // Only include cells that have a type script (UDT cells always have one).
                // A payload has either its whole type script or none of it, so the
                // type script hash and the code hash are present together.
                let (Some(type_script_hash), Some(type_code_hash)) =
                    (*type_script_hash, info.type_code_hash)
                else {
                    continue;
                };

                // Token metadata is the source of truth for whether this typed cell should be
                // treated as UDT input.
                // Without this guard, arbitrary typed cells can be misclassified as UDT burns.
                let Some(token_info) = self.store.get_token(&type_script_hash)? else {
                    continue;
                };
                let Some(standard) =
                    crate::parser::UdtStandard::from_standard_hint(&token_info.standard)
                else {
                    continue;
                };

                let hash_type = token_info.hash_type as i16;
                let type_args = token_info.type_args.clone();

                let amount = match info.udt_amount {
                    Some(amount) => amount,
                    None => {
                        // Owner-mode / non-standard fungible cells (both sUDT and xUDT) can be
                        // typed cells that do not carry a fungible amount. The type script skips
                        // amount validation in owner mode, so these are legitimate on-chain cells
                        // with no trackable amount — skip them from UDT transfer matching rather
                        // than fail-fast the live writer. Consistent with the bulk reducer
                        // (owners/token.rs) and the parse layers, which already warned when they
                        // recorded the missing amount.
                        continue;
                    }
                };
                result.insert(
                    (tx_hash.to_vec(), output_index),
                    (
                        type_script_hash.to_vec(),
                        type_code_hash.to_vec(),
                        hash_type,
                        type_args,
                        lock_script_hash.to_vec(),
                        amount,
                        standard.as_str().to_string(),
                    ),
                );
            }
        }

        Ok(result)
    }

    /// Process a batch of UDT transfers: upsert tokens and update holder balances.
    /// Raw timestamps are preserved in transfer rows; chain times own hourly buckets.
    pub(crate) fn process_udt_transfers_batch_with_state(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
        max_supply_observations: &HashMap<Vec<u8>, MaxSupplyObservation>,
        onchain_token_info: &HashMap<Vec<u8>, OnchainTokenInfoTimeline>,
        block_timestamps: &HashMap<i64, i64>,
        block_chain_times_ms: &HashMap<i64, i64>,
        batch: &mut StoreBatch,
        state: &mut UdtBatchState,
    ) -> Result<()> {
        if transfers.is_empty() {
            // Even if this batch has no transfer deltas, a transaction can still expose
            // supply-info cells that reveal token hard caps or on-chain token metadata.
            // Merge both key sets so a type_hash appearing in both maps is read from DB
            // exactly once — preventing loop 2 from overwriting loop 1's changes.
            let mut all_keys: std::collections::HashSet<&Vec<u8>> =
                std::collections::HashSet::new();
            all_keys.extend(max_supply_observations.keys());
            all_keys.extend(onchain_token_info.keys());
            for type_hash in all_keys {
                if let Some(info) = self.store.get_token(type_hash)? {
                    let updated = TokenRowThroughBatch {
                        type_hash,
                        base: TokenRowBase::Stored(Box::new(info)),
                        transfers: None,
                        max_supply: max_supply_observations.get(type_hash).copied(),
                        onchain: onchain_token_info.get(type_hash),
                    }
                    .stage(self, batch, &state.undo_seq)?;
                    batch.put_token(type_hash, &updated);
                }
            }
            return Ok(());
        }

        // Step 1: Collect unique tokens and aggregate stats
        let mut token_updates: HashMap<Vec<u8>, TokenUpdate> = HashMap::new();
        // Keyed by block so the overlay can record one pre-image per block —
        // a batch-flat `(type_hash, hour)` key cannot say which block moved it.
        let mut hourly_transfer_updates: HashMap<(i64, Vec<u8>, i64), i64> = HashMap::new();

        for (transfer, tx_hash, block_number) in transfers {
            let entry = token_updates
                .entry(transfer.type_script_hash.clone())
                .or_insert_with(|| TokenUpdate {
                    transfer,
                    first_seen_block: *block_number,
                    transfers_by_block: BTreeMap::new(),
                });
            let in_block = entry.transfers_by_block.entry(*block_number).or_insert(0);
            *in_block = in_block.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "token transfers_count overflow: type_hash=0x{}, tx_hash=0x{}, block={}, current={}",
                    hex::encode(&transfer.type_script_hash),
                    hex::encode(tx_hash),
                    block_number,
                    in_block
                )
            })?;

            let chain_time_ms = block_chain_times_ms
                .get(block_number)
                .copied()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing block chain time for UDT transfer aggregation: type_hash=0x{}, tx_hash=0x{}, block_number={}",
                        hex::encode(&transfer.type_script_hash),
                        hex::encode(tx_hash),
                        block_number
                    )
                })?;
            let hour_bucket = ckbadger_common::stats_bucket::stats_hour_index(chain_time_ms)?;
            *hourly_transfer_updates
                .entry((
                    *block_number,
                    transfer.type_script_hash.clone(),
                    hour_bucket,
                ))
                .or_insert(0) += 1;
        }

        // Step 2: Upsert token metadata and transfer counts. Live supply and holder count
        // are derived exclusively from CF_TOKEN_HOLDERS.
        for (type_hash, update) in &token_updates {
            let base = match self.store.get_token(type_hash)? {
                Some(info) => TokenRowBase::Stored(Box::new(info)),
                None => TokenRowBase::CreatedBy {
                    transfer: update.transfer,
                    block_number: update.first_seen_block,
                },
            };
            // The stats CF counter is the source of truth for accumulation;
            // the row's `transfers_count` is its copy.
            let transfers_before = self.store.get_token_transfers_count(type_hash)?;
            let updated = TokenRowThroughBatch {
                type_hash,
                base,
                transfers: Some(TokenTransfersThroughBatch {
                    before: transfers_before,
                    by_block: &update.transfers_by_block,
                }),
                max_supply: max_supply_observations.get(type_hash).copied(),
                onchain: onchain_token_info.get(type_hash),
            }
            .stage(self, batch, &state.undo_seq)?;
            batch.put_token(type_hash, &updated);
            batch.put_token_transfers_count(type_hash, updated.transfers_count);
        }

        // Step 2.5: Apply observed max_supply to tokens not touched by transfers in this batch.
        for type_hash in max_supply_observations.keys() {
            if token_updates.contains_key(type_hash) {
                continue;
            }

            if let Some(info) = self.store.get_token(type_hash)? {
                let before = info.max_supply;
                let updated = TokenRowThroughBatch {
                    type_hash,
                    base: TokenRowBase::Stored(Box::new(info)),
                    transfers: None,
                    max_supply: max_supply_observations.get(type_hash).copied(),
                    onchain: None,
                }
                .stage(self, batch, &state.undo_seq)?;
                if updated.max_supply != before {
                    batch.put_token(type_hash, &updated);
                }
            }
        }

        // Step 2.6: Update per-hour transfer counts using each transfer's block
        // timestamp, in ascending block order so each block's undo pre-image is
        // the previous block's value.
        let mut hourly_in_block_order: Vec<((i64, Vec<u8>, i64), i64)> =
            hourly_transfer_updates.into_iter().collect();
        hourly_in_block_order.sort_by(|a, b| a.0.cmp(&b.0));
        for ((block_number, type_hash, hour_bucket), count_delta) in hourly_in_block_order {
            let key = ckbadger_store::keys::encode_token_hourly_key(&type_hash, hour_bucket);
            state.stats.mutate_hourly(
                self.store.as_ref(),
                batch,
                &state.undo_seq,
                block_number,
                &key,
                count_delta,
                &|| {
                    format!(
                        "token_hourly type_hash=0x{} hour_bucket={} block={}",
                        hex::encode(&type_hash),
                        hour_bucket,
                        block_number
                    )
                },
            )?;
        }

        // Step 3: Aggregate balance changes per (type_hash, lock_hash) as exact
        // `(received, sent)` TokenBalance sums.
        let mut balance_changes: HashMap<(Vec<u8>, Vec<u8>), (TokenBalance, TokenBalance)> =
            HashMap::new();

        for (transfer, tx_hash, _block_number) in transfers {
            let type_hash = &transfer.type_script_hash;

            if let Some(ref from_lock) = transfer.from_lock_hash {
                if !from_lock.is_empty() {
                    let entry = balance_changes
                        .entry((type_hash.clone(), from_lock.clone()))
                        .or_default();
                    let current_sent = entry.1.clone();
                    entry.1 = current_sent
                        .checked_add(&TokenBalance::from(transfer.amount))
                        .ok_or_else(|| {
                        anyhow::anyhow!(
                            "token holder sent-amount overflow: type_hash=0x{}, lock_hash=0x{}, tx_hash=0x{}, sent={}, amount={}",
                            hex::encode(type_hash),
                            hex::encode(from_lock),
                            hex::encode(tx_hash),
                            current_sent,
                            transfer.amount
                        )
                        })?;
                }
            }

            if !transfer.to_lock_hash.is_empty() {
                let entry = balance_changes
                    .entry((type_hash.clone(), transfer.to_lock_hash.clone()))
                    .or_default();
                let current_received = entry.0.clone();
                entry.0 = current_received
                    .checked_add(&TokenBalance::from(transfer.amount))
                    .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token holder received-amount overflow: type_hash=0x{}, lock_hash=0x{}, tx_hash=0x{}, received={}, amount={}",
                        hex::encode(type_hash),
                        hex::encode(&transfer.to_lock_hash),
                        hex::encode(tx_hash),
                        current_received,
                        transfer.amount
                    )
                    })?;
            }
        }

        // Step 4: Apply balance changes. Holder count is read from the resulting holder CF.
        for ((type_hash, lock_hash), (received, sent)) in &balance_changes {
            let existing = self.store.get_token_holder_balance(type_hash, lock_hash)?;
            let old_balance = existing.unwrap_or_else(TokenBalance::zero);
            // Net-difference apply; checked_sub None is the preserved balance-underflow
            // guard (was `new_balance < 0`).
            let new_balance = if received >= sent {
                let net = received
                    .checked_sub(sent)
                    .expect("ordered TokenBalance subtraction");
                old_balance.checked_add(&net).ok_or_else(|| {
                    anyhow::anyhow!(
                        "token holder balance overflow: type_hash=0x{}, lock_hash=0x{}, balance={}, received={}, sent={}",
                        hex::encode(type_hash),
                        hex::encode(lock_hash),
                        old_balance,
                        received,
                        sent
                    )
                })?
            } else {
                let net = sent
                    .checked_sub(received)
                    .expect("ordered TokenBalance subtraction");
                old_balance.checked_sub(&net).ok_or_else(|| {
                    anyhow::anyhow!(
                        "token holder balance underflow: type_hash=0x{}, lock_hash=0x{}, balance={}, received={}, sent={}",
                        hex::encode(type_hash),
                        hex::encode(lock_hash),
                        old_balance,
                        received,
                        sent
                    )
                })?
            };

            if !old_balance.is_zero() {
                batch.delete_token_holder_by_balance(type_hash, lock_hash, &old_balance);
                batch.delete_addr_token_by_balance(lock_hash, type_hash, &old_balance);
            }

            if !new_balance.is_zero() {
                batch.put_token_holder(type_hash, lock_hash, &new_balance);
                batch.put_token_holder_by_balance(type_hash, lock_hash, &new_balance);
                batch.put_addr_token_by_balance(lock_hash, type_hash, &new_balance);
            } else {
                batch.delete_token_holder(type_hash, lock_hash);
            }
        }

        // Step 6: Write individual transfer records for the token transfers tab.
        // Use a per-(type_hash, block_number) counter as tx_idx to generate unique keys.
        let mut transfer_idx: HashMap<(Vec<u8>, i64), i32> = HashMap::new();
        for (transfer, tx_hash, block_number) in transfers {
            let idx = transfer_idx
                .entry((transfer.type_script_hash.clone(), *block_number))
                .or_insert(0);
            let timestamp = block_timestamps
                .get(block_number)
                .copied()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing block timestamp for token transfer record: type_hash=0x{}, tx_hash=0x{}, block_number={}",
                        hex::encode(&transfer.type_script_hash),
                        hex::encode(tx_hash),
                        block_number
                    )
                })?;
            let record = TokenTransferRecord {
                tx_hash: tx_hash.to_vec(),
                block_number: *block_number,
                from_lock_hash: transfer.from_lock_hash.clone(),
                to_lock_hash: transfer.to_lock_hash.clone(),
                amount: transfer.amount,
                is_mint: transfer.is_mint,
                is_burn: transfer.is_burn,
                timestamp,
            };
            batch.put_token_transfer(&transfer.type_script_hash, *block_number, *idx, &record);
            *idx += 1;
        }

        Ok(())
    }
}

/// The first observed max supply fills an unknown one; a conflicting
/// observation keeps the stored value (reported once, by the final row).
/// Both sync paths apply on-chain max-supply claims through this rule: live
/// from the batch writer, bulk from `TokenOwner::emit_snapshot_rows`.
pub(crate) fn apply_observed_max_supply(
    type_hash: &[u8],
    info: &mut TokenInfo,
    observed: u128,
    report: bool,
) {
    match info.max_supply {
        Some(existing) if existing != observed => {
            if report {
                warn!(
                    token_type_hash = %hex::encode(type_hash),
                    existing_max_supply = existing,
                    observed_max_supply = observed,
                    "conflicting on-chain max supply observation; keeping existing value"
                );
            }
        }
        _ => {
            info.max_supply = Some(observed);
        }
    }
}

struct TokenUpdate<'a> {
    transfer: &'a ParsedUdtTransfer,
    first_seen_block: i64,
    /// Transfers of this token per block of the batch.
    transfers_by_block: BTreeMap<i64, i64>,
}

/// Where a token's row starts from in this batch.
enum TokenRowBase<'a> {
    /// The row the store held before the batch.
    Stored(Box<TokenInfo>),
    /// No row yet: the token's first transfer in the batch creates it, at
    /// that transfer's block.
    CreatedBy {
        transfer: &'a ParsedUdtTransfer,
        block_number: i64,
    },
}

/// The `CF_STATS_TOKEN` transfer counter before the batch, plus this batch's
/// transfers per block.
struct TokenTransfersThroughBatch<'a> {
    before: i64,
    by_block: &'a BTreeMap<i64, i64>,
}

impl TokenTransfersThroughBatch<'_> {
    fn through(&self, type_hash: &[u8], block_number: i64) -> Result<i64> {
        let mut total = self.before;
        for (block, count) in self.by_block.range(..=block_number) {
            total = total.checked_add(*count).ok_or_else(|| {
                anyhow::anyhow!(
                    "token total transfer count overflow: type_hash=0x{}, block={}, current={}, added={}",
                    hex::encode(type_hash),
                    block,
                    total,
                    count
                )
            })?;
        }
        Ok(total)
    }
}

/// One token's row through one live batch (J4).
///
/// The live writer applies a batch's metadata observations batch-flat, but a
/// shallow rollback can land on any block inside the batch. [`Self::row_through`]
/// is the one rule for "the row as of the end of block N": the forward write
/// stores `row_through(last block)`, and every block that changes the row's
/// metadata — or creates it — records `row_through(N - 1)` as its undo pre-image,
/// so undo replay restores the row exactly. A block that changes only the
/// transfer count records nothing: rollback stage 9d reverses the count from
/// the deleted transfer rows.
struct TokenRowThroughBatch<'a> {
    type_hash: &'a [u8],
    base: TokenRowBase<'a>,
    /// `None` for a row this batch does not count transfers into.
    transfers: Option<TokenTransfersThroughBatch<'a>>,
    max_supply: Option<MaxSupplyObservation>,
    onchain: Option<&'a OnchainTokenInfoTimeline>,
}

impl TokenRowThroughBatch<'_> {
    /// The row as of the end of `block_number`; `None` while it does not exist.
    /// `report` logs conflicts and divergences — only for the final row, so
    /// each is reported once.
    fn row_through(&self, block_number: i64, report: bool) -> Result<Option<TokenInfo>> {
        let mut row = match &self.base {
            TokenRowBase::Stored(row) => TokenInfo::clone(row),
            TokenRowBase::CreatedBy {
                transfer,
                block_number: created_at,
            } => {
                if block_number < *created_at {
                    return Ok(None);
                }
                TokenInfo {
                    type_code_hash: transfer.type_code_hash.clone(),
                    hash_type: transfer.type_hash_type as u8,
                    type_args: transfer.type_args.clone(),
                    standard: transfer.standard.as_str().to_string(),
                    name: None,
                    symbol: None,
                    decimals: None,
                    max_supply: None,
                    first_seen_block: *created_at,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                    metadata_source: None,
                    chain_metadata: None,
                }
            }
        };
        if let Some(observed) = self
            .max_supply
            .filter(|observed| observed.block_number <= block_number)
        {
            apply_observed_max_supply(self.type_hash, &mut row, observed.max_supply, report);
        }
        if let Some(bound) = self
            .onchain
            .and_then(|timeline| timeline.winner_through(block_number))
        {
            if report {
                apply_onchain_token_info(self.type_hash, &mut row, Some(bound));
            } else {
                row.merge_chain_metadata(&bound.chain_metadata());
            }
        }
        if let Some(transfers) = &self.transfers {
            row.transfers_count = transfers.through(self.type_hash, block_number)?;
        }
        Ok(Some(row))
    }

    /// Record one undo pre-image per block that changes the row's metadata or
    /// creates it, and return the row after the whole batch.
    fn stage(
        &self,
        writer: &BatchWriter,
        batch: &mut StoreBatch,
        undo_seq: &SharedUndoSeq,
    ) -> Result<TokenInfo> {
        let mut change_blocks = BTreeSet::new();
        if let TokenRowBase::CreatedBy { block_number, .. } = &self.base {
            change_blocks.insert(*block_number);
        }
        if let Some(observed) = self.max_supply {
            change_blocks.insert(observed.block_number);
        }
        if let Some(timeline) = self.onchain {
            change_blocks.extend(timeline.blocks());
        }
        for block_number in change_blocks {
            let before = self.row_through(block_number - 1, false)?;
            let after = self.row_through(block_number, false)?;
            if without_transfer_count(&before) == without_transfer_count(&after) {
                continue;
            }
            let pre_image = before
                .map(|row| super::undo_pre_image(&row, "token", self.type_hash, block_number))
                .transpose()?;
            writer.record_object_undo(
                batch,
                block_number,
                ckbadger_store::CF_TOKENS,
                self.type_hash,
                pre_image,
                undo_seq,
            );
        }
        self.row_through(i64::MAX, true)?.ok_or_else(|| {
            anyhow::anyhow!(
                "token row missing after the batch that writes it: type_hash=0x{}",
                hex::encode(self.type_hash)
            )
        })
    }
}

/// A row's metadata and existence: everything but the transfer count.
fn without_transfer_count(row: &Option<TokenInfo>) -> Option<TokenInfo> {
    row.clone().map(|mut row| {
        row.transfers_count = 0;
        row
    })
}

/// How a token's on-chain Unique Cell metadata merges into the row about to
/// be stored. Both sync modes call this: live sync from the batch writer,
/// bulk sync from `TokenOwner::emit_snapshot_rows`. The rule itself —
/// precedence by binding, the stored source, the kept chain values — is
/// `TokenInfo::merge_chain_metadata` (`ckbadger-store` `token_metadata.rs`),
/// the one definition label import and the API also read (IV-26).
///
/// `info` must already carry whatever the store holds for this token, because
/// precedence is decided against it.
///
/// Whichever side wins, a *disagreement* is reported and persisted (the
/// stored source plus `chain_metadata`, which the API serves as `conflict`).
/// This is the only moment in the process where both values exist: label
/// import runs before the first block is indexed, so the conflict check it
/// performs can only compare a label against the label it wrote itself.
pub(crate) fn apply_onchain_token_info(
    type_hash: &[u8],
    info: &mut TokenInfo,
    bound: Option<&crate::sync::token_helpers::OnchainTokenInfo>,
) {
    let Some(bound) = bound else {
        return;
    };
    if let Some(divergence) = info.merge_chain_metadata(&bound.chain_metadata()) {
        warn!(
            token_type_hash = %hex::encode(type_hash),
            binding = ?bound.binding,
            %divergence,
            authority = match bound.binding {
                crate::sync::token_helpers::OnchainInfoBinding::XudtExtension => "chain",
                crate::sync::token_helpers::OnchainInfoBinding::IssuanceCooccurrence => "stored",
            },
            "stored token metadata disagrees with the token's own on-chain Unique Cell \
             (shown as `stored -> chain`); if the stored value came from a bundled label, \
             the label contradicts the chain and the correction belongs upstream in \
             token-labels"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{LiveCellInfo, TokenInfo};
    use ckbadger_store::CkbadgerStore;

    /// One batch's UDT writer state, built the way `write_parsed_batch` builds
    /// it (one overlay, ONE undo sequence), around one transfer pass.
    fn process_transfers_in_one_batch(
        writer: &BatchWriter,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
        max_supply_observations: &HashMap<Vec<u8>, MaxSupplyObservation>,
        onchain_token_info: &HashMap<Vec<u8>, OnchainTokenInfoTimeline>,
        block_timestamps: &HashMap<i64, i64>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let entity_stats = SharedEntityStatsOverlay::new();
        let mut state = writer.new_udt_batch_state(entity_stats.clone(), SharedUndoSeq::default());
        writer.process_udt_transfers_batch_with_state(
            transfers,
            max_supply_observations,
            onchain_token_info,
            block_timestamps,
            block_timestamps,
            batch,
            &mut state,
        )?;
        entity_stats.stage_final(batch)
    }

    #[test]
    /// W3: a token day counts its transactions, so a day whose capacity nets
    /// to zero keeps its row (with the count) instead of being deleted.
    fn test_update_token_daily_deltas_batch_accumulates_and_keeps_a_net_zero_day() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let type_hash = vec![0xAA; 32];

        let mut first = HashMap::new();
        first.insert((type_hash.clone(), 20240115u32), (100i128, 60i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch_flat_for_test(&first, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut second = HashMap::new();
        second.insert((type_hash.clone(), 20240115u32), (-20i128, -10i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch_flat_for_test(&second, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_token_daily_delta(&type_hash, 20240115)
            .unwrap()
            .unwrap();
        assert_eq!(delta.owned_capacity_delta, 80);
        assert_eq!(delta.owned_knowledge_delta, 50);
        assert_eq!(delta.tx_count, 2);

        let mut third = HashMap::new();
        third.insert((type_hash.clone(), 20240115u32), (-80i128, -50i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch_flat_for_test(&third, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_token_daily_delta(&type_hash, 20240115)
            .unwrap()
            .expect("a day with three transactions keeps its row");
        assert_eq!(
            delta,
            ckbadger_store::TokenDailyDelta {
                owned_capacity_delta: 0,
                owned_knowledge_delta: 0,
                tx_count: 3,
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_update_token_daily_deltas_batch_fails_on_corrupted_existing_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let type_hash = vec![0xAC; 32];
        let date = 20240115u32;

        let key = ckbadger_store::keys::encode_token_daily_key(&type_hash, date);
        store
            .put_stats_key(&key, b"not-a-valid-bincode-token-delta")
            .unwrap();

        let mut changes = HashMap::new();
        changes.insert((type_hash, date), (1i128, 1i128));
        let mut batch = StoreBatch::new(&store);
        let err = writer
            .update_token_daily_deltas_batch_flat_for_test(&changes, &mut batch)
            .unwrap_err();
        // Same guard, raised by the token daily codec's width check, which
        // still names the family and the entity.
        assert!(
            err.to_string()
                .contains("entity daily delta has 31 bytes, expected 136")
                && err.to_string().contains("token_daily type_hash="),
            "got: {err}"
        );
    }

    /// A live cell whose payload carries type args but no type code hash (the
    /// shape a pre-v2 payload could decode to through `#[serde(default)]`) can
    /// no longer be resolved through the token's metadata: the v2 payload holds
    /// its whole type script or none, and the store refuses to write (or read)
    /// one that does not, naming the outpoint, so UDT matching never sees it.
    #[test]
    fn test_get_udt_cells_info_batch_never_sees_a_payload_without_type_code_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());

        let tx_hash = vec![0xEF; 32];
        let output_index = 0i16;
        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_code_hash: [0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_code_hash: None,
            type_hash_type: None,
            type_args: Some(vec![0x11; 20]),
            data_size: 16,
            udt_amount: Some(1234),
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        let error = batch
            .put_cell_payload_by_outpoint(&tx_hash, output_index, &cell)
            .expect_err("a partial type script payload must not be stored")
            .to_string();
        assert!(error.contains("type script is partial"), "{error}");
        assert!(error.contains(&hex::encode(&tx_hash)), "{error}");
        assert!(batch.is_empty(), "nothing staged");
    }

    #[test]
    fn test_get_udt_cells_info_batch_does_not_use_consumed_cell_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_code_hash = [0xCB; 32];
        let tx_hash = vec![0xDC; 32];
        let output_index = 0i16;

        let token = TokenInfo {
            type_code_hash: type_code_hash.to_vec(),
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: None,
            chain_metadata: None,
        };

        // The outpoint exists only in consumed_cells (already spent), not live_cells.
        // UDT input lookup must ignore it to avoid replaying historical spend deltas.
        let consumed_cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_code_hash: [0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![0x11; 20]),
            data_size: 16,
            udt_amount: Some(1234),
            data_hash: None,
        };
        // The token is registered under the cell's own type script hash.
        let type_hash = consumed_cell.type_script_hash().unwrap().to_vec();
        store.put_token_direct(&type_hash, &token).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, output_index, &consumed_cell, 1, 2);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        // What the parser ships for this input.
        let shipped = crate::sync::types::InputScriptHashes::from([(
            (tx_hash.clone(), output_index),
            (
                consumed_cell.lock_script_hash(),
                consumed_cell.type_script_hash(),
            ),
        )]);
        let result = writer
            .get_udt_cells_info_batch(&outpoints, &shipped)
            .unwrap();
        assert!(
            result.is_empty(),
            "spent outpoints from consumed_cells must not be reused as UDT inputs"
        );
    }

    #[test]
    fn test_get_udt_cells_info_batch_ignores_typed_cells_without_token_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xEE; 32];
        let output_index = 0i16;

        // Typed cell exists, but token metadata does not. This must NOT be treated as UDT input.
        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_code_hash: [0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_code_hash: Some([0x88; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x99; 32]),
            data_size: 16,
            udt_amount: None,
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        // What the parser ships for this input.
        let shipped = crate::sync::types::InputScriptHashes::from([(
            (tx_hash.clone(), output_index),
            (cell.lock_script_hash(), cell.type_script_hash()),
        )]);
        let result = writer
            .get_udt_cells_info_batch(&outpoints, &shipped)
            .unwrap();
        assert!(
            result.is_empty(),
            "typed cell without token metadata should not be classified as UDT"
        );
    }

    #[test]
    fn test_get_udt_cells_info_batch_skips_xudt_cells_without_amount() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xDD; 32];
        let output_index = 1i16;

        let token = TokenInfo {
            type_code_hash: vec![0x50; 32],
            hash_type: 2, // data1
            type_args: vec![0x11; 32],
            standard: "xudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: None,
            chain_metadata: None,
        };

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_code_hash: [0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_code_hash: Some([0x50; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x11; 32]),
            data_size: 0,
            udt_amount: None,
            data_hash: None,
        };
        // The token is registered under the cell's own type script hash.
        let type_hash = cell.type_script_hash().unwrap().to_vec();
        store.put_token_direct(&type_hash, &token).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        // What the parser ships for this input.
        let shipped = crate::sync::types::InputScriptHashes::from([(
            (tx_hash.clone(), output_index),
            (cell.lock_script_hash(), cell.type_script_hash()),
        )]);
        let result = writer
            .get_udt_cells_info_batch(&outpoints, &shipped)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_udt_cells_info_batch_tolerates_sudt_cells_without_amount() {
        // Owner-mode / non-standard sUDT cells can be typed cells that carry no
        // fungible amount (seen live on testnet). They must be skipped from UDT
        // transfer matching, not fail-fast the live writer — consistent with the
        // xUDT branch, the bulk reducer (owners/token.rs), and the parse layers.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xCC; 32];
        let output_index = 2i16;

        let token = TokenInfo {
            type_code_hash: vec![0x60; 32],
            hash_type: 1,
            type_args: vec![0x21; 32],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: None,
            chain_metadata: None,
        };

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_code_hash: [0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_code_hash: Some([0x60; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x21; 32]),
            data_size: 0,
            udt_amount: None,
            data_hash: None,
        };
        // The token is registered under the cell's own type script hash.
        let type_hash = cell.type_script_hash().unwrap().to_vec();
        store.put_token_direct(&type_hash, &token).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        // What the parser ships for this input.
        let shipped = crate::sync::types::InputScriptHashes::from([(
            (tx_hash.clone(), output_index),
            (cell.lock_script_hash(), cell.type_script_hash()),
        )]);
        let result = writer
            .get_udt_cells_info_batch(&outpoints, &shipped)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_process_udt_transfers_batch_derives_supply_from_holders() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAA; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: vec![0x22; 32],
            amount: 123u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };

        let tx_hash = [0xEF; 32];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(100i64, 1_700_000_000_000i64);
        let transfers = vec![(&transfer, tx_hash.as_slice(), 100i64)];
        let mut max_supply_observations = HashMap::new();
        max_supply_observations.insert(
            type_hash.clone(),
            MaxSupplyObservation {
                max_supply: 1_000,
                block_number: 100,
            },
        );

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &max_supply_observations,
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        let updated = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(updated.max_supply, Some(1_000));
        assert_eq!(
            store.aggregate_token_holder_stats(&type_hash).unwrap(),
            (1, TokenBalance::from(123))
        );
    }

    #[test]
    fn test_process_udt_transfers_batch_persists_transfer_and_holder_updates_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAD; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let to_lock_hash = vec![0x33; 32];
        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: to_lock_hash.clone(),
            amount: 50u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };

        let tx_hash = [0xEF; 32];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(101i64, 1_700_000_000_000i64);
        let transfers = vec![(&transfer, tx_hash.as_slice(), 101i64)];

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &HashMap::new(),
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        let updated = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(updated.transfers_count, 1);
        assert_eq!(
            store.aggregate_token_holder_stats(&type_hash).unwrap(),
            (1, TokenBalance::from(50))
        );
        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 1);
    }

    #[test]
    fn test_process_udt_transfers_batch_updates_ranked_holder_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAE; 32];
        let lock_a = vec![0x31; 32];
        let lock_b = vec![0x32; 32];
        store
            .put_token_direct(
                &type_hash,
                &TokenInfo {
                    type_code_hash: vec![0xCD; 32],
                    hash_type: 1,
                    type_args: vec![0x11; 20],
                    standard: "sudt".to_string(),
                    name: None,
                    symbol: None,
                    decimals: Some(8),
                    max_supply: None,
                    first_seen_block: 100,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                    metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
                    chain_metadata: None,
                },
            )
            .unwrap();

        let mut seed = StoreBatch::new(&store);
        seed.put_token_holder(&type_hash, &lock_a, 100);
        seed.put_token_holder_by_balance(&type_hash, &lock_a, 100);
        seed.put_addr_token_by_balance(&lock_a, &type_hash, 100);
        seed.commit().unwrap();

        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: Some(lock_a.clone()),
            to_lock_hash: lock_b.clone(),
            amount: 40u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: false,
            is_burn: false,
        };

        let tx_hash = [0xEE; 32];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(101i64, 1_700_000_000_000i64);
        let transfers = vec![(&transfer, tx_hash.as_slice(), 101i64)];

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &HashMap::new(),
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        assert_eq!(
            store
                .list_token_holders_by_balance(&type_hash, 10, None)
                .unwrap(),
            vec![
                (lock_a.clone(), TokenBalance::from(60)),
                (lock_b.clone(), TokenBalance::from(40)),
            ]
        );
        assert_eq!(
            store
                .list_address_tokens_by_balance(&lock_a, 10, None)
                .unwrap(),
            vec![(type_hash.clone(), TokenBalance::from(60))]
        );
        assert_eq!(
            store
                .list_address_tokens_by_balance(&lock_b, 10, None)
                .unwrap(),
            vec![(type_hash.clone(), TokenBalance::from(40))]
        );
    }

    #[test]
    fn test_process_udt_transfers_batch_buckets_hourly_transfers_by_each_block_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xA1; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 10,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let transfer_a = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: vec![0x44; 32],
            amount: 11u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };
        let transfer_b = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: vec![0x55; 32],
            amount: 22u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };

        let tx_hash_a = [0xE4; 32];
        let tx_hash_b = [0xE5; 32];
        let transfers = vec![
            (&transfer_a, tx_hash_a.as_slice(), 301i64),
            (&transfer_b, tx_hash_b.as_slice(), 302i64),
        ];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(301i64, 3_599_000i64);
        block_timestamps.insert(302i64, 3_601_000i64);

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &HashMap::new(),
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        let hour0_key = ckbadger_store::keys::encode_token_hourly_key(&type_hash, 0);
        let hour1_key = ckbadger_store::keys::encode_token_hourly_key(&type_hash, 1);
        let hour0 = store.get_stats_key(&hour0_key).unwrap().unwrap();
        let hour1 = store.get_stats_key(&hour1_key).unwrap().unwrap();
        assert_eq!(i64::from_le_bytes(hour0[..8].try_into().unwrap()), 1);
        assert_eq!(i64::from_le_bytes(hour1[..8].try_into().unwrap()), 1);
    }

    #[test]
    fn test_process_udt_transfers_batch_with_state_accumulates_hourly_in_same_uncommitted_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xB1; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 10,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let transfer_a = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: vec![0x77; 32],
            amount: 1u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };
        let transfer_b = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: vec![0x88; 32],
            amount: 1u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };

        let tx_hash_a = [0xF1; 32];
        let tx_hash_b = [0xF2; 32];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(401i64, 3_600_000i64);
        // Raw time regresses before the hour even though parent t* reached it.
        block_timestamps.insert(402i64, 3_599_000i64);
        let block_chain_times_ms = HashMap::from([(401i64, 3_600_000i64), (402, 3_600_000)]);

        let first = vec![(&transfer_a, tx_hash_a.as_slice(), 401i64)];
        let second = vec![(&transfer_b, tx_hash_b.as_slice(), 402i64)];
        let mut batch = StoreBatch::new(&store);
        let entity_stats = SharedEntityStatsOverlay::new();
        let mut state = writer.new_udt_batch_state(entity_stats.clone(), SharedUndoSeq::default());
        writer
            .process_udt_transfers_batch_with_state(
                &first,
                &HashMap::new(),
                &HashMap::new(),
                &block_timestamps,
                &block_chain_times_ms,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .process_udt_transfers_batch_with_state(
                &second,
                &HashMap::new(),
                &HashMap::new(),
                &block_timestamps,
                &block_chain_times_ms,
                &mut batch,
                &mut state,
            )
            .unwrap();
        entity_stats.stage_final(&mut batch).unwrap();
        batch.commit().unwrap();

        let hour1_key = ckbadger_store::keys::encode_token_hourly_key(&type_hash, 1);
        let hour1 = store.get_stats_key(&hour1_key).unwrap().unwrap();
        assert_eq!(i64::from_le_bytes(hour1[..8].try_into().unwrap()), 2);
        assert_eq!(
            store.list_token_transfers(&type_hash, 10, None).unwrap()[0]
                .2
                .timestamp,
            3_599_000
        );
    }

    #[test]
    fn test_process_udt_transfers_batch_applies_max_supply_without_transfers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAB; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 5,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let transfers: Vec<(&ParsedUdtTransfer, &[u8], i64)> = Vec::new();
        let block_timestamps = HashMap::new();
        let mut max_supply_observations = HashMap::new();
        max_supply_observations.insert(
            type_hash.clone(),
            MaxSupplyObservation {
                max_supply: 1_000_000,
                block_number: 100,
            },
        );

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &max_supply_observations,
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        let updated = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(updated.max_supply, Some(1_000_000));
        assert_eq!(updated.transfers_count, 5);
        assert_eq!(
            store.aggregate_token_holder_stats(&type_hash).unwrap(),
            (0, TokenBalance::zero())
        );
    }

    #[test]
    fn test_process_udt_transfers_batch_mint_amount_above_i128_max() {
        // Regression (live-sync twin of the bulk-reducer crash): a canonical sUDT mint
        // whose amount (LE u128) exceeds i128::MAX must land in derived supply and the
        // holder balance without wrapping. The old `amount as i128` accumulation wrapped
        // 2.22e38 negative and drove the supply/holder underflow guards.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let big: u128 = 222_044_604_925_031_325_468_940_491_728_862_838_784;
        let type_hash = vec![0xAF; 32];
        let to_lock_hash = vec![0x33; 32];
        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: to_lock_hash.clone(),
            amount: big,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };

        let tx_hash = [0xEF; 32];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(4_743_232i64, 1_700_000_000_000i64);
        let transfers = vec![(&transfer, tx_hash.as_slice(), 4_743_232i64)];

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &HashMap::new(),
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .expect("mint above i128::MAX must not wrap/underflow");
        batch.commit().unwrap();

        assert_eq!(
            store.aggregate_token_holder_stats(&type_hash).unwrap(),
            (1, TokenBalance::from(big))
        );
        assert_eq!(
            store
                .get_token_holder_balance(&type_hash, &to_lock_hash)
                .unwrap(),
            Some(TokenBalance::from(big))
        );
    }

    #[test]
    fn test_process_udt_transfers_batch_errors_on_holder_balance_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAC; 32];
        let lock_hash = vec![0x44; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 5,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let mut setup = StoreBatch::new(&store);
        setup.put_token_holder(&type_hash, &lock_hash, 10);
        setup.commit().unwrap();

        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: Some(lock_hash),
            to_lock_hash: Vec::new(),
            amount: 20u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: false,
            is_burn: true,
        };
        let tx_hash = [0xE1; 32];
        let transfers = vec![(&transfer, tx_hash.as_slice(), 200i64)];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(200i64, 1_700_000_100_000i64);
        let max_supply_observations = HashMap::new();

        let mut batch = StoreBatch::new(&store);
        let err = process_transfers_in_one_batch(
            &writer,
            &transfers,
            &max_supply_observations,
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap_err();
        assert!(err.to_string().contains("token holder balance underflow"));
    }

    #[test]
    fn test_process_udt_transfers_batch_supports_supply_above_u128() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAD; 32];
        let lock_a = vec![0x55; 32];
        let lock_b = vec![0x56; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 5,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let amount = 200u128 << 120;
        let transfer_a = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: lock_a,
            amount,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };
        let transfer_b = ParsedUdtTransfer {
            to_lock_hash: lock_b,
            ..transfer_a.clone()
        };
        let tx_hash_a = [0xE2; 32];
        let tx_hash_b = [0xE3; 32];
        let transfers = vec![
            (&transfer_a, tx_hash_a.as_slice(), 201i64),
            (&transfer_b, tx_hash_b.as_slice(), 201i64),
        ];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(201i64, 1_700_000_200_000i64);
        let max_supply_observations = HashMap::new();

        let mut batch = StoreBatch::new(&store);
        process_transfers_in_one_batch(
            &writer,
            &transfers,
            &max_supply_observations,
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        let (holders_count, total_supply) = store.aggregate_token_holder_stats(&type_hash).unwrap();
        assert_eq!(holders_count, 2);
        assert_eq!(
            total_supply.to_string(),
            "531691198313966349161522824112137830400"
        );
    }

    #[test]
    fn test_process_udt_transfers_batch_errors_on_missing_block_chain_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAE; 32];
        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: None,
            to_lock_hash: vec![0x66; 32],
            amount: 10u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: true,
            is_burn: false,
        };

        let tx_hash = [0xE3; 32];
        let transfers = vec![(&transfer, tx_hash.as_slice(), 202i64)];
        let block_timestamps = HashMap::new();
        let max_supply_observations = HashMap::new();

        let mut batch = StoreBatch::new(&store);
        let err = process_transfers_in_one_batch(
            &writer,
            &transfers,
            &max_supply_observations,
            &HashMap::new(),
            &block_timestamps,
            &mut batch,
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing block chain time"));
    }

    // -- apply_onchain_token_info binding semantics -----------------------------

    fn onchain_map(
        type_hash: &[u8],
        binding: crate::sync::token_helpers::OnchainInfoBinding,
    ) -> HashMap<Vec<u8>, crate::sync::token_helpers::OnchainTokenInfo> {
        let mut map = HashMap::new();
        map.insert(
            type_hash.to_vec(),
            crate::sync::token_helpers::OnchainTokenInfo {
                info: crate::sync::token_helpers::UniqueTokenInfo {
                    decimal: 8,
                    name: "OnChain".to_string(),
                    symbol: "OC".to_string(),
                    total_supply: None,
                },
                binding,
            },
        );
        map
    }

    fn token_with_metadata() -> TokenInfo {
        TokenInfo {
            type_code_hash: vec![0x55; 32],
            hash_type: 1,
            type_args: vec![0x66; 32],
            standard: "xudt".to_string(),
            name: Some("Labelled".to_string()),
            symbol: Some("LBL".to_string()),
            decimals: Some(4),
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        }
    }

    fn token_without_metadata() -> TokenInfo {
        TokenInfo {
            name: None,
            symbol: None,
            decimals: None,
            metadata_source: None,
            ..token_with_metadata()
        }
    }

    #[test]
    fn test_apply_onchain_token_info_cooccurrence_fills_gaps_only() {
        let type_hash = vec![0xAB; 32];
        let map = onchain_map(
            &type_hash,
            crate::sync::token_helpers::OnchainInfoBinding::IssuanceCooccurrence,
        );

        // Gaps are filled…
        let mut empty = token_without_metadata();
        apply_onchain_token_info(&type_hash, &mut empty, map.get(&type_hash));
        assert_eq!(empty.name.as_deref(), Some("OnChain"));
        assert_eq!(empty.symbol.as_deref(), Some("OC"));
        assert_eq!(empty.decimals, Some(8));

        // …but already-known metadata is never overwritten.
        let mut labelled = token_with_metadata();
        apply_onchain_token_info(&type_hash, &mut labelled, map.get(&type_hash));
        assert_eq!(labelled.name.as_deref(), Some("Labelled"));
        assert_eq!(labelled.symbol.as_deref(), Some("LBL"));
        assert_eq!(labelled.decimals, Some(4));
    }

    #[test]
    fn test_apply_onchain_token_info_extension_binding_still_overwrites() {
        // The cryptographic extension binding keeps its long-standing
        // always-write semantics (label import re-overwrites at startup).
        let type_hash = vec![0xAC; 32];
        let map = onchain_map(
            &type_hash,
            crate::sync::token_helpers::OnchainInfoBinding::XudtExtension,
        );

        let mut labelled = token_with_metadata();
        apply_onchain_token_info(&type_hash, &mut labelled, map.get(&type_hash));
        assert_eq!(labelled.name.as_deref(), Some("OnChain"));
        assert_eq!(labelled.symbol.as_deref(), Some("OC"));
        assert_eq!(labelled.decimals, Some(8));
    }

    #[test]
    fn test_cooccurrence_binding_reports_metadata_it_is_forbidden_to_write() {
        // The regression: a curated label that contradicts the token's own
        // Unique Cell keeps winning here forever (label import runs before the
        // first block is indexed, so the row is never empty by the time the
        // chain value shows up). Silently dropping the chain value is what made
        // the wrong RGB++ symbol survive until a human noticed it in the UI.
        let type_hash = vec![0xAD; 32];
        let map = onchain_map(
            &type_hash,
            crate::sync::token_helpers::OnchainInfoBinding::IssuanceCooccurrence,
        );

        let mut labelled = token_with_metadata();
        let (_, logs) = crate::label_import::test_log_capture::capture_warnings(|| {
            apply_onchain_token_info(&type_hash, &mut labelled, map.get(&type_hash));
        });

        assert!(
            logs.contains(&hex::encode(&type_hash)),
            "the contradicted token must be identified: {logs}"
        );
        assert!(
            logs.contains("LBL") && logs.contains("OC"),
            "both the stored and the on-chain value must be shown: {logs}"
        );
        // Precedence is unchanged: the heuristic binding still must not
        // overwrite already-known metadata.
        assert_eq!(labelled.name.as_deref(), Some("Labelled"));
        assert_eq!(labelled.symbol.as_deref(), Some("LBL"));
        assert_eq!(labelled.decimals, Some(4));
    }

    #[test]
    fn test_extension_binding_reports_the_label_it_overwrites() {
        let type_hash = vec![0xAE; 32];
        let map = onchain_map(
            &type_hash,
            crate::sync::token_helpers::OnchainInfoBinding::XudtExtension,
        );

        let mut labelled = token_with_metadata();
        let (_, logs) = crate::label_import::test_log_capture::capture_warnings(|| {
            apply_onchain_token_info(&type_hash, &mut labelled, map.get(&type_hash));
        });

        assert!(
            logs.contains(&hex::encode(&type_hash)),
            "the contradicted token must be identified: {logs}"
        );
    }

    #[test]
    fn test_agreeing_onchain_info_is_not_reported() {
        let type_hash = vec![0xAF; 32];
        let map = onchain_map(
            &type_hash,
            crate::sync::token_helpers::OnchainInfoBinding::IssuanceCooccurrence,
        );

        let mut agreeing = TokenInfo {
            name: Some("OnChain".to_string()),
            symbol: Some("OC".to_string()),
            decimals: Some(8),
            ..token_with_metadata()
        };
        let (_, logs) = crate::label_import::test_log_capture::capture_warnings(|| {
            apply_onchain_token_info(&type_hash, &mut agreeing, map.get(&type_hash));
        });
        assert!(logs.is_empty(), "agreement must be silent: {logs}");
    }

    #[test]
    fn test_empty_onchain_strings_assert_nothing_and_are_not_reported() {
        // A Unique Cell may carry an empty name/symbol. The write path skips
        // those, so the divergence check must skip them too — otherwise it
        // reports a conflict against a value that is never written.
        let type_hash = vec![0xB0; 32];
        let mut map = onchain_map(
            &type_hash,
            crate::sync::token_helpers::OnchainInfoBinding::XudtExtension,
        );
        let entry = map.get_mut(&type_hash).unwrap();
        entry.info.name = String::new();
        entry.info.symbol = String::new();
        entry.info.decimal = 4;

        let mut labelled = token_with_metadata();
        let (_, logs) = crate::label_import::test_log_capture::capture_warnings(|| {
            apply_onchain_token_info(&type_hash, &mut labelled, map.get(&type_hash));
        });
        assert!(logs.is_empty(), "empty chain fields assert nothing: {logs}");
        assert_eq!(labelled.name.as_deref(), Some("Labelled"));
        assert_eq!(labelled.symbol.as_deref(), Some("LBL"));
    }
}
