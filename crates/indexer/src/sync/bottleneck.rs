// Bottleneck-driven resource controller for bulk sync.
//
// Two independent control dimensions:
//
//   1. BATCH SIZE — overlap-driven with waste-weighted direction.
//      The unit is cells, not bytes.  Cell count is a better proxy for
//      CPU cost because cell density varies ~25x across CKB block ranges.
//
//      overlap = build / (build + waste) measures pipeline efficiency.
//      Single objective: maintain overlap >= OVERLAP_TARGET (90%).
//
//      When overlap < target (pipeline inefficient):
//        Direction determined by waste composition (geometric blend):
//        - recv-dominated → grow (longer build gives prefetch more time)
//        - flush-dominated → shrink (less data reduces I/O pressure)
//        - mixed → geometric mean ≈ hold (opposing forces cancel)
//
//      When overlap >= target (pipeline efficient):
//        Grow proportional to headroom above target to amortize overhead.
//        CPU-bound (high headroom) → aggressive growth.
//        IO-bound with buffering (low headroom) → cautious growth.
//
//   2. I/O RESOURCES — governed by waste classification (ratio).
//      Waste = recv_wait + flush_wait (idle time, ideally zero).
//      Work = build (CPU time, never zero).
//      Classification identifies which waste source dominates and shifts
//      I/O knobs (fetch_threads, bg_jobs) accordingly.
//
//   Dimension  │ Signal                  │ Knobs
//   ───────────│─────────────────────────│──────────────────────────
//   Batch size │ overlap + waste shares  │ target_cells
//   I/O        │ waste composition       │ fetch_threads, bg_jobs
//
// Key design principle: fetch (CKB RocksDB reads via std::thread::scope)
// does NOT compete with build (CPU via rayon) for resources.  Therefore
// Build-classified batches do NOT suppress fetch_threads.  Only Flush
// suppresses fetch threads — and only when the flush channel is actually
// filling up.

const EMA_ALPHA: f64 = 0.5;

// No cell count bounds — build_ms feedback is a self-stabilizing loop
// (high build_ms → shrink cells → lower build_ms → stop shrinking),
// and max_batch_bytes provides the memory safety ceiling.

// Batch bytes bounds (secondary safety cap — NOT dynamically adjusted)
const MIN_BATCH_BYTES: u64 = 1_000_000; // 1 MB
const ABSOLUTE_MAX_BATCH_BYTES: u64 = 8_000_000_000; // 8 GB ceiling

// No block-count bounds — drain_by_cells uses target_cells + max_batch_bytes
// as the two budget dimensions.  Block count is an output, not a control knob.

// Channel depth bounds (batches).  Max is computed from system RAM at
// startup to cap total buffered data.  Each slot holds one batch of raw
// blocks (prefetch) or materialized rows (flush); per-slot size scales
// with batch size.  Absolute ceiling of 8 prevents runaway memory on
// very large machines.
const MIN_CHANNEL_DEPTH: u64 = 1;
const ABSOLUTE_MAX_CHANNEL_DEPTH: u64 = 8;

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

// Pipeline overlap target.  The controller adjusts target_cells to keep
// overlap (build / iteration) at or above this level.  When overlap is
// below target, waste composition determines direction: recv-dominated
// waste → grow (give prefetch more time), flush-dominated → shrink
// (reduce I/O pressure).  When overlap is above target, grow to
// amortize per-batch overhead, proportional to headroom above target.
const OVERLAP_TARGET: f64 = 0.9;

// Per-step safety bounds on target_cells adjustment factor.
const STEP_FLOOR: f64 = 0.5;
const STEP_CEIL: f64 = 2.0;

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
    /// Cells actually drained from the buffer (may be less than target_cells
    /// when supply-limited by prefetch rate or bytes-limited by max_batch_bytes).
    pub actual_cells: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ControllerOutput {
    pub target_cells: u64,
    pub max_batch_bytes: u64,
    pub fetch_threads: u32,
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

    // Current outputs
    target_cells: u64,
    fetch_threads: u32,
    bg_jobs: i32,

    // Bounds
    max_batch_bytes: u64,
    max_fetch_threads: u32,
    min_bg_jobs: i32,
    max_bg_jobs: i32,

    // State
    batch_count: u64,
    prev_bg_jobs: i32,
}

impl BottleneckController {
    pub(crate) fn new(
        initial_target_cells: u64,
        max_fetch_threads: u32,
        max_bg_jobs: i32,
        system_ram_bytes: u64,
    ) -> Self {
        let max_bg_jobs = max_bg_jobs.max(MIN_BG_JOBS);
        let min_bg_jobs = (max_bg_jobs / 4).max(MIN_BG_JOBS);
        let max_fetch_threads = max_fetch_threads.max(MIN_FETCH_THREADS);
        // RAM/16: 8GB→512MB, 16GB→1GB, 32GB→2GB, 64GB→4GB
        let max_batch_bytes =
            (system_ram_bytes / 16).clamp(MIN_BATCH_BYTES, ABSOLUTE_MAX_BATCH_BYTES);
        Self {
            recv_ema: 0.0,
            build_ema: 0.0,
            wait_ema: 0.0,
            l0_ema: 0.0,

            target_cells: initial_target_cells.max(1),
            fetch_threads: max_fetch_threads,
            bg_jobs: max_bg_jobs,

            max_batch_bytes,
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

        // ── Batch size adjustment: overlap-driven with waste-weighted direction ──
        //
        // overlap = build / (build + waste).  Measures pipeline efficiency.
        //
        // Scenario 1 (overlap < target): improve overlap.
        //   recv-dominated waste → grow (longer build gives prefetch more time)
        //   flush-dominated waste → shrink (less data reduces I/O pressure)
        //   mixed → geometric blend by waste shares (hold when balanced)
        //
        // Scenario 2 (overlap ≥ target): optimize throughput.
        //   Grow to amortize per-batch overhead, proportional to headroom
        //   above target.  2a (high headroom = CPU-bound) → aggressive.
        //   2b (low headroom = barely above target) → cautious.
        let overlap = self.build_ema / (self.build_ema + self.recv_ema + self.wait_ema);
        let factor = if overlap >= OVERLAP_TARGET {
            // Scenario 2: overlap good → grow for overhead amortization.
            let headroom = (overlap - OVERLAP_TARGET) / (1.0 - OVERLAP_TARGET);
            1.0 + headroom
        } else {
            // Scenario 1: overlap bad → direction depends on waste composition.
            let waste = self.recv_ema + self.wait_ema;
            let flush_pct = self.wait_ema / waste;
            let recv_pct = 1.0 - flush_pct;
            let recv_pull = (1.0 / overlap).min(STEP_CEIL);
            let flush_pull = (overlap / OVERLAP_TARGET).max(STEP_FLOOR);
            recv_pull.powf(recv_pct) * flush_pull.powf(flush_pct)
        };
        let factor = factor.clamp(STEP_FLOOR, STEP_CEIL);
        self.target_cells = ((self.target_cells as f64 * factor) as u64).max(1);

        // ── Supply cap: don't let target_cells grow far beyond what the
        // pipeline can actually deliver.  When actual_cells << target_cells,
        // the batch is supply-limited (prefetch rate or max_batch_bytes) and
        // growing target further is pointless.  Cap at 2× actual to keep
        // headroom for supply recovery without runaway.
        //
        // Only apply when overlap is BELOW target.  When overlap is healthy,
        // low actual_cells reflects sparse chain data (few cells per block),
        // not a pipeline delivery bottleneck.  Capping in that case creates
        // a death spiral: tiny target → tiny drain → tiny actual → re-cap.
        if signals.actual_cells > 0 && overlap < OVERLAP_TARGET {
            let ceiling = signals.actual_cells.saturating_mul(2);
            if self.target_cells > ceiling {
                self.target_cells = ceiling;
            }
        }

        // ── I/O resource adjustment: waste classification ──
        //
        // recv_ms and wait_ms are waste (idle time); build_ms is work.
        // Classification determines which I/O resource to shift, NOT batch size.
        let bottleneck = self.classify();
        let waste = self.recv_ema + self.wait_ema;
        match bottleneck {
            Bottleneck::Fetch => {
                self.fetch_threads = grow_threads(self.fetch_threads, self.max_fetch_threads);
                self.bg_jobs = (self.bg_jobs - 1).max(self.min_bg_jobs);
            }
            Bottleneck::Build => {
                // No I/O adjustment needed — pipeline is CPU-bound.
                // If waste is near zero, try reducing bg_jobs to yield
                // CPU to build (compaction threads compete with rayon).
                if waste < self.build_ema * 0.05 {
                    self.bg_jobs = (self.bg_jobs - 1).max(self.min_bg_jobs);
                }
            }
            Bottleneck::Flush => {
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
        self.bg_jobs = self.bg_jobs.clamp(self.min_bg_jobs, self.max_bg_jobs);

        Some(ControllerOutput {
            target_cells: self.target_cells,
            max_batch_bytes: self.max_batch_bytes,
            fetch_threads: self.fetch_threads,
            bg_jobs: self.bg_jobs,
            bottleneck,
            recv_ema: self.recv_ema,
            build_ema: self.build_ema,
            wait_ema: self.wait_ema,
            l0_ema: self.l0_ema,
        })
    }

    /// Classify which I/O resource is the dominant source of waste.
    ///
    /// Only governs I/O knobs (fetch_threads, bg_jobs), NOT batch size.
    /// Batch size is adjusted independently by build_ms targeting.
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

    pub(crate) fn target_cells(&self) -> u64 {
        self.target_cells
    }

    pub(crate) fn max_batch_bytes(&self) -> u64 {
        self.max_batch_bytes
    }

    pub(crate) fn fetch_threads(&self) -> u32 {
        self.fetch_threads
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
pub(crate) fn channel_depth_for_ram(system_ram_bytes: u64) -> u64 {
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
            build_ms: 2000.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            actual_cells: u64::MAX, // unconstrained supply for most tests
        }
    }

    #[test]
    fn first_batch_returns_none() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let output = ctrl.observe(&healthy_signals());
        assert!(output.is_none());
    }

    #[test]
    fn fetch_starved_grows_fetch_threads() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial_threads = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Shrink threads first via flush pressure.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 80,
                actual_cells: u64::MAX,
            });
        }
        let after_flush = ctrl.fetch_threads;
        assert!(
            after_flush < initial_threads,
            "flush should have shrunk threads: {} vs initial {}",
            after_flush,
            initial_threads
        );

        // Now fetch-starved: threads should grow back.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Fetch);
        assert!(
            ctrl.fetch_threads > after_flush,
            "fetch-bound should grow fetch_threads: {} vs after_flush {}",
            ctrl.fetch_threads,
            after_flush
        );
    }

    #[test]
    fn flush_pressure_grows_bg_jobs() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        ctrl.bg_jobs = 4;
        ctrl.prev_bg_jobs = 4;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 2000.0,
                flush_wait_ms: 3000.0,
                l0_files: 60,
                actual_cells: u64::MAX,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
        assert!(ctrl.bg_jobs > 4);
    }

    #[test]
    fn high_build_no_waste_still_grows() {
        // High build_ms with zero waste = 100% overlap = grow for amortization.
        // This is correct: the pipeline is perfectly overlapped, bigger batch
        // amortizes overhead better.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial_cells = ctrl.target_cells;

        ctrl.observe(&healthy_signals()); // warmup

        for _ in 0..5 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 6000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert!(
            ctrl.target_cells > initial_cells,
            "high build with zero waste (100% overlap) should grow: {} vs initial {}",
            ctrl.target_cells,
            initial_cells
        );
    }

    #[test]
    fn low_build_grows_target_cells() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial_cells = ctrl.target_cells;

        ctrl.observe(&healthy_signals()); // warmup

        // build_ms = 500 << target (2000) → should grow
        for _ in 0..5 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 500.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert!(
            ctrl.target_cells > initial_cells,
            "low build_ms should grow target_cells: {} vs initial {}",
            ctrl.target_cells,
            initial_cells
        );
    }

    #[test]
    fn high_overlap_grows_for_amortization() {
        // When overlap > 90% (CPU-bound), controller should grow target_cells.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;

        ctrl.observe(&healthy_signals()); // warmup

        // overlap = 2000 / (2000 + 0 + 0) = 100% → headroom 1.0 → factor 2.0
        for _ in 0..5 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 2000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert!(
            ctrl.target_cells > initial * 4,
            "high overlap should grow aggressively: {} should be > {}",
            ctrl.target_cells,
            initial * 4
        );
    }

    #[test]
    fn recv_dominated_waste_grows() {
        // When overlap < 90% and waste is recv-dominated, controller
        // should grow (longer build gives prefetch more time).
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;

        ctrl.observe(&healthy_signals()); // warmup

        // overlap = 1000 / (1000 + 5000 + 0) = 17% → recv pull = 1/0.17 = 6 → clamp 2.0
        for _ in 0..5 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert!(
            ctrl.target_cells > initial,
            "recv-dominated waste should grow: {} should be > initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn flush_dominated_waste_shrinks() {
        // When overlap < 90% and waste is flush-dominated, controller
        // should shrink (reduce I/O pressure).
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;

        ctrl.observe(&healthy_signals()); // warmup

        // overlap = 1000 / (1000 + 0 + 5000) = 17% → flush pull = 0.17/0.9 = 0.19
        for _ in 0..5 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 1000.0,
                flush_wait_ms: 5000.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert!(
            ctrl.target_cells < initial / 4,
            "flush-dominated waste should shrink aggressively: {} should be < {}",
            ctrl.target_cells,
            initial / 4
        );
    }

    #[test]
    fn balanced_waste_holds_steady() {
        // When overlap < 90% but waste is evenly split between recv and flush,
        // the geometric blend should produce factor ≈ 1.0 (hold).
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // overlap = 1000 / (1000 + 2500 + 2500) = 17%, recv_pct = 0.5
        // recv_pull = 1/0.17 = ~6 (clamped 2.0), flush_pull = 0.17/0.9 = 0.19 (clamped 0.5)
        // geometric: 2.0^0.5 * 0.5^0.5 = 1.414 * 0.707 = 1.0
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 2500.0,
                build_ms: 1000.0,
                flush_wait_ms: 2500.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        let ratio = ctrl.target_cells as f64 / 200_000.0;
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "balanced waste should roughly hold: target_cells={} (ratio={:.2})",
            ctrl.target_cells,
            ratio
        );
    }

    #[test]
    fn build_bound_does_not_shrink_fetch_threads() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial_threads = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Near-zero waste → Build classification.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 2000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Build);
        assert!(
            ctrl.fetch_threads >= initial_threads,
            "build-bound must not shrink fetch_threads: {} < initial {}",
            ctrl.fetch_threads,
            initial_threads
        );
    }

    #[test]
    fn bg_jobs_bounds_enforced() {
        let mut ctrl = BottleneckController::new(200_000, 12, 4, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // Fetch-bound: bg_jobs should shrink but not below min.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 5000.0,
                build_ms: 1000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
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
                actual_cells: u64::MAX,
            });
        }
        assert!(ctrl.bg_jobs <= 4);
    }

    #[test]
    fn l0_alone_does_not_trigger_flush_but_bumps_bg_jobs() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        ctrl.bg_jobs = 6;
        ctrl.prev_bg_jobs = 6;

        ctrl.observe(&healthy_signals()); // warmup

        // High L0 but flush channel is empty — pipeline has no backpressure.
        // Should NOT classify as Flush (would suppress fetch needlessly).
        // But should proactively bump bg_jobs to help compaction.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 2000.0,
                flush_wait_ms: 0.0,
                l0_files: 60,
                actual_cells: u64::MAX,
            });
        }

        assert_ne!(
            ctrl.classify(),
            Bottleneck::Flush,
            "L0 alone with empty flush channel must not trigger Flush"
        );
        assert!(
            ctrl.bg_jobs >= ctrl.min_bg_jobs,
            "bg_jobs should stay within bounds: {}",
            ctrl.bg_jobs
        );
    }

    #[test]
    fn flush_wait_dominates_waste_triggers_flush() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // flush_wait dominates waste → Flush classification.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 100.0,
                build_ms: 2000.0,
                flush_wait_ms: 3000.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert_eq!(ctrl.classify(), Bottleneck::Flush);
    }

    #[test]
    fn bg_jobs_if_changed_tracks_transitions() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        assert!(ctrl.bg_jobs_if_changed().is_none());

        ctrl.bg_jobs = 6;
        assert_eq!(ctrl.bg_jobs_if_changed(), Some(6));
        assert!(ctrl.bg_jobs_if_changed().is_none());
    }

    #[test]
    fn target_cells_never_reaches_zero() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // Very high build_ms → shrink aggressively, should never reach zero.
        for _ in 0..100 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 100_000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
            });
        }

        assert!(
            ctrl.target_cells >= 1,
            "target_cells must never be zero: {}",
            ctrl.target_cells
        );
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
    fn bottleneck_code_round_trip() {
        for b in [Bottleneck::Fetch, Bottleneck::Build, Bottleneck::Flush] {
            assert_eq!(Bottleneck::from_code(b.to_code()), Some(b));
        }
        assert_eq!(Bottleneck::from_code(0), None);
        assert_eq!(Bottleneck::from_code(255), None);
    }

    #[test]
    fn fetch_threads_stable_when_build_bound_grow_when_fetch_bound() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.fetch_threads;

        ctrl.observe(&healthy_signals()); // warmup

        // Build-bound: fetch_threads must NOT shrink.
        for _ in 0..10 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 50.0,
                build_ms: 5000.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: u64::MAX,
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
                actual_cells: u64::MAX,
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
                actual_cells: u64::MAX,
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
    fn max_batch_bytes_scales_with_ram() {
        // 8 GB → max 512 MB
        let ctrl_8 = BottleneckController::new(200_000, 12, 8, 8 * GB);
        assert_eq!(ctrl_8.max_batch_bytes, 512 * 1024 * 1024);

        // 32 GB → max 2 GB
        let ctrl_32 = BottleneckController::new(200_000, 12, 8, 32 * GB);
        assert_eq!(ctrl_32.max_batch_bytes, 2 * GB);

        // 128 GB → 128*1024^3/16 = 8 GiB, capped at ABSOLUTE_MAX (8_000_000_000)
        let ctrl_128 = BottleneckController::new(200_000, 12, 8, 128 * GB);
        assert_eq!(ctrl_128.max_batch_bytes, ABSOLUTE_MAX_BATCH_BYTES);
    }

    #[test]
    fn max_batch_bytes_derived_from_ram() {
        let ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        assert_eq!(ctrl.max_batch_bytes(), 2 * GB);
    }

    #[test]
    fn supply_cap_prevents_runaway_when_overlap_low() {
        // When overlap is below target and actual_cells is small (genuinely
        // supply-limited), target_cells should be capped at 2× actual.
        let mut ctrl = BottleneckController::new(500_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // Recv-dominated waste (overlap ~50%), only 100K cells delivered.
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 500.0,
                build_ms: 500.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: 100_000,
            });
        }

        // target_cells should be capped near 200K (2× actual), not millions.
        assert!(
            ctrl.target_cells <= 200_000,
            "supply cap should prevent runaway: target_cells={} should be <= 200000",
            ctrl.target_cells
        );
    }

    #[test]
    fn supply_cap_skipped_when_overlap_healthy() {
        // When overlap is above target, low actual_cells reflects sparse data
        // (few cells per block), not a pipeline bottleneck.  The supply cap
        // must NOT apply — otherwise it creates a death spiral in low-density
        // chain regions.
        let mut ctrl = BottleneckController::new(500_000, 12, 8, 32 * GB);

        ctrl.observe(&healthy_signals()); // warmup

        // Perfect overlap, but very few cells per batch (sparse blocks).
        for _ in 0..20 {
            ctrl.observe(&BatchSignals {
                prefetch_recv_ms: 0.0,
                build_ms: 500.0,
                flush_wait_ms: 0.0,
                l0_files: 5,
                actual_cells: 1,
            });
        }

        // target_cells should grow freely — NOT be clamped to 2.
        assert!(
            ctrl.target_cells > 1000,
            "supply cap should not apply with healthy overlap: target_cells={} should be > 1000",
            ctrl.target_cells
        );
    }
}
