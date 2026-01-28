use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::cell::ParsedCell;

/// live_cells table has 10 columns:
/// tx_hash, output_index, created_at_block, capacity,
/// lock_script_hash, lock_code_hash, lock_args,
/// type_script_hash, type_code_hash, data_size
const LIVE_CELLS_COLUMN_COUNT: i16 = 10;

pub struct CopyLiveCellsWriter {
    buffer: BinaryCopyBuffer,
}

impl Default for CopyLiveCellsWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyLiveCellsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(LIVE_CELLS_COLUMN_COUNT),
        }
    }

    pub fn add_live_cell(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        cell: &ParsedCell,
        created_at_block: i64,
    ) {
        self.buffer.start_row();

        // tx_hash BYTEA NOT NULL
        self.buffer.write_bytea(tx_hash);
        // output_index SMALLINT NOT NULL
        self.buffer.write_i16(output_index);
        // created_at_block BIGINT NOT NULL
        self.buffer.write_i64(created_at_block);
        // capacity BIGINT NOT NULL
        self.buffer.write_i64(cell.capacity);
        // lock_script_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_script_hash);
        // lock_code_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_code_hash);
        // lock_args BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_args);
        // type_script_hash BYTEA (nullable)
        self.buffer
            .write_bytea_opt(cell.type_script_hash.as_deref());
        // type_code_hash BYTEA (nullable)
        self.buffer.write_bytea_opt(cell.type_code_hash.as_deref());
        // data_size INT NOT NULL
        self.buffer.write_i32(cell.data_size);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

pub async fn copy_live_cells(
    client: &Client,
    cells: &[(&[u8], i16, &ParsedCell, i64)],
) -> Result<u64> {
    if cells.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyLiveCellsWriter::new();
    for (tx_hash, output_index, cell, block_number) in cells {
        writer.add_live_cell(tx_hash, *output_index, cell, *block_number);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, type_code_hash, data_size) FROM STDIN WITH (FORMAT BINARY)")
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
    fn test_copy_live_cells_writer() {
        let writer = CopyLiveCellsWriter::new();
        let data = writer.finish();
        // Header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_live_cells_writer_with_data() {
        let mut writer = CopyLiveCellsWriter::new();

        let cell = ParsedCell {
            capacity: 10000000000,
            lock_code_hash: vec![0x02; 32],
            lock_hash_type: 1,
            lock_args: vec![0x03; 20],
            lock_script_hash: vec![0x01; 32],
            type_code_hash: Some(vec![0x05; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x06; 20]),
            type_script_hash: Some(vec![0x04; 32]),
            data_hash: vec![0x07; 32],
            data_size: 100,
            data: vec![],
        };

        let tx_hash = vec![0xaa; 32];
        writer.add_live_cell(&tx_hash, 0, &cell, 12345);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_live_cells_writer_nullable_type() {
        let mut writer = CopyLiveCellsWriter::new();

        let cell = ParsedCell {
            capacity: 10000000000,
            lock_code_hash: vec![0x02; 32],
            lock_hash_type: 1,
            lock_args: vec![0x03; 20],
            lock_script_hash: vec![0x01; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0x07; 32],
            data_size: 0,
            data: vec![],
        };

        let tx_hash = vec![0xbb; 32];
        writer.add_live_cell(&tx_hash, 1, &cell, 67890);

        let data = writer.finish();
        assert!(data.len() > 21);
    }
}
