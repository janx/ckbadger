use anyhow::{Context, Result};

use super::rows::CellInputRow;
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_cell_inputs(&self, inputs: &[CellInputRow]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<CellInputRow>("cell_inputs_all")
            .await
            .context("Failed to create cell_inputs_all insert")?;

        for input in inputs {
            insert
                .write(input)
                .await
                .context("Failed to write cell input row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize cell_inputs_all insert")?;

        Ok(())
    }
}
