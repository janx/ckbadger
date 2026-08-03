use std::fmt;

use anyhow::Result;

use ckbadger_store::types::BulkBuildSessionMarker;
use ckbadger_store::CkbadgerStore;

/// Dedicated process exit code for a persistent state that requires an
/// operator-authorized purge and rebuild. The supervisor must not retry it as
/// a transient crash.
pub const REBUILD_REQUIRED_EXIT_CODE: u8 = 78;

/// Which network's chain stores a rebuild-required error refers to.
///
/// Under the multi-network orchestrator "delete RocksDB and re-sync" is
/// ambiguous — several indexers run side by side. The orchestrator runs one
/// indexer process per network, so the identity is a property of the process
/// and is resolved once, at the entry point that owns the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLocation {
    network: String,
    domain_data_path: String,
    append_only_data_path: String,
}

impl StoreLocation {
    pub fn new(
        network: impl Into<String>,
        domain_data_path: impl Into<String>,
        append_only_data_path: impl Into<String>,
    ) -> Self {
        Self {
            network: network.into(),
            domain_data_path: domain_data_path.into(),
            append_only_data_path: append_only_data_path.into(),
        }
    }
}

impl fmt::Display for StoreLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "network={}, domain_store={}, append_only_store={}",
            self.network, self.domain_data_path, self.append_only_data_path
        )
    }
}

#[derive(Debug)]
pub struct RebuildRequiredError {
    /// What went wrong, without the operator remedy.
    diagnosis: String,
    /// Set at the process entry point, which is the only layer that knows the
    /// network and store paths.
    location: Option<StoreLocation>,
}

impl RebuildRequiredError {
    pub fn new(diagnosis: impl Into<String>) -> Self {
        Self {
            diagnosis: diagnosis.into(),
            location: None,
        }
    }

    /// Name the stores the operator has to delete.
    pub fn with_store_location(mut self, location: StoreLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub(crate) fn incomplete_bulk_session(marker: &BulkBuildSessionMarker) -> Self {
        Self::new(format!(
            "startup fail-fast: detected incomplete bulk build session \
             (run_id={}, started_at={}, start_block={}). bulk sync is single-shot rebuild only",
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
             bulk sync is single-shot rebuild only"
        ))
    }
}

impl fmt::Display for RebuildRequiredError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}; ", self.diagnosis)?;
        match &self.location {
            Some(location) => write!(
                formatter,
                "delete the RocksDB data for {location} and re-sync from genesis"
            ),
            // Unannotated errors keep the historical wording verbatim: only
            // errors that reach an operator via `run_indexer_sync` carry a
            // location, and the rest are asserted on by existing tests.
            None => write!(formatter, "delete RocksDB and re-sync from genesis"),
        }
    }
}

impl std::error::Error for RebuildRequiredError {}

pub fn is_rebuild_required(error: &anyhow::Error) -> bool {
    error.downcast_ref::<RebuildRequiredError>().is_some()
}

/// Name the network and store paths on a rebuild-required error.
///
/// `RebuildRequiredError` is raised deep in the sync engine, which holds no
/// config. This is the single boundary where the process identity is known, so
/// every rebuild-required error is annotated here rather than threading the
/// location through each raise site. Errors of any other kind pass through
/// untouched.
pub fn annotate_rebuild_required(error: anyhow::Error, location: &StoreLocation) -> anyhow::Error {
    if error.downcast_ref::<RebuildRequiredError>().is_none() {
        return error;
    }

    // Preserve the anyhow context chain wrapped around the original error.
    let contexts: Vec<String> = error
        .chain()
        .take_while(|source| source.downcast_ref::<RebuildRequiredError>().is_none())
        .map(|source| source.to_string())
        .collect();
    let rebuild = match error.downcast::<RebuildRequiredError>() {
        Ok(rebuild) => rebuild,
        // `downcast_ref` matched above, so this branch is unreachable. Return the
        // original instead of panicking: this runs while reporting an error.
        Err(original) => return original,
    };

    let mut annotated = anyhow::Error::new(rebuild.with_store_location(location.clone()));
    for context in contexts.into_iter().rev() {
        annotated = annotated.context(context);
    }
    annotated
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

    fn test_location() -> StoreLocation {
        StoreLocation::new(
            "testnet",
            "/srv/ckbadger/testnet/domain",
            "/srv/ckbadger/testnet/ao",
        )
    }

    fn assert_names_location(message: &str) {
        assert!(
            message.contains("network=testnet"),
            "message must name the network: {message}"
        );
        assert!(
            message.contains("domain_store=/srv/ckbadger/testnet/domain"),
            "message must name the domain store path: {message}"
        );
        assert!(
            message.contains("append_only_store=/srv/ckbadger/testnet/ao"),
            "message must name the append-only store path: {message}"
        );
    }

    #[test]
    fn rebuild_required_classification_survives_anyhow_context() {
        let error = anyhow::Error::new(RebuildRequiredError::new("rebuild"))
            .context("indexer startup failed");
        assert!(is_rebuild_required(&error));
    }

    #[test]
    fn interrupted_bulk_session_message_names_network_and_stores() {
        let error =
            RebuildRequiredError::interrupted_bulk_session("run-7", 4_200, "after_bulk_stage", &[])
                .with_store_location(test_location());
        let message = error.to_string();

        assert!(message.contains("run_id=run-7"), "{message}");
        assert!(message.contains("current_block=4200"), "{message}");
        assert!(message.contains("stage=after_bulk_stage"), "{message}");
        assert_names_location(&message);
    }

    #[test]
    fn incomplete_bulk_session_message_names_network_and_stores() {
        let marker = BulkBuildSessionMarker {
            run_id: "run-incomplete".to_string(),
            started_at: 1_710_000_000,
            start_block: 0,
        };
        let error = RebuildRequiredError::incomplete_bulk_session(&marker)
            .with_store_location(test_location());
        let message = error.to_string();

        assert!(message.contains("run_id=run-incomplete"), "{message}");
        assert_names_location(&message);
    }

    #[test]
    fn unlocated_rebuild_required_still_states_the_remedy() {
        let message = RebuildRequiredError::new("something went wrong").to_string();
        assert!(message.contains("something went wrong"), "{message}");
        assert!(
            message.contains("re-sync from genesis"),
            "the remedy must survive without a location: {message}"
        );
    }

    #[test]
    fn annotate_rebuild_required_names_the_stores_and_keeps_classification() {
        let error = anyhow::Error::new(RebuildRequiredError::interrupted_bulk_session(
            "run-9",
            17,
            "after_tracker_reconcile",
            &["worker join failed".to_string()],
        ))
        .context("indexer terminated");

        let annotated = annotate_rebuild_required(error, &test_location());

        assert!(is_rebuild_required(&annotated));
        assert_eq!(annotated.to_string(), "indexer terminated");
        let rendered = format!("{annotated:#}");
        assert!(
            rendered.contains("worker cleanup errors: worker join failed"),
            "{rendered}"
        );
        assert_names_location(&rendered);
    }

    #[test]
    fn annotate_rebuild_required_leaves_unrelated_errors_untouched() {
        let error = anyhow::anyhow!("rpc timeout").context("fetching tip");
        let annotated = annotate_rebuild_required(error, &test_location());

        assert!(!is_rebuild_required(&annotated));
        assert_eq!(format!("{annotated:#}"), "fetching tip: rpc timeout");
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
