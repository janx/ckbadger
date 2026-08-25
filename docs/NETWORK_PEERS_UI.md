# Network Peer Evidence UI

_Date: 2026-08-25 · Status: Implemented · Depends on: `docs/NETWORK_PEER_CRAWLER.md`_

The read-only `/network` page surfaces local CKB L1 crawler evidence without turning an address
advertisement or one observer's failed probe into a global validity claim. The canonical HTTP
contract lives in `docs/API.md`; this document owns the browser presentation and interaction.

## Goal

- Keep advertised candidates, same-network verification, current-round reachability, and retained
  verification visibly distinct.
- Let a summary count drill down to a candidate and then to exact aliases, typed observations,
  timestamps, elapsed time, and Discovery advertisers.
- Preserve the completed snapshot while a later crawl round is active.
- Keep candidate-only version, Geo, ASN, protocol, and RTT metadata absent rather than fabricated.

## Principle Alignment

- **Local First** — all data comes from this instance's network-store secondary; the page polls
  read-only endpoints and needs no hosted observer.
- **CKB Native** — “verified” means an authenticated peer returned a valid Identify for the
  configured CKB network. A parseable multiaddr, open TCP port, or advertisement is insufficient.
- **Single Calculation Path** — summary totals are checked API projections of the persisted
  outcome matrices. The browser renders them and does not independently reinterpret the matrix.
- **Exact Evidence** — the UI presents typed address milestones and Discovery counters, not a
  confidence score.
- **Honest Empty State** — crawler disabled/not-yet-completed is onboarding, while request or
  invariant failures remain errors.

## Data Boundary

The API reads the three network column families through one pinned secondary view:

- `CF_NET_NODES`: optional TTL-retained verification metadata;
- `CF_NET_STATS`: last completed outcome/address/Discovery aggregates and history;
- `CF_NET_CRAWL`: unified candidates, stable last-completed evidence, and optional active evidence.

The crawler remains the sole writer. The frontend never writes RocksDB.

## API Consumption

| Request                                                      | Browser use                                                                         |
| ------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| `GET /network/summary`                                       | onboarding/dashboard switch, four primary counts, completed and active round labels |
| `GET /network/distributions`                                 | version/country/ASN/protocol buckets scoped to retained verified records            |
| `GET /network/history?metric=verifiedPeers&granularity=day`  | verified retained trend                                                             |
| `GET /network/history?metric=reachablePeers&granularity=day` | same-network reachable trend                                                        |
| `GET /network/peers`                                         | unified candidate table, filters, cursor pagination                                 |
| `GET /network/peers/{peerId}`                                | on-demand evidence drawer                                                           |

The page uses TanStack Query and a 30-second refresh interval for summary/distributions. Peer
details are fetched only when a row is expanded. Daily history excludes the incomplete current
day.

## Page States

The page title is **Peers** to distinguish the p2p network from chain-health “Network” widgets.

### Loading

Render a summary/page skeleton while `/network/summary` is unresolved.

### Disabled or waiting

If `enabled=false`, or no completed status exists, show onboarding:

- explain that the crawler is opt-in and local-first;
- show `[crawler] enabled = true` configuration guidance;
- state that outbound whole-network crawling occurs;
- explain optional MaxMind GeoLite2 configuration;
- if a first round is active, show its exact round id, completed/candidate counts, address attempts,
  and blocked reason.

### Completed dashboard

Continue rendering `lastRound` even when `activeRound` exists. The active round appears as a
separately labeled progress badge and never replaces completed evidence.

## Canonical Labels

The four primary cards are:

| Label                      | Exact set                                                        |
| -------------------------- | ---------------------------------------------------------------- |
| **Advertised candidates**  | all candidates classified in the last completed round            |
| **Same-network reachable** | candidates with a successful same-network Identify in that round |
| **Verified retained**      | TTL-retained peer records verified in this or an earlier round   |
| **Verified unavailable**   | retained records without a same-network success in that round    |

The detail line separately shows address attempts, non-successful address observations, exhausted
candidates, foreign-network candidates, and newly verified peers. Never use the removed labels
“Total Known”, “Failed Peer Candidates”, or a bare “Unreachable” peer state.

The evidence-boundary notice must remain visible: an advertisement is not verification, and this
instance's observations are not an estimate of the complete network.

## Distributions and Trends

Distributions are explicitly titled/scoped to retained verification records. They include version,
country, ASN, and protocol buckets plus the exact reachable/unavailable/retained split. Candidate-
only rows never contribute null metadata to these histograms.

The trend component requests `verifiedPeers` and `reachablePeers`. Its two visible areas are
same-network reachable and verified unavailable, whose sum equals verified retained. History
pairing uses matching timestamps and checked subtraction; missing/duplicate buckets or
`reachablePeers > verifiedPeers` are errors, not skipped points or zero repairs.

## Unified Peer Table

`/network/peers` returns candidates whether or not a `NodeRecord` exists. Rows sort by
`lastAdvertisedAt` descending and then peer id, and support cursor pagination plus exact state,
typed address-observation, country, and version filters. The observation selector exposes all six
`AddressProbeResult` values; an unknown state or observation is an API `400`, including when the
network store is not attached yet.

| Display state            | Derivation                                                                |
| ------------------------ | ------------------------------------------------------------------------- |
| `reachable`              | last completed outcome is same-network Identify                           |
| `verifiedUnavailable`    | aliases exhausted and a retained verification record exists               |
| `advertisedUnverified`   | aliases exhausted and no retained verification exists                     |
| `foreignNetwork`         | at least one authenticated foreign Identify, with no same-network success |
| `noCompletedObservation` | candidate has no completed evidence yet                                   |

The table columns are peer id, primary retained alias, evidence state, version, country, ASN,
advertisement time, observation time, last verification, and RTT. Candidate-only metadata is API
`null` and renders as `—`.

## Evidence Drawer

Expanding a row fetches `/network/peers/{peerId}` and shows:

- observation vantage: **this ckbadger instance**;
- first/last advertisement times for every retained alias;
- last completed round/outcome, with the consecutive exhaustion count only for an exhausted
  outcome;
- each completed alias observation with typed result, observation time, and exact elapsed ms;
- separately labeled active-round state and observations, if present;
- optional retained verification metadata and last same-network Identify time;
- exact valid, malformed, and unexpected Discovery message counts plus normalized and rejected
  advertised-address counts from the last successful probe;
- exact retained advertiser peer ids and advertiser observation times.

Typed observation labels are: Dial request failed; No authenticated session before deadline;
Authenticated session without Identify before deadline; Malformed Identify; Foreign network; and
Same-network Identify completed. None is expanded into an unobserved root cause.

## Discovery and Accessibility

- The `g p` shortcut and command palette open the Peers page; no permanent navigation item is
  required.
- Expand buttons have stateful accessible labels and `aria-expanded`.
- Peer ids and addresses retain full-value tooltips while visual text may be truncated.
- MaxMind attribution appears only when at least one non-`Unknown` country is present.

## Verification

- API tests cover the 144/57/87/58 projection, unified candidates, nullable candidate metadata,
  filters/pagination, detail advertisers, and active/completed separation.
- Component tests cover exact labels, onboarding, active progress, distributions/trends, state and
  typed-observation filters, evidence expansion, null metadata rendering, command discovery, and
  MaxMind attribution.
- API-client/MSW tests enforce `/network/peers`, detail, and renamed history metrics.
- Tests explicitly prohibit the removed ambiguous labels.

## Result

- **Behavior change** — the browser now exposes one evidence-oriented candidate/verified view with
  traceable address observations instead of relabeling verified records as all known peers.
- **Re-sync required** — no chain re-sync. The backing schema change requires recreating only the
  network primary and API secondary, then crawling again.
- **What to do next** — after a rebuilt crawl, compare the summary matrix with its derived cards
  and inspect representative rows and evidence drawers for every display state.
