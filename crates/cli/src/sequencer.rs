//! Orchestrator bulk-sync sequencer: start network indexers one at a time so only
//! one network bulk-syncs at a time. Pure decision logic + a generic gating loop;
//! the prod store-reader + spawn wiring live in `supervisor.rs`.

use std::time::Duration;

use anyhow::Result;
use tokio::sync::watch;
use tracing::{error, info};

/// How often the gate restates that it is still waiting. A deferred network can
/// legitimately wait many hours for a mainnet bulk sync, so this must be quiet
/// enough to live in a log file and loud enough that "nothing is happening" is
/// never indistinguishable from "the supervisor forgot".
const WAIT_LOG_INTERVAL: Duration = Duration::from_secs(60);

/// Whether a watched network is out of bulk sync (safe to start the next indexer).
///
/// `bulk_completed` = its `SyncStatus.bulk_sync_completed_at.is_some()`.
/// `lag` = `sync-progress target_block − current_block` (as i128; `None` when no
/// progress record exists yet). i128 preserves the exact difference between two
/// u64 heights, so caught-up/ahead (lag ≤ 0)
/// reads as past-bulk without masking anything.
pub(crate) fn is_past_bulk(bulk_completed: bool, lag: Option<i128>, threshold: u64) -> bool {
    bulk_completed || matches!(lag, Some(l) if l <= i128::from(threshold))
}

/// Everything the gate knows about the gating (previous) network this round.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GateStatus {
    /// `Some(true|false)` on a read; `None` when the round carried no signal
    /// (store not created yet, or an unreadable round). `None` never advances.
    pub past_bulk: Option<bool>,
    /// `target_block − current_block`, when a progress record was read.
    pub lag: Option<i128>,
    /// The gating network's `SyncStatus.bulk_sync_completed_at.is_some()`.
    pub bulk_completed: bool,
    /// `Some(reason)` while the supervisor has the gating indexer restart-blocked.
    /// A blocked gate never advances and is never torn down — it is reported.
    pub blocked: Option<String>,
}

/// Number of polls between "still waiting" logs at `poll` cadence, at least 1 so
/// a poll interval longer than [`WAIT_LOG_INTERVAL`] still logs every round.
pub(crate) fn wait_log_every(poll: Duration) -> u32 {
    let poll_ms = poll.as_millis().max(1);
    u32::try_from(WAIT_LOG_INTERVAL.as_millis() / poll_ms)
        .unwrap_or(u32::MAX)
        .max(1)
}

/// What a spawn attempt did, from the gate's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnOutcome {
    /// The child started; the gate advances to the next network.
    Started,
    /// The spawn failed after exhausting its retries. BULK_SYNC rule 11 forbids
    /// skipping ahead, and one network's spawn failure must not tear the others
    /// down, so the gate stays PARKED on this network: it never advances, never
    /// shuts anything down, and re-attempts on later polls.
    Parked,
}

/// Start deferred indexers one at a time. `names[0]`'s indexer is assumed already
/// spawned by the caller. For each `i` in `1..names.len()`, poll until `names[i-1]`
/// is past bulk, then `spawn(i)`. Generic over the observer and spawn action so
/// the gating logic is testable without real stores/processes.
///
/// `names` are the sequenced indexers' child labels, in `[[network]]` order, and
/// exist so every decision the gate makes says WHICH network it is about.
///
/// `observe(prev)` reports the gating network's [`GateStatus`]. It is `async`
/// because the real supervisor callback hands the blocking RocksDB secondary
/// open/refresh/read to `spawn_blocking` — a sequencer poll must never block a
/// runtime worker for the duration of a RocksDB catch-up — and because reading
/// the gating child's health takes the `SupervisorState` `tokio::sync::Mutex`.
///
/// `spawn(i)` is `async`: the real supervisor callback appends to the shared
/// `SupervisorState` behind a `tokio::sync::Mutex`, which must be awaited (a
/// `blocking_lock` in this async context would panic), so the gate is async. It
/// reports [`SpawnOutcome::Parked`] rather than `Err` when a child could not be
/// started, so a spawn failure stalls exactly one network instead of shutting the
/// whole orchestrator down.
///
/// Waiting is never silent, and a blocked gate never causes a skip or a teardown
/// (BULK_SYNC rule 11): the loop announces each wait, restates it once per
/// [`WAIT_LOG_INTERVAL`], and logs an ERROR on each transition into a blocked
/// gating indexer — then keeps waiting.
pub(crate) async fn sequence_indexers<O, OFut, S, SFut>(
    names: &[String],
    mut observe: O,
    mut spawn: S,
    poll: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()>
where
    O: FnMut(usize) -> OFut,
    OFut: std::future::Future<Output = Result<GateStatus>>,
    S: FnMut(usize) -> SFut,
    SFut: std::future::Future<Output = Result<SpawnOutcome>>,
{
    let log_every = wait_log_every(poll);
    for i in 1..names.len() {
        let gating = names[i - 1].as_str();
        let deferred = names[i].as_str();
        let mut ticks: u32 = 0;
        // Logged on each TRANSITION into/out of blocked, not per tick: a gate can
        // stay blocked for hours and must not drown the log.
        let mut blocked_logged = false;
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            let status = observe(i - 1).await?;

            match &status.blocked {
                Some(reason) if !blocked_logged => {
                    error!(
                        gating = %gating,
                        deferred = %deferred,
                        reason = %reason,
                        "network '{gating}' indexer is blocked; network '{deferred}' will not \
                         start until it is resolved. The sequencer keeps waiting — it never skips \
                         ahead and never stops the running networks"
                    );
                    blocked_logged = true;
                }
                None if blocked_logged => {
                    info!(
                        gating = %gating,
                        deferred = %deferred,
                        "gating indexer is no longer blocked; resuming the wait for it to pass bulk"
                    );
                    blocked_logged = false;
                }
                _ => {}
            }

            if status.blocked.is_none() && status.past_bulk == Some(true) {
                info!(
                    gating = %gating,
                    deferred = %deferred,
                    lag = ?status.lag,
                    bulk_completed = status.bulk_completed,
                    "gating network passed bulk sync; starting the next indexer"
                );
                // Parked: stay on this network. Fall through to the poll sleep so
                // the gate keeps observing (and keeps retrying) instead of
                // advancing past an indexer that never started.
                if spawn(i).await? == SpawnOutcome::Started {
                    break;
                }
            } else if ticks.is_multiple_of(log_every) {
                info!(
                    gating = %gating,
                    deferred = %deferred,
                    lag = ?status.lag,
                    bulk_completed = status.bulk_completed,
                    has_signal = status.past_bulk.is_some(),
                    "waiting for gating network to pass bulk sync before starting the next indexer"
                );
            }
            ticks = ticks.wrapping_add(1);

            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("net{i}/indexer")).collect()
    }

    /// A gate that is readable and not blocked.
    fn signal(past_bulk: Option<bool>) -> GateStatus {
        GateStatus {
            past_bulk,
            ..GateStatus::default()
        }
    }

    #[test]
    fn past_bulk_completed_flag_wins() {
        assert!(is_past_bulk(true, None, 1000));
        assert!(is_past_bulk(true, Some(9_999), 1000));
    }

    #[test]
    fn not_past_bulk_when_far_behind() {
        assert!(!is_past_bulk(false, Some(1_900), 1000));
    }

    #[test]
    fn past_bulk_within_or_at_threshold() {
        assert!(is_past_bulk(false, Some(1_000), 1000));
        assert!(is_past_bulk(false, Some(500), 1000));
    }

    #[test]
    fn past_bulk_when_caught_up_or_ahead() {
        assert!(is_past_bulk(false, Some(0), 1000));
        assert!(is_past_bulk(false, Some(-1), 1000));
    }

    #[test]
    fn not_past_bulk_without_progress() {
        assert!(!is_past_bulk(false, None, 1000));
    }

    #[tokio::test]
    async fn spawns_each_indexer_in_order_only_after_prev_past_bulk() {
        let (_tx, rx) = watch::channel(false);
        let calls = RefCell::new(0usize);
        let spawned = RefCell::new(Vec::<usize>::new());
        sequence_indexers(
            &names(3),
            // false, true, false, true -> each network waits one poll before advancing
            |_prev| {
                let calls = &calls;
                async move {
                    let mut c = calls.borrow_mut();
                    *c += 1;
                    Ok(signal(Some((*c).is_multiple_of(2))))
                }
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();
        assert_eq!(*spawned.borrow(), vec![1, 2]);
        assert!(
            *calls.borrow() >= 4,
            "polled while waiting for each network"
        );
    }

    #[tokio::test]
    async fn single_indexer_starts_with_no_polling() {
        let (_tx, rx) = watch::channel(false);
        let calls = RefCell::new(0usize);
        let spawned = RefCell::new(Vec::<usize>::new());
        sequence_indexers(
            &names(1),
            |_prev| {
                let calls = &calls;
                async move {
                    *calls.borrow_mut() += 1;
                    Ok(signal(Some(true)))
                }
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();
        assert!(spawned.borrow().is_empty());
        assert_eq!(*calls.borrow(), 0);
    }

    #[tokio::test]
    async fn shutdown_stops_before_spawning() {
        let (tx, rx) = watch::channel(false);
        tx.send(true).unwrap();
        let spawned = RefCell::new(Vec::<usize>::new());
        sequence_indexers(
            &names(3),
            |_prev| async { Ok(signal(Some(false))) },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();
        assert!(spawned.borrow().is_empty());
    }

    #[tokio::test]
    async fn status_read_task_failure_stops_sequence_without_spawning() {
        // The reader's `Err` channel is now reserved for supervisor-internal bugs
        // (a panicked `spawn_blocking` task). Store read/decode failures never get
        // here any more — they arrive as `Ok(None)`, covered below.
        let (_tx, rx) = watch::channel(false);
        let spawned = RefCell::new(Vec::<usize>::new());
        let err = sequence_indexers(
            &names(2),
            |_prev| async { Err::<GateStatus, _>(anyhow::anyhow!("status read task panicked")) },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("status read task panicked"));
        assert!(spawned.borrow().is_empty());
    }

    #[tokio::test]
    async fn no_signal_rounds_keep_polling_and_advance_once_the_read_recovers() {
        // Regression for the teardown blast radius: a run of unreadable rounds
        // (transient RocksDB secondary open/catch-up failures against a busy
        // primary, reported by the reader as "no signal") must keep the gate
        // waiting and let it advance normally once the store reads again — never
        // shut every network down, never skip ahead.
        let (_tx, rx) = watch::channel(false);
        let polls = RefCell::new(0usize);
        let spawned = RefCell::new(Vec::<usize>::new());
        sequence_indexers(
            &names(2),
            |_prev| {
                let polls = &polls;
                async move {
                    let mut n = polls.borrow_mut();
                    *n += 1;
                    // 5 unreadable rounds, then a real past-bulk signal.
                    Ok(signal(if *n > 5 { Some(true) } else { None }))
                }
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();

        assert_eq!(*spawned.borrow(), vec![1], "advanced after recovery");
        assert_eq!(*polls.borrow(), 6, "kept polling through every dead round");
    }

    #[tokio::test]
    async fn parked_spawn_retries_the_same_network_and_never_advances() {
        // A spawn failure parks the gate: it stays on network 1, retrying, and
        // must never start network 2 ahead of it (BULK_SYNC rule 11).
        let (_tx, rx) = watch::channel(false);
        let attempts = RefCell::new(Vec::<usize>::new());
        sequence_indexers(
            &names(3),
            |_prev| async { Ok(signal(Some(true))) },
            |i| {
                let attempts = &attempts;
                async move {
                    attempts.borrow_mut().push(i);
                    // Park twice, then succeed on the third attempt.
                    if attempts.borrow().iter().filter(|a| **a == 1).count() < 3 && i == 1 {
                        Ok(SpawnOutcome::Parked)
                    } else {
                        Ok(SpawnOutcome::Started)
                    }
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();

        assert_eq!(
            *attempts.borrow(),
            vec![1, 1, 1, 2],
            "network 1 was retried in place; network 2 only started after it did"
        );
    }

    #[tokio::test]
    async fn parked_spawn_never_tears_down_and_yields_to_shutdown() {
        // A permanently unspawnable network parks forever rather than returning an
        // error (which the supervisor turns into a full orchestrator shutdown).
        // Only the shutdown signal ends the wait.
        let (tx, rx) = watch::channel(false);
        let attempts = RefCell::new(0usize);
        let stop = tx.clone();
        sequence_indexers(
            &names(2),
            |_prev| async { Ok(signal(Some(true))) },
            |_i| {
                let attempts = &attempts;
                let stop = &stop;
                async move {
                    *attempts.borrow_mut() += 1;
                    if *attempts.borrow() >= 4 {
                        stop.send(true).unwrap();
                    }
                    Ok(SpawnOutcome::Parked)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .expect("parking is not an error");

        assert_eq!(*attempts.borrow(), 4, "retried in place until shutdown");
    }

    #[tokio::test]
    async fn spawn_error_stops_before_later_indexers() {
        // `Err` from the spawn callback stays reserved for supervisor invariant
        // violations (child-order/spec-index bugs), which must still fail fast.
        let (_tx, rx) = watch::channel(false);
        let attempted = RefCell::new(Vec::<usize>::new());
        let err = sequence_indexers(
            &names(3),
            |_prev| async { Ok(signal(Some(true))) },
            |i| {
                let attempted = &attempted;
                async move {
                    attempted.borrow_mut().push(i);
                    Err(anyhow::anyhow!("spawn {i} failed"))
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("spawn 1 failed"));
        assert_eq!(*attempted.borrow(), vec![1]);
    }

    #[test]
    fn wait_log_cadence_is_derived_from_the_poll_interval() {
        assert_eq!(wait_log_every(Duration::from_secs(5)), 12); // 60s / 5s
        assert_eq!(wait_log_every(Duration::from_secs(60)), 1);
        // A poll slower than the log interval still logs every round rather than
        // dividing to zero and panicking on `% 0`.
        assert_eq!(wait_log_every(Duration::from_secs(3600)), 1);
        // A zero poll is floored to 1ms rather than dividing by zero.
        assert_eq!(wait_log_every(Duration::ZERO), 60_000);
    }

    #[tokio::test]
    async fn a_blocked_gating_indexer_keeps_the_gate_waiting_and_never_skips_ahead() {
        // The MAJOR defect: a blocked/dead gating indexer left the next network
        // unspawned forever with zero diagnostics. The gate must observe the
        // block, keep polling, and still never start the deferred network.
        let (tx, rx) = watch::channel(false);
        let observed = RefCell::new(0usize);
        let spawned = RefCell::new(Vec::<usize>::new());
        let stop = tx.clone();
        sequence_indexers(
            &names(2),
            |_prev| {
                let observed = &observed;
                let stop = &stop;
                async move {
                    *observed.borrow_mut() += 1;
                    if *observed.borrow() >= 5 {
                        stop.send(true).unwrap();
                    }
                    Ok(GateStatus {
                        past_bulk: Some(false),
                        blocked: Some("rebuild required (exit 78)".to_string()),
                        ..GateStatus::default()
                    })
                }
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();

        assert!(
            spawned.borrow().is_empty(),
            "a blocked gate must never let the next network start"
        );
        assert_eq!(*observed.borrow(), 5, "kept polling the blocked gate");
    }

    #[tokio::test]
    async fn a_blocked_gate_never_advances_even_when_its_store_is_past_bulk() {
        // A rebuild-required or failed handoff can leave durable bulk-complete
        // metadata behind. Child health is the stronger gate: stale progress
        // must never admit the next network while that child is blocked.
        let (tx, rx) = watch::channel(false);
        let observed = RefCell::new(0usize);
        let spawned = RefCell::new(Vec::<usize>::new());
        let stop = tx.clone();
        sequence_indexers(
            &names(2),
            |_prev| {
                let observed = &observed;
                let stop = &stop;
                async move {
                    *observed.borrow_mut() += 1;
                    if *observed.borrow() >= 4 {
                        stop.send(true).unwrap();
                    }
                    Ok(GateStatus {
                        past_bulk: Some(true),
                        bulk_completed: true,
                        blocked: Some("rebuild required (exit 78)".to_string()),
                        ..GateStatus::default()
                    })
                }
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();

        assert!(
            spawned.borrow().is_empty(),
            "blocked child health must override stale past-bulk metadata"
        );
        assert_eq!(*observed.borrow(), 4, "kept polling the blocked gate");
    }

    #[tokio::test]
    async fn a_gate_that_unblocks_then_passes_bulk_still_advances() {
        // Blocking is reported, not terminal: once the operator resolves it, the
        // gate resumes and the deferred network starts normally.
        let (_tx, rx) = watch::channel(false);
        let round = RefCell::new(0usize);
        let spawned = RefCell::new(Vec::<usize>::new());
        sequence_indexers(
            &names(2),
            |_prev| {
                let round = &round;
                async move {
                    let mut n = round.borrow_mut();
                    *n += 1;
                    Ok(match *n {
                        1..=2 => GateStatus {
                            past_bulk: Some(false),
                            blocked: Some("exceeded max restart attempts".to_string()),
                            ..GateStatus::default()
                        },
                        3 => signal(Some(false)),
                        _ => GateStatus {
                            past_bulk: Some(true),
                            lag: Some(12),
                            bulk_completed: true,
                            blocked: None,
                        },
                    })
                }
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(SpawnOutcome::Started)
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap();

        assert_eq!(*spawned.borrow(), vec![1]);
        assert_eq!(*round.borrow(), 4);
    }
}
