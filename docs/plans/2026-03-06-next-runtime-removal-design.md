# Next Runtime Removal Design

**Date:** 2026-03-06

**Context**

The frontend shell has already moved from `Next.js static export` to `Vite + React SPA + Rust frontend server`, but the business/component layer still imports `next/link`, `next/navigation`, `next/image`, and `next/dynamic`. Those imports are currently bridged through Vite aliases and local compat shims.

That keeps the app running, but it leaves the wrong runtime boundary in place:

- production still appears to depend on `next/*` APIs
- `vite.config.ts` must translate Next module names at build time
- `frontend/package.json` still carries `next` and `eslint-config-next`
- future route and page cleanup is harder because business code is still written against the old shell

This phase removes those remaining runtime assumptions.

## Goal

Remove production runtime dependence on `next/*` from the frontend codebase, so the shipped frontend is unambiguously a `Vite + React Router` SPA served by the Rust frontend process.

## Non-Goals

- Do not rewrite the full app directory structure in one pass.
- Do not redesign page UX or API contracts.
- Do not optimize bundle splitting in this phase.
- Do not migrate every `frontend/app/**` file into a new folder layout unless needed by the dependency cleanup.

## Constraints

- Keep the current three-process runtime: `indexer`, `api`, `frontend`.
- Keep production free of Node runtime requirements.
- Preserve current detail-route behavior and SPA fallback behavior.
- Minimize churn in tested page/business components.

## Recommended Approach

Use local frontend abstractions as the new runtime boundary, then replace every runtime `next/*` import with those abstractions.

This is better than keeping long-lived compat aliases because:

- it makes the true architecture visible in the code
- it allows `next` and `eslint-config-next` to be removed cleanly
- it reduces future migration work to normal React refactors instead of dual-stack cleanup

## Architecture Boundary

After this phase:

- runtime navigation comes from a local module such as `frontend/src/navigation.ts`
- runtime links come from a local link component, not `next/link`
- runtime image rendering comes from a local image component, not `next/image`
- lazy client-only loading uses a local utility, not `next/dynamic`
- `frontend/package.json` no longer needs `next` or `eslint-config-next`
- `frontend/vite.config.ts` no longer needs `next/*` alias entries

`frontend/app/**` may still exist temporarily, but only as page wrappers or route-adapter files. It must not be the place where production runtime semantics come from.

## Replacement Strategy

### Navigation

Create a local navigation module that exports the equivalents currently used by the app:

- `useRouter`
- `usePathname`
- `useSearchParams`
- `useParams`
- `redirect`
- `notFound`

Implementation should be based on `react-router-dom` and existing SPA behavior, not on Next emulation.

### Link

Promote the existing local client-side link behavior into the canonical link abstraction and replace `next/link` imports across runtime code.

This avoids sprinkling raw `react-router-dom` links everywhere and keeps a stable app-level navigation API.

### Image

Replace `next/image` with a local image component that supports only the subset of behavior the codebase actually uses.

This phase does not attempt to replicate Next image optimization. The correct local-first behavior is simple image rendering with predictable props.

### Dynamic Client-Only Loading

Replace `next/dynamic` usage with a local helper built around React lazy-loading or direct client-only imports for the small number of affected graph components.

The goal is explicit client-only behavior without preserving Next semantics as a dependency.

## Execution Shape

### Phase A: Remove runtime `next/*` imports

- Replace runtime imports in components and page clients with local abstractions.
- Update tests that currently mock `next/navigation` to mock the local navigation module instead.
- Remove `next/*` aliases from `vite.config.ts`.
- Remove `next` and `eslint-config-next` from `frontend/package.json`.
- Update lint config so it no longer relies on Next-specific config packages.

### Phase B: Tighten the transition boundary

- Review remaining `frontend/app/**` files.
- Keep thin wrappers where they still reduce churn.
- Move or simplify wrappers that still carry real runtime logic.
- Confirm the remaining `app/**` tree is only transitional structure, not a hidden runtime dependency.

## Risks

### Risk: test churn from mocked navigation

Many tests currently mock `next/navigation`.

Mitigation:

- replace mocks mechanically with the local navigation module
- keep the exported API shape intentionally close to the currently used call sites

### Risk: accidental routing regressions

Detail pages and cursor/query handling are sensitive to navigation semantics.

Mitigation:

- keep the existing route tests
- keep detail page regression coverage
- keep Rust SPA fallback tests

### Risk: local abstraction becomes another compatibility trap

Mitigation:

- design the local modules around actual current usage
- do not mirror the full Next API surface
- remove Vite aliasing in the same phase, so the code must use the new boundary directly

## Validation Targets

This phase is complete when all of the following are true:

- `rg "from 'next/|from \"next/" frontend` returns only test fixtures/migration leftovers intended to remain, or nothing
- `frontend/package.json` does not contain `next` or `eslint-config-next`
- `frontend/vite.config.ts` does not alias `next/*`
- `pnpm lint` passes
- `pnpm type-check` passes
- `pnpm build` passes
- route/detail regression tests pass
- Rust frontend server SPA fallback tests still pass

## Follow-Up

Once runtime `next/*` dependence is gone, the next logical phase is bundle and route-structure cleanup:

- code-split the oversized SPA bundle
- decide whether `frontend/app/**` should stay as a transitional shell or be collapsed into `src/routes/**`
