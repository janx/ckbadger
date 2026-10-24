//! S31: the DAO lifecycle, the live-cell summary and `/dao/statistics`
//! against the node's live NervosDAO cells, cell by cell, at one block.
//!
//! Every live DAO cell is exactly one lifecycle entry: a deposit (status 0,
//! keyed by its own outpoint) or a withdraw request (status 1, keyed by the
//! request outpoint). The node's indexer lists those cells directly, so the
//! comparison needs no ckbadger-side derivation: the set of live DAO cells
//! at height `H`, with their capacities and kinds, must equal the set the
//! lifecycle implies at `H`, and the aggregates ckbadger publishes must be
//! that set's counts and sums.
//!
//! This is the check that would have caught POSTMORTEM DAO-032, where three
//! completions were dropped by the live writer: their request cells were
//! spent on chain but ckbadger kept them as pending withdrawals.
//!
//! ## One block on both sides
//!
//! Both sources move while the check runs, and each is pinned to the anchor
//! `H` exactly instead of being raced:
//!
//! * ckbadger: the unfiltered `/dao/deposits` listing is ordered by deposit
//!   block, which a status change never moves, so a walk started after `H`
//!   was read sees every deposit created at or before `H`. Each row carries
//!   the blocks of its own transitions, from which its state AT `H` follows
//!   exactly — however far the indexer has moved since.
//! * node: `get_cells` lists the cells live while it pages. Cells it lists
//!   that were created after `H` are dropped (it reports each cell's creation
//!   block); cells consumed after `H` are re-added from the node's own
//!   history (`get_transactions` over `(H, tip]`, each consumed cell fetched
//!   from its creating transaction). The result is the live set at `H`,
//!   independent of when each page was read.
//!
//! The anchors are re-verified against the node at the end; a reorg below
//! the walk's end makes the run inconclusive, never a failure.
//!
//! Cost: one walk of every DAO deposit through the API (~920 pages on
//! mainnet), one `get_cells` walk (~25 pages), and a handful of transaction
//! fetches for the blocks that passed meanwhile. That is minutes, not
//! seconds — Sampling tier.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context as _};

use super::api_checks::walk_dao_deposits;
use super::checks::*;
use super::entity_history::run_on_dedicated_runtime;
use super::source::{
    collect_transactions, qualify_source, reverify_anchor, RunBudget, SourceAnchor,
    SourceDeclaration, SourceQualification,
};
use crate::rpc::{CkbRpcClient, IndexerIoType, IndexerSearchKey, Script, TransactionWithStatus};

pub struct DaoLiveCellsMatchNode;

const DAO_TYPE_CODE_HASH: &str =
    "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const GET_CELLS_PAGE_LIMIT: u32 = 1_000;
const GET_TRANSACTIONS_PAGE_LIMIT: u32 = 1_000;
/// Runaway guards: mainnet holds ~23k live DAO cells (~25 pages). Reaching
/// either means a cursor is not advancing, which is itself an error.
const GET_CELLS_MAX_PAGES: usize = 10_000;
const GET_TRANSACTIONS_MAX_PAGES: usize = 10_000;
/// Findings listed in full; the rest are counted.
const MAX_LISTED_FINDINGS: usize = 200;

fn dao_type_script() -> Script {
    Script {
        code_hash: DAO_TYPE_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: "0x".to_string(),
    }
}

// ── What ckbadger serves ──────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DaoLifecycleRecord {
    pub(super) tx_hash: String,
    pub(super) output_index: i32,
    pub(super) capacity: String,
    pub(super) deposit_block_number: i64,
    pub(super) status: String,
    pub(super) withdraw_request_block: Option<i64>,
    pub(super) withdraw_request_tx_hash: Option<String>,
    pub(super) withdraw_request_output_index: Option<i32>,
    pub(super) withdraw_block: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DaoStatisticsAnchor {
    pub(super) tip_block_number: i64,
    pub(super) active_deposits: i64,
    pub(super) total_deposited: String,
    pub(super) pending_withdrawal_capacity: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LiveCellSummaryTip {
    block: i64,
    hash: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LiveCellSummaryClasses {
    dao: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LiveCellSummaryView {
    tip: LiveCellSummaryTip,
    classes: LiveCellSummaryClasses,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BlockHashView {
    number: i64,
    hash: String,
}

// ── The compared quantity ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DaoCellKind {
    Deposit,
    Request { deposit_block: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DaoCell {
    pub(super) capacity: u64,
    pub(super) kind: DaoCellKind,
}

impl DaoCell {
    fn describe(&self) -> String {
        match self.kind {
            DaoCellKind::Deposit => format!("deposit of {} shannons", self.capacity),
            DaoCellKind::Request { deposit_block } => format!(
                "withdraw request of {} shannons (deposit block {deposit_block})",
                self.capacity
            ),
        }
    }

    /// RFC-0023 cell data: 8 zero bytes for a deposit, the deposit block
    /// number (u64 LE) for a withdraw request.
    fn from_data(capacity: u64, data: &[u8], outpoint: &str) -> anyhow::Result<Self> {
        let bytes: [u8; 8] = data.try_into().map_err(|_| {
            anyhow!(
                "DAO cell {outpoint} carries {} data bytes; RFC-0023 cells carry 8",
                data.len()
            )
        })?;
        let deposit_block = u64::from_le_bytes(bytes);
        let kind = if deposit_block == 0 {
            DaoCellKind::Deposit
        } else {
            DaoCellKind::Request { deposit_block }
        };
        Ok(Self { capacity, kind })
    }
}

/// Live DAO cells by outpoint (`0x<tx hash>:<index>`, lower-case).
pub(super) type DaoCellSet = BTreeMap<String, DaoCell>;

pub(super) fn outpoint_key(tx_hash: &str, index: u64) -> String {
    format!("{}:{}", tx_hash.to_ascii_lowercase(), index)
}

fn parse_shannons(raw: &str, field: &str, entity: &str) -> anyhow::Result<u64> {
    raw.parse::<u64>()
        .map_err(|e| anyhow!("{field} '{raw}' of {entity} is not a shannon amount: {e}"))
}

/// The live DAO cells ckbadger's lifecycle implies at `height`.
///
/// A row's transitions carry their own blocks, so the state at `height`
/// does not depend on when the row was read. A deposit created after
/// `height` is not part of it.
pub(super) fn lifecycle_cells_at(
    records: &[DaoLifecycleRecord],
    height: i64,
) -> anyhow::Result<DaoCellSet> {
    let mut cells = DaoCellSet::new();
    for record in records {
        let deposit = outpoint_key(&record.tx_hash, u64::try_from(record.output_index)?);
        if record.deposit_block_number > height {
            continue;
        }
        let capacity = parse_shannons(&record.capacity, "capacity", &deposit)?;
        let deposit_block = u64::try_from(record.deposit_block_number)
            .map_err(|_| anyhow!("deposit {deposit} has a negative deposit block"))?;
        let (key, kind) = match record.status.as_str() {
            "deposited" => {
                if record.withdraw_request_block.is_some() || record.withdraw_block.is_some() {
                    bail!("deposit {deposit} is 'deposited' yet records a withdrawal");
                }
                (deposit.clone(), DaoCellKind::Deposit)
            }
            "withdrawing" | "withdrawn" => {
                let request_block = record.withdraw_request_block.ok_or_else(|| {
                    anyhow!(
                        "deposit {deposit} is '{}' without a request block",
                        record.status
                    )
                })?;
                let completed_at = match (record.status.as_str(), record.withdraw_block) {
                    ("withdrawn", Some(block)) => Some(block),
                    ("withdrawn", None) => {
                        bail!("deposit {deposit} is 'withdrawn' without a withdraw block")
                    }
                    (_, Some(_)) => {
                        bail!("deposit {deposit} is 'withdrawing' yet records a withdraw block")
                    }
                    (_, None) => None,
                };
                if completed_at.is_some_and(|block| block <= height) {
                    continue;
                }
                if request_block <= height {
                    let request_tx =
                        record.withdraw_request_tx_hash.as_deref().ok_or_else(|| {
                            anyhow!("deposit {deposit} has a request block but no request tx")
                        })?;
                    let request_index = record.withdraw_request_output_index.ok_or_else(|| {
                        anyhow!("deposit {deposit} has a request block but no request index")
                    })?;
                    (
                        outpoint_key(request_tx, u64::try_from(request_index)?),
                        DaoCellKind::Request { deposit_block },
                    )
                } else {
                    (deposit.clone(), DaoCellKind::Deposit)
                }
            }
            other => bail!("deposit {deposit} has unknown status '{other}'"),
        };
        if cells
            .insert(key.clone(), DaoCell { capacity, kind })
            .is_some()
        {
            bail!("two DAO lifecycle rows claim live cell {key} at block {height}");
        }
    }
    Ok(cells)
}

// ── What the node holds ───────────────────────────────────────────────────

/// A DAO cell `get_cells` listed as live, with the block that created it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListedDaoCell {
    pub(super) outpoint: String,
    pub(super) created_at: u64,
    pub(super) cell: DaoCell,
}

/// A DAO cell the chain consumed inside the replayed range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConsumedDaoCell {
    pub(super) outpoint: String,
    pub(super) created_at: u64,
    pub(super) consumed_at: u64,
    pub(super) cell: DaoCell,
}

/// The node's live DAO cells as observed, plus every DAO cell consumed in
/// `(replay_from, replay_through]`.
#[derive(Debug, Clone)]
pub(super) struct NodeDaoObservation {
    pub(super) listed: Vec<ListedDaoCell>,
    pub(super) consumed: Vec<ConsumedDaoCell>,
    pub(super) replay_from: u64,
    pub(super) replay_through: u64,
}

impl NodeDaoObservation {
    /// The node's live DAO cells at `height`: what it listed minus what was
    /// created after `height`, plus what was consumed after `height` but
    /// created at or before it.
    pub(super) fn cells_at(&self, height: u64) -> anyhow::Result<DaoCellSet> {
        if height < self.replay_from || height > self.replay_through {
            bail!(
                "height {height} is outside the replayed range ({}, {}]",
                self.replay_from,
                self.replay_through
            );
        }
        let mut cells = DaoCellSet::new();
        for listed in self.listed.iter().filter(|c| c.created_at <= height) {
            if cells.insert(listed.outpoint.clone(), listed.cell).is_some() {
                bail!("the node listed DAO cell {} twice", listed.outpoint);
            }
        }
        for consumed in self
            .consumed
            .iter()
            .filter(|c| c.consumed_at > height && c.created_at <= height)
        {
            match cells.insert(consumed.outpoint.clone(), consumed.cell) {
                Some(listed) if listed != consumed.cell => bail!(
                    "DAO cell {} differs between the listing ({}) and its creating transaction ({})",
                    consumed.outpoint,
                    listed.describe(),
                    consumed.cell.describe()
                ),
                _ => {}
            }
        }
        Ok(cells)
    }
}

/// Walk the node's live DAO cells, then replay every DAO consumption since
/// `replay_from`. `Ok(Err(reason))` is an inconclusive source.
async fn observe_node_dao_cells(
    client: &CkbRpcClient,
    budget: &mut RunBudget,
    replay_from: u64,
    must_cover: u64,
) -> anyhow::Result<Result<NodeDaoObservation, String>> {
    budget.charge_request();
    let tip_before = client.get_indexer_tip().await?;
    if tip_before.block_number < must_cover {
        return Ok(Err(format!(
            "node indexer tip {} is behind the anchor {must_cover}",
            tip_before.block_number
        )));
    }

    let search_key = IndexerSearchKey::exact_type(dao_type_script(), None);
    let mut listed = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors: HashSet<String> = HashSet::new();
    for page_number in 0.. {
        if page_number >= GET_CELLS_MAX_PAGES {
            bail!("get_cells did not terminate within {GET_CELLS_MAX_PAGES} pages");
        }
        if let Some(reason) = budget.exhausted() {
            return Ok(Err(reason));
        }
        budget.charge_request();
        let page = client
            .get_cells(&search_key, "asc", GET_CELLS_PAGE_LIMIT, cursor.as_deref())
            .await?;
        if page.objects.is_empty() {
            break;
        }
        if !seen_cursors.insert(page.last_cursor.clone()) {
            bail!(
                "get_cells cursor '{}' repeated: the enumeration is cycling",
                page.last_cursor
            );
        }
        budget.charge_records(page.objects.len());
        for cell in page.objects {
            if cell.type_script.as_ref() != Some(&dao_type_script()) {
                bail!(
                    "get_cells returned a non-DAO cell {}:{} for the DAO type query",
                    cell.tx_hash,
                    cell.index
                );
            }
            let outpoint = outpoint_key(&cell.tx_hash, u64::from(cell.index));
            listed.push(ListedDaoCell {
                cell: DaoCell::from_data(cell.capacity, &cell.output_data, &outpoint)?,
                outpoint,
                created_at: cell.block_number,
            });
        }
        cursor = Some(page.last_cursor);
    }

    budget.charge_request();
    let tip_after = client.get_indexer_tip().await?;
    if tip_after.block_number < tip_before.block_number {
        return Ok(Err(format!(
            "node indexer tip fell from {} to {} during the walk",
            tip_before.block_number, tip_after.block_number
        )));
    }

    // Every DAO cell consumed in (replay_from, tip_after]. Consumed cells
    // created after `replay_from` are kept too: `cells_at` filters by height.
    let history = collect_transactions(
        client,
        &IndexerSearchKey::exact_type(
            dao_type_script(),
            Some((replay_from + 1, tip_after.block_number + 1)),
        ),
        GET_TRANSACTIONS_PAGE_LIMIT,
        GET_TRANSACTIONS_MAX_PAGES,
        budget,
    )
    .await?;
    if !history.complete {
        return Ok(Err(budget.exhausted().unwrap_or_else(|| {
            "DAO history after the anchor was not enumerated completely".to_string()
        })));
    }

    let mut transactions: HashMap<String, TransactionWithStatus> = HashMap::new();
    let mut consumed = Vec::new();
    let mut seen_inputs: HashSet<(String, u32)> = HashSet::new();
    for record in history
        .records
        .iter()
        .filter(|record| record.io_type == IndexerIoType::Input)
    {
        if !seen_inputs.insert((record.tx_hash.to_ascii_lowercase(), record.io_index)) {
            continue;
        }
        let spender = fetch_transaction(client, budget, &mut transactions, &record.tx_hash).await?;
        let input = spender
            .transaction
            .as_ref()
            .and_then(|tx| tx.inputs.get(record.io_index as usize))
            .ok_or_else(|| {
                anyhow!(
                    "transaction {} has no input {} the node indexer recorded",
                    record.tx_hash,
                    record.io_index
                )
            })?
            .previous_output
            .clone();
        let index = parse_hex_quantity(&input.index, "previous_output.index")?;
        let index = u32::try_from(index)?;
        let creator = fetch_transaction(client, budget, &mut transactions, &input.tx_hash).await?;
        let created_at = creator
            .tx_status
            .block_number
            .as_deref()
            .ok_or_else(|| anyhow!("transaction {} has no block number", input.tx_hash))
            .and_then(|raw| parse_hex_quantity(raw, "tx_status.block_number"))
            .with_context(|| format!("transaction {}", input.tx_hash))?;
        let tx = creator
            .transaction
            .as_ref()
            .ok_or_else(|| anyhow!("transaction {} has no body", input.tx_hash))?;
        let output = tx
            .outputs
            .get(index as usize)
            .ok_or_else(|| anyhow!("transaction {} has no output {index}", input.tx_hash))?;
        if output.type_.as_ref() != Some(&dao_type_script()) {
            bail!(
                "the node indexer recorded {}:{index} as a DAO input, but its cell is not DAO-typed",
                input.tx_hash
            );
        }
        let data = tx
            .outputs_data
            .get(index as usize)
            .ok_or_else(|| anyhow!("transaction {} has no output data {index}", input.tx_hash))?;
        let outpoint = outpoint_key(&input.tx_hash, u64::from(index));
        consumed.push(ConsumedDaoCell {
            cell: DaoCell::from_data(
                parse_hex_quantity(&output.capacity, "capacity")?,
                &crate::rpc::parse_hex_to_bytes(data),
                &outpoint,
            )?,
            outpoint,
            created_at,
            consumed_at: record.block_number,
        });
    }

    // The walk and the replay describe one chain only if the block the walk
    // ended on is still canonical.
    budget.charge_request();
    let canonical = client.get_block_hash(tip_after.block_number).await?;
    if !canonical
        .as_deref()
        .is_some_and(|hash| hash.eq_ignore_ascii_case(&tip_after.block_hash))
    {
        return Ok(Err(format!(
            "block {} changed from {} to {:?} during the check (reorg)",
            tip_after.block_number, tip_after.block_hash, canonical
        )));
    }

    Ok(Ok(NodeDaoObservation {
        listed,
        consumed,
        replay_from,
        replay_through: tip_after.block_number,
    }))
}

fn parse_hex_quantity(raw: &str, field: &str) -> anyhow::Result<u64> {
    let digits = raw
        .strip_prefix("0x")
        .ok_or_else(|| anyhow!("{field} '{raw}' is not 0x-prefixed hex"))?;
    u64::from_str_radix(digits, 16).map_err(|e| anyhow!("{field} '{raw}': {e}"))
}

async fn fetch_transaction(
    client: &CkbRpcClient,
    budget: &mut RunBudget,
    cache: &mut HashMap<String, TransactionWithStatus>,
    tx_hash: &str,
) -> anyhow::Result<TransactionWithStatus> {
    let key = tx_hash.to_ascii_lowercase();
    if let Some(tx) = cache.get(&key) {
        return Ok(tx.clone());
    }
    budget.charge_request();
    let tx = client
        .get_transaction(tx_hash)
        .await?
        .ok_or_else(|| anyhow!("node does not know transaction {tx_hash}"))?;
    cache.insert(key, tx.clone());
    Ok(tx)
}

// ── The comparison ────────────────────────────────────────────────────────

/// Every outpoint on which the node and the lifecycle disagree at `height`.
pub(super) fn compare_cells(
    node: &DaoCellSet,
    lifecycle: &DaoCellSet,
    height: u64,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (outpoint, chain) in node {
        match lifecycle.get(outpoint) {
            None => findings.push(Finding {
                entity: outpoint.clone(),
                details: vec![format!(
                    "live on chain at block {height} as a {}, but the DAO lifecycle has no live entry for it",
                    chain.describe()
                )],
            }),
            Some(ours) if ours != chain => findings.push(Finding {
                entity: outpoint.clone(),
                details: vec![format!(
                    "at block {height} the chain holds a {}, the DAO lifecycle a {}",
                    chain.describe(),
                    ours.describe()
                )],
            }),
            Some(_) => {}
        }
    }
    for (outpoint, ours) in lifecycle {
        if !node.contains_key(outpoint) {
            findings.push(Finding {
                entity: outpoint.clone(),
                details: vec![format!(
                    "the DAO lifecycle holds a live {} at block {height}, but the chain has no such live cell",
                    ours.describe()
                )],
            });
        }
    }
    findings
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct DaoCellTotals {
    pub(super) deposits: u64,
    pub(super) deposited: u128,
    pub(super) requests: u64,
    pub(super) requested: u128,
}

pub(super) fn totals(cells: &DaoCellSet) -> DaoCellTotals {
    let mut totals = DaoCellTotals::default();
    for cell in cells.values() {
        match cell.kind {
            DaoCellKind::Deposit => {
                totals.deposits += 1;
                totals.deposited += u128::from(cell.capacity);
            }
            DaoCellKind::Request { .. } => {
                totals.requests += 1;
                totals.requested += u128::from(cell.capacity);
            }
        }
    }
    totals
}

/// `/dao/statistics` must be the counts and sums of the chain's live DAO
/// cells at its own tip.
pub(super) fn compare_statistics(
    chain: DaoCellTotals,
    stats: &DaoStatisticsAnchor,
) -> anyhow::Result<Vec<Finding>> {
    let mut details = Vec::new();
    let live = chain.deposits + chain.requests;
    if u64::try_from(stats.active_deposits).ok() != Some(live) {
        details.push(format!(
            "activeDeposits={} but the chain has {live} live DAO cells ({} deposits + {} withdraw requests)",
            stats.active_deposits, chain.deposits, chain.requests
        ));
    }
    let deposited = parse_shannons(&stats.total_deposited, "totalDeposited", "/dao/statistics")?;
    if u128::from(deposited) != chain.deposited {
        details.push(format!(
            "totalDeposited={deposited} but the chain's live deposits hold {}",
            chain.deposited
        ));
    }
    let pending = parse_shannons(
        &stats.pending_withdrawal_capacity,
        "pendingWithdrawalCapacity",
        "/dao/statistics",
    )?;
    if u128::from(pending) != chain.requested {
        details.push(format!(
            "pendingWithdrawalCapacity={pending} but the chain's live withdraw requests hold {}",
            chain.requested
        ));
    }
    Ok(if details.is_empty() {
        Vec::new()
    } else {
        vec![Finding {
            entity: format!("/dao/statistics @ block {}", stats.tip_block_number),
            details,
        }]
    })
}

/// The live-cell summary's DAO class must be the chain's live DAO-cell count.
pub(super) fn compare_live_summary(chain: &DaoCellSet, block: i64, dao: u64) -> Vec<Finding> {
    let live = chain.len() as u64;
    if live == dao {
        return Vec::new();
    }
    vec![Finding {
        entity: format!("/cells/live-summary @ block {block}"),
        details: vec![format!(
            "classes.dao={dao} but the chain has {live} live DAO cells"
        )],
    }]
}

impl Check for DaoLiveCellsMatchNode {
    fn name(&self) -> &'static str {
        "dao_live_cells_match_node"
    }

    fn description(&self) -> &'static str {
        "Every live NervosDAO cell on the node is exactly one DAO lifecycle entry, and the DAO statistics and live-cell summary count them"
    }

    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }

    fn requires_rpc(&self) -> bool {
        true
    }

    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let rpc_url = ctx
            .rpc_url
            .clone()
            .ok_or_else(|| anyhow!("DAO cell verification requires --rpc-url"))?;

        // Each endpoint is one pinned read, so each is exact at its own tip.
        let stats: DaoStatisticsAnchor = api_get(ctx, "dao/statistics")?;
        let summary: LiveCellSummaryView = api_get(ctx, "cells/live-summary")?;
        let stats_block: BlockHashView =
            api_get(ctx, &format!("blocks/{}", stats.tip_block_number))?;
        if stats_block.number != stats.tip_block_number {
            bail!(
                "blocks/{} answered block {}",
                stats.tip_block_number,
                stats_block.number
            );
        }
        let stats_anchor = SourceAnchor {
            block_number: u64::try_from(stats.tip_block_number)?,
            block_hash: stats_block.hash,
        };
        let summary_anchor = SourceAnchor {
            block_number: u64::try_from(summary.tip.block)?,
            block_hash: summary.tip.hash.clone(),
        };
        let low = stats_anchor.block_number.min(summary_anchor.block_number);
        let high = stats_anchor.block_number.max(summary_anchor.block_number);
        let low_anchor = if stats_anchor.block_number == low {
            stats_anchor.clone()
        } else {
            summary_anchor.clone()
        };

        // After the anchors were read, so every deposit at or below them is
        // already in its fixed place in the listing.
        progress.set_message("dao/deposits (all)");
        let records: Vec<DaoLifecycleRecord> = walk_dao_deposits(ctx, progress, None)?;
        let lifecycle = lifecycle_cells_at(&records, stats.tip_block_number)
            .context("deriving the DAO lifecycle's live cells")?;

        let declaration_path = ctx
            .verify_source_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("verify-source.toml"));
        let declaration = SourceDeclaration::load(&declaration_path)?;
        let client = CkbRpcClient::new(rpc_url);
        let mut budget = ctx.entity_budget.to_run_budget();
        progress.set_message("node live DAO cells");
        type Observed = Option<Result<NodeDaoObservation, String>>;
        let (qualification, observation): (SourceQualification, Observed) =
            run_on_dedicated_runtime(async {
                let qualification = qualify_source(
                    &client,
                    &mut budget,
                    declaration.as_ref(),
                    &declaration_path,
                    &low_anchor,
                )
                .await?;
                if !matches!(qualification, SourceQualification::Qualified(_)) {
                    return Ok((qualification, None));
                }
                let observation = observe_node_dao_cells(&client, &mut budget, low, high).await?;
                let observation = match observation {
                    Ok(observation) => {
                        let mut moved = None;
                        for anchor in [&stats_anchor, &summary_anchor] {
                            if let Some(reason) =
                                reverify_anchor(&client, &mut budget, anchor).await?
                            {
                                moved = Some(reason);
                                break;
                            }
                        }
                        match moved {
                            Some(reason) => Err(reason),
                            None => Ok(observation),
                        }
                    }
                    Err(reason) => Err(reason),
                };
                Ok::<_, anyhow::Error>((qualification, Some(observation)))
            })?;

        {
            let mut profile = ctx
                .source_profile
                .lock()
                .expect("verify source profile lock poisoned");
            if profile.is_none() {
                *profile = Some(qualification.to_report());
            }
        }
        if let SourceQualification::Inconclusive(reason) = &qualification {
            return Ok(CheckResult::inconclusive(format!(
                "chain source not qualified: {reason}"
            )));
        }
        let observation = match observation {
            Some(Ok(observation)) => observation,
            Some(Err(reason)) => return Ok(CheckResult::inconclusive(reason)),
            None => unreachable!("a qualified source is always observed"),
        };

        let chain_at_stats = observation.cells_at(stats_anchor.block_number)?;
        let chain_at_summary = observation.cells_at(summary_anchor.block_number)?;

        let mut findings = compare_cells(&chain_at_stats, &lifecycle, stats_anchor.block_number);
        findings.extend(compare_statistics(totals(&chain_at_stats), &stats)?);
        findings.extend(compare_live_summary(
            &chain_at_summary,
            summary.tip.block,
            summary.classes.dao,
        ));

        let checked = chain_at_stats.len().max(lifecycle.len()) as u64;
        let chain_totals = totals(&chain_at_stats);
        if findings.is_empty() {
            return Ok(CheckResult::pass_with_detail(
                checked,
                format!(
                    "{} live DAO cells at block {} ({} deposits, {} withdraw requests) match the lifecycle, statistics and live-cell summary",
                    chain_at_stats.len(),
                    stats_anchor.block_number,
                    chain_totals.deposits,
                    chain_totals.requests
                ),
            ));
        }
        let total = findings.len();
        if total > MAX_LISTED_FINDINGS {
            findings.truncate(MAX_LISTED_FINDINGS);
            findings.push(Finding {
                entity: "…".to_string(),
                details: vec![format!(
                    "{} more difference(s) not listed",
                    total - MAX_LISTED_FINDINGS
                )],
            });
        }
        Ok(CheckResult::fail(checked, findings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEPOSIT_A: &str = "0xaa";
    const REQUEST_B: &str = "0xbb";

    fn record(
        tx: &str,
        capacity: u64,
        deposit_block: i64,
        status: &str,
        request: Option<(&str, i64)>,
        withdraw_block: Option<i64>,
    ) -> DaoLifecycleRecord {
        DaoLifecycleRecord {
            tx_hash: tx.to_string(),
            output_index: 0,
            capacity: capacity.to_string(),
            deposit_block_number: deposit_block,
            status: status.to_string(),
            withdraw_request_block: request.map(|(_, block)| block),
            withdraw_request_tx_hash: request.map(|(tx, _)| tx.to_string()),
            withdraw_request_output_index: request.map(|_| 0),
            withdraw_block,
        }
    }

    fn deposit(capacity: u64) -> DaoCell {
        DaoCell {
            capacity,
            kind: DaoCellKind::Deposit,
        }
    }

    fn request(capacity: u64, deposit_block: u64) -> DaoCell {
        DaoCell {
            capacity,
            kind: DaoCellKind::Request { deposit_block },
        }
    }

    #[test]
    fn lifecycle_state_at_a_height_follows_each_rows_own_transition_blocks() {
        let records = vec![
            record(DEPOSIT_A, 100, 10, "deposited", None, None),
            // Requested at 20, completed at 30.
            record(
                "0xcc",
                200,
                11,
                "withdrawn",
                Some((REQUEST_B, 20)),
                Some(30),
            ),
            // Created after every height asked below.
            record("0xdd", 300, 50, "deposited", None, None),
        ];

        let at_15 = lifecycle_cells_at(&records, 15).unwrap();
        assert_eq!(
            at_15,
            DaoCellSet::from([
                (outpoint_key(DEPOSIT_A, 0), deposit(100)),
                (outpoint_key("0xcc", 0), deposit(200)),
            ])
        );
        let at_25 = lifecycle_cells_at(&records, 25).unwrap();
        assert_eq!(
            at_25,
            DaoCellSet::from([
                (outpoint_key(DEPOSIT_A, 0), deposit(100)),
                (outpoint_key(REQUEST_B, 0), request(200, 11)),
            ])
        );
        let at_30 = lifecycle_cells_at(&records, 30).unwrap();
        assert_eq!(
            at_30,
            DaoCellSet::from([(outpoint_key(DEPOSIT_A, 0), deposit(100))])
        );
    }

    #[test]
    fn lifecycle_rejects_contradictory_rows() {
        let completed_without_block =
            record("0xcc", 200, 11, "withdrawn", Some((REQUEST_B, 20)), None);
        assert!(lifecycle_cells_at(&[completed_without_block], 40).is_err());
        let pending_with_block = record(
            "0xcc",
            200,
            11,
            "withdrawing",
            Some((REQUEST_B, 20)),
            Some(30),
        );
        assert!(lifecycle_cells_at(&[pending_with_block], 40).is_err());
    }

    fn observation() -> NodeDaoObservation {
        NodeDaoObservation {
            listed: vec![
                ListedDaoCell {
                    outpoint: outpoint_key(DEPOSIT_A, 0),
                    created_at: 10,
                    cell: deposit(100),
                },
                // Created after the anchor: not live at it.
                ListedDaoCell {
                    outpoint: outpoint_key("0xee", 0),
                    created_at: 104,
                    cell: deposit(500),
                },
            ],
            consumed: vec![
                // Live at the anchor, spent while the walk ran.
                ConsumedDaoCell {
                    outpoint: outpoint_key(REQUEST_B, 0),
                    created_at: 20,
                    consumed_at: 102,
                    cell: request(200, 11),
                },
                // Created and spent after the anchor.
                ConsumedDaoCell {
                    outpoint: outpoint_key("0xff", 0),
                    created_at: 101,
                    consumed_at: 103,
                    cell: deposit(600),
                },
            ],
            replay_from: 100,
            replay_through: 105,
        }
    }

    #[test]
    fn node_cells_at_the_anchor_undo_everything_after_it() {
        assert_eq!(
            observation().cells_at(100).unwrap(),
            DaoCellSet::from([
                (outpoint_key(DEPOSIT_A, 0), deposit(100)),
                (outpoint_key(REQUEST_B, 0), request(200, 11)),
            ])
        );
        assert_eq!(
            observation().cells_at(102).unwrap(),
            DaoCellSet::from([
                (outpoint_key(DEPOSIT_A, 0), deposit(100)),
                (outpoint_key("0xff", 0), deposit(600)),
            ])
        );
        assert!(observation().cells_at(99).is_err(), "below the replay");
    }

    /// The DAO-032 shape: ckbadger keeps a request the chain has spent.
    #[test]
    fn a_spent_request_still_pending_in_the_lifecycle_is_a_finding() {
        let chain = DaoCellSet::from([(outpoint_key(DEPOSIT_A, 0), deposit(100))]);
        let lifecycle = DaoCellSet::from([
            (outpoint_key(DEPOSIT_A, 0), deposit(100)),
            (outpoint_key(REQUEST_B, 0), request(200, 11)),
        ]);
        let findings = compare_cells(&chain, &lifecycle, 100);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity, outpoint_key(REQUEST_B, 0));
        assert!(findings[0].details[0].contains("the chain has no such live cell"));

        let stats = DaoStatisticsAnchor {
            tip_block_number: 100,
            active_deposits: 2,
            total_deposited: "100".to_string(),
            pending_withdrawal_capacity: "200".to_string(),
        };
        let stat_findings = compare_statistics(totals(&chain), &stats).unwrap();
        assert_eq!(stat_findings.len(), 1);
        let details = &stat_findings[0].details;
        assert!(
            details.iter().any(|d| d.starts_with("activeDeposits=2")),
            "{details:?}"
        );
        assert!(
            details
                .iter()
                .any(|d| d.starts_with("pendingWithdrawalCapacity=200")),
            "{details:?}"
        );
        assert!(
            !details.iter().any(|d| d.starts_with("totalDeposited")),
            "{details:?}"
        );

        assert!(compare_live_summary(&chain, 100, 1).is_empty());
        assert_eq!(compare_live_summary(&chain, 100, 2).len(), 1);
    }

    #[test]
    fn kind_and_capacity_differences_are_findings() {
        let chain = DaoCellSet::from([(outpoint_key(DEPOSIT_A, 0), deposit(100))]);
        let wrong_capacity = DaoCellSet::from([(outpoint_key(DEPOSIT_A, 0), deposit(101))]);
        assert_eq!(compare_cells(&chain, &wrong_capacity, 1).len(), 1);
        let missing = DaoCellSet::new();
        let findings = compare_cells(&chain, &missing, 1);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].details[0].contains("no live entry"));
        assert!(compare_cells(&chain, &chain, 1).is_empty());
    }

    #[test]
    fn dao_cell_data_must_be_eight_bytes() {
        assert_eq!(
            DaoCell::from_data(1, &[0u8; 8], "x").unwrap().kind,
            DaoCellKind::Deposit
        );
        assert_eq!(
            DaoCell::from_data(1, &7u64.to_le_bytes(), "x")
                .unwrap()
                .kind,
            DaoCellKind::Request { deposit_block: 7 }
        );
        assert!(DaoCell::from_data(1, &[0u8; 4], "x").is_err());
    }

    // ── run() end to end, against a mocked API and node ──────────────────

    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    fn hash(seed: u64) -> String {
        format!("0x{seed:064x}")
    }

    /// Block hashes are `hash(1000 + n)`; the anchor is block 100 and the
    /// node indexer has reached block 102.
    fn block_hash(n: u64) -> String {
        hash(1000 + n)
    }

    const NODE_VERSION: &str = "0.209.0 (test)";
    const ANCHOR: u64 = 100;
    const NODE_TIP: u64 = 102;

    fn dao_type() -> serde_json::Value {
        json!({"code_hash": DAO_TYPE_CODE_HASH, "hash_type": "type", "args": "0x"})
    }

    fn data_hex(deposit_block: u64) -> String {
        format!("0x{}", hex::encode(deposit_block.to_le_bytes()))
    }

    /// The chain: deposit `D1` (block 10) and deposit `D2` (block 11) live;
    /// `D2`'s withdraw request `R2` was made at block 50 and SPENT at block
    /// 101 — after the anchor, so it is live at block 100 and only the
    /// replay can show it. `R3` (deposit block 12, requested at 60) was
    /// spent at block 90, before the anchor.
    struct FakeNode;

    impl Respond for FakeNode {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let params = &body["params"];
            let result = match body["method"].as_str().unwrap() {
                "local_node_info" => json!({"version": NODE_VERSION}),
                "get_block_hash" => {
                    let n = u64::from_str_radix(
                        params[0].as_str().unwrap().trim_start_matches("0x"),
                        16,
                    )
                    .unwrap();
                    json!(block_hash(n))
                }
                "get_indexer_tip" => json!({
                    "block_number": format!("0x{NODE_TIP:x}"),
                    "block_hash": block_hash(NODE_TIP)
                }),
                "get_cells" => {
                    if params[3].is_null() {
                        json!({
                            "objects": [{
                                "block_number": "0xa",
                                "out_point": {"tx_hash": hash(0xd1), "index": "0x0"},
                                "output": {"capacity": "0x174876e800",
                                           "lock": {"code_hash": hash(0x9b), "hash_type": "type", "args": "0x"},
                                           "type": dao_type()},
                                "output_data": data_hex(0),
                                "tx_index": "0x1"
                            }],
                            "last_cursor": "0xc1"
                        })
                    } else {
                        json!({"objects": [], "last_cursor": "0xc2"})
                    }
                }
                "get_transactions" => {
                    // Only the range after the anchor is replayed.
                    assert_eq!(
                        params[0]["filter"]["block_range"],
                        json!([
                            format!("0x{:x}", ANCHOR + 1),
                            format!("0x{:x}", NODE_TIP + 1)
                        ])
                    );
                    if params[3].is_null() {
                        json!({
                            "objects": [{
                                "block_number": "0x65",
                                "io_index": "0x0",
                                "io_type": "input",
                                "tx_hash": hash(0xe2),
                                "tx_index": "0x1"
                            }],
                            "last_cursor": "0xt1"
                        })
                    } else {
                        json!({"objects": [], "last_cursor": "0xt2"})
                    }
                }
                "get_transaction" => {
                    let tx = params[0].as_str().unwrap();
                    let (inputs, outputs, outputs_data, block): (
                        serde_json::Value,
                        serde_json::Value,
                        serde_json::Value,
                        u64,
                    ) = if tx == hash(0xe2) {
                        (
                            json!([{"since": "0x0", "previous_output": {"tx_hash": hash(0xa2), "index": "0x0"}}]),
                            json!([{"capacity": "0x2540be400", "lock": {"code_hash": hash(0x9b), "hash_type": "type", "args": "0x"}, "type": null}]),
                            json!(["0x"]),
                            101,
                        )
                    } else if tx == hash(0xa2) {
                        (
                            json!([]),
                            json!([{"capacity": "0x2540be400", "lock": {"code_hash": hash(0x9b), "hash_type": "type", "args": "0x"}, "type": dao_type()}]),
                            json!([data_hex(11)]),
                            50,
                        )
                    } else {
                        panic!("unexpected transaction {tx}")
                    };
                    json!({
                        "transaction": {
                            "hash": tx, "version": "0x0", "cell_deps": [], "header_deps": [],
                            "inputs": inputs, "outputs": outputs, "outputs_data": outputs_data,
                            "witnesses": []
                        },
                        "tx_status": {"status": "committed", "block_hash": block_hash(block),
                                      "block_number": format!("0x{block:x}")}
                    })
                }
                other => panic!("unexpected RPC {other}"),
            };
            ResponseTemplate::new(200)
                .set_body_json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result}))
        }
    }

    fn lifecycle_row(
        deposit: u64,
        capacity: u64,
        deposit_block: i64,
        status: &str,
        request: Option<(u64, i64)>,
        withdraw_block: Option<i64>,
    ) -> serde_json::Value {
        json!({
            "txHash": hash(deposit), "outputIndex": 0, "capacity": capacity.to_string(),
            "depositBlockNumber": deposit_block, "status": status,
            "withdrawRequestBlock": request.map(|(_, block)| block),
            "withdrawRequestTxHash": request.map(|(tx, _)| hash(tx)),
            "withdrawRequestOutputIndex": request.map(|_| 0),
            "withdrawBlock": withdraw_block,
        })
    }

    struct Fixture {
        _runtime: tokio::runtime::Runtime,
        _api: MockServer,
        _node: MockServer,
        _dir: tempfile::TempDir,
        ctx: CheckContext,
    }

    fn fixture(rows: serde_json::Value, stats: serde_json::Value, summary_dao: u64) -> Fixture {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (api, node) = runtime.block_on(async {
            let api = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/statistics"))
                .respond_with(ResponseTemplate::new(200).set_body_json(stats))
                .mount(&api)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live-summary"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "tip": {"block": ANCHOR, "hash": block_hash(ANCHOR)},
                    "liveCells": summary_dao, "classes": {"dao": summary_dao, "typedNonDao": 0, "plain": 0},
                    "dataBearing": 0
                })))
                .mount(&api)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/blocks/{ANCHOR}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "number": ANCHOR, "hash": block_hash(ANCHOR)
                })))
                .mount(&api)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/deposits"))
                .and(query_param("limit", "100"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": rows, "nextCursor": null
                })))
                .mount(&api)
                .await;
            let node = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(FakeNode)
                .mount(&node)
                .await;
            (api, node)
        });
        let dir = tempfile::tempdir().unwrap();
        let declaration = dir.path().join("verify-source.toml");
        std::fs::write(
            &declaration,
            format!(
                "genesisHash = \"{}\"\nnodeVersion = \"{NODE_VERSION}\"\nindexerVersion = \"{NODE_VERSION}\"\nbuildStartBlock = 0\ncontinuousFromGenesis = true\nprovenance = \"test\"\n",
                block_hash(0)
            ),
        )
        .unwrap();
        let ctx = CheckContext {
            network: ckbadger_common::hardfork::NETWORK_MAINNET,
            api_url: format!("{}/api/v1", api.uri()),
            rpc_url: Some(node.uri()),
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: Some(declaration),
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        };
        Fixture {
            _runtime: runtime,
            _api: api,
            _node: node,
            _dir: dir,
            ctx,
        }
    }

    /// Rows that agree with the chain at block 100: D1 deposited, D2's
    /// request R2 pending, D3 completed at 90.
    fn consistent_rows() -> serde_json::Value {
        json!([
            lifecycle_row(0xd1, 100_000_000_000, 10, "deposited", None, None),
            lifecycle_row(
                0xd2,
                10_000_000_000,
                11,
                "withdrawn",
                Some((0xa2, 50)),
                Some(101)
            ),
            lifecycle_row(
                0xd3,
                10_000_000_000,
                12,
                "withdrawn",
                Some((0xa3, 60)),
                Some(90)
            ),
        ])
    }

    fn consistent_stats() -> serde_json::Value {
        json!({
            "tipBlockNumber": ANCHOR, "activeDeposits": 2,
            "totalDeposited": "100000000000", "pendingWithdrawalCapacity": "10000000000"
        })
    }

    #[test]
    fn run_passes_when_the_lifecycle_matches_the_chain_at_the_anchor() {
        let fx = fixture(consistent_rows(), consistent_stats(), 2);
        let result = DaoLiveCellsMatchNode
            .run(&fx.ctx, &ProgressReporter::new(None))
            .unwrap();
        assert_eq!(result.status, CheckStatus::Pass, "{result:?}");
        assert_eq!(result.items_checked, 2);
    }

    /// DAO-032 as the API served it: D3's completion at block 90 was lost,
    /// so the lifecycle still lists R3 as pending and the aggregates count
    /// it. Every symptom is reported against the chain.
    #[test]
    fn run_fails_on_a_completion_the_lifecycle_lost() {
        let rows = json!([
            lifecycle_row(0xd1, 100_000_000_000, 10, "deposited", None, None),
            lifecycle_row(
                0xd2,
                10_000_000_000,
                11,
                "withdrawn",
                Some((0xa2, 50)),
                Some(101)
            ),
            lifecycle_row(
                0xd3,
                10_000_000_000,
                12,
                "withdrawing",
                Some((0xa3, 60)),
                None
            ),
        ]);
        let stats = json!({
            "tipBlockNumber": ANCHOR, "activeDeposits": 3,
            "totalDeposited": "100000000000", "pendingWithdrawalCapacity": "20000000000"
        });
        // The cell layer saw the spend: the summary is right.
        let fx = fixture(rows, stats, 2);
        let result = DaoLiveCellsMatchNode
            .run(&fx.ctx, &ProgressReporter::new(None))
            .unwrap();
        assert_eq!(result.status, CheckStatus::Fail, "{result:?}");
        let entities: Vec<&str> = result.findings.iter().map(|f| f.entity.as_str()).collect();
        assert_eq!(
            entities,
            vec![
                outpoint_key(&hash(0xa3), 0).as_str(),
                format!("/dao/statistics @ block {ANCHOR}").as_str()
            ],
            "{result:?}"
        );
    }

    #[test]
    fn run_fails_when_the_live_cell_summary_miscounts() {
        let fx = fixture(consistent_rows(), consistent_stats(), 3);
        let result = DaoLiveCellsMatchNode
            .run(&fx.ctx, &ProgressReporter::new(None))
            .unwrap();
        assert_eq!(result.status, CheckStatus::Fail, "{result:?}");
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0].entity.starts_with("/cells/live-summary"));
    }

    #[test]
    fn run_is_inconclusive_without_a_source_declaration() {
        let mut fx = fixture(consistent_rows(), consistent_stats(), 2);
        fx.ctx.verify_source_path = Some(fx._dir.path().join("missing.toml"));
        let result = DaoLiveCellsMatchNode
            .run(&fx.ctx, &ProgressReporter::new(None))
            .unwrap();
        assert_eq!(result.status, CheckStatus::Inconclusive, "{result:?}");
    }
}
