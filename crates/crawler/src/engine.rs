use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
use ckbadger_store::{
    ActiveCrawl, CkbadgerStore, CrawlAddress, CrawlCandidate, CrawlCandidateResult, CrawlProgress,
    HistoryPoint, LatestStatus, NodeRecord, StagedProbeOutcome,
};
use futures::stream::{FuturesUnordered, StreamExt};

use crate::geoip::GeoIp;
use crate::prober::{ProbeCandidate, ProbeObservation, Prober};

/// Top-N (label, count), sorted by count desc then label asc for determinism.
pub fn top_n_histogram<'a>(labels: impl Iterator<Item = &'a str>, n: usize) -> Vec<(String, u64)> {
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for label in labels {
        *counts.entry(label).or_insert(0) += 1;
    }
    let mut values: Vec<(String, u64)> = counts
        .into_iter()
        .map(|(label, count)| (label.to_string(), count))
        .collect();
    values.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    values.truncate(n);
    values
}

/// Map advertised addresses to peer ids. These are address-book references,
/// not proof that the referenced peer was reachable in the same round.
pub fn resolve_known_peers(
    discovered: &[String],
    addr_to_peer: &HashMap<String, Vec<u8>>,
) -> Vec<Vec<u8>> {
    let mut peers: Vec<Vec<u8>> = discovered
        .iter()
        .filter_map(|addr| addr_to_peer.get(addr).cloned())
        .collect();
    peers.sort();
    peers.dedup();
    peers
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
    Completed(LatestStatus),
}

fn checked_inc(value: &mut u64, field: &str, round_id: u64) -> anyhow::Result<()> {
    *value = value
        .checked_add(1)
        .with_context(|| format!("crawler counter overflow: field={field}, round_id={round_id}"))?;
    Ok(())
}

fn prepare_candidate_for_round(candidate: &mut CrawlCandidate, round_id: u64) {
    if candidate.round_id != round_id {
        candidate.round_id = round_id;
        candidate.result = CrawlCandidateResult::Pending;
        candidate.foreign_observed = false;
        candidate.staged_success = None;
    }
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
        round_id,
        last_scheduled_sequence: 0,
        result: CrawlCandidateResult::Pending,
        foreign_observed: false,
        staged_success: None,
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
                last_advertised_at: advertised_at,
                attempted_round: 0,
            });
            record.addresses.sort_by(|a, b| a.addr.cmp(&b.addr));
            if matches!(
                record.result,
                CrawlCandidateResult::Exhausted | CrawlCandidateResult::ForeignNetwork
            ) {
                record.result = CrawlCandidateResult::RetryAlias;
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
            anyhow::bail!(
                "candidate address maps to multiple peers: addr={}, first=0x{}, second=0x{}",
                addr,
                hex::encode(first.as_ref().unwrap()),
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

fn candidate_has_untried_addr(candidate: &CrawlCandidate, round_id: u64) -> bool {
    candidate
        .addresses
        .iter()
        .any(|address| address.attempted_round != round_id)
}

fn select_next_candidate(
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
    round_id: u64,
    in_flight: &HashSet<Vec<u8>>,
) -> Option<(Vec<u8>, String)> {
    let mut selected: Option<(u8, u64, Vec<u8>, String)> = None;
    for (peer_id, candidate) in candidates {
        if candidate.round_id != round_id || in_flight.contains(peer_id) {
            continue;
        }
        let tier = match candidate.result {
            CrawlCandidateResult::Pending => 0,
            CrawlCandidateResult::RetryAlias => 1,
            CrawlCandidateResult::Succeeded
            | CrawlCandidateResult::Exhausted
            | CrawlCandidateResult::ForeignNetwork => continue,
        };
        let Some(addr) = candidate
            .addresses
            .iter()
            .filter(|address| address.attempted_round != round_id)
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
    candidate.round_id == round_id
        && matches!(
            candidate.result,
            CrawlCandidateResult::Succeeded
                | CrawlCandidateResult::Exhausted
                | CrawlCandidateResult::ForeignNetwork
        )
}

fn refresh_foreign_peer_count(
    active: &mut ActiveCrawl,
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
) -> anyhow::Result<()> {
    active.foreign_peers = u64::try_from(
        candidates
            .values()
            .filter(|candidate| candidate.result == CrawlCandidateResult::ForeignNetwork)
            .count(),
    )
    .context("crawler foreign peer count exceeds u64")?;
    Ok(())
}

fn progress_from(
    active: &ActiveCrawl,
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
) -> CrawlProgress {
    CrawlProgress {
        round_id: active.round_id,
        started_at: active.started_at,
        last_checkpoint_at: active.last_checkpoint_at,
        candidate_peers: candidates.len() as u64,
        completed_peers: candidates
            .values()
            .filter(|candidate| candidate_is_terminal(candidate, active.round_id))
            .count() as u64,
        address_attempts: active.address_attempts,
        blocked_reason: active.blocked_reason.clone(),
    }
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
    let checkpoint_address_count = frontier_address_count(&candidates)?;
    let mut dirty = HashSet::new();
    let mut active = match store.get_active_crawl()? {
        Some(mut active) => {
            active.blocked_reason = None;
            for (peer_id, candidate) in &mut candidates {
                if candidate.round_id != active.round_id {
                    prepare_candidate_for_round(candidate, active.round_id);
                    dirty.insert(peer_id.clone());
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
            for (peer_id, candidate) in &mut candidates {
                prepare_candidate_for_round(candidate, round_id);
                dirty.insert(peer_id.clone());
            }
            ActiveCrawl {
                round_id,
                started_at: now,
                last_checkpoint_at: now,
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
    for (peer_id, node) in store.scan_nodes()? {
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

    refresh_foreign_peer_count(&mut active, &candidates)?;
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

fn mark_address_attempted(
    candidate: &mut CrawlCandidate,
    addr: &str,
    round_id: u64,
) -> anyhow::Result<()> {
    let address = candidate
        .addresses
        .iter_mut()
        .find(|candidate_addr| candidate_addr.addr == addr)
        .with_context(|| {
            format!(
                "scheduled address missing from candidate: round_id={}, addr={}",
                round_id, addr
            )
        })?;
    if address.attempted_round == round_id {
        anyhow::bail!(
            "candidate address attempted twice in one round: round_id={}, addr={}",
            round_id,
            addr
        );
    }
    address.attempted_round = round_id;
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
    mark_address_attempted(candidate, &addr, round_id)?;
    checked_inc(&mut active.address_attempts, "address_attempts", round_id)?;

    let mut dirty = HashSet::from([peer_id.clone()]);
    match result.observation {
        ProbeObservation::Reachable => {
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
            let mut normalized_discovered = Vec::new();
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
                    normalized_discovered.push(normalized.addr);
                    dirty.insert(normalized.peer_id);
                }
            }
            normalized_discovered.sort();
            normalized_discovered.dedup();
            outcome.discovered_addrs = normalized_discovered;

            let candidate = candidates.get_mut(&peer_id).with_context(|| {
                format!(
                    "successful candidate disappeared during ingestion: peer_id=0x{}",
                    hex::encode(&peer_id)
                )
            })?;
            candidate.last_advertised_at = candidate.last_advertised_at.max(observed_at);
            candidate.result = CrawlCandidateResult::Succeeded;
            candidate.staged_success = Some(StagedProbeOutcome {
                observed_at,
                client_version: outcome.client_version,
                flags: outcome.flags,
                protocols: outcome.protocols,
                own_addrs: outcome.own_addrs,
                rtt_ms: outcome.rtt_ms,
                discovered_addrs: outcome.discovered_addrs,
            });
        }
        ProbeObservation::ForeignNetwork => {
            if result.outcome.is_some() {
                anyhow::bail!("foreign-network observation unexpectedly carried an outcome");
            }
            let candidate = candidates.get_mut(&peer_id).unwrap();
            candidate.foreign_observed = true;
            candidate.staged_success = None;
            candidate.result = if candidate_has_untried_addr(candidate, round_id) {
                CrawlCandidateResult::RetryAlias
            } else {
                CrawlCandidateResult::ForeignNetwork
            };
        }
        failure => {
            if result.outcome.is_some() {
                anyhow::bail!(
                    "failed probe observation unexpectedly carried an outcome: {:?}",
                    failure
                );
            }
            checked_inc(
                &mut active.failed_address_attempts,
                "failed_address_attempts",
                round_id,
            )?;
            if failure == ProbeObservation::MalformedAddress {
                checked_inc(
                    &mut active.malformed_addresses,
                    "malformed_addresses",
                    round_id,
                )?;
            }
            let candidate = candidates.get_mut(&peer_id).unwrap();
            candidate.staged_success = None;
            candidate.result = if candidate_has_untried_addr(candidate, round_id) {
                CrawlCandidateResult::RetryAlias
            } else if candidate.foreign_observed {
                CrawlCandidateResult::ForeignNetwork
            } else {
                CrawlCandidateResult::Exhausted
            };
        }
    }

    refresh_foreign_peer_count(active, candidates)?;
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
        Metric::TotalNodes,
        Metric::ReachableNodes,
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
        if !candidate_is_terminal(candidate, active.round_id) {
            anyhow::bail!(
                "cannot publish undrained crawl round: round_id={}, peer_id=0x{}, result={:?}",
                active.round_id,
                hex::encode(peer_id),
                candidate.result
            );
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

    let existing_rows = store.scan_nodes()?;
    let existing: BTreeMap<Vec<u8>, NodeRecord> = existing_rows.iter().cloned().collect();
    let mut next_nodes = existing.clone();
    let mut addr_to_peer = HashMap::new();
    for (peer_id, candidate) in &candidates {
        for address in &candidate.addresses {
            addr_to_peer.insert(address.addr.clone(), peer_id.clone());
        }
    }

    let mut new_nodes = 0u64;
    for (peer_id, candidate) in &candidates {
        if candidate.result != CrawlCandidateResult::Succeeded {
            continue;
        }
        let staged = candidate.staged_success.as_ref().with_context(|| {
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
            checked_inc(&mut new_nodes, "new_nodes", active.round_id)?;
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
                known_peers: resolve_known_peers(&staged.discovered_addrs, &addr_to_peer),
            },
        );
    }

    let successful: HashSet<Vec<u8>> = candidates
        .iter()
        .filter(|(_, candidate)| candidate.result == CrawlCandidateResult::Succeeded)
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

    let candidate_peers = candidates.len() as u64;
    let attempted_peers = candidates
        .values()
        .filter(|candidate| candidate_is_terminal(candidate, active.round_id))
        .count() as u64;
    let reachable_peers = candidates
        .values()
        .filter(|candidate| candidate.result == CrawlCandidateResult::Succeeded)
        .count() as u64;
    let unreachable_peers = candidates
        .values()
        .filter(|candidate| candidate.result == CrawlCandidateResult::Exhausted)
        .count() as u64;
    let foreign_peers = candidates
        .values()
        .filter(|candidate| candidate.result == CrawlCandidateResult::ForeignNetwork)
        .count() as u64;
    if foreign_peers != active.foreign_peers {
        anyhow::bail!(
            "foreign peer counter invariant failed: round_id={}, candidates={}, counter={}",
            active.round_id,
            foreign_peers,
            active.foreign_peers
        );
    }

    let total_known = next_nodes.len() as u64;
    let published_reachable = next_nodes
        .values()
        .filter(|record| record.reachable)
        .count() as u64;
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
    );
    let countries = top_n_histogram(
        next_nodes
            .values()
            .filter_map(|record| record.geo.as_ref().map(|geo| geo.country.as_str())),
        cfg.top_n,
    );
    let status = LatestStatus {
        round_id: active.round_id,
        started: active.started_at,
        finished,
        candidate_peers,
        attempted_peers,
        reachable_peers,
        unreachable_peers,
        address_attempts: active.address_attempts,
        failed_address_attempts: active.failed_address_attempts,
        foreign_peers,
        malformed_addresses: active.malformed_addresses,
        new_nodes,
        total_known,
    };

    let mut history_puts = Vec::new();
    for granularity in [Granularity::Hour, Granularity::Day] {
        let bucket = bucket_of(finished, granularity);
        history_puts.push((
            Metric::TotalNodes,
            granularity,
            bucket,
            HistoryPoint {
                scalar: total_known,
                buckets: vec![],
            },
        ));
        history_puts.push((
            Metric::ReachableNodes,
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

    let candidate_cutoff = finished.checked_sub(cfg.node_ttl_secs);
    let mut candidate_deletes = Vec::new();
    let mut candidate_puts = Vec::new();
    for (peer_id, candidate) in &mut candidates {
        candidate.staged_success = None;
        if candidate_cutoff.is_some_and(|cutoff| candidate.last_advertised_at < cutoff) {
            candidate_deletes.push(peer_id.clone());
        } else {
            candidate_puts.push((peer_id.clone(), candidate.clone()));
        }
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

/// Run one bounded execution slice. Partial slices checkpoint only operational
/// state; a drained logical round is atomically published.
pub async fn run_crawl_slice(
    store: &CkbadgerStore,
    prober: &dyn Prober,
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
            let Some((peer_id, addr)) =
                select_next_candidate(&candidates, active.round_id, &in_flight_peers)
            else {
                break;
            };
            active.next_schedule_sequence = active
                .next_schedule_sequence
                .checked_add(1)
                .context("crawler schedule sequence overflow")?;
            candidates
                .get_mut(&peer_id)
                .expect("selected candidate must exist")
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

    if select_next_candidate(&candidates, active.round_id, &HashSet::new()).is_some() {
        active.last_checkpoint_at = clock.now()?;
        store.checkpoint_crawl(&active, &[])?;
        return Ok(CrawlSliceReport::Partial(progress_from(
            &active,
            &candidates,
        )));
    }

    Ok(CrawlSliceReport::Completed(publish_completed_round(
        store, geoip, clock, cfg, &active, candidates,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use crate::geoip::NoGeo;
    use crate::mock_prober::MockProber;
    use crate::prober::{ProbeOutcome, ProbeResult};
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
                Ok(ProbeResult::failed(ProbeObservation::ForeignNetwork))
            } else {
                Ok(ProbeResult::reachable(outcome(b"A", "a2", &[])))
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
            Ok(match self.outcomes.get(addr).cloned() {
                Some(outcome) => ProbeResult::reachable(outcome),
                None => ProbeResult::failed(ProbeObservation::DialFailed),
            })
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
        }
    }

    fn completed(report: CrawlSliceReport) -> LatestStatus {
        match report {
            CrawlSliceReport::Completed(status) => status,
            CrawlSliceReport::Partial(progress) => {
                panic!("expected completed round, got progress: {progress:?}")
            }
        }
    }

    #[test]
    fn histogram_top_n_desc_ties_by_label() {
        let labels = vec!["a", "b", "a", "c", "b", "a"];
        assert_eq!(
            top_n_histogram(labels.into_iter(), 2),
            vec![("a".to_string(), 3), ("b".to_string(), 2)]
        );
    }

    #[test]
    fn resolve_edges_only_to_known_peer_candidates() {
        let mut index = HashMap::new();
        index.insert("addrB".to_string(), vec![b'B']);
        assert_eq!(
            resolve_known_peers(&["addrB".into(), "addrX".into()], &index),
            vec![vec![b'B']]
        );
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
        assert_eq!(status.reachable_peers, 2);
        assert_eq!(status.unreachable_peers, 1);
        assert_eq!(status.total_known, 2);
        assert!(store.get_active_crawl().unwrap().is_none());
        assert_eq!(store.scan_nodes().unwrap().len(), 2);
        let b = store.get_node(b"B").unwrap().unwrap();
        assert_eq!(b.known_peers, vec![b"A".to_vec(), b"addrC".to_vec()]);
        assert_eq!(
            store
                .scan_history(Metric::TotalNodes, Granularity::Hour, 0, u64::MAX)
                .unwrap()
                .len(),
            1
        );
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
        assert_eq!(status.reachable_peers, 2);
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
        assert_eq!(status.reachable_peers, 1);
        assert!(!store.get_node(b"B").unwrap().unwrap().reachable);
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
        assert_eq!(blocked.address_attempts, 0);
        let reason = blocked.blocked_reason.unwrap();
        assert!(reason.contains("checkpoint_candidate_addresses=1"));
        assert!(reason.contains("attempted_added=4"));
        assert!(reason.contains("candidate_addresses_if_accepted=5"));
        let candidates = store.scan_crawl_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1.addresses[0].addr, "addrA");
        assert_eq!(candidates[0].1.result, CrawlCandidateResult::Pending);

        let resumed = RoundConfig {
            max_frontier: Some(5),
            ..RoundConfig::test_defaults()
        };
        let status = completed(
            run_crawl_slice(&store, &prober, &NoGeo, &clock, &resumed)
                .await
                .unwrap(),
        );
        assert_eq!(status.candidate_peers, 5);
        assert_eq!(status.address_attempts, 5);
        assert_eq!(status.reachable_peers, 1);
        assert_eq!(status.unreachable_peers, 4);
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
        assert_eq!(status.reachable_peers, 6);
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
        assert_eq!(reachable_status.reachable_peers, 1);
        assert_eq!(reachable_status.foreign_peers, 0);
        assert_eq!(reachable_status.address_attempts, 2);

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
        assert_eq!(foreign_status.reachable_peers, 0);
        assert_eq!(foreign_status.unreachable_peers, 0);
        assert_eq!(foreign_status.foreign_peers, 1);
        assert_eq!(foreign_status.address_attempts, 2);
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
