//! ELF64 file layout for code-cell data. File offsets and virtual addresses
//! are different coordinate systems; NOBITS sections occupy no file bytes.
use anyhow::{ensure, Context, Result};
use serde::Serialize;

fn u64_string<S: serde::Serializer>(
    value: &u64,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}
fn optional_u64_string<S: serde::Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value {
        Some(value) => serializer.serialize_some(&value.to_string()),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElfHeader {
    pub class: String,
    pub endian: String,
    pub machine: u16,
    #[serde(serialize_with = "u64_string")]
    pub entry: u64,
    #[serde(serialize_with = "u64_string")]
    pub program_header_offset: u64,
    #[serde(serialize_with = "u64_string")]
    pub section_header_offset: u64,
    pub flags: u32,
    pub header_size: u16,
    pub program_header_entry_size: u16,
    pub program_header_count: u16,
    pub section_header_entry_size: u16,
    pub section_header_count: u16,
    pub section_name_index: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElfProgramHeader {
    pub index: usize,
    pub kind: u32,
    pub flags: u32,
    #[serde(serialize_with = "u64_string")]
    pub file_offset: u64,
    #[serde(serialize_with = "u64_string")]
    pub file_size: u64,
    #[serde(serialize_with = "u64_string")]
    pub virtual_address: u64,
    #[serde(serialize_with = "u64_string")]
    pub memory_size: u64,
    #[serde(serialize_with = "u64_string")]
    pub alignment: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElfSection {
    pub index: usize,
    pub name: String,
    pub kind: u32,
    #[serde(serialize_with = "u64_string")]
    pub flags: u64,
    #[serde(serialize_with = "u64_string")]
    pub address: u64,
    /// Null for SHT_NOBITS: this section has a memory extent, not file bytes.
    #[serde(serialize_with = "optional_u64_string")]
    pub file_offset: Option<u64>,
    #[serde(serialize_with = "optional_u64_string")]
    pub file_size: Option<u64>,
    #[serde(serialize_with = "u64_string")]
    pub memory_size: u64,
    #[serde(serialize_with = "u64_string")]
    pub alignment: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElfFileGap {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElfAnalysis {
    pub header: ElfHeader,
    pub program_headers: Vec<ElfProgramHeader>,
    pub sections: Vec<ElfSection>,
    /// Bytes outside declared headers and file-backed ranges. Their meaning
    /// is unspecified; they are not necessarily padding.
    pub unassigned_file_ranges: Vec<ElfFileGap>,
    pub comment: Option<String>,
    pub riscv_attributes: Option<String>,
}

/// Decode the RISC-V psABI format-A attribute framing; only Tag_RISCV_arch
/// (5) is presented as an ISA string. Other tags are skipped by their
/// specified odd-string/even-integer encoding, never searched for text.
fn riscv_arch_attribute(payload: &[u8]) -> Result<String> {
    ensure!(
        payload.first() == Some(&b'A'),
        "RISC-V attributes missing format A marker"
    );
    let mut at = 1usize;
    let mut arch = None;
    let mut saw_riscv_vendor = false;
    while at < payload.len() {
        let start = at;
        let length = u32_at(payload, at, "attribute vendor length")? as usize;
        ensure!(length >= 5, "RISC-V attribute vendor subsection too short");
        let end = start
            .checked_add(length)
            .context("attribute vendor length overflow")?;
        ensure!(
            end <= payload.len(),
            "RISC-V attribute vendor subsection truncated"
        );
        at += 4;
        let vendor = attribute_ntbs(payload, &mut at, end, "vendor")?;
        if vendor == "riscv" {
            saw_riscv_vendor = true;
        }
        while at < end {
            let subsection_start = at;
            let subsection_tag = attribute_uleb(payload, &mut at, end)?;
            let subsection_length = u32_at(payload, at, "attribute file length")? as usize;
            at += 4;
            let subsection_end = subsection_start
                .checked_add(subsection_length)
                .context("attribute file length overflow")?;
            ensure!(
                subsection_end <= end && subsection_end >= at,
                "RISC-V attribute file subsection truncated"
            );
            if vendor == "riscv" && subsection_tag == 1 {
                while at < subsection_end {
                    let tag = attribute_uleb(payload, &mut at, subsection_end)?;
                    if tag % 2 == 1 {
                        let value =
                            attribute_ntbs(payload, &mut at, subsection_end, "string value")?;
                        if tag == 5 {
                            ensure!(arch.is_none(), "duplicate Tag_RISCV_arch");
                            arch = Some(value);
                        }
                    } else {
                        attribute_uleb(payload, &mut at, subsection_end)?;
                    }
                }
            }
            at = subsection_end;
        }
        at = end;
    }
    ensure!(
        saw_riscv_vendor,
        "RISC-V attributes missing riscv vendor subsection"
    );
    Ok(arch.unwrap_or_else(|| "Tag_RISCV_arch absent".into()))
}
fn attribute_ntbs(data: &[u8], at: &mut usize, end: usize, label: &str) -> Result<String> {
    let tail = data
        .get(*at..end)
        .context("attribute string exceeds subsection")?;
    let length = tail
        .iter()
        .position(|&byte| byte == 0)
        .with_context(|| format!("RISC-V attribute {label} lacks terminator"))?;
    let value = String::from_utf8(tail[..length].to_vec())
        .with_context(|| format!("RISC-V attribute {label} is not UTF-8"))?;
    *at += length + 1;
    Ok(value)
}
fn attribute_uleb(data: &[u8], at: &mut usize, end: usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..=63).step_by(7) {
        let byte = *data
            .get(*at)
            .filter(|_| *at < end)
            .context("RISC-V attribute ULEB truncated")?;
        *at += 1;
        ensure!(shift != 63 || byte <= 1, "RISC-V attribute ULEB overflow");
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("RISC-V attribute ULEB overflow")
}

fn slice<'a>(data: &'a [u8], offset: usize, size: usize, label: &str) -> Result<&'a [u8]> {
    let end = offset.checked_add(size).context("ELF offset overflow")?;
    data.get(offset..end).with_context(|| {
        format!(
            "ELF {label} outside file: offset={offset}, size={size}, file={}",
            data.len()
        )
    })
}
fn u16_at(data: &[u8], offset: usize, label: &str) -> Result<u16> {
    Ok(u16::from_le_bytes(
        slice(data, offset, 2, label)?.try_into()?,
    ))
}
fn u32_at(data: &[u8], offset: usize, label: &str) -> Result<u32> {
    Ok(u32::from_le_bytes(
        slice(data, offset, 4, label)?.try_into()?,
    ))
}
fn u64_at(data: &[u8], offset: usize, label: &str) -> Result<u64> {
    Ok(u64::from_le_bytes(
        slice(data, offset, 8, label)?.try_into()?,
    ))
}
fn as_usize(value: u64, label: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("ELF {label} exceeds host address space"))
}
fn file_range(data: &[u8], offset: u64, size: u64, label: &str) -> Result<(usize, usize)> {
    let offset = as_usize(offset, label)?;
    let size = as_usize(size, label)?;
    slice(data, offset, size, label)?;
    Ok((offset, offset + size))
}

pub fn parse(data: &[u8]) -> Result<Option<ElfAnalysis>> {
    if !data.starts_with(b"\x7fELF") {
        return Ok(None);
    }
    ensure!(
        data.len() >= 64,
        "ELF header truncated: {} bytes",
        data.len()
    );
    ensure!(
        data[4] == 2,
        "unsupported ELF class {} (expected ELF64)",
        data[4]
    );
    ensure!(
        data[5] == 1,
        "unsupported ELF endian {} (expected little)",
        data[5]
    );
    ensure!(
        data[6] == 1,
        "invalid ELF identification version {}",
        data[6]
    );
    let header = ElfHeader {
        class: "ELF64".into(),
        endian: "little".into(),
        machine: u16_at(data, 18, "machine")?,
        entry: u64_at(data, 24, "entry")?,
        program_header_offset: u64_at(data, 32, "program header offset")?,
        section_header_offset: u64_at(data, 40, "section header offset")?,
        flags: u32_at(data, 48, "flags")?,
        header_size: u16_at(data, 52, "header size")?,
        program_header_entry_size: u16_at(data, 54, "program header size")?,
        program_header_count: u16_at(data, 56, "program header count")?,
        section_header_entry_size: u16_at(data, 58, "section header size")?,
        section_header_count: u16_at(data, 60, "section header count")?,
        section_name_index: u16_at(data, 62, "section name index")?,
    };
    ensure!(
        header.machine == 243,
        "ELF machine {} is not RISC-V",
        header.machine
    );
    ensure!(
        header.header_size == 64,
        "invalid ELF64 header size {}",
        header.header_size
    );
    ensure!(
        header.program_header_count == 0 || header.program_header_entry_size == 56,
        "invalid ELF64 program header entry size {}",
        header.program_header_entry_size
    );
    ensure!(
        header.section_header_count == 0 || header.section_header_entry_size == 64,
        "invalid ELF64 section header entry size {}",
        header.section_header_entry_size
    );
    ensure!(
        header.section_header_count == 0 || header.section_name_index < header.section_header_count,
        "invalid section name index {} for {} sections",
        header.section_name_index,
        header.section_header_count
    );
    let mut occupied_ranges = vec![(0usize, 64usize)];
    let mut program_headers = Vec::new();
    if header.program_header_count > 0 {
        let offset = as_usize(header.program_header_offset, "program header table")?;
        let size = usize::from(header.program_header_count) * 56;
        slice(data, offset, size, "program header table")?;
        occupied_ranges.push((offset, offset + size));
        for index in 0..usize::from(header.program_header_count) {
            let at = offset + index * 56;
            let item = ElfProgramHeader {
                index,
                kind: u32_at(data, at, "program kind")?,
                flags: u32_at(data, at + 4, "program flags")?,
                file_offset: u64_at(data, at + 8, "program file offset")?,
                virtual_address: u64_at(data, at + 16, "program virtual address")?,
                file_size: u64_at(data, at + 32, "program file size")?,
                memory_size: u64_at(data, at + 40, "program memory size")?,
                alignment: u64_at(data, at + 48, "program alignment")?,
            };
            if item.file_size > 0 {
                occupied_ranges.push(file_range(
                    data,
                    item.file_offset,
                    item.file_size,
                    "program segment",
                )?);
            }
            ensure!(
                item.file_size <= item.memory_size || item.kind != 1,
                "loadable program header {index} has file size larger than memory size"
            );
            program_headers.push(item);
        }
    }
    let mut raw_sections = Vec::new();
    let mut section_file_ranges = Vec::new();
    if header.section_header_count > 0 {
        let offset = as_usize(header.section_header_offset, "section header table")?;
        let size = usize::from(header.section_header_count) * 64;
        slice(data, offset, size, "section header table")?;
        occupied_ranges.push((offset, offset + size));
        for index in 0..usize::from(header.section_header_count) {
            let at = offset + index * 64;
            let name_offset = u32_at(data, at, "section name offset")?;
            let kind = u32_at(data, at + 4, "section kind")?;
            let flags = u64_at(data, at + 8, "section flags")?;
            let address = u64_at(data, at + 16, "section address")?;
            let file_offset = u64_at(data, at + 24, "section file offset")?;
            let size = u64_at(data, at + 32, "section size")?;
            let alignment = u64_at(data, at + 48, "section alignment")?;
            if kind != 8 && size > 0 {
                let range = file_range(data, file_offset, size, "section data")?;
                section_file_ranges.push(range);
                occupied_ranges.push(range);
            }
            raw_sections.push((
                name_offset,
                kind,
                flags,
                address,
                file_offset,
                size,
                alignment,
            ));
        }
    }
    section_file_ranges.sort_unstable();
    ensure!(
        section_file_ranges
            .windows(2)
            .all(|pair| pair[0].1 <= pair[1].0),
        "ELF file-backed sections overlap"
    );
    let names = if raw_sections.is_empty() || header.section_name_index == 0 {
        &[][..]
    } else {
        let name_section = raw_sections[usize::from(header.section_name_index)];
        ensure!(name_section.1 != 8, "section name table cannot be NOBITS");
        let (start, end) = file_range(data, name_section.4, name_section.5, "section name table")?;
        &data[start..end]
    };
    let mut comment = None;
    let mut riscv_attributes = None;
    let mut sections = Vec::new();
    for (index, (name_offset, kind, flags, address, file_offset, size, alignment)) in
        raw_sections.into_iter().enumerate()
    {
        let name = if names.is_empty() {
            format!("section {index} (names unavailable)")
        } else {
            let start = usize::try_from(name_offset)?;
            let tail = names
                .get(start..)
                .with_context(|| format!("section {index} name offset outside name table"))?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .with_context(|| format!("section {index} name lacks terminator"))?;
            String::from_utf8(tail[..end].to_vec())
                .with_context(|| format!("section {index} name is not UTF-8"))?
        };
        if name == ".comment" || name == ".riscv.attributes" {
            ensure!(kind != 8, "{name} has no file bytes");
            let (start, end) = file_range(data, file_offset, size, &name)?;
            let value = if name == ".riscv.attributes" {
                riscv_arch_attribute(&data[start..end])?
            } else {
                data[start..end]
                    .split(|&byte| byte == 0)
                    .filter(|piece| !piece.is_empty())
                    .map(String::from_utf8_lossy)
                    .map(|piece| piece.into_owned())
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            if name == ".comment" {
                comment = Some(value);
            } else {
                riscv_attributes = Some(value);
            }
        }
        sections.push(ElfSection {
            index,
            name,
            kind,
            flags,
            address,
            file_offset: (kind != 8).then_some(file_offset),
            file_size: (kind != 8).then_some(size),
            memory_size: size,
            alignment,
        });
    }
    occupied_ranges.sort_unstable();
    let mut covered = 0usize;
    let mut unassigned_file_ranges = Vec::new();
    for (start, end) in occupied_ranges {
        if start > covered {
            unassigned_file_ranges.push(ElfFileGap {
                start: covered,
                end: start,
            });
        }
        covered = covered.max(end);
    }
    if covered < data.len() {
        unassigned_file_ranges.push(ElfFileGap {
            start: covered,
            end: data.len(),
        });
    }
    Ok(Some(ElfAnalysis {
        header,
        program_headers,
        sections,
        unassigned_file_ranges,
        comment,
        riscv_attributes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        // ELF64 RISC-V with one load segment, one file-backed section, one
        // NOBITS section, and an overlapping segment/section file range.
        let mut data = vec![0; 320];
        data[0..4].copy_from_slice(b"\x7fELF");
        data[4..7].copy_from_slice(&[2, 1, 1]);
        data[18..20].copy_from_slice(&243u16.to_le_bytes());
        data[32..40].copy_from_slice(&64u64.to_le_bytes());
        data[40..48].copy_from_slice(&128u64.to_le_bytes());
        data[52..54].copy_from_slice(&64u16.to_le_bytes());
        data[54..56].copy_from_slice(&56u16.to_le_bytes());
        data[56..58].copy_from_slice(&1u16.to_le_bytes());
        data[58..60].copy_from_slice(&64u16.to_le_bytes());
        data[60..62].copy_from_slice(&3u16.to_le_bytes());
        data[62..64].copy_from_slice(&1u16.to_le_bytes());
        data[64..68].copy_from_slice(&1u32.to_le_bytes());
        data[68..72].copy_from_slice(&5u32.to_le_bytes());
        data[72..80].copy_from_slice(&300u64.to_le_bytes());
        data[96..104].copy_from_slice(&8u64.to_le_bytes());
        data[104..112].copy_from_slice(&16u64.to_le_bytes());
        // Section 1 is a tiny string table at 280.
        data[192..196].copy_from_slice(&3u32.to_le_bytes());
        data[216..224].copy_from_slice(&280u64.to_le_bytes());
        data[224..232].copy_from_slice(&16u64.to_le_bytes());
        // Section 2 is .bss (SHT_NOBITS) at a nonexistent file offset.
        data[256..260].copy_from_slice(&1u32.to_le_bytes());
        data[260..264].copy_from_slice(&8u32.to_le_bytes());
        data[280..296].copy_from_slice(b"\0.bss\0.shstrtab\0");
        data
    }

    #[test]
    fn elf_sections_do_not_double_count_overlaps_or_nobits() {
        let data = fixture();
        let parsed = parse(&data).unwrap().unwrap();
        assert_eq!(parsed.header.machine, 243);
        assert_eq!(parsed.program_headers[0].file_offset, 300);
        assert_eq!(parsed.sections[2].name, ".bss");
        assert_eq!(parsed.sections[2].file_offset, None);
        assert!(parsed
            .unassigned_file_ranges
            .iter()
            .all(|gap| gap.start < gap.end));
    }

    #[test]
    fn malformed_elf_is_an_error_not_a_non_executable_cell() {
        let err = parse(b"\x7fELF\x02\x01\x01").unwrap_err().to_string();
        assert!(err.contains("truncated"));
        assert!(parse(b"ordinary cell data").unwrap().is_none());
    }

    #[test]
    fn sectionless_load_owns_file_range_and_preserves_wide_entry() {
        let mut data = fixture();
        data[40..48].fill(0);
        data[58..64].fill(0);
        data[24..32].copy_from_slice(&9_007_199_254_740_993u64.to_le_bytes());
        // The load segment covers [300, 308); it must not be called unassigned.
        let parsed = parse(&data).unwrap().unwrap();
        assert!(parsed.sections.is_empty());
        assert!(parsed
            .unassigned_file_ranges
            .iter()
            .all(|gap| gap.end <= 300 || gap.start >= 308));
        let json = serde_json::to_value(&parsed).unwrap();
        assert_eq!(json["header"]["entry"], "9007199254740993");
    }

    #[test]
    fn attributes_use_tag_riscv_arch_not_a_stray_rv64_string() {
        let attributes = [
            7u8, b'r', b'v', b'6', b'4', b'f', b'a', b'k', b'e', 0, 5, b'r', b'v', b'3', b'2',
            b'i', b'2', b'p', b'1', 0,
        ];
        let file_length = 1 + 4 + attributes.len();
        let vendor_length = 4 + 6 + file_length;
        let mut data = vec![b'A'];
        data.extend_from_slice(&(vendor_length as u32).to_le_bytes());
        data.extend_from_slice(b"riscv\0");
        data.push(1);
        data.extend_from_slice(&(file_length as u32).to_le_bytes());
        data.extend_from_slice(&attributes);
        assert_eq!(riscv_arch_attribute(&data).unwrap(), "rv32i2p1");
        data.pop();
        assert!(riscv_arch_attribute(&data)
            .unwrap_err()
            .to_string()
            .contains("truncated"));
    }
}
