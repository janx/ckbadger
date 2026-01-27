pub mod cache;
pub mod clickhouse;
pub mod cycles;
pub mod db;
pub mod middleware;
pub mod response;
pub mod routes;
pub mod utils;
pub mod warmup;
pub mod ws;

use axum::{routing::get, Router};
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use cache::CacheBackend;
use cycles::CyclesCalculator;
use middleware::IpRateLimitLayer;
use ws::WsManager;

/// Embedded database migrator for use with `#[sqlx::test]`
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub ws_manager: Arc<WsManager>,
    pub cache: CacheBackend,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub cycles_calculator: Arc<CyclesCalculator>,
}

pub struct AppConfig {
    pub pool: PgPool,
    pub redis_url: Option<String>,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub rate_limit_per_second: Option<u32>,
    pub rate_limit_burst: Option<u32>,
    pub start_background_tasks: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            pool: PgPool::connect_lazy("postgres://localhost/ckbadger").unwrap(),
            redis_url: None,
            ckb_rpc_url: "http://localhost:8114".to_string(),
            ckb_network: "mainnet".to_string(),
            rate_limit_per_second: Some(100),
            rate_limit_burst: Some(200),
            start_background_tasks: true,
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
                CacheBackend::Redis(redis)
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

    let cycles_calculator = CyclesCalculator::new(config.pool.clone(), config.ckb_rpc_url.clone());

    let state = Arc::new(AppState {
        pool: config.pool,
        ws_manager,
        cache,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        cycles_calculator,
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
            ws::start_block_broadcaster(broadcaster_pool, broadcaster_ws, broadcaster_rpc_url)
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
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
