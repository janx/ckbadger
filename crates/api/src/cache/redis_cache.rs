#[cfg(feature = "redis-cache")]
use redis::{aio::ConnectionManager, AsyncCommands, Client};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

#[cfg(feature = "redis-cache")]
#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
}

#[cfg(feature = "redis-cache")]
impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self, String> {
        let client = Client::open(redis_url).map_err(|e| format!("Redis client error: {}", e))?;

        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| format!("Redis connection error: {}", e))?;

        Ok(Self { conn })
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut conn = self.conn.clone();
        let value: Option<String> = conn.get(key).await.ok()?;
        value.and_then(|v| serde_json::from_str(&v).ok())
    }

    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) {
        let mut conn = self.conn.clone();
        if let Ok(json) = serde_json::to_string(value) {
            let _: Result<(), _> = conn.set_ex(key, json, ttl.as_secs()).await;
        }
    }

    pub async fn delete(&self, key: &str) {
        let mut conn = self.conn.clone();
        let _: Result<(), _> = conn.del(key).await;
    }

    pub async fn hgetall<T: DeserializeOwned>(&self, key: &str) -> Vec<T> {
        let mut conn = self.conn.clone();
        let values: Vec<String> = conn.hvals(key).await.unwrap_or_default();
        values
            .into_iter()
            .filter_map(|v| serde_json::from_str(&v).ok())
            .collect()
    }
}

#[cfg(not(feature = "redis-cache"))]
#[derive(Clone)]
pub struct RedisCache;

#[cfg(not(feature = "redis-cache"))]
#[allow(dead_code)]
impl RedisCache {
    pub async fn new(_redis_url: &str) -> Result<Self, String> {
        Ok(Self)
    }

    pub async fn get<T: DeserializeOwned>(&self, _key: &str) -> Option<T> {
        None
    }

    pub async fn set<T: Serialize>(&self, _key: &str, _value: &T, _ttl: Duration) {}

    pub async fn delete(&self, _key: &str) {}

    pub async fn hgetall<T: DeserializeOwned>(&self, _key: &str) -> Vec<T> {
        Vec::new()
    }
}
