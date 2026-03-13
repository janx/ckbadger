use ckb_jsonrpc_types::{
    Either, ResponseFormat, Status, TransactionView as JsonTransactionView,
    TransactionWithStatusResponse,
};
use ckb_store_reader::{
    RpcCellDep, RpcCellInput, RpcCellOutput, RpcOutPoint, RpcScript, RpcTransactionView,
};
use ckb_types::prelude::Entity;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

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

#[derive(Debug, Clone)]
pub(crate) struct TransactionLookup {
    pub status: Status,
    pub transaction: Option<RpcTransactionView>,
    pub cycles: Option<u64>,
    pub fee: Option<u64>,
    pub time_added_to_pool: Option<u64>,
    pub tx_size: Option<i32>,
}

impl TransactionLookup {
    pub fn status_str(&self) -> &'static str {
        match self.status {
            Status::Pending => "pending",
            Status::Proposed => "proposed",
            Status::Committed => "committed",
            Status::Unknown => "unknown",
            Status::Rejected => "rejected",
        }
    }

    pub fn is_pending_like(&self) -> bool {
        matches!(self.status, Status::Pending | Status::Proposed)
    }

    pub fn pending_label(&self) -> &'static str {
        match self.status {
            Status::Pending => "Pending Transaction",
            Status::Proposed => "Proposed Transaction",
            Status::Committed => "Committed Transaction",
            Status::Unknown => "Unknown Transaction",
            Status::Rejected => "Rejected Transaction",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcTransactionView {
    hash: String,
    version: String,
    cell_deps: Vec<JsonRpcCellDep>,
    header_deps: Vec<String>,
    inputs: Vec<JsonRpcCellInput>,
    outputs: Vec<JsonRpcCellOutput>,
    outputs_data: Vec<String>,
    witnesses: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcCellDep {
    out_point: JsonRpcOutPoint,
    dep_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcOutPoint {
    tx_hash: String,
    index: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcCellInput {
    since: String,
    previous_output: JsonRpcOutPoint,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcCellOutput {
    capacity: String,
    lock: JsonRpcScript,
    #[serde(rename = "type")]
    type_: Option<JsonRpcScript>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcScript {
    code_hash: String,
    hash_type: String,
    args: String,
}

fn convert_transaction_view(
    transaction: JsonTransactionView,
) -> Result<(RpcTransactionView, i32), String> {
    let packed_tx: ckb_types::packed::Transaction = transaction.inner.clone().into();
    let tx_size = i32::try_from(packed_tx.as_slice().len())
        .map_err(|_| "transaction serialized size exceeds i32 range".to_string())?;

    let value =
        serde_json::to_value(&transaction).map_err(|e| format!("serialize rpc tx view: {}", e))?;
    let json_tx: JsonRpcTransactionView =
        serde_json::from_value(value).map_err(|e| format!("deserialize rpc tx view: {}", e))?;

    Ok((
        RpcTransactionView {
            hash: json_tx.hash,
            version: json_tx.version,
            cell_deps: json_tx
                .cell_deps
                .into_iter()
                .map(|dep| RpcCellDep {
                    out_point: RpcOutPoint {
                        tx_hash: dep.out_point.tx_hash,
                        index: dep.out_point.index,
                    },
                    dep_type: dep.dep_type,
                })
                .collect(),
            header_deps: json_tx.header_deps,
            inputs: json_tx
                .inputs
                .into_iter()
                .map(|input| RpcCellInput {
                    since: input.since,
                    previous_output: RpcOutPoint {
                        tx_hash: input.previous_output.tx_hash,
                        index: input.previous_output.index,
                    },
                })
                .collect(),
            outputs: json_tx
                .outputs
                .into_iter()
                .map(|output| RpcCellOutput {
                    capacity: output.capacity,
                    lock: RpcScript {
                        code_hash: output.lock.code_hash,
                        hash_type: output.lock.hash_type,
                        args: output.lock.args,
                    },
                    type_: output.type_.map(|script| RpcScript {
                        code_hash: script.code_hash,
                        hash_type: script.hash_type,
                        args: script.args,
                    }),
                })
                .collect(),
            outputs_data: json_tx.outputs_data,
            witnesses: json_tx.witnesses,
        },
        tx_size,
    ))
}

pub(crate) async fn fetch_transaction_lookup(
    url: &str,
    hash: &str,
) -> Result<Option<TransactionLookup>, String> {
    let client = get_http_client();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "get_transaction",
        params: (hash,),
        id: 1,
    };

    let response = client
        .post(url)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<RpcResponse<TransactionWithStatusResponse>>()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(error) = response.error {
        return Err(format!("RPC error {}: {}", error.code, error.message));
    }

    let Some(result) = response.result else {
        return Ok(None);
    };

    let (transaction, tx_size) = match result.transaction {
        Some(ResponseFormat {
            inner: Either::Left(transaction),
        }) => {
            let (transaction, tx_size) = convert_transaction_view(transaction)?;
            (Some(transaction), Some(tx_size))
        }
        Some(ResponseFormat {
            inner: Either::Right(_),
        }) => {
            return Err(
                "get_transaction returned hex transaction data; JSON transaction view required"
                    .to_string(),
            );
        }
        None => (None, None),
    };

    Ok(Some(TransactionLookup {
        status: result.tx_status.status,
        transaction,
        cycles: result.cycles.map(Into::into),
        fee: result.fee.map(Into::into),
        time_added_to_pool: result.time_added_to_pool.map(Into::into),
        tx_size,
    }))
}

pub(crate) fn pending_transaction_resource_error(
    hash: &str,
    status: &str,
    resource: &str,
) -> String {
    format!(
        "Transaction {} is {}. {} is unavailable until it is committed",
        hash, status, resource
    )
}
