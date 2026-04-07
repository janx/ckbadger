use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::metrics::percentile;

// ---------------------------------------------------------------------------
// Sample — sent by each VU after every request
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StressSample {
    pub endpoint_idx: usize,
    pub endpoint_path: String,
    pub read_pattern: String,
    pub latency_ms: f64,
    pub status: u16,
    pub body_size: usize,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Per-endpoint metrics for a single stage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EndpointStageMetrics {
    pub endpoint_path: String,
    pub read_pattern: String,
    pub count: u64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub error_rate: f64,
}

// ---------------------------------------------------------------------------
// Stage health status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    Baseline,
    Ok,
    SoftDegradation,
    ErrorsRising,
    HardFailure,
}

// ---------------------------------------------------------------------------
// StageResult — aggregate metrics for one completed stage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StageResult {
    pub stage_id: usize,
    pub vus: usize,
    pub duration: Duration,
    pub total_requests: u64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub error_rate: f64,
    pub error_count: u64,
    pub connection_refused: u64,
    pub timeouts: u64,
    pub per_endpoint: HashMap<usize, EndpointStageMetrics>,
    pub status: StageStatus,
}

impl StageResult {
    pub fn from_samples(
        stage_id: usize,
        vus: usize,
        duration: Duration,
        samples: &[StressSample],
    ) -> Self {
        let total_requests = samples.len() as u64;
        let wall_secs = duration.as_secs_f64();
        let rps = if wall_secs > 0.0 {
            total_requests as f64 / wall_secs
        } else {
            0.0
        };

        // Overall latency percentiles
        let mut latencies: Vec<f64> = samples.iter().map(|s| s.latency_ms).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let p50_ms = percentile(&latencies, 50.0);
        let p95_ms = percentile(&latencies, 95.0);
        let p99_ms = percentile(&latencies, 99.0);

        // Error counting
        let mut error_count: u64 = 0;
        let mut connection_refused: u64 = 0;
        let mut timeouts: u64 = 0;

        for s in samples {
            if s.error.is_some() {
                error_count += 1;
            }
            if let Some(ref err) = s.error {
                if err.contains("onnection refused") || s.status == 0 {
                    connection_refused += 1;
                }
                if err.contains("timed out") || err.contains("timeout") {
                    timeouts += 1;
                }
            }
        }

        let error_rate = if total_requests > 0 {
            error_count as f64 / total_requests as f64
        } else {
            0.0
        };

        // Per-endpoint aggregation
        let mut endpoint_groups: HashMap<usize, Vec<&StressSample>> = HashMap::new();
        for s in samples {
            endpoint_groups.entry(s.endpoint_idx).or_default().push(s);
        }

        let mut per_endpoint = HashMap::new();
        for (idx, group) in &endpoint_groups {
            let mut ep_latencies: Vec<f64> = group.iter().map(|s| s.latency_ms).collect();
            ep_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let ep_errors = group.iter().filter(|s| s.error.is_some()).count();
            let ep_error_rate = if group.is_empty() {
                0.0
            } else {
                ep_errors as f64 / group.len() as f64
            };

            // Use the first sample's path/pattern as representative
            let first = group[0];

            per_endpoint.insert(
                *idx,
                EndpointStageMetrics {
                    endpoint_path: first.endpoint_path.clone(),
                    read_pattern: first.read_pattern.clone(),
                    count: group.len() as u64,
                    p50_ms: percentile(&ep_latencies, 50.0),
                    p95_ms: percentile(&ep_latencies, 95.0),
                    p99_ms: percentile(&ep_latencies, 99.0),
                    error_rate: ep_error_rate,
                },
            );
        }

        Self {
            stage_id,
            vus,
            duration,
            total_requests,
            rps,
            p50_ms,
            p95_ms,
            p99_ms,
            error_rate,
            error_count,
            connection_refused,
            timeouts,
            per_endpoint,
            status: StageStatus::Ok,
        }
    }
}

// ---------------------------------------------------------------------------
// Degradation detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationSignal {
    None,
    SoftDegradation,
    ErrorsEmerging,
    HardFailure,
}

pub fn detect_degradation(baseline: &StageResult, current: &StageResult) -> DegradationSignal {
    // Check in priority order: hard failure first
    if current.error_rate > 0.10 {
        return DegradationSignal::HardFailure;
    }
    if current.error_rate > 0.01 {
        return DegradationSignal::ErrorsEmerging;
    }
    if baseline.p95_ms > 0.0 && current.p95_ms > 2.0 * baseline.p95_ms {
        return DegradationSignal::SoftDegradation;
    }
    DegradationSignal::None
}

// ---------------------------------------------------------------------------
// Real-time status line
// ---------------------------------------------------------------------------

pub struct StatusLine {
    pub current_stage: usize,
    pub total_stages: usize,
    pub vus: usize,
    pub elapsed_secs: f64,
    pub duration_secs: f64,
    pub rps: f64,
    pub p95_ms: f64,
    pub error_pct: f64,
}

impl StatusLine {
    pub fn print(&self) {
        let progress = if self.duration_secs > 0.0 {
            (self.elapsed_secs / self.duration_secs).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let filled = (progress * 10.0).round() as usize;
        let empty = 10 - filled;
        let bar: String = "▓".repeat(filled) + &"░".repeat(empty);

        eprint!(
            "\r[stage {}/{} \u{00b7} {} VUs \u{00b7} {:.0}s] rps={:.0}  p95={:.0}ms  err={:.1}%  {bar}",
            self.current_stage,
            self.total_stages,
            self.vus,
            self.elapsed_secs,
            self.rps,
            self.p95_ms,
            self.error_pct,
        );
        let _ = std::io::stderr().flush();
    }
}

// ---------------------------------------------------------------------------
// Channel types and helpers
// ---------------------------------------------------------------------------

pub type SampleSender = mpsc::UnboundedSender<StressSample>;
pub type SampleReceiver = mpsc::UnboundedReceiver<StressSample>;

pub fn sample_channel() -> (SampleSender, SampleReceiver) {
    mpsc::unbounded_channel()
}

pub fn drain_samples(rx: &mut SampleReceiver) -> Vec<StressSample> {
    let mut samples = Vec::new();
    while let Ok(sample) = rx.try_recv() {
        samples.push(sample);
    }
    samples
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample(
        endpoint_idx: usize,
        path: &str,
        pattern: &str,
        latency_ms: f64,
        error: Option<&str>,
    ) -> StressSample {
        StressSample {
            endpoint_idx,
            endpoint_path: path.to_string(),
            read_pattern: pattern.to_string(),
            latency_ms,
            status: if error.is_some() { 500 } else { 200 },
            body_size: 100,
            error: error.map(|e| e.to_string()),
        }
    }

    #[test]
    fn test_stage_result_from_samples() {
        let samples = vec![
            make_sample(0, "/blocks", "KeyLookup", 10.0, None),
            make_sample(0, "/blocks", "KeyLookup", 20.0, None),
            make_sample(0, "/blocks", "KeyLookup", 30.0, None),
            make_sample(0, "/blocks", "KeyLookup", 40.0, None),
            make_sample(0, "/blocks", "KeyLookup", 50.0, None),
        ];

        let result = StageResult::from_samples(1, 2, Duration::from_secs(5), &samples);

        assert_eq!(result.stage_id, 1);
        assert_eq!(result.vus, 2);
        assert_eq!(result.total_requests, 5);
        assert!((result.rps - 1.0).abs() < 0.01);
        assert_eq!(result.error_count, 0);
        assert_eq!(result.error_rate, 0.0);
        assert_eq!(result.p50_ms, 30.0);
        assert!(result.p95_ms > 40.0);
        assert_eq!(result.status, StageStatus::Ok);
    }

    #[test]
    fn test_stage_result_with_errors() {
        let samples = vec![
            make_sample(0, "/blocks", "KeyLookup", 10.0, None),
            make_sample(0, "/blocks", "KeyLookup", 20.0, None),
            make_sample(0, "/blocks", "KeyLookup", 30.0, Some("Connection refused")),
            make_sample(0, "/blocks", "KeyLookup", 40.0, Some("request timed out")),
            make_sample(0, "/blocks", "KeyLookup", 50.0, Some("server error")),
        ];

        let result = StageResult::from_samples(1, 2, Duration::from_secs(5), &samples);

        assert_eq!(result.error_count, 3);
        assert!((result.error_rate - 0.6).abs() < 0.01);
        assert_eq!(result.connection_refused, 1);
        assert_eq!(result.timeouts, 1);
    }

    #[test]
    fn test_detect_degradation_baseline() {
        let baseline = StageResult::from_samples(
            0,
            1,
            Duration::from_secs(10),
            &[
                make_sample(0, "/blocks", "KeyLookup", 10.0, None),
                make_sample(0, "/blocks", "KeyLookup", 20.0, None),
                make_sample(0, "/blocks", "KeyLookup", 30.0, None),
            ],
        );

        // Same performance as baseline
        let current = StageResult::from_samples(
            1,
            2,
            Duration::from_secs(10),
            &[
                make_sample(0, "/blocks", "KeyLookup", 10.0, None),
                make_sample(0, "/blocks", "KeyLookup", 20.0, None),
                make_sample(0, "/blocks", "KeyLookup", 30.0, None),
            ],
        );

        assert_eq!(
            detect_degradation(&baseline, &current),
            DegradationSignal::None
        );
    }

    #[test]
    fn test_detect_soft_degradation() {
        let baseline = StageResult::from_samples(
            0,
            1,
            Duration::from_secs(10),
            &[
                make_sample(0, "/blocks", "KeyLookup", 10.0, None),
                make_sample(0, "/blocks", "KeyLookup", 20.0, None),
                make_sample(0, "/blocks", "KeyLookup", 30.0, None),
            ],
        );

        // p95 is much higher (>2x baseline)
        let current = StageResult::from_samples(
            1,
            4,
            Duration::from_secs(10),
            &[
                make_sample(0, "/blocks", "KeyLookup", 50.0, None),
                make_sample(0, "/blocks", "KeyLookup", 60.0, None),
                make_sample(0, "/blocks", "KeyLookup", 100.0, None),
            ],
        );

        assert_eq!(
            detect_degradation(&baseline, &current),
            DegradationSignal::SoftDegradation
        );
    }

    #[test]
    fn test_detect_hard_failure() {
        let baseline = StageResult::from_samples(
            0,
            1,
            Duration::from_secs(10),
            &[
                make_sample(0, "/blocks", "KeyLookup", 10.0, None),
                make_sample(0, "/blocks", "KeyLookup", 20.0, None),
            ],
        );

        // >10% error rate
        let mut error_samples = Vec::new();
        for _ in 0..9 {
            error_samples.push(make_sample(0, "/blocks", "KeyLookup", 10.0, None));
        }
        // 2 errors out of 11 = ~18% error rate > 10%
        error_samples.push(make_sample(
            0,
            "/blocks",
            "KeyLookup",
            10.0,
            Some("server error"),
        ));
        error_samples.push(make_sample(
            0,
            "/blocks",
            "KeyLookup",
            10.0,
            Some("server error"),
        ));

        let current = StageResult::from_samples(1, 4, Duration::from_secs(10), &error_samples);

        assert_eq!(
            detect_degradation(&baseline, &current),
            DegradationSignal::HardFailure
        );
    }

    fn sample(latency_ms: f64, status: u16, error: Option<String>) -> StressSample {
        StressSample {
            endpoint_idx: 0,
            endpoint_path: "/test".to_string(),
            read_pattern: "KeyLookup".to_string(),
            latency_ms,
            status,
            body_size: 100,
            error,
        }
    }

    #[test]
    fn test_detect_errors_emerging() {
        let baseline = StageResult::from_samples(
            0,
            10,
            Duration::from_secs(30),
            &vec![sample(10.0, 200, None); 100],
        );
        // Error rate ~5% (between 1% and 10%)
        let mut error_samples: Vec<StressSample> = vec![sample(10.0, 200, None); 95];
        error_samples.extend(vec![sample(100.0, 500, Some("server error".into())); 5]);
        let current = StageResult::from_samples(1, 50, Duration::from_secs(30), &error_samples);
        assert_eq!(
            detect_degradation(&baseline, &current),
            DegradationSignal::ErrorsEmerging
        );
    }

    #[test]
    fn test_per_endpoint_metrics() {
        let samples = vec![
            make_sample(0, "/blocks", "KeyLookup", 10.0, None),
            make_sample(0, "/blocks", "KeyLookup", 20.0, None),
            make_sample(0, "/blocks", "KeyLookup", 30.0, None),
            make_sample(1, "/txs", "RangeScan", 100.0, None),
            make_sample(1, "/txs", "RangeScan", 200.0, None),
            make_sample(1, "/txs", "RangeScan", 300.0, Some("timeout")),
        ];

        let result = StageResult::from_samples(1, 2, Duration::from_secs(10), &samples);

        assert_eq!(result.per_endpoint.len(), 2);

        let ep0 = result.per_endpoint.get(&0).unwrap();
        assert_eq!(ep0.endpoint_path, "/blocks");
        assert_eq!(ep0.read_pattern, "KeyLookup");
        assert_eq!(ep0.count, 3);
        assert_eq!(ep0.p50_ms, 20.0);
        assert_eq!(ep0.error_rate, 0.0);

        let ep1 = result.per_endpoint.get(&1).unwrap();
        assert_eq!(ep1.endpoint_path, "/txs");
        assert_eq!(ep1.read_pattern, "RangeScan");
        assert_eq!(ep1.count, 3);
        assert_eq!(ep1.p50_ms, 200.0);
        assert!((ep1.error_rate - 1.0 / 3.0).abs() < 0.01);
    }
}
