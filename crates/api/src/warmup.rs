use crate::cache::CacheTtl;
use crate::response::ChartResponse;
use crate::routes::assets::{count_nft_collection_activities_cached, AssetResponse};
use crate::routes::statistics::{
    build_address_cohort_response, build_block_time_distribution_response, build_cell_size_response,
};
use crate::utils::{
    accumulate_owned_capacity, hash_type_to_string, resolve_collection_standard,
    resolve_dob_collection_name, resolve_object_collection_composition_tier_override,
    resolve_object_collection_name,
};
use crate::AppState;
use ckbadger_common::{BackgroundTaskKind, BackgroundTaskState};
use ckbadger_store::AddressBalance;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);

// Cache keys (token/object asset caches use ArcSwap in AppState, not mem_cache)
pub const CACHE_KEY_ADDRESSES_TOP: &str = "addresses:top";
pub const CACHE_KEY_ADDRESSES_ACTIVE: &str = "addresses:active";
pub const CACHE_KEY_SCRIPT_FAMILIES_ALL: &str = "scripts:families:all";
pub const CACHE_KEY_SCRIPTS_ALL: &str = "scripts:all";
pub const CACHE_KEY_SCRIPTS_NAMED: &str = "scripts:named";
pub const CACHE_KEY_SCRIPT_VERSIONS_ALL: &str = "scripts:versions:all";
const ADDRESS_CACHE_LIMIT: usize = 500;
const SPORE_CACHE_LIMIT: usize = 100_000;

/// Typed, pre-indexed spore cache. Built once at warmup, replaced atomically.
/// Eliminates per-request JSON deserialization of the full spore dataset.
///
/// `all` must follow the `(created_at_block DESC, id ASC)` total order: the
/// composite `{block}:{0x-id}` pagination cursor resumes by position in that
/// order, so it is only well-defined when the order is deterministic. Every
/// derived index below inherits the order by pushing indices in `all` order.
pub struct SporeCache {
    /// All entries (spores and cluster cells), (created_at_block DESC, id ASC).
    pub all: Vec<(Vec<u8>, ckbadger_store::ObjectEntry)>,
    /// Indexes into `all` for live non-cluster spores (preserves order).
    /// Cluster cells are collection-level entries served by the cluster
    /// endpoints; the spore objects list must never contain them (their
    /// `/spore/objects/{id}` detail is a 404 by design).
    pub live_indices: Vec<usize>,
    /// owner_lock_hash -> sorted indexes into `all` (live non-cluster spores
    /// only — same exclusion as `live_indices`, same 404 otherwise).
    pub by_owner: HashMap<Vec<u8>, Vec<usize>>,
    /// cluster_id -> sorted indexes into `all` (all non-cluster spores,
    /// preserves order).
    pub by_cluster: HashMap<Vec<u8>, Vec<usize>>,
    /// (index into `all`, lowercased name) for name-search.
    /// Non-cluster spores with a name only. Preserves order.
    pub name_index: Vec<(usize, String)>,
}

impl SporeCache {
    /// Build a SporeCache from a spore list pre-sorted by
    /// `(created_at_block DESC, id ASC)`.
    pub fn build(all: Vec<(Vec<u8>, ckbadger_store::ObjectEntry)>) -> Self {
        let mut live_indices = Vec::new();
        let mut by_owner: HashMap<Vec<u8>, Vec<usize>> = HashMap::new();
        let mut by_cluster: HashMap<Vec<u8>, Vec<usize>> = HashMap::new();
        let mut name_index = Vec::new();

        for (i, (_id, entry)) in all.iter().enumerate() {
            let is_cluster = entry.standard.is_cluster();

            // Spore-serving indexes: non-cluster entries only (see field docs).
            if entry.is_live && !is_cluster {
                live_indices.push(i);
                if let Some(ref owner) = entry.owner_lock_hash {
                    by_owner.entry(owner.clone()).or_default().push(i);
                }
            }

            // Index all non-cluster spores by their collection_id
            if !is_cluster {
                if let Some(ref cluster_id) = entry.collection_id {
                    by_cluster.entry(cluster_id.clone()).or_default().push(i);
                }
                if let Some(ref name) = entry.name {
                    name_index.push((i, name.to_ascii_lowercase()));
                }
            }
        }

        SporeCache {
            all,
            live_indices,
            by_owner,
            by_cluster,
            name_index,
        }
    }
}

/// Cached asset entry with pre-computed metrics, ready for API serving.
#[derive(Clone, Serialize, Deserialize)]
pub struct CachedAssetEntry {
    pub id: String,
    pub asset_type: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub icon_url: Option<String>,
    pub holders_count: i64,
    pub transfers_count: i64,
    pub transfers_24h: i64,
    pub decimals: Option<i16>,
    pub total_supply: Option<String>,
    pub maximum_supply: Option<String>,
    pub content_type: Option<String>,
    pub content_size: Option<i32>,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
    pub owned_capacity: Option<String>,
    pub owned_knowledge: Option<String>,
    pub composition_tier: Option<String>,
    pub onchain_ratio: Option<String>,
    pub onchain_count: Option<i64>,
    // Token-specific fields (None for object entries)
    pub type_code_hash: Option<String>,
    pub type_hash_type: Option<String>,
    pub type_args: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedAddressEntry {
    pub lock_script_hash: String,
    pub balance: String,
    pub live_cells_count: i32,
    pub transactions_count: i64,
    pub last_activity_block: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedScriptEntry {
    pub code_hash: String,
    pub name: String,
}

#[derive(Clone, Eq, PartialEq)]
struct AddressCandidate {
    lock_hash: Vec<u8>,
    balance: i128,
    live_cells_count: i32,
    transactions_count: i64,
    last_activity_block: i64,
}

#[derive(Clone, Eq, PartialEq)]
struct BalanceRank(AddressCandidate);

impl Ord for BalanceRank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .balance
            .cmp(&other.0.balance)
            .then_with(|| self.0.lock_hash.cmp(&other.0.lock_hash))
    }
}

impl PartialOrd for BalanceRank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Eq, PartialEq)]
struct ActivityRank(AddressCandidate);

impl Ord for ActivityRank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .last_activity_block
            .cmp(&other.0.last_activity_block)
            .then_with(|| self.0.lock_hash.cmp(&other.0.lock_hash))
    }
}

impl PartialOrd for ActivityRank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl CachedAssetEntry {
    pub fn to_asset_response(&self) -> AssetResponse {
        AssetResponse {
            id: self.id.clone(),
            asset_type: self.asset_type.clone(),
            standard: self.standard.clone(),
            name: self.name.clone(),
            symbol: self.symbol.clone(),
            icon_url: self.icon_url.clone(),
            published: false,
            famous: false,
            tags: None,
            holders_count: self.holders_count,
            transfers_count: self.transfers_count,
            transfers_24h: self.transfers_24h,
            decimals: self.decimals,
            total_supply: self.total_supply.clone(),
            content_type: self.content_type.clone(),
            content_size: self.content_size,
            cluster_id: self.cluster_id.clone(),
            cluster_name: self.cluster_name.clone(),
            owned_capacity: self.owned_capacity.clone(),
            owned_knowledge: self.owned_knowledge.clone(),
            composition_tier: self.composition_tier.clone(),
            onchain_ratio: self.onchain_ratio.clone(),
            onchain_count: self.onchain_count,
            h_multiplier: {
                match (&self.owned_capacity, &self.owned_knowledge) {
                    (Some(cap_str), Some(occ_str)) => {
                        let cap: f64 = cap_str.parse().unwrap_or(0.0);
                        let occ: f64 = occ_str.parse().unwrap_or(0.0);
                        if occ > 0.0 {
                            Some(((cap / occ) * 100.0).round() / 100.0)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            },
        }
    }
}

fn format_ratio_4(numerator: i64, denominator: i64) -> String {
    if denominator <= 0 {
        return "0.0000".to_string();
    }
    let scaled = numerator
        .saturating_mul(10_000)
        .checked_div(denominator)
        .unwrap_or(0);
    let whole = scaled / 10_000;
    let frac = (scaled % 10_000).abs();
    format!("{whole}.{frac:04}")
}

fn resolve_composition_tier(
    btc_ckb: i64,
    pure_ckb: i64,
    decentralized_mixture: i64,
    centralized_mixture: i64,
    unknown: i64,
) -> String {
    if centralized_mixture > 0 {
        return "centralized_mixture".to_string();
    }
    if decentralized_mixture > 0 {
        return "decentralized_mixture".to_string();
    }
    let total_onchain = btc_ckb + pure_ckb;
    if total_onchain > 0 && unknown == 0 {
        if btc_ckb > 0 {
            return "btc_ckb".to_string();
        }
        return "pure_ckb".to_string();
    }
    "unknown".to_string()
}

fn set_api_cache_refresh_startup(entry: &mut ckbadger_common::BackgroundTaskEntry) {
    entry.kind = BackgroundTaskKind::Watcher;
    entry.state = BackgroundTaskState::Waiting;
    entry.started_at = None;
    entry.message = Some("Waiting for first refresh".to_string());
    entry.last_trigger_reason = Some("startup".to_string());
    entry.error = None;
}

fn set_api_cache_refresh_cycle_start(entry: &mut ckbadger_common::BackgroundTaskEntry) {
    entry.kind = BackgroundTaskKind::Watcher;
    entry.state = BackgroundTaskState::Running;
    entry.started_at = Some(chrono::Utc::now().timestamp());
    entry.message = Some("Refreshing API caches".to_string());
    entry.progress_current = None;
    entry.progress_total = None;
    entry.rate = None;
    entry.eta_seconds = None;
    entry.elapsed_ms = None;
    entry.last_trigger_reason = Some("new_tip".to_string());
    entry.error = None;
}

fn set_api_cache_refresh_idle(entry: &mut ckbadger_common::BackgroundTaskEntry, reason: &str) {
    entry.kind = BackgroundTaskKind::Watcher;
    entry.state = BackgroundTaskState::Waiting;
    entry.message = Some("Idle".to_string());
    entry.last_trigger_reason = Some(reason.to_string());
    entry.error = None;
}

fn set_api_cache_refresh_success(
    entry: &mut ckbadger_common::BackgroundTaskEntry,
    elapsed_ms: f64,
    now_ts: i64,
) {
    set_api_cache_refresh_idle(entry, "new_tip");
    entry.elapsed_ms = Some(elapsed_ms);
    entry.last_success_at = Some(now_ts);
}

fn set_api_cache_refresh_failure(
    entry: &mut ckbadger_common::BackgroundTaskEntry,
    elapsed_ms: f64,
    error: String,
) {
    entry.kind = BackgroundTaskKind::Watcher;
    entry.state = BackgroundTaskState::Failed;
    entry.message = Some("Refresh failed".to_string());
    entry.last_trigger_reason = Some("new_tip".to_string());
    entry.elapsed_ms = Some(elapsed_ms);
    entry.error = Some(error);
}

/// Background loop that refreshes the assets cache every 30 seconds.
/// Skips the refresh cycle when the sync tip block number hasn't changed
/// since the last successful refresh, avoiding wasteful CF scans when idle.
pub async fn refresh_assets_cache_loop(state: Arc<AppState>) {
    let mut last_refreshed_tip: Option<i64> = None;

    state.update_background_task("api_cache_refresh", |entry| {
        set_api_cache_refresh_startup(entry);
    });

    loop {
        let current_tip = state
            .store
            .get_sync_status()
            .map(|s| s.tip_block_number)
            .ok();

        if current_tip.is_some() && current_tip == last_refreshed_tip {
            tracing::trace!(
                "Warmup: tip unchanged at {:?}, skipping refresh",
                current_tip
            );
            state.update_background_task("api_cache_refresh", |entry| {
                set_api_cache_refresh_idle(entry, "tip_unchanged");
            });
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }

        // The first build (no prior refreshed tip) is the initial warmup: the
        // caches are still cold and asset/address/script endpoints return 503
        // until it lands. Mark it distinctly so api/log/TUI show "warming up"
        // rather than a routine refresh.
        let is_initial_warmup = last_refreshed_tip.is_none();
        state.update_background_task("api_cache_refresh", |entry| {
            set_api_cache_refresh_cycle_start(entry);
            if is_initial_warmup {
                entry.message = Some("Initial cache warmup".to_string());
            }
        });
        if is_initial_warmup {
            tracing::info!(
                tip = ?current_tip,
                "Building API asset caches (initial warmup); \
                 asset/address/script endpoints return 503 until ready"
            );
        }

        let cycle_start = std::time::Instant::now();
        let state_clone = state.clone();
        let result =
            tokio::task::spawn_blocking(move || refresh_assets_cache_sync(&state_clone)).await;

        let cycle_elapsed_ms = cycle_start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(Ok(())) => {
                last_refreshed_tip = current_tip;
                tracing::debug!("Assets cache refreshed at tip {:?}", current_tip);
                state.update_background_task("api_cache_refresh", |entry| {
                    set_api_cache_refresh_success(
                        entry,
                        cycle_elapsed_ms,
                        chrono::Utc::now().timestamp(),
                    );
                });
            }
            Ok(Err(e)) => {
                tracing::warn!("Assets cache refresh failed: {}", e);
                state.update_background_task("api_cache_refresh", |entry| {
                    set_api_cache_refresh_failure(entry, cycle_elapsed_ms, e.to_string());
                });
            }
            Err(e) => {
                tracing::warn!("Assets cache refresh task panicked: {}", e);
                state.update_background_task("api_cache_refresh", |entry| {
                    set_api_cache_refresh_failure(
                        entry,
                        cycle_elapsed_ms,
                        format!("task panicked: {}", e),
                    );
                });
            }
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// Lightweight loop that keeps the script cache alive independently of the
/// heavy asset-build cycle.  The script cache only performs 4 fast CF reads
/// (families, infos, versions, named) so a 10-second interval is cheap.
///
/// This loop is decoupled from `refresh_assets_cache_loop` because
/// `build_asset_caches_sync` can take minutes (65 000+ token holder scans),
/// which would let the 45-second script-cache TTL expire before the next
/// refresh cycle even starts.
pub async fn refresh_script_cache_loop(state: Arc<AppState>) {
    // Seed the cache immediately on first iteration (no initial sleep).
    loop {
        let state_clone = state.clone();
        let result =
            tokio::task::spawn_blocking(move || refresh_named_script_cache_sync(&state_clone))
                .await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("Script cache refresh failed: {}", e);
            }
            Err(e) => {
                tracing::warn!("Script cache refresh task panicked: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

/// Independent loop that keeps the address cache alive regardless of the
/// heavy asset-build cycle.  The address cache performs a single CF scan
/// (addr_balance) so a 20-second interval is acceptable.
///
/// Decoupled from `refresh_assets_cache_loop` for the same reason as the
/// script cache: `build_asset_caches_sync` can take minutes, which would
/// let the 30-second address-cache TTL expire before the address refresh
/// even starts.
pub async fn refresh_address_cache_loop(state: Arc<AppState>) {
    loop {
        let state_clone = state.clone();
        let result =
            tokio::task::spawn_blocking(move || refresh_address_cache_sync(&state_clone)).await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("Address cache refresh failed: {}", e);
            }
            Err(e) => {
                tracing::warn!("Address cache refresh task panicked: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

fn push_bounded<T: Ord>(heap: &mut BinaryHeap<Reverse<T>>, item: T, limit: usize) {
    if heap.len() < limit {
        heap.push(Reverse(item));
        return;
    }
    let Some(mut smallest) = heap.peek_mut() else {
        heap.push(Reverse(item));
        return;
    };
    if item > smallest.0 {
        *smallest = Reverse(item);
    }
}

fn cached_address_entry_from_candidate(candidate: AddressCandidate) -> CachedAddressEntry {
    CachedAddressEntry {
        lock_script_hash: format!("0x{}", hex::encode(candidate.lock_hash)),
        balance: candidate.balance.to_string(),
        live_cells_count: candidate.live_cells_count,
        transactions_count: candidate.transactions_count,
        last_activity_block: candidate.last_activity_block,
    }
}

fn refresh_address_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let mut by_balance: BinaryHeap<Reverse<BalanceRank>> = BinaryHeap::new();
    let mut by_activity: BinaryHeap<Reverse<ActivityRank>> = BinaryHeap::new();

    let iter = state
        .store
        .iterator_cf(state.store.cf_addr_balance(), rocksdb::IteratorMode::Start);

    for item in iter {
        let (key, value) =
            item.map_err(|e| anyhow::anyhow!("failed to iterate addr_balance in warmup: {}", e))?;
        let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
            anyhow::anyhow!(
                "failed to deserialize address balance in warmup: lock_hash=0x{}, error={}",
                hex::encode(&key),
                e
            )
        })?;
        if balance.balance < 0 {
            anyhow::bail!(
                "negative balance detected in addr_balance warmup: lock_hash=0x{}, balance={}",
                hex::encode(&key),
                balance.balance
            );
        }
        if balance.live_cells_count < 0 {
            anyhow::bail!(
                "negative live_cells_count detected in addr_balance warmup: lock_hash=0x{}, live_cells_count={}",
                hex::encode(&key),
                balance.live_cells_count
            );
        }
        if balance.txs_count < 0 {
            anyhow::bail!(
                "negative txs_count detected in addr_balance warmup: lock_hash=0x{}, txs_count={}",
                hex::encode(&key),
                balance.txs_count
            );
        }

        let candidate = AddressCandidate {
            lock_hash: key.to_vec(),
            balance: balance.balance,
            live_cells_count: balance.live_cells_count,
            transactions_count: balance.txs_count,
            last_activity_block: balance.last_activity_block,
        };

        if candidate.balance > 0 {
            push_bounded(
                &mut by_balance,
                BalanceRank(candidate.clone()),
                ADDRESS_CACHE_LIMIT,
            );
        }
        push_bounded(
            &mut by_activity,
            ActivityRank(candidate),
            ADDRESS_CACHE_LIMIT,
        );
    }

    let mut top_entries: Vec<AddressCandidate> = by_balance.into_iter().map(|v| v.0 .0).collect();
    top_entries.sort_by(|a, b| {
        b.balance
            .cmp(&a.balance)
            .then_with(|| a.lock_hash.cmp(&b.lock_hash))
    });
    let top_cached: Vec<CachedAddressEntry> = top_entries
        .into_iter()
        .map(cached_address_entry_from_candidate)
        .collect();

    let mut active_entries: Vec<AddressCandidate> =
        by_activity.into_iter().map(|v| v.0 .0).collect();
    active_entries.sort_by(|a, b| {
        b.last_activity_block
            .cmp(&a.last_activity_block)
            .then_with(|| a.lock_hash.cmp(&b.lock_hash))
    });
    let active_cached: Vec<CachedAddressEntry> = active_entries
        .into_iter()
        .map(cached_address_entry_from_candidate)
        .collect();

    state.mem_cache.set(
        CACHE_KEY_ADDRESSES_TOP,
        &top_cached,
        CacheTtl::ADDRESS_BALANCE,
    );
    state.mem_cache.set(
        CACHE_KEY_ADDRESSES_ACTIVE,
        &active_cached,
        CacheTtl::ADDRESS_BALANCE,
    );

    Ok(())
}

fn refresh_spore_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let mut spores = state.store.list_spores(SPORE_CACHE_LIMIT)?;
    // Explicit (created_at_block DESC, id ASC) total order — the composite
    // pagination cursor resumes by position in this order, so ties must break
    // deterministically rather than depending on CF iteration order.
    spores.sort_by(|a, b| {
        b.1.created_at_block
            .cmp(&a.1.created_at_block)
            .then_with(|| a.0.cmp(&b.0))
    });
    let cache = SporeCache::build(spores);
    state.spore_cache.store(Arc::new(Some(cache)));

    warmup_cluster_daily_deltas_sync(state);
    warmup_cluster_activity_counts_sync(state);

    Ok(())
}

fn warmup_cluster_daily_deltas_sync(state: &AppState) {
    // Get all cluster IDs from cluster_agg CF (small CF, fast scan)
    let clusters = match state.store.list_cluster_aggregates() {
        Ok(clusters) => clusters,
        Err(e) => {
            tracing::warn!("Failed to list cluster aggregates for warmup: {}", e);
            return;
        }
    };

    // Scan daily deltas for each cluster to warm the block cache.
    // We don't need the results — just iterating warms the RocksDB block cache.
    let mut warmed = 0usize;
    for (cluster_id, _agg) in &clusters {
        match state.store.list_cluster_daily_deltas(cluster_id) {
            Ok(_) => warmed += 1,
            Err(e) => {
                tracing::warn!(
                    "Failed to warm cluster daily deltas for cluster 0x{}: {}",
                    hex::encode(cluster_id),
                    e
                );
            }
        }
    }

    info!(
        clusters_warmed = warmed,
        clusters_total = clusters.len(),
        "Warmed cluster daily deltas block cache"
    );
}

fn warmup_cluster_activity_counts_sync(state: &AppState) {
    let clusters = match state.store.list_cluster_aggregates() {
        Ok(clusters) => clusters,
        Err(e) => {
            tracing::warn!(
                "Failed to list cluster aggregates for activity count warmup: {}",
                e
            );
            return;
        }
    };

    let mut warmed = 0usize;
    for (cluster_id, _agg) in &clusters {
        match count_nft_collection_activities_cached(
            state.store.as_ref(),
            state.store.as_ref(),
            &state.mem_cache,
            cluster_id,
        ) {
            Ok(_) => warmed += 1,
            Err(e) => {
                tracing::warn!(
                    "Failed to warm activity count for cluster 0x{}: {}",
                    hex::encode(cluster_id),
                    e
                );
            }
        }
    }

    info!(
        clusters_warmed = warmed,
        clusters_total = clusters.len(),
        "Warmed cluster activity count caches"
    );
}

fn refresh_named_script_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let script_families = state.store.list_script_families()?;
    state.mem_cache.set(
        CACHE_KEY_SCRIPT_FAMILIES_ALL,
        &script_families,
        CacheTtl::ASSETS,
    );

    let script_infos = state.store.list_script_infos()?;
    state
        .mem_cache
        .set(CACHE_KEY_SCRIPTS_ALL, &script_infos, CacheTtl::ASSETS);

    let script_versions = state.store.list_script_versions()?;
    state.mem_cache.set(
        CACHE_KEY_SCRIPT_VERSIONS_ALL,
        &script_versions,
        CacheTtl::ASSETS,
    );

    let mut scripts: Vec<CachedScriptEntry> = script_versions
        .into_iter()
        .filter_map(|(version_hash, info)| {
            info.name.map(|name| CachedScriptEntry {
                code_hash: format!("0x{}", hex::encode(version_hash)),
                name,
            })
        })
        .collect();
    scripts.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.code_hash.cmp(&b.code_hash))
    });
    state
        .mem_cache
        .set(CACHE_KEY_SCRIPTS_NAMED, &scripts, CacheTtl::ASSETS);

    Ok(())
}

/// Sync function that computes and caches all asset lists.
/// Uses pre-aggregated CFs for objects, including Spore/DOB collections.
/// Uses a single scan for all token 24h transfers instead of N+1 per-token queries.
fn build_asset_caches_sync(
    state: &AppState,
) -> anyhow::Result<(Vec<CachedAssetEntry>, Vec<CachedAssetEntry>)> {
    let now_ms = chrono::Utc::now().timestamp_millis();

    // -- Token assets (list_tokens + scan_all_token_24h_transfers + per-token holder scans) --
    let tokens = state.store.list_tokens()?;
    let transfers_24h_map = state.store.scan_all_token_24h_transfers(now_ms)?;
    let mut token_assets: Vec<CachedAssetEntry> = Vec::with_capacity(tokens.len());

    for (hash, info) in &tokens {
        // Live-scan CF_TOKEN_HOLDERS for both authoritative aggregates. This is the
        // same single calculation path used by the token detail endpoint.
        let (holders_count, total_supply) = state.store.aggregate_token_holder_stats(hash)?;

        // Skip noise tokens: no name/symbol and no holders
        if info.name.is_none() && info.symbol.is_none() && holders_count == 0 {
            continue;
        }

        let transfers_24h = transfers_24h_map.get(hash.as_slice()).copied().unwrap_or(0);
        let token_daily = state.store.list_token_daily_deltas(hash)?;
        let (owned_capacity, owned_knowledge) = accumulate_owned_capacity(
            token_daily
                .into_iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )
        .map_err(|err| {
            anyhow::anyhow!(
                "invalid token daily deltas during warmup for type_hash=0x{}: {}",
                hex::encode(hash),
                err
            )
        })?;

        token_assets.push(CachedAssetEntry {
            id: format!("0x{}", hex::encode(hash)),
            asset_type: "token".to_string(),
            standard: info.standard.clone(),
            name: info.name.clone(),
            symbol: info.symbol.clone(),
            icon_url: info.icon_url.clone(),
            holders_count,
            transfers_count: info.transfers_count,
            transfers_24h,
            decimals: info.decimals.map(|d| d as i16),
            total_supply: Some(total_supply.to_string()),
            maximum_supply: info.max_supply.map(|s| s.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            owned_capacity: Some(owned_capacity.to_string()),
            owned_knowledge: Some(owned_knowledge.to_string()),
            composition_tier: None,
            onchain_ratio: None,
            onchain_count: None,
            type_code_hash: Some(format!("0x{}", hex::encode(&info.type_code_hash))),
            type_hash_type: hash_type_to_string(info.hash_type).map(|s| s.to_string()),
            type_args: Some(format!("0x{}", hex::encode(&info.type_args))),
            description: info.description.clone(),
        });
    }

    token_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    // -- Object assets, including Spore/DOB collections --
    let mut object_assets: Vec<CachedAssetEntry> = Vec::new();

    // Spore/DOB collections from pre-aggregated cluster_agg CF
    let cluster_aggs = state.store.list_cluster_aggregates()?;
    let spore_transfers_24h_map = state.store.scan_all_spore_24h_transfers(now_ms)?;
    object_assets.reserve(cluster_aggs.len());

    for (cluster_id_bytes, agg) in &cluster_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let cluster_hex = format!("0x{}", hex::encode(cluster_id_bytes));
        let display_name = resolve_dob_collection_name(
            state.store.as_ref(),
            cluster_id_bytes,
            agg.name.as_deref(),
        );
        let transfers_24h = spore_transfers_24h_map
            .get(cluster_id_bytes.as_slice())
            .copied()
            .unwrap_or(0);
        let cluster_daily = state.store.list_cluster_daily_deltas(cluster_id_bytes)?;
        let (owned_capacity, owned_knowledge) = accumulate_owned_capacity(
            cluster_daily
                .into_iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "invalid cluster daily capacity deltas for cluster_id=0x{}: {}",
                hex::encode(cluster_id_bytes),
                e
            )
        })?;
        let total_onchain = agg.btc_ckb_count + agg.pure_ckb_count;
        let onchain_ratio = format_ratio_4(total_onchain, agg.live_count);
        let composition_tier = resolve_composition_tier(
            agg.btc_ckb_count,
            agg.pure_ckb_count,
            agg.decentralized_mixture_count,
            agg.centralized_mixture_count,
            agg.unknown_count,
        );

        object_assets.push(CachedAssetEntry {
            id: cluster_hex.clone(),
            asset_type: "object".to_string(),
            standard: "spore".to_string(),
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.owner_count,
            transfers_count: agg.total_count,
            transfers_24h,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(cluster_hex),
            cluster_name: display_name,
            owned_capacity: Some(owned_capacity.to_string()),
            owned_knowledge: Some(owned_knowledge.to_string()),
            composition_tier: Some(composition_tier),
            onchain_ratio: Some(onchain_ratio),
            onchain_count: Some(total_onchain),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    // Object collections from pre-aggregated nft_collection_agg CF
    let nft_aggs = state.store.list_mnft_collection_aggregates()?;
    let nft_transfers_24h_map = state.store.scan_all_object_24h_transfers(now_ms)?;
    object_assets.reserve(nft_aggs.len());

    for (collection_id_bytes, agg) in &nft_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let collection_hex = format!("0x{}", hex::encode(collection_id_bytes));
        let transfers_24h = nft_transfers_24h_map
            .get(collection_id_bytes.as_slice())
            .copied()
            .unwrap_or(0);
        let raw_standard = agg.standard.asset_standard().to_string();
        let standard = resolve_collection_standard(collection_id_bytes, &raw_standard);
        let asset_type = if standard == "dotbit" || standard == "did_ckb" {
            "identity"
        } else {
            "object"
        };
        let display_name = resolve_object_collection_name(&standard, agg.name.as_deref());
        let has_tier_counts = agg.pure_ckb_count > 0
            || agg.btc_ckb_count > 0
            || agg.decentralized_mixture_count > 0
            || agg.centralized_mixture_count > 0;
        let composition_tier = if has_tier_counts {
            resolve_composition_tier(
                agg.btc_ckb_count,
                agg.pure_ckb_count,
                agg.decentralized_mixture_count,
                agg.centralized_mixture_count,
                agg.unknown_count,
            )
        } else {
            resolve_object_collection_composition_tier_override(&standard)
                .unwrap_or("unknown")
                .to_string()
        };
        let onchain_count = if has_tier_counts {
            agg.pure_ckb_count + agg.btc_ckb_count
        } else if matches!(composition_tier.as_str(), "btc_ckb" | "pure_ckb") {
            agg.live_count
        } else {
            0
        };
        let onchain_ratio = format_ratio_4(onchain_count, agg.live_count);
        let nft_daily = state.store.list_mnft_daily_deltas(collection_id_bytes)?;
        let (owned_capacity, owned_knowledge) = accumulate_owned_capacity(
            nft_daily
                .into_iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "invalid object daily capacity deltas for collection_id=0x{}: {}",
                hex::encode(collection_id_bytes),
                e
            )
        })?;

        object_assets.push(CachedAssetEntry {
            id: collection_hex.clone(),
            asset_type: asset_type.to_string(),
            standard,
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.holders_count,
            transfers_count: agg.total_count,
            transfers_24h,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: display_name,
            owned_capacity: Some(owned_capacity.to_string()),
            owned_knowledge: Some(owned_knowledge.to_string()),
            composition_tier: Some(composition_tier),
            onchain_ratio: Some(onchain_ratio),
            onchain_count: Some(onchain_count),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    // Identity collections from pre-aggregated identity_agg CF
    let identity_aggs = state.store.list_identity_collection_aggregates()?;
    object_assets.reserve(identity_aggs.len());

    for (collection_id_bytes, agg) in &identity_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let collection_hex = format!("0x{}", hex::encode(collection_id_bytes));
        let transfers_24h = nft_transfers_24h_map
            .get(collection_id_bytes.as_slice())
            .copied()
            .unwrap_or(0);
        let standard_str = agg.standard.asset_standard().to_string();
        let standard = resolve_collection_standard(collection_id_bytes, &standard_str);
        let display_name = resolve_object_collection_name(&standard, agg.name.as_deref());
        let composition_tier = resolve_object_collection_composition_tier_override(&standard)
            .unwrap_or("unknown")
            .to_string();
        let onchain_count = if matches!(composition_tier.as_str(), "btc_ckb" | "pure_ckb") {
            agg.live_count
        } else {
            0
        };
        let onchain_ratio = format_ratio_4(onchain_count, agg.live_count);
        let id_daily = state.store.list_mnft_daily_deltas(collection_id_bytes)?;
        let (owned_capacity, owned_knowledge) = accumulate_owned_capacity(
            id_daily
                .into_iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "invalid identity daily capacity deltas for collection_id=0x{}: {}",
                hex::encode(collection_id_bytes),
                e
            )
        })?;

        object_assets.push(CachedAssetEntry {
            id: collection_hex.clone(),
            asset_type: "identity".to_string(),
            standard,
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.holders_count,
            transfers_count: agg.total_count,
            transfers_24h,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: display_name,
            owned_capacity: Some(owned_capacity.to_string()),
            owned_knowledge: Some(owned_knowledge.to_string()),
            composition_tier: Some(composition_tier),
            onchain_ratio: Some(onchain_ratio),
            onchain_count: Some(onchain_count),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    object_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    Ok((token_assets, object_assets))
}

fn refresh_assets_cache_sync(state: &AppState) -> anyhow::Result<()> {
    // NOTE: Script cache is refreshed by its own independent loop
    // (refresh_script_cache_loop) so it survives the long asset build.
    // Do NOT call refresh_named_script_cache_sync() here.

    let (token_assets, object_assets) = match build_asset_caches_sync(state) {
        Ok(caches) => caches,
        Err(err) => {
            state.token_cache.store(Arc::new(None));
            state.object_cache.store(Arc::new(None));
            state.record_asset_cache_warmup_error(err.to_string());
            return Err(err);
        }
    };

    state.token_cache.store(Arc::new(Some(token_assets)));
    state.object_cache.store(Arc::new(Some(object_assets)));
    state.clear_asset_cache_warmup_error();
    // NOTE: Address cache is refreshed by its own independent loop
    // (refresh_address_cache_loop) so it survives the long asset build.
    // Do NOT call refresh_address_cache_sync() here.
    refresh_spore_cache_sync(state)?;

    Ok(())
}

pub async fn warmup_assets_cache_once(state: Arc<AppState>) -> anyhow::Result<()> {
    state.update_background_task("cache_warmup", |entry| {
        entry.kind = BackgroundTaskKind::Job;
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
        entry.message = Some("Warming up asset caches...".to_string());
    });

    let start = std::time::Instant::now();
    let refresh = tokio::task::spawn_blocking(move || {
        // Seed script cache immediately (its independent loop hasn't started yet).
        if let Err(e) = refresh_named_script_cache_sync(&state) {
            tracing::warn!("Initial script cache warmup failed (non-fatal): {}", e);
        }
        // Seed address cache immediately (its independent loop hasn't started yet).
        if let Err(e) = refresh_address_cache_sync(&state) {
            tracing::warn!("Initial address cache warmup failed (non-fatal): {}", e);
        }
        let result = refresh_assets_cache_sync(&state);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        match &result {
            Ok(()) => {
                tracing::info!(elapsed_ms, "Asset cache warmup completed");
                state.update_background_task("cache_warmup", |entry| {
                    entry.state = BackgroundTaskState::Completed;
                    entry.elapsed_ms = Some(elapsed_ms);
                    entry.message = Some("Asset caches ready".to_string());
                });
            }
            Err(e) => {
                state.update_background_task("cache_warmup", |entry| {
                    entry.state = BackgroundTaskState::Failed;
                    entry.elapsed_ms = Some(elapsed_ms);
                    entry.error = Some(e.to_string());
                });
            }
        }
        result
    })
    .await
    .map_err(|e| anyhow::anyhow!("assets cache warmup task panicked: {}", e))?;
    refresh
}

/// Read materialized chart snapshots from store and populate caches.
/// This replaces the old `compute_live_cell_charts` that performed expensive full CF scans.
fn warmup_cell_charts_from_store(state: &AppState) -> Result<(), String> {
    // Cell distribution: size chart
    if let Some((_, snapshot)) = state
        .store
        .get_latest_cell_distribution()
        .map_err(|e| format!("failed to read cell distribution: {e}"))?
    {
        let size_response = build_cell_size_response(&snapshot);
        state.mem_cache.set(
            "chart:cell-size-distribution:v1",
            &size_response,
            CacheTtl::CHART,
        );
    }

    // Address cohort retention chart
    if let Some((_, cohort)) = state
        .store
        .get_latest_address_cohort()
        .map_err(|e| format!("failed to read address cohort: {e}"))?
    {
        let response = build_address_cohort_response(&cohort);
        state.mem_cache.set(
            "chart:address-cohort-retention:v1",
            &response,
            CacheTtl::CHART,
        );
    }

    Ok(())
}

pub async fn warmup_chart_caches(state: Arc<AppState>) {
    info!("Starting cache warmup for charts...");

    // Total chart types: 2 materialized + 10 async = 12
    let chart_start = std::time::Instant::now();
    state.update_background_task("chart_warmup", |entry| {
        entry.kind = BackgroundTaskKind::Job;
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
        entry.message = Some("Warming up chart caches...".to_string());
        entry.progress_current = Some(0);
        entry.progress_total = Some(12);
    });

    // Warm up materialized chart caches from store snapshots (fast read, no CF scan).
    let state_for_cells = state.clone();
    match tokio::task::spawn_blocking(move || warmup_cell_charts_from_store(&state_for_cells)).await
    {
        Ok(Ok(())) => {
            // Transfer mem_cache entries to the async cache backend
            if let Some(size) = state
                .mem_cache
                .get::<ChartResponse>("chart:cell-size-distribution:v1")
            {
                state
                    .cache
                    .set("chart:cell-size-distribution:v1", &size, CacheTtl::CHART)
                    .await;
            }
            if let Some(cohort) = state
                .mem_cache
                .get::<ChartResponse>("chart:address-cohort-retention:v1")
            {
                state
                    .cache
                    .set(
                        "chart:address-cohort-retention:v1",
                        &cohort,
                        CacheTtl::CHART,
                    )
                    .await;
            }
            info!("Warmed up materialized chart caches (cell-size + address-cohort)");
            state.update_background_task("chart_warmup", |entry| {
                entry.progress_current = Some(2);
                entry.message = Some("Materialized charts ready".to_string());
            });
        }
        Ok(Err(e)) => {
            tracing::warn!("Failed to warmup materialized charts: {}", e);
            state.update_background_task("chart_warmup", |entry| {
                entry.progress_current = Some(2);
                entry.message = Some(format!("Materialized charts failed: {}", e));
            });
        }
        Err(e) => {
            tracing::warn!("Materialized chart warmup panicked: {}", e);
            state.update_background_task("chart_warmup", |entry| {
                entry.progress_current = Some(2);
                entry.message = Some(format!("Materialized chart warmup panicked: {}", e));
            });
        }
    }

    // These chart caches used to be prefilled with placeholder payloads (often empty),
    // which overrides real chart handlers after cache flush/restart.
    // Purge them on startup and let route handlers populate on first request.
    const STUB_CHART_KEYS: &[&str] = &[
        "chart:average-block-time",
        "chart:hash-rate",
        "chart:difficulty",
        "chart:uncle-rate",
        "chart:block-time-distribution",
        "chart:block-time-distribution:v2",
        "chart:epoch-time-distribution",
        "chart:epoch-time-length",
        "chart:miner-address-distribution",
        "chart:total-supply",
        "chart:secondary-issuance",
    ];
    for key in STUB_CHART_KEYS {
        state.cache.delete(key).await;
    }

    macro_rules! run_warmup {
        ($key:expr, $fn:ident) => {
            async {
                if state.cache.get::<serde_json::Value>($key).await.is_none() {
                    match $fn(&state).await {
                        Ok(_) => info!("Warmed up cache: {}", $key),
                        Err(e) => tracing::warn!("Failed to warmup {}: {}", $key, e),
                    }
                }
            }
        };
    }

    tokio::join!(
        run_warmup!("chart:average-block-time", warmup_average_block_time),
        run_warmup!("chart:hash-rate", warmup_hash_rate),
        run_warmup!("chart:difficulty", warmup_difficulty),
        run_warmup!("chart:uncle-rate", warmup_uncle_rate),
        run_warmup!(
            "chart:block-time-distribution:v2",
            warmup_block_time_distribution
        ),
        run_warmup!(
            "chart:epoch-time-distribution",
            warmup_epoch_time_distribution
        ),
        run_warmup!("chart:epoch-time-length", warmup_epoch_time_length),
        run_warmup!(
            "chart:miner-address-distribution",
            warmup_miner_distribution
        ),
        run_warmup!("chart:total-supply", warmup_total_supply),
        run_warmup!("chart:secondary-issuance", warmup_secondary_issuance),
    );

    let elapsed_ms = chart_start.elapsed().as_secs_f64() * 1000.0;
    state.update_background_task("chart_warmup", |entry| {
        entry.state = BackgroundTaskState::Completed;
        entry.progress_current = Some(12);
        entry.elapsed_ms = Some(elapsed_ms);
        entry.message = Some("All chart caches ready".to_string());
    });

    info!("Cache warmup completed");
}

async fn warmup_average_block_time(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:average-block-time").await;
    Ok(())
}

async fn warmup_hash_rate(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:hash-rate").await;
    Ok(())
}

async fn warmup_difficulty(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:difficulty").await;
    Ok(())
}

async fn warmup_uncle_rate(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:uncle-rate").await;
    Ok(())
}

async fn warmup_block_time_distribution(state: &AppState) -> Result<(), String> {
    let response = build_block_time_distribution_response(state.store.as_ref())?;

    // Only cache if the distribution has actual data (non-zero ratios).
    // Secondary store may not have caught up at startup, producing all-zero data.
    let has_data = response
        .data
        .iter()
        .any(|p| p.value.parse::<f64>().is_ok_and(|v| v > 0.0));
    if has_data {
        state
            .cache
            .set(
                "chart:block-time-distribution:v2",
                &response,
                CHART_CACHE_TTL,
            )
            .await;
    }

    Ok(())
}

async fn warmup_epoch_time_distribution(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:epoch-time-distribution").await;
    Ok(())
}

async fn warmup_epoch_time_length(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:epoch-time-length").await;
    Ok(())
}

async fn warmup_miner_distribution(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:miner-address-distribution").await;
    Ok(())
}

async fn warmup_total_supply(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:total-supply").await;
    Ok(())
}

async fn warmup_secondary_issuance(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:secondary-issuance").await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;
    use ckbadger_common::{BackgroundTaskEntry, BackgroundTaskKind};
    use ckbadger_store::{ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile};

    fn compact_to_difficulty(compact: i64) -> u64 {
        let difficulty = ckb_compact_to_difficulty(compact as u32);
        difficulty.to_string().parse::<u64>().unwrap_or(u64::MAX)
    }

    fn background_entry(name: &str) -> BackgroundTaskEntry {
        BackgroundTaskEntry {
            name: name.to_string(),
            kind: BackgroundTaskKind::Job,
            state: BackgroundTaskState::Waiting,
            message: Some("placeholder".to_string()),
            progress_current: Some(1),
            progress_total: Some(2),
            rate: Some(3.0),
            eta_seconds: Some(4.0),
            started_at: Some(5),
            elapsed_ms: Some(6.0),
            last_success_at: Some(7),
            last_trigger_reason: Some("old-trigger".to_string()),
            error: Some("old-error".to_string()),
        }
    }

    #[test]
    fn test_compact_to_difficulty_genesis() {
        let difficulty = compact_to_difficulty(0x20010000);
        assert_eq!(difficulty, 256);
    }

    #[test]
    fn test_compact_to_difficulty_higher() {
        let d1 = compact_to_difficulty(0x20010000);
        let d2 = compact_to_difficulty(0x20008000);
        assert_eq!(d2, d1 * 2);
    }

    #[test]
    fn test_compact_to_difficulty_zero_mantissa() {
        assert_eq!(compact_to_difficulty(0x20000000), 0);
    }

    #[test]
    fn test_compact_to_difficulty_lower_exponent() {
        let d_high_exp = compact_to_difficulty(0x20010000);
        let d_low_exp = compact_to_difficulty(0x1f010000);
        assert!(d_low_exp > d_high_exp);
        assert_eq!(d_low_exp, d_high_exp * 256);
    }

    #[test]
    fn test_push_bounded_keeps_top_n_values() {
        let mut heap: BinaryHeap<Reverse<i32>> = BinaryHeap::new();
        push_bounded(&mut heap, 1, 3);
        push_bounded(&mut heap, 4, 3);
        push_bounded(&mut heap, 2, 3);
        push_bounded(&mut heap, 6, 3);
        push_bounded(&mut heap, 3, 3);

        let mut values: Vec<i32> = heap.into_iter().map(|v| v.0).collect();
        values.sort();
        assert_eq!(values, vec![3, 4, 6]);
    }

    #[test]
    fn test_cached_address_entry_from_candidate_formats_hash_and_balance() {
        let candidate = AddressCandidate {
            lock_hash: vec![0xAB; 32],
            balance: 12345,
            live_cells_count: 3,
            transactions_count: 9,
            last_activity_block: 100,
        };
        let entry = cached_address_entry_from_candidate(candidate);
        assert_eq!(entry.lock_script_hash, format!("0x{}", "ab".repeat(32)));
        assert_eq!(entry.balance, "12345");
        assert_eq!(entry.live_cells_count, 3);
        assert_eq!(entry.transactions_count, 9);
        assert_eq!(entry.last_activity_block, 100);
    }

    #[test]
    fn test_api_cache_refresh_startup_marks_waiting_watcher() {
        let mut entry = background_entry("api_cache_refresh");
        set_api_cache_refresh_startup(&mut entry);

        assert_eq!(entry.kind, BackgroundTaskKind::Watcher);
        assert_eq!(entry.state, BackgroundTaskState::Waiting);
        assert_eq!(entry.message.as_deref(), Some("Waiting for first refresh"));
        assert_eq!(entry.last_trigger_reason.as_deref(), Some("startup"));
        assert_eq!(entry.error, None);
        assert_eq!(entry.started_at, None);
    }

    #[test]
    fn test_api_cache_refresh_cycle_start_marks_running_watcher() {
        let mut entry = background_entry("api_cache_refresh");
        set_api_cache_refresh_cycle_start(&mut entry);

        assert_eq!(entry.kind, BackgroundTaskKind::Watcher);
        assert_eq!(entry.state, BackgroundTaskState::Running);
        assert_eq!(entry.message.as_deref(), Some("Refreshing API caches"));
        assert_eq!(entry.last_trigger_reason.as_deref(), Some("new_tip"));
        assert_eq!(entry.error, None);
        assert!(entry.started_at.is_some());
        assert_eq!(entry.elapsed_ms, None);
        assert_eq!(entry.progress_current, None);
        assert_eq!(entry.progress_total, None);
        assert_eq!(entry.rate, None);
        assert_eq!(entry.eta_seconds, None);
    }

    #[test]
    fn test_api_cache_refresh_success_returns_to_waiting_and_records_last_success() {
        let mut entry = background_entry("api_cache_refresh");
        set_api_cache_refresh_success(&mut entry, 4321.0, 1_711_111_111);

        assert_eq!(entry.kind, BackgroundTaskKind::Watcher);
        assert_eq!(entry.state, BackgroundTaskState::Waiting);
        assert_eq!(entry.message.as_deref(), Some("Idle"));
        assert_eq!(entry.elapsed_ms, Some(4321.0));
        assert_eq!(entry.last_success_at, Some(1_711_111_111));
        assert_eq!(entry.last_trigger_reason.as_deref(), Some("new_tip"));
        assert_eq!(entry.error, None);
    }

    #[test]
    fn test_api_cache_refresh_tip_unchanged_stays_waiting_with_idle_message() {
        let mut entry = background_entry("api_cache_refresh");
        set_api_cache_refresh_idle(&mut entry, "tip_unchanged");

        assert_eq!(entry.kind, BackgroundTaskKind::Watcher);
        assert_eq!(entry.state, BackgroundTaskState::Waiting);
        assert_eq!(entry.message.as_deref(), Some("Idle"));
        assert_eq!(entry.last_trigger_reason.as_deref(), Some("tip_unchanged"));
        assert_eq!(entry.error, None);
        assert_eq!(entry.elapsed_ms, Some(6.0));
    }

    #[test]
    fn test_api_cache_refresh_failure_marks_failed_and_keeps_cycle_duration() {
        let mut entry = background_entry("api_cache_refresh");
        set_api_cache_refresh_failure(&mut entry, 987.5, "rpc timeout".to_string());

        assert_eq!(entry.kind, BackgroundTaskKind::Watcher);
        assert_eq!(entry.state, BackgroundTaskState::Failed);
        assert_eq!(entry.message.as_deref(), Some("Refresh failed"));
        assert_eq!(entry.elapsed_ms, Some(987.5));
        assert_eq!(entry.error.as_deref(), Some("rpc timeout"));
        assert_eq!(entry.last_trigger_reason.as_deref(), Some("new_tip"));
    }

    fn make_entry(
        block: i64,
        is_live: bool,
        owner: Option<Vec<u8>>,
        name: Option<&str>,
        is_cluster: bool,
    ) -> ObjectEntry {
        ObjectEntry {
            standard: if is_cluster {
                ObjectStandard::SporeCluster
            } else {
                ObjectStandard::Spore
            },
            collection_id: None,
            token_id: None,
            owner_lock_hash: owner,
            name: name.map(|s| s.to_string()),
            description: None,
            is_live,
            created_at_block: block,
            created_at_tx: vec![0x11; 32],
            extra: ObjectExtra::Spore {
                content_type: "text/plain".to_string(),
                content_length: 5,
                media_profile: SporeMediaProfile::default(),
            },
        }
    }

    #[test]
    fn test_spore_cache_build_indexes() {
        let owner_a = vec![0xAA; 32];
        let owner_b = vec![0xBB; 32];
        let spores = vec![
            (
                vec![0x01; 32],
                make_entry(300, true, Some(owner_a.clone()), Some("Alpha"), false),
            ),
            (
                vec![0x02; 32],
                make_entry(200, false, Some(owner_a.clone()), Some("Beta"), false),
            ),
            (
                vec![0x03; 32],
                make_entry(100, true, Some(owner_b.clone()), Some("Gamma"), false),
            ),
            (
                vec![0x04; 32],
                make_entry(50, true, Some(owner_a.clone()), None, false),
            ),
            (
                vec![0x05; 32],
                make_entry(25, true, Some(owner_b.clone()), Some("Cluster One"), true),
            ),
        ];

        let cache = SporeCache::build(spores);

        assert_eq!(cache.all.len(), 5);
        // The live cluster cell (index 4) is excluded from every spore-serving
        // index: it must not surface in /spore/objects or /spore/owner rows.
        assert_eq!(cache.live_indices, vec![0, 2, 3]);
        assert_eq!(cache.by_owner.get(&owner_a).unwrap(), &vec![0, 3]);
        assert_eq!(cache.by_owner.get(&owner_b).unwrap(), &vec![2]);
        assert_eq!(cache.name_index.len(), 3);
        assert_eq!(cache.name_index[0], (0, "alpha".to_string()));
        assert_eq!(cache.name_index[1], (1, "beta".to_string()));
        assert_eq!(cache.name_index[2], (2, "gamma".to_string()));
    }

    #[test]
    fn test_spore_cache_empty() {
        let cache = SporeCache::build(vec![]);
        assert!(cache.all.is_empty());
        assert!(cache.live_indices.is_empty());
        assert!(cache.by_owner.is_empty());
        assert!(cache.name_index.is_empty());
    }
}
