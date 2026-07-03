# DOB Decode Failure Handling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist DOB decode _failures_ as a first-class outcome so deterministic failures are attempted once (not every worker run) and the API reports an honest `failed` status instead of a perpetual misleading `pending`.

**Architecture:** Change `CF_DOB_DECODED`'s value type from `DobDecodedEntry` to an enum `DecodeOutcome::{Decoded, Failed}`. The worker classifies each decode error via a typed `DobDecodeError` (`is_transient()`); deterministic errors are written as `Failed` and thereafter skipped by the presence-check in `list_undecoded_dob_spores`, transient (RPC/node) errors are never recorded and keep retrying. The decode API reads the full outcome and surfaces `failed` + reason.

**Tech Stack:** Rust (rocksdb, bincode, serde, anyhow, tokio, axum 0.8), wiremock for worker tests, React 19 + Vitest for frontend.

**Design spec:** `docs/DOB_DECODE_FAILURE_HANDLING.md`

## Global Constraints

- **Store boundary:** `CF_DOB_DECODED` is a **domain** CF written only by the indexer (the worker runs in-process with the indexer). No new CF is added; only its value type changes. API opens the store read-only (secondary).
- **Re-sync required: yes.** The `CF_DOB_DECODED` value format changes; old entries cannot be deserialized. After merging, run `ckbadger purge` + re-sync from genesis.
- **Serde:** response structs use `#[serde(rename_all = "camelCase")]`. Axum 0.8 routes use `{id}`. API route prefix is `/api/v1`.
- **Bincode enums are order-sensitive:** append new enum variants at the END only; never reorder/remove `DobDecodeFailureCategory` or `DecodeOutcome` variants.
- **Testing is mandatory** for every change (project rule). Bug-fix behavior gets a regression test.
- **No fragile error-string matching** for classification — use the typed `DobDecodeError`.
- Fail fast: `saturating_sub`/`unwrap_or(0)`/silent guards are forbidden on correctness paths.

---

### Task 1: Store outcome types

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs` (add types after `DobDecodedEntry`, ~line 490; add tests to the existing `#[cfg(test)] mod tests` at ~line 1638)

**Interfaces:**

- Produces:
  - `pub enum DecodeOutcome { Decoded(DobDecodedEntry), Failed(DobDecodeFailure) }`
  - `pub struct DobDecodeFailure { pub category: DobDecodeFailureCategory, pub message: String, pub failed_at: i64 }`
  - `pub enum DobDecodeFailureCategory { Clusterless, ClusterNotFound, ClusterMetadataInvalid, DecoderNotFound, DecoderExecutionFailed, DnaInvalid, Other }` (see Step 3 for the authoritative definition with doc comments)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/ckbadger-store/src/types.rs`:

```rust
#[test]
fn test_decode_outcome_bincode_roundtrip() {
    let decoded = DecodeOutcome::Decoded(DobDecodedEntry {
        steps: vec![],
        media_sources: vec![],
        decoded_at: 1_700_000_000,
    });
    let bytes = bincode::serialize(&decoded).unwrap();
    match bincode::deserialize::<DecodeOutcome>(&bytes).unwrap() {
        DecodeOutcome::Decoded(e) => assert_eq!(e.decoded_at, 1_700_000_000),
        DecodeOutcome::Failed(_) => panic!("expected Decoded"),
    }

    let failed = DecodeOutcome::Failed(DobDecodeFailure {
        category: DobDecodeFailureCategory::DecoderExecutionFailed,
        message: "decoder exited with non-zero code: 7".to_string(),
        failed_at: 1_700_000_001,
    });
    let bytes = bincode::serialize(&failed).unwrap();
    match bincode::deserialize::<DecodeOutcome>(&bytes).unwrap() {
        DecodeOutcome::Failed(f) => {
            assert_eq!(f.category, DobDecodeFailureCategory::DecoderExecutionFailed);
            assert_eq!(f.message, "decoder exited with non-zero code: 7");
            assert_eq!(f.failed_at, 1_700_000_001);
        }
        DecodeOutcome::Decoded(_) => panic!("expected Failed"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-store test_decode_outcome_bincode_roundtrip`
Expected: FAIL — `cannot find type DecodeOutcome`.

- [ ] **Step 3: Add the types**

Insert after `DobDecodedEntry` (after ~line 490) in `crates/ckbadger-store/src/types.rs`:

```rust
/// Persisted outcome of a DOB decode attempt for one spore.
///
/// Stored in `CF_DOB_DECODED`. A `Failed` value is written only for
/// deterministic failures (bad/dangling on-chain data or a decoder that
/// rejected immutable DNA); transient RPC/node errors are never persisted so
/// they keep retrying on the next worker run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecodeOutcome {
    Decoded(DobDecodedEntry),
    Failed(DobDecodeFailure),
}

/// A recorded deterministic DOB decode failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodeFailure {
    pub category: DobDecodeFailureCategory,
    /// Human-readable detail (surfaced to the API `issues` list).
    pub message: String,
    /// Epoch seconds when the failure was recorded.
    pub failed_at: i64,
}

/// Stable taxonomy of deterministic decode failures.
///
/// Bincode-serialized: only append new variants at the END.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DobDecodeFailureCategory {
    /// Clusterless "Sole Spores" — collection_id is SOLE_SPORES_SENTINEL_COLLECTION,
    /// so there is no DOB cluster/decoder to decode against.
    Clusterless,
    /// A real (non-sentinel) cluster_id that is not present in the index.
    ClusterNotFound,
    /// Cluster exists but its metadata is unusable (no description, not JSON,
    /// missing `dob` field, or an invalid decoder reference).
    ClusterMetadataInvalid,
    /// Referenced decoder cell (code_hash or type_id) has no live cell.
    DecoderNotFound,
    /// The decoder binary ran and rejected the spore (non-zero exit, etc.).
    DecoderExecutionFailed,
    /// The spore's on-chain content could not yield valid DNA.
    DnaInvalid,
    /// Any other deterministic failure.
    Other,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-store test_decode_outcome_bincode_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ckbadger-store/src/types.rs
git commit -m "feat(store): add DecodeOutcome/DobDecodeFailure DOB decode types"
```

---

### Task 2: Store read/write for outcomes + schema doc

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs:952-959` (wrap `put_dob_decoded`, add `put_dob_decode_failure`)
- Modify: `crates/ckbadger-store/src/spore_ops.rs:65-72` (`put_dob_decoded_direct` wrap), `:337-345` (add `get_dob_decode_outcome`, reimplement `get_dob_decoded`); tests in `#[cfg(test)] mod tests` at `:477`
- Modify: `docs/STORE_SCHEMA.md` (CF_DOB_DECODED value description)

**Interfaces:**

- Consumes: `DecodeOutcome`, `DobDecodeFailure` (Task 1).
- Produces:
  - `StoreBatch::put_dob_decoded(&mut self, spore_id: &[u8], entry: &DobDecodedEntry)` (unchanged signature; wraps `Decoded`)
  - `StoreBatch::put_dob_decode_failure(&mut self, spore_id: &[u8], failure: &DobDecodeFailure)`
  - `CkbadgerStore::get_dob_decode_outcome(&self, spore_id: &[u8]) -> anyhow::Result<Option<DecodeOutcome>>`
  - `CkbadgerStore::get_dob_decoded(&self, spore_id: &[u8]) -> anyhow::Result<Option<DobDecodedEntry>>` (now success-only)

- [ ] **Step 1: Write the failing tests**

Add to `#[cfg(test)] mod tests` in `crates/ckbadger-store/src/spore_ops.rs`. (Mirror existing tests in this module for imports; they use `crate::CkbadgerStore` and `tempfile`.)

```rust
#[test]
fn test_dob_outcome_read_write_and_undecoded_skip() {
    use crate::batch::StoreBatch;
    use crate::types::{
        DobDecodeFailure, DobDecodeFailureCategory, DobDecodedEntry, ObjectEntry, ObjectExtra,
        ObjectStandard, SporeMediaProfile, CompositionTier,
    };

    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

    // Two dob/0 spores so list_undecoded has candidates.
    let mk = |content: &str| ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(vec![0x11; 32]),
        token_id: None,
        owner_lock_hash: Some(vec![0x33; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: vec![0x44; 32],
        extra: ObjectExtra::Spore {
            content_type: content.to_string(),
            content_length: 3,
            media_profile: SporeMediaProfile {
                tier: CompositionTier::PureCkb,
                sources: vec![],
                issues: vec![],
            },
        },
    };
    let decoded_id = [0xAA_u8; 32];
    let failed_id = [0xBB_u8; 32];
    store.put_spore_direct(&decoded_id, &mk("dob/0")).unwrap();
    store.put_spore_direct(&failed_id, &mk("dob/0")).unwrap();

    // Write one Decoded, one Failed.
    let mut b = StoreBatch::new(&store);
    b.put_dob_decoded(
        &decoded_id,
        &DobDecodedEntry { steps: vec![], media_sources: vec![], decoded_at: 1 },
    );
    b.put_dob_decode_failure(
        &failed_id,
        &DobDecodeFailure {
            category: DobDecodeFailureCategory::ClusterNotFound,
            message: "cluster entry not found".to_string(),
            failed_at: 2,
        },
    );
    b.commit().unwrap();

    // get_dob_decode_outcome returns the right variant.
    match store.get_dob_decode_outcome(&decoded_id).unwrap().unwrap() {
        crate::types::DecodeOutcome::Decoded(e) => assert_eq!(e.decoded_at, 1),
        _ => panic!("expected Decoded"),
    }
    match store.get_dob_decode_outcome(&failed_id).unwrap().unwrap() {
        crate::types::DecodeOutcome::Failed(f) => {
            assert_eq!(f.category, DobDecodeFailureCategory::ClusterNotFound)
        }
        _ => panic!("expected Failed"),
    }

    // get_dob_decoded is success-only.
    assert!(store.get_dob_decoded(&decoded_id).unwrap().is_some());
    assert!(store.get_dob_decoded(&failed_id).unwrap().is_none());

    // list_undecoded skips BOTH (decoded and failed count as processed).
    let undecoded = store.list_undecoded_dob_spores(100, None).unwrap();
    assert!(undecoded.iter().all(|(k, _, _)| k != &decoded_id.to_vec()));
    assert!(undecoded.iter().all(|(k, _, _)| k != &failed_id.to_vec()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-store test_dob_outcome_read_write_and_undecoded_skip`
Expected: FAIL — `no method named put_dob_decode_failure` / `get_dob_decode_outcome`.

- [ ] **Step 3: Update batch writers**

In `crates/ckbadger-store/src/batch.rs`, replace the body of `put_dob_decoded` (952-955) and add `put_dob_decode_failure`:

```rust
    pub fn put_dob_decoded(&mut self, spore_id: &[u8], entry: &crate::types::DobDecodedEntry) {
        let outcome = crate::types::DecodeOutcome::Decoded(entry.clone());
        let value = bincode::serialize(&outcome).expect("serialize DecodeOutcome::Decoded");
        self.put_cf(self.store.cf_dob_decoded(), spore_id, &value);
    }

    pub fn put_dob_decode_failure(
        &mut self,
        spore_id: &[u8],
        failure: &crate::types::DobDecodeFailure,
    ) {
        let outcome = crate::types::DecodeOutcome::Failed(failure.clone());
        let value = bincode::serialize(&outcome).expect("serialize DecodeOutcome::Failed");
        self.put_cf(self.store.cf_dob_decoded(), spore_id, &value);
    }
```

- [ ] **Step 4: Update store reads + direct put**

In `crates/ckbadger-store/src/spore_ops.rs`, change `put_dob_decoded_direct` (65-72) to wrap:

```rust
    pub fn put_dob_decoded_direct(
        &self,
        spore_id: &[u8],
        entry: &crate::types::DobDecodedEntry,
    ) -> anyhow::Result<()> {
        let outcome = crate::types::DecodeOutcome::Decoded(entry.clone());
        let value = bincode::serialize(&outcome)?;
        self.put_cf(self.cf_dob_decoded(), spore_id, &value)
    }
```

Replace `get_dob_decoded` (337-345) with an outcome reader plus a success-only wrapper:

```rust
    pub fn get_dob_decode_outcome(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Option<crate::types::DecodeOutcome>> {
        match self.get_cf(self.cf_dob_decoded(), spore_id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Success-only convenience: returns `Some` only for a `Decoded` outcome.
    /// A `Failed` outcome (or absence) returns `None`.
    pub fn get_dob_decoded(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Option<crate::types::DobDecodedEntry>> {
        Ok(self
            .get_dob_decode_outcome(spore_id)?
            .and_then(|o| match o {
                crate::types::DecodeOutcome::Decoded(e) => Some(e),
                crate::types::DecodeOutcome::Failed(_) => None,
            }))
    }
```

`list_undecoded_dob_spores` (351-396) is **unchanged** — its `get_cf(...).is_none()` presence check (line 382) already treats any stored outcome as "processed".

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p ckbadger-store test_dob_outcome_read_write_and_undecoded_skip`
Expected: PASS.

- [ ] **Step 6: Update the schema doc**

In `docs/STORE_SCHEMA.md`, find the `CF_DOB_DECODED` row/section and change its value description from `DobDecodedEntry` to:
`DecodeOutcome (Decoded(DobDecodedEntry) | Failed(DobDecodeFailure))`. Add a one-line note: "Failed is written only for deterministic failures; transient RPC failures are not persisted."

- [ ] **Step 7: Commit**

```bash
git add crates/ckbadger-store/src/batch.rs crates/ckbadger-store/src/spore_ops.rs docs/STORE_SCHEMA.md
git commit -m "feat(store): read/write DecodeOutcome; get_dob_decoded stays success-only"
```

---

### Task 3: Typed decode error + classification

**Files:**

- Create: `crates/indexer/src/sync/dob_decode_error.rs`
- Modify: `crates/indexer/src/sync/mod.rs` (add `mod dob_decode_error;`)

**Interfaces:**

- Consumes: `DobDecodeFailureCategory` (Task 1).
- Produces:
  - `pub(crate) enum DobDecodeError { ClusterNotFound{cluster_id:Vec<u8>}, ClusterMetadataInvalid{detail:String}, DecoderNotFound{detail:String}, DecoderExecution{detail:String}, DnaInvalid{detail:String}, SporeCellFetch(anyhow::Error), DecoderBinaryFetch(anyhow::Error), Internal(anyhow::Error) }`
  - `impl DobDecodeError { pub fn is_transient(&self) -> bool; pub fn category(&self) -> DobDecodeFailureCategory; }`
  - `impl std::fmt::Display for DobDecodeError` (used for both the log line and the persisted `message`)

- [ ] **Step 1: Write the failing test**

Create `crates/indexer/src/sync/dob_decode_error.rs` with tests at the bottom:

```rust
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
        assert!(!DobDecodeError::ClusterNotFound { cluster_id: vec![1] }.is_transient());
        assert!(!DobDecodeError::ClusterMetadataInvalid { detail: "x".into() }.is_transient());
        assert!(!DobDecodeError::DecoderNotFound { detail: "x".into() }.is_transient());
        assert!(!DobDecodeError::DecoderExecution { detail: "x".into() }.is_transient());
        assert!(!DobDecodeError::DnaInvalid { detail: "x".into() }.is_transient());
    }

    #[test]
    fn test_category_mapping() {
        assert_eq!(DobDecodeError::Clusterless.category(), Cat::Clusterless);
        assert_eq!(
            DobDecodeError::ClusterNotFound { cluster_id: vec![1] }.category(),
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
        let e = DobDecodeError::DecoderExecution { detail: "decoder exited with non-zero code: 7".into() };
        assert!(e.to_string().contains("non-zero code: 7"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer dob_decode_error`
Expected: FAIL — module/type not found.

- [ ] **Step 3: Implement the error type**

Prepend to `crates/indexer/src/sync/dob_decode_error.rs` (above the test module):

```rust
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
                write!(f, "clusterless spore (Sole Spores) — no DOB cluster to decode")
            }
            DobDecodeError::ClusterNotFound { cluster_id } => {
                write!(f, "cluster entry not found for cluster_id=0x{}", hex::encode(cluster_id))
            }
            DobDecodeError::ClusterMetadataInvalid { detail } => write!(f, "{detail}"),
            DobDecodeError::DecoderNotFound { detail } => write!(f, "{detail}"),
            DobDecodeError::DecoderExecution { detail } => write!(f, "{detail}"),
            DobDecodeError::DnaInvalid { detail } => write!(f, "{detail}"),
            DobDecodeError::SporeCellFetch(e) => write!(f, "failed to fetch spore cell data via RPC: {e}"),
            DobDecodeError::DecoderBinaryFetch(e) => write!(f, "failed to fetch decoder binary: {e}"),
            DobDecodeError::Internal(e) => write!(f, "{e}"),
        }
    }
}
```

Add the module declaration in `crates/indexer/src/sync/mod.rs` (alongside the other `mod` lines):

```rust
mod dob_decode_error;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-indexer dob_decode_error`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/sync/dob_decode_error.rs crates/indexer/src/sync/mod.rs
git commit -m "feat(indexer): typed DobDecodeError with transient/deterministic classification"
```

---

### Task 4: Thread typed errors through the decode functions

**Files:**

- Modify: `crates/indexer/src/sync/dob_decode_worker.rs` — `decode_single_spore` (447-556), `extract_dna_from_spore` (671-720), `load_decoder_binary` (559-663). Add a classification test to the existing `#[cfg(test)] mod tests` (~line 952).

**Interfaces:**

- Consumes: `DobDecodeError` (Task 3).
- Produces:
  - `async fn decode_single_spore(...) -> Result<DobDecodedEntry, DobDecodeError>`
  - `async fn extract_dna_from_spore(...) -> Result<String, DobDecodeError>`
  - `async fn load_decoder_binary(...) -> Result<Vec<u8>, DobDecodeError>`
  - (`fetch_output_data_by_outpoint` stays `anyhow::Result<Vec<u8>>`, unchanged)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `crates/indexer/src/sync/dob_decode_worker.rs`:

```rust
#[tokio::test]
async fn test_decode_single_spore_cluster_not_found_is_deterministic() {
    use super::super::dob_decode_error::DobDecodeError;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    let cache_dir = dir.path().join("decoder-cache");
    let decoder_cache = Arc::new(DecoderBinaryCache::new(&cache_dir).unwrap());
    let media_store = Arc::new(MediaBlobStore::new(dir.path().join("media")));
    let ctx = DecodeContext {
        store: store.clone(),
        append_only_store: store.clone(),
        decoder_cache,
        media_store,
        rpc_client: CkbRpcClient::new("http://localhost:9999"),
    };

    // collection_id points to a cluster that does not exist -> ClusterNotFound.
    let spore_id = [0x22u8; 32];
    let missing_cluster = vec![0x99u8; 32];
    let err = decode_single_spore(&spore_id, "dob/0", Some(&missing_cluster), &ctx)
        .await
        .unwrap_err();
    assert!(!err.is_transient(), "cluster-not-found must be deterministic");
    assert!(matches!(err, DobDecodeError::ClusterNotFound { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer test_decode_single_spore_cluster_not_found_is_deterministic`
Expected: FAIL — `decode_single_spore` still returns `anyhow::Result` (no `is_transient`).

- [ ] **Step 3: Rewrite `decode_single_spore`**

In `crates/indexer/src/sync/dob_decode_worker.rs`, add the imports near the top (with the other `use` lines):

```rust
use crate::sync::dob_decode_error::DobDecodeError;
use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
```

Replace `decode_single_spore` (447-556) with:

```rust
async fn decode_single_spore(
    spore_id: &[u8],
    content_type: &str,
    collection_id: Option<&[u8]>,
    ctx: &DecodeContext,
) -> Result<DobDecodedEntry, DobDecodeError> {
    let cluster_id = collection_id.ok_or_else(|| DobDecodeError::ClusterMetadataInvalid {
        detail: "DOB spore has no collection_id — cannot resolve cluster metadata".to_string(),
    })?;

    // Clusterless "Sole Spores": the indexer assigns this sentinel to spores with
    // no real cluster. There is no DOB cluster/decoder to decode — short-circuit
    // with a precise reason instead of a doomed get_spore(sentinel) lookup.
    if cluster_id == SOLE_SPORES_SENTINEL_COLLECTION {
        return Err(DobDecodeError::Clusterless);
    }

    let cluster_entry = ctx
        .store
        .get_spore(cluster_id)
        .map_err(DobDecodeError::Internal)?
        .ok_or_else(|| DobDecodeError::ClusterNotFound {
            cluster_id: cluster_id.to_vec(),
        })?;

    let cluster_description =
        cluster_entry
            .description
            .as_deref()
            .ok_or_else(|| DobDecodeError::ClusterMetadataInvalid {
                detail: "cluster entry has no description".to_string(),
            })?;

    let metadata: Value =
        serde_json::from_str(cluster_description).map_err(|e| DobDecodeError::ClusterMetadataInvalid {
            detail: format!("cluster description is not valid JSON: {e}"),
        })?;

    let dna_hex = extract_dna_from_spore(spore_id, &ctx.store, &ctx.rpc_client).await?;

    let dob_obj = metadata
        .get("dob")
        .ok_or_else(|| DobDecodeError::ClusterMetadataInvalid {
            detail: "cluster metadata missing 'dob' field".to_string(),
        })?;

    let decoder_steps =
        parse_decoder_steps(dob_obj).map_err(|e| DobDecodeError::ClusterMetadataInvalid {
            detail: e.to_string(),
        })?;
    let mut resolved_steps = Vec::with_capacity(decoder_steps.len());
    for step in decoder_steps {
        let binary = load_decoder_binary(&step.decoder_ref, ctx).await?;
        resolved_steps.push(ResolvedDecoderStep {
            binary,
            pattern_json: step.pattern_json,
        });
    }

    let dob_version = parse_dob_version(content_type);

    let decoded = tokio::task::spawn_blocking(move || match dob_version {
        0 => {
            let Some(first_step) = resolved_steps.first() else {
                return Err(anyhow::anyhow!("decoder chain is empty"));
            };
            ckbadger_dob_decoder::decode_dob0(&first_step.binary, &dna_hex, &first_step.pattern_json)
        }
        1 => {
            let decoders: Vec<(&[u8], &str)> = resolved_steps
                .iter()
                .map(|step| (step.binary.as_slice(), step.pattern_json.as_str()))
                .collect();
            ckbadger_dob_decoder::decode_dob1_chain(&decoders, &dna_hex)
        }
        v => Err(anyhow::anyhow!("unsupported DOB version: {v}")),
    })
    .await
    .map_err(|e| DobDecodeError::Internal(anyhow::anyhow!("CKB-VM spawn_blocking panicked: {e}")))?
    .map_err(|e| DobDecodeError::DecoderExecution { detail: e.to_string() })?;

    let coll_id = collection_id.expect("collection_id guaranteed by earlier check");

    let mut steps = Vec::with_capacity(decoded.step_outputs.len());
    let mut all_traits = Vec::new();

    for step_output in &decoded.step_outputs {
        let raw_bytes = step_output.raw_output.as_bytes();
        let media_type = sniff_media_type(raw_bytes);
        let hash = ctx
            .media_store
            .write(coll_id, raw_bytes)
            .map_err(DobDecodeError::Internal)?;

        let traits: Vec<DobDecodedTrait> = step_output
            .traits
            .iter()
            .map(|t| DobDecodedTrait {
                name: t.name.clone(),
                value: format_trait_value(&t.value),
            })
            .collect();

        all_traits.extend(step_output.traits.iter().cloned());

        steps.push(DobDecodedStep {
            step: step_output.step,
            media_type: media_type.to_string(),
            size: raw_bytes.len() as u64,
            hash,
            traits,
        });
    }

    let media_sources = extract_media_sources_from_traits(&all_traits);

    Ok(DobDecodedEntry {
        steps,
        media_sources,
        decoded_at: chrono::Utc::now().timestamp(),
    })
}
```

- [ ] **Step 4: Rewrite `extract_dna_from_spore`**

Replace `extract_dna_from_spore` (671-720) with:

```rust
async fn extract_dna_from_spore(
    spore_id: &[u8],
    store: &CkbadgerStore,
    rpc_client: &CkbRpcClient,
) -> Result<String, DobDecodeError> {
    let outpoints = store
        .list_spore_outpoints_by_spore_id(spore_id)
        .map_err(DobDecodeError::Internal)?;

    let (tx_hash, output_index) = outpoints.first().ok_or_else(|| {
        DobDecodeError::Internal(anyhow::anyhow!(
            "no outpoint found for spore_id=0x{}",
            hex::encode(spore_id)
        ))
    })?;

    // Transient: RPC fetch of the spore's own creation tx.
    let output_data = fetch_output_data_by_outpoint(tx_hash, *output_index, rpc_client)
        .await
        .map_err(DobDecodeError::SporeCellFetch)?;

    // Deterministic: the on-chain content is immutable.
    let content_bytes = SporeParser::parse_spore_content_from_data(&output_data).map_err(|e| {
        DobDecodeError::DnaInvalid {
            detail: format!("failed to parse Spore molecule content: {e}"),
        }
    })?;

    let content_text = String::from_utf8_lossy(&content_bytes);
    parse_dna_hex_from_content_text(&content_text).map_err(|e| DobDecodeError::DnaInvalid {
        detail: format!("failed to extract DNA hex from spore content: {e}"),
    })
}
```

- [ ] **Step 5: Rewrite `load_decoder_binary`**

Replace `load_decoder_binary` (559-663) with (both branches mapped to typed errors):

```rust
async fn load_decoder_binary(
    decoder_ref: &DecoderRef,
    ctx: &DecodeContext,
) -> Result<Vec<u8>, DobDecodeError> {
    match decoder_ref {
        DecoderRef::CodeHash(code_hash) => {
            let cache_key = DecoderBinaryCache::code_hash_key(code_hash);
            if let Some(binary) = ctx.decoder_cache.get(&cache_key) {
                return Ok(Arc::try_unwrap(binary).unwrap_or_else(|arc| (*arc).clone()));
            }

            let (tx_hash, output_index, _) = ctx
                .store
                .find_any_cell_by_data_hash(code_hash, ctx.append_only_store.as_ref())
                .map_err(DobDecodeError::Internal)?
                .ok_or_else(|| DobDecodeError::DecoderNotFound {
                    detail: format!(
                        "decoder code cell missing from local data-hash index: code_hash=0x{}",
                        hex::encode(code_hash)
                    ),
                })?;

            let binary = fetch_output_data_by_outpoint(&tx_hash, output_index, &ctx.rpc_client)
                .await
                .map_err(DobDecodeError::DecoderBinaryFetch)?;

            verify_blake2b_hash(&binary, code_hash).map_err(|e| {
                DobDecodeError::Internal(anyhow::anyhow!(
                    "resolved decoder binary hash mismatch: code_hash=0x{}: {e}",
                    hex::encode(code_hash)
                ))
            })?;

            ctx.decoder_cache
                .put(&cache_key, &binary)
                .map_err(DobDecodeError::Internal)?;

            Ok(binary)
        }
        DecoderRef::TypeId(type_id_hash) => {
            let cache_key = DecoderBinaryCache::type_id_key(type_id_hash);
            if let Some(binary) = ctx.decoder_cache.get(&cache_key) {
                return Ok(Arc::try_unwrap(binary).unwrap_or_else(|arc| (*arc).clone()));
            }

            let type_id_code_hash =
                hex::decode(crate::parser::script::TYPE_ID_CODE_HASH).expect("valid hex constant");
            let type_script_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
                &type_id_code_hash,
                1,
                type_id_hash,
            );

            let cells = ctx
                .store
                .list_cells_by_type(&type_script_hash, 1, None, ctx.append_only_store.as_ref())
                .map_err(DobDecodeError::Internal)?;

            let (tx_hash, output_index, _) = cells.into_iter().next().ok_or_else(|| {
                DobDecodeError::DecoderNotFound {
                    detail: format!(
                        "no live cell found in local index for TypeID decoder: type_id=0x{}",
                        hex::encode(type_id_hash)
                    ),
                }
            })?;

            let binary = fetch_output_data_by_outpoint(&tx_hash, output_index, &ctx.rpc_client)
                .await
                .map_err(DobDecodeError::DecoderBinaryFetch)?;

            ctx.decoder_cache
                .put(&cache_key, &binary)
                .map_err(DobDecodeError::Internal)?;

            Ok(binary)
        }
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ckbadger-indexer test_decode_single_spore_cluster_not_found_is_deterministic`
Then the existing worker suite: `cargo test -p ckbadger-indexer dob_decode_worker`
Expected: PASS (new test + pre-existing `test_load_decoder_binary_resolves_code_hash_via_local_data_hash_index`, `test_extract_dna_from_spore_no_outpoints`, etc. still green — note `test_extract_dna_from_spore_no_outpoints` now asserts an error; update its assertion if it checked message text: it should now assert `err.is_transient()` is `true` because "no outpoint" maps to `Internal`).

If `test_extract_dna_from_spore_no_outpoints` asserted on `.contains("no outpoint found")`, replace that assertion with:

```rust
    assert!(result.is_err());
    assert!(result.unwrap_err().is_transient(), "no-outpoint maps to transient Internal");
```

- [ ] **Step 7: Commit**

```bash
git add crates/indexer/src/sync/dob_decode_worker.rs
git commit -m "refactor(indexer): decode functions return typed DobDecodeError"
```

---

### Task 5: Worker records deterministic failures, skips transient

**Files:**

- Modify: `crates/indexer/src/sync/dob_decode_worker.rs` — the results loop in `run` (162-213). Add an integration test to `#[cfg(test)] mod tests`.

**Interfaces:**

- Consumes: `put_dob_decode_failure` (Task 2), `DobDecodeError::{is_transient, category}` + `Display` (Task 3/4).

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `dob_decode_worker.rs`. This drives one worker pass over a spore whose cluster is missing (deterministic) and asserts a `Failed` record is written and the spore is no longer listed as undecoded:

```rust
#[tokio::test]
async fn test_worker_records_deterministic_failure_and_stops_relisting() {
    use ckbadger_store::types::{
        DecodeOutcome, ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile, CompositionTier,
    };

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    let decoder_cache = Arc::new(DecoderBinaryCache::new(&dir.path().join("cache")).unwrap());
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker = DobDecodeWorker::new(
        store.clone(),
        store.clone(),
        decoder_cache,
        dir.path().join("media"),
        "http://localhost:9999".to_string(),
        shutdown,
    );

    // A dob/0 spore whose collection points at a non-existent cluster.
    let spore_id = [0x22u8; 32];
    let spore = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(vec![0x99u8; 32]),
        token_id: None,
        owner_lock_hash: Some(vec![0x33; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: vec![0x44; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 3,
            media_profile: SporeMediaProfile {
                tier: CompositionTier::PureCkb,
                sources: vec![],
                issues: vec![],
            },
        },
    };
    store.put_spore_direct(&spore_id, &spore).unwrap();

    assert_eq!(store.list_undecoded_dob_spores(100, None).unwrap().len(), 1);

    worker.run().await.unwrap();

    // A Failed outcome is recorded ...
    match store.get_dob_decode_outcome(&spore_id).unwrap().unwrap() {
        DecodeOutcome::Failed(f) => assert_eq!(
            f.category,
            ckbadger_store::types::DobDecodeFailureCategory::ClusterNotFound
        ),
        DecodeOutcome::Decoded(_) => panic!("expected Failed"),
    }
    // ... and the spore is no longer re-listed for decode.
    assert_eq!(store.list_undecoded_dob_spores(100, None).unwrap().len(), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer test_worker_records_deterministic_failure_and_stops_relisting`
Expected: FAIL — worker currently only `warn!`s and writes nothing, so the spore stays listed.

- [ ] **Step 3: Update the results loop**

In `run`, replace the failure arm of the results `match` (176-184) and the surrounding accounting. Change the collection block (162-185) to also gather deterministic failures:

```rust
            let mut decoded_results: Vec<(Vec<u8>, DobDecodedEntry)> = Vec::new();
            let mut failed_records: Vec<(Vec<u8>, ckbadger_store::types::DobDecodeFailure)> = Vec::new();
            let mut batch_skipped: u64 = 0;

            for (spore_id, result) in results {
                match result {
                    Ok(entry) => {
                        debug!(
                            spore_id = hex::encode(&spore_id),
                            steps = entry.steps.len(),
                            media_sources = entry.media_sources.len(),
                            "decoded DOB spore"
                        );
                        decoded_results.push((spore_id, entry));
                    }
                    Err(e) => {
                        batch_skipped += 1;
                        if e.is_transient() {
                            // Not a data problem — keep it retryable (no record).
                            warn!(
                                spore_id = hex::encode(&spore_id),
                                error = %e,
                                "skipping DOB spore decode (transient — will retry)"
                            );
                        } else {
                            // Deterministic — record once so it is not re-attempted.
                            debug!(
                                spore_id = hex::encode(&spore_id),
                                error = %e,
                                "recording un-decodable DOB spore"
                            );
                            failed_records.push((
                                spore_id,
                                ckbadger_store::types::DobDecodeFailure {
                                    category: e.category(),
                                    message: e.to_string(),
                                    failed_at: chrono::Utc::now().timestamp(),
                                },
                            ));
                        }
                    }
                }
            }
```

Then, after the existing `decoded_results` write loop (after line 210, before `total_decoded += batch_committed;`), persist the failures:

```rust
            for (spore_id, failure) in &failed_records {
                let mut store_batch = StoreBatch::new(&self.store);
                store_batch.put_dob_decode_failure(spore_id, failure);
                store_batch.commit()?;
            }
```

(`batch_skipped` already counts both transient and recorded failures, so `total_skipped` accounting is unchanged.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-indexer test_worker_records_deterministic_failure_and_stops_relisting`
Then: `cargo test -p ckbadger-indexer dob_decode`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/sync/dob_decode_worker.rs
git commit -m "feat(indexer): persist deterministic DOB decode failures, keep transient retryable"
```

---

### Task 6: API surfaces `failed` status with reason

**Files:**

- Modify: `crates/api/src/routes/spore.rs` — `decode_spore` (1130-1266), specifically the `get_dob_decoded` call (1153) and the `match decoded` (1160-1265).
- Test: `crates/api/tests/api_spore.rs` (new test modelled on the existing decode test ~line 560-620).

**Interfaces:**

- Consumes: `get_dob_decode_outcome`, `DecodeOutcome` (Task 2). `serve_media`/`render_spore_svg` keep using `get_dob_decoded` (unchanged).

- [ ] **Step 1: Write the failing test**

Add to `crates/api/tests/api_spore.rs` (mirror the imports/helpers of the existing decode test; it uses `store.put_dob_decoded_direct`, `test_config`, `create_router`, and hits `/api/v1/spore/objects/{hex}/decode`). This test writes a `Failed` outcome and asserts `status:"failed"` with the reason:

```rust
#[tokio::test]
async fn test_decode_endpoint_reports_failed_with_reason() {
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{
        DobDecodeFailure, DobDecodeFailureCategory, ObjectEntry, ObjectExtra, ObjectStandard,
        SporeMediaProfile, CompositionTier,
    };

    let (store, _dir) = open_test_store(); // use the same helper the other tests use
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    // A dob/0 spore with a recorded Failed outcome.
    let spore = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(vec![0x11; 32]),
        token_id: None,
        owner_lock_hash: Some(vec![0x33; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: vec![0x44; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 3,
            media_profile: SporeMediaProfile {
                tier: CompositionTier::PureCkb,
                sources: vec![],
                issues: vec![],
            },
        },
    };
    store.put_spore_direct(&spore_id, &spore).unwrap();
    let mut b = StoreBatch::new(&store);
    b.put_dob_decode_failure(
        &spore_id,
        &DobDecodeFailure {
            category: DobDecodeFailureCategory::ClusterMetadataInvalid,
            message: "cluster description is not valid JSON: expected value at line 1".to_string(),
            failed_at: 1_700_000_000,
        },
    );
    b.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = parse_body(response).await; // use the file's existing body helper
    assert_eq!(body["status"], "failed");
    assert!(body["issues"][0]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}
```

(Adjust `open_test_store`, `parse_body`, and helper names to match the exact ones already used in `api_spore.rs`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api test_decode_endpoint_reports_failed_with_reason`
Expected: FAIL — currently returns `status:"pending"` for a spore with no `Decoded` entry.

- [ ] **Step 3: Update `decode_spore`**

In `crates/api/src/routes/spore.rs`, change the store call at 1153 to fetch the full outcome:

```rust
    let decoded = tokio::task::spawn_blocking(move || store.get_dob_decode_outcome(&id_for_decode))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
```

Change the `match decoded { Some(decoded_entry) => ... , None => ... }` to three arms. Keep the existing decoded-body construction, but bind it to the `Decoded` arm; add a `Failed` arm; keep `None` as pending:

```rust
    match decoded {
        Some(ckbadger_store::DecodeOutcome::Decoded(decoded_entry)) => {
            // ... EXISTING body from lines 1162-1251 unchanged, ending with:
            //     ok(SporeDobDecodeResponse { status: "decoded".to_string(), ... })
        }
        Some(ckbadger_store::DecodeOutcome::Failed(failure)) => ok(SporeDobDecodeResponse {
            status: "failed".to_string(),
            spore_id: spore_id_hex,
            content_type,
            dna_hex: None,
            traits: Vec::new(),
            media: vec![],
            issues: vec![failure.message],
        }),
        None => ok(SporeDobDecodeResponse {
            status: "pending".to_string(),
            spore_id: spore_id_hex,
            content_type,
            dna_hex: None,
            traits: Vec::new(),
            media: vec![],
            issues: vec![
                "DOB decode pending — background worker has not processed this spore yet"
                    .to_string(),
            ],
        }),
    }
```

(Confirm `DecodeOutcome` is exported from the `ckbadger_store` crate root; if it is only under `ckbadger_store::types`, use that path. Add the import if the crate re-exports types at root — mirror how `ObjectExtra` is referenced elsewhere in this file, e.g. `ckbadger_store::ObjectExtra`.)

`serve_media` (1275) and `render_spore_svg` (1373) are **unchanged** — they keep calling `get_dob_decoded`, and a `Failed` outcome transparently yields `None` → their existing not-found behavior.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-api test_decode_endpoint_reports_failed_with_reason`
Then the decode regression already in the file: `cargo test -p ckbadger-api decode`
Expected: PASS (new failed-status test + the existing decoded test still green).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/spore.rs crates/api/tests/api_spore.rs
git commit -m "feat(api): decode endpoint reports failed status with reason instead of perpetual pending"
```

---

### Task 7: Frontend surfaces the failure reason

**Files:**

- Modify (optional polish): `frontend/app/objects/[sporeId]/client-page.tsx` — add an "Undecodable" badge when `decodedDobByApi.status === 'failed'`.
- Test: `frontend/__tests__/pages/object-detail.test.tsx` — add a case for the `failed` status (mirror the existing `vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({...})` cases at ~lines 312/336/380).

**Interfaces:**

- Consumes: the `/decode` API contract from Task 6. `SporeDobDecoded` already has `status: string` and `issues: string[]` (`frontend/lib/api.ts:1017`); the component already renders `issues[]`, so the reason surfaces with no component change.

- [ ] **Step 1: Write the failing test**

Add to `frontend/__tests__/pages/object-detail.test.tsx` (mirror the existing mocked-decode test structure; provide the full `SporeDobDecoded` shape):

```tsx
it('shows the failure reason when DOB decode failed', async () => {
  vi.mocked(api.getSporeObjectDecoded).mockResolvedValue({
    status: 'failed',
    sporeId: '0x9999',
    contentType: 'dob/0',
    dnaHex: null,
    traits: [],
    media: [],
    issues: ['cluster description is not valid JSON: expected value at line 1'],
  });

  // ...render the object detail page the same way the other tests in this file do...

  expect(await screen.findByText(/cluster description is not valid JSON/i)).toBeInTheDocument();
});
```

- [ ] **Step 2: Run test to verify it fails (or passes if generic issues rendering already covers it)**

Run: `cd frontend && npx vitest run object-detail`
Expected: If the issues block is gated behind `hasDecodedTraits` (see `client-page.tsx:1128`), the reason will NOT render for a failed decode with no traits → FAIL. That gap is what Step 3 fixes.

- [ ] **Step 3: Render issues for the failed status**

In `frontend/app/objects/[sporeId]/client-page.tsx`, ensure the failure reason renders even when there are no decoded traits. Add, near the primary DOB content render (around the `hasDecodedTraits` branch ~1066), an always-rendered block when the API reports failure:

```tsx
{
  decodedDobByApi?.status === 'failed' && decodedDobByApi.issues.length > 0 && (
    <div className="mb-4 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm">
      <div className="mb-1 font-medium">Undecodable DOB</div>
      <ul className="list-disc pl-5">
        {decodedDobByApi.issues.map((issue, i) => (
          <li key={i}>{issue}</li>
        ))}
      </ul>
    </div>
  );
}
```

(Match the surrounding Tailwind/token conventions in this file; the exact classes can follow the existing issue-list styling at lines 1103-1109.)

- [ ] **Step 4: Run test + type-check + lint**

Run: `cd frontend && npx vitest run object-detail`
Then: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add "frontend/app/objects/[sporeId]/client-page.tsx" frontend/__tests__/pages/object-detail.test.tsx
git commit -m "feat(frontend): show reason for undecodable DOB spores"
```

---

### Task 8: Full verification + re-sync note

**Files:** none (verification only).

- [ ] **Step 1: Full backend build + lint + tests**

Run:

```bash
cargo check && cargo clippy --all-targets -- -D warnings && cargo test
```

Expected: PASS. Clippy clean.

- [ ] **Step 2: Frontend checks**

Run:

```bash
cd frontend && pnpm type-check && pnpm lint && npx vitest run
```

Expected: PASS.

- [ ] **Step 3: Re-sync (manual, documented)**

Because `CF_DOB_DECODED`'s value format changed, existing DB entries are unreadable. On the target machine:

```bash
ckbadger purge
# then re-run the indexer to sync from genesis; the DOB decode worker
# re-attempts every dob/* spore and records deterministic failures fresh.
```

- [ ] **Step 4: Final commit (if any doc tweaks remain)**

Commit only explicit tracked paths — the repo root has unrelated untracked
dotfiles, so NEVER use `git add -A`/`git add .`:

```bash
git add docs/DOB_DECODE_FAILURE_HANDLING.md docs/STORE_SCHEMA.md
git commit -m "docs: note re-sync for DOB decode outcome schema change" || echo "nothing to commit"
```

---

## Self-Review

**Spec coverage:**

- Data model (`DecodeOutcome`/`DobDecodeFailure`/`DobDecodeFailureCategory`) → Task 1 ✓
- Store access (`put_dob_decoded` wrap, `put_dob_decode_failure`, `get_dob_decode_outcome`, `get_dob_decoded` success-only, `list_undecoded` unchanged) → Task 2 ✓
- Classification (`DobDecodeError` typed, `is_transient`/`category`) → Task 3 ✓
- Worker typed-error threading → Task 4 ✓
- Worker record-deterministic / skip-transient → Task 5 ✓
- API `failed` status → Task 6 ✓
- Frontend reason surfacing → Task 7 ✓
- Store schema doc + re-sync → Task 2 (doc) + Task 8 (re-sync) ✓
- Tests for every layer → each task ✓

**Placeholder scan:** No "TBD/TODO"; every code step shows complete code. The only "match the existing helper name" notes are in Task 6/7 tests where the exact helper identifiers (`open_test_store`, `parse_body`, render harness) live in files not fully reproduced here — the engineer copies the sibling test's setup verbatim.

**Type consistency:** `DecodeOutcome`, `DobDecodeFailure { category, message, failed_at }`, `DobDecodeFailureCategory` variants, `DobDecodeError::{is_transient, category}` names are used identically across Tasks 1-6. `put_dob_decode_failure(spore_id, &DobDecodeFailure)` and `get_dob_decode_outcome -> Option<DecodeOutcome>` signatures match between definition (Task 2) and use (Tasks 5, 6).

**Open decision (deferred from spec, intentionally):** whether to expose `category` as a machine-readable field in `SporeDobDecodeResponse`. Current plan uses only `issues` text; adding a field is a follow-up if desired.
