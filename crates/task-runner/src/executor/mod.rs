use anyhow::Result;
use ckbadger_common::{
    ActivitiesRebuildConfig, AddressBalancesRebuildConfig, CellFlowsRebuildConfig,
    CellsStatusRebuildConfig, CyclesBackfillConfig, DotbitRebuildConfig, IndexRebuildConfig,
    LabelImportConfig, MnftRebuildConfig, SecondaryIssuanceBackfillConfig, SporeRebuildConfig,
    StatisticsRebuildConfig, Task, TaskConfig, TaskType, TokenRebuildConfig,
    TxBlockMapRebuildConfig,
};
use futures::future::join_all;
use redis::AsyncCommands;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::db::TaskDb;

mod activities;
mod address_balances;
mod cell_flows;
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
    pool: PgPool,
    database_url: String,
    runner_id: String,
    ckb_rpc_url: String,
    token_labels_path: String,
    redis_url: Option<String>,
    index_rebuild_parallel: usize,
    cycles_batch_size: i64,
    cycles_concurrent: usize,
}

impl TaskExecutor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        database_url: String,
        runner_id: String,
        ckb_rpc_url: String,
        token_labels_path: String,
        redis_url: Option<String>,
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
            redis_url,
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

        // Tasks that clear deferred flags need Redis sync:status invalidation
        let clears_deferred = matches!(
            task_type,
            TaskType::IndexRebuild
                | TaskType::ActivitiesRebuild
                | TaskType::AddressBalancesRebuild
                | TaskType::TokenRebuild
                | TaskType::SporeRebuild
                | TaskType::TxBlockMapRebuild
        );

        let result = match task_type {
            TaskType::CyclesBackfill => self.execute_cycles_backfill(task).await,
            TaskType::IndexRebuild => self.execute_index_rebuild(task).await,
            TaskType::LabelImport => self.execute_label_import(task).await,
            TaskType::StatisticsRebuild => self.execute_statistics_rebuild(task).await,
            TaskType::SporeRebuild => self.execute_spore_rebuild(task).await,
            TaskType::ConsumedAtBackfill => {
                warn!("ConsumedAtBackfill is deprecated, redirecting to cells_status_rebuild");
                self.execute_cells_status_rebuild(task).await
            }
            TaskType::SecondaryIssuanceBackfill => {
                self.execute_secondary_issuance_backfill(task).await
            }
            TaskType::CellsStatusRebuild => self.execute_cells_status_rebuild(task).await,
            TaskType::ActivitiesRebuild => self.execute_activities_rebuild(task).await,
            TaskType::AddressBalancesRebuild => self.execute_address_balances_rebuild(task).await,
            TaskType::TokenRebuild => self.execute_token_rebuild(task).await,
            TaskType::MnftRebuild => self.execute_mnft_rebuild(task).await,
            TaskType::DotbitRebuild => self.execute_dotbit_rebuild(task).await,
            TaskType::DaoRebuild => Err(anyhow::anyhow!(
                "DaoRebuild must be executed by the indexer, not task-runner"
            )),
            TaskType::TxBlockMapRebuild => self.execute_tx_block_map_rebuild(task).await,
            TaskType::CellFlowsRebuild => self.execute_cell_flows_rebuild(task).await,
        };

        // Invalidate Redis sync:status cache after tasks that clear deferred flags
        if result.is_ok() && clears_deferred {
            self.invalidate_sync_status_cache().await;
        }

        result
    }

    /// Delete the Redis sync:status key so the indexer regenerates it with correct deferred state.
    async fn invalidate_sync_status_cache(&self) {
        let Some(ref redis_url) = self.redis_url else {
            warn!("No Redis URL configured, cannot invalidate sync:status cache");
            return;
        };

        match redis::Client::open(redis_url.as_str()) {
            Ok(client) => match client.get_multiplexed_async_connection().await {
                Ok(mut conn) => {
                    let result: Result<(), _> = conn.del("sync:status").await;
                    match result {
                        Ok(()) => info!("Invalidated Redis sync:status cache"),
                        Err(e) => warn!("Failed to invalidate Redis sync:status: {}", e),
                    }
                }
                Err(e) => warn!("Failed to connect to Redis for cache invalidation: {}", e),
            },
            Err(e) => warn!("Invalid Redis URL for cache invalidation: {}", e),
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

    async fn execute_spore_rebuild(&self, task: &Task) -> Result<()> {
        let mut config: SporeRebuildConfig = match task.config_typed() {
            Some(TaskConfig::SporeRebuild(c)) => c,
            _ => SporeRebuildConfig::default(),
        };

        if config.ckb_rpc_url.is_empty() {
            config.ckb_rpc_url = self.ckb_rpc_url.clone();
        }

        spore::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_secondary_issuance_backfill(&self, task: &Task) -> Result<()> {
        let mut config: SecondaryIssuanceBackfillConfig = match task.config_typed() {
            Some(TaskConfig::SecondaryIssuanceBackfill(c)) => c,
            _ => SecondaryIssuanceBackfillConfig::default(),
        };

        if config.ckb_rpc_url.is_empty() {
            config.ckb_rpc_url = self.ckb_rpc_url.clone();
        }

        secondary_issuance::execute(&self.db, &self.pool, &self.database_url, task.id, &config)
            .await
    }

    async fn execute_cells_status_rebuild(&self, task: &Task) -> Result<()> {
        let config: CellsStatusRebuildConfig = match task.config_typed() {
            Some(TaskConfig::CellsStatusRebuild(c)) => c,
            _ => CellsStatusRebuildConfig::default(),
        };

        cells_status::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_activities_rebuild(&self, task: &Task) -> Result<()> {
        let config: ActivitiesRebuildConfig = match task.config_typed() {
            Some(TaskConfig::ActivitiesRebuild(c)) => c,
            _ => ActivitiesRebuildConfig::default(),
        };

        activities::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_address_balances_rebuild(&self, task: &Task) -> Result<()> {
        let config: AddressBalancesRebuildConfig = match task.config_typed() {
            Some(TaskConfig::AddressBalancesRebuild(c)) => c,
            _ => AddressBalancesRebuildConfig::default(),
        };

        address_balances::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_token_rebuild(&self, task: &Task) -> Result<()> {
        let config: TokenRebuildConfig = match task.config_typed() {
            Some(TaskConfig::TokenRebuild(c)) => c,
            _ => TokenRebuildConfig::default(),
        };

        token::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_mnft_rebuild(&self, task: &Task) -> Result<()> {
        let config: MnftRebuildConfig = match task.config_typed() {
            Some(TaskConfig::MnftRebuild(c)) => c,
            _ => MnftRebuildConfig::default(),
        };

        mnft::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_dotbit_rebuild(&self, task: &Task) -> Result<()> {
        let config: DotbitRebuildConfig = match task.config_typed() {
            Some(TaskConfig::DotbitRebuild(c)) => c,
            _ => DotbitRebuildConfig::default(),
        };

        dotbit::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_tx_block_map_rebuild(&self, task: &Task) -> Result<()> {
        let config: TxBlockMapRebuildConfig = match task.config_typed() {
            Some(TaskConfig::TxBlockMapRebuild(c)) => c,
            _ => TxBlockMapRebuildConfig::default(),
        };

        tx_block_map::execute(&self.db, &self.pool, task.id, &config).await
    }

    async fn execute_cell_flows_rebuild(&self, task: &Task) -> Result<()> {
        let config: CellFlowsRebuildConfig = match task.config_typed() {
            Some(TaskConfig::CellFlowsRebuild(c)) => c,
            _ => CellFlowsRebuildConfig::default(),
        };

        cell_flows::execute(&self.db, &self.pool, task.id, &config).await
    }
}

#[cfg(test)]
mod tests {
    use ckbadger_common::TaskType;

    #[test]
    fn test_parallel_task_groups_at_same_priority() {
        // Priority 8 tasks are independent and should run in parallel
        let priority_8: Vec<TaskType> = vec![
            TaskType::AddressBalancesRebuild,
            TaskType::TxBlockMapRebuild,
        ];
        // All require bulk sync completion
        for task_type in &priority_8 {
            assert!(task_type.requires_bulk_sync_completion());
        }
    }

    #[test]
    fn test_priority_7_tasks_are_independent() {
        let priority_7: Vec<TaskType> = vec![
            TaskType::ActivitiesRebuild,
            TaskType::TokenRebuild,
            TaskType::ConsumedAtBackfill,
        ];
        for task_type in &priority_7 {
            assert!(task_type.requires_bulk_sync_completion());
        }
    }

    #[test]
    fn test_priority_6_tasks_are_independent() {
        let priority_6: Vec<TaskType> = vec![
            TaskType::SporeRebuild,
            TaskType::MnftRebuild,
            TaskType::DotbitRebuild,
        ];
        for task_type in &priority_6 {
            assert!(task_type.requires_bulk_sync_completion());
        }
    }

    #[test]
    fn test_cycles_backfill_does_not_require_bulk_sync() {
        assert!(!TaskType::CyclesBackfill.requires_bulk_sync_completion());
    }

    #[test]
    fn test_label_import_does_not_require_bulk_sync() {
        assert!(!TaskType::LabelImport.requires_bulk_sync_completion());
    }

    #[test]
    fn test_clears_deferred_task_types() {
        // These task types clear deferred flags and need Redis invalidation
        let clears_deferred = |t: TaskType| {
            matches!(
                t,
                TaskType::IndexRebuild
                    | TaskType::ActivitiesRebuild
                    | TaskType::AddressBalancesRebuild
                    | TaskType::TokenRebuild
                    | TaskType::SporeRebuild
                    | TaskType::TxBlockMapRebuild
            )
        };

        assert!(clears_deferred(TaskType::IndexRebuild));
        assert!(clears_deferred(TaskType::ActivitiesRebuild));
        assert!(clears_deferred(TaskType::AddressBalancesRebuild));
        assert!(clears_deferred(TaskType::TokenRebuild));
        assert!(clears_deferred(TaskType::SporeRebuild));
        assert!(clears_deferred(TaskType::TxBlockMapRebuild));

        // These should NOT clear deferred flags
        assert!(!clears_deferred(TaskType::CyclesBackfill));
        assert!(!clears_deferred(TaskType::StatisticsRebuild));
        assert!(!clears_deferred(TaskType::LabelImport));
    }
}
