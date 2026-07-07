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
        // Spore-DID hashes live in bit-cell.toml (metadata_slug = "bit-cell"),
        // NOT a spore-did.toml. Keep both arms so the DID hashes are picked up.
        "spore-did" | "bit-cell" => ProtocolScript::SporeDid,
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
                    by_code_hash.insert(parse_hex_to_bytes(code_hash_hex), protocol);
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
        assert_ne!(
            r.get(&parse_hex_to_bytes(
                "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67"
            )),
            Some(ProtocolScript::StablePpVault)
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
