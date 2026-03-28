#[allow(dead_code)]
mod metrics;
#[allow(dead_code)]
mod registry;

use anyhow::Result;
use clap::Parser;

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
    Ok(())
}
