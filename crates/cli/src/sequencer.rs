//! Orchestrator bulk-sync sequencer: start network indexers one at a time so only
//! one network bulk-syncs at a time. Pure decision logic + a generic gating loop;
//! the prod store-reader + spawn wiring live in `supervisor.rs`.

use std::time::Duration;

use anyhow::Result;
use tokio::sync::watch;

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

/// Start deferred indexers one at a time. `indexers[0]` is assumed already spawned
/// by the caller. For each `i` in `1..count`, poll until `indexers[i-1]` is past
/// bulk, then `spawn(i)`. Generic over the status reader and spawn action so the
/// gating logic is testable without real stores/processes.
///
/// `past_bulk(prev)` returns `Ok(Some(true|false))` on a successful read and
/// `Ok(None)` when this round carries no signal (the store has not been created
/// yet). It is `async` because the real supervisor callback hands the blocking
/// RocksDB secondary open/refresh/read to `spawn_blocking` — a sequencer poll
/// must never block a runtime worker for the duration of a RocksDB catch-up.
///
/// `spawn(i)` is `async`: the real supervisor callback appends to the shared
/// `SupervisorState` behind a `tokio::sync::Mutex`, which must be awaited (a
/// `blocking_lock` in this async context would panic), so the gate is async. It
/// reports [`SpawnOutcome::Parked`] rather than `Err` when a child could not be
/// started, so a spawn failure stalls exactly one network instead of shutting the
/// whole orchestrator down.
pub(crate) async fn sequence_indexers<R, RFut, S, SFut>(
    count: usize,
    mut past_bulk: R,
    mut spawn: S,
    poll: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()>
where
    R: FnMut(usize) -> RFut,
    RFut: std::future::Future<Output = Result<Option<bool>>>,
    S: FnMut(usize) -> SFut,
    SFut: std::future::Future<Output = Result<SpawnOutcome>>,
{
    for i in 1..count {
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            if past_bulk(i - 1).await? == Some(true) {
                // Parked: stay on this network. Fall through to the poll sleep so
                // the gate keeps observing (and keeps retrying) instead of
                // advancing past an indexer that never started.
                if spawn(i).await? == SpawnOutcome::Started {
                    break;
                }
            }
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
            3,
            // false, true, false, true -> each network waits one poll before advancing
            |_prev| {
                let calls = &calls;
                async move {
                    let mut c = calls.borrow_mut();
                    *c += 1;
                    Ok(Some((*c).is_multiple_of(2)))
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
            1,
            |_prev| {
                let calls = &calls;
                async move {
                    *calls.borrow_mut() += 1;
                    Ok(Some(true))
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
            3,
            |_prev| async { Ok(Some(false)) },
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
            2,
            |_prev| async { Err(anyhow::anyhow!("status read task panicked")) },
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
            2,
            |_prev| {
                let polls = &polls;
                async move {
                    let mut n = polls.borrow_mut();
                    *n += 1;
                    // 5 unreadable rounds, then a real past-bulk signal.
                    Ok(if *n > 5 { Some(true) } else { None })
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
            3,
            |_prev| async { Ok(Some(true)) },
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
            2,
            |_prev| async { Ok(Some(true)) },
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
            3,
            |_prev| async { Ok(Some(true)) },
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
}
