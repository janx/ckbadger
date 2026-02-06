use serde::{de::DeserializeOwned, Serialize};

#[allow(dead_code)]
#[derive(Clone)]
pub struct RedisCache;

#[allow(dead_code)]
impl RedisCache {
    pub async fn new(_redis_url: &str) -> Result<Self, String> {
        Ok(Self)
    }

    pub async fn get<T: DeserializeOwned>(&self, _key: &str) -> Option<T> {
        None
    }

    pub async fn set<T: Serialize>(&self, _key: &str, _value: &T, _ttl: std::time::Duration) {}

    pub async fn delete(&self, _key: &str) {}

    pub async fn hgetall<T: DeserializeOwned>(&self, _key: &str) -> Vec<T> {
        Vec::new()
    }
}
