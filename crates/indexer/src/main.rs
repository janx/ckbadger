use anyhow::Result;

use crate::config::Config;
use crate::sync::Indexer;

mod cache;
mod config;
mod db;
mod parser;
mod rpc;
mod state;
mod sync;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::from_env()?;
    let indexer = Indexer::from_legacy_config(config).await?;
    indexer.run().await?;

    Ok(())
}
