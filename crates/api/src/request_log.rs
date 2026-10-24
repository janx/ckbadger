//! How a finished HTTP request is logged, and the one way a handler that waits
//! on purpose says so.
//!
//! A request is slow when its own work — its latency minus the time it spent
//! in a deliberate wait — reaches `[api] slow_request_threshold_ms`. Handlers
//! whose contract is to wait (the cycles long-poll for the indexer's result,
//! `/pipeline` for the store to reach the node's pool tip) sleep through
//! [`RequestWait`], which records exactly how long they slept; everything else
//! the request does, before, between and after those sleeps, still counts.
//! So waiting never hides slowness, and slowness is never blamed on a wait.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

/// The time this request has spent waiting on purpose.
///
/// A handler takes it as an extractor and does every deliberate wait through
/// [`RequestWait::sleep`] / [`RequestWait::sleep_until`], one at a time, on
/// the request's own task. The request log subtracts the recorded total from
/// the request's latency before comparing it with the slow-request threshold.
///
/// In a router without the request log ([`log_request`]) the extractor hands
/// out a recorder of its own: the sleeps still happen, and there is no log to
/// report them to.
#[derive(Clone, Default)]
pub struct RequestWait(Arc<Mutex<Duration>>);

impl RequestWait {
    /// Sleep until `deadline`, as part of this request's deliberate wait.
    pub async fn sleep_until(&self, deadline: Instant) {
        let started = Instant::now();
        tokio::time::sleep_until(deadline).await;
        let slept = started.elapsed();
        *self.0.lock().expect("request wait poisoned") += slept;
    }

    /// Sleep for `duration`, as part of this request's deliberate wait.
    pub async fn sleep(&self, duration: Duration) {
        self.sleep_until(Instant::now() + duration).await;
    }

    /// Everything this request has slept through so far.
    pub fn waited(&self) -> Duration {
        *self.0.lock().expect("request wait poisoned")
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestWait {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<RequestWait>()
            .cloned()
            .unwrap_or_default())
    }
}

/// Paths frequently polled by the frontend (2-10s intervals) that generate
/// excessive log volume at INFO level. Logged at DEBUG instead.
const POLLING_PATHS: &[&str] = &[
    "/api/v1/statistics/network",
    "/api/v1/statistics/recent-blocks",
    "/api/v1/activities/latest",
    "/api/v1/mempool/info",
    "/api/v1/mempool/transactions",
    "/api/v1/mempool/blocks",
    "/api/v1/mempool/pending-proposals",
    "/api/v1/pipeline",
];

pub(crate) fn is_polling_path(path: &str) -> bool {
    POLLING_PATHS.contains(&path)
}

/// How a finished request is logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestLogKind {
    /// Its own work reached the slow-request threshold: WARN.
    Slow,
    /// A frequently polled endpoint, done in time: DEBUG.
    PolledCompleted,
    /// Done in time: INFO.
    Completed,
}

/// The deliberate wait a request recorded exceeds its whole latency: its
/// sleeps did not run one at a time inside the request, so its work cannot be
/// told apart from its wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaitExceedsLatency;

fn request_log_kind(
    latency: Duration,
    waited: Duration,
    threshold: Duration,
    polling: bool,
) -> Result<RequestLogKind, WaitExceedsLatency> {
    let work = latency.checked_sub(waited).ok_or(WaitExceedsLatency)?;
    Ok(if work >= threshold {
        RequestLogKind::Slow
    } else if polling {
        RequestLogKind::PolledCompleted
    } else {
        RequestLogKind::Completed
    })
}

/// Log every request once it has produced its response: status, latency, the
/// deliberate wait (only for requests that waited) and the response size.
///
/// Mounted just inside `TraceLayer`, so each line carries its `http_request`
/// span (method, path, query). `threshold` is `[api] slow_request_threshold_ms`.
pub async fn log_request(
    State(threshold): State<Duration>,
    mut request: Request,
    next: Next,
) -> Response {
    let polling = is_polling_path(request.uri().path());
    let wait = RequestWait::default();
    request.extensions_mut().insert(wait.clone());
    let started = Instant::now();
    let response = next.run(request).await;
    let latency = started.elapsed();
    let waited = wait.waited();

    let status = response.status().as_u16();
    let latency_ms = latency.as_millis() as u64;
    // Absent from the line for the requests that never wait.
    let waited_ms = (!waited.is_zero()).then_some(waited.as_millis() as u64);
    let size = response
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    match request_log_kind(latency, waited, threshold, polling) {
        Ok(RequestLogKind::Slow) => {
            tracing::warn!(status, latency_ms, waited_ms, size, "slow request")
        }
        Ok(RequestLogKind::PolledCompleted) => {
            tracing::debug!(status, latency_ms, waited_ms, size, "completed")
        }
        Ok(RequestLogKind::Completed) => {
            tracing::info!(status, latency_ms, waited_ms, size, "completed")
        }
        Err(WaitExceedsLatency) => tracing::error!(
            status,
            latency_ms,
            waited_ms,
            size,
            "request recorded more deliberate wait than its whole latency: its \
             RequestWait sleeps must run one at a time on the request's own task"
        ),
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: Duration = Duration::from_millis(100);

    #[test]
    fn test_polling_path_detection() {
        assert!(is_polling_path("/api/v1/statistics/network"));
        assert!(is_polling_path("/api/v1/activities/latest"));
        assert!(is_polling_path("/api/v1/mempool/info"));
        assert!(is_polling_path("/api/v1/mempool/transactions"));
        assert!(is_polling_path("/api/v1/mempool/blocks"));
        assert!(is_polling_path("/api/v1/mempool/pending-proposals"));
        assert!(is_polling_path("/api/v1/statistics/recent-blocks"));
        assert!(is_polling_path("/api/v1/pipeline"));

        assert!(!is_polling_path("/api/v1/blocks"));
        assert!(!is_polling_path("/api/v1/blocks/12345"));
        assert!(!is_polling_path("/api/v1/transactions/0xabc"));
        assert!(!is_polling_path("/api/v1/search"));
        assert!(!is_polling_path("/api/v1/statistics/tx-stats"));
    }

    #[test]
    fn a_request_that_never_waits_is_slow_at_the_threshold() {
        let kind =
            |ms| request_log_kind(Duration::from_millis(ms), Duration::ZERO, THRESHOLD, false);
        assert_eq!(kind(99), Ok(RequestLogKind::Completed));
        assert_eq!(kind(100), Ok(RequestLogKind::Slow));
    }

    #[test]
    fn the_deliberate_wait_is_not_slowness() {
        // 3 s waited, 99 ms of work.
        assert_eq!(
            request_log_kind(
                Duration::from_millis(3_099),
                Duration::from_secs(3),
                THRESHOLD,
                true
            ),
            Ok(RequestLogKind::PolledCompleted)
        );
    }

    #[test]
    fn slow_work_beside_a_wait_is_still_slow() {
        // 3 s waited, 100 ms of work.
        assert_eq!(
            request_log_kind(
                Duration::from_millis(3_100),
                Duration::from_secs(3),
                THRESHOLD,
                true
            ),
            Ok(RequestLogKind::Slow)
        );
    }

    #[test]
    fn a_wait_longer_than_the_request_is_an_error() {
        assert_eq!(
            request_log_kind(
                Duration::from_millis(500),
                Duration::from_millis(501),
                THRESHOLD,
                false
            ),
            Err(WaitExceedsLatency)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_wait_records_exactly_its_own_sleeps() {
        let wait = RequestWait::default();
        assert_eq!(wait.waited(), Duration::ZERO);

        wait.sleep(Duration::from_millis(250)).await;
        // Work between the sleeps is not wait.
        tokio::time::advance(Duration::from_millis(40)).await;
        wait.sleep_until(Instant::now() + Duration::from_millis(750))
            .await;
        // A deadline already behind it sleeps (and records) nothing.
        wait.sleep_until(Instant::now() - Duration::from_millis(1))
            .await;

        assert_eq!(wait.waited(), Duration::from_secs(1));
    }

    /// The extractor hands the handler the recorder the request log reads.
    #[tokio::test(start_paused = true)]
    async fn the_handler_waits_on_the_request_log_recorder() {
        use axum::body::Body;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        async fn waiting(wait: RequestWait) -> &'static str {
            wait.sleep(Duration::from_millis(300)).await;
            "done"
        }
        let seen = RequestWait::default();
        let app = Router::new()
            .route("/probe", get(waiting))
            .layer(axum::middleware::from_fn({
                let seen = seen.clone();
                move |mut request: Request, next: Next| {
                    let seen = seen.clone();
                    async move {
                        request.extensions_mut().insert(seen);
                        next.run(request).await
                    }
                }
            }));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(seen.waited(), Duration::from_millis(300));
    }
}
