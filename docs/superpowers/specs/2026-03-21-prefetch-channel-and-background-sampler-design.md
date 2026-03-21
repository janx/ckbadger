# Prefetch Channel & Background Sampler Design

**Date:** 2026-03-21
**Status:** Proposed
**Scope:** `crates/indexer/src/sync/bulk_build/mod.rs`, `crates/indexer/src/sync/bulk_build/materialize.rs`, `crates/indexer/src/bulk_sync_perf.rs`

## Problem

The bulk build main loop spends only 57% of wall clock on useful compute (795s build out of 1433s total). Two bottlenecks account for 36% of wasted time:

1. **Prefetch stalls (274s, 19%)**: Single-depth prefetch leaves the main loop idle when `fetch(N+1)` takes longer than `build(N)`. This happens on 69% of batches (467/677 have `prefetch_collect > 100ms`).

2. **Per-iteration overhead (242s, 17%)**: Every iteration runs `memory_stats()` (54 CFs × 6-8 RocksDB property reads), `read_batch_environment()` (3 `/proc` reads), and `write_metrics_file()` (rewrites entire metrics file from all accumulated samples — O(N²) total across the run). This adds ~357ms of dead time per iteration.

### Baseline

| Metric | Value |
|--------|------:|
| Wall clock | 1433s |
| Build compute | 795s (55%) |
| Prefetch stalls | 274s (19%) |
| Per-iteration overhead | 242s (17%) |
| Flush backpressure | 80s (6%) |
| Finalize | 42s (3%) |
| Blocks/sec | 13,185 |
| CPU utilization | 52% (12.6/24 cores) |

### Target

Reduce wall clock to ~1100-1150s by eliminating prefetch stalls and per-iteration overhead. Expected improvement: 20-25%.

---

## Optimization 1: PrefetchChannelHandle (depth=4)

### Design

A self-advancing prefetch worker that continuously fetches blocks and buffers up to 4 results. Mirrors the proven `FlushChannelHandle` pattern.

```
Main loop                              Prefetch Worker (spawn_blocking)
                                       ┌──────────────────────────────┐
                                       │ position = start_block       │
                                       │ loop:                        │
 recv() ◄──── result_channel(4) ◄──────│   span = *span_rx.borrow()  │
                                       │   end = min(pos+span, target)│
 span_tx.send(new_span) ──────────────►│   blocks = fetch(pos, end)  │
                                       │   result_tx.send(blocks)     │
                                       │   position = end + 1        │
                                       └──────────────────────────────┘
```

The worker:
- Owns its position counter and advances based on what it actually fetched (same `effective_end` logic as current code)
- Reads the latest `batch_block_span` from a `watch::Receiver<u64>` before each fetch
- Runs inside `tokio::task::spawn_blocking` with a blocking loop
- Exits when `result_tx.blocking_send()` fails (receiver dropped) or position exceeds `handoff_target`

The main loop:
- Calls `prefetch.recv().await` to get the next batch (blocks when pipeline is empty, instant when data is buffered)
- Calls `prefetch.update_span(batch_block_span)` after adaptive sizing runs
- No longer manages `prefetched_blocks: Option<...>` or spawns per-iteration `JoinHandle`s

### Stale Boundary Analysis

The worker reads `span_rx.borrow()` immediately before each fetch, so spans are at most 1 fetch stale. Three reasons this is acceptable:

1. **Slow adaptive change**: EMA alpha=0.5 with 2x max step ratio and 10k-100k clamp. Over 1 fetch lag, span changes <5% in steady state.

2. **`effective_end` correction**: The main loop processes whatever blocks it receives and moves forward. Over/under-sized batches self-correct within 1-2 iterations of the adaptive controller.

3. **Startup convergence**: Initial batches use `configured_batch_size` (10k). The worker fills the pipeline with 4 small batches while adaptive sizing warms up. The total blocks in those first 4 batches (~40k) are negligible vs 18.9M total.

### `handoff_target` Freshness

During a fresh sync, chain tip advances ~1 block/8s (~180 blocks over 1433s). The worker uses the initial `handoff_target`. When the worker exhausts its range and exits, the main loop handles any remaining blocks via the existing loop condition (`start_block > handoff_target`). If there are tail blocks, the main loop falls back to the synchronous fetch path. In practice, 180 blocks is less than one batch.

### API

```rust
const PREFETCH_DEPTH: usize = 4;

pub(crate) struct PrefetchResult {
    pub blocks: Vec<RawCkbBlock>,
    pub fetch_elapsed: Duration,
    pub effective_end: u64,
}

pub(crate) struct PrefetchWorkerStats {
    pub total_fetches: u64,
    pub total_blocks: u64,
}

pub(crate) struct PrefetchChannelHandle {
    result_rx: tokio::sync::mpsc::Receiver<PrefetchResult>,
    span_tx: tokio::sync::watch::Sender<u64>,
    worker_handle: tokio::task::JoinHandle<Result<PrefetchWorkerStats>>,
}

impl PrefetchChannelHandle {
    /// Create the channel and spawn the prefetch worker.
    ///
    /// The worker immediately begins fetching from `start_block` using
    /// `initial_span` and buffers up to `depth` results. The main loop
    /// consumes results via `recv()` and updates the span via
    /// `update_span()`.
    pub(crate) fn new(
        depth: usize,
        ckb_store: CkbChainReader,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
        initial_span: u64,
    ) -> Self {
        let (result_tx, result_rx) = tokio::sync::mpsc::channel(depth);
        let (span_tx, span_rx) = tokio::sync::watch::channel(initial_span);

        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::prefetch_worker(
                result_tx, span_rx, ckb_store, fetch_pool,
                start_block, handoff_target,
            )
        });

        Self { result_rx, span_tx, worker_handle }
    }

    fn prefetch_worker(
        result_tx: tokio::sync::mpsc::Sender<PrefetchResult>,
        span_rx: tokio::sync::watch::Receiver<u64>,
        ckb_store: CkbChainReader,
        fetch_pool: Arc<rayon::ThreadPool>,
        start_block: u64,
        handoff_target: u64,
    ) -> Result<PrefetchWorkerStats> {
        let mut stats = PrefetchWorkerStats { total_fetches: 0, total_blocks: 0 };
        let mut position = start_block;

        while position <= handoff_target {
            // Read latest span (non-blocking, always succeeds while sender alive)
            let span = *span_rx.borrow();
            let end = std::cmp::min(
                position.saturating_add(span.saturating_sub(1)),
                handoff_target,
            );

            let started = std::time::Instant::now();
            let blocks = Indexer::fetch_blocks_direct_binary(
                &ckb_store, position, end, Some(&fetch_pool),
            )?;
            let block_count = blocks.len() as u64;
            let fetch_elapsed = started.elapsed();

            let result = PrefetchResult {
                blocks,
                fetch_elapsed,
                effective_end: end,
            };

            if result_tx.blocking_send(result).is_err() {
                break; // main loop dropped receiver
            }

            stats.total_fetches += 1;
            stats.total_blocks += block_count;
            position = end.saturating_add(1);
        }

        Ok(stats)
    }

    /// Receive next batch. Blocks if pipeline is empty (worker still fetching).
    /// Returns instantly if buffered results are available.
    pub(crate) async fn recv(&mut self) -> Result<PrefetchResult> {
        self.result_rx
            .recv()
            .await
            .ok_or_else(|| anyhow!("prefetch worker terminated unexpectedly"))
    }

    /// Update batch span. The worker picks this up before its next fetch.
    pub(crate) fn update_span(&self, span: u64) {
        let _ = self.span_tx.send(span);
    }

    /// Number of results buffered and ready to consume (0..=depth).
    pub(crate) fn buffered(&self) -> usize {
        // mpsc receiver doesn't expose this directly; track via
        // capacity math if needed, or omit from initial implementation.
        0 // placeholder
    }

    /// Shut down: drop receiver to signal worker, then join.
    pub(crate) async fn close_and_wait(self) -> Result<PrefetchWorkerStats> {
        drop(self.result_rx);
        drop(self.span_tx);
        self.worker_handle
            .await
            .map_err(|e| anyhow!("prefetch worker panicked: {}", e))?
    }
}
```

### Main Loop Changes

Before (current):
```rust
let mut prefetched_blocks: Option<(Vec<RawCkbBlock>, Duration, u64)> = None;

loop {
    // Use prefetched or sync fetch
    let (blocks, fetch_elapsed, effective_end) =
        if let Some((blocks, elapsed, end)) = prefetched_blocks.take() {
            (blocks, elapsed, end)
        } else {
            // sync fetch (first iteration)
        };

    // Spawn single prefetch for next batch
    let prefetch_handle = if next_start <= handoff_target {
        Some(tokio::task::spawn_blocking(move || { ... }))
    } else { None };

    // Build
    let build_started = Instant::now();
    let (batch_stats, build_timings, pending_flush) = runtime.apply_blocks(&blocks, ...)?;
    let build_elapsed = build_started.elapsed();

    // Collect prefetch (may block)
    let collect_started = Instant::now();
    if let Some(handle) = prefetch_handle {
        prefetched_blocks = Some(handle.await??);
    }
    let prefetch_collect_elapsed = collect_started.elapsed();

    let controllable_ms = (build_elapsed + prefetch_collect_elapsed).as_secs_f64() * 1000.0;

    // ... adaptive sizing updates batch_block_span ...
}
```

After:
```rust
let mut prefetch = PrefetchChannelHandle::new(
    PREFETCH_DEPTH,
    ckb_store.clone(),
    Arc::clone(&fetch_pool),
    0,               // start_block for fresh sync
    handoff_target,
    configured_batch_size,
);

loop {
    // Receive next batch from pipeline
    let recv_started = Instant::now();
    let prefetch_result = prefetch.recv().await?;
    let prefetch_recv_elapsed = recv_started.elapsed();

    let (blocks, fetch_elapsed, effective_end) =
        (prefetch_result.blocks, prefetch_result.fetch_elapsed, prefetch_result.effective_end);

    // Build (unchanged)
    let build_started = Instant::now();
    let (batch_stats, build_timings, pending_flush) = runtime.apply_blocks(&blocks, ...)?;
    let build_elapsed = build_started.elapsed();

    // controllable_ms: build + recv wait (same role as before)
    let controllable_ms = (build_elapsed + prefetch_recv_elapsed).as_secs_f64() * 1000.0;

    // ... flush, perf recording, adaptive sizing ...

    // Feed updated span back to worker
    prefetch.update_span(batch_block_span);
}

// After loop: shut down prefetch worker
let prefetch_stats = prefetch.close_and_wait().await?;
```

### Perf Sample Changes

- `prefetch_collect_ms` → `prefetch_recv_ms` (measures `recv()` wait; 0 when pipeline has data, >0 when drained)
- Add `prefetch_buffered` field (number of results ready in channel)
- `prefetch_depth` replaces `flush_channel_depth` naming convention (add `prefetch_depth` constant to sample)

### CKB Store Refresh

The current code calls `ckb_store.refresh()` at the top of each loop iteration to pick up new chain tip data. With the prefetch worker holding its own `CkbChainReader` clone, the worker's store view may be stale. This is acceptable because:
- The worker fetches historical blocks (0 to handoff_target) that don't change
- `refresh()` only matters for detecting tip advancement
- The main loop still refreshes its own store for `chain_tip` / `handoff_target` checks

If the CkbChainReader requires periodic refresh for RocksDB secondary catchup, the worker should call `refresh()` periodically (e.g., every 100 fetches). This is a minor detail to verify during implementation.

---

## Optimization 2: BackgroundSampler

### Design

A dedicated `std::thread` that periodically samples RocksDB stats and system environment. The main loop reads cached values via a `watch` channel.

```
BackgroundSampler thread               Main loop
┌───────────────────────────┐
│ loop:                     │
│   sleep(200ms)            │
│   stats = memory_stats()  │           let snap = sampler.latest();  // <1μs
│   env = read_batch_env()  │           sample.l0_files = snap.l0_files;
│   watch_tx.send(snapshot) ──────────► sample.compaction_pending_mb = ...
│                           │
│   if shutdown: break      │
└───────────────────────────┘
```

### Why `std::thread` Instead of `spawn_blocking`

`memory_stats()` can take 100-200ms per call (54 CFs × property queries). Using `tokio::task::spawn_blocking` would tie up a thread from the blocking thread pool. A dedicated `std::thread` avoids contending with other `spawn_blocking` tasks (prefetch worker, flush worker).

### API

```rust
#[derive(Clone, Default)]
pub(crate) struct SamplerSnapshot {
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
}

pub(crate) struct BackgroundSampler {
    latest_rx: tokio::sync::watch::Receiver<SamplerSnapshot>,
    shutdown: Arc<AtomicBool>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl BackgroundSampler {
    /// Spawn the sampler thread. It immediately begins sampling at `interval`.
    pub(crate) fn new(
        store: Arc<CkbadgerStore>,
        interval: Duration,
    ) -> Self {
        let (tx, rx) = tokio::sync::watch::channel(SamplerSnapshot::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let handle = std::thread::Builder::new()
            .name("bg-sampler".into())
            .spawn(move || {
                let mut disk_tracker = crate::sys_info::DiskStatsTracker::new(String::new());
                while !shutdown_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    let stats = store.memory_stats();
                    let env = crate::sys_info::read_batch_environment(&mut disk_tracker);
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
                        break; // receiver dropped
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
        // Don't join in Drop (could block), thread will exit on next wake.
    }
}
```

### Main Loop Changes

Before (current):
```rust
// Per-iteration (lines 314-315):
let perf_stats = indexer.writer.store().memory_stats();     // ~100-200ms
let batch_env = crate::sys_info::read_batch_environment(&mut disk_tracker);  // ~5-10ms

let mut sample = BatchSample::new(
    batch_stats.block_count,
    fetch_elapsed.as_secs_f64() + build_elapsed.as_secs_f64(),
    0.0,
    perf_stats.compaction_pending_bytes / (1024 * 1024),
    perf_stats.l0_files_count,
    perf_stats.immutable_memtables,
    timestamp_utc,
    batch_env.load_avg_1m,
    batch_env.mem_available_mb,
    batch_env.disk_read_mb,
    batch_env.disk_write_mb,
);
```

After:
```rust
// Before loop: create sampler
let sampler = BackgroundSampler::new(
    indexer.writer.store().clone(),
    Duration::from_millis(200),
);

// Per-iteration (replaces lines 314-315):
let snap = sampler.latest();  // <1μs

let mut sample = BatchSample::new(
    batch_stats.block_count,
    fetch_elapsed.as_secs_f64() + build_elapsed.as_secs_f64(),
    0.0,
    snap.compaction_pending_mb,
    snap.l0_files,
    snap.imm_memtables,
    timestamp_utc,
    snap.load_avg_1m,
    snap.mem_available_mb,
    snap.disk_read_mb,
    snap.disk_write_mb,
);

// After loop: shut down sampler
sampler.shutdown();
```

### metrics.env O(N²) Fix

The current `write_metrics_file()` re-iterates all accumulated `batch_samples` every call to recompute percentiles and aggregates. With 677 batches, this is O(N) per call × N calls = O(N²) total.

**Fix**: Move `write_metrics_file()` out of the per-batch `record_batch_sample()` path. Write it only:
1. At run completion (inside finalization)
2. Optionally on a periodic timer (e.g., every 30s) for liveness, driven by the background sampler or a separate timer

The per-batch path keeps only the O(1) JSONL append:
```rust
// In record_batch_sample():
fn record_batch_sample(&mut self, sample: BatchSample) {
    self.batch_samples.push(sample);
    self.append_sample_to_jsonl(&self.batch_samples.last().unwrap());
    // REMOVED: self.write_metrics_file();
}
```

### Sampling Interval Rationale

200ms interval means ~5 samples/second. With avg batch time ~2.1s (build_elapsed), the sampler produces ~10 samples per batch. The main loop reads the latest value, which is at most 200ms stale. For RocksDB pressure metrics (L0 files, compaction pending), this staleness is negligible — these counters change on the timescale of seconds, not milliseconds.

### DiskStatsTracker Ownership

`DiskStatsTracker` currently lives in the main loop (`mod.rs:111`). Since it tracks deltas between reads, it must move to the sampler thread. The main loop no longer needs it. The sampler thread creates its own `DiskStatsTracker` internally.

---

## Combined Impact

### Expected Wall Clock

| Component | Before | After | Savings |
|-----------|-------:|------:|--------:|
| Build compute | 795s | 795s | 0s |
| Prefetch stalls | 274s | ~30-50s | ~225-245s |
| Per-iteration overhead | 242s | ~10-20s | ~220-230s |
| Flush backpressure | 80s | 80s | 0s |
| Finalize | 42s | 42s | 0s |
| **Total** | **1433s** | **~960-990s** | **~440-470s (31-33%)** |

### Expected Throughput

| Metric | Before | After |
|--------|-------:|------:|
| Wall clock | 1433s | ~960-990s |
| Blocks/sec | 13,185 | ~19,000-19,700 |
| Txs/sec | 34,222 | ~49,500-51,000 |

### Memory Impact

- Prefetch depth=4 buffers: ~4 × (batch blocks × ~2KB/block). At avg 28k blocks/batch ≈ 4 × 56MB = ~224MB additional peak. Acceptable given 95GB RAM and 47GB available at peak.
- BackgroundSampler: negligible (one `SamplerSnapshot` in watch channel).

### Risk

- **Low**: Both optimizations are additive (new worker threads), not changes to the compute path. Build correctness is unaffected.
- **Measurable**: Both produce perf sample fields (`prefetch_recv_ms`, `prefetch_buffered`) for post-hoc analysis.
- **Rollback**: Revert to depth=1 prefetch or inline sampling by changing constants / removing the workers.

---

## Testing Plan

### PrefetchChannelHandle

1. **Unit: basic flow** — Create handle with depth=2, mock fetch function, send/recv 5 results, verify order and effective_end continuity.
2. **Unit: span update** — Verify worker picks up updated span after `update_span()` call.
3. **Unit: close_and_wait** — Verify clean shutdown: drop receiver, worker exits, stats returned.
4. **Unit: worker exit on target reached** — Worker stops when position > handoff_target, main loop gets `None` on recv.
5. **Integration: full bulk build** — Run bulk build with prefetch depth=4 on testnet or small block range, verify identical DB output to depth=1 baseline.

### BackgroundSampler

1. **Unit: snapshot updates** — Create sampler with 50ms interval, sleep 200ms, verify `latest()` returns non-default values.
2. **Unit: shutdown** — Verify `shutdown()` joins the thread within 1 interval.
3. **Unit: receiver drop** — Drop the sampler, verify thread exits.
4. **Integration: perf sample correctness** — Verify perf JSONL samples still contain valid L0/compaction/load data (values may be slightly stale but within reasonable range).

### metrics.env Fix

1. **Unit: write_metrics_file at finalize** — Verify metrics.env is written after `close()` call with correct aggregate stats.
2. **Regression: JSONL completeness** — Verify all batch samples are still appended to samples.jsonl per-iteration.

---

## Files Changed

| File | Change |
|------|--------|
| `crates/indexer/src/sync/bulk_build/materialize.rs` | Add `PrefetchChannelHandle`, `PrefetchResult`, `PrefetchWorkerStats` |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Replace prefetch logic with channel, replace inline stats with sampler, remove `disk_tracker` |
| `crates/indexer/src/bulk_sync_perf.rs` | Rename `prefetch_collect_ms` → `prefetch_recv_ms`, add `prefetch_buffered`, `prefetch_depth` fields; remove `write_metrics_file()` from `record_batch_sample()`; add `write_metrics_file()` to finalization path |
| `crates/indexer/src/sync/bulk_build/sampler.rs` | New file: `BackgroundSampler`, `SamplerSnapshot` |
| `crates/indexer/src/sync/diagnostics.rs` | Update `record_batch()` atomics for renamed fields |
| `crates/tui/src/ui.rs` | Update field reads for renamed prefetch metrics |
