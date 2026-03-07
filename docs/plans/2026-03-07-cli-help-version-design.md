# CLI Help And Version Design

## Goal

- Change the top-level `ckbadger --help` title text to `A local-first and agent-friendly CKB explorer`.
- Make top-level `ckbadger --version` report a semantic version string extended with the compile-time Git commit hash.

## Principle Alignment

- CKB Native: no protocol or data-path behavior changes; this stays a presentation/runtime metadata change only.
- Local First: the binary reports its exact local build identity without external lookups.
- Agent Friendly: `--help` communicates the project positioning directly, and `--version` exposes an unambiguous build identifier for debugging and automation.

## Problem Summary

- The current help title still says `CKB blockchain explorer`, which misses the current project positioning.
- The current version output only exposes the Cargo package version and does not identify the exact source revision used to build the binary.
- For local debugging and agent-driven workflows, version output should point to one exact build artifact.

## Constraints

- Use the compile-time Git commit hash, not a runtime Git lookup.
- Keep one version construction path.
- Fail fast if the build cannot determine the commit hash.
- Add regression tests for both help text and version formatting.

## Approaches Considered

### Approach 1: Runtime Git lookup

- Call `git` when `--version` runs and append the current commit hash.

Trade-offs:

- Breaks outside a Git checkout.
- Violates the compile-time requirement.
- Adds a second runtime failure mode.

### Approach 2: Build script injects version metadata

- Add `crates/cli/build.rs`.
- Resolve `CARGO_PKG_VERSION` and `git rev-parse --short HEAD` during compilation.
- Export one `CKBADGER_BUILD_VERSION` env var for the crate to use in clap metadata.

Trade-offs:

- Small amount of extra build-script code.
- Correctly binds the binary to the source revision used at build time.
- Keeps a single version path.

### Approach 3: Add a Git metadata crate

- Use `vergen` or similar to inject the version string.

Trade-offs:

- Works, but adds dependency and configuration weight for a very small requirement.
- Not necessary when the needed metadata is minimal.

## Recommendation

- Use Approach 2.
- A small build script is enough to compute `<semver>+<short-hash>` once and expose it to clap.
- If Git metadata cannot be resolved, the build should fail with actionable context.

## Proposed Design

### Help text

- Update the top-level clap `about` string in `crates/cli/src/main.rs` to `A local-first and agent-friendly CKB explorer`.

### Version text

- Add `crates/cli/build.rs`.
- During build:
  - read `CARGO_PKG_VERSION`
  - run `git rev-parse --short HEAD`
  - compose `<cargo-semver>+<short-hash>`
  - export it as `CKBADGER_BUILD_VERSION`
- In `crates/cli/src/main.rs`, make clap use `env!("CKBADGER_BUILD_VERSION")` for the root command version.

### Failure handling

- If `git` cannot be executed, exits non-zero, or returns an empty hash, the build script panics with context.
- No fallback to plain semver and no placeholder hash.

## Affected Files

- `crates/cli/build.rs`
- `crates/cli/src/main.rs`

## Testing Strategy

- Add a help-text test that renders the root command help and asserts the new title exists.
- Add a version-format test that checks the exported build version matches `<semver>+<short-hash>`.
- Run targeted `cargo test -p ckbadger` checks for the new tests.
