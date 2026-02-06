use anyhow::Result;

use crate::config::Config;
use crate::db::DbPool;
use crate::sync::Indexer;

mod cache;
mod config;
mod db;
mod parser;
mod rpc;
mod sync;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::from_env()?;
    let pool = DbPool;
    let indexer = Indexer::new(config, pool).await?;
    indexer.run().await?;

    Ok(())
}
