use std::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, Ordering};

use super::helpers::{encode_adaptive_batch_reason, ADAPTIVE_REASON_UNKNOWN};

// ── Adaptive batch controller constants ──────────────────────────────
//
// Design: maximize throughput by growing batch size and parallelism
// aggressively, backing off only when RocksDB compaction or write
// latency signals pressure.  A cooldown prevents oscillation.

pub(crate) const ADAPTIVE_BATCH_MAX_TXS: u64 = 160_000;
pub(crate) const ADAPTIVE_BATCH_INITIAL_TXS: u64 = 40_000;
const ADAPTIVE_BATCH_BULK_MIN_TXS: u64 = 10_000;
const ADAPTIVE_BATCH_LIVE_MIN_TXS: u64 = 2_000;
pub(crate) const ADAPTIVE_BATCH_MIN_BLOCKS: u64 = 1;
pub(crate) const ADAPTIVE_BATCH_MAX_BLOCKS: u64 = 5_000;
pub(crate) const ADAPTIVE_BATCH_TPB_EMA_ALPHA_PCT: u64 = 20;
pub(crate) const ADAPTIVE_BATCH_INITIAL_TPB_MILLI: u64 = 20_000;
pub(crate) const ADAPTIVE_BATCH_INITIAL_INFLIGHT: u64 = 3;

// Write latency thresholds
const ADAPTIVE_SEVERE_WRITE_MS: f64 = 10_000.0;
const ADAPTIVE_SEVERE_COMMIT_MS: f64 = 3_000.0;
const ADAPTIVE_MODERATE_WRITE_MS: f64 = 6_000.0;

// RocksDB L0 thresholds (mode-dependent)
const ADAPTIVE_BULK_L0_MODERATE: u64 = 64;
const ADAPTIVE_BULK_L0_SEVERE: u64 = 96;
const ADAPTIVE_LIVE_L0_MODERATE: u64 = 20;
const ADAPTIVE_LIVE_L0_SEVERE: u64 = 40;

// Growth / shrink factors (percent)
const ADAPTIVE_SEVERE_SHRINK_PCT: u64 = 50;
const ADAPTIVE_MODERATE_SHRINK_PCT: u64 = 80;
const ADAPTIVE_BULK_GROW_PCT: u64 = 125;
const ADAPTIVE_LIVE_GROW_PCT: u64 = 110;

// Cooldown (batches to wait before growing after pressure)
const ADAPTIVE_SEVERE_COOLDOWN: u64 = 3;
const ADAPTIVE_MODERATE_COOLDOWN: u64 = 2;
const ADAPTIVE_SEVERE_STREAK_REQUIRED: u64 = 2;

// ── Non-adaptive constants that live alongside the batch controller ──

pub(crate) const BULK_PHASE_COMMIT_SLOW_WARN_MS: f64 = 2_000.0;
pub(crate) const UDT_CELL_CACHE_CAPACITY: usize = 100_000;
pub(crate) const PARSER_UNRESOLVED_RETRY_DELAY_MS: u64 = 500;
pub(crate) const PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE: usize = 5;
pub(crate) const PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS: u64 = 8;

// ── Snapshot / input / adjustment types ──────────────────────────────

#[derive(Debug, Clone, Copy)]
pub(crate) struct AdaptiveBatchSnapshot {
    pub(crate) target_batch_txs: u64,
    pub(crate) inflight_limit: u64,
    pub(crate) min_target_batch_txs: u64,
    pub(crate) cooldown_steps: u64,
    pub(crate) last_reason_code: u8,
    pub(crate) adjustment_seq: u64,
    pub(crate) backoff_streak: u64,
    pub(crate) last_adjusted_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AdaptiveBatchProgressSnapshot {
    pub target_batch_txs: u64,
    pub inflight_limit: u64,
    pub min_target_batch_txs: u64,
    pub cooldown_steps: u64,
    pub last_reason: Option<String>,
    pub adjustment_seq: u64,
    pub backoff_streak: u64,
    pub last_adjusted_at: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AdaptiveBatchInput {
    pub(crate) write_ms: f64,
    pub(crate) commit_ms: f64,
    pub(crate) l0_files_max: Option<u64>,
    pub(crate) compaction_pending_bytes: Option<u64>,
    pub(crate) immutable_memtables: Option<u64>,
    pub(crate) severe_pending_threshold: u64,
    pub(crate) moderate_pending_threshold: u64,
    pub(crate) severe_imm_threshold: u64,
    pub(crate) moderate_imm_threshold: u64,
    pub(crate) is_bulk_sync: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AdaptiveBatchAdjustment {
    pub(crate) previous_target_batch_txs: u64,
    pub(crate) new_target_batch_txs: u64,
    pub(crate) previous_inflight_limit: u64,
    pub(crate) new_inflight_limit: u64,
    pub(crate) previous_min_target_batch_txs: u64,
    pub(crate) new_min_target_batch_txs: u64,
    pub(crate) reason: &'static str,
}

// ── AdaptiveBatchController ──────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct AdaptiveBatchController {
    target_batch_txs: AtomicU64,
    inflight_limit: AtomicU64,
    tx_per_block_milli_ema: AtomicU64,
    cooldown_steps: AtomicU64,
    last_reason_code: AtomicU8,
    adjustment_seq: AtomicU64,
    severe_streak: AtomicU64,
    last_adjusted_at: AtomicI64,
    max_inflight_limit: u64,
}

impl AdaptiveBatchController {
    pub(crate) fn new(max_inflight_limit: u64) -> Self {
        let max_inflight_limit = max_inflight_limit.max(1);
        let initial_inflight = ADAPTIVE_BATCH_INITIAL_INFLIGHT.min(max_inflight_limit);
        Self {
            target_batch_txs: AtomicU64::new(ADAPTIVE_BATCH_INITIAL_TXS),
            inflight_limit: AtomicU64::new(initial_inflight),
            tx_per_block_milli_ema: AtomicU64::new(ADAPTIVE_BATCH_INITIAL_TPB_MILLI),
            cooldown_steps: AtomicU64::new(0),
            last_reason_code: AtomicU8::new(ADAPTIVE_REASON_UNKNOWN),
            adjustment_seq: AtomicU64::new(0),
            severe_streak: AtomicU64::new(0),
            last_adjusted_at: AtomicI64::new(0),
            max_inflight_limit,
        }
    }

    pub(crate) fn snapshot(&self) -> AdaptiveBatchSnapshot {
        let last_adjusted_at_raw = self.last_adjusted_at.load(Ordering::Relaxed);
        let is_bulk = true; // snapshot doesn't know mode; use bulk floor as default
        AdaptiveBatchSnapshot {
            target_batch_txs: self.target_batch_txs.load(Ordering::Relaxed),
            inflight_limit: self.inflight_limit.load(Ordering::Relaxed),
            min_target_batch_txs: if is_bulk {
                ADAPTIVE_BATCH_BULK_MIN_TXS
            } else {
                ADAPTIVE_BATCH_LIVE_MIN_TXS
            },
            cooldown_steps: self.cooldown_steps.load(Ordering::Relaxed),
            last_reason_code: self.last_reason_code.load(Ordering::Relaxed),
            adjustment_seq: self.adjustment_seq.load(Ordering::Relaxed),
            backoff_streak: 0,
            last_adjusted_at: (last_adjusted_at_raw > 0).then_some(last_adjusted_at_raw),
        }
    }

    fn record_adjustment(&self, reason_code: u8) {
        self.last_reason_code.store(reason_code, Ordering::Relaxed);
        self.adjustment_seq.fetch_add(1, Ordering::Relaxed);
        self.last_adjusted_at
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    }

    pub(crate) fn estimate_block_span(&self, batch_block_cap: u64) -> u64 {
        let batch_block_cap = batch_block_cap.clamp(1, ADAPTIVE_BATCH_MAX_BLOCKS);
        let min_blocks = ADAPTIVE_BATCH_MIN_BLOCKS.min(batch_block_cap);
        let target_batch_txs = self.target_batch_txs.load(Ordering::Relaxed).max(1);
        let tx_per_block_milli = self.tx_per_block_milli_ema.load(Ordering::Relaxed).max(1);
        let estimated =
            ((target_batch_txs * 1000).saturating_add(tx_per_block_milli - 1)) / tx_per_block_milli;
        estimated.clamp(min_blocks, batch_block_cap)
    }

    pub(crate) fn observe_tx_density(&self, tx_count: usize, block_count: usize) {
        if tx_count == 0 || block_count == 0 {
            return;
        }
        let sample = (((tx_count as u64) * 1000).saturating_add(block_count as u64 - 1))
            / block_count as u64;
        let alpha = ADAPTIVE_BATCH_TPB_EMA_ALPHA_PCT.min(100);
        loop {
            let old = self.tx_per_block_milli_ema.load(Ordering::Relaxed).max(1);
            let blended = ((old.saturating_mul(100 - alpha)).saturating_add(sample * alpha)) / 100;
            if self
                .tx_per_block_milli_ema
                .compare_exchange(old, blended.max(1), Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Core decision function: detect pressure → backoff, or grow.
    ///
    /// Bulk sync mode grows aggressively (25% per healthy batch) and uses
    /// higher L0 thresholds.  Live mode grows conservatively (10%).
    /// Severe pressure requires two consecutive batches to trigger,
    /// preventing noisy single-batch spikes from causing large cuts.
    pub(crate) fn update_after_write(
        &self,
        input: AdaptiveBatchInput,
    ) -> Option<AdaptiveBatchAdjustment> {
        let previous_target = self.target_batch_txs.load(Ordering::Relaxed);
        let previous_inflight = self.inflight_limit.load(Ordering::Relaxed);
        let min_txs = if input.is_bulk_sync {
            ADAPTIVE_BATCH_BULK_MIN_TXS
        } else {
            ADAPTIVE_BATCH_LIVE_MIN_TXS
        };
        let mut new_target = previous_target;
        let mut new_inflight = previous_inflight;
        let reason: &'static str;

        // ── Detect pressure ──────────────────────────────────────────
        let (l0_moderate, l0_severe) = if input.is_bulk_sync {
            (ADAPTIVE_BULK_L0_MODERATE, ADAPTIVE_BULK_L0_SEVERE)
        } else {
            (ADAPTIVE_LIVE_L0_MODERATE, ADAPTIVE_LIVE_L0_SEVERE)
        };

        let writer_severe_signal = input.write_ms >= ADAPTIVE_SEVERE_WRITE_MS
            || input.commit_ms >= ADAPTIVE_SEVERE_COMMIT_MS
            || input.l0_files_max.is_some_and(|l0| l0 >= l0_severe)
            || input
                .compaction_pending_bytes
                .is_some_and(|b| b >= input.severe_pending_threshold)
            || input
                .immutable_memtables
                .is_some_and(|imm| imm >= input.severe_imm_threshold);

        let severe_streak = if writer_severe_signal {
            self.severe_streak.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.severe_streak.store(0, Ordering::Relaxed);
            0
        };
        let severe = severe_streak >= ADAPTIVE_SEVERE_STREAK_REQUIRED;

        let moderate = !severe
            && (input.write_ms >= ADAPTIVE_MODERATE_WRITE_MS
                || input.l0_files_max.is_some_and(|l0| l0 >= l0_moderate)
                || input
                    .compaction_pending_bytes
                    .is_some_and(|b| b >= input.moderate_pending_threshold)
                || input
                    .immutable_memtables
                    .is_some_and(|imm| imm >= input.moderate_imm_threshold));

        // ── Decide ───────────────────────────────────────────────────
        if severe {
            new_target = previous_target * ADAPTIVE_SEVERE_SHRINK_PCT / 100;
            new_inflight = previous_inflight.saturating_sub(1).max(1);
            self.cooldown_steps
                .store(ADAPTIVE_SEVERE_COOLDOWN, Ordering::Relaxed);
            reason = "severe_backoff";
        } else if moderate {
            new_target = previous_target * ADAPTIVE_MODERATE_SHRINK_PCT / 100;
            self.cooldown_steps
                .store(ADAPTIVE_MODERATE_COOLDOWN, Ordering::Relaxed);
            reason = "moderate_backoff";
        } else {
            let cooldown = self.cooldown_steps.load(Ordering::Relaxed);
            if cooldown > 0 {
                self.cooldown_steps.fetch_sub(1, Ordering::Relaxed);
                return None;
            }
            // Healthy: grow.
            let grow_pct = if input.is_bulk_sync {
                ADAPTIVE_BULK_GROW_PCT
            } else {
                ADAPTIVE_LIVE_GROW_PCT
            };
            if input.is_bulk_sync && previous_inflight < self.max_inflight_limit {
                // Bulk mode: grow parallelism first, then batch size.
                new_inflight = previous_inflight + 1;
                reason = "grow_inflight";
            } else {
                new_target = previous_target.saturating_mul(grow_pct).saturating_add(99) / 100;
                reason = "grow_batch";
            }
        }

        // ── Clamp ────────────────────────────────────────────────────
        new_target = new_target.clamp(min_txs, ADAPTIVE_BATCH_MAX_TXS);
        new_inflight = new_inflight.clamp(1, self.max_inflight_limit);

        if new_target == previous_target && new_inflight == previous_inflight {
            return None;
        }

        self.target_batch_txs.store(new_target, Ordering::Relaxed);
        self.inflight_limit.store(new_inflight, Ordering::Relaxed);
        self.record_adjustment(encode_adaptive_batch_reason(reason));

        Some(AdaptiveBatchAdjustment {
            previous_target_batch_txs: previous_target,
            new_target_batch_txs: new_target,
            previous_inflight_limit: previous_inflight,
            new_inflight_limit: new_inflight,
            previous_min_target_batch_txs: min_txs,
            new_min_target_batch_txs: min_txs,
            reason,
        })
    }
}

// ── Free functions ───────────────────────────────────────────────────

pub(super) fn bump_pipeline_reset_epoch(epoch: &AtomicU64) -> u64 {
    epoch.fetch_add(1, Ordering::SeqCst) + 1
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn default_thresholds() -> (u64, u64, u64, u64) {
        (
            8 * 1024 * 1024 * 1024, // severe_pending
            4 * 1024 * 1024 * 1024, // moderate_pending
            60,                     // severe_imm
            30,                     // moderate_imm
        )
    }

    fn healthy_input(is_bulk: bool) -> AdaptiveBatchInput {
        let (sp, mp, si, mi) = default_thresholds();
        AdaptiveBatchInput {
            write_ms: 1_000.0,
            commit_ms: 100.0,
            l0_files_max: Some(5),
            compaction_pending_bytes: Some(0),
            immutable_memtables: Some(2),
            severe_pending_threshold: sp,
            moderate_pending_threshold: mp,
            severe_imm_threshold: si,
            moderate_imm_threshold: mi,
            is_bulk_sync: is_bulk,
        }
    }

    fn severe_input(is_bulk: bool) -> AdaptiveBatchInput {
        let (sp, mp, si, mi) = default_thresholds();
        AdaptiveBatchInput {
            write_ms: ADAPTIVE_SEVERE_WRITE_MS + 1_000.0,
            commit_ms: ADAPTIVE_SEVERE_COMMIT_MS + 100.0,
            l0_files_max: Some(100),
            compaction_pending_bytes: Some(sp + 1),
            immutable_memtables: Some(si + 1),
            severe_pending_threshold: sp,
            moderate_pending_threshold: mp,
            severe_imm_threshold: si,
            moderate_imm_threshold: mi,
            is_bulk_sync: is_bulk,
        }
    }

    #[test]
    fn test_estimate_block_span_clamps_to_bounds() {
        let controller = AdaptiveBatchController::new(16);
        controller
            .target_batch_txs
            .store(100_000, Ordering::Relaxed);
        controller
            .tx_per_block_milli_ema
            .store(2_000_000, Ordering::Relaxed); // 2000 tx/block
        assert_eq!(controller.estimate_block_span(10_000), 50);

        controller
            .tx_per_block_milli_ema
            .store(1_000, Ordering::Relaxed); // 1 tx/block
        assert_eq!(controller.estimate_block_span(500), 500);
    }

    #[test]
    fn test_severe_pressure_requires_consecutive_batches() {
        let controller = AdaptiveBatchController::new(8);
        let input = severe_input(false);

        // First severe signal: streak=1, below threshold.
        let first = controller.update_after_write(input);
        assert!(first.is_some());
        // Should be moderate (only 1 streak), not severe.
        let adj = first.unwrap();
        assert_eq!(adj.reason, "moderate_backoff");

        // Second severe signal: streak=2, triggers severe.
        let second = controller.update_after_write(input).unwrap();
        assert_eq!(second.reason, "severe_backoff");
        assert!(second.new_target_batch_txs < second.previous_target_batch_txs);
        assert!(second.new_inflight_limit < second.previous_inflight_limit);
    }

    #[test]
    fn test_healthy_bulk_grows_inflight_first() {
        let controller = AdaptiveBatchController::new(8);
        let adj = controller
            .update_after_write(healthy_input(true))
            .expect("healthy bulk should adjust");
        assert_eq!(adj.reason, "grow_inflight");
        assert_eq!(adj.new_target_batch_txs, ADAPTIVE_BATCH_INITIAL_TXS);
        assert_eq!(adj.new_inflight_limit, ADAPTIVE_BATCH_INITIAL_INFLIGHT + 1);
    }

    #[test]
    fn test_healthy_bulk_grows_batch_when_inflight_maxed() {
        let controller = AdaptiveBatchController::new(8);
        controller.inflight_limit.store(8, Ordering::Relaxed); // at max
        let adj = controller
            .update_after_write(healthy_input(true))
            .expect("should grow batch when inflight maxed");
        assert_eq!(adj.reason, "grow_batch");
        assert!(adj.new_target_batch_txs > adj.previous_target_batch_txs);
    }

    #[test]
    fn test_healthy_live_grows_batch_size() {
        let controller = AdaptiveBatchController::new(8);
        let adj = controller
            .update_after_write(healthy_input(false))
            .expect("healthy live should grow batch");
        assert_eq!(adj.reason, "grow_batch");
        assert!(adj.new_target_batch_txs > adj.previous_target_batch_txs);
    }

    #[test]
    fn test_moderate_backoff_on_write_latency() {
        let controller = AdaptiveBatchController::new(8);
        let (sp, mp, si, mi) = default_thresholds();
        let adj = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_MODERATE_WRITE_MS + 100.0,
                commit_ms: 100.0,
                l0_files_max: Some(5),
                compaction_pending_bytes: Some(0),
                immutable_memtables: Some(2),
                severe_pending_threshold: sp,
                moderate_pending_threshold: mp,
                severe_imm_threshold: si,
                moderate_imm_threshold: mi,
                is_bulk_sync: false,
            })
            .expect("moderate write latency should trigger backoff");
        assert_eq!(adj.reason, "moderate_backoff");
        assert!(adj.new_target_batch_txs < adj.previous_target_batch_txs);
    }

    #[test]
    fn test_cooldown_blocks_growth_after_pressure() {
        let controller = AdaptiveBatchController::new(8);
        // Trigger moderate backoff.
        let (sp, mp, si, mi) = default_thresholds();
        let _ = controller.update_after_write(AdaptiveBatchInput {
            write_ms: ADAPTIVE_MODERATE_WRITE_MS + 100.0,
            commit_ms: 100.0,
            l0_files_max: Some(5),
            compaction_pending_bytes: Some(0),
            immutable_memtables: Some(2),
            severe_pending_threshold: sp,
            moderate_pending_threshold: mp,
            severe_imm_threshold: si,
            moderate_imm_threshold: mi,
            is_bulk_sync: false,
        });
        let snapshot_after_pressure = controller.snapshot();
        assert!(snapshot_after_pressure.cooldown_steps > 0);

        // Healthy input during cooldown: no adjustment.
        let no_adj = controller.update_after_write(healthy_input(false));
        assert!(no_adj.is_none(), "cooldown should block growth");
        let snapshot_after = controller.snapshot();
        assert_eq!(
            snapshot_after.target_batch_txs,
            snapshot_after_pressure.target_batch_txs
        );
    }

    #[test]
    fn test_bulk_l0_thresholds_are_higher() {
        let controller = AdaptiveBatchController::new(8);
        let (sp, mp, si, mi) = default_thresholds();
        // L0=30 in bulk mode: below bulk moderate (64), should be healthy.
        let adj = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 500.0,
                commit_ms: 50.0,
                l0_files_max: Some(30),
                compaction_pending_bytes: Some(0),
                immutable_memtables: Some(2),
                severe_pending_threshold: sp,
                moderate_pending_threshold: mp,
                severe_imm_threshold: si,
                moderate_imm_threshold: mi,
                is_bulk_sync: true,
            })
            .expect("l0=30 in bulk mode should be healthy");
        assert!(
            adj.reason == "grow_inflight" || adj.reason == "grow_batch",
            "expected growth, got: {}",
            adj.reason
        );

        // L0=30 in live mode: above live moderate (20), should backoff.
        let controller2 = AdaptiveBatchController::new(8);
        let adj2 = controller2
            .update_after_write(AdaptiveBatchInput {
                write_ms: 500.0,
                commit_ms: 50.0,
                l0_files_max: Some(30),
                compaction_pending_bytes: Some(0),
                immutable_memtables: Some(2),
                severe_pending_threshold: sp,
                moderate_pending_threshold: mp,
                severe_imm_threshold: si,
                moderate_imm_threshold: mi,
                is_bulk_sync: false,
            })
            .expect("l0=30 in live mode should trigger backoff");
        assert_eq!(adj2.reason, "moderate_backoff");
    }

    #[test]
    fn test_min_floor_enforced() {
        let controller = AdaptiveBatchController::new(8);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_BULK_MIN_TXS + 1, Ordering::Relaxed);

        // Moderate backoff should not go below bulk floor.
        let (sp, mp, si, mi) = default_thresholds();
        let adj = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_MODERATE_WRITE_MS + 100.0,
                commit_ms: 100.0,
                l0_files_max: Some(5),
                compaction_pending_bytes: Some(0),
                immutable_memtables: Some(2),
                severe_pending_threshold: sp,
                moderate_pending_threshold: mp,
                severe_imm_threshold: si,
                moderate_imm_threshold: mi,
                is_bulk_sync: true,
            })
            .expect("should clamp to bulk floor");
        assert!(adj.new_target_batch_txs >= ADAPTIVE_BATCH_BULK_MIN_TXS);
    }

    #[test]
    fn test_bump_pipeline_reset_epoch_is_monotonic() {
        let epoch = AtomicU64::new(0);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 1);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 2);
        assert_eq!(epoch.load(Ordering::SeqCst), 2);
    }
}
