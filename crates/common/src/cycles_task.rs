use serde::{Deserialize, Serialize};

pub const CYCLES_TASK_QUEUE_KEY: &str = "cycles:task:queue";
pub const CYCLES_TASK_LOCK_PREFIX: &str = "cycles:task:lock:";
pub const CYCLES_TASK_RESULT_PREFIX: &str = "cycles:task:result:";
pub const CYCLES_TASK_LOCK_TTL_SECS: u64 = 120;
pub const CYCLES_TASK_RESULT_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CyclesTaskStatus {
    Done,
    Failed,
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CyclesTaskResult {
    pub status: CyclesTaskStatus,
    pub cycles: Option<i64>,
    pub error: Option<String>,
    pub updated_at: i64,
}

pub fn normalize_tx_hash(hash: &str) -> String {
    let h = hash.strip_prefix("0x").unwrap_or(hash);
    format!("0x{}", h.to_lowercase())
}

pub fn cycles_task_lock_key(hash: &str) -> String {
    format!("{}{}", CYCLES_TASK_LOCK_PREFIX, normalize_tx_hash(hash))
}

pub fn cycles_task_result_key(hash: &str) -> String {
    format!("{}{}", CYCLES_TASK_RESULT_PREFIX, normalize_tx_hash(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tx_hash() {
        assert_eq!(normalize_tx_hash("0xABCD"), "0xabcd");
        assert_eq!(normalize_tx_hash("ABCD"), "0xabcd");
    }

    #[test]
    fn test_cycles_task_keys_include_normalized_hash() {
        assert_eq!(
            cycles_task_lock_key("0xABCD"),
            "cycles:task:lock:0xabcd".to_string()
        );
        assert_eq!(
            cycles_task_result_key("ABCD"),
            "cycles:task:result:0xabcd".to_string()
        );
    }
}
