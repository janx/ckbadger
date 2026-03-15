# Warmup Cache Pending Design

## Problem

Pages backed by API warmup caches can render a misleading empty or generic error state during startup. The API currently reports cache misses as `internal_error`, and the frontend cannot distinguish "cache is still warming up" from a genuine server failure or an actual empty dataset.

## Goals

- Make warmup cache unavailability explicit and machine-readable.
- Show a clear "data is being prepared" message on affected pages.
- Automatically retry data fetching until the cache becomes ready.
- Preserve real empty states and real errors without masking them.

## Non-Goals

- Do not introduce browser-level page reloads.
- Do not add fallback data paths that bypass the warmup cache contract.
- Do not change the underlying cache warmup architecture in this pass.

## Design

### API Contract

- Add a dedicated `warmup_pending` API error variant using HTTP `503 Service Unavailable`.
- Use this variant for routes that already report `"... cache unavailable; warmup in progress"`.
- Keep the message human-readable, but rely on the stable `error` field for frontend behavior.

### Frontend Behavior

- Parse API error payloads into a structured error object that preserves HTTP status and API error code.
- Add a shared warmup-aware query helper for TanStack Query.
- When a query fails with `warmup_pending`, show a shared info state and automatically refetch on a short interval.
- Stop retrying as soon as the query succeeds or fails with a non-warmup error.
- Preserve existing empty-state rendering when the query succeeds with `data: []`.

### Scope

- Backend routes backed by warmup caches:
  - scripts
  - assets / tokens / NFT asset routes
  - top/active addresses
  - spore cache-backed listings
  - search warmup-backed lookups
  - any other route currently returning `"... cache unavailable; warmup in progress"`
- Frontend pages using those routes should adopt the shared warmup-aware query handling.

## Testing

- API regression tests for `503 + warmup_pending`.
- Frontend unit tests for API error parsing and warmup detection.
- Frontend page-level regression test proving warmup message appears and is replaced after a successful retry.
