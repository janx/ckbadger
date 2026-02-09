use anyhow::Result;

use crate::parser::mnft::{ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken};

use super::BatchWriter;

impl BatchWriter {
    pub async fn insert_mnft_issuer(
        &self,
        issuer: &ParsedMnftIssuer,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO mnft_issuers (
                issuer_id, type_script_hash, name, info, owner_lock_hash,
                classes_count, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (issuer_id) DO UPDATE SET
                name = COALESCE(EXCLUDED.name, mnft_issuers.name),
                info = COALESCE(EXCLUDED.info, mnft_issuers.info),
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                classes_count = EXCLUDED.classes_count,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&issuer.issuer_id)
        .bind(&issuer.type_script_hash)
        .bind(&issuer.name)
        .bind(&issuer.info)
        .bind(&issuer.owner_lock_hash)
        .bind(issuer.class_count as i32)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_mnft_issuer(
        &self,
        issuer_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE mnft_issuers SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE issuer_id = $1
            "#,
        )
        .bind(issuer_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_mnft_class(
        &self,
        class: &ParsedMnftClass,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO mnft_classes (
                class_id, type_script_hash, issuer_id, name, description, renderer,
                total, issued, owner_lock_hash, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (class_id) DO UPDATE SET
                name = COALESCE(EXCLUDED.name, mnft_classes.name),
                description = COALESCE(EXCLUDED.description, mnft_classes.description),
                renderer = COALESCE(EXCLUDED.renderer, mnft_classes.renderer),
                total = EXCLUDED.total,
                issued = EXCLUDED.issued,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&class.class_id)
        .bind(&class.type_script_hash)
        .bind(&class.issuer_id)
        .bind(&class.name)
        .bind(&class.description)
        .bind(&class.renderer)
        .bind(class.total as i32)
        .bind(class.issued as i32)
        .bind(&class.owner_lock_hash)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        // Update issuer's class count
        sqlx::query(
            "UPDATE mnft_issuers SET classes_count = classes_count + 1, updated_at = NOW() WHERE issuer_id = $1",
        )
        .bind(&class.issuer_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_mnft_class(
        &self,
        class_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        let class = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT issuer_id FROM mnft_classes WHERE class_id = $1",
        )
        .bind(class_id)
        .fetch_optional(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE mnft_classes SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE class_id = $1
            "#,
        )
        .bind(class_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        if let Some((issuer_id,)) = class {
            sqlx::query(
                "UPDATE mnft_issuers SET classes_count = GREATEST(0, classes_count - 1), updated_at = NOW() WHERE issuer_id = $1",
            )
            .bind(&issuer_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_mnft_class_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        // Classes don't store outpoint in schema, so we query by type_script_hash
        // This is a limitation - we may need to add tx_hash/output_index to schema
        // For now, return None as classes are identified by class_id
        Ok(None)
    }

    pub async fn insert_mnft_token(
        &self,
        token: &ParsedMnftToken,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO mnft_tokens (
                token_id, type_script_hash, tx_hash, output_index, class_id, token_index,
                characteristic, configure, state, owner_lock_hash, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $3)
            ON CONFLICT (token_id) DO UPDATE SET
                tx_hash = EXCLUDED.tx_hash,
                output_index = EXCLUDED.output_index,
                characteristic = EXCLUDED.characteristic,
                configure = EXCLUDED.configure,
                state = EXCLUDED.state,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&token.token_id)
        .bind(&token.type_script_hash)
        .bind(tx_hash)
        .bind(output_index)
        .bind(&token.class_id)
        .bind(token.token_index as i32)
        .bind(&token.characteristic)
        .bind(token.configure as i16)
        .bind(token.state as i16)
        .bind(&token.owner_lock_hash)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        // Update class issued count and transfers_count
        sqlx::query(
            "UPDATE mnft_classes SET issued = issued + 1, transfers_count = transfers_count + 1, updated_at = NOW() WHERE class_id = $1",
        )
        .bind(&token.class_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_mnft_token(
        &self,
        token_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        let token =
            sqlx::query_as::<_, (Vec<u8>,)>("SELECT class_id FROM mnft_tokens WHERE token_id = $1")
                .bind(token_id)
                .fetch_optional(&self.pool)
                .await?;

        sqlx::query(
            r#"
            UPDATE mnft_tokens SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE token_id = $1
            "#,
        )
        .bind(token_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        if let Some((class_id,)) = token {
            sqlx::query(
                "UPDATE mnft_classes SET issued = GREATEST(0, issued - 1), updated_at = NOW() WHERE class_id = $1",
            )
            .bind(&class_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_mnft_token_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT token_id FROM mnft_tokens WHERE tx_hash = $1 AND output_index = $2",
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id))
    }

    /// Batch lookup: find token_ids for multiple outpoints in a single query.
    /// Returns (tx_hash, output_index, token_id) for matches.
    pub async fn get_mnft_token_ids_by_outpoints_batch(
        &self,
        tx_hashes: &[Vec<u8>],
        output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        if tx_hashes.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>)>(
            r#"
            SELECT m.tx_hash, m.output_index, m.token_id
            FROM mnft_tokens m
            INNER JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
              ON m.tx_hash = t.tx_hash AND m.output_index = t.output_index
            WHERE m.is_live = true
            "#,
        )
        .bind(tx_hashes)
        .bind(output_indices)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}
