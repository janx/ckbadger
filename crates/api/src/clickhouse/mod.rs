/// ClickHouse query layer for the API crate.
///
/// This module provides infrastructure for querying ClickHouse:
/// - Connection pooling and health checks
/// - Cursor-based pagination helpers
/// - Query building utilities (hex/unhex, WHERE clauses)
///
/// # Architecture
///
/// The ClickHouse query layer is designed to coexist with the existing PostgreSQL
/// infrastructure during the migration period. It provides:
///
/// - **Connection Management**: Reusable client with connection pooling
/// - **Pagination**: Cursor encoding/decoding compatible with existing API format
/// - **Query Helpers**: Utilities for hash conversion and common query patterns
///
/// # Usage
///
/// ```no_run
/// use ckbadger_api::clickhouse::{ClickHouseClient, encode_cursor, hex_hash};
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = ClickHouseClient::new("http://localhost:8123/ckbadger")?;
/// client.health_check().await?;
///
/// // Use cursor pagination
/// let cursor = encode_cursor(12345, 0);
///
/// // Use hex conversion in queries
/// let query = format!("SELECT {} FROM blocks", hex_hash("hash"));
/// # Ok(())
/// # }
/// ```
pub mod connection;
pub mod pagination;
pub mod query;

pub use connection::ClickHouseClient;
pub use pagination::{decode_cursor, decode_cursor_single, encode_cursor, encode_cursor_single};
pub use query::{build_where_block_range, build_where_hash, hex_hash, unhex_hash};
