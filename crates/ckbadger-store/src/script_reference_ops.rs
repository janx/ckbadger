//! Script reference operations.

use crate::bytes_to_hex;
use crate::keys;
use crate::{CkbadgerStore, ScriptReferenceInfo};

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

    #[test]
    fn test_script_reference_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let reference_hash = vec![0x33; 32];
        let reference = ScriptReferenceInfo {
            reference_hash: reference_hash.clone(),
            hash_type: 1,
            live_cells_count: 4,
            cells_count: 7,
            owned_capacity_sum: 900,
            owned_knowledge_sum: 640,
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
}
