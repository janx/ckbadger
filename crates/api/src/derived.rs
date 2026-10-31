//! The one gate every read of an indexer-derived aggregate goes through
//! (round 3 lane L): the rows are served only when the aggregate's
//! `derive:<name>` meta says `Complete` at the version this API reads. There
//! is no fallback to the scan the aggregate replaced.
//!
//! Where the aggregate IS the response (`/dao/evidence`, `/dao/depositors`),
//! [`require_derived`] answers `503 derive_pending`. Everywhere else the
//! field it feeds is `null` (unknown) until [`is_derived`] holds. Read the
//! meta in the same pinned view as the rows.

use ckbadger_store::derive_ops::{DeriveState, DerivedAggregateId};
use ckbadger_store::CkbadgerStore;

use crate::response::{ApiError, ApiRouteError, DerivePending};

/// Whether `id`'s rows may be served: its meta is `Complete` at the version
/// this API reads. A field fed by the aggregate is `null` while this is
/// `false`.
pub fn is_derived(store: &CkbadgerStore, id: &DerivedAggregateId) -> Result<bool, ApiRouteError> {
    let meta = store.get_derive_meta(id.name).map_err(|e| {
        ApiError::internal(format!("failed to read derive state of {}: {e:#}", id.name))
    })?;
    Ok(meta.is_some_and(|meta| meta.is_complete(id)))
}

/// `Ok` when `id`'s rows may be served from `store` (read in the request's
/// pinned view, like the rows themselves). Otherwise `503 derive_pending`
/// with the derivation's progress, or `500` naming the invariant violation a
/// failed derivation recorded.
pub fn require_derived(
    store: &CkbadgerStore,
    id: &DerivedAggregateId,
) -> Result<(), ApiRouteError> {
    let meta = store.get_derive_meta(id.name).map_err(|e| {
        ApiError::internal(format!("failed to read derive state of {}: {e:#}", id.name))
    })?;
    let pending = |state: &str, rows_done, derived_from_tip| {
        ApiError::derive_pending(DerivePending {
            aggregate: id.name.to_string(),
            state: state.to_string(),
            rows_done,
            derived_from_tip,
        })
    };
    let Some(meta) = meta else {
        return Err(pending("not_started", None, None));
    };
    if meta.version != id.version {
        return Err(pending("outdated", None, None));
    }
    match meta.state {
        DeriveState::Complete { .. } => Ok(()),
        DeriveState::NotStarted => Err(pending("not_started", None, None)),
        DeriveState::Running {
            derived_from_tip,
            rows_done,
            ..
        } => Err(pending("running", Some(rows_done), Some(derived_from_tip))),
        DeriveState::Failed { error } => Err(ApiError::internal(format!(
            "derived aggregate {} failed in the indexer: {error}",
            id.name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use ckbadger_store::derive_ops::{DeriveMeta, DAO_LIVE_AGG};
    use ckbadger_store::StoreBatch;

    fn store_with(meta: Option<DeriveMeta>) -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        if let Some(meta) = meta {
            let mut batch = StoreBatch::new(&store);
            batch.put_derive_meta(DAO_LIVE_AGG.name, &meta);
            batch.commit().unwrap();
        }
        (dir, store)
    }

    fn meta(version: u32, state: DeriveState) -> DeriveMeta {
        DeriveMeta {
            version,
            state,
            run: 1,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn pending_until_complete_then_served() {
        let running = DeriveState::Running {
            derived_from_tip: 50,
            phase: ckbadger_store::derive_ops::RebuildPhase::Folding,
            cursor: None,
            rows_done: 7,
        };
        for (label, seeded, expected_state) in [
            ("missing", None, "not_started"),
            (
                "not started",
                Some(meta(DAO_LIVE_AGG.version, DeriveState::NotStarted)),
                "not_started",
            ),
            (
                "running",
                Some(meta(DAO_LIVE_AGG.version, running.clone())),
                "running",
            ),
            (
                "older rows",
                Some(meta(
                    DAO_LIVE_AGG.version + 1,
                    DeriveState::Complete {
                        derived_from_tip: 1,
                        completed_at_tip: 1,
                        rows_done: 1,
                    },
                )),
                "outdated",
            ),
        ] {
            let (_dir, store) = store_with(seeded);
            let (status, body) = require_derived(&store, &DAO_LIVE_AGG).unwrap_err();
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{label}");
            assert_eq!(body.error, "derive_pending", "{label}");
            let derive = *body.derive.clone().expect("derive progress");
            assert_eq!(derive.state, expected_state, "{label}");
            assert_eq!(derive.aggregate, "dao_live_agg");
            if label == "running" {
                assert_eq!(derive.rows_done, Some(7));
                assert_eq!(derive.derived_from_tip, Some(50));
                let json = serde_json::to_value(&body.0).unwrap();
                assert_eq!(json["derive"]["rowsDone"], 7, "{json}");
            }
        }
        let (_dir, store) = store_with(Some(meta(
            DAO_LIVE_AGG.version,
            DeriveState::Complete {
                derived_from_tip: 1,
                completed_at_tip: 2,
                rows_done: 3,
            },
        )));
        assert!(require_derived(&store, &DAO_LIVE_AGG).is_ok());
    }

    #[test]
    fn a_failed_derivation_is_a_500_naming_its_error() {
        let (_dir, store) = store_with(Some(meta(
            DAO_LIVE_AGG.version,
            DeriveState::Failed {
                error: "invalid DAO lifecycle at outpoint_key=0xa1".to_string(),
            },
        )));
        let (status, body) = require_derived(&store, &DAO_LIVE_AGG).unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            body.message.contains("outpoint_key=0xa1"),
            "{}",
            body.message
        );
        assert!(body.derive.is_none());
    }
}
