use anyhow::Result;
use ckb_store_reader::CkbChainReader;
use ckbadger_common::{LabelImportConfig, Task, TaskConfig, TaskType};
use ckbadger_store::CkbadgerStore;
use futures::future::join_all;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::db::TaskDb;

mod labels;

pub struct TaskExecutor {
    db: TaskDb,
    store: Arc<CkbadgerStore>,
    runner_id: String,
    _ckb_rpc_url: String,
    token_labels_path: String,
    ckb_store: Option<Arc<CkbChainReader>>,
}

impl TaskExecutor {
    pub async fn new(
        store: Arc<CkbadgerStore>,
        runner_id: String,
        ckb_rpc_url: String,
        token_labels_path: String,
        redis_url: Option<String>,
        ckb_store: Option<Arc<CkbChainReader>>,
    ) -> Self {
        let redis_conn = match redis_url {
            Some(ref url) => match redis::Client::open(url.as_str()) {
                Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                    Ok(conn) => {
                        info!("Task runner Redis connection established for command queue");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!(
                            "Failed to connect to Redis for task commands: {}. API task mutations will not be processed.",
                            e
                        );
                        None
                    }
                },
                Err(e) => {
                    warn!("Invalid Redis URL for task commands: {}", e);
                    None
                }
            },
            None => {
                info!("No Redis URL configured — task command queue disabled");
                None
            }
        };

        Self {
            db: TaskDb::new(store.clone(), redis_conn),
            store,
            runner_id,
            _ckb_rpc_url: ckb_rpc_url,
            token_labels_path,
            ckb_store,
        }
    }

    /// Recover orphaned tasks left in 'running' state by a previous runner instance.
    /// Called once at startup before entering the main loop.
    pub async fn recover_orphaned_tasks(&self) -> Result<()> {
        let recovered = self.db.recover_orphaned_tasks(5 * 60).await?;
        if recovered > 0 {
            warn!(
                "Recovered {} orphaned task(s) with stale heartbeat (>5min)",
                recovered
            );
        }
        Ok(())
    }

    pub async fn run_continuous(&self, poll_interval: Duration) -> Result<()> {
        info!("Task executor starting continuous mode");

        self.recover_orphaned_tasks().await?;

        loop {
            // Process any pending Redis commands before checking for tasks
            self.db.process_redis_commands().await;

            match self.run_once().await {
                Ok(executed) => {
                    if !executed {
                        // During idle, sleep in 500ms chunks to check Redis commands frequently
                        let chunks = (poll_interval.as_millis() / 500).max(1) as u64;
                        for _ in 0..chunks {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            self.db.process_redis_commands().await;
                        }
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
        let tasks = self
            .db
            .claim_tasks_at_same_priority(&self.runner_id)
            .await?;
        if tasks.is_empty() {
            return Ok(false);
        }

        if tasks.len() == 1 {
            return self.process_single_task(&tasks[0]).await;
        }

        let task_types: Vec<&str> = tasks.iter().map(|t| t.task_type.as_str()).collect();
        info!(
            "Claimed {} tasks at priority {}, executing in parallel: {:?}",
            tasks.len(),
            tasks[0].priority,
            task_types
        );

        let futs: Vec<_> = tasks
            .iter()
            .map(|task| self.process_single_task(task))
            .collect();
        let results = join_all(futs).await;

        let mut any_executed = false;
        for result in results {
            match result {
                Ok(true) => any_executed = true,
                Ok(false) => {}
                Err(e) => {
                    error!("Parallel task execution error: {}", e);
                    any_executed = true;
                }
            }
        }

        Ok(any_executed)
    }

    async fn process_single_task(&self, task: &Task) -> Result<bool> {
        info!(
            "Processing task {} (type: {}, priority: {})",
            task.id, task.task_type, task.priority
        );

        let task_type = match task.task_type_enum() {
            Some(t) => t,
            None => {
                let err = format!("Invalid task type: {}", task.task_type);
                error!("{}", err);
                self.db.fail_task(task.id, &err).await?;
                return Ok(true);
            }
        };

        if task_type.requires_bulk_sync_completion() {
            match self.db.is_bulk_sync_active().await {
                Ok(true) => {
                    let reason = format!(
                        "Task {} deferred: bulk sync in progress (requires completion)",
                        task_type
                    );
                    info!("{}", reason);
                    self.db.defer_task(task.id, &reason).await?;
                    return Ok(false);
                }
                Ok(false) => {}
                Err(e) => {
                    error!("Failed to check bulk sync status: {}", e);
                }
            }
        }

        let result = self.execute_task(task).await;

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
            TaskType::LabelImport => self.execute_label_import(task).await,
            // These task types are no-ops in RocksDB mode — data is maintained inline by the indexer.
            TaskType::CyclesBackfill
            | TaskType::IndexRebuild
            | TaskType::StatisticsRebuild
            | TaskType::SporeRebuild
            | TaskType::SecondaryIssuanceBackfill
            | TaskType::ConsumedAtBackfill
            | TaskType::CellsStatusRebuild
            | TaskType::AddressBalancesRebuild
            | TaskType::TokenRebuild
            | TaskType::MnftRebuild
            | TaskType::DotbitRebuild
            | TaskType::TxBlockMapRebuild
            | TaskType::CellFlowsRebuild => {
                info!(
                    "Task type {} is a no-op with RocksDB storage (maintained by indexer)",
                    task.task_type
                );
                self.db
                    .complete_task(
                        task.id,
                        Some(serde_json::json!({"message": "No-op: maintained by indexer in RocksDB"})),
                    )
                    .await?;
                Ok(())
            }
            TaskType::DaoRebuild => Err(anyhow::anyhow!(
                "DaoRebuild must be executed by the indexer, not task-runner"
            )),
        }
    }

    async fn execute_label_import(&self, task: &Task) -> Result<()> {
        let mut config: LabelImportConfig = match task.config_typed() {
            Some(TaskConfig::LabelImport(c)) => c,
            _ => LabelImportConfig::default(),
        };

        if config.token_labels_path == "docs/token-labels" {
            config.token_labels_path = self.token_labels_path.clone();
        }

        labels::execute(
            &self.db,
            &self.store,
            self.ckb_store.as_deref(),
            task.id,
            &config,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use ckbadger_common::TaskType;

    #[test]
    fn test_label_import_does_not_require_bulk_sync() {
        assert!(!TaskType::LabelImport.requires_bulk_sync_completion());
    }
}
