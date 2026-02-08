use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::transaction::{ParsedCellDep, ParsedInput};

/// transaction_inputs: tx_hash, tx_block_number, input_index, previous_tx_hash, previous_output_index, since
const INPUTS_COLUMN_COUNT: i16 = 6;

/// transaction_cell_deps: tx_hash, tx_block_number, dep_index, out_point_tx_hash, out_point_index, dep_type
const CELL_DEPS_COLUMN_COUNT: i16 = 6;

pub struct CopyInputsWriter {
    buffer: BinaryCopyBuffer,
}

impl Default for CopyInputsWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyInputsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(INPUTS_COLUMN_COUNT),
        }
    }

    pub fn add_input(
        &mut self,
        tx_hash: &[u8],
        tx_block_number: i64,
        input_index: i16,
        input: &ParsedInput,
    ) {
        self.buffer.start_row();
        self.buffer.write_bytea(tx_hash);
        self.buffer.write_i64(tx_block_number);
        self.buffer.write_i16(input_index);
        self.buffer.write_bytea(&input.previous_tx_hash);
        self.buffer.write_i16(input.previous_output_index as i16);
        self.buffer.write_i64(input.since);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

pub struct CopyCellDepsWriter {
    buffer: BinaryCopyBuffer,
}

impl Default for CopyCellDepsWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyCellDepsWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(CELL_DEPS_COLUMN_COUNT),
        }
    }

    pub fn add_cell_dep(
        &mut self,
        tx_hash: &[u8],
        tx_block_number: i64,
        dep_index: i16,
        dep: &ParsedCellDep,
    ) {
        self.buffer.start_row();
        self.buffer.write_bytea(tx_hash);
        self.buffer.write_i64(tx_block_number);
        self.buffer.write_i16(dep_index);
        self.buffer.write_bytea(&dep.out_point_tx_hash);
        self.buffer.write_i16(dep.out_point_index);
        self.buffer.write_i16(dep.dep_type);
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }
}

pub async fn copy_inputs(
    client: &Client,
    inputs: &[(&[u8], i64, i16, &ParsedInput)],
) -> Result<u64> {
    if inputs.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyInputsWriter::new();
    for (tx_hash, block_number, input_index, input) in inputs {
        writer.add_input(tx_hash, *block_number, *input_index, input);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY transaction_inputs (tx_hash, tx_block_number, input_index, previous_tx_hash, previous_output_index, since) FROM STDIN WITH (FORMAT BINARY)")
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

pub async fn copy_cell_deps(
    client: &Client,
    cell_deps: &[(&[u8], i64, i16, &ParsedCellDep)],
) -> Result<u64> {
    if cell_deps.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyCellDepsWriter::new();
    for (tx_hash, block_number, dep_index, dep) in cell_deps {
        writer.add_cell_dep(tx_hash, *block_number, *dep_index, dep);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY transaction_cell_deps (tx_hash, tx_block_number, dep_index, out_point_tx_hash, out_point_index, dep_type) FROM STDIN WITH (FORMAT BINARY)")
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
    fn test_copy_inputs_writer() {
        let writer = CopyInputsWriter::new();
        let data = writer.finish();
        // Header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_cell_deps_writer() {
        let writer = CopyCellDepsWriter::new();
        let data = writer.finish();
        // Header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_inputs_writer_with_data() {
        let mut writer = CopyInputsWriter::new();
        let tx_hash = vec![0u8; 32];
        let input = ParsedInput {
            previous_tx_hash: [1u8; 32],
            previous_output_index: 5,
            since: 12345,
        };

        writer.add_input(&tx_hash, 100, 0, &input);
        let data = writer.finish();

        // Header (19) + column_count (2) + row_data + trailer (2)
        // row_data: bytea(32) + i64 + i16 + bytea(32) + i16 + i64
        //         = (4+32) + (4+8) + (4+2) + (4+32) + (4+2) + (4+8)
        //         = 36 + 12 + 6 + 36 + 6 + 12 = 108
        assert_eq!(data.len(), 19 + 2 + 108 + 2);
    }

    #[test]
    fn test_copy_cell_deps_writer_with_data() {
        let mut writer = CopyCellDepsWriter::new();
        let tx_hash = vec![0u8; 32];
        let dep = ParsedCellDep {
            out_point_tx_hash: [1u8; 32],
            out_point_index: 3,
            dep_type: 1,
        };

        writer.add_cell_dep(&tx_hash, 200, 0, &dep);
        let data = writer.finish();

        // Header (19) + column_count (2) + row_data + trailer (2)
        // row_data: bytea(32) + i64 + i16 + bytea(32) + i16 + i16
        //         = (4+32) + (4+8) + (4+2) + (4+32) + (4+2) + (4+2)
        //         = 36 + 12 + 6 + 36 + 6 + 6 = 102
        assert_eq!(data.len(), 19 + 2 + 102 + 2);
    }

    #[test]
    fn test_copy_inputs_writer_multiple_rows() {
        let mut writer = CopyInputsWriter::new();
        let tx_hash = vec![0u8; 32];

        for i in 0..3 {
            let input = ParsedInput {
                previous_tx_hash: [i as u8; 32],
                previous_output_index: i,
                since: i as i64 * 1000,
            };
            writer.add_input(&tx_hash, 100, i as i16, &input);
        }

        let data = writer.finish();
        // Header (19) + (column_count (2) + row_data (108)) * 3 + trailer (2)
        assert_eq!(data.len(), 19 + (2 + 108) * 3 + 2);
    }

    #[test]
    fn test_copy_cell_deps_writer_multiple_rows() {
        let mut writer = CopyCellDepsWriter::new();
        let tx_hash = vec![0u8; 32];

        for i in 0..3 {
            let dep = ParsedCellDep {
                out_point_tx_hash: [i as u8; 32],
                out_point_index: i as i16,
                dep_type: (i % 2) as i16,
            };
            writer.add_cell_dep(&tx_hash, 200, i as i16, &dep);
        }

        let data = writer.finish();
        // Header (19) + (column_count (2) + row_data (102)) * 3 + trailer (2)
        assert_eq!(data.len(), 19 + (2 + 102) * 3 + 2);
    }
}
