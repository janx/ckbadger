//! Script family operations.

use crate::keys;
use crate::{CkbadgerStore, ScriptFamilyInfo};

impl CkbadgerStore {
    pub fn get_script_family(&self, family_id: &str) -> anyhow::Result<Option<ScriptFamilyInfo>> {
        match self.get_cf(self.cf_script_families(), family_id.as_bytes())? {
            Some(value) => {
                let info = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script family: family_id={}, error={}",
                        family_id,
                        e
                    )
                })?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    pub fn put_script_family_direct(
        &self,
        family_id: &str,
        info: &ScriptFamilyInfo,
    ) -> anyhow::Result<()> {
        if info.family_id != family_id {
            anyhow::bail!(
                "put_script_family_direct family_id mismatch: key={}, value={}",
                family_id,
                info.family_id
            );
        }
        let value = bincode::serialize(info)?;
        self.put_cf(self.cf_script_families(), family_id.as_bytes(), &value)
    }

    pub fn put_script_version_by_family_direct(
        &self,
        family_id: &str,
        version_hash: &[u8],
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_version_by_family_key(family_id, version_hash);
        self.put_cf(self.cf_script_versions_by_family(), &key, &[])
    }

    pub fn delete_script_version_by_family_direct(
        &self,
        family_id: &str,
        version_hash: &[u8],
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_version_by_family_key(family_id, version_hash);
        self.delete_cf(self.cf_script_versions_by_family(), &key)
    }

    pub fn list_script_version_hashes_by_family(
        &self,
        family_id: &str,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        let prefix = keys::encode_script_version_by_family_prefix(family_id);
        let iter = self.prefix_iterator_cf(self.cf_script_versions_by_family(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate script_versions_by_family in prefix scan: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let (_family_id, version_hash) = keys::decode_script_version_by_family_key(&key);
            results.push(version_hash);
        }

        Ok(results)
    }

    pub fn get_script_family_id_by_name(
        &self,
        family_name: &str,
    ) -> anyhow::Result<Option<String>> {
        match self.get_cf(self.cf_script_family_by_name(), family_name.as_bytes())? {
            Some(value) => {
                let family_id = String::from_utf8(value.to_vec()).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to decode script family id by name: family_name={}, error={}",
                        family_name,
                        e
                    )
                })?;
                Ok(Some(family_id))
            }
            None => Ok(None),
        }
    }

    pub fn put_script_family_name_direct(
        &self,
        family_name: &str,
        family_id: &str,
    ) -> anyhow::Result<()> {
        self.put_cf(
            self.cf_script_family_by_name(),
            family_name.as_bytes(),
            family_id.as_bytes(),
        )
    }

    pub fn delete_script_family_name_direct(&self, family_name: &str) -> anyhow::Result<()> {
        self.delete_cf(self.cf_script_family_by_name(), family_name.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use crate::{CkbadgerStore, ScriptFamilyInfo};

    #[test]
    fn test_script_family_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let family = ScriptFamilyInfo {
            family_id: "family/default-lock".to_string(),
            name: "Default Lock".to_string(),
            description: Some("Mainnet lock family".to_string()),
            website: Some("https://nervos.org".to_string()),
            category: Some("lock".to_string()),
            versions_count: 2,
            live_cells_count: 3,
            cells_count: 5,
            owned_capacity_sum: 600,
            owned_knowledge_sum: 420,
        };

        store
            .put_script_family_direct(&family.family_id, &family)
            .unwrap();

        let loaded = store
            .get_script_family(&family.family_id)
            .unwrap()
            .expect("family should exist");
        assert_eq!(loaded, family);
    }

    #[test]
    fn test_script_version_hashes_by_family_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let family_id = "family/default-lock";
        let version_hash_a = vec![0x11; 32];
        let version_hash_b = vec![0x22; 32];

        store
            .put_script_version_by_family_direct(family_id, &version_hash_a)
            .unwrap();
        store
            .put_script_version_by_family_direct(family_id, &version_hash_b)
            .unwrap();

        let loaded = store
            .list_script_version_hashes_by_family(family_id)
            .unwrap();
        assert_eq!(loaded, vec![version_hash_a, version_hash_b]);
    }

    #[test]
    fn test_script_family_name_index_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let family_id = "family/default-lock";
        let family_name = "Default Lock";

        store
            .put_script_family_name_direct(family_name, family_id)
            .unwrap();

        let loaded = store
            .get_script_family_id_by_name(family_name)
            .unwrap()
            .expect("family name index should exist");
        assert_eq!(loaded, family_id);
    }

    #[test]
    fn test_put_script_family_direct_rejects_mismatched_family_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let family = ScriptFamilyInfo {
            family_id: "family/actual".to_string(),
            name: "Default Lock".to_string(),
            ..Default::default()
        };

        let err = store
            .put_script_family_direct("family/requested", &family)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("put_script_family_direct family_id mismatch"));
    }

    #[test]
    fn test_delete_script_version_by_family_direct_removes_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let family_id = "family/default-lock";
        let version_hash = vec![0x11; 32];

        store
            .put_script_version_by_family_direct(family_id, &version_hash)
            .unwrap();
        store
            .delete_script_version_by_family_direct(family_id, &version_hash)
            .unwrap();

        let loaded = store
            .list_script_version_hashes_by_family(family_id)
            .unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_delete_script_family_name_direct_removes_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        store
            .put_script_family_name_direct("Default Lock", "family/default-lock")
            .unwrap();
        store
            .delete_script_family_name_direct("Default Lock")
            .unwrap();

        let loaded = store.get_script_family_id_by_name("Default Lock").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_list_script_families_fails_on_invalid_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        store
            .put_cf(
                store.cf_script_families(),
                b"family/default-lock",
                b"invalid-script-family",
            )
            .unwrap();

        let err = store.get_script_family("family/default-lock").unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize script family"));
    }
}
