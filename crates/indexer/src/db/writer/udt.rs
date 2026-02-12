use anyhow::Result;
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::TokenInfo;

use crate::parser::{ParsedUdtCell, ParsedUdtTransfer};

use super::BatchWriter;

impl BatchWriter {
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
                let compact =
                    bincode::deserialize::<ckbadger_store::types::CompactConsumedCellInfo>(&val)?;
                Some(compact.to_live_cell_info())
            } else {
                None
            };

            if let Some(info) = cell_info {
                // Only include cells that have a type script (UDT cells always have one)
                if let (Some(ref type_script_hash), Some(ref type_code_hash)) =
                    (&info.type_script_hash, &info.type_code_hash)
                {
                    // Look up the token to get standard info
                    let token_info = self.store.get_token(type_script_hash)?;
                    let standard = token_info
                        .as_ref()
                        .map(|t| t.standard.clone())
                        .unwrap_or_default();

                    // Amount is stored in token_holders, but for consumed cells we may not have it.
                    // The amount for UDT cells comes from the cell data (first 16 bytes).
                    // Since we don't store raw cell data, we rely on the token_holders balance.
                    // For transfer detection, the parser already has the amount from the output data.
                    // Return 0 as amount — the caller uses this primarily for type identification.
                    result.insert(
                        (tx_hash.to_vec(), output_index),
                        (
                            type_script_hash.clone(),
                            type_code_hash.clone(),
                            0i16, // hash_type — not stored in LiveCellInfo, will be in TokenInfo
                            token_info
                                .as_ref()
                                .map(|t| t.type_args.clone())
                                .unwrap_or_default(),
                            info.lock_script_hash.clone(),
                            0u128, // amount — caller gets this from parsed output data
                            standard,
                        ),
                    );
                }
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
        for (type_hash, update) in &token_updates {
            let existing = self.store.get_token(type_hash)?;
            let transfer = update.transfer;

            // Read current total transfers count from stats CF
            let current_total = self.store.get_token_transfers_count(type_hash)?;
            let new_total = current_total + update.transfers_count;

            let mut updated = match existing {
                Some(mut info) => {
                    info.holders_count += 0; // Will be updated in balance step
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

        // Step 5: Update holder counts on tokens
        for (type_hash, holder_delta) in &holder_count_changes {
            if *holder_delta != 0 {
                // Re-read token (may have been updated in step 2) from batch perspective.
                // Since batch isn't committed yet, we read the version we wrote in step 2.
                // For correctness with the batch, we need to track the in-flight value.
                if let Some(mut info) = self.store.get_token(type_hash)? {
                    info.holders_count = (info.holders_count + holder_delta).max(0);
                    // Preserve transfers_count from Step 2 (store has stale value)
                    if let Some(update) = token_updates.get(type_hash.as_slice()) {
                        let current_total = self.store.get_token_transfers_count(type_hash)?;
                        info.transfers_count = current_total + update.transfers_count;
                    }
                    batch.put_token(type_hash, &info);
                }
            }
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
