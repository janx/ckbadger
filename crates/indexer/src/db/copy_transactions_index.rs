use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;

/// Row type for transactions_index COPY: (hash, block_number, tx_index, is_cellbase, timestamp,
/// inputs_count, outputs_count, fee, cycles)
pub type TransactionIndexRow<'a> = (
    &'a [u8],
    i64,
    i32,
    bool,
    DateTime<Utc>,
    i16,
    i16,
    i64,
    Option<i64>,
);

/// transactions_index has 9 columns:
/// hash, block_number, tx_index, is_cellbase, timestamp,
/// inputs_count, outputs_count, fee, cycles
const TX_INDEX_COLUMN_COUNT: i16 = 9;

pub struct CopyTransactionsIndexWriter {
    buffer: BinaryCopyBuffer,
}

impl CopyTransactionsIndexWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(TX_INDEX_COLUMN_COUNT),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_transaction(
        &mut self,
        hash: &[u8],
        block_number: i64,
        tx_index: i32,
        is_cellbase: bool,
        timestamp: DateTime<Utc>,
        inputs_count: i16,
        outputs_count: i16,
        fee: i64,
        cycles: Option<i64>,
    ) {
        self.buffer.start_row();

        self.buffer.write_bytea(hash);
        self.buffer.write_i64(block_number);
        self.buffer.write_i32(tx_index);
        self.buffer.write_bool(is_cellbase);
        self.buffer.write_timestamptz(timestamp);
        self.buffer.write_i16(inputs_count);
        self.buffer.write_i16(outputs_count);
        self.buffer.write_i64(fee);
        self.buffer.write_i64_opt(cycles);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

impl Default for CopyTransactionsIndexWriter {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn copy_transactions_index(
    client: &Client,
    txs: &[TransactionIndexRow<'_>],
) -> Result<u64> {
    if txs.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyTransactionsIndexWriter::new();
    for tx in txs {
        writer.add_transaction(tx.0, tx.1, tx.2, tx.3, tx.4, tx.5, tx.6, tx.7, tx.8);
    }

    let data = writer.finish();

    let sink = client
        .copy_in(
            "COPY transactions_index (hash, block_number, tx_index, is_cellbase, timestamp, \
             inputs_count, outputs_count, fee, cycles) \
             FROM STDIN WITH (FORMAT BINARY)",
        )
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
    fn test_copy_transactions_index_writer() {
        let writer = CopyTransactionsIndexWriter::new();
        let data = writer.finish();
        assert!(data.len() >= 21);
    }
}
