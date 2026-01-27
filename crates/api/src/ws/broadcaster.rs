use chrono::{TimeZone, Utc};
use clickhouse::Row;
use serde::Deserialize;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

use super::manager::{BroadcastMessage, SyncStatus, WsManager};
use crate::clickhouse::{hex_hash, ClickHouseClient};

#[derive(Row, Deserialize)]
struct BlockRow {
    number: u64,
    hash: String,
    timestamp: u32,
    transactions_count: u32,
    epoch_number: u64,
    epoch_index: u32,
    epoch_length: u32,
}

#[derive(Row, Deserialize)]
struct TransactionRow {
    hash: String,
    inputs_count: u32,
    outputs_count: u32,
    fee: String,
    timestamp: u32,
}

#[derive(Row, Deserialize)]
struct TimestampRow {
    timestamp: u32,
}

fn timestamp_to_rfc3339(ts: u32) -> String {
    Utc.timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

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
    clickhouse_client: ClickHouseClient,
    ws_manager: Arc<WsManager>,
    ckb_rpc_url: String,
) {
    let mut last_block_number: Option<i64> = None;
    let mut ticker = interval(Duration::from_secs(2));

    loop {
        ticker.tick().await;

        let tip_block = fetch_tip_block(&ckb_rpc_url).await.unwrap_or(0) as i64;

        let query = format!(
            "SELECT number, {}, timestamp, transactions_count, epoch_number, epoch_index, epoch_length 
             FROM blocks ORDER BY number DESC LIMIT 1",
            hex_hash("hash")
        );

        let latest_result = match clickhouse_client
            .client()
            .query(&query)
            .fetch_optional::<BlockRow>()
            .await
        {
            Ok(Some(row)) => {
                let hash = hex::decode(row.hash.strip_prefix("0x").unwrap_or(&row.hash))
                    .unwrap_or_default();
                Ok(Some((
                    row.number as i64,
                    hash,
                    row.timestamp,
                    row.transactions_count as i32,
                    row.epoch_number as i64,
                    row.epoch_index as i32,
                    row.epoch_length as i32,
                )))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                error!("Failed to query latest block from ClickHouse: {}", e);
                Err(())
            }
        };

        let latest_block = match latest_result {
            Ok(Some(block)) => block,
            Ok(None) => continue,
            Err(_) => {
                error!("Failed to query latest block");
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
            let sync_status = build_sync_status(&clickhouse_client, tip_block).await;
            let (avg_block_time, estimated_epoch_time) =
                calculate_epoch_stats(&clickhouse_client, number, epoch_index, epoch_length).await;

            let msg = BroadcastMessage::NewBlock {
                number,
                hash: format!("0x{}", hex::encode(&hash)),
                timestamp: timestamp_to_rfc3339(timestamp),
                transactions_count: tx_count,
                epoch_number,
                epoch_index,
                epoch_length,
                avg_block_time,
                estimated_epoch_time,
                sync_status,
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

            let query = format!(
                "SELECT number, {}, timestamp, transactions_count, epoch_number, epoch_index, epoch_length 
                 FROM blocks WHERE number > {} ORDER BY number ASC LIMIT 20",
                hex_hash("hash"),
                last
            );

            let new_blocks_result = match clickhouse_client
                .client()
                .query(&query)
                .fetch_all::<BlockRow>()
                .await
            {
                Ok(rows) => {
                    #[allow(clippy::type_complexity)]
                    let blocks: Vec<(i64, Vec<u8>, u32, i32, i64, i32, i32)> = rows
                        .into_iter()
                        .map(|row| {
                            let hash =
                                hex::decode(row.hash.strip_prefix("0x").unwrap_or(&row.hash))
                                    .unwrap_or_default();
                            (
                                row.number as i64,
                                hash,
                                row.timestamp,
                                row.transactions_count as i32,
                                row.epoch_number as i64,
                                row.epoch_index as i32,
                                row.epoch_length as i32,
                            )
                        })
                        .collect();
                    Ok(blocks)
                }
                Err(e) => {
                    error!("Failed to query new blocks from ClickHouse: {}", e);
                    Err(())
                }
            };

            match new_blocks_result {
                Ok(blocks) => {
                    for (num, h, ts, txc, ep_num, ep_idx, ep_len) in blocks {
                        let sync_status = build_sync_status(&clickhouse_client, tip_block).await;
                        let (avg_block_time, estimated_epoch_time) =
                            calculate_epoch_stats(&clickhouse_client, num, ep_idx, ep_len).await;

                        let msg = BroadcastMessage::NewBlock {
                            number: num,
                            hash: format!("0x{}", hex::encode(&h)),
                            timestamp: timestamp_to_rfc3339(ts),
                            transactions_count: txc,
                            epoch_number: ep_num,
                            epoch_index: ep_idx,
                            epoch_length: ep_len,
                            avg_block_time,
                            estimated_epoch_time,
                            sync_status,
                        };
                        info!("Broadcasting new block: {}", num);
                        ws_manager.broadcast_block(msg);

                        broadcast_block_transactions(&clickhouse_client, &ws_manager, num).await;
                        last_block_number = Some(num);
                    }
                }
                Err(_) => {
                    error!("Failed to query new blocks");
                }
            }
        }
    }
}

async fn broadcast_block_transactions(
    clickhouse_client: &ClickHouseClient,
    ws_manager: &Arc<WsManager>,
    block_number: i64,
) {
    let query = format!(
        "SELECT {}, inputs_count, outputs_count, fee, timestamp
         FROM transactions 
         WHERE block_number = {} AND is_cellbase = 0
         ORDER BY tx_index",
        hex_hash("hash"),
        block_number
    );

    let txs_result = match clickhouse_client
        .client()
        .query(&query)
        .fetch_all::<TransactionRow>()
        .await
    {
        Ok(rows) => {
            #[allow(clippy::type_complexity)]
            let transactions: Vec<(Vec<u8>, i32, i32, String, u32)> = rows
                .into_iter()
                .map(|row| {
                    let hash = hex::decode(row.hash.strip_prefix("0x").unwrap_or(&row.hash))
                        .unwrap_or_default();
                    (
                        hash,
                        row.inputs_count as i32,
                        row.outputs_count as i32,
                        row.fee,
                        row.timestamp,
                    )
                })
                .collect();
            Ok(transactions)
        }
        Err(e) => {
            error!("Failed to query block transactions from ClickHouse: {}", e);
            Err(())
        }
    };

    match txs_result {
        Ok(transactions) => {
            for (hash, inputs_count, outputs_count, fee, timestamp) in transactions {
                let msg = BroadcastMessage::NewTransaction {
                    hash: format!("0x{}", hex::encode(&hash)),
                    block_number,
                    inputs_count,
                    outputs_count,
                    fee,
                    timestamp: timestamp_to_rfc3339(timestamp),
                };
                debug!("Broadcasting new transaction: {}", hex::encode(&hash));
                ws_manager.broadcast_transaction(msg);
            }
        }
        Err(_) => {
            error!("Failed to query block transactions");
        }
    }
}

async fn calculate_epoch_stats(
    clickhouse_client: &ClickHouseClient,
    latest_block: i64,
    epoch_index: i32,
    epoch_length: i32,
) -> (String, String) {
    let query = format!(
        "SELECT timestamp FROM blocks
         WHERE number >= {} AND number <= {}
         ORDER BY number ASC",
        latest_block - 1,
        latest_block
    );

    let blocks_result = match clickhouse_client
        .client()
        .query(&query)
        .fetch_all::<TimestampRow>()
        .await
    {
        Ok(rows) => Ok(rows.into_iter().map(|r| r.timestamp).collect()),
        Err(e) => {
            error!(
                "Failed to query blocks for epoch stats from ClickHouse: {}",
                e
            );
            Err(())
        }
    };

    let avg_time = blocks_result
        .ok()
        .and_then(|blocks: Vec<u32>| {
            if blocks.len() == 2 {
                let duration = (blocks[1] as i64 - blocks[0] as i64) as f64;
                Some(duration.abs().max(1.0))
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

async fn build_sync_status(clickhouse_client: &ClickHouseClient, tip_block: i64) -> SyncStatus {
    let query = "SELECT tip_block_number, sync_started_at, COALESCE(sync_started_block, 0) FROM sync_status WHERE id = 1";

    #[derive(Row, Deserialize)]
    struct SyncStatusRow {
        tip_block_number: u64,
        sync_started_at: Option<u32>,
        #[serde(rename = "COALESCE(sync_started_block, 0)")]
        sync_started_block: u64,
    }

    let sync_row: Option<(i64, Option<u32>, i64)> = clickhouse_client
        .client()
        .query(query)
        .fetch_optional::<SyncStatusRow>()
        .await
        .ok()
        .flatten()
        .map(|row| {
            (
                row.tip_block_number as i64,
                row.sync_started_at,
                row.sync_started_block as i64,
            )
        });

    let (synced_block, sync_started_at, sync_started_block) = sync_row.unwrap_or((0, None, 0));

    let blocks_behind = tip_block - synced_block;
    let is_syncing = blocks_behind > 100;
    let progress = if tip_block > 0 {
        (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    let estimated_time = if is_syncing && blocks_behind > 0 {
        if let Some(started_at) = sync_started_at {
            let started_at_dt = Utc.timestamp_opt(started_at as i64, 0).single();
            if let Some(started_at_dt) = started_at_dt {
                let elapsed = Utc::now().signed_duration_since(started_at_dt).num_seconds() as u64;
                let blocks_synced = (synced_block - sync_started_block).max(0) as u64;
                if elapsed > 0 && blocks_synced > 0 {
                    let rate = blocks_synced as f64 / elapsed as f64;
                    let seconds_remaining = (blocks_behind as f64 / rate) as u64;
                    Some(format_duration(seconds_remaining))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    SyncStatus {
        is_syncing,
        synced_block,
        tip_block,
        progress,
        estimated_time,
        chart_data_may_be_incomplete: blocks_behind > 1000,
    }
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
    fn test_timestamp_to_rfc3339() {
        assert_eq!(timestamp_to_rfc3339(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(timestamp_to_rfc3339(1573969227), "2019-11-17T05:20:27+00:00");
    }
}
