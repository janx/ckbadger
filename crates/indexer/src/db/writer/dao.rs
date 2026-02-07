#![allow(dead_code)]

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use super::rows::DaoDepositRow;
use super::BatchWriter;

pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)];
    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)];
    fn block_number(&self) -> i64;
    fn consuming_tx_hash(&self) -> &[u8];
    fn timestamp(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Default)]
pub struct SecondaryIssuanceBreakdown {
    pub secondary_issuance: i64,
    pub miner_secondary: i64,
    pub dao_compensation: i64,
    pub burnt: i64,
}

impl BatchWriter {
    pub async fn write_dao_deposits(&self, dao_deposits: &[DaoDepositRow]) -> Result<()> {
        if dao_deposits.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<DaoDepositRow>("dao_deposits")
            .await
            .context("Failed to create dao_deposits insert")?;

        for deposit in dao_deposits {
            insert
                .write(deposit)
                .await
                .context("Failed to write dao_deposit row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize dao_deposits insert")?;

        Ok(())
    }
}
