use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use ckb_types::prelude::*;

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
    pub dao: [u8; 32],
    pub nonce: Vec<u8>,
    pub extra_hash: Vec<u8>,
    pub proposals_hash: Vec<u8>,
    pub transactions_root: Vec<u8>,
    pub proposals: Vec<Vec<u8>>,
    /// Script hash of the block's own miner, from the cellbase witness lock
    /// (RFC-0022 `CellbaseWitness.lock`). `None` only for the genesis block,
    /// which is not mined. NOT the cellbase output lock — that one pays the
    /// reward of the block 11 confirmations back.
    pub miner_lock_hash: Option<Vec<u8>>,
}

pub struct BlockParser;

impl BlockParser {
    pub fn parse(block: &BlockView) -> Result<ParsedBlock> {
        let header = &block.header;
        let (epoch_number, epoch_index, epoch_length) = Self::parse_epoch(&header.epoch)?;

        let proposals: Vec<Vec<u8>> = block
            .proposals
            .iter()
            .map(|p| parse_hex_to_bytes(p))
            .collect();

        let number = Self::parse_hex_i64(&header.number)?;
        let miner_lock_hash = Self::parse_miner_lock_hash(block, number)?;

        Ok(ParsedBlock {
            number,
            hash: parse_hex_to_bytes(&header.hash),
            parent_hash: parse_hex_to_bytes(&header.parent_hash),
            timestamp: Self::parse_timestamp(&header.timestamp)?,
            version: Self::parse_hex_i32(&header.version)?,
            compact_target: Self::parse_hex_i64(&header.compact_target)?,
            transactions_count: i32::try_from(block.transactions.len()).map_err(|_| {
                anyhow::anyhow!(
                    "block transactions_count exceeds i32: {}",
                    block.transactions.len()
                )
            })?,
            proposals_count: i32::try_from(block.proposals.len()).map_err(|_| {
                anyhow::anyhow!(
                    "block proposals_count exceeds i32: {}",
                    block.proposals.len()
                )
            })?,
            uncles_count: i32::try_from(block.uncles.len()).map_err(|_| {
                anyhow::anyhow!("block uncles_count exceeds i32: {}", block.uncles.len())
            })?,
            epoch_number,
            epoch_index,
            epoch_length,
            dao: Self::parse_fixed_hex_32(&header.dao, "header.dao")?,
            nonce: Self::parse_nonce(&header.nonce)?,
            extra_hash: parse_hex_to_bytes(&header.extra_hash),
            proposals_hash: parse_hex_to_bytes(&header.proposals_hash),
            transactions_root: parse_hex_to_bytes(&header.transactions_root),
            proposals,
            miner_lock_hash,
        })
    }

    /// Extract the block's miner lock script hash from the cellbase witness
    /// (RFC-0022 `CellbaseWitness.lock`). Every mined block MUST carry a valid
    /// CellbaseWitness in its cellbase first witness — the protocol reads the
    /// reward target lock from it 11 blocks later — so a parse failure on a
    /// non-genesis block is an invariant violation, not a soft miss.
    fn parse_miner_lock_hash(block: &BlockView, number: i64) -> Result<Option<Vec<u8>>> {
        if number == 0 {
            // The genesis block is not mined (its witness holds a zero script).
            return Ok(None);
        }
        let cellbase = block
            .transactions
            .first()
            .ok_or_else(|| anyhow::anyhow!("block {} has no cellbase transaction", number))?;
        let witness_hex = cellbase
            .witnesses
            .first()
            .ok_or_else(|| anyhow::anyhow!("block {} cellbase has no witness", number))?;
        let witness_bytes = parse_hex_to_bytes(witness_hex);
        let reader =
            ckb_types::packed::CellbaseWitnessReader::from_slice(&witness_bytes).map_err(|e| {
                anyhow::anyhow!(
                    "block {} cellbase witness is not a valid CellbaseWitness molecule: {}",
                    number,
                    e
                )
            })?;
        let lock = reader.to_entity().lock();
        Ok(Some(lock.calc_script_hash().raw_data().to_vec()))
    }

    pub fn parse_header(header: &HeaderView) -> Result<ParsedBlock> {
        let (epoch_number, epoch_index, epoch_length) = Self::parse_epoch(&header.epoch)?;

        Ok(ParsedBlock {
            number: Self::parse_hex_i64(&header.number)?,
            hash: parse_hex_to_bytes(&header.hash),
            parent_hash: parse_hex_to_bytes(&header.parent_hash),
            timestamp: Self::parse_timestamp(&header.timestamp)?,
            version: Self::parse_hex_i32(&header.version)?,
            compact_target: Self::parse_hex_i64(&header.compact_target)?,
            transactions_count: 0,
            proposals_count: 0,
            uncles_count: 0,
            epoch_number,
            epoch_index,
            epoch_length,
            dao: Self::parse_fixed_hex_32(&header.dao, "header.dao")?,
            nonce: Self::parse_nonce(&header.nonce)?,
            extra_hash: parse_hex_to_bytes(&header.extra_hash),
            proposals_hash: parse_hex_to_bytes(&header.proposals_hash),
            transactions_root: parse_hex_to_bytes(&header.transactions_root),
            proposals: Vec::new(),
            // Header-only parses carry no witnesses; miner attribution is
            // available only from full-block parses.
            miner_lock_hash: None,
        })
    }

    fn parse_epoch(epoch_hex: &str) -> Result<(i64, i32, i32)> {
        let epoch = Self::parse_hex_u64(epoch_hex)?;
        let length = (epoch >> 40) & 0xFFFF;
        let index = (epoch >> 24) & 0xFFFF;
        let number = epoch & 0xFFFFFF;
        Ok((number as i64, index as i32, length as i32))
    }

    fn parse_fixed_hex_32(value: &str, field: &str) -> Result<[u8; 32]> {
        let bytes = parse_hex_to_bytes(value);
        bytes.try_into().map_err(|actual: Vec<u8>| {
            anyhow::anyhow!(
                "invalid {} length: expected 32 bytes, got {}",
                field,
                actual.len()
            )
        })
    }

    pub fn parse_timestamp(timestamp_hex: &str) -> Result<DateTime<Utc>> {
        let ms = Self::parse_hex_u64(timestamp_hex)?;
        let ms = i64::try_from(ms).map_err(|_| {
            anyhow::anyhow!(
                "timestamp over i64 range '{}': {} (max={})",
                timestamp_hex,
                ms,
                i64::MAX
            )
        })?;
        Utc.timestamp_millis_opt(ms).single().ok_or_else(|| {
            anyhow::anyhow!(
                "invalid timestamp in block header: '{}' ({}ms)",
                timestamp_hex,
                ms
            )
        })
    }

    fn parse_nonce(nonce_hex: &str) -> Result<Vec<u8>> {
        let nonce = Self::parse_hex_u128(nonce_hex)?;
        Ok(nonce.to_le_bytes().to_vec())
    }

    fn parse_hex_i64(hex: &str) -> Result<i64> {
        let parsed = Self::parse_hex_u64(hex)?;
        i64::try_from(parsed).map_err(|_| {
            anyhow::anyhow!(
                "block hex over i64 range '{}': {} (max={})",
                hex,
                parsed,
                i64::MAX
            )
        })
    }

    fn parse_hex_i32(hex: &str) -> Result<i32> {
        let parsed = Self::parse_hex_u64(hex)?;
        i32::try_from(parsed).map_err(|_| {
            anyhow::anyhow!(
                "block hex over i32 range '{}': {} (max={})",
                hex,
                parsed,
                i32::MAX
            )
        })
    }

    pub fn parse_block_number(block: &BlockView) -> Result<u64> {
        Self::parse_hex_u64(&block.header.number)
    }

    fn parse_hex_u64(hex: &str) -> Result<u64> {
        let raw = hex;
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        u64::from_str_radix(hex, 16)
            .map_err(|e| anyhow::anyhow!("invalid block hex '{}': {}", raw, e))
    }

    fn parse_hex_u128(hex: &str) -> Result<u128> {
        let raw = hex;
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        u128::from_str_radix(hex, 16)
            .map_err(|e| anyhow::anyhow!("invalid block hex '{}': {}", raw, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script};

    /// Real cellbase first witness of mainnet block 12,000,000: a
    /// CellbaseWitness molecule with the miner's secp lock
    /// (args 0x8211f1b9…) and a "0.113.0 (…)" client-version message.
    const MAINNET_12M_CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";

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
                witnesses: vec![MAINNET_12M_CELLBASE_WITNESS.to_string()],
            }],
            proposals: vec!["0x12345678901234567890".to_string()],
            uncles: vec![],
        }
    }

    /// Regression (F5): the miner is identified by the cellbase WITNESS lock
    /// (the block's own miner), never the cellbase output lock (which pays the
    /// miner of the block 11 confirmations back).
    #[test]
    fn test_parse_miner_lock_hash_uses_cellbase_witness_lock() {
        let block = create_test_block();
        let parsed = BlockParser::parse(&block).unwrap();

        let expected_lock = ckb_types::packed::Script::new_builder()
            .code_hash(
                ckb_types::packed::Byte32::from_slice(
                    &hex::decode(
                        "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .hash_type(ckb_types::core::ScriptHashType::Type.into())
            .args(
                hex::decode("8211f1b938a107cd53b6302cc752a6fc3965638d")
                    .unwrap()
                    .pack(),
            )
            .build();
        let expected_hash = expected_lock.calc_script_hash().raw_data().to_vec();

        assert_eq!(parsed.miner_lock_hash, Some(expected_hash));
    }

    #[test]
    fn test_parse_miner_lock_hash_genesis_is_none() {
        let mut block = create_test_block();
        block.header.number = "0x0".to_string();
        let parsed = BlockParser::parse(&block).unwrap();
        assert_eq!(parsed.miner_lock_hash, None);
    }

    #[test]
    fn test_parse_fails_on_invalid_cellbase_witness() {
        let mut block = create_test_block();
        block.transactions[0].witnesses = vec!["0x".to_string()];
        let err = match BlockParser::parse(&block) {
            Ok(_) => panic!("invalid cellbase witness must error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("CellbaseWitness"), "{err}");
    }

    #[test]
    fn test_parse_epoch_extracts_components_correctly() {
        // epoch format: upper 16 bits = length, middle 16 bits = index, lower 24 bits = number
        // 0x7080006000028 = length:1800 (0x708), index:6, number:40 (0x28)
        let epoch_hex = "0x7080006000028";
        let (number, index, length) = BlockParser::parse_epoch(epoch_hex).unwrap();

        assert_eq!(number, 40, "epoch number should be 40");
        assert_eq!(index, 6, "epoch index should be 6");
        assert_eq!(length, 1800, "epoch length should be 1800");
    }

    #[test]
    fn test_parse_epoch_genesis() {
        // Genesis epoch: number=0, index=0, length=1000
        let epoch_hex = "0x3e80000000000"; // length:1000 (0x3e8), index:0, number:0
        let (number, index, length) = BlockParser::parse_epoch(epoch_hex).unwrap();

        assert_eq!(number, 0);
        assert_eq!(index, 0);
        assert_eq!(length, 1000);
    }

    #[test]
    fn test_parse_timestamp_converts_to_datetime() {
        // 1704067200000 ms = 2024-01-01 00:00:00 UTC
        let timestamp_hex = "0x18cc251f400";
        let dt = BlockParser::parse_timestamp(timestamp_hex).unwrap();

        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_nonce_converts_to_le_bytes() {
        let nonce_hex = "0x12345678";
        let nonce_bytes = BlockParser::parse_nonce(nonce_hex).unwrap();

        // u128 in little-endian
        assert_eq!(nonce_bytes.len(), 16);
        assert_eq!(nonce_bytes[0], 0x78);
        assert_eq!(nonce_bytes[1], 0x56);
        assert_eq!(nonce_bytes[2], 0x34);
        assert_eq!(nonce_bytes[3], 0x12);
    }

    #[test]
    fn test_parse_hex_u64_with_prefix() {
        assert_eq!(BlockParser::parse_hex_u64("0x1234").unwrap(), 0x1234);
        assert_eq!(BlockParser::parse_hex_u64("0xff").unwrap(), 255);
        assert_eq!(BlockParser::parse_hex_u64("0x0").unwrap(), 0);
    }

    #[test]
    fn test_parse_hex_u64_without_prefix() {
        assert_eq!(BlockParser::parse_hex_u64("1234").unwrap(), 0x1234);
        assert_eq!(BlockParser::parse_hex_u64("ff").unwrap(), 255);
    }

    #[test]
    fn test_parse_hex_u64_invalid_returns_error() {
        let err = BlockParser::parse_hex_u64("not_hex").unwrap_err();
        assert!(err.to_string().contains("invalid block hex"));
    }

    #[test]
    fn test_parse_hex_i64_preserves_value() {
        assert_eq!(BlockParser::parse_hex_i64("0x1234").unwrap(), 0x1234);
        assert_eq!(
            BlockParser::parse_hex_i64("0x7fffffffffffffff").unwrap(),
            i64::MAX
        );
    }

    #[test]
    fn test_parse_hex_i64_overflow_returns_error() {
        let err = BlockParser::parse_hex_i64("0x8000000000000000").unwrap_err();
        assert!(err.to_string().contains("block hex over i64 range"));
    }

    #[test]
    fn test_parse_hex_i32_preserves_value() {
        assert_eq!(BlockParser::parse_hex_i32("0x0").unwrap(), 0);
        assert_eq!(BlockParser::parse_hex_i32("0x1").unwrap(), 1);
        assert_eq!(BlockParser::parse_hex_i32("0xff").unwrap(), 255);
    }

    #[test]
    fn test_parse_hex_i32_overflow_returns_error() {
        let err = BlockParser::parse_hex_i32("0x100000000").unwrap_err();
        assert!(err.to_string().contains("block hex over i32 range"));
    }

    #[test]
    fn test_parse_block_extracts_all_fields() {
        let block = create_test_block();
        let parsed = BlockParser::parse(&block).unwrap();

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
        let parsed = BlockParser::parse(&block).unwrap();

        assert_eq!(parsed.proposals.len(), 1);
        assert_eq!(parsed.proposals[0].len(), 10); // 20 hex chars = 10 bytes
    }

    #[test]
    fn test_parse_header_only() {
        let header = create_test_header();
        let parsed = BlockParser::parse_header(&header).unwrap();

        assert_eq!(parsed.number, 0x1234);
        assert_eq!(parsed.transactions_count, 0); // No transactions in header-only parse
        assert_eq!(parsed.proposals_count, 0);
        assert_eq!(parsed.uncles_count, 0);
        assert!(parsed.proposals.is_empty());
    }

    #[test]
    fn test_parse_block_number() {
        let block = create_test_block();
        let number = BlockParser::parse_block_number(&block).unwrap();

        assert_eq!(number, 0x1234); // 4660
    }

    use chrono::Datelike;
    use chrono::Timelike;

    #[test]
    fn test_parse_hex_u128() {
        assert_eq!(BlockParser::parse_hex_u128("0x1").unwrap(), 1u128);
        assert_eq!(
            BlockParser::parse_hex_u128("0xffffffffffffffffffffffffffffffff").unwrap(),
            u128::MAX
        );
    }
}
