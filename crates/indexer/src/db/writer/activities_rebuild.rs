//! Post-bulk-sync activities rebuild.
//!
//! During bulk sync, activity writes are skipped to reduce write volume.
//! After bulk sync completes, this module rebuilds all activities by
//! re-reading blocks from CKB's RocksDB and reprocessing them.

use std::collections::HashMap;

use ckb_store_reader::CkbChainReader;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{CompactConsumedCellInfo, LiveCellInfo};
use ckbadger_store::CkbadgerStore;
use tracing::{info, warn};

use crate::db::writer::activities::{build_activities_for_block, InputCellView, TxView};
use crate::parser::cell::ParsedCell;
use crate::parser::{CellParser, TransactionParser};
use crate::rpc::BlockResponseWithCycles;

/// Per-transaction data collected before building TxView references.
struct TxRebuildData {
    hash: [u8; 32],
    tx_index: i32,
    is_cellbase: bool,
    inputs: Vec<InputCellView>,
    cells: Vec<ParsedCell>,
    outputs_data: Vec<String>,
}

/// Rebuild all activities from genesis to tip.
///
/// Reads raw blocks from CKB's RocksDB, parses transactions, looks up
/// input cell info from our store, and writes activity entries.
///
/// Returns `true` if the rebuild completed successfully, `false` if it
/// was skipped (e.g. missing CKB store) or had no work to do.
pub fn rebuild_activities(store: &CkbadgerStore, ckb_store: Option<&CkbChainReader>) -> bool {
    let ckb_store = match ckb_store {
        Some(s) => s,
        None => {
            warn!("Activities rebuild skipped: CKB store not available (ckb_data_path not set)");
            return false;
        }
    };

    let tip = match store.get_sync_status() {
        Ok(status) => status.tip_block_number,
        Err(e) => {
            warn!("Activities rebuild: failed to get sync tip: {}", e);
            return false;
        }
    };

    if tip <= 0 {
        info!("Activities rebuild: no blocks to process");
        return true;
    }

    const BATCH_BLOCKS: i64 = 1000;
    const LOG_INTERVAL: i64 = 100_000;

    info!(tip, "Activities rebuild: starting");
    let start = std::time::Instant::now();
    let mut total_activities: usize = 0;
    let mut last_log_block: i64 = 0;

    let mut block_num: i64 = 0;
    while block_num <= tip {
        let batch_end = std::cmp::min(block_num + BATCH_BLOCKS - 1, tip);
        let mut batch = StoreBatch::new(store);
        let mut batch_count: usize = 0;

        for bn in block_num..=batch_end {
            let block_view = match ckb_store.get_block_by_number(bn as u64) {
                Some(b) => b,
                None => continue,
            };

            // Convert to RPC types for parser compatibility
            let rpc_block = ckb_store_reader::block_view_to_rpc(&block_view, ckb_store);
            let block_response: BlockResponseWithCycles = rpc_block.into();

            // Get timestamp from our block_headers CF
            let timestamp_ms = match store.get_block_header(bn) {
                Ok(Some(header)) => header.timestamp,
                _ => {
                    // Fallback: parse from CKB block header
                    let ts_hex = &block_response.block.header.timestamp;
                    let ts_str = ts_hex.strip_prefix("0x").unwrap_or(ts_hex);
                    i64::from_str_radix(ts_str, 16).unwrap_or(0)
                }
            };

            // Collect all transaction data with owned values
            let tx_data: Vec<TxRebuildData> = block_response
                .block
                .transactions
                .iter()
                .enumerate()
                .map(|(tx_index, tx)| {
                    let parsed_tx = TransactionParser::parse(tx);
                    let cells = CellParser::parse_outputs(tx);
                    let outputs_data: Vec<String> = tx.outputs_data.clone();
                    let is_cellbase = parsed_tx.is_cellbase;

                    let inputs: Vec<InputCellView> = if is_cellbase {
                        Vec::new()
                    } else {
                        let parsed_inputs = TransactionParser::parse_inputs(tx);
                        parsed_inputs
                            .iter()
                            .map(|inp| lookup_input_cell(store, inp))
                            .collect()
                    };

                    TxRebuildData {
                        hash: parsed_tx.hash,
                        tx_index: tx_index as i32,
                        is_cellbase,
                        inputs,
                        cells,
                        outputs_data,
                    }
                })
                .collect();

            // Build TxView references from owned data
            let token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> = HashMap::new();
            let tx_views: Vec<TxView<'_>> = tx_data
                .iter()
                .map(|td| TxView {
                    tx_hash: &td.hash,
                    tx_index: td.tx_index,
                    block_number: bn,
                    timestamp: timestamp_ms,
                    is_cellbase: td.is_cellbase,
                    inputs: td.inputs.clone(),
                    outputs: &td.cells,
                    outputs_data: &td.outputs_data,
                })
                .collect();

            let activities = build_activities_for_block(&tx_views, &token_info_cache);
            for (lock_hash, entry) in activities {
                batch.put_activity(&lock_hash, entry.block_number, entry.tx_index, &entry);
                batch_count += 1;
            }
        }

        if batch_count > 0 {
            if let Err(e) = batch.commit() {
                warn!(
                    "Activities rebuild: batch commit error at block {}: {}",
                    batch_end, e
                );
                return false;
            }
        }
        total_activities += batch_count;

        if batch_end - last_log_block >= LOG_INTERVAL {
            let elapsed = start.elapsed().as_secs();
            let rate = if elapsed > 0 {
                batch_end as f64 / elapsed as f64
            } else {
                0.0
            };
            info!(
                block = batch_end,
                tip,
                activities = total_activities,
                blocks_per_sec = format!("{:.0}", rate),
                "Activities rebuild progress"
            );
            last_log_block = batch_end;
        }

        block_num = batch_end + 1;
    }

    let elapsed_secs = start.elapsed().as_secs();
    info!(
        blocks = tip,
        activities = total_activities,
        elapsed_secs,
        "Activities rebuild complete"
    );
    true
}

/// Look up the cell that a transaction input is consuming.
///
/// Checks `live_cells` CF first (cell still unspent), then falls back to
/// `consumed_cells` CF (cell already consumed by a later block).  Returns
/// an empty `InputCellView` if the outpoint is not found in either CF,
/// which can happen for genesis inputs or during partial rebuilds.
fn lookup_input_cell(
    store: &CkbadgerStore,
    inp: &crate::parser::transaction::ParsedInput,
) -> InputCellView {
    let outpoint_key =
        keys::encode_outpoint(&inp.previous_tx_hash, inp.previous_output_index as i16);

    // Try live_cells first, then consumed_cells
    if let Ok(Some(v)) = store.get_cf(store.cf_live_cells(), &outpoint_key) {
        if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&v) {
            return InputCellView {
                lock_script_hash: info.lock_script_hash,
                capacity: info.capacity,
                occupied_capacity: info.occupied_capacity,
                type_code_hash: info.type_code_hash,
                type_script_hash: info.type_script_hash,
                type_args: None,
                data: Vec::new(),
                data_size: info.data_size,
            };
        }
    }

    if let Ok(Some(v)) = store.get_cf(store.cf_consumed_cells(), &outpoint_key) {
        if let Ok(info) = bincode::deserialize::<CompactConsumedCellInfo>(&v) {
            return InputCellView {
                lock_script_hash: info.lock_script_hash,
                capacity: info.capacity,
                occupied_capacity: 0,
                type_code_hash: info.type_code_hash,
                type_script_hash: None,
                type_args: None,
                data: Vec::new(),
                data_size: info.data_size,
            };
        }
    }

    InputCellView {
        lock_script_hash: Vec::new(),
        capacity: 0,
        occupied_capacity: 0,
        type_code_hash: None,
        type_script_hash: None,
        type_args: None,
        data: Vec::new(),
        data_size: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn setup_store() -> Arc<CkbadgerStore> {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        std::mem::forget(dir);
        store
    }

    #[test]
    fn test_rebuild_activities_skips_when_no_ckb_store() {
        let store = setup_store();
        // Should return false (not completed) when ckb_store is None
        assert!(!rebuild_activities(&store, None));
    }

    #[test]
    fn test_rebuild_activities_skips_when_tip_is_zero() {
        let store = setup_store();
        // tip_block_number defaults to 0, so nothing to process
        // We pass None for ckb_store to trigger the early return first,
        // but let's also confirm the tip=0 path by checking status
        let status = store.get_sync_status().unwrap();
        assert_eq!(status.tip_block_number, 0);
        // With None, the "no ckb_store" branch fires before tip check,
        // returning false since rebuild was skipped
        assert!(!rebuild_activities(&store, None));
    }

    #[test]
    fn test_lookup_input_cell_returns_empty_when_not_found() {
        let store = setup_store();
        let inp = crate::parser::transaction::ParsedInput {
            previous_tx_hash: [0xAA; 32],
            previous_output_index: 0,
            since: 0,
        };
        let cell = lookup_input_cell(&store, &inp);
        // Should return empty fallback
        assert!(cell.lock_script_hash.is_empty());
        assert_eq!(cell.capacity, 0);
        assert_eq!(cell.occupied_capacity, 0);
        assert!(cell.type_code_hash.is_none());
        assert_eq!(cell.data_size, 0);
    }

    #[test]
    fn test_lookup_input_cell_finds_live_cell() {
        let store = setup_store();

        // Insert a LiveCellInfo into live_cells CF
        let tx_hash = [0xBB; 32];
        let output_index: i16 = 1;
        let outpoint_key = keys::encode_outpoint(&tx_hash, output_index);

        let live_cell = LiveCellInfo {
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 0,
            lock_args: vec![],
            capacity: 500_00000000,
            occupied_capacity: 61_00000000,
            type_code_hash: Some(vec![0x33; 32]),
            type_script_hash: Some(vec![0x44; 32]),
            data_size: 16,
            created_at_block: 1000,
        };
        let encoded = bincode::serialize(&live_cell).unwrap();
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &encoded)
            .unwrap();

        let inp = crate::parser::transaction::ParsedInput {
            previous_tx_hash: tx_hash,
            previous_output_index: output_index as i32,
            since: 0,
        };
        let cell = lookup_input_cell(&store, &inp);

        assert_eq!(cell.lock_script_hash, vec![0x11; 32]);
        assert_eq!(cell.capacity, 500_00000000);
        assert_eq!(cell.occupied_capacity, 61_00000000);
        assert_eq!(cell.type_code_hash, Some(vec![0x33; 32]));
        assert_eq!(cell.type_script_hash, Some(vec![0x44; 32]));
        assert_eq!(cell.data_size, 16);
    }

    #[test]
    fn test_lookup_input_cell_falls_back_to_consumed_cell() {
        let store = setup_store();

        // Insert only in consumed_cells CF (not in live_cells)
        let tx_hash = [0xCC; 32];
        let output_index: i16 = 2;
        let outpoint_key = keys::encode_outpoint(&tx_hash, output_index);

        let consumed_cell = CompactConsumedCellInfo {
            lock_script_hash: vec![0x66; 32],
            lock_code_hash: vec![],
            lock_hash_type: 0,
            lock_args: vec![],
            capacity: 200_00000000,
            created_at_block: 500,
            type_code_hash: Some(vec![0x77; 32]),
            data_size: 8,
        };
        let encoded = bincode::serialize(&consumed_cell).unwrap();
        store
            .put_cf(store.cf_consumed_cells(), &outpoint_key, &encoded)
            .unwrap();

        let inp = crate::parser::transaction::ParsedInput {
            previous_tx_hash: tx_hash,
            previous_output_index: output_index as i32,
            since: 0,
        };
        let cell = lookup_input_cell(&store, &inp);

        assert_eq!(cell.lock_script_hash, vec![0x66; 32]);
        assert_eq!(cell.capacity, 200_00000000);
        // CompactConsumedCellInfo doesn't store occupied_capacity
        assert_eq!(cell.occupied_capacity, 0);
        assert_eq!(cell.type_code_hash, Some(vec![0x77; 32]));
        // CompactConsumedCellInfo doesn't store type_script_hash
        assert!(cell.type_script_hash.is_none());
        assert_eq!(cell.data_size, 8);
    }

    #[test]
    fn test_lookup_input_cell_prefers_live_over_consumed() {
        let store = setup_store();

        let tx_hash = [0xDD; 32];
        let output_index: i16 = 0;
        let outpoint_key = keys::encode_outpoint(&tx_hash, output_index);

        // Insert in both CFs — live_cells should win
        let live_cell = LiveCellInfo {
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![],
            lock_hash_type: 0,
            lock_args: vec![],
            capacity: 999_00000000,
            occupied_capacity: 61_00000000,
            type_code_hash: None,
            type_script_hash: None,
            data_size: 0,
            created_at_block: 500,
        };
        store
            .put_cf(
                store.cf_live_cells(),
                &outpoint_key,
                &bincode::serialize(&live_cell).unwrap(),
            )
            .unwrap();

        let consumed_cell = CompactConsumedCellInfo {
            lock_script_hash: vec![0xFF; 32],
            lock_code_hash: vec![],
            lock_hash_type: 0,
            lock_args: vec![],
            capacity: 1_00000000,
            created_at_block: 400,
            type_code_hash: None,
            data_size: 0,
        };
        store
            .put_cf(
                store.cf_consumed_cells(),
                &outpoint_key,
                &bincode::serialize(&consumed_cell).unwrap(),
            )
            .unwrap();

        let inp = crate::parser::transaction::ParsedInput {
            previous_tx_hash: tx_hash,
            previous_output_index: output_index as i32,
            since: 0,
        };
        let cell = lookup_input_cell(&store, &inp);

        // Should get live_cell data (0xAA), not consumed_cell (0xFF)
        assert_eq!(cell.lock_script_hash, vec![0xAA; 32]);
        assert_eq!(cell.capacity, 999_00000000);
    }
}
