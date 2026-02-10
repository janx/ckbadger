use anyhow::Result;
use ckbadger_common::SporeRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &SporeRebuildConfig,
) -> Result<()> {
    info!("spore rebuild is a no-op with RocksDB storage (spore CF maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: spore CF maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{parse_hex_to_bytes, SporeRebuildConfig, SporeRebuildResult};
    use serde::{Deserialize, Serialize};

    const CLUSTER_CODE_HASH_MAINNET_V2: &str =
        "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075";
    const CLUSTER_CODE_HASH_TESTNET_V2: &str =
        "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058";
    const CLUSTER_CODE_HASH_TESTNET_V1: &str =
        "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080";

    const SPORE_CODE_HASH_MAINNET_V2: &str =
        "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5";
    const SPORE_CODE_HASH_MAINNET_DID: &str =
        "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33";
    const SPORE_CODE_HASH_TESTNET_V2: &str =
        "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d";
    const SPORE_CODE_HASH_TESTNET_V1: &str =
        "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494";

    #[derive(Debug, Serialize)]
    struct RpcRequest {
        jsonrpc: &'static str,
        id: u32,
        method: &'static str,
        params: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct RpcTransaction {
        outputs_data: Vec<String>,
    }

    fn parse_cluster_data(data: &[u8]) -> Option<(Option<String>, Option<String>)> {
        if data.len() < 12 {
            return None;
        }

        let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        if data.len() < total_size.min(data.len()) || total_size < 12 {
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

        let name = read_molecule_bytes_field(data, offset_name, offset_description)
            .map(|b| String::from_utf8_lossy(&b).to_string());

        let description = read_molecule_bytes_field(data, offset_description, end_of_description)
            .map(|b| String::from_utf8_lossy(&b).to_string());

        Some((name, description))
    }

    fn parse_spore_content_type(data: &[u8]) -> Option<String> {
        if data.len() < 16 {
            return None;
        }

        let _total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        let offset_content_type = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
        let offset_content = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;

        let bytes = read_molecule_bytes_field(data, offset_content_type, offset_content)?;
        Some(String::from_utf8_lossy(&bytes).to_string())
    }

    fn parse_spore_cluster_id(data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < 16 {
            return None;
        }

        let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        if data.len() < total_size || total_size < 16 {
            return None;
        }

        let offset_cluster_id = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;

        if offset_cluster_id >= total_size {
            return None;
        }

        if offset_cluster_id + 4 > data.len() {
            return None;
        }

        let opt_header = u32::from_le_bytes(
            data[offset_cluster_id..offset_cluster_id + 4]
                .try_into()
                .ok()?,
        ) as usize;
        if opt_header == 0 {
            return None;
        }

        read_molecule_bytes_field(data, offset_cluster_id, total_size)
    }

    /// Molecule Bytes field layout: [4B content_length LE][content bytes...]
    fn read_molecule_bytes_field(data: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
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

    #[test]
    fn test_default_config() {
        let config = SporeRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
        assert!(config.ckb_rpc_url.is_empty());
    }

    #[test]
    fn test_custom_config() {
        let config = SporeRebuildConfig {
            batch_size: 5_000,
            ckb_rpc_url: "http://localhost:8114".to_string(),
        };
        assert_eq!(config.batch_size, 5_000);
        assert_eq!(config.ckb_rpc_url, "http://localhost:8114");
    }

    #[test]
    fn test_result_serialization() {
        let result = SporeRebuildResult {
            spores_processed: 1000,
            spores_marked_consumed: 500,
            clusters_updated: 10,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["sporesProcessed"], 1000);
        assert_eq!(json["sporesMarkedConsumed"], 500);
        assert_eq!(json["clustersUpdated"], 10);
    }

    fn encode_molecule_bytes(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u32;
        let mut result = len.to_le_bytes().to_vec();
        result.extend_from_slice(data);
        result
    }

    fn build_cluster_data(name: &str, description: &str) -> Vec<u8> {
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

    fn build_spore_data(content_type: &str, content: &[u8], cluster_id: Option<&[u8]>) -> Vec<u8> {
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

    #[test]
    fn test_parse_cluster_data_basic() {
        let data = build_cluster_data("My Collection", "A great collection");
        let result = parse_cluster_data(&data);
        assert!(result.is_some());

        let (name, description) = result.unwrap();
        assert_eq!(name.as_deref(), Some("My Collection"));
        assert_eq!(description.as_deref(), Some("A great collection"));
    }

    #[test]
    fn test_parse_cluster_data_too_short() {
        let data = [0u8; 8];
        assert!(parse_cluster_data(&data).is_none());
    }

    #[test]
    fn test_parse_spore_content_type_basic() {
        let data = build_spore_data("image/png", b"fake png data", None);
        let result = parse_spore_content_type(&data);
        assert_eq!(result.as_deref(), Some("image/png"));
    }

    #[test]
    fn test_parse_spore_content_type_text() {
        let data = build_spore_data("text/plain", b"hello", None);
        let result = parse_spore_content_type(&data);
        assert_eq!(result.as_deref(), Some("text/plain"));
    }

    #[test]
    fn test_parse_spore_content_type_too_short() {
        let data = [0u8; 8];
        assert!(parse_spore_content_type(&data).is_none());
    }

    #[test]
    fn test_parse_spore_cluster_id_present() {
        let cluster_id = [0xab; 32];
        let data = build_spore_data("image/png", b"content", Some(&cluster_id));
        let result = parse_spore_cluster_id(&data);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), cluster_id.to_vec());
    }

    #[test]
    fn test_parse_spore_cluster_id_absent() {
        let data = build_spore_data("image/png", b"content", None);
        let result = parse_spore_cluster_id(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_spore_cluster_id_too_short() {
        let data = [0u8; 8];
        assert!(parse_spore_cluster_id(&data).is_none());
    }

    #[test]
    fn test_read_molecule_bytes_field_valid() {
        let content = b"hello";
        let mut data = (content.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(content);
        let result = read_molecule_bytes_field(&data, 0, data.len());
        assert_eq!(result.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn test_read_molecule_bytes_field_invalid_range() {
        let data = [0u8; 16];
        assert!(read_molecule_bytes_field(&data, 20, 30).is_none());
    }

    #[test]
    fn test_read_molecule_bytes_field_start_ge_end() {
        let data = [0u8; 16];
        assert!(read_molecule_bytes_field(&data, 10, 5).is_none());
    }

    #[test]
    fn test_rpc_request_serialization() {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "get_transaction",
            params: vec!["0xabc123".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"method\":\"get_transaction\""));
        assert!(json.contains("\"params\":[\"0xabc123\"]"));
    }

    #[test]
    fn test_rpc_transaction_deserialization() {
        let json = r#"{
            "outputs_data": ["0xabcdef", "0x123456"]
        }"#;

        let tx: RpcTransaction = serde_json::from_str(json).unwrap();
        assert_eq!(tx.outputs_data.len(), 2);
        assert_eq!(tx.outputs_data[0], "0xabcdef");
        assert_eq!(tx.outputs_data[1], "0x123456");
    }

    #[test]
    fn test_cluster_code_hashes_valid() {
        let h1 = parse_hex_to_bytes(CLUSTER_CODE_HASH_MAINNET_V2);
        let h2 = parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V2);
        let h3 = parse_hex_to_bytes(CLUSTER_CODE_HASH_TESTNET_V1);
        assert_eq!(h1.len(), 32);
        assert_eq!(h2.len(), 32);
        assert_eq!(h3.len(), 32);
    }

    #[test]
    fn test_spore_code_hashes_valid() {
        let h1 = parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2);
        let h2 = parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_DID);
        let h3 = parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V2);
        let h4 = parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V1);
        assert_eq!(h1.len(), 32);
        assert_eq!(h2.len(), 32);
        assert_eq!(h3.len(), 32);
        assert_eq!(h4.len(), 32);
    }
}
