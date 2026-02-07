use anyhow::Result;

use super::rows::{SporeCellRow, SporeClusterRow};
use super::BatchWriter;

impl BatchWriter {
    /// Writes Spore cluster rows to `spore_clusters` table.
    pub async fn write_spore_clusters(&self, rows: &[SporeClusterRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<SporeClusterRow>("spore_clusters")
            .await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Writes Spore cell rows to `spore_cells` table.
    pub async fn write_spore_cells(&self, rows: &[SporeCellRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert::<SporeCellRow>("spore_cells").await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;

        Ok(())
    }
}
