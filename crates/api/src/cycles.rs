use serde::Serialize;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CyclesStatus {
    Done,
    Calculating,
    Queued,
    Failed,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CyclesStatusResponse {
    pub status: CyclesStatus,
    pub cycles: Option<i64>,
    pub error: Option<String>,
}

pub struct CyclesCalculator {
    pending: RwLock<HashSet<String>>,
    calculating: RwLock<Option<String>>,
    failed: RwLock<HashMap<String, String>>,
    request_tx: mpsc::Sender<String>,
}

impl CyclesCalculator {
    pub fn new(pool: PgPool, ckb_rpc_url: String) -> Arc<Self> {
        let (request_tx, request_rx) = mpsc::channel::<String>(1000);

        let calculator = Arc::new(Self {
            pending: RwLock::new(HashSet::new()),
            calculating: RwLock::new(None),
            failed: RwLock::new(HashMap::new()),
            request_tx,
        });

        let worker = CyclesWorker {
            calculator: Arc::clone(&calculator),
            pool,
            ckb_rpc_url,
            request_rx: Mutex::new(request_rx),
        };
        tokio::spawn(worker.run());

        calculator
    }

    pub async fn request_calculation(&self, tx_hash: &str) -> CyclesStatus {
        let normalized_hash = normalize_hash(tx_hash);

        {
            let calculating = self.calculating.read().await;
            if calculating.as_ref() == Some(&normalized_hash) {
                return CyclesStatus::Calculating;
            }
        }

        {
            let pending = self.pending.read().await;
            if pending.contains(&normalized_hash) {
                return CyclesStatus::Queued;
            }
        }

        {
            let mut failed = self.failed.write().await;
            failed.remove(&normalized_hash);
        }

        {
            let mut pending = self.pending.write().await;
            pending.insert(normalized_hash.clone());
        }

        if let Err(e) = self.request_tx.try_send(normalized_hash) {
            warn!("Failed to queue cycles calculation: {}", e);
            return CyclesStatus::Failed;
        }

        CyclesStatus::Queued
    }

    pub async fn get_status(&self, tx_hash: &str) -> CyclesStatus {
        let normalized_hash = normalize_hash(tx_hash);

        {
            let calculating = self.calculating.read().await;
            if calculating.as_ref() == Some(&normalized_hash) {
                return CyclesStatus::Calculating;
            }
        }

        {
            let pending = self.pending.read().await;
            if pending.contains(&normalized_hash) {
                return CyclesStatus::Queued;
            }
        }

        CyclesStatus::Done
    }

    pub async fn get_error(&self, tx_hash: &str) -> Option<String> {
        let normalized_hash = normalize_hash(tx_hash);
        let failed = self.failed.read().await;
        failed.get(&normalized_hash).cloned()
    }

    async fn mark_calculating(&self, tx_hash: &str) {
        let mut calculating = self.calculating.write().await;
        *calculating = Some(tx_hash.to_string());
    }

    async fn mark_complete(&self, tx_hash: &str) {
        {
            let mut pending = self.pending.write().await;
            pending.remove(tx_hash);
        }
        {
            let mut calculating = self.calculating.write().await;
            *calculating = None;
        }
    }

    async fn mark_failed(&self, tx_hash: &str, error: String) {
        {
            let mut pending = self.pending.write().await;
            pending.remove(tx_hash);
        }
        {
            let mut calculating = self.calculating.write().await;
            *calculating = None;
        }
        {
            let mut failed = self.failed.write().await;
            failed.insert(tx_hash.to_string(), error);
        }
    }
}

struct CyclesWorker {
    calculator: Arc<CyclesCalculator>,
    pool: PgPool,
    ckb_rpc_url: String,
    request_rx: Mutex<mpsc::Receiver<String>>,
}

impl CyclesWorker {
    async fn run(self) {
        info!("Cycles calculation worker started");

        let mut rx = self.request_rx.lock().await;
        while let Some(tx_hash) = rx.recv().await {
            self.process_request(&tx_hash).await;
        }

        info!("Cycles calculation worker stopped");
    }

    async fn process_request(&self, tx_hash: &str) {
        debug!("Processing cycles calculation for {}", tx_hash);

        let hash_bytes = match hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)) {
            Ok(b) => b,
            Err(e) => {
                self.calculator
                    .mark_failed(tx_hash, format!("Invalid hash: {}", e))
                    .await;
                return;
            }
        };

        let current_cycles: Option<(Option<i64>,)> =
            sqlx::query_as("SELECT cycles FROM transactions_index WHERE hash = $1")
                .bind(&hash_bytes)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();

        match current_cycles {
            Some((Some(cycles),)) if cycles > 0 => {
                debug!("Transaction {} already has cycles: {}", tx_hash, cycles);
                self.calculator.mark_complete(tx_hash).await;
                return;
            }
            Some((Some(-1),)) => {
                self.calculator
                    .mark_failed(tx_hash, "Calculation previously failed".to_string())
                    .await;
                return;
            }
            None => {
                self.calculator
                    .mark_failed(tx_hash, "Transaction not found".to_string())
                    .await;
                return;
            }
            _ => {}
        }

        let is_cellbase: Option<(bool,)> =
            sqlx::query_as("SELECT is_cellbase FROM transactions_index WHERE hash = $1")
                .bind(&hash_bytes)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();

        if let Some((true,)) = is_cellbase {
            self.calculator.mark_complete(tx_hash).await;
            return;
        }

        self.calculator.mark_calculating(tx_hash).await;

        let formatted_hash = if tx_hash.starts_with("0x") {
            tx_hash.to_string()
        } else {
            format!("0x{}", tx_hash)
        };

        match ckbadger_common::cycles::calculate_cycles(&self.ckb_rpc_url, &formatted_hash).await {
            Ok(cycles) => {
                if let Err(e) = sqlx::query("UPDATE transactions SET cycles = $1 WHERE hash = $2")
                    .bind(cycles)
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await
                {
                    warn!("Failed to update cycles in DB for {}: {}", tx_hash, e);
                }
                let _ = sqlx::query("UPDATE transactions_index SET cycles = $1 WHERE hash = $2")
                    .bind(cycles)
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await;
                debug!("Calculated cycles for {}: {}", tx_hash, cycles);
                self.calculator.mark_complete(tx_hash).await;
            }
            Err(e) => {
                warn!("Failed to calculate cycles for {}: {}", tx_hash, e);
                let _ = sqlx::query("UPDATE transactions SET cycles = -1 WHERE hash = $1")
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await;
                let _ = sqlx::query("UPDATE transactions_index SET cycles = -1 WHERE hash = $1")
                    .bind(&hash_bytes)
                    .execute(&self.pool)
                    .await;
                self.calculator.mark_failed(tx_hash, e).await;
            }
        }
    }
}

fn normalize_hash(hash: &str) -> String {
    let h = hash.strip_prefix("0x").unwrap_or(hash);
    format!("0x{}", h.to_lowercase())
}
