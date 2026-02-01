use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::parser::{ParsedUdtCell, ParsedUdtTransfer};

use super::BatchWriter;

impl BatchWriter {
    pub async fn get_udt_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String)>>
    {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _)| *h).collect();
        let indices: Vec<i16> = outpoints.iter().map(|(_, i)| *i).collect();

        let rows = sqlx::query_as::<
            _,
            (
                Vec<u8>,
                i16,
                Vec<u8>,
                Vec<u8>,
                i16,
                Vec<u8>,
                Vec<u8>,
                String,
                String,
            ),
        >(
            r#"
            SELECT tx_hash, output_index, type_script_hash, type_code_hash, 
                   type_hash_type, type_args, lock_script_hash, amount::text, standard
            FROM udt_cells
            JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
              USING (tx_hash, output_index)
            WHERE is_live = TRUE
            "#,
        )
        .bind(&tx_hashes)
        .bind(&indices)
        .fetch_all(&self.pool)
        .await?;

        let mut result = HashMap::with_capacity(rows.len());
        for (
            tx_hash,
            idx,
            type_script_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            lock_script_hash,
            amount_str,
            standard,
        ) in rows
        {
            let amount: u128 = amount_str.parse().unwrap_or(0);
            result.insert(
                (tx_hash, idx),
                (
                    type_script_hash,
                    type_code_hash,
                    type_hash_type,
                    type_args,
                    lock_script_hash,
                    amount,
                    standard,
                ),
            );
        }

        Ok(result)
    }

    pub async fn insert_udt_cells_batch(
        &self,
        cells: &[(&[u8], i16, &ParsedUdtCell, i64)],
    ) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = cells.iter().map(|(h, _, _, _)| *h).collect();
        let output_indices: Vec<i16> = cells.iter().map(|(_, i, _, _)| *i).collect();
        let type_script_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_script_hash.as_slice())
            .collect();
        let type_code_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_code_hash.as_slice())
            .collect();
        let type_hash_types: Vec<i16> = cells.iter().map(|(_, _, c, _)| c.type_hash_type).collect();
        let type_args: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_args.as_slice())
            .collect();
        let lock_script_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.lock_script_hash.as_slice())
            .collect();
        let amounts: Vec<String> = cells
            .iter()
            .map(|(_, _, c, _)| c.amount.to_string())
            .collect();
        let standards: Vec<&str> = cells
            .iter()
            .map(|(_, _, c, _)| c.standard.as_str())
            .collect();
        let created_at_blocks: Vec<i64> = cells.iter().map(|(_, _, _, b)| *b).collect();

        sqlx::query(
            r#"
            INSERT INTO udt_cells (
                tx_hash, output_index, type_script_hash, type_code_hash, type_hash_type, type_args,
                lock_script_hash, amount, standard, created_at_block
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::smallint[], $3::bytea[], $4::bytea[], $5::smallint[], $6::bytea[],
                $7::bytea[], $8::numeric[], $9::text[], $10::bigint[]
            )
            ON CONFLICT (tx_hash, output_index) DO UPDATE SET
                lock_script_hash = EXCLUDED.lock_script_hash,
                amount = EXCLUDED.amount,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&type_script_hashes)
        .bind(&type_code_hashes)
        .bind(&type_hash_types)
        .bind(&type_args)
        .bind(&lock_script_hashes)
        .bind(&amounts)
        .bind(&standards)
        .bind(&created_at_blocks)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_udt_cells_batch(
        &self,
        outpoints: &[(&[u8], i16, i64, &[u8])],
    ) -> Result<()> {
        if outpoints.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _, _, _)| *h).collect();
        let output_indices: Vec<i16> = outpoints.iter().map(|(_, i, _, _)| *i).collect();
        let consumed_at_blocks: Vec<i64> = outpoints.iter().map(|(_, _, b, _)| *b).collect();
        let consumed_by_txs: Vec<&[u8]> = outpoints.iter().map(|(_, _, _, t)| *t).collect();

        sqlx::query(
            r#"
            UPDATE udt_cells SET
                is_live = FALSE,
                consumed_at_block = u.consumed_at_block,
                consumed_by_tx = u.consumed_by_tx
            FROM (
                SELECT * FROM UNNEST($1::bytea[], $2::smallint[], $3::bigint[], $4::bytea[])
                AS t(tx_hash, output_index, consumed_at_block, consumed_by_tx)
            ) AS u
            WHERE udt_cells.tx_hash = u.tx_hash 
              AND udt_cells.output_index = u.output_index
              AND udt_cells.is_live = TRUE
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&consumed_at_blocks)
        .bind(&consumed_by_txs)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn process_udt_transfer(
        &self,
        transfer: &ParsedUdtTransfer,
        tx_hash: &[u8],
        block_number: i64,
        _timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let token_id = self.upsert_token(transfer, block_number, tx_hash).await?;

        if transfer.is_mint {
            sqlx::query(
                "UPDATE tokens SET total_supply = total_supply + $1::numeric WHERE id = $2",
            )
            .bind(transfer.amount.to_string())
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        } else if transfer.is_burn {
            sqlx::query(
                "UPDATE tokens SET total_supply = GREATEST(total_supply - $1::numeric, 0) WHERE id = $2",
            )
            .bind(transfer.amount.to_string())
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        }

        if let Some(ref from_lock) = transfer.from_lock_hash {
            self.update_token_balance(token_id, from_lock, -(transfer.amount as i64), tx_hash)
                .await?;
        }

        if !transfer.to_lock_hash.is_empty() {
            self.update_token_balance(
                token_id,
                &transfer.to_lock_hash,
                transfer.amount as i64,
                tx_hash,
            )
            .await?;
        }

        Ok(())
    }

    async fn upsert_token(
        &self,
        transfer: &ParsedUdtTransfer,
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            INSERT INTO tokens (
                type_script_hash, type_code_hash, type_hash_type, type_args,
                standard, first_seen_block, first_seen_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (type_script_hash) DO UPDATE SET
                transfers_count = tokens.transfers_count + 1,
                updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(&transfer.type_script_hash)
        .bind(&transfer.type_code_hash)
        .bind(transfer.type_hash_type)
        .bind(&transfer.type_args)
        .bind(transfer.standard.as_str())
        .bind(block_number)
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    async fn update_token_balance(
        &self,
        token_id: i64,
        lock_script_hash: &[u8],
        amount_delta: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        if lock_script_hash.is_empty() {
            return Ok(());
        }

        let existing = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, balance::bigint FROM token_balances WHERE token_id = $1 AND lock_script_hash = $2",
        )
        .bind(token_id)
        .bind(lock_script_hash)
        .fetch_optional(&self.pool)
        .await?;

        match existing {
            Some((id, balance)) => {
                let new_balance = (balance + amount_delta).max(0);

                if new_balance == 0 {
                    sqlx::query("DELETE FROM token_balances WHERE id = $1")
                        .bind(id)
                        .execute(&self.pool)
                        .await?;

                    sqlx::query(
                        "UPDATE tokens SET holders_count = holders_count - 1 WHERE id = $1 AND holders_count > 0",
                    )
                    .bind(token_id)
                    .execute(&self.pool)
                    .await?;
                } else {
                    sqlx::query(
                        "UPDATE token_balances SET balance = $1, last_tx = $2, updated_at = NOW() WHERE id = $3",
                    )
                    .bind(new_balance)
                    .bind(tx_hash)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                }
            }
            None => {
                if amount_delta > 0 {
                    sqlx::query(
                        r#"
                        INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
                        VALUES ($1, $2, $3, $4, $4)
                        "#,
                    )
                    .bind(token_id)
                    .bind(lock_script_hash)
                    .bind(amount_delta)
                    .bind(tx_hash)
                    .execute(&self.pool)
                    .await?;

                    sqlx::query(
                        "UPDATE tokens SET holders_count = holders_count + 1 WHERE id = $1",
                    )
                    .bind(token_id)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(())
    }

    pub async fn process_udt_transfers_batch(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64, DateTime<Utc>)],
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        // Step 1: Collect unique tokens (first occurrence info for new tokens)
        let mut unique_tokens: HashMap<Vec<u8>, (&ParsedUdtTransfer, i64, Vec<u8>)> =
            HashMap::new();
        for (transfer, tx_hash, block_number, _) in transfers {
            unique_tokens
                .entry(transfer.type_script_hash.clone())
                .or_insert((*transfer, *block_number, tx_hash.to_vec()));
        }

        // Step 2: Batch upsert tokens - get existing + insert new, return all IDs
        let type_script_hashes: Vec<&[u8]> = unique_tokens.keys().map(|k| k.as_slice()).collect();

        // Get existing token IDs
        let existing_tokens: Vec<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT type_script_hash, id FROM tokens WHERE type_script_hash = ANY($1)",
        )
        .bind(&type_script_hashes)
        .fetch_all(&self.pool)
        .await?;

        let mut token_ids: HashMap<Vec<u8>, i64> = existing_tokens.into_iter().collect();

        // Insert new tokens (ones not in existing)
        let new_tokens: Vec<_> = unique_tokens
            .iter()
            .filter(|(hash, _)| !token_ids.contains_key(*hash))
            .collect();

        if !new_tokens.is_empty() {
            let new_hashes: Vec<&[u8]> = new_tokens.iter().map(|(h, _)| h.as_slice()).collect();
            let new_code_hashes: Vec<&[u8]> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.type_code_hash.as_slice())
                .collect();
            let new_hash_types: Vec<i16> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.type_hash_type)
                .collect();
            let new_args: Vec<&[u8]> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.type_args.as_slice())
                .collect();
            let new_standards: Vec<&str> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.standard.as_str())
                .collect();
            let new_blocks: Vec<i64> = new_tokens.iter().map(|(_, (_, b, _))| *b).collect();
            let new_txs: Vec<&[u8]> = new_tokens
                .iter()
                .map(|(_, (_, _, tx))| tx.as_slice())
                .collect();

            let inserted: Vec<(Vec<u8>, i64)> = sqlx::query_as(
                r#"
                INSERT INTO tokens (type_script_hash, type_code_hash, type_hash_type, type_args, standard, first_seen_block, first_seen_tx)
                SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::smallint[], $4::bytea[], $5::text[], $6::bigint[], $7::bytea[])
                ON CONFLICT (type_script_hash) DO NOTHING
                RETURNING type_script_hash, id
                "#,
            )
            .bind(&new_hashes)
            .bind(&new_code_hashes)
            .bind(&new_hash_types)
            .bind(&new_args)
            .bind(&new_standards)
            .bind(&new_blocks)
            .bind(&new_txs)
            .fetch_all(&self.pool)
            .await?;

            for (hash, id) in inserted {
                token_ids.insert(hash, id);
            }

            // Re-fetch any that were already inserted by concurrent process
            let still_missing: Vec<&[u8]> = new_tokens
                .iter()
                .filter(|(h, _)| !token_ids.contains_key(*h))
                .map(|(h, _)| h.as_slice())
                .collect();

            if !still_missing.is_empty() {
                let fetched: Vec<(Vec<u8>, i64)> = sqlx::query_as(
                    "SELECT type_script_hash, id FROM tokens WHERE type_script_hash = ANY($1)",
                )
                .bind(&still_missing)
                .fetch_all(&self.pool)
                .await?;

                for (hash, id) in fetched {
                    token_ids.insert(hash, id);
                }
            }
        }

        // Step 3: Aggregate stats per token (transfer counts, supply changes)
        let mut transfer_counts: HashMap<i64, i64> = HashMap::new();
        let mut supply_changes: HashMap<i64, i128> = HashMap::new();

        for (transfer, _, _, _) in transfers {
            let token_id = token_ids[&transfer.type_script_hash];
            *transfer_counts.entry(token_id).or_default() += 1;

            if transfer.is_mint {
                *supply_changes.entry(token_id).or_default() += transfer.amount as i128;
            } else if transfer.is_burn {
                *supply_changes.entry(token_id).or_default() -= transfer.amount as i128;
            }
        }

        // Step 4: Batch update token stats
        if !transfer_counts.is_empty() {
            let stat_ids: Vec<i64> = transfer_counts.keys().copied().collect();
            let stat_counts: Vec<i64> = stat_ids.iter().map(|id| transfer_counts[id]).collect();
            let stat_supply: Vec<String> = stat_ids
                .iter()
                .map(|id| supply_changes.get(id).copied().unwrap_or(0).to_string())
                .collect();

            sqlx::query(
                r#"
                UPDATE tokens t SET
                    transfers_count = t.transfers_count + v.cnt,
                    total_supply = GREATEST(0, t.total_supply + v.supply::numeric),
                    updated_at = NOW()
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::text[]) AS t(id, cnt, supply)) v
                WHERE t.id = v.id
                "#,
            )
            .bind(&stat_ids)
            .bind(&stat_counts)
            .bind(&stat_supply)
            .execute(&self.pool)
            .await?;
        }

        // Step 5: Aggregate balance changes per (token_id, lock_script_hash)
        // Value: (delta as i128, last_tx)
        let mut balance_changes: HashMap<(i64, Vec<u8>), (i128, Vec<u8>)> = HashMap::new();

        for (transfer, tx_hash, _, _) in transfers {
            let token_id = token_ids[&transfer.type_script_hash];

            if let Some(ref from_lock) = transfer.from_lock_hash {
                if !from_lock.is_empty() {
                    balance_changes
                        .entry((token_id, from_lock.clone()))
                        .and_modify(|(d, t)| {
                            *d -= transfer.amount as i128;
                            *t = tx_hash.to_vec();
                        })
                        .or_insert((-(transfer.amount as i128), tx_hash.to_vec()));
                }
            }

            if !transfer.to_lock_hash.is_empty() {
                balance_changes
                    .entry((token_id, transfer.to_lock_hash.clone()))
                    .and_modify(|(d, t)| {
                        *d += transfer.amount as i128;
                        *t = tx_hash.to_vec();
                    })
                    .or_insert((transfer.amount as i128, tx_hash.to_vec()));
            }
        }

        // Step 6: Apply balance changes in batch
        if !balance_changes.is_empty() {
            self.batch_apply_balance_changes(&balance_changes).await?;
        }

        Ok(())
    }

    /// Batch apply balance changes: fetch existing, compute new values, batch update/insert/delete
    async fn batch_apply_balance_changes(
        &self,
        changes: &HashMap<(i64, Vec<u8>), (i128, Vec<u8>)>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        // Step 1: Get all existing balances in one query
        let keys: Vec<_> = changes.keys().collect();
        let query_tokens: Vec<i64> = keys.iter().map(|(t, _)| *t).collect();
        let query_locks: Vec<&[u8]> = keys.iter().map(|(_, l)| l.as_slice()).collect();

        let existing: Vec<(i64, Vec<u8>, String)> = sqlx::query_as(
            r#"
            SELECT tb.token_id, tb.lock_script_hash, tb.balance::text
            FROM token_balances tb
            INNER JOIN (SELECT * FROM UNNEST($1::bigint[], $2::bytea[]) AS t(token_id, lock_hash)) q
            ON tb.token_id = q.token_id AND tb.lock_script_hash = q.lock_hash
            "#,
        )
        .bind(&query_tokens)
        .bind(&query_locks)
        .fetch_all(&self.pool)
        .await?;

        let existing_map: HashMap<(i64, Vec<u8>), i128> = existing
            .into_iter()
            .map(|(t, l, b)| ((t, l), b.parse::<i128>().unwrap_or(0)))
            .collect();

        // Step 2: Categorize into insert/update/delete
        let mut to_insert: Vec<(i64, Vec<u8>, i128, Vec<u8>)> = Vec::new();
        let mut to_update: Vec<(i64, Vec<u8>, i128, Vec<u8>)> = Vec::new();
        let mut to_delete: Vec<(i64, Vec<u8>)> = Vec::new();
        let mut tokens_with_holder_increase: HashMap<i64, i64> = HashMap::new();
        let mut tokens_with_holder_decrease: HashMap<i64, i64> = HashMap::new();

        for ((token_id, lock_hash), (delta, last_tx)) in changes {
            let key = (*token_id, lock_hash.clone());
            let old_balance = existing_map.get(&key).copied().unwrap_or(0);
            let new_balance = (old_balance + delta).max(0);

            if existing_map.contains_key(&key) {
                // Existing record
                if new_balance == 0 {
                    to_delete.push((*token_id, lock_hash.clone()));
                    *tokens_with_holder_decrease.entry(*token_id).or_default() += 1;
                } else {
                    to_update.push((*token_id, lock_hash.clone(), new_balance, last_tx.clone()));
                }
            } else if new_balance > 0 {
                // New holder
                to_insert.push((*token_id, lock_hash.clone(), new_balance, last_tx.clone()));
                *tokens_with_holder_increase.entry(*token_id).or_default() += 1;
            }
        }

        // Step 3: Batch delete (zero balances)
        if !to_delete.is_empty() {
            let del_tokens: Vec<i64> = to_delete.iter().map(|(t, _)| *t).collect();
            let del_locks: Vec<&[u8]> = to_delete.iter().map(|(_, l)| l.as_slice()).collect();

            sqlx::query(
                r#"
                DELETE FROM token_balances tb
                USING (SELECT * FROM UNNEST($1::bigint[], $2::bytea[]) AS t(token_id, lock_hash)) d
                WHERE tb.token_id = d.token_id AND tb.lock_script_hash = d.lock_hash
                "#,
            )
            .bind(&del_tokens)
            .bind(&del_locks)
            .execute(&self.pool)
            .await?;
        }

        // Step 4: Batch insert (new holders)
        if !to_insert.is_empty() {
            let ins_tokens: Vec<i64> = to_insert.iter().map(|(t, _, _, _)| *t).collect();
            let ins_locks: Vec<&[u8]> = to_insert.iter().map(|(_, l, _, _)| l.as_slice()).collect();
            let ins_balances: Vec<String> =
                to_insert.iter().map(|(_, _, b, _)| b.to_string()).collect();
            let ins_txs: Vec<&[u8]> = to_insert.iter().map(|(_, _, _, t)| t.as_slice()).collect();

            sqlx::query(
                r#"
                INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
                SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::numeric[], $4::bytea[], $4::bytea[])
                ON CONFLICT (token_id, lock_script_hash) DO UPDATE SET
                    balance = EXCLUDED.balance,
                    last_tx = EXCLUDED.last_tx,
                    updated_at = NOW()
                "#,
            )
            .bind(&ins_tokens)
            .bind(&ins_locks)
            .bind(&ins_balances)
            .bind(&ins_txs)
            .execute(&self.pool)
            .await?;
        }

        // Step 5: Batch update (existing holders with changed balance)
        if !to_update.is_empty() {
            let upd_tokens: Vec<i64> = to_update.iter().map(|(t, _, _, _)| *t).collect();
            let upd_locks: Vec<&[u8]> = to_update.iter().map(|(_, l, _, _)| l.as_slice()).collect();
            let upd_balances: Vec<String> =
                to_update.iter().map(|(_, _, b, _)| b.to_string()).collect();
            let upd_txs: Vec<&[u8]> = to_update.iter().map(|(_, _, _, t)| t.as_slice()).collect();

            sqlx::query(
                r#"
                UPDATE token_balances tb SET
                    balance = v.balance::numeric,
                    last_tx = v.last_tx,
                    updated_at = NOW()
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::text[], $4::bytea[]) AS t(token_id, lock_hash, balance, last_tx)) v
                WHERE tb.token_id = v.token_id AND tb.lock_script_hash = v.lock_hash
                "#,
            )
            .bind(&upd_tokens)
            .bind(&upd_locks)
            .bind(&upd_balances)
            .bind(&upd_txs)
            .execute(&self.pool)
            .await?;
        }

        // Step 6: Update holders_count for affected tokens
        let mut holder_changes: HashMap<i64, i64> = HashMap::new();
        for (token_id, inc) in tokens_with_holder_increase {
            *holder_changes.entry(token_id).or_default() += inc;
        }
        for (token_id, dec) in tokens_with_holder_decrease {
            *holder_changes.entry(token_id).or_default() -= dec;
        }

        if !holder_changes.is_empty() {
            let hc_tokens: Vec<i64> = holder_changes.keys().copied().collect();
            let hc_deltas: Vec<i64> = hc_tokens.iter().map(|t| holder_changes[t]).collect();

            sqlx::query(
                r#"
                UPDATE tokens t SET
                    holders_count = GREATEST(0, t.holders_count + v.delta::int),
                    updated_at = NOW()
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[]) AS t(id, delta)) v
                WHERE t.id = v.id
                "#,
            )
            .bind(&hc_tokens)
            .bind(&hc_deltas)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}
