//! Deferred State Tests
//!
//! Tests for loading and using deferred state flags from database.
//! Verifies that indexer correctly reads activities_deferred, address_balances_deferred,
//! and token_deferred from sync_status table.

use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_deferred_flags_default_to_false(pool: PgPool) {
    let (activities, address_balances, token): (bool, bool, bool) = sqlx::query_as(
        r#"SELECT 
            COALESCE(activities_deferred, false),
            COALESCE(address_balances_deferred, false),
            COALESCE(token_deferred, false)
        FROM sync_status WHERE id = 1"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(!activities, "activities_deferred should default to false");
    assert!(
        !address_balances,
        "address_balances_deferred should default to false"
    );
    assert!(!token, "token_deferred should default to false");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_activities_deferred_can_be_set(pool: PgPool) {
    sqlx::query(
        "UPDATE sync_status SET activities_deferred = TRUE, activities_deferred_at = NOW() WHERE id = 1",
    )
    .execute(&pool)
    .await
    .unwrap();

    let deferred: (bool,) =
        sqlx::query_as("SELECT COALESCE(activities_deferred, false) FROM sync_status WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(
        deferred.0,
        "activities_deferred should be true after update"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_address_balances_deferred_can_be_set(pool: PgPool) {
    sqlx::query(
        "UPDATE sync_status SET address_balances_deferred = TRUE, address_balances_deferred_at = NOW() WHERE id = 1",
    )
    .execute(&pool)
    .await
    .unwrap();

    let deferred: (bool,) = sqlx::query_as(
        "SELECT COALESCE(address_balances_deferred, false) FROM sync_status WHERE id = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        deferred.0,
        "address_balances_deferred should be true after update"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_token_deferred_can_be_set(pool: PgPool) {
    sqlx::query(
        "UPDATE sync_status SET token_deferred = TRUE, token_deferred_at = NOW() WHERE id = 1",
    )
    .execute(&pool)
    .await
    .unwrap();

    let deferred: (bool,) =
        sqlx::query_as("SELECT COALESCE(token_deferred, false) FROM sync_status WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(deferred.0, "token_deferred should be true after update");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_all_deferred_flags_can_be_set_together(pool: PgPool) {
    sqlx::query(
        r#"UPDATE sync_status SET 
            activities_deferred = TRUE, activities_deferred_at = NOW(),
            address_balances_deferred = TRUE, address_balances_deferred_at = NOW(),
            token_deferred = TRUE, token_deferred_at = NOW()
        WHERE id = 1"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (activities, address_balances, token): (bool, bool, bool) = sqlx::query_as(
        r#"SELECT 
            COALESCE(activities_deferred, false),
            COALESCE(address_balances_deferred, false),
            COALESCE(token_deferred, false)
        FROM sync_status WHERE id = 1"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(activities, "activities_deferred should be true");
    assert!(address_balances, "address_balances_deferred should be true");
    assert!(token, "token_deferred should be true");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_deferred_flags_can_be_cleared(pool: PgPool) {
    sqlx::query(
        r#"UPDATE sync_status SET 
            activities_deferred = TRUE,
            address_balances_deferred = TRUE,
            token_deferred = TRUE
        WHERE id = 1"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"UPDATE sync_status SET 
            activities_deferred = FALSE,
            address_balances_deferred = FALSE,
            token_deferred = FALSE
        WHERE id = 1"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (activities, address_balances, token): (bool, bool, bool) = sqlx::query_as(
        r#"SELECT 
            COALESCE(activities_deferred, false),
            COALESCE(address_balances_deferred, false),
            COALESCE(token_deferred, false)
        FROM sync_status WHERE id = 1"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        !activities,
        "activities_deferred should be false after clear"
    );
    assert!(
        !address_balances,
        "address_balances_deferred should be false after clear"
    );
    assert!(!token, "token_deferred should be false after clear");
}
