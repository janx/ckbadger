use anyhow::Result;

#[derive(Clone, Default)]
pub struct DbPool;

pub async fn create_pool(_database_url: &str) -> Result<DbPool> {
    Ok(DbPool)
}
