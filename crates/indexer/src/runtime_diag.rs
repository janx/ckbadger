use chrono::Utc;
#[cfg(all(not(target_env = "msvc"), not(target_os = "macos")))]
use jemalloc_ctl::{epoch, stats};
use serde::Serialize;

use ckbadger_store::MemoryStats;

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const PROC_SELF_STATUS: &str = "/proc/self/status";

#[derive(Debug, Clone, Default, Serialize)]
pub struct CgroupMemorySnapshot {
    pub memory_current_bytes: Option<u64>,
    pub memory_max_bytes: Option<u64>,
    pub memory_max_raw: Option<String>,
    pub oom_events: Option<u64>,
    pub oom_kill_events: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ProcessMemorySnapshot {
    pub rss_bytes: u64,
    pub rss_anon_bytes: u64,
    pub rss_file_bytes: u64,
    pub rss_shmem_bytes: u64,
    pub swap_bytes: u64,
    pub high_water_rss_bytes: u64,
    pub jemalloc_stats_available: bool,
    pub jemalloc_allocated_bytes: u64,
    pub jemalloc_active_bytes: u64,
    pub jemalloc_resident_bytes: u64,
    pub jemalloc_mapped_bytes: u64,
    pub jemalloc_retained_bytes: u64,
    pub jemalloc_metadata_bytes: u64,
}

impl ProcessMemorySnapshot {
    pub fn committed_bytes(self) -> anyhow::Result<u64> {
        self.rss_bytes.checked_add(self.swap_bytes).ok_or_else(|| {
            anyhow::anyhow!(
                "process committed memory overflow: rss_bytes={} swap_bytes={}",
                self.rss_bytes,
                self.swap_bytes
            )
        })
    }
}

/// Exact memory observations from the two chain stores.
///
/// The block cache and WriteBufferManager are process-wide resources shared by
/// both store handles. They are deliberately read through the domain handle
/// and counted once. Store-local memtables, table readers, compactions, SSTs,
/// L0 files, and immutable memtables are summed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChainStoreMemorySnapshot {
    pub domain_memtable_bytes: u64,
    pub append_only_memtable_bytes: u64,
    pub total_memtable_bytes: u64,
    pub shared_block_cache_bytes: u64,
    pub domain_table_readers_bytes: u64,
    pub append_only_table_readers_bytes: u64,
    pub total_table_readers_bytes: u64,
    pub total_memory_bytes: u64,
    pub domain_compaction_pending_bytes: u64,
    pub append_only_compaction_pending_bytes: u64,
    pub total_compaction_pending_bytes: u64,
    pub num_running_compactions: u64,
    pub sst_files_size: u64,
    pub l0_files_count: u64,
    pub l0_files_max: u64,
    pub l0_worst_cf: String,
    pub immutable_memtables: u64,
    pub top_cf_sizes: Vec<(String, u64)>,
    pub shared_wbm_usage_bytes: u64,
    pub shared_wbm_budget_bytes: u64,
}

pub fn aggregate_chain_store_memory(
    domain: &MemoryStats,
    append_only: &MemoryStats,
) -> anyhow::Result<ChainStoreMemorySnapshot> {
    let domain_memtable_bytes =
        usize_memory_to_u64("domain.memtable_bytes", domain.memtable_bytes)?;
    let append_only_memtable_bytes =
        usize_memory_to_u64("append_only.memtable_bytes", append_only.memtable_bytes)?;
    let total_memtable_bytes = checked_memory_add(
        "total_memtable_bytes",
        domain_memtable_bytes,
        append_only_memtable_bytes,
    )?;

    let shared_block_cache_bytes =
        usize_memory_to_u64("shared.block_cache_bytes", domain.block_cache_bytes)?;
    let domain_table_readers_bytes =
        usize_memory_to_u64("domain.table_readers_bytes", domain.table_readers_bytes)?;
    let append_only_table_readers_bytes = usize_memory_to_u64(
        "append_only.table_readers_bytes",
        append_only.table_readers_bytes,
    )?;
    let total_table_readers_bytes = checked_memory_add(
        "total_table_readers_bytes",
        domain_table_readers_bytes,
        append_only_table_readers_bytes,
    )?;
    let total_memory_bytes = checked_memory_add(
        "total_memory_bytes.memtable_and_cache",
        total_memtable_bytes,
        shared_block_cache_bytes,
    )
    .and_then(|bytes| {
        checked_memory_add(
            "total_memory_bytes.with_table_readers",
            bytes,
            total_table_readers_bytes,
        )
    })?;

    let total_compaction_pending_bytes = checked_memory_add(
        "total_compaction_pending_bytes",
        domain.compaction_pending_bytes,
        append_only.compaction_pending_bytes,
    )?;
    let num_running_compactions = checked_memory_add(
        "num_running_compactions",
        domain.num_running_compactions,
        append_only.num_running_compactions,
    )?;
    let sst_files_size = checked_memory_add(
        "sst_files_size",
        domain.sst_files_size,
        append_only.sst_files_size,
    )?;
    let l0_files_count = checked_memory_add(
        "l0_files_count",
        domain.l0_files_count,
        append_only.l0_files_count,
    )?;
    let immutable_memtables = checked_memory_add(
        "immutable_memtables",
        domain.immutable_memtables,
        append_only.immutable_memtables,
    )?;

    let (l0_files_max, l0_worst_cf) = if append_only.l0_files_max > domain.l0_files_max {
        (
            append_only.l0_files_max,
            format!("append-only.{}", append_only.l0_worst_cf),
        )
    } else {
        (domain.l0_files_max, domain.l0_worst_cf.clone())
    };

    let mut top_cf_sizes = domain.top_cf_sizes.clone();
    top_cf_sizes.extend(
        append_only
            .top_cf_sizes
            .iter()
            .map(|(name, bytes)| (format!("append-only.{name}"), *bytes)),
    );
    top_cf_sizes.sort_by(|a, b| b.1.cmp(&a.1));
    top_cf_sizes.truncate(5);

    Ok(ChainStoreMemorySnapshot {
        domain_memtable_bytes,
        append_only_memtable_bytes,
        total_memtable_bytes,
        shared_block_cache_bytes,
        domain_table_readers_bytes,
        append_only_table_readers_bytes,
        total_table_readers_bytes,
        total_memory_bytes,
        domain_compaction_pending_bytes: domain.compaction_pending_bytes,
        append_only_compaction_pending_bytes: append_only.compaction_pending_bytes,
        total_compaction_pending_bytes,
        num_running_compactions,
        sst_files_size,
        l0_files_count,
        l0_files_max,
        l0_worst_cf,
        immutable_memtables,
        top_cf_sizes,
        shared_wbm_usage_bytes: usize_memory_to_u64(
            "shared.wbm_usage_bytes",
            domain.wbm_usage_bytes,
        )?,
        shared_wbm_budget_bytes: usize_memory_to_u64(
            "shared.wbm_budget_bytes",
            domain.wbm_budget_bytes,
        )?,
    })
}

fn usize_memory_to_u64(field: &str, value: usize) -> anyhow::Result<u64> {
    u64::try_from(value).map_err(|_| {
        anyhow::anyhow!(
            "memory statistic exceeds u64: field={} value={}",
            field,
            value
        )
    })
}

fn checked_memory_add(field: &str, left: u64, right: u64) -> anyhow::Result<u64> {
    left.checked_add(right).ok_or_else(|| {
        anyhow::anyhow!(
            "memory statistic overflow: field={} left={} right={}",
            field,
            left,
            right
        )
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct FlightEvent {
    pub ts: i64,
    pub event: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct FlightRecorder {
    capacity: usize,
    events: Mutex<VecDeque<FlightEvent>>,
}

impl FlightRecorder {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            events: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
        }
    }

    pub fn record(&self, event: &str, detail: impl Into<String>) {
        let mut guard = self.events.lock().unwrap();
        guard.push_back(FlightEvent {
            ts: Utc::now().timestamp(),
            event: event.to_string(),
            detail: detail.into(),
        });
        while guard.len() > self.capacity {
            guard.pop_front();
        }
    }

    pub fn snapshot(&self) -> Vec<FlightEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    }
}

pub fn generate_run_id() -> String {
    format!(
        "run-{}-pid{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id()
    )
}

pub fn generate_incident_id(run_id: &str, sequence: u64) -> String {
    format!("{}-inc-{:06}", run_id, sequence)
}

pub fn read_cgroup_memory_snapshot() -> CgroupMemorySnapshot {
    read_cgroup_memory_snapshot_from(Path::new(CGROUP_ROOT))
}

pub fn read_process_memory_snapshot() -> anyhow::Result<ProcessMemorySnapshot> {
    let content = fs::read_to_string(PROC_SELF_STATUS).map_err(|err| {
        anyhow::anyhow!(
            "failed to read process memory status {}: {}",
            PROC_SELF_STATUS,
            err
        )
    })?;
    let mut snapshot = parse_process_memory_status(&content)?;

    #[cfg(all(not(target_env = "msvc"), not(target_os = "macos")))]
    {
        epoch::advance()
            .map_err(|err| anyhow::anyhow!("failed to advance jemalloc stats epoch: {err}"))?;
        snapshot.jemalloc_stats_available = true;
        snapshot.jemalloc_allocated_bytes = usize_memory_to_u64(
            "jemalloc.allocated_bytes",
            stats::allocated::read()
                .map_err(|err| anyhow::anyhow!("failed to read jemalloc allocated bytes: {err}"))?,
        )?;
        snapshot.jemalloc_active_bytes = usize_memory_to_u64(
            "jemalloc.active_bytes",
            stats::active::read()
                .map_err(|err| anyhow::anyhow!("failed to read jemalloc active bytes: {err}"))?,
        )?;
        snapshot.jemalloc_resident_bytes = usize_memory_to_u64(
            "jemalloc.resident_bytes",
            stats::resident::read()
                .map_err(|err| anyhow::anyhow!("failed to read jemalloc resident bytes: {err}"))?,
        )?;
        snapshot.jemalloc_mapped_bytes = usize_memory_to_u64(
            "jemalloc.mapped_bytes",
            stats::mapped::read()
                .map_err(|err| anyhow::anyhow!("failed to read jemalloc mapped bytes: {err}"))?,
        )?;
        snapshot.jemalloc_retained_bytes = usize_memory_to_u64(
            "jemalloc.retained_bytes",
            stats::retained::read()
                .map_err(|err| anyhow::anyhow!("failed to read jemalloc retained bytes: {err}"))?,
        )?;
        snapshot.jemalloc_metadata_bytes = usize_memory_to_u64(
            "jemalloc.metadata_bytes",
            stats::metadata::read()
                .map_err(|err| anyhow::anyhow!("failed to read jemalloc metadata bytes: {err}"))?,
        )?;
    }

    Ok(snapshot)
}

fn parse_process_memory_status(content: &str) -> anyhow::Result<ProcessMemorySnapshot> {
    let read_field = |name: &str| -> anyhow::Result<u64> {
        let line = content
            .lines()
            .find(|line| line.starts_with(name))
            .ok_or_else(|| anyhow::anyhow!("process memory status missing field {}", name))?;
        let (_, raw) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("malformed process memory field: {}", line))?;
        let mut parts = raw.split_whitespace();
        let kib = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("process memory field has no value: {}", line))?
            .parse::<u64>()
            .map_err(|err| anyhow::anyhow!("invalid process memory field {}: {}", line, err))?;
        let unit = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("process memory field has no unit: {}", line))?;
        if unit != "kB" {
            return Err(anyhow::anyhow!(
                "unsupported process memory field unit: field={} unit={}",
                name,
                unit
            ));
        }
        kib.checked_mul(1024).ok_or_else(|| {
            anyhow::anyhow!(
                "process memory field byte conversion overflow: field={} kib={}",
                name,
                kib
            )
        })
    };

    Ok(ProcessMemorySnapshot {
        rss_bytes: read_field("VmRSS")?,
        rss_anon_bytes: read_field("RssAnon")?,
        rss_file_bytes: read_field("RssFile")?,
        rss_shmem_bytes: read_field("RssShmem")?,
        swap_bytes: read_field("VmSwap")?,
        high_water_rss_bytes: read_field("VmHWM")?,
        ..Default::default()
    })
}

fn read_cgroup_memory_snapshot_from(root: &Path) -> CgroupMemorySnapshot {
    let memory_current_bytes = read_u64_file(&root.join("memory.current"));

    let memory_max_raw = read_trimmed(&root.join("memory.max"));
    let memory_max_bytes = memory_max_raw.as_deref().and_then(|value| {
        if value == "max" {
            None
        } else {
            value.parse().ok()
        }
    });

    let (oom_events, oom_kill_events) = read_memory_events(&root.join("memory.events"));

    CgroupMemorySnapshot {
        memory_current_bytes,
        memory_max_bytes,
        memory_max_raw,
        oom_events,
        oom_kill_events,
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_u64_file(path: &Path) -> Option<u64> {
    read_trimmed(path).and_then(|value| value.parse::<u64>().ok())
}

fn read_memory_events(path: &Path) -> (Option<u64>, Option<u64>) {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return (None, None),
    };

    let mut oom_events = None;
    let mut oom_kill_events = None;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        let parsed = value.parse::<u64>().ok();
        match key {
            "oom" => oom_events = parsed,
            "oom_kill" => oom_kill_events = parsed,
            _ => {}
        }
    }

    (oom_events, oom_kill_events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_run_and_incident_ids() {
        let run_id = generate_run_id();
        assert!(run_id.starts_with("run-"));
        assert!(run_id.contains("-pid"));

        let incident_id = generate_incident_id("run-abc", 42);
        assert_eq!(incident_id, "run-abc-inc-000042");
    }

    #[test]
    fn test_flight_recorder_eviction() {
        let recorder = FlightRecorder::new(2);
        recorder.record("event-1", "a");
        recorder.record("event-2", "b");
        recorder.record("event-3", "c");

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].event, "event-2");
        assert_eq!(snapshot[1].event, "event-3");
    }

    #[test]
    fn test_read_cgroup_snapshot_from_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("memory.current"), "123\n").unwrap();
        fs::write(dir.path().join("memory.max"), "max\n").unwrap();
        fs::write(
            dir.path().join("memory.events"),
            "low 0\noom 7\noom_kill 2\n",
        )
        .unwrap();

        let snapshot = read_cgroup_memory_snapshot_from(dir.path());
        assert_eq!(snapshot.memory_current_bytes, Some(123));
        assert_eq!(snapshot.memory_max_bytes, None);
        assert_eq!(snapshot.memory_max_raw.as_deref(), Some("max"));
        assert_eq!(snapshot.oom_events, Some(7));
        assert_eq!(snapshot.oom_kill_events, Some(2));
    }

    #[test]
    fn test_parse_process_memory_status_uses_rss_plus_swap_as_committed() {
        let status = r#"Name:\tckbadger
VmHWM:   4096 kB
VmRSS:   3072 kB
RssAnon: 2048 kB
RssFile: 768 kB
RssShmem: 256 kB
VmSwap:  1024 kB
"#;
        let snapshot = parse_process_memory_status(status).unwrap();
        assert_eq!(snapshot.rss_bytes, 3 * 1024 * 1024);
        assert_eq!(snapshot.rss_anon_bytes, 2 * 1024 * 1024);
        assert_eq!(snapshot.rss_file_bytes, 768 * 1024);
        assert_eq!(snapshot.rss_shmem_bytes, 256 * 1024);
        assert_eq!(snapshot.swap_bytes, 1024 * 1024);
        assert_eq!(snapshot.high_water_rss_bytes, 4 * 1024 * 1024);
        assert_eq!(snapshot.committed_bytes().unwrap(), 4 * 1024 * 1024);
        assert!(!snapshot.jemalloc_stats_available);
    }

    #[test]
    fn test_chain_store_memory_aggregation_counts_shared_resources_once() {
        let domain = MemoryStats {
            memtable_bytes: 100,
            block_cache_bytes: 300,
            table_readers_bytes: 10,
            compaction_pending_bytes: 1_000,
            num_running_compactions: 2,
            sst_files_size: 10_000,
            l0_files_count: 4,
            l0_files_max: 3,
            l0_worst_cf: "activities".to_string(),
            immutable_memtables: 5,
            top_cf_sizes: vec![("activities".to_string(), 900)],
            wbm_usage_bytes: 400,
            wbm_budget_bytes: 800,
            ..Default::default()
        };
        let append_only = MemoryStats {
            memtable_bytes: 200,
            // Both handles observe the same process-wide resources. These
            // values must not be added to the domain observations.
            block_cache_bytes: 300,
            table_readers_bytes: 20,
            compaction_pending_bytes: 2_000,
            num_running_compactions: 1,
            sst_files_size: 20_000,
            l0_files_count: 6,
            l0_files_max: 6,
            l0_worst_cf: "cells".to_string(),
            immutable_memtables: 7,
            top_cf_sizes: vec![("cells".to_string(), 1_200)],
            wbm_usage_bytes: 400,
            wbm_budget_bytes: 800,
            ..Default::default()
        };

        let snapshot = aggregate_chain_store_memory(&domain, &append_only).unwrap();
        assert_eq!(snapshot.total_memtable_bytes, 300);
        assert_eq!(snapshot.shared_block_cache_bytes, 300);
        assert_eq!(snapshot.total_table_readers_bytes, 30);
        assert_eq!(snapshot.total_memory_bytes, 630);
        assert_eq!(snapshot.total_compaction_pending_bytes, 3_000);
        assert_eq!(snapshot.num_running_compactions, 3);
        assert_eq!(snapshot.sst_files_size, 30_000);
        assert_eq!(snapshot.l0_files_count, 10);
        assert_eq!(snapshot.l0_files_max, 6);
        assert_eq!(snapshot.l0_worst_cf, "append-only.cells");
        assert_eq!(snapshot.immutable_memtables, 12);
        assert_eq!(snapshot.shared_wbm_usage_bytes, 400);
        assert_eq!(snapshot.shared_wbm_budget_bytes, 800);
        assert_eq!(
            snapshot.top_cf_sizes,
            vec![
                ("append-only.cells".to_string(), 1_200),
                ("activities".to_string(), 900)
            ]
        );
    }
}
