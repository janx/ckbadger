use std::collections::HashSet;

use ckbadger_store::types::ScriptInfo;

const UNKNOWN_SCRIPT_NAME: &str = "unknown";

pub fn is_known_script_name(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(UNKNOWN_SCRIPT_NAME)
}

fn hash_type_to_u8(hash_type: &str) -> Option<u8> {
    match hash_type {
        "data" => Some(0),
        "type" => Some(1),
        "data1" => Some(2),
        "data2" => Some(4),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
