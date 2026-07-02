use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;

use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
use ckbadger_store::{CkbadgerStore, HistoryPoint, LatestStatus, NodeRecord};

use crate::geoip::GeoIp;
use crate::prober::Prober;

/// Top-N (label, count), sorted by count desc then label asc for determinism.
pub fn top_n_histogram<'a>(labels: impl Iterator<Item = &'a str>, n: usize) -> Vec<(String, u64)> {
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for l in labels {
        *counts.entry(l).or_insert(0) += 1;
    }
    let mut v: Vec<(String, u64)> = counts
        .into_iter()
        .map(|(k, c)| (k.to_string(), c))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// Map a node's discovered addresses to peer_ids, keeping ONLY those that
/// resolve (i.e. were reachable this round). Honest reachable×reachable edges.
pub fn resolve_known_peers(
    discovered: &[String],
    addr_to_peer: &HashMap<String, Vec<u8>>,
) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = discovered
        .iter()
        .filter_map(|a| addr_to_peer.get(a).cloned())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Extract a literal IP from a multiaddr for GeoIP lookup. `None` for DNS addrs.
pub fn addr_ip(addr: &str) -> Option<IpAddr> {
    let parts: Vec<&str> = addr.split('/').collect();
    for w in parts.windows(2) {
        if w[0] == "ip4" || w[0] == "ip6" {
            if let Ok(ip) = w[1].parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }
    None
}

/// Per-round tunables. Time-window sizes are in seconds; `now`/`round_id` are
/// injected by the caller so a round produces fully deterministic timestamps.
pub struct RoundConfig {
    /// Drop node records not seen within this many seconds.
    pub node_ttl_secs: u64,
    /// Keep hourly history points for this many days.
    pub hourly_retention_days: u64,
    /// Top-N width for version/country share histograms.
    pub top_n: usize,
    /// Optional hard cap on addresses dialed per round (deterministic partial rounds/tests).
    pub max_addrs: Option<usize>,
    /// Optional cap on the total distinct queued frontier set. Bounds memory
    /// against a peer flooding `discovered_addrs`; discoveries past the cap are
    /// dropped and the round is marked `frontier_drained = false`. Independent
    /// of `max_addrs` (which caps dials, not the queued set).
    pub max_frontier: Option<usize>,
    /// Wall-clock budget for one round's BFS; on elapse the round stops and is
    /// marked `frontier_drained=false`. `None` = no time bound (tests).
    pub round_budget: Option<std::time::Duration>,
}

impl RoundConfig {
    pub fn test_defaults() -> Self {
        Self {
            node_ttl_secs: 2_592_000,
            hourly_retention_days: 30,
            top_n: 20,
            max_addrs: None,
            max_frontier: None,
            round_budget: None,
        }
    }
}

pub struct RoundReport {
    pub status: LatestStatus,
}

/// Run one crawl round. `now` is injected unix-seconds (deterministic timestamps).
///
/// Store errors fail fast via `?`; an individual probe returning
/// `Ok(None)` (unreachable/refused/timeout) is a recorded observation, never an
/// abort. The BFS is intentionally sequential for deterministic ordering — real
/// concurrency is introduced later in the service loop without changing the
/// observable result.
pub async fn run_round(
    store: &CkbadgerStore,
    prober: &dyn Prober,
    geoip: &dyn GeoIp,
    cfg: &RoundConfig,
    now: u64,
    round_id: u64,
) -> anyhow::Result<RoundReport> {
    // 1. Seed frontier: bootnodes ∪ known nodes' own_addrs. Build the addr->peer
    //    index from prior state so this round's discovered edges can resolve to
    //    peers we already know without re-dialing them.
    let existing = store.scan_nodes()?;
    let mut addr_to_peer: HashMap<String, Vec<u8>> = HashMap::new();
    let mut frontier: VecDeque<String> = VecDeque::new();
    let mut queued: HashSet<String> = HashSet::new();
    for a in prober.bootnodes() {
        if queued.insert(a.clone()) {
            frontier.push_back(a);
        }
    }
    for (peer, rec) in &existing {
        for a in &rec.own_addrs {
            addr_to_peer.insert(a.clone(), peer.clone());
            if queued.insert(a.clone()) {
                frontier.push_back(a.clone());
            }
        }
    }

    // 2-3. BFS dial + expand. Sequential for determinism (see fn doc).
    let mut dialed = 0u64;
    let mut reachable_outcomes: HashMap<Vec<u8>, crate::prober::ProbeOutcome> = HashMap::new();
    let mut unreachable = 0u64;
    let mut frontier_drained = true;
    let deadline = cfg.round_budget.map(|b| std::time::Instant::now() + b);
    while let Some(addr) = frontier.pop_front() {
        if let Some(cap) = cfg.max_addrs {
            if dialed as usize >= cap {
                frontier_drained = false;
                break;
            }
        }
        // Wall-clock budget: a round that outruns its time bound stops here and
        // is marked partial (mirrors the `max_addrs`/`max_frontier` pattern).
        if let Some(dl) = deadline {
            if std::time::Instant::now() >= dl {
                frontier_drained = false;
                break;
            }
        }
        dialed += 1;
        match prober.probe(&addr).await? {
            Some(o) => {
                addr_to_peer.insert(addr.clone(), o.peer_id.clone());
                for a in &o.own_addrs {
                    addr_to_peer.insert(a.clone(), o.peer_id.clone());
                }
                for a in &o.discovered_addrs {
                    // Already queued this round: not a new frontier entry.
                    if queued.contains(a) {
                        continue;
                    }
                    // Change 1 — frontier bound: defend against a peer flooding
                    // `discovered_addrs` into unbounded memory. Caps the total
                    // distinct queued set; independent of the `max_addrs` dial cap.
                    if let Some(cap) = cfg.max_frontier {
                        if queued.len() >= cap {
                            frontier_drained = false;
                            continue;
                        }
                    }
                    queued.insert(a.clone());
                    frontier.push_back(a.clone());
                }
                reachable_outcomes.insert(o.peer_id.clone(), o);
            }
            None => unreachable += 1,
        }
    }

    // 4-5. Resolve honest reachable×reachable edges + upsert node records.
    let mut new_nodes = 0u64;
    for (peer, o) in &reachable_outcomes {
        let known_peers = resolve_known_peers(&o.discovered_addrs, &addr_to_peer);
        let prior = store.get_node(peer)?;
        let first_seen = prior.as_ref().map(|p| p.first_seen).unwrap_or(now);
        if prior.is_none() {
            new_nodes += 1;
        }
        let mut own_addrs = prior.map(|p| p.own_addrs).unwrap_or_default();
        for a in &o.own_addrs {
            if !own_addrs.contains(a) {
                own_addrs.push(a.clone());
            }
        }
        let (geo, asn) = o
            .own_addrs
            .iter()
            .filter_map(|a| addr_ip(a))
            .map(|ip| geoip.lookup(ip))
            .next()
            .unwrap_or((None, None));
        store.put_node(
            peer,
            &NodeRecord {
                own_addrs,
                client_version: o.client_version.clone(),
                flags: o.flags,
                protocols: o.protocols.clone(),
                first_seen,
                last_seen: now,
                last_reachable_at: now,
                reachable: true,
                geo,
                asn,
                last_rtt_ms: o.rtt_ms,
                known_peers,
            },
        )?;
    }

    // 4. Prune stale nodes + old hourly history BEFORE aggregating (Change 3),
    //    so this round's published stats reflect the post-prune node set.
    //    `now.saturating_sub(..)` is a time-window floor on injected
    //    observational time, NOT a masking guard on a correctness-critical
    //    value: when `now < ttl` the prune horizon is legitimately 0 (nothing is
    //    old enough to prune, since no `last_seen < 0`).
    store.prune_nodes_older_than(now.saturating_sub(cfg.node_ttl_secs))?;
    let hourly_cutoff = bucket_of(
        now.saturating_sub(cfg.hourly_retention_days * 86_400),
        Granularity::Hour,
    );
    for m in [
        Metric::TotalNodes,
        Metric::ReachableNodes,
        Metric::VersionShare,
        Metric::CountryShare,
    ] {
        store.prune_history_before(m, Granularity::Hour, hourly_cutoff)?;
    }

    // 5. Reachable downgrade (Change 2): a node that survived the prune but was
    //    NOT probed-reachable this round must stop advertising `reachable=true`.
    //    Rewrite ONLY the `reachable` flag; last_seen / last_reachable_at /
    //    first_seen / own_addrs / known_peers / geo / asn are left untouched so
    //    TTL pruning (via the preserved last_seen) still governs eventual removal.
    //    The reachable-this-round set is exactly the peers we persisted above.
    let reachable_this_round: HashSet<Vec<u8>> = reachable_outcomes.keys().cloned().collect();
    for (peer, rec) in store.scan_nodes()? {
        if rec.reachable && !reachable_this_round.contains(&peer) {
            let mut downgraded = rec;
            downgraded.reachable = false;
            store.put_node(&peer, &downgraded)?;
        }
    }

    // 6. Aggregate from the fresh, post-prune, post-downgrade node set + write
    //    the status singleton and Hour/Day history points.
    let nodes = store.scan_nodes()?;
    let total_known = nodes.len() as u64;
    let reachable = nodes.iter().filter(|(_, r)| r.reachable).count() as u64;
    let versions = top_n_histogram(
        nodes.iter().map(|(_, r)| r.client_version.as_str()),
        cfg.top_n,
    );
    let countries = top_n_histogram(
        nodes
            .iter()
            .filter_map(|(_, r)| r.geo.as_ref().map(|g| g.country.as_str())),
        cfg.top_n,
    );
    let status = LatestStatus {
        round_id,
        started: now,
        finished: now,
        dialed,
        reachable,
        unreachable,
        foreign_dropped: 0,
        new_nodes,
        total_known,
        frontier_drained,
    };
    store.put_network_status(&status)?;
    for gran in [Granularity::Hour, Granularity::Day] {
        let b = bucket_of(now, gran);
        store.put_history_point(
            Metric::TotalNodes,
            gran,
            b,
            &HistoryPoint {
                scalar: total_known,
                buckets: vec![],
            },
        )?;
        store.put_history_point(
            Metric::ReachableNodes,
            gran,
            b,
            &HistoryPoint {
                scalar: reachable,
                buckets: vec![],
            },
        )?;
        store.put_history_point(
            Metric::VersionShare,
            gran,
            b,
            &HistoryPoint {
                scalar: 0,
                buckets: versions.clone(),
            },
        )?;
        store.put_history_point(
            Metric::CountryShare,
            gran,
            b,
            &HistoryPoint {
                scalar: 0,
                buckets: countries.clone(),
            },
        )?;
    }

    Ok(RoundReport { status })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use ckbadger_store::CkbadgerStore;

    use crate::geoip::NoGeo;
    use crate::mock_prober::MockProber;
    use crate::prober::ProbeOutcome;

    #[test]
    fn histogram_top_n_desc_ties_by_label() {
        let labels = vec!["a", "b", "a", "c", "b", "a"];
        let h = top_n_histogram(labels.into_iter(), 2);
        assert_eq!(h, vec![("a".to_string(), 3), ("b".to_string(), 2)]);
    }

    #[test]
    fn resolve_edges_only_to_reachable_resolved_peers() {
        let mut idx = HashMap::new();
        idx.insert("addrB".to_string(), vec![b'B']);
        // "addrX" is unresolved (unreachable) -> excluded.
        let edges = resolve_known_peers(&["addrB".into(), "addrX".into()], &idx);
        assert_eq!(edges, vec![vec![b'B']]);
    }

    #[test]
    fn addr_ip_extracts_v4() {
        assert_eq!(
            addr_ip("/ip4/1.2.3.4/tcp/8115").unwrap().to_string(),
            "1.2.3.4"
        );
        assert!(addr_ip("/dns4/example.com/tcp/8115").is_none());
    }

    fn oc(peer: &[u8], own: &str, disc: &[&str]) -> ProbeOutcome {
        ProbeOutcome {
            peer_id: peer.to_vec(),
            client_version: "0.119.0".into(),
            flags: 0,
            protocols: vec![],
            own_addrs: vec![own.to_string()],
            rtt_ms: Some(5),
            discovered_addrs: disc.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn round_bfs_discovers_all_reachable_and_writes_stats() {
        // a -> knows b ; b -> knows a and c(unreachable). Bootnode: a only.
        let mut g = std::collections::HashMap::new();
        g.insert("addrA".to_string(), oc(b"A", "addrA", &["addrB"]));
        g.insert(
            "addrB".to_string(),
            oc(b"B", "addrB", &["addrA", "addrC_unreachable"]),
        );
        let prober = MockProber::new(vec!["addrA".into()], g);

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let cfg = RoundConfig::test_defaults();
        let report = run_round(&store, &prober, &NoGeo, &cfg, 10_000, 1)
            .await
            .unwrap();

        // Both reachable nodes discovered via BFS from a single bootnode.
        assert_eq!(store.scan_nodes().unwrap().len(), 2);
        assert_eq!(report.status.reachable, 2);
        assert!(report.status.frontier_drained);
        // Edge honesty: B's known_peers includes A (reachable) but NOT the unreachable addr.
        let b = store.get_node(b"B").unwrap().unwrap();
        assert_eq!(b.known_peers, vec![b"A".to_vec()]);
        // Status singleton + a history point were written.
        assert!(store.get_network_status().unwrap().is_some());
        use ckbadger_store::network_keys::{Granularity, Metric};
        assert_eq!(
            store
                .scan_history(Metric::TotalNodes, Granularity::Hour, 0, u64::MAX)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn partial_round_marks_frontier_not_drained() {
        let mut g = std::collections::HashMap::new();
        g.insert("addrA".to_string(), oc(b"A", "addrA", &["addrB"]));
        g.insert("addrB".to_string(), oc(b"B", "addrB", &[]));
        let prober = MockProber::new(vec!["addrA".into()], g);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let mut cfg = RoundConfig::test_defaults();
        cfg.max_addrs = Some(1); // stop after one dial
        let report = run_round(&store, &prober, &NoGeo, &cfg, 10_000, 1)
            .await
            .unwrap();
        assert!(!report.status.frontier_drained);
    }

    #[tokio::test]
    async fn round_budget_zero_marks_partial_immediately() {
        // A ZERO wall-clock budget is always already elapsed at the top of the
        // BFS loop, so the round stops before dialing and is marked partial.
        // Deterministic (no sleeps): `Instant::now() >= now + ZERO` always holds.
        let mut g = std::collections::HashMap::new();
        g.insert("addrA".to_string(), oc(b"A", "addrA", &["addrB"]));
        let prober = MockProber::new(vec!["addrA".into()], g);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let cfg = RoundConfig {
            round_budget: Some(std::time::Duration::ZERO),
            ..RoundConfig::test_defaults()
        };
        let report = run_round(&store, &prober, &NoGeo, &cfg, 10_000, 1)
            .await
            .unwrap();
        assert!(!report.status.frontier_drained);
    }

    #[tokio::test]
    async fn first_seen_preserved_and_stale_pruned() {
        let mut g = std::collections::HashMap::new();
        g.insert("addrA".to_string(), oc(b"A", "addrA", &[]));
        let prober = MockProber::new(vec!["addrA".into()], g);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let cfg = RoundConfig::test_defaults();
        run_round(&store, &prober, &NoGeo, &cfg, 1_000, 1)
            .await
            .unwrap();
        let first = store.get_node(b"A").unwrap().unwrap().first_seen;
        // Second round much later: first_seen preserved, last_seen advanced.
        run_round(&store, &prober, &NoGeo, &cfg, 2_000, 2)
            .await
            .unwrap();
        let a = store.get_node(b"A").unwrap().unwrap();
        assert_eq!(a.first_seen, first);
        assert_eq!(a.last_seen, 2_000);
        // A stale node from long ago gets pruned (ttl default 30d = 2_592_000s).
        store
            .put_node(b"OLD", &{
                let mut r = a.clone();
                r.last_seen = 1;
                r
            })
            .unwrap();
        run_round(&store, &prober, &NoGeo, &cfg, 3_000_000, 3)
            .await
            .unwrap();
        assert!(store.get_node(b"OLD").unwrap().is_none());
    }

    #[tokio::test]
    async fn unreachable_node_downgraded_but_retained() {
        // Round 1: bootnode A is reachable and discovers B; B is reachable too.
        let mut g1 = std::collections::HashMap::new();
        g1.insert("addrA".to_string(), oc(b"A", "addrA", &["addrB"]));
        g1.insert("addrB".to_string(), oc(b"B", "addrB", &[]));
        let prober1 = MockProber::new(vec!["addrA".into()], g1);

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let cfg = RoundConfig::test_defaults();
        run_round(&store, &prober1, &NoGeo, &cfg, 1_000_000, 1)
            .await
            .unwrap();
        // Baseline: both A and B recorded as reachable.
        assert!(store.get_node(b"A").unwrap().unwrap().reachable);
        assert!(store.get_node(b"B").unwrap().unwrap().reachable);

        // Round 2: only A is reachable now (addrB is absent -> probes to None).
        let mut g2 = std::collections::HashMap::new();
        g2.insert("addrA".to_string(), oc(b"A", "addrA", &["addrB"]));
        let prober2 = MockProber::new(vec!["addrA".into()], g2);
        let report = run_round(&store, &prober2, &NoGeo, &cfg, 1_000_100, 2)
            .await
            .unwrap();

        // B is retained (not pruned) but honestly downgraded to unreachable;
        // A stays reachable. Round status counts only A as reachable.
        let b = store.get_node(b"B").unwrap();
        assert!(b.is_some());
        assert!(!b.unwrap().reachable);
        assert!(store.get_node(b"A").unwrap().unwrap().reachable);
        assert_eq!(report.status.reachable, 1);
        assert_eq!(report.status.total_known, 2);
    }

    #[tokio::test]
    async fn frontier_bound_truncates_and_marks_partial() {
        // Bootnode A is reachable and floods 10 discovered addrs (none reachable).
        let flood = [
            "addr1", "addr2", "addr3", "addr4", "addr5", "addr6", "addr7", "addr8", "addr9",
            "addr10",
        ];
        let mut g = std::collections::HashMap::new();
        g.insert("addrA".to_string(), oc(b"A", "addrA", &flood));
        let prober = MockProber::new(vec!["addrA".into()], g);

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let mut cfg = RoundConfig::test_defaults();
        cfg.max_frontier = Some(3);
        cfg.max_addrs = None;
        let report = run_round(&store, &prober, &NoGeo, &cfg, 10_000, 1)
            .await
            .unwrap();
        // The frontier cap dropped discoveries past the bound, so the round is
        // partial (frontier not fully drained).
        assert!(!report.status.frontier_drained);
    }
}
