//! The one rule of a script's name (lane rd/api-lock-names, the fix round's
//! ruling): **a script is named by the code it runs.**
//!
//! A script is its pair (code hash, hash type). Its name comes from exactly
//! one source, decided by the pair:
//!
//! - a pseudo deployment (Zero Lock, Type ID), which has no code cell, is
//!   named by its own label row (`script_info`, which label import writes for
//!   the pseudo deployments only);
//! - every other pair is named by the family of the code it runs:
//!   [`reference_form_family`], THE membership rule the family usage, the
//!   script list and the most-utilized chart count by, so counting and naming
//!   agree by construction. By `type`, the deployment whose Type ID hash (or
//!   observed binding) it is; by `data` / `data1` / `data2`, the binary whose
//!   data hash it is;
//! - a pair that runs no registered family's code has no name. There is no
//!   read by the code hash alone and no name borrowed across hash kinds: a
//!   Type ID hash by `data` names a cell whose data hashes to a type hash,
//!   which no code does, so it runs nothing and has no name.
//!
//! A named pair that is not the reference the registry registers for its code
//! (another data hash type; the data hash of a version registered by its Type
//! ID) carries its hash type in [`ScriptName::by`], and the page writes it
//! beside the name ("xUDT · by data2").
//!
//! Cost: a [`ScriptNamer`] loads the registry once (every `script_versions`
//! row, every family, the pseudo labels) on its first name, then per distinct
//! pair runs the membership rule (by `type`: the live code cells under that
//! type hash, then the observed binding; by `data*`: the persisted binding)
//! and caches the answer for the rest of its request.

use std::collections::{HashMap, HashSet};

use ckbadger_store::{
    types::{ScriptFamilyInfo, ScriptInfo, ScriptVersionInfo},
    CkbadgerStore, ReferenceCode, ScriptFamilyMembershipIndex,
};

use super::script_resolution::{
    hash_type_to_string, historical_family_member_candidate, is_known_script_name,
    observed_type_id_references_from_versions, reference_form_member_version_with_lineage,
    script_hash_type,
};

/// The family of one reference form, and the binaries it was decided from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceFormFamily<'m> {
    pub family_id: Option<&'m str>,
    /// The binary the form runs now, where it runs exactly one.
    pub current_version_hash: Option<Vec<u8>>,
    /// The binary its family was read from: the current one, or the form's
    /// observed binding where it runs none now (a retired type reference);
    /// none where it runs several.
    pub family_version_hash: Option<Vec<u8>>,
    /// A generic type reference whose live code cells hold several binaries
    /// (the lead's ruling): a family only where all are one family's versions.
    pub runs_several: bool,
}

/// THE family of one reference form (code hash, hash type): the rule the
/// family usage, the script list, the most-utilized chart and every name
/// read. `is_allowed_version` admits a data form's own hash as its binary;
/// `observed_type_id` says the reference is an observed native Type ID.
pub fn reference_form_family<'m>(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    membership: &'m ScriptFamilyMembershipIndex,
    is_allowed_version: &dyn Fn(&[u8]) -> bool,
    observed_type_id: bool,
    reference_hash: &[u8],
    hash_type: u8,
) -> anyhow::Result<ReferenceFormFamily<'m>> {
    let code = reference_form_member_version_with_lineage(
        store,
        cells_store,
        hash_type,
        reference_hash,
        is_allowed_version,
        Some(observed_type_id),
    )?;
    // A form that runs nothing now keeps its chain-observed binding's family
    // (a retired reference: the code it ran).
    let binding = match &code {
        ReferenceCode::None => {
            historical_family_member_candidate(store, hash_type, reference_hash, None)?
        }
        ReferenceCode::One(_) | ReferenceCode::Several(_) => None,
    };
    let family_id =
        membership.family_of_form(reference_hash, hash_type, &code, binding.as_deref())?;
    let current_version_hash = code.current().map(<[u8]>::to_vec);
    Ok(ReferenceFormFamily {
        family_id,
        family_version_hash: current_version_hash.clone().or(binding),
        current_version_hash,
        runs_several: matches!(code, ReferenceCode::Several(_)),
    })
}

/// A script's name, as every site writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptName {
    pub name: String,
    /// The pair's hash type where the pair is not the reference the registry
    /// registers for this code: the same code by another reference, written
    /// beside the name. `None` for a registered reference and a pseudo
    /// deployment.
    pub by: Option<&'static str>,
    /// The family the name is; `None` for a pseudo deployment.
    pub family_id: Option<String>,
    pub deprecated: bool,
    pub description: Option<String>,
    pub category: Option<String>,
}

impl ScriptName {
    /// The hash type to write beside the name, as the wire carries it.
    pub fn by(&self) -> Option<String> {
        self.by.map(str::to_string)
    }
}

/// The registry as the naming rule reads it.
pub struct ScriptRegistry {
    versions: HashMap<Vec<u8>, ScriptVersionInfo>,
    membership: ScriptFamilyMembershipIndex,
    observed_type_ids: HashSet<Vec<u8>>,
    families: HashMap<String, ScriptFamilyInfo>,
    /// Every registered reference form, `(hash type, reference)`.
    registered: HashSet<(u8, Vec<u8>)>,
    /// The pseudo deployments' labels, by pair.
    pseudo: HashMap<(Vec<u8>, u8), ScriptInfo>,
}

impl ScriptRegistry {
    pub fn load(store: &CkbadgerStore) -> anyhow::Result<Self> {
        let versions = store.list_script_versions()?;
        let membership = ScriptFamilyMembershipIndex::new(versions.iter().map(|(_, info)| info))?;
        let observed_type_ids = observed_type_id_references_from_versions(&versions);
        let registered = versions
            .iter()
            .flat_map(|(_, info)| {
                info.registered_references
                    .iter()
                    .map(|(reference, hash_type)| (*hash_type, reference.clone()))
            })
            .collect();
        Ok(Self {
            versions: versions.into_iter().collect(),
            membership,
            observed_type_ids,
            families: store.list_script_families()?.into_iter().collect(),
            registered,
            pseudo: store.list_script_infos()?.into_iter().collect(),
        })
    }

    /// The family membership index the registry was built with.
    pub fn membership(&self) -> &ScriptFamilyMembershipIndex {
        &self.membership
    }

    /// Whether `hash` is a binary of a registered family.
    pub fn is_family_version(&self, hash: &[u8]) -> bool {
        self.versions
            .get(hash)
            .is_some_and(|version| version.family_id.is_some())
    }

    /// Whether the registry registers the pair as a version's reference.
    pub fn is_registered(&self, reference: &[u8], hash_type: u8) -> bool {
        self.registered.contains(&(hash_type, reference.to_vec()))
    }

    /// Whether `reference` is an observed native Type ID.
    pub fn is_observed_type_id(&self, reference: &[u8]) -> bool {
        self.observed_type_ids.contains(reference)
    }

    /// The family of a reference form ([`reference_form_family`]).
    pub fn family<'r>(
        &'r self,
        store: &CkbadgerStore,
        cells_store: &CkbadgerStore,
        reference_hash: &[u8],
        hash_type: u8,
    ) -> anyhow::Result<ReferenceFormFamily<'r>> {
        reference_form_family(
            store,
            cells_store,
            &self.membership,
            &|hash| self.is_family_version(hash),
            self.is_observed_type_id(reference_hash),
            reference_hash,
            hash_type,
        )
    }

    /// THE name of a pair (the module's rule): a pseudo deployment's label,
    /// or the family of the code it runs, or none.
    pub fn name(
        &self,
        store: &CkbadgerStore,
        cells_store: &CkbadgerStore,
        code_hash: &[u8],
        hash_type: u8,
    ) -> anyhow::Result<Option<ScriptName>> {
        let hash_type_label = hash_type_to_string(hash_type).ok_or_else(|| {
            anyhow::anyhow!(
                "script 0x{} has hash type {hash_type}, not a CKB hash type",
                hex::encode(code_hash)
            )
        })?;
        let pseudo = self.pseudo.get(&(code_hash.to_vec(), hash_type));
        let form = self.family(store, cells_store, code_hash, hash_type)?;
        match (pseudo, form.family_id) {
            // Label import writes a label row for a pseudo deployment only,
            // and a pseudo deployment has no code: a pair that is both is a
            // registry fault, named.
            (Some(label), Some(family_id)) => anyhow::bail!(
                "script 0x{} by {hash_type_label} is the pseudo deployment {:?} and runs the \
                 code of family {family_id}",
                hex::encode(code_hash),
                label.name
            ),
            (Some(label), None) => Ok(Some(label.name.trim())
                .filter(|name| is_known_script_name(Some(name)))
                .map(|name| ScriptName {
                    name: name.to_string(),
                    by: None,
                    family_id: None,
                    deprecated: label.deprecated,
                    description: label.description.clone(),
                    category: label.category.clone(),
                })),
            (None, Some(family_id)) => {
                let family = self.families.get(family_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "script 0x{} by {hash_type_label} runs the code of family {family_id}, \
                         which has no family row",
                        hex::encode(code_hash)
                    )
                })?;
                // The version it runs says whether that version is deprecated;
                // a form known only by its family's observed Type ID lineage
                // runs no version now, and takes the family's flag.
                let deprecated = match form
                    .family_version_hash
                    .as_ref()
                    .and_then(|hash| self.versions.get(hash))
                {
                    Some(version) => version.deprecated,
                    None => family.deprecated,
                };
                let registered = self.registered.contains(&(hash_type, code_hash.to_vec()));
                Ok(Some(ScriptName {
                    name: family.name.trim().to_string(),
                    by: (!registered).then_some(hash_type_label),
                    family_id: Some(family_id.to_string()),
                    deprecated,
                    description: family.description.clone(),
                    category: family.category.clone(),
                }))
            }
            (None, None) => Ok(None),
        }
    }
}

/// The names of one request's scripts: the registry loaded once on the first
/// name, each pair named once ([`ScriptRegistry::name`]).
pub struct ScriptNamer<'a> {
    store: &'a CkbadgerStore,
    cells_store: &'a CkbadgerStore,
    registry: Option<ScriptRegistry>,
    names: HashMap<(Vec<u8>, u8), Option<ScriptName>>,
}

impl<'a> ScriptNamer<'a> {
    pub fn new(store: &'a CkbadgerStore, cells_store: &'a CkbadgerStore) -> Self {
        Self {
            store,
            cells_store,
            registry: None,
            names: HashMap::new(),
        }
    }

    /// The name of the pair (`code_hash`, `hash_type`).
    pub fn name(&mut self, code_hash: &[u8], hash_type: u8) -> anyhow::Result<Option<ScriptName>> {
        let key = (code_hash.to_vec(), hash_type);
        if let Some(name) = self.names.get(&key) {
            return Ok(name.clone());
        }
        if self.registry.is_none() {
            self.registry = Some(ScriptRegistry::load(self.store)?);
        }
        let registry = self
            .registry
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the script registry was not loaded"))?;
        let name = registry.name(self.store, self.cells_store, code_hash, hash_type)?;
        self.names.insert(key, name.clone());
        Ok(name)
    }

    /// The name of a pair whose hash type is as stored (`i16`), checked to
    /// be one CKB defines.
    pub fn name_stored(
        &mut self,
        code_hash: &[u8],
        hash_type: i16,
    ) -> anyhow::Result<Option<ScriptName>> {
        let hash_type = script_hash_type(code_hash, hash_type)?;
        self.name(code_hash, hash_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::{ScriptFamilyInfo, ScriptInfo, ScriptVersionInfo};

    /// A family `fam` ("Fam") whose binary `binary` is registered by its Type
    /// ID `type_id` (a binding the indexer records when it sees the code
    /// cell) and by its data hash under `data1`; and the pseudo label
    /// "Zero Lock" at (0x00…00, data).
    fn seeded(type_id: [u8; 32], binary: [u8; 32]) -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store
            .put_script_family_direct(
                "fam",
                &ScriptFamilyInfo {
                    family_id: "fam".into(),
                    name: "Fam".into(),
                    description: Some("the family".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_version(
                &binary,
                &ScriptVersionInfo {
                    version_hash: binary.to_vec(),
                    registered_references: vec![(type_id.to_vec(), 1), (binary.to_vec(), 2)],
                    name: Some("Fam".into()),
                    family_id: Some("fam".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .put_script_version_by_family_direct("fam", &binary)
            .unwrap();
        store
            .put_script_reference_to_version_direct(1, &type_id, &binary)
            .unwrap();
        store
            .put_script_info_direct(
                &[0u8; 32],
                0,
                &ScriptInfo {
                    name: "Zero Lock".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        (dir, store)
    }

    /// The fix round's ruling: a script is named by the code it runs. Its
    /// registered references are named and say nothing more; the same binary
    /// by another data hash type is named and carries its hash type; a Type
    /// ID hash by `data` runs no code and has no name, nor does the binary's
    /// data hash by `type`, nor an unknown pair; a pseudo deployment is named
    /// by its own label row.
    #[test]
    fn a_script_is_named_by_the_code_it_runs() {
        let type_id = [0x9b; 32];
        let binary = [0x70; 32];
        let (_dir, store) = seeded(type_id, binary);
        let mut names = ScriptNamer::new(&store, &store);
        let named = |name: &str, by: Option<&'static str>| ScriptName {
            name: name.to_string(),
            by,
            family_id: Some("fam".to_string()),
            deprecated: false,
            description: Some("the family".to_string()),
            category: None,
        };
        assert_eq!(names.name(&type_id, 1).unwrap(), Some(named("Fam", None)));
        assert_eq!(names.name(&binary, 2).unwrap(), Some(named("Fam", None)));
        assert_eq!(
            names.name(&binary, 0).unwrap(),
            Some(named("Fam", Some("data")))
        );
        assert_eq!(
            names.name(&binary, 4).unwrap(),
            Some(named("Fam", Some("data2")))
        );
        for hash_type in [0u8, 2, 4] {
            assert_eq!(
                names.name(&type_id, hash_type).unwrap(),
                None,
                "a Type ID hash by {hash_type} runs no code"
            );
        }
        assert_eq!(names.name(&binary, 1).unwrap(), None);
        assert_eq!(names.name(&[0x03; 32], 1).unwrap(), None);
        assert_eq!(
            names
                .name(&[0u8; 32], 0)
                .unwrap()
                .map(|name| (name.name, name.by)),
            Some(("Zero Lock".to_string(), None))
        );
        assert_eq!(names.name(&[0u8; 32], 1).unwrap(), None);
        assert!(names.name_stored(&binary, 3).is_err());
    }

    /// One request names each pair once: a second name of the same pair is
    /// not read again.
    #[test]
    fn a_named_pair_is_not_read_again() {
        let (_dir, store) = seeded([0x9b; 32], [0x70; 32]);
        let mut names = ScriptNamer::new(&store, &store);
        names.name(&[0x9b; 32], 1).unwrap();
        store.reset_read_call_counters();
        names.name(&[0x9b; 32], 1).unwrap();
        assert_eq!(store.read_call_counts(), (0, 0), "a named pair is cached");
    }

    /// A label row on a pair that runs a family's code is a registry fault
    /// (label import writes pseudo deployments only), named, never a choice
    /// between two names.
    #[test]
    fn a_label_row_on_a_pair_that_runs_code_is_refused() {
        let (_dir, store) = seeded([0x9b; 32], [0x70; 32]);
        store
            .put_script_info_direct(
                &[0x9b; 32],
                1,
                &ScriptInfo {
                    name: "Default Lock".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let error = ScriptNamer::new(&store, &store)
            .name(&[0x9b; 32], 1)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("is the pseudo deployment") && error.contains("family fam"),
            "{error}"
        );
    }
}
