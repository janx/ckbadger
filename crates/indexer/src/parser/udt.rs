use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::script::ScriptParser;

pub const SUDT_CODE_HASH: &str =
    "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";

pub const XUDT_CODE_HASH_DATA1: &str =
    "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95";

pub const XUDT_CODE_HASH_TYPE: &str =
    "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb";

static SUDT_CODE_HASH_BYTES: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(SUDT_CODE_HASH));
static XUDT_CODE_HASH_DATA1_BYTES: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(XUDT_CODE_HASH_DATA1));
static XUDT_CODE_HASH_TYPE_BYTES: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(XUDT_CODE_HASH_TYPE));

#[derive(Debug, Clone)]
pub enum UdtStandard {
    Sudt,
    Xudt,
}

impl UdtStandard {
    pub fn as_str(&self) -> &'static str {
        match self {
            UdtStandard::Sudt => "sudt",
            UdtStandard::Xudt => "xudt",
        }
    }

    pub fn from_standard_hint(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sudt" => Some(UdtStandard::Sudt),
            "xudt" | "xudt_compatible" => Some(UdtStandard::Xudt),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Self {
        match Self::from_standard_hint(s) {
            Some(standard) => standard,
            None => panic!("unknown UDT standard '{}'", s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedUdtCell {
    pub type_script_hash: Vec<u8>,
    pub type_code_hash: Vec<u8>,
    pub type_hash_type: i16,
    pub type_args: Vec<u8>,
    pub lock_script_hash: Vec<u8>,
    pub amount: u128,
    pub standard: UdtStandard,
}

#[derive(Debug, Clone)]
pub struct ParsedUdtTransfer {
    pub type_script_hash: Vec<u8>,
    pub type_code_hash: Vec<u8>,
    pub type_hash_type: i16,
    pub type_args: Vec<u8>,
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Vec<u8>,
    pub amount: u128,
    pub standard: UdtStandard,
    pub is_mint: bool,
    pub is_burn: bool,
}

pub struct UdtParser;

impl UdtParser {
    pub fn is_udt_type_script(code_hash_hex: &str, hash_type: &str) -> Option<UdtStandard> {
        let code_hash = code_hash_hex.to_lowercase();

        if code_hash == SUDT_CODE_HASH && hash_type == "type" {
            return Some(UdtStandard::Sudt);
        }

        if (code_hash == XUDT_CODE_HASH_DATA1 && hash_type == "data1")
            || (code_hash == XUDT_CODE_HASH_TYPE && hash_type == "type")
        {
            return Some(UdtStandard::Xudt);
        }

        None
    }

    pub fn is_udt_code_hash_bytes(code_hash: &[u8], hash_type: i16) -> Option<UdtStandard> {
        if code_hash == SUDT_CODE_HASH_BYTES.as_slice() && hash_type == 1 {
            return Some(UdtStandard::Sudt);
        }

        if (code_hash == XUDT_CODE_HASH_DATA1_BYTES.as_slice() && hash_type == 2)
            || (code_hash == XUDT_CODE_HASH_TYPE_BYTES.as_slice() && hash_type == 1)
        {
            return Some(UdtStandard::Xudt);
        }

        None
    }

    pub fn parse_amount(data: &[u8]) -> Option<u128> {
        if data.len() < 16 {
            return None;
        }
        let bytes: [u8; 16] = data[..16].try_into().ok()?;
        Some(u128::from_le_bytes(bytes))
    }

    pub fn parse_udt_cells(tx: &TransactionView) -> Vec<ParsedUdtCell> {
        super::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| {
                Self::parse_udt_cell_with_standard_hint(output, data_hex, None)
            })
            .collect()
    }

    pub fn parse_udt_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedUdtCell> {
        Self::parse_udt_cell_with_standard_hint(output, data_hex, None)
    }

    pub fn parse_udt_cell_with_standard_hint(
        output: &CellOutput,
        data_hex: &str,
        standard_hint: Option<&str>,
    ) -> Option<ParsedUdtCell> {
        let type_script = output.type_.as_ref()?;
        let standard = if let Some(standard) =
            Self::is_udt_type_script(&type_script.code_hash, &type_script.hash_type)
        {
            standard
        } else if matches!(standard_hint, Some("xudt_compatible")) {
            UdtStandard::Xudt
        } else {
            return None;
        };

        let data = parse_hex_to_bytes(data_hex);
        let amount = Self::parse_amount(&data)?;

        let type_script_hash = ScriptParser::compute_script_hash(type_script);
        let lock_script_hash = ScriptParser::compute_script_hash(&output.lock);

        Some(ParsedUdtCell {
            type_script_hash,
            type_code_hash: parse_hex_to_bytes(&type_script.code_hash),
            type_hash_type: ScriptParser::hash_type_to_i16(&type_script.hash_type),
            type_args: parse_hex_to_bytes(&type_script.args),
            lock_script_hash,
            amount,
            standard,
        })
    }

    pub fn build_transfers_from_cells(
        input_udts: &[ParsedUdtCell],
        output_udts: &[ParsedUdtCell],
    ) -> Vec<ParsedUdtTransfer> {
        #[derive(Default)]
        struct TokenFlow {
            type_code_hash: Vec<u8>,
            type_hash_type: i16,
            type_args: Vec<u8>,
            standard: Option<UdtStandard>,
            inputs_by_lock: BTreeMap<Vec<u8>, u128>,
            outputs_by_lock: BTreeMap<Vec<u8>, u128>,
        }

        impl TokenFlow {
            fn set_meta_from(&mut self, cell: &ParsedUdtCell) {
                if self.standard.is_none() {
                    self.type_code_hash = cell.type_code_hash.clone();
                    self.type_hash_type = cell.type_hash_type;
                    self.type_args = cell.type_args.clone();
                    self.standard = Some(cell.standard.clone());
                }
            }
        }

        let mut flows: BTreeMap<Vec<u8>, TokenFlow> = BTreeMap::new();

        for input in input_udts {
            let flow = flows.entry(input.type_script_hash.clone()).or_default();
            flow.set_meta_from(input);
            *flow
                .inputs_by_lock
                .entry(input.lock_script_hash.clone())
                .or_insert(0) += input.amount;
        }

        for output in output_udts {
            let flow = flows.entry(output.type_script_hash.clone()).or_default();
            flow.set_meta_from(output);
            *flow
                .outputs_by_lock
                .entry(output.lock_script_hash.clone())
                .or_insert(0) += output.amount;
        }

        let mut transfers = Vec::new();

        for (type_script_hash, flow) in flows {
            let Some(standard) = flow.standard.clone() else {
                continue;
            };

            let mut senders: Vec<(Vec<u8>, u128)> = flow
                .inputs_by_lock
                .iter()
                .filter_map(|(lock, in_amt)| {
                    let out_amt = flow.outputs_by_lock.get(lock).copied().unwrap_or(0);
                    (in_amt > &out_amt).then_some((lock.clone(), in_amt - out_amt))
                })
                .collect();

            let mut receivers: Vec<(Vec<u8>, u128)> = flow
                .outputs_by_lock
                .iter()
                .filter_map(|(lock, out_amt)| {
                    let in_amt = flow.inputs_by_lock.get(lock).copied().unwrap_or(0);
                    (out_amt > &in_amt).then_some((lock.clone(), out_amt - in_amt))
                })
                .collect();

            let mut sender_idx = 0usize;
            let mut receiver_idx = 0usize;
            while sender_idx < senders.len() && receiver_idx < receivers.len() {
                let amount = senders[sender_idx].1.min(receivers[receiver_idx].1);
                if amount > 0 {
                    transfers.push(ParsedUdtTransfer {
                        type_script_hash: type_script_hash.clone(),
                        type_code_hash: flow.type_code_hash.clone(),
                        type_hash_type: flow.type_hash_type,
                        type_args: flow.type_args.clone(),
                        from_lock_hash: Some(senders[sender_idx].0.clone()),
                        to_lock_hash: receivers[receiver_idx].0.clone(),
                        amount,
                        standard: standard.clone(),
                        is_mint: false,
                        is_burn: false,
                    });
                }

                senders[sender_idx].1 -= amount;
                receivers[receiver_idx].1 -= amount;

                if senders[sender_idx].1 == 0 {
                    sender_idx += 1;
                }
                if receivers[receiver_idx].1 == 0 {
                    receiver_idx += 1;
                }
            }

            for (lock, amount) in receivers.into_iter().skip(receiver_idx) {
                if amount == 0 {
                    continue;
                }
                transfers.push(ParsedUdtTransfer {
                    type_script_hash: type_script_hash.clone(),
                    type_code_hash: flow.type_code_hash.clone(),
                    type_hash_type: flow.type_hash_type,
                    type_args: flow.type_args.clone(),
                    from_lock_hash: None,
                    to_lock_hash: lock,
                    amount,
                    standard: standard.clone(),
                    is_mint: true,
                    is_burn: false,
                });
            }

            for (lock, amount) in senders.into_iter().skip(sender_idx) {
                if amount == 0 {
                    continue;
                }
                transfers.push(ParsedUdtTransfer {
                    type_script_hash: type_script_hash.clone(),
                    type_code_hash: flow.type_code_hash.clone(),
                    type_hash_type: flow.type_hash_type,
                    type_args: flow.type_args.clone(),
                    from_lock_hash: Some(lock),
                    to_lock_hash: Vec::new(),
                    amount,
                    standard: standard.clone(),
                    is_mint: false,
                    is_burn: true,
                });
            }
        }

        transfers
    }

    pub fn parse_transfers(
        tx: &TransactionView,
        input_cells: &[(CellOutput, String)],
    ) -> Vec<ParsedUdtTransfer> {
        let output_udts = Self::parse_udt_cells(tx);

        let input_udts: Vec<ParsedUdtCell> = input_cells
            .iter()
            .filter_map(|(output, data_hex)| Self::parse_udt_cell(output, data_hex))
            .collect();

        Self::build_transfers_from_cells(&input_udts, &output_udts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::test_helpers::create_lock_script;
    use crate::rpc::{CellOutput, Script};

    fn create_sudt_type_script() -> Script {
        Script {
            code_hash: SUDT_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_xudt_type_script() -> Script {
        Script {
            code_hash: XUDT_CODE_HASH_TYPE.to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    #[test]
    fn test_is_udt_type_script_sudt() {
        let result = UdtParser::is_udt_type_script(SUDT_CODE_HASH, "type");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), UdtStandard::Sudt));
    }

    #[test]
    fn test_is_udt_type_script_xudt_type() {
        let result = UdtParser::is_udt_type_script(XUDT_CODE_HASH_TYPE, "type");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), UdtStandard::Xudt));
    }

    #[test]
    fn test_is_udt_type_script_xudt_data1() {
        let result = UdtParser::is_udt_type_script(XUDT_CODE_HASH_DATA1, "data1");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), UdtStandard::Xudt));
    }

    #[test]
    fn test_is_udt_type_script_wrong_hash_type() {
        let result = UdtParser::is_udt_type_script(SUDT_CODE_HASH, "data");
        assert!(result.is_none());

        let result = UdtParser::is_udt_type_script(XUDT_CODE_HASH_DATA1, "type");
        assert!(result.is_none());
    }

    #[test]
    fn test_is_udt_type_script_unknown() {
        let result = UdtParser::is_udt_type_script(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            "type",
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_is_udt_type_script_case_insensitive() {
        let upper = SUDT_CODE_HASH.to_uppercase();
        let result = UdtParser::is_udt_type_script(&upper, "type");
        assert!(result.is_some());
    }

    #[test]
    fn test_is_udt_code_hash_bytes_sudt() {
        let code_hash = parse_hex_to_bytes(SUDT_CODE_HASH);
        let result = UdtParser::is_udt_code_hash_bytes(&code_hash, 1);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), UdtStandard::Sudt));
    }

    #[test]
    fn test_is_udt_code_hash_bytes_xudt() {
        let code_hash = parse_hex_to_bytes(XUDT_CODE_HASH_TYPE);
        let result = UdtParser::is_udt_code_hash_bytes(&code_hash, 1);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), UdtStandard::Xudt));

        let code_hash = parse_hex_to_bytes(XUDT_CODE_HASH_DATA1);
        let result = UdtParser::is_udt_code_hash_bytes(&code_hash, 2);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_amount_valid() {
        let data = 1000u128.to_le_bytes();
        let result = UdtParser::parse_amount(&data);
        assert_eq!(result, Some(1000));
    }

    #[test]
    fn test_parse_amount_large_value() {
        let max = u128::MAX;
        let data = max.to_le_bytes();
        let result = UdtParser::parse_amount(&data);
        assert_eq!(result, Some(u128::MAX));
    }

    #[test]
    fn test_parse_amount_too_short() {
        let data = [0u8; 8];
        let result = UdtParser::parse_amount(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_amount_with_extra_data() {
        let mut data = vec![0u8; 32];
        data[..16].copy_from_slice(&500u128.to_le_bytes());
        let result = UdtParser::parse_amount(&data);
        assert_eq!(result, Some(500));
    }

    #[test]
    fn test_parse_udt_cell_sudt() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_sudt_type_script()),
        };
        let amount = 1_000_000u128;
        let data = amount.to_le_bytes().to_vec();
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = UdtParser::parse_udt_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.amount, amount);
        assert!(matches!(parsed.standard, UdtStandard::Sudt));
        assert_eq!(parsed.type_script_hash.len(), 32);
        assert_eq!(parsed.lock_script_hash.len(), 32);
    }

    #[test]
    fn test_parse_udt_cell_xudt() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_xudt_type_script()),
        };
        let amount = 5_000_000u128;
        let data = amount.to_le_bytes().to_vec();
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = UdtParser::parse_udt_cell(&output, &data_hex);
        assert!(result.is_some());
        assert!(matches!(result.unwrap().standard, UdtStandard::Xudt));
    }

    #[test]
    fn test_parse_udt_cell_no_type_script() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };

        let result = UdtParser::parse_udt_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_udt_cell_non_udt_type() {
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

        let result = UdtParser::parse_udt_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_udt_cell_invalid_data() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_sudt_type_script()),
        };

        let result = UdtParser::parse_udt_cell(&output, "0x1234");
        assert!(result.is_none());
    }

    #[test]
    fn test_udt_standard_as_str() {
        assert_eq!(UdtStandard::Sudt.as_str(), "sudt");
        assert_eq!(UdtStandard::Xudt.as_str(), "xudt");
    }

    #[test]
    fn test_udt_standard_parse() {
        assert!(matches!(UdtStandard::parse("sudt"), UdtStandard::Sudt));
        assert!(matches!(UdtStandard::parse("xudt"), UdtStandard::Xudt));
        assert!(matches!(
            UdtStandard::parse("xudt_compatible"),
            UdtStandard::Xudt
        ));
    }

    #[test]
    #[should_panic(expected = "unknown UDT standard")]
    fn test_udt_standard_parse_unknown_panics() {
        let _ = UdtStandard::parse("unknown");
    }

    #[test]
    #[should_panic(expected = "unknown UDT standard")]
    fn test_udt_standard_parse_empty_panics() {
        let _ = UdtStandard::parse("");
    }

    #[test]
    fn test_udt_standard_from_standard_hint() {
        assert!(matches!(
            UdtStandard::from_standard_hint("sudt"),
            Some(UdtStandard::Sudt)
        ));
        assert!(matches!(
            UdtStandard::from_standard_hint("xudt"),
            Some(UdtStandard::Xudt)
        ));
        assert!(matches!(
            UdtStandard::from_standard_hint("xudt_compatible"),
            Some(UdtStandard::Xudt)
        ));
        assert!(UdtStandard::from_standard_hint("omiga_inscription").is_none());
    }

    #[test]
    fn test_parse_udt_cell_with_xudt_compatible_hint() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: "0xbfa35a9c38a676682b65ade8f02be164d48632281477e36f8dc2f41f79e56bfc"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0xd591ebdc69626647e056e13345fd830c8b876bb06aa07ba610479eb77153ea9f"
                    .to_string(),
            }),
        };

        // Unknown code hash should not be parsed as UDT without a standard hint.
        assert!(UdtParser::parse_udt_cell(&output, "0x01000000000000000000000000000000").is_none());

        let parsed = UdtParser::parse_udt_cell_with_standard_hint(
            &output,
            "0x01000000000000000000000000000000",
            Some("xudt_compatible"),
        )
        .unwrap();

        assert_eq!(parsed.amount, 1);
        assert!(matches!(parsed.standard, UdtStandard::Xudt));
    }

    #[test]
    fn test_parse_udt_cell_with_sudt_hint_does_not_parse_unknown_code_hash() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: "0xbfa35a9c38a676682b65ade8f02be164d48632281477e36f8dc2f41f79e56bfc"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0xd591ebdc69626647e056e13345fd830c8b876bb06aa07ba610479eb77153ea9f"
                    .to_string(),
            }),
        };

        let parsed = UdtParser::parse_udt_cell_with_standard_hint(
            &output,
            "0x01000000000000000000000000000000",
            Some("sudt"),
        );

        assert!(parsed.is_none());
    }

    #[test]
    #[should_panic(expected = "outputs/outputs_data length mismatch")]
    fn test_parse_udt_cells_panics_on_outputs_data_length_mismatch() {
        use crate::rpc::TransactionView;

        let tx = TransactionView {
            hash: "0x9999".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: Some(create_sudt_type_script()),
            }],
            outputs_data: vec![],
            witnesses: vec![],
        };

        let _ = UdtParser::parse_udt_cells(&tx);
    }

    #[test]
    fn test_parse_transfers_burn_no_output() {
        use crate::rpc::TransactionView;

        let tx = TransactionView {
            hash: "0x1234".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec![],
        };

        let input_amount = 1_000_000u128;
        let input_data = input_amount.to_le_bytes().to_vec();
        let input_data_hex = format!("0x{}", hex::encode(&input_data));

        let input_cells = vec![(
            CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: Some(create_sudt_type_script()),
            },
            input_data_hex,
        )];

        let transfers = UdtParser::parse_transfers(&tx, &input_cells);

        assert_eq!(transfers.len(), 1);
        let burn = &transfers[0];
        assert!(burn.is_burn);
        assert!(!burn.is_mint);
        assert_eq!(burn.amount, input_amount);
        assert!(burn.from_lock_hash.is_some());
        assert!(burn.to_lock_hash.is_empty());
    }

    #[test]
    fn test_parse_transfers_to_different_address() {
        use crate::rpc::TransactionView;

        let sender_lock = create_lock_script();
        let receiver_lock = Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        };

        let amount = 5_000_000u128;
        let data = amount.to_le_bytes().to_vec();
        let data_hex = format!("0x{}", hex::encode(&data));

        let tx = TransactionView {
            hash: "0x5678".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: receiver_lock,
                type_: Some(create_sudt_type_script()),
            }],
            outputs_data: vec![data_hex.clone()],
            witnesses: vec![],
        };

        let input_cells = vec![(
            CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: sender_lock,
                type_: Some(create_sudt_type_script()),
            },
            data_hex,
        )];

        let transfers = UdtParser::parse_transfers(&tx, &input_cells);

        assert_eq!(transfers.len(), 1);
        let transfer = &transfers[0];
        assert!(!transfer.is_burn);
        assert!(!transfer.is_mint);
        assert_eq!(transfer.amount, amount);
        assert!(transfer.from_lock_hash.is_some());
        assert!(!transfer.to_lock_hash.is_empty());
    }

    #[test]
    fn test_build_transfers_from_cells_multi_input_keeps_exact_deltas() {
        let type_script_hash = vec![0x11; 32];
        let type_code_hash = vec![0x22; 32];
        let type_args = vec![0x33; 20];
        let lock_a = vec![0xA1; 32];
        let lock_b = vec![0xB2; 32];
        let lock_c = vec![0xC3; 32];

        let mk_cell = |lock: &[u8], amount: u128| ParsedUdtCell {
            type_script_hash: type_script_hash.clone(),
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
            lock_script_hash: lock.to_vec(),
            amount,
            standard: UdtStandard::Sudt,
        };

        // in: A=100, B=50; out: C=120 => A=-100, B=-50, C=+120, burn=30
        let inputs = vec![mk_cell(&lock_a, 100), mk_cell(&lock_b, 50)];
        let outputs = vec![mk_cell(&lock_c, 120)];

        let transfers = UdtParser::build_transfers_from_cells(&inputs, &outputs);

        let mut balance_delta: BTreeMap<Vec<u8>, i128> = BTreeMap::new();
        let mut supply_delta: i128 = 0;
        for transfer in transfers {
            if let Some(from) = transfer.from_lock_hash {
                *balance_delta.entry(from).or_default() -= transfer.amount as i128;
            }
            if !transfer.to_lock_hash.is_empty() {
                *balance_delta.entry(transfer.to_lock_hash).or_default() += transfer.amount as i128;
            }
            if transfer.is_mint {
                supply_delta += transfer.amount as i128;
            }
            if transfer.is_burn {
                supply_delta -= transfer.amount as i128;
            }
        }

        assert_eq!(balance_delta.get(&lock_a), Some(&-100));
        assert_eq!(balance_delta.get(&lock_b), Some(&-50));
        assert_eq!(balance_delta.get(&lock_c), Some(&120));
        assert_eq!(supply_delta, -30);
    }
}
