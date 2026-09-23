//! The node-side surface the tx-pool mirror observes.
//!
//! One trait so the mirror's state machine is testable without a node, and so a
//! subscription-based source (`new_transaction` / `proposed_transaction`) can
//! replace polling later without the mirror noticing.
//!
//! Everything here is READ-ONLY node RPC. The mirror writes to no store.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use ckb_store_reader::RpcTransactionView;
use serde::{Deserialize, Serialize};

use crate::routes::tx_lookup::{fetch_transaction_lookup, TransactionLookup};

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

/// One live cell as the node reports it, data included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLiveCell {
    pub capacity: u64,
    pub lock: NodeScript,
    pub type_script: Option<NodeScript>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeScript {
    pub code_hash: [u8; 32],
    pub hash_type: i16,
    pub args: Vec<u8>,
}

/// Read-only node RPC the mirror depends on.
#[async_trait]
pub trait PoolSource: Send + Sync {
    async fn tx_pool_info(&self) -> Result<TxPoolInfo, String>;
    async fn raw_tx_pool_verbose(&self) -> Result<RawTxPool, String>;
    async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<PoolTxLookup>, String>;
    /// `get_live_cell(out_point, with_data = true)`. Data is required: DAO,
    /// `.bit` and UDT interpretation all read it, and the store's
    /// `LiveCellInfo` does not carry it.
    async fn get_live_cell(
        &self,
        tx_hash: &[u8; 32],
        index: u32,
    ) -> Result<Option<NodeLiveCell>, String>;
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
struct RawCellWithStatus {
    cell: Option<RawCellInfo>,
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCellInfo {
    output: RawCellOutput,
    data: Option<RawCellData>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCellOutput {
    capacity: String,
    lock: RawScript,
    #[serde(rename = "type")]
    type_: Option<RawScript>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCellData {
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawScript {
    code_hash: String,
    hash_type: String,
    args: String,
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
/// use. Unknown labels are an error, not a default.
pub fn parse_hash_type(label: &str) -> Result<i16, String> {
    match label {
        "data" => Ok(0),
        "type" => Ok(1),
        "data1" => Ok(2),
        "data2" => Ok(3),
        other => Err(format!("unknown script hash_type '{other}' from node")),
    }
}

impl RawScript {
    fn into_node_script(self, field: &str) -> Result<NodeScript, String> {
        Ok(NodeScript {
            code_hash: parse_hex_hash32(&self.code_hash, &format!("{field}.code_hash"))?,
            hash_type: parse_hash_type(&self.hash_type)?,
            args: parse_hex_bytes(&self.args, &format!("{field}.args"))?,
        })
    }
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
        let client = crate::utils::shared_http_client();
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
            .map_err(|e| format!("{method} request failed: {e}"))?
            .json::<RpcResponse<T>>()
            .await
            .map_err(|e| format!("{method} response decode failed: {e}"))?;

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
        let Some(lookup) = fetch_transaction_lookup(&self.url, &hash_hex).await? else {
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

    async fn get_live_cell(
        &self,
        tx_hash: &[u8; 32],
        index: u32,
    ) -> Result<Option<NodeLiveCell>, String> {
        let out_point = serde_json::json!({
            "tx_hash": format!("0x{}", hex::encode(tx_hash)),
            "index": format!("0x{index:x}"),
        });
        let raw: RawCellWithStatus = self
            .call("get_live_cell", (out_point, true))
            .await?
            .ok_or_else(|| "get_live_cell returned no result".to_string())?;

        if raw.status != "live" {
            return Ok(None);
        }
        let Some(info) = raw.cell else {
            return Ok(None);
        };
        Ok(Some(NodeLiveCell {
            capacity: parse_hex_u64(&info.output.capacity, "live cell capacity")?,
            lock: info.output.lock.into_node_script("live cell lock")?,
            type_script: info
                .output
                .type_
                .map(|script| script.into_node_script("live cell type"))
                .transpose()?,
            data: match info.data {
                Some(data) => parse_hex_bytes(&data.content, "live cell data")?,
                None => {
                    return Err(format!(
                        "get_live_cell(with_data=true) returned no data for 0x{}:{index}",
                        hex::encode(tx_hash)
                    ))
                }
            },
        }))
    }
}

// ---------------------------------------------------------------------------
// Test double
// ---------------------------------------------------------------------------

/// Scripted [`PoolSource`] for mirror tests, and for API tests that want a
/// deterministic pool without a node. Records every call so tests can assert
/// what the mirror did *not* ask for.
#[derive(Default)]
pub struct FakePoolSource {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    info: Option<Result<TxPoolInfo, String>>,
    raw_pool: RawTxPool,
    transactions: HashMap<[u8; 32], Result<Option<PoolTxLookup>, String>>,
    live_cells: HashMap<([u8; 32], u32), NodeLiveCell>,
    calls: Vec<String>,
}

impl FakePoolSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_info(&self, info: TxPoolInfo) {
        self.lock().info = Some(Ok(info));
    }

    pub fn set_info_error(&self, error: impl Into<String>) {
        self.lock().info = Some(Err(error.into()));
    }

    pub fn set_raw_pool(&self, pool: RawTxPool) {
        self.lock().raw_pool = pool;
    }

    pub fn set_transaction(&self, tx_hash: [u8; 32], lookup: PoolTxLookup) {
        self.lock().transactions.insert(tx_hash, Ok(Some(lookup)));
    }

    pub fn set_transaction_error(&self, tx_hash: [u8; 32], error: impl Into<String>) {
        self.lock().transactions.insert(tx_hash, Err(error.into()));
    }

    pub fn set_transaction_missing(&self, tx_hash: [u8; 32]) {
        self.lock().transactions.insert(tx_hash, Ok(None));
    }

    pub fn set_live_cell(&self, tx_hash: [u8; 32], index: u32, cell: NodeLiveCell) {
        self.lock().live_cells.insert((tx_hash, index), cell);
    }

    pub fn remove_live_cell(&self, tx_hash: [u8; 32], index: u32) {
        self.lock().live_cells.remove(&(tx_hash, index));
    }

    /// Every RPC method name this source was asked for, in order.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().expect("fake pool source lock poisoned")
    }
}

#[async_trait]
impl PoolSource for FakePoolSource {
    async fn tx_pool_info(&self) -> Result<TxPoolInfo, String> {
        let mut state = self.lock();
        state.calls.push("tx_pool_info".to_string());
        match state.info.clone() {
            Some(result) => result,
            None => Err("fake pool source has no tx_pool_info scripted".to_string()),
        }
    }

    async fn raw_tx_pool_verbose(&self) -> Result<RawTxPool, String> {
        let mut state = self.lock();
        state.calls.push("get_raw_tx_pool".to_string());
        Ok(state.raw_pool.clone())
    }

    async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<PoolTxLookup>, String> {
        let mut state = self.lock();
        state
            .calls
            .push(format!("get_transaction:0x{}", hex::encode(tx_hash)));
        match state.transactions.get(tx_hash) {
            Some(result) => result.clone(),
            None => Ok(None),
        }
    }

    async fn get_live_cell(
        &self,
        tx_hash: &[u8; 32],
        index: u32,
    ) -> Result<Option<NodeLiveCell>, String> {
        let mut state = self.lock();
        state
            .calls
            .push(format!("get_live_cell:0x{}:{index}", hex::encode(tx_hash)));
        Ok(state.live_cells.get(&(*tx_hash, index)).cloned())
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
        assert_eq!(parse_hash_type("data2").unwrap(), 3);
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
