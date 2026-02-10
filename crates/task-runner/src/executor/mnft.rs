use anyhow::Result;
use ckbadger_common::MnftRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &MnftRebuildConfig,
) -> Result<()> {
    info!("mnft rebuild is a no-op with RocksDB storage (NFT CFs maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: NFT CFs maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{MnftRebuildConfig, MnftRebuildResult};

    #[derive(Default)]
    struct IssuerParsed {
        name: Option<String>,
        info: Option<Vec<u8>>,
    }

    fn parse_issuer_data(data: &[u8]) -> IssuerParsed {
        if data.len() < 9 {
            return IssuerParsed::default();
        }

        let _version = data[0];

        if data.len() <= 11 {
            return IssuerParsed::default();
        }

        let info_size = u16::from_le_bytes(data[9..11].try_into().unwrap_or([0; 2])) as usize;

        if info_size == 0 || data.len() < 11 + info_size {
            return IssuerParsed::default();
        }

        let info_bytes = data[11..11 + info_size].to_vec();
        let name = extract_json_field(&info_bytes, "name");

        IssuerParsed {
            name,
            info: Some(info_bytes),
        }
    }

    struct ClassParsed {
        name: Option<String>,
        description: Option<String>,
        renderer: Option<String>,
        total: u32,
        issued: u32,
    }

    fn parse_class_data(data: &[u8]) -> Option<ClassParsed> {
        if data.len() < 10 {
            return None;
        }

        let _version = data[0];
        let total = u32::from_le_bytes(data[1..5].try_into().ok()?);
        let issued = u32::from_le_bytes(data[5..9].try_into().ok()?);
        let _configure = data[9];

        let mut offset = 10;
        let name = read_vartext(data, &mut offset);
        let description = read_vartext(data, &mut offset);
        let renderer = read_vartext(data, &mut offset);

        Some(ClassParsed {
            name,
            description,
            renderer,
            total,
            issued,
        })
    }

    struct TokenParsed {
        characteristic: Vec<u8>,
        configure: u8,
        state: u8,
    }

    fn parse_token_data(data: &[u8]) -> Option<TokenParsed> {
        if data.len() < 11 {
            return None;
        }

        let _version = data[0];
        let characteristic = data[1..9].to_vec();
        let configure = data[9];
        let state = data[10];

        Some(TokenParsed {
            characteristic,
            configure,
            state,
        })
    }

    fn read_vartext(data: &[u8], offset: &mut usize) -> Option<String> {
        if *offset + 2 > data.len() {
            return None;
        }

        let size = u16::from_le_bytes(data[*offset..*offset + 2].try_into().ok()?) as usize;
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

    fn bytes_to_pg_string(data: &[u8]) -> String {
        String::from_utf8_lossy(data).replace('\0', "")
    }

    #[test]
    fn test_default_config() {
        let config = MnftRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
    }

    #[test]
    fn test_parse_issuer_data_valid() {
        let mut data = vec![0u8];
        data.extend_from_slice(&5u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        let info = r#"{"name":"Test Issuer"}"#;
        data.extend_from_slice(&(info.len() as u16).to_le_bytes());
        data.extend_from_slice(info.as_bytes());

        let parsed = parse_issuer_data(&data);
        assert_eq!(parsed.name, Some("Test Issuer".to_string()));
        assert!(parsed.info.is_some());
    }

    #[test]
    fn test_parse_issuer_data_too_short() {
        let data = vec![0u8; 5];
        let parsed = parse_issuer_data(&data);
        assert!(parsed.name.is_none());
        assert!(parsed.info.is_none());
    }

    #[test]
    fn test_parse_class_data_valid() {
        let mut data = vec![0u8];
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&50u32.to_le_bytes());
        data.push(0b00000011);

        let name = "Test Class";
        data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        data.extend_from_slice(name.as_bytes());

        let desc = "A test collection";
        data.extend_from_slice(&(desc.len() as u16).to_le_bytes());
        data.extend_from_slice(desc.as_bytes());

        data.extend_from_slice(&0u16.to_le_bytes());

        let parsed = parse_class_data(&data).unwrap();
        assert_eq!(parsed.name, Some("Test Class".to_string()));
        assert_eq!(parsed.description, Some("A test collection".to_string()));
        assert!(parsed.renderer.is_none());
        assert_eq!(parsed.total, 100);
        assert_eq!(parsed.issued, 50);
    }

    #[test]
    fn test_parse_class_data_too_short() {
        let data = vec![0u8; 5];
        assert!(parse_class_data(&data).is_none());
    }

    #[test]
    fn test_parse_token_data_valid() {
        let mut data = vec![0u8];
        data.extend_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        data.push(0x01);
        data.push(0x00);

        let parsed = parse_token_data(&data).unwrap();
        assert_eq!(
            parsed.characteristic,
            vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
        );
        assert_eq!(parsed.configure, 0x01);
        assert_eq!(parsed.state, 0x00);
    }

    #[test]
    fn test_parse_token_data_too_short() {
        let data = vec![0u8; 5];
        assert!(parse_token_data(&data).is_none());
    }

    #[test]
    fn test_extract_json_field() {
        let json = r#"{"name":"Alice","age":30}"#;
        assert_eq!(
            extract_json_field(json.as_bytes(), "name"),
            Some("Alice".to_string())
        );
        assert!(extract_json_field(json.as_bytes(), "email").is_none());
    }

    #[test]
    fn test_bytes_to_pg_string() {
        assert_eq!(bytes_to_pg_string(b"hello"), "hello");
        assert_eq!(bytes_to_pg_string(b"null\0byte"), "nullbyte");
    }

    #[test]
    fn test_result_serialization() {
        let result = MnftRebuildResult {
            issuers_created: 10,
            classes_created: 20,
            tokens_created: 300,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["issuersCreated"], 10);
        assert_eq!(json["classesCreated"], 20);
        assert_eq!(json["tokensCreated"], 300);
    }
}
