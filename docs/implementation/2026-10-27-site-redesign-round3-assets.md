# D4 asset read contracts

## Goal

Complete the Object, Collection, DOB, mNFT and identity read contracts used by the approved D4 boards.

## Principle Alignment

- Chain payloads and canonical domain markers are the evidence. An immutable orphan payload is not canonical history.
- Read-only API changes target no persistent store. No indexer, schema, bulk, append-only or network write paths change.
- One cell-version reader supplies all item types; shared entity feeds hydrate whole transactions once.
- DOB traits remain the persisted VM output. Pattern introspection describes declared rules, never creates a second trait calculation.
- Exact all-scope counts and distributions precede paging. Missing or contradictory indexed evidence fails with item/outpoint context.

## Scope and checks

Baseline: api_assets 62 tests and api_spore 28 tests passed. Add regression-first coverage for canonical versions, DID current outpoint, raw protocol decoding, DOB data/filter aggregation and exact mNFT flags. Preserve producer JSON for frontend contract tests.

## Result

Read contracts implemented in three coherent API slices; root integration and real-chain acceptance are recorded separately. Re-sync required: no.

## Read evidence and boundaries

- `cell-versions` validates reverse/forward identity, canonical create/consume markers and transaction order, current owner, script structure and exact occupied capacity. Append-only orphan payloads never establish membership. `.cell` names retain their separate named owner/manager; their physical cell lock is not substituted for those rights.
- `protocol` reads original transactions from the configured local CKB secondary, checks the indexed output evidence, and decodes the exact protocol bytes. Missing local evidence is an error, with no RPC fallback. DID retains raw CBOR and uses explicit tagged JSON for unsafe integers, bytes, non-text maps and tags. `.bit` record values retain hex even when they are not UTF-8.
- Item feeds attach the item's created/consumed cell and item action to the shared full transaction/activity projection. Collection feeds reuse the same projection. Valid empty and virtual Spore collections have empty feeds; namespace confusion and missing canonical indexes fail.
- DOB current-pattern descriptions reuse the worker's metadata parser and version dispatch. DOB0 maps the first decoder, DOB1 maps the declared execution chain. Type ID arguments are converted to the full native Type ID script hash before canonical executable lookup. Unknown exact binary hashes are explicitly unsupported.
- Current pattern/byte mapping and cached VM traits are separate evidence: cached rows do not record the original executed binary. `decodedAt` and `originalDecoderHashRecorded: false` expose that limit. No trait is recalculated from rules. A later VM step replaces an entire name group; multiple values in that step are retained, and collection frequency counts each `(name,value)` at most once per eligible live item.
- Collection holder buckets and trait coverage read the whole membership scope before gallery filtering/paging. Ordinary Spore content is `notDob`, not a pending decode. Incomplete decode coverage does not publish a partial frequency as a whole-collection denominator.
- Weekly collection activity uses persisted monotone chain time, excludes the incomplete current UTC+8 week, fills complete quiet weeks, and returns its exact anchor and `completeThrough`. Raw cell-version timestamps remain raw transaction evidence and do not define chart buckets.

Protocol references checked: web5fans/did-ckb `f37c14af`, nervina-labs/ckb-nft-scripts `88caddde`, dotbitHQ/das-contracts `5186304b`; parent-supplied DOB exact-binary fixtures and provenance are documented in `crates/dob-decoder/tests/fixtures/README.md`.

The second API slice adds `/spore/clusters/{id}/protocol`: current cluster Cell evidence comes from the canonical live type-instance index across registered Cluster deployments, never from item reverse history or stale metadata ownership. The cluster detail uses the same proof for its current owner and exposes `totalCount` separately from live `sporesCount`. No current Cell is represented by `current: null` and null current owner fields.

The three existing Spore/collection capacity-history routes retain their chart fields and their single arithmetic builder. They now bound results to the last complete UTC+8 day from the pinned indexed chain clock and expose `source`, `completeThrough`, and `anchor`. Missing or malformed calendar evidence is an error. Future-only requests have empty data with the real anchor. Raw timestamp reversal does not change day assignment.

Regression-first coverage also rejects mismatched payload script hashes, stale/duplicate current type-index rows, and item IDs in the Cluster chart namespace. The same immutable cell proof computes exact occupied capacity and validates lock/type script hashes for every item and cluster. The producer fixture includes a real cluster Cell and eight members; its cluster capacity totals count member holdings, not the separate cluster Cell.

Validation: 31 asset-history endpoint tests, 62 assets tests, and 28 Spore tests; normal commit hooks run formatting and strict workspace Clippy. No persistent write path changes. Re-sync required: no.

The third API slice resolves DOB Type ID decoders directly through the canonical live type-instance index. This is the sole source, not a fallback: the script-reference usage mapping is not a complete directory of decoder-only deployments and is not queried. The shared current-Cell proof validates index shape, canonical markers, transaction location, recomputed scripts, and uniqueness. The native Type ID code/hash type/args must match exactly; original local CKB transaction bytes must agree with stored data hash and be ELF. Retirement or a live non-ELF replacement yields explicit unsupported current-pattern evidence, never the historical executable. Missing or contradictory local evidence errors with decoder args and outpoint context.

Three additional endpoint regression groups use the fixed real `98b7af…78e1` ELF and cover absent/unrelated usage mappings, retirement/non-ELF, duplicate live rows, stale block keys, data-hash mismatch, missing local reader, and missing raw transaction. Final affected suites: 34 asset-history tests and 28 Spore tests. Cached VM traits still do not acquire a fabricated original execution binary or a claim of contemporaneous execution.

## Separately authorized upstream correction

Raw Cluster evidence exposed an existing two-field Cluster parser defect. A focused shared-parser correction follows the read-only API commits; see [the correction report](../reports/2026-10-27-cluster-v1-parser-correction.md). The API-only slices require no re-sync. Correcting already-indexed two-field Cluster metadata and dependent DOB results does require replay with the fixed indexer; this task does not modify user stores or perform that replay.
