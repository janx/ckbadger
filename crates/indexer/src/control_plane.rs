use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub job_type: String,
    pub status: String,
    pub progress_current: i64,
    pub progress_total: Option<i64>,
    pub checkpoint: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

pub struct ControlPlaneClient {
    pool: PgPool,
    instance_id: Uuid,
    worker_id: String,
}

impl ControlPlaneClient {
    pub async fn connect(
        control_db_url: &str,
        instance_db_url: &str,
        ckb_rpc_url: &str,
        network: &str,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(control_db_url)
            .await?;

        let instance_id =
            Self::register_or_find_instance(&pool, instance_db_url, ckb_rpc_url, network).await?;

        let worker_id = format!(
            "indexer-{}-{}",
            gethostname::gethostname().to_string_lossy(),
            std::process::id()
        );

        info!(
            "Control plane connected, instance_id: {}, worker_id: {}",
            instance_id, worker_id
        );

        Ok(Self {
            pool,
            instance_id,
            worker_id,
        })
    }

    async fn register_or_find_instance(
        pool: &PgPool,
        database_url: &str,
        ckb_rpc_url: &str,
        network: &str,
    ) -> Result<Uuid> {
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM instances WHERE database_url = $1")
                .bind(database_url)
                .fetch_optional(pool)
                .await?;

        if let Some((id,)) = existing {
            sqlx::query(
                "UPDATE instances SET 
                    ckb_rpc_url = $2, 
                    network = $3, 
                    status = CASE WHEN status = 'created' THEN 'syncing' ELSE status END,
                    last_activity_at = NOW()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(ckb_rpc_url)
            .bind(network)
            .execute(pool)
            .await?;

            info!("Found existing instance: {}", id);
            return Ok(id);
        }

        let name = generate_instance_name(database_url);
        let id: (Uuid,) = sqlx::query_as(
            "INSERT INTO instances (name, database_url, ckb_rpc_url, network, status, sync_phase)
             VALUES ($1, $2, $3, $4, 'syncing', 'core_sync')
             RETURNING id",
        )
        .bind(&name)
        .bind(database_url)
        .bind(ckb_rpc_url)
        .bind(network)
        .fetch_one(pool)
        .await?;

        info!("Registered new instance: {} ({})", name, id.0);
        Ok(id.0)
    }

    pub async fn update_progress(
        &self,
        current_block: u64,
        target_block: u64,
        blocks_per_second: f64,
    ) {
        let result = sqlx::query(
            "UPDATE instances SET 
                current_block = $2,
                target_block = $3,
                sync_speed = $4,
                last_activity_at = NOW()
             WHERE id = $1",
        )
        .bind(self.instance_id)
        .bind(current_block as i64)
        .bind(target_block as i64)
        .bind(blocks_per_second)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            warn!("Failed to update control plane progress: {}", e);
        }
    }

    pub async fn set_status(&self, status: &str, sync_phase: Option<&str>) {
        let result = if let Some(phase) = sync_phase {
            sqlx::query(
                "UPDATE instances SET status = $2, sync_phase = $3, last_activity_at = NOW() WHERE id = $1",
            )
            .bind(self.instance_id)
            .bind(status)
            .bind(phase)
            .execute(&self.pool)
            .await
        } else {
            sqlx::query("UPDATE instances SET status = $2, last_activity_at = NOW() WHERE id = $1")
                .bind(self.instance_id)
                .bind(status)
                .execute(&self.pool)
                .await
        };

        if let Err(e) = result {
            warn!("Failed to update control plane status: {}", e);
        }
    }

    pub async fn set_error(&self, error_message: &str) {
        let result = sqlx::query(
            "UPDATE instances SET 
                status = 'failed',
                last_error = $2,
                last_error_at = NOW(),
                error_count = error_count + 1,
                last_activity_at = NOW()
             WHERE id = $1",
        )
        .bind(self.instance_id)
        .bind(error_message)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            warn!("Failed to update control plane error: {}", e);
        }
    }

    pub fn instance_id(&self) -> Uuid {
        self.instance_id
    }

    pub async fn claim_next_job(&self) -> Option<Job> {
        let result = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                String,
                String,
                i64,
                Option<i64>,
                Option<serde_json::Value>,
                DateTime<Utc>,
            ),
        >(
            r#"
            UPDATE sync_jobs SET 
                status = 'running',
                started_at = NOW(),
                worker_id = $2,
                updated_at = NOW()
            WHERE id = (
                SELECT id FROM sync_jobs 
                WHERE instance_id = $1 
                  AND status = 'pending'
                ORDER BY created_at
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, instance_id, job_type, status, progress_current, 
                      progress_total, checkpoint, created_at
            "#,
        )
        .bind(self.instance_id)
        .bind(&self.worker_id)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some(row)) => Some(Job {
                id: row.0,
                instance_id: row.1,
                job_type: row.2,
                status: row.3,
                progress_current: row.4,
                progress_total: row.5,
                checkpoint: row.6,
                created_at: row.7,
            }),
            Ok(None) => None,
            Err(e) => {
                warn!("Failed to claim job: {}", e);
                None
            }
        }
    }

    pub async fn update_job_progress(
        &self,
        job_id: &Uuid,
        current: i64,
        total: Option<i64>,
        rows_per_second: Option<f64>,
    ) {
        let result = sqlx::query(
            r#"
            UPDATE sync_jobs SET
                progress_current = $2,
                progress_total = COALESCE($3, progress_total),
                rows_per_second = $4,
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .bind(current)
        .bind(total)
        .bind(rows_per_second)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            warn!("Failed to update job progress: {}", e);
        }
    }

    pub async fn complete_job(&self, job_id: &Uuid) {
        let result = sqlx::query(
            "UPDATE sync_jobs SET status = 'completed', completed_at = NOW(), updated_at = NOW() WHERE id = $1",
        )
        .bind(job_id)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            warn!("Failed to complete job: {}", e);
        }
    }

    pub async fn fail_job(&self, job_id: &Uuid, error: &str) {
        let result = sqlx::query(
            r#"
            UPDATE sync_jobs SET 
                status = 'failed', 
                error_message = $2,
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .bind(error)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            warn!("Failed to mark job as failed: {}", e);
        }
    }

    pub async fn is_job_cancelled(&self, job_id: &Uuid) -> bool {
        let result: Option<(String,)> =
            sqlx::query_as("SELECT status FROM sync_jobs WHERE id = $1")
                .bind(job_id)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();

        matches!(result, Some((status,)) if status == "cancelled")
    }
}

fn generate_instance_name(database_url: &str) -> String {
    if let Some(db_name) = database_url.rsplit('/').next() {
        let clean_name = db_name.split('?').next().unwrap_or(db_name);
        if !clean_name.is_empty() {
            return clean_name.to_string();
        }
    }
    format!("instance-{}", &Uuid::new_v4().to_string()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_instance_name_from_url() {
        assert_eq!(
            generate_instance_name("postgres://user:pass@host:5432/ckbadger_mainnet"),
            "ckbadger_mainnet"
        );
        assert_eq!(
            generate_instance_name("postgres://localhost/mydb?sslmode=disable"),
            "mydb"
        );
    }

    #[test]
    fn test_generate_instance_name_fallback() {
        let name = generate_instance_name("postgres://localhost/");
        assert!(name.starts_with("instance-"));
    }
}
