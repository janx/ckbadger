use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub use ckbadger_common::format_duration_smart;

const WINDOW_SECS: u64 = 10;
/// EMA smoothing factor: 0.1 = slow adaptation, 0.3 = faster adaptation
const EMA_ALPHA: f64 = 0.1;

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

    /// Calculate ETA based on EMA speed and remaining blocks.
    pub fn eta_seconds(&self) -> Option<f64> {
        let remaining = self.blocks_remaining();
        if remaining == 0 {
            return None;
        }

        let ema_rate = self.ema_blocks_per_second();
        if ema_rate <= 0.0 {
            return None;
        }

        Some(remaining as f64 / ema_rate)
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
    fn test_eta_simple_calculation() {
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
}
