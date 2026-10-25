use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::metrics::{ComputedMetrics, Sample, SampleFailureKind};
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

/// Read the whole response before counting a request as successful. A truncated
/// HTTP body may arrive after a valid status line and must fail the sample.
pub async fn sample_response(
    response: reqwest::Response,
    expect_status: u16,
    latency_ms: f64,
) -> Sample {
    let status = response.status().as_u16();
    match response.bytes().await {
        Ok(body) => Sample {
            latency_ms,
            status,
            body_size: Some(body.len()),
            error: (status != expect_status)
                .then(|| format!("expected status {}, got {}", expect_status, status)),
            failure_kind: (status != expect_status).then_some(SampleFailureKind::UnexpectedStatus),
        },
        Err(error) => Sample {
            latency_ms,
            status,
            body_size: None,
            error: Some(format!("response body read failed: {error}")),
            failure_kind: Some(SampleFailureKind::BodyRead),
        },
    }
}

/// Execute a single HTTP request and record a `Sample`.
pub async fn execute_request(
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
        Ok(response) => sample_response(response, expect_status, latency_ms).await,
        Err(e) => Sample {
            latency_ms,
            status: 0,
            body_size: None,
            error: Some(e.to_string()),
            failure_kind: Some(SampleFailureKind::Transport),
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn truncated_success_body_is_a_failed_sample() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nshort",
                )
                .await
                .unwrap();
        });
        let sample = execute_request(
            &reqwest::Client::new(),
            &ResolvedRequest {
                url: format!("http://{address}/truncated"),
                method: Method::Get,
                body: None,
            },
            200,
        )
        .await;
        server.await.unwrap();
        assert_eq!(sample.status, 200);
        assert_eq!(sample.body_size, None);
        assert!(
            sample.error.is_some(),
            "body read error must fail the sample"
        );
        let stress_sample = crate::stress::collector::StressSample {
            endpoint_idx: 0,
            endpoint_path: "/truncated".to_string(),
            read_pattern: "KeyLookup".to_string(),
            latency_ms: sample.latency_ms,
            status: sample.status,
            body_size: sample.body_size,
            error: sample.error.clone(),
            failure_kind: sample.failure_kind,
        };
        let baseline =
            crate::stress::collector::StageResult::from_samples(0, 1, Duration::from_secs(1), &[]);
        let stage = crate::stress::collector::StageResult::from_samples(
            1,
            1,
            Duration::from_secs(1),
            &[stress_sample],
        );
        assert_eq!(stage.server_error_count, 1);
        assert_eq!(stage.server_error_rate, 1.0);
        assert_eq!(
            crate::stress::collector::detect_degradation(&baseline, &stage),
            crate::stress::collector::DegradationSignal::HardFailure
        );
        let metrics =
            crate::metrics::ComputedMetrics::from_samples(&[sample], Duration::from_secs(1));
        assert_eq!(metrics.avg_body_size, None);
    }

    #[tokio::test]
    async fn truncated_429_is_a_server_failure_but_complete_429_is_rate_limited() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for response in [
                &b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 20\r\nConnection: close\r\n\r\nshort"[..],
                &b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"[..],
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                socket.write_all(response).await.unwrap();
            }
        });
        let client = reqwest::Client::new();
        let resolved = ResolvedRequest {
            url: format!("http://{address}/rate-limited"),
            method: Method::Get,
            body: None,
        };
        let truncated = execute_request(&client, &resolved, 200).await;
        let complete = execute_request(&client, &resolved, 200).await;
        server.await.unwrap();
        assert_eq!(truncated.status, 429);
        assert_eq!(truncated.failure_kind, Some(SampleFailureKind::BodyRead));
        assert_eq!(truncated.body_size, None);
        assert_eq!(complete.status, 429);
        assert_eq!(
            complete.failure_kind,
            Some(SampleFailureKind::UnexpectedStatus)
        );
        assert_eq!(complete.body_size, Some(5));

        let into_stress = |sample: Sample| crate::stress::collector::StressSample {
            endpoint_idx: 0,
            endpoint_path: "/rate-limited".to_string(),
            read_pattern: "KeyLookup".to_string(),
            latency_ms: sample.latency_ms,
            status: sample.status,
            body_size: sample.body_size,
            error: sample.error,
            failure_kind: sample.failure_kind,
        };
        let baseline =
            crate::stress::collector::StageResult::from_samples(0, 1, Duration::from_secs(1), &[]);
        let broken = crate::stress::collector::StageResult::from_samples(
            1,
            1,
            Duration::from_secs(1),
            &[into_stress(truncated)],
        );
        let endpoint = &broken.per_endpoint[&0];
        assert_eq!(broken.error_count, 1);
        assert_eq!(broken.server_error_count, 1);
        assert_eq!(broken.rate_limited_count, 0);
        assert_eq!(broken.error_rate, 1.0);
        assert_eq!(broken.server_error_rate, 1.0);
        assert_eq!(endpoint.error_rate, 1.0);
        assert_eq!(endpoint.server_error_rate, 1.0);
        assert_eq!(
            crate::stress::collector::detect_degradation(&baseline, &broken),
            crate::stress::collector::DegradationSignal::HardFailure
        );

        let limited = crate::stress::collector::StageResult::from_samples(
            1,
            1,
            Duration::from_secs(1),
            &[into_stress(complete)],
        );
        let endpoint = &limited.per_endpoint[&0];
        assert_eq!(limited.error_count, 1);
        assert_eq!(limited.server_error_count, 0);
        assert_eq!(limited.rate_limited_count, 1);
        assert_eq!(limited.error_rate, 0.0);
        assert_eq!(limited.server_error_rate, 0.0);
        assert_eq!(endpoint.error_rate, 0.0);
        assert_eq!(endpoint.server_error_rate, 0.0);
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
