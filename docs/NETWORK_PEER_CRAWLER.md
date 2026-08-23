# Network Peer Crawler

_Date: 2026-08-23 · Status: Implemented · Owner: ckbadger_

Discover the reachable CKB L1 p2p network from a local machine, retain exact observations, and
publish only completed crawl snapshots. Current configuration, storage, and HTTP contracts also
live in `README.md`, `docs/STORE_SCHEMA.md`, and `docs/API.md`.

## Goal

- Answer “which CKB L1 peers can this crawler reach, and how is that set changing?” without
  trusting a hosted dashboard.
- Preserve work across execution-budget boundaries and process restarts so a slow or large
  frontier eventually completes instead of repeatedly starting from the first addresses.
- Expose completed snapshots, distributions, history, a filterable node table, and honest
  in-progress telemetry. Counts are observed values only; they are never extrapolated.

## Principle Alignment

- **Local First** — crawling is an opt-in service and its data lives in the configured local
  network store.
- **CKB Native** — Identify and Discovery use CKB's p2p protocols. Network observations are the
  one documented non-chain store class and cannot be reconstructed from chain replay.
- **Fail Fast** — invalid resource limits, malformed bootnodes, store failures, internal prober
  errors, counter overflow, and frontier overflow propagate with context. Expected remote dial,
  timeout, and handshake failures remain typed observations.
- **Single Calculation Path** — current distributions scan the published `CF_NET_NODES`
  snapshot; completed-round history is materialized from that same snapshot.
- **Exact Counters** — peer-level and address-attempt-level counters have separate fields and are
  updated from durable scheduler transitions.

## Non-Goals

- Fiber L2 discovery.
- A claim to know every node on the network. Results are the peer set reachable from configured
  seeds and recursively advertised addresses during a completed logical round.
- A live connection graph. `known_peers` is bounded address-book gossip, not proof of a live edge.
- Chain-integrity verification. The observational network store is outside `ckbadger verify`.

## Architecture

The `ckbadger-crawler` service in `crates/crawler/` runs under the CLI supervisor beside the
indexer, API, and frontend.

```text
CKB p2p network ── Identify / Discovery ──▶ ckbadger-crawler
                                                  │ sole writer
                                                  ▼
                                      network RocksDB
                               ┌────────────┬────────────┐
                               │ published  │ operational│
                               │ nodes/stats│ crawl state│
                               └────────────┴────────────┘
                                                  ▲ secondary, read-only
                                             ckbadger-api
                                                  │
                                           frontend + TUI
```

The real prober builds a tentacle service using `ckb-network` protocol definitions. A probe is
Feeler-style: connect, validate Identify, request Discovery nodes, then disconnect. It does not
sync blocks. `last_rtt_ms` is elapsed dial-to-Identify time.

The expected Identify network name is derived from `[ckb].network` and its genesis hash. Mainnet
and testnet are supported. A mismatched Identify is a `ForeignNetwork` address observation; the
scheduler still tries that PeerId's remaining aliases. If none succeeds and at least one alias was
authenticated as foreign, the candidate is counted once as a foreign peer rather than unreachable.

Bootnodes come from `[crawler].bootnodes` when set, otherwise the network's built-in list.
Unsupported networks, an empty resolved seed list, a malformed bootnode, or a zero resource bound
fails before crawling.

## Network Store

The crawler is the sole writer of the separate network store at `[store].network_data_path`
(default `data/network`). The API opens it as a read-only secondary. Its three column families are:

| CF             | Key                                   | Value                            | Responsibility                                   |
| -------------- | ------------------------------------- | -------------------------------- | ------------------------------------------------ |
| `CF_NET_NODES` | raw peer id                           | `NodeRecord`                     | Last atomically published peer snapshot          |
| `CF_NET_STATS` | status singleton or history key       | `LatestStatus` / `HistoryPoint`  | Last completed round and hour/day history        |
| `CF_NET_CRAWL` | active singleton or peer-prefixed key | `ActiveCrawl` / `CrawlCandidate` | Durable in-progress frontier and staged outcomes |

`NodeRecord` retains own addresses, client version, capability flags, protocols, first/last seen,
last reachable time, current published reachability, optional Geo/ASN, RTT, and resolved sampled
peer references.

`LatestStatus` has unambiguous units:

```text
round_id, started, finished
candidate_peers, attempted_peers
reachable_peers, unreachable_peers, foreign_peers
address_attempts, failed_address_attempts, malformed_addresses
new_nodes, total_known
```

`CF_NET_CRAWL` stores one peer-keyed candidate with all known aliases. A successful observation is
staged there until the logical round finishes. A slice checkpoint atomically writes the active
metadata and all changed candidates. It never changes the published nodes or latest status.

When the frontier is drained, one RocksDB write batch atomically:

- publishes all node upserts and TTL deletions;
- writes the completed status and history buckets;
- retains/prunes durable candidates;
- removes the active-round singleton.

Consequently readers see either the preceding completed snapshot or the next completed snapshot,
never partially downgraded reachability or a status that does not match its nodes.

This schema changes serialized network-store values and adds `CF_NET_CRAWL`. A pre-change
development network store should be cleared once and rebuilt by crawling. This affects only
observational network data; chain stores do not need a re-sync. Configurations that explicitly set
the former `round_budget_secs` key must rename it to `slice_budget_secs`; the legacy key is rejected
with an actionable startup error rather than silently ignored.

## Crawl Model

A **logical round** is the publication unit. An **execution slice** is only a bounded amount of
admission work inside that round. One logical round can span any number of slices and restarts.

1. If `CF_NET_CRAWL` has an active round, load it and resume. Otherwise allocate the next round id.
2. Merge bootnodes, prior node own-addresses, retained candidate aliases, and newly discovered
   addresses into a durable peer-keyed frontier.
3. Schedule at most one address for a peer at a time. Pending peers are served before alias
   retries, and stable last-scheduled ordering prevents one repeatedly failing peer from starving
   the rest of the frontier.
4. Admit no more than `max_dial_concurrency` probes. The slice deadline stops new admission;
   already admitted probes are drained and checkpointed.
5. A successful same-network Identify stages its node observation and expands the frontier from
   own/Discovery addresses. An address failure, including a foreign-network Identify, retries
   another alias only after pending peers have had a turn. After all aliases finish, a candidate
   with any authenticated foreign observation is classified foreign; otherwise it is unreachable.
6. If work remains, return exact active progress and immediately start the next slice. On process
   restart the same round resumes from the last checkpoint.
7. Only after every candidate is terminal, build and validate the next complete node snapshot,
   downgrade unobserved retained nodes, apply TTL retention, materialize history, and publish it
   atomically.
8. In continuous mode, wait `round_interval_secs` only after a logical round completes. With
   `crawl --once`, success means one complete logical round was published.

`slice_budget_secs` is an admission/checkpoint budget, not a coverage cutoff. `max_frontier` is a
hard safety invariant over durable candidate addresses. Overflow atomically rejects the source
result and candidate additions, preserves the preceding checkpoint, stores an actionable
`blocked_reason`, and returns an error without truncating or publishing the round. Raising the
limit resumes by safely retrying that source.

## Observation and Error Semantics

| Condition                                                             | Handling                                                                             |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Same-network Identify + Discovery                                     | `Reachable`; stage exact peer observation                                            |
| Connect refused or dial/Identify timeout                              | failed address attempt; try an alias if present                                      |
| Handshake rejected                                                    | failed address attempt                                                               |
| Foreign Identify network                                              | record on the candidate; try remaining aliases; count the peer once if none succeeds |
| Malformed discovered address                                          | count `malformed_addresses`; do not invent a candidate                               |
| Geo/ASN miss                                                          | honest `None`; an empty country is discarded                                         |
| Invalid config, bad bootnode, store/invariant/internal prober failure | return error and do not publish                                                      |
| Crash or process restart                                              | retain completed checkpoints and resume the active round                             |
| Frontier limit exceeded                                               | persist blocked reason, return error, publish nothing                                |

Counter invariants are checked before publication. A published reachable node must correspond to
a successful candidate in that round. Timestamps cannot regress, round ids and counters cannot
overflow, and a round with non-terminal candidates cannot be committed.

## API and UI

All `/api/v1/network/*` handlers are secondary/read-only.

| Endpoint                      | Purpose                                                         |
| ----------------------------- | --------------------------------------------------------------- |
| `GET /network/summary`        | Last completed `lastRound` plus separate optional `activeRound` |
| `GET /network/distributions`  | Histograms from the published node snapshot                     |
| `GET /network/nodes`          | Filtered cursor-paginated node list                             |
| `GET /network/nodes/{peerId}` | One published node record                                       |
| `GET /network/history`        | Completed hour/day time buckets                                 |

`activeRound` contains `roundId`, `startedAt`, `lastCheckpointAt`, `candidatePeers`,
`completedPeers`, `addressAttempts`, and optional `blockedReason`. It never replaces or mutates
`lastRound`. The frontend and TUI show active progress separately; completed cards use explicit
peer and address-attempt labels.

The frontend currently renders summary, distributions, trends, and a node table. Daily charts
exclude the incomplete current day. The TUI Peers tab fetches the same API rather than opening
RocksDB directly.

The API may start before the opt-in crawler. Its network-store slot begins empty, retries the
read-only secondary open, and attaches it atomically once the crawler creates or upgrades the
primary; no API restart is required. The normal one-second secondary catch-up loop then keeps it
current. The API never opens the network store read-write.

## Configuration

Per-network `config.toml`:

```toml
[crawler]
enabled = false
round_interval_secs = 900
max_dial_concurrency = 128
dial_timeout_secs = 15
slice_budget_secs = 600
max_frontier = 100000
history_hourly_retention_days = 30
# geoip_city_path = "/path/to/GeoLite2-City.mmdb"
# geoip_asn_path = "/path/to/GeoLite2-ASN.mmdb"
# bootnodes = ["/ip4/127.0.0.1/tcp/8114/p2p/..."]

[store]
network_data_path = "data/network"
```

The dial concurrency, dial timeout, slice budget, and frontier safety bounds must be greater than
zero. In orchestrator mode this is configured inside each
`<orchestrator-root>/<network>/config.toml`.

## Verification

- Engine regression tests cover the historical slice-starvation bug, restart/resume, zero-budget
  non-publication, peer fairness, alias serialization, bounded concurrent probing, exact
  timestamps/counters, TTL, full-round downgrade, frontier overflow, and internal-error
  propagation.
- Real-prober tests use concurrent local tentacle peers and cover same-network success,
  foreign-network Identify, missing-Identify timeout/disconnect, malformed addresses, and protocol
  parsing without public-network CI. Engine tests cover foreign-first aliases and peer-level count.
- Store tests cover the three network CFs, bincode/key round trips, checkpoint progress, reopen,
  and atomic completed-round publication.
- API, frontend, and TUI tests cover the separate completed/active contracts and empty states;
  API tests also cover attaching the secondary after router/API startup.
- `ckbadger crawl --once` remains the manual end-to-end public-network verification command.

## Result

- **Behavior change** — bounded concurrent, peer-fair crawling now persists and resumes one
  logical round until fully drained; only a complete snapshot is published atomically.
- **Storage write path** — all new and changed writes target the mutable, non-chain **network**
  store. Domain and append-only stores are untouched.
- **Re-sync required** — no chain re-sync. Existing pre-change network data needs one clear and
  re-crawl because its status serialization/schema is incompatible.
- **What to do next** — run `ckbadger crawl --once` against the intended network, then confirm
  `/api/v1/network/summary` shows a completed round and no stale active round.
