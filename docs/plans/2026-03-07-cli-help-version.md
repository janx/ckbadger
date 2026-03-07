# CLI Help And Version Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Update the CLI help title and make `ckbadger --version` print `<semver>+<short-hash>` using the compile-time Git commit hash.

**Architecture:** Keep the help text change in clap metadata and add a single build-time version generation path in `crates/cli/build.rs`. The root command consumes one exported environment variable so help and version metadata stay centralized and fail fast when Git metadata is unavailable.

**Tech Stack:** Rust, clap derive, Cargo build scripts, inline unit tests in `crates/cli/src/main.rs`

---

### Task 1: Add failing CLI metadata tests

**Files:**

- Modify: `crates/cli/src/main.rs`
- Test: `crates/cli/src/main.rs`

**Step 1: Write the failing tests**

Add tests that:

```rust
#[test]
fn test_cli_help_uses_project_positioning_title() {
    let mut cmd = Cli::command();
    let help = cmd.render_help().to_string();
    assert!(help.contains("A local-first and agent-friendly CKB explorer"));
}

#[test]
fn test_build_version_uses_semver_plus_short_commit_hash() {
    let version = build_version();
    let mut parts = version.split('+');
    let semver = parts.next().unwrap();
    let hash = parts.next().unwrap();
    assert!(parts.next().is_none());
    assert!(!semver.is_empty());
    assert!(hash.len() >= 7);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger test_cli_help_uses_project_positioning_title --bin ckbadger
cargo test -p ckbadger test_build_version_uses_semver_plus_short_commit_hash --bin ckbadger
```

Expected:

- The help-title test fails because the old help text is still present.
- The version-format test fails because no compile-time build version helper exists yet.

**Step 3: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "test: cover cli help and build version metadata"
```

### Task 2: Implement build-time version generation and help text update

**Files:**

- Create: `crates/cli/build.rs`
- Modify: `crates/cli/src/main.rs`
- Test: `crates/cli/src/main.rs`

**Step 1: Write minimal implementation**

- Create `crates/cli/build.rs` that:
  - reads `CARGO_PKG_VERSION`
  - runs `git rev-parse --short HEAD`
  - validates the result is non-empty
  - exports `CKBADGER_BUILD_VERSION=<semver>+<short-hash>`
  - emits the minimal `cargo:rerun-if-changed` hints needed for Git metadata
- In `crates/cli/src/main.rs`:
  - add a `build_version()` helper returning `env!("CKBADGER_BUILD_VERSION")`
  - set the root clap command `version = env!("CKBADGER_BUILD_VERSION")`
  - update the root `about` string

**Step 2: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger test_cli_help_uses_project_positioning_title --bin ckbadger
cargo test -p ckbadger test_build_version_uses_semver_plus_short_commit_hash --bin ckbadger
```

Expected: PASS

**Step 3: Run focused regression check**

Run:

```bash
cargo test -p ckbadger --bin ckbadger
```

Expected: PASS

**Step 4: Commit**

```bash
git add crates/cli/build.rs crates/cli/src/main.rs
git commit -m "feat: add cli build version metadata"
```
