use std::time::{Duration, Instant};

use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::control_plane::ControlPlaneClient;

const BATCH_SIZE: i64 = 50;
const CONCURRENT_CALCULATIONS: usize = 4;

pub struct CyclesFixTask {
    pool: PgPool,
    ckb_rpc_url: String,
}

impl CyclesFixTask {
    pub fn new(pool: PgPool, ckb_rpc_url: String) -> Self {
        Self { pool, ckb_rpc_url }
    }

    pub async fn run_all(
        &self,
        control_plane: &ControlPlaneClient,
        job_id: &Uuid,
    ) -> Result<()> {
        let (total,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transactions WHERE NOT is_cellbase AND (cycles IS NULL OR cycles = 0)",
        )
        .fetch_one(&self.pool)
        .await?;

        if total == 0 {
            info!("No missing cycles to fix");
            return Ok(());
        }

        info!("Starting to fix {} transactions with missing cycles", total);
        control_plane.update_job_progress(job_id, 0, Some(total), None).await;

        let start_time = Instant::now();
        let mut processed = 0i64;

        loop {
            if control_plane.is_job_cancelled(job_id).await {
                info!("Job cancelled, stopping cycles fix");
                return Ok(());
            }

            let txs: Vec<(Vec<u8>,)> = sqlx::query_as(
                r#"
                SELECT hash FROM transactions 
                WHERE NOT is_cellbase 
                  AND (cycles IS NULL OR cycles = 0)
                ORDER BY block_number
                LIMIT $1
                "#,
            )
            .bind(BATCH_SIZE)
            .fetch_all(&self.pool)
            .await?;

            if txs.is_empty() {
                break;
            }

            let tx_hashes: Vec<String> = txs
                .iter()
                .map(|(h,)| format!("0x{}", hex::encode(h)))
                .collect();

            let batch_processed = self.process_batch(&tx_hashes).await;
            processed += batch_processed as i64;

            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                processed as f64 / elapsed
            } else {
                0.0
            };

            control_plane
                .update_job_progress(job_id, processed, None, Some(speed))
                .await;

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!("Cycles fix completed: {} transactions processed", processed);
        Ok(())
    }

    pub async fn run_range(
        &self,
        start: i64,
        end: i64,
        control_plane: &ControlPlaneClient,
        job_id: &Uuid,
    ) -> Result<()> {
        let (total,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM transactions 
            WHERE block_number BETWEEN $1 AND $2 
              AND NOT is_cellbase 
              AND (cycles IS NULL OR cycles = 0)
            "#,
        )
        .bind(start)
        .bind(end)
        .fetch_one(&self.pool)
        .await?;

        if total == 0 {
            info!("No missing cycles in range {} to {}", start, end);
            return Ok(());
        }

        info!(
            "Starting to fix {} transactions with missing cycles in range {} to {}",
            total, start, end
        );
        control_plane.update_job_progress(job_id, 0, Some(total), None).await;

        let start_time = Instant::now();
        let mut processed = 0i64;

        loop {
            if control_plane.is_job_cancelled(job_id).await {
                info!("Job cancelled, stopping cycles fix");
                return Ok(());
            }

            let txs: Vec<(Vec<u8>,)> = sqlx::query_as(
                r#"
                SELECT hash FROM transactions 
                WHERE block_number BETWEEN $1 AND $2 
                  AND NOT is_cellbase 
                  AND (cycles IS NULL OR cycles = 0)
                ORDER BY block_number
                LIMIT $3
                "#,
            )
            .bind(start)
            .bind(end)
            .bind(BATCH_SIZE)
            .fetch_all(&self.pool)
            .await?;

            if txs.is_empty() {
                break;
            }

            let tx_hashes: Vec<String> = txs
                .iter()
                .map(|(h,)| format!("0x{}", hex::encode(h)))
                .collect();

            let batch_processed = self.process_batch(&tx_hashes).await;
            processed += batch_processed as i64;

            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                processed as f64 / elapsed
            } else {
                0.0
            };

            control_plane
                .update_job_progress(job_id, processed, None, Some(speed))
                .await;

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!(
            "Cycles fix for range {} to {} completed: {} transactions processed",
            start, end, processed
        );
        Ok(())
    }

    async fn process_batch(&self, tx_hashes: &[String]) -> usize {
        let mut futures = FuturesUnordered::new();
        let mut count = 0;

        for tx_hash in tx_hashes {
            let rpc_url = self.ckb_rpc_url.clone();
            let hash = tx_hash.clone();
            futures.push(async move {
                let result = ckbadger_common::cycles::calculate_cycles(&rpc_url, &hash).await;
                (hash, result)
            });

            if futures.len() >= CONCURRENT_CALCULATIONS {
                if let Some((hash, result)) = futures.next().await {
                    self.update_cycles(&hash, result).await;
                    count += 1;
                }
            }
        }

        while let Some((hash, result)) = futures.next().await {
            self.update_cycles(&hash, result).await;
            count += 1;
        }

        count
    }

    async fn update_cycles(&self, tx_hash: &str, result: Result<i64, String>) {
        let hash_bytes =
            hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)).unwrap_or_default();

        match result {
            Ok(cycles) => {
                if let Err(e) = sqlx::query("UPDATE transactions SET cycles = $1 WHERE hash = $2")
                    .bind(cycles)
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await
                {
                    warn!("Failed to update cycles for {}: {}", tx_hash, e);
                } else {
                    debug!("Updated cycles for {}: {}", tx_hash, cycles);
                }
            }
            Err(e) => {
                warn!("Failed to calculate cycles for {}: {}", tx_hash, e);
                let _ = sqlx::query("UPDATE transactions SET cycles = -1 WHERE hash = $1")
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await;
            }
        }
    }
}
