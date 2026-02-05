use anyhow::{anyhow, Result};
use ckbadger_common::{parse_hex_to_bytes, RateCalculator, SporeRebuildConfig, SporeRebuildResult};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::TaskDb;

const CLUSTER_CODE_HASH_MAINNET_V2: &str =
    "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075";
const CLUSTER_CODE_HASH_TESTNET_V2: &str =
    "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058";
const CLUSTER_CODE_HASH_TESTNET_V1: &str =
    "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080";

const SPORE_CODE_HASH_MAINNET_V2: &str =
    "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5";
const SPORE_CODE_HASH_MAINNET_DID: &str =
    "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33";
const SPORE_CODE_HASH_TESTNET_V2: &str =
    "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d";
const SPORE_CODE_HASH_TESTNET_V1: &str =
    "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494";

const RPC_BATCH_SIZE: usize = 250;
const RPC_CONCURRENT: usize = 32;
const HTTP_TIMEOUT_SECS: u64 = 60;
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;
const RETRY_ATTEMPTS: usize = 3;
const RETRY_BACKOFF_MS: u64 = 500;

#[derive(Debug, Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RpcTransaction {
    outputs_data: Vec<String>,
}

type ClusterCellRow = (
    i64,       // created_at_block
    i64,       // id
    Vec<u8>,   // tx_hash
    Vec<u8>,   // type_script_hash
    Vec<u8>,   // type_args (= cluster_id)
    Vec<u8>,   // lock_script_hash
    Option<Vec<u8>>, // data
    i16,       // status
);

type SporeCellRow = (
    i64,              // created_at_block
    i64,              // id
    Vec<u8>,          // tx_hash
    i16,              // output_index
    Vec<u8>,          // type_script_hash
    Vec<u8>,          // type_args (= spore_id)
    Vec<u8>,          // lock_script_hash
    Option<Vec<u8>>,  // data
    i32,              // data_size
    i16,              // status
    Option<i64>,      // consumed_at_block
    Option<Vec<u8>>,  // consumed_by_tx
);

struct ClusterInfo {
    cluster_id: Vec<u8>,
    type_script_hash: Vec<u8>,
    owner_lock_hash: Vec<u8>,
    name: Option<String>,
    description: Option<String>,
    created_at_block: i64,
    created_at_tx: Vec<u8>,
}

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &SporeRebuildConfig,
) -> Result<()> {
    info!(
        "Starting spore rebuild task (batch_size={}, rpc={})",
        config.batch_size,
        if config.ckb_rpc_url.is_empty() {
            "<none>"
        } else {
            &config.ckb_rpc_url
        }
    );

    let mut result = SporeRebuildResult::default();

    db.update_progress(task_id, 0, 100, Some("Phase 1: Truncating spore tables..."), None)
        .await?;

    sqlx::query("TRUNCATE TABLE spore_content CASCADE")
        .execute(pool)
        .await?;
    sqlx::query("TRUNCATE TABLE spore_cells CASCADE")
        .execute(pool)
        .await?;
    sqlx::query("TRUNCATE TABLE spore_clusters CASCADE")
        .execute(pool)
        .await?;
    info!("Truncated spore_content, spore_cells, spore_clusters");

    let cluster_code_hashes: Vec<Vec<u8>> = vec![
        parse_hex_to_bytes(CLUSTER_CODE_HASH_MAINNET_V2),
        parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V2),
        parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V1),
    ];

    let total_cluster_cells: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cells
        WHERE type_code_hash = ANY($1)
          AND type_hash_type IS NOT NULL
          AND type_script_hash IS NOT NULL
          AND type_args IS NOT NULL
        "#,
    )
    .bind(&cluster_code_hashes)
    .fetch_one(pool)
    .await?;

    info!("Total cluster cells to scan: {}", total_cluster_cells);

    db.update_progress(
        task_id,
        0,
        total_cluster_cells,
        Some("Phase 1: Scanning cluster cells..."),
        None,
    )
    .await?;

    let clusters_created = rebuild_clusters(db, pool, task_id, config, &cluster_code_hashes, total_cluster_cells).await?;
    result.clusters_updated = clusters_created;
    info!("Phase 1 complete: {} clusters created", clusters_created);

    let spore_code_hashes: Vec<Vec<u8>> = vec![
        parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2),
        parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_DID),
        parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V2),
        parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V1),
    ];

    let total_spore_cells: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cells
        WHERE type_code_hash = ANY($1)
          AND type_hash_type IS NOT NULL
          AND type_script_hash IS NOT NULL
          AND type_args IS NOT NULL
        "#,
    )
    .bind(&spore_code_hashes)
    .fetch_one(pool)
    .await?;

    info!("Total spore cells to scan: {}", total_spore_cells);

    db.update_progress(
        task_id,
        0,
        total_spore_cells,
        Some("Phase 2: Scanning spore cells..."),
        None,
    )
    .await?;

    let spores_inserted = rebuild_spore_cells(db, pool, task_id, config, &spore_code_hashes, total_spore_cells).await?;
    result.spores_processed = spores_inserted;
    info!("Phase 2 complete: {} spore cells created", spores_inserted);

    if config.ckb_rpc_url.is_empty() {
        warn!("Phase 3 skipped: no ckb_rpc_url configured, cluster_id will remain NULL for spores with large content");
    } else {
        db.update_progress(
            task_id,
            0,
            100,
            Some("Phase 3: Resolving cluster_ids via RPC..."),
            None,
        )
        .await?;

        let resolved = resolve_cluster_ids_via_rpc(db, pool, task_id, config).await;
        match resolved {
            Ok(count) => {
                result.spores_marked_consumed = count;
                info!("Phase 3 complete: {} cluster_ids resolved via RPC", count);
            }
            Err(e) => {
                warn!("Phase 3 failed (non-fatal): {}. Continuing with remaining phases.", e);
            }
        }
    }

    db.update_progress(
        task_id,
        0,
        100,
        Some("Phase 4: Updating cluster spore counts..."),
        None,
    )
    .await?;

    let updated_clusters = sqlx::query(
        r#"
        UPDATE spore_clusters sc
        SET spores_count = COALESCE(sub.cnt, 0),
            updated_at = NOW()
        FROM (
            SELECT cluster_id, COUNT(*)::int AS cnt
            FROM spore_cells
            WHERE cluster_id IS NOT NULL AND is_live = TRUE
            GROUP BY cluster_id
        ) sub
        WHERE sc.cluster_id = sub.cluster_id
          AND sc.spores_count != COALESCE(sub.cnt, 0)
        "#,
    )
    .execute(pool)
    .await?;
    info!(
        "Updated spores_count for {} clusters",
        updated_clusters.rows_affected()
    );

    sqlx::query(
        "UPDATE sync_status SET spore_deferred = FALSE, spore_rebuild_completed_at = NOW() WHERE id = 1",
    )
    .execute(pool)
    .await?;

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "Spore rebuild completed: {} clusters, {} spores, {} cluster_ids resolved",
        result.clusters_updated, result.spores_processed, result.spores_marked_consumed
    );

    Ok(())
}

async fn rebuild_clusters(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &SporeRebuildConfig,
    cluster_code_hashes: &[Vec<u8>],
    total_cells: i64,
) -> Result<i64> {
    let batch_size = config.batch_size as i64;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;
    let mut processed: i64 = 0;
    let mut rate_calc = RateCalculator::default();

    let mut clusters_map: HashMap<Vec<u8>, ClusterInfo> = HashMap::new();

    loop {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled during Phase 1");
            return Ok(0);
        }

        let rows: Vec<ClusterCellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, type_script_hash, type_args,
                   lock_script_hash, data, status
            FROM cells
            WHERE type_code_hash = ANY($1)
              AND type_hash_type IS NOT NULL
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(cluster_code_hashes)
        .bind(last_block)
        .bind(last_id)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let batch_count = rows.len() as i64;
        if let Some(last_row) = rows.last() {
            last_block = last_row.0;
            last_id = last_row.1;
        }

        for (created_at_block, _id, tx_hash, type_script_hash, type_args, lock_script_hash, data, status) in &rows {
            let cluster_id = type_args.clone();

            let (name, description) = data
                .as_deref()
                .and_then(parse_cluster_data)
                .unwrap_or((None, None));

            let entry = clusters_map.entry(cluster_id.clone());
            match entry {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(ClusterInfo {
                        cluster_id,
                        type_script_hash: type_script_hash.clone(),
                        owner_lock_hash: lock_script_hash.clone(),
                        name,
                        description,
                        created_at_block: *created_at_block,
                        created_at_tx: tx_hash.clone(),
                    });
                }
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    if *status == 0 {
                        let info = e.get_mut();
                        info.owner_lock_hash = lock_script_hash.clone();
                        if name.is_some() {
                            info.name = name;
                        }
                        if description.is_some() {
                            info.description = description;
                        }
                    }
                }
            }
        }

        processed += batch_count;
        rate_calc.add_sample(processed);
        db.update_progress(
            task_id,
            processed,
            total_cells,
            Some(&format!(
                "Phase 1: Scanned {} of {} cluster cells, {} unique clusters",
                processed, total_cells, clusters_map.len()
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    if clusters_map.is_empty() {
        return Ok(0);
    }

    let clusters: Vec<ClusterInfo> = clusters_map.into_values().collect();
    let count = clusters.len() as i64;

    let mut cluster_ids: Vec<Vec<u8>> = Vec::with_capacity(clusters.len());
    let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(clusters.len());
    let mut owner_lock_hashes: Vec<Vec<u8>> = Vec::with_capacity(clusters.len());
    let mut names: Vec<Option<String>> = Vec::with_capacity(clusters.len());
    let mut descriptions: Vec<Option<String>> = Vec::with_capacity(clusters.len());
    let mut created_at_blocks: Vec<i64> = Vec::with_capacity(clusters.len());
    let mut created_at_txs: Vec<Vec<u8>> = Vec::with_capacity(clusters.len());

    for c in &clusters {
        cluster_ids.push(c.cluster_id.clone());
        type_script_hashes.push(c.type_script_hash.clone());
        owner_lock_hashes.push(c.owner_lock_hash.clone());
        names.push(c.name.clone());
        descriptions.push(c.description.clone());
        created_at_blocks.push(c.created_at_block);
        created_at_txs.push(c.created_at_tx.clone());
    }

    sqlx::query(
        r#"
        INSERT INTO spore_clusters (
            cluster_id, type_script_hash, owner_lock_hash, name, description,
            created_at_block, created_at_tx
        )
        SELECT * FROM UNNEST(
            $1::bytea[], $2::bytea[], $3::bytea[], $4::text[], $5::text[],
            $6::bigint[], $7::bytea[]
        )
        "#,
    )
    .bind(&cluster_ids)
    .bind(&type_script_hashes)
    .bind(&owner_lock_hashes)
    .bind(&names)
    .bind(&descriptions)
    .bind(&created_at_blocks)
    .bind(&created_at_txs)
    .execute(pool)
    .await?;

    Ok(count)
}

async fn rebuild_spore_cells(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &SporeRebuildConfig,
    spore_code_hashes: &[Vec<u8>],
    total_cells: i64,
) -> Result<i64> {
    let batch_size = config.batch_size as i64;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;
    let mut processed: i64 = 0;
    let mut inserted: i64 = 0;
    let mut rate_calc = RateCalculator::default();

    loop {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled during Phase 2");
            return Ok(inserted);
        }

        let rows: Vec<SporeCellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, output_index, type_script_hash, type_args,
                   lock_script_hash, data, data_size, status, consumed_at_block, consumed_by_tx
            FROM cells
            WHERE type_code_hash = ANY($1)
              AND type_hash_type IS NOT NULL
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(spore_code_hashes)
        .bind(last_block)
        .bind(last_id)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let batch_count = rows.len() as i64;
        if let Some(last_row) = rows.last() {
            last_block = last_row.0;
            last_id = last_row.1;
        }

        let mut spore_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut tx_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut output_indices: Vec<i16> = Vec::with_capacity(rows.len());
        let mut content_types: Vec<String> = Vec::with_capacity(rows.len());
        let mut content_sizes: Vec<i32> = Vec::with_capacity(rows.len());
        let mut owner_lock_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut is_live_arr: Vec<bool> = Vec::with_capacity(rows.len());
        let mut created_at_blocks: Vec<i64> = Vec::with_capacity(rows.len());
        let mut created_at_txs: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut consumed_at_blocks: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut consumed_by_txs: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());

        for (
            created_at_block, _id, tx_hash, output_index, type_script_hash, type_args,
            lock_script_hash, data, data_size, status, consumed_at_block, consumed_by_tx,
        ) in &rows
        {
            let content_type = data
                .as_deref()
                .and_then(parse_spore_content_type)
                .unwrap_or_else(|| "unknown".to_string());

            spore_ids.push(type_args.clone());
            type_script_hashes.push(type_script_hash.clone());
            tx_hashes.push(tx_hash.clone());
            output_indices.push(*output_index);
            content_types.push(content_type);
            content_sizes.push(*data_size);
            owner_lock_hashes.push(lock_script_hash.clone());
            is_live_arr.push(*status == 0);
            created_at_blocks.push(*created_at_block);
            created_at_txs.push(tx_hash.clone());
            consumed_at_blocks.push(*consumed_at_block);
            consumed_by_txs.push(consumed_by_tx.clone());
        }

        if !spore_ids.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO spore_cells (
                    spore_id, type_script_hash, tx_hash, output_index,
                    content_type, content_size, owner_lock_hash, is_live,
                    created_at_block, created_at_tx, consumed_at_block, consumed_by_tx
                )
                SELECT * FROM UNNEST(
                    $1::bytea[], $2::bytea[], $3::bytea[], $4::smallint[],
                    $5::text[], $6::int[], $7::bytea[], $8::bool[],
                    $9::bigint[], $10::bytea[], $11::bigint[], $12::bytea[]
                )
                ON CONFLICT (spore_id) DO UPDATE SET
                    is_live = EXCLUDED.is_live,
                    consumed_at_block = EXCLUDED.consumed_at_block,
                    consumed_by_tx = EXCLUDED.consumed_by_tx,
                    owner_lock_hash = CASE
                        WHEN EXCLUDED.is_live THEN EXCLUDED.owner_lock_hash
                        ELSE spore_cells.owner_lock_hash
                    END,
                    updated_at = NOW()
                "#,
            )
            .bind(&spore_ids)
            .bind(&type_script_hashes)
            .bind(&tx_hashes)
            .bind(&output_indices)
            .bind(&content_types)
            .bind(&content_sizes)
            .bind(&owner_lock_hashes)
            .bind(&is_live_arr)
            .bind(&created_at_blocks)
            .bind(&created_at_txs)
            .bind(&consumed_at_blocks)
            .bind(&consumed_by_txs)
            .execute(pool)
            .await?;

            inserted += spore_ids.len() as i64;
        }

        processed += batch_count;
        rate_calc.add_sample(processed);
        db.update_progress(
            task_id,
            processed,
            total_cells,
            Some(&format!(
                "Phase 2: Processed {} of {} spore cells, inserted {}",
                processed, total_cells, inserted
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    Ok(inserted)
}

async fn resolve_cluster_ids_via_rpc(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &SporeRebuildConfig,
) -> Result<i64> {
    let spore_rows: Vec<(Vec<u8>, Vec<u8>, i16)> = sqlx::query_as(
        "SELECT spore_id, tx_hash, output_index FROM spore_cells WHERE cluster_id IS NULL",
    )
    .fetch_all(pool)
    .await?;

    if spore_rows.is_empty() {
        info!("Phase 3: No spore cells need cluster_id resolution");
        return Ok(0);
    }

    let total = spore_rows.len() as i64;
    info!("Phase 3: {} spore cells need cluster_id resolution", total);

    let mut tx_to_spores: HashMap<Vec<u8>, Vec<(Vec<u8>, i16)>> = HashMap::new();
    for (spore_id, tx_hash, output_index) in spore_rows {
        tx_to_spores
            .entry(tx_hash)
            .or_default()
            .push((spore_id, output_index));
    }

    let unique_txs: Vec<Vec<u8>> = tx_to_spores.keys().cloned().collect();
    let total_txs = unique_txs.len();
    info!(
        "Phase 3: {} unique transactions to fetch via RPC",
        total_txs
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .pool_max_idle_per_host(RPC_CONCURRENT)
        .build()
        .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

    let mut resolved: i64 = 0;
    let mut processed_txs: i64 = 0;
    let mut rate_calc = RateCalculator::default();

    for chunk in unique_txs.chunks(RPC_BATCH_SIZE) {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled during Phase 3");
            return Ok(resolved);
        }

        let tx_hashes_hex: Vec<String> = chunk
            .iter()
            .map(|h| format!("0x{}", hex::encode(h)))
            .collect();

        let rpc_results =
            fetch_transactions_batch(&client, &config.ckb_rpc_url, &tx_hashes_hex).await;

        let rpc_results = match rpc_results {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    "Phase 3: RPC batch failed for {} txs: {}. Skipping batch.",
                    chunk.len(),
                    e
                );
                processed_txs += chunk.len() as i64;
                continue;
            }
        };

        let mut update_spore_ids: Vec<Vec<u8>> = Vec::new();
        let mut update_cluster_ids: Vec<Vec<u8>> = Vec::new();

        for (tx_hash_bytes, tx_hash_hex) in chunk.iter().zip(tx_hashes_hex.iter()) {
            let Some(tx) = rpc_results.get(tx_hash_hex) else {
                continue;
            };

            let Some(spores) = tx_to_spores.get(tx_hash_bytes) else {
                continue;
            };

            for (spore_id, output_index) in spores {
                let idx = *output_index as usize;
                if idx >= tx.outputs_data.len() {
                    continue;
                }

                let output_data_hex = &tx.outputs_data[idx];
                let output_data = parse_hex_to_bytes(output_data_hex);

                if let Some(cluster_id) = parse_spore_cluster_id(&output_data) {
                    update_spore_ids.push(spore_id.clone());
                    update_cluster_ids.push(cluster_id);
                }
            }
        }

        if !update_spore_ids.is_empty() {
            let batch_resolved = update_spore_ids.len() as i64;
            sqlx::query(
                r#"
                UPDATE spore_cells sc
                SET cluster_id = u.cluster_id, updated_at = NOW()
                FROM UNNEST($1::bytea[], $2::bytea[]) AS u(spore_id, cluster_id)
                WHERE sc.spore_id = u.spore_id
                "#,
            )
            .bind(&update_spore_ids)
            .bind(&update_cluster_ids)
            .execute(pool)
            .await?;

            resolved += batch_resolved;
        }

        processed_txs += chunk.len() as i64;
        rate_calc.add_sample(processed_txs);
        db.update_progress(
            task_id,
            processed_txs,
            total_txs as i64,
            Some(&format!(
                "Phase 3: Fetched {} of {} txs, resolved {} cluster_ids",
                processed_txs, total_txs, resolved
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    Ok(resolved)
}

async fn fetch_transactions_batch(
    client: &Client,
    rpc_url: &str,
    tx_hashes: &[String],
) -> Result<HashMap<String, RpcTransaction>> {
    let chunks: Vec<Vec<String>> = tx_hashes
        .chunks(RPC_BATCH_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect();

    let mut results: HashMap<String, RpcTransaction> = HashMap::new();

    let mut rpc_stream = stream::iter(chunks.into_iter().map(|chunk| {
        let client = client.clone();
        let rpc_url = rpc_url.to_string();
        async move { fetch_rpc_batch_with_retry(&client, &rpc_url, &chunk).await }
    }))
    .buffer_unordered(RPC_CONCURRENT);

    while let Some(chunk_result) = rpc_stream.next().await {
        let chunk_results = chunk_result?;
        results.extend(chunk_results);
    }

    Ok(results)
}

async fn fetch_rpc_batch_with_retry(
    client: &Client,
    rpc_url: &str,
    tx_hashes: &[String],
) -> Result<HashMap<String, RpcTransaction>> {
    for attempt in 1..=RETRY_ATTEMPTS {
        match fetch_rpc_batch(client, rpc_url, tx_hashes).await {
            Ok(results) => return Ok(results),
            Err(err) => {
                warn!(
                    "RPC batch request failed (attempt {}/{}): {}",
                    attempt, RETRY_ATTEMPTS, err
                );
                if attempt < RETRY_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(RETRY_BACKOFF_MS * attempt as u64))
                        .await;
                }
            }
        }
    }

    Err(anyhow!(
        "Failed to fetch transactions batch after {} attempts",
        RETRY_ATTEMPTS
    ))
}

async fn fetch_rpc_batch(
    client: &Client,
    rpc_url: &str,
    tx_hashes: &[String],
) -> Result<HashMap<String, RpcTransaction>> {
    let requests: Vec<RpcRequest> = tx_hashes
        .iter()
        .enumerate()
        .map(|(i, hash)| RpcRequest {
            jsonrpc: "2.0",
            id: i as u32,
            method: "get_transaction",
            params: vec![hash.clone()],
        })
        .collect();

    let response = client.post(rpc_url).json(&requests).send().await?;
    let responses: Vec<Value> = response.json().await?;

    let mut results: HashMap<String, RpcTransaction> = HashMap::new();

    for (i, resp_value) in responses.into_iter().enumerate() {
        if i >= tx_hashes.len() {
            break;
        }
        let tx_hash = &tx_hashes[i];

        if let Some(error) = resp_value.get("error") {
            if !error.is_null() {
                let message = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                debug!("RPC error for tx {}: {}", tx_hash, message);
                continue;
            }
        }

        let Some(result) = resp_value.get("result") else {
            continue;
        };
        if result.is_null() {
            continue;
        }

        let Some(tx_obj) = result.get("transaction") else {
            continue;
        };
        if tx_obj.is_null() {
            continue;
        }

        match serde_json::from_value::<RpcTransaction>(tx_obj.clone()) {
            Ok(tx) => {
                results.insert(tx_hash.clone(), tx);
            }
            Err(e) => {
                debug!("Failed to parse transaction {}: {}", tx_hash, e);
            }
        }
    }

    Ok(results)
}

fn parse_cluster_data(data: &[u8]) -> Option<(Option<String>, Option<String>)> {
    if data.len() < 12 {
        return None;
    }

    let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if data.len() < total_size.min(data.len()) || total_size < 12 {
        return None;
    }

    let offset_name = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    let offset_description = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;

    let end_of_description = if data.len() >= 16 {
        data[12..16]
            .try_into()
            .ok()
            .map(|bytes: [u8; 4]| u32::from_le_bytes(bytes) as usize)
            .unwrap_or(total_size)
    } else {
        total_size
    };

    let name = read_molecule_bytes_field(data, offset_name, offset_description)
        .map(|b| String::from_utf8_lossy(&b).to_string());

    let description = read_molecule_bytes_field(data, offset_description, end_of_description)
        .map(|b| String::from_utf8_lossy(&b).to_string());

    Some((name, description))
}

fn parse_spore_content_type(data: &[u8]) -> Option<String> {
    if data.len() < 16 {
        return None;
    }

    let _total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    let offset_content_type = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    let offset_content = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;

    let bytes = read_molecule_bytes_field(data, offset_content_type, offset_content)?;
    Some(String::from_utf8_lossy(&bytes).to_string())
}

fn parse_spore_cluster_id(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 16 {
        return None;
    }

    let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if data.len() < total_size || total_size < 16 {
        return None;
    }

    let offset_cluster_id = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;

    if offset_cluster_id >= total_size {
        return None;
    }

    if offset_cluster_id + 4 > data.len() {
        return None;
    }

    let opt_header = u32::from_le_bytes(data[offset_cluster_id..offset_cluster_id + 4].try_into().ok()?) as usize;
    if opt_header == 0 {
        return None;
    }

    read_molecule_bytes_field(data, offset_cluster_id, total_size)
}

/// Molecule Bytes field layout: [4B content_length LE][content bytes...]
fn read_molecule_bytes_field(data: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
    if start >= end || start + 4 > data.len() {
        return None;
    }

    let content_len = u32::from_le_bytes(data[start..start + 4].try_into().ok()?) as usize;
    let content_start = start + 4;

    if content_start + content_len > data.len() {
        return None;
    }

    Some(data[content_start..content_start + content_len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SporeRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
        assert!(config.ckb_rpc_url.is_empty());
    }

    #[test]
    fn test_custom_config() {
        let config = SporeRebuildConfig {
            batch_size: 5_000,
            ckb_rpc_url: "http://localhost:8114".to_string(),
        };
        assert_eq!(config.batch_size, 5_000);
        assert_eq!(config.ckb_rpc_url, "http://localhost:8114");
    }

    #[test]
    fn test_result_serialization() {
        let result = SporeRebuildResult {
            spores_processed: 1000,
            spores_marked_consumed: 500,
            clusters_updated: 10,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["sporesProcessed"], 1000);
        assert_eq!(json["sporesMarkedConsumed"], 500);
        assert_eq!(json["clustersUpdated"], 10);
    }

    fn encode_molecule_bytes(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u32;
        let mut result = len.to_le_bytes().to_vec();
        result.extend_from_slice(data);
        result
    }

    fn build_cluster_data(name: &str, description: &str) -> Vec<u8> {
        let name_bytes = encode_molecule_bytes(name.as_bytes());
        let desc_bytes = encode_molecule_bytes(description.as_bytes());

        let offset_name = 16u32;
        let offset_desc = offset_name + name_bytes.len() as u32;
        let offset_end = offset_desc + desc_bytes.len() as u32;
        let total_size = offset_end;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_name.to_le_bytes());
        data.extend_from_slice(&offset_desc.to_le_bytes());
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&name_bytes);
        data.extend_from_slice(&desc_bytes);
        data
    }

    fn build_spore_data(content_type: &str, content: &[u8], cluster_id: Option<&[u8]>) -> Vec<u8> {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content);
        let cluster_id_bytes = cluster_id.map(encode_molecule_bytes);

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster_id = offset_content + content_bytes.len() as u32;
        let total_size =
            offset_cluster_id + cluster_id_bytes.as_ref().map(|b| b.len()).unwrap_or(0) as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_content_type.to_le_bytes());
        data.extend_from_slice(&offset_content.to_le_bytes());
        data.extend_from_slice(&offset_cluster_id.to_le_bytes());
        data.extend_from_slice(&content_type_bytes);
        data.extend_from_slice(&content_bytes);
        if let Some(cid) = cluster_id_bytes {
            data.extend_from_slice(&cid);
        }
        data
    }

    #[test]
    fn test_parse_cluster_data_basic() {
        let data = build_cluster_data("My Collection", "A great collection");
        let result = parse_cluster_data(&data);
        assert!(result.is_some());

        let (name, description) = result.unwrap();
        assert_eq!(name.as_deref(), Some("My Collection"));
        assert_eq!(description.as_deref(), Some("A great collection"));
    }

    #[test]
    fn test_parse_cluster_data_too_short() {
        let data = [0u8; 8];
        assert!(parse_cluster_data(&data).is_none());
    }

    #[test]
    fn test_parse_spore_content_type_basic() {
        let data = build_spore_data("image/png", b"fake png data", None);
        let result = parse_spore_content_type(&data);
        assert_eq!(result.as_deref(), Some("image/png"));
    }

    #[test]
    fn test_parse_spore_content_type_text() {
        let data = build_spore_data("text/plain", b"hello", None);
        let result = parse_spore_content_type(&data);
        assert_eq!(result.as_deref(), Some("text/plain"));
    }

    #[test]
    fn test_parse_spore_content_type_too_short() {
        let data = [0u8; 8];
        assert!(parse_spore_content_type(&data).is_none());
    }

    #[test]
    fn test_parse_spore_cluster_id_present() {
        let cluster_id = [0xab; 32];
        let data = build_spore_data("image/png", b"content", Some(&cluster_id));
        let result = parse_spore_cluster_id(&data);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), cluster_id.to_vec());
    }

    #[test]
    fn test_parse_spore_cluster_id_absent() {
        let data = build_spore_data("image/png", b"content", None);
        let result = parse_spore_cluster_id(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_spore_cluster_id_too_short() {
        let data = [0u8; 8];
        assert!(parse_spore_cluster_id(&data).is_none());
    }

    #[test]
    fn test_read_molecule_bytes_field_valid() {
        let content = b"hello";
        let mut data = (content.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(content);
        let result = read_molecule_bytes_field(&data, 0, data.len());
        assert_eq!(result.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn test_read_molecule_bytes_field_invalid_range() {
        let data = [0u8; 16];
        assert!(read_molecule_bytes_field(&data, 20, 30).is_none());
    }

    #[test]
    fn test_read_molecule_bytes_field_start_ge_end() {
        let data = [0u8; 16];
        assert!(read_molecule_bytes_field(&data, 10, 5).is_none());
    }

    #[test]
    fn test_rpc_request_serialization() {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "get_transaction",
            params: vec!["0xabc123".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"method\":\"get_transaction\""));
        assert!(json.contains("\"params\":[\"0xabc123\"]"));
    }

    #[test]
    fn test_rpc_transaction_deserialization() {
        let json = r#"{
            "outputs_data": ["0xabcdef", "0x123456"]
        }"#;

        let tx: RpcTransaction = serde_json::from_str(json).unwrap();
        assert_eq!(tx.outputs_data.len(), 2);
        assert_eq!(tx.outputs_data[0], "0xabcdef");
        assert_eq!(tx.outputs_data[1], "0x123456");
    }

    #[test]
    fn test_cluster_code_hashes_valid() {
        let h1 = parse_hex_to_bytes(CLUSTER_CODE_HASH_MAINNET_V2);
        let h2 = parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V2);
        let h3 = parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V1);
        assert_eq!(h1.len(), 32);
        assert_eq!(h2.len(), 32);
        assert_eq!(h3.len(), 32);
    }

    #[test]
    fn test_spore_code_hashes_valid() {
        let h1 = parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2);
        let h2 = parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_DID);
        let h3 = parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V2);
        let h4 = parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V1);
        assert_eq!(h1.len(), 32);
        assert_eq!(h2.len(), 32);
        assert_eq!(h3.len(), 32);
        assert_eq!(h4.len(), 32);
    }
}
