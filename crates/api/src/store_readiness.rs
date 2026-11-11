//! Serving data only from secondary views that have caught up (J7).
//!
//! The API reads RocksDB secondaries, whose view advances only when
//! `try_catch_up_with_primary()` runs. A freshly opened secondary builds each
//! column family's view at that CF's own latest complete point, so until its
//! first catch-up two CFs the indexer commits together can disagree — round
//! 2's `/tokens` answered 500 for three minutes after a restart because
//! `CF_STATS_MNFT` lagged the collection aggregate CF and the warmup's (correct)
//! "live items ⇒ daily capacity history" invariant fired on healthy data.
//!
//! So the refresh loop below is also the readiness authority:
//!
//! - **Startup.** Its first round runs at once and is retried every
//!   [`STORE_REFRESH_INTERVAL`] until one round has caught up every secondary.
//!   Until then [`StoreReadiness::check`] rejects with `503 store_catching_up`
//!   and [`StoreReadiness::caught_up_once`] holds back the warmup and every
//!   other background reader; each failed attempt is logged at `error!`.
//! - **Steady state.** [`CATCH_UP_FAILURE_STRIKES`] consecutive failures of any
//!   one secondary turn data endpoints into the same 503, carrying that
//!   secondary's last error, until it catches up again. Every further failure
//!   logs at `error!`; the recovery logs at `info!`.
//!
//! One state, one gate: [`require_caught_up_stores`] is mounted once over
//! every route that reads a secondary (`routes::store_routes`).
//!
//! [`SecondaryCatchUp`] is the loop's seam: production advances the stores it
//! opened ([`ProcessSecondaries`]); a test injects one that fails on cue.

use arc_swap::ArcSwapOption;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use ckb_store_reader::CkbChainReader;
use ckbadger_store::read_view;
use ckbadger_store::CkbadgerStore;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::watch;

use crate::response::{ApiError, ApiRouteError};
use crate::AppState;

/// How often the refresh loop advances the secondaries; also the retry
/// cadence of the initial catch-up.
pub const STORE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Consecutive failed catch-ups of one secondary after which data endpoints
/// answer `503 store_catching_up`.
pub const CATCH_UP_FAILURE_STRIKES: u32 = 3;

/// A secondary view this API process reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecondaryStore {
    AppendOnly,
    Domain,
    /// The CKB node's own RocksDB (`ckb-store-reader`), when it opened.
    CkbNode,
    /// The opt-in network-crawler store, once its primary exists.
    Network,
}

impl SecondaryStore {
    /// The name logs and error messages use.
    pub fn name(self) -> &'static str {
        match self {
            Self::AppendOnly => "append-only",
            Self::Domain => "domain",
            Self::CkbNode => "ckb node",
            Self::Network => "network",
        }
    }
}

/// How one secondary's catch-up ended in one round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchUpOutcome {
    pub store: SecondaryStore,
    /// The error text (with its context chain) when the catch-up failed.
    pub result: Result<(), String>,
}

impl CatchUpOutcome {
    pub fn new(store: SecondaryStore, result: anyhow::Result<()>) -> Self {
        Self {
            store,
            result: result.map_err(|error| format!("{error:#}")),
        }
    }
}

/// Advances every secondary view this process reads, together.
pub trait SecondaryCatchUp: Send + Sync {
    /// One catch-up round: every present secondary, domain first, one
    /// outcome each. Blocking — the refresh loop runs it on its own thread,
    /// never inside a pinned read view.
    fn catch_up_round(&self) -> Vec<CatchUpOutcome>;
}

/// The secondaries this API process opened: the production [`SecondaryCatchUp`].
pub struct ProcessSecondaries {
    append_only: Arc<CkbadgerStore>,
    domain: Arc<CkbadgerStore>,
    ckb_node: Option<Arc<CkbChainReader>>,
    network: Arc<ArcSwapOption<CkbadgerStore>>,
}

impl ProcessSecondaries {
    /// The secondaries `state` reads from.
    pub fn of(state: &AppState) -> Self {
        Self {
            append_only: state.append_only_store.clone(),
            domain: state.store.clone(),
            ckb_node: state.ckb_store.clone(),
            network: state.network_store.clone(),
        }
    }
}

impl ProcessSecondaries {
    /// One catch-up round, calling `after_each` with each store as soon as
    /// that store's catch-up has returned.
    ///
    /// The indexer keeps committing while a round runs, so a commit can land
    /// between two of the round's catch-ups; `after_each` is where a test
    /// lands one. Production passes a no-op ([`SecondaryCatchUp::catch_up_round`]).
    pub fn catch_up_round_observed(
        &self,
        after_each: &mut dyn FnMut(SecondaryStore),
    ) -> Vec<CatchUpOutcome> {
        // The opt-in network secondary may attach between rounds.
        let network = self.network.load_full();
        // One exclusive window for every secondary this process reads. It
        // waits for in-flight requests (each pins the view for its whole
        // lifetime) and blocks new ones, so no response can mix a
        // pre-catch-up index row with a post-catch-up entry.
        //
        // The window does not stop the writers. The indexer keeps
        // committing during the round, and each catch-up takes the primary
        // as of its own moment, so the order of the catch-ups decides which
        // store can come out ahead. Domain goes first, and every store whose
        // rows domain rows point at goes after it:
        // - append-only: the indexer commits a batch's cell payloads (synced)
        //   before the domain batch that points at them, so a domain row in
        //   the domain view had its payload committed before the domain
        //   catch-up, hence before the append-only catch-up that follows.
        //   Caught up the other way round, a batch committed between the two
        //   catch-ups leaves domain rows without their payloads (mainnet,
        //   2026-11-11: `/transactions` answered 500 `missing canonical
        //   output payload`). An append-only view ahead of domain is
        //   harmless: payloads are write-once and nothing reads one without
        //   the domain row that names it.
        // - the CKB node: the indexer indexes a block only after the node
        //   has committed it.
        let window = read_view::catch_up_window();
        let mut outcomes = Vec::with_capacity(4);
        let mut advance = |store: SecondaryStore, result: anyhow::Result<()>| {
            outcomes.push(CatchUpOutcome::new(store, result));
            after_each(store);
        };
        advance(
            SecondaryStore::Domain,
            self.domain.catch_up_in_window(&window),
        );
        advance(
            SecondaryStore::AppendOnly,
            self.append_only.catch_up_in_window(&window),
        );
        if let Some(ckb_node) = &self.ckb_node {
            // Owned by ckb-store-reader, which has no window type; it is
            // already inside ours.
            advance(SecondaryStore::CkbNode, ckb_node.refresh());
        }
        if let Some(network) = &network {
            advance(SecondaryStore::Network, network.catch_up_in_window(&window));
        }
        outcomes
    }
}

impl SecondaryCatchUp for ProcessSecondaries {
    fn catch_up_round(&self) -> Vec<CatchUpOutcome> {
        self.catch_up_round_observed(&mut |_| {})
    }
}

/// What failed, how many times in a row, and the latest error.
#[derive(Debug, Clone)]
struct Failures {
    consecutive: u32,
    last_error: String,
}

impl Failures {
    fn record(previous: Option<Failures>, error: &str) -> Self {
        let consecutive = match previous {
            None => 1,
            Some(previous) => previous.consecutive + 1,
        };
        Self {
            consecutive,
            last_error: error.to_owned(),
        }
    }

    fn blocks_serving(&self) -> bool {
        self.consecutive >= CATCH_UP_FAILURE_STRIKES
    }
}

#[derive(Debug)]
enum Phase {
    /// No round has caught up every secondary yet. `None` until the first
    /// attempt ends; then the latest failed attempt.
    FirstCatchUp { last_attempt: Option<Failures> },
    /// The first catch-up landed. Consecutive failures of each secondary
    /// (absent: its last catch-up succeeded), and of rounds that ended with
    /// no outcome at all (the round's task panicked).
    Serving {
        stores: BTreeMap<SecondaryStore, Failures>,
        rounds: Option<Failures>,
    },
}

/// Whether the API's secondary views may serve data (see the module docs).
pub struct StoreReadiness {
    phase: RwLock<Phase>,
    caught_up_once: watch::Sender<bool>,
}

impl Default for StoreReadiness {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreReadiness {
    /// A process that has opened its secondaries and caught none of them up.
    pub fn new() -> Self {
        Self {
            phase: RwLock::new(Phase::FirstCatchUp { last_attempt: None }),
            caught_up_once: watch::Sender::new(false),
        }
    }

    /// `Ok` when data may be served from the secondaries; otherwise the
    /// `503 store_catching_up` that says why not.
    pub fn check(&self) -> Result<(), ApiRouteError> {
        let phase = self.phase.read().expect("store readiness poisoned");
        match &*phase {
            Phase::FirstCatchUp { last_attempt: None } => Err(ApiError::store_catching_up(
                "the API's secondary stores have not completed their first catch-up with \
                 the indexer yet",
            )),
            Phase::FirstCatchUp {
                last_attempt: Some(attempt),
            } => Err(ApiError::store_catching_up(format!(
                "the API's secondary stores have not caught up with the indexer yet: \
                 attempt {} failed: {}",
                attempt.consecutive, attempt.last_error
            ))),
            Phase::Serving { stores, rounds } => {
                let mut blocking: Vec<String> = stores
                    .iter()
                    .filter(|(_, failures)| failures.blocks_serving())
                    .map(|(store, failures)| {
                        format!(
                            "{} store catch-up with the indexer failed {} consecutive times; \
                             last error: {}",
                            store.name(),
                            failures.consecutive,
                            failures.last_error
                        )
                    })
                    .collect();
                if let Some(failures) = rounds.as_ref().filter(|f| f.blocks_serving()) {
                    blocking.push(format!(
                        "store catch-up round failed {} consecutive times; last error: {}",
                        failures.consecutive, failures.last_error
                    ));
                }
                if blocking.is_empty() {
                    Ok(())
                } else {
                    Err(ApiError::store_catching_up(blocking.join("; ")))
                }
            }
        }
    }

    /// Resolves once one round has caught up every secondary.
    pub async fn caught_up_once(&self) {
        self.caught_up_once
            .subscribe()
            .wait_for(|caught_up| *caught_up)
            .await
            .expect("the readiness sender lives as long as the readiness it is waited on");
    }

    /// Run one catch-up round and record it. Blocking (see
    /// [`SecondaryCatchUp::catch_up_round`]).
    pub fn run_round(&self, secondaries: &dyn SecondaryCatchUp) {
        self.record_round(&secondaries.catch_up_round());
    }

    /// Record a finished round's outcomes.
    pub fn record_round(&self, outcomes: &[CatchUpOutcome]) {
        if outcomes.is_empty() {
            // Every round advances at least append-only and domain.
            self.record_failed_round("the round advanced no secondary");
            return;
        }
        let mut phase = self.phase.write().expect("store readiness poisoned");
        match &mut *phase {
            Phase::FirstCatchUp { last_attempt } => {
                let failed: Vec<String> = outcomes
                    .iter()
                    .filter_map(|outcome| match &outcome.result {
                        Ok(()) => None,
                        Err(error) => Some(format!("{} store: {error}", outcome.store.name())),
                    })
                    .collect();
                if failed.is_empty() {
                    let attempts = match last_attempt {
                        None => 1,
                        Some(failed) => failed.consecutive + 1,
                    };
                    tracing::info!(
                        attempts,
                        "secondary stores caught up with the indexer; serving data endpoints \
                         and starting warmup"
                    );
                    *phase = Phase::Serving {
                        stores: BTreeMap::new(),
                        rounds: None,
                    };
                    drop(phase);
                    self.caught_up_once.send_replace(true);
                } else {
                    let attempt = Failures::record(last_attempt.take(), &failed.join("; "));
                    log_failed_first_catch_up(&attempt);
                    *last_attempt = Some(attempt);
                }
            }
            Phase::Serving { stores, rounds } => {
                if let Some(failures) = rounds.take() {
                    log_recovery("store catch-up round", &failures);
                }
                for outcome in outcomes {
                    let name = outcome.store.name();
                    match &outcome.result {
                        Ok(()) => {
                            if let Some(failures) = stores.remove(&outcome.store) {
                                log_recovery(name, &failures);
                            }
                        }
                        Err(error) => {
                            let failures = Failures::record(stores.remove(&outcome.store), error);
                            log_steady_failure(name, &failures);
                            stores.insert(outcome.store, failures);
                        }
                    }
                }
            }
        }
    }

    /// Record a round that ended with no outcome (its task panicked, or it
    /// advanced no secondary).
    pub fn record_failed_round(&self, error: &str) {
        let mut phase = self.phase.write().expect("store readiness poisoned");
        match &mut *phase {
            Phase::FirstCatchUp { last_attempt } => {
                let attempt = Failures::record(last_attempt.take(), error);
                log_failed_first_catch_up(&attempt);
                *last_attempt = Some(attempt);
            }
            Phase::Serving { rounds, .. } => {
                let failures = Failures::record(rounds.take(), error);
                log_steady_failure("store catch-up round", &failures);
                *rounds = Some(failures);
            }
        }
    }
}

fn log_failed_first_catch_up(attempt: &Failures) {
    tracing::error!(
        attempt = attempt.consecutive,
        error = %attempt.last_error,
        "initial secondary catch-up failed; data endpoints answer 503 store_catching_up and \
         warmup waits until a catch-up succeeds (retrying every {}s)",
        STORE_REFRESH_INTERVAL.as_secs()
    );
}

fn log_steady_failure(what: &str, failures: &Failures) {
    if failures.blocks_serving() {
        tracing::error!(
            store = what,
            consecutive = failures.consecutive,
            error = %failures.last_error,
            "secondary catch-up keeps failing; data endpoints answer 503 store_catching_up \
             until it succeeds"
        );
    } else {
        tracing::warn!(
            store = what,
            consecutive = failures.consecutive,
            error = %failures.last_error,
            "secondary catch-up failed; serving the previous view (503 after {} consecutive \
             failures)",
            CATCH_UP_FAILURE_STRIKES
        );
    }
}

fn log_recovery(what: &str, failures: &Failures) {
    if failures.blocks_serving() {
        tracing::info!(
            store = what,
            failed_rounds = failures.consecutive,
            "secondary catch-up succeeded again; data endpoints serving"
        );
    }
}

/// Advance the secondaries now and every [`STORE_REFRESH_INTERVAL`] after,
/// recording each round in `readiness`. The first round runs at once: it is
/// the initial catch-up everything else waits for.
pub async fn run_store_refresh_loop(
    readiness: Arc<StoreReadiness>,
    secondaries: Arc<dyn SecondaryCatchUp>,
) {
    loop {
        let round_readiness = readiness.clone();
        let round = secondaries.clone();
        if let Err(error) =
            tokio::task::spawn_blocking(move || round_readiness.run_round(round.as_ref())).await
        {
            readiness.record_failed_round(&format!("catch-up round task failed: {error}"));
        }
        tokio::time::sleep(STORE_REFRESH_INTERVAL).await;
    }
}

/// Answer `503 store_catching_up` instead of running the handler while
/// [`StoreReadiness::check`] rejects.
pub async fn require_caught_up_stores(
    State(readiness): State<Arc<StoreReadiness>>,
    request: Request,
    next: Next,
) -> Response {
    match readiness.check() {
        Ok(()) => next.run(request).await,
        Err(rejection) => rejection.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn ok(store: SecondaryStore) -> CatchUpOutcome {
        CatchUpOutcome {
            store,
            result: Ok(()),
        }
    }

    fn failed(store: SecondaryStore, error: &str) -> CatchUpOutcome {
        CatchUpOutcome {
            store,
            result: Err(error.to_owned()),
        }
    }

    fn rejection(readiness: &StoreReadiness) -> String {
        let (status, body) = readiness
            .check()
            .expect_err("data endpoints must answer 503");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0.error, "store_catching_up");
        body.0.message.clone()
    }

    fn caught_up() -> StoreReadiness {
        let readiness = StoreReadiness::new();
        readiness.record_round(&[ok(SecondaryStore::AppendOnly), ok(SecondaryStore::Domain)]);
        readiness.check().expect("caught up once");
        readiness
    }

    thread_local! {
        static CAPTURE: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
    }

    /// Writes a formatted event to the emitting thread's capture, if any.
    struct ThreadCaptureWriter;

    impl std::io::Write for ThreadCaptureWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            CAPTURE.with(|capture| {
                if let Some(buffer) = capture.borrow_mut().as_mut() {
                    buffer.extend_from_slice(bytes);
                }
            });
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Every event this module logs while `body` runs on this thread.
    ///
    /// One process-wide subscriber hands each event to the emitting thread's
    /// capture: a per-thread default subscriber can miss a callsite another
    /// test's thread registered first under no subscriber.
    fn logged(body: impl FnOnce()) -> String {
        static SUBSCRIBER: std::sync::Once = std::sync::Once::new();
        SUBSCRIBER.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_max_level(tracing::Level::TRACE)
                .with_writer(|| ThreadCaptureWriter)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("the only global subscriber of the API's unit-test binary");
        });
        CAPTURE.with(|capture| *capture.borrow_mut() = Some(Vec::new()));
        body();
        let bytes = CAPTURE.with(|capture| capture.borrow_mut().take().expect("capture started"));
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn nothing_is_served_before_the_first_round() {
        let readiness = StoreReadiness::new();
        assert!(rejection(&readiness).contains("have not completed their first catch-up"));
    }

    #[test]
    fn a_round_with_any_failed_secondary_is_not_the_first_catch_up() {
        let readiness = StoreReadiness::new();
        for attempt in 1..=2 {
            let log = logged(|| {
                readiness.record_round(&[
                    ok(SecondaryStore::AppendOnly),
                    failed(SecondaryStore::Domain, "MANIFEST replay failed"),
                    ok(SecondaryStore::CkbNode),
                ]);
            });
            assert!(log.contains("ERROR"), "{log}");
            assert!(log.contains(&format!("attempt={attempt}")), "{log}");
            let message = rejection(&readiness);
            assert!(
                message.contains(&format!(
                    "attempt {attempt} failed: domain store: MANIFEST replay failed"
                )),
                "{message}"
            );
        }

        let log = logged(|| {
            readiness.record_round(&[ok(SecondaryStore::AppendOnly), ok(SecondaryStore::Domain)]);
        });
        assert!(log.contains("INFO") && log.contains("attempts=3"), "{log}");
        readiness
            .check()
            .expect("the first successful round serves");
    }

    #[test]
    fn a_round_that_advanced_nothing_is_not_a_catch_up() {
        let readiness = StoreReadiness::new();
        readiness.record_round(&[]);
        assert!(rejection(&readiness).contains("the round advanced no secondary"));

        let readiness = caught_up();
        for _ in 0..CATCH_UP_FAILURE_STRIKES {
            readiness.record_round(&[]);
        }
        assert!(rejection(&readiness).contains("the round advanced no secondary"));
    }

    #[tokio::test]
    async fn caught_up_once_resolves_only_after_a_successful_round() {
        let readiness = Arc::new(StoreReadiness::new());
        let waiter = tokio::spawn({
            let readiness = readiness.clone();
            async move { readiness.caught_up_once().await }
        });
        readiness.record_round(&[
            ok(SecondaryStore::AppendOnly),
            failed(SecondaryStore::Domain, "x"),
        ]);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !waiter.is_finished(),
            "a failed round must not release the warmup"
        );

        readiness.record_round(&[ok(SecondaryStore::AppendOnly), ok(SecondaryStore::Domain)]);
        tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("the first successful round releases the warmup")
            .unwrap();
        // Later waiters do not wait at all.
        tokio::time::timeout(Duration::from_secs(1), readiness.caught_up_once())
            .await
            .expect("already caught up");
    }

    #[test]
    fn three_consecutive_failures_of_one_store_stop_serving_until_it_succeeds() {
        let readiness = caught_up();

        for strike in 1..=2 {
            let log = logged(|| {
                readiness.record_round(&[
                    ok(SecondaryStore::AppendOnly),
                    failed(SecondaryStore::Domain, &format!("strike {strike}")),
                ]);
            });
            assert!(log.contains("WARN") && !log.contains("ERROR"), "{log}");
            readiness
                .check()
                .unwrap_or_else(|_| panic!("{strike} failures keep serving"));
        }

        for strike in 3..=4 {
            let log = logged(|| {
                readiness.record_round(&[
                    ok(SecondaryStore::AppendOnly),
                    failed(SecondaryStore::Domain, &format!("strike {strike}")),
                ]);
            });
            assert!(log.contains("ERROR") && !log.contains("WARN"), "{log}");
            let message = rejection(&readiness);
            assert!(
                message.contains(&format!(
                    "domain store catch-up with the indexer failed {strike} consecutive times; \
                     last error: strike {strike}"
                )),
                "{message}"
            );
        }

        let log = logged(|| {
            readiness.record_round(&[ok(SecondaryStore::AppendOnly), ok(SecondaryStore::Domain)]);
        });
        assert!(
            log.contains("INFO") && log.contains("failed_rounds=4"),
            "{log}"
        );
        readiness.check().expect("the first success serves again");
    }

    #[test]
    fn strikes_count_per_store_not_per_round() {
        let readiness = caught_up();
        // Domain and network alternate failing: neither fails three times in a row.
        for round in 0..6 {
            let (domain, network) = if round % 2 == 0 {
                (
                    failed(SecondaryStore::Domain, "d"),
                    ok(SecondaryStore::Network),
                )
            } else {
                (
                    ok(SecondaryStore::Domain),
                    failed(SecondaryStore::Network, "n"),
                )
            };
            readiness.record_round(&[ok(SecondaryStore::AppendOnly), domain, network]);
            readiness
                .check()
                .expect("no single store failed three times in a row");
        }

        // A success of another store does not reset this one's count.
        for _ in 0..3 {
            readiness.record_round(&[
                failed(SecondaryStore::AppendOnly, "append-only lag"),
                ok(SecondaryStore::Domain),
                ok(SecondaryStore::Network),
            ]);
        }
        assert!(rejection(&readiness).contains("append-only store catch-up"));
    }

    #[test]
    fn rounds_without_outcomes_count_as_failures_too() {
        let readiness = StoreReadiness::new();
        readiness.record_failed_round("catch-up round task failed: panicked");
        assert!(rejection(&readiness).contains("attempt 1 failed: catch-up round task failed"));

        let readiness = caught_up();
        for _ in 0..2 {
            readiness.record_failed_round("task panicked");
            readiness.check().expect("two failed rounds keep serving");
        }
        readiness.record_failed_round("task panicked");
        assert!(rejection(&readiness).contains("round failed 3 consecutive times"));
        readiness.record_round(&[ok(SecondaryStore::AppendOnly), ok(SecondaryStore::Domain)]);
        readiness.check().expect("a completed round clears it");
    }

    /// Domain first, then the stores its rows point at (see
    /// `catch_up_round_observed`); the outcomes come in the order the stores
    /// advanced.
    #[test]
    fn process_secondaries_advance_every_present_store_domain_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let network_dir = tempfile::tempdir().unwrap();
        let network = Arc::new(CkbadgerStore::open_test_network(network_dir.path()).unwrap());
        let slot = Arc::new(ArcSwapOption::from(None));
        let secondaries = ProcessSecondaries {
            append_only: store.clone(),
            domain: store,
            ckb_node: None,
            network: slot.clone(),
        };

        assert_eq!(
            secondaries.catch_up_round(),
            vec![ok(SecondaryStore::Domain), ok(SecondaryStore::AppendOnly)]
        );
        // The network secondary joins the round once it is attached.
        slot.store(Some(network));
        assert_eq!(
            secondaries.catch_up_round(),
            vec![
                ok(SecondaryStore::Domain),
                ok(SecondaryStore::AppendOnly),
                ok(SecondaryStore::Network)
            ]
        );
    }
}
