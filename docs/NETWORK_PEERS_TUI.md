# Network Peer Evidence TUI

_Date: 2026-08-25 · Status: Implemented · Depends on: `docs/NETWORK_PEER_CRAWLER.md`_

The `ckbadger tui` **Peers** tab presents the same completed verification evidence as the browser in
a compact terminal layout. It calls the read-only API; it does not open or mutate the network
store.

## Goal

- Give local operators an immediate view of crawler state and exact peer categories.
- Keep advertisement, same-network reachability, and retained verification distinct in limited
  terminal space.
- Render an actionable error when history buckets cannot support an exact trend.

## Principle Alignment

- **Local First** — the tab consumes the selected local network's API.
- **CKB Native** — same-network Identify is the verification boundary.
- **Single Calculation Path** — summary and distribution totals come from the API's checked
  projections; the TUI does not reinterpret the persisted outcome matrix.
- **Fail Fast** — trend pairing rejects duplicate/missing timestamps and reachable counts greater
  than retained verified counts. It does not skip points, clamp subtraction, or fill missing data.

## API Model

`crates/tui/src/db.rs` decodes:

```rust
NetworkLastRound {
    round_id, started_at, finished_at,
    candidate_peers, verified_retained_peers,
    reachable_peers, verified_unavailable_peers,
    exhausted_candidates, foreign_peers,
    address_attempts, non_successful_address_attempts,
    malformed_addresses, new_verified_peers,
}

NetworkActiveRound {
    round_id, started_at, last_checkpoint_at,
    candidate_peers, completed_peers, address_attempts,
    blocked_reason,
}

NetworkDistributions {
    verified_retained, same_network_reachable, verified_unavailable,
    versions, countries, asns, protocols,
}
```

The tab fetches `/network/summary` first. With completed data, it concurrently fetches:

- `/network/distributions`;
- `/network/history?metric=verifiedPeers&granularity=hour&from=...`;
- `/network/history?metric=reachablePeers&granularity=hour&from=...`.

History is bounded to the recent 48-hour window so payload size remains flat.

## View States

`peers_view_state` chooses one presentation:

| State     | Condition                                       | Presentation                 |
| --------- | ----------------------------------------------- | ---------------------------- |
| Disabled  | `summary.enabled == false`                      | configuration hint           |
| Waiting   | enabled, no completed round or active round     | waiting message              |
| Active    | first round active without prior completed data | round progress               |
| Blocked   | first round has `blockedReason`                 | red actionable reason        |
| Dashboard | completed `lastRound` exists                    | status, distributions, trend |
| Error     | summary unavailable                             | crawler/API error            |

When a later round is active, the dashboard continues to show the prior completed snapshot and adds
a separately labeled `CRAWLING` or `BLOCKED` badge with that active round's id and progress.

## Dashboard Layout

### Crawler Status

The status panel shows:

1. completed round id/age plus optional active badge;
2. **Advertised candidates** and **Same-network reachable**;
3. **Verified retained** and **Verified unavailable**;
4. exact address attempts, non-successful observations, exhausted candidates, and newly verified;
5. foreign-network candidates and malformed advertised addresses;
6. active blocked reason, when present.

The removed terms “Total Known”, “Failed Peer Candidates”, and a bare “Unreachable” state must not
appear.

### Distributions

Version and country top-N charts are computed by the API over retained verified records. A footer
shows the exact same-network reachable, verified unavailable, and verified retained split. ASN and
protocol data are decoded for contract completeness even when the compact panel omits them.

### Verified Peer Trend

`peers_trend_series` joins `verifiedPeers` and `reachablePeers` by exact Unix timestamp. For every
bucket:

```text
verifiedUnavailable = verifiedPeers - reachablePeers
```

The subtraction is checked. Each timestamp must occur exactly once in each series. An extra,
missing, duplicate, or inverted bucket returns a contextual error that is rendered inside the
trend panel. The stacked chart displays same-network reachable plus verified unavailable; their
height is verified retained.

## Interaction

- `4` selects the Peers tab directly.
- `Tab` / `Shift+Tab` cycle through main tabs.
- In orchestrator mode, the network switcher changes the API endpoint and reloads that network's
  crawler evidence.
- Existing refresh cadence updates the tab; no WebSocket path is introduced.

## Verification

- Decode tests enforce the new camelCase summary/distribution contract and reject legacy fixtures.
- Fetch tests assert the `verifiedPeers`/`reachablePeers` history requests.
- View-state tests cover disabled, waiting, active, blocked, dashboard, and error cases.
- Trend tests cover aligned buckets, missing peers, duplicate timestamps, and
  `reachablePeers > verifiedPeers`.
- Render tests assert the canonical labels and prohibit ambiguous legacy labels.

## Result

- **Behavior change** — the Peers tab now reports exact evidence categories and checked trends
  rather than conflating exhausted candidates with retained unavailable peers.
- **Re-sync required** — no chain re-sync. Recreate only the network primary and API secondary for
  the serialized evidence schema, then crawl again.
- **What to do next** — compare the TUI counts with `/network/summary` after the first rebuilt
  crawl and inspect any trend invariant error rather than suppressing it.
