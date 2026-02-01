use anyhow::Result;

use crate::parser::dotbit::ParsedDotbitAccount;

use super::BatchWriter;

impl BatchWriter {
    pub async fn insert_dotbit_account(
        &self,
        account: &ParsedDotbitAccount,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        // For account_name, we use hex-encoded account_id since the parser doesn't extract the human-readable name
        // In a full implementation, this would parse the account name from witness data
        let account_name = format!("0x{}", hex::encode(&account.account_id));

        sqlx::query(
            r#"
            INSERT INTO dotbit_accounts (
                account_id, type_script_hash, tx_hash, output_index, account_name,
                owner_lock_hash, expired_at, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $3)
            ON CONFLICT (account_id) DO UPDATE SET
                tx_hash = EXCLUDED.tx_hash,
                output_index = EXCLUDED.output_index,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                expired_at = EXCLUDED.expired_at,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&account.account_id)
        .bind(&account.type_script_hash)
        .bind(tx_hash)
        .bind(output_index)
        .bind(&account_name)
        .bind(&account.owner_lock_hash)
        .bind(account.expired_at.map(|e| e as i64))
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_dotbit_account(
        &self,
        account_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE dotbit_accounts SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE account_id = $1
            "#,
        )
        .bind(account_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_dotbit_account_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT account_id FROM dotbit_accounts WHERE tx_hash = $1 AND output_index = $2",
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id))
    }
}
