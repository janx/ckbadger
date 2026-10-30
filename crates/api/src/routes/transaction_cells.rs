//! Cell display evidence for transaction tiles. Raw fields come from the actual
//! outpoint; indexed metadata only supplies the asset's immutable identity.
use anyhow::{anyhow, ensure, Context, Result};
use ckbadger_indexer::parser::{
    bit_cell::BitCellParser,
    cell::{CellParser, ParsedCell},
    did_ckb::DidCkbParser,
    dotbit::DotbitParser,
    dotcell::DotCellParser,
    mnft::MnftParser,
    registry::{ProtocolScript, PROTOCOL_REGISTRY},
    spore::SporeParser,
    udt::UdtParser,
};
use ckbadger_store::{
    types::{IdentityStandard, ObjectExtra, ObjectStandard},
    CkbadgerStore,
};
use serde::Serialize;

use crate::response::ScriptResponse;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDisplay {
    pub kind: &'static str,
    pub standard: String,
    /// Asset id, or the full type-script hash for tokens/files.
    pub id: String,
    pub name: Option<String>,
    pub collection_id: Option<String>,
    pub collection_name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    /// Exact raw token amount, without decimal scaling.
    pub amount: Option<String>,
    pub composition_tier: Option<String>,
}
impl CellDisplay {
    fn new(kind: &'static str, standard: &str, id: &[u8]) -> Self {
        Self {
            kind,
            standard: standard.into(),
            id: format!("0x{}", hex::encode(id)),
            name: None,
            collection_id: None,
            collection_name: None,
            symbol: None,
            decimals: None,
            amount: None,
            composition_tier: None,
        }
    }
}

pub fn has_display(script: Option<&ScriptResponse>) -> Result<bool> {
    let Some(script) = script else {
        return Ok(false);
    };
    let code_hash = hex::decode(script.code_hash.trim_start_matches("0x"))?;
    Ok(UdtParser::is_udt_code_hash_bytes(&code_hash, 0).is_some()
        || matches!(
            PROTOCOL_REGISTRY.get(&code_hash),
            Some(
                ProtocolScript::SporeNft
                    | ProtocolScript::MnftToken
                    | ProtocolScript::DidCkb
                    | ProtocolScript::BitCell
                    | ProtocolScript::DotbitAccount
                    | ProtocolScript::DotCellAccount
                    | ProtocolScript::Ckbfs
            )
        ))
}

pub fn from_parts(
    store: &CkbadgerStore,
    capacity: &str,
    lock: &ScriptResponse,
    type_script: Option<&ScriptResponse>,
    data: &[u8],
    indexed: bool,
) -> Result<Option<CellDisplay>> {
    if !has_display(type_script)? {
        return Ok(None);
    }
    let to_rpc = |s: &ScriptResponse| ckbadger_indexer::rpc::Script {
        code_hash: s.code_hash.clone(),
        hash_type: s.hash_type.clone(),
        args: s.args.clone(),
    };
    let output = ckbadger_indexer::rpc::CellOutput {
        capacity: format!("0x{:x}", capacity.parse::<u64>()?),
        lock: to_rpc(lock),
        type_: type_script.map(to_rpc),
    };
    let cell = CellParser::parse_output(&output, &format!("0x{}", hex::encode(data)))?;
    display(store, &cell, indexed)
}

fn display(store: &CkbadgerStore, cell: &ParsedCell, indexed: bool) -> Result<Option<CellDisplay>> {
    let code_hash = cell
        .type_code_hash
        .as_deref()
        .ok_or_else(|| anyhow!("display requires type code hash"))?;
    let type_hash = cell
        .type_script_hash
        .as_deref()
        .ok_or_else(|| anyhow!("display requires type script hash"))?;
    if let Some(standard) = UdtParser::is_udt_code_hash_bytes(
        code_hash,
        cell.type_hash_type
            .ok_or_else(|| anyhow!("display requires type hash type"))?,
    ) {
        let mut result = CellDisplay::new("token", standard.as_str(), type_hash);
        result.amount = Some(
            UdtParser::parse_amount(&cell.data)
                .ok_or_else(|| {
                    anyhow!(
                        "invalid {} amount for token {}",
                        standard.as_str(),
                        result.id
                    )
                })?
                .to_string(),
        );
        let token = store.get_token(type_hash)?;
        ensure!(
            !indexed || token.is_some(),
            "missing indexed token metadata {}",
            result.id
        );
        if let Some(token) = token {
            result.name = token.name;
            result.symbol = token.symbol;
            result.decimals = token
                .decimals
                .map(u8::try_from)
                .transpose()
                .context("invalid token decimals")?;
        }
        return Ok(Some(result));
    }
    let protocol = PROTOCOL_REGISTRY.get(code_hash);
    if protocol == Some(ProtocolScript::Ckbfs) {
        let mut result = CellDisplay::new("file", "ckbfs", type_hash);
        result.name = ckbfs_filename(&cell.data).context("invalid CKBFS cell data")?;
        return Ok(Some(result));
    }
    let malformed = || {
        anyhow!(
            "malformed {} cell type=0x{}",
            protocol.map(|p| p.slug()).unwrap_or("unrecognized"),
            hex::encode(type_hash)
        )
    };
    let object = match protocol {
        Some(ProtocolScript::SporeNft) => {
            let parsed = SporeParser::parse_spore_parsed_cell(cell).ok_or_else(malformed)?;
            Some((parsed.spore_id, parsed.cluster_id, ObjectStandard::Spore))
        }
        Some(ProtocolScript::MnftToken) => {
            let parsed = MnftParser::parse_token_parsed_cell(cell).ok_or_else(malformed)?;
            Some((
                parsed.token_id,
                Some(parsed.class_id),
                ObjectStandard::MnftToken,
            ))
        }
        _ => None,
    };
    if let Some((id, collection_id, standard)) = object {
        let mut result = CellDisplay::new("object", standard.asset_standard(), &id);
        let entry = if standard == ObjectStandard::Spore {
            store.get_spore(&id)?
        } else {
            store.get_mnft(&id)?
        };
        ensure!(
            !indexed || entry.is_some(),
            "missing indexed object metadata {}",
            result.id
        );
        // Spore item name/content and collection membership are immutable. We
        // never use current ownership/liveness to describe this historical cell.
        if let Some(entry) = entry {
            ensure!(
                entry.standard == standard,
                "object {} standard mismatch",
                result.id
            );
            if standard == ObjectStandard::Spore {
                result.name = entry.name;
            }
            match entry.extra {
                ObjectExtra::Spore { media_profile, .. } if standard == ObjectStandard::Spore => {
                    result.composition_tier = Some(media_profile.tier.as_str().into());
                }
                ObjectExtra::MnftToken { .. } if standard == ObjectStandard::MnftToken => {}
                extra => anyhow::bail!(
                    "object {} standard {:?} disagrees with payload variant {:?}",
                    result.id,
                    standard,
                    extra
                ),
            }
        }
        if let Some(collection_id) = collection_id {
            result.collection_id = Some(format!("0x{}", hex::encode(&collection_id)));
            if standard == ObjectStandard::Spore {
                result.collection_name =
                    spore_cluster_name(store, &collection_id, indexed, &result.id)?;
            }
            // mNFT class names/renderers can change. A token cell only proves
            // its class id; the latest class row is not historical evidence.
        }
        return Ok(Some(result));
    }
    let (id, name, standard) = match protocol {
        Some(ProtocolScript::DidCkb) => (
            DidCkbParser::parse_did_parsed_cell(cell)
                .ok_or_else(malformed)?
                .did_id,
            None,
            IdentityStandard::DidCkb,
        ),
        Some(ProtocolScript::BitCell) => {
            let p = BitCellParser::parse_parsed_cell(cell).ok_or_else(malformed)?;
            (p.identity_id, Some(p.account), IdentityStandard::BitCell)
        }
        Some(ProtocolScript::DotbitAccount) => {
            let p = DotbitParser::parse_account_parsed_cell(cell).ok_or_else(malformed)?;
            (p.account_id, p.account, IdentityStandard::DotBit)
        }
        Some(ProtocolScript::DotCellAccount) => {
            let p = DotCellParser::parse_name_parsed_cell(cell)?.ok_or_else(malformed)?;
            if p.is_root() {
                return Ok(None);
            };
            (
                p.id.to_vec(),
                Some(format!("{}.cell", p.label)),
                IdentityStandard::DotCell,
            )
        }
        _ => return Ok(None),
    };
    let mut result = CellDisplay::new("identity", standard.as_str(), &id);
    let entry = store.get_identity(&id)?;
    ensure!(
        !indexed || entry.is_some(),
        "missing indexed identity metadata {}",
        result.id
    );
    result.name = if let Some(entry) = entry {
        ensure!(
            entry.standard == standard,
            "identity {} standard mismatch",
            result.id
        );
        // A .bit account id is the immutable name hash. Its name is learned
        // from validated witnesses at indexing time, not a mutable DID document.
        if standard == IdentityStandard::DotBit {
            entry.name
        } else {
            name
        }
    } else {
        name
    };
    // Other identity names come from this outpoint's bytes. did:ckb deliberately
    // supplies no name because its mutable document is not indexed as a name.
    Ok(Some(result))
}

/// The name of the Spore cluster `cluster_id` that object `object` belongs
/// to: the cluster row's (immutable) name. `required` makes a missing cluster
/// row an error (an indexed object's cluster is always indexed).
fn spore_cluster_name(
    store: &CkbadgerStore,
    cluster_id: &[u8],
    required: bool,
    object: &str,
) -> Result<Option<String>> {
    let collection = store.get_spore(cluster_id)?;
    ensure!(
        !required || collection.is_some(),
        "missing indexed collection metadata {} for object {object}",
        hex::encode(cluster_id),
    );
    let Some(collection) = collection else {
        return Ok(None);
    };
    ensure!(
        collection.standard == ObjectStandard::SporeCluster
            && matches!(collection.extra, ObjectExtra::SporeCluster),
        "object {object} collection is not a Spore cluster",
    );
    Ok(collection.name)
}

/// A collection named by an item delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaCollection {
    pub id: String,
    pub name: Option<String>,
}

/// What an object item delta names besides its id (TX-12, TX-29): its
/// collection, its asset class and its composition tier — the facts its
/// transaction tile ([`CellDisplay`]) shows, by the same rules: a spore's
/// cluster name and media tier are immutable evidence; an m-NFT token proves
/// only its class id (the class row's name and tier can change).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDeltaContext {
    pub collection: Option<DeltaCollection>,
    /// The site's asset class (`dao | token | object | identity`): always
    /// `object` for an object item.
    pub class: &'static str,
    /// `CompositionTier::as_str` (`pure_ckb | btc_ckb | decentralized_mixture |
    /// centralized_mixture | unknown | decode_pending`) for a spore; `None`
    /// for a cluster and an m-NFT token.
    pub composition: Option<&'static str>,
    /// What the object is ([`ObjectDeltaRole::as_str`]); `None` only on a pool
    /// row whose object is not indexed yet.
    pub role: Option<&'static str>,
}

/// The object cells the activity writer emits object item deltas for
/// (`emit_object_item_deltas` over the Spore / Cluster type scripts and the
/// m-NFT token type script) — every kind a committed delta can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectDeltaRole {
    Spore,
    Cluster,
    MnftToken,
}

impl ObjectDeltaRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spore => "spore",
            Self::Cluster => "cluster",
            Self::MnftToken => "mnft_token",
        }
    }
}

/// [`ObjectDeltaContext`] of the object `object_id` (a spore, a Spore cluster
/// or an m-NFT token), read from its indexed row: each kind is described by
/// its own facts.
///
/// - spore: its cluster (immutable name; the Sole Spores sentinel with no
///   name for a clusterless spore) and its media tier;
/// - cluster: itself (its own id and immutable name), no composition — a
///   cluster holds no media;
/// - m-NFT token: its class id only (the class row's name and tier are
///   mutable, not evidence about this item).
///
/// `indexed` says whether the delta comes from a committed transaction. A
/// committed delta's object row is written in the same batch, so an id that
/// names no spore, cluster or m-NFT token is an error. A pool (pending) delta
/// may name an object whose creating transaction is the pending one: no row
/// exists yet, so nothing is known about its kind, collection or composition
/// and all three are `None` — the same rule as a pending tile ([`from_parts`]
/// with `indexed == false`).
pub fn object_delta_context(
    store: &CkbadgerStore,
    object_id: &[u8],
    indexed: bool,
) -> Result<ObjectDeltaContext> {
    let id = format!("0x{}", hex::encode(object_id));
    if let Some(entry) = store.get_spore(object_id)? {
        return match (&entry.standard, &entry.extra) {
            (ObjectStandard::Spore, ObjectExtra::Spore { media_profile, .. }) => {
                let cluster = entry.collection_id.as_deref().with_context(|| {
                    format!("spore {id} has no collection (not even Sole Spores)")
                })?;
                Ok(ObjectDeltaContext {
                    collection: Some(DeltaCollection {
                        id: format!("0x{}", hex::encode(cluster)),
                        name: if cluster == ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION
                        {
                            None
                        } else {
                            spore_cluster_name(store, cluster, true, &id)?
                        },
                    }),
                    class: "object",
                    composition: Some(media_profile.tier.as_str()),
                    role: Some(ObjectDeltaRole::Spore.as_str()),
                })
            }
            (ObjectStandard::SporeCluster, ObjectExtra::SporeCluster) => {
                // A cluster IS its collection (both writers store it with no
                // parent); one that names a parent contradicts that.
                if let Some(parent) = &entry.collection_id {
                    anyhow::bail!(
                        "Spore cluster {id} names a parent collection 0x{}",
                        hex::encode(parent)
                    );
                }
                Ok(ObjectDeltaContext {
                    collection: Some(DeltaCollection {
                        id: id.clone(),
                        name: entry.name,
                    }),
                    class: "object",
                    composition: None,
                    role: Some(ObjectDeltaRole::Cluster.as_str()),
                })
            }
            (standard, extra) => anyhow::bail!(
                "object item delta {id} names a {standard:?} row with payload {extra:?} in the spore store, \
                 neither a spore nor a Spore cluster"
            ),
        };
    }
    if let Some(entry) = store.get_mnft(object_id)? {
        ensure!(
            entry.standard == ObjectStandard::MnftToken
                && matches!(entry.extra, ObjectExtra::MnftToken { .. }),
            "object item delta {id} names a {:?} m-NFT row, not a token",
            entry.standard
        );
        let class = entry
            .collection_id
            .with_context(|| format!("m-NFT token {id} has no class"))?;
        return Ok(ObjectDeltaContext {
            collection: Some(DeltaCollection {
                id: format!("0x{}", hex::encode(&class)),
                name: None,
            }),
            class: "object",
            composition: None,
            role: Some(ObjectDeltaRole::MnftToken.as_str()),
        });
    }
    ensure!(
        !indexed,
        "object item delta {id} names no indexed spore, Spore cluster or m-NFT token"
    );
    Ok(ObjectDeltaContext {
        collection: None,
        class: "object",
        composition: None,
        role: None,
    })
}

/// The common Molecule prefix in CKBFS RFC.md / RFC.v3.md is index,
/// checksum, content_type Bytes, filename Bytes. The contract uses compatible
/// decoding, so additional fields are legal. Metadata extraction validates the
/// table and Bytes boundaries; checksum/backlinks remain consensus concerns.
/// Bytes are not necessarily UTF-8: absence of display text preserves a valid
/// file's exact identity without inventing a lossy name.
/// https://github.com/code-monad/ckbfs/blob/master/contracts/ckbfs/src/entry.rs
fn ckbfs_filename(data: &[u8]) -> Result<Option<String>> {
    let fields = molecule_fields(data)?;
    ensure!(
        fields.len() >= 4,
        "expected at least 4 CKBFS fields, got {}",
        fields.len()
    );
    molecule_bytes(fields[2])?;
    Ok(std::str::from_utf8(molecule_bytes(fields[3])?)
        .ok()
        .map(str::to_owned))
}
/// The witness a CKBFS cell's content starts at: the cell data's first field.
/// Verified against mainnet cells of both bundled deployments: the 4-field
/// table (`0xb5d13ffe…`) holds a `Uint32` (`01000000` → witness 1); the
/// 5-field table (`0x31e63762…`) holds a `Uint32` fixvec of every content
/// witness (`0300000001000000…` → [1, 2, 3]), whose head is the first entry.
/// Any other shape is an error: the contract checks the content witness the
/// index names, so a cell whose index cannot be read is not guessed at.
pub(crate) fn ckbfs_head_witness_index(data: &[u8]) -> Result<usize> {
    let fields = molecule_fields(data)?;
    let index = match fields.len() {
        4 => {
            let index = fields[0];
            ensure!(
                index.len() == 4,
                "CKBFS 4-field index must be a Uint32, got {} bytes",
                index.len()
            );
            u32_at(index, 0)?
        }
        5 => {
            let indexes = fields[0];
            let count = u32_at(indexes, 0)?;
            ensure!(
                count.checked_mul(4).and_then(|bytes| bytes.checked_add(4)) == Some(indexes.len()),
                "CKBFS 5-field index fixvec declares {count} entries in {} bytes",
                indexes.len()
            );
            ensure!(count > 0, "CKBFS 5-field index fixvec names no witness");
            u32_at(indexes, 4)?
        }
        count => anyhow::bail!("CKBFS cell data has {count} fields; expected 4 or 5"),
    };
    Ok(index)
}
fn u32_at(data: &[u8], at: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(
        data.get(at..at + 4)
            .ok_or_else(|| anyhow!("truncated Molecule header"))?
            .try_into()?,
    ) as usize)
}
fn molecule_fields(data: &[u8]) -> Result<Vec<&[u8]>> {
    ensure!(
        u32_at(data, 0)? == data.len(),
        "Molecule total size mismatch"
    );
    if data.len() == 4 {
        return Ok(vec![]);
    }
    let first = u32_at(data, 4)?;
    ensure!(
        first >= 8 && first % 4 == 0 && first <= data.len(),
        "invalid Molecule first offset {first}"
    );
    let count = first / 4 - 1;
    let mut offsets = (0..count)
        .map(|i| u32_at(data, 4 + 4 * i))
        .collect::<Result<Vec<_>>>()?;
    offsets.push(data.len());
    ensure!(
        offsets
            .windows(2)
            .all(|w| w[0] <= w[1] && w[1] <= data.len()),
        "invalid Molecule field offsets"
    );
    Ok(offsets.windows(2).map(|w| &data[w[0]..w[1]]).collect())
}
fn molecule_bytes(data: &[u8]) -> Result<&[u8]> {
    let n = u32_at(data, 0)?;
    ensure!(n == data.len() - 4, "Molecule Bytes length mismatch");
    Ok(&data[4..])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(fields: &[Vec<u8>]) -> Vec<u8> {
        let mut offset = 4 * (fields.len() + 1);
        let total = offset + fields.iter().map(Vec::len).sum::<usize>();
        let mut out = (total as u32).to_le_bytes().to_vec();
        for f in fields {
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += f.len();
        }
        for f in fields {
            out.extend_from_slice(f);
        }
        out
    }
    fn bytes(data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(data);
        out
    }
    fn fields() -> Vec<Vec<u8>> {
        vec![
            0u32.to_le_bytes().to_vec(),
            42u32.to_le_bytes().to_vec(),
            bytes(b"text/plain"),
            bytes("文件.json".as_bytes()),
        ]
    }
    #[test]
    fn ckbfs_metadata_decodes_both_bundled_schemas() {
        assert_eq!(
            ckbfs_filename(&table(&fields())).unwrap().as_deref(),
            Some("文件.json")
        );
        let mut v2 = fields();
        v2[0] = [2u32.to_le_bytes(), 3u32.to_le_bytes(), 5u32.to_le_bytes()].concat();
        let backlink = table(&[
            bytes(&[0x17; 32]),
            [1u32.to_le_bytes(), 2u32.to_le_bytes()].concat(),
            42u32.to_le_bytes().to_vec(),
        ]);
        v2.push(table(&[backlink]));
        assert_eq!(
            ckbfs_filename(&table(&v2)).unwrap().as_deref(),
            Some("文件.json")
        );
        v2[0] = 0u32.to_le_bytes().to_vec();
        v2[4] = 4u32.to_le_bytes().to_vec();
        assert_eq!(
            ckbfs_filename(&table(&v2)).unwrap().as_deref(),
            Some("文件.json")
        );
    }
    #[test]
    fn ckbfs_head_witness_index_reads_both_bundled_layouts() {
        // 4 fields: a Uint32 index.
        let mut v3 = fields();
        v3[0] = 7u32.to_le_bytes().to_vec();
        assert_eq!(ckbfs_head_witness_index(&table(&v3)).unwrap(), 7);
        // 5 fields: a Uint32 fixvec whose first entry is the head witness.
        let mut v2 = fields();
        v2[0] = [3u32, 4, 2, 9]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        v2.push(4u32.to_le_bytes().to_vec());
        assert_eq!(ckbfs_head_witness_index(&table(&v2)).unwrap(), 4);
        // A fixvec whose count disagrees with its width, or that is empty.
        v2[0] = [2u32, 4].iter().flat_map(|n| n.to_le_bytes()).collect();
        assert!(ckbfs_head_witness_index(&table(&v2)).is_err());
        v2[0] = 0u32.to_le_bytes().to_vec();
        assert!(ckbfs_head_witness_index(&table(&v2)).is_err());
        // A 4-field index that is not a Uint32, and other field counts.
        v3[0] = [1u32, 1].iter().flat_map(|n| n.to_le_bytes()).collect();
        assert!(ckbfs_head_witness_index(&table(&v3)).is_err());
        assert!(ckbfs_head_witness_index(&table(&fields()[..3])).is_err());
        assert!(ckbfs_head_witness_index(&[1, 2, 3]).is_err());
    }
    #[test]
    fn ckbfs_metadata_accepts_binary_names_but_rejects_malformed_molecule() {
        let valid = table(&fields());
        for len in 0..valid.len() {
            assert!(ckbfs_filename(&valid[..len]).is_err(), "truncated at {len}");
        }
        let mut bad = valid.clone();
        bad[4..8].copy_from_slice(&999u32.to_le_bytes());
        assert!(ckbfs_filename(&bad).is_err());
        let mut f = fields();
        f[3] = bytes(&[0xff]);
        assert!(ckbfs_filename(&table(&f)).unwrap().is_none());
        f = fields();
        f[3][0..4].copy_from_slice(&999u32.to_le_bytes());
        assert!(ckbfs_filename(&table(&f)).is_err());
        f = fields();
        f.push(vec![0xff]);
        assert_eq!(
            ckbfs_filename(&table(&f)).unwrap().as_deref(),
            Some("文件.json")
        );
        f = fields();
        f[2] = vec![0];
        assert!(ckbfs_filename(&table(&f)).is_err());
    }
    #[test]
    fn unknown_protocol_does_not_decode_lookalike_file_bytes() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let script = ScriptResponse {
            code_hash: format!("0x{}", "44".repeat(32)),
            hash_type: "data1".into(),
            args: "0x".into(),
        };
        assert!(from_parts(
            &store,
            "100000000000",
            &script,
            Some(&script),
            &table(&fields()),
            true
        )
        .unwrap()
        .is_none());
    }
    #[test]
    fn pending_spore_has_exact_identity_without_inventing_media_or_collection_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let code_hash = PROTOCOL_REGISTRY
            .iter()
            .find(|(_, p)| *p == ProtocolScript::SporeNft)
            .unwrap()
            .0;
        let script = ScriptResponse {
            code_hash: format!("0x{}", hex::encode(code_hash)),
            hash_type: "type".into(),
            args: format!("0x{}", "33".repeat(32)),
        };
        let data = table(&[bytes(b"text/plain"), bytes(b"test"), bytes(&[0x44; 32])]);
        let item = from_parts(&store, "100000000000", &script, Some(&script), &data, false)
            .unwrap()
            .unwrap();
        assert_eq!(item.kind, "object");
        assert_eq!(item.id, format!("0x{}", "33".repeat(32)));
        assert_eq!(item.collection_id, Some(format!("0x{}", "44".repeat(32))));
        assert!(
            item.name.is_none()
                && item.collection_name.is_none()
                && item.composition_tier.is_none()
        );
    }
    /// TX-12 / TX-29: an object item delta names its collection, asset class
    /// and composition tier — the facts its transaction tile shows — read
    /// from the spore and cluster rows.
    #[test]
    fn object_delta_context_names_collection_class_and_composition() {
        use ckbadger_store::{
            types::{CompositionTier, ObjectEntry, SporeMediaProfile},
            StoreBatch,
        };
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let cluster = vec![0x86; 32];
        let spore = vec![0x51; 32];
        let token = [vec![0x3c; 24], vec![0, 0, 0, 7]].concat();
        let entry =
            |standard, collection_id: Option<Vec<u8>>, name: Option<&str>, extra| ObjectEntry {
                standard,
                collection_id,
                token_id: None,
                owner_lock_hash: None,
                name: name.map(str::to_string),
                description: None,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0; 32],
                extra,
            };
        let mut batch = StoreBatch::new(&store);
        batch.put_spore(
            &cluster,
            &entry(
                ObjectStandard::SporeCluster,
                None,
                Some("Nervape"),
                ObjectExtra::SporeCluster,
            ),
        );
        batch.put_spore(
            &spore,
            &entry(
                ObjectStandard::Spore,
                Some(cluster.clone()),
                None,
                ObjectExtra::Spore {
                    content_type: "application/vnd.cell.dob@v1".into(),
                    content_length: 10,
                    media_profile: SporeMediaProfile {
                        tier: CompositionTier::BtcCkb,
                        ..Default::default()
                    },
                },
            ),
        );
        batch.put_mnft(
            &token,
            &entry(
                ObjectStandard::MnftToken,
                Some(token[..24].to_vec()),
                None,
                ObjectExtra::MnftToken {
                    token_index: 7,
                    characteristic: vec![0; 8],
                    configure: 0,
                    state: 0,
                },
            ),
        );
        batch.commit().unwrap();

        assert_eq!(
            serde_json::to_value(object_delta_context(&store, &spore, true).unwrap()).unwrap(),
            serde_json::json!({
                "collection": {"id": format!("0x{}", "86".repeat(32)), "name": "Nervape"},
                "class": "object",
                "composition": "btc_ckb",
                "role": "spore",
            })
        );
        // A cluster (its creation / transfer emits an object delta too) is its
        // own collection and holds no media.
        assert_eq!(
            serde_json::to_value(object_delta_context(&store, &cluster, true).unwrap()).unwrap(),
            serde_json::json!({
                "collection": {"id": format!("0x{}", "86".repeat(32)), "name": "Nervape"},
                "class": "object",
                "composition": null,
                "role": "cluster",
            })
        );
        // An m-NFT token names its class; the class's mutable current name and
        // tier are not evidence about this item (the tile's rule).
        assert_eq!(
            serde_json::to_value(object_delta_context(&store, &token, true).unwrap()).unwrap(),
            serde_json::json!({
                "collection": {"id": format!("0x{}", "3c".repeat(24)), "name": null},
                "class": "object",
                "composition": null,
                "role": "mnft_token",
            })
        );
        // An object delta naming no indexed object is an error, not a blank.
        let error = object_delta_context(&store, &[0x99; 32], true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("0x9999"), "{error}");
        // A pending delta's object may not be indexed yet: nothing is known
        // about it beyond its class.
        assert_eq!(
            serde_json::to_value(object_delta_context(&store, &[0x99; 32], false).unwrap())
                .unwrap(),
            serde_json::json!({
                "collection": null, "class": "object", "composition": null, "role": null,
            })
        );
        // An indexed object reads the same from a pending row.
        assert_eq!(
            object_delta_context(&store, &spore, false).unwrap(),
            object_delta_context(&store, &spore, true).unwrap()
        );
        // A spore-store row that is neither a spore nor a cluster is no object
        // the activity writer emits deltas for: an error naming it.
        let foreign = vec![0x5f; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_spore(
            &foreign,
            &entry(
                ObjectStandard::MnftIssuer,
                None,
                None,
                ObjectExtra::MnftIssuer {
                    class_count: 0,
                    set_count: 0,
                    info: None,
                },
            ),
        );
        // A cluster row that names a parent collection contradicts both writers.
        let nested = vec![0x6e; 32];
        batch.put_spore(
            &nested,
            &entry(
                ObjectStandard::SporeCluster,
                Some(cluster.clone()),
                Some("Nested"),
                ObjectExtra::SporeCluster,
            ),
        );
        batch.commit().unwrap();
        let error = object_delta_context(&store, &foreign, true)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&"5f".repeat(32)) && error.contains("MnftIssuer"),
            "{error}"
        );
        let error = object_delta_context(&store, &nested, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("parent collection 0x8686"), "{error}");
    }

    #[test]
    fn historical_mnft_does_not_borrow_mutable_current_class_metadata() {
        use ckbadger_store::{
            types::{CompositionTier, ObjectEntry},
            StoreBatch,
        };
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let id = vec![0x33; 28];
        let class = id[..24].to_vec();
        let token = ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class.clone()),
            token_id: None,
            owner_lock_hash: None,
            name: None,
            description: None,
            is_live: false,
            created_at_block: 1,
            created_at_tx: vec![0; 32],
            extra: ObjectExtra::MnftToken {
                token_index: 0x33333333,
                characteristic: vec![0; 8],
                configure: 0,
                state: 0,
            },
        };
        let current_class = ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: None,
            token_id: None,
            owner_lock_hash: None,
            name: Some("changed later".into()),
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0; 32],
            extra: ObjectExtra::MnftClass {
                description: None,
                renderer: Some("https://changed-later.example".into()),
                total: 10,
                issued: 1,
                configure: 0,
                composition_tier: CompositionTier::CentralizedMixture,
            },
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_mnft(&id, &token);
        batch.put_mnft(&class, &current_class);
        batch.commit().unwrap();
        let script = ScriptResponse {
            code_hash: ckbadger_indexer::parser::mnft::MNFT_TOKEN_CODE_HASH.into(),
            hash_type: "type".into(),
            args: format!("0x{}", hex::encode(&id)),
        };
        let item = from_parts(
            &store,
            "100000000000",
            &script,
            Some(&script),
            &[0; 11],
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(item.id, format!("0x{}", hex::encode(id)));
        assert_eq!(
            item.collection_id,
            Some(format!("0x{}", hex::encode(class)))
        );
        assert!(
            item.name.is_none()
                && item.collection_name.is_none()
                && item.composition_tier.is_none()
        );
    }
    #[test]
    fn spore_metadata_payload_variant_mismatch_is_contextual() {
        use ckbadger_store::types::ObjectEntry;
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let id = [0x33; 32];
        store
            .put_spore_direct(
                &id,
                &ObjectEntry {
                    standard: ObjectStandard::Spore,
                    collection_id: None,
                    token_id: None,
                    owner_lock_hash: None,
                    name: None,
                    description: None,
                    is_live: false,
                    created_at_block: 1,
                    created_at_tx: vec![0; 32],
                    extra: ObjectExtra::SporeCluster,
                },
            )
            .unwrap();
        let code_hash = PROTOCOL_REGISTRY
            .iter()
            .find(|(_, p)| *p == ProtocolScript::SporeNft)
            .unwrap()
            .0;
        let script = ScriptResponse {
            code_hash: format!("0x{}", hex::encode(code_hash)),
            hash_type: "type".into(),
            args: format!("0x{}", hex::encode(id)),
        };
        let data = table(&[bytes(b"text/plain"), bytes(b"test"), vec![]]);
        let error = from_parts(&store, "100000000000", &script, Some(&script), &data, true)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("payload variant") && error.contains(&hex::encode(id)),
            "{error}"
        );
    }
}
