//! Terminal rendering (text + JSON output modes) for verification results.

use std::time::Duration;

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use super::checks::{CheckResult, CheckStatus, CheckTier, CompletedCheck};

/// Create a spinner progress bar for a fast check.
pub fn make_spinner(mp: &MultiProgress, name: &str) -> ProgressBar {
    let pb = mp.add(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(name.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Create a progress bar for a sampling check with known total.
pub fn make_progress_bar(mp: &MultiProgress, name: &str, total: u64) -> ProgressBar {
    let pb = mp.add(ProgressBar::new(total));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} {msg:<36} [{bar:40.cyan/dim}] {pos}/{len}  ETA {eta}")
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb.set_message(name.to_string());
    pb.enable_steady_tick(Duration::from_millis(200));
    pb
}

/// Format a duration in a human-readable compact way.
fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let secs = ms / 1000;
        let mins = secs / 60;
        let remainder = secs % 60;
        format!("{}m {}s", mins, remainder)
    }
}

/// Replace a spinner/progress bar with the final result line.
///
/// One glyph per status, so the terminal never renders a non-verified check
/// the same way it renders a verified one.
pub fn finish_check(pb: &ProgressBar, completed: &CompletedCheck) {
    let duration = format_duration(Duration::from_millis(completed.duration_ms));
    let reason = completed
        .status_reason
        .as_deref()
        .or_else(|| completed.result.as_ref().and_then(|r| r.detail.as_deref()));

    let glyph = match completed.status {
        CheckStatus::Pass => style("✓").green().bold(),
        CheckStatus::Fail => style("✗").red().bold(),
        CheckStatus::Inconclusive => style("?").yellow().bold(),
        CheckStatus::Skipped => style("⊘").yellow(),
        CheckStatus::NotApplicable => style("–").dim(),
        CheckStatus::Error => style("!").red().bold(),
    };

    let suffix = match (completed.status, reason) {
        (CheckStatus::Pass, Some(detail)) => format!("\n    {}", style(detail).dim()),
        (CheckStatus::Fail, _) => String::new(),
        (_, Some(reason)) => format!("  ({})", style(reason).yellow()),
        (_, None) => String::new(),
    };

    pb.finish_with_message(format!(
        "{} {:<40} {} [{}]{}",
        glyph,
        completed.name,
        style(&duration).dim(),
        completed.status,
        suffix,
    ));
}

/// Print a tier header.
pub fn print_tier_header(tier: CheckTier) {
    let label = match tier {
        CheckTier::Fast => "FAST CHECKS",
        CheckTier::Sampling => "SAMPLING CHECKS",
    };
    eprintln!("\n{}", style(label).bold().underlined());
}

/// Print the explorer section header.
pub fn print_explorer_header() {
    eprintln!("\n{}", style("EXPLORER COMPARISON").bold().underlined());
}

/// Render the detail block for every check that did not pass.
fn render_unresolved(report: &VerifyReport, out: &mut String) {
    use std::fmt::Write as _;

    let unresolved: Vec<&CompletedCheck> = report
        .checks
        .iter()
        .filter(|c| !matches!(c.status, CheckStatus::Pass | CheckStatus::NotApplicable))
        .collect();
    if unresolved.is_empty() {
        return;
    }

    let header = format!(
        " {} — UNRESOLVED CHECKS ({}) ",
        report.network.to_uppercase(),
        unresolved.len()
    );
    let width: usize = 60;
    let pad_total = width.saturating_sub(header.len());
    let pad_left = pad_total / 2;
    let pad_right = pad_total - pad_left;
    let _ = writeln!(
        out,
        "\n{}",
        style(format!(
            "{}{}{}",
            "━".repeat(pad_left),
            header,
            "━".repeat(pad_right)
        ))
        .red()
        .bold()
    );

    for (i, check) in unresolved.iter().enumerate() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  [{}] {}  {}",
            check.status,
            style(check.name).red().bold(),
            style(check.description).dim()
        );
        if let Some(reason) = check.status_reason.as_deref() {
            let _ = writeln!(out, "    {}", style(reason).yellow());
        }

        if let Some(ref result) = check.result {
            if result.findings.is_empty() {
                continue;
            }
            let count = result.findings.len();
            let noun = if count == 1 { "mismatch" } else { "mismatches" };
            let _ = writeln!(out, "    {} {} found:", style(count).red().bold(), noun);
            let _ = writeln!(out);
            for finding in result.findings.iter().take(10) {
                let _ = writeln!(out, "    {} {}", style("┌─").dim(), finding.entity);
                for detail in &finding.details {
                    let _ = writeln!(out, "    {}  {}", style("│").dim(), detail);
                }
                let _ = writeln!(out, "    {}", style("└─").dim());
            }
            if count > 10 {
                let _ = writeln!(out, "    {} ... and {} more", style("⋯").dim(), count - 10);
            }
        }

        if i < unresolved.len() - 1 {
            let _ = writeln!(out, "    {}", style("─".repeat(52)).dim());
        }
    }

    let _ = writeln!(out);
}

/// Render the whole run as text. Same statuses, same counts as [`render_json`].
pub fn render_text_summary(run: &VerifyRunReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    for report in &run.networks {
        render_unresolved(report, &mut out);
    }

    for report in &run.networks {
        let mut counts: Vec<(CheckStatus, usize)> = Vec::new();
        for status in [
            CheckStatus::Pass,
            CheckStatus::Fail,
            CheckStatus::Inconclusive,
            CheckStatus::Error,
            CheckStatus::Skipped,
            CheckStatus::NotApplicable,
        ] {
            let n = report.checks.iter().filter(|c| c.status == status).count();
            if n > 0 {
                counts.push((status, n));
            }
        }
        let breakdown = counts
            .iter()
            .map(|(status, n)| format!("{n} {status}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "  {}: {} — {} ({})",
            report.network,
            report.status,
            breakdown,
            format_duration(Duration::from_millis(report.duration_ms))
        );
        if let Some(path) = report.evidence_path.as_deref() {
            let _ = writeln!(out, "    report: {path}");
        }
    }

    let banner = format!(
        "  RESULT: {} (scopeComplete={}, runId={})",
        run.status, run.scope_complete, run.run_id
    );
    let bar = "━".repeat(banner.len() + 4);
    let styled = if run.status == CheckStatus::Pass {
        (style(&bar).green(), style(&banner).green().bold())
    } else {
        (style(&bar).red(), style(&banner).red().bold())
    };
    let _ = writeln!(out, "\n{}\n{}\n{}", styled.0, styled.1, styled.0);
    out
}

/// Print the initial header with API info.
pub fn print_header(api_url: &str, depth: &str, seed: u64, samples: usize, rpc_url: Option<&str>) {
    eprintln!();
    eprintln!("{}", style("ckbadger verify v0.2.0").bold());
    eprintln!("API:   {}", api_url);
    eprintln!("Depth: {} (seed: {}, {} samples)", depth, seed, samples);
    if let Some(rpc) = rpc_url {
        eprintln!("RPC:   {}", rpc);
    }
}

/// Format a number with commas.
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format a signed i128 number with commas.
pub fn format_number_i128(n: i128) -> String {
    let abs = n.unsigned_abs();
    let s = abs.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3 + 1);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    let mut formatted: String = result.chars().rev().collect();
    if n < 0 {
        formatted.insert(0, '-');
    }
    formatted
}

/// Version of the report envelope. Bump on any breaking field change.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// What the history source was proven to be, for the checks that need one.
///
/// Filled in by `verify::source`; `None` means no check in this run required a
/// qualified chain-history source.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProfileReport {
    /// `"qualified"` or `"inconclusive"`.
    pub status: String,
    /// Why the source is not qualified, when it is not.
    pub reason: Option<String>,
    pub genesis_hash: Option<String>,
    pub node_version: Option<String>,
    pub indexer_tip: Option<u64>,
    /// Operator declaration file this profile was read from.
    pub declaration_path: Option<String>,
    /// The operator's statement of how the index was built. It is the whole
    /// evidence for history the runtime cannot re-derive, so it is reported
    /// rather than left in a file nobody reads back.
    pub provenance: Option<String>,
    /// Declared block/cell filters. A filtered index can omit records, so an
    /// empty value here is part of what makes the source qualified.
    pub block_filter: Option<String>,
    pub cell_filter: Option<String>,
}

/// One network's verification result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub schema_version: u32,
    pub run_id: String,
    pub network: String,
    pub status: CheckStatus,
    /// Whether every check in the declared scope reached a verdict. Independent
    /// of `status`: a proven failure beside an unfinished case is
    /// `status=fail, scopeComplete=false`.
    pub scope_complete: bool,
    pub duration_ms: u64,
    pub checks: Vec<CompletedCheck>,
    pub source: Option<SourceProfileReport>,
    /// Where this report was persisted, once it has been written.
    pub evidence_path: Option<String>,
}

impl VerifyReport {
    pub fn new(
        run_id: impl Into<String>,
        network: impl Into<String>,
        checks: Vec<CompletedCheck>,
        duration: Duration,
    ) -> Self {
        let status = aggregate_status(checks.iter().map(|c| c.status));
        let scope_complete = !checks.is_empty() && checks.iter().all(|c| c.status.is_complete());
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            run_id: run_id.into(),
            network: network.into(),
            status,
            scope_complete,
            duration_ms: duration.as_millis() as u64,
            checks,
            source: None,
            evidence_path: None,
        }
    }

    /// A network whose verification could not start at all. It still appears in
    /// the envelope so "both networks were checked" never rests on silence.
    pub fn for_failed_start(
        run_id: impl Into<String>,
        network: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let reason = reason.into();
        let mut report = Self::new(run_id, network, Vec::new(), Duration::ZERO);
        report.status = CheckStatus::Error;
        report.scope_complete = false;
        report.checks = vec![CompletedCheck {
            name: "verify_run",
            description: "the verification run itself",
            tier: "fast".to_string(),
            status: CheckStatus::Error,
            status_reason: Some(reason.clone()),
            duration_ms: 0,
            result: Some(CheckResult::error(reason)),
        }];
        report
    }

    /// Recompute `status`/`scope_complete` after appending a check.
    pub fn refresh(&mut self) {
        self.status = aggregate_status(self.checks.iter().map(|c| c.status));
        self.scope_complete =
            !self.checks.is_empty() && self.checks.iter().all(|c| c.status.is_complete());
    }
}

/// The whole run, across every selected network — the one JSON document.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRunReport {
    pub schema_version: u32,
    pub run_id: String,
    pub status: CheckStatus,
    pub scope_complete: bool,
    pub networks: Vec<VerifyReport>,
}

impl VerifyRunReport {
    pub fn new(run_id: impl Into<String>, networks: Vec<VerifyReport>) -> Self {
        let status = aggregate_status(networks.iter().map(|n| n.status));
        let scope_complete = !networks.is_empty() && networks.iter().all(|n| n.scope_complete);
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            run_id: run_id.into(),
            status,
            scope_complete,
            networks,
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.status.exit_code()
    }
}

/// Merge statuses into the status of the thing that contains them.
///
/// `Fail` outranks everything — a proven inconsistency is not softened by an
/// unfinished case elsewhere. A scope in which nothing ran is `Inconclusive`,
/// never a pass over the empty set.
pub fn aggregate_status(statuses: impl Iterator<Item = CheckStatus>) -> CheckStatus {
    let mut worst: Option<CheckStatus> = None;
    for status in statuses {
        // An explicit narrowing does not change the verdict of what did run;
        // it shows up as `scopeComplete = false`.
        if status == CheckStatus::Skipped {
            continue;
        }
        worst = Some(match worst {
            Some(current) if current.severity() >= status.severity() => current,
            _ => status,
        });
    }
    // Nothing ran: an empty or fully-excluded scope proves nothing.
    worst.unwrap_or(CheckStatus::Inconclusive)
}

/// The single machine-readable document for the whole run.
pub fn render_json(run: &VerifyRunReport) -> String {
    serde_json::to_string_pretty(run).expect("verify report is always serializable")
}

/// Where one network's report for `run_id` is persisted.
pub fn report_path(evidence_root: &std::path::Path, run_id: &str) -> std::path::PathBuf {
    evidence_root.join(run_id).join("report.json")
}

/// Persist one network's report as `<evidence_root>/<run-id>/report.json`.
///
/// Written to a temp file in the same directory and renamed, so an interrupted
/// run leaves no partial file that could be mistaken for complete evidence.
pub fn write_network_report(
    evidence_root: &std::path::Path,
    report: &VerifyReport,
) -> anyhow::Result<std::path::PathBuf> {
    use anyhow::Context as _;

    let dir = evidence_root.join(&report.run_id);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating verify evidence dir {}", dir.display()))?;
    let final_path = report_path(evidence_root, &report.run_id);
    let temp_path = dir.join(format!("report.json.{}.partial", std::process::id()));

    let body = serde_json::to_string_pretty(report)
        .with_context(|| format!("serializing the {} verify report", report.network))?;
    std::fs::write(&temp_path, body).with_context(|| format!("writing {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &final_path).with_context(|| {
        format!(
            "renaming {} to {}",
            temp_path.display(),
            final_path.display()
        )
    })?;
    Ok(final_path)
}

/// List available checks.
pub fn print_check_list(checks: &[(String, String, String)]) {
    eprintln!("{}", style("Available checks:").bold());
    eprintln!();
    for (name, tier, desc) in checks {
        eprintln!("  {:<36} [{}] {}", style(name).cyan(), tier, desc);
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use crate::verify::checks::{CheckResult, CheckStatus, CompletedCheck, Finding};

    fn check(name: &'static str, status: CheckStatus) -> CompletedCheck {
        CompletedCheck {
            name,
            description: "stub",
            tier: "fast".to_string(),
            status,
            status_reason: None,
            duration_ms: 1,
            result: match status {
                CheckStatus::Pass => Some(CheckResult::pass(1)),
                CheckStatus::Fail => Some(CheckResult::fail(
                    1,
                    vec![Finding {
                        entity: "e".into(),
                        details: vec!["1 shannon short".into()],
                    }],
                )),
                CheckStatus::Inconclusive => Some(CheckResult::inconclusive("budget exhausted")),
                CheckStatus::NotApplicable => Some(CheckResult::not_applicable("no such protocol")),
                CheckStatus::Error => Some(CheckResult::error("rpc schema")),
                CheckStatus::Skipped => None,
            },
        }
    }

    fn network(name: &str, checks: Vec<CompletedCheck>) -> VerifyReport {
        VerifyReport::new("run-1", name, checks, Duration::from_millis(5))
    }

    #[test]
    fn a_proven_failure_outranks_error_and_inconclusive() {
        let report = network(
            "mainnet",
            vec![
                check("a", CheckStatus::Fail),
                check("b", CheckStatus::Error),
                check("c", CheckStatus::Inconclusive),
                check("d", CheckStatus::Pass),
            ],
        );
        assert_eq!(report.status, CheckStatus::Fail);
        assert!(
            !report.scope_complete,
            "an unfinished case leaves the scope incomplete even when another case failed"
        );

        let run = VerifyRunReport::new("run-1", vec![report]);
        assert_eq!(run.exit_code(), 1);
        assert!(!run.scope_complete);
    }

    #[test]
    fn error_outranks_inconclusive_and_both_exit_two() {
        let run = VerifyRunReport::new(
            "run-1",
            vec![network(
                "mainnet",
                vec![
                    check("a", CheckStatus::Inconclusive),
                    check("b", CheckStatus::Error),
                ],
            )],
        );
        assert_eq!(run.status, CheckStatus::Error);
        assert_eq!(run.exit_code(), 2);
    }

    #[test]
    fn a_complete_pass_and_proven_not_applicable_exit_zero() {
        let run = VerifyRunReport::new(
            "run-1",
            vec![network(
                "mainnet",
                vec![
                    check("a", CheckStatus::Pass),
                    check("b", CheckStatus::NotApplicable),
                ],
            )],
        );
        assert_eq!(run.status, CheckStatus::Pass);
        assert_eq!(run.exit_code(), 0);
        assert!(run.scope_complete);
    }

    #[test]
    fn explicitly_excluded_checks_exit_zero_but_are_listed_and_not_complete() {
        let mut excluded = check("explorer_thing", CheckStatus::Skipped);
        excluded.status_reason = Some("--no-explorer or explorer URL not set".to_string());
        let run = VerifyRunReport::new(
            "run-1",
            vec![network(
                "mainnet",
                vec![check("a", CheckStatus::Pass), excluded],
            )],
        );

        assert_eq!(run.status, CheckStatus::Pass);
        assert_eq!(run.exit_code(), 0);
        assert!(
            !run.scope_complete,
            "a narrowed run must not be described as a complete preset"
        );

        let json: serde_json::Value = serde_json::from_str(&render_json(&run)).unwrap();
        let listed = json["networks"][0]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "explorer_thing")
            .expect("excluded checks must still be listed");
        assert_eq!(listed["status"], "skipped");
        assert_eq!(
            listed["statusReason"],
            "--no-explorer or explorer URL not set"
        );
    }

    #[test]
    fn a_scope_where_nothing_ran_is_inconclusive_not_a_pass() {
        let mut skipped = check("a", CheckStatus::Skipped);
        skipped.status_reason = Some("--rpc-url not provided".to_string());
        let run = VerifyRunReport::new("run-1", vec![network("mainnet", vec![skipped])]);

        assert_eq!(
            run.status,
            CheckStatus::Inconclusive,
            "a run that verified nothing must not report pass(0)"
        );
        assert_eq!(run.exit_code(), 2);
    }

    #[test]
    fn one_failing_network_and_one_unreachable_network_merge_to_fail() {
        let run = VerifyRunReport::new(
            "run-1",
            vec![
                network("mainnet", vec![check("a", CheckStatus::Fail)]),
                network("testnet", vec![check("a", CheckStatus::Error)]),
            ],
        );

        assert_eq!(run.status, CheckStatus::Fail);
        assert_eq!(run.exit_code(), 1);
        assert!(!run.scope_complete);

        let json: serde_json::Value = serde_json::from_str(&render_json(&run)).unwrap();
        let networks = json["networks"].as_array().unwrap();
        assert_eq!(networks.len(), 2, "both networks must be reported");
        assert_eq!(networks[0]["network"], "mainnet");
        assert_eq!(networks[0]["status"], "fail");
        assert_eq!(networks[1]["network"], "testnet");
        assert_eq!(networks[1]["status"], "error");
    }

    #[test]
    fn json_is_one_versioned_envelope() {
        let run = VerifyRunReport::new(
            "20260923T101500Z-abcdef12",
            vec![
                network("mainnet", vec![check("a", CheckStatus::Pass)]),
                network("testnet", vec![check("a", CheckStatus::Pass)]),
            ],
        );
        let rendered = render_json(&run);
        let json: serde_json::Value =
            serde_json::from_str(&rendered).expect("stdout must be one parseable JSON document");

        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["runId"], "20260923T101500Z-abcdef12");
        assert_eq!(json["status"], "pass");
        assert_eq!(json["scopeComplete"], true);
        assert_eq!(json["networks"].as_array().unwrap().len(), 2);
        assert!(
            !rendered.contains("[mainnet]"),
            "network headings must not be concatenated into the JSON document"
        );
    }

    #[test]
    fn text_summary_states_the_same_status_as_json() {
        let run = VerifyRunReport::new(
            "run-1",
            vec![network(
                "mainnet",
                vec![check("a", CheckStatus::Inconclusive)],
            )],
        );
        let text = render_text_summary(&run);
        let json: serde_json::Value = serde_json::from_str(&render_json(&run)).unwrap();

        assert_eq!(json["status"], "inconclusive");
        assert!(
            text.contains("inconclusive"),
            "text and JSON must be synonymous: {text}"
        );
        assert!(text.contains("mainnet"), "{text}");
    }

    #[test]
    fn a_network_report_lands_atomically_under_its_run_id() {
        let dir = tempfile::tempdir().unwrap();
        let report = network("mainnet", vec![check("a", CheckStatus::Pass)]);

        let path = write_network_report(dir.path(), &report).unwrap();

        assert_eq!(path, dir.path().join("run-1").join("report.json"));
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["network"], "mainnet");
        assert_eq!(written["schemaVersion"], 1);

        let leftovers: Vec<String> = std::fs::read_dir(dir.path().join("run-1"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "report.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "a partial temp file must never survive: {leftovers:?}"
        );
    }
}
