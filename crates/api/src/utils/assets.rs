use anyhow::{anyhow, bail, Result};
use ckbadger_store::types::{
    ObjectStandard, BIT_CELL_SENTINEL_COLLECTION, DID_CKB_SENTINEL_COLLECTION,
    DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

fn non_empty_name(name: Option<&str>) -> Option<String> {
    let trimmed = name?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Default, Deserialize)]
struct NftTiersDoc {
    #[serde(default)]
    overrides: HashMap<String, String>,
}

static OBJECT_COMPOSITION_TIER_OVERRIDES: LazyLock<HashMap<String, String>> =
    LazyLock::new(
        || match load_and_validate_object_composition_tier_overrides() {
            Ok(overrides) => overrides,
            Err(e) => panic!("object_composition_tier_overrides initialization failed: {e}"),
        },
    );

const VALID_TIERS: &[&str] = &[
    "btc_ckb",
    "pure_ckb",
    "decentralized_mixture",
    "centralized_mixture",
    "unknown",
];

fn default_object_composition_tier_overrides() -> HashMap<String, String> {
    let mut defaults = HashMap::new();
    for standard in [
        ".bit",
        "dotbit",
        ".bit-cell",
        "bit-cell",
        "bit_cell",
        "did:ckb",
        "did_ckb",
    ] {
        defaults.insert(
            normalize_standard_alias_key(standard),
            "pure_ckb".to_string(),
        );
    }
    defaults
}

fn load_and_validate_object_composition_tier_overrides() -> Result<HashMap<String, String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/metadata/object-tiers.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            // File may not exist in deployed binaries (CARGO_MANIFEST_DIR is
            // baked at compile time). Fall back to hardcoded defaults.
            return Ok(default_object_composition_tier_overrides());
        }
    };

    let parsed: NftTiersDoc = toml::from_str(&content).map_err(|e| {
        anyhow!(
            "malformed docs/metadata/object-tiers.toml at {}: {}",
            path.display(),
            e
        )
    })?;

    let mut overrides = HashMap::new();
    for (standard, tier) in parsed.overrides {
        let standard = normalize_standard_alias_key(&standard);
        let normalized_tier = tier.trim().to_ascii_lowercase();
        if !VALID_TIERS.contains(&normalized_tier.as_str()) {
            bail!(
                "invalid object_composition_tier_overrides tier for standard='{}': '{}' (valid: {})",
                standard,
                normalized_tier,
                VALID_TIERS.join(", ")
            );
        }
        overrides.insert(standard, normalized_tier);
    }

    Ok(overrides)
}

fn normalize_standard_alias_key(standard: &str) -> String {
    let normalized = standard.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "did_ckb" => "did:ckb".to_string(),
        ".bit-cell" | "bit-cell" => "bit_cell".to_string(),
        _ => normalized,
    }
}

pub fn resolve_object_collection_composition_tier_override(standard: &str) -> Option<&'static str> {
    let normalized = normalize_standard_alias_key(standard);
    OBJECT_COMPOSITION_TIER_OVERRIDES
        .get(&normalized)
        .map(String::as_str)
}

/// Apply one daily delta to owned capacity/knowledge with strict invariant checks.
pub fn apply_owned_capacity_delta(
    owned_capacity: i128,
    owned_knowledge: i128,
    capacity_delta: i128,
    used_delta: i128,
    context: &str,
) -> Result<(i128, i128)> {
    let next_capacity = owned_capacity + capacity_delta;
    if next_capacity < 0 {
        bail!(
            "owned capacity underflow while {}: prev={}, delta={}, next={}",
            context,
            owned_capacity,
            capacity_delta,
            next_capacity
        );
    }

    let next_used = owned_knowledge + used_delta;
    if next_used < 0 {
        bail!(
            "owned knowledge underflow while {}: prev={}, delta={}, next={}",
            context,
            owned_knowledge,
            used_delta,
            next_used
        );
    }

    if next_used > next_capacity {
        bail!(
            "owned knowledge exceeds owned capacity while {}: used={}, capacity={}",
            context,
            next_used,
            next_capacity
        );
    }

    Ok((next_capacity, next_used))
}

/// Accumulate owned capacity/knowledge from ordered daily deltas.
pub fn accumulate_owned_capacity<I>(deltas: I) -> Result<(i128, i128)>
where
    I: IntoIterator<Item = (i128, i128)>,
{
    let mut owned_capacity: i128 = 0;
    let mut owned_knowledge: i128 = 0;

    for (idx, (capacity_delta, used_delta)) in deltas.into_iter().enumerate() {
        (owned_capacity, owned_knowledge) = apply_owned_capacity_delta(
            owned_capacity,
            owned_knowledge,
            capacity_delta,
            used_delta,
            "accumulating owned capacity",
        )
        .map_err(|e| anyhow!("delta #{} invalid: {}", idx + 1, e))?;
    }

    Ok((owned_capacity, owned_knowledge))
}

/// Resolve a DOB collection display name.
///
/// Priority:
/// 1) `cluster_agg.name` (if non-empty)
/// 2) cluster entry name from `spore_data` (if non-empty)
pub fn resolve_dob_collection_name(
    store: &CkbadgerStore,
    cluster_id: &[u8],
    aggregate_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = non_empty_name(aggregate_name) {
        return Some(name);
    }

    if cluster_id == SOLE_SPORES_SENTINEL_COLLECTION {
        return Some("[Sole Spores]".to_string());
    }

    match store.get_spore(cluster_id) {
        Ok(Some(entry)) if entry.standard == ObjectStandard::SporeCluster => {
            non_empty_name(entry.name.as_deref())
        }
        _ => None,
    }
}

/// Resolve the display-level standard for a collection, overriding for
/// sentinel identity collections whose `MnftCollectionAggregate.standard`
/// cannot represent dotbit/did:ckb (those live in `IdentityStandard`).
pub fn resolve_collection_standard(collection_id: &[u8], agg_standard: &str) -> String {
    if collection_id == DOTBIT_SENTINEL_COLLECTION {
        return "dotbit".to_string();
    }
    if collection_id == DID_CKB_SENTINEL_COLLECTION {
        return "did_ckb".to_string();
    }
    if collection_id == BIT_CELL_SENTINEL_COLLECTION {
        return "bit_cell".to_string();
    }
    agg_standard.to_string()
}

/// Resolve an object collection display name.
///
/// Priority:
/// 1) non-empty aggregate name
/// 2) standard fallback (currently ".bit" for dotbit)
pub fn resolve_object_collection_name(
    standard: &str,
    aggregate_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = non_empty_name(aggregate_name) {
        return Some(name);
    }

    if standard.eq_ignore_ascii_case("dotbit") {
        return Some(".bit".to_string());
    }
    if standard.eq_ignore_ascii_case("did_ckb") || standard.eq_ignore_ascii_case("did:ckb") {
        return Some("did:ckb".to_string());
    }
    if standard.eq_ignore_ascii_case("bit_cell") || standard.eq_ignore_ascii_case("bit-cell") {
        return Some(".bit Cell".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::{ObjectEntry, ObjectExtra};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn resolve_name_prefers_aggregate_name() {
        let (_dir, store) = test_store();
        let cluster_id = [0x11u8; 32];

        let resolved = resolve_dob_collection_name(&store, &cluster_id, Some("Agg Name"));
        assert_eq!(resolved.as_deref(), Some("Agg Name"));
    }

    #[test]
    fn resolve_name_falls_back_to_cluster_entry_name() {
        let (_dir, store) = test_store();
        let cluster_id = [0x22u8; 32];

        let entry = ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x33; 32]),
            name: Some("Cluster Entry Name".to_string()),
            description: Some("desc".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![0x44; 32],
            extra: ObjectExtra::SporeCluster,
        };
        store.put_spore_direct(&cluster_id, &entry).unwrap();

        let resolved = resolve_dob_collection_name(&store, &cluster_id, None);
        assert_eq!(resolved.as_deref(), Some("Cluster Entry Name"));
    }

    #[test]
    fn resolve_name_treats_blank_as_missing() {
        let (_dir, store) = test_store();
        let cluster_id = [0x55u8; 32];

        let entry = ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x66; 32]),
            name: Some("   ".to_string()),
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0x77; 32],
            extra: ObjectExtra::SporeCluster,
        };
        store.put_spore_direct(&cluster_id, &entry).unwrap();

        assert!(resolve_dob_collection_name(&store, &cluster_id, Some("  ")).is_none());
    }

    #[test]
    fn resolve_object_name_prefers_aggregate_name() {
        assert_eq!(
            resolve_object_collection_name("dotbit", Some("  Dotbit Club  ")).as_deref(),
            Some("Dotbit Club")
        );
    }

    #[test]
    fn resolve_object_name_falls_back_to_dotbit_default() {
        assert_eq!(
            resolve_object_collection_name("dotbit", None).as_deref(),
            Some(".bit")
        );
        assert_eq!(
            resolve_object_collection_name("DOTBIT", Some("   ")).as_deref(),
            Some(".bit")
        );
    }

    #[test]
    fn resolve_object_name_returns_none_for_other_standards_without_name() {
        assert!(resolve_object_collection_name("m-nft", None).is_none());
    }

    #[test]
    fn resolve_object_name_falls_back_to_did_ckb_default() {
        assert_eq!(
            resolve_object_collection_name("did_ckb", None).as_deref(),
            Some("did:ckb")
        );
        assert_eq!(
            resolve_object_collection_name("did:ckb", Some("   ")).as_deref(),
            Some("did:ckb")
        );
    }

    #[test]
    fn resolve_object_name_falls_back_to_bit_cell_default() {
        assert_eq!(
            resolve_object_collection_name("bit_cell", None).as_deref(),
            Some(".bit Cell")
        );
    }

    #[test]
    fn object_composition_tier_overrides_cover_identity_standards() {
        assert_eq!(
            resolve_object_collection_composition_tier_override("dotbit"),
            Some("pure_ckb")
        );
        assert_eq!(
            resolve_object_collection_composition_tier_override(".bit"),
            Some("pure_ckb")
        );
        assert_eq!(
            resolve_object_collection_composition_tier_override("did_ckb"),
            Some("pure_ckb")
        );
        assert_eq!(
            resolve_object_collection_composition_tier_override("bit_cell"),
            Some("pure_ckb")
        );
        assert_eq!(
            resolve_object_collection_composition_tier_override("did:ckb"),
            Some("pure_ckb")
        );
        assert_eq!(
            resolve_object_collection_composition_tier_override("m-nft"),
            None
        );
    }

    #[test]
    fn accumulate_owned_capacity_sums_valid_deltas() {
        let deltas = vec![(100, 60), (-30, -10), (20, 5)];
        let (capacity, used) = accumulate_owned_capacity(deltas).unwrap();
        assert_eq!(capacity, 90);
        assert_eq!(used, 55);
    }

    #[test]
    fn accumulate_owned_capacity_errors_on_negative_capacity() {
        let deltas = vec![(100, 60), (-150, -10)];
        let err = accumulate_owned_capacity(deltas).unwrap_err();
        assert!(err.to_string().contains("owned capacity underflow"));
    }

    #[test]
    fn accumulate_owned_capacity_errors_on_negative_used() {
        let deltas = vec![(100, 60), (0, -80)];
        let err = accumulate_owned_capacity(deltas).unwrap_err();
        assert!(err.to_string().contains("owned knowledge underflow"));
    }

    #[test]
    fn accumulate_owned_capacity_errors_when_used_exceeds_capacity() {
        let deltas = vec![(100, 60), (-30, -10), (0, 50)];
        let err = accumulate_owned_capacity(deltas).unwrap_err();
        assert!(err
            .to_string()
            .contains("owned knowledge exceeds owned capacity"));
    }

    #[test]
    fn resolve_dob_name_returns_sole_spores_for_sentinel() {
        use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
        let (_dir, store) = test_store();
        let resolved = resolve_dob_collection_name(&store, &SOLE_SPORES_SENTINEL_COLLECTION, None);
        assert_eq!(resolved.as_deref(), Some("[Sole Spores]"));
    }

    #[test]
    fn resolve_dob_name_aggregate_name_overrides_sentinel() {
        use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
        let (_dir, store) = test_store();
        let resolved = resolve_dob_collection_name(
            &store,
            &SOLE_SPORES_SENTINEL_COLLECTION,
            Some("Custom Name"),
        );
        assert_eq!(resolved.as_deref(), Some("Custom Name"));
    }
}
