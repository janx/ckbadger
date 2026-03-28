mod discovery;
#[allow(dead_code)]
mod endpoints;
#[allow(dead_code)]
mod metrics;
#[allow(dead_code)]
mod registry;
#[allow(dead_code)]
mod report;
#[allow(dead_code)]
mod runner;

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;

use crate::discovery::{check_connectivity, print_discovery, run_discovery};

#[derive(Parser)]
#[command(name = "ckbadger-bench", about = "API performance benchmark")]
struct Cli {
    /// API base URL
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    api_url: String,

    /// Frontend URL (for /capabilities)
    #[arg(long, default_value = "http://localhost:8100")]
    frontend_url: String,

    /// Requests per endpoint
    #[arg(long, default_value = "10")]
    iterations: u32,

    /// Concurrent requests
    #[arg(long, default_value = "1")]
    concurrency: u32,

    /// Warmup requests (not measured)
    #[arg(long, default_value = "2")]
    warmup: u32,

    /// Per-request timeout in milliseconds
    #[arg(long, default_value = "10000")]
    timeout_ms: u64,

    /// Filter by module name
    #[arg(long)]
    module: Option<String>,

    /// Filter by endpoint path template
    #[arg(long)]
    endpoint: Option<String>,

    /// Filter by risk tier (high, medium, low)
    #[arg(long)]
    risk: Option<String>,

    /// Output JSON instead of table
    #[arg(long)]
    json: bool,

    /// Save JSON output to file
    #[arg(long)]
    output: Option<String>,

    /// Compare against baseline JSON file
    #[arg(long)]
    compare: Option<String>,

    /// Print discovered params and exit
    #[arg(long)]
    discovery_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("ckbadger-bench: api_url={}", cli.api_url);

    // Build HTTP client with configured timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cli.timeout_ms))
        .build()
        .context("failed to build HTTP client")?;

    // Connectivity check
    println!("Checking API connectivity...");
    check_connectivity(&cli.api_url, &client).await?;
    println!("API is reachable.");

    // Run discovery
    println!("Running discovery...");
    let discovery = run_discovery(&cli.api_url, &cli.frontend_url, &client).await?;

    if cli.discovery_only {
        print_discovery(&discovery);
        return Ok(());
    }

    // Print summary and continue (runner will be wired in later)
    println!(
        "Discovery complete: {} routes, {} params discovered",
        discovery.capabilities_route_count,
        count_discovered_params(&discovery),
    );
    println!(
        "Data modules: tokens={} spore={} dao={} fiber={} identities={} assets={} mempool={} forks={}",
        discovery.availability.has_tokens,
        discovery.availability.has_spore,
        discovery.availability.has_dao,
        discovery.availability.has_fiber,
        discovery.availability.has_identities,
        discovery.availability.has_assets,
        discovery.availability.has_mempool,
        discovery.availability.has_forks,
    );

    Ok(())
}

/// Count how many parameter fields have been populated.
fn count_discovered_params(d: &discovery::Discovery) -> usize {
    let p = &d.params;
    let mut count = 0;
    if p.sync_tip > 0 {
        count += 1;
    }
    if p.latest_block_number > 0 {
        count += 1;
    }
    if !p.latest_block_hash.is_empty() {
        count += 1;
    }
    if p.mid_block_number > 0 {
        count += 1;
    }
    count += p.tx_hashes.len();
    if p.complex_tx_hash.is_some() {
        count += 1;
    }
    count += p.top_addresses.len();
    count += p.top_lock_hashes.len();
    count += p.dao_lock_hashes.len();
    if p.dao_deposit_outpoint.is_some() {
        count += 1;
    }
    count += p.token_type_hashes.len();
    count += p.cluster_ids.len();
    count += p.spore_ids.len();
    count += p.script_names.len();
    if p.live_cell_outpoint.is_some() {
        count += 1;
    }
    if p.fiber_channel_id.is_some() {
        count += 1;
    }
    if p.dotbit_item_id.is_some() {
        count += 1;
    }
    if p.identity_collection_id.is_some() {
        count += 1;
    }
    if p.object_collection_id.is_some() {
        count += 1;
    }
    if p.object_item_id.is_some() {
        count += 1;
    }
    if p.fork_id.is_some() {
        count += 1;
    }
    count
}
