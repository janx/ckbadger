//! B-0 · the rollback property (store refactor lane B).
//!
//! Write blocks through the real live path (`parse_blocks_parallel` → the
//! parser's `precompute_live_batch` → `Indexer::write_parsed_batch`), roll
//! back through the live reorg entry point (`BatchWriter::execute_reorg`), and
//! the domain store must be byte-equal, CF by CF and key by key, to a store
//! that synced only up to the fork point with the same batch boundaries.
//!
//! This is the single regression net for every rollback mechanism: it does
//! not know which mechanism restores a row, only that every row is restored.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView, UncleBlockView,
};
use ckbadger_indexer::sync::live_test_support::{
    indexer_with_stores, reorg_to, restart, startup_label_import, write_live_batch,
};
use ckbadger_indexer::sync::Indexer;
use ckbadger_store::derive_ops::{DeriveMeta, DeriveState};
use ckbadger_store::keys::{self, sync_meta_keys};
use ckbadger_store::{CkbadgerStore, DOMAIN_CFS};

// ---------------------------------------------------------------------------
// The dump
// ---------------------------------------------------------------------------

/// `sync_meta` keys that are not chain state and are skipped by the dump.
/// Each is either process runtime telemetry or the record of the rollback
/// itself, so it legitimately differs between a rolled-back store and one that
/// never saw the orphaned blocks.
const VOLATILE_SYNC_META: &[(&[u8], &str)] = &[
    (
        sync_meta_keys::RUNTIME_STATUS,
        "process runtime telemetry (phase, pid, timestamps), rewritten by the running indexer",
    ),
    (
        sync_meta_keys::SYNC_RUNTIME,
        "the sync status's runtime part (run start, last sync time, rate, deep-fork state), \
         written outside the journal",
    ),
    (
        sync_meta_keys::MEMORY_STATS,
        "RocksDB / process memory samples, wall-clock dependent",
    ),
    (
        sync_meta_keys::SYNC_PROGRESS,
        "throughput telemetry for the TUI, wall-clock dependent",
    ),
    (
        sync_meta_keys::BACKGROUND_TASKS,
        "progress mirror of background tasks for the TUI / pipeline page",
    ),
    (
        sync_meta_keys::REORG_RECONCILE_PENDING,
        "names the reorg:* event this rollback recorded, awaiting replay; part of the event record",
    ),
];

/// Prefix of the reorg event history (`reorg:<ms>:<uuid>`): the record of the
/// rollback itself, never chain state.
const REORG_EVENT_PREFIX: &[u8] = b"reorg:";

/// A derivation's start / finish wall-clock times are telemetry; its state,
/// version and run counter are compared.
fn normalize_derive_meta(value: &[u8]) -> Vec<u8> {
    let mut meta: DeriveMeta = bincode::deserialize(value).expect("decode derive meta");
    meta.started_at_ms = None;
    meta.finished_at_ms = None;
    bincode::serialize(&meta).expect("encode derive meta")
}

/// The dump's normalisation of one `sync_meta` value (wall-clock fields),
/// applied alike to rows read from a store and to values the journal oracle
/// writes back.
fn normalize_sync_meta(key: &[u8], value: &[u8]) -> Vec<u8> {
    if key.starts_with(sync_meta_keys::DERIVE_META_PREFIX) {
        normalize_derive_meta(value)
    } else {
        value.to_vec()
    }
}

/// A journal entry of a `sync_meta` key carries the key's previous value,
/// which holds the same wall-clock fields as the row itself (the previous
/// batch's `sync_status.last_synced_at`, ...): normalise it the same way.
/// Every undo-log row is a journal row (the store's journal value codec).
fn normalize_undo_entry(key: &[u8], value: &[u8]) -> Vec<u8> {
    let (block, seq) = ckbadger_store::keys::decode_reorg_undo_log_key(key);
    assert!(
        ckbadger_store::keys::UndoSeqScope::Journal.owns(seq),
        "undo-log row at block {block} seq {seq:#x} is not a journal row"
    );
    let entry = ckbadger_store::decode_journal_value(value).expect("decode a journal row");
    match entry.previous {
        Some(previous) if entry.cf == ckbadger_store::CF_SYNC_META => {
            ckbadger_store::encode_journal_value(
                entry.cf,
                entry.key,
                Some(&normalize_sync_meta(entry.key, previous)),
            )
            .expect("encode a journal row")
        }
        _ => value.to_vec(),
    }
}

type Dump = BTreeMap<(&'static str, Vec<u8>), Vec<u8>>;

/// Every row of every domain CF, minus the volatile `sync_meta` keys.
///
/// `reorg_undo_log_by_block` is compared per block as a sorted set of
/// entries, keyed `block ‖ rank`: the sequence number a batch assigns to
/// entries of independent keys follows hash-map iteration order, so it is not
/// stable between two stores; every entry and its block are.
fn dump_domain(store: &CkbadgerStore) -> Dump {
    let mut dump = Dump::new();
    let mut undo: BTreeMap<Vec<u8>, Vec<Vec<u8>>> = BTreeMap::new();
    for &cf in DOMAIN_CFS {
        for row in store.iterator_cf(store.cf(cf), rocksdb::IteratorMode::Start) {
            let (key, value) = row.expect("iterate domain CF");
            let mut value = value.to_vec();
            if cf == ckbadger_store::CF_REORG_UNDO_LOG_BY_BLOCK {
                undo.entry(key[..8].to_vec())
                    .or_default()
                    .push(normalize_undo_entry(&key, &value));
                continue;
            }
            if cf == ckbadger_store::CF_SYNC_META {
                if key.starts_with(REORG_EVENT_PREFIX)
                    || VOLATILE_SYNC_META
                        .iter()
                        .any(|(volatile, _)| key.as_ref() == *volatile)
                {
                    continue;
                }
                value = normalize_sync_meta(&key, &value);
            }
            dump.insert((cf, key.to_vec()), value);
        }
    }
    for (block, mut entries) in undo {
        entries.sort();
        for (rank, entry) in entries.into_iter().enumerate() {
            let mut key = block.clone();
            key.extend_from_slice(&(rank as u64).to_be_bytes());
            dump.insert((ckbadger_store::CF_REORG_UNDO_LOG_BY_BLOCK, key), entry);
        }
    }
    dump
}

fn render_key(cf: &str, key: &[u8]) -> String {
    if cf == ckbadger_store::CF_SYNC_META && key.iter().all(|b| b.is_ascii_graphic()) {
        format!("{cf}:{}", String::from_utf8_lossy(key))
    } else {
        format!("{cf}:0x{}", hex::encode(key))
    }
}

/// Compare two dumps, listing every differing row (CF, key, both values)
/// rather than the first.
fn compare_dumps(actual: &Dump, expected: &Dump, context: &str) -> Result<(), String> {
    let mut diffs = Vec::new();
    for ((cf, key), value) in actual {
        match expected.get(&(*cf, key.clone())) {
            None => diffs.push(format!(
                "  extra    {} = 0x{}",
                render_key(cf, key),
                hex::encode(value)
            )),
            Some(other) if other != value => diffs.push(format!(
                "  changed  {}\n           actual   0x{}\n           expected 0x{}",
                render_key(cf, key),
                hex::encode(value),
                hex::encode(other)
            )),
            Some(_) => {}
        }
    }
    for ((cf, key), value) in expected {
        if !actual.contains_key(&(*cf, key.clone())) {
            diffs.push(format!(
                "  missing  {} = 0x{}",
                render_key(cf, key),
                hex::encode(value)
            ));
        }
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{context}: {} row(s) differ from the expected store:\n{}",
            diffs.len(),
            diffs.join("\n")
        ))
    }
}

// ---------------------------------------------------------------------------
// Stores and runs
// ---------------------------------------------------------------------------

struct Stores {
    domain: Arc<CkbadgerStore>,
    _append: Arc<CkbadgerStore>,
    indexer: Indexer,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

/// A domain + append-only store pair with the genesis constants an indexer
/// persists at startup, and no indexer: a fixture store, written directly.
struct FixtureStores {
    domain: Arc<CkbadgerStore>,
    _append: Arc<CkbadgerStore>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

fn open_fixture_stores() -> FixtureStores {
    let domain_dir = tempfile::tempdir().unwrap();
    let append_dir = tempfile::tempdir().unwrap();
    let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
    let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
    domain
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance: GENESIS_C as i128,
            burnt: 0,
            virtual_occupied: 0,
        })
        .unwrap();
    domain
        .set_secondary_epoch_reward(SECONDARY_EPOCH_REWARD)
        .unwrap();
    FixtureStores {
        domain,
        _append: append,
        _dirs: (domain_dir, append_dir),
    }
}

/// The stores the property runs on, driven by an indexer (under the journal
/// discipline, as in production).
fn open_stores() -> Stores {
    let FixtureStores {
        domain,
        _append: append,
        _dirs,
    } = open_fixture_stores();
    // As in production (entry.rs): from the indexer's first write on, an
    // unjournaled commit may write a Journal-class key only where
    // OUTSIDE_JOURNAL covers it, so every path this suite drives is checked.
    domain.enforce_journal_discipline();
    let indexer = indexer_with_stores(domain.clone(), append.clone(), NETWORK, -1, None);
    Stores {
        domain,
        _append: append,
        indexer,
        _dirs,
    }
}

/// Write `chain[..=upto]` as live batches following `plan` (inclusive block
/// ranges in order, covering the chain from 0); a batch that crosses `upto` is
/// truncated there, exactly like a store whose tip stopped at `upto`.
async fn sync_prefix(
    stores: &Stores,
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    from: usize,
    upto: usize,
) {
    for range in plan {
        let start = (*range.start()).max(from);
        let end = (*range.end()).min(upto);
        if start > end {
            continue;
        }
        write_live_batch(&stores.indexer, chain[start..=end].to_vec())
            .await
            .unwrap_or_else(|e| panic!("live batch {start}-{end}: {e:#}"));
    }
}

/// The property, in both forms:
/// - rolled back from `tip` to `fork` equals a store that synced only to
///   `fork` with the same batch boundaries;
/// - replaying `fork+1..=tip` on the rolled-back store equals the store
///   before the rollback (the rolled-back store is a correct base to sync on).
async fn rollback_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
) -> Result<(), String> {
    rollback_property_with_derive(chain, plan, tip, fork, None).await
}

/// When the background derivation of the three derived aggregates runs,
/// relative to the chain.
#[derive(Clone, Copy)]
enum Derive {
    /// Derived to completion right after block `at` committed.
    CompleteAt(usize),
    /// The first aggregate's rebuild started (one chunk) after block `at`.
    RunningAt(usize),
}

fn run_derive(stores: &Stores, derive: Derive) {
    match derive {
        Derive::CompleteAt(_) => {
            let states = ckbadger_indexer::derive::derive_to_completion(&stores.domain)
                .expect("derive to completion");
            assert!(
                states
                    .iter()
                    .all(|(_, state)| matches!(state, DeriveState::Complete { .. })),
                "{states:?}"
            );
        }
        Derive::RunningAt(_) => {
            let mut scheduler =
                ckbadger_indexer::derive::DeriveScheduler::new(&stores.domain).with_chunk_rows(1);
            scheduler.normalize_on_startup().expect("normalize derive");
            scheduler.step().expect("start the first derivation");
            let running = ckbadger_indexer::derive::registry()
                .iter()
                .filter_map(|aggregate| {
                    stores
                        .domain
                        .get_derive_meta(aggregate.id().name)
                        .unwrap()
                        .filter(|meta| matches!(meta.state, DeriveState::Running { .. }))
                })
                .count();
            assert_eq!(running, 1, "exactly the first aggregate is Running");
        }
    }
}

/// Sync `from..=upto`, running `derive` once the chain reaches its block.
async fn sync_with_derive(
    stores: &Stores,
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    upto: usize,
    derive: Option<Derive>,
) {
    match derive {
        Some(Derive::CompleteAt(at) | Derive::RunningAt(at)) if at <= upto => {
            sync_prefix(stores, chain, plan, 0, at).await;
            run_derive(stores, derive.unwrap());
            sync_prefix(stores, chain, plan, at + 1, upto).await;
        }
        _ => sync_prefix(stores, chain, plan, 0, upto).await,
    }
}

/// The derived-aggregate rule: a rollback resets a `Running` derivation, and
/// a `Complete` one whose `completed_at_tip` is above the target. A reset
/// aggregate is `NotStarted` with an empty CF; the reference store never
/// derived, so those rows and metas are checked here and left out of the
/// comparison.
fn aggregates_reset(derive: Option<Derive>, fork: usize) -> bool {
    match derive {
        Some(Derive::RunningAt(at)) => fork >= at,
        Some(Derive::CompleteAt(at)) => fork < at,
        None => false,
    }
}

fn assert_aggregates_reset(store: &CkbadgerStore) -> Result<(), String> {
    for aggregate in ckbadger_indexer::derive::registry() {
        let id = aggregate.id();
        let meta = store.get_derive_meta(id.name).unwrap();
        if !matches!(
            meta.as_ref().map(|m| &m.state),
            None | Some(DeriveState::NotStarted)
        ) {
            return Err(format!(
                "{} is not reset after the rollback: {meta:?}",
                id.name
            ));
        }
        if store
            .iterator_cf(store.cf(id.cf_name), rocksdb::IteratorMode::Start)
            .next()
            .is_some()
        {
            return Err(format!("{} rows survive the reset", id.cf_name));
        }
    }
    Ok(())
}

fn without_aggregates(mut dump: Dump) -> Dump {
    let cfs: Vec<&str> = ckbadger_indexer::derive::registry()
        .iter()
        .map(|aggregate| aggregate.id().cf_name)
        .collect();
    dump.retain(|(cf, key), _| {
        !cfs.contains(cf)
            && !(*cf == ckbadger_store::CF_SYNC_META
                && key.starts_with(sync_meta_keys::DERIVE_META_PREFIX))
    });
    dump
}

async fn rollback_property_with_derive(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
    derive: Option<Derive>,
) -> Result<(), String> {
    let rolled = open_stores();
    sync_with_derive(&rolled, chain, plan, tip, derive).await;
    let at_tip = dump_domain(&rolled.domain);
    reorg_to(&rolled.indexer, fork as i64)
        .await
        .map_err(|e| format!("rollback {tip} -> {fork}: {e:#}"))?;

    let reference = open_stores();
    sync_with_derive(&reference, chain, plan, fork, derive).await;
    if aggregates_reset(derive, fork) {
        assert_aggregates_reset(&rolled.domain)?;
        compare_dumps(
            &without_aggregates(dump_domain(&rolled.domain)),
            &without_aggregates(dump_domain(&reference.domain)),
            &format!("rollback {tip} -> {fork} (aggregates reset)"),
        )?;
        // A reset aggregate is derived again by the scheduler; the replay
        // check below needs the store as it was, so it is skipped.
        return Ok(());
    }
    compare_dumps(
        &dump_domain(&rolled.domain),
        &dump_domain(&reference.domain),
        &format!("rollback {tip} -> {fork}"),
    )?;

    sync_prefix(&rolled, chain, plan, fork + 1, tip).await;
    compare_dumps(
        &dump_domain(&rolled.domain),
        &at_tip,
        &format!("replay {} ..= {tip} after rollback to {fork}", fork + 1),
    )
}

async fn assert_rollback_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
) {
    assert_rollback_properties(chain, plan, tip, &[fork]).await;
}

/// Every fork of `forks`, reporting all failing forks together.
async fn assert_rollback_properties(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    forks: &[usize],
) {
    let mut failures = Vec::new();
    for &fork in forks {
        if let Err(report) = rollback_property(chain, plan, tip, fork).await {
            failures.push(report);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// The journal oracle (store refactor lane B, B-2 … B-4)
// ---------------------------------------------------------------------------

/// Rows whose every live write is attributed to its block, so the journal
/// alone restores them: a whole column family, or one key prefix of a `stats_*`
/// CF. Grows writer by writer through B-3; at B-5 it is every domain CF and
/// the production rollback is the journal.
const JOURNAL_COVERED: &[(&str, Option<u8>)] = &[
    // RangeByBlock: the key starts with the block number.
    (ckbadger_store::CF_BLOCK_HEADERS, None),
    (ckbadger_store::CF_TX_INDEX, None),
    (ckbadger_store::CF_TX_ACTIONS, None),
    (ckbadger_store::CF_REORG_UNDO_LOG_BY_BLOCK, None),
    // B-2: the eight entity stats families, through the stats side of the
    // block overlay.
    (
        ckbadger_store::CF_STATS_SCRIPT,
        Some(keys::STATS_PREFIX_SCRIPT_DAILY),
    ),
    (
        ckbadger_store::CF_STATS_TOKEN,
        Some(keys::STATS_PREFIX_TOKEN_DAILY),
    ),
    (
        ckbadger_store::CF_STATS_TOKEN,
        Some(keys::STATS_PREFIX_TOKEN_HOURLY),
    ),
    (
        ckbadger_store::CF_STATS_SPORE,
        Some(keys::STATS_PREFIX_CLUSTER_DAILY),
    ),
    (
        ckbadger_store::CF_STATS_SPORE,
        Some(keys::STATS_PREFIX_SPORE_DAILY),
    ),
    (
        ckbadger_store::CF_STATS_SPORE,
        Some(keys::STATS_PREFIX_SPORE_HOURLY),
    ),
    (
        ckbadger_store::CF_STATS_MNFT,
        Some(keys::STATS_PREFIX_OBJECT_DAILY),
    ),
    (
        ckbadger_store::CF_STATS_MNFT,
        Some(keys::STATS_PREFIX_OBJECT_HOURLY),
    ),
    // B-3 spore.rs: the object loop's protocol writes are scoped per block,
    // cluster capacity is folded per block, type-index rows carry their block.
    (ckbadger_store::CF_SPORE_DATA, None),
    (ckbadger_store::CF_SPORE_BY_CLUSTER, None),
    (ckbadger_store::CF_CLUSTER_AGG, None),
    (ckbadger_store::CF_STATS_SPORE, None),
    (ckbadger_store::CF_IDENTITY_DATA, None),
    (ckbadger_store::store::CF_IDENTITY_BY_COLLECTION, None),
    (ckbadger_store::store::CF_STATS_IDENTITY, None),
    // B-3 addresses.rs: address balances, script usage and script reference
    // usage are applied block by block.
    (ckbadger_store::CF_ADDR_BALANCE, None),
    (ckbadger_store::store::CF_SCRIPT_INFO, None),
    (ckbadger_store::store::CF_SCRIPT_REFERENCE_INFO, None),
    // B-3 addresses.rs: the script version / family rollups are recomputed
    // and written per block that moves a reference or a Type ID binding.
    (ckbadger_store::store::CF_SCRIPT_VERSIONS, None),
    (ckbadger_store::store::CF_SCRIPT_FAMILIES, None),
    (ckbadger_store::store::CF_SCRIPT_VERSIONS_BY_FAMILY, None),
    (ckbadger_store::store::CF_SCRIPT_VERSIONS_BY_LABEL, None),
    (ckbadger_store::store::CF_SCRIPT_REFERENCE_TO_VERSION, None),
    (ckbadger_store::store::CF_SCRIPT_FAMILY_BY_NAME, None),
    // B-3 statistics.rs: the chain statistics (daily, hourly, daily block,
    // miner, epoch, block / epoch time histograms) and the activity buckets
    // (with their live per-address rows) are written per block.
    (ckbadger_store::CF_STATS_CHAIN, None),
    // B-3 sync/batch.rs: the activity pass is scoped per block (addr_txs and
    // addr_txs_by_prefix rows, TxActions bucket index rows), and the
    // per-prefix participation counters and the collection activity day
    // counters are staged block by block.
    (ckbadger_store::CF_ADDR_TXS, None),
    (ckbadger_store::CF_ADDR_TXS_BY_PREFIX, None),
    (ckbadger_store::CF_TX_ACTIONS_BY_BUCKET, None),
    (ckbadger_store::CF_ADDR_PREFIX_STATS, None),
    (ckbadger_store::CF_COLLECTION_ACTIVITY_DAILY, None),
    // The Fiber channel lifecycle (`process_fiber_channel_events`, its only
    // live writer) runs inside the same per-block activity pass, so it is
    // scoped with it.
    (ckbadger_store::CF_FIBER_CHANNELS, None),
    (ckbadger_store::CF_ADDR_FIBER_CHANNELS, None),
    (ckbadger_store::CF_FIBER_CHANNEL_BY_COMMITMENT, None),
    // B-3 chain.rs: each block header's `block_hash_index` row and each
    // transaction's `tx_hash_map` row are written in their block's scope.
    (ckbadger_store::CF_BLOCK_HASH_INDEX, None),
    (ckbadger_store::CF_TX_HASH_MAP, None),
    // B-3 cells.rs: a created cell's live marker, lock script and index rows
    // in its creating block's scope; a consumed cell's consumed marker,
    // live-marker delete and index-row deletes in its consuming block's.
    (ckbadger_store::CF_LIVE_CELLS, None),
    (ckbadger_store::CF_CONSUMED_CELLS, None),
    (ckbadger_store::CF_LOCK_SCRIPTS, None),
    (ckbadger_store::CF_CELL_BY_LOCK, None),
    (ckbadger_store::CF_CELL_BY_LOCK_CODE, None),
    (ckbadger_store::CF_CELL_BY_TYPE, None),
    (ckbadger_store::CF_CELL_BY_TYPE_CODE, None),
    (ckbadger_store::CF_CELL_BY_DATA_HASH, None),
    // B-3 udt.rs: a token row and its `stats_token` transfer counter are
    // written at every block that moves them, holder rows and both
    // balance-ordered indexes block by block on top of the previous block's
    // balance, transfer records in their block's scope. `stats_token` holds
    // only the transfer counter and the two entity stats families.
    (ckbadger_store::CF_TOKENS, None),
    (ckbadger_store::CF_TOKEN_HOLDERS, None),
    (ckbadger_store::CF_TOKEN_HOLDERS_BY_BALANCE, None),
    (ckbadger_store::CF_ADDR_TOKENS_BY_BALANCE, None),
    (ckbadger_store::store::CF_TOKEN_TRANSFERS, None),
    (ckbadger_store::CF_STATS_TOKEN, None),
    // B-3 mnft.rs: the object type-index rows (`stats_mnft` 0x16) carry the
    // block whose output carried them; the mNFT entity rows were scoped with
    // the object loop (96802ea4). `stats_mnft` holds the object entity stats,
    // the type index, the mNFT / `.bit` outpoint indexes and the collection
    // owner counts, all written per block now.
    (ckbadger_store::CF_MNFT_DATA, None),
    (ckbadger_store::CF_MNFT_BY_COLLECTION, None),
    (ckbadger_store::CF_STATS_MNFT, None),
    // B-3 dotcell.rs: every `.cell` row the object loop writes (owner index,
    // ring roots; the names themselves are identity rows) is in its block's
    // scope since 96802ea4; the `.cell` collection aggregate's activity
    // count is the object_activity_acc.rs step.
    (ckbadger_store::CF_DOTCELL_NAME_BY_OWNER, None),
    (ckbadger_store::CF_DOTCELL_RING, None),
    // B-3 dao.rs: every DAO lifecycle transition (deposit, withdraw request,
    // completion) stages the deposit row and its three secondary index rows
    // in its own block, in chain order, and a request's dao_by_withdraw_tx
    // row at the request's block.
    (ckbadger_store::CF_DAO_DEPOSITS, None),
    (ckbadger_store::CF_DAO_BY_WITHDRAW_TX, None),
    (ckbadger_store::CF_DAO_BY_BLOCK, None),
    (ckbadger_store::CF_DAO_BY_LOCK_BLOCK, None),
    (ckbadger_store::CF_DAO_BY_STATUS_BLOCK, None),
    // B-3 derive/live.rs: the derived aggregates' live rows are written at
    // every block that moves them (exercised by the derived-aggregates case;
    // the other cases never start a derivation, so these CFs stay empty).
    (ckbadger_store::CF_DAO_LIVE_AGG, None),
    (ckbadger_store::CF_OBJECT_COLLECTION_ACTIVITY_COUNT, None),
    (ckbadger_store::CF_BIT_CELL_BY_ACCOUNT, None),
    // B-3 object_activity_acc.rs: the activity pass runs per block right
    // after the block's protocol writes; the block's object / identity
    // collection activity rows (and the `.bit` and `.cell` feeds) are written
    // in its scope and its per-collection activity counts are added to the
    // aggregate rows the block's protocol writes left, in the same block.
    (ckbadger_store::CF_OBJECT_COLLECTION_ACTIVITIES, None),
    (ckbadger_store::CF_IDENTITY_COLLECTION_ACTIVITIES, None),
    (ckbadger_store::CF_MNFT_COLLECTION_AGG, None),
    (ckbadger_store::CF_IDENTITY_AGG, None),
    // `stats_script` holds only the SCRIPT_DAILY family (above).
    (ckbadger_store::CF_STATS_SCRIPT, None),
    // B-3 DAO daily snapshot (coordinator ruling): a sealed day's row is
    // written in its sealer block's scope (the next UTC+8 day's first block);
    // the open day's row is the DAO tip summary, written after every tip
    // change outside the journal, so the oracle runs the tip summary after
    // the replay (`with_tip_summary`), as the B-5 rollback will. The rest of
    // `stats_dao` (latest stats, top depositors) is tip summary only.
    (
        ckbadger_store::CF_STATS_DAO,
        Some(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT),
    ),
    // B-4: the HODL / cell-distribution trackers are rows (creation day →
    // capacity, cohort month → (used, balance)) written per block that moves
    // them, and the day snapshots they seal (`stats_hodl` HODL_WAVE,
    // CELL_DISTRIBUTION, ADDR_COHORT — the CF's only families) in the sealer
    // block's scope.
    (ckbadger_store::CF_HODL_CAPACITY_BY_DATE, None),
    (ckbadger_store::CF_CELL_DIST_COHORT_BY_MONTH, None),
    (ckbadger_store::CF_STATS_HODL, None),
];

/// Covered keys of a column family that is not covered as a whole: a
/// `sync_meta` key, or a key prefix, whose every live write is attributed to
/// its block.
const JOURNAL_COVERED_KEYS: &[(&str, &[u8])] = &[
    // B-3 sync/batch.rs: `sync_status` is written per block (that block's tip
    // and running totals) and each live-cell summary (current value) in its
    // block's scope.
    (ckbadger_store::CF_SYNC_META, sync_meta_keys::SYNC_STATUS),
    (
        ckbadger_store::CF_SYNC_META,
        sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT,
    ),
    (
        ckbadger_store::CF_SYNC_META,
        sync_meta_keys::LIVE_CELL_SUMMARY_INITIALIZED,
    ),
    // B-3 statistics.rs: the hourly retention states depend only on the
    // committed tip and are written in the batch's first block's scope. The
    // coverage contract (`entity_stats_undo_contract`) is not covered: since
    // B-5b it is written outside the journal (it describes which journal rows
    // exist, so a rollback must not restore it); the B-0 dump compares it.
    (
        ckbadger_store::CF_SYNC_META,
        sync_meta_keys::HOURLY_RETENTION_STATE_PREFIX,
    ),
    // B-4: each tracker's cursor row is written in the scope of every block.
    (ckbadger_store::CF_SYNC_META, sync_meta_keys::HODL_CURSOR),
    (
        ckbadger_store::CF_SYNC_META,
        sync_meta_keys::CELL_DIST_CURSOR,
    ),
];

fn is_covered(cf: &str, key: &[u8]) -> bool {
    JOURNAL_COVERED.iter().any(|(covered, prefix)| {
        *covered == cf && prefix.is_none_or(|prefix| key.first() == Some(&prefix))
    }) || JOURNAL_COVERED_KEYS
        .iter()
        .any(|(covered, prefix)| *covered == cf && key.starts_with(prefix))
}

const RANGE_BY_BLOCK_CFS: &[&str] = &[
    ckbadger_store::CF_BLOCK_HEADERS,
    ckbadger_store::CF_TX_INDEX,
    ckbadger_store::CF_TX_ACTIONS,
    ckbadger_store::CF_REORG_UNDO_LOG_BY_BLOCK,
];

type JournalEntry = (u64, String, Vec<u8>, Option<Vec<u8>>);

/// Every `UndoSeqScope::Journal` row, by block: `(seq, cf, key, previous)`.
fn read_journal(store: &CkbadgerStore) -> BTreeMap<i64, Vec<JournalEntry>> {
    let mut journal: BTreeMap<i64, Vec<JournalEntry>> = BTreeMap::new();
    for row in store.iterator_cf(
        store.cf_reorg_undo_log_by_block(),
        rocksdb::IteratorMode::Start,
    ) {
        let (key, value) = row.expect("iterate the undo log");
        let (block, seq) = ckbadger_store::keys::decode_reorg_undo_log_key(&key);
        assert!(
            ckbadger_store::keys::UndoSeqScope::Journal.owns(seq),
            "undo-log row at block {block} seq {seq:#x} is not a journal row"
        );
        let entry = ckbadger_store::decode_journal_value(&value).expect("decode a journal row");
        journal.entry(block).or_default().push((
            seq,
            entry.cf.to_string(),
            entry.key.to_vec(),
            entry.previous.map(<[u8]>::to_vec),
        ));
    }
    journal
}

/// The B-5 rollback, applied to a dump: for each block from the tip down to
/// `fork + 1`, delete its `RangeByBlock` rows and write its journal entries'
/// previous values back in reverse sequence order.
fn simulate_journal_rollback(
    at_tip: &Dump,
    journal: &BTreeMap<i64, Vec<JournalEntry>>,
    tip: usize,
    fork: usize,
) -> Dump {
    let mut dump = at_tip.clone();
    for block in (fork + 1..=tip).rev() {
        // `activities` keys open with the block number descending.
        let ascending = (block as i64).to_be_bytes();
        let descending = ckbadger_store::keys::encode_desc_block_num(block as i64);
        dump.retain(|(cf, key), _| {
            let prefix: &[u8] = if *cf == ckbadger_store::CF_TX_ACTIONS {
                &descending
            } else {
                &ascending
            };
            !(RANGE_BY_BLOCK_CFS.contains(cf) && key.starts_with(prefix))
        });
        let mut entries = journal.get(&(block as i64)).cloned().unwrap_or_default();
        entries.sort_by_key(|(seq, ..)| std::cmp::Reverse(*seq));
        for (_, cf, key, previous) in entries {
            let cf: &'static str = DOMAIN_CFS
                .iter()
                .copied()
                .find(|name| *name == cf)
                .unwrap_or_else(|| panic!("journal entry names unknown CF {cf}"));
            match previous {
                Some(value) => {
                    let value = if cf == ckbadger_store::CF_SYNC_META {
                        normalize_sync_meta(&key, &value)
                    } else {
                        value
                    };
                    dump.insert((cf, key), value);
                }
                None => {
                    dump.remove(&(cf, key));
                }
            }
        }
    }
    dump
}

/// Every rollback ends, like every tip change, with the DAO tip summary
/// (`refresh_latest_dao_statistics`): the open day's `stats_dao` 0x08 row and
/// the DAO singletons are functions of the tip, written outside the journal,
/// and rows of days after the tip's are removed. Run it over the replayed
/// rows — materialised in a fresh store, the summary computed there — and
/// take that store's `stats_dao` rows.
fn with_tip_summary(mut replayed: Dump) -> Dump {
    // A fixture store (no indexer, so no journal discipline): the replayed
    // rows are written directly.
    let stores = open_fixture_stores();
    for ((cf, key), value) in &replayed {
        // The dump keys the undo log by `block ‖ rank`; the tip summary
        // does not read it.
        if *cf == ckbadger_store::CF_REORG_UNDO_LOG_BY_BLOCK {
            continue;
        }
        stores
            .domain
            .put_cf(stores.domain.cf(cf), key, value)
            .expect("materialise a replayed row");
    }
    ckbadger_indexer::db::BatchWriter::new(stores.domain.clone(), stores._append.clone())
        .refresh_latest_dao_statistics()
        .expect("the DAO tip summary over the replayed rows");
    replayed.retain(|(cf, _), _| *cf != ckbadger_store::CF_STATS_DAO);
    for row in stores.domain.iterator_cf(
        stores.domain.cf(ckbadger_store::CF_STATS_DAO),
        rocksdb::IteratorMode::Start,
    ) {
        let (key, value) = row.expect("iterate stats_dao");
        replayed.insert((ckbadger_store::CF_STATS_DAO, key.to_vec()), value.to_vec());
    }
    replayed
}

fn only_covered(mut dump: Dump) -> Dump {
    dump.retain(|(cf, key), _| is_covered(cf, key));
    dump
}

/// The journal alone restores every covered CF to the store that never saw
/// the rolled-back blocks (both stores run `derive` at the same block).
async fn journal_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
    derive: Option<Derive>,
) -> Result<(), String> {
    let stores = open_stores();
    sync_with_derive(&stores, chain, plan, tip, derive).await;
    let at_tip = dump_domain(&stores.domain);
    let journal = read_journal(&stores.domain);
    let simulated = with_tip_summary(simulate_journal_rollback(&at_tip, &journal, tip, fork));
    let reference = open_stores();
    sync_with_derive(&reference, chain, plan, fork, derive).await;
    // Not vacuous: the covered rows the rolled-back blocks changed outside
    // their own block keys are restored by journal entries, not by luck.
    let changed_journaled_rows = only_covered(at_tip.clone())
        .into_iter()
        .filter(|((cf, key), value)| {
            !RANGE_BY_BLOCK_CFS.contains(cf)
                && only_covered(dump_domain(&reference.domain)).get(&(*cf, key.clone()))
                    != Some(value)
        })
        .count();
    let rolled_back_entries: usize = journal
        .range(fork as i64 + 1..)
        .map(|(_, entries)| entries.len())
        .sum();
    if changed_journaled_rows > 0 && rolled_back_entries == 0 {
        return Err(format!(
            "journal replay {tip} -> {fork}: {changed_journaled_rows} covered rows changed above the fork but the journal has no entry there"
        ));
    }
    compare_dumps(
        &only_covered(simulated),
        &only_covered(dump_domain(&reference.domain)),
        &format!("journal replay {tip} -> {fork}"),
    )
}

async fn assert_journal_properties(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    forks: &[usize],
) {
    assert_journal_properties_with_derive(chain, plan, tip, forks, None).await;
}

async fn assert_journal_properties_with_derive(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    forks: &[usize],
    derive: Option<Derive>,
) {
    let mut failures = Vec::new();
    for &fork in forks {
        if let Err(report) = journal_property(chain, plan, tip, fork, derive).await {
            failures.push(report);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Chain fixtures
// ---------------------------------------------------------------------------

const NETWORK: &str = "mainnet";
const GENESIS_C: u64 = 3_360_000_000_000_000_000;
const SECONDARY_EPOCH_REWARD: u64 = 61_369_863_013_698;
const AR_BASE: u64 = 10_000_000_000_000_000;
const EPOCH_LENGTH: u64 = 1800;

/// A real mainnet cellbase first witness (block 12,000,000): every
/// non-genesis cellbase must carry a valid RFC-0022 `CellbaseWitness`.
const CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";
const SECP_CODE_HASH: &str = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";

fn hex32(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn block_hash(number: u64) -> [u8; 32] {
    let mut hash = [0x5bu8; 32];
    hash[0..8].copy_from_slice(&number.to_le_bytes());
    hash
}

fn tx_hash(tag: u16) -> [u8; 32] {
    let mut hash = [0x7au8; 32];
    hash[0..2].copy_from_slice(&tag.to_be_bytes());
    hash
}

fn lock(owner: u8) -> Script {
    Script {
        code_hash: SECP_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode([owner; 20])),
    }
}

fn header(number: u64, timestamp_ms: u64) -> HeaderView {
    let mut dao = [0u8; 32];
    dao[0..8].copy_from_slice(&GENESIS_C.to_le_bytes());
    dao[8..16].copy_from_slice(&(AR_BASE + number * 1_000_000_000).to_le_bytes());
    dao[16..24].copy_from_slice(&10_000_000_000u64.to_le_bytes());
    dao[24..32].copy_from_slice(&100_000_000_000_000u64.to_le_bytes());
    let epoch = (EPOCH_LENGTH << 40) | (number << 24);
    HeaderView {
        version: "0x0".to_string(),
        compact_target: "0x1a08a97e".to_string(),
        timestamp: format!("0x{timestamp_ms:x}"),
        number: format!("0x{number:x}"),
        epoch: format!("0x{epoch:x}"),
        parent_hash: if number == 0 {
            hex32([0; 32])
        } else {
            hex32(block_hash(number - 1))
        },
        transactions_root: hex32([0x22; 32]),
        proposals_hash: hex32([0x33; 32]),
        extra_hash: hex32([0x44; 32]),
        dao: format!("0x{}", hex::encode(dao)),
        nonce: "0x1".to_string(),
        hash: hex32(block_hash(number)),
    }
}

fn cellbase(number: u64, outputs: Vec<(CellOutput, String)>) -> TransactionView {
    let (outputs, outputs_data) = outputs.into_iter().unzip();
    TransactionView {
        hash: hex32(tx_hash(0xc000 + number as u16)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: format!("0x{number:x}"),
            previous_output: OutPoint {
                tx_hash: hex32([0; 32]),
                index: "0xffffffff".to_string(),
            },
        }],
        outputs,
        outputs_data,
        witnesses: vec![CELLBASE_WITNESS.to_string()],
    }
}

fn output(capacity_ckb: u64, lock: Script, type_: Option<Script>) -> CellOutput {
    CellOutput {
        capacity: format!("0x{:x}", capacity_ckb * 100_000_000),
        lock,
        type_,
    }
}

fn input(tx: [u8; 32], index: u32) -> CellInput {
    CellInput {
        since: "0x0".to_string(),
        previous_output: OutPoint {
            tx_hash: hex32(tx),
            index: format!("0x{index:x}"),
        },
    }
}

fn tx(
    tag: u16,
    inputs: Vec<CellInput>,
    outputs: Vec<(CellOutput, String)>,
    witnesses: Vec<String>,
) -> TransactionView {
    let (outputs, outputs_data): (Vec<_>, Vec<_>) = outputs.into_iter().unzip();
    let witnesses = if witnesses.is_empty() {
        vec!["0x".to_string(); inputs.len().max(1)]
    } else {
        witnesses
    };
    TransactionView {
        hash: hex32(tx_hash(tag)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs,
        outputs,
        outputs_data,
        witnesses,
    }
}

struct ChainBuilder {
    blocks: Vec<BlockResponseWithCycles>,
}

impl ChainBuilder {
    fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    /// Append the genesis block: its cellbase funds `outputs` (the chain's
    /// only issuance; every later protocol event spends from it).
    fn genesis(&mut self, timestamp_ms: u64, outputs: Vec<(CellOutput, String)>) {
        assert!(self.blocks.is_empty());
        self.blocks.push(BlockResponseWithCycles {
            block: BlockView {
                header: header(0, timestamp_ms),
                uncles: vec![],
                transactions: vec![cellbase(0, outputs)],
                proposals: vec![],
            },
            cycles: None,
        });
    }

    /// Append the next block at `timestamp_ms` with `txs` after its cellbase.
    fn push(&mut self, timestamp_ms: u64, txs: Vec<TransactionView>) -> u64 {
        let number = self.blocks.len() as u64;
        let mut transactions = vec![cellbase(
            number,
            vec![(output(1_000, lock(0xc0), None), "0x".to_string())],
        )];
        transactions.extend(txs);
        self.blocks.push(BlockResponseWithCycles {
            block: BlockView {
                header: header(number, timestamp_ms),
                uncles: vec![],
                transactions,
                proposals: vec![],
            },
            cycles: None,
        });
        number
    }

    fn next_number(&self) -> u64 {
        self.blocks.len() as u64
    }
}

/// 2026-09-22 09:00:00 UTC+8.
const T0: u64 = 1_790_038_800_000;
const MINUTE: u64 = 60_000;

/// A chain of plain CKB transfers: block 0 funds owners 1..=8, then every
/// block moves one owner's cell to the next owner, so cells are created and
/// consumed inside every rolled-back range.
fn transfer_chain(blocks: u64) -> Vec<BlockResponseWithCycles> {
    let mut chain = ChainBuilder::new();
    chain.genesis(
        T0,
        (1..=8u8)
            .map(|owner| (output(10_000, lock(owner), None), "0x".to_string()))
            .collect(),
    );
    // Each owner's current cell, as (tx, index, capacity).
    let mut cells: Vec<([u8; 32], u32, u64)> = (0..8)
        .map(|i| (tx_hash(0xc000), i as u32, 10_000))
        .collect();
    for n in 1..blocks {
        let from = (n as usize) % 8;
        let to = (from + 1) % 8;
        let (prev_tx, prev_index, capacity) = cells[from];
        let tag = 0x100 + n as u16;
        let moved = capacity - 1;
        chain.push(
            T0 + n * 10 * MINUTE,
            vec![tx(
                tag,
                vec![input(prev_tx, prev_index)],
                vec![
                    (
                        output(moved / 2, lock(to as u8 + 1), None),
                        "0x".to_string(),
                    ),
                    (
                        output(moved - moved / 2, lock(from as u8 + 1), None),
                        "0x".to_string(),
                    ),
                ],
                vec![],
            )],
        );
        cells[from] = (tx_hash(tag), 1, moved - moved / 2);
    }
    let _ = chain.next_number();
    chain.blocks
}

// ---------------------------------------------------------------------------
// The protocol chain: every entity kind, inside the rolled-back ranges
// ---------------------------------------------------------------------------

use crate::object_protocol_state::{
    create_bit_cell_type_script, create_cluster_data, create_cluster_type_script,
    create_mnft_class_data, create_mnft_class_type_script, create_mnft_issuer_data,
    create_mnft_issuer_type_script, create_mnft_token_data, create_mnft_token_type_script,
    create_spore_data, create_spore_type_script, encode_dotbit_account_cell_witness,
};
use ckbadger_indexer::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID;
use ckbadger_indexer::parser::udt::XUDT_CODE_HASH_DATA1;
use ckbadger_indexer::parser::DotCellParser;

const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
/// did:ckb, mainnet (`docs/metadata/scripts/did-ckb.toml`).
const DID_CKB_CODE_HASH: &str =
    "0x4a06164dc34dccade5afe3e847a97b6db743e79f5477fa3295acf02849c5984a";
/// `.cell` account type / lock and the namespace, testnet deployment.
const DOTCELL_ACCOUNT_TYPE: &str =
    "0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9";
const DOTCELL_ACCOUNT_LOCK: &str =
    "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd";
const DOTCELL_NAMESPACE: &str = "0x2510c78057479c9b023fe6e98ce43979e92a1353";
/// Fiber funding lock, mainnet.
const FIBER_FUNDING_LOCK: &str =
    "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1";
/// The `.bit Cell` legacy layout (testnet "20240507.bit", args `0x`).
const BIT_CELL_DATA: &str = "0x000000003c00000010000000240000002c000000a7d4860aaf1dc83daedf75d6022811d2c2ae250b1b46fc69000000000c00000032303234303530372e626974";

/// 2026-09-22 23:00:00 UTC+8 (15:00 UTC): block 6 is the first block of the
/// next UTC+8 day, and every sixth block opens a new UTC hour.
const T_EVENING: u64 = 1_790_089_200_000;

fn data(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn plain(capacity_ckb: u64, owner: u8) -> (CellOutput, String) {
    (output(capacity_ckb, lock(owner), None), "0x".to_string())
}

fn typed(capacity_ckb: u64, owner: u8, type_: Script, bytes: &[u8]) -> (CellOutput, String) {
    (output(capacity_ckb, lock(owner), Some(type_)), data(bytes))
}

fn dao_type() -> Script {
    Script {
        code_hash: DAO_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: "0x".to_string(),
    }
}

fn xudt_type() -> Script {
    Script {
        code_hash: XUDT_CODE_HASH_DATA1.to_string(),
        hash_type: "data1".to_string(),
        args: format!("0x{}", hex::encode([0x31u8; 32])),
    }
}

fn did_type(id: u8) -> Script {
    Script {
        code_hash: DID_CKB_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode([id; 32])),
    }
}

fn dotbit_type() -> Script {
    Script {
        code_hash: DOTBIT_ACCOUNT_CELL_TYPE_ID.to_string(),
        hash_type: "type".to_string(),
        args: "0x".to_string(),
    }
}

fn dotbit_data(account_id: [u8; 20]) -> Vec<u8> {
    let mut bytes = vec![0x21u8; 32];
    bytes.extend_from_slice(&account_id);
    bytes.extend_from_slice(&[0u8; 20]);
    bytes.extend_from_slice(&4_102_444_800u64.to_le_bytes());
    bytes
}

fn dotcell_name(capacity_ckb: u64, label: &str, owner: [u8; 20]) -> (CellOutput, String) {
    let mut bytes = vec![3u8];
    bytes.extend_from_slice(&DotCellParser::records_hash(&[0, 0]));
    bytes.extend_from_slice(&[0u8; 20]);
    bytes.extend_from_slice(&4_102_444_800u64.to_le_bytes()[..5]);
    bytes.extend_from_slice(&owner);
    bytes.extend_from_slice(&owner);
    bytes.extend_from_slice(label.as_bytes());
    (
        CellOutput {
            capacity: format!("0x{:x}", capacity_ckb * 100_000_000),
            lock: Script {
                code_hash: DOTCELL_ACCOUNT_LOCK.to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            },
            type_: Some(Script {
                code_hash: DOTCELL_ACCOUNT_TYPE.to_string(),
                hash_type: "type".to_string(),
                args: DOTCELL_NAMESPACE.to_string(),
            }),
        },
        data(&bytes),
    )
}

/// `WitnessArgs { lock: None, input_type: None, output_type: payload }`.
fn witness_with_records(payload: &[u8]) -> String {
    let total = 16 + 4 + payload.len();
    let mut witness = Vec::with_capacity(total);
    witness.extend_from_slice(&(total as u32).to_le_bytes());
    for _ in 0..3 {
        witness.extend_from_slice(&16u32.to_le_bytes());
    }
    witness.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    witness.extend_from_slice(payload);
    data(&witness)
}

fn fiber_funding(capacity_ckb: u64) -> (CellOutput, String) {
    (
        CellOutput {
            capacity: format!("0x{:x}", capacity_ckb * 100_000_000),
            lock: Script {
                code_hash: FIBER_FUNDING_LOCK.to_string(),
                hash_type: "type".to_string(),
                args: format!("0x{}", hex::encode([0x66u8; 20])),
            },
            type_: None,
        },
        "0x".to_string(),
    )
}

fn amount(units: u128) -> Vec<u8> {
    units.to_le_bytes().to_vec()
}

/// Genesis funding outputs, one per event that needs fresh capacity.
const FUND: u64 = 100_000;
fn funding(index: u32) -> CellInput {
    input(tx_hash(0xc000), index)
}

/// Every entity kind the live writer indexes, created, moved and destroyed
/// in blocks 1..=16 (10 minutes apart from 23:00 UTC+8, so the UTC+8 day and
/// two UTC hours turn inside the range), then `plain_blocks` blocks of plain
/// transfers so a 36-deep rollback can land below every event.
fn protocol_chain(plain_blocks: u64) -> Vec<BlockResponseWithCycles> {
    let t = |n: u64| T_EVENING + n * 10 * MINUTE;
    let cluster_id = [0x11u8; 32];
    let spore_id = [0x12u8; 32];
    let account_id = [0x13u8; 20];
    let issuer_type = create_mnft_issuer_type_script(&format!("0x{}", hex::encode([0x14u8; 32])));
    let issuer_id = [0x15u8; 20];
    let class_type = create_mnft_class_type_script(&issuer_id, 0);
    let mut class_id = issuer_id.to_vec();
    class_id.extend_from_slice(&0u32.to_le_bytes());
    let token_type = create_mnft_token_type_script(&class_id, 0);

    let mut chain = ChainBuilder::new();
    chain.genesis(
        T_EVENING - 10 * MINUTE,
        (0..16).map(|i| plain(FUND, 0x20 + i as u8)).collect(),
    );
    // 1: a Spore cluster.
    chain.push(
        t(1),
        vec![tx(
            0x0101,
            vec![funding(0)],
            vec![
                typed(
                    1_000,
                    0x01,
                    create_cluster_type_script(&cluster_id),
                    &create_cluster_data("Rollback", "{\"dob\":{\"ver\":1}}"),
                ),
                plain(FUND - 1_001, 0x20),
            ],
            vec![],
        )],
    );
    // 2: a Spore minted into it; an xUDT mint.
    chain.push(
        t(2),
        vec![
            tx(
                0x0201,
                vec![funding(1)],
                vec![
                    typed(
                        1_000,
                        0x01,
                        create_spore_type_script(&spore_id),
                        &create_spore_data("text/plain", b"rollback", Some(&cluster_id)),
                    ),
                    plain(FUND - 1_001, 0x21),
                ],
                vec![],
            ),
            tx(
                0x0202,
                vec![funding(2)],
                vec![
                    typed(500, 0x02, xudt_type(), &amount(1_000)),
                    plain(FUND - 501, 0x22),
                ],
                vec![],
            ),
        ],
    );
    // 3: mNFT issuer + class; a NervosDAO deposit (output 0).
    chain.push(
        t(3),
        vec![
            tx(
                0x0301,
                vec![funding(3)],
                vec![
                    typed(500, 0x03, issuer_type, &create_mnft_issuer_data(1, 0, None)),
                    typed(
                        500,
                        0x03,
                        class_type,
                        &create_mnft_class_data(100, 1, 0, "Class", "rollback"),
                    ),
                    plain(FUND - 1_001, 0x23),
                ],
                vec![],
            ),
            tx(
                0x0302,
                vec![funding(4)],
                vec![
                    typed(1_000, 0x04, dao_type(), &[0u8; 8]),
                    plain(FUND - 1_001, 0x24),
                ],
                vec![],
            ),
        ],
    );
    // 4: an mNFT token; a did:ckb.
    chain.push(
        t(4),
        vec![
            tx(
                0x0401,
                vec![funding(5)],
                vec![
                    typed(
                        500,
                        0x05,
                        token_type.clone(),
                        &create_mnft_token_data(&[0u8; 8], 0, 0),
                    ),
                    plain(FUND - 501, 0x25),
                ],
                vec![],
            ),
            tx(
                0x0402,
                vec![funding(6)],
                vec![
                    typed(500, 0x06, did_type(0x16), b"did"),
                    plain(FUND - 501, 0x26),
                ],
                vec![],
            ),
        ],
    );
    // 5: a .bit AccountCell (with its name witness); a .bit Cell.
    chain.push(
        t(5),
        vec![
            tx(
                0x0501,
                vec![funding(7)],
                vec![
                    typed(1_000, 0x07, dotbit_type(), &dotbit_data(account_id)),
                    plain(FUND - 1_001, 0x27),
                ],
                vec![encode_dotbit_account_cell_witness(
                    &account_id,
                    "rollback.bit",
                )],
            ),
            tx(
                0x0502,
                vec![funding(8)],
                vec![
                    (
                        output(1_000, lock(0x08), Some(create_bit_cell_type_script())),
                        BIT_CELL_DATA.to_string(),
                    ),
                    plain(FUND - 1_001, 0x28),
                ],
                vec![],
            ),
        ],
    );
    // 6 (00:00 UTC+8, a new day and a new UTC hour): Spore transfer; xUDT
    // transfer 400 to owner 0x09.
    chain.push(
        t(6),
        vec![
            tx(
                0x0601,
                vec![input(tx_hash(0x0201), 0)],
                vec![typed(
                    999,
                    0x09,
                    create_spore_type_script(&spore_id),
                    &create_spore_data("text/plain", b"rollback", Some(&cluster_id)),
                )],
                vec![],
            ),
            tx(
                0x0602,
                vec![input(tx_hash(0x0202), 0)],
                vec![
                    typed(250, 0x09, xudt_type(), &amount(400)),
                    typed(249, 0x02, xudt_type(), &amount(600)),
                ],
                vec![],
            ),
        ],
    );
    // 7: DAO withdraw request (same output index as the deposit input).
    chain.push(
        t(7),
        vec![tx(
            0x0701,
            vec![input(tx_hash(0x0302), 0)],
            vec![typed(1_000, 0x04, dao_type(), &3u64.to_le_bytes())],
            vec![],
        )],
    );
    // 8: a .cell name (its owner is a 20-byte lock-hash prefix participant).
    chain.push(
        t(8),
        vec![tx(
            0x0801,
            vec![funding(9)],
            vec![
                dotcell_name(300, "rollback", [0x0a; 20]),
                plain(FUND - 301, 0x29),
            ],
            vec![witness_with_records(&[0, 0]), "0x".to_string()],
        )],
    );
    // 9: mNFT token transfer; did:ckb transfer.
    chain.push(
        t(9),
        vec![
            tx(
                0x0901,
                vec![input(tx_hash(0x0401), 0)],
                vec![typed(
                    499,
                    0x0b,
                    token_type,
                    &create_mnft_token_data(&[0u8; 8], 0, 0),
                )],
                vec![],
            ),
            tx(
                0x0902,
                vec![input(tx_hash(0x0402), 0)],
                vec![typed(499, 0x0b, did_type(0x16), b"did")],
                vec![],
            ),
        ],
    );
    // 10: a Fiber channel opens.
    chain.push(
        t(10),
        vec![tx(
            0x0a01,
            vec![funding(10)],
            vec![fiber_funding(1_000), plain(FUND - 1_001, 0x2a)],
            vec![],
        )],
    );
    // 11: the Spore melts; xUDT burn of 100.
    chain.push(
        t(11),
        vec![
            tx(
                0x0b01,
                vec![input(tx_hash(0x0601), 0)],
                vec![plain(998, 0x09)],
                vec![],
            ),
            tx(
                0x0b02,
                vec![input(tx_hash(0x0602), 0)],
                vec![typed(249, 0x09, xudt_type(), &amount(300))],
                vec![],
            ),
        ],
    );
    // 12 (a new UTC hour): DAO withdraw completion.
    chain.push(
        t(12),
        vec![tx(
            0x0c01,
            vec![input(tx_hash(0x0701), 0)],
            vec![plain(1_000, 0x04)],
            vec![],
        )],
    );
    // 13: the .bit account moves to a new owner; the .bit Cell burns.
    chain.push(
        t(13),
        vec![
            tx(
                0x0d01,
                vec![input(tx_hash(0x0501), 0)],
                vec![typed(999, 0x0c, dotbit_type(), &dotbit_data(account_id))],
                vec![encode_dotbit_account_cell_witness(
                    &account_id,
                    "rollback.bit",
                )],
            ),
            tx(
                0x0d02,
                vec![input(tx_hash(0x0502), 0)],
                vec![plain(999, 0x08)],
                vec![],
            ),
        ],
    );
    // 14: the Fiber channel closes.
    chain.push(
        t(14),
        vec![tx(
            0x0e01,
            vec![input(tx_hash(0x0a01), 0)],
            vec![plain(500, 0x0d), plain(499, 0x0e)],
            vec![],
        )],
    );
    // 15: a data cell (a `cell_by_data_hash` row) ...
    chain.push(
        t(15),
        vec![tx(
            0x0f01,
            vec![funding(11)],
            vec![
                (output(1_000, lock(0x0f), None), data(b"code-cell-bytes")),
                plain(FUND - 1_001, 0x2b),
            ],
            vec![],
        )],
    );
    // 16: ... consumed again, inside the same rolled-back range.
    chain.push(
        t(16),
        vec![tx(
            0x1001,
            vec![input(tx_hash(0x0f01), 0)],
            vec![plain(999, 0x0f)],
            vec![],
        )],
    );
    // Plain transfers: owner 0x30+i moves a funding cell each block.
    for n in 0..plain_blocks {
        let number = chain.next_number();
        let index = 12 + (n % 4) as u32;
        let source = if n < 4 {
            funding(index)
        } else {
            input(tx_hash(0x2000 + (n - 4) as u16), 0)
        };
        let capacity = FUND - 1 - n;
        chain.push(
            t(number),
            vec![tx(
                0x2000 + n as u16,
                vec![source],
                vec![plain(capacity, 0x30 + (n % 7) as u8)],
                vec![],
            )],
        );
    }
    chain.blocks
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

fn single_block_plan(len: usize) -> Vec<RangeInclusive<usize>> {
    (0..len).map(|n| n..=n).collect()
}

fn chunked_plan(len: usize, size: usize) -> Vec<RangeInclusive<usize>> {
    (0..len)
        .step_by(size)
        .map(|start| start..=(start + size - 1).min(len - 1))
        .collect()
}

#[tokio::test]
async fn protocol_chain_single_block_batches_roll_back_byte_equal() {
    let chain = protocol_chain(0);
    let plan = single_block_plan(chain.len());
    assert_rollback_properties(&chain, &plan, 16, &[13, 10, 5]).await;
}

#[tokio::test]
async fn protocol_chain_fork_inside_multi_block_batches_rolls_back_byte_equal() {
    let chain = protocol_chain(0);
    // Batches 0, 1-4, 5-8, 9-12, 13-16: every fork below lands inside one.
    let mut plan: Vec<RangeInclusive<usize>> = single_block_plan(1);
    plan.extend(
        chunked_plan(16, 4)
            .into_iter()
            .map(|r| r.start() + 1..=r.end() + 1),
    );
    assert_rollback_properties(&chain, &plan, 16, &[14, 10, 6, 2]).await;
}

#[tokio::test]
async fn protocol_chain_rolls_back_36_blocks_byte_equal() {
    let chain = protocol_chain(30);
    let tip = chain.len() - 1;
    assert_eq!(tip, 46);
    let plan = chunked_plan(chain.len(), 7);
    assert_rollback_property(&chain, &plan, tip, tip - 36).await;
}

#[tokio::test]
async fn derived_aggregates_complete_below_the_target_roll_back_byte_equal() {
    let chain = protocol_chain(0);
    let plan = single_block_plan(chain.len());
    // Completed at 4: rollbacks to 10 and 5 are undone row by row.
    for fork in [10, 5] {
        rollback_property_with_derive(&chain, &plan, 16, fork, Some(Derive::CompleteAt(4)))
            .await
            .unwrap_or_else(|report| panic!("{report}"));
    }
}

#[tokio::test]
async fn derived_aggregates_complete_above_the_target_are_reset() {
    let chain = protocol_chain(0);
    let plan = chunked_plan(chain.len(), 3);
    rollback_property_with_derive(&chain, &plan, 16, 5, Some(Derive::CompleteAt(9)))
        .await
        .unwrap_or_else(|report| panic!("{report}"));
}

#[tokio::test]
async fn derived_aggregates_running_are_reset() {
    let chain = protocol_chain(0);
    let plan = single_block_plan(chain.len());
    rollback_property_with_derive(&chain, &plan, 16, 12, Some(Derive::RunningAt(8)))
        .await
        .unwrap_or_else(|report| panic!("{report}"));
}

#[tokio::test]
async fn journal_restores_covered_cfs_single_block_batches() {
    let chain = protocol_chain(0);
    let plan = single_block_plan(chain.len());
    assert_journal_properties(&chain, &plan, 16, &[13, 10, 5, 0]).await;
}

#[tokio::test]
async fn journal_restores_covered_cfs_forks_inside_batches() {
    let chain = protocol_chain(0);
    let mut plan: Vec<RangeInclusive<usize>> = single_block_plan(1);
    plan.extend(
        chunked_plan(16, 4)
            .into_iter()
            .map(|r| r.start() + 1..=r.end() + 1),
    );
    assert_journal_properties(&chain, &plan, 16, &[14, 10, 6, 2]).await;
}

/// The three derived aggregates, derived to completion when block 4 commits
/// and maintained by every live batch after it: their rows are written per
/// block, so the journal restores them at forks inside later batches (the
/// DAO request at 7 and completion at 12, the object collection rows, the
/// `.bit Cell` transitions at 5 and 13).
#[tokio::test]
async fn journal_restores_covered_cfs_with_derived_aggregates() {
    let chain = protocol_chain(0);
    let mut plan: Vec<RangeInclusive<usize>> = single_block_plan(1);
    plan.extend(
        chunked_plan(16, 4)
            .into_iter()
            .map(|r| r.start() + 1..=r.end() + 1),
    );
    assert_journal_properties_with_derive(
        &chain,
        &plan,
        16,
        &[14, 10, 6],
        Some(Derive::CompleteAt(4)),
    )
    .await;
}

#[tokio::test]
async fn journal_restores_covered_cfs_36_blocks_deep() {
    let chain = protocol_chain(30);
    let tip = chain.len() - 1;
    let plan = chunked_plan(chain.len(), 7);
    assert_journal_properties(&chain, &plan, tip, &[tip - 36]).await;
}

/// The whole protocol chain as ONE live batch (blocks 0..=16): one address
/// moves its balance, stats and activities in many blocks of the batch, the
/// xUDT holder is minted, transferred and burnt, the DAO cell is deposited,
/// withdrawn and completed, all inside it. The batch must commit — no key
/// written from two sub-batches, every key in ascending block order — and the
/// journal alone must restore the covered rows at forks inside the batch.
#[tokio::test]
async fn journal_restores_covered_cfs_whole_protocol_chain_in_one_batch() {
    let chain = protocol_chain(0);
    assert_eq!(chain.len(), 17);
    let plan = chunked_plan(chain.len(), chain.len());
    assert_eq!(plan, vec![0..=16]);
    let stores = open_stores();
    write_live_batch(&stores.indexer, chain.clone())
        .await
        .unwrap_or_else(|e| panic!("the one-batch protocol chain must commit: {e:#}"));
    assert_eq!(
        stores.domain.get_sync_status().unwrap().tip_block_number,
        16
    );
    assert_journal_properties(&chain, &plan, 16, &[14, 9, 4]).await;
}

/// The open day's DAO daily snapshot (`stats_dao` 0x08 of the tip's UTC+8
/// day) is the DAO tip summary, a function of the tip: the row the forward
/// path leaves at a tip, the row a rollback to that tip recomputes, and the
/// row the journal replay followed by the tip summary leaves are the same
/// bytes, and no row is left for a later day. Forks on the first day (2, 5:
/// the day block 6 sealed in the rolled-back store is open again) and on the
/// second (6: its first block, 8, 14), inside multi-block batches.
#[tokio::test]
async fn the_open_day_dao_snapshot_is_a_function_of_the_tip() {
    let chain = protocol_chain(0);
    let mut plan: Vec<RangeInclusive<usize>> = single_block_plan(1);
    plan.extend(
        chunked_plan(16, 4)
            .into_iter()
            .map(|r| r.start() + 1..=r.end() + 1),
    );
    let dao_days = |store: &CkbadgerStore| -> BTreeMap<Vec<u8>, Vec<u8>> {
        store
            .iterator_cf(
                store.cf(ckbadger_store::CF_STATS_DAO),
                rocksdb::IteratorMode::Start,
            )
            .map(|row| row.expect("iterate stats_dao"))
            .filter(|(key, _)| key.first() == Some(&keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT))
            .map(|(key, value)| (key.to_vec(), value.to_vec()))
            .collect()
    };
    let rolled_from_tip = open_stores();
    sync_prefix(&rolled_from_tip, &chain, &plan, 0, 16).await;
    let at_tip = dump_domain(&rolled_from_tip.domain);
    let journal = read_journal(&rolled_from_tip.domain);
    let days_at_tip = dao_days(&rolled_from_tip.domain);
    assert_eq!(
        days_at_tip.len(),
        2,
        "the protocol chain spans two UTC+8 days"
    );
    for fork in [2, 5, 6, 8, 14] {
        let forward = open_stores();
        sync_prefix(&forward, &chain, &plan, 0, fork).await;
        let forward_days = dao_days(&forward.domain);
        let tip_day = ckbadger_common::stats_bucket::stats_day(
            forward
                .domain
                .get_block_header(fork as i64)
                .unwrap()
                .unwrap()
                .chain_time_ms,
        )
        .unwrap();
        let open_key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
            tip_day.format("%Y%m%d").to_string().as_bytes(),
        );
        assert_eq!(
            forward_days.keys().last(),
            Some(&open_key),
            "fork {fork}: the forward store's last day row is the open day's"
        );

        let rolled = open_stores();
        sync_prefix(&rolled, &chain, &plan, 0, 16).await;
        reorg_to(&rolled.indexer, fork as i64).await.unwrap();
        assert_eq!(
            dao_days(&rolled.domain),
            forward_days,
            "fork {fork}: the rollback's DAO daily rows differ from the forward path's"
        );

        let replayed = with_tip_summary(simulate_journal_rollback(&at_tip, &journal, 16, fork));
        let replayed_days: BTreeMap<Vec<u8>, Vec<u8>> = replayed
            .into_iter()
            .filter(|((cf, key), _)| {
                *cf == ckbadger_store::CF_STATS_DAO
                    && key.first() == Some(&keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT)
            })
            .map(|((_, key), value)| (key, value))
            .collect();
        assert_eq!(
            replayed_days, forward_days,
            "fork {fork}: the journal replay and tip summary leave other DAO daily rows"
        );
    }
}

#[tokio::test]
async fn plain_transfers_single_block_batches_roll_back_byte_equal() {
    let chain = transfer_chain(12);
    let plan = single_block_plan(chain.len());
    assert_rollback_property(&chain, &plan, 11, 8).await;
}

#[tokio::test]
async fn plain_transfers_fork_inside_a_multi_block_batch_roll_back_byte_equal() {
    let chain = transfer_chain(14);
    let plan = chunked_plan(chain.len(), 5);
    // Batches 0-4, 5-9, 10-13; fork at 7 lands inside the second batch.
    assert_rollback_property(&chain, &plan, 13, 7).await;
}

// ---------------------------------------------------------------------------
// B-5c · a restart changes no data; bulk and live write the same rollups
// ---------------------------------------------------------------------------

/// The script-reference CFs: the reference usage rows, the reference →
/// binary mapping, the version and family rollups and their two indexes.
const SCRIPT_REFERENCE_CFS: &[&str] = &[
    ckbadger_store::store::CF_SCRIPT_REFERENCE_INFO,
    ckbadger_store::store::CF_SCRIPT_REFERENCE_TO_VERSION,
    ckbadger_store::store::CF_SCRIPT_VERSIONS,
    ckbadger_store::store::CF_SCRIPT_VERSIONS_BY_FAMILY,
    ckbadger_store::store::CF_SCRIPT_VERSIONS_BY_LABEL,
    ckbadger_store::store::CF_SCRIPT_FAMILIES,
];

fn script_reference_rows(dump: &Dump) -> Dump {
    dump.iter()
        .filter(|((cf, _), _)| SCRIPT_REFERENCE_CFS.contains(cf))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Sync `chain` along `plan` restarting the indexer (the startup label import
/// and a new `Indexer` through the production startup steps,
/// `live_test_support::restart`) after each block of `restart_after`; the
/// domain store must be byte-equal to one that synced the chain in one run
/// (one label import before its first batch) with the same batch boundaries.
/// A restart is not a computation point: chain progression is.
async fn restart_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    restart_after: &[usize],
) -> Result<(), String> {
    let tip = chain.len() - 1;
    let one_run = open_stores();
    startup_label_import(&one_run.domain, NETWORK).map_err(|e| format!("{e:#}"))?;
    sync_prefix(&one_run, chain, plan, 0, tip).await;

    let mut restarted = open_stores();
    startup_label_import(&restarted.domain, NETWORK).map_err(|e| format!("{e:#}"))?;
    let mut from = 0;
    for &at in restart_after {
        sync_prefix(&restarted, chain, plan, from, at).await;
        restarted.indexer = restart(restarted.domain.clone(), restarted._append.clone(), NETWORK)
            .map_err(|e| format!("restart after block {at}: {e:#}"))?;
        from = at + 1;
    }
    sync_prefix(&restarted, chain, plan, from, tip).await;

    let expected = dump_domain(&one_run.domain);
    let actual = dump_domain(&restarted.domain);
    compare_dumps(
        &script_reference_rows(&actual),
        &script_reference_rows(&expected),
        &format!("script-reference CFs after restarts at {restart_after:?}"),
    )?;
    compare_dumps(
        &actual,
        &expected,
        &format!("domain store after restarts at {restart_after:?}"),
    )
}

#[tokio::test]
async fn protocol_chain_restarts_change_no_row() {
    let chain = protocol_chain(0);
    let plan = single_block_plan(chain.len());
    restart_property(&chain, &plan, &[0, 3, 7, 11, 15])
        .await
        .unwrap_or_else(|report| panic!("{report}"));
}

#[tokio::test]
async fn protocol_chain_restart_inside_multi_block_batches_changes_no_row() {
    let chain = protocol_chain(0);
    let mut plan: Vec<RangeInclusive<usize>> = single_block_plan(1);
    plan.extend(
        chunked_plan(16, 4)
            .into_iter()
            .map(|r| r.start() + 1..=r.end() + 1),
    );
    restart_property(&chain, &plan, &[4, 10])
        .await
        .unwrap_or_else(|report| panic!("{report}"));
}

/// Bulk finalize and the live per-block rollup classify every script
/// reference with the one routine and build the rollup with the one builder,
/// so a bulk-built store and a live-built store hold the same rows in the
/// script-reference CFs (here: the protocol chain, whose Spore, cluster and
/// xUDT references are data-hash references).
#[tokio::test]
async fn bulk_finalize_and_live_sync_write_the_same_script_reference_rows() {
    let chain = protocol_chain(0);
    let bulk = ckbadger_indexer::sync::materialize_bulk_artifacts_for_test(&chain)
        .unwrap_or_else(|e| panic!("bulk materialization: {e:#}"));
    let bulk_rows: Dump = bulk
        .script_binding_rows
        .iter()
        .map(|((cf, key), value)| ((*cf, key.clone()), value.clone()))
        .collect();
    // The fixture has no labels, so no family rows; the label-seeded parity
    // test in sync/batch.rs (five deployment cases) covers the family CFs.
    for cf in [
        ckbadger_store::store::CF_SCRIPT_REFERENCE_INFO,
        ckbadger_store::store::CF_SCRIPT_REFERENCE_TO_VERSION,
        ckbadger_store::store::CF_SCRIPT_VERSIONS,
    ] {
        assert!(
            bulk_rows.keys().any(|(row_cf, _)| *row_cf == cf),
            "{cf}: the bulk snapshot does not carry this CF, or the fixture does not exercise it"
        );
    }

    let live = open_stores();
    let tip = chain.len() - 1;
    sync_prefix(&live, &chain, &chunked_plan(chain.len(), 4), 0, tip).await;
    let live_rows = script_reference_rows(&dump_domain(&live.domain));

    let data_hash_mappings = bulk_rows
        .keys()
        .filter(|(cf, key)| {
            *cf == ckbadger_store::store::CF_SCRIPT_REFERENCE_TO_VERSION && key[0] != 1
        })
        .count();
    assert!(
        data_hash_mappings > 0,
        "the fixture must exercise data-hash reference mappings"
    );
    compare_dumps(
        &live_rows,
        &bulk_rows,
        "live script-reference rows vs bulk finalize",
    )
    .unwrap_or_else(|report| panic!("{report}"));
}

// ---------------------------------------------------------------------------
// B-5c · the lazy cycles record lives at its transaction's canonical slot
// ---------------------------------------------------------------------------

/// Persist a lazy cycles record (here a replay failure) for every
/// non-cellbase transaction of `blocks`, the way the cycles worker does after
/// the block committed: outside any batch.
fn persist_cycles_records(store: &CkbadgerStore, blocks: RangeInclusive<usize>) {
    for block in blocks {
        let block = block as i64;
        for (index, entry) in store.list_block_txs(block).unwrap() {
            if entry.is_cellbase {
                continue;
            }
            persist_replay_failure(store, block, index, &entry.tx_hash);
        }
    }
}

fn persist_replay_failure(store: &CkbadgerStore, block: i64, index: i32, tx_hash: &[u8; 32]) {
    let header = store.get_block_header(block).unwrap().unwrap();
    let block_hash: [u8; 32] = header.hash.as_slice().try_into().unwrap();
    store
        .persist_tx_cycles(
            block,
            index,
            tx_hash,
            ckbadger_store::TxCyclesWrite::ReplayFailed {
                block_hash,
                failure: ckbadger_common::cycles::ReplayFailure::NoScriptGroups,
            },
        )
        .unwrap_or_else(|e| panic!("persist cycles at block {block} tx {index}: {e:#}"));
}

/// The cycles worker writes a transaction's lazy cycles record after its
/// block committed, outside every block's journal; the record is keyed by the
/// transaction's canonical slot, so it goes with its block when the block is
/// rolled back, and the records of the blocks kept stay.
#[tokio::test]
async fn lazy_cycles_records_of_rolled_back_blocks_go_with_their_blocks() {
    let chain = protocol_chain(0);
    let plan = single_block_plan(chain.len());
    let (tip, fork) = (16, 10);

    let forward = open_stores();
    sync_prefix(&forward, &chain, &plan, 0, fork).await;
    persist_cycles_records(&forward.domain, 1..=fork);

    let rolled = open_stores();
    sync_prefix(&rolled, &chain, &plan, 0, tip).await;
    persist_cycles_records(&rolled.domain, 1..=tip);
    let records_above_fork = (fork + 1..=tip)
        .flat_map(|block| rolled.domain.list_block_txs(block as i64).unwrap())
        .filter(|(_, entry)| !entry.is_cellbase)
        .count();
    assert!(
        records_above_fork > 0,
        "the fixture must have non-cellbase transactions above the fork"
    );

    reorg_to(&rolled.indexer, fork as i64).await.unwrap();
    compare_dumps(
        &dump_domain(&rolled.domain),
        &dump_domain(&forward.domain),
        &format!("rolled back {tip} -> {fork} with lazy cycles records"),
    )
    .unwrap_or_else(|report| panic!("{report}"));
}

/// Cycles computed for a transaction of a block that is then orphaned; the
/// competing block re-includes the transaction. Read through the API's reader
/// (`get_tx_group_cycles_record`), the transaction has no record (the
/// orphaned block's went with it); computed again, it reads back exactly.
#[tokio::test]
async fn a_transaction_re_included_after_a_reorg_has_no_cycles_record_until_recomputed() {
    let chain = transfer_chain(8);
    let plan = single_block_plan(chain.len());
    let stores = open_stores();
    sync_prefix(&stores, &chain, &plan, 0, 7).await;

    let orphaned = 7i64;
    let (index, entry) = stores
        .domain
        .list_block_txs(orphaned)
        .unwrap()
        .into_iter()
        .find(|(_, entry)| !entry.is_cellbase)
        .expect("block 7 has a non-cellbase transaction");
    persist_replay_failure(&stores.domain, orphaned, index, &entry.tx_hash);
    assert!(stores
        .domain
        .get_tx_group_cycles_record(&entry.tx_hash)
        .unwrap()
        .is_some());

    reorg_to(&stores.indexer, orphaned - 1).await.unwrap();
    // The competing block at 7: same parent, same transactions, another hash.
    let mut competing = chain[7].clone();
    let mut hash = block_hash(7);
    hash[31] ^= 0xff;
    competing.block.header.hash = hex32(hash);
    write_live_batch(&stores.indexer, vec![competing])
        .await
        .unwrap_or_else(|e| panic!("competing block 7: {e:#}"));
    let (re_included_block, re_included_index, _) = stores
        .domain
        .get_tx_by_hash(&entry.tx_hash)
        .unwrap()
        .expect("the transaction is canonical again");
    assert_eq!((re_included_block, re_included_index), (orphaned, index));

    assert_eq!(
        stores
            .domain
            .get_tx_group_cycles_record(&entry.tx_hash)
            .unwrap_or_else(|e| panic!("reading the re-included transaction's cycles: {e:#}")),
        None,
        "the orphaned block's record must not survive the reorg"
    );
    persist_replay_failure(&stores.domain, orphaned, index, &entry.tx_hash);
    match stores
        .domain
        .get_tx_group_cycles_record(&entry.tx_hash)
        .unwrap()
    {
        Some(ckbadger_common::cycles_task::TxGroupCyclesRecord::Failed {
            block_hash,
            failure,
        }) => {
            assert_eq!(block_hash, hash, "recomputed against the competing block");
            assert_eq!(
                failure,
                ckbadger_common::cycles::ReplayFailure::NoScriptGroups
            );
        }
        other => panic!("recomputed record: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// B-5c · the sync status's runtime part is not rolled back
// ---------------------------------------------------------------------------

/// The runtime part of the sync status (run start, last sync time, rate,
/// deep-fork state) belongs to the running process and is written outside
/// the journal: a rollback across a restart keeps what the restarted run
/// wrote, where the journal used to restore the previous run's fields with
/// the chain part. The chain part is the fork point's.
#[tokio::test]
async fn a_rollback_across_a_restart_keeps_the_running_processes_runtime_status() {
    let chain = transfer_chain(12);
    let plan = single_block_plan(chain.len());
    let mut stores = open_stores();
    sync_prefix(&stores, &chain, &plan, 0, 8).await;
    stores.indexer = restart(stores.domain.clone(), stores._append.clone(), NETWORK)
        .unwrap_or_else(|e| panic!("restart: {e:#}"));
    let restarted = stores.domain.get_sync_runtime().unwrap();
    assert_eq!(
        restarted.sync_started_block, 8,
        "the restarted run starts at 8"
    );
    sync_prefix(&stores, &chain, &plan, 9, 11).await;

    reorg_to(&stores.indexer, 7).await.unwrap();

    let runtime = stores.domain.get_sync_runtime().unwrap();
    assert_eq!(
        runtime.sync_started_block, 8,
        "the rollback must not restore the previous run's start block"
    );
    assert_eq!(runtime.sync_started_at, restarted.sync_started_at);
    let status = stores.domain.get_sync_status().unwrap();
    assert_eq!(status.tip_block_number, 7);
    assert_eq!(
        status.tip_block_hash,
        stores.domain.get_block_header(7).unwrap().unwrap().hash
    );
}

// ---------------------------------------------------------------------------
// B-5c · the edge-case chain: an epoch boundary, an uncle, a raw timestamp
// earlier than the parent's across a UTC+8 day boundary, a `.cell` sub-name
// ---------------------------------------------------------------------------

/// Epoch length of the edge-case chain: blocks 0..=5 are epoch 0, 6..=11
/// epoch 1, 12..=14 epoch 2.
const EDGE_EPOCH_LENGTH: u64 = 6;
/// The first block of epoch 1 (epoch index 0).
const EDGE_EPOCH_BOUNDARY: usize = 6;
/// The block that includes one uncle (a sibling of block 7).
const EDGE_UNCLE_BLOCK: usize = 8;
/// The first block of the next UTC+8 day (00:05) ...
const EDGE_NEXT_DAY: usize = 9;
/// ... and its child, whose raw timestamp (23:58) is earlier than its
/// parent's and on the previous UTC+8 day; its chain time is its parent's.
const EDGE_BACKWARD_BLOCK: usize = 10;
/// The parent `.cell` name's block and its sub-name's.
const EDGE_PARENT_NAME_BLOCK: usize = 3;
const EDGE_SUB_NAME_BLOCK: usize = 13;
const EDGE_TIP: usize = 14;
const EDGE_PARENT_LABEL: &str = "edge";
const EDGE_SUB_LABEL: &str = "sub.edge";
/// 2026-09-22 22:35 UTC+8: block n is at 22:35 + 10n minutes, so block 8 is
/// at 23:55 and block 9 at 00:05 of the next UTC+8 day.
const T_EDGE: u64 = T_EVENING - 25 * MINUTE;

/// The CKB epoch field of block `number`: number | index << 24 | length << 40.
fn edge_epoch(number: u64) -> String {
    let epoch = (EDGE_EPOCH_LENGTH << 40)
        | ((number % EDGE_EPOCH_LENGTH) << 24)
        | (number / EDGE_EPOCH_LENGTH);
    format!("0x{epoch:x}")
}

/// Plain transfers (as in `transfer_chain`: every block moves one owner's
/// cell to the next owner) over epochs of six blocks, with the four edge
/// cases inside the chain:
/// - an epoch boundary at 6 (and the next at 12);
/// - one uncle in block 8;
/// - block 10's raw timestamp is 7 minutes before block 9's and on the
///   previous UTC+8 day (block 9 opens the next day);
/// - the `.cell` name `edge` minted at 3 and its sub-name `sub.edge` at 13,
///   which writes the parent → child `identity_by_collection` row.
fn edge_case_chain() -> Vec<BlockResponseWithCycles> {
    let t = |n: u64| T_EDGE + n * 10 * MINUTE;
    let mut chain = ChainBuilder::new();
    // Outputs 0..=7 fund owners 1..=8; outputs 8 and 9 the two names.
    let mut genesis: Vec<(CellOutput, String)> =
        (1..=8u8).map(|owner| plain(10_000, owner)).collect();
    genesis.push(plain(FUND, 0x40));
    genesis.push(plain(FUND, 0x41));
    chain.genesis(t(0), genesis);
    let mut cells: Vec<([u8; 32], u32, u64)> = (0..8)
        .map(|i| (tx_hash(0xc000), i as u32, 10_000))
        .collect();
    for n in 1..=EDGE_TIP as u64 {
        let from = (n as usize) % 8;
        let to = (from + 1) % 8;
        let (prev_tx, prev_index, capacity) = cells[from];
        let tag = 0x100 + n as u16;
        let moved = capacity - 1;
        let mut txs = vec![tx(
            tag,
            vec![input(prev_tx, prev_index)],
            vec![
                plain(moved / 2, to as u8 + 1),
                plain(moved - moved / 2, from as u8 + 1),
            ],
            vec![],
        )];
        cells[from] = (tx_hash(tag), 1, moved - moved / 2);
        let name = match n as usize {
            EDGE_PARENT_NAME_BLOCK => Some((0x3001, 8, EDGE_PARENT_LABEL, [0x0a; 20], 0x40)),
            EDGE_SUB_NAME_BLOCK => Some((0x3002, 9, EDGE_SUB_LABEL, [0x0b; 20], 0x41)),
            _ => None,
        };
        if let Some((tag, funding_index, label, owner, change_owner)) = name {
            txs.push(tx(
                tag,
                vec![funding(funding_index)],
                vec![
                    dotcell_name(300, label, owner),
                    plain(FUND - 301, change_owner),
                ],
                vec![witness_with_records(&[0, 0]), "0x".to_string()],
            ));
        }
        let timestamp = if n as usize == EDGE_BACKWARD_BLOCK {
            t(EDGE_NEXT_DAY as u64) - 7 * MINUTE
        } else {
            t(n)
        };
        chain.push(timestamp, txs);
    }
    for (number, block) in chain.blocks.iter_mut().enumerate() {
        block.block.header.epoch = edge_epoch(number as u64);
    }
    // The uncle: a block 7 that lost to the canonical one.
    let sibling = EDGE_UNCLE_BLOCK as u64 - 1;
    let mut uncle = header(sibling, t(sibling) + MINUTE);
    uncle.epoch = edge_epoch(sibling);
    let mut uncle_hash = block_hash(sibling);
    uncle_hash[31] ^= 0xff;
    uncle.hash = hex32(uncle_hash);
    chain.blocks[EDGE_UNCLE_BLOCK]
        .block
        .uncles
        .push(UncleBlockView {
            header: uncle,
            proposals: vec![],
        });
    chain.blocks
}

/// The `identity_by_collection` row listing `sub.edge` under `edge`.
fn edge_sub_name_row() -> Vec<u8> {
    keys::encode_identity_by_collection_key(
        &DotCellParser::derive_id(EDGE_PARENT_LABEL),
        &DotCellParser::derive_id(EDGE_SUB_LABEL),
    )
}

fn has_edge_sub_name_row(store: &CkbadgerStore) -> bool {
    store
        .get_cf(
            store.cf(ckbadger_store::store::CF_IDENTITY_BY_COLLECTION),
            &edge_sub_name_row(),
        )
        .unwrap()
        .is_some()
}

/// The fork points: just below and just above each edge case.
const EDGE_FORKS: &[usize] = &[
    EDGE_EPOCH_BOUNDARY - 1,
    EDGE_EPOCH_BOUNDARY,
    EDGE_UNCLE_BLOCK - 1,
    EDGE_UNCLE_BLOCK,
    EDGE_BACKWARD_BLOCK - 1,
    EDGE_BACKWARD_BLOCK,
    EDGE_SUB_NAME_BLOCK - 1,
    EDGE_SUB_NAME_BLOCK,
];

/// Batches 0, 1-4, 5-8, 9-12, 13-14: the epoch boundary and the uncle block
/// are inside a batch (the boundary's previous epoch start comes from the
/// batch), the backward block's parent is in its batch (the running maximum
/// inside the batch), the sub-name opens a batch; every fork but 12 lands
/// inside a batch.
fn edge_multi_block_plan() -> Vec<RangeInclusive<usize>> {
    vec![0..=0, 1..=4, 5..=8, 9..=12, 13..=14]
}

/// Each edge case is really in the store the chain builds: two complete
/// epochs and the open third, one uncle at block 8, the backward block's
/// chain time held at its parent's across the day boundary, and the parent →
/// child row of the sub-name, gone again after a rollback below its block.
#[tokio::test]
async fn edge_case_chain_exercises_each_edge_case() {
    let chain = edge_case_chain();
    assert_eq!(chain.len(), EDGE_TIP + 1);
    let stores = open_stores();
    sync_prefix(
        &stores,
        &chain,
        &single_block_plan(chain.len()),
        0,
        EDGE_TIP,
    )
    .await;
    let store = &stores.domain;
    let header_at = |block: usize| store.get_block_header(block as i64).unwrap().unwrap();

    // (a) The epoch boundary.
    let boundary = header_at(EDGE_EPOCH_BOUNDARY);
    assert_eq!((boundary.epoch_number, boundary.epoch_index), (1, 0));
    let before = header_at(EDGE_EPOCH_BOUNDARY - 1);
    assert_eq!((before.epoch_number, before.epoch_index), (0, 5));
    for (epoch, start_block, blocks, complete) in
        [(0, 0, 6, true), (1, 6, 6, true), (2, 12, 3, false)]
    {
        let stats = store
            .get_epoch_stats(epoch)
            .unwrap()
            .unwrap_or_else(|| panic!("epoch {epoch} has no stats row at the tip"));
        assert_eq!(
            (stats.start_block, stats.blocks_count, stats.length),
            (start_block, blocks, EDGE_EPOCH_LENGTH as i32),
            "epoch {epoch}"
        );
        assert_eq!(stats.end_timestamp.is_some(), complete, "epoch {epoch}");
    }

    // (b) The uncle.
    assert_eq!(header_at(EDGE_UNCLE_BLOCK).uncles_count, 1);
    assert_eq!(header_at(EDGE_UNCLE_BLOCK - 1).uncles_count, 0);

    // (c) The raw timestamp earlier than the parent's, across the day.
    let parent = header_at(EDGE_NEXT_DAY);
    let backward = header_at(EDGE_BACKWARD_BLOCK);
    assert!(backward.timestamp < parent.timestamp);
    assert_eq!(parent.chain_time_ms, parent.timestamp);
    assert_eq!(backward.chain_time_ms, parent.chain_time_ms);
    let day = |ms: i64| ckbadger_common::stats_bucket::stats_day(ms).unwrap();
    assert_eq!(
        day(header_at(EDGE_NEXT_DAY - 1).chain_time_ms).succ_opt(),
        Some(day(parent.chain_time_ms)),
        "block {EDGE_NEXT_DAY} opens the next UTC+8 day"
    );
    assert_eq!(
        day(backward.timestamp).succ_opt(),
        Some(day(backward.chain_time_ms)),
        "the backward block's raw time is on the previous UTC+8 day"
    );

    // (d) The sub-name's parent → child row, gone below its block.
    assert!(store
        .get_identity(&DotCellParser::derive_id(EDGE_PARENT_LABEL))
        .unwrap()
        .is_some());
    assert!(
        has_edge_sub_name_row(store),
        "no parent → child row at the tip"
    );
    reorg_to(&stores.indexer, EDGE_SUB_NAME_BLOCK as i64 - 1)
        .await
        .unwrap();
    assert!(
        !has_edge_sub_name_row(store),
        "the parent → child row survives a rollback below the sub-name's block"
    );
    assert!(store
        .get_identity(&DotCellParser::derive_id(EDGE_PARENT_LABEL))
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn edge_case_chain_single_block_batches_roll_back_byte_equal() {
    let chain = edge_case_chain();
    let plan = single_block_plan(chain.len());
    assert_rollback_properties(&chain, &plan, EDGE_TIP, EDGE_FORKS).await;
}

#[tokio::test]
async fn edge_case_chain_forks_inside_multi_block_batches_roll_back_byte_equal() {
    let chain = edge_case_chain();
    assert_rollback_properties(&chain, &edge_multi_block_plan(), EDGE_TIP, EDGE_FORKS).await;
}

#[tokio::test]
async fn journal_restores_covered_cfs_of_the_edge_case_chain() {
    let chain = edge_case_chain();
    assert_journal_properties(
        &chain,
        &single_block_plan(chain.len()),
        EDGE_TIP,
        EDGE_FORKS,
    )
    .await;
    assert_journal_properties(&chain, &edge_multi_block_plan(), EDGE_TIP, EDGE_FORKS).await;
}

// ---------------------------------------------------------------------------
// B-5c · a reset derived aggregate derives again to the fork's rows
// ---------------------------------------------------------------------------

/// The rows of the three derived aggregates' CFs.
fn aggregate_rows(dump: &Dump) -> Dump {
    let cfs: Vec<&str> = ckbadger_indexer::derive::registry()
        .iter()
        .map(|aggregate| aggregate.id().cf_name)
        .collect();
    dump.iter()
        .filter(|((cf, _), _)| cfs.contains(cf))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Derive every aggregate to completion (`derive_to_completion`, the
/// scheduler's own loop); every derivation must end `Complete`.
fn derive_complete(
    store: &CkbadgerStore,
    context: &str,
) -> Result<Vec<(&'static str, DeriveState)>, String> {
    let states = ckbadger_indexer::derive::derive_to_completion(store)
        .map_err(|e| format!("{context}: derive to completion: {e:#}"))?;
    if let Some((name, state)) = states
        .iter()
        .find(|(_, state)| !matches!(state, DeriveState::Complete { .. }))
    {
        return Err(format!(
            "{context}: {name} is not Complete after derivation: {state:?}"
        ));
    }
    Ok(states)
}

/// A rollback that resets the derived aggregates leaves them to the
/// scheduler. Derived again on the rolled-back store, the three aggregates'
/// rows must be byte-equal to a derivation on a store that synced only to the
/// fork (with the same batch boundaries, never derived before), and every
/// derivation must be `Complete` on both, at the fork, over the same rows.
/// Returns the aggregate CFs that hold rows at the fork.
async fn rederive_after_reset_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
    derive: Derive,
) -> Result<Vec<&'static str>, String> {
    let context = format!("re-derived after rollback {tip} -> {fork}");
    let rolled = open_stores();
    sync_with_derive(&rolled, chain, plan, tip, Some(derive)).await;
    reorg_to(&rolled.indexer, fork as i64)
        .await
        .map_err(|e| format!("rollback {tip} -> {fork}: {e:#}"))?;
    assert_aggregates_reset(&rolled.domain).map_err(|e| format!("{context}: {e}"))?;

    let reference = open_stores();
    sync_prefix(&reference, chain, plan, 0, fork).await;

    let rolled_states = derive_complete(&rolled.domain, &context)?;
    let reference_states = derive_complete(&reference.domain, "reference")?;
    for (name, state) in &rolled_states {
        if !matches!(
            state,
            DeriveState::Complete { derived_from_tip, completed_at_tip, .. }
                if *derived_from_tip == fork as i64 && *completed_at_tip == fork as i64
        ) {
            return Err(format!(
                "{context}: {name} is not complete at the fork: {state:?}"
            ));
        }
    }
    if rolled_states != reference_states {
        return Err(format!(
            "{context}: derive states differ from the reference's:\n  rolled    {rolled_states:?}\n  reference {reference_states:?}"
        ));
    }

    let rolled_rows = aggregate_rows(&dump_domain(&rolled.domain));
    if rolled_rows.is_empty() {
        return Err(format!(
            "{context}: no derived aggregate holds a row at the fork; the case exercises nothing"
        ));
    }
    compare_dumps(
        &rolled_rows,
        &aggregate_rows(&dump_domain(&reference.domain)),
        &context,
    )?;
    Ok(ckbadger_indexer::derive::registry()
        .iter()
        .map(|aggregate| aggregate.id().cf_name)
        .filter(|cf| rolled_rows.keys().any(|(row_cf, _)| row_cf == cf))
        .collect())
}

async fn assert_rederive_after_reset(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    forks: &[usize],
    derive: Derive,
) {
    let mut failures = Vec::new();
    let mut exercised: Vec<&'static str> = Vec::new();
    for &fork in forks {
        match rederive_after_reset_property(chain, plan, tip, fork, derive).await {
            Ok(cfs) => exercised.extend(cfs),
            Err(report) => failures.push(report),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    for aggregate in ckbadger_indexer::derive::registry() {
        let cf = aggregate.id().cf_name;
        assert!(
            exercised.contains(&cf),
            "{cf} holds no row at any fork of {forks:?}; the case does not exercise it"
        );
    }
}

/// Completed at 9: a rollback below 9 resets the aggregates (the
/// `derived_aggregates_complete_above_the_target_are_reset` case and forks
/// just below the completion and near genesis), and deriving them again
/// gives the fork's rows.
#[tokio::test]
async fn derived_aggregates_reset_below_their_completion_derive_again_to_the_forks_rows() {
    let chain = protocol_chain(0);
    assert_rederive_after_reset(
        &chain,
        &chunked_plan(chain.len(), 3),
        16,
        &[8, 5, 2],
        Derive::CompleteAt(9),
    )
    .await;
    assert_rederive_after_reset(
        &chain,
        &single_block_plan(chain.len()),
        16,
        &[5],
        Derive::CompleteAt(9),
    )
    .await;
}

/// Running since 8: every rollback resets the aggregates (the
/// `derived_aggregates_running_are_reset` case, a fork at the block the
/// derivation started at and one below it), and deriving them again gives
/// the fork's rows.
#[tokio::test]
async fn derived_aggregates_reset_while_running_derive_again_to_the_forks_rows() {
    let chain = protocol_chain(0);
    assert_rederive_after_reset(
        &chain,
        &single_block_plan(chain.len()),
        16,
        &[12, 8, 5],
        Derive::RunningAt(8),
    )
    .await;
}

// ---------------------------------------------------------------------------
// B-5c · a crash around the rollback core
// ---------------------------------------------------------------------------

/// Stores synced along `plan` to `upto` by a run that imported the labels
/// before its first batch, as every start does (`entry.rs`).
async fn synced_with_labels(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    upto: usize,
    derive: Option<Derive>,
) -> Stores {
    let stores = open_stores();
    startup_label_import(&stores.domain, NETWORK)
        .unwrap_or_else(|e| panic!("startup label import: {e:#}"));
    sync_with_derive(&stores, chain, plan, upto, derive).await;
    stores
}

/// The CFs of the rows in which two dumps differ.
fn differing_cfs(actual: &Dump, expected: &Dump) -> Vec<&'static str> {
    let mut cfs: Vec<&'static str> = actual
        .iter()
        .filter(|(key, value)| expected.get(*key) != Some(*value))
        .map(|((cf, _), _)| *cf)
        .chain(
            expected
                .keys()
                .filter(|key| !actual.contains_key(*key))
                .map(|(cf, _)| *cf),
        )
        .collect();
    cfs.sort_unstable();
    cfs.dedup();
    cfs
}

/// The process dies right after the rollback core's one write
/// (`rollback_to_block`), before the DAO tip summary the rollback runs after
/// it. The store then holds the fork's chain state with the old tip's
/// summary, and nothing else stale; the restart's startup steps
/// (`live_test_support::restart`) bring the whole domain store to the store
/// that synced only to the fork.
async fn crash_after_the_core_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
) -> Result<(), String> {
    let mut crashed = synced_with_labels(chain, plan, tip, None).await;
    crashed
        .domain
        .rollback_to_block(fork as i64)
        .map_err(|e| format!("rollback core {tip} -> {fork}: {e:#}"))?;
    let reference = synced_with_labels(chain, plan, fork, None).await;
    let expected = dump_domain(&reference.domain);

    let stale = differing_cfs(&dump_domain(&crashed.domain), &expected);
    if stale != [ckbadger_store::CF_STATS_DAO] {
        return Err(format!(
            "crash after the core {tip} -> {fork}: before the restart the store must differ \
             from the fork's in the DAO tip summary (stats_dao) only, differs in {stale:?}"
        ));
    }

    crashed.indexer = restart(crashed.domain.clone(), crashed._append.clone(), NETWORK)
        .map_err(|e| format!("restart after the core {tip} -> {fork}: {e:#}"))?;
    compare_dumps(
        &dump_domain(&crashed.domain),
        &expected,
        &format!("crash after the core {tip} -> {fork}, then restart"),
    )
}

/// The process dies after the steps the rollback commits before its core —
/// the derived-aggregate reset (`derive::invalidate_for_rollback`; with no
/// pending reorg reconciliation, `begin_rollback` writes nothing) — and the
/// reorg is retried after the restart. The store must equal one whose reorg
/// ran once without a crash, aggregates reset in both.
async fn crash_before_the_core_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
    derive: Derive,
) -> Result<(), String> {
    let context = format!("crash before the core {tip} -> {fork}");
    let mut crashed = synced_with_labels(chain, plan, tip, Some(derive)).await;
    ckbadger_indexer::derive::invalidate_for_rollback(&crashed.domain, fork as i64)
        .map_err(|e| format!("{context}: invalidate: {e:#}"))?;
    assert_aggregates_reset(&crashed.domain)
        .map_err(|e| format!("{context}: the pre-core step did not reset: {e}"))?;
    let crashed_tip = crashed.domain.get_sync_status().unwrap().tip_block_number;
    if crashed_tip != tip as i64 {
        return Err(format!(
            "{context}: the pre-core step moved the tip to {crashed_tip}"
        ));
    }

    crashed.indexer = restart(crashed.domain.clone(), crashed._append.clone(), NETWORK)
        .map_err(|e| format!("{context}: restart: {e:#}"))?;
    reorg_to(&crashed.indexer, fork as i64)
        .await
        .map_err(|e| format!("{context}: the retried reorg: {e:#}"))?;

    let once = synced_with_labels(chain, plan, tip, Some(derive)).await;
    reorg_to(&once.indexer, fork as i64)
        .await
        .map_err(|e| format!("rollback {tip} -> {fork} without a crash: {e:#}"))?;
    assert_aggregates_reset(&once.domain).map_err(|e| format!("{context}: {e}"))?;
    assert_aggregates_reset(&crashed.domain).map_err(|e| format!("{context}: {e}"))?;
    compare_dumps(
        &dump_domain(&crashed.domain),
        &dump_domain(&once.domain),
        &format!("{context}, restart and retried reorg vs one reorg"),
    )
}

#[tokio::test]
async fn a_crash_after_the_rollback_core_is_completed_by_the_restart() {
    let chain = protocol_chain(0);
    let mut failures = Vec::new();
    for fork in [14, 10, 6, 5, 2] {
        if let Err(report) =
            crash_after_the_core_property(&chain, &single_block_plan(chain.len()), 16, fork).await
        {
            failures.push(report);
        }
    }
    let mut plan: Vec<RangeInclusive<usize>> = single_block_plan(1);
    plan.extend(
        chunked_plan(16, 4)
            .into_iter()
            .map(|r| r.start() + 1..=r.end() + 1),
    );
    for fork in [14, 10, 6, 2] {
        if let Err(report) = crash_after_the_core_property(&chain, &plan, 16, fork).await {
            failures.push(report);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[tokio::test]
async fn a_crash_before_the_rollback_core_is_retried_after_the_restart() {
    let chain = protocol_chain(0);
    let mut failures = Vec::new();
    for (plan, fork) in [
        (chunked_plan(chain.len(), 3), 5),
        (chunked_plan(chain.len(), 3), 2),
        (single_block_plan(chain.len()), 8),
    ] {
        if let Err(report) =
            crash_before_the_core_property(&chain, &plan, 16, fork, Derive::CompleteAt(9)).await
        {
            failures.push(report);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
