use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
    task::{Context, Poll},
};
use tower::{Layer, Service};

pub type GlobalRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: Arc<GlobalRateLimiter>,
}

impl RateLimitLayer {
    pub fn new(requests_per_second: u32, burst_size: u32) -> Self {
        let requests_per_second = requests_per_second.max(1);
        let burst_size = burst_size.max(1);
        let quota = Quota::per_second(
            NonZeroU32::new(requests_per_second).expect("max(1) ensures non-zero"),
        )
        .allow_burst(NonZeroU32::new(burst_size).expect("max(1) ensures non-zero"));
        let limiter = Arc::new(RateLimiter::direct(quota));
        Self { limiter }
    }

    pub fn with_limiter(limiter: Arc<GlobalRateLimiter>) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: Arc<GlobalRateLimiter>,
}

impl<S> Service<Request<Body>> for RateLimitService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if limiter.check().is_err() {
                return Ok(RateLimitError.into_response());
            }
            inner.call(req).await
        })
    }
}

pub struct RateLimitError;

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": "Too many requests",
            "code": "RATE_LIMIT_EXCEEDED",
            "retry_after_seconds": 1
        });

        (
            StatusCode::TOO_MANY_REQUESTS,
            [("content-type", "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}

#[derive(Clone)]
pub struct IpRateLimitLayer {
    requests_per_second: u32,
    burst_size: u32,
}

impl IpRateLimitLayer {
    pub fn new(requests_per_second: u32, burst_size: u32) -> Self {
        Self {
            requests_per_second,
            burst_size,
        }
    }
}

use governor::state::keyed::DashMapStateStore;

type IpLimiter = RateLimiter<IpAddr, DashMapStateStore<IpAddr>, DefaultClock>;

impl<S> Layer<S> for IpRateLimitLayer {
    type Service = IpRateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let requests_per_second = self.requests_per_second.max(1);
        let burst_size = self.burst_size.max(1);
        let quota = Quota::per_second(
            NonZeroU32::new(requests_per_second).expect("max(1) ensures non-zero"),
        )
        .allow_burst(NonZeroU32::new(burst_size).expect("max(1) ensures non-zero"));

        let limiter = Arc::new(RateLimiter::dashmap(quota));

        IpRateLimitService { inner, limiter }
    }
}

#[derive(Clone)]
pub struct IpRateLimitService<S> {
    inner: S,
    limiter: Arc<IpLimiter>,
}

impl<S> Service<Request<Body>> for IpRateLimitService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();

        let ip = extract_client_ip(&req);

        Box::pin(async move {
            if let Some(ip) = ip {
                if limiter.check_key(&ip).is_err() {
                    return Ok(RateLimitError.into_response());
                }
            }
            inner.call(req).await
        })
    }
}

fn extract_client_ip<B>(req: &Request<B>) -> Option<IpAddr> {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse().ok())
        })
        .or_else(|| {
            req.extensions()
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.ip())
        })
}

#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 100,
            burst_size: 200,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ApiKeyTier {
    Anonymous,
    Free,
    Standard,
    Premium,
}

impl ApiKeyTier {
    pub fn rate_limit(&self) -> RateLimitConfig {
        match self {
            ApiKeyTier::Anonymous => RateLimitConfig {
                requests_per_second: 10,
                burst_size: 20,
            },
            ApiKeyTier::Free => RateLimitConfig {
                requests_per_second: 30,
                burst_size: 60,
            },
            ApiKeyTier::Standard => RateLimitConfig {
                requests_per_second: 100,
                burst_size: 200,
            },
            ApiKeyTier::Premium => RateLimitConfig {
                requests_per_second: 500,
                burst_size: 1000,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiKeyInfo {
    pub key: String,
    pub tier: ApiKeyTier,
    pub name: Option<String>,
}

type ApiKeyLimiter = RateLimiter<String, DashMapStateStore<String>, DefaultClock>;

#[derive(Clone)]
pub struct ApiKeyRateLimitLayer {
    anonymous_limiter: Arc<IpLimiter>,
    key_limiter: Arc<ApiKeyLimiter>,
    api_keys: Arc<dashmap::DashMap<String, ApiKeyInfo>>,
}

impl ApiKeyRateLimitLayer {
    pub fn new() -> Self {
        let anon_config = ApiKeyTier::Anonymous.rate_limit();
        let anon_quota = Quota::per_second(
            NonZeroU32::new(anon_config.requests_per_second)
                .expect("requests_per_second must be non-zero"),
        )
        .allow_burst(NonZeroU32::new(anon_config.burst_size).expect("burst_size must be non-zero"));

        let premium_config = ApiKeyTier::Premium.rate_limit();
        let key_quota = Quota::per_second(
            NonZeroU32::new(premium_config.requests_per_second)
                .expect("requests_per_second must be non-zero"),
        )
        .allow_burst(
            NonZeroU32::new(premium_config.burst_size).expect("burst_size must be non-zero"),
        );

        Self {
            anonymous_limiter: Arc::new(RateLimiter::dashmap(anon_quota)),
            key_limiter: Arc::new(RateLimiter::dashmap(key_quota)),
            api_keys: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn register_key(&self, key: String, tier: ApiKeyTier, name: Option<String>) {
        self.api_keys
            .insert(key.clone(), ApiKeyInfo { key, tier, name });
    }

    pub fn with_keys(self, keys: Vec<(String, ApiKeyTier, Option<String>)>) -> Self {
        for (key, tier, name) in keys {
            self.register_key(key, tier, name);
        }
        self
    }
}

impl Default for ApiKeyRateLimitLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ApiKeyRateLimitLayer {
    type Service = ApiKeyRateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ApiKeyRateLimitService {
            inner,
            anonymous_limiter: self.anonymous_limiter.clone(),
            key_limiter: self.key_limiter.clone(),
            api_keys: self.api_keys.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ApiKeyRateLimitService<S> {
    inner: S,
    anonymous_limiter: Arc<IpLimiter>,
    key_limiter: Arc<ApiKeyLimiter>,
    api_keys: Arc<dashmap::DashMap<String, ApiKeyInfo>>,
}

impl<S> Service<Request<Body>> for ApiKeyRateLimitService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let anonymous_limiter = self.anonymous_limiter.clone();
        let key_limiter = self.key_limiter.clone();
        let api_keys = self.api_keys.clone();
        let mut inner = self.inner.clone();

        let api_key = req
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let client_ip = extract_client_ip(&req);

        Box::pin(async move {
            match api_key {
                Some(key) => {
                    if let Some(key_info) = api_keys.get(&key) {
                        let tier_config = key_info.tier.rate_limit();
                        let cells_to_consume = match key_info.tier {
                            ApiKeyTier::Free => 17,
                            ApiKeyTier::Standard => 5,
                            ApiKeyTier::Premium => 1,
                            ApiKeyTier::Anonymous => 50,
                        };

                        if key_limiter
                            .check_key_n(
                                &key,
                                NonZeroU32::new(cells_to_consume)
                                    .expect("cells_to_consume is always non-zero"),
                            )
                            .is_err()
                        {
                            return Ok(RateLimitErrorWithTier {
                                tier: Some(key_info.tier.clone()),
                                limit: tier_config.requests_per_second,
                            }
                            .into_response());
                        }
                    } else {
                        return Ok(InvalidApiKeyError.into_response());
                    }
                }
                None => {
                    if let Some(ip) = client_ip {
                        if anonymous_limiter.check_key(&ip).is_err() {
                            return Ok(RateLimitErrorWithTier {
                                tier: None,
                                limit: ApiKeyTier::Anonymous.rate_limit().requests_per_second,
                            }
                            .into_response());
                        }
                    }
                }
            }
            inner.call(req).await
        })
    }
}

pub struct RateLimitErrorWithTier {
    tier: Option<ApiKeyTier>,
    limit: u32,
}

impl IntoResponse for RateLimitErrorWithTier {
    fn into_response(self) -> Response {
        let tier_name = match &self.tier {
            Some(ApiKeyTier::Free) => "free",
            Some(ApiKeyTier::Standard) => "standard",
            Some(ApiKeyTier::Premium) => "premium",
            Some(ApiKeyTier::Anonymous) | None => "anonymous",
        };

        let body = serde_json::json!({
            "error": "Too many requests",
            "code": "RATE_LIMIT_EXCEEDED",
            "tier": tier_name,
            "limit_per_second": self.limit,
            "retry_after_seconds": 1
        });

        (
            StatusCode::TOO_MANY_REQUESTS,
            [("content-type", "application/json"), ("retry-after", "1")],
            body.to_string(),
        )
            .into_response()
    }
}

pub struct InvalidApiKeyError;

impl IntoResponse for InvalidApiKeyError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": "Invalid API key",
            "code": "INVALID_API_KEY"
        });

        (
            StatusCode::UNAUTHORIZED,
            [("content-type", "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}
