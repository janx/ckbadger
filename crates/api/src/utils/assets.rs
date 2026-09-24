use anyhow::{anyhow, bail, Result};
use ckbadger_store::types::{
    identity_alias, identity_display_name, identity_sentinel_standard, ObjectStandard,
    SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::collections::HashMap;
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
    LazyLock::new(default_object_composition_tier_overrides);

const VALID_TIERS: &[&str] = &[
    "btc_ckb",
    "pure_ckb",
    "decentralized_mixture",
    "centralized_mixture",
    "unknown",
];

/// `docs/metadata/object-tiers.toml`, bundled at compile time. The one source
/// of composition-tier overrides: a deployed binary has no `docs/` tree to
/// read, and a hand-typed copy of this table for that case drifted (it never
/// learned `.cell`).
const BUNDLED_OBJECT_TIERS: &str = include_str!("../../../../docs/metadata/object-tiers.toml");

/// The composition-tier overrides every binary carries: the bundled document,
/// validated. A malformed document or an unknown tier is an error, not a
/// silently smaller table.
fn default_object_composition_tier_overrides() -> HashMap<String, String> {
    parse_object_composition_tier_overrides(BUNDLED_OBJECT_TIERS)
        .unwrap_or_else(|e| panic!("bundled docs/metadata/object-tiers.toml is invalid: {e}"))
}

fn parse_object_composition_tier_overrides(content: &str) -> Result<HashMap<String, String>> {
    let parsed: NftTiersDoc = toml::from_str(content)
        .map_err(|e| anyhow!("malformed docs/metadata/object-tiers.toml: {e}"))?;

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
/// cannot represent an identity standard (those live in `IdentityStandard`,
/// resolved through the store's one identity table).
pub fn resolve_collection_standard(collection_id: &[u8], agg_standard: &str) -> String {
    match identity_sentinel_standard(collection_id) {
        Some(standard) => standard.asset_standard().to_string(),
        None => agg_standard.to_string(),
    }
}

/// Resolve an object collection display name.
///
/// Priority:
/// 1) non-empty aggregate name
/// 2) an identity standard's display name from the store's identity table
pub fn resolve_object_collection_name(
    standard: &str,
    aggregate_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = non_empty_name(aggregate_name) {
        return Some(name);
    }

    identity_alias(standard).map(|standard| identity_display_name(standard).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::{ObjectEntry, ObjectExtra};
    use tempfile::TempDir;

    /// Identity collections reach this helper with `ObjectStandard::Spore` as
    /// a placeholder — `ObjectStandard` has no identity variants — so every
    /// identity sentinel needs its own arm or the collection reports itself as
    /// a Spore collection, and the inventory's standard filter matches nothing.
    #[test]
    fn every_identity_sentinel_resolves_to_its_own_standard() {
        use ckbadger_store::types::{
            BIT_CELL_SENTINEL_COLLECTION, DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION,
            DOTCELL_SENTINEL_COLLECTION,
        };
        for (sentinel, expected) in [
            (DOTBIT_SENTINEL_COLLECTION, "dotbit"),
            (BIT_CELL_SENTINEL_COLLECTION, "bit_cell"),
            (DID_CKB_SENTINEL_COLLECTION, "did_ckb"),
            (DOTCELL_SENTINEL_COLLECTION, "dotcell"),
        ] {
            assert_eq!(
                resolve_collection_standard(&sentinel, "spore"),
                expected,
                "sentinel {expected}"
            );
        }
        // A real object collection keeps the aggregate's own standard.
        assert_eq!(resolve_collection_standard(&[0x11; 32], "m-nft"), "m-nft");
    }

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

    /// What a binary knows with no `docs/` tree beside it — every deployed
    /// binary. That table must be the bundled document itself: a hand-typed
    /// copy of it silently missed `.cell` when the document gained it.
    #[test]
    fn tiers_without_a_filesystem_are_the_bundled_document() {
        let overrides = default_object_composition_tier_overrides();
        for standard in ckbadger_store::types::IDENTITY_STANDARDS {
            assert_eq!(
                overrides
                    .get(&normalize_standard_alias_key(standard.asset_standard()))
                    .map(String::as_str),
                Some("pure_ckb"),
                "identity standard `{}` has no bundled tier",
                standard.asset_standard()
            );
        }
        assert_eq!(
            overrides.get(".cell").map(String::as_str),
            Some("pure_ckb"),
            "the display-name key the document lists"
        );
        let document: NftTiersDoc =
            toml::from_str(include_str!("../../../../docs/metadata/object-tiers.toml")).unwrap();
        assert_eq!(
            overrides.len(),
            document
                .overrides
                .keys()
                .map(|key| normalize_standard_alias_key(key))
                .collect::<std::collections::HashSet<_>>()
                .len(),
            "exactly the document's keys, nothing added"
        );
    }

    #[test]
    fn tier_document_rejects_unknown_tiers_and_malformed_toml() {
        let parsed =
            parse_object_composition_tier_overrides("[overrides]\n\"Bit-Cell\" = \" Pure_CKB \"\n")
                .unwrap();
        assert_eq!(parsed.get("bit_cell").map(String::as_str), Some("pure_ckb"));
        let err = parse_object_composition_tier_overrides("[overrides]\n\".cell\" = \"gold\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("'gold'"), "{err}");
        assert!(parse_object_composition_tier_overrides("[overrides\n").is_err());
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
