use anyhow::{Context, Result};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::NoTls;

/// Configuration for COPY operations
#[derive(Debug, Clone)]
pub struct CopyConfig {
    /// Maximum number of connections in the pool
    pub max_copy_connections: usize,
    /// Number of rows to batch in a single COPY operation
    pub copy_batch_size: usize,
    /// Whether COPY operations are enabled (default: true in bulk sync)
    pub copy_enabled: bool,
}

impl Default for CopyConfig {
    fn default() -> Self {
        Self {
            max_copy_connections: 4,
            copy_batch_size: 50_000,
            copy_enabled: true,
        }
    }
}

/// Manager for dedicated tokio-postgres connections for COPY operations
///
/// This pool is separate from the sqlx pool to avoid contention between
/// COPY operations (bulk writes) and regular queries (reads/small writes).
#[derive(Debug)]
pub struct CopyPoolManager {
    pool: Pool,
    config: CopyConfig,
}

impl CopyPoolManager {
    /// Create a new CopyPoolManager from a database URL
    ///
    /// # Arguments
    /// * `database_url` - PostgreSQL connection string (e.g., "postgres://user:pass@host/db")
    /// * `config` - Configuration for COPY operations
    ///
    /// # Example
    /// ```no_run
    /// use ckbadger_indexer::db::copy_pool::{CopyPoolManager, CopyConfig};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let manager = CopyPoolManager::new(
    ///     "postgres://localhost/ckbadger",
    ///     CopyConfig::default()
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(database_url: &str, config: CopyConfig) -> Result<Self> {
        let pg_config = database_url
            .parse::<tokio_postgres::Config>()
            .context("Failed to parse database URL")?;

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let mgr = Manager::from_config(pg_config, NoTls, mgr_config);

        let pool = Pool::builder(mgr)
            .max_size(config.max_copy_connections)
            .build()
            .context("Failed to create connection pool")?;

        Ok(Self { pool, config })
    }

    /// Get a connection from the pool
    ///
    /// # Returns
    /// A pooled connection that can be used for COPY operations
    ///
    /// # Example
    /// ```no_run
    /// # use ckbadger_indexer::db::copy_pool::{CopyPoolManager, CopyConfig};
    /// # async fn example(manager: &CopyPoolManager) -> anyhow::Result<()> {
    /// let conn = manager.get_connection().await?;
    /// // Use conn for COPY operations
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_connection(&self) -> Result<deadpool_postgres::Object> {
        self.pool
            .get()
            .await
            .context("Failed to get connection from pool")
    }

    /// Get the copy batch size configuration
    pub fn copy_batch_size(&self) -> usize {
        self.config.copy_batch_size
    }

    /// Check if COPY operations are enabled
    pub fn is_copy_enabled(&self) -> bool {
        self.config.copy_enabled
    }

    /// Get pool status for monitoring
    pub fn pool_status(&self) -> PoolStatus {
        let status = self.pool.status();
        PoolStatus {
            size: status.size,
            available: status.available,
            max_size: status.max_size,
        }
    }
}

/// Pool status information for monitoring
#[derive(Debug, Clone)]
pub struct PoolStatus {
    /// Current number of connections in the pool
    pub size: usize,
    /// Number of available (idle) connections
    pub available: usize,
    /// Maximum pool size
    pub max_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = CopyConfig::default();
        assert_eq!(config.max_copy_connections, 4);
        assert_eq!(config.copy_batch_size, 50_000);
        assert!(config.copy_enabled);
    }

    #[test]
    fn test_config_custom() {
        let config = CopyConfig {
            max_copy_connections: 8,
            copy_batch_size: 100_000,
            copy_enabled: false,
        };
        assert_eq!(config.max_copy_connections, 8);
        assert_eq!(config.copy_batch_size, 100_000);
        assert!(!config.copy_enabled);
    }

    #[test]
    fn test_pool_rejects_invalid_url() {
        let result = CopyPoolManager::new("not-a-valid-url", CopyConfig::default());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse database URL"));
    }

    #[test]
    fn test_pool_rejects_malformed_url() {
        let result = CopyPoolManager::new("not://a/valid/postgres/url", CopyConfig::default());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse database URL"));
    }

    // Integration test - requires a running PostgreSQL instance
    // Run with: cargo test test_pool_creation_success -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_pool_creation_success() {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/ckbadger_test".to_string());

        let config = CopyConfig {
            max_copy_connections: 2,
            copy_batch_size: 1000,
            copy_enabled: true,
        };

        let manager = CopyPoolManager::new(&database_url, config.clone()).unwrap();

        // Verify configuration
        assert_eq!(manager.copy_batch_size(), 1000);
        assert!(manager.is_copy_enabled());

        // Verify pool status
        let status = manager.pool_status();
        assert_eq!(status.max_size, 2);
        assert_eq!(status.size, 0); // No connections created yet

        // Get a connection
        let conn = manager.get_connection().await.unwrap();
        drop(conn);

        // Verify connection was returned to pool
        let status = manager.pool_status();
        assert_eq!(status.size, 1);
        assert_eq!(status.available, 1);
    }

    #[tokio::test]
    #[ignore]
    async fn test_pool_concurrent_connections() {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/ckbadger_test".to_string());

        let manager = CopyPoolManager::new(
            &database_url,
            CopyConfig {
                max_copy_connections: 3,
                ..Default::default()
            },
        )
        .unwrap();

        // Get multiple connections concurrently
        let conn1 = manager.get_connection().await.unwrap();
        let conn2 = manager.get_connection().await.unwrap();
        let conn3 = manager.get_connection().await.unwrap();

        let status = manager.pool_status();
        assert_eq!(status.size, 3);
        assert_eq!(status.available, 0);

        // Return connections
        drop(conn1);
        drop(conn2);
        drop(conn3);

        let status = manager.pool_status();
        assert_eq!(status.available, 3);
    }
}
