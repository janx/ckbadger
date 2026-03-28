use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::metrics::{ComputedMetrics, Sample};
use crate::registry::{DiscoveredParams, EndpointEntry, Method, ResolvedRequest};

/// Result of benchmarking one endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointResult {
    pub module: String,
    pub method: String,
    pub path_template: String,
    pub description: String,
    pub resolved_url: String,
    pub read_pattern: String,
    pub risk_tier: String,
    pub samples: Vec<Sample>,
    pub metrics: ComputedMetrics,
    #[serde(with = "duration_millis")]
    pub wall_clock: Duration,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

pub struct RunConfig {
    pub iterations: u32,
    pub concurrency: u32,
    pub warmup: u32,
}

/// Execute a single HTTP request and record a `Sample`.
async fn execute_request(
    client: &reqwest::Client,
    resolved: &ResolvedRequest,
    expect_status: u16,
) -> Sample {
    let start = Instant::now();

    let result = match resolved.method {
        Method::Get => client.get(&resolved.url).send().await,
        Method::Post => {
            let mut builder = client.post(&resolved.url);
            if let Some(ref body) = resolved.body {
                builder = builder
                    .header("Content-Type", "application/json")
                    .body(body.clone());
            }
            builder.send().await
        }
    };

    let latency = start.elapsed();
    let latency_ms = latency.as_secs_f64() * 1000.0;

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let body_bytes = response.bytes().await.unwrap_or_default();
            let body_size = body_bytes.len();
            let error = if status != expect_status {
                Some(format!("expected status {}, got {}", expect_status, status))
            } else {
                None
            };

            Sample {
                latency_ms,
                status,
                body_size,
                error,
            }
        }
        Err(e) => Sample {
            latency_ms,
            status: 0,
            body_size: 0,
            error: Some(e.to_string()),
        },
    }
}

/// Benchmark a single endpoint entry.
///
/// Returns `EndpointResult` with skipped=true if the endpoint cannot be resolved
/// (missing discovery params), or with measured samples and computed metrics otherwise.
pub async fn bench_endpoint(
    client: &reqwest::Client,
    entry: &EndpointEntry,
    api_base: &str,
    params: &DiscoveredParams,
    config: &RunConfig,
) -> Result<EndpointResult> {
    let resolved = match (entry.resolve)(api_base, params) {
        Some(r) => r,
        None => {
            return Ok(EndpointResult {
                module: entry.module.to_string(),
                method: entry.method.to_string(),
                path_template: entry.path_template.to_string(),
                description: entry.description.to_string(),
                resolved_url: String::new(),
                read_pattern: entry.read_pattern.to_string(),
                risk_tier: format!("{:?}", entry.risk_tier),
                samples: Vec::new(),
                metrics: ComputedMetrics::from_samples(&[], Duration::ZERO),
                wall_clock: Duration::ZERO,
                skipped: true,
                skip_reason: Some("could not resolve params".to_string()),
            });
        }
    };

    // Warmup phase: fire requests but discard results.
    for _ in 0..config.warmup {
        let _ = execute_request(client, &resolved, entry.expect_status).await;
    }

    // Capture the resolved URL before any potential move into Arc.
    let resolved_url = resolved.url.clone();

    // Measured phase.
    let wall_start = Instant::now();

    let samples = if config.concurrency <= 1 {
        // Sequential execution.
        let mut samples = Vec::with_capacity(config.iterations as usize);
        for _ in 0..config.iterations {
            let sample = execute_request(client, &resolved, entry.expect_status).await;
            samples.push(sample);
        }
        samples
    } else {
        // Concurrent execution with semaphore-bounded spawns.
        let semaphore = Arc::new(Semaphore::new(config.concurrency as usize));
        let client = Arc::new(client.clone());
        let resolved = Arc::new(resolved);
        let expect_status = entry.expect_status;

        let mut handles = Vec::with_capacity(config.iterations as usize);

        for _ in 0..config.iterations {
            let permit = semaphore.clone().acquire_owned().await?;
            let client = Arc::clone(&client);
            let resolved = Arc::clone(&resolved);

            let handle = tokio::spawn(async move {
                let sample = execute_request(&client, &resolved, expect_status).await;
                drop(permit);
                sample
            });
            handles.push(handle);
        }

        let mut samples = Vec::with_capacity(config.iterations as usize);
        for handle in handles {
            samples.push(handle.await?);
        }
        samples
    };

    let wall_clock = wall_start.elapsed();
    let metrics = ComputedMetrics::from_samples(&samples, wall_clock);

    Ok(EndpointResult {
        module: entry.module.to_string(),
        method: entry.method.to_string(),
        path_template: entry.path_template.to_string(),
        description: entry.description.to_string(),
        resolved_url,
        read_pattern: entry.read_pattern.to_string(),
        risk_tier: format!("{:?}", entry.risk_tier),
        samples,
        metrics,
        wall_clock,
        skipped: false,
        skip_reason: None,
    })
}

mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64() * 1000.0)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = f64::deserialize(d)?;
        Ok(Duration::from_secs_f64(ms / 1000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn test_run_config_defaults() {
        let config = RunConfig {
            iterations: 10,
            concurrency: 1,
            warmup: 2,
        };
        assert_eq!(config.iterations, 10);
        assert_eq!(config.concurrency, 1);
        assert_eq!(config.warmup, 2);
    }

    #[test]
    fn test_endpoint_result_skipped() {
        let result = EndpointResult {
            module: "test".to_string(),
            method: "GET".to_string(),
            path_template: "/test".to_string(),
            description: "test endpoint".to_string(),
            resolved_url: String::new(),
            read_pattern: "KeyLookup".to_string(),
            risk_tier: "Low".to_string(),
            samples: Vec::new(),
            metrics: ComputedMetrics::from_samples(&[], Duration::ZERO),
            wall_clock: Duration::ZERO,
            skipped: true,
            skip_reason: Some("missing param".to_string()),
        };
        assert!(result.skipped);
        assert_eq!(result.skip_reason.as_deref(), Some("missing param"));
    }

    #[tokio::test]
    async fn test_bench_endpoint_skip_on_unresolvable() {
        let client = reqwest::Client::new();
        let entry = EndpointEntry {
            module: "test",
            method: registry::Method::Get,
            path_template: "/test/{id}",
            description: "test endpoint",
            resolve: Box::new(|_base, _params| None),
            expect_status: 200,
            risk_tier: registry::RiskTier::Low,
            read_pattern: registry::ReadPattern::KeyLookup,
        };
        let params = DiscoveredParams::default();
        let config = RunConfig {
            iterations: 5,
            concurrency: 1,
            warmup: 0,
        };

        let result = bench_endpoint(&client, &entry, "http://localhost:0", &params, &config)
            .await
            .unwrap();
        assert!(result.skipped);
        assert!(result.skip_reason.is_some());
        assert!(result.samples.is_empty());
    }
}
