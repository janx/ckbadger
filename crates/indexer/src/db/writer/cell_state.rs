use anyhow::{Context, Result};

use super::rows::CellStateRow;
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_cell_states(&self, states: &[CellStateRow]) -> Result<()> {
        if states.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<CellStateRow>("cell_state")
            .await
            .context("Failed to create cell_state insert")?;

        for state in states {
            insert
                .write(state)
                .await
                .context("Failed to write cell state row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize cell_state insert")?;

        Ok(())
    }
}
