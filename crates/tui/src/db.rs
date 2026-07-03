use anyhow::Result;
use ckbadger_common::{
    format_duration_smart, BackgroundTaskEntry, BackgroundTasksData, BulkBuildProgressData,
    MemoryStatsData, PipelineProgressData, SyncProgressData, SyncStatusData,
};
use ckbadger_store::{
    secondary_store_path, CkbadgerStore, MemoryProfile, SecondaryStoreOwner, StoreRuntimeConfig,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiNetworkStats {
    pub latest_block: i64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub transactions_per_day: String,
    #[serde(default)]
    pub api_background_tasks: Option<Vec<BackgroundTaskEntry>>,
}

fn parse_epoch_string(epoch: &str) -> (i64, i32, i32) {
    let parts: Vec<&str> = epoch.splitn(2, '(').collect();
    let epoch_number = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Some(inner) = parts.get(1).and_then(|s| s.strip_suffix(')')) {
        let idx_parts: Vec<&str> = inner.splitn(2, '/').collect();
        let epoch_index = idx_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let epoch_length = idx_parts
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1800);
        (epoch_number, epoch_index, epoch_length)
    } else {
        (epoch_number, 0, 1800)
    }
}

#[derive(Debug, Clone)]
pub struct SyncStatusRow {
    pub tip_block: i64,
    pub chain_tip: i64,
    pub is_syncing: bool,
    pub is_bulk_sync: bool,
    pub progress: f64,
    pub elapsed_time: Option<String>,
    pub eta: Option<String>,
    pub eta_seconds: Option<f64>,
    pub rate_realtime: Option<f64>,
    pub rate_ema: Option<f64>,
    pub tx_rate_realtime: Option<f64>,
    pub tx_rate_ema: Option<f64>,
    pub db_write_ms: Option<f64>,
    pub db_commit_ms: Option<f64>,
    pub rpc_fetch_ms: Option<f64>,
    pub pipeline: Option<PipelineProgressData>,
    pub pipeline_reset_epoch: Option<u64>,
    pub pipeline_reset_reason: Option<String>,
    pub last_batch_blocks: Option<u64>,
    pub startup_phase: Option<String>,
    pub is_direct_db_read: bool,
    pub bulk_build: Option<BulkBuildProgressData>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeDiagData {
    pub active_run_id: Option<String>,
    pub last_run_id: Option<String>,
    pub heartbeat_block: i64,
    pub heartbeat_target_block: i64,
    pub heartbeat_stage: Option<String>,
    pub heartbeat_age_secs: Option<i64>,
    pub heartbeat_oom_events: Option<u64>,
    pub heartbeat_oom_kill_events: Option<u64>,
    pub last_incident_summary: Option<String>,
    pub last_shutdown_reason: Option<String>,
    pub last_exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct SupervisorServiceData {
    pub name: String,
    pub pid: u32,
    pub status: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ServiceLogTailData {
    pub service: String,
    pub last_line: String,
}

#[derive(Debug, Clone)]
pub struct ChainInfoData {
    pub latest_block: i64,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub difficulty: String,
    pub hash_rate: String,
    pub avg_block_time: String,
    pub tps: String,
    pub tx_24h: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ApiServiceInfo {
    pub reachable: bool,
    pub latency_ms: Option<f64>,
    pub status_code: Option<u16>,
    pub latest_block: Option<i64>,
    pub tps: Option<String>,
    pub avg_block_time: Option<String>,
    pub error: Option<String>,
}

fn chain_info_from_api_stats(stats: &ApiNetworkStats) -> ChainInfoData {
    let tx_24h = stats.transactions_per_day.parse::<i64>().unwrap_or(0);
    let (epoch_number, epoch_index, epoch_length) = parse_epoch_string(&stats.epoch);

    ChainInfoData {
        latest_block: stats.latest_block,
        epoch_number,
        epoch_index,
        epoch_length,
        difficulty: stats.difficulty.clone(),
        hash_rate: stats.hash_rate.clone(),
        avg_block_time: stats.avg_block_time.clone(),
        tps: stats.tps.clone(),
        tx_24h,
    }
}

const LEGACY_BULK_SYNC_THRESHOLD_BLOCKS: i64 = 1000;
const SYNC_PROGRESS_STALE_SECS: i64 = 60;

fn sync_modes_from_progress(
    _progress: &SyncProgressData,
    status_data: Option<&SyncStatusData>,
    blocks_behind: i64,
) -> (bool, bool) {
    let is_syncing = blocks_behind > 0;
    let is_bulk_sync = status_data
        .and_then(|s| {
            // If bulk_sync_completed_at is None and sync is in progress, it's bulk sync
            if s.bulk_sync_completed_at.is_none() && is_syncing {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or(blocks_behind > LEGACY_BULK_SYNC_THRESHOLD_BLOCKS);
    (is_syncing, is_bulk_sync)
}

fn sync_progress_is_stale(progress: &SyncProgressData, now_ts: i64) -> bool {
    now_ts.saturating_sub(progress.updated_at) > SYNC_PROGRESS_STALE_SECS
}

pub struct TuiDb {
    store: Option<Arc<CkbadgerStore>>,
    api_url: String,
    supervisor_socket_path: Option<PathBuf>,
    service_log_dir: Option<PathBuf>,
    http: reqwest::Client,
    memory_profile: MemoryProfile,
    ckbadger_workdir: PathBuf,
    ckb_workdir: PathBuf,
    ckb_db_path: PathBuf,
    domain_data_path: PathBuf,
    append_only_data_path: PathBuf,
    store_runtime_config: StoreRuntimeConfig,
}

pub struct TuiPathConfig<'a> {
    pub domain_data_path: &'a str,
    pub append_only_data_path: &'a str,
    pub ckbadger_workdir: &'a str,
    pub ckb_workdir: &'a str,
    pub ckb_db_path: &'a str,
}

impl TuiDb {
    pub fn memory_profile(&self) -> &MemoryProfile {
        &self.memory_profile
    }

    pub fn ckbadger_workdir(&self) -> &Path {
        &self.ckbadger_workdir
    }

    pub fn ckb_workdir(&self) -> &Path {
        &self.ckb_workdir
    }

    pub fn ckb_db_path(&self) -> &Path {
        &self.ckb_db_path
    }

    pub fn domain_data_path(&self) -> &Path {
        &self.domain_data_path
    }

    pub fn append_only_data_path(&self) -> &Path {
        &self.append_only_data_path
    }

    pub fn direct_io_reads_enabled(&self) -> bool {
        self.store_runtime_config.direct_io_reads
    }

    pub async fn new(api_url: &str, domain_data_path: &str, append_only_data_path: &str) -> Self {
        Self::new_with_monitoring(
            api_url,
            TuiPathConfig {
                domain_data_path,
                append_only_data_path,
                ckbadger_workdir: ".",
                ckb_workdir: ".",
                ckb_db_path: ".",
            },
            None,
            None,
            StoreRuntimeConfig::default(),
        )
        .await
    }

    pub async fn new_with_supervisor_socket(
        api_url: &str,
        domain_data_path: &str,
        append_only_data_path: &str,
        supervisor_socket_path: Option<&str>,
    ) -> Self {
        Self::new_with_monitoring(
            api_url,
            TuiPathConfig {
                domain_data_path,
                append_only_data_path,
                ckbadger_workdir: ".",
                ckb_workdir: ".",
                ckb_db_path: ".",
            },
            supervisor_socket_path,
            None,
            StoreRuntimeConfig::default(),
        )
        .await
    }

    pub async fn new_with_monitoring(
        api_url: &str,
        path_config: TuiPathConfig<'_>,
        supervisor_socket_path: Option<&str>,
        service_log_dir: Option<&str>,
        store_runtime_config: StoreRuntimeConfig,
    ) -> Self {
        // Try to open the domain store in secondary (read-only) mode
        let secondary_path =
            secondary_store_path(path_config.domain_data_path, SecondaryStoreOwner::Tui);
        let store = match CkbadgerStore::open_domain_secondary_with_runtime(
            Path::new(path_config.domain_data_path),
            secondary_path.as_path(),
            store_runtime_config,
        ) {
            Ok(s) => {
                eprintln!(
                    "TUI: opened domain store (secondary) at {}",
                    path_config.domain_data_path
                );
                Some(Arc::new(s))
            }
            Err(e) => {
                eprintln!("TUI: failed to open domain store: {e}");
                None
            }
        };

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_default();

        Self {
            store,
            api_url: api_url.to_string(),
            supervisor_socket_path: supervisor_socket_path.map(PathBuf::from),
            service_log_dir: service_log_dir.map(PathBuf::from),
            http,
            memory_profile: MemoryProfile::for_secondary_with_config(store_runtime_config),
            ckbadger_workdir: PathBuf::from(path_config.ckbadger_workdir),
            ckb_workdir: PathBuf::from(path_config.ckb_workdir),
            ckb_db_path: PathBuf::from(path_config.ckb_db_path),
            domain_data_path: PathBuf::from(path_config.domain_data_path),
            append_only_data_path: PathBuf::from(path_config.append_only_data_path),
            store_runtime_config,
        }
    }

    /// Refresh the secondary store to catch up with the primary.
    fn refresh_store(&self) {
        if let Some(ref store) = self.store {
            if let Err(e) = store.refresh() {
                eprintln!("TUI: store refresh failed: {e}");
            }
        }
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        self.refresh_store();
        self.get_sync_status_without_refresh()
    }

    fn get_sync_progress_from_store(&self) -> Option<SyncProgressData> {
        let store = self.store.as_ref()?;
        let bytes = store.get_sync_progress().ok()??;
        serde_json::from_slice(&bytes).ok()
    }

    fn get_sync_status_from_store(&self) -> Option<SyncStatusData> {
        let store = self.store.as_ref()?;
        let sync = store.get_sync_status().ok()?;
        let tip = sync.tip_block_number;
        let total_tx = sync.total_transactions;
        let total_cells = sync.total_cells_created;
        let total_live_cells = sync.total_cells_created - sync.total_cells_consumed;

        Some(SyncStatusData {
            tip_block_number: tip,
            tip_block_hash: format!("0x{}", hex::encode(&sync.tip_block_hash)),
            total_transactions: total_tx,
            total_cells,
            total_live_cells,
            total_addresses: 0,
            last_synced_at: sync.last_synced_at,
            sync_started_at: sync.sync_started_at,
            sync_started_block: sync.sync_started_block,
            sync_ema_rate: sync.sync_ema_rate,
            bulk_sync_completed_at: sync.bulk_sync_completed_at,
            bulk_sync_completed_block: sync.bulk_sync_completed_block,
        })
    }

    fn build_from_progress(
        &self,
        progress: &SyncProgressData,
        status_data: &Option<SyncStatusData>,
    ) -> SyncStatusRow {
        let progress_tip = progress.current_block as i64;
        let status_tip = status_data
            .as_ref()
            .map(|s| s.tip_block_number)
            .unwrap_or(0);
        let tip_block = progress_tip.max(status_tip);
        let chain_tip = progress.target_block as i64;
        let blocks_behind = chain_tip - tip_block;
        let (is_syncing, is_bulk_sync) =
            sync_modes_from_progress(progress, status_data.as_ref(), blocks_behind);

        let elapsed_time = status_data.as_ref().and_then(|s| {
            s.sync_started_at.map(|started| {
                let end = s
                    .bulk_sync_completed_at
                    .unwrap_or_else(|| chrono::Utc::now().timestamp());
                format_duration_smart((end - started) as f64)
            })
        });

        SyncStatusRow {
            tip_block,
            chain_tip,
            is_syncing,
            is_bulk_sync,
            progress: progress.progress_percentage,
            elapsed_time,
            eta: Some(progress.eta_formatted.clone()),
            eta_seconds: progress.eta_seconds,
            rate_realtime: Some(progress.blocks_per_second),
            rate_ema: Some(progress.ema_blocks_per_second),
            tx_rate_realtime: progress.txs_per_second,
            tx_rate_ema: progress.ema_txs_per_second,
            db_write_ms: progress.db_write_ms,
            db_commit_ms: progress.db_commit_ms,
            rpc_fetch_ms: progress.rpc_fetch_ms,
            pipeline: progress.pipeline.clone(),
            pipeline_reset_epoch: progress.pipeline_reset_epoch,
            pipeline_reset_reason: progress.pipeline_reset_reason.clone(),
            last_batch_blocks: progress.last_batch_blocks,
            startup_phase: progress.startup_phase.clone(),
            is_direct_db_read: progress.is_direct_db_read,
            bulk_build: progress.bulk_build.clone(),
        }
    }

    fn build_from_status(&self, status: &SyncStatusData) -> Result<SyncStatusRow> {
        let tip_block = status.tip_block_number;
        let chain_tip = tip_block;

        let blocks_behind = chain_tip - tip_block;
        let is_syncing = blocks_behind > 0;

        let progress = if chain_tip > 0 {
            (tip_block as f64 / chain_tip as f64 * 100.0).min(100.0)
        } else {
            100.0
        };

        let elapsed_time = status.sync_started_at.map(|started| {
            let end = status
                .bulk_sync_completed_at
                .unwrap_or_else(|| chrono::Utc::now().timestamp());
            format_duration_smart((end - started) as f64)
        });

        let eta = if is_syncing {
            status.sync_ema_rate.and_then(|rate| {
                if rate > 0.0 {
                    Some(format_duration_smart(blocks_behind as f64 / rate))
                } else {
                    None
                }
            })
        } else {
            None
        };

        Ok(SyncStatusRow {
            tip_block,
            chain_tip,
            is_syncing,
            is_bulk_sync: is_syncing && status.bulk_sync_completed_at.is_none(),
            progress,
            elapsed_time,
            eta,
            eta_seconds: None,
            rate_realtime: None,
            rate_ema: status.sync_ema_rate,
            tx_rate_realtime: None,
            tx_rate_ema: None,
            db_write_ms: None,
            db_commit_ms: None,
            rpc_fetch_ms: None,
            pipeline: None,
            pipeline_reset_epoch: None,
            pipeline_reset_reason: None,
            last_batch_blocks: None,
            startup_phase: None,
            is_direct_db_read: false,
            bulk_build: None,
        })
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        self.refresh_store();
        self.get_memory_stats_without_refresh()
    }

    pub async fn get_runtime_diag(&self) -> Option<RuntimeDiagData> {
        self.refresh_store();
        self.get_runtime_diag_without_refresh()
    }

    pub async fn get_local_snapshot(
        &self,
    ) -> (
        Result<SyncStatusRow>,
        Option<MemoryStatsData>,
        Option<RuntimeDiagData>,
        Option<BackgroundTasksData>,
    ) {
        self.refresh_store();
        let bg_tasks = self
            .store
            .as_ref()
            .and_then(|s| s.get_background_tasks().ok());
        (
            self.get_sync_status_without_refresh(),
            self.get_memory_stats_without_refresh(),
            self.get_runtime_diag_without_refresh(),
            bg_tasks,
        )
    }

    fn get_sync_status_without_refresh(&self) -> Result<SyncStatusRow> {
        let now_ts = chrono::Utc::now().timestamp();
        let progress_data: Option<SyncProgressData> = self
            .get_sync_progress_from_store()
            .filter(|progress| !sync_progress_is_stale(progress, now_ts));
        let status_data: Option<SyncStatusData> = self.get_sync_status_from_store();

        if let Some(ref progress) = progress_data {
            return Ok(self.build_from_progress(progress, &status_data));
        }

        if let Some(ref status) = status_data {
            return self.build_from_status(status);
        }

        Err(anyhow::anyhow!(
            "sync status unavailable: store not accessible or empty"
        ))
    }

    fn get_memory_stats_without_refresh(&self) -> Option<MemoryStatsData> {
        let store = self.store.as_ref()?;
        let bytes = store.get_memory_stats().ok()??;
        let mut mem: MemoryStatsData = serde_json::from_slice(&bytes).ok()?;

        if mem.total_transactions == 0 || mem.total_cells == 0 {
            if let Ok(sync) = store.get_sync_status() {
                mem.total_transactions = sync.total_transactions;
                mem.total_cells = sync.total_cells_created;
                mem.total_live_cells = sync.total_cells_created - sync.total_cells_consumed;
                mem.total_addresses = 0;
            }
        }
        Some(mem)
    }

    fn get_runtime_diag_without_refresh(&self) -> Option<RuntimeDiagData> {
        let store = self.store.as_ref()?;
        let status = store.get_runtime_status().ok()?;
        let heartbeat_age_secs = if status.last_heartbeat_at > 0 {
            Some((chrono::Utc::now().timestamp() - status.last_heartbeat_at).max(0))
        } else {
            None
        };

        Some(RuntimeDiagData {
            active_run_id: status.active_run_id,
            last_run_id: status.last_run_id,
            heartbeat_block: status.last_heartbeat_block,
            heartbeat_target_block: status.last_heartbeat_target_block,
            heartbeat_stage: status.last_heartbeat_stage,
            heartbeat_age_secs,
            heartbeat_oom_events: status.last_heartbeat_oom_events,
            heartbeat_oom_kill_events: status.last_heartbeat_oom_kill_events,
            last_incident_summary: status.last_incident_summary,
            last_shutdown_reason: status.last_shutdown_reason,
            last_exit_code: status.last_exit_code,
        })
    }

    pub async fn get_chain_info_and_api_service_info(
        &self,
    ) -> (
        Option<ChainInfoData>,
        ApiServiceInfo,
        Option<Vec<BackgroundTaskEntry>>,
    ) {
        let mut api_info = ApiServiceInfo::default();
        let url = format!("{}/statistics/network", self.api_url);
        let started = Instant::now();

        let response = self.http.get(&url).send().await;
        api_info.latency_ms = Some(started.elapsed().as_secs_f64() * 1000.0);

        let response = match response {
            Ok(resp) => resp,
            Err(e) => {
                api_info.error = Some(format!("request failed: {e}"));
                return (None, api_info, None);
            }
        };

        api_info.reachable = true;
        api_info.status_code = Some(response.status().as_u16());
        if !response.status().is_success() {
            api_info.error = Some(format!("http {}", response.status()));
            return (None, api_info, None);
        }

        match response.json::<ApiNetworkStats>().await {
            Ok(stats) => {
                api_info.latest_block = Some(stats.latest_block);
                api_info.tps = Some(stats.tps.clone());
                api_info.avg_block_time = Some(stats.avg_block_time.clone());
                let chain_info = chain_info_from_api_stats(&stats);
                let api_bg_tasks = stats.api_background_tasks;
                (Some(chain_info), api_info, api_bg_tasks)
            }
            Err(e) => {
                api_info.error = Some(format!("decode failed: {e}"));
                (None, api_info, None)
            }
        }
    }

    pub async fn get_supervisor_services(&self) -> Option<Vec<SupervisorServiceData>> {
        let socket_path = self.supervisor_socket_path.as_ref()?;
        if !socket_path.exists() {
            return None;
        }

        let request = ckbadger_ipc::IpcRequest::GetServiceStatus;
        let response = tokio::time::timeout(
            Duration::from_millis(400),
            ckbadger_ipc::ipc_request(socket_path, &request),
        )
        .await
        .ok()?
        .ok()?;

        let ckbadger_ipc::IpcResponse::ServiceStatus { services } = response else {
            return None;
        };

        Some(
            services
                .into_iter()
                .map(|service| SupervisorServiceData {
                    name: service.name,
                    pid: service.pid,
                    status: service.status.to_string(),
                    uptime_secs: service.uptime_secs,
                })
                .collect(),
        )
    }

    pub async fn get_service_log_tails(&self) -> Option<Vec<ServiceLogTailData>> {
        let log_dir = self.service_log_dir.as_ref()?;
        if !log_dir.exists() {
            return None;
        }

        let entries = std::fs::read_dir(log_dir).ok()?;
        let mut tails = Vec::new();
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("log") {
                continue;
            }
            let service = path.file_stem()?.to_str()?.to_string();
            if let Some(last_line) = read_last_non_empty_line(&path) {
                tails.push(ServiceLogTailData { service, last_line });
            }
        }

        tails.sort_by(|a, b| a.service.cmp(&b.service));
        Some(tails)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLastRound {
    pub round_id: u64,
    pub started: u64,
    pub finished: u64,
    pub dialed: u64,
    pub reachable: u64,
    pub unreachable: u64,
    pub foreign_dropped: u64,
    pub new_nodes: u64,
    pub total_known: u64,
    pub frontier_drained: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    pub enabled: bool,
    pub has_data: bool,
    pub last_round: Option<NetworkLastRound>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LabelCount {
    pub label: String,
    pub count: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDistributions {
    pub total_known: u64,
    pub reachable: u64,
    pub unreachable: u64,
    pub versions: Vec<LabelCount>,
    pub countries: Vec<LabelCount>,
    pub asns: Vec<LabelCount>,
    pub protocols: Vec<LabelCount>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NetworkHistoryPoint {
    pub ts: u64,
    pub scalar: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NetworkHistory {
    pub points: Vec<NetworkHistoryPoint>,
}

#[derive(Debug, Clone, Default)]
pub struct PeersData {
    pub summary: Option<NetworkSummary>,
    pub distributions: Option<NetworkDistributions>,
    pub total_history: Vec<NetworkHistoryPoint>,
    pub reachable_history: Vec<NetworkHistoryPoint>,
    pub error: Option<String>,
}

impl TuiDb {
    /// Fetch crawler status + (if it has data) distributions and hourly trend from the
    /// Plan-2 `/network/*` API. Best-effort: any error is recorded in `error`, never panics.
    pub async fn get_peers_data(&self) -> PeersData {
        let mut out = PeersData::default();

        // 1. summary (drives the off/waiting/dashboard switch)
        match self.fetch_json::<NetworkSummary>("/network/summary").await {
            Ok(s) => out.summary = Some(s),
            Err(e) => {
                out.error = Some(e);
                return out;
            }
        }
        // 2. off / no-data => nothing to chart
        let has_data = out.summary.as_ref().map(|s| s.has_data).unwrap_or(false);
        if !has_data {
            return out;
        }
        // 3. distributions + hourly trend (best-effort)
        match self
            .fetch_json::<NetworkDistributions>("/network/distributions")
            .await
        {
            Ok(d) => out.distributions = Some(d),
            Err(e) => out.error = Some(e),
        }
        match self
            .fetch_json::<NetworkHistory>("/network/history?metric=totalNodes&granularity=hour")
            .await
        {
            Ok(h) => out.total_history = h.points,
            Err(e) => out.error = Some(e),
        }
        match self
            .fetch_json::<NetworkHistory>("/network/history?metric=reachableNodes&granularity=hour")
            .await
        {
            Ok(h) => out.reachable_history = h.points,
            Err(e) => out.error = Some(e),
        }
        out
    }

    /// GET `self.api_url + path` and decode JSON, mapping every failure to a String
    /// (mirrors the error handling around the existing `/statistics/network` fetch).
    async fn fetch_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = format!("{}{}", self.api_url, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("http {}", resp.status()));
        }
        resp.json::<T>()
            .await
            .map_err(|e| format!("decode failed: {e}"))
    }
}

fn read_last_non_empty_line(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().rev().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        parse_epoch_string, sync_modes_from_progress, sync_progress_is_stale, NetworkDistributions,
        NetworkHistory, NetworkSummary, TuiDb, TuiPathConfig, LEGACY_BULK_SYNC_THRESHOLD_BLOCKS,
        SYNC_PROGRESS_STALE_SECS,
    };
    use ckbadger_common::{SyncProgressData, SyncStatusData};
    use ckbadger_ipc::{
        IpcHandler, IpcRequest, IpcResponse, IpcServer, ServiceInfo, ServiceStatus,
    };
    use ckbadger_store::{
        secondary_store_path, CkbadgerStore, RuntimeStatus, SecondaryStoreOwner, StoreRuntimeConfig,
    };
    use std::future::Future;
    use std::path::Path;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    fn sample_progress() -> SyncProgressData {
        SyncProgressData {
            current_block: 1000,
            target_block: 2000,
            last_batch_blocks: Some(64),
            blocks_per_second: 100.0,
            ema_blocks_per_second: 95.0,
            txs_per_second: Some(2_000.0),
            ema_txs_per_second: Some(1_900.0),
            eta_seconds: Some(90.0),
            eta_formatted: "1m 30s".to_string(),
            progress_percentage: 10.0,
            updated_at: 1_234_567_890,
            startup_phase: Some("bulk_sync".to_string()),
            is_direct_db_read: false,
            db_write_ms: Some(11.0),
            db_commit_ms: Some(4.0),
            rpc_fetch_ms: Some(7.0),
            pipeline: None,
            pipeline_reset_epoch: None,
            pipeline_reset_reason: None,
            bulk_build: None,
        }
    }

    fn cleanup_tui_temp_stores(domain_path: &Path, append_path: &Path) {
        let secondary_path = secondary_store_path(domain_path, SecondaryStoreOwner::Tui);
        for path in [secondary_path.as_path(), append_path, domain_path] {
            if path.exists() {
                std::fs::remove_dir_all(path).unwrap();
            }
        }
    }

    #[test]
    fn parse_epoch_full() {
        assert_eq!(parse_epoch_string("100(800/1800)"), (100, 800, 1800));
    }

    #[test]
    fn parse_epoch_without_details() {
        assert_eq!(parse_epoch_string("101"), (101, 0, 1800));
    }

    #[test]
    fn parse_epoch_invalid() {
        assert_eq!(parse_epoch_string("bad"), (0, 0, 1800));
    }

    #[test]
    fn parse_network_summary_null_and_populated() {
        let off: NetworkSummary =
            serde_json::from_str(r#"{"enabled":false,"hasData":false,"lastRound":null}"#).unwrap();
        assert!(!off.enabled && !off.has_data && off.last_round.is_none());

        let on: NetworkSummary = serde_json::from_str(
            r#"{"enabled":true,"hasData":true,"lastRound":{"roundId":5,"started":1,"finished":2,"dialed":8,"reachable":3,"unreachable":1,"foreignDropped":0,"newNodes":2,"totalKnown":4,"frontierDrained":true}}"#,
        ).unwrap();
        let lr = on.last_round.unwrap();
        assert_eq!(lr.total_known, 4);
        assert_eq!(lr.reachable, 3);
        assert!(lr.frontier_drained);
    }

    #[test]
    fn parse_distributions_and_history() {
        let d: NetworkDistributions = serde_json::from_str(
            r#"{"totalKnown":4,"reachable":3,"unreachable":1,"versions":[{"label":"0.119.0","count":3}],"countries":[{"label":"US","count":2}],"asns":[],"protocols":[]}"#,
        ).unwrap();
        assert_eq!(d.versions[0].label, "0.119.0");
        assert_eq!(d.countries[0].count, 2);

        let h: NetworkHistory =
            serde_json::from_str(r#"{"points":[{"ts":3600,"scalar":4},{"ts":7200,"scalar":5}]}"#)
                .unwrap();
        assert_eq!(h.points.len(), 2);
        assert_eq!(h.points[1].scalar, 5);
    }

    #[test]
    fn sync_modes_from_progress_uses_lag_when_status_missing() {
        let progress = sample_progress();
        let (is_syncing, is_bulk_sync) = sync_modes_from_progress(&progress, None, 10_000);
        assert!(is_syncing);
        assert!(is_bulk_sync);
    }

    #[test]
    fn sync_modes_from_progress_falls_back_to_status_or_legacy_lag() {
        let progress = sample_progress();

        // bulk_sync_completed_at is None => bulk sync still in progress
        let status_hint = SyncStatusData {
            bulk_sync_completed_at: None,
            ..Default::default()
        };
        let (is_syncing, is_bulk_sync) = sync_modes_from_progress(&progress, Some(&status_hint), 8);
        assert!(is_syncing);
        assert!(is_bulk_sync);

        let (is_syncing_legacy, is_bulk_sync_legacy) =
            sync_modes_from_progress(&progress, None, 1001);
        assert!(is_syncing_legacy);
        assert!(is_bulk_sync_legacy);
    }

    #[test]
    fn sync_modes_legacy_threshold_constant_is_stable() {
        assert_eq!(LEGACY_BULK_SYNC_THRESHOLD_BLOCKS, 1000);
    }

    #[test]
    fn sync_progress_stale_rule_is_stable() {
        assert_eq!(SYNC_PROGRESS_STALE_SECS, 60);
        let progress = sample_progress();
        let now_ts = progress.updated_at + SYNC_PROGRESS_STALE_SECS;
        assert!(!sync_progress_is_stale(&progress, now_ts));
        assert!(sync_progress_is_stale(&progress, now_ts + 1));
    }

    #[tokio::test]
    async fn tui_db_exposes_paths_and_profile_without_store() {
        let db = TuiDb::new(
            "http://127.0.0.1:3001/api/v1",
            "/tmp/nonexistent-domain-store-test",
            "/tmp/nonexistent-append-store-test",
        )
        .await;
        assert_eq!(
            db.domain_data_path(),
            Path::new("/tmp/nonexistent-domain-store-test")
        );
        assert_eq!(
            db.append_only_data_path(),
            Path::new("/tmp/nonexistent-append-store-test")
        );
        assert_eq!(db.ckbadger_workdir(), Path::new("."));
        assert_eq!(db.ckb_workdir(), Path::new("."));
        assert_eq!(db.ckb_db_path(), Path::new("."));
        assert!(db.memory_profile().is_secondary);
    }

    #[tokio::test]
    async fn tui_sync_status_progress_uses_persisted_elapsed_time() {
        let test_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let domain_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-domain"));
        let append_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-append"));
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let store = CkbadgerStore::open_domain(&domain_path).unwrap();

        let mut progress = sample_progress();
        progress.updated_at = chrono::Utc::now().timestamp();
        let progress_bytes = serde_json::to_vec(&progress).unwrap();
        store.put_sync_progress(&progress_bytes).unwrap();

        store
            .set_sync_status(&ckbadger_store::types::SyncStatus {
                tip_block_number: progress.current_block as i64,
                tip_block_hash: vec![0x11; 32],
                total_transactions: 1,
                total_cells_created: 1,
                total_cells_consumed: 0,
                last_synced_at: 1_700_000_000,
                sync_started_at: Some(1_700_000_000),
                sync_started_block: 0,
                sync_ema_rate: Some(95.0),
                bulk_sync_completed_at: Some(1_700_000_090),
                bulk_sync_completed_block: Some(progress.target_block as i64),
                deep_fork_detected: false,
                deep_fork_info: None,
            })
            .unwrap();

        let db = TuiDb::new(
            "http://127.0.0.1:3001/api/v1",
            domain_path.to_str().unwrap(),
            append_path.to_str().unwrap(),
        )
        .await;

        let sync = db.get_sync_status().await.unwrap();
        assert_eq!(sync.eta.as_deref(), Some("1m 30s"));
        assert_eq!(sync.elapsed_time.as_deref(), Some("1m 30s"));

        cleanup_tui_temp_stores(&domain_path, &append_path);
    }

    #[tokio::test]
    async fn tui_test_cleanup_must_remove_secondary_directory() {
        let test_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let domain_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-domain"));
        let append_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-append"));
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();
        let _store = CkbadgerStore::open_domain(&domain_path).unwrap();

        {
            let _db = TuiDb::new(
                "http://127.0.0.1:3001/api/v1",
                domain_path.to_str().unwrap(),
                append_path.to_str().unwrap(),
            )
            .await;
        }

        let secondary_path = secondary_store_path(&domain_path, SecondaryStoreOwner::Tui);
        assert!(secondary_path.exists());

        cleanup_tui_temp_stores(&domain_path, &append_path);

        assert!(
            !secondary_path.exists(),
            "secondary path should be removed during test cleanup: {}",
            secondary_path.display()
        );
    }

    #[tokio::test]
    async fn tui_sync_status_ignores_stale_progress_data() {
        let test_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let domain_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-domain"));
        let append_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-append"));
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let store = CkbadgerStore::open_domain(&domain_path).unwrap();

        let mut stale_progress = sample_progress();
        stale_progress.current_block = 100;
        stale_progress.target_block = 200;
        stale_progress.updated_at = chrono::Utc::now().timestamp() - (SYNC_PROGRESS_STALE_SECS + 5);
        store
            .put_sync_progress(&serde_json::to_vec(&stale_progress).unwrap())
            .unwrap();

        store
            .set_sync_status(&ckbadger_store::types::SyncStatus {
                tip_block_number: 150,
                tip_block_hash: vec![0x22; 32],
                total_transactions: 9000,
                total_cells_created: 20000,
                total_cells_consumed: 10000,
                last_synced_at: chrono::Utc::now().timestamp(),
                sync_started_at: Some(1_700_000_000),
                sync_started_block: 1,
                sync_ema_rate: Some(42.0),
                bulk_sync_completed_at: None,
                bulk_sync_completed_block: None,
                deep_fork_detected: false,
                deep_fork_info: None,
            })
            .unwrap();

        let db = TuiDb::new(
            "http://127.0.0.1:3001/api/v1",
            domain_path.to_str().unwrap(),
            append_path.to_str().unwrap(),
        )
        .await;

        let sync = db.get_sync_status().await.unwrap();
        assert_eq!(sync.tip_block, 150);
        assert!(sync.rate_realtime.is_none());
        assert_eq!(sync.rate_ema, Some(42.0));

        cleanup_tui_temp_stores(&domain_path, &append_path);
    }

    #[tokio::test]
    async fn tui_runtime_diag_reads_runtime_status_from_store() {
        let test_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let domain_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-domain"));
        let append_path = std::env::temp_dir().join(format!("ckbadger-tui-{test_id}-append"));
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let store = CkbadgerStore::open_domain(&domain_path).unwrap();
        store
            .set_runtime_status(&RuntimeStatus {
                active_run_id: Some("run-active".to_string()),
                last_run_id: Some("run-last".to_string()),
                run_started_at: 1_700_000_000,
                last_heartbeat_at: chrono::Utc::now().timestamp() - 7,
                last_heartbeat_block: 123,
                last_heartbeat_target_block: 130,
                last_heartbeat_stage: Some("bulk_sync".to_string()),
                last_heartbeat_oom_events: Some(2),
                last_heartbeat_oom_kill_events: Some(1),
                last_shutdown_reason: Some("graceful_shutdown".to_string()),
                last_exit_code: Some(0),
                last_incident_id: Some("run-active-inc-000001".to_string()),
                last_incident_at: chrono::Utc::now().timestamp() - 12,
                last_incident_summary: Some("pipeline backpressure".to_string()),
                last_shutdown_at: chrono::Utc::now().timestamp() - 20,
            })
            .unwrap();

        let db = TuiDb::new(
            "http://127.0.0.1:3001/api/v1",
            domain_path.to_str().unwrap(),
            append_path.to_str().unwrap(),
        )
        .await;

        let runtime = db.get_runtime_diag().await.expect("runtime diagnostics");
        assert_eq!(runtime.active_run_id.as_deref(), Some("run-active"));
        assert_eq!(runtime.last_run_id.as_deref(), Some("run-last"));
        assert_eq!(runtime.heartbeat_block, 123);
        assert_eq!(runtime.heartbeat_target_block, 130);
        assert_eq!(runtime.heartbeat_stage.as_deref(), Some("bulk_sync"));
        assert!(runtime.heartbeat_age_secs.is_some());
        assert_eq!(runtime.heartbeat_oom_events, Some(2));
        assert_eq!(runtime.heartbeat_oom_kill_events, Some(1));
        assert_eq!(
            runtime.last_incident_summary.as_deref(),
            Some("pipeline backpressure")
        );
        assert_eq!(
            runtime.last_shutdown_reason.as_deref(),
            Some("graceful_shutdown")
        );
        assert_eq!(runtime.last_exit_code, Some(0));

        cleanup_tui_temp_stores(&domain_path, &append_path);
    }

    struct StaticStatusHandler;

    impl IpcHandler for StaticStatusHandler {
        fn handle(
            &self,
            request: IpcRequest,
        ) -> Pin<Box<dyn Future<Output = IpcResponse> + Send + '_>> {
            Box::pin(async move {
                match request {
                    IpcRequest::GetServiceStatus => IpcResponse::ServiceStatus {
                        services: vec![
                            ServiceInfo {
                                name: "indexer".to_string(),
                                pid: 1234,
                                status: ServiceStatus::Running,
                                uptime_secs: 42,
                            },
                            ServiceInfo {
                                name: "api".to_string(),
                                pid: 5678,
                                status: ServiceStatus::Restarting,
                                uptime_secs: 7,
                            },
                        ],
                    },
                    IpcRequest::Ping => IpcResponse::Pong,
                    IpcRequest::Shutdown { .. } => IpcResponse::Ok,
                }
            })
        }
    }

    #[tokio::test]
    async fn tui_supervisor_services_reads_ipc_status() {
        let test_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("ckbadger-tui-ipc-{test_id}"));
        let domain_path = root.join("data-domain");
        let append_path = root.join("data-append");
        let socket_path = root.join("indexer.sock");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let handler: Arc<dyn IpcHandler + Send + Sync> = Arc::new(StaticStatusHandler);
        let server = IpcServer::new(socket_path.clone(), handler);
        let server_handle = tokio::spawn(async move {
            server.listen().await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(60)).await;

        let db = TuiDb::new_with_supervisor_socket(
            "http://127.0.0.1:3001/api/v1",
            domain_path.to_str().unwrap(),
            append_path.to_str().unwrap(),
            Some(socket_path.to_str().unwrap()),
        )
        .await;

        let services = db
            .get_supervisor_services()
            .await
            .expect("supervisor services");
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "indexer");
        assert_eq!(services[0].status, "running");
        assert_eq!(services[1].name, "api");
        assert_eq!(services[1].status, "restarting");

        server_handle.abort();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn tui_service_log_tails_reads_last_non_empty_lines() {
        let test_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("ckbadger-tui-logs-{test_id}"));
        let domain_path = root.join("data-domain");
        let append_path = root.join("data-append");
        let log_dir = root.join("run-logs");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();
        std::fs::create_dir_all(&log_dir).unwrap();

        std::fs::write(
            log_dir.join("indexer.log"),
            "2026-01-01 boot\n\n2026-01-01 pipeline mismatch\n",
        )
        .unwrap();
        std::fs::write(log_dir.join("api.log"), "first\nsecond\n").unwrap();

        let db = TuiDb::new_with_monitoring(
            "http://127.0.0.1:3001/api/v1",
            TuiPathConfig {
                domain_data_path: domain_path.to_str().unwrap(),
                append_only_data_path: append_path.to_str().unwrap(),
                ckbadger_workdir: ".",
                ckb_workdir: ".",
                ckb_db_path: ".",
            },
            None,
            Some(log_dir.to_str().unwrap()),
            StoreRuntimeConfig::default(),
        )
        .await;

        let tails = db.get_service_log_tails().await.expect("service log tails");
        assert_eq!(tails.len(), 2);
        assert_eq!(tails[0].service, "api");
        assert_eq!(tails[0].last_line, "second");
        assert_eq!(tails[1].service, "indexer");
        assert_eq!(tails[1].last_line, "2026-01-01 pipeline mismatch");

        std::fs::remove_dir_all(root).unwrap();
    }
}
