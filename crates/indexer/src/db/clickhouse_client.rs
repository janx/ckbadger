#![allow(dead_code)]

//! ClickHouse client wrapper with connection management and query helpers.

use std::time::Duration;

use anyhow::{Context, Result};
use clickhouse::{insert::Insert, inserter::Inserter, Client, Row};
use serde::{de::DeserializeOwned, Serialize};

const DEFAULT_MAX_ROWS: u64 = 500_000;
const DEFAULT_MAX_BYTES: u64 = 50_000_000;
const DEFAULT_PERIOD_SECS: u64 = 15;

#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub max_inserter_rows: u64,
    pub max_inserter_bytes: u64,
    pub inserter_period_secs: u64,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_string(),
            database: "ckbadger".to_string(),
            user: None,
            password: None,
            max_inserter_rows: DEFAULT_MAX_ROWS,
            max_inserter_bytes: DEFAULT_MAX_BYTES,
            inserter_period_secs: DEFAULT_PERIOD_SECS,
        }
    }
}

impl ClickHouseConfig {
    pub fn new(url: impl Into<String>, database: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            database: database.into(),
            ..Default::default()
        }
    }

    pub fn with_credentials(mut self, user: impl Into<String>, password: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self.password = Some(password.into());
        self
    }

    pub fn with_inserter_limits(mut self, max_rows: u64, max_bytes: u64) -> Self {
        self.max_inserter_rows = max_rows;
        self.max_inserter_bytes = max_bytes;
        self
    }

    pub fn from_env() -> Result<Self> {
        let url = std::env::var("CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://localhost:8123".to_string());
        let database = std::env::var("CLICKHOUSE_DATABASE")
            .unwrap_or_else(|_| "ckbadger".to_string());
        let user = std::env::var("CLICKHOUSE_USER").ok();
        let password = std::env::var("CLICKHOUSE_PASSWORD").ok();

        Ok(Self {
            url,
            database,
            user,
            password,
            ..Default::default()
        })
    }
}

#[derive(Clone)]
pub struct ClickHouseClient {
    client: Client,
    config: ClickHouseConfig,
}

impl ClickHouseClient {
    pub fn new(config: ClickHouseConfig) -> Self {
        let mut client = Client::default()
            .with_url(&config.url)
            .with_database(&config.database);

        if let Some(ref user) = config.user {
            client = client.with_user(user);
        }
        if let Some(ref password) = config.password {
            client = client.with_password(password);
        }

        Self { client, config }
    }

    pub fn from_env() -> Result<Self> {
        let config = ClickHouseConfig::from_env()?;
        Ok(Self::new(config))
    }

    pub fn config(&self) -> &ClickHouseConfig {
        &self.config
    }

    pub fn inner(&self) -> &Client {
        &self.client
    }

    pub async fn query_one<T>(&self, query: &str) -> Result<Option<T>>
    where
        T: Row + DeserializeOwned,
    {
        self.client
            .query(query)
            .fetch_one::<T>()
            .await
            .map(Some)
            .or_else(|e| {
                if e.to_string().contains("not enough data") {
                    Ok(None)
                } else {
                    Err(e)
                }
            })
            .context("Failed to fetch row")
    }

    pub async fn query_all<T>(&self, query: &str) -> Result<Vec<T>>
    where
        T: Row + DeserializeOwned,
    {
        self.client
            .query(query)
            .fetch_all::<T>()
            .await
            .context("Failed to fetch rows")
    }

    pub async fn execute(&self, query: &str) -> Result<()> {
        self.client
            .query(query)
            .execute()
            .await
            .context("Failed to execute statement")
    }

    pub async fn insert<T>(&self, table: &str) -> Result<Insert<T>>
    where
        T: Row + Serialize,
    {
        self.client
            .insert(table)
            .await
            .context("Failed to create insert")
    }

    pub fn inserter<T>(&self, table: &str) -> Result<Inserter<T>>
    where
        T: Row + Serialize,
    {
        let inserter = self
            .client
            .inserter(table)
            .map_err(|e| anyhow::anyhow!("Failed to create inserter: {}", e))?
            .with_max_rows(self.config.max_inserter_rows)
            .with_max_bytes(self.config.max_inserter_bytes)
            .with_period(Some(Duration::from_secs(self.config.inserter_period_secs)));

        Ok(inserter)
    }

    pub fn inserter_with_limits<T>(
        &self,
        table: &str,
        max_rows: u64,
        max_bytes: u64,
    ) -> Result<Inserter<T>>
    where
        T: Row + Serialize,
    {
        let inserter = self
            .client
            .inserter(table)
            .map_err(|e| anyhow::anyhow!("Failed to create inserter: {}", e))?
            .with_max_rows(max_rows)
            .with_max_bytes(max_bytes)
            .with_period(Some(Duration::from_secs(self.config.inserter_period_secs)));

        Ok(inserter)
    }

    pub async fn ping(&self) -> Result<()> {
        self.execute("SELECT 1").await
    }
}
