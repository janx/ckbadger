use chrono::{DateTime, TimeZone, Utc};

use crate::rpc::{parse_hex_to_bytes, BlockView, HeaderView};

pub struct ParsedBlock {
    pub number: i64,
    pub hash: Vec<u8>,
    pub parent_hash: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub version: i32,
    pub compact_target: i64,
    pub transactions_count: i32,
    pub proposals_count: i32,
    pub uncles_count: i32,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub dao: Vec<u8>,
    pub nonce: Vec<u8>,
    pub extra_hash: Vec<u8>,
    pub proposals_hash: Vec<u8>,
    pub transactions_root: Vec<u8>,
    pub proposals: Vec<Vec<u8>>,
}

pub struct BlockParser;

impl BlockParser {
    pub fn parse(block: &BlockView) -> ParsedBlock {
        let header = &block.header;
        let (epoch_number, epoch_index, epoch_length) = Self::parse_epoch(&header.epoch);

        let proposals: Vec<Vec<u8>> = block
            .proposals
            .iter()
            .map(|p| parse_hex_to_bytes(p))
            .collect();

        ParsedBlock {
            number: Self::parse_hex_i64(&header.number),
            hash: parse_hex_to_bytes(&header.hash),
            parent_hash: parse_hex_to_bytes(&header.parent_hash),
            timestamp: Self::parse_timestamp(&header.timestamp),
            version: Self::parse_hex_i32(&header.version),
            compact_target: Self::parse_hex_i64(&header.compact_target),
            transactions_count: i32::try_from(block.transactions.len()).unwrap_or_else(|_| {
                panic!(
                    "block transactions_count exceeds i32: {}",
                    block.transactions.len()
                )
            }),
            proposals_count: i32::try_from(block.proposals.len()).unwrap_or_else(|_| {
                panic!(
                    "block proposals_count exceeds i32: {}",
                    block.proposals.len()
                )
            }),
            uncles_count: i32::try_from(block.uncles.len()).unwrap_or_else(|_| {
                panic!("block uncles_count exceeds i32: {}", block.uncles.len())
            }),
            epoch_number,
            epoch_index,
            epoch_length,
            dao: parse_hex_to_bytes(&header.dao),
            nonce: Self::parse_nonce(&header.nonce),
            extra_hash: parse_hex_to_bytes(&header.extra_hash),
            proposals_hash: parse_hex_to_bytes(&header.proposals_hash),
            transactions_root: parse_hex_to_bytes(&header.transactions_root),
            proposals,
        }
    }

    pub fn parse_header(header: &HeaderView) -> ParsedBlock {
        let (epoch_number, epoch_index, epoch_length) = Self::parse_epoch(&header.epoch);

        ParsedBlock {
            number: Self::parse_hex_i64(&header.number),
            hash: parse_hex_to_bytes(&header.hash),
            parent_hash: parse_hex_to_bytes(&header.parent_hash),
            timestamp: Self::parse_timestamp(&header.timestamp),
            version: Self::parse_hex_i32(&header.version),
            compact_target: Self::parse_hex_i64(&header.compact_target),
            transactions_count: 0,
            proposals_count: 0,
            uncles_count: 0,
            epoch_number,
            epoch_index,
            epoch_length,
            dao: parse_hex_to_bytes(&header.dao),
            nonce: Self::parse_nonce(&header.nonce),
            extra_hash: parse_hex_to_bytes(&header.extra_hash),
            proposals_hash: parse_hex_to_bytes(&header.proposals_hash),
            transactions_root: parse_hex_to_bytes(&header.transactions_root),
            proposals: Vec::new(),
        }
    }

    fn parse_epoch(epoch_hex: &str) -> (i64, i32, i32) {
        let epoch = Self::parse_hex_u64(epoch_hex);
        let length = (epoch >> 40) & 0xFFFF;
        let index = (epoch >> 24) & 0xFFFF;
        let number = epoch & 0xFFFFFF;
        (number as i64, index as i32, length as i32)
    }

    pub fn parse_timestamp(timestamp_hex: &str) -> DateTime<Utc> {
        let ms = Self::parse_hex_u64(timestamp_hex);
        let ms = i64::try_from(ms).unwrap_or_else(|_| {
            panic!(
                "timestamp over i64 range '{}': {} (max={})",
                timestamp_hex,
                ms,
                i64::MAX
            )
        });
        Utc.timestamp_millis_opt(ms)
            .single()
            .expect("Invalid timestamp in block header")
    }

    fn parse_nonce(nonce_hex: &str) -> Vec<u8> {
        let nonce = Self::parse_hex_u128(nonce_hex);
        nonce.to_le_bytes().to_vec()
    }

    fn parse_hex_i64(hex: &str) -> i64 {
        let parsed = Self::parse_hex_u64(hex);
        i64::try_from(parsed).unwrap_or_else(|_| {
            panic!(
                "block hex over i64 range '{}': {} (max={})",
                hex,
                parsed,
                i64::MAX
            )
        })
    }

    fn parse_hex_i32(hex: &str) -> i32 {
        let parsed = Self::parse_hex_u64(hex);
        i32::try_from(parsed).unwrap_or_else(|_| {
            panic!(
                "block hex over i32 range '{}': {} (max={})",
                hex,
                parsed,
                i32::MAX
            )
        })
    }

    pub fn parse_block_number(block: &BlockView) -> u64 {
        Self::parse_hex_u64(&block.header.number)
    }

    fn parse_hex_u64(hex: &str) -> u64 {
        let raw = hex;
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        u64::from_str_radix(hex, 16)
            .unwrap_or_else(|e| panic!("invalid block hex '{}': {}", raw, e))
    }

    fn parse_hex_u128(hex: &str) -> u128 {
        let raw = hex;
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        u128::from_str_radix(hex, 16)
            .unwrap_or_else(|e| panic!("invalid block hex '{}': {}", raw, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script};

    fn create_test_header() -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: "0x18c7b3b2b00".to_string(), // 1704067200000 (2024-01-01 00:00:00 UTC)
            number: "0x1234".to_string(),           // 4660
            epoch: "0x7080006000028".to_string(),   // epoch 40, index 6, length 1800
            parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000001"
                .to_string(),
            transactions_root: "0x0000000000000000000000000000000000000000000000000000000000000002"
                .to_string(),
            proposals_hash: "0x0000000000000000000000000000000000000000000000000000000000000003"
                .to_string(),
            extra_hash: "0x0000000000000000000000000000000000000000000000000000000000000004"
                .to_string(),
            dao: "0x0000000000000000000000000000000000000000000000000000000000000005".to_string(),
            nonce: "0x12345678".to_string(),
            hash: "0x0000000000000000000000000000000000000000000000000000000000000006".to_string(),
        }
    }

    fn create_test_block() -> BlockView {
        let script = Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };

        BlockView {
            header: create_test_header(),
            transactions: vec![crate::rpc::TransactionView {
                hash: "0x0000000000000000000000000000000000000000000000000000000000000007"
                    .to_string(),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![CellInput {
                    previous_output: OutPoint {
                        tx_hash:
                            "0x0000000000000000000000000000000000000000000000000000000000000000"
                                .to_string(),
                        index: "0x0".to_string(),
                    },
                    since: "0x0".to_string(),
                }],
                outputs: vec![CellOutput {
                    capacity: "0x174876e800".to_string(), // 100 CKB
                    lock: script.clone(),
                    type_: None,
                }],
                outputs_data: vec!["0x".to_string()],
                witnesses: vec!["0x".to_string()],
            }],
            proposals: vec!["0x12345678901234567890".to_string()],
            uncles: vec![],
        }
    }

    #[test]
    fn test_parse_epoch_extracts_components_correctly() {
        // epoch format: upper 16 bits = length, middle 16 bits = index, lower 24 bits = number
        // 0x7080006000028 = length:1800 (0x708), index:6, number:40 (0x28)
        let epoch_hex = "0x7080006000028";
        let (number, index, length) = BlockParser::parse_epoch(epoch_hex);

        assert_eq!(number, 40, "epoch number should be 40");
        assert_eq!(index, 6, "epoch index should be 6");
        assert_eq!(length, 1800, "epoch length should be 1800");
    }

    #[test]
    fn test_parse_epoch_genesis() {
        // Genesis epoch: number=0, index=0, length=1000
        let epoch_hex = "0x3e80000000000"; // length:1000 (0x3e8), index:0, number:0
        let (number, index, length) = BlockParser::parse_epoch(epoch_hex);

        assert_eq!(number, 0);
        assert_eq!(index, 0);
        assert_eq!(length, 1000);
    }

    #[test]
    fn test_parse_timestamp_converts_to_datetime() {
        // 1704067200000 ms = 2024-01-01 00:00:00 UTC
        let timestamp_hex = "0x18cc251f400";
        let dt = BlockParser::parse_timestamp(timestamp_hex);

        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_nonce_converts_to_le_bytes() {
        let nonce_hex = "0x12345678";
        let nonce_bytes = BlockParser::parse_nonce(nonce_hex);

        // u128 in little-endian
        assert_eq!(nonce_bytes.len(), 16);
        assert_eq!(nonce_bytes[0], 0x78);
        assert_eq!(nonce_bytes[1], 0x56);
        assert_eq!(nonce_bytes[2], 0x34);
        assert_eq!(nonce_bytes[3], 0x12);
    }

    #[test]
    fn test_parse_hex_u64_with_prefix() {
        assert_eq!(BlockParser::parse_hex_u64("0x1234"), 0x1234);
        assert_eq!(BlockParser::parse_hex_u64("0xff"), 255);
        assert_eq!(BlockParser::parse_hex_u64("0x0"), 0);
    }

    #[test]
    fn test_parse_hex_u64_without_prefix() {
        assert_eq!(BlockParser::parse_hex_u64("1234"), 0x1234);
        assert_eq!(BlockParser::parse_hex_u64("ff"), 255);
    }

    #[test]
    #[should_panic(expected = "invalid block hex")]
    fn test_parse_hex_u64_invalid_panics() {
        let _ = BlockParser::parse_hex_u64("not_hex");
    }

    #[test]
    fn test_parse_hex_i64_preserves_value() {
        assert_eq!(BlockParser::parse_hex_i64("0x1234"), 0x1234);
        assert_eq!(BlockParser::parse_hex_i64("0x7fffffffffffffff"), i64::MAX);
    }

    #[test]
    #[should_panic(expected = "block hex over i64 range")]
    fn test_parse_hex_i64_overflow_panics() {
        let _ = BlockParser::parse_hex_i64("0x8000000000000000");
    }

    #[test]
    fn test_parse_hex_i32_preserves_value() {
        assert_eq!(BlockParser::parse_hex_i32("0x0"), 0);
        assert_eq!(BlockParser::parse_hex_i32("0x1"), 1);
        assert_eq!(BlockParser::parse_hex_i32("0xff"), 255);
    }

    #[test]
    #[should_panic(expected = "block hex over i32 range")]
    fn test_parse_hex_i32_overflow_panics() {
        let _ = BlockParser::parse_hex_i32("0x100000000");
    }

    #[test]
    fn test_parse_block_extracts_all_fields() {
        let block = create_test_block();
        let parsed = BlockParser::parse(&block);

        assert_eq!(parsed.number, 0x1234); // 4660
        assert_eq!(parsed.transactions_count, 1);
        assert_eq!(parsed.proposals_count, 1);
        assert_eq!(parsed.uncles_count, 0);
        assert_eq!(parsed.epoch_number, 40);
        assert_eq!(parsed.epoch_index, 6);
        assert_eq!(parsed.epoch_length, 1800);
        assert_eq!(parsed.version, 0);
    }

    #[test]
    fn test_parse_block_parses_proposals() {
        let block = create_test_block();
        let parsed = BlockParser::parse(&block);

        assert_eq!(parsed.proposals.len(), 1);
        assert_eq!(parsed.proposals[0].len(), 10); // 20 hex chars = 10 bytes
    }

    #[test]
    fn test_parse_header_only() {
        let header = create_test_header();
        let parsed = BlockParser::parse_header(&header);

        assert_eq!(parsed.number, 0x1234);
        assert_eq!(parsed.transactions_count, 0); // No transactions in header-only parse
        assert_eq!(parsed.proposals_count, 0);
        assert_eq!(parsed.uncles_count, 0);
        assert!(parsed.proposals.is_empty());
    }

    #[test]
    fn test_parse_block_number() {
        let block = create_test_block();
        let number = BlockParser::parse_block_number(&block);

        assert_eq!(number, 0x1234); // 4660
    }

    use chrono::Datelike;
    use chrono::Timelike;

    #[test]
    fn test_parse_hex_u128() {
        assert_eq!(BlockParser::parse_hex_u128("0x1"), 1u128);
        assert_eq!(
            BlockParser::parse_hex_u128("0xffffffffffffffffffffffffffffffff"),
            u128::MAX
        );
    }
}
