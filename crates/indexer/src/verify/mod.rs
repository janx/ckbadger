//! Verification/acceptance testing suite for ckbadger data integrity.
//!
//! Provides a `verify` CLI subcommand that validates data via the ckbadger REST API,
//! spot-checks against a CKB RPC node, and compares against the official explorer.

pub mod api_checks;
pub mod checks;
pub mod explorer;
pub mod report;
pub mod sampling;

use std::path::PathBuf;
use std::time::Instant;

use indicatif::MultiProgress;

use checks::{execute_check, Check, CheckContext, CheckTier, CompletedCheck, ProgressReporter};

/// CLI arguments for the verify subcommand.
#[derive(clap::Args, Debug)]
pub struct VerifyArgs {
    /// CKB network whose data is being verified.
    #[arg(long, default_value = "mainnet")]
    pub network: String,

    /// ckbadger API base URL.
    #[arg(long, default_value = "http://localhost:3001/api/v1")]
    pub api_url: String,

    /// CKB RPC URL for spot-checks.
    #[arg(long)]
    pub rpc_url: Option<String>,

    /// Official explorer API URL.
    #[arg(long, default_value = "https://mainnet-api.explorer.nervos.org")]
    pub explorer_url: String,

    /// Skip official explorer comparison checks.
    #[arg(long)]
    pub no_explorer: bool,

    /// Check depth tier.
    #[arg(long, default_value = "fast", value_parser = parse_depth)]
    pub depth: CheckTier,

    /// Number of samples for sampling tier.
    #[arg(long, default_value = "1000")]
    pub sample_count: usize,

    /// Deterministic seed for reproducibility.
    #[arg(long, default_value = "42")]
    pub seed: u64,

    /// Max allowed deviation from explorer data (as fraction, e.g. 0.001 = 0.1%).
    #[arg(long, default_value = "0.001")]
    pub tolerance: f64,

    /// Output format.
    #[arg(long, default_value = "text", value_parser = parse_format)]
    pub format: OutputFormat,

    /// Run specific checks only (comma-separated names).
    #[arg(long, value_delimiter = ',')]
    pub checks: Option<Vec<String>>,

    /// List available checks and exit.
    #[arg(long)]
    pub list_checks: bool,

    /// Directory for caching explorer API responses.
    #[arg(long)]
    pub cache_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
}

fn parse_depth(s: &str) -> Result<CheckTier, String> {
    match s.to_lowercase().as_str() {
        "fast" => Ok(CheckTier::Fast),
        "sampling" => Ok(CheckTier::Sampling),
        _ => Err(format!("Invalid depth: {}. Use fast or sampling", s)),
    }
}

fn parse_format(s: &str) -> Result<OutputFormat, String> {
    match s.to_lowercase().as_str() {
        "text" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        _ => Err(format!("Invalid format: {}. Use text or json", s)),
    }
}

/// Collect all registered checks.
fn all_checks() -> Vec<Box<dyn Check>> {
    let mut checks: Vec<Box<dyn Check>> = Vec::new();
    checks.extend(api_checks::api_checks());
    checks.extend(explorer::explorer_checks());
    checks
}

/// Main entry point for the verify subcommand.
pub fn run(args: VerifyArgs) -> anyhow::Result<()> {
    let all = all_checks();

    // Handle --list-checks
    if args.list_checks {
        let check_info: Vec<(String, String, String)> = all
            .iter()
            .map(|c| {
                (
                    c.name().to_string(),
                    c.tier().to_string(),
                    c.description().to_string(),
                )
            })
            .collect();
        report::print_check_list(&check_info);
        return Ok(());
    }

    let explorer_url = if args.no_explorer {
        None
    } else {
        Some(args.explorer_url.clone())
    };
    let network = ckbadger_common::hardfork::normalize_network(&args.network)
        .ok_or_else(|| anyhow::anyhow!("unsupported verify network '{}'", args.network))?;

    let cache_dir = args.cache_dir.as_ref().map(PathBuf::from).or_else(|| {
        // Default cache dir next to working directory
        Some(PathBuf::from(".verify-cache"))
    });

    let ctx = CheckContext {
        network,
        api_url: args.api_url.clone(),
        rpc_url: args.rpc_url.clone(),
        explorer_url,
        http: reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?,
        sample_count: args.sample_count,
        seed: args.seed,
        tolerance: args.tolerance,
        cache_dir,
    };

    // Filter checks by tier and name
    let checks_to_run: Vec<&dyn Check> = all
        .iter()
        .filter(|c| {
            // Filter by tier
            if c.tier() > args.depth && !c.requires_explorer() {
                return false;
            }
            // Explorer checks run at sampling tier
            if c.requires_explorer() && args.depth < CheckTier::Sampling {
                return false;
            }
            // Filter by name if --checks specified
            if let Some(ref names) = args.checks {
                return names.iter().any(|n| n == c.name());
            }
            true
        })
        .map(|c| c.as_ref())
        .collect();

    let is_json = args.format == OutputFormat::Json;
    let mp = if is_json {
        MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
    } else {
        MultiProgress::new()
    };

    if !is_json {
        report::print_header(
            &args.api_url,
            &args.depth.to_string(),
            args.seed,
            args.sample_count,
            args.rpc_url.as_deref(),
        );
    }

    let start = Instant::now();
    let mut results: Vec<CompletedCheck> = Vec::new();

    // Group checks by tier for section headers
    let mut current_tier: Option<CheckTier> = None;
    let mut in_explorer_section = false;

    for check in &checks_to_run {
        let tier = check.tier();
        let is_explorer = check.requires_explorer();

        // Print section headers
        if !is_json {
            if is_explorer && !in_explorer_section {
                report::print_explorer_header();
                in_explorer_section = true;
            } else if !is_explorer && (current_tier.is_none() || current_tier != Some(tier)) {
                report::print_tier_header(tier);
                current_tier = Some(tier);
            }
        }

        // Create progress bar or spinner
        let pb = match check.estimated_total(&ctx) {
            Some(total) if total > 1 => report::make_progress_bar(&mp, check.name(), total),
            _ => report::make_spinner(&mp, check.name()),
        };

        let progress = ProgressReporter::new(Some(pb.clone()));
        let completed = execute_check(*check, &ctx, &progress);

        report::finish_check(&pb, &completed);
        results.push(completed);
    }

    let total_duration = start.elapsed();

    if is_json {
        report::print_json_report(&results, total_duration);
    } else {
        report::print_failure_summary(&results);
        report::print_summary(&results, total_duration);
    }

    let has_failures = results.iter().any(|r| !r.passed);
    if has_failures {
        let failed_count = results.iter().filter(|r| !r.passed).count();
        anyhow::bail!(
            "verification failed: {} of {} checks did not pass",
            failed_count,
            results.len()
        );
    }

    Ok(())
}
