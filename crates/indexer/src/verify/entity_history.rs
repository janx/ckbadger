//! Chain-derived expectation for one entity's capacity history.
//!
//! This is the verifier's independent oracle. It takes an entity selector and a
//! qualified CKB node, enumerates every input and output that ever touched that
//! entity between genesis and the anchor, and recomputes the daily capacity and
//! occupied-capacity deltas from the cells themselves.
//!
//! Independence is the property that makes it worth anything, so:
//!
//! * it never calls the production writer, parser or `PROTOCOL_REGISTRY` — the
//!   selector plus the chain records fully determine the expectation, which is
//!   why a code hash the registry has never seen produces the same numbers;
//! * it reuses only shared primitives: `ckb_types` for the script hash,
//!   `ckbadger_common::dao::occupied_capacity_shannons` for the one definition
//!   of occupied capacity, and `ckbadger_common::block_date_from_ms` for the
//!   one definition of the UTC+8 day boundary;
//! * arithmetic is checked `i128` throughout, compared with zero tolerance.
//!   `--tolerance` belongs to the explorer comparisons and is not inherited
//!   here: one shannon is a failure.
//!
//! The three facets — per-day deltas, running prefix totals, and the current
//! live value — share this one scan. They are diagnostics on the same
//! invariant, so a report says which of them differed and where.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::{anyhow, Context as _};

use super::checks::{
    Check, CheckContext, CheckResult, CheckTier, EntitySelector, Finding, ProgressReporter,
};
use super::manifest::{EntityCoverage, VerifyManifest};
use super::source::{
    collect_transactions, qualify_source, reverify_anchor, RunBudget, SourceAnchor,
    SourceDeclaration, SourceQualification,
};
use crate::rpc::{CkbRpcClient, IndexerIoType, IndexerSearchKey, IndexerTxRecord, Script};

/// Initial independent budget (V3). Not a proven default: it is measured and
/// revised per network, and exhausting it is `Inconclusive`, never a pass.
pub const MAX_ENTITIES_PER_RUN: usize = 16;
const MAX_HISTORY_RECORDS: usize = 200_000;
const MAX_RPC_REQUESTS: usize = 10_000;
const MAX_WALL_SECONDS: u64 = 600;
const PAGE_LIMIT: u32 = 1_000;
const MAX_PAGES: usize = 10_000;

/// Selectors for entities known to have been damaged by the shallow-fork
/// rollback this verification exists to catch. Always in the default candidate
/// set so a regression cannot quietly stop being sampled.
fn incident_selectors(network: &str) -> Vec<EntitySelector> {
    let ids: &[&str] = match network {
        "testnet" => &["0xd485c2271949c232e3f5d46128336c716f90bcbf3cb278696083689fbbcd407a"],
        "mainnet" => &[
            "0x3390b8cb174b5623fd72a2dc5af13ea428ff171573f500b0e796e1f7336bcabe",
            "0x17e3a05e0a33dd00eb601ee4f0115039168930a5ce98c5455e5be456e2ba2de8",
        ],
        _ => &[],
    };
    ids.iter()
        .map(|id| EntitySelector {
            kind: "token".to_string(),
            id: (*id).to_string(),
        })
        .collect()
}

/// A capacity/occupied pair in shannons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeltaPair {
    pub capacity: i128,
    pub occupied: i128,
}

impl DeltaPair {
    fn add(&mut self, capacity: i128, occupied: i128) -> anyhow::Result<()> {
        self.capacity = self
            .capacity
            .checked_add(capacity)
            .ok_or_else(|| anyhow!("capacity accumulation overflowed i128"))?;
        self.occupied = self
            .occupied
            .checked_add(occupied)
            .ok_or_else(|| anyhow!("occupied accumulation overflowed i128"))?;
        Ok(())
    }
}

/// What the chain says one entity's history is.
#[derive(Debug, Clone, Default)]
pub struct TokenHistoryExpectation {
    /// Net change per UTC+8 day.
    pub daily: BTreeMap<u32, DeltaPair>,
    /// Cumulative totals from genesis through each day that has a delta.
    pub prefix: BTreeMap<u32, DeltaPair>,
    /// Live totals at the anchor.
    pub current: DeltaPair,
    pub records: usize,
}

impl TokenHistoryExpectation {
    fn finish(&mut self) -> anyhow::Result<()> {
        let mut running = DeltaPair::default();
        for (date, delta) in &self.daily {
            running.add(delta.capacity, delta.occupied)?;
            self.prefix.insert(*date, running);
        }
        self.current = running;
        Ok(())
    }
}

/// Which of the three diagnostics differed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Daily,
    Prefix,
    Current,
}

impl Facet {
    fn as_str(self) -> &'static str {
        match self {
            Facet::Daily => "daily",
            Facet::Prefix => "prefix",
            Facet::Current => "current",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FacetDifference {
    pub facet: Facet,
    /// `capacity` or `knowledge` (the occupied-capacity component).
    pub component: &'static str,
    pub date: Option<u32>,
    pub expected: i128,
    pub actual: i128,
}

impl FacetDifference {
    fn render(&self) -> String {
        let where_ = match self.date {
            Some(date) => format!("{} {}", self.facet.as_str(), date),
            None => self.facet.as_str().to_string(),
        };
        format!(
            "{where_} {}: expected {} but the index has {} (diff {})",
            self.component,
            self.expected,
            self.actual,
            self.actual - self.expected
        )
    }
}

// ---------------------------------------------------------------------------
// The typed export, as this crate consumes it.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportAnchor {
    pub block_number: u64,
    pub block_hash: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDailyRow {
    pub date: u32,
    pub capacity_delta: String,
    pub knowledge_delta: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEntity {
    pub kind: String,
    pub id: String,
    pub present: Option<bool>,
    pub row_count: Option<u64>,
    pub complete: bool,
    pub daily: Vec<ExportDailyRow>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityStatisticsExport {
    pub anchor: ExportAnchor,
    pub complete: bool,
    pub entities: Vec<ExportedEntity>,
}

impl ExportedEntity {
    /// Parse the wire strings into checked `i128`. A value that is not an exact
    /// decimal integer is an error, never a coerced zero.
    fn daily_pairs(&self) -> anyhow::Result<BTreeMap<u32, DeltaPair>> {
        let mut rows = BTreeMap::new();
        for row in &self.daily {
            let capacity: i128 = row.capacity_delta.parse().with_context(|| {
                format!(
                    "capacityDelta '{}' for {} on {} is not a decimal integer",
                    row.capacity_delta, self.id, row.date
                )
            })?;
            let occupied: i128 = row.knowledge_delta.parse().with_context(|| {
                format!(
                    "knowledgeDelta '{}' for {} on {} is not a decimal integer",
                    row.knowledge_delta, self.id, row.date
                )
            })?;
            if rows
                .insert(row.date, DeltaPair { capacity, occupied })
                .is_some()
            {
                anyhow::bail!("export listed {} twice for {}", row.date, self.id);
            }
        }
        Ok(rows)
    }
}

/// Compare the chain-derived expectation against the exported index rows.
///
/// A day the chain says is non-zero but the index omits is a difference against
/// zero; a non-zero row the chain does not have is a difference the other way.
/// A genuinely zero net day may legitimately have no row.
pub fn compare_token_history(
    expected: &TokenHistoryExpectation,
    exported: &ExportedEntity,
) -> anyhow::Result<Vec<FacetDifference>> {
    let actual_daily = exported.daily_pairs()?;
    let mut differences = Vec::new();

    let mut dates: Vec<u32> = expected.daily.keys().copied().collect();
    dates.extend(actual_daily.keys().copied());
    dates.sort_unstable();
    dates.dedup();

    for date in &dates {
        let want = expected.daily.get(date).copied().unwrap_or_default();
        let got = actual_daily.get(date).copied().unwrap_or_default();
        if want.capacity != got.capacity {
            differences.push(FacetDifference {
                facet: Facet::Daily,
                component: "capacity",
                date: Some(*date),
                expected: want.capacity,
                actual: got.capacity,
            });
        }
        if want.occupied != got.occupied {
            differences.push(FacetDifference {
                facet: Facet::Daily,
                component: "knowledge",
                date: Some(*date),
                expected: want.occupied,
                actual: got.occupied,
            });
        }
    }

    // Prefix totals over the union of dates, so a missing day shifts every
    // later cumulative value and is reported as such.
    let mut running = DeltaPair::default();
    for date in &dates {
        let got = actual_daily.get(date).copied().unwrap_or_default();
        running.add(got.capacity, got.occupied)?;
        let Some(want) = expected.prefix.get(date) else {
            continue;
        };
        if want.capacity != running.capacity {
            differences.push(FacetDifference {
                facet: Facet::Prefix,
                component: "capacity",
                date: Some(*date),
                expected: want.capacity,
                actual: running.capacity,
            });
        }
        if want.occupied != running.occupied {
            differences.push(FacetDifference {
                facet: Facet::Prefix,
                component: "knowledge",
                date: Some(*date),
                expected: want.occupied,
                actual: running.occupied,
            });
        }
    }

    if expected.current.capacity != running.capacity {
        differences.push(FacetDifference {
            facet: Facet::Current,
            component: "capacity",
            date: None,
            expected: expected.current.capacity,
            actual: running.capacity,
        });
    }
    if expected.current.occupied != running.occupied {
        differences.push(FacetDifference {
            facet: Facet::Current,
            component: "knowledge",
            date: None,
            expected: expected.current.occupied,
            actual: running.occupied,
        });
    }

    Ok(differences)
}

// ---------------------------------------------------------------------------
// Chain-side collection
// ---------------------------------------------------------------------------

/// Everything one entity's enumeration consumed and could not cover.
#[derive(Debug, Clone)]
pub struct TokenExpectationOutcome {
    pub expectation: TokenHistoryExpectation,
    pub complete: bool,
    pub uncovered: Vec<String>,
}

/// The run's chain-fact cache and budget.
///
/// One collector serves every entity in the run: a transaction or header
/// fetched for one entity is reused by the next, and every request they make
/// spends the same run-wide allowance. Per-entity collectors re-fetched shared
/// facts and silently multiplied the budget by the entity count.
pub(crate) struct Collector<'a> {
    client: &'a CkbRpcClient,
    txs: HashMap<String, crate::rpc::TransactionView>,
    headers: HashMap<u64, i64>,
    budget: &'a mut RunBudget,
}

impl<'a> Collector<'a> {
    pub(crate) fn new(client: &'a CkbRpcClient, budget: &'a mut RunBudget) -> Self {
        Self {
            client,
            txs: HashMap::new(),
            headers: HashMap::new(),
            budget,
        }
    }

    /// Fetch and cache a committed transaction. `None` means the node does not
    /// have it, which the caller reports as uncovered rather than as zero.
    async fn transaction(
        &mut self,
        hash: &str,
    ) -> anyhow::Result<Option<crate::rpc::TransactionView>> {
        if let Some(tx) = self.txs.get(hash) {
            return Ok(Some(tx.clone()));
        }
        self.budget.charge_request();
        let Some(with_status) = self.client.get_transaction(hash).await? else {
            return Ok(None);
        };
        let Some(tx) = with_status.transaction else {
            return Ok(None);
        };
        self.txs.insert(hash.to_string(), tx.clone());
        Ok(Some(tx))
    }

    /// Committed timestamp of a block, in milliseconds.
    async fn timestamp_ms(&mut self, block: u64) -> anyhow::Result<Option<i64>> {
        if let Some(ts) = self.headers.get(&block) {
            return Ok(Some(*ts));
        }
        self.budget.charge_request();
        let Some(header) = self.client.get_header_by_number(block).await? else {
            return Ok(None);
        };
        let raw = header.timestamp.trim_start_matches("0x");
        let ts = i64::from_str_radix(raw, 16).with_context(|| {
            format!(
                "block {block} header timestamp '{}' is not a hex integer",
                header.timestamp
            )
        })?;
        self.headers.insert(block, ts);
        Ok(Some(ts))
    }
}

/// Exact occupied capacity of one output, in shannons.
///
/// Reuses the single shared definition (`8 + 33 + lock args + 33 + type args +
/// data`) rather than restating it, so the verifier and the writer can never
/// drift on what "occupied" means.
fn output_occupied(output: &crate::rpc::CellOutput, data_hex: &str) -> anyhow::Result<i128> {
    let data_len = hex_byte_len(data_hex).context("output data")?;
    let lock_args_len = hex_byte_len(&output.lock.args).context("lock args")?;
    let type_args_len = match output.type_.as_ref() {
        Some(script) => Some(hex_byte_len(&script.args).context("type args")?),
        None => None,
    };
    let occupied =
        ckbadger_common::dao::occupied_capacity_shannons(data_len, lock_args_len, type_args_len)?;
    Ok(i128::from(occupied))
}

fn hex_byte_len(raw: &str) -> anyhow::Result<usize> {
    let stripped = raw.strip_prefix("0x").unwrap_or(raw);
    if !stripped.len().is_multiple_of(2) {
        anyhow::bail!("'{raw}' is not a whole number of bytes");
    }
    if !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("'{raw}' is not hex");
    }
    Ok(stripped.len() / 2)
}

fn parse_capacity_shannons(raw: &str) -> anyhow::Result<i128> {
    let stripped = raw.strip_prefix("0x").unwrap_or(raw);
    let value = u64::from_str_radix(stripped, 16)
        .with_context(|| format!("capacity '{raw}' is not a hex integer"))?;
    Ok(i128::from(value))
}

/// The UTC+8 day key a block's timestamp falls in.
fn date_key(timestamp_ms: i64) -> anyhow::Result<u32> {
    ckbadger_common::block_date_from_ms(timestamp_ms)
        .format("%Y%m%d")
        .to_string()
        .parse()
        .map_err(|e| anyhow!("timestamp {timestamp_ms} did not format into a date key: {e}"))
}

/// Collect the full `[0, anchor]` history of one token type script.
///
/// Takes the run's collector so this entity reuses what earlier entities
/// already fetched and spends the same run-wide budget.
pub(crate) async fn collect_token_expectation(
    collector: &mut Collector<'_>,
    type_script: &Script,
    anchor_height: u64,
) -> anyhow::Result<TokenExpectationOutcome> {
    // `block_range` is half-open, so `anchor + 1` includes the anchor.
    let range_end = anchor_height.checked_add(1).ok_or_else(|| {
        anyhow!("anchor height {anchor_height} has no successor: cannot build a block range")
    })?;
    let search_key = IndexerSearchKey::exact_type(type_script.clone(), Some((0, range_end)));

    let client = collector.client;
    let page =
        collect_transactions(client, &search_key, PAGE_LIMIT, MAX_PAGES, collector.budget).await?;

    let mut expectation = TokenHistoryExpectation::default();
    let mut uncovered: Vec<String> = Vec::new();
    let mut complete = page.complete;
    let mut seen: HashMap<(String, u8, u32), u64> = HashMap::new();

    for record in &page.records {
        if let Some(reason) = collector.budget.exhausted() {
            complete = false;
            uncovered.push(format!(
                "{reason}; stopped after folding in {} record(s) for this entity",
                expectation.records
            ));
            break;
        }
        if record.block_number > anchor_height {
            // Impossible under the half-open block_range: a node that ignores
            // the filter is returning records the export never saw, and
            // skipping them quietly would hide that.
            anyhow::bail!(
                "node indexer returned record {} at block {} above the requested range \
                 [0, {anchor_height}]: the block_range filter was not applied",
                record.tx_hash,
                record.block_number
            );
        }

        let identity = (
            record.tx_hash.clone(),
            match record.io_type {
                IndexerIoType::Input => 0u8,
                IndexerIoType::Output => 1u8,
            },
            record.io_index,
        );
        if let Some(previous) = seen.insert(identity.clone(), record.block_number) {
            if previous != record.block_number {
                anyhow::bail!(
                    "node indexer returned {}#{:?}[{}] at two different blocks ({previous} and {})",
                    record.tx_hash,
                    record.io_type,
                    record.io_index,
                    record.block_number
                );
            }
            // Same record twice at the same height: the enumeration is the
            // source's, and counting it twice would invent capacity.
            continue;
        }

        let Some(timestamp_ms) = collector.timestamp_ms(record.block_number).await? else {
            complete = false;
            uncovered.push(format!(
                "no header for block {} (record {})",
                record.block_number, record.tx_hash
            ));
            continue;
        };
        let date = date_key(timestamp_ms)?;

        match resolve_cell(collector, record).await? {
            Some((capacity, occupied)) => {
                let signed = match record.io_type {
                    IndexerIoType::Output => (capacity, occupied),
                    IndexerIoType::Input => (-capacity, -occupied),
                };
                expectation
                    .daily
                    .entry(date)
                    .or_default()
                    .add(signed.0, signed.1)?;
                expectation.records += 1;
            }
            None => {
                complete = false;
                uncovered.push(format!(
                    "could not resolve {:?} {}[{}] at block {}",
                    record.io_type, record.tx_hash, record.io_index, record.block_number
                ));
            }
        }
    }

    if !page.complete {
        // Say which limit stopped it: "incomplete" without a cause reads as a
        // short history rather than an exhausted allowance.
        let cause = collector
            .budget
            .exhausted()
            .unwrap_or_else(|| format!("page limit of {MAX_PAGES} reached"));
        uncovered.push(format!(
            "enumeration stopped after {} page(s) with {} record(s): {cause}",
            page.pages,
            page.records.len()
        ));
    }

    expectation.finish()?;
    Ok(TokenExpectationOutcome {
        expectation,
        complete,
        uncovered,
    })
}

/// Resolve the cell a history record refers to: the output itself, or the
/// previous output an input consumed.
async fn resolve_cell(
    collector: &mut Collector<'_>,
    record: &IndexerTxRecord,
) -> anyhow::Result<Option<(i128, i128)>> {
    let Some(tx) = collector.transaction(&record.tx_hash).await? else {
        return Ok(None);
    };

    let (owner_tx, index) = match record.io_type {
        IndexerIoType::Output => (tx, record.io_index as usize),
        IndexerIoType::Input => {
            let input = tx.inputs.get(record.io_index as usize).ok_or_else(|| {
                anyhow!(
                    "transaction {} has no input {}",
                    record.tx_hash,
                    record.io_index
                )
            })?;
            let previous = input.previous_output.clone();
            let prev_index = usize::try_from(
                u64::from_str_radix(previous.index.trim_start_matches("0x"), 16).with_context(
                    || format!("previous_output index '{}' is not hex", previous.index),
                )?,
            )?;
            let Some(prev_tx) = collector.transaction(&previous.tx_hash).await? else {
                return Ok(None);
            };
            (prev_tx, prev_index)
        }
    };

    let output = owner_tx
        .outputs
        .get(index)
        .ok_or_else(|| anyhow!("transaction {} has no output {index}", owner_tx.hash))?;
    let data = owner_tx
        .outputs_data
        .get(index)
        .ok_or_else(|| anyhow!("transaction {} has no outputs_data {index}", owner_tx.hash))?;

    Ok(Some((
        parse_capacity_shannons(&output.capacity)?,
        output_occupied(output, data)?,
    )))
}

// ---------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------

/// Compute the token type script's hash exactly as CKB does.
fn script_hash(script: &Script) -> anyhow::Result<String> {
    use ckb_types::prelude::*;

    let code: [u8; 32] = hex::decode(script.code_hash.trim_start_matches("0x"))
        .with_context(|| format!("code_hash '{}' is not hex", script.code_hash))?
        .try_into()
        .map_err(|_| anyhow!("code_hash '{}' is not 32 bytes", script.code_hash))?;
    let hash_type = match script.hash_type.as_str() {
        "data" => 0u8,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        other => anyhow::bail!("unknown hash_type '{other}'"),
    };
    let args = hex::decode(script.args.trim_start_matches("0x"))
        .with_context(|| format!("args '{}' is not hex", script.args))?;
    let packed = ckb_types::packed::Script::new_builder()
        .code_hash(code.pack())
        .hash_type(ckb_types::packed::Byte::new(hash_type))
        .args(args.pack())
        .build();
    let bytes: [u8; 32] = packed.calc_script_hash().unpack();
    Ok(format!("0x{}", hex::encode(bytes)))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenScriptResponse {
    type_code_hash: String,
    type_hash_type: String,
    type_args: String,
}

/// Resolve `token:<type_hash>` to its full type script.
///
/// The API supplies the candidate script, but the selector is only accepted
/// once the script hashes back to the requested id — so a wrong or stale script
/// cannot redirect the verification onto a different entity.
fn resolve_token_script(ctx: &CheckContext, type_hash: &str) -> anyhow::Result<Script> {
    let response: TokenScriptResponse =
        super::checks::api_get(ctx, &format!("tokens/{type_hash}"))?;
    let script = Script {
        code_hash: response.type_code_hash,
        hash_type: response.type_hash_type,
        args: response.type_args,
    };
    let computed = script_hash(&script)?;
    if !computed.eq_ignore_ascii_case(type_hash) {
        anyhow::bail!(
            "the script the index reports for {type_hash} hashes to {computed}: \
             the selector cannot be resolved to that entity"
        );
    }
    Ok(script)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenListEntry {
    type_script_hash: String,
}

/// Default candidates: the known incident selectors plus the head of the API's
/// token directory. The directory is a risk hint only — it can never prove its
/// own completeness, which is why it does not decide the verdict.
fn default_candidates(ctx: &CheckContext) -> Vec<EntitySelector> {
    let mut candidates = incident_selectors(ctx.network);
    let listed: Result<Vec<TokenListEntry>, _> = super::checks::api_get(ctx, "tokens?limit=8");
    if let Ok(entries) = listed {
        for entry in entries {
            if candidates.len() >= MAX_ENTITIES_PER_RUN {
                break;
            }
            let selector = EntitySelector {
                kind: "token".to_string(),
                id: entry.type_script_hash,
            };
            if !candidates.contains(&selector) {
                candidates.push(selector);
            }
        }
    }
    candidates
}

/// The chain-side phase: qualify the source, then enumerate each entity.
struct ChainWork {
    qualification: SourceQualification,
    /// One outcome per script, in the order given. Empty when the source did
    /// not qualify — an unqualified source must not produce numbers at all.
    outcomes: Vec<TokenExpectationOutcome>,
    /// Run-wide totals, so the manifest reports what the run actually spent
    /// rather than the sum of per-entity counters over a shared cache.
    rpc_requests: usize,
    history_records: usize,
}

async fn qualify_and_collect(
    client: &CkbRpcClient,
    declaration: Option<&SourceDeclaration>,
    declaration_path: &std::path::Path,
    anchor: &SourceAnchor,
    scripts: &[(EntitySelector, Script)],
    budget: &mut RunBudget,
) -> anyhow::Result<ChainWork> {
    budget.charge_request();
    let qualification = qualify_source(client, declaration, declaration_path, anchor).await?;
    if matches!(qualification, SourceQualification::Inconclusive(_)) {
        return Ok(ChainWork {
            qualification,
            outcomes: Vec::new(),
            rpc_requests: budget.rpc_requests,
            history_records: budget.records,
        });
    }

    let mut outcomes = Vec::with_capacity(scripts.len());
    {
        let mut collector = Collector::new(client, budget);
        for (_, script) in scripts {
            outcomes.push(
                collect_token_expectation(&mut collector, script, anchor.block_number).await?,
            );
        }
    }

    // The anchor was canonical when the walk started; a walk can take minutes,
    // and a reorg underneath it would have the node enumerating a different
    // chain than the export describes. Re-verifying it here is what keeps a
    // moved chain from being reported as a proven inconsistency.
    if let Some(reason) = reverify_anchor(client, anchor).await? {
        return Ok(ChainWork {
            qualification: SourceQualification::Inconclusive(reason),
            outcomes: Vec::new(),
            rpc_requests: budget.rpc_requests,
            history_records: budget.records,
        });
    }

    Ok(ChainWork {
        rpc_requests: budget.rpc_requests,
        history_records: budget.records,
        qualification,
        outcomes,
    })
}

/// Drive one future to completion on a thread of its own.
///
/// Checks run on a blocking thread that still carries the caller's tokio
/// context, and building or dropping a runtime there panics ("Cannot drop a
/// runtime in a context where blocking is not allowed"). A plain OS thread has
/// no such context, so the runtime's whole lifetime stays off the caller's.
fn run_on_dedicated_runtime<F, T>(future: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>> + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(future)
            })
            .join()
            .map_err(|_| anyhow!("the chain-history worker thread panicked"))?
    })
}

/// S25: one entity's full capacity history, recomputed from the chain.
pub struct EntityCapacityHistoryMatchesChain;

impl Check for EntityCapacityHistoryMatchesChain {
    fn name(&self) -> &'static str {
        "entity_capacity_history_matches_chain"
    }

    fn description(&self) -> &'static str {
        "Every daily capacity delta, prefix total and current value of an entity matches the chain"
    }

    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }

    fn requires_rpc(&self) -> bool {
        true
    }

    /// The selection is the entity list, not `--sample-count`: each selected
    /// entity is then verified exhaustively.
    fn requires_sampling(&self) -> bool {
        false
    }

    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let rpc_url = ctx
            .rpc_url
            .clone()
            .ok_or_else(|| anyhow!("entity history verification requires --rpc-url"))?;

        let selectors: Vec<EntitySelector> = if ctx.entities.is_empty() {
            default_candidates(ctx)
        } else {
            ctx.entities.clone()
        };
        if selectors.len() > MAX_ENTITIES_PER_RUN {
            anyhow::bail!(
                "{} entities selected, the independent budget covers at most {MAX_ENTITIES_PER_RUN}",
                selectors.len()
            );
        }
        let (supported, unsupported): (Vec<_>, Vec<_>) = selectors
            .into_iter()
            .partition(|selector| selector.kind == "token");
        if supported.is_empty() {
            return Ok(CheckResult::inconclusive(format!(
                "no token entity to verify; this delivery covers the token family only \
                 (requested: {})",
                unsupported
                    .iter()
                    .map(EntitySelector::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        // The export is one request per case, so its anchor and rows are one
        // pinned view.
        let export: EntityStatisticsExport = super::checks::api_post(
            ctx,
            "verify/entity-statistics",
            &serde_json::json!({
                "entities": supported
                    .iter()
                    .map(|s| serde_json::json!({"kind": s.kind, "id": s.id}))
                    .collect::<Vec<_>>()
            }),
        )?;

        let anchor = SourceAnchor {
            block_number: export.anchor.block_number,
            block_hash: export.anchor.block_hash.clone(),
        };

        if !export.complete {
            return Ok(CheckResult::inconclusive(format!(
                "the typed export at anchor {} is incomplete; a truncated export cannot be \
                 compared against a full history",
                anchor.block_number
            )));
        }

        let declaration_path = ctx
            .verify_source_path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("verify-source.toml"));
        let declaration = SourceDeclaration::load(&declaration_path)?;

        // Resolving each selector to its full type script is a blocking API
        // read, so it happens before the chain work moves to its own thread.
        let mut scripts: Vec<(EntitySelector, Script)> = Vec::with_capacity(supported.len());
        let mut inconclusive: Vec<String> = Vec::new();
        for selector in &supported {
            match export
                .entities
                .iter()
                .find(|entity| entity.id.eq_ignore_ascii_case(&selector.id))
            {
                None => inconclusive.push(format!("the export did not return {selector}")),
                Some(exported) if !exported.complete => inconclusive.push(format!(
                    "{selector}: the export of this entity is incomplete"
                )),
                Some(_) => {
                    scripts.push((selector.clone(), resolve_token_script(ctx, &selector.id)?))
                }
            }
        }

        let client = CkbRpcClient::new(rpc_url);
        let mut budget = RunBudget::new(
            MAX_HISTORY_RECORDS,
            MAX_RPC_REQUESTS,
            Duration::from_secs(MAX_WALL_SECONDS),
        );
        let work = run_on_dedicated_runtime(qualify_and_collect(
            &client,
            declaration.as_ref(),
            &declaration_path,
            &anchor,
            &scripts,
            &mut budget,
        ))?;

        // Publish what the expected values rest on, whether or not it
        // qualified: a run whose source was rejected must say so in its report.
        *ctx.source_profile
            .lock()
            .expect("verify source profile lock poisoned") = Some(work.qualification.to_report());

        let SourceQualification::Qualified(profile) = &work.qualification else {
            let SourceQualification::Inconclusive(reason) = &work.qualification else {
                unreachable!("qualification is one of two variants")
            };
            return Ok(CheckResult::inconclusive(format!(
                "history source not qualified: {reason}"
            )));
        };

        let mut manifest = VerifyManifest::new(
            ctx.network,
            anchor.block_number,
            &anchor.block_hash,
            work.qualification.to_report(),
        );
        manifest.budget_records = MAX_HISTORY_RECORDS;
        manifest.budget_rpc_requests = MAX_RPC_REQUESTS;
        manifest.budget_seconds = MAX_WALL_SECONDS;
        manifest.index_start_block = profile.index_start_block;
        // Run-wide, counted once: summing per-entity counters over a shared
        // cache would report requests that were never made.
        manifest.rpc_requests = work.rpc_requests;
        manifest.history_records = work.history_records;

        for selector in &supported {
            if let Some(reason) = inconclusive
                .iter()
                .find(|reason| reason.contains(&selector.to_string()))
            {
                manifest
                    .entities
                    .push(EntityCoverage::incomplete(selector, reason.clone()));
            }
        }

        let mut findings: Vec<Finding> = Vec::new();
        let mut checked = 0u64;

        for ((selector, _), outcome) in scripts.iter().zip(work.outcomes.iter()) {
            progress.set_message(&selector.to_string());
            let exported = export
                .entities
                .iter()
                .find(|entity| entity.id.eq_ignore_ascii_case(&selector.id))
                .expect("only exported entities reach the chain phase");

            if !outcome.complete {
                inconclusive.push(format!(
                    "{selector}: history not fully covered ({})",
                    outcome.uncovered.join("; ")
                ));
                manifest.entities.push(EntityCoverage::incomplete(
                    selector,
                    outcome.uncovered.join("; "),
                ));
                continue;
            }

            let differences = compare_token_history(&outcome.expectation, exported)?;
            checked += 1;
            manifest.entities.push(EntityCoverage::complete(
                selector,
                outcome.expectation.records,
                differences.len(),
            ));

            if !differences.is_empty() {
                // One entity is one failed item, however many days differ.
                let mut details: Vec<String> = vec![format!(
                    "anchor {}/{}, {} chain record(s), {} difference(s)",
                    anchor.block_number,
                    anchor.block_hash,
                    outcome.expectation.records,
                    differences.len()
                )];
                details.extend(differences.iter().take(64).map(FacetDifference::render));
                if differences.len() > 64 {
                    details.push(format!("… and {} more", differences.len() - 64));
                }
                findings.push(Finding {
                    entity: selector.to_string(),
                    details,
                });
            }
            progress.inc(1);
        }

        if let Some(dir) = ctx.evidence_dir.as_deref() {
            manifest.write(dir)?;
        }

        if !findings.is_empty() {
            let mut result = CheckResult::fail(checked, findings);
            if !inconclusive.is_empty() {
                result.detail = Some(format!("not covered: {}", inconclusive.join("; ")));
            }
            return Ok(result);
        }
        if checked == 0 {
            return Ok(CheckResult::inconclusive(format!(
                "no entity could be verified: {}",
                inconclusive.join("; ")
            )));
        }
        if !inconclusive.is_empty() {
            return Ok(CheckResult::inconclusive(format!(
                "{checked} entity/entities matched the chain, but {} could not be covered: {}",
                inconclusive.len(),
                inconclusive.join("; ")
            )));
        }
        Ok(CheckResult::pass_with_detail(
            checked,
            format!(
                "{checked} entity/entities verified exactly against the chain at anchor {}",
                anchor.block_number
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(capacity: i128, occupied: i128) -> DeltaPair {
        DeltaPair { capacity, occupied }
    }

    fn expectation(rows: &[(u32, i128, i128)]) -> TokenHistoryExpectation {
        let mut expectation = TokenHistoryExpectation::default();
        for (date, capacity, occupied) in rows {
            expectation.daily.insert(*date, pair(*capacity, *occupied));
        }
        expectation.finish().unwrap();
        expectation
    }

    fn exported(rows: &[(u32, i128, i128)]) -> ExportedEntity {
        ExportedEntity {
            kind: "token".to_string(),
            id: "0xaa".to_string(),
            present: Some(true),
            row_count: Some(rows.len() as u64),
            complete: true,
            daily: rows
                .iter()
                .map(|(date, capacity, occupied)| ExportDailyRow {
                    date: *date,
                    capacity_delta: capacity.to_string(),
                    knowledge_delta: occupied.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_exact_match_produces_no_difference() {
        let rows = [(20260101, 100, 50), (20260102, -40, -20)];
        let differences = compare_token_history(&expectation(&rows), &exported(&rows)).unwrap();
        assert!(differences.is_empty(), "{differences:?}");
    }

    #[test]
    fn one_shannon_is_a_difference() {
        let chain = [(20260101, 100, 50)];
        let index = [(20260101, 99, 50)];
        let differences = compare_token_history(&expectation(&chain), &exported(&index)).unwrap();
        assert!(differences
            .iter()
            .any(|d| d.facet == Facet::Daily && d.component == "capacity"));
        assert!(differences.iter().any(|d| d.facet == Facet::Current));
    }

    #[test]
    fn a_zero_net_day_may_legitimately_have_no_row() {
        let chain = [(20260101, 100, 50), (20260102, 0, 0)];
        let index = [(20260101, 100, 50)];
        let differences = compare_token_history(&expectation(&chain), &exported(&index)).unwrap();
        assert!(differences.is_empty(), "{differences:?}");
    }

    #[test]
    fn a_missing_non_zero_day_is_reported_against_zero() {
        let chain = [(20260101, 100, 50), (20260102, 7, 3)];
        let index = [(20260101, 100, 50)];
        let differences = compare_token_history(&expectation(&chain), &exported(&index)).unwrap();
        let daily = differences
            .iter()
            .find(|d| d.facet == Facet::Daily && d.date == Some(20260102))
            .expect("the deleted day must be named");
        assert_eq!(daily.expected, 7);
        assert_eq!(daily.actual, 0);
    }

    #[test]
    fn an_isolated_extra_row_the_chain_does_not_have_is_reported() {
        let chain = [(20260101, 100, 50)];
        let index = [(20260101, 100, 50), (20260103, 5, 5)];
        let differences = compare_token_history(&expectation(&chain), &exported(&index)).unwrap();
        assert!(differences
            .iter()
            .any(|d| d.facet == Facet::Daily && d.date == Some(20260103)));
    }

    #[test]
    fn offsetting_days_still_differ_on_the_daily_facet_only() {
        let chain = [(20260101, 100, 0), (20260102, 100, 0)];
        let index = [(20260101, 150, 0), (20260102, 50, 0)];
        let differences = compare_token_history(&expectation(&chain), &exported(&index)).unwrap();
        assert!(differences.iter().any(|d| d.facet == Facet::Daily));
        assert!(
            !differences.iter().any(|d| d.facet == Facet::Current),
            "the totals do agree; only the history is wrong: {differences:?}"
        );
    }

    #[test]
    fn a_non_decimal_delta_is_an_error_not_a_zero() {
        let mut entity = exported(&[(20260101, 1, 1)]);
        entity.daily[0].capacity_delta = "1.5".to_string();
        let error = compare_token_history(&expectation(&[(20260101, 1, 1)]), &entity).unwrap_err();
        assert!(error.to_string().contains("decimal integer"), "{error}");
    }

    #[test]
    fn occupied_capacity_follows_the_shared_definition() {
        let output = crate::rpc::CellOutput {
            capacity: "0x0".to_string(),
            lock: Script {
                code_hash: format!("0x{}", "00".repeat(32)),
                hash_type: "type".to_string(),
                args: format!("0x{}", "11".repeat(20)),
            },
            type_: Some(Script {
                code_hash: format!("0x{}", "00".repeat(32)),
                hash_type: "type".to_string(),
                args: format!("0x{}", "22".repeat(32)),
            }),
        };
        // 8 + (33 + 20) + (33 + 32) + 16 data bytes = 142 bytes.
        let occupied = output_occupied(&output, &format!("0x{}", "33".repeat(16))).unwrap();
        assert_eq!(occupied, 142 * 100_000_000);
    }

    /// blake2b over a hand-written molecule `Script` table — an encoding path
    /// independent of `ckb_types`' builder.
    fn molecule_script_hash(code_hash: &[u8; 32], hash_type: u8, args: &[u8]) -> String {
        const HEADER_SIZE: u32 = 4 + 3 * 4;
        let offset_code_hash = HEADER_SIZE;
        let offset_hash_type = offset_code_hash + 32;
        let offset_args = offset_hash_type + 1;
        let total_size = offset_args + 4 + args.len() as u32;

        let mut buf = Vec::with_capacity(total_size as usize);
        buf.extend_from_slice(&total_size.to_le_bytes());
        buf.extend_from_slice(&offset_code_hash.to_le_bytes());
        buf.extend_from_slice(&offset_hash_type.to_le_bytes());
        buf.extend_from_slice(&offset_args.to_le_bytes());
        buf.extend_from_slice(code_hash);
        buf.push(hash_type);
        buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
        buf.extend_from_slice(args);

        let mut hasher = ckb_hash::new_blake2b();
        hasher.update(&buf);
        let mut out = [0u8; 32];
        hasher.finalize(&mut out);
        format!("0x{}", hex::encode(out))
    }

    /// Cross-check rather than a golden constant: the selector resolution is
    /// only safe if this hash is CKB's, so it is checked against a second,
    /// independently written encoding.
    #[test]
    fn the_script_hash_agrees_with_an_independent_molecule_encoding() {
        let code: [u8; 32] = [0x9b; 32];
        let args = vec![0xab, 0xcd, 0xef];
        for (label, hash_type) in [("data", 0u8), ("type", 1), ("data1", 2), ("data2", 4)] {
            let script = Script {
                code_hash: format!("0x{}", hex::encode(code)),
                hash_type: label.to_string(),
                args: format!("0x{}", hex::encode(&args)),
            };
            assert_eq!(
                script_hash(&script).unwrap(),
                molecule_script_hash(&code, hash_type, &args),
                "hash_type {label}"
            );
        }
    }

    #[test]
    fn hash_type_is_part_of_the_script_identity() {
        let code = format!("0x{}", "9b".repeat(32));
        let as_data = script_hash(&Script {
            code_hash: code.clone(),
            hash_type: "data".to_string(),
            args: "0x".to_string(),
        })
        .unwrap();
        let as_type = script_hash(&Script {
            code_hash: code,
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        })
        .unwrap();
        assert_ne!(as_data, as_type);
    }

    #[test]
    fn an_unknown_hash_type_is_refused_rather_than_defaulted() {
        let error = script_hash(&Script {
            code_hash: format!("0x{}", "9b".repeat(32)),
            hash_type: "type2".to_string(),
            args: "0x".to_string(),
        })
        .unwrap_err();
        assert!(error.to_string().contains("type2"), "{error}");
    }

    // -----------------------------------------------------------------
    // Run-wide budget and shared chain facts
    // -----------------------------------------------------------------

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::method as http_method;
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    fn test_hash(seed: u64) -> String {
        format!("0x{seed:064x}")
    }

    /// One transaction creating two differently-typed token cells, so two
    /// entities' histories genuinely share a chain fact.
    struct SharedTxNode {
        tx_calls: Arc<AtomicUsize>,
        header_calls: Arc<AtomicUsize>,
        page_calls: Arc<AtomicUsize>,
        code_a: String,
        code_b: String,
    }

    impl Respond for SharedTxNode {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let method = body["method"].as_str().unwrap_or_default();
            let params = &body["params"];
            let result = match method {
                "get_transactions" => {
                    let n = self.page_calls.fetch_add(1, Ordering::SeqCst);
                    // Which script is being asked for decides which output.
                    let code = params[0]["script"]["code_hash"].as_str().unwrap();
                    let io_index = if code == self.code_a { "0x0" } else { "0x1" };
                    if n.is_multiple_of(2) {
                        serde_json::json!({
                            "objects": [{
                                "block_number": "0xa",
                                "io_index": io_index,
                                "io_type": "output",
                                "tx_hash": test_hash(1),
                                "tx_index": "0x0"
                            }],
                            "last_cursor": format!("0xc{n}")
                        })
                    } else {
                        serde_json::json!({"objects": [], "last_cursor": "0x"})
                    }
                }
                "get_transaction" => {
                    self.tx_calls.fetch_add(1, Ordering::SeqCst);
                    let output = |code: &str| {
                        serde_json::json!({
                            "capacity": "0x3b9aca00",
                            "lock": {"code_hash": test_hash(0xaa), "hash_type": "type", "args": "0x"},
                            "type": {"code_hash": code, "hash_type": "type", "args": "0x"}
                        })
                    };
                    serde_json::json!({
                        "transaction": {
                            "hash": test_hash(1),
                            "version": "0x0",
                            "cell_deps": [], "header_deps": [], "inputs": [],
                            "outputs": [output(&self.code_a), output(&self.code_b)],
                            "outputs_data": ["0x", "0x"],
                            "witnesses": []
                        },
                        "tx_status": {"status": "committed", "block_hash": test_hash(9), "block_number": "0xa"}
                    })
                }
                "get_header_by_number" => {
                    self.header_calls.fetch_add(1, Ordering::SeqCst);
                    serde_json::json!({
                        "version": "0x0", "compact_target": "0x1a08a97e",
                        "timestamp": "0x1a0a8b0b880", "number": "0xa", "epoch": "0x0",
                        "parent_hash": test_hash(0), "transactions_root": test_hash(0),
                        "proposals_hash": test_hash(0), "extra_hash": test_hash(0),
                        "dao": format!("0x{}", "00".repeat(32)), "nonce": "0x0",
                        "hash": test_hash(9)
                    })
                }
                other => panic!("unexpected RPC {other}"),
            };
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"jsonrpc":"2.0","id":1,"result":result}))
        }
    }

    struct SharedTxFixture {
        server: MockServer,
        tx_calls: Arc<AtomicUsize>,
        header_calls: Arc<AtomicUsize>,
        page_calls: Arc<AtomicUsize>,
        script_a: Script,
        script_b: Script,
    }

    async fn shared_tx_fixture() -> SharedTxFixture {
        let code_a = test_hash(0xa1);
        let code_b = test_hash(0xb2);
        let tx_calls = Arc::new(AtomicUsize::new(0));
        let header_calls = Arc::new(AtomicUsize::new(0));
        let page_calls = Arc::new(AtomicUsize::new(0));
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .respond_with(SharedTxNode {
                tx_calls: tx_calls.clone(),
                header_calls: header_calls.clone(),
                page_calls: page_calls.clone(),
                code_a: code_a.clone(),
                code_b: code_b.clone(),
            })
            .mount(&server)
            .await;
        let script = |code: String| Script {
            code_hash: code,
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        SharedTxFixture {
            server,
            tx_calls,
            header_calls,
            page_calls,
            script_a: script(code_a),
            script_b: script(code_b),
        }
    }

    /// V0 requires one fetched chain fact to serve every item that needs it.
    /// Per-entity caches re-fetch the same transaction and header for each
    /// entity, which also makes the RPC budget a per-entity budget.
    #[tokio::test]
    async fn chain_facts_are_shared_across_entities() {
        let fixture = shared_tx_fixture().await;
        let client = CkbRpcClient::new(fixture.server.uri());
        let mut budget = RunBudget::new(
            MAX_HISTORY_RECORDS,
            MAX_RPC_REQUESTS,
            Duration::from_secs(MAX_WALL_SECONDS),
        );
        let mut collector = Collector::new(&client, &mut budget);

        collect_token_expectation(&mut collector, &fixture.script_a, 100)
            .await
            .unwrap();
        collect_token_expectation(&mut collector, &fixture.script_b, 100)
            .await
            .unwrap();

        assert_eq!(
            fixture.tx_calls.load(Ordering::SeqCst),
            1,
            "the second entity must reuse the transaction the first already fetched"
        );
        assert_eq!(
            fixture.header_calls.load(Ordering::SeqCst),
            1,
            "the block header is the same chain fact for both entities"
        );
    }

    /// Pagination used to run before the collector existed, so its requests
    /// were counted in neither the RPC budget nor the deadline.
    #[tokio::test]
    async fn pagination_requests_count_against_the_run_budget() {
        let fixture = shared_tx_fixture().await;
        let client = CkbRpcClient::new(fixture.server.uri());
        let mut budget = RunBudget::new(
            MAX_HISTORY_RECORDS,
            MAX_RPC_REQUESTS,
            Duration::from_secs(MAX_WALL_SECONDS),
        );
        let mut collector = Collector::new(&client, &mut budget);
        collect_token_expectation(&mut collector, &fixture.script_a, 100)
            .await
            .unwrap();

        let pages = fixture.page_calls.load(Ordering::SeqCst);
        assert_eq!(pages, 2, "one page of records plus the terminating page");
        assert_eq!(
            budget.rpc_requests, 4,
            "2 pages + 1 header + 1 transaction all spend the same budget"
        );
    }

    /// The budget is one run-wide allowance across all entities, so an entity
    /// that exhausts it leaves the next one uncovered rather than starting
    /// again with a fresh allowance.
    #[tokio::test]
    async fn a_spent_run_budget_leaves_the_next_entity_uncovered() {
        let fixture = shared_tx_fixture().await;
        let client = CkbRpcClient::new(fixture.server.uri());
        // Enough for the first entity's walk, not for a second.
        let mut budget = RunBudget::new(MAX_HISTORY_RECORDS, 4, Duration::from_secs(600));
        let mut collector = Collector::new(&client, &mut budget);

        let first = collect_token_expectation(&mut collector, &fixture.script_a, 100)
            .await
            .unwrap();
        assert!(first.complete, "{:?}", first.uncovered);

        let second = collect_token_expectation(&mut collector, &fixture.script_b, 100)
            .await
            .unwrap();
        assert!(
            !second.complete,
            "a spent run budget must not be refilled per entity"
        );
        assert!(
            second.uncovered.iter().any(|r| r.contains("budget")),
            "{:?}",
            second.uncovered
        );
    }

    #[test]
    fn incident_selectors_are_always_candidates() {
        let mainnet = incident_selectors("mainnet");
        assert_eq!(mainnet.len(), 2);
        assert!(mainnet.iter().all(|s| s.kind == "token"));
        assert_eq!(incident_selectors("testnet").len(), 1);
    }
}
