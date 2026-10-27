# D5 Scripts and Cells — implementation and acceptance

## Goal

Complete D5 of the approved site redesign: Scripts inventory, family/reference
detail, Cell and Code Cell. This round starts at accepted D4 revision `9cbc8f33`
and integrates on `integration/site-redesign-round4`. Two implementation lanes
explicitly used **gpt-6-sol**. Root coordinated, reviewed, integrated and ran
independent acceptance; an existing agent supplied a read-only boundary review.

## Principle Alignment

- Design starts from `docs/prompts/`, the approved boards and
  `docs/SCRIPTS_CODE_CELLS_AND_REFS.md`. Family, exact reference form, bytecode
  version and concrete code-cell outpoint remain distinct identities.
- No production write path changes target the **domain**, **append-only** or
  **network** store. The API remains read-only. The metadata role loader is a
  shared read of bundled metadata plus the existing workdir overlay; six unused
  registry entries now declare their role. No schema, backfill or new index is
  introduced, and gap 44's script transaction feed remains dropped.
- Current executable resolution requires live chain evidence. Historical family
  membership and historical transaction execution have separate meanings:
  indexed provenance supplies the former, actual transaction dependencies the
  latter. A same-byte binary under another type hash cannot satisfy an exact
  type reference. Native Type ID is treated as a built-in rule.
- Byte ranges, capacity, timestamps, witness configuration and ELF offsets are
  exact. CellOutput framing is separate from occupied capacity; all cell-data
  bytes count. Runtime history retains the accepted indexed-calendar dependency;
  no all-header scan was added.

## Result

### Delivered behavior

- Scripts shows complete pre-pagination class/role totals, role filtering, search
  and stable pagination. Mixed reference forms and unused registered scripts
  remain visible. Family history includes deployment, retirement, reactivation
  and multiple anchors, in canonical transaction order.
- Custody evidence binds the actual multisig witness configuration to its lock
  arguments, exposes threshold/key hashes and exact retained keys at handover,
  and distinguishes a current holder from an observed signer. The never-signed
  claim uses complete address transaction counts. Capacity markers and custody
  bands use the indexed chart day.
- Cell pages expose exact creation/spend locations, consuming input, raw times
  and chain-time lifetime or live age with an indexed-tip anchor. The microscope
  contains original Molecule CellOutput bytes, complete field maps and script
  hashes, plus deterministic data layouts. Actual creating/spending transaction
  groups, witnesses and dependencies reuse TxStructure.
- Code Cells expose ELF headers, program/section tables, file versus memory
  ranges, toolchain/psABI attributes, current exact-reference candidates and
  complete Type ID lineage. Distinct binary ordinals remain stable across
  reactivation. Upgrade cohorts distinguish changed from unchanged bytecode in
  the same transaction, including cellbase's empty consumed-input cohort.
- The pages use the shared flat layouts, approved anatomy colours and exact
  values. Complete hashes wrap on mobile. Compact plot glyphs retain full event
  labels in the legend and controls. API failures show context; HTTP 404 remains
  distinct from an unavailable source.

### Corrections found during integration

Independent review and regression tests closed exact-reference candidate leaks,
cross-family Type ID history truncation, missing canonical transaction proof,
same-block ordering, old-binary/current-usage confusion and native binding/live
binary mismatch. Generic type resolution now considers all live binaries and
retains ambiguity instead of promoting a persisted historical binding to the
current executable. `/cells/by-script` reads the requested reference form rather
than remapping it through an unrelated legacy ScriptInfo association.

The first complete Rust attempt at `564eceaa` stopped on two existing utilization
chart assertions. Its current-resolution correction had removed retired generic
references from historical family buckets. Sol extracted one historical family
candidate helper shared by Script usage and charts; the original assertions
remain intact. Three old Script API fixture producers also lacked canonical
transaction index rows; they now use the existing canonical fixture helper,
without weakening production validation or test assertions.

Browser preflights found overflowing full dependency/current-code hashes and
overlapping event labels. Both were fixed with regressions. Error-state review
also found API failures presented as “Script not found”; named and raw-reference
failures now retain their source and message. The first final frontend test run
encountered sandbox `EPERM` in two Vite loopback-server tests; the same assertions
passed when rerun with local binding allowed.

### Integrated gates

Final validation passed. At `3e94a4c1`, the complete Rust command
`cargo test --workspace --no-fail-fast --offline --locked -j 2` passed
**4,713 tests across 73 suites, with zero failures**. Three existing opt-in
native-VM tests remain ignored by the default command; this round does not
change their verifier paths. The final frontend source, unchanged from
`223776f9`, passed **1,599 tests in 196 files**, type checking, lint and production
build. HEAD stayed fixed during each gate.

The preceding full Rust attempt at `bcd7be71` completed all 73 suites with 4,709
passes and four failures in one filter fixture. It declared 61 CKB occupied for
a 74-byte cell (8-byte capacity and two 33-byte empty-args scripts). The test-only
repair preserves every filtering assertion and strict production validation;
the complete final rerun above closes those four failures.

The five freshly exported API producer fixtures exactly match the committed
frontend consumer JSON. Formatting, workspace check and strict all-target
Clippy pass. Production source is fixed at `bcd7be71`; the final test-fixture and
documentation changes do not change that production source.

The frozen debug CLI is
`.worktrees/site-redesign-round4-results/d5-scripts-cells-accepted-ckbadger`,
SHA-256 `7c9887f95f073de21854dcbde39f511813acb5e41c54ab3a8c9dd9d795e7b62d`.
The frontend production build preceded the final Rust checks and CLI build.

### Independent chain evidence

The oracle reads original node outputs and transactions and independently
serializes/hashes their bytes. It also compares ELF metadata with `readelf`,
multisig configuration with the actual upgrade witnesses, and the upgrade cohort
with exact old/new Type ID outpoints. It does not derive expected values from
the new API implementation.

| Evidence                | Concrete sample                                                                                                                         |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Spore Cell              | `0xbe1b9761b6258b002769a0ff356d4b45469d467a08e978604506c2d2df936cd8:1`; CellOutput 195 bytes, data 181 bytes, occupied 320 bytes        |
| JoyID v5                | `0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0`; CellOutput 182 bytes, ELF 100,312 bytes, occupied 100,438 bytes |
| Exact Type ID reference | `0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323`; five distinct observed JoyID versions                             |
| Historical JoyID v1     | `0xbac0f6f0a177a03974870df47ab0c3e761c5dd19e9ca165358d0b72bd7f433f5:0`; original version ordinal and exact spend retained               |
| Genesis code            | `0xe2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c:1`; readable ELF with cellbase chronology                           |

The real JoyID transition retains exactly **2 of 5** multisig keys. In its v5
transaction, cohort outputs 0 and 1 change bytecode while outputs 2–4 retain it.
ELF validation distinguishes `.bss` memory from file bytes and checks structured
architecture attributes against the
[RISC-V psABI](https://riscv-non-isa.github.io/riscv-elf-psabi-doc/#_layout_of_riscv_attributes_section).

The final frozen binary passed **161 independent assertions** and all 12 API
probes. All **16 real mainnet page/width combinations** passed at 1440 and 390 px:
Scripts, JoyID, its raw reference, unused AlwaysSuccess, Spore Cell, current and
historical JoyID Code Cells, and a genesis Code Cell. Checks cover page hierarchy,
runtime errors, document overflow, canonical links, role/pagination reset, empty
search, custody handover and exact byte focus/paging. Root inspected desktop and
mobile screenshots, including Code Cell ELF, lineage and transaction sections.
Wide ELF tables retain their own horizontal scroll area on mobile.

The separate **18 controlled loading/404/500 states** also pass at both widths:
Scripts list loading/failure, Cell loading/missing/failure, family missing/failure,
and raw-reference lookup/code-cell failures. The raw code-cell error case uses an
unregistered reference so the normal named-family redirect does not bypass the
tested interface. Its initial teardown closed a page while unrelated header
requests remained in flight; the harness now keeps loading requests pending
until page closure and handles only teardown cancellation separately. The
already-passing real-chain pages and data assertions were not rerun for these
harness-only corrections. All temporary child processes were stopped after each
successful or failed attempt.

### Read performance observations

These are individual local HTTP observations from the preserved D4 binary and
the final D5 binary, on a shared host with differing cache/tip state. They are
not a paired throughput benchmark or proof of unchanged latency. D5 returns and
validates substantially more evidence; the extra read cost is visible here.

| Route                 | D4 ms | Final D5 ms | D4 / D5 response bytes |
| --------------------- | ----: | ----------: | ---------------------: |
| Scripts, 100 families |   5.2 |       103.1 |        24,197 / 32,300 |
| JoyID family          | 153.5 |       233.1 |         4,943 / 20,142 |
| Spore Cell            |  16.9 |        25.4 |          2,620 / 6,068 |
| JoyID v5 Code Cell    |  48.3 |       248.9 |      202,115 / 212,562 |
| Historical JoyID v1   |  42.7 |       212.4 |      195,074 / 202,517 |

An earlier D5 implementation repeated the family membership scan for each
family: the retained preflight recorded 5.63 s initially and 2.62 s on its next
one-row page request. One shared membership pass and one lineage preparation per
request removed this repeated work. The subsequent preflight measured 44–47 ms;
the final full-list/page-one requests measured 103/83 ms. Those observations show
the large repeated-scan problem is gone, while preserving the full-response
totals instead of calculating only the displayed page.

### Evidence and scope

All commands, immutable build metadata, failed preflights and final logs live in
`.worktrees/site-redesign-round4-results/`. The committed API-produced JSON
fixtures live under `frontend/__tests__/fixtures/d5/` and `fixtures/scripts/`.
The full gate regenerates and deep-compares these producer and consumer files.
The main result records are `integrated-backend-gates.json`,
`integrated-frontend-gates.json`, `accepted-build.json` and
`final-browser-acceptance.json`. The latter combines the successful real-chain
and controlled-state stages and links their original records, including the
earlier harness failures; none of those failed runs is relabeled as a pass.

Temporary APIs use port 18105 and separate RocksDB secondary directories;
production frontend previews use port 3148. Primary stores and user services
remain untouched. Browser HTTP entity checks stub the realtime socket; they do
not claim a new WebSocket acceptance. Responses pin their own coherent read view,
while different requests can observe later tips. Aggregate comparisons therefore
use a single response rather than assume a frozen cross-request snapshot.

This selected-entity acceptance is distinct from the previously accepted full
64-check run `20261027T084559Z-08e068e3`; it does not claim another complete chain
verification or an independent proof of all indexed-calendar dates.

- Re-sync required: **no** for D5. This does not change the separately documented
  D4 historical Cluster parser rebuild boundary.
- Next: **D6 DAO and charts**, followed by D7 network topics and D8 error pages.
