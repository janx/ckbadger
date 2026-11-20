//! Data-driven protocol-script detection, built once from the bundled label
//! registry (`docs/metadata/scripts/*.toml`). Network-agnostic union: a
//! code_hash uniquely identifies a protocol across networks (no collisions).

use std::collections::{HashMap, HashSet};
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

/// The hash type bytes CKB defines (data, type, data1, data2): a batch
/// pre-filter that knows only code hashes asks [`ProtocolRegistry::script`]
/// of each.
pub const HASH_TYPES: [i16; 4] = [0, 1, 2, 4];

/// A lock that carries no protocol: the registry's standard locks (access
/// control only). A transaction does not "call" one (`lock_calls`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardLock {
    DefaultLock,
    DefaultMultisig,
    AnyoneCanPay,
    Omnilock,
    PwLock,
    JoyId,
}

/// Map a registry file's slug to a standard lock.
fn slug_to_standard_lock(slug: &str) -> Option<StandardLock> {
    Some(match slug {
        "default-lock" => StandardLock::DefaultLock,
        "default-multisig" => StandardLock::DefaultMultisig,
        "anyone-can-pay-lock" => StandardLock::AnyoneCanPay,
        "omni-lock" => StandardLock::Omnilock,
        "pw-lock" => StandardLock::PwLock,
        "joyid" => StandardLock::JoyId,
        _ => return None,
    })
}

/// The registry file stem of the Unique Cell (`docs/metadata/scripts/unique-cell.toml`).
const UNIQUE_CELL_SLUG: &str = "unique-cell";

/// What a registered pair runs, for the indexer: a protocol's script, a
/// standard lock, or the Unique Cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredScript {
    Protocol(ProtocolScript),
    StandardLock(StandardLock),
    /// The Unique Cell type script: a token-info cell whose args the script
    /// derives itself (Type ID-like), so it is the one cell an xUDT
    /// extension can name by its args (resync fix round S1). No protocol
    /// ([`ProtocolRegistry::protocol`] is `None` for it) and no lock.
    UniqueCell,
}

/// The scripts the indexer recognises, keyed by the pair CKB names a script
/// by (code hash, hash type byte: data 0, type 1, data1 2, data2 4).
///
/// A registered script runs at: each deployment's registered reference
/// (`canonical_ref_hash` by `canonical_hash_type`, or a pseudo deployment's
/// pair), and each registered version's binary by every data hash type
/// (`version_hash` by data, data1 and data2: the same code on each VM). No
/// other pair runs it. In particular a code hash registered by `type` is no
/// protocol by `data` (the Nervos DAO is its consensus reference alone, by
/// `type`: the consensus pays compensation for that script only): a lock (which runs only when its cell is spent) can
/// be put on a cell under any pair, and a cell under the protocol's code hash
/// by another hash type runs a binary whose data hash is that code hash, which
/// no registered deployment holds.
///
/// The upgrade catch: a `type` reference runs whatever its Type ID cell holds
/// now, so the pair stays the protocol's across upgrades of that cell; a
/// binary pair names one version's code, and a new version's binary is a
/// protocol's pair only once its version is registered here.
pub struct ProtocolRegistry {
    pub(crate) by_pair: HashMap<([u8; 32], u8), RegisteredScript>,
    /// The code hashes of the Unique Cell's registered pairs (a fixed
    /// handful, both networks), compared before its pair is looked up: bulk's
    /// serial lane asks [`Self::is_unique_cell`] for every typed output
    /// (resync fix round 2, C).
    unique_cell_code_hashes: Vec<[u8; 32]>,
}

/// The registry file stem of the Nervos DAO.
const DAO_SLUG: &str = "nervos-dao";

/// THE pair rule: every pair (code hash, hash type byte) the registry entry
/// `meta` (file stem `slug`) runs at, on both networks — each deployment's
/// registered reference (`canonical_ref_hash` by `canonical_hash_type`, or a
/// pseudo deployment's pair), and each registered version's binary by every
/// data hash type (data, data1, data2: the same code on each VM). The Nervos
/// DAO is its consensus reference alone: the consensus computes compensation
/// for the DAO type script by type, never for its binary by a data hash type.
///
/// The one enumeration of what a registry entry runs: the protocol registry
/// ([`PROTOCOL_REGISTRY`]) and the script layouts ([`SCRIPT_LAYOUTS`], a lock
/// call's args layout and a type's CoBuild schema) are both keyed by it, so a
/// pair that runs no registered code is no protocol, decodes nothing and is
/// named nothing (resync fix round S2).
fn registered_pairs(meta: &crate::label_import::ScriptMetadata, slug: &str) -> Vec<([u8; 32], u8)> {
    let key = |code_hash: Vec<u8>| -> [u8; 32] {
        code_hash.as_slice().try_into().unwrap_or_else(|_| {
            panic!(
                "{slug}: registry code hash 0x{} is not 32 bytes — build.rs validates this",
                hex::encode(&code_hash)
            )
        })
    };
    let mut pairs = Vec::new();
    for net in [meta.mainnet.as_ref(), meta.testnet.as_ref()]
        .into_iter()
        .flatten()
    {
        for dep in net.import_deployments() {
            match &dep {
                ImportDeployment::Version(version) => {
                    pairs.push((
                        key(parse_hex_to_bytes(&version.canonical_ref_hash)),
                        hash_type_byte(version.canonical_hash_type.as_str(), slug),
                    ));
                    let attachment = crate::label_import::version_attachment(version)
                        .unwrap_or_else(|e| {
                            panic!("{slug}: bundled version entry: {e} — build.rs validates this")
                        });
                    match attachment {
                        crate::label_import::VersionAttachment::Attachable(_)
                            if slug == DAO_SLUG => {}
                        crate::label_import::VersionAttachment::Attachable(binary) => {
                            let binary = key(binary);
                            for data_type in [0u8, 2, 4] {
                                pairs.push((binary, data_type));
                            }
                        }
                        crate::label_import::VersionAttachment::NoVersion => {}
                        crate::label_import::VersionAttachment::Invalid(reason) => {
                            panic!("{slug}: bundled version entry: {reason}")
                        }
                    }
                }
                ImportDeployment::Pseudo(pseudo) => pairs.push((
                    key(parse_hex_to_bytes(&pseudo.code_hash)),
                    hash_type_byte(pseudo.hash_type.as_str(), slug),
                )),
            }
        }
    }
    pairs
}

impl ProtocolRegistry {
    fn from_bundled() -> Self {
        let mut by_pair: HashMap<([u8; 32], u8), RegisteredScript> = HashMap::new();
        for meta in script_labels() {
            let Some(slug) = meta.metadata_slug.as_deref() else {
                continue;
            };
            let script = match (slug_to_protocol(slug), slug_to_standard_lock(slug)) {
                (Some(protocol), _) => RegisteredScript::Protocol(protocol),
                (None, Some(lock)) => RegisteredScript::StandardLock(lock),
                (None, None) if slug == UNIQUE_CELL_SLUG => RegisteredScript::UniqueCell,
                (None, None) => continue,
            };
            assert_eq!(
                slug == DAO_SLUG,
                script == RegisteredScript::Protocol(ProtocolScript::Dao),
                "{slug}: the DAO's slug and the DAO protocol must agree"
            );
            for (code_hash, hash_type) in registered_pairs(&meta, slug) {
                if let Some(prev) = by_pair.insert((code_hash, hash_type), script) {
                    assert_eq!(
                        prev, script,
                        "protocol registry pair collision: 0x{} by {hash_type} maps to both {prev:?} and {script:?} — two scripts/*.toml register one pair",
                        hex::encode(code_hash)
                    );
                }
            }
        }
        for required in [
            ProtocolScript::Dao,
            ProtocolScript::Sudt,
            ProtocolScript::Xudt,
        ] {
            assert!(
                by_pair
                    .values()
                    .any(|s| *s == RegisteredScript::Protocol(required)),
                "protocol registry missing required-core protocol {required:?} — check docs/metadata/scripts"
            );
        }
        assert!(
            by_pair.values().any(|s| *s == RegisteredScript::UniqueCell),
            "protocol registry missing the Unique Cell — check docs/metadata/scripts/unique-cell.toml"
        );
        let mut unique_cell_code_hashes: Vec<[u8; 32]> = by_pair
            .iter()
            .filter(|(_, script)| **script == RegisteredScript::UniqueCell)
            .map(|((code_hash, _), _)| *code_hash)
            .collect();
        unique_cell_code_hashes.sort_unstable();
        unique_cell_code_hashes.dedup();
        Self {
            by_pair,
            unique_cell_code_hashes,
        }
    }

    /// THE lookup: what the script `(code_hash, hash_type)` runs, if it is
    /// registered. Every detector reads a script through it, lock and type
    /// alike.
    ///
    /// For a type script the pair is exact by construction: a type script
    /// runs when its cell is created, so a cell carries a type pair only when
    /// that pair's code ran, and the registered pairs are the pairs a
    /// protocol's code runs at. For a lock it is the check itself: a lock runs
    /// only when its cell is spent, so anyone can lock a cell under a protocol
    /// lock's code hash by another hash type, and that cell is no protocol's.
    ///
    /// A hash type outside the CKB bytes is no registered pair.
    pub fn script(&self, code_hash: &[u8], hash_type: i16) -> Option<RegisteredScript> {
        // No allocation: the key is the 32 bytes and the hash type byte (a
        // code hash of another width or a hash type outside the bytes is no
        // registered pair).
        let code_hash: [u8; 32] = code_hash.try_into().ok()?;
        let hash_type = u8::try_from(hash_type).ok()?;
        self.by_pair.get(&(code_hash, hash_type)).copied()
    }

    /// The protocol the pair runs ([`Self::script`]).
    pub fn protocol(&self, code_hash: &[u8], hash_type: i16) -> Option<ProtocolScript> {
        match self.script(code_hash, hash_type)? {
            RegisteredScript::Protocol(protocol) => Some(protocol),
            RegisteredScript::StandardLock(_) | RegisteredScript::UniqueCell => None,
        }
    }

    /// Whether the pair runs the Unique Cell ([`Self::script`]): the one
    /// recognition of a Unique Cell, for the xUDT extension binding and claim
    /// and the issuance co-occurrence binding, on both sync paths.
    ///
    /// The code hash is compared with the Unique Cell's few first (32-byte
    /// comparisons, most decided by the first byte); only a match asks the
    /// pair, whose hash type decides. Bulk's serial lane asks this for every
    /// typed output (resync fix round 2, C).
    pub fn is_unique_cell(&self, code_hash: &[u8], hash_type: i16) -> bool {
        if !self
            .unique_cell_code_hashes
            .iter()
            .any(|unique| unique.as_slice() == code_hash)
        {
            return false;
        }
        #[cfg(test)]
        UNIQUE_CELL_PAIR_LOOKUPS.with(|lookups| lookups.set(lookups.get() + 1));
        self.script(code_hash, hash_type) == Some(RegisteredScript::UniqueCell)
    }

    /// Whether the pair runs `protocol` ([`Self::script`]).
    pub fn is(&self, code_hash: &[u8], hash_type: i16, protocol: ProtocolScript) -> bool {
        self.protocol(code_hash, hash_type) == Some(protocol)
    }

    /// Whether the pair runs a standard lock ([`Self::script`]).
    pub fn is_standard_lock(&self, code_hash: &[u8], hash_type: i16) -> bool {
        matches!(
            self.script(code_hash, hash_type),
            Some(RegisteredScript::StandardLock(_))
        )
    }

    /// Every registered pair a protocol runs at.
    pub fn protocol_pairs(&self) -> impl Iterator<Item = ((&[u8], u8), ProtocolScript)> + '_ {
        self.by_pair
            .iter()
            .filter_map(|((code_hash, hash_type), script)| match script {
                RegisteredScript::Protocol(protocol) => {
                    Some(((code_hash.as_slice(), *hash_type), *protocol))
                }
                RegisteredScript::StandardLock(_) | RegisteredScript::UniqueCell => None,
            })
    }
}

/// A bundled hash type label's byte; build.rs validates the labels.
fn hash_type_byte(label: &str, slug: &str) -> u8 {
    ckbadger_common::hash_type_from_label(label)
        .unwrap_or_else(|| panic!("{slug}: hash type `{label}` — build.rs validates this"))
}

#[cfg(test)]
thread_local! {
    /// The pair lookups [`ProtocolRegistry::is_unique_cell`] made on this
    /// thread: the cost on bulk's serial lane, where it is asked for every
    /// typed output (resync fix round 2, C).
    pub(crate) static UNIQUE_CELL_PAIR_LOOKUPS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

pub static PROTOCOL_REGISTRY: LazyLock<ProtocolRegistry> =
    LazyLock::new(ProtocolRegistry::from_bundled);

/// The registry's category of the token scripts (`category = "udt"`).
const UDT_CATEGORY: &str = "udt";

/// The xUDT-compatible deployments' pairs (resync fix round, logic NOTE N3):
/// every registry entry of category `udt` other than sUDT and xUDT (which the
/// registry recognises as those protocols), at the pairs of THE pair rule
/// ([`registered_pairs`]: its reference by its hash type and its version's
/// binary by every data hash type), deprecated or not (a deprecated
/// deployment's cells still run its code). It was a narrower rule bundled by
/// build.rs (references only, deprecated skipped), so a compatible
/// deployment's binary by data1 was no UDT while the xUDT binary by data2 was.
static XUDT_COMPATIBLE_PAIRS: LazyLock<HashSet<([u8; 32], u8)>> = LazyLock::new(|| {
    let mut pairs = HashSet::new();
    for meta in script_labels() {
        let Some(slug) = meta.metadata_slug.as_deref() else {
            continue;
        };
        if meta.category.as_deref() != Some(UDT_CATEGORY)
            || matches!(
                slug_to_protocol(slug),
                Some(ProtocolScript::Sudt | ProtocolScript::Xudt)
            )
        {
            continue;
        }
        pairs.extend(registered_pairs(&meta, slug));
    }
    pairs
});

/// Whether `(code_hash, hash_type)` runs an xUDT-compatible deployment's code
/// ([`XUDT_COMPATIBLE_PAIRS`]). No allocation: the key is the 32 bytes and the
/// hash type byte; a hash type outside the CKB bytes is no pair.
pub(crate) fn is_xudt_compatible(code_hash: &[u8], hash_type: i16) -> bool {
    let (Ok(code_hash), Ok(hash_type)) = (<[u8; 32]>::try_from(code_hash), u8::try_from(hash_type))
    else {
        return false;
    };
    XUDT_COMPATIBLE_PAIRS.contains(&(code_hash, hash_type))
}

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

/// Every registry entry that declares a layout, keyed by each pair its
/// deployments run at ([`registered_pairs`], THE pair rule, the protocol
/// registry's too), on both networks: a pair that runs no registered code has
/// no layout, so it decodes nothing and is named nothing (resync fix round
/// S2).
pub struct ScriptLayouts {
    by_pair: HashMap<([u8; 32], u8), std::sync::Arc<ScriptLayout>>,
}

fn decode_layout_hex(value: &str, at: &str) -> Vec<u8> {
    hex::decode(value.trim_start_matches("0x"))
        .unwrap_or_else(|e| panic!("{at}: invalid hex `{value}`: {e} — build.rs validates this"))
}

impl ScriptLayouts {
    fn from_bundled() -> Self {
        let mut by_pair: HashMap<([u8; 32], u8), std::sync::Arc<ScriptLayout>> = HashMap::new();
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
            for pair in registered_pairs(&meta, &slug) {
                if let Some(previous) = by_pair.insert(pair, layout.clone()) {
                    // Same file, another network or version: idempotent. A
                    // different file on the same pair is a data bug.
                    assert_eq!(
                        previous.slug,
                        layout.slug,
                        "script layout pair collision: 0x{} by {} is declared by both {} and {}",
                        hex::encode(pair.0),
                        pair.1,
                        previous.slug,
                        layout.slug
                    );
                }
            }
        }
        Self { by_pair }
    }

    /// The layouts of the registry entry whose code the script
    /// `(code_hash, hash_type)` runs; `None` when that pair runs no
    /// registered code or its entry declares no layout. A hash type outside
    /// the CKB bytes is no registered pair.
    pub fn get(&self, code_hash: &[u8], hash_type: i16) -> Option<&ScriptLayout> {
        let code_hash: [u8; 32] = code_hash.try_into().ok()?;
        let hash_type = u8::try_from(hash_type).ok()?;
        self.by_pair
            .get(&(code_hash, hash_type))
            .map(|layout| layout.as_ref())
    }

    /// Every `((code_hash, hash_type), layout)` entry.
    pub fn iter(&self) -> impl Iterator<Item = ((&[u8], u8), &ScriptLayout)> + '_ {
        self.by_pair.iter().map(|((code_hash, hash_type), layout)| {
            ((code_hash.as_slice(), *hash_type), layout.as_ref())
        })
    }
}

pub static SCRIPT_LAYOUTS: LazyLock<ScriptLayouts> = LazyLock::new(ScriptLayouts::from_bundled);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    /// resync fix round 2, C: asking whether a typed output is the Unique Cell
    /// hashes no pair for another code hash.
    #[test]
    fn asking_whether_a_cell_is_the_unique_cell_hashes_no_pair_for_another_code() {
        // resync fix round 2, C (logic SHOULD-3): bulk's serial lane asks
        // this for every typed output of every transaction
        // (`owners/token.rs`, `token_helpers::is_unique_cell`). For a code
        // hash that is not one of the Unique Cell's few, the answer is no
        // without hashing a pair; only the Unique Cell's code hashes reach
        // the pair (its hash type decides).
        let xudt = parse_hex_to_bytes(
            "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95",
        );
        let unique_cell = parse_hex_to_bytes(
            "0x2c8c11c985da60b0a330c61a85507416d6382c130ba67f0c47ab071e00aec628",
        );
        let lookups = || UNIQUE_CELL_PAIR_LOOKUPS.with(std::cell::Cell::get);
        let before = lookups();
        for hash_type in [0, 1, 2, 4] {
            assert!(!crate::sync::token_helpers::is_unique_cell(
                &xudt, hash_type
            ));
            assert!(!crate::sync::token_helpers::is_unique_cell(
                &[0x5a; 32],
                hash_type
            ));
        }
        assert_eq!(
            lookups() - before,
            0,
            "no pair hashed for another code hash"
        );
        assert!(crate::sync::token_helpers::is_unique_cell(&unique_cell, 2));
        assert!(!crate::sync::token_helpers::is_unique_cell(&unique_cell, 1));
        assert_eq!(
            lookups() - before,
            2,
            "the Unique Cell's code hash reaches its pair"
        );
    }

    /// resync fix round S1: the Unique Cell is recognised by its pair, by the
    /// one pair rule: each network's registered reference (mainnet: the
    /// binary 0x2c8c… by data1; testnet: 0x8e34… by type) and each version's
    /// binary by every data hash type (mainnet 0x2c8c…, testnet 0xcc25…).
    /// The mainnet binary by `type`, or the testnet reference by a data hash
    /// type, runs no Unique Cell; it is no protocol and no lock.
    #[test]
    fn the_unique_cell_runs_only_at_its_registered_pairs() {
        let mainnet = parse_hex_to_bytes(
            "0x2c8c11c985da60b0a330c61a85507416d6382c130ba67f0c47ab071e00aec628",
        );
        let testnet_reference = parse_hex_to_bytes(
            "0x8e341bcfec6393dcd41e635733ff2dca00a6af546949f70c57a706c0f344df8b",
        );
        let testnet_binary = parse_hex_to_bytes(
            "0xcc2518c2c1384f2473c96f63c4e74074984296f358512ee7f54c848d4c135040",
        );
        for (code_hash, hash_type, runs) in [
            (&mainnet, 0, true),
            (&mainnet, 2, true),
            (&mainnet, 4, true),
            (&mainnet, 1, false),
            (&testnet_reference, 1, true),
            (&testnet_reference, 0, false),
            (&testnet_reference, 2, false),
            (&testnet_binary, 2, true),
            (&testnet_binary, 1, false),
        ] {
            assert_eq!(
                PROTOCOL_REGISTRY.is_unique_cell(code_hash, hash_type),
                runs,
                "0x{} by {hash_type}",
                hex::encode(code_hash)
            );
            assert_eq!(PROTOCOL_REGISTRY.protocol(code_hash, hash_type), None);
            assert!(!PROTOCOL_REGISTRY.is_standard_lock(code_hash, hash_type));
        }
    }

    #[test]
    fn ckbfs_deployments_have_one_canonical_protocol_identity() {
        for hash in [
            "31e6376287d223b8c0410d562fb422f04d1d617b2947596a14c3d2efb7218d3a",
            "b5d13ffe0547c78021c01fe24dce2e959a1ed8edbca3cb93dd2e9f57fb56d695",
        ] {
            assert_eq!(
                // registered by data1
                PROTOCOL_REGISTRY
                    .protocol(&hex::decode(hash).unwrap(), 2)
                    .map(|p| p.slug()),
                Some("ckbfs")
            );
        }
    }

    #[test]
    fn a_registered_binary_runs_by_every_data_hash_type_but_the_dao_is_its_reference() {
        // resync/protocol-pairs: Fiber's funding-lock binary (one binary on
        // both networks) is the funding lock by data, data1 and data2; its
        // type reference by type; the reference by data is nothing.
        let funding_binary = parse_hex_to_bytes(
            "0x17b1910fbcfdfc146ee2ed05587f0e862b799d33ca3c8e1c52d18f2f67716e47",
        );
        let funding_reference = parse_hex_to_bytes(
            "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1",
        );
        for hash_type in [0, 2, 4] {
            assert_eq!(
                PROTOCOL_REGISTRY.protocol(&funding_binary, hash_type),
                Some(ProtocolScript::FiberFunding)
            );
        }
        assert_eq!(PROTOCOL_REGISTRY.protocol(&funding_binary, 1), None);
        assert_eq!(
            PROTOCOL_REGISTRY.protocol(&funding_reference, 1),
            Some(ProtocolScript::FiberFunding)
        );
        assert_eq!(PROTOCOL_REGISTRY.protocol(&funding_reference, 0), None);
        // The DAO's consensus script is its reference by type; its binary by a
        // data hash type is no DAO.
        let dao_binary = parse_hex_to_bytes(
            "0x32064a14ce10d95d4b7343054cc19d73b25b16ae61a6c681011ca781a60c7923",
        );
        for hash_type in [0, 1, 2, 4] {
            assert_eq!(PROTOCOL_REGISTRY.protocol(&dao_binary, hash_type), None);
        }
        assert_eq!(
            PROTOCOL_REGISTRY.protocol(&parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH), 1),
            Some(ProtocolScript::Dao)
        );
        // A hash type outside the CKB bytes, or a code hash of another width,
        // is no registered pair.
        assert_eq!(PROTOCOL_REGISTRY.protocol(&funding_reference, 300), None);
        assert_eq!(
            PROTOCOL_REGISTRY.protocol(&funding_reference[..31], 1),
            None
        );
    }

    #[test]
    fn detects_mainnet_and_testnet_hashes() {
        let r = &*PROTOCOL_REGISTRY;
        // sUDT: mainnet AND testnet both map to Sudt (the testnet fix this plan delivers)
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5"
                ),
                1
            ),
            Some(ProtocolScript::Sudt)
        );
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4"
                ),
                1
            ),
            Some(ProtocolScript::Sudt)
        );
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x48dbf59b4c7ee1547238021b4869bceedf4eea6b43772e5d66ef8865b6ae7212"
                ),
                0 // registered by data
            ),
            Some(ProtocolScript::Sudt)
        );
        // `.bit Cell` and the Web5 `did:ckb` contract are separate protocols.
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x0b1f412fbae26853ff7d082d422c2bdd9e2ff94ee8aaec11240a5b34cc6e890f"
                ),
                1
            ),
            Some(ProtocolScript::BitCell)
        );
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID_TESTNET),
                1
            ),
            Some(ProtocolScript::DotbitAccount)
        );
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x510150477b10d6ab551a509b71265f3164e9fd4137fcb5a4322f49f03092c7c5"
                ),
                1
            ),
            Some(ProtocolScript::DidCkb)
        );
        // mNFT issuer testnet
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0xb59879b6ea6fff985223117fa499ce84f8cfb028c4ffdfdf5d3ec19e905a11ed"
                ),
                1
            ),
            Some(ProtocolScript::MnftIssuer)
        );
        // Fiber funding testnet (new file)
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c"
                ),
                1
            ),
            Some(ProtocolScript::FiberFunding)
        );
        // Stable++ vault: the CORRECTED value maps; the old pool-lock value does NOT map to vault
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x4ed68fcb7eaa4ff78d46a2fad88a32ce9caffd4b96a0a4bba96ff4871f018675"
                ),
                1
            ),
            Some(ProtocolScript::StablePpVault)
        );
        // The old pool-lock value must not map to ANY protocol (not just "not vault").
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67"
                ),
                1
            ),
            None
        );
        // unknown hash → None
        assert_eq!(
            r.protocol(
                &parse_hex_to_bytes(
                    "0x0000000000000000000000000000000000000000000000000000000000000000"
                ),
                1
            ),
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
        for ((code_hash, _), protocol) in r.protocol_pairs() {
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
        for ((_, _), protocol) in r.protocol_pairs() {
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
            .get(
                &parse_hex_to_bytes(
                    "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
                ),
                1,
            )
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
            .get(
                &parse_hex_to_bytes(
                    "0x6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24",
                ),
                2,
            )
            .expect("Nervape Shadow Lock layout");
        assert_eq!(
            shadow.lock_args,
            Some(LockArgsLayout::Fields(vec![
                LockArgsField {
                    name: "prefix".into(),
                    bytes: 1,
                    equals: None
                },
                LockArgsField {
                    name: "hash".into(),
                    bytes: 32,
                    equals: None
                },
            ]))
        );
        // A codec-named family: every default-lock code hash.
        let default_lock = parse_hex_to_bytes(
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
        );
        assert_eq!(
            SCRIPT_LAYOUTS
                .get(&default_lock, 1)
                .and_then(|layout| layout.lock_args.clone()),
            Some(LockArgsLayout::Codec("secp256k1_blake160".into()))
        );
        // resync fix round S2: by the pair. The default lock's code hash by
        // data runs no registered code: no layout, so nothing decodes or is
        // named under it.
        assert!(SCRIPT_LAYOUTS.get(&default_lock, 0).is_none());
    }

    /// resync fix round S2: the layouts are keyed by THE pair rule the protocol
    /// registry is keyed by (`registered_pairs`): every pair the registry
    /// registers for an entry with a layout has its layout, and no other pair
    /// of that code hash does.
    #[test]
    fn script_layouts_are_keyed_by_the_registry_pairs() {
        for meta in script_labels() {
            let Some(slug) = meta.metadata_slug.clone() else {
                continue;
            };
            if meta.lock_args.is_none() && meta.cobuild.is_none() {
                continue;
            }
            let pairs = registered_pairs(&meta, &slug);
            for (code_hash, hash_type) in &pairs {
                assert_eq!(
                    SCRIPT_LAYOUTS
                        .get(code_hash, i16::from(*hash_type))
                        .map(|layout| layout.slug.as_str()),
                    Some(slug.as_str()),
                    "{slug}: 0x{} by {hash_type}",
                    hex::encode(code_hash)
                );
                for other in HASH_TYPES {
                    if !pairs.contains(&(*code_hash, u8::try_from(other).unwrap())) {
                        assert!(
                            SCRIPT_LAYOUTS
                                .get(code_hash, other)
                                .is_none_or(|layout| layout.slug != slug),
                            "{slug}: 0x{} by {other} runs no registered code",
                            hex::encode(code_hash)
                        );
                    }
                }
            }
        }
    }

    /// Tests report NOTE-6: the layout pairs written out, not taken from
    /// `registered_pairs` (the test above holds the implementation to
    /// itself). The Default Lock's args read by `secp256k1_blake160` at its
    /// reference by `type` and at its binary by data, data1 and data2, and at
    /// no other hash type of either.
    #[test]
    fn the_default_locks_layout_is_at_its_written_out_pairs() {
        let reference = parse_hex_to_bytes(
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
        );
        let binary = parse_hex_to_bytes(
            "0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649",
        );
        for (code_hash, hash_type, layout) in [
            (&reference, 1, true),
            (&reference, 0, false),
            (&reference, 2, false),
            (&reference, 4, false),
            (&binary, 0, true),
            (&binary, 2, true),
            (&binary, 4, true),
            (&binary, 1, false),
        ] {
            assert_eq!(
                SCRIPT_LAYOUTS
                    .get(code_hash, hash_type)
                    .map(|layout| layout.slug.as_str()),
                layout.then_some("default-lock"),
                "0x{} by {hash_type}",
                hex::encode(code_hash)
            );
        }
    }

    /// R-7: Spore-family type scripts declare the `SporeAction` items they
    /// emit; xUDT declares none (no action schema is specified for it).
    #[test]
    fn script_layouts_read_cobuild_schemas_from_the_registry() {
        let spore_code = PROTOCOL_REGISTRY
            .protocol_pairs()
            .find(|((_, hash_type), protocol)| {
                // Spore's references are data1 deployments
                *protocol == ProtocolScript::SporeNft && *hash_type == 2
            })
            .unwrap()
            .0
             .0
            .to_vec();
        let spore = SCRIPT_LAYOUTS
            .get(&spore_code, 2)
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
        for ((code_hash, hash_type), protocol) in PROTOCOL_REGISTRY.protocol_pairs() {
            if protocol == ProtocolScript::Xudt {
                assert!(
                    SCRIPT_LAYOUTS
                        .get(code_hash, i16::from(hash_type))
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
        assert!(r
            .by_pair
            .values()
            .any(|p| *p == RegisteredScript::Protocol(ProtocolScript::Dao)));
        assert!(r
            .by_pair
            .values()
            .any(|p| *p == RegisteredScript::Protocol(ProtocolScript::Sudt)));
        assert!(r
            .by_pair
            .values()
            .any(|p| *p == RegisteredScript::Protocol(ProtocolScript::Xudt)));
    }
}
