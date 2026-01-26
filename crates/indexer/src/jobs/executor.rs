use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::control_plane::{ControlPlaneClient, Job};

use super::{CyclesFixTask, ScriptLabelsTask, UdtLabelsTask};

pub struct JobExecutor {
    instance_pool: PgPool,
    control_plane: Arc<ControlPlaneClient>,
    ckb_rpc_url: String,
    token_labels_path: Option<String>,
}

impl JobExecutor {
    pub fn new(
        instance_pool: PgPool,
        control_plane: Arc<ControlPlaneClient>,
        ckb_rpc_url: String,
        token_labels_path: Option<String>,
    ) -> Self {
        Self {
            instance_pool,
            control_plane,
            ckb_rpc_url,
            token_labels_path,
        }
    }

    pub async fn run(self) {
        info!("JobExecutor started, polling for jobs...");

        loop {
            if let Some(job) = self.control_plane.claim_next_job().await {
                info!("Claimed job: {} (type: {})", job.id, job.job_type);

                let result = self.execute(&job).await;

                match result {
                    Ok(_) => {
                        info!("Job {} completed successfully", job.id);
                        self.control_plane.complete_job(&job.id).await;
                    }
                    Err(e) => {
                        warn!("Job {} failed: {}", job.id, e);
                        self.control_plane.fail_job(&job.id, &e.to_string()).await;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn execute(&self, job: &Job) -> Result<()> {
        match job.job_type.as_str() {
            "fix_missing_cycles" => {
                CyclesFixTask::new(self.instance_pool.clone(), self.ckb_rpc_url.clone())
                    .run_all(&self.control_plane, &job.id)
                    .await
            }
            "fix_cycles_range" => {
                let (start, end) = self.parse_range(&job.checkpoint)?;
                CyclesFixTask::new(self.instance_pool.clone(), self.ckb_rpc_url.clone())
                    .run_range(start, end, &self.control_plane, &job.id)
                    .await
            }
            "update_udt_labels" => {
                UdtLabelsTask::new(self.instance_pool.clone(), self.token_labels_path.clone())
                    .run(&self.control_plane, &job.id)
                    .await
            }
            "update_script_labels" => {
                ScriptLabelsTask::new(self.instance_pool.clone(), self.token_labels_path.clone())
                    .run(&self.control_plane, &job.id)
                    .await
            }
            other => {
                warn!("Unknown job type: {}, skipping", other);
                Ok(())
            }
        }
    }

    fn parse_range(&self, checkpoint: &Option<serde_json::Value>) -> Result<(i64, i64)> {
        let checkpoint = checkpoint
            .as_ref()
            .ok_or_else(|| anyhow!("Missing checkpoint for fix_cycles_range job"))?;

        let start = checkpoint
            .get("start")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow!("Missing 'start' in checkpoint"))?;

        let end = checkpoint
            .get("end")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow!("Missing 'end' in checkpoint"))?;

        Ok((start, end))
    }
}
