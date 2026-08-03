//! Data-driven protocol-script detection, built once from the bundled label
//! registry (`docs/metadata/scripts/*.toml`). Network-agnostic union: a
//! code_hash uniquely identifies a protocol across networks (no collisions).

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::label_import::bundled::script_labels;
use crate::label_import::ImportDeployment;
use crate::rpc::parse_hex_to_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolScript {
    Dao,
    Sudt,
    Xudt,
    SporeNft,
    BitCell,
    DidCkb,
    // Retained for the legacy Spore identity writer. No bundled metadata slug
    // currently maps to this variant; `.bit Cell` and `did:ckb` are distinct
    // protocols and must never enter this path.
    SporeDid,
    Cluster,
    MnftIssuer,
    MnftClass,
    MnftToken,
    RgbppLock,
    BtcTimeLock,
    DotbitAccount,
    FiberFunding,
    FiberCommitment,
    StablePpAsset,
    StablePpPool,
    StablePpIntent,
    StablePpVault,
    UtxoSwapIntent,
}

/// Map a registry file's `metadata_slug` (file stem) to a protocol identity.
/// Slugs not listed here are labels-only scripts with no parser detection.
fn slug_to_protocol(slug: &str) -> Option<ProtocolScript> {
    Some(match slug {
        "nervos-dao" => ProtocolScript::Dao,
        "simple-udt" => ProtocolScript::Sudt,
        "xudt" => ProtocolScript::Xudt,
        "spore" => ProtocolScript::SporeNft,
        "bit-cell" => ProtocolScript::BitCell,
        "did-ckb" => ProtocolScript::DidCkb,
        "spore-cluster" => ProtocolScript::Cluster,
        "m-nft-issuer" => ProtocolScript::MnftIssuer,
        "m-nft-class" => ProtocolScript::MnftClass,
        "m-nft" => ProtocolScript::MnftToken,
        "rgb" => ProtocolScript::RgbppLock,
        "btc-time-lock" => ProtocolScript::BtcTimeLock,
        "bit-account" => ProtocolScript::DotbitAccount,
        "fiber-funding-lock" => ProtocolScript::FiberFunding,
        "fiber-commitment-lock" => ProtocolScript::FiberCommitment,
        "stable-asset" => ProtocolScript::StablePpAsset,
        "stable-pool" => ProtocolScript::StablePpPool,
        "stable-intent-lock" => ProtocolScript::StablePpIntent,
        "stable-vault-lock" => ProtocolScript::StablePpVault,
        "utxoswap-intent-lock" => ProtocolScript::UtxoSwapIntent,
        _ => return None,
    })
}

pub struct ProtocolRegistry {
    pub(crate) by_code_hash: HashMap<Vec<u8>, ProtocolScript>,
}

impl ProtocolRegistry {
    fn from_bundled() -> Self {
        let mut by_code_hash = HashMap::new();
        for meta in script_labels() {
            let Some(slug) = meta.metadata_slug.as_deref() else {
                continue;
            };
            let Some(protocol) = slug_to_protocol(slug) else {
                continue;
            };
            for net in [meta.mainnet.as_ref(), meta.testnet.as_ref()]
                .into_iter()
                .flatten()
            {
                for dep in net.import_deployments() {
                    // Both shapes reference the on-chain script code_hash cell:
                    // Version → canonical_ref_hash, Pseudo → code_hash.
                    let code_hash_hex = match &dep {
                        ImportDeployment::Version(version) => &version.canonical_ref_hash,
                        ImportDeployment::Pseudo(pseudo) => &pseudo.code_hash,
                    };
                    let bytes = parse_hex_to_bytes(code_hash_hex);
                    // Fail-fast: the network-agnostic union premise is that a code_hash
                    // uniquely identifies one protocol. Same-protocol duplicates are
                    // idempotent; a DIFFERENT protocol on the same key is a data bug.
                    if let Some(prev) = by_code_hash.insert(bytes.clone(), protocol) {
                        assert_eq!(
                            prev, protocol,
                            "protocol registry code_hash collision: 0x{} maps to both {prev:?} and {protocol:?} — two scripts/*.toml share a canonical_ref_hash",
                            hex::encode(&bytes)
                        );
                    }
                }
            }
        }
        // Fail-fast: required-core protocols must be present.
        for required in [
            ProtocolScript::Dao,
            ProtocolScript::Sudt,
            ProtocolScript::Xudt,
        ] {
            assert!(
                by_code_hash.values().any(|p| *p == required),
                "protocol registry missing required-core protocol {required:?} — check docs/metadata/scripts"
            );
        }
        Self { by_code_hash }
    }

    pub fn get(&self, code_hash: &[u8]) -> Option<ProtocolScript> {
        self.by_code_hash.get(code_hash).copied()
    }

    pub fn is(&self, code_hash: &[u8], script: ProtocolScript) -> bool {
        self.by_code_hash.get(code_hash) == Some(&script)
    }

    /// Iterate every `(code_hash, protocol)` entry in the registry.
    ///
    /// The `ProtocolScript` is yielded by value (it is `Copy`); the code_hash
    /// is borrowed. Used by consumers that need to build their own derived
    /// lookup (e.g. the activity builder's `code_hash → AssetKind` map).
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, ProtocolScript)> + '_ {
        self.by_code_hash
            .iter()
            .map(|(code_hash, p)| (code_hash, *p))
    }
}

pub static PROTOCOL_REGISTRY: LazyLock<ProtocolRegistry> =
    LazyLock::new(ProtocolRegistry::from_bundled);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn detects_mainnet_and_testnet_hashes() {
        let r = &*PROTOCOL_REGISTRY;
        // sUDT: mainnet AND testnet both map to Sudt (the testnet fix this plan delivers)
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5"
            )),
            Some(ProtocolScript::Sudt)
        );
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4"
            )),
            Some(ProtocolScript::Sudt)
        );
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x48dbf59b4c7ee1547238021b4869bceedf4eea6b43772e5d66ef8865b6ae7212"
            )),
            Some(ProtocolScript::Sudt)
        );
        // `.bit Cell` and the Web5 `did:ckb` contract are separate protocols.
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x0b1f412fbae26853ff7d082d422c2bdd9e2ff94ee8aaec11240a5b34cc6e890f"
            )),
            Some(ProtocolScript::BitCell)
        );
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID_TESTNET
            )),
            Some(ProtocolScript::DotbitAccount)
        );
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x510150477b10d6ab551a509b71265f3164e9fd4137fcb5a4322f49f03092c7c5"
            )),
            Some(ProtocolScript::DidCkb)
        );
        // mNFT issuer testnet
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0xb59879b6ea6fff985223117fa499ce84f8cfb028c4ffdfdf5d3ec19e905a11ed"
            )),
            Some(ProtocolScript::MnftIssuer)
        );
        // Fiber funding testnet (new file)
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c"
            )),
            Some(ProtocolScript::FiberFunding)
        );
        // Stable++ vault: the CORRECTED value maps; the old pool-lock value does NOT map to vault
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x4ed68fcb7eaa4ff78d46a2fad88a32ce9caffd4b96a0a4bba96ff4871f018675"
            )),
            Some(ProtocolScript::StablePpVault)
        );
        // The old pool-lock value must not map to ANY protocol (not just "not vault").
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67"
            )),
            None
        );
        // unknown hash → None
        assert_eq!(
            r.get(&parse_hex_to_bytes(
                "0x0000000000000000000000000000000000000000000000000000000000000000"
            )),
            None
        );
    }

    #[test]
    fn required_core_protocols_present() {
        let r = &*PROTOCOL_REGISTRY;
        // DAO, sUDT, xUDT must be present (fail-fast guard covers this at build())
        assert!(r.by_code_hash.values().any(|p| *p == ProtocolScript::Dao));
        assert!(r.by_code_hash.values().any(|p| *p == ProtocolScript::Sudt));
        assert!(r.by_code_hash.values().any(|p| *p == ProtocolScript::Xudt));
    }
}
