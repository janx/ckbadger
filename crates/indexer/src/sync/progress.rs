use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use ckbadger_common::format_duration_smart;

/// Sliding window duration for rate calculation (seconds).
const WINDOW_SECS: f64 = 30.0;
/// Background refresh interval (milliseconds). Ensures the displayed rate
/// smoothly decreases when no batches complete (slow batch / idle).
const REFRESH_INTERVAL_MS: u64 = 1000;
/// EMA smoothing factor: 0.1 = slow adaptation, 0.3 = faster adaptation
const EMA_ALPHA: f64 = 0.1;
/// Minimum time span (seconds) before computing a meaningful rate.
/// Avoids astronomical rates from a single event with near-zero elapsed time.
const MIN_SPAN_SECS: f64 = 0.5;

struct BatchEvent {
    completed_at: Instant,
    block_count: u64,
}

pub struct SyncProgress {
    current_block: AtomicU64,
    target_block: AtomicU64,
    /// Sliding window of recent batch completions.
    window: Mutex<VecDeque<BatchEvent>>,
    /// Current instantaneous rate (stored as bits of f64).
    current_rate: AtomicU64,
    /// EMA (Exponential Moving Average) for smoother speed estimation (stored as bits of f64).
    ema_rate: AtomicU64,
    /// Guard to ensure the refresher thread is only started once.
    refresher_running: AtomicBool,
}

impl SyncProgress {
    pub fn new(start_block: u64, target_block: u64) -> Self {
        Self {
            current_block: AtomicU64::new(start_block),
            target_block: AtomicU64::new(target_block),
            window: Mutex::new(VecDeque::new()),
            current_rate: AtomicU64::new(0),
            ema_rate: AtomicU64::new(0),
            refresher_running: AtomicBool::new(false),
        }
    }

    /// Record a completed batch. Called by the writer after each batch commit.
    /// Updates the sliding window, recomputes rate, and updates EMA.
    pub fn record_batch(&self, block: u64, count: u64) {
        self.current_block.store(block, Ordering::SeqCst);
        let now = Instant::now();
        let mut window = self.window.lock().unwrap();
        window.push_back(BatchEvent {
            completed_at: now,
            block_count: count,
        });
        Self::evict_old(&mut window, now);
        let rate = Self::compute_rate(&window, now);
        self.current_rate.store(rate.to_bits(), Ordering::SeqCst);
        self.update_ema(rate);
    }

    /// Recompute the rate using the current time without adding new events.
    /// Called periodically by the refresher thread so that the displayed rate
    /// naturally decreases when no batches are completing (slow batch / idle).
    fn refresh_rate(&self) {
        let now = Instant::now();
        let mut window = self.window.lock().unwrap();
        Self::evict_old(&mut window, now);
        let rate = Self::compute_rate(&window, now);
        self.current_rate.store(rate.to_bits(), Ordering::SeqCst);
        // Don't update EMA during refresh — only on actual batch completion.
    }

    /// Remove events older than the sliding window.
    fn evict_old(window: &mut VecDeque<BatchEvent>, now: Instant) {
        let cutoff = now - Duration::from_secs_f64(WINDOW_SECS);
        while let Some(front) = window.front() {
            if front.completed_at < cutoff {
                window.pop_front();
            } else {
                break;
            }
        }
    }

    /// Compute rate = total_blocks_in_window / (now - oldest_event_time).
    /// Using `now` as the denominator (instead of latest event) ensures the
    /// rate smoothly decreases when no new batches arrive.
    fn compute_rate(window: &VecDeque<BatchEvent>, now: Instant) -> f64 {
        if window.is_empty() {
            return 0.0;
        }
        let total_blocks: u64 = window.iter().map(|e| e.block_count).sum();
        let oldest = window.front().unwrap().completed_at;
        let span = now.duration_since(oldest).as_secs_f64();
        if span < MIN_SPAN_SECS {
            // Not enough elapsed time for a meaningful rate yet.
            return 0.0;
        }
        total_blocks as f64 / span
    }

    fn update_ema(&self, current_rate: f64) {
        if current_rate <= 0.0 {
            return;
        }
        let old_ema = f64::from_bits(self.ema_rate.load(Ordering::SeqCst));
        let new_ema = if old_ema == 0.0 {
            current_rate
        } else {
            EMA_ALPHA * current_rate + (1.0 - EMA_ALPHA) * old_ema
        };
        self.ema_rate.store(new_ema.to_bits(), Ordering::SeqCst);
    }

    /// Spawn a background thread that recomputes the rate every second.
    /// This ensures the displayed rate smoothly decays during slow batches
    /// instead of freezing at the last computed value.
    pub fn start_refresher(self: &Arc<Self>) {
        if self.refresher_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let progress = Arc::clone(self);
        std::thread::Builder::new()
            .name("rate-refresher".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(REFRESH_INTERVAL_MS));
                progress.refresh_rate();
            })
            .expect("failed to spawn rate-refresher thread");
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
    fn test_record_batch_updates_block() {
        let progress = SyncProgress::new(0, 100);
        progress.record_batch(5, 5);
        assert_eq!(progress.current(), 5);
    }

    #[test]
    fn test_record_batch_multiple() {
        let progress = SyncProgress::new(0, 1000);
        progress.record_batch(100, 100);
        assert_eq!(progress.current(), 100);
        progress.record_batch(200, 100);
        assert_eq!(progress.current(), 200);
    }

    #[test]
    fn test_progress_percentage() {
        let progress = SyncProgress::new(0, 100);
        progress.record_batch(50, 50);
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
        progress.record_batch(100, 100);
        assert!(progress.is_synced());
    }

    #[test]
    fn test_blocks_per_second_initial_returns_zero() {
        let progress = SyncProgress::new(0, 1000);
        assert_eq!(progress.blocks_per_second(), 0.0);
    }

    #[test]
    fn test_rate_becomes_positive_after_time() {
        let progress = Arc::new(SyncProgress::new(0, 100_000));
        progress.start_refresher();
        // Record a batch, then wait > REFRESH_INTERVAL_MS + MIN_SPAN_SECS
        // so the refresher has time to fire and compute a rate with span > 0.5s
        progress.record_batch(5000, 5000);
        thread::sleep(Duration::from_millis(1200));
        let rate = progress.blocks_per_second();
        assert!(
            rate > 0.0,
            "rate should be positive after refresher fires: {}",
            rate
        );
    }

    #[test]
    fn test_rate_decreases_during_idle() {
        let progress = Arc::new(SyncProgress::new(0, 100_000));
        progress.start_refresher();
        // Record batches with short delays to build up rate
        progress.record_batch(5000, 5000);
        thread::sleep(Duration::from_millis(600));
        progress.record_batch(10000, 5000);
        thread::sleep(Duration::from_millis(200));
        let rate1 = progress.blocks_per_second();
        // Now stop producing batches and wait — rate should decrease
        thread::sleep(Duration::from_millis(2000));
        let rate2 = progress.blocks_per_second();
        assert!(
            rate2 < rate1 || rate1 == 0.0,
            "rate should decrease during idle: rate1={}, rate2={}",
            rate1,
            rate2
        );
    }

    #[test]
    fn test_ema_updates_on_batch() {
        let progress = SyncProgress::new(0, 100_000);
        // First batch — wait for span > MIN_SPAN_SECS
        progress.record_batch(1000, 1000);
        thread::sleep(Duration::from_millis(600));
        // Second batch — now span > 0.5s so rate is computed and EMA set
        progress.record_batch(2000, 1000);
        let ema1 = progress.ema_blocks_per_second();
        assert!(
            ema1 > 0.0,
            "EMA should be positive after batches with sufficient span: {}",
            ema1
        );

        // Record a much larger batch to push rate higher
        thread::sleep(Duration::from_millis(100));
        progress.record_batch(52000, 50000);
        let ema2 = progress.ema_blocks_per_second();
        assert!(
            ema2 > ema1,
            "EMA should increase with higher throughput: ema1={}, ema2={}",
            ema1,
            ema2
        );
    }

    #[test]
    fn test_refresher_only_starts_once() {
        let progress = Arc::new(SyncProgress::new(0, 1000));
        progress.start_refresher();
        // Second call should be a no-op (not panic or spawn a second thread)
        progress.start_refresher();
        assert!(progress.refresher_running.load(Ordering::SeqCst));
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

    #[test]
    fn test_window_evicts_old_events() {
        let progress = SyncProgress::new(0, 100_000);
        // Manually insert an event with an old timestamp
        {
            let mut window = progress.window.lock().unwrap();
            window.push_back(BatchEvent {
                completed_at: Instant::now() - Duration::from_secs(60),
                block_count: 1000,
            });
        }
        // Record a new batch — the old event should be evicted
        progress.record_batch(2000, 1000);
        let window = progress.window.lock().unwrap();
        assert_eq!(window.len(), 1, "old event should have been evicted");
    }

    #[test]
    fn test_rate_zero_when_window_empty() {
        let window = VecDeque::new();
        let rate = SyncProgress::compute_rate(&window, Instant::now());
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_rate_zero_when_span_too_small() {
        let mut window = VecDeque::new();
        let now = Instant::now();
        window.push_back(BatchEvent {
            completed_at: now,
            block_count: 10000,
        });
        // Span ≈ 0, should return 0.0 (below MIN_SPAN_SECS)
        let rate = SyncProgress::compute_rate(&window, now);
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_compute_rate_basic() {
        let mut window = VecDeque::new();
        let now = Instant::now();
        // Event 2 seconds ago with 10000 blocks
        window.push_back(BatchEvent {
            completed_at: now - Duration::from_secs(2),
            block_count: 10000,
        });
        let rate = SyncProgress::compute_rate(&window, now);
        // rate = 10000 / 2.0 = 5000.0
        assert!(
            (rate - 5000.0).abs() < 100.0,
            "rate should be ~5000: {}",
            rate
        );
    }

    #[test]
    fn test_compute_rate_multiple_events() {
        let mut window = VecDeque::new();
        let now = Instant::now();
        window.push_back(BatchEvent {
            completed_at: now - Duration::from_secs(4),
            block_count: 5000,
        });
        window.push_back(BatchEvent {
            completed_at: now - Duration::from_secs(2),
            block_count: 5000,
        });
        // total = 10000, span = 4s → rate ≈ 2500
        let rate = SyncProgress::compute_rate(&window, now);
        assert!(
            (rate - 2500.0).abs() < 100.0,
            "rate should be ~2500: {}",
            rate
        );
    }
}
