use std::sync::atomic::{AtomicU64, Ordering};

pub struct SyncProgress {
    current_block: AtomicU64,
    target_block: AtomicU64,
    blocks_processed: AtomicU64,
    start_time: std::time::Instant,
}

impl SyncProgress {
    pub fn new(start_block: u64, target_block: u64) -> Self {
        Self {
            current_block: AtomicU64::new(start_block),
            target_block: AtomicU64::new(target_block),
            blocks_processed: AtomicU64::new(0),
            start_time: std::time::Instant::now(),
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
        let processed = self.blocks_processed.load(Ordering::SeqCst) as f64;
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            processed / elapsed
        } else {
            0.0
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_progress_new() {
        let progress = SyncProgress::new(100, 1000);
        assert_eq!(progress.current(), 100);
        assert_eq!(progress.target(), 1000);
    }

    #[test]
    fn test_sync_progress_blocks_remaining() {
        let progress = SyncProgress::new(100, 1000);
        assert_eq!(progress.blocks_remaining(), 900);
    }

    #[test]
    fn test_sync_progress_blocks_remaining_at_tip() {
        let progress = SyncProgress::new(1000, 1000);
        assert_eq!(progress.blocks_remaining(), 0);
    }

    #[test]
    fn test_sync_progress_blocks_remaining_ahead_of_tip() {
        // Edge case: current > target (shouldn't happen, but test saturating_sub)
        let progress = SyncProgress::new(1100, 1000);
        assert_eq!(progress.blocks_remaining(), 0);
    }

    #[test]
    fn test_sync_progress_is_synced() {
        let progress = SyncProgress::new(999, 1000);
        assert!(!progress.is_synced());

        let progress = SyncProgress::new(1000, 1000);
        assert!(progress.is_synced());

        let progress = SyncProgress::new(1001, 1000);
        assert!(progress.is_synced());
    }

    #[test]
    fn test_sync_progress_update_current() {
        let progress = SyncProgress::new(0, 1000);
        progress.update_current(500);
        assert_eq!(progress.current(), 500);
        assert_eq!(progress.blocks_remaining(), 500);
    }

    #[test]
    fn test_sync_progress_update_current_batch() {
        let progress = SyncProgress::new(0, 1000);
        progress.update_current_batch(100, 100);
        assert_eq!(progress.current(), 100);
        assert_eq!(progress.blocks_remaining(), 900);
    }

    #[test]
    fn test_sync_progress_update_target() {
        let progress = SyncProgress::new(100, 1000);
        progress.update_target(2000);
        assert_eq!(progress.target(), 2000);
        assert_eq!(progress.blocks_remaining(), 1900);
    }

    #[test]
    fn test_sync_progress_percentage_zero_target() {
        let progress = SyncProgress::new(0, 0);
        assert_eq!(progress.progress_percentage(), 100.0);
    }

    #[test]
    fn test_sync_progress_percentage_partial() {
        let progress = SyncProgress::new(500, 1000);
        assert!((progress.progress_percentage() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_sync_progress_percentage_complete() {
        let progress = SyncProgress::new(1000, 1000);
        assert!((progress.progress_percentage() - 100.0).abs() < 0.001);
    }

    /// Test that simulates bulk sync threshold check logic
    #[test]
    fn test_bulk_sync_threshold_logic() {
        let threshold = 1000u64;

        // Far behind - bulk sync should be active
        let progress = SyncProgress::new(0, 10_000_000);
        assert!(progress.blocks_remaining() > threshold);

        // Just above threshold
        let progress = SyncProgress::new(9_998_999, 10_000_000);
        assert!(progress.blocks_remaining() > threshold);

        // At threshold - bulk sync should NOT be active
        let progress = SyncProgress::new(9_999_000, 10_000_000);
        assert!(progress.blocks_remaining() == threshold);

        // Below threshold - bulk sync should NOT be active
        let progress = SyncProgress::new(9_999_500, 10_000_000);
        assert!(progress.blocks_remaining() < threshold);
    }
}
