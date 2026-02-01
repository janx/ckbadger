use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use tracing::{info, warn};

use ckbadger_common::dao::calculate_estimated_apc;

use crate::parser::{ParsedDaoDeposit, ParsedDaoWithdrawRequest};

use super::BatchWriter;

const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;

pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)];
    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)];
    fn block_number(&self) -> i64;
    fn consuming_tx_hash(&self) -> &[u8];
    fn timestamp(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Default)]
pub struct SecondaryIssuanceBreakdown {
    pub secondary_issuance: i64,
    pub miner_secondary: i64,
    pub dao_compensation: i64,
    pub burnt: i64,
}

fn extract_ar_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn extract_total_issuance_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 8 {
        return None;
    }
    let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

impl BatchWriter {
    pub async fn get_block_dao_field(&self, block_number: i64) -> Result<Option<Vec<u8>>> {
        if let Some(store) = &self.live_cell_store {
            if let Some(dao) = store.get_dao_field(block_number) {
                return Ok(Some(dao));
            }
        }

        let row = sqlx::query_as::<_, (Vec<u8>,)>("SELECT dao FROM blocks WHERE number = $1")
            .bind(block_number)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|(dao,)| dao))
    }

    pub async fn insert_dao_deposit(
        &self,
        deposit: &ParsedDaoDeposit,
        block_number: i64,
        timestamp: DateTime<Utc>,
        deposit_ar: i64,
    ) -> Result<()> {
        let inserted: Option<(i64,)> = sqlx::query_as(
            r#"
            INSERT INTO dao_deposits (
                tx_hash, output_index, lock_script_hash, capacity,
                deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, status
            ) VALUES ($1, $2, $3, $4, $5, $1, $6, $7, 0)
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(&deposit.tx_hash)
        .bind(deposit.output_index as i16)
        .bind(&deposit.lock_script_hash)
        .bind(deposit.capacity)
        .bind(block_number)
        .bind(timestamp)
        .bind(deposit_ar)
        .fetch_optional(&self.pool)
        .await?;

        if inserted.is_some() {
            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = total_deposited + $1,
                    active_deposits = active_deposits + 1,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(deposit.capacity)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update_dao_withdraw_request(
        &self,
        request: &ParsedDaoWithdrawRequest,
        block_number: i64,
        timestamp: DateTime<Utc>,
        withdraw_ar: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE dao_deposits SET
                status = 1,
                withdraw_request_block = $3,
                withdraw_request_tx = $4,
                withdraw_request_timestamp = $5,
                withdraw_request_ar = $6
            WHERE tx_hash = $1 AND output_index = $2 AND status = 0
            "#,
        )
        .bind(&request.original_tx_hash)
        .bind(request.original_output_index as i16)
        .bind(block_number)
        .bind(&request.tx_hash)
        .bind(timestamp)
        .bind(withdraw_ar)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn complete_dao_withdrawal(
        &self,
        withdraw_request_tx_hash: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let deposit = sqlx::query_as::<_, (i64, i64, i64, Vec<u8>, i16)>(
            r#"
            SELECT capacity::bigint, deposit_block_number, withdraw_request_block, tx_hash, output_index 
            FROM dao_deposits 
            WHERE withdraw_request_tx = $1 AND status = 1
            "#,
        )
        .bind(withdraw_request_tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((
            capacity,
            deposit_block,
            request_block,
            original_tx_hash,
            original_output_index,
        )) = deposit
        {
            let compensation = self
                .calculate_dao_compensation(capacity, deposit_block, request_block)
                .await?
                .unwrap_or(0);

            sqlx::query(
                r#"
                UPDATE dao_deposits SET
                    status = 2,
                    withdraw_block = $3,
                    withdraw_tx = $4,
                    withdraw_timestamp = $5,
                    compensation = $6
                WHERE tx_hash = $1 AND output_index = $2
                "#,
            )
            .bind(&original_tx_hash)
            .bind(original_output_index)
            .bind(block_number)
            .bind(tx_hash)
            .bind(timestamp)
            .bind(compensation)
            .execute(&self.pool)
            .await?;

            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = GREATEST(0, total_deposited - $1),
                    active_deposits = GREATEST(0, active_deposits - 1),
                    total_compensation_paid = total_compensation_paid + $2,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(capacity)
            .bind(compensation)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Find DAO deposits consumed by inputs. Handles both Phase 1 (matches tx_hash)
    /// and Phase 2 (matches withdraw_request_tx for status=1 records).
    pub async fn find_consumed_dao_deposits(
        &self,
        inputs: &[(&[u8], i32)],
    ) -> Result<Vec<(i64, Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| *h).collect();
        let output_indices: Vec<i16> = inputs.iter().map(|(_, i)| *i as i16).collect();
        let query1 = r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status 
            FROM dao_deposits 
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
        "#;
        let rows1: Vec<(i64, Vec<u8>, i16, String, i64, i16)> = sqlx::query_as(query1)
            .bind(&tx_hashes)
            .bind(&output_indices)
            .fetch_all(&self.pool)
            .await?;

        for row in rows1 {
            seen_ids.insert(row.0);
            results.push(row);
        }

        let query2 = r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status 
            FROM dao_deposits 
            WHERE withdraw_request_tx IN (SELECT * FROM UNNEST($1::bytea[])) AND status = 1
        "#;
        let rows2: Vec<(i64, Vec<u8>, i16, String, i64, i16)> = sqlx::query_as(query2)
            .bind(&tx_hashes)
            .fetch_all(&self.pool)
            .await?;

        for row in rows2 {
            if !seen_ids.contains(&row.0) {
                results.push(row);
            }
        }

        Ok(results)
    }

    pub async fn process_dao_withdrawals(
        &self,
        consumed_dao_deposits: &[(i64, Vec<u8>, i16, String, i64, i16)],
        new_dao_outputs: &[(Vec<u8>, i16, Vec<u8>, i64, u64)],
        block_number: i64,
        consuming_tx_hash: &[u8],
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        for (
            deposit_id,
            _original_tx_hash,
            original_output_index,
            capacity_str,
            deposit_block,
            status,
        ) in consumed_dao_deposits
        {
            let capacity: i64 = capacity_str.parse().unwrap_or(0);

            if *status == 0 {
                let matching_output = new_dao_outputs
                    .iter()
                    .find(|(_, _, _, cap, _)| *cap == capacity);

                if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                    sqlx::query(
                        r#"
                        UPDATE dao_deposits SET
                            status = 1,
                            withdraw_request_block = $3,
                            withdraw_request_tx = $4,
                            withdraw_request_timestamp = $5
                        WHERE id = $1 AND status = 0
                        "#,
                    )
                    .bind(deposit_id)
                    .bind(*original_output_index)
                    .bind(block_number)
                    .bind(new_tx_hash.as_slice())
                    .bind(timestamp)
                    .execute(&self.pool)
                    .await?;
                }
            } else if *status == 1 {
                let withdraw_request_block = sqlx::query_as::<_, (Option<i64>,)>(
                    "SELECT withdraw_request_block FROM dao_deposits WHERE id = $1",
                )
                .bind(deposit_id)
                .fetch_one(&self.pool)
                .await?
                .0
                .unwrap_or(block_number);

                let compensation = self
                    .calculate_dao_compensation(capacity, *deposit_block, withdraw_request_block)
                    .await?;

                sqlx::query(
                    r#"
                    UPDATE dao_deposits SET
                        status = 2,
                        withdraw_block = $2,
                        withdraw_tx = $3,
                        withdraw_timestamp = $4,
                        compensation = $5
                    WHERE id = $1
                    "#,
                )
                .bind(deposit_id)
                .bind(block_number)
                .bind(consuming_tx_hash)
                .bind(timestamp)
                .bind(compensation)
                .execute(&self.pool)
                .await?;

                sqlx::query(
                    r#"
                    UPDATE dao_statistics SET
                        total_deposited = GREATEST(0, total_deposited - $1),
                        active_deposits = GREATEST(0, active_deposits - 1),
                        total_compensation_paid = total_compensation_paid + COALESCE($2, 0),
                        updated_at = NOW()
                    WHERE id = 1
                    "#,
                )
                .bind(capacity)
                .bind(compensation)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn calculate_dao_compensation(
        &self,
        capacity: i64,
        deposit_block: i64,
        withdraw_request_block: i64,
    ) -> Result<Option<i64>> {
        let deposit_dao = self.get_block_dao_field(deposit_block).await?;
        let withdraw_dao = self.get_block_dao_field(withdraw_request_block).await?;

        match (deposit_dao, withdraw_dao) {
            (Some(d), Some(w)) => {
                let ar_deposit = extract_ar_from_dao(&d).unwrap_or(1);
                let ar_withdraw = extract_ar_from_dao(&w).unwrap_or(1);

                if ar_deposit == 0 {
                    return Ok(Some(0));
                }

                let capacity_u128 = capacity as u128;
                let free_capacity = capacity_u128.saturating_sub(DAO_OCCUPIED_CAPACITY as u128);
                let compensation = (free_capacity * ar_withdraw as u128 / ar_deposit as u128)
                    .saturating_sub(free_capacity);

                Ok(Some(compensation as i64))
            }
            _ => Ok(None),
        }
    }

    pub async fn insert_dao_deposits_batch(
        &self,
        deposits: &[(ParsedDaoDeposit, i64, DateTime<Utc>, i64)],
    ) -> Result<()> {
        if deposits.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = deposits
            .iter()
            .map(|(d, _, _, _)| d.tx_hash.as_slice())
            .collect();
        let output_indices: Vec<i16> = deposits
            .iter()
            .map(|(d, _, _, _)| d.output_index as i16)
            .collect();
        let lock_hashes: Vec<&[u8]> = deposits
            .iter()
            .map(|(d, _, _, _)| d.lock_script_hash.as_slice())
            .collect();
        let capacities: Vec<i64> = deposits.iter().map(|(d, _, _, _)| d.capacity).collect();
        let block_numbers: Vec<i64> = deposits.iter().map(|(_, b, _, _)| *b).collect();
        let timestamps: Vec<DateTime<Utc>> = deposits.iter().map(|(_, _, t, _)| *t).collect();
        let ars: Vec<i64> = deposits.iter().map(|(_, _, _, a)| *a).collect();

        let inserted: Vec<(i64, i64)> = sqlx::query_as(
            r#"
            INSERT INTO dao_deposits (
                tx_hash, output_index, lock_script_hash, capacity,
                deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, status
            )
            SELECT t.tx_hash, t.output_index, t.lock_script_hash, t.capacity,
                   t.block_number, t.tx_hash, t.timestamp, t.ar, 0
            FROM UNNEST($1::bytea[], $2::smallint[], $3::bytea[], $4::bigint[], $5::bigint[], $6::timestamptz[], $7::bigint[])
            AS t(tx_hash, output_index, lock_script_hash, capacity, block_number, timestamp, ar)
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            RETURNING id, capacity::bigint
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&lock_hashes)
        .bind(&capacities)
        .bind(&block_numbers)
        .bind(&timestamps)
        .bind(&ars)
        .fetch_all(&self.pool)
        .await?;

        if !inserted.is_empty() {
            let total_capacity: i64 = inserted.iter().map(|(_, c)| c).sum();
            let count = inserted.len() as i64;

            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = total_deposited + $1,
                    active_deposits = active_deposits + $2,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(total_capacity)
            .bind(count)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn find_consumed_dao_deposits_batch(
        &self,
        inputs: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result_map: HashMap<(Vec<u8>, i16), (i64, Vec<u8>, i16, String, i64, i16)> =
            HashMap::new();

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| *h).collect();
        let output_indices: Vec<i16> = inputs.iter().map(|(_, i)| *i).collect();

        let rows1: Vec<(i64, Vec<u8>, i16, String, i64, i16)> = sqlx::query_as(
            r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status
            FROM dao_deposits
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .fetch_all(&self.pool)
        .await?;

        for row in rows1 {
            result_map.insert((row.1.clone(), row.2), row);
        }

        let rows2: Vec<(i64, Vec<u8>, i16, String, i64, i16, Vec<u8>)> = sqlx::query_as(
            r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status, withdraw_request_tx
            FROM dao_deposits
            WHERE withdraw_request_tx IN (SELECT * FROM UNNEST($1::bytea[])) AND status = 1
            "#,
        )
        .bind(&tx_hashes)
        .fetch_all(&self.pool)
        .await?;

        for row in rows2 {
            let key = (row.6.clone(), 0i16);
            result_map
                .entry(key)
                .or_insert((row.0, row.1, row.2, row.3, row.4, row.5));
        }

        Ok(result_map)
    }

    pub async fn process_dao_withdrawals_batch<T>(&self, contexts: &[T]) -> Result<()>
    where
        T: DaoWithdrawalContextTrait,
    {
        if contexts.is_empty() {
            return Ok(());
        }

        let mut phase1_updates: Vec<(i64, i64, Vec<u8>, DateTime<Utc>)> = Vec::new();
        let mut phase2_updates: Vec<(i64, i64, Vec<u8>, DateTime<Utc>, i64, i64)> = Vec::new();
        let mut total_withdrawn_capacity: i64 = 0;
        let mut total_compensation: i64 = 0;
        let mut completed_count: i64 = 0;

        let mut all_deposit_blocks: HashSet<i64> = HashSet::new();
        let mut all_request_blocks: HashSet<i64> = HashSet::new();

        for ctx in contexts {
            for (_, _, _, _, deposit_block, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    all_deposit_blocks.insert(*deposit_block);
                }
            }
        }

        for ctx in contexts {
            for (deposit_id, _, _, _, _, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    let request_block: Option<i64> = sqlx::query_scalar(
                        "SELECT withdraw_request_block FROM dao_deposits WHERE id = $1",
                    )
                    .bind(deposit_id)
                    .fetch_optional(&self.pool)
                    .await?
                    .flatten();

                    if let Some(rb) = request_block {
                        all_request_blocks.insert(rb);
                    }
                }
            }
        }

        let all_blocks: Vec<i64> = all_deposit_blocks
            .union(&all_request_blocks)
            .copied()
            .collect();
        let dao_fields: HashMap<i64, Vec<u8>> = if !all_blocks.is_empty() {
            let mut result = HashMap::new();
            let mut missing = all_blocks.clone();

            if let Some(store) = &self.live_cell_store {
                let cached = store.get_dao_fields_batch(&all_blocks);
                for (block_num, dao) in cached {
                    result.insert(block_num, dao);
                }
                missing.retain(|n| !result.contains_key(n));
            }

            if !missing.is_empty() {
                let rows: Vec<(i64, Vec<u8>)> =
                    sqlx::query_as("SELECT number, dao FROM blocks WHERE number = ANY($1)")
                        .bind(&missing)
                        .fetch_all(&self.pool)
                        .await?;
                for (block_num, dao) in rows {
                    result.insert(block_num, dao);
                }
            }

            result
        } else {
            HashMap::new()
        };

        for ctx in contexts {
            for (deposit_id, _, _, capacity_str, deposit_block, status) in ctx.consumed_deposits() {
                let capacity: i64 = capacity_str.parse().unwrap_or(0);

                if *status == 0 {
                    let matching_output = ctx
                        .new_dao_outputs()
                        .iter()
                        .find(|(_, _, _, cap, _)| *cap == capacity);

                    if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                        phase1_updates.push((
                            *deposit_id,
                            ctx.block_number(),
                            new_tx_hash.clone(),
                            ctx.timestamp(),
                        ));
                    }
                } else if *status == 1 {
                    let request_block: i64 = sqlx::query_scalar(
                        "SELECT withdraw_request_block FROM dao_deposits WHERE id = $1",
                    )
                    .bind(deposit_id)
                    .fetch_optional(&self.pool)
                    .await?
                    .flatten()
                    .unwrap_or(ctx.block_number());

                    let compensation = if let (Some(dep_dao), Some(req_dao)) = (
                        dao_fields.get(deposit_block),
                        dao_fields.get(&request_block),
                    ) {
                        let ar_deposit = extract_ar_from_dao(dep_dao).unwrap_or(1);
                        let ar_withdraw = extract_ar_from_dao(req_dao).unwrap_or(1);
                        if ar_deposit > 0 {
                            let cap_u128 = capacity as u128;
                            let free = cap_u128.saturating_sub(DAO_OCCUPIED_CAPACITY as u128);
                            Some(
                                ((free * ar_withdraw as u128 / ar_deposit as u128)
                                    .saturating_sub(free)) as i64,
                            )
                        } else {
                            Some(0)
                        }
                    } else {
                        None
                    };

                    phase2_updates.push((
                        *deposit_id,
                        ctx.block_number(),
                        ctx.consuming_tx_hash().to_vec(),
                        ctx.timestamp(),
                        compensation.unwrap_or(0),
                        capacity,
                    ));

                    total_withdrawn_capacity += capacity;
                    total_compensation += compensation.unwrap_or(0);
                    completed_count += 1;
                }
            }
        }

        if !phase1_updates.is_empty() {
            let ids: Vec<i64> = phase1_updates.iter().map(|(id, _, _, _)| *id).collect();
            let blocks: Vec<i64> = phase1_updates.iter().map(|(_, b, _, _)| *b).collect();
            let txs: Vec<&[u8]> = phase1_updates
                .iter()
                .map(|(_, _, tx, _)| tx.as_slice())
                .collect();
            let timestamps: Vec<DateTime<Utc>> =
                phase1_updates.iter().map(|(_, _, _, t)| *t).collect();

            sqlx::query(
                r#"
                UPDATE dao_deposits d SET
                    status = 1,
                    withdraw_request_block = v.block_number,
                    withdraw_request_tx = v.tx_hash,
                    withdraw_request_timestamp = v.timestamp
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::bytea[], $4::timestamptz[])
                      AS t(id, block_number, tx_hash, timestamp)) v
                WHERE d.id = v.id AND d.status = 0
                "#,
            )
            .bind(&ids)
            .bind(&blocks)
            .bind(&txs)
            .bind(&timestamps)
            .execute(&self.pool)
            .await?;
        }

        if !phase2_updates.is_empty() {
            let ids: Vec<i64> = phase2_updates
                .iter()
                .map(|(id, _, _, _, _, _)| *id)
                .collect();
            let blocks: Vec<i64> = phase2_updates.iter().map(|(_, b, _, _, _, _)| *b).collect();
            let txs: Vec<&[u8]> = phase2_updates
                .iter()
                .map(|(_, _, tx, _, _, _)| tx.as_slice())
                .collect();
            let timestamps: Vec<DateTime<Utc>> =
                phase2_updates.iter().map(|(_, _, _, t, _, _)| *t).collect();
            let compensations: Vec<i64> =
                phase2_updates.iter().map(|(_, _, _, _, c, _)| *c).collect();

            sqlx::query(
                r#"
                UPDATE dao_deposits d SET
                    status = 2,
                    withdraw_block = v.block_number,
                    withdraw_tx = v.tx_hash,
                    withdraw_timestamp = v.timestamp,
                    compensation = v.compensation
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::bytea[], $4::timestamptz[], $5::bigint[])
                      AS t(id, block_number, tx_hash, timestamp, compensation)) v
                WHERE d.id = v.id
                "#,
            )
            .bind(&ids)
            .bind(&blocks)
            .bind(&txs)
            .bind(&timestamps)
            .bind(&compensations)
            .execute(&self.pool)
            .await?;

            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = GREATEST(0, total_deposited - $1),
                    active_deposits = GREATEST(0, active_deposits - $2),
                    total_compensation_paid = total_compensation_paid + $3,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(total_withdrawn_capacity)
            .bind(completed_count)
            .bind(total_compensation)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_secondary_issuance_state(&self) -> Result<(u128, u128, u128, u128, i64)> {
        let row = sqlx::query_as::<_, (String, String, String, String, i64)>(
            r#"SELECT 
                COALESCE(cumulative_secondary_issuance, '0'),
                COALESCE(cumulative_miner_secondary, '0'),
                COALESCE(cumulative_dao_compensation, '0'),
                COALESCE(cumulative_burnt, '0'),
                COALESCE(last_processed_block, 0)
            FROM dao_statistics WHERE id = 1"#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((sec, miner, dao, burnt, block)) => Ok((
                sec.parse().unwrap_or(0),
                miner.parse().unwrap_or(0),
                dao.parse().unwrap_or(0),
                burnt.parse().unwrap_or(0),
                block,
            )),
            None => Ok((0, 0, 0, 0, 0)),
        }
    }

    pub async fn accumulate_secondary_issuance(
        &self,
        breakdown: &SecondaryIssuanceBreakdown,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO block_secondary_issuance (
                block_number, block_timestamp, secondary_issuance, miner_secondary, dao_compensation, burnt
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (block_number) DO NOTHING
            "#,
        )
        .bind(block_number)
        .bind(block_timestamp)
        .bind(breakdown.secondary_issuance)
        .bind(breakdown.miner_secondary)
        .bind(breakdown.dao_compensation)
        .bind(breakdown.burnt)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE dao_statistics SET
                cumulative_secondary_issuance = (COALESCE(cumulative_secondary_issuance, '0')::numeric + $1)::text,
                cumulative_miner_secondary = (COALESCE(cumulative_miner_secondary, '0')::numeric + $2)::text,
                cumulative_dao_compensation = (COALESCE(cumulative_dao_compensation, '0')::numeric + $3)::text,
                cumulative_burnt = (COALESCE(cumulative_burnt, '0')::numeric + $4)::text,
                mining_reward = (COALESCE(cumulative_miner_secondary, '0')::numeric + $2)::text,
                deposit_compensation = (COALESCE(cumulative_dao_compensation, '0')::numeric + $3)::text,
                burnt = (COALESCE(cumulative_burnt, '0')::numeric + $4)::text,
                last_processed_block = $5,
                updated_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(breakdown.secondary_issuance)
        .bind(breakdown.miner_secondary)
        .bind(breakdown.dao_compensation)
        .bind(breakdown.burnt)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn recalculate_dao_extended_statistics(&self, _current_block: i64) -> Result<()> {
        let latest = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT number, dao FROM blocks WHERE dao IS NOT NULL ORDER BY number DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        let (latest_block, latest_dao) = match latest {
            Some((num, dao)) => (num, dao),
            None => {
                warn!("DAO stats: no blocks with dao field found");
                return Ok(());
            }
        };

        let latest_ar = match extract_ar_from_dao(&latest_dao) {
            Some(ar) => ar,
            None => {
                warn!(
                    "DAO stats: failed to extract AR from block {}, dao len={}",
                    latest_block,
                    latest_dao.len()
                );
                return Ok(());
            }
        };
        let total_issuance = extract_total_issuance_from_dao(&latest_dao).unwrap_or(0);

        let base_stats = sqlx::query_as::<_, (String, i64, i64)>(
            r#"SELECT 
                CAST(COALESCE(SUM(capacity), 0) AS TEXT),
                COUNT(DISTINCT lock_script_hash),
                COUNT(*)
            FROM dao_deposits WHERE status = 0"#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(("0".to_string(), 0, 0));

        let compensation_paid = sqlx::query_as::<_, (String,)>(
            "SELECT CAST(COALESCE(SUM(compensation), 0) AS TEXT) FROM dao_deposits WHERE status = 2 AND compensation IS NOT NULL"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(("0".to_string(),));

        let avg_epochs: (Option<f64>,) = sqlx::query_as(
            r#"SELECT AVG(($1 - deposit_block_number)::float8 / 1800.0) 
            FROM dao_deposits 
            WHERE status = 0 AND deposit_block_number <= $1"#,
        )
        .bind(latest_block)
        .fetch_one(&self.pool)
        .await
        .unwrap_or((None,));

        let deposits_with_ar = sqlx::query_as::<_, (String, Vec<u8>)>(
            r#"SELECT 
                CAST(d.capacity AS TEXT),
                b.dao
            FROM dao_deposits d
            JOIN blocks b ON d.deposit_block_number = b.number
            WHERE d.status = 0 AND b.dao IS NOT NULL AND d.deposit_block_number <= $1"#,
        )
        .bind(latest_block)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut total_unclaimed: u128 = 0;
        let dao_occupied_capacity_u128 = DAO_OCCUPIED_CAPACITY as u128;

        for (capacity_str, deposit_dao) in &deposits_with_ar {
            let capacity: u128 = capacity_str.parse().unwrap_or(0);
            let free_capacity = capacity.saturating_sub(dao_occupied_capacity_u128);

            if let Some(ar_deposit) = extract_ar_from_dao(deposit_dao) {
                if ar_deposit > 0 {
                    let compensation = (free_capacity * latest_ar as u128 / ar_deposit as u128)
                        .saturating_sub(free_capacity);
                    total_unclaimed += compensation;
                }
            }
        }

        let secondary_burnt: u128 = sqlx::query_as::<_, (String,)>(
            "SELECT COALESCE(cumulative_burnt, '0') FROM dao_statistics WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|(s,)| s.parse().unwrap_or(0))
        .unwrap_or(0);

        let estimated_apc = calculate_estimated_apc(total_issuance, secondary_burnt);

        let avg_epochs_val = avg_epochs.0.unwrap_or(0.0);

        info!(
            "DAO stats update: block={}, ar={}, issuance={}, deposits_matched={}, unclaimed={}, apc={:.2}%, avg_epochs={:.1}",
            latest_block,
            latest_ar,
            total_issuance,
            deposits_with_ar.len(),
            total_unclaimed,
            estimated_apc,
            avg_epochs_val
        );

        sqlx::query(
            r#"
            UPDATE dao_statistics SET
                total_deposited = $1::numeric,
                total_depositors = $2,
                active_deposits = $3,
                total_compensation_paid = $4::numeric,
                unclaimed_compensation = $5::numeric,
                average_deposit_epochs = $6,
                estimated_apc = $7,
                updated_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(&base_stats.0)
        .bind(base_stats.1 as i32)
        .bind(base_stats.2 as i32)
        .bind(&compensation_paid.0)
        .bind(total_unclaimed.to_string())
        .bind(avg_epochs_val as i32)
        .bind(format!("{:.2}", estimated_apc))
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
