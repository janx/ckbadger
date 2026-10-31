mod asset_catalog;
pub mod cache;
pub mod consensus;
pub mod cycles;
pub mod embedded_frontend;
pub mod entry;
mod frontend_formats;
pub mod frontend_proxy;
pub mod middleware;
pub mod pool;
pub mod request_log;
pub mod response;
pub mod routes;
pub mod store_readiness;
pub mod utils;
pub mod warmup;
pub mod ws;

use anyhow::Context;
use arc_swap::{ArcSwap, ArcSwapOption};
use axum::{routing::get, Router};
use ckbadger_common::{
    BackgroundTaskEntry, BackgroundTaskKind, BackgroundTaskState, BackgroundTasksData,
};
use ckbadger_store::read_view::{self, ReadViewGuard};
use ckbadger_store::CkbadgerStore;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultOnFailure, TraceLayer};
use tracing::Level;

use cache::{CacheBackend, InMemoryCache};
use ckb_store_reader::CkbChainReader;
use consensus::{ConsensusCell, ConsensusParams};
use cycles::CyclesClient;
use middleware::IpRateLimitLayer;
use pool::PoolMirror;
use response::{ApiError, ApiRouteError};
use store_readiness::StoreReadiness;
use warmup::SporeCache;
use ws::WsManager;

pub use request_log::RequestWait;

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
    /// Hot-swappable read-only secondary of the network-crawler store. The slot
    /// starts empty when the crawler has not created its primary yet and is
    /// populated without restarting the API once that primary becomes available.
    pub network_store: Arc<ArcSwapOption<CkbadgerStore>>,
    /// Whether the network crawler is enabled in config (drives UI availability
    /// hints independently of whether a store snapshot exists yet).
    pub crawler_enabled: bool,
    pub ws_manager: Arc<WsManager>,
    pub cache: CacheBackend,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub script_roles: std::collections::HashMap<String, (String, String)>,
    /// The chain's consensus parameters (proposal window, block limits), read
    /// from the node's `get_consensus` on first use. Use [`AppState::consensus`].
    pub consensus: Arc<ConsensusCell>,
    pub cycles_client: Arc<CyclesClient>,
    /// Direct read-only access to the resolved CKB RocksDB path.
    pub ckb_store: Option<Arc<CkbChainReader>>,
    /// Optional guard that keeps a temporary CKB RocksDB fixture alive for the router lifetime.
    pub ckb_db_cleanup: Option<Arc<CleanupPathGuard>>,
    /// In-memory cache for assets/tokens/object data (refreshed by background loop).
    pub mem_cache: InMemoryCache,
    /// `/pipeline`'s verified cellbase payments, bounded, by paying block hash.
    pub pipeline_pays: Arc<routes::PipelinePaysCache>,
    /// `/addresses/{addr}/cells?class=dao&order=capacity`: each owner's
    /// ordered DAO subset, per canonical tip (BA-34).
    pub dao_capacity_memo: Arc<routes::DaoCapacityMemo>,
    pub activity_window: Arc<crate::cache::ActivityWindowCache>,
    /// Last asset cache warmup failure. `None` means warmup is still pending or last refresh succeeded.
    pub asset_cache_warmup_error: Arc<RwLock<Option<String>>>,
    /// Background task status for observability (API-side tasks only).
    pub background_tasks: Arc<RwLock<BackgroundTasksData>>,
    /// Root directory for content-addressed media blobs (decoded DOB outputs).
    pub dob_decode_dir: PathBuf,
    /// Typed spore cache with pre-computed indexes, replaced atomically by warmup.
    pub spore_cache: Arc<ArcSwap<Option<SporeCache>>>,
    /// Token asset cache, replaced atomically by warmup loop (no TTL expiry).
    pub token_cache: Arc<ArcSwap<Option<Vec<warmup::CachedAssetEntry>>>>,
    /// Object asset cache, replaced atomically by warmup loop (no TTL expiry).
    pub object_cache: Arc<ArcSwap<Option<Vec<warmup::CachedAssetEntry>>>>,
    /// In-memory mirror of the node's transaction pool. Memory only: it writes
    /// to no store, and no value it holds enters a balance, holder list or
    /// statistic. Disabled mirrors publish `enabled: false` rather than an
    /// empty pool.
    pub pool_mirror: Arc<PoolMirror>,
    /// Upper bound on mirrored pool transactions (`[api] pool_max_tracked_txs`).
    pub pool_max_tracked_txs: std::num::NonZeroUsize,
    /// `/dao/evidence` for the current canonical tip (API memory only; DAO-17).
    pub dao_evidence: Arc<cache::TipMemo<routes::DaoEvidenceMemo>>,
    /// The fork history of one canonical tip (`routes::ForkView`).
    pub fork_view: Arc<cache::TipMemo<routes::ForkView>>,
    /// The Fiber channels and network of one canonical tip (`routes::FiberView`).
    pub fiber_view: Arc<cache::TipMemo<routes::FiberView>>,
    /// Whether the secondaries have caught up enough to serve data (J7): set by
    /// the store refresh loop, checked by the one gate over the store routes.
    pub store_readiness: Arc<StoreReadiness>,
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

    /// Load the token asset cache snapshot. Returns None if warmup hasn't populated it yet.
    pub fn load_token_cache(&self) -> Option<Vec<warmup::CachedAssetEntry>> {
        let guard = self.token_cache.load();
        guard.as_ref().clone()
    }

    /// Load the object asset cache snapshot. Returns None if warmup hasn't populated it yet.
    pub fn load_object_cache(&self) -> Option<Vec<warmup::CachedAssetEntry>> {
        let guard = self.object_cache.load();
        guard.as_ref().clone()
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

    /// The chain's consensus parameters, the API's one source for the NC-Max
    /// proposal window and the block limits. Loaded once from the node's
    /// `get_consensus`; when the node cannot answer the request fails — no
    /// hardcoded value stands in (a testnet or devnet chain may differ).
    pub async fn consensus(&self) -> Result<&ConsensusParams, ApiRouteError> {
        self.consensus
            .get_or_load(&self.ckb_rpc_url)
            .await
            .map_err(|e| ApiError::internal(format!("consensus parameters unavailable: {e}")))
    }

    /// The genesis economic baseline (issuance / burnt / virtual-occupied),
    /// derived once at block 0 and persisted by the indexer. Read-only from the
    /// secondary domain store. Fails fast if the indexer has not derived it yet
    /// rather than silently substituting a hardcoded constant.
    ///
    /// "Not written yet" is a normal startup window — the API's store-exists gate
    /// opens as soon as the indexer creates the stores, before it fetches block 0
    /// and writes the baseline — so it reports 503 `initializing`, which the SPA
    /// retries behind its initializing banner. A failed *read* stays a 500: that
    /// one is genuinely broken.
    pub fn genesis_baseline(&self) -> Result<ckbadger_store::GenesisBaseline, ApiRouteError> {
        self.store
            .get_genesis_baseline()
            .map_err(|e| ApiError::internal(format!("read genesis baseline: {e}")))?
            .ok_or_else(|| {
                ApiError::initializing("genesis baseline not yet derived (indexer still starting?)")
            })
    }
}

pub struct AppConfig {
    pub store: Arc<CkbadgerStore>,
    pub append_only_store: Arc<CkbadgerStore>,
    /// Hot-swappable read-only secondary of the network-crawler store (opt-in).
    pub network_store: Arc<ArcSwapOption<CkbadgerStore>>,
    /// Whether the network crawler is enabled in config.
    pub crawler_enabled: bool,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    /// Effective workdir metadata overlay, if present (same source as indexer label import).
    pub metadata_path: Option<PathBuf>,
    /// Consensus parameters the caller already holds — test fixtures hand the
    /// API a chain's parameters this way. Production passes `None`: the API
    /// reads them from the node's `get_consensus` on first use.
    pub consensus: Option<ConsensusParams>,
    pub rate_limit_per_second: Option<u32>,
    pub rate_limit_burst: Option<u32>,
    pub slow_request_threshold_ms: u64,
    pub start_background_tasks: bool,
    /// Resolved path to the CKB node RocksDB directory for direct reads.
    pub ckb_db_path: String,
    /// Optional guard that keeps a temporary CKB RocksDB fixture alive for the router lifetime.
    pub ckb_db_cleanup: Option<Arc<CleanupPathGuard>>,
    /// Root directory for content-addressed media blobs.
    pub dob_decode_dir: PathBuf,
    /// Directory where API writes cycles calculation request files for the indexer worker.
    pub cycles_request_dir: Option<PathBuf>,
    /// Mirror the node's tx pool in process memory (`[api] pool_mirror_enabled`).
    pub pool_mirror_enabled: bool,
    /// Poll interval for that mirror (`[api] pool_poll_interval_ms`). Non-zero
    /// by type: the config parser rejects 0, so there is nothing to clamp.
    pub pool_poll_interval_ms: std::num::NonZeroU64,
    /// Tracking cap for that mirror (`[api] pool_max_tracked_txs`).
    pub pool_max_tracked_txs: std::num::NonZeroUsize,
    /// Test seam: advances the secondaries in place of the stores above, so a
    /// test can make catch-up fail on cue. Production passes `None`: the API
    /// advances the stores it was given ([`store_readiness::ProcessSecondaries`]).
    pub secondary_catch_up: Option<Arc<dyn store_readiness::SecondaryCatchUp>>,
}

/// Seed the asset/address/script caches at startup *only when no background
/// refresh loops will do it*.
///
/// The warmup performs full-store scans (every address, every token + holders,
/// spores/clusters) and can take minutes on a synced mainnet DB. It must never
/// block the API's HTTP listener bind.
///
/// - `defer == true` (production): `create_router` spawns the recurring refresh
///   loops, which seed every cache on their first iteration and keep them fresh,
///   reporting progress via the `api_cache_refresh` task. Running a one-shot
///   warmup here too would duplicate those full-store scans — in particular the
///   minutes-long asset build, which outlasts the loop interval and so cannot be
///   deduplicated by timing. So the bind path does no warmup work; the loops own
///   the single seeding path and the listener binds immediately.
/// - `defer == false` (tests / embedded use without loops): no loop will seed
///   the caches, so run the warmup synchronously before the first request is
///   served, surfacing progress via the `cache_warmup` task.
///
/// Either way nothing is warmed before the secondaries' first successful
/// catch-up (J7): a freshly opened secondary's column families can disagree,
/// and a warmup invariant would then fail on healthy data.
pub async fn dispatch_initial_warmup(state: Arc<AppState>, defer: bool) {
    state.store_readiness.caught_up_once().await;
    if defer {
        tracing::info!(
            "API accepting connections; asset caches warming in background via refresh loops \
             (asset/address/script endpoints return 503 warmup_pending until ready)"
        );
        return;
    }

    state.update_background_task("cache_warmup", |entry| {
        entry.kind = BackgroundTaskKind::Job;
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
        entry.message = Some("Warming up asset caches...".to_string());
    });
    if let Err(e) = warmup::warmup_assets_cache_once(state).await {
        tracing::warn!("Initial assets cache warmup failed: {}", e);
    }
}

/// Build the API router, failing fast on invalid metadata roles/slugs.
pub async fn try_create_router(config: AppConfig) -> anyhow::Result<Router> {
    try_create_app(config).await.map(|(_state, router)| router)
}

/// [`try_create_router`], also handing back the state the router serves, so a
/// caller (a test) can observe its caches and background tasks.
pub async fn try_create_app(config: AppConfig) -> anyhow::Result<(Arc<AppState>, Router)> {
    let script_roles = ckbadger_indexer::label_import::effective_script_roles(
        config.metadata_path.as_deref(),
        &config.ckb_network,
    )
    .context("failed to read effective script metadata roles")?;
    let ws_manager = Arc::new(WsManager::new());

    let cache = CacheBackend::new();
    tracing::info!("In-memory cache initialized");

    let cycles_client = CyclesClient::new(config.cycles_request_dir.clone());

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

    let pool_mirror = Arc::new(PoolMirror::new(config.pool_mirror_enabled));
    let pool_poll_interval = std::time::Duration::from_millis(config.pool_poll_interval_ms.get());

    let state = Arc::new(AppState {
        store: config.store,
        append_only_store: config.append_only_store,
        network_store: config.network_store,
        crawler_enabled: config.crawler_enabled,
        ws_manager,
        cache,
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        script_roles,
        consensus: Arc::new(match config.consensus {
            Some(params) => ConsensusCell::preloaded(params),
            None => ConsensusCell::new(),
        }),
        cycles_client,
        ckb_store,
        ckb_db_cleanup: config.ckb_db_cleanup,
        mem_cache,
        pipeline_pays: Arc::new(routes::PipelinePaysCache::new()),
        dao_capacity_memo: Arc::new(routes::DaoCapacityMemo::new()),
        activity_window: Default::default(),
        asset_cache_warmup_error: Arc::new(RwLock::new(None)),
        background_tasks: Arc::new(RwLock::new(BackgroundTasksData::default())),
        dob_decode_dir: config.dob_decode_dir,
        spore_cache: Arc::new(ArcSwap::from_pointee(None)),
        token_cache: Arc::new(ArcSwap::from_pointee(None)),
        object_cache: Arc::new(ArcSwap::from_pointee(None)),
        pool_mirror,
        pool_max_tracked_txs: config.pool_max_tracked_txs,
        dao_evidence: Arc::new(cache::TipMemo::new()),
        fork_view: Arc::new(cache::TipMemo::new()),
        fiber_view: Arc::new(cache::TipMemo::new()),
        store_readiness: Arc::new(StoreReadiness::new()),
    });

    // The refresh loop's first round is the initial catch-up: it runs at once
    // and repeats every second until it succeeds. Until then the store routes
    // answer 503 `store_catching_up` and nothing below reads a secondary.
    let secondaries: Arc<dyn store_readiness::SecondaryCatchUp> = match config.secondary_catch_up {
        Some(injected) => injected,
        None => Arc::new(store_readiness::ProcessSecondaries::of(&state)),
    };
    tokio::spawn(store_readiness::run_store_refresh_loop(
        state.store_readiness.clone(),
        secondaries,
    ));

    if config.start_background_tasks {
        tokio::spawn(start_background_tasks(state.clone(), pool_poll_interval));
    } else {
        dispatch_initial_warmup(state.clone(), false).await;
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

    let slow_threshold = std::time::Duration::from_millis(config.slow_request_threshold_ms);

    // The span every request's log lines carry; `request_log::log_request`
    // (just inside it) writes the one line per finished request.
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                path = request.uri().path(),
                query = request.uri().query().unwrap_or(""),
            )
        })
        .on_response(())
        .on_failure(DefaultOnFailure::new().level(Level::ERROR));

    let router = Router::new()
        .nest("/api/v1", api_v1_routes(&state))
        .route("/ws", get(ws::ws_handler))
        // Innermost layer: pin the read view around handler execution only,
        // after rate limiting has already rejected what it will reject. The
        // store-readiness gate is a route layer, so it runs inside this pin
        // and its verdict and the handler's reads share one view.
        .layer(axum::middleware::from_fn(pin_read_view))
        .layer(rate_limit_layer)
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(axum::middleware::from_fn_with_state(
            slow_threshold,
            request_log::log_request,
        ))
        .layer(trace_layer)
        .with_state(state.clone());
    Ok((state, router))
}

/// Production's background tasks: chart warmup, the asset/script/address
/// cache loops, the WebSocket broadcasters and the tx-pool mirror. Each reads
/// the secondaries, so none starts before their first successful catch-up.
async fn start_background_tasks(state: Arc<AppState>, pool_poll_interval: std::time::Duration) {
    dispatch_initial_warmup(state.clone(), true).await;

    tokio::spawn(warmup::warmup_chart_caches(state.clone()));
    tokio::spawn(ws::start_block_broadcaster(
        state.store.clone(),
        state.append_only_store.clone(),
        state.ws_manager.clone(),
        state.ckb_rpc_url.clone(),
        state.ckb_network.clone(),
    ));
    tokio::spawn(ws::start_reorg_broadcaster(
        state.store.clone(),
        state.ws_manager.clone(),
    ));
    tokio::spawn(warmup::refresh_assets_cache_loop(state.clone()));
    tokio::spawn(warmup::refresh_script_cache_loop(state.clone()));
    tokio::spawn(warmup::refresh_address_cache_loop(state.clone()));
    tokio::spawn(pool::refresh_pool_mirror_loop(state, pool_poll_interval));
}

/// Every `/api/v1` route, the ones that read a secondary behind the
/// store-readiness gate (503 `store_catching_up` until the stores have caught
/// up, see [`store_readiness`]). Node-RPC-only routes are not gated.
///
/// Public so test routers mount the production gate too.
pub fn api_v1_routes(state: &AppState) -> Router<Arc<AppState>> {
    routes::store_routes()
        .route_layer(axum::middleware::from_fn_with_state(
            state.store_readiness.clone(),
            store_readiness::require_caught_up_stores,
        ))
        .merge(routes::node_rpc_routes())
}

/// Unwrap wrapper for tests and callers that already assume valid metadata.
pub async fn create_router(config: AppConfig) -> Router {
    try_create_router(config)
        .await
        .expect("failed to create API router")
}

/// This request's pin on the process-wide read view.
///
/// The API reads RocksDB secondaries, which cannot take snapshots and whose
/// view advances only on catch-up. Pinning it for the whole request is what
/// makes a response coherent: index row and the entry it points at, domain
/// marker and append-only payload, every read in the handler — all one view.
/// Without it a catch-up landing mid-request tears them apart (see
/// `ckbadger_store::read_view`).
///
/// Handlers get it as `Extension<RequestReadView>` and only need it to opt out
/// via [`RequestReadView::release`].
#[derive(Clone)]
pub struct RequestReadView(Arc<std::sync::Mutex<Option<ReadViewGuard>>>);

impl RequestReadView {
    fn pin() -> Self {
        Self(Arc::new(std::sync::Mutex::new(Some(
            read_view::acquire_read(),
        ))))
    }

    /// Stop pinning the view for the rest of this request.
    ///
    /// Only for handlers whose contract is to observe the *next* view — the
    /// cycles long-poll waits for the indexer to write a result, and
    /// `/pipeline` waits for the store to reach the node's pool tip, so both
    /// must let catch-up run. Everything read after this call may come from a
    /// newer view than what was read before it; a handler that still needs
    /// one coherent view pins its own (`/pipeline` pins one per attempt).
    /// Such a handler sleeps through [`RequestWait`], so the request log does
    /// not count its wait as slowness.
    pub fn release(&self) {
        let _ = self.0.lock().expect("request read view poisoned").take();
    }
}

/// Pin one read view per request, released when the response is produced.
///
/// Public so test routers can mount the production layer stack:
/// `.layer(axum::middleware::from_fn(pin_read_view))`.
pub async fn pin_read_view(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let view = RequestReadView::pin();
    request.extensions_mut().insert(view.clone());
    let response = next.run(request).await;
    drop(view);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read view is process-wide, so these two must not overlap.
    static READ_VIEW_TEST_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn pinned_scopes_probe() -> String {
        read_view::pinned_read_scopes().to_string()
    }

    async fn releasing_probe(view: Option<axum::Extension<RequestReadView>>) -> String {
        let before = read_view::pinned_read_scopes();
        if let Some(axum::Extension(view)) = view {
            view.release();
        }
        format!("{before},{}", read_view::pinned_read_scopes())
    }

    #[tokio::test]
    async fn test_request_pins_one_read_view_for_the_handler_lifetime() {
        use axum::body::Body;
        use axum::extract::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let _serial = READ_VIEW_TEST_SERIAL.lock().await;

        let app = Router::new()
            .route("/probe", get(pinned_scopes_probe))
            .layer(axum::middleware::from_fn(pin_read_view));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(
            &body[..],
            b"1",
            "handler must run inside exactly one pinned read view"
        );
        assert_eq!(
            read_view::pinned_read_scopes(),
            0,
            "the pin must be released once the response is produced"
        );
    }

    #[tokio::test]
    async fn test_handler_can_release_its_read_view_to_await_new_data() {
        use axum::body::Body;
        use axum::extract::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let _serial = READ_VIEW_TEST_SERIAL.lock().await;

        let app = Router::new()
            .route("/probe", get(releasing_probe))
            .layer(axum::middleware::from_fn(pin_read_view));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(
            &body[..],
            b"1,0",
            "release() must let catch-up proceed for the rest of the request"
        );
    }

    #[test]
    fn test_api_routes_are_nested_under_v1() {
        // Verify the module-level routes() function returns a valid router.
        // This exercises the route merging logic in routes::api_routes().
        let _router: Router<Arc<AppState>> = routes::api_routes();
    }
}
