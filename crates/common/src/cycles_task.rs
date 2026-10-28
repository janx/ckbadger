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

/// One executed CKB lock/type group. Persisted only by lazy execution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ScriptGroupKind {
    Lock,
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScriptGroupCycles {
    pub script_hash: [u8; 32],
    pub group_type: ScriptGroupKind,
    pub cycles: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionCycles {
    pub block_hash: [u8; 32],
    pub cycles: i64,
    pub groups: Vec<ScriptGroupCycles>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TxGroupCyclesRecord {
    Complete(TransactionCycles),
    Failed {
        block_hash: [u8; 32],
        failure: crate::cycles::ReplayFailure,
    },
}

impl TransactionCycles {
    pub fn validate(&self) -> Result<(), String> {
        if self.cycles < 0 {
            return Err("successful transaction cycles cannot be negative".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        let sum = self.groups.iter().try_fold(0_i64, |sum, group| {
            if group.cycles < 0 {
                return Err("successful group cycles cannot be negative".to_string());
            }
            if !seen.insert((group.group_type, group.script_hash)) {
                return Err(format!(
                    "duplicate cycles group: {:?} 0x{}",
                    group.group_type,
                    hex::encode(group.script_hash)
                ));
            }
            sum.checked_add(group.cycles)
                .ok_or_else(|| "group cycles sum overflow".to_string())
        })?;
        if sum != self.cycles {
            return Err(format!(
                "group cycles sum {sum} differs from transaction total {}",
                self.cycles
            ));
        }
        Ok(())
    }
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
