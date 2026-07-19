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

/// Start deferred indexers one at a time. `indexers[0]` is assumed already spawned
/// by the caller. For each `i` in `1..count`, poll until `indexers[i-1]` is past
/// bulk, then `spawn(i)`. Generic over the status reader and spawn action so the
/// gating logic is testable without real stores/processes.
///
/// `past_bulk(prev)` returns `Ok(Some(true|false))` on a successful read,
/// `Ok(None)` only while the store has not been created, and `Err` for an
/// existing store that cannot be read or decoded.
///
/// `spawn(i)` is `async`: the real supervisor callback appends to the shared
/// `SupervisorState` behind a `tokio::sync::Mutex`, which must be awaited (a
/// `blocking_lock` in this async context would panic), so the gate is async.
pub(crate) async fn sequence_indexers<R, S, Fut>(
    count: usize,
    mut past_bulk: R,
    mut spawn: S,
    poll: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()>
where
    R: FnMut(usize) -> Result<Option<bool>>,
    S: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    for i in 1..count {
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            if past_bulk(i - 1)? == Some(true) {
                spawn(i).await?;
                break;
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
                let mut c = calls.borrow_mut();
                *c += 1;
                Ok(Some((*c).is_multiple_of(2)))
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(())
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
                *calls.borrow_mut() += 1;
                Ok(Some(true))
            },
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(())
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
            |_prev| Ok(Some(false)),
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(())
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
    async fn status_read_error_stops_sequence_without_spawning() {
        let (_tx, rx) = watch::channel(false);
        let spawned = RefCell::new(Vec::<usize>::new());
        let err = sequence_indexers(
            2,
            |_prev| Err(anyhow::anyhow!("corrupt sync progress")),
            |i| {
                let spawned = &spawned;
                async move {
                    spawned.borrow_mut().push(i);
                    Ok(())
                }
            },
            Duration::from_millis(1),
            rx,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("corrupt sync progress"));
        assert!(spawned.borrow().is_empty());
    }

    #[tokio::test]
    async fn spawn_error_stops_before_later_indexers() {
        let (_tx, rx) = watch::channel(false);
        let attempted = RefCell::new(Vec::<usize>::new());
        let err = sequence_indexers(
            3,
            |_prev| Ok(Some(true)),
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
