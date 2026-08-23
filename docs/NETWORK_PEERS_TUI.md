# Network Peers TUI — Design Spec

_Date: 2026-07-03 · Status: Implemented (design record) · Depends on: `docs/NETWORK_PEER_CRAWLER.md` + `docs/NETWORK_PEERS_UI.md`_

> Plan boundaries and “what next” notes are retained as historical rationale. Current TUI
> ownership and entry points live in `docs/ARCHITECTURE_MAP.md`.

Add a **"Peers" tab** to the monitoring TUI (`crates/tui`) that surfaces the p2p crawler's operational
health and the discovered-network snapshot, by fetching the Plan-2 `/network/*` API over the TUI's
existing HTTP client.

## Goal

- Give `ckbadger tui` an at-a-glance view of the crawler: is it running, when did it last crawl, how
  many nodes are reachable, and what the network looks like (versions, countries) and how it's
  trending — without leaving the terminal.
- Deliverables: a 4th `MainTab::Peers` with a **status block + distributions charts + a trend chart**,
  plus the `db.rs` data layer that feeds it.

## Principle Alignment

- **Local First / thin monitor** — reuse the TUI's existing reqwest client (as it already does for
  `/statistics/network`); do NOT open a third RocksDB secondary. The API owns the computation
  (single calculation path); the TUI only renders.
- **Fail-fast vs "no data"** — an API/network error degrades to an on-screen error line (like the
  existing `ApiServiceInfo` health), never a panic; the crawler being off / not-yet-run is a NORMAL
  state driven by `summary.enabled` / `summary.hasData`, rendered as a message (not an error).
- **Honesty** — labels match the web dashboard: completed peer counters and address attempts have
  distinct names, and an active logical round is shown as separate progress; no "total network"
  claim.
- **Reuse, no new primitives** — reuse `chart.rs` (`render_bar_chart` / `render_stacked_bar_chart`),
  the existing density/layout helpers, and the refresh-tick loop.

## Non-Goals

- No new API endpoints (Plan 2's `/network/{summary,distributions,history}` suffice).
- No third store secondary in the TUI (HTTP-only for peers data).
- No node table / per-node detail in the TUI (that's the web dashboard); no interactive filtering.
- No ASN/protocol charts in v1 (space-limited) — a compact summary line at most; deferred to a later pass.

## Background

`crates/tui` today: three tabs `MainTab::{Overview, Sync, System}` (cycled via a key). Data comes
from a **hybrid** `db.rs`: a **domain-store secondary** (`open_domain_secondary_with_runtime`) for
direct sync/memory reads, plus a **reqwest HTTP client** to the API (`GET /statistics/network`) for
chain stats + API health/latency (`ApiServiceInfo`). Rendering is in `ui.rs` (density-aware layout,
`LayoutDensity`) + `chart.rs` (`render_bar_chart`, `render_stacked_bar_chart`). The **network store
is a separate, opt-in RocksDB instance the TUI does not open** — but Plan 2's API already exposes
everything a peers view needs.

---

## Architecture

- **Data layer** (`crates/tui/src/db.rs`): add response types + a `get_peers_data()` method that
  fetches the Plan-2 endpoints over the existing `self.http` client and assembles a `PeersData`.
- **Tab** (`crates/tui/src/ui.rs`): add `MainTab::Peers`, thread it through `next()`/`prev()` cycling
  (Overview → Sync → System → **Peers** → Overview), the tab-title bar, the footer key hint, and the
  input/scroll `match` arms (Peers scroll behavior mirrors `System`).
- **App state + refresh**: add `peers_data: Option<PeersData>`; the existing refresh tick (which
  already fetches sync/memory/`/statistics/network`) also calls `get_peers_data()` and stores it.

### Data types (`db.rs`, `serde` `Deserialize`, camelCase → snake via `rename_all`)

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkLastRound {
    round_id: u64, started_at: u64, finished_at: u64,
    candidate_peers: u64, attempted_peers: u64,
    reachable_peers: u64, unreachable_peers: u64,
    address_attempts: u64, failed_address_attempts: u64,
    foreign_peers: u64, malformed_addresses: u64,
    new_nodes: u64, total_known: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkActiveRound {
    round_id: u64, started_at: u64, last_checkpoint_at: u64,
    candidate_peers: u64, completed_peers: u64,
    address_attempts: u64, blocked_reason: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkSummary {
    enabled: bool,
    has_data: bool,
    last_round: Option<NetworkLastRound>,
    active_round: Option<NetworkActiveRound>,
}

#[derive(Debug, Clone, Deserialize)]
struct LabelCount { label: String, count: u64 }
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkDistributions {
    total_known: u64, reachable: u64, unreachable: u64,
    versions: Vec<LabelCount>, countries: Vec<LabelCount>,
    asns: Vec<LabelCount>, protocols: Vec<LabelCount>,
}
#[derive(Debug, Clone, Deserialize)]
struct NetworkHistoryPoint { ts: u64, scalar: u64 }   // `buckets` omitted (not needed for the scalar trend)
#[derive(Debug, Clone, Deserialize)]
struct NetworkHistory { points: Vec<NetworkHistoryPoint> }
```

### `PeersData` + fetch

```rust
pub struct PeersData {
    pub summary: Option<NetworkSummary>,
    pub distributions: Option<NetworkDistributions>,
    pub total_history: Vec<NetworkHistoryPoint>,     // metric=totalNodes,   granularity=hour
    pub reachable_history: Vec<NetworkHistoryPoint>, // metric=reachableNodes, granularity=hour
    pub error: Option<String>,
}
```

`get_peers_data()`:

1. `GET /network/summary`. On transport/http error ⇒ `PeersData { error: Some(..), ..default }` (return early).
2. If `summary.has_data` is false (crawler off or no round yet) ⇒ return with just `summary` set
   (skip the heavier fetches — nothing to chart).
3. Else fetch `GET /network/distributions`, `GET /network/history?metric=totalNodes&granularity=hour`,
   and `?metric=reachableNodes&granularity=hour`; fill the fields. A failure on any of these sets
   `error` but keeps whatever succeeded (best-effort, never panics).

Cadence: called from the existing refresh tick. Crawler data changes on the ~15-min crawl cadence, so
this is comfortably within the tick budget; the summary-first short-circuit means the off state costs
one small request.

---

## Peers tab layout (`ui.rs`, `render_peers_tab`)

Top-to-bottom, density-aware (mirror the existing tabs' `LayoutDensity` handling — shrink/hide the
trend on short terminals):

1. **Status block** (~3–5 lines) — a labeled stat grid from `summary.last_round`:
   - Crawler: on/off · completed `#round_id` · last-round age from `finished_at`.
   - An active round adds `CRAWLING #round_id completed_peers/candidate_peers` without altering
     the displayed completed snapshot.
   - Discovered: `total_known` · `reachable_peers` · `unreachable_peers`.
   - Completed round: `address_attempts` · `failed_address_attempts` · `new_nodes` ·
     `foreign_peers`.
2. **Distributions** (two side-by-side bar charts via `render_bar_chart`):
   - Left: version top-N (`distributions.versions`).
   - Right: country top-N (`distributions.countries`).
   - A one-line `reachable` vs `unreachable` summary. (Charts take the first N entries — the API
     already returns them count-desc.)
3. **Trend** (bottom, `render_stacked_bar_chart`): per hour bucket, bar height = totalNodes with a
   `reachable` segment + an `unreachable` segment, synthesized by zipping `total_history` and
   `reachable_history` on `ts`, where `unreachable = total.scalar - reachable.scalar`. `reachable ≤
total` holds by construction (same-round aggregate); a bucket where `reachable > total` is an
   upstream invariant violation. The current helper skips that bucket and surfaces a gap; buckets
   present in only one series are also skipped. This describes the implementation but is a known
   deviation from ckbadger's fail-fast invariant policy: a correctness fix should return an
   explicit error and select the Peers error view, not silently omit the point or clamp it with
   `saturating_sub`. The chart otherwise shows network size + reachability over the recent window.

### Off / waiting / error states (replace the charts with a centered message)

- `error.is_some()` ⇒ an error line at the top (reuse the api-health error style).
- `summary.enabled == false` ⇒ "Crawler disabled — set `[crawler].enabled = true` in this
  network's config.toml".
- `enabled == true && has_data == false` ⇒ "Crawler enabled — waiting for the first round…".

---

## Testing (MANDATORY — per CLAUDE.md)

- **Deserialization** (`db.rs` `#[cfg(test)]`): `NetworkSummary`/`NetworkDistributions`/`NetworkHistory`
  parse from canned API JSON (assert camelCase mapping, `lastRound: null` → `None`, populated case).
- **Tab cycling** (`ui.rs`): `MainTab::next()`/`prev()` include `Peers` in the correct cycle
  (Overview→Sync→System→Peers→Overview and reverse).
- **Pure helpers**: last-round age formatting; completed/active state selection; the current trend
  zip/synthesis behavior (`total`,`reachable` → `(reachable, unreachable)` per hour); the state
  selector (error vs disabled vs waiting vs dashboard) as a small pure function over `PeersData`.
- **Render** (only if the crate already uses ratatui `TestBackend` — the implementer confirms and
  mirrors the existing UI test approach): render `render_peers_tab` to a buffer and assert key text —
  status labels + a version/country bar label when `hasData`; the "Crawler disabled" text when
  `enabled=false`. If there is no `TestBackend` precedent, the parse + cycling + pure-helper tests are
  the required coverage (do NOT introduce a heavy new test harness for one tab).

---

## Result

- **Behavior change** — `ckbadger tui` gains a 4th "Peers" tab: crawler status + distributions +
  trend, fetched from the Plan-2 `/network/*` API. No change to the crawler, API, or other tabs'
  behavior; opt-in-aware (shows a disabled/waiting message until the crawler runs).
- **Re-sync required** — **No** (read-only monitoring via HTTP).
- **What to do next** — keep the TUI as a thin HTTP reader of the API's single network-data
  calculation path and make invalid trend buckets select an explicit error state. A topology graph
  remains outside the current TUI scope.
