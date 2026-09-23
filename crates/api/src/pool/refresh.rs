//! The background loop that keeps the tx-pool mirror fresh.
//!
//! Same shape as `warmup::refresh_*_loop`: spawned by `create_router` when
//! background tasks are enabled, reporting into `background_tasks` so
//! `/status` and the TUI can see whether the mirror is healthy.

use std::sync::Arc;
use std::time::Duration;

use ckbadger_common::{BackgroundTaskKind, BackgroundTaskState};

use super::mirror::{PoolRefresher, PoolRefresherConfig};
use super::source::HttpPoolSource;
use crate::AppState;

pub const POOL_MIRROR_TASK: &str = "pool_mirror";

/// Poll the node's tx pool until the process exits.
///
/// The loop owns the mirror's working set; each round publishes a whole
/// snapshot. Nothing here writes to any store.
pub async fn refresh_pool_mirror_loop(state: Arc<AppState>, poll_interval: Duration) {
    if !state.pool_mirror.enabled() {
        tracing::info!("tx-pool mirror disabled by configuration");
        return;
    }

    let source = Arc::new(HttpPoolSource::new(state.ckb_rpc_url.clone()));
    let mut refresher = PoolRefresher::new(
        source,
        state.store.clone(),
        state.pool_mirror.clone(),
        PoolRefresherConfig {
            max_tracked_txs: state.pool_max_tracked_txs,
            is_mainnet: state.ckb_network == "mainnet",
        },
    );

    state.update_background_task(POOL_MIRROR_TASK, |entry| {
        entry.kind = BackgroundTaskKind::Watcher;
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
        entry.message = Some("Mirroring node tx pool".to_string());
    });

    let mut last_error: Option<String> = None;
    loop {
        let started = std::time::Instant::now();
        let outcome = refresher.refresh_once().await;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

        match outcome.error {
            Some(error) => {
                if last_error.as_deref() != Some(error.as_str()) {
                    tracing::warn!("tx-pool mirror refresh failed: {}", error);
                    last_error = Some(error.clone());
                }
                state.update_background_task(POOL_MIRROR_TASK, |entry| {
                    entry.state = BackgroundTaskState::Failed;
                    entry.elapsed_ms = Some(elapsed_ms);
                    entry.error = Some(error.clone());
                    entry.message = Some("Pool view unavailable".to_string());
                });
            }
            None => {
                last_error = None;
                state.update_background_task(POOL_MIRROR_TASK, |entry| {
                    entry.state = BackgroundTaskState::Running;
                    entry.elapsed_ms = Some(elapsed_ms);
                    entry.error = None;
                    entry.last_success_at = Some(chrono::Utc::now().timestamp());
                    entry.progress_current = Some(outcome.tracked as u64);
                    entry.message = Some(if outcome.skipped {
                        format!("{} pool transactions (unchanged)", outcome.tracked)
                    } else if outcome.entry_errors > 0 {
                        format!(
                            "{} pool transactions, {} unreadable",
                            outcome.tracked, outcome.entry_errors
                        )
                    } else {
                        format!("{} pool transactions", outcome.tracked)
                    });
                });
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}
