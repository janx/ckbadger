use ckb_store_reader::CkbChainReader;
use ckbadger_common::sync::{format_duration_smart, SyncProgressData};
use ckbadger_store::CkbadgerStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

use super::manager::{BroadcastMessage, SyncStatus, WsManager};
use crate::routes::activities::build_global_activity_response;
use crate::utils::format_duration;

pub(crate) const FAST_SYNC_THRESHOLD: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncMode {
    FastSync,
    Realtime,
}

pub(crate) fn determine_sync_mode(synced_block: i64, tip_block: i64) -> SyncMode {
    let blocks_behind = tip_block - synced_block;
    if blocks_behind > FAST_SYNC_THRESHOLD {
        SyncMode::FastSync
    } else {
        SyncMode::Realtime
    }
}

/// Maximum gap before skipping intermediate blocks during WS broadcast.
pub(crate) const BROADCAST_GAP_THRESHOLD: i64 = 20;

pub(crate) fn adjust_for_broadcast_gap(
    sync_mode: SyncMode,
    last_block: i64,
    current_block: i64,
) -> Option<i64> {
    if sync_mode == SyncMode::Realtime {
        let gap = current_block - last_block;
        if gap > BROADCAST_GAP_THRESHOLD {
            return Some(current_block - 1);
        }
    }
    None
}

pub async fn start_block_broadcaster(
    store: Arc<CkbadgerStore>,
    ws_manager: Arc<WsManager>,
    ckb_rpc_url: String,
    ckb_store: Option<Arc<CkbChainReader>>,
) {
    let mut last_block_number: Option<i64> = None;
    let mut ticker = interval(Duration::from_secs(2));

    loop {
        ticker.tick().await;

        let tip_block = fetch_tip_block(&ckb_rpc_url).await.unwrap_or(0) as i64;

        // Get latest block from store
        let latest_block = match store.get_sync_tip_block() {
            Ok(Some((number, header))) => Some((number, header)),
            Ok(None) => continue,
            Err(e) => {
                error!("Failed to query latest block from store: {}", e);
                continue;
            }
        };

        let (number, header) = match latest_block {
            Some(b) => b,
            None => continue,
        };

        if last_block_number.is_none() {
            last_block_number = Some(number);
            continue;
        }

        if Some(number) == last_block_number {
            continue;
        }

        let sync_mode = determine_sync_mode(number, tip_block);

        if let Some(last) = last_block_number {
            if let Some(adjusted) = adjust_for_broadcast_gap(sync_mode, last, number) {
                info!(
                    "Skipping {} blocks for WS broadcast (fast-sync catchup: {} -> {})",
                    number - last - 1,
                    last,
                    number
                );
                last_block_number = Some(adjusted);
            }
        }

        let hash_hex = format!("0x{}", hex::encode(&header.hash));
        let epoch_number = header.epoch_number;
        let epoch_index = header.epoch_index;
        let epoch_length = header.epoch_length;
        let tx_count = header.transactions_count;

        if sync_mode == SyncMode::FastSync {
            let sync_status = build_sync_status(&store, tip_block);
            let (avg_block_time, estimated_epoch_time) =
                calculate_epoch_stats(&store, number, epoch_index, epoch_length);

            let timestamp_ms = header.timestamp;
            let timestamp_str = chrono::DateTime::from_timestamp(timestamp_ms / 1000, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();

            let msg = BroadcastMessage::NewBlock {
                number,
                hash: hash_hex,
                timestamp: timestamp_str,
                transactions_count: tx_count,
                epoch_number,
                epoch_index,
                epoch_length,
                avg_block_time,
                estimated_epoch_time,
                sync_status: Box::new(sync_status),
            };
            debug!(
                "Fast-sync: broadcasting latest block {} ({} behind)",
                number,
                tip_block - number
            );
            ws_manager.broadcast_block(msg);
            last_block_number = Some(number);
        } else {
            let last =
                last_block_number.expect("last_block_number must be Some after initial setup");

            // Get blocks between last and current
            let mut new_blocks = Vec::new();
            for block_num in (last + 1)..=(number.min(last + 20)) {
                if let Ok(Some(hdr)) = store.get_block_header(block_num) {
                    new_blocks.push((block_num, hdr));
                }
            }

            for (num, hdr) in new_blocks {
                let sync_status = build_sync_status(&store, tip_block);
                let (avg_block_time, estimated_epoch_time) =
                    calculate_epoch_stats(&store, num, hdr.epoch_index, hdr.epoch_length);

                let timestamp_str = chrono::DateTime::from_timestamp(hdr.timestamp / 1000, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default();

                let msg = BroadcastMessage::NewBlock {
                    number: num,
                    hash: format!("0x{}", hex::encode(&hdr.hash)),
                    timestamp: timestamp_str,
                    transactions_count: hdr.transactions_count,
                    epoch_number: hdr.epoch_number,
                    epoch_index: hdr.epoch_index,
                    epoch_length: hdr.epoch_length,
                    avg_block_time,
                    estimated_epoch_time,
                    sync_status: Box::new(sync_status),
                };
                info!("Broadcasting new block: {}", num);
                ws_manager.broadcast_block(msg);

                broadcast_block_transactions(&store, &ws_manager, &ckb_store, num);
                last_block_number = Some(num);
            }

            // Broadcast latest activities after new blocks
            broadcast_latest_activities(&store, &ws_manager);
        }
    }
}

fn get_block_tx_hashes(
    ckb_store: &Option<Arc<CkbChainReader>>,
    block_num: i64,
) -> Option<Vec<Vec<u8>>> {
    let store = ckb_store.as_ref()?;
    let block_hash_bytes = store.get_block_hash(block_num as u64)?;
    let block = store.get_block(&block_hash_bytes)?;
    Some(
        block
            .transactions()
            .into_iter()
            .map(|tx| tx.hash().raw_data().to_vec())
            .collect(),
    )
}

fn broadcast_block_transactions(
    store: &CkbadgerStore,
    ws_manager: &Arc<WsManager>,
    ckb_store: &Option<Arc<CkbChainReader>>,
    block_number: i64,
) {
    let tx_hashes = match get_block_tx_hashes(ckb_store, block_number) {
        Some(h) => h,
        None => {
            debug!(
                "Cannot resolve tx hashes for block {} (CKB store unavailable)",
                block_number
            );
            return;
        }
    };

    let txs = match store.list_block_txs(block_number) {
        Ok(txs) => txs,
        Err(e) => {
            error!("Failed to query block transactions: {}", e);
            return;
        }
    };

    let block_header = store.get_block_header(block_number).ok().flatten();
    let timestamp_str = block_header
        .and_then(|h| chrono::DateTime::from_timestamp(h.timestamp / 1000, 0))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    for (tx_idx, entry) in txs {
        if entry.is_cellbase {
            continue;
        }

        let tx_hash = match tx_hashes.get(tx_idx as usize) {
            Some(h) => format!("0x{}", hex::encode(h)),
            None => continue,
        };

        let msg = BroadcastMessage::NewTransaction {
            hash: tx_hash,
            block_number,
            inputs_count: entry.inputs_count as i32,
            outputs_count: entry.outputs_count as i32,
            fee: entry.fee.to_string(),
            timestamp: timestamp_str.clone(),
        };
        ws_manager.broadcast_transaction(msg);
    }
}

fn broadcast_latest_activities(store: &CkbadgerStore, ws_manager: &Arc<WsManager>) {
    match store.get_latest_activities() {
        Ok(items) if !items.is_empty() => {
            let mut script_info_cache = HashMap::new();
            let activities: Vec<serde_json::Value> = items
                .into_iter()
                .take(8)
                .filter_map(|item| {
                    match build_global_activity_response(
                        store,
                        "mainnet", // TODO: derive from config if needed
                        &item,
                        &mut script_info_cache,
                    ) {
                        Ok(activity) => match serde_json::to_value(activity) {
                            Ok(value) => Some(value),
                            Err(e) => {
                                error!(
                                    "Failed to encode latest activity broadcast for tx=0x{}: {}",
                                    hex::encode(&item.entry.tx_hash),
                                    e
                                );
                                None
                            }
                        },
                        Err(e) => {
                            error!(
                                "Failed to serialize latest activity broadcast for tx=0x{}: {}",
                                hex::encode(&item.entry.tx_hash),
                                e
                            );
                            None
                        }
                    }
                })
                .collect();
            ws_manager.broadcast_activities(BroadcastMessage::LatestActivities { activities });
        }
        _ => {}
    }
}

fn calculate_epoch_stats(
    store: &CkbadgerStore,
    latest_block: i64,
    epoch_index: i32,
    epoch_length: i32,
) -> (String, String) {
    let avg_time = if latest_block > 0 {
        let prev_header = store.get_block_header(latest_block - 1).ok().flatten();
        let curr_header = store.get_block_header(latest_block).ok().flatten();

        match (prev_header, curr_header) {
            (Some(prev), Some(curr)) => {
                let diff_ms = curr.timestamp - prev.timestamp;
                (diff_ms as f64 / 1000.0).max(1.0)
            }
            _ => 10.0,
        }
    } else {
        10.0
    };

    let avg_block_time = format!("{:.2}s", avg_time);

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_seconds = (remaining_blocks as f64 * avg_time) as u64;
    let estimated_epoch_time = format_duration(estimated_seconds);

    (avg_block_time, estimated_epoch_time)
}

fn build_sync_status(store: &CkbadgerStore, tip_block: i64) -> SyncStatus {
    let store_sync = store.get_sync_status().unwrap_or_default();
    let synced_block = store_sync.tip_block_number;
    let db_ema_rate = store_sync.sync_ema_rate;
    let sync_started_at = store_sync.sync_started_at;
    let bulk_sync_completed_at = store_sync.bulk_sync_completed_at;

    let blocks_behind = tip_block - synced_block;
    let is_syncing = blocks_behind > 100;
    let is_bulk_syncing = blocks_behind > 1000;

    let sync_mode = if bulk_sync_completed_at.is_some() && !is_syncing {
        "synced".to_string()
    } else if is_bulk_syncing {
        "bulk".to_string()
    } else if is_syncing {
        "normal".to_string()
    } else {
        "synced".to_string()
    };

    let now = chrono::Utc::now().timestamp();
    let elapsed_time = sync_started_at.map(|started| {
        let end = bulk_sync_completed_at.unwrap_or(now);
        format_duration_smart((end - started) as f64)
    });

    let total_time =
        if let (Some(started), Some(completed)) = (sync_started_at, bulk_sync_completed_at) {
            Some(format_duration_smart((completed - started) as f64))
        } else {
            None
        };

    let sync_progress_from_store: Option<SyncProgressData> = store
        .get_sync_progress()
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());

    let (
        progress,
        estimated_time,
        blocks_per_second,
        ema_blocks_per_second,
        txs_per_second,
        ema_txs_per_second,
    ) = if let Some(ref sp) = sync_progress_from_store {
        let stale = chrono::Utc::now().timestamp() - sp.updated_at > 60;
        if !stale && is_syncing {
            (
                sp.progress_percentage,
                Some(sp.eta_formatted.clone()),
                Some(sp.blocks_per_second),
                Some(sp.ema_blocks_per_second),
                sp.txs_per_second,
                sp.ema_txs_per_second,
            )
        } else {
            let p = if tip_block > 0 {
                (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let (ema, eta) = if is_syncing {
                if let Some(rate) = db_ema_rate {
                    if rate > 0.0 {
                        let remaining = blocks_behind as f64;
                        let eta_secs = remaining / rate;
                        let eta_str = format_duration_smart(eta_secs);
                        (Some(rate), Some(eta_str))
                    } else {
                        (Some(rate), None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };
            (p, eta, ema, ema, None, None)
        }
    } else {
        let p = if tip_block > 0 {
            (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        let (ema, eta) = if is_syncing {
            if let Some(rate) = db_ema_rate {
                if rate > 0.0 {
                    let remaining = blocks_behind as f64;
                    let eta_secs = remaining / rate;
                    let eta_str = format_duration_smart(eta_secs);
                    (Some(rate), Some(eta_str))
                } else {
                    (Some(rate), None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        (p, eta, ema, ema, None, None)
    };

    SyncStatus {
        is_syncing,
        synced_block,
        tip_block,
        progress,
        estimated_time,
        chart_data_may_be_incomplete: blocks_behind > 1000,
        blocks_per_second,
        ema_blocks_per_second,
        txs_per_second,
        ema_txs_per_second,
        sync_mode,
        started_at: sync_started_at,
        elapsed_time,
        total_time,
    }
}

async fn fetch_tip_block(ckb_rpc_url: &str) -> Result<u64, String> {
    #[derive(serde::Serialize)]
    struct RpcRequest {
        jsonrpc: &'static str,
        method: &'static str,
        params: Vec<()>,
        id: u64,
    }

    #[derive(serde::Deserialize)]
    struct RpcResponse {
        result: Option<String>,
    }

    let client = reqwest::Client::new();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "get_tip_block_number",
        params: vec![],
        id: 1,
    };

    let response = client
        .post(ckb_rpc_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<RpcResponse>()
        .await
        .map_err(|e| e.to_string())?;

    let hex = response.result.ok_or("Empty RPC response")?;
    let hex = hex.strip_prefix("0x").unwrap_or(&hex);
    u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
}

pub async fn start_reorg_broadcaster(store: Arc<CkbadgerStore>, ws_manager: Arc<WsManager>) {
    let mut last_deep_fork_state: Option<bool> = None;
    let mut ticker = interval(Duration::from_secs(5));

    loop {
        ticker.tick().await;

        // Check for deep fork state changes from the store
        let deep_fork_info = match store.get_deep_fork_info() {
            Ok(info) => info,
            Err(e) => {
                error!("Failed to query deep fork info: {}", e);
                continue;
            }
        };

        let detected = deep_fork_info.is_some();

        let should_broadcast = match last_deep_fork_state {
            None => detected,
            Some(prev) => prev != detected,
        };

        if should_broadcast {
            last_deep_fork_state = Some(detected);

            if let Some(info) = deep_fork_info {
                let msg = BroadcastMessage::DeepFork {
                    detected: true,
                    depth: info.depth,
                    db_tip: info.db_tip,
                    chain_tip: info.chain_tip,
                    fork_point: info.fork_point,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                info!("Broadcasting deep fork event: depth={}", info.depth);
                ws_manager.broadcast_reorg(msg);
            } else {
                let msg = BroadcastMessage::DeepFork {
                    detected: false,
                    depth: 0,
                    db_tip: 0,
                    chain_tip: 0,
                    fork_point: 0,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                info!("Broadcasting deep fork resolution");
                ws_manager.broadcast_reorg(msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_sync_mode_fast_sync_when_far_behind() {
        assert_eq!(determine_sync_mode(1000, 2000), SyncMode::FastSync);
        assert_eq!(determine_sync_mode(0, 18_000_000), SyncMode::FastSync);
    }

    #[test]
    fn test_determine_sync_mode_realtime_when_caught_up() {
        assert_eq!(determine_sync_mode(1000, 1000), SyncMode::Realtime);
        assert_eq!(determine_sync_mode(1000, 1050), SyncMode::Realtime);
        assert_eq!(determine_sync_mode(1000, 1100), SyncMode::Realtime);
    }

    #[test]
    fn test_determine_sync_mode_boundary() {
        assert_eq!(determine_sync_mode(1000, 1100), SyncMode::Realtime);
        assert_eq!(determine_sync_mode(1000, 1101), SyncMode::FastSync);
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(3599), "59m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(86399), "23h 59m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
        assert_eq!(format_duration(172800), "2d");
        assert_eq!(format_duration(259200), "3d");
    }

    #[test]
    fn test_broadcast_gap_no_adjustment_in_fast_sync() {
        let result = adjust_for_broadcast_gap(SyncMode::FastSync, 1000, 5000);
        assert!(result.is_none());
    }

    #[test]
    fn test_broadcast_gap_no_adjustment_for_small_gap() {
        let result = adjust_for_broadcast_gap(SyncMode::Realtime, 1000, 1005);
        assert!(result.is_none());
    }

    #[test]
    fn test_broadcast_gap_at_threshold_no_adjustment() {
        let result = adjust_for_broadcast_gap(SyncMode::Realtime, 1000, 1020);
        assert!(result.is_none());
    }

    #[test]
    fn test_broadcast_gap_above_threshold_adjusts() {
        let result = adjust_for_broadcast_gap(SyncMode::Realtime, 1000, 1021);
        assert_eq!(result, Some(1020));
    }

    #[test]
    fn test_broadcast_gap_large_skip() {
        let result = adjust_for_broadcast_gap(SyncMode::Realtime, 16_000_000, 18_000_000);
        assert_eq!(result, Some(17_999_999));
    }

    #[test]
    fn test_broadcast_gap_threshold_value() {
        assert_eq!(BROADCAST_GAP_THRESHOLD, 20);
    }
}
