use anyhow::{anyhow, Result};
use ckbadger_common::{
    RateCalculator, SecondaryIssuanceBackfillConfig, SecondaryIssuanceBackfillResult,
};
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::TaskDb;

const RETRY_ATTEMPTS: usize = 3;
const RETRY_BACKOFF_MS: u64 = 500;

#[derive(Debug, Serialize)]
struct RpcRequest<T> {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: T,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
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
    task_id: Uuid,
    config: &SecondaryIssuanceBackfillConfig,
) -> Result<()> {
    info!(
        "Starting secondary issuance backfill: rpc={}, batch_size={}, concurrent={}",
        config.ckb_rpc_url, config.batch_size, config.concurrent_requests
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

    let client = Client::new();
    let mut rate_calc = RateCalculator::default();
    let mut result = SecondaryIssuanceBackfillResult {
        blocks_total: total_blocks,
        ..Default::default()
    };
    let mut processed: i64 = 0;
    let mut current = start_block;
    let concurrent = config.concurrent_requests.max(1);
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

        let block_numbers: Vec<i64> = blocks.iter().map(|b| b.number).collect();
        let dao_deposits = fetch_dao_deposits(pool, &block_numbers).await?;

        let mut futures = FuturesUnordered::new();
        let mut batch_rows: Vec<BlockIssuanceRow> = Vec::with_capacity(blocks.len());

        for block in blocks {
            let client = client.clone();
            let rpc_url = config.ckb_rpc_url.clone();
            let deposits = *dao_deposits.get(&block.number).unwrap_or(&0);

            futures.push(async move {
                let row = process_block(&client, &rpc_url, block, deposits).await?;
                Ok::<BlockIssuanceRow, anyhow::Error>(row)
            });

            if futures.len() >= concurrent {
                if let Some(result) = futures.next().await {
                    batch_rows.push(result?);
                }
            }
        }

        while let Some(result) = futures.next().await {
            batch_rows.push(result?);
        }

        insert_secondary_issuance_rows(pool, &batch_rows).await?;
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
        tokio::time::sleep(Duration::from_millis(50)).await;
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

async fn fetch_dao_deposits(pool: &PgPool, block_numbers: &[i64]) -> Result<HashMap<i64, u128>> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        r#"
        SELECT b.block_number, COALESCE(SUM(d.capacity::numeric), 0)::text
        FROM UNNEST($1::bigint[]) AS b(block_number)
        LEFT JOIN dao_deposits d
          ON d.deposit_block_number < b.block_number
         AND (d.withdraw_block IS NULL OR d.withdraw_block >= b.block_number)
        GROUP BY b.block_number
        "#,
    )
    .bind(block_numbers)
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::new();
    for (block_number, total) in rows {
        map.insert(block_number, total.parse().unwrap_or(0));
    }

    Ok(map)
}

async fn process_block(
    client: &Client,
    rpc_url: &str,
    block: BlockRow,
    dao_deposits: u128,
) -> Result<BlockIssuanceRow> {
    let block_hash = format!("0x{}", hex::encode(&block.hash));
    let economic_state = fetch_block_economic_state(client, rpc_url, &block_hash).await?;
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

async fn fetch_block_economic_state(
    client: &Client,
    rpc_url: &str,
    block_hash: &str,
) -> Result<BlockEconomicState> {
    for attempt in 1..=RETRY_ATTEMPTS {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "get_block_economic_state",
            params: vec![block_hash.to_string()],
        };

        let response = client.post(rpc_url).json(&request).send().await;
        match response {
            Ok(resp) => {
                let parsed: RpcResponse<BlockEconomicState> = resp.json().await?;
                if let Some(error) = parsed.error {
                    warn!(
                        "RPC error for block {}: {} ({})",
                        block_hash, error.message, error.code
                    );
                } else if let Some(result) = parsed.result {
                    return Ok(result);
                }
            }
            Err(err) => {
                warn!("RPC request failed for block {}: {}", block_hash, err);
            }
        }

        if attempt < RETRY_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(RETRY_BACKOFF_MS * attempt as u64)).await;
        }
    }

    Err(anyhow!(
        "Failed to fetch economic state for block {} after {} attempts",
        block_hash,
        RETRY_ATTEMPTS
    ))
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

fn u128_to_i64(value: u128) -> Result<i64> {
    i64::try_from(value).map_err(|_| anyhow!("Value too large for i64: {}", value))
}

async fn insert_secondary_issuance_rows(pool: &PgPool, rows: &[BlockIssuanceRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let numbers: Vec<i64> = rows.iter().map(|r| r.number).collect();
    let timestamps: Vec<chrono::DateTime<chrono::Utc>> = rows.iter().map(|r| r.timestamp).collect();
    let secondary: Vec<i64> = rows.iter().map(|r| r.secondary_issuance).collect();
    let miner: Vec<i64> = rows.iter().map(|r| r.miner_secondary).collect();
    let dao: Vec<i64> = rows.iter().map(|r| r.dao_compensation).collect();
    let burnt: Vec<i64> = rows.iter().map(|r| r.burnt).collect();

    sqlx::query(
        r#"
        INSERT INTO block_secondary_issuance (
            block_number, block_timestamp, secondary_issuance, miner_secondary, dao_compensation, burnt
        )
        SELECT * FROM UNNEST(
            $1::bigint[],
            $2::timestamptz[],
            $3::bigint[],
            $4::bigint[],
            $5::bigint[],
            $6::bigint[]
        )
        ON CONFLICT (block_number) DO NOTHING
        "#,
    )
    .bind(&numbers)
    .bind(&timestamps)
    .bind(&secondary)
    .bind(&miner)
    .bind(&dao)
    .bind(&burnt)
    .execute(pool)
    .await?;

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
}
