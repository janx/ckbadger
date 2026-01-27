pub fn encode_cursor(block_number: i64, index: i32) -> String {
    format!("{}:{}", block_number, index)
}

pub fn decode_cursor(cursor: &str) -> Option<(i64, i32)> {
    let parts: Vec<&str> = cursor.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let block_number = parts[0].parse().ok()?;
    let index = parts[1].parse().ok()?;
    Some((block_number, index))
}

pub fn encode_cursor_single(id: i64) -> String {
    id.to_string()
}

pub fn decode_cursor_single(cursor: &str) -> Option<i64> {
    cursor.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_cursor() {
        let cursor = encode_cursor(12345, 67);
        assert_eq!(cursor, "12345:67");

        let decoded = decode_cursor(&cursor);
        assert_eq!(decoded, Some((12345, 67)));
    }

    #[test]
    fn test_decode_cursor_invalid_format() {
        assert_eq!(decode_cursor("invalid"), None);
        assert_eq!(decode_cursor("12345"), None);
        assert_eq!(decode_cursor("12345:67:89"), None);
        assert_eq!(decode_cursor("abc:def"), None);
    }

    #[test]
    fn test_encode_decode_cursor_single() {
        let cursor = encode_cursor_single(12345);
        assert_eq!(cursor, "12345");

        let decoded = decode_cursor_single(&cursor);
        assert_eq!(decoded, Some(12345));
    }

    #[test]
    fn test_decode_cursor_single_invalid() {
        assert_eq!(decode_cursor_single("invalid"), None);
        assert_eq!(decode_cursor_single("12.34"), None);
    }

    #[test]
    fn test_cursor_edge_cases() {
        assert_eq!(decode_cursor(&encode_cursor(0, 0)), Some((0, 0)));
        assert_eq!(
            decode_cursor(&encode_cursor(i64::MAX, i32::MAX)),
            Some((i64::MAX, i32::MAX))
        );
        assert_eq!(decode_cursor_single(&encode_cursor_single(0)), Some(0));
        assert_eq!(
            decode_cursor_single(&encode_cursor_single(i64::MAX)),
            Some(i64::MAX)
        );
    }
}
