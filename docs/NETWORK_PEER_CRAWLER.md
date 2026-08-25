# Network Peer Crawler

_Date: 2026-08-25 · Status: Implemented · Owner: ckbadger_

Discover the reachable CKB L1 p2p network from a local machine, retain exact observations, and
publish only completed crawl snapshots. Current configuration, storage, and HTTP contracts also
live in `README.md`, `docs/STORE_SCHEMA.md`, and `docs/API.md`.

## Goal

- Answer “which CKB L1 peers can this crawler reach, and how is that set changing?” without
  trusting a hosted dashboard.
- Preserve work across execution-budget boundaries and process restarts so a slow or large
  frontier eventually completes instead of repeatedly starting from the first addresses.
- Expose completed snapshots, distributions, history, a unified candidate/verified peer table,
  address-level evidence, and honest in-progress telemetry. Counts are observed values only;
  they are never extrapolated.

## Principle Alignment

- **Local First** — crawling is an opt-in service and its data lives in the configured local
  network store.
- **CKB Native** — Identify and Discovery use CKB's p2p protocols. Network observations are the
  one documented non-chain store class and cannot be reconstructed from chain replay.
- **Fail Fast** — invalid resource limits, malformed bootnodes, store failures, internal prober
  errors, counter overflow, and frontier overflow propagate with context. Expected remote dial,
  timeout, and handshake failures remain typed observations.
- **Single Calculation Path** — the crawler classifies each completed candidate through the
  store-owned checked outcome/evidence helpers. Status, API, UI, and TUI project those persisted
  facts rather than reinterpreting failures.
- **Exact Evidence** — candidate outcomes, retained verification, address probe milestones, and
  Discovery response counters are separate typed facts. No confidence score or global liveness
  claim is inferred from them.

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
authenticated as foreign, the candidate receives one `ForeignNetwork` completed outcome. It is
not called globally unreachable.

Bootnodes come from `[crawler].bootnodes` when set, otherwise the network's built-in list.
Unsupported networks, an empty resolved seed list, a malformed bootnode, or a zero resource bound
fails before crawling.

## Network Store

The crawler is the sole writer of the separate network store at `[store].network_data_path`
(default `data/network`). The API opens it as a read-only secondary. Its three column families are:

| CF             | Key                                   | Value                            | Responsibility                                               |
| -------------- | ------------------------------------- | -------------------------------- | ------------------------------------------------------------ |
| `CF_NET_NODES` | raw peer id                           | `NodeRecord`                     | TTL-retained same-network verification records               |
| `CF_NET_STATS` | status singleton or history key       | `LatestStatus` / `HistoryPoint`  | Completed evidence aggregates and hour/day history           |
| `CF_NET_CRAWL` | active singleton or peer-prefixed key | `ActiveCrawl` / `CrawlCandidate` | Durable frontier, active probe state, and completed evidence |

`NodeRecord` exists only after an authenticated same-network Identify. It retains own addresses,
client version, capability flags, protocols, first/last seen, last reachable time, latest-round
reachability, optional Geo/ASN, RTT, exact last Discovery counters, and resolved advertised peer
references. `known_peers` is address-book gossip, not a live topology edge.

`LatestStatus` persists one disjoint peer-outcome matrix:

```text
same_network_identified
exhausted_with_retained_verification
exhausted_without_retained_verification
foreign_with_retained_verification
foreign_without_retained_verification
```

It also persists a disjoint histogram for every `AddressProbeResult`, aggregate Discovery evidence,
`malformed_addresses`, and `new_verified_peers`. Checked helpers derive `candidatePeers`,
`verifiedRetainedPeers`, `reachablePeers`, `verifiedUnavailablePeers`, `exhaustedCandidates`,
`foreignPeers`, `addressAttempts`, and `nonSuccessfulAddressAttempts`; those totals are not
independently mutable fields.

`CF_NET_CRAWL` stores one peer-keyed candidate with all retained aliases. Its `active` probe state
is resumable operational state for one round. Its `last_completed` evidence is immutable while a
later round is active and contains one terminal peer outcome plus at most one typed observation per
attempted alias. A successful observation is staged only in `active` until the logical round
finishes. A slice checkpoint atomically writes active metadata and changed candidates; it never
changes published nodes or latest status.

When the frontier is drained, one RocksDB write batch atomically:

- publishes verified-node upserts and TTL deletions;
- moves each terminal active probe to `last_completed` and retains/prunes candidates;
- writes the checked outcome matrix, address/Discovery histograms, and history buckets;
- removes the active-round singleton.

Consequently readers see either the preceding completed snapshot or the next completed snapshot,
never partially downgraded reachability or a status that does not match its nodes.

This implementation changes every serialized network value but adds no column family. Recreate the
development network primary and API secondary once, then crawl again. For the default mainnet work
directory these are `work/mainnet/data/network` and
`work/mainnet/data/network-api-secondary`. Domain and append-only stores are untouched; no chain
re-sync is required.

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
   own/Discovery addresses. A non-successful address observation, including a foreign-network
   Identify, retries another alias only after pending peers have had a turn. After all aliases
   finish, a candidate with any authenticated foreign observation is classified foreign;
   otherwise its aliases are classified exhausted from this crawler in this round.
6. If work remains, return exact active progress and immediately start the next slice. On process
   restart the same round resumes from the last checkpoint.
7. Only after every candidate is terminal, build and validate the next complete verified-node
   snapshot, downgrade unobserved retained records, apply TTL retention, classify the disjoint
   matrices, materialize history, and publish everything atomically.
8. In continuous mode, wait `round_interval_secs` only after a logical round completes. With
   `crawl --once`, success means one complete logical round was published.

The completed-round log prints checked peer-outcome projections, all six address-result buckets,
and Discovery counters. It is the operational acceptance surface for comparing a real
`crawl --once` run with persisted/API evidence without introducing another calculation path.

TTL pruning is deliberately two-stage for evidence traceability. A candidate attempted in round
N remains present with round N `last_completed` evidence even if its last advertisement has just
expired. If it has no retained verified record at the start of round N+1, it is not reactivated or
counted; the N+1 atomic commit deletes that unchanged candidate. Thus every latest-round summary
row remains drillable until a newer completed round replaces the summary.

`slice_budget_secs` is an admission/checkpoint budget, not a coverage cutoff. `max_frontier` is a
hard safety invariant over durable candidate addresses. Overflow atomically rejects the source
result and candidate additions, preserves the preceding checkpoint, stores an actionable
`blocked_reason`, and returns an error without truncating or publishing the round. Raising the
limit resumes by safely retrying that source.

## Observation and Error Semantics

Every attempted alias stores exactly one terminal milestone:

| `AddressProbeResult`                                | Exact observation boundary                                       |
| --------------------------------------------------- | ---------------------------------------------------------------- |
| `DialRequestFailed`                                 | tentacle rejected the address-keyed dial request                 |
| `NoAuthenticatedSessionBeforeDeadline`              | no authenticated transport session before the deadline           |
| `AuthenticatedSessionWithoutIdentifyBeforeDeadline` | authenticated session, but no valid Identify before the deadline |
| `MalformedIdentify`                                 | authenticated peer sent an invalid Identify payload              |
| `ForeignNetwork`                                    | valid Identify names a different CKB network                     |
| `SameNetworkIdentified`                             | authenticated peer returned a valid configured-network Identify  |

The first three results do not identify a remote root cause. TCP preflight, ping, DNS, and RPC
probing are not alternate verification paths. Only `SameNetworkIdentified` creates or refreshes a
`NodeRecord`.

Tentacle callbacks are correlated by expected peer id, canonical dial address, and authenticated
session id. A late event from one alias cannot mutate a later alias attempt for the same peer. The
Identify boundary uses one absolute monotonic deadline and timestamps each terminal event; after
the deadline the prober cleans up only after an address-matched dial terminal or authenticated
session arrives. Failure to receive either within the local terminal-delivery watchdog marks the
prober unhealthy and aborts publication instead of turning a local event-delivery failure into a
remote observation.

Address evidence elapsed time is measured from probe start to the actual callback timestamp for
same-network, foreign-network, malformed-Identify, and dial-request results. Both deadline results
use the single absolute Identify deadline minus probe start. Poll wake-up delay and the Discovery
grace window never enter persisted Identify RTT evidence.

Discovery is independent evidence captured after Identify. Checked counters distinguish valid
`Nodes` messages, malformed messages, unexpected messages, normalized advertised addresses, and
rejected advertised addresses. No reply is all-zero evidence; a valid empty `Nodes` reply has a
positive `valid_nodes_messages` count and zero advertised-address counts.

| Other condition                                                    | Handling                                                 |
| ------------------------------------------------------------------ | -------------------------------------------------------- |
| Malformed newly discovered address                                 | count `malformed_addresses`; do not invent a candidate   |
| Malformed scheduled persisted alias                                | invariant error with peer/address/round; publish nothing |
| Geo/ASN miss                                                       | honest `None`; an empty country is discarded             |
| Service termination, closed control, poisoned state, local failure | return error and do not publish the logical round        |
| Crash or process restart                                           | retain checkpoints and resume the active round           |
| Frontier limit exceeded                                            | persist blocked reason, return error, publish nothing    |

Before publication, the store reconstructs the outcome matrix and address histogram from exact
candidate evidence using the same checked classification helpers as the crawler. It rejects
unknown or duplicate aliases, mismatched outcome/result combinations, a new verified record
without a same-network success, per-peer reachability disagreement, count overflow,
snapshot/matrix drift, Discovery aggregate drift, and a non-terminal or wrong-round candidate. It
also requires each publication to be the exact transition from the durable active checkpoint and
requires the checkpoint, rebuilt evidence, and status histograms to match. Successful staged
evidence uniquely determines the published node fields and known-peer edges (Geo/ASN remain the
lookup result); an unavailable retained node may change only `reachable` to false. Candidate
observations must fall inside the persisted round clock, and the successful observation timestamp
must equal the staged-success timestamp. The failure happens before the RocksDB batch is written,
so the preceding completed snapshot remains intact.

## API and UI

All `/api/v1/network/*` handlers are secondary/read-only.

| Endpoint                      | Purpose                                                             |
| ----------------------------- | ------------------------------------------------------------------- |
| `GET /network/summary`        | Checked completed outcome/evidence projections plus active progress |
| `GET /network/distributions`  | Histograms explicitly scoped to retained verified records           |
| `GET /network/peers`          | Unified candidate list with nullable verified metadata              |
| `GET /network/peers/{peerId}` | Aliases, typed probes, retained verification, Discovery advertisers |
| `GET /network/history`        | `verifiedPeers`/`reachablePeers` and share hour/day buckets         |

`activeRound` contains `roundId`, `startedAt`, `lastCheckpointAt`, `candidatePeers`,
`completedPeers`, `addressAttempts`, and optional `blockedReason`. It never replaces or mutates
`lastRound`. The frontend and TUI show active progress separately; completed cards use explicit
peer and address-attempt labels.

The frontend renders the four primary terms **Advertised candidates**, **Same-network reachable**,
**Verified retained**, and **Verified unavailable**. The peer table can expand an evidence drawer
showing observation vantage, retained aliases, completed and active probe evidence, last successful
verification, and exact advertisers. Candidate-only version/Geo/ASN/RTT values are `null` in the
API and `—` in the UI, never fabricated node metadata. The TUI uses the same terms and derives the
verified-unavailable trend only from aligned exact history buckets; duplicate/missing buckets or
`reachablePeers > verifiedPeers` render an explicit error.

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

- Engine regression tests cover the 144/57/87/58 semantic split, completed evidence surviving a
  later active round, restart/resume, peer fairness, alias serialization, concurrent probing,
  exact histograms, consecutive exhaustion, TTL, frontier overflow, and internal errors.
- Real-prober tests cover same/foreign network Identify, pre-authentication versus authenticated
  Identify deadlines, valid empty versus absent Discovery, malformed/unexpected Discovery,
  unhealthy service state, and persisted malformed-address invariants without public-network CI.
- Store tests cover bincode/key round trips, reopen, atomic publication, and rejection of
  inconsistent matrices, unretained aliases, and incorrect new-verification counts.
- API tests cover checked projections, unified candidates, nullable candidate metadata, detail
  advertisers, filtering/pagination, and completed/active separation. Frontend and TUI tests cover
  the exact labels, evidence drawer, API contract, and trend invariants.
- `ckbadger crawl --once` remains the manual end-to-end public-network verification command.

## Result

- **Behavior change** — advertised candidates, same-network verification, current-round
  reachability, retained verification, address milestones, and Discovery evidence are independently
  inspectable. Ambiguous `totalKnown`, `unreachablePeers`, and `/network/nodes` contracts are gone.
- **Storage write path** — all new and changed writes target the mutable, non-chain **network**
  store. Domain and append-only stores are untouched.
- **Re-sync required** — no chain re-sync. Recreate only the network primary and its API secondary,
  then crawl again, because serialized network values are incompatible.
- **What to do next** — run `ckbadger crawl --once` against the intended network, then confirm
  `/api/v1/network/summary` shows a completed round and no stale active round.
