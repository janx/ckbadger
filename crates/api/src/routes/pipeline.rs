//! `GET /api/v1/pipeline` — one snapshot of the transaction pipeline: the
//! recent chain as CKB structures it (header, proposal zone, uncles,
//! committed transactions), derived in ONE request under one pinned read view.
//!
//! Every derived field has exactly one computation, in this module's pure
//! functions, from chain data:
//! - a block's proposal zone is [`block_proposal_zone`] (own proposals, then
//!   the embedded uncles');
//! - a proposal's state comes from the commitment window of the consensus
//!   [`ProposalWindow`] and the committed transactions of the blocks in it;
//! - a committed transaction's `proposedIn` is the earliest proposal inside
//!   `[c - farthest, c - closest]` — the proposal the proposal reward pays;
//! - a cellbase pays block `X - (farthest + 1)`, cross-checked against the
//!   node's `get_block_economic_state(..).finalized_at`;
//! - a pool transaction's stage comes from the same zones, not from the node's
//!   pending/proposed labels (those only feed the P1 check), and only when
//!   the pool was read at ckbadger's sync tip (`status`). The node has a block
//!   a moment before ckbadger indexes it, so the request first waits, at most
//!   [`PIPELINE_TIP_WAIT`], for the two tips to agree; it holds no read view
//!   while it waits, and the snapshot is built under the one view whose tip
//!   ended the wait;
//! - NEXT is a prediction from the committable pool transactions and the
//!   consensus block limits.
//!
//! Chain-guaranteed invariants are checked, never repaired: a violation is a
//! 500 naming the invariant (C1–C5, P1, P3) and the block/transaction it broke
//! on.
//!
//! Read-only: store + node DB (`CkbChainReader`) reads and node RPC calls.

use axum::{extract::State, routing::get, Extension, Router};
use ckb_store_reader::CkbChainReader;
use ckb_types::prelude::*;
use ckbadger_store::read_view::{self, ReadViewGuard};
use ckbadger_store::{CachedBlockHeader, CkbadgerStore, TxIndexEntry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

use crate::cache::{CacheTtl, InMemoryCache};
use crate::consensus::{ConsensusParams, ProposalWindow};
use crate::pool::{HttpPoolSource, PoolSource, RawTxPool, TxPoolInfo};
use crate::response::{ok, ApiError, ApiResult, ApiRouteError};
use crate::routes::blocks::fetch_block_economic_state;
use crate::routes::proposal_window::{
    block_proposal_zone, earliest_proposal_in_window, resolve_committed_txs_with, ZoneProposal,
};
use crate::routes::transactions::{
    tx_cycles_status, tx_fee_rate, tx_fee_rate_value, tx_serialized_size_in_block, TxCyclesStatus,
};
use crate::{AppState, RequestReadView};

/// Blocks the snapshot shows, newest first (frontend `layout.ts` `G.maxBlocks`).
pub(crate) const PIPELINE_BLOCKS: i64 = 24;
/// Committed transactions served per block besides the cellbase: the most a
/// card can draw — its field is `BLOCK_FIELD_CAPACITY` = `G.cardCols ×
/// G.fieldRows` = 8 × 5 cells in frontend `layout.ts` and the cellbase takes
/// the first, so 39 1×1 squares. NEXT's card is the same size and lists the same.
pub(crate) const PIPELINE_BLOCK_TXS: usize = 39;

/// Pending pool transactions served: the pending card's body capacity,
/// `POOL_PENDING_CAPACITY` = `G.cardCols × G.poolRows` = 8 × 7 in frontend
/// `layout.ts`.
pub(crate) const PIPELINE_POOL_PENDING: usize = 56;
/// Proposed pool transactions served: the proposed card's body capacity,
/// `POOL_PROPOSED_CAPACITY` = `G.cardCols × G.poolRows` = 8 × 7 in frontend
/// `layout.ts`.
pub(crate) const PIPELINE_POOL_PROPOSED: usize = 56;

/// Genesis is the chain's first block; nothing lies below it.
const GENESIS: i64 = 0;

/// How long `/pipeline` waits for ckbadger's sync tip and the node's pool tip
/// to agree before it serves the chain alone, as `poolTipMismatch`.
///
/// The node attaches a block before ckbadger has it: the live indexer polls
/// the node once a second (`[indexer] poll_interval_ms` = 1000), writes the
/// block, and this API's secondaries see it at their next catch-up, which
/// also runs once a second. On mainnet the store view trails the node by
/// 1–2 s, and by up to ~3 s when both cadences line up badly; the pool can
/// trail too, since ckb updates it asynchronously after attaching a block. A
/// 3 s bound covers that normal lag, so a mismatch past it means the indexer
/// really is behind. It is also well under the 8–10 s mainnet block interval,
/// so a waited snapshot is still the newest one when it is served.
pub const PIPELINE_TIP_WAIT: Duration = Duration::from_secs(3);

/// How often a waiting `/pipeline` re-reads both tips.
///
/// The store view only advances at the API's 1 s catch-up, so polling much
/// faster buys nothing, while each attempt briefly pins a view and asks the
/// node for `tx_pool_info`. A quarter of the catch-up cadence serves a
/// snapshot within 250 ms of the catch-up that makes the tips agree, in at
/// most 13 attempts per waiting request (one at once, one per poll, the last
/// at the deadline).
const PIPELINE_TIP_POLL: Duration = Duration::from_millis(250);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/pipeline", get(get_pipeline))
}

// ---------------------------------------------------------------------------
// Wire types (the contract the frontend renders; camelCase)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineSnapshot {
    pub status: SnapshotStatus,
    pub consensus: PipelineConsensus,
    /// ckbadger's sync tip (store).
    pub tip: BlockRef,
    /// The node's tip as `tx_pool_info` reports it.
    pub pool_tip: BlockRef,
    /// Newest first, at most [`PIPELINE_BLOCKS`].
    pub blocks: Vec<PipelineBlock>,
    /// `None` unless `status` is `ok`.
    pub pool: Option<PipelinePool>,
    /// `None` unless `status` is `ok`.
    pub next: Option<PipelineNext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotStatus {
    /// The tx pool was read at ckbadger's sync tip: pool and chain are one view.
    Ok,
    /// The node's pool tip still differs from ckbadger's sync tip (number or
    /// hash), or moved while the pool was read, after [`PIPELINE_TIP_WAIT`]:
    /// pool stages cannot be derived from the chain the snapshot shows, and
    /// the indexer (or the node's pool) really is behind.
    PoolTipMismatch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelinePool {
    pub pending: Vec<PipelinePoolTx>,
    pub proposed: Vec<PipelinePoolTx>,
    /// Counts before truncation.
    pub pending_total: usize,
    pub proposed_total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelinePoolTx {
    pub hash: String,
    /// The node's entry size: serialized size in block (molecule + 4).
    pub size: u64,
    pub fee: String,
    /// Shannons per 1000 bytes, [`tx_fee_rate`].
    pub fee_rate: String,
    pub cycles: u64,
    /// When the node accepted it into its pool, ms.
    pub arrived_at: u64,
    /// Set exactly for the transactions in `proposed`.
    pub proposed_in: Option<ProposedIn>,
}

/// The predicted next block, cellbase excluded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineNext {
    /// Largest first, at most [`PIPELINE_BLOCK_TXS`] (as a block's `txs`).
    pub txs: Vec<NextTx>,
    /// Predicted transactions beyond `txs`.
    pub omitted_txs: usize,
    pub bytes: u64,
    pub cycles: u64,
    /// Proposals the next block can carry: unproposed pool transactions, up
    /// to `max_block_proposals_limit`.
    pub proposals: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextTx {
    pub hash: String,
    /// Serialized size in block (molecule + 4), as [`PipelinePoolTx::size`].
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConsensus {
    pub proposal_window_closest: i64,
    pub proposal_window_farthest: i64,
    pub max_block_bytes: u64,
    pub max_block_cycles: u64,
    pub max_block_proposals: u64,
}

impl From<&ConsensusParams> for PipelineConsensus {
    fn from(params: &ConsensusParams) -> Self {
        Self {
            proposal_window_closest: params.tx_proposal_window.closest(),
            proposal_window_farthest: params.tx_proposal_window.farthest(),
            max_block_bytes: params.max_block_bytes,
            max_block_cycles: params.max_block_cycles,
            max_block_proposals: params.max_block_proposals_limit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRef {
    pub number: i64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineBlock {
    pub number: i64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: String,
    /// Transactions in the block, cellbase included.
    pub tx_count: usize,
    /// Σ serialized size in block (molecule + 4) of every transaction,
    /// cellbase included.
    pub bytes: i64,
    pub cycles: Option<i64>,
    pub cycles_pending: bool,
    /// Zone order: the block's own proposals first, then its uncles'.
    pub proposals: Vec<PipelineProposal>,
    pub uncles: Vec<BlockRef>,
    /// Largest first, at most [`PIPELINE_BLOCK_TXS`]; the cellbase is served
    /// separately.
    pub txs: Vec<PipelineTx>,
    /// `tx_count - 1 - txs.len()`.
    pub omitted_txs: usize,
    pub cellbase: PipelineCellbase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalState {
    /// Proposed in a block too recent for the next block to commit it.
    Early,
    /// Committable now and still inside its window.
    Waiting,
    Committed,
    /// Its window closed without a commit.
    Expired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProposal {
    pub short_id: String,
    /// Index into [`PipelineBlock::uncles`]; `None` for the block's own.
    pub uncle_index: Option<usize>,
    pub state: ProposalState,
    pub tx_hash: Option<String>,
    pub committed_block: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedIn {
    pub block: i64,
    pub uncle_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineTx {
    pub hash: String,
    pub size: i32,
    pub fee: String,
    /// Shannons per 1000 bytes, [`tx_fee_rate`].
    pub fee_rate: String,
    pub cycles: Option<i64>,
    pub cycles_status: TxCyclesStatus,
    pub proposed_in: ProposedIn,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineCellbase {
    pub hash: String,
    pub size: i32,
    /// The block whose reward this cellbase pays; `None` while that would be
    /// genesis or earlier (those blocks' cellbases pay nothing).
    pub pays: Option<BlockRef>,
}

fn hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

// ---------------------------------------------------------------------------
// Chain loading (store + node DB, under the request's pinned read view)
// ---------------------------------------------------------------------------

/// What one block contributes, as the node DB holds it. Immutable per block
/// hash, so it is cached by hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeBody {
    /// `(number, hash)` of each embedded uncle, in embedding order.
    uncles: Vec<(i64, [u8; 32])>,
    zone: Vec<ZoneProposal>,
    /// Transaction hashes in block order; index 0 is the cellbase.
    tx_hashes: Vec<[u8; 32]>,
}

fn node_body_cache_key(hash: &[u8; 32]) -> String {
    format!("pipeline:node-body:{}", hex::encode(hash))
}

/// A loaded block: the store header, the node body, and — for the blocks the
/// snapshot shows — the store's per-transaction rows, index-aligned with
/// `body.tx_hashes`.
#[derive(Debug, Clone)]
struct LoadedBlock {
    number: i64,
    hash: [u8; 32],
    header: CachedBlockHeader,
    body: NodeBody,
    txs: Option<Vec<TxIndexEntry>>,
}

/// The chain the snapshot is derived from: the shown blocks plus the
/// `farthest` older blocks whose proposal zones the oldest shown block's
/// transactions may have been proposed in.
#[derive(Debug, Clone)]
struct LoadedChain {
    tip: i64,
    /// Ascending, contiguous, ending at `tip`.
    blocks: BTreeMap<i64, LoadedBlock>,
}

impl LoadedChain {
    /// Shown blocks, newest first. Genesis is never shown: it is not mined, and
    /// it neither proposes nor commits a proposed transaction.
    fn shown(&self) -> impl Iterator<Item = &LoadedBlock> {
        self.blocks
            .values()
            .rev()
            .take_while(|b| b.number > self.tip - PIPELINE_BLOCKS && b.number > GENESIS)
    }
}

fn hash32(bytes: &[u8], what: &str) -> Result<[u8; 32], ApiRouteError> {
    <[u8; 32]>::try_from(bytes)
        .map_err(|_| ApiError::internal(format!("{what} is {} bytes, not 32", bytes.len())))
}

/// ckbadger's sync tip, as one pinned read view reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncTip {
    number: i64,
    hash: [u8; 32],
}

fn read_sync_tip(store: &CkbadgerStore) -> Result<SyncTip, ApiRouteError> {
    let sync = store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {e}")))?;
    if sync.tip_block_hash.is_empty() {
        return Err(ApiError::initializing(
            "no block indexed yet (indexer still starting?)",
        ));
    }
    Ok(SyncTip {
        number: sync.tip_block_number,
        hash: hash32(&sync.tip_block_hash, "sync tip hash")?,
    })
}

/// One pinned read view and ckbadger's sync tip as read under it. Every store
/// and node-DB read of one snapshot happens while this is alive, so the whole
/// chain section is one view.
struct PinnedTip {
    tip: SyncTip,
    _view: ReadViewGuard,
}

/// Pin a fresh read view and read the sync tip under it.
async fn pin_sync_tip(store: &Arc<CkbadgerStore>) -> Result<PinnedTip, ApiRouteError> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || {
        let view = read_view::acquire_read();
        let tip = read_sync_tip(&store)?;
        Ok(PinnedTip { tip, _view: view })
    })
    .await
    .map_err(|e| ApiError::internal(format!("pipeline sync tip read panicked: {e}")))?
}

/// Load the chain ending at `tip`, the sync tip read under the caller's pinned
/// view (the one the pool read was compared against).
fn load_chain(
    store: &CkbadgerStore,
    ckb: &CkbChainReader,
    mem_cache: &InMemoryCache,
    window: ProposalWindow,
    sync_tip: SyncTip,
) -> Result<LoadedChain, ApiRouteError> {
    let SyncTip {
        number: tip,
        hash: tip_hash,
    } = sync_tip;

    let span = (PIPELINE_BLOCKS - 1) + window.farthest();
    let lowest = (tip - span).max(GENESIS);
    let lowest_shown = tip - PIPELINE_BLOCKS + 1;

    let mut blocks = BTreeMap::new();
    for number in lowest..=tip {
        let header = store
            .get_block_header(number)
            .map_err(|e| ApiError::internal(format!("block {number} header read failed: {e}")))?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "store has no header for block {number} below its sync tip {tip}"
                ))
            })?;
        let hash = hash32(&header.hash, &format!("block {number} hash"))?;
        let body = load_node_body(ckb, mem_cache, number, &hash)?;
        let txs = if number >= lowest_shown && number > GENESIS {
            Some(load_store_txs(store, number, &hash, &header, &body)?)
        } else {
            None
        };
        blocks.insert(
            number,
            LoadedBlock {
                number,
                hash,
                header,
                body,
                txs,
            },
        );
    }

    let tip_block = &blocks[&tip];
    if tip_block.hash != tip_hash {
        return Err(ApiError::internal(format!(
            "C5: sync tip is block {tip} {} but the store header at {tip} is {}",
            hex(&tip_hash),
            hex(&tip_block.hash)
        )));
    }

    Ok(LoadedChain { tip, blocks })
}

/// C1: every block the snapshot reads is in the node DB under the hash the
/// store indexed it by, at the same height.
fn load_node_body(
    ckb: &CkbChainReader,
    mem_cache: &InMemoryCache,
    number: i64,
    hash: &[u8; 32],
) -> Result<NodeBody, ApiRouteError> {
    let key = node_body_cache_key(hash);
    if let Some(cached) = mem_cache.get::<NodeBody>(&key) {
        return Ok(cached);
    }
    let block = ckb.get_block(hash).ok_or_else(|| {
        ApiError::internal(format!(
            "C1: block {number} {} is indexed but not in the node DB",
            hex(hash)
        ))
    })?;
    if block.number() as i64 != number {
        return Err(ApiError::internal(format!(
            "C1: block {} is at height {} in the node DB but at {number} in the store",
            hex(hash),
            block.number()
        )));
    }
    let body = NodeBody {
        uncles: block
            .uncles()
            .into_iter()
            .map(|uncle| (uncle.number() as i64, uncle.hash().unpack()))
            .collect(),
        zone: block_proposal_zone(&block),
        tx_hashes: block
            .transactions()
            .iter()
            .map(|tx| tx.hash().unpack())
            .collect(),
    };
    mem_cache.set(&key, &body, CacheTtl::BLOCK);
    Ok(body)
}

/// The store's per-transaction rows of a shown block. They must line up with
/// the node block one to one: the hash comes from the node, size/fee/cycles
/// from the store, joined by index.
fn load_store_txs(
    store: &CkbadgerStore,
    number: i64,
    hash: &[u8; 32],
    header: &CachedBlockHeader,
    body: &NodeBody,
) -> Result<Vec<TxIndexEntry>, ApiRouteError> {
    let rows = store
        .list_block_txs(number)
        .map_err(|e| ApiError::internal(format!("block {number} tx rows read failed: {e}")))?;
    let coherent = rows.len() == body.tx_hashes.len()
        && usize::try_from(header.transactions_count) == Ok(rows.len())
        && rows
            .iter()
            .enumerate()
            .all(|(i, (tx_idx, _))| usize::try_from(*tx_idx) == Ok(i));
    if !coherent {
        return Err(ApiError::internal(format!(
            "C1: block {number} {}: the store indexes {} transaction rows (header says {}), \
             the node block has {}",
            hex(hash),
            rows.len(),
            header.transactions_count,
            body.tx_hashes.len()
        )));
    }
    Ok(rows.into_iter().map(|(_, entry)| entry).collect())
}

// ---------------------------------------------------------------------------
// Derivation (pure)
// ---------------------------------------------------------------------------

/// The state of a proposal made in `proposer` that `commit` (if any) resolved,
/// seen from `tip`.
fn proposal_state(
    window: ProposalWindow,
    tip: i64,
    proposer: i64,
    committed: bool,
) -> ProposalState {
    if committed {
        ProposalState::Committed
    } else if proposer + window.closest() > tip + 1 {
        ProposalState::Early
    } else if proposer + window.farthest() > tip {
        ProposalState::Waiting
    } else {
        ProposalState::Expired
    }
}

/// Build the shown blocks, newest first. `pays` maps each shown block number to
/// its verified reward target. Fails with the invariant (C2, C3, C5) and the
/// block/transaction it broke on.
fn build_chain(
    window: ProposalWindow,
    chain: &LoadedChain,
    pays: &HashMap<i64, Option<BlockRef>>,
) -> Result<Vec<PipelineBlock>, String> {
    // C5: the blocks read form one chain.
    for (parent, block) in chain.blocks.values().zip(chain.blocks.values().skip(1)) {
        if block.header.parent_hash != parent.hash {
            return Err(format!(
                "C5: block {} {} names parent {} but block {} is {}",
                block.number,
                hex(&block.hash),
                hex(&block.header.parent_hash),
                parent.number,
                hex(&parent.hash)
            ));
        }
    }

    let tx_hashes_at = |n: i64| -> Result<Option<&[[u8; 32]]>, String> {
        if n > chain.tip {
            return Ok(None);
        }
        chain
            .blocks
            .get(&n)
            .map(|b| Some(b.body.tx_hashes.as_slice()))
            .ok_or_else(|| format!("block {n} inside a commitment window was not loaded"))
    };
    let zone_at = |n: i64| -> Result<Option<&[ZoneProposal]>, String> {
        chain
            .blocks
            .get(&n)
            .map(|b| Some(b.body.zone.as_slice()))
            .ok_or_else(|| format!("block {n} inside a proposal window was not loaded"))
    };

    let mut shown = Vec::new();
    for block in chain.shown() {
        let number = block.number;
        let txs = block
            .txs
            .as_ref()
            .ok_or_else(|| format!("shown block {number} was loaded without its transactions"))?;

        // C3: exactly one cellbase, at index 0.
        let cellbase_at_zero = txs.first().is_some_and(|entry| entry.is_cellbase);
        let extra_cellbase = txs.iter().skip(1).position(|entry| entry.is_cellbase);
        if !cellbase_at_zero || extra_cellbase.is_some() {
            return Err(format!(
                "C3: block {number} {} must have exactly one cellbase, at index 0 \
                 (index 0 cellbase: {cellbase_at_zero}, another cellbase at index: {:?})",
                hex(&block.hash),
                extra_cellbase.map(|i| i + 1)
            ));
        }

        // Proposals and their commitments.
        let short_ids: Vec<Vec<u8>> = block.body.zone.iter().map(|p| p.short_id.clone()).collect();
        let commitments = resolve_committed_txs_with(window, number, &short_ids, &tx_hashes_at)?;
        let proposals = block
            .body
            .zone
            .iter()
            .zip(commitments)
            .map(|(proposal, commitment)| PipelineProposal {
                short_id: hex(&proposal.short_id),
                uncle_index: proposal.uncle.as_ref().map(|u| u.index),
                state: proposal_state(window, chain.tip, number, commitment.is_some()),
                tx_hash: commitment.as_ref().map(|(tx_hash, _)| hex(tx_hash)),
                committed_block: commitment.map(|(_, block)| block),
            })
            .collect();

        // Committed transactions.
        let mut bytes: i64 = 0;
        let mut any_failed = false;
        let mut committed = Vec::with_capacity(txs.len());
        for (index, (entry, tx_hash)) in txs.iter().zip(&block.body.tx_hashes).enumerate() {
            let size = tx_serialized_size_in_block(entry.tx_size);
            bytes += i64::from(size);
            if index == 0 {
                continue;
            }
            let status = tx_cycles_status(entry.cycles);
            any_failed |= status == TxCyclesStatus::Failed;
            // C2: a committed transaction was proposed inside its window.
            let hit = earliest_proposal_in_window(window, number, &tx_hash[..10], &zone_at)?
                .ok_or_else(|| {
                    format!(
                        "C2: block {number} commits {} but no block in [{}, {}] proposed it",
                        hex(tx_hash),
                        number - window.farthest(),
                        number - window.closest()
                    )
                })?;
            let fee = u128::try_from(entry.fee).map_err(|_| {
                format!(
                    "block {number} tx {} has negative fee {}",
                    hex(tx_hash),
                    entry.fee
                )
            })?;
            committed.push((
                index,
                PipelineTx {
                    hash: hex(tx_hash),
                    size,
                    fee: entry.fee.to_string(),
                    fee_rate: tx_fee_rate(fee, size),
                    cycles: (status == TxCyclesStatus::Done)
                        .then_some(entry.cycles)
                        .flatten(),
                    cycles_status: status,
                    proposed_in: ProposedIn {
                        block: hit.block_number,
                        uncle_index: hit.uncle.map(|u| u.index),
                    },
                },
            ));
        }
        committed.sort_by(|(ia, a), (ib, b)| b.size.cmp(&a.size).then(ia.cmp(ib)));
        let non_cellbase = committed.len();
        committed.truncate(PIPELINE_BLOCK_TXS);
        let omitted_txs = non_cellbase - committed.len();

        let cellbase_hash = block.body.tx_hashes[0];
        let timestamp = chrono::DateTime::from_timestamp_millis(block.header.timestamp)
            .ok_or_else(|| {
                format!(
                    "block {number} timestamp {} ms is out of range",
                    block.header.timestamp
                )
            })?
            .to_rfc3339();

        shown.push(PipelineBlock {
            number,
            hash: hex(&block.hash),
            parent_hash: hex(&block.header.parent_hash),
            timestamp,
            tx_count: txs.len(),
            bytes,
            cycles: block.header.cycles,
            cycles_pending: block.header.cycles.is_none() && !any_failed,
            proposals,
            uncles: block
                .body
                .uncles
                .iter()
                .map(|(number, hash)| BlockRef {
                    number: *number,
                    hash: hex(hash),
                })
                .collect(),
            txs: committed.into_iter().map(|(_, tx)| tx).collect(),
            omitted_txs,
            cellbase: PipelineCellbase {
                hash: hex(&cellbase_hash),
                size: tx_serialized_size_in_block(txs[0].tx_size),
                pays: pays
                    .get(&number)
                    .cloned()
                    .ok_or_else(|| format!("reward target of block {number} was not resolved"))?,
            },
        });
    }
    Ok(shown)
}

// ---------------------------------------------------------------------------
// Cellbase reward targets (node RPC, cached per block hash)
// ---------------------------------------------------------------------------

fn pays_cache_key(hash: &[u8; 32]) -> String {
    format!("pipeline:pays:{}", hex::encode(hash))
}

/// The block whose reward `block`'s cellbase pays: `X - (farthest + 1)`, the
/// finalization delay. Blocks whose target would be genesis or earlier pay
/// nothing. C4: the node must report that target finalized AT `block`.
async fn resolve_pays(
    state: &AppState,
    window: ProposalWindow,
    chain: &LoadedChain,
    block: &LoadedBlock,
) -> Result<Option<BlockRef>, ApiRouteError> {
    let target = block.number - (window.farthest() + 1);
    if target <= GENESIS {
        return Ok(None);
    }
    let key = pays_cache_key(&block.hash);
    if let Some(cached) = state.mem_cache.get::<BlockRef>(&key) {
        return Ok(Some(cached));
    }
    let target_hash = match chain.blocks.get(&target) {
        Some(loaded) => loaded.hash,
        None => {
            let header = state
                .store
                .get_block_header(target)
                .map_err(|e| ApiError::internal(format!("block {target} header read failed: {e}")))?
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "store has no header for block {target}, the reward target of block {}",
                        block.number
                    ))
                })?;
            hash32(&header.hash, &format!("block {target} hash"))?
        }
    };
    let economic_state = fetch_block_economic_state(&state.ckb_rpc_url, &target_hash)
        .await?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "C4: block {} {} is indexed, yet the node reports its reward target \
                 block {target} {} as not finalized",
                block.number,
                hex(&block.hash),
                hex(&target_hash)
            ))
        })?;
    if economic_state.finalized_at != block.hash {
        return Err(ApiError::internal(format!(
            "C4: block {target} {} is the reward target of block {} {}, but the node \
             reports it finalized at {}",
            hex(&target_hash),
            block.number,
            hex(&block.hash),
            hex(&economic_state.finalized_at)
        )));
    }
    let pays = BlockRef {
        number: target,
        hash: hex(&target_hash),
    };
    state.mem_cache.set(&key, &pays, CacheTtl::BLOCK);
    Ok(Some(pays))
}

// ---------------------------------------------------------------------------
// Tx pool section (pure)
// ---------------------------------------------------------------------------

/// How the node labels a pool transaction. Used only to check P1: the stage
/// the snapshot serves is derived from the chain's proposal zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum RpcPoolStage {
    Pending,
    Proposed,
}

/// One verbose `get_raw_tx_pool` entry, every field the node's own value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PoolEntry {
    hash: [u8; 32],
    rpc_stage: RpcPoolStage,
    /// The node's entry size — serialized size in block (molecule + 4), the
    /// convention `tx_serialized_size_in_block` gives committed transactions.
    size: u64,
    fee: u64,
    cycles: u64,
    arrived_at_ms: u64,
}

impl PoolEntry {
    fn short_id(&self) -> &[u8] {
        &self.hash[..10]
    }
}

/// A pool transaction with its chain-derived stage.
#[derive(Debug, Clone)]
struct ClassifiedPoolTx {
    entry: PoolEntry,
    /// The proposal that stages it: the latest one the next block may commit
    /// from, else the latest one still too recent (the node's "gap").
    proposed_in: Option<ProposedIn>,
    /// The next block (tip + 1) may commit it.
    committable: bool,
    /// Shannons per 1000 bytes — the served `feeRate`, numerically.
    fee_rate: u128,
}

/// Stage every pool transaction from the proposal zones of the blocks whose
/// proposals are still open at `tip`, and check the node's "proposed" label
/// against that (P1): a node lists as proposed only what the next block may
/// commit (`[tip + 1 - farthest, tip + 1 - closest]`).
///
/// The converse is not an invariant. A node with `[block_assembler]` moves
/// committable entries to proposed and lists its gap (`(tip + 1 - closest,
/// tip]`) as pending, but a node without one never moves anything out of
/// pending (ckb v0.209 `tx-pool/src/process.rs` `_update_tx_pool_for_reorg`,
/// `mine_mode`), so a pending label says nothing about proposals.
fn classify_pool<'z>(
    window: ProposalWindow,
    tip: i64,
    zone_at: impl Fn(i64) -> Result<&'z [ZoneProposal], String>,
    entries: Vec<PoolEntry>,
) -> Result<Vec<ClassifiedPoolTx>, String> {
    let committable_lo = tip + 1 - window.farthest();
    let committable_hi = tip + 1 - window.closest();

    // short id -> every open proposal of it, `(block, uncle index)`.
    let mut open: HashMap<&[u8], Vec<ProposedIn>> = HashMap::new();
    for number in committable_lo.max(GENESIS)..=tip {
        for proposal in zone_at(number)? {
            open.entry(proposal.short_id.as_slice())
                .or_default()
                .push(ProposedIn {
                    block: number,
                    uncle_index: proposal.uncle.as_ref().map(|u| u.index),
                });
        }
    }

    entries
        .into_iter()
        .map(|entry| {
            let hits = open.get(entry.short_id()).map(Vec::as_slice).unwrap_or(&[]);
            let latest = |committable: bool| {
                hits.iter()
                    .filter(|hit| (hit.block <= committable_hi) == committable)
                    .max_by_key(|hit| hit.block)
                    .copied()
            };
            let committable_hit = latest(true);
            let gap_hit = latest(false);
            let tx = hex(&entry.hash);
            if entry.rpc_stage == RpcPoolStage::Proposed && committable_hit.is_none() {
                return Err(format!(
                    "P1: the node lists {tx} as proposed at tip {tip}, but no block in \
                     [{committable_lo}, {committable_hi}] proposed it"
                ));
            }
            if entry.size == 0 {
                return Err(format!("the node reports pool tx {tx} with size 0"));
            }
            let fee_rate = tx_fee_rate_value(u128::from(entry.fee), pool_size_i32(&entry)?);
            Ok(ClassifiedPoolTx {
                proposed_in: committable_hit.or(gap_hit),
                committable: committable_hit.is_some(),
                fee_rate,
                entry,
            })
        })
        .collect()
}

fn pool_size_i32(entry: &PoolEntry) -> Result<i32, String> {
    i32::try_from(entry.size).map_err(|_| {
        format!(
            "the node reports pool tx {} with size {}, beyond any block",
            hex(&entry.hash),
            entry.size
        )
    })
}

/// Fee rate desc, then arrival asc, then hash — the order the next block's
/// packer is predicted to take committable transactions in.
fn by_fee_rate_then_arrival(a: &ClassifiedPoolTx, b: &ClassifiedPoolTx) -> std::cmp::Ordering {
    b.fee_rate
        .cmp(&a.fee_rate)
        .then(a.entry.arrived_at_ms.cmp(&b.entry.arrived_at_ms))
        .then(a.entry.hash.cmp(&b.entry.hash))
}

/// Predict the next block's transactions (cellbase excluded): the
/// committable pool transactions by fee rate desc / arrival asc, each taken
/// when it still fits `max_block_bytes` and `max_block_cycles` and skipped
/// when it does not (the packer keeps scanning). `proposals` is how many
/// unproposed transactions the next block can propose.
fn predict_next(
    consensus: &ConsensusParams,
    classified: &[ClassifiedPoolTx],
    pending_total: usize,
) -> Result<PipelineNext, String> {
    let mut candidates: Vec<&ClassifiedPoolTx> =
        classified.iter().filter(|tx| tx.committable).collect();
    candidates.sort_by(|a, b| by_fee_rate_then_arrival(a, b));

    let (mut bytes, mut cycles) = (0u64, 0u64);
    let mut picked: Vec<NextTx> = Vec::new();
    for tx in candidates {
        let with_bytes = bytes
            .checked_add(tx.entry.size)
            .ok_or_else(|| "predicted next-block bytes overflow u64".to_string())?;
        let with_cycles = cycles
            .checked_add(tx.entry.cycles)
            .ok_or_else(|| "predicted next-block cycles overflow u64".to_string())?;
        if with_bytes > consensus.max_block_bytes || with_cycles > consensus.max_block_cycles {
            continue;
        }
        bytes = with_bytes;
        cycles = with_cycles;
        picked.push(NextTx {
            hash: hex(&tx.entry.hash),
            size: tx.entry.size,
        });
    }

    // Served like a block's transactions: largest first (stable, so equal
    // sizes keep their packing order), capped at the card's capacity.
    picked.sort_by_key(|tx| std::cmp::Reverse(tx.size));
    let predicted = picked.len();
    picked.truncate(PIPELINE_BLOCK_TXS);
    let omitted_txs = predicted - picked.len();

    let pending = u64::try_from(pending_total)
        .map_err(|_| format!("pending pool count {pending_total} does not fit u64"))?;
    Ok(PipelineNext {
        txs: picked,
        omitted_txs,
        bytes,
        cycles,
        proposals: pending.min(consensus.max_block_proposals_limit),
    })
}

fn pool_tx(tx: &ClassifiedPoolTx) -> PipelinePoolTx {
    PipelinePoolTx {
        hash: hex(&tx.entry.hash),
        size: tx.entry.size,
        fee: tx.entry.fee.to_string(),
        fee_rate: tx.fee_rate.to_string(),
        cycles: tx.entry.cycles,
        arrived_at: tx.entry.arrived_at_ms,
        proposed_in: tx.proposed_in,
    }
}

/// The pool grids: pending oldest first; proposed by newest proposal, then
/// oldest arrival. Truncated to the grids' capacities with the totals kept.
fn build_pool(classified: &[ClassifiedPoolTx]) -> PipelinePool {
    let mut pending: Vec<&ClassifiedPoolTx> = classified
        .iter()
        .filter(|tx| tx.proposed_in.is_none())
        .collect();
    let mut proposed: Vec<&ClassifiedPoolTx> = classified
        .iter()
        .filter(|tx| tx.proposed_in.is_some())
        .collect();
    pending.sort_by(|a, b| {
        a.entry
            .arrived_at_ms
            .cmp(&b.entry.arrived_at_ms)
            .then(a.entry.hash.cmp(&b.entry.hash))
    });
    proposed.sort_by(|a, b| {
        let block = |tx: &ClassifiedPoolTx| tx.proposed_in.map(|p| p.block);
        block(b)
            .cmp(&block(a))
            .then(a.entry.arrived_at_ms.cmp(&b.entry.arrived_at_ms))
            .then(a.entry.hash.cmp(&b.entry.hash))
    });
    let (pending_total, proposed_total) = (pending.len(), proposed.len());
    PipelinePool {
        pending: pending
            .into_iter()
            .take(PIPELINE_POOL_PENDING)
            .map(pool_tx)
            .collect(),
        proposed: proposed
            .into_iter()
            .take(PIPELINE_POOL_PROPOSED)
            .map(pool_tx)
            .collect(),
        pending_total,
        proposed_total,
    }
}

/// Name, on every not-yet-committed proposal of a shown block, the pool
/// transaction it proposes; then check P3: every staged pool transaction is
/// named by the proposal its `proposedIn` points at (block and uncle).
fn link_pool_to_proposals(
    blocks: &mut [PipelineBlock],
    classified: &[ClassifiedPoolTx],
) -> Result<(), String> {
    let mut by_short_id: HashMap<String, &ClassifiedPoolTx> = HashMap::new();
    for tx in classified {
        if let Some(other) = by_short_id.insert(hex(tx.entry.short_id()), tx) {
            return Err(format!(
                "pool transactions {} and {} share proposal short id {}",
                hex(&other.entry.hash),
                hex(&tx.entry.hash),
                hex(tx.entry.short_id())
            ));
        }
    }
    for block in blocks.iter_mut() {
        for proposal in block.proposals.iter_mut() {
            if proposal.tx_hash.is_none() {
                proposal.tx_hash = by_short_id
                    .get(&proposal.short_id)
                    .map(|tx| hex(&tx.entry.hash));
            }
        }
    }
    for tx in classified {
        let Some(proposed_in) = tx.proposed_in else {
            continue;
        };
        let tx_hash = hex(&tx.entry.hash);
        let short_id = hex(tx.entry.short_id());
        let named = blocks
            .iter()
            .find(|b| b.number == proposed_in.block)
            .and_then(|b| b.proposals.iter().find(|p| p.short_id == short_id))
            .is_some_and(|p| {
                p.uncle_index == proposed_in.uncle_index && p.tx_hash.as_deref() == Some(&tx_hash)
            });
        if !named {
            return Err(format!(
                "P3: pool tx {tx_hash} is staged by block {} (uncle {:?}), but that block's \
                 proposal {short_id} does not name it",
                proposed_in.block, proposed_in.uncle_index
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tx pool read (node RPC, cached 2 s per ckbadger tip) and the tip wait
// ---------------------------------------------------------------------------

/// What one bracketed read of the node's tx pool saw: `tx_pool_info`, the
/// verbose pool, `tx_pool_info` again.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PoolRead {
    /// The node's tip was ckbadger's sync tip before and after the verbose
    /// pool was read: the pool belongs to the chain the snapshot shows.
    AtTip(Vec<PoolEntry>),
    /// The node's tip is not ckbadger's sync tip (number or hash), or moved
    /// while the pool was read. The node's tip as the last `tx_pool_info` of
    /// the read reported it.
    Elsewhere { number: u64, hash: [u8; 32] },
}

/// Only reads at a tip are cached, for the mempool endpoints' 2 s, under that
/// whole tip (number and hash): a hit is a pool read at exactly the tip the
/// snapshot's chain ends at, never one from another block or fork. A read
/// elsewhere is not cached — it is what the tip wait is waiting to change.
fn pool_read_cache_key(tip: &SyncTip) -> String {
    format!("pipeline:pool:{}:{}", tip.number, hex::encode(tip.hash))
}

fn pool_entries(raw: RawTxPool) -> Vec<PoolEntry> {
    let stage = |stage: RpcPoolStage| {
        move |(hash, meta): ([u8; 32], crate::pool::PoolEntryMeta)| PoolEntry {
            hash,
            rpc_stage: stage,
            size: meta.size,
            fee: meta.fee,
            cycles: meta.cycles,
            arrived_at_ms: meta.time_added_to_pool_ms,
        }
    };
    raw.pending
        .into_iter()
        .map(stage(RpcPoolStage::Pending))
        .chain(raw.proposed.into_iter().map(stage(RpcPoolStage::Proposed)))
        .collect()
}

async fn read_pool(
    source: &dyn PoolSource,
    mem_cache: &InMemoryCache,
    tip: &SyncTip,
) -> Result<PoolRead, ApiRouteError> {
    let key = pool_read_cache_key(tip);
    if let Some(entries) = mem_cache.get::<Vec<PoolEntry>>(&key) {
        return Ok(PoolRead::AtTip(entries));
    }
    let node = |e: String| ApiError::internal(format!("tx pool read failed: {e}"));
    let at_tip = |info: &TxPoolInfo| {
        i64::try_from(info.tip_number) == Ok(tip.number) && info.tip_hash == tip.hash
    };
    let elsewhere = |info: TxPoolInfo| PoolRead::Elsewhere {
        number: info.tip_number,
        hash: info.tip_hash,
    };

    let before = source.tx_pool_info().await.map_err(node)?;
    if !at_tip(&before) {
        return Ok(elsewhere(before));
    }
    let raw = source.raw_tx_pool_verbose().await.map_err(node)?;
    // A block landing during the read leaves the verbose pool belonging to
    // either tip: not a read at ckbadger's tip.
    let after = source.tx_pool_info().await.map_err(node)?;
    if !at_tip(&after) {
        return Ok(elsewhere(after));
    }
    let entries = pool_entries(raw);
    mem_cache.set(&key, &entries, CacheTtl::MEMPOOL_INFO);
    Ok(PoolRead::AtTip(entries))
}

/// Wait, at most [`PIPELINE_TIP_WAIT`], for ckbadger's sync tip and the
/// node's pool tip to agree, then hand back the view to build the snapshot
/// under and the pool read made against it.
///
/// Each attempt pins a fresh view, reads the sync tip under it and reads the
/// pool against that tip. Tips that agree (number and hash) end the wait at
/// once. Otherwise the attempt's view is dropped before sleeping, because a
/// pinned view blocks the catch-up the wait is for, and the next attempt pins
/// a new one. One loop covers every way the tips part: the node ahead of the
/// store (the indexer has not written the block yet), the store ahead of the
/// node (ckb updates its pool after attaching a block), and the same height on
/// two forks (a reorg race). An attempt at the deadline that still disagrees
/// is returned as it is: the snapshot is then built under its view, as a
/// `poolTipMismatch`.
async fn await_pool_at_sync_tip(
    store: &Arc<CkbadgerStore>,
    mem_cache: &InMemoryCache,
    source: &dyn PoolSource,
) -> Result<(PinnedTip, PoolRead), ApiRouteError> {
    let deadline = Instant::now() + PIPELINE_TIP_WAIT;
    loop {
        let pinned = pin_sync_tip(store).await?;
        let read = read_pool(source, mem_cache, &pinned.tip).await?;
        let now = Instant::now();
        if matches!(read, PoolRead::AtTip(_)) || now >= deadline {
            return Ok((pinned, read));
        }
        drop(pinned);
        tokio::time::sleep_until(deadline.min(now + PIPELINE_TIP_POLL)).await;
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn get_pipeline(
    State(state): State<Arc<AppState>>,
    // Optional so the handler still works in routers built without the
    // read-view middleware.
    read_view: Option<Extension<RequestReadView>>,
) -> ApiResult<PipelineSnapshot> {
    // The tip wait must let catch-up run, so the request's own pin goes
    // before anything is read: each attempt pins its own view, and the
    // snapshot is built under the view of the attempt that ends the wait.
    if let Some(Extension(view)) = read_view {
        view.release();
    }
    let consensus = state.consensus().await?.clone();
    let ckb = state.ckb_store.clone().ok_or_else(|| {
        ApiError::internal(
            "/pipeline reads proposal zones and uncles from the node DB; \
             configure ckb_db_path so the API can open it",
        )
    })?;
    let source = HttpPoolSource::new(state.ckb_rpc_url.clone());
    let (pinned, pool_read) =
        await_pool_at_sync_tip(&state.store, &state.mem_cache, &source).await?;
    ok(build_snapshot(&state, &consensus, ckb, pinned, pool_read).await?)
}

/// Build the snapshot at `pinned`'s sync tip, every store and node-DB read
/// under its view, with the pool section from `pool_read`.
async fn build_snapshot(
    state: &AppState,
    consensus: &ConsensusParams,
    ckb: Arc<CkbChainReader>,
    pinned: PinnedTip,
    pool_read: PoolRead,
) -> Result<PipelineSnapshot, ApiRouteError> {
    let window = consensus.tx_proposal_window;
    let chain = {
        let store = state.store.clone();
        let mem_cache = state.mem_cache.clone();
        let tip = pinned.tip;
        tokio::task::spawn_blocking(move || load_chain(&store, &ckb, &mem_cache, window, tip))
            .await
            .map_err(|e| ApiError::internal(format!("pipeline chain load panicked: {e}")))??
    };

    let mut pays = HashMap::new();
    for block in chain.shown() {
        pays.insert(
            block.number,
            resolve_pays(state, window, &chain, block).await?,
        );
    }
    // Every store read of the snapshot is done.
    drop(pinned);

    let mut blocks = build_chain(window, &chain, &pays).map_err(ApiError::internal)?;

    // Same contract as /blocks/{id}/fee-stats: a block whose cycles are not
    // known yet asks the indexer's cycles worker to compute them.
    if state.cycles_client.is_enabled() {
        for block in blocks.iter().filter(|b| b.cycles_pending) {
            let client = state.cycles_client.clone();
            let number = block.number;
            tokio::spawn(async move {
                if let Err(e) = client.enqueue_block(number).await {
                    tracing::debug!("Failed to enqueue block {number} cycles request: {e}");
                }
            });
        }
    }

    let tip = BlockRef {
        number: chain.tip,
        hash: hex(&chain.blocks[&chain.tip].hash),
    };
    let (status, pool_tip, pool, next) = match pool_read {
        PoolRead::Elsewhere { number, hash } => {
            let pool_tip = BlockRef {
                number: i64::try_from(number).map_err(|_| {
                    ApiError::internal(format!("tx_pool_info tip number {number} does not fit i64"))
                })?,
                hash: hex(&hash),
            };
            (SnapshotStatus::PoolTipMismatch, pool_tip, None, None)
        }
        PoolRead::AtTip(entries) => {
            let zone_at = |n: i64| {
                chain
                    .blocks
                    .get(&n)
                    .map(|b| b.body.zone.as_slice())
                    .ok_or_else(|| format!("block {n} with open proposals was not loaded"))
            };
            let classified =
                classify_pool(window, chain.tip, zone_at, entries).map_err(ApiError::internal)?;
            link_pool_to_proposals(&mut blocks, &classified).map_err(ApiError::internal)?;
            let pool = build_pool(&classified);
            let next = predict_next(consensus, &classified, pool.pending_total)
                .map_err(ApiError::internal)?;
            (SnapshotStatus::Ok, tip.clone(), Some(pool), Some(next))
        }
    };

    Ok(PipelineSnapshot {
        status,
        consensus: PipelineConsensus::from(consensus),
        tip,
        pool_tip,
        blocks,
        pool,
        next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::fake::FakePoolSource;
    use crate::pool::PoolEntryMeta;

    fn mainnet_window() -> ProposalWindow {
        ProposalWindow::new(2, 10).unwrap()
    }

    #[test]
    fn proposal_state_follows_the_window_from_the_tip() {
        let w = mainnet_window();
        let tip = 100;
        // Proposed at the tip: the next block (tip + 1) is too early.
        assert_eq!(proposal_state(w, tip, 100, false), ProposalState::Early);
        // Proposed one block back: tip + 1 = P + closest, committable.
        assert_eq!(proposal_state(w, tip, 99, false), ProposalState::Waiting);
        // Window's last block is the next one.
        assert_eq!(proposal_state(w, tip, 91, false), ProposalState::Waiting);
        // Window ended at the tip without a commit.
        assert_eq!(proposal_state(w, tip, 90, false), ProposalState::Expired);
        assert_eq!(proposal_state(w, tip, 90, true), ProposalState::Committed);
        assert_eq!(proposal_state(w, tip, 100, true), ProposalState::Committed);
    }

    fn consensus(max_block_proposals_limit: u64) -> ConsensusParams {
        ConsensusParams {
            tx_proposal_window: mainnet_window(),
            max_block_bytes: 1_000,
            max_block_cycles: 1_000,
            max_block_proposals_limit,
        }
    }

    fn entry(id: u8, stage: RpcPoolStage, size: u64, fee: u64, cycles: u64) -> PoolEntry {
        let mut hash = [0xFF; 32];
        hash[..10].copy_from_slice(&[id; 10]);
        PoolEntry {
            hash,
            rpc_stage: stage,
            size,
            fee,
            cycles,
            arrived_at_ms: u64::from(id),
        }
    }

    fn zone(ids: &[u8]) -> Vec<ZoneProposal> {
        ids.iter()
            .map(|id| ZoneProposal {
                short_id: vec![*id; 10],
                uncle: None,
            })
            .collect()
    }

    /// Zones of blocks 91..=100 (tip 100): id 1 at 95, id 2 at the tip, id 3
    /// at 91 and again at the tip.
    fn zones() -> HashMap<i64, Vec<ZoneProposal>> {
        (91..=100)
            .map(|n| {
                let ids: &[u8] = match n {
                    95 => &[1],
                    100 => &[2, 3],
                    91 => &[3],
                    _ => &[],
                };
                (n, zone(ids))
            })
            .collect()
    }

    fn classify(entries: Vec<PoolEntry>) -> Result<Vec<ClassifiedPoolTx>, String> {
        let zones = zones();
        classify_pool(
            mainnet_window(),
            100,
            |n| {
                zones
                    .get(&n)
                    .map(Vec::as_slice)
                    .ok_or_else(|| format!("no zone {n}"))
            },
            entries,
        )
    }

    #[test]
    fn classify_pool_stages_from_zones() {
        let classified = classify(vec![
            entry(1, RpcPoolStage::Proposed, 100, 1_000, 1),
            entry(2, RpcPoolStage::Pending, 100, 1_000, 1),
            entry(3, RpcPoolStage::Proposed, 100, 1_000, 1),
            entry(4, RpcPoolStage::Pending, 100, 1_000, 1),
        ])
        .unwrap();
        let stage = |i: usize| {
            (
                classified[i].proposed_in.map(|p| p.block),
                classified[i].committable,
            )
        };
        assert_eq!(stage(0), (Some(95), true));
        // Proposed at the tip: staged, but too early for the next block.
        assert_eq!(stage(1), (Some(100), false));
        // Proposed at 91 (committable) and again at the tip (gap): the
        // committable proposal stages it, as the node's own split does.
        assert_eq!(stage(2), (Some(91), true));
        assert_eq!(stage(3), (None, false));
        assert_eq!(classified[0].fee_rate, 10_000);
    }

    #[test]
    fn classify_pool_checks_the_node_labels() {
        let p1 = classify(vec![entry(4, RpcPoolStage::Proposed, 100, 1, 1)]).unwrap_err();
        assert!(p1.starts_with("P1:"), "{p1}");
        // A non-mining node lists every pool tx as pending, committable ones included: the
        // chain stages it.
        let pending = classify(vec![entry(1, RpcPoolStage::Pending, 100, 1, 1)]).unwrap();
        assert_eq!(pending[0].proposed_in.map(|p| p.block), Some(95));
        assert!(pending[0].committable);
        // A gap tx the node calls pending is consistent.
        assert!(classify(vec![entry(2, RpcPoolStage::Pending, 100, 1, 1)]).is_ok());
    }

    #[test]
    fn predict_next_skips_what_does_not_fit_and_caps_proposals() {
        // feeRate (per KB): 1 -> 10_000, 2 -> 20_000, 3 -> 5_000, 4 -> 30_000.
        let classified = classify(vec![
            entry(1, RpcPoolStage::Proposed, 600, 6_000, 10),
            entry(3, RpcPoolStage::Proposed, 300, 1_500, 10),
            entry(2, RpcPoolStage::Pending, 100, 2_000, 10),
            entry(4, RpcPoolStage::Pending, 100, 3_000, 10),
        ])
        .unwrap();
        let next = predict_next(&consensus(1), &classified, 5).unwrap();
        // Only committable txs (1, 3); both fit 1_000 bytes.
        let hashes: Vec<String> = next.txs.iter().map(|t| t.hash.clone()).collect();
        assert_eq!(
            hashes,
            vec![
                hex(&classified[0].entry.hash),
                hex(&classified[1].entry.hash)
            ]
        );
        assert_eq!((next.bytes, next.cycles), (900, 20));
        assert_eq!(next.proposals, 1, "min(pending, max_block_proposals_limit)");

        let tight = ConsensusParams {
            max_block_bytes: 650,
            ..consensus(1500)
        };
        let next = predict_next(&tight, &classified, 5).unwrap();
        assert_eq!(next.txs.len(), 1);
        assert_eq!(next.bytes, 600);
        assert_eq!(next.proposals, 5);
    }

    #[test]
    fn predict_next_serves_at_most_a_card_of_txs() {
        let classified: Vec<ClassifiedPoolTx> = (0..300u32)
            .map(|i| {
                let mut hash = [0u8; 32];
                hash[..4].copy_from_slice(&i.to_be_bytes());
                ClassifiedPoolTx {
                    entry: PoolEntry {
                        hash,
                        rpc_stage: RpcPoolStage::Proposed,
                        size: 1 + u64::from(i % 3),
                        fee: 1,
                        cycles: 1,
                        arrived_at_ms: u64::from(i),
                    },
                    proposed_in: Some(ProposedIn {
                        block: 99,
                        uncle_index: None,
                    }),
                    committable: true,
                    fee_rate: 1,
                }
            })
            .collect();
        let next = predict_next(&consensus(1500), &classified, 0).unwrap();
        assert_eq!(next.txs.len(), PIPELINE_BLOCK_TXS);
        assert_eq!(next.omitted_txs, 300 - PIPELINE_BLOCK_TXS);
        assert_eq!(next.bytes, 600, "bytes cover every predicted tx");
        assert!(next.txs.windows(2).all(|w| w[0].size >= w[1].size));
    }

    fn shown_block(number: i64, proposals: Vec<PipelineProposal>) -> PipelineBlock {
        PipelineBlock {
            number,
            hash: hex(&[number as u8; 32]),
            parent_hash: hex(&[0; 32]),
            timestamp: "2023-11-14T22:13:20+00:00".to_string(),
            tx_count: 1,
            bytes: 300,
            cycles: Some(0),
            cycles_pending: false,
            proposals,
            uncles: vec![],
            txs: vec![],
            omitted_txs: 0,
            cellbase: PipelineCellbase {
                hash: hex(&[0xCB; 32]),
                size: 300,
                pays: None,
            },
        }
    }

    fn open_proposal(id: u8, uncle_index: Option<usize>) -> PipelineProposal {
        PipelineProposal {
            short_id: hex(&[id; 10]),
            uncle_index,
            state: ProposalState::Waiting,
            tx_hash: None,
            committed_block: None,
        }
    }

    #[test]
    fn link_names_the_pool_tx_on_its_proposal() {
        let classified = classify(vec![entry(1, RpcPoolStage::Proposed, 100, 1, 1)]).unwrap();
        let mut blocks = vec![shown_block(95, vec![open_proposal(1, None)])];
        link_pool_to_proposals(&mut blocks, &classified).unwrap();
        assert_eq!(
            blocks[0].proposals[0].tx_hash,
            Some(hex(&classified[0].entry.hash))
        );
    }

    #[test]
    fn link_fails_when_the_staging_proposal_does_not_match() {
        let classified = classify(vec![entry(1, RpcPoolStage::Proposed, 100, 1, 1)]).unwrap();
        // The served zone says the id came from an uncle; the pool tx's stage says
        // the block's own proposal (P3).
        let mut blocks = vec![shown_block(95, vec![open_proposal(1, Some(0))])];
        let err = link_pool_to_proposals(&mut blocks, &classified).unwrap_err();
        assert!(err.starts_with("P3:"), "{err}");
        // A staging block missing from the shown chain is the same violation.
        let err = link_pool_to_proposals(&mut [], &classified).unwrap_err();
        assert!(err.starts_with("P3:"), "{err}");
    }

    #[test]
    fn link_rejects_two_pool_txs_with_one_short_id() {
        let mut twin = entry(4, RpcPoolStage::Pending, 100, 1, 1);
        twin.hash[31] = 0x00;
        let classified = classify(vec![entry(4, RpcPoolStage::Pending, 100, 1, 1), twin]).unwrap();
        let err = link_pool_to_proposals(&mut [], &classified).unwrap_err();
        assert!(err.contains("share proposal short id"), "{err}");
    }

    #[test]
    fn proposal_state_uses_the_consensus_closest_edge() {
        // A chain whose closest edge is 3: proposals at tip and tip - 1 are early.
        let w = ProposalWindow::new(3, 10).unwrap();
        assert_eq!(proposal_state(w, 100, 100, false), ProposalState::Early);
        assert_eq!(proposal_state(w, 100, 99, false), ProposalState::Early);
        assert_eq!(proposal_state(w, 100, 98, false), ProposalState::Waiting);
    }

    // -----------------------------------------------------------------------
    // Waiting for ckbadger's sync tip and the node's pool tip to agree
    // -----------------------------------------------------------------------

    /// A store whose sync tip is `(number, hash)`.
    fn store_at(number: i64, hash: [u8; 32]) -> (tempfile::TempDir, Arc<CkbadgerStore>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        move_sync_tip(&store, number, hash);
        (dir, store)
    }

    fn move_sync_tip(store: &CkbadgerStore, number: i64, hash: [u8; 32]) {
        store
            .update_sync_status(|s| {
                s.tip_block_number = number;
                s.tip_block_hash = hash.to_vec();
            })
            .unwrap();
    }

    fn node_at(number: u64, hash: [u8; 32]) -> TxPoolInfo {
        TxPoolInfo {
            tip_hash: hash,
            tip_number: number,
            last_txs_updated_at: 1,
        }
    }

    fn one_pending_tx() -> RawTxPool {
        RawTxPool {
            pending: vec![(
                [0x0C; 32],
                PoolEntryMeta {
                    fee: 1_000,
                    size: 500,
                    cycles: 10,
                    ancestors_count: 0,
                    time_added_to_pool_ms: 1,
                },
            )],
            proposed: vec![],
        }
    }

    fn calls(source: &FakePoolSource, method: &str) -> usize {
        source.calls().iter().filter(|call| *call == method).count()
    }

    #[tokio::test(start_paused = true)]
    async fn tip_wait_gives_up_at_its_deadline() {
        let (_dir, store) = store_at(13, [0x13; 32]);
        let source = FakePoolSource::new();
        source.set_info(node_at(12, [0x12; 32]));

        let started = Instant::now();
        let (pinned, read) = await_pool_at_sync_tip(&store, &InMemoryCache::new(), &source)
            .await
            .unwrap();
        let waited = started.elapsed();

        assert!(waited >= PIPELINE_TIP_WAIT, "gave up after {waited:?}");
        assert!(
            waited < PIPELINE_TIP_WAIT + PIPELINE_TIP_POLL,
            "overran its deadline: {waited:?}"
        );
        assert_eq!(
            pinned.tip,
            SyncTip {
                number: 13,
                hash: [0x13; 32]
            }
        );
        assert_eq!(
            read,
            PoolRead::Elsewhere {
                number: 12,
                hash: [0x12; 32]
            }
        );
        // One read at once, one per poll, the last one at the deadline.
        let reads = (PIPELINE_TIP_WAIT.as_millis() / PIPELINE_TIP_POLL.as_millis()) as usize + 1;
        assert_eq!(calls(&source, "tx_pool_info"), reads);
        assert_eq!(
            calls(&source, "get_raw_tx_pool"),
            0,
            "a pool at another tip is never read"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn tip_wait_serves_once_a_lagging_pool_reaches_the_sync_tip() {
        let (_dir, store) = store_at(13, [0x13; 32]);
        let source = Arc::new(FakePoolSource::new());
        // ckb attached block 13 and ckbadger indexed it, but the node has not
        // updated its pool for it yet.
        source.set_info(node_at(12, [0x12; 32]));
        source.set_raw_pool(one_pending_tx());
        let node = source.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PIPELINE_TIP_POLL * 3 + PIPELINE_TIP_POLL / 2).await;
            node.set_info(node_at(13, [0x13; 32]));
        });

        let started = Instant::now();
        let (pinned, read) = await_pool_at_sync_tip(&store, &InMemoryCache::new(), &*source)
            .await
            .unwrap();

        assert_eq!(
            started.elapsed(),
            PIPELINE_TIP_POLL * 4,
            "served at the first poll after the pool caught up"
        );
        assert_eq!(pinned.tip.number, 13);
        let PoolRead::AtTip(entries) = read else {
            panic!("expected the pool at the sync tip, got {read:?}");
        };
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn tip_wait_follows_the_store_through_a_reorg_at_equal_height() {
        // ckbadger indexed block 13 A while the node switched to 13 B: the
        // same height on two forks.
        let (_dir, store) = store_at(13, [0xA0; 32]);
        let source = FakePoolSource::new();
        source.set_info(node_at(13, [0xB0; 32]));
        source.set_raw_pool(one_pending_tx());
        let indexer = store.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PIPELINE_TIP_POLL + PIPELINE_TIP_POLL / 2).await;
            // The indexer rolls A back and indexes B.
            move_sync_tip(&indexer, 13, [0xB0; 32]);
        });

        let started = Instant::now();
        let (pinned, read) = await_pool_at_sync_tip(&store, &InMemoryCache::new(), &source)
            .await
            .unwrap();

        assert_eq!(started.elapsed(), PIPELINE_TIP_POLL * 2);
        assert_eq!(
            pinned.tip,
            SyncTip {
                number: 13,
                hash: [0xB0; 32]
            },
            "the snapshot is built at the tip both sides agree on"
        );
        assert!(matches!(read, PoolRead::AtTip(_)), "{read:?}");
    }

    #[tokio::test]
    async fn pool_read_elsewhere_is_never_cached() {
        let source = FakePoolSource::new();
        let cache = InMemoryCache::new();
        let tip = SyncTip {
            number: 13,
            hash: [0x13; 32],
        };
        source.set_info(node_at(12, [0x12; 32]));
        let lagging = read_pool(&source, &cache, &tip).await.unwrap();
        assert!(matches!(lagging, PoolRead::Elsewhere { .. }), "{lagging:?}");

        // The node's pool catches up a moment later: the next read sees it, not
        // a cached mismatch.
        source.set_info(node_at(13, [0x13; 32]));
        source.set_raw_pool(one_pending_tx());
        let caught_up = read_pool(&source, &cache, &tip).await.unwrap();
        assert!(matches!(caught_up, PoolRead::AtTip(_)), "{caught_up:?}");
    }

    #[tokio::test]
    async fn pool_read_cache_serves_only_the_tip_it_was_read_at() {
        let source = FakePoolSource::new();
        let cache = InMemoryCache::new();
        let a = SyncTip {
            number: 13,
            hash: [0xA0; 32],
        };
        source.set_info(node_at(13, [0xA0; 32]));
        source.set_raw_pool(one_pending_tx());
        let at_a = read_pool(&source, &cache, &a).await.unwrap();
        assert!(
            matches!(at_a, PoolRead::AtTip(ref e) if e.len() == 1),
            "{at_a:?}"
        );

        // Read again at the same tip: the cached read, the node is not asked.
        source.clear_calls();
        assert_eq!(read_pool(&source, &cache, &a).await.unwrap(), at_a);
        assert!(source.calls().is_empty(), "{:?}", source.calls());

        // Another tip never gets that read: the key is the whole tip, number
        // and hash.
        source.set_raw_pool(RawTxPool::default());
        for other in [
            SyncTip {
                number: 13,
                hash: [0xB0; 32],
            },
            SyncTip {
                number: 14,
                hash: [0xA0; 32],
            },
        ] {
            let number = u64::try_from(other.number).unwrap();
            source.set_info(node_at(number, other.hash));
            assert_eq!(
                read_pool(&source, &cache, &other).await.unwrap(),
                PoolRead::AtTip(vec![]),
                "{other:?}"
            );
        }
    }
}
