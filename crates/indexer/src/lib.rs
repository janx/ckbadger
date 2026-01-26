pub mod cache;
pub mod config;
pub mod control_plane;
pub mod db;
pub mod jobs;
pub mod parser;
pub mod rebuild;
pub mod rpc;
pub mod sync;

pub use cache::CacheInvalidator;
pub use config::Config;
pub use control_plane::ControlPlaneClient;
pub use jobs::JobExecutor;
pub use rebuild::RebuildRunner;

/// Migrator for sqlx::test integration tests
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");
