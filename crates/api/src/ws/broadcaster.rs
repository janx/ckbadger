use chrono::{DateTime, Utc};
use ckbadger_common::{format_duration_smart, SyncProgressData, SYNC_PROGRESS_REDIS_KEY};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

use super::manager::{BroadcastMessage, IndexRebuildStatus, SyncStatus, WsManager};
use crate::cache::CacheBackend;

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

pub async fn start_block_broadcaster(
    pool: PgPool,
    ws_manager: Arc<WsManager>,
    ckb_rpc_url: String,
    cache: CacheBackend,
) {
    let mut last_block_number: Option<i64> = None;
    let mut ticker = interval(Duration::from_secs(2));

    loop {
        ticker.tick().await;

        let tip_block = fetch_tip_block(&ckb_rpc_url).await.unwrap_or(0) as i64;

        let latest_result = sqlx::query_as::<
            _,
            (
                i64,
                Vec<u8>,
                chrono::DateTime<chrono::Utc>,
                i32,
                i64,
                i32,
                i32,
            ),
        >(
            "SELECT number, hash, timestamp, transactions_count, epoch_number, epoch_index, epoch_length 
             FROM blocks ORDER BY number DESC LIMIT 1",
        )
        .fetch_optional(&pool)
        .await;

        let latest_block = match latest_result {
            Ok(Some(block)) => block,
            Ok(None) => continue,
            Err(e) => {
                error!("Failed to query latest block: {}", e);
                continue;
            }
        };

        let (number, hash, timestamp, tx_count, epoch_number, epoch_index, epoch_length) =
            latest_block;

        if last_block_number.is_none() {
            last_block_number = Some(number);
            continue;
        }

        if Some(number) == last_block_number {
            continue;
        }

        let sync_mode = determine_sync_mode(number, tip_block);

        if sync_mode == SyncMode::FastSync {
            let sync_status = build_sync_status(&pool, &cache, tip_block).await;
            let index_rebuild_status = build_index_rebuild_status(&pool).await;
            let (avg_block_time, estimated_epoch_time) =
                calculate_epoch_stats(&pool, number, epoch_index, epoch_length).await;

            let msg = BroadcastMessage::NewBlock {
                number,
                hash: format!("0x{}", hex::encode(&hash)),
                timestamp: timestamp.to_rfc3339(),
                transactions_count: tx_count,
                epoch_number,
                epoch_index,
                epoch_length,
                avg_block_time,
                estimated_epoch_time,
                sync_status,
                index_rebuild_status,
            };
            debug!(
                "Fast-sync: broadcasting latest block {} ({} behind)",
                number,
                tip_block - number
            );
            ws_manager.broadcast_block(msg);
            last_block_number = Some(number);
        } else {
            let last = last_block_number.unwrap();
            let new_blocks = sqlx::query_as::<
                _,
                (
                    i64,
                    Vec<u8>,
                    chrono::DateTime<chrono::Utc>,
                    i32,
                    i64,
                    i32,
                    i32,
                ),
            >(
                "SELECT number, hash, timestamp, transactions_count, epoch_number, epoch_index, epoch_length 
                 FROM blocks WHERE number > $1 ORDER BY number ASC LIMIT 20",
            )
            .bind(last)
            .fetch_all(&pool)
            .await;

            match new_blocks {
                Ok(blocks) => {
                    for (num, h, ts, txc, ep_num, ep_idx, ep_len) in blocks {
                        let sync_status = build_sync_status(&pool, &cache, tip_block).await;
                        let index_rebuild_status = build_index_rebuild_status(&pool).await;
                        let (avg_block_time, estimated_epoch_time) =
                            calculate_epoch_stats(&pool, num, ep_idx, ep_len).await;

                        let msg = BroadcastMessage::NewBlock {
                            number: num,
                            hash: format!("0x{}", hex::encode(&h)),
                            timestamp: ts.to_rfc3339(),
                            transactions_count: txc,
                            epoch_number: ep_num,
                            epoch_index: ep_idx,
                            epoch_length: ep_len,
                            avg_block_time,
                            estimated_epoch_time,
                            sync_status,
                            index_rebuild_status,
                        };
                        info!("Broadcasting new block: {}", num);
                        ws_manager.broadcast_block(msg);

                        broadcast_block_transactions(&pool, &ws_manager, num).await;
                        last_block_number = Some(num);
                    }
                }
                Err(e) => {
                    error!("Failed to query new blocks: {}", e);
                }
            }
        }
    }
}

async fn broadcast_block_transactions(
    pool: &PgPool,
    ws_manager: &Arc<WsManager>,
    block_number: i64,
) {
    let txs = sqlx::query_as::<_, (Vec<u8>, i32, i32, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT hash, inputs_count::int4, outputs_count::int4, fee::text, timestamp
        FROM transactions 
        WHERE block_number = $1 AND is_cellbase = false
        ORDER BY tx_index
        "#,
    )
    .bind(block_number)
    .fetch_all(pool)
    .await;

    match txs {
        Ok(transactions) => {
            for (hash, inputs_count, outputs_count, fee, timestamp) in transactions {
                let msg = BroadcastMessage::NewTransaction {
                    hash: format!("0x{}", hex::encode(&hash)),
                    block_number,
                    inputs_count,
                    outputs_count,
                    fee,
                    timestamp: timestamp.to_rfc3339(),
                };
                debug!("Broadcasting new transaction: {}", hex::encode(&hash));
                ws_manager.broadcast_transaction(msg);
            }
        }
        Err(e) => {
            error!("Failed to query block transactions: {}", e);
        }
    }
}

async fn calculate_epoch_stats(
    pool: &PgPool,
    latest_block: i64,
    epoch_index: i32,
    epoch_length: i32,
) -> (String, String) {
    // Use last 2 blocks for avg time calculation (avoids expensive self-join on 100 blocks)
    let blocks_result = sqlx::query_as::<_, (DateTime<Utc>,)>(
        r#"
        SELECT timestamp FROM blocks
        WHERE number >= $1 - 1 AND number <= $1
        ORDER BY number ASC
        "#,
    )
    .bind(latest_block)
    .fetch_all(pool)
    .await;

    let avg_time = blocks_result
        .ok()
        .and_then(|blocks| {
            if blocks.len() == 2 {
                let duration = blocks[1].0.signed_duration_since(blocks[0].0).num_seconds() as f64;
                Some(duration.max(1.0))
            } else {
                None
            }
        })
        .unwrap_or(10.0);

    let avg_block_time = format!("{:.2}s", avg_time);

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_seconds = (remaining_blocks as f64 * avg_time) as u64;
    let estimated_epoch_time = format_duration(estimated_seconds);

    (avg_block_time, estimated_epoch_time)
}

async fn build_sync_status(pool: &PgPool, cache: &CacheBackend, tip_block: i64) -> SyncStatus {
    let sync_row: Option<(i64, Option<f64>)> =
        sqlx::query_as("SELECT tip_block_number, sync_ema_rate FROM sync_status WHERE id = 1")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    let (synced_block, db_ema_rate) = sync_row.unwrap_or((0, None));
    let blocks_behind = tip_block - synced_block;
    let is_syncing = blocks_behind > 100;

    let sync_progress_from_redis: Option<SyncProgressData> =
        cache.get(SYNC_PROGRESS_REDIS_KEY).await;

    let (progress, estimated_time, blocks_per_second, ema_blocks_per_second) =
        if let Some(ref sp) = sync_progress_from_redis {
            let stale = Utc::now().timestamp() - sp.updated_at > 60;
            if !stale && is_syncing {
                (
                    sp.progress_percentage,
                    Some(sp.eta_formatted.clone()),
                    Some(sp.blocks_per_second),
                    Some(sp.ema_blocks_per_second),
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
                (p, eta, ema, ema)
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
            (p, eta, ema, ema)
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
    }
}

async fn build_index_rebuild_status(pool: &PgPool) -> Option<IndexRebuildStatus> {
    #[derive(serde::Deserialize)]
    struct ProgressData {
        total: i32,
        completed: i32,
        current: Option<String>,
    }

    let row: Option<(bool, Option<String>)> = sqlx::query_as(
        "SELECT COALESCE(indexes_deferred, false), indexes_rebuild_progress FROM sync_status WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let (indexes_deferred, progress_json) = row?;
    if !indexes_deferred {
        return None;
    }

    let progress_data = progress_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<ProgressData>(json).ok());

    let (total, completed, current_index) = match progress_data {
        Some(p) => (p.total, p.completed, p.current),
        None => (0, 0, None),
    };

    let is_rebuilding = completed < total && total > 0;
    if !is_rebuilding && total == 0 {
        return None;
    }

    let progress = if total > 0 {
        (completed as f64 / total as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    Some(IndexRebuildStatus {
        is_rebuilding,
        total,
        completed,
        current_index,
        progress,
    })
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
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

pub async fn start_reorg_broadcaster(pool: PgPool, ws_manager: Arc<WsManager>) {
    let mut last_reorg_id: Option<i64> = None;
    let mut last_deep_fork_state: Option<bool> = None;
    let mut ticker = interval(Duration::from_secs(5));

    loop {
        ticker.tick().await;

        let reorg_result =
            sqlx::query_as::<_, (i64, String, i32, i64, i64, i64, i32, i32, DateTime<Utc>)>(
                r#"
            SELECT id, event_type, depth, old_tip_number, new_tip_number, fork_point_number,
                   orphaned_blocks_count, orphaned_transactions_count, detected_at
            FROM reorg_events
            WHERE ($1::bigint IS NULL OR id > $1)
            ORDER BY id DESC
            LIMIT 1
            "#,
            )
            .bind(last_reorg_id)
            .fetch_optional(&pool)
            .await;

        if let Ok(Some((
            id,
            event_type,
            depth,
            old_tip,
            new_tip,
            fork_point,
            orphaned_blocks,
            orphaned_txs,
            detected_at,
        ))) = reorg_result
        {
            last_reorg_id = Some(id);

            if event_type == "deep" {
                let msg = BroadcastMessage::DeepFork {
                    detected: true,
                    depth,
                    db_tip: old_tip,
                    chain_tip: new_tip,
                    fork_point,
                    timestamp: detected_at.to_rfc3339(),
                };
                info!("Broadcasting deep fork event: depth={}", depth);
                ws_manager.broadcast_reorg(msg);
            } else {
                let msg = BroadcastMessage::Reorg {
                    depth,
                    old_tip,
                    new_tip,
                    fork_point,
                    orphaned_blocks,
                    orphaned_txs,
                    timestamp: detected_at.to_rfc3339(),
                };
                info!(
                    "Broadcasting reorg event: depth={}, old_tip={}, new_tip={}",
                    depth, old_tip, new_tip
                );
                ws_manager.broadcast_reorg(msg);
            }
        }

        let deep_fork_result = sqlx::query_as::<
            _,
            (
                bool,
                Option<i32>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<DateTime<Utc>>,
            ),
        >(
            r#"
            SELECT COALESCE(deep_fork_detected, FALSE), deep_fork_depth, deep_fork_db_tip, 
                   deep_fork_chain_tip, deep_fork_fork_point, deep_fork_at
            FROM sync_status WHERE id = 1
            "#,
        )
        .fetch_optional(&pool)
        .await;

        if let Ok(Some((detected, depth, db_tip, chain_tip, fork_point, detected_at))) =
            deep_fork_result
        {
            let should_broadcast = match last_deep_fork_state {
                None => detected,
                Some(prev) => prev != detected,
            };

            if should_broadcast {
                last_deep_fork_state = Some(detected);
                let msg = BroadcastMessage::DeepFork {
                    detected,
                    depth: depth.unwrap_or(0),
                    db_tip: db_tip.unwrap_or(0),
                    chain_tip: chain_tip.unwrap_or(0),
                    fork_point: fork_point.unwrap_or(0),
                    timestamp: detected_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
                };
                info!(
                    "Broadcasting deep fork status change: detected={}",
                    detected
                );
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
}
