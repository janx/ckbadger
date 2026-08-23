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

use crate::network_keys::{
    crawl_candidate_key, history_key, history_prefix, Granularity, Metric, CRAWL_ACTIVE_KEY,
    CRAWL_CANDIDATE_PREFIX, STATS_STATUS_KEY,
};
use crate::store::{CkbadgerStore, CF_NET_CRAWL, CF_NET_NODES, CF_NET_STATS};
use crate::{
    bytes_to_hex, ActiveCrawl, CrawlCandidate, CrawlCandidateResult, CrawlProgress, HistoryPoint,
    LatestStatus, NodeRecord,
};

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
                deleted += 1;
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
                deleted += 1;
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
        let candidate_peers = candidates.len() as u64;
        let completed_peers = candidates
            .iter()
            .filter(|(_, candidate)| {
                candidate.round_id == active.round_id
                    && matches!(
                        candidate.result,
                        CrawlCandidateResult::Succeeded
                            | CrawlCandidateResult::Exhausted
                            | CrawlCandidateResult::ForeignNetwork
                    )
            })
            .count() as u64;
        Ok(Some(CrawlProgress {
            round_id: active.round_id,
            started_at: active.started_at,
            last_checkpoint_at: active.last_checkpoint_at,
            candidate_peers,
            completed_peers,
            address_attempts: active.address_attempts,
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
        candidate_deletes: &[Vec<u8>],
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
        for peer_id in candidate_deletes {
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
            total_known: 9,
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
                Metric::TotalNodes,
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
            Metric::ReachableNodes,
            Granularity::Hour,
            11,
            &HistoryPoint {
                scalar: 999,
                buckets: vec![],
            },
        )
        .unwrap();
        let got = s
            .scan_history(Metric::TotalNodes, Granularity::Hour, 10, 11)
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
            .prune_history_before(Metric::TotalNodes, Granularity::Hour, 12)
            .unwrap();
        assert_eq!(pruned, 2);
        assert_eq!(
            s.scan_history(Metric::TotalNodes, Granularity::Hour, 0, 100)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn crawl_checkpoint_and_round_commit_are_durable_and_atomic() {
        use crate::network_keys::{Granularity, Metric};
        use crate::{CrawlAddress, CrawlCandidate, CrawlCandidateResult, StagedProbeOutcome};

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
                last_advertised_at: 10,
                attempted_round: 0,
            }],
            first_discovered_at: 10,
            last_advertised_at: 10,
            round_id: 1,
            ..Default::default()
        };
        s.checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();
        let progress = s.get_crawl_progress().unwrap().unwrap();
        assert_eq!(progress.candidate_peers, 1);
        assert_eq!(progress.completed_peers, 0);

        active.address_attempts = 1;
        active.last_checkpoint_at = 11;
        candidate.addresses[0].attempted_round = 1;
        candidate.result = CrawlCandidateResult::Succeeded;
        candidate.staged_success = Some(StagedProbeOutcome {
            observed_at: 11,
            client_version: "0.119.0".into(),
            ..Default::default()
        });
        s.checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();
        assert_eq!(s.get_crawl_progress().unwrap().unwrap().completed_peers, 1);

        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 12,
            candidate_peers: 1,
            attempted_peers: 1,
            reachable_peers: 1,
            new_nodes: 1,
            total_known: 1,
            address_attempts: 1,
            ..Default::default()
        };
        candidate.staged_success = None;
        let error = s
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), rec(11)), (Vec::new(), rec(11))],
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
            &[(b"A".to_vec(), rec(11))],
            &[],
            &[(b"A".to_vec(), candidate.clone())],
            &[],
            &status,
            &[(
                Metric::TotalNodes,
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
        assert_eq!(
            reopened
                .get_crawl_candidate(b"A")
                .unwrap()
                .unwrap()
                .staged_success,
            None
        );
        assert_eq!(
            reopened
                .scan_history(Metric::TotalNodes, Granularity::Hour, 0, 0)
                .unwrap()[0]
                .1
                .scalar,
            1
        );
    }

    #[test]
    fn network_secondary_reads_active_and_published_state_across_all_three_cfs() {
        use crate::{CrawlCandidate, CrawlCandidateResult};

        let primary_dir = tempfile::tempdir().unwrap();
        let secondary_dir = tempfile::tempdir().unwrap();
        let primary = CkbadgerStore::open_test_network(primary_dir.path()).unwrap();
        let active = ActiveCrawl {
            round_id: 1,
            started_at: 10,
            last_checkpoint_at: 11,
            ..Default::default()
        };
        let mut candidate = CrawlCandidate {
            first_discovered_at: 10,
            last_advertised_at: 10,
            round_id: 1,
            ..Default::default()
        };
        primary
            .checkpoint_crawl(&active, &[(b"A".to_vec(), candidate.clone())])
            .unwrap();

        let secondary =
            CkbadgerStore::open_network_secondary(primary_dir.path(), secondary_dir.path())
                .unwrap();
        assert_eq!(secondary.get_active_crawl().unwrap(), Some(active));
        assert!(secondary.get_network_status().unwrap().is_none());

        candidate.result = CrawlCandidateResult::Succeeded;
        let status = LatestStatus {
            round_id: 1,
            started: 10,
            finished: 12,
            candidate_peers: 1,
            attempted_peers: 1,
            reachable_peers: 1,
            address_attempts: 1,
            new_nodes: 1,
            total_known: 1,
            ..Default::default()
        };
        primary
            .commit_crawl_round(
                1,
                &[(b"A".to_vec(), rec(12))],
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
}
