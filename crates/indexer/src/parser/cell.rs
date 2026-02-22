use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::script::ScriptParser;

pub struct ParsedCell {
    pub capacity: i64,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub lock_script_hash: Vec<u8>,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub data_hash: Vec<u8>,
    pub data_size: i32,
    pub data: Vec<u8>,
}

pub struct CellParser;

impl CellParser {
    pub fn parse_outputs(tx: &TransactionView) -> Vec<ParsedCell> {
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .map(|(output, data)| Self::parse_output(output, data))
            .collect()
    }

    pub fn parse_output(output: &CellOutput, data_hex: &str) -> ParsedCell {
        let data = parse_hex_to_bytes(data_hex);
        let data_hash = ScriptParser::compute_data_hash(&data);

        let lock_script_hash = ScriptParser::compute_script_hash(&output.lock);

        let (type_code_hash, type_hash_type, type_args, type_script_hash) =
            if let Some(ref type_script) = output.type_ {
                (
                    Some(parse_hex_to_bytes(&type_script.code_hash)),
                    Some(ScriptParser::hash_type_to_i16(&type_script.hash_type)),
                    Some(parse_hex_to_bytes(&type_script.args)),
                    Some(ScriptParser::compute_script_hash(type_script)),
                )
            } else {
                (None, None, None, None)
            };

        ParsedCell {
            capacity: Self::parse_capacity_i64(&output.capacity),
            lock_code_hash: parse_hex_to_bytes(&output.lock.code_hash),
            lock_hash_type: ScriptParser::hash_type_to_i16(&output.lock.hash_type),
            lock_args: parse_hex_to_bytes(&output.lock.args),
            lock_script_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            type_script_hash,
            data_hash,
            data_size: data.len() as i32,
            data,
        }
    }

    fn parse_capacity_i64(capacity_hex: &str) -> i64 {
        let hex = capacity_hex.strip_prefix("0x").unwrap_or(capacity_hex);
        let parsed = u64::from_str_radix(hex, 16)
            .unwrap_or_else(|e| panic!("invalid cell capacity hex '{}': {}", capacity_hex, e));
        i64::try_from(parsed).unwrap_or_else(|_| {
            panic!(
                "cell capacity over i64 range '{}': {} (max={})",
                capacity_hex,
                parsed,
                i64::MAX
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{CellInput, CellOutput, OutPoint, Script, TransactionView};

    const SECP256K1_CODE_HASH: &str =
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";

    fn create_lock_script() -> Script {
        Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_type_script() -> Script {
        Script {
            code_hash: "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        }
    }

    fn create_cell_output(capacity: &str, with_type: bool) -> CellOutput {
        CellOutput {
            capacity: capacity.to_string(),
            lock: create_lock_script(),
            type_: if with_type {
                Some(create_type_script())
            } else {
                None
            },
        }
    }

    fn create_test_tx() -> TransactionView {
        TransactionView {
            hash: "0x0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                previous_output: OutPoint {
                    tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                    index: "0x0".to_string(),
                },
                since: "0x0".to_string(),
            }],
            outputs: vec![
                create_cell_output("0x174876e800", false),
                create_cell_output("0x2540be400", true),
            ],
            outputs_data: vec!["0x".to_string(), "0xdeadbeef".to_string()],
            witnesses: vec![],
        }
    }

    #[test]
    fn test_parse_capacity_i64_with_prefix() {
        assert_eq!(
            CellParser::parse_capacity_i64("0x174876e800"),
            100_000_000_000
        );
        assert_eq!(
            CellParser::parse_capacity_i64("0x2540be400"),
            10_000_000_000
        );
    }

    #[test]
    fn test_parse_capacity_i64_without_prefix() {
        assert_eq!(
            CellParser::parse_capacity_i64("174876e800"),
            100_000_000_000
        );
    }

    #[test]
    #[should_panic(expected = "invalid cell capacity hex")]
    fn test_parse_capacity_i64_invalid_panics() {
        let _ = CellParser::parse_capacity_i64("invalid");
    }

    #[test]
    #[should_panic(expected = "cell capacity over i64 range")]
    fn test_parse_capacity_i64_overflow_panics() {
        let _ = CellParser::parse_capacity_i64("0x8000000000000000");
    }

    #[test]
    fn test_parse_output_without_type_script() {
        let output = create_cell_output("0x174876e800", false);
        let data_hex = "0x";
        let parsed = CellParser::parse_output(&output, data_hex);

        assert_eq!(parsed.capacity, 100_000_000_000);
        assert_eq!(parsed.lock_code_hash.len(), 32);
        assert_eq!(parsed.lock_hash_type, 1);
        assert_eq!(parsed.lock_args.len(), 20);
        assert!(parsed.type_code_hash.is_none());
        assert!(parsed.type_hash_type.is_none());
        assert!(parsed.type_args.is_none());
        assert!(parsed.type_script_hash.is_none());
        assert_eq!(parsed.data_size, 0);
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn test_parse_output_with_type_script() {
        let output = create_cell_output("0x2540be400", true);
        let data_hex = "0xdeadbeef";
        let parsed = CellParser::parse_output(&output, data_hex);

        assert_eq!(parsed.capacity, 10_000_000_000);
        assert!(parsed.type_code_hash.is_some());
        assert_eq!(parsed.type_code_hash.as_ref().unwrap().len(), 32);
        assert_eq!(parsed.type_hash_type, Some(1));
        assert!(parsed.type_args.is_some());
        assert!(parsed.type_script_hash.is_some());
        assert_eq!(parsed.type_script_hash.as_ref().unwrap().len(), 32);
        assert_eq!(parsed.data_size, 4);
        assert_eq!(parsed.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn test_parse_output_computes_data_hash() {
        let output = create_cell_output("0x174876e800", false);
        let data_hex = "0xdeadbeef";
        let parsed = CellParser::parse_output(&output, data_hex);

        assert_eq!(parsed.data_hash.len(), 32);
        assert_ne!(parsed.data_hash, vec![0u8; 32]);
    }

    #[test]
    fn test_parse_output_computes_lock_script_hash() {
        let output = create_cell_output("0x174876e800", false);
        let parsed = CellParser::parse_output(&output, "0x");

        assert_eq!(parsed.lock_script_hash.len(), 32);
        assert_ne!(parsed.lock_script_hash, vec![0u8; 32]);
    }

    #[test]
    fn test_parse_outputs_multiple() {
        let tx = create_test_tx();
        let parsed = CellParser::parse_outputs(&tx);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].capacity, 100_000_000_000);
        assert!(parsed[0].type_code_hash.is_none());
        assert_eq!(parsed[1].capacity, 10_000_000_000);
        assert!(parsed[1].type_code_hash.is_some());
    }

    #[test]
    fn test_parse_outputs_empty_tx() {
        let tx = TransactionView {
            hash: "0x0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![],
            outputs_data: vec![],
            witnesses: vec![],
        };
        let parsed = CellParser::parse_outputs(&tx);
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_lock_hash_type_mapping() {
        let script_type = Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let script_data = Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "data".to_string(),
            args: "0x".to_string(),
        };
        let script_data1 = Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "data1".to_string(),
            args: "0x".to_string(),
        };

        let output_type = CellOutput {
            capacity: "0x0".to_string(),
            lock: script_type,
            type_: None,
        };
        let output_data = CellOutput {
            capacity: "0x0".to_string(),
            lock: script_data,
            type_: None,
        };
        let output_data1 = CellOutput {
            capacity: "0x0".to_string(),
            lock: script_data1,
            type_: None,
        };

        assert_eq!(
            CellParser::parse_output(&output_type, "0x").lock_hash_type,
            1
        );
        assert_eq!(
            CellParser::parse_output(&output_data, "0x").lock_hash_type,
            0
        );
        assert_eq!(
            CellParser::parse_output(&output_data1, "0x").lock_hash_type,
            2
        );
    }
}
