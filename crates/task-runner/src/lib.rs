pub mod db;
pub mod executor;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");
