mod discovery;
mod endpoints;
mod metrics;
mod registry;
mod report;
mod runner;
mod stress;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::discovery::{check_connectivity, print_discovery, run_discovery};
use crate::registry::RiskTier;

#[derive(Parser)]
#[command(name = "ckbadger-bench", about = "API performance benchmark")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    bench_args: BenchArgs,
}

#[derive(Subcommand)]
enum Commands {
    /// Run per-endpoint benchmarks (default when no subcommand given)
    Bench(BenchArgs),
    /// Run multi-stage stress tests
    Stress(stress::StressArgs),
}

#[derive(Debug, Clone, Parser)]
pub struct BenchArgs {
    /// API base URL
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    api_url: String,

    /// Frontend URL (for /capabilities)
    #[arg(long, default_value = "http://localhost:8100")]
    frontend_url: String,

    /// Requests per endpoint
    #[arg(long, default_value = "10")]
    iterations: u32,

    /// Concurrent requests
    #[arg(long, default_value = "1")]
    concurrency: u32,

    /// Warmup requests (not measured)
    #[arg(long, default_value = "2")]
    warmup: u32,

    /// Per-request timeout in milliseconds
    #[arg(long, default_value = "10000")]
    timeout_ms: u64,

    /// Filter by module name
    #[arg(long)]
    module: Option<String>,

    /// Filter by endpoint path template
    #[arg(long)]
    endpoint: Option<String>,

    /// Filter by risk tier (high, medium, low)
    #[arg(long)]
    risk: Option<String>,

    /// Output JSON instead of table
    #[arg(long)]
    json: bool,

    /// Save JSON output to file
    #[arg(long)]
    output: Option<String>,

    /// Directory for auto-saved timestamped reports
    #[arg(long)]
    output_dir: Option<PathBuf>,

    /// Compare against baseline JSON file
    #[arg(long)]
    compare: Option<String>,

    /// Print discovered params and exit
    #[arg(long)]
    discovery_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Stress(args)) => stress::run_stress(args).await,
        Some(Commands::Bench(args)) => run_bench(args).await,
        None => run_bench(cli.bench_args).await,
    }
}

async fn run_bench(cli: BenchArgs) -> Result<()> {
    println!("ckbadger-bench: api_url={}", cli.api_url);

    // Build HTTP client with configured timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cli.timeout_ms))
        .build()
        .context("failed to build HTTP client")?;

    // Connectivity check
    println!("Checking API connectivity...");
    check_connectivity(&cli.api_url, &client).await?;
    println!("API is reachable.");

    // Run discovery
    println!("Running discovery...");
    let discovery = run_discovery(&cli.api_url, &cli.frontend_url, &client).await?;

    if cli.discovery_only {
        print_discovery(&discovery);
        return Ok(());
    }

    // Build registry
    let mut registry = endpoints::register_all();
    let total_registered = registry.entries.len();

    // Apply filters
    if let Some(ref module) = cli.module {
        registry.filter_module(module);
    }
    if let Some(ref endpoint) = cli.endpoint {
        registry.filter_endpoint(endpoint);
    }
    if let Some(ref risk_str) = cli.risk {
        if let Some(tier) = RiskTier::from_str_opt(risk_str) {
            registry.filter_risk(tier);
        } else {
            bail!("Invalid risk tier: {risk_str}. Use: high, medium, low");
        }
    }

    registry.sort_by_risk();

    eprintln!(
        "Running {} of {} endpoints ({} iterations, concurrency {})...\n",
        registry.entries.len(),
        total_registered,
        cli.iterations,
        cli.concurrency,
    );

    let run_config = runner::RunConfig {
        iterations: cli.iterations,
        concurrency: cli.concurrency,
        warmup: cli.warmup,
    };

    // Execute benchmarks
    let mut results = Vec::new();
    let bench_start = Instant::now();

    for (i, entry) in registry.entries.iter().enumerate() {
        eprint!(
            "[{}/{}] {} {} {} ...",
            i + 1,
            registry.entries.len(),
            entry.module,
            entry.method,
            entry.path_template,
        );

        let result =
            runner::bench_endpoint(&client, entry, &cli.api_url, &discovery.params, &run_config)
                .await?;

        if result.skipped {
            eprintln!(" SKIPPED");
        } else if result.metrics.error_rate > 0.0 {
            // Show error status codes in progress line
            let mut statuses = std::collections::BTreeMap::new();
            for s in &result.samples {
                if s.error.is_some() {
                    *statuses.entry(s.status).or_insert(0u32) += 1;
                }
            }
            let codes: Vec<String> = statuses
                .iter()
                .map(|(code, count)| format!("{code}x{count}"))
                .collect();
            eprintln!(
                " p95={:.0}ms ERRORS({})",
                result.metrics.p95_ms,
                codes.join(",")
            );
        } else {
            eprintln!(" p95={:.0}ms", result.metrics.p95_ms);
        }

        results.push(result);
    }

    let total_time = bench_start.elapsed();
    eprintln!("\nBenchmark complete in {:.1}s", total_time.as_secs_f64());

    // Build report
    let bench_report = report::build_report(
        &results,
        &cli.api_url,
        cli.iterations,
        cli.concurrency,
        cli.warmup,
    );

    // Output
    if cli.json {
        report::print_json(&bench_report)?;
    } else {
        report::print_table(&bench_report);
    }

    // Auto-save timestamped report to output_dir
    if let Some(ref dir) = cli.output_dir {
        let timestamp = bench_report.timestamp.replace(':', "-").replace('+', "p");
        let auto_path = dir.join(format!("{timestamp}.json"));
        report::save_json(&bench_report, &auto_path)?;
    }

    if let Some(ref path) = cli.output {
        report::save_json(&bench_report, path.as_ref())?;
    }

    if let Some(ref baseline_path) = cli.compare {
        report::compare_reports(&bench_report, baseline_path)?;
    }

    Ok(())
}
