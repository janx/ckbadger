use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use super::collector::{EndpointStageMetrics, StageResult, StageStatus};
use super::scenario::Scenario;

// ---------------------------------------------------------------------------
// ScenarioReport -- collected results for one scenario
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioReport {
    pub scenario: Scenario,
    pub stage_results: Vec<StageResult>,
    pub soft_degradation_vus: Option<u32>,
    pub breaking_point_vus: Option<u32>,
}

// ---------------------------------------------------------------------------
// StressConfig -- serializable run configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressConfig {
    pub scenarios: String,
    pub stage_duration_secs: u64,
    pub auto_ramp: bool,
    pub think_time_ms: String,
    pub timeout_ms: u64,
}

// ---------------------------------------------------------------------------
// StressReport -- top-level report containing all scenarios
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressReport {
    pub timestamp: String,
    pub target: String,
    pub config: StressConfig,
    pub scenarios: Vec<ScenarioReport>,
}

// ---------------------------------------------------------------------------
// JSON-serializable report types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageJson {
    pub stage_id: usize,
    pub vus: usize,
    pub duration_secs: f64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub error_rate: f64,
    pub error_count: u64,
    pub connection_refused: u64,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointBreakdownEntry {
    pub endpoint_path: String,
    pub read_pattern: String,
    pub stable_p95_ms: f64,
    pub break_p95_ms: f64,
    pub degradation: f64,
    pub verdict: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPatternSummaryEntry {
    pub pattern: String,
    pub endpoint_count: usize,
    pub stable_avg_p95_ms: f64,
    pub break_avg_p95_ms: f64,
    pub degradation: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioJson {
    pub scenario: Scenario,
    pub stages: Vec<StageJson>,
    pub endpoint_breakdown: Vec<EndpointBreakdownEntry>,
    pub read_pattern_summary: Vec<ReadPatternSummaryEntry>,
    pub soft_degradation_vus: Option<u32>,
    pub breaking_point_vus: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressReportJson {
    pub timestamp: String,
    pub target: String,
    pub config: StressConfig,
    pub scenarios: Vec<ScenarioJson>,
}

// ---------------------------------------------------------------------------
// stage_status_label
// ---------------------------------------------------------------------------

pub fn stage_status_label(status: StageStatus) -> &'static str {
    match status {
        StageStatus::Baseline => "baseline",
        StageStatus::Ok => "ok",
        StageStatus::SoftDegradation => "\u{26a0} soft degradation",
        StageStatus::ErrorsRising => "\u{26a0} errors rising",
        StageStatus::HardFailure => "\u{2716} breaking point",
    }
}

// ---------------------------------------------------------------------------
// build_endpoint_breakdown
// ---------------------------------------------------------------------------

fn endpoint_verdict(breaking_metrics: &EndpointStageMetrics, degradation: f64) -> String {
    if breaking_metrics.error_rate > 0.1 {
        "\u{2716} critical".to_string()
    } else if degradation > 10.0 {
        "\u{2716} first to break".to_string()
    } else if degradation > 3.0 {
        format!("{degradation:.1}\u{00d7} slow")
    } else {
        "ok".to_string()
    }
}

pub fn build_endpoint_breakdown(
    stable: &StageResult,
    breaking: &StageResult,
) -> Vec<EndpointBreakdownEntry> {
    let mut entries = Vec::new();

    for (&idx, stable_ep) in &stable.per_endpoint {
        if let Some(break_ep) = breaking.per_endpoint.get(&idx) {
            let degradation = if stable_ep.p95_ms > 0.0 {
                break_ep.p95_ms / stable_ep.p95_ms
            } else {
                1.0
            };

            let verdict = endpoint_verdict(break_ep, degradation);

            entries.push(EndpointBreakdownEntry {
                endpoint_path: stable_ep.endpoint_path.clone(),
                read_pattern: stable_ep.read_pattern.clone(),
                stable_p95_ms: stable_ep.p95_ms,
                break_p95_ms: break_ep.p95_ms,
                degradation,
                verdict,
            });
        }
    }

    // Sort by degradation descending, then by endpoint_path for stability
    entries.sort_by(|a, b| {
        b.degradation
            .partial_cmp(&a.degradation)
            .unwrap()
            .then_with(|| a.endpoint_path.cmp(&b.endpoint_path))
    });
    entries
}

// ---------------------------------------------------------------------------
// build_read_pattern_summary
// ---------------------------------------------------------------------------

pub fn build_read_pattern_summary(
    stable: &StageResult,
    breaking: &StageResult,
) -> Vec<ReadPatternSummaryEntry> {
    // Group per_endpoint entries by read_pattern
    let mut pattern_stable: HashMap<String, Vec<f64>> = HashMap::new();
    let mut pattern_break: HashMap<String, Vec<f64>> = HashMap::new();

    for (&idx, stable_ep) in &stable.per_endpoint {
        pattern_stable
            .entry(stable_ep.read_pattern.clone())
            .or_default()
            .push(stable_ep.p95_ms);

        if let Some(break_ep) = breaking.per_endpoint.get(&idx) {
            pattern_break
                .entry(break_ep.read_pattern.clone())
                .or_default()
                .push(break_ep.p95_ms);
        }
    }

    let mut entries = Vec::new();
    for (pattern, stable_vals) in &pattern_stable {
        let endpoint_count = stable_vals.len();
        let stable_avg = stable_vals.iter().sum::<f64>() / stable_vals.len() as f64;
        let break_vals = pattern_break.get(pattern);
        let break_avg = match break_vals {
            Some(vals) if !vals.is_empty() => vals.iter().sum::<f64>() / vals.len() as f64,
            _ => stable_avg, // no breaking data: assume same
        };
        let degradation = if stable_avg > 0.0 {
            break_avg / stable_avg
        } else {
            1.0
        };

        entries.push(ReadPatternSummaryEntry {
            pattern: pattern.clone(),
            endpoint_count,
            stable_avg_p95_ms: stable_avg,
            break_avg_p95_ms: break_avg,
            degradation,
        });
    }

    // Sort by degradation descending
    entries.sort_by(|a, b| b.degradation.partial_cmp(&a.degradation).unwrap());
    entries
}

// ---------------------------------------------------------------------------
// Table formatting helpers
// ---------------------------------------------------------------------------

fn pad_right(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - s.len()))
    }
}

fn pad_left(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(width - s.len()))
    }
}

/// Find the last stage before HardFailure or ErrorsRising -- the "stable" stage.
fn find_last_stable(stages: &[StageResult]) -> Option<&StageResult> {
    // Walk backwards looking for Ok or Baseline
    stages.iter().rev().find(|s| {
        matches!(
            s.status,
            StageStatus::Baseline | StageStatus::Ok | StageStatus::SoftDegradation
        )
    })
}

// ---------------------------------------------------------------------------
// print_tables
// ---------------------------------------------------------------------------

pub fn print_tables(report: &StressReport) {
    for sr in &report.scenarios {
        eprintln!();
        eprintln!("=== Scenario: {:?} ===", sr.scenario);
        eprintln!();

        // 1. Stage summary table
        eprintln!(
            "  {}  {}  {}  {}  {}  {}  {}  {}  Status",
            pad_right("Stage", 6),
            pad_right("VUs", 6),
            pad_right("Dur(s)", 7),
            pad_right("RPS", 8),
            pad_right("p50", 8),
            pad_right("p95", 8),
            pad_right("p99", 8),
            pad_right("Err%", 7)
        );
        eprintln!("  {}", "-".repeat(78));

        for stage in &sr.stage_results {
            let status_label = stage_status_label(stage.status);
            eprintln!(
                "  {}  {}  {}  {}  {}  {}  {}  {}  {}",
                pad_right(&format!("{}", stage.stage_id + 1), 6),
                pad_right(&format!("{}", stage.vus), 6),
                pad_right(&format!("{:.0}", stage.duration.as_secs_f64()), 7),
                pad_right(&format!("{:.0}", stage.rps), 8),
                pad_right(&format!("{:.0}ms", stage.p50_ms), 8),
                pad_right(&format!("{:.0}ms", stage.p95_ms), 8),
                pad_right(&format!("{:.0}ms", stage.p99_ms), 8),
                pad_right(&format!("{:.1}%", stage.error_rate * 100.0), 7),
                status_label,
            );
        }

        // Soft degradation / breaking point summary
        eprintln!();
        if let Some(vus) = sr.soft_degradation_vus {
            eprintln!("  \u{26a0}  Soft degradation detected at {vus} VUs");
        }
        if let Some(vus) = sr.breaking_point_vus {
            eprintln!("  \u{2716}  Breaking point reached at {vus} VUs");
        } else {
            eprintln!("  \u{2705}  No breaking point detected within test range");
        }

        // 2. Endpoint breakdown (last stable vs last stage)
        if sr.stage_results.len() >= 2 {
            let last_stage = sr.stage_results.last().unwrap();
            if let Some(stable_stage) = find_last_stable(&sr.stage_results) {
                // Only show if the last stage differs from stable
                if stable_stage.stage_id != last_stage.stage_id {
                    let breakdown = build_endpoint_breakdown(stable_stage, last_stage);

                    if !breakdown.is_empty() {
                        eprintln!();
                        eprintln!(
                            "  --- Endpoint Breakdown (stable @{}VUs vs break @{}VUs) ---",
                            stable_stage.vus, last_stage.vus
                        );
                        eprintln!();
                        eprintln!(
                            "  {}  {}  {}  {}  Verdict",
                            pad_right("Endpoint", 30),
                            pad_right("Pattern", 14),
                            pad_right("Stable p95", 12),
                            pad_right("Break p95", 12)
                        );
                        eprintln!("  {}", "-".repeat(80));

                        for ep in &breakdown {
                            eprintln!(
                                "  {}  {}  {}  {}  {}",
                                pad_right(&ep.endpoint_path, 30),
                                pad_right(&ep.read_pattern, 14),
                                pad_right(&format!("{:.0}ms", ep.stable_p95_ms), 12),
                                pad_right(&format!("{:.0}ms", ep.break_p95_ms), 12),
                                ep.verdict,
                            );
                        }

                        // "First to break" and "Most resilient"
                        if let Some(worst) = breakdown.first() {
                            if worst.degradation > 1.0 {
                                eprintln!();
                                eprintln!(
                                    "  First to break: {} ({:.1}\u{00d7})",
                                    worst.endpoint_path, worst.degradation
                                );
                            }
                        }
                        if let Some(best) = breakdown.last() {
                            eprintln!(
                                "  Most resilient: {} ({:.1}\u{00d7})",
                                best.endpoint_path, best.degradation
                            );
                        }
                    }

                    // 3. Read pattern summary
                    let pattern_summary = build_read_pattern_summary(stable_stage, last_stage);

                    if !pattern_summary.is_empty() {
                        eprintln!();
                        eprintln!("  --- Read Pattern Summary ---");
                        eprintln!();
                        eprintln!(
                            "  {}  {}  {}  {}  Degradation",
                            pad_right("ReadPattern", 14),
                            pad_left("Endpoints", 10),
                            pad_right("Avg p95 @stable", 16),
                            pad_right("Avg p95 @break", 16)
                        );
                        eprintln!("  {}", "-".repeat(72));

                        for ps in &pattern_summary {
                            let flag = if ps.degradation > 25.0 {
                                " \u{2716}"
                            } else if ps.degradation > 15.0 {
                                " \u{26a0}"
                            } else {
                                ""
                            };
                            eprintln!(
                                "  {}  {}  {}  {}  {:.1}\u{00d7}{}",
                                pad_right(&ps.pattern, 14),
                                pad_left(&format!("{}", ps.endpoint_count), 10),
                                pad_right(&format!("{:.0}ms", ps.stable_avg_p95_ms), 16),
                                pad_right(&format!("{:.0}ms", ps.break_avg_p95_ms), 16),
                                ps.degradation,
                                flag,
                            );
                        }
                    }
                }
            }
        }

        eprintln!();
    }
}

// ---------------------------------------------------------------------------
// JSON output helpers
// ---------------------------------------------------------------------------

fn build_report_json(report: &StressReport) -> StressReportJson {
    let scenarios = report
        .scenarios
        .iter()
        .map(|sr| {
            let stages: Vec<StageJson> = sr
                .stage_results
                .iter()
                .map(|s| StageJson {
                    stage_id: s.stage_id,
                    vus: s.vus,
                    duration_secs: s.duration.as_secs_f64(),
                    rps: s.rps,
                    p50_ms: s.p50_ms,
                    p95_ms: s.p95_ms,
                    p99_ms: s.p99_ms,
                    error_rate: s.error_rate,
                    error_count: s.error_count,
                    connection_refused: s.connection_refused,
                    status: stage_status_label(s.status).to_string(),
                })
                .collect();

            // Build endpoint breakdown and pattern summary from last stable vs last
            let last_stage = sr.stage_results.last();
            let stable_stage = find_last_stable(&sr.stage_results);

            let (endpoint_breakdown, read_pattern_summary) = match (stable_stage, last_stage) {
                (Some(stable), Some(last)) if stable.stage_id != last.stage_id => {
                    let breakdown = build_endpoint_breakdown(stable, last);
                    let pattern = build_read_pattern_summary(stable, last);
                    (breakdown, pattern)
                }
                _ => (Vec::new(), Vec::new()),
            };

            ScenarioJson {
                scenario: sr.scenario,
                stages,
                endpoint_breakdown,
                read_pattern_summary,
                soft_degradation_vus: sr.soft_degradation_vus,
                breaking_point_vus: sr.breaking_point_vus,
            }
        })
        .collect();

    StressReportJson {
        timestamp: report.timestamp.clone(),
        target: report.target.clone(),
        config: report.config.clone(),
        scenarios,
    }
}

// ---------------------------------------------------------------------------
// print_json
// ---------------------------------------------------------------------------

pub fn print_json(report: &StressReport) -> Result<()> {
    let json_report = build_report_json(report);
    let json = serde_json::to_string_pretty(&json_report)?;
    println!("{json}");
    Ok(())
}

// ---------------------------------------------------------------------------
// save_json
// ---------------------------------------------------------------------------

pub fn save_json(report: &StressReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json_report = build_report_json(report);
    let json = serde_json::to_string_pretty(&json_report)?;
    std::fs::write(path, json)?;
    eprintln!("Report saved to {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;
    use crate::stress::collector::{EndpointStageMetrics, StageResult, StageStatus};

    fn make_stage(
        stage_id: usize,
        vus: usize,
        status: StageStatus,
        per_endpoint: HashMap<usize, EndpointStageMetrics>,
    ) -> StageResult {
        StageResult {
            stage_id,
            vus,
            duration: Duration::from_secs(30),
            total_requests: 1000,
            rps: 33.3,
            p50_ms: 10.0,
            p95_ms: 50.0,
            p99_ms: 100.0,
            error_rate: 0.0,
            error_count: 0,
            connection_refused: 0,
            timeouts: 0,
            per_endpoint,
            status,
        }
    }

    fn make_ep(path: &str, pattern: &str, p95_ms: f64, error_rate: f64) -> EndpointStageMetrics {
        EndpointStageMetrics {
            endpoint_path: path.to_string(),
            read_pattern: pattern.to_string(),
            count: 100,
            p50_ms: p95_ms * 0.5,
            p95_ms,
            p99_ms: p95_ms * 1.5,
            error_rate,
        }
    }

    #[test]
    fn test_stage_status_label() {
        assert_eq!(stage_status_label(StageStatus::Baseline), "baseline");
        assert_eq!(stage_status_label(StageStatus::Ok), "ok");
        assert!(stage_status_label(StageStatus::SoftDegradation).contains("soft degradation"));
        assert!(stage_status_label(StageStatus::ErrorsRising).contains("errors rising"));
        assert!(stage_status_label(StageStatus::HardFailure).contains("breaking point"));
    }

    #[test]
    fn test_build_read_pattern_summary() {
        // Two endpoints with different patterns
        let mut stable_eps = HashMap::new();
        stable_eps.insert(0, make_ep("/blocks", "KeyLookup", 10.0, 0.0));
        stable_eps.insert(1, make_ep("/txs", "RangeScan", 20.0, 0.0));
        stable_eps.insert(2, make_ep("/blocks/{hash}", "KeyLookup", 15.0, 0.0));

        let mut break_eps = HashMap::new();
        break_eps.insert(0, make_ep("/blocks", "KeyLookup", 50.0, 0.0));
        break_eps.insert(1, make_ep("/txs", "RangeScan", 200.0, 0.0));
        break_eps.insert(2, make_ep("/blocks/{hash}", "KeyLookup", 75.0, 0.0));

        let stable = make_stage(0, 10, StageStatus::Ok, stable_eps);
        let breaking = make_stage(1, 100, StageStatus::HardFailure, break_eps);

        let summary = build_read_pattern_summary(&stable, &breaking);

        assert_eq!(summary.len(), 2, "should have 2 patterns");

        // RangeScan should be first (higher degradation: 200/20 = 10x)
        assert_eq!(summary[0].pattern, "RangeScan");
        assert_eq!(summary[0].endpoint_count, 1);
        assert!((summary[0].stable_avg_p95_ms - 20.0).abs() < 0.01);
        assert!((summary[0].break_avg_p95_ms - 200.0).abs() < 0.01);
        assert!((summary[0].degradation - 10.0).abs() < 0.01);

        // KeyLookup: avg stable = (10+15)/2 = 12.5, avg break = (50+75)/2 = 62.5
        // degradation = 62.5 / 12.5 = 5.0
        assert_eq!(summary[1].pattern, "KeyLookup");
        assert_eq!(summary[1].endpoint_count, 2);
        assert!((summary[1].stable_avg_p95_ms - 12.5).abs() < 0.01);
        assert!((summary[1].break_avg_p95_ms - 62.5).abs() < 0.01);
        assert!((summary[1].degradation - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_build_endpoint_breakdown() {
        let mut stable_eps = HashMap::new();
        stable_eps.insert(0, make_ep("/blocks", "KeyLookup", 10.0, 0.0));
        stable_eps.insert(1, make_ep("/txs", "RangeScan", 20.0, 0.0));
        stable_eps.insert(2, make_ep("/cells", "CrossStore", 5.0, 0.0));

        let mut break_eps = HashMap::new();
        // /blocks: degradation 5.0x -> "N.Nx slow"
        break_eps.insert(0, make_ep("/blocks", "KeyLookup", 50.0, 0.0));
        // /txs: error_rate > 0.1 -> "critical"
        break_eps.insert(1, make_ep("/txs", "RangeScan", 100.0, 0.2));
        // /cells: degradation 12.0x -> "first to break"
        break_eps.insert(2, make_ep("/cells", "CrossStore", 60.0, 0.0));

        let stable = make_stage(0, 10, StageStatus::Ok, stable_eps);
        let breaking = make_stage(1, 100, StageStatus::HardFailure, break_eps);

        let breakdown = build_endpoint_breakdown(&stable, &breaking);

        assert_eq!(breakdown.len(), 3);

        // Sorted by degradation desc, then path asc:
        //   /cells (12.0), /blocks (5.0), /txs (5.0)
        // /cells = 60/5 = 12.0
        assert_eq!(breakdown[0].endpoint_path, "/cells");
        assert!((breakdown[0].degradation - 12.0).abs() < 0.01);
        assert!(
            breakdown[0].verdict.contains("first to break"),
            "expected 'first to break', got: {}",
            breakdown[0].verdict
        );

        // /blocks = 50/10 = 5.0, no errors => "N.Nx slow"
        assert_eq!(breakdown[1].endpoint_path, "/blocks");
        assert!((breakdown[1].degradation - 5.0).abs() < 0.01);
        assert!(
            breakdown[1].verdict.contains("slow"),
            "expected 'slow', got: {}",
            breakdown[1].verdict
        );

        // /txs = 100/20 = 5.0 but error_rate > 0.1 => "critical"
        assert_eq!(breakdown[2].endpoint_path, "/txs");
        assert!((breakdown[2].degradation - 5.0).abs() < 0.01);
        assert!(
            breakdown[2].verdict.contains("critical"),
            "expected 'critical', got: {}",
            breakdown[2].verdict
        );
    }
}
