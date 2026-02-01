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
    Hash,
}

struct DeferrableIndex {
    name: &'static str,
    table: &'static str,
    definition: &'static str,
    partition_type: PartitionType,
    priority: u8,
}

struct DeferrableConstraint {
    name: &'static str,
    table: &'static str,
    columns: &'static str,
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
    DeferrableIndex { name: "idx_tx_hash", table: "transactions", definition: "CREATE INDEX {name} ON {table}(hash)", partition_type: PartitionType::Range, priority: 1 },
    DeferrableIndex { name: "idx_tx_timestamp", table: "transactions", definition: "CREATE INDEX {name} ON {table}(timestamp DESC)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_tx_short_hash", table: "transactions", definition: "CREATE INDEX {name} ON {table}(short_hash, block_number)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_tx_cursor", table: "transactions", definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_tx_list_covering", table: "transactions", definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC) INCLUDE (hash, inputs_count, outputs_count, fee, is_cellbase, timestamp)", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_outpoint", table: "cells", definition: "CREATE INDEX {name} ON {table}(tx_hash, output_index)", partition_type: PartitionType::Range, priority: 1 },
    DeferrableIndex { name: "idx_cells_lock_live", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) WHERE status = 0", partition_type: PartitionType::Range, priority: 1 },
    DeferrableIndex { name: "idx_cells_lock_script_details", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_script_hash) INCLUDE (lock_code_hash, lock_hash_type, lock_args)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_cells_type_live", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_script_hash, created_at_block DESC) WHERE status = 0 AND type_script_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_cells_consumed_by", table: "cells", definition: "CREATE INDEX {name} ON {table}(consumed_by_tx) WHERE consumed_by_tx IS NOT NULL", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_cells_type_script_hash", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_cells_lock_code_hash", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_code_hash)", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_type_code_hash", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_lock_code_hash_live", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_code_hash, lock_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_type_code_hash_live", table: "cells", definition: "CREATE INDEX {name} ON {table}(type_code_hash, type_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0 AND type_code_hash IS NOT NULL", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_cells_list_covering", table: "cells", definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) INCLUDE (tx_hash, output_index, capacity, type_script_hash, data_size) WHERE status = 0", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_inputs_previous", table: "transaction_inputs", definition: "CREATE INDEX {name} ON {table}(previous_tx_hash, previous_output_index)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_inputs_tx", table: "transaction_inputs", definition: "CREATE INDEX {name} ON {table}(tx_hash)", partition_type: PartitionType::Range, priority: 2 },
    DeferrableIndex { name: "idx_cell_deps_tx", table: "transaction_cell_deps", definition: "CREATE INDEX {name} ON {table}(tx_hash)", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_uncles_hash", table: "uncle_blocks", definition: "CREATE INDEX {name} ON {table}(hash)", partition_type: PartitionType::Range, priority: 3 },
    DeferrableIndex { name: "idx_live_cells_lock", table: "live_cells", definition: "CREATE INDEX {name} ON {table}(lock_script_hash)", partition_type: PartitionType::Hash, priority: 1 },
    DeferrableIndex { name: "idx_live_cells_lock_code", table: "live_cells", definition: "CREATE INDEX {name} ON {table}(lock_code_hash)", partition_type: PartitionType::Hash, priority: 2 },
    DeferrableIndex { name: "idx_live_cells_type", table: "live_cells", definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL", partition_type: PartitionType::Hash, priority: 2 },
    DeferrableIndex { name: "idx_live_cells_type_code", table: "live_cells", definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL", partition_type: PartitionType::Hash, priority: 2 },
    DeferrableIndex { name: "idx_live_cells_block", table: "live_cells", definition: "CREATE INDEX {name} ON {table}(created_at_block)", partition_type: PartitionType::Hash, priority: 3 },
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

    let mut sorted_indexes: Vec<_> = DEFERRABLE_INDEXES.iter().collect();
    sorted_indexes.sort_by_key(|idx| idx.priority);

    let mut rate_calc = RateCalculator::default();

    for idx in sorted_indexes {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled, stopping");
            return Ok(());
        }

        result.current_index = Some(idx.name.to_string());
        db.update_result(task_id, &serde_json::to_value(&result)?)
            .await?;

        let msg = format!("Building index: {}", idx.name);
        db.append_log(task_id, &msg).await?;

        let start = Instant::now();
        let build_result = match idx.partition_type {
            PartitionType::Range => {
                rebuild_partitioned_index(
                    pool,
                    idx,
                    RANGE_PARTITION_SUFFIXES,
                    config.parallel_connections,
                )
                .await
            }
            PartitionType::Hash => {
                rebuild_partitioned_index(
                    pool,
                    idx,
                    HASH_PARTITION_SUFFIXES,
                    config.parallel_connections,
                )
                .await
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        match build_result {
            Ok(_) => {
                result.completed_indexes += 1;
                result.completed.push(IndexCompletionInfo {
                    name: idx.name.to_string(),
                    duration_ms,
                });
                info!("Built index {} in {}ms", idx.name, duration_ms);
            }
            Err(e) => {
                result.failed.push(IndexFailureInfo {
                    name: idx.name.to_string(),
                    error: e.to_string(),
                });
                warn!("Failed to build index {}: {}", idx.name, e);
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

    sqlx::query("UPDATE sync_status SET indexes_deferred = false, indexes_dropped_at = NULL")
        .execute(pool)
        .await?;
    info!("Cleared indexes_deferred flag in sync_status");

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    Ok(())
}

const MAX_RETRIES: usize = 3;
const RETRY_DELAY_MS: u64 = 5000;

struct IndexBuildTask {
    index_name: String,
    sql: String,
    attempt: usize,
}

async fn rebuild_partitioned_index(
    pool: &PgPool,
    idx: &DeferrableIndex,
    suffixes: &[&str],
    max_parallel: usize,
) -> Result<()> {
    let effective_parallel = max_parallel.min(4);

    let mut join_set: JoinSet<std::result::Result<String, (IndexBuildTask, String)>> =
        JoinSet::new();
    let mut pending: Vec<IndexBuildTask> = suffixes
        .iter()
        .map(|suffix| {
            let table = format!("{}{}", idx.table, suffix);
            let base_name = &idx.name[4..];
            let index_name = format!("{}_{}{}", idx.table, base_name, suffix);
            let definition = idx
                .definition
                .replace("{name}", &index_name)
                .replace("{table}", &table);
            let sql = definition.replace("CREATE INDEX", "CREATE INDEX CONCURRENTLY IF NOT EXISTS");
            IndexBuildTask {
                index_name,
                sql,
                attempt: 1,
            }
        })
        .collect();
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

async fn rebuild_partitioned_constraint(
    pool: &PgPool,
    constraint: &DeferrableConstraint,
    max_parallel: usize,
) -> Result<()> {
    let mut join_set: JoinSet<Result<bool>> = JoinSet::new();
    let mut pending: Vec<&str> = RANGE_PARTITION_SUFFIXES.to_vec();

    while !pending.is_empty() || !join_set.is_empty() {
        while join_set.len() < max_parallel && !pending.is_empty() {
            let suffix = pending.remove(0);
            let pool = pool.clone();
            let table_name = format!("{}{}", constraint.table, suffix);
            let constraint_name = format!("{}_{}", table_name, constraint.name);
            let columns = constraint.columns.to_string();

            join_set.spawn(async move {
                let check_sql = format!(
                    "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
                    constraint_name, table_name
                );
                let exists: Option<(i32,)> = sqlx::query_as(&check_sql).fetch_optional(&pool).await?;

                if exists.is_none() {
                    let add_sql = format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({})",
                        table_name, constraint_name, columns
                    );
                    sqlx::query(&add_sql).execute(&pool).await?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            });
        }

        if let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => warn!("Failed to add constraint: {}", e),
                Err(e) => warn!("Task panicked: {}", e),
            }
        }
    }

    Ok(())
}
