use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

pub struct ControlPlaneClient {
    pool: PgPool,
    instance_id: Uuid,
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

        info!("Control plane connected, instance_id: {}", instance_id);

        Ok(Self { pool, instance_id })
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
