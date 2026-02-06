use anyhow::{Context, Result};

use super::rows::{to_hash32, CanonicalBlockRow};
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_canonical_blocks(
        &self,
        mappings: &[(u64, Vec<u8>, u64)],
    ) -> Result<()> {
        if mappings.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<CanonicalBlockRow>("canonical_blocks")
            .await
            .context("Failed to create canonical_blocks insert")?;

        for (number, block_hash, canon_version) in mappings {
            let row = CanonicalBlockRow {
                number: *number,
                block_hash: to_hash32(block_hash),
                canon_version: *canon_version,
            };
            insert
                .write(&row)
                .await
                .context("Failed to write canonical block row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize canonical_blocks insert")?;

        Ok(())
    }
}
