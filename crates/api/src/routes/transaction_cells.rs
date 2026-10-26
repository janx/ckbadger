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
                let collection = store.get_spore(&collection_id)?;
                ensure!(
                    !indexed || collection.is_some(),
                    "missing indexed collection metadata {} for object {}",
                    hex::encode(&collection_id),
                    result.id
                );
                if let Some(collection) = collection {
                    ensure!(
                        collection.standard == ObjectStandard::SporeCluster
                            && matches!(collection.extra, ObjectExtra::SporeCluster),
                        "object {} collection is not a Spore cluster",
                        result.id
                    );
                    result.collection_name = collection.name;
                }
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
