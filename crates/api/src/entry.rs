use anyhow::{bail, Result};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

use ckbadger_store::{
    secondary_store_path, CkbadgerStore, SecondaryStoreOwner, StoreRuntimeConfig,
};

use crate::embedded_frontend;
use crate::{create_router, AppConfig};

/// Configuration for starting the API server.
/// This is the interface the CLI binary uses to start the API.
pub struct ApiServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub host: String,
    pub port: u16,
    pub rate_limit: u32,
    pub rate_limit_burst: u32,
    pub slow_request_threshold_ms: u64,
    pub ckb_db_path: String,
    pub store_runtime_config: StoreRuntimeConfig,
    /// Root directory for content-addressed media blobs.
    pub dob_decode_dir: PathBuf,
    /// Directory where API writes cycles calculation request files for the indexer worker.
    pub cycles_request_dir: Option<std::path::PathBuf>,
    /// Path to the network-crawler store primary. The API opens a read-only
    /// secondary only when this primary already exists (opt-in crawler).
    pub network_data_path: String,
    /// Whether the network crawler is enabled in config (surfaced to the UI).
    pub crawler_enabled: bool,
}

/// One backend network the frontend proxy can route to.
#[derive(Clone, Debug)]
pub struct FrontendNetwork {
    pub name: String,
    pub api_host: String,
    pub api_port: u16,
}

/// Configuration for the standalone frontend server.
#[derive(Clone)]
pub struct FrontendServiceConfig {
    pub host: String,
    pub port: u16,
    pub api_port: u16,
    pub ckb_network: String,
    pub ckb_rpc_url: String,
    pub build_version: String,
    /// Local filesystem override directory (e.g. workdir/frontend/).
    /// When set, serves from disk instead of embedded assets.
    pub frontend_dir: Option<PathBuf>,
    /// All networks this frontend serves (proxy targets). Single-network mode
    /// is a one-element vec matching `ckb_network`/`api_port`.
    pub networks: Vec<FrontendNetwork>,
    /// Default network for the `/` redirect + un-prefixed paths.
    pub default_network: String,
}

/// Run the API server (API + WebSocket only, no frontend). Blocks until shutdown.
pub async fn run_api(config: ApiServiceConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let domain_current = Path::new(&config.domain_data_path).join("CURRENT");
    let append_current = Path::new(&config.append_only_data_path).join("CURRENT");

    // Phase 1: bind immediately, serve 503 "initializing" until the stores exist.
    // When started by the supervisor alongside the indexer — especially a network
    // still queued for sequential bulk sync — the stores may not exist for hours.
    // Instead of bailing and crash-looping under the supervisor, bind the port now
    // and serve 503 "initializing" until the indexer creates both stores, then fall
    // through to phase 2 and serve the real router.
    if !(domain_current.exists() && append_current.exists()) {
        info!("API pre-sync on {} (store not present yet)", addr);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, presync_router(&config.ckb_network))
            .with_graceful_shutdown(wait_for_stores(
                domain_current.clone(),
                append_current.clone(),
            ))
            .await?;
    }

    // Phase 2: stores exist — open them and serve the real router (unchanged below).
    let secondary_path = secondary_store_path(&config.domain_data_path, SecondaryStoreOwner::Api);
    info!(
        "Opening ckbadger domain store (secondary) at: {} -> {}",
        config.domain_data_path,
        secondary_path.display()
    );
    let store = Arc::new(CkbadgerStore::open_domain_secondary_with_runtime(
        Path::new(&config.domain_data_path),
        secondary_path.as_path(),
        config.store_runtime_config,
    )?);

    let append_only_secondary_path =
        secondary_store_path(&config.append_only_data_path, SecondaryStoreOwner::Api);
    info!(
        "Opening ckbadger append-only store (secondary) at: {} -> {}",
        config.append_only_data_path,
        append_only_secondary_path.display()
    );
    let append_only_store = Arc::new(CkbadgerStore::open_append_only_secondary_with_runtime(
        Path::new(&config.append_only_data_path),
        append_only_secondary_path.as_path(),
        config.store_runtime_config,
    )?);

    // The network-crawler store is opt-in: open a read-only secondary only when
    // the crawler has already produced a primary (CURRENT marker present). A
    // missing primary or an open failure is a normal `None`, never a startup
    // error — the API stays read-only and never writes this store.
    let network_store = {
        let primary = Path::new(&config.network_data_path);
        if primary.join("CURRENT").exists() {
            let sec = secondary_store_path(&config.network_data_path, SecondaryStoreOwner::Api);
            info!(
                "Opening ckbadger network store (secondary) at: {} -> {}",
                config.network_data_path,
                sec.display()
            );
            match CkbadgerStore::open_network_secondary_with_runtime(
                primary,
                sec.as_path(),
                config.store_runtime_config,
            ) {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    tracing::warn!("network store present but failed to open secondary: {e}");
                    None
                }
            }
        } else {
            None
        }
    };

    let app_config = AppConfig {
        store,
        append_only_store,
        network_store,
        crawler_enabled: config.crawler_enabled,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        rate_limit_per_second: Some(config.rate_limit),
        rate_limit_burst: Some(config.rate_limit_burst),
        slow_request_threshold_ms: config.slow_request_threshold_ms,
        start_background_tasks: true,
        ckb_db_path: config.ckb_db_path,
        ckb_db_cleanup: None,
        dob_decode_dir: config.dob_decode_dir,
        cycles_request_dir: config.cycles_request_dir.clone(),
    };
    let app = create_router(app_config).await;

    info!("Starting API server on {}", addr);
    let listener = bind_with_retry(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// A minimal router served while a network's store doesn't exist yet: every route
/// returns 503 with the codebase's error shape (`error` = code) so the frontend's
/// `isNetworkInitializingError` (mirroring `warmup_pending`) can detect it.
fn presync_router(network: &str) -> axum::Router {
    let network = network.to_string();
    axum::Router::new().fallback(move || {
        let network = network.clone();
        async move {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "error": "initializing",
                    "network": network,
                    "message": "This network has not started syncing yet",
                })),
            )
        }
    })
}

/// Resolve once both the domain and append-only stores exist (indefinite wait, with
/// periodic progress logs — a queued network may wait hours for its turn).
///
/// Takes owned paths so the returned future is `'static`, as required by
/// `axum::serve(..).with_graceful_shutdown(..)`.
async fn wait_for_stores(domain_current: PathBuf, append_current: PathBuf) {
    let start = std::time::Instant::now();
    loop {
        if domain_current.exists() && append_current.exists() {
            return;
        }
        if start.elapsed().as_secs().is_multiple_of(30) {
            info!(
                "Waiting for indexer to create stores ({}s)...",
                start.elapsed().as_secs()
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Re-bind after the pre-sync listener was dropped. On the Linux/localhost target a
/// freed LISTEN socket rebinds immediately — TIME_WAIT on the pre-sync client
/// connections does not block re-binding the listening port — so a short retry
/// (rather than SO_REUSEADDR) is enough to cover the sub-second drop→rebind gap; the
/// real bind error surfaces if every attempt fails.
async fn bind_with_retry(addr: &str) -> Result<tokio::net::TcpListener> {
    for attempt in 0..20 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(_) if attempt < 19 => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await
            }
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

/// Run the standalone frontend server. Blocks until shutdown.
///
/// Serving priority:
/// 1. `frontend_dir` is set → serve from filesystem (local override)
/// 2. Embedded assets exist → serve from binary
/// 3. Neither → bail with helpful message
pub async fn run_frontend_server(config: FrontendServiceConfig) -> Result<()> {
    let app = build_frontend_router(config.clone())?;

    let addr = format!("{}:{}", config.host, config.port);
    info!("Starting frontend server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

#[derive(Clone)]
struct FrontendFsState {
    root_dir: PathBuf,
    index_path: PathBuf,
}

#[derive(Clone)]
struct FrontendRuntimeConfig {
    ckb_network: String,
    ckb_rpc_url: String,
    build_version: String,
    /// Live networks offered by the SPA switcher; emitted as `[{ name }]`.
    networks: Vec<FrontendNetwork>,
    /// Network the SPA selects by default on first load.
    default_network: String,
}

pub fn build_frontend_router(config: FrontendServiceConfig) -> Result<Router> {
    let runtime_config = FrontendRuntimeConfig {
        ckb_network: config.ckb_network.clone(),
        ckb_rpc_url: config.ckb_rpc_url.clone(),
        build_version: config.build_version.clone(),
        networks: config.networks.clone(),
        default_network: config.default_network.clone(),
    };

    // Network-aware reverse proxy for `/api/{network}/v1/*` + `/ws/{network}`.
    // Built as a self-contained `Router<()>` (its state is erased by `.with_state`)
    // so it can be merged into either serving branch ahead of the SPA fallback.
    // Only one branch runs (each `return`s), so moving it into the first branch
    // that executes and again in the next is sound — the earlier move diverges.
    let proxy_state = Arc::new(crate::frontend_proxy::ProxyState::new(
        config
            .networks
            .iter()
            .map(|n| (n.name.clone(), (n.api_host.clone(), n.api_port)))
            .collect(),
    ));
    let proxy_router = crate::frontend_proxy::proxy_router(proxy_state);

    // The capabilities document describes THIS origin, so it needs the concrete
    // networks the proxy above routes to.
    let capability_networks: Vec<String> = config.networks.iter().map(|n| n.name.clone()).collect();
    let capability_default_network = config.default_network.clone();

    if let Some(frontend_dir) = config.frontend_dir {
        let index_path = frontend_dir.join("index.html");
        if !index_path.is_file() {
            bail!(
                "Frontend assets directory {} is missing index.html",
                frontend_dir.display()
            );
        }

        info!(
            "Frontend server: serving from filesystem at {}",
            frontend_dir.display()
        );

        let state = FrontendFsState {
            root_dir: frontend_dir,
            index_path,
        };

        return Ok(Router::new()
            .route(
                "/runtime-config.js",
                get({
                    let runtime_config = runtime_config.clone();
                    move || frontend_runtime_config_handler(runtime_config.clone())
                }),
            )
            .route(
                "/capabilities",
                get(move |headers| {
                    frontend_capabilities_handler(
                        headers,
                        capability_networks.clone(),
                        capability_default_network.clone(),
                    )
                }),
            )
            .merge(proxy_router)
            .fallback({
                let state = state.clone();
                move |uri| frontend_filesystem_handler(State(state.clone()), uri)
            }));
    }

    if embedded_frontend::has_embedded_assets() {
        info!("Frontend server: serving embedded assets");
        return Ok(Router::new()
            .route(
                "/runtime-config.js",
                get({
                    let runtime_config = runtime_config.clone();
                    move || frontend_runtime_config_handler(runtime_config.clone())
                }),
            )
            .route(
                "/capabilities",
                get(move |headers| {
                    frontend_capabilities_handler(
                        headers,
                        capability_networks.clone(),
                        capability_default_network.clone(),
                    )
                }),
            )
            .merge(proxy_router)
            .fallback(embedded_frontend::embedded_frontend_handler));
    }

    bail!(
        "No frontend assets available. Either:\n  \
         - Build the frontend: cd frontend && pnpm build\n  \
         - Then rebuild the binary: cargo build -p ckbadger\n  \
         - Or place assets in workdir/frontend/dist or workdir/frontend/"
    );
}

async fn frontend_runtime_config_handler(config: FrontendRuntimeConfig) -> Response {
    // Live network list for the SPA switcher. Element shape is `{ name }` — the
    // switcher reads `n.name`; per-network backend ports stay server-side (the
    // reverse proxy owns them) and are intentionally not exposed here.
    let networks = config
        .networks
        .iter()
        .map(|n| serde_json::json!({ "name": n.name }))
        .collect::<Vec<_>>();

    // Base patterns are RELATIVE + same-origin. The literal `{network}` token is
    // a placeholder the SPA substitutes with the active network at call time
    // (and it builds an absolute ws://|wss:// URL from `wsUrlPattern`), so no
    // absolute host/port is computed here anymore.
    let runtime_config = serde_json::json!({
        "networks": networks,
        "defaultNetwork": config.default_network,
        "apiBasePattern": "/api/{network}/v1",
        "wsUrlPattern": "/ws/{network}",
        // Back-compat: still read by resolveCkbNetwork/resolveCkbRpcUrl until
        // their consumers migrate to the per-network patterns (later task).
        // `ckbNetwork` mirrors the orchestrator's default network.
        "ckbNetwork": config.ckb_network,
        "ckbRpcUrl": config.ckb_rpc_url,
        "buildVersion": config.build_version,
    });
    let runtime_config_json =
        serde_json::to_string(&runtime_config).expect("failed to serialize runtime config");

    let body = format!("window.__CKBADGER_RUNTIME_CONFIG__ = {runtime_config_json};\n");

    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn frontend_capabilities_handler(
    headers: HeaderMap,
    networks: Vec<String>,
    default_network: String,
) -> Response {
    let origin = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|host| format!("http://{}", host))
        .unwrap_or_default();
    let body = build_capabilities_json(&origin, &networks, &default_network);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "public, s-maxage=10, stale-while-revalidate=30",
            ),
        ],
        body,
    )
        .into_response()
}

/// Build the `/capabilities` JSON payload.
///
/// This is the Rust equivalent of `frontend/lib/ai/capabilities.ts` `buildAiCapabilities()`.
/// The route patterns are kept in sync manually — they change infrequently.
///
/// Every path here must be a real route on the origin serving the document. This
/// origin is the shared frontend server, where only the network-prefixed
/// patterns exist: `/api/v1` and `/ws` match no route, fall through to the SPA
/// fallback and answer `200 text/html`, so advertising them would hand any agent
/// that trusts this document a path that "succeeds" with a web page. The
/// concrete `networks` list is what makes the `{network}` placeholder usable.
fn build_capabilities_json(origin: &str, networks: &[String], default_network: &str) -> String {
    serde_json::json!({
        "origin": origin,
        "site": {
            "name": "ckbadger",
            "pageBasePattern": "/{network}",
            "apiBasePattern": "/api/{network}/v1",
            "wsUrlPattern": "/ws/{network}",
            "networks": networks,
            "defaultNetwork": default_network
        },
        "formatNegotiation": {
            "priority": ["query.format", "path.suffix", "accept.header"],
            "supportedFormats": ["html", "md", "raw"],
            "markdown": {
                "suffix": ".md",
                "query": "format=md",
                "accept": "text/markdown"
            },
            "raw": {
                "suffix": ".raw",
                "query": "format=raw",
                "accept": "application/vnd.ckbadger.raw+json",
                "profileQuery": "profile=<name>",
                "defaultProfile": "default"
            }
        },
        "responseHeaders": {
            "raw": {
                "formatHeader": "x-ckbadger-format",
                "profileHeader": "x-ckbadger-profile",
                "schemaHeader": "x-ckbadger-schema"
            }
        },
        "responseMetadata": {
            "markdown": {
                "frontmatterFields": [
                    "title", "path", "canonical", "pageType",
                    "generatedAt", "buildVersion", "formatVersion"
                ]
            },
            "raw": {
                "metaFields": [
                    "format", "profile", "schemaVersion", "buildVersion",
                    "network", "path", "canonical", "pageType", "generatedAt"
                ]
            }
        },
        "routes": {
            "markdown": [
                "/", "/activities", "/address/{addr}",
                "/inventory/tokens", "/inventory/objects", "/inventory/identities",
                "/blocks", "/blocks/{id}", "/cell/{outpoint}",
                "/charts", "/charts/{slug}",
                "/classes/{classId}", "/clusters/{clusterId}",
                "/dao", "/dao/charts",
                "/forks", "/forks/{id}", "/hardforks", "/network",
                "/identities/{collectionId}",
                "/identities/dotbit/{identityId}", "/identities/did/{identityId}",
                "/objects", "/objects/{sporeId}", "/objects/mnft/{objectId}",
                "/script/{codeHash}", "/scripts", "/scripts/{name}",
                "/tokens", "/tokens/{typeHash}",
                "/fiber/channels", "/fiber/channels/{channelId}",
                "/transactions", "/tx/{hash}"
            ],
            "raw": [
                "/blocks/{id}", "/cell/{outpoint}",
                "/identities/dotbit/{identityId}", "/identities/did/{identityId}",
                "/objects/mnft/{objectId}", "/tx/{hash}"
            ]
        },
        "rawProfiles": {
            "routes": {
                "/blocks/{id}": ["default"],
                "/cell/{outpoint}": ["default"],
                "/identities/dotbit/{identityId}": ["default"],
                "/identities/did/{identityId}": ["default"],
                "/objects/mnft/{objectId}": ["default"],
                "/tx/{hash}": ["default", "debugger"]
            },
            "strictErrors": {
                "invalidProfile": "invalid_profile",
                "profileNotSupported": "profile_not_supported"
            },
            "txDebuggerProfile": {
                "route": "/tx/{hash}",
                "profile": "debugger",
                "payloadPath": "data.txDebugger.mockTransaction",
                "debuggerCommandTemplate": "curl \"<url>.raw?profile=debugger\" | jq '.data.txDebugger.mockTransaction' > mock_tx.json && ckb-debugger --tx-file mock_tx.json --cell-index 0 --cell-type input --script-group-type lock"
            },
            "txWitnessPayload": {
                "route": "/tx/{hash}",
                "payloadPath": "data.txWitness",
                "fields": ["available", "witnessesCount", "inputCount", "analyses", "inference"]
            }
        }
    })
    .to_string()
}

async fn frontend_filesystem_handler(State(state): State<FrontendFsState>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Prevent path traversal
    if path.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad request").into_response();
    }

    if path.is_empty() {
        return serve_frontend_file(&state.index_path, "").await;
    }

    let candidate = state.root_dir.join(path);

    // Defense-in-depth: verify resolved path is within root_dir
    if let (Ok(canonical), Ok(root_canonical)) =
        (candidate.canonicalize(), state.root_dir.canonicalize())
    {
        if !canonical.starts_with(&root_canonical) {
            return (StatusCode::BAD_REQUEST, "bad request").into_response();
        }
    }

    if candidate.is_file() {
        return serve_frontend_file(&candidate, path).await;
    }

    if !path_looks_like_file(path) {
        return serve_frontend_file(&state.index_path, "").await;
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn serve_frontend_file(path: &Path, request_path: &str) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache_control = if request_path.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache_control),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn path_looks_like_file(path: &str) -> bool {
    let segment = match path.rsplit_once('/') {
        Some((_, s)) => s,
        None => path,
    };
    // A file has a dot-extension like "app.js" or "favicon.ico".
    // Dot-prefixed segments like ".bit" are SPA route params, not files.
    segment.contains('.') && !segment.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[test]
    fn test_api_service_config_fields() {
        let config = ApiServiceConfig {
            domain_data_path: "/data/domain".to_string(),
            append_only_data_path: "/data/append".to_string(),
            ckb_rpc_url: "http://localhost:8114".to_string(),
            ckb_network: "mainnet".to_string(),
            host: "0.0.0.0".to_string(),
            port: 3001,
            rate_limit: 100,
            rate_limit_burst: 200,
            slow_request_threshold_ms: 100,
            ckb_db_path: "/ckb/data/db".to_string(),
            store_runtime_config: StoreRuntimeConfig::default(),
            dob_decode_dir: PathBuf::from("/data/media"),
            cycles_request_dir: None,
            network_data_path: "/data/network".to_string(),
            crawler_enabled: false,
        };

        assert_eq!(config.domain_data_path, "/data/domain");
        assert_eq!(config.append_only_data_path, "/data/append");
        assert_eq!(config.ckb_rpc_url, "http://localhost:8114");
        assert_eq!(config.ckb_network, "mainnet");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3001);
        assert_eq!(config.rate_limit, 100);
        assert_eq!(config.rate_limit_burst, 200);
        assert_eq!(config.ckb_db_path, "/ckb/data/db");
        assert_eq!(config.store_runtime_config, StoreRuntimeConfig::default());
        assert_eq!(config.dob_decode_dir, PathBuf::from("/data/media"));
        assert_eq!(config.network_data_path, "/data/network");
        assert!(!config.crawler_enabled);
    }

    #[test]
    fn test_frontend_service_config_fields() {
        let config = FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
            build_version: "0.1.0+testbuild".to_string(),
            frontend_dir: Some(PathBuf::from("/work/frontend")),
            default_network: "mainnet".to_string(),
            networks: vec![FrontendNetwork {
                name: "mainnet".to_string(),
                api_host: "127.0.0.1".to_string(),
                api_port: 8101,
            }],
        };

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8100);
        assert_eq!(config.api_port, 8101);
        assert_eq!(config.ckb_network, "mainnet");
        assert_eq!(config.ckb_rpc_url, "http://127.0.0.1:8114");
        assert_eq!(
            config.frontend_dir.as_ref().unwrap(),
            &PathBuf::from("/work/frontend")
        );
        assert_eq!(config.default_network, "mainnet");
        assert_eq!(config.networks.len(), 1);
        assert_eq!(config.networks[0].name, "mainnet");
        assert_eq!(config.networks[0].api_port, 8101);
    }

    #[test]
    fn test_frontend_service_config_no_dir() {
        let config = FrontendServiceConfig {
            host: "0.0.0.0".to_string(),
            port: 3000,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
            build_version: "0.1.0+testbuild".to_string(),
            frontend_dir: None,
            default_network: "mainnet".to_string(),
            networks: vec![FrontendNetwork {
                name: "mainnet".to_string(),
                api_host: "127.0.0.1".to_string(),
                api_port: 8101,
            }],
        };

        assert!(config.frontend_dir.is_none());
    }

    #[test]
    fn test_path_looks_like_file() {
        assert!(path_looks_like_file("favicon.ico"));
        assert!(path_looks_like_file("assets/app.js"));
        assert!(!path_looks_like_file("script/0x1234"));
        assert!(!path_looks_like_file("blocks"));
        // Dot-prefixed segments are SPA route params, not files
        assert!(!path_looks_like_file("identities/.bit"));
        assert!(!path_looks_like_file("identities/did:ckb"));
        assert!(!path_looks_like_file("identities/dotbit"));
    }

    #[tokio::test]
    async fn test_build_frontend_router_bails_without_index_html() {
        let dir = tempfile::tempdir().unwrap();
        let err = build_frontend_router(FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
            build_version: "0.1.0+testbuild".to_string(),
            frontend_dir: Some(dir.path().to_path_buf()),
            default_network: "mainnet".to_string(),
            networks: vec![FrontendNetwork {
                name: "mainnet".to_string(),
                api_host: "127.0.0.1".to_string(),
                api_port: 8101,
            }],
        })
        .unwrap_err();

        assert!(err.to_string().contains("missing index.html"));
    }

    #[tokio::test]
    async fn test_build_frontend_router_returns_404_for_missing_file_like_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();
        let router = build_frontend_router(FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
            build_version: "0.1.0+testbuild".to_string(),
            frontend_dir: Some(dir.path().to_path_buf()),
            default_network: "mainnet".to_string(),
            networks: vec![FrontendNetwork {
                name: "mainnet".to_string(),
                api_host: "127.0.0.1".to_string(),
                api_port: 8101,
            }],
        })
        .unwrap();

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/assets/missing.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, "not found");
    }

    #[tokio::test]
    async fn test_frontend_runtime_config_route_uses_service_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();

        // Orchestrator-shape config: two live networks, default mainnet.
        let router = build_frontend_router(FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
            build_version: "0.1.0+feature/foo@abcdef123456".to_string(),
            frontend_dir: Some(dir.path().to_path_buf()),
            default_network: "mainnet".to_string(),
            networks: vec![
                FrontendNetwork {
                    name: "mainnet".to_string(),
                    api_host: "127.0.0.1".to_string(),
                    api_port: 8101,
                },
                FrontendNetwork {
                    name: "testnet".to_string(),
                    api_host: "127.0.0.1".to_string(),
                    api_port: 8102,
                },
            ],
        })
        .unwrap();

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/runtime-config.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/javascript; charset=utf-8"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();

        // Assigns the global the SPA reads.
        assert!(
            text.contains("window.__CKBADGER_RUNTIME_CONFIG__ ="),
            "missing global assignment: {text}"
        );

        // Live-network list (the SPA switcher reads `n.name`).
        assert!(
            text.contains("\"networks\""),
            "missing networks key: {text}"
        );
        assert!(text.contains("\"mainnet\""), "missing mainnet name: {text}");
        assert!(text.contains("\"testnet\""), "missing testnet name: {text}");
        assert!(
            text.contains("\"defaultNetwork\":\"mainnet\""),
            "missing defaultNetwork: {text}"
        );

        // Network-relative, same-origin base patterns; the literal {network}
        // placeholder is substituted by the SPA for the active network.
        assert!(
            text.contains("\"apiBasePattern\":\"/api/{network}/v1\""),
            "missing apiBasePattern: {text}"
        );
        assert!(
            text.contains("\"wsUrlPattern\":\"/ws/{network}\""),
            "missing wsUrlPattern: {text}"
        );

        // Back-compat fields still emitted for un-migrated consumers.
        assert!(
            text.contains("buildVersion"),
            "missing buildVersion: {text}"
        );
        assert!(
            text.contains("\"0.1.0+feature/foo@abcdef123456\""),
            "missing build version value: {text}"
        );
        assert!(
            text.contains("\"ckbNetwork\":\"mainnet\""),
            "missing ckbNetwork: {text}"
        );

        // Old absolute-URL fields are gone (superseded by the relative patterns).
        assert!(
            !text.contains("\"apiBase\":"),
            "legacy apiBase should be removed: {text}"
        );
        assert!(
            !text.contains("\"wsUrl\":"),
            "legacy wsUrl should be removed: {text}"
        );
    }

    #[tokio::test]
    async fn test_build_frontend_router_multi_network_config_builds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();

        // An orchestrator-shape config carrying two networks must build a router
        // without panic (single-network is just the one-element case).
        let router = build_frontend_router(FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: String::new(),
            build_version: "0.1.0+testbuild".to_string(),
            frontend_dir: Some(dir.path().to_path_buf()),
            default_network: "mainnet".to_string(),
            networks: vec![
                FrontendNetwork {
                    name: "mainnet".to_string(),
                    api_host: "127.0.0.1".to_string(),
                    api_port: 8101,
                },
                FrontendNetwork {
                    name: "testnet".to_string(),
                    api_host: "127.0.0.1".to_string(),
                    api_port: 8102,
                },
            ],
        })
        .expect("two-network config should build a router");

        // The SPA still serves through the multi-network router.
        let response = router
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_capabilities_route_returns_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();

        let router = build_frontend_router(FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
            build_version: "0.1.0+testbuild".to_string(),
            frontend_dir: Some(dir.path().to_path_buf()),
            default_network: "mainnet".to_string(),
            networks: vec![
                FrontendNetwork {
                    name: "mainnet".to_string(),
                    api_host: "127.0.0.1".to_string(),
                    api_port: 8101,
                },
                FrontendNetwork {
                    name: "testnet".to_string(),
                    api_host: "127.0.0.1".to_string(),
                    api_port: 8102,
                },
            ],
        })
        .unwrap();

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/capabilities")
                    .header("host", "localhost:8100")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["origin"], "http://localhost:8100");
        assert_eq!(json["site"]["name"], "ckbadger");
        assert_eq!(json["site"]["pageBasePattern"], "/{network}");
        assert_eq!(json["site"]["apiBasePattern"], "/api/{network}/v1");
        assert_eq!(json["site"]["wsUrlPattern"], "/ws/{network}");
        // The `{network}` placeholder is only usable with the list of networks
        // this origin actually serves.
        assert_eq!(
            json["site"]["networks"],
            serde_json::json!(["mainnet", "testnet"])
        );
        assert_eq!(json["site"]["defaultNetwork"], "mainnet");
        // The single-network paths are NOT routes on this origin: they miss the
        // proxy and fall through to the SPA, which answers 200 text/html.
        // Advertising them in a machine-readable document poisons any agent that
        // trusts it, so they must be absent.
        assert!(json["site"].get("directApiBase").is_none());
        assert!(json["site"].get("directWsUrl").is_none());
        assert!(json["site"].get("apiBase").is_none());
        assert!(json["routes"]["markdown"].as_array().unwrap().len() > 20);
        assert!(
            json["routes"]["markdown"]
                .as_array()
                .unwrap()
                .iter()
                .any(|route| route == "/network"),
            "Rust capabilities route matrix must advertise the network page"
        );
        assert!(!json["routes"]["raw"].as_array().unwrap().is_empty());
        assert_eq!(
            json["formatNegotiation"]["raw"]["accept"],
            "application/vnd.ckbadger.raw+json"
        );
        assert_eq!(
            json["rawProfiles"]["txDebuggerProfile"]["profile"],
            "debugger"
        );
    }
}

#[cfg(test)]
mod presync_tests {
    use super::presync_router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // oneshot

    #[tokio::test]
    async fn presync_router_returns_503_initializing_for_any_path() {
        let app = presync_router("testnet");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/v1/statistics/network")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "initializing");
        assert_eq!(json["network"], "testnet");
        assert!(json["message"].as_str().unwrap().contains("sync"));
    }
}
