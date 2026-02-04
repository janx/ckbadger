use chrono::NaiveDate;
use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn get_daily_stats(pool: &PgPool, date: NaiveDate) -> Option<(i64, i64)> {
    sqlx::query_as::<_, (i64, i64)>(
        "SELECT total_live_cells, total_data_size FROM daily_statistics WHERE date = $1",
    )
    .bind(date)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_data_size_increases_with_new_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();

    writer
        .update_daily_statistics(
            date, 1,    // blocks_count
            10,   // transactions_count
            5,    // cells_created
            0,    // cells_consumed
            0,    // capacity_transferred
            500,  // data_size_added (500 bytes)
            0,    // data_size_consumed
            None, // dao_field
        )
        .await
        .unwrap();

    let (live_cells, data_size) = get_daily_stats(&pool, date).await.unwrap();
    assert_eq!(live_cells, 5);
    assert_eq!(data_size, 500);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_data_size_decreases_when_cells_consumed(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let date1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let date2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    // Day 1: Create 5 cells with 500 bytes total
    writer
        .update_daily_statistics(date1, 1, 10, 5, 0, 0, 500, 0, None)
        .await
        .unwrap();

    // Day 2: Create 3 cells (300 bytes), consume 2 cells (200 bytes)
    // Net: +1 cell, +100 bytes
    writer
        .update_daily_statistics(date2, 1, 5, 3, 2, 0, 300, 200, None)
        .await
        .unwrap();

    let (live_cells, data_size) = get_daily_stats(&pool, date2).await.unwrap();
    assert_eq!(live_cells, 6); // 5 + 3 - 2 = 6
    assert_eq!(data_size, 600); // 500 + 300 - 200 = 600
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_data_size_net_negative_when_more_consumed(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let date1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let date2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    // Day 1: Create 10 cells with 1000 bytes
    writer
        .update_daily_statistics(date1, 1, 10, 10, 0, 0, 1000, 0, None)
        .await
        .unwrap();

    // Day 2: Create 2 cells (100 bytes), consume 5 cells (600 bytes)
    // Net: -3 cells, -500 bytes
    writer
        .update_daily_statistics(date2, 1, 5, 2, 5, 0, 100, 600, None)
        .await
        .unwrap();

    let (live_cells, data_size) = get_daily_stats(&pool, date2).await.unwrap();
    assert_eq!(live_cells, 7); // 10 + 2 - 5 = 7
    assert_eq!(data_size, 500); // 1000 + 100 - 600 = 500
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_same_day_multiple_updates_accumulate(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();

    // First batch: 3 cells, 300 bytes
    writer
        .update_daily_statistics(date, 1, 5, 3, 0, 0, 300, 0, None)
        .await
        .unwrap();

    // Second batch on same day: 2 cells created, 1 consumed
    // data: +200 bytes created, 100 bytes consumed
    writer
        .update_daily_statistics(date, 1, 3, 2, 1, 0, 200, 100, None)
        .await
        .unwrap();

    let (live_cells, data_size) = get_daily_stats(&pool, date).await.unwrap();
    assert_eq!(live_cells, 4); // 3 + 2 - 1 = 4
    assert_eq!(data_size, 400); // 300 + 200 - 100 = 400
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cumulative_tracking_across_days(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let day1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let day2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let day3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    // Day 1: +10 cells, +1000 bytes
    writer
        .update_daily_statistics(day1, 1, 10, 10, 0, 0, 1000, 0, None)
        .await
        .unwrap();

    // Day 2: +5 cells, -3 cells = +2 net, +500 - 300 = +200 bytes net
    writer
        .update_daily_statistics(day2, 1, 8, 5, 3, 0, 500, 300, None)
        .await
        .unwrap();

    // Day 3: +2 cells, -8 cells = -6 net, +100 - 700 = -600 bytes net
    writer
        .update_daily_statistics(day3, 1, 10, 2, 8, 0, 100, 700, None)
        .await
        .unwrap();

    // Verify cumulative values for each day
    let (cells1, size1) = get_daily_stats(&pool, day1).await.unwrap();
    assert_eq!(cells1, 10);
    assert_eq!(size1, 1000);

    let (cells2, size2) = get_daily_stats(&pool, day2).await.unwrap();
    assert_eq!(cells2, 12); // 10 + 2
    assert_eq!(size2, 1200); // 1000 + 200

    let (cells3, size3) = get_daily_stats(&pool, day3).await.unwrap();
    assert_eq!(cells3, 6); // 12 - 6
    assert_eq!(size3, 600); // 1200 - 600
}
