use std::collections::{BTreeSet, HashSet};

use ckbadger_store::{
    types::{PositionedCellInfo, ScriptInfo, ScriptReferenceInfo, ScriptVersionInfo},
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
pub struct ScriptReferenceVariant {
    pub reference_hash: Vec<u8>,
    pub hash_type: u8,
    pub info: ScriptReferenceInfo,
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
    pub available_references: Vec<ScriptReferenceVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousCurrentScriptVersion {
    pub available_references: Vec<ScriptReferenceVariant>,
    pub version_hashes: Vec<Vec<u8>>,
    pub type_matches: Vec<TypeReferenceLiveMatch>,
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

pub fn list_script_reference_variants(
    store: &CkbadgerStore,
    reference_hash: &[u8],
) -> anyhow::Result<Vec<ScriptReferenceVariant>> {
    let mut variants: Vec<_> = store
        .list_script_references_by_hash(reference_hash)?
        .into_iter()
        .map(|(hash_type, info)| ScriptReferenceVariant {
            reference_hash: reference_hash.to_vec(),
            hash_type,
            info,
        })
        .collect();
    variants.sort_by_key(|variant| variant.hash_type);
    Ok(variants)
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

fn resolve_variant_current_versions(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    variant: &ScriptReferenceVariant,
) -> anyhow::Result<(Vec<Vec<u8>>, Vec<TypeReferenceLiveMatch>)> {
    if variant.hash_type == 1 {
        let type_matches =
            resolve_live_type_reference_matches(store, cells_store, &variant.reference_hash)?;
        let version_hashes: Vec<Vec<u8>> = type_matches
            .iter()
            .map(|entry| entry.version_hash.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        return Ok((version_hashes, type_matches));
    }

    Ok((vec![variant.reference_hash.clone()], Vec::new()))
}

pub fn resolve_script_version_by_reference(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    reference_hash: &[u8],
    requested_hash_type: Option<u8>,
) -> anyhow::Result<CurrentScriptVersionResolution> {
    let available_references = list_script_reference_variants(store, reference_hash)?;
    if available_references.is_empty() {
        return Ok(CurrentScriptVersionResolution::NotFound);
    }

    let selected_references: Vec<_> = available_references
        .iter()
        .filter(|variant| {
            requested_hash_type
                .map(|hash_type| variant.hash_type == hash_type)
                .unwrap_or(true)
        })
        .cloned()
        .collect();
    if selected_references.is_empty() {
        return Ok(CurrentScriptVersionResolution::NotFound);
    }

    let mut version_hashes = BTreeSet::new();
    let mut type_matches = Vec::new();

    for variant in &selected_references {
        let (resolved_versions, resolved_type_matches) =
            resolve_variant_current_versions(store, cells_store, variant)?;
        version_hashes.extend(resolved_versions);
        type_matches.extend(resolved_type_matches);
    }

    let version_hashes: Vec<Vec<u8>> = version_hashes.into_iter().collect();
    if version_hashes.is_empty() {
        return Ok(CurrentScriptVersionResolution::NotFound);
    }
    if version_hashes.len() > 1 {
        return Ok(CurrentScriptVersionResolution::Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion {
                available_references,
                version_hashes,
                type_matches,
            },
        )));
    }

    let version_hash = version_hashes[0].clone();
    let version_info = store.get_script_version(&version_hash)?;
    Ok(CurrentScriptVersionResolution::Resolved(Box::new(
        CurrentScriptVersion {
            version_hash,
            version_info,
            available_references,
        },
    )))
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

pub fn list_current_references_for_version(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    version_hash: &[u8],
) -> anyhow::Result<Vec<ScriptReferenceVariant>> {
    let mut matches = Vec::new();
    for ((reference_hash, hash_type), info) in store.list_script_references()? {
        let variant = ScriptReferenceVariant {
            reference_hash: reference_hash.clone(),
            hash_type,
            info,
        };
        let (resolved_versions, _) =
            resolve_variant_current_versions(store, cells_store, &variant)?;
        if resolved_versions.len() == 1 && resolved_versions[0] == version_hash {
            matches.push(variant);
        }
    }
    matches.sort_by(|left, right| {
        left.reference_hash
            .cmp(&right.reference_hash)
            .then_with(|| left.hash_type.cmp(&right.hash_type))
    });
    Ok(matches)
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
    fn test_resolve_script_version_by_reference_returns_ambiguity_for_live_type_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference_hash = vec![0x33; 32];
        let version_hash_a = vec![0x44; 32];
        let version_hash_b = vec![0x55; 32];

        store
            .put_script_reference(
                &reference_hash,
                1,
                &ScriptReferenceInfo {
                    reference_hash: reference_hash.clone(),
                    hash_type: 1,
                    ..Default::default()
                },
            )
            .unwrap();

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

        let resolution =
            resolve_script_version_by_reference(&store, &store, &reference_hash, None).unwrap();
        match resolution {
            CurrentScriptVersionResolution::Ambiguous(ambiguous) => {
                assert_eq!(
                    ambiguous.version_hashes,
                    vec![version_hash_a, version_hash_b]
                );
                assert_eq!(ambiguous.type_matches.len(), 2);
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }
}
