use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::ParsedUdtCell;

/// UDT cells table columns for COPY:
/// tx_hash, output_index, type_script_hash, type_code_hash, type_hash_type, type_args,
/// lock_script_hash, amount, standard, is_live, created_at_block
const UDT_CELLS_COLUMN_COUNT: i16 = 11;

pub struct CopyUdtCellsWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyUdtCellsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(UDT_CELLS_COLUMN_COUNT),
            row_count: 0,
        }
    }

    pub fn add_cell(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        cell: &ParsedUdtCell,
        created_at_block: i64,
    ) {
        self.buffer.start_row();

        // tx_hash BYTEA NOT NULL
        self.buffer.write_bytea(tx_hash);
        // output_index SMALLINT NOT NULL
        self.buffer.write_i16(output_index);
        // type_script_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.type_script_hash);
        // type_code_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.type_code_hash);
        // type_hash_type SMALLINT NOT NULL
        self.buffer.write_i16(cell.type_hash_type);
        // type_args BYTEA NOT NULL
        self.buffer.write_bytea(&cell.type_args);
        // lock_script_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_script_hash);
        // amount NUMERIC(40,0) NOT NULL
        self.buffer.write_numeric(&cell.amount.to_string());
        // standard TEXT NOT NULL
        self.buffer.write_text(cell.standard.as_str());
        // is_live BOOLEAN NOT NULL DEFAULT TRUE
        self.buffer.write_bool(true);
        // created_at_block BIGINT NOT NULL
        self.buffer.write_i64(created_at_block);

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

impl Default for CopyUdtCellsWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute COPY for udt_cells
///
/// This is much faster than INSERT for bulk operations because:
/// 1. No SQL parsing overhead
/// 2. Binary format - no text conversion
/// 3. No conflict checking (assumes no duplicates during bulk sync)
///
/// # Arguments
/// * `client` - tokio-postgres client from CopyPoolManager
/// * `cells` - Slice of tuples (tx_hash, output_index, ParsedUdtCell, created_at_block)
///
/// # Returns
/// Number of rows inserted
pub async fn copy_udt_cells(
    client: &Client,
    cells: &[(&[u8], i16, &ParsedUdtCell, i64)],
) -> Result<u64> {
    if cells.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyUdtCellsWriter::new();
    for (tx_hash, output_index, cell, block_number) in cells {
        writer.add_cell(tx_hash, *output_index, cell, *block_number);
    }

    let data = writer.finish();

    let sink = client
        .copy_in(
            "COPY udt_cells (tx_hash, output_index, type_script_hash, type_code_hash, \
             type_hash_type, type_args, lock_script_hash, amount, standard, is_live, \
             created_at_block) FROM STDIN WITH (FORMAT BINARY)",
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
    use crate::parser::UdtStandard;

    fn create_test_udt_cell() -> ParsedUdtCell {
        ParsedUdtCell {
            type_script_hash: vec![0x01; 32],
            type_code_hash: vec![0x02; 32],
            type_hash_type: 1,
            type_args: vec![0x03; 20],
            lock_script_hash: vec![0x04; 32],
            amount: 1000000000000u128,
            standard: UdtStandard::Sudt,
        }
    }

    #[test]
    fn test_copy_udt_cells_writer_creates_buffer() {
        let writer = CopyUdtCellsWriter::new();
        let data = writer.finish();
        // Should have header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_udt_cells_writer_is_empty() {
        let writer = CopyUdtCellsWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_udt_cells_writer_add_cell() {
        let mut writer = CopyUdtCellsWriter::new();
        let cell = create_test_udt_cell();
        let tx_hash = [0xaa; 32];

        writer.add_cell(&tx_hash, 0, &cell, 12345);

        assert!(!writer.is_empty());
        assert_eq!(writer.row_count(), 1);

        let data = writer.finish();
        // Header (19) + column_count (2) + data + trailer (2)
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_udt_cells_writer_multiple_cells() {
        let mut writer = CopyUdtCellsWriter::new();
        let cell1 = create_test_udt_cell();
        let cell2 = ParsedUdtCell {
            type_script_hash: vec![0x11; 32],
            type_code_hash: vec![0x12; 32],
            type_hash_type: 0,
            type_args: vec![0x13; 20],
            lock_script_hash: vec![0x14; 32],
            amount: 999999999999u128,
            standard: UdtStandard::Xudt,
        };

        let tx_hash1 = [0xaa; 32];
        let tx_hash2 = [0xbb; 32];

        writer.add_cell(&tx_hash1, 0, &cell1, 12345);
        writer.add_cell(&tx_hash2, 1, &cell2, 12346);

        assert_eq!(writer.row_count(), 2);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_udt_cells_writer_large_amount() {
        let mut writer = CopyUdtCellsWriter::new();
        let cell = ParsedUdtCell {
            type_script_hash: vec![0x01; 32],
            type_code_hash: vec![0x02; 32],
            type_hash_type: 1,
            type_args: vec![0x03; 20],
            lock_script_hash: vec![0x04; 32],
            // Very large amount (u128 max territory)
            amount: 340282366920938463463374607431768211455u128,
            standard: UdtStandard::Sudt,
        };

        let tx_hash = [0xaa; 32];
        writer.add_cell(&tx_hash, 0, &cell, 12345);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_udt_cells_writer_default() {
        let writer = CopyUdtCellsWriter::default();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }
}
