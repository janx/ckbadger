#[allow(dead_code)] // consumed by later tasks (vu, stage scheduler)
pub mod collector;
pub mod report;
#[allow(dead_code)] // consumed by later tasks (vu, stage scheduler)
pub mod scenario;
pub mod vu;

use anyhow::Result;
use clap::Args;

/// CLI arguments for the stress subcommand.
#[derive(Debug, Args)]
pub struct StressArgs {
    /// API base URL
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    pub api_url: String,
}

/// Entry point for the stress subcommand (not yet implemented).
pub async fn run_stress(_args: StressArgs) -> Result<()> {
    eprintln!("stress subcommand is not yet implemented");
    Ok(())
}
