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
//!   node's `get_block_economic_state(..).finalized_at`.
//!
//! Chain-guaranteed invariants are checked, never repaired: a violation is a
//! 500 naming the invariant (C1–C5) and the block/transaction it broke on.
//!
//! Read-only: store + node DB (`CkbChainReader`) reads and node RPC calls.

use axum::{extract::State, routing::get, Router};
use ckb_store_reader::CkbChainReader;
use ckb_types::prelude::*;
use ckbadger_store::{CachedBlockHeader, CkbadgerStore, TxIndexEntry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::cache::{CacheTtl, InMemoryCache};
use crate::consensus::{ConsensusParams, ProposalWindow};
use crate::response::{ok, ApiError, ApiResult, ApiRouteError};
use crate::routes::blocks::fetch_block_economic_state;
use crate::routes::proposal_window::{
    block_proposal_zone, earliest_proposal_in_window, resolve_committed_txs_with, ZoneProposal,
};
use crate::routes::transactions::{
    tx_cycles_status, tx_fee_rate, tx_serialized_size_in_block, TxCyclesStatus,
};
use crate::AppState;

/// Blocks the snapshot shows, newest first (frontend `layout.ts` `G.maxBlocks`).
pub(crate) const PIPELINE_BLOCKS: i64 = 24;
/// Committed transactions served per block besides the cellbase: the 1×1
/// capacity of a full-width card, `G.maxCols × G.fieldRows` = 14 × 17 in
/// frontend `layout.ts`.
pub(crate) const PIPELINE_BLOCK_TXS: usize = 238;

/// Genesis is the chain's first block; nothing lies below it.
const GENESIS: i64 = 0;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/pipeline", get(get_pipeline))
}

// ---------------------------------------------------------------------------
// Wire types (the contract the frontend renders; camelCase)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineSnapshot {
    pub consensus: PipelineConsensus,
    /// ckbadger's sync tip (store).
    pub tip: BlockRef,
    /// Newest first, at most [`PIPELINE_BLOCKS`].
    pub blocks: Vec<PipelineBlock>,
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

fn load_chain(
    store: &CkbadgerStore,
    ckb: &CkbChainReader,
    mem_cache: &InMemoryCache,
    window: ProposalWindow,
) -> Result<LoadedChain, ApiRouteError> {
    let sync = store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {e}")))?;
    if sync.tip_block_hash.is_empty() {
        return Err(ApiError::initializing(
            "no block indexed yet (indexer still starting?)",
        ));
    }
    let tip = sync.tip_block_number;
    let tip_hash = hash32(&sync.tip_block_hash, "sync tip hash")?;

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
// Handler
// ---------------------------------------------------------------------------

async fn get_pipeline(State(state): State<Arc<AppState>>) -> ApiResult<PipelineSnapshot> {
    let consensus = state.consensus().await?.clone();
    let window = consensus.tx_proposal_window;
    let ckb = state.ckb_store.clone().ok_or_else(|| {
        ApiError::internal(
            "/pipeline reads proposal zones and uncles from the node DB; \
             configure ckb_db_path so the API can open it",
        )
    })?;

    let chain = {
        let store = state.store.clone();
        let mem_cache = state.mem_cache.clone();
        tokio::task::spawn_blocking(move || load_chain(&store, &ckb, &mem_cache, window))
            .await
            .map_err(|e| ApiError::internal(format!("pipeline chain load panicked: {e}")))??
    };

    let mut pays = HashMap::new();
    for block in chain.shown() {
        pays.insert(
            block.number,
            resolve_pays(&state, window, &chain, block).await?,
        );
    }

    let blocks = build_chain(window, &chain, &pays).map_err(ApiError::internal)?;

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

    let tip = &chain.blocks[&chain.tip];
    ok(PipelineSnapshot {
        consensus: PipelineConsensus::from(&consensus),
        tip: BlockRef {
            number: chain.tip,
            hash: hex(&tip.hash),
        },
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn proposal_state_uses_the_consensus_closest_edge() {
        // A chain whose closest edge is 3: proposals at tip and tip - 1 are early.
        let w = ProposalWindow::new(3, 10).unwrap();
        assert_eq!(proposal_state(w, 100, 100, false), ProposalState::Early);
        assert_eq!(proposal_state(w, 100, 99, false), ProposalState::Early);
        assert_eq!(proposal_state(w, 100, 98, false), ProposalState::Waiting);
    }
}
