use anyhow::{Context, Result};

use super::rows::BlockRow;
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_blocks(&self, blocks: &[BlockRow]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<BlockRow>("blocks_all")
            .await
            .context("Failed to create blocks_all insert")?;

        for block in blocks {
            insert
                .write(block)
                .await
                .context("Failed to write block row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize blocks_all insert")?;

        Ok(())
    }
}
