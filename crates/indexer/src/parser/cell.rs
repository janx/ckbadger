use anyhow::Result;

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::script::ScriptParser;

#[derive(Debug, Clone)]
pub struct ParsedCell {
    pub capacity: i64,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub lock_script_hash: Vec<u8>,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub data_hash: [u8; 32],
    pub data_size: i32,
    pub data: Vec<u8>,
}

impl ParsedCell {
    /// The `cells` payload of this output: the one conversion from a parsed
    /// output to the persisted cell. The payload keeps the script fields and
    /// derives the script hashes from them, so a code hash that is not 32
    /// bytes is an error here rather than a different hash later.
    pub fn to_live_cell_info(
        &self,
        udt_amount: Option<u128>,
    ) -> Result<ckbadger_store::LiveCellInfo> {
        let code_hash = |bytes: &[u8], script: &str| -> Result<[u8; 32]> {
            <[u8; 32]>::try_from(bytes).map_err(|_| {
                anyhow::anyhow!(
                    "{script} script code hash is {} bytes, not 32: 0x{}",
                    bytes.len(),
                    hex::encode(bytes)
                )
            })
        };
        Ok(ckbadger_store::LiveCellInfo {
            capacity: self.capacity,
            lock_code_hash: code_hash(&self.lock_code_hash, "lock")?,
            lock_hash_type: self.lock_hash_type,
            lock_args: self.lock_args.clone(),
            type_code_hash: self
                .type_code_hash
                .as_deref()
                .map(|bytes| code_hash(bytes, "type"))
                .transpose()?,
            type_hash_type: self.type_hash_type,
            type_args: self.type_args.clone(),
            data_size: self.data_size,
            udt_amount,
            data_hash: Some(self.data_hash),
        })
    }
}

pub struct CellParser;

impl CellParser {
    pub fn parse_outputs(tx: &TransactionView) -> Result<Vec<ParsedCell>> {
        tx.outputs
            .iter()
            .zip(tx.outputs_data.iter())
            .map(|(output, data)| Self::parse_output(output, data))
            .collect()
    }

    pub fn parse_output(output: &CellOutput, data_hex: &str) -> Result<ParsedCell> {
        let data = parse_hex_to_bytes(data_hex);
        let data_hash = ScriptParser::compute_data_hash(&data);

        let lock_script_hash = ScriptParser::compute_script_hash(&output.lock);

        let (type_code_hash, type_hash_type, type_args, type_script_hash) =
            if let Some(ref type_script) = output.type_ {
                (
                    Some(parse_hex_to_bytes(&type_script.code_hash)),
                    Some(ScriptParser::hash_type_to_i16(&type_script.hash_type)),
                    Some(parse_hex_to_bytes(&type_script.args)),
                    Some(ScriptParser::compute_script_hash(type_script)),
                )
            } else {
                (None, None, None, None)
            };

        Ok(ParsedCell {
            capacity: super::parse_capacity_i64(&output.capacity),
            lock_code_hash: parse_hex_to_bytes(&output.lock.code_hash),
            lock_hash_type: ScriptParser::hash_type_to_i16(&output.lock.hash_type),
            lock_args: parse_hex_to_bytes(&output.lock.args),
            lock_script_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            type_script_hash,
            data_hash,
            data_size: i32::try_from(data.len())
                .map_err(|_| anyhow::anyhow!("cell data size {} exceeds i32::MAX", data.len()))?,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::test_helpers::create_lock_script;
    use crate::rpc::{CellInput, CellOutput, OutPoint, Script, TransactionView};

    const SECP256K1_CODE_HASH: &str =
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";

    fn create_type_script() -> Script {
        Script {
            code_hash: "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        }
    }

    fn create_cell_output(capacity: &str, with_type: bool) -> CellOutput {
        CellOutput {
            capacity: capacity.to_string(),
            lock: create_lock_script(),
            type_: if with_type {
                Some(create_type_script())
            } else {
                None
            },
        }
    }

    fn create_test_tx() -> TransactionView {
        TransactionView {
            hash: "0x0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                previous_output: OutPoint {
                    tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000"
                        .to_string(),
                    index: "0x0".to_string(),
                },
                since: "0x0".to_string(),
            }],
            outputs: vec![
                create_cell_output("0x174876e800", false),
                create_cell_output("0x2540be400", true),
            ],
            outputs_data: vec!["0x".to_string(), "0xdeadbeef".to_string()],
            witnesses: vec![],
        }
    }

    #[test]
    fn test_parse_output_without_type_script() {
        let output = create_cell_output("0x174876e800", false);
        let data_hex = "0x";
        let parsed = CellParser::parse_output(&output, data_hex).unwrap();

        assert_eq!(parsed.capacity, 100_000_000_000);
        assert_eq!(parsed.lock_code_hash.len(), 32);
        assert_eq!(parsed.lock_hash_type, 1);
        assert_eq!(parsed.lock_args.len(), 20);
        assert!(parsed.type_code_hash.is_none());
        assert!(parsed.type_hash_type.is_none());
        assert!(parsed.type_args.is_none());
        assert!(parsed.type_script_hash.is_none());
        assert_eq!(parsed.data_size, 0);
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn test_parse_output_with_type_script() {
        let output = create_cell_output("0x2540be400", true);
        let data_hex = "0xdeadbeef";
        let parsed = CellParser::parse_output(&output, data_hex).unwrap();

        assert_eq!(parsed.capacity, 10_000_000_000);
        assert!(parsed.type_code_hash.is_some());
        assert_eq!(parsed.type_code_hash.as_ref().unwrap().len(), 32);
        assert_eq!(parsed.type_hash_type, Some(1));
        assert!(parsed.type_args.is_some());
        assert!(parsed.type_script_hash.is_some());
        assert_eq!(parsed.type_script_hash.as_ref().unwrap().len(), 32);
        assert_eq!(parsed.data_size, 4);
        assert_eq!(parsed.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn test_parse_output_computes_data_hash() {
        let output = create_cell_output("0x174876e800", false);
        let data_hex = "0xdeadbeef";
        let parsed = CellParser::parse_output(&output, data_hex).unwrap();

        assert_ne!(parsed.data_hash, [0u8; 32]);
    }

    #[test]
    fn test_parse_output_computes_lock_script_hash() {
        let output = create_cell_output("0x174876e800", false);
        let parsed = CellParser::parse_output(&output, "0x").unwrap();

        assert_eq!(parsed.lock_script_hash.len(), 32);
        assert_ne!(parsed.lock_script_hash, vec![0u8; 32]);
    }

    #[test]
    fn test_parse_outputs_multiple() {
        let tx = create_test_tx();
        let parsed = CellParser::parse_outputs(&tx).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].capacity, 100_000_000_000);
        assert!(parsed[0].type_code_hash.is_none());
        assert_eq!(parsed[1].capacity, 10_000_000_000);
        assert!(parsed[1].type_code_hash.is_some());
    }

    #[test]
    fn test_parse_outputs_empty_tx() {
        let tx = TransactionView {
            hash: "0x0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![],
            outputs_data: vec![],
            witnesses: vec![],
        };
        let parsed = CellParser::parse_outputs(&tx).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_lock_hash_type_mapping() {
        let script_type = Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let script_data = Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "data".to_string(),
            args: "0x".to_string(),
        };
        let script_data1 = Script {
            code_hash: SECP256K1_CODE_HASH.to_string(),
            hash_type: "data1".to_string(),
            args: "0x".to_string(),
        };

        let output_type = CellOutput {
            capacity: "0x0".to_string(),
            lock: script_type,
            type_: None,
        };
        let output_data = CellOutput {
            capacity: "0x0".to_string(),
            lock: script_data,
            type_: None,
        };
        let output_data1 = CellOutput {
            capacity: "0x0".to_string(),
            lock: script_data1,
            type_: None,
        };

        assert_eq!(
            CellParser::parse_output(&output_type, "0x")
                .unwrap()
                .lock_hash_type,
            1
        );
        assert_eq!(
            CellParser::parse_output(&output_data, "0x")
                .unwrap()
                .lock_hash_type,
            0
        );
        assert_eq!(
            CellParser::parse_output(&output_data1, "0x")
                .unwrap()
                .lock_hash_type,
            2
        );
    }

    // ---- `cells` payload v2 (`LiveCellInfo`) on real-shaped cells ----

    const SUDT_CODE_HASH: &str =
        "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
    const XUDT_CODE_HASH: &str =
        "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95";

    fn rpc_script(code_hash: &str, hash_type: &str, args: &[u8]) -> Script {
        Script {
            code_hash: code_hash.to_string(),
            hash_type: hash_type.to_string(),
            args: format!("0x{}", hex::encode(args)),
        }
    }

    /// Ten real-shaped mainnet outputs: six secp256k1/blake160 cells (20-byte
    /// args, no type, empty data), two sUDT and one xUDT cell (32-byte type
    /// args, 16-byte amount data; one amount above `u64`), and one 1,000-byte
    /// data cell. Every output gets its data hash, as both writers store it
    /// for every cell. The v1 layout averaged 217 B on this mix, against
    /// 219 B measured over mainnet's 102.6M `cells` rows.
    fn real_shaped_outputs() -> Vec<(CellOutput, String, Option<u128>)> {
        let lock = |seed: u8| rpc_script(SECP256K1_CODE_HASH, "type", &[seed; 20]);
        let plain = |capacity: u64, seed: u8| {
            let output = CellOutput {
                capacity: format!("{capacity:#x}"),
                lock: lock(seed),
                type_: None,
            };
            (output, "0x".to_string(), None)
        };
        let udt = |code_hash: &str, hash_type: &str, owner: u8, amount: u128, seed: u8| {
            let output = CellOutput {
                capacity: format!("{:#x}", 142_00000000u64),
                lock: lock(seed),
                type_: Some(rpc_script(code_hash, hash_type, &[owner; 32])),
            };
            let data = format!("0x{}", hex::encode(amount.to_le_bytes()));
            (output, data, Some(amount))
        };
        let data_cell = CellOutput {
            capacity: format!("{:#x}", 110_000_000_000u64),
            lock: lock(0x0a),
            type_: None,
        };
        vec![
            plain(61_00000000, 0x01),
            plain(100_00000000, 0x02),
            plain(100_000_000_000, 0x03),
            plain(9_876_543_210_000, 0x04),
            plain(142_00000000, 0x05),
            plain(400_000_000_000, 0x06),
            udt(SUDT_CODE_HASH, "type", 0xa1, 100_000_000_000_000, 0x07),
            udt(SUDT_CODE_HASH, "type", 0xa2, 500_000_000_000_000_000, 0x08),
            udt(XUDT_CODE_HASH, "data1", 0xa3, 10u128.pow(24), 0x09),
            (data_cell, format!("0x{}", "c3".repeat(1000)), None),
        ]
    }

    fn real_shaped_cells() -> Vec<(ParsedCell, ckbadger_store::LiveCellInfo)> {
        real_shaped_outputs()
            .into_iter()
            .map(|(output, data, udt_amount)| {
                let parsed = CellParser::parse_output(&output, &data).unwrap();
                let info = parsed.to_live_cell_info(udt_amount).unwrap();
                (parsed, info)
            })
            .collect()
    }

    /// The v1 payload exactly as the pre-2026-11-04 writer stored it.
    #[derive(serde::Serialize)]
    struct LiveCellInfoV1<'a> {
        capacity: i64,
        lock_script_hash: &'a Vec<u8>,
        lock_code_hash: &'a Vec<u8>,
        lock_hash_type: i16,
        lock_args: &'a Vec<u8>,
        type_script_hash: &'a Option<Vec<u8>>,
        type_code_hash: &'a Option<Vec<u8>>,
        type_hash_type: Option<i16>,
        type_args: &'a Option<Vec<u8>>,
        data_size: i32,
        occupied_capacity: i64,
        udt_amount: Option<u128>,
        data_hash: Option<Vec<u8>>,
    }

    #[test]
    fn cells_payload_v2_of_real_shaped_cells_averages_at_most_140_bytes() {
        let cells = real_shaped_cells();
        let v2: Vec<usize> = cells
            .iter()
            .map(|(_, info)| info.encode().unwrap().len())
            .collect();
        let v1: Vec<usize> = cells
            .iter()
            .map(|(parsed, info)| {
                bincode::serialize(&LiveCellInfoV1 {
                    capacity: parsed.capacity,
                    lock_script_hash: &parsed.lock_script_hash,
                    lock_code_hash: &parsed.lock_code_hash,
                    lock_hash_type: parsed.lock_hash_type,
                    lock_args: &parsed.lock_args,
                    type_script_hash: &parsed.type_script_hash,
                    type_code_hash: &parsed.type_code_hash,
                    type_hash_type: parsed.type_hash_type,
                    type_args: &parsed.type_args,
                    data_size: parsed.data_size,
                    occupied_capacity: info.occupied_capacity(),
                    udt_amount: info.udt_amount,
                    data_hash: Some(parsed.data_hash.to_vec()),
                })
                .unwrap()
                .len()
            })
            .collect();
        let (v2_total, v1_total) = (v2.iter().sum::<usize>(), v1.iter().sum::<usize>());
        assert!(
            v2_total <= 140 * v2.len() && v2_total * 100 <= v1_total * 65,
            "v2 average {:.1} B (limit 140 B, and <= 65 % of v1's {:.1} B); \
             per-cell v2 {v2:?}, v1 {v1:?}",
            v2_total as f64 / v2.len() as f64,
            v1_total as f64 / v1.len() as f64,
        );
    }

    fn ckb_types_script_hash(code_hash: &[u8; 32], hash_type: i16, args: &[u8]) -> [u8; 32] {
        use ckb_types::prelude::*;
        ckb_types::packed::Script::new_builder()
            .code_hash(code_hash.pack())
            .hash_type(ckb_types::packed::Byte::new(
                u8::try_from(hash_type).unwrap(),
            ))
            .args(args.to_vec().pack())
            .build()
            .calc_script_hash()
            .unpack()
    }

    /// The payload stops storing the script hashes; the derived ones must be
    /// the bytes v1 stored (the parser's hashes, which every index key that
    /// embeds a script hash is built from), and agree with ckb-types'
    /// independent encoder.
    #[test]
    fn cells_payload_v2_script_hashes_equal_what_the_v1_writer_stored() {
        for (index, (parsed, info)) in real_shaped_cells().iter().enumerate() {
            assert_eq!(
                info.lock_script_hash().to_vec(),
                parsed.lock_script_hash,
                "cell {index}: lock script hash"
            );
            assert_eq!(
                info.type_script_hash().map(|hash| hash.to_vec()),
                parsed.type_script_hash,
                "cell {index}: type script hash"
            );
            assert_eq!(
                info.lock_script_hash(),
                ckb_types_script_hash(&info.lock_code_hash, info.lock_hash_type, &info.lock_args),
                "cell {index}: lock script hash vs ckb-types"
            );
            if let Some(type_code_hash) = &info.type_code_hash {
                assert_eq!(
                    info.type_script_hash(),
                    Some(ckb_types_script_hash(
                        type_code_hash,
                        info.type_hash_type.unwrap(),
                        info.type_args.as_ref().unwrap(),
                    )),
                    "cell {index}: type script hash vs ckb-types"
                );
            }
        }
        assert_eq!(
            real_shaped_cells()
                .iter()
                .filter(|(_, info)| info.type_code_hash.is_some())
                .count(),
            3,
            "the fixture exercises sUDT and xUDT type scripts"
        );
    }

    /// The plain secp256k1 cell, the most common shape on chain, encodes in
    /// 101 bytes: capacity 9, lock code hash 32, hash type 1, args 1 + 20,
    /// three absent type-script fields 3, data size 1, absent UDT amount 1,
    /// data hash 1 + 32. No occupied capacity: it is derived.
    #[test]
    fn cells_payload_of_a_plain_secp_cell_is_101_bytes() {
        let cells = real_shaped_cells();
        let (parsed, info) = &cells[0];
        assert!(parsed.type_code_hash.is_none() && parsed.data_size == 0);
        assert_eq!(info.encode().unwrap().len(), 101);
    }

    /// The derived occupied capacity is CKB's own: ckb-types'
    /// `CellOutput::occupied_capacity` over the same output and data, for every
    /// fixture shape plus a type script with long args.
    #[test]
    fn derived_occupied_capacity_equals_ckb_types_for_every_cell_shape() {
        use ckb_types::{bytes::Bytes, core::Capacity, packed, prelude::*};

        let packed_script = |code_hash: &[u8; 32], hash_type: i16, args: &[u8]| {
            packed::Script::new_builder()
                .code_hash(code_hash.pack())
                .hash_type(packed::Byte::new(u8::try_from(hash_type).unwrap()))
                .args(Bytes::from(args.to_vec()).pack())
                .build()
        };
        let mut outputs = real_shaped_outputs();
        outputs.push((
            CellOutput {
                capacity: format!("{:#x}", 100_000_000_000u64),
                lock: rpc_script(SECP256K1_CODE_HASH, "type", &[0x0b; 20]),
                type_: Some(rpc_script(XUDT_CODE_HASH, "data1", &[0xa4; 300])),
            },
            format!("0x{}", "d4".repeat(50)),
            None,
        ));
        assert_eq!(outputs.len(), 11);
        for (index, (output, data, udt_amount)) in outputs.iter().enumerate() {
            let parsed = CellParser::parse_output(output, data).unwrap();
            let info = parsed.to_live_cell_info(*udt_amount).unwrap();
            let packed_output = packed::CellOutput::new_builder()
                .capacity(Capacity::shannons(u64::try_from(info.capacity).unwrap()).pack())
                .lock(packed_script(
                    &info.lock_code_hash,
                    info.lock_hash_type,
                    &info.lock_args,
                ))
                .type_(
                    info.type_code_hash
                        .as_ref()
                        .map(|code_hash| {
                            packed_script(
                                code_hash,
                                info.type_hash_type.unwrap(),
                                info.type_args.as_ref().unwrap(),
                            )
                        })
                        .pack(),
                )
                .build();
            let ckb = packed_output
                .occupied_capacity(Capacity::bytes(parsed.data.len()).unwrap())
                .unwrap()
                .as_u64();
            assert_eq!(
                u64::try_from(info.occupied_capacity()).unwrap(),
                ckb,
                "cell {index}: derived occupied capacity vs ckb-types"
            );
        }
    }
}
