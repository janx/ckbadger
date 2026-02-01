use anyhow::{anyhow, Result};
use reqwest::Client;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub use ckbadger_common::{parse_capacity, parse_hex_to_bytes, parse_hex_u32};

use super::types::*;

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

    fn next_ids(&self, count: usize) -> Vec<u64> {
        let start = self.request_id.fetch_add(count as u64, Ordering::SeqCst);
        (start..start + count as u64).collect()
    }

    async fn call<P, R>(&self, method: &'static str, params: P) -> Result<R>
    where
        P: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let request = JsonRpcRequest::new(self.next_id(), method, params);
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

    async fn call_batch(
        &self,
        requests: Vec<JsonRpcBatchRequest>,
    ) -> Result<Vec<JsonRpcBatchResponseItem>> {
        if requests.is_empty() {
            return Ok(vec![]);
        }

        let response = self
            .client
            .post(&self.url)
            .json(&requests)
            .send()
            .await?
            .json::<Vec<JsonRpcBatchResponseItem>>()
            .await?;

        Ok(response)
    }

    pub async fn get_tip_block_number(&self) -> Result<u64> {
        let result: String = self.call("get_tip_block_number", ()).await?;
        parse_hex_u64(&result)
    }

    pub async fn get_tip_header(&self) -> Result<TipHeader> {
        self.call("get_tip_header", ()).await
    }

    pub async fn get_block_by_number(
        &self,
        number: u64,
    ) -> Result<Option<BlockResponseWithCycles>> {
        let hex_number = format!("0x{:x}", number);
        self.call("get_block_by_number", (hex_number, Some("0x2"), Some(true)))
            .await
    }

    pub async fn get_block_hash(&self, number: u64) -> Result<Option<String>> {
        let hex_number = format!("0x{:x}", number);
        self.call("get_block_hash", (hex_number,)).await
    }

    pub async fn get_block(&self, hash: &str) -> Result<Option<BlockView>> {
        self.call("get_block", (hash,)).await
    }

    pub async fn get_block_economic_state(&self, hash: &str) -> Result<Option<BlockEconomicState>> {
        self.call("get_block_economic_state", (hash,)).await
    }

    pub async fn get_transaction(&self, hash: &str) -> Result<Option<TransactionWithStatus>> {
        self.call("get_transaction", (hash,)).await
    }

    pub async fn get_tx_pool_info(&self) -> Result<TxPoolInfo> {
        self.call("tx_pool_info", ()).await
    }

    pub async fn get_raw_tx_pool(&self) -> Result<RawTxPool> {
        self.call("get_raw_tx_pool", (Option::<bool>::None,)).await
    }

    pub async fn get_raw_tx_pool_verbose(&self) -> Result<RawTxPoolVerbose> {
        self.call("get_raw_tx_pool", (Some(true),)).await
    }

    pub async fn get_blocks_batch(
        &self,
        block_numbers: &[u64],
    ) -> Result<Vec<Option<BlockResponseWithCycles>>> {
        if block_numbers.is_empty() {
            return Ok(vec![]);
        }

        let ids = self.next_ids(block_numbers.len());
        let requests: Vec<JsonRpcBatchRequest> = block_numbers
            .iter()
            .zip(ids.iter())
            .map(|(num, id)| {
                let hex_number = format!("0x{:x}", num);
                JsonRpcBatchRequest::new(
                    *id,
                    "get_block_by_number",
                    serde_json::json!([hex_number, "0x2", true]),
                )
            })
            .collect();

        let responses = self.call_batch(requests).await?;

        let mut id_to_response: std::collections::HashMap<u64, JsonRpcBatchResponseItem> =
            responses.into_iter().map(|r| (r.id, r)).collect();

        let mut results = Vec::with_capacity(block_numbers.len());
        for id in ids {
            if let Some(resp) = id_to_response.remove(&id) {
                if let Some(error) = resp.error {
                    return Err(anyhow!("RPC batch error {}: {}", error.code, error.message));
                }
                let block: Option<BlockResponseWithCycles> =
                    resp.result.map(serde_json::from_value).transpose()?;
                results.push(block);
            } else {
                results.push(None);
            }
        }

        Ok(results)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TransactionWithStatus {
    pub transaction: Option<TransactionView>,
    pub tx_status: TxStatus,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TxStatus {
    pub status: String,
    pub block_hash: Option<String>,
    pub block_number: Option<String>,
}

fn parse_hex_u64(hex: &str) -> Result<u64> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(hex, 16).map_err(|e| anyhow!("Failed to parse hex: {}", e))
}
