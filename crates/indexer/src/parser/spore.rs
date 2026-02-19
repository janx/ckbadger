use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::bytes_to_pg_string;
use super::script::ScriptParser;

// Mainnet Spore v2 (latest)
pub const SPORE_CODE_HASH_MAINNET_V2: &str =
    "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5";
// Mainnet Spore v2 DID (type script based)
pub const SPORE_CODE_HASH_MAINNET_DID: &str =
    "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33";
// Testnet Spore v2 preview
pub const SPORE_CODE_HASH_TESTNET_V2: &str =
    "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d";
// Testnet Spore v1 latest
pub const SPORE_CODE_HASH_TESTNET_V1: &str =
    "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494";

// Mainnet Cluster v2 (latest)
pub const CLUSTER_CODE_HASH_MAINNET_V2: &str =
    "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075";
// Testnet Cluster v2 preview
pub const CLUSTER_CODE_HASH_TESTNET_V2: &str =
    "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058";
// Testnet Cluster v1 latest
pub const CLUSTER_CODE_HASH_TESTNET_V1: &str =
    "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080";

// Legacy aliases for backward compatibility
pub const SPORE_CODE_HASH: &str = SPORE_CODE_HASH_MAINNET_V2;
pub const CLUSTER_CODE_HASH: &str = CLUSTER_CODE_HASH_MAINNET_V2;

#[derive(Debug, Clone)]
pub struct ParsedSporeCell {
    pub spore_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub content_type: String,
    pub content: Vec<u8>,
    pub cluster_id: Option<Vec<u8>>,
    pub owner_lock_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedClusterCell {
    pub cluster_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub owner_lock_hash: Vec<u8>,
}

pub struct SporeParser;

impl SporeParser {
    pub fn is_spore_type_script(code_hash: &[u8]) -> bool {
        let spore_hashes = [
            parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2),
            parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_DID),
            parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V2),
            parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V1),
        ];
        spore_hashes.iter().any(|h| code_hash == h.as_slice())
    }

    pub fn is_cluster_type_script(code_hash: &[u8]) -> bool {
        let cluster_hashes = [
            parse_hex_to_bytes(CLUSTER_CODE_HASH_MAINNET_V2),
            parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V2),
            parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V1),
        ];
        cluster_hashes.iter().any(|h| code_hash == h.as_slice())
    }

    pub fn parse_spore_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedSporeCell> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_spore_type_script(&type_code_hash) {
            return None;
        }

        let data = parse_hex_to_bytes(data_hex);
        let spore_data = Self::parse_spore_data(&data)?;

        let spore_id = parse_hex_to_bytes(&type_script.args);
        let type_script_hash = ScriptParser::compute_script_hash(type_script);
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock);

        Some(ParsedSporeCell {
            spore_id,
            type_script_hash,
            content_type: spore_data.content_type,
            content: spore_data.content,
            cluster_id: spore_data.cluster_id,
            owner_lock_hash,
        })
    }

    pub fn parse_cluster_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedClusterCell> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_cluster_type_script(&type_code_hash) {
            return None;
        }

        let data = parse_hex_to_bytes(data_hex);
        let cluster_data = Self::parse_cluster_data(&data)?;

        let cluster_id = parse_hex_to_bytes(&type_script.args);
        let type_script_hash = ScriptParser::compute_script_hash(type_script);
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock);

        Some(ParsedClusterCell {
            cluster_id,
            type_script_hash,
            name: cluster_data.name,
            description: cluster_data.description,
            owner_lock_hash,
        })
    }

    pub fn parse_spores(tx: &TransactionView) -> Vec<ParsedSporeCell> {
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| Self::parse_spore_cell(output, data_hex))
            .collect()
    }

    pub fn parse_clusters(tx: &TransactionView) -> Vec<ParsedClusterCell> {
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| Self::parse_cluster_cell(output, data_hex))
            .collect()
    }

    pub fn parse_spore_cluster_id_from_data(data: &[u8]) -> Option<Vec<u8>> {
        Self::parse_spore_data(data).and_then(|parsed| parsed.cluster_id)
    }

    fn parse_spore_data(data: &[u8]) -> Option<SporeData> {
        if data.len() < 16 {
            return None;
        }

        let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        if data.len() < total_size || total_size < 16 {
            return None;
        }

        let offset_content_type = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
        let offset_content = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
        let offset_cluster_id = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;

        let content_type = Self::read_bytes_field(data, offset_content_type, offset_content)?;
        let content_type_str = bytes_to_pg_string(&content_type);

        let content = Self::read_bytes_field(data, offset_content, offset_cluster_id)?;

        let cluster_id = if offset_cluster_id < total_size {
            Self::read_bytes_opt_field(data, offset_cluster_id, total_size)
        } else {
            None
        };

        Some(SporeData {
            content_type: content_type_str,
            content,
            cluster_id,
        })
    }

    fn parse_cluster_data(data: &[u8]) -> Option<ClusterData> {
        if data.len() < 12 {
            return None;
        }

        let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        if data.len() < total_size || total_size < 12 {
            return None;
        }

        let offset_name = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
        let offset_description = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;

        let end_of_description = if data.len() >= 16 {
            data[12..16]
                .try_into()
                .ok()
                .map(|bytes: [u8; 4]| u32::from_le_bytes(bytes) as usize)
                .unwrap_or(total_size)
        } else {
            total_size
        };

        let name = Self::read_bytes_field(data, offset_name, offset_description)
            .map(|b| bytes_to_pg_string(&b));

        let description = Self::read_bytes_field(data, offset_description, end_of_description)
            .map(|b| bytes_to_pg_string(&b));

        Some(ClusterData { name, description })
    }

    fn read_bytes_field(data: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
        if start >= end || start + 4 > data.len() {
            return None;
        }

        let content_len = u32::from_le_bytes(data[start..start + 4].try_into().ok()?) as usize;
        let content_start = start + 4;

        if content_start + content_len > data.len() {
            return None;
        }

        Some(data[content_start..content_start + content_len].to_vec())
    }

    fn read_bytes_opt_field(data: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
        if start >= end || start >= data.len() {
            return None;
        }

        if start + 4 > data.len() {
            return None;
        }

        let opt_header = u32::from_le_bytes(data[start..start + 4].try_into().ok()?) as usize;

        if opt_header == 0 {
            return None;
        }

        Self::read_bytes_field(data, start, end)
    }
}

struct SporeData {
    content_type: String,
    content: Vec<u8>,
    cluster_id: Option<Vec<u8>>,
}

struct ClusterData {
    name: Option<String>,
    description: Option<String>,
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

    fn create_spore_type_script(spore_id: &str) -> Script {
        Script {
            code_hash: SPORE_CODE_HASH.to_string(),
            hash_type: "data1".to_string(),
            args: spore_id.to_string(),
        }
    }

    fn create_cluster_type_script(cluster_id: &str) -> Script {
        Script {
            code_hash: CLUSTER_CODE_HASH.to_string(),
            hash_type: "data1".to_string(),
            args: cluster_id.to_string(),
        }
    }

    fn encode_molecule_bytes(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u32;
        let mut result = len.to_le_bytes().to_vec();
        result.extend_from_slice(data);
        result
    }

    fn create_spore_data(content_type: &str, content: &[u8], cluster_id: Option<&[u8]>) -> Vec<u8> {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content);
        let cluster_id_bytes = cluster_id.map(encode_molecule_bytes);

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster_id = offset_content + content_bytes.len() as u32;
        let total_size =
            offset_cluster_id + cluster_id_bytes.as_ref().map(|b| b.len()).unwrap_or(0) as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_content_type.to_le_bytes());
        data.extend_from_slice(&offset_content.to_le_bytes());
        data.extend_from_slice(&offset_cluster_id.to_le_bytes());
        data.extend_from_slice(&content_type_bytes);
        data.extend_from_slice(&content_bytes);
        if let Some(cid) = cluster_id_bytes {
            data.extend_from_slice(&cid);
        }
        data
    }

    fn create_cluster_data(name: &str, description: &str) -> Vec<u8> {
        let name_bytes = encode_molecule_bytes(name.as_bytes());
        let desc_bytes = encode_molecule_bytes(description.as_bytes());

        let offset_name = 16u32;
        let offset_desc = offset_name + name_bytes.len() as u32;
        let offset_end = offset_desc + desc_bytes.len() as u32;
        let total_size = offset_end;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_name.to_le_bytes());
        data.extend_from_slice(&offset_desc.to_le_bytes());
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&name_bytes);
        data.extend_from_slice(&desc_bytes);
        data
    }

    #[test]
    fn test_is_spore_type_script() {
        let spore_hash = parse_hex_to_bytes(SPORE_CODE_HASH);
        assert!(SporeParser::is_spore_type_script(&spore_hash));

        let other_hash = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!SporeParser::is_spore_type_script(&other_hash));
    }

    #[test]
    fn test_is_cluster_type_script() {
        let cluster_hash = parse_hex_to_bytes(CLUSTER_CODE_HASH);
        assert!(SporeParser::is_cluster_type_script(&cluster_hash));

        let other_hash = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!SporeParser::is_cluster_type_script(&other_hash));
    }

    #[test]
    fn test_parse_spore_cell_basic() {
        let spore_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_spore_type_script(spore_id)),
        };

        let data = create_spore_data("image/png", b"fake png data", None);
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = SporeParser::parse_spore_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.content_type, "image/png");
        assert_eq!(parsed.content, b"fake png data");
        assert!(parsed.cluster_id.is_none());
        assert_eq!(parsed.spore_id.len(), 32);
        assert_eq!(parsed.owner_lock_hash.len(), 32);
    }

    #[test]
    fn test_parse_spore_cell_with_cluster() {
        let spore_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let cluster_id = [0xab; 32];
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_spore_type_script(spore_id)),
        };

        let data = create_spore_data("text/plain", b"hello world", Some(&cluster_id));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = SporeParser::parse_spore_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert!(parsed.cluster_id.is_some());
        assert_eq!(parsed.cluster_id.as_ref().unwrap().len(), 32);
    }

    #[test]
    fn test_parse_spore_cluster_id_from_data() {
        let cluster_id = [0xabu8; 32];
        let data = create_spore_data("text/plain", b"hello world", Some(&cluster_id));

        let parsed = SporeParser::parse_spore_cluster_id_from_data(&data).unwrap();
        assert_eq!(parsed, cluster_id.to_vec());
    }

    #[test]
    fn test_parse_spore_cell_non_spore_type() {
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

        let result = SporeParser::parse_spore_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_spore_cell_no_type_script() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };

        let result = SporeParser::parse_spore_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_cluster_cell_basic() {
        let cluster_id = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_cluster_type_script(cluster_id)),
        };

        let data = create_cluster_data("My Collection", "A collection of art");
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = SporeParser::parse_cluster_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, Some("My Collection".to_string()));
        assert_eq!(parsed.description, Some("A collection of art".to_string()));
        assert_eq!(parsed.cluster_id.len(), 32);
        assert_eq!(parsed.owner_lock_hash.len(), 32);
    }

    #[test]
    fn test_parse_cluster_cell_non_cluster_type() {
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

        let result = SporeParser::parse_cluster_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_spore_data_too_short() {
        let data = [0u8; 8];
        let result = SporeParser::parse_spore_data(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_cluster_data_too_short() {
        let data = [0u8; 8];
        let result = SporeParser::parse_cluster_data(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_bytes_field_invalid_range() {
        let data = [0u8; 16];
        let result = SporeParser::read_bytes_field(&data, 20, 30);
        assert!(result.is_none());
    }

    #[test]
    fn test_cluster_id_parsed_as_32_bytes_not_28() {
        let spore_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let cluster_id_32_bytes = [0xab; 32];
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_spore_type_script(spore_id)),
        };

        let data = create_spore_data("image/png", b"test", Some(&cluster_id_32_bytes));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = SporeParser::parse_spore_cell(&output, &data_hex);
        let parsed = result.expect("Should parse successfully");

        let cluster_id = parsed.cluster_id.expect("Should have cluster_id");
        assert_eq!(
            cluster_id.len(),
            32,
            "cluster_id must be 32 bytes, not 28 (molecule Bytes size = content length)"
        );
        assert_eq!(cluster_id, cluster_id_32_bytes.to_vec());
    }

    #[test]
    fn test_molecule_bytes_encoding() {
        let content = [0x12, 0x34, 0x56, 0x78];
        let encoded = encode_molecule_bytes(&content);

        assert_eq!(encoded.len(), 8);
        assert_eq!(&encoded[0..4], &[4, 0, 0, 0]);
        assert_eq!(&encoded[4..8], &content);
    }
}
