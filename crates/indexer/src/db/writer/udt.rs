use anyhow::Result;
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{TokenDailyDelta, TokenInfo, TokenTransferRecord};

use crate::parser::{ParsedUdtCell, ParsedUdtTransfer};

use super::BatchWriter;

impl BatchWriter {
    pub fn update_token_daily_deltas_batch(
        &self,
        changes: &HashMap<(Vec<u8>, u32), (i64, i64)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let mut keyed_changes: Vec<(Vec<u8>, i64, i64)> = Vec::with_capacity(changes.len());
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
            .map(|(key, _, _)| (self.store.cf_stats(), key.as_slice()))
            .collect();
        let existing_results = self.store.multi_get_cf(cf_keys);

        for ((key, live_cap_delta, live_occupied_delta), existing_res) in
            keyed_changes.into_iter().zip(existing_results.into_iter())
        {
            let mut existing: TokenDailyDelta = match existing_res {
                Ok(Some(value)) => bincode::deserialize(&value).unwrap_or_default(),
                _ => TokenDailyDelta::default(),
            };
            existing.live_capacity_delta += live_cap_delta;
            existing.live_occupied_capacity_delta += live_occupied_delta;
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
            let outpoint_key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);

            // Try live cells first, then consumed cells
            let cell_info = if let Some(val) = self
                .store
                .get_cf(self.store.cf_live_cells(), &outpoint_key)?
            {
                Some(bincode::deserialize::<ckbadger_store::types::LiveCellInfo>(
                    &val,
                )?)
            } else if let Some(val) = self
                .store
                .get_cf(self.store.cf_consumed_cells(), &outpoint_key)?
            {
                ckbadger_store::types::decode_consumed_cell_info(&val)
                    .map(|c| c.to_live_cell_info())
            } else {
                None
            };

            if let Some(info) = cell_info {
                // Only include cells that have a type script hash (UDT cells always have one).
                let Some(type_script_hash) = info.type_script_hash.as_ref() else {
                    continue;
                };

                // Token metadata is the source of truth for standard/hash_type/type_args.
                // LiveCellInfo from older schema versions may miss type_code_hash, so fall back
                // to token metadata before dropping the input from UDT matching.
                let token_info = self.store.get_token(type_script_hash)?;
                let type_code_hash = info
                    .type_code_hash
                    .clone()
                    .or_else(|| token_info.as_ref().map(|t| t.type_code_hash.clone()));
                let Some(type_code_hash) = type_code_hash else {
                    continue;
                };

                let hash_type = token_info
                    .as_ref()
                    .map(|t| t.hash_type as i16)
                    .unwrap_or(0i16);
                let type_args = token_info
                    .as_ref()
                    .map(|t| t.type_args.clone())
                    .unwrap_or_default();
                let standard = token_info
                    .as_ref()
                    .map(|t| t.standard.clone())
                    .unwrap_or_default();

                // Amount is stored in token_holders, but for consumed cells we may not have it.
                // The amount for UDT cells comes from the cell data (first 16 bytes).
                // Since we don't store raw cell data, we rely on the token_holders balance.
                // For transfer detection, the parser already has the amount from the output data.
                // Return 0 as amount — caller uses this for type identification/matching.
                result.insert(
                    (tx_hash.to_vec(), output_index),
                    (
                        type_script_hash.clone(),
                        type_code_hash,
                        hash_type,
                        type_args,
                        info.lock_script_hash.clone(),
                        0u128, // amount — caller gets this from parsed output data
                        standard,
                    ),
                );
            }
        }

        Ok(result)
    }

    /// Insert UDT cells into the store. UDT cell data is part of the live_cells CF
    /// (already written by cells.rs), so this is a no-op for the cell data itself.
    /// The token-specific metadata is handled by process_udt_transfers_batch.
    pub fn insert_udt_cells_batch(
        &self,
        _cells: &[(&[u8], i16, &ParsedUdtCell, i64)],
    ) -> Result<()> {
        // UDT cell data (outpoint → cell info) is already stored in live_cells CF
        // by the cells writer. No separate UDT cells table needed.
        Ok(())
    }

    /// Mark UDT cells as consumed. Already handled by cells.rs consume_cells_batch.
    pub fn consume_udt_cells_batch(&self, _outpoints: &[(&[u8], i16, i64, &[u8])]) -> Result<()> {
        // Cell consumption is handled by cells.rs which moves from live_cells to consumed_cells.
        Ok(())
    }

    /// Process a batch of UDT transfers: upsert tokens and update holder balances.
    /// `block_timestamps` maps block_number → timestamp_ms for hourly bucket computation.
    pub fn process_udt_transfers_batch(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
        block_timestamps: &HashMap<i64, i64>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        // Step 1: Collect unique tokens and aggregate stats
        let mut token_updates: HashMap<Vec<u8>, TokenUpdate> = HashMap::new();

        for (transfer, _tx_hash, block_number) in transfers {
            let entry = token_updates
                .entry(transfer.type_script_hash.clone())
                .or_insert_with(|| TokenUpdate {
                    transfer,
                    block_number: *block_number,
                    transfers_count: 0,
                    supply_delta: 0i128,
                });
            entry.transfers_count += 1;

            if transfer.is_mint {
                entry.supply_delta += transfer.amount as i128;
            } else if transfer.is_burn {
                entry.supply_delta -= transfer.amount as i128;
            }
        }

        // Step 2: Upsert tokens + transfer counts (merged to avoid double iteration)
        // Track in-flight token state so Step 5 can update holders on newly-created tokens.
        let mut inflight_tokens: HashMap<Vec<u8>, TokenInfo> = HashMap::new();

        for (type_hash, update) in &token_updates {
            let existing = self.store.get_token(type_hash)?;
            let transfer = update.transfer;

            // Read current total transfers count from stats CF
            let current_total = self.store.get_token_transfers_count(type_hash)?;
            let new_total = current_total + update.transfers_count;

            let mut updated = match existing {
                Some(mut info) => {
                    if let Some(ref mut supply) = info.total_supply {
                        *supply = (*supply + update.supply_delta).max(0);
                    }
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
                    holders_count: 0,
                    first_seen_block: update.block_number,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                },
            };

            // Embed transfers_count into TokenInfo
            updated.transfers_count = new_total;
            batch.put_token(type_hash, &updated);
            inflight_tokens.insert(type_hash.clone(), updated);

            // Also write to stats CF (source of truth for accumulation)
            batch.put_token_transfers_count(type_hash, new_total);

            // Hourly bucket: determine hour from block timestamp
            if let Some(&ts_ms) = block_timestamps.get(&update.block_number) {
                let hour_bucket = ts_ms / 3_600_000;
                let current_hourly = {
                    let key = ckbadger_store::keys::encode_token_hourly_key(type_hash, hour_bucket);
                    match self.store.get_cf(self.store.cf_stats(), &key)? {
                        Some(v) if v.len() == 8 => i64::from_le_bytes(v[..8].try_into().unwrap()),
                        _ => 0,
                    }
                };
                batch.put_token_hourly_transfer(
                    type_hash,
                    hour_bucket,
                    current_hourly + update.transfers_count,
                );
            }
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
            let new_balance = (old_balance + delta).max(0);

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
                // Use in-flight token from Step 2 (handles both new and existing tokens).
                // Fallback to store for tokens not in this batch (shouldn't happen, but safe).
                let info = inflight_tokens
                    .get(type_hash)
                    .cloned()
                    .or_else(|| self.store.get_token(type_hash).ok().flatten());

                if let Some(mut info) = info {
                    info.holders_count = (info.holders_count + holder_delta).max(0);
                    batch.put_token(type_hash, &info);
                }
            }
        }

        // Step 6: Write individual transfer records for the token transfers tab.
        // Use a per-(type_hash, block_number) counter as tx_idx to generate unique keys.
        let mut transfer_idx: HashMap<(Vec<u8>, i64), i32> = HashMap::new();
        for (transfer, tx_hash, block_number) in transfers {
            let idx = transfer_idx
                .entry((transfer.type_script_hash.clone(), *block_number))
                .or_insert(0);
            let timestamp = block_timestamps.get(block_number).copied().unwrap_or(0);
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

struct TokenUpdate<'a> {
    transfer: &'a ParsedUdtTransfer,
    block_number: i64,
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
        first.insert((type_hash.clone(), 20240115u32), (100i64, 60i64));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch(&first, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut second = HashMap::new();
        second.insert((type_hash.clone(), 20240115u32), (-20i64, -10i64));
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
        third.insert((type_hash.clone(), 20240115u32), (-80i64, -50i64));
        let mut batch = StoreBatch::new(&store);
        writer
            .update_token_daily_deltas_batch(&third, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let delta = store.get_token_daily_delta(&type_hash, 20240115).unwrap();
        assert!(delta.is_none());
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
            data_size: 16,
            occupied_capacity: 0,
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
        assert_eq!(entry.5, 0u128);
        assert_eq!(entry.6, "sudt".to_string());
    }
}
