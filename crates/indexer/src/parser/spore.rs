use ckbadger_store::types::SporeMediaProfile;

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::bytes_to_safe_string;
use super::cell::ParsedCell;
use super::script::ScriptParser;

// Mainnet Spore v2 (latest)
pub const SPORE_CODE_HASH_MAINNET_V2: &str =
    "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5";
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

#[derive(Debug, Clone)]
pub struct ParsedSporeCell {
    pub spore_id: Vec<u8>,
    pub type_script_hash: Vec<u8>,
    pub is_did: bool,
    pub content_type: String,
    pub content: Vec<u8>,
    pub cluster_id: Option<Vec<u8>>,
    pub owner_lock_hash: Vec<u8>,
    /// Pre-computed media profile from parser stage (bulk sync).
    /// `None` means the writer should compute it on the fly (live sync).
    pub media_profile: Option<SporeMediaProfile>,
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
    pub fn is_did_type_script(code_hash: &[u8]) -> bool {
        crate::parser::registry::PROTOCOL_REGISTRY
            .is(code_hash, crate::parser::registry::ProtocolScript::SporeDid)
    }

    pub fn is_spore_nft_type_script(code_hash: &[u8]) -> bool {
        crate::parser::registry::PROTOCOL_REGISTRY
            .is(code_hash, crate::parser::registry::ProtocolScript::SporeNft)
    }

    pub fn is_spore_type_script(code_hash: &[u8]) -> bool {
        Self::is_spore_nft_type_script(code_hash) || Self::is_did_type_script(code_hash)
    }

    pub fn is_cluster_type_script(code_hash: &[u8]) -> bool {
        crate::parser::registry::PROTOCOL_REGISTRY
            .is(code_hash, crate::parser::registry::ProtocolScript::Cluster)
    }

    pub fn parse_spore_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedSporeCell> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);
        let is_did = Self::is_did_type_script(&type_code_hash);

        if !is_did && !Self::is_spore_nft_type_script(&type_code_hash) {
            return None;
        }

        let spore_id = parse_hex_to_bytes(&type_script.args);
        let type_script_hash = ScriptParser::compute_script_hash(type_script);
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock);

        if is_did {
            return Some(ParsedSporeCell {
                spore_id,
                type_script_hash,
                is_did: true,
                content_type: String::new(),
                content: Vec::new(),
                cluster_id: None,
                owner_lock_hash,
                media_profile: None,
            });
        }

        let data = parse_hex_to_bytes(data_hex);
        let spore_data = Self::parse_spore_data(&data)?;

        Some(ParsedSporeCell {
            spore_id,
            type_script_hash,
            is_did: false,
            content_type: spore_data.content_type,
            content: spore_data.content,
            cluster_id: spore_data.cluster_id,
            owner_lock_hash,
            media_profile: None,
        })
    }

    pub fn parse_spore_parsed_cell(cell: &ParsedCell) -> Option<ParsedSporeCell> {
        let type_code_hash = cell.type_code_hash.as_ref()?;
        let type_args = cell.type_args.as_ref()?;
        let type_script_hash = cell.type_script_hash.as_ref()?;
        let is_did = Self::is_did_type_script(type_code_hash);

        if !is_did && !Self::is_spore_nft_type_script(type_code_hash) {
            return None;
        }

        if type_args.len() != 32 {
            return None;
        }

        if is_did {
            return Some(ParsedSporeCell {
                spore_id: type_args.clone(),
                type_script_hash: type_script_hash.clone(),
                is_did: true,
                content_type: String::new(),
                content: Vec::new(),
                cluster_id: None,
                owner_lock_hash: cell.lock_script_hash.clone(),
                media_profile: None,
            });
        }

        let spore_data = Self::parse_spore_data(&cell.data)?;
        Some(ParsedSporeCell {
            spore_id: type_args.clone(),
            type_script_hash: type_script_hash.clone(),
            is_did: false,
            content_type: spore_data.content_type,
            content: spore_data.content,
            cluster_id: spore_data.cluster_id,
            owner_lock_hash: cell.lock_script_hash.clone(),
            media_profile: None,
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

    pub fn parse_cluster_parsed_cell(cell: &ParsedCell) -> Option<ParsedClusterCell> {
        let type_code_hash = cell.type_code_hash.as_ref()?;
        let type_args = cell.type_args.as_ref()?;
        let type_script_hash = cell.type_script_hash.as_ref()?;
        if !Self::is_cluster_type_script(type_code_hash) || type_args.len() != 32 {
            return None;
        }

        let cluster_data = Self::parse_cluster_data(&cell.data)?;
        Some(ParsedClusterCell {
            cluster_id: type_args.clone(),
            type_script_hash: type_script_hash.clone(),
            name: cluster_data.name,
            description: cluster_data.description,
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    pub fn parse_spores(tx: &TransactionView) -> Vec<ParsedSporeCell> {
        super::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| Self::parse_spore_cell(output, data_hex))
            .collect()
    }

    pub fn parse_spores_with_output_indices(tx: &TransactionView) -> Vec<(usize, ParsedSporeCell)> {
        super::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .enumerate()
            .filter_map(|(output_index, (output, data_hex))| {
                Self::parse_spore_cell(output, data_hex).map(|spore| (output_index, spore))
            })
            .collect()
    }

    pub fn parse_clusters(tx: &TransactionView) -> Vec<ParsedClusterCell> {
        super::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .filter_map(|(output, data_hex)| Self::parse_cluster_cell(output, data_hex))
            .collect()
    }

    pub fn parse_clusters_with_output_indices(
        tx: &TransactionView,
    ) -> Vec<(usize, ParsedClusterCell)> {
        super::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .enumerate()
            .filter_map(|(output_index, (output, data_hex))| {
                Self::parse_cluster_cell(output, data_hex).map(|cluster| (output_index, cluster))
            })
            .collect()
    }

    pub fn parse_spore_cluster_id_from_data(data: &[u8]) -> Option<Vec<u8>> {
        Self::parse_spore_data(data).and_then(|parsed| parsed.cluster_id)
    }

    /// Extract the raw content bytes from a Spore molecule.
    ///
    /// Used by the DOB decode worker to obtain DNA from a spore cell's
    /// output data without needing the full ParsedSporeCell context.
    pub(crate) fn parse_spore_content_from_data(data: &[u8]) -> Option<Vec<u8>> {
        Self::parse_spore_data(data).map(|parsed| parsed.content)
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
        let content_type_str = bytes_to_safe_string(&content_type);

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
            .map(|b| bytes_to_safe_string(&b));

        let description = Self::read_bytes_field(data, offset_description, end_of_description)
            .map(|b| bytes_to_safe_string(&b));

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
    use crate::parser::bit_cell::BIT_CELL_CODE_HASH_MAINNET;
    use crate::parser::cell::CellParser;
    use crate::parser::test_helpers::create_lock_script;
    use crate::rpc::{CellOutput, Script, TransactionView};

    fn create_spore_type_script(spore_id: &str) -> Script {
        Script {
            code_hash: SPORE_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: spore_id.to_string(),
        }
    }

    fn create_bit_cell_type_script(type_args: &str) -> Script {
        Script {
            code_hash: BIT_CELL_CODE_HASH_MAINNET.to_string(),
            hash_type: "type".to_string(),
            args: type_args.to_string(),
        }
    }

    fn create_cluster_type_script(cluster_id: &str) -> Script {
        Script {
            code_hash: CLUSTER_CODE_HASH_MAINNET_V2.to_string(),
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
        let spore_hash = parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2);
        assert!(SporeParser::is_spore_type_script(&spore_hash));

        let bit_cell_hash = parse_hex_to_bytes(BIT_CELL_CODE_HASH_MAINNET);
        assert!(!SporeParser::is_spore_type_script(&bit_cell_hash));

        let other_hash = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!SporeParser::is_spore_type_script(&other_hash));
    }

    #[test]
    fn test_is_spore_nft_type_script_excludes_bit_cell() {
        let spore_hash = parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2);
        assert!(SporeParser::is_spore_nft_type_script(&spore_hash));

        let bit_cell_hash = parse_hex_to_bytes(BIT_CELL_CODE_HASH_MAINNET);
        assert!(!SporeParser::is_spore_nft_type_script(&bit_cell_hash));
    }

    #[test]
    fn test_is_cluster_type_script() {
        let cluster_hash = parse_hex_to_bytes(CLUSTER_CODE_HASH_MAINNET_V2);
        assert!(SporeParser::is_cluster_type_script(&cluster_hash));

        let other_hash = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!SporeParser::is_cluster_type_script(&other_hash));
    }

    #[test]
    fn test_real_testnet_did_ckb_cell_classifies_as_did() {
        use crate::parser::test_helpers::real_did_ckb;

        // Registry maps docs/metadata/scripts/did-ckb.toml (slug "did-ckb") on
        // both networks; the did classifier must fire for those code hashes.
        let testnet = parse_hex_to_bytes(real_did_ckb::TYPE_CODE_HASH_TESTNET);
        assert!(
            SporeParser::is_did_type_script(&testnet),
            "did:ckb testnet code_hash must classify as did"
        );
        let mainnet = parse_hex_to_bytes(real_did_ckb::TYPE_CODE_HASH_MAINNET);
        assert!(
            SporeParser::is_did_type_script(&mainnet),
            "did:ckb mainnet code_hash must classify as did"
        );
    }

    #[test]
    fn test_real_testnet_did_ckb_cell_parses_item_id_from_args() {
        use crate::parser::test_helpers::real_did_ckb;

        // Live-path parse of the audited testnet cell 0x00290adc…:0
        // (block 18082860): the identity item id is the full type-script args.
        let (output, data_hex) = real_did_ckb::cell_32();
        let parsed = SporeParser::parse_spore_cell(&output, data_hex)
            .expect("real did:ckb cell must be classified by the live parse path");
        assert!(parsed.is_did);
        assert_eq!(
            parsed.spore_id,
            parse_hex_to_bytes(real_did_ckb::CELL_32_ARGS),
            "item id must be the exact type-script args"
        );
        assert_eq!(parsed.owner_lock_hash.len(), 32);
    }

    #[test]
    fn test_real_testnet_did_ckb_20_byte_args_cell_parses_in_bulk_path() {
        use crate::parser::test_helpers::real_did_ckb;

        // 31 of 421 live testnet did:ckb cells carry 20-byte args. The bulk
        // (ParsedCell) path must classify them and preserve the exact 20-byte
        // item id; a fixed 32-byte gate silently drops these real cells.
        let (output, data_hex) = real_did_ckb::cell_20();
        let parsed_cell = CellParser::parse_output(&output, data_hex).expect("parsed cell");
        let parsed = SporeParser::parse_spore_parsed_cell(&parsed_cell)
            .expect("real 20-byte-args did:ckb cell must be classified by the bulk parse path");
        assert!(parsed.is_did);
        let expected_id = parse_hex_to_bytes(real_did_ckb::CELL_20_ARGS);
        assert_eq!(expected_id.len(), 20);
        assert_eq!(
            parsed.spore_id, expected_id,
            "20-byte item id must be preserved verbatim"
        );
    }

    #[test]
    fn excludes_testnet_bit_cell_from_spore() {
        let t = parse_hex_to_bytes(
            "0x0b1f412fbae26853ff7d082d422c2bdd9e2ff94ee8aaec11240a5b34cc6e890f",
        );
        assert!(!SporeParser::is_did_type_script(&t));
        assert!(!SporeParser::is_spore_type_script(&t));
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
    fn test_parse_bit_cell_is_not_spore() {
        let type_args = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_bit_cell_type_script(type_args)),
        };

        assert!(SporeParser::parse_spore_cell(&output, "0x0102030405").is_none());
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
    fn test_parse_spore_parsed_cell_basic() {
        let spore_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_spore_type_script(spore_id)),
        };
        let data = create_spore_data("image/png", b"fake png data", None);
        let data_hex = format!("0x{}", hex::encode(&data));
        let parsed_cell = CellParser::parse_output(&output, &data_hex).expect("parsed cell");

        let parsed = SporeParser::parse_spore_parsed_cell(&parsed_cell).expect("spore facts");

        assert!(!parsed.is_did);
        assert_eq!(parsed.spore_id.len(), 32);
        assert_eq!(parsed.content_type, "image/png");
        assert_eq!(parsed.content, b"fake png data");
        assert!(parsed.cluster_id.is_none());
        assert_eq!(
            parsed.type_script_hash,
            parsed_cell.type_script_hash.unwrap()
        );
    }

    #[test]
    fn test_parse_spore_parsed_cell_excludes_bit_cell() {
        let type_args = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_bit_cell_type_script(type_args)),
        };
        let parsed_cell = CellParser::parse_output(&output, "0x0102030405").expect("parsed cell");

        assert!(SporeParser::parse_spore_parsed_cell(&parsed_cell).is_none());
    }

    #[test]
    fn test_parse_cluster_parsed_cell_basic() {
        let cluster_id = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_cluster_type_script(cluster_id)),
        };
        let data = create_cluster_data("My Collection", "A collection of art");
        let data_hex = format!("0x{}", hex::encode(&data));
        let parsed_cell = CellParser::parse_output(&output, &data_hex).expect("parsed cell");

        let parsed = SporeParser::parse_cluster_parsed_cell(&parsed_cell).expect("cluster facts");

        assert_eq!(parsed.cluster_id.len(), 32);
        assert_eq!(parsed.name, Some("My Collection".to_string()));
        assert_eq!(parsed.description, Some("A collection of art".to_string()));
        assert_eq!(
            parsed.type_script_hash,
            parsed_cell.type_script_hash.unwrap()
        );
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
    fn test_parse_spore_content_from_data_extracts_content() {
        let content = b"{\"dna\":\"0xaabbccdd\"}";
        let data = create_spore_data("dob/0", content, Some(&[0x11; 32]));
        let extracted = SporeParser::parse_spore_content_from_data(&data).unwrap();
        assert_eq!(extracted, content.to_vec());
    }

    #[test]
    fn test_parse_spore_content_from_data_too_short() {
        let result = SporeParser::parse_spore_content_from_data(&[0u8; 8]);
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

    #[test]
    #[should_panic(expected = "outputs/outputs_data length mismatch")]
    fn test_parse_spores_panics_on_outputs_data_length_mismatch() {
        let tx = TransactionView {
            hash: "0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            outputs_data: vec![],
            witnesses: vec![],
        };
        let _ = SporeParser::parse_spores(&tx);
    }

    #[test]
    #[should_panic(expected = "outputs/outputs_data length mismatch")]
    fn test_parse_clusters_panics_on_outputs_data_length_mismatch() {
        let tx = TransactionView {
            hash: "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            outputs_data: vec![],
            witnesses: vec![],
        };
        let _ = SporeParser::parse_clusters(&tx);
    }

    #[test]
    fn test_parse_spores_with_output_indices_preserves_real_output_index() {
        let spore_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let non_spore_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };
        let spore_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_spore_type_script(spore_id)),
        };
        let data = create_spore_data("image/png", b"test", None);
        let data_hex = format!("0x{}", hex::encode(&data));

        let tx = TransactionView {
            hash: "0xaa".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![non_spore_output, spore_output],
            outputs_data: vec!["0x".to_string(), data_hex],
            witnesses: vec![],
        };

        let parsed = SporeParser::parse_spores_with_output_indices(&tx);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].0, 1,
            "spore at output index 1 must preserve real index, not filtered index 0"
        );
    }
}
