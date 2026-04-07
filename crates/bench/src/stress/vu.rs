use std::sync::Arc;
use std::time::Instant;

use rand::Rng;
use tokio_util::sync::CancellationToken;

use crate::registry::{DiscoveredParams, EndpointEntry, ResolvedRequest};
use crate::runner::execute_request;

use super::collector::{SampleSender, StressSample};
use super::scenario::{pick_endpoint, EndpointGroup, FRONTEND_ROUTES};

// ---------------------------------------------------------------------------
// ResolvedEndpoint — pre-resolved endpoint ready for stress execution
// ---------------------------------------------------------------------------

pub struct ResolvedEndpoint {
    pub idx: usize,
    pub path_template: String,
    pub read_pattern: String,
    pub resolved: ResolvedRequest,
    pub expect_status: u16,
}

// ---------------------------------------------------------------------------
// Resolution helpers
// ---------------------------------------------------------------------------

/// Attempt to resolve a single endpoint entry for stress testing.
pub fn resolve_for_stress(
    entry: &EndpointEntry,
    api_base: &str,
    params: &DiscoveredParams,
) -> Option<ResolvedRequest> {
    (entry.resolve)(api_base, params)
}

/// Pre-resolve all endpoints, returning only those that resolve successfully.
pub fn resolve_all(
    entries: &[EndpointEntry],
    api_base: &str,
    params: &DiscoveredParams,
) -> Vec<ResolvedEndpoint> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            resolve_for_stress(entry, api_base, params).map(|resolved| ResolvedEndpoint {
                idx,
                path_template: entry.path_template.to_string(),
                read_pattern: entry.read_pattern.to_string(),
                resolved,
                expect_status: entry.expect_status,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ResolvedTarget — unified target for API and frontend requests
// ---------------------------------------------------------------------------

/// A resolved target that can be either an API endpoint or a frontend route.
pub enum ResolvedTarget {
    Api(ResolvedEndpoint),
    Frontend {
        idx: usize,
        route: String,
        url: String,
    },
}

/// Pre-resolve all endpoints including frontend routes.
///
/// Creates API `ResolvedTarget`s from `resolve_all()`, then appends Frontend
/// targets for each `FRONTEND_ROUTE` with `idx` starting at `entries.len()`.
pub fn resolve_all_with_frontend(
    entries: &[EndpointEntry],
    api_base: &str,
    frontend_url: &str,
    params: &DiscoveredParams,
) -> Vec<ResolvedTarget> {
    let api_targets: Vec<ResolvedTarget> = resolve_all(entries, api_base, params)
        .into_iter()
        .map(ResolvedTarget::Api)
        .collect();

    let frontend_targets: Vec<ResolvedTarget> = FRONTEND_ROUTES
        .iter()
        .enumerate()
        .map(|(i, route)| ResolvedTarget::Frontend {
            idx: entries.len() + i,
            route: route.to_string(),
            url: format!("{}{}", frontend_url.trim_end_matches('/'), route),
        })
        .collect();

    let mut all = api_targets;
    all.extend(frontend_targets);
    all
}

// ---------------------------------------------------------------------------
// Virtual User task
// ---------------------------------------------------------------------------

/// Spawn a single virtual user (VU) as a tokio task.
///
/// The VU loops: pick a random endpoint from weighted groups, execute the
/// request, send the sample to the collector, and optionally sleep for a
/// random think time.
pub fn spawn_vu(
    client: Arc<reqwest::Client>,
    resolved_targets: Arc<Vec<ResolvedTarget>>,
    groups: Arc<Vec<EndpointGroup>>,
    tx: SampleSender,
    think_time: Option<(u64, u64)>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if cancel.is_cancelled() {
                break;
            }

            // Pick an endpoint index from weighted groups
            let target_idx = pick_endpoint(&groups);

            // Find the matching pre-resolved target
            let target = match resolved_targets.iter().find(|t| match t {
                ResolvedTarget::Api(ep) => ep.idx == target_idx,
                ResolvedTarget::Frontend { idx, .. } => *idx == target_idx,
            }) {
                Some(t) => t,
                None => continue, // target didn't resolve, skip
            };

            let stress_sample = match target {
                ResolvedTarget::Api(ep) => {
                    let sample = execute_request(&client, &ep.resolved, ep.expect_status).await;
                    StressSample {
                        endpoint_idx: ep.idx,
                        endpoint_path: ep.path_template.clone(),
                        read_pattern: ep.read_pattern.clone(),
                        latency_ms: sample.latency_ms,
                        status: sample.status,
                        body_size: sample.body_size,
                        error: sample.error,
                    }
                }
                ResolvedTarget::Frontend { idx, route, url } => {
                    let start = Instant::now();
                    let result = client.get(url).send().await;
                    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

                    match result {
                        Ok(response) => {
                            let status = response.status().as_u16();
                            let body_bytes = response.bytes().await.unwrap_or_default();
                            let body_size = body_bytes.len();
                            let error = if status != 200 {
                                Some(format!("expected status 200, got {}", status))
                            } else {
                                None
                            };
                            StressSample {
                                endpoint_idx: *idx,
                                endpoint_path: route.clone(),
                                read_pattern: "Frontend".to_string(),
                                latency_ms,
                                status,
                                body_size,
                                error,
                            }
                        }
                        Err(e) => StressSample {
                            endpoint_idx: *idx,
                            endpoint_path: route.clone(),
                            read_pattern: "Frontend".to_string(),
                            latency_ms,
                            status: 0,
                            body_size: 0,
                            error: Some(e.to_string()),
                        },
                    }
                }
            };

            if tx.send(stress_sample).is_err() {
                // Collector dropped — stop this VU
                break;
            }

            // Optional think time
            if let Some((min_ms, max_ms)) = think_time {
                let ms = if min_ms >= max_ms {
                    min_ms
                } else {
                    rand::rng().random_range(min_ms..=max_ms)
                };
                let sleep_dur = tokio::time::Duration::from_millis(ms);
                tokio::select! {
                    _ = tokio::time::sleep(sleep_dur) => {}
                    _ = cancel.cancelled() => { break; }
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{get, EndpointEntry, Method, ReadPattern, RiskTier};

    #[allow(clippy::type_complexity)]
    fn make_entry(
        resolve_fn: Box<dyn Fn(&str, &DiscoveredParams) -> Option<ResolvedRequest> + Send + Sync>,
    ) -> EndpointEntry {
        EndpointEntry {
            module: "test",
            method: Method::Get,
            path_template: "/test/{id}",
            description: "test endpoint",
            resolve: resolve_fn,
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        }
    }

    #[test]
    fn test_resolve_endpoint_returns_none_for_missing_params() {
        let entry = make_entry(Box::new(|_base, _params| None));
        let params = DiscoveredParams::default();
        let result = resolve_for_stress(&entry, "http://localhost:8101/api/v1", &params);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_endpoint_returns_request() {
        let entry = make_entry(Box::new(|base, _params| {
            Some(get(&format!("{base}/test/42")))
        }));
        let params = DiscoveredParams::default();
        let result = resolve_for_stress(&entry, "http://localhost:8101/api/v1", &params);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert_eq!(resolved.url, "http://localhost:8101/api/v1/test/42");
    }
}
