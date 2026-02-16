//! Task command queue types for Redis-based API → task-runner communication.
//!
//! The API opens RocksDB in secondary (read-only) mode, so all task mutation
//! operations are sent as commands through a Redis queue. The task-runner
//! (which has primary write access) consumes and executes them.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Redis key for the command queue (LIST).
pub const TASK_CMD_QUEUE_KEY: &str = "task:cmd:queue";

/// Redis key prefix for command payloads.
const TASK_CMD_PREFIX: &str = "task:cmd:";

/// Redis key prefix for command results.
const TASK_CMD_RESULT_PREFIX: &str = "task:cmd:result:";

/// TTL for command payloads in seconds.
pub const TASK_CMD_TTL_SECS: u64 = 60;

/// TTL for command results in seconds.
pub const TASK_CMD_RESULT_TTL_SECS: u64 = 30;

/// Returns the Redis key for a command payload: `task:cmd:{cmd_id}`
pub fn task_cmd_key(cmd_id: &Uuid) -> String {
    format!("{}{}", TASK_CMD_PREFIX, cmd_id)
}

/// Returns the Redis key for a command result: `task:cmd:result:{cmd_id}`
pub fn task_cmd_result_key(cmd_id: &Uuid) -> String {
    format!("{}{}", TASK_CMD_RESULT_PREFIX, cmd_id)
}

/// A command sent from the API to the task-runner via Redis.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCommand {
    /// Unique ID for this command (used to correlate results).
    pub id: Uuid,
    /// The action to perform.
    pub action: TaskCommandAction,
}

/// The action a task command should perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskCommandAction {
    /// Create a new task.
    Create {
        task_type: String,
        config: serde_json::Value,
    },
    /// Cancel a pending/running/paused task.
    Cancel { task_id: Uuid },
    /// Pause a running task.
    Pause { task_id: Uuid },
    /// Resume a paused task.
    Resume { task_id: Uuid },
    /// Retry a failed task.
    Retry { task_id: Uuid },
    /// Delete a completed/failed/cancelled task.
    Delete { task_id: Uuid },
}

/// The result of executing a task command, published by the task-runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCommandResult {
    /// The command ID this result corresponds to.
    pub cmd_id: Uuid,
    /// Whether the command succeeded.
    pub success: bool,
    /// The affected task ID (for create, this is the new task ID).
    pub task_id: Option<Uuid>,
    /// Error message if the command failed.
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_helpers() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            task_cmd_key(&id),
            "task:cmd:550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(
            task_cmd_result_key(&id),
            "task:cmd:result:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_command_serialization_roundtrip() {
        let cmd = TaskCommand {
            id: Uuid::new_v4(),
            action: TaskCommandAction::Cancel {
                task_id: Uuid::new_v4(),
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: TaskCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd.id, deserialized.id);
    }

    #[test]
    fn test_create_action_serialization() {
        let cmd = TaskCommand {
            id: Uuid::new_v4(),
            action: TaskCommandAction::Create {
                task_type: "label_import".to_string(),
                config: serde_json::json!({"tokenLabelsPath": "docs/token-labels"}),
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"create\""));
        let deserialized: TaskCommand = serde_json::from_str(&json).unwrap();
        match deserialized.action {
            TaskCommandAction::Create { task_type, .. } => {
                assert_eq!(task_type, "label_import");
            }
            _ => panic!("Expected Create action"),
        }
    }

    #[test]
    fn test_result_serialization_roundtrip() {
        let result = TaskCommandResult {
            cmd_id: Uuid::new_v4(),
            success: true,
            task_id: Some(Uuid::new_v4()),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: TaskCommandResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.cmd_id, deserialized.cmd_id);
        assert!(deserialized.success);
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_result_with_error() {
        let result = TaskCommandResult {
            cmd_id: Uuid::new_v4(),
            success: false,
            task_id: None,
            error: Some("Task not found".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: TaskCommandResult = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.success);
        assert_eq!(deserialized.error.as_deref(), Some("Task not found"));
    }

    #[test]
    fn test_all_action_variants_serialize() {
        let task_id = Uuid::new_v4();
        let actions = vec![
            TaskCommandAction::Create {
                task_type: "label_import".to_string(),
                config: serde_json::Value::Null,
            },
            TaskCommandAction::Cancel { task_id },
            TaskCommandAction::Pause { task_id },
            TaskCommandAction::Resume { task_id },
            TaskCommandAction::Retry { task_id },
            TaskCommandAction::Delete { task_id },
        ];
        for action in actions {
            let cmd = TaskCommand {
                id: Uuid::new_v4(),
                action,
            };
            let json = serde_json::to_string(&cmd).unwrap();
            let _: TaskCommand = serde_json::from_str(&json).unwrap();
        }
    }
}
