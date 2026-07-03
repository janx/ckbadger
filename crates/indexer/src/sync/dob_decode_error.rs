//! Typed error for DOB decode attempts.
//!
//! Classification drives retry policy: transient (RPC/node/IO) errors are NOT
//! persisted and keep retrying; deterministic errors are recorded once as a
//! `Failed` outcome and skipped thereafter.

use ckbadger_store::types::DobDecodeFailureCategory;

#[derive(Debug)]
pub(crate) enum DobDecodeError {
    // --- deterministic: recorded then skipped ---
    Clusterless,
    ClusterNotFound { cluster_id: Vec<u8> },
    ClusterMetadataInvalid { detail: String },
    DecoderNotFound { detail: String },
    DecoderExecution { detail: String },
    DnaInvalid { detail: String },
    // --- transient: never recorded, keeps retrying ---
    SporeCellFetch(anyhow::Error),
    DecoderBinaryFetch(anyhow::Error),
    Internal(anyhow::Error),
}

impl DobDecodeError {
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            DobDecodeError::SporeCellFetch(_)
                | DobDecodeError::DecoderBinaryFetch(_)
                | DobDecodeError::Internal(_)
        )
    }

    /// Category for the persisted record. Only meaningful for deterministic
    /// variants (transient variants are never persisted); returns `Other` for
    /// transient variants as a safe default.
    pub fn category(&self) -> DobDecodeFailureCategory {
        match self {
            DobDecodeError::Clusterless => DobDecodeFailureCategory::Clusterless,
            DobDecodeError::ClusterNotFound { .. } => DobDecodeFailureCategory::ClusterNotFound,
            DobDecodeError::ClusterMetadataInvalid { .. } => {
                DobDecodeFailureCategory::ClusterMetadataInvalid
            }
            DobDecodeError::DecoderNotFound { .. } => DobDecodeFailureCategory::DecoderNotFound,
            DobDecodeError::DecoderExecution { .. } => {
                DobDecodeFailureCategory::DecoderExecutionFailed
            }
            DobDecodeError::DnaInvalid { .. } => DobDecodeFailureCategory::DnaInvalid,
            DobDecodeError::SporeCellFetch(_)
            | DobDecodeError::DecoderBinaryFetch(_)
            | DobDecodeError::Internal(_) => DobDecodeFailureCategory::Other,
        }
    }
}

impl std::fmt::Display for DobDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DobDecodeError::Clusterless => {
                write!(
                    f,
                    "clusterless spore (Sole Spores) — no DOB cluster to decode"
                )
            }
            DobDecodeError::ClusterNotFound { cluster_id } => {
                write!(
                    f,
                    "cluster entry not found for cluster_id=0x{}",
                    hex::encode(cluster_id)
                )
            }
            DobDecodeError::ClusterMetadataInvalid { detail } => write!(f, "{detail}"),
            DobDecodeError::DecoderNotFound { detail } => write!(f, "{detail}"),
            DobDecodeError::DecoderExecution { detail } => write!(f, "{detail}"),
            DobDecodeError::DnaInvalid { detail } => write!(f, "{detail}"),
            DobDecodeError::SporeCellFetch(e) => {
                write!(f, "failed to fetch spore cell data via RPC: {e}")
            }
            DobDecodeError::DecoderBinaryFetch(e) => {
                write!(f, "failed to fetch decoder binary: {e}")
            }
            DobDecodeError::Internal(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::DobDecodeFailureCategory as Cat;

    #[test]
    fn test_is_transient_classification() {
        assert!(DobDecodeError::SporeCellFetch(anyhow::anyhow!("x")).is_transient());
        assert!(DobDecodeError::DecoderBinaryFetch(anyhow::anyhow!("x")).is_transient());
        assert!(DobDecodeError::Internal(anyhow::anyhow!("x")).is_transient());

        assert!(!DobDecodeError::Clusterless.is_transient());
        assert!(!DobDecodeError::ClusterNotFound {
            cluster_id: vec![1]
        }
        .is_transient());
        assert!(!DobDecodeError::ClusterMetadataInvalid { detail: "x".into() }.is_transient());
        assert!(!DobDecodeError::DecoderNotFound { detail: "x".into() }.is_transient());
        assert!(!DobDecodeError::DecoderExecution { detail: "x".into() }.is_transient());
        assert!(!DobDecodeError::DnaInvalid { detail: "x".into() }.is_transient());
    }

    #[test]
    fn test_category_mapping() {
        assert_eq!(DobDecodeError::Clusterless.category(), Cat::Clusterless);
        assert_eq!(
            DobDecodeError::ClusterNotFound {
                cluster_id: vec![1]
            }
            .category(),
            Cat::ClusterNotFound
        );
        assert_eq!(
            DobDecodeError::ClusterMetadataInvalid { detail: "x".into() }.category(),
            Cat::ClusterMetadataInvalid
        );
        assert_eq!(
            DobDecodeError::DecoderNotFound { detail: "x".into() }.category(),
            Cat::DecoderNotFound
        );
        assert_eq!(
            DobDecodeError::DecoderExecution { detail: "x".into() }.category(),
            Cat::DecoderExecutionFailed
        );
        assert_eq!(
            DobDecodeError::DnaInvalid { detail: "x".into() }.category(),
            Cat::DnaInvalid
        );
    }

    #[test]
    fn test_display_includes_detail() {
        let e = DobDecodeError::DecoderExecution {
            detail: "decoder exited with non-zero code: 7".into(),
        };
        assert!(e.to_string().contains("non-zero code: 7"));
    }
}
