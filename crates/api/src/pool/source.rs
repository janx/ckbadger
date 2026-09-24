//! The node-side surface the tx-pool mirror observes.
//!
//! One trait so the mirror's state machine is testable without a node, and so a
//! subscription-based source (`new_transaction` / `proposed_transaction`) can
//! replace polling later without the mirror noticing.
//!
//! Everything here is READ-ONLY node RPC. The mirror writes to no store.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use ckb_store_reader::RpcTransactionView;
use serde::{Deserialize, Serialize};

use crate::routes::tx_lookup::{
    describe_http_error, fetch_transaction_lookup_with, TransactionLookup,
};

/// A transaction's status as the node reports it.
///
/// Node status is authoritative for every pool transition; the local store is
/// consulted only to decide when a committed record may be dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeTxStatus {
    Pending,
    Proposed,
    Committed,
    Unknown,
    Rejected,
}

/// `tx_pool_info`: the cheap poll that decides whether any further RPC is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxPoolInfo {
    pub tip_hash: [u8; 32],
    pub tip_number: u64,
    pub last_txs_updated_at: u64,
}

/// One verbose `get_raw_tx_pool` entry. Every field is the node's own exact
/// value; none of them is ever recomputed or defaulted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolEntryMeta {
    pub fee: u64,
    pub size: u64,
    pub cycles: u64,
    pub ancestors_count: u64,
    /// `timestamp` of the verbose pool entry: when the node accepted the
    /// transaction into its pool, in milliseconds.
    pub time_added_to_pool_ms: u64,
}

/// The verbose pool, split the way the node reports it.
#[derive(Debug, Clone, Default)]
pub struct RawTxPool {
    pub pending: Vec<([u8; 32], PoolEntryMeta)>,
    pub proposed: Vec<([u8; 32], PoolEntryMeta)>,
}

/// `get_transaction` reduced to what the mirror needs.
#[derive(Debug, Clone)]
pub struct PoolTxLookup {
    pub status: NodeTxStatus,
    pub transaction: Option<RpcTransactionView>,
    pub block_number: Option<i64>,
    pub block_hash: Option<[u8; 32]>,
}

/// A block header reduced to what input resolution needs: the DAO field, whose
/// accumulated rate prices a Nervos DAO withdrawal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeHeader {
    pub number: u64,
    pub hash: [u8; 32],
    pub dao: [u8; 32],
}

/// Read-only node RPC the mirror depends on.
#[async_trait]
pub trait PoolSource: Send + Sync {
    async fn tx_pool_info(&self) -> Result<TxPoolInfo, String>;
    async fn raw_tx_pool_verbose(&self) -> Result<RawTxPool, String>;
    /// `get_transaction(tx_hash)`. The node answers for committed AND pool
    /// transactions, which is what makes it the one source for the cells a
    /// transaction spends: a previous output is `outputs[index]` of the
    /// transaction that created it, spent or not.
    async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<PoolTxLookup>, String>;
    /// `get_header(block_hash)`: the block that committed a DAO withdraw
    /// request, whose accumulated rate is the withdrawal's `AR_withdraw`.
    async fn get_header(&self, block_hash: &[u8; 32]) -> Result<Option<NodeHeader>, String>;
    /// `get_header_by_number(number)`: the deposit block a withdraw-request
    /// cell names in its data, whose accumulated rate is `AR_deposit`.
    async fn get_header_by_number(&self, number: u64) -> Result<Option<NodeHeader>, String>;
}

// ---------------------------------------------------------------------------
// HTTP implementation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct RpcRequest<T> {
    jsonrpc: &'static str,
    method: &'static str,
    params: T,
    id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTxPoolInfo {
    tip_hash: String,
    tip_number: String,
    last_txs_updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTxPoolVerbose {
    pending: HashMap<String, RawPoolEntry>,
    proposed: HashMap<String, RawPoolEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawPoolEntry {
    cycles: String,
    size: String,
    fee: String,
    ancestors_count: String,
    timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawHeader {
    hash: String,
    number: String,
    dao: String,
}

impl RawHeader {
    fn into_node_header(self, method: &str) -> Result<NodeHeader, String> {
        let dao = parse_hex_bytes(&self.dao, &format!("{method}.dao"))?;
        Ok(NodeHeader {
            number: parse_hex_u64(&self.number, &format!("{method}.number"))?,
            hash: parse_hex_hash32(&self.hash, &format!("{method}.hash"))?,
            dao: <[u8; 32]>::try_from(dao.as_slice())
                .map_err(|_| format!("{method}.dao '{}' from node is not 32 bytes", self.dao))?,
        })
    }
}

/// Parse a `0x`-prefixed hex quantity. A malformed node value is an error with
/// the offending text, never a zero.
pub fn parse_hex_u64(value: &str, field: &str) -> Result<u64, String> {
    let stripped = value.strip_prefix("0x").unwrap_or(value);
    u64::from_str_radix(stripped, 16)
        .map_err(|e| format!("invalid hex {field} '{value}' from node: {e}"))
}

/// Parse a `0x`-prefixed hex byte string.
pub fn parse_hex_bytes(value: &str, field: &str) -> Result<Vec<u8>, String> {
    let stripped = value.strip_prefix("0x").unwrap_or(value);
    hex::decode(stripped).map_err(|e| format!("invalid hex {field} '{value}' from node: {e}"))
}

/// Parse a `0x`-prefixed 32-byte hash.
pub fn parse_hex_hash32(value: &str, field: &str) -> Result<[u8; 32], String> {
    let bytes = parse_hex_bytes(value, field)?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| format!("{field} '{value}' from node is not 32 bytes"))
}

/// CKB `hash_type` label to the numeric form the store and the activity builder
/// use, through the workspace's one table (`ckbadger_common::hash_type`).
/// Unknown labels are an error, not a default.
pub fn parse_hash_type(label: &str) -> Result<i16, String> {
    ckbadger_common::hash_type_from_label(label)
        .map(i16::from)
        .ok_or_else(|| format!("unknown script hash_type '{label}' from node"))
}

/// How long [`HttpPoolSource`] waits for the node to accept a connection.
const POOL_RPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The pool source's own HTTP client. The mirror bounds every call it makes
/// (`POOL_RPC_TIMEOUT`); this client is the second line of defence, so the
/// `/tx` pending branch — which talks to the node through the same source —
/// cannot hang on a black-holed node either.
fn pool_rpc_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(POOL_RPC_CONNECT_TIMEOUT)
            .timeout(super::mirror::POOL_RPC_TIMEOUT)
            .build()
            .expect("static reqwest client configuration is valid")
    })
}

/// Talks to the local CKB node over JSON-RPC.
pub struct HttpPoolSource {
    url: String,
}

impl HttpPoolSource {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    async fn call<P: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<Option<T>, String> {
        let client = pool_rpc_client();
        let request = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        };
        let response = client
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("{method} request failed: {}", describe_http_error(e)))?
            .json::<RpcResponse<T>>()
            .await
            .map_err(|e| {
                format!(
                    "{method} response decode failed: {}",
                    describe_http_error(e)
                )
            })?;

        if let Some(error) = response.error {
            return Err(format!(
                "{method} RPC error {}: {}",
                error.code, error.message
            ));
        }
        Ok(response.result)
    }
}

fn pool_entries(
    raw: HashMap<String, RawPoolEntry>,
    bucket: &str,
) -> Result<Vec<([u8; 32], PoolEntryMeta)>, String> {
    raw.into_iter()
        .map(|(hash, entry)| {
            let tx_hash = parse_hex_hash32(&hash, &format!("{bucket} pool tx_hash"))?;
            Ok((
                tx_hash,
                PoolEntryMeta {
                    fee: parse_hex_u64(&entry.fee, "pool entry fee")?,
                    size: parse_hex_u64(&entry.size, "pool entry size")?,
                    cycles: parse_hex_u64(&entry.cycles, "pool entry cycles")?,
                    ancestors_count: parse_hex_u64(
                        &entry.ancestors_count,
                        "pool entry ancestors_count",
                    )?,
                    time_added_to_pool_ms: parse_hex_u64(&entry.timestamp, "pool entry timestamp")?,
                },
            ))
        })
        .collect()
}

fn node_status(lookup: &TransactionLookup) -> NodeTxStatus {
    use ckb_jsonrpc_types::Status;
    match lookup.status {
        Status::Pending => NodeTxStatus::Pending,
        Status::Proposed => NodeTxStatus::Proposed,
        Status::Committed => NodeTxStatus::Committed,
        Status::Unknown => NodeTxStatus::Unknown,
        Status::Rejected => NodeTxStatus::Rejected,
    }
}

#[async_trait]
impl PoolSource for HttpPoolSource {
    async fn tx_pool_info(&self) -> Result<TxPoolInfo, String> {
        let raw: RawTxPoolInfo = self
            .call("tx_pool_info", ())
            .await?
            .ok_or_else(|| "tx_pool_info returned no result".to_string())?;
        Ok(TxPoolInfo {
            tip_hash: parse_hex_hash32(&raw.tip_hash, "tx_pool_info.tip_hash")?,
            tip_number: parse_hex_u64(&raw.tip_number, "tx_pool_info.tip_number")?,
            last_txs_updated_at: parse_hex_u64(
                &raw.last_txs_updated_at,
                "tx_pool_info.last_txs_updated_at",
            )?,
        })
    }

    async fn raw_tx_pool_verbose(&self) -> Result<RawTxPool, String> {
        let raw: RawTxPoolVerbose = self
            .call("get_raw_tx_pool", (Some(true),))
            .await?
            .ok_or_else(|| "get_raw_tx_pool returned no result".to_string())?;
        Ok(RawTxPool {
            pending: pool_entries(raw.pending, "pending")?,
            proposed: pool_entries(raw.proposed, "proposed")?,
        })
    }

    async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<PoolTxLookup>, String> {
        let hash_hex = format!("0x{}", hex::encode(tx_hash));
        let Some(lookup) =
            fetch_transaction_lookup_with(pool_rpc_client(), &self.url, &hash_hex).await?
        else {
            return Ok(None);
        };
        let status = node_status(&lookup);
        Ok(Some(PoolTxLookup {
            status,
            transaction: lookup.transaction,
            block_number: lookup.block_number,
            block_hash: lookup.block_hash,
        }))
    }

    async fn get_header(&self, block_hash: &[u8; 32]) -> Result<Option<NodeHeader>, String> {
        let hash_hex = format!("0x{}", hex::encode(block_hash));
        let Some(raw) = self
            .call::<_, RawHeader>("get_header", (&hash_hex,))
            .await?
        else {
            return Ok(None);
        };
        let header = raw.into_node_header("get_header")?;
        if header.hash != *block_hash {
            return Err(format!(
                "get_header({hash_hex}) returned header 0x{}",
                hex::encode(header.hash)
            ));
        }
        Ok(Some(header))
    }

    async fn get_header_by_number(&self, number: u64) -> Result<Option<NodeHeader>, String> {
        let number_hex = format!("0x{number:x}");
        let Some(raw) = self
            .call::<_, RawHeader>("get_header_by_number", (&number_hex,))
            .await?
        else {
            return Ok(None);
        };
        let header = raw.into_node_header("get_header_by_number")?;
        if header.number != number {
            return Err(format!(
                "get_header_by_number({number}) returned header #{}",
                header.number
            ));
        }
        Ok(Some(header))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hash_type_rejects_unknown_label() {
        assert_eq!(parse_hash_type("data").unwrap(), 0);
        assert_eq!(parse_hash_type("type").unwrap(), 1);
        assert_eq!(parse_hash_type("data1").unwrap(), 2);
        assert_eq!(parse_hash_type("data2").unwrap(), 4);
        let error = parse_hash_type("bogus").unwrap_err();
        assert!(
            error.contains("bogus"),
            "unknown hash_type must name the offending value, got: {error}"
        );
    }

    #[test]
    fn test_parse_hex_u64_reports_the_bad_value() {
        assert_eq!(parse_hex_u64("0x1f", "fee").unwrap(), 31);
        let error = parse_hex_u64("0xzz", "fee").unwrap_err();
        assert!(error.contains("fee") && error.contains("0xzz"), "{error}");
    }

    #[test]
    fn test_parse_hex_hash32_rejects_wrong_length() {
        let error = parse_hex_hash32("0xabcd", "tip_hash").unwrap_err();
        assert!(error.contains("not 32 bytes"), "{error}");
    }
}
