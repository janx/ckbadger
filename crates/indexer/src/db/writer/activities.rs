use anyhow::{Context, Result};

use super::rows::ActivityRow;
use super::BatchWriter;

impl BatchWriter {
    pub async fn write_activities(&self, activities: &[ActivityRow]) -> Result<()> {
        if activities.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<ActivityRow>("activities_all")
            .await
            .context("Failed to create activities_all insert")?;

        for activity in activities {
            insert
                .write(activity)
                .await
                .context("Failed to write activity row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize activities_all insert")?;

        Ok(())
    }
}
