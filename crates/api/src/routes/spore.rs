use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::assets::{
    count_nft_collection_activities_cached, list_canonical_nft_collection_activities_page,
};
use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{
    default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
};
use crate::utils::{apply_live_capacity_delta, date_keys_inclusive, parse_chart_date_range};
use crate::warmup::{CachedAssetEntry, CACHE_KEY_ASSETS_NFT, CACHE_KEY_SPORES_ALL};
use crate::AppState;
use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
type CachedSporeRows = Vec<(Vec<u8>, ckbadger_store::ObjectEntry)>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/spore/clusters", get(list_clusters))
        .route("/spore/clusters/{cluster_id}", get(get_cluster))
        .route(
            "/spore/clusters/{cluster_id}/charts/capacity-history",
            get(get_cluster_capacity_chart),
        )
        .route(
            "/spore/clusters/{cluster_id}/holders",
            get(get_cluster_holders),
        )
        .route(
            "/spore/clusters/{cluster_id}/activities",
            get(get_cluster_activities),
        )
        .route(
            "/spore/clusters/{cluster_id}/spores",
            get(get_spores_by_cluster),
        )
        .route("/spore/objects", get(list_spores))
        .route("/spore/objects/{spore_id}", get(get_spore))
        .route("/spore/objects/{spore_id}/decode", get(decode_spore))
        .route(
            "/spore/objects/{spore_id}/charts/capacity-history",
            get(get_spore_capacity_chart),
        )
        .route("/spore/owner/{lock_hash}", get(get_spores_by_owner))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ChartRangeParams {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClusterHoldersParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClusterActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    action: Option<String>,
}

fn decode_cluster_holders_cursor(
    raw: &str,
) -> Result<(i64, Vec<u8>), (axum::http::StatusCode, axum::Json<ApiError>)> {
    let mut parts = raw.split(':');
    let count = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster holders cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid cluster holders cursor"))?;
    let lock_hash_hex = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster holders cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request("Invalid cluster holders cursor"));
    }
    let lock_hash = hex::decode(lock_hash_hex.strip_prefix("0x").unwrap_or(lock_hash_hex))
        .map_err(|_| ApiError::bad_request("Invalid cluster holders cursor"))?;
    if lock_hash.len() != 32 {
        return Err(ApiError::bad_request("Invalid cluster holders cursor"));
    }
    Ok((count, lock_hash))
}

fn decode_cluster_activity_cursor(
    raw: &str,
) -> Result<(i64, i32), (axum::http::StatusCode, axum::Json<ApiError>)> {
    let mut parts = raw.split(':');
    let block = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster activities cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid cluster activities cursor"))?;
    let tx_index = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster activities cursor"))?
        .parse::<i32>()
        .map_err(|_| ApiError::bad_request("Invalid cluster activities cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request("Invalid cluster activities cursor"));
    }
    Ok((block, tx_index))
}

fn normalize_cluster_activity_action_filter(
    raw: Option<&str>,
) -> Result<Option<String>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    let Some(raw_value) = raw else {
        return Ok(None);
    };
    let normalized = raw_value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    match normalized.as_str() {
        "mint" | "transfer" | "burn" => Ok(Some(normalized)),
        _ => Err(ApiError::bad_request(
            "Invalid cluster activity action filter. Expected one of: mint, transfer, burn",
        )),
    }
}

fn parse_fixed_len_hex(
    raw: &str,
    expected_len: usize,
    err_msg: &'static str,
) -> Result<Vec<u8>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request(err_msg))?;
    if bytes.len() != expected_len {
        return Err(ApiError::bad_request(err_msg));
    }
    Ok(bytes)
}

/// Parse a cluster_id URL parameter. Accepts "sole-spores" alias
/// or a 32-byte hex string.
fn parse_cluster_id_param(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    if raw.eq_ignore_ascii_case("sole-spores") {
        return Ok(SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
    }
    parse_fixed_len_hex(
        raw,
        32,
        "Invalid cluster ID (expected 32-byte hex or 'sole-spores')",
    )
}

fn is_sole_spores_sentinel(id: &[u8]) -> bool {
    id == SOLE_SPORES_SENTINEL_COLLECTION
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterStorageProfileResponse {
    pub tier: String,
    pub fully_onchain_count: i64,
    pub decentralized_external_count: i64,
    pub centralized_dependent_count: i64,
    pub unknown_count: i64,
    pub fully_onchain_ratio: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterResponse {
    pub cluster_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub owner_lock_hash: String,
    pub owner_address: Option<String>,
    pub spores_count: i32,
    pub holders_count: i64,
    pub activities_count: i64,
    pub created_at_block: i64,
    pub live_capacity: Option<String>,
    #[serde(rename = "liveCommonKnowledgeSize")]
    pub live_used_capacity: Option<String>,
    pub storage_profile: ClusterStorageProfileResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeMediaSourceResponse {
    pub uri: String,
    pub scheme: String,
    pub source_location: String,
    pub dependency_tier: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeMediaProfileResponse {
    pub tier: String,
    pub sources: Vec<SporeMediaSourceResponse>,
    pub has_renderable_image: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeResponse {
    pub spore_id: String,
    pub tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<i32>,
    pub cluster_id: Option<String>,
    pub content_type: String,
    pub content_size: i32,
    pub owner_lock_hash: String,
    pub owner_address: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub live_capacity: Option<String>,
    #[serde(rename = "liveCommonKnowledgeSize")]
    pub live_used_capacity: Option<String>,
    pub media_profile: Option<SporeMediaProfileResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterHolderResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DobTraitResponse {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeDobDecodeResponse {
    pub spore_id: String,
    pub content_type: String,
    pub dna_hex: Option<String>,
    pub traits: Vec<DobTraitResponse>,
    pub svg_markup: Option<String>,
    pub issues: Vec<String>,
}

/// Convert an ObjectEntry from the store into a SporeResponse.
fn spore_to_response(
    spore_id: &[u8],
    entry: &ckbadger_store::ObjectEntry,
    live_capacity: Option<i128>,
    live_used_capacity: Option<i128>,
) -> SporeResponse {
    let (content_type, content_size, media_profile) = match &entry.extra {
        ckbadger_store::ObjectExtra::Spore {
            content_type,
            content_length,
            media_profile,
        } => (
            content_type.clone(),
            *content_length as i32,
            Some(SporeMediaProfileResponse {
                tier: media_profile.tier.as_str().to_string(),
                sources: media_profile
                    .sources
                    .iter()
                    .map(|source| SporeMediaSourceResponse {
                        uri: source.uri.clone(),
                        scheme: source.scheme.clone(),
                        source_location: source.source_location.clone(),
                        dependency_tier: source.dependency_tier.as_str().to_string(),
                    })
                    .collect(),
                has_renderable_image: media_profile.has_renderable_image,
                issues: media_profile.issues.clone(),
            }),
        ),
        _ => (String::new(), 0, None),
    };
    SporeResponse {
        spore_id: format!("0x{}", hex::encode(spore_id)),
        tx_hash: format!("0x{}", hex::encode(&entry.created_at_tx)),
        output_index: None, // ObjectEntry does not store output_index; needs schema addition
        cluster_id: entry
            .collection_id
            .as_ref()
            .filter(|c| !is_sole_spores_sentinel(c))
            .map(|c| format!("0x{}", hex::encode(c))),
        content_type,
        content_size,
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
            .unwrap_or_default(),
        owner_address: None,
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
        live_capacity: live_capacity.map(|v| v.to_string()),
        live_used_capacity: live_used_capacity.map(|v| v.to_string()),
        media_profile,
    }
}

#[derive(Debug, Clone)]
struct Dob0PatternElement {
    trait_name: String,
    dna_offset: usize,
    dna_length: usize,
    pattern_type: String,
    trait_args: Option<Value>,
    dob_type: Option<String>,
}

#[derive(Debug, Clone)]
struct Dob1PatternElement {
    image_name: String,
    svg_fields: String,
    trait_name: String,
    pattern_type: String,
    trait_args: Option<Value>,
}

fn clean_hex(raw: &str) -> Option<String> {
    let mut normalized = raw.trim().to_ascii_lowercase();
    if let Some(stripped) = normalized.strip_prefix("0x") {
        normalized = stripped.to_string();
    }
    normalized.retain(|c| !c.is_whitespace());
    if normalized.is_empty() || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if normalized.len() % 2 == 1 {
        return Some(format!("0{normalized}"));
    }
    Some(normalized)
}

fn json_to_usize(value: &Value) -> Option<usize> {
    value.as_u64().and_then(|v| usize::try_from(v).ok())
}

fn format_json_value(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(s) => s.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn parse_le_modulo(bytes: &[u8], modulo: usize) -> usize {
    if modulo == 0 {
        return 0;
    }
    let m = modulo as u128;
    let mut acc: u128 = 0;
    let mut factor: u128 = 1 % m;
    for b in bytes {
        acc = (acc + (((*b as u128 % m) * factor) % m)) % m;
        factor = (factor * 256) % m;
    }
    acc as usize
}

fn parse_le_u128(bytes: &[u8]) -> Option<u128> {
    if bytes.len() > 16 {
        return None;
    }
    let mut value: u128 = 0;
    for (idx, b) in bytes.iter().enumerate() {
        value |= (*b as u128) << (idx * 8);
    }
    Some(value)
}

fn read_molecule_bytes_field(data: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
    if start >= end || start + 4 > data.len() {
        return None;
    }
    let len = u32::from_le_bytes(data[start..start + 4].try_into().ok()?) as usize;
    let value_start = start + 4;
    let value_end = value_start.checked_add(len)?;
    if value_end > data.len() || value_end > end {
        return None;
    }
    Some(data[value_start..value_end].to_vec())
}

fn parse_spore_content_from_output_data(data_hex: &str) -> Option<(String, Vec<u8>)> {
    let raw = clean_hex(data_hex)?;
    let data = hex::decode(raw).ok()?;
    if data.len() < 16 {
        return None;
    }

    let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if total_size < 16 || data.len() < total_size {
        return None;
    }

    let offset_content_type = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    let offset_content = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let offset_cluster_id = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;
    if !(16 <= offset_content_type
        && offset_content_type <= offset_content
        && offset_content <= offset_cluster_id
        && offset_cluster_id <= total_size)
    {
        return None;
    }

    let content_type_bytes = read_molecule_bytes_field(&data, offset_content_type, offset_content)?;
    let content_bytes = read_molecule_bytes_field(&data, offset_content, offset_cluster_id)?;
    let content_type = String::from_utf8_lossy(&content_type_bytes)
        .replace('\0', "")
        .trim()
        .to_string();
    Some((content_type, content_bytes))
}

fn parse_dna_hex_from_content_text(content_text: &str) -> Option<String> {
    let trimmed = content_text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let extract_from_json = |value: &Value| -> Option<String> {
        match value {
            Value::String(s) => clean_hex(s),
            Value::Array(items) => items.first().and_then(|v| v.as_str()).and_then(clean_hex),
            Value::Object(map) => map.get("dna").and_then(|v| v.as_str()).and_then(clean_hex),
            _ => None,
        }
    };

    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        return extract_from_json(&parsed);
    }

    clean_hex(trimmed)
}

fn normalize_dob0_pattern_element(value: &Value) -> Option<Dob0PatternElement> {
    if let Value::Array(items) = value {
        let trait_name = items.first()?.as_str()?.to_string();
        let dob_type = items.get(1).and_then(|v| v.as_str()).map(|s| s.to_string());
        let dna_offset = json_to_usize(items.get(2)?)?;
        let dna_length = json_to_usize(items.get(3)?)?;
        let pattern_type = items
            .get(4)
            .and_then(|v| v.as_str())
            .unwrap_or("raw")
            .to_string();
        let trait_args = items.get(5).cloned();
        return Some(Dob0PatternElement {
            trait_name,
            dna_offset,
            dna_length,
            pattern_type,
            trait_args,
            dob_type,
        });
    }

    let obj = value.as_object()?;
    let trait_name = obj.get("traitName")?.as_str()?.to_string();
    let dna_offset = json_to_usize(obj.get("dnaOffset")?)?;
    let dna_length = json_to_usize(obj.get("dnaLength")?)?;
    let pattern_type = obj
        .get("patternType")
        .and_then(|v| v.as_str())
        .unwrap_or("raw")
        .to_string();
    let dob_type = obj
        .get("dobType")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(Dob0PatternElement {
        trait_name,
        dna_offset,
        dna_length,
        pattern_type,
        trait_args: obj.get("traitArgs").cloned(),
        dob_type,
    })
}

fn normalize_dob1_pattern_element(value: &Value) -> Option<Dob1PatternElement> {
    if let Value::Array(items) = value {
        let image_name = items.first()?.as_str()?.to_string();
        let svg_fields = items.get(1)?.as_str()?.to_string();
        let trait_name = items.get(2)?.as_str()?.to_string();
        let pattern_type = items.get(3)?.as_str()?.to_string();
        let trait_args = items.get(4).cloned();
        return Some(Dob1PatternElement {
            image_name,
            svg_fields,
            trait_name,
            pattern_type,
            trait_args,
        });
    }

    let obj = value.as_object()?;
    Some(Dob1PatternElement {
        image_name: obj.get("imageName")?.as_str()?.to_string(),
        svg_fields: obj.get("svgFields")?.as_str()?.to_string(),
        trait_name: obj.get("traitName")?.as_str()?.to_string(),
        pattern_type: obj.get("patternType")?.as_str()?.to_string(),
        trait_args: obj.get("traitArgs").cloned(),
    })
}

fn extract_dob0_pattern(metadata: &Value) -> Vec<Dob0PatternElement> {
    let dob = if let Some(v) = metadata.get("dob").and_then(|v| v.as_object()) {
        v
    } else {
        return Vec::new();
    };

    let ver = dob.get("ver").and_then(|v| v.as_i64());
    if ver.unwrap_or(0) == 0 {
        if let Some(patterns) = dob.get("pattern").and_then(|v| v.as_array()) {
            return patterns
                .iter()
                .filter_map(normalize_dob0_pattern_element)
                .collect();
        }
    }

    if let Some(decoders) = dob.get("decoders").and_then(|v| v.as_array()) {
        for decoder in decoders {
            if let Some(patterns) = decoder.get("pattern").and_then(|v| v.as_array()) {
                let normalized: Vec<Dob0PatternElement> = patterns
                    .iter()
                    .filter_map(normalize_dob0_pattern_element)
                    .collect();
                if !normalized.is_empty() {
                    return normalized;
                }
            }
        }
    }
    Vec::new()
}

fn extract_dob1_pattern(metadata: &Value) -> Vec<Dob1PatternElement> {
    let dob = if let Some(v) = metadata.get("dob").and_then(|v| v.as_object()) {
        v
    } else {
        return Vec::new();
    };
    let decoders = if let Some(v) = dob.get("decoders").and_then(|v| v.as_array()) {
        v
    } else {
        return Vec::new();
    };

    for decoder in decoders {
        if let Some(patterns) = decoder.get("pattern").and_then(|v| v.as_array()) {
            let normalized: Vec<Dob1PatternElement> = patterns
                .iter()
                .filter_map(normalize_dob1_pattern_element)
                .collect();
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    Vec::new()
}

fn decode_dob0_trait_value(pattern: &Dob0PatternElement, dna_slice: &[u8]) -> Value {
    let kind = pattern.pattern_type.to_ascii_lowercase();

    match kind.as_str() {
        "options" => {
            if let Some(Value::Array(options)) = &pattern.trait_args {
                if options.is_empty() {
                    return Value::Null;
                }
                let idx = parse_le_modulo(dna_slice, options.len());
                return options[idx].clone();
            }
            Value::Null
        }
        "range" => {
            if let Some(Value::Array(args)) = &pattern.trait_args {
                if args.len() < 2 {
                    return Value::Null;
                }
                let min = args[0].as_i64();
                let max = args[1].as_i64();
                if let (Some(a), Some(b)) = (min, max) {
                    let lo = a.min(b);
                    let hi = a.max(b);
                    if let Some(width) = hi.checked_sub(lo).and_then(|d| d.checked_add(1)) {
                        if let Ok(width_usize) = usize::try_from(width) {
                            let offset = parse_le_modulo(dna_slice, width_usize) as i64;
                            return Value::from(lo + offset);
                        }
                    }
                }
            }
            Value::Null
        }
        "utf8" => Value::String(
            String::from_utf8_lossy(dna_slice)
                .trim_end_matches('\0')
                .to_string(),
        ),
        "rawnumber" => {
            if let Some(v) = parse_le_u128(dna_slice) {
                Value::String(v.to_string())
            } else {
                Value::String(format!("0x{}", hex::encode(dna_slice)))
            }
        }
        "rawstring" => Value::String(format!("0x{}", hex::encode(dna_slice))),
        "raw" => {
            if pattern
                .dob_type
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case("number"))
            {
                if let Some(v) = parse_le_u128(dna_slice) {
                    Value::String(v.to_string())
                } else {
                    Value::String(format!("0x{}", hex::encode(dna_slice)))
                }
            } else {
                Value::String(format!("0x{}", hex::encode(dna_slice)))
            }
        }
        _ => Value::String(format!("0x{}", hex::encode(dna_slice))),
    }
}

fn selector_matches(selector: &Value, trait_value: &str) -> bool {
    match selector {
        Value::String(s) if s == "*" => true,
        Value::Array(items) => items.iter().any(|item| selector_matches(item, trait_value)),
        _ => format_json_value(selector) == trait_value,
    }
}

fn resolve_dob1_snippet(
    pattern: &Dob1PatternElement,
    traits: &HashMap<String, String>,
) -> Option<String> {
    let kind = pattern.pattern_type.to_ascii_lowercase();
    if kind == "raw" {
        return pattern
            .trait_args
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if kind != "options" {
        return None;
    }

    let mut wildcard: Option<String> = None;
    let trait_value = traits.get(&pattern.trait_name).cloned().unwrap_or_default();
    let options = pattern.trait_args.as_ref()?.as_array()?;
    for option in options {
        let Some(pair) = option.as_array() else {
            continue;
        };
        if pair.len() < 2 {
            continue;
        }
        let selector = &pair[0];
        let snippet = if let Some(v) = pair[1].as_str() {
            v
        } else {
            continue;
        };
        if selector_matches(selector, "*") {
            wildcard = Some(snippet.to_string());
        }
        if selector_matches(selector, &trait_value) {
            return Some(snippet.to_string());
        }
    }
    wildcard
}

fn build_dob1_svg(
    patterns: &[Dob1PatternElement],
    traits: &HashMap<String, String>,
) -> Option<String> {
    if patterns.is_empty() {
        return None;
    }

    let mut images: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for pattern in patterns {
        let Some(snippet) = resolve_dob1_snippet(pattern, traits) else {
            continue;
        };
        let entry = images
            .entry(pattern.image_name.clone())
            .or_insert_with(|| (Vec::new(), Vec::new()));
        if pattern.svg_fields == "attributes" {
            entry.0.push(snippet);
        } else if pattern.svg_fields == "elements" {
            entry.1.push(snippet);
        }
    }

    let image_key = if images.contains_key("IMAGE.0") {
        "IMAGE.0".to_string()
    } else {
        images.keys().next()?.to_string()
    };
    let (attrs, elements) = images.get(&image_key)?;
    if elements.is_empty() {
        return None;
    }

    let attr_text = if attrs.is_empty() {
        "xmlns='http://www.w3.org/2000/svg' viewBox='0 0 500 500'".to_string()
    } else {
        attrs.join(" ")
    };
    let element_text = elements.join("");
    Some(format!("<svg {attr_text}>{element_text}</svg>"))
}

fn decode_dob_embedded(
    content_type: &str,
    content_text: Option<&str>,
    cluster_description: Option<&str>,
) -> (
    Option<String>,
    Vec<DobTraitResponse>,
    Option<String>,
    Vec<String>,
) {
    let mut issues = Vec::new();
    if !content_type.to_ascii_lowercase().starts_with("dob/") {
        issues.push(format!(
            "Unsupported content type for DOB decode: {content_type}"
        ));
        return (None, Vec::new(), None, issues);
    }

    let metadata = cluster_description.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    if metadata.is_none() {
        issues.push("Missing or invalid DOB metadata in cluster description".to_string());
    }

    let dna_hex = content_text.and_then(parse_dna_hex_from_content_text);
    if dna_hex.is_none() {
        issues.push("Missing or invalid DNA in DOB content".to_string());
    }

    let mut traits: Vec<DobTraitResponse> = Vec::new();
    if let (Some(meta), Some(dna_hex)) = (metadata.as_ref(), dna_hex.as_ref()) {
        let Ok(dna_bytes) = hex::decode(dna_hex) else {
            issues.push("DNA hex decode failed".to_string());
            return (Some(dna_hex.clone()), traits, None, issues);
        };
        let patterns = extract_dob0_pattern(meta);
        if patterns.is_empty() {
            issues.push("No DOB/0 pattern found in cluster metadata".to_string());
        } else {
            for pattern in patterns {
                let start = pattern.dna_offset.min(dna_bytes.len());
                let end = (start + pattern.dna_length).min(dna_bytes.len());
                let raw = decode_dob0_trait_value(&pattern, &dna_bytes[start..end]);
                traits.push(DobTraitResponse {
                    name: pattern.trait_name,
                    value: format_json_value(&raw),
                });
            }
        }
    }

    let mut svg_markup = None;
    if let Some(meta) = metadata.as_ref() {
        let dob1_patterns = extract_dob1_pattern(meta);
        if !dob1_patterns.is_empty() {
            let trait_map: HashMap<String, String> = traits
                .iter()
                .map(|item| (item.name.clone(), item.value.clone()))
                .collect();
            svg_markup = build_dob1_svg(&dob1_patterns, &trait_map);
            if svg_markup.is_none() {
                issues.push(
                    "DOB/1 SVG pattern detected but no renderable SVG output was produced"
                        .to_string(),
                );
            }
        }
    }

    (dna_hex, traits, svg_markup, issues)
}

fn is_text_like_content_type(content_type: &str) -> bool {
    let normalized = content_type.trim().to_ascii_lowercase();
    normalized.starts_with("text/")
        || normalized.contains("json")
        || normalized.contains("xml")
        || normalized.contains("javascript")
        || normalized.starts_with("dob/")
}

fn load_spore_content_from_ckb(
    state: &Arc<AppState>,
    spore_id: &[u8],
    entry: &ckbadger_store::ObjectEntry,
) -> anyhow::Result<(String, Vec<u8>)> {
    let ckb_store = state.ckb_store.as_ref().ok_or_else(|| {
        anyhow::anyhow!("CKB direct store unavailable; set [ckb].workdir in ckbadger.toml")
    })?;

    if entry.created_at_tx.len() != 32 {
        anyhow::bail!(
            "invalid spore created_at_tx length: expected 32, got {}",
            entry.created_at_tx.len()
        );
    }
    let tx_hash_arr: [u8; 32] = entry
        .created_at_tx
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid spore created_at_tx bytes"))?;

    let tx_view = ckb_store.get_transaction(&tx_hash_arr).ok_or_else(|| {
        anyhow::anyhow!(
            "spore creation transaction not found in CKB store: 0x{}",
            hex::encode(tx_hash_arr)
        )
    })?;
    let rpc_tx = ckb_store_reader::convert_transaction_view(&tx_view);

    let target_spore_id = format!("0x{}", hex::encode(spore_id));
    for (output, output_data) in rpc_tx.outputs.iter().zip(rpc_tx.outputs_data.iter()) {
        let Some(type_script) = output.type_.as_ref() else {
            continue;
        };
        if !type_script.args.eq_ignore_ascii_case(&target_spore_id) {
            continue;
        }
        if let Some(parsed) = parse_spore_content_from_output_data(output_data) {
            return Ok(parsed);
        }
    }

    anyhow::bail!(
        "spore output data not found in tx 0x{} for spore 0x{}",
        hex::encode(tx_hash_arr),
        hex::encode(spore_id)
    )
}

fn format_yyyymmdd_for_chart(date: u32) -> String {
    let s = format!("{date:08}");
    format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8])
}

fn build_capacity_history_chart(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
) -> anyhow::Result<StackedAreaChartResponse> {
    build_capacity_history_chart_with_initial(deltas, title, 0, 0, None, None)
}

fn build_capacity_history_chart_with_initial(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
    initial_capacity: i128,
    initial_used: i128,
    from_date: Option<u32>,
    to_date: Option<u32>,
) -> anyhow::Result<StackedAreaChartResponse> {
    if initial_capacity < 0 {
        anyhow::bail!(
            "invalid initial capacity for spore chart: {}",
            initial_capacity
        );
    }
    if initial_used < 0 {
        anyhow::bail!(
            "invalid initial common knowledge size for spore chart: {}",
            initial_used
        );
    }
    if initial_used > initial_capacity {
        anyhow::bail!(
            "invalid initial common knowledge size/capacity for spore chart: used={}, capacity={}",
            initial_used,
            initial_capacity
        );
    }
    let mut daily_deltas: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
    for (date, capacity_delta, used_delta) in deltas {
        let entry = daily_deltas.entry(date).or_insert((0, 0));
        entry.0 = entry.0.checked_add(capacity_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "capacity delta overflow while building spore capacity history chart: date={}",
                date
            )
        })?;
        entry.1 = entry.1.checked_add(used_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "used delta overflow while building spore capacity history chart: date={}",
                date
            )
        })?;
    }

    let chart_bounds = match (from_date, to_date) {
        (Some(from), Some(to)) => Some((from, to)),
        (Some(from), None) => daily_deltas
            .keys()
            .next_back()
            .copied()
            .map(|last| (from, last)),
        (None, Some(to)) => daily_deltas.keys().next().copied().map(|first| (first, to)),
        (None, None) => {
            let first = daily_deltas.keys().next().copied();
            let last = daily_deltas.keys().next_back().copied();
            first.zip(last)
        }
    };
    let dates = if let Some((start, end)) = chart_bounds {
        date_keys_inclusive(start, end).map_err(|e| anyhow::anyhow!(e))?
    } else {
        Vec::new()
    };

    let mut running_capacity = initial_capacity;
    let mut running_used = initial_used;
    let mut data = Vec::with_capacity(dates.len());

    for date in dates {
        let (capacity_delta, used_delta) = daily_deltas.get(&date).copied().unwrap_or((0, 0));
        (running_capacity, running_used) = apply_live_capacity_delta(
            running_capacity,
            running_used,
            capacity_delta,
            used_delta,
            &format!("building spore capacity history chart at date {}", date),
        )?;
        let unused = running_capacity - running_used;
        let mut values = std::collections::HashMap::new();
        values.insert("used".to_string(), running_used.to_string());
        values.insert("unused".to_string(), unused.to_string());

        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values,
        });
    }

    Ok(StackedAreaChartResponse {
        data,
        series: vec![
            StackedAreaSeries {
                key: "used".to_string(),
                label: "Used".to_string(),
                color: "#f59e0b".to_string(),
            },
            StackedAreaSeries {
                key: "unused".to_string(),
                label: "Unused".to_string(),
                color: "#06b6d4".to_string(),
            },
        ],
        title,
    })
}

fn latest_capacity_from_chart(
    chart: &StackedAreaChartResponse,
) -> (Option<String>, Option<String>) {
    if let Some(last) = chart.data.last() {
        let used = last.values.get("used").cloned();
        let unused = last.values.get("unused").cloned();
        let capacity = match (&used, &unused) {
            (Some(o), Some(u)) => {
                let total = o.parse::<i128>().unwrap_or(0) + u.parse::<i128>().unwrap_or(0);
                Some(total.to_string())
            }
            _ => None,
        };
        return (capacity, used);
    }
    (Some("0".to_string()), Some("0".to_string()))
}

fn format_ratio_4(numerator: i64, denominator: i64) -> String {
    if denominator <= 0 {
        return "0.0000".to_string();
    }
    let scaled = numerator
        .saturating_mul(10_000)
        .checked_div(denominator)
        .unwrap_or(0);
    let whole = scaled / 10_000;
    let frac = (scaled % 10_000).abs();
    format!("{whole}.{frac:04}")
}

fn resolve_storage_tier(
    fully_onchain: i64,
    decentralized_external: i64,
    centralized_dependent: i64,
    unknown: i64,
) -> String {
    if centralized_dependent > 0 {
        return "centralized_dependent".to_string();
    }
    if decentralized_external > 0 {
        return "decentralized_external".to_string();
    }
    if fully_onchain > 0 && unknown == 0 {
        return "fully_onchain".to_string();
    }
    "unknown".to_string()
}

fn cluster_storage_profile_from_aggregate(
    aggregate: Option<&ckbadger_store::types::ClusterAggregate>,
    spores_count: i64,
) -> ClusterStorageProfileResponse {
    let fully_onchain_count = aggregate.map(|a| a.fully_onchain_count).unwrap_or(0);
    let decentralized_external_count = aggregate
        .map(|a| a.decentralized_external_count)
        .unwrap_or(0);
    let centralized_dependent_count = aggregate
        .map(|a| a.centralized_dependent_count)
        .unwrap_or(0);
    let unknown_count = aggregate
        .map(|a| a.unknown_count)
        .unwrap_or(spores_count.max(0));
    ClusterStorageProfileResponse {
        tier: resolve_storage_tier(
            fully_onchain_count,
            decentralized_external_count,
            centralized_dependent_count,
            unknown_count,
        ),
        fully_onchain_count,
        decentralized_external_count,
        centralized_dependent_count,
        unknown_count,
        fully_onchain_ratio: format_ratio_4(fully_onchain_count, spores_count),
    }
}

/// List clusters — use cached NFT assets (filtered to Spore) when available.
async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    // Try cached NFT assets first (Spore entries carry cluster grouping)
    if let Some(cached_nfts) = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_NFT)
    {
        return serve_clusters_from_cache(cached_nfts, cursor_block, limit, &state);
    }

    Err(state.asset_cache_unavailable("cluster cache unavailable; warmup in progress"))
}

fn serve_clusters_from_cache(
    cached: Vec<CachedAssetEntry>,
    cursor_block: i64,
    limit: usize,
    state: &Arc<AppState>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let cluster_ids = unique_cluster_ids_from_cached_entries(&cached);

    let spore_entries = state
        .store
        .get_spores_batch(&cluster_ids)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut spore_map: HashMap<Vec<u8>, ckbadger_store::ObjectEntry> =
        HashMap::with_capacity(spore_entries.len());
    for (cluster_id, entry_opt) in spore_entries {
        if let Some(entry) = entry_opt {
            spore_map.insert(cluster_id, entry);
        }
    }

    let cluster_aggregates = state
        .store
        .get_cluster_aggregates_batch(&cluster_ids)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut cluster_agg_map: HashMap<Vec<u8>, ckbadger_store::ClusterAggregate> =
        HashMap::with_capacity(cluster_aggregates.len());
    for (cluster_id, aggregate_opt) in cluster_aggregates {
        if let Some(aggregate) = aggregate_opt {
            cluster_agg_map.insert(cluster_id, aggregate);
        }
    }

    let clusters =
        build_cluster_responses_from_cached_entries(cached, &spore_map, &cluster_agg_map);

    let filtered: Vec<_> = clusters
        .iter()
        .filter(|c| c.created_at_block < cursor_block)
        .take(limit + 1)
        .cloned()
        .collect();

    let has_more = filtered.len() > limit;
    let page: Vec<_> = filtered.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|c| c.created_at_block.to_string())
    } else {
        None
    };

    ok(CursorPaginatedResponse::without_total(
        page,
        limit as i64,
        next_cursor,
    ))
}

fn cluster_id_bytes_from_cached_entry(entry: &CachedAssetEntry) -> Option<Vec<u8>> {
    if entry.standard != "spore" {
        return None;
    }
    let cluster_id_hex = entry.cluster_id.as_ref().unwrap_or(&entry.id);
    hex::decode(cluster_id_hex.strip_prefix("0x").unwrap_or(cluster_id_hex)).ok()
}

fn unique_cluster_ids_from_cached_entries(cached: &[CachedAssetEntry]) -> Vec<Vec<u8>> {
    let mut unique_ids = Vec::new();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for cluster_id in cached.iter().filter_map(cluster_id_bytes_from_cached_entry) {
        if seen.insert(cluster_id.clone()) {
            unique_ids.push(cluster_id);
        }
    }
    unique_ids
}

fn build_cluster_responses_from_cached_entries(
    cached: Vec<CachedAssetEntry>,
    spore_map: &HashMap<Vec<u8>, ckbadger_store::ObjectEntry>,
    cluster_agg_map: &HashMap<Vec<u8>, ckbadger_store::ClusterAggregate>,
) -> Vec<ClusterResponse> {
    let mut clusters = Vec::new();

    for entry in cached {
        let Some(cluster_id) = cluster_id_bytes_from_cached_entry(&entry) else {
            continue;
        };

        let cluster_entry = spore_map.get(&cluster_id);
        let cluster_aggregate = cluster_agg_map.get(&cluster_id);
        let created_at_block = cluster_entry.map(|e| e.created_at_block).unwrap_or(0);
        let description = cluster_entry.and_then(|e| e.description.clone());
        let owner_lock_hash = cluster_entry.and_then(|e| e.owner_lock_hash.clone());

        clusters.push(ClusterResponse {
            cluster_id: entry.id,
            name: entry.name,
            description,
            owner_lock_hash: owner_lock_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h)))
                .unwrap_or_default(),
            owner_address: None,
            spores_count: entry.transfers_count as i32, // transfers_count holds spore count for DOB
            holders_count: cluster_aggregate.map(|a| a.owner_count).unwrap_or(0),
            activities_count: 0,
            created_at_block,
            live_capacity: None,
            live_used_capacity: None,
            storage_profile: cluster_storage_profile_from_aggregate(
                cluster_aggregate,
                entry.transfers_count,
            ),
        });
    }

    clusters.sort_by(|a, b| b.created_at_block.cmp(&a.created_at_block));
    clusters
}

fn load_spores_cached_or_store(state: &Arc<AppState>) -> Result<CachedSporeRows, ApiRouteError> {
    if let Some(cached) = state.mem_cache.get::<CachedSporeRows>(CACHE_KEY_SPORES_ALL) {
        return Ok(cached);
    }
    Err(ApiError::warmup_pending(
        "spore cache unavailable; warmup in progress",
    ))
}

fn collect_spore_page<F>(
    all_spores: &[(Vec<u8>, ckbadger_store::ObjectEntry)],
    limit: usize,
    mut predicate: F,
) -> Vec<(&Vec<u8>, &ckbadger_store::ObjectEntry)>
where
    F: FnMut(&ckbadger_store::ObjectEntry) -> bool,
{
    let mut page: Vec<(&Vec<u8>, &ckbadger_store::ObjectEntry)> = Vec::with_capacity(limit + 1);
    for (spore_id, entry) in all_spores {
        if !predicate(entry) {
            continue;
        }
        page.push((spore_id, entry));
        if page.len() > limit {
            break;
        }
    }
    page
}

async fn get_cluster_holders(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ClusterHoldersParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterHolderResponse>> {
    let id = parse_cluster_id_param(&cluster_id)?;
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_cluster_holders_cursor)
        .transpose()?;

    let store = state.store.clone();
    let id_c = id.clone();
    let owners = tokio::task::spawn_blocking(move || store.list_cluster_owner_counts(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if owners.is_empty() && !is_sole_spores_sentinel(&id) {
        let store = state.store.clone();
        let id_c = id.clone();
        let cluster_exists = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some();
        if !cluster_exists {
            return Err(ApiError::not_found("Cluster not found"));
        }
    }

    let mut rows = owners;
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total = rows.len() as i64;
    let start_idx = if let Some((cursor_count, cursor_lock_hash)) = cursor {
        rows.iter()
            .position(|(lock_hash, count)| *count == cursor_count && *lock_hash == cursor_lock_hash)
            .map(|idx| idx + 1)
            .ok_or_else(|| ApiError::bad_request("Invalid cluster holders cursor"))?
    } else {
        0
    };

    let page: Vec<_> = rows.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();
    let next_cursor = if has_more {
        page.last()
            .map(|(lock_hash, count)| format!("{}:{}", count, hex::encode(lock_hash)))
    } else {
        None
    };

    let response_rows: Vec<ClusterHolderResponse> = page
        .into_iter()
        .map(|(lock_hash, count)| ClusterHolderResponse {
            lock_script_hash: format!("0x{}", hex::encode(lock_hash)),
            address: None,
            item_count: *count,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        response_rows,
        total,
        limit as i64,
        next_cursor,
    ))
}

async fn get_cluster_activities(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ClusterActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterActivityResponse>> {
    let id = parse_cluster_id_param(&cluster_id)?;
    let limit = params.limit.clamp(1, 100);
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_cluster_activity_cursor)
        .transpose()?;
    let action_filter = normalize_cluster_activity_action_filter(params.action.as_deref())?;

    // Validate cluster exists (sentinel always passes)
    if !is_sole_spores_sentinel(&id) {
        let store = state.store.clone();
        let id_c = id.clone();
        let cluster_exists = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some();
        if !cluster_exists {
            return Err(ApiError::not_found("Cluster not found"));
        }
    }

    // Use pre-computed collection activity index and drop orphaned history rows.
    let results = list_canonical_nft_collection_activities_page(
        state.store.as_ref(),
        state.store.as_ref(),
        &id,
        (limit as usize) + 1,
        cursor,
        action_filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() as i64 > limit;
    let page: Vec<ClusterActivityResponse> = results
        .into_iter()
        .take(limit as usize)
        .map(|(block_number, tx_index, entry)| {
            let actions: Vec<String> = entry
                .actions
                .iter()
                .map(|a| match a {
                    ckbadger_store::AssetAction::Mint => "mint".to_string(),
                    ckbadger_store::AssetAction::Transfer => "transfer".to_string(),
                    ckbadger_store::AssetAction::Burn => "burn".to_string(),
                    ckbadger_store::AssetAction::Recycle => "recycle".to_string(),
                    ckbadger_store::AssetAction::Renew => "renew".to_string(),
                    ckbadger_store::AssetAction::Update => "update".to_string(),
                })
                .collect();
            ClusterActivityResponse {
                tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
                block_number,
                tx_index,
                timestamp: entry.timestamp_ms.to_string(),
                actions,
            }
        })
        .collect();

    let next_cursor = if has_more {
        page.last()
            .map(|row| format!("{}:{}", row.block_number, row.tx_index))
    } else {
        None
    };

    ok(CursorPaginatedResponse::without_total(
        page,
        limit,
        next_cursor,
    ))
}

/// Get spores by cluster — use secondary index instead of full scan.
async fn get_spores_by_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let id = parse_cluster_id_param(&cluster_id)?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    // Use secondary index for efficient lookup
    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_spores =
        tokio::task::spawn_blocking(move || store.list_spores_by_cluster(&id_c, 10_000))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut filtered: Vec<_> = cluster_spores
        .into_iter()
        .filter(|(_, entry)| entry.is_live && entry.created_at_block < cursor_block)
        .collect();

    filtered.sort_by(|a, b| b.1.created_at_block.cmp(&a.1.created_at_block));

    let page: Vec<_> = filtered.iter().take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .into_iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}

/// Get cluster — point lookup + count from secondary index (no full scan).
async fn get_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
) -> ApiResult<ClusterResponse> {
    let id = parse_cluster_id_param(&cluster_id)?;

    // Look up the cluster entry directly
    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_aggregate = tokio::task::spawn_blocking(move || store.get_cluster_aggregate(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Count spores in cluster using secondary index
    let store = state.store.clone();
    let id_c = id.clone();
    let spores_count = tokio::task::spawn_blocking(move || store.count_spores_in_cluster(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if spores_count == 0 && cluster_entry.is_none() && !is_sole_spores_sentinel(&id) {
        return Err(ApiError::not_found("Cluster not found"));
    }

    let holders_count = cluster_aggregate
        .as_ref()
        .map(|agg| agg.owner_count)
        .unwrap_or(0);
    let activities_count = count_nft_collection_activities_cached(
        state.store.as_ref(),
        state.store.as_ref(),
        &state.mem_cache,
        &id,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (name, description, owner_lock_hash, created_at_block) = if is_sole_spores_sentinel(&id) {
        (
            Some("Sole Spores".to_string()),
            Some("Spores not belonging to any cluster".to_string()),
            None,
            0i64,
        )
    } else {
        let name = cluster_entry.as_ref().and_then(|e| e.name.clone());
        let description = cluster_entry.as_ref().and_then(|e| e.description.clone());
        let owner_lock_hash = cluster_entry
            .as_ref()
            .and_then(|e| e.owner_lock_hash.clone());
        let created_at_block = cluster_entry
            .as_ref()
            .map(|e| e.created_at_block)
            .unwrap_or(0);
        (name, description, owner_lock_hash, created_at_block)
    };
    let store = state.store.clone();
    let id_c = id.clone();
    let daily = tokio::task::spawn_blocking(move || store.list_cluster_daily_deltas(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let chart = build_capacity_history_chart(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.live_capacity_delta,
                    delta.live_used_capacity_delta,
                )
            })
            .collect(),
        format!(
            "{} Capacity History",
            name.clone().unwrap_or_else(|| "Spore Cluster".to_string())
        ),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let (live_capacity, live_used_capacity) = latest_capacity_from_chart(&chart);

    ok(ClusterResponse {
        cluster_id: format!("0x{}", hex::encode(&id)),
        name,
        description,
        owner_lock_hash: owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
            .unwrap_or_default(),
        owner_address: None,
        spores_count: spores_count as i32,
        holders_count,
        activities_count,
        created_at_block,
        live_capacity,
        live_used_capacity,
        storage_profile: cluster_storage_profile_from_aggregate(
            cluster_aggregate.as_ref(),
            spores_count,
        ),
    })
}

async fn list_spores(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let all_spores = load_spores_cached_or_store(&state)?;
    let page = collect_spore_page(&all_spores, limit, |entry| {
        entry.is_live && entry.created_at_block < cursor_block
    });
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .into_iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}

async fn get_spore(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> ApiResult<SporeResponse> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match entry {
        Some(entry) => {
            let daily = state
                .store
                .list_spore_daily_deltas(&id)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let chart = build_capacity_history_chart(
                daily
                    .into_iter()
                    .map(|(date, delta)| {
                        (
                            date,
                            delta.live_capacity_delta,
                            delta.live_used_capacity_delta,
                        )
                    })
                    .collect(),
                "Spore Capacity History".to_string(),
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
            let (live_capacity, live_used_capacity) = latest_capacity_from_chart(&chart);
            let cap = live_capacity.and_then(|v| v.parse::<i128>().ok());
            let occ = live_used_capacity.and_then(|v| v.parse::<i128>().ok());
            ok(spore_to_response(&id, &entry, cap, occ))
        }
        None => Err(ApiError::not_found("Spore not found")),
    }
}

async fn decode_spore(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> ApiResult<SporeDobDecodeResponse> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;

    let mut content_type = match &entry.extra {
        ckbadger_store::ObjectExtra::Spore { content_type, .. } => content_type.clone(),
        _ => String::new(),
    };

    let cluster_description = if let Some(cluster_id) = entry.collection_id.as_ref() {
        state
            .store
            .get_spore(cluster_id)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .and_then(|cluster| cluster.description)
    } else {
        None
    };

    let mut issues = Vec::new();
    let mut content_text: Option<String> = None;
    match load_spore_content_from_ckb(&state, &id, &entry) {
        Ok((loaded_content_type, content_bytes)) => {
            content_type = loaded_content_type;
            if is_text_like_content_type(&content_type) {
                if content_bytes.len() > 256 * 1024 {
                    issues.push(format!(
                        "DOB content is too large for text decode: {} bytes",
                        content_bytes.len()
                    ));
                } else {
                    content_text = Some(String::from_utf8_lossy(&content_bytes).to_string());
                }
            }
        }
        Err(e) => {
            issues.push(format!("Failed to load on-chain spore content: {e}"));
        }
    }

    let (dna_hex, traits, svg_markup, mut decode_issues) = decode_dob_embedded(
        &content_type,
        content_text.as_deref(),
        cluster_description.as_deref(),
    );
    issues.append(&mut decode_issues);

    ok(SporeDobDecodeResponse {
        spore_id: format!("0x{}", hex::encode(&id)),
        content_type,
        dna_hex,
        traits,
        svg_markup,
        issues,
    })
}

async fn get_cluster_capacity_chart(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let id = parse_cluster_id_param(&cluster_id)?;

    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let store = state.store.clone();
    let id_c = id.clone();
    let spores_count = tokio::task::spawn_blocking(move || store.count_spores_in_cluster(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if spores_count == 0 && cluster_entry.is_none() && !is_sole_spores_sentinel(&id) {
        return Err(ApiError::not_found("Cluster not found"));
    }

    let name = if is_sole_spores_sentinel(&id) {
        "Sole Spores".to_string()
    } else {
        cluster_entry
            .as_ref()
            .and_then(|e| e.name.clone())
            .unwrap_or_else(|| "Spore Cluster".to_string())
    };
    let store = state.store.clone();
    let id_c = id.clone();
    let daily = tokio::task::spawn_blocking(move || {
        store.list_cluster_daily_deltas_in_range(&id_c, from_date, to_date)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_used) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_used: i128 = 0;
        let baseline = state
            .store
            .list_cluster_daily_deltas_in_range(&id, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_used) = apply_live_capacity_delta(
                base_capacity,
                base_used,
                delta.live_capacity_delta,
                delta.live_used_capacity_delta,
                "building cluster baseline capacity history chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_used)
    } else {
        (0, 0)
    };

    ok(build_capacity_history_chart_with_initial(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.live_capacity_delta,
                    delta.live_used_capacity_delta,
                )
            })
            .collect(),
        format!("{name} Capacity History"),
        initial_capacity,
        initial_used,
        from_date,
        to_date,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
}

async fn get_spore_capacity_chart(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if entry.is_none() {
        return Err(ApiError::not_found("Spore not found"));
    }

    let store = state.store.clone();
    let id_c = id.clone();
    let daily = tokio::task::spawn_blocking(move || {
        store.list_spore_daily_deltas_in_range(&id_c, from_date, to_date)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_used) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_used: i128 = 0;
        let baseline = state
            .store
            .list_spore_daily_deltas_in_range(&id, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_used) = apply_live_capacity_delta(
                base_capacity,
                base_used,
                delta.live_capacity_delta,
                delta.live_used_capacity_delta,
                "building spore baseline capacity history chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_used)
    } else {
        (0, 0)
    };

    ok(build_capacity_history_chart_with_initial(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.live_capacity_delta,
                    delta.live_used_capacity_delta,
                )
            })
            .collect(),
        "Spore Capacity History".to_string(),
        initial_capacity,
        initial_used,
        from_date,
        to_date,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
}

async fn get_spores_by_owner(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let all_spores = load_spores_cached_or_store(&state)?;
    let page = collect_spore_page(&all_spores, limit, |entry| {
        entry.is_live
            && entry.owner_lock_hash.as_ref() == Some(&hash)
            && entry.created_at_block < cursor_block
    });
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .into_iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_molecule_bytes(raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + raw.len());
        out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        out.extend_from_slice(raw);
        out
    }

    fn make_spore_entry(
        created_at_block: i64,
        owner_lock_hash: Option<Vec<u8>>,
    ) -> ckbadger_store::ObjectEntry {
        ckbadger_store::ObjectEntry {
            standard: ckbadger_store::ObjectStandard::Spore,
            collection_id: None,
            token_id: None,
            owner_lock_hash,
            name: Some("sample".to_string()),
            description: None,
            is_live: true,
            created_at_block,
            created_at_tx: vec![0x11; 32],
            extra: ckbadger_store::ObjectExtra::Spore {
                content_type: "text/plain".to_string(),
                content_length: 5,
                media_profile: ckbadger_store::SporeMediaProfile::default(),
            },
        }
    }

    fn make_spore_output_data_hex(content_type: &str, content_text: &str) -> String {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content_text.as_bytes());

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster_id = offset_content + content_bytes.len() as u32;
        let total_size = offset_cluster_id;

        let mut out = Vec::new();
        out.extend_from_slice(&total_size.to_le_bytes());
        out.extend_from_slice(&offset_content_type.to_le_bytes());
        out.extend_from_slice(&offset_content.to_le_bytes());
        out.extend_from_slice(&offset_cluster_id.to_le_bytes());
        out.extend_from_slice(&content_type_bytes);
        out.extend_from_slice(&content_bytes);
        format!("0x{}", hex::encode(out))
    }

    #[test]
    fn test_parse_spore_content_from_output_data() {
        let data_hex = make_spore_output_data_hex("dob/0", r#"{ "dna": "0a01ff00" }"#);
        let parsed = parse_spore_content_from_output_data(&data_hex).expect("parse spore output");
        assert_eq!(parsed.0, "dob/0");
        assert!(String::from_utf8_lossy(&parsed.1).contains("\"dna\""));
    }

    #[test]
    fn test_collect_spore_page_respects_limit_plus_one() {
        let spores = vec![
            (vec![0x01; 32], make_spore_entry(300, Some(vec![0xAA; 32]))),
            (vec![0x02; 32], make_spore_entry(200, Some(vec![0xAA; 32]))),
            (vec![0x03; 32], make_spore_entry(100, Some(vec![0xAA; 32]))),
        ];
        let page = collect_spore_page(&spores, 2, |_| true);
        assert_eq!(page.len(), 3);
        assert_eq!(page[0].1.created_at_block, 300);
        assert_eq!(page[2].1.created_at_block, 100);
    }

    #[test]
    fn test_collect_spore_page_filters_by_predicate() {
        let spores = vec![
            (vec![0x01; 32], make_spore_entry(300, Some(vec![0xAA; 32]))),
            (vec![0x02; 32], make_spore_entry(200, Some(vec![0xBB; 32]))),
            (vec![0x03; 32], make_spore_entry(100, Some(vec![0xAA; 32]))),
        ];
        let target_owner = vec![0xAA; 32];
        let page = collect_spore_page(&spores, 10, |entry| {
            entry.owner_lock_hash.as_ref() == Some(&target_owner)
        });
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].1.created_at_block, 300);
        assert_eq!(page[1].1.created_at_block, 100);
    }

    #[test]
    fn test_build_cluster_responses_from_cached_entries_uses_maps_and_sorts() {
        let first_cluster = vec![0x11; 32];
        let second_cluster = vec![0x22; 32];

        let cached = vec![
            CachedAssetEntry {
                id: format!("0x{}", hex::encode(&first_cluster)),
                asset_type: "object".to_string(),
                standard: "spore".to_string(),
                name: Some("First".to_string()),
                symbol: None,
                icon_url: None,
                holders_count: 0,
                transfers_count: 7,
                transfers_24h: 0,
                decimals: None,
                total_supply: None,
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                live_capacity: None,
                live_used_capacity: None,
                storage_tier: None,
                fully_onchain_ratio: None,
                fully_onchain_count: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                description: None,
            },
            CachedAssetEntry {
                id: format!("0x{}", hex::encode(&second_cluster)),
                asset_type: "object".to_string(),
                standard: "spore".to_string(),
                name: Some("Second".to_string()),
                symbol: None,
                icon_url: None,
                holders_count: 0,
                transfers_count: 3,
                transfers_24h: 0,
                decimals: None,
                total_supply: None,
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                live_capacity: None,
                live_used_capacity: None,
                storage_tier: None,
                fully_onchain_ratio: None,
                fully_onchain_count: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                description: None,
            },
        ];

        let mut spore_map = HashMap::new();
        spore_map.insert(
            first_cluster.clone(),
            ckbadger_store::ObjectEntry {
                standard: ckbadger_store::ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0xAA; 32]),
                name: Some("Cluster A".to_string()),
                description: Some("A".to_string()),
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0xAA; 32],
                extra: ckbadger_store::ObjectExtra::SporeCluster,
            },
        );
        spore_map.insert(
            second_cluster.clone(),
            ckbadger_store::ObjectEntry {
                standard: ckbadger_store::ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0xBB; 32]),
                name: Some("Cluster B".to_string()),
                description: Some("B".to_string()),
                is_live: true,
                created_at_block: 200,
                created_at_tx: vec![0xBB; 32],
                extra: ckbadger_store::ObjectExtra::SporeCluster,
            },
        );

        let mut cluster_agg_map = HashMap::new();
        cluster_agg_map.insert(
            first_cluster.clone(),
            ckbadger_store::ClusterAggregate {
                owner_count: 12,
                ..Default::default()
            },
        );
        cluster_agg_map.insert(
            second_cluster.clone(),
            ckbadger_store::ClusterAggregate {
                owner_count: 34,
                ..Default::default()
            },
        );

        let clusters =
            build_cluster_responses_from_cached_entries(cached, &spore_map, &cluster_agg_map);
        assert_eq!(clusters.len(), 2);
        assert_eq!(
            clusters[0].cluster_id,
            format!("0x{}", hex::encode(&second_cluster))
        );
        assert_eq!(clusters[0].created_at_block, 200);
        assert_eq!(clusters[0].holders_count, 34);
        assert_eq!(
            clusters[1].cluster_id,
            format!("0x{}", hex::encode(&first_cluster))
        );
        assert_eq!(clusters[1].created_at_block, 100);
        assert_eq!(clusters[1].holders_count, 12);
    }

    #[test]
    fn test_decode_dob_embedded_dob0_traits() {
        let cluster_description = serde_json::json!({
            "dob": {
                "ver": 0,
                "pattern": [
                    {
                        "traitName": "Background",
                        "dobType": "String",
                        "dnaOffset": 0,
                        "dnaLength": 1,
                        "patternType": "options",
                        "traitArgs": ["red", "blue"]
                    },
                    {
                        "traitName": "Level",
                        "dobType": "Number",
                        "dnaOffset": 1,
                        "dnaLength": 1,
                        "patternType": "range",
                        "traitArgs": [10, 20]
                    }
                ]
            }
        })
        .to_string();

        let (dna_hex, traits, svg_markup, issues) = decode_dob_embedded(
            "dob/0",
            Some(r#"{ "dna": "0a01ff00" }"#),
            Some(&cluster_description),
        );

        assert_eq!(dna_hex.as_deref(), Some("0a01ff00"));
        assert_eq!(traits.len(), 2);
        assert_eq!(traits[0].name, "Background");
        assert_eq!(traits[0].value, "red");
        assert_eq!(traits[1].name, "Level");
        assert_eq!(traits[1].value, "11");
        assert!(svg_markup.is_none());
        assert!(issues.is_empty());
    }

    #[test]
    fn test_decode_dob_embedded_dob1_svg() {
        let cluster_description = serde_json::json!({
            "dob": {
                "ver": 1,
                "decoders": [
                    {
                        "pattern": [
                            {
                                "traitName": "BackgroundColor",
                                "dobType": "String",
                                "dnaOffset": 0,
                                "dnaLength": 1,
                                "patternType": "options",
                                "traitArgs": ["red", "blue"]
                            }
                        ]
                    },
                    {
                        "pattern": [
                            {
                                "imageName": "IMAGE.0",
                                "svgFields": "attributes",
                                "traitName": "",
                                "patternType": "raw",
                                "traitArgs": "xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'"
                            },
                            {
                                "imageName": "IMAGE.0",
                                "svgFields": "elements",
                                "traitName": "BackgroundColor",
                                "patternType": "options",
                                "traitArgs": [
                                    ["red", "<rect width='100' height='100' fill='red' />"],
                                    ["blue", "<rect width='100' height='100' fill='blue' />"]
                                ]
                            }
                        ]
                    }
                ]
            }
        })
        .to_string();

        let (_, traits, svg_markup, issues) = decode_dob_embedded(
            "dob/0",
            Some(r#"{ "dna": "01" }"#),
            Some(&cluster_description),
        );

        assert_eq!(traits.len(), 1);
        assert_eq!(traits[0].name, "BackgroundColor");
        assert_eq!(traits[0].value, "blue");
        let svg = svg_markup.expect("svg rendered");
        assert!(svg.contains("<svg "));
        assert!(svg.contains("fill='blue'"));
        assert!(issues.is_empty());
    }

    #[test]
    fn test_decode_dob_embedded_dob1_svg_with_array_pattern() {
        let cluster_description = serde_json::json!({
            "dob": {
                "ver": 1,
                "decoders": [
                    {
                        "pattern": [
                            ["BackgroundColor", "String", 0, 1, "options", ["red", "blue"]]
                        ]
                    },
                    {
                        "pattern": [
                            ["IMAGE.0", "attributes", "", "raw", "xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'"],
                            ["IMAGE.0", "elements", "BackgroundColor", "options", [
                                ["red", "<rect width='100' height='100' fill='red' />"],
                                ["blue", "<rect width='100' height='100' fill='blue' />"]
                            ]]
                        ]
                    }
                ]
            }
        })
        .to_string();

        let (_, traits, svg_markup, issues) = decode_dob_embedded(
            "dob/0",
            Some(r#"{ "dna": "01" }"#),
            Some(&cluster_description),
        );

        assert_eq!(traits.len(), 1);
        assert_eq!(traits[0].name, "BackgroundColor");
        assert_eq!(traits[0].value, "blue");
        let svg = svg_markup.expect("svg rendered");
        assert!(svg.contains("<svg "));
        assert!(svg.contains("fill='blue'"));
        assert!(issues.is_empty());
    }

    #[test]
    fn test_parse_fixed_len_hex_rejects_non_32_bytes() {
        let err = parse_fixed_len_hex("0x1234", 32, "Invalid cluster ID").unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_parse_cluster_id_param_sole_spores_alias() {
        let result = parse_cluster_id_param("sole-spores").unwrap();
        assert_eq!(result, SOLE_SPORES_SENTINEL_COLLECTION.to_vec());

        let result = parse_cluster_id_param("Sole-Spores").unwrap();
        assert_eq!(result, SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_parse_cluster_id_param_hex() {
        let hex_id = "ab".repeat(32);
        let result = parse_cluster_id_param(&hex_id).unwrap();
        assert_eq!(result, vec![0xab; 32]);

        let hex_id_0x = format!("0x{}", "cd".repeat(32));
        let result = parse_cluster_id_param(&hex_id_0x).unwrap();
        assert_eq!(result, vec![0xcd; 32]);
    }

    #[test]
    fn test_is_sole_spores_sentinel() {
        assert!(is_sole_spores_sentinel(&SOLE_SPORES_SENTINEL_COLLECTION));
        assert!(!is_sole_spores_sentinel(&[0xab; 32]));
    }
}
