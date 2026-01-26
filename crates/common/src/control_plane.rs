#![allow(dead_code)]

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    #[default]
    Created,
    Syncing,
    Rebuilding,
    Ready,
    Active,
    Failed,
    Archived,
}

impl From<&str> for InstanceStatus {
    fn from(s: &str) -> Self {
        match s {
            "created" => Self::Created,
            "syncing" => Self::Syncing,
            "rebuilding" => Self::Rebuilding,
            "ready" => Self::Ready,
            "active" => Self::Active,
            "failed" => Self::Failed,
            "archived" => Self::Archived,
            _ => Self::Created,
        }
    }
}

impl InstanceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Syncing => "syncing",
            Self::Rebuilding => "rebuilding",
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    #[default]
    Pending,
    CoreSync,
    RebuildLiveCells,
    RebuildBalances,
    RebuildScriptUsage,
    RebuildStatistics,
    RebuildIndexes,
    RebuildAddressTx,
    Completed,
}

impl From<&str> for SyncPhase {
    fn from(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "core_sync" => Self::CoreSync,
            "rebuild_live_cells" => Self::RebuildLiveCells,
            "rebuild_balances" => Self::RebuildBalances,
            "rebuild_script_usage" => Self::RebuildScriptUsage,
            "rebuild_statistics" => Self::RebuildStatistics,
            "rebuild_indexes" => Self::RebuildIndexes,
            "rebuild_address_tx" => Self::RebuildAddressTx,
            "completed" => Self::Completed,
            _ => Self::Pending,
        }
    }
}

impl SyncPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::CoreSync => "core_sync",
            Self::RebuildLiveCells => "rebuild_live_cells",
            Self::RebuildBalances => "rebuild_balances",
            Self::RebuildScriptUsage => "rebuild_script_usage",
            Self::RebuildStatistics => "rebuild_statistics",
            Self::RebuildIndexes => "rebuild_indexes",
            Self::RebuildAddressTx => "rebuild_address_tx",
            Self::Completed => "completed",
        }
    }

    pub fn next(&self) -> Option<SyncPhase> {
        match self {
            Self::Pending => Some(Self::CoreSync),
            Self::CoreSync => Some(Self::RebuildLiveCells),
            Self::RebuildLiveCells => Some(Self::RebuildBalances),
            Self::RebuildBalances => Some(Self::RebuildScriptUsage),
            Self::RebuildScriptUsage => Some(Self::RebuildStatistics),
            Self::RebuildStatistics => Some(Self::RebuildIndexes),
            Self::RebuildIndexes => Some(Self::RebuildAddressTx),
            Self::RebuildAddressTx => Some(Self::Completed),
            Self::Completed => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: Uuid,
    pub name: String,
    pub database_url: String,
    pub ckb_rpc_url: String,
    pub network: String,
    pub status: InstanceStatus,
    pub sync_phase: SyncPhase,
    pub current_block: i64,
    pub target_block: Option<i64>,
    pub sync_speed: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncJob {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub job_type: String,
    pub status: String,
    pub progress_current: i64,
    pub progress_total: Option<i64>,
    pub progress_percent: Option<f64>,
    pub rows_per_second: Option<f64>,
    pub started_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEvent {
    pub id: i64,
    pub instance_id: Option<Uuid>,
    pub event_type: String,
    pub severity: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    pub batch_size: u32,
    pub parallel_fetch_size: u32,
    pub pipeline_buffer: u32,
    pub bulk_sync_mode: bool,
    pub skip_live_cells: bool,
    pub skip_address_transactions: bool,
    pub skip_statistics: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            batch_size: 3000,
            parallel_fetch_size: 64,
            pipeline_buffer: 4,
            bulk_sync_mode: true,
            skip_live_cells: true,
            skip_address_transactions: true,
            skip_statistics: true,
        }
    }
}

pub struct ControlPlane {
    pool: PgPool,
}

impl ControlPlane {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn list_instances(&self) -> Result<Vec<Instance>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                Option<i64>,
                Option<f64>,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT 
                id, name, database_url, ckb_rpc_url, network,
                status, sync_phase, current_block, target_block, 
                sync_speed, created_at, last_activity_at, last_error
            FROM instances
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let instances = rows
            .into_iter()
            .map(|row| Instance {
                id: row.0,
                name: row.1,
                database_url: row.2,
                ckb_rpc_url: row.3,
                network: row.4,
                status: InstanceStatus::from(row.5.as_str()),
                sync_phase: SyncPhase::from(row.6.as_str()),
                current_block: row.7,
                target_block: row.8,
                sync_speed: row.9,
                created_at: row.10,
                last_activity_at: row.11,
                last_error: row.12,
            })
            .collect();

        Ok(instances)
    }

    pub async fn get_instance(&self, instance_id: &Uuid) -> Result<Option<Instance>> {
        let row = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                Option<i64>,
                Option<f64>,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT 
                id, name, database_url, ckb_rpc_url, network,
                status, sync_phase, current_block, target_block, 
                sync_speed, created_at, last_activity_at, last_error
            FROM instances
            WHERE id = $1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Instance {
            id: row.0,
            name: row.1,
            database_url: row.2,
            ckb_rpc_url: row.3,
            network: row.4,
            status: InstanceStatus::from(row.5.as_str()),
            sync_phase: SyncPhase::from(row.6.as_str()),
            current_block: row.7,
            target_block: row.8,
            sync_speed: row.9,
            created_at: row.10,
            last_activity_at: row.11,
            last_error: row.12,
        }))
    }

    pub async fn list_running_jobs(&self) -> Result<Vec<SyncJob>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                String,
                String,
                i64,
                Option<i64>,
                Option<f64>,
                Option<f64>,
                Option<DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT 
                id, instance_id, job_type, status,
                progress_current, progress_total, progress_percent,
                rows_per_second, started_at, error_message
            FROM sync_jobs
            WHERE status IN ('pending', 'queued', 'running')
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let jobs = rows
            .into_iter()
            .map(|row| SyncJob {
                id: row.0,
                instance_id: row.1,
                job_type: row.2,
                status: row.3,
                progress_current: row.4,
                progress_total: row.5,
                progress_percent: row.6,
                rows_per_second: row.7,
                started_at: row.8,
                error_message: row.9,
            })
            .collect();

        Ok(jobs)
    }

    pub async fn list_jobs_for_instance(&self, instance_id: &Uuid) -> Result<Vec<SyncJob>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                String,
                String,
                i64,
                Option<i64>,
                Option<f64>,
                Option<f64>,
                Option<DateTime<Utc>>,
                Option<String>,
            ),
        >(
            r#"
            SELECT 
                id, instance_id, job_type, status,
                progress_current, progress_total, progress_percent,
                rows_per_second, started_at, error_message
            FROM sync_jobs
            WHERE instance_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(instance_id)
        .fetch_all(&self.pool)
        .await?;

        let jobs = rows
            .into_iter()
            .map(|row| SyncJob {
                id: row.0,
                instance_id: row.1,
                job_type: row.2,
                status: row.3,
                progress_current: row.4,
                progress_total: row.5,
                progress_percent: row.6,
                rows_per_second: row.7,
                started_at: row.8,
                error_message: row.9,
            })
            .collect();

        Ok(jobs)
    }

    pub async fn list_recent_events(&self, limit: i64) -> Result<Vec<SyncEvent>> {
        let rows = sqlx::query_as::<_, (i64, Option<Uuid>, String, String, String, DateTime<Utc>)>(
            r#"
            SELECT id, instance_id, event_type, severity, message, created_at
            FROM sync_events
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let events = rows
            .into_iter()
            .map(|row| SyncEvent {
                id: row.0,
                instance_id: row.1,
                event_type: row.2,
                severity: row.3,
                message: row.4,
                created_at: row.5,
            })
            .collect();

        Ok(events)
    }

    pub async fn get_active_instance_id(&self) -> Result<Option<Uuid>> {
        let row = sqlx::query_as::<_, (Option<Uuid>,)>(
            "SELECT instance_id FROM active_instance WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|r| r.0))
    }

    pub async fn get_active_instance(&self) -> Result<Option<Instance>> {
        let id = self.get_active_instance_id().await?;
        match id {
            Some(id) => self.get_instance(&id).await,
            None => Ok(None),
        }
    }

    pub async fn set_active_instance(&self, instance_id: &Uuid, switched_by: &str) -> Result<()> {
        sqlx::query("SELECT set_active_instance($1, $2, NULL)")
            .bind(instance_id)
            .bind(switched_by)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_instance(
        &self,
        name: &str,
        database_url: &str,
        ckb_rpc_url: &str,
        network: &str,
    ) -> Result<Uuid> {
        let row = sqlx::query_as::<_, (Uuid,)>(
            r#"
            INSERT INTO instances (name, database_url, ckb_rpc_url, network)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(database_url)
        .bind(ckb_rpc_url)
        .bind(network)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    pub async fn create_instance_with_config(
        &self,
        name: &str,
        database_url: &str,
        ckb_rpc_url: &str,
        network: &str,
        config: &SyncConfig,
    ) -> Result<Uuid> {
        let config_json = serde_json::to_value(config)?;
        let row = sqlx::query_as::<_, (Uuid,)>(
            r#"
            INSERT INTO instances (name, database_url, ckb_rpc_url, network, config)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(database_url)
        .bind(ckb_rpc_url)
        .bind(network)
        .bind(config_json)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    pub async fn delete_instance(&self, instance_id: &Uuid) -> Result<()> {
        sqlx::query("DELETE FROM instances WHERE id = $1")
            .bind(instance_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_instance_status(
        &self,
        instance_id: &Uuid,
        status: InstanceStatus,
    ) -> Result<()> {
        sqlx::query("UPDATE instances SET status = $1, last_activity_at = NOW() WHERE id = $2")
            .bind(status.as_str())
            .bind(instance_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_instance_progress(
        &self,
        instance_id: &Uuid,
        current_block: i64,
        target_block: Option<i64>,
        sync_speed: Option<f64>,
    ) -> Result<()> {
        sqlx::query("SELECT update_instance_progress($1, $2, $3, $4)")
            .bind(instance_id)
            .bind(current_block)
            .bind(target_block)
            .bind(sync_speed)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn transition_instance_phase(
        &self,
        instance_id: &Uuid,
        new_phase: SyncPhase,
        new_status: Option<InstanceStatus>,
    ) -> Result<()> {
        sqlx::query("SELECT transition_instance_phase($1, $2, $3)")
            .bind(instance_id)
            .bind(new_phase.as_str())
            .bind(new_status.map(|s| s.as_str()))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_instance_error(&self, instance_id: &Uuid, error: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE instances 
            SET last_error = $1, 
                last_error_at = NOW(), 
                error_count = error_count + 1,
                status = 'failed',
                last_activity_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(error)
        .bind(instance_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_job(
        &self,
        instance_id: &Uuid,
        job_type: &str,
        progress_total: Option<i64>,
    ) -> Result<Uuid> {
        let row = sqlx::query_as::<_, (Uuid,)>(
            r#"
            INSERT INTO sync_jobs (instance_id, job_type, progress_total, status)
            VALUES ($1, $2, $3, 'pending')
            RETURNING id
            "#,
        )
        .bind(instance_id)
        .bind(job_type)
        .bind(progress_total)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    pub async fn create_job_with_checkpoint(
        &self,
        instance_id: &Uuid,
        job_type: &str,
        progress_total: Option<i64>,
        checkpoint: &serde_json::Value,
    ) -> Result<Uuid> {
        let row = sqlx::query_as::<_, (Uuid,)>(
            r#"
            INSERT INTO sync_jobs (instance_id, job_type, progress_total, checkpoint, status)
            VALUES ($1, $2, $3, $4, 'pending')
            RETURNING id
            "#,
        )
        .bind(instance_id)
        .bind(job_type)
        .bind(progress_total)
        .bind(checkpoint)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    pub async fn start_job(&self, job_id: &Uuid, worker_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_jobs 
            SET status = 'running', started_at = NOW(), worker_id = $1, updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(worker_id)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_job_progress(
        &self,
        job_id: &Uuid,
        progress_current: i64,
        rows_per_second: Option<f64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_jobs 
            SET progress_current = $1, rows_per_second = $2, updated_at = NOW()
            WHERE id = $3
            "#,
        )
        .bind(progress_current)
        .bind(rows_per_second)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_job(&self, job_id: &Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_jobs 
            SET status = 'completed', completed_at = NOW(), updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fail_job(&self, job_id: &Uuid, error: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_jobs 
            SET status = 'failed', 
                error_message = $1, 
                updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(error)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cancel_job(&self, job_id: &Uuid) -> Result<()> {
        sqlx::query("UPDATE sync_jobs SET status = 'cancelled', updated_at = NOW() WHERE id = $1")
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn log_event(
        &self,
        instance_id: Option<&Uuid>,
        job_id: Option<&Uuid>,
        event_type: &str,
        severity: &str,
        message: &str,
        details: Option<&serde_json::Value>,
        source: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO sync_events (instance_id, job_id, event_type, severity, message, details, source)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(instance_id)
        .bind(job_id)
        .bind(event_type)
        .bind(severity)
        .bind(message)
        .bind(details)
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_system_config(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let row =
            sqlx::query_as::<_, (serde_json::Value,)>("SELECT value FROM system_config WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(|r| r.0))
    }

    pub async fn set_system_config(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO system_config (key, value, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = NOW()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_instance_config(&self, instance_id: &Uuid) -> Result<Option<SyncConfig>> {
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT config FROM instances WHERE id = $1",
        )
        .bind(instance_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((json,)) => Ok(Some(serde_json::from_value(json)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_status_from_str() {
        assert_eq!(InstanceStatus::from("created"), InstanceStatus::Created);
        assert_eq!(InstanceStatus::from("syncing"), InstanceStatus::Syncing);
        assert_eq!(InstanceStatus::from("rebuilding"), InstanceStatus::Rebuilding);
        assert_eq!(InstanceStatus::from("ready"), InstanceStatus::Ready);
        assert_eq!(InstanceStatus::from("active"), InstanceStatus::Active);
        assert_eq!(InstanceStatus::from("failed"), InstanceStatus::Failed);
        assert_eq!(InstanceStatus::from("archived"), InstanceStatus::Archived);
        // Unknown values default to Created
        assert_eq!(InstanceStatus::from("unknown"), InstanceStatus::Created);
        assert_eq!(InstanceStatus::from(""), InstanceStatus::Created);
    }

    #[test]
    fn test_instance_status_as_str() {
        assert_eq!(InstanceStatus::Created.as_str(), "created");
        assert_eq!(InstanceStatus::Syncing.as_str(), "syncing");
        assert_eq!(InstanceStatus::Rebuilding.as_str(), "rebuilding");
        assert_eq!(InstanceStatus::Ready.as_str(), "ready");
        assert_eq!(InstanceStatus::Active.as_str(), "active");
        assert_eq!(InstanceStatus::Failed.as_str(), "failed");
        assert_eq!(InstanceStatus::Archived.as_str(), "archived");
    }

    #[test]
    fn test_instance_status_roundtrip() {
        let statuses = [
            InstanceStatus::Created,
            InstanceStatus::Syncing,
            InstanceStatus::Rebuilding,
            InstanceStatus::Ready,
            InstanceStatus::Active,
            InstanceStatus::Failed,
            InstanceStatus::Archived,
        ];
        for status in statuses {
            let s = status.as_str();
            assert_eq!(InstanceStatus::from(s), status);
        }
    }

    #[test]
    fn test_sync_phase_from_str() {
        assert_eq!(SyncPhase::from("pending"), SyncPhase::Pending);
        assert_eq!(SyncPhase::from("core_sync"), SyncPhase::CoreSync);
        assert_eq!(SyncPhase::from("rebuild_live_cells"), SyncPhase::RebuildLiveCells);
        assert_eq!(SyncPhase::from("rebuild_balances"), SyncPhase::RebuildBalances);
        assert_eq!(SyncPhase::from("rebuild_script_usage"), SyncPhase::RebuildScriptUsage);
        assert_eq!(SyncPhase::from("rebuild_statistics"), SyncPhase::RebuildStatistics);
        assert_eq!(SyncPhase::from("rebuild_indexes"), SyncPhase::RebuildIndexes);
        assert_eq!(SyncPhase::from("rebuild_address_tx"), SyncPhase::RebuildAddressTx);
        assert_eq!(SyncPhase::from("completed"), SyncPhase::Completed);
        // Unknown values default to Pending
        assert_eq!(SyncPhase::from("unknown"), SyncPhase::Pending);
        assert_eq!(SyncPhase::from(""), SyncPhase::Pending);
    }

    #[test]
    fn test_sync_phase_as_str() {
        assert_eq!(SyncPhase::Pending.as_str(), "pending");
        assert_eq!(SyncPhase::CoreSync.as_str(), "core_sync");
        assert_eq!(SyncPhase::RebuildLiveCells.as_str(), "rebuild_live_cells");
        assert_eq!(SyncPhase::RebuildBalances.as_str(), "rebuild_balances");
        assert_eq!(SyncPhase::RebuildScriptUsage.as_str(), "rebuild_script_usage");
        assert_eq!(SyncPhase::RebuildStatistics.as_str(), "rebuild_statistics");
        assert_eq!(SyncPhase::RebuildIndexes.as_str(), "rebuild_indexes");
        assert_eq!(SyncPhase::RebuildAddressTx.as_str(), "rebuild_address_tx");
        assert_eq!(SyncPhase::Completed.as_str(), "completed");
    }

    #[test]
    fn test_sync_phase_roundtrip() {
        let phases = [
            SyncPhase::Pending,
            SyncPhase::CoreSync,
            SyncPhase::RebuildLiveCells,
            SyncPhase::RebuildBalances,
            SyncPhase::RebuildScriptUsage,
            SyncPhase::RebuildStatistics,
            SyncPhase::RebuildIndexes,
            SyncPhase::RebuildAddressTx,
            SyncPhase::Completed,
        ];
        for phase in phases {
            let s = phase.as_str();
            assert_eq!(SyncPhase::from(s), phase);
        }
    }

    #[test]
    fn test_sync_phase_next() {
        assert_eq!(SyncPhase::Pending.next(), Some(SyncPhase::CoreSync));
        assert_eq!(SyncPhase::CoreSync.next(), Some(SyncPhase::RebuildLiveCells));
        assert_eq!(SyncPhase::RebuildLiveCells.next(), Some(SyncPhase::RebuildBalances));
        assert_eq!(SyncPhase::RebuildBalances.next(), Some(SyncPhase::RebuildScriptUsage));
        assert_eq!(SyncPhase::RebuildScriptUsage.next(), Some(SyncPhase::RebuildStatistics));
        assert_eq!(SyncPhase::RebuildStatistics.next(), Some(SyncPhase::RebuildIndexes));
        assert_eq!(SyncPhase::RebuildIndexes.next(), Some(SyncPhase::RebuildAddressTx));
        assert_eq!(SyncPhase::RebuildAddressTx.next(), Some(SyncPhase::Completed));
        assert_eq!(SyncPhase::Completed.next(), None);
    }

    #[test]
    fn test_sync_phase_full_progression() {
        let mut phase = SyncPhase::Pending;
        let expected = [
            SyncPhase::CoreSync,
            SyncPhase::RebuildLiveCells,
            SyncPhase::RebuildBalances,
            SyncPhase::RebuildScriptUsage,
            SyncPhase::RebuildStatistics,
            SyncPhase::RebuildIndexes,
            SyncPhase::RebuildAddressTx,
            SyncPhase::Completed,
        ];
        
        for expected_phase in expected {
            phase = phase.next().expect("should have next phase");
            assert_eq!(phase, expected_phase);
        }
        assert!(phase.next().is_none(), "Completed should have no next phase");
    }

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.batch_size, 3000);
        assert_eq!(config.parallel_fetch_size, 64);
        assert_eq!(config.pipeline_buffer, 4);
        assert!(config.bulk_sync_mode);
        assert!(config.skip_live_cells);
        assert!(config.skip_address_transactions);
        assert!(config.skip_statistics);
    }

    #[test]
    fn test_instance_status_default() {
        let status = InstanceStatus::default();
        assert_eq!(status, InstanceStatus::Created);
    }

    #[test]
    fn test_sync_phase_default() {
        let phase = SyncPhase::default();
        assert_eq!(phase, SyncPhase::Pending);
    }
}
