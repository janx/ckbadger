# Build Version Branch-Aware Format Design

## Goal

- Change the compile-time build version format from `<semver>+<short-hash>` to:
  - `main`: `<semver>@<short-hash>`
  - non-`main`: `<semver>+<branch-name>@<short-hash>`
- Keep a single version computation path so CLI, footer, markdown, raw, and runtime-config all continue to reuse the same string.

## Principle Alignment

- CKB Native: no chain semantics or derived data behavior changes; this is build metadata formatting only.
- Local First: the local artifact reports both source branch context and exact commit identity directly from the local checkout.
- Agent Friendly: one richer version string helps agents distinguish builds from `main` versus feature branches without extra Git inspection.

## Problem Summary

- The current build version only includes semantic version and commit hash.
- That is enough to identify one revision, but it does not expose whether the build came from `main` or a feature branch.
- Since the same buildVersion is now surfaced in CLI, runtime-config, footer, markdown, and raw outputs, its format should carry branch context in the single existing computation path.

## Constraints

- Build version must still be computed exactly once at compile time.
- Branch names must be preserved verbatim; do not sanitize `/` or other Git branch characters.
- `main` must not render `+main`.
- Consumers must not introduce their own branch-aware formatting rules.
- Existing surfaces should continue to consume `CKBADGER_BUILD_VERSION` as an opaque string.

## Approaches Considered

### Approach 1: Update the existing build formatter only

- Extend the compile-time formatter to include branch context.
- Reuse the exact same `CKBADGER_BUILD_VERSION` env var everywhere else unchanged.

Trade-offs:

- Minimal surface area.
- Strongest single-calculation-path alignment.
- Requires a small shared formatter helper to make branch formatting testable.

### Approach 2: Leave build formatter simple and append branch in consumers

- Keep `CKBADGER_BUILD_VERSION=<semver>@<hash>`.
- Have CLI or frontend append branch labels separately when displaying it.

Trade-offs:

- Creates multiple formatting paths.
- Violates the explicit goal to keep one version computation path.

### Approach 3: Separate branch and hash fields in runtime config

- Compute multiple build metadata fields and let each consumer render its own combined string.

Trade-offs:

- More flexible.
- Unnecessary for the requested format change and weaker for consistency.

## Recommendation

- Use Approach 1.
- Put the formatting rule in one shared pure helper used by `build.rs`, then keep every consumer as a passive reader of the single resulting version string.

## Proposed Design

### 1. Shared formatter helper

- Add a small pure helper, for example `format_build_version(semver, branch_name, commit_hash) -> String`.
- Rules:
  - `main` -> `{semver}@{commit_hash}`
  - anything else -> `{semver}+{branch_name}@{commit_hash}`

### 2. Build script inputs

- `build.rs` continues to fetch:
  - `CARGO_PKG_VERSION`
  - short commit hash
- It also fetches the current branch name.
- The helper produces the final string exported as `CKBADGER_BUILD_VERSION`.

### 3. Consumer contract

- No consumer-side formatting changes.
- CLI clap metadata, frontend runtime-config, footer, markdown frontmatter, raw meta, and discovery docs keep consuming the final opaque string.

### 4. Failure handling

- If branch name cannot be resolved, the build fails with actionable context.
- No silent fallback to dropping branch context except the explicit `main` rule.

## Affected Files

- `crates/cli/build.rs`
- `crates/cli/src/main.rs`
- new shared CLI version-format helper file
- tests that assert example build version strings in API/frontend

## Testing Strategy

- Add pure formatter tests for:
  - `main` omission
  - non-main inclusion with verbatim slash-containing branch names
- Update CLI integration test to assert:
  - version contains exactly one `@`
  - hash is hex
  - `+main@` does not appear
- Update propagation tests to use an example non-main string like `0.1.0+feature/foo@abcdef123456`
