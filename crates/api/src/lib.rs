pub mod cache;
pub mod cycles;
pub mod db;
pub mod middleware;
pub mod response;
pub mod routes;
pub mod tx_block_map;
pub mod utils;
pub mod warmup;
pub mod ws;

use axum::{routing::get, Router};
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use cache::CacheBackend;
use ckb_store_reader::CkbChainReader;
use cycles::CyclesCalculator;
use middleware::IpRateLimitLayer;
use ws::WsManager;

/// Embedded database migrator for use with `#[sqlx::test]`
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// Read-optimized pool, backed by a read replica when configured.
    /// Falls back to `pool` (primary) when no replica URL is provided.
    pub read_pool: PgPool,
    pub ws_manager: Arc<WsManager>,
    pub cache: CacheBackend,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub cycles_calculator: Arc<CyclesCalculator>,
    /// Direct read-only access to CKB node's RocksDB (when configured).
    pub ckb_store: Option<Arc<CkbChainReader>>,
}

pub struct AppConfig {
    pub pool: PgPool,
    /// Optional read replica pool. When `None`, all reads use the primary `pool`.
    pub read_pool: Option<PgPool>,
    pub redis_url: Option<String>,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub rate_limit_per_second: Option<u32>,
    pub rate_limit_burst: Option<u32>,
    pub start_background_tasks: bool,
    /// Path to CKB node's RocksDB data directory for direct reads.
    pub ckb_data_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            pool: PgPool::connect_lazy("postgres://localhost/ckbadger")
                .expect("Failed to create lazy connection pool for default config"),
            read_pool: None,
            redis_url: None,
            ckb_rpc_url: "http://localhost:8114".to_string(),
            ckb_network: "mainnet".to_string(),
            rate_limit_per_second: Some(100),
            rate_limit_burst: Some(200),
            start_background_tasks: true,
            ckb_data_path: None,
        }
    }
}

pub async fn create_router(config: AppConfig) -> Router {
    let ws_manager = Arc::new(WsManager::new());

    let cache = match config.redis_url {
        #[cfg(feature = "redis-cache")]
        Some(ref url) => match cache::RedisCache::new(url).await {
            Ok(redis) => {
                tracing::info!("Redis cache connected");
                CacheBackend::Redis(Box::new(redis))
            }
            Err(e) => {
                tracing::warn!("Failed to connect to Redis: {}, running without cache", e);
                CacheBackend::None
            }
        },
        #[cfg(not(feature = "redis-cache"))]
        Some(_) => {
            tracing::warn!("Redis URL provided but redis-cache feature not enabled");
            CacheBackend::None
        }
        None => {
            tracing::info!("No Redis URL configured, running without cache");
            CacheBackend::None
        }
    };

    let broadcaster_pool = config.pool.clone();
    let broadcaster_rpc_url = config.ckb_rpc_url.clone();
    let broadcaster_cache = cache.clone();

    let cycles_calculator = CyclesCalculator::new(config.pool.clone(), config.ckb_rpc_url.clone());

    let read_pool = config.read_pool.unwrap_or_else(|| config.pool.clone());

    let ckb_store = match config.ckb_data_path.as_deref() {
        Some(path) => {
            let reader = CkbChainReader::open(path)
                .expect("Failed to open CKB RocksDB — check CKB_DATA_PATH");
            tracing::info!("CKB direct RocksDB reader opened at {}", path);
            Some(Arc::new(reader))
        }
        None => None,
    };

    let state = Arc::new(AppState {
        pool: config.pool,
        read_pool,
        ws_manager,
        cache,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        cycles_calculator,
        ckb_store,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let rate_limit_per_second = config.rate_limit_per_second.unwrap_or(100);
    let rate_limit_burst = config.rate_limit_burst.unwrap_or(200);
    let rate_limit_layer = IpRateLimitLayer::new(rate_limit_per_second, rate_limit_burst);

    tracing::info!(
        "Rate limiting enabled: {} req/s, burst: {}",
        rate_limit_per_second,
        rate_limit_burst
    );

    if config.start_background_tasks {
        let warmup_state = state.clone();
        tokio::spawn(async move {
            warmup::warmup_chart_caches(warmup_state).await;
        });

        let broadcaster_ws = state.ws_manager.clone();
        tokio::spawn(async move {
            ws::start_block_broadcaster(
                broadcaster_pool,
                broadcaster_ws,
                broadcaster_rpc_url,
                broadcaster_cache,
            )
            .await;
        });

        let reorg_broadcaster_pool = state.pool.clone();
        let reorg_broadcaster_ws = state.ws_manager.clone();
        tokio::spawn(async move {
            ws::start_reorg_broadcaster(reorg_broadcaster_pool, reorg_broadcaster_ws).await;
        });
    }

    Router::new()
        .nest("/api/v1", routes::api_routes())
        .route("/ws", get(ws::ws_handler))
        .layer(rate_limit_layer)
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_config_default_has_no_read_pool() {
        let config = AppConfig::default();
        assert!(config.read_pool.is_none());
    }

    #[tokio::test]
    async fn test_app_config_default_values() {
        let config = AppConfig::default();
        assert_eq!(config.ckb_rpc_url, "http://localhost:8114");
        assert_eq!(config.ckb_network, "mainnet");
        assert_eq!(config.rate_limit_per_second, Some(100));
        assert_eq!(config.rate_limit_burst, Some(200));
        assert!(config.start_background_tasks);
        assert!(config.redis_url.is_none());
    }
}
