// Bottleneck-driven resource controller for bulk sync.
//
// Measures where time is spent per batch iteration (fetch wait, build CPU,
// flush wait) and shifts resources toward the bottleneck stage.  Replaces
// the previous multi-mechanism adaptive system (ThroughputController,
// AdaptiveBatchController, prefetch disk throttle, cooldown/streak logic)
// with a single controller that detects the actual bottleneck and responds.
//
// Three knobs, three signals:
//
//   Signal                        │ Knob               │ What it controls
//   ──────────────────────────────│────────────────────│──────────────────────────
//   prefetch_recv_ms              │ prefetch_ahead     │ Fetch/build overlap depth
//   build_ms                      │ batch_span         │ Work volume per iteration
//   flush_wait + L0 + channel fill│ bg_jobs            │ RocksDB compaction threads
//
// Key design principle: fetch (CKB RocksDB reads via std::thread::scope)
// does NOT compete with build (CPU via rayon) for resources.  Therefore
// Build-classified batches must NOT suppress prefetch_ahead or
// fetch_threads.  Only Flush suppresses prefetch — and only when the
// flush channel is actually filling up.

const EMA_ALPHA: f64 = 0.5;

// Batch span bounds (blocks)
pub(crate) const MIN_SPAN: u64 = 10_000;
pub(crate) const MAX_SPAN: u64 = 300_000;

// Channel depth bounds (batches).  Max is computed from system RAM at
// startup to cap total buffered data.  Each slot holds one batch of raw
// blocks (prefetch) or materialized rows (flush); per-slot size scales
// with batch_span.  Absolute ceiling of 8 prevents runaway memory on
// very large machines.
const MIN_CHANNEL_DEPTH: u64 = 1;
const ABSOLUTE_MAX_CHANNEL_DEPTH: u64 = 8;
const INITIAL_PREFETCH_RATIO: u64 = 2; // initial = max / 2

// Prefetch ahead bounds
const MIN_PREFETCH: u64 = 1;

// Fetch thread bounds
const MIN_FETCH_THREADS: u32 = 2;

// Background jobs bounds
const MIN_BG_JOBS: i32 = 2;

// Bottleneck classification thresholds (fraction of total iteration time).
// Flush checked first (compound risk from L0 buildup).
const FLUSH_PCT_THRESHOLD: f64 = 0.4;
const FETCH_PCT_THRESHOLD: f64 = 0.5;
// L0 threshold for proactive bg_jobs increase.  When L0 is above this but
// the flush channel still has room, we bump bg_jobs to help compaction
// catch up — without suppressing the pipeline (no Flush classification).
const FLUSH_L0_THRESHOLD: f64 = 40.0;
// Flush channel fill ratio above this → flush is falling behind.  This is
// a more direct signal than L0 or flush_wait (which only fires when the
// channel is completely full).
const FLUSH_CHANNEL_FILL_THRESHOLD: f64 = 0.75;

// Immutable memtable ratio thresholds.  When imm_memtables exceeds
// moderate_threshold, memtable flushes can't keep up with writes — the
// process is accumulating memory that will never be reclaimed until
// flush completes.  This is the leading indicator of OOM: the flush
// channel may have room (no flush_wait) but RocksDB is silently
// piling up memtables in process RSS.
const IMM_MEMTABLE_MODERATE_RATIO: f64 = 0.5;

// Per-step span change safety bounds.  These limit how much batch_span
// can change in a single iteration, preventing chaotic overshooting.
// The actual step factor is derived from signal strength (dominance ratio),
// not from fixed constants.
const SPAN_STEP_MIN: f64 = 0.5;
const SPAN_STEP_MAX: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bottleneck {
    Fetch,
    Build,
    Flush,
}

impl Bottleneck {
    /// Encode as u8 for atomic storage: 1=Fetch, 2=Build, 3=Flush.
    pub(crate) fn to_code(self) -> u8 {
        match self {
            Self::Fetch => 1,
            Self::Build => 2,
            Self::Flush => 3,
        }
    }

    /// Decode from u8. Returns None for unknown codes.
    #[allow(dead_code)] // used by TUI reader in a later task
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Fetch),
            2 => Some(Self::Build),
            3 => Some(Self::Flush),
            _ => None,
        }
    }
}

impl std::fmt::Display for Bottleneck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch => write!(f, "fetch"),
            Self::Build => write!(f, "build"),
            Self::Flush => write!(f, "flush"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BatchSignals {
    pub prefetch_recv_ms: f64,
    pub build_ms: f64,
    pub flush_wait_ms: f64,
    pub l0_files: u64,
    pub flush_channel_pending: u64,
    pub flush_channel_capacity: u64,
    pub cell_count: u64,
    pub block_count: u64,
    /// Current number of immutable memtables across all column families.
    /// This is a leading indicator of memory pressure: high values mean
    /// RocksDB flushes can't keep up with writes, and process RSS is
    /// growing uncontrollably.
    pub imm_memtables: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ControllerOutput {
    pub batch_span: u64,
    pub prefetch_ahead: u64,
    pub fetch_threads: u32,
    pub bg_jobs: i32,
    pub bottleneck: Bottleneck,
    pub recv_ema: f64,
    pub build_ema: f64,
    pub wait_ema: f64,
    pub l0_ema: f64,
    pub flush_fill_ema: f64,
    pub density_ema: f64,
    pub imm_ema: f64,
}

#[derive(Debug)]
pub(crate) struct BottleneckController {
    // EMAs
    recv_ema: f64,
    build_ema: f64,
    wait_ema: f64,
    l0_ema: f64,
    flush_fill_ema: f64,
    density_ema: f64,
    imm_ema: f64,

    // Current outputs
    batch_span: u64,
    prefetch_ahead: u64,
    fetch_threads: u32,
    bg_jobs: i32,

    // Bounds
    max_channel_depth: u64,
    max_fetch_threads: u32,
    min_bg_jobs: i32,
    max_bg_jobs: i32,

    // Immutable memtable threshold from MemoryProfile.  When imm_ema
    // exceeds this, classify as Flush regardless of timing signals.
    moderate_imm_threshold: u64,

    // State
    batch_count: u64,
    prev_bg_jobs: i32,
}

impl BottleneckController {
    pub(crate) fn new(
        initial_span: u64,
        max_fetch_threads: u32,
        max_bg_jobs: i32,
        system_ram_bytes: u64,
        moderate_imm_threshold: u64,
    ) -> Self {
        let max_bg_jobs = max_bg_jobs.max(MIN_BG_JOBS);
        let min_bg_jobs = (max_bg_jobs / 4).max(MIN_BG_JOBS);
        let max_fetch_threads = max_fetch_threads.max(MIN_FETCH_THREADS);
        let max_channel_depth = channel_depth_for_ram(system_ram_bytes);
        let initial_prefetch = (max_channel_depth / INITIAL_PREFETCH_RATIO)
            .max(MIN_PREFETCH)
            .min(max_channel_depth);
        Self {
            recv_ema: 0.0,
            build_ema: 0.0,
            wait_ema: 0.0,
            l0_ema: 0.0,
            flush_fill_ema: 0.0,
            density_ema: 0.0,
            imm_ema: 0.0,

            batch_span: initial_span.clamp(MIN_SPAN, MAX_SPAN),
            prefetch_ahead: initial_prefetch,
            fetch_threads: max_fetch_threads,
            bg_jobs: max_bg_jobs,

            max_channel_depth,
            max_fetch_threads,
            min_bg_jobs,
            max_bg_jobs,
            moderate_imm_threshold,

            batch_count: 0,
            prev_bg_jobs: max_bg_jobs,
        }
    }

    /// Feed one batch's timing signals and get updated resource allocation.
    ///
    /// Returns `None` for the first batch (warmup, not representative).
    pub(crate) fn observe(&mut self, signals: &BatchSignals) -> Option<ControllerOutput> {
        self.batch_count += 1;

        // Skip first batch (warmup).
        if self.batch_count <= 1 {
            return None;
        }

        // Update timing EMAs.
        self.recv_ema = ema(self.recv_ema, signals.prefetch_recv_ms);
        self.build_ema = ema(self.build_ema, signals.build_ms);
        self.wait_ema = ema(self.wait_ema, signals.flush_wait_ms);
        self.l0_ema = ema(self.l0_ema, signals.l0_files as f64);

        // Track flush channel fill ratio (0.0 = empty, 1.0 = full).
        let fill_ratio = if signals.flush_channel_capacity > 0 {
            signals.flush_channel_pending as f64 / signals.flush_channel_capacity as f64
        } else {
            0.0
        };
        self.flush_fill_ema = ema(self.flush_fill_ema, fill_ratio);

        // Track immutable memtable pressure (0.0 = none, 1.0 = at threshold).
        self.imm_ema = ema(self.imm_ema, signals.imm_memtables as f64);

        // Classify bottleneck.
        let bottleneck = self.classify();

        // Adjust knobs.  Span step is proportional to the bottleneck
        // signal's dominance — stronger signal → larger adjustment,
        // weak signal near classification boundary → gentle nudge.
        let total = self.recv_ema + self.build_ema + self.wait_ema;
        match bottleneck {
            Bottleneck::Fetch => {
                self.prefetch_ahead = (self.prefetch_ahead + 1).min(self.max_channel_depth);
                self.fetch_threads = grow_threads(self.fetch_threads, self.max_fetch_threads);
                let fetch_dominance = if total > 0.0 {
                    self.recv_ema / total
                } else {
                    0.0
                };
                let factor = (1.0 + fetch_dominance).clamp(SPAN_STEP_MIN, SPAN_STEP_MAX);
                self.batch_span = ((self.batch_span as f64 * factor) as u64).min(MAX_SPAN);
                self.bg_jobs = (self.bg_jobs - 1).max(self.min_bg_jobs);
            }
            Bottleneck::Build => {
                // Build-bound: do not adjust batch_span.  Growing span
                // increases per-batch build time, which increases flush
                // burstiness and slows controller responsiveness without
                // meaningfully improving throughput.  Fetch resources are
                // left untouched — fetch uses independent std::thread::scope
                // threads and does not compete with build CPU (rayon pool).
            }
            Bottleneck::Flush => {
                // Flush-bound: reduce read I/O (prefetch + fetch_threads) to
                // yield disk bandwidth to compaction.  Shrink factor derived
                // from the stronger of the two flush signals (time share or
                // channel fill ratio).
                self.prefetch_ahead = (self.prefetch_ahead - 1).max(MIN_PREFETCH);
                self.fetch_threads = shrink_threads(self.fetch_threads);
                let flush_dominance = if total > 0.0 {
                    (self.wait_ema / total).max(self.flush_fill_ema)
                } else {
                    self.flush_fill_ema
                };
                let factor = (1.0 - flush_dominance).clamp(SPAN_STEP_MIN, SPAN_STEP_MAX);
                self.batch_span = ((self.batch_span as f64 * factor) as u64).max(MIN_SPAN);
                self.bg_jobs = (self.bg_jobs + 1).min(self.max_bg_jobs);
            }
        }

        // Proactive bg_jobs: if L0 is building up but the flush channel still
        // has room (not classified as Flush), bump bg_jobs to help compaction
        // catch up — without suppressing the pipeline.
        if bottleneck != Bottleneck::Flush && self.l0_ema > FLUSH_L0_THRESHOLD {
            self.bg_jobs = (self.bg_jobs + 1).min(self.max_bg_jobs);
        }

        // Final clamp.
        self.batch_span = self.batch_span.clamp(MIN_SPAN, MAX_SPAN);
        self.prefetch_ahead = self
            .prefetch_ahead
            .clamp(MIN_PREFETCH, self.max_channel_depth);
        self.bg_jobs = self.bg_jobs.clamp(self.min_bg_jobs, self.max_bg_jobs);

        // Cell-density correction: scale span inversely with density
        // change so the next batch targets consistent cell volume, not
        // block count.  Without this, entering a high-density chain
        // region causes a build_ms spike that takes multiple iterations
        // to recover from (the time-based logic reacts one batch late).
        let batch_density = if signals.block_count > 0 {
            signals.cell_count as f64 / signals.block_count as f64
        } else {
            0.0
        };
        if batch_density > 0.0 {
            let prev = self.density_ema;
            self.density_ema = if prev > 0.0 {
                ema(prev, batch_density)
            } else {
                batch_density // initialize on first batch with cells
            };
            if prev > 0.0 {
                // prev / new_ema: density increased → ratio < 1 → shrink span
                //                 density decreased → ratio > 1 → grow span
                let ratio = prev / self.density_ema;
                self.batch_span =
                    ((self.batch_span as f64 * ratio) as u64).clamp(MIN_SPAN, MAX_SPAN);
            }
        }

        Some(ControllerOutput {
            batch_span: self.batch_span,
            prefetch_ahead: self.prefetch_ahead,
            fetch_threads: self.fetch_threads,
            bg_jobs: self.bg_jobs,
            bottleneck,
            recv_ema: self.recv_ema,
            build_ema: self.build_ema,
            wait_ema: self.wait_ema,
            l0_ema: self.l0_ema,
            flush_fill_ema: self.flush_fill_ema,
            density_ema: self.density_ema,
            imm_ema: self.imm_ema,
        })
    }

    fn classify(&self) -> Bottleneck {
        let total = self.recv_ema + self.build_ema + self.wait_ema;
        if total < 1.0 {
            return Bottleneck::Build;
        }

        let flush_pct = self.wait_ema / total;
        let fetch_pct = self.recv_ema / total;

        // Immutable memtable pressure: when imm_ema exceeds the moderate
        // threshold, RocksDB memtable flushes can't keep up with writes.
        // This is a *leading* indicator of OOM — the flush channel may
        // still have room (no flush_wait), but process RSS is growing
        // because memtable memory cannot be reclaimed until flush I/O
        // completes.  Classify as Flush to reduce batch_span and yield
        // disk bandwidth to RocksDB flush/compaction.
        let imm_stressed = if self.moderate_imm_threshold > 0 {
            let imm_ratio = self.imm_ema / self.moderate_imm_threshold as f64;
            imm_ratio > IMM_MEMTABLE_MODERATE_RATIO
        } else {
            false
        };

        // Flush stressed when the flush channel shows actual backpressure:
        //  - flush_wait dominates batch time (channel was full, writer blocked)
        //  - flush channel filling up (consumer falling behind producer)
        //  - immutable memtables accumulating (memory pressure, leading OOM indicator)
        //
        // L0 pileup alone does NOT trigger Flush classification.  High L0
        // with an empty flush channel means RocksDB compaction is slow but
        // the pipeline has no backpressure — suppressing fetch would starve
        // the pipeline for no reason.  Instead, L0 is handled separately
        // via proactive bg_jobs bumping in observe().
        let flush_stressed = flush_pct > FLUSH_PCT_THRESHOLD
            || self.flush_fill_ema > FLUSH_CHANNEL_FILL_THRESHOLD
            || imm_stressed;
        if flush_stressed {
            return Bottleneck::Flush;
        }
        if fetch_pct > FETCH_PCT_THRESHOLD {
            return Bottleneck::Fetch;
        }
        Bottleneck::Build
    }

    pub(crate) fn batch_span(&self) -> u64 {
        self.batch_span
    }

    pub(crate) fn prefetch_ahead(&self) -> u64 {
        self.prefetch_ahead
    }

    pub(crate) fn fetch_threads(&self) -> u32 {
        self.fetch_threads
    }

    /// Max channel depth for both prefetch and flush channels, computed
    /// from system RAM at startup.
    pub(crate) fn channel_depth(&self) -> u64 {
        self.max_channel_depth
    }

    /// Returns (current_bg_jobs, changed) where `changed` is true only if
    /// bg_jobs differs from the previous call's value.
    pub(crate) fn bg_jobs_if_changed(&mut self) -> Option<i32> {
        if self.bg_jobs != self.prev_bg_jobs {
            self.prev_bg_jobs = self.bg_jobs;
            Some(self.bg_jobs)
        } else {
            None
        }
    }
}

fn ema(current: f64, sample: f64) -> f64 {
    current * (1.0 - EMA_ALPHA) + sample * EMA_ALPHA
}

/// +25% per step (minimum step of 1).
fn grow_threads(t: u32, max: u32) -> u32 {
    (t + (t / 4).max(1)).min(max)
}

/// -25% per step (minimum step of 1).
fn shrink_threads(t: u32) -> u32 {
    t.saturating_sub((t / 4).max(1)).max(MIN_FETCH_THREADS)
}

/// Compute max channel depth (for both prefetch and flush) from system RAM.
///
/// Budget: ~2GB per channel slot at peak (raw blocks + pending rows).
/// Halved to leave room for RocksDB + in-memory build structures.
///
///   RAM      depth
///   ≤16 GB   2
///   32 GB    4
///   64 GB    8
///   128 GB   8 (capped)
fn channel_depth_for_ram(system_ram_bytes: u64) -> u64 {
    const GB: u64 = 1024 * 1024 * 1024;
    let depth = system_ram_bytes / (8 * GB);
    depth.clamp(MIN_CHANNEL_DEPTH, ABSOLUTE_MAX_CHANNEL_DEPTH)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    /// Default moderate_imm_threshold for tests (matching 32GB profile).
    const TEST_MODERATE_IMM: u64 = 30;

    fn healthy_signals() -> BatchSignals {
        BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        }
    }

    #[test]
    fn first_batch_returns_none() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        let output = ctrl.observe(&healthy_signals());
        assert!(output.is_none());
    }

    #[test]
    fn fetch_starved_grows_prefetch_and_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        let initial_span = ctrl.batch_span;
        let initial_prefetch = ctrl.prefetch_ahead;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Fetch);
        assert!(ctrl.prefetch_ahead > initial_prefetch);
        assert!(ctrl.batch_span > initial_span);
    }

    #[test]
    fn flush_pressure_shrinks_span_grows_bg_jobs() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        ctrl.bg_jobs = 4;
        ctrl.prev_bg_jobs = 4;
        let initial_span = ctrl.batch_span;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 2000.0,
                flush_wait_ms: 3000.0,
                l0_files: 60,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
        assert!(ctrl.batch_span < initial_span);
        assert!(ctrl.bg_jobs > 4);
    }

    #[test]
    fn build_bound_does_not_adjust_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        let initial_span = ctrl.batch_span;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 100.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Build);
        assert_eq!(
            ctrl.batch_span, initial_span,
            "build-bound must not adjust span: {} vs initial {}",
            ctrl.batch_span, initial_span
        );
    }

    #[test]
    fn build_bound_does_not_shrink_prefetch_or_fetch_threads() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        let initial_prefetch = ctrl.prefetch_ahead;
        let initial_threads = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Sustained build-bound: prefetch and fetch_threads must NOT decrease.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Build);
        assert!(
            ctrl.prefetch_ahead >= initial_prefetch,
            "build-bound must not shrink prefetch_ahead: {} < initial {}",
            ctrl.prefetch_ahead,
            initial_prefetch
        );
        assert!(
            ctrl.fetch_threads >= initial_threads,
            "build-bound must not shrink fetch_threads: {} < initial {}",
            ctrl.fetch_threads,
            initial_threads
        );
    }

    #[test]
    fn span_bounds_enforced() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..100 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 100,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert!(ctrl.batch_span >= MIN_SPAN);
    }

    #[test]
    fn bg_jobs_bounds_enforced() {
        let mut ctrl = BottleneckController::new(50_000, 12, 4, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        // Fetch-bound: bg_jobs should shrink but not below min.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }
        assert!(ctrl.bg_jobs >= MIN_BG_JOBS);

        // Flush-bound: bg_jobs should grow but not above max (4).
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 80,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }
        assert!(ctrl.bg_jobs <= 4);
    }

    #[test]
    fn fetch_threads_stable_when_build_bound_grow_when_fetch_bound() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        let initial = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Build-bound: fetch_threads must NOT shrink (fetch uses independent
        // threads, does not compete with build CPU).
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }
        assert_eq!(
            ctrl.fetch_threads, initial,
            "build-bound must not change fetch_threads"
        );

        // Flush-bound: fetch_threads should shrink (yield disk to compaction).
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 80,

                flush_channel_pending: 7,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }
        assert!(
            ctrl.fetch_threads < initial,
            "flush-bound should shrink fetch_threads: {} vs initial {}",
            ctrl.fetch_threads,
            initial
        );

        let after_flush = ctrl.fetch_threads;

        // Fetch-bound: fetch_threads should grow back.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }
        assert!(
            ctrl.fetch_threads > after_flush,
            "fetch-bound should grow fetch_threads: {} vs after_flush {}",
            ctrl.fetch_threads,
            after_flush
        );
    }

    #[test]
    fn l0_alone_does_not_trigger_flush_but_bumps_bg_jobs() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        ctrl.bg_jobs = 6;
        ctrl.prev_bg_jobs = 6;

        ctrl.observe(&healthy_signals()); // warmup

        // High L0 but flush channel is empty — pipeline has no backpressure.
        // Should NOT classify as Flush (would suppress prefetch needlessly).
        // But should proactively bump bg_jobs to help compaction.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 3000.0,
                flush_wait_ms: 0.0,
                l0_files: 60,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_ne!(
            ctrl.classify(),
            Bottleneck::Flush,
            "L0 alone with empty flush channel must not trigger Flush"
        );
        assert!(
            ctrl.bg_jobs > 6,
            "proactive bg_jobs should have increased: {}",
            ctrl.bg_jobs
        );
    }

    #[test]
    fn l0_with_channel_pressure_triggers_flush() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        // High L0 AND flush channel filling up — genuine flush stress.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 3000.0,
                flush_wait_ms: 0.0,
                l0_files: 60,

                flush_channel_pending: 7,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
    }

    #[test]
    fn flush_channel_fill_triggers_flush() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        // Channel nearly full (7/8 = 87.5% > 75% threshold) but L0 low
        // and no flush_wait yet.  Channel fill is an earlier signal.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 3000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 7,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
    }

    #[test]
    fn empty_flush_channel_does_not_trigger_flush() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        // Channel nearly empty, low L0, no flush_wait — should NOT be Flush.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 3000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }

        assert_ne!(ctrl.classify(), Bottleneck::Flush);
    }

    #[test]
    fn bg_jobs_if_changed_tracks_transitions() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        assert!(ctrl.bg_jobs_if_changed().is_none());

        ctrl.bg_jobs = 6;
        assert_eq!(ctrl.bg_jobs_if_changed(), Some(6));
        assert!(ctrl.bg_jobs_if_changed().is_none());
    }

    #[test]
    fn prefetch_ahead_bounds() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        // 32 GB → channel_depth = 4, initial prefetch = 4/2 = 2
        assert_eq!(ctrl.max_channel_depth, 4);
        assert_eq!(ctrl.prefetch_ahead, 2);

        ctrl.observe(&healthy_signals()); // warmup

        // Sustained fetch starvation should hit channel_depth.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 10_000.0,
                build_ms: 100.0,
                flush_wait_ms: 0.0,
                l0_files: 0,

                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 0,
            });
        }
        assert_eq!(ctrl.prefetch_ahead, ctrl.max_channel_depth);
    }

    #[test]
    fn channel_depth_scales_with_ram() {
        assert_eq!(channel_depth_for_ram(8 * GB), 1);
        assert_eq!(channel_depth_for_ram(16 * GB), 2);
        assert_eq!(channel_depth_for_ram(32 * GB), 4);
        assert_eq!(channel_depth_for_ram(64 * GB), 8);
        assert_eq!(channel_depth_for_ram(128 * GB), 8); // capped
        assert_eq!(channel_depth_for_ram(256 * GB), 8); // capped
    }

    #[test]
    fn span_step_scales_with_signal_strength() {
        // Mild fetch dominance → small growth step.
        let mut mild = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        mild.observe(&healthy_signals()); // warmup
        mild.observe(&BatchSignals {
            prefetch_recv_ms: 2600.0, // 52% of total → barely Fetch
            build_ms: 2000.0,
            flush_wait_ms: 400.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        let mild_growth = mild.batch_span as f64 / 100_000.0;

        // Extreme fetch dominance → large growth step.
        let mut extreme = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        extreme.observe(&healthy_signals()); // warmup
        extreme.observe(&BatchSignals {
            prefetch_recv_ms: 9000.0, // 90% of total → severely Fetch
            build_ms: 500.0,
            flush_wait_ms: 500.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        let extreme_growth = extreme.batch_span as f64 / 100_000.0;

        assert!(
            extreme_growth > mild_growth,
            "stronger signal should produce larger step: extreme {:.2} vs mild {:.2}",
            extreme_growth,
            mild_growth
        );

        // Similarly for flush: mild vs extreme shrink.
        let mut mild_flush = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        mild_flush.observe(&healthy_signals()); // warmup
        mild_flush.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 2800.0,
            flush_wait_ms: 2100.0, // 42% → barely Flush
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        let mild_shrink = mild_flush.batch_span as f64 / 100_000.0;

        let mut extreme_flush =
            BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        extreme_flush.observe(&healthy_signals()); // warmup
        extreme_flush.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 1000.0,
            flush_wait_ms: 8000.0, // 88% → severely Flush
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        let extreme_shrink = extreme_flush.batch_span as f64 / 100_000.0;

        assert!(
            extreme_shrink < mild_shrink,
            "stronger flush signal should produce bigger shrink: extreme {:.2} vs mild {:.2}",
            extreme_shrink,
            mild_shrink
        );
    }

    #[test]
    fn density_increase_shrinks_span() {
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        // Warmup with low density (5 cells/block).
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });

        // Second batch initializes density_ema, no correction yet.
        let output = ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        assert!(output.is_some());
        assert!(
            ctrl.density_ema > 0.0,
            "density_ema should be initialized: {}",
            ctrl.density_ema
        );
        let span_before = ctrl.batch_span;

        // 10x density jump (50 cells/block).  Span should shrink.
        let output = ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 500_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        assert!(output.is_some());
        assert!(
            ctrl.batch_span < span_before,
            "10x density increase should shrink span: {} vs before {}",
            ctrl.batch_span,
            span_before
        );
    }

    #[test]
    fn density_decrease_grows_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        // Warmup with high density (50 cells/block).
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 500_000,
            block_count: 10_000,
            imm_memtables: 0,
        });

        // Initialize density_ema.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 500_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        let span_before = ctrl.batch_span;

        // 10x density drop (5 cells/block).  Span should grow.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        assert!(
            ctrl.batch_span > span_before,
            "10x density decrease should grow span: {} vs before {}",
            ctrl.batch_span,
            span_before
        );
    }

    #[test]
    fn stable_density_no_span_change() {
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        // Warmup.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });

        // Initialize density_ema.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        let span_after_init = ctrl.batch_span;

        // Same density — density correction ratio ≈ 1.0.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            flush_channel_pending: 1,
            flush_channel_capacity: 8,
            cell_count: 50_000,
            block_count: 10_000,
            imm_memtables: 0,
        });
        assert_eq!(
            ctrl.batch_span, span_after_init,
            "stable density should not change span: {} vs {}",
            ctrl.batch_span, span_after_init
        );
    }

    #[test]
    fn bottleneck_code_round_trip() {
        for b in [Bottleneck::Fetch, Bottleneck::Build, Bottleneck::Flush] {
            assert_eq!(Bottleneck::from_code(b.to_code()), Some(b));
        }
        assert_eq!(Bottleneck::from_code(0), None);
        assert_eq!(Bottleneck::from_code(255), None);
    }

    #[test]
    fn imm_memtable_pressure_triggers_flush_and_shrinks_span() {
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);
        let initial_span = ctrl.batch_span;

        ctrl.observe(&healthy_signals()); // warmup

        // Simulate the OOM scenario: build-dominated timing (controller
        // would normally hold span constant), but immutable memtables
        // accumulating because flushes can't keep up with writes.
        // moderate threshold = 30, 50% ratio → triggers at imm_ema > 15.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0, // low fetch (not fetch-bound)
                build_ms: 5000.0,       // build-dominated timing
                flush_wait_ms: 0.0,     // no flush_wait (channel has room)
                l0_files: 5,
                flush_channel_pending: 1, // channel not full
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 20, // above moderate ratio (20 > 15)
            });
        }

        // imm pressure should override build classification → Flush
        assert_eq!(
            ctrl.classify(),
            Bottleneck::Flush,
            "high imm_memtables should trigger Flush even when timing looks build-bound"
        );
        assert!(
            ctrl.batch_span < initial_span,
            "batch_span should shrink under imm pressure: {} vs initial {}",
            ctrl.batch_span,
            initial_span
        );
    }

    #[test]
    fn low_imm_memtables_does_not_trigger_flush() {
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        // Low imm_memtables (well below moderate ratio) should not
        // interfere with normal bottleneck classification.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 2, // well below threshold
            });
        }

        assert_eq!(
            ctrl.classify(),
            Bottleneck::Fetch,
            "low imm_memtables should not override fetch classification"
        );
    }

    #[test]
    fn severe_imm_memtables_forces_aggressive_span_shrink() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB, TEST_MODERATE_IMM);

        ctrl.observe(&healthy_signals()); // warmup

        // Severe imm pressure (above 75% of threshold = 22.5) with
        // build-dominated timing — without imm signal this would be
        // Build-classified (span held constant).
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                flush_channel_pending: 1,
                flush_channel_capacity: 8,
                cell_count: 50_000,
                block_count: 10_000,
                imm_memtables: 25, // severe (25/30 = 83%)
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
        // Span should shrink significantly from 200k.
        assert!(
            ctrl.batch_span < 150_000,
            "severe imm pressure should aggressively shrink span: {}",
            ctrl.batch_span
        );
    }
}
