use crate::cache::CacheTtl;
use crate::routes::assets::AssetResponse;
use crate::routes::statistics::build_block_time_distribution_response;
use crate::utils::{
    accumulate_live_capacity, resolve_collection_standard, resolve_dob_collection_name,
    resolve_nft_collection_name, resolve_nft_collection_storage_tier_override,
};
use crate::AppState;
use ckbadger_store::AddressBalance;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);

// Cache keys for assets
pub const CACHE_KEY_ASSETS_TOKEN: &str = "assets:token";
pub const CACHE_KEY_ASSETS_NFT: &str = "assets:nft";
pub const CACHE_KEY_ADDRESSES_TOP: &str = "addresses:top";
pub const CACHE_KEY_ADDRESSES_ACTIVE: &str = "addresses:active";
pub const CACHE_KEY_SPORES_ALL: &str = "spores:all";
pub const CACHE_KEY_SCRIPTS_ALL: &str = "scripts:all";
pub const CACHE_KEY_SCRIPTS_NAMED: &str = "scripts:named";
const ADDRESS_CACHE_LIMIT: usize = 500;
const SPORE_CACHE_LIMIT: usize = 100_000;

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
    pub live_capacity: Option<String>,
    pub live_occupied_capacity: Option<String>,
    pub storage_tier: Option<String>,
    pub fully_onchain_ratio: Option<String>,
    pub fully_onchain_count: Option<i64>,
    // Token-specific fields (None for NFT entries)
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
            live_capacity: self.live_capacity.clone(),
            live_occupied_capacity: self.live_occupied_capacity.clone(),
            storage_tier: self.storage_tier.clone(),
            fully_onchain_ratio: self.fully_onchain_ratio.clone(),
            fully_onchain_count: self.fully_onchain_count,
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

fn resolve_storage_tier(
    fully_onchain: i64,
    decentralized_external: i64,
    centralized_dependent: i64,
    unknown: i64,
) -> String {
    if centralized_dependent > 0 {
        return "centralized_dependent".to_string();
    }
    if decentralized_external > 0 {
        return "decentralized_external".to_string();
    }
    if fully_onchain > 0 && unknown == 0 {
        return "fully_onchain".to_string();
    }
    if unknown > 0 {
        return "unknown".to_string();
    }
    "unknown".to_string()
}

/// Background loop that refreshes the assets cache every 30 seconds.
/// Skips the refresh cycle when the sync tip block number hasn't changed
/// since the last successful refresh, avoiding wasteful CF scans when idle.
pub async fn refresh_assets_cache_loop(state: Arc<AppState>) {
    let mut last_refreshed_tip: i64 = -1;
    loop {
        let current_tip = state
            .store
            .get_sync_status()
            .map(|s| s.tip_block_number)
            .unwrap_or(-1);

        if current_tip == last_refreshed_tip {
            tracing::trace!("Warmup: tip unchanged at {}, skipping refresh", current_tip);
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }

        let state_clone = state.clone();
        let result =
            tokio::task::spawn_blocking(move || refresh_assets_cache_sync(&state_clone)).await;

        match result {
            Ok(Ok(())) => {
                last_refreshed_tip = current_tip;
                tracing::debug!("Assets cache refreshed at tip {}", current_tip);
            }
            Ok(Err(e)) => {
                tracing::warn!("Assets cache refresh failed: {}", e);
            }
            Err(e) => {
                tracing::warn!("Assets cache refresh task panicked: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

fn hash_type_to_string(hash_type: u8) -> String {
    match hash_type {
        0 => "data".to_string(),
        1 => "type".to_string(),
        2 => "data1".to_string(),
        4 => "data2".to_string(),
        _ => format!("unknown({})", hash_type),
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
    spores.sort_by(|a, b| b.1.created_at_block.cmp(&a.1.created_at_block));
    state
        .mem_cache
        .set(CACHE_KEY_SPORES_ALL, &spores, CacheTtl::ASSETS);
    Ok(())
}

fn refresh_named_script_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let script_infos = state.store.list_script_infos()?;
    state
        .mem_cache
        .set(CACHE_KEY_SCRIPTS_ALL, &script_infos, CacheTtl::ASSETS);

    let mut scripts: Vec<CachedScriptEntry> = script_infos
        .into_iter()
        .filter_map(|(code_hash, info)| {
            info.name.map(|name| CachedScriptEntry {
                code_hash: format!("0x{}", hex::encode(code_hash)),
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
/// Uses pre-aggregated CFs for NFTs, including Spore/DOB collections.
/// Uses a single scan for all token 24h transfers instead of N+1 per-token queries.
fn refresh_assets_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let ttl = CacheTtl::ASSETS;
    let now_ms = chrono::Utc::now().timestamp_millis();

    // -- Token assets (2 scans: list_tokens + scan_all_token_24h_transfers) --
    let tokens = state.store.list_tokens()?;
    let transfers_24h_map = state.store.scan_all_token_24h_transfers(now_ms)?;
    let mut token_assets: Vec<CachedAssetEntry> = Vec::with_capacity(tokens.len());

    for (hash, info) in &tokens {
        // Skip noise tokens: no name/symbol and no holders
        if info.name.is_none() && info.symbol.is_none() && info.holders_count == 0 {
            continue;
        }

        let transfers_24h = transfers_24h_map.get(hash.as_slice()).copied().unwrap_or(0);
        let token_daily = state.store.list_token_daily_deltas(hash)?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(token_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
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
            holders_count: info.holders_count,
            transfers_count: info.transfers_count,
            transfers_24h,
            decimals: info.decimals.map(|d| d as i16),
            total_supply: info.total_supply.map(|s| s.to_string()),
            maximum_supply: info.max_supply.map(|s| s.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            storage_tier: None,
            fully_onchain_ratio: None,
            fully_onchain_count: None,
            type_code_hash: Some(format!("0x{}", hex::encode(&info.type_code_hash))),
            type_hash_type: Some(hash_type_to_string(info.hash_type)),
            type_args: Some(format!("0x{}", hex::encode(&info.type_args))),
            description: info.description.clone(),
        });
    }

    token_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    state
        .mem_cache
        .set(CACHE_KEY_ASSETS_TOKEN, &token_assets, ttl);

    // -- NFT assets, including Spore/DOB collections --
    let mut nft_assets: Vec<CachedAssetEntry> = Vec::new();

    // Spore/DOB collections from pre-aggregated cluster_agg CF
    let cluster_aggs = state.store.list_cluster_aggregates()?;
    let spore_transfers_24h_map = state.store.scan_all_spore_24h_transfers(now_ms)?;
    nft_assets.reserve(cluster_aggs.len());

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
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(cluster_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| {
                anyhow::anyhow!(
                    "invalid cluster daily capacity deltas for cluster_id=0x{}: {}",
                    hex::encode(cluster_id_bytes),
                    e
                )
            })?;
        let fully_onchain_ratio = format_ratio_4(agg.fully_onchain_count, agg.live_count);
        let storage_tier = resolve_storage_tier(
            agg.fully_onchain_count,
            agg.decentralized_external_count,
            agg.centralized_dependent_count,
            agg.unknown_count,
        );

        nft_assets.push(CachedAssetEntry {
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
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            storage_tier: Some(storage_tier),
            fully_onchain_ratio: Some(fully_onchain_ratio),
            fully_onchain_count: Some(agg.fully_onchain_count),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    // NFT collections from pre-aggregated nft_collection_agg CF
    let nft_aggs = state.store.list_object_collection_aggregates()?;
    let nft_transfers_24h_map = state.store.scan_all_nft_24h_transfers(now_ms)?;
    nft_assets.reserve(nft_aggs.len());

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
        let display_name = resolve_nft_collection_name(&standard, agg.name.as_deref());
        let storage_tier = resolve_nft_collection_storage_tier_override(&standard)
            .unwrap_or("unknown")
            .to_string();
        let fully_onchain_count = if storage_tier == "fully_onchain" {
            agg.live_count
        } else {
            0
        };
        let fully_onchain_ratio = format_ratio_4(fully_onchain_count, agg.live_count);
        let nft_daily = state.store.list_object_daily_deltas(collection_id_bytes)?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(nft_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| {
                anyhow::anyhow!(
                    "invalid NFT daily capacity deltas for collection_id=0x{}: {}",
                    hex::encode(collection_id_bytes),
                    e
                )
            })?;

        nft_assets.push(CachedAssetEntry {
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
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            storage_tier: Some(storage_tier),
            fully_onchain_ratio: Some(fully_onchain_ratio),
            fully_onchain_count: Some(fully_onchain_count),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    // Identity collections from pre-aggregated identity_agg CF
    let identity_aggs = state.store.list_identity_collection_aggregates()?;
    nft_assets.reserve(identity_aggs.len());

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
        let display_name = resolve_nft_collection_name(&standard, agg.name.as_deref());
        let storage_tier = resolve_nft_collection_storage_tier_override(&standard)
            .unwrap_or("unknown")
            .to_string();
        let fully_onchain_count = if storage_tier == "fully_onchain" {
            agg.live_count
        } else {
            0
        };
        let fully_onchain_ratio = format_ratio_4(fully_onchain_count, agg.live_count);
        let id_daily = state.store.list_object_daily_deltas(collection_id_bytes)?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(id_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| {
                anyhow::anyhow!(
                    "invalid identity daily capacity deltas for collection_id=0x{}: {}",
                    hex::encode(collection_id_bytes),
                    e
                )
            })?;

        nft_assets.push(CachedAssetEntry {
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
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            storage_tier: Some(storage_tier),
            fully_onchain_ratio: Some(fully_onchain_ratio),
            fully_onchain_count: Some(fully_onchain_count),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    nft_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    state.mem_cache.set(CACHE_KEY_ASSETS_NFT, &nft_assets, ttl);
    refresh_address_cache_sync(state)?;
    refresh_spore_cache_sync(state)?;
    refresh_named_script_cache_sync(state)?;

    Ok(())
}

pub async fn warmup_assets_cache_once(state: Arc<AppState>) -> anyhow::Result<()> {
    let refresh = tokio::task::spawn_blocking(move || refresh_assets_cache_sync(&state))
        .await
        .map_err(|e| anyhow::anyhow!("assets cache warmup task panicked: {}", e))?;
    refresh
}

pub async fn warmup_chart_caches(state: Arc<AppState>) {
    info!("Starting cache warmup for charts...");

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

    state
        .cache
        .set(
            "chart:block-time-distribution:v2",
            &response,
            CHART_CACHE_TTL,
        )
        .await;

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

    fn compact_to_difficulty(compact: i64) -> u64 {
        let difficulty = ckb_compact_to_difficulty(compact as u32);
        difficulty.to_string().parse::<u64>().unwrap_or(u64::MAX)
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
}
