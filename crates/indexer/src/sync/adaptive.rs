use std::sync::atomic::{AtomicU64, Ordering};

// ── Live sync batch span controller ──────────────────────────────────
//
// Live sync only.  Bulk sync uses BottleneckController (bottleneck.rs).
//
// Computes block_span from a fixed target transaction count and a
// density EMA (txs per block).  The channel backpressure from the
// bounded pipeline naturally limits throughput — no pressure detection
// or adaptive sizing needed.

pub(crate) const LIVE_BATCH_TARGET_TXS: u64 = 40_000;
pub(crate) const LIVE_BATCH_MIN_BLOCKS: u64 = 1;
pub(crate) const LIVE_BATCH_MAX_BLOCKS: u64 = 5_000;
const TPB_EMA_ALPHA_PCT: u64 = 20;
const INITIAL_TPB_MILLI: u64 = 20_000; // 20 txs/block initial estimate

// ── Non-adaptive constants that live alongside the batch controller ──

pub(crate) const BULK_PHASE_COMMIT_SLOW_WARN_MS: f64 = 2_000.0;
pub(crate) const UDT_CELL_CACHE_CAPACITY: usize = 100_000;
pub(crate) const PARSER_UNRESOLVED_RETRY_DELAY_MS: u64 = 500;
pub(crate) const PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE: usize = 5;
pub(crate) const PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS: u64 = 8;

// ── LiveBatchController ──────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct LiveBatchController {
    tx_per_block_milli_ema: AtomicU64,
}

impl LiveBatchController {
    pub(crate) fn new() -> Self {
        Self {
            tx_per_block_milli_ema: AtomicU64::new(INITIAL_TPB_MILLI),
        }
    }

    /// Compute block span from the fixed tx target and current density EMA.
    pub(crate) fn estimate_block_span(&self) -> u64 {
        let tx_per_block_milli = self.tx_per_block_milli_ema.load(Ordering::Relaxed).max(1);
        let estimated = ((LIVE_BATCH_TARGET_TXS * 1000).saturating_add(tx_per_block_milli - 1))
            / tx_per_block_milli;
        estimated.clamp(LIVE_BATCH_MIN_BLOCKS, LIVE_BATCH_MAX_BLOCKS)
    }

    /// Update the density EMA after observing a batch.
    pub(crate) fn observe_tx_density(&self, tx_count: usize, block_count: usize) {
        if tx_count == 0 || block_count == 0 {
            return;
        }
        let sample = (((tx_count as u64) * 1000).saturating_add(block_count as u64 - 1))
            / block_count as u64;
        let alpha = TPB_EMA_ALPHA_PCT.min(100);
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

    #[test]
    fn estimate_block_span_clamps_to_bounds() {
        let ctrl = LiveBatchController::new();
        // High density: 2000 tx/block → 40000/2000 = 20 blocks
        ctrl.tx_per_block_milli_ema
            .store(2_000_000, Ordering::Relaxed);
        assert_eq!(ctrl.estimate_block_span(), 20);

        // Low density: 1 tx/block → 40000 blocks, capped at MAX
        ctrl.tx_per_block_milli_ema.store(1_000, Ordering::Relaxed);
        assert_eq!(ctrl.estimate_block_span(), LIVE_BATCH_MAX_BLOCKS);
    }

    #[test]
    fn observe_tx_density_updates_ema() {
        let ctrl = LiveBatchController::new();
        let before = ctrl.tx_per_block_milli_ema.load(Ordering::Relaxed);

        // Feed high density: 100 tx/block = 100_000 milli
        ctrl.observe_tx_density(1000, 10);
        let after = ctrl.tx_per_block_milli_ema.load(Ordering::Relaxed);

        // EMA should move toward 100_000 from initial 20_000
        assert!(
            after > before,
            "EMA should increase: {} vs {}",
            after,
            before
        );
    }

    #[test]
    fn observe_tx_density_ignores_zero() {
        let ctrl = LiveBatchController::new();
        let before = ctrl.tx_per_block_milli_ema.load(Ordering::Relaxed);
        ctrl.observe_tx_density(0, 10);
        assert_eq!(ctrl.tx_per_block_milli_ema.load(Ordering::Relaxed), before);
        ctrl.observe_tx_density(10, 0);
        assert_eq!(ctrl.tx_per_block_milli_ema.load(Ordering::Relaxed), before);
    }

    #[test]
    fn bump_pipeline_reset_epoch_is_monotonic() {
        let epoch = AtomicU64::new(0);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 1);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 2);
        assert_eq!(epoch.load(Ordering::SeqCst), 2);
    }
}
