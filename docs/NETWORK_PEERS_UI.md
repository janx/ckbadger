# Network Peers UI — Design Spec (Plan 2: Surfacing)

_Date: 2026-07-02 · Status: Approved (design) · Depends on: `docs/NETWORK_PEER_CRAWLER.md` (Plan 1, merged)_

Surface the whole-network CKB L1 peer-crawler data (the `network` store built in Plan 1) as a
read-only `/network` API and a frontend **"Peers"** dashboard: summary, distributions, historical
trends, and a filterable node table.

## Goal

- Turn the observations the crawler already writes to `CF_NET_NODES` / `CF_NET_STATS` into a usable,
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

Plan 1 (merged, branch history in `feature/network-peer-crawler`) shipped: the `network` store class
(`open_network` / `open_network_secondary`, CFs `CF_NET_NODES` → `NodeRecord`, `CF_NET_STATS` →
`0x00` `LatestStatus` singleton + `[metric][gran][BE bucket]` → `HistoryPoint`); store read ops
`get_node` / `scan_nodes` / `get_network_status` / `scan_history`; the `ckbadger-crawler` service;
`[crawler]` config (`enabled=false` default). This plan only READS that store.

---

## Architecture

- **API** (`crates/api`):
  - `AppState` (`crates/api/src/lib.rs`) gains `network_store: Option<Arc<CkbadgerStore>>` and
    `crawler_enabled: bool` (from `[crawler].enabled`). The Option is `Some` only when the network
    store primary (`data/network`, i.e. `CURRENT` exists) is present — the crawler is opt-in, so on
    most deployments it is `None`.
  - `entry.rs` opens the network secondary via `CkbadgerStore::open_network_secondary(primary,
secondary)` guarded by a path-exists check; failure to open (absent/locked) ⇒ `None` + a
    `tracing::warn`, never an API startup failure.
  - Add `network.refresh()` (guarded by the Option) to the existing secondary-refresh loop
    (`crates/api/src/lib.rs:~342`) so `/network` reflects new crawl rounds automatically.
  - New `crates/api/src/routes/network.rs` with `routes()` merged into `api_routes()`
    (`routes/mod.rs`). Handlers follow the existing pattern:
    `async fn h(State(state): State<Arc<AppState>>, ...) -> ApiResult<Json<T>>`.
- **Frontend** (`frontend`):
  - New page `frontend/app/network/` (`page.tsx` wrapper + `client-page.tsx` client component),
    matching the codebase's dynamic-route split.
  - New methods on the `api` object in `frontend/lib/api.ts` + `camelCase` response types.
  - TanStack Query v5 with a poll interval (30–60s). Nav label **"Peers"** (see Naming below).

### Naming

The frontend already uses "**Network**" for **chain** health (`NetworkHealth` widget: tip/tps). To
avoid confusion, the new page's nav/UI label is **"Peers"** (the p2p node network), while the backend
stays at `/network/*` and the `network` store name (already shipped). This is a label choice only; no
backend rename.

---

## API Endpoints (`/network/*`, read-only, `camelCase` via serde)

All read from `state.network_store`. When it is `None` (crawler never ran): `/summary` returns
`hasData:false`; the others return empty results (`[]` / empty page / empty series) — **not** errors.

### `GET /network/summary`

Drives the onboarding-vs-dashboard switch.

```jsonc
NetworkSummary {
  enabled: bool,                 // [crawler].enabled
  hasData: bool,                 // network_store is Some AND get_network_status() is Some
  lastRound: LatestStatus | null // the CF_NET_STATS 0x00 singleton, camelCase
}
LatestStatus {
  roundId, started, finished, dialed, reachable, unreachable,
  foreignDropped, newNodes, totalKnown, frontierDrained
}
```

### `GET /network/distributions`

Single calculation path: scan `CF_NET_NODES`, aggregate in-memory (node count is small).

```jsonc
NetworkDistributions {
  totalKnown, reachable, unreachable,          // reachable = count(reachable==true); unreachable = totalKnown - reachable
  versions:  [{ label, count }],               // top-N by count, desc
  countries: [{ label, count }],               // geo=None OR empty country ("") both grouped as "Unknown"
  asns:      [{ label, count }],               // label "AS{num} {org}"; asn=None ⇒ "Unknown"
  protocols: [{ label, count }]                // per advertised protocol
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
  how to enable it: set `[crawler].enabled = true` in `ckbadger.toml` (+ optional `geoip_city_path` /
  `geoip_asn_path` for geo/ASN), noting it does outbound whole-network crawling. When `enabled=true`
  but `hasData=false`, a "crawler enabled — waiting for the first round" variant.
- **`hasData=true` → Dashboard**:
  1. **Summary cards** (from `lastRound`): discovered reachable / unreachable / total known /
     last-round age; a **"partial round"** badge when `!frontierDrained`. Honest labels — "discovered
     reachable nodes", never "total network nodes".
  2. **Distributions** (from `/distributions`): version (bar/pie), country (bar/list — no map),
     ASN top-N (bar), protocol support, reachable-vs-unreachable. Reuse existing chart components
     (`frontend/components/ui/`).
  3. **Trends** (from `/history`): line charts for node count + reachable count over time; version-
     share and country-share stacked-area over time (reuse the existing stacked-area chart type).
     Daily charts exclude the incomplete current day.
  4. **Node table** (from `/nodes`): paginated + filterable (reachable / country / version); columns
     peerId (truncated), address, version, country, ASN, reachable badge, last-seen (relative), RTT.
     No row-detail route.

- **Nav**: an always-visible "Peers" entry (the empty state covers the no-data case). **MaxMind
  attribution** in the page footer whenever GeoLite2-derived geo/ASN is shown.

### States & error handling

- Per-section loading skeletons (TanStack Query loading).
- A store-read failure surfaces as a per-section error card, not a full-page crash.
- "No data" is the onboarding state (driven by `summary.hasData`), NOT an error.
- Background poll refresh; a subtle "updated Xm ago" from `lastRound.finished`.

---

## Testing (MANDATORY — per CLAUDE.md table)

- **API** — `crates/api/tests/api_network.rs` (per-resource convention; shared helpers in
  `crates/api/tests/common/mod.rs`):
  - A test helper to open + seed a network secondary (extend the existing domain+append test
    harness) — put a few `NodeRecord`s + a `LatestStatus` + history buckets.
  - Per endpoint: happy path (seeded) asserting shape + aggregation correctness (e.g. reachable
    count, top-N ordering, current-day excluded); **empty store** (`hasData:false`, empty
    distributions/nodes/history — not errors); `nodes/{peerId}` not-found.
  - Boundary test: the network handle is opened **secondary/read-only** (no write path in the API).
- **Frontend** — `frontend/__tests__/`:
  - Renders onboarding when `hasData:false` / `enabled:false`; renders dashboard sections when
    `hasData:true`; node-table rows + filter interaction; trend chart excludes the incomplete current
    day; honest labels present; MaxMind attribution shown when geo present.
  - MSW handlers for `/network/*` (`frontend/__tests__/msw/handlers.ts`).
- **lib/api.ts** method tests if the codebase tests API methods (mirror existing coverage).

---

## Result

- **Behavior change** — new read-only `/network/*` API + a "Peers" frontend dashboard (summary,
  distributions, trends, node table) surfacing the crawler's `network` store. Opt-in-aware: shows
  onboarding until the crawler runs. No change to the crawler, chain indexer, or chain API.
- **Re-sync required** — **No** (read-only surfacing of existing data).
- **What to do next** — **Plan 3**: the topology (reachability/gossip) graph — react-force-graph via
  `frontend/lib/dynamic-client.tsx`, a `/network/graph` endpoint over `known_peers`, honest
  "non-authoritative" caption, node/degree caps, and a country choropleth map. Also the pre-enable
  crawler follow-ups from `docs/NETWORK_PEER_CRAWLER.md` (concurrency, live `crawl --once`, etc.).
