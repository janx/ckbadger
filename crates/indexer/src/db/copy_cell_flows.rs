use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;

/// 8 columns: block_number, tx_hash, output_index, flow_type, lock_script_hash, capacity, data_size, consumed_by_tx
const CELL_FLOWS_COLUMN_COUNT: i16 = 8;

pub struct CopyCellFlowsWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyCellFlowsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(CELL_FLOWS_COLUMN_COUNT),
            row_count: 0,
        }
    }

    /// Add a cell flow record to the buffer
    ///
    /// # Arguments
    /// * `block_number` - Block number where the flow occurred
    /// * `tx_hash` - Transaction hash
    /// * `output_index` - Cell's output index
    /// * `flow_type` - 0=created (output), 1=consumed (input)
    /// * `lock_script_hash` - Lock script hash of the cell
    /// * `capacity` - Capacity in shannons
    /// * `data_size` - Size of cell data in bytes
    /// * `consumed_by_tx` - Consuming tx hash (only for flow_type=1)
    #[allow(clippy::too_many_arguments)]
    pub fn add_flow(
        &mut self,
        block_number: i64,
        tx_hash: &[u8],
        output_index: i16,
        flow_type: i16,
        lock_script_hash: &[u8],
        capacity: i64,
        data_size: i32,
        consumed_by_tx: Option<&[u8]>,
    ) {
        self.buffer.start_row();

        // block_number (BIGINT)
        self.buffer.write_i64(block_number);
        // tx_hash (BYTEA)
        self.buffer.write_bytea(tx_hash);
        // output_index (SMALLINT)
        self.buffer.write_i16(output_index);
        // flow_type (SMALLINT)
        self.buffer.write_i16(flow_type);
        // lock_script_hash (BYTEA)
        self.buffer.write_bytea(lock_script_hash);
        // capacity (BIGINT)
        self.buffer.write_i64(capacity);
        // data_size (INTEGER)
        self.buffer.write_i32(data_size);
        // consumed_by_tx (BYTEA, nullable)
        self.buffer.write_bytea_opt(consumed_by_tx);

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

impl Default for CopyCellFlowsWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Copy cell flows to database using binary COPY protocol
#[allow(clippy::type_complexity)]
pub async fn copy_cell_flows(
    client: &Client,
    flows: &[(i64, &[u8], i16, i16, &[u8], i64, i32, Option<&[u8]>)],
) -> Result<u64> {
    if flows.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyCellFlowsWriter::new();
    for &(
        block_number,
        tx_hash,
        output_index,
        flow_type,
        lock_script_hash,
        capacity,
        data_size,
        consumed_by_tx,
    ) in flows
    {
        writer.add_flow(
            block_number,
            tx_hash,
            output_index,
            flow_type,
            lock_script_hash,
            capacity,
            data_size,
            consumed_by_tx,
        );
    }

    let data = writer.finish();

    let sink = client
        .copy_in(
            "COPY cell_flows (block_number, tx_hash, output_index, flow_type, \
             lock_script_hash, capacity, data_size, consumed_by_tx) FROM STDIN WITH (FORMAT BINARY)",
        )
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

/// Delete all cell flows at or after a block number (for reorg)
pub async fn delete_cell_flows_from(client: &Client, from_block: i64) -> Result<u64> {
    let result = client
        .execute(
            "DELETE FROM cell_flows WHERE block_number >= $1",
            &[&from_block],
        )
        .await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_cell_flows_writer_creates_buffer() {
        let writer = CopyCellFlowsWriter::new();
        let data = writer.finish();
        // At minimum, has header (19 bytes) + trailer (2 bytes)
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_cell_flows_writer_is_empty() {
        let writer = CopyCellFlowsWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_cell_flows_writer_add_flow() {
        let mut writer = CopyCellFlowsWriter::new();

        writer.add_flow(
            12345,
            &[0xaa; 32],
            0,
            0, // created
            &[0xbb; 32],
            100_000_000,
            64,
            None,
        );

        assert!(!writer.is_empty());
        assert_eq!(writer.row_count(), 1);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_cell_flows_writer_multiple_flows() {
        let mut writer = CopyCellFlowsWriter::new();

        // Created flow
        writer.add_flow(12345, &[0xaa; 32], 0, 0, &[0xbb; 32], 100_000_000, 64, None);
        // Consumed flow
        writer.add_flow(
            12346,
            &[0xaa; 32],
            0,
            1,
            &[0xbb; 32],
            100_000_000,
            64,
            Some(&[0xee; 32]),
        );
        // Another created flow
        writer.add_flow(
            12345,
            &[0xcc; 32],
            1,
            0,
            &[0xdd; 32],
            200_000_000,
            128,
            None,
        );

        assert_eq!(writer.row_count(), 3);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_cell_flows_writer_default() {
        let writer = CopyCellFlowsWriter::default();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }
}
