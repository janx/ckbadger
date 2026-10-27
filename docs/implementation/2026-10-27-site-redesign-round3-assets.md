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

Implementation in progress. Re-sync required: no.

## Read evidence and boundaries

- `cell-versions` validates reverse/forward identity, canonical create/consume markers and transaction order, current owner, script structure and exact occupied capacity. Append-only orphan payloads never establish membership. `.cell` names retain their separate named owner/manager; their physical cell lock is not substituted for those rights.
- `protocol` reads original transactions from the configured local CKB secondary, checks the indexed output evidence, and decodes the exact protocol bytes. Missing local evidence is an error, with no RPC fallback. DID retains raw CBOR and uses explicit tagged JSON for unsafe integers, bytes, non-text maps and tags. `.bit` record values retain hex even when they are not UTF-8.
- Item feeds attach the item's created/consumed cell and item action to the shared full transaction/activity projection. Collection feeds reuse the same projection. Valid empty and virtual Spore collections have empty feeds; namespace confusion and missing canonical indexes fail.
- DOB current-pattern descriptions reuse the worker's metadata parser and version dispatch. DOB0 maps the first decoder, DOB1 maps the declared execution chain. Type ID arguments are converted to the full native Type ID script hash before canonical executable lookup. Unknown exact binary hashes are explicitly unsupported.
- Current pattern/byte mapping and cached VM traits are separate evidence: cached rows do not record the original executed binary. `decodedAt` and `originalDecoderHashRecorded: false` expose that limit. No trait is recalculated from rules. A later VM step replaces an entire name group; multiple values in that step are retained, and collection frequency counts each `(name,value)` at most once per eligible live item.
- Collection holder buckets and trait coverage read the whole membership scope before gallery filtering/paging. Ordinary Spore content is `notDob`, not a pending decode. Incomplete decode coverage does not publish a partial frequency as a whole-collection denominator.
- Weekly collection activity uses persisted monotone chain time, excludes the incomplete current UTC+8 week, fills complete quiet weeks, and returns its exact anchor and `completeThrough`. Raw cell-version timestamps remain raw transaction evidence and do not define chart buckets.

Protocol references checked: web5fans/did-ckb `f37c14af`, nervina-labs/ckb-nft-scripts `88caddde`, dotbitHQ/das-contracts `5186304b`; parent-supplied DOB exact-binary fixtures and provenance are documented in `crates/dob-decoder/tests/fixtures/README.md`.

The first API slice does not yet finish cluster-cell raw evidence or complete-day cutoffs on the older capacity chart handlers; those are explicit follow-up work in this lane, not placeholders declared complete.
