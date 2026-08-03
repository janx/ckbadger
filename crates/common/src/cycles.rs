use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::process::Stdio;
use tokio::process::Command;
use tracing::debug;

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
}

/// Mirrors `ckb_mock_tx_types::ReprMockCellDep`; see [`MockInput::header`] for
/// the header association, which `load_header(source = CellDep)` reads back.
#[derive(Debug, Serialize)]
pub struct MockCellDep {
    pub cell_dep: CellDep,
    pub output: Output,
    pub data: String,
    pub header: String,
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

#[derive(Debug, Deserialize)]
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

pub async fn calculate_cycles(ckb_rpc_url: &str, tx_hash: &str) -> Result<i64, String> {
    let (tx, tx_status) = fetch_transaction(ckb_rpc_url, tx_hash).await?;

    // Consensus pins VM selection to the epoch of the block that COMMITTED the
    // transaction (RFC0032/RFC0049). Without a commit block there is no epoch,
    // so cycles are undefined — fail fast instead of guessing a VM.
    let block_hash = committed_block_hash(tx_hash, &tx_status)?;

    let epochs = fetch_vm_activation_epochs(ckb_rpc_url).await?;
    let header = fetch_header(ckb_rpc_url, &block_hash).await?;
    let epoch_number = parse_epoch_number(&header.epoch).map_err(|e| {
        format!(
            "invalid epoch in commit header {} for tx {}: {}",
            block_hash, tx_hash, e
        )
    })?;
    let script_version = script_version_for_epoch(epoch_number, &epochs);

    let mock_tx = build_mock_transaction(ckb_rpc_url, &tx).await?;
    run_ckb_debugger(&mock_tx, script_version).await
}

async fn fetch_transaction(
    ckb_rpc_url: &str,
    tx_hash: &str,
) -> Result<(RpcTransaction, TxStatus), String> {
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
        .map_err(|e| format!("RPC request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(err) = resp.error {
        return Err(format!("RPC error: {}", err.message));
    }

    let with_status = resp
        .result
        .ok_or_else(|| format!("Transaction {} not found", tx_hash))?;
    let status = with_status.tx_status.status.clone();
    let tx = with_status.transaction.ok_or_else(|| {
        format!(
            "Transaction {} has no transaction body (status: '{}')",
            tx_hash, status
        )
    })?;

    Ok((tx, with_status.tx_status))
}

/// The hash of the block that committed `tx_hash`.
///
/// Single source for "which block does this transaction belong to", used both
/// for consensus VM selection and for every resolved cell's header association.
/// A transaction that is not committed has no such block, so this fails fast
/// instead of substituting a placeholder.
fn committed_block_hash(tx_hash: &str, status: &TxStatus) -> Result<String, String> {
    if status.status != "committed" {
        return Err(format!(
            "transaction {} has status '{}', not 'committed'",
            tx_hash, status.status
        ));
    }
    status.block_hash.clone().ok_or_else(|| {
        format!(
            "transaction {} is 'committed' but the node returned no block_hash",
            tx_hash
        )
    })
}

/// Resolve the committing block of `tx_hash` without downloading its body.
///
/// Verbosity `0x1` returns the `tx_status` only (`transaction: null`), which is
/// all the header association needs — the genesis system-script transaction
/// alone is 2.4 MB, and cell payloads are resolved separately.
async fn fetch_committing_block_hash(ckb_rpc_url: &str, tx_hash: &str) -> Result<String, String> {
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
        .map_err(|e| format!("RPC request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(err) = resp.error {
        return Err(format!("RPC error: {}", err.message));
    }

    let with_status = resp
        .result
        .ok_or_else(|| format!("Transaction {} not found", tx_hash))?;

    committed_block_hash(tx_hash, &with_status.tx_status)
}

/// Fetch the VM activation epochs (RFC0032 -> VM1, RFC0049 -> VM2) from the
/// chain's `get_consensus` response. Chain data is the source of truth — no
/// per-network hardcoded tables.
async fn fetch_vm_activation_epochs(ckb_rpc_url: &str) -> Result<VmActivationEpochs, String> {
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
        .map_err(|e| format!("RPC request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(err) = resp.error {
        return Err(format!("RPC error: {}", err.message));
    }

    let consensus = resp
        .result
        .ok_or_else(|| "get_consensus returned no result".to_string())?;

    parse_vm_activation_epochs(&consensus)
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
fn script_version_for_epoch(epoch_number: u64, epochs: &VmActivationEpochs) -> u8 {
    if epoch_number >= epochs.vm2 {
        2
    } else if epoch_number >= epochs.vm1 {
        1
    } else {
        0
    }
}

fn parse_hex_u64(hex: &str) -> Result<u64, String> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(stripped, 16).map_err(|e| format!("invalid hex u64 '{}': {}", hex, e))
}

/// Extract the epoch number from a header `epoch` field.
///
/// The header packs `length << 40 | index << 24 | number`; the epoch number is
/// the low 24 bits.
fn parse_epoch_number(epoch_hex: &str) -> Result<u64, String> {
    Ok(parse_hex_u64(epoch_hex)? & 0xFF_FFFF)
}

/// A cell resolved from chain data, together with the block that created it.
struct ResolvedCell {
    output: RpcOutput,
    data: String,
    /// Hash of the block that committed the creating transaction; see
    /// [`MockInput::header`].
    block_hash: String,
}

/// Resolve one out point into the cell payload plus its header association.
async fn fetch_cell(ckb_rpc_url: &str, out_point: &RpcOutPoint) -> Result<ResolvedCell, String> {
    let (output, data) = fetch_cell_with_data(ckb_rpc_url, out_point).await?;
    let block_hash = fetch_committing_block_hash(ckb_rpc_url, &out_point.tx_hash)
        .await
        .map_err(|e| {
            format!(
                "cannot resolve header association for cell {}:{}: {}",
                out_point.tx_hash, out_point.index, e
            )
        })?;
    Ok(ResolvedCell {
        output,
        data,
        block_hash,
    })
}

async fn fetch_cell_with_data(
    ckb_rpc_url: &str,
    out_point: &RpcOutPoint,
) -> Result<(RpcOutput, String), String> {
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
        .map_err(|e| format!("RPC request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

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
) -> Result<(RpcOutput, String), String> {
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
        .map_err(|e| format!("RPC request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(err) = resp.error {
        return Err(format!("RPC error: {}", err.message));
    }

    let tx = resp
        .result
        .and_then(|r| r.transaction)
        .ok_or_else(|| format!("Transaction {} not found", out_point.tx_hash))?;

    let index = parse_hex_index(&out_point.index)?;
    let output = tx
        .outputs
        .get(index)
        .cloned()
        .ok_or_else(|| format!("Output index {} out of range", index))?;
    // outputs_data is 1:1 with outputs in the molecule layout; a short vector
    // means the node returned a malformed transaction, not an empty cell.
    let data = tx.outputs_data.get(index).cloned().ok_or_else(|| {
        format!(
            "transaction {} has {} outputs but only {} outputs_data entries (index {})",
            out_point.tx_hash,
            tx.outputs.len(),
            tx.outputs_data.len(),
            index
        )
    })?;

    Ok((output, data))
}

async fn fetch_header(ckb_rpc_url: &str, block_hash: &str) -> Result<RpcHeader, String> {
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
        .map_err(|e| format!("RPC request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(err) = resp.error {
        return Err(format!("RPC error: {}", err.message));
    }

    resp.result
        .ok_or_else(|| format!("Header {} not found", block_hash))
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
    }
}

async fn build_mock_transaction(
    ckb_rpc_url: &str,
    tx: &RpcTransaction,
) -> Result<MockTransaction, String> {
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
            let referenced_out_points = parse_dep_group_data(&group_data)?;
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

/// Identifies a representative cell for one script group within a mock transaction.
struct ScriptGroupRef {
    cell_index: usize,
    /// "input" or "output"
    cell_type: &'static str,
    /// "lock" or "type"
    script_group_type: &'static str,
}

fn script_identity(s: &Script) -> String {
    format!("{}:{}:{}", s.code_hash, s.hash_type, s.args)
}

/// Enumerate all unique script groups in a mock transaction.
fn enumerate_script_groups(mock_tx: &MockTransaction) -> Vec<ScriptGroupRef> {
    let mut groups = Vec::new();
    let mut seen_locks = HashSet::new();
    let mut seen_types = HashSet::new();

    // Lock groups from inputs
    for (i, mock_input) in mock_tx.mock_info.inputs.iter().enumerate() {
        let key = script_identity(&mock_input.output.lock);
        if seen_locks.insert(key) {
            groups.push(ScriptGroupRef {
                cell_index: i,
                cell_type: "input",
                script_group_type: "lock",
            });
        }
    }

    // Type groups from inputs
    for (i, mock_input) in mock_tx.mock_info.inputs.iter().enumerate() {
        if let Some(ref ts) = mock_input.output.type_script {
            let key = script_identity(ts);
            if seen_types.insert(key) {
                groups.push(ScriptGroupRef {
                    cell_index: i,
                    cell_type: "input",
                    script_group_type: "type",
                });
            }
        }
    }

    // Type groups from outputs not already seen in inputs
    for (i, output) in mock_tx.tx.outputs.iter().enumerate() {
        if let Some(ref ts) = output.type_script {
            let key = script_identity(ts);
            if seen_types.insert(key) {
                groups.push(ScriptGroupRef {
                    cell_index: i,
                    cell_type: "output",
                    script_group_type: "type",
                });
            }
        }
    }

    groups
}

/// Build the ckb-debugger argument list for one script group.
///
/// `--script-version` must always be pinned explicitly: ckb-debugger defaults
/// to VM2 for `hash_type: "type"` groups, but consensus selects the VM from the
/// commit block's epoch.
fn debugger_args_for_group(
    temp_file: &str,
    group: &ScriptGroupRef,
    script_version: u8,
) -> Vec<String> {
    vec![
        "--tx-file".to_string(),
        temp_file.to_string(),
        "--cell-index".to_string(),
        group.cell_index.to_string(),
        "--cell-type".to_string(),
        group.cell_type.to_string(),
        "--script-group-type".to_string(),
        group.script_group_type.to_string(),
        "--script-version".to_string(),
        script_version.to_string(),
    ]
}

async fn run_ckb_debugger_for_group(
    temp_file: &str,
    group: &ScriptGroupRef,
    script_version: u8,
) -> Result<i64, String> {
    let output = Command::new("ckb-debugger")
        .args(debugger_args_for_group(temp_file, group, script_version))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run ckb-debugger: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    debug!(
        group_type = group.script_group_type,
        cell_type = group.cell_type,
        cell_index = group.cell_index,
        script_version,
        "ckb-debugger stdout: {}",
        stdout
    );
    debug!("ckb-debugger stderr: {}", stderr);

    let combined = format!("{}\n{}", stdout, stderr);
    parse_group_cycles(&combined, output.status.code())
}

async fn run_ckb_debugger(mock_tx: &MockTransaction, script_version: u8) -> Result<i64, String> {
    let json = serde_json::to_string(mock_tx)
        .map_err(|e| format!("Failed to serialize mock transaction: {}", e))?;

    debug!(script_version, "Running ckb-debugger with mock transaction");

    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("ckbadger_mock_tx_{}.json", uuid::Uuid::new_v4()));

    tokio::fs::write(&temp_file, &json)
        .await
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    let temp_file_str = temp_file
        .to_str()
        .ok_or_else(|| "Temp file path contains invalid UTF-8".to_string())?;

    let groups = enumerate_script_groups(mock_tx);
    if groups.is_empty() {
        let _ = tokio::fs::remove_file(&temp_file).await;
        // Every committed non-cellbase transaction resolves at least one input
        // cell, hence at least one lock group. Zero groups means the mock
        // transaction was built wrong; reporting 0 cycles would be a lie.
        return Err(
            "mock transaction has no script groups: a committed transaction always runs \
             at least one lock script"
                .to_string(),
        );
    }

    let mut total_cycles: i64 = 0;
    for group in &groups {
        match run_ckb_debugger_for_group(temp_file_str, group, script_version).await {
            Ok(cycles) => {
                total_cycles = total_cycles
                    .checked_add(cycles)
                    .ok_or_else(|| "cycles overflow".to_string())?;
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp_file).await;
                return Err(format!(
                    "Failed to calculate cycles for {} {} group (cell {}): {}",
                    group.script_group_type, group.cell_type, group.cell_index, e
                ));
            }
        }
    }

    let _ = tokio::fs::remove_file(&temp_file).await;
    Ok(total_cycles)
}

/// The `Run result: <code>` line ckb-debugger prints for every script run.
fn parse_run_result(output: &str) -> Option<i64> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Run result:")
            .and_then(|rest| rest.trim().parse::<i64>().ok())
    })
}

/// Cycles consumed by one script group, or an error if the group did not pass.
///
/// A script group only *has* a cycle count when it succeeds: consensus rejects
/// a transaction whose script exits non-zero, so a failed run has no cycles to
/// report. ckb-debugger still prints a partial `All cycles:` line for a failed
/// run (and exits 254), so both the reported `Run result` and the child process
/// exit code must be verified before any number is harvested.
fn parse_group_cycles(output: &str, exit_code: Option<i32>) -> Result<i64, String> {
    let run_result = parse_run_result(output).ok_or_else(|| {
        format!(
            "ckb-debugger reported no `Run result` line (exit {}): {}",
            describe_exit_code(exit_code),
            summarize_output(output)
        )
    })?;

    if run_result != 0 {
        return Err(format!(
            "script group failed: ckb-debugger reported `Run result: {}` (exit {}): {}",
            run_result,
            describe_exit_code(exit_code),
            summarize_output(output)
        ));
    }

    if exit_code != Some(0) {
        return Err(format!(
            "ckb-debugger exited with {} despite `Run result: 0`: {}",
            describe_exit_code(exit_code),
            summarize_output(output)
        ));
    }

    parse_all_cycles_from_output(output)
}

fn describe_exit_code(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => code.to_string(),
        None => "signal (no exit code)".to_string(),
    }
}

fn summarize_output(output: &str) -> String {
    let trimmed = output.trim();
    let truncated: String = trimmed.chars().take(500).collect();
    let suffix = if truncated.len() < trimmed.len() {
        "... (truncated)"
    } else {
        ""
    };
    format!("{}{}", truncated.replace('\n', " | "), suffix)
}

fn parse_all_cycles_from_output(output: &str) -> Result<i64, String> {
    for line in output.lines() {
        if line.contains("All cycles:") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let cycles_part = parts[1].trim();
                let num_str = cycles_part.split('(').next().unwrap_or(cycles_part).trim();
                if let Ok(cycles) = num_str.parse::<i64>() {
                    return Ok(cycles);
                }
            }
        }
    }

    for line in output.lines() {
        if line.contains("Total cycles consumed:") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let cycles_part = parts[1].trim();
                let num_str = cycles_part.split('(').next().unwrap_or(cycles_part).trim();
                if let Ok(cycles) = num_str.parse::<i64>() {
                    return Ok(cycles);
                }
            }
        }
    }

    Err(format!(
        "Could not parse cycles from ckb-debugger output: {}",
        summarize_output(output)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_all_cycles() {
        let output = "Run result: 0\nAll cycles: 1673136(1.6M)";
        assert_eq!(parse_all_cycles_from_output(output), Ok(1673136));

        let output2 =
            "Run result: 0\nTotal cycles consumed: 7059(6.9K)\nTransfer cycles: 4537(4.4K)";
        assert_eq!(parse_all_cycles_from_output(output2), Ok(7059));
    }

    #[test]
    fn test_parse_group_cycles_accepts_successful_run() {
        // Verbatim ckb-debugger 1.0.0 output for the Nervos DAO type group of
        // mainnet tx 0x6fa94cb2... once the header association is supplied.
        let ok = "Run result: 0\nAll cycles: 14095(13.8K)\n";
        assert_eq!(parse_group_cycles(ok, Some(0)), Ok(14095));
    }

    #[test]
    fn test_parse_group_cycles_rejects_failed_run_result() {
        // Verbatim ckb-debugger 1.0.0 output for the same DAO type group when
        // `load_header(source = Input)` cannot resolve: the group aborts after
        // 8270 cycles and the process exits 254. Harvesting that partial number
        // is what made every DAO transaction report wrong cycles as `done`.
        let failed = "Run result: 2\nAll cycles: 8270(8.1K)\n";
        let err = parse_group_cycles(failed, Some(254)).unwrap_err();
        assert!(err.contains("Run result: 2"), "unexpected error: {}", err);
        assert!(
            err.contains("254"),
            "error must name the exit code: {}",
            err
        );
    }

    #[test]
    fn test_parse_group_cycles_rejects_negative_run_result() {
        // Genesis transactions: consensus never executed their scripts, and a
        // replay of the secp data-hash lock exits -2 after 15511 cycles.
        let failed = "Run result: -2\nAll cycles: 15511(15.1K)\n";
        let err = parse_group_cycles(failed, Some(254)).unwrap_err();
        assert!(err.contains("Run result: -2"), "unexpected error: {}", err);
        // The trap this guards: the cycles line alone parses cleanly, so
        // without the run-result check the aborted run reports a number.
        assert_eq!(parse_all_cycles_from_output(failed), Ok(15511));
    }

    #[test]
    fn test_parse_group_cycles_rejects_nonzero_exit_without_run_result() {
        // A crashed/aborted debugger prints no `Run result` at all.
        let err = parse_group_cycles("thread 'main' panicked at ...\n", Some(101)).unwrap_err();
        assert!(err.contains("no `Run result`"), "unexpected error: {}", err);
    }

    #[test]
    fn test_parse_group_cycles_rejects_nonzero_exit_code() {
        // Successful-looking output plus a non-zero exit is still a failure.
        let err = parse_group_cycles("Run result: 0\nAll cycles: 100(100)\n", Some(1)).unwrap_err();
        assert!(err.contains("exited with 1"), "unexpected error: {}", err);
    }

    #[test]
    fn test_parse_group_cycles_rejects_signal_termination() {
        let err = parse_group_cycles("Run result: 0\nAll cycles: 100(100)\n", None).unwrap_err();
        assert!(err.contains("signal"), "unexpected error: {}", err);
    }

    #[test]
    fn test_summarize_output_truncates_on_char_boundary() {
        // Debugger output can carry multi-byte characters; byte slicing would
        // panic mid-character.
        let noisy = "Run result: 2\n".to_string() + &"✗".repeat(600);
        let summary = summarize_output(&noisy);
        assert!(summary.ends_with("... (truncated)"));
        assert!(summary.starts_with("Run result: 2 | "));
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
        };
        assert_eq!(
            committed_block_hash("0xf398", &committed),
            Ok(DEPOSIT_BLOCK_HASH.to_string())
        );

        let pending = TxStatus {
            status: "pending".to_string(),
            block_hash: None,
        };
        assert!(committed_block_hash("0xf398", &pending)
            .unwrap_err()
            .contains("not 'committed'"));

        // Committed without a hash is a node invariant violation, not a None.
        let broken = TxStatus {
            status: "committed".to_string(),
            block_hash: None,
        };
        assert!(committed_block_hash("0xf398", &broken)
            .unwrap_err()
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
    fn test_debugger_args_include_script_version() {
        let group = ScriptGroupRef {
            cell_index: 3,
            cell_type: "input",
            script_group_type: "lock",
        };
        let args = debugger_args_for_group("/tmp/mock_tx.json", &group, 1);
        let pos = args
            .iter()
            .position(|a| a == "--script-version")
            .unwrap_or_else(|| {
                panic!(
                    "ckb-debugger args must pin --script-version (consensus VM selection \
                     is defined by the commit block's epoch; without it ckb-debugger \
                     defaults to VM2 for every group), got: {:?}",
                    args
                )
            });
        assert_eq!(
            args[pos + 1],
            "1",
            "mapped script version must follow the flag"
        );

        // The pre-existing group args must survive the refactor.
        for expected in [
            "--tx-file",
            "/tmp/mock_tx.json",
            "--cell-index",
            "3",
            "--cell-type",
            "input",
            "--script-group-type",
            "lock",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "missing arg {}",
                expected
            );
        }
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
    /// override via CKBADGER_TEST_RPC) plus ckb-debugger on PATH. Run with:
    /// `cargo test -p ckbadger-common -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires local mainnet CKB node + ckb-debugger binary"]
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
            let cycles = calculate_cycles(&rpc_url, tx_hash)
                .await
                .unwrap_or_else(|e| {
                    panic!("calculate_cycles failed for {} ({}): {}", tx_hash, era, e)
                });
            println!("{} ({}): {} cycles", tx_hash, era, cycles);
            assert_eq!(
                cycles, expected,
                "cycles mismatch for {} ({})",
                tx_hash, era
            );
        }
    }

    /// Zero script groups used to be reported as 0 cycles, which the API then
    /// serves as "pending" forever. A committed transaction always resolves at
    /// least one input cell, so no groups means the mock transaction is wrong.
    #[tokio::test]
    async fn test_run_ckb_debugger_rejects_mock_tx_without_script_groups() {
        let mock_tx = MockTransaction {
            mock_info: MockInfo {
                inputs: vec![],
                cell_deps: vec![],
                header_deps: vec![],
            },
            tx: Transaction {
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![],
                outputs: vec![],
                outputs_data: vec![],
                witnesses: vec![],
            },
        };
        let err = run_ckb_debugger(&mock_tx, 1)
            .await
            .expect_err("no script groups must not report 0 cycles");
        assert!(
            err.contains("no script groups"),
            "unexpected error: {}",
            err
        );
    }

    /// Nervos DAO withdraw phases, which are the transactions that need the
    /// header association: the DAO type script calls `load_header(Input)` for
    /// the deposit/withdraw block. Without it the DAO group aborts and its
    /// partial cycles used to be summed into the total.
    ///
    /// Expected values are consensus truth, independently reported by the
    /// official explorer. Requires a synced mainnet CKB node plus ckb-debugger.
    #[tokio::test]
    #[ignore = "requires local mainnet CKB node + ckb-debugger binary"]
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
            let cycles = calculate_cycles(&rpc_url, tx_hash)
                .await
                .unwrap_or_else(|e| {
                    panic!("calculate_cycles failed for {} ({}): {}", tx_hash, phase, e)
                });
            println!("{} ({}): {} cycles", tx_hash, phase, cycles);
            assert_eq!(
                cycles, expected,
                "cycles mismatch for {} ({})",
                tx_hash, phase
            );
        }
    }

    /// Genesis transactions were never script-verified by consensus, so a
    /// replay legitimately fails (`Run result: -2` after 15511 cycles). The
    /// failure must surface as an error instead of being served as `done`.
    #[tokio::test]
    #[ignore = "requires local mainnet CKB node + ckb-debugger binary"]
    async fn test_calculate_cycles_rejects_failed_genesis_replay() {
        let rpc_url = std::env::var("CKBADGER_TEST_RPC")
            .unwrap_or_else(|_| "http://127.0.0.1:8114".to_string());

        let err = calculate_cycles(
            &rpc_url,
            "0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c",
        )
        .await
        .expect_err("a failed script replay must not produce a cycle count");
        println!("genesis tx error: {}", err);
        assert!(err.contains("Run result: -2"), "unexpected error: {}", err);
    }

    #[test]
    fn test_enumerate_script_groups_single_lock() {
        let lock = Script {
            code_hash: "0xaaa".to_string(),
            hash_type: "type".to_string(),
            args: "0x01".to_string(),
        };
        let mock_tx = MockTransaction {
            mock_info: MockInfo {
                inputs: vec![MockInput {
                    input: Input {
                        previous_output: OutPoint {
                            tx_hash: "0x00".to_string(),
                            index: "0x0".to_string(),
                        },
                        since: "0x0".to_string(),
                    },
                    output: Output {
                        capacity: "0x100".to_string(),
                        lock: lock.clone(),
                        type_script: None,
                    },
                    data: "0x".to_string(),
                    header: DEPOSIT_BLOCK_HASH.to_string(),
                }],
                cell_deps: vec![],
                header_deps: vec![],
            },
            tx: Transaction {
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![],
                outputs: vec![],
                outputs_data: vec![],
                witnesses: vec![],
            },
        };
        let groups = enumerate_script_groups(&mock_tx);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].script_group_type, "lock");
    }

    #[test]
    fn test_enumerate_script_groups_deduplicates_same_lock() {
        let lock = Script {
            code_hash: "0xaaa".to_string(),
            hash_type: "type".to_string(),
            args: "0x01".to_string(),
        };
        let make_input = |idx: &str| MockInput {
            input: Input {
                previous_output: OutPoint {
                    tx_hash: "0x00".to_string(),
                    index: idx.to_string(),
                },
                since: "0x0".to_string(),
            },
            output: Output {
                capacity: "0x100".to_string(),
                lock: lock.clone(),
                type_script: None,
            },
            data: "0x".to_string(),
            header: DEPOSIT_BLOCK_HASH.to_string(),
        };
        let mock_tx = MockTransaction {
            mock_info: MockInfo {
                inputs: vec![make_input("0x0"), make_input("0x1")],
                cell_deps: vec![],
                header_deps: vec![],
            },
            tx: Transaction {
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![],
                outputs: vec![],
                outputs_data: vec![],
                witnesses: vec![],
            },
        };
        let groups = enumerate_script_groups(&mock_tx);
        assert_eq!(groups.len(), 1); // Same lock -> one group
    }

    #[test]
    fn test_enumerate_script_groups_lock_and_type() {
        let lock = Script {
            code_hash: "0xaaa".to_string(),
            hash_type: "type".to_string(),
            args: "0x01".to_string(),
        };
        let type_s = Script {
            code_hash: "0xbbb".to_string(),
            hash_type: "data".to_string(),
            args: "0x02".to_string(),
        };
        let mock_tx = MockTransaction {
            mock_info: MockInfo {
                inputs: vec![MockInput {
                    input: Input {
                        previous_output: OutPoint {
                            tx_hash: "0x00".to_string(),
                            index: "0x0".to_string(),
                        },
                        since: "0x0".to_string(),
                    },
                    output: Output {
                        capacity: "0x100".to_string(),
                        lock: lock.clone(),
                        type_script: Some(type_s.clone()),
                    },
                    data: "0x".to_string(),
                    header: DEPOSIT_BLOCK_HASH.to_string(),
                }],
                cell_deps: vec![],
                header_deps: vec![],
            },
            tx: Transaction {
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![],
                outputs: vec![],
                outputs_data: vec![],
                witnesses: vec![],
            },
        };
        let groups = enumerate_script_groups(&mock_tx);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].script_group_type, "lock");
        assert_eq!(groups[1].script_group_type, "type");
    }

    #[test]
    fn test_enumerate_script_groups_output_only_type() {
        let lock = Script {
            code_hash: "0xaaa".to_string(),
            hash_type: "type".to_string(),
            args: "0x01".to_string(),
        };
        let type_out = Script {
            code_hash: "0xccc".to_string(),
            hash_type: "data".to_string(),
            args: "0x03".to_string(),
        };
        let mock_tx = MockTransaction {
            mock_info: MockInfo {
                inputs: vec![MockInput {
                    input: Input {
                        previous_output: OutPoint {
                            tx_hash: "0x00".to_string(),
                            index: "0x0".to_string(),
                        },
                        since: "0x0".to_string(),
                    },
                    output: Output {
                        capacity: "0x100".to_string(),
                        lock: lock.clone(),
                        type_script: None,
                    },
                    data: "0x".to_string(),
                    header: DEPOSIT_BLOCK_HASH.to_string(),
                }],
                cell_deps: vec![],
                header_deps: vec![],
            },
            tx: Transaction {
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![],
                outputs: vec![Output {
                    capacity: "0x100".to_string(),
                    lock: lock.clone(),
                    type_script: Some(type_out.clone()),
                }],
                outputs_data: vec!["0x".to_string()],
                witnesses: vec![],
            },
        };
        let groups = enumerate_script_groups(&mock_tx);
        assert_eq!(groups.len(), 2); // 1 lock (input) + 1 type (output only)
        assert_eq!(groups[1].cell_type, "output");
        assert_eq!(groups[1].cell_index, 0);
    }
}
