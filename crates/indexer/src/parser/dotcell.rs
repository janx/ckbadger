//! `.cell` (DotCell) name cell parsing.
//!
//! Layout and semantics are spec §1.2–§1.5, verified byte-for-byte against
//! both networks on 2026-09-23. The data types live in `ckbadger_store::types`
//! so the identity entry, the parser and the API read path share one
//! vocabulary; this module owns the decoding.

use anyhow::{anyhow, bail, Result};
use ckb_hash::new_blake2b;
use ckbadger_store::types::derive_dotcell_id;
use std::ops::Range;

use super::cell::ParsedCell;
use super::registry::{ProtocolScript, PROTOCOL_REGISTRY};
use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

pub use ckbadger_store::types::{DotCellNameData, DotCellRecord};

/// The only layout version ever written on either network.
pub const DOTCELL_LAYOUT_VERSION: u8 = 3;
/// Fixed header length; the label runs from here to the end of the data.
pub const DOTCELL_HEADER_LEN: usize = 98;
pub const DOTCELL_ID_LEN: usize = 20;

// Byte ranges of the name cell header (spec §1.2). The ONE offset table:
// `parse_name_data` slices with these, and the API's cell-page segment view
// labels the same bytes with them, so the two can never disagree.
/// `[0..1]` layout version (u8).
pub const DOTCELL_LAYOUT_VERSION_RANGE: Range<usize> = 0..1;
/// `[1..33]` blake2b of the records payload.
pub const DOTCELL_RECORDS_HASH_RANGE: Range<usize> = 1..33;
/// `[33..53]` next id in the ordered ring (zero = end of ring).
pub const DOTCELL_NEXT_ID_RANGE: Range<usize> = 33..53;
/// `[53..58]` expiry, unix seconds, u40 little-endian.
pub const DOTCELL_EXPIRY_RANGE: Range<usize> = 53..58;
/// `[58..78]` owner = first 20 bytes of the owner's lock script hash.
pub const DOTCELL_OWNER_RANGE: Range<usize> = 58..78;
/// `[78..98]` manager = first 20 bytes of the manager's lock script hash.
pub const DOTCELL_MANAGER_RANGE: Range<usize> = 78..98;
/// `next == 0` marks the end of the uniqueness ring.
pub const DOTCELL_ZERO_ID: [u8; 20] = [0u8; 20];
/// Sale Lock args = `seller_lock_hash(32) ‖ price_shannons(u64 LE)`.
pub const DOTCELL_SALE_LOCK_ARGS_LEN: usize = 40;
/// The 30-day grace period the contract enforces after expiry, in seconds.
pub const DOTCELL_GRACE_SECONDS: u64 = 2_592_000;

pub struct DotCellParser;

impl DotCellParser {
    pub fn is_account_type_script(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotCellAccount)
    }

    pub fn is_account_lock(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotCellAccountLock)
    }

    pub fn is_sale_lock(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotCellSaleLock)
    }

    /// `blake2b(label)[..20]`. One implementation, in the store crate, because
    /// the API resolves `alice.cell` to an id with the same function.
    pub fn derive_id(label: &str) -> [u8; 20] {
        derive_dotcell_id(label)
    }

    /// Decode a name cell's data. Spec §1.2:
    ///
    /// ```text
    /// [ 0.. 1] layout version = 3
    /// [ 1..33] blake2b of the records payload
    /// [33..53] next id in the ordered ring (zero = end of ring)
    /// [53..58] expiry, unix seconds, u40 little-endian
    /// [58..78] owner   = first 20 bytes of the owner's lock script hash
    /// [78..98] manager = first 20 bytes of the manager's lock script hash
    /// [98..  ] label, UTF-8 ("" only on the ring root)
    /// ```
    ///
    /// Anything that does not match is an error, not a cell to skip: a name
    /// cell the indexer cannot decode is chain state it would silently lose.
    pub fn parse_name_data(data: &[u8]) -> Result<DotCellNameData> {
        if data.len() < DOTCELL_HEADER_LEN {
            bail!(
                "dotcell name data too short: len={} need>={}",
                data.len(),
                DOTCELL_HEADER_LEN
            );
        }
        if data[0] != DOTCELL_LAYOUT_VERSION {
            bail!(
                "dotcell layout version {} unsupported (expected {})",
                data[0],
                DOTCELL_LAYOUT_VERSION
            );
        }
        let label = std::str::from_utf8(&data[DOTCELL_HEADER_LEN..])
            .map_err(|e| anyhow!("dotcell label is not UTF-8: {e}"))?
            .to_string();
        let mut expiry = [0u8; 8];
        expiry[..DOTCELL_EXPIRY_RANGE.len()].copy_from_slice(&data[DOTCELL_EXPIRY_RANGE]);
        let id = Self::derive_id(&label);
        Ok(DotCellNameData {
            layout_version: data[DOTCELL_LAYOUT_VERSION_RANGE.start],
            records_hash: data[DOTCELL_RECORDS_HASH_RANGE]
                .try_into()
                .expect("32 bytes"),
            next_id: data[DOTCELL_NEXT_ID_RANGE].try_into().expect("20 bytes"),
            expired_at: u64::from_le_bytes(expiry),
            owner_hash20: data[DOTCELL_OWNER_RANGE].try_into().expect("20 bytes"),
            manager_hash20: data[DOTCELL_MANAGER_RANGE].try_into().expect("20 bytes"),
            label,
            id,
        })
    }

    /// blake2b of a records payload — the value a name cell stores in
    /// `data[1..33]`.
    pub fn records_hash(payload: &[u8]) -> [u8; 32] {
        let mut hasher = new_blake2b();
        hasher.update(payload);
        let mut out = [0u8; 32];
        hasher.finalize(&mut out);
        out
    }

    /// Decode a records payload. Spec §1.3:
    ///
    /// ```text
    /// u16 count, then per record:
    ///   u8 key_len, key | u8 label_len, label | u16 value_len, value | u32 ttl
    /// ```
    ///
    /// All little-endian, and consumed exactly: trailing bytes mean the
    /// payload is not what this decoder thinks it is.
    pub fn parse_records(payload: &[u8]) -> Result<Vec<DotCellRecord>> {
        fn take<'a>(payload: &'a [u8], pos: &mut usize, n: usize, what: &str) -> Result<&'a [u8]> {
            let end = pos
                .checked_add(n)
                .ok_or_else(|| anyhow!("dotcell records offset overflow while reading {what}"))?;
            if end > payload.len() {
                bail!(
                    "dotcell records payload truncated while reading {what}: need {end}, have {}",
                    payload.len()
                );
            }
            let slice = &payload[*pos..end];
            *pos = end;
            Ok(slice)
        }

        let mut pos = 0usize;
        let count = u16::from_le_bytes(
            take(payload, &mut pos, 2, "count")?
                .try_into()
                .expect("2 bytes"),
        );
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let key_len = take(payload, &mut pos, 1, "key_len")?[0] as usize;
            let key = std::str::from_utf8(take(payload, &mut pos, key_len, "key")?)
                .map_err(|e| anyhow!("dotcell record {i} key is not UTF-8: {e}"))?
                .to_string();
            let label_len = take(payload, &mut pos, 1, "label_len")?[0] as usize;
            let label = std::str::from_utf8(take(payload, &mut pos, label_len, "label")?)
                .map_err(|e| anyhow!("dotcell record {i} label is not UTF-8: {e}"))?
                .to_string();
            let value_len = u16::from_le_bytes(
                take(payload, &mut pos, 2, "value_len")?
                    .try_into()
                    .expect("2 bytes"),
            ) as usize;
            let value = take(payload, &mut pos, value_len, "value")?.to_vec();
            let ttl = u32::from_le_bytes(
                take(payload, &mut pos, 4, "ttl")?
                    .try_into()
                    .expect("4 bytes"),
            );
            out.push(DotCellRecord {
                key,
                label,
                value,
                ttl,
            });
        }
        if pos != payload.len() {
            bail!(
                "dotcell records payload has {} trailing bytes after {} records",
                payload.len() - pos,
                count
            );
        }
        Ok(out)
    }

    /// The `output_type` field of molecule
    /// `WitnessArgs { lock: BytesOpt, input_type: BytesOpt, output_type: BytesOpt }`.
    /// `Ok(None)` means the field is absent (the option is empty).
    pub fn witness_output_type(witness: &[u8]) -> Result<Option<&[u8]>> {
        if witness.is_empty() {
            return Ok(None);
        }
        if witness.len() < 16 {
            bail!("WitnessArgs too short: {} bytes", witness.len());
        }
        let total = u32::from_le_bytes(witness[0..4].try_into().expect("4 bytes")) as usize;
        if total != witness.len() {
            bail!(
                "WitnessArgs total {} != witness length {}",
                total,
                witness.len()
            );
        }
        let offset = |i: usize| {
            u32::from_le_bytes(witness[4 + 4 * i..8 + 4 * i].try_into().expect("4 bytes")) as usize
        };
        let (lock_off, input_type_off, output_type_off) = (offset(0), offset(1), offset(2));
        if lock_off != 16
            || input_type_off < lock_off
            || output_type_off < input_type_off
            || output_type_off > total
        {
            bail!(
                "WitnessArgs offsets malformed: total={total}, lock={lock_off}, input_type={input_type_off}, output_type={output_type_off}"
            );
        }
        let field = &witness[output_type_off..total];
        if field.is_empty() {
            return Ok(None);
        }
        if field.len() < 4 {
            bail!(
                "WitnessArgs output_type Bytes header truncated: {} bytes",
                field.len()
            );
        }
        let len = u32::from_le_bytes(field[0..4].try_into().expect("4 bytes")) as usize;
        if 4 + len != field.len() {
            bail!(
                "WitnessArgs output_type Bytes length {} != field payload {}",
                len,
                field.len() - 4
            );
        }
        Ok(Some(&field[4..]))
    }

    /// The records of a name cell, from the witness at that cell's own output
    /// index, verified against the hash the cell itself carries.
    pub fn parse_witness_records_bytes(
        witness: &[u8],
        expected_hash: &[u8; 32],
    ) -> Result<Vec<DotCellRecord>> {
        let payload = Self::witness_output_type(witness)?.ok_or_else(|| {
            anyhow!("dotcell name cell has no records payload in its witness output_type")
        })?;
        let actual = Self::records_hash(payload);
        if &actual != expected_hash {
            bail!(
                "dotcell records hash mismatch: cell says 0x{}, witness payload hashes to 0x{}",
                hex::encode(expected_hash),
                hex::encode(actual)
            );
        }
        Self::parse_records(payload)
    }

    pub fn parse_witness_records(
        witness_hex: &str,
        expected_hash: &[u8; 32],
    ) -> Result<Vec<DotCellRecord>> {
        Self::parse_witness_records_bytes(&parse_hex_to_bytes(witness_hex), expected_hash)
    }

    /// Sale Lock args: `seller_lock_hash(32) ‖ price_shannons(u64 LE)`.
    pub fn parse_sale_lock_args(args: &[u8]) -> Result<([u8; 32], u64)> {
        if args.len() != DOTCELL_SALE_LOCK_ARGS_LEN {
            bail!(
                "dotcell sale lock args must be {} bytes, got {}",
                DOTCELL_SALE_LOCK_ARGS_LEN,
                args.len()
            );
        }
        Ok((
            args[..32].try_into().expect("32 bytes"),
            u64::from_le_bytes(args[32..40].try_into().expect("8 bytes")),
        ))
    }

    /// Live path: a raw RPC output plus its data hex. `Ok(None)` means the
    /// cell is not a name cell; `Err` means it is one and its data is wrong.
    pub fn parse_name_cell(output: &CellOutput, data_hex: &str) -> Result<Option<DotCellNameData>> {
        let Some(type_script) = output.type_.as_ref() else {
            return Ok(None);
        };
        if !Self::is_account_type_script(&parse_hex_to_bytes(&type_script.code_hash)) {
            return Ok(None);
        }
        Self::parse_name_data(&parse_hex_to_bytes(data_hex)).map(Some)
    }

    /// Bulk path: an already-parsed cell. Same contract as `parse_name_cell`.
    pub fn parse_name_parsed_cell(cell: &ParsedCell) -> Result<Option<DotCellNameData>> {
        let Some(code_hash) = cell.type_code_hash.as_ref() else {
            return Ok(None);
        };
        if !Self::is_account_type_script(code_hash) {
            return Ok(None);
        }
        Self::parse_name_data(&cell.data).map(Some)
    }

    /// Every name cell a transaction creates, with its records decoded from
    /// the witness at that output's own index. A name output with no witness
    /// there is an error: the records payload is chain state, and the hash in
    /// the cell says it exists.
    ///
    /// Every error names `tx=0x…, output_index=N` in the same words as the
    /// bulk path's `parse_protocol_facts`, so one malformed cell is located
    /// identically whichever sync mode meets it.
    pub fn parse_name_cells_with_output_indices(
        tx: &TransactionView,
    ) -> Result<Vec<(usize, DotCellNameData, Vec<DotCellRecord>)>> {
        let mut out = Vec::new();
        for (index, (output, data_hex)) in tx.outputs.iter().zip(&tx.outputs_data).enumerate() {
            let Some(name) = Self::parse_name_cell(output, data_hex).map_err(|e| {
                anyhow!(
                    "dotcell name cell parse failed: tx={}, output_index={}: {e}",
                    tx.hash,
                    index
                )
            })?
            else {
                continue;
            };
            let witness = tx.witnesses.get(index).ok_or_else(|| {
                anyhow!(
                    "dotcell name cell has no witness at its output index: tx={}, output_index={} (witnesses={})",
                    tx.hash,
                    index,
                    tx.witnesses.len()
                )
            })?;
            let records =
                Self::parse_witness_records(witness, &name.records_hash).map_err(|e| {
                    anyhow!(
                        "dotcell records: tx={}, output_index={}: {e}",
                        tx.hash,
                        index
                    )
                })?;
            out.push((index, name, records));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::test_helpers::real_dotcell as fixture;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn account_type_script_is_recognised_on_both_networks() {
        for hex in [
            fixture::ACCOUNT_TYPE_CODE_HASH_MAINNET,
            fixture::ACCOUNT_TYPE_CODE_HASH_TESTNET,
        ] {
            assert!(
                DotCellParser::is_account_type_script(&parse_hex_to_bytes(hex)),
                "{hex} must be the Cells Account type script"
            );
        }
        for hex in [
            fixture::ACCOUNT_LOCK_CODE_HASH_MAINNET,
            fixture::SALE_LOCK_CODE_HASH_MAINNET,
            fixture::PRICE_TYPE_CODE_HASH_MAINNET,
        ] {
            assert!(
                !DotCellParser::is_account_type_script(&parse_hex_to_bytes(hex)),
                "{hex} is not the Account type script"
            );
        }
    }

    #[test]
    fn account_lock_and_sale_lock_are_distinct_on_both_networks() {
        for hex in [
            fixture::ACCOUNT_LOCK_CODE_HASH_MAINNET,
            fixture::ACCOUNT_LOCK_CODE_HASH_TESTNET,
        ] {
            let bytes = parse_hex_to_bytes(hex);
            assert!(DotCellParser::is_account_lock(&bytes), "{hex} account lock");
            assert!(!DotCellParser::is_sale_lock(&bytes));
        }
        for hex in [
            fixture::SALE_LOCK_CODE_HASH_MAINNET,
            fixture::SALE_LOCK_CODE_HASH_TESTNET,
        ] {
            let bytes = parse_hex_to_bytes(hex);
            assert!(DotCellParser::is_sale_lock(&bytes), "{hex} sale lock");
            assert!(!DotCellParser::is_account_lock(&bytes));
        }
    }

    /// The published field ranges tile the fixed header exactly, in order,
    /// with each field its declared width — the API labels segments with
    /// them, so a gap or overlap would mislabel chain bytes.
    #[test]
    fn header_ranges_tile_the_fixed_header() {
        let ranges = [
            DOTCELL_LAYOUT_VERSION_RANGE,
            DOTCELL_RECORDS_HASH_RANGE,
            DOTCELL_NEXT_ID_RANGE,
            DOTCELL_EXPIRY_RANGE,
            DOTCELL_OWNER_RANGE,
            DOTCELL_MANAGER_RANGE,
        ];
        let mut cursor = 0;
        for range in &ranges {
            assert_eq!(
                range.start, cursor,
                "{range:?} must start where the previous ended"
            );
            cursor = range.end;
        }
        assert_eq!(cursor, DOTCELL_HEADER_LEN);
        assert_eq!(
            ranges.iter().map(|r| r.len()).collect::<Vec<_>>(),
            vec![1, 32, DOTCELL_ID_LEN, 5, DOTCELL_ID_LEN, DOTCELL_ID_LEN]
        );
    }

    #[test]
    fn parse_name_data_decodes_real_mainnet_support_cell() {
        let d = DotCellParser::parse_name_data(&parse_hex_to_bytes(fixture::M2_OUT1_DATA)).unwrap();
        assert_eq!(d.layout_version, 3);
        assert_eq!(d.label, "support");
        assert_eq!(
            d.id.to_vec(),
            parse_hex_to_bytes("0x62d71147ac82b83c8531126cacb0d2f072bfd94a")
        );
        assert_eq!(
            d.next_id.to_vec(),
            parse_hex_to_bytes("0x65b5fe7e7070b506f69bd8cabf9e427211106645")
        );
        assert_eq!(d.expired_at, 1_821_507_678);
        assert_eq!(
            d.owner_hash20.to_vec(),
            parse_hex_to_bytes("0x57d926a44d83fc13b21ce037b1e31f4223e3c867")
        );
        assert_eq!(d.manager_hash20, d.owner_hash20);
        assert_eq!(
            d.records_hash.to_vec(),
            parse_hex_to_bytes(
                "0x72ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2"
            )
        );
        assert!(!d.is_root());
        assert_eq!(d.parent_id(), None);
        // `support` was registered with no records, and its own-index witness
        // says so: an empty payload that still hashes to the cell's value.
        assert!(
            DotCellParser::parse_witness_records(fixture::M2_WITNESS_1, &d.records_hash)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ring_root_parses_as_root() {
        let d = DotCellParser::parse_name_data(&parse_hex_to_bytes(fixture::M1_OUT0_DATA)).unwrap();
        assert!(d.is_root());
        assert_eq!(d.expired_at, 0);
        assert_eq!(d.owner_hash20, [0u8; 20]);
        assert_eq!(d.manager_hash20, [0u8; 20]);
        assert_eq!(d.next_id, DOTCELL_ZERO_ID);
        assert_eq!(
            d.id.to_vec(),
            parse_hex_to_bytes("0x44f4c69744d5f8c55d642062949dcae49bc4e7ef")
        );
    }

    #[test]
    fn subname_parent_id_is_hash_of_label_after_first_dot() {
        let d = DotCellParser::parse_name_data(&parse_hex_to_bytes(fixture::T3_OUT1_DATA)).unwrap();
        assert_eq!(d.label, "shop.v3-first-name");
        assert_eq!(
            d.parent_id().unwrap().to_vec(),
            parse_hex_to_bytes("0x4144e782dfaadeeb07625e11e4b6de717893aacb")
        );
    }

    #[test]
    fn parse_records_decodes_maria_six_records_and_hash_matches() {
        let data = parse_hex_to_bytes(fixture::T2_OUT0_DATA);
        let name = DotCellParser::parse_name_data(&data).unwrap();
        assert_eq!(name.label, "maria");
        let records =
            DotCellParser::parse_witness_records(fixture::T2_WITNESS_0, &name.records_hash)
                .unwrap();
        assert_eq!(records.len(), 6);
        assert_eq!(
            records[0],
            DotCellRecord {
                key: "address.309".into(),
                label: String::new(),
                value: b"ckt1qrfrwcdnvssswdwpn3s9v8fp87emat306ctjwsm3nmlkjg8qyza2cqgqq9x75zu4l7gld606r6eyd00m4lzy3zkxkq4nywzu".to_vec(),
                ttl: 300,
            }
        );
        assert_eq!(
            records.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            vec![
                "address.309",
                "address.0",
                "address.60",
                "profile.email",
                "profile.phone",
                "dweb.ckbfs",
            ]
        );
        assert!(records.iter().all(|r| r.ttl == 300 && r.label.is_empty()));
    }

    #[test]
    fn parse_witness_records_rejects_hash_mismatch() {
        let bad = [0xAAu8; 32];
        let err = DotCellParser::parse_witness_records(fixture::T2_WITNESS_0, &bad).unwrap_err();
        assert!(err.to_string().contains("records hash mismatch"), "{err}");
    }

    #[test]
    fn parse_records_rejects_trailing_and_truncated_payloads() {
        // count=1, key "k", empty label, value "v", ttl 300 — exactly consumed.
        let mut payload = vec![
            0x01, 0x00, 0x01, b'k', 0x00, 0x01, 0x00, b'v', 0x2c, 0x01, 0x00, 0x00,
        ];
        let parsed = DotCellParser::parse_records(&payload).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].ttl, 300);
        assert_eq!(parsed[0].value, b"v".to_vec());

        payload.push(0x00);
        assert!(DotCellParser::parse_records(&payload)
            .unwrap_err()
            .to_string()
            .contains("trailing"));
        assert!(DotCellParser::parse_records(&[0x02, 0x00, 0x01, b'k'])
            .unwrap_err()
            .to_string()
            .contains("truncated"));
    }

    #[test]
    fn parse_name_data_rejects_other_layout_versions_and_short_data() {
        let mut d = parse_hex_to_bytes(fixture::M2_OUT1_DATA);
        d[0] = 4;
        assert!(DotCellParser::parse_name_data(&d)
            .unwrap_err()
            .to_string()
            .contains("layout version 4"));
        assert!(DotCellParser::parse_name_data(&[3u8; 97])
            .unwrap_err()
            .to_string()
            .contains("too short"));
        let mut bad_utf8 = parse_hex_to_bytes(fixture::M2_OUT1_DATA);
        bad_utf8.push(0xFF);
        assert!(DotCellParser::parse_name_data(&bad_utf8)
            .unwrap_err()
            .to_string()
            .contains("UTF-8"));
    }

    #[test]
    fn parse_sale_lock_args_reads_seller_and_price_and_hash20_equals_listed_owner() {
        let (seller, price) =
            DotCellParser::parse_sale_lock_args(&parse_hex_to_bytes(fixture::T6_SALE_LOCK_ARGS))
                .unwrap();
        assert_eq!(
            seller.to_vec(),
            parse_hex_to_bytes(
                "0x9d602bfc26415da790c79526703cbbc1e9267cbe4be38f8f98b769ad0a90e1a1"
            )
        );
        assert_eq!(price, 10_000_000_000);
        assert!(DotCellParser::parse_sale_lock_args(&[0u8; 39]).is_err());

        // "For sale" is a property of the NAME: the listed name's owner20 is
        // the first 20 bytes of the Sale Lock instance's own script hash.
        let lock_hash =
            crate::parser::ScriptParser::compute_script_hash(&fixture::t6_sale_lock_script());
        assert_eq!(
            &lock_hash[..20],
            &parse_hex_to_bytes("0xcb736f437a28b77ecb038cc147c2171ed83fc371")[..]
        );
        let listed = DotCellParser::parse_name_data(&parse_hex_to_bytes(
            fixture::T6_LIST_CARTAOPROVA.outputs[0].data,
        ))
        .unwrap();
        assert_eq!(&listed.owner_hash20[..], &lock_hash[..20]);
    }

    /// PROTO-007: the live entry point and the bulk entry point must decode the
    /// same real cell to the same value, or a from-genesis rebuild and an
    /// incremental sync disagree about the chain.
    #[test]
    fn live_and_bulk_name_parsing_agree_on_real_cells() {
        for (output, data_hex) in [fixture::m2_out1(), fixture::t2_out0(), fixture::m1_out0()] {
            let live = DotCellParser::parse_name_cell(&output, data_hex)
                .unwrap()
                .expect("dotcell typed");
            let parsed = crate::parser::CellParser::parse_output(&output, data_hex).unwrap();
            let bulk = DotCellParser::parse_name_parsed_cell(&parsed)
                .unwrap()
                .expect("dotcell typed");
            assert_eq!(live, bulk);
        }
    }

    #[test]
    fn non_dotcell_cells_parse_to_none() {
        let (output, data) = crate::parser::test_helpers::real_did_ckb::cell_32();
        assert!(DotCellParser::parse_name_cell(&output, data)
            .unwrap()
            .is_none());
        let parsed = crate::parser::CellParser::parse_output(&output, data).unwrap();
        assert!(DotCellParser::parse_name_parsed_cell(&parsed)
            .unwrap()
            .is_none());
    }

    #[test]
    fn live_output_scan_pairs_every_name_with_its_own_index_witness() {
        let tx = fixture::T2_REGISTER_JOAOM.transaction();
        let found = DotCellParser::parse_name_cells_with_output_indices(&tx).unwrap();
        assert_eq!(found.len(), 2, "both name outputs are returned");
        assert_eq!(found[0].0, 0);
        assert_eq!(found[0].1.label, "maria");
        assert_eq!(found[0].2.len(), 6, "maria's records come from witness 0");
        assert_eq!(found[1].0, 1);
        assert_eq!(found[1].1.label, "joaom");
        assert!(
            found[1].2.is_empty(),
            "joaom was registered with no records"
        );
    }

    #[test]
    fn live_output_scan_fails_when_the_name_output_has_no_witness() {
        let mut tx = fixture::T2_REGISTER_JOAOM.transaction();
        tx.witnesses.truncate(1);
        let err = DotCellParser::parse_name_cells_with_output_indices(&tx).unwrap_err();
        assert!(err.to_string().contains("witness"), "{err}");
    }

    /// Every live-side failure names the cell the way bulk's
    /// `parse_protocol_facts` does: `tx=0x…, output_index=N`.
    #[test]
    fn live_output_scan_locates_every_failure_like_bulk() {
        let base = fixture::T2_REGISTER_JOAOM.transaction();
        let locator = format!("tx={}, output_index=1", base.hash);

        let mut no_witness = base.clone();
        no_witness.witnesses.truncate(1);
        let err = DotCellParser::parse_name_cells_with_output_indices(&no_witness).unwrap_err();
        assert!(
            err.to_string().starts_with(&format!(
                "dotcell name cell has no witness at its output index: {locator}"
            )),
            "{err}"
        );

        let mut bad_layout = base.clone();
        bad_layout.outputs_data[1] = format!("0x04{}", &bad_layout.outputs_data[1][4..]);
        let err = DotCellParser::parse_name_cells_with_output_indices(&bad_layout).unwrap_err();
        assert!(
            err.to_string()
                .starts_with(&format!("dotcell name cell parse failed: {locator}: ")),
            "{err}"
        );
        assert!(err.to_string().contains("layout version 4"), "{err}");

        let mut wrong_records = base;
        wrong_records.witnesses.swap(0, 1);
        let err = DotCellParser::parse_name_cells_with_output_indices(&wrong_records).unwrap_err();
        assert!(
            err.to_string().starts_with("dotcell records: tx=0x"),
            "{err}"
        );
        assert!(err.to_string().contains("output_index=0"), "{err}");
    }

    #[test]
    fn derive_id_matches_protocol_vectors() {
        for (label, id) in [
            ("", "0x44f4c69744d5f8c55d642062949dcae49bc4e7ef"),
            ("support", "0x62d71147ac82b83c8531126cacb0d2f072bfd94a"),
            ("cellula", "0x629e27043d21c27bf12624ad840ac1ce848b3424"),
            ("maria", "0x2224948f63975a7a0741139cd5d2a45b9fb02c03"),
            ("joaom", "0x241e3586a41eb75dd6d68bf555acea74d6649ed5"),
            ("apt", "0x37561deee27ed512016aa4fd60418487fec9f944"),
            ("abuse", "0xa8d5f7507b9f3d30090253a741c1c80cb0cb121c"),
            (
                "v3-first-name",
                "0x4144e782dfaadeeb07625e11e4b6de717893aacb",
            ),
            (
                "shop.v3-first-name",
                "0xbb008a3e9045554d5b1b609c072b59b404320f9f",
            ),
            (
                "cartaoprova3695",
                "0xac485eebad1a642cc759559e02782791ad3ae89c",
            ),
            ("ref43euew", "0xeb3945173ac92a68c3158acfda30d9f19e609015"),
        ] {
            assert_eq!(
                DotCellParser::derive_id(label).to_vec(),
                parse_hex_to_bytes(id),
                "derive_id({label:?})"
            );
        }
    }
}
