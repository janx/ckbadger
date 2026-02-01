use serde::{Deserialize, Serialize};
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

#[derive(Debug, Serialize)]
pub struct MockInput {
    pub input: Input,
    pub output: Output,
    pub data: String,
}

#[derive(Debug, Serialize)]
pub struct MockCellDep {
    pub cell_dep: CellDep,
    pub output: Output,
    pub data: String,
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

pub async fn calculate_cycles(ckb_rpc_url: &str, tx_hash: &str) -> Result<i64, String> {
    let tx = fetch_transaction(ckb_rpc_url, tx_hash).await?;
    let mock_tx = build_mock_transaction(ckb_rpc_url, &tx).await?;
    run_ckb_debugger(&mock_tx).await
}

async fn fetch_transaction(ckb_rpc_url: &str, tx_hash: &str) -> Result<RpcTransaction, String> {
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

    resp.result
        .and_then(|r| r.transaction)
        .ok_or_else(|| "Transaction not found".to_string())
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
    let data = tx
        .outputs_data
        .get(index)
        .cloned()
        .unwrap_or_else(|| "0x".to_string());

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

async fn build_mock_transaction(
    ckb_rpc_url: &str,
    tx: &RpcTransaction,
) -> Result<MockTransaction, String> {
    let mut mock_inputs = Vec::new();
    for input in &tx.inputs {
        let (output, data) = fetch_cell_with_data(ckb_rpc_url, &input.previous_output).await?;
        mock_inputs.push(MockInput {
            input: Input {
                previous_output: OutPoint {
                    tx_hash: input.previous_output.tx_hash.clone(),
                    index: input.previous_output.index.clone(),
                },
                since: input.since.clone(),
            },
            output: Output {
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
            },
            data,
        });
    }

    let mut mock_cell_deps = Vec::new();
    for cell_dep in &tx.cell_deps {
        let (output, data) = fetch_cell_with_data(ckb_rpc_url, &cell_dep.out_point).await?;

        mock_cell_deps.push(MockCellDep {
            cell_dep: CellDep {
                out_point: OutPoint {
                    tx_hash: cell_dep.out_point.tx_hash.clone(),
                    index: cell_dep.out_point.index.clone(),
                },
                dep_type: cell_dep.dep_type.clone(),
            },
            output: Output {
                capacity: output.capacity.clone(),
                lock: Script {
                    code_hash: output.lock.code_hash.clone(),
                    hash_type: output.lock.hash_type.clone(),
                    args: output.lock.args.clone(),
                },
                type_script: output.type_script.clone().map(|t| Script {
                    code_hash: t.code_hash,
                    hash_type: t.hash_type,
                    args: t.args,
                }),
            },
            data: data.clone(),
        });

        if cell_dep.dep_type == "dep_group" {
            let referenced_out_points = parse_dep_group_data(&data)?;
            for ref_out_point in referenced_out_points {
                let already_exists = mock_cell_deps.iter().any(|d| {
                    d.cell_dep.out_point.tx_hash == ref_out_point.tx_hash
                        && d.cell_dep.out_point.index == ref_out_point.index
                });
                if already_exists {
                    continue;
                }

                let (ref_output, ref_data) =
                    fetch_cell_with_data(ckb_rpc_url, &ref_out_point).await?;

                mock_cell_deps.push(MockCellDep {
                    cell_dep: CellDep {
                        out_point: OutPoint {
                            tx_hash: ref_out_point.tx_hash,
                            index: ref_out_point.index,
                        },
                        dep_type: "code".to_string(),
                    },
                    output: Output {
                        capacity: ref_output.capacity,
                        lock: Script {
                            code_hash: ref_output.lock.code_hash,
                            hash_type: ref_output.lock.hash_type,
                            args: ref_output.lock.args,
                        },
                        type_script: ref_output.type_script.map(|t| Script {
                            code_hash: t.code_hash,
                            hash_type: t.hash_type,
                            args: t.args,
                        }),
                    },
                    data: ref_data,
                });
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

async fn run_ckb_debugger(mock_tx: &MockTransaction) -> Result<i64, String> {
    let json = serde_json::to_string(mock_tx)
        .map_err(|e| format!("Failed to serialize mock transaction: {}", e))?;

    debug!("Running ckb-debugger with mock transaction");

    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("ckbadger_mock_tx_{}.json", uuid::Uuid::new_v4()));

    tokio::fs::write(&temp_file, &json)
        .await
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    let temp_file_str = temp_file
        .to_str()
        .ok_or_else(|| "Temp file path contains invalid UTF-8".to_string())?;

    let output = Command::new("ckb-debugger")
        .args([
            "--tx-file",
            temp_file_str,
            "--cell-index",
            "0",
            "--cell-type",
            "input",
            "--script-group-type",
            "lock",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run ckb-debugger: {}", e))?;

    let _ = tokio::fs::remove_file(&temp_file).await;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    debug!("ckb-debugger stdout: {}", stdout);
    debug!("ckb-debugger stderr: {}", stderr);

    let combined = format!("{}\n{}", stdout, stderr);
    parse_all_cycles_from_output(&combined)
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

    let truncated_output = if output.len() > 500 {
        format!("{}... (truncated)", &output[..500])
    } else {
        output.to_string()
    };
    Err(format!(
        "Could not parse cycles from ckb-debugger output: {}",
        truncated_output.replace('\n', " | ")
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
}
