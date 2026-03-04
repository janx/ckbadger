use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use super::adaptive::bump_pipeline_reset_epoch;
use super::indexer::Indexer;
use super::types::SyncAction;

impl Indexer {
    pub(crate) async fn run_sequential(&self) -> Result<()> {
        loop {
            if self.rebuild_pause_flag.load(Ordering::SeqCst) {
                debug!("Sync paused for index rebuild");
                sleep(Duration::from_millis(500)).await;
                continue;
            }

            // Bulk sync is an optimistic rebuild path and must not run reorg/deep-fork handling.
            let should_handle_reorg =
                self.should_handle_reorg_for_lag(self.progress.blocks_remaining());
            if should_handle_reorg && self.repo.has_unresolved_deep_fork()? {
                if let Some(repeat) = self.repeated_warning_snapshot(
                    "sequential_deep_fork_unresolved",
                    Duration::from_secs(120),
                ) {
                    warn!(
                        run_id = %self.run_id,
                        repeat_count = repeat.total_count,
                        suppressed_since_last = repeat.suppressed_since_last_emit,
                        first_seen_secs_ago = repeat.first_seen_secs_ago,
                        "Deep fork unresolved, sync paused. Waiting for manual intervention..."
                    );
                }
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            match self.sync_batch().await {
                Ok(SyncAction::CaughtUp) => {
                    sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                }
                Ok(SyncAction::Continue) => {}
                Ok(SyncAction::ReorgHandled) => {
                    self.cell_cache.clear();
                    self.udt_cell_cache.clear();
                    let (reorg_tip, _) = self.repo.get_sync_tip().await?;
                    self.reconcile_hodl_tracker_with_tip(reorg_tip)?;
                    let new_epoch = bump_pipeline_reset_epoch(&self.pipeline_reset_epoch);
                    info!(
                        epoch = new_epoch,
                        reorg_tip,
                        "Reorg handled, caches cleared, HODL tracker reconciled, epoch bumped, continuing sync from fork point"
                    );
                }
                Ok(SyncAction::DeepForkPaused) => {
                    warn!("Deep fork detected, sync paused");
                    sleep(Duration::from_secs(30)).await;
                }
                Err(e) => {
                    let incident_id =
                        self.report_incident("sync_batch_failed", format!("error={:?}", e));
                    error!(
                        run_id = %self.run_id,
                        incident_id = %incident_id,
                        error = ?e,
                        "Sync error"
                    );
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
