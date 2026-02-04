use anyhow::{anyhow, Result};
use ckbadger_common::{
    RateCalculator, SecondaryIssuanceBackfillConfig, SecondaryIssuanceBackfillResult,
};
use futures::{
    stream::{self, StreamExt},
    SinkExt,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::Duration;
use tokio_postgres::NoTls;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::TaskDb;

const RETRY_ATTEMPTS: usize = 3;
const RETRY_BACKOFF_MS: u64 = 500;
const RPC_BATCH_SIZE: usize = 250;
const HTTP_TIMEOUT_SECS: u64 = 60;
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BlockEconomicState {
    issuance: BlockIssuance,
    miner_reward: MinerReward,
}

#[derive(Debug, Deserialize)]
struct BlockIssuance {
    secondary: String,
}

#[derive(Debug, Deserialize)]
struct MinerReward {
    secondary: String,
}

#[derive(Debug, Clone)]
struct BlockRow {
    number: i64,
    hash: Vec<u8>,
    dao: Vec<u8>,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct BlockIssuanceRow {
    number: i64,
    timestamp: chrono::DateTime<chrono::Utc>,
    secondary_issuance: i64,
    miner_secondary: i64,
    dao_compensation: i64,
    burnt: i64,
}

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    database_url: &str,
    task_id: Uuid,
    config: &SecondaryIssuanceBackfillConfig,
) -> Result<()> {
    info!(
        "Starting secondary issuance backfill: rpc={}, batch_size={}, rpc_batch_size={}, concurrent_requests={}",
        config.ckb_rpc_url, config.batch_size, RPC_BATCH_SIZE, config.concurrent_requests
    );

    let max_block: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
        .fetch_one(pool)
        .await?;

    // Genesis block (0) has no economic state - CKB RPC returns null for it
    let start_block = config.start_block.unwrap_or(1).max(1);
    let end_block = config.end_block.unwrap_or(max_block).min(max_block);

    if start_block > end_block {
        let result = SecondaryIssuanceBackfillResult {
            blocks_processed: 0,
            blocks_total: 0,
            errors: vec![],
        };
        db.complete_task(task_id, Some(serde_json::to_value(&result)?))
            .await?;
        return Ok(());
    }

    let total_blocks = end_block - start_block + 1;
    db.update_progress(
        task_id,
        0,
        total_blocks,
        Some("Resetting secondary issuance state"),
        None,
    )
    .await?;

    reset_secondary_issuance_state(pool).await?;

    let client = Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .pool_max_idle_per_host(config.concurrent_requests)
        .build()
        .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

    db.update_progress(
        task_id,
        0,
        total_blocks,
        Some("Pre-loading DAO deposit events"),
        None,
    )
    .await?;

    let dao_events = preload_all_dao_events(pool).await?;
    let mut active_deposit_cache: HashMap<i64, u128> = HashMap::new();

    let mut rate_calc = RateCalculator::default();
    let mut result = SecondaryIssuanceBackfillResult {
        blocks_total: total_blocks,
        ..Default::default()
    };
    let mut processed: i64 = 0;
    let mut current = start_block;
    let batch_size = config.batch_size.max(1);

    while current <= end_block {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled, stopping");
            return Ok(());
        }

        let batch_end = (current + batch_size - 1).min(end_block);
        let blocks = fetch_block_rows(pool, current, batch_end).await?;
        if blocks.is_empty() {
            current = batch_end + 1;
            continue;
        }

        let dao_deposits =
            compute_batch_dao_deposits(current, batch_end, &dao_events, &mut active_deposit_cache);

        let block_hashes: Vec<String> = blocks
            .iter()
            .map(|b| format!("0x{}", hex::encode(&b.hash)))
            .collect();
        let economic_states = fetch_block_economic_states_batch(
            &client,
            &config.ckb_rpc_url,
            &block_hashes,
            config.concurrent_requests,
        )
        .await?;

        let mut batch_rows: Vec<BlockIssuanceRow> = Vec::with_capacity(blocks.len());
        for block in &blocks {
            let block_hash = format!("0x{}", hex::encode(&block.hash));
            let economic_state = economic_states.get(&block_hash).ok_or_else(|| {
                anyhow!(
                    "Missing economic state for block {} ({})",
                    block.number,
                    block_hash
                )
            })?;

            let deposits = *dao_deposits.get(&block.number).unwrap_or(&0);
            let row = process_block_with_state(block, economic_state, deposits)?;
            batch_rows.push(row);
        }

        insert_secondary_issuance_rows_copy(database_url, &batch_rows).await?;
        update_secondary_issuance_totals(pool, &batch_rows, batch_end).await?;

        processed += batch_rows.len() as i64;
        result.blocks_processed = processed;
        rate_calc.add_sample(processed);

        let msg = format!(
            "Processed blocks {}-{} ({:.1}%)",
            current,
            batch_end,
            (processed as f64 / total_blocks as f64) * 100.0
        );

        db.update_progress(
            task_id,
            processed,
            total_blocks,
            Some(&msg),
            rate_calc.rate(),
        )
        .await?;

        db.update_result(task_id, &serde_json::to_value(&result)?)
            .await?;

        current = batch_end + 1;
    }

    info!(
        "Secondary issuance backfill completed: {} blocks processed",
        result.blocks_processed
    );

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    Ok(())
}

async fn reset_secondary_issuance_state(pool: &PgPool) -> Result<()> {
    sqlx::query("TRUNCATE TABLE block_secondary_issuance")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        UPDATE dao_statistics SET
            cumulative_secondary_issuance = '0',
            cumulative_miner_secondary = '0',
            cumulative_dao_compensation = '0',
            cumulative_burnt = '0',
            mining_reward = '0',
            deposit_compensation = '0',
            burnt = '0',
            last_processed_block = 0,
            updated_at = NOW()
        WHERE id = 1
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn fetch_block_rows(pool: &PgPool, start: i64, end: i64) -> Result<Vec<BlockRow>> {
    let rows = sqlx::query_as::<_, (i64, Vec<u8>, Vec<u8>, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT number, hash, dao, timestamp
        FROM blocks
        WHERE number >= $1 AND number <= $2
        ORDER BY number
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(number, hash, dao, timestamp)| BlockRow {
            number,
            hash,
            dao,
            timestamp,
        })
        .collect())
}

struct DaoDepositEvents {
    deposits_by_block: HashMap<i64, u128>,
    withdrawals_by_block: HashMap<i64, u128>,
}

async fn preload_all_dao_events(pool: &PgPool) -> Result<DaoDepositEvents> {
    info!("Pre-loading all DAO deposit events...");

    let deposit_rows = sqlx::query_as::<_, (i64, String)>(
        r#"
        SELECT deposit_block_number, COALESCE(SUM(capacity::numeric), 0)::text
        FROM dao_deposits
        GROUP BY deposit_block_number
        ORDER BY deposit_block_number
        "#,
    )
    .fetch_all(pool)
    .await?;

    let withdraw_rows = sqlx::query_as::<_, (i64, String)>(
        r#"
        SELECT withdraw_block, COALESCE(SUM(capacity::numeric), 0)::text
        FROM dao_deposits
        WHERE withdraw_block IS NOT NULL
        GROUP BY withdraw_block
        ORDER BY withdraw_block
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut deposits_by_block = HashMap::new();
    for (block_number, total) in deposit_rows {
        deposits_by_block.insert(block_number, parse_numeric_u128(&total)?);
    }

    let mut withdrawals_by_block = HashMap::new();
    for (block_number, total) in withdraw_rows {
        withdrawals_by_block.insert(block_number, parse_numeric_u128(&total)?);
    }

    info!(
        "Loaded {} deposit events and {} withdrawal events",
        deposits_by_block.len(),
        withdrawals_by_block.len()
    );

    Ok(DaoDepositEvents {
        deposits_by_block,
        withdrawals_by_block,
    })
}

fn compute_active_dao_deposit_at_block(
    block_number: i64,
    events: &DaoDepositEvents,
    cache: &mut HashMap<i64, u128>,
) -> u128 {
    if let Some(&cached) = cache.get(&block_number) {
        return cached;
    }

    let mut active: u128 = 0;
    for block in 1..=block_number {
        if let Some(&deposit) = events.deposits_by_block.get(&block) {
            active = active.saturating_add(deposit);
        }
        if let Some(&withdrawal) = events.withdrawals_by_block.get(&block) {
            active = active.saturating_sub(withdrawal);
        }
    }

    cache.insert(block_number, active);
    active
}

fn compute_batch_dao_deposits(
    start: i64,
    end: i64,
    events: &DaoDepositEvents,
    active_deposit_cache: &mut HashMap<i64, u128>,
) -> HashMap<i64, u128> {
    let initial_active = if start > 1 {
        compute_active_dao_deposit_at_block(start - 1, events, active_deposit_cache)
    } else {
        0
    };

    let mut active = initial_active;
    let mut result = HashMap::new();

    for block in start..=end {
        result.insert(block, active);

        if let Some(&withdrawal) = events.withdrawals_by_block.get(&block) {
            active = active.saturating_sub(withdrawal);
        }
        if let Some(&deposit) = events.deposits_by_block.get(&block) {
            active = active.saturating_add(deposit);
        }
    }

    if end > 0 {
        active_deposit_cache.insert(end, result.get(&end).copied().unwrap_or(0));
    }

    result
}

async fn fetch_block_economic_states_batch(
    client: &Client,
    rpc_url: &str,
    block_hashes: &[String],
    concurrent_requests: usize,
) -> Result<HashMap<String, BlockEconomicState>> {
    let mut results: HashMap<String, BlockEconomicState> = HashMap::new();

    let rpc_url = rpc_url.to_string();
    let chunks: Vec<Vec<String>> = block_hashes
        .chunks(RPC_BATCH_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect();

    let concurrency = concurrent_requests.max(1);
    let mut stream = stream::iter(chunks.into_iter().map(|chunk| {
        let client = client.clone();
        let rpc_url = rpc_url.clone();
        async move { fetch_rpc_batch_with_retry(&client, &rpc_url, &chunk).await }
    }))
    .buffer_unordered(concurrency);

    while let Some(chunk_result) = stream.next().await {
        let chunk_results = chunk_result?;
        results.extend(chunk_results);
    }

    Ok(results)
}

async fn fetch_rpc_batch_with_retry(
    client: &Client,
    rpc_url: &str,
    block_hashes: &[String],
) -> Result<HashMap<String, BlockEconomicState>> {
    for attempt in 1..=RETRY_ATTEMPTS {
        match fetch_rpc_batch(client, rpc_url, block_hashes).await {
            Ok(results) => return Ok(results),
            Err(err) => {
                warn!(
                    "RPC batch request failed (attempt {}/{}): {}",
                    attempt, RETRY_ATTEMPTS, err
                );
                if attempt < RETRY_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(RETRY_BACKOFF_MS * attempt as u64))
                        .await;
                }
            }
        }
    }

    Err(anyhow!(
        "Failed to fetch economic states batch after {} attempts",
        RETRY_ATTEMPTS
    ))
}

async fn fetch_rpc_batch(
    client: &Client,
    rpc_url: &str,
    block_hashes: &[String],
) -> Result<HashMap<String, BlockEconomicState>> {
    let requests: Vec<RpcRequest> = block_hashes
        .iter()
        .enumerate()
        .map(|(i, hash)| RpcRequest {
            jsonrpc: "2.0",
            id: i as u32,
            method: "get_block_economic_state",
            params: vec![hash.clone()],
        })
        .collect();

    let response = client.post(rpc_url).json(&requests).send().await?;
    let responses: Vec<Value> = response.json().await?;

    let mut results: HashMap<String, BlockEconomicState> = HashMap::new();

    for (i, resp_value) in responses.into_iter().enumerate() {
        let block_hash = &block_hashes[i];

        if let Some(error) = resp_value.get("error") {
            if !error.is_null() {
                let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                let message = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                warn!(
                    "RPC error for block {}: {} (code: {})",
                    block_hash, message, code
                );
                continue;
            }
        }

        if let Some(result) = resp_value.get("result") {
            if result.is_null() {
                warn!("RPC returned null for block {}", block_hash);
                continue;
            }
            match serde_json::from_value::<BlockEconomicState>(result.clone()) {
                Ok(state) => {
                    results.insert(block_hash.clone(), state);
                }
                Err(e) => {
                    warn!("Failed to parse economic state for {}: {}", block_hash, e);
                }
            }
        }
    }

    Ok(results)
}

fn process_block_with_state(
    block: &BlockRow,
    economic_state: &BlockEconomicState,
    dao_deposits: u128,
) -> Result<BlockIssuanceRow> {
    let (total_issuance, occupied) = parse_dao_field(&block.dao)
        .ok_or_else(|| anyhow!("Invalid DAO field at block {}", block.number))?;

    let secondary_issuance = parse_hex_u128(&economic_state.issuance.secondary)?;
    let miner_secondary = parse_hex_u128(&economic_state.miner_reward.secondary)?;
    let non_miner_secondary = secondary_issuance.saturating_sub(miner_secondary);

    let total_issuance = total_issuance as u128;
    let occupied = occupied as u128;
    let denominator = total_issuance.saturating_sub(occupied);

    let (dao_compensation, burnt) = if denominator > 0 {
        let dao_share = (non_miner_secondary * dao_deposits) / denominator;
        let burnt_share = non_miner_secondary.saturating_sub(dao_share);
        (dao_share, burnt_share)
    } else {
        (0, non_miner_secondary)
    };

    Ok(BlockIssuanceRow {
        number: block.number,
        timestamp: block.timestamp,
        secondary_issuance: u128_to_i64(secondary_issuance)?,
        miner_secondary: u128_to_i64(miner_secondary)?,
        dao_compensation: u128_to_i64(dao_compensation)?,
        burnt: u128_to_i64(burnt)?,
    })
}

fn parse_dao_field(dao: &[u8]) -> Option<(u64, u64)> {
    if dao.len() < 32 {
        return None;
    }
    let total_issuance = u64::from_le_bytes(dao[0..8].try_into().ok()?);
    let occupied_capacity = u64::from_le_bytes(dao[24..32].try_into().ok()?);
    Some((total_issuance, occupied_capacity))
}

fn parse_hex_u128(value: &str) -> Result<u128> {
    let hex = value.strip_prefix("0x").unwrap_or(value);
    u128::from_str_radix(hex, 16).map_err(|e| anyhow!("Invalid hex value {}: {}", value, e))
}

fn parse_numeric_u128(value: &str) -> Result<u128> {
    value
        .parse::<u128>()
        .map_err(|e| anyhow!("Invalid numeric value {}: {}", value, e))
}

fn u128_to_i64(value: u128) -> Result<i64> {
    i64::try_from(value).map_err(|_| anyhow!("Value too large for i64: {}", value))
}

async fn insert_secondary_issuance_rows_copy(
    database_url: &str,
    rows: &[BlockIssuanceRow],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let (client, connection) = tokio_postgres::connect(database_url, NoTls)
        .await
        .map_err(|e| anyhow!("COPY connection failed: {}", e))?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            warn!("COPY connection error: {}", e);
        }
    });

    let copy_stmt = r#"COPY block_secondary_issuance 
        (block_number, block_timestamp, secondary_issuance, miner_secondary, dao_compensation, burnt) 
        FROM STDIN WITH (FORMAT text, DELIMITER E'\t')"#;

    let sink = client
        .copy_in(copy_stmt)
        .await
        .map_err(|e| anyhow!("COPY IN failed: {}", e))?;
    futures::pin_mut!(sink);

    let mut text_data = String::with_capacity(rows.len() * 100);
    for row in rows {
        use std::fmt::Write;
        writeln!(
            &mut text_data,
            "{}\t{}\t{}\t{}\t{}\t{}",
            row.number,
            row.timestamp.format("%Y-%m-%d %H:%M:%S%.6f%:z"),
            row.secondary_issuance,
            row.miner_secondary,
            row.dao_compensation,
            row.burnt
        )
        .expect("String write should not fail");
    }

    sink.send(bytes::Bytes::from(text_data))
        .await
        .map_err(|e| anyhow!("COPY send failed: {}", e))?;
    let rows_written = sink
        .finish()
        .await
        .map_err(|e| anyhow!("COPY finish failed: {}", e))?;
    debug!("COPY wrote {} rows", rows_written);

    Ok(())
}

async fn update_secondary_issuance_totals(
    pool: &PgPool,
    rows: &[BlockIssuanceRow],
    last_block: i64,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let total_secondary: i64 = rows.iter().map(|r| r.secondary_issuance).sum();
    let total_miner: i64 = rows.iter().map(|r| r.miner_secondary).sum();
    let total_dao: i64 = rows.iter().map(|r| r.dao_compensation).sum();
    let total_burnt: i64 = rows.iter().map(|r| r.burnt).sum();

    debug!(
        "Updating secondary issuance totals: secondary={}, miner={}, dao={}, burnt={}, last_block={}",
        total_secondary, total_miner, total_dao, total_burnt, last_block
    );

    sqlx::query(
        r#"
        UPDATE dao_statistics SET
            cumulative_secondary_issuance = (COALESCE(cumulative_secondary_issuance, '0')::numeric + $1)::text,
            cumulative_miner_secondary = (COALESCE(cumulative_miner_secondary, '0')::numeric + $2)::text,
            cumulative_dao_compensation = (COALESCE(cumulative_dao_compensation, '0')::numeric + $3)::text,
            cumulative_burnt = (COALESCE(cumulative_burnt, '0')::numeric + $4)::text,
            mining_reward = (COALESCE(cumulative_miner_secondary, '0')::numeric + $2)::text,
            deposit_compensation = (COALESCE(cumulative_dao_compensation, '0')::numeric + $3)::text,
            burnt = (COALESCE(cumulative_burnt, '0')::numeric + $4)::text,
            last_processed_block = GREATEST(last_processed_block, $5),
            updated_at = NOW()
        WHERE id = 1
        "#,
    )
    .bind(total_secondary)
    .bind(total_miner)
    .bind(total_dao)
    .bind(total_burnt)
    .bind(last_block)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dao_field_extracts_values() {
        let mut dao = vec![0u8; 32];
        dao[0..8].copy_from_slice(&123u64.to_le_bytes());
        dao[24..32].copy_from_slice(&456u64.to_le_bytes());

        let parsed = parse_dao_field(&dao).unwrap();
        assert_eq!(parsed.0, 123);
        assert_eq!(parsed.1, 456);
    }

    #[test]
    fn test_parse_hex_u128_handles_prefix() {
        let value = parse_hex_u128("0x10").unwrap();
        assert_eq!(value, 16);
    }

    #[test]
    fn test_u128_to_i64_range_check() {
        let max = u128::from(i64::MAX as u64);
        assert!(u128_to_i64(max).is_ok());

        let too_large = max + 1;
        assert!(u128_to_i64(too_large).is_err());
    }

    #[test]
    fn test_rpc_batch_size_constant() {
        assert_eq!(RPC_BATCH_SIZE, 250);
    }

    #[test]
    fn test_process_block_with_state_calculates_burnt_correctly() {
        let block = BlockRow {
            number: 100,
            hash: vec![0u8; 32],
            dao: {
                let mut dao = vec![0u8; 32];
                let total_issuance: u64 = 1_000_000_000_000;
                let occupied: u64 = 100_000_000_000;
                dao[0..8].copy_from_slice(&total_issuance.to_le_bytes());
                dao[24..32].copy_from_slice(&occupied.to_le_bytes());
                dao
            },
            timestamp: chrono::Utc::now(),
        };

        let economic_state = BlockEconomicState {
            issuance: BlockIssuance {
                secondary: "0x5f5e100".to_string(), // 100_000_000
            },
            miner_reward: MinerReward {
                secondary: "0x2faf080".to_string(), // 50_000_000
            },
        };

        let dao_deposits: u128 = 200_000_000_000;

        let row = process_block_with_state(&block, &economic_state, dao_deposits).unwrap();

        assert_eq!(row.number, 100);
        assert_eq!(row.secondary_issuance, 100_000_000);
        assert_eq!(row.miner_secondary, 50_000_000);

        let non_miner = 100_000_000i64 - 50_000_000;
        let denominator = 1_000_000_000_000u128 - 100_000_000_000;
        let expected_dao_share = (non_miner as u128 * dao_deposits) / denominator;
        let expected_burnt = non_miner as u128 - expected_dao_share;

        assert_eq!(row.dao_compensation, expected_dao_share as i64);
        assert_eq!(row.burnt, expected_burnt as i64);
    }

    #[test]
    fn test_process_block_with_state_handles_zero_denominator() {
        let block = BlockRow {
            number: 1,
            hash: vec![0u8; 32],
            dao: {
                let mut dao = vec![0u8; 32];
                let total_issuance: u64 = 100;
                let occupied: u64 = 100;
                dao[0..8].copy_from_slice(&total_issuance.to_le_bytes());
                dao[24..32].copy_from_slice(&occupied.to_le_bytes());
                dao
            },
            timestamp: chrono::Utc::now(),
        };

        let economic_state = BlockEconomicState {
            issuance: BlockIssuance {
                secondary: "0x64".to_string(), // 100
            },
            miner_reward: MinerReward {
                secondary: "0x32".to_string(), // 50
            },
        };

        let row = process_block_with_state(&block, &economic_state, 1000).unwrap();

        assert_eq!(row.dao_compensation, 0);
        assert_eq!(row.burnt, 50);
    }

    #[test]
    fn test_compute_batch_dao_deposits_applies_withdraws_then_deposits() {
        let mut deposits_by_block = HashMap::new();
        deposits_by_block.insert(10i64, 5u128);
        deposits_by_block.insert(11i64, 10u128);

        let mut withdrawals_by_block = HashMap::new();
        withdrawals_by_block.insert(10i64, 20u128);

        let events = DaoDepositEvents {
            deposits_by_block,
            withdrawals_by_block,
        };

        let mut cache: HashMap<i64, u128> = HashMap::new();
        cache.insert(9, 100u128);

        let series = compute_batch_dao_deposits(10, 12, &events, &mut cache);

        assert_eq!(series.get(&10), Some(&100));
        assert_eq!(series.get(&11), Some(&85));
        assert_eq!(series.get(&12), Some(&95));
    }

    #[test]
    fn test_rpc_request_serialization() {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "get_block_economic_state",
            params: vec!["0xabc123".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"method\":\"get_block_economic_state\""));
        assert!(json.contains("\"params\":[\"0xabc123\"]"));
    }

    #[test]
    fn test_block_economic_state_deserialization() {
        let json = r#"{
            "issuance": {"primary": "0x0", "secondary": "0x5f5e100"},
            "miner_reward": {"primary": "0x0", "secondary": "0x2faf080", "committed": "0x0", "proposal": "0x0"}
        }"#;

        let state: BlockEconomicState = serde_json::from_str(json).unwrap();
        assert_eq!(state.issuance.secondary, "0x5f5e100");
        assert_eq!(state.miner_reward.secondary, "0x2faf080");
    }

    #[test]
    fn test_compute_active_dao_deposit_at_block_with_cache() {
        let mut deposits = HashMap::new();
        deposits.insert(5i64, 100u128);
        deposits.insert(10i64, 50u128);

        let mut withdrawals = HashMap::new();
        withdrawals.insert(8i64, 30u128);

        let events = DaoDepositEvents {
            deposits_by_block: deposits,
            withdrawals_by_block: withdrawals,
        };

        let mut cache: HashMap<i64, u128> = HashMap::new();

        let at_block_10 = compute_active_dao_deposit_at_block(10, &events, &mut cache);
        assert_eq!(at_block_10, 120);

        assert!(cache.contains_key(&10));

        let at_block_10_cached = compute_active_dao_deposit_at_block(10, &events, &mut cache);
        assert_eq!(at_block_10_cached, 120);
    }

    #[test]
    fn test_compute_batch_dao_deposits_empty_events() {
        let events = DaoDepositEvents {
            deposits_by_block: HashMap::new(),
            withdrawals_by_block: HashMap::new(),
        };

        let mut cache: HashMap<i64, u128> = HashMap::new();
        let series = compute_batch_dao_deposits(1, 5, &events, &mut cache);

        for block in 1..=5 {
            assert_eq!(series.get(&block), Some(&0));
        }
    }

    #[test]
    fn test_http_timeout_constants() {
        assert_eq!(HTTP_TIMEOUT_SECS, 60);
        assert_eq!(HTTP_CONNECT_TIMEOUT_SECS, 10);
    }
}
