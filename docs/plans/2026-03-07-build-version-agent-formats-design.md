# Build Version In Footer And Agent Formats Design

## Goal

- Expose the same compile-time `buildVersion` string in three user/agent-facing surfaces:
  - frontend footer
  - markdown page output (`.md`)
  - raw page output (`.raw`)
- Keep one exact version source and avoid introducing any second calculation path.

## Principle Alignment

- CKB Native: no chain semantics or derived-chain data paths change; this is presentation and agent-format metadata only.
- Local First: the local binary reports its exact local build identity directly from the build artifact.
- Agent Friendly: HTML, markdown, and raw outputs all expose the same build identity in machine-readable places with no extra request choreography.

## Problem Summary

- CLI now exposes `buildVersion`, but the frontend footer and agent-oriented page formats do not.
- ckbadger already supports agent-facing `.md` and `.raw` variants for page URLs, so omitting build identity there makes debugging and automation weaker.
- The project explicitly wants one build-time version path, not separate version logic in API, frontend, and docs.

## Constraints

- The version source remains the compile-time build metadata already produced for the CLI.
- Do not add an API endpoint just to fetch the version.
- Do not add a second Git lookup path in frontend or API crates.
- `.md` and `.raw` should carry version metadata in their structured meta sections, not as duplicated free-text body content.
- Footer should display the same injected version string humans can see.

## Existing Relevant Architecture

- `crates/cli/build.rs` already computes `CKBADGER_BUILD_VERSION=<semver>+<short-hash>`.
- `crates/cli/src/main.rs` already reads that env var and uses it for clap metadata.
- `crates/api/src/entry.rs` already serves `/runtime-config.js` for frontend runtime settings.
- `frontend/lib/runtime-config.ts` already centralizes frontend runtime config reads.
- `.md` output already has shared YAML frontmatter via `frontend/lib/ai/markdown-format.ts`.
- `.raw` output already has a shared `meta` object via `frontend/lib/ai/raw-renderer.ts`.

## Approaches Considered

### Approach 1: Footer reads runtime config, markdown/raw read browser globals directly

- Add `buildVersion` to `/runtime-config.js`.
- Footer and agent renderers call global runtime-config readers directly.

Trade-offs:

- Low code volume.
- Keeps one source.
- Leaves markdown/raw renderers implicitly bound to browser-global access, which is weaker for testability and agent-format determinism.

### Approach 2: Single Rust injection path, explicit frontend read boundary

- CLI passes the already-built version string into frontend server config.
- Frontend server injects `buildVersion` through `/runtime-config.js`.
- `frontend/lib/runtime-config.ts` becomes the single frontend read boundary.
- Footer uses the runtime-config helper.
- Markdown/raw/capabilities use explicit helper inputs or a shared runtime-config read helper rather than each inventing their own source.

Trade-offs:

- Slightly more plumbing.
- Keeps one source and one frontend read boundary.
- Strongest alignment with single-calculation-path and agent-friendly output contracts.

### Approach 3: New API `/version` endpoint plus footer/agent fetches

- Add version route in API and have frontend/agent outputs read from it.

Trade-offs:

- Adds unnecessary network choreography and a second read path.
- Violates the stated goal to avoid a second calculation/read contract for the same value.

## Recommendation

- Use Approach 2.
- The build version should be computed exactly once at compile time, passed from CLI to frontend server, injected once into runtime config, and then reused consistently by footer, markdown, raw, and capability discovery.

## Proposed Design

### 1. Shared runtime contract

- Extend `FrontendServiceConfig` with `build_version: String`.
- In `crates/cli/src/main.rs`, pass the existing `BUILD_VERSION` constant into the frontend service config.
- Extend the Rust-side runtime-config payload (`/runtime-config.js`) with `buildVersion`.

### 2. Frontend runtime-config boundary

- Extend `frontend/lib/runtime-config.ts`:
  - `CkbadgerRuntimeConfig.buildVersion?: string`
  - `resolveBuildVersion(config?: CkbadgerRuntimeConfig): string`
- The helper should fail fast on blank explicit values and only use the single injected value or a clearly defined local default constant for tests.
- Other consumers should not read `window.__CKBADGER_RUNTIME_CONFIG__` directly.

### 3. Footer output

- Update `frontend/components/layout/site-footer.tsx` to display the build version in the existing footer panel.
- Keep the current visual language; add the version as a compact monospace label so it remains scannable for humans and agents.

### 4. Markdown output

- Extend `MarkdownDocMeta` with `buildVersion`.
- Add `buildVersion` to the YAML frontmatter emitted by `buildMarkdownDocument`.
- Update markdown renderer meta construction to source the value from the shared runtime-config boundary.
- Do not duplicate the version in markdown body sections.

### 5. Raw output

- Extend `RawMeta` with `buildVersion`.
- Include `buildVersion` in every raw response meta object.
- Source it from the same shared runtime-config boundary used by markdown/footer.

### 6. Capability and discovery output

- Update `frontend/lib/ai/capabilities.ts` to describe that `.md` frontmatter and `.raw.meta` carry `buildVersion`.
- Update `frontend/public/llms.txt` and `frontend/public/llms-full.txt` so discovery text tells agents where to find build identity.

## Failure Handling

- Build metadata generation remains fail-fast in `crates/cli/build.rs`; no fallback hash or `unknown` placeholder.
- Runtime-config injection should carry the exact provided string without recomputation.
- Frontend helpers should not silently rewrite malformed explicit `buildVersion` values.

## Affected Files

- `crates/cli/src/main.rs`
- `crates/api/src/entry.rs`
- `frontend/lib/runtime-config.ts`
- `frontend/components/layout/site-footer.tsx`
- `frontend/lib/ai/markdown-format.ts`
- `frontend/lib/ai/markdown-renderer.ts`
- `frontend/lib/ai/raw-renderer.ts`
- `frontend/lib/ai/capabilities.ts`
- `frontend/public/llms.txt`
- `frontend/public/llms-full.txt`

## Testing Strategy

- Rust:
  - verify `/runtime-config.js` now includes `buildVersion`
- Frontend:
  - runtime-config helper test for `resolveBuildVersion`
  - footer component test for visible version
  - markdown-format test for frontmatter `buildVersion`
  - markdown-renderer test for emitted `buildVersion`
  - raw-renderer test for `meta.buildVersion`
  - capabilities test if capability payload is extended
- Verification commands:
  - `cargo test -p ckbadger-api frontend_runtime_config`
  - `cd frontend && pnpm test -- --runInBand ...` or targeted `vitest` route/helper tests
