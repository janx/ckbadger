use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "backfill-cycles")]
#[command(about = "Backfill transaction cycles from CKB RPC")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, env = "CKB_RPC_URL", default_value = "http://127.0.0.1:8114")]
    ckb_rpc_url: String,

    #[arg(long, default_value = "100")]
    batch_size: i64,

    #[arg(long)]
    start_block: Option<i64>,

    #[arg(long)]
    end_block: Option<i64>,
}

#[derive(serde::Serialize)]
struct RpcRequest<T> {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: T,
}

#[derive(serde::Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
}

#[derive(serde::Deserialize)]
struct BlockResponseWithCycles {
    block: BlockData,
    #[serde(default)]
    cycles: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
struct BlockData {
    transactions: Vec<TransactionView>,
    #[serde(flatten)]
    _extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct TransactionView {
    hash: String,
    #[serde(flatten)]
    _extra: std::collections::HashMap<String, serde_json::Value>,
}

fn parse_hex_u64(hex: &str) -> u64 {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(hex, 16).unwrap_or(0)
}

async fn get_block_with_cycles(
    client: &reqwest::Client,
    rpc_url: &str,
    block_number: i64,
) -> Result<Option<BlockResponseWithCycles>> {
    let request = RpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method: "get_block_by_number",
        params: (format!("0x{:x}", block_number), Some("0x2"), Some(true)),
    };

    for attempt in 0..3 {
        let result = client.post(rpc_url).json(&request).send().await;

        let response = match result {
            Ok(r) => r,
            Err(e) => {
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                    continue;
                }
                return Err(e.into());
            }
        };

        let response_text = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                    continue;
                }
                return Err(e.into());
            }
        };

        match serde_json::from_str::<RpcResponse<BlockResponseWithCycles>>(&response_text) {
            Ok(r) => return Ok(r.result),
            Err(e) => {
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                    continue;
                }
                let preview = if response_text.len() > 200 {
                    format!("{}...", &response_text[..200])
                } else {
                    response_text.clone()
                };
                return Err(anyhow::anyhow!(
                    "JSON parse error for block {}: {} | Response preview: {}",
                    block_number,
                    e,
                    preview
                ));
            }
        }
    }
    unreachable!()
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("backfill_cycles=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    info!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&args.database_url)
        .await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let (min_block, max_block): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(MIN(block_number), 0), COALESCE(MAX(block_number), 0) FROM transactions WHERE cycles IS NULL AND NOT is_cellbase",
    )
    .fetch_one(&pool)
    .await?;

    let start_block = args.start_block.unwrap_or(min_block);
    let end_block = args.end_block.unwrap_or(max_block);

    info!(
        "Backfilling cycles from block {} to {}",
        start_block, end_block
    );

    let total_null: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM transactions WHERE cycles IS NULL AND NOT is_cellbase AND block_number BETWEEN $1 AND $2",
    )
    .bind(start_block)
    .bind(end_block)
    .fetch_one(&pool)
    .await?;

    info!("Total transactions with NULL cycles: {}", total_null.0);

    let mut current_block = start_block;
    let mut total_updated = 0i64;

    while current_block <= end_block {
        let batch_end = (current_block + args.batch_size - 1).min(end_block);

        let blocks_to_process: Vec<(i64, Vec<u8>)> = sqlx::query_as(
            r#"
            SELECT DISTINCT b.number, b.hash
            FROM blocks b
            JOIN transactions t ON t.block_number = b.number
            WHERE t.cycles IS NULL AND NOT t.is_cellbase
              AND b.number BETWEEN $1 AND $2
            ORDER BY b.number
            "#,
        )
        .bind(current_block)
        .bind(batch_end)
        .fetch_all(&pool)
        .await?;

        for (block_number, _block_hash) in blocks_to_process {
            match get_block_with_cycles(&client, &args.ckb_rpc_url, block_number).await {
                Ok(Some(response)) => {
                    if let Some(cycles) = response.cycles {
                        for (idx, cycle_hex) in cycles.iter().enumerate() {
                            let tx_index = idx + 1;
                            if tx_index < response.block.transactions.len() {
                                let tx_hash = &response.block.transactions[tx_index].hash;
                                let tx_hash_bytes =
                                    hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash))
                                        .unwrap_or_default();
                                let cycles_val = parse_hex_u64(cycle_hex) as i64;

                                sqlx::query(
                                    "UPDATE transactions SET cycles = $1 WHERE hash = $2 AND cycles IS NULL",
                                )
                                .bind(cycles_val)
                                .bind(&tx_hash_bytes)
                                .execute(&pool)
                                .await?;

                                total_updated += 1;
                            }
                        }
                    }
                }
                Ok(None) => {
                    warn!("Block {} not found", block_number);
                }
                Err(e) => {
                    warn!("Error fetching block {}: {}", block_number, e);
                }
            }
        }

        if batch_end % 10000 == 0 || batch_end == end_block {
            info!(
                "Progress: block {} / {}, updated {} transactions",
                batch_end, end_block, total_updated
            );
        }

        current_block = batch_end + 1;
    }

    info!(
        "Backfill complete. Updated {} transactions with cycles data.",
        total_updated
    );

    Ok(())
}
