use anyhow::{Context, Result};

use super::rows::TransactionRow;
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_transactions(&self, txs: &[TransactionRow]) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<TransactionRow>("transactions_all")
            .await
            .context("Failed to create transactions_all insert")?;

        for tx in txs {
            insert
                .write(tx)
                .await
                .context("Failed to write transaction row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize transactions_all insert")?;

        Ok(())
    }
}
