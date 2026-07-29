use std::collections::{HashMap, HashSet};

use ckbadger_store::{
    types::{PositionedCellInfo, ScriptInfo, ScriptVersionInfo},
    CkbadgerStore,
};

const UNKNOWN_SCRIPT_NAME: &str = "unknown";

pub fn is_known_script_name(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(UNKNOWN_SCRIPT_NAME)
}

pub fn hash_type_to_u8(hash_type: &str) -> Option<u8> {
    match hash_type {
        "data" => Some(0),
        "type" => Some(1),
        "data1" => Some(2),
        "data2" => Some(4),
        _ => None,
    }
}

pub fn hash_type_to_string(hash_type: u8) -> Option<&'static str> {
    match hash_type {
        0 => Some("data"),
        1 => Some("type"),
        2 => Some("data1"),
        4 => Some("data2"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeReferenceLiveMatch {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub version_hash: Vec<u8>,
}

pub type VersionCodeCell = (Vec<u8>, i16, PositionedCellInfo, bool);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentScriptVersion {
    pub version_hash: Vec<u8>,
    pub version_info: Option<ScriptVersionInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousCurrentScriptVersion {
    pub version_hashes: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentScriptVersionResolution {
    NotFound,
    Resolved(Box<CurrentScriptVersion>),
    Ambiguous(Box<AmbiguousCurrentScriptVersion>),
}

fn matches_any_reference(info: &ScriptInfo, refs: &HashSet<Vec<u8>>) -> bool {
    refs.contains(&info.code_hash)
        || info
            .dep_type_hash
            .as_ref()
            .map(|h| refs.contains(h))
            .unwrap_or(false)
        || info
            .dep_data_hash
            .as_ref()
            .map(|h| refs.contains(h))
            .unwrap_or(false)
}

fn extend_references(info: &ScriptInfo, refs: &mut HashSet<Vec<u8>>) -> bool {
    let mut changed = false;

    if refs.insert(info.code_hash.clone()) {
        changed = true;
    }
    if let Some(dep_type_hash) = &info.dep_type_hash {
        if refs.insert(dep_type_hash.clone()) {
            changed = true;
        }
    }
    if let Some(dep_data_hash) = &info.dep_data_hash {
        if refs.insert(dep_data_hash.clone()) {
            changed = true;
        }
    }

    changed
}

fn collect_related_infos<'a>(
    all_infos: &'a [ScriptInfo],
    reference_hash: &[u8],
) -> Vec<&'a ScriptInfo> {
    let mut refs = HashSet::new();
    refs.insert(reference_hash.to_vec());

    let mut seen_codes = HashSet::new();
    let mut related = Vec::new();

    loop {
        let mut changed = false;

        for info in all_infos {
            if seen_codes.contains(&info.code_hash) {
                continue;
            }
            if matches_any_reference(info, &refs) {
                seen_codes.insert(info.code_hash.clone());
                related.push(info);
                extend_references(info, &mut refs);
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    related
}

fn preferred_info<'a>(infos: &[&'a ScriptInfo], reference_hash: &[u8]) -> &'a ScriptInfo {
    infos
        .iter()
        .copied()
        .max_by_key(|info| {
            (
                u8::from(is_known_script_name(info.name.as_deref())),
                u8::from(info.hash_type == 1),
                u8::from(info.dep_type_hash.is_some()),
                u8::from(info.code_cell_tx_hash.is_some() && info.code_cell_output_index.is_some()),
                u8::from(info.code_hash == reference_hash),
            )
        })
        .unwrap_or(infos[0])
}

pub fn related_code_hashes_for_reference(
    all_infos: &[ScriptInfo],
    reference_hash: &[u8],
) -> Vec<Vec<u8>> {
    let mut seen = HashSet::new();
    let mut hashes = Vec::new();

    for info in collect_related_infos(all_infos, reference_hash) {
        if seen.insert(info.code_hash.clone()) {
            hashes.push(info.code_hash.clone());
        }
    }

    hashes
}

pub fn merge_script_info_for_reference(
    all_infos: &[ScriptInfo],
    reference_hash: &[u8],
) -> Option<ScriptInfo> {
    let related = collect_related_infos(all_infos, reference_hash);
    if related.is_empty() {
        return None;
    }

    let direct = related
        .iter()
        .copied()
        .find(|info| info.code_hash == reference_hash);
    let preferred = preferred_info(&related, reference_hash);

    let mut merged = direct.cloned().unwrap_or_else(|| preferred.clone());
    merged.code_hash = reference_hash.to_vec();

    if !is_known_script_name(merged.name.as_deref()) {
        merged.name = related
            .iter()
            .find_map(|info| {
                if is_known_script_name(info.name.as_deref()) {
                    info.name.clone()
                } else {
                    None
                }
            })
            .or_else(|| preferred.name.clone());
    }
    if !merged.deprecated {
        merged.deprecated = related.iter().any(|info| info.deprecated);
    }

    if merged.description.is_none() {
        merged.description = preferred.description.clone();
    }
    if merged.website.is_none() {
        merged.website = preferred.website.clone();
    }
    if merged.category.is_none() {
        merged.category = preferred.category.clone();
    }

    if merged.dep_type_hash.is_none() {
        merged.dep_type_hash = preferred
            .dep_type_hash
            .clone()
            .or_else(|| related.iter().find_map(|info| info.dep_type_hash.clone()));
    }
    if merged.dep_data_hash.is_none() {
        merged.dep_data_hash = preferred
            .dep_data_hash
            .clone()
            .or_else(|| related.iter().find_map(|info| info.dep_data_hash.clone()));
    }

    if merged.code_cell_tx_hash.is_none() || merged.code_cell_output_index.is_none() {
        if let Some(info) = related
            .iter()
            .copied()
            .find(|info| info.code_cell_tx_hash.is_some() && info.code_cell_output_index.is_some())
        {
            merged.code_cell_tx_hash = info.code_cell_tx_hash.clone();
            merged.code_cell_output_index = info.code_cell_output_index;
        }
    }

    Some(merged)
}

pub fn resolve_code_hash_for_hash_type(
    all_infos: &[ScriptInfo],
    reference_hash: &[u8],
    hash_type: &str,
) -> Option<Vec<u8>> {
    let hash_type_u8 = hash_type_to_u8(hash_type)?;
    let related = collect_related_infos(all_infos, reference_hash);
    if related.is_empty() {
        return None;
    }

    if let Some(info) = related
        .iter()
        .copied()
        .find(|info| info.hash_type == hash_type_u8)
    {
        return Some(info.code_hash.clone());
    }

    // "type" is a strict capability: if no type reference exists in the deployment,
    // do not silently fall back to a data-family reference.
    if hash_type_u8 == 1 {
        return None;
    }

    if related
        .iter()
        .any(|info| info.code_hash.as_slice() == reference_hash)
    {
        return Some(reference_hash.to_vec());
    }

    Some(preferred_info(&related, reference_hash).code_hash.clone())
}

pub fn deployment_key_for_script(info: &ScriptInfo) -> Vec<u8> {
    if let Some(dep_type_hash) = &info.dep_type_hash {
        return dep_type_hash.clone();
    }
    if info.hash_type == 1 {
        return info.code_hash.clone();
    }
    if let Some(dep_data_hash) = &info.dep_data_hash {
        return dep_data_hash.clone();
    }
    info.code_hash.clone()
}

pub fn deployment_reference_hashes(info: &ScriptInfo) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let type_ref = info
        .dep_type_hash
        .clone()
        .or_else(|| (info.hash_type == 1).then(|| info.code_hash.clone()));

    let data_ref = info.dep_data_hash.clone().or_else(|| {
        (info.hash_type == 0 || info.hash_type == 2 || info.hash_type == 4)
            .then(|| info.code_hash.clone())
    });

    (type_ref, data_ref)
}

/// Minimal cell metadata needed for per-transaction script resolution.
#[derive(Debug, Clone)]
pub struct DepCellInfo {
    pub type_script_hash: Option<Vec<u8>>,
    pub data_hash: Option<Vec<u8>>,
}

/// Build a code_hash -> version_hash mapping from a transaction's resolved dep cells.
///
/// For type references: type_script_hash -> data_hash (the bytecode version).
/// For data references: data_hash -> data_hash (identity -- the code_hash IS the version).
///
/// First-seen wins for defensive correctness. In valid CKB transactions, each type_script_hash
/// appears at most once in cell_deps (CKB consensus rejects duplicates).
pub fn build_dep_cell_mappings(dep_cells: &[DepCellInfo]) -> HashMap<Vec<u8>, Vec<u8>> {
    let mut mappings: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    for cell in dep_cells {
        if let (Some(type_hash), Some(data_hash)) = (&cell.type_script_hash, &cell.data_hash) {
            mappings
                .entry(type_hash.clone())
                .or_insert_with(|| data_hash.clone());
        }
        if let Some(data_hash) = &cell.data_hash {
            mappings
                .entry(data_hash.clone())
                .or_insert_with(|| data_hash.clone());
        }
    }

    mappings
}

/// Read a cell's metadata from ckbadger's append-only store by outpoint.
fn read_dep_cell_info(
    cells_store: &CkbadgerStore,
    tx_hash: &[u8],
    output_index: i16,
) -> Option<DepCellInfo> {
    let key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);
    let info = cells_store.get_cell_by_outpoint_key(&key).ok()??;
    Some(DepCellInfo {
        type_script_hash: info.type_script_hash,
        data_hash: info.data_hash,
    })
}

/// Expand a dep_group cell into its constituent outpoints.
fn expand_dep_group(
    ckb_store: &ckb_store_reader::CkbChainReader,
    tx_hash_bytes: &[u8; 32],
    output_index: u32,
) -> Vec<(Vec<u8>, i16)> {
    let data = match ckb_store.get_cell_data(tx_hash_bytes, output_index) {
        Some(d) => d,
        None => return vec![],
    };
    let result = crate::routes::cells::parse_dep_group(&data, data.len() as i32);
    let items = match result.items {
        Some(items) if result.is_dep_group => items,
        _ => return vec![],
    };
    items
        .into_iter()
        .filter_map(|item| {
            let hash =
                hex::decode(item.tx_hash.strip_prefix("0x").unwrap_or(&item.tx_hash)).ok()?;
            let index = i16::try_from(item.output_index).ok()?;
            Some((hash, index))
        })
        .collect()
}

/// Resolve all dep cells for a transaction, building a code_hash -> version_hash mapping.
///
/// Returns `None` if the CKB store is unavailable or the transaction is not found.
/// Individual dep cells that can't be read are silently skipped --
/// the caller should fall back to global resolution for any code_hash not in the mapping.
pub fn resolve_dep_cells_for_transaction(
    state: &crate::AppState,
    tx_hash_hex: &str,
) -> Option<HashMap<Vec<u8>, Vec<u8>>> {
    let ckb_store = state.ckb_store.as_ref()?;
    let tx_hash_bytes_vec =
        hex::decode(tx_hash_hex.strip_prefix("0x").unwrap_or(tx_hash_hex)).ok()?;
    if tx_hash_bytes_vec.len() != 32 {
        return None;
    }
    let mut tx_hash_arr = [0u8; 32];
    tx_hash_arr.copy_from_slice(&tx_hash_bytes_vec);

    let tx_view = ckb_store.get_transaction(&tx_hash_arr)?;
    let rpc_tx = ckb_store_reader::convert_transaction_view(&tx_view);

    let mut outpoints: Vec<(Vec<u8>, i16)> = Vec::new();

    for dep in &rpc_tx.cell_deps {
        let dep_tx_hash = hex::decode(
            dep.out_point
                .tx_hash
                .strip_prefix("0x")
                .unwrap_or(&dep.out_point.tx_hash),
        )
        .ok()?;
        let dep_index_str = dep
            .out_point
            .index
            .strip_prefix("0x")
            .unwrap_or(&dep.out_point.index);
        let dep_index = u32::from_str_radix(dep_index_str, 16).ok()?;

        if dep.dep_type == "dep_group" {
            if dep_tx_hash.len() == 32 {
                let mut dep_tx_arr = [0u8; 32];
                dep_tx_arr.copy_from_slice(&dep_tx_hash);
                let expanded = expand_dep_group(ckb_store, &dep_tx_arr, dep_index);
                outpoints.extend(expanded);
            }
        } else {
            let index = i16::try_from(dep_index).ok()?;
            outpoints.push((dep_tx_hash, index));
        }
    }

    let dep_cells: Vec<DepCellInfo> = outpoints
        .iter()
        .filter_map(|(tx_hash, idx)| read_dep_cell_info(&state.append_only_store, tx_hash, *idx))
        .collect();

    Some(build_dep_cell_mappings(&dep_cells))
}

pub fn resolve_live_type_reference_matches(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    reference_hash: &[u8],
) -> anyhow::Result<Vec<TypeReferenceLiveMatch>> {
    let mut matches = Vec::new();
    for (tx_hash, output_index, cell) in
        store.list_cells_by_type(reference_hash, usize::MAX, None, cells_store)?
    {
        let version_hash = cell.cell.data_hash.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "live type-referenced code cell is missing data_hash: reference_hash=0x{}, outpoint=0x{}:{}",
                hex::encode(reference_hash),
                hex::encode(&tx_hash),
                output_index
            )
        })?;
        matches.push(TypeReferenceLiveMatch {
            tx_hash,
            output_index,
            version_hash,
        });
    }
    Ok(matches)
}

/// Return the persisted reference->version mapping when it constitutes a real
/// resolution candidate.
///
/// Type mappings (hash_type=1) are always candidates: they are only written
/// from resolved dep cells (live path) or observed live code-cell versions
/// (bulk path).
///
/// Data-family mappings (hash_type 0/2/4) are self-mappings written on ANY
/// observed data-form usage — including junk locks that reuse a type-reference
/// hash with a data hash_type even though no binary with that data hash exists
/// on chain. Such a mapping only counts as a candidate when at least one code
/// cell (live or consumed) carries data whose hash equals the version hash,
/// read from the same CF_CELL_BY_DATA_HASH index the code-cells endpoint uses.
pub fn persisted_reference_version_candidate(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(version_hash) = store.get_script_reference_version_hash(hash_type, reference_hash)?
    else {
        return Ok(None);
    };
    if matches!(hash_type, 0 | 2 | 4)
        && store
            .find_any_cell_by_data_hash(&version_hash, cells_store)?
            .is_none()
    {
        return Ok(None);
    }
    Ok(Some(version_hash))
}

/// Resolve which version an observed reference form belongs to, for
/// family-membership purposes.
///
/// This is THE single membership computation shared by the family detail
/// observed-reference grouping, family capacity-history chart aggregation and
/// most-utilized chart grouping: first the persisted candidate (validated by
/// [`persisted_reference_version_candidate`]), then — for data-family forms —
/// the reference hash itself when the caller recognizes it as an admissible
/// version hash (self usage of a known version's binary).
pub fn reference_form_member_version(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
    is_allowed_version: &dyn Fn(&[u8]) -> bool,
) -> anyhow::Result<Option<Vec<u8>>> {
    if let Some(version_hash) =
        persisted_reference_version_candidate(store, cells_store, hash_type, reference_hash)?
    {
        return Ok(Some(version_hash));
    }
    if matches!(hash_type, 0 | 2 | 4) && is_allowed_version(reference_hash) {
        return Ok(Some(reference_hash.to_vec()));
    }
    Ok(None)
}

/// Resolve ONE observed reference form -- the exact (reference_hash,
/// hash_type) pair -- to its version.
///
/// Unlike [`resolve_script_by_hash`], which answers "what does this hash mean
/// anywhere on chain" and therefore spans every form of a deployment, this
/// answers "what does this hash mean when used with this hash_type". It is the
/// resolution behind the code-cell endpoints when the caller supplies a
/// hash_type, so a junk data form that reuses a type reference's bytes cannot
/// borrow the real deployment's code cells.
///
/// Membership itself is not recomputed here: it reuses
/// [`reference_form_member_version`] (persisted candidate, then a data-family
/// self-reference) with "a code cell carries this bytecode" as the
/// admissible-version rule, then -- for type forms only, which have no
/// self-reference branch -- falls back to live type-referenced code cells.
pub fn resolve_script_form_by_hash(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    hash_type: u8,
    reference_hash: &[u8],
) -> anyhow::Result<CurrentScriptVersionResolution> {
    if hash_type_to_string(hash_type).is_none() {
        anyhow::bail!(
            "cannot resolve a script reference form with unknown hash_type: reference_hash=0x{}, hash_type={}",
            hex::encode(reference_hash),
            hash_type
        );
    }

    let has_own_code_cell = store
        .find_any_cell_by_data_hash(reference_hash, cells_store)?
        .is_some();
    let member_version = reference_form_member_version(
        store,
        cells_store,
        hash_type,
        reference_hash,
        &|hash: &[u8]| has_own_code_cell && hash == reference_hash,
    )?;
    if let Some(version_hash) = member_version {
        let version_info = store.get_script_version(&version_hash)?;
        return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
            CurrentScriptVersion {
                version_hash,
                version_info,
            },
        )));
    }

    if hash_type != 1 {
        return Ok(CurrentScriptVersionResolution::NotFound);
    }

    let type_matches = resolve_live_type_reference_matches(store, cells_store, reference_hash)?;
    let unique_versions: Vec<Vec<u8>> = {
        let mut seen = HashSet::new();
        type_matches
            .iter()
            .filter(|m| seen.insert(m.version_hash.clone()))
            .map(|m| m.version_hash.clone())
            .collect()
    };
    match unique_versions.len() {
        0 => Ok(CurrentScriptVersionResolution::NotFound),
        1 => {
            let version_hash = unique_versions[0].clone();
            let version_info = store.get_script_version(&version_hash)?;
            Ok(CurrentScriptVersionResolution::Resolved(Box::new(
                CurrentScriptVersion {
                    version_hash,
                    version_info,
                },
            )))
        }
        _ => Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion {
                version_hashes: unique_versions,
            },
        ))),
    }
}

/// Resolve a script hash to a version using cell indexes.
///
/// Resolution order:
/// 1. Type reference: code cells whose type_script_hash matches -> data_hash is version
/// 2. Data-family: CF_SCRIPT_INFO has an entry -> reference_hash is the version
/// 3. Direct version lookup: CF_SCRIPT_VERSIONS has an entry (from labels)
pub fn resolve_script_by_hash(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    reference_hash: &[u8],
) -> anyhow::Result<CurrentScriptVersionResolution> {
    let direct_version = store.get_script_version(reference_hash)?;
    let persisted_versions = {
        let mut seen = HashSet::new();
        let mut versions = Vec::new();
        for hash_type in [0u8, 1u8, 2u8, 4u8] {
            if let Some(version_hash) = persisted_reference_version_candidate(
                store,
                cells_store,
                hash_type,
                reference_hash,
            )? {
                if seen.insert(version_hash.clone()) {
                    versions.push(version_hash);
                }
            }
        }
        versions
    };

    if persisted_versions.len() == 1 {
        let version_hash = persisted_versions[0].clone();
        let version_info = store.get_script_version(&version_hash)?;
        return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
            CurrentScriptVersion {
                version_hash,
                version_info,
            },
        )));
    }

    if persisted_versions.len() > 1 {
        return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion {
                version_hashes: persisted_versions,
            },
        )));
    }

    let type_matches = resolve_live_type_reference_matches(store, cells_store, reference_hash)?;
    if let Some(version_info) = direct_version {
        let mut unique_versions: Vec<Vec<u8>> = {
            let mut seen = HashSet::new();
            type_matches
                .iter()
                .filter(|m| seen.insert(m.version_hash.clone()))
                .map(|m| m.version_hash.clone())
                .collect()
        };
        if unique_versions.is_empty() {
            return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
                CurrentScriptVersion {
                    version_hash: reference_hash.to_vec(),
                    version_info: Some(version_info),
                },
            )));
        }
        if unique_versions.len() == 1 && unique_versions[0] == reference_hash {
            return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
                CurrentScriptVersion {
                    version_hash: reference_hash.to_vec(),
                    version_info: Some(version_info),
                },
            )));
        }
        if !unique_versions.iter().any(|hash| hash == reference_hash) {
            unique_versions.push(reference_hash.to_vec());
        }
        unique_versions.sort();
        return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion {
                version_hashes: unique_versions,
            },
        )));
    }
    if !type_matches.is_empty() {
        let unique_versions: Vec<Vec<u8>> = {
            let mut seen = HashSet::new();
            type_matches
                .iter()
                .filter(|m| seen.insert(m.version_hash.clone()))
                .map(|m| m.version_hash.clone())
                .collect()
        };
        if unique_versions.len() > 1 {
            return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
                AmbiguousCurrentScriptVersion {
                    version_hashes: unique_versions,
                },
            )));
        }
        let version_hash = unique_versions[0].clone();
        let version_info = store.get_script_version(&version_hash)?;
        return Ok(CurrentScriptVersionResolution::Resolved(Box::new(
            CurrentScriptVersion {
                version_hash,
                version_info,
            },
        )));
    }

    Ok(CurrentScriptVersionResolution::NotFound)
}

pub fn list_version_code_cells(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    version_hash: &[u8],
) -> anyhow::Result<Vec<VersionCodeCell>> {
    let mut code_cells = store.list_all_cells_by_data_hash(version_hash, cells_store)?;
    code_cells.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| left.2.created_at_block.cmp(&right.2.created_at_block))
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    Ok(code_cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::{batch::StoreBatch, types::LiveCellInfo, CkbadgerStore};

    #[test]
    fn test_merge_reference_uses_known_metadata() {
        let data_hash = vec![0x70; 32];
        let type_hash = vec![0x9b; 32];

        let data_ref = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0,
            name: None,
            lock_live_cells_count: 7,
            ..Default::default()
        };

        let type_ref = ScriptInfo {
            code_hash: type_hash.clone(),
            hash_type: 1,
            name: Some("SECP256K1_BLAKE160".to_string()),
            dep_data_hash: Some(data_hash.clone()),
            dep_type_hash: Some(type_hash.clone()),
            ..Default::default()
        };

        let merged = merge_script_info_for_reference(&[data_ref, type_ref], &data_hash).unwrap();
        assert_eq!(merged.code_hash, data_hash);
        assert_eq!(merged.hash_type, 0);
        assert_eq!(merged.name.as_deref(), Some("SECP256K1_BLAKE160"));
        assert_eq!(merged.dep_type_hash, Some(type_hash));
    }

    #[test]
    fn test_resolve_code_hash_for_hash_type_switches_reference() {
        let data_hash = vec![0x70; 32];
        let type_hash = vec![0x9b; 32];

        let data_ref = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0,
            ..Default::default()
        };
        let type_ref = ScriptInfo {
            code_hash: type_hash.clone(),
            hash_type: 1,
            dep_data_hash: Some(data_hash.clone()),
            dep_type_hash: Some(type_hash.clone()),
            ..Default::default()
        };

        let all = [data_ref, type_ref];
        assert_eq!(
            resolve_code_hash_for_hash_type(&all, &data_hash, "data"),
            Some(data_hash.clone())
        );
        assert_eq!(
            resolve_code_hash_for_hash_type(&all, &data_hash, "type"),
            Some(type_hash)
        );
    }

    #[test]
    fn test_resolve_code_hash_for_hash_type_does_not_fallback_to_data_for_type_request() {
        let data_hash = vec![0x70; 32];
        let data_only = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0,
            ..Default::default()
        };

        assert_eq!(
            resolve_code_hash_for_hash_type(&[data_only], &data_hash, "type"),
            None
        );
    }

    #[test]
    fn test_resolve_script_by_hash_returns_ambiguity_for_live_type_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x33; 32];
        let version_hash_a = vec![0x44; 32];
        let version_hash_b = vec![0x55; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x70; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(version_hash_a.clone()),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x70; 32], 0);
        batch.put_cell(
            &[0x71; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x21; 32],
                lock_code_hash: vec![0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x23; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(version_hash_b.clone()),
            },
            11,
        );
        batch.put_cell_by_type(&reference_hash, 11, &[0x71; 32], 0);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Ambiguous(ambiguous) => {
                assert_eq!(
                    ambiguous.version_hashes,
                    vec![version_hash_a, version_hash_b]
                );
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_uses_persisted_mapping_before_live_type_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x63; 32];
        let mapped_version_hash = vec![0x74; 32];
        let conflicting_version_hash = vec![0x85; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &mapped_version_hash)
            .unwrap();
        store
            .put_script_version(
                &mapped_version_hash,
                &ScriptVersionInfo {
                    version_hash: mapped_version_hash.clone(),
                    name: Some("Mapped Version".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x90; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(mapped_version_hash.clone()),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x90; 32], 0);
        batch.put_cell(
            &[0x91; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x21; 32],
                lock_code_hash: vec![0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x23; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(conflicting_version_hash),
            },
            11,
        );
        batch.put_cell_by_type(&reference_hash, 11, &[0x91; 32], 0);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, mapped_version_hash);
                assert_eq!(
                    resolved
                        .version_info
                        .as_ref()
                        .and_then(|info| info.name.as_deref()),
                    Some("Mapped Version")
                );
            }
            other => panic!("expected persisted mapping to win, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_resolves_unique_live_type_match_without_persisted_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x47; 32];
        let version_hash = vec![0x58; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x92; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(version_hash.clone()),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x92; 32], 0);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
                assert!(resolved.version_info.is_none());
            }
            other => panic!("expected unique live type match resolution, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_ignores_data_self_mapping_without_code_cell() {
        // Mainnet secp junk-lock scenario: the persisted type-form mapping
        // resolves the reference to the real bytecode version, while a garbage
        // data-form self-mapping exists for the same reference bytes even
        // though no on-chain code cell carries data whose hash equals the
        // reference. The junk self-mapping must not create ambiguity.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x9b; 32];
        let version_hash = vec![0x70; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &version_hash)
            .unwrap();
        // Garbage self-mapping written from data-form usage of the same bytes.
        store
            .put_script_reference_to_version_direct(0, &reference_hash, &reference_hash)
            .unwrap();
        store
            .put_script_version(
                &version_hash,
                &ScriptVersionInfo {
                    version_hash: version_hash.clone(),
                    name: Some("SECP256K1_BLAKE160".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
                assert_eq!(
                    resolved
                        .version_info
                        .as_ref()
                        .and_then(|info| info.name.as_deref()),
                    Some("SECP256K1_BLAKE160")
                );
            }
            other => panic!("expected junk data self-mapping to be ignored, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_by_hash_keeps_data_self_mapping_backed_by_code_cell() {
        // A data-form self-mapping whose bytecode exists on chain (a code cell
        // with matching data hash) must stay resolvable.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x70; 32];
        let code_cell_tx = vec![0x77; 32];

        store
            .put_script_reference_to_version_direct(0, &reference_hash, &reference_hash)
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &code_cell_tx,
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 4,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(reference_hash.clone()),
            },
            5,
        );
        batch.put_cell_by_data_hash(&reference_hash, 5, &code_cell_tx, 0);
        batch.commit().unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, reference_hash);
            }
            other => {
                panic!("expected code-cell-backed data self-mapping to resolve, got {other:?}")
            }
        }
    }

    #[test]
    fn test_resolve_script_form_separates_type_and_junk_data_forms() {
        // Mainnet secp junk-lock scenario, asked per form: the type form
        // resolves to the real bytecode version, while the data form of the
        // same reference bytes -- whose binary does not exist on chain --
        // resolves to nothing instead of borrowing the type form's version.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x9b; 32];
        let version_hash = vec![0x70; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &version_hash)
            .unwrap();
        store
            .put_script_reference_to_version_direct(0, &reference_hash, &reference_hash)
            .unwrap();

        match resolve_script_form_by_hash(&store, &store, 1, &reference_hash).unwrap() {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
            }
            other => panic!("expected the type form to resolve, got {other:?}"),
        }
        assert_eq!(
            resolve_script_form_by_hash(&store, &store, 0, &reference_hash).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_script_form_resolves_data_form_backed_by_code_cell() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x70; 32];
        let code_cell_tx = vec![0x77; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &code_cell_tx,
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 4,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(reference_hash.clone()),
            },
            5,
        );
        batch.put_cell_by_data_hash(&reference_hash, 5, &code_cell_tx, 0);
        batch.commit().unwrap();

        match resolve_script_form_by_hash(&store, &store, 0, &reference_hash).unwrap() {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, reference_hash);
            }
            other => panic!("expected the code-cell-backed data form to resolve, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_script_form_uses_live_type_matches_without_persisted_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x47; 32];
        let version_hash = vec![0x58; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0x92; 32],
            0,
            &LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x12; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(reference_hash.clone()),
                type_code_hash: Some(vec![0x13; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 0,
                occupied_capacity: 80,
                udt_amount: None,
                data_hash: Some(version_hash.clone()),
            },
            10,
        );
        batch.put_cell_by_type(&reference_hash, 10, &[0x92; 32], 0);
        batch.commit().unwrap();

        match resolve_script_form_by_hash(&store, &store, 1, &reference_hash).unwrap() {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, version_hash);
            }
            other => panic!("expected the live type match to resolve, got {other:?}"),
        }
        // The same bytes used as a data form have no binary on chain.
        assert_eq!(
            resolve_script_form_by_hash(&store, &store, 0, &reference_hash).unwrap(),
            CurrentScriptVersionResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_script_form_rejects_unknown_hash_type() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let err = resolve_script_form_by_hash(&store, &store, 3, &[0x9b; 32]).unwrap_err();
        assert!(err.to_string().contains("unknown hash_type"), "{err}");
    }

    #[test]
    fn test_build_dep_cell_mappings_type_and_data() {
        let type_hash = vec![0x9b; 32];
        let data_hash = vec![0x70; 32];

        let dep_cells = vec![DepCellInfo {
            type_script_hash: Some(type_hash.clone()),
            data_hash: Some(data_hash.clone()),
        }];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.get(&type_hash), Some(&data_hash));
        assert_eq!(mappings.get(&data_hash), Some(&data_hash));
    }

    #[test]
    fn test_build_dep_cell_mappings_first_seen_wins() {
        let type_hash = vec![0x9b; 32];
        let data_hash_a = vec![0x70; 32];
        let data_hash_b = vec![0x71; 32];

        let dep_cells = vec![
            DepCellInfo {
                type_script_hash: Some(type_hash.clone()),
                data_hash: Some(data_hash_a.clone()),
            },
            DepCellInfo {
                type_script_hash: Some(type_hash.clone()),
                data_hash: Some(data_hash_b.clone()),
            },
        ];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.get(&type_hash), Some(&data_hash_a));
    }

    #[test]
    fn test_build_dep_cell_mappings_skips_none_fields() {
        let dep_cells = vec![
            DepCellInfo {
                type_script_hash: None,
                data_hash: None,
            },
            DepCellInfo {
                type_script_hash: Some(vec![0xAA; 32]),
                data_hash: None,
            },
            DepCellInfo {
                type_script_hash: None,
                data_hash: Some(vec![0xBB; 32]),
            },
        ];

        let mappings = build_dep_cell_mappings(&dep_cells);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings.get(&vec![0xBB; 32]), Some(&vec![0xBB; 32]));
        assert!(!mappings.contains_key(&vec![0xAA; 32]));
    }

    #[test]
    fn test_resolve_script_by_hash_resolves_direct_version_hash_without_persisted_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x52; 32];

        store
            .put_script_info_direct(
                &reference_hash,
                &ScriptInfo {
                    code_hash: reference_hash.clone(),
                    hash_type: 0,
                    lock_cells_count: 1,
                    lock_live_cells_count: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_version(
                &reference_hash,
                &ScriptVersionInfo {
                    version_hash: reference_hash.clone(),
                    name: Some("Legacy Fallback".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let resolution = resolve_script_by_hash(&store, &store, &reference_hash).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Resolved(resolved) => {
                assert_eq!(resolved.version_hash, reference_hash);
                assert_eq!(
                    resolved
                        .version_info
                        .as_ref()
                        .and_then(|info| info.name.as_deref()),
                    Some("Legacy Fallback")
                );
            }
            other => panic!("expected direct version resolution, got {other:?}"),
        }
    }
}
