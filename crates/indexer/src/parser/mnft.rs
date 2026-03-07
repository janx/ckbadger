use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::bytes_to_pg_string;
use super::cell::ParsedCell;
use super::script::ScriptParser;

pub const MNFT_ISSUER_CODE_HASH: &str =
    "0x24b04faf80ded836efc05247778eec4ec02548dab6e2012c0107374aa3f68b81";

pub const MNFT_CLASS_CODE_HASH: &str =
    "0xd51e6eaf48124c601f41abe173f1da550b4cbca9c6a166781906a287abbb3d9a";

pub const MNFT_TOKEN_CODE_HASH: &str =
    "0x2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9";

#[derive(Debug, Clone)]
pub struct ParsedMnftIssuer {
    pub issuer_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub name: Option<String>,
    pub info: Option<Vec<u8>>,
    pub class_count: u32,
    pub set_count: u32,
    pub owner_lock_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedMnftClass {
    pub class_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub issuer_id: Vec<u8>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub renderer: Option<String>,
    pub total: u32,
    pub issued: u32,
    pub configure: u8,
    pub owner_lock_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedMnftToken {
    pub token_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub class_id: Vec<u8>,
    pub token_index: u32,
    pub characteristic: Vec<u8>,
    pub configure: u8,
    pub state: u8,
    pub owner_lock_hash: Vec<u8>,
}

pub struct MnftParser;

impl MnftParser {
    pub fn is_issuer_type_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(MNFT_ISSUER_CODE_HASH);
        code_hash == hash.as_slice()
    }

    pub fn is_class_type_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(MNFT_CLASS_CODE_HASH);
        code_hash == hash.as_slice()
    }

    pub fn is_token_type_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH);
        code_hash == hash.as_slice()
    }

    pub fn parse_issuer_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedMnftIssuer> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_issuer_type_script(&type_code_hash) {
            return None;
        }

        let data = parse_hex_to_bytes(data_hex);
        let issuer_data = Self::parse_issuer_data(&data)?;

        let type_script_hash = ScriptParser::compute_script_hash(type_script)
            .unwrap_or_else(|e| panic!("mNFT type script hash failed: {}", e));
        let issuer_id = type_script_hash[..20].to_vec();
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock)
            .unwrap_or_else(|e| panic!("mNFT lock script hash failed: {}", e));

        Some(ParsedMnftIssuer {
            issuer_id,
            type_script_hash,
            name: issuer_data.name,
            info: issuer_data.info,
            class_count: issuer_data.class_count,
            set_count: issuer_data.set_count,
            owner_lock_hash,
        })
    }

    pub fn parse_issuer_parsed_cell(cell: &ParsedCell) -> Option<ParsedMnftIssuer> {
        let type_code_hash = cell.type_code_hash.as_ref()?;
        if !Self::is_issuer_type_script(type_code_hash) {
            return None;
        }

        let issuer_data = Self::parse_issuer_data(&cell.data)?;
        let type_script_hash = cell
            .type_script_hash
            .clone()
            .expect("mNFT issuer parsed cell missing type_script_hash");

        Some(ParsedMnftIssuer {
            issuer_id: type_script_hash[..20].to_vec(),
            type_script_hash,
            name: issuer_data.name,
            info: issuer_data.info,
            class_count: issuer_data.class_count,
            set_count: issuer_data.set_count,
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    pub fn parse_class_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedMnftClass> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_class_type_script(&type_code_hash) {
            return None;
        }

        let args = parse_hex_to_bytes(&type_script.args);
        if args.len() < 24 {
            return None;
        }

        let issuer_id = args[..20].to_vec();

        let data = parse_hex_to_bytes(data_hex);
        let class_data = Self::parse_class_data(&data)?;

        let type_script_hash = ScriptParser::compute_script_hash(type_script)
            .unwrap_or_else(|e| panic!("mNFT type script hash failed: {}", e));
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock)
            .unwrap_or_else(|e| panic!("mNFT lock script hash failed: {}", e));

        Some(ParsedMnftClass {
            // mNFT class identity is issuer_id(20B) + class_index(4B).
            // Extra args bytes (if present) are not part of the class id and
            // must be ignored to keep class_id consistent with token.class_id.
            class_id: args[..24].to_vec(),
            type_script_hash,
            issuer_id,
            name: class_data.name,
            description: class_data.description,
            renderer: class_data.renderer,
            total: class_data.total,
            issued: class_data.issued,
            configure: class_data.configure,
            owner_lock_hash,
        })
    }

    pub fn parse_class_parsed_cell(cell: &ParsedCell) -> Option<ParsedMnftClass> {
        let type_code_hash = cell.type_code_hash.as_ref()?;
        if !Self::is_class_type_script(type_code_hash) {
            return None;
        }

        let args = cell
            .type_args
            .as_ref()
            .expect("mNFT class parsed cell missing type_args");
        if args.len() < 24 {
            return None;
        }

        let class_data = Self::parse_class_data(&cell.data)?;

        Some(ParsedMnftClass {
            class_id: args[..24].to_vec(),
            type_script_hash: cell
                .type_script_hash
                .clone()
                .expect("mNFT class parsed cell missing type_script_hash"),
            issuer_id: args[..20].to_vec(),
            name: class_data.name,
            description: class_data.description,
            renderer: class_data.renderer,
            total: class_data.total,
            issued: class_data.issued,
            configure: class_data.configure,
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    pub fn parse_token_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedMnftToken> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_token_type_script(&type_code_hash) {
            return None;
        }

        let args = parse_hex_to_bytes(&type_script.args);
        if args.len() < 28 {
            return None;
        }

        let class_id = args[..24].to_vec();
        let token_index = u32::from_le_bytes(args[24..28].try_into().ok()?);

        let data = parse_hex_to_bytes(data_hex);
        let token_data = Self::parse_token_data(&data)?;

        let type_script_hash = ScriptParser::compute_script_hash(type_script)
            .unwrap_or_else(|e| panic!("mNFT type script hash failed: {}", e));
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock)
            .unwrap_or_else(|e| panic!("mNFT lock script hash failed: {}", e));

        Some(ParsedMnftToken {
            token_id: args,
            type_script_hash,
            class_id,
            token_index,
            characteristic: token_data.characteristic,
            configure: token_data.configure,
            state: token_data.state,
            owner_lock_hash,
        })
    }

    pub fn parse_token_parsed_cell(cell: &ParsedCell) -> Option<ParsedMnftToken> {
        let type_code_hash = cell.type_code_hash.as_ref()?;
        if !Self::is_token_type_script(type_code_hash) {
            return None;
        }

        let args = cell
            .type_args
            .as_ref()
            .expect("mNFT token parsed cell missing type_args");
        if args.len() < 28 {
            return None;
        }

        let token_data = Self::parse_token_data(&cell.data)?;

        Some(ParsedMnftToken {
            token_id: args.clone(),
            type_script_hash: cell
                .type_script_hash
                .clone()
                .expect("mNFT token parsed cell missing type_script_hash"),
            class_id: args[..24].to_vec(),
            token_index: u32::from_le_bytes(args[24..28].try_into().ok()?),
            characteristic: token_data.characteristic,
            configure: token_data.configure,
            state: token_data.state,
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    pub fn parse_issuers(tx: &TransactionView) -> Vec<ParsedMnftIssuer> {
        if tx.outputs.len() != tx.outputs_data.len() {
            panic!(
                "transaction outputs mismatch while parsing mNFT issuers: tx_hash={}, outputs={}, outputs_data={}",
                tx.hash,
                tx.outputs.len(),
                tx.outputs_data.len()
            );
        }
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| Self::parse_issuer_cell(output, data_hex))
            .collect()
    }

    pub fn parse_classes_with_output_indices(
        tx: &TransactionView,
    ) -> Vec<(usize, ParsedMnftClass)> {
        if tx.outputs.len() != tx.outputs_data.len() {
            panic!(
                "transaction outputs mismatch while parsing mNFT classes: tx_hash={}, outputs={}, outputs_data={}",
                tx.hash,
                tx.outputs.len(),
                tx.outputs_data.len()
            );
        }
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .enumerate()
            .filter_map(|(output_index, (output, data_hex))| {
                Self::parse_class_cell(output, data_hex).map(|class| (output_index, class))
            })
            .collect()
    }

    pub fn parse_tokens_with_output_indices(tx: &TransactionView) -> Vec<(usize, ParsedMnftToken)> {
        if tx.outputs.len() != tx.outputs_data.len() {
            panic!(
                "transaction outputs mismatch while parsing mNFT tokens: tx_hash={}, outputs={}, outputs_data={}",
                tx.hash,
                tx.outputs.len(),
                tx.outputs_data.len()
            );
        }
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .enumerate()
            .filter_map(|(output_index, (output, data_hex))| {
                Self::parse_token_cell(output, data_hex).map(|token| (output_index, token))
            })
            .collect()
    }

    fn parse_issuer_data(data: &[u8]) -> Option<IssuerData> {
        if data.len() < 9 {
            return None;
        }

        let _version = data[0];
        let class_count = u32::from_be_bytes(data[1..5].try_into().ok()?);
        let set_count = u32::from_be_bytes(data[5..9].try_into().ok()?);

        let (name, info) = if data.len() > 11 {
            let info_size = u16::from_be_bytes(data[9..11].try_into().ok()?) as usize;
            if data.len() >= 11 + info_size && info_size > 0 {
                let info_bytes = data[11..11 + info_size].to_vec();
                let name = Self::extract_json_field(&info_bytes, "name");
                (name, Some(info_bytes))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Some(IssuerData {
            class_count,
            set_count,
            name,
            info,
        })
    }

    fn parse_class_data(data: &[u8]) -> Option<ClassData> {
        if data.len() < 10 {
            return None;
        }

        let _version = data[0];
        let total = u32::from_be_bytes(data[1..5].try_into().ok()?);
        let issued = u32::from_be_bytes(data[5..9].try_into().ok()?);
        let configure = data[9];

        let mut offset = 10;

        let name = Self::read_vartext(data, &mut offset);
        let description = Self::read_vartext(data, &mut offset);
        let renderer = Self::read_vartext(data, &mut offset);

        Some(ClassData {
            total,
            issued,
            configure,
            name,
            description,
            renderer,
        })
    }

    fn parse_token_data(data: &[u8]) -> Option<TokenData> {
        if data.len() < 11 {
            return None;
        }

        let _version = data[0];
        let characteristic = data[1..9].to_vec();
        let configure = data[9];
        let state = data[10];

        Some(TokenData {
            characteristic,
            configure,
            state,
        })
    }

    fn read_vartext(data: &[u8], offset: &mut usize) -> Option<String> {
        if *offset + 2 > data.len() {
            return None;
        }

        let size = u16::from_be_bytes(data[*offset..*offset + 2].try_into().ok()?) as usize;
        *offset += 2;

        if size == 0 || *offset + size > data.len() {
            return None;
        }

        let text = bytes_to_pg_string(&data[*offset..*offset + size]);
        *offset += size;
        Some(text)
    }

    fn extract_json_field(data: &[u8], field: &str) -> Option<String> {
        let text = bytes_to_pg_string(data);
        let key = format!("\"{}\"", field);
        let start = text.find(&key)?;
        let colon_pos = text[start..].find(':')?;
        let value_start = start + colon_pos + 1;

        let trimmed = text[value_start..].trim_start();
        if let Some(stripped) = trimmed.strip_prefix('"') {
            let quote_end = stripped.find('"')?;
            Some(stripped[..quote_end].to_string())
        } else {
            None
        }
    }
}

struct IssuerData {
    class_count: u32,
    set_count: u32,
    name: Option<String>,
    info: Option<Vec<u8>>,
}

struct ClassData {
    total: u32,
    issued: u32,
    configure: u8,
    name: Option<String>,
    description: Option<String>,
    renderer: Option<String>,
}

struct TokenData {
    characteristic: Vec<u8>,
    configure: u8,
    state: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{CellDep, CellInput, CellOutput, Script, TransactionView};

    fn create_lock_script() -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_issuer_type_script(type_id: &str) -> Script {
        Script {
            code_hash: MNFT_ISSUER_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: type_id.to_string(),
        }
    }

    fn create_class_type_script(issuer_id: &[u8], class_id: u32) -> Script {
        let mut args = issuer_id.to_vec();
        args.extend_from_slice(&class_id.to_le_bytes());
        Script {
            code_hash: MNFT_CLASS_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(&args)),
        }
    }

    fn create_token_type_script(class_id: &[u8], token_index: u32) -> Script {
        let mut args = class_id.to_vec();
        args.extend_from_slice(&token_index.to_le_bytes());
        Script {
            code_hash: MNFT_TOKEN_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(&args)),
        }
    }

    fn create_issuer_data(class_count: u32, set_count: u32, info: Option<&str>) -> Vec<u8> {
        let version = 0u8;
        let mut data = vec![version];
        data.extend_from_slice(&class_count.to_be_bytes());
        data.extend_from_slice(&set_count.to_be_bytes());

        if let Some(info_str) = info {
            let info_bytes = info_str.as_bytes();
            data.extend_from_slice(&(info_bytes.len() as u16).to_be_bytes());
            data.extend_from_slice(info_bytes);
        }
        data
    }

    fn create_class_data(
        total: u32,
        issued: u32,
        configure: u8,
        name: &str,
        description: &str,
    ) -> Vec<u8> {
        let version = 0u8;
        let mut data = vec![version];
        data.extend_from_slice(&total.to_be_bytes());
        data.extend_from_slice(&issued.to_be_bytes());
        data.push(configure);

        data.extend_from_slice(&(name.len() as u16).to_be_bytes());
        data.extend_from_slice(name.as_bytes());

        data.extend_from_slice(&(description.len() as u16).to_be_bytes());
        data.extend_from_slice(description.as_bytes());

        let empty_renderer_size = 0u16;
        data.extend_from_slice(&empty_renderer_size.to_be_bytes());

        data
    }

    fn create_token_data(characteristic: &[u8; 8], configure: u8, state: u8) -> Vec<u8> {
        let version = 0u8;
        let mut data = vec![version];
        data.extend_from_slice(characteristic);
        data.push(configure);
        data.push(state);
        data
    }

    fn create_dummy_tx(outputs: Vec<CellOutput>, outputs_data: Vec<String>) -> TransactionView {
        TransactionView {
            hash: "0x00".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::<String>::new(),
            inputs: Vec::<CellInput>::new(),
            outputs,
            outputs_data,
            witnesses: Vec::<String>::new(),
        }
    }

    #[test]
    fn test_is_issuer_type_script() {
        let hash = parse_hex_to_bytes(MNFT_ISSUER_CODE_HASH);
        assert!(MnftParser::is_issuer_type_script(&hash));

        let other = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!MnftParser::is_issuer_type_script(&other));
    }

    #[test]
    fn test_is_class_type_script() {
        let hash = parse_hex_to_bytes(MNFT_CLASS_CODE_HASH);
        assert!(MnftParser::is_class_type_script(&hash));
    }

    #[test]
    fn test_is_token_type_script() {
        let hash = parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH);
        assert!(MnftParser::is_token_type_script(&hash));
    }

    #[test]
    fn test_parse_issuer_cell() {
        let type_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_issuer_type_script(type_id)),
        };

        let info = r#"{"name":"Test Issuer","website":"https://test.com"}"#;
        let data = create_issuer_data(5, 2, Some(info));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = MnftParser::parse_issuer_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.class_count, 5);
        assert_eq!(parsed.set_count, 2);
        assert_eq!(parsed.name, Some("Test Issuer".to_string()));
        assert_eq!(parsed.issuer_id.len(), 20);
    }

    #[test]
    fn test_parse_class_cell() {
        let issuer_id = [0xab; 20];
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_class_type_script(&issuer_id, 3)),
        };

        let data = create_class_data(100, 50, 0b00000011, "Test Class", "A test NFT collection");
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = MnftParser::parse_class_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.total, 100);
        assert_eq!(parsed.issued, 50);
        assert_eq!(parsed.configure, 0b00000011);
        assert_eq!(parsed.name, Some("Test Class".to_string()));
        assert_eq!(
            parsed.description,
            Some("A test NFT collection".to_string())
        );
        assert_eq!(parsed.issuer_id, issuer_id.to_vec());
    }

    #[test]
    fn test_parse_class_cell_with_extended_args_uses_first_24_bytes() {
        let issuer_id = [0xcd; 20];
        let mut args = issuer_id.to_vec();
        args.extend_from_slice(&7u32.to_le_bytes());
        args.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]); // extension bytes

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: MNFT_CLASS_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: format!("0x{}", hex::encode(&args)),
            }),
        };

        let data = create_class_data(10, 1, 0, "Extended Args Class", "desc");
        let data_hex = format!("0x{}", hex::encode(&data));

        let parsed = MnftParser::parse_class_cell(&output, &data_hex).expect("must parse");
        assert_eq!(parsed.class_id, args[..24].to_vec());
        assert_eq!(parsed.issuer_id, issuer_id.to_vec());
    }

    #[test]
    fn test_parse_token_cell() {
        let issuer_id = [0xab; 20];
        let mut class_id = issuer_id.to_vec();
        class_id.extend_from_slice(&3u32.to_le_bytes());

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_token_type_script(&class_id, 42)),
        };

        let characteristic = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
        let data = create_token_data(&characteristic, 0b00000001, 0b00000000);
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = MnftParser::parse_token_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.token_index, 42);
        assert_eq!(parsed.configure, 0b00000001);
        assert_eq!(parsed.state, 0b00000000);
        assert_eq!(parsed.characteristic, characteristic.to_vec());
        assert_eq!(parsed.class_id, class_id);
    }

    #[test]
    fn test_parse_token_parsed_cell_matches_raw_path() {
        let issuer_id = [0xab; 20];
        let mut class_id = issuer_id.to_vec();
        class_id.extend_from_slice(&3u32.to_le_bytes());

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_token_type_script(&class_id, 42)),
        };

        let characteristic = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
        let data = create_token_data(&characteristic, 0b00000001, 0b00000000);
        let data_hex = format!("0x{}", hex::encode(&data));
        let parsed_cell =
            crate::parser::cell::CellParser::parse_output(&output, &data_hex).expect("parsed cell");

        let raw = MnftParser::parse_token_cell(&output, &data_hex).expect("raw");
        let preparsed = MnftParser::parse_token_parsed_cell(&parsed_cell).expect("preparsed");

        assert_eq!(preparsed.token_id, raw.token_id);
        assert_eq!(preparsed.type_script_hash, raw.type_script_hash);
        assert_eq!(preparsed.class_id, raw.class_id);
        assert_eq!(preparsed.token_index, raw.token_index);
        assert_eq!(preparsed.characteristic, raw.characteristic);
        assert_eq!(preparsed.configure, raw.configure);
        assert_eq!(preparsed.state, raw.state);
        assert_eq!(preparsed.owner_lock_hash, raw.owner_lock_hash);
    }

    #[test]
    fn test_parse_issuer_cell_no_type() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };

        let result = MnftParser::parse_issuer_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_class_cell_invalid_args() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: MNFT_CLASS_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: "0x1234".to_string(),
            }),
        };

        let result = MnftParser::parse_class_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_token_data_too_short() {
        let data = [0u8; 5];
        let result = MnftParser::parse_token_data(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_json_field() {
        let json = r#"{"name":"Alice","age":30}"#;
        let result = MnftParser::extract_json_field(json.as_bytes(), "name");
        assert_eq!(result, Some("Alice".to_string()));

        let result_missing = MnftParser::extract_json_field(json.as_bytes(), "email");
        assert!(result_missing.is_none());
    }

    #[test]
    fn test_parse_issuer_cell_big_endian_info_size() {
        let type_id = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_issuer_type_script(type_id)),
        };

        let info = r#"{"name":"BE Issuer"}"#;
        let mut data = vec![0u8];
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&(info.len() as u16).to_be_bytes());
        data.extend_from_slice(info.as_bytes());
        let data_hex = format!("0x{}", hex::encode(&data));

        let parsed = MnftParser::parse_issuer_cell(&output, &data_hex).expect("must parse");
        assert_eq!(parsed.class_count, 1);
        assert_eq!(parsed.set_count, 2);
        assert_eq!(parsed.name.as_deref(), Some("BE Issuer"));
    }

    #[test]
    fn test_parse_class_cell_big_endian_vartext_layout() {
        let issuer_id = [0x11; 20];
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_class_type_script(&issuer_id, 9)),
        };

        let mut data = vec![0u8];
        data.extend_from_slice(&20u32.to_be_bytes());
        data.extend_from_slice(&7u32.to_be_bytes());
        data.push(0xc0);
        data.extend_from_slice(&8u16.to_be_bytes());
        data.extend_from_slice(b"Class-BE");
        data.extend_from_slice(&4u16.to_be_bytes());
        data.extend_from_slice(b"desc");
        data.extend_from_slice(&0u16.to_be_bytes());
        let data_hex = format!("0x{}", hex::encode(&data));

        let parsed = MnftParser::parse_class_cell(&output, &data_hex).expect("must parse");
        assert_eq!(parsed.total, 20);
        assert_eq!(parsed.issued, 7);
        assert_eq!(parsed.configure, 0xc0);
        assert_eq!(parsed.name.as_deref(), Some("Class-BE"));
        assert_eq!(parsed.description.as_deref(), Some("desc"));
    }

    #[test]
    fn test_parse_tokens_with_output_indices_preserves_real_output_index() {
        let issuer_id = [0xab; 20];
        let mut class_id = issuer_id.to_vec();
        class_id.extend_from_slice(&3u32.to_le_bytes());

        let non_mnft_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };
        let mnft_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_token_type_script(&class_id, 7)),
        };

        let non_mnft_data = "0x".to_string();
        let token_data = create_token_data(&[1, 2, 3, 4, 5, 6, 7, 8], 0, 0);
        let token_data_hex = format!("0x{}", hex::encode(token_data));

        let tx = create_dummy_tx(
            vec![
                non_mnft_output,
                mnft_output,
                CellOutput {
                    capacity: "0x174876e800".to_string(),
                    lock: create_lock_script(),
                    type_: None,
                },
            ],
            vec![non_mnft_data, token_data_hex, "0x".to_string()],
        );

        let parsed = MnftParser::parse_tokens_with_output_indices(&tx);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, 1);
        assert_eq!(parsed[0].1.token_index, 7);
    }

    #[test]
    fn test_parse_classes_with_output_indices_preserves_real_output_index() {
        let issuer_id = [0xcd; 20];
        let non_mnft_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };
        let class_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_class_type_script(&issuer_id, 9)),
        };

        let class_data = create_class_data(10, 1, 0, "Class-Idx", "desc");
        let class_data_hex = format!("0x{}", hex::encode(class_data));

        let tx = create_dummy_tx(
            vec![non_mnft_output.clone(), non_mnft_output, class_output],
            vec!["0x".to_string(), "0x".to_string(), class_data_hex],
        );

        let parsed = MnftParser::parse_classes_with_output_indices(&tx);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, 2);
        assert_eq!(parsed[0].1.total, 10);
    }

    #[test]
    #[should_panic(expected = "transaction outputs mismatch while parsing mNFT issuers")]
    fn test_parse_issuers_panics_on_outputs_data_length_mismatch() {
        let tx = create_dummy_tx(
            vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            vec![],
        );
        let _ = MnftParser::parse_issuers(&tx);
    }
}
