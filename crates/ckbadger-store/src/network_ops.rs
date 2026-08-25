//! Network-store node, stats, and durable crawler operations
//! (`CF_NET_NODES`, `CF_NET_STATS`, `CF_NET_CRAWL`).
//!
//! Node records (`CF_NET_NODES`) are keyed by raw `peer_id` bytes and hold the
//! crawler's view of a peer (addresses, client version, reachability, geo/asn,
//! sampled known peers).
//!
//! Stats (`CF_NET_STATS`) hold the latest-round status singleton (keyed by
//! [`STATS_STATUS_KEY`]) plus time-bucketed history points, keyed by
//! `[metric][granularity][big-endian bucket]` (see [`history_key`]). The
//! big-endian bucket keeps each `(metric, granularity)` series in chronological
//! key order, so range scans and prunes are contiguous.
//!
//! The crawler is the sole writer. Convenience operations use direct writes;
//! slice checkpoints and completed-round publication use RocksDB write batches
//! so their cross-CF state transitions are atomic.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::network_keys::{
    crawl_candidate_key, history_key, history_prefix, Granularity, Metric, CRAWL_ACTIVE_KEY,
    CRAWL_CANDIDATE_PREFIX, STATS_STATUS_KEY,
};
use crate::store::{CkbadgerStore, CF_NET_CRAWL, CF_NET_NODES, CF_NET_STATS};
use crate::{
    bytes_to_hex, checked_candidate_alias_map, checked_resolve_known_peers, ActiveCandidateState,
    ActiveCrawl, AddressObservationHistogram, CompletedCandidateEvidence,
    CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlCandidate, CrawlProgress,
    DiscoveryEvidence, HistoryPoint, LatestStatus, NodeRecord,
};

struct CompletedCrawlCommit<'a> {
    round_id: u64,
    node_puts: &'a [(Vec<u8>, NodeRecord)],
    node_deletes: &'a [Vec<u8>],
    candidate_puts: &'a [(Vec<u8>, CrawlCandidate)],
    candidate_deletes: &'a [(Vec<u8>, CrawlCandidate)],
    active: &'a ActiveCrawl,
    status: &'a LatestStatus,
}

impl CkbadgerStore {
    /// Insert or overwrite the node record for `peer_id`.
    pub fn put_node(&self, peer_id: &[u8], rec: &NodeRecord) -> anyhow::Result<()> {
        let cf = self.cf(CF_NET_NODES);
        let value = bincode::serialize(rec)?;
        self.put_cf(cf, peer_id, &value)
    }

    /// Fetch the node record for `peer_id`, or `None` if absent.
    pub fn get_node(&self, peer_id: &[u8]) -> anyhow::Result<Option<NodeRecord>> {
        let cf = self.cf(CF_NET_NODES);
        match self.get_cf(cf, peer_id)? {
            Some(value) => {
                let rec = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize node record in get_node: peer_id=0x{}, error={}",
                        bytes_to_hex(peer_id),
                        e
                    )
                })?;
                Ok(Some(rec))
            }
            None => Ok(None),
        }
    }

    /// Scan every node record, returning `(peer_id, record)` pairs.
    pub fn scan_nodes(&self) -> anyhow::Result<Vec<(Vec<u8>, NodeRecord)>> {
        let cf = self.cf(CF_NET_NODES);
        let iter = self.iterator_cf(cf, rocksdb::IteratorMode::Start);
        let mut out = Vec::new();
        for item in iter {
            let (key, value) = item
                .map_err(|e| anyhow::anyhow!("failed to iterate net_nodes in scan_nodes: {}", e))?;
            let rec: NodeRecord = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize node record in scan_nodes: peer_id=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            out.push((key.to_vec(), rec));
        }
        Ok(out)
    }

    /// Delete the node record for `peer_id` (no-op if absent).
    pub fn delete_node(&self, peer_id: &[u8]) -> anyhow::Result<()> {
        let cf = self.cf(CF_NET_NODES);
        self.delete_cf(cf, peer_id)
    }

    /// Delete records whose `last_seen` is strictly older than `cutoff`.
    /// Returns the number of records deleted.
    pub fn prune_nodes_older_than(&self, cutoff: u64) -> anyhow::Result<u64> {
        let cf = self.cf(CF_NET_NODES);
        let mut deleted = 0u64;
        for (peer_id, rec) in self.scan_nodes()? {
            if rec.last_seen < cutoff {
                self.delete_cf(cf, &peer_id)?;
                deleted = deleted.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "network node prune count overflow: cutoff={}, peer_id=0x{}",
                        cutoff,
                        bytes_to_hex(&peer_id)
                    )
                })?;
            }
        }
        Ok(deleted)
    }

    // --- Stats ops (`CF_NET_STATS`) ---

    /// Store the latest-round status singleton, overwriting the previous round.
    pub fn put_network_status(&self, s: &LatestStatus) -> anyhow::Result<()> {
        let cf = self.cf(CF_NET_STATS);
        let value = bincode::serialize(s)?;
        self.put_cf(cf, &STATS_STATUS_KEY, &value)
    }

    /// Fetch the latest-round status singleton, or `None` if no round has
    /// finished yet.
    pub fn get_network_status(&self) -> anyhow::Result<Option<LatestStatus>> {
        let cf = self.cf(CF_NET_STATS);
        match self.get_cf(cf, &STATS_STATUS_KEY)? {
            Some(value) => {
                let status = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize network status in get_network_status: {}",
                        e
                    )
                })?;
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    /// Insert or overwrite the history point for one `(metric, granularity,
    /// bucket)`.
    pub fn put_history_point(
        &self,
        m: Metric,
        g: Granularity,
        bucket: u64,
        p: &HistoryPoint,
    ) -> anyhow::Result<()> {
        let cf = self.cf(CF_NET_STATS);
        let value = bincode::serialize(p)?;
        self.put_cf(cf, &history_key(m, g, bucket), &value)
    }

    /// Inclusive range scan over a single `(metric, granularity)` series,
    /// returning `(bucket, point)` pairs in ascending bucket order. Buckets
    /// outside `[from, to]` and any other series are excluded.
    ///
    /// Uses a forward iterator seeked to the `from` key, stopping as soon as a
    /// key leaves this `(metric, granularity)` prefix or its bucket exceeds
    /// `to` — both are guaranteed by the big-endian key layout.
    pub fn scan_history(
        &self,
        m: Metric,
        g: Granularity,
        from: u64,
        to: u64,
    ) -> anyhow::Result<Vec<(u64, HistoryPoint)>> {
        let cf = self.cf(CF_NET_STATS);
        let prefix = history_prefix(m, g);
        let start = history_key(m, g, from);
        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        );
        let mut out = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate net_stats in scan_history: {}", e)
            })?;
            // Left this (metric, granularity) series: nothing further can match.
            if key.len() != 10 || key[0..2] != prefix {
                break;
            }
            let bucket = u64::from_be_bytes(key[2..10].try_into().unwrap());
            // Ascending order ⇒ once past `to`, every later bucket is too.
            if bucket > to {
                break;
            }
            let point: HistoryPoint = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize history point in scan_history: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            out.push((bucket, point));
        }
        Ok(out)
    }

    /// Delete history points strictly older than `cutoff` in a single `(metric,
    /// granularity)` series. Returns the number of buckets deleted.
    pub fn prune_history_before(
        &self,
        m: Metric,
        g: Granularity,
        cutoff: u64,
    ) -> anyhow::Result<u64> {
        let cf = self.cf(CF_NET_STATS);
        let mut deleted = 0u64;
        for (bucket, _) in self.scan_history(m, g, 0, u64::MAX)? {
            if bucket < cutoff {
                self.delete_cf(cf, &history_key(m, g, bucket))?;
                deleted = deleted.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "network history prune count overflow: metric={m:?}, granularity={g:?}, cutoff={cutoff}, bucket={bucket}"
                    )
                })?;
            }
        }
        Ok(deleted)
    }

    // --- Durable crawl scheduler ops (`CF_NET_CRAWL`) ---

    pub fn get_active_crawl(&self) -> anyhow::Result<Option<ActiveCrawl>> {
        let cf = self.cf(CF_NET_CRAWL);
        match self.get_cf(cf, &CRAWL_ACTIVE_KEY)? {
            Some(value) => bincode::deserialize(&value)
                .map(Some)
                .map_err(|e| anyhow::anyhow!("failed to deserialize active crawl: error={}", e)),
            None => Ok(None),
        }
    }

    pub fn get_crawl_candidate(&self, peer_id: &[u8]) -> anyhow::Result<Option<CrawlCandidate>> {
        let cf = self.cf(CF_NET_CRAWL);
        let key = crawl_candidate_key(peer_id);
        match self.get_cf(cf, &key)? {
            Some(value) => bincode::deserialize(&value).map(Some).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize crawl candidate: peer_id=0x{}, error={}",
                    bytes_to_hex(peer_id),
                    e
                )
            }),
            None => Ok(None),
        }
    }

    pub fn scan_crawl_candidates(&self) -> anyhow::Result<Vec<(Vec<u8>, CrawlCandidate)>> {
        let cf = self.cf(CF_NET_CRAWL);
        let start = [CRAWL_CANDIDATE_PREFIX];
        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        );
        let mut out = Vec::new();
        for item in iter {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("failed to iterate net_crawl candidates: {}", e))?;
            if key.first().copied() != Some(CRAWL_CANDIDATE_PREFIX) {
                break;
            }
            if key.len() == 1 {
                anyhow::bail!("invalid empty peer id in net_crawl candidate key");
            }
            let peer_id = key[1..].to_vec();
            let candidate: CrawlCandidate = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize crawl candidate during scan: peer_id=0x{}, error={}",
                    bytes_to_hex(&peer_id),
                    e
                )
            })?;
            out.push((peer_id, candidate));
        }
        Ok(out)
    }

    /// Atomically persist active-round metadata and every changed peer
    /// candidate. This is the durability boundary after a probe completes.
    pub fn checkpoint_crawl(
        &self,
        active: &ActiveCrawl,
        candidates: &[(Vec<u8>, CrawlCandidate)],
    ) -> anyhow::Result<()> {
        let cf = self.cf(CF_NET_CRAWL);
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(cf, CRAWL_ACTIVE_KEY, bincode::serialize(active)?);
        for (peer_id, candidate) in candidates {
            if peer_id.is_empty() {
                anyhow::bail!("cannot checkpoint crawl candidate with empty peer id");
            }
            batch.put_cf(
                cf,
                crawl_candidate_key(peer_id),
                bincode::serialize(candidate)?,
            );
        }
        self.write_batch(batch)
    }

    /// Return exact active progress without conflating peer and address units.
    pub fn get_crawl_progress(&self) -> anyhow::Result<Option<CrawlProgress>> {
        let Some(active) = self.get_active_crawl()? else {
            return Ok(None);
        };
        let candidates = self.scan_crawl_candidates()?;
        let candidate_peers = u64::try_from(
            candidates
                .iter()
                .filter(|(_, candidate)| {
                    candidate
                        .active
                        .as_ref()
                        .is_some_and(|probe| probe.round_id == active.round_id)
                })
                .count(),
        )
        .map_err(|_| anyhow::anyhow!("active crawl candidate count exceeds u64"))?;
        let completed_peers = u64::try_from(
            candidates
                .iter()
                .filter(|(_, candidate)| {
                    candidate.active.as_ref().is_some_and(|probe| {
                        probe.round_id == active.round_id
                            && matches!(
                                probe.state,
                                ActiveCandidateState::Succeeded
                                    | ActiveCandidateState::Exhausted
                                    | ActiveCandidateState::ForeignNetwork
                            )
                    })
                })
                .count(),
        )
        .map_err(|_| anyhow::anyhow!("active crawl completed peer count exceeds u64"))?;
        Ok(Some(CrawlProgress {
            round_id: active.round_id,
            started_at: active.started_at,
            last_checkpoint_at: active.last_checkpoint_at,
            candidate_peers,
            completed_peers,
            address_attempts: active
                .address_observations
                .address_attempts(active.round_id)?,
            blocked_reason: active.blocked_reason,
        }))
    }

    /// Atomically replace the published node snapshot delta, completed status,
    /// history changes, candidate inventory changes, and active-round marker.
    /// The API secondary therefore cannot observe a partially published round.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_crawl_round(
        &self,
        round_id: u64,
        node_puts: &[(Vec<u8>, NodeRecord)],
        node_deletes: &[Vec<u8>],
        candidate_puts: &[(Vec<u8>, CrawlCandidate)],
        candidate_deletes: &[(Vec<u8>, CrawlCandidate)],
        status: &LatestStatus,
        history_puts: &[(Metric, Granularity, u64, HistoryPoint)],
        history_deletes: &[(Metric, Granularity, u64)],
    ) -> anyhow::Result<()> {
        let active = self.get_active_crawl()?.ok_or_else(|| {
            anyhow::anyhow!("cannot commit crawl round {}: no active crawl", round_id)
        })?;
        if active.round_id != round_id || status.round_id != round_id {
            anyhow::bail!(
                "cannot commit crawl round: active_round={}, requested_round={}, status_round={}",
                active.round_id,
                round_id,
                status.round_id
            );
        }

        self.validate_completed_crawl_commit(CompletedCrawlCommit {
            round_id,
            node_puts,
            node_deletes,
            candidate_puts,
            candidate_deletes,
            active: &active,
            status,
        })?;

        let nodes_cf = self.cf(CF_NET_NODES);
        let stats_cf = self.cf(CF_NET_STATS);
        let crawl_cf = self.cf(CF_NET_CRAWL);
        let mut batch = rocksdb::WriteBatch::default();

        for (peer_id, rec) in node_puts {
            if peer_id.is_empty() {
                anyhow::bail!("cannot publish node with empty peer id: round={}", round_id);
            }
            batch.put_cf(nodes_cf, peer_id, bincode::serialize(rec)?);
        }
        for peer_id in node_deletes {
            batch.delete_cf(nodes_cf, peer_id);
        }
        for (peer_id, candidate) in candidate_puts {
            if peer_id.is_empty() {
                anyhow::bail!(
                    "cannot publish crawl candidate with empty peer id: round={}",
                    round_id
                );
            }
            batch.put_cf(
                crawl_cf,
                crawl_candidate_key(peer_id),
                bincode::serialize(candidate)?,
            );
        }
        for (peer_id, _) in candidate_deletes {
            batch.delete_cf(crawl_cf, crawl_candidate_key(peer_id));
        }
        batch.put_cf(stats_cf, STATS_STATUS_KEY, bincode::serialize(status)?);
        for (metric, granularity, bucket, point) in history_puts {
            batch.put_cf(
                stats_cf,
                history_key(*metric, *granularity, *bucket),
                bincode::serialize(point)?,
            );
        }
        for (metric, granularity, bucket) in history_deletes {
            batch.delete_cf(stats_cf, history_key(*metric, *granularity, *bucket));
        }
        batch.delete_cf(crawl_cf, CRAWL_ACTIVE_KEY);
        self.write_batch(batch)
    }

    fn validate_completed_crawl_commit(
        &self,
        commit: CompletedCrawlCommit<'_>,
    ) -> anyhow::Result<()> {
        let CompletedCrawlCommit {
            round_id,
            node_puts,
            node_deletes,
            candidate_puts,
            candidate_deletes,
            active,
            status,
        } = commit;
        if status.started != active.started_at {
            anyhow::bail!(
                "completed status start invariant failed: round_id={}, active_started={}, status_started={}",
                round_id,
                active.started_at,
                status.started
            );
        }
        if status.finished < status.started {
            anyhow::bail!(
                "completed status clock invariant failed: round_id={}, started={}, finished={}",
                round_id,
                status.started,
                status.finished
            );
        }
        if status.address_observations != active.address_observations {
            anyhow::bail!(
                "active address observation histogram invariant failed: round_id={}, active={:?}, status={:?}",
                round_id,
                active.address_observations,
                status.address_observations
            );
        }
        if status.malformed_addresses != active.malformed_addresses {
            anyhow::bail!(
                "active malformed-address invariant failed: round_id={}, active={}, status={}",
                round_id,
                active.malformed_addresses,
                status.malformed_addresses
            );
        }
        if let Some(reason) = active.blocked_reason.as_ref() {
            anyhow::bail!(
                "cannot publish blocked crawl round: round_id={}, blocked_reason={}",
                round_id,
                reason
            );
        }
        let persisted_candidates: BTreeMap<Vec<u8>, CrawlCandidate> =
            self.scan_crawl_candidates()?.into_iter().collect();
        let mut submitted_candidate_ids = BTreeSet::new();
        for (peer_id, _) in candidate_puts.iter().chain(candidate_deletes) {
            if peer_id.is_empty() {
                anyhow::bail!(
                    "completed candidate has empty peer id: round_id={}",
                    round_id
                );
            }
            if !submitted_candidate_ids.insert(peer_id.clone()) {
                anyhow::bail!(
                    "completed candidate appears more than once: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
        }
        if let Some(peer_id) = persisted_candidates
            .keys()
            .find(|peer_id| !submitted_candidate_ids.contains(*peer_id))
        {
            anyhow::bail!(
                "candidate inventory omitted persisted candidate: round_id={}, peer_id=0x{}",
                round_id,
                bytes_to_hex(peer_id)
            );
        }
        if let Some(peer_id) = submitted_candidate_ids
            .iter()
            .find(|peer_id| !persisted_candidates.contains_key(*peer_id))
        {
            anyhow::bail!(
                "candidate inventory contains unpersisted candidate: round_id={}, peer_id=0x{}",
                round_id,
                bytes_to_hex(peer_id)
            );
        }
        let alias_to_peer = checked_candidate_alias_map(&persisted_candidates, round_id)?;

        let mut put_node_ids = HashSet::new();
        for (peer_id, _) in node_puts {
            if peer_id.is_empty() {
                anyhow::bail!("cannot publish node with empty peer id: round={}", round_id);
            }
            if !put_node_ids.insert(peer_id.as_slice()) {
                anyhow::bail!(
                    "cannot publish duplicate node: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
        }
        let mut deleted_node_ids = HashSet::new();
        for peer_id in node_deletes {
            if peer_id.is_empty() {
                anyhow::bail!("cannot delete node with empty peer id: round={}", round_id);
            }
            if !deleted_node_ids.insert(peer_id.as_slice()) {
                anyhow::bail!(
                    "cannot delete duplicate node: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
            if put_node_ids.contains(peer_id.as_slice()) {
                anyhow::bail!(
                    "node appears in both put and delete sets: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
        }

        let prior_nodes: BTreeMap<Vec<u8>, NodeRecord> = self.scan_nodes()?.into_iter().collect();
        let new_verified_peers = u64::try_from(
            node_puts
                .iter()
                .filter(|(peer_id, _)| !prior_nodes.contains_key(peer_id))
                .count(),
        )
        .map_err(|_| anyhow::anyhow!("new verified peer count exceeds u64"))?;
        if new_verified_peers != status.new_verified_peers {
            anyhow::bail!(
                "new verified peer invariant failed: round_id={}, node_delta={}, status={}",
                round_id,
                new_verified_peers,
                status.new_verified_peers
            );
        }
        let mut final_nodes = prior_nodes.clone();
        for peer_id in node_deletes {
            final_nodes.remove(peer_id);
        }
        for (peer_id, record) in node_puts {
            final_nodes.insert(peer_id.clone(), record.clone());
        }

        let mut peer_outcomes = CompletedPeerOutcomes::default();
        let mut address_observations = AddressObservationHistogram::default();
        for (peer_id, candidate, deleted) in candidate_puts
            .iter()
            .map(|(peer_id, candidate)| (peer_id, candidate, false))
            .chain(
                candidate_deletes
                    .iter()
                    .map(|(peer_id, candidate)| (peer_id, candidate, true)),
            )
        {
            if candidate.active.is_some() {
                anyhow::bail!(
                    "completed candidate still has active evidence: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
            let completed = candidate.last_completed.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "completed candidate is missing evidence: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                )
            })?;
            if completed.round_id != round_id {
                if !deleted || completed.round_id > round_id {
                    anyhow::bail!(
                        "completed candidate evidence round mismatch: round_id={}, peer_id=0x{}, evidence_round={}, deleted={}",
                        round_id,
                        bytes_to_hex(peer_id),
                        completed.round_id,
                        deleted
                    );
                }
                let prior_candidate = persisted_candidates.get(peer_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "deferred-prune candidate is missing from persisted inventory: round_id={}, peer_id=0x{}",
                        round_id,
                        bytes_to_hex(peer_id)
                    )
                })?;
                if candidate != prior_candidate {
                    anyhow::bail!(
                        "deferred-prune candidate evidence changed before deletion: round_id={}, peer_id=0x{}, evidence_round={}",
                        round_id,
                        bytes_to_hex(peer_id),
                        completed.round_id
                    );
                }
                if final_nodes.contains_key(peer_id) {
                    anyhow::bail!(
                        "cannot prune candidate evidence for a retained verified peer: round_id={}, peer_id=0x{}, evidence_round={}",
                        round_id,
                        bytes_to_hex(peer_id),
                        completed.round_id
                    );
                }
                continue;
            }
            if deleted {
                anyhow::bail!(
                    "cannot delete candidate evidence from the latest completed round: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
            let prior_candidate = persisted_candidates.get(peer_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "completed candidate is missing from persisted inventory: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                )
            })?;
            let prior_active = prior_candidate.active.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "completed candidate lacks durable active evidence: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                )
            })?;
            if prior_active.round_id != round_id {
                anyhow::bail!(
                    "durable candidate active round mismatch: round_id={}, peer_id=0x{}, candidate_round={}",
                    round_id,
                    bytes_to_hex(peer_id),
                    prior_active.round_id
                );
            }
            for evidence in &prior_active.observations {
                if evidence.observed_at < active.started_at
                    || evidence.observed_at > status.finished
                {
                    anyhow::bail!(
                        "candidate observation clock invariant failed: round_id={}, peer_id=0x{}, addr={}, observed_at={}, started_at={}, finished_at={}",
                        round_id,
                        bytes_to_hex(peer_id),
                        evidence.address,
                        evidence.observed_at,
                        active.started_at,
                        status.finished
                    );
                }
            }
            let expected_outcome = match prior_active.state {
                ActiveCandidateState::Succeeded => {
                    if prior_active.staged_success.is_none() {
                        anyhow::bail!(
                            "durable successful candidate lacks staged outcome: round_id={}, peer_id=0x{}",
                            round_id,
                            bytes_to_hex(peer_id)
                        );
                    }
                    CompletedCandidateOutcome::SameNetworkIdentified
                }
                ActiveCandidateState::Exhausted => CompletedCandidateOutcome::Exhausted,
                ActiveCandidateState::ForeignNetwork => CompletedCandidateOutcome::ForeignNetwork,
                state => {
                    anyhow::bail!(
                        "durable candidate is not terminal at publish: round_id={}, peer_id=0x{}, state={:?}",
                        round_id,
                        bytes_to_hex(peer_id),
                        state
                    );
                }
            };
            if expected_outcome != CompletedCandidateOutcome::SameNetworkIdentified
                && prior_active.staged_success.is_some()
            {
                anyhow::bail!(
                    "durable non-successful candidate retains staged outcome: round_id={}, peer_id=0x{}, state={:?}",
                    round_id,
                    bytes_to_hex(peer_id),
                    prior_active.state
                );
            }
            let expected_consecutive_exhausted_rounds = if expected_outcome
                == CompletedCandidateOutcome::Exhausted
            {
                match prior_candidate.last_completed.as_ref() {
                    Some(prior) if prior.outcome == CompletedCandidateOutcome::Exhausted => prior
                        .consecutive_exhausted_rounds
                        .checked_add(1)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "consecutive exhausted rounds overflow: round_id={}, peer_id=0x{}, prior={}",
                                round_id,
                                bytes_to_hex(peer_id),
                                prior.consecutive_exhausted_rounds
                            )
                        })?,
                    _ => 1,
                }
            } else {
                0
            };
            if completed.consecutive_exhausted_rounds != expected_consecutive_exhausted_rounds {
                anyhow::bail!(
                    "consecutive exhausted rounds invariant failed: round_id={}, peer_id=0x{}, expected={}, actual={}",
                    round_id,
                    bytes_to_hex(peer_id),
                    expected_consecutive_exhausted_rounds,
                    completed.consecutive_exhausted_rounds
                );
            }
            let expected_completed = CompletedCandidateEvidence {
                round_id,
                outcome: expected_outcome,
                observations: prior_active.observations.clone(),
                consecutive_exhausted_rounds: expected_consecutive_exhausted_rounds,
            };
            let mut expected_candidate = prior_candidate.clone();
            expected_candidate.active = None;
            expected_candidate.last_completed = Some(expected_completed.clone());
            if candidate != &expected_candidate {
                anyhow::bail!(
                    "candidate transition invariant failed: round_id={}, peer_id=0x{}",
                    round_id,
                    bytes_to_hex(peer_id)
                );
            }
            address_observations.checked_record_candidate(
                &prior_active.observations,
                &prior_candidate.addresses,
                expected_outcome,
                round_id,
                peer_id,
            )?;
            let retained = final_nodes.contains_key(peer_id);
            if retained
                && !prior_nodes.contains_key(peer_id)
                && expected_outcome != CompletedCandidateOutcome::SameNetworkIdentified
            {
                anyhow::bail!(
                    "new verified node lacks same-network evidence: round_id={}, peer_id=0x{}, outcome={:?}",
                    round_id,
                    bytes_to_hex(peer_id),
                    expected_outcome
                );
            }
            match expected_outcome {
                CompletedCandidateOutcome::SameNetworkIdentified => {
                    let staged = prior_active.staged_success.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "durable successful candidate lacks staged outcome: round_id={}, peer_id=0x{}",
                            round_id,
                            bytes_to_hex(peer_id)
                        )
                    })?;
                    let mut successful_observations =
                        prior_active.observations.iter().filter(|evidence| {
                            evidence.result == crate::AddressProbeResult::SameNetworkIdentified
                        });
                    let successful_observation = successful_observations.next().ok_or_else(|| {
                        anyhow::anyhow!(
                            "staged success has no same-network address evidence: round_id={}, peer_id=0x{}",
                            round_id,
                            bytes_to_hex(peer_id)
                        )
                    })?;
                    if successful_observations.next().is_some() {
                        anyhow::bail!(
                            "staged success has multiple same-network address observations: round_id={}, peer_id=0x{}",
                            round_id,
                            bytes_to_hex(peer_id)
                        );
                    }
                    if staged.observed_at != successful_observation.observed_at {
                        anyhow::bail!(
                            "staged success timestamp invariant failed: round_id={}, peer_id=0x{}, addr={}, evidence_observed_at={}, staged_observed_at={}",
                            round_id,
                            bytes_to_hex(peer_id),
                            successful_observation.address,
                            successful_observation.observed_at,
                            staged.observed_at
                        );
                    }
                    let record = final_nodes.get(peer_id).ok_or_else(|| {
                        anyhow::anyhow!(
                            "successful node transition invariant failed: round_id={}, peer_id=0x{}, reason=missing final node",
                            round_id,
                            bytes_to_hex(peer_id)
                        )
                    })?;
                    if let Some(prior) = prior_nodes.get(peer_id) {
                        if staged.observed_at < prior.last_seen {
                            anyhow::bail!(
                                "successful node observation time regressed: round_id={}, peer_id=0x{}, prior_last_seen={}, observed_at={}",
                                round_id,
                                bytes_to_hex(peer_id),
                                prior.last_seen,
                                staged.observed_at
                            );
                        }
                    }
                    let expected_first_seen = prior_nodes
                        .get(peer_id)
                        .map(|prior| prior.first_seen)
                        .unwrap_or(staged.observed_at);
                    let mut expected_own_addrs = prior_nodes
                        .get(peer_id)
                        .map(|prior| prior.own_addrs.clone())
                        .unwrap_or_default();
                    expected_own_addrs.extend(staged.own_addrs.iter().cloned());
                    expected_own_addrs.sort();
                    expected_own_addrs.dedup();
                    let expected_known_peers = checked_resolve_known_peers(
                        &staged.discovered_addrs,
                        &alias_to_peer,
                        round_id,
                        peer_id,
                    )?;
                    let matches_staged = record.own_addrs == expected_own_addrs
                        && record.client_version == staged.client_version
                        && record.flags == staged.flags
                        && record.protocols == staged.protocols
                        && record.first_seen == expected_first_seen
                        && record.last_seen == staged.observed_at
                        && record.last_reachable_at == staged.observed_at
                        && record.reachable
                        && record.last_rtt_ms == staged.rtt_ms
                        && record.discovery == staged.discovery
                        && record.known_peers == expected_known_peers;
                    if !matches_staged {
                        anyhow::bail!(
                            "successful node transition invariant failed: round_id={}, peer_id=0x{}",
                            round_id,
                            bytes_to_hex(peer_id)
                        );
                    }
                }
                CompletedCandidateOutcome::Exhausted
                | CompletedCandidateOutcome::ForeignNetwork => {
                    if let Some(record) = final_nodes.get(peer_id) {
                        let prior = prior_nodes.get(peer_id).ok_or_else(|| {
                            anyhow::anyhow!(
                                "unavailable node transition invariant failed: round_id={}, peer_id=0x{}, reason=missing prior node",
                                round_id,
                                bytes_to_hex(peer_id)
                            )
                        })?;
                        let mut expected = prior.clone();
                        expected.reachable = false;
                        if record != &expected {
                            anyhow::bail!(
                                "unavailable node transition invariant failed: round_id={}, peer_id=0x{}",
                                round_id,
                                bytes_to_hex(peer_id)
                            );
                        }
                    }
                }
            }
            if let Some(record) = final_nodes.get(peer_id) {
                let expected_reachable =
                    expected_outcome == CompletedCandidateOutcome::SameNetworkIdentified;
                if record.reachable != expected_reachable {
                    anyhow::bail!(
                        "candidate outcome disagrees with node reachability: round_id={}, peer_id=0x{}, outcome={:?}, reachable={}",
                        round_id,
                        bytes_to_hex(peer_id),
                        expected_outcome,
                        record.reachable
                    );
                }
            }
            peer_outcomes.checked_record(expected_outcome, retained, round_id, peer_id)?;
        }

        if peer_outcomes != status.peer_outcomes {
            anyhow::bail!(
                "peer outcome matrix invariant failed: round_id={}, candidate_evidence={:?}, status={:?}",
                round_id,
                peer_outcomes,
                status.peer_outcomes
            );
        }
        if address_observations != active.address_observations {
            anyhow::bail!(
                "durable candidate address observation histogram invariant failed: round_id={}, candidate_evidence={:?}, active={:?}",
                round_id,
                address_observations,
                active.address_observations
            );
        }
        if address_observations != status.address_observations {
            anyhow::bail!(
                "address observation histogram invariant failed: round_id={}, candidate_evidence={:?}, status={:?}",
                round_id,
                address_observations,
                status.address_observations
            );
        }
        let final_verified = u64::try_from(final_nodes.len())
            .map_err(|_| anyhow::anyhow!("verified peer count exceeds u64"))?;
        if peer_outcomes.verified_retained_peers(round_id)? != final_verified {
            anyhow::bail!(
                "verified peer snapshot invariant failed: round_id={}, matrix={}, nodes={}",
                round_id,
                peer_outcomes.verified_retained_peers(round_id)?,
                final_verified
            );
        }
        let reachable = u64::try_from(
            final_nodes
                .values()
                .filter(|record| record.reachable)
                .count(),
        )
        .map_err(|_| anyhow::anyhow!("reachable peer count exceeds u64"))?;
        if reachable != peer_outcomes.reachable_peers() {
            anyhow::bail!(
                "reachable peer snapshot invariant failed: round_id={}, matrix={}, nodes={}",
                round_id,
                peer_outcomes.reachable_peers(),
                reachable
            );
        }
        let mut discovery = DiscoveryEvidence::default();
        for record in final_nodes.values().filter(|record| record.reachable) {
            discovery.checked_add_assign(&record.discovery, round_id)?;
        }
        if discovery != status.discovery {
            anyhow::bail!(
                "Discovery evidence invariant failed: round_id={}, nodes={:?}, status={:?}",
                round_id,
                discovery,
                status.discovery
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(last_seen: u64) -> NodeRecord {
        NodeRecord {
            own_addrs: vec![],
            client_version: "0.119.0".into(),
            flags: 0,
            protocols: vec![],
            first_seen: 1,
            last_seen,
            last_reachable_at: last_seen,
            reachable: true,
            geo: None,
            asn: None,
            last_rtt_ms: None,
            discovery: Default::default(),
            known_peers: vec![],
        }
    }

    #[test]
    fn put_get_scan_delete_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let s = CkbadgerStore::open_test_network(dir.path()).unwrap();
        s.put_node(b"peerA", &rec(10)).unwrap();
        s.put_node(b"peerB", &rec(20)).unwrap();
        assert_eq!(s.get_node(b"peerA").unwrap().unwrap().last_seen, 10);
        assert_eq!(s.scan_nodes().unwrap().len(), 2);
        s.delete_node(b"peerA").unwrap();
        assert!(s.get_node(b"peerA").unwrap().is_none());
        assert_eq!(s.scan_nodes().unwrap().len(), 1);
    }

    #[test]
    fn prune_deletes_only_stale() {
        let dir = tempfile::tempdir().unwrap();
        let s = CkbadgerStore::open_test_network(dir.path()).unwrap();
        s.put_node(b"old", &rec(100)).unwrap();
        s.put_node(b"fresh", &rec(1000)).unwrap();
        let deleted = s.prune_nodes_older_than(500).unwrap();
        assert_eq!(deleted, 1);
        assert!(s.get_node(b"old").unwrap().is_none());
        assert!(s.get_node(b"fresh").unwrap().is_some());
    }

    #[test]
    fn status_singleton_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let s = CkbadgerStore::open_test_network(dir.path()).unwrap();
        assert!(s.get_network_status().unwrap().is_none());
        let st = crate::LatestStatus {
            round_id: 3,
            peer_outcomes: crate::CompletedPeerOutcomes {
                same_network_identified: 9,
                ..Default::default()
            },
            ..Default::default()
        };
        s.put_network_status(&st).unwrap();
        assert_eq!(s.get_network_status().unwrap().unwrap(), st);
    }

    #[test]
    fn history_range_scan_and_prune() {
        use crate::network_keys::{Granularity, Metric};
        use crate::HistoryPoint;
        let dir = tempfile::tempdir().unwrap();
        let s = CkbadgerStore::open_test_network(dir.path()).unwrap();
        for b in [10u64, 11, 12] {
            s.put_history_point(
                Metric::VerifiedPeers,
                Granularity::Hour,
                b,
                &HistoryPoint {
                    scalar: b,
                    buckets: vec![],
                },
            )
            .unwrap();
        }
        // Different metric must not leak into the scan.
        s.put_history_point(
            Metric::ReachablePeers,
            Granularity::Hour,
            11,
            &HistoryPoint {
                scalar: 999,
                buckets: vec![],
            },
        )
        .unwrap();
        let got = s
            .scan_history(Metric::VerifiedPeers, Granularity::Hour, 10, 11)
            .unwrap();
        assert_eq!(
            got,
            vec![
                (
                    10,
                    HistoryPoint {
                        scalar: 10,
                        buckets: vec![]
                    }
                ),
                (
                    11,
                    HistoryPoint {
                        scalar: 11,
                        buckets: vec![]
                    }
                ),
            ]
        );
        let pruned = s
            .prune_history_before(Metric::VerifiedPeers, Granularity::Hour, 12)
            .unwrap();
        assert_eq!(pruned, 2);
        assert_eq!(
            s.scan_history(Metric::VerifiedPeers, Granularity::Hour, 0, 100)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn crawl_checkpoint_and_round_commit_are_durable_and_atomic() {
        use crate::network_keys::{Granularity, Metric};
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressProbeEvidence, AddressProbeResult,
            CompletedCandidateEvidence, CompletedCandidateOutcome, CompletedPeerOutcomes,
            CrawlAddress, CrawlCandidate, StagedProbeOutcome,
        };

        let dir = tempfile::tempdir().unwrap();
        let s = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let mut active = ActiveCrawl {
            round_id: 1,
            started_at: 10,
            last_checkpoint_at: 10,
            ..Default::default()
        };
        let mut candidate = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        s.checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();
        let progress = s.get_crawl_progress().unwrap().unwrap();
        assert_eq!(progress.candidate_peers, 1);
        assert_eq!(progress.completed_peers, 0);

        active.address_observations.same_network_identified = 1;
        active.last_checkpoint_at = 11;
        candidate.active = Some(ActiveCandidateProbe {
            round_id: 1,
            state: ActiveCandidateState::Succeeded,
            observations: vec![AddressProbeEvidence {
                address: "addrA".into(),
                round_id: 1,
                observed_at: 11,
                elapsed_ms: 1,
                result: AddressProbeResult::SameNetworkIdentified,
            }],
            staged_success: Some(StagedProbeOutcome {
                observed_at: 11,
                client_version: "0.119.0".into(),
                ..Default::default()
            }),
        });
        s.checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();
        assert_eq!(s.get_crawl_progress().unwrap().unwrap().completed_peers, 1);

        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 12,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            address_observations: active.address_observations.clone(),
            new_verified_peers: 1,
            ..Default::default()
        };
        candidate.active = None;
        candidate.last_completed = Some(CompletedCandidateEvidence {
            round_id: 1,
            outcome: CompletedCandidateOutcome::SameNetworkIdentified,
            observations: vec![AddressProbeEvidence {
                address: "addrA".into(),
                round_id: 1,
                observed_at: 11,
                elapsed_ms: 1,
                result: AddressProbeResult::SameNetworkIdentified,
            }],
            consecutive_exhausted_rounds: 0,
        });
        let mut published = rec(11);
        published.first_seen = 11;
        let error = s
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), published.clone()), (Vec::new(), rec(11))],
                &[],
                &[(b"A".to_vec(), candidate.clone())],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();
        assert!(error.to_string().contains("empty peer id"));
        drop(s);

        let s = CkbadgerStore::open_test_network(dir.path()).unwrap();
        assert!(s.get_active_crawl().unwrap().is_some());
        assert!(s.get_network_status().unwrap().is_none());
        assert!(s.get_node(b"A").unwrap().is_none());
        s.commit_crawl_round(
            1,
            &[(b"A".to_vec(), published)],
            &[],
            &[(b"A".to_vec(), candidate.clone())],
            &[],
            &status,
            &[(
                Metric::VerifiedPeers,
                Granularity::Hour,
                0,
                HistoryPoint {
                    scalar: 1,
                    buckets: vec![],
                },
            )],
            &[],
        )
        .unwrap();
        drop(s);

        let reopened = CkbadgerStore::open_test_network(dir.path()).unwrap();
        assert!(reopened.get_active_crawl().unwrap().is_none());
        assert_eq!(reopened.get_network_status().unwrap().unwrap(), status);
        assert!(reopened.get_node(b"A").unwrap().is_some());
        let persisted = reopened.get_crawl_candidate(b"A").unwrap().unwrap();
        assert!(persisted.active.is_none());
        assert_eq!(persisted.last_completed.unwrap().round_id, 1);
        assert_eq!(
            reopened
                .scan_history(Metric::VerifiedPeers, Granularity::Hour, 0, 0)
                .unwrap()[0]
                .1
                .scalar,
            1
        );
    }

    #[test]
    fn active_progress_excludes_candidates_deferred_for_pruning() {
        use crate::{
            ActiveCandidateProbe, CompletedCandidateEvidence, CompletedCandidateOutcome,
            CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 2,
                    ..Default::default()
                },
                &[
                    (
                        b"active".to_vec(),
                        CrawlCandidate {
                            active: Some(ActiveCandidateProbe {
                                round_id: 2,
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ),
                    (
                        b"deferred".to_vec(),
                        CrawlCandidate {
                            last_completed: Some(CompletedCandidateEvidence {
                                round_id: 1,
                                outcome: CompletedCandidateOutcome::Exhausted,
                                observations: Vec::new(),
                                consecutive_exhausted_rounds: 1,
                            }),
                            ..Default::default()
                        },
                    ),
                ],
            )
            .unwrap();

        let progress = store.get_crawl_progress().unwrap().unwrap();
        assert_eq!(progress.candidate_peers, 1);
        assert_eq!(progress.completed_peers, 0);
        assert_eq!(store.scan_crawl_candidates().unwrap().len(), 2);
    }

    #[test]
    fn network_secondary_reads_active_and_published_state_across_all_three_cfs() {
        use crate::{
            ActiveCandidateProbe, AddressProbeEvidence, AddressProbeResult,
            CompletedCandidateEvidence, CompletedCandidateOutcome, CompletedPeerOutcomes,
            CrawlAddress, CrawlCandidate,
        };

        let primary_dir = tempfile::tempdir().unwrap();
        let secondary_dir = tempfile::tempdir().unwrap();
        let primary = CkbadgerStore::open_test_network(primary_dir.path()).unwrap();
        let mut active = ActiveCrawl {
            round_id: 1,
            started_at: 10,
            last_checkpoint_at: 11,
            ..Default::default()
        };
        let mut candidate = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        primary
            .checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();

        let secondary =
            CkbadgerStore::open_network_secondary(primary_dir.path(), secondary_dir.path())
                .unwrap();
        assert_eq!(secondary.get_active_crawl().unwrap(), Some(active.clone()));
        assert!(secondary.get_network_status().unwrap().is_none());

        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 11,
            elapsed_ms: 1,
            result: AddressProbeResult::SameNetworkIdentified,
        };
        active.address_observations.same_network_identified = 1;
        candidate.active = Some(ActiveCandidateProbe {
            round_id: 1,
            state: crate::ActiveCandidateState::Succeeded,
            observations: vec![observation.clone()],
            staged_success: Some(crate::StagedProbeOutcome {
                observed_at: 11,
                client_version: "0.119.0".into(),
                ..Default::default()
            }),
        });
        primary
            .checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();
        candidate.active = None;
        candidate.last_completed = Some(CompletedCandidateEvidence {
            round_id: 1,
            outcome: CompletedCandidateOutcome::SameNetworkIdentified,
            observations: vec![observation],
            consecutive_exhausted_rounds: 0,
        });
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 12,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            address_observations: crate::AddressObservationHistogram {
                same_network_identified: 1,
                ..Default::default()
            },
            new_verified_peers: 1,
            ..Default::default()
        };
        let mut published = rec(11);
        published.first_seen = 11;
        primary
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), published)],
                &[],
                &[(b"A".to_vec(), candidate)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap();

        secondary.refresh().unwrap();
        assert!(secondary.get_active_crawl().unwrap().is_none());
        assert_eq!(secondary.get_network_status().unwrap(), Some(status));
        assert!(secondary.get_node(b"A").unwrap().is_some());
    }

    #[test]
    fn completed_commit_rejects_status_that_disagrees_with_candidate_evidence() {
        use crate::{
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 10,
            elapsed_ms: 1,
            result: AddressProbeResult::SameNetworkIdentified,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![crate::CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 1,
                last_advertised_at: 1,
            }],
            active: Some(crate::ActiveCandidateProbe {
                round_id: 1,
                state: crate::ActiveCandidateState::Succeeded,
                observations: vec![observation.clone()],
                staged_success: Some(crate::StagedProbeOutcome {
                    observed_at: 10,
                    client_version: "0.119.0".into(),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let candidate = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::SameNetworkIdentified,
                observations: vec![observation],
                consecutive_exhausted_rounds: 0,
            }),
            ..checkpoint.clone()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint)],
            )
            .unwrap();
        let inconsistent = LatestStatus {
            round_id: 1,
            finished: 10,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            // Deliberately omits the candidate's successful address observation.
            address_observations: Default::default(),
            new_verified_peers: 1,
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[(
                    b"A".to_vec(),
                    NodeRecord {
                        first_seen: 10,
                        ..rec(10)
                    },
                )],
                &[],
                &[(b"A".to_vec(), candidate)],
                &[],
                &inconsistent,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("address observation histogram invariant"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_observation_for_an_unretained_alias() {
        use crate::{
            AddressObservationHistogram, AddressProbeEvidence, AddressProbeResult,
            CompletedCandidateEvidence, CompletedCandidateOutcome, CompletedPeerOutcomes,
            CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrB".into(),
            round_id: 1,
            observed_at: 10,
            elapsed_ms: 1,
            result: AddressProbeResult::SameNetworkIdentified,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 1,
                last_advertised_at: 1,
            }],
            active: Some(crate::ActiveCandidateProbe {
                round_id: 1,
                state: crate::ActiveCandidateState::Succeeded,
                observations: vec![observation.clone()],
                staged_success: Some(crate::StagedProbeOutcome {
                    observed_at: 10,
                    client_version: "0.119.0".into(),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let candidate = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::SameNetworkIdentified,
                observations: vec![observation],
                consecutive_exhausted_rounds: 0,
            }),
            ..checkpoint.clone()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    address_observations: AddressObservationHistogram {
                        same_network_identified: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint)],
            )
            .unwrap();
        let status = LatestStatus {
            round_id: 1,
            finished: 10,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                same_network_identified: 1,
                ..Default::default()
            },
            new_verified_peers: 1,
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), rec(10))],
                &[],
                &[(b"A".to_vec(), candidate)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error.to_string().contains("unknown alias"));
        assert!(error.to_string().contains("peer_id=0x41"));
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_incorrect_new_verified_peer_count() {
        use crate::{
            AddressObservationHistogram, AddressProbeEvidence, AddressProbeResult,
            CompletedCandidateEvidence, CompletedCandidateOutcome, CompletedPeerOutcomes,
            CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 10,
            elapsed_ms: 1,
            result: AddressProbeResult::SameNetworkIdentified,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 1,
                last_advertised_at: 1,
            }],
            active: Some(crate::ActiveCandidateProbe {
                round_id: 1,
                state: crate::ActiveCandidateState::Succeeded,
                observations: vec![observation.clone()],
                staged_success: Some(crate::StagedProbeOutcome {
                    observed_at: 10,
                    client_version: "0.119.0".into(),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let candidate = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::SameNetworkIdentified,
                observations: vec![observation],
                consecutive_exhausted_rounds: 0,
            }),
            ..checkpoint.clone()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    address_observations: AddressObservationHistogram {
                        same_network_identified: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint)],
            )
            .unwrap();
        let status = LatestStatus {
            round_id: 1,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                same_network_identified: 1,
                ..Default::default()
            },
            // One node is new in this commit, so zero is inconsistent.
            new_verified_peers: 0,
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), rec(10))],
                &[],
                &[(b"A".to_vec(), candidate)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error.to_string().contains("new verified peer invariant"));
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_an_omitted_persisted_candidate() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
        };

        fn active_exhausted(peer: &str) -> CrawlCandidate {
            let address = format!("addr{peer}");
            CrawlCandidate {
                addresses: vec![CrawlAddress {
                    addr: address.clone(),
                    first_advertised_at: 1,
                    last_advertised_at: 1,
                }],
                first_discovered_at: 1,
                last_advertised_at: 1,
                active: Some(ActiveCandidateProbe {
                    round_id: 1,
                    state: ActiveCandidateState::Exhausted,
                    observations: vec![AddressProbeEvidence {
                        address,
                        round_id: 1,
                        observed_at: 10,
                        elapsed_ms: 1,
                        result: AddressProbeResult::DialRequestFailed,
                    }],
                    staged_success: None,
                }),
                ..Default::default()
            }
        }

        fn completed(mut candidate: CrawlCandidate) -> CrawlCandidate {
            let active = candidate.active.take().unwrap();
            candidate.last_completed = Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: active.observations,
                consecutive_exhausted_rounds: 1,
            });
            candidate
        }

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let candidate_a = active_exhausted("A");
        let candidate_b = active_exhausted("B");
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    address_observations: AddressObservationHistogram {
                        dial_request_failed: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &[
                    (b"A".to_vec(), candidate_a.clone()),
                    (b"B".to_vec(), candidate_b),
                ],
            )
            .unwrap();
        let status = LatestStatus {
            round_id: 1,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_without_retained_verification: 1,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                dial_request_failed: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[],
                &[],
                &[(b"A".to_vec(), completed(candidate_a))],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error.to_string().contains("candidate inventory"));
        assert!(error.to_string().contains("peer_id=0x42"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store
            .get_crawl_candidate(b"B")
            .unwrap()
            .unwrap()
            .active
            .is_some());
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_an_unpersisted_extra_candidate() {
        use crate::{
            AddressObservationHistogram, AddressProbeEvidence, AddressProbeResult,
            CompletedCandidateEvidence, CompletedCandidateOutcome, CompletedPeerOutcomes,
            CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    address_observations: AddressObservationHistogram {
                        dial_request_failed: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &[],
            )
            .unwrap();
        let candidate = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 1,
                last_advertised_at: 1,
            }],
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![AddressProbeEvidence {
                    address: "addrA".into(),
                    round_id: 1,
                    observed_at: 10,
                    elapsed_ms: 1,
                    result: AddressProbeResult::DialRequestFailed,
                }],
                consecutive_exhausted_rounds: 1,
            }),
            ..Default::default()
        };
        let status = LatestStatus {
            round_id: 1,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_without_retained_verification: 1,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                dial_request_failed: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[],
                &[],
                &[(b"A".to_vec(), candidate)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error.to_string().contains("candidate inventory"));
        assert!(error.to_string().contains("peer_id=0x41"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_incorrect_consecutive_exhausted_rounds() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 2,
            observed_at: 20,
            elapsed_ms: 1,
            result: AddressProbeResult::DialRequestFailed,
        };
        let prior = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 1,
                last_advertised_at: 20,
            }],
            first_discovered_at: 1,
            last_advertised_at: 20,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![],
                consecutive_exhausted_rounds: 2,
            }),
            active: Some(ActiveCandidateProbe {
                round_id: 2,
                state: ActiveCandidateState::Exhausted,
                observations: vec![observation.clone()],
                staged_success: None,
            }),
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 2,
                    address_observations: AddressObservationHistogram {
                        dial_request_failed: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &[(b"A".to_vec(), prior.clone())],
            )
            .unwrap();
        let submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 2,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![observation],
                consecutive_exhausted_rounds: 99,
            }),
            ..prior
        };
        let status = LatestStatus {
            round_id: 2,
            finished: 20,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_without_retained_verification: 1,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                dial_request_failed: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                2,
                &[],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("consecutive exhausted rounds invariant"));
        assert!(message.contains("round_id=2"));
        assert!(message.contains("peer_id=0x41"));
        assert!(message.contains("expected=3"));
        assert!(message.contains("actual=99"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_candidate_transition_tampering() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 11,
            elapsed_ms: 1,
            result: AddressProbeResult::DialRequestFailed,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                state: ActiveCandidateState::Exhausted,
                observations: vec![observation.clone()],
                staged_success: None,
            }),
            ..Default::default()
        };
        let histogram = AddressObservationHistogram {
            dial_request_failed: 1,
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 10,
                    last_checkpoint_at: 11,
                    address_observations: histogram.clone(),
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint.clone())],
            )
            .unwrap();
        let mut submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![observation],
                consecutive_exhausted_rounds: 1,
            }),
            ..checkpoint
        };
        submitted.first_discovered_at = 999;
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 11,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_without_retained_verification: 1,
                ..Default::default()
            },
            address_observations: histogram,
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error.to_string().contains("candidate transition invariant"));
        assert!(error.to_string().contains("peer_id=0x41"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_active_histogram_mismatch() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 11,
            elapsed_ms: 1,
            result: AddressProbeResult::DialRequestFailed,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                state: ActiveCandidateState::Exhausted,
                observations: vec![observation.clone()],
                staged_success: None,
            }),
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 10,
                    last_checkpoint_at: 11,
                    // Deliberately omits the persisted candidate observation.
                    address_observations: AddressObservationHistogram::default(),
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint.clone())],
            )
            .unwrap();
        let submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![observation],
                consecutive_exhausted_rounds: 1,
            }),
            ..checkpoint
        };
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 11,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_without_retained_verification: 1,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                dial_request_failed: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("active address observation histogram invariant"));
        assert!(store.get_active_crawl().unwrap().is_some());
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_successful_node_transition_tampering() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
            StagedProbeOutcome,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 11,
            elapsed_ms: 1,
            result: AddressProbeResult::SameNetworkIdentified,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                state: ActiveCandidateState::Succeeded,
                observations: vec![observation.clone()],
                staged_success: Some(StagedProbeOutcome {
                    observed_at: 11,
                    client_version: "expected-client".into(),
                    flags: 7,
                    protocols: vec!["identify".into()],
                    own_addrs: vec!["ownA".into()],
                    rtt_ms: Some(1),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let histogram = AddressObservationHistogram {
            same_network_identified: 1,
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 10,
                    last_checkpoint_at: 11,
                    address_observations: histogram.clone(),
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint.clone())],
            )
            .unwrap();
        let submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::SameNetworkIdentified,
                observations: vec![observation],
                consecutive_exhausted_rounds: 0,
            }),
            ..checkpoint
        };
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 11,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            address_observations: histogram,
            new_verified_peers: 1,
            ..Default::default()
        };
        let mut tampered = rec(11);
        tampered.first_seen = 11;
        tampered.own_addrs = vec!["ownA".into()];
        tampered.client_version = "tampered-client".into();
        tampered.flags = 7;
        tampered.protocols = vec!["identify".into()];
        tampered.last_rtt_ms = Some(1);

        let error = store
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), tampered)],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("successful node transition invariant"));
        assert!(error.to_string().contains("peer_id=0x41"));
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_staged_success_timestamp_diverging_from_evidence() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
            StagedProbeOutcome,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 11,
            elapsed_ms: 1,
            result: AddressProbeResult::SameNetworkIdentified,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                state: ActiveCandidateState::Succeeded,
                observations: vec![observation.clone()],
                staged_success: Some(StagedProbeOutcome {
                    observed_at: 12,
                    client_version: "0.119.0".into(),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let histogram = AddressObservationHistogram {
            same_network_identified: 1,
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 10,
                    last_checkpoint_at: 12,
                    address_observations: histogram.clone(),
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint.clone())],
            )
            .unwrap();
        let submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::SameNetworkIdentified,
                observations: vec![observation],
                consecutive_exhausted_rounds: 0,
            }),
            ..checkpoint
        };
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 12,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 1,
                ..Default::default()
            },
            address_observations: histogram,
            new_verified_peers: 1,
            ..Default::default()
        };
        let mut node = rec(12);
        node.first_seen = 12;

        let error = store
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), node)],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("staged success timestamp invariant"));
        assert!(error.to_string().contains("evidence_observed_at=11"));
        assert!(error.to_string().contains("staged_observed_at=12"));
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_observation_outside_round_clock() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 9,
            elapsed_ms: 1,
            result: AddressProbeResult::DialRequestFailed,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 8,
                last_advertised_at: 8,
            }],
            first_discovered_at: 8,
            last_advertised_at: 8,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                state: ActiveCandidateState::Exhausted,
                observations: vec![observation.clone()],
                staged_success: None,
            }),
            ..Default::default()
        };
        let histogram = AddressObservationHistogram {
            dial_request_failed: 1,
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 10,
                    last_checkpoint_at: 11,
                    address_observations: histogram.clone(),
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint.clone())],
            )
            .unwrap();
        let submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![observation],
                consecutive_exhausted_rounds: 1,
            }),
            ..checkpoint
        };
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 11,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_without_retained_verification: 1,
                ..Default::default()
            },
            address_observations: histogram,
            ..Default::default()
        };

        let error = store
            .commit_crawl_round(
                1,
                &[],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("candidate observation clock invariant"));
        assert!(message.contains("peer_id=0x41"));
        assert!(message.contains("addr=addrA"));
        assert!(message.contains("observed_at=9"));
        assert!(message.contains("started_at=10"));
        assert!(store.get_network_status().unwrap().is_none());
    }

    #[test]
    fn completed_commit_rejects_unavailable_node_transition_tampering() {
        use crate::{
            ActiveCandidateProbe, ActiveCandidateState, AddressObservationHistogram,
            AddressProbeEvidence, AddressProbeResult, CompletedCandidateEvidence,
            CompletedCandidateOutcome, CompletedPeerOutcomes, CrawlAddress, CrawlCandidate,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_network(dir.path()).unwrap();
        let prior_node = rec(9);
        store.put_node(b"A", &prior_node).unwrap();
        let observation = AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 1,
            observed_at: 11,
            elapsed_ms: 1,
            result: AddressProbeResult::DialRequestFailed,
        };
        let checkpoint = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "addrA".into(),
                first_advertised_at: 10,
                last_advertised_at: 10,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            active: Some(ActiveCandidateProbe {
                round_id: 1,
                state: ActiveCandidateState::Exhausted,
                observations: vec![observation.clone()],
                staged_success: None,
            }),
            ..Default::default()
        };
        let histogram = AddressObservationHistogram {
            dial_request_failed: 1,
            ..Default::default()
        };
        store
            .checkpoint_crawl(
                &ActiveCrawl {
                    round_id: 1,
                    started_at: 10,
                    last_checkpoint_at: 11,
                    address_observations: histogram.clone(),
                    ..Default::default()
                },
                &[(b"A".to_vec(), checkpoint.clone())],
            )
            .unwrap();
        let submitted = CrawlCandidate {
            active: None,
            last_completed: Some(CompletedCandidateEvidence {
                round_id: 1,
                outcome: CompletedCandidateOutcome::Exhausted,
                observations: vec![observation],
                consecutive_exhausted_rounds: 1,
            }),
            ..checkpoint
        };
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 11,
            peer_outcomes: CompletedPeerOutcomes {
                exhausted_with_retained_verification: 1,
                ..Default::default()
            },
            address_observations: histogram,
            ..Default::default()
        };
        let mut tampered = prior_node;
        tampered.reachable = false;
        tampered.client_version = "tampered-client".into();

        let error = store
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), tampered)],
                &[],
                &[(b"A".to_vec(), submitted)],
                &[],
                &status,
                &[],
                &[],
            )
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("unavailable node transition invariant"));
        assert!(error.to_string().contains("peer_id=0x41"));
        assert!(store.get_network_status().unwrap().is_none());
    }
}
