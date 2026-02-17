//! Terminal rendering (text + JSON output modes) for verification results.

use std::time::Duration;

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use super::checks::{CheckTier, CompletedCheck};

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

/// Create a progress bar for a sampling/exhaustive check with known total.
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
pub fn finish_check(pb: &ProgressBar, completed: &CompletedCheck) {
    let duration = format_duration(Duration::from_millis(completed.duration_ms));

    if completed.skipped {
        let reason = completed.skip_reason.as_deref().unwrap_or("skipped");
        pb.finish_with_message(format!(
            "{} {:<40} {} ({})",
            style("⊘").yellow(),
            completed.name,
            style(&duration).dim(),
            style(reason).yellow()
        ));
    } else if completed.passed {
        let detail = completed
            .result
            .as_ref()
            .and_then(|r| r.detail.as_deref())
            .unwrap_or("");
        let detail_str = if detail.is_empty() {
            String::new()
        } else {
            format!("\n    {}", style(detail).dim())
        };
        pb.finish_with_message(format!(
            "{} {:<40} {}{}",
            style("✓").green().bold(),
            completed.name,
            style(&duration).dim(),
            detail_str,
        ));
    } else {
        pb.finish_with_message(format!(
            "{} {:<40} {}",
            style("✗").red().bold(),
            completed.name,
            style(&duration).dim(),
        ));
    }
}

/// Print findings (errors) for a failed check.
pub fn print_findings(completed: &CompletedCheck) {
    if let Some(ref result) = completed.result {
        if result.findings.is_empty() {
            return;
        }
        let count = result.findings.len();
        let noun = if count == 1 { "mismatch" } else { "mismatches" };
        eprintln!("    {} {} found:\n", style(count).red().bold(), noun);
        for finding in result.findings.iter().take(10) {
            eprintln!("    {} {}", style("┌─").dim(), finding.entity);
            for detail in &finding.details {
                eprintln!("    {}  {}", style("│").dim(), detail);
            }
            eprintln!("    {}", style("└─").dim());
        }
        if count > 10 {
            eprintln!("    {} ... and {} more", style("│").dim(), count - 10,);
        }
        eprintln!();
    }
}

/// Print a tier header.
pub fn print_tier_header(tier: CheckTier) {
    let label = match tier {
        CheckTier::Fast => "FAST CHECKS",
        CheckTier::Sampling => "SAMPLING CHECKS",
        CheckTier::Exhaustive => "EXHAUSTIVE CHECKS",
    };
    eprintln!("\n{}", style(label).bold().underlined());
}

/// Print the explorer section header.
pub fn print_explorer_header() {
    eprintln!("\n{}", style("EXPLORER COMPARISON").bold().underlined());
}

/// Print the summary banner.
pub fn print_summary(results: &[CompletedCheck], total_duration: Duration) {
    let passed = results.iter().filter(|c| c.passed && !c.skipped).count();
    let failed = results.iter().filter(|c| !c.passed).count();
    let skipped = results.iter().filter(|c| c.skipped).count();
    let duration_str = format_duration(total_duration);

    eprintln!();
    let banner = format!(
        "  RESULT: {} passed, {} failed, {} skipped ({})",
        passed, failed, skipped, duration_str
    );
    let width = banner.len() + 4;
    let bar = "━".repeat(width);

    if failed > 0 {
        eprintln!("{}", style(&bar).red());
        eprintln!("{}", style(&banner).red().bold());
        eprintln!("{}", style(&bar).red());
    } else {
        eprintln!("{}", style(&bar).green());
        eprintln!("{}", style(&banner).green().bold());
        eprintln!("{}", style(&bar).green());
    }
}

/// Print the initial header with store info.
pub fn print_header(
    data_path: &str,
    tip: i64,
    depth: &str,
    seed: u64,
    samples: usize,
    rpc_url: Option<&str>,
) {
    eprintln!();
    eprintln!("{}", style("ckbadger verify v0.1.0").bold());
    eprintln!("Store: {} (tip: #{})", data_path, format_number(tip as u64));
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

/// JSON output for CI/machine consumption.
#[derive(serde::Serialize)]
pub struct JsonReport {
    pub summary: JsonSummary,
    pub checks: Vec<CompletedCheck>,
}

#[derive(serde::Serialize)]
pub struct JsonSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration_ms: u64,
}

pub fn print_json_report(results: &[CompletedCheck], total_duration: Duration) {
    let passed = results.iter().filter(|c| c.passed && !c.skipped).count();
    let failed = results.iter().filter(|c| !c.passed).count();
    let skipped = results.iter().filter(|c| c.skipped).count();

    let report = JsonReport {
        summary: JsonSummary {
            passed,
            failed,
            skipped,
            total_duration_ms: total_duration.as_millis() as u64,
        },
        checks: results.to_vec(),
    };

    let json = serde_json::to_string_pretty(&report).unwrap_or_default();
    println!("{}", json);
}

/// List available checks.
pub fn print_check_list(checks: &[(String, String, String)]) {
    eprintln!("{}", style("Available checks:").bold());
    eprintln!();
    for (name, tier, desc) in checks {
        eprintln!("  {:<36} [{}] {}", style(name).cyan(), tier, desc);
    }
}
