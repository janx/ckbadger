use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
}

impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut conn = self.conn.clone();
        match conn.get::<_, Option<String>>(key).await {
            Ok(Some(data)) => match serde_json::from_str(&data) {
                Ok(value) => {
                    debug!(key = %key, "Cache hit");
                    Some(value)
                }
                Err(e) => {
                    warn!(key = %key, error = %e, "Failed to deserialize cached value");
                    None
                }
            },
            Ok(None) => {
                debug!(key = %key, "Cache miss");
                None
            }
            Err(e) => {
                warn!(key = %key, error = %e, "Redis get error");
                None
            }
        }
    }

    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) {
        let mut conn = self.conn.clone();
        match serde_json::to_string(value) {
            Ok(data) => {
                let result: Result<(), _> = conn.set_ex(key, data, ttl.as_secs()).await;
                if let Err(e) = result {
                    warn!(key = %key, error = %e, "Redis set error");
                } else {
                    debug!(key = %key, ttl_secs = ttl.as_secs(), "Cache set");
                }
            }
            Err(e) => {
                warn!(key = %key, error = %e, "Failed to serialize value for cache");
            }
        }
    }

    pub async fn delete(&self, key: &str) {
        let mut conn = self.conn.clone();
        let result: Result<(), _> = conn.del(key).await;
        if let Err(e) = result {
            warn!(key = %key, error = %e, "Redis delete error");
        }
    }
}
