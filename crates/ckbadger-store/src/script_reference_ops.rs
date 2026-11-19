//! Script reference operations.

use crate::bytes_to_hex;
use crate::keys;
use crate::{CkbadgerStore, ScriptReferenceInfo};

pub type ScriptReferenceInfoEntry = ((Vec<u8>, u8), ScriptReferenceInfo);

/// What one script reference form runs now: THE input of the family rule
/// (lane rd/api-lock-names, the lead's ruling on a type reference whose live
/// code cells hold several binaries). The indexer's rollup and the API read
/// every form's family through [`ScriptFamilyMembershipIndex::family_of_form`]
/// from this, so counting and naming agree by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceCode {
    /// One binary: a data form's own hash, a native Type ID reference's
    /// binding, or the one binary a generic type reference's live code cells
    /// hold.
    One(Vec<u8>),
    /// A generic (non Type ID) type reference whose live code cells hold
    /// several distinct binaries, sorted. It runs whichever of them a spender
    /// puts in the cell deps: none of them is "the" code it runs.
    Several(Vec<Vec<u8>>),
    /// No binary now: a retired type reference (its chain-observed binding,
    /// if any, keeps its family) or a data form no code cell carries.
    None,
}

impl ReferenceCode {
    /// A generic type reference's code from the distinct binaries its live
    /// code cells hold: none, one, or several.
    pub fn of_live_binaries(binaries: std::collections::BTreeSet<Vec<u8>>) -> Self {
        match binaries.len() {
            0 => Self::None,
            1 => Self::One(binaries.into_iter().next().expect("one binary")),
            _ => Self::Several(binaries.into_iter().collect()),
        }
    }

    /// The one binary the form runs, where it runs exactly one.
    pub fn current(&self) -> Option<&[u8]> {
        match self {
            Self::One(binary) => Some(binary),
            Self::Several(_) | Self::None => None,
        }
    }
}

/// Metadata-backed family membership is independent of current executable
/// resolution. Build once per rollup/request, then resolve each reference form.
#[derive(Debug, Default)]
pub struct ScriptFamilyMembershipIndex {
    version_families: std::collections::HashMap<Vec<u8>, String>,
    type_id_families: std::collections::HashMap<Vec<u8>, String>,
}

impl ScriptFamilyMembershipIndex {
    pub fn new<'a>(
        versions: impl IntoIterator<Item = &'a crate::ScriptVersionInfo>,
    ) -> anyhow::Result<Self> {
        let versions: Vec<_> = versions.into_iter().collect();
        let mut index = Self::default();
        let mut registered = std::collections::HashMap::<Vec<u8>, String>::new();
        for version in &versions {
            let Some(family) = &version.family_id else {
                anyhow::ensure!(
                    version.registered_references.is_empty(),
                    "registered references lack a family: binary=0x{}",
                    bytes_to_hex(&version.version_hash)
                );
                continue;
            };
            if let Some(previous) = index
                .version_families
                .insert(version.version_hash.clone(), family.clone())
            {
                anyhow::ensure!(previous == *family, "binary registered in conflicting families: binary=0x{} families={previous}/{family}", bytes_to_hex(&version.version_hash));
            }
            for (reference, kind) in &version.registered_references {
                anyhow::ensure!(matches!(kind, 0 | 1 | 2 | 4), "unsupported registered reference hash type: binary=0x{} reference=0x{} hash_type={kind}", bytes_to_hex(&version.version_hash), bytes_to_hex(reference));
                if *kind == 1 {
                    if let Some(previous) = registered.insert(reference.clone(), family.clone()) {
                        anyhow::ensure!(previous == *family, "type-id reference registered in conflicting families: reference=0x{} families={previous}/{family}", bytes_to_hex(reference));
                    }
                }
            }
        }
        for version in versions {
            for (reference, block) in &version.type_id_deployments {
                let Some(family) = registered.get(reference) else {
                    continue;
                };
                anyhow::ensure!(version.family_id.as_ref() == Some(family),
                    "observed registered type-id contradicts family: reference=0x{} block={} binary=0x{} registered_family={} version_family={:?}", bytes_to_hex(reference), block, bytes_to_hex(&version.version_hash), family, version.family_id);
                index
                    .type_id_families
                    .insert(reference.clone(), family.clone());
            }
        }
        Ok(index)
    }

    /// THE family of one reference form from what it runs (the one rule,
    /// lane rd/api-lock-names): one binary, its family (a Type ID reference's
    /// observed lineage first); several live binaries, the family they are
    /// ALL versions of, else none (several families, or any binary outside
    /// the registry: a second binary stood beside a registered one does not
    /// earn the registered family's name or count); no binary, the family of
    /// its chain-observed `binding` (a retired reference: the code it ran).
    pub fn family_of_form(
        &self,
        reference_hash: &[u8],
        hash_type: u8,
        code: &ReferenceCode,
        binding: Option<&[u8]>,
    ) -> anyhow::Result<Option<&str>> {
        match code {
            ReferenceCode::One(binary) => {
                self.resolve_family(reference_hash, hash_type, Some(binary))
            }
            ReferenceCode::Several(binaries) => {
                anyhow::ensure!(
                    hash_type == 1 && binaries.len() > 1,
                    "several live binaries for a form that is not a type reference: reference=0x{} hash_type={} binaries={}",
                    bytes_to_hex(reference_hash),
                    hash_type,
                    binaries.len()
                );
                let mut families = binaries
                    .iter()
                    .map(|binary| self.version_families.get(binary).map(String::as_str));
                let first = families.next().flatten();
                Ok(match first {
                    Some(family) if families.all(|other| other == Some(family)) => Some(family),
                    _ => None,
                })
            }
            ReferenceCode::None => self.resolve_family(reference_hash, hash_type, binding),
        }
    }

    pub fn resolve_family(
        &self,
        reference_hash: &[u8],
        hash_type: u8,
        current_version_hash: Option<&[u8]>,
    ) -> anyhow::Result<Option<&str>> {
        match hash_type {
            0 | 2 | 4 => {
                if let Some(current) = current_version_hash {
                    anyhow::ensure!(current == reference_hash, "data reference resolved to a different binary: reference=0x{} hash_type={} binary=0x{}", bytes_to_hex(reference_hash), hash_type, bytes_to_hex(current));
                }
                Ok(self
                    .version_families
                    .get(reference_hash)
                    .map(String::as_str))
            }
            1 => {
                let historical_family = self.type_id_families.get(reference_hash);
                let current_family =
                    current_version_hash.and_then(|hash| self.version_families.get(hash));
                if let (Some(historical), Some(current)) = (historical_family, current_version_hash)
                {
                    anyhow::ensure!(current_family == Some(historical), "current type-id binary contradicts observed family: reference=0x{} binary=0x{} historical_family={} current_family={:?}", bytes_to_hex(reference_hash), bytes_to_hex(current), historical, current_family);
                }
                // These are separate facts: known lineage determines family
                // even with no executable; an active binary can supply its
                // imported family before any Type ID lineage is observed.
                Ok(match historical_family {
                    Some(family) => Some(family.as_str()),
                    None => current_family.map(String::as_str),
                })
            }
            _ => anyhow::bail!(
                "unsupported family reference hash type: reference=0x{} hash_type={}",
                bytes_to_hex(reference_hash),
                hash_type
            ),
        }
    }
}

/// Exact totals over distinct reference forms. This is the shared writer/API
/// operator; it never attributes retired reference usage to a historical binary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptFamilyUsage {
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub lock_cells_count: i64,
    pub type_cells_count: i64,
    pub capacity_sum: i128,
    pub used_capacity_sum: i128,
    pub owned_capacity_sum: i128,
    pub owned_knowledge_sum: i128,
}

impl ScriptFamilyUsage {
    pub fn add_reference(&mut self, info: &ScriptReferenceInfo) -> anyhow::Result<()> {
        let context = || {
            format!(
                "reference=0x{} hash_type={}",
                bytes_to_hex(&info.reference_hash),
                info.hash_type
            )
        };
        let add_i64 = |metric: &str, current: i64, left: i64, right: i64| -> anyhow::Result<i64> {
            anyhow::ensure!(
                left >= 0 && right >= 0,
                "negative family {metric}: {} left={left} right={right}",
                context()
            );
            current
                .checked_add(left)
                .and_then(|n| n.checked_add(right))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "family {metric} overflow: {} current={current} left={left} right={right}",
                        context()
                    )
                })
        };
        let add_i128 =
            |metric: &str, current: i128, left: i128, right: i128| -> anyhow::Result<i128> {
                anyhow::ensure!(
                    left >= 0 && right >= 0,
                    "negative family {metric}: {} left={left} right={right}",
                    context()
                );
                current
                    .checked_add(left)
                    .and_then(|n| n.checked_add(right))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                        "family {metric} overflow: {} current={current} left={left} right={right}",
                        context()
                    )
                    })
            };
        for (side, used, capacity, knowledge, owned) in [
            (
                "lock",
                info.lock_used_capacity_sum,
                info.lock_capacity_sum,
                info.lock_owned_knowledge_sum,
                info.lock_owned_capacity_sum,
            ),
            (
                "type",
                info.type_used_capacity_sum,
                info.type_capacity_sum,
                info.type_owned_knowledge_sum,
                info.type_owned_capacity_sum,
            ),
        ] {
            anyhow::ensure!(
                used <= capacity,
                "family {side} used capacity exceeds capacity: {} used={used} capacity={capacity}",
                context()
            );
            anyhow::ensure!(knowledge <= owned, "family {side} owned knowledge exceeds capacity: {} knowledge={knowledge} owned_capacity={owned}", context());
        }
        let next = Self {
            cells_count: add_i64(
                "cells_count",
                self.cells_count,
                info.lock_cells_count,
                info.type_cells_count,
            )?,
            live_cells_count: add_i64(
                "live_cells_count",
                self.live_cells_count,
                info.lock_live_cells_count,
                info.type_live_cells_count,
            )?,
            lock_cells_count: add_i64(
                "lock_cells_count",
                self.lock_cells_count,
                info.lock_cells_count,
                0,
            )?,
            type_cells_count: add_i64(
                "type_cells_count",
                self.type_cells_count,
                info.type_cells_count,
                0,
            )?,
            capacity_sum: add_i128(
                "capacity_sum",
                self.capacity_sum,
                info.lock_capacity_sum,
                info.type_capacity_sum,
            )?,
            used_capacity_sum: add_i128(
                "used_capacity_sum",
                self.used_capacity_sum,
                info.lock_used_capacity_sum,
                info.type_used_capacity_sum,
            )?,
            owned_capacity_sum: add_i128(
                "owned_capacity_sum",
                self.owned_capacity_sum,
                info.lock_owned_capacity_sum,
                info.type_owned_capacity_sum,
            )?,
            owned_knowledge_sum: add_i128(
                "owned_knowledge_sum",
                self.owned_knowledge_sum,
                info.lock_owned_knowledge_sum,
                info.type_owned_knowledge_sum,
            )?,
        };
        *self = next;
        Ok(())
    }

    pub fn apply_to(&self, family: &mut crate::ScriptFamilyInfo) {
        family.cells_count = self.cells_count;
        family.live_cells_count = self.live_cells_count;
        family.lock_cells_count = self.lock_cells_count;
        family.type_cells_count = self.type_cells_count;
        family.owned_capacity_sum = self.owned_capacity_sum;
        family.owned_knowledge_sum = self.owned_knowledge_sum;
    }

    pub fn validate_persisted(&self, family: &crate::ScriptFamilyInfo) -> anyhow::Result<()> {
        let expected = (
            self.cells_count,
            self.live_cells_count,
            self.lock_cells_count,
            self.type_cells_count,
            self.owned_capacity_sum,
            self.owned_knowledge_sum,
        );
        let actual = (
            family.cells_count,
            family.live_cells_count,
            family.lock_cells_count,
            family.type_cells_count,
            family.owned_capacity_sum,
            family.owned_knowledge_sum,
        );
        anyhow::ensure!(expected == actual, "persisted script family usage mismatch: family={} expected(cells,live,lock,type,owned_capacity,owned_knowledge)={expected:?} actual={actual:?}", family.family_id);
        Ok(())
    }
}

impl CkbadgerStore {
    pub fn get_script_reference_info(
        &self,
        hash_type: u8,
        reference_hash: &[u8],
    ) -> anyhow::Result<Option<ScriptReferenceInfo>> {
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        match self.get_cf(self.cf_script_reference_info(), &key)? {
            Some(value) => {
                let info = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script reference info: key=0x{}, error={}",
                        bytes_to_hex(&key),
                        e
                    )
                })?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    pub fn put_script_reference_info_direct(
        &self,
        hash_type: u8,
        reference_hash: &[u8],
        info: &ScriptReferenceInfo,
    ) -> anyhow::Result<()> {
        if info.hash_type != hash_type {
            anyhow::bail!(
                "put_script_reference_info_direct hash_type mismatch: key={}, value={}",
                hash_type,
                info.hash_type
            );
        }
        if info.reference_hash.as_slice() != reference_hash {
            anyhow::bail!(
                "put_script_reference_info_direct reference_hash mismatch: key=0x{}, value=0x{}",
                bytes_to_hex(reference_hash),
                bytes_to_hex(&info.reference_hash)
            );
        }
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        let value = bincode::serialize(info)?;
        self.put_cf(self.cf_script_reference_info(), &key, &value)
    }

    pub fn delete_script_reference_info_direct(
        &self,
        hash_type: u8,
        reference_hash: &[u8],
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        self.delete_cf(self.cf_script_reference_info(), &key)
    }

    pub fn get_script_reference_version_hash(
        &self,
        hash_type: u8,
        reference_hash: &[u8],
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        match self.get_cf(self.cf_script_reference_to_version(), &key)? {
            Some(value) => {
                if value.len() != 32 {
                    anyhow::bail!(
                        "malformed script_reference_to_version value: key=0x{}, expected_len=32, actual_len={}",
                        bytes_to_hex(&key),
                        value.len()
                    );
                }
                Ok(Some(value.to_vec()))
            }
            None => Ok(None),
        }
    }

    pub fn list_script_reference_infos(&self) -> anyhow::Result<Vec<ScriptReferenceInfoEntry>> {
        let iter = self.iterator_cf(
            self.cf_script_reference_info(),
            rocksdb::IteratorMode::Start,
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate script_reference_info in list_script_reference_infos: {}",
                    e
                )
            })?;
            let (hash_type, reference_hash) = keys::decode_script_reference_key(&key);
            let info: ScriptReferenceInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize script reference info in list_script_reference_infos: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push(((reference_hash, hash_type), info));
        }

        Ok(results)
    }

    pub fn put_script_reference_to_version_direct(
        &self,
        hash_type: u8,
        reference_hash: &[u8],
        version_hash: &[u8],
    ) -> anyhow::Result<()> {
        if version_hash.len() != 32 {
            anyhow::bail!(
                "put_script_reference_to_version_direct expects 32-byte version_hash, got {}",
                version_hash.len()
            );
        }
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        self.put_cf(self.cf_script_reference_to_version(), &key, version_hash)
    }

    pub fn delete_script_reference_to_version_direct(
        &self,
        hash_type: u8,
        reference_hash: &[u8],
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        self.delete_cf(self.cf_script_reference_to_version(), &key)
    }
}

#[cfg(test)]
mod tests {
    use crate::{keys, CkbadgerStore, ScriptReferenceInfo};

    /// The lead's ruling (lane rd/api-lock-names): a generic type reference
    /// whose live code cells hold several binaries is that family's only
    /// where every binary is a version of one registered family; two families,
    /// or a binary outside the registry, give it none. One binary is its
    /// family; none falls back to the chain-observed binding.
    #[test]
    fn a_form_running_several_binaries_has_a_family_only_when_all_are_one_familys() {
        use super::ReferenceCode;
        use crate::{ScriptFamilyMembershipIndex, ScriptVersionInfo};
        let version = |hash: u8, family: &str| ScriptVersionInfo {
            version_hash: vec![hash; 32],
            family_id: Some(family.into()),
            ..Default::default()
        };
        let (v1, v2, other) = (
            version(0x31, "xudt"),
            version(0x32, "xudt"),
            version(0x33, "omni"),
        );
        let index = ScriptFamilyMembershipIndex::new([&v1, &v2, &other]).unwrap();
        let generic = [0x77; 32];
        let several = |binaries: &[u8]| {
            ReferenceCode::of_live_binaries(binaries.iter().map(|b| vec![*b; 32]).collect())
        };
        assert_eq!(
            index
                .family_of_form(&generic, 1, &several(&[0x31, 0x32]), None)
                .unwrap(),
            Some("xudt"),
            "two versions of one registered family"
        );
        assert_eq!(
            index
                .family_of_form(&generic, 1, &several(&[0x31, 0x33]), None)
                .unwrap(),
            None,
            "two families"
        );
        assert_eq!(
            index
                .family_of_form(&generic, 1, &several(&[0x31, 0x99]), None)
                .unwrap(),
            None,
            "a binary outside the registry"
        );
        assert_eq!(
            index
                .family_of_form(&generic, 1, &several(&[0x31]), None)
                .unwrap(),
            Some("xudt"),
            "one binary"
        );
        assert_eq!(several(&[]), ReferenceCode::None);
        assert_eq!(
            index
                .family_of_form(&generic, 1, &ReferenceCode::None, Some(&[0x33; 32]))
                .unwrap(),
            Some("omni"),
            "retired: the chain-observed binding's family"
        );
        assert_eq!(
            index
                .family_of_form(&generic, 1, &ReferenceCode::None, None)
                .unwrap(),
            None
        );
        let error = index
            .family_of_form(&[0x31; 32], 0, &several(&[0x31, 0x32]), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a type reference"), "{error}");
    }

    #[test]
    fn family_membership_keeps_observed_lineage_separate_from_current_binary() {
        use crate::{ScriptFamilyMembershipIndex, ScriptVersionInfo};
        let reference = vec![0x11; 32];
        let old = ScriptVersionInfo {
            version_hash: vec![0x21; 32],
            family_id: Some("family".into()),
            canonical_reference_hash: Some(reference.clone()),
            canonical_hash_type: Some(1),
            registered_references: vec![(reference.clone(), 1)],
            type_id_deployments: vec![(reference.clone(), 10)],
            ..Default::default()
        };
        let new = ScriptVersionInfo {
            version_hash: vec![0x22; 32],
            registered_references: Vec::new(),
            type_id_deployments: vec![(reference.clone(), 20)],
            ..old.clone()
        };
        let metadata_only = ScriptVersionInfo {
            version_hash: vec![0x23; 32],
            canonical_reference_hash: Some(vec![0x12; 32]),
            registered_references: vec![(vec![0x12; 32], 1)],
            type_id_deployments: vec![],
            ..old.clone()
        };
        let index = ScriptFamilyMembershipIndex::new([&old, &new, &metadata_only]).unwrap();
        assert_eq!(
            index.resolve_family(&reference, 1, None).unwrap(),
            Some("family")
        );
        assert_eq!(
            index
                .resolve_family(&reference, 1, Some(&new.version_hash))
                .unwrap(),
            Some("family")
        );
        for kind in [0, 2, 4] {
            assert_eq!(
                index.resolve_family(&old.version_hash, kind, None).unwrap(),
                Some("family")
            );
        }
        assert_eq!(
            index.resolve_family(&[0x12; 32], 1, None).unwrap(),
            None,
            "labels alone do not invent observed lineage"
        );
        assert_eq!(index.resolve_family(&[0x99; 32], 1, None).unwrap(), None);
        assert!(index
            .resolve_family(&reference, 1, Some(&[0x99; 32]))
            .is_err());
        assert!(index
            .resolve_family(&old.version_hash, 0, Some(&new.version_hash))
            .is_err());
        assert!(index.resolve_family(&reference, 3, None).is_err());
        let conflicting = ScriptVersionInfo {
            version_hash: vec![0x24; 32],
            family_id: Some("other".into()),
            ..old.clone()
        };
        let error = ScriptFamilyMembershipIndex::new([&old, &conflicting])
            .unwrap_err()
            .to_string();
        assert!(error.contains("conflicting families"));
        assert!(error.contains(&hex::encode(reference)));
    }

    #[test]
    fn unregistered_type_id_copy_can_switch_families_without_registering_its_history() {
        use crate::{ScriptFamilyMembershipIndex, ScriptVersionInfo};
        let copy = vec![0xe3; 32];
        let a = ScriptVersionInfo {
            version_hash: vec![0x41; 32],
            family_id: Some("a".into()),
            registered_references: vec![(vec![0x51; 32], 1), (vec![0x52; 32], 1)],
            type_id_deployments: vec![
                (vec![0x51; 32], 10),
                (vec![0x52; 32], 11),
                (copy.clone(), 12),
            ],
            ..Default::default()
        };
        let b = ScriptVersionInfo {
            version_hash: vec![0x42; 32],
            family_id: Some("b".into()),
            registered_references: vec![(vec![0x61; 32], 1)],
            type_id_deployments: vec![(vec![0x61; 32], 20), (copy.clone(), 21)],
            ..Default::default()
        };
        let index = ScriptFamilyMembershipIndex::new([&a, &b]).unwrap();
        assert_eq!(
            index
                .resolve_family(&copy, 1, Some(&a.version_hash))
                .unwrap(),
            Some("a")
        );
        assert_eq!(
            index
                .resolve_family(&copy, 1, Some(&b.version_hash))
                .unwrap(),
            Some("b")
        );
        assert_eq!(index.resolve_family(&copy, 1, None).unwrap(), None);
        assert_eq!(
            index.resolve_family(&[0x51; 32], 1, None).unwrap(),
            Some("a")
        );
        assert_eq!(
            index.resolve_family(&[0x52; 32], 1, None).unwrap(),
            Some("a")
        );
        assert!(index
            .resolve_family(&[0x51; 32], 1, Some(&b.version_hash))
            .is_err());
    }

    #[test]
    fn family_usage_is_shared_checked_and_validates_persisted_counts() {
        use crate::{ScriptFamilyInfo, ScriptFamilyUsage};
        let info = ScriptReferenceInfo {
            reference_hash: vec![0x11; 32],
            hash_type: 1,
            lock_cells_count: 2,
            lock_live_cells_count: 2,
            type_cells_count: 4,
            type_live_cells_count: 4,
            lock_capacity_sum: 200,
            type_capacity_sum: 400,
            lock_used_capacity_sum: 40,
            type_used_capacity_sum: 80,
            lock_owned_capacity_sum: 100,
            type_owned_capacity_sum: 200,
            lock_owned_knowledge_sum: 20,
            type_owned_knowledge_sum: 40,
        };
        let mut usage = ScriptFamilyUsage::default();
        usage.add_reference(&info).unwrap();
        assert_eq!(
            usage,
            ScriptFamilyUsage {
                cells_count: 6,
                live_cells_count: 6,
                lock_cells_count: 2,
                type_cells_count: 4,
                capacity_sum: 600,
                used_capacity_sum: 120,
                owned_capacity_sum: 300,
                owned_knowledge_sum: 60,
            }
        );
        let mut second = info.clone();
        second.reference_hash = vec![0x22; 32];
        second.hash_type = 2;
        usage.add_reference(&second).unwrap();
        assert_eq!(usage.cells_count, 12);
        assert_eq!(usage.owned_capacity_sum, 600);
        let mut family = ScriptFamilyInfo {
            family_id: "family".into(),
            versions_count: 3,
            ..Default::default()
        };
        usage.apply_to(&mut family);
        usage.validate_persisted(&family).unwrap();
        assert_eq!(family.versions_count, 3);
        family.live_cells_count += 1;
        let err = usage.validate_persisted(&family).unwrap_err().to_string();
        assert!(err.contains("family=family"));
        assert!(err.contains("mismatch"));
        let original = usage;
        for invalid in [
            ScriptReferenceInfo {
                lock_cells_count: -1,
                ..info.clone()
            },
            ScriptReferenceInfo {
                lock_capacity_sum: -1,
                ..info.clone()
            },
            ScriptReferenceInfo {
                lock_owned_capacity_sum: -1,
                ..info.clone()
            },
            ScriptReferenceInfo {
                lock_owned_knowledge_sum: 101,
                ..info.clone()
            },
            ScriptReferenceInfo {
                lock_used_capacity_sum: 201,
                ..info.clone()
            },
        ] {
            assert!(usage.add_reference(&invalid).is_err());
            assert_eq!(
                usage, original,
                "invalid input must not partially update totals"
            );
        }
        let mut overflow = ScriptFamilyUsage {
            cells_count: i64::MAX,
            ..Default::default()
        };
        let original = overflow;
        let err = overflow.add_reference(&info).unwrap_err().to_string();
        assert!(err.contains("overflow"));
        assert!(err.contains(&hex::encode(&info.reference_hash)));
        assert_eq!(overflow, original);
    }

    #[test]
    fn test_script_reference_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference_hash = vec![0x33; 32];
        let reference = ScriptReferenceInfo {
            reference_hash: reference_hash.clone(),
            hash_type: 1,
            lock_cells_count: 4,
            lock_live_cells_count: 3,
            lock_capacity_sum: 700,
            lock_owned_capacity_sum: 500,
            lock_used_capacity_sum: 280,
            lock_owned_knowledge_sum: 200,
            type_cells_count: 3,
            type_live_cells_count: 1,
            type_capacity_sum: 400,
            type_owned_capacity_sum: 400,
            type_used_capacity_sum: 220,
            type_owned_knowledge_sum: 160,
        };

        store
            .put_script_reference_info_direct(
                reference.hash_type,
                &reference.reference_hash,
                &reference,
            )
            .unwrap();

        let loaded = store
            .get_script_reference_info(reference.hash_type, &reference.reference_hash)
            .unwrap()
            .expect("reference should exist");
        assert_eq!(loaded, reference);
    }

    #[test]
    fn test_script_reference_to_version_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let hash_type = 1;
        let reference_hash = vec![0x44; 32];
        let version_hash = vec![0x55; 32];

        store
            .put_script_reference_to_version_direct(hash_type, &reference_hash, &version_hash)
            .unwrap();

        let loaded = store
            .get_script_reference_version_hash(hash_type, &reference_hash)
            .unwrap()
            .expect("reference->version mapping should exist");
        assert_eq!(loaded, version_hash);
    }

    #[test]
    fn test_put_script_reference_info_direct_rejects_hash_type_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference = ScriptReferenceInfo {
            reference_hash: vec![0x33; 32],
            hash_type: 2,
            ..Default::default()
        };

        let err = store
            .put_script_reference_info_direct(1, &reference.reference_hash, &reference)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("put_script_reference_info_direct hash_type mismatch"));
    }

    #[test]
    fn test_put_script_reference_info_direct_rejects_reference_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference = ScriptReferenceInfo {
            reference_hash: vec![0x33; 32],
            hash_type: 1,
            ..Default::default()
        };

        let err = store
            .put_script_reference_info_direct(1, &[0x44; 32], &reference)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("put_script_reference_info_direct reference_hash mismatch"));
    }

    #[test]
    fn test_delete_script_reference_info_direct_removes_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference_hash = vec![0x33; 32];
        let reference = ScriptReferenceInfo {
            reference_hash: reference_hash.clone(),
            hash_type: 1,
            ..Default::default()
        };

        store
            .put_script_reference_info_direct(1, &reference_hash, &reference)
            .unwrap();
        store
            .delete_script_reference_info_direct(1, &reference_hash)
            .unwrap();

        let loaded = store.get_script_reference_info(1, &reference_hash).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_delete_script_reference_to_version_direct_removes_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference_hash = vec![0x44; 32];

        store
            .put_script_reference_to_version_direct(1, &reference_hash, &[0x55; 32])
            .unwrap();
        store
            .delete_script_reference_to_version_direct(1, &reference_hash)
            .unwrap();

        let loaded = store
            .get_script_reference_version_hash(1, &reference_hash)
            .unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_put_script_reference_to_version_direct_rejects_non_32_byte_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let err = store
            .put_script_reference_to_version_direct(1, &[0x44; 32], &[0x55; 31])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("put_script_reference_to_version_direct expects 32-byte version_hash"));
    }

    #[test]
    fn test_get_script_reference_version_hash_rejects_malformed_stored_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let key = keys::encode_script_reference_key(1, &[0x44; 32]);
        store
            .put_cf(store.cf_script_reference_to_version(), &key, &[0x55; 31])
            .unwrap();

        let err = store
            .get_script_reference_version_hash(1, &[0x44; 32])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("malformed script_reference_to_version value"));
    }

    #[test]
    fn test_script_reference_roundtrip_fails_on_invalid_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let key = keys::encode_script_reference_key(1, &[0x44; 32]);
        store
            .put_cf(
                store.cf_script_reference_info(),
                &key,
                b"invalid-script-reference",
            )
            .unwrap();

        let err = store.get_script_reference_info(1, &[0x44; 32]).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize script reference info"));
    }

    #[test]
    fn test_list_script_reference_infos_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference_hash = vec![0x66; 32];
        let reference = ScriptReferenceInfo {
            reference_hash: reference_hash.clone(),
            hash_type: 2,
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            lock_capacity_sum: 100,
            lock_owned_capacity_sum: 100,
            lock_used_capacity_sum: 61,
            lock_owned_knowledge_sum: 61,
            type_cells_count: 2,
            type_live_cells_count: 1,
            type_capacity_sum: 200,
            type_owned_capacity_sum: 120,
            type_used_capacity_sum: 142,
            type_owned_knowledge_sum: 71,
        };

        store
            .put_script_reference_info_direct(2, &reference_hash, &reference)
            .unwrap();

        let loaded = store.list_script_reference_infos().unwrap();
        assert_eq!(loaded, vec![((reference_hash, 2), reference)]);
    }
}
