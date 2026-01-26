#![allow(dead_code)]

use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use std::time::Instant;
use tracing::{error, info};

const PARTITION_SIZE: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildTask {
    LiveCells,
    AddressBalances,
    ScriptUsageStats,
    DailyStatistics,
    HourlyStatistics,
    EpochStatistics,
    MinerStatistics,
    Indexes,
    AddressTransactions,
}

impl RebuildTask {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LiveCells => "live_cells",
            Self::AddressBalances => "address_balances",
            Self::ScriptUsageStats => "script_usage_stats",
            Self::DailyStatistics => "daily_statistics",
            Self::HourlyStatistics => "hourly_statistics",
            Self::EpochStatistics => "epoch_statistics",
            Self::MinerStatistics => "miner_statistics",
            Self::Indexes => "indexes",
            Self::AddressTransactions => "address_transactions",
        }
    }

    pub fn all_ordered() -> Vec<Self> {
        vec![
            Self::LiveCells,
            Self::AddressBalances,
            Self::ScriptUsageStats,
            Self::DailyStatistics,
            Self::HourlyStatistics,
            Self::EpochStatistics,
            Self::MinerStatistics,
            Self::Indexes,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct RebuildProgress {
    pub task_name: String,
    pub status: String,
    pub progress_current: i64,
    pub progress_total: Option<i64>,
    pub partition_current: Option<i32>,
    pub partition_total: Option<i32>,
    pub rows_per_second: Option<f64>,
}

pub struct RebuildRunner {
    pool: PgPool,
}

impl RebuildRunner {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn run_full_rebuild(&self) -> Result<()> {
        info!("Starting full rebuild of derived tables");
        let start = Instant::now();

        for task in RebuildTask::all_ordered() {
            self.run_task(task).await?;
        }

        info!(
            "Full rebuild completed in {:.2}s",
            start.elapsed().as_secs_f64()
        );
        Ok(())
    }

    pub async fn run_task(&self, task: RebuildTask) -> Result<()> {
        let task_name = task.name();
        info!("Starting rebuild task: {}", task_name);

        self.update_progress(task_name, "running", 0, None, None, None)
            .await?;

        let start = Instant::now();
        let result = match task {
            RebuildTask::LiveCells => self.rebuild_live_cells().await,
            RebuildTask::AddressBalances => self.rebuild_address_balances().await,
            RebuildTask::ScriptUsageStats => self.rebuild_script_usage_stats().await,
            RebuildTask::DailyStatistics => self.rebuild_daily_statistics().await,
            RebuildTask::HourlyStatistics => self.rebuild_hourly_statistics().await,
            RebuildTask::EpochStatistics => self.rebuild_epoch_statistics().await,
            RebuildTask::MinerStatistics => self.rebuild_miner_statistics().await,
            RebuildTask::Indexes => self.rebuild_indexes().await,
            RebuildTask::AddressTransactions => self.rebuild_address_transactions().await,
        };

        let elapsed = start.elapsed();
        match result {
            Ok(rows) => {
                let rows_per_sec = rows as f64 / elapsed.as_secs_f64();
                info!(
                    "Completed rebuild task: {} ({} rows in {:.2}s, {:.0} rows/s)",
                    task_name,
                    rows,
                    elapsed.as_secs_f64(),
                    rows_per_sec
                );
                self.update_progress(task_name, "completed", rows, Some(rows), None, Some(rows_per_sec))
                    .await?;
                Ok(())
            }
            Err(e) => {
                error!("Failed rebuild task {}: {}", task_name, e);
                self.set_error(task_name, &e.to_string()).await?;
                Err(e)
            }
        }
    }

    async fn rebuild_live_cells(&self) -> Result<i64> {
        let max_block: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(created_at_block), 0) FROM cells")
            .fetch_one(&self.pool)
            .await?;

        if max_block == 0 {
            return Ok(0);
        }

        let total_partitions = ((max_block / PARTITION_SIZE) + 1) as i32;
        self.update_progress("live_cells", "running", 0, None, Some(0), None)
            .await?;

        sqlx::query("TRUNCATE live_cells")
            .execute(&self.pool)
            .await?;

        let mut total_rows = 0i64;
        let start = Instant::now();

        for partition in 0..total_partitions {
            let partition_start = (partition as i64) * PARTITION_SIZE;
            let partition_end = partition_start + PARTITION_SIZE;

            let rows: i64 = sqlx::query_scalar(
                "SELECT rebuild_live_cells_partition($1, $2)",
            )
            .bind(partition_start)
            .bind(partition_end)
            .fetch_one(&self.pool)
            .await?;

            total_rows += rows;

            let elapsed = start.elapsed().as_secs_f64();
            let rows_per_sec = if elapsed > 0.0 {
                total_rows as f64 / elapsed
            } else {
                0.0
            };

            self.update_progress(
                "live_cells",
                "running",
                total_rows,
                None,
                Some(partition + 1),
                Some(rows_per_sec),
            )
            .await?;

            if (partition + 1) % 5 == 0 {
                info!(
                    "live_cells rebuild: partition {}/{}, {} rows total, {:.0} rows/s",
                    partition + 1,
                    total_partitions,
                    total_rows,
                    rows_per_sec
                );
            }
        }

        Ok(total_rows)
    }

    async fn rebuild_address_balances(&self) -> Result<i64> {
        let rows: i64 = sqlx::query_scalar("SELECT rebuild_address_balances()")
            .fetch_one(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn rebuild_script_usage_stats(&self) -> Result<i64> {
        let rows: i64 = sqlx::query_scalar("SELECT rebuild_script_usage_stats()")
            .fetch_one(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn rebuild_daily_statistics(&self) -> Result<i64> {
        let rows: i64 = sqlx::query_scalar("SELECT rebuild_daily_statistics()")
            .fetch_one(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn rebuild_hourly_statistics(&self) -> Result<i64> {
        let rows: i64 = sqlx::query_scalar("SELECT rebuild_hourly_statistics()")
            .fetch_one(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn rebuild_epoch_statistics(&self) -> Result<i64> {
        let rows: i64 = sqlx::query_scalar("SELECT rebuild_epoch_statistics()")
            .fetch_one(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn rebuild_miner_statistics(&self) -> Result<i64> {
        let rows: i64 = sqlx::query_scalar("SELECT rebuild_miner_statistics()")
            .fetch_one(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn rebuild_indexes(&self) -> Result<i64> {
        info!("Dropping indexes for rebuild...");
        let dropped: i32 = sqlx::query_scalar("SELECT drop_sync_indexes()")
            .fetch_one(&self.pool)
            .await?;
        info!("Dropped {} indexes", dropped);

        info!("Recreating indexes concurrently...");
        let created: i32 = sqlx::query_scalar("SELECT recreate_sync_indexes()")
            .fetch_one(&self.pool)
            .await?;
        info!("Recreated {} indexes", created);

        Ok(created as i64)
    }

    async fn rebuild_address_transactions(&self) -> Result<i64> {
        let max_block: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(block_number), 0) FROM transactions")
                .fetch_one(&self.pool)
                .await?;

        if max_block == 0 {
            return Ok(0);
        }

        sqlx::query("TRUNCATE address_transactions")
            .execute(&self.pool)
            .await?;

        let total_partitions = ((max_block / PARTITION_SIZE) + 1) as i32;
        let mut total_rows = 0i64;
        let start = Instant::now();

        for partition in 0..total_partitions {
            let partition_start = (partition as i64) * PARTITION_SIZE;
            let partition_end = partition_start + PARTITION_SIZE;

            let rows = sqlx::query(
                r#"
                INSERT INTO address_transactions (
                    lock_script_hash, tx_hash, block_number, tx_type, capacity_change, timestamp
                )
                SELECT 
                    c.lock_script_hash,
                    c.tx_hash,
                    c.created_at_block,
                    CASE 
                        WHEN c.status = 0 THEN 1 
                        ELSE 3 
                    END as tx_type,
                    c.capacity::bigint as capacity_change,
                    b.timestamp
                FROM cells c
                JOIN blocks b ON c.created_at_block = b.number
                WHERE c.created_at_block >= $1 AND c.created_at_block < $2
                ON CONFLICT (lock_script_hash, block_number, tx_hash) DO NOTHING
                "#,
            )
            .bind(partition_start)
            .bind(partition_end)
            .execute(&self.pool)
            .await?;

            total_rows += rows.rows_affected() as i64;

            let elapsed = start.elapsed().as_secs_f64();
            let rows_per_sec = if elapsed > 0.0 {
                total_rows as f64 / elapsed
            } else {
                0.0
            };

            self.update_progress(
                "address_transactions",
                "running",
                total_rows,
                None,
                Some(partition + 1),
                Some(rows_per_sec),
            )
            .await?;

            if (partition + 1) % 5 == 0 {
                info!(
                    "address_transactions rebuild: partition {}/{}, {} rows total, {:.0} rows/s",
                    partition + 1,
                    total_partitions,
                    total_rows,
                    rows_per_sec
                );
            }
        }

        Ok(total_rows)
    }

    async fn update_progress(
        &self,
        task_name: &str,
        status: &str,
        progress_current: i64,
        progress_total: Option<i64>,
        partition_current: Option<i32>,
        rows_per_second: Option<f64>,
    ) -> Result<()> {
        let started_at = if status == "running" && progress_current == 0 {
            Some(Utc::now())
        } else {
            None
        };

        let completed_at = if status == "completed" {
            Some(Utc::now())
        } else {
            None
        };

        sqlx::query(
            r#"
            INSERT INTO rebuild_progress (
                task_name, status, progress_current, progress_total, 
                partition_current, rows_per_second, started_at, completed_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (task_name) DO UPDATE SET
                status = EXCLUDED.status,
                progress_current = EXCLUDED.progress_current,
                progress_total = COALESCE(EXCLUDED.progress_total, rebuild_progress.progress_total),
                partition_current = COALESCE(EXCLUDED.partition_current, rebuild_progress.partition_current),
                rows_per_second = COALESCE(EXCLUDED.rows_per_second, rebuild_progress.rows_per_second),
                started_at = COALESCE(rebuild_progress.started_at, EXCLUDED.started_at),
                completed_at = EXCLUDED.completed_at
            "#,
        )
        .bind(task_name)
        .bind(status)
        .bind(progress_current)
        .bind(progress_total)
        .bind(partition_current)
        .bind(rows_per_second)
        .bind(started_at)
        .bind(completed_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn set_error(&self, task_name: &str, error: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE rebuild_progress 
            SET status = 'failed', error_message = $2
            WHERE task_name = $1
            "#,
        )
        .bind(task_name)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_progress(&self, task_name: &str) -> Result<Option<RebuildProgress>> {
        let row = sqlx::query_as::<
            _,
            (String, String, i64, Option<i64>, Option<i32>, Option<i32>, Option<f64>),
        >(
            r#"
            SELECT task_name, status, progress_current, progress_total, 
                   partition_current, partition_total, rows_per_second
            FROM rebuild_progress
            WHERE task_name = $1
            "#,
        )
        .bind(task_name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(name, status, current, total, part_cur, part_total, rps)| RebuildProgress {
            task_name: name,
            status,
            progress_current: current,
            progress_total: total,
            partition_current: part_cur,
            partition_total: part_total,
            rows_per_second: rps,
        }))
    }

    pub async fn get_all_progress(&self) -> Result<Vec<RebuildProgress>> {
        let rows = sqlx::query_as::<
            _,
            (String, String, i64, Option<i64>, Option<i32>, Option<i32>, Option<f64>),
        >(
            r#"
            SELECT task_name, status, progress_current, progress_total, 
                   partition_current, partition_total, rows_per_second
            FROM rebuild_progress
            ORDER BY task_name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(name, status, current, total, part_cur, part_total, rps)| RebuildProgress {
                task_name: name,
                status,
                progress_current: current,
                progress_total: total,
                partition_current: part_cur,
                partition_total: part_total,
                rows_per_second: rps,
            })
            .collect())
    }

    pub async fn reset_progress(&self) -> Result<()> {
        sqlx::query("TRUNCATE rebuild_progress")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rebuild_task_name() {
        assert_eq!(RebuildTask::LiveCells.name(), "live_cells");
        assert_eq!(RebuildTask::AddressBalances.name(), "address_balances");
        assert_eq!(RebuildTask::ScriptUsageStats.name(), "script_usage_stats");
        assert_eq!(RebuildTask::DailyStatistics.name(), "daily_statistics");
        assert_eq!(RebuildTask::HourlyStatistics.name(), "hourly_statistics");
        assert_eq!(RebuildTask::EpochStatistics.name(), "epoch_statistics");
        assert_eq!(RebuildTask::MinerStatistics.name(), "miner_statistics");
        assert_eq!(RebuildTask::Indexes.name(), "indexes");
        assert_eq!(RebuildTask::AddressTransactions.name(), "address_transactions");
    }

    #[test]
    fn test_rebuild_task_all_ordered() {
        let tasks = RebuildTask::all_ordered();
        
        assert_eq!(tasks.len(), 8);
        
        assert_eq!(tasks[0], RebuildTask::LiveCells);
        assert_eq!(tasks[1], RebuildTask::AddressBalances);
        assert_eq!(tasks[2], RebuildTask::ScriptUsageStats);
        assert_eq!(tasks[3], RebuildTask::DailyStatistics);
        assert_eq!(tasks[4], RebuildTask::HourlyStatistics);
        assert_eq!(tasks[5], RebuildTask::EpochStatistics);
        assert_eq!(tasks[6], RebuildTask::MinerStatistics);
        assert_eq!(tasks[7], RebuildTask::Indexes);
    }

    #[test]
    fn test_rebuild_task_all_ordered_does_not_include_address_transactions() {
        let tasks = RebuildTask::all_ordered();
        assert!(!tasks.contains(&RebuildTask::AddressTransactions));
    }

    #[test]
    fn test_rebuild_task_order_reflects_dependencies() {
        let tasks = RebuildTask::all_ordered();
        
        let live_cells_pos = tasks.iter().position(|t| *t == RebuildTask::LiveCells).unwrap();
        let address_balances_pos = tasks.iter().position(|t| *t == RebuildTask::AddressBalances).unwrap();
        let indexes_pos = tasks.iter().position(|t| *t == RebuildTask::Indexes).unwrap();
        
        assert!(live_cells_pos < address_balances_pos);
        assert!(indexes_pos == tasks.len() - 1);
    }

    #[test]
    fn test_rebuild_task_equality() {
        assert_eq!(RebuildTask::LiveCells, RebuildTask::LiveCells);
        assert_ne!(RebuildTask::LiveCells, RebuildTask::AddressBalances);
    }

    #[test]
    fn test_rebuild_task_clone() {
        let task = RebuildTask::LiveCells;
        let cloned = task;
        assert_eq!(task, cloned);
    }

    #[test]
    fn test_partition_size_constant() {
        assert_eq!(PARTITION_SIZE, 1_000_000);
    }
}
