# Site redesign round 4 — Scripts and Cells

## Goal

Implement D5 of the approved `2026-10-26-site-redesign-implementation-plan.md`:
Scripts inventory, script family/reference detail, Cell and Code Cell. Start from
accepted D4 revision `9cbc8f33` on `integration/site-redesign-round4`.

The approved boards and their builders remain in the ignored
`docs/superpowers/plans/2026-10-25-site-ui-review/` directory. `FINAL_REVIEW.md`
overrides older board typography, anatomy colours, tier spacing and the obsolete
script transaction feed. The remaining-gap audit from round 2 is a source map;
current shared components and APIs must be inspected before adding replacements.

## Principle Alignment

- Follow `docs/prompts/WORLD_VIEW.md`, `INFORMATION_DESIGN.md` and
  `docs/SCRIPTS_CODE_CELLS_AND_REFS.md`. A family, runtime reference, exact binary
  version and concrete code-cell outpoint are distinct entities.
- This round changes read-time API contracts and frontend presentation. There
  are no planned domain, append-only or network writes, schema changes, new
  indexes or backfills. Existing primary stores and services remain untouched.
- Current type-reference resolution is separate from historical execution.
  Actual transaction dependencies establish historical code; today's live
  binary cannot stand in for them. Native Type ID is not a deployed ELF file.
- Custody claims require chain evidence. Verify multisig configuration against
  the actual lock arguments; unsupported locks and unavailable evidence remain
  explicit. Lack of a witnessed code upgrade alone does not prove a custodian
  has never spent any cell.
- Exact occupied bytes exclude CellOutput serialization framing but include all
  bytes inside cell data. Byte maps cover every byte exactly once; deterministic
  protocol decoding does not compete with a heuristic alternate interpretation.
- Capacity history stays in tier 1 and uses the accepted indexed calendar. Do
  not fetch all canonical headers for runtime verification. Script detail ends
  with **Its cells**; gap 44 remains dropped to protect bulk-sync performance.
- Cells are developer pages throughout, with no artificial tier-2 boundary.
  Reuse the accepted shared charts, cell anatomy, HexMap and TxStructure.

## Execution

The user requested Sol for implementation. Two agents explicitly use
`gpt-6-sol`; the platform thread limit prevented a third fresh agent. Each owns
one complete feature slice, with an isolated worktree and Cargo target seeded
by copy-on-write from the accepted root build. Root owns coordination,
independent review, integration, acceptance and reports.

| Lane        | Ownership                                                                      | Worktree                                  |
| ----------- | ------------------------------------------------------------------------------ | ----------------------------------------- |
| Scripts Sol | Scripts API, custody/lineage contracts, list/detail UI and tests               | `.worktrees/site-redesign-round4-scripts` |
| Cells Sol   | Cell API, chronology/serialization/ELF/transaction evidence, Cell UI and tests | `.worktrees/site-redesign-round4-cells`   |

Shared files have explicit owners by section. Script response structs and
script-resolution helpers belong to Scripts; Cell response structs and
transaction evidence belong to Cells. Frontend API types follow the same
division. Producers publish actual serialized API JSON for consumers.

| Slice            | Required behavior                                                                                                                | Acceptance evidence                                                                               |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Script inventory | Complete class/role totals; fixed versus replaceable code; unused registered role; search, sort and pagination                   | Data/data1/data2, type references, native Type ID, unused roles, mixed forms, full-scope totals   |
| Script history   | Chronological observed versions and deployments, custody changes, history markers                                                | Retirement, burn, reactivation, multiple anchors, unregistered copies, version with no deployment |
| Script custody   | Current and previous locks, verified multisig configuration and key overlap, precise never-signed scope                          | Actual upgrade witnesses, hash mismatch failure, unsupported custody, absent evidence             |
| Its cells        | Owner, resolved type name, exact anatomy, capacity/occupied and created block                                                    | Lock/type union deduplication, filters before paging, cursor continuity and reset                 |
| Cell chronology  | Created/spent timestamps, consuming input index, lifetime with explicit chain-time basis                                         | Live and spent cells, actual creating/spending transaction groups, output locks do not execute    |
| Cell microscope  | Exact CellOutput serialization, complete field maps, script hashes over actual serialized ranges                                 | With/without type, args lengths, empty data, Spore length words, malformed typed bytes            |
| Code Cell        | ELF header, program headers, sections, padding, toolchain and attributes; reference/consumer/lineage and upgrade-cohort evidence | Real ELF, malformed ELF, non-ELF retirement, ambiguous type refs, NOBITS versus file bytes        |
| Presentation     | Approved hierarchy, exact values and meaningful links, accessible hover/focus, explicit errors                                   | Desktop 1440 and mobile 390, loading/error/empty, large values, overflow and interactions         |

Run affected tests before refactors and regression failures before fixes. Each
lane supplies focused tests and producer fixtures. Final integration runs the
appropriate Rust/frontend gates with a production frontend build before the
last Rust checks, then fixture and read-only mainnet acceptance. Temporary APIs
use independent secondary directories and separate ports; all task services are
stopped after acceptance. Record fixture and live evidence separately.

## Result

D5 implementation and integrated acceptance are complete. Two `gpt-6-sol`
agents delivered the Scripts and Cells feature slices. Production source is
fixed at `bcd7be71`; final test-fixture revision is `3e94a4c1`.

- Complete Rust gates pass: **4,713 tests in 73 suites**, zero failures, formatting,
  workspace check and strict all-target Clippy. Three existing opt-in native-VM
  tests remain unrun by the default command.
- Final frontend source passes **1,599 tests in 196 files**, type checking, lint
  and production build. Five regenerated API fixtures exactly match their
  committed frontend consumers.
- Read-only mainnet acceptance passes **161 independent assertions** and
  **16 real page/width combinations**, plus **18 controlled loading/error states**
  at 1440/390 px. All temporary APIs/previews were stopped.
- Re-sync required: **no** for D5. No schema or production store write path
  changed. Existing services and primary stores were not replaced or rebuilt.
- What to do next: **D6 DAO and charts**; D7 (Fiber, forks, network and hardforks)
  and D8 (not-found/crash pages) remain subsequent rounds.

The [detailed acceptance report](reports/2026-10-27-site-redesign-round4-acceptance.md)
records independent byte/witness/ELF evidence, corrected regressions, exact
binary provenance, read-performance observations and verification limits.
