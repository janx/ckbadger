use std::sync::LazyLock;

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::script::ScriptParser;

pub const DAO_CODE_HASH: &str =
    "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";

static DAO_CODE_HASH_BYTES: LazyLock<Vec<u8>> = LazyLock::new(|| parse_hex_to_bytes(DAO_CODE_HASH));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaoState {
    Deposit,
    WithdrawRequest,
}

#[derive(Debug, Clone)]
pub struct ParsedDaoCell {
    pub lock_script_hash: Vec<u8>,
    pub capacity: i64,
    pub state: DaoState,
    pub deposit_block_number: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ParsedDaoDeposit {
    pub tx_hash: Vec<u8>,
    pub output_index: i32,
    pub lock_script_hash: Vec<u8>,
    pub capacity: i64,
}

#[derive(Debug, Clone)]
pub struct ParsedDaoWithdrawRequest {
    pub tx_hash: Vec<u8>,
    pub output_index: i32,
    pub lock_script_hash: Vec<u8>,
    pub capacity: i64,
    pub deposit_block_number: u64,
    pub original_tx_hash: Vec<u8>,
    pub original_output_index: i32,
}

pub struct DaoParser;

fn checked_usize_to_i32(value: usize, context: &str) -> anyhow::Result<i32> {
    i32::try_from(value).map_err(|_| {
        anyhow::anyhow!(
            "DAO index exceeds i32 range while parsing {}: {}",
            context,
            value
        )
    })
}

impl DaoParser {
    pub fn is_dao_code_hash(code_hash: &[u8]) -> bool {
        code_hash == DAO_CODE_HASH_BYTES.as_slice()
    }

    pub fn is_dao_cell(output: &CellOutput) -> bool {
        if let Some(ref type_script) = output.type_ {
            if type_script.hash_type != "type" {
                return false;
            }
            let code_hash = parse_hex_to_bytes(&type_script.code_hash);
            return Self::is_dao_code_hash(&code_hash);
        }
        false
    }

    pub fn parse_dao_state(data: &[u8]) -> Option<DaoState> {
        if data.len() != 8 {
            return None;
        }

        if data == [0u8; 8] {
            Some(DaoState::Deposit)
        } else {
            Some(DaoState::WithdrawRequest)
        }
    }

    pub fn parse_deposit_block_number(data: &[u8]) -> Option<u64> {
        if data.len() != 8 {
            return None;
        }
        if data == [0u8; 8] {
            return None;
        }
        let bytes: [u8; 8] = data.try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    }

    pub fn parse_dao_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedDaoCell> {
        if !Self::is_dao_cell(output) {
            return None;
        }

        let data = parse_hex_to_bytes(data_hex);
        if data.len() != 8 {
            panic!(
                "DAO cell data length invariant violation while parsing DAO cell: expected=8, got={}, data_hex={}",
                data.len(),
                data_hex
            );
        }
        let state = Self::parse_dao_state(&data)
            .unwrap_or_else(|| panic!("DAO state parse failed with validated 8-byte data"));
        let lock_script_hash = ScriptParser::compute_script_hash(&output.lock);
        let deposit_block_number = Self::parse_deposit_block_number(&data);

        Some(ParsedDaoCell {
            lock_script_hash,
            capacity: super::parse_capacity_i64(&output.capacity),
            state,
            deposit_block_number,
        })
    }

    pub fn parse_deposits_from_cells(
        tx_hash: &[u8],
        cells: &[super::cell::ParsedCell],
    ) -> anyhow::Result<Vec<ParsedDaoDeposit>> {
        let dao_hash = &*DAO_CODE_HASH_BYTES;
        cells
            .iter()
            .enumerate()
            .map(|(idx, cell)| -> anyhow::Result<Option<ParsedDaoDeposit>> {
                let type_code_hash = match cell.type_code_hash.as_ref() {
                    Some(v) => v,
                    None => return Ok(None),
                };
                if type_code_hash != dao_hash {
                    return Ok(None);
                }
                // hash_type must be "type" (value 1) to match is_dao_cell() behavior
                if cell.type_hash_type != Some(1) {
                    return Ok(None);
                }
                if cell.data_size != 8 {
                    return Ok(None);
                }
                if cell.data != [0u8; 8] {
                    return Ok(None);
                }
                Ok(Some(ParsedDaoDeposit {
                    tx_hash: tx_hash.to_vec(),
                    output_index: checked_usize_to_i32(idx, "deposit output index")?,
                    lock_script_hash: cell.lock_script_hash.clone(),
                    capacity: cell.capacity,
                }))
            })
            .filter_map(|r| r.transpose())
            .collect::<anyhow::Result<Vec<_>>>()
    }

    pub fn parse_withdraw_requests(
        tx: &TransactionView,
        tx_hash: &[u8],
        input_cells: &[(Vec<u8>, i32, CellOutput, String)],
    ) -> anyhow::Result<Vec<ParsedDaoWithdrawRequest>> {
        super::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .enumerate()
            .map(|(idx, (output, data_hex))| -> anyhow::Result<Option<ParsedDaoWithdrawRequest>> {
                let dao_cell = match Self::parse_dao_cell(output, data_hex) {
                    Some(c) => c,
                    None => return Ok(None),
                };
                if dao_cell.state != DaoState::WithdrawRequest {
                    return Ok(None);
                }

                let deposit_block_number = dao_cell.deposit_block_number
                    .ok_or_else(|| anyhow::anyhow!(
                        "DAO WithdrawRequest at output index {} has no deposit_block_number, tx_hash=0x{}",
                        idx, hex::encode(tx_hash)
                    ))?;

                let (orig_tx, orig_idx, _, _) = input_cells.get(idx)
                    .ok_or_else(|| anyhow::anyhow!(
                        "DAO WithdrawRequest at output index {} has no corresponding input cell \
                         (input_cells.len()={}), tx_hash=0x{}",
                        idx, input_cells.len(), hex::encode(tx_hash)
                    ))?;

                Ok(Some(ParsedDaoWithdrawRequest {
                    tx_hash: tx_hash.to_vec(),
                    output_index: checked_usize_to_i32(idx, "withdraw request output index")?,
                    lock_script_hash: dao_cell.lock_script_hash,
                    capacity: dao_cell.capacity,
                    deposit_block_number,
                    original_tx_hash: orig_tx.clone(),
                    original_output_index: *orig_idx,
                }))
            })
            .filter_map(|r| r.transpose())
            .collect::<anyhow::Result<Vec<_>>>()
    }

    pub fn extract_ar_from_dao_field(dao: &[u8]) -> Option<u64> {
        if dao.len() < 16 {
            return None;
        }
        let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dao_code_hash() {
        let dao_hash = parse_hex_to_bytes(
            "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e",
        );
        assert!(DaoParser::is_dao_code_hash(&dao_hash));

        let other_hash = vec![0u8; 32];
        assert!(!DaoParser::is_dao_code_hash(&other_hash));

        let secp_hash = parse_hex_to_bytes(
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
        );
        assert!(!DaoParser::is_dao_code_hash(&secp_hash));
    }

    #[test]
    fn test_parse_dao_state_deposit() {
        let deposit_data = [0u8; 8];
        assert_eq!(
            DaoParser::parse_dao_state(&deposit_data),
            Some(DaoState::Deposit)
        );
    }

    #[test]
    fn test_parse_dao_state_withdraw_request() {
        let block_number: u64 = 12345;
        let withdraw_data = block_number.to_le_bytes();
        assert_eq!(
            DaoParser::parse_dao_state(&withdraw_data),
            Some(DaoState::WithdrawRequest)
        );
    }

    #[test]
    fn test_parse_dao_state_invalid_length() {
        let invalid_data = [0u8; 4];
        assert_eq!(DaoParser::parse_dao_state(&invalid_data), None);
    }

    #[test]
    fn test_parse_deposit_block_number() {
        let block_number: u64 = 123456789;
        let data = block_number.to_le_bytes();
        assert_eq!(
            DaoParser::parse_deposit_block_number(&data),
            Some(123456789)
        );
    }

    #[test]
    fn test_checked_usize_to_i32_accepts_valid_value() {
        assert_eq!(checked_usize_to_i32(123, "test").unwrap(), 123);
    }

    #[test]
    fn test_checked_usize_to_i32_errors_on_overflow() {
        let err =
            checked_usize_to_i32((i32::MAX as usize) + 1, "deposit output index").unwrap_err();
        assert!(err.to_string().contains("exceeds i32 range"));
    }

    #[test]
    fn test_parse_deposit_block_number_zero_is_none() {
        let data = [0u8; 8];
        assert_eq!(DaoParser::parse_deposit_block_number(&data), None);
    }

    #[test]
    #[should_panic(expected = "DAO cell data length invariant violation while parsing DAO cell")]
    fn test_parse_dao_cell_panics_on_invalid_data_length_for_dao_type() {
        let output = crate::rpc::CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: crate::rpc::Script {
                code_hash: "0x".to_string() + &"11".repeat(32),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            },
            type_: Some(crate::rpc::Script {
                code_hash: DAO_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            }),
        };

        let _ = DaoParser::parse_dao_cell(&output, "0x0102");
    }

    #[test]
    fn test_extract_ar_from_dao_field() {
        let mut dao = vec![0u8; 32];
        let ar: u64 = 0x0001_0000_0000_0000;
        dao[8..16].copy_from_slice(&ar.to_le_bytes());

        assert_eq!(DaoParser::extract_ar_from_dao_field(&dao), Some(ar));
    }

    #[test]
    fn test_extract_ar_from_dao_field_invalid_length() {
        let short_dao = vec![0u8; 8];
        assert_eq!(DaoParser::extract_ar_from_dao_field(&short_dao), None);
    }

    #[test]
    fn test_calculate_compensation_basic() {
        use ckbadger_common::dao::calculate_dao_compensation_from_ar;
        let capacity: i64 = 200_00000000;
        let ar_deposit: u64 = 10_000_000_000_000_000;
        let ar_withdraw: u64 = 10_100_000_000_000_000;

        let compensation =
            calculate_dao_compensation_from_ar(capacity, ar_deposit, ar_withdraw).unwrap();

        let free_capacity = (capacity as u128) - 102_00000000u128;
        let expected = (free_capacity * ar_withdraw as u128 / ar_deposit as u128) - free_capacity;
        assert_eq!(compensation, expected as i64);
    }

    #[test]
    fn test_calculate_compensation_zero_ar_deposit() {
        use ckbadger_common::dao::calculate_dao_compensation_from_ar;
        // ar_deposit == 0 is invalid chain data (AR starts at 10^16), returns Err
        let result = calculate_dao_compensation_from_ar(200_00000000, 0, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_compensation_no_growth() {
        use ckbadger_common::dao::calculate_dao_compensation_from_ar;
        let ar = 10_000_000_000_000_000u64;
        let compensation = calculate_dao_compensation_from_ar(200_00000000, ar, ar).unwrap();
        assert_eq!(compensation, 0);
    }

    #[test]
    fn test_calculate_compensation_returns_err_when_capacity_below_occupied() {
        use ckbadger_common::dao::calculate_dao_compensation_from_ar;
        let result = calculate_dao_compensation_from_ar(100, 10, 11);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_deposits_from_cells_deposit() {
        use super::super::cell::ParsedCell;

        let dao_hash = parse_hex_to_bytes(DAO_CODE_HASH);
        let cells = vec![ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            lock_script_hash: vec![1; 32],
            type_code_hash: Some(dao_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![2; 32]),
            data_hash: [0; 32],
            data_size: 8,
            data: vec![0; 8],
        }];

        let deposits = DaoParser::parse_deposits_from_cells(&[0; 32], &cells).unwrap();
        assert_eq!(deposits.len(), 1);
        assert_eq!(deposits[0].capacity, 100_00000000);
    }

    #[test]
    fn test_parse_deposits_from_cells_withdrawing_not_counted() {
        use super::super::cell::ParsedCell;

        let dao_hash = parse_hex_to_bytes(DAO_CODE_HASH);
        let block_num: u64 = 12345;
        let cells = vec![ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            lock_script_hash: vec![1; 32],
            type_code_hash: Some(dao_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![2; 32]),
            data_hash: [0; 32],
            data_size: 8,
            data: block_num.to_le_bytes().to_vec(),
        }];

        let deposits = DaoParser::parse_deposits_from_cells(&[0; 32], &cells).unwrap();
        assert_eq!(deposits.len(), 0);
    }

    #[test]
    fn test_parse_withdraw_requests_errors_on_missing_input() {
        let dao_code_hash = DAO_CODE_HASH.to_string();
        let lock = crate::rpc::Script {
            code_hash: "0x".to_string() + &"11".repeat(32),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let dao_type = crate::rpc::Script {
            code_hash: dao_code_hash,
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let output = crate::rpc::CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: lock.clone(),
            type_: Some(dao_type.clone()),
        };
        let tx = crate::rpc::TransactionView {
            hash: "0x".to_string() + &"aa".repeat(32),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![output.clone(), output],
            outputs_data: vec![
                "0x0100000000000000".to_string(),
                "0x0200000000000000".to_string(),
            ],
            witnesses: vec![],
        };

        let input_cells = vec![(
            vec![0xAB; 32],
            0,
            crate::rpc::CellOutput {
                capacity: "0x174876e800".to_string(),
                lock,
                type_: Some(dao_type),
            },
            "0x00".to_string(),
        )];

        // Should error: output[0] matches input[0], but output[1] has
        // no corresponding input — this is an invariant violation.
        let err = DaoParser::parse_withdraw_requests(&tx, &[0xCC; 32], &input_cells).unwrap_err();
        assert!(err.to_string().contains("no corresponding input cell"));
    }

    #[test]
    fn test_parse_withdraw_requests_positional_match() {
        let dao_code_hash = DAO_CODE_HASH.to_string();
        let lock = crate::rpc::Script {
            code_hash: "0x".to_string() + &"11".repeat(32),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let dao_type = crate::rpc::Script {
            code_hash: dao_code_hash,
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let output = crate::rpc::CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: lock.clone(),
            type_: Some(dao_type.clone()),
        };
        let tx = crate::rpc::TransactionView {
            hash: "0x".to_string() + &"aa".repeat(32),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![output],
            outputs_data: vec!["0x0100000000000000".to_string()],
            witnesses: vec![],
        };

        let input_cells = vec![(
            vec![0xAB; 32],
            0,
            crate::rpc::CellOutput {
                capacity: "0x174876e800".to_string(),
                lock,
                type_: Some(dao_type),
            },
            "0x00".to_string(),
        )];

        let parsed = DaoParser::parse_withdraw_requests(&tx, &[0xCC; 32], &input_cells).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].original_tx_hash, vec![0xAB; 32]);
        assert_eq!(parsed[0].original_output_index, 0);
    }

    #[test]
    #[should_panic(expected = "outputs/outputs_data length mismatch")]
    fn test_parse_withdraw_requests_panics_on_outputs_data_length_mismatch() {
        let tx = crate::rpc::TransactionView {
            hash: "0x".to_string() + &"bb".repeat(32),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![crate::rpc::CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: crate::rpc::Script {
                    code_hash: "0x".to_string() + &"11".repeat(32),
                    hash_type: "type".to_string(),
                    args: "0x".to_string(),
                },
                type_: None,
            }],
            outputs_data: vec![],
            witnesses: vec![],
        };
        let _ = DaoParser::parse_withdraw_requests(&tx, &[0xCC; 32], &[]);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_compensation_non_negative(
            // Canonical function uses i64, so cap at i64::MAX
            capacity in 102_00000000i64..10_000_000_000_000i64,
            ar_deposit in 1u64..u64::MAX / 2,
            ar_growth in 0u64..1_000_000_000_000u64,
        ) {
            use ckbadger_common::dao::calculate_dao_compensation_from_ar;
            let ar_withdraw = ar_deposit.saturating_add(ar_growth);

            let compensation = calculate_dao_compensation_from_ar(
                capacity,
                ar_deposit,
                ar_withdraw
            ).unwrap();

            prop_assert!(compensation <= capacity);
        }

        #[test]
        fn prop_compensation_bounded_by_capacity(
            capacity in 102_00000000i64..10_000_000_000_000i64,
            ar_deposit in 1_000_000_000_000_000u64..10_000_000_000_000_000u64,
            ar_multiplier in 1u64..10u64,
        ) {
            use ckbadger_common::dao::calculate_dao_compensation_from_ar;
            let ar_withdraw = ar_deposit.saturating_mul(ar_multiplier);

            let compensation = calculate_dao_compensation_from_ar(
                capacity,
                ar_deposit,
                ar_withdraw
            ).unwrap();

            let max_possible = (capacity as i128).saturating_mul(ar_multiplier as i128);
            prop_assert!(
                (compensation as i128) <= max_possible,
                "Compensation {} should not exceed {} (capacity * ar_multiplier)",
                compensation,
                max_possible
            );
        }

        #[test]
        fn prop_dao_state_roundtrip(block_number in 1u64..u64::MAX) {
            let data = block_number.to_le_bytes();
            let state = DaoParser::parse_dao_state(&data);
            prop_assert_eq!(state, Some(DaoState::WithdrawRequest));

            let parsed_block = DaoParser::parse_deposit_block_number(&data);
            prop_assert_eq!(parsed_block, Some(block_number));
        }
    }
}
