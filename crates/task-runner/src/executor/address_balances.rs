use anyhow::Result;
use ckbadger_common::AddressBalancesRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &AddressBalancesRebuildConfig,
) -> Result<()> {
    info!("address_balances rebuild is a no-op with RocksDB storage (address balances CF maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: address balances CF maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{AddressBalancesRebuildConfig, AddressBalancesRebuildResult};

    #[test]
    fn test_default_config() {
        let config = AddressBalancesRebuildConfig::default();
        assert!(config._reserved.is_none());
    }

    #[test]
    fn test_result_struct() {
        let result = AddressBalancesRebuildResult {
            addresses_updated: 12345,
        };
        assert_eq!(result.addresses_updated, 12345);
    }

    #[test]
    fn test_result_serialization() {
        let result = AddressBalancesRebuildResult {
            addresses_updated: 999,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["addressesUpdated"], 999);
    }
}
