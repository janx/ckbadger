use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::time::Instant;
use tokio::task::JoinSet;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexRebuildProgress {
    pub total: usize,
    pub completed: usize,
    pub current: Option<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone)]
struct DeferrableIndex {
    name: &'static str,
    table: &'static str,
    definition: &'static str,
    is_partitioned: bool,
    priority: u8,
}

const PARTITION_SUFFIXES: &[&str] = &[
    "_p00", "_p01", "_p02", "_p03", "_p04", "_p05", "_p06", "_p07", "_p08", "_p09",
];

const DEFERRABLE_INDEXES: &[DeferrableIndex] = &[
    DeferrableIndex {
        name: "idx_blocks_hash",
        table: "blocks",
        definition: "CREATE INDEX {name} ON {table}(hash)",
        is_partitioned: true,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_blocks_epoch",
        table: "blocks",
        definition: "CREATE INDEX {name} ON {table}(epoch_number)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_blocks_miner",
        table: "blocks",
        definition: "CREATE INDEX {name} ON {table}(miner_lock_hash) WHERE miner_lock_hash IS NOT NULL",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_tx_hash",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(hash)",
        is_partitioned: true,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_tx_timestamp",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(timestamp DESC)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_tx_short_hash",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(short_hash, block_number)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_tx_cursor",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_tx_list_covering",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC) INCLUDE (hash, inputs_count, outputs_count, fee, is_cellbase, timestamp)",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_outpoint",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(tx_hash, output_index)",
        is_partitioned: true,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_cells_lock_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) WHERE status = 0",
        is_partitioned: true,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_cells_lock_script_details",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash) INCLUDE (lock_code_hash, lock_hash_type, lock_args)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_type_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_script_hash, created_at_block DESC) WHERE status = 0 AND type_script_hash IS NOT NULL",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_consumed_by",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(consumed_by_tx) WHERE consumed_by_tx IS NOT NULL",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_type_script_hash",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_lock_code_hash",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_code_hash)",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_type_code_hash",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_lock_code_hash_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_code_hash, lock_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_type_code_hash_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_code_hash, type_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0 AND type_code_hash IS NOT NULL",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_list_covering",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) INCLUDE (tx_hash, output_index, capacity, type_script_hash, data_size) WHERE status = 0",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_inputs_previous",
        table: "transaction_inputs",
        definition: "CREATE INDEX {name} ON {table}(previous_tx_hash, previous_output_index)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_inputs_tx",
        table: "transaction_inputs",
        definition: "CREATE INDEX {name} ON {table}(tx_hash)",
        is_partitioned: true,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cell_deps_tx",
        table: "transaction_cell_deps",
        definition: "CREATE INDEX {name} ON {table}(tx_hash)",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_uncles_hash",
        table: "uncle_blocks",
        definition: "CREATE INDEX {name} ON {table}(hash)",
        is_partitioned: true,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_live_cells_lock",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash)",
        is_partitioned: false,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_live_cells_lock_code",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(lock_code_hash)",
        is_partitioned: false,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_live_cells_type",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL",
        is_partitioned: false,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_live_cells_type_code",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL",
        is_partitioned: false,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_live_cells_block",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(created_at_block)",
        is_partitioned: false,
        priority: 3,
    },
];

pub struct IndexManager {
    pool: PgPool,
}

impl IndexManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn is_indexes_deferred(&self) -> Result<bool> {
        let row: (bool,) = sqlx::query_as("SELECT indexes_deferred FROM sync_status WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    pub async fn drop_deferrable_indexes(&self) -> Result<usize> {
        info!("Dropping deferrable indexes for bulk sync optimization...");
        let start = Instant::now();
        let mut dropped_count = 0;

        for idx in DEFERRABLE_INDEXES {
            if idx.is_partitioned {
                for suffix in PARTITION_SUFFIXES {
                    let index_name = format!("{}_{}", idx.table, &idx.name[4..]);
                    let partition_index = format!("{}{}", index_name, suffix);
                    if self.drop_index_if_exists(&partition_index).await? {
                        dropped_count += 1;
                    }
                }
                if self.drop_index_if_exists(idx.name).await? {
                    dropped_count += 1;
                }
            } else if self.drop_index_if_exists(idx.name).await? {
                dropped_count += 1;
            }
        }

        sqlx::query(
            "UPDATE sync_status SET indexes_deferred = TRUE, indexes_dropped_at = NOW() WHERE id = 1",
        )
        .execute(&self.pool)
        .await?;

        info!("Dropped {} indexes in {:?}", dropped_count, start.elapsed());
        Ok(dropped_count)
    }

    async fn drop_index_if_exists(&self, name: &str) -> Result<bool> {
        let sql = format!("DROP INDEX IF EXISTS {}", name);
        let result = sqlx::query(&sql).execute(&self.pool).await;
        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("Failed to drop index {}: {}", name, e);
                Ok(false)
            }
        }
    }

    pub async fn rebuild_indexes_parallel(
        &self,
        max_parallel: usize,
    ) -> Result<IndexRebuildProgress> {
        info!(
            "Starting parallel index rebuild with max {} concurrent tasks",
            max_parallel
        );
        let start = Instant::now();

        sqlx::query(
            "UPDATE sync_status SET indexes_rebuild_started_at = NOW(), indexes_rebuild_progress = $1 WHERE id = 1",
        )
        .bind(serde_json::to_string(&IndexRebuildProgress {
            total: DEFERRABLE_INDEXES.len(),
            completed: 0,
            current: None,
            failed: vec![],
        })?)
        .execute(&self.pool)
        .await?;

        let mut progress = IndexRebuildProgress {
            total: DEFERRABLE_INDEXES.len(),
            completed: 0,
            current: None,
            failed: vec![],
        };

        let mut sorted_indexes: Vec<_> = DEFERRABLE_INDEXES.iter().collect();
        sorted_indexes.sort_by_key(|idx| idx.priority);

        for idx in sorted_indexes {
            progress.current = Some(idx.name.to_string());
            self.update_progress(&progress).await?;

            let result = if idx.is_partitioned {
                self.rebuild_partitioned_index_parallel(idx, max_parallel)
                    .await
            } else {
                self.rebuild_single_index(idx).await
            };

            match result {
                Ok(_) => {
                    progress.completed += 1;
                    info!(
                        "Rebuilt index {} ({}/{})",
                        idx.name, progress.completed, progress.total
                    );
                }
                Err(e) => {
                    warn!("Failed to rebuild index {}: {}", idx.name, e);
                    progress.failed.push(idx.name.to_string());
                }
            }

            self.update_progress(&progress).await?;
        }

        progress.current = None;
        self.update_progress(&progress).await?;

        sqlx::query(
            "UPDATE sync_status SET indexes_deferred = FALSE, indexes_rebuild_completed_at = NOW() WHERE id = 1",
        )
        .execute(&self.pool)
        .await?;

        info!(
            "Index rebuild completed in {:?}. {}/{} succeeded, {} failed",
            start.elapsed(),
            progress.completed,
            progress.total,
            progress.failed.len()
        );

        Ok(progress)
    }

    async fn rebuild_partitioned_index_parallel(
        &self,
        idx: &DeferrableIndex,
        max_parallel: usize,
    ) -> Result<()> {
        info!(
            "Rebuilding partitioned index {} on {} partitions",
            idx.name,
            PARTITION_SUFFIXES.len()
        );
        let start = Instant::now();

        let mut join_set: JoinSet<Result<String>> = JoinSet::new();
        let mut pending_partitions: Vec<&str> = PARTITION_SUFFIXES.to_vec();

        while !pending_partitions.is_empty() || !join_set.is_empty() {
            while join_set.len() < max_parallel && !pending_partitions.is_empty() {
                let suffix = pending_partitions.remove(0);
                let pool = self.pool.clone();
                let table = format!("{}{}", idx.table, suffix);
                let base_name = &idx.name[4..];
                let index_name = format!("{}_{}{}", idx.table, base_name, suffix);
                let definition = idx
                    .definition
                    .replace("{name}", &index_name)
                    .replace("{table}", &table);
                let sql =
                    definition.replace("CREATE INDEX", "CREATE INDEX CONCURRENTLY IF NOT EXISTS");

                join_set.spawn(async move {
                    sqlx::query(&sql).execute(&pool).await?;
                    Ok(index_name)
                });
            }

            if let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok(name)) => {
                        info!("  Created partition index: {}", name);
                    }
                    Ok(Err(e)) => {
                        warn!("  Failed to create partition index: {}", e);
                    }
                    Err(e) => {
                        warn!("  Task panicked: {}", e);
                    }
                }
            }
        }

        info!(
            "Partitioned index {} rebuilt in {:?}",
            idx.name,
            start.elapsed()
        );
        Ok(())
    }

    async fn rebuild_single_index(&self, idx: &DeferrableIndex) -> Result<()> {
        let sql = idx
            .definition
            .replace("{name}", idx.name)
            .replace("{table}", idx.table);
        let sql = sql.replace("CREATE INDEX", "CREATE INDEX CONCURRENTLY IF NOT EXISTS");

        sqlx::query(&sql).execute(&self.pool).await?;
        Ok(())
    }

    async fn update_progress(&self, progress: &IndexRebuildProgress) -> Result<()> {
        sqlx::query("UPDATE sync_status SET indexes_rebuild_progress = $1 WHERE id = 1")
            .bind(serde_json::to_string(progress)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn check_indexes_exist(&self) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pg_indexes WHERE indexname = 'idx_tx_hash' OR indexname = 'transactions_hash_idx'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    pub async fn get_rebuild_progress(&self) -> Result<Option<IndexRebuildProgress>> {
        let row: (Option<String>,) =
            sqlx::query_as("SELECT indexes_rebuild_progress FROM sync_status WHERE id = 1")
                .fetch_one(&self.pool)
                .await?;

        match row.0 {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deferrable_indexes_count() {
        assert!(DEFERRABLE_INDEXES.len() >= 20);
    }

    #[test]
    fn test_partition_suffixes() {
        assert_eq!(PARTITION_SUFFIXES.len(), 10);
    }

    #[test]
    fn test_index_definition_format() {
        for idx in DEFERRABLE_INDEXES {
            assert!(
                idx.definition.contains("{name}"),
                "Missing {{name}} in {}",
                idx.name
            );
            assert!(
                idx.definition.contains("{table}"),
                "Missing {{table}} in {}",
                idx.name
            );
        }
    }

    #[test]
    fn test_index_names_start_with_idx() {
        for idx in DEFERRABLE_INDEXES {
            assert!(
                idx.name.starts_with("idx_"),
                "Index {} should start with idx_",
                idx.name
            );
        }
    }

    #[test]
    fn test_priority_values_valid() {
        for idx in DEFERRABLE_INDEXES {
            assert!(
                idx.priority >= 1 && idx.priority <= 3,
                "Index {} has invalid priority {}",
                idx.name,
                idx.priority
            );
        }
    }

    #[test]
    fn test_partitioned_indexes_have_correct_tables() {
        let partitioned_tables = [
            "blocks",
            "transactions",
            "cells",
            "transaction_inputs",
            "transaction_cell_deps",
            "uncle_blocks",
            "block_proposals",
        ];

        for idx in DEFERRABLE_INDEXES {
            if idx.is_partitioned {
                assert!(
                    partitioned_tables.contains(&idx.table),
                    "Index {} marked as partitioned but table {} is not partitioned",
                    idx.name,
                    idx.table
                );
            }
        }
    }

    #[test]
    fn test_non_partitioned_indexes() {
        let non_partitioned: Vec<_> = DEFERRABLE_INDEXES
            .iter()
            .filter(|idx| !idx.is_partitioned)
            .collect();

        assert!(
            !non_partitioned.is_empty(),
            "Should have some non-partitioned indexes"
        );
        for idx in non_partitioned {
            assert_eq!(
                idx.table, "live_cells",
                "Non-partitioned index {} should be on live_cells",
                idx.name
            );
        }
    }

    #[test]
    fn test_sql_replacement() {
        let idx = &DEFERRABLE_INDEXES[0];
        let sql = idx
            .definition
            .replace("{name}", idx.name)
            .replace("{table}", idx.table);

        assert!(!sql.contains("{name}"));
        assert!(!sql.contains("{table}"));
        assert!(sql.contains("CREATE INDEX"));
    }

    #[test]
    fn test_concurrently_replacement() {
        let sql = "CREATE INDEX idx_test ON table(col)";
        let result = sql.replace("CREATE INDEX", "CREATE INDEX CONCURRENTLY IF NOT EXISTS");
        assert_eq!(
            result,
            "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_test ON table(col)"
        );
    }

    #[test]
    fn test_partition_index_name_generation() {
        let idx = DeferrableIndex {
            name: "idx_cells_lock_live",
            table: "cells",
            definition: "CREATE INDEX {name} ON {table}(lock_script_hash)",
            is_partitioned: true,
            priority: 1,
        };

        let suffix = "_p00";
        let base_name = &idx.name[4..];
        let index_name = format!("{}_{}{}", idx.table, base_name, suffix);

        assert_eq!(index_name, "cells_cells_lock_live_p00");
    }

    #[test]
    fn test_index_rebuild_progress_serialization() {
        let progress = IndexRebuildProgress {
            total: 20,
            completed: 5,
            current: Some("idx_cells_lock".to_string()),
            failed: vec!["idx_test".to_string()],
        };

        let json = serde_json::to_string(&progress).unwrap();
        let parsed: IndexRebuildProgress = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.total, 20);
        assert_eq!(parsed.completed, 5);
        assert_eq!(parsed.current, Some("idx_cells_lock".to_string()));
        assert_eq!(parsed.failed, vec!["idx_test".to_string()]);
    }
}
