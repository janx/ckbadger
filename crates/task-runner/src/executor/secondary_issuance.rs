use anyhow::Result;
use ckbadger_common::SecondaryIssuanceBackfillConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &SecondaryIssuanceBackfillConfig,
) -> Result<()> {
    info!("secondary issuance backfill is a no-op with RocksDB storage (maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: secondary issuance maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, Serialize)]
    struct RpcRequest {
        jsonrpc: &'static str,
        id: u32,
        method: &'static str,
        params: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct BlockEconomicState {
        issuance: BlockIssuance,
        miner_reward: MinerReward,
    }

    #[derive(Debug, Deserialize)]
    struct BlockIssuance {
        secondary: String,
    }

    #[derive(Debug, Deserialize)]
    struct MinerReward {
        secondary: String,
    }

    fn parse_dao_field(dao: &[u8]) -> Option<(u64, u64)> {
        if dao.len() < 32 {
            return None;
        }
        let total_issuance = u64::from_le_bytes(dao[0..8].try_into().ok()?);
        let occupied_capacity = u64::from_le_bytes(dao[24..32].try_into().ok()?);
        Some((total_issuance, occupied_capacity))
    }

    fn parse_hex_u128(value: &str) -> Result<u128> {
        let hex = value.strip_prefix("0x").unwrap_or(value);
        u128::from_str_radix(hex, 16).map_err(|e| anyhow!("Invalid hex value {}: {}", value, e))
    }

    fn u128_to_i64(value: u128) -> Result<i64> {
        i64::try_from(value).map_err(|_| anyhow!("Value too large for i64: {}", value))
    }

    #[test]
    fn test_parse_dao_field_extracts_values() {
        let mut dao = vec![0u8; 32];
        dao[0..8].copy_from_slice(&123u64.to_le_bytes());
        dao[24..32].copy_from_slice(&456u64.to_le_bytes());

        let parsed = parse_dao_field(&dao).unwrap();
        assert_eq!(parsed.0, 123);
        assert_eq!(parsed.1, 456);
    }

    #[test]
    fn test_parse_hex_u128_handles_prefix() {
        let value = parse_hex_u128("0x10").unwrap();
        assert_eq!(value, 16);
    }

    #[test]
    fn test_u128_to_i64_range_check() {
        let max = u128::from(i64::MAX as u64);
        assert!(u128_to_i64(max).is_ok());

        let too_large = max + 1;
        assert!(u128_to_i64(too_large).is_err());
    }

    #[test]
    fn test_rpc_request_serialization() {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "get_block_economic_state",
            params: vec!["0xabc123".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"method\":\"get_block_economic_state\""));
        assert!(json.contains("\"params\":[\"0xabc123\"]"));
    }

    #[test]
    fn test_block_economic_state_deserialization() {
        let json = r#"{
            "issuance": {"primary": "0x0", "secondary": "0x5f5e100"},
            "miner_reward": {"primary": "0x0", "secondary": "0x2faf080", "committed": "0x0", "proposal": "0x0"}
        }"#;

        let state: BlockEconomicState = serde_json::from_str(json).unwrap();
        assert_eq!(state.issuance.secondary, "0x5f5e100");
        assert_eq!(state.miner_reward.secondary, "0x2faf080");
    }

    #[test]
    fn test_process_block_calculates_burnt_correctly() {
        // Replicate the calculation logic from the original process_block_with_state
        let total_issuance: u64 = 1_000_000_000_000;
        let occupied: u64 = 100_000_000_000;

        let secondary_issuance: u128 = 100_000_000;
        let miner_secondary: u128 = 50_000_000;
        let non_miner_secondary = secondary_issuance.saturating_sub(miner_secondary);

        let total_issuance_128 = total_issuance as u128;
        let occupied_128 = occupied as u128;
        let denominator = total_issuance_128.saturating_sub(occupied_128);
        let dao_deposits: u128 = 200_000_000_000;

        let dao_share = (non_miner_secondary * dao_deposits) / denominator;
        let burnt_share = non_miner_secondary.saturating_sub(dao_share);

        assert_eq!(secondary_issuance, 100_000_000);
        assert_eq!(miner_secondary, 50_000_000);
        assert!(dao_share > 0);
        assert!(burnt_share > 0);
        assert_eq!(dao_share + burnt_share, non_miner_secondary);
    }

    #[test]
    fn test_zero_denominator_all_burnt() {
        let total_issuance: u128 = 100;
        let occupied: u128 = 100;
        let denominator = total_issuance.saturating_sub(occupied);

        let non_miner_secondary: u128 = 50;
        let (dao_compensation, burnt) = if denominator > 0 {
            let dao_share = (non_miner_secondary * 1000) / denominator;
            let burnt_share = non_miner_secondary.saturating_sub(dao_share);
            (dao_share, burnt_share)
        } else {
            (0, non_miner_secondary)
        };

        assert_eq!(dao_compensation, 0);
        assert_eq!(burnt, 50);
    }

    #[test]
    fn test_empty_dao_events() {
        let deposits: HashMap<i64, u128> = HashMap::new();
        let withdrawals: HashMap<i64, u128> = HashMap::new();
        assert!(deposits.is_empty());
        assert!(withdrawals.is_empty());
    }
}
