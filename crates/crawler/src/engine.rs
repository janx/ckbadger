use std::collections::HashMap;
use std::net::IpAddr;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
}
