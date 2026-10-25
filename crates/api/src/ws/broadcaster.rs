use ckbadger_common::sync::{format_duration_smart, SyncProgressData};
use ckbadger_store::CkbadgerStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

use super::manager::{BroadcastMessage, SyncStatus, WsManager};
use crate::routes::activities::build_global_activity_response;

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
    append_only_store: Arc<CkbadgerStore>,
    ws_manager: Arc<WsManager>,
    ckb_rpc_url: String,
    ckb_network: String,
) {
    let mut last_block_number: Option<i64> = None;
    let mut ticker = interval(Duration::from_secs(2));
    let http_client = reqwest::Client::new();

    loop {
        ticker.tick().await;

        let tip_block = match fetch_tip_block(&http_client, &ckb_rpc_url).await {
            Ok(tip) => match i64::try_from(tip) {
                Ok(tip) => tip,
                Err(_) => {
                    error!(tip, "RPC tip exceeds supported block number range");
                    continue;
                }
            },
            Err(e) => {
                error!("Failed to fetch RPC tip for WS broadcast: {e}");
                continue;
            }
        };

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
            let (sync_status, timestamp_str) = match build_sync_status(&store, tip_block)
                .and_then(|status| Ok((status, format_block_timestamp(header.timestamp)?)))
            {
                Ok(payload) => payload,
                Err(e) => {
                    error!(block = number, "Failed to build WS block payload: {e:#}");
                    continue;
                }
            };

            let msg = BroadcastMessage::NewBlock {
                number,
                hash: hash_hex,
                timestamp: timestamp_str,
                transactions_count: tx_count,
                epoch_number,
                epoch_index,
                epoch_length,
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
            let mut block_read_error = None;
            for block_num in (last + 1)..=(number.min(last + 20)) {
                match store.get_block_header(block_num) {
                    Ok(Some(hdr)) => new_blocks.push((block_num, hdr)),
                    Ok(None) => {
                        block_read_error = Some(format!("missing block header {block_num}"));
                        break;
                    }
                    Err(e) => {
                        block_read_error =
                            Some(format!("failed to read block header {block_num}: {e}"));
                        break;
                    }
                }
            }
            if let Some(e) = block_read_error {
                error!("Failed to prepare WS block range: {e}");
                continue;
            }

            let mut payload_failed = false;
            for (num, hdr) in new_blocks {
                let payload = (|| -> anyhow::Result<_> {
                    Ok((
                        build_sync_status(&store, tip_block)?,
                        format_block_timestamp(hdr.timestamp)?,
                        build_block_transaction_messages(&store, num)?,
                    ))
                })();
                let (sync_status, timestamp_str, transactions) = match payload {
                    Ok(payload) => payload,
                    Err(e) => {
                        error!(block = num, "Failed to build WS block payload: {e:#}");
                        payload_failed = true;
                        break;
                    }
                };

                let msg = BroadcastMessage::NewBlock {
                    number: num,
                    hash: format!("0x{}", hex::encode(&hdr.hash)),
                    timestamp: timestamp_str,
                    transactions_count: hdr.transactions_count,
                    epoch_number: hdr.epoch_number,
                    epoch_index: hdr.epoch_index,
                    epoch_length: hdr.epoch_length,
                    sync_status: Box::new(sync_status),
                };
                info!("Broadcasting new block: {}", num);
                ws_manager.broadcast_block(msg);

                for transaction in transactions {
                    ws_manager.broadcast_transaction(transaction);
                }
                last_block_number = Some(num);
            }

            // Broadcast latest activities after new blocks
            if !payload_failed {
                broadcast_latest_activities(&store, &append_only_store, &ws_manager, &ckb_network);
            }
        }
    }
}

fn format_block_timestamp(timestamp_ms: i64) -> anyhow::Result<String> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| anyhow::anyhow!("unrepresentable block timestamp_ms={timestamp_ms}"))
}

fn build_block_transaction_messages(
    store: &CkbadgerStore,
    block_number: i64,
) -> anyhow::Result<Vec<BroadcastMessage>> {
    let header = store.get_block_header(block_number)?.ok_or_else(|| {
        anyhow::anyhow!("missing block header {block_number} for transaction broadcast")
    })?;
    let timestamp_str = format_block_timestamp(header.timestamp)?;
    let txs = store.list_block_txs(block_number)?;
    let mut messages = Vec::new();
    for (tx_idx, entry) in txs {
        if entry.is_cellbase {
            continue;
        }
        if tx_idx < 0 {
            anyhow::bail!("negative transaction index {tx_idx} in block {block_number}");
        }
        messages.push(BroadcastMessage::NewTransaction {
            hash: format!("0x{}", hex::encode(entry.tx_hash)),
            block_number,
            inputs_count: i32::from(entry.inputs_count),
            outputs_count: i32::from(entry.outputs_count),
            fee: entry.fee.to_string(),
            timestamp: timestamp_str.clone(),
        });
    }
    Ok(messages)
}

fn broadcast_latest_activities(
    store: &CkbadgerStore,
    ao_store: &CkbadgerStore,
    ws_manager: &Arc<WsManager>,
    ckb_network: &str,
) {
    let items = match store.get_latest_activities() {
        Ok(items) if !items.is_empty() => items,
        Ok(_) => return,
        Err(e) => {
            error!("Failed to read latest activities for WS broadcast: {e}");
            return;
        }
    };
    let mut script_info_cache = HashMap::new();
    let mut address_cache = HashMap::new();
    let activities = items
        .iter()
        .take(8)
        .map(|actions| {
            build_global_activity_response(
                store,
                ao_store,
                ckb_network,
                actions,
                &mut script_info_cache,
                &mut address_cache,
            )
            .and_then(|activity| serde_json::to_value(activity).map_err(Into::into))
            .map_err(|e| anyhow::anyhow!("tx=0x{}: {e}", hex::encode(&actions.tx_hash)))
        })
        .collect::<anyhow::Result<Vec<serde_json::Value>>>();
    match activities {
        Ok(activities) => {
            ws_manager.broadcast_activities(BroadcastMessage::LatestActivities { activities })
        }
        Err(e) => error!("Failed to build latest activity WS payload: {e:#}"),
    }
}

fn build_sync_status(store: &CkbadgerStore, tip_block: i64) -> anyhow::Result<SyncStatus> {
    let store_sync = store.get_sync_status()?;
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
        .get_sync_progress()?
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()?;

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

    Ok(SyncStatus {
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
    })
}

async fn fetch_tip_block(client: &reqwest::Client, ckb_rpc_url: &str) -> Result<u64, String> {
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
    use ckbadger_store::{
        types::{CachedBlockHeader, TxIndexEntry},
        StoreBatch,
    };

    #[test]
    fn transaction_push_uses_canonical_index_hash_without_ckb_reader() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(
            7,
            &CachedBlockHeader {
                hash: vec![7; 32],
                parent_hash: vec![6; 32],
                timestamp: 1_700_000_000_123,
                chain_time_ms: 1_700_000_000_123,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 2,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
        batch.put_tx_index(
            7,
            1,
            &TxIndexEntry {
                tx_hash: [0xAB; 32],
                is_cellbase: false,
                timestamp: 1_700_000_000_123,
                inputs_count: 1,
                outputs_count: 2,
                fee: 42,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.commit().unwrap();
        let messages = build_block_transaction_messages(&store, 7).unwrap();
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            BroadcastMessage::NewTransaction {
                hash,
                timestamp,
                fee,
                ..
            } => {
                assert_eq!(hash, &format!("0x{}", "ab".repeat(32)));
                assert!(timestamp.contains(".123"));
                assert_eq!(fee, "42");
            }
            _ => panic!("expected transaction push"),
        }
    }

    #[test]
    fn ws_payload_rejects_missing_header_and_bad_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert!(build_block_transaction_messages(&store, 7)
            .unwrap_err()
            .to_string()
            .contains("missing block header 7"));
        assert!(format_block_timestamp(i64::MAX).is_err());
    }

    #[test]
    fn ws_sync_status_rejects_corrupt_progress_instead_of_zero_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.put_sync_progress(b"not-json").unwrap();
        assert!(build_sync_status(&store, 7).is_err());
    }

    /// A `new_block` push must never carry rolling network statistics. Pushing
    /// them meant a second computation path: the broadcaster derived the
    /// "average" block time from the newest block pair alone, so the client
    /// cache traded `/statistics/network`'s window average for single-interval
    /// noise (and a fabricated 10.00s whenever a header was missing) on every
    /// block.
    #[test]
    fn new_block_push_carries_no_rolling_network_statistics() {
        let msg = BroadcastMessage::NewBlock {
            number: 20_022_562,
            hash: format!("0x{}", "ab".repeat(32)),
            timestamp: "2026-08-01T00:00:21+08:00".to_string(),
            transactions_count: 1,
            epoch_number: 12_345,
            epoch_index: 900,
            epoch_length: 1800,
            sync_status: Box::new(SyncStatus {
                is_syncing: false,
                synced_block: 20_022_562,
                tip_block: 20_022_562,
                progress: 100.0,
                estimated_time: None,
                chart_data_may_be_incomplete: false,
                blocks_per_second: None,
                ema_blocks_per_second: None,
                txs_per_second: None,
                ema_txs_per_second: None,
                sync_mode: "synced".to_string(),
                started_at: None,
                elapsed_time: None,
                total_time: None,
            }),
        };

        let json = serde_json::to_value(&msg).expect("serialize new_block push");
        let data = json["data"].as_object().expect("new_block data object");

        assert!(!data.contains_key("avgBlockTime"));
        assert!(!data.contains_key("estimatedEpochTime"));
        // The block's own facts still travel with the push.
        assert_eq!(data["number"], 20_022_562);
        assert_eq!(data["epochIndex"], 900);
        assert_eq!(data["epochLength"], 1800);
    }

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
