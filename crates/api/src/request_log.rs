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
//!
//! A server error (5xx) carries its cause on the same line: what the response
//! said, as `error_kind` (the API error's code) and `error` (its message). A
//! failure other than 503 — the API's designed "not yet, retry" answer — is
//! logged at ERROR.

use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use serde::Deserialize;
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
    /// A server error other than 503: ERROR, slow or not.
    Failed,
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
    status: StatusCode,
    latency: Duration,
    waited: Duration,
    threshold: Duration,
    polling: bool,
) -> Result<RequestLogKind, WaitExceedsLatency> {
    let work = latency.checked_sub(waited).ok_or(WaitExceedsLatency)?;
    Ok(
        if status.is_server_error() && status != StatusCode::SERVICE_UNAVAILABLE {
            RequestLogKind::Failed
        } else if work >= threshold {
            RequestLogKind::Slow
        } else if polling {
            RequestLogKind::PolledCompleted
        } else {
            RequestLogKind::Completed
        },
    )
}

/// Log every request once it has produced its response: status, latency, the
/// deliberate wait (only for requests that waited), the response size and, for
/// a server error, what the response said.
///
/// Mounted just inside the compression layer, itself just inside
/// `TraceLayer`: each line carries its `http_request` span (method, path,
/// query), and a server error's body is read as the handler wrote it.
/// `threshold` is `[api] slow_request_threshold_ms`.
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

    let status_code = response.status();
    // Absent from the line unless the response is a server error.
    let (response, server_error) = if status_code.is_server_error() {
        let (response, said) = read_server_error(response).await;
        (response, Some(said))
    } else {
        (response, None)
    };
    let status = status_code.as_u16();
    let latency_ms = latency.as_millis() as u64;
    // Absent from the line for the requests that never wait.
    let waited_ms = (!waited.is_zero()).then_some(waited.as_millis() as u64);
    let size = response_size(response.headers());
    let error_kind = server_error
        .as_ref()
        .and_then(|error| error.kind.as_deref());
    let error = server_error.as_ref().map(|error| error.detail.as_str());

    match request_log_kind(status_code, latency, waited, threshold, polling) {
        Ok(RequestLogKind::Failed) => {
            tracing::error!(status, latency_ms, waited_ms, size, error_kind, error, "failed")
        }
        Ok(RequestLogKind::Slow) => {
            tracing::warn!(
                status,
                latency_ms,
                waited_ms,
                size,
                error_kind,
                error,
                "slow request"
            )
        }
        Ok(RequestLogKind::PolledCompleted) => {
            tracing::debug!(
                status,
                latency_ms,
                waited_ms,
                size,
                error_kind,
                error,
                "completed"
            )
        }
        Ok(RequestLogKind::Completed) => {
            tracing::info!(
                status,
                latency_ms,
                waited_ms,
                size,
                error_kind,
                error,
                "completed"
            )
        }
        Err(WaitExceedsLatency) => tracing::error!(
            status,
            latency_ms,
            waited_ms,
            size,
            error_kind,
            error,
            "request recorded more deliberate wait than its whole latency: its \
             RequestWait sleeps must run one at a time on the request's own task"
        ),
    }
    response
}

/// What a server-error response said, for its request's log line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ServerErrorBody {
    /// The API error's code (`internal_error`, `store_catching_up`, ...);
    /// `None` when the body is not an API error.
    kind: Option<String>,
    /// The API error's message; the whole body when it is not an API error.
    detail: String,
}

/// The JSON every `ApiError` response carries.
#[derive(Deserialize)]
struct ApiErrorBody {
    error: String,
    message: String,
}

impl ServerErrorBody {
    fn of(body: &[u8]) -> Self {
        match serde_json::from_slice::<ApiErrorBody>(body) {
            Ok(ApiErrorBody { error, message }) => Self {
                kind: Some(error),
                detail: message,
            },
            Err(_) => Self {
                kind: None,
                detail: String::from_utf8_lossy(body).into_owned(),
            },
        }
    }
}

/// Read a server error's body (one small JSON value) whole, and hand it on
/// byte for byte with what it said.
async fn read_server_error(response: Response) -> (Response, ServerErrorBody) {
    let (mut parts, body) = response.into_parts();
    match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => {
            let said = ServerErrorBody::of(&bytes);
            (Response::from_parts(parts, Body::from(bytes)), said)
        }
        Err(read_error) => {
            // The body is gone: the client gets the status alone, and the
            // log says why.
            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
            (
                Response::from_parts(parts, Body::empty()),
                ServerErrorBody {
                    kind: None,
                    detail: format!("the response body could not be read: {read_error}"),
                },
            )
        }
    }
}

fn response_size(headers: &axum::http::HeaderMap) -> Option<u64> {
    headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_invalid_content_length_is_unknown_size() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(response_size(&headers), None);
        headers.insert(axum::http::header::CONTENT_LENGTH, "bad".parse().unwrap());
        assert_eq!(response_size(&headers), None);
        headers.insert(axum::http::header::CONTENT_LENGTH, "0".parse().unwrap());
        assert_eq!(response_size(&headers), Some(0));
    }

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
        let kind = |ms| {
            request_log_kind(
                StatusCode::OK,
                Duration::from_millis(ms),
                Duration::ZERO,
                THRESHOLD,
                false,
            )
        };
        assert_eq!(kind(99), Ok(RequestLogKind::Completed));
        assert_eq!(kind(100), Ok(RequestLogKind::Slow));
    }

    #[test]
    fn the_deliberate_wait_is_not_slowness() {
        // 3 s waited, 99 ms of work.
        assert_eq!(
            request_log_kind(
                StatusCode::OK,
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
                StatusCode::OK,
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
                StatusCode::OK,
                Duration::from_millis(500),
                Duration::from_millis(501),
                THRESHOLD,
                false
            ),
            Err(WaitExceedsLatency)
        );
    }

    #[test]
    fn a_server_error_other_than_503_is_a_failure_slow_or_not() {
        let kind = |status, ms| {
            request_log_kind(
                status,
                Duration::from_millis(ms),
                Duration::ZERO,
                THRESHOLD,
                true,
            )
        };
        for status in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert_eq!(kind(status, 1), Ok(RequestLogKind::Failed), "{status}");
            assert_eq!(kind(status, 100), Ok(RequestLogKind::Failed), "{status}");
        }
        // 503 is the API's designed "not yet, retry" answer, logged as any
        // other completed request; so are client errors.
        assert_eq!(
            kind(StatusCode::SERVICE_UNAVAILABLE, 1),
            Ok(RequestLogKind::PolledCompleted)
        );
        assert_eq!(
            kind(StatusCode::SERVICE_UNAVAILABLE, 100),
            Ok(RequestLogKind::Slow)
        );
        assert_eq!(
            kind(StatusCode::NOT_FOUND, 1),
            Ok(RequestLogKind::PolledCompleted)
        );
    }

    #[test]
    fn a_server_error_body_says_its_api_error_or_itself() {
        let body = serde_json::to_vec(
            &crate::response::ApiError::internal("missing canonical output payload 0xab:0")
                .1
                 .0,
        )
        .unwrap();
        assert_eq!(
            ServerErrorBody::of(&body),
            ServerErrorBody {
                kind: Some("internal_error".into()),
                detail: "missing canonical output payload 0xab:0".into(),
            }
        );
        assert_eq!(
            ServerErrorBody::of(b"upstream connect error"),
            ServerErrorBody {
                kind: None,
                detail: "upstream connect error".into(),
            }
        );
    }

    /// A server error's body reaches the client byte for byte after the log
    /// has read it; nothing else is read at all.
    #[tokio::test]
    async fn the_log_hands_every_body_on_unchanged() {
        use axum::routing::get;
        use axum::Router;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        async fn failing() -> crate::response::ApiRouteError {
            crate::response::ApiError::internal("block 7 header read: corrupt")
        }
        async fn fine() -> &'static str {
            "fine"
        }
        let app = Router::new()
            .route("/failing", get(failing))
            .route("/fine", get(fine))
            .layer(axum::middleware::from_fn_with_state(THRESHOLD, log_request));
        let get = |uri: &'static str| {
            app.clone().oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
        };

        let response = get("/failing").await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"error": "internal_error", "message": "block 7 header read: corrupt"})
        );

        let response = get("/fine").await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"fine");
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
