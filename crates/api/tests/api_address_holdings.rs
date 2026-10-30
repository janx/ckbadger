use crate::common::*;
use ckb_types::{
    bytes::Bytes,
    core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
    packed,
    prelude::*,
};
use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
use ckbadger_store::types::AddressBalance;

fn script(protocol: ProtocolScript, args: &[u8]) -> packed::Script {
    let hash = PROTOCOL_REGISTRY
        .iter()
        .filter(|(_, p)| *p == protocol)
        .min_by_key(|(hash, _)| *hash)
        .unwrap()
        .0;
    packed::Script::new_builder()
        .code_hash(packed::Byte32::from_slice(hash).unwrap())
        .hash_type(1u8.into())
        .args(Bytes::copy_from_slice(args).pack())
        .build()
}
fn bytes(v: &[u8]) -> Vec<u8> {
    [(v.len() as u32).to_le_bytes().as_slice(), v].concat()
}
fn table(fields: &[Vec<u8>]) -> Vec<u8> {
    let mut offset = 4 * (fields.len() + 1);
    let mut result = ((offset + fields.iter().map(Vec::len).sum::<usize>()) as u32)
        .to_le_bytes()
        .to_vec();
    for field in fields {
        result.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += field.len();
    }
    for field in fields {
        result.extend_from_slice(field);
    }
    result
}

struct Fixture {
    app: axum::Router,
    store: Arc<CkbadgerStore>,
    owner: String,
    hash: Vec<u8>,
    balance: i128,
    occupied: i128,
}

async fn fixture() -> Fixture {
    fixture_with_cluster(true).await
}
async fn fixture_with_cluster(has_cluster: bool) -> Fixture {
    let store = test_store();
    seed_genesis_baseline(&store);
    let lock = packed::Script::new_builder()
        .code_hash([0x11; 32].pack())
        .hash_type(1u8.into())
        .args([0x22; 20].pack())
        .build();
    let owner = lock.calc_script_hash().as_slice().to_vec();
    let token = script(ProtocolScript::Sudt, &[0x30; 32]);
    let cluster = vec![0x40; 32];
    let mnft_class = vec![0x45; 24];
    let mut mnft_id = mnft_class.clone();
    mnft_id.extend_from_slice(&7u32.to_be_bytes());
    let bit_data = [
        0u32.to_le_bytes().as_slice(),
        &table(&[
            vec![0; 20],
            1_900_000_000u64.to_le_bytes().to_vec(),
            bytes(b"holder.bit"),
        ]),
    ]
    .concat();
    let mut dotbit_data = vec![0; 80];
    dotbit_data[32..52].copy_from_slice(&[0x63; 20]);
    dotbit_data[72..80].copy_from_slice(&1_950_000_000u64.to_le_bytes());
    // Four classes, both DAO phases, two token cells of one kind, two object
    // standards, and all non-.cell identity standards. Unknown typed is script.
    let kinds = vec![
        (None, vec![]),
        (
            Some(
                packed::Script::new_builder()
                    .code_hash([0x99; 32].pack())
                    .hash_type(2u8.into())
                    .build(),
            ),
            vec![],
        ),
        (Some(script(ProtocolScript::Dao, &[])), vec![0; 8]),
        (
            Some(script(ProtocolScript::Dao, &[])),
            5u64.to_le_bytes().to_vec(),
        ),
        (Some(token.clone()), 5u128.to_le_bytes().to_vec()),
        (Some(token.clone()), 9u128.to_le_bytes().to_vec()),
        (
            Some(script(ProtocolScript::SporeNft, &[0x41; 32])),
            table(&[
                bytes(b"text/plain"),
                bytes(b"hello"),
                if has_cluster { bytes(&cluster) } else { vec![] },
            ]),
        ),
        (
            Some(script(ProtocolScript::MnftToken, &mnft_id)),
            vec![0; 11],
        ),
        (Some(script(ProtocolScript::DidCkb, &[0x61; 20])), vec![]),
        (Some(script(ProtocolScript::BitCell, &[0x62; 32])), bit_data),
        (
            Some(script(ProtocolScript::DotbitAccount, &[0x63; 20])),
            dotbit_data,
        ),
    ];
    let outputs: Vec<_> = kinds
        .iter()
        .map(|(t, _)| {
            packed::CellOutput::new_builder()
                .capacity(100_000_000_000u64.pack())
                .lock(lock.clone())
                .type_(t.clone().pack())
                .build()
        })
        .collect();
    let tx = TransactionBuilder::default()
        .outputs(outputs.clone())
        .outputs_data(
            kinds
                .iter()
                .map(|(_, d)| Bytes::copy_from_slice(d).pack())
                .collect::<Vec<_>>(),
        )
        .build();
    let hash = tx.hash().as_slice().to_vec();
    let chain = seed_ckb_chain(&[BlockBuilder::default()
        .number(7u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1000).pack())
        .transaction(tx)
        .build()]);
    let mut batch = StoreBatch::new(&store);
    let mut occupied = 0i128;
    for (i, ((t, data), output)) in kinds.iter().zip(&outputs).enumerate() {
        let used =
            (8 + 33 + 20 + t.as_ref().map_or(0, |s| 33 + s.args().raw_data().len()) + data.len())
                as i64
                * 100_000_000;
        occupied += i128::from(used);
        let cell = LiveCellInfo {
            capacity: 100_000_000_000,
            lock_script_hash: owner.clone(),
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            type_script_hash: t.as_ref().map(|s| s.calc_script_hash().as_slice().to_vec()),
            type_code_hash: t.as_ref().map(|s| s.code_hash().as_slice().to_vec()),
            type_hash_type: t.as_ref().map(|s| i16::from(s.hash_type().as_slice()[0])),
            type_args: t.as_ref().map(|s| s.args().raw_data().to_vec()),
            data_size: data.len() as i32,
            occupied_capacity: used,
            udt_amount: matches!(i, 4 | 5)
                .then(|| u128::from_le_bytes(data.as_slice().try_into().unwrap())),
            data_hash: Some(ckb_hash::blake2b_256(data).to_vec()),
        };
        let actual_capacity: u64 = output.capacity().unpack();
        assert_eq!(actual_capacity, cell.capacity as u64);
        batch.put_cell(&hash, i as i16, &cell, 7);
        batch.put_cell_by_lock(&owner, 7, &hash, i as i16);
    }
    let balance = kinds.len() as i128 * 100_000_000_000;
    batch.put_addr_balance(
        &owner,
        &AddressBalance {
            balance,
            used_capacity: occupied,
            live_cells_count: kinds.len() as i32,
            total_cells_count: kinds.len() as i64,
            txs_count: 1,
            first_seen_block: 7,
            first_seen_tx: hash.clone(),
            last_activity_block: 7,
            last_activity_tx: hash.clone(),
        },
    );
    batch.put_token(
        token.calc_script_hash().as_slice(),
        &TokenInfo {
            type_code_hash: token.code_hash().as_slice().to_vec(),
            hash_type: 1,
            type_args: vec![0x30; 32],
            standard: "sudt".into(),
            name: Some("Exact token".into()),
            symbol: Some("EXACT".into()),
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 7,
            icon_url: None,
            description: None,
            transfers_count: 1,
        },
    );
    batch.put_spore_outpoint(&hash, 6, &[0x41; 32]);
    batch.put_mnft_token_outpoint(&hash, 7, &mnft_id);
    batch.put_spore_outpoint(&hash, 8, &[0x61; 20]);
    batch.put_spore_outpoint(&hash, 9, &[0x62; 32]);
    batch.put_dotbit_account_outpoint(&hash, 10, &[0x63; 20]);
    batch.put_dotbit_outpoint_by_account_id(&[0x63; 20], &hash, 10, AssetAction::Mint);
    batch.put_addr_token_by_balance(&owner, token.calc_script_hash().as_slice(), 14);
    batch.put_token_holder(token.calc_script_hash().as_slice(), &owner, 14);
    let deposit = |status| DaoDepositCacheEntry {
        capacity: 100_000_000_000,
        occupied_capacity: 10_200_000_000,
        deposit_block_number: 5,
        deposit_timestamp: 1_700_000_000_000,
        lock_script_hash: owner.clone(),
        deposit_ar: 10_000_000_000_000_000,
        status,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_request_occupied_capacity: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
        deposit_chain_time_ms: 1_700_000_000_000,
    };
    let mut active = deposit(0);
    active.deposit_block_number = 7;
    batch.put_dao_deposit(&ckbadger_store::keys::encode_outpoint(&hash, 2), &active);
    let previous_deposit = ckbadger_store::keys::encode_outpoint(&[0x91; 32], 0);
    let mut withdrawing = deposit(1);
    withdrawing.withdraw_request_tx = Some(hash.clone());
    withdrawing.withdraw_request_output_index = Some(3);
    withdrawing.withdraw_request_block = Some(7);
    withdrawing.withdraw_request_occupied_capacity = Some(10_200_000_000);
    withdrawing.withdraw_request_ar = Some(10_100_000_000_000_000);
    // The withdrawal-request belongs to this address even when the deposit did not.
    withdrawing.lock_script_hash = vec![0x88; 32];
    batch.put_dao_deposit(&previous_deposit, &withdrawing);
    batch.put_dao_by_withdraw_tx(&hash, 3, &previous_deposit);
    let object = |standard, collection_id, extra| ObjectEntry {
        standard,
        collection_id,
        token_id: None,
        owner_lock_hash: Some(owner.clone()),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: vec![0x80; 32],
        extra,
    };
    batch.put_spore(
        &[0x41; 32],
        &object(
            ObjectStandard::Spore,
            Some(if has_cluster {
                cluster.clone()
            } else {
                ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION.to_vec()
            }),
            ObjectExtra::Spore {
                content_type: "text/plain".into(),
                content_length: 5,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::PureCkb,
                    ..Default::default()
                },
            },
        ),
    );
    let mut collection = object(
        ObjectStandard::SporeCluster,
        None,
        ObjectExtra::SporeCluster,
    );
    collection.name = Some("Spore group".into());
    batch.put_spore(&cluster, &collection);
    batch.put_mnft(
        &mnft_id,
        &object(
            ObjectStandard::MnftToken,
            Some(mnft_class.clone()),
            ObjectExtra::MnftToken {
                token_index: 7,
                characteristic: vec![0; 8],
                configure: 0,
                state: 0,
            },
        ),
    );
    let mut class = object(
        ObjectStandard::MnftClass,
        None,
        ObjectExtra::MnftClass {
            description: None,
            renderer: Some("https://example.org/{id}".into()),
            total: 1,
            issued: 1,
            configure: 0,
            composition_tier: CompositionTier::CentralizedMixture,
        },
    );
    class.name = Some("Current mNFT group".into());
    batch.put_mnft(&mnft_class, &class);
    for (id, standard, name, extra) in [
        (
            vec![0x61; 20],
            IdentityStandard::DidCkb,
            None,
            IdentityExtra::DidCkb,
        ),
        (
            vec![0x62; 32],
            IdentityStandard::BitCell,
            Some("holder.bit".into()),
            IdentityExtra::BitCell {
                account_id: vec![0x64; 20],
                expired_at: 1_900_000_000,
            },
        ),
        (
            vec![0x63; 20],
            IdentityStandard::DotBit,
            Some("original.bit".into()),
            IdentityExtra::DotBit {
                expired_at: Some(1_950_000_000),
                registered_at: Some(1),
                status: Some(0),
            },
        ),
    ] {
        batch.put_identity(
            &id,
            &IdentityEntry {
                standard,
                owner_lock_hash: Some(owner.clone()),
                name,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x81; 32],
                extra,
            },
        );
    }
    // Historical owner reverse metadata is deliberately unrelated to the live index.
    batch.put_identity(
        &[0x70; 20],
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(owner.clone()),
            name: None,
            is_live: false,
            created_at_block: 1,
            created_at_tx: vec![0x82; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.commit().unwrap();
    seed_block_headers_at(&store, &[(5, 1_700_000_000_000), (7, 1_700_000_001_000)]);
    let mut header = store.get_block_header(7).unwrap().unwrap();
    header.dao[8..16].copy_from_slice(&10_200_000_000_000_000u64.to_le_bytes());
    let mut tip = StoreBatch::new(&store);
    tip.put_block_header(7, &header);
    tip.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = 7;
            s.tip_block_hash = header.hash.clone();
        })
        .unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store.clone(),
        chain.path,
        Some(chain.cleanup),
    ))
    .await;
    Fixture {
        app,
        store,
        owner: format!("0x{}", hex::encode(owner)),
        hash,
        balance,
        occupied,
    }
}
async fn get(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let r = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| serde_json::json!({"body":String::from_utf8_lossy(&body)})),
    )
}

#[tokio::test]
async fn address_holdings_counts_all_four_classes_and_exact_capacity_partition() {
    let f = fixture().await;
    let before = f
        .store
        .get_cf(
            f.store.cf_cells(),
            &ckbadger_store::keys::encode_outpoint(&f.hash, 0),
        )
        .unwrap();
    let (status, json) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["holdings"],
        serde_json::json!({"dao":2,"token":1,"object":2,"identity":3})
    );
    assert_eq!(json["liveCellsCount"], 11);
    assert_eq!(json["balance"], f.balance.to_string());
    assert_eq!(json["occupiedCapacity"], f.occupied.to_string());
    assert_eq!(json["depositCellsCount"], 1);
    assert_eq!(json["withdrawalRequestCellsCount"], 1);
    let number = |v: &serde_json::Value| v.as_str().unwrap().parse::<i128>().unwrap();
    assert_eq!(
        number(&json["occupiedCapacity"])
            + number(&json["freeCapacity"])
            + number(&json["daoFreeCapacity"]),
        f.balance
    );
    for (class, count) in [
        ("dao", 2),
        ("token", 2),
        ("object", 2),
        ("identity", 3),
        ("plain", 1),
        ("script", 1),
    ] {
        assert_eq!(json["classes"][class]["cellCount"], count);
    }
    assert_eq!(json["objectComposition"]["pureCkb"], 1);
    assert_eq!(json["objectComposition"]["centralizedMixture"], 1);
    assert_eq!(json["objectCollections"].as_array().unwrap().len(), 2);
    assert_eq!(
        f.store
            .get_cf(
                f.store.cf_cells(),
                &ckbadger_store::keys::encode_outpoint(&f.hash, 0)
            )
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn address_cells_filter_before_paging_and_preserve_identity_evidence() {
    let f = fixture().await;
    let path = format!("/api/v1/addresses/{}/cells?class=identity&limit=1", f.owner);
    let (s, one) = get(&f.app, &path).await;
    assert_eq!(s, StatusCode::OK, "{one}");
    assert_eq!(one["data"][0]["display"]["standard"], "did_ckb");
    let cursor = one["nextCursor"].as_str().unwrap();
    let (s, two) = get(&f.app, &format!("{path}&cursor={cursor}")).await;
    assert_eq!(s, StatusCode::OK, "{two}");
    assert_eq!(two["data"][0]["display"]["name"], "holder.bit");
    assert_eq!(two["data"][0]["expiresAt"], 1_900_000_000u64);
    assert_eq!(two["data"][0]["heldSinceBlock"], 7); // current outpoint, not mint at 1
    let (s, three) = get(
        &f.app,
        &format!("{path}&cursor={}", two["nextCursor"].as_str().unwrap()),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{three}");
    assert_eq!(three["data"][0]["display"]["standard"], "dotbit");
    assert!(three["nextCursor"].is_null());
    let (s, wrong) = get(
        &f.app,
        &format!(
            "/api/v1/addresses/{}/cells?class=object&cursor={cursor}",
            f.owner
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{wrong}");
}

#[tokio::test]
async fn address_holdings_rejects_orphan_live_index_and_inconsistent_aggregate() {
    let f = fixture().await;
    let mut batch = StoreBatch::new(&f.store);
    batch.put_cell_by_lock(&hex::decode(&f.owner[2..]).unwrap(), 7, &[0xee; 32], 0);
    batch.commit().unwrap();
    let (s, error) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert!(error.to_string().contains("eeeeeeee"), "{error}");
}

#[tokio::test]
async fn address_holdings_rejects_missing_known_metadata_and_never_reports_zero() {
    let f = fixture().await;
    f.store
        .delete_cf(f.store.cf_identity_data(), &[0x61; 20])
        .unwrap();
    let (s, error) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert!(
        error.to_string().contains("missing indexed identity"),
        "{error}"
    );
}

#[tokio::test]
async fn address_identity_held_since_follows_self_updates_but_not_transfer_or_remint() {
    for (same_owner, linked, expected) in [(true, true, 2), (false, true, 7), (true, false, 7)] {
        let f = fixture().await;
        let current = f.store.get_cell(&f.hash, 8, &f.store).unwrap().unwrap();
        let mut previous = current.cell.clone();
        if !same_owner {
            previous.lock_script_hash = vec![0x88; 32];
        }
        let mut b = StoreBatch::new(&f.store);
        b.put_consumed_cell_with_consumer(
            &[0xa1; 32],
            0,
            &previous,
            2,
            if linked { 7 } else { 4 },
            Some(if linked { &f.hash } else { &[0xa2; 32] }),
        );
        b.put_spore_outpoint(&[0xa1; 32], 0, &[0x61; 20]);
        b.commit().unwrap();
        let (s, j) = get(
            &f.app,
            &format!("/api/v1/addresses/{}/cells?class=identity&limit=1", f.owner),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{j}");
        assert_eq!(j["data"][0]["heldSinceBlock"], expected, "{j}");
    }
}

#[tokio::test]
async fn address_dao_rows_use_current_lock_owned_cells_including_transferred_requests() {
    let f = fixture().await;
    let (s, j) = get(
        &f.app,
        &format!("/api/v1/addresses/{}/cells?class=dao", f.owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert!(j["total"].is_null(), "class-filtered pages omit total: {j}");
    assert_eq!(j["data"][0]["dao"]["status"], "deposited");
    assert_eq!(j["data"][1]["dao"]["status"], "withdrawing");
    assert_eq!(
        j["data"][1]["dao"]["lockScriptHash"],
        format!("0x{}", "88".repeat(32))
    );
    assert_eq!(j["data"][1]["lockScriptHash"], f.owner);
}

#[tokio::test]
async fn address_transaction_cycles_are_nullable_with_explicit_status() {
    let store = test_store();
    let lock = [0x42; 32];
    for (i, cycles) in [None, Some(0), Some(-1), Some(123)].into_iter().enumerate() {
        let hash = [i as u8 + 1; 32];
        let height = i as i64 + 1;
        seed_committed_activity(&store, &lock, &hash, height, 0, 100, 0);
        let mut tx = store.get_tx_index(height, 0).unwrap().unwrap();
        tx.cycles = cycles;
        let mut b = StoreBatch::new(&store);
        b.put_tx_index(height, 0, &tx);
        b.commit().unwrap();
    }
    store
        .update_sync_status(|s| s.tip_block_number = 4)
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (s, j) = get(
        &app,
        &format!("/api/v1/addresses/0x{}/transactions", hex::encode(lock)),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    for row in j["data"].as_array().unwrap() {
        match row["blockNumber"].as_i64().unwrap() {
            4 => {
                assert_eq!(row["cycles"], 123);
                assert_eq!(row["cyclesStatus"], "complete");
            }
            3 => {
                assert!(row["cycles"].is_null());
                assert_eq!(row["cyclesStatus"], "failed");
            }
            _ => {
                assert!(row["cycles"].is_null());
                assert_eq!(row["cyclesStatus"], "pending");
            }
        }
    }
}

#[tokio::test]
async fn zero_amount_token_cells_keep_capacity_but_do_not_create_positive_balance_holdings() {
    let f = fixture().await;
    let mut zero = f
        .store
        .get_cell(&f.hash, 4, &f.store)
        .unwrap()
        .unwrap()
        .cell;
    let token = script(ProtocolScript::Sudt, &[0x31; 32]);
    zero.type_args = Some(vec![0x31; 32]);
    zero.type_script_hash = Some(token.calc_script_hash().as_slice().to_vec());
    zero.udt_amount = Some(0);
    let mut metadata = f
        .store
        .get_token(
            f.store
                .get_cell(&f.hash, 4, &f.store)
                .unwrap()
                .unwrap()
                .type_script_hash
                .as_ref()
                .unwrap(),
        )
        .unwrap()
        .unwrap();
    metadata.type_args = vec![0x31; 32];
    let owner = hex::decode(&f.owner[2..]).unwrap();
    let mut balance = f.store.get_addr_balance(&owner).unwrap().unwrap();
    balance.balance += i128::from(zero.capacity);
    balance.used_capacity += i128::from(zero.occupied_capacity);
    balance.live_cells_count += 1;
    let mut b = StoreBatch::new(&f.store);
    b.put_cell(&[0xc1; 32], 0, &zero, 7);
    b.put_cell_by_lock(&owner, 7, &[0xc1; 32], 0);
    b.put_token(token.calc_script_hash().as_slice(), &metadata);
    b.put_addr_balance(&owner, &balance);
    b.commit().unwrap();
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["classes"]["token"]["cellCount"], 3);
    assert_eq!(
        j["holdings"]["token"], 1,
        "zero token amounts have no positive balance holding"
    );
    let (s, tokens) = get(&f.app, &format!("/api/v1/addresses/{}/tokens", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{tokens}");
    assert_eq!(tokens["data"].as_array().unwrap().len(), 1);
    assert_eq!(tokens["data"][0]["balance"], "14");
}

#[tokio::test]
async fn sparse_class_page_counts_full_owner_set_without_node_reads_for_summary() {
    let f = fixture().await;
    let owner = hex::decode(&f.owner[2..]).unwrap();
    let plain = f
        .store
        .get_cell(&f.hash, 0, &f.store)
        .unwrap()
        .unwrap()
        .cell;
    let mut balance = f.store.get_addr_balance(&owner).unwrap().unwrap();
    let mut b = StoreBatch::new(&f.store);
    for n in 0..2048i16 {
        b.put_cell(&[0xa3; 32], n, &plain, 1);
        b.put_cell_by_lock(&owner, 1, &[0xa3; 32], n);
    }
    balance.balance += i128::from(plain.capacity) * 2048;
    balance.used_capacity += i128::from(plain.occupied_capacity) * 2048;
    balance.live_cells_count += 2048;
    b.put_addr_balance(&owner, &balance);
    b.commit().unwrap();
    // The summary needs only indexed domain/payload evidence, not raw node data.
    let app = create_router(test_config(f.store.clone())).await;
    let start = std::time::Instant::now();
    let (s, j) = get(&app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    eprintln!("D3 2059-cell streaming summary: {:?}", start.elapsed());
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["liveCellsCount"], 2059);
    let (s, j) = get(
        &f.app,
        &format!("/api/v1/addresses/{}/cells?class=identity&limit=1", f.owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert!(
        j["total"].is_null(),
        "class-filtered pages omit total so they need not scan all live cells: {j}"
    );
    assert_eq!(j["data"].as_array().unwrap().len(), 1);
    assert!(j["nextCursor"].is_string());

    // Unfiltered pages use the address balance aggregate for total and still
    // page without a full owner scan for the row set.
    let (s, all) = get(
        &f.app,
        &format!("/api/v1/addresses/{}/cells?limit=1", f.owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{all}");
    assert_eq!(all["total"], 2059);
    assert_eq!(all["data"].as_array().unwrap().len(), 1);
    assert!(all["nextCursor"].is_string());
}

#[tokio::test]
async fn address_holdings_rejects_inconsistent_persisted_balance() {
    let f = fixture().await;
    let owner = hex::decode(&f.owner[2..]).unwrap();
    let mut balance = f.store.get_addr_balance(&owner).unwrap().unwrap();
    balance.balance += 1;
    f.store.put_addr_balance_direct(&owner, &balance).unwrap();
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{j}");
    assert!(j.to_string().contains("aggregate disagrees"), "{j}");
}

#[tokio::test]
async fn address_identity_history_rejects_multiple_predecessors() {
    let f = fixture().await;
    let cell = f
        .store
        .get_cell(&f.hash, 8, &f.store)
        .unwrap()
        .unwrap()
        .cell;
    let mut b = StoreBatch::new(&f.store);
    for hash in [[0xb1; 32], [0xb2; 32]] {
        b.put_consumed_cell_with_consumer(&hash, 0, &cell, 2, 7, Some(&f.hash));
        b.put_spore_outpoint(&hash, 0, &[0x61; 20]);
    }
    b.commit().unwrap();
    let (s, j) = get(
        &f.app,
        &format!("/api/v1/addresses/{}/cells?class=identity&limit=1", f.owner),
    )
    .await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{j}");
    assert!(j.to_string().contains("multiple predecessors"), "{j}");
}

#[tokio::test]
async fn address_named_dotcell_rights_do_not_add_physical_identity_cells_or_capacity() {
    let f = fixture().await;
    let owner = hex::decode(&f.owner[2..]).unwrap();
    let id = ckbadger_store::types::derive_dotcell_id("named");
    let mut b = StoreBatch::new(&f.store);
    b.put_identity(
        &id,
        &IdentityEntry {
            standard: IdentityStandard::DotCell,
            owner_lock_hash: None,
            name: Some("named.cell".into()),
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0xdd; 32],
            extra: IdentityExtra::DotCell {
                label: "named".into(),
                namespace_args: [0; 20],
                layout_version: 1,
                expired_at: 1_950_000_000,
                owner_hash20: owner[..20].try_into().unwrap(),
                manager_hash20: [0; 20],
                next_id: [0; 20],
                records_hash: [0; 32],
                records: vec![],
                parent_id: None,
            },
        },
    );
    b.put_dotcell_name_by_owner(&owner[..20], &id);
    b.commit().unwrap();
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["holdings"]["identity"], 3);
    assert_eq!(j["balance"], f.balance.to_string());
    let (s, names) = get(
        &f.app,
        &format!("/api/v1/addresses/{}/dotcell-names", f.owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{names}");
    assert_eq!(names["data"][0]["name"], "named.cell");
}

#[tokio::test]
async fn address_producer_wire_contract_preserves_required_nullable_fields() {
    let f = fixture().await;
    let (s, cells) = get(&f.app, &format!("/api/v1/addresses/{}/cells", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{cells}");
    for cell in cells["data"].as_array().unwrap() {
        for key in [
            "txHash",
            "outputIndex",
            "capacity",
            "occupiedCapacity",
            "lockScriptHash",
            "typeScriptHash",
            "typeCodeHash",
            "typeHashType",
            "dataSize",
            "createdAtBlock",
            "class",
            "display",
            "expiresAt",
            "heldSinceBlock",
            "dao",
        ] {
            assert!(cell.get(key).is_some(), "missing {key}: {cell}");
        }
        assert!(cell.get("commonKnowledgeSize").is_none());
    }
    assert!(cells["data"][0]["typeScriptHash"].is_null());
    assert!(cells["data"][0]["display"].is_null());
    let (s, holdings) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{holdings}");
    eprintln!("D3_CELLS_JSON={cells}");
    eprintln!("D3_HOLDINGS_JSON={holdings}");
}

#[tokio::test]
async fn address_holdings_does_not_include_completed_original_depositor_history() {
    let f = fixture().await;
    let mut old = f
        .store
        .get_dao_deposit(&ckbadger_store::keys::encode_outpoint(&f.hash, 2))
        .unwrap()
        .unwrap();
    old.status = 2;
    old.withdraw_request_tx = Some(vec![0xf1; 32]);
    old.withdraw_request_output_index = Some(0);
    old.withdraw_request_block = Some(7);
    old.withdraw_request_ar = Some(10_100_000_000_000_000);
    old.withdraw_request_occupied_capacity = Some(old.occupied_capacity);
    old.withdraw_tx = Some(vec![0xf2; 32]);
    old.withdraw_block = Some(8);
    old.withdraw_to_output_index = Some(0);
    old.compensation = Some(898_000_000);
    let mut b = StoreBatch::new(&f.store);
    b.put_dao_deposit(&ckbadger_store::keys::encode_outpoint(&[0xf0; 32], 0), &old);
    b.commit().unwrap();
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["holdings"]["dao"], 2);
    assert_eq!(j["daoUnclaimedCompensation"], "2694000000");
}

#[tokio::test]
async fn address_holdings_rejects_noncanonical_type_hash_byte_without_truncation() {
    let f = fixture().await;
    let owner = hex::decode(&f.owner[2..]).unwrap();
    let mut invalid = f
        .store
        .get_cell(&f.hash, 1, &f.store)
        .unwrap()
        .unwrap()
        .cell;
    invalid.type_hash_type = Some(257);
    let mut b = StoreBatch::new(&f.store);
    b.put_cell(&[0xf3; 32], 0, &invalid, 7);
    b.put_cell_by_lock(&owner, 7, &[0xf3; 32], 0);
    b.commit().unwrap();
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{j}");
    assert!(j.to_string().contains("hash type 257"), "{j}");
}

#[tokio::test]
async fn address_dotcell_ring_root_is_script_infrastructure_not_an_identity() {
    let f = fixture().await;
    let owner = hex::decode(&f.owner[2..]).unwrap();
    let t = script(ProtocolScript::DotCellAccount, &[0x33; 20]);
    let mut root = f
        .store
        .get_cell(&f.hash, 1, &f.store)
        .unwrap()
        .unwrap()
        .cell;
    root.type_script_hash = Some(t.calc_script_hash().as_slice().to_vec());
    root.type_code_hash = Some(t.code_hash().as_slice().to_vec());
    root.type_hash_type = Some(1);
    root.type_args = Some(vec![0x33; 20]);
    root.data_size = 144;
    root.occupied_capacity = (8 + 33 + 20 + 33 + 20 + 144) * 100_000_000;
    let mut balance = f.store.get_addr_balance(&owner).unwrap().unwrap();
    balance.balance += i128::from(root.capacity);
    balance.used_capacity += i128::from(root.occupied_capacity);
    balance.live_cells_count += 1;
    let mut b = StoreBatch::new(&f.store);
    b.put_cell(&[0xf4; 32], 0, &root, 7);
    b.put_cell_by_lock(&owner, 7, &[0xf4; 32], 0);
    b.put_spore_outpoint(
        &[0xf4; 32],
        0,
        &ckbadger_store::types::derive_dotcell_id(""),
    );
    b.put_dotcell_ring(
        &[0x33; 20],
        &ckbadger_store::types::DotCellRingRoot {
            root_tx_hash: vec![0xf4; 32],
            root_output_index: 0,
            first_id: [0; 20],
            created_at_block: 7,
        },
    );
    b.put_addr_balance(&owner, &balance);
    b.commit().unwrap();
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["classes"]["script"]["cellCount"], 2);
    assert_eq!(j["holdings"]["identity"], 3);
}

#[tokio::test]
async fn address_clusterless_spore_uses_no_fabricated_onchain_collection() {
    let f = fixture_with_cluster(false).await;
    let (s, j) = get(&f.app, &format!("/api/v1/addresses/{}/holdings", f.owner)).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let group = j["objectCollections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["standard"] == "spore")
        .unwrap();
    assert!(group["id"].is_null());
    assert!(group["name"].is_null());
    assert_eq!(group["itemCount"], 1);
    let (s, j) = get(
        &f.app,
        &format!("/api/v1/addresses/{}/cells?class=object&limit=1", f.owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert!(j["data"][0]["display"]["collectionId"].is_null());
}
