/// Parses a hex string (with or without "0x" prefix) to bytes.
pub fn parse_hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    hex::decode(hex).unwrap_or_default()
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
}
