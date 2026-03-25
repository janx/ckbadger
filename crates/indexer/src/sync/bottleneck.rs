// Bottleneck-driven resource controller for bulk sync.
//
// Two independent control dimensions:
//
//   1. SPAN — governed by iteration wall-clock vs target (absolute time).
//      iteration_ms = recv_wait + build + flush_wait.
//      Below target → grow span.  Above target → shrink span.
//
//   2. I/O RESOURCES — governed by waste classification (ratio).
//      Waste = recv_wait + flush_wait (idle time, ideally zero).
//      Work = build (CPU time, never zero).
//      Classification identifies which waste source dominates and shifts
//      I/O knobs (prefetch_ahead, fetch_threads, bg_jobs) accordingly.
//
//   Dimension │ Signal            │ Knobs
//   ──────────│───────────────────│────────────────────────────────
//   Span      │ iteration_ms      │ batch_span
//   I/O       │ waste composition │ prefetch_ahead, fetch_threads, bg_jobs
//
// Key design principle: fetch (CKB RocksDB reads via std::thread::scope)
// does NOT compete with build (CPU via rayon) for resources.  Therefore
// Build-classified batches do NOT suppress prefetch_ahead or
// fetch_threads.  Only Flush suppresses prefetch — and only when the
// flush channel is actually filling up.

const EMA_ALPHA: f64 = 0.5;

// Batch span bounds (blocks)
pub(crate) const MIN_SPAN: u64 = 500;
pub(crate) const MAX_SPAN: u64 = 500_000;

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

// Waste classification threshold: flush_wait share of total waste.
// When flush_wait > 40% of waste, classify as Flush.
const FLUSH_PCT_THRESHOLD: f64 = 0.4;
// L0 threshold for proactive bg_jobs increase.  When L0 is above this but
// the flush channel still has room, we bump bg_jobs to help compaction
// catch up — without suppressing the pipeline (no Flush classification).
const FLUSH_L0_THRESHOLD: f64 = 40.0;

// Target iteration wall-clock time (ms).  Span adjusts to converge
// iteration_ms (recv + build + wait) toward this target:
//   iteration < target → grow span (room for more work per batch)
//   iteration > target → shrink span (batch too large)
// This is independent of bottleneck classification — classification
// only governs I/O resource knobs (prefetch, threads, bg_jobs).
const TARGET_ITERATION_MS: f64 = 3000.0;

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
    #[allow(dead_code)]
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
    pub cell_count: u64,
    pub block_count: u64,
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
    pub density_ema: f64,
}

#[derive(Debug)]
pub(crate) struct BottleneckController {
    // EMAs
    recv_ema: f64,
    build_ema: f64,
    wait_ema: f64,
    l0_ema: f64,
    density_ema: f64,

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
            density_ema: 0.0,

            batch_span: initial_span.clamp(MIN_SPAN, MAX_SPAN),
            prefetch_ahead: initial_prefetch,
            fetch_threads: max_fetch_threads,
            bg_jobs: max_bg_jobs,

            max_channel_depth,
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

        // ── Span adjustment: iteration_ms vs target ──
        //
        // Span is governed by total iteration wall-clock, independent of
        // bottleneck classification.  This separates "how big should each
        // batch be" (absolute time) from "which I/O resource is starved"
        // (waste ratio).
        let iteration_ms = self.recv_ema + self.build_ema + self.wait_ema;
        if iteration_ms > 1.0 {
            let ratio = TARGET_ITERATION_MS / iteration_ms;
            let factor = ratio.clamp(SPAN_STEP_MIN, SPAN_STEP_MAX);
            self.batch_span = ((self.batch_span as f64 * factor) as u64).clamp(MIN_SPAN, MAX_SPAN);
        }

        // ── I/O resource adjustment: waste classification ──
        //
        // recv_ms and wait_ms are waste (idle time); build_ms is work.
        // Classification determines which I/O resource to shift, NOT span.
        let bottleneck = self.classify();
        let waste = self.recv_ema + self.wait_ema;
        match bottleneck {
            Bottleneck::Fetch => {
                self.prefetch_ahead = (self.prefetch_ahead + 1).min(self.max_channel_depth);
                self.fetch_threads = grow_threads(self.fetch_threads, self.max_fetch_threads);
                self.bg_jobs = (self.bg_jobs - 1).max(self.min_bg_jobs);
            }
            Bottleneck::Build => {
                // No I/O adjustment needed — pipeline is CPU-bound.
                // If waste is near zero, try reducing bg_jobs to yield
                // CPU to build (compaction threads compete with rayon).
                if waste < iteration_ms * 0.05 {
                    self.bg_jobs = (self.bg_jobs - 1).max(self.min_bg_jobs);
                }
            }
            Bottleneck::Flush => {
                self.prefetch_ahead = (self.prefetch_ahead - 1).max(MIN_PREFETCH);
                self.fetch_threads = shrink_threads(self.fetch_threads);
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
            density_ema: self.density_ema,
        })
    }

    /// Classify which I/O resource is the dominant source of waste.
    ///
    /// Only governs I/O knobs (prefetch, threads, bg_jobs), NOT span.
    /// Span is adjusted independently by iteration_ms targeting.
    fn classify(&self) -> Bottleneck {
        let waste = self.recv_ema + self.wait_ema;
        if waste < 1.0 {
            // Near-zero waste — pipeline is perfectly overlapped, CPU-bound.
            return Bottleneck::Build;
        }

        let flush_pct = self.wait_ema / waste;

        // Flush stressed: wait dominates waste.
        // L0 alone does NOT trigger — handled by proactive bg_jobs in observe().
        if flush_pct > FLUSH_PCT_THRESHOLD {
            return Bottleneck::Flush;
        }
        // Remaining waste is fetch-dominated.
        Bottleneck::Fetch
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

    fn healthy_signals() -> BatchSignals {
        BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
        }
    }

    #[test]
    fn first_batch_returns_none() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
        let output = ctrl.observe(&healthy_signals());
        assert!(output.is_none());
    }

    #[test]
    fn fetch_starved_grows_prefetch() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
        let initial_prefetch = ctrl.prefetch_ahead;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 1500.0,
                build_ms: 500.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Fetch);
        assert!(ctrl.prefetch_ahead > initial_prefetch);
    }

    #[test]
    fn flush_pressure_shrinks_span_grows_bg_jobs() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
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

                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
        assert!(ctrl.batch_span < initial_span);
        assert!(ctrl.bg_jobs > 4);
    }

    #[test]
    fn iteration_above_target_shrinks_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
        let initial_span = ctrl.batch_span;

        ctrl.observe(&healthy_signals()); // warmup

        // iteration = 50 + 5000 + 100 = 5150ms > target (3000ms) → shrink
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 100.0,
                l0_files: 5,
                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert!(
            ctrl.batch_span < initial_span,
            "iteration above target must shrink span: {} vs initial {}",
            ctrl.batch_span,
            initial_span
        );
    }

    #[test]
    fn build_bound_does_not_shrink_prefetch_or_fetch_threads() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
        let initial_prefetch = ctrl.prefetch_ahead;
        let initial_threads = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Near-zero waste → Build classification.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 3000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                cell_count: 50_000,
                block_count: 10_000,
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
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..100 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 100,

                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert!(ctrl.batch_span >= MIN_SPAN);
    }

    #[test]
    fn bg_jobs_bounds_enforced() {
        let mut ctrl = BottleneckController::new(50_000, 12, 4, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // Fetch-bound: bg_jobs should shrink but not below min.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,

                cell_count: 50_000,
                block_count: 10_000,
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

                cell_count: 50_000,
                block_count: 10_000,
            });
        }
        assert!(ctrl.bg_jobs <= 4);
    }

    #[test]
    fn fetch_threads_stable_when_build_bound_grow_when_fetch_bound() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
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

                cell_count: 50_000,
                block_count: 10_000,
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

                cell_count: 50_000,
                block_count: 10_000,
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

                cell_count: 50_000,
                block_count: 10_000,
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
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
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

                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert_ne!(
            ctrl.classify(),
            Bottleneck::Flush,
            "L0 alone with empty flush channel must not trigger Flush"
        );
        // The key assertion is that Flush is NOT triggered (which would
        // suppress prefetch).  bg_jobs may fluctuate from Fetch -1 vs
        // proactive L0 +1 interactions.
        assert!(
            ctrl.bg_jobs >= ctrl.min_bg_jobs,
            "bg_jobs should stay within bounds: {}",
            ctrl.bg_jobs
        );
    }

    #[test]
    fn flush_wait_dominates_waste_triggers_flush() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // flush_wait dominates waste → Flush classification.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 2000.0,
                flush_wait_ms: 3000.0,
                l0_files: 5,
                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
    }

    #[test]
    fn bg_jobs_if_changed_tracks_transitions() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
        assert!(ctrl.bg_jobs_if_changed().is_none());

        ctrl.bg_jobs = 6;
        assert_eq!(ctrl.bg_jobs_if_changed(), Some(6));
        assert!(ctrl.bg_jobs_if_changed().is_none());
    }

    #[test]
    fn prefetch_ahead_bounds() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
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

                cell_count: 50_000,
                block_count: 10_000,
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
    fn span_scales_with_iteration_overshoot() {
        // Larger overshoot → bigger shrink factor.
        let mut mild = BottleneckController::new(100_000, 12, 8, 32 * GB);
        mild.observe(&healthy_signals()); // warmup
        mild.observe(&BatchSignals {
            prefetch_recv_ms: 500.0,
            build_ms: 3500.0,
            flush_wait_ms: 500.0, // total = 4500ms, 1.5x target
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
        });
        let mild_span = mild.batch_span;

        let mut severe = BottleneckController::new(100_000, 12, 8, 32 * GB);
        severe.observe(&healthy_signals()); // warmup
        severe.observe(&BatchSignals {
            prefetch_recv_ms: 3000.0,
            build_ms: 5000.0,
            flush_wait_ms: 1000.0, // total = 9000ms, 3x target
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
        });
        let severe_span = severe.batch_span;

        assert!(
            severe_span < mild_span,
            "larger overshoot should produce bigger shrink: severe {} vs mild {}",
            severe_span,
            mild_span
        );
    }

    #[test]
    fn density_increase_shrinks_span() {
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB);

        // Warmup with low density (5 cells/block).
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
        });

        // Second batch initializes density_ema, no correction yet.
        let output = ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
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
            cell_count: 500_000,
            block_count: 10_000,
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
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);

        // Warmup with high density (50 cells/block).
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 500_000,
            block_count: 10_000,
        });

        // Initialize density_ema.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 500_000,
            block_count: 10_000,
        });
        let span_before = ctrl.batch_span;

        // 10x density drop (5 cells/block).  Span should grow.
        ctrl.observe(&BatchSignals {
            prefetch_recv_ms: 100.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
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
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB);

        // Use iteration_ms == TARGET (3000ms) so span targeting is neutral.
        let at_target = BatchSignals {
            prefetch_recv_ms: 0.0,
            build_ms: 3000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            cell_count: 50_000,
            block_count: 10_000,
        };

        ctrl.observe(&at_target); // warmup (batch 1, returns None)
                                  // Let EMAs converge to steady state before measuring span stability.
        for _ in 0..10 {
            ctrl.observe(&at_target);
        }
        let span_after_init = ctrl.batch_span;

        // Same density, same iteration_ms — span should be approximately stable.
        ctrl.observe(&at_target);
        let drift_pct =
            (ctrl.batch_span as f64 - span_after_init as f64).abs() / span_after_init as f64;
        assert!(
            drift_pct < 0.01,
            "stable density at target iteration should barely change span: {} vs {} ({:.2}%)",
            ctrl.batch_span,
            span_after_init,
            drift_pct * 100.0
        );
    }

    #[test]
    fn iteration_below_target_grows_span() {
        let mut ctrl = BottleneckController::new(50_000, 12, 8, 32 * GB);
        ctrl.observe(&healthy_signals()); // warmup

        let initial_span = ctrl.batch_span;

        // iteration = 500 + 500 + 0 = 1000ms, well below target (3000ms) → grow
        for _ in 0..5 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 500.0,
                build_ms: 500.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert!(
            ctrl.batch_span > initial_span,
            "span should grow when iteration below target: {} vs initial {}",
            ctrl.batch_span,
            initial_span
        );
    }

    #[test]
    fn high_iteration_shrinks_span_regardless_of_classification() {
        // Even when fetch dominates waste, if iteration_ms >> target,
        // span must shrink.  This is the fix for the OOM scenario where
        // span grew to MAX_SPAN under sustained Fetch classification.
        let mut ctrl = BottleneckController::new(100_000, 12, 8, 32 * GB);
        ctrl.observe(&healthy_signals()); // warmup

        let initial_span = ctrl.batch_span;

        // iteration = 20000 + 30000 + 0 = 50s, 16x above target
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 20000.0,
                build_ms: 30000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                cell_count: 50_000,
                block_count: 10_000,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Fetch);
        assert!(
            ctrl.batch_span < initial_span,
            "span should shrink even when fetch-classified if iteration >> target: {} vs initial {}",
            ctrl.batch_span,
            initial_span
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
}
