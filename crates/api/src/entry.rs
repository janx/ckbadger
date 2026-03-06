use anyhow::{bail, Result};
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

use ckbadger_store::{secondary_store_path, CkbadgerStore, SecondaryStoreOwner};

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
    pub ckb_data_path: Option<String>,
}

/// Configuration for the standalone frontend server.
#[derive(Clone)]
pub struct FrontendServiceConfig {
    pub host: String,
    pub port: u16,
    /// Local filesystem override directory (e.g. workdir/frontend/).
    /// When set, serves from disk instead of embedded assets.
    pub frontend_dir: Option<PathBuf>,
}

/// Run the API server (API + WebSocket only, no frontend). Blocks until shutdown.
pub async fn run_api(config: ApiServiceConfig) -> Result<()> {
    let secondary_path = secondary_store_path(&config.domain_data_path, SecondaryStoreOwner::Api);
    info!(
        "Opening ckbadger domain store (secondary) at: {} -> {}",
        config.domain_data_path,
        secondary_path.display()
    );
    let store = Arc::new(CkbadgerStore::open_domain_secondary(
        Path::new(&config.domain_data_path),
        secondary_path.as_path(),
    )?);

    let append_only_secondary_path =
        secondary_store_path(&config.append_only_data_path, SecondaryStoreOwner::Api);
    info!(
        "Opening ckbadger append-only store (secondary) at: {} -> {}",
        config.append_only_data_path,
        append_only_secondary_path.display()
    );
    let append_only_store = Arc::new(CkbadgerStore::open_append_only_secondary(
        Path::new(&config.append_only_data_path),
        append_only_secondary_path.as_path(),
    )?);

    let app_config = AppConfig {
        store,
        append_only_store,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        rate_limit_per_second: Some(config.rate_limit),
        rate_limit_burst: Some(config.rate_limit_burst),
        start_background_tasks: true,
        ckb_data_path: config.ckb_data_path,
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

pub fn build_frontend_router(config: FrontendServiceConfig) -> Result<Router> {
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

        return Ok(Router::new().fallback({
            let state = state.clone();
            move |uri| frontend_filesystem_handler(State(state.clone()), uri)
        }));
    }

    if embedded_frontend::has_embedded_assets() {
        info!("Frontend server: serving embedded assets");
        return Ok(Router::new().fallback(embedded_frontend::embedded_frontend_handler));
    }

    bail!(
        "No frontend assets available. Either:\n  \
         - Build the frontend: cd frontend && pnpm build\n  \
         - Then rebuild the binary: cargo build -p ckbadger\n  \
         - Or place assets in workdir/frontend/dist or workdir/frontend/"
    );
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
    }

    #[test]
    fn test_frontend_service_config_fields() {
        let config = FrontendServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 8100,
            frontend_dir: Some(PathBuf::from("/work/frontend")),
        };

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8100);
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
}
