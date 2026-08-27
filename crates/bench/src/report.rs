use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::runner::EndpointResult;

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

const SLOW_THRESHOLD_MS: f64 = 100.0;
const VERY_SLOW_THRESHOLD_MS: f64 = 500.0;
const REGRESSION_THRESHOLD_PCT: f64 = 20.0;
const IMPROVEMENT_THRESHOLD_PCT: f64 = -10.0;

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReport {
    pub timestamp: String,
    pub config: ReportConfig,
    pub summary: ReportSummary,
    pub results: Vec<ReportEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportConfig {
    pub api_base: String,
    pub iterations: u32,
    pub concurrency: u32,
    pub warmup: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub tested: usize,
    pub skipped: usize,
    pub slow_count: usize,
    pub very_slow_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportEntry {
    pub module: String,
    pub method: String,
    pub path_template: String,
    pub resolved_url: String,
    pub read_pattern: String,
    pub risk_tier: String,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub error_rate: f64,
    /// HTTP status code histogram for error responses (status != expect_status).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub error_statuses: BTreeMap<u16, u32>,
    pub avg_body_size: usize,
    pub throughput_rps: f64,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Build report from raw results
// ---------------------------------------------------------------------------

pub fn build_report(
    results: &[EndpointResult],
    api_base: &str,
    iterations: u32,
    concurrency: u32,
    warmup: u32,
) -> BenchmarkReport {
    let entries: Vec<ReportEntry> = results.iter().map(entry_from_result).collect();

    let tested = entries.iter().filter(|e| !e.skipped).count();
    let skipped = entries.iter().filter(|e| e.skipped).count();
    let slow_count = entries
        .iter()
        .filter(|e| !e.skipped && e.p95_ms > SLOW_THRESHOLD_MS)
        .count();
    let very_slow_count = entries
        .iter()
        .filter(|e| !e.skipped && e.p95_ms > VERY_SLOW_THRESHOLD_MS)
        .count();
    let error_count = entries
        .iter()
        .filter(|e| !e.skipped && e.error_rate > 0.0)
        .count();

    BenchmarkReport {
        timestamp: Utc::now().to_rfc3339(),
        config: ReportConfig {
            api_base: api_base.to_string(),
            iterations,
            concurrency,
            warmup,
        },
        summary: ReportSummary {
            tested,
            skipped,
            slow_count,
            very_slow_count,
            error_count,
        },
        results: entries,
    }
}

fn entry_from_result(r: &EndpointResult) -> ReportEntry {
    let mut error_statuses = BTreeMap::new();
    for sample in &r.samples {
        if sample.error.is_some() {
            *error_statuses.entry(sample.status).or_insert(0) += 1;
        }
    }

    ReportEntry {
        module: r.module.clone(),
        method: r.method.clone(),
        path_template: r.path_template.clone(),
        resolved_url: r.resolved_url.clone(),
        read_pattern: r.read_pattern.clone(),
        risk_tier: r.risk_tier.clone(),
        p50_ms: r.metrics.p50_ms,
        p95_ms: r.metrics.p95_ms,
        p99_ms: r.metrics.p99_ms,
        min_ms: r.metrics.min_ms,
        max_ms: r.metrics.max_ms,
        mean_ms: r.metrics.mean_ms,
        error_rate: r.metrics.error_rate,
        error_statuses,
        avg_body_size: r.metrics.avg_body_size,
        throughput_rps: r.metrics.throughput_rps,
        skipped: r.skipped,
        skip_reason: r.skip_reason.clone(),
    }
}

// ---------------------------------------------------------------------------
// Terminal table output
// ---------------------------------------------------------------------------

pub fn print_table(report: &BenchmarkReport) {
    // Header
    println!();
    println!("=== ckbadger-bench report ===");
    println!("  Timestamp:   {}", report.timestamp);
    println!("  API URL:     {}", report.config.api_base);
    println!(
        "  Iterations:  {}   Concurrency: {}   Warmup: {}",
        report.config.iterations, report.config.concurrency, report.config.warmup
    );
    println!();

    // Column headers
    println!(
        "{:<12} {:<6} {:<36} {:<12} {:>8} {:>8} {:>8} {:>6} {:>8} Flag",
        "Module", "Method", "Endpoint", "Pattern", "p50", "p95", "p99", "Errs", "Size"
    );
    println!("{}", "-".repeat(120));

    // Group by module, with blank line separators
    let mut current_module = "";
    for entry in &report.results {
        if entry.module != current_module {
            if !current_module.is_empty() {
                println!();
            }
            current_module = &entry.module;
        }

        let flag = entry_flag(entry);

        if entry.skipped {
            println!(
                "{:<12} {:<6} {:<36} {:<12} {:>8} {:>8} {:>8} {:>6} {:>8} {}",
                entry.module,
                entry.method,
                truncate(&entry.path_template, 36),
                entry.read_pattern,
                "-",
                "-",
                "-",
                "-",
                "-",
                flag,
            );
        } else {
            println!(
                "{:<12} {:<6} {:<36} {:<12} {:>8.2} {:>8.2} {:>8.2} {:>5.1}% {:>8} {}",
                entry.module,
                entry.method,
                truncate(&entry.path_template, 36),
                entry.read_pattern,
                entry.p50_ms,
                entry.p95_ms,
                entry.p99_ms,
                entry.error_rate * 100.0,
                format_size(entry.avg_body_size),
                flag,
            );
        }
    }
    println!("{}", "-".repeat(120));

    // Summary line
    let s = &report.summary;
    println!(
        "Tested: {}  Skipped: {}  Slow (p95>{}ms): {}  Very slow (p95>{}ms): {}  Errors: {}",
        s.tested,
        s.skipped,
        SLOW_THRESHOLD_MS as u32,
        s.slow_count,
        VERY_SLOW_THRESHOLD_MS as u32,
        s.very_slow_count,
        s.error_count,
    );

    // Top 5 slowest
    let mut non_skipped: Vec<&ReportEntry> = report.results.iter().filter(|e| !e.skipped).collect();
    non_skipped.sort_by(|a, b| b.p95_ms.partial_cmp(&a.p95_ms).unwrap());
    let top5: Vec<&ReportEntry> = non_skipped.into_iter().take(5).collect();
    if !top5.is_empty() {
        println!();
        println!("Top 5 slowest (by p95):");
        for (i, entry) in top5.iter().enumerate() {
            println!(
                "  {}. {:<36}  p95={:.2}ms  p50={:.2}ms  pattern={}",
                i + 1,
                entry.path_template,
                entry.p95_ms,
                entry.p50_ms,
                entry.read_pattern,
            );
        }
    }
    println!();
}

fn entry_flag(entry: &ReportEntry) -> String {
    if entry.skipped {
        "SKIPPED".to_string()
    } else if entry.p95_ms > VERY_SLOW_THRESHOLD_MS {
        "VERY SLOW".to_string()
    } else if entry.error_rate > 0.0 {
        if entry.error_statuses.is_empty() {
            "ERRORS".to_string()
        } else {
            // Show status codes, e.g. "ERRORS(404)" or "ERRORS(404,500)"
            let codes: Vec<String> = entry
                .error_statuses
                .keys()
                .map(|code| code.to_string())
                .collect();
            format!("ERRORS({})", codes.join(","))
        }
    } else if entry.p95_ms > SLOW_THRESHOLD_MS {
        "SLOW".to_string()
    } else {
        String::new()
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

pub fn print_json(report: &BenchmarkReport) -> Result<()> {
    let json = serde_json::to_string_pretty(report).context("failed to serialize report")?;
    println!("{json}");
    Ok(())
}

pub fn save_json(report: &BenchmarkReport, path: &std::path::Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report).context("failed to serialize report")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, json)
        .with_context(|| format!("failed to write report to {}", path.display()))?;
    println!("Report saved to {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Regression comparison
// ---------------------------------------------------------------------------

pub fn compare_reports(current: &BenchmarkReport, baseline_path: &str) -> Result<()> {
    let baseline_json = fs::read_to_string(baseline_path)
        .with_context(|| format!("failed to read {baseline_path}"))?;
    let baseline: BenchmarkReport =
        serde_json::from_str(&baseline_json).context("failed to parse baseline JSON")?;

    println!();
    println!("=== Regression comparison ===");
    println!("  Current:  {}", current.timestamp);
    println!("  Baseline: {}", baseline.timestamp);
    println!();
    println!(
        "{:<6} {:<36} {:>10} {:>10} {:>10} Verdict",
        "Method", "Endpoint", "Base p95", "Curr p95", "Change"
    );
    println!("{}", "-".repeat(90));

    let mut regression_count = 0usize;
    let mut improved_count = 0usize;
    let mut stable_count = 0usize;
    let mut unmatched_count = 0usize;

    for entry in &current.results {
        if entry.skipped {
            continue;
        }

        let baseline_entry = baseline.results.iter().find(|b| {
            !b.skipped && b.path_template == entry.path_template && b.method == entry.method
        });

        match baseline_entry {
            Some(base) => {
                let change_pct = if base.p95_ms > 0.0 {
                    (entry.p95_ms - base.p95_ms) / base.p95_ms * 100.0
                } else {
                    0.0
                };

                let verdict = if change_pct > REGRESSION_THRESHOLD_PCT {
                    regression_count += 1;
                    "REGRESSION"
                } else if change_pct < IMPROVEMENT_THRESHOLD_PCT {
                    improved_count += 1;
                    "improved"
                } else {
                    stable_count += 1;
                    "stable"
                };

                println!(
                    "{:<6} {:<36} {:>9.2}ms {:>9.2}ms {:>+9.1}% {}",
                    entry.method,
                    truncate(&entry.path_template, 36),
                    base.p95_ms,
                    entry.p95_ms,
                    change_pct,
                    verdict,
                );
            }
            None => {
                unmatched_count += 1;
                println!(
                    "{:<6} {:<36} {:>10} {:>9.2}ms {:>10} new",
                    entry.method,
                    truncate(&entry.path_template, 36),
                    "N/A",
                    entry.p95_ms,
                    "-",
                );
            }
        }
    }

    println!("{}", "-".repeat(90));
    println!(
        "Regressions: {}  Improved: {}  Stable: {}  New: {}",
        regression_count, improved_count, stable_count, unmatched_count,
    );

    if regression_count > 0 {
        println!(
            "\nWARNING: {} endpoint(s) regressed by more than {}%",
            regression_count, REGRESSION_THRESHOLD_PCT as u32,
        );
    }
    println!();

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::metrics::{ComputedMetrics, Sample};

    use super::*;

    fn make_result(
        module: &str,
        method: &str,
        path: &str,
        p95: f64,
        error_rate: f64,
        skipped: bool,
    ) -> EndpointResult {
        make_result_with_status(module, method, path, p95, error_rate, skipped, 500)
    }

    fn make_result_with_status(
        module: &str,
        method: &str,
        path: &str,
        p95: f64,
        error_rate: f64,
        skipped: bool,
        error_status: u16,
    ) -> EndpointResult {
        EndpointResult {
            module: module.to_string(),
            method: method.to_string(),
            path_template: path.to_string(),
            description: String::new(),
            resolved_url: format!("http://localhost:8101/api/v1{path}"),
            read_pattern: "KeyLookup".to_string(),
            risk_tier: "low".to_string(),
            samples: vec![Sample {
                latency_ms: p95,
                status: if error_rate > 0.0 { error_status } else { 200 },
                body_size: 1024,
                error: if error_rate > 0.0 {
                    Some(format!("expected status 200, got {error_status}"))
                } else {
                    None
                },
            }],
            metrics: ComputedMetrics {
                p50_ms: p95 * 0.8,
                p95_ms: p95,
                p99_ms: p95 * 1.1,
                min_ms: p95 * 0.5,
                max_ms: p95 * 1.2,
                mean_ms: p95 * 0.9,
                std_dev_ms: 1.0,
                error_rate,
                avg_body_size: 1024,
                throughput_rps: 100.0,
            },
            wall_clock: Duration::from_millis(100),
            skipped,
            skip_reason: if skipped {
                Some("no params".to_string())
            } else {
                None
            },
        }
    }

    #[test]
    fn test_build_report_counts() {
        let results = vec![
            make_result("blocks", "GET", "/blocks/{number}", 50.0, 0.0, false),
            make_result("blocks", "GET", "/blocks/latest", 150.0, 0.0, false),
            make_result("txs", "GET", "/txs/{hash}", 600.0, 0.0, false),
            make_result("dao", "GET", "/dao/stats", 80.0, 0.05, false),
            make_result("fiber", "GET", "/fiber/channels", 0.0, 0.0, true),
        ];

        let report = build_report(&results, "http://localhost:8101/api/v1", 10, 1, 2);

        assert_eq!(report.summary.tested, 4);
        assert_eq!(report.summary.skipped, 1);
        assert_eq!(report.summary.slow_count, 2); // 150ms and 600ms both > 100ms
        assert_eq!(report.summary.very_slow_count, 1); // 600ms > 500ms
        assert_eq!(report.summary.error_count, 1); // dao has errors
        assert_eq!(report.results.len(), 5);
    }

    #[test]
    fn test_build_report_empty() {
        let report = build_report(&[], "http://localhost:8101/api/v1", 10, 1, 2);
        assert_eq!(report.summary.tested, 0);
        assert_eq!(report.summary.skipped, 0);
        assert_eq!(report.results.len(), 0);
    }

    #[test]
    fn test_entry_flag() {
        let mut entry = entry_from_result(&make_result("m", "GET", "/p", 50.0, 0.0, false));
        assert_eq!(entry_flag(&entry), "");

        entry.p95_ms = 150.0;
        assert_eq!(entry_flag(&entry), "SLOW");

        entry.p95_ms = 600.0;
        assert_eq!(entry_flag(&entry), "VERY SLOW");

        // Error with status code
        let entry_404 = entry_from_result(&make_result_with_status(
            "m", "GET", "/p", 50.0, 1.0, false, 404,
        ));
        assert_eq!(entry_flag(&entry_404), "ERRORS(404)");

        let entry_500 = entry_from_result(&make_result_with_status(
            "m", "GET", "/p", 50.0, 1.0, false, 500,
        ));
        assert_eq!(entry_flag(&entry_500), "ERRORS(500)");

        entry.skipped = true;
        assert_eq!(entry_flag(&entry), "SKIPPED");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a long string", 10), "this is...");
        assert_eq!(truncate("exact_len!", 10), "exact_len!");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500B");
        assert_eq!(format_size(1500), "1.5KB");
        assert_eq!(format_size(2_500_000), "2.5MB");
    }

    #[test]
    fn test_json_roundtrip() {
        let results = vec![
            make_result("blocks", "GET", "/blocks/{number}", 50.0, 0.0, false),
            make_result("fiber", "GET", "/fiber/channels", 0.0, 0.0, true),
        ];
        let report = build_report(&results, "http://localhost:8101/api/v1", 10, 1, 2);

        let json = serde_json::to_string_pretty(&report).unwrap();
        let deserialized: BenchmarkReport = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.results.len(), 2);
        assert_eq!(deserialized.summary.tested, 1);
        assert_eq!(deserialized.summary.skipped, 1);
        assert_eq!(deserialized.config.iterations, 10);
    }

    #[test]
    fn test_error_statuses_in_report() {
        let results = vec![
            make_result_with_status("scripts", "GET", "/scripts/{name}", 10.0, 1.0, false, 404),
            make_result_with_status("dao", "GET", "/dao/calculator", 10.0, 1.0, false, 500),
            make_result("blocks", "GET", "/blocks/latest", 10.0, 0.0, false),
        ];
        let report = build_report(&results, "http://localhost:8101/api/v1", 10, 1, 2);

        // scripts entry should have 404 in error_statuses
        let scripts = &report.results[0];
        assert_eq!(scripts.error_statuses.get(&404), Some(&1));
        assert!(!scripts.error_statuses.contains_key(&200));

        // dao entry should have 500
        let dao = &report.results[1];
        assert_eq!(dao.error_statuses.get(&500), Some(&1));

        // blocks entry should have empty error_statuses
        let blocks = &report.results[2];
        assert!(blocks.error_statuses.is_empty());
    }
}
