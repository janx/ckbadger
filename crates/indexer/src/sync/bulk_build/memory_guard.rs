use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use crate::runtime_diag::{read_process_memory_snapshot, ProcessMemorySnapshot};

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
    ) -> Result<ProcessMemorySnapshot> {
        let snapshot = read_process_memory_snapshot()?;
        self.check_snapshot(snapshot, phase, block_number, retained_component_bytes)?;
        Ok(snapshot)
    }

    pub(crate) fn safe_batch_input_bytes(
        self,
        snapshot: ProcessMemorySnapshot,
        configured_max_batch_bytes: u64,
        block_number: u64,
        retained_component_bytes: &HashMap<String, u64>,
    ) -> Result<u64> {
        self.check_snapshot(
            snapshot,
            "before_batch",
            block_number,
            retained_component_bytes,
        )?;
        let committed = snapshot.committed_bytes()?;
        let headroom = self.limit_bytes - committed;
        let safe_input = headroom / BUILD_TRANSIENT_MULTIPLIER;
        if safe_input < MIN_SAFE_BATCH_INPUT_BYTES {
            bail!(
                "bulk process memory headroom cannot safely build another batch: block={} committed_bytes={} limit_bytes={} headroom_bytes={} required_min_input_bytes={} transient_multiplier={} retained_components={}",
                block_number,
                committed,
                self.limit_bytes,
                headroom,
                MIN_SAFE_BATCH_INPUT_BYTES,
                BUILD_TRANSIENT_MULTIPLIER,
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
    ) -> Result<()> {
        let committed = snapshot.committed_bytes()?;
        if committed > self.limit_bytes {
            bail!(
                "bulk process memory budget exceeded: phase={} block={} committed_bytes={} limit_bytes={} rss_bytes={} rss_anon_bytes={} swap_bytes={} high_water_rss_bytes={} retained_components={}",
                phase,
                block_number,
                committed,
                self.limit_bytes,
                snapshot.rss_bytes,
                snapshot.rss_anon_bytes,
                snapshot.swap_bytes,
                snapshot.high_water_rss_bytes,
                format_retained_components(retained_component_bytes)
            );
        }
        Ok(())
    }
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
            swap_bytes: swap,
            high_water_rss_bytes: rss,
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
            .safe_batch_input_bytes(snapshot(committed, 0), 2 * GIB, 100, &HashMap::new())
            .unwrap();
        assert_eq!(cap, (2 * GIB) / BUILD_TRANSIENT_MULTIPLIER);
    }

    #[test]
    fn budget_failure_reports_actual_rss_swap_and_owner_breakdown() {
        let guard = BulkMemoryGuard::new(Some(8), 32 * GIB).unwrap();
        let owners = HashMap::from([("live_cells".to_string(), 123)]);
        let error = guard
            .check_snapshot(snapshot(7 * GIB, 2 * GIB), "after_batch", 42, &owners)
            .unwrap_err()
            .to_string();
        assert!(error.contains("memory budget exceeded"), "{error}");
        assert!(error.contains("block=42"), "{error}");
        assert!(error.contains("swap_bytes=2147483648"), "{error}");
        assert!(error.contains("live_cells:123"), "{error}");
    }
}
