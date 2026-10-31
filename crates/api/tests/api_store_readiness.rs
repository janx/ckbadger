//! J7: the API serves data only from secondary views that have caught up with
//! the indexer. One successful catch-up of every secondary comes before
//! warmup and before any data endpoint answers; afterwards three consecutive
//! failed catch-ups of one secondary turn data endpoints into
//! `503 store_catching_up` until that secondary catches up again.
//!
//! The refresh loop runs an injected [`SecondaryCatchUp`] that waits, every
//! round, for the test to cue the domain store's outcome. It opens no catch-up
//! window, so a round waiting for its cue never holds up a request.

use crate::common::*;

use ckbadger_api::store_readiness::{CatchUpOutcome, SecondaryCatchUp, SecondaryStore};
use ckbadger_api::try_create_app;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Longest a test waits for the refresh loop to reach its next round (the
/// loop's cadence is one second).
const ROUND_WAIT: Duration = Duration::from_secs(10);

/// A refresh-loop seam whose every round waits for the test's cue.
struct CuedCatchUp {
    started: mpsc::UnboundedSender<()>,
    cues: std::sync::Mutex<mpsc::UnboundedReceiver<Result<(), String>>>,
}

impl SecondaryCatchUp for CuedCatchUp {
    fn catch_up_round(&self) -> Vec<CatchUpOutcome> {
        // A send fails only once the test has returned and dropped its side.
        self.started.send(()).ok();
        let domain = self
            .cues
            .lock()
            .expect("cue receiver poisoned")
            .blocking_recv()
            // The test is over: nothing reads the outcome any more.
            .unwrap_or(Ok(()));
        vec![
            CatchUpOutcome {
                store: SecondaryStore::AppendOnly,
                result: Ok(()),
            },
            CatchUpOutcome {
                store: SecondaryStore::Domain,
                result: domain,
            },
        ]
    }
}

/// The test's side of a [`CuedCatchUp`].
struct Cue {
    started: mpsc::UnboundedReceiver<()>,
    cues: mpsc::UnboundedSender<Result<(), String>>,
}

impl Cue {
    fn new() -> (Self, Arc<CuedCatchUp>) {
        let (started_tx, started_rx) = mpsc::unbounded_channel();
        let (cues_tx, cues_rx) = mpsc::unbounded_channel();
        (
            Self {
                started: started_rx,
                cues: cues_tx,
            },
            Arc::new(CuedCatchUp {
                started: started_tx,
                cues: std::sync::Mutex::new(cues_rx),
            }),
        )
    }

    /// Wait until the refresh loop starts its next round. By then the
    /// previous round's outcome is recorded: the loop records a round before
    /// it sleeps towards the next one.
    async fn next_round(&mut self) {
        tokio::time::timeout(ROUND_WAIT, self.started.recv())
            .await
            .expect("the refresh loop reaches its next round")
            .expect("the refresh loop is alive");
    }

    /// End the round that is waiting: the domain store's catch-up outcome.
    fn answer(&self, domain: Result<(), &str>) {
        self.cues
            .send(domain.map_err(str::to_owned))
            .expect("a round is waiting for its cue");
    }
}

fn warmup_started(state: &AppState) -> bool {
    state
        .background_tasks
        .read()
        .unwrap()
        .tasks
        .iter()
        .any(|task| task.name == "api_cache_refresh")
}

fn assert_store_catching_up(status: StatusCode, json: &serde_json::Value, needles: &[&str]) {
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(json["error"], "store_catching_up", "{json}");
    let message = json["message"].as_str().expect("message");
    for needle in needles {
        assert!(message.contains(needle), "{needle:?} not in {message:?}");
    }
}

/// (a) Until one round has caught up every secondary, data endpoints answer
/// `503 store_catching_up` naming the failed attempt, and no warmup starts;
/// node-RPC-only endpoints stay up. (b) The first successful round starts the
/// warmup and the endpoint answers.
#[tokio::test]
async fn data_endpoints_and_warmup_wait_for_the_first_successful_catch_up() {
    let rpc = MockServer::start().await;
    mount_tx_pool_rpc(&rpc, &[]).await;
    let (mut cue, catch_up) = Cue::new();
    let mut config = test_config(test_store());
    config.ckb_rpc_url = rpc.uri();
    // Production wiring: the refresh loops own the warmup.
    config.start_background_tasks = true;
    config.pool_mirror_enabled = false;
    config.secondary_catch_up = Some(catch_up);
    let (state, app) = try_create_app(config).await.expect("router");

    cue.next_round().await;
    cue.answer(Err("injected domain catch-up failure #1"));
    cue.next_round().await;

    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_store_catching_up(
        status,
        &json,
        &["attempt 1", "domain", "injected domain catch-up failure #1"],
    );
    assert!(
        !warmup_started(&state),
        "warmup must not start before the first successful catch-up"
    );
    assert!(state.token_cache.load().is_none());

    // Read from the node's RPC only: no secondary view to wait for.
    let (status, json) = get_json(&app, "/mempool/transactions").await;
    assert_eq!(status, StatusCode::OK, "{json}");

    cue.answer(Err("injected domain catch-up failure #2"));
    cue.next_round().await;
    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_store_catching_up(
        status,
        &json,
        &["attempt 2", "injected domain catch-up failure #2"],
    );
    assert!(!warmup_started(&state));

    cue.answer(Ok(()));
    cue.next_round().await;

    // The gate is open; the warmup runs and `/tokens` answers once it lands.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (status, json) = get_json(&app, "/tokens?limit=2").await;
        if status == StatusCode::OK {
            break;
        }
        assert_eq!(json["error"], "warmup_pending", "{json}");
        assert!(Instant::now() < deadline, "warmup never landed: {json}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(warmup_started(&state));
}

/// (c) After the first catch-up, two consecutive failures keep serving, the
/// third turns data endpoints into `503 store_catching_up` with the last error
/// text, and the store's next success serves again. Embedded mode (no
/// background loops) still warms up only after the first catch-up.
#[tokio::test]
async fn three_consecutive_failed_catch_ups_turn_data_endpoints_into_503() {
    let (mut cue, catch_up) = Cue::new();
    let mut config = test_config(test_store());
    config.secondary_catch_up = Some(catch_up);
    let app = tokio::spawn(try_create_app(config));

    cue.next_round().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !app.is_finished(),
        "embedded mode must not warm up or serve before the first catch-up"
    );
    cue.answer(Ok(()));
    let (_state, app) = app.await.unwrap().expect("router");
    cue.next_round().await;

    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_eq!(status, StatusCode::OK, "{json}");

    for strike in 1..=2 {
        cue.answer(Err(&format!("injected domain catch-up failure #{strike}")));
        cue.next_round().await;
        let (status, json) = get_json(&app, "/tokens?limit=2").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{strike} consecutive failures must not stop serving: {json}"
        );
    }

    cue.answer(Err("injected domain catch-up failure #3"));
    cue.next_round().await;
    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_store_catching_up(
        status,
        &json,
        &[
            "domain",
            "3 consecutive",
            "injected domain catch-up failure #3",
        ],
    );

    cue.answer(Err("injected domain catch-up failure #4"));
    cue.next_round().await;
    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_store_catching_up(
        status,
        &json,
        &["4 consecutive", "injected domain catch-up failure #4"],
    );

    cue.answer(Ok(()));
    cue.next_round().await;
    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the store's next success must serve again: {json}"
    );
}
