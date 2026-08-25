# Network Peer Crawler

_Date: 2026-08-25 · Status: Implemented · Owner: ckbadger_

Observe CKB L1 network participation from a local machine, retain exact Discovery, direct-session,
and crawler-dial facts, and publish only completed snapshots. Current configuration, storage, and
HTTP contracts also live in `README.md`, `docs/STORE_SCHEMA.md`, and `docs/API.md`.

## Goal

- Answer “which CKB L1 peers this local stack has observed, how they were observed, and which
  advertised addresses could this crawler dial?” without trusting a hosted dashboard.
- Preserve peers observed through the configured local CKB node's real sessions, including peers
  with no reusable advertised address.
- Preserve work across execution-budget boundaries and process restarts so a slow or large
  frontier eventually completes instead of repeatedly starting from the first addresses.
- Expose completed snapshots, distributions, history, a unified peer table, address-level probe
  evidence, direct-session direction, and honest in-progress telemetry. Counts are observed
  values only; they are never extrapolated.

## Principle Alignment

- **Local First** — crawling is an opt-in service and its data lives in the configured local
  network store.
- **CKB Native** — outbound probes use CKB Identify and Discovery, while direct-session evidence
  comes from the configured CKB node's `local_node_info` and `get_peers` RPC facts. Network
  observations are the one documented non-chain store class and cannot be reconstructed from
  chain replay.
- **Fail Fast** — invalid resource limits, malformed bootnodes, store failures, internal prober
  errors, counter overflow, and frontier overflow propagate with context. Expected remote dial,
  timeout, and handshake failures remain typed observations.
- **Single Calculation Path** — the crawler classifies each completed candidate through the
  store-owned checked outcome/evidence helpers. Status, API, UI, and TUI project those persisted
  facts rather than reinterpreting failures.
- **Exact Evidence** — participation evidence, session initiation direction, advertised aliases,
  and crawler dial results are orthogonal typed facts. No confidence score, NAT/firewall cause,
  “home node”, or global liveness claim is inferred from their combination.

## Non-Goals

- Fiber L2 discovery.
- A claim to know every node on the network. Results cover configured seeds and recursively
  advertised addresses, plus the sessions visible to the configured local CKB node, during a
  completed logical round.
- A live connection graph. `known_peers` is bounded address-book gossip, not proof of a live edge.
- NAT, firewall, hosting environment, or node-operator classification. A failed crawler dial is
  only a failed attempt from this crawler to that alias at that time. Even when a peer-initiated
  direct session is also observed, the evidence does not prove why reverse dialing failed.
- A persistent crawler-prober p2p identity across process restarts. Persisting that private key is
  deferred; only the configured local CKB observer's reported `peerId` is durable evidence here.
- Chain-integrity verification. The observational network store is outside `ckbadger verify`.

## Architecture

The `ckbadger-crawler` service in `crates/crawler/` runs under the CLI supervisor beside the
indexer, API, and frontend.

```text
CKB p2p network ── Identify / Discovery ────────▶ ckbadger-crawler
configured CKB RPC ── local_node_info/get_peers ─▶       │ sole writer
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

The prober has no inbound listener. Its Discovery `GetNodes` request therefore uses version 0 and
no listen port; version 1 would incorrectly invite the remote to reuse and gossip the crawler's
ephemeral outbound source port. During the bounded Discovery grace period, all valid regular
response and `announce` `Nodes` payloads are unioned. An announce received first is retained but
does not end the wait for the requested non-announce response. Omission from a later randomized
payload is not negative evidence and never erases an earlier positive address observation.

The expected Identify network name is derived from `[ckb].network` and its genesis hash. Before
accepting configured-node RPC observations, the crawler also compares `get_block_hash(0)` with the
configured network's exact genesis hash. A mismatch fails with both hashes and publishes none of
those observations. Mainnet and testnet are supported. A mismatched Identify is a `ForeignNetwork`
address observation; the scheduler still tries that PeerId's remaining aliases. If none succeeds
and at least one alias was authenticated as foreign, the candidate receives one `ForeignNetwork`
completed outcome. It is not called globally unreachable.

Bootnodes come from `[crawler].bootnodes` when set, otherwise the network's built-in list.
Unsupported networks, an empty resolved seed list, a malformed bootnode, or a zero resource bound
fails before crawling.

## Network Store

The crawler is the sole writer of the separate network store at `[store].network_data_path`
(default `data/network`). The API opens it as a read-only secondary. Its three column families are:

| CF             | Key                                   | Value                            | Responsibility                                                     |
| -------------- | ------------------------------------- | -------------------------------- | ------------------------------------------------------------------ |
| `CF_NET_NODES` | raw peer id                           | `NodeRecord`                     | TTL-retained same-network crawler-Identify records                 |
| `CF_NET_STATS` | status singleton or history key       | `LatestStatus` / `HistoryPoint`  | Completed dial/session evidence and hour/day history               |
| `CF_NET_CRAWL` | active singleton or peer-prefixed key | `ActiveCrawl` / `CrawlCandidate` | Durable frontier, advertisements, sessions, and completed evidence |

`NodeRecord` exists only after an outbound crawler probe receives an authenticated same-network
Identify. It retains own addresses, client version, capability flags, protocols, first/last seen,
last reachable time, latest-round reachability, optional Geo/ASN, RTT, exact last Discovery
counters, and resolved advertised peer references. `known_peers` is address-book gossip, not a
live topology edge.

Advertisements are retained target-centrically on the advertised `CrawlCandidate`; no additional
column family or reverse scan of `NodeRecord.known_peers` is required for a peer detail lookup.
Each positive `(advertiser_peer_id, alias)` fact keeps exact first/latest observation times,
first/latest completed round ids, and an observation count. A later Discovery response that omits
the target does not retract the fact. Evidence is removed only with its expired target alias under
the normal candidate TTL policy.

`LatestStatus` persists one disjoint peer-outcome matrix:

```text
same_network_identified
exhausted_with_retained_verification
exhausted_without_retained_verification
foreign_with_retained_verification
foreign_without_retained_verification
```

It also persists a disjoint histogram for every `AddressProbeResult`, aggregate Discovery evidence,
`malformed_addresses`, `new_verified_peers`, longitudinal `local_observer` evidence, and the
completed round's `direct_session_observations` split into observer- and peer-initiated counts.
Checked helpers derive `candidatePeers`,
`verifiedRetainedPeers`, `reachablePeers`, `verifiedUnavailablePeers`, `exhaustedCandidates`,
`foreignPeers`, `addressAttempts`, and `nonSuccessfulAddressAttempts`; those totals are not
independently mutable fields.

`CF_NET_CRAWL` stores one peer-keyed candidate with retained dial aliases, target-centric
`advertisements`, current-round `staged_direct_sessions`, and longitudinal `direct_sessions`. Its
`active` probe state is resumable operational state for one round. Its `last_completed` evidence is
immutable while a later round is active and contains one terminal peer outcome plus at most one
typed observation per attempted alias. `ActiveCrawl.local_observer_observation` is the durable
once-per-round RPC marker, while `direct_session_targets` identifies the candidate rows checkpointed
with it. Newly observed advertisements, direct sessions, and successful probes are staged in active
round state. A slice checkpoint atomically writes that staged state and changed candidates; it
never changes published nodes or latest status.

When the frontier is drained, one RocksDB write batch atomically:

- publishes verified-node upserts and TTL deletions;
- moves each terminal active probe to `last_completed`, merges staged positive advertisement and
  direct-session observations into durable target records, and retains/prunes candidates;
- writes the checked outcome matrix, address/Discovery histograms, and history buckets;
- removes the active-round singleton.

Consequently readers see either the preceding completed snapshot or the next completed snapshot,
never partially downgraded reachability or a status that does not match its nodes.

This implementation changes serialized network values but adds no column family: ckbadger-store
remains 63 CFs total (59 domain + 1 append-only + 3 network). Recreate the
development network primary and API secondary once, then crawl again. For the default mainnet work
directory these are `work/mainnet/data/network` and
`work/mainnet/data/network-api-secondary`. Domain and append-only stores are untouched; no chain
re-sync is required.

## Crawl Model

A **logical round** is the publication unit. An **execution slice** is only a bounded amount of
admission work inside that round. One logical round can span any number of slices and restarts.

1. If `CF_NET_CRAWL` has an active round, load it and resume. Otherwise allocate the next round id.
2. Merge bootnodes, prior node own-addresses, retained candidate aliases, and newly discovered
   addresses into a durable peer-keyed frontier. Merge configured-node session peer IDs as
   participants, but never promote RPC session addresses into crawler dial aliases.
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
7. Only after every schedulable dial candidate is terminal, build and validate the next complete
   verified-node and session snapshot, downgrade unobserved retained crawler records, apply TTL
   retention, classify the disjoint matrices, materialize history, and publish everything
   atomically. Addressless direct-only candidates never acquire a dial-probe state.
8. In continuous mode, wait `round_interval_secs` only after a logical round completes. With
   `crawl --once`, success means one complete logical round was published.
   The completed-round log prints checked peer-outcome projections, all six address-result buckets,
   and Discovery counters. It is the operational acceptance surface for comparing a real
   `crawl --once` run with persisted/API evidence without introducing another calculation path.

TTL pruning is deliberately two-stage for evidence traceability. A candidate attempted in round
N remains present with round N `last_completed` evidence even if its last advertisement has just
expired. If it has no retained verified record at the start of round N+1, it is not reactivated or
counted; the N+1 atomic commit applies the exact alias/advertisement and direct-session TTL
transitions, then deletes it if no positive evidence remains. Thus every latest-round summary row
remains drillable until a newer completed round replaces the summary.

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

The configured local CKB node contributes a different, positive observation path:

| RPC fact                      | Exact meaning from the configured local observer's vantage               |
| ----------------------------- | ------------------------------------------------------------------------ |
| `local_node_info`             | The configured CKB node's own peer identity and advertised node metadata |
| `get_peers.is_outbound=true`  | The local observer initiated this real CKB session                       |
| `get_peers.is_outbound=false` | The remote peer initiated this real CKB session to the observer          |

The public direction values are therefore `observerInitiated` and `peerInitiated`. “Outbound” is
never interpreted from the remote peer's perspective. A peer-initiated session proves that the
remote peer participated in a real session to this observer at that observation time; a separate
failed crawler probe proves only that this crawler did not establish a new connection to the
attempted alias. Together they still do not identify NAT, firewall, or any other cause.

`get_peers` may report no address for a valid session, and an inbound connection may expose a
temporary source port. Addressless peers are retained as direct-session-only participants. Any
reported session addresses remain session metadata and are never admitted to the crawler frontier
or used as dial aliases.

Completed direct-session evidence is target-centric and keyed by
`(observer_peer_id, initiator)`. It retains exact first/latest observation times and rounds,
observation count, and the latest session metadata. Completed local-observer evidence separately
retains the corresponding longitudinal `local_node_info` facts. Failure to see the same session in
a later round does not retroactively negate the earlier positive participation observation. A
separate direct-session time cutoff eventually expires it; advertised or successfully dialed
aliases do not refresh session evidence.

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

Discovery is independent evidence captured after Identify. The no-listener crawler sends
`GetNodes` version 0 with no listen port. Checked counters distinguish `valid_nodes_messages`,
`valid_response_messages`, `valid_announce_messages`, malformed/unexpected messages, normalized
advertised addresses, and rejected advertised addresses. Responses plus announces must exactly
equal all valid `Nodes` messages. Valid regular responses and announces are unioned during the
grace window; only a non-announce response satisfies the request wait. No reply is all-zero
evidence; a valid empty response has positive valid-response and valid-Nodes counts with zero
advertised-address counts.

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

| Endpoint                      | Purpose                                                         |
| ----------------------------- | --------------------------------------------------------------- |
| `GET /network/summary`        | Completed dial/session projections plus active progress         |
| `GET /network/distributions`  | Histograms explicitly scoped to retained verified records       |
| `GET /network/peers`          | Unified observed peers, including addressless direct-only peers |
| `GET /network/peers/{peerId}` | Advertisements, direct sessions, typed probes, and verification |
| `GET /network/history`        | `verifiedPeers`/`reachablePeers` and share hour/day buckets     |

`activeRound` contains `roundId`, `startedAt`, `lastCheckpointAt`, `candidatePeers`,
`completedPeers`, `addressAttempts`, and optional `blockedReason`. It never replaces or mutates
`lastRound`. The frontend and TUI show active progress separately; completed cards use explicit
peer and address-attempt labels.

`lastRound.localObserver` exposes the latest longitudinal configured-node evidence, while
`directSessionObservations.{observerInitiated,peerInitiated}` counts the exact `get_peers` rows in
that completed round. They are evidence fields for API/TUI consumers, not additional history
series or inferred network-health cards. The TUI renders only those current-round counts as
**peer → configured observer** and **observer → peer**.

The frontend renders the four crawler terms **Advertised candidates**, **Same-network reachable**,
**Verified retained**, and **Verified unavailable**, alongside direct-participation and session
direction evidence. `crawlerDialState` is separate from
`participation.{discoveryAdvertised,directSessionObserved,crawlerIdentified}` and the distinct
`sessionInitiators`; list order uses the latest retained alias/advertisement, direct-session, or
crawler-Identify positive fact. The table renders those participation facts as **Discovery ad**,
**Direct CKB session**, and
**Identify**, and renders direction literally as **observer → peer** or **peer → observer**. The
arrows name connection initiation, not data-flow direction. The peer table can expand an evidence
drawer showing observation vantage, retained dial aliases, completed and active probe evidence,
longitudinal direct sessions, last successful verification, and exact target-centric advertisers.
A direct-only peer's primary address, advertisement times, and dial-observation time are `null`,
not fabricated from its session connection. Candidate-only version/Geo/ASN/RTT values are also
`null` in the API and `—` in the UI. The TUI uses the same terms and derives the
verified-unavailable trend only from aligned exact history buckets;
duplicate/missing buckets or `reachablePeers > verifiedPeers` render an explicit error.

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
  Identify deadlines, Discovery v0 with no advertised listen port, response/announce union, valid
  empty versus absent Discovery, malformed/unexpected Discovery, unhealthy service state, and
  persisted malformed-address invariants without public-network CI.
- RPC-observer tests cover exact genesis guarding, both `is_outbound` directions, canonical
  session metadata, and valid addressless direct sessions.
- Store tests cover bincode/key round trips, reopen, atomic publication, and rejection of
  inconsistent matrices, unretained aliases, malformed target-centric advertisements,
  direct-session drift, and incorrect new-verification counts.
- API tests cover checked projections, summary observer/session fields, orthogonal participation
  and crawler-dial facts, addressless direct-only peers, nullable alias metadata, target-centric
  advertisers/sessions, filtering/pagination, and completed/active separation. Frontend tests
  cover the exact participation labels, direction arrows, evidence drawer, and API contract; TUI
  tests cover additive summary parsing, evidence terminology, and trend invariants.
- `ckbadger crawl --once` remains the manual end-to-end public-network verification command.

## Result

- **Behavior change** — Discovery advertisements, direct CKB sessions and their observer-relative
  initiation direction, same-network crawler verification, retained dialability, and exact address
  milestones are independently inspectable. Addressless direct-only peers remain visible, while
  ambiguous `totalKnown`, `unreachablePeers`, NAT/home-node inference, and `/network/nodes`
  contracts are absent.
- **Storage write path** — all new and changed writes target the mutable, non-chain **network**
  store. Domain and append-only stores are untouched.
- **Re-sync required** — no chain re-sync. Recreate only the 3-CF network primary and its API
  secondary, then crawl again, because serialized network values are incompatible; the 59-CF
  domain and 1-CF append-only stores are untouched.
- **What to do next** — run `ckbadger crawl --once` against the intended network, then confirm
  `/api/v1/network/summary` shows a completed round and no stale active round. Persistent crawler
  prober identity remains deferred; add it before making any cross-restart claim about a stable
  outbound-crawler p2p identity.
