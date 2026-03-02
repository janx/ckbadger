use anyhow::{bail, Result};
use std::collections::HashMap;
use tracing::warn;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{TokenDailyDelta, TokenInfo, TokenTransferRecord};

use crate::parser::ParsedUdtTransfer;

use super::BatchWriter;

impl BatchWriter {
    pub fn update_token_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i128, i128)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut keyed_changes: Vec<(Vec<u8>, i128, i128)> = Vec::with_capacity(changes.len());
        for ((type_hash, date_yyyymmdd), (live_cap_delta, live_occupied_delta)) in changes {
            if *live_cap_delta == 0 && *live_occupied_delta == 0 {
                continue;
            }
            keyed_changes.push((
                ckbadger_store::keys::encode_token_daily_key(type_hash, *date_yyyymmdd).to_vec(),
                *live_cap_delta,
                *live_occupied_delta,
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

        for ((key, live_cap_delta, live_occupied_delta), existing_res) in
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
            existing.live_capacity_delta = existing
                .live_capacity_delta
                .checked_add(live_cap_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily capacity delta overflow: key=0x{}, current={}, delta={}",
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
                        "token daily occupied delta overflow: key=0x{}, current={}, delta={}",
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
            let cell_info = self.store.get_cell(tx_hash, output_index)?;

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
                    None => match standard {
                        crate::parser::UdtStandard::Xudt => {
                            // xUDT-compatible typed cells can be owner/metadata cells that do not
                            // carry a fungible amount. Skip them from UDT transfer matching.
                            continue;
                        }
                        crate::parser::UdtStandard::Sudt => {
                            return Err(anyhow::anyhow!(
                                "missing udt_amount in cell info for UDT input: outpoint=0x{}:{}, type_script_hash=0x{}",
                                hex::encode(tx_hash),
                                output_index,
                                hex::encode(type_script_hash)
                            ));
                        }
                    },
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
    pub fn process_udt_transfers_batch(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
        max_supply_observations: &HashMap<Vec<u8>, i128>,
        block_timestamps: &HashMap<i64, i64>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if transfers.is_empty() {
            // Even if this batch has no transfer deltas, a transaction can still expose
            // supply-info cells that reveal token hard caps.
            for type_hash in max_supply_observations.keys() {
                if let Some(mut info) = self.store.get_token(type_hash)? {
                    let before = info.max_supply;
                    Self::apply_observed_max_supply(type_hash, &mut info, max_supply_observations);
                    if info.max_supply != before {
                        batch.put_token(type_hash, &info);
                    }
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
                    supply_delta: 0i128,
                });
            entry.transfers_count += 1;

            if transfer.is_mint {
                entry.supply_delta += transfer.amount as i128;
            } else if transfer.is_burn {
                entry.supply_delta -= transfer.amount as i128;
            }

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

        // Step 2: Upsert tokens + transfer counts (in-memory first).
        // Track in-flight token state so Step 5 can update holders on newly-created tokens,
        // then persist each token once after holder adjustments.
        let mut inflight_tokens: HashMap<Vec<u8>, TokenInfo> = HashMap::new();

        for (type_hash, update) in &token_updates {
            let existing = self.store.get_token(type_hash)?;
            let transfer = update.transfer;

            // Read current total transfers count from stats CF
            let current_total = self.store.get_token_transfers_count(type_hash)?;
            let new_total = current_total + update.transfers_count;

            let mut updated = match existing {
                Some(mut info) => {
                    let supply = info.total_supply.get_or_insert(0);
                    let next_supply = *supply + update.supply_delta;
                    if next_supply < 0 {
                        bail!(
                            "token supply underflow: type_hash=0x{}, supply={}, delta={}",
                            hex::encode(type_hash),
                            *supply,
                            update.supply_delta
                        );
                    }
                    *supply = next_supply;
                    info
                }
                None => TokenInfo {
                    type_code_hash: transfer.type_code_hash.clone(),
                    hash_type: transfer.type_hash_type as u8,
                    type_args: transfer.type_args.clone(),
                    standard: transfer.standard.as_str().to_string(),
                    name: None,
                    symbol: None,
                    decimals: None,
                    total_supply: if update.supply_delta > 0 {
                        Some(update.supply_delta)
                    } else {
                        Some(0)
                    },
                    max_supply: None,
                    holders_count: 0,
                    first_seen_block: update.first_seen_block,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                },
            };
            Self::apply_observed_max_supply(type_hash, &mut updated, max_supply_observations);

            // Embed transfers_count into TokenInfo
            updated.transfers_count = new_total;
            inflight_tokens.insert(type_hash.clone(), updated);

            // Also write to stats CF (source of truth for accumulation)
            batch.put_token_transfers_count(type_hash, new_total);
        }

        // Step 2.5: Apply observed max_supply to tokens not touched by transfers in this batch.
        for type_hash in max_supply_observations.keys() {
            if inflight_tokens.contains_key(type_hash) {
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
            let current_hourly = match self.store.get_stats_key(&key)? {
                Some(v) => {
                    if v.len() != 8 {
                        bail!(
                            "invalid token hourly transfer value length: type_hash=0x{}, hour_bucket={}, len={}",
                            hex::encode(&type_hash),
                            hour_bucket,
                            v.len()
                        );
                    }
                    i64::from_le_bytes(v[..8].try_into().map_err(|_| {
                        anyhow::anyhow!(
                            "failed to decode token hourly transfer value as i64: type_hash=0x{}, hour_bucket={}",
                            hex::encode(&type_hash),
                            hour_bucket
                        )
                    })?)
                }
                None => 0,
            };
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
        }

        // Step 3: Aggregate balance changes per (type_hash, lock_hash)
        let mut balance_changes: HashMap<(Vec<u8>, Vec<u8>), i128> = HashMap::new();

        for (transfer, _tx_hash, _block_number) in transfers {
            let type_hash = &transfer.type_script_hash;

            if let Some(ref from_lock) = transfer.from_lock_hash {
                if !from_lock.is_empty() {
                    *balance_changes
                        .entry((type_hash.clone(), from_lock.clone()))
                        .or_default() -= transfer.amount as i128;
                }
            }

            if !transfer.to_lock_hash.is_empty() {
                *balance_changes
                    .entry((type_hash.clone(), transfer.to_lock_hash.clone()))
                    .or_default() += transfer.amount as i128;
            }
        }

        // Step 4: Apply balance changes with holder count tracking
        let mut holder_count_changes: HashMap<Vec<u8>, i64> = HashMap::new();

        for ((type_hash, lock_hash), delta) in &balance_changes {
            let existing = self.store.get_token_holder_balance(type_hash, lock_hash)?;
            let old_balance = existing.unwrap_or(0);
            let new_balance = old_balance + delta;
            if new_balance < 0 {
                bail!(
                    "token holder balance underflow: type_hash=0x{}, lock_hash=0x{}, balance={}, delta={}",
                    hex::encode(type_hash),
                    hex::encode(lock_hash),
                    old_balance,
                    delta
                );
            }

            if old_balance == 0 && new_balance > 0 {
                // New holder
                *holder_count_changes.entry(type_hash.clone()).or_default() += 1;
            } else if old_balance > 0 && new_balance == 0 {
                // Lost holder
                *holder_count_changes.entry(type_hash.clone()).or_default() -= 1;
            }

            if new_balance > 0 {
                batch.put_token_holder(type_hash, lock_hash, new_balance);
            } else {
                batch.delete_token_holder(type_hash, lock_hash);
            }
        }

        // Step 5: Update holder counts on tokens using in-flight state
        for (type_hash, holder_delta) in &holder_count_changes {
            if *holder_delta != 0 {
                if let Some(info) = inflight_tokens.get_mut(type_hash) {
                    let next_holders_count = info.holders_count + holder_delta;
                    if next_holders_count < 0 {
                        bail!(
                            "token holders count underflow: type_hash=0x{}, holders_count={}, delta={}",
                            hex::encode(type_hash),
                            info.holders_count,
                            holder_delta
                        );
                    }
                    info.holders_count = next_holders_count;
                } else if let Some(mut info) = self.store.get_token(type_hash)? {
                    // Fallback path for tokens not touched in Step 2 (defensive).
                    let next_holders_count = info.holders_count + holder_delta;
                    if next_holders_count < 0 {
                        bail!(
                            "token holders count underflow: type_hash=0x{}, holders_count={}, delta={}",
                            hex::encode(type_hash),
                            info.holders_count,
                            holder_delta
                        );
                    }
                    info.holders_count = next_holders_count;
                    batch.put_token(type_hash, &info);
                }
            }
        }

        // Persist each in-flight token once after all adjustments.
        for (type_hash, info) in &inflight_tokens {
            batch.put_token(type_hash, info);
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
        observations: &HashMap<Vec<u8>, i128>,
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
    supply_delta: i128,
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
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
        assert_eq!(delta.live_capacity_delta, 80);
        assert_eq!(delta.live_occupied_capacity_delta, 50);

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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            created_at_block: 1,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: None,
            type_args: Some(vec![0x11; 20]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: Some(1234),
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell);
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
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
            created_at_block: 1,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash),
            type_code_hash: Some(type_code_hash),
            type_args: Some(vec![0x11; 20]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: Some(1234),
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, output_index, &consumed_cell, 2);
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let tx_hash = vec![0xEE; 32];
        let output_index = 0i16;

        // Typed cell exists, but token metadata does not. This must NOT be treated as UDT input.
        let cell = LiveCellInfo {
            capacity: 100_000_000,
            created_at_block: 1,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(vec![0x77; 32]),
            type_code_hash: Some(vec![0x88; 32]),
            type_args: Some(vec![0x99; 32]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell);
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            created_at_block: 1,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash),
            type_code_hash: Some(vec![0x50; 32]),
            type_args: Some(vec![0x11; 32]),
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let result = writer.get_udt_cells_info_batch(&outpoints).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_udt_cells_info_batch_errors_on_sudt_cells_without_amount() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let cell = LiveCellInfo {
            capacity: 100_000_000,
            created_at_block: 1,
            lock_script_hash: vec![0x22; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![0x44; 20],
            type_script_hash: Some(type_hash),
            type_code_hash: Some(vec![0x60; 32]),
            type_args: Some(vec![0x21; 32]),
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &cell);
        batch.commit().unwrap();

        let outpoints = vec![(tx_hash.as_slice(), output_index)];
        let err = writer.get_udt_cells_info_batch(&outpoints).unwrap_err();
        assert!(err.to_string().contains("missing udt_amount"));
    }

    #[test]
    fn test_process_udt_transfers_batch_initializes_missing_total_supply() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let type_hash = vec![0xAA; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            total_supply: None,
            max_supply: None,
            holders_count: 0,
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
        max_supply_observations.insert(type_hash.clone(), 1_000i128);

        let mut batch = StoreBatch::new(&store);
        writer
            .process_udt_transfers_batch(
                &transfers,
                &max_supply_observations,
                &block_timestamps,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let updated = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(updated.total_supply, Some(123));
        assert_eq!(updated.max_supply, Some(1_000));
    }

    #[test]
    fn test_process_udt_transfers_batch_persists_transfer_and_holder_updates_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let type_hash = vec![0xAD; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
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
            .process_udt_transfers_batch(&transfers, &HashMap::new(), &block_timestamps, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let updated = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(updated.total_supply, Some(50));
        assert_eq!(updated.transfers_count, 1);
        assert_eq!(updated.holders_count, 1);
        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 1);
    }

    #[test]
    fn test_process_udt_transfers_batch_buckets_hourly_transfers_by_each_block_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let type_hash = vec![0xA1; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
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
            .process_udt_transfers_batch(&transfers, &HashMap::new(), &block_timestamps, &mut batch)
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
    fn test_process_udt_transfers_batch_applies_max_supply_without_transfers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let type_hash = vec![0xAB; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            total_supply: Some(42),
            max_supply: None,
            holders_count: 3,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 5,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let transfers: Vec<(&ParsedUdtTransfer, &[u8], i64)> = Vec::new();
        let block_timestamps = HashMap::new();
        let mut max_supply_observations = HashMap::new();
        max_supply_observations.insert(type_hash.clone(), 1_000_000i128);

        let mut batch = StoreBatch::new(&store);
        writer
            .process_udt_transfers_batch(
                &transfers,
                &max_supply_observations,
                &block_timestamps,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let updated = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(updated.max_supply, Some(1_000_000));
        assert_eq!(updated.total_supply, Some(42));
        assert_eq!(updated.transfers_count, 5);
    }

    #[test]
    fn test_process_udt_transfers_batch_errors_on_supply_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
            total_supply: Some(10),
            max_supply: None,
            holders_count: 1,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 5,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let mut setup = StoreBatch::new(&store);
        setup.put_token_holder(&type_hash, &lock_hash, 100);
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
                &block_timestamps,
                &mut batch,
            )
            .unwrap_err();
        assert!(err.to_string().contains("supply underflow"));
    }

    #[test]
    fn test_process_udt_transfers_batch_errors_on_holders_count_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let type_hash = vec![0xAD; 32];
        let lock_hash = vec![0x55; 32];
        let token = TokenInfo {
            type_code_hash: vec![0xCD; 32],
            hash_type: 1,
            type_args: vec![0x11; 20],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: Some(8),
            total_supply: Some(100),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 100,
            icon_url: None,
            description: None,
            transfers_count: 5,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let mut setup = StoreBatch::new(&store);
        setup.put_token_holder(&type_hash, &lock_hash, 5);
        setup.commit().unwrap();

        let transfer = ParsedUdtTransfer {
            type_script_hash: type_hash.clone(),
            type_code_hash: vec![0xCD; 32],
            type_hash_type: 1,
            type_args: vec![0x11; 20],
            from_lock_hash: Some(lock_hash),
            to_lock_hash: Vec::new(),
            amount: 5u128,
            standard: crate::parser::UdtStandard::Sudt,
            is_mint: false,
            is_burn: true,
        };
        let tx_hash = [0xE2; 32];
        let transfers = vec![(&transfer, tx_hash.as_slice(), 201i64)];
        let mut block_timestamps = HashMap::new();
        block_timestamps.insert(201i64, 1_700_000_200_000i64);
        let max_supply_observations = HashMap::new();

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .process_udt_transfers_batch(
                &transfers,
                &max_supply_observations,
                &block_timestamps,
                &mut batch,
            )
            .unwrap_err();
        assert!(err.to_string().contains("holders count underflow"));
    }

    #[test]
    fn test_process_udt_transfers_batch_errors_on_missing_block_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
                &block_timestamps,
                &mut batch,
            )
            .unwrap_err();
        assert!(err.to_string().contains("missing block timestamp"));
    }
}
