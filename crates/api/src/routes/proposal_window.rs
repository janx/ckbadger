//! NC-Max proposals: a block's proposal zone and its commitment window.
//!
//! The single shared path for two on-chain facts every endpoint that shows
//! proposals relies on — `/graph/proposals/{block_number}`,
//! `/blocks/{id}/proposals`, `/transactions/{hash}/lifecycle`:
//! - which short ids a block proposes ([`block_proposal_zone`]: its own
//!   proposals plus those of the uncles it embeds), and
//! - which transaction committed a proposal ([`resolve_committed_txs`]).
//!
//! Keeping one implementation of each is what stops the endpoints from
//! disagreeing about the same chain data. The window itself is the chain's
//! consensus `tx_proposal_window` ([`crate::consensus::ProposalWindow`]),
//! never a constant.

use ckb_store_reader::CkbChainReader;
use ckb_types::prelude::*;
use std::collections::HashSet;

use crate::consensus::ProposalWindow;

/// One entry of a block's proposal zone: the short id, plus the uncle that
/// carried it when it did not come from the block's own `proposals()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZoneProposal {
    /// The 10-byte proposal short id (a transaction hash prefix).
    pub short_id: Vec<u8>,
    /// `None` for a proposal in the block's own `proposals()`.
    pub uncle: Option<ZoneUncle>,
}

/// The embedded uncle a zone entry came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZoneUncle {
    /// Position of the uncle in the containing block's `uncles()`.
    pub index: usize,
    /// The uncle's own header number (not the containing block's).
    pub number: i64,
    pub hash: [u8; 32],
}

/// The whole proposal zone CKB consensus attributes to `block`: its own
/// `proposals()` plus the proposal zones of the uncles it embeds — the set
/// `BlockView::union_proposal_ids` yields, in a stable order.
///
/// A proposal borne by an uncle belongs to the main-chain block that embeds
/// the uncle — that is the block whose commitment window the proposal opens.
///
/// Order and de-duplication: the block's own proposals first, in block order,
/// then each uncle's in embedding order. The block's own zone wins over its
/// uncles' when both carry an id, and an id listed by two embedded uncles is
/// one proposal, attributed to the first uncle in embedding order. The uncle
/// data already travels inside the block, so this costs no extra I/O.
pub(crate) fn block_proposal_zone(block: &ckb_types::core::BlockView) -> Vec<ZoneProposal> {
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut zone = Vec::new();

    for proposal_id in block.data().proposals().into_iter() {
        let short_id = proposal_id.raw_data().to_vec();
        if seen.insert(short_id.clone()) {
            zone.push(ZoneProposal {
                short_id,
                uncle: None,
            });
        }
    }

    for (index, uncle) in block.uncles().into_iter().enumerate() {
        let carrier = ZoneUncle {
            index,
            number: uncle.number() as i64,
            hash: uncle.hash().unpack(),
        };
        for proposal_id in uncle.data().proposals().into_iter() {
            let short_id = proposal_id.raw_data().to_vec();
            if seen.insert(short_id.clone()) {
                zone.push(ZoneProposal {
                    short_id,
                    uncle: Some(carrier.clone()),
                });
            }
        }
    }

    zone
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_types::core::{BlockBuilder, EpochNumberWithFraction, UncleBlockView};
    use ckb_types::packed::ProposalShortId;

    fn id(byte: u8) -> ProposalShortId {
        ProposalShortId::new([byte; 10])
    }

    fn uncle(number: u64, ids: &[u8], nonce: u128) -> UncleBlockView {
        let epoch = EpochNumberWithFraction::new(1, 0, 1800);
        let mut builder = BlockBuilder::default()
            .number(number.pack())
            .epoch(epoch.pack())
            .nonce(nonce.pack());
        for byte in ids {
            builder = builder.proposal(id(*byte));
        }
        builder.build().as_uncle()
    }

    #[test]
    fn zone_is_own_proposals_then_uncles_deduplicated() {
        let epoch = EpochNumberWithFraction::new(1, 0, 1800);
        let first_uncle = uncle(9, &[2, 3], 1);
        let second_uncle = uncle(9, &[3, 4], 2);
        let first_hash: [u8; 32] = first_uncle.hash().unpack();
        let second_hash: [u8; 32] = second_uncle.hash().unpack();
        let block = BlockBuilder::default()
            .number(10u64.pack())
            .epoch(epoch.pack())
            .proposal(id(1))
            .proposal(id(2))
            .uncle(first_uncle)
            .uncle(second_uncle)
            .build();

        let zone = block_proposal_zone(&block);
        let ids: Vec<u8> = zone.iter().map(|p| p.short_id[0]).collect();
        // 2 is the block's own (not the first uncle's); 3 belongs to the first
        // uncle that lists it; 4 to the second.
        assert_eq!(ids, vec![1, 2, 3, 4]);
        assert_eq!(zone[0].uncle, None);
        assert_eq!(zone[1].uncle, None);
        assert_eq!(
            zone[2].uncle,
            Some(ZoneUncle {
                index: 0,
                number: 9,
                hash: first_hash
            })
        );
        assert_eq!(
            zone[3].uncle,
            Some(ZoneUncle {
                index: 1,
                number: 9,
                hash: second_hash
            })
        );
    }
}
