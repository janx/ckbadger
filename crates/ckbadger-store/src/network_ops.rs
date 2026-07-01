//! Network-store node and stats operations (`CF_NET_NODES`, `CF_NET_STATS`).
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
//! The crawler is the sole writer at low frequency, so these use direct
//! `put_cf`/`delete_cf` writes rather than a [`crate::StoreBatch`].

use crate::network_keys::{history_key, history_prefix, Granularity, Metric, STATS_STATUS_KEY};
use crate::store::{CkbadgerStore, CF_NET_NODES, CF_NET_STATS};
use crate::{bytes_to_hex, HistoryPoint, LatestStatus, NodeRecord};

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
            frontier_drained: true,
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
}
