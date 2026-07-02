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
}

/// Run the API server (API + WebSocket only, no frontend). Blocks until shutdown.
pub async fn run_api(config: ApiServiceConfig) -> Result<()> {
    // When started by the supervisor alongside the indexer, the domain store
    // may not exist yet (the indexer creates it on first open).  Wait up to
    // 60 seconds for the CURRENT marker file before attempting to open the
    // secondary instance — this avoids the "No such file or directory"
    // crash-restart loop on fresh installs.
    let domain_current = Path::new(&config.domain_data_path).join("CURRENT");
    let wait_start = std::time::Instant::now();
    let max_wait = std::time::Duration::from_secs(60);
    while !domain_current.exists() {
        if wait_start.elapsed() >= max_wait {
            bail!(
                "domain store not found after {}s — is the indexer running? (expected: {})",
                max_wait.as_secs(),
                domain_current.display(),
            );
        }
        info!(
            "Waiting for indexer to create domain store ({}s)...",
            wait_start.elapsed().as_secs()
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

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
            match CkbadgerStore::open_network_secondary(primary, sec.as_path()) {
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

    let addr = format!("{}:{}", config.host, config.port);
    info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
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
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone)]
struct FrontendFsState {
    root_dir: PathBuf,
    index_path: PathBuf,
}

#[derive(Clone)]
struct FrontendRuntimeConfig {
    api_port: u16,
    frontend_port: u16,
    ckb_network: String,
    ckb_rpc_url: String,
    build_version: String,
}

pub fn build_frontend_router(config: FrontendServiceConfig) -> Result<Router> {
    let runtime_config = FrontendRuntimeConfig {
        api_port: config.api_port,
        frontend_port: config.port,
        ckb_network: config.ckb_network.clone(),
        ckb_rpc_url: config.ckb_rpc_url.clone(),
        build_version: config.build_version.clone(),
    };

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
            .route("/capabilities", get(frontend_capabilities_handler))
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
            .route("/capabilities", get(frontend_capabilities_handler))
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
    let network = serde_json::to_string(&config.ckb_network)
        .expect("failed to serialize ckb_network for runtime config");
    let rpc_url =
        serde_json::to_string(&config.ckb_rpc_url).expect("failed to serialize ckb_rpc_url");
    let build_version = serde_json::to_string(&config.build_version)
        .expect("failed to serialize build_version for runtime config");
    let body = format!(
        r#"(() => {{
  const protocol = window.location.protocol === 'https:' ? 'https:' : 'http:';
  const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const defaultPort = window.location.protocol === 'https:' ? '443' : '80';
  const currentPort = window.location.port || defaultPort;
  const behindProxy = currentPort !== '{api_port}' && currentPort !== '{frontend_port}';
  const host = behindProxy
    ? window.location.host
    : (window.location.hostname || '127.0.0.1') + ':{api_port}';
  window.__CKBADGER_RUNTIME_CONFIG__ = {{
    apiBase: `${{protocol}}//${{host}}/api/v1`,
    wsUrl: `${{wsProtocol}}//${{host}}/ws`,
    ckbNetwork: {network},
    ckbRpcUrl: {rpc_url},
    buildVersion: {build_version},
  }};
}})();
"#,
        api_port = config.api_port,
        frontend_port = config.frontend_port,
        network = network,
        rpc_url = rpc_url,
        build_version = build_version,
    );

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

async fn frontend_capabilities_handler(headers: HeaderMap) -> Response {
    let origin = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|host| format!("http://{}", host))
        .unwrap_or_default();
    let body = build_capabilities_json(&origin);
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
fn build_capabilities_json(origin: &str) -> String {
    serde_json::json!({
        "origin": origin,
        "site": {
            "name": "ckbadger",
            "apiBase": "/api/v1"
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
                "/forks", "/forks/{id}", "/hardforks",
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

        let router = build_frontend_router(FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 9101,
            ckb_network: "testnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:18114".to_string(),
            build_version: "0.1.0+feature/foo@abcdef123456".to_string(),
            frontend_dir: Some(dir.path().to_path_buf()),
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
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            text.contains("'9101'"),
            "should contain api_port for proxy detection"
        );
        assert!(
            text.contains("'8100'"),
            "should contain frontend_port for proxy detection"
        );
        assert!(
            text.contains(":9101"),
            "should contain api_port in host fallback"
        );
        assert!(text.contains("\"testnet\""));
        assert!(text.contains("\"http://127.0.0.1:18114\""));
        assert!(text.contains("buildVersion"));
        assert!(text.contains("\"0.1.0+feature/foo@abcdef123456\""));
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
        assert_eq!(json["site"]["apiBase"], "/api/v1");
        assert!(json["routes"]["markdown"].as_array().unwrap().len() > 20);
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
