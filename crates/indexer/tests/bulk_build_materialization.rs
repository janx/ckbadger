use ckbadger_indexer::sync::run_sample_bulk_materialization_for_test;

#[test]
fn materializer_streams_append_only_rows_before_final_snapshot() {
    let report = run_sample_bulk_materialization_for_test().expect("sample bulk materialization");

    assert!(report.streamed_history_rows > 0);
    assert!(report.final_snapshot_rows > 0);
}
