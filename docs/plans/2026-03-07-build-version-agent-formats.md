# Build Version In Footer And Agent Formats Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reuse the existing compile-time build version string across the frontend footer, markdown output, raw output, and discovery metadata without introducing a second calculation path.

**Architecture:** Keep the single source of truth in the CLI build metadata, pass that value into the frontend server config, inject it once through `/runtime-config.js`, and reuse a shared frontend runtime-config helper everywhere else. Agent-facing `.md` and `.raw` outputs gain `buildVersion` inside their structured meta sections rather than free-text duplication.

**Tech Stack:** Rust, Axum, clap build metadata, TypeScript, React, Vitest

---

### Task 1: Add failing tests for runtime-config and footer version exposure

**Files:**

- Modify: `crates/api/src/entry.rs`
- Modify: `frontend/__tests__/lib/runtime-config.test.ts`
- Modify: `frontend/__tests__/components/site-footer.test.tsx`

**Step 1: Write the failing tests**

Add/update tests that assert:

```rust
assert!(text.contains("buildVersion"));
assert!(text.contains("\"0.1.0+feature/foo@abcdef123456\""));
```

and:

```ts
expect(resolveBuildVersion({ buildVersion: '0.1.0+feature/foo@abcdef123456' })).toBe(
  '0.1.0+feature/foo@abcdef123456'
);
expect(screen.getByText('0.1.0+feature/foo@abcdef123456')).toBeInTheDocument();
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-api test_frontend_runtime_config_route_uses_service_config --lib
cd frontend && pnpm test -- --run frontend/__tests__/lib/runtime-config.test.ts frontend/__tests__/components/site-footer.test.tsx
```

Expected:

- Rust runtime-config test fails because `buildVersion` is not injected yet.
- Frontend tests fail because no build-version helper exists and footer does not render the version.

**Step 3: Write minimal implementation**

- Add `build_version: String` to `FrontendServiceConfig`.
- Pass the CLI `BUILD_VERSION` into `FrontendServiceConfig`.
- Inject `buildVersion` into `/runtime-config.js`.
- Extend `CkbadgerRuntimeConfig` and add `resolveBuildVersion(...)`.
- Render the version in `SiteFooter`.

**Step 4: Run tests to verify they pass**

Run the same commands from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/cli/src/main.rs crates/api/src/entry.rs frontend/lib/runtime-config.ts frontend/components/layout/site-footer.tsx frontend/__tests__/lib/runtime-config.test.ts frontend/__tests__/components/site-footer.test.tsx
git commit -m "feat(frontend): expose build version in runtime config"
```

### Task 2: Add failing tests for markdown and raw meta version fields

**Files:**

- Modify: `frontend/__tests__/lib/markdown-format.test.ts`
- Modify: `frontend/__tests__/lib/markdown-renderer.test.ts`
- Modify: `frontend/__tests__/lib/raw-renderer.test.ts`

**Step 1: Write the failing tests**

Add/update tests that assert:

```ts
expect(output).toContain('buildVersion: "0.1.0+feature/foo@abcdef123456"');
expect(result.body).toContain('buildVersion:');
expect(result.body.meta.buildVersion).toBe('0.1.0+feature/foo@abcdef123456');
```

where tests inject runtime config explicitly or stub `window.__CKBADGER_RUNTIME_CONFIG__`.

**Step 2: Run tests to verify they fail**

Run:

```bash
cd frontend && pnpm test -- --run frontend/__tests__/lib/markdown-format.test.ts frontend/__tests__/lib/markdown-renderer.test.ts frontend/__tests__/lib/raw-renderer.test.ts
```

Expected:

- Markdown tests fail because `MarkdownDocMeta` and renderer output do not include `buildVersion`.
- Raw test fails because `RawMeta` has no `buildVersion`.

**Step 3: Write minimal implementation**

- Extend `MarkdownDocMeta` and frontmatter emission with `buildVersion`.
- Extend markdown renderer meta construction to include shared build version.
- Extend `RawMeta` and `buildMeta(...)` with shared build version.
- Keep version text only in structured meta, not duplicated in page body.

**Step 4: Run tests to verify they pass**

Run the same command from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/lib/ai/markdown-format.ts frontend/lib/ai/markdown-renderer.ts frontend/lib/ai/raw-renderer.ts frontend/__tests__/lib/markdown-format.test.ts frontend/__tests__/lib/markdown-renderer.test.ts frontend/__tests__/lib/raw-renderer.test.ts
git commit -m "feat(ai): add build version to markdown and raw meta"
```

### Task 3: Update capability/discovery metadata and verify end-to-end behavior

**Files:**

- Modify: `frontend/lib/ai/capabilities.ts`
- Modify: `frontend/__tests__/lib/capabilities.test.ts`
- Modify: `frontend/public/llms.txt`
- Modify: `frontend/public/llms-full.txt`

**Step 1: Write the failing tests**

Add/update capability tests to assert the payload documents `buildVersion` availability for markdown/raw metadata.

**Step 2: Run tests to verify they fail**

Run:

```bash
cd frontend && pnpm test -- --run frontend/__tests__/lib/capabilities.test.ts
```

Expected: FAIL because capability/discovery metadata does not describe build-version fields yet.

**Step 3: Write minimal implementation**

- Update `buildAiCapabilities(...)` to describe where agents find `buildVersion`.
- Update `llms.txt` and `llms-full.txt` so markdown and raw discovery text mention `buildVersion`.

**Step 4: Run tests to verify they pass**

Run:

```bash
cd frontend && pnpm test -- --run frontend/__tests__/lib/capabilities.test.ts frontend/__tests__/lib/runtime-config.test.ts frontend/__tests__/lib/markdown-format.test.ts frontend/__tests__/lib/markdown-renderer.test.ts frontend/__tests__/lib/raw-renderer.test.ts frontend/__tests__/components/site-footer.test.tsx
cargo test -p ckbadger-api test_frontend_runtime_config_route_uses_service_config --lib
```

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/lib/ai/capabilities.ts frontend/__tests__/lib/capabilities.test.ts frontend/public/llms.txt frontend/public/llms-full.txt
git commit -m "docs(ai): describe build version in agent formats"
```
