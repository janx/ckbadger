use anyhow::Result;
use ckbadger_common::TokenRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &TokenRebuildConfig,
) -> Result<()> {
    info!("token rebuild is a no-op with RocksDB storage (token CFs maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: token CFs maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{parse_hex_to_bytes, TokenRebuildResult};

    const SUDT_CODE_HASH: &str =
        "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
    const XUDT_CODE_HASH_DATA1: &str =
        "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95";
    const XUDT_CODE_HASH_TYPE: &str =
        "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb";

    fn parse_udt_amount(data: &[u8]) -> Option<u128> {
        if data.len() < 16 {
            return None;
        }
        Some(u128::from_le_bytes(data[0..16].try_into().ok()?))
    }

    fn udt_standard(
        code_hash: &[u8],
        hash_type: i16,
        sudt_hash: &[u8],
        xudt_data1_hash: &[u8],
        xudt_type_hash: &[u8],
    ) -> Option<&'static str> {
        if code_hash == sudt_hash && hash_type == 1 {
            return Some("sudt");
        }

        if (code_hash == xudt_data1_hash && hash_type == 2)
            || (code_hash == xudt_type_hash && hash_type == 1)
        {
            return Some("xudt");
        }

        None
    }

    #[test]
    fn test_parse_udt_amount_valid() {
        let amount = 42u128;
        let data = amount.to_le_bytes();
        assert_eq!(parse_udt_amount(&data), Some(amount));
    }

    #[test]
    fn test_parse_udt_amount_too_short() {
        let data = [0u8; 8];
        assert!(parse_udt_amount(&data).is_none());
    }

    #[test]
    fn test_udt_standard_detection() {
        let sudt_hash = parse_hex_to_bytes(SUDT_CODE_HASH);
        let xudt_data1_hash = parse_hex_to_bytes(XUDT_CODE_HASH_DATA1);
        let xudt_type_hash = parse_hex_to_bytes(XUDT_CODE_HASH_TYPE);

        assert_eq!(
            udt_standard(&sudt_hash, 1, &sudt_hash, &xudt_data1_hash, &xudt_type_hash),
            Some("sudt")
        );
        assert_eq!(
            udt_standard(
                &xudt_data1_hash,
                2,
                &sudt_hash,
                &xudt_data1_hash,
                &xudt_type_hash
            ),
            Some("xudt")
        );
        assert_eq!(
            udt_standard(
                &xudt_type_hash,
                1,
                &sudt_hash,
                &xudt_data1_hash,
                &xudt_type_hash
            ),
            Some("xudt")
        );
        assert!(
            udt_standard(&sudt_hash, 2, &sudt_hash, &xudt_data1_hash, &xudt_type_hash).is_none()
        );
    }

    #[test]
    fn test_result_serialization() {
        let result = TokenRebuildResult {
            tokens_created: 3,
            balances_updated: 5,
            udt_cells_created: 8,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["tokensCreated"], 3);
        assert_eq!(json["balancesUpdated"], 5);
        assert_eq!(json["udtCellsCreated"], 8);
    }
}
