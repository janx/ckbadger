//! Deterministic LCG sampler for reproducible sampling checks.

/// Linear Congruential Generator with deterministic seeding.
/// Uses standard LCG constants (Numerical Recipes).
pub struct LcgSampler {
    state: u64,
}

impl LcgSampler {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Generate N unique random numbers in [0, max).
    pub fn sample_range(&mut self, n: usize, max: u64) -> Vec<u64> {
        if max == 0 {
            return vec![];
        }
        let n = n.min(max as usize);
        let mut result = Vec::with_capacity(n);
        // Use reservoir-like approach for small n relative to max
        let mut seen = std::collections::HashSet::with_capacity(n);
        let max_attempts = n * 10;
        let mut attempts = 0;
        while result.len() < n && attempts < max_attempts {
            let val = self.next() % max;
            if seen.insert(val) {
                result.push(val);
            }
            attempts += 1;
        }
        result.sort();
        result
    }
}

/// Compute a skip interval for uniformly sampling a CF iterator.
/// Returns the skip interval to get approximately `n` samples from `total` items.
pub fn skip_interval(total: u64, n: usize) -> u64 {
    if n == 0 || total == 0 {
        return 1;
    }
    (total / n as u64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lcg_deterministic() {
        let mut s1 = LcgSampler::new(42);
        let mut s2 = LcgSampler::new(42);
        let r1 = s1.sample_range(10, 1000);
        let r2 = s2.sample_range(10, 1000);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_lcg_different_seeds_differ() {
        let mut s1 = LcgSampler::new(42);
        let mut s2 = LcgSampler::new(99);
        let r1 = s1.sample_range(10, 1000);
        let r2 = s2.sample_range(10, 1000);
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_lcg_sample_range_unique() {
        let mut s = LcgSampler::new(42);
        let result = s.sample_range(100, 10_000);
        assert_eq!(result.len(), 100);
        let set: std::collections::HashSet<_> = result.iter().collect();
        assert_eq!(set.len(), 100);
    }

    #[test]
    fn test_lcg_sample_range_bounded() {
        let mut s = LcgSampler::new(42);
        let result = s.sample_range(50, 100);
        for v in &result {
            assert!(*v < 100);
        }
    }

    #[test]
    fn test_lcg_sample_range_capped_at_max() {
        let mut s = LcgSampler::new(42);
        let result = s.sample_range(200, 50);
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn test_lcg_sample_range_zero_max() {
        let mut s = LcgSampler::new(42);
        let result = s.sample_range(10, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_skip_interval_normal() {
        assert_eq!(skip_interval(1000, 10), 100);
        assert_eq!(skip_interval(100, 100), 1);
    }

    #[test]
    fn test_skip_interval_edge_cases() {
        assert_eq!(skip_interval(0, 10), 1);
        assert_eq!(skip_interval(100, 0), 1);
    }
}
