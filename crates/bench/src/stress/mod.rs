pub mod collector;
pub mod report;
pub mod scenario;
pub mod vu;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::Args;
use tokio_util::sync::CancellationToken;

use crate::discovery::{check_connectivity, run_discovery};
use crate::endpoints;
use crate::metrics::percentile;

use self::collector::{
    detect_degradation, drain_samples, sample_channel, DegradationSignal, StageResult, StageStatus,
    StatusLine,
};
use self::report::{ScenarioReport, StressConfig, StressReport};
use self::scenario::{build_frontend_group, build_heavy_groups, build_mixed_groups, Scenario};
use self::vu::{resolve_all, resolve_all_with_frontend, spawn_vu, ResolvedTarget};

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

/// CLI arguments for the stress subcommand.
#[derive(Debug, Args)]
pub struct StressArgs {
    /// Scenario to run: "mixed", "heavy", or "mixed,heavy"
    #[arg(long, default_value = "mixed")]
    pub scenario: String,

    /// Comma-separated VU counts per stage (e.g. "10,25,50,100,200,300")
    #[arg(long, default_value = "10,25,50,100,200,300")]
    pub stages: String,

    /// Duration of each stage in seconds
    #[arg(long, default_value = "30")]
    pub stage_duration: u64,

    /// Auto-ramp: start at first stage value, double until breakpoint
    #[arg(long)]
    pub auto_ramp: bool,

    /// Think-time range in ms (e.g. "50-200")
    #[arg(long, default_value = "50-200")]
    pub think_time_ms: String,

    /// Remote host (overrides api_url and frontend_url)
    #[arg(long)]
    pub remote_host: Option<String>,

    /// API base URL
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    pub api_url: String,

    /// Frontend base URL
    #[arg(long, default_value = "http://localhost:8100")]
    pub frontend_url: String,

    /// Per-request timeout in milliseconds
    #[arg(long, default_value = "10000")]
    pub timeout_ms: u64,

    /// Warmup duration in seconds (1 VU, samples discarded)
    #[arg(long, default_value = "5")]
    pub warmup_duration: u64,

    /// Output JSON instead of tables
    #[arg(long)]
    pub json: bool,

    /// Save JSON report to this file
    #[arg(long)]
    pub output: Option<String>,

    /// Directory for auto-saved timestamped JSON reports
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Parse a comma-separated list of VU counts.
pub fn parse_stages(s: &str) -> Vec<u32> {
    s.split(',')
        .filter_map(|part| part.trim().parse::<u32>().ok())
        .collect()
}

/// Generate an auto-ramp sequence: start, start*2, start*4, ... up to 10000.
/// Returns at most 20 entries.
pub fn auto_ramp_sequence(start: u32) -> Vec<u32> {
    let mut stages = Vec::new();
    let mut current = start;
    while current <= 10_000 && stages.len() < 20 {
        stages.push(current);
        current = current.saturating_mul(2);
    }
    stages
}

/// Parse think-time range from "min-max" format.
fn parse_think_time(s: &str) -> Result<(u64, u64)> {
    if let Some((min_s, max_s)) = s.split_once('-') {
        let min: u64 = min_s
            .trim()
            .parse()
            .with_context(|| format!("invalid think-time min: {min_s:?}"))?;
        let max: u64 = max_s
            .trim()
            .parse()
            .with_context(|| format!("invalid think-time max: {max_s:?}"))?;
        if min > max {
            bail!("think-time min ({min}) > max ({max})");
        }
        Ok((min, max))
    } else {
        let ms: u64 = s
            .trim()
            .parse()
            .with_context(|| format!("invalid think-time: {s:?}"))?;
        Ok((ms, ms))
    }
}

/// Resolve target URLs from optional remote_host or explicit URLs.
pub fn resolve_target(
    remote_host: Option<&str>,
    api_url: &str,
    frontend_url: &str,
) -> (String, String) {
    match remote_host {
        Some(host) => {
            let api = format!("http://{}:8101/api/v1", host);
            let frontend = format!("http://{}:8100", host);
            (api, frontend)
        }
        None => (api_url.to_string(), frontend_url.to_string()),
    }
}

/// Label the target for display: "local" or "remote (host:port)".
pub fn target_label(api_url: &str) -> String {
    // Extract the host from the URL
    if let Some(after_scheme) = api_url
        .strip_prefix("http://")
        .or_else(|| api_url.strip_prefix("https://"))
    {
        let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
        let host = host_port.split(':').next().unwrap_or(host_port);
        if host == "localhost" || host == "127.0.0.1" {
            return "local".to_string();
        }
        return format!("remote ({host_port})");
    }
    "local".to_string()
}

// ---------------------------------------------------------------------------
// Main orchestrator
// ---------------------------------------------------------------------------

/// Run multi-stage stress tests against the API.
pub async fn run_stress(args: StressArgs) -> Result<()> {
    // 1. Validate mutual exclusion of remote_host and non-default URLs
    let default_api = "http://localhost:8101/api/v1";
    let default_frontend = "http://localhost:8100";

    if args.remote_host.is_some()
        && (args.api_url != default_api || args.frontend_url != default_frontend)
    {
        bail!("--remote-host cannot be combined with --api-url or --frontend-url");
    }

    // 2. Resolve target URLs
    let (api_url, frontend_url) = resolve_target(
        args.remote_host.as_deref(),
        &args.api_url,
        &args.frontend_url,
    );
    let label = target_label(&api_url);

    // 3. Parse scenarios, stages, think_time
    let scenarios = Scenario::parse(&args.scenario)?;
    let stages = if args.auto_ramp {
        let first = parse_stages(&args.stages).first().copied().unwrap_or(10);
        auto_ramp_sequence(first)
    } else {
        parse_stages(&args.stages)
    };
    if stages.is_empty() {
        bail!("no valid stage VU counts parsed from --stages");
    }
    let think_time = parse_think_time(&args.think_time_ms)?;

    eprintln!("ckbadger stress: target={label}  api={api_url}");
    eprintln!(
        "  scenarios={:?}  stages={:?}  stage_duration={}s  think_time={}-{}ms",
        scenarios, stages, args.stage_duration, think_time.0, think_time.1
    );

    // 4. Build HTTP client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(args.timeout_ms))
        .pool_max_idle_per_host(500)
        .build()
        .context("failed to build HTTP client")?;

    // 5. Connectivity check + discovery
    eprintln!("Checking API connectivity...");
    check_connectivity(&api_url, &client).await?;
    eprintln!("API is reachable.");

    eprintln!("Running discovery...");
    let discovery = run_discovery(&api_url, &frontend_url, &client).await?;
    eprintln!(
        "Discovery complete: {} routes, params discovered.",
        discovery.capabilities_route_count
    );

    // 6. Build registry and pre-resolve API endpoints for counting
    let registry = endpoints::register_all();
    let api_resolved = resolve_all(&registry.entries, &api_url, &discovery.params);
    let resolved_count = api_resolved.len();
    eprintln!(
        "Resolved {resolved_count}/{} endpoints for stress testing.",
        registry.entries.len()
    );

    if resolved_count == 0 {
        bail!("no endpoints could be resolved — check discovery results");
    }

    drop(api_resolved); // freed — per-scenario resolution below
    let client = Arc::new(client);

    // 7. Run each scenario
    let mut scenario_reports = Vec::new();

    for scenario in &scenarios {
        eprintln!("\n{}", "=".repeat(60));
        eprintln!("Scenario: {scenario:?}");
        eprintln!("{}", "=".repeat(60));

        // Per-scenario endpoint resolution and group building
        let (resolved_targets, groups) = match scenario {
            Scenario::Mixed => {
                let targets = resolve_all_with_frontend(
                    &registry.entries,
                    &api_url,
                    &frontend_url,
                    &discovery.params,
                );
                let mut groups = build_mixed_groups(&registry.entries);
                groups.push(build_frontend_group(registry.entries.len()));
                (targets, groups)
            }
            Scenario::Heavy | Scenario::Api => {
                let targets: Vec<ResolvedTarget> =
                    resolve_all(&registry.entries, &api_url, &discovery.params)
                        .into_iter()
                        .map(ResolvedTarget::Api)
                        .collect();
                let groups = match scenario {
                    Scenario::Heavy => build_heavy_groups(&registry.entries),
                    Scenario::Api => scenario::build_api_group(&registry.entries),
                    _ => unreachable!(),
                };
                (targets, groups)
            }
        };

        if groups.is_empty() {
            eprintln!("  (no endpoint groups for {scenario:?}, skipping)");
            continue;
        }

        let resolved_targets = Arc::new(resolved_targets);
        let groups = Arc::new(groups);

        // Warmup: 1 VU for warmup_duration, discard all samples
        if args.warmup_duration > 0 {
            eprintln!("  Warming up ({} VU, {}s)...", 1, args.warmup_duration);
            let (warmup_tx, mut warmup_rx) = sample_channel();
            let cancel = CancellationToken::new();

            let handle = spawn_vu(
                Arc::clone(&client),
                Arc::clone(&resolved_targets),
                Arc::clone(&groups),
                warmup_tx,
                Some(think_time),
                cancel.clone(),
            );

            tokio::time::sleep(Duration::from_secs(args.warmup_duration)).await;
            cancel.cancel();
            let _ = handle.await;

            // Discard warmup samples
            let discarded = drain_samples(&mut warmup_rx).len();
            eprintln!("  Warmup done ({discarded} samples discarded).");
        }

        // Run stages
        let mut stage_results: Vec<StageResult> = Vec::new();
        let mut baseline: Option<StageResult> = None;
        let mut soft_degradation_vus: Option<u32> = None;
        let mut breaking_point_vus: Option<u32> = None;

        for (stage_idx, &vu_count) in stages.iter().enumerate() {
            eprintln!();
            eprintln!(
                "  Stage {}/{}: {} VUs for {}s",
                stage_idx + 1,
                stages.len(),
                vu_count,
                args.stage_duration
            );

            let (tx, mut rx) = sample_channel();
            let cancel = CancellationToken::new();

            // Spawn VUs
            let mut handles = Vec::new();
            for _ in 0..vu_count {
                // Only Mixed scenario gets think_time
                let vu_think = match scenario {
                    Scenario::Mixed => Some(think_time),
                    Scenario::Heavy | Scenario::Api => None,
                };
                let handle = spawn_vu(
                    Arc::clone(&client),
                    Arc::clone(&resolved_targets),
                    Arc::clone(&groups),
                    tx.clone(),
                    vu_think,
                    cancel.clone(),
                );
                handles.push(handle);
            }
            // Drop our copy of the sender so the channel closes when VUs finish
            drop(tx);

            // Stage duration loop: sleep 1s, drain samples, compute rolling stats
            let stage_start = Instant::now();
            let mut all_samples = Vec::new();

            for tick in 1..=args.stage_duration {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let batch = drain_samples(&mut rx);
                all_samples.extend(batch);

                let elapsed = stage_start.elapsed().as_secs_f64();
                let rps = if elapsed > 0.0 {
                    all_samples.len() as f64 / elapsed
                } else {
                    0.0
                };
                let mut latencies: Vec<f64> = all_samples.iter().map(|s| s.latency_ms).collect();
                latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let p95 = percentile(&latencies, 95.0);
                let errors = all_samples.iter().filter(|s| s.error.is_some()).count();
                let error_pct = if all_samples.is_empty() {
                    0.0
                } else {
                    errors as f64 / all_samples.len() as f64 * 100.0
                };

                let status_line = StatusLine {
                    current_stage: stage_idx + 1,
                    total_stages: stages.len(),
                    vus: vu_count as usize,
                    elapsed_secs: tick as f64,
                    duration_secs: args.stage_duration as f64,
                    rps,
                    p95_ms: p95,
                    error_pct,
                };
                status_line.print();
            }

            // Cancel VUs and join
            cancel.cancel();
            for handle in handles {
                let _ = handle.await;
            }

            // Drain remaining samples
            let remaining = drain_samples(&mut rx);
            all_samples.extend(remaining);

            // Clear status line
            eprint!("\r{:>80}\r", "");

            let stage_duration = stage_start.elapsed();

            // Compute stage result
            let mut result = StageResult::from_samples(
                stage_idx,
                vu_count as usize,
                stage_duration,
                &all_samples,
            );

            // Set stage status
            if stage_idx == 0 {
                result.status = StageStatus::Baseline;
                baseline = Some(result.clone());
            } else if let Some(ref bl) = baseline {
                let signal = detect_degradation(bl, &result);
                match signal {
                    DegradationSignal::None => result.status = StageStatus::Ok,
                    DegradationSignal::SoftDegradation => {
                        result.status = StageStatus::SoftDegradation;
                        if soft_degradation_vus.is_none() {
                            soft_degradation_vus = Some(vu_count);
                        }
                    }
                    DegradationSignal::ErrorsEmerging => {
                        result.status = StageStatus::ErrorsRising;
                        if soft_degradation_vus.is_none() {
                            soft_degradation_vus = Some(vu_count);
                        }
                    }
                    DegradationSignal::HardFailure => {
                        result.status = StageStatus::HardFailure;
                        if breaking_point_vus.is_none() {
                            breaking_point_vus = Some(vu_count);
                        }
                    }
                }
            }

            eprintln!(
                "  -> {} reqs, {:.0} rps, p50={:.0}ms p95={:.0}ms p99={:.0}ms, err={:.1}%, status={:?}",
                result.total_requests,
                result.rps,
                result.p50_ms,
                result.p95_ms,
                result.p99_ms,
                result.error_rate * 100.0,
                result.status,
            );

            let should_break = args.auto_ramp && result.status == StageStatus::HardFailure;

            stage_results.push(result);

            if should_break {
                eprintln!("  Auto-ramp: hard failure detected, stopping.");
                break;
            }
        }

        scenario_reports.push(ScenarioReport {
            scenario: *scenario,
            stage_results,
            soft_degradation_vus,
            breaking_point_vus,
        });
    }

    // 8. Build and output report
    let stress_report = StressReport {
        timestamp: Utc::now().to_rfc3339(),
        target: label,
        config: StressConfig {
            scenarios: args.scenario.clone(),
            stage_duration_secs: args.stage_duration,
            auto_ramp: args.auto_ramp,
            think_time_ms: args.think_time_ms.clone(),
            timeout_ms: args.timeout_ms,
        },
        scenarios: scenario_reports,
    };

    eprintln!();
    if args.json {
        report::print_json(&stress_report)?;
    } else {
        report::print_tables(&stress_report);
    }

    if let Some(ref dir) = args.output_dir {
        let timestamp = stress_report.timestamp.replace(':', "-").replace('+', "p");
        let auto_path = dir.join(format!("stress-{timestamp}.json"));
        report::save_json(&stress_report, &auto_path)?;
    }

    if let Some(ref path) = args.output {
        report::save_json(&stress_report, path.as_ref())?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stages_default() {
        let stages = parse_stages("10,25,50,100,200,300");
        assert_eq!(stages, vec![10, 25, 50, 100, 200, 300]);
    }

    #[test]
    fn test_parse_stages_custom() {
        let stages = parse_stages("5,10,20");
        assert_eq!(stages, vec![5, 10, 20]);
    }

    #[test]
    fn test_auto_ramp_sequence() {
        let seq = auto_ramp_sequence(10);
        // 10, 20, 40, 80, 160, 320, 640, 1280, 2560, 5120
        assert!(
            seq.len() >= 6,
            "expected at least 6 entries, got {}",
            seq.len()
        );
        assert_eq!(seq[0], 10);
        assert_eq!(seq[1], 20);
        assert_eq!(seq[2], 40);
        // All entries <= 10000
        for &v in &seq {
            assert!(v <= 10_000);
        }
        // Each entry is double the previous
        for i in 1..seq.len() {
            assert_eq!(seq[i], seq[i - 1] * 2);
        }
    }

    #[test]
    fn test_resolve_target_local() {
        let (api, frontend) = resolve_target(
            None,
            "http://localhost:8101/api/v1",
            "http://localhost:8100",
        );
        assert_eq!(api, "http://localhost:8101/api/v1");
        assert_eq!(frontend, "http://localhost:8100");
    }

    #[test]
    fn test_resolve_target_remote() {
        let (api, frontend) = resolve_target(
            Some("192.168.1.100"),
            "http://localhost:8101/api/v1",
            "http://localhost:8100",
        );
        assert_eq!(api, "http://192.168.1.100:8101/api/v1");
        assert_eq!(frontend, "http://192.168.1.100:8100");
    }

    #[test]
    fn test_target_label_local() {
        assert_eq!(target_label("http://localhost:8101/api/v1"), "local");
        assert_eq!(target_label("http://127.0.0.1:8101/api/v1"), "local");
    }

    #[test]
    fn test_target_label_remote() {
        let label = target_label("http://192.168.1.100:8101/api/v1");
        assert_eq!(label, "remote (192.168.1.100:8101)");
    }
}
