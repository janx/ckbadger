use anyhow::{anyhow, bail, Result};
use ckbadger_store::types::ObjectStandard;
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
struct ScriptNameOverridesDoc {
    #[serde(default)]
    nft_storage_tier_overrides: HashMap<String, String>,
}

static NFT_STORAGE_TIER_OVERRIDES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let parsed = load_script_name_overrides_doc();

    let mut overrides = HashMap::new();
    for (standard, tier) in parsed.nft_storage_tier_overrides {
        let standard = normalize_standard_alias_key(&standard);
        let normalized_tier = tier.trim().to_ascii_lowercase();
        if !matches!(
            normalized_tier.as_str(),
            "fully_onchain" | "decentralized_external" | "centralized_dependent" | "unknown"
        ) {
            panic!(
                "invalid nft_storage_tier_overrides tier for standard='{}': {}",
                standard, normalized_tier
            );
        }
        overrides.insert(standard, normalized_tier);
    }

    overrides
});

fn default_nft_storage_tier_overrides() -> HashMap<String, String> {
    let mut defaults = HashMap::new();
    for standard in [".bit", "dotbit", "did:ckb", "did_ckb"] {
        defaults.insert(
            normalize_standard_alias_key(standard),
            "fully_onchain".to_string(),
        );
    }
    defaults
}

fn load_script_name_overrides_doc() -> ScriptNameOverridesDoc {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/script-name-overrides.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            return ScriptNameOverridesDoc {
                nft_storage_tier_overrides: default_nft_storage_tier_overrides(),
            };
        }
    };

    serde_json::from_str(&content).unwrap_or_else(|err| {
        panic!(
            "invalid docs/script-name-overrides.json at {}: {err}",
            path.display()
        );
    })
}

fn normalize_standard_alias_key(standard: &str) -> String {
    let normalized = standard.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "did_ckb" => "did:ckb".to_string(),
        _ => normalized,
    }
}

pub fn resolve_nft_collection_storage_tier_override(standard: &str) -> Option<&'static str> {
    let normalized = normalize_standard_alias_key(standard);
    NFT_STORAGE_TIER_OVERRIDES
        .get(&normalized)
        .map(String::as_str)
}

/// Apply one daily delta to live/occupied capacity with strict invariant checks.
pub fn apply_live_capacity_delta(
    live_capacity: i128,
    live_occupied: i128,
    capacity_delta: i128,
    occupied_delta: i128,
    context: &str,
) -> Result<(i128, i128)> {
    let next_capacity = live_capacity + capacity_delta;
    if next_capacity < 0 {
        bail!(
            "live capacity underflow while {}: prev={}, delta={}, next={}",
            context,
            live_capacity,
            capacity_delta,
            next_capacity
        );
    }

    let next_occupied = live_occupied + occupied_delta;
    if next_occupied < 0 {
        bail!(
            "live occupied capacity underflow while {}: prev={}, delta={}, next={}",
            context,
            live_occupied,
            occupied_delta,
            next_occupied
        );
    }

    if next_occupied > next_capacity {
        bail!(
            "live occupied capacity exceeds live capacity while {}: occupied={}, capacity={}",
            context,
            next_occupied,
            next_capacity
        );
    }

    Ok((next_capacity, next_occupied))
}

/// Accumulate live capacity/occupied capacity from ordered daily deltas.
pub fn accumulate_live_capacity<I>(deltas: I) -> Result<(i128, i128)>
where
    I: IntoIterator<Item = (i128, i128)>,
{
    let mut live_capacity: i128 = 0;
    let mut live_occupied: i128 = 0;

    for (idx, (capacity_delta, occupied_delta)) in deltas.into_iter().enumerate() {
        (live_capacity, live_occupied) = apply_live_capacity_delta(
            live_capacity,
            live_occupied,
            capacity_delta,
            occupied_delta,
            "accumulating live capacity",
        )
        .map_err(|e| anyhow!("delta #{} invalid: {}", idx + 1, e))?;
    }

    Ok((live_capacity, live_occupied))
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

    match store.get_spore(cluster_id) {
        Ok(Some(entry)) if entry.standard == ObjectStandard::SporeCluster => {
            non_empty_name(entry.name.as_deref())
        }
        _ => None,
    }
}

const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";
const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";

/// Resolve the display-level standard for a collection, overriding for
/// sentinel identity collections whose `ObjectCollectionAggregate.standard`
/// cannot represent dotbit/did:ckb (those live in `IdentityStandard`).
pub fn resolve_collection_standard(collection_id: &[u8], agg_standard: &str) -> String {
    if collection_id == DOTBIT_SENTINEL_COLLECTION {
        return "dotbit".to_string();
    }
    if collection_id == DID_CKB_SENTINEL_COLLECTION {
        return "did_ckb".to_string();
    }
    agg_standard.to_string()
}

/// Resolve an NFT collection display name.
///
/// Priority:
/// 1) non-empty aggregate name
/// 2) standard fallback (currently ".bit" for dotbit)
pub fn resolve_nft_collection_name(standard: &str, aggregate_name: Option<&str>) -> Option<String> {
    if let Some(name) = non_empty_name(aggregate_name) {
        return Some(name);
    }

    if standard.eq_ignore_ascii_case("dotbit") {
        return Some(".bit".to_string());
    }
    if standard.eq_ignore_ascii_case("did_ckb") || standard.eq_ignore_ascii_case("did:ckb") {
        return Some("did:ckb".to_string());
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
    fn resolve_nft_name_prefers_aggregate_name() {
        assert_eq!(
            resolve_nft_collection_name("dotbit", Some("  Dotbit Club  ")).as_deref(),
            Some("Dotbit Club")
        );
    }

    #[test]
    fn resolve_nft_name_falls_back_to_dotbit_default() {
        assert_eq!(
            resolve_nft_collection_name("dotbit", None).as_deref(),
            Some(".bit")
        );
        assert_eq!(
            resolve_nft_collection_name("DOTBIT", Some("   ")).as_deref(),
            Some(".bit")
        );
    }

    #[test]
    fn resolve_nft_name_returns_none_for_other_standards_without_name() {
        assert!(resolve_nft_collection_name("m-nft", None).is_none());
    }

    #[test]
    fn resolve_nft_name_falls_back_to_did_ckb_default() {
        assert_eq!(
            resolve_nft_collection_name("did_ckb", None).as_deref(),
            Some("did:ckb")
        );
        assert_eq!(
            resolve_nft_collection_name("did:ckb", Some("   ")).as_deref(),
            Some("did:ckb")
        );
    }

    #[test]
    fn nft_storage_tier_overrides_cover_dotbit_and_did_ckb() {
        assert_eq!(
            resolve_nft_collection_storage_tier_override("dotbit"),
            Some("fully_onchain")
        );
        assert_eq!(
            resolve_nft_collection_storage_tier_override(".bit"),
            Some("fully_onchain")
        );
        assert_eq!(
            resolve_nft_collection_storage_tier_override("did_ckb"),
            Some("fully_onchain")
        );
        assert_eq!(
            resolve_nft_collection_storage_tier_override("did:ckb"),
            Some("fully_onchain")
        );
        assert_eq!(resolve_nft_collection_storage_tier_override("m-nft"), None);
    }

    #[test]
    fn accumulate_live_capacity_sums_valid_deltas() {
        let deltas = vec![(100, 60), (-30, -10), (20, 5)];
        let (capacity, occupied) = accumulate_live_capacity(deltas).unwrap();
        assert_eq!(capacity, 90);
        assert_eq!(occupied, 55);
    }

    #[test]
    fn accumulate_live_capacity_errors_on_negative_capacity() {
        let deltas = vec![(100, 60), (-150, -10)];
        let err = accumulate_live_capacity(deltas).unwrap_err();
        assert!(err.to_string().contains("live capacity underflow"));
    }

    #[test]
    fn accumulate_live_capacity_errors_on_negative_occupied() {
        let deltas = vec![(100, 60), (0, -80)];
        let err = accumulate_live_capacity(deltas).unwrap_err();
        assert!(err.to_string().contains("live occupied capacity underflow"));
    }

    #[test]
    fn accumulate_live_capacity_errors_when_occupied_exceeds_capacity() {
        let deltas = vec![(100, 60), (-30, -10), (0, 50)];
        let err = accumulate_live_capacity(deltas).unwrap_err();
        assert!(err
            .to_string()
            .contains("live occupied capacity exceeds live capacity"));
    }
}
