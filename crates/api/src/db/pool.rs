use anyhow::Result;
use ckbadger_common::{ClickHouseClient, ClickHouseConfig};

pub type DbPool = ClickHouseClient;

pub async fn create_pool(_database_url: &str) -> Result<DbPool> {
    let config = ClickHouseConfig::from_env()?;
    let client = ClickHouseClient::new(config);
    Ok(client)
}
