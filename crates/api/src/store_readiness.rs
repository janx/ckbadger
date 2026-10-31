//! Advancing the API's secondary views.
//!
//! The API reads RocksDB secondaries, whose view advances only when
//! `try_catch_up_with_primary()` runs. The refresh loop below advances every
//! secondary this process reads, together, once per [`STORE_REFRESH_INTERVAL`].
//!
//! [`SecondaryCatchUp`] is the loop's seam: production advances the stores it
//! opened ([`ProcessSecondaries`]); a test injects one that fails on cue.

use arc_swap::ArcSwapOption;
use ckb_store_reader::CkbChainReader;
use ckbadger_store::read_view;
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;
use std::time::Duration;

use crate::AppState;

/// How often the refresh loop advances the secondaries.
pub const STORE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// A secondary view this API process reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecondaryStore {
    AppendOnly,
    Domain,
    /// The CKB node's own RocksDB (`ckb-store-reader`), when it opened.
    CkbNode,
    /// The opt-in network-crawler store, once its primary exists.
    Network,
}

impl SecondaryStore {
    /// The name logs and error messages use.
    pub fn name(self) -> &'static str {
        match self {
            Self::AppendOnly => "append-only",
            Self::Domain => "domain",
            Self::CkbNode => "ckb node",
            Self::Network => "network",
        }
    }
}

/// How one secondary's catch-up ended in one round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchUpOutcome {
    pub store: SecondaryStore,
    /// The error text (with its context chain) when the catch-up failed.
    pub result: Result<(), String>,
}

impl CatchUpOutcome {
    pub fn new(store: SecondaryStore, result: anyhow::Result<()>) -> Self {
        Self {
            store,
            result: result.map_err(|error| format!("{error:#}")),
        }
    }
}

/// Advances every secondary view this process reads, together.
pub trait SecondaryCatchUp: Send + Sync {
    /// One catch-up round: every present secondary, append-only first, one
    /// outcome each. Blocking — the refresh loop runs it on its own thread,
    /// never inside a pinned read view.
    fn catch_up_round(&self) -> Vec<CatchUpOutcome>;
}

/// The secondaries this API process opened: the production [`SecondaryCatchUp`].
pub struct ProcessSecondaries {
    append_only: Arc<CkbadgerStore>,
    domain: Arc<CkbadgerStore>,
    ckb_node: Option<Arc<CkbChainReader>>,
    network: Arc<ArcSwapOption<CkbadgerStore>>,
}

impl ProcessSecondaries {
    /// The secondaries `state` reads from.
    pub fn of(state: &AppState) -> Self {
        Self {
            append_only: state.append_only_store.clone(),
            domain: state.store.clone(),
            ckb_node: state.ckb_store.clone(),
            network: state.network_store.clone(),
        }
    }
}

impl SecondaryCatchUp for ProcessSecondaries {
    fn catch_up_round(&self) -> Vec<CatchUpOutcome> {
        // The opt-in network secondary may attach between rounds.
        let network = self.network.load_full();
        // One exclusive window for every secondary this process reads. It
        // waits for in-flight requests (each pins the view for its whole
        // lifetime) and blocks new ones, so no response can mix a
        // pre-catch-up index row with a post-catch-up entry, and no
        // cross-store read can see domain advanced past append-only. Refresh
        // order inside the window is therefore invisible to readers;
        // append-only stays first to match the indexer's own commit order.
        let window = read_view::catch_up_window();
        let mut outcomes = vec![
            CatchUpOutcome::new(
                SecondaryStore::AppendOnly,
                self.append_only.catch_up_in_window(&window),
            ),
            CatchUpOutcome::new(
                SecondaryStore::Domain,
                self.domain.catch_up_in_window(&window),
            ),
        ];
        if let Some(ckb_node) = &self.ckb_node {
            // Owned by ckb-store-reader, which has no window type; it is
            // already inside ours.
            outcomes.push(CatchUpOutcome::new(
                SecondaryStore::CkbNode,
                ckb_node.refresh(),
            ));
        }
        if let Some(network) = &network {
            outcomes.push(CatchUpOutcome::new(
                SecondaryStore::Network,
                network.catch_up_in_window(&window),
            ));
        }
        outcomes
    }
}

/// Advance the secondaries every [`STORE_REFRESH_INTERVAL`], forever.
pub async fn run_store_refresh_loop(secondaries: Arc<dyn SecondaryCatchUp>) {
    loop {
        tokio::time::sleep(STORE_REFRESH_INTERVAL).await;
        let round = secondaries.clone();
        match tokio::task::spawn_blocking(move || round.catch_up_round()).await {
            Ok(outcomes) => {
                for outcome in outcomes {
                    if let Err(error) = outcome.result {
                        tracing::warn!("{} store refresh failed: {}", outcome.store.name(), error);
                    }
                }
            }
            Err(error) => tracing::warn!("Store refresh task panicked: {}", error),
        }
    }
}
