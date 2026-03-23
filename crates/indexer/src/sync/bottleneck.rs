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
//   Signal              │ Knob               │ What it controls
//   ────────────────────│────────────────────│──────────────────────────
//   prefetch_recv_ms    │ prefetch_ahead     │ Fetch/build overlap depth
//   build_ms            │ batch_span         │ Work volume per iteration
//   flush_wait_ms + L0  │ bg_jobs            │ RocksDB compaction threads

const EMA_ALPHA: f64 = 0.3;

// Batch span bounds (blocks)
pub(crate) const MIN_SPAN: u64 = 10_000;
pub(crate) const MAX_SPAN: u64 = 100_000;

// Prefetch ahead bounds (batches)
const MIN_PREFETCH: u64 = 1;
pub(crate) const MAX_PREFETCH: u64 = 8;
const INITIAL_PREFETCH: u64 = 4;

// Flush ahead bounds (batches buffered between build and flush worker)
pub(crate) const MAX_FLUSH_AHEAD: u64 = 8;
const INITIAL_FLUSH_AHEAD: u64 = 4;

// Fetch thread bounds
const MIN_FETCH_THREADS: u32 = 2;

// Background jobs bounds
const MIN_BG_JOBS: i32 = 2;

// Row budget (materialization cap per batch)
const MAX_HISTORY_ROWS: f64 = 800_000.0;
const ROWS_EMA_ALPHA: f64 = 0.3;
const INITIAL_ROWS_PER_BLOCK: f64 = 30.0;

// Bottleneck classification thresholds (fraction of total iteration time).
// Flush checked first (compound risk from L0 buildup).
const FLUSH_PCT_THRESHOLD: f64 = 0.4;
const FETCH_PCT_THRESHOLD: f64 = 0.5;
// Leading indicator: L0 EMA above this → treat as flush-stressed even if
// flush_wait hasn't spiked yet (the channel hasn't filled, but it will).
const FLUSH_L0_THRESHOLD: f64 = 40.0;

// Adjustment step factors
const SPAN_GROW: f64 = 1.10;
const SPAN_SHRINK: f64 = 0.80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bottleneck {
    Fetch,
    Build,
    Flush,
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
    pub actual_blocks: u64,
    pub history_rows: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ControllerOutput {
    pub batch_span: u64,
    pub prefetch_ahead: u64,
    pub fetch_threads: u32,
    pub flush_ahead: u64,
    pub bg_jobs: i32,
    pub bottleneck: Bottleneck,
    pub recv_ema: f64,
    pub build_ema: f64,
    pub wait_ema: f64,
    pub l0_ema: f64,
}

#[derive(Debug)]
pub(crate) struct BottleneckController {
    // EMAs
    recv_ema: f64,
    build_ema: f64,
    wait_ema: f64,
    l0_ema: f64,
    rows_per_block_ema: f64,
    rows_ema_observed: bool,

    // Current outputs
    batch_span: u64,
    prefetch_ahead: u64,
    fetch_threads: u32,
    flush_ahead: u64,
    bg_jobs: i32,

    // Bounds
    max_fetch_threads: u32,
    min_bg_jobs: i32,
    max_bg_jobs: i32,

    // State
    batch_count: u64,
    prev_bg_jobs: i32,
}

impl BottleneckController {
    pub(crate) fn new(initial_span: u64, max_fetch_threads: u32, max_bg_jobs: i32) -> Self {
        let max_bg_jobs = max_bg_jobs.max(MIN_BG_JOBS);
        let min_bg_jobs = (max_bg_jobs / 4).max(MIN_BG_JOBS);
        let max_fetch_threads = max_fetch_threads.max(MIN_FETCH_THREADS);
        Self {
            recv_ema: 0.0,
            build_ema: 0.0,
            wait_ema: 0.0,
            l0_ema: 0.0,
            rows_per_block_ema: INITIAL_ROWS_PER_BLOCK,
            rows_ema_observed: false,

            batch_span: initial_span.clamp(MIN_SPAN, MAX_SPAN),
            prefetch_ahead: INITIAL_PREFETCH,
            fetch_threads: max_fetch_threads,
            flush_ahead: INITIAL_FLUSH_AHEAD,
            bg_jobs: max_bg_jobs,

            max_fetch_threads,
            min_bg_jobs,
            max_bg_jobs,

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

        // Update rows/block EMA for history row budget cap.
        if signals.history_rows > 0 && signals.actual_blocks > 0 {
            let sample = signals.history_rows as f64 / signals.actual_blocks as f64;
            self.rows_per_block_ema =
                self.rows_per_block_ema * (1.0 - ROWS_EMA_ALPHA) + sample * ROWS_EMA_ALPHA;
            self.rows_ema_observed = true;
        }

        // Classify bottleneck.
        let bottleneck = self.classify();

        // Adjust knobs.
        match bottleneck {
            Bottleneck::Fetch => {
                self.prefetch_ahead = (self.prefetch_ahead + 1).min(MAX_PREFETCH);
                self.fetch_threads = grow_threads(self.fetch_threads, self.max_fetch_threads);
                self.batch_span = grow_span(self.batch_span);
                self.bg_jobs = (self.bg_jobs - 1).max(self.min_bg_jobs);
            }
            Bottleneck::Build => {
                // Build-bound: shrink fetch overlap to give build more CPU.
                self.prefetch_ahead = (self.prefetch_ahead - 1).max(MIN_PREFETCH);
                self.fetch_threads = shrink_threads(self.fetch_threads);
                self.batch_span = grow_span(self.batch_span);
            }
            Bottleneck::Flush => {
                // Flush-bound: grow flush_ahead to absorb transient compaction
                // spikes without blocking build.
                self.fetch_threads = shrink_threads(self.fetch_threads);
                self.flush_ahead = (self.flush_ahead + 1).min(MAX_FLUSH_AHEAD);
                self.batch_span = shrink_span(self.batch_span);
                self.bg_jobs = (self.bg_jobs + 1).min(self.max_bg_jobs);
            }
        }

        // Row budget cap: prevent a single batch from generating too many
        // materialization rows (keeps per-batch flush bounded).
        if self.rows_ema_observed && self.rows_per_block_ema > 0.0 {
            let row_cap = (MAX_HISTORY_ROWS / self.rows_per_block_ema) as u64;
            self.batch_span = self.batch_span.min(row_cap);
        }

        // Final clamp.
        self.batch_span = self.batch_span.clamp(MIN_SPAN, MAX_SPAN);
        self.prefetch_ahead = self.prefetch_ahead.clamp(MIN_PREFETCH, MAX_PREFETCH);
        self.bg_jobs = self.bg_jobs.clamp(self.min_bg_jobs, self.max_bg_jobs);

        Some(ControllerOutput {
            batch_span: self.batch_span,
            prefetch_ahead: self.prefetch_ahead,
            fetch_threads: self.fetch_threads,
            flush_ahead: self.flush_ahead,
            bg_jobs: self.bg_jobs,
            bottleneck,
            recv_ema: self.recv_ema,
            build_ema: self.build_ema,
            wait_ema: self.wait_ema,
            l0_ema: self.l0_ema,
        })
    }

    fn classify(&self) -> Bottleneck {
        let total = self.recv_ema + self.build_ema + self.wait_ema;
        if total < 1.0 {
            return Bottleneck::Build;
        }

        let flush_pct = self.wait_ema / total;
        let fetch_pct = self.recv_ema / total;

        let flush_stressed = flush_pct > FLUSH_PCT_THRESHOLD || self.l0_ema > FLUSH_L0_THRESHOLD;
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

    pub(crate) fn flush_ahead(&self) -> u64 {
        self.flush_ahead
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

fn grow_span(span: u64) -> u64 {
    ((span as f64 * SPAN_GROW) as u64).min(MAX_SPAN)
}

fn shrink_span(span: u64) -> u64 {
    ((span as f64 * SPAN_SHRINK) as u64).max(MIN_SPAN)
}

/// +25% per step (minimum step of 1).
fn grow_threads(t: u32, max: u32) -> u32 {
    (t + (t / 4).max(1)).min(max)
}

/// -25% per step (minimum step of 1).
fn shrink_threads(t: u32) -> u32 {
    t.saturating_sub((t / 4).max(1)).max(MIN_FETCH_THREADS)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_signals() -> BatchSignals {
        BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            actual_blocks: 10_000,
            history_rows: 100_000,
        }
    }

    #[test]
    fn first_batch_returns_none() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        let output = ctrl.observe(&healthy_signals());
        assert!(output.is_none());
    }

    #[test]
    fn fetch_starved_grows_prefetch_and_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        let initial_span = ctrl.batch_span;
        let initial_prefetch = ctrl.prefetch_ahead;

        // Warmup.
        ctrl.observe(&healthy_signals());

        // Feed fetch-heavy signals.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_blocks: 10_000,
                history_rows: 100_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Fetch);
        assert!(ctrl.prefetch_ahead > initial_prefetch);
        assert!(ctrl.batch_span > initial_span);
    }

    #[test]
    fn flush_pressure_shrinks_span_grows_bg_jobs() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        // Start bg_jobs below max so we can see it grow.
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
                actual_blocks: 5000,
                history_rows: 50_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
        assert!(ctrl.batch_span < initial_span);
        assert!(ctrl.bg_jobs > 4);
    }

    #[test]
    fn build_bound_grows_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        let initial_span = ctrl.batch_span;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 100.0,
                l0_files: 5,
                actual_blocks: 5000,
                history_rows: 50_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Build);
        assert!(ctrl.batch_span > initial_span);
    }

    #[test]
    fn row_budget_caps_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);

        ctrl.observe(&healthy_signals()); // warmup

        // 40 rows/block → after EMA converges, budget = 800K/40 = 20K blocks.
        // Without row cap, build-bound would grow span well past 50K.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 5000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_blocks: 10_000,
                history_rows: 400_000, // 40 rows/block
            });
        }

        // Span should be capped by row budget (~20K) instead of growing to MAX.
        assert!(
            ctrl.batch_span <= 25_000,
            "span {} should be capped by row budget (~20K)",
            ctrl.batch_span
        );
    }

    #[test]
    fn span_bounds_enforced() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);

        ctrl.observe(&healthy_signals()); // warmup

        // Severe flush pressure — span should not go below MIN_SPAN.
        for _ in 0..100 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 100,
                actual_blocks: 10_000,
                history_rows: 100_000,
            });
        }

        assert!(ctrl.batch_span >= MIN_SPAN);
    }

    #[test]
    fn bg_jobs_bounds_enforced() {
        let mut ctrl = BottleneckController::new(50_000, 12, 4);

        ctrl.observe(&healthy_signals()); // warmup

        // Fetch-bound: bg_jobs should shrink but not below min.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_blocks: 10_000,
                history_rows: 100_000,
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
                actual_blocks: 10_000,
                history_rows: 100_000,
            });
        }
        assert!(ctrl.bg_jobs <= 4);
    }

    #[test]
    fn fetch_threads_grow_when_fetch_bound_shrink_when_build_bound() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        let initial = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Build-bound: fetch_threads should shrink.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_blocks: 10_000,
                history_rows: 100_000,
            });
        }
        assert!(
            ctrl.fetch_threads < initial,
            "build-bound should shrink fetch_threads: {} vs initial {}",
            ctrl.fetch_threads,
            initial
        );

        let after_build = ctrl.fetch_threads;

        // Fetch-bound: fetch_threads should grow back.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_blocks: 10_000,
                history_rows: 100_000,
            });
        }
        assert!(
            ctrl.fetch_threads > after_build,
            "fetch-bound should grow fetch_threads: {} vs after_build {}",
            ctrl.fetch_threads,
            after_build
        );
    }

    #[test]
    fn l0_leading_indicator_triggers_flush() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);

        ctrl.observe(&healthy_signals()); // warmup

        // High L0 but zero flush_wait (channel hasn't backed up yet).
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 3000.0,
                flush_wait_ms: 0.0,
                l0_files: 60,
                actual_blocks: 10_000,
                history_rows: 100_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
    }

    #[test]
    fn bg_jobs_if_changed_tracks_transitions() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        assert!(ctrl.bg_jobs_if_changed().is_none()); // no change yet

        ctrl.bg_jobs = 6;
        assert_eq!(ctrl.bg_jobs_if_changed(), Some(6));
        assert!(ctrl.bg_jobs_if_changed().is_none()); // same value, no change
    }

    #[test]
    fn prefetch_ahead_bounds() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8);
        assert_eq!(ctrl.prefetch_ahead, INITIAL_PREFETCH);

        ctrl.observe(&healthy_signals()); // warmup

        // Sustained fetch starvation should hit MAX_PREFETCH.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 10_000.0,
                build_ms: 100.0,
                flush_wait_ms: 0.0,
                l0_files: 0,
                actual_blocks: 50_000,
                history_rows: 100_000,
            });
        }
        assert_eq!(ctrl.prefetch_ahead, MAX_PREFETCH);
    }
}
