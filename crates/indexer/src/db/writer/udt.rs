use anyhow::Result;

use super::rows::UdtCellRow;
use super::BatchWriter;

impl BatchWriter {
    /// Writes UDT cell rows to `udt_cells` table.
    pub async fn write_udt_cells(&self, rows: &[UdtCellRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert::<UdtCellRow>("udt_cells").await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;

        Ok(())
    }
}
