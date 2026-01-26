use crate::rpc::{parse_hex_to_bytes, parse_hex_u32, TransactionView};

pub struct ParsedTransaction {
    pub hash: Vec<u8>,
    pub version: i32,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub witnesses_count: i32,
    pub cell_deps_count: i32,
    pub header_deps_count: i32,
    pub is_cellbase: bool,
    pub tx_size: i32,
}

pub struct ParsedInput {
    pub previous_tx_hash: Vec<u8>,
    pub previous_output_index: i32,
    pub since: i64,
}

pub struct ParsedCellDep {
    pub out_point_tx_hash: Vec<u8>,
    pub out_point_index: i16,
    pub dep_type: i16,
}

pub struct TransactionParser;

impl TransactionParser {
    pub fn parse(tx: &TransactionView) -> ParsedTransaction {
        let is_cellbase = tx.inputs.first().is_some_and(|input| {
            input.previous_output.tx_hash
                == "0x0000000000000000000000000000000000000000000000000000000000000000"
        });

        ParsedTransaction {
            hash: parse_hex_to_bytes(&tx.hash),
            version: parse_hex_u32(&tx.version) as i32,
            inputs_count: tx.inputs.len() as i32,
            outputs_count: tx.outputs.len() as i32,
            witnesses_count: tx.witnesses.len() as i32,
            cell_deps_count: tx.cell_deps.len() as i32,
            header_deps_count: tx.header_deps.len() as i32,
            is_cellbase,
            tx_size: Self::calculate_serialized_size(tx),
        }
    }

    pub fn calculate_serialized_size(tx: &TransactionView) -> i32 {
        const MOLECULE_NUMBER_SIZE: usize = 4;
        const OUTPOINT_SIZE: usize = 36;
        const CELLINPUT_SIZE: usize = 44;

        let mut size = MOLECULE_NUMBER_SIZE;

        size += MOLECULE_NUMBER_SIZE;
        size += MOLECULE_NUMBER_SIZE;

        let raw_tx_size = {
            let mut raw_size = MOLECULE_NUMBER_SIZE;
            raw_size += MOLECULE_NUMBER_SIZE * 6;

            raw_size += MOLECULE_NUMBER_SIZE;

            raw_size += MOLECULE_NUMBER_SIZE;
            for _cell_dep in &tx.cell_deps {
                raw_size += OUTPOINT_SIZE;
                raw_size += 1;
            }

            raw_size += MOLECULE_NUMBER_SIZE;
            raw_size += tx.header_deps.len() * 32;

            raw_size += MOLECULE_NUMBER_SIZE;
            raw_size += tx.inputs.len() * CELLINPUT_SIZE;

            raw_size += MOLECULE_NUMBER_SIZE;
            for output in &tx.outputs {
                let lock_args = parse_hex_to_bytes(&output.lock.args);
                let lock_size =
                    MOLECULE_NUMBER_SIZE + 32 + 1 + MOLECULE_NUMBER_SIZE + lock_args.len();

                let type_size = if let Some(type_script) = &output.type_ {
                    let type_args = parse_hex_to_bytes(&type_script.args);
                    MOLECULE_NUMBER_SIZE + 32 + 1 + MOLECULE_NUMBER_SIZE + type_args.len()
                } else {
                    0
                };

                let output_size =
                    MOLECULE_NUMBER_SIZE + MOLECULE_NUMBER_SIZE * 3 + 8 + lock_size + type_size;
                raw_size += MOLECULE_NUMBER_SIZE + output_size;
            }

            raw_size += MOLECULE_NUMBER_SIZE;
            for output_data in &tx.outputs_data {
                let data = parse_hex_to_bytes(output_data);
                raw_size += MOLECULE_NUMBER_SIZE + data.len();
            }

            raw_size
        };

        size += raw_tx_size;

        size += MOLECULE_NUMBER_SIZE;
        for witness in &tx.witnesses {
            let witness_data = parse_hex_to_bytes(witness);
            size += MOLECULE_NUMBER_SIZE + witness_data.len();
        }

        size as i32
    }

    pub fn parse_inputs(tx: &TransactionView) -> Vec<ParsedInput> {
        tx.inputs
            .iter()
            .map(|input| ParsedInput {
                previous_tx_hash: parse_hex_to_bytes(&input.previous_output.tx_hash),
                previous_output_index: parse_hex_u32(&input.previous_output.index) as i32,
                since: Self::parse_since(&input.since),
            })
            .collect()
    }

    pub fn parse_cell_deps(tx: &TransactionView) -> Vec<ParsedCellDep> {
        tx.cell_deps
            .iter()
            .map(|cell_dep| ParsedCellDep {
                out_point_tx_hash: parse_hex_to_bytes(&cell_dep.out_point.tx_hash),
                out_point_index: parse_hex_u32(&cell_dep.out_point.index) as i16,
                dep_type: match cell_dep.dep_type.as_str() {
                    "dep_group" => 1,
                    _ => 0,
                },
            })
            .collect()
    }

    pub fn calculate_output_capacity(tx: &TransactionView) -> String {
        let total: u128 = tx
            .outputs
            .iter()
            .map(|output| Self::parse_capacity_u128(&output.capacity))
            .sum();
        total.to_string()
    }

    fn parse_since(since_hex: &str) -> i64 {
        let hex = since_hex.strip_prefix("0x").unwrap_or(since_hex);
        u64::from_str_radix(hex, 16).unwrap_or(0) as i64
    }

    fn parse_capacity_u128(capacity_hex: &str) -> u128 {
        let hex = capacity_hex.strip_prefix("0x").unwrap_or(capacity_hex);
        u64::from_str_radix(hex, 16).unwrap_or(0) as u128
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{CellDep, CellInput, CellOutput, OutPoint, Script};

    fn create_script() -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_cellbase_tx() -> TransactionView {
        TransactionView {
            hash: "0x0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                previous_output: OutPoint {
                    tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                    index: "0xffffffff".to_string(),
                },
                since: "0x0".to_string(),
            }],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_script(),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        }
    }

    fn create_normal_tx() -> TransactionView {
        TransactionView {
            hash: "0x0000000000000000000000000000000000000000000000000000000000000002".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![
                CellDep {
                    out_point: OutPoint {
                        tx_hash:
                            "0xe2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c"
                                .to_string(),
                        index: "0x0".to_string(),
                    },
                    dep_type: "dep_group".to_string(),
                },
                CellDep {
                    out_point: OutPoint {
                        tx_hash:
                            "0x8f8c79eb6671709633fe6a46de93c0fedc9c1b8a6527a18d3983879542635c9f"
                                .to_string(),
                        index: "0x1".to_string(),
                    },
                    dep_type: "code".to_string(),
                },
            ],
            header_deps: vec![
                "0x0000000000000000000000000000000000000000000000000000000000000003".to_string(),
            ],
            inputs: vec![
                CellInput {
                    previous_output: OutPoint {
                        tx_hash:
                            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                                .to_string(),
                        index: "0x0".to_string(),
                    },
                    since: "0x0".to_string(),
                },
                CellInput {
                    previous_output: OutPoint {
                        tx_hash:
                            "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                                .to_string(),
                        index: "0x2".to_string(),
                    },
                    since: "0x400000000a000001".to_string(),
                },
            ],
            outputs: vec![
                CellOutput {
                    capacity: "0x174876e800".to_string(),
                    lock: create_script(),
                    type_: None,
                },
                CellOutput {
                    capacity: "0x2540be400".to_string(),
                    lock: create_script(),
                    type_: Some(Script {
                        code_hash:
                            "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
                                .to_string(),
                        hash_type: "type".to_string(),
                        args: "0x".to_string(),
                    }),
                },
            ],
            outputs_data: vec!["0x".to_string(), "0xdeadbeef".to_string()],
            witnesses: vec!["0x55000000".to_string(), "0x".to_string()],
        }
    }

    #[test]
    fn test_parse_detects_cellbase() {
        let cellbase = create_cellbase_tx();
        let parsed = TransactionParser::parse(&cellbase);

        assert!(parsed.is_cellbase);
    }

    #[test]
    fn test_parse_detects_non_cellbase() {
        let tx = create_normal_tx();
        let parsed = TransactionParser::parse(&tx);

        assert!(!parsed.is_cellbase);
    }

    #[test]
    fn test_parse_counts_components() {
        let tx = create_normal_tx();
        let parsed = TransactionParser::parse(&tx);

        assert_eq!(parsed.inputs_count, 2);
        assert_eq!(parsed.outputs_count, 2);
        assert_eq!(parsed.cell_deps_count, 2);
        assert_eq!(parsed.header_deps_count, 1);
        assert_eq!(parsed.witnesses_count, 2);
    }

    #[test]
    fn test_parse_extracts_hash() {
        let tx = create_normal_tx();
        let parsed = TransactionParser::parse(&tx);

        assert_eq!(parsed.hash.len(), 32);
        assert_eq!(parsed.hash[31], 0x02);
    }

    #[test]
    fn test_parse_extracts_version() {
        let tx = create_normal_tx();
        let parsed = TransactionParser::parse(&tx);

        assert_eq!(parsed.version, 0);
    }

    #[test]
    fn test_parse_inputs() {
        let tx = create_normal_tx();
        let inputs = TransactionParser::parse_inputs(&tx);

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].previous_tx_hash.len(), 32);
        assert_eq!(inputs[0].previous_output_index, 0);
        assert_eq!(inputs[0].since, 0);
        assert_eq!(inputs[1].previous_output_index, 2);
        assert_ne!(inputs[1].since, 0);
    }

    #[test]
    fn test_parse_since_absolute() {
        let since_hex = "0x400000000a000001";
        let since = TransactionParser::parse_since(since_hex);
        assert_eq!(since, 0x400000000a000001u64 as i64);
    }

    #[test]
    fn test_parse_cell_deps() {
        let tx = create_normal_tx();
        let cell_deps = TransactionParser::parse_cell_deps(&tx);

        assert_eq!(cell_deps.len(), 2);
        assert_eq!(cell_deps[0].out_point_tx_hash.len(), 32);
        assert_eq!(cell_deps[0].out_point_index, 0);
        assert_eq!(cell_deps[0].dep_type, 1);
        assert_eq!(cell_deps[1].out_point_index, 1);
        assert_eq!(cell_deps[1].dep_type, 0);
    }

    #[test]
    fn test_calculate_output_capacity() {
        let tx = create_normal_tx();
        let total = TransactionParser::calculate_output_capacity(&tx);

        let expected: u128 = 100_000_000_000 + 10_000_000_000;
        assert_eq!(total, expected.to_string());
    }

    #[test]
    fn test_calculate_serialized_size_cellbase() {
        let tx = create_cellbase_tx();
        let size = TransactionParser::calculate_serialized_size(&tx);

        assert!(size > 0);
        assert!(size < 1000);
    }

    #[test]
    fn test_calculate_serialized_size_normal_tx() {
        let tx = create_normal_tx();
        let size = TransactionParser::calculate_serialized_size(&tx);

        assert!(size > 0);
        assert!(
            size > TransactionParser::calculate_serialized_size(&create_cellbase_tx()),
            "normal tx should be larger than cellbase"
        );
    }

    #[test]
    fn test_parse_capacity_u128() {
        assert_eq!(
            TransactionParser::parse_capacity_u128("0x174876e800"),
            100_000_000_000u128
        );
        assert_eq!(TransactionParser::parse_capacity_u128("0x0"), 0u128);
        assert_eq!(TransactionParser::parse_capacity_u128("invalid"), 0u128);
    }

    #[test]
    fn test_tx_size_is_stored() {
        let tx = create_normal_tx();
        let parsed = TransactionParser::parse(&tx);

        assert!(parsed.tx_size > 0);
    }
}
