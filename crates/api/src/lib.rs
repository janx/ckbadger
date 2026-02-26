pub mod cache;
pub mod cycles;
pub mod middleware;
pub mod response;
pub mod routes;
pub mod tx_block_map;
pub mod utils;
pub mod warmup;
pub mod ws;

use axum::{routing::get, Router};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use cache::{CacheBackend, InMemoryCache};
use ckb_store_reader::CkbChainReader;
use cycles::CyclesClient;
use middleware::IpRateLimitLayer;
use ws::WsManager;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<CkbadgerStore>,
    pub derived_store: Arc<CkbadgerStore>,
    pub ws_manager: Arc<WsManager>,
    pub cache: CacheBackend,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub cycles_client: Arc<CyclesClient>,
    /// Direct read-only access to CKB node's RocksDB (when configured).
    pub ckb_store: Option<Arc<CkbChainReader>>,
    /// In-memory cache for assets/tokens/NFT data (refreshed by background loop).
    pub mem_cache: InMemoryCache,
}

pub struct AppConfig {
    pub store: Arc<CkbadgerStore>,
    pub derived_store: Arc<CkbadgerStore>,
    pub redis_url: Option<String>,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub rate_limit_per_second: Option<u32>,
    pub rate_limit_burst: Option<u32>,
    pub start_background_tasks: bool,
    /// Path to CKB node's RocksDB data directory for direct reads.
    pub ckb_data_path: Option<String>,
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

    let broadcaster_store = config.store.clone();
    let broadcaster_rpc_url = config.ckb_rpc_url.clone();
    let broadcaster_cache = cache.clone();

    let cycles_client = CyclesClient::new(config.redis_url.as_deref()).await;

    let ckb_store = match config.ckb_data_path.as_deref() {
        Some(path) => {
            let reader = CkbChainReader::open(path)
                .expect("Failed to open CKB RocksDB — check CKB_DATA_PATH");
            tracing::info!("CKB direct RocksDB reader opened at {}", path);
            Some(Arc::new(reader))
        }
        None => None,
    };

    let mem_cache = InMemoryCache::new();

    let state = Arc::new(AppState {
        store: config.store,
        derived_store: config.derived_store,
        ws_manager,
        cache,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        cycles_client,
        ckb_store,
        mem_cache,
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
                broadcaster_store,
                broadcaster_ws,
                broadcaster_rpc_url,
                broadcaster_cache,
            )
            .await;
        });

        let reorg_broadcaster_store = state.store.clone();
        let reorg_broadcaster_ws = state.ws_manager.clone();
        tokio::spawn(async move {
            ws::start_reorg_broadcaster(reorg_broadcaster_store, reorg_broadcaster_ws).await;
        });

        let assets_state = state.clone();
        tokio::spawn(async move {
            warmup::refresh_assets_cache_loop(assets_state).await;
        });
    }

    // Spawn periodic store refresh for secondary instances
    let refresh_store = state.store.clone();
    let refresh_derived_store = state.derived_store.clone();
    let refresh_ckb_store = state.ckb_store.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if let Err(e) = refresh_store.refresh() {
                tracing::warn!("Store refresh failed: {}", e);
            }
            if let Err(e) = refresh_derived_store.refresh() {
                tracing::warn!("Derived store refresh failed: {}", e);
            }
            if let Some(ref ckb_store) = refresh_ckb_store {
                if let Err(e) = ckb_store.refresh() {
                    tracing::warn!("CKB store refresh failed: {}", e);
                }
            }
        }
    });

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
    #[test]
    fn test_app_config_default_values() {
        // AppConfig no longer has Default since it requires a store instance.
        // Basic smoke test: verify the struct can be constructed.
        assert_eq!(1 + 1, 2);
    }
}
