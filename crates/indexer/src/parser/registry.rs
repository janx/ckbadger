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
    /// Witness-backed file cells; read classification only, no indexed asset kind.
    Ckbfs,
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
    /// `.cell` (Cells) naming protocol: the name cell's type script.
    DotCellAccount,
    /// `.cell` Account Lock — every name cell's lock, empty args.
    DotCellAccountLock,
    /// `.cell` Sale Lock — args `seller_lock_hash(32) ‖ price_shannons(u64 LE)`.
    DotCellSaleLock,
    /// `.cell` Price cell type script (decoded in Phase 2).
    DotCellPrice,
}

impl ProtocolScript {
    /// The registry slug (`docs/metadata/scripts/<slug>.toml` file stem) that
    /// names this protocol — the inverse of [`slug_to_protocol`]. API
    /// consumers receive this string as a cell script's protocol identity so
    /// they never match code hashes themselves.
    pub fn slug(self) -> &'static str {
        match self {
            ProtocolScript::Dao => "nervos-dao",
            ProtocolScript::Ckbfs => "ckbfs",
            ProtocolScript::Sudt => "simple-udt",
            ProtocolScript::Xudt => "xudt",
            ProtocolScript::SporeNft => "spore",
            ProtocolScript::BitCell => "bit-cell",
            ProtocolScript::DidCkb => "did-ckb",
            // Not reachable through the registry (no bundled slug maps to it);
            // named so the match stays exhaustive.
            ProtocolScript::SporeDid => "spore-did",
            ProtocolScript::Cluster => "spore-cluster",
            ProtocolScript::MnftIssuer => "m-nft-issuer",
            ProtocolScript::MnftClass => "m-nft-class",
            ProtocolScript::MnftToken => "m-nft",
            ProtocolScript::RgbppLock => "rgb",
            ProtocolScript::BtcTimeLock => "btc-time-lock",
            ProtocolScript::DotbitAccount => "bit-account",
            ProtocolScript::FiberFunding => "fiber-funding-lock",
            ProtocolScript::FiberCommitment => "fiber-commitment-lock",
            ProtocolScript::StablePpAsset => "stable-asset",
            ProtocolScript::StablePpPool => "stable-pool",
            ProtocolScript::StablePpIntent => "stable-intent-lock",
            ProtocolScript::StablePpVault => "stable-vault-lock",
            ProtocolScript::UtxoSwapIntent => "utxoswap-intent-lock",
            ProtocolScript::DotCellAccount => "dotcell-account",
            ProtocolScript::DotCellAccountLock => "dotcell-account-lock",
            ProtocolScript::DotCellSaleLock => "dotcell-sale-lock",
            ProtocolScript::DotCellPrice => "dotcell-price",
        }
    }
}

/// Map a registry file's `metadata_slug` (file stem) to a protocol identity.
/// Slugs not listed here are labels-only scripts with no parser detection.
fn slug_to_protocol(slug: &str) -> Option<ProtocolScript> {
    Some(match slug {
        "nervos-dao" => ProtocolScript::Dao,
        "ckbfs" => ProtocolScript::Ckbfs,
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
        "dotcell-account" => ProtocolScript::DotCellAccount,
        "dotcell-account-lock" => ProtocolScript::DotCellAccountLock,
        "dotcell-sale-lock" => ProtocolScript::DotCellSaleLock,
        "dotcell-price" => ProtocolScript::DotCellPrice,
        _ => return None,
    })
}

/// Display names of the protocols the indexer emits in activity protocol
/// actions, keyed by protocol id — `docs/metadata/protocols.toml`, validated
/// and bundled by `build.rs`.
static PROTOCOL_NAMES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_slice(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/bundled_protocol_names.json"
    )))
    .expect("bundled protocol names JSON is invalid — build.rs bug")
});

/// The display name of protocol `id`, `None` when the bundled registry has no
/// entry for it.
pub fn protocol_display_name(id: &str) -> Option<&'static str> {
    PROTOCOL_NAMES.get(id).map(String::as_str)
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

// ── Script layouts (R-7, R-8) ────────────────────────────────────────────
// What the registry says about how a script's bytes read: a lock's args
// layout (`[lock_args]`) and a type script's CoBuild action schema
// (`[cobuild]`). Read-path data only: the API decodes with it; no indexer
// stage consults it.

/// How a lock's args read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockArgsLayout {
    /// A layout the API implements from its spec (flag-driven optional
    /// fields, variable tails), named by the registry entry.
    Codec(String),
    /// A fixed sequence of byte fields; the args are exactly their total
    /// length.
    Fields(Vec<LockArgsField>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockArgsField {
    pub name: String,
    pub bytes: usize,
    /// The field's only valid value, when the layout fixes it.
    pub equals: Option<Vec<u8>>,
}

/// A type script's CoBuild actions: `Action.data` is the molecule union
/// `schema`, of which the script emits exactly `actions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CobuildSchema {
    pub schema: String,
    pub actions: Vec<CobuildActionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CobuildActionItem {
    /// Union item id.
    pub id: u32,
    pub name: String,
    /// Field count of the item's molecule table.
    pub fields: usize,
}

/// One registry entry's layouts, with the names that identify it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLayout {
    /// `docs/metadata/scripts/<slug>.toml` file stem.
    pub slug: String,
    pub name: String,
    pub lock_args: Option<LockArgsLayout>,
    pub cobuild: Option<CobuildSchema>,
}

/// Every registry entry that declares a layout, keyed by each code hash its
/// deployments reference (`canonical_ref_hash`, or a pseudo `code_hash`), on
/// both networks.
pub struct ScriptLayouts {
    by_code_hash: HashMap<Vec<u8>, std::sync::Arc<ScriptLayout>>,
}

fn decode_layout_hex(value: &str, at: &str) -> Vec<u8> {
    hex::decode(value.trim_start_matches("0x"))
        .unwrap_or_else(|e| panic!("{at}: invalid hex `{value}`: {e} — build.rs validates this"))
}

impl ScriptLayouts {
    fn from_bundled() -> Self {
        let mut by_code_hash: HashMap<Vec<u8>, std::sync::Arc<ScriptLayout>> = HashMap::new();
        for meta in script_labels() {
            if meta.lock_args.is_none() && meta.cobuild.is_none() {
                continue;
            }
            let slug = meta
                .metadata_slug
                .clone()
                .expect("bundled script labels carry their file stem — build.rs bug");
            let lock_args = meta.lock_args.as_ref().map(|layout| match &layout.codec {
                Some(codec) => LockArgsLayout::Codec(codec.clone()),
                None => LockArgsLayout::Fields(
                    layout
                        .fields
                        .iter()
                        .map(|field| LockArgsField {
                            name: field.name.clone(),
                            bytes: field.bytes,
                            equals: field
                                .equals
                                .as_deref()
                                .map(|value| decode_layout_hex(value, &slug)),
                        })
                        .collect(),
                ),
            });
            let cobuild = meta.cobuild.as_ref().map(|cobuild| CobuildSchema {
                schema: cobuild.schema.clone(),
                actions: cobuild
                    .actions
                    .iter()
                    .map(|action| CobuildActionItem {
                        id: action.id,
                        name: action.name.clone(),
                        fields: action.fields,
                    })
                    .collect(),
            });
            let layout = std::sync::Arc::new(ScriptLayout {
                slug: slug.clone(),
                name: meta.name.clone(),
                lock_args,
                cobuild,
            });
            for net in [meta.mainnet.as_ref(), meta.testnet.as_ref()]
                .into_iter()
                .flatten()
            {
                for dep in net.import_deployments() {
                    let code_hash_hex = match &dep {
                        ImportDeployment::Version(version) => &version.canonical_ref_hash,
                        ImportDeployment::Pseudo(pseudo) => &pseudo.code_hash,
                    };
                    let bytes = decode_layout_hex(code_hash_hex, &slug);
                    if let Some(previous) = by_code_hash.insert(bytes.clone(), layout.clone()) {
                        // Same file, another network or version: idempotent. A
                        // different file on the same code hash is a data bug.
                        assert_eq!(
                            previous.slug,
                            layout.slug,
                            "script layout code_hash collision: 0x{} is declared by both {} and {}",
                            hex::encode(&bytes),
                            previous.slug,
                            layout.slug
                        );
                    }
                }
            }
        }
        Self { by_code_hash }
    }

    /// The layouts of the registry entry a script with `code_hash` belongs
    /// to; `None` when no entry for that code hash declares one.
    pub fn get(&self, code_hash: &[u8]) -> Option<&ScriptLayout> {
        self.by_code_hash
            .get(code_hash)
            .map(|layout| layout.as_ref())
    }

    /// Every `(code_hash, layout)` entry.
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &ScriptLayout)> + '_ {
        self.by_code_hash
            .iter()
            .map(|(code_hash, layout)| (code_hash, layout.as_ref()))
    }
}

pub static SCRIPT_LAYOUTS: LazyLock<ScriptLayouts> = LazyLock::new(ScriptLayouts::from_bundled);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn ckbfs_deployments_have_one_canonical_protocol_identity() {
        for hash in [
            "31e6376287d223b8c0410d562fb422f04d1d617b2947596a14c3d2efb7218d3a",
            "b5d13ffe0547c78021c01fe24dce2e959a1ed8edbca3cb93dd2e9f57fb56d695",
        ] {
            assert_eq!(
                PROTOCOL_REGISTRY
                    .get(&hex::decode(hash).unwrap())
                    .map(|p| p.slug()),
                Some("ckbfs")
            );
        }
    }

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

    /// `slug()` is the inverse of `slug_to_protocol` for every protocol the
    /// registry can actually hand out: the API serves these strings as the
    /// protocol identity of a cell's scripts, so a slug that does not map back
    /// would name a protocol the rest of the system does not know.
    #[test]
    fn slug_round_trips_for_every_registered_code_hash() {
        let r = &*PROTOCOL_REGISTRY;
        let mut seen = 0usize;
        for (code_hash, protocol) in r.iter() {
            assert_eq!(
                slug_to_protocol(protocol.slug()),
                Some(protocol),
                "slug {:?} of {protocol:?} (code_hash 0x{}) must map back to it",
                protocol.slug(),
                hex::encode(code_hash)
            );
            seen += 1;
        }
        assert!(seen > 0, "registry must not be empty");
    }

    #[test]
    fn slug_is_unique_per_variant() {
        let r = &*PROTOCOL_REGISTRY;
        let mut by_slug: HashMap<&'static str, ProtocolScript> = HashMap::new();
        for (_, protocol) in r.iter() {
            if let Some(prev) = by_slug.insert(protocol.slug(), protocol) {
                assert_eq!(
                    prev,
                    protocol,
                    "slug {:?} is shared by {prev:?} and {protocol:?}",
                    protocol.slug()
                );
            }
        }
        // The retained legacy variant has its own slug too, distinct from all
        // registry slugs (the match is exhaustive; nothing maps to it).
        assert!(!by_slug.contains_key(ProtocolScript::SporeDid.slug()));
        assert_eq!(slug_to_protocol(ProtocolScript::SporeDid.slug()), None);
    }

    /// R-8: the JoyID and Nervape Shadow Lock entries declare their args
    /// field layouts, reachable from every code hash their files list.
    #[test]
    fn script_layouts_read_lock_args_fields_from_the_registry() {
        let joyid = SCRIPT_LAYOUTS
            .get(&parse_hex_to_bytes(
                "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            ))
            .expect("JoyID layout");
        assert_eq!(joyid.slug, "joyid");
        assert_eq!(joyid.name, "JoyID");
        assert_eq!(
            joyid.lock_args,
            Some(LockArgsLayout::Fields(vec![
                LockArgsField {
                    name: "subType".into(),
                    bytes: 2,
                    equals: None
                },
                LockArgsField {
                    name: "pubkeyHash".into(),
                    bytes: 20,
                    equals: None
                },
            ]))
        );
        let shadow = SCRIPT_LAYOUTS
            .get(&parse_hex_to_bytes(
                "0x6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24",
            ))
            .expect("Nervape Shadow Lock layout");
        assert_eq!(
            shadow.lock_args,
            Some(LockArgsLayout::Fields(vec![
                LockArgsField {
                    name: "prefix".into(),
                    bytes: 1,
                    equals: Some(vec![0x01])
                },
                LockArgsField {
                    name: "ckbfsTypeHash".into(),
                    bytes: 32,
                    equals: None
                },
            ]))
        );
        // A codec-named family: every default-lock code hash.
        assert_eq!(
            SCRIPT_LAYOUTS
                .get(&parse_hex_to_bytes(
                    "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                ))
                .and_then(|layout| layout.lock_args.clone()),
            Some(LockArgsLayout::Codec("secp256k1_blake160".into()))
        );
    }

    /// R-7: Spore-family type scripts declare the `SporeAction` items they
    /// emit; xUDT declares none (no action schema is specified for it).
    #[test]
    fn script_layouts_read_cobuild_schemas_from_the_registry() {
        let spore_code = PROTOCOL_REGISTRY
            .iter()
            .find(|(_, protocol)| *protocol == ProtocolScript::SporeNft)
            .unwrap()
            .0
            .clone();
        let spore = SCRIPT_LAYOUTS
            .get(&spore_code)
            .and_then(|layout| layout.cobuild.clone())
            .expect("Spore CoBuild schema");
        assert_eq!(spore.schema, "spore");
        assert_eq!(
            spore
                .actions
                .iter()
                .map(|action| (action.id, action.name.as_str(), action.fields))
                .collect::<Vec<_>>(),
            vec![
                (0, "MintSpore", 3),
                (1, "TransferSpore", 3),
                (2, "BurnSpore", 2)
            ]
        );
        for (code_hash, protocol) in PROTOCOL_REGISTRY.iter() {
            if protocol == ProtocolScript::Xudt {
                assert!(
                    SCRIPT_LAYOUTS
                        .get(code_hash)
                        .is_none_or(|layout| layout.cobuild.is_none()),
                    "xUDT has no specified CoBuild action schema"
                );
            }
        }
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
