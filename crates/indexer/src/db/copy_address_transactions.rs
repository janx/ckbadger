use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;

/// 6 columns: lock_script_hash, tx_hash, block_number, tx_type, capacity_change, timestamp
const ADDR_TX_COLUMN_COUNT: i16 = 6;

pub struct CopyAddressTransactionsWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyAddressTransactionsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(ADDR_TX_COLUMN_COUNT),
            row_count: 0,
        }
    }

    pub fn add_record(
        &mut self,
        lock_script_hash: &[u8],
        tx_hash: &[u8],
        block_number: i64,
        tx_type: i16,
        capacity_change: i64,
        timestamp: DateTime<Utc>,
    ) {
        self.buffer.start_row();

        self.buffer.write_bytea(lock_script_hash);
        self.buffer.write_bytea(tx_hash);
        self.buffer.write_i64(block_number);
        self.buffer.write_i16(tx_type);
        self.buffer.write_i64(capacity_change);
        self.buffer.write_timestamptz(timestamp);

        self.row_count += 1;
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }

    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }
}

impl Default for CopyAddressTransactionsWriter {
    fn default() -> Self {
        Self::new()
    }
}

pub type AddressTransactionRow = (Vec<u8>, Vec<u8>, i64, i16, i64, DateTime<Utc>);

pub async fn copy_address_transactions(
    client: &Client,
    records: &[AddressTransactionRow],
) -> Result<u64> {
    if records.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyAddressTransactionsWriter::new();
    for (lock_hash, tx_hash, block_num, tx_type, cap_change, ts) in records {
        writer.add_record(lock_hash, tx_hash, *block_num, *tx_type, *cap_change, *ts);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY address_transactions (lock_script_hash, tx_hash, block_number, tx_type, capacity_change, timestamp) FROM STDIN WITH (FORMAT BINARY)")
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_copy_address_transactions_writer_creates_buffer() {
        let writer = CopyAddressTransactionsWriter::new();
        let data = writer.finish();
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_address_transactions_writer_is_empty() {
        let writer = CopyAddressTransactionsWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_address_transactions_writer_add_record() {
        let mut writer = CopyAddressTransactionsWriter::new();
        let lock_hash = [0xaa; 32];
        let tx_hash = [0xbb; 32];
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        writer.add_record(&lock_hash, &tx_hash, 12345, 1, 100_000_000, timestamp);

        assert!(!writer.is_empty());
        assert_eq!(writer.row_count(), 1);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_address_transactions_writer_multiple_records() {
        let mut writer = CopyAddressTransactionsWriter::new();
        let lock_hash = [0xaa; 32];
        let tx_hash1 = [0xbb; 32];
        let tx_hash2 = [0xcc; 32];
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        writer.add_record(&lock_hash, &tx_hash1, 12345, 1, 100_000_000, timestamp);
        writer.add_record(&lock_hash, &tx_hash2, 12346, 2, -50_000_000, timestamp);

        assert_eq!(writer.row_count(), 2);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_address_transactions_writer_negative_capacity() {
        let mut writer = CopyAddressTransactionsWriter::new();
        let lock_hash = [0xaa; 32];
        let tx_hash = [0xbb; 32];
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        writer.add_record(&lock_hash, &tx_hash, 12345, 2, -500_000_000_000, timestamp);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_address_transactions_writer_default() {
        let writer = CopyAddressTransactionsWriter::default();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_address_transactions_writer_tx_types() {
        let mut writer = CopyAddressTransactionsWriter::new();
        let lock_hash = [0xaa; 32];
        let tx_hash = [0xbb; 32];
        let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        writer.add_record(&lock_hash, &tx_hash, 1, 1, 100, timestamp);
        writer.add_record(&lock_hash, &tx_hash, 2, 2, -100, timestamp);
        writer.add_record(&lock_hash, &tx_hash, 3, 3, 0, timestamp);

        assert_eq!(writer.row_count(), 3);
    }
}
