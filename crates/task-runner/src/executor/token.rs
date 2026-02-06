use anyhow::Result;
use ckbadger_common::{parse_hex_to_bytes, RateCalculator, TokenRebuildConfig, TokenRebuildResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

const SUDT_CODE_HASH: &str = "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
const XUDT_CODE_HASH_DATA1: &str =
    "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95";
const XUDT_CODE_HASH_TYPE: &str =
    "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb";

type UdtCellRow = (
    i64,
    i64,
    Vec<u8>,
    i16,
    Vec<u8>,
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
    config: &TokenRebuildConfig,
) -> Result<()> {
    info!("Starting token_rebuild task");

    let sudt_hash = parse_hex_to_bytes(SUDT_CODE_HASH);
    let xudt_data1_hash = parse_hex_to_bytes(XUDT_CODE_HASH_DATA1);
    let xudt_type_hash = parse_hex_to_bytes(XUDT_CODE_HASH_TYPE);
    let code_hashes = vec![
        sudt_hash.clone(),
        xudt_data1_hash.clone(),
        xudt_type_hash.clone(),
    ];

    let total_cells: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cells
        WHERE type_code_hash = ANY($1)
          AND type_hash_type IS NOT NULL
          AND type_script_hash IS NOT NULL
          AND type_args IS NOT NULL
          AND data_size >= 16
        "#,
    )
    .bind(&code_hashes)
    .fetch_one(pool)
    .await?;

    db.update_progress(
        task_id,
        0,
        total_cells,
        Some("Truncating token tables"),
        None,
    )
    .await?;

    // Truncate all token tables in a single statement with CASCADE
    // to handle foreign key constraints (token_balances -> tokens)
    sqlx::query("TRUNCATE TABLE udt_cells, token_balances, tokens CASCADE")
        .execute(pool)
        .await?;

    db.update_progress(task_id, 0, total_cells, Some("Scanning UDT cells"), None)
        .await?;

    let mut processed: i64 = 0;
    let mut udt_cells_created: i64 = 0;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;
    let mut rate_calc = RateCalculator::default();

    loop {
        let rows: Vec<UdtCellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, output_index, type_script_hash, type_code_hash,
                   type_hash_type, type_args, lock_script_hash, data, status, consumed_at_block, consumed_by_tx
            FROM cells
            WHERE type_code_hash = ANY($1)
              AND type_hash_type IS NOT NULL
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND data_size >= 16
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(&code_hashes)
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
            .map(|row| (row.0, row.1))
            .unwrap_or((last_block, last_id));

        let mut tx_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut output_indices: Vec<i16> = Vec::with_capacity(rows.len());
        let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_code_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_hash_types: Vec<i16> = Vec::with_capacity(rows.len());
        let mut type_args_list: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut lock_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut amounts: Vec<String> = Vec::with_capacity(rows.len());
        let mut standards: Vec<&str> = Vec::with_capacity(rows.len());
        let mut is_live: Vec<bool> = Vec::with_capacity(rows.len());
        let mut created_at_blocks: Vec<i64> = Vec::with_capacity(rows.len());
        let mut consumed_at_blocks: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut consumed_by_txs: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());

        for (
            created_at_block,
            _id,
            tx_hash,
            output_index,
            type_script_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            lock_script_hash,
            data,
            status,
            consumed_at_block,
            consumed_by_tx,
        ) in rows
        {
            let Some(standard) = udt_standard(
                &type_code_hash,
                type_hash_type,
                &sudt_hash,
                &xudt_data1_hash,
                &xudt_type_hash,
            ) else {
                continue;
            };

            let Some(data) = data.as_deref() else {
                continue;
            };
            let Some(amount) = parse_udt_amount(data) else {
                continue;
            };

            tx_hashes.push(tx_hash);
            output_indices.push(output_index);
            type_script_hashes.push(type_script_hash);
            type_code_hashes.push(type_code_hash);
            type_hash_types.push(type_hash_type);
            type_args_list.push(type_args);
            lock_script_hashes.push(lock_script_hash);
            amounts.push(amount.to_string());
            standards.push(standard);
            is_live.push(status == 0);
            created_at_blocks.push(created_at_block);
            consumed_at_blocks.push(consumed_at_block);
            consumed_by_txs.push(consumed_by_tx);
        }

        if !tx_hashes.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO udt_cells (
                    tx_hash, output_index, type_script_hash, type_code_hash, type_hash_type, type_args,
                    lock_script_hash, amount, standard, is_live, created_at_block, consumed_at_block, consumed_by_tx
                )
                SELECT * FROM UNNEST(
                    $1::bytea[], $2::smallint[], $3::bytea[], $4::bytea[], $5::smallint[], $6::bytea[],
                    $7::bytea[], $8::numeric[], $9::text[], $10::bool[], $11::bigint[], $12::bigint[], $13::bytea[]
                )
                "#,
            )
            .bind(&tx_hashes)
            .bind(&output_indices)
            .bind(&type_script_hashes)
            .bind(&type_code_hashes)
            .bind(&type_hash_types)
            .bind(&type_args_list)
            .bind(&lock_script_hashes)
            .bind(&amounts)
            .bind(&standards)
            .bind(&is_live)
            .bind(&created_at_blocks)
            .bind(&consumed_at_blocks)
            .bind(&consumed_by_txs)
            .execute(pool)
            .await?;

            udt_cells_created += tx_hashes.len() as i64;
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
                "Processed {} of {} UDT cells, inserted {}",
                processed, total_cells, udt_cells_created
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    db.update_progress(
        task_id,
        processed,
        total_cells,
        Some("Rebuilding tokens"),
        rate_calc.rate(),
    )
    .await?;

    let tokens_created = rebuild_tokens(pool).await?;

    db.update_progress(
        task_id,
        processed,
        total_cells,
        Some("Rebuilding token balances"),
        rate_calc.rate(),
    )
    .await?;

    let balances_updated = rebuild_token_balances(pool).await?;

    sqlx::query(
        "UPDATE sync_status SET token_deferred = FALSE, token_rebuild_completed_at = NOW() WHERE id = 1",
    )
    .execute(pool)
    .await?;

    let result = TokenRebuildResult {
        tokens_created,
        balances_updated,
        udt_cells_created,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "token_rebuild completed: {} tokens, {} balances, {} udt_cells",
        tokens_created, balances_updated, udt_cells_created
    );

    Ok(())
}

async fn rebuild_tokens(pool: &PgPool) -> Result<i64> {
    sqlx::query(
        r#"
        WITH first_seen AS (
            SELECT DISTINCT ON (type_script_hash)
                type_script_hash,
                type_code_hash,
                type_hash_type,
                type_args,
                standard,
                created_at_block AS first_seen_block,
                tx_hash AS first_seen_tx
            FROM udt_cells
            ORDER BY type_script_hash, created_at_block, tx_hash
        ),
        live_supply AS (
            SELECT type_script_hash, SUM(amount)::numeric AS total_supply
            FROM udt_cells
            WHERE is_live = TRUE
            GROUP BY type_script_hash
        ),
        holders AS (
            SELECT type_script_hash, COUNT(DISTINCT lock_script_hash)::int AS holders_count
            FROM udt_cells
            WHERE is_live = TRUE
            GROUP BY type_script_hash
        )
        INSERT INTO tokens (
            type_script_hash, type_code_hash, type_hash_type, type_args, standard,
            total_supply, holders_count, transfers_count, transfers_24h,
            first_seen_block, first_seen_tx
        )
        SELECT
            f.type_script_hash,
            f.type_code_hash,
            f.type_hash_type,
            f.type_args,
            f.standard,
            COALESCE(s.total_supply, 0),
            COALESCE(h.holders_count, 0),
            0,
            0,
            f.first_seen_block,
            f.first_seen_tx
        FROM first_seen f
        LEFT JOIN live_supply s ON s.type_script_hash = f.type_script_hash
        LEFT JOIN holders h ON h.type_script_hash = f.type_script_hash
        "#,
    )
    .execute(pool)
    .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tokens")
        .fetch_one(pool)
        .await?;

    Ok(count)
}

async fn rebuild_token_balances(pool: &PgPool) -> Result<i64> {
    sqlx::query(
        r#"
        WITH balances AS (
            SELECT
                type_script_hash,
                lock_script_hash,
                SUM(amount)::numeric AS balance,
                MIN(created_at_block) AS first_block,
                MAX(created_at_block) AS last_block
            FROM udt_cells
            WHERE is_live = TRUE
            GROUP BY type_script_hash, lock_script_hash
        ),
        first_tx AS (
            SELECT DISTINCT ON (uc.type_script_hash, uc.lock_script_hash)
                uc.type_script_hash,
                uc.lock_script_hash,
                uc.tx_hash AS first_tx
            FROM udt_cells uc
            JOIN balances b
              ON b.type_script_hash = uc.type_script_hash
             AND b.lock_script_hash = uc.lock_script_hash
            WHERE uc.created_at_block = b.first_block
            ORDER BY uc.type_script_hash, uc.lock_script_hash, uc.tx_hash ASC
        ),
        last_tx AS (
            SELECT DISTINCT ON (uc.type_script_hash, uc.lock_script_hash)
                uc.type_script_hash,
                uc.lock_script_hash,
                uc.tx_hash AS last_tx
            FROM udt_cells uc
            JOIN balances b
              ON b.type_script_hash = uc.type_script_hash
             AND b.lock_script_hash = uc.lock_script_hash
            WHERE uc.created_at_block = b.last_block
            ORDER BY uc.type_script_hash, uc.lock_script_hash, uc.tx_hash DESC
        )
        INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
        SELECT
            t.id,
            b.lock_script_hash,
            b.balance,
            f.first_tx,
            l.last_tx
        FROM balances b
        JOIN tokens t ON t.type_script_hash = b.type_script_hash
        JOIN first_tx f
          ON f.type_script_hash = b.type_script_hash
         AND f.lock_script_hash = b.lock_script_hash
        JOIN last_tx l
          ON l.type_script_hash = b.type_script_hash
         AND l.lock_script_hash = b.lock_script_hash
        "#,
    )
    .execute(pool)
    .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM token_balances")
        .fetch_one(pool)
        .await?;

    Ok(count)
}

fn parse_udt_amount(data: &[u8]) -> Option<u128> {
    if data.len() < 16 {
        return None;
    }
    Some(u128::from_le_bytes(data[0..16].try_into().ok()?))
}

fn udt_standard(
    code_hash: &[u8],
    hash_type: i16,
    sudt_hash: &[u8],
    xudt_data1_hash: &[u8],
    xudt_type_hash: &[u8],
) -> Option<&'static str> {
    if code_hash == sudt_hash && hash_type == 1 {
        return Some("sudt");
    }

    if (code_hash == xudt_data1_hash && hash_type == 2)
        || (code_hash == xudt_type_hash && hash_type == 1)
    {
        return Some("xudt");
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_udt_amount_valid() {
        let amount = 42u128;
        let data = amount.to_le_bytes();
        assert_eq!(parse_udt_amount(&data), Some(amount));
    }

    #[test]
    fn test_parse_udt_amount_too_short() {
        let data = [0u8; 8];
        assert!(parse_udt_amount(&data).is_none());
    }

    #[test]
    fn test_udt_standard_detection() {
        let sudt_hash = parse_hex_to_bytes(SUDT_CODE_HASH);
        let xudt_data1_hash = parse_hex_to_bytes(XUDT_CODE_HASH_DATA1);
        let xudt_type_hash = parse_hex_to_bytes(XUDT_CODE_HASH_TYPE);

        assert_eq!(
            udt_standard(&sudt_hash, 1, &sudt_hash, &xudt_data1_hash, &xudt_type_hash),
            Some("sudt")
        );
        assert_eq!(
            udt_standard(
                &xudt_data1_hash,
                2,
                &sudt_hash,
                &xudt_data1_hash,
                &xudt_type_hash
            ),
            Some("xudt")
        );
        assert_eq!(
            udt_standard(
                &xudt_type_hash,
                1,
                &sudt_hash,
                &xudt_data1_hash,
                &xudt_type_hash
            ),
            Some("xudt")
        );
        assert!(
            udt_standard(&sudt_hash, 2, &sudt_hash, &xudt_data1_hash, &xudt_type_hash).is_none()
        );
    }

    #[test]
    fn test_result_serialization() {
        let result = TokenRebuildResult {
            tokens_created: 3,
            balances_updated: 5,
            udt_cells_created: 8,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["tokensCreated"], 3);
        assert_eq!(json["balancesUpdated"], 5);
        assert_eq!(json["udtCellsCreated"], 8);
    }
}
