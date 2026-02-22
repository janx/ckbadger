#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardforkResource {
    pub label: &'static str,
    pub url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardforkSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub short_name: &'static str,
    pub edition_year: i32,
    pub activation_epoch: i64,
    pub activation_date: &'static str,
    pub summary: &'static str,
    pub resources: &'static [HardforkResource],
}

pub const NETWORK_MAINNET: &str = "mainnet";
pub const NETWORK_TESTNET: &str = "testnet";

const MIRANA_RESOURCES: &[HardforkResource] = &[
    HardforkResource {
        label: "CKB2021",
        url: "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0037-ckb2021/0037-ckb2021.md",
    },
    HardforkResource {
        label: "Migration Guide",
        url: "https://github.com/jordanmack/nervos-ckb2021-hard-fork-migration-guide",
    },
];

const MEEPO_RESOURCES: &[HardforkResource] = &[
    HardforkResource {
        label: "CKB2023",
        url: "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0051-ckb2023/0051-ckb2023.md",
    },
    HardforkResource {
        label: "RFC49 VM V2",
        url: "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0049-ckb-vm-version-2/0049-ckb-vm-version-2.md",
    },
    HardforkResource {
        label: "RFC50 Spawn",
        url: "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0050-vm-syscalls-3/0050-vm-syscalls-3.md",
    },
];

const MAINNET_HARDFORKS: &[HardforkSpec] = &[
    HardforkSpec {
        id: "mirana-2021",
        name: "CKB Edition Mirana",
        short_name: "Mirana",
        edition_year: 2021,
        activation_epoch: 5414,
        activation_date: "2022-05-10",
        summary: "CKB-VM v1 activation, extension field, and consensus patch bundle.",
        resources: MIRANA_RESOURCES,
    },
    HardforkSpec {
        id: "meepo-2024",
        name: "CKB Edition Meepo",
        short_name: "Meepo",
        edition_year: 2024,
        activation_epoch: 12293,
        activation_date: "2025-07-01",
        summary: "CKB-VM v2 activation with Spawn syscall and hash_type data2 support.",
        resources: MEEPO_RESOURCES,
    },
];

const TESTNET_HARDFORKS: &[HardforkSpec] = &[
    HardforkSpec {
        id: "mirana-2021",
        name: "CKB Edition Mirana",
        short_name: "Mirana",
        edition_year: 2021,
        activation_epoch: 3113,
        activation_date: "2021-11-24",
        summary: "CKB-VM v1 activation, extension field, and consensus patch bundle.",
        resources: MIRANA_RESOURCES,
    },
    HardforkSpec {
        id: "meepo-2024",
        name: "CKB Edition Meepo",
        short_name: "Meepo",
        edition_year: 2024,
        activation_epoch: 9690,
        activation_date: "2024-10-25",
        summary: "CKB-VM v2 activation with Spawn syscall and hash_type data2 support.",
        resources: MEEPO_RESOURCES,
    },
];

pub fn normalize_network(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "mainnet" | "ckb" => Some(NETWORK_MAINNET),
        "testnet" | "pudge" => Some(NETWORK_TESTNET),
        _ => None,
    }
}

pub fn hardforks_for_network(network: &str) -> Option<&'static [HardforkSpec]> {
    match normalize_network(network) {
        Some(NETWORK_MAINNET) => Some(MAINNET_HARDFORKS),
        Some(NETWORK_TESTNET) => Some(TESTNET_HARDFORKS),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{hardforks_for_network, normalize_network};

    #[test]
    fn normalize_network_aliases() {
        assert_eq!(normalize_network("mainnet"), Some("mainnet"));
        assert_eq!(normalize_network(" CKB "), Some("mainnet"));
        assert_eq!(normalize_network("testnet"), Some("testnet"));
        assert_eq!(normalize_network("pudge"), Some("testnet"));
        assert_eq!(normalize_network("devnet"), None);
    }

    #[test]
    fn hardfork_list_is_epoch_sorted() {
        let events = hardforks_for_network("mainnet").expect("mainnet hardfork list");
        assert!(events.len() >= 2);
        assert!(events
            .windows(2)
            .all(|w| w[0].activation_epoch < w[1].activation_epoch));
    }
}
