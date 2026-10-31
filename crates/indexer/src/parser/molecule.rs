//! Molecule decoding for every parser: one reading per molecule type.
//!
//! Each reader returns `None` when the bytes are not that molecule type (a
//! short header, a size that does not match, an offset out of range). The
//! callers decode untrusted chain data — witnesses of every shape, type args
//! a protocol may or may not use — where "not this shape" is an expected
//! answer, not an indexer invariant violation.

/// A molecule `Number` / header word: 4 bytes, little-endian.
pub(crate) fn parse_u32(data: &[u8]) -> Option<usize> {
    let raw: [u8; 4] = data.try_into().ok()?;
    Some(u32::from_le_bytes(raw) as usize)
}

/// A molecule `Bytes` (`fixvec<byte>`): a 4-byte item count — the content
/// length — then exactly that many bytes. Returns the content.
pub(crate) fn parse_bytes(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 4 {
        return None;
    }
    let item_count = parse_u32(&data[0..4])?;
    if item_count.checked_add(4)? != data.len() {
        return None;
    }
    Some(&data[4..])
}

/// Every field of one molecule `table`: total size, then one offset per field
/// (the first offset fixes the field count), then the fields.
fn parse_table_fields(data: &[u8]) -> Option<Vec<&[u8]>> {
    if data.len() < 8 {
        return None;
    }
    let total_size = parse_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }
    let first_offset = parse_u32(&data[4..8])?;
    if first_offset < 8 || first_offset > total_size || first_offset % 4 != 0 {
        return None;
    }
    let field_count = first_offset / 4 - 1;
    let mut offsets = Vec::with_capacity(field_count + 1);
    for idx in 0..field_count {
        let start = 4 + idx * 4;
        offsets.push(parse_u32(&data[start..start + 4])?);
    }
    offsets.push(total_size);
    for pair in offsets.windows(2) {
        if pair[0] > pair[1] || pair[1] > total_size {
            return None;
        }
    }
    Some(
        offsets
            .windows(2)
            .map(|pair| &data[pair[0]..pair[1]])
            .collect(),
    )
}

/// A `table` with exactly `field_count` fields.
pub(crate) fn parse_table_fields_exact(data: &[u8], field_count: usize) -> Option<Vec<&[u8]>> {
    parse_table_fields(data).filter(|fields| fields.len() == field_count)
}

/// A `table` with at least `min_field_count` fields (a newer schema version
/// may append fields; readers of the first ones still decode it).
pub(crate) fn parse_table_fields_at_least(
    data: &[u8],
    min_field_count: usize,
) -> Option<Vec<&[u8]>> {
    parse_table_fields(data).filter(|fields| fields.len() >= min_field_count)
}

/// The items of a molecule `dynvec`: total size, one offset per item, then the
/// items. An empty dynvec is its 4-byte total size alone.
pub(crate) fn parse_dynvec_items(data: &[u8]) -> Option<Vec<&[u8]>> {
    if data.len() < 4 {
        return None;
    }
    let total_size = parse_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }
    if total_size == 4 {
        return Some(Vec::new());
    }
    if data.len() < 8 {
        return None;
    }
    let first_offset = parse_u32(&data[4..8])?;
    if first_offset < 8 || first_offset > total_size || first_offset % 4 != 0 {
        return None;
    }
    let item_count = first_offset / 4 - 1;
    let mut offsets = Vec::with_capacity(item_count + 1);
    for idx in 0..item_count {
        let start = 4 + idx * 4;
        offsets.push(parse_u32(&data[start..start + 4])?);
    }
    offsets.push(total_size);
    for pair in offsets.windows(2) {
        if pair[0] > pair[1] || pair[1] > total_size {
            return None;
        }
    }
    Some(
        offsets
            .windows(2)
            .map(|pair| &data[pair[0]..pair[1]])
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_types::{bytes::Bytes, packed, prelude::*};

    /// The node's own serialization is the reference: a `Bytes` header is the
    /// content length (POSTMORTEM-class bug: a copy read it as the total size
    /// and never decoded a real xUDT extension script).
    #[test]
    fn bytes_reads_the_node_serialization() {
        let content = [0x5a_u8; 20];
        let encoded = Bytes::copy_from_slice(&content).pack();
        assert_eq!(&encoded.as_slice()[..4], &20u32.to_le_bytes());
        assert_eq!(parse_bytes(encoded.as_slice()), Some(&content[..]));

        let empty = Bytes::new().pack();
        assert_eq!(parse_bytes(empty.as_slice()), Some(&[][..]));
    }

    #[test]
    fn bytes_rejects_a_header_that_does_not_match_the_content() {
        let mut total_size_header = 24u32.to_le_bytes().to_vec();
        total_size_header.extend_from_slice(&[0x5a; 20]);
        assert_eq!(parse_bytes(&total_size_header), None);
        assert_eq!(parse_bytes(&[0x01, 0x00, 0x00]), None);
        assert_eq!(parse_bytes(&u32::MAX.to_le_bytes()), None);
    }

    #[test]
    fn table_fields_read_a_node_script() {
        let script = packed::Script::new_builder()
            .code_hash(packed::Byte32::new([0xcc; 32]))
            .hash_type(packed::Byte::new(1))
            .args(Bytes::copy_from_slice(&[0x9a; 20]).pack())
            .build();
        let fields = parse_table_fields_exact(script.as_slice(), 3).unwrap();
        assert_eq!(fields[0], &[0xcc; 32][..]);
        assert_eq!(fields[1], &[1][..]);
        assert_eq!(parse_bytes(fields[2]), Some(&[0x9a; 20][..]));
        assert!(parse_table_fields_exact(script.as_slice(), 4).is_none());
        assert!(parse_table_fields_at_least(script.as_slice(), 2).is_some());
        assert!(parse_table_fields_at_least(script.as_slice(), 4).is_none());
    }

    #[test]
    fn dynvec_reads_node_bytes_vec() {
        let vec = packed::BytesVec::new_builder()
            .push(Bytes::from_static(b"ab").pack())
            .push(Bytes::from_static(b"cde").pack())
            .build();
        let items = parse_dynvec_items(vec.as_slice()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(parse_bytes(items[0]), Some(&b"ab"[..]));
        assert_eq!(parse_bytes(items[1]), Some(&b"cde"[..]));
        let empty = packed::BytesVec::new_builder().build();
        assert_eq!(parse_dynvec_items(empty.as_slice()), Some(Vec::new()));
    }
}
