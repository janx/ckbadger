use anyhow::Result;
use ckbadger_common::{BackgroundTaskEntry, BackgroundTasksData, MemoryStatsData};
use ckbadger_store::StoreRuntimeConfig;
use std::path::PathBuf;

use crate::db::{
    fetch_service_log_tails, fetch_supervisor_services, ApiServiceInfo, ChainInfoData, PeersData,
    RuntimeDiagData, ServiceLogTailData, SupervisorServiceData, SyncStatusRow, TuiDb,
    TuiPathConfig,
};

/// One network's resolved paths + API endpoint, built by the CLI. The TUI stays
/// free of `ckbadger-config`; it receives only these resolved primitives.
#[derive(Debug, Clone)]
pub struct TuiNetwork {
    pub name: String,
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub ckbadger_workdir: String,
    pub ckb_workdir: String,
    pub ckb_db_path: String,
    pub api_url: String,
    pub store_runtime_config: StoreRuntimeConfig,
}

/// One network's cheap/local snapshot — everything read from its RocksDB secondary
/// store in a single pass. `sync` is a `Result` so a store failure is an explicit
/// error state (fail-fast), never a silent zero.
pub struct NetworkLocal {
    pub name: String,
    pub sync: Result<SyncStatusRow>,
    pub memory: Option<MemoryStatsData>,
    pub runtime: Option<RuntimeDiagData>,
    pub indexer_bg: Option<BackgroundTasksData>,
}

/// Aggregates one `TuiDb` per network behind the single shared supervisor socket
/// and root log dir. Single-network is the 1-element case and behaves identically
/// to a bare `TuiDb`.
pub struct MultiNetworkDb {
    networks: Vec<(String, TuiDb)>,
    supervisor_socket: Option<PathBuf>,
    service_log_dir: Option<PathBuf>,
    selected: usize,
}

impl MultiNetworkDb {
    /// Open one secondary store per network. `supervisor_socket` / `service_log_dir`
    /// are the SHARED root paths (per-network `TuiDb`s get `None` for both).
    pub async fn new(
        networks: Vec<TuiNetwork>,
        supervisor_socket: Option<String>,
        service_log_dir: Option<String>,
    ) -> Self {
        assert!(
            !networks.is_empty(),
            "MultiNetworkDb requires at least one network"
        );
        let mut dbs = Vec::with_capacity(networks.len());
        for net in networks {
            let db = TuiDb::new_with_monitoring(
                &net.api_url,
                TuiPathConfig {
                    domain_data_path: &net.domain_data_path,
                    append_only_data_path: &net.append_only_data_path,
                    ckbadger_workdir: &net.ckbadger_workdir,
                    ckb_workdir: &net.ckb_workdir,
                    ckb_db_path: &net.ckb_db_path,
                },
                None,
                None,
                net.store_runtime_config,
            )
            .await;
            dbs.push((net.name, db));
        }
        Self {
            networks: dbs,
            supervisor_socket: supervisor_socket.map(PathBuf::from),
            service_log_dir: service_log_dir.map(PathBuf::from),
            selected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.networks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.networks.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        self.networks.iter().map(|(n, _)| n.as_str()).collect()
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected(&self) -> &TuiDb {
        &self.networks[self.selected].1
    }

    pub fn selected_name(&self) -> &str {
        &self.networks[self.selected].0
    }

    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.networks.len();
    }

    pub fn select_prev(&mut self) {
        self.selected = (self.selected + self.networks.len() - 1) % self.networks.len();
    }

    /// Cheap local snapshots for EVERY network (RocksDB secondary reads).
    pub async fn refresh_all_local(&self) -> Vec<NetworkLocal> {
        let mut out = Vec::with_capacity(self.networks.len());
        for (name, db) in &self.networks {
            let (sync, memory, runtime, indexer_bg) = db.get_local_snapshot().await;
            out.push(NetworkLocal {
                name: name.clone(),
                sync,
                memory,
                runtime,
                indexer_bg,
            });
        }
        out
    }

    /// Cheap local snapshot for the SELECTED network only.
    pub async fn selected_local(
        &self,
    ) -> (
        Result<SyncStatusRow>,
        Option<MemoryStatsData>,
        Option<RuntimeDiagData>,
        Option<BackgroundTasksData>,
    ) {
        self.selected().get_local_snapshot().await
    }

    /// Service status from the single SHARED supervisor socket (all networks,
    /// labeled `"<net>/<svc>"`).
    pub async fn get_supervisor_services(&self) -> Option<Vec<SupervisorServiceData>> {
        let socket = self.supervisor_socket.as_ref()?;
        fetch_supervisor_services(socket).await
    }

    /// Log tails from the single SHARED root log dir.
    pub async fn get_service_log_tails(&self) -> Option<Vec<ServiceLogTailData>> {
        let dir = self.service_log_dir.as_ref()?;
        fetch_service_log_tails(dir).await
    }

    /// HTTP chain stats for the SELECTED network only.
    pub async fn selected_chain_info_and_api(
        &self,
    ) -> (
        Option<ChainInfoData>,
        ApiServiceInfo,
        Option<Vec<BackgroundTaskEntry>>,
    ) {
        self.selected().get_chain_info_and_api_service_info().await
    }

    /// Peers dashboard for the SELECTED network only.
    pub async fn selected_peers(&self) -> PeersData {
        self.selected().get_peers_data().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(name: &str) -> TuiNetwork {
        TuiNetwork {
            name: name.to_string(),
            domain_data_path: format!("/nonexistent-ckbadger-tui/{name}/domain"),
            append_only_data_path: format!("/nonexistent-ckbadger-tui/{name}/append"),
            ckbadger_workdir: ".".into(),
            ckb_workdir: ".".into(),
            ckb_db_path: ".".into(),
            api_url: format!("http://127.0.0.1:1/{name}"),
            store_runtime_config: StoreRuntimeConfig::default(),
        }
    }

    #[tokio::test]
    async fn selection_wraps_around_both_directions() {
        let mut db = MultiNetworkDb::new(vec![net("mainnet"), net("testnet")], None, None).await;
        assert_eq!(db.len(), 2);
        assert_eq!(db.names(), vec!["mainnet", "testnet"]);
        assert_eq!(db.selected_index(), 0);
        assert_eq!(db.selected_name(), "mainnet");
        db.select_next();
        assert_eq!(db.selected_name(), "testnet");
        db.select_next(); // wraps 1 -> 0
        assert_eq!(db.selected_name(), "mainnet");
        db.select_prev(); // wraps 0 -> 1
        assert_eq!(db.selected_name(), "testnet");
    }

    #[tokio::test]
    async fn refresh_all_local_yields_one_entry_per_network_erroring_without_store() {
        let db = MultiNetworkDb::new(vec![net("mainnet"), net("testnet")], None, None).await;
        let locals = db.refresh_all_local().await;
        assert_eq!(locals.len(), 2);
        assert_eq!(locals[0].name, "mainnet");
        assert_eq!(locals[1].name, "testnet");
        // No store could be opened at the fake path => explicit error, not a zero.
        assert!(locals[0].sync.is_err());
    }

    #[tokio::test]
    async fn supervisor_services_is_none_without_shared_socket() {
        let db = MultiNetworkDb::new(vec![net("mainnet")], None, None).await;
        assert!(db.get_supervisor_services().await.is_none());
        assert!(db.get_service_log_tails().await.is_none());
    }
}
