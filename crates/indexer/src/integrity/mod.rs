use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

const BATCH_SIZE: i64 = 50;
const CONCURRENT_CALCULATIONS: usize = 4;
const MAX_RECENT_FIXES: i64 = 100;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UdtLabelInfo {
    pub name: Option<String>,
    pub symbol: String,
    pub icon: Option<String>,
    pub decimal: i16,
    pub tags: Option<Vec<String>>,
    pub manager: Option<String>,
    #[serde(rename = "type")]
    pub type_script: UdtTypeScript,
    pub type_hash: String,
    pub description: Option<String>,
    pub udt_type: String,
    pub published: bool,
    pub email: Option<String>,
    pub famous: bool,
    pub operator_website: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UdtTypeScript {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLabelInfo {
    pub name: String,
    pub description: String,
    pub rfc: String,
    pub website: String,
    pub source_url: String,
    pub decoder_type: Option<String>,
    pub deployments: ScriptDeployments,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptDeployments {
    pub mainnet: Vec<ScriptDeployment>,
    pub testnet: Vec<ScriptDeployment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDeployment {
    pub tag: Option<String>,
    pub deprecated: bool,
    pub hash_type: String,
    pub data_hash: String,
    pub type_hash: String,
    pub code_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScriptNameOverrides {
    #[serde(default)]
    pub overrides: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub deprecated: Vec<String>,
}

pub struct DataIntegrityService {
    pool: PgPool,
    ckb_rpc_url: String,
    token_labels_path: Option<String>,
    trigger_rx: mpsc::Receiver<IntegrityCheck>,
    pending_count: Arc<AtomicU64>,
    total_count: Arc<AtomicU64>,
    processed_count: Arc<AtomicU64>,
    udt_info_running: Arc<AtomicBool>,
    udt_info_total: Arc<AtomicU64>,
    udt_info_processed: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub enum IntegrityCheck {
    CyclesForBlockRange { start: i64, end: i64 },
    AllMissingCycles,
    UdtInfoUpdate,
    ScriptInfoUpdate,
    AllLabelsUpdate,
}

/// Timeout in seconds to consider the integrity service as not running
const HEARTBEAT_TIMEOUT_SECS: i64 = 30;

pub struct IntegrityServiceHandle {
    trigger_tx: mpsc::Sender<IntegrityCheck>,
    pool: PgPool,
    udt_info_running: Arc<AtomicBool>,
    udt_info_total: Arc<AtomicU64>,
    udt_info_processed: Arc<AtomicU64>,
}

impl IntegrityServiceHandle {
    pub async fn trigger(&self, check: IntegrityCheck) {
        if let Err(e) = self.trigger_tx.send(check).await {
            warn!("Failed to trigger integrity check: {}", e);
        }
    }

    pub async fn is_running(&self) -> bool {
        let result: Option<(Option<chrono::DateTime<chrono::Utc>>,)> =
            sqlx::query_as("SELECT integrity_heartbeat FROM sync_status WHERE id = 1")
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();

        match result {
            Some((Some(heartbeat),)) => {
                let elapsed = chrono::Utc::now()
                    .signed_duration_since(heartbeat)
                    .num_seconds();
                elapsed < HEARTBEAT_TIMEOUT_SECS
            }
            _ => false,
        }
    }

    pub fn udt_info_running(&self) -> bool {
        self.udt_info_running.load(Ordering::Relaxed)
    }

    pub fn udt_info_total(&self) -> u64 {
        self.udt_info_total.load(Ordering::Relaxed)
    }

    pub fn udt_info_processed(&self) -> u64 {
        self.udt_info_processed.load(Ordering::Relaxed)
    }
}

impl DataIntegrityService {
    pub fn new(
        pool: PgPool,
        ckb_rpc_url: String,
        token_labels_path: Option<String>,
    ) -> (Self, IntegrityServiceHandle) {
        let (trigger_tx, trigger_rx) = mpsc::channel(100);
        let pending_count = Arc::new(AtomicU64::new(0));
        let total_count = Arc::new(AtomicU64::new(0));
        let processed_count = Arc::new(AtomicU64::new(0));
        let udt_info_running = Arc::new(AtomicBool::new(false));
        let udt_info_total = Arc::new(AtomicU64::new(0));
        let udt_info_processed = Arc::new(AtomicU64::new(0));

        let service = Self {
            pool: pool.clone(),
            ckb_rpc_url,
            token_labels_path,
            trigger_rx,
            pending_count: Arc::clone(&pending_count),
            total_count: Arc::clone(&total_count),
            processed_count: Arc::clone(&processed_count),
            udt_info_running: Arc::clone(&udt_info_running),
            udt_info_total: Arc::clone(&udt_info_total),
            udt_info_processed: Arc::clone(&udt_info_processed),
        };

        let handle = IntegrityServiceHandle {
            trigger_tx,
            pool,
            udt_info_running,
            udt_info_total,
            udt_info_processed,
        };

        (service, handle)
    }

    pub async fn run(mut self) {
        info!("Data integrity service started, waiting for sync to catch up before running checks");

        self.update_script_labels().await;
        self.update_udt_info().await;

        while let Some(check) = self.trigger_rx.recv().await {
            match check {
                IntegrityCheck::CyclesForBlockRange { start, end } => {
                    self.fix_cycles_for_range(start, end).await;
                    self.clear_heartbeat().await;
                }
                IntegrityCheck::AllMissingCycles => {
                    self.fix_all_missing_cycles().await;
                    self.clear_heartbeat().await;
                }
                IntegrityCheck::UdtInfoUpdate => {
                    self.update_udt_info().await;
                }
                IntegrityCheck::ScriptInfoUpdate => {
                    self.update_script_labels().await;
                }
                IntegrityCheck::AllLabelsUpdate => {
                    self.update_script_labels().await;
                    self.update_udt_info().await;
                }
            }
        }
    }

    async fn update_heartbeat(&self) {
        let pending = self.pending_count.load(Ordering::Relaxed) as i64;
        let total = self.total_count.load(Ordering::Relaxed) as i64;
        let processed = self.processed_count.load(Ordering::Relaxed) as i64;
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET 
                integrity_heartbeat = NOW(),
                integrity_pending_count = $1,
                integrity_total_count = $2,
                integrity_processed_count = $3
            WHERE id = 1
            "#,
        )
        .bind(pending)
        .bind(total)
        .bind(processed)
        .execute(&self.pool)
        .await;
    }

    async fn clear_heartbeat(&self) {
        let _ = sqlx::query("UPDATE sync_status SET integrity_heartbeat = NULL WHERE id = 1")
            .execute(&self.pool)
            .await;
    }

    async fn start_new_run(&self, total: i64) {
        self.total_count.store(total as u64, Ordering::Relaxed);
        self.processed_count.store(0, Ordering::Relaxed);
        self.pending_count.store(total as u64, Ordering::Relaxed);
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET 
                integrity_heartbeat = NOW(),
                integrity_total_count = $1,
                integrity_processed_count = 0,
                integrity_pending_count = $1,
                integrity_started_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(total)
        .execute(&self.pool)
        .await;
    }

    async fn fix_cycles_for_range(&self, start: i64, end: i64) {
        info!("Checking cycles for block range {} to {}", start, end);

        let total_in_range: (i64,) = match sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM transactions 
            WHERE block_number BETWEEN $1 AND $2 
              AND NOT is_cellbase 
              AND (cycles IS NULL OR cycles = 0)
            "#,
        )
        .bind(start)
        .bind(end)
        .fetch_one(&self.pool)
        .await
        {
            Ok(count) => count,
            Err(e) => {
                warn!("Failed to count transactions for cycles fix: {}", e);
                return;
            }
        };

        if total_in_range.0 == 0 {
            info!("No missing cycles found in range {} to {}", start, end);
            return;
        }

        info!(
            "Starting to fix {} transactions with missing cycles in range {} to {}",
            total_in_range.0, start, end
        );
        self.start_new_run(total_in_range.0).await;

        loop {
            let txs: Vec<(Vec<u8>,)> = match sqlx::query_as(
                r#"
                SELECT hash FROM transactions 
                WHERE block_number BETWEEN $1 AND $2 
                  AND NOT is_cellbase 
                  AND (cycles IS NULL OR cycles = 0)
                ORDER BY block_number
                LIMIT $3
                "#,
            )
            .bind(start)
            .bind(end)
            .bind(BATCH_SIZE)
            .fetch_all(&self.pool)
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    warn!("Failed to query transactions for cycles fix: {}", e);
                    break;
                }
            };

            if txs.is_empty() {
                break;
            }

            let tx_hashes: Vec<String> = txs
                .iter()
                .map(|(h,)| format!("0x{}", hex::encode(h)))
                .collect();

            self.calculate_and_update_batch(&tx_hashes).await;
            self.update_heartbeat().await;

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        self.pending_count.store(0, Ordering::Relaxed);
    }

    async fn fix_all_missing_cycles(&self) {
        let total_missing: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transactions WHERE NOT is_cellbase AND (cycles IS NULL OR cycles = 0)",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0,));

        if total_missing.0 == 0 {
            info!("No missing cycles to fix");
            return;
        }

        info!(
            "Starting to fix {} transactions with missing cycles",
            total_missing.0
        );
        self.start_new_run(total_missing.0).await;

        loop {
            let txs: Vec<(Vec<u8>,)> = match sqlx::query_as(
                r#"
                SELECT hash FROM transactions 
                WHERE NOT is_cellbase 
                  AND (cycles IS NULL OR cycles = 0)
                ORDER BY block_number
                LIMIT $1
                "#,
            )
            .bind(BATCH_SIZE)
            .fetch_all(&self.pool)
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    warn!("Failed to query transactions for cycles fix: {}", e);
                    break;
                }
            };

            if txs.is_empty() {
                break;
            }

            let tx_hashes: Vec<String> = txs
                .iter()
                .map(|(h,)| format!("0x{}", hex::encode(h)))
                .collect();

            self.calculate_and_update_batch(&tx_hashes).await;
            self.update_heartbeat().await;

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        self.pending_count.store(0, Ordering::Relaxed);
    }

    async fn calculate_and_update_batch(&self, tx_hashes: &[String]) {
        use futures::stream::{FuturesUnordered, StreamExt};

        let mut futures = FuturesUnordered::new();

        for tx_hash in tx_hashes {
            let rpc_url = self.ckb_rpc_url.clone();
            let hash = tx_hash.clone();
            futures.push(async move {
                let result = ckbadger_common::cycles::calculate_cycles(&rpc_url, &hash).await;
                (hash, result)
            });

            if futures.len() >= CONCURRENT_CALCULATIONS {
                if let Some((hash, result)) = futures.next().await {
                    self.update_cycles(&hash, result).await;
                }
            }
        }

        while let Some((hash, result)) = futures.next().await {
            self.update_cycles(&hash, result).await;
        }
    }

    async fn update_cycles(&self, tx_hash: &str, result: Result<i64, String>) {
        match result {
            Ok(cycles) => {
                let hash_bytes =
                    hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)).unwrap_or_default();

                if let Err(e) = sqlx::query("UPDATE transactions SET cycles = $1 WHERE hash = $2")
                    .bind(cycles)
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await
                {
                    warn!("Failed to update cycles for {}: {}", tx_hash, e);
                } else {
                    debug!("Updated cycles for {}: {}", tx_hash, cycles);
                    self.record_recent_fix(&hash_bytes, cycles).await;
                    self.processed_count.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                warn!("Failed to calculate cycles for {}: {}", tx_hash, e);
                let hash_bytes =
                    hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)).unwrap_or_default();
                let _ = sqlx::query("UPDATE transactions SET cycles = -1 WHERE hash = $1")
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await;
                self.processed_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Use saturating_sub to prevent underflow when pending_count is already 0
        let current = self.pending_count.load(Ordering::Relaxed);
        if current > 0 {
            self.pending_count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    async fn record_recent_fix(&self, tx_hash: &[u8], cycles: i64) {
        let _ = sqlx::query("INSERT INTO integrity_recent_fixes (tx_hash, cycles) VALUES ($1, $2)")
            .bind(tx_hash)
            .bind(cycles)
            .execute(&self.pool)
            .await;

        let _ = sqlx::query(
            r#"
            DELETE FROM integrity_recent_fixes 
            WHERE id NOT IN (
                SELECT id FROM integrity_recent_fixes 
                ORDER BY fixed_at DESC 
                LIMIT $1
            )
            "#,
        )
        .bind(MAX_RECENT_FIXES)
        .execute(&self.pool)
        .await;
    }

    async fn update_udt_info(&self) {
        let labels_path = match &self.token_labels_path {
            Some(p) => p.clone(),
            None => {
                info!("Token labels path not configured, skipping UDT info update");
                return;
            }
        };

        info!("Starting UDT info update from {}", labels_path);
        self.udt_info_running.store(true, Ordering::Relaxed);

        let labels = match self.load_token_labels(&labels_path).await {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to load token labels: {}", e);
                self.udt_info_running.store(false, Ordering::Relaxed);
                self.update_udt_info_status_in_db(false).await;
                return;
            }
        };

        let total = labels.len() as u64;
        self.udt_info_total.store(total, Ordering::Relaxed);
        self.udt_info_processed.store(0, Ordering::Relaxed);
        self.start_udt_info_run(total as i64).await;

        info!("Found {} token labels to import", total);

        for label in labels {
            if let Err(e) = self.upsert_token_label(&label).await {
                warn!(
                    "Failed to upsert token label for {}: {}",
                    label.type_hash, e
                );
            }
            self.udt_info_processed.fetch_add(1, Ordering::Relaxed);
        }

        info!("UDT info update completed");
        self.udt_info_running.store(false, Ordering::Relaxed);
        self.update_udt_info_status_in_db(false).await;
    }

    async fn load_token_labels(&self, base_path: &str) -> Result<Vec<UdtLabelInfo>, String> {
        let mut labels = Vec::new();

        for network in &["mainnet", "testnet"] {
            let network_path = Path::new(base_path)
                .join("information")
                .join("udt")
                .join(network);

            if !network_path.exists() {
                continue;
            }

            let entries = match std::fs::read_dir(&network_path) {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read directory {:?}: {}", network_path, e);
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let index_path = path.join("index.json");
                if !index_path.exists() {
                    continue;
                }

                match std::fs::read_to_string(&index_path) {
                    Ok(content) => match serde_json::from_str::<UdtLabelInfo>(&content) {
                        Ok(label) => {
                            if label.published {
                                labels.push(label);
                            }
                        }
                        Err(e) => {
                            debug!("Failed to parse {:?}: {}", index_path, e);
                        }
                    },
                    Err(e) => {
                        debug!("Failed to read {:?}: {}", index_path, e);
                    }
                }
            }
        }

        Ok(labels)
    }

    async fn upsert_token_label(&self, label: &UdtLabelInfo) -> Result<(), String> {
        let type_hash = hex::decode(
            label
                .type_hash
                .strip_prefix("0x")
                .unwrap_or(&label.type_hash),
        )
        .map_err(|e| format!("Invalid type hash: {}", e))?;

        let tags: Option<Vec<String>> = label.tags.clone();

        let result = sqlx::query(
            r#"
            UPDATE tokens SET
                name = COALESCE($2, name),
                symbol = COALESCE($3, symbol),
                decimals = $4,
                description = COALESCE($5, description),
                icon_url = COALESCE($6, icon_url),
                published = $7,
                famous = $8,
                tags = $9,
                udt_type = $10,
                manager = $11,
                email = $12,
                operator_website = $13,
                label_updated_at = NOW(),
                updated_at = NOW()
            WHERE type_script_hash = $1
            "#,
        )
        .bind(&type_hash)
        .bind(&label.name)
        .bind(&label.symbol)
        .bind(label.decimal)
        .bind(&label.description)
        .bind(&label.icon)
        .bind(label.published)
        .bind(label.famous)
        .bind(&tags)
        .bind(&label.udt_type)
        .bind(&label.manager)
        .bind(&label.email)
        .bind(&label.operator_website)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        if result.rows_affected() > 0 {
            debug!(
                "Updated token label for {} ({})",
                label.type_hash, label.symbol
            );
        }

        Ok(())
    }

    async fn start_udt_info_run(&self, total: i64) {
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET
                udt_info_running = true,
                udt_info_total_count = $1,
                udt_info_processed_count = 0,
                udt_info_started_at = NOW(),
                udt_info_last_check_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(total)
        .execute(&self.pool)
        .await;
    }

    async fn update_udt_info_status_in_db(&self, running: bool) {
        let total = self.udt_info_total.load(Ordering::Relaxed) as i64;
        let processed = self.udt_info_processed.load(Ordering::Relaxed) as i64;
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET
                udt_info_running = $1,
                udt_info_total_count = $2,
                udt_info_processed_count = $3,
                udt_info_last_check_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(running)
        .bind(total)
        .bind(processed)
        .execute(&self.pool)
        .await;
    }

    async fn update_script_labels(&self) {
        let labels_path = match &self.token_labels_path {
            Some(p) => p.clone(),
            None => {
                info!("Token labels path not configured, skipping script labels update");
                return;
            }
        };

        info!("Starting script labels update from {}", labels_path);

        let scripts = match self.load_script_labels(&labels_path).await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to load script labels: {}", e);
                self.update_script_info_status_in_db(false).await;
                return;
            }
        };

        let total = scripts.len() as i64;
        info!("Found {} script labels to import", total);
        self.start_script_info_run(total).await;

        let mut imported = 0;
        for script in scripts {
            if let Err(e) = self.upsert_script_label(&script).await {
                warn!("Failed to upsert script label for {}: {}", script.name, e);
            } else {
                imported += 1;
            }
        }

        info!("Script labels update completed: {} imported", imported);

        self.finish_script_info_run(imported).await;
    }

    async fn start_script_info_run(&self, total: i64) {
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET
                script_info_running = true,
                script_info_total_count = $1,
                script_info_processed_count = 0,
                script_info_started_at = NOW(),
                script_info_last_check_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(total)
        .execute(&self.pool)
        .await;
    }

    async fn finish_script_info_run(&self, processed: i64) {
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET
                script_info_running = false,
                script_info_processed_count = $1,
                script_info_last_check_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(processed)
        .execute(&self.pool)
        .await;
    }

    async fn update_script_info_status_in_db(&self, running: bool) {
        let _ = sqlx::query(
            r#"
            UPDATE sync_status SET
                script_info_running = $1,
                script_info_last_check_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(running)
        .execute(&self.pool)
        .await;
    }

    fn load_overrides(&self, base_path: &str) -> ScriptNameOverrides {
        load_script_overrides(base_path)
    }

    async fn load_script_labels(&self, base_path: &str) -> Result<Vec<ScriptLabelInfo>, String> {
        let mut scripts = Vec::new();
        let overrides = self.load_overrides(base_path);

        let script_path = Path::new(base_path).join("information").join("script");

        if !script_path.exists() {
            return Err(format!("Script path {:?} does not exist", script_path));
        }

        let entries = std::fs::read_dir(&script_path).map_err(|e| e.to_string())?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let index_path = path.join("index.json");
            if !index_path.exists() {
                continue;
            }

            match std::fs::read_to_string(&index_path) {
                Ok(content) => match serde_json::from_str::<ScriptLabelInfo>(&content) {
                    Ok(mut script) => {
                        if let Some(new_name) = overrides.overrides.get(&script.name) {
                            debug!("Overriding script name: {} -> {}", script.name, new_name);
                            script.name = new_name.clone();
                        }
                        for deployment in script.deployments.mainnet.iter_mut() {
                            let code_hash_lower = deployment.code_hash.to_lowercase();
                            if overrides
                                .deprecated
                                .iter()
                                .any(|d| d.to_lowercase() == code_hash_lower)
                            {
                                deployment.deprecated = true;
                            }
                        }
                        for deployment in script.deployments.testnet.iter_mut() {
                            let code_hash_lower = deployment.code_hash.to_lowercase();
                            if overrides
                                .deprecated
                                .iter()
                                .any(|d| d.to_lowercase() == code_hash_lower)
                            {
                                deployment.deprecated = true;
                            }
                        }
                        scripts.push(script);
                    }
                    Err(e) => {
                        debug!("Failed to parse {:?}: {}", index_path, e);
                    }
                },
                Err(e) => {
                    debug!("Failed to read {:?}: {}", index_path, e);
                }
            }
        }

        Ok(scripts)
    }

    async fn upsert_script_label(&self, script: &ScriptLabelInfo) -> Result<(), String> {
        for (network, deployments) in [
            ("mainnet", &script.deployments.mainnet),
            ("testnet", &script.deployments.testnet),
        ] {
            for deployment in deployments {
                let code_hash = hex::decode(
                    deployment
                        .code_hash
                        .strip_prefix("0x")
                        .unwrap_or(&deployment.code_hash),
                )
                .map_err(|e| format!("Invalid code_hash: {}", e))?;

                let data_hash = if deployment.data_hash.is_empty() {
                    None
                } else {
                    Some(
                        hex::decode(
                            deployment
                                .data_hash
                                .strip_prefix("0x")
                                .unwrap_or(&deployment.data_hash),
                        )
                        .map_err(|e| format!("Invalid data_hash: {}", e))?,
                    )
                };

                let type_hash = if deployment.type_hash.is_empty() {
                    None
                } else {
                    Some(
                        hex::decode(
                            deployment
                                .type_hash
                                .strip_prefix("0x")
                                .unwrap_or(&deployment.type_hash),
                        )
                        .map_err(|e| format!("Invalid type_hash: {}", e))?,
                    )
                };

                // Use empty string for empty tags to ensure UNIQUE constraint works correctly
                // (PostgreSQL treats multiple NULLs as distinct in UNIQUE constraints)
                let tag = deployment.tag.clone().unwrap_or_default();

                let rfc = if script.rfc.is_empty() {
                    None
                } else {
                    Some(&script.rfc)
                };

                let website = if script.website.is_empty() {
                    None
                } else {
                    Some(&script.website)
                };

                let source_url = if script.source_url.is_empty() {
                    None
                } else {
                    Some(&script.source_url)
                };

                let description = if script.description.is_empty() {
                    None
                } else {
                    Some(&script.description)
                };

                sqlx::query(
                    r#"
                    INSERT INTO known_scripts (
                        code_hash, name, description, rfc, website, source_url, decoder_type,
                        network, hash_type, data_hash, type_hash, tag, deprecated, 
                        is_system, label_source, label_updated_at
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'token-labels', NOW())
                    ON CONFLICT (code_hash, network, tag) DO UPDATE SET
                        name = EXCLUDED.name,
                        description = COALESCE(EXCLUDED.description, known_scripts.description),
                        rfc = COALESCE(EXCLUDED.rfc, known_scripts.rfc),
                        website = COALESCE(EXCLUDED.website, known_scripts.website),
                        source_url = COALESCE(EXCLUDED.source_url, known_scripts.source_url),
                        decoder_type = COALESCE(EXCLUDED.decoder_type, known_scripts.decoder_type),
                        hash_type = EXCLUDED.hash_type,
                        data_hash = EXCLUDED.data_hash,
                        type_hash = EXCLUDED.type_hash,
                        deprecated = EXCLUDED.deprecated,
                        label_updated_at = NOW()
                    "#,
                )
                .bind(&code_hash)
                .bind(&script.name)
                .bind(description)
                .bind(rfc)
                .bind(website)
                .bind(source_url)
                .bind(&script.decoder_type)
                .bind(network)
                .bind(&deployment.hash_type)
                .bind(&data_hash)
                .bind(&type_hash)
                .bind(&tag)
                .bind(deployment.deprecated)
                .bind(false)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;

                debug!(
                    "Upserted script label: {} ({}) for {}",
                    script.name,
                    if tag.is_empty() { "default" } else { &tag },
                    network
                );
            }
        }

        Ok(())
    }
}

fn load_script_overrides(base_path: &str) -> ScriptNameOverrides {
    let overrides_path = Path::new(base_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join("script-name-overrides.json");

    debug!("Looking for script overrides at: {:?}", overrides_path);

    if !overrides_path.exists() {
        info!("Script overrides file not found at {:?}", overrides_path);
        return ScriptNameOverrides::default();
    }

    match std::fs::read_to_string(&overrides_path) {
        Ok(content) => match serde_json::from_str::<ScriptNameOverrides>(&content) {
            Ok(data) => {
                info!(
                    "Loaded {} script name overrides and {} deprecated scripts",
                    data.overrides.len(),
                    data.deprecated.len()
                );
                data
            }
            Err(e) => {
                warn!("Failed to parse script overrides: {}", e);
                ScriptNameOverrides::default()
            }
        },
        Err(e) => {
            debug!("No script overrides file found: {}", e);
            ScriptNameOverrides::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_load_script_overrides_valid_file() {
        let temp_dir = TempDir::new().unwrap();
        let overrides_path = temp_dir.path().join("script-name-overrides.json");

        let content = r#"{
            "overrides": {
                "DAS Lock": ".bit Lock",
                "Web5 DID": "did:ckb"
            },
            "deprecated": ["0xabc123"]
        }"#;

        let mut file = std::fs::File::create(&overrides_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert_eq!(data.overrides.len(), 2);
        assert_eq!(
            data.overrides.get("DAS Lock"),
            Some(&".bit Lock".to_string())
        );
        assert_eq!(data.overrides.get("Web5 DID"), Some(&"did:ckb".to_string()));
        assert_eq!(data.deprecated.len(), 1);
        assert_eq!(data.deprecated[0], "0xabc123");
    }

    #[test]
    fn test_load_script_overrides_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert!(data.overrides.is_empty());
        assert!(data.deprecated.is_empty());
    }

    #[test]
    fn test_load_script_overrides_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let overrides_path = temp_dir.path().join("script-name-overrides.json");

        let mut file = std::fs::File::create(&overrides_path).unwrap();
        file.write_all(b"invalid json").unwrap();

        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert!(data.overrides.is_empty());
        assert!(data.deprecated.is_empty());
    }

    #[test]
    fn test_load_script_overrides_empty() {
        let temp_dir = TempDir::new().unwrap();
        let overrides_path = temp_dir.path().join("script-name-overrides.json");

        let content = r#"{"overrides": {}}"#;

        let mut file = std::fs::File::create(&overrides_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert!(data.overrides.is_empty());
        assert!(data.deprecated.is_empty());
    }

    #[test]
    fn test_script_overrides_from_actual_docs() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let project_root = Path::new(manifest_dir).parent().unwrap().parent().unwrap();
        let base_path = project_root.join("docs/token-labels");

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert_eq!(
            data.overrides.get("DAS Lock"),
            Some(&".bit Lock".to_string())
        );
        assert_eq!(
            data.overrides.get("DID Account"),
            Some(&".bit Account".to_string())
        );
        assert_eq!(
            data.overrides.get("DID Cell"),
            Some(&".bit Cell".to_string())
        );
        assert_eq!(data.overrides.get("Web5 DID"), Some(&"did:ckb".to_string()));
        assert!(data.deprecated.len() >= 5);
    }

    #[test]
    fn test_deprecated_code_hash_applied_to_deployment() {
        let overrides = ScriptNameOverrides {
            overrides: std::collections::HashMap::new(),
            deprecated: vec!["0xABC123".to_string(), "0xdef456".to_string()],
        };

        let mut deployment = ScriptDeployment {
            code_hash: "0xabc123".to_string(),
            hash_type: "type".to_string(),
            data_hash: String::new(),
            type_hash: String::new(),
            deprecated: false,
            tag: None,
        };

        let code_hash_lower = deployment.code_hash.to_lowercase();
        if overrides
            .deprecated
            .iter()
            .any(|d| d.to_lowercase() == code_hash_lower)
        {
            deployment.deprecated = true;
        }

        assert!(deployment.deprecated);
    }

    #[test]
    fn test_deprecated_code_hash_case_insensitive() {
        let overrides = ScriptNameOverrides {
            overrides: std::collections::HashMap::new(),
            deprecated: vec!["0xABCDEF".to_string()],
        };

        for test_hash in &["0xabcdef", "0xABCDEF", "0xAbCdEf"] {
            let code_hash_lower = test_hash.to_lowercase();
            let matches = overrides
                .deprecated
                .iter()
                .any(|d| d.to_lowercase() == code_hash_lower);
            assert!(matches, "Should match {}", test_hash);
        }
    }

    #[test]
    fn test_non_deprecated_code_hash_unchanged() {
        let overrides = ScriptNameOverrides {
            overrides: std::collections::HashMap::new(),
            deprecated: vec!["0xabc123".to_string()],
        };

        let mut deployment = ScriptDeployment {
            code_hash: "0x999999".to_string(),
            hash_type: "type".to_string(),
            data_hash: String::new(),
            type_hash: String::new(),
            deprecated: false,
            tag: None,
        };

        let code_hash_lower = deployment.code_hash.to_lowercase();
        if overrides
            .deprecated
            .iter()
            .any(|d| d.to_lowercase() == code_hash_lower)
        {
            deployment.deprecated = true;
        }

        assert!(!deployment.deprecated);
    }
}
