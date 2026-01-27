use anyhow::Result;
use clickhouse::Client;

#[derive(Clone)]
pub struct ClickHouseClient {
    client: Client,
}

impl ClickHouseClient {
    pub fn new(url: &str, user: &str, password: &str, database: &str) -> Result<Self> {
        let client = Client::default()
            .with_url(url)
            .with_user(user)
            .with_password(password)
            .with_database(database);
        Ok(Self { client })
    }

    pub async fn health_check(&self) -> Result<()> {
        let result: u8 = self.client.query("SELECT 1").fetch_one().await?;
        if result != 1 {
            anyhow::bail!("Health check failed: expected 1, got {}", result);
        }
        Ok(())
    }

    pub async fn get_version(&self) -> Result<String> {
        let version: String = self.client.query("SELECT version()").fetch_one().await?;
        Ok(version)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    // Note: query_json and execute methods removed as they're not compatible
    // with the ClickHouse client library API. Use client() method to access
    // the underlying client for custom queries.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let result = ClickHouseClient::new("http://localhost:8123/ckbadger");
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_with_credentials() {
        let result = ClickHouseClient::new("http://user:pass@localhost:8123/ckbadger");
        assert!(result.is_ok());
    }
}
