use anyhow::Result;
use clickhouse::Client;

/// ClickHouse client wrapper with connection pooling support.
///
/// The underlying `clickhouse::Client` handles connection pooling internally.
/// Clients should be cloned and reused across the application for optimal performance.
///
/// # Example
///
/// ```no_run
/// use ckbadger_indexer::db::ClickHouseClient;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = ClickHouseClient::new("http://ckbadger:changeme@localhost:8123/ckbadger")?;
/// client.health_check().await?;
/// let version = client.get_version().await?;
/// println!("ClickHouse version: {}", version);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ClickHouseClient {
    client: Client,
}

impl ClickHouseClient {
    /// Create a new ClickHouse client from connection parameters.
    ///
    /// # Arguments
    ///
    /// * `url` - Base URL in format: `http://host:port`
    /// * `user` - Username for authentication
    /// * `password` - Password for authentication
    /// * `database` - Database name
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use ckbadger_indexer::db::ClickHouseClient;
    /// let client = ClickHouseClient::new("http://localhost:8123", "ckbadger", "changeme", "ckbadger")?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new(url: &str, user: &str, password: &str, database: &str) -> Result<Self> {
        let client = Client::default()
            .with_url(url)
            .with_user(user)
            .with_password(password)
            .with_database(database);
        Ok(Self { client })
    }

    /// Verify connectivity to ClickHouse by executing a simple query.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the query cannot be executed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use ckbadger_indexer::db::ClickHouseClient;
    /// # async fn example() -> anyhow::Result<()> {
    /// # let client = ClickHouseClient::new("http://localhost:8123")?;
    /// client.health_check().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn health_check(&self) -> Result<()> {
        let result: u8 = self.client.query("SELECT 1").fetch_one().await?;
        if result != 1 {
            anyhow::bail!("Health check failed: expected 1, got {}", result);
        }
        Ok(())
    }

    /// Query the ClickHouse server version.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use ckbadger_indexer::db::ClickHouseClient;
    /// # async fn example() -> anyhow::Result<()> {
    /// # let client = ClickHouseClient::new("http://localhost:8123")?;
    /// let version = client.get_version().await?;
    /// println!("ClickHouse version: {}", version);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_version(&self) -> Result<String> {
        let version: String = self.client.query("SELECT version()").fetch_one().await?;
        Ok(version)
    }

    /// Get a reference to the underlying ClickHouse client.
    ///
    /// This allows direct access to the client for advanced operations
    /// not covered by the wrapper methods.
    pub fn client(&self) -> &Client {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let result = ClickHouseClient::new("http://localhost:8123", "default", "", "default");
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_with_credentials() {
        let result = ClickHouseClient::new("http://localhost:8123", "user", "pass", "mydb");
        assert!(result.is_ok());
    }

    // Integration tests require a running ClickHouse instance
    // These are commented out but can be enabled for manual testing
    //
    // #[tokio::test]
    // async fn test_health_check() {
    //     let client = ClickHouseClient::new("http://localhost:8123/default").unwrap();
    //     let result = client.health_check().await;
    //     assert!(result.is_ok());
    // }
    //
    // #[tokio::test]
    // async fn test_get_version() {
    //     let client = ClickHouseClient::new("http://localhost:8123/default").unwrap();
    //     let version = client.get_version().await.unwrap();
    //     assert!(!version.is_empty());
    //     println!("ClickHouse version: {}", version);
    // }
}
