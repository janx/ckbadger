//! Network-store node operations (`CF_NET_NODES`).
//!
//! Node records are keyed by raw `peer_id` bytes and hold the crawler's view of
//! a peer (addresses, client version, reachability, geo/asn, sampled known
//! peers). The crawler is the sole writer at low frequency, so these use direct
//! `put_cf`/`delete_cf` writes rather than a [`crate::StoreBatch`].

use crate::bytes_to_hex;
use crate::store::{CkbadgerStore, CF_NET_NODES};
use crate::NodeRecord;

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
}
