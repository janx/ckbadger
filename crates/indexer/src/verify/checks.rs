//! Core types and trait for verification checks.

pub use super::entity_history::EntityBudget;

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

/// One entity a chain-derived check should verify, as `<kind>:<id>`.
///
/// The family is part of the selector because an id alone does not say which
/// adapter owns it, and an adapter that guesses would silently verify the wrong
/// index.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySelector {
    pub kind: String,
    pub id: String,
}

impl EntitySelector {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let (kind, id) = raw.split_once(':').ok_or_else(|| {
            format!("entity selector '{raw}' must be '<kind>:<id>', e.g. token:0x…")
        })?;
        if kind.is_empty() || id.is_empty() {
            return Err(format!(
                "entity selector '{raw}' must name both a kind and an id"
            ));
        }
        Ok(Self {
            kind: kind.to_string(),
            id: canonical_entity_id(raw, id)?,
        })
    }
}

/// Every entity id is hex bytes, and the index and its export key them as
/// `0x` + lowercase. A bare or upper-case id is the same entity, so it is
/// canonicalized here; anything that is not whole bytes of hex is rejected
/// rather than left to match nothing.
fn canonical_entity_id(raw: &str, id: &str) -> Result<String, String> {
    let digits = id
        .strip_prefix("0x")
        .or_else(|| id.strip_prefix("0X"))
        .unwrap_or(id);
    if digits.is_empty()
        || !digits.len().is_multiple_of(2)
        || !digits.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(format!(
            "entity selector '{raw}' must carry a hex id of whole bytes, e.g. token:0x…"
        ));
    }
    Ok(format!("0x{}", digits.to_ascii_lowercase()))
}

impl std::fmt::Display for EntitySelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.kind, self.id)
    }
}

/// Context shared with every check. All data comes via HTTP — no store dependency.
pub struct CheckContext {
    /// Canonical CKB network name (`mainnet` or `testnet`).
    pub network: &'static str,
    pub api_url: String,
    pub rpc_url: Option<String>,
    pub explorer_url: Option<String>,
    pub http: reqwest::blocking::Client,
    pub sample_count: usize,
    pub seed: u64,
    pub tolerance: f64,
    pub cache_dir: Option<PathBuf>,
    /// Entities explicitly selected with `--entity`. Empty means the check
    /// picks its own candidates.
    pub entities: Vec<EntitySelector>,
    /// The operator's `verify-source.toml`, which qualifies the chain-history
    /// source. Absent means no chain-derived check can reach `Pass`.
    pub verify_source_path: Option<PathBuf>,
    /// Where this run's manifest and evidence are written.
    pub evidence_dir: Option<PathBuf>,
    /// How much a chain-derived check may spend. Explicit so a run that does
    /// not fit is answered by raising the budget, not by narrowing scope.
    pub entity_budget: super::entity_history::EntityBudget,
    /// Where a chain-derived check publishes the history source it qualified.
    ///
    /// The run report has to say what its expected values were derived from,
    /// and only the check that qualified the source knows; this is the one
    /// channel back, rather than a second qualification in the runner.
    pub source_profile: std::sync::Mutex<Option<super::report::SourceProfileReport>>,
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

/// Outcome of one check, as reported and as merged into the process exit code.
///
/// The distinction the binary pass/fail pair could not express is between
/// *evidence of a data inconsistency* and *absence of evidence*: a check that
/// could not run, ran out of budget, or hit a transport error proves nothing
/// about the data and must never be rendered as green.
///
/// - `Pass` — the whole declared scope of the check was verified and agreed.
/// - `Fail` — a difference was proven. Exit code 1.
/// - `Inconclusive` — chain/anchor moved, budget exhausted, adapter unsupported
///   or the source data was incomplete. Exit code 2.
/// - `Skipped` — the operator explicitly narrowed the scope (`--checks`,
///   `--no-explorer`, no `--rpc-url`). Exit code 0, but the scope is incomplete.
/// - `NotApplicable` — independently proven that the network has no such object.
/// - `Error` — RPC/response schema, a cursor that does not advance, a bad
///   parameter, or a local execution failure. Exit code 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Pass,
    Fail,
    Inconclusive,
    Skipped,
    NotApplicable,
    Error,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Fail => "fail",
            CheckStatus::Inconclusive => "inconclusive",
            CheckStatus::Skipped => "skipped",
            CheckStatus::NotApplicable => "notApplicable",
            CheckStatus::Error => "error",
        }
    }

    /// Process exit code this status contributes: `Fail (1) > Error/Inconclusive (2) > 0`.
    ///
    /// The numeric value is not the merge order — see [`CheckStatus::severity`].
    pub fn exit_code(self) -> u8 {
        match self {
            CheckStatus::Pass | CheckStatus::NotApplicable | CheckStatus::Skipped => 0,
            CheckStatus::Fail => 1,
            CheckStatus::Inconclusive | CheckStatus::Error => 2,
        }
    }

    /// Merge rank: the highest-ranking status of any check becomes the run's.
    ///
    /// `Skipped` shares the lowest rank because an explicit narrowing is not a
    /// verdict; it is reported through `scopeComplete` instead.
    pub fn severity(self) -> u8 {
        match self {
            CheckStatus::Pass | CheckStatus::NotApplicable | CheckStatus::Skipped => 0,
            CheckStatus::Inconclusive => 1,
            CheckStatus::Error => 2,
            CheckStatus::Fail => 3,
        }
    }

    /// Whether this status covered the scope it claimed.
    ///
    /// `Skipped`, `Inconclusive` and `Error` all leave part of the declared
    /// scope unverified, which is reported separately from the status itself.
    pub fn is_complete(self) -> bool {
        matches!(
            self,
            CheckStatus::Pass | CheckStatus::Fail | CheckStatus::NotApplicable
        )
    }

    pub fn is_pass(self) -> bool {
        self == CheckStatus::Pass
    }
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
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
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub status: CheckStatus,
    pub items_checked: u64,
    pub items_failed: u64,
    /// Optional detail message shown on the result line. For a non-`Pass`
    /// status this carries the reason or evidence for that status.
    pub detail: Option<String>,
    pub findings: Vec<Finding>,
}

impl CheckResult {
    pub fn pass(items_checked: u64) -> Self {
        Self {
            status: CheckStatus::Pass,
            items_checked,
            items_failed: 0,
            detail: None,
            findings: vec![],
        }
    }

    pub fn pass_with_detail(items_checked: u64, detail: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Pass,
            items_checked,
            items_failed: 0,
            detail: Some(detail.into()),
            findings: vec![],
        }
    }

    pub fn fail(items_checked: u64, findings: Vec<Finding>) -> Self {
        let items_failed = findings.len() as u64;
        Self {
            status: CheckStatus::Fail,
            items_checked,
            items_failed,
            detail: None,
            findings,
        }
    }

    /// The check could not reach a verdict: the anchor moved, the budget ran
    /// out, the adapter does not cover this family, or the source was
    /// incomplete. `reason` must say which, so the run is auditable.
    pub fn inconclusive(reason: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Inconclusive,
            items_checked: 0,
            items_failed: 0,
            detail: Some(reason.into()),
            findings: vec![],
        }
    }

    /// Independently proven that the network holds no object of this kind.
    /// `evidence` records the proof; an empty local list is never enough.
    pub fn not_applicable(evidence: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::NotApplicable,
            items_checked: 0,
            items_failed: 0,
            detail: Some(evidence.into()),
            findings: vec![],
        }
    }

    /// Transport, schema, parameter or local execution failure — no statement
    /// about the data.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Error,
            items_checked: 0,
            items_failed: 0,
            detail: Some(message.into()),
            findings: vec![],
        }
    }

    pub fn passed(&self) -> bool {
        self.status.is_pass()
    }
}

/// Completed check with metadata for reporting.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedCheck {
    pub name: &'static str,
    pub description: &'static str,
    pub tier: String,
    pub status: CheckStatus,
    /// Why this check carries a non-`Pass` status. Required for every status
    /// other than `Pass` and `Fail` (whose evidence is its findings).
    pub status_reason: Option<String>,
    pub duration_ms: u64,
    pub result: Option<CheckResult>,
}

impl CompletedCheck {
    pub fn passed(&self) -> bool {
        self.status.is_pass()
    }

    pub fn skipped(&self) -> bool {
        self.status == CheckStatus::Skipped
    }

    /// A check the operator explicitly excluded from an otherwise wider scope.
    pub fn excluded(check: &dyn Check, reason: impl Into<String>) -> Self {
        Self {
            name: check.name(),
            description: check.description(),
            tier: check.tier().to_string(),
            status: CheckStatus::Skipped,
            status_reason: Some(reason.into()),
            duration_ms: 0,
            result: None,
        }
    }
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
    /// Whether this check actually reads `--sample-count`. Only those checks
    /// have nothing to verify at `--sample-count 0`, which is then a request
    /// error rather than a pass over an empty set.
    ///
    /// Deliberately not "is it in the sampling tier": most sampling-tier checks
    /// validate a whole chart or index and never look at `sample_count`, so
    /// tying the two would report them as parameter errors for a parameter they
    /// ignore.
    fn requires_sampling(&self) -> bool {
        false
    }
    /// Estimated total items (for progress bar length). None = use spinner instead.
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        None
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult>;
}

/// Run a check and wrap the result with timing and its status.
///
/// A check that never ran is `Skipped`, a check that could not reach the data
/// is `Error` — neither is a pass. Only the check's own verdict produces
/// `Pass`/`Fail`.
pub fn execute_check(
    check: &dyn Check,
    ctx: &CheckContext,
    progress: &ProgressReporter,
) -> CompletedCheck {
    let meta = |status: CheckStatus, reason: Option<String>, duration_ms, result| CompletedCheck {
        name: check.name(),
        description: check.description(),
        tier: check.tier().to_string(),
        status,
        status_reason: reason,
        duration_ms,
        result,
    };

    if check.requires_rpc() && ctx.rpc_url.is_none() {
        return meta(
            CheckStatus::Skipped,
            Some("--rpc-url not provided".to_string()),
            0,
            None,
        );
    }
    if check.requires_explorer() && ctx.explorer_url.is_none() {
        return meta(
            CheckStatus::Skipped,
            Some("--no-explorer or explorer URL not set".to_string()),
            0,
            None,
        );
    }
    if check.requires_sampling() && ctx.sample_count == 0 {
        let message = "--sample-count 0 leaves a sampling check nothing to verify".to_string();
        return meta(
            CheckStatus::Error,
            Some(message.clone()),
            0,
            Some(CheckResult::error(message)),
        );
    }

    let start = Instant::now();
    let result = check.run(ctx, progress);
    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(check_result) => {
            let status = check_result.status;
            let reason = match status {
                CheckStatus::Pass | CheckStatus::Fail => None,
                _ => check_result.detail.clone(),
            };
            meta(status, reason, duration_ms, Some(check_result))
        }
        Err(e) => {
            let message = format!("check could not be completed: {e:#}");
            meta(
                CheckStatus::Error,
                Some(message.clone()),
                duration_ms,
                Some(CheckResult::error(message)),
            )
        }
    }
}

const WARMUP_PENDING_MAX_ATTEMPTS: usize = 30;
const WARMUP_PENDING_RETRY_DELAY_MS: u64 = 1_000;

/// What one API GET produced: the decoded body, or a 404 carrying its detail.
enum ApiGetOutcome<T> {
    Found(T),
    NotFound { detail: String },
}

/// The one GET path every API read goes through: exponential-backoff retry on
/// 429 (Too Many Requests) and warmup-pending retry on 503. A 404 is returned
/// as [`ApiGetOutcome::NotFound`]; every other non-2xx, transport or decode
/// failure is an error.
fn api_get_outcome<T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    path: &str,
) -> anyhow::Result<ApiGetOutcome<T>> {
    let url = format!(
        "{}/{}",
        ctx.api_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let mut backoff_ms = 500;
    for attempt in 0..WARMUP_PENDING_MAX_ATTEMPTS {
        let resp = ctx.http.get(&url).send()?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            && attempt + 1 < WARMUP_PENDING_MAX_ATTEMPTS
        {
            std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
            backoff_ms = (backoff_ms * 2).min(8_000);
            continue;
        }
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
                && is_warmup_pending_body(&body)
                && attempt + 1 < WARMUP_PENDING_MAX_ATTEMPTS
            {
                std::thread::sleep(std::time::Duration::from_millis(
                    WARMUP_PENDING_RETRY_DELAY_MS,
                ));
                continue;
            }
            let detail = if body.is_empty() {
                String::new()
            } else {
                // Truncate on character boundaries: a byte slice can split a
                // multi-byte sequence and panic while reporting an error.
                format!(": {}", body.chars().take(512).collect::<String>())
            };
            if status == reqwest::StatusCode::NOT_FOUND {
                return Ok(ApiGetOutcome::NotFound { detail });
            }
            anyhow::bail!("GET {} returned {}{}", path, status, detail);
        }
        return Ok(ApiGetOutcome::Found(resp.json()?));
    }
    unreachable!()
}

/// HTTP GET whose every non-2xx answer — 404 included — is an error.
/// Shared by api_checks and explorer modules.
pub(super) fn api_get<T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    path: &str,
) -> anyhow::Result<T> {
    match api_get_outcome(ctx, path)? {
        ApiGetOutcome::Found(value) => Ok(value),
        ApiGetOutcome::NotFound { detail } => anyhow::bail!(
            "GET {} returned {}{}",
            path,
            reqwest::StatusCode::NOT_FOUND,
            detail
        ),
    }
}

/// HTTP GET for a resource whose absence is itself a statement: `Ok(None)` on
/// HTTP 404 ONLY. A 5xx, a transport failure or a body that does not decode is
/// still an error — folding those into "absent" is how a broken API reads as a
/// network without the protocol.
pub(super) fn api_get_or_not_found<T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    path: &str,
) -> anyhow::Result<Option<T>> {
    match api_get_outcome(ctx, path)? {
        ApiGetOutcome::Found(value) => Ok(Some(value)),
        ApiGetOutcome::NotFound { .. } => Ok(None),
    }
}

/// HTTP POST with a JSON body, for the typed verify export.
///
/// Deliberately without the warmup/429 retry loop of [`api_get`]: the export is
/// one request per case and its anchor is only valid inside that request, so a
/// silent retry would compare rows from one view against an anchor from
/// another.
pub(super) fn api_post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    path: &str,
    body: &B,
) -> anyhow::Result<T> {
    let url = format!(
        "{}/{}",
        ctx.api_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let response = ctx.http.post(&url).json(body).send()?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        anyhow::bail!(
            "POST {} returned {}{}",
            path,
            status,
            if body.is_empty() {
                String::new()
            } else {
                format!(": {}", body.chars().take(512).collect::<String>())
            }
        );
    }
    Ok(response.json()?)
}

fn is_warmup_pending_body(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.as_str())
                .map(str::to_owned)
        })
        .is_some_and(|error| error == "warmup_pending")
}

#[cfg(test)]
mod status_model_tests {
    use super::*;

    fn ctx(rpc_url: Option<&str>, explorer_url: Option<&str>, sample_count: usize) -> CheckContext {
        CheckContext {
            network: "mainnet",
            api_url: "http://127.0.0.1:1/api/v1".to_string(),
            rpc_url: rpc_url.map(str::to_string),
            explorer_url: explorer_url.map(str::to_string),
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_millis(50))
                .build()
                .unwrap(),
            sample_count,
            seed: 42,
            tolerance: 0.0,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: None,
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        }
    }

    struct Stub {
        tier: CheckTier,
        rpc: bool,
        explorer: bool,
        sampling: bool,
        outcome: fn() -> anyhow::Result<CheckResult>,
    }

    impl Check for Stub {
        fn name(&self) -> &'static str {
            "stub_check"
        }
        fn description(&self) -> &'static str {
            "stub"
        }
        fn tier(&self) -> CheckTier {
            self.tier
        }
        fn requires_rpc(&self) -> bool {
            self.rpc
        }
        fn requires_explorer(&self) -> bool {
            self.explorer
        }
        fn requires_sampling(&self) -> bool {
            self.sampling
        }
        fn run(
            &self,
            _ctx: &CheckContext,
            _progress: &ProgressReporter,
        ) -> anyhow::Result<CheckResult> {
            (self.outcome)()
        }
    }

    fn stub(tier: CheckTier, outcome: fn() -> anyhow::Result<CheckResult>) -> Stub {
        Stub {
            tier,
            rpc: false,
            explorer: false,
            sampling: false,
            outcome,
        }
    }

    fn progress() -> ProgressReporter {
        ProgressReporter::new(None)
    }

    #[test]
    fn exit_codes_follow_the_severity_table() {
        assert_eq!(CheckStatus::Pass.exit_code(), 0);
        assert_eq!(CheckStatus::NotApplicable.exit_code(), 0);
        assert_eq!(CheckStatus::Skipped.exit_code(), 0);
        assert_eq!(CheckStatus::Fail.exit_code(), 1);
        assert_eq!(CheckStatus::Inconclusive.exit_code(), 2);
        assert_eq!(CheckStatus::Error.exit_code(), 2);
    }

    #[test]
    fn only_pass_fail_and_not_applicable_complete_the_declared_scope() {
        assert!(CheckStatus::Pass.is_complete());
        assert!(CheckStatus::Fail.is_complete());
        assert!(CheckStatus::NotApplicable.is_complete());
        assert!(!CheckStatus::Skipped.is_complete());
        assert!(!CheckStatus::Inconclusive.is_complete());
        assert!(!CheckStatus::Error.is_complete());
    }

    #[test]
    fn constructors_carry_their_status_and_evidence() {
        assert_eq!(CheckResult::pass(3).status, CheckStatus::Pass);
        assert_eq!(
            CheckResult::fail(
                3,
                vec![Finding {
                    entity: "e".into(),
                    details: vec!["d".into()]
                }]
            )
            .status,
            CheckStatus::Fail
        );

        let inconclusive = CheckResult::inconclusive("anchor moved during the walk");
        assert_eq!(inconclusive.status, CheckStatus::Inconclusive);
        assert_eq!(
            inconclusive.detail.as_deref(),
            Some("anchor moved during the walk")
        );
        assert!(!inconclusive.passed());

        let not_applicable = CheckResult::not_applicable("no Spore deployment on this network");
        assert_eq!(not_applicable.status, CheckStatus::NotApplicable);
        assert_eq!(
            not_applicable.detail.as_deref(),
            Some("no Spore deployment on this network")
        );
        assert!(!not_applicable.passed());

        let error = CheckResult::error("cursor did not advance");
        assert_eq!(error.status, CheckStatus::Error);
        assert_eq!(error.detail.as_deref(), Some("cursor did not advance"));
        assert!(!error.passed());
    }

    #[test]
    fn a_skipped_check_no_longer_counts_as_a_pass() {
        let check = Stub {
            rpc: true,
            ..stub(CheckTier::Fast, || Ok(CheckResult::pass(1)))
        };
        let completed = execute_check(&check, &ctx(None, None, 10), &progress());

        assert_eq!(completed.status, CheckStatus::Skipped);
        assert!(
            !completed.passed(),
            "a check that never ran must not be counted as passing"
        );
        assert!(completed
            .status_reason
            .as_deref()
            .unwrap_or_default()
            .contains("--rpc-url"));
    }

    #[test]
    fn an_excluded_explorer_check_is_skipped_with_an_auditable_reason() {
        let check = Stub {
            explorer: true,
            ..stub(CheckTier::Fast, || Ok(CheckResult::pass(1)))
        };
        let completed = execute_check(&check, &ctx(Some("http://rpc"), None, 10), &progress());

        assert_eq!(completed.status, CheckStatus::Skipped);
        assert!(!completed.passed());
        assert!(completed
            .status_reason
            .as_deref()
            .unwrap_or_default()
            .contains("--no-explorer"));
    }

    #[test]
    fn a_check_that_returns_err_is_an_error_not_a_data_failure() {
        let check = stub(CheckTier::Fast, || {
            Err(anyhow::anyhow!("connection refused"))
        });
        let completed = execute_check(&check, &ctx(None, None, 10), &progress());

        assert_eq!(
            completed.status,
            CheckStatus::Error,
            "a local/transport failure is not proof of a data inconsistency"
        );
        assert!(!completed.passed());
        let reason = completed.status_reason.clone().unwrap_or_default();
        assert!(reason.contains("connection refused"), "{reason}");
    }

    #[test]
    fn a_check_that_reads_the_sample_count_errors_when_it_is_zero() {
        let check = Stub {
            sampling: true,
            ..stub(CheckTier::Sampling, || Ok(CheckResult::pass(1)))
        };
        let completed = execute_check(&check, &ctx(None, None, 0), &progress());

        assert_eq!(
            completed.status,
            CheckStatus::Error,
            "sample_count=0 for a check that samples is a request error, never a pass"
        );
        let reason = completed.status_reason.clone().unwrap_or_default();
        assert!(reason.contains("sample-count"), "{reason}");
    }

    /// Most sampling-tier checks validate a whole chart or index and never read
    /// `sample_count`. Reporting them as parameter errors would mislabel ~30
    /// checks over a parameter they ignore.
    #[test]
    fn a_sampling_tier_check_that_ignores_the_sample_count_is_unaffected() {
        let check = stub(CheckTier::Sampling, || Ok(CheckResult::pass(1)));
        let completed = execute_check(&check, &ctx(None, None, 0), &progress());
        assert_eq!(completed.status, CheckStatus::Pass);
    }

    #[test]
    fn a_fast_check_is_unaffected_by_sample_count_zero() {
        let check = stub(CheckTier::Fast, || Ok(CheckResult::pass(1)));
        let completed = execute_check(&check, &ctx(None, None, 0), &progress());
        assert_eq!(completed.status, CheckStatus::Pass);
    }

    /// Exactly the checks that read `--sample-count` declare it. A check added
    /// later that samples but does not declare it would pass over an empty
    /// selection at `--sample-count 0`.
    #[test]
    fn the_registry_declares_sampling_only_where_the_count_is_read() {
        let declared: Vec<&str> = crate::verify::all_checks_for_test()
            .iter()
            .filter(|c| c.requires_sampling())
            .map(|c| c.name())
            .collect();
        let mut expected = vec![
            "block_hash_roundtrip",
            "block_parent_chain",
            "address_balance_spot_check",
            "rpc_block_spot_check",
            "token_activity_transfer_bidirectional",
            "spore_owner_roundtrip",
            "object_asset_collection_consistency",
            "participant_rows_consistency",
            "dotcell_records_hash_parity",
        ];
        let mut declared = declared;
        declared.sort_unstable();
        expected.sort_unstable();
        assert_eq!(declared, expected);
    }

    /// `api_get_or_not_found` turns exactly one answer into "absent": HTTP 404.
    /// A 5xx, a body that does not decode and an unreachable API all stay
    /// errors, so a broken API can never read as a missing resource.
    #[test]
    fn api_get_or_not_found_maps_only_a_404_to_none() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            let get = |route: &str, response: ResponseTemplate| {
                Mock::given(method("GET"))
                    .and(path(format!("/api/v1/{route}")))
                    .respond_with(response)
            };
            get("found", ResponseTemplate::new(200).set_body_json(7))
                .mount(&server)
                .await;
            get("missing", ResponseTemplate::new(404))
                .mount(&server)
                .await;
            get("broken", ResponseTemplate::new(500))
                .mount(&server)
                .await;
            get(
                "garbled",
                ResponseTemplate::new(200).set_body_string("not json"),
            )
            .mount(&server)
            .await;
        });
        let served = CheckContext {
            api_url: format!("{}/api/v1", server.uri()),
            ..ctx(None, None, 1)
        };

        assert_eq!(
            api_get_or_not_found::<u32>(&served, "found").unwrap(),
            Some(7)
        );
        assert_eq!(
            api_get_or_not_found::<u32>(&served, "missing").unwrap(),
            None
        );
        let err = api_get_or_not_found::<u32>(&served, "broken").unwrap_err();
        assert!(format!("{err:#}").contains("500"), "{err:#}");
        assert!(api_get_or_not_found::<u32>(&served, "garbled").is_err());

        // The plain reader still treats a 404 as an error, with its status.
        let err = api_get::<u32>(&served, "missing").unwrap_err();
        assert!(format!("{err:#}").contains("404"), "{err:#}");

        // Nothing listens on port 1: a transport failure is an error too.
        assert!(api_get_or_not_found::<u32>(&ctx(None, None, 1), "found").is_err());
    }

    /// `--entity` ids are canonical `0x` + lowercase hex whatever the operator
    /// typed; ids that are not whole bytes of hex are rejected.
    #[test]
    fn entity_selector_ids_are_canonical_hex() {
        let canonical = EntitySelector::parse("token:0xabcd").unwrap();
        assert_eq!(canonical.id, "0xabcd");
        for raw in ["token:ABCD", "token:abcd", "token:0XAbCd", "token:0xABCD"] {
            assert_eq!(EntitySelector::parse(raw).unwrap(), canonical, "{raw}");
        }
        for raw in ["token:0x", "token:abc", "token:0xzz", "token:dotbit"] {
            assert!(EntitySelector::parse(raw).is_err(), "{raw}");
        }
    }
}
