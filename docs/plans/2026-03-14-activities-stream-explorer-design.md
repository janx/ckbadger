# Activities Stream Explorer Design

**Date**: 2026-03-14
**Status**: Approved
**Scope**: API pagination/filtering + frontend `/activities` stream explorer

## Goal

Turn `/activities` from a resized homepage widget into a dedicated global activity explorer that:

- stays stream-like instead of becoming a table
- supports infinite scroll for older canonical activities
- supports lightweight top-level filters
- does not interrupt readers when new activities arrive while they are browsing history

## Principle Alignment

- **CKB Native**: preserve activity rows as owner-centric interpretations of canonical chain changes, with activity badges and details that expose CKB/DAO/token/object/identity/script/protocol semantics.
- **Local First**: rely on deterministic API reads over canonical indexed data; no new background jobs or non-chain caches.
- **Agent Friendly**: separate the homepage latest snapshot widget from the dedicated explorer so each surface has a single purpose and a simpler state model.

## Current Problems

1. `/activities` is currently just `LatestActivities` stretched into a page, so it still behaves like a homepage widget instead of a browseable explorer.
2. The current stream rows wrap the entire row in a tx link while also rendering nested links for addresses, assets, and scripts, producing invalid nested anchors and hydration warnings.
3. The current endpoint `GET /activities/latest?limit=64` is a latest-window snapshot, not a pageable history API. It cannot support true infinite scroll.
4. There is no page-level empty state, error state, or loading-more state.
5. There are no page-level filters, so users cannot narrow the stream by activity class.

## Product Decision

`/activities` remains a **stream**, not a table and not a dashboard.

The page should feel like a live feed of canonical CKB activity:

- newest items at the top
- older items loaded by scrolling down
- explicit quick links to tx/block/address/detail pages
- lightweight filters, not a heavy query builder
- new-activity buffering when the user is away from the top

The homepage `LatestActivities` component remains a separate latest snapshot widget and is not the owner of `/activities` behavior.

## UX Design

### Page Structure

1. Existing `Header`
2. `PageHeader`
3. Sticky filter bar
4. Conditional "N new activities" notice
5. Stream list with date separators
6. Bottom load status area

### Page Header

Keep the existing `PageHeader` component, but use page-specific copy that reflects the real behavior of the explorer.

Recommended subtitle:

`Live canonical activity stream across CKB, with infinite scroll into older history`

### Sticky Filter Bar

The filter bar is pinned below the page header while the user scrolls. It exposes these public filters:

- `All`
- `CKB`
- `Token`
- `Object`
- `Identity`
- `DAO`
- `Script`
- `Protocol`

These are explorer-level buckets, not low-level internal storage categories.

`Script` is intentionally a merged public category that includes both type-call and lock-call style script activity. We keep the UI simple even if the backend internally maps this to multiple lower-level predicates.

### Stream Row Layout

Each row keeps the stream feel and uses a compact three-layer information hierarchy.

**Row line 1**

- headline badge
- relative timestamp
- explicit quick links for tx and block

**Row line 2**

- owner address
- primary value/change

**Row line 3 (optional)**

- secondary CKB delta
- asset/script/protocol detail
- supporting metadata only when it adds meaning

Rows should not be fully wrapped in a link. Navigation must be explicit through child links to avoid invalid anchor nesting and to preserve precise click targets.

### Date Separators

The stream gets light chronological separators:

- `Today`
- `Yesterday`
- absolute dates like `Mar 12`

These are reading anchors only, not a timeline control.

### New Activity Handling

There are two independent flows:

- **older history flow**: append older pages to the bottom
- **new head flow**: poll the head for fresher activities

If the user is near the top:

- prepend new activities immediately
- keep the existing subtle new-item highlight

If the user is not near the top:

- do not mutate the visible list immediately
- accumulate results in `pendingNewItems`
- show a sticky notice like `12 new activities`
- on click, merge those items at the top and scroll the user back to the head

This preserves the meaning of the feed:

- top = now
- bottom = older history

### Bottom States

The stream footer shows one of:

- `Loading older activities...`
- `End of stream`
- `Failed to load older activities`

Top refresh failure should not clear the existing list. It should degrade softly while preserving visible content.

## Data Model and API Design

## Existing API Boundary

Current latest endpoint:

- `GET /activities/latest?limit=...`
- clamped to 64
- returns only a latest snapshot window

This endpoint remains owned by the homepage latest widget.

## New API Requirement

Add a new global canonical activities pagination API for the explorer page.

Recommended shape:

`GET /activities`

Supported params:

- `limit`
- `cursor`
- `filter`

Response shape:

```json
{
  "data": [...],
  "nextCursor": "opaque-cursor-or-null",
  "hasMore": true
}
```

### Filter Contract

Public filter values:

- `all`
- `ckb`
- `token`
- `object`
- `identity`
- `dao`
- `script`
- `protocol`

The API should own the mapping from these public buckets to canonical activity predicates. The frontend should not reconstruct protocol/script meaning client-side from mixed raw rows.

### Canonicality Rules

The new endpoint must keep the same canonical filtering guarantees as the existing latest endpoint and address activities endpoint:

- only canonical rows
- no fallback calculation chain
- one consistent derivation path

## Frontend Architecture

Create a dedicated page-level component for the explorer rather than stretching `LatestActivities`.

Recommended ownership split:

- homepage widget stays in `frontend/components/latest-activities.tsx`
- explorer page gets a dedicated container component, for example `frontend/components/activities-stream-page.tsx`
- shared badge/render helpers continue to live in reusable activity UI helpers

This keeps the widget and explorer state machines separate:

- homepage widget: latest snapshot + small visual pulse
- explorer page: filters + infinite scroll + pending new buffer + load-more lifecycle

## State Flow

Explorer page state:

- `activeFilter`
- `visibleItems`
- `olderCursor`
- `pendingNewItems`
- `isAtTop`
- `isLoadingOlder`
- `olderLoadError`
- `headRefreshError`

Behavior:

1. initial load fetches newest page for the active filter
2. scrolling near bottom loads the next page and appends
3. polling fetches the newest page for the active filter
4. dedupe is done by stable global activity identity
5. filter change resets the entire stream state and scrolls to top

## Testing Strategy

### Backend

- API integration test for `/api/v1/activities`
- cursor pagination test
- canonical filtering test
- filter mapping tests for all public filter values

### Frontend

- route/page render test for `/activities`
- filter switching test
- infinite scroll append test
- pending-new-items banner test
- empty state test
- error state test
- regression test proving rows no longer render nested anchors

## Scope

### Files Likely Touched

- `crates/api/src/routes/activities.rs`
- `crates/api/tests/api_integration.rs`
- `frontend/lib/api.ts`
- `frontend/app/activities/page.tsx`
- `frontend/components/latest-activities.tsx` only if extracting shared row helpers is worthwhile
- new explorer-specific component(s), likely under `frontend/components/`
- frontend route/page tests

### Storage Impact

- No RocksDB schema changes
- No indexer write-path changes
- API remains read-only

## Validation

- `cargo test -p ckbadger-api`
- targeted API integration tests for the new `/activities` endpoint
- `cd frontend && pnpm test -- --run <targeted activities tests>`
- `cd frontend && pnpm type-check`
- `cd frontend && pnpm lint`

## Result

This design keeps `/activities` stream-like while making it a real explorer:

- infinite history browsing
- live head updates without scroll disruption
- lightweight filters
- explicit navigation targets
- clearer ownership boundary between homepage widget and explorer page
