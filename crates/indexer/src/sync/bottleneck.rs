// Bottleneck-driven resource controller for bulk sync.
//
// Two independent control dimensions:
//
//   1. BATCH SIZE — build-time band + build/IO overlap.
//      Primary goal: keep batch build time in [2s, 5s].
//        Below band → grow target_cells to fill compute budget.
//        Above band → shrink target_cells (batch genuinely too large).
//      IO wait (recv + flush) is excluded from the band check:
//      shrinking batch size cannot reduce IO-bound time and only
//      increases per-batch overhead (prefetch, finalize, channel sync).
//      Secondary goal (in-band): push toward build ≈ IO.
//        build > IO → IO has headroom, grow batch (more data
//        may push IO toward its non-linear knee, increasing
//        throughput until build ≈ IO or build hits ceiling).
//        IO ≥ build → physical IO limit reached, hold steady.
//
//   2. I/O RESOURCES — governed by waste classification (ratio).
//      Waste = recv_wait + flush_wait (idle time, ideally zero).
//      Work = build (CPU time, never zero).
//      Classification identifies which waste source dominates and shifts
//      I/O knobs (fetch_threads, bg_jobs) accordingly.
//
//   Dimension  │ Signal            │ Knobs
//   ───────────│───────────────────│──────────────────────────
//   Batch size │ build time,       │ target_cells
//              │ build vs IO       │
//   I/O        │ waste composition │ fetch_threads, bg_jobs
//
// Key design principle: fetch (CKB RocksDB reads via std::thread::scope)
// does NOT compete with build (CPU via rayon) for resources.  Therefore
// Build-classified batches do NOT suppress fetch_threads.  Only Flush
// suppresses fetch threads — and only when the flush channel is actually
// filling up.

const EMA_ALPHA: f64 = 0.5;

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

// Build-time target range (ms).  The controller keeps batch build
// time inside this band.  Below MIN → grow target_cells; above MAX →
// shrink.  Inside the band → optimize build/IO overlap.  IO wait
// (recv + flush) is excluded — it cannot be reduced by batch sizing.
// Band: [2000, 5000] — wider window reduces oscillation on machines
// where build time varies batch-to-batch.
const BUILD_TIME_MIN: f64 = 2000.0;
const BUILD_TIME_MAX: f64 = 5000.0;

// Overlap growth gain.  When build > IO inside the wall-clock band,
// target_cells grows by OVERLAP_GAIN × (build − IO) / wall_clock.
// 0.5 = moderate: converges toward build ≈ IO in ~4–6 batches.
const OVERLAP_GAIN: f64 = 0.5;

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
    /// Actual cells consumed by the batch (from drain_by_cells).
    /// Used to detect supply-limited batches and prevent unbounded target growth.
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

        // ── Batch size: build-time band + build/IO overlap ─────────
        //
        // Priority 1 (override): keep build in [BUILD_TIME_MIN, BUILD_TIME_MAX].
        //   Below band → grow to fill compute budget.
        //   Above band → shrink (batch genuinely too large).
        //   IO wait (recv + flush) is excluded: shrinking batch size
        //   cannot reduce IO-bound time and only adds per-batch overhead.
        //
        // Priority 2 (in-band): optimize build/IO overlap.
        //   build > IO → IO has headroom, grow toward build ≈ IO.
        //   IO ≥ build → IO-bound (physical limit), hold steady.
        let wall_clock = self.recv_ema + self.build_ema + self.wait_ema;
        let io_ms = self.recv_ema + self.wait_ema;

        let factor = if self.build_ema < 1.0 {
            // Near-zero build (cold start) — grow to discover capacity.
            STEP_CEIL
        } else if self.build_ema < BUILD_TIME_MIN {
            // Below band: batch too small, grow to fill compute budget.
            (BUILD_TIME_MIN / self.build_ema).min(STEP_CEIL)
        } else if self.build_ema > BUILD_TIME_MAX {
            // Above band: build itself is too slow, batch genuinely too large.
            (BUILD_TIME_MAX / self.build_ema).max(STEP_FLOOR)
        } else {
            // Build in band: optimize build/IO overlap.
            if self.build_ema > io_ms {
                // Build-dominant: IO has headroom, grow toward build ≈ IO.
                // Growth proportional to IO gap, capped by build ceiling.
                let overlap_factor = 1.0 + OVERLAP_GAIN * (self.build_ema - io_ms) / wall_clock;
                let build_cap = BUILD_TIME_MAX / self.build_ema;
                overlap_factor.min(build_cap)
            } else {
                // IO-dominant: at physical limit, hold steady.
                1.0
            }
        };
        let factor = factor.clamp(STEP_FLOOR, STEP_CEIL);
        let raw = ((self.target_cells as f64 * factor) as u64).max(1);
        // Cap target to 4× actual cells consumed.  Prevents unbounded growth
        // when supply-limited (actual << target): the controller would keep
        // growing because timing signals don't change when the batch can't
        // fill the budget.  When demand-limited (actual ≈ target), the cap
        // is 4× target — well above the max factor of 2× — so it never binds.
        let supply_cap = signals.actual_cells.saturating_mul(4).max(1);
        self.target_cells = raw.min(supply_cap);

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

    fn signals(recv: f64, build: f64, flush: f64) -> BatchSignals {
        BatchSignals {
            prefetch_recv_ms: recv,
            build_ms: build,
            flush_wait_ms: flush,
            l0_files: 5,
            actual_cells: u64::MAX, // demand-limited (cap never binds)
        }
    }

    /// Feed warmup + N identical batches, return final output.
    fn run_batches(
        ctrl: &mut BottleneckController,
        n: usize,
        sig: &BatchSignals,
    ) -> ControllerOutput {
        ctrl.observe(sig); // warmup (returns None)
        let mut last = None;
        for _ in 0..n {
            last = ctrl.observe(sig);
        }
        last.expect("should have output after warmup + N batches")
    }

    // ── Build-time band tests ──────────────────────────────────────

    #[test]
    fn first_batch_returns_none() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        assert!(ctrl.observe(&signals(100.0, 2000.0, 0.0)).is_none());
    }

    #[test]
    fn cold_start_grows_to_discover_capacity() {
        // When build EMA is near zero (cold start after warmup),
        // build_ema < 1.0 → factor = STEP_CEIL = 2.0
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        // Warmup with near-zero signals
        ctrl.observe(&signals(0.0, 0.0, 0.0));
        // Second batch also near-zero — EMAs stay near zero
        ctrl.observe(&signals(0.0, 0.1, 0.0));
        assert!(
            ctrl.target_cells > initial,
            "cold start should grow: {} should be > initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn build_below_min_grows() {
        // build = 300 < 1000 → grow (batch too small)
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        run_batches(&mut ctrl, 5, &signals(200.0, 300.0, 0.0));
        assert!(
            ctrl.target_cells > initial,
            "build < MIN should grow: {} should be > initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn build_above_max_shrinks() {
        // build = 7000 > 5000 → shrink (batch genuinely too large)
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        run_batches(&mut ctrl, 5, &signals(100.0, 7000.0, 100.0));
        assert!(
            ctrl.target_cells < initial,
            "build > MAX should shrink: {} should be < initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn io_wait_above_band_does_not_shrink() {
        // build = 3500 ∈ [2000, 5000], IO = 500 + 2000 = 2500
        // IO-dominant: hold (don't shrink for flush wait)
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        run_batches(&mut ctrl, 10, &signals(500.0, 3500.0, 2000.0));
        assert!(
            ctrl.target_cells >= initial,
            "IO wait should not cause shrink: {} should be >= initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn in_range_build_dominant_grows() {
        // build = 3500 ∈ [2000, 5000], IO = 200
        // build(3500) > IO(200) → grow
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        run_batches(&mut ctrl, 5, &signals(100.0, 3500.0, 100.0));
        assert!(
            ctrl.target_cells > initial,
            "in-range build-dominant should grow: {} should be > initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn in_band_io_dominant_holds() {
        // build = 4000 ∈ [2000, 5000], IO = 2400 + 2400 = 4800
        // build(4000) < IO(4800) → hold.
        // build value chosen so build_ema reaches band floor (2000)
        // after one EMA step (4000 * 0.5 = 2000), avoiding warmup growth.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        run_batches(&mut ctrl, 10, &signals(2400.0, 4000.0, 2400.0));
        assert_eq!(
            ctrl.target_cells, initial,
            "in-band IO-dominant should hold: {} should equal initial {}",
            ctrl.target_cells, initial
        );
    }

    #[test]
    fn build_below_min_grows_even_when_io_dominant() {
        // build = 500 < 2000 (below band), IO = 800 + 700 = 1500
        // Even though IO > build, batch is too small → grow.
        // IO wait cannot be reduced by batch sizing.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.target_cells;
        run_batches(&mut ctrl, 5, &signals(800.0, 500.0, 700.0));
        assert!(
            ctrl.target_cells > initial,
            "build < MIN should grow regardless of IO: {} should be > initial {}",
            ctrl.target_cells,
            initial
        );
    }

    #[test]
    fn build_equals_io_converges() {
        // build = 4000, IO = 2000 + 2000 = 4000
        // build(4000) = IO(4000) → factor = 1.0.
        // build value chosen so build_ema enters band (2000) after one EMA step.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        // After warmup the EMAs won't be exactly 4000/4000 due to smoothing,
        // but after many batches they converge. Check stability.
        run_batches(&mut ctrl, 20, &signals(2000.0, 4000.0, 2000.0));
        let ratio = ctrl.target_cells as f64 / 200_000.0;
        assert!(
            ratio > 0.8 && ratio < 1.2,
            "build ≈ IO should stabilize: ratio={:.3} (target={})",
            ratio,
            ctrl.target_cells
        );
    }

    #[test]
    fn growth_capped_by_build_ceiling() {
        // build(4800) near ceiling → build_cap = 5000/build_ema limits growth.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        run_batches(&mut ctrl, 1, &signals(50.0, 4800.0, 50.0));
        // After 1 batch the EMA is a blend with warmup, but factor should be small.
        // Verify target_cells didn't jump more than 50%.
        assert!(
            ctrl.target_cells < 300_000,
            "growth near ceiling should be modest: {}",
            ctrl.target_cells
        );
    }

    // ── I/O resource tests (unchanged behavior) ────────────────────

    #[test]
    fn fetch_starved_grows_fetch_threads() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial_threads = ctrl.fetch_threads;

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

        // Shrink threads first via flush pressure.
        for _ in 0..10 {
            ctrl.observe(&signals(50.0, 1000.0, 5000.0));
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
            ctrl.observe(&signals(5000.0, 1000.0, 0.0));
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

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

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
    fn build_bound_does_not_shrink_fetch_threads() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial_threads = ctrl.fetch_threads;

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

        // Near-zero waste → Build classification.
        for _ in 0..20 {
            ctrl.observe(&signals(0.0, 2000.0, 0.0));
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

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

        // Fetch-bound: bg_jobs should shrink but not below min.
        for _ in 0..20 {
            ctrl.observe(&signals(5000.0, 1000.0, 0.0));
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

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

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
    }

    #[test]
    fn flush_wait_dominates_waste_triggers_flush() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

        for _ in 0..10 {
            ctrl.observe(&signals(100.0, 2000.0, 3000.0));
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

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

        // build >> 3000 → aggressive shrink, should never hit zero.
        for _ in 0..100 {
            ctrl.observe(&signals(0.0, 100_000.0, 0.0));
        }

        assert!(
            ctrl.target_cells >= 1,
            "target_cells must never be zero: {}",
            ctrl.target_cells
        );
    }

    #[test]
    fn fetch_threads_stable_when_build_bound_grow_when_fetch_bound() {
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        let initial = ctrl.fetch_threads;

        ctrl.observe(&signals(100.0, 2000.0, 0.0)); // warmup

        // Build-bound: fetch_threads must NOT shrink.
        for _ in 0..10 {
            ctrl.observe(&signals(50.0, 5000.0, 0.0));
        }
        assert_eq!(
            ctrl.fetch_threads, initial,
            "build-bound must not change fetch_threads"
        );

        // Flush-bound: fetch_threads should shrink.
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
            ctrl.observe(&signals(5000.0, 1000.0, 0.0));
        }
        assert!(
            ctrl.fetch_threads > after_flush,
            "fetch-bound should grow fetch_threads: {} vs after_flush {}",
            ctrl.fetch_threads,
            after_flush
        );
    }

    // ── Infrastructure tests (unchanged) ───────────────────────────

    #[test]
    fn channel_depth_scales_with_ram() {
        assert_eq!(channel_depth_for_ram(8 * GB), 1);
        assert_eq!(channel_depth_for_ram(16 * GB), 2);
        assert_eq!(channel_depth_for_ram(32 * GB), 4);
        assert_eq!(channel_depth_for_ram(64 * GB), 8);
        assert_eq!(channel_depth_for_ram(128 * GB), 8);
        assert_eq!(channel_depth_for_ram(256 * GB), 8);
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
    fn max_batch_bytes_scales_with_ram() {
        let ctrl_8 = BottleneckController::new(200_000, 12, 8, 8 * GB);
        assert_eq!(ctrl_8.max_batch_bytes, 512 * 1024 * 1024);

        let ctrl_32 = BottleneckController::new(200_000, 12, 8, 32 * GB);
        assert_eq!(ctrl_32.max_batch_bytes, 2 * GB);

        let ctrl_128 = BottleneckController::new(200_000, 12, 8, 128 * GB);
        assert_eq!(ctrl_128.max_batch_bytes, ABSOLUTE_MAX_BATCH_BYTES);
    }

    #[test]
    fn max_batch_bytes_derived_from_ram() {
        let ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        assert_eq!(ctrl.max_batch_bytes(), 2 * GB);
    }

    // ── Supply-feedback tests ─────────────────────────────────────────

    #[test]
    fn supply_limited_caps_target_cells() {
        // Reproduces the bug: build in-band, zero waste, but prefetch
        // can only supply 50K cells — target must not grow to u64::MAX.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);

        let supply_limited = BatchSignals {
            prefetch_recv_ms: 0.0,
            build_ms: 2200.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            actual_cells: 50_000, // supply-limited: much less than target
        };

        ctrl.observe(&supply_limited); // warmup
        for _ in 0..100 {
            ctrl.observe(&supply_limited);
        }

        // With 4× supply cap, target should be at most 4 * 50_000 = 200_000.
        assert!(
            ctrl.target_cells <= 200_000,
            "supply-limited target must be capped: {} should be <= 200_000",
            ctrl.target_cells
        );
    }

    #[test]
    fn supply_limited_recovers_from_diverged_target() {
        // If target has already diverged (e.g. u64::MAX), one batch with
        // actual_cells feedback should snap it back.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);
        ctrl.target_cells = u64::MAX; // simulate already-diverged state

        let supply_limited = BatchSignals {
            prefetch_recv_ms: 0.0,
            build_ms: 2200.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            actual_cells: 100_000,
        };

        ctrl.observe(&supply_limited); // warmup
        ctrl.observe(&supply_limited); // first real batch

        assert!(
            ctrl.target_cells <= 400_000,
            "diverged target must snap back: {} should be <= 400_000 (4 × 100K)",
            ctrl.target_cells
        );
    }

    #[test]
    fn demand_limited_cap_does_not_bind() {
        // When actual ≈ target (demand-limited), the 4× cap should not
        // interfere with normal wall-clock / overlap growth.
        let mut ctrl = BottleneckController::new(200_000, 12, 8, 32 * GB);

        // wall = 500 < 1000 → grow. actual_cells = target so cap = 4× target.
        // factor ≤ 2.0 < 4.0 → cap never binds.
        let demand_limited = BatchSignals {
            prefetch_recv_ms: 200.0,
            build_ms: 300.0,
            flush_wait_ms: 0.0,
            l0_files: 5,
            actual_cells: 200_000, // matches initial target
        };

        ctrl.observe(&demand_limited); // warmup
        ctrl.observe(&demand_limited);

        assert!(
            ctrl.target_cells > 200_000,
            "demand-limited should still grow normally: {}",
            ctrl.target_cells
        );
    }
}
