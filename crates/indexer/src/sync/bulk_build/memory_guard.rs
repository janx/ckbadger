use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use crate::runtime_diag::{
    read_process_memory_snapshot, ChainStoreMemorySnapshot, ProcessMemorySnapshot,
};

const GIB: u64 = 1024 * 1024 * 1024;
const MIN_SAFE_BATCH_INPUT_BYTES: u64 = 1_000_000;
const BUILD_TRANSIENT_MULTIPLIER: u64 = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) struct BulkMemoryGuard {
    limit_bytes: u64,
}

impl BulkMemoryGuard {
    pub(crate) fn new(configured_gb: Option<u64>, automatic_bytes: u64) -> Result<Self> {
        let limit_bytes = match configured_gb {
            Some(0) => bail!("bulk process memory budget must be greater than zero"),
            Some(gb) => gb.checked_mul(GIB).ok_or_else(|| {
                anyhow!(
                    "bulk process memory budget overflows bytes: configured_gb={}",
                    gb
                )
            })?,
            None if automatic_bytes == 0 => {
                bail!("automatic bulk process memory budget resolved to zero bytes")
            }
            None => automatic_bytes,
        };
        Ok(Self { limit_bytes })
    }

    pub(crate) fn limit_bytes(self) -> u64 {
        self.limit_bytes
    }

    pub(crate) fn checkpoint(
        self,
        phase: &str,
        block_number: u64,
        retained_component_bytes: &HashMap<String, u64>,
        store_memory: &ChainStoreMemorySnapshot,
    ) -> Result<ProcessMemorySnapshot> {
        let snapshot = read_process_memory_snapshot()?;
        self.check_snapshot(
            snapshot,
            phase,
            block_number,
            retained_component_bytes,
            store_memory,
        )?;
        Ok(snapshot)
    }

    pub(crate) fn safe_batch_input_bytes(
        self,
        snapshot: ProcessMemorySnapshot,
        configured_max_batch_bytes: u64,
        block_number: u64,
        retained_component_bytes: &HashMap<String, u64>,
        store_memory: &ChainStoreMemorySnapshot,
    ) -> Result<u64> {
        self.check_snapshot(
            snapshot,
            "before_batch",
            block_number,
            retained_component_bytes,
            store_memory,
        )?;
        let committed = snapshot.committed_bytes()?;
        let headroom = self.limit_bytes - committed;
        let safe_input = headroom / BUILD_TRANSIENT_MULTIPLIER;
        if safe_input < MIN_SAFE_BATCH_INPUT_BYTES {
            bail!(
                "bulk process memory headroom cannot safely build another batch: block={} committed_bytes={} limit_bytes={} headroom_bytes={} required_min_input_bytes={} transient_multiplier={} process_memory={} rocksdb_memory={} retained_components={}",
                block_number,
                committed,
                self.limit_bytes,
                headroom,
                MIN_SAFE_BATCH_INPUT_BYTES,
                BUILD_TRANSIENT_MULTIPLIER,
                format_process_memory(snapshot),
                format_store_memory(store_memory),
                format_retained_components(retained_component_bytes)
            );
        }
        Ok(configured_max_batch_bytes.min(safe_input))
    }

    fn check_snapshot(
        self,
        snapshot: ProcessMemorySnapshot,
        phase: &str,
        block_number: u64,
        retained_component_bytes: &HashMap<String, u64>,
        store_memory: &ChainStoreMemorySnapshot,
    ) -> Result<()> {
        let committed = snapshot.committed_bytes()?;
        if committed > self.limit_bytes {
            bail!(
                "bulk process memory budget exceeded: phase={} block={} committed_bytes={} limit_bytes={} process_memory={} rocksdb_memory={} retained_components={}",
                phase,
                block_number,
                committed,
                self.limit_bytes,
                format_process_memory(snapshot),
                format_store_memory(store_memory),
                format_retained_components(retained_component_bytes)
            );
        }
        Ok(())
    }
}

fn format_process_memory(snapshot: ProcessMemorySnapshot) -> String {
    format!(
        "rss_bytes:{},rss_anon_bytes:{},rss_file_bytes:{},rss_shmem_bytes:{},swap_bytes:{},high_water_rss_bytes:{},jemalloc_stats_available:{},jemalloc_allocated_bytes:{},jemalloc_active_bytes:{},jemalloc_resident_bytes:{},jemalloc_mapped_bytes:{},jemalloc_retained_bytes:{},jemalloc_metadata_bytes:{}",
        snapshot.rss_bytes,
        snapshot.rss_anon_bytes,
        snapshot.rss_file_bytes,
        snapshot.rss_shmem_bytes,
        snapshot.swap_bytes,
        snapshot.high_water_rss_bytes,
        snapshot.jemalloc_stats_available,
        snapshot.jemalloc_allocated_bytes,
        snapshot.jemalloc_active_bytes,
        snapshot.jemalloc_resident_bytes,
        snapshot.jemalloc_mapped_bytes,
        snapshot.jemalloc_retained_bytes,
        snapshot.jemalloc_metadata_bytes,
    )
}

fn format_store_memory(snapshot: &ChainStoreMemorySnapshot) -> String {
    format!(
        "domain_memtable_bytes:{},append_only_memtable_bytes:{},shared_block_cache_bytes:{},domain_table_readers_bytes:{},append_only_table_readers_bytes:{},total_memory_bytes:{},domain_compaction_pending_bytes:{},append_only_compaction_pending_bytes:{},shared_wbm_usage_bytes:{},shared_wbm_budget_bytes:{}",
        snapshot.domain_memtable_bytes,
        snapshot.append_only_memtable_bytes,
        snapshot.shared_block_cache_bytes,
        snapshot.domain_table_readers_bytes,
        snapshot.append_only_table_readers_bytes,
        snapshot.total_memory_bytes,
        snapshot.domain_compaction_pending_bytes,
        snapshot.append_only_compaction_pending_bytes,
        snapshot.shared_wbm_usage_bytes,
        snapshot.shared_wbm_budget_bytes,
    )
}

fn format_retained_components(retained_component_bytes: &HashMap<String, u64>) -> String {
    let mut entries = retained_component_bytes.iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .map(|(name, bytes)| format!("{}:{}", name, bytes))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(rss: u64, swap: u64) -> ProcessMemorySnapshot {
        ProcessMemorySnapshot {
            rss_bytes: rss,
            rss_anon_bytes: rss,
            rss_file_bytes: 0,
            rss_shmem_bytes: 0,
            swap_bytes: swap,
            high_water_rss_bytes: rss,
            ..Default::default()
        }
    }

    fn store_memory() -> ChainStoreMemorySnapshot {
        ChainStoreMemorySnapshot {
            domain_memtable_bytes: 101,
            append_only_memtable_bytes: 202,
            shared_block_cache_bytes: 303,
            domain_table_readers_bytes: 404,
            append_only_table_readers_bytes: 505,
            total_memory_bytes: 1_515,
            domain_compaction_pending_bytes: 606,
            append_only_compaction_pending_bytes: 707,
            shared_wbm_usage_bytes: 808,
            shared_wbm_budget_bytes: 909,
            ..Default::default()
        }
    }

    #[test]
    fn explicit_process_budget_overrides_automatic_share() {
        let guard = BulkMemoryGuard::new(Some(8), 32 * GIB).unwrap();
        assert_eq!(guard.limit_bytes(), 8 * GIB);
        assert_eq!(
            BulkMemoryGuard::new(None, 32 * GIB).unwrap().limit_bytes(),
            32 * GIB
        );
    }

    #[test]
    fn batch_input_cap_shrinks_to_preserve_transient_headroom() {
        let guard = BulkMemoryGuard::new(Some(8), 32 * GIB).unwrap();
        let committed = 6 * GIB;
        let cap = guard
            .safe_batch_input_bytes(
                snapshot(committed, 0),
                2 * GIB,
                100,
                &HashMap::new(),
                &store_memory(),
            )
            .unwrap();
        assert_eq!(cap, (2 * GIB) / BUILD_TRANSIENT_MULTIPLIER);
    }

    #[test]
    fn budget_failure_reports_actual_rss_swap_and_owner_breakdown() {
        let guard = BulkMemoryGuard::new(Some(8), 32 * GIB).unwrap();
        let owners = HashMap::from([("live_cells".to_string(), 123)]);
        let error = guard
            .check_snapshot(
                snapshot(7 * GIB, 2 * GIB),
                "after_batch",
                42,
                &owners,
                &store_memory(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("memory budget exceeded"), "{error}");
        assert!(error.contains("block=42"), "{error}");
        assert!(error.contains("swap_bytes:2147483648"), "{error}");
        assert!(error.contains("domain_memtable_bytes:101"), "{error}");
        assert!(error.contains("append_only_memtable_bytes:202"), "{error}");
        assert!(error.contains("live_cells:123"), "{error}");
    }

    #[test]
    fn headroom_failure_reports_process_allocator_and_both_store_breakdowns() {
        let guard = BulkMemoryGuard::new(Some(8), 32 * GIB).unwrap();
        let mut process = snapshot(8 * GIB - 2 * 1024 * 1024, 0);
        process.rss_file_bytes = 11;
        process.rss_shmem_bytes = 12;
        process.jemalloc_stats_available = true;
        process.jemalloc_allocated_bytes = 13;
        process.jemalloc_active_bytes = 14;
        process.jemalloc_resident_bytes = 15;
        process.jemalloc_mapped_bytes = 16;
        process.jemalloc_retained_bytes = 17;
        process.jemalloc_metadata_bytes = 18;

        let error = guard
            .safe_batch_input_bytes(
                process,
                2 * GIB,
                19_204_202,
                &HashMap::from([("pipeline.flush_reserved".to_string(), 19)]),
                &store_memory(),
            )
            .unwrap_err()
            .to_string();

        assert!(error.contains("headroom cannot safely build"), "{error}");
        assert!(error.contains("block=19204202"), "{error}");
        assert!(error.contains("rss_file_bytes:11"), "{error}");
        assert!(error.contains("rss_shmem_bytes:12"), "{error}");
        assert!(error.contains("jemalloc_stats_available:true"), "{error}");
        assert!(error.contains("jemalloc_allocated_bytes:13"), "{error}");
        assert!(error.contains("jemalloc_resident_bytes:15"), "{error}");
        assert!(error.contains("append_only_memtable_bytes:202"), "{error}");
        assert!(error.contains("shared_wbm_usage_bytes:808"), "{error}");
        assert!(error.contains("pipeline.flush_reserved:19"), "{error}");
    }
}
