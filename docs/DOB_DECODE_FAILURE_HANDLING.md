# DOB Decode Failure Handling — Design Spec

Status: Approved design (2026-07-03), pending implementation plan.

## Goal

- Stop the DOB decode worker from re-attempting the same permanently-un-decodable
  spores on every run — eliminating the wasted CKB-VM / RPC work and the recurring
  identical `WARN skipping DOB spore decode` noise.
- Fix the user-facing bug where the API reports `status: "pending"` ("background
  worker has not processed this spore yet") forever for spores the worker HAS
  processed and definitively failed to decode.
- Solve both with a single mechanism: persist a decode **failure** as a first-class
  outcome alongside decode success.

## Background (root cause)

The background worker (`crates/indexer/src/sync/dob_decode_worker.rs`) scans
`list_undecoded_dob_spores()`, which considers a `dob/*` spore "undecoded" purely
when `CF_DOB_DECODED` has **no** entry for it (`spore_ops.rs:382`, a presence
check). A failed decode writes nothing, so the spore stays "undecoded" and is
re-attempted on every worker run.

Evidence from a real run log (`temp/run/logs/indexer.log`, 13 worker runs over
2026-06-04 → 2026-07-03): 378 `WARN` lines, but only **30 distinct spores**. 29
fail deterministically on every run; 1 (`adb34332…`) is a pure transient RPC blip.
Breakdown of the 29 deterministic failures:

| Count | Reason                                                                                                                                                                                                                                                                                                                                        | Nature                |
| ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| 9     | `cluster entry not found` (cluster_id = `SOLE_SPORES_SENTINEL_COLLECTION` = ASCII `sole_spores_collection__________`) — these are **clusterless "Sole Spores"**: the indexer assigns this sentinel to spores minted with no real cluster (`crates/indexer/src/db/writer/spore.rs:590`), so there is no DOB cluster/decoder metadata to decode | clusterless by design |
| 8     | `cluster description is not valid JSON` (member of a plain, non-DOB cluster)                                                                                                                                                                                                                                                                  | immutable data        |
| 7     | `decoder exited with non-zero code` (7 or 3 — the on-chain decoder binary rejected the spore's DNA/pattern)                                                                                                                                                                                                                                   | immutable data        |
| 5     | `no live cell found in local index for TypeID decoder: 0x11b80e…` — **verified against mainnet: 0 transactions ever, this Type ID was never deployed** (control: another type_id decoder the worker resolves fine has 1 live cell)                                                                                                            | dangling reference    |

The transient category (`failed to fetch spore cell data via RPC`, 13 log lines)
is the spore's own creation-tx fetch failing at the node; 12 of those 13 are
spores that also fail deterministically anyway, only `adb34332…` is purely
transient (the configured node `127.0.0.1:8114` was briefly down). These must
NOT be recorded as permanent — they self-heal on the next run.

None of this is a ckbadger correctness bug; it is bad/dangling on-chain data plus
transient node availability. The worker is right to skip; it is wrong to (a) re-do
the work forever and (b) let the API misreport the state.

## Principle Alignment

- **Single read path** — `CF_DOB_DECODED` becomes the one authoritative source for
  "what happened to this spore's decode" (success or failure), no parallel
  fallback judgement chain.
- **Fail fast with actionable context** — classification uses a **typed error**, not
  fragile matching on error-message strings.
- **Rebuild-cheap** — deterministic failures are remembered only for this DB's
  lifetime; a `purge` + re-sync from genesis re-attempts and re-classifies
  everything, so a dependency that legitimately appears on-chain later self-heals
  on the next rebuild.

## Decisions (locked)

1. **Scope**: fix both the internal waste/noise AND the user-facing status.
2. **Retry policy**: record-and-skip deterministic failures for this DB lifetime;
   never record transient failures (they keep retrying every run); rely on DB
   rebuild to re-attempt.
3. **Storage model**: **Approach B** — unify `CF_DOB_DECODED`'s value into an enum
   `DecodeOutcome::{ Decoded, Failed }` (single CF, single lookup answers the
   question), rather than a separate failure CF.

## Design

### 1. Data model — `crates/ckbadger-store/src/types.rs`

`CF_DOB_DECODED` value type changes from `DobDecodedEntry` to:

```rust
pub enum DecodeOutcome {
    Decoded(DobDecodedEntry),   // existing success payload, unchanged
    Failed(DobDecodeFailure),   // deterministic failures ONLY (never transient)
}

pub struct DobDecodeFailure {
    pub category: DobDecodeFailureCategory,
    pub message: String,   // human-readable detail (the former anyhow chain text)
    pub failed_at: i64,    // epoch seconds
}

// Stable persisted taxonomy. Append new variants at the END only (bincode is
// order-sensitive); never reorder/remove.
pub enum DobDecodeFailureCategory {
    Clusterless,            // clusterless "Sole Spores" (SOLE_SPORES_SENTINEL_COLLECTION)
    ClusterNotFound,        // a real (non-sentinel) cluster_id that is not in the index
    ClusterMetadataInvalid,
    DecoderNotFound,
    DecoderExecutionFailed,
    DnaInvalid,
    Other,
}
```

### 2. Classification — `crates/indexer/src/sync/dob_decode_worker.rs`

Introduce a typed `DobDecodeError` (new `enum`, e.g. in a `dob_decode_error.rs`
sibling module). `decode_single_spore`, `extract_dna_from_spore`, and
`load_decoder_binary` change their return type from `anyhow::Result<_>` to
`Result<_, DobDecodeError>`; each `?` site maps to the correct variant.
`fetch_output_data_by_outpoint` stays `anyhow::Result` (pure RPC helper); its two
callers `.map_err(...)` it into the appropriate **transient** variant.

```rust
enum DobDecodeError {
    // deterministic → recorded as Failed, then skipped
    Clusterless,                                // collection_id == SOLE_SPORES_SENTINEL_COLLECTION
    ClusterNotFound { cluster_id: Vec<u8> },    // a real cluster_id not present in the index
    ClusterMetadataInvalid { detail: String }, // no description / not JSON / missing `dob` / bad decoder ref
    DecoderNotFound { detail: String },         // code_hash OR type_id has no live cell in index
    DecoderExecution { detail: String },        // decoder ran and rejected (non-zero exit etc.)
    DnaInvalid { detail: String },              // molecule/hex parse fail, no DNA in content

    // transient → NOT recorded, keeps retrying next run
    SporeCellFetch(anyhow::Error),              // RPC fetch of the spore's own cell
    DecoderBinaryFetch(anyhow::Error),          // RPC fetch of the decoder binary
    Internal(anyhow::Error),                    // spawn panic, store IO, hash mismatch
}

`decode_single_spore` short-circuits before any lookup: if `cluster_id ==
SOLE_SPORES_SENTINEL_COLLECTION` it returns `Clusterless` immediately (a clean
"clusterless Sole Spore — no DOB cluster to decode" message), instead of doing a
doomed `get_spore(sentinel)` that yields the cryptic `cluster entry not found for
cluster_id=0x736f6c65…`. Other synthetic sentinels (dotbit/did:ckb) are not
`dob/*` and won't reach here; if one ever did it falls through to `ClusterNotFound`
harmlessly.
```

```rust
impl DobDecodeError {
    fn is_transient(&self) -> bool {
        matches!(self, Self::SporeCellFetch(_) | Self::DecoderBinaryFetch(_) | Self::Internal(_))
    }
    // Only called for deterministic variants when building the persisted record.
    fn category(&self) -> DobDecodeFailureCategory { /* variant → category */ }
    // Display impl provides `message` for the persisted record and API `issues`.
}
```

Rationale for the mixed helpers: `extract_dna_from_spore` mixes a transient RPC
fetch with deterministic molecule/DNA parsing; `load_decoder_binary` mixes a
transient RPC fetch with a deterministic "no live cell" resolution — so the typed
error must reach into these helpers, not just the top level.

This makes `adb34332…` (only ever `SporeCellFetch`) classify transient → not
recorded → succeeds next run. The 12 "transient-and-also-deterministic" spores get
recorded the first run where RPC succeeds far enough to hit their real failure.

### 3. Store access layer

`crates/ckbadger-store/src/batch.rs`:

- `put_dob_decoded(spore_id, &DobDecodedEntry)` — unchanged signature; internally
  wraps in `DecodeOutcome::Decoded` before serialize.
- **new** `put_dob_decode_failure(spore_id, &DobDecodeFailure)` — serializes
  `DecodeOutcome::Failed`.
- `delete_dob_decoded(spore_id)` — unchanged (deletes the key regardless of variant).

`crates/ckbadger-store/src/spore_ops.rs`:

- **new** `get_dob_decode_outcome(spore_id) -> anyhow::Result<Option<DecodeOutcome>>`
  — full outcome.
- `get_dob_decoded(spore_id) -> anyhow::Result<Option<DobDecodedEntry>>` — kept as a
  **success-only convenience** (returns `None` for `Failed` or absent), reimplemented
  on top of `get_dob_decode_outcome`. This is why `serve_media` and
  `render_spore_svg` need NO change.
- `put_dob_decoded_direct(spore_id, &DobDecodedEntry)` — wraps in
  `DecodeOutcome::Decoded` (keeps the existing test at `api_spore.rs:592` working);
  optionally add a `put_dob_decode_failure_direct` for tests.
- `list_undecoded_dob_spores` — **unchanged**: it only does a presence check
  (`get_cf(...).is_none()`). A `Failed` (deterministic) entry counts as "processed"
  → skipped; transient is never written → still returned → retried.

### 4. Worker loop

In the results-handling loop, split on the error:

- `e.is_transient()` → keep the existing `warn!("skipping DOB spore decode", …)`
  and drop it (no record) so it retries next run — preserves operator visibility
  of node issues.
- else (deterministic) → collect `(spore_id, DobDecodeFailure { category, message,
failed_at })`, persist via a `put_dob_decode_failure` batch (same per-spore commit
  style as `decoded_results`), and log once at `debug!`. Add a `failed_recorded`
  counter to the end-of-run summary.

Net effect: deterministic failures leave `list_undecoded` after one attempt →
zero recurring WARN, zero repeated CKB-VM/RPC work.

### 5. API — `crates/api/src/routes/spore.rs`

Only `decode_spore` (route `/spore/objects/{spore_id}/decode`) changes: swap
`get_dob_decoded` → `get_dob_decode_outcome`, and match three arms:

- `Some(DecodeOutcome::Decoded(entry))` → existing `status:"decoded"` logic.
- `Some(DecodeOutcome::Failed(f))` → **new** `status:"failed"`, `issues:[f.message]`.
  (Optional: also expose a machine-readable `category` — decide during planning;
  `SporeDobDecodeResponse` currently has `status` + `issues` and needs no struct
  change if we only use `issues`.)
- `None` → existing `status:"pending"` (now truthful: only for not-yet-attempted).

`serve_media` and `render_spore_svg` keep calling `get_dob_decoded` (success-only)
— a `Failed` outcome transparently behaves as `None` → 404, which is correct.

### 6. Frontend

`frontend/app/objects/[sporeId]/client-page.tsx` already renders `issues[]`
generically, so honest failure reasons surface automatically in place of the old
misleading pending text. Optional polish: a distinct "Undecodable" badge when
`status === "failed"`. Update `frontend/__tests__/pages/object-detail.test.tsx` to
cover the failed status.

### 7. Store schema / re-sync impact

`CF_DOB_DECODED`'s value format changes; existing entries (bare `DobDecodedEntry`)
cannot be deserialized as `DecodeOutcome`.

- **Re-sync required: yes** — `ckbadger purge` + re-sync from genesis (the sanctioned
  drop-and-rebuild path). (`CF_DOB_DECODED` is worker-derived, so in principle only
  that CF needs clearing, but no single-CF drop command is exposed — purge is the
  supported route.)
- Update `docs/STORE_SCHEMA.md` `CF_DOB_DECODED` value description
  (`DobDecodedEntry` → `DecodeOutcome`).

### 8. Testing (mandatory)

- **store unit tests**: put `Decoded`/`Failed` → `get_dob_decode_outcome` round-trips;
  `get_dob_decoded` returns `None` for `Failed`; `list_undecoded_dob_spores` skips
  spores with either outcome present.
- **worker unit tests**: a table test for `DobDecodeError::is_transient()` /
  `category()`; a deterministic failure writes a `Failed` record and is NOT returned
  by `list_undecoded_dob_spores` on the next scan; a transient failure writes NO
  record and IS still returned (reuse the existing `wiremock` fixtures).
- **API test** (`crates/api/tests/api_spore.rs`): `Failed` → `status:"failed"` with the
  reason in `issues`; `Decoded` → `"decoded"`; absent → `"pending"`.
- **regression**: a `cluster-not-found` spore returns `"failed"`, not `"pending"`.
- **frontend**: `object-detail.test.tsx` asserts the failed status renders the reason.

## Touch points

- `crates/ckbadger-store/src/types.rs` — `DecodeOutcome`, `DobDecodeFailure`, `DobDecodeFailureCategory`
- `crates/ckbadger-store/src/spore_ops.rs` — `get_dob_decode_outcome`, `get_dob_decoded` (success-only), `put_dob_decoded_direct`
- `crates/ckbadger-store/src/batch.rs` — `put_dob_decoded` (wrap), `put_dob_decode_failure` (new)
- `crates/indexer/src/sync/dob_decode_worker.rs` (+ new `DobDecodeError`) — typed errors, worker record path
- `crates/api/src/routes/spore.rs` — `decode_spore` only
- `frontend/app/objects/[sporeId]/client-page.tsx` (optional badge) + `object-detail.test.tsx`
- `docs/STORE_SCHEMA.md` — CF_DOB_DECODED value note

## Result

- **Behavior change**: deterministic DOB decode failures are persisted once and
  skipped thereafter (no repeated work, no repeated WARN); the decode API reports
  an honest `failed` + reason instead of a perpetual misleading `pending`; transient
  RPC failures keep retrying and self-heal.
- **Re-sync required: yes** (`CF_DOB_DECODED` value format change → purge + re-sync).
- **What to do next**: turn this spec into an implementation plan (store types →
  store ops → worker classification → API → frontend/tests → docs), TDD per the
  testing section.
