use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tx_hash() {
        assert_eq!(normalize_tx_hash("0xABCD"), "0xabcd");
        assert_eq!(normalize_tx_hash("ABCD"), "0xabcd");
    }
}
