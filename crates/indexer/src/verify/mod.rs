//! Verification/acceptance testing suite for ckbadger data integrity.
//!
//! Provides a `verify` CLI subcommand that validates data via the ckbadger REST API,
//! spot-checks against a CKB RPC node, and compares against the official explorer.

pub mod api_checks;
pub mod checks;
pub mod entity_history;
pub mod explorer;
pub mod manifest;
pub mod report;
pub mod sampling;
pub mod source;

use std::path::PathBuf;
use std::time::Instant;

use indicatif::MultiProgress;

use checks::{
    execute_check, Check, CheckContext, CheckStatus, CheckTier, CompletedCheck, ProgressReporter,
};
pub use report::{VerifyReport, VerifyRunReport};

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

    /// Shared id for this run, so every network's report lands under one
    /// directory name and one envelope. Generated when absent.
    #[arg(long)]
    pub run_id: Option<String>,

    /// Root under which `<run-id>/report.json` is persisted for this network
    /// (production: `<network workdir>/perf/verify`). No file is written when
    /// absent.
    #[arg(long)]
    pub evidence_dir: Option<String>,

    /// Verify these entities exactly, as `<kind>:<id>` (repeatable), instead of
    /// the chain-derived checks' default candidates.
    #[arg(long = "entity", value_name = "KIND:ID")]
    pub entities: Vec<String>,

    /// The operator's history-source declaration for this network
    /// (production: `<network workdir>/verify-source.toml`).
    #[arg(long)]
    pub verify_source: Option<String>,
}

/// A verification run that did not end in `Pass`.
///
/// Carries the exit code the process must return, so the CLI's error mapping
/// has one source for `Fail (1) > Error/Inconclusive (2)` rather than
/// re-deriving it.
#[derive(Debug)]
pub struct VerifyOutcome {
    pub status: CheckStatus,
    pub exit_code: u8,
    pub summary: String,
}

impl std::fmt::Display for VerifyOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "verification {}: {}", self.status, self.summary)
    }
}

impl std::error::Error for VerifyOutcome {}

/// A fresh run id: sortable, and unique across concurrent runs.
pub fn new_run_id() -> String {
    format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
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
    checks.push(Box::new(entity_history::EntityCapacityHistoryMatchesChain));
    checks.extend(explorer::explorer_checks());
    checks
}

/// Whether a check runs at the requested depth. Explorer checks are gated on
/// their own rule rather than their tier, so this is the single definition both
/// the filter and `--checks` validation use.
fn runs_at_depth(check: &dyn Check, depth: CheckTier) -> bool {
    if check.requires_explorer() {
        return depth >= CheckTier::Sampling;
    }
    check.tier() <= depth
}

/// Reject a `--checks` selection that would run nothing, rather than reporting
/// an all-green run over an empty set. A typo or a too-shallow `--depth` is a
/// mistake in the request, not a passing verification.
fn validate_check_selection(
    all: &[Box<dyn Check>],
    selected: Option<&[String]>,
    depth: CheckTier,
) -> anyhow::Result<()> {
    let Some(selected) = selected else {
        return Ok(());
    };

    let unknown: Vec<&str> = selected
        .iter()
        .filter(|name| !all.iter().any(|c| c.name() == name.as_str()))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!(
            "unknown check name(s): {} — see --list-checks",
            unknown.join(", ")
        );
    }

    let excluded: Vec<String> = all
        .iter()
        .filter(|c| selected.iter().any(|name| name == c.name()))
        .filter(|c| !runs_at_depth(c.as_ref(), depth))
        .map(|c| format!("{} ({})", c.name(), c.tier()))
        .collect();
    if !excluded.is_empty() {
        anyhow::bail!(
            "--checks selected {} which do not run at --depth {}; raise the depth",
            excluded.join(", "),
            depth
        );
    }

    Ok(())
}

/// Print the check registry. Separate from [`run`], which always verifies.
pub fn list_checks() {
    let check_info: Vec<(String, String, String)> = all_checks()
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
}

/// Verify one network and return its structured report.
///
/// Never prints the report and never bails on a failing check: the caller owns
/// rendering and the exit code, so every selected network gets a report even
/// when an earlier one already failed. Live progress still goes to stderr.
pub fn run(args: VerifyArgs) -> anyhow::Result<VerifyReport> {
    let all = all_checks();

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

    let run_id = args.run_id.clone().unwrap_or_else(new_run_id);
    let evidence_root = args.evidence_dir.as_ref().map(PathBuf::from);
    let entities = args
        .entities
        .iter()
        .map(|raw| checks::EntitySelector::parse(raw))
        .collect::<Result<Vec<_>, String>>()
        .map_err(|error| anyhow::anyhow!("{error}"))?;

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
        entities,
        verify_source_path: args.verify_source.as_ref().map(PathBuf::from),
        evidence_dir: evidence_root
            .as_ref()
            .map(|root| root.join(&run_id))
            .clone(),
    };

    validate_check_selection(&all, args.checks.as_deref(), args.depth)?;

    // The declared scope is what `--depth` covers. Within it, `--checks` is an
    // explicit narrowing: the checks it leaves out are reported as skipped with
    // their reason, so a narrowed run is never described as a complete preset.
    let in_scope: Vec<&dyn Check> = all
        .iter()
        .filter(|c| runs_at_depth(c.as_ref(), args.depth))
        .map(|c| c.as_ref())
        .collect();
    let selected = |check: &dyn Check| {
        args.checks
            .as_ref()
            .is_none_or(|names| names.iter().any(|n| n == check.name()))
    };

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

    for check in &in_scope {
        if !selected(*check) {
            results.push(CompletedCheck::excluded(
                *check,
                "not selected by --checks".to_string(),
            ));
            continue;
        }

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

    let mut verify_report = VerifyReport::new(run_id, network, results, start.elapsed());

    // Persisting the evidence is part of the run: a report that could not be
    // written is an Error, not a silently unrecorded pass.
    if let Some(root) = evidence_root {
        verify_report.evidence_path = Some(
            report::report_path(&root, &verify_report.run_id)
                .to_string_lossy()
                .into_owned(),
        );
        match report::write_network_report(&root, &verify_report) {
            Ok(_) => {}
            Err(error) => {
                verify_report.evidence_path = None;
                let reason = format!("verify report could not be persisted: {error:#}");
                verify_report.checks.push(CompletedCheck {
                    name: "report_persistence",
                    description: "the run's report is written to disk before it is reported",
                    tier: "fast".to_string(),
                    status: CheckStatus::Error,
                    status_reason: Some(reason.clone()),
                    duration_ms: 0,
                    result: Some(checks::CheckResult::error(reason)),
                });
                verify_report.refresh();
            }
        }
    }

    Ok(verify_report)
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn first_named(pick: impl Fn(&dyn Check) -> bool) -> String {
        all_checks()
            .iter()
            .find(|c| pick(c.as_ref()))
            .map(|c| c.name().to_string())
            .expect("registry should contain such a check")
    }

    #[test]
    fn no_selection_runs_the_whole_tier() {
        validate_check_selection(&all_checks(), None, CheckTier::Fast).unwrap();
    }

    #[test]
    fn a_selected_check_of_the_requested_tier_is_accepted() {
        let fast = first_named(|c| c.tier() == CheckTier::Fast && !c.requires_explorer());
        validate_check_selection(&all_checks(), Some(&[fast]), CheckTier::Fast).unwrap();
    }

    #[test]
    fn an_unknown_check_name_is_rejected() {
        let error = validate_check_selection(
            &all_checks(),
            Some(&["dao_status_index_matches_depsoits".to_string()]),
            CheckTier::Sampling,
        )
        .expect_err("a typo must not report an all-green run over zero checks");
        let message = error.to_string();
        assert!(message.contains("unknown check name"), "{message}");
        assert!(
            message.contains("dao_status_index_matches_depsoits"),
            "{message}"
        );
    }

    #[test]
    fn a_selected_check_above_the_requested_depth_is_rejected() {
        let sampling = first_named(|c| c.tier() == CheckTier::Sampling && !c.requires_explorer());
        let error = validate_check_selection(
            &all_checks(),
            Some(std::slice::from_ref(&sampling)),
            CheckTier::Fast,
        )
        .expect_err("selecting a sampling check at fast depth must not run nothing");
        let message = error.to_string();
        assert!(message.contains(&sampling), "{message}");
        assert!(message.contains("raise the depth"), "{message}");
    }

    #[test]
    fn explorer_checks_are_gated_on_sampling_depth_not_their_tier() {
        let explorer_check = all_checks()
            .into_iter()
            .find(|c| c.requires_explorer())
            .expect("registry should contain explorer checks");
        assert!(!runs_at_depth(explorer_check.as_ref(), CheckTier::Fast));
        assert!(runs_at_depth(explorer_check.as_ref(), CheckTier::Sampling));
    }
}
