use anyhow::Result;

use crate::parser::{ParsedClusterCell, ParsedSporeCell};

use super::BatchWriter;

impl BatchWriter {
    pub async fn insert_spore_cluster(
        &self,
        cluster: &ParsedClusterCell,
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO spore_clusters (
                cluster_id, type_script_hash, name, description, owner_lock_hash,
                created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (cluster_id) DO UPDATE SET
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                updated_at = NOW()
            "#,
        )
        .bind(&cluster.cluster_id)
        .bind(&cluster.type_script_hash)
        .bind(&cluster.name)
        .bind(&cluster.description)
        .bind(&cluster.owner_lock_hash)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_spore_cell(
        &self,
        spore: &ParsedSporeCell,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO spore_cells (
                spore_id, type_script_hash, tx_hash, output_index, cluster_id,
                content_type, content_size, owner_lock_hash, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $3)
            ON CONFLICT (spore_id) DO UPDATE SET
                tx_hash = EXCLUDED.tx_hash,
                output_index = EXCLUDED.output_index,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&spore.spore_id)
        .bind(&spore.type_script_hash)
        .bind(tx_hash)
        .bind(output_index)
        .bind(&spore.cluster_id)
        .bind(&spore.content_type)
        .bind(spore.content.len() as i32)
        .bind(&spore.owner_lock_hash)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        if let Some(ref cluster_id) = spore.cluster_id {
            sqlx::query(
                "UPDATE spore_clusters SET spores_count = spores_count + 1, updated_at = NOW() WHERE cluster_id = $1",
            )
            .bind(cluster_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn insert_spore_content(&self, spore_id: &[u8], content: &[u8]) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO spore_content (spore_id, content)
            VALUES ($1, $2)
            ON CONFLICT (spore_id) DO NOTHING
            "#,
        )
        .bind(spore_id)
        .bind(content)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_spore(
        &self,
        spore_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        let spore = sqlx::query_as::<_, (Option<Vec<u8>>,)>(
            "SELECT cluster_id FROM spore_cells WHERE spore_id = $1",
        )
        .bind(spore_id)
        .fetch_optional(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE spore_cells SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE spore_id = $1
            "#,
        )
        .bind(spore_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        if let Some((Some(cluster_id),)) = spore {
            sqlx::query(
                "UPDATE spore_clusters SET spores_count = GREATEST(0, spores_count - 1), updated_at = NOW() WHERE cluster_id = $1",
            )
            .bind(&cluster_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT spore_id FROM spore_cells WHERE tx_hash = $1 AND output_index = $2",
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id))
    }

    /// Batch lookup: find spore_ids for multiple outpoints in a single query.
    /// Returns (tx_hash, output_index, spore_id) for matches.
    pub async fn get_spore_ids_by_outpoints_batch(
        &self,
        tx_hashes: &[Vec<u8>],
        output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        if tx_hashes.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>)>(
            r#"
            SELECT s.tx_hash, s.output_index, s.spore_id
            FROM spore_cells s
            INNER JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
              ON s.tx_hash = t.tx_hash AND s.output_index = t.output_index
            WHERE s.is_live = true
            "#,
        )
        .bind(tx_hashes)
        .bind(output_indices)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}
