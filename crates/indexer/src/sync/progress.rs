use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use ckbadger_common::format_duration_smart;

const SAMPLE_INTERVAL_SECS: u64 = 5;
/// EMA smoothing factor: 0.1 = slow adaptation, 0.3 = faster adaptation
const EMA_ALPHA: f64 = 0.1;

pub struct SyncProgress {
    current_block: AtomicU64,
    target_block: AtomicU64,
    blocks_processed: AtomicU64,
    // Sampler thread state
    sampler_running: AtomicBool,
    current_rate: AtomicU64, // stored as bits of f64
    // EMA (Exponential Moving Average) for smoother speed estimation
    ema_rate: AtomicU64, // stored as bits of f64
}

impl SyncProgress {
    pub fn new(start_block: u64, target_block: u64) -> Self {
        Self {
            current_block: AtomicU64::new(start_block),
            target_block: AtomicU64::new(target_block),
            blocks_processed: AtomicU64::new(0),
            sampler_running: AtomicBool::new(false),
            current_rate: AtomicU64::new(0),
            ema_rate: AtomicU64::new(0),
        }
    }

    pub fn update_current(&self, block: u64) {
        self.current_block.store(block, Ordering::SeqCst);
        self.blocks_processed.fetch_add(1, Ordering::SeqCst);
    }

    pub fn update_current_batch(&self, block: u64, count: u64) {
        self.current_block.store(block, Ordering::SeqCst);
        self.blocks_processed.fetch_add(count, Ordering::SeqCst);
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

    /// Spawn a background thread that samples `blocks_processed` every
    /// `SAMPLE_INTERVAL_SECS` seconds and updates `current_rate` / EMA.
    pub fn start_sampler(self: &Arc<Self>) {
        // Guard: only start once
        if self.sampler_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let progress = Arc::clone(self);
        std::thread::Builder::new()
            .name("speed-sampler".into())
            .spawn(move || {
                let mut prev_blocks = progress.blocks_processed.load(Ordering::SeqCst);
                let mut prev_time = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_secs(SAMPLE_INTERVAL_SECS));
                    let now = Instant::now();
                    let curr_blocks = progress.blocks_processed.load(Ordering::SeqCst);
                    let delta = curr_blocks.saturating_sub(prev_blocks);
                    let elapsed = now.duration_since(prev_time).as_secs_f64();
                    if elapsed > 0.0 {
                        let rate = delta as f64 / elapsed;
                        progress
                            .current_rate
                            .store(rate.to_bits(), Ordering::SeqCst);
                        progress.update_ema(rate);
                    }
                    prev_blocks = curr_blocks;
                    prev_time = now;
                }
            })
            .expect("failed to spawn speed-sampler thread");
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
        f64::from_bits(self.current_rate.load(Ordering::SeqCst))
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
    fn test_blocks_per_second_after_sampler() {
        let progress = Arc::new(SyncProgress::new(0, 100_000));
        progress.start_sampler();
        // Process blocks during the sample interval so the delta is non-zero
        thread::sleep(Duration::from_millis(500));
        progress.update_current_batch(5000, 5000);
        // Wait for the sampler to wake and compute the rate
        thread::sleep(Duration::from_millis(5500));
        let rate = progress.blocks_per_second();
        assert!(
            rate > 0.0,
            "rate should be positive after sampler: {}",
            rate
        );
    }

    #[test]
    fn test_ema_updates_via_sampler() {
        let progress = Arc::new(SyncProgress::new(0, 100_000));
        progress.start_sampler();

        // Process blocks during first interval
        thread::sleep(Duration::from_millis(500));
        progress.update_current_batch(1000, 1000);
        // Wait for first sample
        thread::sleep(Duration::from_millis(5500));
        let ema1 = progress.ema_blocks_per_second();
        assert!(
            ema1 > 0.0,
            "EMA should be positive after first sample: {}",
            ema1
        );

        // Process many more blocks during second interval
        progress.update_current_batch(11000, 10000);
        // Wait for second sample
        thread::sleep(Duration::from_millis(5500));
        let ema2 = progress.ema_blocks_per_second();
        assert!(
            ema2 > ema1,
            "EMA should increase with higher throughput: ema1={}, ema2={}",
            ema1,
            ema2
        );
    }

    #[test]
    fn test_sampler_only_starts_once() {
        let progress = Arc::new(SyncProgress::new(0, 1000));
        progress.start_sampler();
        // Second call should be a no-op (not panic or spawn a second thread)
        progress.start_sampler();
        assert!(progress.sampler_running.load(Ordering::SeqCst));
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
