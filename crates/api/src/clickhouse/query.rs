use axum::http::StatusCode;
use axum::Json;

use crate::response::ApiError;

pub fn hex_hash(field: &str) -> String {
    format!("lower(hex({}))", field)
}

pub fn unhex_hash(hex_str: &str) -> Result<Vec<u8>, (StatusCode, Json<ApiError>)> {
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    if hex_str.len() != 64 {
        return Err(ApiError::bad_request(format!(
            "Invalid hash length: expected 64 hex characters, got {}",
            hex_str.len()
        )));
    }

    hex::decode(hex_str).map_err(|e| ApiError::bad_request(format!("Invalid hex string: {}", e)))
}

pub fn build_where_hash(field: &str, hash: &str) -> Result<String, (StatusCode, Json<ApiError>)> {
    let _bytes = unhex_hash(hash)?;
    Ok(format!(
        "{} = unhex('{}')",
        field,
        hash.strip_prefix("0x").unwrap_or(hash)
    ))
}

pub fn build_where_block_range(start: Option<i64>, end: Option<i64>) -> String {
    match (start, end) {
        (Some(start), Some(end)) => {
            format!("block_number >= {} AND block_number <= {}", start, end)
        }
        (Some(start), None) => format!("block_number >= {}", start),
        (None, Some(end)) => format!("block_number <= {}", end),
        (None, None) => "1=1".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_hash() {
        assert_eq!(hex_hash("tx_hash"), "lower(hex(tx_hash))");
        assert_eq!(hex_hash("block_hash"), "lower(hex(block_hash))");
    }

    #[test]
    fn test_unhex_hash_valid() {
        let hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let result = unhex_hash(hash);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_unhex_hash_without_prefix() {
        let hash = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let result = unhex_hash(hash);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_unhex_hash_invalid_length() {
        let hash = "0x1234";
        let result = unhex_hash(hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_unhex_hash_invalid_hex() {
        let hash = "0xzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        let result = unhex_hash(hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_where_hash() {
        let hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let result = build_where_hash("tx_hash", hash);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "tx_hash = unhex('1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef')"
        );
    }

    #[test]
    fn test_build_where_block_range() {
        assert_eq!(
            build_where_block_range(Some(100), Some(200)),
            "block_number >= 100 AND block_number <= 200"
        );
        assert_eq!(
            build_where_block_range(Some(100), None),
            "block_number >= 100"
        );
        assert_eq!(
            build_where_block_range(None, Some(200)),
            "block_number <= 200"
        );
        assert_eq!(build_where_block_range(None, None), "1=1");
    }

    #[test]
    fn test_unhex_hash_roundtrip() {
        let original = vec![
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
            0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78,
            0x90, 0xab, 0xcd, 0xef,
        ];
        let hex_str = format!("0x{}", hex::encode(&original));
        let decoded = unhex_hash(&hex_str).unwrap();
        assert_eq!(decoded, original);
    }
}
