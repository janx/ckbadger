use anyhow::Result;
use ckbadger_common::{parse_hex_to_bytes, DotbitRebuildConfig, DotbitRebuildResult, RateCalculator};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

const DOTBIT_ACCOUNT_CELL_TYPE_ID: &str =
    "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918";

type CellRow = (
    i64,
    i64,
    Vec<u8>,
    i16,
    Vec<u8>,
    Vec<u8>,
    Option<Vec<u8>>,
    i16,
    Option<i64>,
    Option<Vec<u8>>,
);

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &DotbitRebuildConfig,
) -> Result<()> {
    info!("Starting dotbit_rebuild task");

    let code_hash = parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);

    let total_cells: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cells
        WHERE type_code_hash = $1
          AND type_script_hash IS NOT NULL
          AND type_args IS NOT NULL
        "#,
    )
    .bind(&code_hash)
    .fetch_one(pool)
    .await?;

    db.update_progress(
        task_id,
        0,
        total_cells,
        Some("Truncating dotbit_accounts table"),
        None,
    )
    .await?;

    sqlx::query("TRUNCATE TABLE dotbit_accounts")
        .execute(pool)
        .await?;

    let mut processed: i64 = 0;
    let mut accounts_created: i64 = 0;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;
    let mut rate_calc = RateCalculator::default();

    db.update_progress(
        task_id,
        processed,
        total_cells,
        Some("Scanning DotBit account cells"),
        None,
    )
    .await?;

    loop {
        if db.check_cancelled(task_id).await? {
            return Err(anyhow::anyhow!("Task cancelled"));
        }

        let rows: Vec<CellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, output_index, type_script_hash, type_args,
                   lock_script_hash, status, consumed_at_block, consumed_by_tx
            FROM cells
            WHERE type_code_hash = $1
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(&code_hash)
        .bind(last_block)
        .bind(last_id)
        .bind(config.batch_size)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let batch_count = rows.len() as i64;
        let (batch_last_block, batch_last_id) = rows
            .last()
            .map(|r| (r.0, r.1))
            .unwrap_or((last_block, last_id));

        let mut account_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut tx_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut output_indices: Vec<i16> = Vec::with_capacity(rows.len());
        let mut account_names: Vec<String> = Vec::with_capacity(rows.len());
        let mut owner_lock_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut next_account_ids: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());
        let mut expired_ats: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut is_lives: Vec<bool> = Vec::with_capacity(rows.len());
        let mut created_at_blocks: Vec<i64> = Vec::with_capacity(rows.len());
        let mut created_at_txs: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut consumed_at_blocks: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut consumed_by_txs: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());

        for row in &rows {
            let type_args = &row.5;
            if type_args.len() < 20 {
                processed += 1;
                continue;
            }

            let account_id = type_args[..20].to_vec();
            let account_name = format!("0x{}", hex::encode(&account_id));

            // Extract next_account_id from cell data (bytes 32-52 if available)
            let next_account_id = if let Some(data) = &row.6 {
                if data.len() >= 52 {
                    let next_id = data[32..52].to_vec();
                    if next_id.iter().all(|&b| b == 0) {
                        None
                    } else {
                        Some(next_id)
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Extract expired_at from cell data (bytes 52-60 if available)
            let expired_at = if let Some(data) = &row.6 {
                if data.len() >= 60 {
                    let bytes: [u8; 8] = data[52..60].try_into().unwrap_or([0; 8]);
                    let timestamp = u64::from_le_bytes(bytes);
                    if timestamp > 0 {
                        Some(timestamp as i64)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            account_ids.push(account_id);
            type_script_hashes.push(row.4.clone());
            tx_hashes.push(row.2.clone());
            output_indices.push(row.3);
            account_names.push(account_name);
            owner_lock_hashes.push(row.6.clone().unwrap_or_default());
            next_account_ids.push(next_account_id);
            expired_ats.push(expired_at);
            is_lives.push(row.7 == 0);
            created_at_blocks.push(row.0);
            created_at_txs.push(row.2.clone());
            consumed_at_blocks.push(row.8);
            consumed_by_txs.push(row.9.clone());
        }

        if !account_ids.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO dotbit_accounts (
                    account_id, type_script_hash, tx_hash, output_index, account_name,
                    owner_lock_hash, next_account_id, expired_at, is_live,
                    created_at_block, created_at_tx, consumed_at_block, consumed_by_tx
                )
                SELECT * FROM UNNEST(
                    $1::bytea[], $2::bytea[], $3::bytea[], $4::smallint[], $5::text[],
                    $6::bytea[], $7::bytea[], $8::bigint[], $9::bool[],
                    $10::bigint[], $11::bytea[], $12::bigint[], $13::bytea[]
                )
                ON CONFLICT (account_id) DO UPDATE SET
                    tx_hash = EXCLUDED.tx_hash,
                    output_index = EXCLUDED.output_index,
                    owner_lock_hash = EXCLUDED.owner_lock_hash,
                    next_account_id = EXCLUDED.next_account_id,
                    expired_at = EXCLUDED.expired_at,
                    is_live = EXCLUDED.is_live,
                    consumed_at_block = EXCLUDED.consumed_at_block,
                    consumed_by_tx = EXCLUDED.consumed_by_tx,
                    updated_at = NOW()
                "#,
            )
            .bind(&account_ids)
            .bind(&type_script_hashes)
            .bind(&tx_hashes)
            .bind(&output_indices)
            .bind(&account_names)
            .bind(&owner_lock_hashes)
            .bind(&next_account_ids)
            .bind(&expired_ats)
            .bind(&is_lives)
            .bind(&created_at_blocks)
            .bind(&created_at_txs)
            .bind(&consumed_at_blocks)
            .bind(&consumed_by_txs)
            .execute(pool)
            .await?;

            accounts_created += account_ids.len() as i64;
        }

        processed += batch_count;
        last_block = batch_last_block;
        last_id = batch_last_id;

        rate_calc.add_sample(processed);
        db.update_progress(
            task_id,
            processed,
            total_cells,
            Some(&format!(
                "Scanned {} cells, {} accounts inserted",
                processed, accounts_created
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    let result = DotbitRebuildResult {
        accounts_created,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "dotbit_rebuild completed: {} accounts created",
        accounts_created
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DotbitRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
    }
}
