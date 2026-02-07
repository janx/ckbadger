use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Simple CKB RPC client for mempool queries
#[derive(Clone)]
pub struct CkbRpcClient {
    client: Client,
    url: String,
    request_id: Arc<AtomicU64>,
}

impl CkbRpcClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            url: url.into(),
            request_id: Arc::new(AtomicU64::new(1)),
        }
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    async fn call<P, R>(&self, method: &'static str, params: P) -> Result<R>
    where
        P: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: self.next_id(),
            method,
            params,
        };

        let response = self
            .client
            .post(&self.url)
            .json(&request)
            .send()
            .await?
            .json::<JsonRpcResponse<R>>()
            .await?;

        if let Some(error) = response.error {
            return Err(anyhow!("RPC error {}: {}", error.code, error.message));
        }

        response.result.ok_or_else(|| anyhow!("Empty RPC response"))
    }

    /// Get transaction pool statistics
    pub async fn get_tx_pool_info(&self) -> Result<TxPoolInfo> {
        self.call("tx_pool_info", ()).await
    }

    /// Get transaction pool with verbose details (fee, size, cycles per tx)
    pub async fn get_raw_tx_pool_verbose(&self) -> Result<RawTxPoolVerbose> {
        self.call("get_raw_tx_pool", (Some(true),)).await
    }

    /// Get tip header with epoch and difficulty info
    pub async fn get_tip_header(&self) -> Result<TipHeader> {
        self.call("get_tip_header", ()).await
    }
}

#[derive(Serialize)]
struct JsonRpcRequest<T> {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: T,
}

#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Response from `tx_pool_info` RPC
#[derive(Debug, Clone, Deserialize)]
pub struct TxPoolInfo {
    pub tip_hash: String,
    pub tip_number: String,
    pub pending: String,
    pub proposed: String,
    pub orphan: String,
    pub total_tx_cycles: String,
    pub total_tx_size: String,
    pub min_fee_rate: String,
    pub last_txs_updated_at: String,
}

/// Response from `get_raw_tx_pool` RPC with verbose=true
#[derive(Debug, Clone, Deserialize)]
pub struct RawTxPoolVerbose {
    pub pending: HashMap<String, TxPoolEntry>,
    pub proposed: HashMap<String, TxPoolEntry>,
}

/// Detailed entry for a transaction in the txpool
#[derive(Debug, Clone, Deserialize)]
pub struct TxPoolEntry {
    pub cycles: String,
    pub size: String,
    pub fee: String,
    pub ancestors_count: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TipHeader {
    pub compact_target: String,
    pub timestamp: String,
    pub number: String,
    pub epoch: String,
    pub hash: String,
}

pub fn parse_hex_u64(hex: &str) -> Result<u64> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(hex, 16).map_err(|e| anyhow!("Failed to parse hex '{}': {}", hex, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_u64() {
        assert_eq!(parse_hex_u64("0x10").unwrap(), 16);
        assert_eq!(parse_hex_u64("0xff").unwrap(), 255);
        assert_eq!(parse_hex_u64("0x0").unwrap(), 0);
        assert_eq!(parse_hex_u64("ff").unwrap(), 255);
    }
}
