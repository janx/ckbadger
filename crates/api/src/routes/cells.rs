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
use crate::warmup::{
    CachedAddressEntry, CachedAssetEntry, CACHE_KEY_ADDRESSES_ACTIVE, CACHE_KEY_ADDRESSES_TOP,
    CACHE_KEY_ASSETS_TOKEN, CACHE_KEY_SCRIPTS_ALL,
};
use crate::AppState;
use ckbadger_store::{keys, CkbadgerStore};

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
const ADDR_TX_SCAN_CHUNK_SIZE: usize = 128;

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

    let (content_start, content_end, content_bytes) =
        read_molecule_bytes_field(data, offset_content, offset_cluster_id)?;
    let content_human_value = summarize_spore_content_human_value(&content_type, &content_bytes);
    segments.push(CellDataSegment {
        label: "content".to_string(),
        start: content_start as i32,
        end: content_end as i32,
        meaning: "Spore binary payload".to_string(),
        human_value: content_human_value,
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

fn summarize_spore_content_human_value(content_type: &str, content: &[u8]) -> String {
    let normalized = content_type.trim().to_ascii_lowercase();
    let text_like = normalized.starts_with("text/")
        || normalized.contains("json")
        || normalized.contains("xml")
        || normalized.contains("javascript")
        || normalized.contains("dob/");

    if text_like {
        if let Some(text) = parse_readable_utf8(content) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return format!(
                    "{} ({} bytes)",
                    truncate_for_preview(trimmed, 120),
                    content.len()
                );
            }
        }
    }

    format!("{} bytes", content.len())
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
    let len = u16::from_be_bytes(data[offset..offset + 2].try_into().ok()?) as usize;
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
        let class_count = u32::from_be_bytes(data[1..5].try_into().ok()?);
        let set_count = u32::from_be_bytes(data[5..9].try_into().ok()?);

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
                meaning: "Number of classes under this issuer (u32 BE)".to_string(),
                human_value: class_count.to_string(),
            },
            CellDataSegment {
                label: "set_count".to_string(),
                start: 5,
                end: 9,
                meaning: "Number of sets under this issuer (u32 BE)".to_string(),
                human_value: set_count.to_string(),
            },
        ];

        if data.len() >= 11 {
            let info_size = u16::from_be_bytes(data[9..11].try_into().ok()?) as usize;
            segments.push(CellDataSegment {
                label: "info_size".to_string(),
                start: 9,
                end: 11,
                meaning: "Length of issuer metadata blob (u16 BE)".to_string(),
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
        let total = u32::from_be_bytes(data[1..5].try_into().ok()?);
        let issued = u32::from_be_bytes(data[5..9].try_into().ok()?);
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
                meaning: "Class max supply (u32 BE)".to_string(),
                human_value: total.to_string(),
            },
            CellDataSegment {
                label: "issued".to_string(),
                start: 5,
                end: 9,
                meaning: "Class issued count (u32 BE)".to_string(),
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

fn parse_readable_utf8(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    if text
        .chars()
        .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
    {
        return Some(text.to_string());
    }
    None
}

fn truncate_for_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("...");
    out
}

fn parse_molecule_table_shape(data: &[u8]) -> Option<(usize, usize)> {
    if data.len() < 8 {
        return None;
    }

    let total_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if total_size != data.len() {
        return None;
    }

    let first_offset = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    if !(8..=total_size).contains(&first_offset) || first_offset % 4 != 0 {
        return None;
    }

    let field_count = first_offset / 4;
    if !(2..=256).contains(&field_count) {
        return None;
    }

    if data.len() < field_count * 4 {
        return None;
    }

    let mut prev = first_offset;
    for i in 2..field_count {
        let offset = u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().ok()?) as usize;
        if offset < prev || offset > total_size || offset % 4 != 0 {
            return None;
        }
        prev = offset;
    }

    Some((field_count, total_size))
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

    if data.iter().all(|b| *b == 0) {
        guesses.push(CellDataGuess {
            kind: "zero_pattern".to_string(),
            confidence: "high".to_string(),
            reason: "Payload is entirely zero bytes".to_string(),
            mime_type: None,
            human_value: Some(format!("{} zero bytes", data.len())),
        });
    }

    if let Some((field_count, total_size)) = parse_molecule_table_shape(data) {
        guesses.push(CellDataGuess {
            kind: "structure_pattern".to_string(),
            confidence: "medium".to_string(),
            reason: "Looks like Molecule table layout (LE total size + monotonic offsets)"
                .to_string(),
            mime_type: Some("application/x-molecule-table".to_string()),
            human_value: Some(format!(
                "total_size={} bytes, fields={}",
                total_size, field_count
            )),
        });
    }

    if data.len() == 4 {
        let value = u32::from_le_bytes(
            data.try_into()
                .expect("data length already checked as exactly 4 bytes"),
        );
        guesses.push(CellDataGuess {
            kind: "numeric_pattern".to_string(),
            confidence: "medium".to_string(),
            reason: "Payload length is exactly 4 bytes (common u32 LE encoding)".to_string(),
            mime_type: None,
            human_value: Some(value.to_string()),
        });
    }

    if data.len() == 8 {
        let value = u64::from_le_bytes(
            data.try_into()
                .expect("data length already checked as exactly 8 bytes"),
        );
        guesses.push(CellDataGuess {
            kind: "numeric_pattern".to_string(),
            confidence: "medium".to_string(),
            reason: "Payload length is exactly 8 bytes (common u64 LE encoding)".to_string(),
            mime_type: None,
            human_value: Some(value.to_string()),
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
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("<svg") || (lower.starts_with("<?xml") && lower.contains("<svg")) {
                guesses.push(CellDataGuess {
                    kind: "text_pattern".to_string(),
                    confidence: "high".to_string(),
                    reason: "UTF-8 payload looks like SVG/XML markup".to_string(),
                    mime_type: Some("image/svg+xml".to_string()),
                    human_value: Some(truncate_for_preview(trimmed, 120)),
                });
            }

            if lower.starts_with("ipfs://")
                || lower.starts_with("https://")
                || lower.starts_with("http://")
                || lower.starts_with("did:ckb:")
                || lower.starts_with("ckb://")
            {
                guesses.push(CellDataGuess {
                    kind: "text_pattern".to_string(),
                    confidence: "medium".to_string(),
                    reason: "UTF-8 payload looks like a URI".to_string(),
                    mime_type: Some("text/uri-list".to_string()),
                    human_value: Some(truncate_for_preview(trimmed, 120)),
                });
            }

            guesses.push(CellDataGuess {
                kind: "text_pattern".to_string(),
                confidence: "medium".to_string(),
                reason: "Payload is printable UTF-8 text".to_string(),
                mime_type: Some("text/plain".to_string()),
                human_value: Some(truncate_for_preview(trimmed, 120)),
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

fn build_script_hint_guess(
    info: &ckbadger_store::LiveCellInfo,
    data: &[u8],
) -> Option<CellDataGuess> {
    let type_code_hash = info.type_code_hash.as_ref()?;

    let (label, expectation, confidence) = if is_dao_type_code_hash(type_code_hash) {
        (
            "DAO",
            "expected exactly 8 bytes (deposit marker or deposit block number)",
            "high",
        )
    } else if is_spore_type_code_hash(type_code_hash) {
        (
            "Spore",
            "expected Molecule table layout: [total_size, offsets, content_type, content, optional cluster_id]",
            "high",
        )
    } else if is_cluster_type_code_hash(type_code_hash) {
        (
            "Spore Cluster",
            "expected Molecule table layout: [total_size, offsets, name, description, mutant_id]",
            "high",
        )
    } else if is_mnft_issuer_type_code_hash(type_code_hash) {
        (
            "mNFT Issuer",
            "expected binary layout: version(1) + class_count(4) + set_count(4) + optional info blob",
            "medium",
        )
    } else if is_mnft_class_type_code_hash(type_code_hash) {
        (
            "mNFT Class",
            "expected binary layout: version(1) + total(4) + issued(4) + configure(1) + vartext fields",
            "medium",
        )
    } else if is_mnft_token_type_code_hash(type_code_hash) {
        (
            "mNFT Token",
            "expected binary layout: version(1) + characteristic(8) + configure(1) + state(1)",
            "medium",
        )
    } else if let Some(standard) = detect_udt_standard_from_code_hash(type_code_hash) {
        let upper = standard.to_uppercase();
        return Some(CellDataGuess {
            kind: "script_hint".to_string(),
            confidence: if data.len() < 16 {
                "high".to_string()
            } else {
                "medium".to_string()
            },
            reason: format!(
                "Type script indicates {} but payload does not decode as canonical {} amount",
                upper, upper
            ),
            mime_type: None,
            human_value: Some(format!(
                "{}; observed length={} bytes",
                "expected first 16 bytes as little-endian u128 amount",
                data.len()
            )),
        });
    } else if is_dotbit_account_type_code_hash(type_code_hash) {
        (
            "dotbit account",
            "expected at least 52 bytes: account_hash(32) + account_id(20)",
            "high",
        )
    } else {
        return None;
    };

    Some(CellDataGuess {
        kind: "script_hint".to_string(),
        confidence: confidence.to_string(),
        reason: format!(
            "Type script indicates {} but payload does not match expected layout",
            label
        ),
        mime_type: None,
        human_value: Some(format!(
            "{}; observed length={} bytes",
            expectation,
            data.len()
        )),
    })
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

    let mut heuristic_guesses = build_heuristic_guesses(data);
    if deterministic.is_none() {
        if let Some(script_hint) = build_script_hint_guess(info, data) {
            heuristic_guesses.insert(0, script_hint);
        }
    }

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
        udt_amount: info.udt_amount.map(|amount| amount.to_string()),
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
                    .list_cells_by_lock(
                        lock_bytes,
                        limit * 10 + 1,
                        after_key_ref,
                        &state.append_only_store,
                    )
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
                        .list_cells_by_lock(
                            lock_bytes,
                            limit * 10 + 1,
                            after_key_ref,
                            &state.append_only_store,
                        )
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
                        .list_cells_by_lock(
                            lock_bytes,
                            limit + 1,
                            after_key_ref,
                            &state.append_only_store,
                        )
                        .map_err(|e| ApiError::internal(e.to_string()))?
                }
            }
            (None, Some(type_bytes)) => state
                .store
                .list_cells_by_type(
                    type_bytes,
                    limit + 1,
                    after_key_ref,
                    &state.append_only_store,
                )
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

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<ckbadger_store::ScriptInfo>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .map(|rows| rows.into_iter().map(|(_, info)| info).collect())
        .ok_or_else(|| ApiError::internal("script cache unavailable; warmup in progress"))
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

    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(&state)?;

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
            .list_cells_by_lock_code_hash(
                &resolved_code_hash,
                fetch_limit,
                after_key_ref,
                &state.append_only_store,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?,
        "type" => state
            .store
            .list_cells_by_type_code_hash(
                &resolved_code_hash,
                fetch_limit,
                after_key_ref,
                &state.append_only_store,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?,
        _ => {
            // "both": merge results from lock and type indexes
            let mut merged = state
                .store
                .list_cells_by_lock_code_hash(
                    &resolved_code_hash,
                    fetch_limit,
                    after_key_ref,
                    &state.append_only_store,
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let type_results = state
                .store
                .list_cells_by_type_code_hash(
                    &resolved_code_hash,
                    fetch_limit,
                    after_key_ref,
                    &state.append_only_store,
                )
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
        .list_cells_by_lock(&lock_hash, 1, None, &state.append_only_store)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (lock_script, lock_script_info, address) =
        if let Some((_, _, info)) = cells_for_script.first() {
            let script_info = state
                .store
                .get_script_info(&info.lock_code_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?;

            // LiveCellInfo doesn't store hash_type directly; derive from code_hash via script_info
            let hash_type_num = script_info
                .as_ref()
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

            let script_info_response = script_info.map(|si| LockScriptInfo {
                code_hash: format!("0x{}", hex::encode(&si.code_hash)),
                name: si.name.unwrap_or_else(|| "Unknown".to_string()),
                script_kind: Some("lock".to_string()),
                deprecated: false,
            });

            (Some(script), script_info_response, addr)
        } else {
            // No live cells found, also check consumed cells for script info.
            // For now, just return what we have.
            (None, None, input_address)
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
    state: &Arc<AppState>,
    data_hash: &[u8],
    type_script_hash: Option<&Vec<u8>>,
) -> Result<Option<Vec<CodeCellScript>>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(state)?;

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
        Ok(None)
    } else {
        Ok(Some(scripts))
    }
}

fn lookup_dao_info(
    store: &ckbadger_store::CkbadgerStore,
    tx_hash: &[u8],
    output_index: i16,
) -> Option<DaoInfo> {
    let outpoint_key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);

    let entry = store.get_dao_deposit(&outpoint_key).ok()?;

    // If not found by outpoint, try by withdraw-request/withdraw-complete tx hash
    let entry = if entry.is_none() {
        let outpoint_key_data = store
            .get_dao_deposit_by_withdraw_tx(tx_hash, output_index)
            .ok()?;
        if let Some(key_data) = outpoint_key_data {
            let candidate = store.get_dao_deposit(&key_data).ok()??;
            let matches_withdraw_request =
                candidate.withdraw_request_output_index == Some(output_index);
            let matches_withdraw_to = candidate.withdraw_to_output_index == Some(output_index);
            if matches_withdraw_request || matches_withdraw_to {
                Some(candidate)
            } else {
                None
            }
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
        .get_cell(&hash_bytes, output_idx, &state.append_only_store)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Try consumed cells if not found in live
    let consumed_cell = if live_cell.is_none() {
        state
            .store
            .get_consumed_cell_info(&hash_bytes, output_idx, &state.append_only_store)
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

    let type_script = if let Some(code_hash) = info.type_code_hash.as_ref() {
        let type_hash_type_num: i16 = state
            .store
            .get_script_info(code_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map(|si| si.hash_type as i16)
            .unwrap_or(1);
        Some(ScriptResponse {
            code_hash: format!("0x{}", hex::encode(code_hash)),
            hash_type: hash_type_str(type_hash_type_num).to_string(),
            args: format!(
                "0x{}",
                info.type_args
                    .as_ref()
                    .map_or_else(String::new, hex::encode)
            ),
        })
    } else {
        None
    };

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

    let code_cell_of = if let Some(dh) = data_hash.as_ref() {
        lookup_code_cell_scripts(&state, dh, info.type_script_hash.as_ref())?
    } else {
        None
    };

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
    let limit = params.limit.clamp(1, 500) as usize;

    if let Some(cached) = state
        .mem_cache
        .get::<Vec<CachedAddressEntry>>(CACHE_KEY_ADDRESSES_TOP)
    {
        return ok(top_addresses_from_cache(cached, limit));
    }
    Err(ApiError::internal(
        "top addresses cache unavailable; warmup in progress",
    ))
}

async fn get_active_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActiveAddressesParams>,
) -> ApiResult<Vec<ActiveAddressResponse>> {
    let sync_status = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let limit = params.limit.clamp(1, 500) as usize;
    let days = params.days.clamp(1, 365);

    let tip_block = sync_status.tip_block_number;
    let blocks_per_day: i64 = 8640;
    let min_block = tip_block.saturating_sub(days * blocks_per_day);

    if let Some(cached) = state
        .mem_cache
        .get::<Vec<CachedAddressEntry>>(CACHE_KEY_ADDRESSES_ACTIVE)
    {
        return ok(active_addresses_from_cache(cached, min_block, limit));
    }
    Err(ApiError::internal(
        "active addresses cache unavailable; warmup in progress",
    ))
}

fn top_addresses_from_cache(
    cached: Vec<CachedAddressEntry>,
    limit: usize,
) -> Vec<TopAddressResponse> {
    cached
        .into_iter()
        .take(limit)
        .map(|entry| TopAddressResponse {
            lock_script_hash: entry.lock_script_hash,
            balance: entry.balance,
            live_cells_count: entry.live_cells_count,
            transactions_count: entry.transactions_count,
        })
        .collect()
}

fn active_addresses_from_cache(
    cached: Vec<CachedAddressEntry>,
    min_block: i64,
    limit: usize,
) -> Vec<ActiveAddressResponse> {
    cached
        .into_iter()
        .filter(|entry| entry.last_activity_block >= min_block)
        .take(limit)
        .map(|entry| ActiveAddressResponse {
            lock_script_hash: entry.lock_script_hash,
            balance: entry.balance,
            live_cells_count: entry.live_cells_count,
            transactions_count: entry.transactions_count,
            last_activity_block: entry.last_activity_block,
        })
        .collect()
}

fn is_canonical_addr_tx(
    store: &CkbadgerStore,
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
) -> anyhow::Result<bool> {
    let Some((canonical_block, canonical_tx_idx)) = store.get_tx_location(tx_hash)? else {
        return Ok(false);
    };
    if canonical_block != block_num || canonical_tx_idx != tx_idx {
        return Ok(false);
    }
    Ok(store
        .get_tx_index(canonical_block, canonical_tx_idx)?
        .is_some())
}

fn list_canonical_addr_txs_page(
    store: &CkbadgerStore,
    addr_tx_store: &CkbadgerStore,
    lock_hash: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let scan_limit = ADDR_TX_SCAN_CHUNK_SIZE.max(limit);
    let mut out = Vec::with_capacity(limit);
    let mut scan_cursor = cursor;

    loop {
        let rows = addr_tx_store.list_addr_txs_recent(lock_hash, scan_limit, scan_cursor)?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let mut last_seen = None;
        for (block_num, tx_idx, tx_hash) in rows {
            last_seen = Some((block_num, tx_idx));
            if is_canonical_addr_tx(store, block_num, tx_idx, &tx_hash)? {
                out.push((block_num, tx_idx, tx_hash));
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
        if rows_len < scan_limit {
            break;
        }
        let Some(last_seen_cursor) = last_seen else {
            break;
        };
        scan_cursor = Some(last_seen_cursor);
    }

    Ok(out)
}

fn load_cells_preferring_consumed(
    store: &CkbadgerStore,
    outpoints: &[(&[u8], i16)],
    cells_store: &CkbadgerStore,
) -> Result<
    HashMap<(Vec<u8>, i16), ckbadger_store::LiveCellInfo>,
    (axum::http::StatusCode, axum::Json<ApiError>),
> {
    if outpoints.is_empty() {
        return Ok(HashMap::new());
    }

    let mut cells = store
        .get_cells_batch(outpoints, cells_store)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let consumed_cells = store
        .get_consumed_cells_batch(outpoints, cells_store)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    for (outpoint, cell) in consumed_cells {
        cells.insert(outpoint, cell);
    }
    Ok(cells)
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

    // Fetch canonical recent transactions for this address (newest first).
    let addr_txs = list_canonical_addr_txs_page(
        state.store.as_ref(),
        state.store.as_ref(),
        &lock_hash,
        limit + 1,
        cursor,
    )
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

            // Check outputs belonging to this address via batch cell lookups.
            let output_outpoints: Vec<(&[u8], i16)> = (0..outputs_count)
                .map(|output_index| (tx_hash.as_slice(), output_index))
                .collect();
            let output_cells = load_cells_preferring_consumed(state.store.as_ref(), &output_outpoints, state.append_only_store.as_ref())?;
            for cell in output_cells.values() {
                if let Some(ref tch) = cell.type_code_hash {
                    script_code_hashes.insert(tch.clone());
                }
                script_code_hashes.insert(cell.lock_code_hash.clone());
                if cell.lock_script_hash == lock_hash {
                    output_capacity += cell.capacity as i128;
                    has_outputs = true;
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
                            let mut input_outpoints: Vec<(Vec<u8>, i16)> = Vec::new();
                            for input in tx_view.inputs().into_iter() {
                                let prev_hash: [u8; 32] =
                                    input.previous_output().tx_hash().unpack();
                                let prev_index_u32: u32 = input.previous_output().index().unpack();
                                let prev_index =
                                    i16::try_from(prev_index_u32).map_err(|_| {
                                        ApiError::internal(format!(
                                            "input output index exceeds i16 for tx 0x{} at block {}: prev_index={}",
                                            hex::encode(&tx_hash),
                                            block_number,
                                            prev_index_u32
                                        ))
                                    })?;
                                input_outpoints.push((prev_hash.to_vec(), prev_index));
                            }

                            let input_refs: Vec<(&[u8], i16)> = input_outpoints
                                .iter()
                                .map(|(prev_hash, prev_index)| (prev_hash.as_slice(), *prev_index))
                                .collect();
                            let input_cells =
                                load_cells_preferring_consumed(state.store.as_ref(), &input_refs, state.append_only_store.as_ref())?;

                            for (prev_hash, prev_index) in input_outpoints {
                                // Check if this input is a DAO withdrawal request
                                if let Ok(Some(outpoint_key)) =
                                    state.store.get_dao_deposit_by_withdraw_tx(&prev_hash, prev_index)
                                {
                                    if let Ok(Some(entry)) =
                                        state.store.get_dao_deposit(&outpoint_key)
                                    {
                                        if let Some(comp) = entry.compensation {
                                            dao_compensation += comp as i128;
                                        }
                                    }
                                }
                                let cell = input_cells.get(&(prev_hash.clone(), prev_index));
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
                .map(
                    |ch| -> Result<String, (axum::http::StatusCode, axum::Json<ApiError>)> {
                    let known_name = state
                        .store
                        .get_script_info(ch)
                        .map_err(|e| ApiError::internal(e.to_string()))?
                        .and_then(|si| si.name)
                        .map(|name| name.trim().to_string())
                        .filter(|name| is_known_script_label(name));
                    Ok(known_name.unwrap_or_else(|| format_script_code_hash_label(ch)))
                    },
                )
                .collect::<Result<Vec<_>, _>>()?;
            // Filter out common lock scripts that aren't interesting as labels.
            script_labels.retain(|name| {
                !matches!(
                    name.as_str(),
                    "Default Lock" | "Default Multisig" | "anyone_can_pay"
                )
            });
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

    let cached_tokens = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN)
        .ok_or_else(|| ApiError::internal("token cache unavailable; warmup in progress"))?;

    let mut token_balances: Vec<(String, CachedAssetEntry, i128)> = Vec::new();

    for token_info in cached_tokens {
        let type_hash = hex::decode(
            token_info
                .id
                .strip_prefix("0x")
                .unwrap_or(token_info.id.as_str()),
        )
        .map_err(|_| {
            ApiError::internal(format!(
                "invalid token hash in warmup cache: {}",
                token_info.id
            ))
        })?;
        if let Ok(Some(balance)) = state.store.get_token_holder_balance(&type_hash, &lock_hash) {
            if balance > 0 {
                token_balances.push((token_info.id.clone(), token_info, balance));
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
            .map(|(type_hash, _, balance)| format!("{}:{}", balance, type_hash))
    } else {
        None
    };

    let tokens: Vec<AddressTokenResponse> = token_balances
        .into_iter()
        .map(|(type_hash, token_info, balance)| AddressTokenResponse {
            type_script_hash: type_hash,
            standard: token_info.standard,
            name: token_info.name,
            symbol: token_info.symbol,
            decimals: token_info.decimals.unwrap_or(0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::LiveCellInfo;
    use ckbadger_store::TxIndexEntry;

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
        out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
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
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
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
        assert!(resp.udt_amount.is_none());
    }

    #[test]
    fn test_cell_info_to_response_preserves_udt_amount() {
        let info = LiveCellInfo {
            udt_amount: Some(12_345),
            ..make_info()
        };
        let tx_hash = vec![4u8; 32];
        let resp = cell_info_to_response(&tx_hash, 1, &info);
        assert_eq!(resp.udt_amount.as_deref(), Some("12345"));
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
        data.extend_from_slice(&12u32.to_be_bytes()); // class_count
        data.extend_from_slice(&3u32.to_be_bytes()); // set_count
        data.extend_from_slice(&(info_blob.len() as u16).to_be_bytes());
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
        data.extend_from_slice(&100u32.to_be_bytes()); // total
        data.extend_from_slice(&7u32.to_be_bytes()); // issued
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
    fn test_analyze_cell_data_detects_mnft_class_segments_big_endian_layout() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(MNFT_CLASS_CODE_HASH.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x23; 32]),
            ..make_info()
        };

        // version(1) + total(4 BE) + issued(4 BE) + configure(1)
        // + name_len(2 BE) + name + desc_len(2 BE) + desc + renderer_len(2 BE)
        let data = hex::decode("000000001400000014c0000a466972737420537465700004646573630000")
            .expect("valid hex");

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("mnft class decode");
        assert_eq!(deterministic.kind, "mnft_class_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "total" && s.human_value == "20"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "issued" && s.human_value == "20"));
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "name" && s.human_value == "First Step"));
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
    fn test_analyze_cell_data_spore_text_content_exposes_preview() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(SPORE_CODE_HASHES[0].trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x41; 32]),
            ..make_info()
        };
        let data = make_spore_data("text/plain", b"hello spore text", None);

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        let deterministic = analysis.deterministic.expect("spore decode");
        let content_segment = deterministic
            .segments
            .iter()
            .find(|s| s.label == "content")
            .expect("content segment");
        assert!(content_segment.human_value.contains("hello spore text"));
        assert!(content_segment.human_value.contains("16 bytes"));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_molecule_table_guess() {
        let info = make_info();
        let mut data = Vec::new();
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        data.extend_from_slice(&[0x55, 0x66, 0x77, 0x88]);

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis.heuristic_guesses.iter().any(|g| {
            g.mime_type.as_deref() == Some("application/x-molecule-table")
                && g.human_value
                    .as_deref()
                    .is_some_and(|v| v.contains("fields=2"))
        }));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_svg_guess() {
        let info = make_info();
        let data = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec();

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("image/svg+xml")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_uri_guess() {
        let info = make_info();
        let data = b"ipfs://bafybeigdyrztm".to_vec();

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("text/uri-list")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_u32_u64_zero_guesses() {
        let info = make_info();

        let u32_data = 12345u32.to_le_bytes().to_vec();
        let u32_analysis = analyze_cell_data(&info, &u32_data, u32_data.len() as i32);
        assert!(u32_analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.kind == "numeric_pattern" && g.human_value.as_deref() == Some("12345")));

        let u64_data = 0u64.to_le_bytes().to_vec();
        let u64_analysis = analyze_cell_data(&info, &u64_data, u64_data.len() as i32);
        assert!(u64_analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.kind == "zero_pattern"));
        assert!(u64_analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.kind == "numeric_pattern" && g.human_value.as_deref() == Some("0")));
    }

    #[test]
    fn test_analyze_cell_data_builds_script_hint_for_short_dao_payload() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(DAO_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x51; 32]),
            ..make_info()
        };
        let data = vec![0u8; 4];

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis.deterministic.is_none());
        assert!(analysis.heuristic_guesses.iter().any(|g| {
            g.kind == "script_hint"
                && g.reason.contains("DAO")
                && g.human_value
                    .as_deref()
                    .is_some_and(|v| v.contains("observed length=4 bytes"))
        }));
    }

    #[test]
    fn test_analyze_cell_data_builds_script_hint_for_short_dotbit_payload() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(DOTBIT_ACCOUNT_CELL_TYPE_ID.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x52; 32]),
            ..make_info()
        };
        let data = vec![0u8; 20];

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis.deterministic.is_none());
        assert!(analysis.heuristic_guesses.iter().any(|g| {
            g.kind == "script_hint"
                && g.reason.contains("dotbit")
                && g.human_value
                    .as_deref()
                    .is_some_and(|v| v.contains("at least 52 bytes"))
        }));
    }

    #[test]
    fn test_analyze_cell_data_builds_script_hint_for_short_udt_payload() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(SUDT_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x53; 32]),
            ..make_info()
        };
        let data = vec![0u8; 8];

        let analysis = analyze_cell_data(&info, &data, data.len() as i32);
        assert!(analysis.deterministic.is_none());
        assert!(analysis.heuristic_guesses.iter().any(|g| {
            g.kind == "script_hint"
                && g.reason.contains("SUDT")
                && g.human_value
                    .as_deref()
                    .is_some_and(|v| v.contains("first 16 bytes"))
        }));
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

    #[test]
    fn test_list_canonical_addr_txs_page_filters_orphaned_entries() {
        let root = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let lock_hash = [0xAA; 32];

        let stale_tx = vec![0x30; 32];
        let canonical_tx_new = vec![0x20; 32];
        let canonical_tx_old = vec![0x10; 32];

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };
        let mut domain_batch = StoreBatch::new(&domain);
        domain_batch.put_addr_tx(&lock_hash, 30, 0, &stale_tx);
        domain_batch.put_addr_tx(&lock_hash, 20, 0, &canonical_tx_new);
        domain_batch.put_addr_tx(&lock_hash, 10, 0, &canonical_tx_old);
        domain_batch.put_tx_hash_map(&stale_tx, 30, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_new, 20, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_old, 10, 0);
        domain_batch.put_tx_index(20, 0, &tx_index);
        domain_batch.put_tx_index(10, 0, &tx_index);
        domain_batch.commit().unwrap();

        let rows = list_canonical_addr_txs_page(&domain, &domain, &lock_hash, 3, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 20);
        assert_eq!(rows[1].0, 10);
        assert_eq!(rows[0].2, canonical_tx_new);
        assert_eq!(rows[1].2, canonical_tx_old);
    }

    #[test]
    fn test_top_addresses_from_cache_respects_limit_order() {
        let cached = vec![
            CachedAddressEntry {
                lock_script_hash: "0x01".to_string(),
                balance: "300".to_string(),
                live_cells_count: 3,
                transactions_count: 30,
                last_activity_block: 100,
            },
            CachedAddressEntry {
                lock_script_hash: "0x02".to_string(),
                balance: "200".to_string(),
                live_cells_count: 2,
                transactions_count: 20,
                last_activity_block: 90,
            },
        ];
        let rows = top_addresses_from_cache(cached, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lock_script_hash, "0x01");
        assert_eq!(rows[0].balance, "300");
    }

    #[test]
    fn test_active_addresses_from_cache_filters_by_min_block() {
        let cached = vec![
            CachedAddressEntry {
                lock_script_hash: "0x01".to_string(),
                balance: "100".to_string(),
                live_cells_count: 1,
                transactions_count: 10,
                last_activity_block: 100,
            },
            CachedAddressEntry {
                lock_script_hash: "0x02".to_string(),
                balance: "90".to_string(),
                live_cells_count: 1,
                transactions_count: 9,
                last_activity_block: 80,
            },
        ];
        let rows = active_addresses_from_cache(cached, 90, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lock_script_hash, "0x01");
        assert_eq!(rows[0].last_activity_block, 100);
    }

    #[test]
    fn test_load_cells_preferring_consumed_merges_live_and_consumed_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let tx_live = [0x11u8; 32];
        let tx_consumed = [0x22u8; 32];

        let live_cell = LiveCellInfo {
            capacity: 42_00000000,
            ..make_info()
        };
        let consumed_cell = LiveCellInfo {
            capacity: 99_00000000,
            ..make_info()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_live, 0, &live_cell);
        batch.put_cell(&tx_consumed, 0, &consumed_cell);
        batch.put_consumed_cell_with_consumer(&tx_consumed, 0, &consumed_cell, 123, None);
        batch.delete_cell(&tx_consumed, 0);
        batch.commit().unwrap();

        let outpoints: Vec<(&[u8], i16)> = vec![(&tx_live, 0), (&tx_consumed, 0)];
        let cells = load_cells_preferring_consumed(&store, &outpoints, &store).unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(
            cells.get(&(tx_live.to_vec(), 0)).map(|c| c.capacity),
            Some(42_00000000)
        );
        assert_eq!(
            cells.get(&(tx_consumed.to_vec(), 0)).map(|c| c.capacity),
            Some(99_00000000)
        );
    }
}
