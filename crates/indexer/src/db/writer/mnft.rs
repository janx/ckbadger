use anyhow::Result;

use super::rows::{MnftClassRow, MnftIssuerRow, MnftTokenRow};
use super::BatchWriter;

impl BatchWriter {
    /// Writes mNFT issuer rows to `mnft_issuers` table.
    pub async fn write_mnft_issuers(&self, rows: &[MnftIssuerRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert::<MnftIssuerRow>("mnft_issuers").await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Writes mNFT class rows to `mnft_classes` table.
    pub async fn write_mnft_classes(&self, rows: &[MnftClassRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert::<MnftClassRow>("mnft_classes").await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Writes mNFT token rows to `mnft_tokens` table.
    pub async fn write_mnft_tokens(&self, rows: &[MnftTokenRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert::<MnftTokenRow>("mnft_tokens").await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;

        Ok(())
    }
}
