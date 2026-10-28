mod vm;
use crate::cycles_task::ScriptGroupKind;
use ckb_types::prelude::*;
use serde::{Deserialize, Serialize};

/// Only failures after chain data has been obtained are terminal replay results.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CyclesError {
    #[error("Chain data unavailable: {0}")]
    Unavailable(String),
    #[error("{0}")]
    Replay(ReplayFailure),
}

/// Fixed-size, public chain evidence. Detailed diagnostics belong only in logs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplayFailure {
    DependencyResolution,
    InvalidChainData,
    ScriptGroup {
        group_type: ScriptGroupKind,
        script_hash: [u8; 32],
    },
    NoScriptGroups,
    CyclesOverflow,
}

impl std::fmt::Display for ReplayFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DependencyResolution => {
                f.write_str("Script dependency resolution failed during replay")
            }
            Self::InvalidChainData => f.write_str("Invalid chain data during replay"),
            Self::ScriptGroup {
                group_type,
                script_hash,
            } => write!(
                f,
                "Script group {} 0x{} failed during replay",
                match group_type {
                    ScriptGroupKind::Lock => "lock",
                    ScriptGroupKind::Type => "type",
                },
                hex::encode(script_hash)
            ),
            Self::NoScriptGroups => f.write_str("Transaction has no script groups during replay"),
            Self::CyclesOverflow => f.write_str("Cycles overflow during replay"),
        }
    }
}

impl std::error::Error for ReplayFailure {}

impl ReplayFailure {
    fn logged(self, detail: impl std::fmt::Display) -> Self {
        tracing::warn!(failure = %self, error = %detail, "Cycles replay failed");
        self
    }

    fn invalid_data(detail: impl std::fmt::Display) -> Self {
        Self::InvalidChainData.logged(detail)
    }
}

impl CyclesError {
    fn invalid_data(detail: impl std::fmt::Display) -> Self {
        Self::Replay(ReplayFailure::invalid_data(detail))
    }
}

#[derive(Debug, Serialize)]
pub struct MockTransaction {
    pub mock_info: MockInfo,
    pub tx: Transaction,
}

#[derive(Debug, Serialize)]
pub struct MockInfo {
    pub inputs: Vec<MockInput>,
    pub cell_deps: Vec<MockCellDep>,
    pub header_deps: Vec<MockHeader>,
}

/// Mirrors `ckb_mock_tx_types::ReprMockInput`.
///
/// `header` is the cell's *header association*: the hash of the block that
/// committed the transaction which created this cell. The node derives it for
/// every resolved cell (`CellMeta::transaction_info`), and scripts read it back
/// with `load_header(source = Input)` — the Nervos DAO type script does exactly
/// that in both withdraw phases. Upstream types it `Option<H256>`, but a cell
/// consumed by a committed transaction always has a committing block, so an
/// unresolvable one is an invariant violation, not a `None`.
#[derive(Debug, Serialize)]
pub struct MockInput {
    pub input: Input,
    pub output: Output,
    pub data: String,
    pub header: String,
    #[serde(skip)]
    pub transaction_info: ckb_types::core::TransactionInfo,
}

/// Mirrors `ckb_mock_tx_types::ReprMockCellDep`; see [`MockInput::header`] for
/// the header association, which `load_header(source = CellDep)` reads back.
#[derive(Debug, Serialize)]
pub struct MockCellDep {
    pub cell_dep: CellDep,
    pub output: Output,
    pub data: String,
    pub header: String,
    #[serde(skip)]
    pub transaction_info: ckb_types::core::TransactionInfo,
}

#[derive(Debug, Serialize, Clone)]
pub struct MockHeader {
    pub compact_target: String,
    pub hash: String,
    pub number: String,
    pub parent_hash: String,
    pub nonce: String,
    pub timestamp: String,
    pub transactions_root: String,
    pub proposals_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncles_hash: Option<String>,
    pub version: String,
    pub epoch: String,
    pub dao: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Input {
    pub previous_output: OutPoint,
    pub since: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Output {
    pub capacity: String,
    pub lock: Script,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_script: Option<Script>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Script {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct OutPoint {
    pub tx_hash: String,
    pub index: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CellDep {
    pub out_point: OutPoint,
    pub dep_type: String,
}

#[derive(Debug, Serialize)]
pub struct Transaction {
    pub version: String,
    pub cell_deps: Vec<CellDep>,
    pub header_deps: Vec<String>,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub outputs_data: Vec<String>,
    pub witnesses: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RpcResponse<T> {
    pub result: Option<T>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct TransactionWithStatus {
    pub transaction: Option<RpcTransaction>,
    pub tx_status: TxStatus,
}

#[derive(Debug, Deserialize)]
pub struct TxStatus {
    pub status: String,
    pub block_hash: Option<String>,
    pub tx_index: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RpcTransaction {
    pub version: String,
    pub cell_deps: Vec<RpcCellDep>,
    pub header_deps: Vec<String>,
    pub inputs: Vec<RpcInput>,
    pub outputs: Vec<RpcOutput>,
    pub outputs_data: Vec<String>,
    pub witnesses: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcCellDep {
    pub out_point: RpcOutPoint,
    pub dep_type: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcInput {
    pub previous_output: RpcOutPoint,
    pub since: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcOutPoint {
    pub tx_hash: String,
    pub index: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcOutput {
    pub capacity: String,
    pub lock: RpcScript,
    #[serde(rename = "type")]
    pub type_script: Option<RpcScript>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcScript {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RpcHeader {
    pub compact_target: String,
    pub hash: String,
    pub number: String,
    pub parent_hash: String,
    pub nonce: String,
    pub timestamp: String,
    pub transactions_root: String,
    pub proposals_hash: String,
    pub extra_hash: Option<String>,
    pub uncles_hash: Option<String>,
    pub version: String,
    pub epoch: String,
    pub dao: String,
}

#[derive(Debug, Deserialize)]
pub struct LiveCellResponse {
    pub cell: Option<CellWithData>,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CellWithData {
    pub output: RpcOutput,
    pub data: RpcCellData,
}

#[derive(Debug, Deserialize)]
pub struct RpcCellData {
    pub content: String,
}

/// VM activation epochs read from the chain's `get_consensus` hardfork features.
///
/// `u64::MAX` means the feature is not activated on this chain (absent entry or
/// null `epoch_number`), so no reachable epoch ever selects that VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmActivationEpochs {
    /// RFC0032 (CKB-VM version 1) activation epoch.
    pub vm1: u64,
    /// RFC0049 (CKB-VM version 2) activation epoch.
    pub vm2: u64,
}

/// RFC number activating CKB-VM version 1 (CKB2021 / Mirana).
const RFC_VM1: &str = "0032";
/// RFC number activating CKB-VM version 2 (CKB2023 / Meepo).
const RFC_VM2: &str = "0049";

pub async fn calculate_cycles(
    ckb_rpc_url: &str,
    tx_hash: &str,
    expected_block_hash: &[u8; 32],
) -> Result<crate::cycles_task::TransactionCycles, CyclesError> {
    let (tx, status) = fetch_transaction(ckb_rpc_url, tx_hash).await?;
    let block_hash = committed_block_hash(tx_hash, &status)?;
    if block_hash != format!("0x{}", hex::encode(expected_block_hash)) {
        return Err(CyclesError::Unavailable(format!("transaction {tx_hash} commit block differs from canonical store: node={block_hash} store=0x{}", hex::encode(expected_block_hash))));
    }
    let (epochs, max_cycles) = fetch_vm_consensus(ckb_rpc_url).await?;
    let commit = vm::header(&fetch_header(ckb_rpc_url, &block_hash).await?)
        .map_err(CyclesError::invalid_data)?;
    if commit.number() == 0 {
        return Err(CyclesError::invalid_data(
            "genesis transactions have no consensus script execution",
        ));
    }
    let mock = build_mock_transaction(ckb_rpc_url, &tx).await?;
    let mut extensions = std::collections::HashMap::new();
    for hash in &tx.header_deps {
        if let Some(extension) = fetch_block_extension(ckb_rpc_url, hash).await? {
            let key = ckb_types::packed::Byte32::from_slice(
                &hex::decode(hash.trim_start_matches("0x")).map_err(CyclesError::invalid_data)?,
            )
            .map_err(CyclesError::invalid_data)?;
            extensions.insert(key, extension.pack());
        }
    }
    tokio::task::spawn_blocking(move || vm::execute(mock, commit, epochs, max_cycles, extensions))
        .await
        .map_err(|e| CyclesError::invalid_data(format!("cycles VM worker failed: {e}")))?
        .map_err(CyclesError::Replay)
}

async fn fetch_block_extension(
    url: &str,
    hash: &str,
) -> Result<Option<ckb_types::bytes::Bytes>, CyclesError> {
    #[derive(Deserialize)]
    struct BlockExtension {
        extension: Option<String>,
    }
    let response: RpcResponse<BlockExtension> = reqwest::Client::new()
        .post(url)
        .json(&serde_json::json!({"id":1,"jsonrpc":"2.0","method":"get_block","params":[hash]}))
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(e.to_string()))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(e.to_string()))?;
    if let Some(error) = response.error {
        return Err(CyclesError::Unavailable(format!(
            "block extension RPC error: {}",
            error.message
        )));
    }
    let block = response.result.ok_or_else(|| {
        CyclesError::Unavailable(format!("header dependency block missing: {hash}"))
    })?;
    block
        .extension
        .map(|value| {
            hex::decode(value.trim_start_matches("0x"))
                .map(Into::into)
                .map_err(CyclesError::invalid_data)
        })
        .transpose()
}

async fn fetch_transaction(
    ckb_rpc_url: &str,
    tx_hash: &str,
) -> Result<(RpcTransaction, TxStatus), CyclesError> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_transaction",
        "params": [tx_hash]
    });

    let resp: RpcResponse<TransactionWithStatus> = client
        .post(ckb_rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("RPC request failed: {e}")))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("Failed to parse RPC response: {e}")))?;

    if let Some(err) = resp.error {
        return Err(CyclesError::Unavailable(format!(
            "RPC error: {}",
            err.message
        )));
    }

    let with_status = resp
        .result
        .ok_or_else(|| CyclesError::Unavailable(format!("Transaction {} not found", tx_hash)))?;
    let status = with_status.tx_status.status.clone();
    let tx = with_status.transaction.ok_or_else(|| {
        CyclesError::Unavailable(format!(
            "Transaction {} has no transaction body (status: '{}')",
            tx_hash, status
        ))
    })?;

    Ok((tx, with_status.tx_status))
}

/// The hash of the block that committed `tx_hash`.
///
/// Single source for "which block does this transaction belong to", used both
/// for consensus VM selection and for every resolved cell's header association.
/// A transaction that is not committed has no such block, so this fails fast
/// instead of substituting a placeholder.
fn committed_block_hash(tx_hash: &str, status: &TxStatus) -> Result<String, CyclesError> {
    if status.status != "committed" {
        return Err(CyclesError::Unavailable(format!(
            "transaction {} has status '{}', not 'committed'",
            tx_hash, status.status
        )));
    }
    status.block_hash.clone().ok_or_else(|| {
        CyclesError::Unavailable(format!(
            "transaction {} is 'committed' but the node returned no block_hash",
            tx_hash
        ))
    })
}

/// Resolve the committing block of `tx_hash` without downloading its body.
///
/// Verbosity `0x1` returns the `tx_status` only (`transaction: null`), which is
/// all the header association needs — the genesis system-script transaction
/// alone is 2.4 MB, and cell payloads are resolved separately.
async fn fetch_committing_transaction_info(
    ckb_rpc_url: &str,
    tx_hash: &str,
) -> Result<ckb_types::core::TransactionInfo, CyclesError> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_transaction",
        "params": [tx_hash, "0x1"]
    });

    let resp: RpcResponse<TransactionWithStatus> = client
        .post(ckb_rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("RPC request failed: {e}")))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("Failed to parse RPC response: {e}")))?;

    if let Some(err) = resp.error {
        return Err(CyclesError::Unavailable(format!(
            "RPC error: {}",
            err.message
        )));
    }

    let with_status = resp
        .result
        .ok_or_else(|| CyclesError::Unavailable(format!("Transaction {} not found", tx_hash)))?;

    let block_hash = committed_block_hash(tx_hash, &with_status.tx_status)?;
    let index = with_status.tx_status.tx_index.as_deref().ok_or_else(|| {
        CyclesError::invalid_data(format!("committed transaction lacks tx_index: {tx_hash}"))
    })?;
    let index = parse_hex_index(index).map_err(CyclesError::invalid_data)?;
    let header = vm::header(&fetch_header(ckb_rpc_url, &block_hash).await?)
        .map_err(CyclesError::invalid_data)?;
    Ok(ckb_types::core::TransactionInfo::new(
        header.number(),
        header.epoch(),
        header.hash(),
        index,
    ))
}

/// Fetch the VM activation epochs (RFC0032 -> VM1, RFC0049 -> VM2) from the
/// chain's `get_consensus` response. Chain data is the source of truth — no
/// per-network hardcoded tables.
async fn fetch_vm_consensus(ckb_rpc_url: &str) -> Result<(VmActivationEpochs, u64), CyclesError> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_consensus",
        "params": []
    });

    let resp: RpcResponse<serde_json::Value> = client
        .post(ckb_rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("RPC request failed: {e}")))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("Failed to parse RPC response: {e}")))?;

    if let Some(err) = resp.error {
        return Err(CyclesError::Unavailable(format!(
            "RPC error: {}",
            err.message
        )));
    }

    let consensus = resp
        .result
        .ok_or_else(|| CyclesError::Unavailable("get_consensus returned no result".into()))?;

    let max = consensus
        .get("max_block_cycles")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CyclesError::invalid_data("consensus missing max_block_cycles"))?;
    Ok((
        parse_vm_activation_epochs(&consensus).map_err(CyclesError::invalid_data)?,
        parse_hex_u64(max).map_err(CyclesError::invalid_data)?,
    ))
}

/// Extract VM activation epochs from a `get_consensus` result object.
///
/// An absent rfc entry or a null `epoch_number` means the feature is not
/// activated on this chain and maps to `u64::MAX`. A malformed
/// `hardfork_features` shape or a non-hex epoch value is an error.
fn parse_vm_activation_epochs(consensus: &serde_json::Value) -> Result<VmActivationEpochs, String> {
    let features = consensus
        .get("hardfork_features")
        .and_then(|f| f.as_array())
        .ok_or_else(|| "get_consensus result missing hardfork_features array".to_string())?;

    Ok(VmActivationEpochs {
        vm1: vm_activation_epoch_for_rfc(features, RFC_VM1)?,
        vm2: vm_activation_epoch_for_rfc(features, RFC_VM2)?,
    })
}

fn vm_activation_epoch_for_rfc(features: &[serde_json::Value], rfc: &str) -> Result<u64, String> {
    let entry = features
        .iter()
        .find(|f| f.get("rfc").and_then(|r| r.as_str()) == Some(rfc));

    match entry {
        // Absent entry: the feature does not exist on this chain.
        None => Ok(u64::MAX),
        Some(feature) => match feature.get("epoch_number") {
            // Null (or omitted) epoch_number: known feature, not activated.
            None | Some(serde_json::Value::Null) => Ok(u64::MAX),
            Some(serde_json::Value::String(hex)) => parse_hex_u64(hex)
                .map_err(|e| format!("invalid epoch_number for rfc {}: {}", rfc, e)),
            Some(other) => Err(format!(
                "unexpected epoch_number type for rfc {}: {}",
                rfc, other
            )),
        },
    }
}

/// Map a commit-block epoch number to the consensus CKB-VM script version.
#[cfg(test)]
fn script_version_for_epoch(epoch_number: u64, epochs: &VmActivationEpochs) -> u8 {
    vm::type_script_version_at_epoch(epoch_number, *epochs)
}

fn parse_hex_u64(hex: &str) -> Result<u64, String> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(stripped, 16).map_err(|e| format!("invalid hex u64 '{}': {}", hex, e))
}

/// Extract the epoch number from a header `epoch` field.
///
/// The header packs `length << 40 | index << 24 | number`; the epoch number is
/// the low 24 bits.
#[cfg(test)]
fn parse_epoch_number(epoch_hex: &str) -> Result<u64, String> {
    Ok(
        ckb_types::core::EpochNumberWithFraction::from_full_value(parse_hex_u64(epoch_hex)?)
            .number(),
    )
}

/// A cell resolved from chain data, together with the block that created it.
struct ResolvedCell {
    output: RpcOutput,
    data: String,
    /// Hash of the block that committed the creating transaction; see
    /// [`MockInput::header`].
    block_hash: String,
    transaction_info: ckb_types::core::TransactionInfo,
}

/// Resolve one out point into the cell payload plus its header association.
async fn fetch_cell(
    ckb_rpc_url: &str,
    out_point: &RpcOutPoint,
) -> Result<ResolvedCell, CyclesError> {
    let (output, data) = fetch_cell_with_data(ckb_rpc_url, out_point).await?;
    let transaction_info =
        fetch_committing_transaction_info(ckb_rpc_url, &out_point.tx_hash).await?;
    Ok(ResolvedCell {
        output,
        data,
        block_hash: format!("{:#x}", transaction_info.block_hash),
        transaction_info,
    })
}

async fn fetch_cell_with_data(
    ckb_rpc_url: &str,
    out_point: &RpcOutPoint,
) -> Result<(RpcOutput, String), CyclesError> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_live_cell",
        "params": [{
            "tx_hash": out_point.tx_hash,
            "index": out_point.index
        }, true]
    });

    let resp: RpcResponse<LiveCellResponse> = client
        .post(ckb_rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("RPC request failed: {e}")))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("Failed to parse RPC response: {e}")))?;

    if let Some(error) = resp.error {
        return Err(CyclesError::Unavailable(format!(
            "live cell RPC error for {}:{}: {}",
            out_point.tx_hash, out_point.index, error.message
        )));
    }
    if let Some(result) = resp.result {
        if let Some(cell) = result.cell {
            return Ok((cell.output, cell.data.content));
        }
    }

    fetch_cell_from_transaction(ckb_rpc_url, out_point).await
}

async fn fetch_cell_from_transaction(
    ckb_rpc_url: &str,
    out_point: &RpcOutPoint,
) -> Result<(RpcOutput, String), CyclesError> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_transaction",
        "params": [out_point.tx_hash]
    });

    let resp: RpcResponse<TransactionWithStatus> = client
        .post(ckb_rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("RPC request failed: {e}")))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("Failed to parse RPC response: {e}")))?;

    if let Some(err) = resp.error {
        return Err(CyclesError::Unavailable(format!(
            "RPC error: {}",
            err.message
        )));
    }

    let tx = resp.result.and_then(|r| r.transaction).ok_or_else(|| {
        CyclesError::Unavailable(format!("Transaction {} not found", out_point.tx_hash))
    })?;

    let index = parse_hex_index(&out_point.index).map_err(CyclesError::invalid_data)?;
    let output =
        tx.outputs.get(index).cloned().ok_or_else(|| {
            CyclesError::invalid_data(format!("Output index {} out of range", index))
        })?;
    // outputs_data is 1:1 with outputs in the molecule layout; a short vector
    // means the node returned a malformed transaction, not an empty cell.
    let data = tx.outputs_data.get(index).cloned().ok_or_else(|| {
        CyclesError::invalid_data(format!(
            "transaction {} has {} outputs but only {} outputs_data entries (index {})",
            out_point.tx_hash,
            tx.outputs.len(),
            tx.outputs_data.len(),
            index
        ))
    })?;

    Ok((output, data))
}

async fn fetch_header(ckb_rpc_url: &str, block_hash: &str) -> Result<RpcHeader, CyclesError> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_header",
        "params": [block_hash]
    });

    let resp: RpcResponse<RpcHeader> = client
        .post(ckb_rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("RPC request failed: {e}")))?
        .json()
        .await
        .map_err(|e| CyclesError::Unavailable(format!("Failed to parse RPC response: {e}")))?;

    if let Some(err) = resp.error {
        return Err(CyclesError::Unavailable(format!(
            "RPC error: {}",
            err.message
        )));
    }

    resp.result
        .ok_or_else(|| CyclesError::Unavailable(format!("Header {} not found", block_hash)))
}

fn parse_hex_index(hex: &str) -> Result<usize, String> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    usize::from_str_radix(hex, 16).map_err(|e| format!("Invalid hex index: {}", e))
}

fn parse_dep_group_data(data: &str) -> Result<Vec<RpcOutPoint>, String> {
    let bytes = hex::decode(data.strip_prefix("0x").unwrap_or(data))
        .map_err(|e| format!("Invalid dep_group data: {}", e))?;

    if bytes.len() < 4 {
        return Err("dep_group data too short".to_string());
    }

    let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let mut out_points = Vec::with_capacity(count);

    let mut offset = 4;
    for _ in 0..count {
        if offset + 36 > bytes.len() {
            return Err("dep_group data truncated".to_string());
        }

        let tx_hash = format!("0x{}", hex::encode(&bytes[offset..offset + 32]));
        let index = u32::from_le_bytes([
            bytes[offset + 32],
            bytes[offset + 33],
            bytes[offset + 34],
            bytes[offset + 35],
        ]);

        out_points.push(RpcOutPoint {
            tx_hash,
            index: format!("0x{:x}", index),
        });

        offset += 36;
    }

    Ok(out_points)
}

fn output_from_rpc(output: RpcOutput) -> Output {
    Output {
        capacity: output.capacity,
        lock: Script {
            code_hash: output.lock.code_hash,
            hash_type: output.lock.hash_type,
            args: output.lock.args,
        },
        type_script: output.type_script.map(|t| Script {
            code_hash: t.code_hash,
            hash_type: t.hash_type,
            args: t.args,
        }),
    }
}

/// Assemble a mock input, carrying the resolved cell's header association.
fn mock_input_from(input: &RpcInput, cell: ResolvedCell) -> MockInput {
    MockInput {
        input: Input {
            previous_output: OutPoint {
                tx_hash: input.previous_output.tx_hash.clone(),
                index: input.previous_output.index.clone(),
            },
            since: input.since.clone(),
        },
        output: output_from_rpc(cell.output),
        data: cell.data,
        header: cell.block_hash,
        transaction_info: cell.transaction_info,
    }
}

/// Assemble a mock cell dep, carrying the resolved cell's header association.
///
/// Used both for the transaction's own cell deps and for the cells a
/// `dep_group` expands into (those are always `code` deps).
fn mock_cell_dep_from(out_point: &RpcOutPoint, dep_type: &str, cell: ResolvedCell) -> MockCellDep {
    MockCellDep {
        cell_dep: CellDep {
            out_point: OutPoint {
                tx_hash: out_point.tx_hash.clone(),
                index: out_point.index.clone(),
            },
            dep_type: dep_type.to_string(),
        },
        output: output_from_rpc(cell.output),
        data: cell.data,
        header: cell.block_hash,
        transaction_info: cell.transaction_info,
    }
}

async fn build_mock_transaction(
    ckb_rpc_url: &str,
    tx: &RpcTransaction,
) -> Result<MockTransaction, CyclesError> {
    let mut mock_inputs = Vec::new();
    for input in &tx.inputs {
        let cell = fetch_cell(ckb_rpc_url, &input.previous_output).await?;
        mock_inputs.push(mock_input_from(input, cell));
    }

    let mut mock_cell_deps = Vec::new();
    for cell_dep in &tx.cell_deps {
        let cell = fetch_cell(ckb_rpc_url, &cell_dep.out_point).await?;
        let group_data = cell.data.clone();

        mock_cell_deps.push(mock_cell_dep_from(
            &cell_dep.out_point,
            &cell_dep.dep_type,
            cell,
        ));

        if cell_dep.dep_type == "dep_group" {
            let referenced_out_points =
                parse_dep_group_data(&group_data).map_err(CyclesError::invalid_data)?;
            for ref_out_point in referenced_out_points {
                let already_exists = mock_cell_deps.iter().any(|d| {
                    d.cell_dep.out_point.tx_hash == ref_out_point.tx_hash
                        && d.cell_dep.out_point.index == ref_out_point.index
                });
                if already_exists {
                    continue;
                }

                let ref_cell = fetch_cell(ckb_rpc_url, &ref_out_point).await?;
                mock_cell_deps.push(mock_cell_dep_from(&ref_out_point, "code", ref_cell));
            }
        }
    }

    let mut mock_headers = Vec::new();
    for header_hash in &tx.header_deps {
        let header = fetch_header(ckb_rpc_url, header_hash).await?;
        mock_headers.push(MockHeader {
            compact_target: header.compact_target,
            hash: header.hash,
            number: header.number,
            parent_hash: header.parent_hash,
            nonce: header.nonce,
            timestamp: header.timestamp,
            transactions_root: header.transactions_root,
            proposals_hash: header.proposals_hash,
            extra_hash: header.extra_hash,
            uncles_hash: header.uncles_hash,
            version: header.version,
            epoch: header.epoch,
            dao: header.dao,
        });
    }

    let transaction = Transaction {
        version: tx.version.clone(),
        cell_deps: tx
            .cell_deps
            .iter()
            .map(|cd| CellDep {
                out_point: OutPoint {
                    tx_hash: cd.out_point.tx_hash.clone(),
                    index: cd.out_point.index.clone(),
                },
                dep_type: cd.dep_type.clone(),
            })
            .collect(),
        header_deps: tx.header_deps.clone(),
        inputs: tx
            .inputs
            .iter()
            .map(|i| Input {
                previous_output: OutPoint {
                    tx_hash: i.previous_output.tx_hash.clone(),
                    index: i.previous_output.index.clone(),
                },
                since: i.since.clone(),
            })
            .collect(),
        outputs: tx
            .outputs
            .iter()
            .map(|o| Output {
                capacity: o.capacity.clone(),
                lock: Script {
                    code_hash: o.lock.code_hash.clone(),
                    hash_type: o.lock.hash_type.clone(),
                    args: o.lock.args.clone(),
                },
                type_script: o.type_script.as_ref().map(|t| Script {
                    code_hash: t.code_hash.clone(),
                    hash_type: t.hash_type.clone(),
                    args: t.args.clone(),
                }),
            })
            .collect(),
        outputs_data: tx.outputs_data.clone(),
        witnesses: tx.witnesses.clone(),
    };

    Ok(MockTransaction {
        mock_info: MockInfo {
            inputs: mock_inputs,
            cell_deps: mock_cell_deps,
            header_deps: mock_headers,
        },
        tx: transaction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn calculate_for_committed_test(
        rpc: &str,
        hash: &str,
    ) -> Result<crate::cycles_task::TransactionCycles, CyclesError> {
        let (_, status) = fetch_transaction(rpc, hash).await?;
        let block = committed_block_hash(hash, &status)?;
        let bytes: [u8; 32] = hex::decode(block.trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap();
        calculate_cycles(rpc, hash, &bytes).await
    }

    const DEPOSIT_BLOCK_HASH: &str =
        "0x41e2ea50e0557f06c6e791f75d466528bc6524d5aaefe07789d36208c0fdea7d";

    fn resolved_cell(block_hash: &str) -> ResolvedCell {
        ResolvedCell {
            output: RpcOutput {
                capacity: "0x2540be400".to_string(),
                lock: RpcScript {
                    code_hash: "0xaaa".to_string(),
                    hash_type: "type".to_string(),
                    args: "0x01".to_string(),
                },
                type_script: None,
            },
            data: "0x".to_string(),
            block_hash: block_hash.to_string(),
            transaction_info: ckb_types::core::TransactionInfo::new(
                1,
                ckb_types::core::EpochNumberWithFraction::new(1, 0, 1),
                ckb_types::packed::Byte32::from_slice(
                    &hex::decode(block_hash.trim_start_matches("0x")).unwrap(),
                )
                .unwrap(),
                1,
            ),
        }
    }

    /// The Nervos DAO type script calls `load_header(source = Input)` in both
    /// withdraw phases. Without the header association the debugger answers
    /// ItemMissing and the whole DAO group fails.
    #[test]
    fn test_mock_input_carries_header_association() {
        let input = RpcInput {
            previous_output: RpcOutPoint {
                tx_hash: "0xf398".to_string(),
                index: "0x0".to_string(),
            },
            since: "0x0".to_string(),
        };
        let mock = mock_input_from(&input, resolved_cell(DEPOSIT_BLOCK_HASH));
        assert_eq!(mock.header, DEPOSIT_BLOCK_HASH);

        // Field name and placement must match ckb_mock_tx_types::ReprMockInput.
        let json = serde_json::to_value(&mock).unwrap();
        assert_eq!(json["header"], DEPOSIT_BLOCK_HASH);
        assert_eq!(json["input"]["previous_output"]["tx_hash"], "0xf398");
    }

    #[test]
    fn test_mock_cell_dep_carries_header_association() {
        let out_point = RpcOutPoint {
            tx_hash: "0xe2fb".to_string(),
            index: "0x1".to_string(),
        };
        let mock = mock_cell_dep_from(&out_point, "code", resolved_cell(DEPOSIT_BLOCK_HASH));
        assert_eq!(mock.header, DEPOSIT_BLOCK_HASH);

        let json = serde_json::to_value(&mock).unwrap();
        assert_eq!(json["header"], DEPOSIT_BLOCK_HASH);
        assert_eq!(json["cell_dep"]["dep_type"], "code");
    }

    #[test]
    fn test_committed_block_hash_requires_a_commit_block() {
        let committed = TxStatus {
            status: "committed".to_string(),
            block_hash: Some(DEPOSIT_BLOCK_HASH.to_string()),
            tx_index: Some("0x1".into()),
        };
        assert_eq!(
            committed_block_hash("0xf398", &committed),
            Ok(DEPOSIT_BLOCK_HASH.to_string())
        );

        let pending = TxStatus {
            status: "pending".to_string(),
            block_hash: None,
            tx_index: None,
        };
        assert!(committed_block_hash("0xf398", &pending)
            .unwrap_err()
            .to_string()
            .contains("not 'committed'"));

        // Committed without a hash is a node invariant violation, not a None.
        let broken = TxStatus {
            status: "committed".to_string(),
            block_hash: None,
            tx_index: None,
        };
        assert!(committed_block_hash("0xf398", &broken)
            .unwrap_err()
            .to_string()
            .contains("no block_hash"));
    }

    #[test]
    fn test_parse_dep_group_data() {
        let data = "0x02000000e2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c03000000e2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c01000000";
        let out_points = parse_dep_group_data(data).unwrap();
        assert_eq!(out_points.len(), 2);
        assert_eq!(
            out_points[0].tx_hash,
            "0xe2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c"
        );
        assert_eq!(out_points[0].index, "0x3");
        assert_eq!(out_points[1].index, "0x1");
    }

    #[test]
    fn test_script_version_for_epoch_boundaries() {
        // Mainnet chain-truth epochs: RFC0032 at 5414, RFC0049 at 12293.
        let mainnet = VmActivationEpochs {
            vm1: 5414,
            vm2: 12293,
        };
        assert_eq!(script_version_for_epoch(5413, &mainnet), 0);
        assert_eq!(script_version_for_epoch(5414, &mainnet), 1);
        assert_eq!(script_version_for_epoch(12292, &mainnet), 1);
        assert_eq!(script_version_for_epoch(12293, &mainnet), 2);

        // vm2 not activated -> even the max representable epoch number (24 bits)
        // stays on the lower version.
        let no_vm2 = VmActivationEpochs {
            vm1: 5414,
            vm2: u64::MAX,
        };
        assert_eq!(script_version_for_epoch(0xFF_FFFF, &no_vm2), 1);

        // Nothing activated -> always VM0.
        let no_forks = VmActivationEpochs {
            vm1: u64::MAX,
            vm2: u64::MAX,
        };
        assert_eq!(script_version_for_epoch(0, &no_forks), 0);
        assert_eq!(script_version_for_epoch(0xFF_FFFF, &no_forks), 0);
    }

    /// Shaped like a real mainnet `get_consensus` result (extra fields included
    /// to prove tolerance; key order matches the live node).
    fn consensus_fixture() -> serde_json::Value {
        serde_json::json!({
            "block_version": "0x0",
            "genesis_hash": "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5",
            "hardfork_features": [
                { "epoch_number": "0x1526", "rfc": "0028" },
                { "epoch_number": "0x0", "rfc": "0029" },
                { "epoch_number": "0x0", "rfc": "0030" },
                { "epoch_number": "0x0", "rfc": "0031" },
                { "epoch_number": "0x1526", "rfc": "0032" },
                { "epoch_number": "0x0", "rfc": "0036" },
                { "epoch_number": "0x0", "rfc": "0038" },
                { "epoch_number": "0x3005", "rfc": "0048" },
                { "epoch_number": "0x3005", "rfc": "0049" }
            ],
            "id": "ckb",
            "max_block_cycles": "0xd09dc300"
        })
    }

    #[test]
    fn test_parse_vm_activation_epochs_from_consensus_fixture() {
        let epochs = parse_vm_activation_epochs(&consensus_fixture()).unwrap();
        assert_eq!(epochs.vm1, 5414); // 0x1526
        assert_eq!(epochs.vm2, 12293); // 0x3005
    }

    #[test]
    fn test_parse_vm_activation_epochs_null_epoch_means_not_activated() {
        // A chain where RFC0049 is known but not activated: epoch_number null.
        let mut consensus = consensus_fixture();
        let features = consensus["hardfork_features"].as_array_mut().unwrap();
        features[8]["epoch_number"] = serde_json::Value::Null;

        let epochs = parse_vm_activation_epochs(&consensus).unwrap();
        assert_eq!(epochs.vm1, 5414);
        assert_eq!(epochs.vm2, u64::MAX);
    }

    #[test]
    fn test_parse_vm_activation_epochs_absent_entry_means_not_activated() {
        // A chain that predates RFC0049 entirely: no "0049" entry.
        let mut consensus = consensus_fixture();
        let features = consensus["hardfork_features"].as_array_mut().unwrap();
        features.retain(|f| f["rfc"] != "0049");

        let epochs = parse_vm_activation_epochs(&consensus).unwrap();
        assert_eq!(epochs.vm1, 5414);
        assert_eq!(epochs.vm2, u64::MAX);
    }

    #[test]
    fn test_parse_vm_activation_epochs_malformed_is_error() {
        // Missing hardfork_features entirely.
        let no_features = serde_json::json!({ "id": "ckb" });
        assert!(parse_vm_activation_epochs(&no_features)
            .unwrap_err()
            .contains("hardfork_features"));

        // Non-hex epoch_number on the rfc 0032 entry.
        let mut bad_hex = consensus_fixture();
        bad_hex["hardfork_features"][4]["epoch_number"] = serde_json::json!("not-hex");
        assert!(parse_vm_activation_epochs(&bad_hex)
            .unwrap_err()
            .contains("rfc 0032"));
    }

    #[test]
    fn test_parse_epoch_number_from_header_epoch() {
        // Header epoch packs length << 40 | index << 24 | number.
        let packed: u64 = (1800u64 << 40) | (5u64 << 24) | 5414;
        let hex = format!("0x{:x}", packed);
        assert_eq!(parse_epoch_number(&hex), Ok(5414));

        // Genesis-style plain epoch.
        assert_eq!(parse_epoch_number("0x0"), Ok(0));

        // Parse failures are errors, not defaults.
        assert!(parse_epoch_number("0xzz").is_err());
        assert!(parse_epoch_number("").is_err());
    }

    /// Empirical consensus-truth validation against a local mainnet node.
    ///
    /// Requires a synced mainnet CKB node (default http://127.0.0.1:8114,
    /// override via CKBADGER_TEST_RPC) with its historical blocks available. Run with:
    /// `cargo test -p ckbadger-common -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires local mainnet CKB node"]
    async fn test_calculate_cycles_matches_consensus_across_vm_eras() {
        let rpc_url = std::env::var("CKBADGER_TEST_RPC")
            .unwrap_or_else(|_| "http://127.0.0.1:8114".to_string());

        let cases: [(&str, i64, &str); 3] = [
            // NOTE: the official explorer reports 1,700,168 for this tx, which
            // is its VM1 execution — impossible at commit time (epoch 264,
            // January 2020; RFC0032/VM1 activated at epoch 5414, May 2022).
            // Consensus verification of block 420003 ran VM0: 1,709,221.
            // ckb-debugger per version: v0=1,709,221 v1=1,700,168 v2=1,644,449.
            (
                "0x0f8a27d60818030ad7da6a35559a79fb052ffb0f4911f9b9ae04a40ee8ba0747",
                1_709_221,
                "VM0 era (epoch 264)",
            ),
            (
                "0x648c489a392b17e8345644f1612b8f112c6a1d680eff92034988b42946abceac",
                3_051_004,
                "VM1 era",
            ),
            (
                "0x32cc2f0725595cb7bdda6f97f3e737069e1f7191b37ed9c41daf01c4b921d579",
                6_895_905,
                "VM2 era",
            ),
        ];

        for (tx_hash, expected, era) in cases {
            let cycles = calculate_for_committed_test(&rpc_url, tx_hash)
                .await
                .unwrap_or_else(|e| {
                    panic!("calculate_cycles failed for {} ({}): {}", tx_hash, era, e)
                });
            cycles.validate().unwrap();
            assert!(!cycles.groups.is_empty());
            println!("{} ({}): {} cycles", tx_hash, era, cycles.cycles);
            assert_eq!(
                cycles.cycles, expected,
                "cycles mismatch for {} ({})",
                tx_hash, era
            );
        }
    }

    /// Nervos DAO withdraw phases, which are the transactions that need the
    /// header association: the DAO type script calls `load_header(Input)` for
    /// the deposit/withdraw block. Without it the DAO group aborts and its
    /// partial cycles used to be summed into the total.
    ///
    /// Expected values are consensus truth, independently reported by the
    /// official explorer. Requires a synced mainnet CKB node with its historical blocks available.
    #[tokio::test]
    #[ignore = "requires local mainnet CKB node"]
    async fn test_calculate_cycles_includes_dao_script_group() {
        let rpc_url = std::env::var("CKBADGER_TEST_RPC")
            .unwrap_or_else(|_| "http://127.0.0.1:8114".to_string());

        let cases: [(&str, i64, &str); 2] = [
            (
                // Phase 1, withdraw request (block 10457626, epoch 7995 -> VM1).
                // Broken total was 3_423_276 (DAO group aborted at 8_364).
                "0xf398adf5ff836bbdd9cf67af5557c470447c07e85601bf5d02b0f28f866a6aef",
                3_428_744,
                "DAO phase 1 (withdraw request)",
            ),
            (
                // Phase 2, completion (block 10463622, epoch 8000 -> VM1).
                // Broken total was 3_374_403 (DAO group aborted at 8_270).
                "0x6fa94cb21df82144505c5a9e5d3197e431ea0296a09c55a3e83e669f9ac01ab9",
                3_380_228,
                "DAO phase 2 (withdraw completion)",
            ),
        ];

        for (tx_hash, expected, phase) in cases {
            let cycles = calculate_for_committed_test(&rpc_url, tx_hash)
                .await
                .unwrap_or_else(|e| {
                    panic!("calculate_cycles failed for {} ({}): {}", tx_hash, phase, e)
                });
            cycles.validate().unwrap();
            assert!(cycles
                .groups
                .iter()
                .any(|group| group.group_type == crate::cycles_task::ScriptGroupKind::Type));
            println!("{} ({}): {} cycles", tx_hash, phase, cycles.cycles);
            assert_eq!(
                cycles.cycles, expected,
                "cycles mismatch for {} ({})",
                tx_hash, phase
            );
        }
    }

    /// Genesis transactions were never script-verified by consensus, so a
    /// replay legitimately fails (`Run result: -2` after 15511 cycles). The
    /// failure must surface as an error instead of being served as `done`.
    #[tokio::test]
    #[ignore = "requires local mainnet CKB node"]
    async fn test_calculate_cycles_rejects_failed_genesis_replay() {
        let rpc_url = std::env::var("CKBADGER_TEST_RPC")
            .unwrap_or_else(|_| "http://127.0.0.1:8114".to_string());

        let err = calculate_for_committed_test(
            &rpc_url,
            "0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c",
        )
        .await
        .expect_err("a failed script replay must not produce a cycle count");
        println!("genesis tx error: {}", err);
        assert!(
            matches!(err, CyclesError::Replay(ReplayFailure::InvalidChainData)),
            "unexpected error: {}",
            err
        );
    }
}
