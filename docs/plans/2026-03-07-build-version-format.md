# Build Version Branch-Aware Format Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Change the single compile-time buildVersion string to include branch context as `<semver><+branch>@<short-hash>`, omitting `+main`.

**Architecture:** Extract the formatter rule into one shared pure helper so `build.rs` remains the only place that computes the final string, but tests can cover both `main` and non-`main` behavior without depending on the current checkout branch. All consumers continue to reuse the resulting `CKBADGER_BUILD_VERSION` unchanged.

**Tech Stack:** Rust build scripts, clap metadata, Axum runtime-config tests, Vitest

---

### Task 1: Add failing tests for the new version format contract

**Files:**

- Modify: `crates/cli/src/main.rs`
- Test: `crates/cli/src/main.rs`

**Step 1: Write the failing tests**

Add tests that assert:

```rust
assert_eq!(
    build_version_format::format_build_version("0.1.0", "main", "abcdef123456"),
    "0.1.0@abcdef123456"
);

assert_eq!(
    build_version_format::format_build_version("0.1.0", "feature/foo", "abcdef123456"),
    "0.1.0+feature/foo@abcdef123456"
);
```

and update the runtime CLI version test to require `@` and forbid `+main@`.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger test_cli_version --bin ckbadger
```

Expected:

- The pure formatter test fails to compile or fails because the formatter does not exist yet.
- The runtime version test fails because the current version string still uses `+<hash>` instead of `@<hash>`.

**Step 3: Write minimal implementation**

- Add a shared pure formatter helper under `crates/cli/src/`.
- Use it from `build.rs`.
- Fetch branch name in `build.rs` and format according to the new rule.

**Step 4: Run test to verify it passes**

Run the same command from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/cli/build.rs crates/cli/src/main.rs crates/cli/src/build_version_format.rs
git commit -m "feat(cli): include branch in build version"
```

### Task 2: Update propagation tests and docs to the new example format

**Files:**

- Modify: `crates/api/src/entry.rs`
- Modify: `frontend/__tests__/lib/runtime-config.test.ts`
- Modify: `frontend/__tests__/components/site-footer.test.tsx`
- Modify: `frontend/__tests__/lib/markdown-format.test.ts`
- Modify: `frontend/__tests__/lib/markdown-renderer.test.ts`
- Modify: `frontend/__tests__/lib/raw-renderer.test.ts`
- Modify: `docs/plans/2026-03-07-build-version-agent-formats-design.md`
- Modify: `docs/plans/2026-03-07-build-version-agent-formats.md`

**Step 1: Write/update the tests**

Switch sample strings from:

```text
0.1.0+feature/foo@abcdef123456
```

to:

```text
0.1.0+feature/foo@abcdef123456
```

so tests also prove branch names are preserved verbatim through runtime-config, footer, markdown, and raw outputs.

**Step 2: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-api test_frontend_runtime_config_route_uses_service_config --lib
cd frontend && pnpm test -- --run __tests__/lib/runtime-config.test.ts __tests__/components/site-footer.test.tsx __tests__/lib/markdown-format.test.ts __tests__/lib/markdown-renderer.test.ts __tests__/lib/raw-renderer.test.ts
```

Expected: PASS

**Step 3: Commit**

```bash
git add crates/api/src/entry.rs frontend/__tests__/lib/runtime-config.test.ts frontend/__tests__/components/site-footer.test.tsx frontend/__tests__/lib/markdown-format.test.ts frontend/__tests__/lib/markdown-renderer.test.ts frontend/__tests__/lib/raw-renderer.test.ts docs/plans/2026-03-07-build-version-agent-formats-design.md docs/plans/2026-03-07-build-version-agent-formats.md
git commit -m "test: update build version examples for branch format"
```

### Task 3: Verify full affected surface

**Files:**

- Modify: none unless verification exposes an issue

**Step 1: Run focused verification**

Run:

```bash
cargo test -p ckbadger --bin ckbadger
cargo test -p ckbadger-api --test frontend_server_spa
cd frontend && pnpm test -- --run __tests__/lib/capabilities.test.ts __tests__/lib/runtime-config.test.ts __tests__/components/site-footer.test.tsx __tests__/lib/markdown-format.test.ts __tests__/lib/markdown-renderer.test.ts __tests__/lib/raw-renderer.test.ts
cd frontend && pnpm type-check
cd frontend && pnpm lint
```

Expected: PASS

**Step 2: Commit**

```bash
git commit --allow-empty -m "chore: verify branch-aware build version format"
```
