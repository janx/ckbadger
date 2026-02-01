use anyhow::Result;
use ckbadger_common::{
    CyclesBackfillConfig, IndexRebuildConfig, LabelImportConfig, StatisticsRebuildConfig, Task,
    TaskConfig, TaskType,
};
use sqlx::PgPool;
use std::time::Duration;
use tracing::{error, info};

use crate::db::TaskDb;

mod cycles;
mod index;
mod labels;
pub mod statistics;

pub struct TaskExecutor {
    db: TaskDb,
    pool: PgPool,
    runner_id: String,
    ckb_rpc_url: String,
    token_labels_path: String,
    index_rebuild_parallel: usize,
    cycles_batch_size: i64,
    cycles_concurrent: usize,
}

impl TaskExecutor {
    pub fn new(
        pool: PgPool,
        runner_id: String,
        ckb_rpc_url: String,
        token_labels_path: String,
        index_rebuild_parallel: usize,
        cycles_batch_size: i64,
        cycles_concurrent: usize,
    ) -> Self {
        Self {
            db: TaskDb::new(pool.clone()),
            pool,
            runner_id,
            ckb_rpc_url,
            token_labels_path,
            index_rebuild_parallel,
            cycles_batch_size,
            cycles_concurrent,
        }
    }

    pub async fn run_continuous(&self, poll_interval: Duration) -> Result<()> {
        info!("Task executor starting continuous mode");

        loop {
            match self.run_once().await {
                Ok(executed) => {
                    if !executed {
                        tokio::time::sleep(poll_interval).await;
                    }
                }
                Err(e) => {
                    error!("Task execution error: {}", e);
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    pub async fn run_once(&self) -> Result<bool> {
        let task = match self.db.claim_next_task(&self.runner_id).await? {
            Some(t) => t,
            None => return Ok(false),
        };

        info!(
            "Claimed task {} (type: {}, status: {})",
            task.id, task.task_type, task.status
        );

        let result = self.execute_task(&task).await;

        match result {
            Ok(()) => {
                info!("Task {} completed successfully", task.id);
            }
            Err(e) => {
                error!("Task {} failed: {}", task.id, e);
                self.db.fail_task(task.id, &e.to_string()).await?;
            }
        }

        Ok(true)
    }

    async fn execute_task(&self, task: &Task) -> Result<()> {
        let task_type = task
            .task_type_enum()
            .ok_or_else(|| anyhow::anyhow!("Invalid task type: {}", task.task_type))?;

        match task_type {
            TaskType::CyclesBackfill => self.execute_cycles_backfill(task).await,
            TaskType::IndexRebuild => self.execute_index_rebuild(task).await,
            TaskType::LabelImport => self.execute_label_import(task).await,
            TaskType::StatisticsRebuild => self.execute_statistics_rebuild(task).await,
            TaskType::LiveCellsPopulate => Err(anyhow::anyhow!(
                "LiveCellsPopulate must be executed by the indexer, not task-runner"
            )),
        }
    }

    async fn execute_cycles_backfill(&self, task: &Task) -> Result<()> {
        let config: CyclesBackfillConfig = match task.config_typed() {
            Some(TaskConfig::CyclesBackfill(c)) => c,
            _ => CyclesBackfillConfig {
                ckb_rpc_url: self.ckb_rpc_url.clone(),
                batch_size: self.cycles_batch_size,
                concurrent_requests: self.cycles_concurrent,
                ..Default::default()
            },
        };

        cycles::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_index_rebuild(&self, task: &Task) -> Result<()> {
        let config: IndexRebuildConfig = match task.config_typed() {
            Some(TaskConfig::IndexRebuild(c)) => c,
            _ => IndexRebuildConfig {
                parallel_connections: self.index_rebuild_parallel,
                ..Default::default()
            },
        };

        index::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_label_import(&self, task: &Task) -> Result<()> {
        let mut config: LabelImportConfig = match task.config_typed() {
            Some(TaskConfig::LabelImport(c)) => c,
            _ => LabelImportConfig::default(),
        };

        if config.token_labels_path == "docs/token-labels" {
            config.token_labels_path = self.token_labels_path.clone();
        }

        labels::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_statistics_rebuild(&self, task: &Task) -> Result<()> {
        let config: StatisticsRebuildConfig = match task.config_typed() {
            Some(TaskConfig::StatisticsRebuild(c)) => c,
            _ => StatisticsRebuildConfig::default(),
        };

        statistics::execute(&self.db, &self.pool, task.id, &config).await
    }
}
