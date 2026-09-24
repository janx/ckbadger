mod activities;
mod assets;
mod blocks;
mod cells;
mod dao;
mod fiber;
mod forks;
mod graph;
mod hardforks;
mod heavy_pages;
mod identities;
mod mempool;
mod scripts;
mod search;
mod spore;
mod statistics;
mod tokens;
mod transactions;

use crate::registry::Registry;

pub fn register_all() -> Registry {
    let mut reg = Registry::new();
    for entry in activities::entries() {
        reg.add(entry);
    }
    for entry in assets::entries() {
        reg.add(entry);
    }
    for entry in blocks::entries() {
        reg.add(entry);
    }
    for entry in cells::entries() {
        reg.add(entry);
    }
    for entry in dao::entries() {
        reg.add(entry);
    }
    for entry in fiber::entries() {
        reg.add(entry);
    }
    for entry in forks::entries() {
        reg.add(entry);
    }
    for entry in graph::entries() {
        reg.add(entry);
    }
    for entry in hardforks::entries() {
        reg.add(entry);
    }
    for entry in heavy_pages::entries() {
        reg.add(entry);
    }
    for entry in identities::entries() {
        reg.add(entry);
    }
    for entry in mempool::entries() {
        reg.add(entry);
    }
    for entry in scripts::entries() {
        reg.add(entry);
    }
    for entry in search::entries() {
        reg.add(entry);
    }
    for entry in spore::entries() {
        reg.add(entry);
    }
    for entry in statistics::entries() {
        reg.add(entry);
    }
    for entry in tokens::entries() {
        reg.add(entry);
    }
    for entry in transactions::entries() {
        reg.add(entry);
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::register_all;
    use crate::registry::{DiscoveredParams, Method};

    const BASE: &str = "http://127.0.0.1:8101/api/v1";
    const DOTCELL_ID: &str = "0x0102030405060708090a0b0c0d0e0f1011121314";
    const LOCK_HASH: &str = "0xabababababababababababababababababababababababababababababababab";

    /// Resolve the one GET entry registered for `template`.
    fn resolve(template: &str, params: &DiscoveredParams) -> Option<String> {
        let registry = register_all();
        let entry = registry
            .entries
            .iter()
            .find(|entry| entry.path_template == template)
            .unwrap_or_else(|| panic!("{template} is not in the bench endpoint list"));
        assert_eq!(entry.method, Method::Get, "{template}");
        (entry.resolve)(BASE, params).map(|request| request.url)
    }

    fn with_dotcell_name() -> DiscoveredParams {
        DiscoveredParams {
            dotcell_item_id: Some(DOTCELL_ID.to_string()),
            ..Default::default()
        }
    }

    fn with_top_lock_hash() -> DiscoveredParams {
        DiscoveredParams {
            top_lock_hashes: vec![LOCK_HASH.to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn dotcell_name_endpoints_resolve_from_the_discovered_name() {
        let params = with_dotcell_name();
        assert_eq!(
            resolve("/assets/identities/dotcell/ring", &params).as_deref(),
            Some(format!("{BASE}/assets/identities/dotcell/ring").as_str())
        );
        assert_eq!(
            resolve(
                "/assets/identities/dotcell/items/{id_or_name}/children",
                &params
            )
            .as_deref(),
            Some(
                format!("{BASE}/assets/identities/dotcell/items/{DOTCELL_ID}/children?limit=20")
                    .as_str()
            )
        );
    }

    /// A network with no `.cell` deployment has no name to discover, and no
    /// ring root either: its `.cell` entries are skipped, not failed.
    #[test]
    fn dotcell_name_endpoints_are_skipped_without_a_dotcell_deployment() {
        let params = DiscoveredParams::default();
        assert_eq!(resolve("/assets/identities/dotcell/ring", &params), None);
        assert_eq!(
            resolve(
                "/assets/identities/dotcell/items/{id_or_name}/children",
                &params
            ),
            None
        );
    }

    #[test]
    fn address_dotcell_endpoints_resolve_from_the_top_lock_hash() {
        let params = with_top_lock_hash();
        assert_eq!(
            resolve("/addresses/{addr}/dotcell-names", &params).as_deref(),
            Some(format!("{BASE}/addresses/{LOCK_HASH}/dotcell-names?limit=20").as_str())
        );
        // The prefix is the lock hash's first 20 bytes: `0x` + 40 hex.
        assert_eq!(
            resolve("/addresses/prefix/{prefix}/transactions", &params).as_deref(),
            Some(
                format!(
                    "{BASE}/addresses/prefix/{}/transactions?limit=20",
                    &LOCK_HASH[..42]
                )
                .as_str()
            )
        );
        assert_eq!(
            resolve(
                "/addresses/prefix/{prefix}/transactions",
                &DiscoveredParams::default()
            ),
            None
        );
    }
}
