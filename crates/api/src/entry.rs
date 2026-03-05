use anyhow::{bail, Result};
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use ckbadger_store::CkbadgerStore;

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
pub struct FrontendServiceConfig {
    pub host: String,
    pub port: u16,
    /// Local filesystem override directory (e.g. workdir/frontend/).
    /// When set, serves from disk instead of embedded assets.
    pub frontend_dir: Option<PathBuf>,
}

/// Run the API server (API + WebSocket only, no frontend). Blocks until shutdown.
pub async fn run_api(config: ApiServiceConfig) -> Result<()> {
    let secondary_path = format!("{}-api-secondary", config.domain_data_path);
    info!(
        "Opening ckbadger domain store (secondary) at: {} -> {}",
        config.domain_data_path, secondary_path
    );
    let store = Arc::new(CkbadgerStore::open_domain_secondary(
        &config.domain_data_path,
        &secondary_path,
    )?);

    let append_only_secondary_path = format!("{}-api-secondary", config.append_only_data_path);
    info!(
        "Opening ckbadger append-only store (secondary) at: {} -> {}",
        config.append_only_data_path, append_only_secondary_path
    );
    let append_only_store = Arc::new(CkbadgerStore::open_append_only_secondary(
        &config.append_only_data_path,
        &append_only_secondary_path,
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
    let app = if let Some(ref frontend_dir) = config.frontend_dir {
        use tower_http::services::{ServeDir, ServeFile};

        let index_path = frontend_dir.join("index.html");
        let serve_dir = ServeDir::new(frontend_dir).fallback(ServeFile::new(index_path));
        info!(
            "Frontend server: serving from filesystem at {}",
            frontend_dir.display()
        );
        Router::new().fallback_service(serve_dir)
    } else if embedded_frontend::has_embedded_assets() {
        info!("Frontend server: serving embedded assets");
        Router::new().fallback(embedded_frontend::embedded_frontend_handler)
    } else {
        bail!(
            "No frontend assets available. Either:\n  \
             - Build the frontend: cd frontend && pnpm build\n  \
             - Then rebuild the binary: cargo build -p ckbadger\n  \
             - Or place assets in workdir/frontend/"
        );
    };

    let addr = format!("{}:{}", config.host, config.port);
    info!("Starting frontend server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
