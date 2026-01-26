use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::script::ScriptParser;

pub const DOTBIT_ACCOUNT_CELL_TYPE_ID: &str =
    "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918";

pub const DOTBIT_DAS_LOCK_TYPE_ID: &str =
    "0x9376c3b5811942960a846691e16e477cf43d7c7fa654067c9948dfcd09a32137";

const HASH_BYTES_LEN: usize = 32;
const ACCOUNT_ID_LEN: usize = 20;

#[derive(Debug, Clone)]
pub struct ParsedDotbitAccount {
    pub account_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub next_account_id: Option<Vec<u8>>,
    pub expired_at: Option<u64>,
    pub owner_lock_hash: Vec<u8>,
}

pub struct DotbitParser;

impl DotbitParser {
    pub fn is_account_cell_type_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);
        code_hash == hash.as_slice()
    }

    pub fn is_das_lock_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(DOTBIT_DAS_LOCK_TYPE_ID);
        code_hash == hash.as_slice()
    }

    pub fn parse_account_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedDotbitAccount> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_account_cell_type_script(&type_code_hash) {
            return None;
        }

        let data = parse_hex_to_bytes(data_hex);

        let min_len = HASH_BYTES_LEN + ACCOUNT_ID_LEN;
        if data.len() < min_len {
            return None;
        }

        let account_id = data[HASH_BYTES_LEN..HASH_BYTES_LEN + ACCOUNT_ID_LEN].to_vec();

        let next_account_id = if data.len() >= HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2 {
            let next_id =
                data[HASH_BYTES_LEN + ACCOUNT_ID_LEN..HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2].to_vec();
            if next_id.iter().all(|&b| b == 0) {
                None
            } else {
                Some(next_id)
            }
        } else {
            None
        };

        let expired_at = if data.len() >= HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2 + 8 {
            let offset = HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2;
            let bytes: [u8; 8] = data[offset..offset + 8].try_into().ok()?;
            Some(u64::from_le_bytes(bytes))
        } else {
            None
        };

        let type_script_hash = ScriptParser::compute_script_hash(type_script);
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock);

        Some(ParsedDotbitAccount {
            account_id,
            type_script_hash,
            next_account_id,
            expired_at,
            owner_lock_hash,
        })
    }

    pub fn parse_accounts(tx: &TransactionView) -> Vec<ParsedDotbitAccount> {
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| Self::parse_account_cell(output, data_hex))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{CellOutput, Script};

    fn create_lock_script() -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_account_cell_type_script() -> Script {
        Script {
            code_hash: DOTBIT_ACCOUNT_CELL_TYPE_ID.to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        }
    }

    fn create_account_cell_data(
        account_id: &[u8; 20],
        next_account_id: Option<&[u8; 20]>,
        expired_at: Option<u64>,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 32];

        data.extend_from_slice(account_id);

        if let Some(next_id) = next_account_id {
            data.extend_from_slice(next_id);
        } else {
            data.extend_from_slice(&[0u8; 20]);
        }

        if let Some(exp) = expired_at {
            data.extend_from_slice(&exp.to_le_bytes());
        }

        data
    }

    #[test]
    fn test_is_account_cell_type_script() {
        let hash = parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);
        assert!(DotbitParser::is_account_cell_type_script(&hash));

        let other = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!DotbitParser::is_account_cell_type_script(&other));
    }

    #[test]
    fn test_parse_account_cell_basic() {
        let account_id: [u8; 20] = [0xab; 20];
        let next_account_id: [u8; 20] = [0xcd; 20];
        let expired_at = 1735689600u64;

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };

        let data = create_account_cell_data(&account_id, Some(&next_account_id), Some(expired_at));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.account_id, account_id.to_vec());
        assert_eq!(parsed.next_account_id, Some(next_account_id.to_vec()));
        assert_eq!(parsed.expired_at, Some(expired_at));
    }

    #[test]
    fn test_parse_account_cell_no_next() {
        let account_id: [u8; 20] = [0xab; 20];
        let expired_at = 1735689600u64;

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };

        let data = create_account_cell_data(&account_id, None, Some(expired_at));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.account_id, account_id.to_vec());
        assert!(parsed.next_account_id.is_none());
    }

    #[test]
    fn test_parse_account_cell_no_type() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };

        let result = DotbitParser::parse_account_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_account_cell_wrong_type() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: "0x0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            }),
        };

        let result = DotbitParser::parse_account_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_account_cell_data_too_short() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };

        let short_data = vec![0u8; 40];
        let data_hex = format!("0x{}", hex::encode(&short_data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_none());
    }
}
