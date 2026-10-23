//! NC-Max proposal commitment-window resolution.
//!
//! The single shared path answering "which transaction committed this proposal
//! short id?" — used by both `/graph/proposals/{block_number}` and
//! `/blocks/{id}/proposals`. Keeping one resolver is what stops the two
//! endpoints from disagreeing about the same on-chain fact.
//!
//! The window itself is the chain's consensus `tx_proposal_window`
//! ([`crate::consensus::ProposalWindow`]), never a constant.

use ckb_store_reader::CkbChainReader;
use ckb_types::prelude::*;

use crate::consensus::ProposalWindow;

/// Resolve each proposal short id of `proposer_block` to the transaction that
/// commits it: `(tx_hash, committing block number)`, index-aligned with
/// `short_ids`.
///
/// Semantics (shared, exact): scan the commitment window
/// `proposer_block + closest ..= proposer_block + farthest` in chain order —
/// blocks ascending, transactions in in-block order — and the FIRST transaction whose hash starts with the short id wins. A missing
/// window block (a tip-adjacent window extending past the chain head) is
/// skipped: proposals unresolved within the blocks that exist stay `None`,
/// which is honest not-yet-committed data, not a fallback.
pub(crate) fn resolve_committed_txs(
    ckb_store: &CkbChainReader,
    window: ProposalWindow,
    proposer_block: i64,
    short_ids: &[Vec<u8>],
) -> Vec<Option<(Vec<u8>, i64)>> {
    let mut resolved: Vec<Option<(Vec<u8>, i64)>> = vec![None; short_ids.len()];
    if short_ids.is_empty() {
        return resolved;
    }

    for commit_block_num in window.commit_range(proposer_block) {
        if resolved.iter().all(Option::is_some) {
            break;
        }
        let Some(commit_block) = ckb_store.get_block_by_number(commit_block_num as u64) else {
            continue;
        };
        for tx in commit_block.transactions() {
            let tx_hash_bytes: [u8; 32] = tx.hash().unpack();
            for (i, short_id) in short_ids.iter().enumerate() {
                if resolved[i].is_none() && tx_hash_bytes[..short_id.len()] == short_id[..] {
                    resolved[i] = Some((tx_hash_bytes.to_vec(), commit_block_num));
                }
            }
        }
    }

    resolved
}
