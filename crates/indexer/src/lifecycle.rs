use anyhow::Result;
use thiserror::Error;

use ckbadger_store::types::BulkBuildSessionMarker;
use ckbadger_store::CkbadgerStore;

/// Dedicated process exit code for a persistent state that requires an
/// operator-authorized purge and rebuild. The supervisor must not retry it as
/// a transient crash.
pub const REBUILD_REQUIRED_EXIT_CODE: u8 = 78;

#[derive(Debug, Error)]
#[error("{message}")]
pub struct RebuildRequiredError {
    message: String,
}

impl RebuildRequiredError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(crate) fn incomplete_bulk_session(marker: &BulkBuildSessionMarker) -> Self {
        Self::new(format!(
            "startup fail-fast: detected incomplete bulk build session \
             (run_id={}, started_at={}, start_block={}). bulk sync is single-shot rebuild only; \
             delete RocksDB and re-sync from genesis",
            marker.run_id, marker.started_at, marker.start_block
        ))
    }

    pub(crate) fn interrupted_bulk_session(
        run_id: &str,
        current_block: u64,
        stage: &str,
        cleanup_errors: &[String],
    ) -> Self {
        let cleanup_context = if cleanup_errors.is_empty() {
            String::new()
        } else {
            format!(" worker cleanup errors: {}.", cleanup_errors.join("; "))
        };
        Self::new(format!(
            "bulk build interrupted by shutdown request \
             (run_id={run_id}, current_block={current_block}, stage={stage}).{cleanup_context} \
             bulk sync is single-shot rebuild only; delete RocksDB and re-sync from genesis"
        ))
    }
}

pub fn is_rebuild_required(error: &anyhow::Error) -> bool {
    error.downcast_ref::<RebuildRequiredError>().is_some()
}

pub(crate) fn fail_fast_if_bulk_build_session_incomplete(store: &CkbadgerStore) -> Result<()> {
    if let Some(marker) = store.get_bulk_build_session_marker()? {
        return Err(RebuildRequiredError::incomplete_bulk_session(&marker).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_required_classification_survives_anyhow_context() {
        let error = anyhow::Error::new(RebuildRequiredError::new("rebuild"))
            .context("indexer startup failed");
        assert!(is_rebuild_required(&error));
    }

    #[test]
    fn incomplete_bulk_preflight_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let marker = BulkBuildSessionMarker {
            run_id: "run-incomplete".to_string(),
            started_at: 1_710_000_000,
            start_block: 0,
        };
        store.set_bulk_build_session_marker(Some(&marker)).unwrap();
        let runtime_before = store.get_runtime_status().unwrap();
        let sync_before = bincode::serialize(&store.get_sync_status().unwrap()).unwrap();

        let error = fail_fast_if_bulk_build_session_incomplete(&store).unwrap_err();

        assert!(is_rebuild_required(&error));
        assert_eq!(store.get_runtime_status().unwrap(), runtime_before);
        assert_eq!(
            bincode::serialize(&store.get_sync_status().unwrap()).unwrap(),
            sync_before
        );
        assert_eq!(store.get_bulk_build_session_marker().unwrap(), Some(marker));
    }
}
