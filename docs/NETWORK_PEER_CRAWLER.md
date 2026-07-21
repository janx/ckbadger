# Network Peer Crawler — Design Spec

_Date: 2026-07-01 · Status: Approved (design) · Owner: ckbadger_

Discover and collect statistics on the **whole CKB L1 node p2p network** — a local-first
crawler that maps the reachable node set, its client/version/geo distribution, historical
trends, and an honest reachability/gossip graph.

---

## Goal

- Answer "who is on the CKB L1 network right now, and how is it changing?" from a machine the
  user controls, without trusting a hosted dashboard.
- Deliverables (approved scope): **(1)** snapshot + distributions, **(2)** historical trend
  curves, **(3)** topology graph. **No** per-node detail page.
- Prior art validating feasibility: Nervos runs an official **CKB Node Probe**
  (<https://nodes.ckb.dev/>). ckbadger's differentiator is **local-first**: you run the crawler
  yourself, the data is yours.

## Principle Alignment

- **Local First** — a new self-contained service/binary; data lives on the user's disk; no
  central dependency. Crawling is opt-in (respects the user's network posture).
- **CKB Native, chain data is the only source of truth** — this feature is the **one sanctioned
  exception**. P2P peer topology is network-layer, ephemeral, non-deterministic, and **cannot be
  rebuilt from genesis**. It is therefore isolated in a **third logical store** governed by
  different rules, so the two chain stores' "rebuild from genesis" invariant stays intact.
- **Fail Fast, No Silent Fallback** — configuration/invariant errors abort loudly; expected
  network failures (refused/timeout/handshake-fail) are **recorded as observations**, not masked.
- **Single Calculation Path** — "current distributions" have exactly one path: scan
  `CF_NET_NODES`. History is the same aggregate materialized at round end. No dual paths.
- **Numerical Precision** — node counts are **discovered/reachable only**, never sampled,
  extrapolated, or multiplied by a coverage factor.

## Non-Goals

- Not the Fiber L2 gossip network (separate protocol stack; possible future spec).
- Not a true real-time connection topology — a discovery crawler cannot observe live edges
  (`get_peers` is local-node-only). We produce a **reachability / address-book (gossip) graph**.
- Not a per-node detail page. A point API endpoint exists for hover popovers and API/agent use.
- Not part of the chain data-integrity `verify` suite (observational data, outside the 55 checks).

---

## Architecture

New service **`ckbadger-crawler`** (new crate `crates/crawler/`, library run under the
`crates/cli` supervisor, peer to indexer/api/frontend).

```
CKB p2p network ──dial / Identify / Discovery / Ping──▶ ckbadger-crawler
                                                         (ckb-network NetworkController +
                                                          custom Prober / ProtocolHandlers)
                                                              │ RW — SOLE writer
                                                              ▼
                                                   network store  (CF_NET_NODES, CF_NET_STATS)
                                                              ▲ secondary — read-only
                                                   ckbadger-api ──▶ frontend /network + TUI
```

- Depends on **`ckb-network`** pinned to match the project (`0.119.x`, consistent with
  `ckb-types`/`ckb-hash 0.119`). Constructs a `NetworkController` registering only
  `Ping + Discovery + Identify + DisconnectMessage`, with **Feeler-style** behavior
  (connect → interrogate → disconnect). **No Sync/Relay** — no block download.
- Network identifier is derived from `[ckb].network` (main/test); `ckb-network` enforces it in
  Identify, so **foreign-network nodes are auto-rejected** (counted for transparency).
- **Bootnodes**: read from the supervised CKB node's chain spec / `ckb.toml` in `[ckb].workdir`;
  fall back to `ckb-network` built-in defaults; optional `[crawler].bootnodes` override.
  **No resolvable bootnodes ⇒ fail-fast (refuse to start).**
- **Third store** `network` (`[store].network_data_path`, default `data/network`): crawler opens
  **RW as the sole writer**; API opens **secondary (read-only)**, mirroring the existing dual-store
  pattern. Storage code lives in `crates/ckbadger-store/` as a distinct RocksDB instance
  (module `network_store.rs` + keys + ops), consistent with the storage crate owning all schema.

### Store-boundary doctrine update (MANDATORY, principle-sync)

The third store is a controlled exception and MUST be documented in the same commit in both
`CLAUDE.md` and `README.md` store-boundary sections:

- **Domain store** (59 CFs) and **append-only store** (1 CF `CF_CELLS`) — unchanged; still bound
  by "rebuild from genesis".
- **Network store** (new, 2 CFs) — **third logical class: network-layer observations. Non-chain,
  non-deterministic, not rebuildable from genesis, TTL-retained.** The only store explicitly
  exempt from the rebuild invariant. Written exclusively by `ckbadger-crawler`; read-only for API.

---

## Data Model — `network` store (2 CFs, all `CF_NET_` prefixed)

| CF             | key                                                      | value                                          | serves                                      |
| -------------- | -------------------------------------------------------- | ---------------------------------------------- | ------------------------------------------- |
| `CF_NET_NODES` | `peer_id` (tentacle PeerId bytes)                        | `NodeRecord`                                   | snapshot / distributions / graph / hover    |
| `CF_NET_STATS` | `0x00` singleton **or** `metric(1)+gran(1)+ts_bucket(8)` | latest-round status **or** aggregate histogram | latest status (monitoring) + history trends |

```rust
struct NodeRecord {
    own_addrs: Vec<String>,          // node's own listen multiaddrs (union across rounds)
    client_version: String,          // from Identify
    flags: u64,                      // CKB Identify capability flags
    protocols: Vec<String>,          // negotiated / advertised protocol names
    first_seen: u64,                 // unix secs, set once
    last_seen: u64,                  // unix secs, any contact this round
    last_reachable_at: u64,          // unix secs, only when Identify handshake completed
    reachable: bool,                 // true only if handshake completed THIS round
    geo: Option<Geo>,                // { country, city, lat, lon }; None = Unknown (honest)
    asn: Option<Asn>,                // { number, org };            None = Unknown (honest)
    last_rtt_ms: Option<u32>,        // from Ping
    known_peers: Vec<PeerId>,        // reachable peers whose addr appeared in this node's Nodes
                                     // response THIS round (fresh sample; replaced each round)
}
```

`CF_NET_STATS`:

- `0x00` → `LatestStatus { round_id, started, finished, dialed, reachable, unreachable,
foreign_dropped, new_nodes, total_known, frontier_drained }`.
- `metric(1)+gran(1)+ts(8)` → aggregate for a time bucket. `metric` ∈ {TotalNodes=1,
  ReachableNodes=2, VersionShare=3, CountryShare=4}; `gran` ∈ {Hour=1, Day=2}; `ts` = bucket index.
  Scalar metrics store a count; share metrics store a serialized top-N map.

**Dropped from an earlier 5-CF draft** (deliberately, for simplicity):

- `CF_ADDR_INDEX` → the `addr → peer_id` index is an **in-memory structure rebuilt each round** by
  scanning `CF_NET_NODES` (node/address counts are small). No persistent secondary index.
- `CF_EDGES` → folded into `NodeRecord.known_peers`; loses per-edge timestamps (YAGNI) and
  naturally enforces honest reachable×reachable edges.
- `CF_ROUND_META` → folded into the `CF_NET_STATS` `0x00` singleton; historical round counts are
  already covered by the time-series buckets.

**Single calculation path:** current distributions are computed one way — scan `CF_NET_NODES`.
Each round end materializes that same aggregate into `CF_NET_STATS` as the historical point.

---

## Crawl Algorithm — discrete rounds

Each round = one whole-network BFS = one historical data point. (Discrete rounds, not a rolling
view: clean snapshots, simple, one time-series point per round.)

1. **Seed frontier** = bootnodes ∪ `own_addrs` of all `CF_NET_NODES` records (optionally ∪ local
   node `get_peers` as free seeds). Build in-memory `addr → peer_id` index from `CF_NET_NODES`.
2. **Dial + Feeler probe** (bounded concurrency): dial each address → secio handshake yields
   `peer_id` → **Identify** (version / flags / network-id / own listen addrs) → **Ping** (RTT) →
   **Discovery** send `GetNodes`, receive `Nodes` (address-book sample; allowed because we are the
   outbound side, per RFC 0012) → **disconnect** (do not hold thousands of connections).
3. **BFS expansion**: new addresses from `Nodes` responses enter the frontier and are dialed,
   until the frontier drains **or** the round budget (wall-clock / max addresses) is hit.
4. **Resolve edges**: for each identified node A, map its `Nodes` addresses → `peer_id` via the
   index; keep only those resolving to a **reachable** node B → write into `A.known_peers`.
5. **Persist** (idempotent per-node upsert into `CF_NET_NODES`): update `last_seen`; if handshake
   done, update `last_reachable_at`/`reachable`, version/flags/protocols/rtt; union `own_addrs`;
   replace `known_peers` with this round's sample; set `first_seen` only if new; increment
   new-node counter. GeoIP-enrich by node IP (miss ⇒ Unknown).
6. **Aggregate + write history**: compute round aggregate (total known, reachable, version
   histogram, country histogram) → write `CF_NET_STATS` `0x00` singleton + append time-series
   buckets.
7. **Prune / TTL**: delete `CF_NET_NODES` records absent > 30 days (truly gone); short-term
   unreachable only sets `reachable=false` + keeps `last_reachable_at`. History: hourly buckets
   pruned after 30 days; daily buckets kept long-term.

### Concurrency & budget

- Bounded dial concurrency (config, default **128** — a polite crawler); per-dial timeout
  (connect + Identify + Discovery, default **15s** total) then force-disconnect.
- Round budget: max wall-clock (default **600s**) OR max addresses, first to hit stops the round.
- **A partial round is not an error**: persist normally, but set `frontier_drained = false` so no
  consumer mistakes it for full coverage.

---

## Error Handling — config/invariant ⇒ fail-fast; network noise ⇒ record

Tolerating dial failures does **not** violate "no silent fallback": the failure **is** the datum
(reachability). We record it; we do not paper over it.

| Condition                                                                  | Handling                                                                                                                                                                                                                                                      |
| -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No resolvable bootnodes                                                    | **fail-fast**, refuse to start                                                                                                                                                                                                                                |
| MMDB path set but unreadable/corrupt                                       | **fail-fast** (misconfig)                                                                                                                                                                                                                                     |
| network store cannot open RW / write fails                                 | **fail-fast** with `round_id`+key context                                                                                                                                                                                                                     |
| `[ckb].network` unknown / genesis mismatch                                 | **fail-fast**                                                                                                                                                                                                                                                 |
| connect refused/timeout, handshake fail, Identify timeout, peer disconnect | **record** as unreachable this round, continue                                                                                                                                                                                                                |
| foreign network-id (auto-rejected by ckb-network)                          | **drop** node, count `foreign_dropped`                                                                                                                                                                                                                        |
| geo/asn lookup miss                                                        | `Unknown` (honest empty, not abort)                                                                                                                                                                                                                           |
| crawler crash mid-round                                                    | a crawl round is the persistence unit — this round's reachable observations are written after its (wall-clock-bounded) BFS completes; a crash mid-round discards that round's in-progress observations, which are re-collected idempotently on the next round |

## Honesty Invariants

- `reachable = true` **only** when the Identify handshake completed this round; else "known
  address, unconfirmed".
- Node counts are **discovered/reachable only** — never sampled or extrapolated.
- Distributions carry **no inbound/outbound** dimension (direction is a local-node concept; a
  whole-network crawler always dials outbound).
- Graph edges are **reachable × reachable, resolved** only; the UI states plainly it is a
  reachability/gossip graph, not real-time connections.
- GeoIP miss ⇒ `Unknown`, never a guessed default.

---

## API — `crates/api/src/routes/network.rs` (all secondary read-only, `camelCase`)

| Endpoint                                              | Purpose                                                                             |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `GET /network/summary`                                | latest `LatestStatus` + headline numbers                                            |
| `GET /network/distributions`                          | scan `CF_NET_NODES`: version / country / ASN / protocol / reachable histograms      |
| `GET /network/nodes`                                  | cursor-paginated node list (filter reachable/country/version); table + graph source |
| `GET /network/nodes/{peerId}`                         | single `NodeRecord` (hover popover, API/agent use; no dedicated page)               |
| `GET /network/graph?cap=&minDegree=`                  | `{nodes, edges}` from `known_peers`, with a cap to bound render size                |
| `GET /network/history?metric=&granularity=&from=&to=` | range scan `CF_NET_STATS` buckets for trend charts                                  |

WebSocket push of round-complete status is a possible future addition, not v1.

## Frontend — `frontend/app/network/` (`page.tsx` + `client-page.tsx`; types/methods in `lib/api.ts`; TanStack Query v5)

1. **Summary cards** — discovered reachable / unreachable / last-round age; a "partial round" badge
   when `!frontierDrained`. Honest wording: "discovered reachable nodes", not "total network nodes".
2. **Distributions** — version, country, ASN top-N, protocol (reuse existing chart components).
3. **Trends** — node count / version share / country share over time. **Exclude the incomplete
   current day** on daily charts (codebase gotcha).
4. **Topology graph** — `react-force-graph-2d` loaded via `frontend/lib/dynamic-client.tsx`
   (codebase gotcha). Hover → popover. **Prominent caption:** "reachability / address-book
   propagation graph — not a real-time connection topology." Node/degree cap control.

MaxMind attribution shown in the page footer.

## TUI / CLI

- **TUI** — add a crawler line to the supervised-services view (running / last-round age / next-round
  ETA); optional "Network" panel (total known + top-5 versions/countries).
- **CLI** — `ckbadger crawl [--once]` (`--once` runs a single round and exits, for manual
  verification / cron); `ckbadger status` shows crawler + network status; `ckbadger purge --network`
  clears the network store (cheap to re-crawl).

## Configuration (`ckbadger.toml`)

```toml
[crawler]
enabled = false            # opt-in: outbound whole-network crawling is a distinct posture
round_interval_secs = 900  # 15 min
max_dial_concurrency = 128
dial_timeout_secs = 15
round_budget_secs = 600
history_hourly_retention_days = 30    # hourly buckets 30d; daily buckets long-term
# geoip_city_path / geoip_asn_path : unset ⇒ geo/asn disabled; set-but-unreadable ⇒ fail-fast
# bootnodes : optional override; else from node config / ckb-network defaults

[store]
network_data_path = "data/network"
```

---

## Testing (MANDATORY — per CLAUDE.md testing table)

**Core testability design:** abstract the p2p layer behind a `Prober` / `DiscoverySource` trait so
the round engine (BFS, edge resolution, upsert, aggregation, partial-round marking, TTL prune) is
tested against a **mock prober** with scripted Identify/Nodes/reachability — no real network.

- **Crawler engine unit tests (mock prober):** BFS covers all reachable mock nodes; unreachable
  addresses marked correctly; edges only reachable×reachable; foreign-network dropped and counted;
  budget cap ⇒ `frontier_drained=false`; TTL prune deletes only long-absent; re-run is idempotent
  (crash recovery ⇒ identical state).
- **Store unit tests:** `NodeRecord` and history-bucket key encode/decode roundtrip; upsert + scan
  distributions; history range scan; prune. **Boundary test:** API path opens the network store in
  **secondary/read-only** mode and it is isolated from the two chain stores.
- **API tests:** `crates/api/tests/api_network.rs` (per-resource convention) — each endpoint:
  happy path + empty store + not-found, against a seeded network store.
- **Frontend tests:** `frontend/__tests__/` for each page section + MSW handlers for the new
  endpoints; assert the graph honesty caption renders and daily charts exclude the current day.
- **Manual:** `ckbadger crawl --once` runs one real round for human verification (not in CI).

## Documentation Sync (MANDATORY)

Same commit as the storage change: update `CLAUDE.md` + `README.md` store-boundary sections with
the third-store class; add `docs/STORE_SCHEMA.md` entries for `CF_NET_NODES` / `CF_NET_STATS`; note
the network store's exemption from the `verify` suite in `docs/TESTING.md`.

## Open Defaults (recommended; adjustable)

1. **Cadence** — 15 min/round (~96 points/day; churn is slow; not noisy).
2. **GeoIP** — MaxMind GeoLite2 City + ASN local MMDB; path via config; **not bundled** (license:
   free account + attribution, redistribution restricted).
3. **Retention** — hourly buckets 30d, daily long-term; node records deleted after 30d absence;
   `known_peers` replaced each round.
4. **enabled** — default `false` (opt-in).

---

## Known Limitations & Pre-Enable Follow-ups (Foundation)

The Foundation branch ships the round engine and its deterministic tests, but the
following MUST be addressed before the crawler is enabled/run on a real network:

- Dialing is sequential; `max_dial_concurrency` is not yet applied (bounded
  concurrency is a planned optimization now that rounds are time-bounded).
- Persistence is per-round (batched after the BFS), not incremental; a mid-round
  crash re-crawls next round.
- **A live `ckbadger crawl --once` against a real testnet/mainnet node has NOT
  been run — the `ckb-network` prober path is unverified end-to-end and MUST be
  confirmed before enabling.**
- `LatestStatus.foreign_dropped` is currently always 0 (foreign-network peers are
  counted as unreachable); populate it if the transparency metric is wanted.
- `MaxmindGeoIp` can produce `Geo { country: "" }`; enforce empty-country ⇒ drop
  to `None` per the `NodeRecord` invariant.
- The p2p identity/peer-store lives under the OS temp dir; move it under the work
  dir (shared-host secret-key hygiene).
- Commit `Cargo.lock` for the deployable binary so the `tokio-yamux` /
  `tentacle-secio` version pins can be removed.
- Bootnodes are not read from the supervised node's `[ckb].workdir` `ckb.toml`
  (only config override + built-in defaults).
- `default_config_toml()` has no `[crawler]` sample (discoverability);
  `parse_only_flag`'s None arm duplicates `enabled_services`' base list.
- `open_network_secondary` (the API read path) ships untested (covered in Plan 2).

---

## Result

- **Behavior change** — new opt-in `ckbadger-crawler` service; new `network` store (2 CFs); new
  `/network/*` API; new `/network` frontend page (distributions, trends, reachability graph); TUI +
  CLI additions. No change to chain indexing/API.
- **Re-sync required** — **No.** The crawler is independent of chain sync; the network store builds
  itself by crawling. Purging the network store only costs a re-crawl.
- **What to do next** — proceed to the implementation plan (writing-plans): sequence as
  `ckbadger-store` (network store schema/ops) → `crates/crawler` (Prober trait + mock + round
  engine, then ckb-network prober) → `crates/api` routes → frontend page → TUI/CLI → docs sync.

```

```
