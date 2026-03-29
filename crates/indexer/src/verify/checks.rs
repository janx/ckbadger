//! Core types and trait for verification checks.

use std::path::PathBuf;
use std::time::Instant;

/// Severity tier. Derives PartialOrd so `tier <= depth` naturally includes lower tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckTier {
    Fast,
    Sampling,
}

impl std::fmt::Display for CheckTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckTier::Fast => write!(f, "fast"),
            CheckTier::Sampling => write!(f, "sampling"),
        }
    }
}

/// Context shared with every check. All data comes via HTTP — no store dependency.
pub struct CheckContext {
    pub api_url: String,
    pub rpc_url: Option<String>,
    pub explorer_url: Option<String>,
    pub http: reqwest::blocking::Client,
    pub sample_count: usize,
    pub seed: u64,
    pub tolerance: f64,
    pub cache_dir: Option<PathBuf>,
}

/// Progress reporter wrapping indicatif. Checks call .inc() to advance progress.
pub struct ProgressReporter {
    bar: Option<indicatif::ProgressBar>,
}

impl ProgressReporter {
    pub fn new(bar: Option<indicatif::ProgressBar>) -> Self {
        Self { bar }
    }

    pub fn inc(&self, n: u64) {
        if let Some(ref bar) = self.bar {
            bar.inc(n);
        }
    }

    pub fn set_message(&self, msg: &str) {
        if let Some(ref bar) = self.bar {
            bar.set_message(msg.to_string());
        }
    }
}

/// A single finding (mismatch or error) from a check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Finding {
    /// Human-readable entity identifier (address, block number, etc.)
    pub entity: String,
    /// Detail lines describing the mismatch.
    pub details: Vec<String>,
}

/// Result of running a single check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckResult {
    pub passed: bool,
    pub items_checked: u64,
    pub items_failed: u64,
    /// Optional detail message shown on the pass line.
    pub detail: Option<String>,
    pub findings: Vec<Finding>,
}

impl CheckResult {
    pub fn pass(items_checked: u64) -> Self {
        Self {
            passed: true,
            items_checked,
            items_failed: 0,
            detail: None,
            findings: vec![],
        }
    }

    pub fn pass_with_detail(items_checked: u64, detail: impl Into<String>) -> Self {
        Self {
            passed: true,
            items_checked,
            items_failed: 0,
            detail: Some(detail.into()),
            findings: vec![],
        }
    }

    pub fn fail(items_checked: u64, findings: Vec<Finding>) -> Self {
        let items_failed = findings.len() as u64;
        Self {
            passed: false,
            items_checked,
            items_failed,
            detail: None,
            findings,
        }
    }
}

/// Completed check with metadata for reporting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompletedCheck {
    pub name: &'static str,
    pub description: &'static str,
    pub tier: String,
    pub passed: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
    pub result: Option<CheckResult>,
}

/// Core trait every check implements.
pub trait Check: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn tier(&self) -> CheckTier;
    fn requires_rpc(&self) -> bool {
        false
    }
    fn requires_explorer(&self) -> bool {
        false
    }
    /// Estimated total items (for progress bar length). None = use spinner instead.
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        None
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult>;
}

/// Run a check and wrap the result with timing and skip logic.
pub fn execute_check(
    check: &dyn Check,
    ctx: &CheckContext,
    progress: &ProgressReporter,
) -> CompletedCheck {
    // Check skip conditions
    if check.requires_rpc() && ctx.rpc_url.is_none() {
        return CompletedCheck {
            name: check.name(),
            description: check.description(),
            tier: check.tier().to_string(),
            passed: true,
            skipped: true,
            skip_reason: Some("--rpc-url not provided".to_string()),
            duration_ms: 0,
            result: None,
        };
    }
    if check.requires_explorer() && ctx.explorer_url.is_none() {
        return CompletedCheck {
            name: check.name(),
            description: check.description(),
            tier: check.tier().to_string(),
            passed: true,
            skipped: true,
            skip_reason: Some("--no-explorer or explorer URL not set".to_string()),
            duration_ms: 0,
            result: None,
        };
    }

    let start = Instant::now();
    let result = check.run(ctx, progress);
    let duration = start.elapsed();

    match result {
        Ok(check_result) => CompletedCheck {
            name: check.name(),
            description: check.description(),
            tier: check.tier().to_string(),
            passed: check_result.passed,
            skipped: false,
            skip_reason: None,
            duration_ms: duration.as_millis() as u64,
            result: Some(check_result),
        },
        Err(e) => CompletedCheck {
            name: check.name(),
            description: check.description(),
            tier: check.tier().to_string(),
            passed: false,
            skipped: false,
            skip_reason: None,
            duration_ms: duration.as_millis() as u64,
            result: Some(CheckResult::fail(
                0,
                vec![Finding {
                    entity: "error".to_string(),
                    details: vec![format!("Check failed with error: {}", e)],
                }],
            )),
        },
    }
}

/// HTTP GET with exponential-backoff retry on 429 (Too Many Requests).
/// Shared by api_checks and explorer modules.
pub(super) fn api_get<T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    path: &str,
) -> anyhow::Result<T> {
    let url = format!(
        "{}/{}",
        ctx.api_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let mut backoff_ms = 500;
    for attempt in 0..5 {
        let resp = ctx.http.get(&url).send()?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < 4 {
            std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
            backoff_ms *= 2;
            continue;
        }
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            let detail = if body.is_empty() {
                String::new()
            } else {
                format!(": {}", &body[..body.len().min(512)])
            };
            anyhow::bail!("GET {} returned {}{}", path, status, detail);
        }
        return Ok(resp.json()?);
    }
    unreachable!()
}
