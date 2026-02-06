use anyhow::Result;
use ckbadger_common::{parse_hex_to_bytes, MnftRebuildConfig, MnftRebuildResult, RateCalculator};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

const MNFT_ISSUER_CODE_HASH: &str =
    "0x24b04faf80ded836efc05247778eec4ec02548dab6e2012c0107374aa3f68b81";
const MNFT_CLASS_CODE_HASH: &str =
    "0xd51e6eaf48124c601f41abe173f1da550b4cbca9c6a166781906a287abbb3d9a";
const MNFT_TOKEN_CODE_HASH: &str =
    "0x2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9";

type CellRow = (
    i64,
    i64,
    Vec<u8>,
    i16,
    Vec<u8>,
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
    config: &MnftRebuildConfig,
) -> Result<()> {
    info!("Starting mnft_rebuild task");

    let issuer_hash = parse_hex_to_bytes(MNFT_ISSUER_CODE_HASH);
    let class_hash = parse_hex_to_bytes(MNFT_CLASS_CODE_HASH);
    let token_hash = parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH);
    let all_code_hashes = vec![issuer_hash.clone(), class_hash.clone(), token_hash.clone()];

    let total_cells: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cells
        WHERE type_code_hash = ANY($1)
          AND type_script_hash IS NOT NULL
          AND type_args IS NOT NULL
        "#,
    )
    .bind(&all_code_hashes)
    .fetch_one(pool)
    .await?;

    db.update_progress(
        task_id,
        0,
        total_cells,
        Some("Truncating M-NFT tables"),
        None,
    )
    .await?;

    sqlx::query("TRUNCATE TABLE mnft_tokens, mnft_classes, mnft_issuers")
        .execute(pool)
        .await?;

    let mut processed: i64 = 0;
    let mut rate_calc = RateCalculator::default();

    let issuers_created = rebuild_issuers(
        db,
        pool,
        task_id,
        config,
        &[issuer_hash],
        total_cells,
        &mut processed,
        &mut rate_calc,
    )
    .await?;
    info!("Phase 1 complete: {} issuers created", issuers_created);

    let classes_created = rebuild_classes(
        db,
        pool,
        task_id,
        config,
        &[class_hash],
        total_cells,
        &mut processed,
        &mut rate_calc,
    )
    .await?;
    info!("Phase 2 complete: {} classes created", classes_created);

    let tokens_created = rebuild_tokens(
        db,
        pool,
        task_id,
        config,
        &[token_hash],
        total_cells,
        &mut processed,
        &mut rate_calc,
    )
    .await?;
    info!("Phase 3 complete: {} tokens created", tokens_created);

    db.update_progress(
        task_id,
        processed,
        total_cells,
        Some("Phase 4: Updating aggregate counters"),
        rate_calc.rate(),
    )
    .await?;

    update_aggregate_counters(pool).await?;
    info!("Phase 4 complete: aggregate counters updated");

    let result = MnftRebuildResult {
        issuers_created,
        classes_created,
        tokens_created,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "mnft_rebuild completed: {} issuers, {} classes, {} tokens",
        issuers_created, classes_created, tokens_created
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn rebuild_issuers(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &MnftRebuildConfig,
    code_hashes: &[Vec<u8>],
    total_cells: i64,
    processed: &mut i64,
    rate_calc: &mut RateCalculator,
) -> Result<i64> {
    db.update_progress(
        task_id,
        *processed,
        total_cells,
        Some("Phase 1: Scanning issuer cells"),
        None,
    )
    .await?;

    let code_hashes_vec = code_hashes.to_vec();
    let mut issuers_created: i64 = 0;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;

    loop {
        if db.check_cancelled(task_id).await? {
            return Err(anyhow::anyhow!("Task cancelled"));
        }

        let rows: Vec<CellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, output_index, type_script_hash, type_args,
                   lock_script_hash, data, status, consumed_at_block, consumed_by_tx
            FROM cells
            WHERE type_code_hash = ANY($1)
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(&code_hashes_vec)
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

        let mut issuer_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut names: Vec<Option<String>> = Vec::with_capacity(rows.len());
        let mut infos: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());
        let mut owner_lock_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut classes_counts: Vec<i32> = Vec::with_capacity(rows.len());
        let mut is_lives: Vec<bool> = Vec::with_capacity(rows.len());
        let mut created_at_blocks: Vec<i64> = Vec::with_capacity(rows.len());
        let mut created_at_txs: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut consumed_at_blocks: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut consumed_by_txs: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());

        for row in &rows {
            let type_script_hash = &row.4;
            if type_script_hash.len() < 20 {
                continue;
            }

            let issuer_id = type_script_hash[..20].to_vec();
            let parsed = row.7.as_deref().map(parse_issuer_data).unwrap_or_default();

            issuer_ids.push(issuer_id);
            type_script_hashes.push(type_script_hash.clone());
            names.push(parsed.name);
            infos.push(parsed.info);
            owner_lock_hashes.push(row.6.clone());
            classes_counts.push(0);
            is_lives.push(row.8 == 0);
            created_at_blocks.push(row.0);
            created_at_txs.push(row.2.clone());
            consumed_at_blocks.push(row.9);
            consumed_by_txs.push(row.10.clone());
        }

        if !issuer_ids.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO mnft_issuers (
                    issuer_id, type_script_hash, name, info, owner_lock_hash,
                    classes_count, is_live, created_at_block, created_at_tx,
                    consumed_at_block, consumed_by_tx
                )
                SELECT * FROM UNNEST(
                    $1::bytea[], $2::bytea[], $3::text[], $4::bytea[], $5::bytea[],
                    $6::int[], $7::bool[], $8::bigint[], $9::bytea[],
                    $10::bigint[], $11::bytea[]
                )
                ON CONFLICT (issuer_id) DO UPDATE SET
                    owner_lock_hash = EXCLUDED.owner_lock_hash,
                    is_live = EXCLUDED.is_live,
                    consumed_at_block = EXCLUDED.consumed_at_block,
                    consumed_by_tx = EXCLUDED.consumed_by_tx,
                    updated_at = NOW()
                "#,
            )
            .bind(&issuer_ids)
            .bind(&type_script_hashes)
            .bind(&names)
            .bind(&infos)
            .bind(&owner_lock_hashes)
            .bind(&classes_counts)
            .bind(&is_lives)
            .bind(&created_at_blocks)
            .bind(&created_at_txs)
            .bind(&consumed_at_blocks)
            .bind(&consumed_by_txs)
            .execute(pool)
            .await?;

            issuers_created += issuer_ids.len() as i64;
        }

        *processed += batch_count;
        last_block = batch_last_block;
        last_id = batch_last_id;

        rate_calc.add_sample(*processed);
        db.update_progress(
            task_id,
            *processed,
            total_cells,
            Some(&format!(
                "Phase 1: {} issuer cells scanned, {} issuers inserted",
                *processed, issuers_created
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    Ok(issuers_created)
}

#[allow(clippy::too_many_arguments)]
async fn rebuild_classes(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &MnftRebuildConfig,
    code_hashes: &[Vec<u8>],
    total_cells: i64,
    processed: &mut i64,
    rate_calc: &mut RateCalculator,
) -> Result<i64> {
    db.update_progress(
        task_id,
        *processed,
        total_cells,
        Some("Phase 2: Scanning class cells"),
        rate_calc.rate(),
    )
    .await?;

    let code_hashes_vec = code_hashes.to_vec();
    let mut classes_created: i64 = 0;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;

    loop {
        if db.check_cancelled(task_id).await? {
            return Err(anyhow::anyhow!("Task cancelled"));
        }

        let rows: Vec<CellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, output_index, type_script_hash, type_args,
                   lock_script_hash, data, status, consumed_at_block, consumed_by_tx
            FROM cells
            WHERE type_code_hash = ANY($1)
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(&code_hashes_vec)
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

        let mut class_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut class_issuer_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut class_names: Vec<Option<String>> = Vec::with_capacity(rows.len());
        let mut descriptions: Vec<Option<String>> = Vec::with_capacity(rows.len());
        let mut renderers: Vec<Option<String>> = Vec::with_capacity(rows.len());
        let mut totals: Vec<i32> = Vec::with_capacity(rows.len());
        let mut issueds: Vec<i32> = Vec::with_capacity(rows.len());
        let mut owner_lock_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut is_lives: Vec<bool> = Vec::with_capacity(rows.len());
        let mut created_at_blocks: Vec<i64> = Vec::with_capacity(rows.len());
        let mut created_at_txs: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut consumed_at_blocks: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut consumed_by_txs: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());

        for row in &rows {
            let type_args = &row.5;
            if type_args.len() < 24 {
                *processed += 1;
                continue;
            }

            let issuer_id = type_args[..20].to_vec();
            let class_id = type_args.clone();

            let parsed = row.7.as_deref().and_then(parse_class_data);
            let (name, description, renderer, total, issued) = match parsed {
                Some(cd) => (cd.name, cd.description, cd.renderer, cd.total, cd.issued),
                None => (None, None, None, 0, 0),
            };

            class_ids.push(class_id);
            type_script_hashes.push(row.4.clone());
            class_issuer_ids.push(issuer_id);
            class_names.push(name);
            descriptions.push(description);
            renderers.push(renderer);
            totals.push(total as i32);
            issueds.push(issued as i32);
            owner_lock_hashes.push(row.6.clone());
            is_lives.push(row.8 == 0);
            created_at_blocks.push(row.0);
            created_at_txs.push(row.2.clone());
            consumed_at_blocks.push(row.9);
            consumed_by_txs.push(row.10.clone());
        }

        if !class_ids.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO mnft_classes (
                    class_id, type_script_hash, issuer_id, name, description, renderer,
                    total, issued, owner_lock_hash, is_live,
                    created_at_block, created_at_tx, consumed_at_block, consumed_by_tx
                )
                SELECT * FROM UNNEST(
                    $1::bytea[], $2::bytea[], $3::bytea[], $4::text[], $5::text[], $6::text[],
                    $7::int[], $8::int[], $9::bytea[], $10::bool[],
                    $11::bigint[], $12::bytea[], $13::bigint[], $14::bytea[]
                )
                ON CONFLICT (class_id) DO UPDATE SET
                    name = COALESCE(EXCLUDED.name, mnft_classes.name),
                    description = COALESCE(EXCLUDED.description, mnft_classes.description),
                    renderer = COALESCE(EXCLUDED.renderer, mnft_classes.renderer),
                    total = EXCLUDED.total,
                    issued = EXCLUDED.issued,
                    owner_lock_hash = EXCLUDED.owner_lock_hash,
                    is_live = EXCLUDED.is_live,
                    consumed_at_block = EXCLUDED.consumed_at_block,
                    consumed_by_tx = EXCLUDED.consumed_by_tx,
                    updated_at = NOW()
                "#,
            )
            .bind(&class_ids)
            .bind(&type_script_hashes)
            .bind(&class_issuer_ids)
            .bind(&class_names)
            .bind(&descriptions)
            .bind(&renderers)
            .bind(&totals)
            .bind(&issueds)
            .bind(&owner_lock_hashes)
            .bind(&is_lives)
            .bind(&created_at_blocks)
            .bind(&created_at_txs)
            .bind(&consumed_at_blocks)
            .bind(&consumed_by_txs)
            .execute(pool)
            .await?;

            classes_created += class_ids.len() as i64;
        }

        *processed += batch_count;
        last_block = batch_last_block;
        last_id = batch_last_id;

        rate_calc.add_sample(*processed);
        db.update_progress(
            task_id,
            *processed,
            total_cells,
            Some(&format!(
                "Phase 2: {} cells scanned, {} classes inserted",
                *processed, classes_created
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    Ok(classes_created)
}

#[allow(clippy::too_many_arguments)]
async fn rebuild_tokens(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &MnftRebuildConfig,
    code_hashes: &[Vec<u8>],
    total_cells: i64,
    processed: &mut i64,
    rate_calc: &mut RateCalculator,
) -> Result<i64> {
    db.update_progress(
        task_id,
        *processed,
        total_cells,
        Some("Phase 3: Scanning token cells"),
        rate_calc.rate(),
    )
    .await?;

    let code_hashes_vec = code_hashes.to_vec();
    let mut tokens_created: i64 = 0;
    let mut last_block: i64 = -1;
    let mut last_id: i64 = -1;

    loop {
        if db.check_cancelled(task_id).await? {
            return Err(anyhow::anyhow!("Task cancelled"));
        }

        let rows: Vec<CellRow> = sqlx::query_as(
            r#"
            SELECT created_at_block, id, tx_hash, output_index, type_script_hash, type_args,
                   lock_script_hash, data, status, consumed_at_block, consumed_by_tx
            FROM cells
            WHERE type_code_hash = ANY($1)
              AND type_script_hash IS NOT NULL
              AND type_args IS NOT NULL
              AND (created_at_block > $2 OR (created_at_block = $2 AND id > $3))
            ORDER BY created_at_block, id
            LIMIT $4
            "#,
        )
        .bind(&code_hashes_vec)
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

        let mut token_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut type_script_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut tx_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut output_indices: Vec<i16> = Vec::with_capacity(rows.len());
        let mut token_class_ids: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut token_indices: Vec<i32> = Vec::with_capacity(rows.len());
        let mut characteristics: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());
        let mut configures: Vec<i16> = Vec::with_capacity(rows.len());
        let mut states: Vec<i16> = Vec::with_capacity(rows.len());
        let mut owner_lock_hashes: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut is_lives: Vec<bool> = Vec::with_capacity(rows.len());
        let mut created_at_blocks: Vec<i64> = Vec::with_capacity(rows.len());
        let mut created_at_txs: Vec<Vec<u8>> = Vec::with_capacity(rows.len());
        let mut consumed_at_blocks: Vec<Option<i64>> = Vec::with_capacity(rows.len());
        let mut consumed_by_txs: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());

        for row in &rows {
            let type_args = &row.5;
            if type_args.len() < 28 {
                *processed += 1;
                continue;
            }

            let class_id = type_args[..24].to_vec();
            let token_index =
                u32::from_le_bytes(type_args[24..28].try_into().unwrap_or([0; 4])) as i32;
            let token_id = type_args.clone();

            let parsed = row.7.as_deref().and_then(parse_token_data);
            let (characteristic, configure, state) = match parsed {
                Some(td) => (
                    Some(td.characteristic),
                    td.configure as i16,
                    td.state as i16,
                ),
                None => (None, 0, 0),
            };

            token_ids.push(token_id);
            type_script_hashes.push(row.4.clone());
            tx_hashes.push(row.2.clone());
            output_indices.push(row.3);
            token_class_ids.push(class_id);
            token_indices.push(token_index);
            characteristics.push(characteristic);
            configures.push(configure);
            states.push(state);
            owner_lock_hashes.push(row.6.clone());
            is_lives.push(row.8 == 0);
            created_at_blocks.push(row.0);
            created_at_txs.push(row.2.clone());
            consumed_at_blocks.push(row.9);
            consumed_by_txs.push(row.10.clone());
        }

        if !token_ids.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO mnft_tokens (
                    token_id, type_script_hash, tx_hash, output_index, class_id, token_index,
                    characteristic, configure, state, owner_lock_hash, is_live,
                    created_at_block, created_at_tx, consumed_at_block, consumed_by_tx
                )
                SELECT * FROM UNNEST(
                    $1::bytea[], $2::bytea[], $3::bytea[], $4::smallint[], $5::bytea[], $6::int[],
                    $7::bytea[], $8::smallint[], $9::smallint[], $10::bytea[], $11::bool[],
                    $12::bigint[], $13::bytea[], $14::bigint[], $15::bytea[]
                )
                ON CONFLICT (token_id) DO UPDATE SET
                    tx_hash = EXCLUDED.tx_hash,
                    output_index = EXCLUDED.output_index,
                    characteristic = EXCLUDED.characteristic,
                    configure = EXCLUDED.configure,
                    state = EXCLUDED.state,
                    owner_lock_hash = EXCLUDED.owner_lock_hash,
                    is_live = EXCLUDED.is_live,
                    consumed_at_block = EXCLUDED.consumed_at_block,
                    consumed_by_tx = EXCLUDED.consumed_by_tx,
                    updated_at = NOW()
                "#,
            )
            .bind(&token_ids)
            .bind(&type_script_hashes)
            .bind(&tx_hashes)
            .bind(&output_indices)
            .bind(&token_class_ids)
            .bind(&token_indices)
            .bind(&characteristics)
            .bind(&configures)
            .bind(&states)
            .bind(&owner_lock_hashes)
            .bind(&is_lives)
            .bind(&created_at_blocks)
            .bind(&created_at_txs)
            .bind(&consumed_at_blocks)
            .bind(&consumed_by_txs)
            .execute(pool)
            .await?;

            tokens_created += token_ids.len() as i64;
        }

        *processed += batch_count;
        last_block = batch_last_block;
        last_id = batch_last_id;

        rate_calc.add_sample(*processed);
        db.update_progress(
            task_id,
            *processed,
            total_cells,
            Some(&format!(
                "Phase 3: {} cells scanned, {} tokens inserted",
                *processed, tokens_created
            )),
            rate_calc.rate(),
        )
        .await?;
    }

    Ok(tokens_created)
}

async fn update_aggregate_counters(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE mnft_issuers mi
        SET classes_count = sub.cnt,
            updated_at = NOW()
        FROM (
            SELECT issuer_id, COUNT(*)::int AS cnt
            FROM mnft_classes
            WHERE is_live = TRUE
            GROUP BY issuer_id
        ) sub
        WHERE mi.issuer_id = sub.issuer_id
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        UPDATE mnft_classes mc
        SET issued = sub.cnt,
            updated_at = NOW()
        FROM (
            SELECT class_id, COUNT(*)::int AS cnt
            FROM mnft_tokens
            WHERE is_live = TRUE
            GROUP BY class_id
        ) sub
        WHERE mc.class_id = sub.class_id
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        UPDATE mnft_classes mc
        SET holders_count = sub.cnt,
            updated_at = NOW()
        FROM (
            SELECT class_id, COUNT(DISTINCT owner_lock_hash)::int AS cnt
            FROM mnft_tokens
            WHERE is_live = TRUE
            GROUP BY class_id
        ) sub
        WHERE mc.class_id = sub.class_id
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Default)]
struct IssuerParsed {
    name: Option<String>,
    info: Option<Vec<u8>>,
}

fn parse_issuer_data(data: &[u8]) -> IssuerParsed {
    if data.len() < 9 {
        return IssuerParsed::default();
    }

    let _version = data[0];

    if data.len() <= 11 {
        return IssuerParsed::default();
    }

    let info_size = u16::from_le_bytes(data[9..11].try_into().unwrap_or([0; 2])) as usize;

    if info_size == 0 || data.len() < 11 + info_size {
        return IssuerParsed::default();
    }

    let info_bytes = data[11..11 + info_size].to_vec();
    let name = extract_json_field(&info_bytes, "name");

    IssuerParsed {
        name,
        info: Some(info_bytes),
    }
}

struct ClassParsed {
    name: Option<String>,
    description: Option<String>,
    renderer: Option<String>,
    total: u32,
    issued: u32,
}

fn parse_class_data(data: &[u8]) -> Option<ClassParsed> {
    if data.len() < 10 {
        return None;
    }

    let _version = data[0];
    let total = u32::from_le_bytes(data[1..5].try_into().ok()?);
    let issued = u32::from_le_bytes(data[5..9].try_into().ok()?);
    let _configure = data[9];

    let mut offset = 10;
    let name = read_vartext(data, &mut offset);
    let description = read_vartext(data, &mut offset);
    let renderer = read_vartext(data, &mut offset);

    Some(ClassParsed {
        name,
        description,
        renderer,
        total,
        issued,
    })
}

struct TokenParsed {
    characteristic: Vec<u8>,
    configure: u8,
    state: u8,
}

fn parse_token_data(data: &[u8]) -> Option<TokenParsed> {
    if data.len() < 11 {
        return None;
    }

    let _version = data[0];
    let characteristic = data[1..9].to_vec();
    let configure = data[9];
    let state = data[10];

    Some(TokenParsed {
        characteristic,
        configure,
        state,
    })
}

fn read_vartext(data: &[u8], offset: &mut usize) -> Option<String> {
    if *offset + 2 > data.len() {
        return None;
    }

    let size = u16::from_le_bytes(data[*offset..*offset + 2].try_into().ok()?) as usize;
    *offset += 2;

    if size == 0 || *offset + size > data.len() {
        return None;
    }

    let text = bytes_to_pg_string(&data[*offset..*offset + size]);
    *offset += size;
    Some(text)
}

fn extract_json_field(data: &[u8], field: &str) -> Option<String> {
    let text = bytes_to_pg_string(data);
    let key = format!("\"{}\"", field);
    let start = text.find(&key)?;
    let colon_pos = text[start..].find(':')?;
    let value_start = start + colon_pos + 1;

    let trimmed = text[value_start..].trim_start();
    if let Some(stripped) = trimmed.strip_prefix('"') {
        let quote_end = stripped.find('"')?;
        Some(stripped[..quote_end].to_string())
    } else {
        None
    }
}

fn bytes_to_pg_string(data: &[u8]) -> String {
    String::from_utf8_lossy(data).replace('\0', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MnftRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
    }

    #[test]
    fn test_parse_issuer_data_valid() {
        let mut data = vec![0u8];
        data.extend_from_slice(&5u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        let info = r#"{"name":"Test Issuer"}"#;
        data.extend_from_slice(&(info.len() as u16).to_le_bytes());
        data.extend_from_slice(info.as_bytes());

        let parsed = parse_issuer_data(&data);
        assert_eq!(parsed.name, Some("Test Issuer".to_string()));
        assert!(parsed.info.is_some());
    }

    #[test]
    fn test_parse_issuer_data_too_short() {
        let data = vec![0u8; 5];
        let parsed = parse_issuer_data(&data);
        assert!(parsed.name.is_none());
        assert!(parsed.info.is_none());
    }

    #[test]
    fn test_parse_class_data_valid() {
        let mut data = vec![0u8];
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&50u32.to_le_bytes());
        data.push(0b00000011);

        let name = "Test Class";
        data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        data.extend_from_slice(name.as_bytes());

        let desc = "A test collection";
        data.extend_from_slice(&(desc.len() as u16).to_le_bytes());
        data.extend_from_slice(desc.as_bytes());

        data.extend_from_slice(&0u16.to_le_bytes());

        let parsed = parse_class_data(&data).unwrap();
        assert_eq!(parsed.name, Some("Test Class".to_string()));
        assert_eq!(parsed.description, Some("A test collection".to_string()));
        assert!(parsed.renderer.is_none());
        assert_eq!(parsed.total, 100);
        assert_eq!(parsed.issued, 50);
    }

    #[test]
    fn test_parse_class_data_too_short() {
        let data = vec![0u8; 5];
        assert!(parse_class_data(&data).is_none());
    }

    #[test]
    fn test_parse_token_data_valid() {
        let mut data = vec![0u8];
        data.extend_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        data.push(0x01);
        data.push(0x00);

        let parsed = parse_token_data(&data).unwrap();
        assert_eq!(
            parsed.characteristic,
            vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
        );
        assert_eq!(parsed.configure, 0x01);
        assert_eq!(parsed.state, 0x00);
    }

    #[test]
    fn test_parse_token_data_too_short() {
        let data = vec![0u8; 5];
        assert!(parse_token_data(&data).is_none());
    }

    #[test]
    fn test_extract_json_field() {
        let json = r#"{"name":"Alice","age":30}"#;
        assert_eq!(
            extract_json_field(json.as_bytes(), "name"),
            Some("Alice".to_string())
        );
        assert!(extract_json_field(json.as_bytes(), "email").is_none());
    }

    #[test]
    fn test_bytes_to_pg_string() {
        assert_eq!(bytes_to_pg_string(b"hello"), "hello");
        assert_eq!(bytes_to_pg_string(b"null\0byte"), "nullbyte");
    }

    #[test]
    fn test_result_serialization() {
        let result = MnftRebuildResult {
            issuers_created: 10,
            classes_created: 20,
            tokens_created: 300,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["issuersCreated"], 10);
        assert_eq!(json["classesCreated"], 20);
        assert_eq!(json["tokensCreated"], 300);
    }
}
