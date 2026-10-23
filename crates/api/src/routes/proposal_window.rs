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
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::convert::Infallible;

use crate::consensus::ProposalWindow;

/// One entry of a block's proposal zone: the short id, plus the uncle that
/// carried it when it did not come from the block's own `proposals()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ZoneProposal {
    /// The 10-byte proposal short id (a transaction hash prefix).
    pub short_id: Vec<u8>,
    /// `None` for a proposal in the block's own `proposals()`.
    pub uncle: Option<ZoneUncle>,
}

/// The embedded uncle a zone entry came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// The transaction that committed a proposal, `(tx_hash, committing block
/// number)`; `None` while no block in the window (yet) committed it.
pub(crate) type Commitment = Option<(Vec<u8>, i64)>;

/// Resolve each proposal short id of `proposer_block` to the transaction that
/// commits it, index-aligned with `short_ids`, reading the window blocks from
/// the node DB by number.
///
/// Semantics are [`resolve_committed_txs_with`]'s. A missing window block (a
/// tip-adjacent window extending past the chain head) is skipped: proposals
/// unresolved within the blocks that exist stay `None`, which is honest
/// not-yet-committed data, not a fallback.
pub(crate) fn resolve_committed_txs(
    ckb_store: &CkbChainReader,
    window: ProposalWindow,
    proposer_block: i64,
    short_ids: &[Vec<u8>],
) -> Vec<Commitment> {
    let block_tx_hashes = |number: i64| -> Result<Option<Vec<[u8; 32]>>, Infallible> {
        Ok(ckb_store.get_block_by_number(number as u64).map(|block| {
            block
                .transactions()
                .iter()
                .map(|tx| tx.hash().unpack())
                .collect()
        }))
    };
    match resolve_committed_txs_with(window, proposer_block, short_ids, block_tx_hashes) {
        Ok(resolved) => resolved,
        Err(never) => match never {},
    }
}

/// The one commitment rule, over any source of block transaction hashes.
///
/// Scan the commitment window `proposer_block + closest ..= proposer_block +
/// farthest` in chain order — blocks ascending, transactions in in-block
/// order — and the FIRST transaction whose hash starts with the short id wins.
/// `block_tx_hashes(n)` yields block `n`'s transaction hashes, `Ok(None)` for
/// a block that does not exist (yet), or an error that aborts the scan.
pub(crate) fn resolve_committed_txs_with<H, E>(
    window: ProposalWindow,
    proposer_block: i64,
    short_ids: &[Vec<u8>],
    mut block_tx_hashes: impl FnMut(i64) -> Result<Option<H>, E>,
) -> Result<Vec<Commitment>, E>
where
    H: AsRef<[[u8; 32]]>,
{
    let mut resolved: Vec<Commitment> = vec![None; short_ids.len()];
    if short_ids.is_empty() {
        return Ok(resolved);
    }

    for commit_block_num in window.commit_range(proposer_block) {
        if resolved.iter().all(Option::is_some) {
            break;
        }
        let Some(tx_hashes) = block_tx_hashes(commit_block_num)? else {
            continue;
        };
        for tx_hash in tx_hashes.as_ref() {
            for (i, short_id) in short_ids.iter().enumerate() {
                if resolved[i].is_none() && tx_hash[..short_id.len()] == short_id[..] {
                    resolved[i] = Some((tx_hash.to_vec(), commit_block_num));
                }
            }
        }
    }

    Ok(resolved)
}

/// Where a committed transaction's proposal was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProposalHit {
    /// Main-chain block whose zone (own or uncle-borne) carried the short id.
    pub block_number: i64,
    pub uncle: Option<ZoneUncle>,
}

/// The proposal a transaction committed in `commit_block` was committed
/// under: the EARLIEST one inside `[commit_block - farthest, commit_block -
/// closest]` — the proposal CKB pays the proposal reward for. Window numbers
/// below genesis are not blocks and are not asked for.
///
/// `zone_of(n)` yields block `n`'s proposal zone ([`block_proposal_zone`]),
/// `Ok(None)` for a block the caller does not have, or an error that aborts.
pub(crate) fn earliest_proposal_in_window<Z, E>(
    window: ProposalWindow,
    commit_block: i64,
    short_id: &[u8],
    mut zone_of: impl FnMut(i64) -> Result<Option<Z>, E>,
) -> Result<Option<ProposalHit>, E>
where
    Z: AsRef<[ZoneProposal]>,
{
    for block_number in window.proposal_range(commit_block).filter(|n| *n >= 0) {
        let Some(zone) = zone_of(block_number)? else {
            continue;
        };
        if let Some(entry) = zone.as_ref().iter().find(|p| p.short_id == short_id) {
            return Ok(Some(ProposalHit {
                block_number,
                uncle: entry.uncle.clone(),
            }));
        }
    }
    Ok(None)
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

    fn window() -> ProposalWindow {
        ProposalWindow::new(2, 10).unwrap()
    }

    fn own(byte: u8) -> ZoneProposal {
        ZoneProposal {
            short_id: vec![byte; 10],
            uncle: None,
        }
    }

    #[test]
    fn earliest_proposal_is_the_first_in_the_window() {
        let zones: std::collections::HashMap<i64, Vec<ZoneProposal>> = [
            (1, vec![own(7)]),
            (3, vec![own(7)]),
            (5, vec![own(7), own(8)]),
        ]
        .into();
        let zone_of = |n: i64| Ok::<_, Infallible>(zones.get(&n).map(Vec::as_slice));

        // Commit at 12: window [2, 10]; block 1 is outside it.
        let hit = earliest_proposal_in_window(window(), 12, &[7; 10], zone_of).unwrap();
        assert_eq!(hit.map(|h| h.block_number), Some(3));
        // Commit at 4: window [-6, 2] — only blocks 0..=2 exist to ask.
        let hit = earliest_proposal_in_window(window(), 4, &[7; 10], zone_of).unwrap();
        assert_eq!(hit.map(|h| h.block_number), Some(1));
        // Commit at 6: window [-4, 4] reaches block 3 but not 5.
        let hit = earliest_proposal_in_window(window(), 6, &[8; 10], zone_of).unwrap();
        assert_eq!(hit, None);
    }

    #[test]
    fn earliest_proposal_stops_on_a_source_error() {
        let err = earliest_proposal_in_window(window(), 12, &[7; 10], |n| {
            Err::<Option<Vec<ZoneProposal>>, String>(format!("block {n} unavailable"))
        })
        .unwrap_err();
        assert_eq!(err, "block 2 unavailable");
    }

    #[test]
    fn commitment_is_the_first_matching_tx_in_chain_order() {
        let tx = |prefix: u8, tail: u8| {
            let mut hash = [tail; 32];
            hash[..10].copy_from_slice(&[prefix; 10]);
            hash
        };
        let blocks: std::collections::HashMap<i64, Vec<[u8; 32]>> = [
            (102, vec![tx(9, 1)]),
            // 103 does not exist (yet): skipped.
            (104, vec![tx(7, 2), tx(7, 3)]),
            (106, vec![tx(7, 4), tx(8, 5)]),
        ]
        .into();
        let resolved = resolve_committed_txs_with(
            window(),
            100,
            &[vec![7; 10], vec![8; 10], vec![6; 10]],
            |n| Ok::<_, Infallible>(blocks.get(&n).map(Vec::as_slice)),
        )
        .unwrap();
        assert_eq!(resolved[0], Some((tx(7, 2).to_vec(), 104)));
        assert_eq!(resolved[1], Some((tx(8, 5).to_vec(), 106)));
        assert_eq!(resolved[2], None);
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
