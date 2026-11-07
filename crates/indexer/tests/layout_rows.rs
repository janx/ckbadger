//! Store refactor lane C (pointer keys): the exact bytes of one row of every
//! domain column family whose key or value layout the lane changes, written
//! through the real live path from the rollback property's protocol chain.
//! A layout change shows up here as a changed expected row, so the diff of a
//! layout commit carries the before → after bytes.

use std::collections::BTreeMap;

use ckbadger_store::keys;

use crate::rollback_property::{open_stores, protocol_chain, single_block_plan, sync_prefix};

/// How a row is chosen in its column family.
enum Pick {
    /// The first row in key order.
    First,
    /// The row at this key.
    Key(Vec<u8>),
}

fn picks() -> Vec<(&'static str, Pick)> {
    vec![
        (ckbadger_store::CF_CONSUMED_CELLS, Pick::First),
        (ckbadger_store::CF_ADDR_TXS, Pick::First),
        (ckbadger_store::CF_ADDR_TXS_BY_PREFIX, Pick::First),
        (ckbadger_store::CF_TX_ACTIONS, Pick::First),
        (ckbadger_store::CF_OBJECT_COLLECTION_ACTIVITIES, Pick::First),
        (
            ckbadger_store::CF_IDENTITY_COLLECTION_ACTIVITIES,
            Pick::First,
        ),
        (
            ckbadger_store::CF_BLOCK_HEADERS,
            Pick::Key(keys::encode_block_num(1).to_vec()),
        ),
        (ckbadger_store::CF_ADDR_BALANCE, Pick::First),
        (
            ckbadger_store::CF_TX_INDEX,
            Pick::Key(keys::encode_tx_slot_key(1, 1)),
        ),
    ]
}

/// `(cf, key hex, value hex)` of every pick, as the store holds them now.
const EXPECTED: &[(&str, &str, &str)] = &[
    (
        "activities",
        "7fffffffffffffef7ffffffe",
        "000000000000000000000000000000000000000000000000010000000000000000000000fbc7a8d1837a80f2eaa9a3ccf371c5e84322df75f15d9f26f5c150090f0cc2b1001f0afaffffffffffffffffffffffff00d197a6ffffffff0000000000000000000000",
    ),
    (
        "addr_balance",
        "1e2d62803000d30926bfba76bd85e87cd422e837a7778e89ddc2d28df59eef3d",
        "00d70500010900000000000000000000009d966b0100000000000000000000000100000002000000000000000200000000000000010000000000000000000000000000002000000000000000c0007a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a0a0000000000000020000000000000000a017a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a",
    ),
    (
        "addr_txs",
        "1e2d62803000d30926bfba76bd85e87cd422e837a7778e89ddc2d28df59eef3d7ffffffffffffff57ffffffe",
        "003793b1e8ffffff019000",
    ),
    (
        "addr_txs_by_prefix",
        "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a7ffffffffffffff77ffffffe",
        "0000000000000000041400",
    ),
    (
        "block_headers",
        "0000000000000001",
        "200000000000000001000000000000005b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b40a9aac9a0010000000000000000000001000000080700002000000000000000000050ad321ea12e00ca5babf286230000e40b540200000000407a10f35a00000200000000000000000000007ea9081a0120000000000000002a6dfcd6c13bbd4f8d224f185994a28c4e0b7429e5183983194766b4c8d3242a0040a9aac9a0010000",
    ),
    (
        "consumed_cells",
        "02017a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a0000",
        "0200000000000000060000000000000001000000",
    ),
    (
        "identity_collection_activities",
        "6269745f63656c6c5f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f7ffffffffffffff27ffffffd",
        "010000000000000002000000",
    ),
    (
        "object_collection_activities",
        "11111111111111111111111111111111111111111111111111111111111111117ffffffffffffff47ffffffe",
        "010000000000000002000000",
    ),
    (
        "tx_index",
        "000000000000000100000001",
        "01017a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a0040a9aac9a00100000100020000e1f50500000000d8010000004000",
    ),
];

#[tokio::test]
async fn one_row_of_every_pointer_layout_cf_has_its_pinned_bytes() {
    let chain = protocol_chain(0);
    let stores = open_stores();
    let plan = single_block_plan(chain.len());
    sync_prefix(&stores, &chain, &plan, 0, chain.len() - 1).await;
    let store = &stores.domain;

    let mut actual = BTreeMap::new();
    for (cf, pick) in picks() {
        let (key, value) = match pick {
            Pick::First => {
                let mut iter = store.iterator_cf(store.cf(cf), rocksdb::IteratorMode::Start);
                let (key, value) = iter
                    .next()
                    .unwrap_or_else(|| panic!("{cf} has no row"))
                    .expect("read row");
                (key.to_vec(), value.to_vec())
            }
            Pick::Key(key) => {
                let value = store
                    .get_cf(store.cf(cf), &key)
                    .expect("read row")
                    .unwrap_or_else(|| panic!("{cf} has no row 0x{}", hex::encode(&key)));
                (key, value)
            }
        };
        actual.insert(cf, (hex::encode(key), hex::encode(value)));
    }

    let expected: BTreeMap<&str, (String, String)> = EXPECTED
        .iter()
        .map(|(cf, key, value)| (*cf, (key.to_string(), value.to_string())))
        .collect();
    if actual != expected {
        let rendered: Vec<String> = actual
            .iter()
            .map(|(cf, (key, value))| {
                format!(
                    "    (\n        \"{cf}\",\n        \"{key}\",\n        \"{value}\",\n    ),"
                )
            })
            .collect();
        panic!(
            "pinned rows differ; the store holds:\n{}",
            rendered.join("\n")
        );
    }
}
