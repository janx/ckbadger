use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use tracing::info;

use super::BatchWriter;

impl BatchWriter {
    pub async fn update_hourly_statistics(
        &self,
        hour: DateTime<Utc>,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO hourly_statistics (
                hour, blocks_count, transactions_count, cells_created, cells_consumed, 
                capacity_transferred
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (hour) DO UPDATE SET
                blocks_count = hourly_statistics.blocks_count + EXCLUDED.blocks_count,
                transactions_count = hourly_statistics.transactions_count + EXCLUDED.transactions_count,
                cells_created = hourly_statistics.cells_created + EXCLUDED.cells_created,
                cells_consumed = hourly_statistics.cells_consumed + EXCLUDED.cells_consumed,
                capacity_transferred = hourly_statistics.capacity_transferred + EXCLUDED.capacity_transferred,
                updated_at = NOW()
            "#,
        )
        .bind(hour)
        .bind(blocks_count)
        .bind(transactions_count)
        .bind(cells_created)
        .bind(cells_consumed)
        .bind(capacity_transferred)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_statistics(
        &self,
        date: NaiveDate,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i64,
        data_size_added: i64,
        data_size_consumed: i64,
        dao_field: Option<&[u8]>,
    ) -> Result<()> {
        let prev_cumulative = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"
            SELECT 
                COALESCE(total_live_cells, 0), 
                COALESCE(total_dead_cells, 0),
                COALESCE(total_all_cells, 0),
                COALESCE(total_data_size, 0)
            FROM daily_statistics
            WHERE date < $1
            ORDER BY date DESC
            LIMIT 1
            "#,
        )
        .bind(date)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or((0, 0, 0, 0));

        let net_cells = (cells_created - cells_consumed) as i64;
        let net_data_size = data_size_added - data_size_consumed;

        let knowledge_size: Option<String> =
            dao_field.and_then(|dao| calculate_knowledge_size(dao).map(|v| v.to_string()));

        sqlx::query(
            r#"
            INSERT INTO daily_statistics (
                date, blocks_count, transactions_count, cells_created, cells_consumed, 
                capacity_transferred, total_live_cells, total_dead_cells, total_all_cells, total_data_size,
                knowledge_size
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $12::numeric)
            ON CONFLICT (date) DO UPDATE SET
                blocks_count = daily_statistics.blocks_count + EXCLUDED.blocks_count,
                transactions_count = daily_statistics.transactions_count + EXCLUDED.transactions_count,
                cells_created = daily_statistics.cells_created + EXCLUDED.cells_created,
                cells_consumed = daily_statistics.cells_consumed + EXCLUDED.cells_consumed,
                capacity_transferred = daily_statistics.capacity_transferred + EXCLUDED.capacity_transferred,
                total_live_cells = daily_statistics.total_live_cells + $4 - $5,
                total_dead_cells = daily_statistics.total_dead_cells + $5,
                total_all_cells = daily_statistics.total_all_cells + $4,
                total_data_size = daily_statistics.total_data_size + $11,
                knowledge_size = COALESCE($12::numeric, daily_statistics.knowledge_size)
            "#,
        )
        .bind(date)
        .bind(blocks_count)
        .bind(transactions_count)
        .bind(cells_created)
        .bind(cells_consumed)
        .bind(capacity_transferred)
        .bind(prev_cumulative.0 + net_cells)
        .bind(prev_cumulative.1 + cells_consumed as i64)
        .bind(prev_cumulative.2 + cells_created as i64)
        .bind(prev_cumulative.3 + net_data_size)
        .bind(net_data_size)
        .bind(knowledge_size)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_block_stats(
        &self,
        date: NaiveDate,
        compact_target: i64,
        uncles_count: i32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO daily_block_stats (date, avg_compact_target, block_count, total_uncles, avg_uncle_rate)
            VALUES ($1, $2, 1, $3, $3::float / 1.0)
            ON CONFLICT (date) DO UPDATE SET
                avg_compact_target = ((daily_block_stats.avg_compact_target * daily_block_stats.block_count + $2) / (daily_block_stats.block_count + 1))::bigint,
                block_count = daily_block_stats.block_count + 1,
                total_uncles = daily_block_stats.total_uncles + $3,
                avg_uncle_rate = (daily_block_stats.total_uncles + $3)::float / (daily_block_stats.block_count + 1)::float
            "#,
        )
        .bind(date)
        .bind(compact_target)
        .bind(uncles_count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_miner_statistics(
        &self,
        lock_script_hash: &[u8],
        block_number: i64,
        date: NaiveDate,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
            VALUES ($1, $2, 1, $3)
            ON CONFLICT (date, miner_lock_hash) DO UPDATE SET
                blocks_count = miner_statistics.blocks_count + 1,
                last_block_number = $3
            "#,
        )
        .bind(date)
        .bind(lock_script_hash)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn upsert_epoch_statistics(
        &self,
        epoch_number: i64,
        block_number: i64,
        epoch_length: i32,
        timestamp: DateTime<Utc>,
        epoch_index: i32,
        transactions_count: i32,
    ) -> Result<()> {
        if epoch_index == 0 {
            sqlx::query(
                r#"
                INSERT INTO epoch_statistics (
                    epoch_number, start_block, blocks_count, length, 
                    start_timestamp, difficulty, transactions_count
                )
                VALUES ($1, $2, 1, $3, $4, 0, $5)
                ON CONFLICT (epoch_number) DO UPDATE SET
                    blocks_count = epoch_statistics.blocks_count + 1,
                    transactions_count = epoch_statistics.transactions_count + $5,
                    updated_at = NOW()
                "#,
            )
            .bind(epoch_number)
            .bind(block_number)
            .bind(epoch_length)
            .bind(timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE epoch_statistics SET
                    end_block = $2,
                    blocks_count = blocks_count + 1,
                    end_timestamp = $3,
                    transactions_count = transactions_count + $4,
                    updated_at = NOW()
                WHERE epoch_number = $1
                "#,
            )
            .bind(epoch_number)
            .bind(block_number)
            .bind(timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update_dao_daily_snapshot(&self, date: NaiveDate) -> Result<()> {
        let stats = sqlx::query_as::<_, (String, i64, String, i64)>(
            r#"
            SELECT 
                COALESCE(SUM(capacity::numeric), 0)::text as total_deposit,
                COUNT(DISTINCT lock_script_hash) as depositors_count,
                COALESCE(SUM(CASE WHEN deposit_timestamp::date = $1 THEN capacity::numeric ELSE 0 END), 0)::text as daily_deposit,
                COUNT(CASE WHEN deposit_timestamp::date = $1 THEN 1 END) as daily_deposit_count
            FROM dao_deposits
            WHERE deposit_timestamp::date <= $1
              AND (withdraw_timestamp IS NULL OR withdraw_timestamp::date > $1)
            "#,
        )
        .bind(date)
        .fetch_one(&self.pool)
        .await?;

        let dao_data = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT dao FROM blocks_index WHERE timestamp::date = $1 ORDER BY number DESC LIMIT 1",
        )
        .bind(date)
        .fetch_optional(&self.pool)
        .await?;

        let total_issuance = dao_data
            .as_ref()
            .and_then(|(dao,)| {
                if dao.len() >= 8 {
                    let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
                    Some(u64::from_le_bytes(bytes).to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "0".to_string());

        // Incremental: get previous day's cumulative values and add today's delta.
        // Falls back to full SUM if no previous snapshot exists (first run or rebuild).
        let secondary_issuance = sqlx::query_as::<_, (String, String, String)>(
            r#"
            SELECT
                CASE WHEN prev.cumulative_burnt IS NOT NULL
                    THEN (prev.cumulative_burnt::numeric + COALESCE(today.daily_burnt, 0))::text
                    ELSE COALESCE((
                        SELECT SUM(burnt)::text FROM block_secondary_issuance WHERE block_timestamp::date <= $1
                    ), '0')
                END,
                CASE WHEN prev.cumulative_mining_reward IS NOT NULL
                    THEN (prev.cumulative_mining_reward::numeric + COALESCE(today.daily_miner, 0))::text
                    ELSE COALESCE((
                        SELECT SUM(miner_secondary)::text FROM block_secondary_issuance WHERE block_timestamp::date <= $1
                    ), '0')
                END,
                CASE WHEN prev.cumulative_deposit_compensation IS NOT NULL
                    THEN (prev.cumulative_deposit_compensation::numeric + COALESCE(today.daily_dao, 0))::text
                    ELSE COALESCE((
                        SELECT SUM(dao_compensation)::text FROM block_secondary_issuance WHERE block_timestamp::date <= $1
                    ), '0')
                END
            FROM (SELECT 1) _dummy
            LEFT JOIN LATERAL (
                SELECT cumulative_burnt, cumulative_mining_reward, cumulative_deposit_compensation
                FROM dao_daily_snapshots
                WHERE date < $1
                ORDER BY date DESC LIMIT 1
            ) prev ON true
            LEFT JOIN LATERAL (
                SELECT
                    SUM(burnt)::numeric as daily_burnt,
                    SUM(miner_secondary)::numeric as daily_miner,
                    SUM(dao_compensation)::numeric as daily_dao
                FROM block_secondary_issuance
                WHERE block_timestamp::date = $1
            ) today ON true
            "#,
        )
        .bind(date)
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|_| ("0".to_string(), "0".to_string(), "0".to_string()));

        sqlx::query(
            r#"
            INSERT INTO dao_daily_snapshots (
                date, total_deposit, depositors_count, daily_deposit, daily_deposit_count, 
                total_issuance, cumulative_burnt, cumulative_mining_reward, cumulative_deposit_compensation,
                dao_data
            )
            VALUES ($1, $2::numeric, $3, $4::numeric, $5, $6::numeric, $7, $8, $9, $10)
            ON CONFLICT (date) DO UPDATE SET
                total_deposit = EXCLUDED.total_deposit,
                depositors_count = EXCLUDED.depositors_count,
                daily_deposit = EXCLUDED.daily_deposit,
                daily_deposit_count = EXCLUDED.daily_deposit_count,
                total_issuance = EXCLUDED.total_issuance,
                cumulative_burnt = EXCLUDED.cumulative_burnt,
                cumulative_mining_reward = EXCLUDED.cumulative_mining_reward,
                cumulative_deposit_compensation = EXCLUDED.cumulative_deposit_compensation,
                dao_data = EXCLUDED.dao_data
            "#,
        )
        .bind(date)
        .bind(&stats.0)
        .bind(stats.1 as i32)
        .bind(&stats.2)
        .bind(stats.3 as i32)
        .bind(&total_issuance)
        .bind(&secondary_issuance.0)
        .bind(&secondary_issuance.1)
        .bind(&secondary_issuance.2)
        .bind(dao_data.map(|(d,)| d))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_previous_block_timestamp(
        &self,
        block_number: i64,
    ) -> Result<Option<DateTime<Utc>>> {
        if block_number <= 0 {
            return Ok(None);
        }

        let row = sqlx::query_as::<_, (DateTime<Utc>,)>(
            "SELECT timestamp FROM blocks_index WHERE number = $1",
        )
        .bind(block_number - 1)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(ts,)| ts))
    }

    pub async fn get_dao_deposits_at_block(&self, block_number: i64) -> Result<u128> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT COALESCE(SUM(capacity::numeric), 0)::text
            FROM dao_deposits
            WHERE deposit_block_number < $1
              AND (withdraw_block IS NULL OR withdraw_block >= $1)
            "#,
        )
        .bind(block_number)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0.parse().unwrap_or(0))
    }

    pub async fn get_previous_epoch_duration_minutes(
        &self,
        epoch_number: i64,
    ) -> Result<Option<f64>> {
        let row = sqlx::query_as::<_, (f64,)>(
            r#"
            SELECT (EXTRACT(EPOCH FROM (MAX(timestamp) - MIN(timestamp))) / 60.0)::float8
            FROM blocks
            WHERE epoch_number = $1
            "#,
        )
        .bind(epoch_number)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(d,)| d))
    }

    pub async fn get_last_epoch_start(
        &self,
        before_block: i64,
    ) -> Result<Option<(i64, DateTime<Utc>)>> {
        let row = sqlx::query_as::<_, (i64, DateTime<Utc>)>(
            r#"
            SELECT epoch_number, timestamp
            FROM blocks
            WHERE number < $1 AND epoch_index = 0
            ORDER BY number DESC
            LIMIT 1
            "#,
        )
        .bind(before_block)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn update_block_time_distribution(&self, _block_time_seconds: i64) -> Result<()> {
        // No-op: block_time_distribution uses sliding window (recent 50K blocks)
        // and is rebuilt periodically via statistics_rebuild task
        Ok(())
    }

    pub async fn update_daily_avg_block_time(
        &self,
        date: NaiveDate,
        block_time_ms: i64,
    ) -> Result<()> {
        if block_time_ms < 0 {
            return Ok(());
        }

        // Use incremental average: new_avg = (old_avg * count + new_value) / (count + 1)
        sqlx::query(
            r#"
            UPDATE daily_statistics
            SET avg_block_time_ms = CASE
                WHEN avg_block_time_ms IS NULL THEN $2
                ELSE ((avg_block_time_ms * (blocks_count - 1) + $2) / blocks_count)::integer
            END,
            updated_at = NOW()
            WHERE date = $1
            "#,
        )
        .bind(date)
        .bind(block_time_ms as i32)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_avg_block_time_batch(
        &self,
        date: NaiveDate,
        avg_block_time_ms: i64,
        block_count: i32,
    ) -> Result<()> {
        if block_count <= 0 {
            return Ok(());
        }

        // Batch update: merge new batch avg with existing avg using weighted average
        sqlx::query(
            r#"
            UPDATE daily_statistics
            SET avg_block_time_ms = CASE
                WHEN avg_block_time_ms IS NULL THEN $2
                ELSE ((avg_block_time_ms * (blocks_count - $3) + $2 * $3) / blocks_count)::integer
            END,
            updated_at = NOW()
            WHERE date = $1
            "#,
        )
        .bind(date)
        .bind(avg_block_time_ms as i32)
        .bind(block_count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_epoch_time_distribution(
        &self,
        epoch_number: i64,
        epoch_duration_minutes: f64,
    ) -> Result<()> {
        if epoch_number <= 0 || epoch_duration_minutes < 0.0 {
            return Ok(());
        }

        // Use 1-minute buckets to match official CKB Explorer
        let bucket_minutes = epoch_duration_minutes.round() as i32;

        sqlx::query(
            r#"
            INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
            VALUES ($1, 1)
            ON CONFLICT (bucket_minutes) DO UPDATE SET
                epoch_count = epoch_time_distribution.epoch_count + 1,
                updated_at = NOW()
            "#,
        )
        .bind(bucket_minutes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_block_stats_batch(
        &self,
        date: NaiveDate,
        avg_compact_target: i64,
        block_count: i32,
        total_uncles: i32,
    ) -> Result<()> {
        let avg_uncle_rate = if block_count > 0 {
            total_uncles as f64 / block_count as f64
        } else {
            0.0
        };

        sqlx::query(
            r#"
            INSERT INTO daily_block_stats (date, avg_compact_target, block_count, total_uncles, avg_uncle_rate)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (date) DO UPDATE SET
                avg_compact_target = ((daily_block_stats.avg_compact_target * daily_block_stats.block_count + $2 * $3) / (daily_block_stats.block_count + $3))::bigint,
                block_count = daily_block_stats.block_count + $3,
                total_uncles = daily_block_stats.total_uncles + $4,
                avg_uncle_rate = (daily_block_stats.total_uncles + $4)::float / (daily_block_stats.block_count + $3)::float
            "#,
        )
        .bind(date)
        .bind(avg_compact_target)
        .bind(block_count)
        .bind(total_uncles)
        .bind(avg_uncle_rate)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_miner_statistics_batch(
        &self,
        lock_script_hash: &[u8],
        last_block_number: i64,
        date: NaiveDate,
        blocks_count: i32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (date, miner_lock_hash) DO UPDATE SET
                blocks_count = miner_statistics.blocks_count + $3,
                last_block_number = GREATEST(miner_statistics.last_block_number, $4)
            "#,
        )
        .bind(date)
        .bind(lock_script_hash)
        .bind(blocks_count)
        .bind(last_block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn upsert_epoch_statistics_batch(
        &self,
        epoch_number: i64,
        start_block: i64,
        end_block: i64,
        epoch_length: i32,
        start_timestamp: DateTime<Utc>,
        end_timestamp: DateTime<Utc>,
        transactions_count: i32,
        is_new: bool,
    ) -> Result<()> {
        if is_new {
            sqlx::query(
                r#"
                INSERT INTO epoch_statistics (
                    epoch_number, start_block, end_block, blocks_count, length, 
                    start_timestamp, end_timestamp, difficulty, transactions_count
                )
                VALUES ($1, $2, $3, $3 - $2 + 1, $4, $5, $6, 0, $7)
                ON CONFLICT (epoch_number) DO UPDATE SET
                    end_block = GREATEST(epoch_statistics.end_block, EXCLUDED.end_block),
                    blocks_count = GREATEST(epoch_statistics.end_block, EXCLUDED.end_block) - epoch_statistics.start_block + 1,
                    end_timestamp = EXCLUDED.end_timestamp,
                    transactions_count = epoch_statistics.transactions_count + EXCLUDED.transactions_count,
                    updated_at = NOW()
                "#,
            )
            .bind(epoch_number)
            .bind(start_block)
            .bind(end_block)
            .bind(epoch_length)
            .bind(start_timestamp)
            .bind(end_timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE epoch_statistics SET
                    end_block = GREATEST(end_block, $2),
                    blocks_count = GREATEST(end_block, $2) - start_block + 1,
                    end_timestamp = $3,
                    transactions_count = transactions_count + $4,
                    updated_at = NOW()
                WHERE epoch_number = $1
                "#,
            )
            .bind(epoch_number)
            .bind(end_block)
            .bind(end_timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update_block_time_distribution_batch(
        &self,
        _bucket_seconds: i32,
        _count: i32,
    ) -> Result<()> {
        // No-op: rebuilt periodically via statistics_rebuild task
        Ok(())
    }

    pub async fn update_epoch_time_distribution_batch(
        &self,
        bucket_minutes: i32,
        count: i32,
    ) -> Result<()> {
        if bucket_minutes < 0 {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
            VALUES ($1, $2)
            ON CONFLICT (bucket_minutes) DO UPDATE SET
                epoch_count = epoch_time_distribution.epoch_count + $2,
                updated_at = NOW()
            "#,
        )
        .bind(bucket_minutes)
        .bind(count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn refresh_token_24h_transfers(&self) -> Result<u64> {
        // Optimized: Single GROUP BY scan instead of N+1 correlated subqueries
        // This reduces query time from ~15s to <1s for 700+ tokens
        let result = sqlx::query(
            r#"
            WITH block_24h_ago AS (
                SELECT COALESCE(
                    (SELECT number FROM blocks_index
                     WHERE timestamp >= (SELECT MAX(timestamp) - INTERVAL '24 hours' FROM blocks_index)
                     ORDER BY number ASC LIMIT 1),
                    0
                ) as block_num
            ),
            transfer_counts AS (
                SELECT type_script_hash, COUNT(*) as cnt
                FROM cells
                WHERE created_at_block >= (SELECT block_num FROM block_24h_ago)
                  AND type_script_hash IS NOT NULL
                GROUP BY type_script_hash
            )
            UPDATE tokens t 
            SET transfers_24h = COALESCE(tc.cnt, 0),
                updated_at = NOW()
            FROM transfer_counts tc
            WHERE t.type_script_hash = tc.type_script_hash
            "#,
        )
        .execute(&self.pool)
        .await?;

        let updated_with_transfers = result.rows_affected();

        // Reset tokens with no transfers in 24h to 0
        let reset_result = sqlx::query(
            r#"
            WITH block_24h_ago AS (
                SELECT COALESCE(
                    (SELECT number FROM blocks_index
                     WHERE timestamp >= (SELECT MAX(timestamp) - INTERVAL '24 hours' FROM blocks_index)
                     ORDER BY number ASC LIMIT 1),
                    0
                ) as block_num
            ),
            active_tokens AS (
                SELECT DISTINCT type_script_hash
                FROM cells
                WHERE created_at_block >= (SELECT block_num FROM block_24h_ago)
                  AND type_script_hash IS NOT NULL
            )
            UPDATE tokens t
            SET transfers_24h = 0,
                updated_at = NOW()
            WHERE t.transfers_24h > 0
              AND NOT EXISTS (
                  SELECT 1 FROM active_tokens at 
                  WHERE at.type_script_hash = t.type_script_hash
              )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(updated_with_transfers + reset_result.rows_affected())
    }

    pub async fn refresh_mnft_24h_transfers(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            WITH block_24h_ago AS (
                SELECT COALESCE(
                    (SELECT number FROM blocks_index
                     WHERE timestamp >= (SELECT MAX(timestamp) - INTERVAL '24 hours' FROM blocks_index)
                     ORDER BY number ASC LIMIT 1),
                    0
                ) as block_num
            ),
            transfer_counts AS (
                SELECT 
                    SUBSTRING(asset_id FROM 1 FOR 24) AS class_id,
                    COUNT(*) as cnt
                FROM activities
                WHERE activity_category = 'nft' 
                  AND metadata->>'nftType' = 'mnft'
                  AND block_number >= (SELECT block_num FROM block_24h_ago)
                GROUP BY SUBSTRING(asset_id FROM 1 FOR 24)
            )
            UPDATE mnft_classes mc
            SET transfers_24h = COALESCE(tc.cnt, 0)::int,
                updated_at = NOW()
            FROM transfer_counts tc
            WHERE mc.class_id = tc.class_id
            "#,
        )
        .execute(&self.pool)
        .await?;

        let updated_with_transfers = result.rows_affected();

        let reset_result = sqlx::query(
            r#"
            WITH block_24h_ago AS (
                SELECT COALESCE(
                    (SELECT number FROM blocks_index
                     WHERE timestamp >= (SELECT MAX(timestamp) - INTERVAL '24 hours' FROM blocks_index)
                     ORDER BY number ASC LIMIT 1),
                    0
                ) as block_num
            ),
            active_classes AS (
                SELECT DISTINCT SUBSTRING(asset_id FROM 1 FOR 24) AS class_id
                FROM activities
                WHERE activity_category = 'nft' 
                  AND metadata->>'nftType' = 'mnft'
                  AND block_number >= (SELECT block_num FROM block_24h_ago)
            )
            UPDATE mnft_classes mc
            SET transfers_24h = 0,
                updated_at = NOW()
            WHERE mc.transfers_24h > 0
              AND NOT EXISTS (
                  SELECT 1 FROM active_classes ac 
                  WHERE ac.class_id = mc.class_id
              )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(updated_with_transfers + reset_result.rows_affected())
    }

    pub async fn rebuild_mnft_statistics(&self) -> Result<u64> {
        info!("Rebuilding mnft_classes statistics (holders_count, transfers_count)...");

        let result = sqlx::query(
            r#"
            WITH holder_stats AS (
                SELECT 
                    SUBSTRING(asset_id FROM 1 FOR 24) AS class_id,
                    COUNT(DISTINCT to_lock_hash) AS holders_count
                FROM activities
                WHERE activity_category = 'nft' 
                  AND metadata->>'nftType' = 'mnft'
                  AND activity_type != 'NFT_BURN'
                GROUP BY SUBSTRING(asset_id FROM 1 FOR 24)
            ),
            transfer_stats AS (
                SELECT 
                    SUBSTRING(asset_id FROM 1 FOR 24) AS class_id,
                    COUNT(*) AS transfers_count
                FROM activities
                WHERE activity_category = 'nft' 
                  AND metadata->>'nftType' = 'mnft'
                GROUP BY SUBSTRING(asset_id FROM 1 FOR 24)
            )
            UPDATE mnft_classes mc
            SET holders_count = COALESCE(h.holders_count, 0)::int,
                transfers_count = COALESCE(t.transfers_count, 0),
                updated_at = NOW()
            FROM holder_stats h
            FULL OUTER JOIN transfer_stats t ON h.class_id = t.class_id
            WHERE mc.class_id = COALESCE(h.class_id, t.class_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!(
            "Rebuilt mnft_classes statistics for {} classes",
            result.rows_affected()
        );
        Ok(result.rows_affected())
    }

    pub async fn rebuild_all_statistics(&self) -> Result<()> {
        info!("Rebuilding all statistics after bulk sync completion...");

        self.rebuild_daily_statistics().await?;
        self.rebuild_daily_block_stats().await?;
        self.rebuild_hourly_statistics().await?;
        self.rebuild_miner_statistics().await?;
        self.rebuild_block_time_distribution().await?;
        self.rebuild_epoch_time_distribution().await?;
        self.rebuild_dao_daily_snapshots().await?;
        self.rebuild_mnft_statistics().await?;

        info!("All statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_daily_statistics(&self) -> Result<()> {
        info!("Rebuilding daily_statistics...");
        sqlx::query("TRUNCATE TABLE daily_statistics")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO daily_statistics (
                date, blocks_count, transactions_count, cells_created, cells_consumed,
                capacity_transferred, total_live_cells, total_dead_cells, total_all_cells,
                total_data_size, avg_block_time_ms
            )
            WITH block_times AS (
                SELECT
                    number,
                    timestamp,
                    timestamp::date as date,
                    tx_count,
                    EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
                FROM blocks_index
            ),
            daily_blocks AS (
                SELECT
                    date,
                    COUNT(*) as blocks_count,
                    SUM(tx_count) as transactions_count,
                    AVG(block_time_ms)::int as avg_block_time_ms
                FROM block_times
                GROUP BY date
            ),
            daily_cells AS (
                SELECT
                    b.timestamp::date as date,
                    SUM(CASE WHEN c.created_at_block = b.number THEN 1 ELSE 0 END) as cells_created,
                    SUM(CASE WHEN c.consumed_at_block = b.number THEN 1 ELSE 0 END) as cells_consumed,
                    SUM(CASE WHEN c.created_at_block = b.number THEN c.capacity ELSE 0 END) as capacity_transferred,
                    SUM(CASE WHEN c.created_at_block = b.number THEN c.data_size ELSE 0 END) as data_size_added,
                    SUM(CASE WHEN c.consumed_at_block = b.number THEN c.data_size ELSE 0 END) as data_size_consumed
                FROM blocks_index b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                GROUP BY b.timestamp::date
            )
            SELECT 
                db.date,
                db.blocks_count::int,
                db.transactions_count::int,
                COALESCE(dc.cells_created, 0)::int,
                COALESCE(dc.cells_consumed, 0)::int,
                COALESCE(dc.capacity_transferred, 0),
                SUM(COALESCE(dc.cells_created, 0) - COALESCE(dc.cells_consumed, 0)) 
                    OVER (ORDER BY db.date) as total_live_cells,
                SUM(COALESCE(dc.cells_consumed, 0)) 
                    OVER (ORDER BY db.date) as total_dead_cells,
                SUM(COALESCE(dc.cells_created, 0)) 
                    OVER (ORDER BY db.date) as total_all_cells,
                SUM(COALESCE(dc.data_size_added, 0) - COALESCE(dc.data_size_consumed, 0)) 
                    OVER (ORDER BY db.date) as total_data_size,
                db.avg_block_time_ms
            FROM daily_blocks db
            LEFT JOIN daily_cells dc ON db.date = dc.date
            ORDER BY db.date
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("daily_statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_daily_block_stats(&self) -> Result<()> {
        info!("Rebuilding daily_block_stats...");
        sqlx::query("TRUNCATE TABLE daily_block_stats")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO daily_block_stats (
                date, avg_compact_target, block_count, total_uncles, avg_uncle_rate, avg_block_time_ms
            )
            WITH block_times AS (
                SELECT
                    number,
                    timestamp,
                    timestamp::date as date,
                    compact_target,
                    uncles_count,
                    EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
                FROM blocks_index
            )
            SELECT 
                date,
                AVG(compact_target)::bigint as avg_compact_target,
                COUNT(*)::int as block_count,
                SUM(uncles_count)::int as total_uncles,
                SUM(uncles_count)::float / NULLIF(COUNT(*), 0)::float as avg_uncle_rate,
                AVG(block_time_ms)::int as avg_block_time_ms
            FROM block_times
            GROUP BY date
            ORDER BY date
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("daily_block_stats rebuild completed");
        Ok(())
    }

    async fn rebuild_hourly_statistics(&self) -> Result<()> {
        info!("Rebuilding hourly_statistics...");
        sqlx::query("TRUNCATE TABLE hourly_statistics")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO hourly_statistics (
                hour, blocks_count, transactions_count, cells_created, cells_consumed, capacity_transferred
            )
            WITH hourly_blocks AS (
                SELECT
                    date_trunc('hour', timestamp) as hour,
                    COUNT(*) as blocks_count,
                    SUM(tx_count) as transactions_count
                FROM blocks_index
                GROUP BY date_trunc('hour', timestamp)
            ),
            hourly_cells AS (
                SELECT
                    date_trunc('hour', b.timestamp) as hour,
                    SUM(CASE WHEN c.created_at_block = b.number THEN 1 ELSE 0 END) as cells_created,
                    SUM(CASE WHEN c.consumed_at_block = b.number THEN 1 ELSE 0 END) as cells_consumed,
                    SUM(CASE WHEN c.created_at_block = b.number THEN c.capacity ELSE 0 END) as capacity_transferred
                FROM blocks_index b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                GROUP BY date_trunc('hour', b.timestamp)
            )
            SELECT 
                hb.hour,
                hb.blocks_count::int,
                hb.transactions_count::int,
                COALESCE(hc.cells_created, 0)::int,
                COALESCE(hc.cells_consumed, 0)::int,
                COALESCE(hc.capacity_transferred, 0)
            FROM hourly_blocks hb
            LEFT JOIN hourly_cells hc ON hb.hour = hc.hour
            ORDER BY hb.hour
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("hourly_statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_miner_statistics(&self) -> Result<()> {
        info!("Rebuilding miner_statistics...");
        sqlx::query("TRUNCATE TABLE miner_statistics")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
            SELECT 
                b.timestamp::date as date,
                c.lock_script_hash as miner_lock_hash,
                COUNT(*)::int as blocks_count,
                MAX(b.number) as last_block_number
            FROM blocks_index b
            JOIN transactions_index t ON t.block_number = b.number AND t.tx_index = 0
            JOIN cells c ON c.tx_hash = t.hash AND c.output_index = 0
            GROUP BY b.timestamp::date, c.lock_script_hash
            ORDER BY date, blocks_count DESC
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("miner_statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_block_time_distribution(&self) -> Result<()> {
        info!("Rebuilding block_time_distribution (recent 50K blocks, 100ms buckets)...");
        sqlx::query("TRUNCATE TABLE block_time_distribution")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            WITH recent_blocks AS (
                SELECT number, timestamp
                FROM blocks
                WHERE number > 0
                ORDER BY number DESC
                LIMIT 50000
            ),
            block_times AS (
                SELECT 
                    EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
                FROM recent_blocks
            )
            INSERT INTO block_time_distribution (bucket_ms, block_count)
            SELECT 
                LEAST(CEIL(block_time_ms / 100.0)::int * 100, 50000) as bucket_ms,
                COUNT(*) as block_count
            FROM block_times
            WHERE block_time_ms IS NOT NULL AND block_time_ms > 0
            GROUP BY bucket_ms
            ORDER BY bucket_ms
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("block_time_distribution rebuild completed");
        Ok(())
    }

    async fn rebuild_epoch_time_distribution(&self) -> Result<()> {
        info!("Rebuilding epoch_time_distribution...");
        sqlx::query("TRUNCATE TABLE epoch_time_distribution")
            .execute(&self.pool)
            .await?;

        // Use 1-minute buckets to match official CKB Explorer
        sqlx::query(
            r#"
            INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
            SELECT 
                ROUND(epoch_minutes)::int as bucket_minutes,
                COUNT(*) as epoch_count
            FROM (
                SELECT 
                    EXTRACT(EPOCH FROM (end_timestamp - start_timestamp)) / 60 as epoch_minutes
                FROM epoch_statistics
                WHERE end_timestamp IS NOT NULL
            ) epoch_times
            GROUP BY bucket_minutes
            ORDER BY bucket_minutes
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("epoch_time_distribution rebuild completed");
        Ok(())
    }

    async fn rebuild_dao_daily_snapshots(&self) -> Result<()> {
        info!("Rebuilding dao_daily_snapshots...");
        sqlx::query("TRUNCATE TABLE dao_daily_snapshots")
            .execute(&self.pool)
            .await?;

        // Single query with window functions: O(n) instead of O(n²)
        sqlx::query(
            r#"
            INSERT INTO dao_daily_snapshots (
                date, total_deposit, depositors_count, daily_deposit, daily_deposit_count,
                total_issuance, cumulative_burnt, cumulative_mining_reward, cumulative_deposit_compensation,
                dao_data
            )
            WITH 
            dates AS (
                SELECT DISTINCT timestamp::date as date FROM blocks
            ),
            secondary_daily AS (
                SELECT 
                    block_timestamp::date as date,
                    SUM(burnt)::numeric as daily_burnt,
                    SUM(miner_secondary)::numeric as daily_miner,
                    SUM(dao_compensation)::numeric as daily_dao
                FROM block_secondary_issuance
                GROUP BY block_timestamp::date
            ),
            secondary_cumulative AS (
                SELECT 
                    date,
                    SUM(daily_burnt) OVER w as cumulative_burnt,
                    SUM(daily_miner) OVER w as cumulative_mining_reward,
                    SUM(daily_dao) OVER w as cumulative_deposit_compensation
                FROM secondary_daily
                WINDOW w AS (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
            ),
            last_blocks AS (
                SELECT DISTINCT ON (timestamp::date)
                    timestamp::date as date,
                    dao
                FROM blocks
                ORDER BY timestamp::date, number DESC
            ),
            deposit_events AS (
                SELECT deposit_timestamp::date as date, capacity, lock_script_hash, 1 as event_type
                FROM dao_deposits
                UNION ALL
                SELECT withdraw_timestamp::date as date, capacity, lock_script_hash, -1 as event_type
                FROM dao_deposits
                WHERE withdraw_timestamp IS NOT NULL
            ),
            deposit_daily AS (
                SELECT 
                    date,
                    SUM(capacity * event_type)::numeric as delta_capacity,
                    SUM(CASE WHEN event_type = 1 THEN capacity ELSE 0 END)::numeric as daily_deposit,
                    COUNT(*) FILTER (WHERE event_type = 1)::int as daily_deposit_count
                FROM deposit_events
                GROUP BY date
            ),
            deposit_cumulative AS (
                SELECT 
                    date,
                    daily_deposit,
                    daily_deposit_count,
                    SUM(delta_capacity) OVER w as total_deposit
                FROM deposit_daily
                WINDOW w AS (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
            ),
            depositor_spans AS (
                SELECT 
                    lock_script_hash,
                    MIN(deposit_timestamp::date) as first_deposit,
                    CASE 
                        WHEN bool_or(withdraw_timestamp IS NULL) THEN '9999-12-31'::date
                        ELSE MAX(withdraw_timestamp::date)
                    END as last_active
                FROM dao_deposits
                GROUP BY lock_script_hash
            ),
            depositor_events AS (
                SELECT first_deposit as date, 1 as delta FROM depositor_spans
                UNION ALL
                SELECT last_active as date, -1 as delta FROM depositor_spans WHERE last_active < '9999-12-31'::date
            ),
            depositor_daily AS (
                SELECT date, SUM(delta)::int as delta_depositors
                FROM depositor_events
                GROUP BY date
            ),
            depositor_cumulative AS (
                SELECT 
                    date,
                    SUM(delta_depositors) OVER w as depositors_count
                FROM depositor_daily
                WINDOW w AS (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
            )
            SELECT 
                d.date,
                COALESCE(dc_latest.total_deposit, 0) as total_deposit,
                COALESCE(depc_latest.depositors_count, (
                    SELECT COUNT(DISTINCT lock_script_hash) FROM depositor_spans 
                    WHERE first_deposit <= d.date AND last_active >= d.date
                ))::int as depositors_count,
                COALESCE(dc.daily_deposit, 0) as daily_deposit,
                COALESCE(dc.daily_deposit_count, 0) as daily_deposit_count,
                COALESCE((
                    get_byte(lb.dao, 0)::bigint +
                    get_byte(lb.dao, 1)::bigint * 256 +
                    get_byte(lb.dao, 2)::bigint * 65536 +
                    get_byte(lb.dao, 3)::bigint * 16777216 +
                    get_byte(lb.dao, 4)::bigint * 4294967296::bigint +
                    get_byte(lb.dao, 5)::bigint * 1099511627776::bigint +
                    get_byte(lb.dao, 6)::bigint * 281474976710656::bigint +
                    get_byte(lb.dao, 7)::bigint * 72057594037927936::bigint
                )::numeric, 0) as total_issuance,
                COALESCE(sc.cumulative_burnt::text, '0') as cumulative_burnt,
                COALESCE(sc.cumulative_mining_reward::text, '0') as cumulative_mining_reward,
                COALESCE(sc.cumulative_deposit_compensation::text, '0') as cumulative_deposit_compensation,
                lb.dao as dao_data
            FROM dates d
            LEFT JOIN deposit_cumulative dc ON d.date = dc.date
            LEFT JOIN LATERAL (
                SELECT total_deposit FROM deposit_cumulative WHERE date <= d.date ORDER BY date DESC LIMIT 1
            ) dc_latest ON true
            LEFT JOIN depositor_cumulative depc ON d.date = depc.date
            LEFT JOIN LATERAL (
                SELECT depositors_count FROM depositor_cumulative WHERE date <= d.date ORDER BY date DESC LIMIT 1
            ) depc_latest ON true
            LEFT JOIN last_blocks lb ON d.date = lb.date
            LEFT JOIN secondary_cumulative sc ON d.date = sc.date
            ORDER BY d.date
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("dao_daily_snapshots rebuild completed");
        Ok(())
    }
}

/// Calculates knowledge_size from DAO field bytes.
/// Formula: U field (bytes 24-32, little-endian u64) - BURN_ADJUSTMENT
/// where BURN_ADJUSTMENT = 8,400,000,000 CKB * 0.6 * 10^8 shannons = 504,000,000,000,000,000
///
/// This matches the official CKB Explorer formula:
/// `knowledge_size = dao.U - (BURN_QUOTA * 0.6)`
///
/// The U field represents the total occupied capacity (Common Knowledge Size) in the DAO header.
pub fn calculate_knowledge_size(dao_field: &[u8]) -> Option<i128> {
    // BURN_QUOTA * 0.6 in shannons = 8,400,000,000 CKB * 0.6 * 10^8
    const BURN_ADJUSTMENT: i128 = 504_000_000_000_000_000;

    if dao_field.len() >= 32 {
        let bytes: [u8; 8] = dao_field[24..32].try_into().ok()?;
        let u_field = u64::from_le_bytes(bytes) as i128;
        Some(u_field - BURN_ADJUSTMENT)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // BURN_ADJUSTMENT constant for tests
    const BURN_ADJUSTMENT: i128 = 504_000_000_000_000_000;

    #[test]
    fn test_calculate_knowledge_size_extracts_u_field() {
        // Create a mock DAO field (32 bytes)
        // DAO structure: [0..8]=C, [8..16]=AR, [16..24]=S, [24..32]=U
        let mut dao = vec![0u8; 32];

        // Set U field (bytes 24-32) to a known value: 600,000,000,000,000,000 shannons
        // In little-endian
        let u_value: u64 = 600_000_000_000_000_000;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao);
        assert!(result.is_some());

        // Expected: 600,000,000,000,000,000 - 504,000,000,000,000,000 = 96,000,000,000,000,000
        let expected = u_value as i128 - BURN_ADJUSTMENT;
        assert_eq!(result.unwrap(), expected);
        assert_eq!(result.unwrap(), 96_000_000_000_000_000);
    }

    #[test]
    fn test_calculate_knowledge_size_handles_real_mainnet_value() {
        // Real mainnet DAO field from block ~14,000,000 (approximate)
        // U field should be around 500+ quadrillion shannons
        let mut dao = vec![0u8; 32];

        // Simulate a realistic U value: ~520 quadrillion shannons (~5200 billion CKB occupied)
        let u_value: u64 = 520_000_000_000_000_000;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao);
        assert!(result.is_some());

        // After subtracting BURN_ADJUSTMENT, should be positive
        let knowledge_size = result.unwrap();
        assert!(knowledge_size > 0);

        // Expected: 520 - 504 = 16 quadrillion shannons = 160 billion CKB
        assert_eq!(knowledge_size, 16_000_000_000_000_000);
    }

    #[test]
    fn test_calculate_knowledge_size_returns_none_for_short_dao() {
        // DAO field too short (< 32 bytes)
        let short_dao = vec![0u8; 24];
        assert!(calculate_knowledge_size(&short_dao).is_none());

        let empty_dao: Vec<u8> = vec![];
        assert!(calculate_knowledge_size(&empty_dao).is_none());
    }

    #[test]
    fn test_calculate_knowledge_size_handles_minimum_u_value() {
        // U field at exactly BURN_ADJUSTMENT should give 0
        let mut dao = vec![0u8; 32];
        let u_value: u64 = BURN_ADJUSTMENT as u64;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_calculate_knowledge_size_handles_u_below_burn_adjustment() {
        // If U is below BURN_ADJUSTMENT (shouldn't happen in practice), result is negative
        let mut dao = vec![0u8; 32];
        let u_value: u64 = 400_000_000_000_000_000; // Less than BURN_ADJUSTMENT
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao);
        assert!(result.is_some());
        assert!(result.unwrap() < 0);
    }

    #[test]
    fn test_calculate_knowledge_size_uses_little_endian() {
        // Verify little-endian byte order is correctly used
        let mut dao = vec![0u8; 32];

        // Set U field to 0x0102030405060708 (little-endian stored as 08 07 06 05 04 03 02 01)
        dao[24] = 0x08;
        dao[25] = 0x07;
        dao[26] = 0x06;
        dao[27] = 0x05;
        dao[28] = 0x04;
        dao[29] = 0x03;
        dao[30] = 0x02;
        dao[31] = 0x01;

        let expected_u: u64 = 0x0102030405060708;
        let result = calculate_knowledge_size(&dao);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), expected_u as i128 - BURN_ADJUSTMENT);
    }

    #[test]
    fn test_burn_adjustment_constant_matches_formula() {
        // BURN_QUOTA = 8,400,000,000 CKB (genesis burnt tokens)
        // BURN_ADJUSTMENT = BURN_QUOTA * 0.6 * 10^8 (convert to shannons)
        let burn_quota_ckb: i128 = 8_400_000_000;
        let shannons_per_ckb: i128 = 100_000_000;
        let expected = (burn_quota_ckb * shannons_per_ckb * 6) / 10; // * 0.6

        assert_eq!(BURN_ADJUSTMENT, expected);
        assert_eq!(BURN_ADJUSTMENT, 504_000_000_000_000_000);
    }
}
