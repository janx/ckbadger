use ckbadger_indexer::db::copy_cells::CopyCellsWriter;
use ckbadger_indexer::db::copy_format::BinaryCopyBuffer;
use ckbadger_indexer::db::copy_inputs::{CopyCellDepsWriter, CopyInputsWriter};
use ckbadger_indexer::db::copy_live_cells::CopyLiveCellsWriter;
use ckbadger_indexer::db::copy_transactions::CopyTransactionsWriter;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_indexer::parser::transaction::{ParsedCellDep, ParsedInput};

fn create_test_cell() -> ParsedCell {
    ParsedCell {
        capacity: 100_00000000,
        lock_code_hash: vec![0u8; 32],
        lock_hash_type: 0,
        lock_args: vec![1, 2, 3],
        lock_script_hash: vec![0u8; 32],
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        type_script_hash: None,
        data_hash: vec![0u8; 32],
        data_size: 0,
        data: vec![],
    }
}

fn create_test_input() -> ParsedInput {
    ParsedInput {
        previous_tx_hash: vec![0u8; 32],
        previous_output_index: 0,
        since: 0,
    }
}

fn create_test_cell_dep() -> ParsedCellDep {
    ParsedCellDep {
        out_point_tx_hash: vec![0u8; 32],
        out_point_index: 0,
        dep_type: 0,
    }
}

#[test]
fn test_copy_format_header_structure() {
    let buf = BinaryCopyBuffer::new(3);
    let data = buf.finish();

    assert_eq!(&data[0..11], b"PGCOPY\n\xff\r\n\0");
    assert_eq!(&data[11..15], &[0, 0, 0, 0]);
    assert_eq!(&data[15..19], &[0, 0, 0, 0]);
    assert_eq!(&data[19..21], &[0xFF, 0xFF]);
}

#[test]
fn test_cells_writer_produces_valid_binary() {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    let mut writer = CopyCellsWriter::new();
    writer.add_cell(&tx_hash, 0, &cell, 1000);
    let data = writer.finish();

    assert!(&data[0..11] == b"PGCOPY\n\xff\r\n\0");
    assert!(data.len() > 100);
}

#[test]
fn test_transactions_writer_produces_valid_binary() {
    use chrono::Utc;

    let mut writer = CopyTransactionsWriter::new();
    writer.add_transaction(
        &[0u8; 32],
        1000,
        0,
        0,
        1,
        2,
        1,
        1,
        0,
        100_00000000,
        99_00000000,
        1_00000000,
        Some(500),
        Some(1000000),
        false,
        Utc::now(),
    );
    let data = writer.finish();

    assert!(&data[0..11] == b"PGCOPY\n\xff\r\n\0");
    assert!(data.len() > 50);
}

#[test]
fn test_inputs_writer_produces_valid_binary() {
    let input = create_test_input();
    let tx_hash = vec![0u8; 32];

    let mut writer = CopyInputsWriter::new();
    writer.add_input(&tx_hash, 1000, 0, &input);
    let data = writer.finish();

    assert!(&data[0..11] == b"PGCOPY\n\xff\r\n\0");
    assert!(data.len() > 50);
}

#[test]
fn test_cell_deps_writer_produces_valid_binary() {
    let dep = create_test_cell_dep();
    let tx_hash = vec![0u8; 32];

    let mut writer = CopyCellDepsWriter::new();
    writer.add_cell_dep(&tx_hash, 1000, 0, &dep);
    let data = writer.finish();

    assert!(&data[0..11] == b"PGCOPY\n\xff\r\n\0");
    assert!(data.len() > 50);
}

#[test]
fn test_live_cells_writer_produces_valid_binary() {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    let mut writer = CopyLiveCellsWriter::new();
    writer.add_live_cell(&tx_hash, 0, &cell, 1000);
    let data = writer.finish();

    assert!(&data[0..11] == b"PGCOPY\n\xff\r\n\0");
    assert!(data.len() > 50);
}

#[test]
fn test_multiple_cells_produces_larger_buffer() {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    let mut writer1 = CopyCellsWriter::new();
    writer1.add_cell(&tx_hash, 0, &cell, 1000);
    let data1 = writer1.finish();

    let mut writer2 = CopyCellsWriter::new();
    writer2.add_cell(&tx_hash, 0, &cell, 1000);
    writer2.add_cell(&tx_hash, 1, &cell, 1000);
    writer2.add_cell(&tx_hash, 2, &cell, 1000);
    let data2 = writer2.finish();

    assert!(data2.len() > data1.len());
    assert!(data2.len() > data1.len() * 2);
}

#[test]
fn test_cells_with_type_script() {
    let mut cell = create_test_cell();
    cell.type_code_hash = Some(vec![1u8; 32]);
    cell.type_hash_type = Some(1);
    cell.type_args = Some(vec![2, 3, 4]);
    cell.type_script_hash = Some(vec![5u8; 32]);

    let tx_hash = vec![0u8; 32];

    let mut writer = CopyCellsWriter::new();
    writer.add_cell(&tx_hash, 0, &cell, 1000);
    let data = writer.finish();

    assert!(&data[0..11] == b"PGCOPY\n\xff\r\n\0");
    assert!(data.len() > 150);
}

#[test]
fn test_partition_distribution() {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    let cells_p0: Vec<(&[u8], i16, &ParsedCell, i64)> = vec![(&tx_hash, 0, &cell, 1_000_000)];

    let cells_p1: Vec<(&[u8], i16, &ParsedCell, i64)> = vec![(&tx_hash, 0, &cell, 6_000_000)];

    assert_eq!(cells_p0[0].3 / 5_000_000, 0);
    assert_eq!(cells_p1[0].3 / 5_000_000, 1);
}
