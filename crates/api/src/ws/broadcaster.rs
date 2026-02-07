use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::cache::CacheBackend;
use crate::db::DbPool;
use crate::rpc::{parse_hex_u64, CkbRpcClient};
use crate::ws::manager::{BroadcastMessage, IndexRebuildStatus, SyncStatus};
use crate::ws::WsManager;

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockRow {
    number: u64,
    hash: String,
    timestamp: i64,
    transactions_count: u32,
    epoch_number: u64,
    epoch_index: u32,
    epoch_length: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TaskRow {
    #[allow(dead_code)]
    task_type: String,
    status: String,
    progress_current: i64,
    progress_total: i64,
    progress_details: String,
}

pub async fn start_block_broadcaster(
    pool: DbPool,
    ws_manager: Arc<WsManager>,
    ckb_rpc_url: String,
    _cache: CacheBackend,
) {
    info!("Starting WebSocket block broadcaster");

    let mut poll_interval = interval(Duration::from_secs(2));
    let mut last_block_number: Option<u64> = None;

    loop {
        poll_interval.tick().await;

        let sync_status_data = _cache.get_sync_status(&pool).await;
        let redis_tip = sync_status_data.tip_block_number as u64;

        if last_block_number == Some(redis_tip) {
            continue;
        }

        let hash_without_prefix = sync_status_data
            .tip_block_hash
            .strip_prefix("0x")
            .unwrap_or(&sync_status_data.tip_block_hash);

        let query = format!(
            r#"
            SELECT 
                b.number as number,
                hex(b.hash) as hash,
                b.timestamp as timestamp,
                b.transactions_count as transactions_count,
                b.epoch_number as epoch_number,
                b.epoch_index as epoch_index,
                b.epoch_length as epoch_length
            FROM blocks_all b
            WHERE b.number = {} AND b.hash = unhex('{}')
            LIMIT 1
            "#,
            redis_tip, hash_without_prefix
        );

        let block_row: Option<BlockRow> = match pool.query_one(&query).await {
            Ok(row) => row,
            Err(e) => {
                warn!("Failed to query block {} for broadcaster: {}", redis_tip, e);
                continue;
            }
        };

        let Some(block) = block_row else {
            continue;
        };

        debug!("Broadcasting new block: {}", block.number);
        last_block_number = Some(block.number);

        let rpc = CkbRpcClient::new(&ckb_rpc_url);
        let sync_progress = _cache.get_sync_progress().await;
        let sync_status_data = _cache.get_sync_status(&pool).await;

        let (tip_number, is_syncing, sync_status) = match rpc.get_tip_header().await {
            Ok(tip) => {
                let tip_num = parse_hex_u64(&tip.number).unwrap_or(0);
                let syncing = tip_num > block.number + 10;
                let progress = if tip_num > 0 {
                    (block.number as f64 / tip_num as f64) * 100.0
                } else {
                    100.0
                };

                let (ema_bps, bps, eta, elapsed, started, total) =
                    if let Some(ref sp) = sync_progress {
                        let eta_str = sp.eta_formatted.clone();
                        let elapsed = sync_status_data
                            .bulk_sync_elapsed_seconds()
                            .map(|s| ckbadger_common::sync::format_duration_smart(s as f64));
                        let total = sync_status_data
                            .bulk_sync_total_seconds()
                            .map(|s| ckbadger_common::sync::format_duration_smart(s as f64));
                        (
                            Some(sp.ema_blocks_per_second),
                            Some(sp.blocks_per_second),
                            if eta_str.is_empty() {
                                None
                            } else {
                                Some(eta_str)
                            },
                            elapsed,
                            sync_status_data.sync_started_at,
                            total,
                        )
                    } else {
                        (
                            None,
                            None,
                            None,
                            None,
                            sync_status_data.sync_started_at,
                            None,
                        )
                    };

                let status = SyncStatus {
                    is_syncing: syncing,
                    synced_block: block.number as i64,
                    tip_block: tip_num as i64,
                    progress,
                    estimated_time: eta,
                    chart_data_may_be_incomplete: syncing,
                    blocks_per_second: bps,
                    ema_blocks_per_second: ema_bps,
                    sync_mode: if syncing {
                        "bulk".to_string()
                    } else {
                        "live".to_string()
                    },
                    started_at: started,
                    elapsed_time: elapsed,
                    total_time: total,
                };
                (tip_num, syncing, status)
            }
            Err(e) => {
                warn!("Failed to get tip header for broadcaster: {}", e);
                let status = SyncStatus {
                    is_syncing: false,
                    synced_block: block.number as i64,
                    tip_block: block.number as i64,
                    progress: 100.0,
                    estimated_time: None,
                    chart_data_may_be_incomplete: false,
                    blocks_per_second: None,
                    ema_blocks_per_second: None,
                    sync_mode: "live".to_string(),
                    started_at: None,
                    elapsed_time: None,
                    total_time: None,
                };
                (block.number, false, status)
            }
        };

        let index_rebuild_status = get_index_rebuild_status(&pool).await;

        let blocks_remaining = block.epoch_length.saturating_sub(block.epoch_index);
        let estimated_epoch_seconds = blocks_remaining as u64 * 8;
        let estimated_epoch_time = format_duration(estimated_epoch_seconds);

        let avg_block_time = "~8s".to_string();

        let msg = BroadcastMessage::NewBlock {
            number: block.number as i64,
            hash: format!("0x{}", block.hash.to_lowercase()),
            timestamp: chrono::DateTime::from_timestamp_millis(block.timestamp)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
            transactions_count: block.transactions_count as i32,
            epoch_number: block.epoch_number as i64,
            epoch_index: block.epoch_index as i32,
            epoch_length: block.epoch_length as i32,
            avg_block_time,
            estimated_epoch_time,
            sync_status: Box::new(sync_status),
            index_rebuild_status,
        };

        ws_manager.broadcast_block(msg);

        let _ = (tip_number, is_syncing);
    }
}

async fn get_index_rebuild_status(pool: &DbPool) -> Option<IndexRebuildStatus> {
    let query = r#"
        SELECT task_type, status, progress_current, progress_total, progress_details
        FROM tasks
        WHERE task_type = 'index_rebuild' AND status IN ('pending', 'running')
        ORDER BY created_at DESC
        LIMIT 1
    "#;

    let task: Option<TaskRow> = pool.query_one(query).await.ok().flatten();

    task.map(|t| {
        let progress = if t.progress_total > 0 {
            (t.progress_current as f64 / t.progress_total as f64) * 100.0
        } else {
            0.0
        };

        IndexRebuildStatus {
            is_rebuilding: t.status == "running",
            total: t.progress_total as i32,
            completed: t.progress_current as i32,
            current_index: if t.progress_details.is_empty() {
                None
            } else {
                Some(t.progress_details)
            },
            progress,
        }
    })
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

pub async fn start_reorg_broadcaster(_pool: DbPool, _ws_manager: Arc<WsManager>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_hours_and_minutes() {
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h 0m");
        assert_eq!(format_duration(7320), "2h 2m");
    }

    #[test]
    fn test_format_duration_minutes_only() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(30), "0m");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(3599), "59m");
    }

    #[test]
    fn test_format_duration_large_values() {
        assert_eq!(format_duration(86400), "24h 0m");
        assert_eq!(format_duration(90000), "25h 0m");
    }
}
