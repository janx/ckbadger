use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};

use super::helpers::{
    decode_adaptive_batch_reason, encode_adaptive_batch_reason, ADAPTIVE_REASON_EARLY_HEIGHT_BOOST,
    ADAPTIVE_REASON_UNKNOWN,
};

// ── Adaptive batch controller constants ──────────────────────────────

pub(crate) const ADAPTIVE_BATCH_BASE_MIN_TXS: u64 = 10_000;
pub(crate) const ADAPTIVE_BATCH_HARD_MIN_TXS: u64 = 2_000;
pub(crate) const ADAPTIVE_BATCH_MAX_TXS: u64 = 160_000;
pub(crate) const ADAPTIVE_BATCH_INITIAL_TXS: u64 = 40_000;
pub(crate) const ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF: u64 = 4_000_000;
pub(crate) const ADAPTIVE_BATCH_EARLY_TARGET_TXS: u64 = 120_000;
pub(crate) const ADAPTIVE_BATCH_MIN_BLOCKS: u64 = 1;
pub(crate) const ADAPTIVE_BATCH_MAX_BLOCKS: u64 = 5_000;
pub(crate) const ADAPTIVE_BATCH_TPB_EMA_ALPHA_PCT: u64 = 20; // 0.20
pub(crate) const ADAPTIVE_BATCH_INITIAL_TPB_MILLI: u64 = 20_000; // 20.0 tx/block
pub(crate) const ADAPTIVE_BATCH_INITIAL_INFLIGHT: u64 = 3;
pub(crate) const ADAPTIVE_BATCH_WRITE_TARGET_MS: f64 = 2_500.0;
pub(crate) const ADAPTIVE_BATCH_WRITE_LO_MS: f64 = 1_500.0;
pub(crate) const ADAPTIVE_BATCH_WRITE_HI_MS: f64 = 6_000.0;
pub(crate) const ADAPTIVE_BATCH_WRITE_HEALTHY_US_PER_TX: f64 = 300.0;
pub(crate) const ADAPTIVE_BATCH_WRITE_TARGET_US_PER_TX: f64 = 450.0;
pub(crate) const ADAPTIVE_BATCH_WRITE_HI_US_PER_TX: f64 = 900.0;
pub(crate) const ADAPTIVE_BATCH_SEVERE_WRITE_MS: f64 = 10_000.0;
pub(crate) const ADAPTIVE_BATCH_SEVERE_COMMIT_MS: f64 = 3_000.0;
pub(crate) const ADAPTIVE_BATCH_SEVERE_WRITE_US_PER_TX: f64 = 1_500.0;
pub(crate) const ADAPTIVE_BATCH_SEVERE_CONSECUTIVE_REQUIRED: u64 = 2;
pub(crate) const ADAPTIVE_BATCH_SEVERE_COOLDOWN_STEPS: u64 = 2;
pub(crate) const ADAPTIVE_BATCH_TXPS_EMA_ALPHA_PCT: u64 = 20; // 0.20
pub(crate) const ADAPTIVE_BATCH_TXPS_STEPUP_MIN_RETAIN_PCT: u64 = 98;
pub(crate) const ADAPTIVE_BATCH_TXPS_BACKOFF_DROP_PCT: u64 = 95;
pub(crate) const ADAPTIVE_BATCH_PARSE_PRESSURE_PCT: f64 = 95.0;
pub(crate) const ADAPTIVE_BATCH_WRITER_PRESSURE_PCT: f64 = 90.0;
pub(crate) const ADAPTIVE_BATCH_PARSE_HEALTHY_PCT: f64 = 60.0;
pub(crate) const ADAPTIVE_BATCH_WRITER_HEALTHY_PCT: f64 = 60.0;
pub(crate) const ADAPTIVE_BATCH_MEMORY_PRESSURE_PCT: f64 = 80.0;
pub(crate) const ADAPTIVE_BATCH_MEMORY_HEALTHY_PCT: f64 = 70.0;
pub(crate) const ADAPTIVE_BATCH_MIN_FLOOR_STEP_DOWN_PCT: u64 = 80;
pub(crate) const ADAPTIVE_BATCH_MIN_FLOOR_STEP_UP_PCT: u64 = 110;
pub(crate) const ADAPTIVE_BATCH_MIN_FLOOR_RECOVER_WRITE_US_PER_TX: f64 = 220.0;
pub(crate) const ADAPTIVE_BATCH_HEALTHY_STEP_UP_PCT: u64 = 120;
pub(crate) const ADAPTIVE_BATCH_HEALTHY_BONUS_STEP_UP_PCT: u64 = 110;
pub(crate) const ADAPTIVE_BATCH_HEALTHY_BONUS_STREAK: u64 = 3;
pub(crate) const ADAPTIVE_BATCH_HEALTHY_BONUS_COMMIT_MS: f64 = 1_200.0;
pub(crate) const ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS: u64 = 1_000_000;
pub(crate) const ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS: u64 = 40_000;
pub(crate) const ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT: u64 = 6;
pub(crate) const ADAPTIVE_BATCH_BULK_SEVERE_MIN_TARGET_TXS: u64 = ADAPTIVE_BATCH_BASE_MIN_TXS;
pub(crate) const ADAPTIVE_BATCH_BULK_SEVERE_MIN_INFLIGHT: u64 = 2;

// ── Non-adaptive constants that live alongside the batch controller ──

pub(crate) const BULK_PHASE_COMMIT_SLOW_WARN_MS: f64 = 2_000.0;
pub(crate) const UDT_CELL_CACHE_CAPACITY: usize = 100_000;
pub(crate) const PARSER_UNRESOLVED_RETRY_DELAY_MS: u64 = 500;
pub(crate) const PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE: usize = 5;
pub(crate) const PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS: u64 = 8;
pub(crate) const ADAPTIVE_SUB_BATCH_INPUT_CAP_SCALE_NUM: u64 = 5;
pub(crate) const ADAPTIVE_SUB_BATCH_INPUT_CAP_SCALE_DEN: u64 = 4;

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
    pub(crate) batch_tx_count: usize,
    pub(crate) blocks_remaining: u64,
    pub(crate) parse_queue_fill_pct: Option<f64>,
    pub(crate) writer_queue_fill_pct: Option<f64>,
    pub(crate) memory_ratio_pct: Option<f64>,
    /// Max L0 file count across all CFs (from memory_stats)
    pub(crate) l0_files_max: Option<u64>,
    /// Pending compaction bytes (from memory_stats)
    pub(crate) compaction_pending_bytes: Option<u64>,
    /// Total immutable memtables across all CFs (from memory_stats)
    pub(crate) immutable_memtables: Option<u64>,
    /// Dynamic pressure thresholds from MemoryProfile
    pub(crate) severe_pending_threshold: u64,
    pub(crate) moderate_pending_threshold: u64,
    pub(crate) severe_imm_threshold: u64,
    pub(crate) moderate_imm_threshold: u64,
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
    min_target_batch_txs: AtomicU64,
    tx_per_block_milli_ema: AtomicU64,
    tx_per_sec_milli_ema: AtomicU64,
    cooldown_steps: AtomicU64,
    last_reason_code: AtomicU8,
    adjustment_seq: AtomicU64,
    backoff_streak: AtomicU64,
    severe_pressure_streak: AtomicU64,
    healthy_streak: AtomicU64,
    last_adjusted_at: AtomicI64,
    max_inflight_limit: u64,
    early_height_boost_applied: AtomicBool,
}

impl AdaptiveBatchController {
    pub(crate) fn new(max_inflight_limit: u64) -> Self {
        let max_inflight_limit = max_inflight_limit.max(1);
        let initial_inflight = ADAPTIVE_BATCH_INITIAL_INFLIGHT.min(max_inflight_limit);
        Self {
            target_batch_txs: AtomicU64::new(ADAPTIVE_BATCH_INITIAL_TXS),
            inflight_limit: AtomicU64::new(initial_inflight),
            min_target_batch_txs: AtomicU64::new(ADAPTIVE_BATCH_BASE_MIN_TXS),
            tx_per_block_milli_ema: AtomicU64::new(ADAPTIVE_BATCH_INITIAL_TPB_MILLI),
            tx_per_sec_milli_ema: AtomicU64::new(0),
            cooldown_steps: AtomicU64::new(0),
            last_reason_code: AtomicU8::new(ADAPTIVE_REASON_UNKNOWN),
            adjustment_seq: AtomicU64::new(0),
            backoff_streak: AtomicU64::new(0),
            severe_pressure_streak: AtomicU64::new(0),
            healthy_streak: AtomicU64::new(0),
            last_adjusted_at: AtomicI64::new(0),
            max_inflight_limit,
            early_height_boost_applied: AtomicBool::new(false),
        }
    }

    pub(crate) fn snapshot(&self) -> AdaptiveBatchSnapshot {
        let last_adjusted_at_raw = self.last_adjusted_at.load(Ordering::Relaxed);
        AdaptiveBatchSnapshot {
            target_batch_txs: self.target_batch_txs.load(Ordering::Relaxed),
            inflight_limit: self.inflight_limit.load(Ordering::Relaxed),
            min_target_batch_txs: self.min_target_batch_txs.load(Ordering::Relaxed),
            cooldown_steps: self.cooldown_steps.load(Ordering::Relaxed),
            last_reason_code: self.last_reason_code.load(Ordering::Relaxed),
            adjustment_seq: self.adjustment_seq.load(Ordering::Relaxed),
            backoff_streak: self.backoff_streak.load(Ordering::Relaxed),
            last_adjusted_at: (last_adjusted_at_raw > 0).then_some(last_adjusted_at_raw),
        }
    }

    pub(crate) fn record_adjustment(&self, reason_code: u8) {
        self.last_reason_code.store(reason_code, Ordering::Relaxed);
        self.adjustment_seq.fetch_add(1, Ordering::Relaxed);
        self.last_adjusted_at
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);

        if decode_adaptive_batch_reason(reason_code)
            .is_some_and(|reason| reason.contains("backoff"))
        {
            self.backoff_streak.fetch_add(1, Ordering::Relaxed);
        } else {
            self.backoff_streak.store(0, Ordering::Relaxed);
        }
    }

    pub(crate) fn maybe_apply_early_height_boost(&self, start_block: u64) -> Option<(u64, u64)> {
        if start_block >= ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF {
            return None;
        }
        if self
            .early_height_boost_applied
            .swap(true, Ordering::Relaxed)
        {
            return None;
        }

        let previous_target_batch_txs = self.target_batch_txs.load(Ordering::Relaxed);
        let boosted_target_batch_txs = previous_target_batch_txs
            .max(ADAPTIVE_BATCH_EARLY_TARGET_TXS)
            .clamp(
                self.min_target_batch_txs.load(Ordering::Relaxed),
                ADAPTIVE_BATCH_MAX_TXS,
            );
        self.target_batch_txs
            .store(boosted_target_batch_txs, Ordering::Relaxed);
        self.record_adjustment(ADAPTIVE_REASON_EARLY_HEIGHT_BOOST);

        Some((previous_target_batch_txs, boosted_target_batch_txs))
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

    pub(crate) fn observe_tx_throughput(
        &self,
        tx_count: usize,
        write_ms: f64,
    ) -> Option<(u64, u64)> {
        if tx_count == 0 || write_ms <= 0.0 {
            return None;
        }

        let write_us = (write_ms * 1000.0).round() as u64;
        if write_us == 0 {
            return None;
        }

        let sample = (((tx_count as u128) * 1_000_000u128).saturating_add(write_us as u128 - 1))
            / write_us as u128;
        let sample = sample.clamp(1, u64::MAX as u128) as u64;
        let alpha = ADAPTIVE_BATCH_TXPS_EMA_ALPHA_PCT.min(100);

        loop {
            let old = self.tx_per_sec_milli_ema.load(Ordering::Relaxed);
            let blended = if old == 0 {
                sample
            } else {
                ((old.saturating_mul(100 - alpha)).saturating_add(sample * alpha)) / 100
            };
            if self
                .tx_per_sec_milli_ema
                .compare_exchange(old, blended.max(1), Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some((old, blended.max(1)));
            }
        }
    }

    fn step_down_min_floor(min_floor: u64) -> u64 {
        let lowered = min_floor.saturating_mul(ADAPTIVE_BATCH_MIN_FLOOR_STEP_DOWN_PCT) / 100;
        lowered.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS)
    }

    fn step_up_min_floor(min_floor: u64) -> u64 {
        let raised = min_floor
            .saturating_mul(ADAPTIVE_BATCH_MIN_FLOOR_STEP_UP_PCT)
            .saturating_add(99)
            / 100;
        raised.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS)
    }

    pub(crate) fn update_after_write(
        &self,
        input: AdaptiveBatchInput,
    ) -> Option<AdaptiveBatchAdjustment> {
        let previous_target_batch_txs = self.target_batch_txs.load(Ordering::Relaxed);
        let previous_inflight_limit = self.inflight_limit.load(Ordering::Relaxed);
        let previous_min_target_batch_txs = self
            .min_target_batch_txs
            .load(Ordering::Relaxed)
            .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS);
        let mut new_target_batch_txs = previous_target_batch_txs;
        let mut new_inflight_limit = previous_inflight_limit;
        let mut new_min_target_batch_txs = previous_min_target_batch_txs;
        let reason: Option<&'static str>;
        let near_tip = input.blocks_remaining <= ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS;
        let write_us_per_tx = if input.batch_tx_count > 0 && input.write_ms > 0.0 {
            Some((input.write_ms * 1000.0) / input.batch_tx_count as f64)
        } else {
            None
        };
        let txps_ema = self.observe_tx_throughput(input.batch_tx_count, input.write_ms);
        let throughput_not_worse = txps_ema.is_none_or(|(old, new)| {
            old == 0
                || (new.saturating_mul(100))
                    >= old.saturating_mul(ADAPTIVE_BATCH_TXPS_STEPUP_MIN_RETAIN_PCT)
        });
        let throughput_drop_under_load = txps_ema.is_some_and(|(old, new)| {
            old > 0
                && (new.saturating_mul(100))
                    < old.saturating_mul(ADAPTIVE_BATCH_TXPS_BACKOFF_DROP_PCT)
                && (input.writer_queue_fill_pct.is_some_and(|pct| pct >= 60.0)
                    || input.parse_queue_fill_pct.is_some_and(|pct| pct >= 60.0))
        });
        let high_unit_write_cost =
            write_us_per_tx.is_some_and(|us| us >= ADAPTIVE_BATCH_WRITE_HI_US_PER_TX);
        let target_unit_write_cost =
            write_us_per_tx.is_some_and(|us| us >= ADAPTIVE_BATCH_WRITE_TARGET_US_PER_TX);
        let queue_pressure = input
            .parse_queue_fill_pct
            .is_some_and(|pct| pct >= ADAPTIVE_BATCH_PARSE_PRESSURE_PCT)
            || input
                .writer_queue_fill_pct
                .is_some_and(|pct| pct >= ADAPTIVE_BATCH_WRITER_PRESSURE_PCT);

        // RocksDB internal pressure signals: detect compaction backlog, L0 pile-up,
        // and immutable memtable accumulation BEFORE they cause write stalls.
        // L0 thresholds (40/20) are architectural; pending bytes and immutable memtable
        // thresholds scale with the memory profile.
        let rocksdb_severe_pressure = input.l0_files_max.is_some_and(|l0| l0 >= 40)
            || input
                .compaction_pending_bytes
                .is_some_and(|b| b >= input.severe_pending_threshold)
            || input
                .immutable_memtables
                .is_some_and(|imm| imm >= input.severe_imm_threshold);
        let rocksdb_moderate_pressure = input.l0_files_max.is_some_and(|l0| l0 >= 20)
            || input
                .compaction_pending_bytes
                .is_some_and(|b| b >= input.moderate_pending_threshold)
            || input
                .immutable_memtables
                .is_some_and(|imm| imm >= input.moderate_imm_threshold);

        let severe_pressure_signal = input.write_ms >= ADAPTIVE_BATCH_SEVERE_WRITE_MS
            || input.commit_ms >= ADAPTIVE_BATCH_SEVERE_COMMIT_MS
            || write_us_per_tx.is_some_and(|us| us >= ADAPTIVE_BATCH_SEVERE_WRITE_US_PER_TX)
            || rocksdb_severe_pressure;
        let severe_pressure_streak = if severe_pressure_signal {
            self.severe_pressure_streak.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.severe_pressure_streak.store(0, Ordering::Relaxed);
            0
        };
        let severe_pressure = severe_pressure_streak >= ADAPTIVE_BATCH_SEVERE_CONSECUTIVE_REQUIRED;
        let moderate_pressure = target_unit_write_cost
            || (input.write_ms > ADAPTIVE_BATCH_WRITE_HI_MS && throughput_drop_under_load)
            || (queue_pressure && throughput_drop_under_load)
            || input
                .memory_ratio_pct
                .is_some_and(|pct| pct >= ADAPTIVE_BATCH_MEMORY_PRESSURE_PCT)
            || rocksdb_moderate_pressure;

        if severe_pressure {
            new_target_batch_txs = ((previous_target_batch_txs as f64) * 0.7).round() as u64;
            new_inflight_limit = previous_inflight_limit.saturating_sub(1).max(1);
            self.cooldown_steps
                .store(ADAPTIVE_BATCH_SEVERE_COOLDOWN_STEPS, Ordering::Relaxed);
            self.healthy_streak.store(0, Ordering::Relaxed);

            let at_floor = previous_target_batch_txs <= previous_min_target_batch_txs;
            if at_floor && previous_inflight_limit <= 2 && high_unit_write_cost {
                new_min_target_batch_txs = Self::step_down_min_floor(previous_min_target_batch_txs);
                reason = Some(
                    if new_min_target_batch_txs < previous_min_target_batch_txs {
                        "pressure_backoff_floor_down"
                    } else {
                        "severe_pressure_backoff"
                    },
                );
            } else {
                reason = Some("severe_pressure_backoff");
            }
        } else if moderate_pressure {
            new_target_batch_txs = ((previous_target_batch_txs as f64) * 0.9).round() as u64;
            self.healthy_streak.store(0, Ordering::Relaxed);
            reason = Some("moderate_backoff");
        } else {
            let cooldown = self.cooldown_steps.load(Ordering::Relaxed);
            if cooldown > 0 {
                self.cooldown_steps.fetch_sub(1, Ordering::Relaxed);
                self.healthy_streak.store(0, Ordering::Relaxed);
                reason = None;
            } else {
                let healthy = input.write_ms < ADAPTIVE_BATCH_WRITE_LO_MS
                    && write_us_per_tx
                        .is_some_and(|us| us < ADAPTIVE_BATCH_WRITE_HEALTHY_US_PER_TX)
                    && input
                        .parse_queue_fill_pct
                        .is_some_and(|pct| pct < ADAPTIVE_BATCH_PARSE_HEALTHY_PCT)
                    && input
                        .writer_queue_fill_pct
                        .is_some_and(|pct| pct < ADAPTIVE_BATCH_WRITER_HEALTHY_PCT)
                    && input
                        .memory_ratio_pct
                        .is_none_or(|pct| pct < ADAPTIVE_BATCH_MEMORY_HEALTHY_PCT)
                    && !rocksdb_moderate_pressure;
                if healthy && throughput_not_worse {
                    let healthy_streak = self.healthy_streak.fetch_add(1, Ordering::Relaxed) + 1;
                    if previous_inflight_limit < self.max_inflight_limit {
                        new_inflight_limit = previous_inflight_limit + 1;
                    } else {
                        let mut growth_pct = ADAPTIVE_BATCH_HEALTHY_STEP_UP_PCT;
                        if healthy_streak >= ADAPTIVE_BATCH_HEALTHY_BONUS_STREAK
                            && input.write_ms < ADAPTIVE_BATCH_WRITE_TARGET_MS
                            && input.commit_ms < ADAPTIVE_BATCH_HEALTHY_BONUS_COMMIT_MS
                        {
                            growth_pct = growth_pct
                                .saturating_mul(ADAPTIVE_BATCH_HEALTHY_BONUS_STEP_UP_PCT)
                                / 100;
                        }
                        new_target_batch_txs = previous_target_batch_txs
                            .saturating_mul(growth_pct)
                            .saturating_add(99)
                            / 100;
                    }
                    let should_recover_floor = previous_min_target_batch_txs
                        < ADAPTIVE_BATCH_BASE_MIN_TXS
                        && write_us_per_tx.is_some_and(|us| {
                            us <= ADAPTIVE_BATCH_MIN_FLOOR_RECOVER_WRITE_US_PER_TX
                        })
                        && input.parse_queue_fill_pct.is_some_and(|pct| pct < 30.0)
                        && input.writer_queue_fill_pct.is_some_and(|pct| pct < 30.0)
                        && previous_target_batch_txs > previous_min_target_batch_txs;
                    if should_recover_floor {
                        new_min_target_batch_txs =
                            Self::step_up_min_floor(previous_min_target_batch_txs);
                        reason = Some(
                            if new_min_target_batch_txs > previous_min_target_batch_txs {
                                "healthy_step_up_floor_recover"
                            } else {
                                "healthy_step_up"
                            },
                        );
                    } else {
                        reason = Some("healthy_step_up");
                    }
                } else {
                    self.healthy_streak.store(0, Ordering::Relaxed);
                    reason = None;
                }
            }
        }

        if near_tip {
            new_min_target_batch_txs = new_min_target_batch_txs
                .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS);
        } else if severe_pressure {
            // When RocksDB is under sustained severe pressure in far-bulk mode,
            // relax the usual bulk floors so controller can keep backing off.
            let min_inflight = ADAPTIVE_BATCH_BULK_SEVERE_MIN_INFLIGHT.min(self.max_inflight_limit);
            new_inflight_limit = new_inflight_limit.max(min_inflight);
            new_min_target_batch_txs = new_min_target_batch_txs
                .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_MAX_TXS)
                .min(ADAPTIVE_BATCH_BULK_SEVERE_MIN_TARGET_TXS);
        } else {
            let min_inflight =
                ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT.min(self.max_inflight_limit);
            new_inflight_limit = new_inflight_limit.max(min_inflight);
            new_min_target_batch_txs = new_min_target_batch_txs
                .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_MAX_TXS)
                .max(ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS);
        }
        new_target_batch_txs =
            new_target_batch_txs.clamp(new_min_target_batch_txs, ADAPTIVE_BATCH_MAX_TXS);
        new_inflight_limit = new_inflight_limit.clamp(1, self.max_inflight_limit);

        if new_target_batch_txs == previous_target_batch_txs
            && new_inflight_limit == previous_inflight_limit
            && new_min_target_batch_txs == previous_min_target_batch_txs
        {
            return None;
        }

        self.target_batch_txs
            .store(new_target_batch_txs, Ordering::Relaxed);
        self.inflight_limit
            .store(new_inflight_limit, Ordering::Relaxed);
        self.min_target_batch_txs
            .store(new_min_target_batch_txs, Ordering::Relaxed);
        self.record_adjustment(encode_adaptive_batch_reason(reason.unwrap_or("adjusted")));

        Some(AdaptiveBatchAdjustment {
            previous_target_batch_txs,
            new_target_batch_txs,
            previous_inflight_limit,
            new_inflight_limit,
            previous_min_target_batch_txs,
            new_min_target_batch_txs,
            reason: reason.unwrap_or("adjusted"),
        })
    }
}

// ── Free functions ───────────────────────────────────────────────────

/// Build a fetch sub-batch plan based on per-block tx/input counts.
/// Returns `(block_count, tx_count, input_count)` tuples for each sub-batch.
pub(crate) fn plan_fetch_sub_batches(
    tx_counts: &[usize],
    input_counts: &[usize],
    tx_cap: usize,
    input_cap: usize,
) -> Vec<(usize, usize, usize)> {
    assert!(
        tx_counts.len() == input_counts.len(),
        "tx_counts and input_counts must have the same length"
    );
    assert!(
        tx_cap > 0 && input_cap > 0,
        "tx_cap and input_cap must be > 0 to avoid infinite sub-batch splitting"
    );

    if tx_counts.is_empty() {
        return Vec::new();
    }

    let mut plan = Vec::new();
    let mut sub_blocks = 0usize;
    let mut sub_txs = 0usize;
    let mut sub_inputs = 0usize;

    for (&txs, &inputs) in tx_counts.iter().zip(input_counts.iter()) {
        sub_blocks += 1;
        sub_txs = sub_txs
            .checked_add(txs)
            .expect("sub-batch tx total overflow while planning fetch splits");
        sub_inputs = sub_inputs
            .checked_add(inputs)
            .expect("sub-batch input total overflow while planning fetch splits");

        if sub_txs >= tx_cap || sub_inputs >= input_cap {
            plan.push((sub_blocks, sub_txs, sub_inputs));
            sub_blocks = 0;
            sub_txs = 0;
            sub_inputs = 0;
        }
    }

    if sub_blocks > 0 {
        plan.push((sub_blocks, sub_txs, sub_inputs));
    }

    plan
}

pub(crate) fn adaptive_sub_batch_tx_cap(target_batch_txs: u64, min_target_batch_txs: u64) -> usize {
    let min_target_batch_txs =
        min_target_batch_txs.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_MAX_TXS);
    target_batch_txs
        .saturating_mul(2)
        .clamp(min_target_batch_txs, ADAPTIVE_BATCH_MAX_TXS) as usize
}

pub(crate) fn adaptive_sub_batch_input_cap(
    target_batch_txs: u64,
    min_target_batch_txs: u64,
) -> usize {
    let tx_cap = adaptive_sub_batch_tx_cap(target_batch_txs, min_target_batch_txs) as u64;
    let scaled = tx_cap
        .saturating_mul(ADAPTIVE_SUB_BATCH_INPUT_CAP_SCALE_NUM)
        .saturating_add(ADAPTIVE_SUB_BATCH_INPUT_CAP_SCALE_DEN - 1)
        / ADAPTIVE_SUB_BATCH_INPUT_CAP_SCALE_DEN;
    scaled
        .clamp(min_target_batch_txs, ADAPTIVE_BATCH_MAX_TXS)
        .try_into()
        .expect("adaptive input cap must fit usize")
}

pub(super) fn bump_pipeline_reset_epoch(epoch: &AtomicU64) -> u64 {
    epoch.fetch_add(1, Ordering::SeqCst) + 1
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_adaptive_batch_estimate_block_span_clamps_to_bounds() {
        let controller = AdaptiveBatchController::new(16);
        controller
            .target_batch_txs
            .store(100_000, Ordering::Relaxed);
        controller
            .tx_per_block_milli_ema
            .store(2_000_000, Ordering::Relaxed); // 2000 tx/block
                                                  // Estimated span = 50 blocks.
        assert_eq!(controller.estimate_block_span(10_000), 50);

        controller
            .tx_per_block_milli_ema
            .store(1_000, Ordering::Relaxed); // 1 tx/block
                                              // Estimated span = 100_000, but cap by batch_block_cap.
        assert_eq!(controller.estimate_block_span(500), 500);
    }

    #[test]
    fn test_adaptive_batch_moderate_backoff_reduces_target_only() {
        let controller = AdaptiveBatchController::new(8);
        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_HI_MS + 1.0,
                commit_ms: 0.0,
                batch_tx_count: 8_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("moderate pressure should reduce target");
        assert_eq!(adjustment.reason, "moderate_backoff");
        assert_eq!(adjustment.new_target_batch_txs, 36_000);
        assert_eq!(
            adjustment.new_inflight_limit,
            ADAPTIVE_BATCH_INITIAL_INFLIGHT
        );
    }

    #[test]
    fn test_adaptive_batch_healthy_step_up_prioritizes_inflight_recovery() {
        let controller = AdaptiveBatchController::new(8);
        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_LO_MS - 100.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("healthy signal should adjust inflight first");
        assert_eq!(adjustment.reason, "healthy_step_up");
        assert_eq!(adjustment.new_target_batch_txs, ADAPTIVE_BATCH_INITIAL_TXS);
        assert_eq!(
            adjustment.new_inflight_limit,
            ADAPTIVE_BATCH_INITIAL_INFLIGHT + 1
        );
    }

    #[test]
    fn test_adaptive_batch_bulk_distance_floor_enforced() {
        let controller = AdaptiveBatchController::new(8);
        controller.target_batch_txs.store(20_000, Ordering::Relaxed);
        controller
            .min_target_batch_txs
            .store(ADAPTIVE_BATCH_HARD_MIN_TXS, Ordering::Relaxed);
        controller.inflight_limit.store(2, Ordering::Relaxed);

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_LO_MS - 100.0,
                commit_ms: 0.0,
                batch_tx_count: 5_000,
                blocks_remaining: ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS + 1,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("far bulk mode should enforce minimum floors");
        assert_eq!(
            adjustment.new_min_target_batch_txs,
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS
        );
        assert!(adjustment.new_target_batch_txs >= ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS);
        assert_eq!(
            adjustment.new_inflight_limit,
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT
        );
    }

    #[test]
    fn test_adaptive_batch_far_bulk_severe_pressure_can_relax_bulk_floors() {
        let controller = AdaptiveBatchController::new(8);
        controller.target_batch_txs.store(
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            Ordering::Relaxed,
        );
        controller.min_target_batch_txs.store(
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            Ordering::Relaxed,
        );
        controller
            .inflight_limit
            .store(ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT, Ordering::Relaxed);

        let first = controller.update_after_write(AdaptiveBatchInput {
            write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 100.0,
            commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
            batch_tx_count: 8_000,
            blocks_remaining: ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS + 10_000,
            parse_queue_fill_pct: Some(98.0),
            writer_queue_fill_pct: Some(98.0),
            memory_ratio_pct: Some(85.0),
            l0_files_max: Some(120),
            compaction_pending_bytes: Some(6 * 1024 * 1024 * 1024),
            immutable_memtables: Some(40),
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        if let Some(first_adjustment) = first {
            assert_eq!(
                first_adjustment.new_inflight_limit,
                ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT
            );
            assert_eq!(
                first_adjustment.new_min_target_batch_txs,
                ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS
            );
            assert_eq!(
                first_adjustment.new_target_batch_txs,
                ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS
            );
        }

        let second = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 200.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 200.0,
                batch_tx_count: 8_000,
                blocks_remaining: ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS + 10_000,
                parse_queue_fill_pct: Some(98.0),
                writer_queue_fill_pct: Some(98.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: Some(130),
                compaction_pending_bytes: Some(7 * 1024 * 1024 * 1024),
                immutable_memtables: Some(45),
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("sustained severe pressure should relax far-bulk floors");
        assert_eq!(second.reason, "severe_pressure_backoff");
        assert!(
            second.new_inflight_limit < ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT,
            "far-bulk inflight floor should relax under sustained severe pressure"
        );
        assert!(
            second.new_min_target_batch_txs < ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            "far-bulk min target floor should relax under sustained severe pressure"
        );
        assert!(
            second.new_target_batch_txs < ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            "target batch txs should be allowed below far-bulk floor during severe pressure"
        );
    }

    #[test]
    fn test_adaptive_batch_floor_down_when_pressure_at_floor_and_single_inflight() {
        let controller = AdaptiveBatchController::new(1);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_BASE_MIN_TXS, Ordering::Relaxed);

        let first = controller.update_after_write(AdaptiveBatchInput {
            write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 100.0,
            commit_ms: 0.0,
            batch_tx_count: 8_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(97.0),
            writer_queue_fill_pct: Some(95.0),
            memory_ratio_pct: Some(85.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            first.is_none(),
            "first severe sample should not floor-down yet"
        );

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 100.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
                batch_tx_count: 8_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(97.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("consecutive severe pressure at floor should lower adaptive min floor");

        assert_eq!(adjustment.reason, "pressure_backoff_floor_down");
        assert!(
            adjustment.new_min_target_batch_txs < adjustment.previous_min_target_batch_txs,
            "adaptive min floor should go down under sustained pressure"
        );
    }

    #[test]
    fn test_adaptive_batch_floor_recovers_on_healthy_throughput() {
        let controller = AdaptiveBatchController::new(8);
        controller
            .min_target_batch_txs
            .store(ADAPTIVE_BATCH_HARD_MIN_TXS, Ordering::Relaxed);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_HARD_MIN_TXS + 2_000, Ordering::Relaxed);

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("healthy throughput should recover adaptive min floor");

        assert_eq!(adjustment.reason, "healthy_step_up_floor_recover");
        assert!(
            adjustment.new_min_target_batch_txs > adjustment.previous_min_target_batch_txs,
            "adaptive min floor should recover upward"
        );
    }

    #[test]
    fn test_adaptive_batch_severe_pressure_requires_consecutive_batches_before_backoff() {
        let controller = AdaptiveBatchController::new(8);

        let first = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(98.0),
                writer_queue_fill_pct: Some(98.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first severe sample should only moderate-backoff");
        assert_eq!(first.reason, "moderate_backoff");

        let second = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 2_000.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(98.0),
                writer_queue_fill_pct: Some(98.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("second severe sample should trigger severe backoff");
        assert_eq!(second.reason, "severe_pressure_backoff");
        assert!(second.new_target_batch_txs < second.previous_target_batch_txs);
        assert!(second.new_inflight_limit < second.previous_inflight_limit);
    }

    #[test]
    fn test_adaptive_batch_high_queue_without_throughput_drop_does_not_backoff() {
        let controller = AdaptiveBatchController::new(8);

        let first = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first healthy sample should step up");
        assert_eq!(first.reason, "healthy_step_up");

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: 150.0,
            commit_ms: 0.0,
            batch_tx_count: 6_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(99.0),
            writer_queue_fill_pct: Some(99.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            no_adjustment.is_none(),
            "queue fullness alone should not force backoff when tx throughput improves"
        );
    }

    #[test]
    fn test_adaptive_batch_floor_down_requires_real_pressure_signal() {
        let controller = AdaptiveBatchController::new(1);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_BASE_MIN_TXS, Ordering::Relaxed);

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: 200.0,
            commit_ms: 0.0,
            batch_tx_count: 10_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(97.0),
            writer_queue_fill_pct: Some(95.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            no_adjustment.is_none(),
            "at-floor min target should not be lowered by queue pressure alone"
        );
        assert_eq!(
            controller.snapshot().min_target_batch_txs,
            ADAPTIVE_BATCH_BASE_MIN_TXS
        );
    }

    #[test]
    fn test_adaptive_batch_near_tip_can_drop_min_floor_below_bulk_floor() {
        let controller = AdaptiveBatchController::new(8);
        controller.min_target_batch_txs.store(
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            Ordering::Relaxed,
        );
        controller.target_batch_txs.store(
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            Ordering::Relaxed,
        );

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_HI_MS + 50.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 10_000,
                parse_queue_fill_pct: Some(95.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("near-tip path should allow lower min floor");

        assert_eq!(adjustment.reason, "moderate_backoff");
        assert_eq!(
            adjustment.new_min_target_batch_txs,
            ADAPTIVE_BATCH_BASE_MIN_TXS
        );
    }

    #[test]
    fn test_adaptive_batch_step_up_requires_throughput_not_worse() {
        let controller = AdaptiveBatchController::new(8);

        let first = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first healthy sample should step up");
        assert_eq!(first.reason, "healthy_step_up");

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: 1_500.0,
            commit_ms: 0.0,
            batch_tx_count: 10_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(10.0),
            writer_queue_fill_pct: Some(10.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            no_adjustment.is_none(),
            "step-up should pause when throughput degrades despite healthy queues"
        );
    }

    #[test]
    fn test_adaptive_batch_cooldown_blocks_immediate_step_up_after_pressure() {
        let controller = AdaptiveBatchController::new(8);
        let _ = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(95.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first pressure sample should adjust");
        let _ = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 2_000.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(95.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("second pressure sample should trigger cooldown");
        let snapshot_after_pressure = controller.snapshot();

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: ADAPTIVE_BATCH_WRITE_LO_MS - 100.0,
            commit_ms: 0.0,
            batch_tx_count: 10_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(10.0),
            writer_queue_fill_pct: Some(10.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(no_adjustment.is_none());
        let snapshot_after_healthy = controller.snapshot();
        assert_eq!(
            snapshot_after_healthy.target_batch_txs,
            snapshot_after_pressure.target_batch_txs
        );
        assert_eq!(
            snapshot_after_healthy.inflight_limit,
            snapshot_after_pressure.inflight_limit
        );
    }

    #[test]
    fn test_adaptive_batch_early_height_boost_applies_once() {
        let controller = AdaptiveBatchController::new(8);
        let first = controller
            .maybe_apply_early_height_boost(123)
            .expect("early-chain boost should apply once");
        assert_eq!(first.0, ADAPTIVE_BATCH_INITIAL_TXS);
        assert_eq!(first.1, ADAPTIVE_BATCH_EARLY_TARGET_TXS);
        assert_eq!(
            controller.snapshot().target_batch_txs,
            ADAPTIVE_BATCH_EARLY_TARGET_TXS
        );

        let second = controller.maybe_apply_early_height_boost(456);
        assert!(second.is_none(), "boost should not reapply");
    }

    #[test]
    fn test_adaptive_batch_early_height_boost_skips_after_cutoff() {
        let controller = AdaptiveBatchController::new(8);
        let skipped = controller.maybe_apply_early_height_boost(ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF);
        assert!(skipped.is_none());
        assert_eq!(
            controller.snapshot().target_batch_txs,
            ADAPTIVE_BATCH_INITIAL_TXS
        );
    }

    #[test]
    fn test_plan_fetch_sub_batches_without_split() {
        let plan = plan_fetch_sub_batches(&[10, 20, 30], &[11, 22, 33], 1000, 1000);
        assert_eq!(plan, vec![(3, 60, 66)]);
    }

    #[test]
    fn test_plan_fetch_sub_batches_with_tx_split() {
        let plan = plan_fetch_sub_batches(&[2, 2, 1, 5], &[1, 1, 1, 1], 3, 10);
        assert_eq!(plan, vec![(2, 4, 2), (2, 6, 2)]);
    }

    #[test]
    fn test_plan_fetch_sub_batches_with_input_split() {
        let plan = plan_fetch_sub_batches(&[2, 2, 1, 5], &[3, 3, 1, 1], 100, 5);
        assert_eq!(plan, vec![(2, 4, 6), (2, 6, 2)]);
    }

    #[test]
    fn test_plan_fetch_sub_batches_empty() {
        let plan = plan_fetch_sub_batches(&[], &[], 100, 100);
        assert!(plan.is_empty());
    }

    #[test]
    fn test_adaptive_sub_batch_tx_cap_scales_with_target() {
        assert_eq!(
            adaptive_sub_batch_tx_cap(10_000, ADAPTIVE_BATCH_BASE_MIN_TXS),
            20_000
        );
        assert_eq!(
            adaptive_sub_batch_tx_cap(40_000, ADAPTIVE_BATCH_BASE_MIN_TXS),
            80_000
        );
    }

    #[test]
    fn test_adaptive_sub_batch_tx_cap_respects_adaptive_ceiling() {
        assert_eq!(
            adaptive_sub_batch_tx_cap(ADAPTIVE_BATCH_MAX_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS),
            ADAPTIVE_BATCH_MAX_TXS as usize
        );
        assert_eq!(
            adaptive_sub_batch_tx_cap(ADAPTIVE_BATCH_MAX_TXS * 2, ADAPTIVE_BATCH_BASE_MIN_TXS),
            ADAPTIVE_BATCH_MAX_TXS as usize
        );
    }

    #[test]
    fn test_adaptive_sub_batch_tx_cap_respects_adaptive_floor() {
        assert_eq!(
            adaptive_sub_batch_tx_cap(2_500, 8_000),
            8_000,
            "sub-batch cap should never drop below adaptive min floor"
        );
        assert_eq!(
            adaptive_sub_batch_tx_cap(500, 500),
            ADAPTIVE_BATCH_HARD_MIN_TXS as usize,
            "adaptive min floor should still respect hard safety minimum"
        );
    }

    #[test]
    fn test_adaptive_sub_batch_input_cap_scales_from_tx_cap() {
        assert_eq!(
            adaptive_sub_batch_input_cap(10_000, ADAPTIVE_BATCH_BASE_MIN_TXS),
            25_000
        );
        assert_eq!(
            adaptive_sub_batch_input_cap(40_000, ADAPTIVE_BATCH_BASE_MIN_TXS),
            100_000
        );
    }

    #[test]
    fn test_adaptive_sub_batch_input_cap_respects_ceiling() {
        assert_eq!(
            adaptive_sub_batch_input_cap(ADAPTIVE_BATCH_MAX_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS),
            ADAPTIVE_BATCH_MAX_TXS as usize
        );
        assert_eq!(
            adaptive_sub_batch_input_cap(ADAPTIVE_BATCH_MAX_TXS * 2, ADAPTIVE_BATCH_BASE_MIN_TXS),
            ADAPTIVE_BATCH_MAX_TXS as usize
        );
    }

    #[test]
    #[should_panic(expected = "tx_counts and input_counts must have the same length")]
    fn test_plan_fetch_sub_batches_panics_on_mismatched_block_vectors() {
        let _ = plan_fetch_sub_batches(&[1], &[1, 2], 100, 100);
    }

    #[test]
    #[should_panic(expected = "tx_cap and input_cap must be > 0")]
    fn test_plan_fetch_sub_batches_panics_on_zero_limit() {
        let _ = plan_fetch_sub_batches(&[1], &[1], 0, 100);
    }

    #[test]
    fn test_bump_pipeline_reset_epoch_is_monotonic() {
        let epoch = AtomicU64::new(0);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 1);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 2);
        assert_eq!(epoch.load(Ordering::SeqCst), 2);
    }
}
