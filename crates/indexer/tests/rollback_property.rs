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
    TransactionView,
};
use ckbadger_indexer::sync::live_test_support::{indexer_with_stores, reorg_to, write_live_batch};
use ckbadger_indexer::sync::Indexer;
use ckbadger_store::derive_ops::{DeriveMeta, DeriveState};
use ckbadger_store::keys::{self, sync_meta_keys};
use ckbadger_store::types::SyncStatus;
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

/// `SyncStatus` fields that are wall-clock telemetry, zeroed before
/// comparison; every chain field (tip, totals, bulk completion, deep fork)
/// is compared byte for byte.
fn normalize_sync_status(value: &[u8]) -> Vec<u8> {
    let mut status: SyncStatus = bincode::deserialize(value).expect("decode sync_status");
    status.last_synced_at = 0;
    status.sync_started_at = None;
    status.sync_ema_rate = None;
    bincode::serialize(&status).expect("encode sync_status")
}

/// The HODL / cell-distribution tracker blobs serialise maps as vectors in
/// hash-map iteration order; sort them so two stores holding the same tracker
/// compare equal.
fn normalize_tracker(key: &[u8], value: &[u8]) -> Vec<u8> {
    if key == sync_meta_keys::HODL_TRACKER {
        let mut state: ckbadger_store::types::HodlTrackerState =
            bincode::deserialize(value).expect("decode hodl_tracker");
        state.capacity_by_date.sort();
        bincode::serialize(&state).expect("encode hodl_tracker")
    } else if key == sync_meta_keys::CELL_DIST_TRACKER {
        let mut state: ckbadger_store::types::CellDistributionTrackerState =
            bincode::deserialize(value).expect("decode cell_dist_tracker");
        state.cohort_accum.sort();
        bincode::serialize(&state).expect("encode cell_dist_tracker")
    } else {
        value.to_vec()
    }
}

/// A derivation's start / finish wall-clock times are telemetry; its state,
/// version and run counter are compared.
fn normalize_derive_meta(value: &[u8]) -> Vec<u8> {
    let mut meta: DeriveMeta = bincode::deserialize(value).expect("decode derive meta");
    meta.started_at_ms = None;
    meta.finished_at_ms = None;
    bincode::serialize(&meta).expect("encode derive meta")
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
                undo.entry(key[..8].to_vec()).or_default().push(value);
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
                if key.as_ref() == sync_meta_keys::SYNC_STATUS {
                    value = normalize_sync_status(&value);
                }
                if key.starts_with(sync_meta_keys::DERIVE_META_PREFIX) {
                    value = normalize_derive_meta(&value);
                }
                value = normalize_tracker(&key, &value);
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
                "  changed  {}\n           rolled back 0x{}\n           expected    0x{}",
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
            "{context}: {} row(s) differ from the store that never saw the rolled-back blocks:\n{}",
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

fn open_stores() -> Stores {
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
    let indexer = indexer_with_stores(domain.clone(), append.clone(), NETWORK, -1, None);
    Stores {
        domain,
        _append: append,
        indexer,
        _dirs: (domain_dir, append_dir),
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
];

fn is_covered(cf: &str, key: &[u8]) -> bool {
    JOURNAL_COVERED.iter().any(|(covered, prefix)| {
        *covered == cf && prefix.is_none_or(|prefix| key.first() == Some(&prefix))
    })
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
        if !ckbadger_store::keys::UndoSeqScope::Journal.owns(seq) {
            continue;
        }
        match bincode::deserialize::<ckbadger_store::types::UndoLogEntry>(&value)
            .expect("decode a journal row")
        {
            ckbadger_store::types::UndoLogEntry::KeyMutation {
                cf_name,
                key,
                previous_value,
                ..
            } => journal
                .entry(block)
                .or_default()
                .push((seq, cf_name, key, previous_value)),
            other => panic!("journal row at block {block} is not a key mutation: {other:?}"),
        }
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

fn only_covered(mut dump: Dump) -> Dump {
    dump.retain(|(cf, key), _| is_covered(cf, key));
    dump
}

/// The journal alone restores every covered CF to the store that never saw
/// the rolled-back blocks.
async fn journal_property(
    chain: &[BlockResponseWithCycles],
    plan: &[RangeInclusive<usize>],
    tip: usize,
    fork: usize,
) -> Result<(), String> {
    let stores = open_stores();
    sync_prefix(&stores, chain, plan, 0, tip).await;
    let at_tip = dump_domain(&stores.domain);
    let journal = read_journal(&stores.domain);
    let simulated = simulate_journal_rollback(&at_tip, &journal, tip, fork);
    let reference = open_stores();
    sync_prefix(&reference, chain, plan, 0, fork).await;
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
    let mut failures = Vec::new();
    for &fork in forks {
        if let Err(report) = journal_property(chain, plan, tip, fork).await {
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

#[tokio::test]
async fn journal_restores_covered_cfs_36_blocks_deep() {
    let chain = protocol_chain(30);
    let tip = chain.len() - 1;
    let plan = chunked_plan(chain.len(), 7);
    assert_journal_properties(&chain, &plan, tip, &[tip - 36]).await;
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
