use anyhow::{bail, Result};
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Router};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

use ckbadger_store::{
    secondary_store_path, CkbadgerStore, SecondaryStoreOwner, StoreRuntimeConfig,
};

use crate::embedded_frontend;
use crate::{create_router, AppConfig};

fn require_ckb_data_path<'a>(path: Option<&'a str>, context: &str) -> Result<&'a str> {
    let path = path.map(str::trim).unwrap_or_default();
    if path.is_empty() {
        bail!(
            "{}: [ckb].data_path is required and must point to the CKB node RocksDB directory",
            context
        );
    }
    Ok(path)
}

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
    pub ckb_data_path: Option<String>,
    pub store_runtime_config: StoreRuntimeConfig,
}

/// Configuration for the standalone frontend server.
#[derive(Clone)]
pub struct FrontendServiceConfig {
    pub host: String,
    pub port: u16,
    pub api_port: u16,
    pub ckb_network: String,
    pub ckb_rpc_url: String,
    /// Local filesystem override directory (e.g. workdir/frontend/).
    /// When set, serves from disk instead of embedded assets.
    pub frontend_dir: Option<PathBuf>,
}

/// Run the API server (API + WebSocket only, no frontend). Blocks until shutdown.
pub async fn run_api(config: ApiServiceConfig) -> Result<()> {
    let ckb_data_path = require_ckb_data_path(config.ckb_data_path.as_deref(), "api fail-fast")?;
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

    let app_config = AppConfig {
        store,
        append_only_store,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        rate_limit_per_second: Some(config.rate_limit),
        rate_limit_burst: Some(config.rate_limit_burst),
        start_background_tasks: true,
        ckb_data_path: Some(ckb_data_path.to_string()),
    };
    let app = create_router(app_config).await;

    let addr = format!("{}:{}", config.host, config.port);
    info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

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
    ckb_network: String,
    ckb_rpc_url: String,
}

pub fn build_frontend_router(config: FrontendServiceConfig) -> Result<Router> {
    let runtime_config = FrontendRuntimeConfig {
        api_port: config.api_port,
        ckb_network: config.ckb_network.clone(),
        ckb_rpc_url: config.ckb_rpc_url.clone(),
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
    let body = format!(
        r#"(() => {{
  const protocol = window.location.protocol === 'https:' ? 'https:' : 'http:';
  const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const hostname = window.location.hostname || '127.0.0.1';
  window.__CKBADGER_RUNTIME_CONFIG__ = {{
    apiBase: `${{protocol}}//${{hostname}}:{api_port}/api/v1`,
    wsUrl: `${{wsProtocol}}//${{hostname}}:{api_port}/ws`,
    ckbNetwork: {network},
    ckbRpcUrl: {rpc_url},
  }};
}})();
"#,
        api_port = config.api_port,
        network = network,
        rpc_url = rpc_url,
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

async fn frontend_filesystem_handler(State(state): State<FrontendFsState>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() {
        return serve_frontend_file(&state.index_path, "").await;
    }

    let candidate = state.root_dir.join(path);
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
    match path.rsplit_once('/') {
        Some((_, segment)) => segment.contains('.'),
        None => path.contains('.'),
    }
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
            ckb_data_path: None,
            store_runtime_config: StoreRuntimeConfig::default(),
        };

        assert_eq!(config.domain_data_path, "/data/domain");
        assert_eq!(config.append_only_data_path, "/data/append");
        assert_eq!(config.ckb_rpc_url, "http://localhost:8114");
        assert_eq!(config.ckb_network, "mainnet");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3001);
        assert_eq!(config.rate_limit, 100);
        assert_eq!(config.rate_limit_burst, 200);
        assert!(config.ckb_data_path.is_none());
        assert_eq!(config.store_runtime_config, StoreRuntimeConfig::default());
    }

    #[test]
    fn test_require_ckb_data_path_rejects_missing() {
        let err = require_ckb_data_path(None, "api test").unwrap_err();
        assert!(err.to_string().contains("[ckb].data_path is required"));
    }

    #[test]
    fn test_require_ckb_data_path_rejects_blank() {
        let err = require_ckb_data_path(Some("   "), "api test").unwrap_err();
        assert!(err.to_string().contains("[ckb].data_path is required"));
    }

    #[test]
    fn test_require_ckb_data_path_accepts_trimmed_value() {
        let path = require_ckb_data_path(Some(" /var/lib/ckb/data/db "), "api test").unwrap();
        assert_eq!(path, "/var/lib/ckb/data/db");
    }

    #[test]
    fn test_frontend_service_config_fields() {
        let config = FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            api_port: 8101,
            ckb_network: "mainnet".to_string(),
            ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
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
        assert!(text.contains(":9101/api/v1"));
        assert!(text.contains("\"testnet\""));
        assert!(text.contains("\"http://127.0.0.1:18114\""));
    }
}
