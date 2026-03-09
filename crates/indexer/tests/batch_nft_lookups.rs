//! Integration tests for Spore and Object batch operations via ckbadger-store.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::{ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile};
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

#[test]
fn test_spore_insert_retrieve() {
    let store = setup_store();

    let spore_id = vec![0xAA; 32];
    let entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(vec![0xBB; 32]),
        token_id: None,
        owner_lock_hash: Some(vec![0xCC; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 100,
        created_at_tx: vec![0xDD; 32],
        extra: ObjectExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 4096,
            media_profile: SporeMediaProfile::default(),
        },
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_spore(&spore_id, &entry);
    batch.commit().unwrap();

    let results = store.list_spores(10).unwrap();
    assert_eq!(results.len(), 1);

    let (key, retrieved) = &results[0];
    assert_eq!(key, &spore_id);
    assert_eq!(retrieved.standard, ObjectStandard::Spore);
    assert_eq!(retrieved.collection_id.as_ref().unwrap(), &vec![0xBB; 32]);
    assert_eq!(retrieved.owner_lock_hash.as_ref().unwrap(), &vec![0xCC; 32]);
    assert!(retrieved.is_live);
    assert_eq!(retrieved.created_at_block, 100);
    assert_eq!(retrieved.created_at_tx, vec![0xDD; 32]);
}

#[test]
fn test_spore_consume_burn() {
    let store = setup_store();

    let spore_id = vec![0x11; 32];

    // Insert a live spore
    let live_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(vec![0x22; 32]),
        token_id: None,
        owner_lock_hash: Some(vec![0x33; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 50,
        created_at_tx: vec![0x44; 32],
        extra: ObjectExtra::Spore {
            content_type: "text/plain".to_string(),
            content_length: 256,
            media_profile: SporeMediaProfile::default(),
        },
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_spore(&spore_id, &live_entry);
    batch.commit().unwrap();

    // Verify it is live
    let results = store.list_spores(10).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].1.is_live);

    // "Burn" (consume) the spore by writing the same id with is_live=false
    let burned_entry = ObjectEntry {
        standard: live_entry.standard,
        collection_id: live_entry.collection_id.clone(),
        token_id: None,
        owner_lock_hash: live_entry.owner_lock_hash.clone(),
        name: None,
        description: None,
        is_live: false,
        created_at_block: live_entry.created_at_block,
        created_at_tx: live_entry.created_at_tx.clone(),
        extra: ObjectExtra::Spore {
            content_type: "text/plain".to_string(),
            content_length: 256,
            media_profile: SporeMediaProfile::default(),
        },
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_spore(&spore_id, &burned_entry);
    batch.commit().unwrap();

    // Verify overwrite: same key, now is_live=false
    let results = store.list_spores(10).unwrap();
    assert_eq!(results.len(), 1);
    assert!(!results[0].1.is_live);
    assert_eq!(results[0].1.created_at_block, 50);
}

#[test]
fn test_object_entry_insert_retrieve() {
    let store = setup_store();

    let object_id = vec![0x55; 32];
    let entry = ObjectEntry {
        standard: ObjectStandard::MnftToken,
        collection_id: Some(vec![0x66; 32]),
        token_id: Some(vec![0x00, 0x01]),
        owner_lock_hash: Some(vec![0x77; 32]),
        name: Some("My mNFT".to_string()),
        description: None,
        is_live: true,
        created_at_block: 200,
        created_at_tx: vec![0xDD; 32],
        extra: ObjectExtra::MnftToken {
            token_index: 1,
            characteristic: vec![],
            configure: 0,
            state: 0,
        },
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_object(&object_id, &entry);
    batch.commit().unwrap();

    let results = store.list_objects(10).unwrap();
    assert_eq!(results.len(), 1);

    let (key, retrieved) = &results[0];
    assert_eq!(key, &object_id);
    assert_eq!(retrieved.standard, ObjectStandard::MnftToken);
    assert_eq!(retrieved.collection_id.as_ref().unwrap(), &vec![0x66; 32]);
    assert_eq!(retrieved.token_id.as_ref().unwrap(), &vec![0x00, 0x01]);
    assert_eq!(retrieved.owner_lock_hash.as_ref().unwrap(), &vec![0x77; 32]);
    assert_eq!(retrieved.name.as_ref().unwrap(), "My mNFT");
    assert!(retrieved.is_live);
    assert_eq!(retrieved.created_at_block, 200);
}

#[test]
fn test_list_spores_with_limit() {
    let store = setup_store();

    // Insert 5 spores with distinct IDs
    let mut batch = StoreBatch::new(&store);
    for i in 0u8..5 {
        let mut spore_id = vec![0u8; 32];
        spore_id[0] = i;
        let entry = ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: None,
            token_id: None,
            owner_lock_hash: None,
            name: None,
            description: None,
            is_live: true,
            created_at_block: i as i64,
            created_at_tx: vec![i; 32],
            extra: ObjectExtra::Spore {
                content_type: format!("type_{}", i),
                content_length: i as i64 * 100,
                media_profile: SporeMediaProfile::default(),
            },
        };
        batch.put_spore(&spore_id, &entry);
    }
    batch.commit().unwrap();

    // List with limit 3 — should return exactly 3
    let results = store.list_spores(3).unwrap();
    assert_eq!(results.len(), 3);

    // List with limit 10 — should return all 5
    let results = store.list_spores(10).unwrap();
    assert_eq!(results.len(), 5);
}

#[test]
fn test_list_objects_with_limit() {
    let store = setup_store();

    // Insert 5 objects with distinct IDs
    let mut batch = StoreBatch::new(&store);
    for i in 0u8..5 {
        let mut object_id = vec![0u8; 32];
        object_id[0] = i;
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: None,
            token_id: Some(vec![i]),
            owner_lock_hash: None,
            name: Some(format!("object_{}", i)),
            description: None,
            is_live: true,
            created_at_block: i as i64 * 10,
            created_at_tx: vec![i; 32],
            extra: ObjectExtra::MnftToken {
                token_index: i as u32,
                characteristic: vec![],
                configure: 0,
                state: 0,
            },
        };
        batch.put_object(&object_id, &entry);
    }
    batch.commit().unwrap();

    // List with limit 2 — should return exactly 2
    let results = store.list_objects(2).unwrap();
    assert_eq!(results.len(), 2);

    // List with limit 100 — should return all 5
    let results = store.list_objects(100).unwrap();
    assert_eq!(results.len(), 5);
}
