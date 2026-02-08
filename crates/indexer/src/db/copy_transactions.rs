use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;

/// Transactions table has 17 columns (excluding short_hash which is GENERATED):
/// hash, block_number, block_hash, tx_index, version, inputs_count, outputs_count,
/// witnesses_count, cell_deps_count, header_deps_count,
/// total_input_capacity, total_output_capacity, fee, tx_size, cycles,
/// is_cellbase, timestamp
const TX_COLUMN_COUNT: i16 = 17;

pub type TransactionRow<'a> = (
    &'a [u8],      // hash
    i64,           // block_number
    &'a [u8],      // block_hash
    i32,           // tx_index
    i32,           // version
    i16,           // inputs_count
    i16,           // outputs_count
    i16,           // witnesses_count
    i16,           // cell_deps_count
    i16,           // header_deps_count
    i64,           // total_input_capacity
    i64,           // total_output_capacity
    i64,           // fee
    Option<i32>,   // tx_size
    Option<i64>,   // cycles
    bool,          // is_cellbase
    DateTime<Utc>, // timestamp
);

pub struct CopyTransactionsWriter {
    buffer: BinaryCopyBuffer,
}

impl Default for CopyTransactionsWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyTransactionsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(TX_COLUMN_COUNT),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_transaction(
        &mut self,
        hash: &[u8],
        block_number: i64,
        block_hash: &[u8],
        tx_index: i32,
        version: i32,
        inputs_count: i16,
        outputs_count: i16,
        witnesses_count: i16,
        cell_deps_count: i16,
        header_deps_count: i16,
        total_input_capacity: i64,
        total_output_capacity: i64,
        fee: i64,
        tx_size: Option<i32>,
        cycles: Option<i64>,
        is_cellbase: bool,
        timestamp: DateTime<Utc>,
    ) {
        self.buffer.start_row();

        self.buffer.write_bytea(hash);
        self.buffer.write_i64(block_number);
        self.buffer.write_bytea(block_hash);
        self.buffer.write_i32(tx_index);
        self.buffer.write_i32(version);
        self.buffer.write_i16(inputs_count);
        self.buffer.write_i16(outputs_count);
        self.buffer.write_i16(witnesses_count);
        self.buffer.write_i16(cell_deps_count);
        self.buffer.write_i16(header_deps_count);
        self.buffer.write_i64(total_input_capacity);
        self.buffer.write_i64(total_output_capacity);
        self.buffer.write_i64(fee);
        self.buffer.write_i32_opt(tx_size);
        self.buffer.write_i64_opt(cycles);
        self.buffer.write_bool(is_cellbase);
        self.buffer.write_timestamptz(timestamp);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

/// Execute COPY for transactions
pub async fn copy_transactions(client: &Client, txs: &[TransactionRow<'_>]) -> Result<u64> {
    if txs.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyTransactionsWriter::new();
    for tx in txs {
        writer.add_transaction(
            tx.0, tx.1, tx.2, tx.3, tx.4, tx.5, tx.6, tx.7, tx.8, tx.9, tx.10, tx.11, tx.12, tx.13,
            tx.14, tx.15, tx.16,
        );
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY transactions (hash, block_number, block_hash, tx_index, version, inputs_count, outputs_count, witnesses_count, cell_deps_count, header_deps_count, total_input_capacity, total_output_capacity, fee, tx_size, cycles, is_cellbase, timestamp) FROM STDIN WITH (FORMAT BINARY)")
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

    #[test]
    fn test_copy_transactions_writer_creates_buffer() {
        let writer = CopyTransactionsWriter::new();
        let data = writer.finish();
        assert!(data.len() >= 21);
    }
}
