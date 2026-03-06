# Frontend Bundle Optimization Design

**Date:** 2026-03-06

## Context

The frontend runtime is now fully decoupled from `next/*` and builds as a pure `Vite + React Router` SPA. The remaining obvious frontend issue is bundle size: the main entry chunk is still roughly `813 kB` after minification, and Vite warns that the entry bundle is oversized.

The goal of this phase is not to silence a warning mechanically. The goal is to reduce initial JS cost for the local explorer UI by moving genuinely heavy code off the main path.

## Goal

Reduce the main frontend entry bundle by lazily loading heavy visualization components and other clearly non-essential-at-startup UI code.

## Non-Goals

- Do not redesign page UX.
- Do not rewrite the route tree structure in this phase.
- Do not change API contracts.
- Do not optimize every page indiscriminately.
- Do not start with Rollup `manualChunks` as the primary strategy.

## Constraints

- Preserve current route behavior and page semantics.
- Accept a brief loading fallback for heavy visualizations.
- Keep the implementation local and explicit in app code, not hidden in opaque bundler rules.
- Keep validation grounded in actual bundle output and route/test behavior.

## Recommended Approach

Use real component-level lazy loading around the heaviest runtime dependencies first, especially the `react-force-graph-2d` path and similar heavy graph/visualization entry points.

This is preferable to bundler-only chunk splitting because:

- it directly reduces what the app executes on first load
- it keeps the loading boundary visible in code
- it is easier to reason about and test
- it composes cleanly with later route-level splitting if needed

## Scope Boundary

This phase focuses only on:

- heavy graph components
- clearly heavy visualization sections that are not needed for every first paint
- route-local heavy UI blocks where lazy fallback is acceptable

This phase does not yet do broad route-level code splitting across the entire explorer.

## Candidate Targets

### Highest Priority

- `frontend/components/cell-graph.tsx`
- `frontend/components/proposal-graph.tsx`
- anything else that directly pulls `react-force-graph-2d`

These are the most obvious first targets because they pull a large graphing dependency and are not needed for every initial render.

### Secondary Candidates

- graph-heavy detail tabs
- chart sections that are only visible on specific routes
- any visualization component that brings large vendor code into the main entry

## Two-Phase Execution Shape

### Phase A: Real Lazy Boundaries

- Identify the heaviest contributors to the entry chunk.
- Wrap them in explicit lazy boundaries with clear loading fallbacks.
- Ensure the visual fallback is acceptable and deterministic.
- Verify key routes and graph pages still work.

### Phase B: Measure and Decide

- Rebuild and compare the output bundle sizes.
- If the main entry chunk drops enough, stop here.
- If it remains too large, follow up with selective route-level lazy loading for low-frequency pages.

## Why Not Start With `manualChunks`

`manualChunks` is not the right first move because it can rearrange files without meaningfully reducing startup execution cost. It is useful as a secondary bundler-tuning tool, but not as the primary architecture decision.

For this codebase, explicit lazy boundaries are more local-first and more agent-friendly.

## Risks

### Risk: loading fallback harms UX

Mitigation:

- only lazy-load clearly heavy, non-essential-at-startup components
- provide stable loading placeholders with roughly correct dimensions

### Risk: route regressions on detail pages

Mitigation:

- keep route/detail regression tests
- verify graph-related pages after each split

### Risk: minimal size improvement

Mitigation:

- measure after Phase A
- if the main entry is still too large, escalate to selective route-level splitting

## Validation Targets

This phase is successful when:

- `pnpm build` still passes
- graph/detail regression tests still pass
- the main entry chunk is meaningfully smaller than the current baseline
- heavy graph code is no longer eagerly included in the main startup path

## Follow-Up

If the entry chunk remains too large after heavy-component splitting, the next step is targeted route-level lazy loading for less frequently visited explorer pages rather than global bundler chunk heuristics.
