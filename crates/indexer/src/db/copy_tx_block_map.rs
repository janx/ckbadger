use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;

/// tx_block_map table has 2 columns:
/// tx_hash (BYTEA), block_number (BIGINT)
const TX_BLOCK_MAP_COLUMN_COUNT: i16 = 2;

pub struct CopyTxBlockMapWriter {
    buffer: BinaryCopyBuffer,
}

impl Default for CopyTxBlockMapWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyTxBlockMapWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(TX_BLOCK_MAP_COLUMN_COUNT),
        }
    }

    pub fn add_tx_block_map(&mut self, tx_hash: &[u8], block_number: i64) {
        self.buffer.start_row();
        self.buffer.write_bytea(tx_hash);
        self.buffer.write_i64(block_number);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

/// Execute COPY for tx_block_map
pub async fn copy_tx_block_map(client: &Client, rows: &[(&[u8], i64)]) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyTxBlockMapWriter::new();
    for (tx_hash, block_number) in rows {
        writer.add_tx_block_map(tx_hash, *block_number);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY tx_block_map (tx_hash, block_number) FROM STDIN WITH (FORMAT BINARY)")
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows_written = sink.finish().await?;

    Ok(rows_written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_tx_block_map_writer_creates_buffer() {
        let writer = CopyTxBlockMapWriter::new();
        let data = writer.finish();
        // Header is 19 bytes, should have at least that
        assert!(data.len() >= 19);
    }

    #[test]
    fn test_copy_tx_block_map_writer_adds_row() {
        let mut writer = CopyTxBlockMapWriter::new();
        writer.add_tx_block_map(&[0x01; 32], 12345);
        let data = writer.finish();
        // Header (19) + row header (2) + tx_hash (4 + 32) + block_number (4 + 8) = 69 bytes min
        assert!(data.len() >= 69);
    }
}
