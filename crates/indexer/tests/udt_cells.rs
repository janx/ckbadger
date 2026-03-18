//! Integration tests for UDT (User Defined Token) operations in ckbadger-store.
//!
//! Tests token insertion, holder balance management, holder deletion,
//! and listing operations with limits.

use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::materialize_token_state_for_test;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::TokenInfo;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

fn fixture_lock_script(args_hex: &str) -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: args_hex.to_string(),
    }
}

fn fixture_header(number: u64) -> HeaderView {
    HeaderView {
        version: "0x0".to_string(),
        compact_target: "0x1a08a97e".to_string(),
        timestamp: "0x18c7b3b2b00".to_string(),
        number: format!("0x{number:x}"),
        epoch: "0x7080006000028".to_string(),
        parent_hash: format!("0x{}", "11".repeat(32)),
        transactions_root: format!("0x{}", "22".repeat(32)),
        proposals_hash: format!("0x{}", "33".repeat(32)),
        extra_hash: format!("0x{}", "44".repeat(32)),
        dao: format!("0x{}", "00".repeat(32)),
        nonce: "0x1".to_string(),
        hash: format!("0x{}", "55".repeat(32)),
    }
}

fn bulk_build_udt_fixture() -> BlockResponseWithCycles {
    let type_script = Script {
        code_hash: "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5".to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", "12".repeat(32)),
    };

    let tx0 = TransactionView {
        hash: format!("0x{}", "da".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: format!("0x{}", "00".repeat(32)),
                index: "0xffffffff".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
            type_: Some(type_script.clone()),
        }],
        outputs_data: vec![format!("0x{}", hex::encode(1000u128.to_le_bytes()))],
        witnesses: vec!["0x".to_string()],
    };

    let tx1 = TransactionView {
        hash: format!("0x{}", "db".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: tx0.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![
            CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "02".repeat(20))),
                type_: Some(type_script.clone()),
            },
            CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                type_: Some(type_script.clone()),
            },
        ],
        outputs_data: vec![
            format!("0x{}", hex::encode(400u128.to_le_bytes())),
            format!("0x{}", hex::encode(600u128.to_le_bytes())),
        ],
        witnesses: vec!["0x".to_string()],
    };

    let tx2 = TransactionView {
        hash: format!("0x{}", "dc".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: tx1.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
            type_: Some(type_script),
        }],
        outputs_data: vec![format!("0x{}", hex::encode(400u128.to_le_bytes()))],
        witnesses: vec!["0x".to_string()],
    };

    BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(14_000_900),
            uncles: vec![],
            transactions: vec![tx0, tx1, tx2],
            proposals: vec![],
        },
        cycles: None,
    }
}

fn make_token(
    name: &str,
    symbol: &str,
    decimals: i32,
    total_supply: i128,
    holders_count: i64,
    first_seen_block: i64,
) -> TokenInfo {
    TokenInfo {
        type_code_hash: vec![0x44u8; 32],
        hash_type: 1,
        type_args: vec![0x55u8; 20],
        standard: "xUDT".to_string(),
        name: Some(name.to_string()),
        symbol: Some(symbol.to_string()),
        decimals: Some(decimals),
        total_supply: Some(total_supply),
        max_supply: None,
        holders_count,
        first_seen_block,
        icon_url: None,
        description: Some(format!("{} token", name)),
        transfers_count: 0,
    }
}

#[test]
fn test_insert_token_retrieve_by_type_hash() {
    let store = setup_store();
    let type_hash = vec![0xA1u8; 32];
    let token = make_token("TestCoin", "TC", 8, 100_000_000_000_000, 42, 1000);

    let mut batch = StoreBatch::new(&store);
    batch.put_token(&type_hash, &token);
    batch.commit().unwrap();

    let retrieved = store.get_token(&type_hash).unwrap();
    assert!(retrieved.is_some(), "token should exist after insertion");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.name, Some("TestCoin".to_string()));
    assert_eq!(retrieved.symbol, Some("TC".to_string()));
    assert_eq!(retrieved.decimals, Some(8));
    assert_eq!(retrieved.total_supply, Some(100_000_000_000_000));
    assert_eq!(retrieved.holders_count, 42);
    assert_eq!(retrieved.first_seen_block, 1000);
    assert_eq!(retrieved.standard, "xUDT");
    assert_eq!(retrieved.hash_type, 1);
    assert_eq!(retrieved.type_code_hash, vec![0x44u8; 32]);
    assert_eq!(retrieved.type_args, vec![0x55u8; 20]);
    assert_eq!(retrieved.description, Some("TestCoin token".to_string()));

    // Verify listing includes this token
    let all_tokens = store.list_tokens().unwrap();
    assert_eq!(all_tokens.len(), 1);
    assert_eq!(all_tokens[0].0, type_hash);
    assert_eq!(all_tokens[0].1.name, Some("TestCoin".to_string()));

    // Verify non-existent type hash returns None
    let missing = store.get_token(&[0xFFu8; 32]).unwrap();
    assert!(missing.is_none());
}

#[test]
fn test_update_token_holder_balance() {
    let store = setup_store();
    let type_hash = vec![0xB1u8; 32];
    let lock_hash = vec![0xC1u8; 32];

    // Insert token info first
    let token = make_token("HolderCoin", "HC", 6, 500_000_000_000, 1, 2000);
    let mut batch = StoreBatch::new(&store);
    batch.put_token(&type_hash, &token);
    batch.put_token_holder(&type_hash, &lock_hash, 1000_000000);
    batch.commit().unwrap();

    // Verify holder balance
    let balance = store
        .get_token_holder_balance(&type_hash, &lock_hash)
        .unwrap();
    assert_eq!(balance, Some(1000_000000));

    // Update holder balance (simulating receiving more tokens)
    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &lock_hash, 2500_000000);
    batch.commit().unwrap();

    let updated = store
        .get_token_holder_balance(&type_hash, &lock_hash)
        .unwrap();
    assert_eq!(updated, Some(2500_000000));

    // Verify via list_token_holders
    let holders = store.list_token_holders(&type_hash, 100).unwrap();
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].0, lock_hash);
    assert_eq!(holders[0].1, 2500_000000);
}

#[test]
fn test_delete_token_holder_zero_balance() {
    let store = setup_store();
    let type_hash = vec![0xD1u8; 32];
    let lock_hash = vec![0xE1u8; 32];

    // Insert holder with a balance
    let token = make_token("DelCoin", "DC", 8, 100_00000000, 1, 3000);
    let mut batch = StoreBatch::new(&store);
    batch.put_token(&type_hash, &token);
    batch.put_token_holder(&type_hash, &lock_hash, 50_00000000);
    batch.commit().unwrap();

    // Verify it exists
    let before = store
        .get_token_holder_balance(&type_hash, &lock_hash)
        .unwrap();
    assert_eq!(before, Some(50_00000000));

    // Delete the holder (simulating balance going to zero)
    let mut batch = StoreBatch::new(&store);
    batch.delete_token_holder(&type_hash, &lock_hash);
    batch.commit().unwrap();

    // Verify holder is gone
    let after = store
        .get_token_holder_balance(&type_hash, &lock_hash)
        .unwrap();
    assert!(after.is_none(), "holder should be gone after deletion");

    // list_token_holders should return empty
    let holders = store.list_token_holders(&type_hash, 100).unwrap();
    assert!(
        holders.is_empty(),
        "no holders should remain after deletion"
    );
}

#[test]
fn test_list_token_holders_with_limit() {
    let store = setup_store();
    let type_hash = vec![0xF1u8; 32];

    let holder1 = vec![0x01u8; 32];
    let holder2 = vec![0x02u8; 32];
    let holder3 = vec![0x03u8; 32];
    let holder4 = vec![0x04u8; 32];
    let holder5 = vec![0x05u8; 32];

    let token = make_token("BigCoin", "BC", 8, 10000_00000000, 5, 100);

    let mut batch = StoreBatch::new(&store);
    batch.put_token(&type_hash, &token);
    batch.put_token_holder(&type_hash, &holder1, 1000_00000000);
    batch.put_token_holder(&type_hash, &holder2, 2000_00000000);
    batch.put_token_holder(&type_hash, &holder3, 3000_00000000);
    batch.put_token_holder(&type_hash, &holder4, 500_00000000);
    batch.put_token_holder(&type_hash, &holder5, 4500_00000000);
    batch.commit().unwrap();

    // List all holders (limit 100)
    let all = store.list_token_holders(&type_hash, 100).unwrap();
    assert_eq!(all.len(), 5, "should have 5 holders total");

    // List with limit of 3
    let limited = store.list_token_holders(&type_hash, 3).unwrap();
    assert_eq!(limited.len(), 3, "limit should restrict to 3 holders");

    // List with limit of 1
    let single = store.list_token_holders(&type_hash, 1).unwrap();
    assert_eq!(single.len(), 1, "limit of 1 should return exactly 1");

    // Verify all balances are correct by collecting them
    let all_balances: Vec<i128> = all.iter().map(|(_, bal)| *bal).collect();
    assert!(all_balances.contains(&1000_00000000));
    assert!(all_balances.contains(&2000_00000000));
    assert!(all_balances.contains(&3000_00000000));
    assert!(all_balances.contains(&500_00000000));
    assert!(all_balances.contains(&4500_00000000));

    // Verify a different type_hash returns no holders
    let other_type = vec![0x99u8; 32];
    let empty = store.list_token_holders(&other_type, 100).unwrap();
    assert!(
        empty.is_empty(),
        "unrelated type_hash should have no holders"
    );
}

#[test]
fn bulk_build_token_owner_materializes_live_token_and_holder_state_without_db_reads() {
    let snapshot =
        materialize_token_state_for_test(&[bulk_build_udt_fixture()]).expect("token snapshot");

    assert_eq!(snapshot.tokens.len(), 1);
    let (type_hash, token) = snapshot.tokens.iter().next().expect("token info");
    assert_eq!(token.standard, "sudt");
    assert_eq!(
        token.type_code_hash,
        vec![
            0x5e, 0x7a, 0x36, 0xa7, 0x7e, 0x68, 0xee, 0xcc, 0x01, 0x3d, 0xfa, 0x2f, 0xe6, 0xa2,
            0x3f, 0x3b, 0x6c, 0x34, 0x4b, 0x04, 0x00, 0x58, 0x08, 0x69, 0x4a, 0xe6, 0xdd, 0x45,
            0xee, 0xa4, 0xcf, 0xd5,
        ]
    );
    assert_eq!(token.hash_type, 1);
    assert_eq!(token.type_args, vec![0x12; 32]);
    assert_eq!(token.first_seen_block, 14_000_900);
    assert_eq!(token.total_supply, Some(1000));
    assert_eq!(token.holders_count, 1);
    assert_eq!(token.transfers_count, 0);

    let holders = snapshot.token_holders.get(type_hash).expect("holders");
    assert_eq!(holders.len(), 1);
    let holder_balances: Vec<i128> = holders.values().copied().collect();
    assert_eq!(holder_balances, vec![1000]);

    assert_eq!(snapshot.addr_tokens.len(), 1);
    let mut addr_balances = snapshot
        .addr_tokens
        .values()
        .map(|tokens: &std::collections::HashMap<Vec<u8>, i128>| {
            assert_eq!(tokens.len(), 1);
            *tokens.get(type_hash).expect("address token balance")
        })
        .collect::<Vec<_>>();
    addr_balances.sort_unstable();
    assert_eq!(addr_balances, vec![1000]);
}
