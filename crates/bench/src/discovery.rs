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
    pub capabilities_route_count: Option<usize>,
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
    println!(
        "  Route count: {}",
        discovery
            .capabilities_route_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
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

async fn discover_capabilities(frontend_url: &str, client: &reqwest::Client) -> Option<usize> {
    let url = format!("{}/capabilities", frontend_url.trim_end_matches('/'));
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "  [warn] capabilities endpoint unreachable ({}): {}",
                url, e
            );
            return None;
        }
    };

    if !resp.status().is_success() {
        eprintln!("  [warn] capabilities returned status {}", resp.status());
        return None;
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  [warn] capabilities body parse failed: {}", e);
            return None;
        }
    };

    match parse_capabilities_route_count(&body) {
        Ok(count) => Some(count),
        Err(error) => {
            eprintln!("  [warn] capabilities shape invalid: {error}");
            None
        }
    }
}

fn parse_capabilities_route_count(body: &Value) -> Result<usize> {
    let routes = body
        .get("routes")
        .context("/capabilities missing routes object")?;
    let md = routes
        .get("markdown")
        .and_then(Value::as_array)
        .context("/capabilities missing routes.markdown array")?;
    let raw = routes
        .get("raw")
        .and_then(Value::as_array)
        .context("/capabilities missing routes.raw array")?;
    md.len()
        .checked_add(raw.len())
        .context("/capabilities route count overflow")
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
    params.sync_tip = required_u64(&network_stats, "latestBlock", "/statistics/network")?;

    // latest_block_number, latest_block_hash from /blocks?limit=1
    let blocks = fetch_json(client, &format!("{}/blocks?limit=1", base)).await?;
    let first_block = required_data_array(&blocks, "/blocks")?
        .first()
        .context("/blocks returned no tip row after /statistics/network reported a tip")?;
    params.latest_block_number = required_u64(first_block, "number", "/blocks.data[0]")?;
    params.latest_block_hash = required_string(first_block, "hash", "/blocks.data[0]")?.to_string();

    // mid_block_number
    params.mid_block_number = params.latest_block_number / 2;

    // tx_hashes from /transactions?limit=5
    let txs = fetch_json(client, &format!("{}/transactions?limit=5", base)).await?;
    {
        let arr = required_data_array(&txs, "/transactions")?;
        params.tx_hashes = required_strings(arr, "hash", "/transactions.data")?;
    }

    // top_addresses, top_lock_hashes from /addresses/top?limit=5
    // This endpoint returns a bare Vec<TopAddressResponse>, not CursorPaginated.
    let top = fetch_json(client, &format!("{}/addresses/top?limit=5", base)).await?;
    let top_items = required_bare_array(&top, "/addresses/top")?;
    for (index, item) in top_items.iter().enumerate() {
        params.top_lock_hashes.push(
            required_string(item, "lockScriptHash", &format!("/addresses/top[{index}]"))?
                .to_string(),
        );
        // Address text is genuinely optional for a lock script not encoded by this network.
        if let Some(addr) = item.get("address").and_then(|v| v.as_str()) {
            params.top_addresses.push(addr.to_string());
        }
    }

    // busiest_lock_hashes: top 10 addresses by transaction count
    {
        let top_by_tx = fetch_json(client, &format!("{}/addresses/top?limit=50", base)).await?;
        let top_tx_items = required_bare_array(&top_by_tx, "/addresses/top")?;
        // Sort by transactionsCount descending to find busiest addresses
        let mut with_counts: Vec<(&str, u64)> = top_tx_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let context = format!("/addresses/top[{index}]");
                Ok((
                    required_string(item, "lockScriptHash", &context)?,
                    required_u64(item, "transactionsCount", &context)?,
                ))
            })
            .collect::<Result<_>>()?;
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
        let dep_items = required_array_field(&depositors, "depositors", "/dao/top-depositors")?;
        params.dao_lock_hashes = required_strings(
            dep_items,
            "lockScriptHash",
            "/dao/top-depositors.depositors",
        )?;

        // dao deposit params from /dao/deposits?limit=1
        let deposits = fetch_json(client, &format!("{}/dao/deposits?limit=1", base)).await?;
        if let Some(first) = required_data_array(&deposits, "/dao/deposits")?.first() {
            let tx_hash = required_string(first, "txHash", "/dao/deposits.data[0]")?.to_string();
            let output_index = required_u32(first, "outputIndex", "/dao/deposits.data[0]")?;
            params.dao_deposit_outpoint = Some((tx_hash, output_index));
            // Extract capacity and deposit_block for /dao/calculator
            params.dao_deposit_capacity =
                Some(required_string(first, "capacity", "/dao/deposits.data[0]")?.to_string());
            params.dao_deposit_block = Some(
                i64::try_from(required_u64(
                    first,
                    "depositBlockNumber",
                    "/dao/deposits.data[0]",
                )?)
                .context("DAO deposit block exceeds i64")?,
            );
        }
    }

    // token_type_hashes from /tokens?limit=3 (if has_tokens)
    if availability.has_tokens {
        let tokens = fetch_json(client, &format!("{}/tokens?limit=3", base)).await?;
        {
            let arr = required_data_array(&tokens, "/tokens")?;
            params.token_type_hashes = required_strings(arr, "typeScriptHash", "/tokens.data")?;
        }

        // top_token_type_hashes: top 10 tokens by holders (default sort)
        let top_tokens = fetch_json(client, &format!("{}/tokens?limit=10", base)).await?;
        {
            let arr = required_data_array(&top_tokens, "/tokens")?;
            let candidates = required_strings(arr, "typeScriptHash", "/tokens.data")?;
            params.top_token_type_hashes =
                validate_ids(client, base, candidates, "tokens/{id}").await;
        }
    }

    // cluster_ids, spore_ids from spore endpoints (if has_spore)
    if availability.has_spore {
        let clusters = fetch_json(client, &format!("{}/spore/clusters?limit=3", base)).await?;
        {
            let arr = required_data_array(&clusters, "/spore/clusters")?;
            params.cluster_ids = required_strings(arr, "clusterId", "/spore/clusters.data")?;
        }

        let spores = fetch_json(client, &format!("{}/spore/objects?limit=10", base)).await?;
        {
            let arr = required_data_array(&spores, "/spore/objects")?;
            let candidate_ids = required_strings(arr, "sporeId", "/spore/objects.data")?;
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
        {
            let arr = required_data_array(&all_clusters, "/spore/clusters")?;
            let mut with_counts: Vec<(&str, u64)> = arr
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let context = format!("/spore/clusters.data[{index}]");
                    Ok((
                        required_string(item, "clusterId", &context)?,
                        required_u64(item, "sporesCount", &context)?,
                    ))
                })
                .collect::<Result<_>>()?;
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
    {
        let arr = required_data_array(&scripts, "/scripts")?;
        params.script_names = required_strings(arr, "name", "/scripts.data")?;
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
    {
        let arr = required_data_array(&top_scripts, "/scripts")?;
        let candidates = required_strings(arr, "name", "/scripts.data")?;
        params.top_script_names = validate_ids(client, base, candidates, "scripts/{id}").await;
    }

    // live_cell_outpoint from the first live cell of the top address
    if let Some(lock_hash) = params.top_lock_hashes.first() {
        let live_cells = fetch_json(client, &live_cell_discovery_url(base, lock_hash)).await?;
        if let Some(first) = required_data_array(&live_cells, "/cells/live")?.first() {
            let tx_hash = required_string(first, "txHash", "/cells/live.data[0]")?.to_string();
            let output_index = required_u32(first, "outputIndex", "/cells/live.data[0]")?;
            params.live_cell_outpoint = Some((tx_hash, output_index));
        }
    }

    // fiber_channel_id from /fiber/channels?limit=1 (if has_fiber)
    if availability.has_fiber {
        let channels = fetch_json(client, &format!("{}/fiber/channels?limit=1", base)).await?;
        if let Some(first) = required_data_array(&channels, "/fiber/channels")?.first() {
            params.fiber_channel_id =
                Some(required_string(first, "channelId", "/fiber/channels.data[0]")?.to_string());
        }
    }

    // dotbit_item_id from /assets/identities/dotbit/items?limit=1 (if has_identities)
    if availability.has_identities {
        let items = fetch_json(
            client,
            &format!("{}/assets/identities/dotbit/items?limit=1", base),
        )
        .await?;
        if let Some(first) = required_data_array(&items, "/assets/identities/dotbit/items")?.first()
        {
            params.dotbit_item_id = Some(
                required_string(first, "nftId", "/assets/identities/dotbit/items.data[0]")?
                    .to_string(),
            );
        }
        params.identity_collection_id = Some("dotbit".to_string());

        // top_dotbit_item_ids: first 10 .bit identity items
        let top_items = fetch_json(
            client,
            &format!("{}/assets/identities/dotbit/items?limit=10", base),
        )
        .await?;
        {
            let arr = required_data_array(&top_items, "/assets/identities/dotbit/items")?;
            let candidates =
                required_strings(arr, "nftId", "/assets/identities/dotbit/items.data")?;
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
        if let Some(first) =
            required_data_array(&did_items, "/assets/identities/did_ckb/items")?.first()
        {
            params.did_item_id = Some(
                required_string(first, "nftId", "/assets/identities/did_ckb/items.data[0]")?
                    .to_string(),
            );
        }

        // `.cell` item discovery. The collection is absent on a network that
        // has no `.cell` deployment, so a missing id simply skips its entries.
        let dotcell_items = fetch_json(
            client,
            &format!("{}/assets/identities/dotcell/items?limit=1", base),
        )
        .await?;
        if let Some(first) =
            required_data_array(&dotcell_items, "/assets/identities/dotcell/items")?.first()
        {
            params.dotcell_item_id = Some(
                required_string(first, "nftId", "/assets/identities/dotcell/items.data[0]")?
                    .to_string(),
            );
        }
    }

    // object_collection_id from /assets (if has_assets)
    // Try multiple candidates and validate each against the detail endpoint.
    if availability.has_assets {
        let assets = fetch_json(client, &format!("{}/assets?limit=10&type=object", base)).await?;
        {
            let arr = required_data_array(&assets, "/assets")?;
            let candidate_ids = required_strings(arr, "id", "/assets.data")?;
            let valid = validate_ids(client, base, candidate_ids, "assets/objects/{id}").await;
            params.object_collection_id = valid.first().cloned();

            // object_item_id from /assets/objects/{collection_id}/items?limit=1
            if let Some(cid) = &params.object_collection_id {
                let items_val = fetch_json(
                    client,
                    &format!("{}/assets/objects/{}/items?limit=1", base, cid),
                )
                .await?;
                if let Some(first_item) =
                    required_data_array(&items_val, "/assets/objects/items")?.first()
                {
                    params.object_item_id = Some(
                        required_string(first_item, "nftId", "/assets/objects/items.data[0]")?
                            .to_string(),
                    );
                }
            }
        }
    }

    // fork_id from /forks (if has_forks)
    if availability.has_forks {
        let forks = fetch_json(client, &format!("{}/forks", base)).await?;
        if let Some(first) = required_data_array(&forks, "/forks")?.first() {
            params.fork_id = Some(required_u64(first, "id", "/forks.data[0]")?.to_string());
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

fn required_data_array<'a>(value: &'a Value, endpoint: &str) -> Result<&'a [Value]> {
    required_array_field(value, "data", endpoint)
}

fn required_array_field<'a>(value: &'a Value, field: &str, endpoint: &str) -> Result<&'a [Value]> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .with_context(|| format!("{endpoint}: required {field} array missing or invalid"))
}

fn required_bare_array<'a>(value: &'a Value, endpoint: &str) -> Result<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .with_context(|| format!("{endpoint}: required top-level array missing or invalid"))
}

fn required_u64(value: &Value, field: &str, context: &str) -> Result<u64> {
    value.get(field).and_then(Value::as_u64).with_context(|| {
        format!("{context}: required nonnegative {field} integer missing or invalid")
    })
}

fn required_u32(value: &Value, field: &str, context: &str) -> Result<u32> {
    let number = required_u64(value, field, context)?;
    u32::try_from(number).with_context(|| format!("{context}: {field} exceeds u32: {number}"))
}

fn required_string<'a>(value: &'a Value, field: &str, context: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("{context}: required nonempty {field} string missing or invalid"))
}

fn required_strings(rows: &[Value], field: &str, context: &str) -> Result<Vec<String>> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            required_string(row, field, &format!("{context}[{index}]")).map(str::to_string)
        })
        .collect()
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

/// `/cells/live` lists only through an index, so discovery filters by a lock hash.
fn live_cell_discovery_url(base: &str, lock_hash: &str) -> String {
    format!("{base}/cells/live?lock_script_hash={lock_hash}&limit=1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_cell_discovery_filters_by_a_lock_hash() {
        // `/cells/live` answers 400 without a lock or type script hash filter.
        assert_eq!(
            live_cell_discovery_url("http://h/api/v1", "0xab"),
            "http://h/api/v1/cells/live?lock_script_hash=0xab&limit=1"
        );
    }

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
    fn required_discovery_fields_reject_missing_invalid_and_narrowing_values() {
        assert!(
            required_u64(&serde_json::json!({}), "latestBlock", "/statistics/network")
                .unwrap_err()
                .to_string()
                .contains("latestBlock")
        );
        assert!(required_u64(
            &serde_json::json!({"latestBlock": -1}),
            "latestBlock",
            "/statistics/network"
        )
        .is_err());
        assert!(required_data_array(&serde_json::json!({"data": null}), "/blocks").is_err());
        assert!(required_bare_array(&serde_json::json!({"data": []}), "/addresses/top").is_err());
        assert!(required_u32(
            &serde_json::json!({"outputIndex": 4294967296u64}),
            "outputIndex",
            "/cells/live.data[0]"
        )
        .is_err());
        assert!(required_string(
            &serde_json::json!({"txHash": ""}),
            "txHash",
            "/cells/live.data[0]"
        )
        .is_err());
        assert!(
            required_data_array(&serde_json::json!({"data": []}), "/blocks")
                .unwrap()
                .is_empty()
        );
        assert!(required_strings(
            &[serde_json::json!({"hash":"0xab"}), serde_json::json!({})],
            "hash",
            "/transactions.data"
        )
        .unwrap_err()
        .to_string()
        .contains("[1]"));
    }

    #[test]
    fn capabilities_unavailable_shape_cannot_report_zero_routes() {
        assert!(
            parse_capabilities_route_count(&serde_json::json!({"routes":{"markdown":[]}})).is_err()
        );
        assert_eq!(
            parse_capabilities_route_count(&serde_json::json!({"routes":{"markdown":[],"raw":[]}}))
                .unwrap(),
            0
        );
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
