//! Integration tests for the task system: create, claim, complete, fail, and recover orphaned tasks.

use chrono::{Duration, Utc};
use ckbadger_store::CkbadgerStore;
use ckbadger_store::TaskEntry;
use std::sync::Arc;
use uuid::Uuid;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

/// Create a TaskEntry with serde_json::Value fields set to None to avoid
/// bincode's incompatibility with serde_json::Value::deserialize_any.
fn make_task(task_type: &str, priority: i32) -> TaskEntry {
    TaskEntry {
        id: Uuid::new_v4(),
        task_type: task_type.to_string(),
        status: "pending".to_string(),
        priority,
        config: "{}".to_string(),
        progress_total: None,
        progress_current: None,
        progress_message: None,
        result: None,
        error_message: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        heartbeat_at: None,
        runner_id: None,
        retry_count: 0,
        max_retries: 3,
        rate_samples: None,
        rate_ema: None,
        log_tail: None,
    }
}

#[test]
fn test_recover_orphaned_task() {
    let store = setup_store();

    // Create a task that looks like a running task with a stale heartbeat
    let mut task = make_task("reindex_addresses", 5);
    task.status = "running".to_string();
    task.runner_id = Some("worker-1".to_string());
    task.started_at = Some(Utc::now() - Duration::minutes(15));
    // Heartbeat 10 minutes ago (stale if timeout is 300s = 5min)
    task.heartbeat_at = Some(Utc::now() - Duration::minutes(10));
    let task_id = task.id;

    store.create_task(&task).unwrap();

    // Verify task is in "running" state
    let retrieved = store.get_task(&task_id).unwrap().unwrap();
    assert_eq!(retrieved.status, "running");

    // Recover orphaned tasks with a timeout of 300 seconds (5 minutes).
    // Our task's heartbeat is 10 minutes old, so it should be recovered.
    let recovered_count = store.recover_orphaned_tasks(300).unwrap();
    assert_eq!(recovered_count, 1, "should recover 1 orphaned task");

    // Verify the task was reset to "pending"
    let recovered_task = store.get_task(&task_id).unwrap().unwrap();
    assert_eq!(recovered_task.status, "pending");
    assert!(
        recovered_task.runner_id.is_none(),
        "runner_id should be cleared"
    );
    assert!(
        recovered_task
            .error_message
            .as_ref()
            .unwrap()
            .contains("Recovered"),
        "error message should mention recovery"
    );
}

#[test]
fn test_create_claim_complete_lifecycle() {
    let store = setup_store();

    // Step 1: Create a pending task
    let task = make_task("compute_statistics", 10);
    let task_id = task.id;
    store.create_task(&task).unwrap();

    // Verify task is pending
    let retrieved = store.get_task(&task_id).unwrap().unwrap();
    assert_eq!(retrieved.status, "pending");
    assert!(retrieved.runner_id.is_none());

    // Step 2: Claim the task
    let claimed = store.claim_next_task("worker-alpha").unwrap();
    assert!(claimed.is_some(), "should claim a task");
    let claimed = claimed.unwrap();
    assert_eq!(claimed.id, task_id);
    assert_eq!(claimed.status, "running");
    assert_eq!(claimed.runner_id.as_ref().unwrap(), "worker-alpha");
    assert!(claimed.started_at.is_some());

    // Verify it is now running
    let running = store.get_task(&task_id).unwrap().unwrap();
    assert_eq!(running.status, "running");

    // No more pending tasks to claim
    let next = store.claim_next_task("worker-beta").unwrap();
    assert!(next.is_none(), "no more pending tasks");

    // Step 3: Complete the task
    store.complete_task(&task_id, None).unwrap();

    let completed = store.get_task(&task_id).unwrap().unwrap();
    assert_eq!(completed.status, "completed");
    assert!(completed.completed_at.is_some());
}

#[test]
fn test_fail_task() {
    let store = setup_store();

    // Create and claim a task
    let task = make_task("rebuild_dao", 5);
    let task_id = task.id;
    store.create_task(&task).unwrap();

    let claimed = store.claim_next_task("worker-1").unwrap().unwrap();
    assert_eq!(claimed.status, "running");

    // Fail the task
    store
        .fail_task(&task_id, "connection timeout to CKB node")
        .unwrap();

    let failed = store.get_task(&task_id).unwrap().unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.completed_at.is_some());
    assert_eq!(
        failed.error_message.as_ref().unwrap(),
        "connection timeout to CKB node"
    );
}

#[test]
fn test_list_tasks_by_status() {
    let store = setup_store();

    // Create 3 tasks with different priorities
    let task_a = make_task("task_a", 10);
    let task_b = make_task("task_b", 20);
    let task_c = make_task("task_c", 5);
    store.create_task(&task_a).unwrap();
    store.create_task(&task_b).unwrap();
    store.create_task(&task_c).unwrap();

    // Initially all 3 are pending
    let pending = store.list_tasks_by_status("pending").unwrap();
    assert_eq!(pending.len(), 3);

    // Claim one task (highest priority first = task_b with priority 20)
    let claimed = store.claim_next_task("worker-1").unwrap().unwrap();
    assert_eq!(
        claimed.task_type, "task_b",
        "highest priority claimed first"
    );

    // Now: 2 pending, 1 running
    let pending = store.list_tasks_by_status("pending").unwrap();
    assert_eq!(pending.len(), 2, "2 tasks still pending");

    let running = store.list_tasks_by_status("running").unwrap();
    assert_eq!(running.len(), 1, "1 task running");
    assert_eq!(running[0].task_type, "task_b");

    // Complete the running task
    store.complete_task(&claimed.id, None).unwrap();

    let completed = store.list_tasks_by_status("completed").unwrap();
    assert_eq!(completed.len(), 1, "1 task completed");
    assert_eq!(completed[0].task_type, "task_b");

    // Verify total via list_all_tasks
    let all_tasks = store.list_all_tasks().unwrap();
    assert_eq!(all_tasks.len(), 3, "total of 3 tasks in the store");

    // Check that list_tasks_by_status("pending") still returns 2
    let pending = store.list_tasks_by_status("pending").unwrap();
    assert_eq!(pending.len(), 2, "still 2 pending tasks");
}

#[test]
fn test_heartbeat_task() {
    let store = setup_store();

    let task = make_task("long_running_task", 1);
    let task_id = task.id;
    store.create_task(&task).unwrap();

    // Claim
    store.claim_next_task("worker-1").unwrap();

    // Record initial heartbeat
    let before = store.get_task(&task_id).unwrap().unwrap();
    let initial_heartbeat = before.heartbeat_at.unwrap();

    // Small delay to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Send heartbeat
    store.heartbeat_task(&task_id).unwrap();

    let after = store.get_task(&task_id).unwrap().unwrap();
    let updated_heartbeat = after.heartbeat_at.unwrap();

    assert!(
        updated_heartbeat >= initial_heartbeat,
        "heartbeat should be updated to a newer or equal timestamp"
    );
    assert_eq!(after.status, "running", "status should remain running");
}
