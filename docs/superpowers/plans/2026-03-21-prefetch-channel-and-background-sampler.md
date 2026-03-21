# Prefetch Channel & Background Sampler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce bulk sync wall clock by ~31-33% (1433s → ~960-990s) by buffering prefetch to depth=4 and moving per-iteration overhead to a background thread.

**Architecture:** Two new worker constructs mirror the existing `FlushChannelHandle` pattern. `PrefetchChannelHandle` runs a self-advancing fetch worker with a bounded result channel (depth=4) and watch-channel span feedback. `BackgroundSampler` runs a dedicated `std::thread` that periodically samples RocksDB stats and `/proc`, publishing via a watch channel. The O(N²) `write_metrics_file()` call is removed from per-batch paths and deferred to finalization.

**Tech Stack:** Rust, tokio (mpsc/watch channels, spawn_blocking), std::thread, RocksDB, rayon

**Spec:** `docs/superpowers/specs/2026-03-21-prefetch-channel-and-background-sampler-design.md`

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `crates/indexer/src/sync/bulk_build/prefetch.rs` | `PrefetchChannelHandle`, `PrefetchResult`, `PrefetchExitReason`, `PrefetchWorkerStats` | Create |
| `crates/indexer/src/sync/bulk_build/sampler.rs` | `BackgroundSampler`, `SamplerSnapshot` | Create |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Main loop: replace prefetch logic, replace inline stats, add module declarations | Modify |
| `crates/indexer/src/bulk_sync_perf.rs` | Rename `prefetch_collect_ms` → `prefetch_recv_ms`, add `prefetch_depth`, remove per-batch `write_metrics_file()` | Modify |
| `crates/indexer/src/sync/diagnostics.rs` | Rename `last_prefetch_collect_us` → `last_prefetch_recv_us` in atomics and snapshot | Modify |
| `crates/tui/src/ui.rs` | Update all reads of `prefetch_collect_ms` → `prefetch_recv_ms` | Modify |

---

### Task 1: Add `PrefetchChannelHandle` with tests

**Files:**
- Create: `crates/indexer/src/sync/bulk_build/prefetch.rs`
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:41-48` (add `pub(crate) mod prefetch;`)

- [ ] **Step 1: Create prefetch.rs with types and PrefetchChannelHandle**

Create `crates/indexer/src/sync/bulk_build/prefetch.rs` with the full implementation from the spec:

```rust
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::binary_facts::RawCkbBlock;
use super::BULK_BUILD_MIN_BLOCK_SPAN;
use crate::sync::indexer::Indexer;

/// Result of a single prefetch operation.
pub(crate) struct PrefetchResult {
    pub blocks: Vec<RawCkbBlock>,
    pub fetch_elapsed: Duration,
    pub effective_end: u64,
}

/// Why the prefetch worker exited.
#[derive(Debug)]
pub(crate) enum PrefetchExitReason {
    Completed,
    ReceiverDropped,
}

/// Aggregate stats from the prefetch worker.
#[derive(Debug)]
pub(crate) struct PrefetchWorkerStats {
    pub total_fetches: u64,
    pub total_blocks: u64,
    pub exit_reason: PrefetchExitReason,
}

/// Bounded prefetch pipeline that continuously fetches blocks ahead of the
/// main build loop. Mirrors the `FlushChannelHandle` pattern.
///
/// The worker advances its own position counter and reads the latest
/// `batch_block_span` via a watch channel before each fetch.
pub(crate) struct PrefetchChannelHandle {
    result_rx: tokio::sync::mpsc::Receiver<Result<PrefetchResult>>,
    span_tx: tokio::sync::watch::Sender<u64>,
    worker_handle: tokio::task::JoinHandle<Result<PrefetchWorkerStats>>,
}

impl PrefetchChannelHandle {
    pub(crate) fn new(
        depth: usize,
        ckb_store: Arc<ckb_store_reader::CkbChainReader>,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
        initial_span: u64,
    ) -> Self {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(depth);
        let (span_tx, span_rx) = tokio::sync::watch::channel(initial_span);

        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::prefetch_worker(
                result_tx,
                span_rx,
                ckb_store,
                fetch_pool,
                start_block,
                handoff_target,
            )
        });

        Self {
            result_rx,
            span_tx,
            worker_handle,
        }
    }

    fn prefetch_worker(
        result_tx: tokio::sync::mpsc::Sender<Result<PrefetchResult>>,
        span_rx: tokio::sync::watch::Receiver<u64>,
        ckb_store: Arc<ckb_store_reader::CkbChainReader>,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
    ) -> Result<PrefetchWorkerStats> {
        let mut stats = PrefetchWorkerStats {
            total_fetches: 0,
            total_blocks: 0,
            exit_reason: PrefetchExitReason::Completed,
        };
        let mut position = start_block;

        while position <= handoff_target {
            let span = (*span_rx.borrow()).max(BULK_BUILD_MIN_BLOCK_SPAN);
            let end = std::cmp::min(
                position.saturating_add(span.saturating_sub(1)),
                handoff_target,
            );

            let started = Instant::now();
            let fetch_result =
                Indexer::fetch_blocks_direct_binary(&ckb_store, position, end, Some(&fetch_pool));

            let to_send = match fetch_result {
                Ok(blocks) => {
                    let block_count = blocks.len() as u64;
                    stats.total_fetches += 1;
                    stats.total_blocks += block_count;
                    Ok(PrefetchResult {
                        blocks,
                        fetch_elapsed: started.elapsed(),
                        effective_end: end,
                    })
                }
                Err(e) => Err(e),
            };
            let is_err = to_send.is_err();

            if result_tx.blocking_send(to_send).is_err() {
                stats.exit_reason = PrefetchExitReason::ReceiverDropped;
                break;
            }

            if is_err {
                break;
            }

            position = end.saturating_add(1);
        }

        Ok(stats)
    }

    /// Receive next batch. Blocks if pipeline is empty.
    pub(crate) async fn recv(&mut self) -> Result<PrefetchResult> {
        match self.result_rx.recv().await {
            Some(result) => result,
            None => Err(anyhow!(
                "prefetch worker terminated without sending an error"
            )),
        }
    }

    /// Update batch span. The worker picks this up before its next fetch.
    pub(crate) fn update_span(&self, span: u64) {
        let _ = self.span_tx.send(span);
    }

    /// Shut down: drop channels to signal worker, then join.
    pub(crate) async fn close_and_wait(self) -> Result<PrefetchWorkerStats> {
        drop(self.result_rx);
        drop(self.span_tx);
        self.worker_handle
            .await
            .map_err(|e| anyhow!("prefetch worker panicked: {}", e))?
    }
}
```

- [ ] **Step 2: Add module declaration**

In `crates/indexer/src/sync/bulk_build/mod.rs`, add after the existing module declarations (line 48):

```rust
pub(crate) mod prefetch;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`

Note: The import of `crate::ckb_store_reader::CkbChainReader` may need adjustment — check the actual type path used in `mod.rs` line 102-104. If `ckb_store()` returns a different type, adjust the import. Also check how `Indexer::fetch_blocks_direct_binary` is accessed — it's defined in `crates/indexer/src/sync/pipeline.rs:2821` and may need a `use super::super::pipeline::Indexer` or similar path.

- [ ] **Step 4: Write unit tests for PrefetchChannelHandle**

Add `#[cfg(test)] mod tests` at the bottom of `prefetch.rs`. Since `fetch_blocks_direct_binary` requires a real CKB RocksDB, these tests should NOT test the worker directly. Instead test the channel mechanics:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prefetch_channel_recv_gets_results_in_order() {
        // Test the channel mechanics directly without a real CKB store.
        // Spawn a mock worker that sends numbered results.
        let depth = 2;
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(depth);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);

        let worker = tokio::task::spawn(async move {
            for i in 0..5u64 {
                let result = Ok(PrefetchResult {
                    blocks: vec![],
                    fetch_elapsed: Duration::from_millis(10),
                    effective_end: (i + 1) * 10_000 - 1,
                });
                result_tx.send(result).await.unwrap();
            }
        });

        let mut handle = PrefetchChannelHandle {
            result_rx,
            span_tx,
            worker_handle: tokio::task::spawn_blocking(|| Ok(PrefetchWorkerStats {
                total_fetches: 5,
                total_blocks: 50_000,
                exit_reason: PrefetchExitReason::Completed,
            })),
        };

        for i in 0..5u64 {
            let result = handle.recv().await.unwrap();
            assert_eq!(result.effective_end, (i + 1) * 10_000 - 1);
        }

        worker.await.unwrap();
    }

    #[tokio::test]
    async fn prefetch_channel_recv_returns_error_on_worker_exit() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(2);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);

        // Drop the sender immediately — simulates worker exit.
        drop(result_tx);

        let mut handle = PrefetchChannelHandle {
            result_rx,
            span_tx,
            worker_handle: tokio::task::spawn_blocking(|| Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                exit_reason: PrefetchExitReason::Completed,
            })),
        };

        let result = handle.recv().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn prefetch_channel_propagates_fetch_errors() {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(2);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);

        let worker = tokio::task::spawn(async move {
            result_tx
                .send(Err(anyhow!("simulated fetch error")))
                .await
                .unwrap();
        });

        let mut handle = PrefetchChannelHandle {
            result_rx,
            span_tx,
            worker_handle: tokio::task::spawn_blocking(|| Ok(PrefetchWorkerStats {
                total_fetches: 0,
                total_blocks: 0,
                exit_reason: PrefetchExitReason::Completed,
            })),
        };

        let result = handle.recv().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("simulated fetch error"));

        worker.await.unwrap();
    }

    #[tokio::test]
    async fn prefetch_channel_close_and_wait_returns_stats() {
        let (_result_tx, result_rx) = tokio::sync::mpsc::channel::<Result<PrefetchResult>>(2);
        let (span_tx, _span_rx) = tokio::sync::watch::channel(10_000u64);

        let handle = PrefetchChannelHandle {
            result_rx,
            span_tx,
            worker_handle: tokio::task::spawn_blocking(|| Ok(PrefetchWorkerStats {
                total_fetches: 42,
                total_blocks: 420_000,
                exit_reason: PrefetchExitReason::Completed,
            })),
        };

        let stats = handle.close_and_wait().await.unwrap();
        assert_eq!(stats.total_fetches, 42);
        assert_eq!(stats.total_blocks, 420_000);
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p ckbadger-indexer prefetch_channel`
Expected: All 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/prefetch.rs crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(bulk-build): add PrefetchChannelHandle with bounded channel and tests"
```

---

### Task 2: Add `BackgroundSampler` with tests

**Files:**
- Create: `crates/indexer/src/sync/bulk_build/sampler.rs`
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:41-49` (add `pub(crate) mod sampler;`)

- [ ] **Step 1: Create sampler.rs with BackgroundSampler**

Create `crates/indexer/src/sync/bulk_build/sampler.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::sys_info;
use ckbadger_store::CkbadgerStore;

/// Snapshot of RocksDB and system metrics captured by the background sampler.
#[derive(Clone, Default, Debug)]
pub(crate) struct SamplerSnapshot {
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
}

/// Background thread that periodically samples RocksDB stats and system
/// environment, publishing snapshots via a watch channel.
///
/// Removes `memory_stats()` and `read_batch_environment()` from the
/// main build loop's critical path.
pub(crate) struct BackgroundSampler {
    latest_rx: tokio::sync::watch::Receiver<SamplerSnapshot>,
    shutdown: Arc<AtomicBool>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl BackgroundSampler {
    /// Spawn the sampler thread. It immediately begins sampling at `interval`.
    pub(crate) fn new(store: Arc<CkbadgerStore>, interval: Duration) -> Self {
        let (tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let handle = std::thread::Builder::new()
            .name("bg-sampler".into())
            .spawn(move || {
                let mut disk_tracker = sys_info::DiskStatsTracker::new(String::new());
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    let stats = store.memory_stats();
                    let env = sys_info::read_batch_environment(&mut disk_tracker);
                    let snapshot = SamplerSnapshot {
                        compaction_pending_mb: stats.compaction_pending_bytes / (1024 * 1024),
                        l0_files: stats.l0_files_count,
                        imm_memtables: stats.immutable_memtables,
                        load_avg_1m: env.load_avg_1m,
                        mem_available_mb: env.mem_available_mb,
                        disk_read_mb: env.disk_read_mb,
                        disk_write_mb: env.disk_write_mb,
                    };
                    if tx.send(snapshot).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to spawn background sampler thread");

        Self {
            latest_rx: rx,
            shutdown,
            worker_handle: Some(handle),
        }
    }

    /// Non-blocking read of latest sampled values.
    pub(crate) fn latest(&self) -> SamplerSnapshot {
        self.latest_rx.borrow().clone()
    }

    /// Signal shutdown and join the thread.
    pub(crate) fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for BackgroundSampler {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_snapshot_default_is_zeroed() {
        let snap = SamplerSnapshot::default();
        assert_eq!(snap.l0_files, 0);
        assert_eq!(snap.compaction_pending_mb, 0);
        assert_eq!(snap.load_avg_1m, 0.0);
        assert_eq!(snap.mem_available_mb, 0);
    }

    #[test]
    fn sampler_shutdown_joins_thread() {
        // Create a sampler that cannot actually call memory_stats (no real store),
        // so we test the shutdown/join mechanics with a mock.
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let (tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());

        let handle = std::thread::Builder::new()
            .name("test-sampler".into())
            .spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                    if tx.send(SamplerSnapshot {
                        l0_files: 42,
                        ..Default::default()
                    }).is_err() {
                        break;
                    }
                }
            })
            .unwrap();

        let sampler = BackgroundSampler {
            latest_rx: rx,
            shutdown,
            worker_handle: Some(handle),
        };

        // Let it run briefly
        std::thread::sleep(Duration::from_millis(50));
        let snap = sampler.latest();
        assert_eq!(snap.l0_files, 42);

        // Shutdown should return promptly
        sampler.shutdown();
    }

    #[test]
    fn sampler_drop_signals_shutdown() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let shutdown_check = Arc::clone(&shutdown);
        let (_tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());

        let handle = std::thread::Builder::new()
            .name("test-sampler-drop".into())
            .spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
            .unwrap();

        let sampler = BackgroundSampler {
            latest_rx: rx,
            shutdown,
            worker_handle: Some(handle),
        };

        drop(sampler);

        // After drop, the shutdown flag should be set.
        assert!(shutdown_check.load(Ordering::Relaxed));
    }
}
```

- [ ] **Step 2: Add module declaration**

In `crates/indexer/src/sync/bulk_build/mod.rs`, add after the prefetch module declaration:

```rust
pub(crate) mod sampler;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`

Note: Verify that `ckbadger_store::CkbadgerStore` is the correct import path. Check the existing import at `mod.rs` line 108 (`indexer.writer.store()` returns `Arc<CkbadgerStore>`). Also verify `sys_info::read_batch_environment` and `sys_info::DiskStatsTracker` import paths match the existing usage at `mod.rs:111` and `mod.rs:315`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer sampler`
Expected: All 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/sampler.rs crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(bulk-build): add BackgroundSampler with periodic RocksDB and /proc sampling"
```

---

### Task 3: Rename `prefetch_collect_ms` → `prefetch_recv_ms` and add `prefetch_depth`

This is a rename-only task. No logic changes. Touch all layers: perf struct, common progress data, diagnostics atomics, TUI reads.

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs:64,135` (BatchSample field)
- Modify: `crates/common/src/sync.rs:238,776` (BulkBuildProgressData field + default)
- Modify: `crates/indexer/src/sync/diagnostics.rs:297,363,420-421,458-460` (atomic + snapshot)
- Modify: `crates/tui/src/ui.rs:436,442,451,453,2224,2229,2230` (TUI reads)
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:364,411` (field assignment + diagnostics call)
- Modify: test files in diagnostics.rs and ui.rs

- [ ] **Step 1: Rename in BatchSample (bulk_sync_perf.rs)**

At line 64, rename field:
```rust
// OLD: pub prefetch_collect_ms: f64,
pub prefetch_recv_ms: f64,
```

At line 135, rename initialization:
```rust
// OLD: prefetch_collect_ms: 0.0,
prefetch_recv_ms: 0.0,
```

Add a new field after `flush_channel_depth` (around line 68):
```rust
pub prefetch_depth: u64,
```

And initialize it in `new()` (around line 139):
```rust
prefetch_depth: 0,
```

- [ ] **Step 2: Rename in BulkBuildProgressData (common/sync.rs)**

At line 238, rename field in the `BulkBuildProgressData` struct:
```rust
// OLD: pub prefetch_collect_ms: Option<f64>,
pub prefetch_recv_ms: Option<f64>,
```

At line 776 (or wherever the Default impl initializes this field), rename:
```rust
// OLD: prefetch_collect_ms: None,
prefetch_recv_ms: None,
```

This struct bridges `diagnostics.rs` → TUI. If you miss this, `cargo check` will catch it immediately.

- [ ] **Step 3: Rename in diagnostics.rs**

At line 297, rename atomic:
```rust
// OLD: last_prefetch_collect_us: AtomicU64,
last_prefetch_recv_us: AtomicU64,
```

At line 363, rename parameter:
```rust
// OLD: prefetch_collect_ms: f64,
prefetch_recv_ms: f64,
```

At lines 420-421, rename store:
```rust
// OLD: self.last_prefetch_collect_us.store(ms_to_us(prefetch_collect_ms), ...);
self.last_prefetch_recv_us.store(ms_to_us(prefetch_recv_ms), Ordering::Relaxed);
```

At lines 458-460, rename snapshot field:
```rust
// OLD: prefetch_collect_ms: Some(us_to_ms(self.last_prefetch_collect_us.load(...)))
prefetch_recv_ms: Some(us_to_ms(self.last_prefetch_recv_us.load(Ordering::Relaxed))),
```

Also update the snapshot struct (find `BulkBuildPerfSnapshot` struct) — rename the field there too.

- [ ] **Step 4: Rename in TUI (ui.rs)**

At all usage sites (lines 436, 442, 451, 453, 2224, 2229, 2230), rename:
```rust
// OLD: let prefetch_collect_ms = bb.prefetch_collect_ms.unwrap_or(0.0);
let prefetch_recv_ms = bb.prefetch_recv_ms.unwrap_or(0.0);
```

Update all calculations that reference `prefetch_collect_ms` to use `prefetch_recv_ms`.

- [ ] **Step 5: Rename in mod.rs**

At line 364:
```rust
// OLD: sample.prefetch_collect_ms = prefetch_collect_elapsed.as_secs_f64() * 1000.0;
sample.prefetch_recv_ms = prefetch_collect_elapsed.as_secs_f64() * 1000.0;
```

At line 411 (diagnostics call), update the parameter name in the call.

- [ ] **Step 6: Update all tests**

Search for `prefetch_collect_ms` in test code across:
- `crates/indexer/src/sync/diagnostics.rs` (lines 1052, 1107, 1136, 1149-1153)
- `crates/tui/src/ui.rs` (lines 6160, 6173, 6192-6213)
- Any other test files

Rename all occurrences.

- [ ] **Step 7: Verify**

Run: `cargo check && cargo test --lib`
Expected: Compiles and all tests pass. Run `cargo test -p ckbadger-indexer` and `cargo test -p ckbadger-tui` specifically.

- [ ] **Step 8: Commit**

```bash
git add -u
git commit -m "refactor(bulk-build): rename prefetch_collect_ms to prefetch_recv_ms, add prefetch_depth field"
```

---

### Task 4: Remove `write_metrics_file()` from per-batch paths (O(N²) fix)

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs:296-298,303-305,313-314`

- [ ] **Step 1: Read the current code**

Read `crates/indexer/src/bulk_sync_perf.rs` lines 295-320 to see the exact current code for all three methods.

- [ ] **Step 2: Remove write_metrics_file from record_batch_sample**

At line 298, remove the `write_metrics_file` call. The method should become:
```rust
pub fn record_batch_sample(&mut self, sample: BatchSample) -> Result<()> {
    self.append_sample("batch", &sample)?;
    self.batch_samples.push(sample);
    // REMOVED: self.write_metrics_file(&self.build_metrics(STATUS_RUNNING, None))?;
    Ok(())
}
```

- [ ] **Step 3: Remove write_metrics_file from record_heartbeat_sample**

At line 305, remove the `write_metrics_file` call. Same pattern.

- [ ] **Step 4: Remove write_metrics_file from set_materialization_report**

At line 314, remove the `write_metrics_file` call. Same pattern.

- [ ] **Step 5: Ensure write_metrics_file is called at finalization**

Search for where the run is finalized (look for `STATUS_COMPLETED` or `finish`/`close`/`finalize` methods in `bulk_sync_perf.rs`). The `write_metrics_file` should be called there. If it's already called at finalization, no additional change is needed. If not, add it to the finalization path.

Search: `grep -n "STATUS_COMPLETED\|finalize\|fn close\|fn finish" crates/indexer/src/bulk_sync_perf.rs`

The `append_trend_line` method (line 1041) is called at run completion. Ensure `write_metrics_file` is also called there or in the same finalization sequence.

- [ ] **Step 6: Add a test verifying metrics.env is written at finalization**

```rust
#[test]
fn test_metrics_file_not_written_per_batch() {
    // Create a BulkSyncPerfRun, record 3 batch samples.
    // Verify metrics.env does NOT exist after each record_batch_sample.
    // Then call finalize/close.
    // Verify metrics.env exists and contains valid data.
}
```

The exact test structure depends on how `BulkSyncPerfRun` is constructed in tests. Look at existing tests for the pattern.

- [ ] **Step 7: Run tests**

Run: `cargo test -p ckbadger-indexer bulk_sync_perf`
Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs
git commit -m "perf(bulk-build): remove O(N²) write_metrics_file from per-batch paths, defer to finalization"
```

---

### Task 5: Integrate PrefetchChannelHandle into the main loop

This is the core change. Replace the single-depth prefetch with the channel.

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:111,126-136,154-265,271,308-318,345-346,362-364,382-414,493`

- [ ] **Step 1: Read the current main loop code**

Read `crates/indexer/src/sync/bulk_build/mod.rs` lines 100-500 to understand the full loop structure before making changes.

- [ ] **Step 2: Add PREFETCH_DEPTH constant**

Near the existing constants (lines 50-57), add:
```rust
const PREFETCH_DEPTH: usize = 4;
```

- [ ] **Step 3: Replace prefetch initialization (before loop)**

Replace lines 130-136 (the `prefetched_blocks` Option):
```rust
// OLD:
// let mut prefetched_blocks: Option<(Vec<binary_facts::RawCkbBlock>, Duration, u64)> = None;

// NEW: determine initial handoff_target for the prefetch worker
let initial_chain_tip = ckb_store.tip_number().ok_or_else(|| {
    anyhow!("failed to get chain tip from CKB RocksDB for prefetch init")
})?;
let initial_handoff = initial_chain_tip.saturating_sub(indexer.config.bulk_sync_threshold);
let prefetch_start = if indexer.progress.current() == 0 {
    0
} else {
    indexer.progress.current() + 1
};
let mut prefetch = prefetch::PrefetchChannelHandle::new(
    PREFETCH_DEPTH,
    ckb_store.clone(),
    Arc::clone(&fetch_pool),
    prefetch_start,
    initial_handoff,
    configured_batch_size,
);
```

- [ ] **Step 4: Replace block acquisition in the loop**

Replace lines 154-265 (the chain_tip refresh, boundary calculation, prefetch take/sync fetch, prefetch spawn, build, and collect) with:

```rust
loop {
    ckb_store.refresh()?;
    let chain_tip = ckb_store.tip_number().ok_or_else(|| {
        anyhow!("failed to get chain tip from CKB RocksDB during bulk build")
    })?;
    indexer.progress.update_target(chain_tip);

    let current_block = indexer.progress.current();
    let blocks_remaining = chain_tip.saturating_sub(current_block);
    if blocks_remaining <= indexer.config.bulk_sync_threshold {
        break;
    }

    // Receive next batch from prefetch pipeline.
    // recv() returns Err in two cases:
    // - Worker sent a fetch error through the channel (propagated with context)
    // - Worker terminated (channel closed, e.g., reached handoff_target)
    let recv_started = Instant::now();
    let prefetch_result = match prefetch.recv().await {
        Ok(result) => result,
        Err(e) => {
            // Channel closed = worker finished its range (normal exit).
            // Fetch error = propagated from worker (abnormal).
            // In either case, break and let close_and_wait() report stats.
            info!(error = %e, "prefetch channel closed, ending bulk build loop");
            break;
        }
    };
    let prefetch_recv_elapsed = recv_started.elapsed();

    let (blocks, fetch_elapsed, effective_end) = (
        prefetch_result.blocks,
        prefetch_result.fetch_elapsed,
        prefetch_result.effective_end,
    );

    let build_started = Instant::now();
    let (batch_stats, build_timings, pending_flush) =
        runtime.apply_blocks(&blocks, indexer.config.is_mainnet(), &token_info_cache)?;
    let build_elapsed = build_started.elapsed();

    let controllable_ms =
        (build_elapsed + prefetch_recv_elapsed).as_secs_f64() * 1000.0;

    // ... rest of loop body (flush, perf recording, adaptive sizing) stays the same ...
```

**Important**: The `next_start`, `next_end` calculation, and the `prefetch_handle` spawn block (lines 220-249) should be entirely removed. The boundary computation for `start_block` and `end_block` (lines 167-197) is also removed — the prefetch worker handles this.

- [ ] **Step 5: Update perf sample fields**

Replace the prefetch-related sample fields (around line 362-364):
```rust
// OLD:
// sample.prefetch_collect_ms = prefetch_collect_elapsed.as_secs_f64() * 1000.0;

// NEW:
sample.prefetch_recv_ms = prefetch_recv_elapsed.as_secs_f64() * 1000.0;
sample.prefetch_depth = PREFETCH_DEPTH as u64;
```

- [ ] **Step 6: Update diagnostics record_batch call**

At lines 382-414, update the parameter for the renamed field:
```rust
// OLD: prefetch_collect_elapsed.as_secs_f64() * 1000.0,
// NEW:
prefetch_recv_elapsed.as_secs_f64() * 1000.0,
```

- [ ] **Step 7: Add update_span after adaptive sizing**

After the adaptive sizing block (around line 493), add:
```rust
prefetch.update_span(batch_block_span);
```

- [ ] **Step 8: Add shutdown after loop**

After the loop breaks (before the existing flush_channel close_and_wait), add:
```rust
let _prefetch_stats = prefetch.close_and_wait().await?;
```

This should go BEFORE the flush channel shutdown (which is around line 517).

- [ ] **Step 9: Remove unused variables**

Remove `disk_tracker` creation (line 111) — it will move to the sampler in the next task.
Remove the `prefetched_blocks` variable declaration if still present.
Clean up any unused `next_start`, `next_end`, `prefetch_handle`, `collect_started`, `prefetch_collect_elapsed` variables.

- [ ] **Step 10: Verify**

Run: `cargo check -p ckbadger-indexer`
Expected: Compiles with no errors.

Run: `cargo test -p ckbadger-indexer --lib`
Expected: All unit tests pass.

- [ ] **Step 11: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "perf(bulk-build): replace single-depth prefetch with PrefetchChannelHandle (depth 4)"
```

---

### Task 6: Integrate BackgroundSampler into the main loop

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:111,314-330`

- [ ] **Step 1: Read current code around lines 314-330**

Read the current `memory_stats()` and `read_batch_environment()` calls to confirm exact locations.

- [ ] **Step 2: Create sampler before the loop**

Near line 111 (where `disk_tracker` was), add:
```rust
let sampler = sampler::BackgroundSampler::new(
    indexer.writer.store().clone(),
    Duration::from_millis(200),
);
```

Remove the `disk_tracker` line if not already removed in Task 5.

- [ ] **Step 3: Replace per-iteration stats with sampler.latest()**

Replace lines 314-330 (the `memory_stats()` + `read_batch_environment()` + `BatchSample::new()` block):

```rust
// OLD:
// let perf_stats = indexer.writer.store().memory_stats();
// let batch_env = crate::sys_info::read_batch_environment(&mut disk_tracker);
// let mut sample = BatchSample::new(
//     batch_stats.block_count,
//     fetch_elapsed.as_secs_f64() + build_elapsed.as_secs_f64(),
//     0.0,
//     perf_stats.compaction_pending_bytes / (1024 * 1024),
//     perf_stats.l0_files_count,
//     perf_stats.immutable_memtables,
//     ...
//     batch_env.load_avg_1m,
//     batch_env.mem_available_mb,
//     batch_env.disk_read_mb,
//     batch_env.disk_write_mb,
// );

// NEW:
let snap = sampler.latest();
let mut sample = BatchSample::new(
    batch_stats.block_count,
    fetch_elapsed.as_secs_f64() + build_elapsed.as_secs_f64(),
    0.0,
    snap.compaction_pending_mb,
    snap.l0_files,
    snap.imm_memtables,
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string(),
    snap.load_avg_1m,
    snap.mem_available_mb,
    snap.disk_read_mb,
    snap.disk_write_mb,
);
```

- [ ] **Step 4: Shutdown sampler after finalization**

After the flush channel shutdown and finalization (after the existing `finalize_bulk_stage` code), add:
```rust
sampler.shutdown();
```

This must be AFTER flush_channel.close_and_wait() per the shutdown ordering spec.

- [ ] **Step 5: Clean up unused imports**

Remove any imports of `sys_info::DiskStatsTracker` or `sys_info::read_batch_environment` from the main loop file if they're no longer used there. Check if `memory_stats` import is still needed (it shouldn't be in the main loop anymore).

- [ ] **Step 6: Verify**

Run: `cargo check -p ckbadger-indexer`
Expected: Compiles.

Run: `cargo test -p ckbadger-indexer --lib`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "perf(bulk-build): replace inline memory_stats/sys_info with BackgroundSampler"
```

---

### Task 7: Final verification and clippy

**Files:** All changed files

- [ ] **Step 1: Run full check + clippy**

Run: `cargo check && cargo clippy`
Expected: No errors, no warnings on changed code.

- [ ] **Step 2: Run all Rust tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Run frontend checks (unchanged but verify)**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: No regressions.

- [ ] **Step 4: Commit any clippy fixes if needed**

```bash
git add -u
git commit -m "fix: address clippy warnings from prefetch/sampler changes"
```

Only commit if there were actual fixes. Skip if clean.
