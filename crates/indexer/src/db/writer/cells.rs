use anyhow::{Context, Result};

use super::rows::CellOutputRow;
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_cell_outputs(&self, cells: &[CellOutputRow]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<CellOutputRow>("cell_outputs_all")
            .await
            .context("Failed to create cell_outputs_all insert")?;

        for cell in cells {
            insert
                .write(cell)
                .await
                .context("Failed to write cell output row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize cell_outputs_all insert")?;

        Ok(())
    }
}
