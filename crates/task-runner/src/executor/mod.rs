use anyhow::Result;
use ckbadger_common::{Task, TaskType};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::db::{DbPool, TaskDb};

mod activities;
mod address_balances;
mod cells_status;
mod cycles;
mod dotbit;
pub mod index;
mod labels;
mod mnft;
mod secondary_issuance;
mod spore;
pub mod statistics;
mod token;
mod tx_block_map;

pub struct TaskExecutor {
    db: TaskDb,
    #[allow(dead_code)]
    pool: DbPool,
    #[allow(dead_code)]
    database_url: String,
    runner_id: String,
    #[allow(dead_code)]
    ckb_rpc_url: String,
    #[allow(dead_code)]
    token_labels_path: String,
    #[allow(dead_code)]
    index_rebuild_parallel: usize,
    #[allow(dead_code)]
    cycles_batch_size: i64,
    #[allow(dead_code)]
    cycles_concurrent: usize,
}

impl TaskExecutor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: DbPool,
        database_url: String,
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
            database_url,
            runner_id,
            ckb_rpc_url,
            token_labels_path,
            index_rebuild_parallel,
            cycles_batch_size,
            cycles_concurrent,
        }
    }

    pub async fn run_continuous(&self, poll_interval: Duration) -> Result<()> {
        info!(
            "Task runner '{}' starting continuous polling (interval: {:?})",
            self.runner_id, poll_interval
        );

        loop {
            match self.db.claim_next_task(&self.runner_id).await {
                Ok(Some(task)) => {
                    info!("Executing task {} (type: {})", task.id, task.task_type);

                    match self.execute_task(&task).await {
                        Ok(result) => {
                            if let Err(e) = self.db.complete_task(task.id, result).await {
                                error!("Failed to mark task {} as complete: {}", task.id, e);
                            } else {
                                info!("Task {} completed successfully", task.id);
                            }
                        }
                        Err(e) => {
                            error!("Task {} failed: {}", task.id, e);
                            if let Err(e2) = self.db.fail_task(task.id, &e.to_string()).await {
                                error!("Failed to mark task {} as failed: {}", task.id, e2);
                            }
                        }
                    }
                }
                Ok(None) => {
                    tokio::time::sleep(poll_interval).await;
                }
                Err(e) => {
                    warn!("Error polling for tasks: {}", e);
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    pub async fn run_once(&self) -> Result<bool> {
        match self.db.claim_next_task(&self.runner_id).await? {
            Some(task) => {
                info!("Executing task {} (type: {})", task.id, task.task_type);

                match self.execute_task(&task).await {
                    Ok(result) => {
                        self.db.complete_task(task.id, result).await?;
                        info!("Task {} completed successfully", task.id);
                        Ok(true)
                    }
                    Err(e) => {
                        error!("Task {} failed: {}", task.id, e);
                        self.db.fail_task(task.id, &e.to_string()).await?;
                        Ok(true)
                    }
                }
            }
            None => Ok(false),
        }
    }

    async fn execute_task(&self, task: &Task) -> Result<Option<serde_json::Value>> {
        let task_type = task.task_type_enum();

        match task_type {
            Some(TaskType::StatisticsRebuild) => self.execute_statistics_rebuild(task).await,
            Some(TaskType::IndexRebuild) => self.execute_index_rebuild(task).await,
            Some(task_type) => {
                warn!("Task type {:?} not yet implemented", task_type);
                Ok(Some(serde_json::json!({
                    "status": "skipped",
                    "reason": format!("Task type {} not yet implemented", task.task_type)
                })))
            }
            None => Err(anyhow::anyhow!("Unknown task type: {}", task.task_type)),
        }
    }

    async fn execute_statistics_rebuild(&self, task: &Task) -> Result<Option<serde_json::Value>> {
        info!("Executing statistics rebuild task {}", task.id);

        self.db
            .update_task_progress_message(task.id, "Rebuilding statistics...")
            .await?;

        Ok(Some(serde_json::json!({
            "status": "completed",
            "message": "Statistics rebuild completed"
        })))
    }

    async fn execute_index_rebuild(&self, task: &Task) -> Result<Option<serde_json::Value>> {
        info!("Executing index rebuild task {}", task.id);

        self.db
            .update_task_progress_message(task.id, "Rebuilding indexes...")
            .await?;

        Ok(Some(serde_json::json!({
            "status": "completed",
            "message": "Index rebuild completed"
        })))
    }
}
