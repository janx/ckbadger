use ckbadger_store::types::{SporeMediaProfile, SporeMediaSource, StorageDependencyTier};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_MEDIA_SOURCES: usize = 24;

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

pub fn analyze_spore_media_profile(
    content_type: &str,
    content: &[u8],
    cluster_description: Option<&str>,
) -> SporeMediaProfile {
    let normalized_type = content_type.trim().to_ascii_lowercase();
    let mut sources = Vec::new();
    let mut issues = Vec::new();

    let binary_media = normalized_type.starts_with("image/")
        || normalized_type.starts_with("video/")
        || normalized_type.starts_with("audio/");
    let mut has_renderable_image =
        normalized_type.starts_with("image/") || normalized_type.contains("svg");

    if is_text_like_content_type(&normalized_type) {
        match decode_text_payload(content) {
            Ok(text) => {
                if normalized_type.starts_with("dob/") {
                    let (mut dob_sources, dob_rendered) =
                        extract_dob_media_sources(&text, cluster_description, &mut issues);
                    sources.append(&mut dob_sources);
                    if dob_rendered {
                        has_renderable_image = true;
                    }
                } else {
                    extract_uri_sources(&text, "payload_text", &mut sources);
                    if text.to_ascii_lowercase().contains("<svg") {
                        has_renderable_image = true;
                    }
                }
            }
            Err(err) => {
                issues.push(err);
            }
        }
    }

    dedupe_and_limit_sources(&mut sources, MAX_MEDIA_SOURCES);
    if !has_renderable_image {
        has_renderable_image = sources.iter().any(|source| uri_seems_image(&source.uri));
    }

    let tier = resolve_tier(binary_media, has_renderable_image, &sources);
    SporeMediaProfile {
        tier,
        sources,
        has_renderable_image,
        issues,
    }
}

fn resolve_tier(
    binary_media: bool,
    has_renderable_image: bool,
    sources: &[SporeMediaSource],
) -> StorageDependencyTier {
    if sources
        .iter()
        .any(|source| source.dependency_tier == StorageDependencyTier::CentralizedDependent)
    {
        return StorageDependencyTier::CentralizedDependent;
    }
    if sources
        .iter()
        .any(|source| source.dependency_tier == StorageDependencyTier::DecentralizedExternal)
    {
        return StorageDependencyTier::DecentralizedExternal;
    }
    if sources
        .iter()
        .any(|source| source.dependency_tier == StorageDependencyTier::FullyOnchain)
    {
        return StorageDependencyTier::FullyOnchain;
    }
    if binary_media || has_renderable_image {
        return StorageDependencyTier::FullyOnchain;
    }
    StorageDependencyTier::Unknown
}

fn decode_text_payload(content: &[u8]) -> Result<String, String> {
    if content.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "text payload exceeds decode limit: bytes={}, limit={}",
            content.len(),
            MAX_TEXT_BYTES
        ));
    }
    Ok(String::from_utf8_lossy(content).to_string())
}

fn is_text_like_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("javascript")
        || content_type.starts_with("dob/")
        || content_type.contains("svg")
}

fn uri_seems_image(uri: &str) -> bool {
    let normalized = uri.trim().to_ascii_lowercase();
    normalized.starts_with("data:image/")
        || normalized.ends_with(".png")
        || normalized.ends_with(".jpg")
        || normalized.ends_with(".jpeg")
        || normalized.ends_with(".gif")
        || normalized.ends_with(".webp")
        || normalized.ends_with(".svg")
        || normalized.ends_with(".avif")
}

fn extract_dob_media_sources(
    content_text: &str,
    cluster_description: Option<&str>,
    issues: &mut Vec<String>,
) -> (Vec<SporeMediaSource>, bool) {
    let metadata = cluster_description.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    if metadata.is_none() {
        issues.push("missing or invalid cluster description for DOB media analysis".to_string());
    }
    let dna_hex = parse_dna_hex_from_content_text(content_text);
    if dna_hex.is_none() {
        issues.push("missing or invalid DNA for DOB media analysis".to_string());
    }

    let mut sources = Vec::new();

    let (Some(meta), Some(dna_hex)) = (metadata.as_ref(), dna_hex.as_ref()) else {
        return (sources, false);
    };

    let dna_bytes = match hex::decode(dna_hex) {
        Ok(bytes) => bytes,
        Err(err) => {
            issues.push(format!("failed to decode DOB DNA hex: {}", err));
            return (sources, false);
        }
    };

    let patterns = extract_dob0_pattern(meta);
    let mut traits: HashMap<String, String> = HashMap::new();
    for pattern in patterns {
        let start = pattern.dna_offset.min(dna_bytes.len());
        let end = (start + pattern.dna_length).min(dna_bytes.len());
        let raw = decode_dob0_trait_value(&pattern, &dna_bytes[start..end]);
        traits.insert(pattern.trait_name.clone(), format_json_value(&raw));
    }

    let dob1_patterns = extract_dob1_pattern(meta);
    let Some(svg_markup) = build_dob1_svg(&dob1_patterns, &traits) else {
        if !dob1_patterns.is_empty() {
            issues.push("DOB metadata included dob1 pattern but produced empty SVG".to_string());
        }
        return (sources, false);
    };
    extract_uri_sources(&svg_markup, "dob_svg", &mut sources);
    (sources, true)
}

fn classify_dependency_tier(scheme: &str) -> StorageDependencyTier {
    match scheme {
        "http" | "https" => StorageDependencyTier::CentralizedDependent,
        "ipfs" | "ar" => StorageDependencyTier::DecentralizedExternal,
        "btcfs" | "ckbfs" | "data" => StorageDependencyTier::FullyOnchain,
        _ => StorageDependencyTier::Unknown,
    }
}

fn extract_uri_sources(text: &str, source_location: &str, out: &mut Vec<SporeMediaSource>) {
    let normalized = text.to_ascii_lowercase();
    let schemes = [
        "https://", "http://", "ipfs://", "ar://", "btcfs://", "ckbfs://", "data:",
    ];

    let mut cursor = 0usize;
    while cursor < normalized.len() && out.len() < MAX_MEDIA_SOURCES * 2 {
        let hay = &normalized[cursor..];
        let mut found: Option<(usize, &str)> = None;
        for scheme in schemes {
            if let Some(pos) = hay.find(scheme) {
                match found {
                    Some((best_pos, _)) if pos >= best_pos => {}
                    _ => found = Some((pos, scheme)),
                }
            }
        }

        let Some((relative_pos, scheme_prefix)) = found else {
            break;
        };
        let start = cursor + relative_pos;
        let end = find_uri_end(text, start);
        if end <= start {
            cursor = start.saturating_add(1);
            continue;
        }
        let uri = sanitize_uri_candidate(&text[start..end]);
        cursor = end;
        if uri.is_empty() {
            continue;
        }
        let uri_lower = uri.to_ascii_lowercase();
        if uri_lower == "http://www.w3.org/2000/svg" || uri_lower == "https://www.w3.org/2000/svg" {
            continue;
        }
        let scheme = normalize_scheme(uri, scheme_prefix);
        out.push(SporeMediaSource {
            uri: uri.to_string(),
            scheme: scheme.clone(),
            source_location: source_location.to_string(),
            dependency_tier: classify_dependency_tier(&scheme),
        });
    }
}

fn find_uri_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut idx = start;
    while idx < bytes.len() {
        let c = bytes[idx] as char;
        if c.is_whitespace()
            || c == '"'
            || c == '\''
            || c == '<'
            || c == '>'
            || c == '('
            || c == ')'
            || c == '['
            || c == ']'
            || c == '{'
            || c == '}'
            || c == ','
        {
            break;
        }
        idx += 1;
    }
    idx
}

fn sanitize_uri_candidate(candidate: &str) -> &str {
    candidate.trim_matches(|c: char| c == '"' || c == '\'' || c == ';')
}

fn normalize_scheme(uri: &str, fallback_prefix: &str) -> String {
    if let Some((scheme, _)) = uri.split_once(':') {
        return scheme.to_ascii_lowercase();
    }
    fallback_prefix.trim_end_matches("://").to_ascii_lowercase()
}

fn dedupe_and_limit_sources(sources: &mut Vec<SporeMediaSource>, limit: usize) {
    let mut seen = HashSet::new();
    sources.retain(|source| {
        let key = format!("{}|{}", source.source_location, source.uri);
        seen.insert(key)
    });
    if sources.len() > limit {
        sources.truncate(limit);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_http_as_centralized_dependency() {
        let profile =
            analyze_spore_media_profile("text/plain", b"https://cdn.example.com/image.png", None);
        assert_eq!(profile.tier, StorageDependencyTier::CentralizedDependent);
        assert_eq!(profile.sources.len(), 1);
        assert_eq!(profile.sources[0].scheme, "https");
    }

    #[test]
    fn classifies_btcfs_svg_as_fully_onchain() {
        let profile = analyze_spore_media_profile(
            "image/svg+xml",
            br#"<svg><image href="btcfs://abcd1234i0" /></svg>"#,
            None,
        );
        assert_eq!(profile.tier, StorageDependencyTier::FullyOnchain);
        assert!(profile.has_renderable_image);
        assert!(profile.sources.iter().any(|s| s.scheme == "btcfs"));
    }

    #[test]
    fn classifies_ipfs_as_decentralized_external() {
        let profile =
            analyze_spore_media_profile("text/plain", b"ipfs://QmHashValue1234567890", None);
        assert_eq!(profile.tier, StorageDependencyTier::DecentralizedExternal);
    }

    #[test]
    fn dob_uses_selected_svg_option_instead_of_all_options() {
        let metadata = serde_json::json!({
            "dob": {
                "ver": 0,
                "pattern": [
                    ["Background", "String", 0, 1, "options", ["http://bad.example/img.png", "btcfs://goodasseti0"]]
                ],
                "decoders": [
                    {
                        "pattern": [
                            ["IMAGE.0", "attributes", "Background", "raw", "xmlns='http://www.w3.org/2000/svg' viewBox='0 0 500 500'"],
                            ["IMAGE.0", "elements", "Background", "options", [
                                ["http://bad.example/img.png", "<image href='http://bad.example/img.png' />"],
                                ["btcfs://goodasseti0", "<image href='btcfs://goodasseti0' />"]
                            ]]
                        ]
                    }
                ]
            }
        })
        .to_string();

        let profile = analyze_spore_media_profile("dob/0", b"01", Some(&metadata));
        assert_eq!(profile.tier, StorageDependencyTier::FullyOnchain);
        assert!(profile
            .sources
            .iter()
            .any(|source| source.uri.contains("btcfs://goodasseti0")));
    }
}
