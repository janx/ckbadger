use anyhow::{bail, Result};
use std::collections::HashMap;
use tracing::warn;

use ckbadger_common::TokenBalance;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{TokenDailyDelta, TokenInfo, TokenTransferRecord};
use ckbadger_store::CkbadgerStore;

use crate::parser::ParsedUdtTransfer;

use super::BatchWriter;

#[derive(Default)]
pub(crate) struct UdtBatchState {
    hourly_transfers: HashMap<Vec<u8>, i64>,
}

impl UdtBatchState {
    fn get_hourly_transfer(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        if let Some(cached) = self.hourly_transfers.get(key) {
            return Ok(*cached);
        }
        let loaded = match store.get_stats_key(key)? {
            Some(v) => {
                if v.len() != 8 {
                    bail!(
                        "invalid token hourly transfer value length: key=0x{}, len={}",
                        hex::encode(key),
                        v.len()
                    );
                }
                i64::from_le_bytes(v[..8].try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "failed to decode token hourly transfer value as i64: key=0x{}",
                        hex::encode(key)
                    )
                })?)
            }
            None => 0,
        };
        self.hourly_transfers.insert(key.to_vec(), loaded);
        Ok(loaded)
    }

    fn put_hourly_transfer(&mut self, key: Vec<u8>, count: i64) {
        self.hourly_transfers.insert(key, count);
    }
}

impl BatchWriter {
    pub(crate) fn new_udt_batch_state(&self) -> UdtBatchState {
        UdtBatchState::default()
    }

    pub fn update_token_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut keyed_changes: Vec<(Vec<u8>, i128, i128)> = Vec::with_capacity(changes.len());
        for ((type_hash, date_yyyymmdd), (owned_cap_delta, owned_knowledge_delta)) in changes {
            if *owned_cap_delta == 0 && *owned_knowledge_delta == 0 {
                continue;
            }
            keyed_changes.push((
                ckbadger_store::keys::encode_token_daily_key(type_hash, *date_yyyymmdd).to_vec(),
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
            let mut existing: TokenDailyDelta = match existing_res {
                Ok(Some(value)) => bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize token daily delta: key=0x{}, error={}",
                        hex::encode(&key),
                        e
                    )
                })?,
                Ok(None) => TokenDailyDelta::default(),
                Err(e) => {
                    bail!(
                        "failed to read token daily delta: key=0x{}, error={}",
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
                        "token daily capacity delta overflow: key=0x{}, current={}, delta={}",
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
                        "token daily used delta overflow: key=0x{}, current={}, delta={}",
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

    /// Look up UDT cell info for multiple outpoints.
    /// Returns (type_script_hash, type_code_hash, type_hash_type, type_args, lock_script_hash, amount, standard).
    pub fn get_udt_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
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
                // Only include cells that have a type script hash (UDT cells always have one).
                let Some(type_script_hash) = info.type_script_hash.as_ref() else {
                    continue;
                };

                // Token metadata is the source of truth for whether this typed cell should be
                // treated as UDT input.
                // Without this guard, arbitrary typed cells can be misclassified as UDT burns.
                let Some(token_info) = self.store.get_token(type_script_hash)? else {
                    continue;
                };
                let Some(standard) =
                    crate::parser::UdtStandard::from_standard_hint(&token_info.standard)
                else {
                    continue;
                };

                // LiveCellInfo from older schema versions may miss type_code_hash, so fall back
                // to token metadata before dropping the input from UDT matching.
                let type_code_hash = info
                    .type_code_hash
                    .clone()
                    .or_else(|| Some(token_info.type_code_hash.clone()));
                let Some(type_code_hash) = type_code_hash else {
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
                        type_script_hash.clone(),
                        type_code_hash,
                        hash_type,
                        type_args,
                        info.lock_script_hash.clone(),
                        amount,
                        standard.as_str().to_string(),
                    ),
                );
            }
        }

        Ok(result)
    }

    /// Process a batch of UDT transfers: upsert tokens and update holder balances.
    /// `block_timestamps` maps block_number → timestamp_ms for hourly bucket computation.
    #[allow(private_interfaces)]
    pub fn process_udt_transfers_batch(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
        max_supply_observations: &HashMap<Vec<u8>, u128>,
        onchain_token_info: &HashMap<Vec<u8>, crate::sync::token_helpers::OnchainTokenInfo>,
        block_timestamps: &HashMap<i64, i64>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut state = self.new_udt_batch_state();
        self.process_udt_transfers_batch_with_state(
            transfers,
            max_supply_observations,
            onchain_token_info,
            block_timestamps,
            batch,
            &mut state,
        )
    }

    pub(crate) fn process_udt_transfers_batch_with_state(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
        max_supply_observations: &HashMap<Vec<u8>, u128>,
        onchain_token_info: &HashMap<Vec<u8>, crate::sync::token_helpers::OnchainTokenInfo>,
        block_timestamps: &HashMap<i64, i64>,
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
                if let Some(mut info) = self.store.get_token(type_hash)? {
                    Self::apply_observed_max_supply(type_hash, &mut info, max_supply_observations);
                    apply_onchain_token_info(
                        type_hash,
                        &mut info,
                        onchain_token_info.get(type_hash),
                    );
                    batch.put_token(type_hash, &info);
                }
            }
            return Ok(());
        }

        // Step 1: Collect unique tokens and aggregate stats
        let mut token_updates: HashMap<Vec<u8>, TokenUpdate> = HashMap::new();
        let mut hourly_transfer_updates: HashMap<(Vec<u8>, i64), i64> = HashMap::new();

        for (transfer, tx_hash, block_number) in transfers {
            let entry = token_updates
                .entry(transfer.type_script_hash.clone())
                .or_insert_with(|| TokenUpdate {
                    transfer,
                    first_seen_block: *block_number,
                    transfers_count: 0,
                });
            entry.transfers_count = entry.transfers_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "token transfers_count overflow: type_hash=0x{}, tx_hash=0x{}, current={}",
                    hex::encode(&transfer.type_script_hash),
                    hex::encode(tx_hash),
                    entry.transfers_count
                )
            })?;

            let ts_ms = block_timestamps
                .get(block_number)
                .copied()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing block timestamp for UDT transfer aggregation: type_hash=0x{}, tx_hash=0x{}, block_number={}",
                        hex::encode(&transfer.type_script_hash),
                        hex::encode(tx_hash),
                        block_number
                    )
                })?;
            let hour_bucket = ts_ms / 3_600_000;
            *hourly_transfer_updates
                .entry((transfer.type_script_hash.clone(), hour_bucket))
                .or_insert(0) += 1;
        }

        // Step 2: Upsert token metadata and transfer counts. Live supply and holder count
        // are derived exclusively from CF_TOKEN_HOLDERS.
        for (type_hash, update) in &token_updates {
            let existing = self.store.get_token(type_hash)?;
            let transfer = update.transfer;

            // Read current total transfers count from stats CF
            let current_total = self.store.get_token_transfers_count(type_hash)?;
            let new_total = current_total
                .checked_add(update.transfers_count)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token total transfer count overflow: type_hash=0x{}, current={}, added={}",
                        hex::encode(type_hash),
                        current_total,
                        update.transfers_count
                    )
                })?;

            let mut updated = match existing {
                Some(info) => info,
                None => TokenInfo {
                    type_code_hash: transfer.type_code_hash.clone(),
                    hash_type: transfer.type_hash_type as u8,
                    type_args: transfer.type_args.clone(),
                    standard: transfer.standard.as_str().to_string(),
                    name: None,
                    symbol: None,
                    decimals: None,
                    max_supply: None,
                    first_seen_block: update.first_seen_block,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                },
            };
            Self::apply_observed_max_supply(type_hash, &mut updated, max_supply_observations);
            apply_onchain_token_info(type_hash, &mut updated, onchain_token_info.get(type_hash));

            updated.transfers_count = new_total;
            batch.put_token(type_hash, &updated);

            // Also write to stats CF (source of truth for accumulation)
            batch.put_token_transfers_count(type_hash, new_total);
        }

        // Step 2.5: Apply observed max_supply to tokens not touched by transfers in this batch.
        for type_hash in max_supply_observations.keys() {
            if token_updates.contains_key(type_hash) {
                continue;
            }

            if let Some(mut info) = self.store.get_token(type_hash)? {
                let before = info.max_supply;
                Self::apply_observed_max_supply(type_hash, &mut info, max_supply_observations);
                if info.max_supply != before {
                    batch.put_token(type_hash, &info);
                }
            }
        }

        // Step 2.6: Update per-hour transfer counts using each transfer's block timestamp.
        for ((type_hash, hour_bucket), count_delta) in hourly_transfer_updates {
            let key = ckbadger_store::keys::encode_token_hourly_key(&type_hash, hour_bucket);
            let current_hourly = state.get_hourly_transfer(self.store.as_ref(), &key)?;
            let updated_hourly = current_hourly.checked_add(count_delta).ok_or_else(|| {
                anyhow::anyhow!(
                    "token hourly transfer overflow: type_hash=0x{}, hour_bucket={}, current={}, delta={}",
                    hex::encode(&type_hash),
                    hour_bucket,
                    current_hourly,
                    count_delta
                )
            })?;
            batch.put_token_hourly_transfer(&type_hash, hour_bucket, updated_hourly);
            state.put_hourly_transfer(key, updated_hourly);
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

    fn apply_observed_max_supply(
        type_hash: &[u8],
        info: &mut TokenInfo,
        observations: &HashMap<Vec<u8>, u128>,
    ) {
        let Some(&observed) = observations.get(type_hash) else {
            return;
        };
        match info.max_supply {
            Some(existing) if existing != observed => {
                warn!(
                    token_type_hash = %hex::encode(type_hash),
                    existing_max_supply = existing,
                    observed_max_supply = observed,
                    "conflicting on-chain max supply observation; keeping existing value"
                );
            }
            _ => {
                info.max_supply = Some(observed);
            }
        }
    }
}

struct TokenUpdate<'a> {
    transfer: &'a ParsedUdtTransfer,
    first_seen_block: i64,
    transfers_count: i64,
}

/// The one definition of how a token's on-chain Unique Cell metadata merges
/// into the row about to be stored. Both sync modes call this: live sync from
/// the batch writer, bulk sync from `TokenOwner::emit_snapshot_rows`. Keeping
/// a second copy of the rule is how the two modes came to store different
/// metadata for the same chain data.
///
/// `info` must already carry whatever the store holds for this token, because
/// precedence is decided against it. Precedence follows
/// [`crate::sync::token_helpers::OnchainInfoBinding`] and nothing else:
///
/// * `XudtExtension` — the xUDT's own args name the Unique Cell, so the
///   association is cryptographic and the chain wins.
/// * `IssuanceCooccurrence` — the Unique Cell was merely co-created in the
///   token's mint transaction. That is a heuristic, so it fills gaps only and
///   never displaces already-known metadata (a curated label, or an earlier
///   extension-bound observation, which is the stronger evidence).
///
/// Whichever side wins, a *disagreement* is reported. This is the only moment
/// in the process where both values exist: label import runs once, before the
/// first block is indexed, so the conflict check it performs can only compare
/// a label against the label it wrote itself. Without the report here, a
/// bundled label that contradicts the chain stays silently authoritative and
/// is discoverable only by a human noticing the wrong name in the UI — which
/// is exactly how the wrong RGB++ symbol was eventually found.
pub(crate) fn apply_onchain_token_info(
    type_hash: &[u8],
    info: &mut TokenInfo,
    bound: Option<&crate::sync::token_helpers::OnchainTokenInfo>,
) {
    let Some(bound) = bound else {
        return;
    };

    // Only values that would actually be written can conflict: an empty
    // name/symbol in a Unique Cell asserts nothing and is never stored.
    if let Some(divergence) = crate::label_import::token_metadata_divergence(
        info,
        Some(bound.info.name.as_str()).filter(|name| !name.is_empty()),
        Some(bound.info.symbol.as_str()).filter(|symbol| !symbol.is_empty()),
        Some(i32::from(bound.info.decimal)),
    ) {
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

    match bound.binding {
        crate::sync::token_helpers::OnchainInfoBinding::XudtExtension => {
            // Cryptographic binding: always write on-chain data.
            if !bound.info.name.is_empty() {
                info.name = Some(bound.info.name.clone());
            }
            if !bound.info.symbol.is_empty() {
                info.symbol = Some(bound.info.symbol.clone());
            }
            info.decimals = Some(bound.info.decimal as i32);
        }
        crate::sync::token_helpers::OnchainInfoBinding::IssuanceCooccurrence => {
            // Heuristic same-tx binding: fill gaps only, never overwrite
            // already-known metadata (TOML labels or earlier on-chain info).
            if info.name.is_none() && !bound.info.name.is_empty() {
                info.name = Some(bound.info.name.clone());
            }
            if info.symbol.is_none() && !bound.info.symbol.is_empty() {
                info.symbol = Some(bound.info.symbol.clone());
            }
            if info.decimals.is_none() {
                info.decimals = Some(bound.info.decimal as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{LiveCellInfo, TokenInfo};
    use ckbadger_store::CkbadgerStore;

    #[test]
    fn test_update_token_daily_deltas_batch_accumulates_and_deletes_zero_net() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let type_hash = vec![0xAA; 32];

        let mut first = HashMap::new();
        first.insert((type_hash.clone(), 20240115u32), (100i128, 60i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch(&first, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut second = HashMap::new();
        second.insert((type_hash.clone(), 20240115u32), (-20i128, -10i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch(&second, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store
            .get_token_daily_delta(&type_hash, 20240115)
            .unwrap()
            .unwrap();
        assert_eq!(delta.owned_capacity_delta, 80);
        assert_eq!(delta.owned_knowledge_delta, 50);

        let mut third = HashMap::new();
        third.insert((type_hash.clone(), 20240115u32), (-80i128, -50i128));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch(&third, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store.get_token_daily_delta(&type_hash, 20240115).unwrap();
        assert!(delta.is_none());
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
            .update_token_daily_deltas_batch(&changes, &mut batch)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize token daily delta"));
    }

    #[test]
    fn test_get_udt_cells_info_batch_falls_back_to_token_type_code_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xAB; 32];
        let type_code_hash = vec![0xCD; 32];
        let tx_hash = vec![0xEF; 32];
        let output_index = 0i16;

        // Token metadata exists, but the live cell intentionally misses type_code_hash.
        let token = TokenInfo {
            type_code_hash: type_code_hash.clone(),
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
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: None,
            type_hash_type: None,
            type_args: Some(vec![0x11; 20]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: Some(1234),
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let result = writer.get_udt_cells_info_batch(&outpoints).unwrap();
        let entry = result
            .get(&(tx_hash.clone(), output_index))
            .expect("udt input should be resolved");

        assert_eq!(entry.0, type_hash);
        assert_eq!(entry.1, type_code_hash);
        assert_eq!(entry.2, 1i16);
        assert_eq!(entry.3, vec![0x11; 20]);
        assert_eq!(entry.4, vec![0x22; 32]);
        assert_eq!(entry.5, 1234u128);
        assert_eq!(entry.6, "sudt".to_string());
    }

    #[test]
    fn test_get_udt_cells_info_batch_does_not_use_consumed_cell_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_hash = vec![0xBA; 32];
        let type_code_hash = vec![0xCB; 32];
        let tx_hash = vec![0xDC; 32];
        let output_index = 0i16;

        let token = TokenInfo {
            type_code_hash: type_code_hash.clone(),
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
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        // The outpoint exists only in consumed_cells (already spent), not live_cells.
        // UDT input lookup must ignore it to avoid replaying historical spend deltas.
        let consumed_cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash),
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![0x11; 20]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: Some(1234),
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, output_index, &consumed_cell, 1, 2);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let result = writer.get_udt_cells_info_batch(&outpoints).unwrap();
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
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(vec![0x77; 32]),
            type_code_hash: Some(vec![0x88; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x99; 32]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let result = writer.get_udt_cells_info_batch(&outpoints).unwrap();
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

        let type_hash = vec![0xAC; 32];
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
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash),
            type_code_hash: Some(vec![0x50; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x11; 32]),
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let result = writer.get_udt_cells_info_batch(&outpoints).unwrap();
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

        let type_hash = vec![0xAD; 32];
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
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash),
            type_code_hash: Some(vec![0x60; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x21; 32]),
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell, 1);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let result = writer.get_udt_cells_info_batch(&outpoints).unwrap();
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
        max_supply_observations.insert(type_hash.clone(), 1_000u128);

        let mut batch = StoreBatch::new(&store);
        writer
            .process_udt_transfers_batch(
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
        writer
            .process_udt_transfers_batch(
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
        writer
            .process_udt_transfers_batch(
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
        writer
            .process_udt_transfers_batch(
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
        block_timestamps.insert(401i64, 1_800_000i64);
        block_timestamps.insert(402i64, 2_000_000i64);

        let first = vec![(&transfer_a, tx_hash_a.as_slice(), 401i64)];
        let second = vec![(&transfer_b, tx_hash_b.as_slice(), 402i64)];
        let mut batch = StoreBatch::new(&store);
        let mut state = writer.new_udt_batch_state();
        writer
            .process_udt_transfers_batch_with_state(
                &first,
                &HashMap::new(),
                &HashMap::new(),
                &block_timestamps,
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
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let hour0_key = ckbadger_store::keys::encode_token_hourly_key(&type_hash, 0);
        let hour0 = store.get_stats_key(&hour0_key).unwrap().unwrap();
        assert_eq!(i64::from_le_bytes(hour0[..8].try_into().unwrap()), 2);
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
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let transfers: Vec<(&ParsedUdtTransfer, &[u8], i64)> = Vec::new();
        let block_timestamps = HashMap::new();
        let mut max_supply_observations = HashMap::new();
        max_supply_observations.insert(type_hash.clone(), 1_000_000u128);

        let mut batch = StoreBatch::new(&store);
        writer
            .process_udt_transfers_batch(
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
        writer
            .process_udt_transfers_batch(
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
        let err = writer
            .process_udt_transfers_batch(
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
        writer
            .process_udt_transfers_batch(
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
    fn test_process_udt_transfers_batch_errors_on_missing_block_timestamp() {
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
        let err = writer
            .process_udt_transfers_batch(
                &transfers,
                &max_supply_observations,
                &HashMap::new(),
                &block_timestamps,
                &mut batch,
            )
            .unwrap_err();
        assert!(err.to_string().contains("missing block timestamp"));
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
        }
    }

    fn token_without_metadata() -> TokenInfo {
        TokenInfo {
            name: None,
            symbol: None,
            decimals: None,
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
