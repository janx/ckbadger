use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::cell::ParsedCell;

/// Cells table has 16 columns:
/// tx_hash, output_index, capacity, lock_code_hash, lock_hash_type, lock_args,
/// lock_script_hash, type_code_hash, type_hash_type, type_args, type_script_hash,
/// data_hash, data_size, data, status, created_at_block
const CELLS_COLUMN_COUNT: i16 = 16;

/// Maximum cell data preview size (matches writer.rs)
const CELL_DATA_PREVIEW_SIZE: usize = 512;

pub struct CopyCellsWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyCellsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(CELLS_COLUMN_COUNT),
            row_count: 0,
        }
    }

    pub fn add_cell(
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
        // capacity NUMERIC NOT NULL (stored as i64)
        self.buffer.write_i64(cell.capacity);
        // lock_code_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_code_hash);
        // lock_hash_type SMALLINT NOT NULL
        self.buffer.write_i16(cell.lock_hash_type);
        // lock_args BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_args);
        // lock_script_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.lock_script_hash);
        // type_code_hash BYTEA (nullable)
        self.buffer.write_bytea_opt(cell.type_code_hash.as_deref());
        // type_hash_type SMALLINT (nullable)
        self.buffer.write_i16_opt(cell.type_hash_type);
        // type_args BYTEA (nullable)
        self.buffer.write_bytea_opt(cell.type_args.as_deref());
        // type_script_hash BYTEA (nullable)
        self.buffer
            .write_bytea_opt(cell.type_script_hash.as_deref());
        // data_hash BYTEA NOT NULL
        self.buffer.write_bytea(&cell.data_hash);
        // data_size INT NOT NULL
        self.buffer.write_i32(cell.data_size);
        // data BYTEA (nullable, truncated to 512 bytes)
        if cell.data.is_empty() {
            self.buffer.write_null();
        } else {
            let preview = &cell.data[..cell.data.len().min(CELL_DATA_PREVIEW_SIZE)];
            self.buffer.write_bytea(preview);
        }
        // status SMALLINT NOT NULL (0 = live)
        self.buffer.write_i16(0);
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

impl Default for CopyCellsWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute COPY for cells
///
/// # Arguments
/// * `client` - tokio-postgres client from CopyPoolManager
/// * `cells` - Slice of tuples (tx_hash, output_index, ParsedCell, created_at_block)
///
/// # Returns
/// Number of rows inserted
///
/// # Example
/// ```no_run
/// use ckbadger_indexer::db::copy_cells::copy_cells;
/// use ckbadger_indexer::parser::cell::ParsedCell;
///
/// # async fn example(client: &tokio_postgres::Client, cells: Vec<ParsedCell>) -> anyhow::Result<()> {
/// let tx_hash: &[u8] = &[0u8; 32];
/// let data: Vec<_> = cells.iter().enumerate()
///     .map(|(i, cell)| (tx_hash, i as i16, cell, 12345i64))
///     .collect();
/// let rows = copy_cells(client, &data).await?;
/// println!("Inserted {} rows", rows);
/// # Ok(())
/// # }
/// ```
pub async fn copy_cells(client: &Client, cells: &[(&[u8], i16, &ParsedCell, i64)]) -> Result<u64> {
    if cells.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyCellsWriter::new();
    for (tx_hash, output_index, cell, block_number) in cells {
        writer.add_cell(tx_hash, *output_index, cell, *block_number);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY cells (tx_hash, output_index, capacity, lock_code_hash, lock_hash_type, lock_args, lock_script_hash, type_code_hash, type_hash_type, type_args, type_script_hash, data_hash, data_size, data, status, created_at_block) FROM STDIN WITH (FORMAT BINARY)")
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

    fn create_test_cell() -> ParsedCell {
        ParsedCell {
            capacity: 100_000_000_000,
            lock_code_hash: vec![0x01; 32],
            lock_hash_type: 1,
            lock_args: vec![0x02; 20],
            lock_script_hash: vec![0x03; 32],
            type_code_hash: Some(vec![0x04; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x05; 10]),
            type_script_hash: Some(vec![0x06; 32]),
            data_hash: vec![0x07; 32],
            data_size: 4,
            data: vec![0xde, 0xad, 0xbe, 0xef],
        }
    }

    #[test]
    fn test_copy_cells_writer_creates_buffer() {
        let writer = CopyCellsWriter::new();
        let data = writer.finish();
        // Should have header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_cells_writer_is_empty() {
        let writer = CopyCellsWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_cells_writer_add_cell() {
        let mut writer = CopyCellsWriter::new();
        let cell = create_test_cell();
        let tx_hash = [0xaa; 32];

        writer.add_cell(&tx_hash, 0, &cell, 12345);

        assert!(!writer.is_empty());
        assert_eq!(writer.row_count(), 1);

        let data = writer.finish();
        // Header (19) + column_count (2) + data + trailer (2)
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_cells_writer_multiple_cells() {
        let mut writer = CopyCellsWriter::new();
        let cell1 = create_test_cell();
        let cell2 = ParsedCell {
            capacity: 200_000_000_000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 0,
            lock_args: vec![0x12; 20],
            lock_script_hash: vec![0x13; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0x17; 32],
            data_size: 0,
            data: vec![],
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
    fn test_copy_cells_writer_empty_data() {
        let mut writer = CopyCellsWriter::new();
        let cell = ParsedCell {
            capacity: 100_000_000_000,
            lock_code_hash: vec![0x01; 32],
            lock_hash_type: 1,
            lock_args: vec![0x02; 20],
            lock_script_hash: vec![0x03; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0x07; 32],
            data_size: 0,
            data: vec![],
        };

        let tx_hash = [0xaa; 32];
        writer.add_cell(&tx_hash, 0, &cell, 12345);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_cells_writer_large_data_truncated() {
        let mut writer = CopyCellsWriter::new();
        let large_data = vec![0xff; 1024]; // 1KB data
        let cell = ParsedCell {
            capacity: 100_000_000_000,
            lock_code_hash: vec![0x01; 32],
            lock_hash_type: 1,
            lock_args: vec![0x02; 20],
            lock_script_hash: vec![0x03; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0x07; 32],
            data_size: 1024,
            data: large_data,
        };

        let tx_hash = [0xaa; 32];
        writer.add_cell(&tx_hash, 0, &cell, 12345);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        // Data should be truncated to 512 bytes in the buffer
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_cells_writer_nullable_type_script() {
        let mut writer = CopyCellsWriter::new();
        let cell_with_type = create_test_cell();
        let cell_without_type = ParsedCell {
            capacity: 100_000_000_000,
            lock_code_hash: vec![0x01; 32],
            lock_hash_type: 1,
            lock_args: vec![0x02; 20],
            lock_script_hash: vec![0x03; 32],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0x07; 32],
            data_size: 0,
            data: vec![],
        };

        let tx_hash = [0xaa; 32];
        writer.add_cell(&tx_hash, 0, &cell_with_type, 12345);
        writer.add_cell(&tx_hash, 1, &cell_without_type, 12345);

        assert_eq!(writer.row_count(), 2);
        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_cells_writer_default() {
        let writer = CopyCellsWriter::default();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }
}
