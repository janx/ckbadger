use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub latency_ms: f64,
    pub status: u16,
    pub body_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputedMetrics {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub std_dev_ms: f64,
    pub error_rate: f64,
    pub avg_body_size: usize,
    pub throughput_rps: f64,
}

impl ComputedMetrics {
    pub fn from_samples(samples: &[Sample], wall_clock: Duration) -> Self {
        if samples.is_empty() {
            return Self {
                p50_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                min_ms: 0.0,
                max_ms: 0.0,
                mean_ms: 0.0,
                std_dev_ms: 0.0,
                error_rate: 0.0,
                avg_body_size: 0,
                throughput_rps: 0.0,
            };
        }

        let mut latencies: Vec<f64> = samples.iter().map(|s| s.latency_ms).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let n = latencies.len();
        let sum: f64 = latencies.iter().sum();
        let mean = sum / n as f64;
        let variance = latencies.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();
        let errors = samples.iter().filter(|s| s.error.is_some()).count();
        let total_body: usize = samples.iter().map(|s| s.body_size).sum();
        let wall_secs = wall_clock.as_secs_f64();
        let throughput = if wall_secs > 0.0 {
            n as f64 / wall_secs
        } else {
            0.0
        };

        Self {
            p50_ms: percentile(&latencies, 50.0),
            p95_ms: percentile(&latencies, 95.0),
            p99_ms: percentile(&latencies, 99.0),
            min_ms: latencies[0],
            max_ms: latencies[n - 1],
            mean_ms: mean,
            std_dev_ms: std_dev,
            error_rate: errors as f64 / n as f64,
            avg_body_size: total_body / n,
            throughput_rps: throughput,
        }
    }
}

pub fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (pct / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let frac = rank - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&data, 50.0), 3.0);
        assert_eq!(percentile(&data, 0.0), 1.0);
        assert_eq!(percentile(&data, 100.0), 5.0);
    }

    #[test]
    fn test_percentile_single() {
        let data = vec![42.0];
        assert_eq!(percentile(&data, 50.0), 42.0);
        assert_eq!(percentile(&data, 95.0), 42.0);
    }

    #[test]
    fn test_computed_metrics_from_samples() {
        let samples = vec![
            Sample {
                latency_ms: 10.0,
                status: 200,
                body_size: 100,
                error: None,
            },
            Sample {
                latency_ms: 20.0,
                status: 200,
                body_size: 200,
                error: None,
            },
            Sample {
                latency_ms: 30.0,
                status: 200,
                body_size: 300,
                error: None,
            },
        ];
        let m = ComputedMetrics::from_samples(&samples, Duration::from_secs(1));
        assert_eq!(m.mean_ms, 20.0);
        assert_eq!(m.min_ms, 10.0);
        assert_eq!(m.max_ms, 30.0);
        assert_eq!(m.error_rate, 0.0);
    }

    #[test]
    fn test_computed_metrics_with_errors() {
        let samples = vec![
            Sample {
                latency_ms: 10.0,
                status: 200,
                body_size: 100,
                error: None,
            },
            Sample {
                latency_ms: 20.0,
                status: 500,
                body_size: 0,
                error: Some("timeout".to_string()),
            },
        ];
        let m = ComputedMetrics::from_samples(&samples, Duration::from_secs(1));
        assert_eq!(m.error_rate, 0.5);
    }

    #[test]
    fn test_computed_metrics_empty() {
        let m = ComputedMetrics::from_samples(&[], Duration::from_secs(1));
        assert_eq!(m.mean_ms, 0.0);
        assert_eq!(m.min_ms, 0.0);
        assert_eq!(m.max_ms, 0.0);
        assert_eq!(m.error_rate, 0.0);
        assert_eq!(m.avg_body_size, 0);
        assert_eq!(m.throughput_rps, 0.0);
    }
}
