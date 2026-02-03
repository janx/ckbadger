use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Instant;

pub use ckbadger_common::format_duration_smart;

const WINDOW_SECS: u64 = 10;
/// EMA smoothing factor: 0.1 = slow adaptation, 0.3 = faster adaptation
const EMA_ALPHA: f64 = 0.1;

/// Number of speed samples to keep for trend analysis
const SPEED_HISTORY_SIZE: usize = 30; // 30 windows × 10 secs = 5 minutes of history

/// A speed sample recording block position and rate at that point
#[derive(Debug, Clone, Copy)]
struct SpeedSample {
    block_number: u64,
    blocks_per_sec: f64,
}

pub struct SyncProgress {
    current_block: AtomicU64,
    target_block: AtomicU64,
    blocks_processed: AtomicU64,
    start_time: Instant,
    // Sliding window for real-time blocks/sec
    window_start_millis: AtomicU64,
    window_blocks: AtomicU64,
    // Cache last computed rate to avoid returning 0 during window reset
    last_rate: AtomicU64, // stored as bits of f64
    // EMA (Exponential Moving Average) for smoother speed estimation
    ema_rate: AtomicU64, // stored as bits of f64
    // Speed history for trend analysis (block_number, speed)
    speed_history: RwLock<VecDeque<SpeedSample>>,
}

impl SyncProgress {
    pub fn new(start_block: u64, target_block: u64) -> Self {
        let now_millis = Instant::now().elapsed().as_millis() as u64;
        Self {
            current_block: AtomicU64::new(start_block),
            target_block: AtomicU64::new(target_block),
            blocks_processed: AtomicU64::new(0),
            start_time: Instant::now(),
            window_start_millis: AtomicU64::new(now_millis),
            window_blocks: AtomicU64::new(0),
            last_rate: AtomicU64::new(0),
            ema_rate: AtomicU64::new(0),
            speed_history: RwLock::new(VecDeque::with_capacity(SPEED_HISTORY_SIZE)),
        }
    }

    pub fn update_current(&self, block: u64) {
        self.current_block.store(block, Ordering::SeqCst);
        self.blocks_processed.fetch_add(1, Ordering::SeqCst);
        self.add_to_window(1);
    }

    pub fn update_current_batch(&self, block: u64, count: u64) {
        self.current_block.store(block, Ordering::SeqCst);
        self.blocks_processed.fetch_add(count, Ordering::SeqCst);
        self.add_to_window(count);
    }

    fn elapsed_millis(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    fn add_to_window(&self, count: u64) {
        let now = self.elapsed_millis();
        let window_start = self.window_start_millis.load(Ordering::SeqCst);
        let window_ms = WINDOW_SECS * 1000;

        if now.saturating_sub(window_start) >= window_ms {
            let window_blocks = self.window_blocks.load(Ordering::SeqCst);
            let elapsed_secs = now.saturating_sub(window_start) as f64 / 1000.0;
            if elapsed_secs > 0.0 {
                let rate = window_blocks as f64 / elapsed_secs;
                self.last_rate.store(rate.to_bits(), Ordering::SeqCst);
                self.update_ema(rate);
            }
            self.window_start_millis.store(now, Ordering::SeqCst);
            self.window_blocks.store(count, Ordering::SeqCst);
        } else {
            self.window_blocks.fetch_add(count, Ordering::SeqCst);
        }
    }

    fn update_ema(&self, current_rate: f64) {
        let old_ema = f64::from_bits(self.ema_rate.load(Ordering::SeqCst));
        let new_ema = if old_ema == 0.0 {
            current_rate
        } else {
            EMA_ALPHA * current_rate + (1.0 - EMA_ALPHA) * old_ema
        };
        self.ema_rate.store(new_ema.to_bits(), Ordering::SeqCst);

        let block_number = self.current_block.load(Ordering::SeqCst);
        self.record_speed_sample(block_number, new_ema);
    }

    fn record_speed_sample(&self, block_number: u64, blocks_per_sec: f64) {
        if let Ok(mut history) = self.speed_history.write() {
            if history.len() >= SPEED_HISTORY_SIZE {
                history.pop_front();
            }
            history.push_back(SpeedSample {
                block_number,
                blocks_per_sec,
            });
        }
    }

    pub fn update_target(&self, target: u64) {
        self.target_block.store(target, Ordering::SeqCst);
    }

    pub fn current(&self) -> u64 {
        self.current_block.load(Ordering::SeqCst)
    }

    pub fn target(&self) -> u64 {
        self.target_block.load(Ordering::SeqCst)
    }

    pub fn blocks_remaining(&self) -> u64 {
        let target = self.target();
        let current = self.current();
        target.saturating_sub(current)
    }

    pub fn blocks_per_second(&self) -> f64 {
        let now = self.elapsed_millis();
        let window_start = self.window_start_millis.load(Ordering::SeqCst);
        let elapsed_ms = now.saturating_sub(window_start);

        if elapsed_ms < 1000 {
            return f64::from_bits(self.last_rate.load(Ordering::SeqCst));
        }

        let window_blocks = self.window_blocks.load(Ordering::SeqCst);
        let elapsed_secs = elapsed_ms as f64 / 1000.0;
        window_blocks as f64 / elapsed_secs
    }

    pub fn ema_blocks_per_second(&self) -> f64 {
        f64::from_bits(self.ema_rate.load(Ordering::SeqCst))
    }

    pub fn is_synced(&self) -> bool {
        self.current() >= self.target()
    }

    pub fn progress_percentage(&self) -> f64 {
        let target = self.target() as f64;
        let current = self.current() as f64;
        if target > 0.0 {
            (current / target) * 100.0
        } else {
            100.0
        }
    }

    /// Calculate speed trend using linear regression on speed history.
    /// Returns (slope, intercept) where:
    /// - slope: speed change per block (negative = slowing down)
    /// - intercept: extrapolated speed at block 0
    ///
    /// Returns None if insufficient data (need at least 3 samples).
    fn calculate_speed_trend(&self) -> Option<(f64, f64)> {
        let history = self.speed_history.read().ok()?;
        if history.len() < 3 {
            return None;
        }

        // Linear regression: speed = slope * block_number + intercept
        // Using ordinary least squares
        let n = history.len() as f64;
        let mut sum_x = 0.0; // block numbers
        let mut sum_y = 0.0; // speeds
        let mut sum_xy = 0.0;
        let mut sum_xx = 0.0;

        for sample in history.iter() {
            let x = sample.block_number as f64;
            let y = sample.blocks_per_sec;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_xx += x * x;
        }

        let denominator = n * sum_xx - sum_x * sum_x;
        if denominator.abs() < 1e-10 {
            return None;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / denominator;
        let intercept = (sum_y - slope * sum_x) / n;

        Some((slope, intercept))
    }

    /// Predict speed at a future block number based on trend.
    /// Clamps to a minimum of 10% of current EMA to avoid unrealistic predictions.
    fn predict_speed_at(&self, block_number: u64) -> f64 {
        let current_ema = self.ema_blocks_per_second();
        if current_ema <= 0.0 {
            return 0.0;
        }

        match self.calculate_speed_trend() {
            Some((slope, intercept)) => {
                let predicted = slope * block_number as f64 + intercept;
                // Clamp: at least 10% of current EMA, at most 200% of current EMA
                predicted.clamp(current_ema * 0.1, current_ema * 2.0)
            }
            None => current_ema,
        }
    }

    /// Calculate ETA using trend-based prediction.
    /// Divides remaining blocks into segments and predicts speed for each.
    /// Falls back to simple EMA-based calculation if trend data is insufficient.
    pub fn eta_seconds(&self) -> Option<f64> {
        let remaining = self.blocks_remaining();
        if remaining == 0 {
            return None;
        }

        let ema_rate = self.ema_blocks_per_second();
        if ema_rate <= 0.0 {
            return None;
        }

        // If no trend data, fall back to simple calculation
        if self.calculate_speed_trend().is_none() {
            return Some(remaining as f64 / ema_rate);
        }

        let current = self.current();
        let target = self.target();

        let segment_size = (remaining / 10).clamp(1, 100_000);
        let mut total_time = 0.0;
        let mut block = current;

        while block < target {
            let segment_end = (block + segment_size).min(target);
            let segment_blocks = segment_end - block;

            let midpoint = block + segment_blocks / 2;
            let predicted_speed = self.predict_speed_at(midpoint);

            if predicted_speed > 0.0 {
                total_time += segment_blocks as f64 / predicted_speed;
            }

            block = segment_end;
        }

        Some(total_time)
    }

    /// Format ETA as human-readable string with smart units.
    /// - < 1 hour: "45m 30s"
    /// - >= 1 hour: "2h 15m"
    /// - >= 1 day: "1d 5h"
    pub fn eta_formatted(&self) -> String {
        match self.eta_seconds() {
            None => "N/A".to_string(),
            Some(secs) => format_duration_smart(secs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_new_initializes_correctly() {
        let progress = SyncProgress::new(100, 1000);
        assert_eq!(progress.current(), 100);
        assert_eq!(progress.target(), 1000);
        assert_eq!(progress.blocks_remaining(), 900);
    }

    #[test]
    fn test_update_current_increments_block() {
        let progress = SyncProgress::new(0, 100);
        progress.update_current(5);
        assert_eq!(progress.current(), 5);
    }

    #[test]
    fn test_update_current_batch() {
        let progress = SyncProgress::new(0, 1000);
        progress.update_current_batch(100, 100);
        assert_eq!(progress.current(), 100);
    }

    #[test]
    fn test_progress_percentage() {
        let progress = SyncProgress::new(0, 100);
        progress.update_current_batch(50, 50);
        assert!((progress.progress_percentage() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_percentage_zero_target() {
        let progress = SyncProgress::new(0, 0);
        assert!((progress.progress_percentage() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_is_synced() {
        let progress = SyncProgress::new(0, 100);
        assert!(!progress.is_synced());
        progress.update_current_batch(100, 100);
        assert!(progress.is_synced());
    }

    #[test]
    fn test_blocks_per_second_initial_returns_zero() {
        let progress = SyncProgress::new(0, 1000);
        assert_eq!(progress.blocks_per_second(), 0.0);
    }

    #[test]
    fn test_blocks_per_second_after_processing() {
        let progress = SyncProgress::new(0, 10000);
        progress.update_current_batch(1000, 1000);
        thread::sleep(Duration::from_millis(1100));
        let rate = progress.blocks_per_second();
        assert!(rate > 0.0, "rate should be positive: {}", rate);
        assert!(rate < 2000.0, "rate should be reasonable: {}", rate);
    }

    #[test]
    fn test_blocks_per_second_uses_cached_rate_when_window_too_short() {
        let progress = SyncProgress::new(0, 10000);
        progress.update_current_batch(1000, 1000);
        thread::sleep(Duration::from_millis(1100));
        let _ = progress.blocks_per_second();

        thread::sleep(Duration::from_secs(11));
        progress.update_current_batch(2000, 1000);

        let rate = progress.blocks_per_second();
        assert!(rate > 0.0, "should return cached rate, got: {}", rate);
    }

    #[test]
    fn test_ema_smooths_rate_fluctuations() {
        let progress = SyncProgress::new(0, 100000);

        // Window 1: 1000 blocks
        progress.update_current_batch(1000, 1000);
        thread::sleep(Duration::from_millis(10200));
        // This triggers window 1 completion: rate ≈ 98 blocks/sec, EMA = 98
        progress.update_current_batch(2000, 1000);
        let ema1 = progress.ema_blocks_per_second();
        assert!(
            ema1 > 0.0,
            "EMA should be positive after first window: {}",
            ema1
        );

        // Window 2: 1000 blocks (from previous batch)
        thread::sleep(Duration::from_millis(10200));
        // This triggers window 2 completion: rate ≈ 98 blocks/sec
        // EMA updates but stays similar since rate is similar
        progress.update_current_batch(12000, 10000);

        // Window 3: 10000 blocks - need to wait for this window to complete
        // to see the higher rate reflected in EMA
        thread::sleep(Duration::from_millis(10200));
        // This triggers window 3 completion: rate ≈ 980 blocks/sec
        // EMA = 0.1 * 980 + 0.9 * ~98 ≈ 186
        progress.update_current_batch(13000, 1000);
        let ema2 = progress.ema_blocks_per_second();

        assert!(
            ema2 > ema1,
            "EMA should increase with higher rate: ema1={}, ema2={}",
            ema1,
            ema2
        );
    }

    #[test]
    fn test_ema_blocks_per_second_after_window_reset() {
        let progress = SyncProgress::new(0, 10000);
        progress.update_current_batch(1000, 1000);
        thread::sleep(Duration::from_millis(10100));
        progress.update_current_batch(2000, 1000);
        let ema = progress.ema_blocks_per_second();
        assert!(
            ema > 0.0,
            "EMA should be positive after window reset: {}",
            ema
        );
    }

    #[test]
    fn test_update_target() {
        let progress = SyncProgress::new(0, 100);
        assert_eq!(progress.target(), 100);
        progress.update_target(200);
        assert_eq!(progress.target(), 200);
    }

    #[test]
    fn test_format_duration_smart_seconds() {
        assert_eq!(format_duration_smart(0.0), "0s");
        assert_eq!(format_duration_smart(30.0), "30s");
        assert_eq!(format_duration_smart(59.0), "59s");
    }

    #[test]
    fn test_format_duration_smart_minutes() {
        assert_eq!(format_duration_smart(60.0), "1m 0s");
        assert_eq!(format_duration_smart(90.0), "1m 30s");
        assert_eq!(format_duration_smart(2730.0), "45m 30s");
        assert_eq!(format_duration_smart(3599.0), "59m 59s");
    }

    #[test]
    fn test_format_duration_smart_hours() {
        assert_eq!(format_duration_smart(3600.0), "1h 0m");
        assert_eq!(format_duration_smart(8100.0), "2h 15m");
        assert_eq!(format_duration_smart(86399.0), "23h 59m");
    }

    #[test]
    fn test_format_duration_smart_days() {
        assert_eq!(format_duration_smart(86400.0), "1d 0h");
        assert_eq!(format_duration_smart(104400.0), "1d 5h");
        assert_eq!(format_duration_smart(172800.0), "2d 0h");
    }

    #[test]
    fn test_eta_returns_none_when_synced() {
        let progress = SyncProgress::new(100, 100);
        assert!(progress.eta_seconds().is_none());
    }

    #[test]
    fn test_eta_returns_none_when_ema_zero() {
        let progress = SyncProgress::new(0, 1000);
        assert!(progress.eta_seconds().is_none());
    }

    #[test]
    fn test_eta_formatted_returns_na_when_unavailable() {
        let progress = SyncProgress::new(0, 1000);
        assert_eq!(progress.eta_formatted(), "N/A");
    }

    #[test]
    fn test_speed_trend_needs_minimum_samples() {
        let progress = SyncProgress::new(0, 1_000_000);
        assert!(
            progress.calculate_speed_trend().is_none(),
            "Trend should be None with no samples"
        );

        progress.record_speed_sample(1000, 100.0);
        assert!(
            progress.calculate_speed_trend().is_none(),
            "Trend should be None with 1 sample"
        );

        progress.record_speed_sample(2000, 95.0);
        assert!(
            progress.calculate_speed_trend().is_none(),
            "Trend should be None with 2 samples"
        );

        progress.record_speed_sample(3000, 90.0);
        assert!(
            progress.calculate_speed_trend().is_some(),
            "Trend should be Some with 3 samples"
        );
    }

    #[test]
    fn test_speed_trend_detects_slowdown() {
        let progress = SyncProgress::new(0, 1_000_000);

        progress.record_speed_sample(0, 1000.0);
        progress.record_speed_sample(100_000, 800.0);
        progress.record_speed_sample(200_000, 600.0);
        progress.record_speed_sample(300_000, 400.0);

        let (slope, _) = progress.calculate_speed_trend().unwrap();
        assert!(
            slope < 0.0,
            "Slope should be negative for slowing down: {}",
            slope
        );
    }

    #[test]
    fn test_speed_trend_detects_speedup() {
        let progress = SyncProgress::new(0, 1_000_000);

        progress.record_speed_sample(0, 400.0);
        progress.record_speed_sample(100_000, 600.0);
        progress.record_speed_sample(200_000, 800.0);
        progress.record_speed_sample(300_000, 1000.0);

        let (slope, _) = progress.calculate_speed_trend().unwrap();
        assert!(
            slope > 0.0,
            "Slope should be positive for speeding up: {}",
            slope
        );
    }

    #[test]
    fn test_predict_speed_clamps_to_reasonable_range() {
        let progress = SyncProgress::new(0, 1_000_000);
        progress
            .ema_rate
            .store((500.0_f64).to_bits(), Ordering::SeqCst);

        progress.record_speed_sample(0, 1000.0);
        progress.record_speed_sample(100_000, 500.0);
        progress.record_speed_sample(200_000, 100.0);

        let predicted = progress.predict_speed_at(1_000_000);
        let min_allowed = 500.0 * 0.1;
        let max_allowed = 500.0 * 2.0;
        assert!(
            predicted >= min_allowed && predicted <= max_allowed,
            "Predicted speed {} should be between {} and {}",
            predicted,
            min_allowed,
            max_allowed
        );
    }

    #[test]
    fn test_eta_with_trend_accounts_for_slowdown() {
        let progress = SyncProgress::new(0, 1_000_000);
        progress
            .ema_rate
            .store((1000.0_f64).to_bits(), Ordering::SeqCst);

        progress.record_speed_sample(0, 2000.0);
        progress.record_speed_sample(100_000, 1500.0);
        progress.record_speed_sample(200_000, 1000.0);

        let eta = progress.eta_seconds().unwrap();
        let simple_eta = 1_000_000.0 / 1000.0;

        assert!(
            eta > simple_eta,
            "ETA with slowdown trend ({}) should be > simple ETA ({})",
            eta,
            simple_eta
        );
    }

    #[test]
    fn test_eta_falls_back_to_simple_without_trend() {
        let progress = SyncProgress::new(0, 10_000);
        progress
            .ema_rate
            .store((100.0_f64).to_bits(), Ordering::SeqCst);

        let eta = progress.eta_seconds().unwrap();
        let expected = 10_000.0 / 100.0;
        assert!(
            (eta - expected).abs() < 1.0,
            "ETA {} should be close to simple calculation {}",
            eta,
            expected
        );
    }

    #[test]
    fn test_speed_history_respects_max_size() {
        let progress = SyncProgress::new(0, 1_000_000);

        for i in 0..(SPEED_HISTORY_SIZE + 10) {
            progress.record_speed_sample(i as u64 * 1000, 100.0);
        }

        let history = progress.speed_history.read().unwrap();
        assert_eq!(
            history.len(),
            SPEED_HISTORY_SIZE,
            "History size should be capped at {}",
            SPEED_HISTORY_SIZE
        );
    }
}
