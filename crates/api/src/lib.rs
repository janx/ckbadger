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
use ckbadger_common::{
    BackgroundTaskEntry, BackgroundTaskKind, BackgroundTaskState, BackgroundTasksData,
};
use ckbadger_store::CkbadgerStore;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use cache::{CacheBackend, InMemoryCache};
use ckb_store_reader::CkbChainReader;
use cycles::CyclesClient;
use middleware::IpRateLimitLayer;
use response::{ApiError, ApiRouteError};
use ws::WsManager;

#[derive(Debug)]
pub struct CleanupPathGuard {
    path: PathBuf,
}

impl CleanupPathGuard {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl Drop for CleanupPathGuard {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to remove temporary directory {}: {}",
                    self.path.display(),
                    error
                );
            }
        }
    }
}

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
    /// Optional guard that keeps a temporary CKB RocksDB fixture alive for the router lifetime.
    pub ckb_db_cleanup: Option<Arc<CleanupPathGuard>>,
    /// In-memory cache for assets/tokens/NFT data (refreshed by background loop).
    pub mem_cache: InMemoryCache,
    /// Last asset cache warmup failure. `None` means warmup is still pending or last refresh succeeded.
    pub asset_cache_warmup_error: Arc<RwLock<Option<String>>>,
    /// Background task status for observability (API-side tasks only).
    pub background_tasks: Arc<RwLock<BackgroundTasksData>>,
    /// Root directory for content-addressed media blobs (decoded DOB outputs).
    pub dob_decode_dir: PathBuf,
}

impl AppState {
    pub fn record_asset_cache_warmup_error(&self, message: impl Into<String>) {
        *self
            .asset_cache_warmup_error
            .write()
            .expect("asset cache warmup error lock poisoned") = Some(message.into());
    }

    pub fn clear_asset_cache_warmup_error(&self) {
        *self
            .asset_cache_warmup_error
            .write()
            .expect("asset cache warmup error lock poisoned") = None;
    }

    /// Update a single API-side background task by name, inserting if absent.
    pub fn update_background_task(
        &self,
        task_name: &str,
        f: impl FnOnce(&mut BackgroundTaskEntry),
    ) {
        let mut data = self
            .background_tasks
            .write()
            .expect("background tasks lock poisoned");
        let entry = match data.tasks.iter_mut().find(|t| t.name == task_name) {
            Some(existing) => existing,
            None => {
                data.tasks.push(BackgroundTaskEntry {
                    name: task_name.to_string(),
                    kind: BackgroundTaskKind::Job,
                    state: BackgroundTaskState::Waiting,
                    message: None,
                    progress_current: None,
                    progress_total: None,
                    rate: None,
                    eta_seconds: None,
                    started_at: None,
                    elapsed_ms: None,
                    last_success_at: None,
                    last_trigger_reason: None,
                    error: None,
                });
                data.tasks.last_mut().unwrap()
            }
        };
        f(entry);
        data.updated_at = chrono::Utc::now().timestamp();
    }

    pub fn asset_cache_unavailable(&self, pending_message: &'static str) -> ApiRouteError {
        let warmup_error = self
            .asset_cache_warmup_error
            .read()
            .expect("asset cache warmup error lock poisoned")
            .clone();

        if let Some(message) = warmup_error {
            ApiError::internal(format!("asset cache warmup failed: {message}"))
        } else {
            ApiError::warmup_pending(pending_message)
        }
    }
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
    /// Optional guard that keeps a temporary CKB RocksDB fixture alive for the router lifetime.
    pub ckb_db_cleanup: Option<Arc<CleanupPathGuard>>,
    /// Root directory for content-addressed media blobs.
    pub dob_decode_dir: PathBuf,
}

pub async fn create_router(config: AppConfig) -> Router {
    let ws_manager = Arc::new(WsManager::new());

    let cache = CacheBackend::new();
    tracing::info!("In-memory cache initialized");

    let broadcaster_store = config.store.clone();
    let broadcaster_ao_store = config.append_only_store.clone();
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
        ckb_db_cleanup: config.ckb_db_cleanup,
        mem_cache,
        asset_cache_warmup_error: Arc::new(RwLock::new(None)),
        background_tasks: Arc::new(RwLock::new(BackgroundTasksData::default())),
        dob_decode_dir: config.dob_decode_dir,
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
                broadcaster_ao_store,
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
