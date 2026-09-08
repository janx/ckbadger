//! Production page-format bridge. The existing TypeScript renderers are bundled
//! into this binary and run in an isolated QuickJS context, without Node or an
//! external renderer service. Rust owns all outbound I/O and resource limits.

use anyhow::{anyhow, bail, Context, Result};
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Function, Object, Promise};
use rust_embed::Embed;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::entry::{FrontendNetwork, FrontendServiceConfig};
use crate::frontend_proxy::UpstreamTarget;

const RENDER_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

struct FetchBudget {
    bytes: AtomicUsize,
    requests: AtomicUsize,
    in_flight: Semaphore,
}

impl FetchBudget {
    fn new() -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            in_flight: Semaphore::new(8),
        }
    }
}

#[derive(Embed)]
#[folder = "../../frontend/server-dist/"]
#[allow_missing = true]
struct RendererAssets;

pub(crate) struct FormatState {
    bundle: Arc<[u8]>,
    config: FrontendServiceConfig,
    public_origin: Option<String>,
    capabilities: Value,
    route_documentation: String,
    client: reqwest::Client,
    permits: Arc<Semaphore>,
}

impl FormatState {
    pub(crate) fn new(config: FrontendServiceConfig) -> Result<Arc<Self>> {
        let public_origin = config
            .public_origin
            .as_deref()
            .map(validate_origin)
            .transpose()
            .context("invalid frontend.public_origin")?;
        let bundle: Arc<[u8]> = RendererAssets::get("agent-renderer.js")
            .context("Missing embedded agent renderer: run pnpm --dir frontend build, then rebuild ckbadger")?
            .data.into_owned().into();
        let runtime = rquickjs::Runtime::new()?;
        runtime.set_memory_limit(64 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime)?;
        let (capabilities, route_documentation) = context.with(|ctx| -> Result<_> {
            ctx.eval::<(), _>(bundle.as_ref())
                .catch(&ctx)
                .map_err(|e| anyhow!(e.to_string()))?;
            let module: Object = ctx.globals().get("CkbadgerAgent")?;
            let discovery: Function = module.get("discovery")?;
            let input = json!({
                "pathname": "/", "origin": "", "runtimeConfig": runtime_config(&config, None),
            });
            let output: String = discovery
                .call((input.to_string(),))
                .catch(&ctx)
                .map_err(|e| anyhow!(e.to_string()))?;
            let docs: Function = module.get("routeDocumentation")?;
            Ok((serde_json::from_str(&output)?, docs.call::<_, String>(())?))
        })?;
        Ok(Arc::new(Self {
            bundle,
            config,
            public_origin,
            capabilities,
            route_documentation,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(RENDER_TIMEOUT)
                .build()?,
            permits: Arc::new(Semaphore::new(4)),
        }))
    }

    fn origin(&self, request: &Request) -> Result<String> {
        if let Some(origin) = &self.public_origin {
            return Ok(origin.clone());
        }
        let headers = request.headers();
        let trusted = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|peer| peer.0.ip().is_loopback());
        let host = if trusted {
            single_header(headers, "x-forwarded-host")?
        } else {
            None
        }
        .or(single_header(headers, "host")?)
        .context("request has no Host; configure frontend.public_origin")?;
        let scheme = if trusted {
            single_header(headers, "x-forwarded-proto")?
        } else {
            None
        }
        .unwrap_or("http");
        validate_origin(&format!("{scheme}://{host}"))
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().map(|value| value.to_str()).transpose()?;
    if values.next().is_some() || value.is_some_and(|value| value.contains(',')) {
        bail!("ambiguous {name} header; configure frontend.public_origin");
    }
    Ok(value)
}

fn validate_origin(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || value.chars().any(char::is_whitespace)
        || value.contains('\\')
    {
        bail!("expected an HTTP(S) origin without credentials, path, query or fragment: {value}");
    }
    Ok(url.origin().ascii_serialization())
}

fn runtime_config(config: &FrontendServiceConfig, network: Option<&FrontendNetwork>) -> Value {
    json!({
        "networks": config.networks.iter().map(|n| json!({"name": n.name})).collect::<Vec<_>>(),
        "defaultNetwork": config.default_network,
        "ckbNetwork": network.map(|n| n.name.as_str()).unwrap_or(&config.default_network),
        "ckbRpcUrl": network.map(|n| n.ckb_rpc_url.as_str()).unwrap_or("/_ckbadger/rpc"),
        "buildVersion": config.build_version,
        "apiBasePattern": "/api/{network}/v1", "wsUrlPattern": "/ws/{network}",
    })
}

pub(crate) async fn capabilities(
    State(state): State<Arc<FormatState>>,
    request: Request,
) -> Response {
    let origin = match state.origin(&request) {
        Ok(origin) => origin,
        Err(error) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_origin", error.to_string())
        }
    };
    let mut capabilities = state.capabilities.clone();
    capabilities["origin"] = origin.into();
    with_representation_headers(
        (
            [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
            capabilities.to_string(),
        )
            .into_response(),
    )
}

pub(crate) async fn llms(State(state): State<Arc<FormatState>>, request: Request) -> Response {
    let source = if request.uri().path() == "/llms-full.txt" {
        include_str!("../../../frontend/public/llms-full.txt")
    } else {
        include_str!("../../../frontend/public/llms.txt")
    };
    with_representation_headers(
        (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            source.replace(
                "<!-- REGISTERED_PAGE_FORMATS -->",
                &state.route_documentation,
            ),
        )
            .into_response(),
    )
}

fn with_representation_headers(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn error_response(status: StatusCode, code: &str, message: String) -> Response {
    with_representation_headers(
        (
            status,
            axum::Json(json!({"error": {"code": code, "message": message}})),
        )
            .into_response(),
    )
}

// This is only an inexpensive candidate filter. The bundled negotiation function
// makes the sole representation decision, including q-values and query priority.
fn is_page_request(request: &Request, state: &FormatState) -> bool {
    let path = request.uri().path();
    let network_page = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .is_some_and(|name| {
            state
                .config
                .networks
                .iter()
                .any(|network| network.name == name)
        });
    !["/api", "/ws", "/assets"]
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
        && ![
            "/capabilities",
            "/runtime-config.js",
            "/llms.txt",
            "/llms-full.txt",
        ]
        .contains(&path)
        && (network_page
            || path.ends_with(".md")
            || path.ends_with(".raw")
            || !path
                .rsplit('/')
                .next()
                .is_some_and(|segment| segment.contains('.') && !segment.starts_with('.')))
}

pub(crate) async fn negotiate(
    State(state): State<Arc<FormatState>>,
    mut request: Request,
    next: Next,
) -> Response {
    if !is_page_request(&request, &state) {
        return next.run(request).await;
    }
    let accept = request
        .headers()
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(",");
    let path = request.uri().path().to_string();
    let normalized_path = path.trim_end_matches('/');
    let query = request.uri().query().unwrap_or("").to_string();
    let has_format_query = query.split('&').any(|pair| {
        let key = pair.split('=').next().unwrap_or("");
        key == "format" || key.contains('%')
    });
    let normalized_accept = accept.to_ascii_lowercase();
    if !normalized_path.ends_with(".md")
        && !normalized_path.ends_with(".raw")
        && !has_format_query
        && !normalized_accept.contains("text/markdown")
        && !normalized_accept.contains("application/vnd.ckbadger.raw+json")
    {
        return with_representation_headers(next.run(request).await);
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Page formats require GET or HEAD".into(),
        );
    }
    let origin = match state.origin(&request) {
        Ok(origin) => origin,
        Err(error) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_origin", error.to_string())
        }
    };
    let Ok(permit) = state.permits.clone().try_acquire_owned() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "renderer_busy",
            "All four page renderer slots are in use; retry later".into(),
        );
    };
    let network = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .map(|name| {
            name.strip_suffix(".md")
                .or_else(|| name.strip_suffix(".raw"))
                .unwrap_or(name)
        })
        .and_then(|name| state.config.networks.iter().find(|n| n.name == name))
        .cloned();
    let input = json!({
        "pathname": path, "query": query, "accept": accept, "method": request.method().as_str(),
        "origin": origin, "runtimeConfig": runtime_config(&state.config, network.as_ref()),
    })
    .to_string();
    let client_ip = crate::utils::client_ip::resolve_client_ip(
        request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|peer| peer.0.ip()),
        request.headers(),
    );
    let handle = tokio::runtime::Handle::current();
    let rendered = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        handle.block_on(run_renderer(state, network, client_ip, input))
    })
    .await;
    let output = match rendered {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let timed_out = error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some();
            return error_response(
                if timed_out {
                    StatusCode::GATEWAY_TIMEOUT
                } else {
                    StatusCode::BAD_GATEWAY
                },
                if timed_out {
                    "renderer_timeout"
                } else {
                    "renderer_failed"
                },
                format!("{path}: {error:#}"),
            );
        }
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "renderer_failed",
                format!("{path}: {error}"),
            )
        }
    };
    if let Some(html_path) = output.html_path {
        let uri = if query.is_empty() {
            html_path
        } else {
            format!("{html_path}?{query}")
        };
        *request.uri_mut() = match uri.parse() {
            Ok(uri) => uri,
            Err(error) => {
                return error_response(StatusCode::BAD_REQUEST, "invalid_path", format!("{error}"))
            }
        };
        return with_representation_headers(next.run(request).await);
    }
    let mut response = Response::builder().status(output.status);
    for (name, value) in output.headers {
        response = response.header(name, value);
    }
    let body = if request.method() == Method::HEAD {
        Body::empty()
    } else {
        Body::from(output.body)
    };
    response.body(body).unwrap_or_else(|error| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_renderer_response",
            error.to_string(),
        )
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderOutput {
    status: u16,
    body: String,
    headers: HashMap<String, String>,
    html_path: Option<String>,
}

async fn run_renderer(
    state: Arc<FormatState>,
    network: Option<FrontendNetwork>,
    client_ip: Option<std::net::IpAddr>,
    input: String,
) -> Result<RenderOutput> {
    let deadline = Instant::now() + RENDER_TIMEOUT;
    let runtime = AsyncRuntime::new()?;
    runtime.set_memory_limit(64 * 1024 * 1024).await;
    runtime.set_max_stack_size(1024 * 1024).await;
    runtime
        .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)))
        .await;
    let context = AsyncContext::full(&runtime).await?;
    tokio::time::timeout(RENDER_TIMEOUT, context.async_with(async |ctx| -> Result<RenderOutput> {
        let client = state.client.clone();
        let budget = Arc::new(FetchBudget::new());
        let fetch = Function::new(ctx.clone(), rquickjs::function::Async(move |url: String, method: String, body: String| {
            let client = client.clone(); let network = network.clone();
            let budget = budget.clone();
            async move {
                let result = fetch_json(&client, network.as_ref(), client_ip, &url, &method, &body, &budget).await;
                let (status, body) = match result {
                    Ok(response) => response,
                    Err(error) => {
                        let status = if error.downcast_ref::<reqwest::Error>().is_some_and(|e| e.is_timeout()) { 504 } else { 502 };
                        (status, json!({"error": "upstream_error", "message": format!("{url}: {error:#}")}).to_string())
                    }
                };
                json!({"status": status, "body": body}).to_string()
            }
        }))?;
        ctx.globals().set("__ckbadgerFetch", fetch)?;
        ctx.eval::<(), _>(state.bundle.as_ref()).catch(&ctx).map_err(|e| anyhow!(e.to_string()))?;
        let module: Object = ctx.globals().get("CkbadgerAgent")?;
        let render: Function = module.get("render")?;
        let promise: Promise = render.call((input,))?;
        let output = promise.into_future::<String>().await.catch(&ctx).map_err(|e| anyhow!(e.to_string()))?;
        Ok(serde_json::from_str(&output)?)
    })).await.context("page rendering exceeded 30 seconds")?
}

async fn fetch_json(
    client: &reqwest::Client,
    network: Option<&FrontendNetwork>,
    client_ip: Option<std::net::IpAddr>,
    requested_url: &str,
    method: &str,
    body: &str,
    budget: &FetchBudget,
) -> Result<(u16, String)> {
    let network = network.context("no configured network for renderer request")?;
    let prefix = format!("/api/{}/v1/", network.name);
    let url = if let Some(rest) = requested_url.strip_prefix(&prefix) {
        let (path, query) = rest
            .split_once('?')
            .map(|(p, q)| (p, format!("?{q}")))
            .unwrap_or((rest, String::new()));
        if method != "GET" && !(method == "POST" && path == "scripts/lookup") {
            bail!("renderer API requests must be read-only: {method} {requested_url}");
        }
        let target = UpstreamTarget::from_bind_host(network.api_host.clone(), network.api_port);
        let url = target
            .http_url(path, &query)
            .context("invalid renderer API path")?;
        if !url.path().starts_with("/api/v1/") {
            bail!("renderer API path escapes /api/v1/: {requested_url}");
        }
        url
    } else if requested_url == network.ckb_rpc_url && method == "POST" {
        let rpc: Value = serde_json::from_str(body)?;
        if !matches!(
            rpc["method"].as_str(),
            Some("get_transaction" | "get_live_cell" | "get_header")
        ) {
            bail!(
                "renderer RPC method is not read-only or not registered: {}",
                rpc["method"]
            );
        }
        reqwest::Url::parse(&network.ckb_rpc_url)?
    } else {
        bail!("renderer requested an unconfigured upstream: {requested_url}");
    };
    if budget.requests.fetch_add(1, Ordering::Relaxed) >= 256 {
        bail!("renderer exceeded 256 upstream requests: {requested_url}");
    }
    let _permit = budget.in_flight.acquire().await?;
    let mut builder = client
        .request(method.parse()?, url)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(ip) = client_ip {
        builder = builder
            .header("x-forwarded-for", ip.to_string())
            .header("x-real-ip", ip.to_string());
    }
    let mut response = builder.body(body.to_string()).send().await?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            bail!("renderer upstream response exceeds 16 MiB: {requested_url}");
        }
        budget
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                total
                    .checked_add(chunk.len())
                    .filter(|bytes| *bytes <= 64 * 1024 * 1024)
            })
            .map_err(|_| {
                anyhow!(
                    "renderer upstream responses exceed the 64 MiB total budget: {requested_url}"
                )
            })?;
        bytes.extend_from_slice(&chunk);
    }
    // Surface non-JSON upstream failures instead of letting HTML look like data.
    serde_json::from_slice::<Value>(&bytes).context("renderer upstream did not return JSON")?;
    Ok((status, String::from_utf8(bytes)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(public_origin: Option<&str>) -> Arc<FormatState> {
        FormatState::new(FrontendServiceConfig {
            host: "127.0.0.1".into(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".into(),
            ckb_rpc_url: String::new(),
            public_origin: public_origin.map(str::to_string),
            build_version: "test".into(),
            frontend_dir: None,
            default_network: "mainnet".into(),
            networks: vec![FrontendNetwork {
                name: "mainnet".into(),
                api_host: "127.0.0.1".into(),
                api_port: 8101,
                ckb_rpc_url: "http://127.0.0.1:8114".into(),
            }],
        })
        .unwrap()
    }

    #[test]
    fn public_origin_requires_an_origin_not_a_url_with_extra_components() {
        assert_eq!(
            validate_origin("https://explorer.example/").unwrap(),
            "https://explorer.example"
        );
        assert_eq!(
            validate_origin("http://[::1]:8100").unwrap(),
            "http://[::1]:8100"
        );
        for value in [
            "ftp://example.org",
            "https://example.org/path",
            "https://example.org?x=1",
            "https://example.org#x",
            "https://user:secret@example.org",
            "https://example.org\\path",
            " https://example.org",
        ] {
            assert!(validate_origin(value).is_err(), "{value}");
        }
    }

    #[test]
    fn origin_uses_explicit_config_then_only_loopback_forwarding_headers() {
        let request = |peer: &str| {
            let mut request = Request::builder()
                .header("host", "127.0.0.1:8100")
                .header("x-forwarded-host", "explorer.example")
                .header("x-forwarded-proto", "https")
                .body(Body::empty())
                .unwrap();
            request
                .extensions_mut()
                .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
            request
        };
        let state = state(None);
        assert_eq!(
            state.origin(&request("127.0.0.1:9999")).unwrap(),
            "https://explorer.example"
        );
        assert_eq!(
            state.origin(&request("203.0.113.1:9999")).unwrap(),
            "http://127.0.0.1:8100"
        );
        let mut no_peer = request("127.0.0.1:9999");
        no_peer.extensions_mut().clear();
        assert_eq!(state.origin(&no_peer).unwrap(), "http://127.0.0.1:8100");
        let configured = self::state(Some("https://configured.example"));
        assert_eq!(
            configured.origin(&request("127.0.0.1:9999")).unwrap(),
            "https://configured.example"
        );
    }

    #[tokio::test]
    async fn renderer_cannot_use_another_network_or_mutating_rpc() {
        let client = reqwest::Client::new();
        let network = FrontendNetwork {
            name: "testnet".into(),
            api_host: "127.0.0.1".into(),
            api_port: 1,
            ckb_rpc_url: "http://127.0.0.1:2".into(),
        };
        for (url, method, body, expected) in [
            (
                "/api/mainnet/v1/blocks/42",
                "GET",
                "",
                "unconfigured upstream",
            ),
            (
                "/api/testnet/v1/blocks/42",
                "DELETE",
                "",
                "must be read-only",
            ),
            ("/api/testnet/v1/../../ws", "GET", "", "escapes /api/v1/"),
            (
                "http://127.0.0.1:2",
                "POST",
                r#"{"method":"send_transaction"}"#,
                "not read-only",
            ),
        ] {
            let error = fetch_json(
                &client,
                Some(&network),
                None,
                url,
                method,
                body,
                &FetchBudget::new(),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
    }
}
