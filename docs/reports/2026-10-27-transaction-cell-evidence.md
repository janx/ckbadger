# Transaction cell evidence — D2 completion

## Goal

Close the approved Main v5 board's per-cell identity/subtitle/media and CKBFS continuity omissions. The board requires a specific object and file to remain recognizable across input/output cells; a generic type-script label does not supply that evidence.

Scope: Rust transaction detail responses and regression tests. Frontend contract/rendering is owned by the frontend lane (`9707f5ed`). No new endpoint, persistent field, index, or indexer write was introduced.

## Principle Alignment

- **CKB Native / traceable semantics:** each display descriptor starts from the actual cell's type script and bytes. Inputs fetch their own previous-output data from the node DB, including consumed cells.
- **Single calculation path:** committed and resolved pool cells call the same display builder, reusing existing UDT, Spore, mNFT and identity parsers. CKBFS classification extends the shared bundled registry; there is no API hash table.
- **Fail fast:** missing indexed item metadata, malformed registered cell data, missing input data, invalid token decimals, and standard/payload mismatches include the transaction/outpoint or item ID. Valid binary filenames are not malformed protocol data.
- **Local First / store boundaries:** all added operations are reads. Domain, append-only and network stores have no changed writes. No re-sync is needed for this follow-up.

## Read contract and evidence decisions

Inputs and outputs both expose required nullable `display`. A descriptor has `kind`, `standard`, `id`, and nullable `name`, `collectionId`, `collectionName`, `symbol`, `decimals`, `amount`, `compositionTier`. Raw token amounts are integer strings; file identity is the full type-script hash. Plain cells, cellbase pseudo-inputs and unrelated protocols have `display: null`.

| Field / domain | Owning evidence and semantics |
| --- | --- |
| Input type hash type | Immutable `LiveCellInfo.type_hash_type`; missing/invalid values fail. The old TokenInfo → ScriptInfo → `unknown` guessing chain was removed. Imported label forms cannot override the actual script. |
| Token identity / amount | Canonical UDT classifier and exact actual-cell amount parser; ID is full type-script hash. Name/symbol/decimals are indexed token metadata; unknown optional metadata stays null. |
| Spore identity / collection | Existing Spore parser reads the actual cell ID/collection. Canonical item/cluster records supply immutable names and the persisted per-item composition result, independent of current liveness or owner. Pending decode remains `decode_pending`. |
| mNFT identity | Existing mNFT parser reads the token/class IDs. Latest class names/renderers are mutable (`writer/mnft.rs` updates them and moves tier counts), so historical token tiles do not borrow those latest values: optional names/tier remain null. |
| Identities | Existing standard-specific parsers read ID and name from the actual cell. `.bit` uses its indexed immutable account-name label learned from validated witnesses; did:ckb supplies no invented name from its mutable document. `.cell` infrastructure root is not an identity. |
| CKBFS filename | One strict Molecule table-prefix extractor reads the actual cell bytes. Both bundled versions share fields 2/3 as content-type/filename Bytes. Compatible extra fields are legal. UTF-8 text is returned verbatim; valid binary names yield null text, retaining the exact file ID. |
| Continuity | The descriptor identifies the file/object/identity; the already exposed complete type-script instance groups prove input/output equality. Frontend only draws an unambiguous one-input/one-output relation. |
| Pool | The same builder consumes resolved cell bytes; unindexed optional asset metadata remains absent instead of being invented. No writes or lazy media decoding are triggered. |

CKBFS primary sources: [RFC.md](https://github.com/code-monad/ckbfs/blob/master/RFC.md), [RFC.v3.md](https://github.com/code-monad/ckbfs/blob/master/RFC.v3.md), and [contract entry.rs](https://github.com/code-monad/ckbfs/blob/master/contracts/ckbfs/src/entry.rs). The contract calls `from_compatible_slice` and compares filename/content-type bytes; it does not require UTF-8. The API extracts display metadata, not a second consensus checksum/backlink validator.

## Registry prerequisite

Commit `f8e314ba` recognizes the two bundled CKBFS deployment hashes via `ProtocolScript::Ckbfs` and explicitly maps it to the existing bulk `Plain` semantic tag. No protocol facts, asset kind or reducer state was added. Activity `CodeHashes` skips it through its existing non-asset arm. Existing all-registry bulk/live classification parity passes. The profile lane received this prerequisite before the final diagnostic build.

## Validation

- RED: registry detection returned `None` instead of `ckbfs`; fixed and verified. Five focused CKBFS tests and the exact all-registry bulk/live parity test passed.
- RED: exact input fixture failed with `invalid script hash type unknown`, despite stored data1 evidence. The owning read path now uses the immutable hash type. A conflicting imported `type` label and a missing hash type are explicit regression cases.
- RED: detail omitted display fields and returned 200 for missing indexed object metadata / malformed registered file bytes. The targeted endpoint suite passed after implementation (five tests including the shared test helper).
- New unit cases cover v2/v3/compatible CKBFS fields, invalid offsets/Bytes sizes, valid binary names, unregistered lookalike data, unindexed Spore metadata, mutable mNFT class exclusion, and inconsistent Spore payload variants.
- The first full API run compiled and ran 647 unit tests: 625 passed, 22 existing mock-socket tests failed with sandbox `EPERM` on port binding. Cargo stopped before running integration binaries. All five new unit cases present in that run passed.
- Final refinements (historical consumed-output fixture, deterministic registry fixture selection, payload-variant invariant/test) are source-complete. Their final execution, full API socket-capable rerun, check, strict clippy and formatting gate are **pending the root integration gates after the performance quiet window**. No check was skipped or claimed green.

### Integrated `.cell` fixture correction

The root integrated run at `1eb5a929` exposed one older fixture that seeded committed `.cell` transaction/header/live-cell rows without the identities indexing necessarily creates. This was reproduced before changing the fixture (`api-dotcell-fixture-red.log`, exit 101). Both live `insert_dotcell_name` (`db/writer/dotcell.rs`) and bulk `ObjectOwner::insert_dotcell` persist ordinary names as `IdentityEntry` with `IdentityExtra::DotCell`; only the empty-label ring root deliberately has no identity. The API already excludes that root.

The fixture now derives both identity entries and validated records from its existing cell/witness bytes through `DotCellParser`, matching the writer contract without changing production code. Existing witness assertions remain, and both output display descriptors must agree with their decoded identity IDs and names. A separate incomplete-store regression preserves the contextual 500 for missing identity metadata, including the transaction, output index and identity ID.

The complete affected suites passed with local socket access: `api_transactions` **41/41** and `api_transaction_cells` **5/5** (`api-dotcell-fixture-green.log`). API test-target `cargo check`, strict `cargo clippy -- -D warnings`, touched-file rustfmt and `git diff --check` also passed. The original root integrated failure remains in `site-redesign-d2-cell-evidence-final-integrated-test.log`; the full workspace rerun remains owned by root. No persistent production writes or schema changes are part of this fixture correction; re-sync is not required.

## Result

The API now provides the exact evidence needed by Transaction Main v5 tiles and file continuity. Read-time reconstruction honors historical cell bytes and reports unavailable or inconsistent required evidence with context.

- Re-sync required: **no** for this API/registry follow-up; the broader round-1 storage changes still have their separately documented re-sync requirement.
- Next: root integrates the API and frontend commits, runs the pending gates, and completes the existing performance gate / user post-resync checks.
