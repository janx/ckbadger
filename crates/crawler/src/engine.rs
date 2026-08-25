use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
use ckbadger_store::{
    checked_apply_alias_verifications, checked_candidate_alias_map,
    checked_merge_advertisement_evidence, checked_merge_direct_session_evidence,
    checked_merge_local_observer_evidence, checked_prune_candidate_aliases,
    checked_prune_direct_session_evidence, checked_resolve_known_peers, crawl_address_is_fresh,
    ActiveCandidateProbe, ActiveCandidateState, ActiveCrawl, AddressProbeEvidence,
    AddressProbeResult, AdvertisementEvidence, CkbadgerStore, CompletedCandidateEvidence,
    CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate, CrawlProgress,
    DirectSessionObservation, DirectSessionObservationSummary, DirectSessionProtocol,
    DiscoveryEvidence, HistoryPoint, LatestStatus, LocalObserverObservation, LocalObserverProtocol,
    NodeRecord, StagedProbeOutcome,
};
use futures::stream::{FuturesUnordered, StreamExt};

use crate::geoip::GeoIp;
use crate::prober::{ProbeCandidate, Prober};
use crate::rpc_observer::{LocalPeerObserver, LocalPeerSnapshot};

fn checked_histogram_increment(
    count: &mut u64,
    round_id: u64,
    metric: Metric,
    label: &str,
) -> anyhow::Result<()> {
    *count = count.checked_add(1).with_context(|| {
        format!(
            "crawler histogram count overflow: round_id={round_id}, metric={metric:?}, label={label}"
        )
    })?;
    Ok(())
}

/// Top-N (label, count), sorted by count desc then label asc for determinism.
pub fn top_n_histogram<'a>(
    labels: impl Iterator<Item = &'a str>,
    n: usize,
    round_id: u64,
    metric: Metric,
) -> anyhow::Result<Vec<(String, u64)>> {
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for label in labels {
        let count = counts.entry(label).or_insert(0);
        checked_histogram_increment(count, round_id, metric, label)?;
    }
    let mut values: Vec<(String, u64)> = counts
        .into_iter()
        .map(|(label, count)| (label.to_string(), count))
        .collect();
    values.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    values.truncate(n);
    Ok(values)
}

/// Extract a literal IP from a multiaddr for GeoIP lookup. `None` for DNS addrs.
pub fn addr_ip(addr: &str) -> Option<IpAddr> {
    let parts: Vec<&str> = addr.split('/').collect();
    for window in parts.windows(2) {
        if window[0] == "ip4" || window[0] == "ip6" {
            if let Ok(ip) = window[1].parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }
    None
}

pub trait CrawlClock: Send + Sync {
    fn now(&self) -> anyhow::Result<u64>;
}

pub struct SystemCrawlClock;

impl CrawlClock for SystemCrawlClock {
    fn now(&self) -> anyhow::Result<u64> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before UNIX epoch")?
            .as_secs())
    }
}

/// Per-slice tunables. One logical round can span several slices; only a
/// drained logical round is published.
pub struct RoundConfig {
    pub node_ttl_secs: u64,
    pub hourly_retention_days: u64,
    pub top_n: usize,
    pub max_dial_concurrency: usize,
    /// Optional deterministic admission cap used by tests.
    pub max_addrs: Option<usize>,
    /// Maximum distinct durable candidate addresses. Overflow is fatal rather
    /// than silently truncating coverage.
    pub max_frontier: Option<usize>,
    /// Admission budget for one execution slice. Already admitted probes drain.
    pub slice_budget: Option<Duration>,
}

impl RoundConfig {
    pub fn test_defaults() -> Self {
        Self {
            node_ttl_secs: 2_592_000,
            hourly_retention_days: 30,
            top_n: 20,
            max_dial_concurrency: 4,
            max_addrs: None,
            max_frontier: None,
            slice_budget: None,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.max_dial_concurrency == 0 {
            anyhow::bail!("crawler max_dial_concurrency must be greater than zero");
        }
        if self.max_frontier == Some(0) {
            anyhow::bail!("crawler max_frontier must be greater than zero");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrawlSliceReport {
    Partial(CrawlProgress),
    Completed(Box<LatestStatus>),
}

fn checked_inc(value: &mut u64, field: &str, round_id: u64) -> anyhow::Result<()> {
    *value = value
        .checked_add(1)
        .with_context(|| format!("crawler counter overflow: field={field}, round_id={round_id}"))?;
    Ok(())
}

fn prepare_candidate_for_round(candidate: &mut CrawlCandidate, round_id: u64) {
    if candidate
        .active
        .as_ref()
        .is_none_or(|active| active.round_id != round_id)
    {
        candidate.active = Some(ActiveCandidateProbe {
            round_id,
            ..Default::default()
        });
    }
}

fn checked_candidate_has_fresh_alias(
    candidate: &CrawlCandidate,
    cutoff: Option<u64>,
    round_id: u64,
) -> anyhow::Result<bool> {
    let mut aliases = HashSet::new();
    for address in &candidate.addresses {
        if address.addr.is_empty() {
            anyhow::bail!("cannot prune an empty candidate alias: round_id={round_id}");
        }
        if address.first_advertised_at > address.last_advertised_at {
            anyhow::bail!(
                "candidate alias time regressed while pruning: round_id={}, addr={}, first_advertised_at={}, last_advertised_at={}",
                round_id,
                address.addr,
                address.first_advertised_at,
                address.last_advertised_at
            );
        }
        if !aliases.insert(address.addr.as_str()) {
            anyhow::bail!(
                "candidate contains duplicate alias while pruning: round_id={}, addr={}",
                round_id,
                address.addr
            );
        }
    }
    Ok(candidate
        .addresses
        .iter()
        .any(|address| crawl_address_is_fresh(address, cutoff)))
}

fn stage_advertisement(
    target: &mut CrawlCandidate,
    advertiser_peer_id: &[u8],
    alias: &str,
    observed_at: u64,
    round_id: u64,
) -> anyhow::Result<()> {
    if advertiser_peer_id.is_empty() {
        anyhow::bail!(
            "cannot stage advertisement with empty advertiser: round_id={}, alias={}",
            round_id,
            alias
        );
    }
    if !target.addresses.iter().any(|address| address.addr == alias) {
        anyhow::bail!(
            "cannot stage advertisement for an unretained target alias: round_id={}, advertiser_peer_id=0x{}, alias={}",
            round_id,
            hex::encode(advertiser_peer_id),
            alias
        );
    }
    let active = target
        .active
        .as_mut()
        .filter(|active| active.round_id == round_id)
        .with_context(|| {
            format!(
                "advertised target is missing active round state: round_id={}, advertiser_peer_id=0x{}, alias={}",
                round_id,
                hex::encode(advertiser_peer_id),
                alias
            )
        })?;
    let key = (advertiser_peer_id, alias);
    match active.staged_advertisements.binary_search_by(|evidence| {
        (evidence.advertiser_peer_id.as_slice(), evidence.alias.as_str()).cmp(&key)
    }) {
        Ok(_) => anyhow::bail!(
            "duplicate target advertisement in one source probe: round_id={}, advertiser_peer_id=0x{}, alias={}",
            round_id,
            hex::encode(advertiser_peer_id),
            alias
        ),
        Err(index) => active.staged_advertisements.insert(
            index,
            AdvertisementEvidence {
                advertiser_peer_id: advertiser_peer_id.to_vec(),
                alias: alias.to_string(),
                first_observed_at: observed_at,
                last_observed_at: observed_at,
                first_observed_round: round_id,
                last_observed_round: round_id,
                observation_count: 1,
            },
        ),
    }
    Ok(())
}

fn merge_candidate(
    candidates: &mut BTreeMap<Vec<u8>, CrawlCandidate>,
    candidate: ProbeCandidate,
    advertised_at: u64,
    round_id: u64,
) -> bool {
    let peer_id = candidate.peer_id;
    let record = candidates.entry(peer_id).or_insert_with(|| CrawlCandidate {
        addresses: Vec::new(),
        first_discovered_at: advertised_at,
        last_advertised_at: advertised_at,
        last_scheduled_sequence: 0,
        active: Some(ActiveCandidateProbe {
            round_id,
            ..Default::default()
        }),
        last_completed: None,
        advertisements: Vec::new(),
        staged_direct_sessions: Vec::new(),
        direct_sessions: Vec::new(),
    });
    prepare_candidate_for_round(record, round_id);

    let mut changed = false;
    if advertised_at > record.last_advertised_at {
        record.last_advertised_at = advertised_at;
        changed = true;
    }
    match record
        .addresses
        .iter_mut()
        .find(|existing| existing.addr == candidate.addr)
    {
        Some(existing) => {
            if advertised_at > existing.last_advertised_at {
                existing.last_advertised_at = advertised_at;
                changed = true;
            }
        }
        None => {
            record.addresses.push(CrawlAddress {
                addr: candidate.addr,
                first_advertised_at: advertised_at,
                last_advertised_at: advertised_at,
                last_verified_at: None,
            });
            record.addresses.sort_by(|a, b| a.addr.cmp(&b.addr));
            if let Some(active) = record.active.as_mut() {
                if matches!(
                    active.state,
                    ActiveCandidateState::Exhausted | ActiveCandidateState::ForeignNetwork
                ) {
                    active.state = ActiveCandidateState::RetryAlias;
                }
            }
            changed = true;
        }
    }
    changed
}

fn ingest_addr(
    prober: &dyn Prober,
    candidates: &mut BTreeMap<Vec<u8>, CrawlCandidate>,
    addr: &str,
    peer_hint: Option<&[u8]>,
    advertised_at: u64,
    active: &mut ActiveCrawl,
    dirty: &mut HashSet<Vec<u8>>,
) -> anyhow::Result<Option<ProbeCandidate>> {
    let known_peer = if peer_hint.is_none() {
        let mut matches = candidates.iter().filter_map(|(peer_id, candidate)| {
            candidate
                .addresses
                .iter()
                .any(|known| known.addr == addr)
                .then_some(peer_id.clone())
        });
        let first = matches.next();
        if let Some(second) = matches.next() {
            let first = first.as_ref().context(
                "candidate address duplicate scan produced a second peer without a first peer",
            )?;
            anyhow::bail!(
                "candidate address maps to multiple peers: addr={}, first=0x{}, second=0x{}",
                addr,
                hex::encode(first),
                hex::encode(second)
            );
        }
        first
    } else {
        None
    };
    let normalized = match (peer_hint, known_peer) {
        (None, Some(peer_id)) => Some(ProbeCandidate {
            peer_id,
            addr: addr.to_string(),
        }),
        _ => prober.candidate_from_addr(addr, peer_hint)?,
    };
    let Some(candidate) = normalized else {
        checked_inc(
            &mut active.malformed_addresses,
            "malformed_addresses",
            active.round_id,
        )?;
        return Ok(None);
    };
    if candidate.peer_id.is_empty() {
        anyhow::bail!("prober produced an empty candidate peer id: addr={addr}");
    }
    if merge_candidate(
        candidates,
        candidate.clone(),
        advertised_at,
        active.round_id,
    ) {
        dirty.insert(candidate.peer_id.clone());
    }
    Ok(Some(candidate))
}

fn frontier_address_count(candidates: &BTreeMap<Vec<u8>, CrawlCandidate>) -> anyhow::Result<usize> {
    candidates.values().try_fold(0usize, |total, candidate| {
        total
            .checked_add(candidate.addresses.len())
            .context("crawler candidate address count overflow")
    })
}

fn ensure_frontier_bound(
    store: &CkbadgerStore,
    active: &ActiveCrawl,
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
    max_frontier: Option<usize>,
    checkpoint_address_count: usize,
    source_peer: Option<&[u8]>,
) -> anyhow::Result<()> {
    let Some(limit) = max_frontier else {
        return Ok(());
    };
    let count = frontier_address_count(candidates)?;
    if count <= limit {
        return Ok(());
    }
    let attempted_added = count.checked_sub(checkpoint_address_count).with_context(|| {
        format!(
            "crawler frontier shrank during an insert-only transition: round_id={}, checkpoint_addresses={}, candidate_addresses={}",
            active.round_id, checkpoint_address_count, count
        )
    })?;
    let source = source_peer
        .map(hex::encode)
        .unwrap_or_else(|| "round-seed".to_string());
    let reason = format!(
        "crawler frontier capacity exceeded: round_id={}, limit={}, checkpoint_candidate_addresses={}, attempted_added={}, candidate_addresses_if_accepted={}, source_peer={}",
        active.round_id, limit, checkpoint_address_count, attempted_added, count, source
    );
    // Preserve the last valid candidate/result/counter checkpoint exactly. The
    // over-limit insert batch and source probe transition are rejected
    // together, so raising the limit safely retries the source without
    // double-counting it.
    let mut checkpoint = match store.get_active_crawl()? {
        Some(checkpoint) => {
            if checkpoint.round_id != active.round_id {
                anyhow::bail!(
                    "crawler active round changed during frontier validation: expected={}, actual={}",
                    active.round_id,
                    checkpoint.round_id
                );
            }
            checkpoint
        }
        None => ActiveCrawl {
            round_id: active.round_id,
            started_at: active.started_at,
            last_checkpoint_at: active.last_checkpoint_at,
            ..Default::default()
        },
    };
    checkpoint.blocked_reason = Some(reason.clone());
    store.checkpoint_crawl(&checkpoint, &[])?;
    anyhow::bail!(reason)
}

fn candidate_has_untried_addr(
    candidate: &CrawlCandidate,
    round_id: u64,
    cutoff: Option<u64>,
) -> bool {
    let Some(active) = candidate
        .active
        .as_ref()
        .filter(|active| active.round_id == round_id)
    else {
        return false;
    };
    candidate.addresses.iter().any(|address| {
        crawl_address_is_fresh(address, cutoff)
            && !active
                .observations
                .iter()
                .any(|e| e.address == address.addr)
    })
}

fn select_next_candidate(
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
    round_id: u64,
    cutoff: Option<u64>,
    in_flight: &HashSet<Vec<u8>>,
) -> Option<(Vec<u8>, String)> {
    let mut selected: Option<(u8, u64, Vec<u8>, String)> = None;
    for (peer_id, candidate) in candidates {
        let Some(active) = candidate
            .active
            .as_ref()
            .filter(|active| active.round_id == round_id)
        else {
            continue;
        };
        if in_flight.contains(peer_id) {
            continue;
        }
        let tier = match active.state {
            ActiveCandidateState::Pending => 0,
            ActiveCandidateState::RetryAlias => 1,
            ActiveCandidateState::Succeeded
            | ActiveCandidateState::Exhausted
            | ActiveCandidateState::ForeignNetwork => continue,
        };
        let Some(addr) = candidate
            .addresses
            .iter()
            .filter(|address| {
                crawl_address_is_fresh(address, cutoff)
                    && !active
                        .observations
                        .iter()
                        .any(|evidence| evidence.address == address.addr)
            })
            .map(|address| address.addr.as_str())
            .min()
        else {
            continue;
        };
        let key = (
            tier,
            candidate.last_scheduled_sequence,
            peer_id.clone(),
            addr.to_string(),
        );
        if selected.as_ref().is_none_or(|current| key < *current) {
            selected = Some(key);
        }
    }
    selected.map(|(_, _, peer_id, addr)| (peer_id, addr))
}

fn candidate_is_terminal(candidate: &CrawlCandidate, round_id: u64) -> bool {
    candidate.active.as_ref().is_some_and(|active| {
        active.round_id == round_id
            && matches!(
                active.state,
                ActiveCandidateState::Succeeded
                    | ActiveCandidateState::Exhausted
                    | ActiveCandidateState::ForeignNetwork
            )
    })
}

fn progress_from(
    active: &ActiveCrawl,
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
) -> anyhow::Result<CrawlProgress> {
    let candidate_peers = u64::try_from(
        candidates
            .values()
            .filter(|candidate| {
                candidate
                    .active
                    .as_ref()
                    .is_some_and(|probe| probe.round_id == active.round_id)
            })
            .count(),
    )
    .context("active crawler candidate count exceeds u64")?;
    let completed_peers = u64::try_from(
        candidates
            .values()
            .filter(|candidate| candidate_is_terminal(candidate, active.round_id))
            .count(),
    )
    .context("active crawler completed peer count exceeds u64")?;
    Ok(CrawlProgress {
        round_id: active.round_id,
        started_at: active.started_at,
        last_checkpoint_at: active.last_checkpoint_at,
        candidate_peers,
        completed_peers,
        address_attempts: active
            .address_observations
            .address_attempts(active.round_id)?,
        blocked_reason: active.blocked_reason.clone(),
    })
}

fn changed_candidates(
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
    dirty: &HashSet<Vec<u8>>,
) -> anyhow::Result<Vec<(Vec<u8>, CrawlCandidate)>> {
    dirty
        .iter()
        .map(|peer_id| {
            let candidate = candidates.get(peer_id).with_context(|| {
                format!(
                    "dirty crawl candidate missing from memory: peer_id=0x{}",
                    hex::encode(peer_id)
                )
            })?;
            Ok((peer_id.clone(), candidate.clone()))
        })
        .collect()
}

fn initialize_or_resume(
    store: &CkbadgerStore,
    prober: &dyn Prober,
    clock: &dyn CrawlClock,
    cfg: &RoundConfig,
) -> anyhow::Result<(ActiveCrawl, BTreeMap<Vec<u8>, CrawlCandidate>)> {
    let now = clock.now()?;
    let mut candidates: BTreeMap<Vec<u8>, CrawlCandidate> =
        store.scan_crawl_candidates()?.into_iter().collect();
    let node_rows = store.scan_nodes()?;
    let checkpoint_address_count = frontier_address_count(&candidates)?;
    let mut dirty = HashSet::new();
    let mut active = match store.get_active_crawl()? {
        Some(mut active) => {
            active.blocked_reason = None;
            for (peer_id, candidate) in &candidates {
                if let Some(probe) = candidate.active.as_ref() {
                    if probe.round_id != active.round_id {
                        anyhow::bail!(
                            "candidate active round mismatch while resuming: active_round={}, peer_id=0x{}, candidate_round={}",
                            active.round_id,
                            hex::encode(peer_id),
                            probe.round_id
                        );
                    }
                }
            }
            active
        }
        None => {
            let last_round = store
                .get_network_status()?
                .map(|status| status.round_id)
                .unwrap_or(0);
            let round_id = last_round
                .checked_add(1)
                .context("crawler round id overflow")?;
            let candidate_cutoff = now.checked_sub(cfg.node_ttl_secs);
            for (peer_id, candidate) in &mut candidates {
                if let Some(orphan) = candidate.active.as_ref() {
                    anyhow::bail!(
                        "candidate has active evidence without an active crawl: next_round={}, peer_id=0x{}, candidate_round={}",
                        round_id,
                        hex::encode(peer_id),
                        orphan.round_id
                    );
                }
                let eligible =
                    checked_candidate_has_fresh_alias(candidate, candidate_cutoff, round_id)?;
                if eligible {
                    prepare_candidate_for_round(candidate, round_id);
                    dirty.insert(peer_id.clone());
                }
            }
            ActiveCrawl {
                round_id,
                started_at: now,
                last_checkpoint_at: now,
                alias_freshness_cutoff: candidate_cutoff,
                direct_session_freshness_cutoff: now.checked_sub(cfg.node_ttl_secs),
                ..Default::default()
            }
        }
    };

    for addr in prober.bootnodes() {
        let candidate = ingest_addr(
            prober,
            &mut candidates,
            &addr,
            None,
            now,
            &mut active,
            &mut dirty,
        )?
        .with_context(|| format!("crawler bootnode is malformed or lacks peer id: {addr}"))?;
        dirty.insert(candidate.peer_id);
    }
    for (peer_id, node) in node_rows {
        if active
            .alias_freshness_cutoff
            .is_some_and(|cutoff| node.last_seen < cutoff)
        {
            continue;
        }
        for addr in node.own_addrs {
            if let Some(candidate) = ingest_addr(
                prober,
                &mut candidates,
                &addr,
                Some(&peer_id),
                node.last_seen,
                &mut active,
                &mut dirty,
            )? {
                dirty.insert(candidate.peer_id);
            }
        }
    }

    ensure_frontier_bound(
        store,
        &active,
        &candidates,
        cfg.max_frontier,
        checkpoint_address_count,
        None,
    )?;
    active.last_checkpoint_at = clock.now()?;
    let updates = changed_candidates(&candidates, &dirty)?;
    store.checkpoint_crawl(&active, &updates)?;
    Ok((active, candidates))
}

fn validate_staged_direct_snapshot(
    active: &ActiveCrawl,
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
) -> anyhow::Result<()> {
    if active
        .direct_session_targets
        .windows(2)
        .any(|window| window[0] >= window[1])
        || active.direct_session_targets.iter().any(Vec::is_empty)
    {
        anyhow::bail!(
            "active direct-session targets are empty, duplicate, or not canonically sorted: round_id={}",
            active.round_id
        );
    }

    let staged_targets: Vec<Vec<u8>> = candidates
        .iter()
        .filter_map(|(peer_id, candidate)| {
            (!candidate.staged_direct_sessions.is_empty()).then_some(peer_id.clone())
        })
        .collect();
    if staged_targets != active.direct_session_targets {
        anyhow::bail!(
            "active direct-session target inventory mismatch: round_id={}, marker_targets={}, candidate_targets={}",
            active.round_id,
            active.direct_session_targets.len(),
            staged_targets.len()
        );
    }

    let Some(observer) = active.local_observer_observation.as_ref() else {
        if !staged_targets.is_empty() {
            anyhow::bail!(
                "direct-session rows exist without a local observer snapshot: round_id={}",
                active.round_id
            );
        }
        return Ok(());
    };
    if observer.observed_at < active.started_at || observer.observed_at > active.last_checkpoint_at
    {
        anyhow::bail!(
            "local observer checkpoint clock invariant failed: round_id={}, started_at={}, observed_at={}, checkpoint_at={}",
            active.round_id,
            active.started_at,
            observer.observed_at,
            active.last_checkpoint_at
        );
    }
    checked_merge_local_observer_evidence(None, observer, active.round_id)?;
    for target_peer_id in &staged_targets {
        let candidate = candidates.get(target_peer_id).with_context(|| {
            format!(
                "staged direct-session target disappeared: round_id={}, peer_id=0x{}",
                active.round_id,
                hex::encode(target_peer_id)
            )
        })?;
        if candidate.staged_direct_sessions.iter().any(|observation| {
            observation.observer_peer_id != observer.peer_id
                || observation.observed_at != observer.observed_at
        }) {
            anyhow::bail!(
                "direct-session row disagrees with local observer snapshot: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
                active.round_id,
                hex::encode(target_peer_id),
                hex::encode(&observer.peer_id)
            );
        }
        let mut checked = candidate.direct_sessions.clone();
        checked_merge_direct_session_evidence(
            &mut checked,
            &candidate.staged_direct_sessions,
            active.round_id,
            target_peer_id,
        )?;
    }
    Ok(())
}

async fn observe_local_sessions_once(
    store: &CkbadgerStore,
    observer: Option<&dyn LocalPeerObserver>,
    clock: &dyn CrawlClock,
    active: &mut ActiveCrawl,
    candidates: &mut BTreeMap<Vec<u8>, CrawlCandidate>,
) -> anyhow::Result<()> {
    validate_staged_direct_snapshot(active, candidates)?;
    let Some(observer_client) = observer else {
        return Ok(());
    };
    if active.local_observer_observation.is_some() {
        return Ok(());
    }

    let LocalPeerSnapshot { observer, sessions } =
        observer_client.observe().await.with_context(|| {
            format!(
                "failed to observe configured local CKB peer: round_id={}",
                active.round_id
            )
        })?;
    let observed_at = clock.now()?;
    if observed_at < active.started_at {
        anyhow::bail!(
            "system clock moved backwards during local peer observation: round_id={}, started_at={}, observed_at={}",
            active.round_id,
            active.started_at,
            observed_at
        );
    }
    let observer_peer_id = observer.peer_id.clone();
    active.local_observer_observation = Some(LocalObserverObservation {
        round_id: active.round_id,
        observed_at,
        peer_id: observer.peer_id,
        client_version: observer.client_version,
        active: observer.active,
        addresses: observer.addresses,
        protocols: observer
            .protocols
            .into_iter()
            .map(|protocol| LocalObserverProtocol {
                id: protocol.id,
                name: protocol.name,
                support_versions: protocol.support_versions,
            })
            .collect(),
        connections: observer.connections,
    });

    let mut dirty = HashSet::new();
    for session in sessions {
        if session.peer_id.is_empty() || session.peer_id == observer_peer_id {
            anyhow::bail!(
                "local RPC produced an invalid direct-session target: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
                active.round_id,
                hex::encode(&session.peer_id),
                hex::encode(&observer_peer_id)
            );
        }
        let target_peer_id = session.peer_id;
        let candidate = candidates.entry(target_peer_id.clone()).or_default();
        candidate
            .staged_direct_sessions
            .push(DirectSessionObservation {
                round_id: active.round_id,
                observed_at,
                observer_peer_id: observer_peer_id.clone(),
                initiator: session.initiator,
                client_version: session.client_version,
                session_addresses: session.session_addresses,
                connected_duration_ms: session.connected_duration_ms,
                last_ping_duration_ms: session.last_ping_duration_ms,
                protocols: session
                    .protocols
                    .into_iter()
                    .map(|protocol| DirectSessionProtocol {
                        id: protocol.id,
                        version: protocol.version,
                    })
                    .collect(),
            });
        candidate.staged_direct_sessions.sort_by(|left, right| {
            (&left.observer_peer_id, left.initiator)
                .cmp(&(&right.observer_peer_id, right.initiator))
        });
        dirty.insert(target_peer_id);
    }
    active.direct_session_targets = dirty.iter().cloned().collect();
    active.direct_session_targets.sort();
    active.last_checkpoint_at = clock.now()?;
    if active.last_checkpoint_at < observed_at {
        anyhow::bail!(
            "system clock moved backwards while checkpointing local peer observation: round_id={}, observed_at={}, checkpoint_at={}",
            active.round_id,
            observed_at,
            active.last_checkpoint_at
        );
    }
    validate_staged_direct_snapshot(active, candidates)?;
    let updates = changed_candidates(candidates, &dirty)?;
    store.checkpoint_crawl(active, &updates)
}

fn record_address_observation(
    candidate: &mut CrawlCandidate,
    evidence: AddressProbeEvidence,
) -> anyhow::Result<()> {
    let round_id = evidence.round_id;
    candidate
        .addresses
        .iter()
        .find(|candidate_addr| candidate_addr.addr == evidence.address)
        .with_context(|| {
            format!(
                "scheduled address missing from candidate: round_id={}, addr={}",
                round_id, evidence.address
            )
        })?;
    let active = candidate
        .active
        .as_mut()
        .filter(|active| active.round_id == round_id)
        .with_context(|| {
            format!(
                "candidate missing active probe state: round_id={}, addr={}",
                round_id, evidence.address
            )
        })?;
    if active
        .observations
        .iter()
        .any(|prior| prior.address == evidence.address)
    {
        anyhow::bail!(
            "candidate address attempted twice in one round: round_id={}, addr={}",
            round_id,
            evidence.address
        );
    }
    active.observations.push(evidence);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_probe_result(
    store: &CkbadgerStore,
    prober: &dyn Prober,
    clock: &dyn CrawlClock,
    cfg: &RoundConfig,
    active: &mut ActiveCrawl,
    candidates: &mut BTreeMap<Vec<u8>, CrawlCandidate>,
    peer_id: Vec<u8>,
    addr: String,
    result: crate::prober::ProbeResult,
) -> anyhow::Result<()> {
    let checkpoint_address_count = frontier_address_count(candidates)?;
    let observed_at = clock.now()?;
    let round_id = active.round_id;
    let candidate = candidates.get_mut(&peer_id).with_context(|| {
        format!(
            "completed probe has no candidate: round_id={}, peer_id=0x{}",
            round_id,
            hex::encode(&peer_id)
        )
    })?;
    let observation = result.observation;
    record_address_observation(
        candidate,
        AddressProbeEvidence {
            address: addr.clone(),
            round_id,
            observed_at,
            elapsed_ms: result.elapsed_ms,
            result: observation,
        },
    )?;
    active
        .address_observations
        .checked_record(observation, round_id)?;

    let mut dirty = HashSet::from([peer_id.clone()]);
    match observation {
        AddressProbeResult::SameNetworkIdentified => {
            let mut outcome = result.outcome.with_context(|| {
                format!(
                    "reachable observation missing outcome: round_id={}, peer_id=0x{}",
                    round_id,
                    hex::encode(&peer_id)
                )
            })?;
            if outcome.peer_id != peer_id {
                anyhow::bail!(
                    "probe peer mismatch: round_id={}, expected=0x{}, actual=0x{}",
                    round_id,
                    hex::encode(&peer_id),
                    hex::encode(&outcome.peer_id)
                );
            }

            let own_addrs = outcome.own_addrs.clone();
            for own_addr in own_addrs {
                if let Some(bound) = ingest_addr(
                    prober,
                    candidates,
                    &own_addr,
                    Some(&peer_id),
                    observed_at,
                    active,
                    &mut dirty,
                )? {
                    dirty.insert(bound.peer_id);
                }
            }
            let mut normalized_advertisements = Vec::new();
            for discovered in &outcome.discovered_addrs {
                if let Some(normalized) = ingest_addr(
                    prober,
                    candidates,
                    discovered,
                    None,
                    observed_at,
                    active,
                    &mut dirty,
                )? {
                    normalized_advertisements.push((normalized.peer_id.clone(), normalized.addr));
                    dirty.insert(normalized.peer_id);
                } else {
                    outcome.discovery.rejected_advertised_addresses = outcome
                        .discovery
                        .rejected_advertised_addresses
                        .checked_add(1)
                        .with_context(|| {
                            format!(
                                "Discovery rejected-address counter overflow: round_id={}, peer_id=0x{}",
                                round_id,
                                hex::encode(&peer_id)
                            )
                        })?;
                }
            }
            normalized_advertisements.sort();
            normalized_advertisements.dedup();
            for (target_peer_id, alias) in &normalized_advertisements {
                let target = candidates.get_mut(target_peer_id).with_context(|| {
                    format!(
                        "normalized advertised target disappeared: round_id={}, advertiser_peer_id=0x{}, target_peer_id=0x{}, alias={}",
                        round_id,
                        hex::encode(&peer_id),
                        hex::encode(target_peer_id),
                        alias
                    )
                })?;
                stage_advertisement(target, &peer_id, alias, observed_at, round_id)?;
                dirty.insert(target_peer_id.clone());
            }
            let normalized_discovered: Vec<String> = normalized_advertisements
                .into_iter()
                .map(|(_, alias)| alias)
                .collect();
            if outcome.discovery.normalized_advertised_addresses != 0 {
                anyhow::bail!(
                    "prober populated crawler-owned normalized Discovery counter: round_id={}, peer_id=0x{}, value={}",
                    round_id,
                    hex::encode(&peer_id),
                    outcome.discovery.normalized_advertised_addresses
                );
            }
            outcome.discovery.normalized_advertised_addresses = u64::try_from(
                normalized_discovered.len(),
            )
            .context("normalized Discovery address count exceeds u64 during crawler commit")?;
            outcome.discovered_addrs = normalized_discovered;

            let candidate = candidates.get_mut(&peer_id).with_context(|| {
                format!(
                    "successful candidate disappeared during ingestion: peer_id=0x{}",
                    hex::encode(&peer_id)
                )
            })?;
            let active_probe = candidate
                .active
                .as_mut()
                .filter(|active_probe| active_probe.round_id == round_id)
                .context("successful candidate lost active round state")?;
            active_probe.state = ActiveCandidateState::Succeeded;
            active_probe.staged_success = Some(StagedProbeOutcome {
                observed_at,
                client_version: outcome.client_version,
                flags: outcome.flags,
                protocols: outcome.protocols,
                own_addrs: outcome.own_addrs,
                rtt_ms: outcome.rtt_ms,
                discovered_addrs: outcome.discovered_addrs,
                discovery: outcome.discovery,
            });
        }
        AddressProbeResult::ForeignNetwork => {
            if result.outcome.is_some() {
                anyhow::bail!("foreign-network observation unexpectedly carried an outcome");
            }
            let candidate = candidates.get_mut(&peer_id).with_context(|| {
                format!(
                    "foreign candidate disappeared: round_id={}, peer_id=0x{}",
                    round_id,
                    hex::encode(&peer_id)
                )
            })?;
            let has_untried =
                candidate_has_untried_addr(candidate, round_id, active.alias_freshness_cutoff);
            let active_probe = candidate
                .active
                .as_mut()
                .filter(|probe| probe.round_id == round_id)
                .context("foreign candidate lost active round state")?;
            active_probe.staged_success = None;
            active_probe.state = if has_untried {
                ActiveCandidateState::RetryAlias
            } else {
                ActiveCandidateState::ForeignNetwork
            };
        }
        failure => {
            if result.outcome.is_some() {
                anyhow::bail!(
                    "failed probe observation unexpectedly carried an outcome: {:?}",
                    failure
                );
            }
            let candidate = candidates.get_mut(&peer_id).with_context(|| {
                format!(
                    "failed candidate disappeared: round_id={}, peer_id=0x{}",
                    round_id,
                    hex::encode(&peer_id)
                )
            })?;
            let has_untried =
                candidate_has_untried_addr(candidate, round_id, active.alias_freshness_cutoff);
            let foreign_observed = candidate
                .active
                .as_ref()
                .filter(|probe| probe.round_id == round_id)
                .context("failed candidate lost active round state")?
                .observations
                .iter()
                .any(|evidence| evidence.result == AddressProbeResult::ForeignNetwork);
            let active_probe = candidate
                .active
                .as_mut()
                .filter(|probe| probe.round_id == round_id)
                .context("failed candidate lost mutable active round state")?;
            active_probe.staged_success = None;
            active_probe.state = if has_untried {
                ActiveCandidateState::RetryAlias
            } else if foreign_observed {
                ActiveCandidateState::ForeignNetwork
            } else {
                ActiveCandidateState::Exhausted
            };
        }
    }

    ensure_frontier_bound(
        store,
        active,
        candidates,
        cfg.max_frontier,
        checkpoint_address_count,
        Some(&peer_id),
    )?;
    active.last_checkpoint_at = clock.now()?;
    let updates = changed_candidates(candidates, &dirty)?;
    store.checkpoint_crawl(active, &updates)
}

fn history_deletes(
    store: &CkbadgerStore,
    finished: u64,
    retention_days: u64,
) -> anyhow::Result<Vec<(Metric, Granularity, u64)>> {
    let retention_secs = retention_days
        .checked_mul(86_400)
        .context("crawler hourly history retention overflow")?;
    let Some(cutoff_secs) = finished.checked_sub(retention_secs) else {
        return Ok(Vec::new());
    };
    let cutoff = bucket_of(cutoff_secs, Granularity::Hour);
    let mut deletes = Vec::new();
    for metric in [
        Metric::VerifiedPeers,
        Metric::ReachablePeers,
        Metric::VersionShare,
        Metric::CountryShare,
    ] {
        for (bucket, _) in store.scan_history(metric, Granularity::Hour, 0, u64::MAX)? {
            if bucket < cutoff {
                deletes.push((metric, Granularity::Hour, bucket));
            }
        }
    }
    Ok(deletes)
}

fn publish_completed_round(
    store: &CkbadgerStore,
    geoip: &dyn GeoIp,
    clock: &dyn CrawlClock,
    cfg: &RoundConfig,
    active: &ActiveCrawl,
    mut candidates: BTreeMap<Vec<u8>, CrawlCandidate>,
) -> anyhow::Result<LatestStatus> {
    for (peer_id, candidate) in &candidates {
        match candidate.active.as_ref() {
            Some(probe) if probe.round_id != active.round_id => {
                anyhow::bail!(
                    "cannot publish candidate from another active round: round_id={}, peer_id=0x{}, candidate_round={}",
                    active.round_id,
                    hex::encode(peer_id),
                    probe.round_id
                );
            }
            Some(_) if !candidate_is_terminal(candidate, active.round_id) => {
                let state = candidate.active.as_ref().map(|probe| probe.state);
                anyhow::bail!(
                    "cannot publish undrained crawl round: round_id={}, peer_id=0x{}, result={:?}",
                    active.round_id,
                    hex::encode(peer_id),
                    state
                );
            }
            Some(_) => {}
            None => {
                if let Some(prior) = candidate.last_completed.as_ref() {
                    if prior.round_id >= active.round_id {
                        anyhow::bail!(
                            "inactive candidate completed round is not prior to publish: round_id={}, peer_id=0x{}, candidate_round={}",
                            active.round_id,
                            hex::encode(peer_id),
                            prior.round_id
                        );
                    }
                } else if candidate.staged_direct_sessions.is_empty()
                    && candidate.direct_sessions.is_empty()
                {
                    anyhow::bail!(
                        "inactive candidate lacks dial or direct-session evidence: round_id={}, peer_id=0x{}",
                        active.round_id,
                        hex::encode(peer_id)
                    );
                }
            }
        }
    }

    let finished = clock.now()?;
    if finished < active.started_at {
        anyhow::bail!(
            "system clock moved backwards during crawl: round_id={}, started_at={}, finished_at={}",
            active.round_id,
            active.started_at,
            finished
        );
    }

    // Verification freshness and TTL deletion become visible only together
    // with the completed node/status snapshot. Partial checkpoints retain the
    // last completed aliases and advertisement history unchanged.
    for (peer_id, candidate) in &mut candidates {
        if let Some(active_probe) = candidate.active.as_ref() {
            checked_apply_alias_verifications(
                &mut candidate.addresses,
                &active_probe.observations,
                active.round_id,
                peer_id,
            )?;
        }
        checked_prune_candidate_aliases(
            candidate,
            active.alias_freshness_cutoff,
            active.round_id,
            peer_id,
        )?;
    }

    let existing_rows = store.scan_nodes()?;
    let existing: BTreeMap<Vec<u8>, NodeRecord> = existing_rows.iter().cloned().collect();
    let mut next_nodes = existing.clone();
    let addr_to_peer = checked_candidate_alias_map(&candidates, active.round_id)?;

    let mut new_verified_peers = 0u64;
    for (peer_id, candidate) in &candidates {
        let Some(active_probe) = candidate.active.as_ref() else {
            continue;
        };
        if active_probe.state != ActiveCandidateState::Succeeded {
            continue;
        }
        let staged = active_probe.staged_success.as_ref().with_context(|| {
            format!(
                "successful candidate missing staged outcome: round_id={}, peer_id=0x{}",
                active.round_id,
                hex::encode(peer_id)
            )
        })?;
        let prior = existing.get(peer_id);
        if let Some(prior) = prior {
            if staged.observed_at < prior.last_seen {
                anyhow::bail!(
                    "crawler observation time regressed: round_id={}, peer_id=0x{}, prior_last_seen={}, observed_at={}",
                    active.round_id,
                    hex::encode(peer_id),
                    prior.last_seen,
                    staged.observed_at
                );
            }
        }
        if prior.is_none() {
            checked_inc(
                &mut new_verified_peers,
                "new_verified_peers",
                active.round_id,
            )?;
        }
        let first_seen = prior
            .map(|record| record.first_seen)
            .unwrap_or(staged.observed_at);
        let mut own_addrs = prior
            .map(|record| record.own_addrs.clone())
            .unwrap_or_default();
        own_addrs.extend(staged.own_addrs.iter().cloned());
        own_addrs.sort();
        own_addrs.dedup();

        let lookup = staged
            .own_addrs
            .iter()
            .map(String::as_str)
            .chain(
                candidate
                    .addresses
                    .iter()
                    .map(|address| address.addr.as_str()),
            )
            .find_map(addr_ip)
            .map(|ip| geoip.lookup(ip))
            .unwrap_or((None, None));
        let geo = lookup.0.filter(|geo| !geo.country.is_empty());
        next_nodes.insert(
            peer_id.clone(),
            NodeRecord {
                own_addrs,
                client_version: staged.client_version.clone(),
                flags: staged.flags,
                protocols: staged.protocols.clone(),
                first_seen,
                last_seen: staged.observed_at,
                last_reachable_at: staged.observed_at,
                reachable: true,
                geo,
                asn: lookup.1,
                last_rtt_ms: staged.rtt_ms,
                discovery: staged.discovery.clone(),
                known_peers: checked_resolve_known_peers(
                    &staged.discovered_addrs,
                    &addr_to_peer,
                    active.round_id,
                    peer_id,
                )?,
            },
        );
    }

    let successful: HashSet<Vec<u8>> = candidates
        .iter()
        .filter(|(_, candidate)| {
            candidate.active.as_ref().is_some_and(|probe| {
                probe.round_id == active.round_id && probe.state == ActiveCandidateState::Succeeded
            })
        })
        .map(|(peer_id, _)| peer_id.clone())
        .collect();
    for (peer_id, record) in &mut next_nodes {
        if !successful.contains(peer_id) {
            record.reachable = false;
        }
    }

    if let Some(cutoff) = finished.checked_sub(cfg.node_ttl_secs) {
        next_nodes.retain(|_, record| record.last_seen >= cutoff);
    }
    let node_deletes: Vec<Vec<u8>> = existing
        .keys()
        .filter(|peer_id| !next_nodes.contains_key(*peer_id))
        .cloned()
        .collect();
    let node_puts: Vec<(Vec<u8>, NodeRecord)> = next_nodes
        .iter()
        .map(|(peer_id, record)| (peer_id.clone(), record.clone()))
        .collect();

    let mut peer_outcomes = CompletedPeerOutcomes::default();
    let mut evidence_histogram = ckbadger_store::AddressObservationHistogram::default();
    let mut discovery = DiscoveryEvidence::default();
    for (peer_id, candidate) in &candidates {
        let Some(probe) = candidate.active.as_ref() else {
            continue;
        };
        let completed_outcome = match probe.state {
            ActiveCandidateState::Succeeded => CompletedCandidateOutcome::SameNetworkIdentified,
            ActiveCandidateState::Exhausted => CompletedCandidateOutcome::Exhausted,
            ActiveCandidateState::ForeignNetwork => CompletedCandidateOutcome::ForeignNetwork,
            ActiveCandidateState::Pending | ActiveCandidateState::RetryAlias => {
                anyhow::bail!(
                    "non-terminal candidate reached completed classification: round_id={}, peer_id=0x{}, state={:?}",
                    active.round_id,
                    hex::encode(peer_id),
                    probe.state
                );
            }
        };
        evidence_histogram.checked_record_candidate(
            &probe.observations,
            &candidate.addresses,
            completed_outcome,
            active.round_id,
            peer_id,
        )?;
        let retained = next_nodes.contains_key(peer_id);
        peer_outcomes.checked_record(completed_outcome, retained, active.round_id, peer_id)?;
        if completed_outcome == CompletedCandidateOutcome::SameNetworkIdentified {
            discovery.checked_add_assign(
                &probe
                    .staged_success
                    .as_ref()
                    .context("successful candidate missing staged Discovery evidence")?
                    .discovery,
                active.round_id,
            )?;
        }
    }
    if evidence_histogram != active.address_observations {
        anyhow::bail!(
            "address observation histogram invariant failed: round_id={}, candidate_evidence={:?}, active={:?}",
            active.round_id,
            evidence_histogram,
            active.address_observations
        );
    }

    let verified_retained_peers =
        u64::try_from(next_nodes.len()).context("retained verified peer count exceeds u64")?;
    if peer_outcomes.verified_retained_peers(active.round_id)? != verified_retained_peers {
        anyhow::bail!(
            "verified retained invariant failed: round_id={}, matrix={}, published={}",
            active.round_id,
            peer_outcomes.verified_retained_peers(active.round_id)?,
            verified_retained_peers
        );
    }
    let reachable_peers = peer_outcomes.reachable_peers();
    let published_reachable = u64::try_from(
        next_nodes
            .values()
            .filter(|record| record.reachable)
            .count(),
    )
    .with_context(|| {
        format!(
            "published reachable peer count exceeds u64: round_id={}",
            active.round_id
        )
    })?;
    if published_reachable != reachable_peers {
        anyhow::bail!(
            "reachable snapshot invariant failed: round_id={}, candidate_successes={}, published_reachable={}",
            active.round_id,
            reachable_peers,
            published_reachable
        );
    }

    let versions = top_n_histogram(
        next_nodes
            .values()
            .map(|record| record.client_version.as_str()),
        cfg.top_n,
        active.round_id,
        Metric::VersionShare,
    )?;
    let countries = top_n_histogram(
        next_nodes
            .values()
            .filter_map(|record| record.geo.as_ref().map(|geo| geo.country.as_str())),
        cfg.top_n,
        active.round_id,
        Metric::CountryShare,
    )?;
    let prior_status = store.get_network_status()?;
    let local_observer = match active.local_observer_observation.as_ref() {
        Some(observation) => {
            if observation.observed_at < active.started_at || observation.observed_at > finished {
                anyhow::bail!(
                    "local observer publication clock invariant failed: round_id={}, started_at={}, observed_at={}, finished_at={}",
                    active.round_id,
                    active.started_at,
                    observation.observed_at,
                    finished
                );
            }
            Some(checked_merge_local_observer_evidence(
                prior_status
                    .as_ref()
                    .and_then(|status| status.local_observer.as_ref()),
                observation,
                active.round_id,
            )?)
        }
        None => prior_status.and_then(|status| status.local_observer),
    };
    let mut direct_session_observations = DirectSessionObservationSummary::default();
    for (target_peer_id, candidate) in &candidates {
        for observation in &candidate.staged_direct_sessions {
            if observation.observed_at < active.started_at || observation.observed_at > finished {
                anyhow::bail!(
                    "direct-session publication clock invariant failed: round_id={}, target_peer_id=0x{}, observed_at={}, started_at={}, finished_at={}",
                    active.round_id,
                    hex::encode(target_peer_id),
                    observation.observed_at,
                    active.started_at,
                    finished
                );
            }
            direct_session_observations.checked_record(
                observation.initiator,
                active.round_id,
                target_peer_id,
            )?;
        }
    }
    let status = LatestStatus {
        round_id: active.round_id,
        started: active.started_at,
        finished,
        peer_outcomes,
        address_observations: active.address_observations.clone(),
        discovery,
        malformed_addresses: active.malformed_addresses,
        new_verified_peers,
        local_observer,
        direct_session_observations,
    };

    let mut history_puts = Vec::new();
    for granularity in [Granularity::Hour, Granularity::Day] {
        let bucket = bucket_of(finished, granularity);
        history_puts.push((
            Metric::VerifiedPeers,
            granularity,
            bucket,
            HistoryPoint {
                scalar: verified_retained_peers,
                buckets: vec![],
            },
        ));
        history_puts.push((
            Metric::ReachablePeers,
            granularity,
            bucket,
            HistoryPoint {
                scalar: reachable_peers,
                buckets: vec![],
            },
        ));
        history_puts.push((
            Metric::VersionShare,
            granularity,
            bucket,
            HistoryPoint {
                scalar: 0,
                buckets: versions.clone(),
            },
        ));
        history_puts.push((
            Metric::CountryShare,
            granularity,
            bucket,
            HistoryPoint {
                scalar: 0,
                buckets: countries.clone(),
            },
        ));
    }
    let history_deletes = history_deletes(store, finished, cfg.hourly_retention_days)?;

    let mut candidate_deletes = Vec::new();
    let mut candidate_puts = Vec::new();
    for (peer_id, candidate) in &mut candidates {
        checked_merge_direct_session_evidence(
            &mut candidate.direct_sessions,
            &candidate.staged_direct_sessions,
            active.round_id,
            peer_id,
        )?;
        candidate.staged_direct_sessions.clear();
        checked_prune_direct_session_evidence(
            &mut candidate.direct_sessions,
            active.direct_session_freshness_cutoff,
            active.round_id,
            peer_id,
        )?;
        let Some(active_probe) = candidate.active.take() else {
            let retain = next_nodes.contains_key(peer_id)
                || !candidate.addresses.is_empty()
                || !candidate.advertisements.is_empty()
                || !candidate.direct_sessions.is_empty();
            if retain {
                candidate_puts.push((peer_id.clone(), candidate.clone()));
            } else {
                candidate_deletes.push((peer_id.clone(), candidate.clone()));
            }
            continue;
        };
        let outcome = match active_probe.state {
            ActiveCandidateState::Succeeded => CompletedCandidateOutcome::SameNetworkIdentified,
            ActiveCandidateState::Exhausted => CompletedCandidateOutcome::Exhausted,
            ActiveCandidateState::ForeignNetwork => CompletedCandidateOutcome::ForeignNetwork,
            state => anyhow::bail!(
                "candidate became non-terminal before commit: round_id={}, peer_id=0x{}, state={:?}",
                active.round_id,
                hex::encode(peer_id),
                state
            ),
        };
        let consecutive_exhausted_rounds = if outcome == CompletedCandidateOutcome::Exhausted {
            candidate
                .last_completed
                .as_ref()
                .filter(|prior| prior.outcome == CompletedCandidateOutcome::Exhausted)
                .map(|prior| prior.consecutive_exhausted_rounds)
                .unwrap_or(0)
                .checked_add(1)
                .with_context(|| {
                    format!(
                        "consecutive exhausted round counter overflow: round_id={}, peer_id=0x{}",
                        active.round_id,
                        hex::encode(peer_id)
                    )
                })?
        } else {
            0
        };
        checked_merge_advertisement_evidence(
            &mut candidate.advertisements,
            &active_probe.staged_advertisements,
            &candidate.addresses,
            active.round_id,
            peer_id,
        )?;
        candidate.last_completed = Some(CompletedCandidateEvidence {
            round_id: active.round_id,
            outcome,
            observations: active_probe.observations,
            consecutive_exhausted_rounds,
        });
        candidate_puts.push((peer_id.clone(), candidate.clone()));
    }

    store.commit_crawl_round(
        active.round_id,
        &node_puts,
        &node_deletes,
        &candidate_puts,
        &candidate_deletes,
        &status,
        &history_puts,
        &history_deletes,
    )?;
    Ok(status)
}

async fn run_crawl_slice_inner(
    store: &CkbadgerStore,
    prober: &dyn Prober,
    observer: Option<&dyn LocalPeerObserver>,
    geoip: &dyn GeoIp,
    clock: &dyn CrawlClock,
    cfg: &RoundConfig,
) -> anyhow::Result<CrawlSliceReport> {
    cfg.validate()?;
    let deadline = cfg
        .slice_budget
        .map(|budget| {
            Instant::now()
                .checked_add(budget)
                .context("crawler slice budget exceeds the monotonic clock range")
        })
        .transpose()?;
    let (mut active, mut candidates) = initialize_or_resume(store, prober, clock, cfg)?;
    observe_local_sessions_once(store, observer, clock, &mut active, &mut candidates).await?;
    let mut admitted = 0usize;
    let mut in_flight_peers = HashSet::new();
    let mut in_flight = FuturesUnordered::new();

    loop {
        while in_flight.len() < cfg.max_dial_concurrency {
            let address_cap_reached = cfg.max_addrs.is_some_and(|cap| admitted >= cap);
            let deadline_reached = deadline.is_some_and(|deadline| Instant::now() >= deadline);
            if address_cap_reached || deadline_reached {
                break;
            }
            let Some((peer_id, addr)) = select_next_candidate(
                &candidates,
                active.round_id,
                active.alias_freshness_cutoff,
                &in_flight_peers,
            ) else {
                break;
            };
            active.next_schedule_sequence = active
                .next_schedule_sequence
                .checked_add(1)
                .context("crawler schedule sequence overflow")?;
            candidates
                .get_mut(&peer_id)
                .with_context(|| {
                    format!(
                        "selected candidate disappeared before scheduling: round_id={}, peer_id=0x{}",
                        active.round_id,
                        hex::encode(&peer_id)
                    )
                })?
                .last_scheduled_sequence = active.next_schedule_sequence;
            in_flight_peers.insert(peer_id.clone());
            admitted += 1;
            in_flight.push(async move {
                let result = prober.probe(&peer_id, &addr).await;
                (peer_id, addr, result)
            });
        }

        let Some((peer_id, addr, result)) = in_flight.next().await else {
            break;
        };
        if !in_flight_peers.remove(&peer_id) {
            anyhow::bail!(
                "completed probe was not in flight: round_id={}, peer_id=0x{}",
                active.round_id,
                hex::encode(&peer_id)
            );
        }
        let result = result.with_context(|| {
            format!(
                "crawler probe failed internally: round_id={}, peer_id=0x{}, addr={}",
                active.round_id,
                hex::encode(&peer_id),
                addr
            )
        })?;
        apply_probe_result(
            store,
            prober,
            clock,
            cfg,
            &mut active,
            &mut candidates,
            peer_id,
            addr,
            result,
        )?;
    }

    if select_next_candidate(
        &candidates,
        active.round_id,
        active.alias_freshness_cutoff,
        &HashSet::new(),
    )
    .is_some()
    {
        active.last_checkpoint_at = clock.now()?;
        store.checkpoint_crawl(&active, &[])?;
        return Ok(CrawlSliceReport::Partial(progress_from(
            &active,
            &candidates,
        )?));
    }

    Ok(CrawlSliceReport::Completed(Box::new(
        publish_completed_round(store, geoip, clock, cfg, &active, candidates)?,
    )))
}

/// Run one bounded execution slice without a configured local RPC observer.
/// This entrypoint remains useful for deterministic engine tests and embedders;
/// the production service uses [`run_crawl_slice_with_observer`].
pub async fn run_crawl_slice(
    store: &CkbadgerStore,
    prober: &dyn Prober,
    geoip: &dyn GeoIp,
    clock: &dyn CrawlClock,
    cfg: &RoundConfig,
) -> anyhow::Result<CrawlSliceReport> {
    run_crawl_slice_inner(store, prober, None, geoip, clock, cfg).await
}

/// Run one bounded execution slice and atomically stage exactly one configured
/// local CKB RPC snapshot per logical round. A resumed slice reuses the durable
/// marker and never samples the same logical round twice.
pub async fn run_crawl_slice_with_observer(
    store: &CkbadgerStore,
    prober: &dyn Prober,
    observer: &dyn LocalPeerObserver,
    geoip: &dyn GeoIp,
    clock: &dyn CrawlClock,
    cfg: &RoundConfig,
) -> anyhow::Result<CrawlSliceReport> {
    run_crawl_slice_inner(store, prober, Some(observer), geoip, clock, cfg).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use crate::geoip::NoGeo;
    use crate::mock_prober::MockProber;
    use crate::prober::{ProbeOutcome, ProbeResult};
    use crate::rpc_observer::{
        DirectSessionSnapshot, LocalObserverSnapshot, LocalPeerSnapshot, LocalProtocolSnapshot,
    };
    use async_trait::async_trait;

    struct TestClock(AtomicU64);

    impl TestClock {
        fn new(now: u64) -> Self {
            Self(AtomicU64::new(now))
        }

        fn set(&self, now: u64) {
            self.0.store(now, Ordering::SeqCst);
        }
    }

    impl CrawlClock for TestClock {
        fn now(&self) -> anyhow::Result<u64> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }

    struct CountingObserver {
        snapshot: LocalPeerSnapshot,
        calls: AtomicUsize,
    }

    impl CountingObserver {
        fn addressless_peer_initiated(target_peer_id: &[u8]) -> Self {
            Self::peer_initiated(target_peer_id, Vec::new())
        }

        fn peer_initiated(target_peer_id: &[u8], session_addresses: Vec<String>) -> Self {
            Self {
                snapshot: LocalPeerSnapshot {
                    observer: LocalObserverSnapshot {
                        peer_id: b"observer".to_vec(),
                        client_version: "ckb-observer".into(),
                        active: true,
                        addresses: vec!["/ip4/127.0.0.1/tcp/8115".into()],
                        protocols: vec![LocalProtocolSnapshot {
                            id: 1,
                            name: "identify".into(),
                            support_versions: vec!["0.0.1".into()],
                        }],
                        connections: 1,
                    },
                    sessions: vec![DirectSessionSnapshot {
                        peer_id: target_peer_id.to_vec(),
                        client_version: "ckb-direct".into(),
                        session_addresses,
                        initiator: ckbadger_store::SessionInitiator::Peer,
                        connected_duration_ms: 10,
                        last_ping_duration_ms: None,
                        protocols: Vec::new(),
                    }],
                },
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LocalPeerObserver for CountingObserver {
        async fn observe(&self) -> anyhow::Result<LocalPeerSnapshot> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.snapshot.clone())
        }
    }

    struct InstrumentedProber {
        bootnodes: Vec<String>,
        peer_by_addr: HashMap<String, Vec<u8>>,
        outcomes: HashMap<String, ProbeOutcome>,
        delay: Duration,
        attempts: Mutex<Vec<String>>,
        in_flight: AtomicUsize,
        peak_in_flight: AtomicUsize,
        peer_in_flight: Mutex<HashMap<Vec<u8>, usize>>,
        peer_peak: Mutex<HashMap<Vec<u8>, usize>>,
    }

    struct ForeignAliasProber {
        second_reachable: bool,
        attempts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Prober for ForeignAliasProber {
        fn candidate_from_addr(
            &self,
            addr: &str,
            peer_hint: Option<&[u8]>,
        ) -> anyhow::Result<Option<ProbeCandidate>> {
            let peer_id = peer_hint.unwrap_or(b"A").to_vec();
            Ok(Some(ProbeCandidate {
                peer_id,
                addr: addr.to_string(),
            }))
        }

        async fn probe(&self, peer_id: &[u8], addr: &str) -> anyhow::Result<ProbeResult> {
            if peer_id != b"A" || !matches!(addr, "a1" | "a2") {
                anyhow::bail!(
                    "unexpected foreign-alias probe: peer_id={:?}, addr={}",
                    peer_id,
                    addr
                );
            }
            self.attempts.lock().unwrap().push(addr.to_string());
            if addr == "a1" || !self.second_reachable {
                ProbeResult::failed(AddressProbeResult::ForeignNetwork, 1)
            } else {
                Ok(ProbeResult::reachable(outcome(b"A", "a2", &[]), 1))
            }
        }

        fn bootnodes(&self) -> Vec<String> {
            vec!["a1".into(), "a2".into()]
        }
    }

    impl InstrumentedProber {
        fn new(
            bootnodes: Vec<String>,
            peer_by_addr: HashMap<String, Vec<u8>>,
            outcomes: HashMap<String, ProbeOutcome>,
        ) -> Self {
            Self {
                bootnodes,
                peer_by_addr,
                outcomes,
                delay: Duration::from_millis(20),
                attempts: Mutex::new(Vec::new()),
                in_flight: AtomicUsize::new(0),
                peak_in_flight: AtomicUsize::new(0),
                peer_in_flight: Mutex::new(HashMap::new()),
                peer_peak: Mutex::new(HashMap::new()),
            }
        }

        fn attempts(&self) -> Vec<String> {
            self.attempts.lock().unwrap().clone()
        }

        fn peak_in_flight(&self) -> usize {
            self.peak_in_flight.load(Ordering::SeqCst)
        }

        fn peer_peak(&self, peer_id: &[u8]) -> usize {
            self.peer_peak
                .lock()
                .unwrap()
                .get(peer_id)
                .copied()
                .unwrap_or(0)
        }

        fn update_peak(target: &AtomicUsize, value: usize) {
            let mut current = target.load(Ordering::SeqCst);
            while value > current {
                match target.compare_exchange(current, value, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
        }
    }

    #[async_trait]
    impl Prober for InstrumentedProber {
        fn candidate_from_addr(
            &self,
            addr: &str,
            peer_hint: Option<&[u8]>,
        ) -> anyhow::Result<Option<ProbeCandidate>> {
            let peer_id = match peer_hint {
                Some(peer_id) => peer_id.to_vec(),
                None => match self.peer_by_addr.get(addr) {
                    Some(peer_id) => peer_id.clone(),
                    None => return Ok(None),
                },
            };
            Ok(Some(ProbeCandidate {
                peer_id,
                addr: addr.to_string(),
            }))
        }

        async fn probe(&self, peer_id: &[u8], addr: &str) -> anyhow::Result<ProbeResult> {
            let expected = self
                .peer_by_addr
                .get(addr)
                .with_context(|| format!("instrumented probe has no peer mapping: addr={addr}"))?;
            if expected != peer_id {
                anyhow::bail!(
                    "instrumented probe peer mismatch: addr={}, expected={:?}, actual={:?}",
                    addr,
                    expected,
                    peer_id
                );
            }
            self.attempts.lock().unwrap().push(addr.to_string());
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            Self::update_peak(&self.peak_in_flight, current);
            {
                let mut in_flight = self.peer_in_flight.lock().unwrap();
                let peer_current = in_flight.entry(peer_id.to_vec()).or_insert(0);
                *peer_current += 1;
                let mut peaks = self.peer_peak.lock().unwrap();
                let peak = peaks.entry(peer_id.to_vec()).or_insert(0);
                *peak = (*peak).max(*peer_current);
            }

            tokio::time::sleep(self.delay).await;

            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            {
                let mut in_flight = self.peer_in_flight.lock().unwrap();
                let peer_current = in_flight.get_mut(peer_id).unwrap();
                *peer_current -= 1;
            }
            match self.outcomes.get(addr).cloned() {
                Some(outcome) => Ok(ProbeResult::reachable(outcome, 1)),
                None => ProbeResult::failed(AddressProbeResult::DialRequestFailed, 1),
            }
        }

        fn bootnodes(&self) -> Vec<String> {
            self.bootnodes.clone()
        }
    }

    fn outcome(peer: &[u8], own: &str, discovered: &[&str]) -> ProbeOutcome {
        ProbeOutcome {
            peer_id: peer.to_vec(),
            client_version: "0.119.0".into(),
            flags: 0,
            protocols: vec![],
            own_addrs: vec![own.to_string()],
            rtt_ms: Some(5),
            discovered_addrs: discovered.iter().map(|addr| addr.to_string()).collect(),
            discovery: DiscoveryEvidence::default(),
        }
    }

    fn completed(report: CrawlSliceReport) -> LatestStatus {
        match report {
            CrawlSliceReport::Completed(status) => *status,
            CrawlSliceReport::Partial(progress) => {
                panic!("expected completed round, got progress: {progress:?}")
            }
        }
    }

    fn reachable(status: &LatestStatus) -> u64 {
        status.peer_outcomes.reachable_peers()
    }

    fn exhausted(status: &LatestStatus) -> u64 {
        status
            .peer_outcomes
            .exhausted_candidates(status.round_id)
            .unwrap()
    }

    fn foreign(status: &LatestStatus) -> u64 {
        status.peer_outcomes.foreign_peers(status.round_id).unwrap()
    }

    fn verified_retained(status: &LatestStatus) -> u64 {
        status
            .peer_outcomes
            .verified_retained_peers(status.round_id)
            .unwrap()
    }

    fn address_attempts(status: &LatestStatus) -> u64 {
        status
            .address_observations
            .address_attempts(status.round_id)
            .unwrap()
    }

    #[test]
    fn histogram_top_n_desc_ties_by_label() {
        let labels = vec!["a", "b", "a", "c", "b", "a"];
        assert_eq!(
            top_n_histogram(labels.into_iter(), 2, 1, Metric::VersionShare).unwrap(),
            vec![("a".to_string(), 3), ("b".to_string(), 2)]
        );
    }

    #[test]
    fn histogram_counter_overflow_fails_with_round_and_metric_context() {
        let mut count = u64::MAX;
        let error =
            checked_histogram_increment(&mut count, 53, Metric::CountryShare, "US").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("round_id=53"));
        assert!(message.contains("metric=CountryShare"));
        assert!(message.contains("label=US"));
    }

    #[test]
    fn resolve_edges_through_the_checked_candidate_alias_index() {
        let candidates = BTreeMap::from([(
            vec![b'B'],
            CrawlCandidate {
                addresses: vec![CrawlAddress {
                    addr: "addrB".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )]);
        let index = checked_candidate_alias_map(&candidates, 1).unwrap();
        assert_eq!(
            checked_resolve_known_peers(&["addrB".into()], &index, 1, b"A").unwrap(),
            vec![vec![b'B']]
        );
        assert!(checked_resolve_known_peers(&["addrX".into()], &index, 1, b"A").is_err());
    }

    #[test]
    fn addr_ip_extracts_v4() {
        assert_eq!(
            addr_ip("/ip4/1.2.3.4/tcp/8115").unwrap().to_string(),
            "1.2.3.4"
        );
        assert!(addr_ip("/dns4/example.com/tcp/8115").is_none());
    }

    #[tokio::test]
    async fn complete_round_discovers_bfs_and_atomically_publishes() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &["addrB"]));
        graph.insert(
            "addrB".to_string(),
            outcome(b"B", "addrB", &["addrA", "addrC"]),
        );
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);

        let status = completed(
            run_crawl_slice(
                &store,
                &prober,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(reachable(&status), 2);
        assert_eq!(exhausted(&status), 1);
        assert_eq!(verified_retained(&status), 2);
        assert!(store.get_active_crawl().unwrap().is_none());
        assert_eq!(store.scan_nodes().unwrap().len(), 2);
        let b = store.get_node(b"B").unwrap().unwrap();
        assert_eq!(b.known_peers, vec![b"A".to_vec(), b"addrC".to_vec()]);
        assert_eq!(
            store
                .scan_history(Metric::VerifiedPeers, Granularity::Hour, 0, u64::MAX)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn addressless_direct_session_is_published_without_dial_or_node_inference() {
        let graph = HashMap::from([("addrA".to_string(), outcome(b"A", "addrA", &[]))]);
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let observer = CountingObserver::addressless_peer_initiated(b"direct-only");
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);

        let status = completed(
            run_crawl_slice_with_observer(
                &store,
                &prober,
                &observer,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );

        assert_eq!(observer.calls(), 1);
        assert_eq!(status.direct_session_observations.peer_initiated, 1);
        assert_eq!(status.direct_session_observations.observer_initiated, 0);
        assert_eq!(
            status
                .peer_outcomes
                .candidate_peers(status.round_id)
                .unwrap(),
            1
        );
        assert_eq!(
            status
                .address_observations
                .address_attempts(status.round_id)
                .unwrap(),
            1
        );
        assert!(store.get_node(b"direct-only").unwrap().is_none());
        let candidate = store.get_crawl_candidate(b"direct-only").unwrap().unwrap();
        assert!(candidate.addresses.is_empty());
        assert!(candidate.active.is_none());
        assert!(candidate.last_completed.is_none());
        assert!(candidate.staged_direct_sessions.is_empty());
        assert_eq!(candidate.direct_sessions.len(), 1);
        assert_eq!(
            candidate.direct_sessions[0].initiator,
            ckbadger_store::SessionInitiator::Peer
        );
        assert!(candidate.direct_sessions[0].session_addresses.is_empty());
    }

    #[tokio::test]
    async fn direct_session_addresses_never_enter_the_dial_frontier() {
        let graph = HashMap::from([("addrA".to_string(), outcome(b"A", "addrA", &[]))]);
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let session_addr = "/ip4/198.51.100.9/tcp/54321";
        let observer =
            CountingObserver::peer_initiated(b"direct-only", vec![session_addr.to_string()]);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);

        completed(
            run_crawl_slice_with_observer(
                &store,
                &prober,
                &observer,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );

        assert_eq!(prober.attempts(), vec!["addrA".to_string()]);
        let candidate = store.get_crawl_candidate(b"direct-only").unwrap().unwrap();
        assert!(candidate.addresses.is_empty());
        assert_eq!(candidate.direct_sessions.len(), 1);
        assert_eq!(
            candidate.direct_sessions[0].session_addresses,
            vec![session_addr.to_string()]
        );
    }

    #[tokio::test]
    async fn resumed_logical_round_reuses_the_checkpointed_rpc_snapshot() {
        let graph = HashMap::from([("addrA".to_string(), outcome(b"A", "addrA", &[]))]);
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let observer = CountingObserver::addressless_peer_initiated(b"direct-only");
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let paused = RoundConfig {
            max_addrs: Some(0),
            ..RoundConfig::test_defaults()
        };

        assert!(matches!(
            run_crawl_slice_with_observer(&store, &prober, &observer, &NoGeo, &clock, &paused,)
                .await
                .unwrap(),
            CrawlSliceReport::Partial(_)
        ));
        assert!(matches!(
            run_crawl_slice_with_observer(&store, &prober, &observer, &NoGeo, &clock, &paused,)
                .await
                .unwrap(),
            CrawlSliceReport::Partial(_)
        ));
        assert_eq!(observer.calls(), 1);

        completed(
            run_crawl_slice_with_observer(
                &store,
                &prober,
                &observer,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(observer.calls(), 1);
    }

    #[tokio::test]
    async fn missing_later_rpc_snapshot_is_not_negative_but_direct_ttl_expires_independently() {
        let graph = HashMap::from([("addrA".to_string(), outcome(b"A", "addrA", &[]))]);
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let first_observer = CountingObserver::addressless_peer_initiated(b"direct-only");
        let mut empty_observer = CountingObserver::addressless_peer_initiated(b"unused");
        empty_observer.snapshot.sessions.clear();
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(1_000);
        let cfg = RoundConfig {
            node_ttl_secs: 100,
            ..RoundConfig::test_defaults()
        };

        completed(
            run_crawl_slice_with_observer(&store, &prober, &first_observer, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        clock.set(1_050);
        let second = completed(
            run_crawl_slice_with_observer(&store, &prober, &empty_observer, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        assert_eq!(second.direct_session_observations.total(2).unwrap(), 0);
        assert_eq!(
            store
                .get_crawl_candidate(b"direct-only")
                .unwrap()
                .unwrap()
                .direct_sessions
                .len(),
            1
        );

        clock.set(1_101);
        completed(
            run_crawl_slice_with_observer(&store, &prober, &empty_observer, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        assert!(store.get_crawl_candidate(b"direct-only").unwrap().is_none());
    }

    #[tokio::test]
    async fn advertisement_evidence_is_staged_then_merged_and_not_erased_by_absence() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &["addrB"]));
        graph.insert("addrB".to_string(), outcome(b"addrB", "addrB", &[]));
        let first_prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let partial_cfg = RoundConfig {
            max_addrs: Some(1),
            ..RoundConfig::test_defaults()
        };

        assert!(matches!(
            run_crawl_slice(&store, &first_prober, &NoGeo, &clock, &partial_cfg)
                .await
                .unwrap(),
            CrawlSliceReport::Partial(_)
        ));
        let staged = store.get_crawl_candidate(b"addrB").unwrap().unwrap();
        assert!(staged.advertisements.is_empty());
        assert_eq!(
            staged.active.as_ref().unwrap().staged_advertisements.len(),
            1
        );

        clock.set(10_001);
        completed(
            run_crawl_slice(&store, &first_prober, &NoGeo, &clock, &partial_cfg)
                .await
                .unwrap(),
        );
        let published = store.get_crawl_candidate(b"addrB").unwrap().unwrap();
        assert_eq!(published.advertisements.len(), 1);
        let evidence = &published.advertisements[0];
        assert_eq!(evidence.advertiser_peer_id, b"A");
        assert_eq!(evidence.alias, "addrB");
        assert_eq!(evidence.first_observed_at, 10_000);
        assert_eq!(evidence.last_observed_at, 10_000);
        assert_eq!(evidence.first_observed_round, 1);
        assert_eq!(evidence.last_observed_round, 1);
        assert_eq!(evidence.observation_count, 1);

        let second_graph = HashMap::from([
            ("addrA".to_string(), outcome(b"A", "addrA", &[])),
            ("addrB".to_string(), outcome(b"addrB", "addrB", &[])),
        ]);
        let second_prober = MockProber::new(vec!["addrA".into()], second_graph);
        clock.set(20_000);
        completed(
            run_crawl_slice(
                &store,
                &second_prober,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            store
                .get_crawl_candidate(b"addrB")
                .unwrap()
                .unwrap()
                .advertisements,
            published.advertisements
        );
    }

    #[test]
    fn completed_alias_pruning_uses_exact_advertisement_or_dial_freshness() {
        let mut candidate = CrawlCandidate {
            addresses: vec![
                CrawlAddress {
                    addr: "stale".into(),
                    first_advertised_at: 1,
                    last_advertised_at: 10,
                    last_verified_at: None,
                },
                CrawlAddress {
                    addr: "fresh".into(),
                    first_advertised_at: 2,
                    last_advertised_at: 20,
                    last_verified_at: Some(90),
                },
            ],
            last_advertised_at: 90,
            advertisements: vec![
                AdvertisementEvidence {
                    advertiser_peer_id: b"source".to_vec(),
                    alias: "fresh".into(),
                    first_observed_at: 2,
                    last_observed_at: 90,
                    first_observed_round: 1,
                    last_observed_round: 2,
                    observation_count: 2,
                },
                AdvertisementEvidence {
                    advertiser_peer_id: b"source".to_vec(),
                    alias: "stale".into(),
                    first_observed_at: 1,
                    last_observed_at: 10,
                    first_observed_round: 1,
                    last_observed_round: 1,
                    observation_count: 1,
                },
            ],
            ..Default::default()
        };

        checked_prune_candidate_aliases(&mut candidate, Some(50), 53, b"target").unwrap();
        assert_eq!(candidate.addresses.len(), 1);
        assert_eq!(candidate.addresses[0].addr, "fresh");
        assert_eq!(candidate.addresses[0].last_advertised_at, 20);
        assert_eq!(candidate.addresses[0].last_verified_at, Some(90));
        assert_eq!(candidate.advertisements.len(), 1);
        assert_eq!(candidate.advertisements[0].alias, "fresh");
        assert_eq!(candidate.last_advertised_at, 90);
    }

    #[tokio::test]
    async fn partial_round_resumes_unvisited_suffix_instead_of_restarting_seed_prefix() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &["addrB"]));
        graph.insert("addrB".to_string(), outcome(b"B", "addrB", &[]));
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let mut cfg = RoundConfig::test_defaults();
        cfg.max_addrs = Some(1);

        let first = run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
            .await
            .unwrap();
        assert!(matches!(first, CrawlSliceReport::Partial(_)));
        assert!(store.scan_nodes().unwrap().is_empty());
        clock.set(10_001);
        let second = run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
            .await
            .unwrap();

        assert_eq!(prober.attempts(), vec!["addrA", "addrB"]);
        let status = completed(second);
        assert_eq!(reachable(&status), 2);
        assert_eq!(status.started, 10_000);
        assert_eq!(status.finished, 10_001);
        assert!(store.get_node(b"B").unwrap().is_some());
    }

    #[tokio::test]
    async fn restart_between_partial_slices_preserves_suffix_progress() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &["addrB"]));
        graph.insert("addrB".to_string(), outcome(b"B", "addrB", &[]));
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let clock = TestClock::new(10_000);
        let mut cfg = RoundConfig::test_defaults();
        cfg.max_addrs = Some(1);

        {
            let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
            assert!(matches!(
                run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                    .await
                    .unwrap(),
                CrawlSliceReport::Partial(_)
            ));
        }
        let reopened = CkbadgerStore::open_test_network(dir.path()).unwrap();
        clock.set(10_001);
        let report = run_crawl_slice(&reopened, &prober, &NoGeo, &clock, &cfg)
            .await
            .unwrap();
        assert_eq!(completed(report).round_id, 1);
        assert_eq!(prober.attempts(), vec!["addrA", "addrB"]);
    }

    #[tokio::test]
    async fn zero_slice_budget_checkpoints_without_publishing() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &[]));
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let cfg = RoundConfig {
            slice_budget: Some(Duration::ZERO),
            ..RoundConfig::test_defaults()
        };
        assert!(matches!(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
            CrawlSliceReport::Partial(_)
        ));
        assert!(prober.attempts().is_empty());
        assert!(store.get_network_status().unwrap().is_none());
        assert!(store.scan_nodes().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unrepresentable_slice_budget_fails_before_checkpointing() {
        let prober = MockProber::new(vec!["addrA".into()], HashMap::new());
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let cfg = RoundConfig {
            slice_budget: Some(Duration::MAX),
            ..RoundConfig::test_defaults()
        };

        let error = run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("monotonic clock range"));
        assert!(store.get_active_crawl().unwrap().is_none());
        assert!(prober.attempts().is_empty());
    }

    #[tokio::test]
    async fn first_seen_is_preserved_and_stale_nodes_are_pruned() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &[]));
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(1_000);
        let cfg = RoundConfig::test_defaults();
        completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        let first_seen = store.get_node(b"A").unwrap().unwrap().first_seen;

        clock.set(2_000);
        completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        let a = store.get_node(b"A").unwrap().unwrap();
        assert_eq!(a.first_seen, first_seen);
        assert_eq!(a.last_seen, 2_000);

        store
            .put_node(
                b"OLD",
                &NodeRecord {
                    own_addrs: vec!["addrOLD".into()],
                    last_seen: 1,
                    ..a
                },
            )
            .unwrap();
        clock.set(3_000_000);
        completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        assert!(store.get_node(b"OLD").unwrap().is_none());
    }

    #[tokio::test]
    async fn successful_dial_keeps_exact_alias_fresh_without_identify_own_addrs() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 1,
                    last_checkpoint_at: 1,
                    ..Default::default()
                },
                &[(
                    b"A".to_vec(),
                    CrawlCandidate {
                        addresses: vec![CrawlAddress {
                            addr: "addrA".into(),
                            first_advertised_at: 1,
                            last_advertised_at: 1,
                            last_verified_at: None,
                        }],
                        first_discovered_at: 1,
                        last_advertised_at: 1,
                        active: Some(ActiveCandidateProbe {
                            round_id: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )],
            )
            .unwrap();
        let prober = MockProber::new(
            Vec::new(),
            HashMap::from([(
                "addrA".to_string(),
                ProbeOutcome {
                    own_addrs: Vec::new(),
                    ..outcome(b"A", "addrA", &[])
                },
            )]),
        );
        let clock = TestClock::new(100);
        let cfg = RoundConfig {
            node_ttl_secs: 50,
            ..RoundConfig::test_defaults()
        };

        completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        let first = store.get_crawl_candidate(b"A").unwrap().unwrap();
        assert_eq!(first.addresses[0].last_advertised_at, 1);
        assert_eq!(first.addresses[0].last_verified_at, Some(100));

        clock.set(120);
        completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        let second = store.get_crawl_candidate(b"A").unwrap().unwrap();
        assert_eq!(second.addresses[0].last_verified_at, Some(120));
        assert_eq!(store.get_node(b"A").unwrap().unwrap().last_seen, 120);
    }

    #[tokio::test]
    async fn unreachable_node_is_downgraded_only_after_complete_round() {
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &["addrB"]));
        graph.insert("addrB".to_string(), outcome(b"B", "addrB", &[]));
        let first_prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(1_000_000);
        let cfg = RoundConfig::test_defaults();
        completed(
            run_crawl_slice(&store, &first_prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );

        let mut second_graph = HashMap::new();
        second_graph.insert("addrA".to_string(), outcome(b"A", "addrA", &["addrB"]));
        let second_prober = MockProber::new(vec!["addrA".into()], second_graph);
        let mut partial_cfg = RoundConfig::test_defaults();
        partial_cfg.max_addrs = Some(1);
        clock.set(1_000_100);
        assert!(matches!(
            run_crawl_slice(&store, &second_prober, &NoGeo, &clock, &partial_cfg)
                .await
                .unwrap(),
            CrawlSliceReport::Partial(_)
        ));
        assert!(store.get_node(b"B").unwrap().unwrap().reachable);
        assert_eq!(store.get_network_status().unwrap().unwrap().round_id, 1);

        let status = completed(
            run_crawl_slice(&store, &second_prober, &NoGeo, &clock, &partial_cfg)
                .await
                .unwrap(),
        );
        assert_eq!(reachable(&status), 1);
        assert!(!store.get_node(b"B").unwrap().unwrap().reachable);
    }

    #[tokio::test]
    async fn exhausted_evidence_counts_completed_rounds_and_survives_next_active_round() {
        let prober = MockProber::new(vec!["addrA".into()], HashMap::new());
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(1_000);

        let first = completed(
            run_crawl_slice(
                &store,
                &prober,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(exhausted(&first), 1);
        let peer_id = b"addrA";
        let evidence = store
            .get_crawl_candidate(peer_id)
            .unwrap()
            .unwrap()
            .last_completed
            .unwrap();
        assert_eq!(evidence.round_id, 1);
        assert_eq!(evidence.consecutive_exhausted_rounds, 1);
        assert_eq!(evidence.observations.len(), 1);

        clock.set(2_000);
        completed(
            run_crawl_slice(
                &store,
                &prober,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        let evidence = store
            .get_crawl_candidate(peer_id)
            .unwrap()
            .unwrap()
            .last_completed
            .unwrap();
        assert_eq!(evidence.round_id, 2);
        assert_eq!(evidence.consecutive_exhausted_rounds, 2);

        let partial = RoundConfig {
            slice_budget: Some(Duration::ZERO),
            ..RoundConfig::test_defaults()
        };
        clock.set(3_000);
        assert!(matches!(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &partial)
                .await
                .unwrap(),
            CrawlSliceReport::Partial(_)
        ));
        let candidate = store.get_crawl_candidate(peer_id).unwrap().unwrap();
        assert_eq!(candidate.last_completed.unwrap().round_id, 2);
        assert_eq!(candidate.active.unwrap().round_id, 3);
    }

    #[tokio::test]
    async fn latest_round_retains_stale_attempt_evidence_then_prunes_without_retrying_it() {
        let prober = MockProber::new(Vec::new(), HashMap::new());
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(1_000);
        let cfg = RoundConfig {
            node_ttl_secs: 10,
            ..RoundConfig::test_defaults()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 1,
                    last_checkpoint_at: 1,
                    ..Default::default()
                },
                &[(
                    (b"A").to_vec(),
                    CrawlCandidate {
                        addresses: vec![CrawlAddress {
                            addr: "addrA".into(),
                            first_advertised_at: 1,
                            last_advertised_at: 1,
                            last_verified_at: None,
                        }],
                        first_discovered_at: 1,
                        last_advertised_at: 1,
                        active: Some(ActiveCandidateProbe {
                            round_id: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )],
            )
            .unwrap();

        let first = completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        let first_candidates = store.scan_crawl_candidates().unwrap();
        assert_eq!(first.peer_outcomes.candidate_peers(1).unwrap(), 1);
        assert_eq!(
            first_candidates
                .iter()
                .filter(|(_, candidate)| {
                    candidate
                        .last_completed
                        .as_ref()
                        .is_some_and(|completed| completed.round_id == 1)
                })
                .count(),
            1
        );
        assert_eq!(first_candidates[0].1.last_advertised_at, 1);
        assert_eq!(prober.attempts(), vec!["addrA"]);

        clock.set(1_001);
        let second = completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        assert_eq!(second.peer_outcomes.candidate_peers(2).unwrap(), 0);
        assert_eq!(prober.attempts(), vec!["addrA"]);
        assert!(store.scan_crawl_candidates().unwrap().is_empty());
    }

    #[tokio::test]
    async fn frontier_overflow_fails_without_publishing() {
        let flood = ["addr1", "addr2", "addr3", "addr4"];
        let mut graph = HashMap::new();
        graph.insert("addrA".to_string(), outcome(b"A", "addrA", &flood));
        let prober = MockProber::new(vec!["addrA".into()], graph);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let cfg = RoundConfig {
            max_frontier: Some(3),
            ..RoundConfig::test_defaults()
        };
        let error = run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("frontier capacity exceeded"));
        assert!(store.get_network_status().unwrap().is_none());
        assert!(store.scan_nodes().unwrap().is_empty());
        let blocked = store.get_active_crawl().unwrap().unwrap();
        assert_eq!(
            blocked
                .address_observations
                .address_attempts(blocked.round_id)
                .unwrap(),
            0
        );
        let reason = blocked.blocked_reason.unwrap();
        assert!(reason.contains("checkpoint_candidate_addresses=1"));
        assert!(reason.contains("attempted_added=4"));
        assert!(reason.contains("candidate_addresses_if_accepted=5"));
        let candidates = store.scan_crawl_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1.addresses[0].addr, "addrA");
        assert_eq!(
            candidates[0].1.active.as_ref().unwrap().state,
            ActiveCandidateState::Pending
        );

        let resumed = RoundConfig {
            max_frontier: Some(5),
            ..RoundConfig::test_defaults()
        };
        let status = completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &resumed)
                .await
                .unwrap(),
        );
        assert_eq!(
            status
                .peer_outcomes
                .candidate_peers(status.round_id)
                .unwrap(),
            5
        );
        assert_eq!(address_attempts(&status), 5);
        assert_eq!(reachable(&status), 1);
        assert_eq!(exhausted(&status), 4);
    }

    #[tokio::test]
    async fn bounded_concurrency_reaches_but_never_exceeds_configured_limit() {
        let mut peer_by_addr = HashMap::new();
        let mut outcomes = HashMap::new();
        let mut bootnodes = Vec::new();
        for index in 0..6u8 {
            let addr = format!("addr{index}");
            let peer = vec![index + 1];
            bootnodes.push(addr.clone());
            peer_by_addr.insert(addr.clone(), peer.clone());
            outcomes.insert(addr.clone(), outcome(&peer, &addr, &[]));
        }
        let prober = InstrumentedProber::new(bootnodes, peer_by_addr, outcomes);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let cfg = RoundConfig {
            max_dial_concurrency: 3,
            ..RoundConfig::test_defaults()
        };

        let status = completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        assert_eq!(reachable(&status), 6);
        assert_eq!(prober.peak_in_flight(), 3);
    }

    #[tokio::test]
    async fn peer_gets_one_attempt_before_any_alias_retry_and_aliases_never_overlap() {
        let peer_by_addr = HashMap::from([
            ("a1".to_string(), b"A".to_vec()),
            ("a2".to_string(), b"A".to_vec()),
            ("b1".to_string(), b"B".to_vec()),
        ]);
        let outcomes = HashMap::from([
            ("a2".to_string(), outcome(b"A", "a2", &[])),
            ("b1".to_string(), outcome(b"B", "b1", &[])),
        ]);
        let prober = InstrumentedProber::new(Vec::new(), peer_by_addr, outcomes);
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        for (peer_id, addrs) in [
            (b"A".as_slice(), vec!["a1".into(), "a2".into()]),
            (b"B".as_slice(), vec!["b1".into()]),
        ] {
            store
                .put_node(
                    peer_id,
                    &NodeRecord {
                        own_addrs: addrs,
                        client_version: "old".into(),
                        flags: 0,
                        protocols: vec![],
                        first_seen: 1,
                        last_seen: 1,
                        last_reachable_at: 1,
                        reachable: true,
                        geo: None,
                        asn: None,
                        last_rtt_ms: None,
                        discovery: DiscoveryEvidence::default(),
                        known_peers: vec![],
                    },
                )
                .unwrap();
        }
        let clock = TestClock::new(10_000);
        let cfg = RoundConfig {
            max_dial_concurrency: 1,
            ..RoundConfig::test_defaults()
        };

        completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &cfg)
                .await
                .unwrap(),
        );
        assert_eq!(prober.attempts(), vec!["a1", "b1", "a2"]);
        assert_eq!(prober.peer_peak(b"A"), 1);
        assert_eq!(prober.peer_peak(b"B"), 1);
    }

    #[tokio::test]
    async fn foreign_address_does_not_skip_remaining_aliases_and_counts_peer_once() {
        let clock = TestClock::new(10_000);

        let reachable_dir = tempfile::tempdir().unwrap();
        let reachable_store = CkbadgerStore::open_test_network(reachable_dir.path()).unwrap();
        let reachable_prober = ForeignAliasProber {
            second_reachable: true,
            attempts: Mutex::new(Vec::new()),
        };
        let reachable_status = completed(
            run_crawl_slice(
                &reachable_store,
                &reachable_prober,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(*reachable_prober.attempts.lock().unwrap(), vec!["a1", "a2"]);
        assert_eq!(reachable(&reachable_status), 1);
        assert_eq!(foreign(&reachable_status), 0);
        assert_eq!(address_attempts(&reachable_status), 2);
        let evidence = reachable_store
            .get_crawl_candidate(b"A")
            .unwrap()
            .unwrap()
            .last_completed
            .unwrap();
        assert_eq!(evidence.observations.len(), 2);
        assert_eq!(
            evidence.observations[0].result,
            AddressProbeResult::ForeignNetwork
        );
        assert_eq!(
            evidence.observations[1].result,
            AddressProbeResult::SameNetworkIdentified
        );

        let foreign_dir = tempfile::tempdir().unwrap();
        let foreign_store = CkbadgerStore::open_test_network(foreign_dir.path()).unwrap();
        let foreign_prober = ForeignAliasProber {
            second_reachable: false,
            attempts: Mutex::new(Vec::new()),
        };
        let foreign_status = completed(
            run_crawl_slice(
                &foreign_store,
                &foreign_prober,
                &NoGeo,
                &clock,
                &RoundConfig::test_defaults(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(*foreign_prober.attempts.lock().unwrap(), vec!["a1", "a2"]);
        assert_eq!(reachable(&foreign_status), 0);
        assert_eq!(exhausted(&foreign_status), 0);
        assert_eq!(foreign(&foreign_status), 1);
        assert_eq!(address_attempts(&foreign_status), 2);
    }

    #[tokio::test]
    async fn internal_probe_error_propagates_without_publishing() {
        struct ErrorProber;

        #[async_trait]
        impl Prober for ErrorProber {
            fn candidate_from_addr(
                &self,
                addr: &str,
                _peer_hint: Option<&[u8]>,
            ) -> anyhow::Result<Option<ProbeCandidate>> {
                Ok(Some(ProbeCandidate {
                    peer_id: b"A".to_vec(),
                    addr: addr.to_string(),
                }))
            }

            async fn probe(&self, _peer_id: &[u8], _addr: &str) -> anyhow::Result<ProbeResult> {
                anyhow::bail!("injected prober invariant failure")
            }

            fn bootnodes(&self) -> Vec<String> {
                vec!["addrA".into()]
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let clock = TestClock::new(10_000);
        let error = run_crawl_slice(
            &store,
            &ErrorProber,
            &NoGeo,
            &clock,
            &RoundConfig::test_defaults(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("failed internally"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store.get_network_status().unwrap().is_none());
        assert!(store.scan_nodes().unwrap().is_empty());
    }
}
