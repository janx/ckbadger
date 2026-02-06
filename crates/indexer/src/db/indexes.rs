use anyhow::Result;
use sqlx::PgPool;
use std::time::Instant;
use tracing::{info, warn};

/// Partition scheme type for tables
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartitionType {
    /// Not partitioned - create index directly on table
    #[allow(dead_code)]
    None,
    /// Range partitioned by block number (10 partitions: _p00 to _p09)
    Range,
    /// Hash partitioned (16 partitions: _p00 to _p15)
    Hash,
}

#[derive(Debug, Clone)]
struct DeferrableIndex {
    name: &'static str,
    table: &'static str,
    #[allow(dead_code)]
    definition: &'static str,
    /// Partition type determines which suffixes to use
    partition_type: PartitionType,
    #[allow(dead_code)]
    priority: u8,
}

/// Represents a UNIQUE CONSTRAINT that can be dropped during bulk sync.
/// Unlike indexes, constraints require ALTER TABLE to drop/add.
#[derive(Debug, Clone)]
struct DeferrableConstraint {
    /// Base constraint name (without partition suffix)
    name: &'static str,
    /// Base table name (without partition suffix)
    table: &'static str,
    #[allow(dead_code)]
    columns: &'static str,
    #[allow(dead_code)]
    is_partitioned: bool,
}

const RANGE_PARTITION_SUFFIXES: &[&str] = &[
    "_p00", "_p01", "_p02", "_p03", "_p04", "_p05", "_p06", "_p07", "_p08", "_p09",
];

const HASH_PARTITION_SUFFIXES: &[&str] = &[
    "_p00", "_p01", "_p02", "_p03", "_p04", "_p05", "_p06", "_p07", "_p08", "_p09", "_p10", "_p11",
    "_p12", "_p13", "_p14", "_p15",
];

/// UNIQUE constraints that are safe to drop during bulk sync.
/// These constraints are validated by CKB node, so they're redundant
/// when syncing from a trusted local node.
const DEFERRABLE_CONSTRAINTS: &[DeferrableConstraint] = &[
    // cells: (created_at_block, tx_hash, output_index) - CKB guarantees cell uniqueness
    DeferrableConstraint {
        name: "created_at_block_tx_hash_output_index_key",
        table: "cells",
        columns: "created_at_block, tx_hash, output_index",
        is_partitioned: true,
    },
    // transaction_inputs: (tx_block_number, tx_hash, input_index) - sequential indices
    DeferrableConstraint {
        name: "tx_block_number_tx_hash_input_index_key",
        table: "transaction_inputs",
        columns: "tx_block_number, tx_hash, input_index",
        is_partitioned: true,
    },
    // transaction_cell_deps: (tx_block_number, tx_hash, dep_index) - sequential indices
    DeferrableConstraint {
        name: "tx_block_number_tx_hash_dep_index_key",
        table: "transaction_cell_deps",
        columns: "tx_block_number, tx_hash, dep_index",
        is_partitioned: true,
    },
    // block_proposals: (block_number, proposal_index) - sequential indices
    DeferrableConstraint {
        name: "block_number_proposal_index_key",
        table: "block_proposals",
        columns: "block_number, proposal_index",
        is_partitioned: true,
    },
    // uncle_blocks: (block_number, uncle_index) - sequential indices
    DeferrableConstraint {
        name: "block_number_uncle_index_key",
        table: "uncle_blocks",
        columns: "block_number, uncle_index",
        is_partitioned: true,
    },
];

const DEFERRABLE_INDEXES: &[DeferrableIndex] = &[
    DeferrableIndex {
        name: "idx_blocks_hash",
        table: "blocks",
        definition: "CREATE INDEX {name} ON {table}(hash)",
        partition_type: PartitionType::Range,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_blocks_epoch",
        table: "blocks",
        definition: "CREATE INDEX {name} ON {table}(epoch_number)",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_blocks_miner",
        table: "blocks",
        definition: "CREATE INDEX {name} ON {table}(miner_lock_hash) WHERE miner_lock_hash IS NOT NULL",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_tx_timestamp",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(timestamp DESC)",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_tx_short_hash",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(short_hash, block_number)",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_tx_cursor",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC)",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_tx_list_covering",
        table: "transactions",
        definition: "CREATE INDEX {name} ON {table}(block_number DESC, tx_index DESC) INCLUDE (hash, inputs_count, outputs_count, fee, is_cellbase, timestamp)",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_lock_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) WHERE status = 0",
        partition_type: PartitionType::Range,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_cells_lock_script_details",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash) INCLUDE (lock_code_hash, lock_hash_type, lock_args)",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_type_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_script_hash, created_at_block DESC) WHERE status = 0 AND type_script_hash IS NOT NULL",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_type_script_hash",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL",
        partition_type: PartitionType::Range,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_cells_lock_code_hash",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_code_hash)",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_type_code_hash",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_lock_code_hash_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_code_hash, lock_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_type_code_hash_live",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(type_code_hash, type_hash_type, created_at_block DESC, output_index DESC) WHERE status = 0 AND type_code_hash IS NOT NULL",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_cells_list_covering",
        table: "cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash, created_at_block DESC) INCLUDE (tx_hash, output_index, capacity, type_script_hash, data_size) WHERE status = 0",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_uncles_hash",
        table: "uncle_blocks",
        definition: "CREATE INDEX {name} ON {table}(hash)",
        partition_type: PartitionType::Range,
        priority: 3,
    },
    DeferrableIndex {
        name: "idx_live_cells_lock",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(lock_script_hash)",
        partition_type: PartitionType::Hash,
        priority: 1,
    },
    DeferrableIndex {
        name: "idx_live_cells_lock_code",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(lock_code_hash)",
        partition_type: PartitionType::Hash,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_live_cells_type",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(type_script_hash) WHERE type_script_hash IS NOT NULL",
        partition_type: PartitionType::Hash,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_live_cells_type_code",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(type_code_hash) WHERE type_code_hash IS NOT NULL",
        partition_type: PartitionType::Hash,
        priority: 2,
    },
    DeferrableIndex {
        name: "idx_live_cells_block",
        table: "live_cells",
        definition: "CREATE INDEX {name} ON {table}(created_at_block)",
        partition_type: PartitionType::Hash,
        priority: 3,
    },
];

use crate::cache::CacheInvalidator;

pub struct IndexManager {
    pool: PgPool,
    cache_invalidator: Option<CacheInvalidator>,
}

impl IndexManager {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(pool: PgPool, cache_invalidator: CacheInvalidator) -> Self {
        Self {
            pool,
            cache_invalidator: Some(cache_invalidator),
        }
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
            let suffixes = Self::get_partition_suffixes(idx.partition_type);
            if let Some(suffixes) = suffixes {
                for suffix in suffixes {
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

        if let Some(cache) = &self.cache_invalidator {
            cache
                .update_sync_status(|status| {
                    status.set_indexes_deferred(true);
                })
                .await;
        }

        info!("Dropped {} indexes in {:?}", dropped_count, start.elapsed());
        Ok(dropped_count)
    }

    fn get_partition_suffixes(partition_type: PartitionType) -> Option<&'static [&'static str]> {
        match partition_type {
            PartitionType::None => None,
            PartitionType::Range => Some(RANGE_PARTITION_SUFFIXES),
            PartitionType::Hash => Some(HASH_PARTITION_SUFFIXES),
        }
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

    pub async fn check_indexes_exist(&self) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pg_indexes WHERE indexname = 'idx_tx_hash' OR indexname = 'transactions_hash_idx'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    pub async fn drop_deferrable_constraints(&self) -> Result<usize> {
        info!("Dropping deferrable UNIQUE constraints for bulk sync optimization...");
        let start = Instant::now();
        let mut dropped_count = 0;

        for constraint in DEFERRABLE_CONSTRAINTS {
            // Drop on parent table - PostgreSQL propagates to all partitions
            let constraint_name = format!("{}_{}", constraint.table, constraint.name);
            if self
                .drop_constraint_if_exists(constraint.table, &constraint_name)
                .await?
            {
                dropped_count += 1;
            }
        }

        info!(
            "Dropped {} UNIQUE constraints in {:?}",
            dropped_count,
            start.elapsed()
        );
        Ok(dropped_count)
    }

    async fn drop_constraint_if_exists(&self, table: &str, constraint_name: &str) -> Result<bool> {
        let check_sql = format!(
            "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
            constraint_name, table
        );

        let exists: Option<(i32,)> = sqlx::query_as(&check_sql)
            .fetch_optional(&self.pool)
            .await?;

        if exists.is_some() {
            let drop_sql = format!(
                "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
                table, constraint_name
            );
            match sqlx::query(&drop_sql).execute(&self.pool).await {
                Ok(_) => {
                    info!("Dropped constraint: {}.{}", table, constraint_name);
                    Ok(true)
                }
                Err(e) => {
                    warn!("Failed to drop constraint {}: {}", constraint_name, e);
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }

    pub async fn check_constraints_exist(&self) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pg_constraint WHERE conname LIKE 'cells_p%_created_at_block_tx_hash_output_index_key'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
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
    fn test_range_partition_suffixes() {
        assert_eq!(RANGE_PARTITION_SUFFIXES.len(), 10);
        for (i, suffix) in RANGE_PARTITION_SUFFIXES.iter().enumerate() {
            assert_eq!(*suffix, format!("_p{:02}", i));
        }
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
        let range_partitioned_tables = [
            "blocks",
            "transactions",
            "cells",
            "transaction_inputs",
            "transaction_cell_deps",
            "uncle_blocks",
            "block_proposals",
        ];

        for idx in DEFERRABLE_INDEXES {
            if idx.partition_type == PartitionType::Range {
                assert!(
                    range_partitioned_tables.contains(&idx.table),
                    "Index {} marked as Range partitioned but table {} is not range partitioned",
                    idx.name,
                    idx.table
                );
            }
        }
    }

    #[test]
    fn test_hash_partitioned_indexes() {
        let hash_partitioned: Vec<_> = DEFERRABLE_INDEXES
            .iter()
            .filter(|idx| idx.partition_type == PartitionType::Hash)
            .collect();

        assert!(
            !hash_partitioned.is_empty(),
            "Should have some hash-partitioned indexes"
        );
        for idx in hash_partitioned {
            assert_eq!(
                idx.table, "live_cells",
                "Hash-partitioned index {} should be on live_cells",
                idx.name
            );
        }
    }

    #[test]
    fn test_partition_suffix_counts() {
        assert_eq!(RANGE_PARTITION_SUFFIXES.len(), 10);
        assert_eq!(HASH_PARTITION_SUFFIXES.len(), 16);
    }

    #[test]
    fn test_live_cells_indexes_are_hash_partitioned() {
        for idx in DEFERRABLE_INDEXES {
            if idx.table == "live_cells" {
                assert_eq!(
                    idx.partition_type,
                    PartitionType::Hash,
                    "live_cells index {} should be Hash partitioned",
                    idx.name
                );
            }
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
            partition_type: PartitionType::Range,
            priority: 1,
        };

        let suffix = "_p00";
        let base_name = &idx.name[4..];
        let index_name = format!("{}_{}{}", idx.table, base_name, suffix);

        assert_eq!(index_name, "cells_cells_lock_live_p00");
    }

    #[test]
    fn test_deferrable_constraints_count() {
        assert_eq!(DEFERRABLE_CONSTRAINTS.len(), 5);
    }

    #[test]
    fn test_deferrable_constraints_are_partitioned() {
        for constraint in DEFERRABLE_CONSTRAINTS {
            assert!(
                constraint.is_partitioned,
                "Constraint {} should be partitioned",
                constraint.name
            );
        }
    }

    #[test]
    fn test_deferrable_constraints_have_valid_tables() {
        let partitioned_tables = [
            "cells",
            "transaction_inputs",
            "transaction_cell_deps",
            "block_proposals",
            "uncle_blocks",
        ];

        for constraint in DEFERRABLE_CONSTRAINTS {
            assert!(
                partitioned_tables.contains(&constraint.table),
                "Constraint {} has invalid table {}",
                constraint.name,
                constraint.table
            );
        }
    }

    #[test]
    fn test_deferrable_constraints_name_ends_with_key() {
        for constraint in DEFERRABLE_CONSTRAINTS {
            assert!(
                constraint.name.ends_with("_key"),
                "Constraint name {} should end with _key",
                constraint.name
            );
        }
    }

    #[test]
    fn test_constraint_name_generation_for_partition() {
        let constraint = &DEFERRABLE_CONSTRAINTS[0];
        let suffix = "_p00";
        let table_name = format!("{}{}", constraint.table, suffix);
        let constraint_name = format!("{}_{}", table_name, constraint.name);

        assert_eq!(
            constraint_name,
            "cells_p00_created_at_block_tx_hash_output_index_key"
        );
    }

    #[test]
    fn test_constraint_columns_not_empty() {
        for constraint in DEFERRABLE_CONSTRAINTS {
            assert!(
                !constraint.columns.is_empty(),
                "Constraint {} has empty columns",
                constraint.name
            );
            assert!(
                constraint.columns.contains(',') || constraint.columns.contains('_'),
                "Constraint {} should have multiple columns or underscore",
                constraint.name
            );
        }
    }

    #[test]
    fn test_constraint_drop_uses_parent_table_name() {
        // DROP must target parent table; partition tables fail with "cannot drop inherited constraint"
        for constraint in DEFERRABLE_CONSTRAINTS {
            assert!(
                !constraint.table.contains("_p0"),
                "Constraint table '{}' should be parent table, not partition",
                constraint.table
            );
        }
    }
}
