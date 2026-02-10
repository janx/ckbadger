use anyhow::Result;
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::AddressBalance;

use super::BatchWriter;

impl BatchWriter {
    pub fn update_address_balances_batch(
        &self,
        changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        for (lock_hash, (balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash)) in
            changes
        {
            // Read-modify-write for each address
            let existing = self.store.get_addr_balance(lock_hash)?;

            let updated = match existing {
                Some(mut bal) => {
                    bal.balance += *balance_delta as i128;
                    bal.live_cells_count = (bal.live_cells_count + *live_delta).max(0);
                    bal.total_cells_count += *total_delta as i64;
                    bal.txs_count += tx_delta;
                    bal.last_activity_block = *block_num;
                    bal.last_activity_tx = tx_hash.to_vec();
                    bal
                }
                None => AddressBalance {
                    balance: *balance_delta as i128,
                    live_cells_count: (*live_delta).max(0),
                    total_cells_count: (*total_delta).max(0) as i64,
                    txs_count: *tx_delta,
                    first_seen_block: *block_num,
                    first_seen_tx: tx_hash.to_vec(),
                    last_activity_block: *block_num,
                    last_activity_tx: tx_hash.to_vec(),
                },
            };

            batch.put_addr_balance(lock_hash, &updated);
        }

        Ok(())
    }

    pub fn update_script_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        for ((code_hash, is_type), (cells_delta, live_delta, cap_delta, live_cap_delta)) in changes
        {
            let script_kind = if *is_type { "type" } else { "lock" };

            // Read-modify-write for script usage stats
            let existing = self.store.get_script_info(code_hash)?;

            let updated = match existing {
                Some(mut info) => {
                    if script_kind == "lock" {
                        info.lock_cells_count += cells_delta;
                        info.lock_live_cells_count += live_delta;
                        info.lock_capacity_sum += cap_delta;
                        info.lock_live_capacity_sum += live_cap_delta;
                    } else {
                        info.type_cells_count += cells_delta;
                        info.type_live_cells_count += live_delta;
                        info.type_capacity_sum += cap_delta;
                        info.type_live_capacity_sum += live_cap_delta;
                    }
                    info
                }
                None => {
                    let mut info = ckbadger_store::types::ScriptInfo::default();
                    if script_kind == "lock" {
                        info.lock_cells_count = *cells_delta;
                        info.lock_live_cells_count = *live_delta;
                        info.lock_capacity_sum = *cap_delta;
                        info.lock_live_capacity_sum = *live_cap_delta;
                    } else {
                        info.type_cells_count = *cells_delta;
                        info.type_live_cells_count = *live_delta;
                        info.type_capacity_sum = *cap_delta;
                        info.type_live_capacity_sum = *live_cap_delta;
                    }
                    info
                }
            };

            batch.put_script_info(code_hash, &updated);
        }

        Ok(())
    }
}
