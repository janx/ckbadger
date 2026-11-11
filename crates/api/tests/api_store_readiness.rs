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

use ckbadger_api::store_readiness::{
    CatchUpOutcome, ProcessSecondaries, SecondaryCatchUp, SecondaryStore,
};
use ckbadger_api::try_create_app;
use ckbadger_store::types::SyncStatus;
use std::path::Path;
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

/// The fixture chain's block `number`: one cellbase with one output.
fn fixture_cellbase(number: i64) -> [u8; 32] {
    let mut hash = [0xCB; 32];
    hash[24..].copy_from_slice(&number.to_be_bytes());
    hash
}

fn fixture_block_hash(number: i64) -> Vec<u8> {
    let mut hash = vec![0xB1; 32];
    hash[24..].copy_from_slice(&number.to_be_bytes());
    hash
}

fn fixture_output_capacity(number: i64) -> i64 {
    6_100_000_000 + number
}

/// The indexer's two primaries, written the way its live writer
/// (`sync/batch.rs`) commits a batch, reduced to what `/transactions` reads:
/// the cell payloads to the append-only store in a synced commit first, then
/// the header, transaction index and sync status to the domain store in one
/// batch. Payloads must be durable before the domain rows that point at them
/// (crash safety), so this order is the writer's, and every reader has to
/// live with it.
struct IndexerPrimaries {
    domain: CkbadgerStore,
    append_only: CkbadgerStore,
}

impl IndexerPrimaries {
    /// Production's layout under `root`: `domain/` and `append-only/`.
    fn open(root: &Path) -> Self {
        Self {
            domain: CkbadgerStore::open_domain(root.join("domain")).unwrap(),
            append_only: CkbadgerStore::open_append_only(root.join("append-only")).unwrap(),
        }
    }

    fn commit_block(&self, number: i64) {
        let tx_hash = fixture_cellbase(number);
        let timestamp = 1_700_000_000_000 + number * 8_000;

        let mut payloads = StoreBatch::new(&self.append_only);
        payloads
            .put_cell_payload_by_outpoint(
                &tx_hash,
                0,
                &LiveCellInfo {
                    capacity: fixture_output_capacity(number),
                    lock_code_hash: [0x21; 32],
                    lock_hash_type: 1,
                    lock_args: vec![0x22; 20],
                    type_code_hash: None,
                    type_hash_type: None,
                    type_args: None,
                    data_size: 0,
                    udt_amount: None,
                    data_hash: None,
                },
            )
            .unwrap();
        payloads.commit_synced().unwrap();

        let mut rows = StoreBatch::new(&self.domain);
        put_test_block_header(&mut rows, number, &fixture_block_hash(number), timestamp);
        rows.put_tx_hash_map(&tx_hash, number, 0);
        rows.put_tx_index(
            number,
            0,
            &TxIndexEntry {
                tx_hash,
                is_cellbase: true,
                timestamp,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 1,
                cycles: None,
                semantic_tags: 0,
            },
        );
        rows.put_sync_status(&SyncStatus {
            tip_block_number: number,
            tip_block_hash: fixture_block_hash(number),
            total_transactions: number,
            ..Default::default()
        })
        .unwrap();
        rows.commit().unwrap();
    }

    /// The API's two secondaries of these primaries, as production opens them.
    fn api_secondaries(&self, root: &Path) -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
        (
            Arc::new(
                CkbadgerStore::open_domain_secondary(
                    root.join("domain").as_path(),
                    root.join("domain-secondary").as_path(),
                )
                .unwrap(),
            ),
            Arc::new(
                CkbadgerStore::open_append_only_secondary(
                    root.join("append-only").as_path(),
                    root.join("append-only-secondary").as_path(),
                )
                .unwrap(),
            ),
        )
    }
}

/// The blocks whose cellbases `GET /transactions?limit=20` serves, newest
/// first, each row's hash and output capacity checked against the fixture and
/// the page's total against its rows: one coherent view.
async fn served_cellbases(app: &axum::Router, context: &str) -> Vec<i64> {
    let (status, json) = get_json(app, "/transactions?limit=20").await;
    assert_eq!(status, StatusCode::OK, "{context}: {json}");
    let rows = json["data"].as_array().expect("data");
    assert_eq!(
        json["total"].as_u64(),
        Some(rows.len() as u64),
        "{context}: the count and the rows come from one view: {json}"
    );
    rows.iter()
        .map(|row| {
            let number = row["blockNumber"].as_i64().expect("blockNumber");
            assert_eq!(
                row["hash"],
                format!("0x{}", hex::encode(fixture_cellbase(number))),
                "{context}: {row}"
            );
            assert_eq!(
                row["outputCapacity"],
                fixture_output_capacity(number).to_string(),
                "{context}: {row}"
            );
            number
        })
        .collect()
}

/// The indexer keeps committing while the API's refresh round runs, and it
/// commits a block's payloads (append-only) before the domain rows that point
/// at them. Whichever two of the round's catch-ups a commit lands between,
/// the view the round leaves must never hold a domain row whose payload the
/// append-only view lacks.
///
/// Mainnet, 2026-11-11 08:34:26: block 20983950 committed while a round ran;
/// the round had advanced append-only just before the block's payload commit
/// and domain just after its domain commit, so for the second until the next
/// round the domain view held the block's transactions and the append-only
/// view not their outputs, and `GET /transactions?limit=20` answered 500
/// (`missing canonical output payload`) once in that second.
#[tokio::test]
async fn a_block_committed_during_a_catch_up_round_never_serves_rows_without_their_payloads() {
    // The test state opens no node reader and no network store: a round
    // advances the domain and append-only secondaries, so a commit can land
    // after either one.
    for gap in 0..2 {
        let root = tempfile::tempdir().unwrap();
        let indexer = IndexerPrimaries::open(root.path());
        indexer.commit_block(1);
        let (domain, append_only) = indexer.api_secondaries(root.path());
        // Its first round catches block 1 up.
        let state = test_app_state(test_config_with_append_only(domain, append_only));
        let app = router_from_state(state.clone());
        assert_eq!(served_cellbases(&app, "after the first round").await, [1]);

        let mut steps = 0;
        let mut landed_after = None;
        let outcomes = ProcessSecondaries::of(&state).catch_up_round_observed(&mut |store| {
            if steps == gap {
                indexer.commit_block(2);
                landed_after = Some(store);
            }
            steps += 1;
        });
        assert_eq!(steps, 2, "{outcomes:?}");
        assert!(
            outcomes.iter().all(|outcome| outcome.result.is_ok()),
            "{outcomes:?}"
        );
        let context = format!(
            "block 2 committed after the round advanced the {} store",
            landed_after.expect("the commit landed").name()
        );
        // Block 2 is served only once its payload is in the view too.
        let served = served_cellbases(&app, &context).await;
        assert!(served == [1] || served == [2, 1], "{context}: {served:?}");

        // The next round sees everything the indexer committed.
        ProcessSecondaries::of(&state).catch_up_round();
        assert_eq!(served_cellbases(&app, "after the next round").await, [2, 1]);
    }
}
