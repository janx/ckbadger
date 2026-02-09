use anyhow::Result;
use ckbadger_common::{
    IndexCompletionInfo, IndexFailureInfo, IndexRebuildConfig, IndexRebuildResult, RateCalculator,
};
use sqlx::PgPool;
use std::time::Instant;
use tokio::task::JoinSet;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::TaskDb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartitionType {
    Range,
    #[allow(dead_code)]
    Hash,
}

struct DeferrableIndex {
    name: &'static str,
    table: &'static str,
    definition: &'static str,
    partition_type: PartitionType,
    priority: u8,
}

pub struct DeferrableConstraint {
    pub name: &'static str,
    pub table: &'static str,
    pub columns: &'static str,
}

const RANGE_PARTITION_SUFFIXES: &[&str] = &[
    "_p00", "_p01", "_p02", "_p03", "_p04", "_p05", "_p06", "_p07", "_p08", "_p09",
];

const HASH_PARTITION_SUFFIXES: &[&str] = &[
    "_p00", "_p01", "_p02", "_p03", "_p04", "_p05", "_p06", "_p07", "_p08", "_p09", "_p10", "_p11",
    "_p12", "_p13", "_p14", "_p15",
];

const DEFERRABLE_INDEXES: &[DeferrableIndex] = &[
    DeferrableIndex { name: "idx_blocks_hash", table: "blocks", definition: "CREATE INDEX {name} ON {table}(hash)", partition_type: PartitionType::Range, priority: 1 },
    DeferrableIndex { name: "idx_blocks_epoch", table: "blocks", definition: "CREATE INDEX {name} ON {table}(epoch_number)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_blocks_miner", table: "blocks", definition: "CREATE INDEX {name} ON {table}(miner_lock_hash) WHERE miner_lock_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_tx_timestamp", table: "transactions", definition: "CREATE INDEX {name} ON {table}(timestamp DESC)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_tx_cursor", table: "transactions", definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_tx_list_covering", table: "transactions", definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC) INCLUDE (hash, inputs_count, outputs_count, fee, is_cellbase, timestamp)", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_type_script_hash", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_cells_lock_code_hash", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_code_hash)", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_type_code_hash", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_lock_code_hash_live", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_code_hash, lock_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_type_code_hash_live", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_code_hash, type_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0 AND type_code_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_list_covering", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) INCLUDE (tx_hash, output_index, capacity, type_script_hash, data_size) WHERE status = 0", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_uncles_hash", table: "uncle_blocks", definition: "CREATE INDEX {name} ON {table}(hash)", partition_type: PartitionType::Range, priority: 3 },
];

const DEFERRABLE_CONSTRAINTS: &[DeferrableConstraint] = &[
    DeferrableConstraint {
        name: "created_at_block_tx_hash_output_index_key",
        table: "cells",
        columns: "created_at_block, tx_hash, output_index",
    },
    DeferrableConstraint {
        name: "tx_block_number_tx_hash_input_index_key",
        table: "transaction_inputs",
        columns: "tx_block_number, tx_hash, input_index",
    },
    DeferrableConstraint {
        name: "tx_block_number_tx_hash_dep_index_key",
        table: "transaction_cell_deps",
        columns: "tx_block_number, tx_hash, dep_index",
    },
    DeferrableConstraint {
        name: "block_number_proposal_index_key",
        table: "block_proposals",
        columns: "block_number, proposal_index",
    },
    DeferrableConstraint {
        name: "block_number_uncle_index_key",
        table: "uncle_blocks",
        columns: "block_number, uncle_index",
    },
];

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &IndexRebuildConfig,
) -> Result<()> {
    info!(
        "Starting index rebuild: parallel={}, rebuild_constraints={}",
        config.parallel_connections, config.rebuild_constraints
    );

    let total_indexes = DEFERRABLE_INDEXES.len();
    let total_constraints = if config.rebuild_constraints {
        DEFERRABLE_CONSTRAINTS.len()
    } else {
        0
    };

    let mut result = IndexRebuildResult {
        total_indexes,
        completed_indexes: 0,
        current_index: None,
        completed: vec![],
        failed: vec![],
        total_constraints,
        completed_constraints: 0,
    };

    db.update_progress(
        task_id,
        0,
        (total_indexes + total_constraints) as i64,
        Some("Starting index rebuild..."),
        None,
    )
    .await?;

    // Group indexes by priority, then build each priority group concurrently.
    // Indexes use CREATE INDEX CONCURRENTLY (no exclusive locks), so indexes on
    // different tables — and even the same table — can be built in parallel.
    // This reduces total wall time by ~30-40% vs building one-at-a-time.
    let mut priority_groups: std::collections::BTreeMap<u8, Vec<&DeferrableIndex>> =
        std::collections::BTreeMap::new();
    for idx in DEFERRABLE_INDEXES {
        priority_groups.entry(idx.priority).or_default().push(idx);
    }

    let mut rate_calc = RateCalculator::default();

    for (priority, group) in &priority_groups {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled, stopping");
            return Ok(());
        }

        let group_names: Vec<&str> = group.iter().map(|i| i.name).collect();
        info!(
            "Building priority {} indexes concurrently: {:?}",
            priority, group_names
        );
        let msg = format!(
            "Building {} priority-{} indexes: {}",
            group.len(),
            priority,
            group_names.join(", ")
        );
        db.append_log(task_id, &msg).await?;

        let mut join_set: JoinSet<(String, std::result::Result<(), String>, u64)> = JoinSet::new();

        for idx in group {
            let pool = pool.clone();
            let name = idx.name.to_string();
            let table = idx.table;
            let definition = idx.definition;
            let partition_type = idx.partition_type;
            let parallel = config.parallel_connections;

            join_set.spawn(async move {
                let start = Instant::now();
                let suffixes = match partition_type {
                    PartitionType::Range => RANGE_PARTITION_SUFFIXES,
                    PartitionType::Hash => HASH_PARTITION_SUFFIXES,
                };
                // Build partition index SQL commands
                let partition_tasks: Vec<IndexBuildTask> = suffixes
                    .iter()
                    .map(|suffix| {
                        let part_table = format!("{}{}", table, suffix);
                        let base_name = &name[4..]; // strip "idx_"
                        let index_name = format!("{}_{}{}", table, base_name, suffix);
                        let sql = definition
                            .replace("{name}", &index_name)
                            .replace("{table}", &part_table)
                            .replace("CREATE INDEX", "CREATE INDEX CONCURRENTLY IF NOT EXISTS");
                        IndexBuildTask {
                            index_name,
                            sql,
                            attempt: 1,
                        }
                    })
                    .collect();

                let result =
                    run_partition_index_tasks(&pool, partition_tasks, parallel.min(4)).await;
                let duration_ms = start.elapsed().as_millis() as u64;
                (name, result.map_err(|e| e.to_string()), duration_ms)
            });
        }

        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((name, build_result, duration_ms)) => match build_result {
                    Ok(_) => {
                        result.completed_indexes += 1;
                        result.completed.push(IndexCompletionInfo {
                            name: name.clone(),
                            duration_ms,
                        });
                        info!("Built index {} in {}ms", name, duration_ms);
                    }
                    Err(e) => {
                        result.failed.push(IndexFailureInfo {
                            name: name.clone(),
                            error: e.clone(),
                        });
                        warn!("Failed to build index {}: {}", name, e);
                    }
                },
                Err(e) => {
                    warn!("Index build task panicked: {}", e);
                }
            }

            rate_calc.add_sample(result.completed_indexes as i64);
            let progress = result.completed_indexes + result.completed_constraints;
            let msg = format!(
                "Indexes: {}/{}, Constraints: {}/{}",
                result.completed_indexes,
                total_indexes,
                result.completed_constraints,
                total_constraints
            );
            db.update_progress(
                task_id,
                progress as i64,
                (total_indexes + total_constraints) as i64,
                Some(&msg),
                rate_calc.rate(),
            )
            .await?;
        }
    }

    result.current_index = None;

    if config.rebuild_constraints {
        for constraint in DEFERRABLE_CONSTRAINTS {
            if db.check_cancelled(task_id).await? {
                info!("Task cancelled, stopping");
                return Ok(());
            }

            let msg = format!("Building constraint: {}", constraint.name);
            db.append_log(task_id, &msg).await?;

            let build_result =
                rebuild_partitioned_constraint(pool, constraint, config.parallel_connections).await;

            match build_result {
                Ok(_) => {
                    result.completed_constraints += 1;
                    info!("Built constraint {}", constraint.name);
                }
                Err(e) => {
                    warn!("Failed to build constraint {}: {}", constraint.name, e);
                }
            }

            let progress = result.completed_indexes + result.completed_constraints;
            let msg = format!(
                "Indexes: {}/{}, Constraints: {}/{}",
                result.completed_indexes,
                total_indexes,
                result.completed_constraints,
                total_constraints
            );
            db.update_progress(
                task_id,
                progress as i64,
                (total_indexes + total_constraints) as i64,
                Some(&msg),
                rate_calc.rate(),
            )
            .await?;
        }
    }

    info!(
        "Index rebuild completed: {}/{} indexes, {}/{} constraints",
        result.completed_indexes, total_indexes, result.completed_constraints, total_constraints
    );

    // Verify all required UNIQUE constraints exist before clearing deferred flag
    let missing = verify_constraints_exist(pool).await?;
    if !missing.is_empty() {
        warn!(
            "Found {} missing constraints after rebuild, attempting to create them",
            missing.len()
        );
        for (table, constraint_name, columns) in &missing {
            let sql = format!(
                "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({})",
                table, constraint_name, columns
            );
            match sqlx::query(&sql).execute(pool).await {
                Ok(_) => info!("Created missing constraint: {}", constraint_name),
                Err(e) => {
                    warn!("Failed to create constraint {}: {}", constraint_name, e);
                    return Err(anyhow::anyhow!(
                        "Cannot clear indexes_deferred: constraint {} is missing and could not be created: {}",
                        constraint_name,
                        e
                    ));
                }
            }
        }
    }

    sqlx::query("UPDATE sync_status SET indexes_deferred = false, indexes_dropped_at = NULL")
        .execute(pool)
        .await?;
    info!("Cleared indexes_deferred flag in sync_status");

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    Ok(())
}

/// Verify that all required UNIQUE constraints exist on partition tables.
/// Returns a list of (table_name, constraint_name, columns) for any missing constraints.
async fn verify_constraints_exist(pool: &PgPool) -> Result<Vec<(String, String, String)>> {
    let mut missing = Vec::new();

    for constraint in DEFERRABLE_CONSTRAINTS {
        for suffix in RANGE_PARTITION_SUFFIXES {
            let table_name = format!("{}{}", constraint.table, suffix);
            let constraint_name = format!("{}_{}", table_name, constraint.name);

            let exists: Option<(i32,)> = sqlx::query_as(&format!(
                "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
                constraint_name, table_name
            ))
            .fetch_optional(pool)
            .await?;

            if exists.is_none() {
                missing.push((table_name, constraint_name, constraint.columns.to_string()));
            }
        }

        // Also check the parent table
        let parent_constraint_name = format!("{}_{}", constraint.table, constraint.name);
        let parent_exists: Option<(i32,)> = sqlx::query_as(&format!(
            "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
            parent_constraint_name, constraint.table
        ))
        .fetch_optional(pool)
        .await?;

        if parent_exists.is_none() {
            missing.push((
                constraint.table.to_string(),
                parent_constraint_name,
                constraint.columns.to_string(),
            ));
        }
    }

    if missing.is_empty() {
        info!(
            "All {} UNIQUE constraints verified",
            DEFERRABLE_CONSTRAINTS.len() * (RANGE_PARTITION_SUFFIXES.len() + 1)
        );
    } else {
        warn!("Missing {} UNIQUE constraints", missing.len());
    }

    Ok(missing)
}

const MAX_RETRIES: usize = 3;
const RETRY_DELAY_MS: u64 = 5000;

struct IndexBuildTask {
    index_name: String,
    sql: String,
    attempt: usize,
}

/// Run a set of partition-level index build tasks with parallelism and retry logic.
async fn run_partition_index_tasks(
    pool: &PgPool,
    tasks: Vec<IndexBuildTask>,
    max_parallel: usize,
) -> Result<()> {
    let effective_parallel = max_parallel.min(4);
    let mut join_set: JoinSet<std::result::Result<String, (IndexBuildTask, String)>> =
        JoinSet::new();
    let mut pending = tasks;
    let mut retry_queue: Vec<IndexBuildTask> = Vec::new();

    while !pending.is_empty() || !join_set.is_empty() || !retry_queue.is_empty() {
        while join_set.len() < effective_parallel && !pending.is_empty() {
            let task = pending.remove(0);
            let pool = pool.clone();
            let sql = task.sql.clone();
            let index_name = task.index_name.clone();
            let attempt = task.attempt;

            join_set.spawn(async move {
                match sqlx::query(&sql).execute(&pool).await {
                    Ok(_) => Ok(index_name),
                    Err(e) => Err((
                        IndexBuildTask {
                            index_name,
                            sql,
                            attempt,
                        },
                        e.to_string(),
                    )),
                }
            });
        }

        while join_set.len() < effective_parallel && !retry_queue.is_empty() {
            let mut task = retry_queue.remove(0);
            task.attempt += 1;
            let pool = pool.clone();
            let sql = task.sql.clone();
            let index_name = task.index_name.clone();
            let attempt = task.attempt;

            info!(
                "Retrying index {} (attempt {}/{})",
                index_name, attempt, MAX_RETRIES
            );

            join_set.spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                match sqlx::query(&sql).execute(&pool).await {
                    Ok(_) => Ok(index_name),
                    Err(e) => Err((
                        IndexBuildTask {
                            index_name,
                            sql,
                            attempt,
                        },
                        e.to_string(),
                    )),
                }
            });
        }

        if let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(_)) => {}
                Ok(Err((task, err_str))) => {
                    let is_lock_timeout = err_str.contains("lock timeout")
                        || err_str.contains("could not obtain lock")
                        || err_str.contains("canceling statement due to lock timeout");

                    if is_lock_timeout && task.attempt < MAX_RETRIES {
                        warn!(
                            "Lock timeout for {}, will retry (attempt {}/{})",
                            task.index_name, task.attempt, MAX_RETRIES
                        );
                        retry_queue.push(task);
                    } else if is_lock_timeout {
                        warn!(
                            "Failed to create index {} after {} retries: {}",
                            task.index_name, MAX_RETRIES, err_str
                        );
                    } else {
                        warn!(
                            "Failed to create partition index {}: {}",
                            task.index_name, err_str
                        );
                    }
                }
                Err(e) => warn!("Task panicked: {}", e),
            }
        }
    }

    Ok(())
}

struct ConstraintBuildTask {
    table_name: String,
    constraint_name: String,
    columns: String,
    attempt: usize,
}

fn is_lock_timeout_error(err_str: &str) -> bool {
    err_str.contains("lock timeout")
        || err_str.contains("could not obtain lock")
        || err_str.contains("canceling statement due to lock timeout")
}

pub async fn rebuild_partitioned_constraint(
    pool: &PgPool,
    constraint: &DeferrableConstraint,
    max_parallel: usize,
) -> Result<()> {
    let effective_parallel = max_parallel.min(4);

    let mut join_set: JoinSet<std::result::Result<String, (ConstraintBuildTask, String)>> =
        JoinSet::new();
    let mut pending: Vec<ConstraintBuildTask> = RANGE_PARTITION_SUFFIXES
        .iter()
        .map(|suffix| {
            let table_name = format!("{}{}", constraint.table, suffix);
            let constraint_name = format!("{}_{}", table_name, constraint.name);
            ConstraintBuildTask {
                table_name,
                constraint_name,
                columns: constraint.columns.to_string(),
                attempt: 1,
            }
        })
        .collect();
    let mut retry_queue: Vec<ConstraintBuildTask> = Vec::new();

    while !pending.is_empty() || !join_set.is_empty() || !retry_queue.is_empty() {
        while join_set.len() < effective_parallel && !pending.is_empty() {
            let task = pending.remove(0);
            let pool = pool.clone();
            let table_name = task.table_name.clone();
            let constraint_name = task.constraint_name.clone();
            let columns = task.columns.clone();

            join_set.spawn(async move {
                let check_sql = format!(
                    "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
                    constraint_name, table_name
                );
                let exists: Option<(i32,)> = match sqlx::query_as(&check_sql)
                    .fetch_optional(&pool)
                    .await
                {
                    Ok(r) => r,
                    Err(e) => return Err((task, e.to_string())),
                };

                if exists.is_none() {
                    let add_sql = format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({})",
                        table_name, constraint_name, columns
                    );
                    match sqlx::query(&add_sql).execute(&pool).await {
                        Ok(_) => Ok(constraint_name),
                        Err(e) => Err((task, e.to_string())),
                    }
                } else {
                    Ok(constraint_name)
                }
            });
        }

        while join_set.len() < effective_parallel && !retry_queue.is_empty() {
            let mut task = retry_queue.remove(0);
            task.attempt += 1;
            let pool = pool.clone();
            let table_name = task.table_name.clone();
            let constraint_name = task.constraint_name.clone();
            let columns = task.columns.clone();
            let attempt = task.attempt;

            info!(
                "Retrying constraint {} (attempt {}/{})",
                constraint_name, attempt, MAX_RETRIES
            );

            join_set.spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                let add_sql = format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({})",
                    table_name, constraint_name, columns
                );
                match sqlx::query(&add_sql).execute(&pool).await {
                    Ok(_) => Ok(constraint_name),
                    Err(e) => Err((task, e.to_string())),
                }
            });
        }

        if let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(_)) => {}
                Ok(Err((task, err_str))) => {
                    if is_lock_timeout_error(&err_str) && task.attempt < MAX_RETRIES {
                        warn!(
                            "Lock timeout for constraint {}, will retry (attempt {}/{})",
                            task.constraint_name, task.attempt, MAX_RETRIES
                        );
                        retry_queue.push(task);
                    } else if is_lock_timeout_error(&err_str) {
                        warn!(
                            "Failed to create constraint {} after {} retries: {}",
                            task.constraint_name, MAX_RETRIES, err_str
                        );
                    } else {
                        warn!(
                            "Failed to create partition constraint {}: {}",
                            task.constraint_name, err_str
                        );
                    }
                }
                Err(e) => warn!("Task panicked: {}", e),
            }
        }
    }

    // Add the constraint to the parent table after all partitions are done
    let parent_constraint_name = format!("{}_{}", constraint.table, constraint.name);
    let check_parent_sql = format!(
        "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
        parent_constraint_name, constraint.table
    );
    let parent_exists: Option<(i32,)> = sqlx::query_as(&check_parent_sql)
        .fetch_optional(pool)
        .await?;

    if parent_exists.is_none() {
        let add_parent_sql = format!(
            "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({})",
            constraint.table, parent_constraint_name, constraint.columns
        );

        let mut attempt = 1;
        loop {
            match sqlx::query(&add_parent_sql).execute(pool).await {
                Ok(_) => {
                    info!(
                        "Added UNIQUE constraint {} to parent table {}",
                        parent_constraint_name, constraint.table
                    );
                    break;
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if is_lock_timeout_error(&err_str) && attempt < MAX_RETRIES {
                        warn!(
                            "Lock timeout for parent constraint {}, will retry (attempt {}/{})",
                            parent_constraint_name, attempt, MAX_RETRIES
                        );
                        attempt += 1;
                        tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS))
                            .await;
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }
    } else {
        info!(
            "UNIQUE constraint {} already exists on parent table {}",
            parent_constraint_name, constraint.table
        );
    }

    Ok(())
}
