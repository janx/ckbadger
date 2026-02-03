//! Pending proposal types for the Transaction Flow visualization.
//!
//! CKB uses a Two-Step Transaction Confirmation mechanism:
//! 1. Pending: Transaction in mempool waiting to be proposed
//! 2. Proposed: Transaction's short ID written to a block's proposal zone
//! 3. Committed: Transaction included in a block (2-10 blocks after proposal)
//!
//! This module provides types for caching proposal details in Redis,
//! enabling the frontend to display pending proposals with full tx metadata.

use serde::{Deserialize, Serialize};

/// Redis key prefix for pending proposals hash map
pub const PENDING_PROPOSALS_REDIS_KEY: &str = "proposals:pending";

/// TTL for proposal entries (5 minutes = ~30 blocks)
pub const PROPOSAL_CACHE_TTL_SECS: u64 = 300;

/// CKB Two-Step Confirmation window constants
pub const PROPOSAL_WINDOW_CLOSEST: u64 = 2; // Minimum blocks before commit allowed
pub const PROPOSAL_WINDOW_FARTHEST: u64 = 10; // Maximum blocks before proposal expires

/// Cached proposal entry stored in Redis.
///
/// Key: `proposals:pending` (Redis Hash)
/// Field: proposal_id_hex (20 char hex string, e.g., "abcd1234567890abcdef")
/// Value: JSON-serialized CachedProposal
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedProposal {
    /// 10-byte proposal ID (hex encoded without 0x prefix)
    pub proposal_id: String,

    /// Full 32-byte transaction hash if known (hex encoded with 0x prefix)
    pub full_tx_hash: Option<String>,

    /// Block number where this proposal was included
    pub proposed_at_block: i64,

    /// Index within the block's proposals array
    pub proposed_at_index: i16,

    /// Transaction fee in shannons (from mempool at proposal time)
    pub fee: Option<u64>,

    /// Serialized transaction size in bytes
    pub size: Option<u64>,

    /// Transaction cycles consumed
    pub cycles: Option<u64>,

    /// Fee rate (shannon per byte) = fee / size
    pub fee_rate: Option<f64>,

    /// Unix timestamp when this proposal was cached
    pub cached_at: i64,
}

impl CachedProposal {
    /// Create a new cached proposal with minimal info (no mempool data found)
    pub fn new_minimal(
        proposal_id: String,
        proposed_at_block: i64,
        proposed_at_index: i16,
    ) -> Self {
        Self {
            proposal_id,
            full_tx_hash: None,
            proposed_at_block,
            proposed_at_index,
            fee: None,
            size: None,
            cycles: None,
            fee_rate: None,
            cached_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a cached proposal with full transaction details from mempool
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_details(
        proposal_id: String,
        full_tx_hash: String,
        proposed_at_block: i64,
        proposed_at_index: i16,
        fee: u64,
        size: u64,
        cycles: u64,
    ) -> Self {
        let fee_rate = if size > 0 {
            Some(fee as f64 / size as f64)
        } else {
            None
        };

        Self {
            proposal_id,
            full_tx_hash: Some(full_tx_hash),
            proposed_at_block,
            proposed_at_index,
            fee: Some(fee),
            size: Some(size),
            cycles: Some(cycles),
            fee_rate,
            cached_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Calculate blocks remaining until the commit window closes.
    /// Returns None if the proposal has already expired.
    pub fn blocks_until_expiry(&self, current_tip: i64) -> Option<i64> {
        let expiry_block = self.proposed_at_block + PROPOSAL_WINDOW_FARTHEST as i64;
        let remaining = expiry_block - current_tip;
        if remaining > 0 {
            Some(remaining)
        } else {
            None
        }
    }

    /// Check if this proposal can be committed at the given block.
    /// Commit is allowed between [proposed_at + 2, proposed_at + 10]
    pub fn can_commit_at(&self, block_number: i64) -> bool {
        let earliest = self.proposed_at_block + PROPOSAL_WINDOW_CLOSEST as i64;
        let latest = self.proposed_at_block + PROPOSAL_WINDOW_FARTHEST as i64;
        block_number >= earliest && block_number <= latest
    }

    /// Check if this proposal has expired (past the commit window)
    pub fn is_expired(&self, current_tip: i64) -> bool {
        current_tip > self.proposed_at_block + PROPOSAL_WINDOW_FARTHEST as i64
    }
}

/// Response type for the pending proposals API endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingProposalsResponse {
    /// List of pending proposals (proposed but not yet committed/expired)
    pub proposals: Vec<PendingProposal>,

    /// Current chain tip block number
    pub tip_block_number: i64,

    /// Total count of pending proposals
    pub total_count: usize,
}

/// A pending proposal with computed fields for frontend display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingProposal {
    /// 10-byte proposal ID (hex with 0x prefix)
    pub proposal_id: String,

    /// Full transaction hash if known (hex with 0x prefix)
    pub full_tx_hash: Option<String>,

    /// Block number where proposed
    pub proposed_at_block: i64,

    /// Index within the block's proposals
    pub proposed_at_index: i16,

    /// Blocks remaining until commit window closes (0 = can commit now, negative = expired)
    pub blocks_until_expiry: i64,

    /// Transaction fee in shannons
    pub fee: Option<u64>,

    /// Transaction size in bytes
    pub size: Option<u64>,

    /// Transaction cycles
    pub cycles: Option<u64>,

    /// Fee rate (shannon per byte)
    pub fee_rate: Option<f64>,
}

impl PendingProposal {
    /// Create from a cached proposal with the current tip
    pub fn from_cached(cached: &CachedProposal, current_tip: i64) -> Self {
        let expiry_block = cached.proposed_at_block + PROPOSAL_WINDOW_FARTHEST as i64;
        let blocks_until_expiry = expiry_block - current_tip;

        Self {
            proposal_id: format!("0x{}", cached.proposal_id),
            full_tx_hash: cached.full_tx_hash.clone(),
            proposed_at_block: cached.proposed_at_block,
            proposed_at_index: cached.proposed_at_index,
            blocks_until_expiry,
            fee: cached.fee,
            size: cached.size,
            cycles: cached.cycles,
            fee_rate: cached.fee_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_proposal_new_minimal() {
        let proposal = CachedProposal::new_minimal("abcd1234567890abcdef".to_string(), 1000, 0);

        assert_eq!(proposal.proposal_id, "abcd1234567890abcdef");
        assert_eq!(proposal.proposed_at_block, 1000);
        assert_eq!(proposal.proposed_at_index, 0);
        assert!(proposal.full_tx_hash.is_none());
        assert!(proposal.fee.is_none());
    }

    #[test]
    fn test_cached_proposal_new_with_details() {
        let proposal = CachedProposal::new_with_details(
            "abcd1234567890abcdef".to_string(),
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            1000,
            0,
            1000000, // 0.01 CKB fee
            500,     // 500 bytes
            100000,  // 100k cycles
        );

        assert!(proposal.full_tx_hash.is_some());
        assert_eq!(proposal.fee, Some(1000000));
        assert_eq!(proposal.size, Some(500));
        assert!((proposal.fee_rate.unwrap() - 2000.0).abs() < 0.01); // 1000000 / 500 = 2000
    }

    #[test]
    fn test_blocks_until_expiry() {
        let proposal = CachedProposal::new_minimal("abcd1234567890abcdef".to_string(), 1000, 0);

        // At block 1005, 5 blocks remaining until expiry (1000 + 10 = 1010)
        assert_eq!(proposal.blocks_until_expiry(1005), Some(5));

        // At block 1010, 0 blocks remaining
        assert_eq!(proposal.blocks_until_expiry(1010), None);

        // At block 1011, expired
        assert_eq!(proposal.blocks_until_expiry(1011), None);
    }

    #[test]
    fn test_can_commit_at() {
        let proposal = CachedProposal::new_minimal("abcd1234567890abcdef".to_string(), 1000, 0);

        // Too early (before block 1002)
        assert!(!proposal.can_commit_at(1001));

        // Valid commit window [1002, 1010]
        assert!(proposal.can_commit_at(1002));
        assert!(proposal.can_commit_at(1005));
        assert!(proposal.can_commit_at(1010));

        // Expired (after block 1010)
        assert!(!proposal.can_commit_at(1011));
    }

    #[test]
    fn test_is_expired() {
        let proposal = CachedProposal::new_minimal("abcd1234567890abcdef".to_string(), 1000, 0);

        assert!(!proposal.is_expired(1000));
        assert!(!proposal.is_expired(1010)); // Last valid block
        assert!(proposal.is_expired(1011)); // Expired
    }

    #[test]
    fn test_pending_proposal_from_cached() {
        let cached = CachedProposal::new_with_details(
            "abcd1234567890abcdef".to_string(),
            "0xfull_hash".to_string(),
            1000,
            0,
            1000000,
            500,
            100000,
        );

        let pending = PendingProposal::from_cached(&cached, 1005);

        assert_eq!(pending.proposal_id, "0xabcd1234567890abcdef");
        assert_eq!(pending.full_tx_hash, Some("0xfull_hash".to_string()));
        assert_eq!(pending.blocks_until_expiry, 5); // 1010 - 1005 = 5
    }

    #[test]
    fn test_serialization_roundtrip() {
        let proposal = CachedProposal::new_with_details(
            "abcd1234567890abcdef".to_string(),
            "0xfull_hash".to_string(),
            1000,
            0,
            1000000,
            500,
            100000,
        );

        let json = serde_json::to_string(&proposal).unwrap();
        let parsed: CachedProposal = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.proposal_id, proposal.proposal_id);
        assert_eq!(parsed.fee, proposal.fee);
    }
}
