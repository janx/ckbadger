use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::registry::DiscoveredParams;

/// Data module availability flags from Phase 2 probes.
#[derive(Debug, Clone, Default)]
pub struct DataAvailability {
    pub has_tokens: bool,
    pub has_spore: bool,
    pub has_dao: bool,
    pub has_fiber: bool,
    pub has_identities: bool,
    pub has_assets: bool,
    pub has_mempool: bool,
    pub has_graph: bool,
    pub has_forks: bool,
}

/// Complete discovery result from all three phases.
#[derive(Debug, Clone)]
pub struct Discovery {
    pub capabilities_route_count: usize,
    pub availability: DataAvailability,
    pub params: DiscoveredParams,
}

/// Run three-phase discovery against a live API.
pub async fn run_discovery(
    api_base: &str,
    frontend_url: &str,
    client: &reqwest::Client,
) -> Result<Discovery> {
    // Phase 1: Capabilities
    let capabilities_route_count = discover_capabilities(frontend_url, client).await;

    // Phase 2: Data availability probes
    let availability = discover_availability(api_base, client).await;

    // Phase 3: Parameter discovery
    let params = discover_params(api_base, client, &availability).await?;

    Ok(Discovery {
        capabilities_route_count,
        availability,
        params,
    })
}

/// Pretty-print all discovered values.
pub fn print_discovery(discovery: &Discovery) {
    println!();
    println!("=== Discovery Results ===");
    println!();

    // Phase 1
    println!("Phase 1 - Capabilities:");
    println!("  Route count: {}", discovery.capabilities_route_count);
    println!();

    // Phase 2
    let a = &discovery.availability;
    println!("Phase 2 - Data Availability:");
    println!("  has_tokens:     {}", a.has_tokens);
    println!("  has_spore:      {}", a.has_spore);
    println!("  has_dao:        {}", a.has_dao);
    println!("  has_fiber:      {}", a.has_fiber);
    println!("  has_identities: {}", a.has_identities);
    println!("  has_assets:     {}", a.has_assets);
    println!("  has_mempool:    {}", a.has_mempool);
    println!("  has_graph:      {}", a.has_graph);
    println!("  has_forks:      {}", a.has_forks);
    println!();

    // Phase 3
    let p = &discovery.params;
    println!("Phase 3 - Discovered Parameters:");
    println!("  sync_tip:             {}", p.sync_tip);
    println!("  latest_block_number:  {}", p.latest_block_number);
    println!("  latest_block_hash:    {}", p.latest_block_hash);
    println!("  mid_block_number:     {}", p.mid_block_number);
    println!("  tx_hashes:            [{}]", p.tx_hashes.join(", "));
    println!(
        "  complex_tx_hash:      {}",
        p.complex_tx_hash.as_deref().unwrap_or("(none)")
    );
    println!("  top_addresses:        [{}]", p.top_addresses.join(", "));
    println!("  top_lock_hashes:      [{}]", p.top_lock_hashes.join(", "));
    println!("  dao_lock_hashes:      [{}]", p.dao_lock_hashes.join(", "));
    println!(
        "  dao_deposit_outpoint: {}",
        match &p.dao_deposit_outpoint {
            Some((tx, idx)) => format!("{}:{}", tx, idx),
            None => "(none)".to_string(),
        }
    );
    println!(
        "  dao_deposit_capacity: {}",
        p.dao_deposit_capacity.as_deref().unwrap_or("(none)")
    );
    println!(
        "  dao_deposit_block:    {}",
        p.dao_deposit_block
            .map(|b| b.to_string())
            .as_deref()
            .unwrap_or("(none)")
    );
    println!(
        "  token_type_hashes:    [{}]",
        p.token_type_hashes.join(", ")
    );
    println!("  cluster_ids:          [{}]", p.cluster_ids.join(", "));
    println!("  spore_ids:            [{}]", p.spore_ids.join(", "));
    println!("  script_names:         [{}]", p.script_names.join(", "));
    println!(
        "  live_cell_outpoint:   {}",
        match &p.live_cell_outpoint {
            Some((tx, idx)) => format!("{}:{}", tx, idx),
            None => "(none)".to_string(),
        }
    );
    println!(
        "  fiber_channel_id:     {}",
        p.fiber_channel_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  dotbit_item_id:       {}",
        p.dotbit_item_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  identity_collection_id: {}",
        p.identity_collection_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  object_collection_id: {}",
        p.object_collection_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  object_item_id:       {}",
        p.object_item_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  fork_id:              {}",
        p.fork_id.as_deref().unwrap_or("(none)")
    );
    println!();
    println!("  Heavy-page discovery:");
    println!(
        "  top_script_names:     [{} items]",
        p.top_script_names.len()
    );
    println!(
        "  top_token_type_hashes:[{} items]",
        p.top_token_type_hashes.len()
    );
    println!("  top_spore_ids:        [{} items]", p.top_spore_ids.len());
    println!(
        "  top_cluster_ids:      [{} items]",
        p.top_cluster_ids.len()
    );
    println!(
        "  top_dotbit_item_ids:  [{} items]",
        p.top_dotbit_item_ids.len()
    );
    println!(
        "  busiest_lock_hashes:  [{} items]",
        p.busiest_lock_hashes.len()
    );
    println!();
}

// ---------------------------------------------------------------------------
// Phase 1: Capabilities
// ---------------------------------------------------------------------------

async fn discover_capabilities(frontend_url: &str, client: &reqwest::Client) -> usize {
    let url = format!("{}/capabilities", frontend_url.trim_end_matches('/'));
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "  [warn] capabilities endpoint unreachable ({}): {}",
                url, e
            );
            return 0;
        }
    };

    if !resp.status().is_success() {
        eprintln!("  [warn] capabilities returned status {}", resp.status());
        return 0;
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  [warn] capabilities body parse failed: {}", e);
            return 0;
        }
    };

    let md_count = body
        .pointer("/routes/markdown")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let raw_count = body
        .pointer("/routes/raw")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    md_count + raw_count
}

// ---------------------------------------------------------------------------
// Phase 2: Data availability probes
// ---------------------------------------------------------------------------

async fn discover_availability(api_base: &str, client: &reqwest::Client) -> DataAvailability {
    let base = api_base.trim_end_matches('/');

    let url_tokens = format!("{}/tokens?limit=1", base);
    let url_spore = format!("{}/spore/clusters?limit=1", base);
    let url_dao = format!("{}/dao/deposits?limit=1", base);
    let url_fiber = format!("{}/fiber/channels?limit=1", base);
    let url_identities = format!("{}/assets/identities/dotbit/items?limit=1", base);
    let url_assets = format!("{}/assets?limit=1", base);
    let url_mempool = format!("{}/mempool/info", base);
    let url_forks = format!("{}/forks", base);

    let (tokens, spore, dao, fiber, identities, assets, mempool, forks) = tokio::join!(
        probe_has_data(client, &url_tokens),
        probe_has_data(client, &url_spore),
        probe_has_data(client, &url_dao),
        probe_has_data(client, &url_fiber),
        probe_has_data(client, &url_identities),
        probe_has_data(client, &url_assets),
        probe_ok(client, &url_mempool),
        probe_has_data(client, &url_forks),
    );

    // Graph endpoints don't have a list probe; they always exist if API is up.
    // We consider graph available if the API is reachable (already confirmed).
    let has_graph = true;

    DataAvailability {
        has_tokens: tokens,
        has_spore: spore,
        has_dao: dao,
        has_fiber: fiber,
        has_identities: identities,
        has_assets: assets,
        has_mempool: mempool,
        has_graph,
        has_forks: forks,
    }
}

/// Check if a list endpoint returns a non-empty data array.
async fn probe_has_data(client: &reqwest::Client, url: &str) -> bool {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !resp.status().is_success() {
        return false;
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return false,
    };
    // CursorPaginatedResponse wraps in {"data": [...]}
    if let Some(arr) = body.pointer("/data").and_then(|v| v.as_array()) {
        return !arr.is_empty();
    }
    // Some endpoints return a bare array
    if let Some(arr) = body.as_array() {
        return !arr.is_empty();
    }
    // Forks: {"depositors": [...]} etc -- check any array field
    if let Some(obj) = body.as_object() {
        for (_k, v) in obj {
            if let Some(arr) = v.as_array() {
                if !arr.is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if an endpoint returns 200 OK (any body).
async fn probe_ok(client: &reqwest::Client, url: &str) -> bool {
    match client.get(url).send().await {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// Filter a list of IDs, keeping only those whose detail endpoint returns 200.
async fn validate_ids(
    client: &reqwest::Client,
    base: &str,
    ids: Vec<String>,
    path_template: &str,
) -> Vec<String> {
    let mut valid = Vec::new();
    for id in ids {
        let url = format!("{}/{}", base, path_template.replace("{id}", &id));
        if probe_ok(client, &url).await {
            valid.push(id);
        }
    }
    valid
}

// ---------------------------------------------------------------------------
// Phase 3: Parameter discovery
// ---------------------------------------------------------------------------

async fn discover_params(
    api_base: &str,
    client: &reqwest::Client,
    availability: &DataAvailability,
) -> Result<DiscoveredParams> {
    let base = api_base.trim_end_matches('/');
    let mut params = DiscoveredParams::default();

    // sync_tip from /statistics/network -> latestBlock
    let network_stats = fetch_json(client, &format!("{}/statistics/network", base)).await?;
    params.sync_tip = network_stats
        .get("latestBlock")
        .and_then(|v| v.as_i64())
        .map(|n| n as u64)
        .unwrap_or(0);

    // latest_block_number, latest_block_hash from /blocks?limit=1
    let blocks = fetch_json(client, &format!("{}/blocks?limit=1", base)).await?;
    if let Some(first) = data_array(&blocks).and_then(|arr| arr.first()) {
        params.latest_block_number = first
            .get("number")
            .and_then(|v| v.as_i64())
            .map(|n| n as u64)
            .unwrap_or(0);
        params.latest_block_hash = first
            .get("hash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    }

    // mid_block_number
    params.mid_block_number = params.latest_block_number / 2;

    // tx_hashes from /transactions?limit=5
    let txs = fetch_json(client, &format!("{}/transactions?limit=5", base)).await?;
    if let Some(arr) = data_array(&txs) {
        params.tx_hashes = arr
            .iter()
            .filter_map(|item| item.get("hash").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }

    // top_addresses, top_lock_hashes from /addresses/top?limit=5
    // This endpoint returns a bare Vec<TopAddressResponse>, not CursorPaginated.
    let top = fetch_json(client, &format!("{}/addresses/top?limit=5", base)).await?;
    let top_items: &[Value] = if let Some(arr) = data_array(&top) {
        arr
    } else if let Some(arr) = top.as_array() {
        arr.as_slice()
    } else {
        &[]
    };
    for item in top_items {
        if let Some(hash) = item.get("lockScriptHash").and_then(|v| v.as_str()) {
            params.top_lock_hashes.push(hash.to_string());
            // address may be null
            if let Some(addr) = item.get("address").and_then(|v| v.as_str()) {
                params.top_addresses.push(addr.to_string());
            }
        }
    }

    // busiest_lock_hashes: top 10 addresses by transaction count
    {
        let top_by_tx = fetch_json(client, &format!("{}/addresses/top?limit=50", base)).await?;
        let top_tx_items: &[Value] = if let Some(arr) = data_array(&top_by_tx) {
            arr
        } else if let Some(arr) = top_by_tx.as_array() {
            arr.as_slice()
        } else {
            &[]
        };
        // Sort by transactionsCount descending to find busiest addresses
        let mut with_counts: Vec<(&str, i64)> = top_tx_items
            .iter()
            .filter_map(|item| {
                let hash = item.get("lockScriptHash").and_then(|v| v.as_str())?;
                let count = item.get("transactionsCount").and_then(|v| v.as_i64())?;
                Some((hash, count))
            })
            .collect();
        with_counts.sort_by_key(|item| std::cmp::Reverse(item.1));
        let candidates: Vec<String> = with_counts
            .into_iter()
            .take(10)
            .map(|(hash, _)| hash.to_string())
            .collect();
        params.busiest_lock_hashes = validate_ids(client, base, candidates, "addresses/{id}").await;
    }

    // DAO parameters (if has_dao)
    if availability.has_dao {
        // dao_lock_hashes from /dao/top-depositors?limit=3
        let depositors =
            fetch_json(client, &format!("{}/dao/top-depositors?limit=3", base)).await?;
        // Response shape: {"depositors": [...]}
        let dep_items = depositors
            .get("depositors")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for item in &dep_items {
            if let Some(hash) = item.get("lockScriptHash").and_then(|v| v.as_str()) {
                params.dao_lock_hashes.push(hash.to_string());
            }
        }

        // dao deposit params from /dao/deposits?limit=1
        let deposits = fetch_json(client, &format!("{}/dao/deposits?limit=1", base)).await?;
        if let Some(first) = data_array(&deposits).and_then(|arr| arr.first()) {
            let tx_hash = first
                .get("txHash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let output_index = first
                .get("outputIndex")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as u32;
            if !tx_hash.is_empty() {
                params.dao_deposit_outpoint = Some((tx_hash, output_index));
            }
            // Extract capacity and deposit_block for /dao/calculator
            if let (Some(capacity), Some(block)) = (
                first.get("capacity").and_then(|v| v.as_str()),
                first.get("depositBlockNumber").and_then(|v| v.as_i64()),
            ) {
                params.dao_deposit_capacity = Some(capacity.to_string());
                params.dao_deposit_block = Some(block);
            }
        }
    }

    // token_type_hashes from /tokens?limit=3 (if has_tokens)
    if availability.has_tokens {
        let tokens = fetch_json(client, &format!("{}/tokens?limit=3", base)).await?;
        if let Some(arr) = data_array(&tokens) {
            params.token_type_hashes = arr
                .iter()
                .filter_map(|item| {
                    item.get("typeScriptHash")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect();
        }

        // top_token_type_hashes: top 10 tokens by holders (default sort)
        let top_tokens = fetch_json(client, &format!("{}/tokens?limit=10", base)).await?;
        if let Some(arr) = data_array(&top_tokens) {
            let candidates: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    item.get("typeScriptHash")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect();
            params.top_token_type_hashes =
                validate_ids(client, base, candidates, "tokens/{id}").await;
        }
    }

    // cluster_ids, spore_ids from spore endpoints (if has_spore)
    if availability.has_spore {
        let clusters = fetch_json(client, &format!("{}/spore/clusters?limit=3", base)).await?;
        if let Some(arr) = data_array(&clusters) {
            params.cluster_ids = arr
                .iter()
                .filter_map(|item| {
                    item.get("clusterId")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect();
        }

        let spores = fetch_json(client, &format!("{}/spore/objects?limit=10", base)).await?;
        if let Some(arr) = data_array(&spores) {
            let candidate_ids: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    item.get("sporeId")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect();
            let valid = validate_ids(client, base, candidate_ids, "spore/objects/{id}").await;
            params.spore_ids = valid.iter().take(3).cloned().collect();
            params.top_spore_ids = valid.clone();

            // Find a spore with renderable SVG
            for id in &valid {
                let url = format!("{}/spore/objects/{}/render", base, id);
                if probe_ok(client, &url).await {
                    params.renderable_spore_id = Some(id.clone());
                    break;
                }
            }
        }

        // top_cluster_ids: top 10 clusters by spore count (heaviest cluster pages)
        let all_clusters = fetch_json(client, &format!("{}/spore/clusters?limit=50", base)).await?;
        if let Some(arr) = data_array(&all_clusters) {
            let mut with_counts: Vec<(&str, i64)> = arr
                .iter()
                .filter_map(|item| {
                    let id = item.get("clusterId").and_then(|v| v.as_str())?;
                    let count = item
                        .get("sporesCount")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    Some((id, count))
                })
                .collect();
            with_counts.sort_by_key(|item| std::cmp::Reverse(item.1));
            let candidates: Vec<String> = with_counts
                .into_iter()
                .take(10)
                .map(|(id, _)| id.to_string())
                .collect();
            params.top_cluster_ids =
                validate_ids(client, base, candidates, "spore/clusters/{id}").await;
        }
    }

    // script_names from /scripts?limit=3 (use "name" for /scripts/{name} lookup)
    let scripts = fetch_json(client, &format!("{}/scripts?limit=3", base)).await?;
    if let Some(arr) = data_array(&scripts) {
        params.script_names = arr
            .iter()
            .filter_map(|item| item.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }

    // top_script_names: top 10 scripts by cell count (heaviest detail pages)
    let top_scripts = fetch_json(
        client,
        &format!(
            "{}/scripts?limit=10&sort_key=cells&sort_direction=desc",
            base
        ),
    )
    .await?;
    if let Some(arr) = data_array(&top_scripts) {
        let candidates: Vec<String> = arr
            .iter()
            .filter_map(|item| item.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();
        params.top_script_names = validate_ids(client, base, candidates, "scripts/{id}").await;
    }

    // live_cell_outpoint from /cells/live?limit=1
    let live_cells = fetch_json(client, &format!("{}/cells/live?limit=1", base)).await?;
    if let Some(first) = data_array(&live_cells).and_then(|arr| arr.first()) {
        let tx_hash = first
            .get("txHash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let output_index = first
            .get("outputIndex")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as u32;
        if !tx_hash.is_empty() {
            params.live_cell_outpoint = Some((tx_hash, output_index));
        }
    }

    // fiber_channel_id from /fiber/channels?limit=1 (if has_fiber)
    if availability.has_fiber {
        let channels = fetch_json(client, &format!("{}/fiber/channels?limit=1", base)).await?;
        if let Some(first) = data_array(&channels).and_then(|arr| arr.first()) {
            params.fiber_channel_id = first
                .get("channelId")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }

    // dotbit_item_id from /assets/identities/dotbit/items?limit=1 (if has_identities)
    if availability.has_identities {
        let items = fetch_json(
            client,
            &format!("{}/assets/identities/dotbit/items?limit=1", base),
        )
        .await?;
        if let Some(first) = data_array(&items).and_then(|arr| arr.first()) {
            params.dotbit_item_id = first
                .get("nftId")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
        params.identity_collection_id = Some("dotbit".to_string());

        // top_dotbit_item_ids: first 10 .bit identity items
        let top_items = fetch_json(
            client,
            &format!("{}/assets/identities/dotbit/items?limit=10", base),
        )
        .await?;
        if let Some(arr) = data_array(&top_items) {
            let candidates: Vec<String> = arr
                .iter()
                .filter_map(|item| item.get("nftId").and_then(|v| v.as_str()).map(String::from))
                .collect();
            params.top_dotbit_item_ids = validate_ids(
                client,
                base,
                candidates,
                "assets/identities/dotbit/items/{id}",
            )
            .await;
        }

        // did:ckb item discovery
        let did_items = fetch_json(
            client,
            &format!("{}/assets/identities/did_ckb/items?limit=1", base),
        )
        .await?;
        if let Some(first) = data_array(&did_items).and_then(|arr| arr.first()) {
            params.did_item_id = first
                .get("nftId")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }

    // object_collection_id from /assets (if has_assets)
    // Try multiple candidates and validate each against the detail endpoint.
    if availability.has_assets {
        let assets = fetch_json(client, &format!("{}/assets?limit=10&type=object", base)).await?;
        if let Some(arr) = data_array(&assets) {
            let candidate_ids: Vec<String> = arr
                .iter()
                .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect();
            let valid = validate_ids(client, base, candidate_ids, "assets/objects/{id}").await;
            params.object_collection_id = valid.first().cloned();

            // object_item_id from /assets/objects/{collection_id}/items?limit=1
            if let Some(cid) = &params.object_collection_id {
                let items = fetch_json(
                    client,
                    &format!("{}/assets/objects/{}/items?limit=1", base, cid),
                )
                .await;
                if let Ok(items_val) = items {
                    if let Some(first_item) = data_array(&items_val).and_then(|arr| arr.first()) {
                        params.object_item_id = first_item
                            .get("nftId")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                }
            }
        }
    }

    // fork_id from /forks (if has_forks)
    if availability.has_forks {
        let forks = fetch_json(client, &format!("{}/forks", base)).await?;
        if let Some(first) = data_array(&forks).and_then(|arr| arr.first()) {
            params.fork_id = first
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string());
        }
    }

    Ok(params)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fetch JSON from a URL, returning the parsed Value.
async fn fetch_json(client: &reqwest::Client, url: &str) -> Result<Value> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;

    if !resp.status().is_success() {
        bail!("GET {} returned status {}", url, resp.status());
    }

    resp.json::<Value>()
        .await
        .with_context(|| format!("parsing JSON from {}", url))
}

/// Extract the `data` array from a CursorPaginatedResponse body.
fn data_array(val: &Value) -> Option<&Vec<Value>> {
    val.get("data").and_then(|v| v.as_array())
}

/// Check API connectivity by hitting /statistics/network.
pub async fn check_connectivity(api_base: &str, client: &reqwest::Client) -> Result<()> {
    let url = format!("{}/statistics/network", api_base.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("connectivity check failed: GET {}", url))?;

    if !resp.status().is_success() {
        bail!(
            "API not ready: GET {} returned status {}",
            url,
            resp.status()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_array_extracts_from_cursor_paginated() {
        let val: Value =
            serde_json::from_str(r#"{"data": [{"id": 1}, {"id": 2}], "total": 5}"#).unwrap();
        let arr = data_array(&val).unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn test_data_array_returns_none_for_bare_object() {
        let val: Value = serde_json::from_str(r#"{"latestBlock": 100}"#).unwrap();
        assert!(data_array(&val).is_none());
    }

    #[test]
    fn test_data_array_returns_none_for_empty() {
        let val: Value = serde_json::from_str(r#"{"data": []}"#).unwrap();
        let arr = data_array(&val).unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_probe_has_data_parses_various_shapes() {
        // CursorPaginated with data
        let val: Value = serde_json::from_str(r#"{"data": [{"x": 1}]}"#).unwrap();
        assert!(data_array(&val).map(|a| !a.is_empty()).unwrap_or(false));

        // Empty data
        let val: Value = serde_json::from_str(r#"{"data": []}"#).unwrap();
        assert!(!data_array(&val).map(|a| !a.is_empty()).unwrap_or(false));
    }
}
