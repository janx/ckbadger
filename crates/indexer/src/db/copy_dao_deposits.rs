use std::collections::HashMap;

use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::db::rocksdb_live_cell_store::DaoDepositCacheEntry;

type DaoDepositCacheList = Vec<(Vec<u8>, i16, DaoDepositCacheEntry)>;

/// dao_deposits table columns (excluding auto-generated `id`):
/// tx_hash, output_index, lock_script_hash, capacity,
/// deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar,
/// status,
/// withdraw_request_block, withdraw_request_tx, withdraw_request_timestamp, withdraw_request_ar,
/// withdraw_block, withdraw_tx, withdraw_timestamp,
/// compensation
const DAO_DEPOSITS_COLUMN_COUNT: i16 = 17;

const COPY_SQL: &str = "COPY dao_deposits (\
    tx_hash, output_index, lock_script_hash, capacity, \
    deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, \
    status, \
    withdraw_request_block, withdraw_request_tx, withdraw_request_timestamp, withdraw_request_ar, \
    withdraw_block, withdraw_tx, withdraw_timestamp, \
    compensation\
) FROM STDIN WITH (FORMAT BINARY)";

fn build_copy_buffer(
    deposits: &DaoDepositCacheList,
    timestamps: &HashMap<i64, DateTime<Utc>>,
) -> Bytes {
    let mut buf = BinaryCopyBuffer::new(DAO_DEPOSITS_COLUMN_COUNT);

    for (tx_hash, output_index, entry) in deposits {
        buf.start_row();

        // tx_hash BYTEA NOT NULL
        buf.write_bytea(tx_hash);
        // output_index SMALLINT NOT NULL
        buf.write_i16(*output_index);
        // lock_script_hash BYTEA NOT NULL
        buf.write_bytea(&entry.lock_script_hash);
        // capacity NUMERIC(20,0) NOT NULL
        buf.write_numeric(&entry.capacity.to_string());
        // deposit_block_number BIGINT NOT NULL
        buf.write_i64(entry.deposit_block_number);
        // deposit_tx_hash BYTEA NOT NULL (same as tx_hash for deposits)
        buf.write_bytea(tx_hash);
        // deposit_timestamp TIMESTAMPTZ NOT NULL
        if let Some(ts) = timestamps.get(&entry.deposit_block_number) {
            buf.write_timestamptz(*ts);
        } else {
            buf.write_null();
        }
        // deposit_ar NUMERIC(20,0) NOT NULL
        buf.write_numeric(&entry.deposit_ar.to_string());
        // status SMALLINT NOT NULL
        buf.write_i16(entry.status);

        // withdraw_request_block BIGINT (nullable)
        buf.write_i64_opt(entry.withdraw_request_block);
        // withdraw_request_tx BYTEA (nullable)
        buf.write_bytea_opt(entry.withdraw_request_tx.as_deref());
        // withdraw_request_timestamp TIMESTAMPTZ (nullable)
        let wr_ts = entry
            .withdraw_request_block
            .and_then(|b| timestamps.get(&b).copied());
        buf.write_timestamptz_opt(wr_ts);
        // withdraw_request_ar NUMERIC(20,0) (nullable)
        let wr_ar = entry.withdraw_request_ar.map(|v| v.to_string());
        buf.write_numeric_opt(wr_ar.as_deref());

        // withdraw_block BIGINT (nullable)
        buf.write_i64_opt(entry.withdraw_block);
        // withdraw_tx BYTEA (nullable)
        buf.write_bytea_opt(entry.withdraw_tx.as_deref());
        // withdraw_timestamp TIMESTAMPTZ (nullable)
        let w_ts = entry
            .withdraw_block
            .and_then(|b| timestamps.get(&b).copied());
        buf.write_timestamptz_opt(w_ts);
        // compensation NUMERIC(20,0) (nullable)
        let comp = entry.compensation.map(|v| v.to_string());
        buf.write_numeric_opt(comp.as_deref());
    }

    buf.finish().freeze()
}

pub async fn copy_dao_deposits_from_rocksdb(
    client: &Client,
    deposits: &DaoDepositCacheList,
    timestamps: &HashMap<i64, DateTime<Utc>>,
) -> Result<u64> {
    if deposits.is_empty() {
        return Ok(0);
    }

    let data = build_copy_buffer(deposits, timestamps);

    let sink = client.copy_in(COPY_SQL).await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

pub fn collect_block_numbers(deposits: &DaoDepositCacheList) -> Vec<i64> {
    let mut blocks = std::collections::HashSet::new();
    for (_, _, entry) in deposits {
        blocks.insert(entry.deposit_block_number);
        if let Some(b) = entry.withdraw_request_block {
            blocks.insert(b);
        }
        if let Some(b) = entry.withdraw_block {
            blocks.insert(b);
        }
    }
    blocks.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_copy_buffer_empty() {
        let deposits = vec![];
        let timestamps = HashMap::new();
        let data = build_copy_buffer(&deposits, &timestamps);
        // Header (19 bytes) + trailer (2 bytes) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_build_copy_buffer_with_active_deposit() {
        let entry = DaoDepositCacheEntry {
            capacity: 10000_00000000,
            deposit_block_number: 100,
            lock_script_hash: vec![0x01; 32],
            deposit_ar: 10000000000000000,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };
        let deposits = vec![(vec![0xaa; 32], 0i16, entry)];
        let mut timestamps = HashMap::new();
        timestamps.insert(100i64, Utc::now());

        let data = build_copy_buffer(&deposits, &timestamps);
        assert!(data.len() > 21);
    }

    #[test]
    fn test_build_copy_buffer_with_withdrawn_deposit() {
        let entry = DaoDepositCacheEntry {
            capacity: 20000_00000000,
            deposit_block_number: 100,
            lock_script_hash: vec![0x02; 32],
            deposit_ar: 10000000000000000,
            status: 2,
            withdraw_request_tx: Some(vec![0xbb; 32]),
            withdraw_request_block: Some(200),
            withdraw_request_ar: Some(10050000000000000),
            withdraw_block: Some(380),
            withdraw_tx: Some(vec![0xcc; 32]),
            compensation: Some(100_00000000),
        };
        let deposits = vec![(vec![0xdd; 32], 1i16, entry)];
        let mut timestamps = HashMap::new();
        timestamps.insert(100i64, Utc::now());
        timestamps.insert(200i64, Utc::now());
        timestamps.insert(380i64, Utc::now());

        let data = build_copy_buffer(&deposits, &timestamps);
        assert!(data.len() > 21);
    }

    #[test]
    fn test_collect_block_numbers_dedup() {
        let entry1 = DaoDepositCacheEntry {
            capacity: 100,
            deposit_block_number: 10,
            lock_script_hash: vec![],
            deposit_ar: 1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };
        let entry2 = DaoDepositCacheEntry {
            capacity: 200,
            deposit_block_number: 10, // same block
            lock_script_hash: vec![],
            deposit_ar: 1,
            status: 1,
            withdraw_request_tx: Some(vec![0x01; 32]),
            withdraw_request_block: Some(20),
            withdraw_request_ar: Some(2),
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        };
        let deposits = vec![
            (vec![0xaa; 32], 0i16, entry1),
            (vec![0xbb; 32], 0i16, entry2),
        ];

        let blocks = collect_block_numbers(&deposits);
        // Should contain {10, 20} - deduplicated
        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains(&10));
        assert!(blocks.contains(&20));
    }
}
