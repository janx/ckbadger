use anyhow::Result;
use ckbadger_common::{CyclesBackfillConfig, CyclesBackfillResult, RateCalculator};
use futures::stream::{FuturesUnordered, StreamExt};
use sqlx::PgPool;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &CyclesBackfillConfig,
) -> Result<()> {
    info!(
        "Starting cycles backfill: rpc={}, batch_size={}, concurrent={}",
        config.ckb_rpc_url, config.batch_size, config.concurrent_requests
    );

    let total_missing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM transactions_index WHERE NOT is_cellbase AND (cycles IS NULL OR cycles = 0)",
    )
    .fetch_one(pool)
    .await?;

    if total_missing.0 == 0 {
        info!("No missing cycles to fix");
        db.complete_task(
            task_id,
            Some(serde_json::to_value(CyclesBackfillResult::default())?),
        )
        .await?;
        return Ok(());
    }

    info!("Found {} transactions with missing cycles", total_missing.0);

    db.update_progress(task_id, 0, total_missing.0, Some("Starting..."), None)
        .await?;

    let mut result = CyclesBackfillResult::default();
    let mut rate_calc = RateCalculator::default();
    let mut processed: i64 = 0;

    loop {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled, stopping");
            return Ok(());
        }

        let txs: Vec<(Vec<u8>,)> = sqlx::query_as(
            r#"
            SELECT hash FROM transactions_index
            WHERE NOT is_cellbase
              AND (cycles IS NULL OR cycles = 0)
            ORDER BY block_number
            LIMIT $1
            "#,
        )
        .bind(config.batch_size)
        .fetch_all(pool)
        .await?;

        if txs.is_empty() {
            break;
        }

        let tx_hashes: Vec<String> = txs
            .iter()
            .map(|(h,)| format!("0x{}", hex::encode(h)))
            .collect();

        let batch_result = calculate_and_update_batch(
            pool,
            &config.ckb_rpc_url,
            &tx_hashes,
            config.concurrent_requests,
        )
        .await;

        result.transactions_processed += batch_result.0;
        result.cycles_updated += batch_result.1;
        if !batch_result.2.is_empty() {
            result.errors.extend(batch_result.2);
        }

        processed += batch_result.0;
        rate_calc.add_sample(processed);

        let msg = format!(
            "Processed {}/{} ({:.1}%)",
            processed,
            total_missing.0,
            (processed as f64 / total_missing.0 as f64) * 100.0
        );

        db.update_progress(
            task_id,
            processed,
            total_missing.0,
            Some(&msg),
            rate_calc.rate(),
        )
        .await?;

        db.update_result(task_id, &serde_json::to_value(&result)?)
            .await?;

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    info!(
        "Cycles backfill completed: {} processed, {} updated",
        result.transactions_processed, result.cycles_updated
    );

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    Ok(())
}

async fn calculate_and_update_batch(
    pool: &PgPool,
    rpc_url: &str,
    tx_hashes: &[String],
    concurrent: usize,
) -> (i64, i64, Vec<String>) {
    let mut futures = FuturesUnordered::new();
    let mut processed = 0i64;
    let mut updated = 0i64;
    let mut errors = Vec::new();

    for tx_hash in tx_hashes {
        let rpc_url = rpc_url.to_string();
        let hash = tx_hash.clone();
        futures.push(async move {
            let result = ckbadger_common::cycles::calculate_cycles(&rpc_url, &hash).await;
            (hash, result)
        });

        if futures.len() >= concurrent {
            if let Some((hash, result)) = futures.next().await {
                processed += 1;
                match update_cycles(pool, &hash, result).await {
                    Ok(true) => updated += 1,
                    Ok(false) => {}
                    Err(e) => errors.push(format!("{}: {}", hash, e)),
                }
            }
        }
    }

    while let Some((hash, result)) = futures.next().await {
        processed += 1;
        match update_cycles(pool, &hash, result).await {
            Ok(true) => updated += 1,
            Ok(false) => {}
            Err(e) => errors.push(format!("{}: {}", hash, e)),
        }
    }

    (processed, updated, errors)
}

async fn update_cycles(pool: &PgPool, tx_hash: &str, result: Result<i64, String>) -> Result<bool> {
    let hash_bytes = hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash))?;

    match result {
        Ok(cycles) => {
            sqlx::query("UPDATE transactions_index SET cycles = $1 WHERE hash = $2")
                .bind(cycles)
                .bind(&hash_bytes)
                .execute(pool)
                .await?;
            debug!("Updated cycles for {}: {}", tx_hash, cycles);
            Ok(true)
        }
        Err(e) => {
            warn!("Failed to calculate cycles for {}: {}", tx_hash, e);
            sqlx::query("UPDATE transactions_index SET cycles = -1 WHERE hash = $1")
                .bind(&hash_bytes)
                .execute(pool)
                .await?;
            Ok(false)
        }
    }
}
