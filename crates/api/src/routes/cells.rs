#![allow(clippy::type_complexity)]
#![allow(clippy::manual_is_multiple_of)]

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::{
    is_genesis_special_burn_cell, GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::utils::{
    address_to_lock_script_hash, deployment_key_for_script, deployment_reference_hashes,
    is_ckb_address, is_known_script_name, merge_script_info_for_reference,
    resolve_code_hash_for_hash_type, script_to_address, shannon_to_ckb,
};
use crate::AppState;
use ckbadger_store::keys;

const SHANNONS_PER_CKB: i64 = 100_000_000;
const UNKNOWN_SCRIPT_NAME: &str = "unknown";
const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const SUDT_CODE_HASH: &str = "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
const XUDT_CODE_HASH_DATA1: &str =
    "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95";
const XUDT_CODE_HASH_TYPE: &str =
    "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb";
const DOTBIT_ACCOUNT_CELL_TYPE_ID: &str =
    "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918";
const MNFT_ISSUER_CODE_HASH: &str =
    "0x24b04faf80ded836efc05247778eec4ec02548dab6e2012c0107374aa3f68b81";
const MNFT_CLASS_CODE_HASH: &str =
    "0xd51e6eaf48124c601f41abe173f1da550b4cbca9c6a166781906a287abbb3d9a";
const MNFT_TOKEN_CODE_HASH: &str =
    "0x2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9";
const SPORE_CODE_HASHES: [&str; 4] = [
    "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5",
    "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33",
    "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d",
    "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494",
];
const CLUSTER_CODE_HASHES: [&str; 3] = [
    "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075",
    "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058",
    "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080",
];

fn is_known_script_label(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(UNKNOWN_SCRIPT_NAME)
}

fn format_script_code_hash_label(code_hash: &[u8]) -> String {
    let full = hex::encode(code_hash);
    let prefix = &full[..full.len().min(10)];
    let suffix_start = full.len().saturating_sub(8);
    let suffix = &full[suffix_start..];
    format!("script:0x{}...{}", prefix, suffix)
}

struct DepGroupParseResult {
    is_dep_group: bool,
    items: Option<Vec<DepGroupItem>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDataSegment {
    pub label: String,
    pub start: i32,
    pub end: i32,
    pub meaning: String,
    pub human_value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDeterministicDecode {
    pub kind: String,
    pub summary: String,
    pub segments: Vec<CellDataSegment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDataGuess {
    pub kind: String,
    pub confidence: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDataAnalysis {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deterministic: Option<CellDeterministicDecode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heuristic_guesses: Vec<CellDataGuess>,
}

fn parse_dep_group(data: &[u8], data_size: i32) -> DepGroupParseResult {
    let full_size = data_size as usize;

    // OutPointVec format: 4 bytes count + N * 36 bytes OutPoints
    if full_size < 40 || (full_size - 4) % 36 != 0 {
        return DepGroupParseResult {
            is_dep_group: false,
            items: None,
        };
    }

    if data.len() < 4 {
        return DepGroupParseResult {
            is_dep_group: false,
            items: None,
        };
    }

    let count = match data[0..4].try_into().ok().map(u32::from_le_bytes) {
        Some(c) => c as usize,
        None => {
            return DepGroupParseResult {
                is_dep_group: false,
                items: None,
            }
        }
    };

    let expected_size = 4 + count * 36;
    if count == 0 || count > 256 || expected_size != full_size {
        return DepGroupParseResult {
            is_dep_group: false,
            items: None,
        };
    }

    // At this point we know it's a valid dep group format
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 4 + i * 36;
        if offset + 36 > data.len() {
            break;
        }
        let tx_hash = format!("0x{}", hex::encode(&data[offset..offset + 32]));
        if let Some(index) = data[offset + 32..offset + 36]
            .try_into()
            .ok()
            .map(u32::from_le_bytes)
        {
            items.push(DepGroupItem {
                tx_hash,
                output_index: index,
            });
        }
    }

    DepGroupParseResult {
        is_dep_group: true,
        items: if items.len() == count {
            Some(items)
        } else {
            None // Data truncated, can't return complete list
        },
    }
}

fn is_spore_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    SPORE_CODE_HASHES.iter().any(|h| *h == code_hash_hex)
}

fn is_cluster_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    CLUSTER_CODE_HASHES.iter().any(|h| *h == code_hash_hex)
}

fn is_dotbit_account_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    code_hash_hex == DOTBIT_ACCOUNT_CELL_TYPE_ID
}

fn is_dao_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    code_hash_hex == DAO_CODE_HASH
}

fn is_mnft_issuer_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    code_hash_hex == MNFT_ISSUER_CODE_HASH
}

fn is_mnft_class_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    code_hash_hex == MNFT_CLASS_CODE_HASH
}

fn is_mnft_token_type_code_hash(code_hash: &[u8]) -> bool {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    code_hash_hex == MNFT_TOKEN_CODE_HASH
}

fn read_molecule_bytes_field(
    data: &[u8],
    start: usize,
    end: usize,
) -> Option<(usize, usize, Vec<u8>)> {
    if start >= end || start + 4 > data.len() {
        return None;
    }
    let len = u32::from_le_bytes(data[start..start + 4].try_into().ok()?) as usize;
    let value_start = start + 4;
    let value_end = value_start.checked_add(len)?;
    if value_end > data.len() || value_end > end {
        return None;
    }
    Some((
        value_start,
        value_end,
        data[value_start..value_end].to_vec(),
    ))
}

fn maybe_parse_spore_decode(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDeterministicDecode> {
    let type_code_hash = info.type_code_hash.as_ref()?;
    if !is_spore_type_code_hash(type_code_hash) {
        return None;
    }
    if data.len() < 16 {
        return None;
    }

    let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if total_size < 16 || data.len() < total_size {
        return None;
    }

    let offset_content_type = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    let offset_content = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let offset_cluster_id = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;

    if !(16 <= offset_content_type
        && offset_content_type <= offset_content
        && offset_content <= offset_cluster_id
        && offset_cluster_id <= total_size)
    {
        return None;
    }

    let mut segments = vec![
        CellDataSegment {
            label: "total_size".to_string(),
            start: 0,
            end: 4,
            meaning: "Molecule table total size (u32 LE)".to_string(),
            human_value: total_size.to_string(),
        },
        CellDataSegment {
            label: "offset_content_type".to_string(),
            start: 4,
            end: 8,
            meaning: "Offset to content_type bytes field".to_string(),
            human_value: offset_content_type.to_string(),
        },
        CellDataSegment {
            label: "offset_content".to_string(),
            start: 8,
            end: 12,
            meaning: "Offset to content bytes field".to_string(),
            human_value: offset_content.to_string(),
        },
        CellDataSegment {
            label: "offset_cluster_id".to_string(),
            start: 12,
            end: 16,
            meaning: "Offset to optional cluster_id bytes field".to_string(),
            human_value: offset_cluster_id.to_string(),
        },
    ];

    let (content_type_start, content_type_end, content_type_bytes) =
        read_molecule_bytes_field(data, offset_content_type, offset_content)?;
    let content_type = String::from_utf8_lossy(&content_type_bytes).replace('\0', "");
    segments.push(CellDataSegment {
        label: "content_type".to_string(),
        start: content_type_start as i32,
        end: content_type_end as i32,
        meaning: "Spore content MIME type".to_string(),
        human_value: content_type.clone(),
    });

    let (content_start, content_end, _content_bytes) =
        read_molecule_bytes_field(data, offset_content, offset_cluster_id)?;
    segments.push(CellDataSegment {
        label: "content".to_string(),
        start: content_start as i32,
        end: content_end as i32,
        meaning: "Spore binary payload".to_string(),
        human_value: format!("{} bytes", content_end.saturating_sub(content_start)),
    });

    if offset_cluster_id < total_size && offset_cluster_id + 4 <= data.len() {
        let opt_header = u32::from_le_bytes(
            data[offset_cluster_id..offset_cluster_id + 4]
                .try_into()
                .ok()?,
        );
        if opt_header == 0 {
            segments.push(CellDataSegment {
                label: "cluster_id".to_string(),
                start: offset_cluster_id as i32,
                end: (offset_cluster_id + 4) as i32,
                meaning: "Optional cluster id marker".to_string(),
                human_value: "none".to_string(),
            });
        } else if let Some((cluster_start, cluster_end, cluster_bytes)) =
            read_molecule_bytes_field(data, offset_cluster_id, total_size)
        {
            segments.push(CellDataSegment {
                label: "cluster_id".to_string(),
                start: cluster_start as i32,
                end: cluster_end as i32,
                meaning: "Cluster id bytes".to_string(),
                human_value: format!("0x{}", hex::encode(cluster_bytes)),
            });
        }
    }

    Some(CellDeterministicDecode {
        kind: "spore_cell".to_string(),
        summary: format!(
            "Spore molecule layout detected (content_type={}, content_bytes={})",
            content_type,
            content_end.saturating_sub(content_start)
        ),
        segments,
    })
}

fn maybe_parse_cluster_decode(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDeterministicDecode> {
    let type_code_hash = info.type_code_hash.as_ref()?;
    if !is_cluster_type_code_hash(type_code_hash) {
        return None;
    }
    if data.len() < 12 {
        return None;
    }

    let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if total_size < 12 || data.len() < total_size {
        return None;
    }

    let offset_name = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    let offset_description = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    if !(12 <= offset_name && offset_name <= offset_description && offset_description <= total_size)
    {
        return None;
    }

    let mut segments = vec![
        CellDataSegment {
            label: "total_size".to_string(),
            start: 0,
            end: 4,
            meaning: "Molecule table total size (u32 LE)".to_string(),
            human_value: total_size.to_string(),
        },
        CellDataSegment {
            label: "offset_name".to_string(),
            start: 4,
            end: 8,
            meaning: "Offset to cluster name bytes".to_string(),
            human_value: offset_name.to_string(),
        },
        CellDataSegment {
            label: "offset_description".to_string(),
            start: 8,
            end: 12,
            meaning: "Offset to cluster description bytes".to_string(),
            human_value: offset_description.to_string(),
        },
    ];

    let desc_end = if data.len() >= 16 {
        let candidate = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;
        if candidate >= offset_description && candidate <= total_size {
            candidate
        } else {
            total_size
        }
    } else {
        total_size
    };

    if let Some((name_start, name_end, name_bytes)) =
        read_molecule_bytes_field(data, offset_name, offset_description)
    {
        segments.push(CellDataSegment {
            label: "name".to_string(),
            start: name_start as i32,
            end: name_end as i32,
            meaning: "Cluster name".to_string(),
            human_value: String::from_utf8_lossy(&name_bytes).replace('\0', ""),
        });
    }
    if let Some((desc_start, desc_end_offset, desc_bytes)) =
        read_molecule_bytes_field(data, offset_description, desc_end)
    {
        segments.push(CellDataSegment {
            label: "description".to_string(),
            start: desc_start as i32,
            end: desc_end_offset as i32,
            meaning: "Cluster description".to_string(),
            human_value: String::from_utf8_lossy(&desc_bytes).replace('\0', ""),
        });
    }

    Some(CellDeterministicDecode {
        kind: "spore_cluster_cell".to_string(),
        summary: "Spore cluster molecule layout detected".to_string(),
        segments,
    })
}

fn read_u16_text_field(data: &[u8], offset: usize) -> Option<(usize, usize, String, usize)> {
    if offset + 2 > data.len() {
        return None;
    }
    let len = u16::from_le_bytes(data[offset..offset + 2].try_into().ok()?) as usize;
    let value_start = offset + 2;
    let value_end = value_start.checked_add(len)?;
    if len == 0 || value_end > data.len() {
        return None;
    }
    let value = String::from_utf8_lossy(&data[value_start..value_end]).replace('\0', "");
    Some((value_start, value_end, value, value_end))
}

fn maybe_parse_mnft_decode(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDeterministicDecode> {
    let type_code_hash = info.type_code_hash.as_ref()?;

    if is_mnft_issuer_type_code_hash(type_code_hash) {
        if data.len() < 9 {
            return None;
        }
        let version = data[0];
        let class_count = u32::from_le_bytes(data[1..5].try_into().ok()?);
        let set_count = u32::from_le_bytes(data[5..9].try_into().ok()?);

        let mut segments = vec![
            CellDataSegment {
                label: "version".to_string(),
                start: 0,
                end: 1,
                meaning: "mNFT issuer schema version".to_string(),
                human_value: version.to_string(),
            },
            CellDataSegment {
                label: "class_count".to_string(),
                start: 1,
                end: 5,
                meaning: "Number of classes under this issuer (u32 LE)".to_string(),
                human_value: class_count.to_string(),
            },
            CellDataSegment {
                label: "set_count".to_string(),
                start: 5,
                end: 9,
                meaning: "Number of sets under this issuer (u32 LE)".to_string(),
                human_value: set_count.to_string(),
            },
        ];

        if data.len() >= 11 {
            let info_size = u16::from_le_bytes(data[9..11].try_into().ok()?) as usize;
            segments.push(CellDataSegment {
                label: "info_size".to_string(),
                start: 9,
                end: 11,
                meaning: "Length of issuer metadata blob (u16 LE)".to_string(),
                human_value: info_size.to_string(),
            });
            let info_start = 11usize;
            let info_end = info_start.checked_add(info_size)?;
            if info_size > 0 && info_end <= data.len() {
                let info_value =
                    String::from_utf8_lossy(&data[info_start..info_end]).replace('\0', "");
                segments.push(CellDataSegment {
                    label: "info_blob".to_string(),
                    start: info_start as i32,
                    end: info_end as i32,
                    meaning: "Issuer metadata payload".to_string(),
                    human_value: info_value.chars().take(120).collect(),
                });
            }
        }

        if let Some(last_end) = segments.last().map(|s| s.end as usize) {
            if data.len() > last_end {
                segments.push(CellDataSegment {
                    label: "trailing_payload".to_string(),
                    start: last_end as i32,
                    end: data.len() as i32,
                    meaning: "Trailing bytes after parsed issuer fields".to_string(),
                    human_value: format!("{} bytes", data.len() - last_end),
                });
            }
        }

        return Some(CellDeterministicDecode {
            kind: "mnft_issuer_cell".to_string(),
            summary: "mNFT issuer layout detected".to_string(),
            segments,
        });
    }

    if is_mnft_class_type_code_hash(type_code_hash) {
        if data.len() < 10 {
            return None;
        }
        let version = data[0];
        let total = u32::from_le_bytes(data[1..5].try_into().ok()?);
        let issued = u32::from_le_bytes(data[5..9].try_into().ok()?);
        let configure = data[9];

        let mut segments = vec![
            CellDataSegment {
                label: "version".to_string(),
                start: 0,
                end: 1,
                meaning: "mNFT class schema version".to_string(),
                human_value: version.to_string(),
            },
            CellDataSegment {
                label: "total".to_string(),
                start: 1,
                end: 5,
                meaning: "Class max supply (u32 LE)".to_string(),
                human_value: total.to_string(),
            },
            CellDataSegment {
                label: "issued".to_string(),
                start: 5,
                end: 9,
                meaning: "Class issued count (u32 LE)".to_string(),
                human_value: issued.to_string(),
            },
            CellDataSegment {
                label: "configure".to_string(),
                start: 9,
                end: 10,
                meaning: "Class configure flags".to_string(),
                human_value: format!("0x{:02x}", configure),
            },
        ];

        let mut offset = 10usize;
        if let Some((start, end, value, next)) = read_u16_text_field(data, offset) {
            segments.push(CellDataSegment {
                label: "name".to_string(),
                start: start as i32,
                end: end as i32,
                meaning: "Class name".to_string(),
                human_value: value,
            });
            offset = next;
        } else {
            return Some(CellDeterministicDecode {
                kind: "mnft_class_cell".to_string(),
                summary: "mNFT class header parsed; text fields unavailable".to_string(),
                segments,
            });
        }

        if let Some((start, end, value, next)) = read_u16_text_field(data, offset) {
            segments.push(CellDataSegment {
                label: "description".to_string(),
                start: start as i32,
                end: end as i32,
                meaning: "Class description".to_string(),
                human_value: value,
            });
            offset = next;
        } else {
            return Some(CellDeterministicDecode {
                kind: "mnft_class_cell".to_string(),
                summary: "mNFT class parsed up to name/description boundary".to_string(),
                segments,
            });
        }

        if let Some((start, end, value, next)) = read_u16_text_field(data, offset) {
            segments.push(CellDataSegment {
                label: "renderer".to_string(),
                start: start as i32,
                end: end as i32,
                meaning: "Class renderer descriptor".to_string(),
                human_value: value,
            });
            offset = next;
        }

        if data.len() > offset {
            segments.push(CellDataSegment {
                label: "trailing_payload".to_string(),
                start: offset as i32,
                end: data.len() as i32,
                meaning: "Trailing bytes after parsed class fields".to_string(),
                human_value: format!("{} bytes", data.len() - offset),
            });
        }

        return Some(CellDeterministicDecode {
            kind: "mnft_class_cell".to_string(),
            summary: "mNFT class layout detected".to_string(),
            segments,
        });
    }

    if is_mnft_token_type_code_hash(type_code_hash) {
        if data.len() < 11 {
            return None;
        }
        let version = data[0];
        let characteristic = &data[1..9];
        let configure = data[9];
        let state = data[10];

        let mut segments = vec![
            CellDataSegment {
                label: "version".to_string(),
                start: 0,
                end: 1,
                meaning: "mNFT token schema version".to_string(),
                human_value: version.to_string(),
            },
            CellDataSegment {
                label: "characteristic".to_string(),
                start: 1,
                end: 9,
                meaning: "8-byte token characteristic field".to_string(),
                human_value: format!("0x{}", hex::encode(characteristic)),
            },
            CellDataSegment {
                label: "configure".to_string(),
                start: 9,
                end: 10,
                meaning: "Token configure flags".to_string(),
                human_value: format!("0x{:02x}", configure),
            },
            CellDataSegment {
                label: "state".to_string(),
                start: 10,
                end: 11,
                meaning: "Token state flags".to_string(),
                human_value: format!("0x{:02x}", state),
            },
        ];

        if data.len() > 11 {
            segments.push(CellDataSegment {
                label: "trailing_payload".to_string(),
                start: 11,
                end: data.len() as i32,
                meaning: "Trailing bytes after parsed token fields".to_string(),
                human_value: format!("{} bytes", data.len() - 11),
            });
        }

        return Some(CellDeterministicDecode {
            kind: "mnft_token_cell".to_string(),
            summary: "mNFT token layout detected".to_string(),
            segments,
        });
    }

    None
}

fn maybe_parse_dao_decode(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDeterministicDecode> {
    let type_code_hash = info.type_code_hash.as_ref()?;
    if !is_dao_type_code_hash(type_code_hash) {
        return None;
    }
    if data.len() != 8 {
        return None;
    }

    if data == [0u8; 8] {
        return Some(CellDeterministicDecode {
            kind: "dao_deposit_cell".to_string(),
            summary: "DAO deposit cell detected (8-byte zero data)".to_string(),
            segments: vec![CellDataSegment {
                label: "dao_state".to_string(),
                start: 0,
                end: 8,
                meaning: "DAO state marker".to_string(),
                human_value: "deposit".to_string(),
            }],
        });
    }

    let deposit_block_number = u64::from_le_bytes(data.try_into().ok()?);
    Some(CellDeterministicDecode {
        kind: "dao_withdraw_request_cell".to_string(),
        summary: format!(
            "DAO withdraw-request cell detected (deposit_block_number={})",
            deposit_block_number
        ),
        segments: vec![
            CellDataSegment {
                label: "dao_state".to_string(),
                start: 0,
                end: 8,
                meaning: "DAO state marker".to_string(),
                human_value: "withdraw_request".to_string(),
            },
            CellDataSegment {
                label: "deposit_block_number".to_string(),
                start: 0,
                end: 8,
                meaning: "Original deposit block number (u64 LE)".to_string(),
                human_value: deposit_block_number.to_string(),
            },
        ],
    })
}

fn detect_udt_standard_from_code_hash(code_hash: &[u8]) -> Option<&'static str> {
    let code_hash_hex = format!("0x{}", hex::encode(code_hash));
    if code_hash_hex == SUDT_CODE_HASH {
        return Some("sudt");
    }
    if code_hash_hex == XUDT_CODE_HASH_DATA1 || code_hash_hex == XUDT_CODE_HASH_TYPE {
        return Some("xudt");
    }
    None
}

fn maybe_parse_udt_decode(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDeterministicDecode> {
    let type_code_hash = info.type_code_hash.as_ref()?;
    let standard = detect_udt_standard_from_code_hash(type_code_hash)?;

    if data.len() < 16 {
        return None;
    }

    let amount = u128::from_le_bytes(data[0..16].try_into().ok()?);
    let mut segments = vec![CellDataSegment {
        label: "amount".to_string(),
        start: 0,
        end: 16,
        meaning: format!("{} amount in little-endian u128", standard.to_uppercase()),
        human_value: amount.to_string(),
    }];

    if data.len() > 16 {
        segments.push(CellDataSegment {
            label: "extension_data".to_string(),
            start: 16,
            end: data.len() as i32,
            meaning: "Additional payload bytes beyond canonical UDT amount".to_string(),
            human_value: format!("{} bytes", data.len() - 16),
        });
    }

    Some(CellDeterministicDecode {
        kind: "udt_amount".to_string(),
        summary: format!(
            "{} cell data starts with amount={} (u128 LE)",
            standard.to_uppercase(),
            amount
        ),
        segments,
    })
}

fn maybe_parse_dotbit_decode(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDeterministicDecode> {
    let type_code_hash = info.type_code_hash.as_ref()?;
    if !is_dotbit_account_type_code_hash(type_code_hash) {
        return None;
    }
    if data.len() < 52 {
        return None;
    }

    let mut segments = Vec::new();
    segments.push(CellDataSegment {
        label: "account_hash".to_string(),
        start: 0,
        end: 32,
        meaning: "DAS account hash prefix".to_string(),
        human_value: format!("0x{}", hex::encode(&data[0..32])),
    });
    segments.push(CellDataSegment {
        label: "account_id".to_string(),
        start: 32,
        end: 52,
        meaning: "Unique 20-byte DAS account id".to_string(),
        human_value: format!("0x{}", hex::encode(&data[32..52])),
    });

    if data.len() >= 72 {
        let next_id = &data[52..72];
        let is_zero = next_id.iter().all(|b| *b == 0);
        segments.push(CellDataSegment {
            label: "next_account_id".to_string(),
            start: 52,
            end: 72,
            meaning: "Linked-list pointer to next account".to_string(),
            human_value: if is_zero {
                "none".to_string()
            } else {
                format!("0x{}", hex::encode(next_id))
            },
        });
    }

    if data.len() >= 80 {
        let expired_at = u64::from_le_bytes(data[72..80].try_into().ok()?);
        let expired_at_str = if let Ok(expired_i64) = i64::try_from(expired_at) {
            chrono::DateTime::from_timestamp(expired_i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| format!("unix:{}", expired_at))
        } else {
            format!("unix:{}", expired_at)
        };
        segments.push(CellDataSegment {
            label: "expired_at".to_string(),
            start: 72,
            end: 80,
            meaning: "Account expiration timestamp (seconds)".to_string(),
            human_value: expired_at_str,
        });
    }

    if data.len() > 80 {
        segments.push(CellDataSegment {
            label: "trailing_payload".to_string(),
            start: 80,
            end: data.len() as i32,
            meaning: "Remaining bytes in account cell payload".to_string(),
            human_value: format!("{} bytes", data.len() - 80),
        });
    }

    Some(CellDeterministicDecode {
        kind: "dotbit_account".to_string(),
        summary: "DAS account cell layout detected from type script".to_string(),
        segments,
    })
}

fn maybe_parse_dep_group_decode(data: &[u8], data_size: i32) -> Option<CellDeterministicDecode> {
    let parsed = parse_dep_group(data, data_size);
    if !parsed.is_dep_group || data.len() < 4 {
        return None;
    }

    let count = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    let mut segments = Vec::new();
    segments.push(CellDataSegment {
        label: "count".to_string(),
        start: 0,
        end: 4,
        meaning: "Number of outpoints in dep group".to_string(),
        human_value: count.to_string(),
    });

    for idx in 0..count {
        let base = 4 + idx * 36;
        if base + 36 > data.len() {
            break;
        }

        segments.push(CellDataSegment {
            label: format!("outpoint[{}].tx_hash", idx),
            start: base as i32,
            end: (base + 32) as i32,
            meaning: "Referenced transaction hash".to_string(),
            human_value: format!("0x{}", hex::encode(&data[base..base + 32])),
        });

        let output_index = u32::from_le_bytes(data[base + 32..base + 36].try_into().ok()?);
        segments.push(CellDataSegment {
            label: format!("outpoint[{}].output_index", idx),
            start: (base + 32) as i32,
            end: (base + 36) as i32,
            meaning: "Referenced output index (u32 LE)".to_string(),
            human_value: output_index.to_string(),
        });
    }

    let parsed_count = (segments.len().saturating_sub(1)) / 2;
    Some(CellDeterministicDecode {
        kind: "dep_group_out_point_vec".to_string(),
        summary: format!(
            "Dep group OutPointVec with {} item(s); decoded {} item(s) from available bytes",
            count, parsed_count
        ),
        segments,
    })
}

fn parse_printable_utf8(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    if text
        .chars()
        .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
    {
        return Some(text.to_string());
    }
    None
}

fn build_heuristic_guesses(data: &[u8]) -> Vec<CellDataGuess> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut guesses = Vec::new();

    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "PNG signature matched (89 50 4E 47 0D 0A 1A 0A)".to_string(),
            mime_type: Some("image/png".to_string()),
            human_value: None,
        });
    } else if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "JPEG SOI marker matched (FF D8 FF)".to_string(),
            mime_type: Some("image/jpeg".to_string()),
            human_value: None,
        });
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "GIF header matched".to_string(),
            mime_type: Some("image/gif".to_string()),
            human_value: None,
        });
    } else if data.starts_with(b"%PDF-") {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "PDF header matched".to_string(),
            mime_type: Some("application/pdf".to_string()),
            human_value: None,
        });
    } else if data.starts_with(b"\x7FELF") {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "ELF binary header matched".to_string(),
            mime_type: Some("application/x-elf".to_string()),
            human_value: None,
        });
    } else if data.starts_with(b"\0asm") {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "WebAssembly binary header matched (00 61 73 6D)".to_string(),
            mime_type: Some("application/wasm".to_string()),
            human_value: None,
        });
    } else if data.len() >= 4
        && data[0] == 0x50
        && data[1] == 0x4B
        && data[2] == 0x03
        && data[3] == 0x04
    {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "ZIP local file header matched (PK\\x03\\x04)".to_string(),
            mime_type: Some("application/zip".to_string()),
            human_value: None,
        });
    } else if data.len() >= 2 && data[0] == 0x1F && data[1] == 0x8B {
        guesses.push(CellDataGuess {
            kind: "magic_number".to_string(),
            confidence: "high".to_string(),
            reason: "GZIP header matched (1F 8B)".to_string(),
            mime_type: Some("application/gzip".to_string()),
            human_value: None,
        });
    }

    if data.len() == 16 {
        let value = u128::from_le_bytes(
            data.try_into()
                .expect("data length already checked as exactly 16 bytes"),
        );
        guesses.push(CellDataGuess {
            kind: "numeric_pattern".to_string(),
            confidence: "medium".to_string(),
            reason: "Payload length is exactly 16 bytes (common u128 LE encoding)".to_string(),
            mime_type: None,
            human_value: Some(value.to_string()),
        });
    }

    if let Some(text) = parse_printable_utf8(data) {
        let trimmed = text.trim();
        if (trimmed.starts_with('{') || trimmed.starts_with('['))
            && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
        {
            guesses.push(CellDataGuess {
                kind: "text_pattern".to_string(),
                confidence: "high".to_string(),
                reason: "UTF-8 payload parses as JSON".to_string(),
                mime_type: Some("application/json".to_string()),
                human_value: Some(trimmed.chars().take(120).collect()),
            });
        } else {
            guesses.push(CellDataGuess {
                kind: "text_pattern".to_string(),
                confidence: "medium".to_string(),
                reason: "Payload is printable UTF-8 text".to_string(),
                mime_type: Some("text/plain".to_string()),
                human_value: Some(trimmed.chars().take(120).collect()),
            });
        }
    }

    if guesses.is_empty() {
        guesses.push(CellDataGuess {
            kind: "binary_fallback".to_string(),
            confidence: "low".to_string(),
            reason: "No known signature matched; treated as opaque binary".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            human_value: Some(format!("{} bytes", data.len())),
        });
    }

    guesses
}

fn analyze_cell_data(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
    data_size: i32,
) -> CellDataAnalysis {
    let deterministic = maybe_parse_dao_decode(info, data)
        .or_else(|| maybe_parse_spore_decode(info, data))
        .or_else(|| maybe_parse_cluster_decode(info, data))
        .or_else(|| maybe_parse_mnft_decode(info, data))
        .or_else(|| maybe_parse_udt_decode(info, data))
        .or_else(|| maybe_parse_dotbit_decode(info, data))
        .or_else(|| {
            if info.type_code_hash.is_none() {
                maybe_parse_dep_group_decode(data, data_size)
            } else {
                None
            }
        });

    let heuristic_guesses = build_heuristic_guesses(data);

    CellDataAnalysis {
        deterministic,
        heuristic_guesses,
    }
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/cells/live", get(list_live_cells))
        .route("/cells/by-script", get(list_cells_by_script))
        .route("/cells/{tx_hash}/{output_index}", get(get_cell))
        .route("/addresses/top", get(get_top_addresses))
        .route("/addresses/active", get(get_active_addresses))
        .route("/addresses/{addr}", get(get_address))
        .route(
            "/addresses/{addr}/transactions",
            get(get_address_transactions),
        )
        .route("/addresses/{addr}/tokens", get(get_address_tokens))
        .route(
            "/addresses/{addr}/stats-history",
            get(get_address_stats_history),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListCellsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    lock_script_hash: Option<String>,
    type_script_hash: Option<String>,
    type_code_hash: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListCellsByScriptParams {
    #[serde(default = "default_limit")]
    limit: i64,
    code_hash: String,
    hash_type: String,
    #[serde(default = "default_script_kind")]
    script_kind: String,
    #[allow(dead_code)]
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

fn default_script_kind() -> String {
    "both".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellResponse {
    pub tx_hash: String,
    pub output_index: i32,
    pub capacity: String,
    pub lock_script_hash: String,
    pub type_script_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_code_hash: Option<String>,
    pub data_size: i32,
    pub created_at_block: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_occupied_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udt_amount: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepGroupItem {
    pub tx_hash: String,
    pub output_index: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellScript {
    pub name: String,
    pub code_hash: String,
    pub hash_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_type_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_data_hash: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoInfo {
    pub is_dao_cell: bool,
    pub dao_status: String,
    pub deposit_block_number: i64,
    pub deposit_timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_request_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_request_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compensation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compensation_ckb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_apc: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDetailResponse {
    pub tx_hash: String,
    pub output_index: i32,
    pub capacity: String,
    pub occupied_capacity: i64,
    pub occupied_capacity_breakdown: OccupiedCapacityBreakdown,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_occupied_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub type_script_hash: Option<String>,
    pub data_size: i32,
    pub created_at_block: i64,
    pub status: String,
    pub consumed_at_block: Option<i64>,
    pub consumed_by_tx: Option<String>,
    pub lock: ScriptResponse,
    #[serde(rename = "type")]
    pub type_script: Option<ScriptResponse>,
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_analysis: Option<CellDataAnalysis>,
    pub is_dep_group: bool,
    pub dep_group_items: Option<Vec<DepGroupItem>>,
    pub code_cell_of: Option<Vec<CodeCellScript>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dao_info: Option<DaoInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OccupiedCapacityBreakdown {
    pub capacity_field_bytes: i64,
    pub lock_script_bytes: i64,
    pub type_script_bytes: i64,
    pub data_bytes: i64,
    pub total_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScriptInfo {
    pub code_hash: String,
    pub name: String,
    pub script_kind: Option<String>,
    pub deprecated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub balance: String,
    pub occupied_capacity: String,
    pub live_cells_count: i64,
    pub transactions_count: i64,
    pub recent_activities_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_script: Option<ScriptResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_script_info: Option<LockScriptInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopAddressResponse {
    pub lock_script_hash: String,
    pub balance: String,
    pub live_cells_count: i32,
    pub transactions_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct TopAddressesParams {
    #[serde(default = "default_top_limit")]
    limit: i64,
}

fn default_top_limit() -> i64 {
    100
}

#[derive(Debug, Deserialize)]
pub struct ActiveAddressesParams {
    #[serde(default = "default_top_limit")]
    limit: i64,
    #[serde(default = "default_days")]
    days: i64,
}

fn default_days() -> i64 {
    7
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAddressResponse {
    pub lock_script_hash: String,
    pub balance: String,
    pub live_cells_count: i32,
    pub transactions_count: i64,
    pub last_activity_block: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressTransactionResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_type: String,
    pub capacity_change: String,
    pub timestamp: String,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub fee: String,
    pub is_cellbase: bool,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub script_labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressTxParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressTokensParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[allow(dead_code)]
    cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressTokenResponse {
    pub type_script_hash: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: i16,
    pub icon_url: Option<String>,
    pub balance: String,
}

/// Helper to convert a LiveCellInfo into a CellResponse.
fn cell_info_to_response(
    tx_hash: &[u8],
    output_index: i16,
    info: &ckbadger_store::LiveCellInfo,
) -> CellResponse {
    let is_special_burn = is_genesis_special_burn_cell(&info.lock_args, info.created_at_block);
    CellResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
        output_index: output_index as i32,
        capacity: info.capacity.to_string(),
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_code_hash: info
            .type_code_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        cell_type: if is_special_burn {
            Some("genesis_special_burn".to_string())
        } else {
            None
        },
        virtual_occupied_capacity: if is_special_burn {
            Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string())
        } else {
            None
        },
        udt_amount: None,
    }
}

fn estimated_occupied_capacity_breakdown(
    info: &ckbadger_store::LiveCellInfo,
) -> OccupiedCapacityBreakdown {
    let capacity_field_bytes = 8;
    let lock_script_bytes = 32 + 1 + info.lock_args.len() as i64;
    let type_script_bytes = if info.type_code_hash.is_some() {
        32 + 1 + info.type_args.as_ref().map_or(0, |args| args.len() as i64)
    } else {
        0
    };
    let data_bytes = info.data_size as i64;
    let total_bytes = capacity_field_bytes + lock_script_bytes + type_script_bytes + data_bytes;

    OccupiedCapacityBreakdown {
        capacity_field_bytes,
        lock_script_bytes,
        type_script_bytes,
        data_bytes,
        total_bytes,
    }
}

/// Decode a cell cursor (hex-encoded full cell index key).
fn decode_cell_cursor(cursor: &str) -> Option<Vec<u8>> {
    hex::decode(cursor.strip_prefix("0x").unwrap_or(cursor)).ok()
}

/// Encode a cell cursor from the last result's components.
fn encode_cell_cursor(
    script_hash: &[u8],
    block_num: i64,
    tx_hash: &[u8],
    output_index: i16,
) -> String {
    let key = keys::encode_cell_index_key(script_hash, block_num, tx_hash, output_index);
    hex::encode(key)
}

async fn list_live_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let after_key = params.cursor.as_deref().and_then(decode_cell_cursor);

    let lock_hash_bytes = if let Some(ref lock_hash) = params.lock_script_hash {
        Some(if is_ckb_address(lock_hash) {
            address_to_lock_script_hash(lock_hash)
                .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
        } else {
            hex::decode(lock_hash.strip_prefix("0x").unwrap_or(lock_hash))
                .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?
        })
    } else {
        None
    };

    let type_hash_bytes = if let Some(ref type_hash) = params.type_script_hash {
        Some(
            hex::decode(type_hash.strip_prefix("0x").unwrap_or(type_hash))
                .map_err(|_| ApiError::bad_request("Invalid type script hash"))?,
        )
    } else {
        None
    };

    let _type_code_hash_bytes = if let Some(ref code_hash) = params.type_code_hash {
        Some(
            hex::decode(code_hash.strip_prefix("0x").unwrap_or(code_hash))
                .map_err(|_| ApiError::bad_request("Invalid type code hash"))?,
        )
    } else {
        None
    };

    let after_key_ref = after_key.as_deref();

    // Fetch cells from the store based on available filters.
    // The store supports listing by lock hash or type hash via prefix scans.
    let raw_cells: Vec<(Vec<u8>, i16, ckbadger_store::LiveCellInfo)> =
        match (&lock_hash_bytes, &type_hash_bytes) {
            (Some(lock_bytes), Some(type_bytes)) => {
                // Filter by lock first (usually more selective), then post-filter by type
                let all = state
                    .store
                    .list_cells_by_lock(lock_bytes, limit * 10 + 1, after_key_ref)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                all.into_iter()
                    .filter(|(_, _, info)| {
                        info.type_script_hash
                            .as_ref()
                            .map(|h| h == type_bytes)
                            .unwrap_or(false)
                    })
                    .take(limit + 1)
                    .collect()
            }
            (Some(lock_bytes), None) => {
                // For type_code_hash filtering, list by lock then post-filter
                if let Some(ref tch) = _type_code_hash_bytes {
                    let all = state
                        .store
                        .list_cells_by_lock(lock_bytes, limit * 10 + 1, after_key_ref)
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                    all.into_iter()
                        .filter(|(_, _, info)| {
                            info.type_code_hash
                                .as_ref()
                                .map(|h| h == tch)
                                .unwrap_or(false)
                        })
                        .take(limit + 1)
                        .collect()
                } else {
                    state
                        .store
                        .list_cells_by_lock(lock_bytes, limit + 1, after_key_ref)
                        .map_err(|e| ApiError::internal(e.to_string()))?
                }
            }
            (None, Some(type_bytes)) => state
                .store
                .list_cells_by_type(type_bytes, limit + 1, after_key_ref)
                .map_err(|e| ApiError::internal(e.to_string()))?,
            (None, None) => {
                // No filter: not practical for RocksDB full scan, return empty.
                // The old PG query scanned the whole table; in RocksDB we can't
                // efficiently paginate the full live_cells CF without a secondary index.
                Vec::new()
            }
        };

    let has_more = raw_cells.len() > limit;
    let raw_cells: Vec<_> = raw_cells.into_iter().take(limit).collect();

    // Determine which script hash was used as the index prefix for cursor encoding
    let index_hash = lock_hash_bytes.as_deref().or(type_hash_bytes.as_deref());

    let next_cursor = if has_more {
        raw_cells.last().and_then(|(tx_hash, output_index, info)| {
            index_hash.map(|h| encode_cell_cursor(h, info.created_at_block, tx_hash, *output_index))
        })
    } else {
        None
    };

    let cells: Vec<CellResponse> = raw_cells
        .iter()
        .map(|(tx_hash, output_index, info)| cell_info_to_response(tx_hash, *output_index, info))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        cells,
        limit as i64,
        next_cursor,
    ))
}

fn parse_hash_type(hash_type: &str) -> Option<u8> {
    match hash_type {
        "data" => Some(0),
        "type" => Some(1),
        "data1" => Some(2),
        "data2" => Some(4),
        _ => None,
    }
}

async fn list_cells_by_script(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsByScriptParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    let hash_type_num = parse_hash_type(&params.hash_type).ok_or_else(|| {
        ApiError::bad_request("Invalid hash_type. Must be one of: data, type, data1, data2")
    })?;

    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = state
        .store
        .list_script_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .map(|(_, info)| info)
        .collect();

    // "type" references are not universally available. If this deployment has no type reference,
    // return empty instead of silently mapping to data-family references.
    if hash_type_num == 1 {
        if let Some(info) = merge_script_info_for_reference(&all_script_infos, &code_hash_bytes) {
            let (type_ref, _) = deployment_reference_hashes(&info);
            if type_ref.is_none() {
                return ok(CursorPaginatedResponse::new(
                    Vec::new(),
                    0,
                    limit as i64,
                    None,
                ));
            }
        }
    }

    let resolved_code_hash =
        resolve_code_hash_for_hash_type(&all_script_infos, &code_hash_bytes, &params.hash_type)
            .unwrap_or_else(|| code_hash_bytes.clone());

    let script_kind = params.script_kind.as_str();

    // Look up script info from the resolved reference hash to get count.
    let script_info = all_script_infos
        .iter()
        .find(|info| info.code_hash == resolved_code_hash);

    let total: i64 = match (script_kind, script_info) {
        ("lock", Some(si)) => si.lock_live_cells_count,
        ("type", Some(si)) => si.type_live_cells_count,
        (_, Some(si)) => si.lock_live_cells_count + si.type_live_cells_count,
        (_, None) => 0,
    };

    // Parse cursor for pagination
    let after_key = params.cursor.as_deref().and_then(decode_cell_cursor);
    let after_key_ref = after_key.as_deref();

    // Fetch limit+1 to detect has_more
    let fetch_limit = limit + 1;

    // Use code_hash indexes for efficient prefix scans
    let results: Vec<(Vec<u8>, i16, ckbadger_store::LiveCellInfo)> = match script_kind {
        "lock" => state
            .store
            .list_cells_by_lock_code_hash(&resolved_code_hash, fetch_limit, after_key_ref)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        "type" => state
            .store
            .list_cells_by_type_code_hash(&resolved_code_hash, fetch_limit, after_key_ref)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        _ => {
            // "both": merge results from lock and type indexes
            let mut merged = state
                .store
                .list_cells_by_lock_code_hash(&resolved_code_hash, fetch_limit, after_key_ref)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let type_results = state
                .store
                .list_cells_by_type_code_hash(&resolved_code_hash, fetch_limit, after_key_ref)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for r in type_results {
                if merged.len() >= fetch_limit {
                    break;
                }
                // Deduplicate: a cell could match both lock and type
                if !merged.iter().any(|(h, i, _)| h == &r.0 && *i == r.1) {
                    merged.push(r);
                }
            }
            merged
        }
    };

    let has_more = results.len() > limit;
    let results: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        results.last().map(|(tx_hash, output_index, info)| {
            encode_cell_cursor(
                &resolved_code_hash,
                info.created_at_block,
                tx_hash,
                *output_index,
            )
        })
    } else {
        None
    };

    let cells: Vec<CellResponse> = results
        .iter()
        .map(|(tx_hash, output_index, info)| cell_info_to_response(tx_hash, *output_index, info))
        .collect();

    ok(CursorPaginatedResponse::new(
        cells,
        total,
        limit as i64,
        next_cursor,
    ))
}

async fn get_address(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
) -> ApiResult<AddressResponse> {
    // Check cache first
    let cache_key = CacheKeys::address_balance(&addr);
    if let Some(cached) = state.cache.get::<AddressResponse>(&cache_key).await {
        return ok(cached);
    }

    let (lock_hash, input_address) = if is_ckb_address(&addr) {
        let hash = address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?;
        (hash, Some(addr.clone()))
    } else {
        let hash = hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?;
        (hash, None)
    };

    // Get balance from the store
    let addr_balance = state
        .store
        .get_addr_balance(&lock_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (balance, occupied_capacity, live_cells_count, transactions_count) = match &addr_balance {
        Some(ab) => (
            ab.balance.to_string(),
            ab.occupied_capacity.to_string(),
            ab.live_cells_count as i64,
            ab.txs_count,
        ),
        None => ("0".to_string(), "0".to_string(), 0, 0),
    };

    // Try to find a cell for this lock hash to get the lock script details
    let cells_for_script = state
        .store
        .list_cells_by_lock(&lock_hash, 1, None)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (lock_script, address) = if let Some((_, _, info)) = cells_for_script.first() {
        // LiveCellInfo doesn't store hash_type directly; derive from code_hash via script_info
        let hash_type_num = state
            .store
            .get_script_info(&info.lock_code_hash)
            .ok()
            .flatten()
            .map(|si| si.hash_type as i16)
            .unwrap_or(1); // Default to "type"

        let hash_type_str = match hash_type_num {
            0 => "data",
            1 => "type",
            2 => "data1",
            4 => "data2",
            _ => "data",
        };

        let script = ScriptResponse {
            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
            hash_type: hash_type_str.to_string(),
            args: format!("0x{}", hex::encode(&info.lock_args)),
        };

        let addr = input_address.or_else(|| {
            script_to_address(
                &info.lock_code_hash,
                hash_type_num,
                &info.lock_args,
                &state.ckb_network,
            )
            .ok()
        });

        (Some(script), addr)
    } else {
        // No live cells found, also check consumed cells for script info.
        // For now, just return what we have.
        (None, input_address)
    };

    // Look up lock script info from script_info CF
    let lock_script_info = if let Some((_, _, info)) = cells_for_script.first() {
        state
            .store
            .get_script_info(&info.lock_code_hash)
            .ok()
            .flatten()
            .map(|si| LockScriptInfo {
                code_hash: format!("0x{}", hex::encode(&si.code_hash)),
                name: si.name.unwrap_or_else(|| "Unknown".to_string()),
                script_kind: Some("lock".to_string()),
                deprecated: false,
            })
    } else {
        None
    };

    let recent_activities_count = transactions_count;

    let response = AddressResponse {
        lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
        address,
        balance,
        occupied_capacity,
        live_cells_count,
        transactions_count,
        recent_activities_count,
        lock_script,
        lock_script_info,
    };

    // Cache the response for 30 seconds
    state
        .cache
        .set(&cache_key, &response, CacheTtl::ADDRESS_BALANCE)
        .await;

    ok(response)
}

fn lookup_code_cell_scripts(
    store: &ckbadger_store::CkbadgerStore,
    data_hash: &[u8],
    type_script_hash: Option<&Vec<u8>>,
) -> Option<Vec<CodeCellScript>> {
    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = store
        .list_script_infos()
        .ok()?
        .into_iter()
        .map(|(_, info)| info)
        .collect();

    let mut scripts: Vec<CodeCellScript> = Vec::new();
    let mut deployment_index: HashMap<Vec<u8>, usize> = HashMap::new();

    let mut upsert = |reference_hash: &[u8], hash_type_hint: &str| {
        let Some(resolved) = merge_script_info_for_reference(&all_script_infos, reference_hash)
        else {
            return;
        };

        let deployment_key = deployment_key_for_script(&resolved);
        let code_hash_for_link =
            resolve_code_hash_for_hash_type(&all_script_infos, reference_hash, "type")
                .unwrap_or_else(|| reference_hash.to_vec());
        let (deployment_type_hash, deployment_data_hash) = deployment_reference_hashes(&resolved);
        let name = resolved.name.unwrap_or_else(|| "Unknown".to_string());

        let entry = CodeCellScript {
            name,
            code_hash: format!("0x{}", hex::encode(code_hash_for_link)),
            hash_type: hash_type_hint.to_string(),
            deployment_type_hash: deployment_type_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
            deployment_data_hash: deployment_data_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
        };

        if let Some(idx) = deployment_index.get(&deployment_key).copied() {
            let existing = &mut scripts[idx];
            if !is_known_script_name(Some(existing.name.as_str()))
                && is_known_script_name(Some(entry.name.as_str()))
            {
                existing.name = entry.name.clone();
            }
            if existing.hash_type != "type" && entry.hash_type == "type" {
                existing.hash_type = "type".to_string();
                existing.code_hash = entry.code_hash.clone();
            }
            if existing.deployment_type_hash.is_none() {
                existing.deployment_type_hash = entry.deployment_type_hash.clone();
            }
            if existing.deployment_data_hash.is_none() {
                existing.deployment_data_hash = entry.deployment_data_hash.clone();
            }
            return;
        }

        deployment_index.insert(deployment_key, scripts.len());
        scripts.push(entry);
    };

    upsert(data_hash, "data");
    if let Some(type_hash) = type_script_hash {
        upsert(type_hash, "type");
    }

    if scripts.is_empty() {
        None
    } else {
        Some(scripts)
    }
}

fn lookup_dao_info(
    store: &ckbadger_store::CkbadgerStore,
    tx_hash: &[u8],
    output_index: i16,
) -> Option<DaoInfo> {
    let outpoint_key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);

    let entry = store.get_dao_deposit(&outpoint_key).ok()?;

    // If not found by outpoint, try by withdraw_tx
    let entry = if entry.is_none() {
        let outpoint_key_data = store.get_dao_deposit_by_withdraw_tx(tx_hash).ok()?;
        if let Some(key_data) = outpoint_key_data {
            store.get_dao_deposit(&key_data).ok()?
        } else {
            None
        }
    } else {
        entry
    }?;

    let dao_status = match entry.status {
        0 => "deposited",
        1 => "withdrawing",
        2 => "withdrawn",
        _ => "unknown",
    }
    .to_string();

    // Get block header for deposit timestamp
    let deposit_timestamp = store
        .get_block_header(entry.deposit_block_number)
        .ok()
        .flatten()
        .map(|h| {
            chrono::DateTime::from_timestamp(h.timestamp / 1000, 0)
                .unwrap_or_default()
                .to_rfc3339()
        })
        .unwrap_or_default();

    let withdraw_request_timestamp = entry.withdraw_request_block.and_then(|bn| {
        store.get_block_header(bn).ok().flatten().map(|h| {
            chrono::DateTime::from_timestamp(h.timestamp / 1000, 0)
                .unwrap_or_default()
                .to_rfc3339()
        })
    });

    let withdraw_timestamp = entry.withdraw_block.and_then(|bn| {
        store.get_block_header(bn).ok().flatten().map(|h| {
            chrono::DateTime::from_timestamp(h.timestamp / 1000, 0)
                .unwrap_or_default()
                .to_rfc3339()
        })
    });

    let compensation = entry.compensation.map(|c| c.to_string());
    let compensation_ckb = compensation.as_ref().map(|c| shannon_to_ckb(c));

    Some(DaoInfo {
        is_dao_cell: true,
        dao_status,
        deposit_block_number: entry.deposit_block_number,
        deposit_timestamp,
        withdraw_request_block: entry.withdraw_request_block,
        withdraw_request_timestamp,
        withdraw_block: entry.withdraw_block,
        withdraw_timestamp,
        compensation,
        compensation_ckb,
        estimated_apc: None,
    })
}

async fn get_cell(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((tx_hash, output_index)): axum::extract::Path<(String, i32)>,
) -> ApiResult<CellDetailResponse> {
    let hash_bytes = hex::decode(tx_hash.strip_prefix("0x").unwrap_or(&tx_hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let output_idx = output_index as i16;

    // Try live cells first
    let live_cell = state
        .store
        .get_cell(&hash_bytes, output_idx)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Try consumed cells if not found in live
    let consumed_cell = if live_cell.is_none() {
        state
            .store
            .get_consumed_cell_info(&hash_bytes, output_idx)
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        None
    };

    let (info, status_str, consumed_meta) = match (live_cell, consumed_cell) {
        (Some(cell), _) => (cell, "live", None),
        (None, Some(cell)) => (
            cell.cell,
            "dead",
            Some((cell.consumed_at_block, cell.consumed_by_tx)),
        ),
        (None, None) => return Err(ApiError::not_found("Cell not found")),
    };

    // Use the cell's own stored hash_type (not from script_info, which is a canonical
    // default and may differ from the actual per-cell hash_type).
    let lock_hash_type_num: i16 = info.lock_hash_type;

    let hash_type_str = |ht: i16| match ht {
        0 => "data",
        1 => "type",
        2 => "data1",
        4 => "data2",
        _ => "data",
    };

    let type_script = info.type_code_hash.as_ref().map(|code_hash| {
        let type_hash_type_num: i16 = state
            .store
            .get_script_info(code_hash)
            .ok()
            .flatten()
            .map(|si| si.hash_type as i16)
            .unwrap_or(1);
        ScriptResponse {
            code_hash: format!("0x{}", hex::encode(code_hash)),
            hash_type: hash_type_str(type_hash_type_num).to_string(),
            args: format!(
                "0x{}",
                info.type_args
                    .as_ref()
                    .map_or_else(String::new, hex::encode)
            ),
        }
    });

    let address = script_to_address(
        &info.lock_code_hash,
        lock_hash_type_num,
        &info.lock_args,
        &state.ckb_network,
    )
    .ok();

    // For cell data (e.g. dep groups), read from CKB direct store if available
    let cell_data = state.ckb_store.as_ref().and_then(|ckb| {
        if hash_bytes.len() == 32 {
            let mut tx_h = [0u8; 32];
            tx_h.copy_from_slice(&hash_bytes);
            ckb.get_cell_data(&tx_h, output_index as u32)
        } else {
            None
        }
    });

    let dep_group_result = cell_data
        .as_ref()
        .map(|d| parse_dep_group(d, info.data_size))
        .unwrap_or(DepGroupParseResult {
            is_dep_group: false,
            items: None,
        });

    // Compute data_hash from cell data for code_cell lookup
    let data_hash = cell_data.as_ref().map(|d| {
        use ckb_hash::new_blake2b;
        let mut hasher = new_blake2b();
        hasher.update(d);
        let mut hash = vec![0u8; 32];
        hasher.finalize(&mut hash);
        hash
    });

    let code_cell_of = data_hash
        .as_ref()
        .and_then(|dh| lookup_code_cell_scripts(&state.store, dh, info.type_script_hash.as_ref()));

    let data_analysis = cell_data
        .as_ref()
        .map(|d| analyze_cell_data(&info, d, info.data_size));

    let occupied_capacity_breakdown = estimated_occupied_capacity_breakdown(&info);
    let occupied_capacity = if info.occupied_capacity > 0 {
        info.occupied_capacity
    } else {
        occupied_capacity_breakdown
            .total_bytes
            .saturating_mul(SHANNONS_PER_CKB)
    };

    let is_satoshi = is_genesis_special_burn_cell(&info.lock_args, info.created_at_block);
    let (cell_type, virtual_occupied_capacity) = if is_satoshi {
        (
            Some("genesis_special_burn".to_string()),
            Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
        )
    } else {
        (None, None)
    };

    let (consumed_at_block, consumed_by_tx) = if let Some((block, tx)) = consumed_meta {
        let tx_hash = tx.map(|raw| format!("0x{}", hex::encode(raw)));
        (if block > 0 { Some(block) } else { None }, tx_hash)
    } else {
        (None, None)
    };

    let dao_info = lookup_dao_info(&state.store, &hash_bytes, output_idx);

    ok(CellDetailResponse {
        tx_hash: format!("0x{}", hex::encode(&hash_bytes)),
        output_index: output_idx as i32,
        capacity: info.capacity.to_string(),
        occupied_capacity,
        occupied_capacity_breakdown,
        virtual_occupied_capacity,
        cell_type,
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        address,
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        status: status_str.to_string(),
        consumed_at_block,
        consumed_by_tx,
        lock: ScriptResponse {
            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
            hash_type: hash_type_str(lock_hash_type_num).to_string(),
            args: format!("0x{}", hex::encode(&info.lock_args)),
        },
        type_script,
        data: cell_data.map(|d| format!("0x{}", hex::encode(d))),
        data_analysis,
        is_dep_group: dep_group_result.is_dep_group,
        dep_group_items: dep_group_result.items,
        code_cell_of,
        dao_info,
    })
}

async fn get_top_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopAddressesParams>,
) -> ApiResult<Vec<TopAddressResponse>> {
    let sync_status = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if sync_status.address_balances_deferred {
        return ok(Vec::new());
    }

    let limit = params.limit.clamp(1, 500) as usize;

    let rows = state
        .store
        .top_addresses(limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let addresses: Vec<TopAddressResponse> = rows
        .into_iter()
        .filter(|(_, ab)| ab.balance > 0)
        .map(|(lock_hash, ab)| TopAddressResponse {
            lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
            balance: ab.balance.to_string(),
            live_cells_count: ab.live_cells_count,
            transactions_count: ab.txs_count,
        })
        .collect();

    ok(addresses)
}

async fn get_active_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActiveAddressesParams>,
) -> ApiResult<Vec<ActiveAddressResponse>> {
    let sync_status = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if sync_status.address_balances_deferred {
        return ok(Vec::new());
    }

    let limit = params.limit.clamp(1, 500) as usize;
    let days = params.days.clamp(1, 365);

    let tip_block = sync_status.tip_block_number;
    let blocks_per_day: i64 = 8640;
    let min_block = tip_block.saturating_sub(days * blocks_per_day);

    // Full scan of addr_balance CF, filter by last_activity_block
    let iter = state
        .store
        .iterator_cf(state.store.cf_addr_balance(), rocksdb::IteratorMode::Start);

    let mut all: Vec<(Vec<u8>, ckbadger_store::AddressBalance)> = Vec::new();
    for item in iter.flatten() {
        let (key, value) = item;
        if let Ok(ab) = bincode::deserialize::<ckbadger_store::AddressBalance>(&value) {
            if ab.last_activity_block >= min_block {
                all.push((key.to_vec(), ab));
            }
        }
    }

    // Sort by last_activity_block desc
    all.sort_by(|a, b| b.1.last_activity_block.cmp(&a.1.last_activity_block));
    all.truncate(limit);

    let addresses: Vec<ActiveAddressResponse> = all
        .into_iter()
        .map(|(lock_hash, ab)| ActiveAddressResponse {
            lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
            balance: ab.balance.to_string(),
            live_cells_count: ab.live_cells_count,
            transactions_count: ab.txs_count,
            last_activity_block: ab.last_activity_block,
        })
        .collect();

    ok(addresses)
}

async fn get_address_transactions(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AddressTxParams>,
) -> ApiResult<CursorPaginatedResponse<AddressTransactionResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));

    // Fetch recent transactions for this address (newest first)
    let addr_txs = state
        .store
        .list_addr_txs_recent(&lock_hash, limit + 1, cursor)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = addr_txs.len() > limit;
    let addr_txs: Vec<_> = addr_txs.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        addr_txs
            .last()
            .map(|(block_num, tx_idx, _)| encode_cursor(*block_num, *tx_idx))
    } else {
        None
    };

    let txs: Vec<AddressTransactionResponse> = addr_txs
        .into_iter()
        .map(
            |(block_number, tx_idx, tx_hash)| -> Result<
                AddressTransactionResponse,
                (axum::http::StatusCode, axum::Json<ApiError>),
            > {
            let timestamp = state
                .store
                .get_block_header(block_number)
                .ok()
                .flatten()
                .map(|h| {
                    chrono::DateTime::from_timestamp_millis(h.timestamp)
                        .unwrap_or_default()
                        .to_rfc3339()
                })
                .unwrap_or_default();

            let tx_entry = state
                .store
                .get_tx_index(block_number, tx_idx)
                .ok()
                .flatten();
            let is_cellbase = tx_entry.as_ref().map(|e| e.is_cellbase).unwrap_or(false);
            let outputs_count = tx_entry.as_ref().map(|e| e.outputs_count).unwrap_or(0);

            // Compute capacity change: sum outputs to this address minus sum inputs from this address
            let mut output_capacity: i128 = 0;
            let mut input_capacity: i128 = 0;
            let mut has_outputs = false;
            let mut has_inputs = false;
            let mut script_code_hashes: std::collections::HashSet<Vec<u8>> =
                std::collections::HashSet::new();

            // Check outputs belonging to this address
            for idx in 0..outputs_count {
                let cell = state
                    .store
                    .get_cell(&tx_hash, idx)
                    .ok()
                    .flatten()
                    .or_else(|| state.store.get_consumed_cell(&tx_hash, idx).ok().flatten());
                if let Some(cell) = cell {
                    if let Some(ref tch) = cell.type_code_hash {
                        script_code_hashes.insert(tch.clone());
                    }
                    script_code_hashes.insert(cell.lock_code_hash.clone());
                    if cell.lock_script_hash == lock_hash {
                        output_capacity += cell.capacity as i128;
                        has_outputs = true;
                    }
                }
            }

            // Check inputs belonging to this address (resolve previous outpoints)
            let mut dao_compensation: i128 = 0;
            if !is_cellbase {
                if let Some(ref ckb_store) = state.ckb_store {
                    if tx_hash.len() == 32 {
                        let mut tx_hash_arr = [0u8; 32];
                        tx_hash_arr.copy_from_slice(&tx_hash);
                        if let Some(tx_view) = ckb_store.get_transaction(&tx_hash_arr) {
                            use ckb_types::prelude::*;
                            for input in tx_view.inputs().into_iter() {
                                let prev_hash: [u8; 32] =
                                    input.previous_output().tx_hash().unpack();
                                let prev_index: u32 = input.previous_output().index().unpack();
                                // Check if this input is a DAO withdrawal request
                                if let Ok(Some(outpoint_key)) =
                                    state.store.get_dao_deposit_by_withdraw_tx(&prev_hash)
                                {
                                    if let Ok(Some(entry)) =
                                        state.store.get_dao_deposit(&outpoint_key)
                                    {
                                        if let Some(comp) = entry.compensation {
                                            dao_compensation += comp as i128;
                                        }
                                    }
                                }
                                let cell = state
                                    .store
                                    .get_consumed_cell(&prev_hash, prev_index as i16)
                                    .ok()
                                    .flatten()
                                    .or_else(|| {
                                        state
                                            .store
                                            .get_cell(&prev_hash, prev_index as i16)
                                            .ok()
                                            .flatten()
                                    });
                                if let Some(cell) = cell {
                                    if let Some(ref tch) = cell.type_code_hash {
                                        script_code_hashes.insert(tch.clone());
                                    }
                                    script_code_hashes.insert(cell.lock_code_hash.clone());
                                    if cell.lock_script_hash == lock_hash {
                                        input_capacity += cell.capacity as i128;
                                        has_inputs = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let capacity_change = output_capacity - input_capacity;
            let tx_type = if has_inputs && has_outputs {
                if capacity_change < 0 {
                    "sent"
                } else if capacity_change > 0 {
                    "received"
                } else {
                    "internal"
                }
            } else if has_outputs {
                "received"
            } else if has_inputs {
                "sent"
            } else {
                "transfer"
            };

            let inputs_count = tx_entry.as_ref().map(|e| e.inputs_count).unwrap_or(0);
            let stored_fee = tx_entry.as_ref().map(|e| e.fee as i128).unwrap_or(0);
            // For DAO withdrawals, stored fee = actual_fee - compensation (negative).
            // Correct by adding back the DAO compensation.
            let fee_total = stored_fee + dao_compensation;
            if fee_total < 0 {
                return Err(ApiError::internal(format!(
                    "negative corrected transaction fee for tx 0x{} at block {}: stored_fee={}, dao_compensation={}, corrected={}",
                    hex::encode(&tx_hash),
                    block_number,
                    stored_fee,
                    dao_compensation,
                    fee_total
                )));
            }
            let fee = i64::try_from(fee_total).map_err(|_| {
                ApiError::internal(format!(
                    "corrected transaction fee exceeds i64 for tx 0x{} at block {}: {}",
                    hex::encode(&tx_hash),
                    block_number,
                    fee_total
                ))
            })?;
            let tx_size = tx_entry.as_ref().map(|e| e.tx_size);
            let cycles = tx_entry.as_ref().and_then(|e| e.cycles);

            // Resolve script labels from collected code hashes (type + lock scripts)
            let mut script_labels: Vec<String> = script_code_hashes
                .iter()
                .map(|ch| {
                    let known_name = state
                        .store
                        .get_script_info(ch)
                        .ok()
                        .flatten()
                        .and_then(|si| si.name)
                        .map(|name| name.trim().to_string())
                        .filter(|name| is_known_script_label(name));
                    known_name.unwrap_or_else(|| format_script_code_hash_label(ch))
                })
                .filter(|name| {
                    // Filter out common lock scripts that aren't interesting as labels
                    !matches!(
                        name.as_str(),
                        "Default Lock" | "Default Multisig" | "anyone_can_pay"
                    )
                })
                .collect();
            script_labels.sort();
            script_labels.dedup();

            Ok(AddressTransactionResponse {
                tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                block_number,
                tx_type: tx_type.to_string(),
                capacity_change: capacity_change.to_string(),
                timestamp,
                inputs_count,
                outputs_count,
                fee: fee.to_string(),
                is_cellbase,
                tx_size,
                cycles,
                script_labels,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    ok(CursorPaginatedResponse::without_total(
        txs,
        limit as i64,
        next_cursor,
    ))
}

async fn get_address_tokens(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AddressTokensParams>,
) -> ApiResult<CursorPaginatedResponse<AddressTokenResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    // Get all tokens and check balances for this address
    let all_tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut token_balances: Vec<(Vec<u8>, ckbadger_store::TokenInfo, i128)> = Vec::new();

    for (type_hash, token_info) in all_tokens {
        if let Ok(Some(balance)) = state.store.get_token_holder_balance(&type_hash, &lock_hash) {
            if balance > 0 {
                token_balances.push((type_hash, token_info, balance));
            }
        }
    }

    // Sort by balance descending
    token_balances.sort_by(|a, b| b.2.cmp(&a.2));

    let has_more = token_balances.len() > limit;
    let token_balances: Vec<_> = token_balances.into_iter().take(limit).collect();

    let next_cursor: Option<String> = if has_more {
        token_balances
            .last()
            .map(|(type_hash, _, balance)| format!("{}:{}", balance, hex::encode(type_hash)))
    } else {
        None
    };

    let tokens: Vec<AddressTokenResponse> = token_balances
        .into_iter()
        .map(|(type_hash, token_info, balance)| AddressTokenResponse {
            type_script_hash: format!("0x{}", hex::encode(&type_hash)),
            standard: token_info.standard,
            name: token_info.name,
            symbol: token_info.symbol,
            decimals: token_info.decimals.unwrap_or(0) as i16,
            icon_url: token_info.icon_url,
            balance: balance.to_string(),
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        tokens,
        limit as i64,
        next_cursor,
    ))
}

async fn get_address_stats_history(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
) -> ApiResult<crate::routes::statistics::StackedAreaChartResponse> {
    use crate::routes::statistics::{
        StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries,
    };

    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    // Date range: today - 365 days to today
    let now = chrono::Utc::now();
    let today = now.format("%Y%m%d").to_string().parse::<u32>().unwrap_or(0);
    let one_year_ago = (now - chrono::Duration::days(365))
        .format("%Y%m%d")
        .to_string()
        .parse::<u32>()
        .unwrap_or(0);

    let daily_stats = state
        .store
        .list_addr_daily_stats(&lock_hash, one_year_ago, today)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get current live_cells_count to compute baseline
    let addr_balance = state
        .store
        .get_addr_balance(&lock_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let current_live_cells = addr_balance
        .map(|ab| ab.live_cells_count as i64)
        .unwrap_or(0);

    // Sum all cells_delta in range to find baseline
    let total_delta: i64 = daily_stats.iter().map(|(_, s)| s.cells_delta as i64).sum();
    let baseline_live_cells = current_live_cells - total_delta;

    // Build cumulative series
    let mut cum_activities: i64 = 0;
    let mut cum_txs: i64 = 0;
    let mut live_cells = baseline_live_cells;

    let data: Vec<StackedAreaDataPoint> = daily_stats
        .into_iter()
        .map(|(date, stats)| {
            cum_activities += stats.activities as i64;
            cum_txs += stats.txs as i64;
            live_cells += stats.cells_delta as i64;

            let date_str = format!("{}-{}-{}", date / 10000, (date / 100) % 100, date % 100);

            let mut values = std::collections::HashMap::new();
            values.insert(
                "cumulativeActivities".to_string(),
                cum_activities.to_string(),
            );
            values.insert("liveCells".to_string(), live_cells.to_string());
            values.insert("cumulativeTransactions".to_string(), cum_txs.to_string());

            StackedAreaDataPoint {
                date: date_str,
                values,
            }
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "cumulativeActivities".to_string(),
            label: "Cumulative Activities".to_string(),
            color: "#22c55e".to_string(),
        },
        StackedAreaSeries {
            key: "liveCells".to_string(),
            label: "Live Cells".to_string(),
            color: "#f59e0b".to_string(),
        },
        StackedAreaSeries {
            key: "cumulativeTransactions".to_string(),
            label: "Cumulative Transactions".to_string(),
            color: "#8b5cf6".to_string(),
        },
    ];

    ok(StackedAreaChartResponse {
        data,
        series,
        title: "Address Stats History".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::LiveCellInfo;

    fn encode_molecule_bytes(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + input.len());
        out.extend_from_slice(&(input.len() as u32).to_le_bytes());
        out.extend_from_slice(input);
        out
    }

    fn make_spore_data(content_type: &str, content: &[u8], cluster_id: Option<&[u8]>) -> Vec<u8> {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content);
        let cluster_bytes = cluster_id.map(encode_molecule_bytes);

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster = offset_content + content_bytes.len() as u32;
        let total_size = offset_cluster + cluster_bytes.as_ref().map_or(0u32, |b| b.len() as u32);

        let mut out = Vec::new();
        out.extend_from_slice(&total_size.to_le_bytes());
        out.extend_from_slice(&offset_content_type.to_le_bytes());
        out.extend_from_slice(&offset_content.to_le_bytes());
        out.extend_from_slice(&offset_cluster.to_le_bytes());
        out.extend_from_slice(&content_type_bytes);
        out.extend_from_slice(&content_bytes);
        if let Some(bytes) = cluster_bytes {
            out.extend_from_slice(&bytes);
        }
        out
    }

    fn make_cluster_data(name: &str, description: &str) -> Vec<u8> {
        let name_bytes = encode_molecule_bytes(name.as_bytes());
        let desc_bytes = encode_molecule_bytes(description.as_bytes());
        let offset_name = 16u32;
        let offset_desc = offset_name + name_bytes.len() as u32;
        let offset_end = offset_desc + desc_bytes.len() as u32;

        let mut out = Vec::new();
        out.extend_from_slice(&offset_end.to_le_bytes());
        out.extend_from_slice(&offset_name.to_le_bytes());
        out.extend_from_slice(&offset_desc.to_le_bytes());
        out.extend_from_slice(&offset_end.to_le_bytes());
        out.extend_from_slice(&name_bytes);
        out.extend_from_slice(&desc_bytes);
        out
    }

    fn make_mnft_vartext(value: &str) -> Vec<u8> {
        let bytes = value.as_bytes();
        let mut out = Vec::with_capacity(2 + bytes.len());
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
        out
    }

    fn make_info() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 100,
            lock_script_hash: vec![0u8; 32],
            lock_code_hash: vec![1u8; 32],
            lock_hash_type: 1,
            lock_args: vec![2u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
        }
    }

    #[test]
    fn test_parse_dep_group_valid() {
        // 2 outpoints: count(4) + 2 * 36 = 76 bytes
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        // OutPoint 1: 32 bytes tx_hash + 4 bytes index
        data.extend_from_slice(&[1u8; 32]);
        data.extend_from_slice(&0u32.to_le_bytes());
        // OutPoint 2
        data.extend_from_slice(&[2u8; 32]);
        data.extend_from_slice(&1u32.to_le_bytes());

        let result = parse_dep_group(&data, 76);
        assert!(result.is_dep_group);
        assert!(result.items.is_some());
        let items = result.items.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].output_index, 0);
        assert_eq!(items[1].output_index, 1);
    }

    #[test]
    fn test_parse_dep_group_invalid_size() {
        let data = vec![0u8; 10];
        let result = parse_dep_group(&data, 10);
        assert!(!result.is_dep_group);
    }

    #[test]
    fn test_parse_dep_group_zero_count() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        let result = parse_dep_group(&data, 4);
        assert!(!result.is_dep_group);
    }

    #[test]
    fn test_parse_hash_type() {
        assert_eq!(parse_hash_type("data"), Some(0));
        assert_eq!(parse_hash_type("type"), Some(1));
        assert_eq!(parse_hash_type("data1"), Some(2));
        assert_eq!(parse_hash_type("data2"), Some(4));
        assert_eq!(parse_hash_type("invalid"), None);
    }

    #[test]
    fn test_cell_info_to_response_normal() {
        let info = make_info();
        let tx_hash = vec![3u8; 32];
        let resp = cell_info_to_response(&tx_hash, 0, &info);
        assert_eq!(resp.output_index, 0);
        assert_eq!(resp.capacity, "10000000000");
        assert!(resp.cell_type.is_none());
        assert!(resp.virtual_occupied_capacity.is_none());
    }

    #[test]
    fn test_estimated_occupied_capacity_breakdown_without_type_script() {
        let info = LiveCellInfo {
            data_size: 16,
            ..make_info()
        };

        let breakdown = estimated_occupied_capacity_breakdown(&info);
        assert_eq!(breakdown.capacity_field_bytes, 8);
        assert_eq!(breakdown.lock_script_bytes, 53);
        assert_eq!(breakdown.type_script_bytes, 0);
        assert_eq!(breakdown.data_bytes, 16);
        assert_eq!(breakdown.total_bytes, 77);
    }

    #[test]
    fn test_estimated_occupied_capacity_breakdown_with_type_script() {
        let info = LiveCellInfo {
            type_script_hash: Some(vec![3u8; 32]),
            type_code_hash: Some(vec![4u8; 32]),
            type_args: Some(vec![5u8; 24]),
            data_size: 16,
            ..make_info()
        };

        let breakdown = estimated_occupied_capacity_breakdown(&info);
        assert_eq!(breakdown.capacity_field_bytes, 8);
        assert_eq!(breakdown.lock_script_bytes, 53);
        assert_eq!(breakdown.type_script_bytes, 57);
        assert_eq!(breakdown.data_bytes, 16);
        assert_eq!(breakdown.total_bytes, 134);
    }

    #[test]
    fn test_analyze_cell_data_detects_udt_amount_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(SUDT_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x11; 32]),
            data_size: 16,
            ..make_info()
        };
        let mut data = vec![0u8; 16];
        data[0] = 0x2a;

        let analysis = analyze_cell_data(&info, &data, 16);
        let deterministic = analysis.deterministic.expect("deterministic decode");
        assert_eq!(deterministic.kind, "udt_amount");
        assert_eq!(deterministic.segments.len(), 1);
        assert_eq!(deterministic.segments[0].label, "amount");
        assert_eq!(deterministic.segments[0].start, 0);
        assert_eq!(deterministic.segments[0].end, 16);
        assert_eq!(deterministic.segments[0].human_value, "42");
    }

    #[test]
    fn test_analyze_cell_data_detects_spore_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(SPORE_CODE_HASHES[0].trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x11; 32]),
            ..make_info()
        };
        let cluster_id = vec![0xAA; 32];
        let data = make_spore_data("image/png", &[1, 2, 3, 4], Some(cluster_id.as_slice()));

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("spore decode");
        assert_eq!(deterministic.kind, "spore_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "content_type" && s.human_value == "image/png"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "content" && s.human_value == "4 bytes"));
        assert!(deterministic.segments.iter().any(|s| {
            s.label == "cluster_id" && s.human_value == format!("0x{}", hex::encode(&cluster_id))
        }));
    }

    #[test]
    fn test_analyze_cell_data_detects_spore_cluster_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(CLUSTER_CODE_HASHES[0].trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x19; 32]),
            ..make_info()
        };
        let data = make_cluster_data("Genesis Collection", "Primary cluster");

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("spore cluster decode");
        assert_eq!(deterministic.kind, "spore_cluster_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "name" && s.human_value == "Genesis Collection"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "description" && s.human_value == "Primary cluster"));
    }

    #[test]
    fn test_analyze_cell_data_detects_mnft_issuer_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(MNFT_ISSUER_CODE_HASH.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x20; 32]),
            ..make_info()
        };

        let info_blob = br#"{"name":"Issuer-01","info":"demo"}"#;
        let mut data = Vec::new();
        data.push(1); // version
        data.extend_from_slice(&12u32.to_le_bytes()); // class_count
        data.extend_from_slice(&3u32.to_le_bytes()); // set_count
        data.extend_from_slice(&(info_blob.len() as u16).to_le_bytes());
        data.extend_from_slice(info_blob);

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("mnft issuer decode");
        assert_eq!(deterministic.kind, "mnft_issuer_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "class_count" && s.human_value == "12"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "set_count" && s.human_value == "3"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "info_blob" && s.human_value.contains("Issuer-01")));
    }

    #[test]
    fn test_analyze_cell_data_detects_mnft_class_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(MNFT_CLASS_CODE_HASH.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x21; 32]),
            ..make_info()
        };
        let mut data = Vec::new();
        data.push(1); // version
        data.extend_from_slice(&100u32.to_le_bytes()); // total
        data.extend_from_slice(&7u32.to_le_bytes()); // issued
        data.push(0x0f); // configure
        data.extend_from_slice(&make_mnft_vartext("Class-A"));
        data.extend_from_slice(&make_mnft_vartext("Main collection"));
        data.extend_from_slice(&make_mnft_vartext("renderer:v1"));

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("mnft class decode");
        assert_eq!(deterministic.kind, "mnft_class_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "total" && s.human_value == "100"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "name" && s.human_value == "Class-A"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "renderer" && s.human_value == "renderer:v1"));
    }

    #[test]
    fn test_analyze_cell_data_detects_mnft_token_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(MNFT_TOKEN_CODE_HASH.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x22; 32]),
            ..make_info()
        };
        let mut data = Vec::new();
        data.push(2); // version
        data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd]); // characteristic
        data.push(0x81); // configure
        data.push(0x04); // state

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("mnft token decode");
        assert_eq!(deterministic.kind, "mnft_token_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "characteristic" && s.human_value == "0x11223344aabbccdd"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "configure" && s.human_value == "0x81"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "state" && s.human_value == "0x04"));
    }

    #[test]
    fn test_analyze_cell_data_detects_dao_deposit_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(DAO_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x33; 32]),
            ..make_info()
        };
        let data = vec![0u8; 8];

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("dao deposit decode");
        assert_eq!(deterministic.kind, "dao_deposit_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "dao_state" && s.human_value == "deposit"));
    }

    #[test]
    fn test_analyze_cell_data_detects_dao_withdraw_request_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(DAO_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x34; 32]),
            ..make_info()
        };
        let block_number = 987654u64;
        let data = block_number.to_le_bytes().to_vec();

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("dao withdraw decode");
        assert_eq!(deterministic.kind, "dao_withdraw_request_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "dao_state" && s.human_value == "withdraw_request"));
        assert!(deterministic.segments.iter().any(
            |s| s.label == "deposit_block_number" && s.human_value == block_number.to_string()
        ));
    }

    #[test]
    fn test_analyze_cell_data_detects_dep_group_segments() {
        let info = make_info();
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[0xAB; 32]);
        data.extend_from_slice(&3u32.to_le_bytes());

        let analysis = analyze_cell_data(&info, &data, 40);
        let deterministic = analysis.deterministic.expect("dep group decode");
        assert_eq!(deterministic.kind, "dep_group_out_point_vec");
        assert_eq!(deterministic.segments[0].label, "count");
        assert_eq!(deterministic.segments[0].human_value, "1");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "outpoint[0].tx_hash"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "outpoint[0].output_index"));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_png_guess() {
        let info = make_info();
        let data = b"\x89PNG\r\n\x1a\nhello".to_vec();

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis.deterministic.is_none());
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("image/png")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_wasm_guess() {
        let info = make_info();
        let data = b"\0asm\x01\0\0\0".to_vec();

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("application/wasm")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_zip_guess() {
        let info = make_info();
        let data = vec![0x50, 0x4B, 0x03, 0x04, 0x14, 0x00];

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("application/zip")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_gzip_guess() {
        let info = make_info();
        let data = vec![0x1F, 0x8B, 0x08, 0x00];

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("application/gzip")));
    }

    #[test]
    fn test_analyze_cell_data_does_not_force_dep_group_for_typed_unknown_cells() {
        let info = LiveCellInfo {
            type_code_hash: Some(vec![0x99; 32]),
            type_script_hash: Some(vec![0x12; 32]),
            ..make_info()
        };
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[0xAB; 32]);
        data.extend_from_slice(&3u32.to_le_bytes());

        let analysis = analyze_cell_data(&info, &data, 40);
        assert!(analysis.deterministic.is_none());
    }

    #[test]
    fn test_is_known_script_label() {
        assert!(!is_known_script_label("unknown"));
        assert!(!is_known_script_label(" Unknown "));
        assert!(!is_known_script_label(" "));
        assert!(is_known_script_label("Secp256k1"));
    }

    #[test]
    fn test_format_script_code_hash_label() {
        let label = format_script_code_hash_label(&[0xAB; 32]);
        assert_eq!(label, "script:0xababababab...abababab");
    }
}
