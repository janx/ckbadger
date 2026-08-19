//! Process-wide read view for secondary (read-only) store instances.
//!
//! A RocksDB secondary cannot take snapshots — `GetSnapshot` fails with
//! `Not implemented: snapshot not supported in secondary mode` — and its view
//! advances *only* when `try_catch_up_with_primary()` runs. So any read that
//! resolves an index row and then loads the row it points at spans two views
//! when a catch-up lands in between: the iterator, pinned at creation, still
//! yields the pre-catch-up index row while the point lookup already sees the
//! post-catch-up entry. Cross-checks between the two then fail on healthy data
//! (`dao_by_status_block stale status: expected=0, actual=1`), and reads
//! without a cross-check silently return a torn mix of two views.
//!
//! Catch-up being the single mutation point of a reader's view is what makes
//! this fixable at the process level: a read scope pins the view for its whole
//! duration, and catch-up waits for the pinned scopes to end. That restores,
//! per process, the guarantee `snapshot()` gives on a primary — and it is the
//! guarantee every multi-CF read path already assumes.
//!
//! Both sides are bounded, so neither can be starved by the other: catch-up
//! takes priority over read scopes that arrive after it queues, but only for
//! [`CATCH_UP_YIELD_GRACE`], after which reads proceed on the current view and
//! the blocked catch-up reports itself every [`STALL_WARN_INTERVAL`].
//!
//! Scoping rules:
//! - Pinned: one HTTP request in `ckbadger-api` (a middleware holds the guard
//!   for the handler's lifetime), so a response can never mix two views.
//! - Deliberately not pinned: handlers that wait for the indexer to write new
//!   data (cycles long-poll) — they release the pin before waiting, because
//!   their whole purpose is to observe the *next* view. Pinning them would
//!   block the catch-up they are waiting for.
//! - Deliberately not pinned: background full-store scans (asset/address/script
//!   cache warmup) and the WebSocket broadcasters. They interleave external I/O
//!   and minutes-long scans with store reads; pinning them would freeze
//!   catch-up for that long. They accept drift and are rebuilt periodically.
//!
//! A read view must never be held across a call to [`crate::CkbadgerStore::refresh`]
//! on the same thread — that self-deadlocks. Catch-up runs on its own thread
//! (`spawn_blocking` in the API), never inside a pinned scope.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long catch-up waits before reporting that a read view is holding it up.
const STALL_WARN_INTERVAL: Duration = Duration::from_secs(5);

/// How long a new read scope yields to a queued catch-up before pinning anyway.
///
/// Yielding is what keeps a steady request stream from starving catch-up.
/// Bounding the yield is what keeps one pathological scope — a handler awaiting
/// a wedged CKB node, say — from freezing every later request behind it: after
/// the grace, requests are served again from the current (coherent, if stale)
/// view while catch-up keeps waiting for a gap and logs the stall.
const CATCH_UP_YIELD_GRACE: Duration = Duration::from_millis(100);

struct LatchState {
    /// Read scopes currently pinning the view.
    readers: usize,
    /// A catch-up window is open.
    catching_up: bool,
    /// Catch-up windows queued behind the current readers. New readers wait
    /// behind them, so a steady request stream cannot starve catch-up.
    catch_up_waiting: usize,
}

struct ViewLatch {
    state: Mutex<LatchState>,
    signal: Condvar,
}

static LATCH: ViewLatch = ViewLatch {
    state: Mutex::new(LatchState {
        readers: 0,
        catching_up: false,
        catch_up_waiting: 0,
    }),
    signal: Condvar::new(),
};

/// Pins the process-wide read view until dropped.
///
/// Every store read taken while this is alive observes the same view, across
/// column families and across store instances (domain, append-only, network,
/// CKB reader), because all of them advance only inside a [`CatchUpWindow`].
#[must_use = "the read view is released as soon as the guard is dropped"]
pub struct ReadViewGuard {
    _private: (),
}

impl Drop for ReadViewGuard {
    fn drop(&mut self) {
        let mut state = LATCH.state.lock().expect("read view latch poisoned");
        state.readers -= 1;
        if state.readers == 0 {
            LATCH.signal.notify_all();
        }
    }
}

/// Pin the current view for the lifetime of the returned guard.
///
/// Waits out an open catch-up window (bounded by one
/// `try_catch_up_with_primary()` call) and yields to a queued one for at most
/// [`CATCH_UP_YIELD_GRACE`].
pub fn acquire_read() -> ReadViewGuard {
    let mut state = LATCH.state.lock().expect("read view latch poisoned");
    let yield_until = Instant::now() + CATCH_UP_YIELD_GRACE;
    loop {
        if state.catching_up {
            state = LATCH
                .signal
                .wait(state)
                .expect("read view latch poisoned while waiting for catch-up");
            continue;
        }
        if state.catch_up_waiting == 0 {
            break;
        }
        let Some(remaining) = yield_until.checked_duration_since(Instant::now()) else {
            break;
        };
        state = LATCH
            .signal
            .wait_timeout(state, remaining)
            .expect("read view latch poisoned while yielding to catch-up")
            .0;
    }
    state.readers += 1;
    ReadViewGuard { _private: () }
}

/// Exclusive window in which secondary views may advance.
///
/// Holding one guarantees no pinned read scope is in flight, so several stores
/// can be caught up together and no reader can observe them half-advanced.
#[must_use = "the catch-up window closes as soon as the guard is dropped"]
pub struct CatchUpWindow {
    _private: (),
}

impl Drop for CatchUpWindow {
    fn drop(&mut self) {
        let mut state = LATCH.state.lock().expect("read view latch poisoned");
        state.catching_up = false;
        LATCH.signal.notify_all();
    }
}

/// Open a catch-up window, waiting for in-flight read scopes to finish.
///
/// Must not be called from a thread that holds a [`ReadViewGuard`].
pub fn catch_up_window() -> CatchUpWindow {
    let mut state = LATCH.state.lock().expect("read view latch poisoned");
    state.catch_up_waiting += 1;
    let mut waited = Duration::ZERO;
    while state.catching_up || state.readers > 0 {
        let (next, timeout) = LATCH
            .signal
            .wait_timeout(state, STALL_WARN_INTERVAL)
            .expect("read view latch poisoned while waiting for readers");
        state = next;
        if timeout.timed_out() {
            waited += STALL_WARN_INTERVAL;
            tracing::warn!(
                readers = state.readers,
                waited_secs = waited.as_secs(),
                "secondary catch-up is blocked by a long-lived read view; \
                 the served view is stale until it is released"
            );
        }
    }
    state.catch_up_waiting -= 1;
    state.catching_up = true;
    CatchUpWindow { _private: () }
}

/// Read scopes currently pinning the view (diagnostics and tests).
pub fn pinned_read_scopes() -> usize {
    LATCH
        .state
        .lock()
        .expect("read view latch poisoned")
        .readers
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    /// The latch is process-wide, so these tests must not overlap each other.
    static TEST_SERIAL: Mutex<()> = Mutex::new(());

    /// Long enough for a spawned thread to reach its blocking wait.
    const SETTLE: Duration = Duration::from_millis(150);

    #[test]
    fn catch_up_waits_for_pinned_read_scope() {
        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        let view = acquire_read();
        assert_eq!(pinned_read_scopes(), 1);

        let opened = Arc::new(AtomicBool::new(false));
        let opened_in_thread = Arc::clone(&opened);
        let catcher = thread::spawn(move || {
            let _window = catch_up_window();
            opened_in_thread.store(true, Ordering::SeqCst);
        });

        thread::sleep(SETTLE);
        assert!(
            !opened.load(Ordering::SeqCst),
            "catch-up must not open while a read scope is pinned"
        );

        drop(view);
        catcher.join().expect("catch-up thread panicked");
        assert!(opened.load(Ordering::SeqCst));
        assert_eq!(pinned_read_scopes(), 0);
    }

    #[test]
    fn new_read_scope_yields_to_a_queued_catch_up_but_is_not_frozen_by_it() {
        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        // A long-lived scope, as a handler awaiting a wedged node would be.
        let first = acquire_read();

        let catcher = thread::spawn(catch_up_window);
        thread::sleep(SETTLE);

        let started = Instant::now();
        let second = acquire_read();
        let yielded_for = started.elapsed();

        assert!(
            yielded_for >= CATCH_UP_YIELD_GRACE,
            "a new read scope must yield to a queued catch-up (waited {yielded_for:?})"
        );
        assert!(
            yielded_for < CATCH_UP_YIELD_GRACE * 20,
            "yielding must be bounded: one stuck scope cannot freeze later requests \
             (waited {yielded_for:?})"
        );

        drop(second);
        drop(first);
        drop(catcher.join().expect("catch-up thread panicked"));
    }

    #[test]
    fn read_scopes_do_not_block_each_other() {
        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        let first = acquire_read();
        let second = acquire_read();
        assert_eq!(pinned_read_scopes(), 2);
        drop(second);
        assert_eq!(pinned_read_scopes(), 1);
        drop(first);
        assert_eq!(pinned_read_scopes(), 0);
    }
}
