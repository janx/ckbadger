# Site redesign: indexer lane (2026-10-26)

## Goal

- Implement every round-one persisted correction from the approved site redesign and bulk-sync impact assessment, with one eventual user-run re-sync.

## Principle Alignment

- Chain data is authoritative; preserve one calculation for bulk, live and rollback.
- Every changed persistent write targets the **domain** store. No network-store writes or append-only `CF_CELLS` payload changes are planned. Existing append-only rejection tests form the storage boundary gate.
- No new per-transaction index rows, inline DOB VM decoding, script transaction index, backfill, or running-store changes. Protocol-specific reduction stays beside the address reducer.

## Decisions before implementation

| Item               | Decision                                                                                                                                                                        | Write target / read contract                                                       |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| Fix 6 B            | Persist explicit DOB decode-pending tier; retain decoded immutable DNA profile during transfer                                                                                  | Existing object/profile and aggregate domain rows                                  |
| Fix 7 / gap 31     | Persist exact identity item action in existing history rows; common bulk/live classifier consumes ActionData and existing owner/expiry facts                                    | Existing identity outpoint history values; `.cell` reads existing protocol actions |
| Fix 8              | Derive registered type-id ELF family versions from chain lineage; narrow tracked cells to code-bearing type IDs                                                                 | Existing script domain metadata/rollups                                            |
| Fix 11             | Persist separate phase-0 deposit and phase-1 request counts in existing latest singleton                                                                                        | DAO domain singleton                                                               |
| Fix 15             | Record bidirectional fork reversal links on observed switchback, live only                                                                                                      | Existing sync-meta domain event rows                                               |
| CKB moved / gap 9  | Sum positive participant deltas, excluding cellbase, once; transaction Value is read-time                                                                                       | Existing daily/hourly activity rows; `TxActions` helper for API                    |
| Gap 22             | Correct existing mNFT collection activity counter                                                                                                                               | Existing aggregate domain row                                                      |
| Gap 58             | Existing `depositors_count` already persists unique phase-0 locks in bulk/live; fix rollback recomputation to the same semantics, preserving `cumulative_depositors` separately | Existing DAO daily domain snapshot                                                 |
| All other C groups | API read-time or lazy execution, outside this lane                                                                                                                              | No additional bulk writes                                                          |

## Result

- Work in progress. Regression tests are added and observed failing before fixes; existing tests bracket refactors.
- Re-sync required: **yes**, after integration; the user performs the one final re-sync.
- Next: targeted regression gates, workspace Rust gates, integration, then root-owned paired scratch performance runs.
