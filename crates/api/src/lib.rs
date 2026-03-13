pub mod cache;
pub mod cycles;
pub mod embedded_frontend;
pub mod entry;
pub mod middleware;
pub mod response;
pub mod routes;
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
    pub append_only_store: Arc<CkbadgerStore>,
    pub ws_manager: Arc<WsManager>,
    pub cache: CacheBackend,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub cycles_client: Arc<CyclesClient>,
    /// Direct read-only access to the resolved CKB RocksDB path.
    pub ckb_store: Option<Arc<CkbChainReader>>,
    /// In-memory cache for assets/tokens/NFT data (refreshed by background loop).
    pub mem_cache: InMemoryCache,
}

pub struct AppConfig {
    pub store: Arc<CkbadgerStore>,
    pub append_only_store: Arc<CkbadgerStore>,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub rate_limit_per_second: Option<u32>,
    pub rate_limit_burst: Option<u32>,
    pub start_background_tasks: bool,
    /// Resolved path to the CKB node RocksDB directory for direct reads.
    pub ckb_db_path: String,
}

pub async fn create_router(config: AppConfig) -> Router {
    let ws_manager = Arc::new(WsManager::new());

    let cache = CacheBackend::new();
    tracing::info!("In-memory cache initialized");

    let broadcaster_store = config.store.clone();
    let broadcaster_rpc_url = config.ckb_rpc_url.clone();

    let cycles_client = CyclesClient::disabled();

    let ckb_store = match CkbChainReader::open(&config.ckb_db_path) {
        Ok(reader) => {
            tracing::info!("CKB direct RocksDB reader opened at {}", config.ckb_db_path);
            Some(Arc::new(reader))
        }
        Err(e) => {
            tracing::warn!(
                "CKB direct RocksDB reader unavailable ({}); spore decode will use RPC fallback",
                e
            );
            None
        }
    };

    let mem_cache = InMemoryCache::new();

    let state = Arc::new(AppState {
        store: config.store,
        append_only_store: config.append_only_store,
        ws_manager,
        cache,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        cycles_client,
        ckb_store,
        mem_cache,
    });

    if let Err(e) = warmup::warmup_assets_cache_once(state.clone()).await {
        tracing::warn!("Initial assets cache warmup failed: {}", e);
    }

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
        let broadcaster_ckb_store = state.ckb_store.clone();
        let broadcaster_network = state.ckb_network.clone();
        tokio::spawn(async move {
            ws::start_block_broadcaster(
                broadcaster_store,
                broadcaster_ws,
                broadcaster_rpc_url,
                broadcaster_network,
                broadcaster_ckb_store,
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

    // Spawn periodic store refresh for secondary instances.
    let refresh_store = state.store.clone();
    let refresh_append_only_store = state.append_only_store.clone();
    let refresh_ckb_store = state.ckb_store.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let store = refresh_store.clone();
            let append_only = refresh_append_only_store.clone();
            let ckb = refresh_ckb_store.clone();
            let result = tokio::task::spawn_blocking(move || {
                if let Err(e) = store.refresh() {
                    tracing::warn!("Store refresh failed: {}", e);
                }
                if let Err(e) = append_only.refresh() {
                    tracing::warn!("Append-only store refresh failed: {}", e);
                }
                if let Some(ref ckb_store) = ckb {
                    if let Err(e) = ckb_store.refresh() {
                        tracing::warn!("CKB store refresh failed: {}", e);
                    }
                }
            })
            .await;
            if let Err(e) = result {
                tracing::warn!("Store refresh task panicked: {}", e);
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
    use super::*;

    #[test]
    fn test_api_routes_are_nested_under_v1() {
        // Verify the module-level routes() function returns a valid router.
        // This exercises the route merging logic in routes::api_routes().
        let _router: Router<Arc<AppState>> = routes::api_routes();
    }
}
