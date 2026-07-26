# Network Peer Crawler — Design Spec

_Date: 2026-07-01 · Status: Implemented (design record) · Owner: ckbadger_

> This file preserves the crawler's design rationale. For current configuration, store schema,
> and HTTP routes, use `README.md`, `docs/STORE_SCHEMA.md`, and `docs/API.md`. Foundation-era
> follow-up lists below are historical context, not an authoritative current backlog.

Discover and collect statistics on the **whole CKB L1 node p2p network** — a local-first
crawler that maps the reachable node set, its client/version/geo distribution, historical
trends, and sampled peer gossip. The current UI does not render a topology graph.

---

## Goal

- Answer "who is on the CKB L1 network right now, and how is it changing?" from a machine the
  user controls, without trusting a hosted dashboard.
- Implemented surfaces: **(1)** snapshot + distributions, **(2)** historical trend curves,
  **(3)** filterable node table, plus an API-only point lookup. A topology graph is not currently
  exposed.
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
- **Fail Fast, No Silent Fallback** — startup configuration errors abort loudly; expected network
  failures (refused/timeout/handshake-fail) are **recorded as observations**, not masked. The
  current post-start round-error propagation gap is called out explicitly below.
- **Single Calculation Path** — "current distributions" have exactly one path: scan
  `CF_NET_NODES`. History is the same aggregate materialized at round end. No dual paths.
- **Numerical Precision** — node counts are **discovered/reachable only**, never sampled,
  extrapolated, or multiplied by a coverage factor.

## Non-Goals

- Not the Fiber L2 gossip network (separate protocol stack; possible future spec).
- Not a true real-time connection topology — a discovery crawler cannot observe live edges
  (`get_peers` is local-node-only). Node records retain only a bounded sample of advertised peers;
  the current API and UI do not turn that sample into a topology graph.
- Not a per-node detail page. A point API endpoint exists for hover popovers and API/agent use.
- Not part of the chain data-integrity `verify` suite (observational data, outside the 56 checks).

---

## Architecture

The **`ckbadger-crawler`** service lives in `crates/crawler/` and runs under the
`crates/cli` supervisor, peer to indexer/API/frontend.

```
CKB p2p network ──dial / Identify / Discovery──▶ ckbadger-crawler
                                                   (tentacle Service +
                                                    uncompressed handlers)
                                                        │ RW — SOLE writer
                                                        ▼
                                             network store  (CF_NET_NODES, CF_NET_STATS)
                                                        ▲ secondary — read-only
                                             ckbadger-api ──▶ frontend /network + TUI
```

- The real prober builds a tentacle service directly and reuses `ckb-network` 0.119's
  `SupportProtocols` definitions. Identify and Discovery are registered uncompressed, matching
  CKB's built-in framing. Each probe is **Feeler-style** (connect → interrogate → disconnect);
  there is no Sync, Relay, or Ping protocol and no block download. `last_rtt_ms` is the elapsed
  dial-to-Identify time, not an ICMP/CKB Ping measurement.
- The expected Identify name is derived from `[ckb].network` plus its genesis hash. Only mainnet
  and testnet are supported. A foreign-network Identify never marks a node reachable, but the
  current prober reports it through the generic unreachable result, so `foreign_dropped` remains
  zero.
- **Bootnodes**: optional `[crawler].bootnodes` overrides the built-in list for the selected
  mainnet/testnet network. An unsupported network or an empty resolved seed list fails startup;
  malformed, refused, or timed-out seed dials count as failed probe attempts.
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
| `CF_NET_NODES` | `peer_id` (tentacle PeerId bytes)                        | `NodeRecord`                                   | snapshot / distributions / list / detail    |
| `CF_NET_STATS` | `0x00` singleton **or** `metric(1)+gran(1)+ts_bucket(8)` | latest-round status **or** aggregate histogram | latest status (monitoring) + history trends |

```rust
struct NodeRecord {
    own_addrs: Vec<String>,          // node's own listen multiaddrs (union across rounds)
    client_version: String,          // from Identify
    flags: u64,                      // CKB Identify capability flags
    protocols: Vec<String>,          // protocol names opened during the successful probe
    first_seen: u64,                 // unix secs, set once
    last_seen: u64,                  // unix secs, latest successful same-network Identify
    last_reachable_at: u64,          // same successful-probe timestamp
    reachable: bool,                 // true only if handshake completed THIS round
    geo: Option<Geo>,                // { country, city, lat, lon }; None = Unknown (honest)
    asn: Option<Asn>,                // { number, org };            None = Unknown (honest)
    last_rtt_ms: Option<u32>,        // elapsed dial-to-Identify time
    known_peers: Vec<PeerId>,        // peer IDs resolved through the known address map from this
                                     // node's latest successful Nodes response; not proof that
                                     // each peer was reachable in the same round
}
```

`CF_NET_STATS`:

- `0x00` → `LatestStatus { round_id, started, finished, dialed, reachable, unreachable,
foreign_dropped, new_nodes, total_known, frontier_drained }`. Here `reachable` is the count of
  post-round node records marked reachable, while `unreachable` is the count of address probes that
  returned no same-network Identify. Those fields have different units; node-level unreachable is
  derived by `/network/distributions` as `total_known - reachable`.
- `metric(1)+gran(1)+ts(8)` → aggregate for a time bucket. `metric` ∈ {TotalNodes=1,
  ReachableNodes=2, VersionShare=3, CountryShare=4}; `gran` ∈ {Hour=1, Day=2}; `ts` = bucket index.
  Scalar metrics store a count; share metrics store a serialized top-N map.

**Dropped from an earlier 5-CF draft** (deliberately, for simplicity):

- `CF_ADDR_INDEX` → the `addr → peer_id` index is an **in-memory structure rebuilt each round** by
  scanning `CF_NET_NODES` (node/address counts are small). No persistent secondary index.
- `CF_EDGES` → folded into `NodeRecord.known_peers`; loses per-edge timestamps (YAGNI) and
  records only address-book references resolvable through the in-memory known-address map. The
  current UI does not expose these references as graph edges.
- `CF_ROUND_META` → folded into the `CF_NET_STATS` `0x00` singleton; historical round counts are
  already covered by the time-series buckets.

**Single calculation path:** current distributions are computed one way — scan `CF_NET_NODES`.
Each round end materializes that same aggregate into `CF_NET_STATS` as the historical point.

---

## Crawl Algorithm — discrete rounds

Each completed round is one bounded BFS attempt. It overwrites the current hour/day history
buckets with the latest post-round aggregate; it does not append a separate key per round. A round
whose wall-clock or frontier budget is exhausted is persisted with `frontier_drained=false`.

1. **Seed frontier** = bootnodes ∪ `own_addrs` of all `CF_NET_NODES` records. Build the in-memory
   `addr → peer_id` index from `CF_NET_NODES`.
2. **Dial + Feeler probe** (currently sequential): dial each address → secio handshake yields
   `peer_id` → **Identify** (version / flags / network-id / own listen addrs) → **Discovery** send
   `GetNodes`, receive `Nodes` (address-book sample; allowed because we are the outbound side, per
   RFC 0012) → **disconnect**.
3. **BFS expansion**: new addresses from `Nodes` responses enter the frontier and are dialed,
   until the frontier drains or the wall-clock/frontier budget is hit.
4. **Resolve peer references**: for each identified node A, map its `Nodes` addresses → `peer_id`
   through the address index, which is seeded from existing records and updated by successful
   probes. Drop unresolved addresses and de-duplicate IDs. A resolved peer may still have failed
   its probe this round, so `known_peers` is address-book gossip, not a reachability edge set.
5. **Persist successes** (idempotent per-node upsert): update successful-probe timestamps,
   reachability, version/flags/protocols/elapsed time; union `own_addrs`; replace `known_peers`
   with the new sample; preserve `first_seen`; GeoIP-enrich by node IP (miss ⇒ Unknown).
6. **Prune + downgrade**: delete node records whose last successful observation is older than 30
   days; retain newer nodes not reached this round but set `reachable=false`. Prune hourly history
   after its configured retention; keep daily buckets long-term.
7. **Aggregate + write history**: scan the post-prune/post-downgrade nodes, then write the latest
   status plus hour/day history for total known, reachable, version, and country.

### Concurrency & budget

- Dials are currently sequential. `max_dial_concurrency` defaults to 128 but is not yet applied.
- `dial_timeout_secs` (default 15s) bounds the wait for a same-network Identify. A successful
  Identify then gets a separate, bounded Discovery grace period.
- The 600s round budget is checked between probes; `max_frontier` (default 100,000) bounds queued
  addresses.
- **A partial round is not an error**: persist normally, but set `frontier_drained = false` so no
  consumer mistakes it for full coverage.

---

## Error Handling — config/invariant ⇒ fail-fast; network noise ⇒ record

Tolerating dial failures does **not** violate "no silent fallback": the failure **is** the datum
(reachability). We record it; we do not paper over it.

| Condition                                                    | Current handling                                                                                                                                                                                                                                                                   |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Unsupported network, store-open failure, or invalid MMDB     | **fail-fast at startup**                                                                                                                                                                                                                                                           |
| Malformed address, connect refused/timeout, Identify timeout | increment the failed address-probe (`unreachable`) count; continue                                                                                                                                                                                                                 |
| Foreign Identify name                                        | never mark reachable; currently folded into the generic unreachable count (`foreign_dropped=0`)                                                                                                                                                                                    |
| Geo/ASN lookup miss                                          | `Unknown` (honest empty, not abort)                                                                                                                                                                                                                                                |
| Store/read/write or other round-engine error after startup   | the round returns an error, but the continuous service currently logs it and tries the next round; `crawl --once` also returns after logging. This is a known fail-fast gap, not the intended storage-invariant policy.                                                            |
| Crash during BFS or persistence                              | BFS observations have not yet been written; per-node persistence after BFS is idempotent but not one atomic RocksDB batch, so a crash can leave only some node upserts. Latest status/history are published after those writes, and the next round re-probes/upserts the node set. |

## Honesty Invariants

- `reachable = true` **only** when the Identify handshake completed this round; else "known
  address, unconfirmed".
- Node counts are **discovered/reachable only** — never sampled or extrapolated.
- Distributions carry **no inbound/outbound** dimension (direction is a local-node concept; a
  whole-network crawler always dials outbound).
- Sampled `known_peers` are observational address-book gossip from a node's latest successful
  probe, not live connection edges or proof of same-round reachability. They are retained while
  that node is temporarily unreachable but are not exposed as an authoritative graph.
- GeoIP miss ⇒ `Unknown`, never a guessed default.

---

## API — `crates/api/src/routes/network.rs` (all secondary read-only, `camelCase`)

| Endpoint                                              | Purpose                                                                        |
| ----------------------------------------------------- | ------------------------------------------------------------------------------ |
| `GET /network/summary`                                | latest `LatestStatus` + headline numbers                                       |
| `GET /network/distributions`                          | scan `CF_NET_NODES`: version / country / ASN / protocol / reachable histograms |
| `GET /network/nodes`                                  | cursor-paginated node list (filter reachable/country/version)                  |
| `GET /network/nodes/{peerId}`                         | single `NodeRecord` for API/agent use; no dedicated page                       |
| `GET /network/history?metric=&granularity=&from=&to=` | range scan `CF_NET_STATS` buckets for trend charts                             |

WebSocket push of round-complete status is a possible future addition, not v1.

## Frontend — `frontend/app/network/` (`page.tsx` + `client-page.tsx`; types/methods in `lib/api.ts`; TanStack Query v5)

1. **Summary cards** — discovered reachable / failed probe addresses / total known /
   last-round age; a "partial round" badge when `!frontierDrained`. Current web/TUI labels still
   abbreviate the failed-probe field as "Unreachable"; `/network/distributions` is the source for
   node-level reachable/unreachable counts.
2. **Distributions** — version, country, ASN top-N, protocol (reuse existing chart components).
3. **Trends** — node count / version share / country share over time. **Exclude the incomplete
   current day** on daily charts (codebase gotcha).
4. **Nodes table** — cursor pagination plus reachable/country/version filters. A topology graph
   was part of the original design but is not implemented.

MaxMind attribution shown in the page footer.

## TUI / CLI

- **TUI** — the Peers tab fetches summary, distributions, and hourly history through the API; it
  does not open the network store directly.
- **CLI** — `ckbadger crawl [--once]` (`--once` runs a single round and exits, for manual
  verification / cron). `ckbadger status` lists the crawler process when supervised and reports
  per-network chain sync, but does not print crawler-store aggregates. `ckbadger purge` deletes
  chain-derived domain/append-only data but deliberately preserves the observational network
  store. There is currently no separate network-store purge flag.

## Configuration (per-network `config.toml`)

```toml
[crawler]
enabled = false            # opt-in: outbound whole-network crawling is a distinct posture
round_interval_secs = 900  # 15 min
max_dial_concurrency = 128
dial_timeout_secs = 15
round_budget_secs = 600
max_frontier = 100000
history_hourly_retention_days = 30    # hourly buckets 30d; daily buckets long-term
# geoip_city_path / geoip_asn_path : unset ⇒ geo/asn disabled; set-but-unreadable ⇒ fail-fast
# bootnodes : optional override; otherwise built-in mainnet/testnet lists

[store]
network_data_path = "data/network"
```

In orchestrator mode, configure this section separately in
`<orchestrator-root>/<network>/config.toml`; the top-level `ckbadger.toml` does not contain
crawler or store settings.

---

## Testing (MANDATORY — per CLAUDE.md testing table)

**Core testability design:** abstract the p2p layer behind a `Prober` / `DiscoverySource` trait so
the round engine (BFS, edge resolution, upsert, aggregation, partial-round marking, TTL prune) is
tested against a **mock prober** with scripted Identify/Nodes/reachability — no real network.

- **Crawler engine unit tests (mock prober):** BFS covers all reachable mock nodes; unreachable
  addresses are counted; known-peer resolution drops unresolved addresses and de-duplicates peer
  IDs; wall-clock/frontier caps set `frontier_drained=false`; TTL pruning and reachability
  downgrade are covered.
- **Real-prober protocol tests:** Identify/Discovery molecule parsing plus a local uncompressed
  tentacle peer prove that the prober captures the advertised address sample without a real
  mainnet/testnet dependency.
- **Store unit tests:** `NodeRecord` and history-bucket key encode/decode roundtrip; upsert + scan
  distributions; history range scan; prune. The production API path opens the network store
  **secondary/read-only**, but a dedicated production-path integration test remains open.
- **API tests:** `crates/api/tests/api_network.rs` (per-resource convention) — each endpoint:
  happy path + empty store + not-found, against a seeded network store.
- **Frontend tests:** `frontend/__tests__/` for summary, distributions, trends, node table, and
  opt-in empty states, plus MSW handlers for the endpoints; daily charts exclude the current day.
- **Manual:** `ckbadger crawl --once` runs one real round for human verification (not in CI).

## Documentation Sync (MANDATORY)

Same commit as the storage change: update `CLAUDE.md` + `README.md` store-boundary sections with
the third-store class; add `docs/STORE_SCHEMA.md` entries for `CF_NET_NODES` / `CF_NET_STATS`; note
the network store's exemption from the `verify` suite in `docs/TESTING.md`.

## Open Defaults (recommended; adjustable)

1. **Cadence** — 15 min/round (~96 points/day; churn is slow; not noisy).
2. **GeoIP** — MaxMind GeoLite2 City + ASN local MMDB; path via config; **not bundled** (license:
   free account + attribution, redistribution restricted).
3. **Retention** — hourly buckets 30d, daily long-term; node records deleted after 30d without a
   successful observation; `known_peers` replaced on the node's next successful probe.
4. **enabled** — default `false` (opt-in).

---

## Foundation-Era Follow-ups (Historical)

The following list captured the state of the original foundation branch. Verify each item against
current code before treating it as open work:

- Dialing is sequential; `max_dial_concurrency` is not yet applied (bounded
  concurrency is a planned optimization now that rounds are time-bounded).
- Node persistence starts after BFS but uses individual upserts rather than one atomic batch; a
  crash during that phase can leave partial node updates before the next round re-crawls.
- **A live `ckbadger crawl --once` against a real testnet/mainnet node has NOT
  been run — the tentacle-direct prober path is unverified end-to-end and MUST be
  confirmed before enabling.**
- `LatestStatus.foreign_dropped` is currently always 0 (foreign-network peers are
  counted as unreachable); populate it if the transparency metric is wanted.
- `LatestStatus.unreachable` counts failed address probes, not unreachable stored nodes; rename
  the web/TUI summary label or materialize a separate node-level field so units are unmistakable.
- Round-engine errors are logged and retried by the continuous loop, and `crawl --once` returns
  success after logging one; storage/invariant failures should instead propagate according to the
  project's fail-fast rule.
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
  `/network/*` API; new `/network` frontend page (summary, distributions, trends, node table);
  TUI + CLI additions. No change to chain indexing.
- **Re-sync required** — **No.** The crawler is independent of chain sync; the network store builds
  itself by crawling. Purging the network store only costs a re-crawl.
- **What to do next** — validate any proposed crawler change against the sole-writer/network-store
  boundary and update the current schema/API/config documents in the same change.
