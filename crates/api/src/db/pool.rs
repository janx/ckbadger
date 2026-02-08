use anyhow::Result;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use std::str::FromStr;

/// API statement timeout in milliseconds.
/// Prevents slow queries from blocking the connection pool.
pub const STATEMENT_TIMEOUT_MS: &str = "15000";

/// Maximum number of database connections for the API pool.
pub const MAX_CONNECTIONS: u32 = 32;

pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    let options = PgConnectOptions::from_str(database_url)?
        .statement_cache_capacity(256)
        .options([("statement_timeout", STATEMENT_TIMEOUT_MS)]);

    let pool = PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statement_timeout_value() {
        let ms: u64 = STATEMENT_TIMEOUT_MS.parse().unwrap();
        assert_eq!(ms, 15000);
    }

    #[test]
    fn test_max_connections_value() {
        assert_eq!(MAX_CONNECTIONS, 32);
    }

    #[test]
    fn test_pg_connect_options_from_valid_url() {
        let url = "postgres://user:pass@localhost:5432/testdb";
        let options = PgConnectOptions::from_str(url);
        assert!(options.is_ok(), "valid URL should parse successfully");
    }

    #[test]
    fn test_pg_connect_options_from_invalid_url() {
        let url = "not-a-valid-url";
        let options = PgConnectOptions::from_str(url);
        assert!(options.is_err(), "invalid URL should fail to parse");
    }
}
