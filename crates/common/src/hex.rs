/// Parses a hex string (with or without "0x" prefix) to bytes.
pub fn parse_hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    hex::decode(hex).unwrap_or_default()
}

/// Parses a hex string (with or without "0x" prefix) to a fixed-size 32-byte hash.
/// Returns [0u8; 32] if the input is invalid or not exactly 32 bytes.
pub fn parse_hex_to_hash(hex: &str) -> [u8; 32] {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let bytes = hex::decode(hex).unwrap_or_default();
    if bytes.len() == 32 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        arr
    } else {
        [0u8; 32]
    }
}

/// Parses a hex string to u32.
pub fn parse_hex_u32(hex: &str) -> u32 {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u32::from_str_radix(hex, 16).unwrap_or(0)
}

/// Parses a hex capacity string to decimal string.
pub fn parse_capacity(hex: &str) -> String {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(hex, 16)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_to_bytes() {
        assert_eq!(parse_hex_to_bytes("0x1234"), vec![0x12, 0x34]);
        assert_eq!(parse_hex_to_bytes("1234"), vec![0x12, 0x34]);
        assert_eq!(parse_hex_to_bytes("0x"), Vec::<u8>::new());
        assert_eq!(parse_hex_to_bytes("invalid"), Vec::<u8>::new());
    }

    #[test]
    fn test_parse_hex_u32() {
        assert_eq!(parse_hex_u32("0x10"), 16);
        assert_eq!(parse_hex_u32("ff"), 255);
        assert_eq!(parse_hex_u32("invalid"), 0);
    }

    #[test]
    fn test_parse_capacity() {
        assert_eq!(parse_capacity("0x174876e800"), "100000000000");
        assert_eq!(parse_capacity("invalid"), "0");
    }

    #[test]
    fn test_parse_hex_to_hash_valid_with_prefix() {
        let hex = "0x0000000000000000000000000000000000000000000000000000000000000001";
        let result = parse_hex_to_hash(hex);
        let mut expected = [0u8; 32];
        expected[31] = 1;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_hex_to_hash_valid_without_prefix() {
        let hex = "0000000000000000000000000000000000000000000000000000000000000001";
        let result = parse_hex_to_hash(hex);
        let mut expected = [0u8; 32];
        expected[31] = 1;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_hex_to_hash_all_ff() {
        let hex = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let result = parse_hex_to_hash(hex);
        assert_eq!(result, [0xff; 32]);
    }

    #[test]
    fn test_parse_hex_to_hash_too_short() {
        let result = parse_hex_to_hash("0x1234");
        assert_eq!(result, [0u8; 32]);
    }

    #[test]
    fn test_parse_hex_to_hash_too_long() {
        let hex = "0x00000000000000000000000000000000000000000000000000000000000000000000";
        let result = parse_hex_to_hash(hex);
        assert_eq!(result, [0u8; 32]);
    }

    #[test]
    fn test_parse_hex_to_hash_invalid_hex() {
        let result = parse_hex_to_hash("0xZZZZZZ");
        assert_eq!(result, [0u8; 32]);
    }

    #[test]
    fn test_parse_hex_to_hash_empty() {
        let result = parse_hex_to_hash("");
        assert_eq!(result, [0u8; 32]);
    }

    #[test]
    fn test_parse_hex_to_hash_just_prefix() {
        let result = parse_hex_to_hash("0x");
        assert_eq!(result, [0u8; 32]);
    }
}
