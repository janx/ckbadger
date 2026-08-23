# Network Peers UI — Design Spec (Plan 2: Surfacing)

_Date: 2026-07-02 · Status: Implemented (design record) · Depends on: `docs/NETWORK_PEER_CRAWLER.md`_

> Plan boundaries and “what next” notes are retained as historical rationale. Current route and
> response contracts live in `docs/API.md`.

Surface the whole-network CKB L1 peer-crawler data (the `network` store built in Plan 1) as a
read-only `/network` API and a frontend **"Peers"** dashboard: summary, distributions, historical
trends, and a filterable node table.

## Goal

- Turn the observations the crawler writes to `CF_NET_NODES` / `CF_NET_STATS` and the exact active
  progress in `CF_NET_CRAWL` into a usable,
  honest dashboard, without changing the crawler or the chain indexer/API.
- Deliverables: **summary cards, distributions, trend charts, node table** — plus the `/network/*`
  read API that backs them.

## Principle Alignment

- **Local First** — read-only surfacing of local crawler data; the API opens the network store as a
  secondary (read-only), never writing. Poll-based UI (no push infra needed for ~15-min-cadence data).
- **CKB Native / store boundary** — the API reads the third `network` store via a new secondary
  handle; it never writes it (the crawler is the sole writer). No chain-store changes.
- **Fail Fast vs "no data"** — a store read failure is a real error surfaced per-section; but the
  crawler being **off or not-yet-run** is a NORMAL state (`hasData:false`), rendered as onboarding,
  never an error.
- **Single Calculation Path** — current distributions are computed exactly one way: scan
  `CF_NET_NODES`. Trends come from the already-materialised `CF_NET_STATS` history buckets.
- **Numerical Precision** — counts are reported as-observed (reachable/total from the store); no
  extrapolation. `reachable` is meaningful (Plan 1's fix downgrades not-seen nodes to
  `reachable=false`), so `reachable ≤ totalKnown`.

## Non-Goals (explicit scope boundaries)

- **No topology graph** — deferred to **Plan 3** (reachability/gossip graph, react-force-graph).
- **No TUI** — the crawler status line/panel in the TUI is a later pass.
- **No per-node detail page** — the node table has no row-detail route (a point API endpoint exists
  for agents/API only).
- **No choropleth map** — country distribution is a bar/list in Plan 2 (a map pairs with the Plan 3
  graph work).
- **No WebSocket push** — the UI polls (network data changes on the crawl cadence, minutes).

## Background

The `network` store class exposes `CF_NET_NODES` → published `NodeRecord`, `CF_NET_STATS` → the
completed `LatestStatus` singleton and history, and `CF_NET_CRAWL` → durable active-round state.
The API reads all three through a secondary; it never writes the store.

---

## Architecture

- **API** (`crates/api`):
  - `AppState` (`crates/api/src/lib.rs`) holds a hot-swappable read-only network-store secondary
    plus `crawler_enabled: bool` (from `[crawler].enabled`). The slot is empty until the opt-in
    crawler creates `data/network`.
  - `entry.rs` attempts the secondary open before router construction and retries in the
    background when the primary is absent or still being upgraded. Once available, it installs
    the secondary atomically; API restart is not required. Repeated identical open failures are
    logged once rather than once per retry.
  - The existing secondary-refresh loop loads the current slot and calls `network.refresh()` when
    attached, so `/network` reflects new crawl rounds automatically.
  - New `crates/api/src/routes/network.rs` with `routes()` merged into `api_routes()`
    (`routes/mod.rs`). Handlers follow the existing pattern:
    `async fn h(State(state): State<Arc<AppState>>, ...) -> ApiResult<Json<T>>`.
- **Frontend** (`frontend`):
  - New page `frontend/app/network/` (`page.tsx` wrapper + `client-page.tsx` client component),
    matching the codebase's dynamic-route split.
  - New methods on the `api` object in `frontend/lib/api.ts` + `camelCase` response types.
  - TanStack Query v5 with a poll interval (30–60s). The page and command-palette label is
    **"Peers"** (see Naming below).

### Naming

The frontend already uses "**Network**" for **chain** health (`NetworkHealth` widget: tip/tps). To
avoid confusion, the page/command label is **"Peers"** (the p2p node network), while the backend
stays at `/network/*` and the `network` store name (already shipped). The page is available through
the `g p` shortcut and command palette rather than a permanent navbar item. This is a label choice
only; no backend rename.

---

## API Endpoints (`/network/*`, read-only, `camelCase` via serde)

All read from `state.network_store`. When its slot is empty (crawler never ran): `/summary` returns
`hasData:false`; the others return empty results (`[]` / empty page / empty series) — **not** errors.

### `GET /network/summary`

Drives the onboarding-vs-dashboard switch.

```jsonc
NetworkSummary {
  enabled: bool,
  hasData: bool,                      // a completed status exists
  lastRound: LatestStatus | null,     // CF_NET_STATS 0x00, completed only
  activeRound: ActiveCrawl | null     // exact CF_NET_CRAWL progress, separate from lastRound
}
LatestStatus {
  roundId, startedAt, finishedAt,
  candidatePeers, attemptedPeers, reachablePeers, unreachablePeers,
  addressAttempts, failedAddressAttempts, foreignPeers, malformedAddresses,
  newNodes, totalKnown
}
ActiveCrawl {
  roundId, startedAt, lastCheckpointAt,
  candidatePeers, completedPeers, addressAttempts, blockedReason
}
```

Peer-level and address-attempt-level counters have different, explicit names. `activeRound` never
overwrites the completed snapshot; consumers can show progress while continuing to render the
last internally consistent `lastRound`.

### `GET /network/distributions`

Single calculation path: scan `CF_NET_NODES`, aggregate in-memory (node count is small).

```jsonc
NetworkDistributions {
  totalKnown, reachable, unreachable,          // reachable = count(reachable==true); unreachable = totalKnown - reachable
  versions:  [{ label, count }],               // top-N by count, desc
  countries: [{ label, count }],               // geo=None OR empty country ("") both grouped as "Unknown"
  asns:      [{ label, count }],               // label "AS{num} {org}"; asn=None ⇒ "Unknown"
  protocols: [{ label, count }]                // per successfully opened probe protocol
}
```

### `GET /network/history?metric=&granularity=&from=&to=`

Range-scan `CF_NET_STATS` buckets (`scan_history`). `metric` ∈ {`totalNodes`,`reachableNodes`,
`versionShare`,`countryShare`}; `granularity` ∈ {`hour`,`day`}; `from`/`to` are unix seconds
(mapped to bucket indices). Daily series EXCLUDE the incomplete current day (codebase gotcha).

```jsonc
NetworkHistory {
  metric, granularity,
  points: [{ ts, scalar, buckets: [{label,count}] }]  // scalar for count metrics; buckets for share metrics
}
```

### `GET /network/nodes?cursor=&limit=&reachable=&country=&version=`

Filterable, cursor-paginated node table. Handler scans `CF_NET_NODES`, applies filters, sorts by
`lastSeen` desc then `peerId`, slices by cursor (cursor = last item's `peerId` hex; node set is
small so an in-memory scan+slice is fine).

```jsonc
NetworkNodesPage {
  items: [NodeSummary],
  nextCursor: string | null
}
NodeSummary {
  peerId,              // hex
  addr,                // primary own_addr (first), else ""
  version, country, asn, reachable,
  lastSeen, lastReachableAt, rttMs
}
```

### `GET /network/nodes/{peerId}` (optional, API/agent only — no frontend page)

Full `NodeRecord` (camelCase) for `peerId` hex, or `ApiError::not_found`. Included for
agent-friendliness (llms.txt ethos); cut it if undesired.

---

## Frontend — "Peers" page (`frontend/app/network/`)

`client-page.tsx` fetches `summary` first and switches:

- **`enabled=false` OR `hasData=false` → Onboarding empty state.** A panel that explains the peer
  crawler (whole-network CKB L1 node discovery; local-first — you run it, the data is yours), states
  the honesty caveat (**discoverable/reachable nodes only**, not the full hidden network), and shows
  how to enable it: set `[crawler].enabled = true` in that network's `config.toml` (+ optional
  `geoip_city_path` / `geoip_asn_path` for geo/ASN), noting it does outbound whole-network
  crawling. When `enabled=true` but `hasData=false`, a "crawler enabled — waiting for the first
  round" variant includes exact `activeRound.completedPeers / candidatePeers` progress when an
  active checkpoint exists.
- **`hasData=true` → Dashboard**:
  1. **Summary cards** (from completed `lastRound`): discovered reachable peers / failed peer
     candidates / total known / last-round age. A separate active-round badge reports the current
     round id and exact completed/candidate progress. The detail line keeps address attempts,
     failed addresses, and foreign peers distinct. Honest labels use "discovered", never "total
     network nodes".
  2. **Distributions** (from `/distributions`): version (bar/pie), country (bar/list — no map),
     ASN top-N (bar), protocol support, reachable-vs-unreachable. Reuse existing chart components
     (`frontend/components/ui/`).
  3. **Trends** (from `/history`): line charts for node count + reachable count over time; version-
     share and country-share stacked-area over time (reuse the existing stacked-area chart type).
     Daily charts exclude the incomplete current day.
  4. **Node table** (from `/nodes`): paginated + filterable (reachable / country / version); columns
     peerId (truncated), address, version, country, ASN, reachable badge, last-seen (relative), RTT.
     No row-detail route.

- **Discovery**: no permanent navbar entry; the `g p` shortcut and command palette open the
  **"Peers"** page (the empty state covers the no-data case). **MaxMind attribution** appears in the
  page footer whenever GeoLite2-derived geo/ASN is shown.

### States & error handling

- Per-section loading skeletons (TanStack Query loading).
- A store-read failure surfaces as a per-section error card, not a full-page crash.
- "No data" is the onboarding state (driven by `summary.hasData`), NOT an error.
- Background poll refresh; a subtle "updated Xm ago" from `lastRound.finishedAt`.

---

## Testing (MANDATORY — per CLAUDE.md table)

- **API** — `crates/api/tests/api_network.rs` (per-resource convention; shared helpers in
  `crates/api/tests/common/mod.rs`):
  - The shared harness opens and seeds an isolated test network store with two `NodeRecord`s, a
    `LatestStatus`, and history buckets, then injects that handle into the read-only API state.
  - Endpoint tests cover seeded and empty states, aggregation, current-day exclusion, filters,
    pagination, point lookup, malformed peer IDs, not-found behavior, hot attachment after router
    construction, and secondary attachment when the crawler primary appears after API startup.
- **Frontend** — `frontend/__tests__/`:
  - Tests cover onboarding and per-network configuration guidance, active first-round progress,
    completed/active separation, distributions, trend query boundaries, node rows and reachability
    filtering, shortcut/command discovery, and conditional MaxMind attribution.
  - MSW handlers for `/network/*` live in `frontend/__tests__/msw/handlers.ts`; API method coverage
    lives in `frontend/__tests__/lib/network-api.test.ts`.

---

## Result

- **Behavior change** — new read-only `/network/*` API + a "Peers" frontend dashboard (summary,
  distributions, trends, node table) surfacing the crawler's `network` store. Opt-in-aware: shows
  onboarding until the crawler runs. No change to the crawler, chain indexer, or chain API.
- **Re-sync required** — no chain re-sync. The crawler schema migration itself requires a one-time
  clear/re-crawl of a pre-change development network store.
- **What to do next** — a future topology (reachability/gossip) graph would require a new,
  explicitly non-authoritative API/UX contract; it is not part of the current implementation.
