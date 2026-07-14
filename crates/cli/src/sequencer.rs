//! Orchestrator bulk-sync sequencer: start network indexers one at a time so only
//! one network bulk-syncs at a time. Pure decision logic + a generic gating loop;
//! the prod store-reader + spawn wiring live in `supervisor.rs`.

use std::time::Duration;
use tokio::sync::watch;

/// Whether a watched network is out of bulk sync (safe to start the next indexer).
///
/// `bulk_completed` = its `SyncStatus.bulk_sync_completed_at.is_some()`.
/// `lag` = `sync-progress target_block − current_block` (as i64; `None` when no
/// progress record exists yet). i64 (not saturating) so caught-up/ahead (lag ≤ 0)
/// reads as past-bulk without masking anything.
// Wired into the supervisor by Task 4 (`run_supervisor_sequenced`); unused until then.
#[allow(dead_code)]
pub(crate) fn is_past_bulk(bulk_completed: bool, lag: Option<i64>, threshold: u64) -> bool {
    bulk_completed || matches!(lag, Some(l) if l <= threshold as i64)
}

/// Start deferred indexers one at a time. `indexers[0]` is assumed already spawned
/// by the caller. For each `i` in `1..count`, poll until `indexers[i-1]` is past
/// bulk, then `spawn(i)`. Generic over the status reader and spawn action so the
/// gating logic is testable without real stores/processes.
///
/// `past_bulk(prev)` returns `Some(true|false)` on a successful read, or `None`
/// when the store can't be read yet (treated as "not past bulk" — keep waiting).
// Wired into the supervisor by Task 4 (`run_supervisor_sequenced`); unused until then.
#[allow(dead_code)]
pub(crate) async fn sequence_indexers<R, S>(
    count: usize,
    mut past_bulk: R,
    mut spawn: S,
    poll: Duration,
    mut shutdown: watch::Receiver<bool>,
) where
    R: FnMut(usize) -> Option<bool>,
    S: FnMut(usize),
{
    for i in 1..count {
        loop {
            if *shutdown.borrow() {
                return;
            }
            if past_bulk(i - 1) == Some(true) {
                spawn(i);
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        return;
                    }
                }
            }
        }
    }
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
                let mut c = calls.borrow_mut();
                *c += 1;
                Some((*c).is_multiple_of(2))
            },
            |i| spawned.borrow_mut().push(i),
            Duration::from_millis(1),
            rx,
        )
        .await;
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
                *calls.borrow_mut() += 1;
                Some(true)
            },
            |i| spawned.borrow_mut().push(i),
            Duration::from_millis(1),
            rx,
        )
        .await;
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
            |_prev| Some(false),
            |i| spawned.borrow_mut().push(i),
            Duration::from_millis(1),
            rx,
        )
        .await;
        assert!(spawned.borrow().is_empty());
    }
}
