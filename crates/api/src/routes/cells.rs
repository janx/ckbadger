#![allow(clippy::type_complexity)]
#![allow(clippy::manual_is_multiple_of)]

#[path = "cell_anatomy.rs"]
mod cell_anatomy;
#[path = "cell_elf.rs"]
mod cell_elf;

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use ckb_types::prelude::Entity;
use ckbadger_common::TokenBalance;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{
    default_limit, encode_cursor, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse, ScriptResponse,
};
use crate::utils::address::compute_script_hash;
use crate::utils::script_resolution::{list_version_code_cells, CurrentScriptVersionResolution};
use crate::utils::{
    address_to_lock_script_hash, deployment_key_for_script, deployment_reference_hashes,
    is_ckb_address, is_known_script_name, merge_script_info_for_reference, parse_address_to_script,
    parse_hash32, parse_optional_block_tx_cursor, parse_output_index,
    resolve_code_hash_for_hash_type, resolve_script_form_by_hash, script_to_address,
    shannon_to_ckb,
};
use crate::warmup::{
    CachedAddressEntry, CACHE_KEY_ADDRESSES_ACTIVE, CACHE_KEY_ADDRESSES_TOP, CACHE_KEY_SCRIPTS_ALL,
};
use crate::AppState;
use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
use ckbadger_store::{keys, CkbadgerStore};

const SHANNONS_PER_CKB: i64 = 100_000_000;
// All protocol detection (DAO / sUDT / xUDT / .bit-account / mNFT issuer·class·token /
// Spore NFT / Spore Cluster) is delegated to the shared network-agnostic
// `ckbadger_indexer::parser::registry::PROTOCOL_REGISTRY`, which covers mainnet + testnet.
// The string consts below survive only as TEST fixtures that construct cells whose
// type_code_hash matches a known protocol; they are compiled under `cfg(test)` so the
// library build carries no dead detection constants.
#[cfg(test)]
const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
#[cfg(test)]
const SUDT_CODE_HASH: &str = "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
#[cfg(test)]
const DOTBIT_ACCOUNT_CELL_TYPE_ID: &str =
    "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918";
#[cfg(test)]
const MNFT_ISSUER_CODE_HASH: &str =
    "0x24b04faf80ded836efc05247778eec4ec02548dab6e2012c0107374aa3f68b81";
#[cfg(test)]
const MNFT_CLASS_CODE_HASH: &str =
    "0xd51e6eaf48124c601f41abe173f1da550b4cbca9c6a166781906a287abbb3d9a";
#[cfg(test)]
const MNFT_TOKEN_CODE_HASH: &str =
    "0x2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9";
#[cfg(test)]
const SPORE_CODE_HASHES: [&str; 3] = [
    "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5",
    "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d",
    "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494",
];
#[cfg(test)]
const CLUSTER_CODE_HASHES: [&str; 3] = [
    "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075",
    "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058",
    "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080",
];
const ADDR_TX_SCAN_CHUNK_SIZE: usize = 128;

/// Convert a `semantic_tags` bitmap into human-readable script label strings.
/// Returns an empty vec when no bits are set (including legacy `0` values).
fn script_labels_from_semantic_tags(semantic_tags: u16) -> Vec<String> {
    use ckbadger_store::types::semantic_tags as st;
    let mut labels = Vec::new();
    if semantic_tags & st::DAO != 0 {
        labels.push("NervosDAO".to_string());
    }
    if semantic_tags & st::SUDT != 0 {
        labels.push("sUDT".to_string());
    }
    if semantic_tags & st::XUDT != 0 {
        labels.push("xUDT".to_string());
    }
    if semantic_tags & st::DOTBIT != 0 {
        labels.push(".bit".to_string());
    }
    if semantic_tags & st::MNFT != 0 {
        labels.push("mNFT".to_string());
    }
    if semantic_tags & st::SPORE != 0 {
        labels.push("Spore".to_string());
    }
    if semantic_tags & st::CLUSTER != 0 {
        labels.push("Spore Cluster".to_string());
    }
    if semantic_tags & st::DID_CKB != 0 {
        labels.push("did:ckb".to_string());
    }
    if semantic_tags & st::DOTCELL != 0 {
        labels.push(".cell".to_string());
    }
    labels
}

pub(crate) struct DepGroupParseResult {
    pub(crate) is_dep_group: bool,
    pub(crate) items: Option<Vec<DepGroupItem>>,
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
    pub token: Option<CellTokenDecode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deterministic: Option<CellDeterministicDecode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heuristic_guesses: Vec<CellDataGuess>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellTokenDecode {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub raw_amount: String,
    pub formatted_amount: Option<String>,
}

fn enrich_token_decode(
    analysis: &mut CellDataAnalysis,
    token: &ckbadger_store::TokenInfo,
) -> anyhow::Result<()> {
    let Some(decode) = analysis
        .deterministic
        .as_mut()
        .filter(|d| d.kind == "udt_amount")
    else {
        return Ok(());
    };
    let amount = decode
        .segments
        .iter_mut()
        .find(|segment| segment.label == "amount")
        .ok_or_else(|| anyhow::anyhow!("UDT decode is missing amount segment"))?;
    let raw = amount.human_value.clone();
    anyhow::ensure!(
        raw.chars().all(|c| c.is_ascii_digit()),
        "invalid raw UDT amount {raw}"
    );
    let decimals = token
        .decimals
        .map(u8::try_from)
        .transpose()
        .map_err(|_| anyhow::anyhow!("invalid token decimals {:?}", token.decimals))?;
    let formatted = decimals.map(|decimals| {
        let decimals = usize::from(decimals);
        if decimals == 0 {
            return raw.clone();
        }
        let padded = format!("{:0>width$}", raw, width = decimals + 1);
        let at = padded.len() - decimals;
        format!("{}.{}", &padded[..at], &padded[at..])
    });
    if let Some(formatted) = &formatted {
        amount.human_value = match &token.symbol {
            Some(symbol) => format!("{formatted} {symbol}"),
            None => formatted.clone(),
        };
    }
    analysis.token = Some(CellTokenDecode {
        name: token.name.clone(),
        symbol: token.symbol.clone(),
        decimals,
        raw_amount: raw,
        formatted_amount: formatted,
    });
    Ok(())
}

/// The registry protocol each of a cell's scripts belongs to, as the
/// registry's own slug (`dotcell-account`, `did-ckb`, …). Consumers identify
/// protocols by these names and never compare code hashes themselves. `null`
/// on a side is a definite answer: that script is not a registered protocol.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellProtocolScript {
    pub lock: Option<String>,
    #[serde(rename = "type")]
    pub type_script: Option<String>,
}

fn cell_protocol_script(info: &ckbadger_store::LiveCellInfo) -> CellProtocolScript {
    let slug_of = |code_hash: &[u8]| {
        PROTOCOL_REGISTRY
            .get(code_hash)
            .map(|protocol| protocol.slug().to_string())
    };
    CellProtocolScript {
        lock: slug_of(&info.lock_code_hash),
        type_script: info.type_code_hash.as_deref().and_then(slug_of),
    }
}

pub(crate) fn parse_dep_group(data: &[u8], data_size: i32) -> DepGroupParseResult {
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
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::SporeNft)
}

fn is_cluster_type_code_hash(code_hash: &[u8]) -> bool {
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::Cluster)
}

fn is_dotbit_account_type_code_hash(code_hash: &[u8]) -> bool {
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotbitAccount)
}

fn is_dao_type_code_hash(code_hash: &[u8]) -> bool {
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::Dao)
}

fn is_mnft_issuer_type_code_hash(code_hash: &[u8]) -> bool {
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::MnftIssuer)
}

fn is_mnft_class_type_code_hash(code_hash: &[u8]) -> bool {
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::MnftClass)
}

fn is_mnft_token_type_code_hash(code_hash: &[u8]) -> bool {
    PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::MnftToken)
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
    info: &ckbadger_store::PositionedCellInfo,
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
    segments.push(CellDataSegment {
        label: "content_type_length".to_string(),
        start: offset_content_type as i32,
        end: content_type_start as i32,
        meaning: "Molecule Bytes length word (u32 LE)".to_string(),
        human_value: content_type_bytes.len().to_string(),
    });
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
    segments.push(CellDataSegment {
        label: "content_length".to_string(),
        start: offset_content as i32,
        end: content_start as i32,
        meaning: "Molecule Bytes length word (u32 LE)".to_string(),
        human_value: content_bytes.len().to_string(),
    });
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
                label: "cluster_id_length".to_string(),
                start: offset_cluster_id as i32,
                end: cluster_start as i32,
                meaning: "Molecule optional Bytes length word (u32 LE)".to_string(),
                human_value: cluster_bytes.len().to_string(),
            });
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
    info: &ckbadger_store::PositionedCellInfo,
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
    info: &ckbadger_store::PositionedCellInfo,
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
    info: &ckbadger_store::PositionedCellInfo,
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
    ckbadger_indexer::parser::udt::UdtParser::is_udt_code_hash_bytes(code_hash, 0)
        .map(|standard| standard.as_str())
}

fn maybe_parse_udt_decode(
    info: &ckbadger_store::PositionedCellInfo,
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

/// Layer-0 decode of a `.bit` AccountCell:
/// `account_hash(32) ‖ account_id(20) ‖ next_account_id(20) ‖ expired_at(u64 LE) ‖ account`,
/// where the tail is the UTF-8 account name itself (`casinox.bit`).
///
/// `Err` when the tail is not UTF-8: the DAS contracts only ever write the
/// account name there, so a non-UTF-8 tail is corrupt data, not an opaque
/// payload to be shown as bytes.
fn maybe_parse_dotbit_decode(
    info: &ckbadger_store::PositionedCellInfo,
    data: &[u8],
) -> anyhow::Result<Option<CellDeterministicDecode>> {
    let Some(type_code_hash) = info.type_code_hash.as_ref() else {
        return Ok(None);
    };
    if !is_dotbit_account_type_code_hash(type_code_hash) {
        return Ok(None);
    }
    if data.len() < 52 {
        return Ok(None);
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
        let expired_at = u64::from_le_bytes(
            data[72..80]
                .try_into()
                .expect("data length already checked as at least 80 bytes"),
        );
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
        let account = std::str::from_utf8(&data[80..]).map_err(|e| {
            anyhow::anyhow!(
                "dotbit account name is not UTF-8: bytes 80..{} of the AccountCell data: {e}",
                data.len()
            )
        })?;
        segments.push(CellDataSegment {
            label: "account".to_string(),
            start: 80,
            end: data.len() as i32,
            meaning: "DAS account name (UTF-8, includes .bit suffix)".to_string(),
            human_value: account.to_string(),
        });
    }

    Ok(Some(CellDeterministicDecode {
        kind: "dotbit_account".to_string(),
        summary: "DAS account cell layout detected from type script".to_string(),
        segments,
    }))
}

/// Layer-0 decode of a `.cell` name cell or the ring root: the values come
/// from the indexer's own `DotCellParser::parse_name_data` and the segment
/// offsets from the parser's own field ranges — no second offset table here.
///
/// `Ok(None)`: not a Cells Account cell. `Err`: it is one and its data does
/// not decode. The indexer refuses to store such a cell (live and bulk both
/// bail), so meeting one here is a broken invariant, never a guess.
fn maybe_parse_dotcell_decode(
    info: &ckbadger_store::PositionedCellInfo,
    data: &[u8],
) -> anyhow::Result<Option<CellDeterministicDecode>> {
    use anyhow::Context as _;
    use ckbadger_indexer::parser::dotcell::{
        DotCellParser, DOTCELL_EXPIRY_RANGE, DOTCELL_HEADER_LEN, DOTCELL_LAYOUT_VERSION_RANGE,
        DOTCELL_MANAGER_RANGE, DOTCELL_NEXT_ID_RANGE, DOTCELL_OWNER_RANGE,
        DOTCELL_RECORDS_HASH_RANGE, DOTCELL_ZERO_ID,
    };

    let Some(type_code_hash) = info.type_code_hash.as_deref() else {
        return Ok(None);
    };
    if !DotCellParser::is_account_type_script(type_code_hash) {
        return Ok(None);
    }
    let name =
        DotCellParser::parse_name_data(data).context("dotcell name cell data does not decode")?;

    let hex = |bytes: &[u8]| format!("0x{}", hex::encode(bytes));
    let segment = |label: &str,
                   range: std::ops::Range<usize>,
                   meaning: &str,
                   human_value: String| CellDataSegment {
        label: label.to_string(),
        start: range.start as i32,
        end: range.end as i32,
        meaning: meaning.to_string(),
        human_value,
    };

    // A u40 always fits in i64 and in chrono's range; either failing means
    // the parser handed back something that is not a u40.
    let expired_at = i64::try_from(name.expired_at)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "dotcell expiry {} is not a representable u40 timestamp",
                name.expired_at
            )
        })?
        .to_rfc3339();
    let next_id = if name.next_id == DOTCELL_ZERO_ID {
        "end of ring".to_string()
    } else {
        hex(&name.next_id)
    };
    let owner = hex(&name.owner_hash20);
    let manager = if name.manager_hash20 == name.owner_hash20 {
        format!("{} (same as owner)", hex(&name.manager_hash20))
    } else {
        format!("{} (delegated)", hex(&name.manager_hash20))
    };
    let label = if name.is_root() {
        "(empty)".to_string()
    } else {
        let mut value = format!("{}.cell · id {}", name.label, hex(&name.id));
        if let Some(parent) = name.parent_label() {
            value.push_str(&format!(" · sub-name of {parent}.cell"));
        }
        value
    };

    let segments = vec![
        segment(
            "layout_version",
            DOTCELL_LAYOUT_VERSION_RANGE,
            "Layout version (u8)",
            name.layout_version.to_string(),
        ),
        segment(
            "records_hash",
            DOTCELL_RECORDS_HASH_RANGE,
            "blake2b of the records payload, carried in WitnessArgs.output_type of the \
             creating transaction's witness at this output index",
            hex(&name.records_hash),
        ),
        segment(
            "next_id",
            DOTCELL_NEXT_ID_RANGE,
            "Next name id in the ordered uniqueness ring (zero = end of ring)",
            next_id.clone(),
        ),
        segment(
            "expired_at",
            DOTCELL_EXPIRY_RANGE,
            "Expiry, unix seconds (u40 little-endian)",
            format!("{expired_at} (unix {})", name.expired_at),
        ),
        segment(
            "owner_hash20",
            DOTCELL_OWNER_RANGE,
            "Owner: first 20 bytes of the owner's lock script hash",
            owner.clone(),
        ),
        segment(
            "manager_hash20",
            DOTCELL_MANAGER_RANGE,
            "Manager: first 20 bytes of the manager's lock script hash",
            manager,
        ),
        segment(
            "label",
            DOTCELL_HEADER_LEN..data.len(),
            "Name label, UTF-8 (empty only on the ring root); id = blake2b(label)[..20]",
            label,
        ),
    ];

    if name.is_root() {
        let namespace = info.type_args.as_deref().ok_or_else(|| {
            anyhow::anyhow!("dotcell ring root has a type code_hash but no stored type args")
        })?;
        return Ok(Some(CellDeterministicDecode {
            kind: "dotcell_ring_root".to_string(),
            summary: format!(
                ".cell ring root of namespace {}; first name id: {next_id}",
                hex(namespace)
            ),
            segments,
        }));
    }
    Ok(Some(CellDeterministicDecode {
        kind: "dotcell_name".to_string(),
        summary: format!(
            "{}.cell name cell (layout v{}); owner {owner}; expires {expired_at}; records are in \
             the creating transaction's witness at this output index",
            name.label, name.layout_version
        ),
        segments,
    }))
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
    info: &ckbadger_store::PositionedCellInfo,
    data: &[u8],
) -> Option<CellDataGuess> {
    let Some(type_code_hash) = info.type_code_hash.as_ref() else {
        return build_lock_script_hint_guess(info, data);
    };

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
    } else if PROTOCOL_REGISTRY.is(type_code_hash, ProtocolScript::DotCellAccount) {
        // Unreachable in practice: an undecodable Account cell is an error in
        // `maybe_parse_dotcell_decode`, never a hint. Kept so the family is
        // described completely.
        (
            "Cells Account (.cell name)",
            "expected 98-byte header (version, records hash, next id, u40 expiry, owner, manager) + UTF-8 label",
            "high",
        )
    } else if PROTOCOL_REGISTRY.is(type_code_hash, ProtocolScript::DotCellPrice) {
        // A price cell is not decoded on this page; say what it is without
        // claiming its bytes are wrong.
        return Some(CellDataGuess {
            kind: "script_hint".to_string(),
            confidence: "medium".to_string(),
            reason: "Type script indicates Cells Price (.cell price cell); its payload is not decoded here"
                .to_string(),
            mime_type: None,
            human_value: Some(format!(
                "layout: version(u8) + factor_bps(u16 LE) + reserved(u16); observed length={} bytes",
                data.len()
            )),
        });
    } else {
        return build_lock_script_hint_guess(info, data);
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

/// A hint from the LOCK script, for `.cell` cells whose type script does not
/// explain them: a Sale Lock offer cell, or an Account Lock cell that does
/// not carry the Account type.
fn build_lock_script_hint_guess(
    info: &ckbadger_store::PositionedCellInfo,
    data: &[u8],
) -> Option<CellDataGuess> {
    let (reason, expectation) = match PROTOCOL_REGISTRY.get(&info.lock_code_hash) {
        Some(ProtocolScript::DotCellSaleLock) => (
            "Lock script indicates Cells Sale Lock (.cell offer cell); its payload is not decoded here",
            "lock args = seller_lock_hash(32) + price_shannons(u64 LE); data = molecule Script of the seller's payout lock",
        ),
        Some(ProtocolScript::DotCellAccountLock) => (
            "Lock script indicates Cells Account Lock but the type script is not Cells Account",
            "a Cells Account Lock cell is expected to carry the Cells Account type and a .cell name payload",
        ),
        _ => return None,
    };
    Some(CellDataGuess {
        kind: "script_hint".to_string(),
        confidence: "medium".to_string(),
        reason: reason.to_string(),
        mime_type: None,
        human_value: Some(format!(
            "{expectation}; observed length={} bytes",
            data.len()
        )),
    })
}

/// Layer-0 analysis of a cell's data. Fallible: a cell whose script says it
/// is a protocol cell the indexer decodes (and would have refused to store
/// had it not decoded) but whose bytes do not decode here is a broken
/// invariant, reported with context rather than shown as a guess.
fn analyze_cell_data(
    info: &ckbadger_store::PositionedCellInfo,
    data: &[u8],
    data_size: i32,
) -> anyhow::Result<CellDataAnalysis> {
    // The fallible decoders come first: each either decodes its protocol's
    // cell, says it is not one, or reports the broken invariant.
    // Every decoder gates on its own registry protocol, so their order only
    // decides which kind of failure surfaces first, never which decode wins.
    let deterministic = if let Some(decode) = maybe_parse_dotcell_decode(info, data)? {
        Some(decode)
    } else if let Some(decode) = maybe_parse_dotbit_decode(info, data)? {
        Some(decode)
    } else {
        maybe_parse_dao_decode(info, data)
            .or_else(|| maybe_parse_spore_decode(info, data))
            .or_else(|| maybe_parse_cluster_decode(info, data))
            .or_else(|| maybe_parse_mnft_decode(info, data))
            .or_else(|| maybe_parse_udt_decode(info, data))
            .or_else(|| {
                if info.type_code_hash.is_none() {
                    maybe_parse_dep_group_decode(data, data_size)
                } else {
                    None
                }
            })
    };

    if info
        .type_code_hash
        .as_deref()
        .is_some_and(is_spore_type_code_hash)
        && !deterministic
            .as_ref()
            .is_some_and(|decoded| decoded.kind == "spore_cell")
    {
        anyhow::bail!("recognized Spore cell has invalid Molecule payload");
    }

    let mut heuristic_guesses = if deterministic.is_none() {
        build_heuristic_guesses(data)
    } else {
        Vec::new()
    };
    if deterministic.is_none() {
        if let Some(script_hint) = build_script_hint_guess(info, data) {
            heuristic_guesses.insert(0, script_hint);
        }
    }

    Ok(CellDataAnalysis {
        token: None,
        deterministic,
        heuristic_guesses,
    })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/cells/live-summary", get(get_live_cell_summary))
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
        .route(
            "/addresses/prefix/{prefix}/transactions",
            get(get_prefix_transactions),
        )
        .route(
            "/addresses/{addr}/dotcell-names",
            get(get_address_dotcell_names),
        )
        .route("/addresses/{addr}/tokens", get(get_address_tokens))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellSummaryTipResponse {
    pub block: i64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellSummaryClassesResponse {
    pub dao: u64,
    pub typed_non_dao: u64,
    pub plain: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellSummaryResponse {
    pub tip: LiveCellSummaryTipResponse,
    pub live_cells: u64,
    pub classes: LiveCellSummaryClassesResponse,
    pub data_bearing: u64,
}

async fn get_live_cell_summary(
    State(state): State<Arc<AppState>>,
) -> ApiResult<LiveCellSummaryResponse> {
    let summary = state.store.get_live_cell_summary().map_err(|error| {
        ApiError::internal(format!("failed to read live-cell summary: {error}"))
    })?;
    let summary = match summary {
        Some(summary) => summary,
        None => {
            let initializing = || {
                ApiError::initializing(
                    "live-cell summary is unavailable while the indexer initializes or reconciles a reorg",
                )
            };
            let initialized = state
                .store
                .is_live_cell_summary_initialized()
                .map_err(|error| {
                    ApiError::internal(format!(
                        "failed to read live-cell summary initialization state: {error}"
                    ))
                })?;
            if !initialized {
                return Err(initializing());
            }
            let rollback_in_progress =
                state
                    .store
                    .is_rollback_cleanup_in_progress()
                    .map_err(|error| {
                        ApiError::internal(format!(
                            "failed to read live-cell summary rollback state: {error}"
                        ))
                    })?;
            if rollback_in_progress {
                return Err(initializing());
            }
            let status = state.store.get_sync_status().map_err(|error| {
                ApiError::internal(format!(
                    "failed to read sync status for live-cell summary: {error}"
                ))
            })?;
            if status.deep_fork_detected || status.tip_block_hash.is_empty() {
                return Err(initializing());
            }
            return Err(ApiError::internal(format!(
                "initialized live-cell summary is missing outside reconciliation: tip_block={} tip_hash=0x{}",
                status.tip_block_number,
                hex::encode(status.tip_block_hash),
            )));
        }
    };
    let live_cells = summary.live_cells().map_err(|error| {
        ApiError::internal(format!("invalid persisted live-cell summary: {error}"))
    })?;

    ok(LiveCellSummaryResponse {
        tip: LiveCellSummaryTipResponse {
            block: summary.tip_block_number,
            hash: format!("0x{}", hex::encode(summary.tip_block_hash)),
        },
        live_cells,
        classes: LiveCellSummaryClassesResponse {
            dao: summary.dao,
            typed_non_dao: summary.typed_non_dao,
            plain: summary.plain,
        },
        data_bearing: summary.data_bearing,
    })
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
    cursor: Option<String>,
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
    #[serde(rename = "commonKnowledgeSize")]
    pub occupied_capacity: String,
    pub common_knowledge_size_breakdown: OccupiedCapacityBreakdown,
    pub lock_script_hash: String,
    pub address: String,
    pub type_script_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_code_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_hash_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_protocol_name: Option<String>,
    pub data_size: i32,
    pub created_at_block: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    #[serde(
        rename = "virtualCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_used_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udt_amount: Option<String>,
    /// `cells/by-script` only: which index enumerated this row (`lock` or
    /// `type`). In `script_kind=both` this is the phase of the composite
    /// cursor a client would build from this row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_script_kind: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DepGroupItem {
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
pub struct CodeCellReference {
    pub reference_hash: String,
    pub hash_type: String,
    pub lock_live_cells_count: i64,
    pub type_live_cells_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellEvidence {
    /// Identity of these exact bytes, independent of the publishing outpoint.
    pub version_hash: String,
    pub current_references: Vec<CodeCellReference>,
    pub upgrade_cohort: Vec<CodeCellUpgrade>,
    pub type_id_lineage: Vec<CodeCellTypeIdLineage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellTypeIdLineage {
    pub type_script_hash: String,
    pub observed_at_block: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellUpgrade {
    pub previous_tx_hash: String,
    pub previous_output_index: u32,
    pub current_tx_hash: String,
    pub current_output_index: u32,
    pub type_script_hash: String,
    pub previous_version_hash: Option<String>,
    pub current_version_hash: String,
    pub binary_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellSpendRequirement {
    pub kind: String,
    pub reference_hash: String,
    pub hash_type: String,
    /// Current chain candidate; never evidence of this cell's historical execution.
    pub resolution: String,
    pub version_hashes: Vec<String>,
    pub candidate_cells: Vec<CellSpendCandidate>,
    /// Consumed instances of the same version, separate from usable deps.
    pub historical_cells: Vec<CellSpendCandidate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellSpendCandidate {
    pub tx_hash: String,
    pub output_index: i16,
    pub version_hash: String,
    pub live: bool,
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
    #[serde(rename = "commonKnowledgeSize")]
    pub used_capacity: String,
    #[serde(rename = "commonKnowledgeSizeBreakdown")]
    pub used_capacity_breakdown: OccupiedCapacityBreakdown,
    #[serde(
        rename = "virtualCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_used_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub type_script_hash: Option<String>,
    pub data_size: i32,
    pub created_at_block: i64,
    pub created_timestamp: String,
    pub status: String,
    pub consumed_at_block: Option<i64>,
    pub consumed_by_tx: Option<String>,
    pub spent_timestamp: Option<String>,
    pub consuming_input_index: Option<u32>,
    pub lifetime_ms: Option<i64>,
    /// Age at the indexed chain tip, using monotonic chain time.
    pub live_age_ms: Option<i64>,
    pub live_age_at_block: Option<i64>,
    pub lock: ScriptResponse,
    #[serde(rename = "type")]
    pub type_script: Option<ScriptResponse>,
    /// Registry protocol slugs of `lock` / `type`; always present.
    pub protocol_script: CellProtocolScript,
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_analysis: Option<CellDataAnalysis>,
    pub is_dep_group: bool,
    pub dep_group_items: Option<Vec<DepGroupItem>>,
    pub code_cell_of: Option<Vec<CodeCellScript>>,
    /// Canonical serialized CellOutput, with a nonoverlapping field map.
    pub cell_output_molecule: cell_anatomy::MoleculeCellOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elf: Option<cell_elf::ElfAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_cell: Option<CodeCellEvidence>,
    pub current_spend_requirements: Vec<CellSpendRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dao_info: Option<DaoInfo>,
}

#[derive(Debug, Clone, Serialize)]
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
    #[serde(rename = "commonKnowledgeSize")]
    pub used_capacity: String,
    pub live_cells_count: i64,
    pub transactions_count: i64,
    pub recent_activities_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_script: Option<ScriptResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_script_info: Option<LockScriptInfo>,
    /// What the address has pending in the node's tx pool, whatever a list
    /// filters; `null` when the mirror is disabled or unhealthy. Always
    /// computed per request — never served from the response cache.
    pub pending_summary: Option<crate::routes::address_pool::PendingSummaryResponse>,
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
    /// Null for a transaction still in the node's tx pool: it has no block yet.
    pub block_number: Option<i64>,
    pub tx_type: String,
    pub capacity_change: String,
    /// Block time. Null for a tx-pool row, which reports `timeAddedToPool`
    /// instead — when the node accepted it, not when a block confirmed it.
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_added_to_pool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<crate::pool::InterpretationResponse>,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub fee: String,
    pub is_cellbase: bool,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub cycles_status: String,
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
    cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressTokenResponse {
    pub type_script_hash: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    /// None when unknown (no TOML label and no on-chain info cell) — never a
    /// fabricated 0, which would be indistinguishable from true 0 decimals.
    pub decimals: Option<i16>,
    pub icon_url: Option<String>,
    pub balance: String,
}

fn parse_address_token_cursor(
    cursor: &str,
) -> Result<(TokenBalance, Vec<u8>), (axum::http::StatusCode, axum::Json<ApiError>)> {
    let (balance, type_hash_hex) = cursor
        .split_once(':')
        .ok_or_else(|| ApiError::bad_request("Invalid address token cursor"))?;
    let balance = balance
        .parse::<TokenBalance>()
        .map_err(|_| ApiError::bad_request("Invalid address token cursor"))?;
    let type_hash = hex::decode(type_hash_hex.strip_prefix("0x").unwrap_or(type_hash_hex))
        .map_err(|_| ApiError::bad_request("Invalid address token cursor"))?;
    if type_hash.len() != 32 {
        return Err(ApiError::bad_request("Invalid address token cursor"));
    }
    Ok((balance, type_hash))
}

/// Helper to convert a LiveCellInfo into a CellResponse.
///
/// `network` selects the per-network genesis burn policy and `virtual_occupied`
/// is the derived `baseline.virtual_occupied` (shannons) reported for genesis
/// burn cells; both are read once per request in the handler and threaded in.
fn cell_info_to_response(
    tx_hash: &[u8],
    output_index: i16,
    info: &ckbadger_store::PositionedCellInfo,
    network: &str,
    virtual_occupied: i128,
) -> Result<CellResponse, ApiRouteError> {
    let is_special_burn = info.created_at_block == 0
        && ckbadger_common::burn_policy::burn_policy(network)
            .is_some_and(|p| info.lock_args.as_slice() == p.lock_args);
    let breakdown = estimated_occupied_capacity_breakdown(info);
    let expected_occupied = breakdown
        .total_bytes
        .checked_mul(SHANNONS_PER_CKB)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "cell {}:{} occupied capacity overflow",
                hex::encode(tx_hash),
                output_index
            ))
        })?;
    if info.occupied_capacity != expected_occupied {
        return Err(ApiError::internal(format!(
            "cell {}:{} indexed occupied capacity {} differs from exact anatomy {}",
            hex::encode(tx_hash),
            output_index,
            info.occupied_capacity,
            expected_occupied
        )));
    }
    let address = script_to_address(
        &info.lock_code_hash,
        info.lock_hash_type,
        &info.lock_args,
        network,
    )
    .map_err(|error| {
        ApiError::internal(format!(
            "cell {}:{} owner address: {error}",
            hex::encode(tx_hash),
            output_index
        ))
    })?;
    let type_hash_type = match (&info.type_code_hash, info.type_hash_type) {
        (None, None) => None,
        (Some(_), Some(kind)) => Some(
            hash_type_to_str(kind)
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "cell {}:{} invalid type hash_type {kind}",
                        hex::encode(tx_hash),
                        output_index
                    ))
                })?
                .to_string(),
        ),
        _ => {
            return Err(ApiError::internal(format!(
                "cell {}:{} incomplete type script",
                hex::encode(tx_hash),
                output_index
            )))
        }
    };
    Ok(CellResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
        output_index: output_index as i32,
        capacity: info.capacity.to_string(),
        occupied_capacity: info.occupied_capacity.to_string(),
        common_knowledge_size_breakdown: breakdown,
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        address,
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_code_hash: info
            .type_code_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_hash_type,
        type_protocol_name: info
            .type_code_hash
            .as_ref()
            .and_then(|hash| PROTOCOL_REGISTRY.get(hash))
            .map(|protocol| protocol.slug().to_string()),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        cell_type: if is_special_burn {
            Some("genesis_special_burn".to_string())
        } else {
            None
        },
        virtual_used_capacity: if is_special_burn {
            Some(virtual_occupied.to_string())
        } else {
            None
        },
        udt_amount: info.udt_amount.map(|amount| amount.to_string()),
        matched_script_kind: None,
    })
}

fn estimated_occupied_capacity_breakdown(
    info: &ckbadger_store::PositionedCellInfo,
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

fn current_spend_requirements(
    state: &AppState,
    info: &ckbadger_store::PositionedCellInfo,
    outpoint: &str,
) -> Result<Vec<CellSpendRequirement>, ApiRouteError> {
    let mut forms = vec![("lock", info.lock_code_hash.as_slice(), info.lock_hash_type)];
    if let Some(type_hash) = info.type_code_hash.as_ref() {
        let hash_type = info.type_hash_type.ok_or_else(|| {
            ApiError::internal(format!(
                "cell {outpoint} type reference is missing hash_type"
            ))
        })?;
        forms.push(("type", type_hash.as_slice(), hash_type));
    }
    forms
        .into_iter()
        .map(|(kind, hash, hash_type)| {
            let hash_type_name = hash_type_to_str(hash_type).ok_or_else(|| {
                ApiError::internal(format!(
                    "cell {outpoint} {kind} reference has invalid hash_type {hash_type}"
                ))
            })?;
            let built_in_type_id = kind == "type"
                && hash_type == 1
                && hex::encode(hash) == ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH;
            let (resolution, hashes) = if built_in_type_id {
                ("builtin", Vec::new())
            } else {
                let resolved = resolve_script_form_by_hash(
                    &state.store,
                    &state.append_only_store,
                    hash_type as u8,
                    hash,
                )
                .map_err(|error| {
                    ApiError::internal(format!(
                        "cell {outpoint} current {kind} reference resolution: {error:#}"
                    ))
                })?;
                match resolved {
                    CurrentScriptVersionResolution::NotFound => ("not_found", Vec::new()),
                    CurrentScriptVersionResolution::Resolved(version) => {
                        ("resolved", vec![version.version_hash])
                    }
                    CurrentScriptVersionResolution::Ambiguous(versions) => {
                        ("ambiguous", versions.version_hashes)
                    }
                }
            };
            let mut candidate_cells = Vec::new();
            let mut historical_cells = Vec::new();
            for version in &hashes {
                let entries =
                    list_version_code_cells(&state.store, &state.append_only_store, version)
                        .map_err(|error| {
                            ApiError::internal(format!(
                                "cell {outpoint} current {kind} candidate cells: {error:#}"
                            ))
                        })?;
                let (live, spent) = eligible_reference_cells(hash_type, hash, version, entries);
                candidate_cells.extend(live);
                historical_cells.extend(spent);
            }
            let resolution = available_reference_resolution(resolution, &hashes, &candidate_cells);
            Ok(CellSpendRequirement {
                kind: kind.into(),
                reference_hash: format!("0x{}", hex::encode(hash)),
                hash_type: hash_type_name.into(),
                resolution: resolution.into(),
                version_hashes: hashes
                    .into_iter()
                    .map(|hash| format!("0x{}", hex::encode(hash)))
                    .collect(),
                candidate_cells,
                historical_cells,
            })
        })
        .collect()
}

fn eligible_reference_cells(
    hash_type: i16,
    reference_hash: &[u8],
    version_hash: &[u8],
    entries: Vec<crate::utils::script_resolution::VersionCodeCell>,
) -> (Vec<CellSpendCandidate>, Vec<CellSpendCandidate>) {
    let mut live = Vec::new();
    let mut historical = Vec::new();
    for (tx_hash, output_index, cell, is_live) in entries {
        if hash_type == 1 && cell.type_script_hash.as_deref() != Some(reference_hash) {
            continue;
        }
        let entry = CellSpendCandidate {
            tx_hash: format!("0x{}", hex::encode(tx_hash)),
            output_index,
            version_hash: format!("0x{}", hex::encode(version_hash)),
            live: is_live,
        };
        if is_live {
            live.push(entry);
        } else {
            historical.push(entry);
        }
    }
    (live, historical)
}

fn available_reference_resolution<'a>(
    resolution: &'a str,
    hashes: &[Vec<u8>],
    candidates: &[CellSpendCandidate],
) -> &'a str {
    if resolution != "builtin" && !hashes.is_empty() && candidates.is_empty() {
        "unavailable"
    } else {
        resolution
    }
}

/// Same-transaction Type ID ELF successors. Type ID's unique input/output
/// continuation proves a predecessor; a generic type script can appear on
/// multiple inputs and does not prove a one-to-one upgrade.
fn code_cell_upgrade_cohort(
    state: &AppState,
    tx: &ckb_types::core::TransactionView,
    tx_hash: &[u8],
    outpoint: &str,
) -> Result<Vec<CodeCellUpgrade>, ApiRouteError> {
    use ckb_types::prelude::Unpack;
    if tx.is_cellbase() {
        return Ok(Vec::new());
    }
    let mut inputs = Vec::new();
    for input in tx.inputs() {
        let previous = input.previous_output();
        let previous_hash = previous.tx_hash().as_slice().to_vec();
        let index: u32 = previous.index().unpack();
        if previous_hash == [0u8; 32] && index == u32::MAX {
            return Err(ApiError::internal(format!(
                "cell {outpoint} non-cellbase transaction contains null outpoint"
            )));
        }
        let index_i16 = i16::try_from(index).map_err(|_| {
            ApiError::internal(format!(
                "cell {outpoint} upgrade cohort input output index {index} exceeds store range"
            ))
        })?;
        let consumed = state
            .store
            .get_consumed_cell_info(&previous_hash, index_i16, &state.append_only_store)
            .map_err(|error| {
                ApiError::internal(format!("cell {outpoint} upgrade cohort input: {error}"))
            })?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "cell {outpoint} upgrade cohort input 0x{}:{index} missing consumed marker",
                    hex::encode(&previous_hash)
                ))
            })?;
        if consumed.consumed_by_tx.as_deref() != Some(tx_hash) {
            return Err(ApiError::internal(format!(
                "cell {outpoint} upgrade cohort input 0x{}:{index} has wrong consumer",
                hex::encode(&previous_hash)
            )));
        }
        if let Some(type_hash) = consumed.cell.type_script_hash {
            inputs.push((previous_hash, index, type_hash, consumed.cell.data_hash));
        }
    }
    let mut upgrades = Vec::new();
    for (index, (output, data)) in tx.outputs().into_iter().zip(tx.outputs_data()).enumerate() {
        let Some(type_script) = output.type_().to_opt() else {
            continue;
        };
        if type_script.code_hash().as_slice()
            != hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH)
                .expect("constant hex")
            || type_script.hash_type().as_slice() != [1]
        {
            continue;
        }
        let Some(_) = cell_elf::parse(&data.raw_data()).map_err(|error| {
            ApiError::internal(format!(
                "cell {outpoint} upgrade cohort output {index} ELF parse: {error:#}"
            ))
        })?
        else {
            continue;
        };
        let type_hash = type_script.calc_script_hash();
        let matches: Vec<_> = inputs
            .iter()
            .filter(|(_, _, hash, _)| hash.as_slice() == type_hash.as_slice())
            .collect();
        if matches.is_empty() {
            continue;
        }
        if matches.len() != 1 {
            return Err(ApiError::internal(format!(
                "cell {outpoint} Type ID upgrade cohort output {index} has {} matching inputs",
                matches.len()
            )));
        }
        let (previous_hash, previous_index, _, previous_version) = matches[0];
        let mut hasher = ckb_hash::new_blake2b();
        hasher.update(&data.raw_data());
        let mut version = [0u8; 32];
        hasher.finalize(&mut version);
        upgrades.push(CodeCellUpgrade {
            previous_tx_hash: format!("0x{}", hex::encode(previous_hash)),
            previous_output_index: *previous_index,
            current_tx_hash: format!("0x{}", hex::encode(tx_hash)),
            current_output_index: u32::try_from(index).map_err(|_| {
                ApiError::internal(format!("cell {outpoint} upgrade output index overflow"))
            })?,
            type_script_hash: format!("0x{}", hex::encode(type_hash.as_slice())),
            previous_version_hash: previous_version
                .as_ref()
                .map(|hash| format!("0x{}", hex::encode(hash))),
            current_version_hash: format!("0x{}", hex::encode(version)),
            binary_changed: previous_version
                .as_ref()
                .map(|hash| hash.as_slice() != version),
        });
    }
    Ok(upgrades)
}

/// A cell index key: script_hash(32B) + block_num(8B BE) + outpoint(34B).
/// Mirrors what `keys::encode_cell_index_key` builds.
const CELL_INDEX_KEY_LEN: usize = 32 + 8 + keys::OUTPOINT_KEY_SIZE;

/// Decode a cell cursor (hex-encoded full cell index key) for a scan over
/// `expected_prefix`.
///
/// Validated rather than best-effort, on the model of `parse_by_script_cursor`.
/// The unchecked version returned `Option` and was consumed with `.and_then`,
/// which produced two silent wrong answers: non-hex became "no cursor" so the
/// client got page 1 again and paginated forever, and a well-formed but
/// wrong-length or foreign-prefix cursor was handed to RocksDB verbatim as a
/// seek key, landing outside the scanned range so the `starts_with` break
/// returned an empty page that reads as "end of results".
fn parse_cell_cursor(cursor: &str, expected_prefix: &[u8]) -> Result<Vec<u8>, ApiRouteError> {
    let key = hex::decode(cursor.strip_prefix("0x").unwrap_or(cursor))
        .map_err(|_| ApiError::bad_request("Invalid cursor: not a hex-encoded cell index key"))?;
    if key.len() != CELL_INDEX_KEY_LEN {
        return Err(ApiError::bad_request(format!(
            "Invalid cursor: expected a {CELL_INDEX_KEY_LEN}-byte cell index key, got {} bytes",
            key.len()
        )));
    }
    if !key.starts_with(expected_prefix) {
        return Err(ApiError::bad_request(
            "Invalid cursor: key belongs to a different script than the one being listed",
        ));
    }
    Ok(key)
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

/// Which prefix-scannable index `list_live_cells` enumerates.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LiveCellIndex {
    Lock,
    Type,
}

/// One `list_live_cells` page request against a prefix-scannable index.
struct LiveCellScan<'a> {
    index: LiveCellIndex,
    index_hash: &'a [u8],
    needed: usize,
    after_key: Option<Vec<u8>>,
    /// Whether `keep` can reject rows. Drives read sizing only.
    post_filtered: bool,
}

/// Enumerate one live-cell index, keeping only rows that satisfy `keep`.
///
/// This is the single scan path for `list_live_cells`. It previously existed as
/// three near-copies of the same continuation loop selected by a
/// `match (&lock_hash, &type_hash)`, and only one of them consulted
/// `type_code_hash` — so a validated filter was silently dropped on the other
/// branches and the caller got rows that did not match what it asked for.
/// With the predicate passed in, a new filter cannot be honoured on one branch
/// and forgotten on another.
///
/// The loop exists because post-filtering can discard most of a batch; it reads
/// further batches until the page is full or the index is exhausted, rather
/// than over-reading by a fixed multiplier and silently truncating the page.
fn scan_live_cells(
    store: &ckbadger_store::CkbadgerStore,
    append_only_store: &ckbadger_store::CkbadgerStore,
    scan: LiveCellScan<'_>,
    keep: impl Fn(&ckbadger_store::PositionedCellInfo) -> bool,
) -> anyhow::Result<Vec<(Vec<u8>, i16, ckbadger_store::PositionedCellInfo)>> {
    let LiveCellScan {
        index,
        index_hash,
        needed,
        after_key,
        post_filtered,
    } = scan;

    // With no post-filter every row counts, so one exact-size read suffices.
    let batch_size = if post_filtered {
        needed * 2 + 1
    } else {
        needed
    };
    const MAX_ITERATIONS: usize = 50;

    let mut results = Vec::with_capacity(needed);
    let mut current_after_key = after_key;

    for _ in 0..MAX_ITERATIONS {
        let batch = match index {
            LiveCellIndex::Lock => store.list_cells_by_lock(
                index_hash,
                batch_size,
                current_after_key.as_deref(),
                append_only_store,
            )?,
            LiveCellIndex::Type => store.list_cells_by_type(
                index_hash,
                batch_size,
                current_after_key.as_deref(),
                append_only_store,
            )?,
        };
        let scan_exhausted = batch.len() < batch_size;

        for (tx_hash, output_index, info) in batch {
            current_after_key = Some(keys::encode_cell_index_key(
                index_hash,
                info.created_at_block,
                &tx_hash,
                output_index,
            ));
            if keep(&info) {
                results.push((tx_hash, output_index, info));
                if results.len() >= needed {
                    break;
                }
            }
        }

        if results.len() >= needed || scan_exhausted {
            break;
        }
    }

    Ok(results)
}

async fn list_live_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let lock_hash_bytes = if let Some(ref lock_hash) = params.lock_script_hash {
        Some(if is_ckb_address(lock_hash) {
            address_to_lock_script_hash(lock_hash)
                .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
        } else {
            parse_hash32(lock_hash, "lock_script_hash")?
        })
    } else {
        None
    };

    let type_hash_bytes = if let Some(ref type_hash) = params.type_script_hash {
        Some(parse_hash32(type_hash, "type_script_hash")?)
    } else {
        None
    };

    let type_code_hash_bytes = if let Some(ref code_hash) = params.type_code_hash {
        Some(parse_hash32(code_hash, "type_code_hash")?)
    } else {
        None
    };

    // Only the lock and type *script hash* indexes are prefix-scannable here, so
    // one of them has to anchor the scan. Everything else narrows it afterwards.
    let (index, index_hash) = match (&lock_hash_bytes, &type_hash_bytes) {
        (Some(lock_bytes), _) => (LiveCellIndex::Lock, lock_bytes.as_slice()),
        (None, Some(type_bytes)) => (LiveCellIndex::Type, type_bytes.as_slice()),
        (None, None) => {
            // Answering this would mean a full scan of the live-cell CF. Saying
            // so is required: the previous code returned an empty page, which a
            // client cannot distinguish from "no such cells exist".
            let detail = if type_code_hash_bytes.is_some() {
                "type_code_hash alone is not a scannable index; add lock_script_hash or \
                 type_script_hash, or use /cells/by-script?code_hash=..&hash_type=.. which \
                 reads the cell-by-code index directly"
            } else {
                "provide lock_script_hash or type_script_hash"
            };
            return Err(ApiError::bad_request(format!(
                "Listing live cells requires a filter that selects an index: {detail}"
            )));
        }
    };

    // Filters the scanned index does not already enforce.
    let post_type_hash = match index {
        LiveCellIndex::Lock => type_hash_bytes.as_deref(),
        LiveCellIndex::Type => None,
    };
    let post_type_code_hash = type_code_hash_bytes.as_deref();
    let post_filtered = post_type_hash.is_some() || post_type_code_hash.is_some();

    let after_key = match params.cursor.as_deref() {
        None | Some("") => None,
        Some(cursor) => Some(parse_cell_cursor(cursor, index_hash)?),
    };

    let raw_cells = scan_live_cells(
        &state.store,
        &state.append_only_store,
        LiveCellScan {
            index,
            index_hash,
            needed: limit + 1,
            after_key,
            post_filtered,
        },
        |info| {
            post_type_hash.is_none_or(|want| info.type_script_hash.as_deref() == Some(want))
                && post_type_code_hash
                    .is_none_or(|want| info.type_code_hash.as_deref() == Some(want))
        },
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = raw_cells.len() > limit;
    let raw_cells: Vec<_> = raw_cells.into_iter().take(limit).collect();

    // The cursor resumes the index that was actually scanned, so it is prefixed
    // with the same hash `parse_cell_cursor` will check on the next request.
    let next_cursor = if has_more {
        raw_cells.last().map(|(tx_hash, output_index, info)| {
            encode_cell_cursor(index_hash, info.created_at_block, tx_hash, *output_index)
        })
    } else {
        None
    };

    let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
    let cells: Vec<CellResponse> = raw_cells
        .iter()
        .map(|(tx_hash, output_index, info)| {
            cell_info_to_response(
                tx_hash,
                *output_index,
                info,
                &state.ckb_network,
                virtual_occupied,
            )
        })
        .collect::<Result<_, _>>()?;

    // Return pre-computed total when filtering by lock_script_hash only.
    let total = match (&lock_hash_bytes, &type_hash_bytes, &type_code_hash_bytes) {
        (Some(lock_bytes), None, None) => state
            .store
            .get_addr_balance(lock_bytes)
            .map_err(|error| {
                ApiError::internal(format!(
                    "address balance 0x{} read: {error}",
                    hex::encode(lock_bytes)
                ))
            })?
            .map(|ab| ab.live_cells_count as i64),
        _ => None,
    };

    match total {
        Some(t) => ok(CursorPaginatedResponse::new(
            cells,
            t,
            limit as i64,
            next_cursor,
        )),
        None => ok(CursorPaginatedResponse::without_total(
            cells,
            limit as i64,
            next_cursor,
        )),
    }
}

fn parse_hash_type(hash_type: &str) -> Option<u8> {
    ckbadger_common::hash_type_from_label(hash_type)
}

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<ckbadger_store::ScriptInfo>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .map(|rows| rows.into_iter().map(|(_, info)| info).collect())
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

/// Which cell-by-code index a by-script row was enumerated from. In
/// `script_kind=both` the lock index is exhausted before the type index
/// starts, so the phase plus the row's raw index key resumes pagination
/// exactly.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ByScriptPhase {
    Lock,
    Type,
}

impl ByScriptPhase {
    fn as_str(self) -> &'static str {
        match self {
            ByScriptPhase::Lock => "lock",
            ByScriptPhase::Type => "type",
        }
    }
}

/// One by-script result row: enumeration phase + outpoint + cell payload.
type ByScriptRow = (
    ByScriptPhase,
    Vec<u8>,
    i16,
    ckbadger_store::PositionedCellInfo,
);

/// Parse a `cells/by-script` cursor.
///
/// For `script_kind=lock|type` the cursor is the hex-encoded 75-byte
/// cell-by-code index key of the last returned row. For `script_kind=both` it
/// is phase-composite: `lock:<hex>` or `type:<hex>`. The key must belong to
/// the requested `(code_hash, hash_type)` form — anything else is a client
/// error, not a silent empty page.
fn parse_by_script_cursor(
    cursor: &str,
    script_kind: &str,
    expected_prefix: &[u8],
) -> Result<(ByScriptPhase, Vec<u8>), ApiRouteError> {
    let (phase, key_hex) = match script_kind {
        "lock" => (ByScriptPhase::Lock, cursor),
        "type" => (ByScriptPhase::Type, cursor),
        _ => match cursor.split_once(':') {
            Some(("lock", rest)) => (ByScriptPhase::Lock, rest),
            Some(("type", rest)) => (ByScriptPhase::Type, rest),
            _ => return Err(ApiError::bad_request(
                "Invalid cursor for script_kind=both: expected \"lock:<hex>\" or \"type:<hex>\"",
            )),
        },
    };
    let key = hex::decode(key_hex.strip_prefix("0x").unwrap_or(key_hex))
        .map_err(|_| ApiError::bad_request("Invalid cursor: not a hex-encoded cell index key"))?;
    if key.len() != keys::CELL_CODE_INDEX_KEY_SIZE || !key.starts_with(expected_prefix) {
        return Err(ApiError::bad_request(
            "Invalid cursor: key does not match the requested code_hash/hash_type form",
        ));
    }
    Ok((phase, key))
}

/// Collect up to `fetch_limit` live cells of the reference form
/// `(code_hash, hash_type)` for one `script_kind`, resuming from `cursor`.
///
/// Each form is a contiguous key range in the cell-by-code indexes, so this
/// reads rows directly — no cross-form scanning or post-filtering. In `both`
/// mode the lock index is enumerated to exhaustion first, then the type index;
/// type-phase rows whose lock script also uses this exact form are skipped
/// because the lock phase already emitted them.
fn collect_by_script_rows(
    store: &ckbadger_store::CkbadgerStore,
    append_only_store: &ckbadger_store::CkbadgerStore,
    script_kind: &str,
    code_hash: &[u8],
    hash_type: u8,
    fetch_limit: usize,
    cursor: Option<(ByScriptPhase, Vec<u8>)>,
) -> anyhow::Result<Vec<ByScriptRow>> {
    let mut rows: Vec<ByScriptRow> = Vec::new();
    match script_kind {
        "lock" => {
            let after = cursor.as_ref().map(|(_, key)| key.as_slice());
            for (tx_hash, output_index, info) in store.list_cells_by_lock_code_hash(
                code_hash,
                hash_type,
                fetch_limit,
                after,
                append_only_store,
            )? {
                rows.push((ByScriptPhase::Lock, tx_hash, output_index, info));
            }
        }
        "type" => {
            let after = cursor.as_ref().map(|(_, key)| key.as_slice());
            for (tx_hash, output_index, info) in store.list_cells_by_type_code_hash(
                code_hash,
                hash_type,
                fetch_limit,
                after,
                append_only_store,
            )? {
                rows.push((ByScriptPhase::Type, tx_hash, output_index, info));
            }
        }
        "both" => {
            let (start_phase, after_key) = match cursor {
                Some((phase, key)) => (phase, Some(key)),
                None => (ByScriptPhase::Lock, None),
            };
            if start_phase == ByScriptPhase::Lock {
                for (tx_hash, output_index, info) in store.list_cells_by_lock_code_hash(
                    code_hash,
                    hash_type,
                    fetch_limit,
                    after_key.as_deref(),
                    append_only_store,
                )? {
                    rows.push((ByScriptPhase::Lock, tx_hash, output_index, info));
                }
            }
            if rows.len() < fetch_limit {
                // Lock phase exhausted (or resuming inside the type phase).
                let mut type_after: Option<Vec<u8>> = match start_phase {
                    ByScriptPhase::Type => after_key,
                    ByScriptPhase::Lock => None,
                };
                loop {
                    let need = fetch_limit - rows.len();
                    let page = store.list_cells_by_type_code_hash(
                        code_hash,
                        hash_type,
                        need,
                        type_after.as_deref(),
                        append_only_store,
                    )?;
                    let page_len = page.len();
                    for (tx_hash, output_index, info) in page {
                        type_after = Some(keys::encode_cell_code_index_key(
                            code_hash,
                            hash_type,
                            info.created_at_block,
                            &tx_hash,
                            output_index,
                        ));
                        // A cell whose lock script also uses this exact form
                        // was already emitted by the exhausted lock phase.
                        let lock_also_matches = info.cell.lock_code_hash.as_slice() == code_hash
                            && info.cell.lock_hash_type == i16::from(hash_type);
                        if lock_also_matches {
                            continue;
                        }
                        rows.push((ByScriptPhase::Type, tx_hash, output_index, info));
                    }
                    if rows.len() >= fetch_limit || page_len < need {
                        break;
                    }
                }
            }
        }
        other => anyhow::bail!("unsupported script_kind in collect_by_script_rows: {other}"),
    }
    Ok(rows)
}

async fn list_cells_by_script(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsByScriptParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let code_hash_bytes = parse_hash32(&params.code_hash, "code_hash")?;

    let hash_type_num = parse_hash_type(&params.hash_type).ok_or_else(|| {
        ApiError::bad_request("Invalid hash_type. Must be one of: data, type, data1, data2")
    })?;

    let script_kind = params.script_kind.as_str();
    if !matches!(script_kind, "lock" | "type" | "both") {
        return Err(ApiError::bad_request(
            "Invalid script_kind. Must be one of: lock, type, both",
        ));
    }
    let is_both = script_kind == "both";

    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(&state)?;

    // "type" references are not universally available. If this deployment has no type reference,
    // return empty instead of silently mapping to data-family references.
    if hash_type_num == 1 {
        if let Some(info) = merge_script_info_for_reference(&all_script_infos, &code_hash_bytes) {
            let (type_ref, _) = deployment_reference_hashes(&info);
            if type_ref.is_none() {
                return if is_both {
                    ok(CursorPaginatedResponse::without_total(
                        Vec::new(),
                        limit as i64,
                        None,
                    ))
                } else {
                    ok(CursorPaginatedResponse::new(
                        Vec::new(),
                        0,
                        limit as i64,
                        None,
                    ))
                };
            }
        }
    }

    let resolved_code_hash =
        resolve_code_hash_for_hash_type(&all_script_infos, &code_hash_bytes, &params.hash_type)
            .unwrap_or_else(|| code_hash_bytes.clone());

    let expected_prefix = keys::encode_cell_code_index_prefix(&resolved_code_hash, hash_type_num);
    let cursor = params
        .cursor
        .as_deref()
        .map(|cursor| parse_by_script_cursor(cursor, script_kind, &expected_prefix))
        .transpose()?;
    let is_first_page = cursor.is_none();

    // Fetch limit+1 to detect has_more
    let fetch_limit = limit + 1;

    // Store reads run on the blocking pool: the prefix scans and per-row
    // payload loads are synchronous RocksDB I/O.
    let store = state.store.clone();
    let append_only_store = state.append_only_store.clone();
    let script_kind_owned = script_kind.to_string();
    let resolved_code_hash_blocking = resolved_code_hash.clone();
    let (rows, total) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        // Total comes from the per-form reference counters — the same universe
        // as the rows (live cells whose lock/type script is exactly
        // {resolved_code_hash, hash_type}). For `both` the deduplicated union
        // has no cheap exact counter, so total is omitted rather than wrong.
        let total: Option<i64> = if script_kind_owned == "both" {
            None
        } else {
            let reference_info =
                store.get_script_reference_info(hash_type_num, &resolved_code_hash_blocking)?;
            Some(
                match (script_kind_owned.as_str(), reference_info.as_ref()) {
                    ("lock", Some(info)) => info.lock_live_cells_count,
                    ("type", Some(info)) => info.type_live_cells_count,
                    _ => 0,
                },
            )
        };
        let rows = collect_by_script_rows(
            &store,
            &append_only_store,
            &script_kind_owned,
            &resolved_code_hash_blocking,
            hash_type_num,
            fetch_limit,
            cursor,
        )?;
        Ok((rows, total))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    // Rows and total share one universe (live cells of exactly this reference
    // form). A first page larger than the per-form counter means the counter
    // is missing or wrong — fail fast instead of reporting a bogus total.
    if let Some(total) = total {
        if is_first_page && rows.len() as i64 > total {
            return Err(ApiError::internal(format!(
                "cells/by-script rows exceed the per-form reference counter: code_hash=0x{}, hash_type={}, script_kind={}, rows={}, total={}",
                hex::encode(&resolved_code_hash),
                hash_type_num,
                script_kind,
                rows.len(),
                total
            )));
        }
    }

    let has_more = rows.len() > limit;
    let rows: Vec<ByScriptRow> = rows.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        rows.last().map(|(phase, tx_hash, output_index, info)| {
            let key = keys::encode_cell_code_index_key(
                &resolved_code_hash,
                hash_type_num,
                info.created_at_block,
                tx_hash,
                *output_index,
            );
            if is_both {
                format!("{}:{}", phase.as_str(), hex::encode(key))
            } else {
                hex::encode(key)
            }
        })
    } else {
        None
    };

    let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
    let cells: Vec<CellResponse> = rows
        .iter()
        .map(|(phase, tx_hash, output_index, info)| {
            let mut cell = cell_info_to_response(
                tx_hash,
                *output_index,
                info,
                &state.ckb_network,
                virtual_occupied,
            )?;
            cell.matched_script_kind = Some(phase.as_str().to_string());
            Ok(cell)
        })
        .collect::<Result<_, ApiRouteError>>()?;

    match total {
        Some(total) => ok(CursorPaginatedResponse::new(
            cells,
            total,
            limit as i64,
            next_cursor,
        )),
        None => ok(CursorPaginatedResponse::without_total(
            cells,
            limit as i64,
            next_cursor,
        )),
    }
}

async fn get_address(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
) -> ApiResult<AddressResponse> {
    // Check cache first. The cache holds the chain view only; the pending
    // summary is the pool's, and is attached fresh on every request.
    let cache_key = CacheKeys::address_balance(&addr);
    if let Some(mut cached) = state.cache.get::<AddressResponse>(&cache_key).await {
        let lock_hash = parse_hash32(&cached.lock_script_hash, "cached address lock hash")?;
        cached.pending_summary = address_pending_summary(&state, &lock_hash).await?;
        return ok(cached);
    }

    let (lock_hash, canonical_input_address) = if is_ckb_address(&addr) {
        let script = parse_address_to_script(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?;
        let hash = compute_script_hash(&script.code_hash, script.hash_type, &script.args);
        // The response `address` is always the canonical encoding of the lock
        // script on the serving network — computed through the same
        // `script_to_address` path as every other handler, never the raw input
        // echoed back (uppercase input canonicalizes to lowercase, a
        // foreign-HRP input to this network's HRP).
        let canonical = script_to_address(
            &script.code_hash,
            script.hash_type as i16,
            &script.args,
            &state.ckb_network,
        )
        .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?;
        (hash, Some(canonical))
    } else {
        let hash = parse_hash32(&addr, "address/lock script hash")?;
        (hash, None)
    };

    // Get balance from the store
    let store = state.store.clone();
    let lock_hash_c = lock_hash.clone();
    let lock32: [u8; 32] = lock_hash.as_slice().try_into().map_err(|_| {
        ApiError::internal(format!(
            "address lookup expects a 32-byte lock hash, got {} bytes",
            lock_hash.len()
        ))
    })?;
    let (addr_balance, transactions_count) = tokio::task::spawn_blocking(move || {
        // `address_tx_count` is the ONE place cell participations and
        // protocol-named participations are added together.
        Ok::<_, anyhow::Error>((
            store.get_addr_balance(&lock_hash_c)?,
            store.address_tx_count(&lock32)?,
        ))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (balance, used_capacity, live_cells_count) = match &addr_balance {
        Some(ab) => (
            ab.balance.to_string(),
            ab.used_capacity.to_string(),
            ab.live_cells_count as i64,
        ),
        None => ("0".to_string(), "0".to_string(), 0),
    };

    // Resolve the lock script through the single canonical path: `CF_LOCK_SCRIPTS`
    // (written once per lock at cell creation, never deleted). Deriving it from a
    // live cell instead would lose the script the moment an address is fully spent
    // (completed DAO withdrawers, emptied wallets), which is exactly the state the
    // rest of the response still describes.
    let store = state.store.clone();
    let lock_hash_c = lock_hash.clone();
    let lock_script_entry =
        tokio::task::spawn_blocking(move || store.get_lock_script(&lock_hash_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (lock_script, lock_script_info, address) = if let Some(entry) = lock_script_entry {
        let store = state.store.clone();
        let code_hash_c = entry.code_hash.clone();
        let script_info = tokio::task::spawn_blocking(move || store.get_script_info(&code_hash_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

        // The stored per-lock hash_type is authoritative (never a script_info
        // canonical default). Only 0/1/2/4 are valid on chain, so anything else is
        // upstream store corruption and must be reported, not rendered as a guess.
        let hash_type_str = hash_type_to_str(entry.hash_type).ok_or_else(|| {
            ApiError::internal(format!(
                "invalid stored lock hash_type: lock_hash=0x{}, hash_type={}",
                hex::encode(&lock_hash),
                entry.hash_type
            ))
        })?;

        let script = ScriptResponse {
            code_hash: format!("0x{}", hex::encode(&entry.code_hash)),
            hash_type: hash_type_str.to_string(),
            args: format!("0x{}", hex::encode(&entry.args)),
        };

        // Canonical encoding from the stored lock script (identical to the
        // input-derived script when the query was an address: equal lock
        // hashes imply equal scripts).
        let addr = Some(
            script_to_address(
                &entry.code_hash,
                entry.hash_type,
                &entry.args,
                &state.ckb_network,
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to encode address: lock_hash=0x{}, error={}",
                    hex::encode(&lock_hash),
                    e
                ))
            })?,
        );

        let script_info_response = script_info.map(|si| LockScriptInfo {
            code_hash: format!("0x{}", hex::encode(&si.code_hash)),
            name: si.name.unwrap_or_else(|| "Unknown".to_string()),
            script_kind: Some("lock".to_string()),
            deprecated: si.deprecated,
        });

        (Some(script), script_info_response, addr)
    } else {
        // No cell with this lock has ever been indexed, so the store knows no
        // script components: report no guessed `lockScript`. When the query
        // itself was an address, its decoded lock script still yields the
        // canonical encoding (same `script_to_address` path as above).
        (None, None, canonical_input_address)
    };

    let recent_activities_count = transactions_count;

    let response = AddressResponse {
        lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
        address,
        balance,
        used_capacity,
        live_cells_count,
        transactions_count,
        recent_activities_count,
        lock_script,
        lock_script_info,
        pending_summary: None,
    };

    // Cache the chain view for 30 seconds
    state
        .cache
        .set(&cache_key, &response, CacheTtl::ADDRESS_BALANCE)
        .await;

    let mut response = response;
    response.pending_summary = address_pending_summary(&state, &lock_hash).await?;
    ok(response)
}

/// The address's pending summary from the current pool snapshot, through the
/// shared page-one segment.
async fn address_pending_summary(
    state: &Arc<AppState>,
    lock_hash: &[u8],
) -> Result<
    Option<crate::routes::address_pool::PendingSummaryResponse>,
    (axum::http::StatusCode, axum::Json<ApiError>),
> {
    let lock32: [u8; 32] = lock_hash.try_into().map_err(|_| {
        ApiError::internal(format!(
            "address pending summary expects a 32-byte lock hash, got {} bytes",
            lock_hash.len()
        ))
    })?;
    let snapshot = state.pool_mirror.load();
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        crate::routes::address_pool::pending_summary(store.as_ref(), snapshot.as_ref(), &lock32)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))
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
) -> anyhow::Result<Option<DaoInfo>> {
    let outpoint_key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);
    let context = format!("DAO cell 0x{}:{output_index}", hex::encode(tx_hash));

    let entry = store
        .get_dao_deposit(&outpoint_key)
        .map_err(|error| anyhow::anyhow!("{context} deposit read: {error}"))?;

    // If not found by outpoint, try by withdraw-request/withdraw-complete tx hash
    let entry = if entry.is_none() {
        let outpoint_key_data = store
            .get_dao_deposit_by_withdraw_tx(tx_hash, output_index)
            .map_err(|error| anyhow::anyhow!("{context} withdraw index read: {error}"))?;
        if let Some(key_data) = outpoint_key_data {
            let candidate = store
                .get_dao_deposit(&key_data)
                .map_err(|error| anyhow::anyhow!("{context} indexed deposit read: {error}"))?
                .ok_or_else(|| {
                    anyhow::anyhow!("{context} withdraw index points to missing deposit")
                })?;
            let matches_withdraw_request =
                candidate.withdraw_request_output_index == Some(output_index);
            let matches_withdraw_to = candidate.withdraw_to_output_index == Some(output_index);
            if matches_withdraw_request || matches_withdraw_to {
                Some(candidate)
            } else {
                anyhow::bail!("{context} withdraw index points to mismatched output index")
            }
        } else {
            None
        }
    } else {
        entry
    };
    let Some(entry) = entry else {
        return Ok(None);
    };

    let dao_status = match entry.status {
        0 => "deposited",
        1 => "withdrawing",
        2 => "withdrawn",
        _ => anyhow::bail!("{context} has invalid DAO status {}", entry.status),
    }
    .to_string();

    // Get block header for deposit timestamp
    let timestamp_for_block = |bn: i64| -> anyhow::Result<String> {
        let header = store
            .get_block_header(bn)
            .map_err(|error| anyhow::anyhow!("{context} block {bn} header read: {error}"))?
            .ok_or_else(|| anyhow::anyhow!("{context} missing block {bn} header"))?;
        let timestamp =
            chrono::DateTime::from_timestamp_millis(header.timestamp).ok_or_else(|| {
                anyhow::anyhow!(
                    "{context} block {bn} invalid timestamp {}",
                    header.timestamp
                )
            })?;
        Ok(timestamp.to_rfc3339())
    };
    let deposit_timestamp = timestamp_for_block(entry.deposit_block_number)?;
    let withdraw_request_timestamp = entry
        .withdraw_request_block
        .map(timestamp_for_block)
        .transpose()?;
    let withdraw_timestamp = entry.withdraw_block.map(timestamp_for_block).transpose()?;

    let compensation = entry.compensation.map(|c| c.to_string());
    let compensation_ckb = compensation.as_ref().map(|c| shannon_to_ckb(c));

    Ok(Some(DaoInfo {
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
    }))
}

async fn get_cell(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((tx_hash, output_index)): axum::extract::Path<(String, i32)>,
) -> ApiResult<CellDetailResponse> {
    let hash_bytes = parse_hash32(&tx_hash, "tx_hash")?;

    // The old `as i16` wrapped: output 65536 became output 0 and this handler
    // served output 0's body as the requested outpoint.
    let output_idx = parse_output_index(output_index, "output index")?;

    // Try live cells first, then consumed
    let store = state.store.clone();
    let ao_store = state.append_only_store.clone();
    let hash_c = hash_bytes.clone();
    let (live_cell, consumed_cell) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let live_cell = store.get_cell(&hash_c, output_idx, &ao_store)?;
        let consumed_cell = if live_cell.is_none() {
            store.get_consumed_cell_info(&hash_c, output_idx, &ao_store)?
        } else {
            None
        };
        Ok((live_cell, consumed_cell))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (info, status_str, consumed_meta): (
        ckbadger_store::PositionedCellInfo,
        &str,
        Option<(i64, Option<Vec<u8>>)>,
    ) = match (live_cell, consumed_cell) {
        (Some(cell), _) => (cell, "live", None),
        (None, Some(cell)) => (
            cell.to_positioned_cell_info(),
            "dead",
            Some((cell.consumed_at_block, cell.consumed_by_tx)),
        ),
        (None, None) => return Err(ApiError::not_found("Cell not found")),
    };

    // Use the cell's own stored hash_type (not from script_info, which is a canonical
    // default and may differ from the actual per-cell hash_type). Only 0/1/2/4 are
    // valid on chain, so anything else is upstream store corruption and must be
    // reported, not rendered as a guess — the same rule `get_address` applies, via
    // the same `hash_type_to_str` so this module keeps one conversion path.
    let outpoint = format!("0x{}:{}", hex::encode(&hash_bytes), output_idx);
    let lock_hash_type_num: i16 = info.lock_hash_type;
    let lock_hash_type_str = hash_type_to_str(lock_hash_type_num).ok_or_else(|| {
        ApiError::internal(format!(
            "invalid stored lock hash_type: outpoint={}, hash_type={}",
            outpoint, lock_hash_type_num
        ))
    })?;

    let type_script = if let Some(code_hash) = info.type_code_hash.as_ref() {
        // Use the cell's own stored type_hash_type (not script_info canonical
        // default). A cell that has a type code_hash always had a hash_type on
        // chain, so a missing one is an incomplete indexer write, not a default.
        let type_hash_type_num: i16 = info.type_hash_type.ok_or_else(|| {
            ApiError::internal(format!(
                "cell has a type script code_hash but no stored type hash_type: outpoint={}",
                outpoint
            ))
        })?;
        let type_hash_type_str = hash_type_to_str(type_hash_type_num).ok_or_else(|| {
            ApiError::internal(format!(
                "invalid stored type hash_type: outpoint={}, hash_type={}",
                outpoint, type_hash_type_num
            ))
        })?;
        Some(ScriptResponse {
            code_hash: format!("0x{}", hex::encode(code_hash)),
            hash_type: type_hash_type_str.to_string(),
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

    // Every cell has a lock, so it always has an address. Swallowing the encoder
    // error rendered `address: null`, indistinguishable from a cell that has none.
    let address = Some(
        script_to_address(
            &info.lock_code_hash,
            lock_hash_type_num,
            &info.lock_args,
            &state.ckb_network,
        )
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to encode address: outpoint={}, error={}",
                outpoint, e
            ))
        })?,
    );

    if info.data_size < 0 {
        return Err(ApiError::internal(format!(
            "negative data size for {outpoint}"
        )));
    }
    let node_tx = if let Some(reader) = state.ckb_store.as_ref() {
        let hash: [u8; 32] = hash_bytes.as_slice().try_into().expect("validated hash");
        Some(reader.get_transaction(&hash).ok_or_else(|| {
            ApiError::internal(format!(
                "cell transaction for {outpoint} missing from CKB RocksDB"
            ))
        })?)
    } else {
        None
    };
    if info.data_size > 0 && node_tx.is_none() {
        return Err(ApiError::internal(format!(
            "cell data for {outpoint} requires CKB RocksDB reader"
        )));
    }
    let raw_output_bytes = node_tx
        .as_ref()
        .map(|tx| {
            tx.outputs()
                .get(output_index as usize)
                .map(|output| output.as_slice().to_vec())
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "cell output for {outpoint} missing from CKB RocksDB"
                    ))
                })
        })
        .transpose()?;
    let cell_data = Some(if let Some(tx) = node_tx.as_ref() {
        let data = tx
            .outputs_data()
            .get(output_index as usize)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "cell data output for {outpoint} missing from CKB RocksDB"
                ))
            })?
            .raw_data()
            .to_vec();
        if data.len() != info.data_size as usize {
            return Err(ApiError::internal(format!(
                "cell data size mismatch for {outpoint}: payload={} node={}",
                info.data_size,
                data.len()
            )));
        }
        data
    } else {
        Vec::new()
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
    let elf = cell_data
        .as_ref()
        .map(|data| cell_elf::parse(data))
        .transpose()
        .map_err(|error| {
            ApiError::internal(format!("ELF parse failed: outpoint={outpoint}: {error:#}"))
        })?
        .flatten();
    let code_cell = if elf.is_some() {
        let hash = data_hash.as_ref().expect("ELF bytes were hashed");
        let mut current_references = Vec::new();
        for (reference_hash, hash_type_num, hash_type_name) in [
            (hash.as_slice(), 0u8, "data"),
            (hash.as_slice(), 2u8, "data1"),
            (hash.as_slice(), 4u8, "data2"),
        ]
        .into_iter()
        .chain(
            info.type_script_hash
                .as_ref()
                .map(|type_hash| (type_hash.as_slice(), 1u8, "type")),
        ) {
            if hash_type_num == 1 {
                let current = state
                    .store
                    .get_script_reference_version_hash(1, reference_hash)
                    .map_err(|error| {
                        ApiError::internal(format!(
                            "code cell type resolution read: {outpoint}: {error}"
                        ))
                    })?;
                if current.as_deref() != Some(hash.as_slice()) {
                    continue;
                }
            }
            if let Some(reference) = state
                .store
                .get_script_reference_info(hash_type_num, reference_hash)
                .map_err(|error| {
                    ApiError::internal(format!("code cell reference read: {outpoint}: {error}"))
                })?
            {
                current_references.push(CodeCellReference {
                    reference_hash: format!("0x{}", hex::encode(reference_hash)),
                    hash_type: hash_type_name.into(),
                    lock_live_cells_count: reference.lock_live_cells_count,
                    type_live_cells_count: reference.type_live_cells_count,
                });
            }
        }
        Some(CodeCellEvidence {
            version_hash: format!("0x{}", hex::encode(hash)),
            current_references,
            upgrade_cohort: code_cell_upgrade_cohort(
                &state,
                node_tx
                    .as_ref()
                    .expect("ELF data requires original transaction"),
                &hash_bytes,
                &outpoint,
            )?,
            type_id_lineage: state
                .store
                .get_script_version(hash)
                .map_err(|error| {
                    ApiError::internal(format!("cell {outpoint} Type ID lineage read: {error}"))
                })?
                .map(|version| {
                    version
                        .type_id_deployments
                        .into_iter()
                        .map(|(type_hash, block)| CodeCellTypeIdLineage {
                            type_script_hash: format!("0x{}", hex::encode(type_hash)),
                            observed_at_block: block,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    } else {
        None
    };

    let mut data_analysis = cell_data
        .as_ref()
        .map(|d| analyze_cell_data(&info, d, info.data_size))
        .transpose()
        .map_err(|e| {
            ApiError::internal(format!(
                "cell data analysis failed: outpoint={outpoint}: {e:#}"
            ))
        })?;

    if let (Some(analysis), Some(type_hash)) = (&mut data_analysis, &info.type_script_hash) {
        if analysis
            .deterministic
            .as_ref()
            .is_some_and(|decode| decode.kind == "udt_amount")
        {
            let token = state
                .store
                .get_token(type_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "recognized token cell {outpoint} lacks indexed TokenInfo"
                    ))
                })?;
            enrich_token_decode(analysis, &token).map_err(|e| ApiError::internal(e.to_string()))?;
        }
    }

    let occupied_capacity_breakdown = estimated_occupied_capacity_breakdown(&info);
    let expected_occupied = occupied_capacity_breakdown
        .total_bytes
        .checked_mul(SHANNONS_PER_CKB)
        .ok_or_else(|| ApiError::internal(format!("cell {outpoint} occupied capacity overflow")))?;
    if info.occupied_capacity != expected_occupied {
        return Err(ApiError::internal(format!(
            "cell {outpoint} indexed occupied capacity {} differs from exact anatomy {}",
            info.occupied_capacity, expected_occupied
        )));
    }
    let occupied_capacity = info.occupied_capacity;

    let is_satoshi = info.created_at_block == 0
        && ckbadger_common::burn_policy::burn_policy(&state.ckb_network)
            .is_some_and(|p| info.lock_args.as_slice() == p.lock_args);
    let (cell_type, virtual_occupied_capacity) = if is_satoshi {
        let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
        (
            Some("genesis_special_burn".to_string()),
            Some(virtual_occupied.to_string()),
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

    let timestamp_at = |block: i64| -> Result<(i64, String), ApiRouteError> {
        let header = state
            .store
            .get_block_header(block)
            .map_err(|error| {
                ApiError::internal(format!(
                    "cell {outpoint} block {block} header read: {error}"
                ))
            })?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "cell {outpoint} missing indexed block {block} header"
                ))
            })?;
        let date = chrono::DateTime::from_timestamp_millis(header.timestamp).ok_or_else(|| {
            ApiError::internal(format!(
                "cell {outpoint} block {block} invalid timestamp {}",
                header.timestamp
            ))
        })?;
        Ok((header.chain_time_ms, date.to_rfc3339()))
    };
    let (created_ms, created_timestamp) = timestamp_at(info.created_at_block)?;
    let (spent_timestamp, lifetime_ms) = if let Some(block) = consumed_at_block {
        let (spent_ms, timestamp) = timestamp_at(block)?;
        let lifetime = spent_ms.checked_sub(created_ms).ok_or_else(|| {
            ApiError::internal(format!(
                "cell {outpoint} lifetime overflow: created={created_ms} spent={spent_ms}"
            ))
        })?;
        if lifetime < 0 {
            return Err(ApiError::internal(format!(
                "cell {outpoint} spent before creation: created={created_ms} spent={spent_ms}"
            )));
        }
        (Some(timestamp), Some(lifetime))
    } else {
        (None, None)
    };
    let (live_age_ms, live_age_at_block) = if consumed_at_block.is_none() {
        if let Some((tip_block, tip_header)) =
            state.store.get_sync_tip_block().map_err(|error| {
                ApiError::internal(format!("cell {outpoint} sync tip read: {error}"))
            })?
        {
            if tip_block < info.created_at_block {
                return Err(ApiError::internal(format!(
                    "cell {outpoint} created at block {} beyond indexed tip {tip_block}",
                    info.created_at_block
                )));
            }
            (
                Some(
                    tip_header
                        .chain_time_ms
                        .checked_sub(created_ms)
                        .ok_or_else(|| {
                            ApiError::internal(format!(
                                "cell {outpoint} live age overflow at tip {tip_block}"
                            ))
                        })?,
                ),
                Some(tip_block),
            )
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };
    let consuming_input_index = if let Some(tx_hash) = consumed_by_tx.as_ref() {
        use ckb_types::prelude::Unpack;
        let reader = state.ckb_store.as_ref().ok_or_else(|| {
            ApiError::internal(format!(
                "spending transaction for cell {outpoint} requires CKB RocksDB reader"
            ))
        })?;
        let spent_hash: [u8; 32] = hex::decode(tx_hash.trim_start_matches("0x"))
            .expect("indexed spending transaction hash is hex")
            .try_into()
            .map_err(|_| {
                ApiError::internal(format!(
                    "cell {outpoint} has invalid consuming tx hash {tx_hash}"
                ))
            })?;
        let tx = reader.get_transaction(&spent_hash).ok_or_else(|| {
            ApiError::internal(format!(
                "spending transaction {tx_hash} for cell {outpoint} missing from CKB RocksDB"
            ))
        })?;
        let matches: Vec<usize> = tx
            .inputs()
            .into_iter()
            .enumerate()
            .filter_map(|(index, input)| {
                let prev = input.previous_output();
                let prev_index: u32 = prev.index().unpack();
                (prev.tx_hash().as_slice() == hash_bytes.as_slice()
                    && prev_index == u32::from(output_idx as u16))
                .then_some(index)
            })
            .collect();
        if matches.len() != 1 {
            return Err(ApiError::internal(format!("spending transaction {tx_hash} references cell {outpoint} {} times (expected exactly once)", matches.len())));
        }
        Some(
            u32::try_from(matches[0])
                .map_err(|_| ApiError::internal(format!("cell {outpoint} input index overflow")))?,
        )
    } else {
        None
    };

    let dao_info = lookup_dao_info(&state.store, &hash_bytes, output_idx)
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let cell_output_molecule = cell_anatomy::cell_output(&info).map_err(|error| {
        ApiError::internal(format!(
            "CellOutput serialization failed: outpoint={outpoint}: {error:#}"
        ))
    })?;
    if let Some(raw) = raw_output_bytes {
        let serialized = hex::decode(cell_output_molecule.bytes.trim_start_matches("0x"))
            .expect("Molecule encoder returned hex");
        if raw != serialized {
            return Err(ApiError::internal(format!(
                "cell {outpoint} indexed output differs from original CKB transaction CellOutput"
            )));
        }
    }
    let indexed_lock_hash = format!("0x{}", hex::encode(&info.lock_script_hash));
    if cell_output_molecule.lock_script_hash != indexed_lock_hash {
        return Err(ApiError::internal(format!(
            "cell {outpoint} lock script hash mismatch: serialized={} indexed={indexed_lock_hash}",
            cell_output_molecule.lock_script_hash
        )));
    }
    let indexed_type_hash = info
        .type_script_hash
        .as_ref()
        .map(|hash| format!("0x{}", hex::encode(hash)));
    if cell_output_molecule.type_script_hash != indexed_type_hash {
        return Err(ApiError::internal(format!("cell {outpoint} type script hash mismatch: serialized={:?} indexed={indexed_type_hash:?}", cell_output_molecule.type_script_hash)));
    }
    match (info.data_hash.as_ref(), data_hash.as_ref()) {
        (Some(indexed), Some(actual)) if indexed != actual => {
            return Err(ApiError::internal(format!(
                "cell {outpoint} data hash mismatch: indexed=0x{} actual=0x{}",
                hex::encode(indexed),
                hex::encode(actual)
            )));
        }
        (None, Some(_)) if info.data_size > 0 => {
            return Err(ApiError::internal(format!(
                "cell {outpoint} indexed data hash missing for {} bytes",
                info.data_size
            )))
        }
        (Some(_), None) => {
            return Err(ApiError::internal(format!(
                "cell {outpoint} indexed data hash exists but original bytes are unavailable"
            )))
        }
        _ => {}
    }

    let current_spend_requirements = current_spend_requirements(&state, &info, &outpoint)?;

    ok(CellDetailResponse {
        tx_hash: format!("0x{}", hex::encode(&hash_bytes)),
        output_index: output_idx as i32,
        capacity: info.capacity.to_string(),
        used_capacity: occupied_capacity.to_string(),
        used_capacity_breakdown: occupied_capacity_breakdown,
        virtual_used_capacity: virtual_occupied_capacity,
        cell_type,
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        address,
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        created_timestamp,
        status: status_str.to_string(),
        consumed_at_block,
        consumed_by_tx,
        spent_timestamp,
        consuming_input_index,
        lifetime_ms,
        live_age_ms,
        live_age_at_block,
        lock: ScriptResponse {
            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
            hash_type: lock_hash_type_str.to_string(),
            args: format!("0x{}", hex::encode(&info.lock_args)),
        },
        type_script,
        protocol_script: cell_protocol_script(&info),
        data: cell_data.map(|d| format!("0x{}", hex::encode(d))),
        data_analysis,
        is_dep_group: dep_group_result.is_dep_group,
        dep_group_items: dep_group_result.items,
        code_cell_of,
        cell_output_molecule,
        elf,
        code_cell,
        current_spend_requirements,
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
    Err(ApiError::warmup_pending(
        "top addresses cache unavailable; warmup in progress",
    ))
}

async fn get_active_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActiveAddressesParams>,
) -> ApiResult<Vec<ActiveAddressResponse>> {
    let store = state.store.clone();
    let sync_status = tokio::task::spawn_blocking(move || store.get_sync_status())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let limit = params.limit.clamp(1, 500) as usize;
    let days = params.days.clamp(1, 365);

    let store = state.store.clone();
    let min_block = tokio::task::spawn_blocking(move || {
        active_addresses_min_block(&store, sync_status.tip_block_number, days)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?
    .map_err(|error| ApiError::internal(error.to_string()))?;

    if let Some(cached) = state
        .mem_cache
        .get::<Vec<CachedAddressEntry>>(CACHE_KEY_ADDRESSES_ACTIVE)
    {
        return ok(active_addresses_from_cache(cached, min_block, limit));
    }
    Err(ApiError::warmup_pending(
        "active addresses cache unavailable; warmup in progress",
    ))
}

fn active_addresses_min_block(
    store: &ckbadger_store::CkbadgerStore,
    tip_block: i64,
    days: i64,
) -> anyhow::Result<i64> {
    let tip = store.get_block_header(tip_block)?.ok_or_else(|| {
        anyhow::anyhow!("missing canonical tip header for active addresses: block={tip_block}")
    })?;
    let window_ms = days
        .checked_mul(86_400_000)
        .ok_or_else(|| anyhow::anyhow!("active-address window overflow: days={days}"))?;
    let boundary_ms = tip.chain_time_ms.checked_sub(window_ms).ok_or_else(|| {
        anyhow::anyhow!(
            "active-address boundary overflow: tip={tip_block} chain_time_ms={} days={days}",
            tip.chain_time_ms
        )
    })?;
    Ok(store
        .find_last_block_at_or_before_chain_time_ms(boundary_ms)?
        .map_or(0, |block| block + 1))
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
) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, ckbadger_store::AddrTxValue)>> {
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
        for (block_num, tx_idx, tx_hash, addr_tx_value) in rows {
            last_seen = Some((block_num, tx_idx));
            if is_canonical_addr_tx(store, block_num, tx_idx, &tx_hash)? {
                out.push((block_num, tx_idx, tx_hash, addr_tx_value));
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

/// One `addr_txs_by_prefix` row.
///
/// The prefix index is the half of an address's history that a protocol named
/// rather than a cell recorded, so it has its own read path: verification and
/// debugging need to see it as itself, not merged into an address that may not
/// even be resolvable.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixTransactionResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub capacity_change: String,
    pub tx_type: String,
    pub tags: u16,
}

/// `GET /addresses/prefix/{prefix}/transactions` — the transactions a protocol
/// named this 20-byte lock-hash prefix in.
async fn get_prefix_transactions(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(prefix): axum::extract::Path<String>,
    Query(params): Query<AddressTxParams>,
) -> ApiResult<CursorPaginatedResponse<PrefixTransactionResponse>> {
    let bytes = crate::utils::hash::parse_lock_hash_prefix20(&prefix, "lock hash prefix")?;
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;

    let store = state.store.clone();
    let rows = tokio::task::spawn_blocking(move || {
        store.list_addr_txs_by_prefix_recent(&bytes, limit + 1, cursor)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() > limit;
    let page = &rows[..rows.len().min(limit)];
    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let txs = page
        .iter()
        .map(
            |(block_number, tx_index, tx_hash, value)| PrefixTransactionResponse {
                tx_hash: format!("0x{}", hex::encode(tx_hash)),
                block_number: *block_number,
                tx_index: *tx_index,
                capacity_change: value.capacity_change.to_string(),
                tx_type: value.tx_type_str().to_string(),
                tags: value.tags,
            },
        )
        .collect();

    ok(CursorPaginatedResponse::without_total(
        txs,
        limit as i64,
        next_cursor,
    ))
}

/// One `.cell` name an address holds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressDotCellNameResponse {
    pub identity_id: String,
    pub label: String,
    pub name: String,
    pub expired_at: u64,
}

/// `GET /addresses/{addr}/dotcell-names` — the `.cell` names this address owns.
///
/// A name's owner is the first 20 bytes of its owner's lock script hash, so
/// the lookup is a prefix seek on the address's own lock hash — no scan, and
/// no guess about which full hash a prefix belongs to.
async fn get_address_dotcell_names(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AddressTxParams>,
) -> ApiResult<CursorPaginatedResponse<AddressDotCellNameResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        parse_hash32(&addr, "address/lock script hash")?
    };
    let owner20: [u8; 20] = lock_hash[..20].try_into().expect("20 bytes");
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor: Option<[u8; 20]> = match params.cursor.as_deref() {
        Some(raw) => {
            let bytes = crate::utils::hash::parse_lock_hash_prefix20(raw, ".cell name cursor")?;
            Some(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| ApiError::bad_request(".cell name cursor must be 20 bytes"))?,
            )
        }
        None => None,
    };

    let store = state.store.clone();
    let ids = tokio::task::spawn_blocking(move || {
        store.list_dotcell_names_by_owner20(&owner20, cursor, limit + 1)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = ids.len() > limit;
    let page = &ids[..ids.len().min(limit)];
    let next_cursor = if has_more {
        page.last().map(|id| format!("0x{}", hex::encode(id)))
    } else {
        None
    };

    let mut rows = Vec::with_capacity(page.len());
    for id in page {
        let entry = state
            .store
            .get_identity(id)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    ".cell owner index points at a missing identity: identity_id=0x{}",
                    hex::encode(id)
                ))
            })?;
        let (label, expired_at) = match &entry.extra {
            ckbadger_store::types::IdentityExtra::DotCell {
                label, expired_at, ..
            } => (label.clone(), *expired_at),
            _ => {
                return Err(ApiError::internal(format!(
                    ".cell identity has wrong extra variant: identity_id=0x{}",
                    hex::encode(id)
                )))
            }
        };
        rows.push(AddressDotCellNameResponse {
            identity_id: format!("0x{}", hex::encode(id)),
            name: format!("{label}.cell"),
            label,
            expired_at,
        });
    }

    ok(CursorPaginatedResponse::without_total(
        rows,
        limit as i64,
        next_cursor,
    ))
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
        parse_hash32(&addr, "address/lock script hash")?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;

    // Tx-pool rows are a page-one-only segment, above every committed row: a
    // pool transaction can only land in a future block. They never enter a
    // cursor, so pagination stays stable while the pool churns.
    let pool_rows = if cursor.is_none() {
        let snapshot = state.pool_mirror.load();
        let store = state.store.clone();
        let lock32: [u8; 32] = lock_hash.as_slice().try_into().map_err(|_| {
            ApiError::internal(format!(
                "address transactions expect a 32-byte lock hash, got {} bytes",
                lock_hash.len()
            ))
        })?;
        let (rows, summary) = tokio::task::spawn_blocking(move || {
            build_pool_transaction_rows(store.as_ref(), snapshot.as_ref(), &lock32)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
        Some((rows, summary))
    } else {
        None
    };

    // Fetch canonical recent transactions for this address (newest first).
    // Each row now includes the materialized AddrTxValue with capacity_change and tx_type.
    let store = state.store.clone();
    let lock_hash_c = lock_hash.clone();
    let addr_txs = tokio::task::spawn_blocking(move || {
        list_canonical_addr_txs_page(
            store.as_ref(),
            store.as_ref(),
            &lock_hash_c,
            limit + 1,
            cursor,
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = addr_txs.len() > limit;
    let addr_txs: Vec<_> = addr_txs.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        addr_txs
            .last()
            .map(|(block_num, tx_idx, _, _)| encode_cursor(*block_num, *tx_idx))
    } else {
        None
    };

    // Deduplicate block header lookups: collect unique block numbers first.
    let unique_blocks: Vec<i64> = {
        let mut blocks: Vec<i64> = addr_txs.iter().map(|(bn, _, _, _)| *bn).collect();
        blocks.sort_unstable();
        blocks.dedup();
        blocks
    };
    // Every row here is canonical, so its block header exists; a read failure,
    // a missing header or an unrenderable timestamp is store corruption,
    // reported with the block rather than served as an empty timestamp.
    let mut block_timestamps: HashMap<i64, String> = HashMap::with_capacity(unique_blocks.len());
    for block_num in unique_blocks {
        let header = state
            .store
            .get_block_header(block_num)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to read block header for block {block_num} (address transactions): {e}"
                ))
            })?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "missing block header for block {block_num}, which holds a canonical address transaction"
                ))
            })?;
        let ts = chrono::DateTime::from_timestamp_millis(header.timestamp)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "block {block_num} header timestamp {} ms is out of range",
                    header.timestamp
                ))
            })?
            .to_rfc3339();
        block_timestamps.insert(block_num, ts);
    }

    let committed_txs: Vec<AddressTransactionResponse> = addr_txs
        .into_iter()
        .map(
            |(block_number, tx_idx, tx_hash, addr_val)| -> Result<
                AddressTransactionResponse,
                (axum::http::StatusCode, axum::Json<ApiError>),
            > {
                let timestamp = block_timestamps.get(&block_number).cloned().ok_or_else(|| {
                    ApiError::internal(format!(
                        "address transaction at block {block_number} has no resolved block timestamp"
                    ))
                })?;

                let tx_entry = state
                    .store
                    .get_tx_index(block_number, tx_idx)
                    .map_err(|e| {
                        ApiError::internal(format!(
                            "failed to read tx_index for tx 0x{} at block {}:{}: {}",
                            hex::encode(&tx_hash),
                            block_number,
                            tx_idx,
                            e
                        ))
                    })?
                    .ok_or_else(|| {
                        ApiError::internal(format!(
                            "missing tx_index for canonical tx 0x{} at block {}:{}",
                            hex::encode(&tx_hash),
                            block_number,
                            tx_idx
                        ))
                    })?;

                // capacity_change and tx_type come from materialized AddrTxValue — no cell reads needed.
                let capacity_change = addr_val.capacity_change;
                let tx_type = addr_val.tx_type_str();

                // fee is already DAO-corrected at index time — no compensation lookups needed.
                let fee = tx_entry.fee;

                let inputs_count = tx_entry.inputs_count;
                let outputs_count = tx_entry.outputs_count;
                let is_cellbase = tx_entry.is_cellbase;
                let tx_size = Some(crate::routes::transactions::tx_serialized_size_in_block(
                    tx_entry.tx_size,
                ));
                let (cycles, cycles_status) = crate::routes::transactions::display_cycles(
                    tx_entry.cycles, is_cellbase || block_number == 0, &tx_hash,
                )?;

                // Script labels from semantic_tags bitmap (single calculation path).
                // semantic_tags == 0 means "plain CKB transfer" → empty labels is correct.
                let script_labels = script_labels_from_semantic_tags(tx_entry.semantic_tags);

                Ok(AddressTransactionResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    block_number: Some(block_number),
                    tx_type: tx_type.to_string(),
                    capacity_change: (capacity_change as i128).to_string(),
                    timestamp: Some(timestamp),
                    pool_status: None,
                    time_added_to_pool: None,
                    interpretation: None,
                    inputs_count,
                    outputs_count,
                    fee: fee.to_string(),
                    is_cellbase,
                    tx_size,
                    cycles,
                    cycles_status,
                    script_labels,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    // `total` stays the COMMITTED transaction count — cell participations plus
    // protocol-named ones, through the one summing helper. Pool rows are
    // reported beside it in `pool`, never added to it.
    let total_lock: [u8; 32] = lock_hash.as_slice().try_into().map_err(|_| {
        ApiError::internal(format!(
            "address transactions expect a 32-byte lock hash, got {} bytes",
            lock_hash.len()
        ))
    })?;
    let total = state
        .store
        .address_tx_count(&total_lock)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (mut txs, pool) = match pool_rows {
        Some((rows, summary)) => (rows, Some(summary)),
        None => (Vec::new(), None),
    };
    txs.extend(committed_txs);

    let response = CursorPaginatedResponse::new(txs, total, limit as i64, next_cursor);
    ok(match pool {
        Some(pool) => response.with_pool(pool),
        None => response,
    })
}

/// This lock's tx-pool rows for page one, and the summary describing them:
/// the shared page-one segment, every row of it (this list takes no filter).
///
/// Every value is either the node's own exact pool-entry figure (fee, size,
/// cycles) or comes from the same derivation the indexer uses for committed
/// rows — `AddrTxValue::new` for capacity change and tx type, the semantic-tag
/// bitmap for script labels — so a row cannot change meaning when it commits.
///
/// Blocking: reads the store for the committed-wins dedup.
fn build_pool_transaction_rows(
    store: &CkbadgerStore,
    snapshot: &crate::pool::PoolSnapshot,
    lock_hash: &[u8; 32],
) -> anyhow::Result<(
    Vec<AddressTransactionResponse>,
    crate::pool::PoolSummaryResponse,
)> {
    let segment = crate::routes::address_pool::page_one_segment(store, snapshot, lock_hash)?;

    let mut rows = Vec::with_capacity(segment.rows.len());
    for row in &segment.rows {
        let record = &row.record;
        let addr_tx = &row.participant.addr_tx;
        rows.push(AddressTransactionResponse {
            tx_hash: format!("0x{}", hex::encode(record.tx_hash)),
            block_number: None,
            tx_type: addr_tx.tx_type_str().to_string(),
            capacity_change: (addr_tx.capacity_change as i128).to_string(),
            timestamp: None,
            pool_status: Some(record.pool_status.as_str().to_string()),
            time_added_to_pool: Some(
                crate::pool::pool_timestamp_rfc3339(i64::try_from(
                    record.entry.time_added_to_pool_ms,
                )?)
                .map_err(|e| anyhow::anyhow!("{e}"))?,
            ),
            interpretation: Some(crate::pool::InterpretationResponse::from(
                &record.interpretation,
            )),
            inputs_count: record.inputs_count,
            outputs_count: record.outputs_count,
            fee: record.entry.fee.to_string(),
            is_cellbase: record.is_cellbase,
            tx_size: Some(i32::try_from(record.entry.size)?),
            cycles: Some(i64::try_from(record.entry.cycles)?),
            cycles_status: "complete".into(),
            script_labels: script_labels_from_semantic_tags(record.semantic_tags),
        });
    }

    let summary =
        crate::routes::address_pool::pool_summary(snapshot, &segment.rows, segment.truncated)?;
    Ok((rows, summary))
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
        parse_hash32(&addr, "address/lock script hash")?
    };

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = params
        .cursor
        .as_deref()
        .map(parse_address_token_cursor)
        .transpose()?;

    let mut token_balances = state
        .store
        .list_address_tokens_by_balance(&lock_hash, limit + 1, cursor)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = token_balances.len() > limit;
    if has_more {
        token_balances.truncate(limit);
    }

    let token_type_hashes: Vec<Vec<u8>> = token_balances
        .iter()
        .map(|(type_hash, _)| type_hash.clone())
        .collect();
    let store = state.store.clone();
    let token_infos =
        tokio::task::spawn_blocking(move || store.get_tokens_batch(&token_type_hashes))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let token_info_by_hash: HashMap<Vec<u8>, ckbadger_store::TokenInfo> = token_infos
        .into_iter()
        .map(|(type_hash, info)| match info {
            Some(info) => Ok((type_hash, info)),
            None => Err(ApiError::internal(format!(
                "missing token metadata for addr token index: lock_hash=0x{}, type_hash=0x{}",
                hex::encode(&lock_hash),
                hex::encode(&type_hash)
            ))),
        })
        .collect::<Result<_, _>>()?;

    let next_cursor: Option<String> = if has_more {
        token_balances
            .last()
            .map(|(type_hash, balance)| format!("{}:{}", balance, hex::encode(type_hash)))
    } else {
        None
    };

    let tokens: Vec<AddressTokenResponse> = token_balances
        .into_iter()
        .map(|(type_hash, balance)| {
            let token_info = token_info_by_hash.get(&type_hash).ok_or_else(|| {
                ApiError::internal(format!(
                    "missing batch token metadata after lookup: lock_hash=0x{}, type_hash=0x{}",
                    hex::encode(&lock_hash),
                    hex::encode(&type_hash)
                ))
            })?;
            Ok(AddressTokenResponse {
                type_script_hash: format!("0x{}", hex::encode(&type_hash)),
                standard: token_info.standard.clone(),
                name: token_info.name.clone(),
                symbol: token_info.symbol.clone(),
                decimals: token_info.decimals.map(|d| d as i16),
                icon_url: token_info.icon_url.clone(),
                balance: balance.to_string(),
            })
        })
        .collect::<Result<_, _>>()?;

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

    /// The mainnet-derived `baseline.virtual_occupied` (shannons): the genesis
    /// burnt capacity (8.4B CKB) times the 6/10 occupied ratio == 504e15.
    const VIRTUAL_OCCUPIED_MAINNET: i128 = 504_000_000_000_000_000;

    fn make_payload() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 10000000000,
            lock_script_hash: vec![0u8; 32],
            lock_code_hash: vec![1u8; 32],
            lock_hash_type: 1,
            lock_args: vec![2u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 6_100_000_000,
            udt_amount: None,
            data_hash: None,
        }
    }

    fn make_info() -> ckbadger_store::PositionedCellInfo {
        ckbadger_store::PositionedCellInfo::new(make_payload(), 100)
    }

    fn positioned(cell: LiveCellInfo) -> ckbadger_store::PositionedCellInfo {
        ckbadger_store::PositionedCellInfo::new(cell, 100)
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
        let resp =
            cell_info_to_response(&tx_hash, 0, &info, "mainnet", VIRTUAL_OCCUPIED_MAINNET).unwrap();
        assert_eq!(resp.output_index, 0);
        assert_eq!(resp.capacity, "10000000000");
        assert!(resp.cell_type.is_none());
        assert!(resp.virtual_used_capacity.is_none());
        assert!(resp.udt_amount.is_none());
    }

    #[test]
    fn type_reference_candidates_exclude_other_anchors_and_consumed_copies() {
        let wanted = vec![0x11; 32];
        let other = vec![0x22; 32];
        let version = vec![0x33; 32];
        let typed = |reference: Vec<u8>| {
            let mut payload = make_payload();
            payload.type_script_hash = Some(reference);
            payload.type_code_hash = Some(vec![0x44; 32]);
            payload.type_hash_type = Some(1);
            payload.type_args = Some(vec![]);
            positioned(payload)
        };
        let entries = vec![
            (vec![1; 32], 0, typed(other), true),
            (vec![2; 32], 0, typed(wanted.clone()), false),
            (vec![3; 32], 0, typed(wanted.clone()), true),
        ];
        let (live, historical) = eligible_reference_cells(1, &wanted, &version, entries);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].tx_hash, format!("0x{}", hex::encode([3; 32])));
        assert_eq!(historical.len(), 1);
        assert_eq!(historical[0].tx_hash, format!("0x{}", hex::encode([2; 32])));
        assert_eq!(
            available_reference_resolution("resolved", std::slice::from_ref(&version), &live),
            "resolved"
        );
        assert_eq!(
            available_reference_resolution("resolved", &[version], &[]),
            "unavailable"
        );
    }

    #[test]
    fn test_cell_info_to_response_preserves_udt_amount() {
        let info = LiveCellInfo {
            udt_amount: Some(12_345),
            ..make_payload()
        };
        let info = positioned(info);
        let tx_hash = vec![4u8; 32];
        let resp =
            cell_info_to_response(&tx_hash, 1, &info, "mainnet", VIRTUAL_OCCUPIED_MAINNET).unwrap();
        assert_eq!(resp.udt_amount.as_deref(), Some("12345"));
    }

    /// A genesis (block 0) output whose lock args equal the Satoshi dead-address
    /// pubkey hash is tagged `genesis_special_burn` and reports the network's
    /// derived `baseline.virtual_occupied` as `virtualUsedCapacity`.
    #[test]
    fn test_cell_info_to_response_genesis_satoshi_burn_tagged() {
        use ckbadger_common::dao::SATOSHI_PUBKEY_HASH;
        let payload = LiveCellInfo {
            lock_args: SATOSHI_PUBKEY_HASH.to_vec(),
            ..make_payload()
        };
        // created_at_block == 0 -> genesis output
        let info = ckbadger_store::PositionedCellInfo::new(payload, 0);
        let tx_hash = vec![5u8; 32];
        let resp =
            cell_info_to_response(&tx_hash, 0, &info, "mainnet", VIRTUAL_OCCUPIED_MAINNET).unwrap();
        assert_eq!(resp.cell_type.as_deref(), Some("genesis_special_burn"));
        assert_eq!(
            resp.virtual_used_capacity.as_deref(),
            Some("504000000000000000")
        );
    }

    /// A Satoshi-args output that is NOT at genesis (block > 0) is not tagged;
    /// nor is a genesis output with non-Satoshi lock args.
    #[test]
    fn test_cell_info_to_response_non_genesis_or_non_satoshi_not_tagged() {
        use ckbadger_common::dao::SATOSHI_PUBKEY_HASH;
        // Satoshi args but block 100 -> not a genesis burn cell.
        let satoshi_non_genesis = ckbadger_store::PositionedCellInfo::new(
            LiveCellInfo {
                lock_args: SATOSHI_PUBKEY_HASH.to_vec(),
                ..make_payload()
            },
            100,
        );
        let tx_hash = vec![6u8; 32];
        let resp = cell_info_to_response(
            &tx_hash,
            0,
            &satoshi_non_genesis,
            "mainnet",
            VIRTUAL_OCCUPIED_MAINNET,
        )
        .unwrap();
        assert!(resp.cell_type.is_none());
        assert!(resp.virtual_used_capacity.is_none());

        // Genesis (block 0) but non-Satoshi args -> not tagged.
        let genesis_non_satoshi = ckbadger_store::PositionedCellInfo::new(make_payload(), 0);
        let resp = cell_info_to_response(
            &tx_hash,
            0,
            &genesis_non_satoshi,
            "mainnet",
            VIRTUAL_OCCUPIED_MAINNET,
        )
        .unwrap();
        assert!(resp.cell_type.is_none());
        assert!(resp.virtual_used_capacity.is_none());
    }

    #[test]
    fn test_estimated_occupied_capacity_breakdown_without_type_script() {
        let info = LiveCellInfo {
            data_size: 16,
            ..make_payload()
        };
        let info = positioned(info);

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
            ..make_payload()
        };
        let info = positioned(info);

        let breakdown = estimated_occupied_capacity_breakdown(&info);
        assert_eq!(breakdown.capacity_field_bytes, 8);
        assert_eq!(breakdown.lock_script_bytes, 53);
        assert_eq!(breakdown.type_script_bytes, 57);
        assert_eq!(breakdown.data_bytes, 16);
        assert_eq!(breakdown.total_bytes, 134);
    }

    #[test]
    fn token_decode_formats_decimals_without_rounding_or_guessing() {
        let mut analysis = CellDataAnalysis {
            token: None,
            heuristic_guesses: vec![],
            deterministic: Some(CellDeterministicDecode {
                kind: "udt_amount".into(),
                summary: String::new(),
                segments: vec![CellDataSegment {
                    label: "amount".into(),
                    start: 0,
                    end: 16,
                    meaning: String::new(),
                    human_value: "123456789012345678901234567890".into(),
                }],
            }),
        };
        let token = ckbadger_store::TokenInfo {
            type_code_hash: vec![0; 32],
            hash_type: 1,
            type_args: vec![],
            standard: "xudt".into(),
            name: Some("USDI".into()),
            symbol: Some("USDI".into()),
            decimals: Some(6),
            max_supply: None,
            first_seen_block: 1,
            icon_url: None,
            description: None,
            transfers_count: 0,
        };
        enrich_token_decode(&mut analysis, &token).unwrap();
        assert_eq!(
            analysis.token.as_ref().unwrap().formatted_amount.as_deref(),
            Some("123456789012345678901234.567890")
        );
        assert_eq!(
            analysis.deterministic.unwrap().segments[0].human_value,
            "123456789012345678901234.567890 USDI"
        );
    }

    #[test]
    fn redesign_usdi_uses_indexer_token_classification_without_heuristics() {
        let info = positioned(LiveCellInfo {
            type_code_hash: Some(
                hex::decode("cc9dc33ef234e14bc788c43a4848556a5fb16401a04662fc55db9bb201987037")
                    .unwrap(),
            ),
            type_script_hash: Some(vec![0x11; 32]),
            data_size: 16,
            ..make_payload()
        });
        let analysis = analyze_cell_data(&info, &300_000_000u128.to_le_bytes(), 16).unwrap();
        assert_eq!(analysis.deterministic.unwrap().kind, "udt_amount");
        assert!(analysis.heuristic_guesses.is_empty());
    }

    #[test]
    fn test_analyze_cell_data_detects_udt_amount_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(SUDT_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x11; 32]),
            data_size: 16,
            ..make_payload()
        };
        let info = positioned(info);
        let mut data = vec![0u8; 16];
        data[0] = 0x2a;

        let analysis = analyze_cell_data(&info, &data, 16).expect("cell data analysis");
        let deterministic = analysis.deterministic.expect("deterministic decode");
        assert_eq!(deterministic.kind, "udt_amount");
        assert_eq!(deterministic.segments.len(), 1);
        assert_eq!(deterministic.segments[0].label, "amount");
        assert_eq!(deterministic.segments[0].start, 0);
        assert_eq!(deterministic.segments[0].end, 16);
        assert_eq!(deterministic.segments[0].human_value, "42");
    }

    // Testnet sUDT type-script code_hash. Distinct from the mainnet SUDT_CODE_HASH,
    // it is only classifiable because detection now flows through the shared
    // network-agnostic PROTOCOL_REGISTRY instead of mainnet-only local consts.
    const TESTNET_SUDT_CODE_HASH: &str =
        "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4";

    #[test]
    fn test_analyze_cell_data_detects_testnet_udt_amount_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(TESTNET_SUDT_CODE_HASH.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x11; 32]),
            data_size: 16,
            ..make_payload()
        };
        let info = positioned(info);
        let mut data = vec![0u8; 16];
        data[0] = 0x2a;

        let analysis = analyze_cell_data(&info, &data, 16).expect("cell data analysis");
        let deterministic = analysis
            .deterministic
            .expect("testnet sUDT must classify as UDT via the registry");
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
            ..make_payload()
        };
        let info = positioned(info);
        let cluster_id = vec![0xAA; 32];
        let data = make_spore_data("image/png", &[1, 2, 3, 4], Some(cluster_id.as_slice()));

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
        let mut end = 0;
        for segment in &deterministic.segments {
            assert_eq!(
                segment.start, end,
                "Spore bytes must be completely partitioned"
            );
            end = segment.end;
        }
        assert_eq!(end as usize, data.len());
        for label in ["content_type_length", "content_length", "cluster_id_length"] {
            assert!(deterministic
                .segments
                .iter()
                .any(|segment| segment.label == label));
        }
    }

    #[test]
    fn test_bit_cell_is_not_classified_as_spore() {
        for code_hash in [
            "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33",
            "0x0b1f412fbae26853ff7d082d422c2bdd9e2ff94ee8aaec11240a5b34cc6e890f",
        ] {
            let bytes = hex::decode(code_hash.trim_start_matches("0x")).unwrap();
            assert!(!is_spore_type_code_hash(&bytes));
        }
    }

    #[test]
    fn test_analyze_cell_data_detects_spore_cluster_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(CLUSTER_CODE_HASHES[0].trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x19; 32]),
            ..make_payload()
        };
        let info = positioned(info);
        let data = make_cluster_data("Genesis Collection", "Primary cluster");

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);

        let info_blob = br#"{"name":"Issuer-01","info":"demo"}"#;
        let mut data = Vec::new();
        data.push(1); // version
        data.extend_from_slice(&12u32.to_be_bytes()); // class_count
        data.extend_from_slice(&3u32.to_be_bytes()); // set_count
        data.extend_from_slice(&(info_blob.len() as u16).to_be_bytes());
        data.extend_from_slice(info_blob);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let mut data = Vec::new();
        data.push(1); // version
        data.extend_from_slice(&100u32.to_be_bytes()); // total
        data.extend_from_slice(&7u32.to_be_bytes()); // issued
        data.push(0x0f); // configure
        data.extend_from_slice(&make_mnft_vartext("Class-A"));
        data.extend_from_slice(&make_mnft_vartext("Main collection"));
        data.extend_from_slice(&make_mnft_vartext("renderer:v1"));

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);

        // version(1) + total(4 BE) + issued(4 BE) + configure(1)
        // + name_len(2 BE) + name + desc_len(2 BE) + desc + renderer_len(2 BE)
        let data = hex::decode("000000001400000014c0000a466972737420537465700004646573630000")
            .expect("valid hex");

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let mut data = Vec::new();
        data.push(2); // version
        data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd]); // characteristic
        data.push(0x81); // configure
        data.push(0x04); // state

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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

    // Testnet mNFT token code_hash (m-nft.toml `[testnet]`). Proves the registry-backed
    // `is_mnft_token_type_code_hash` classifies testnet cells — the mainnet-only local
    // const could not, which is exactly the testnet gap this migration closes.
    const TESTNET_MNFT_TOKEN_CODE_HASH: &str =
        "0xb1837b5ad01a88558731953062d1f5cb547adf89ece01e8934a9f0aeed2d959f";

    #[test]
    fn test_analyze_cell_data_detects_testnet_mnft_token_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(
                hex::decode(TESTNET_MNFT_TOKEN_CODE_HASH.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x24; 32]),
            ..make_payload()
        };
        let info = positioned(info);
        let mut data = Vec::new();
        data.push(3); // version
        data.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33]); // characteristic
        data.push(0x00); // configure
        data.push(0x00); // state

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        let deterministic = analysis
            .deterministic
            .expect("testnet mNFT token must classify via the registry");
        assert_eq!(deterministic.kind, "mnft_token_cell");
        assert!(deterministic
            .segments
            .iter()
            .any(|s| s.label == "characteristic" && s.human_value == "0xdeadbeef00112233"));
    }

    #[test]
    fn test_analyze_cell_data_detects_dao_deposit_segments() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(DAO_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x33; 32]),
            ..make_payload()
        };
        let info = positioned(info);
        let data = vec![0u8; 8];

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let block_number = 987654u64;
        let data = block_number.to_le_bytes().to_vec();

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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

        let analysis = analyze_cell_data(&info, &data, 40).expect("cell data analysis");
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

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("application/wasm")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_zip_guess() {
        let info = make_info();
        let data = vec![0x50, 0x4B, 0x03, 0x04, 0x14, 0x00];

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("application/zip")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_gzip_guess() {
        let info = make_info();
        let data = vec![0x1F, 0x8B, 0x08, 0x00];

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let data = make_spore_data("text/plain", b"hello spore text", None);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("image/svg+xml")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_uri_guess() {
        let info = make_info();
        let data = b"ipfs://bafybeigdyrztm".to_vec();

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.mime_type.as_deref() == Some("text/uri-list")));
    }

    #[test]
    fn test_analyze_cell_data_builds_heuristic_u32_u64_zero_guesses() {
        let info = make_info();

        let u32_data = 12345u32.to_le_bytes().to_vec();
        let u32_analysis =
            analyze_cell_data(&info, &u32_data, u32_data.len() as i32).expect("cell data analysis");
        assert!(u32_analysis
            .heuristic_guesses
            .iter()
            .any(|g| g.kind == "numeric_pattern" && g.human_value.as_deref() == Some("12345")));

        let u64_data = 0u64.to_le_bytes().to_vec();
        let u64_analysis =
            analyze_cell_data(&info, &u64_data, u64_data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let data = vec![0u8; 4];

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let data = vec![0u8; 20];

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis.deterministic.is_none());
        assert!(analysis.heuristic_guesses.iter().any(|g| {
            g.kind == "script_hint"
                && g.reason.contains("dotbit")
                && g.human_value
                    .as_deref()
                    .is_some_and(|v| v.contains("at least 52 bytes"))
        }));
    }

    /// A `.bit` AccountCell shaped like mainnet `0xc51411d9…a33c:0`:
    /// account_hash(32) + account_id(20) + next_account_id(20) +
    /// expired_at(u64 LE) + the account name itself.
    fn dotbit_account_cell(tail: &[u8]) -> (ckbadger_store::PositionedCellInfo, Vec<u8>) {
        let info = positioned(LiveCellInfo {
            type_code_hash: Some(
                hex::decode(DOTBIT_ACCOUNT_CELL_TYPE_ID.trim_start_matches("0x")).unwrap(),
            ),
            type_script_hash: Some(vec![0x54; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            ..make_payload()
        });
        let mut data = Vec::new();
        data.extend_from_slice(&[0x11; 32]);
        data.extend_from_slice(&[0x22; 20]);
        data.extend_from_slice(&[0x33; 20]);
        data.extend_from_slice(&1_800_000_000u64.to_le_bytes());
        data.extend_from_slice(tail);
        (info, data)
    }

    #[test]
    fn test_analyze_cell_data_dotbit_emits_account_segment() {
        let (info, data) = dotbit_account_cell(b"casinox.bit");
        assert_eq!(data.len(), 91);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        let decode = analysis.deterministic.expect("dotbit decode");
        assert_eq!(decode.kind, "dotbit_account");
        let account = decode
            .segments
            .iter()
            .find(|s| s.label == "account")
            .unwrap_or_else(|| panic!("account segment missing: {decode:?}"));
        assert_eq!((account.start, account.end), (80, 91));
        assert_eq!(account.human_value, "casinox.bit");
        assert!(
            decode
                .segments
                .iter()
                .all(|s| s.label != "trailing_payload"),
            "the tail is the account name, not an opaque payload: {decode:?}"
        );
    }

    #[test]
    fn test_analyze_cell_data_dotbit_non_utf8_tail_is_an_error() {
        let (info, data) = dotbit_account_cell(&[0xff, 0xfe]);

        let err = analyze_cell_data(&info, &data, data.len() as i32)
            .expect_err("an AccountCell whose name is not UTF-8 is corrupt data, not a payload");
        let text = format!("{err:#}");
        assert!(text.contains("dotbit account name is not UTF-8"), "{text}");
    }

    #[test]
    fn test_analyze_cell_data_builds_script_hint_for_short_udt_payload() {
        let info = LiveCellInfo {
            type_code_hash: Some(hex::decode(SUDT_CODE_HASH.trim_start_matches("0x")).unwrap()),
            type_script_hash: Some(vec![0x53; 32]),
            ..make_payload()
        };
        let info = positioned(info);
        let data = vec![0u8; 8];

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
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
            ..make_payload()
        };
        let info = positioned(info);
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[0xAB; 32]);
        data.extend_from_slice(&3u32.to_le_bytes());

        let analysis = analyze_cell_data(&info, &data, 40).expect("cell data analysis");
        assert!(analysis.deterministic.is_none());
    }

    // ── `.cell` (DotCell) fixtures ───────────────────────────────────────
    // Copied from `crates/indexer/src/parser/dotcell_fixtures.rs` (node-verified
    // chain data, 2026-09-24); that module is `#[cfg(test)]` in the indexer
    // crate and not reachable from here.

    /// `ACCOUNT_TYPE_CODE_HASH_MAINNET` (docs/metadata/scripts/dotcell-account.toml).
    const DOTCELL_ACCOUNT_TYPE_MAINNET: &str =
        "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54";
    /// `ACCOUNT_LOCK_CODE_HASH_MAINNET` (docs/metadata/scripts/dotcell-account-lock.toml).
    const DOTCELL_ACCOUNT_LOCK_MAINNET: &str =
        "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab";

    fn hex_bytes(value: &str) -> Vec<u8> {
        hex::decode(value.trim_start_matches("0x")).unwrap()
    }

    #[test]
    fn test_cell_protocol_script_uses_registry_slugs() {
        let name_cell = LiveCellInfo {
            lock_code_hash: hex_bytes(DOTCELL_ACCOUNT_LOCK_MAINNET),
            lock_args: vec![],
            type_code_hash: Some(hex_bytes(DOTCELL_ACCOUNT_TYPE_MAINNET)),
            type_script_hash: Some(vec![0x61; 32]),
            type_hash_type: Some(1),
            type_args: Some(hex_bytes("0xb4f4302965b7d6421481a520ee7eb5971a5e808c")),
            ..make_payload()
        };
        let protocol = cell_protocol_script(&name_cell);
        assert_eq!(protocol.lock.as_deref(), Some("dotcell-account-lock"));
        assert_eq!(protocol.type_script.as_deref(), Some("dotcell-account"));

        // A plain lock with no type script names no protocol on either side;
        // `{lock: null, type: null}` is itself an answer, not an absence.
        let plain = make_payload();
        let protocol = cell_protocol_script(&plain);
        assert_eq!(protocol.lock, None);
        assert_eq!(protocol.type_script, None);
        let json = serde_json::to_value(&protocol).unwrap();
        assert_eq!(json, serde_json::json!({ "lock": null, "type": null }));
    }

    /// `M2_OUT1_DATA`: `support.cell` as registered on mainnet (105 bytes).
    const DOTCELL_SUPPORT_DATA: &str = "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e265b5fe7e7070b506f69bd8cabf9e4272111066455e00926c0057d926a44d83fc13b21ce037b1e31f4223e3c86757d926a44d83fc13b21ce037b1e31f4223e3c867737570706f7274";
    /// `M1_OUT0_DATA`: the mainnet ring root (98 bytes, empty label).
    const DOTCELL_RING_ROOT_DATA: &str = "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
    /// `PRICE_TYPE_CODE_HASH_MAINNET` (docs/metadata/scripts/dotcell-price.toml).
    const DOTCELL_PRICE_TYPE_MAINNET: &str =
        "0x97bf5f760cf72f918f13704d7184933b79d4ddc1fd85075762373e531152d4f9";
    /// `SALE_LOCK_CODE_HASH_MAINNET` (docs/metadata/scripts/dotcell-sale-lock.toml).
    const DOTCELL_SALE_LOCK_MAINNET: &str =
        "0x086c8f4e9d4272e3dfbaca399792f730e6604591e87931ee6d67047a3c900879";

    /// A mainnet `.cell` name cell: Account Lock (empty args) + Account type
    /// under the mainnet namespace.
    fn dotcell_name_info(data: &[u8]) -> ckbadger_store::PositionedCellInfo {
        positioned(LiveCellInfo {
            lock_code_hash: hex_bytes(DOTCELL_ACCOUNT_LOCK_MAINNET),
            lock_args: vec![],
            type_code_hash: Some(hex_bytes(DOTCELL_ACCOUNT_TYPE_MAINNET)),
            type_script_hash: Some(vec![0x61; 32]),
            type_hash_type: Some(1),
            type_args: Some(hex_bytes("0xb4f4302965b7d6421481a520ee7eb5971a5e808c")),
            data_size: data.len() as i32,
            ..make_payload()
        })
    }

    fn dotcell_segment<'a>(
        decode: &'a CellDeterministicDecode,
        label: &str,
    ) -> &'a CellDataSegment {
        decode
            .segments
            .iter()
            .find(|s| s.label == label)
            .unwrap_or_else(|| panic!("segment {label} missing from {decode:?}"))
    }

    #[test]
    fn test_analyze_cell_data_decodes_dotcell_name() {
        let data = hex_bytes(DOTCELL_SUPPORT_DATA);
        let info = dotcell_name_info(&data);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        let decode = analysis.deterministic.expect("dotcell name decode");
        assert_eq!(decode.kind, "dotcell_name");
        assert!(
            decode.summary.contains("support.cell"),
            "{}",
            decode.summary
        );

        let shape: Vec<(&str, i32, i32)> = decode
            .segments
            .iter()
            .map(|s| (s.label.as_str(), s.start, s.end))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("layout_version", 0, 1),
                ("records_hash", 1, 33),
                ("next_id", 33, 53),
                ("expired_at", 53, 58),
                ("owner_hash20", 58, 78),
                ("manager_hash20", 78, 98),
                ("label", 98, 105),
            ]
        );
        assert_eq!(decode.segments[0].human_value, "3");
        assert_eq!(
            decode.segments[1].human_value,
            "0x72ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2"
        );
        assert_eq!(
            decode.segments[2].human_value,
            "0x65b5fe7e7070b506f69bd8cabf9e427211106645"
        );
        // 1,821,507,678 = 2027-09-21T06:21:18Z.
        assert!(
            decode.segments[3].human_value.contains("2027-09-21"),
            "{}",
            decode.segments[3].human_value
        );
        assert!(
            decode.segments[3].human_value.contains("1821507678"),
            "{}",
            decode.segments[3].human_value
        );
        assert!(
            decode.segments[3].meaning.contains("u40"),
            "{}",
            decode.segments[3].meaning
        );
        assert_eq!(
            decode.segments[4].human_value,
            "0x57d926a44d83fc13b21ce037b1e31f4223e3c867"
        );
        assert!(
            decode.segments[5].human_value.contains("same as owner"),
            "{}",
            decode.segments[5].human_value
        );
        assert_eq!(
            decode.segments[6].human_value,
            "support.cell · id 0x62d71147ac82b83c8531126cacb0d2f072bfd94a"
        );
    }

    #[test]
    fn test_analyze_cell_data_decodes_dotcell_sub_name_parent() {
        let mut data = hex_bytes(DOTCELL_SUPPORT_DATA);
        data.truncate(98);
        data.extend_from_slice(b"shop.support");
        let info = dotcell_name_info(&data);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        let decode = analysis.deterministic.expect("dotcell sub-name decode");
        assert_eq!(decode.kind, "dotcell_name");
        let label = dotcell_segment(&decode, "label");
        assert_eq!((label.start, label.end), (98, 110));
        assert!(
            label.human_value.starts_with("shop.support.cell · id 0x"),
            "{}",
            label.human_value
        );
        assert!(
            label.human_value.contains("sub-name of support.cell"),
            "{}",
            label.human_value
        );
    }

    #[test]
    fn test_analyze_cell_data_decodes_dotcell_ring_root() {
        let data = hex_bytes(DOTCELL_RING_ROOT_DATA);
        let info = dotcell_name_info(&data);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        let decode = analysis.deterministic.expect("dotcell ring root decode");
        assert_eq!(decode.kind, "dotcell_ring_root");
        assert!(decode.summary.contains("ring root"), "{}", decode.summary);
        assert_eq!(
            dotcell_segment(&decode, "next_id").human_value,
            "end of ring"
        );
        let label = dotcell_segment(&decode, "label");
        assert_eq!((label.start, label.end), (98, 98));
        assert_eq!(label.human_value, "(empty)");
    }

    #[test]
    fn test_analyze_cell_data_decodes_dotcell_manager_delegated() {
        let mut data = hex_bytes(DOTCELL_SUPPORT_DATA);
        data[78..98].copy_from_slice(&[0xab; 20]);
        let info = dotcell_name_info(&data);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        let decode = analysis.deterministic.expect("dotcell name decode");
        let manager = dotcell_segment(&decode, "manager_hash20");
        assert!(
            manager.human_value.contains("delegated"),
            "{}",
            manager.human_value
        );
        assert!(
            manager.human_value.contains(&"ab".repeat(20)),
            "{}",
            manager.human_value
        );
    }

    #[test]
    fn test_analyze_cell_data_dotcell_undecodable_is_an_error() {
        let mut data = hex_bytes(DOTCELL_SUPPORT_DATA);
        data.truncate(97);
        let info = dotcell_name_info(&data);

        let err = analyze_cell_data(&info, &data, data.len() as i32)
            .expect_err("an Account-typed cell whose data does not decode is an invariant break");
        let text = format!("{err:#}");
        assert!(text.contains("dotcell"), "{text}");
        assert!(text.contains("need>=98"), "{text}");
    }

    #[test]
    fn test_analyze_cell_data_dotcell_exact_decode_has_no_guesses() {
        let data = hex_bytes(DOTCELL_SUPPORT_DATA);
        let info = dotcell_name_info(&data);

        let analysis =
            analyze_cell_data(&info, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis.deterministic.is_some());
        assert!(analysis.heuristic_guesses.is_empty());
        assert!(
            analysis
                .heuristic_guesses
                .iter()
                .all(|g| g.kind != "script_hint"),
            "a decoded cell gets no script hint: {:?}",
            analysis.heuristic_guesses
        );
    }

    #[test]
    fn test_analyze_cell_data_builds_script_hint_for_dotcell_family() {
        // A Price-typed cell this page does not decode says what it is.
        let price = positioned(LiveCellInfo {
            type_code_hash: Some(hex_bytes(DOTCELL_PRICE_TYPE_MAINNET)),
            type_script_hash: Some(vec![0x62; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            ..make_payload()
        });
        let data = vec![0x01, 0x9f, 0x24];
        let analysis =
            analyze_cell_data(&price, &data, data.len() as i32).expect("cell data analysis");
        assert!(analysis.deterministic.is_none());
        assert_eq!(analysis.heuristic_guesses[0].kind, "script_hint");
        assert!(
            analysis.heuristic_guesses[0].reason.contains("Cells Price"),
            "{:?}",
            analysis.heuristic_guesses[0]
        );

        // No type script at all: the hint comes from the lock.
        let sale = positioned(LiveCellInfo {
            lock_code_hash: hex_bytes(DOTCELL_SALE_LOCK_MAINNET),
            lock_args: vec![0x11; 40],
            ..make_payload()
        });
        let analysis = analyze_cell_data(&sale, &[], 0).expect("cell data analysis");
        assert!(analysis.deterministic.is_none());
        assert_eq!(analysis.heuristic_guesses[0].kind, "script_hint");
        assert!(
            analysis.heuristic_guesses[0]
                .reason
                .contains("Cells Sale Lock"),
            "{:?}",
            analysis.heuristic_guesses[0]
        );
    }

    #[test]
    fn test_is_known_script_label() {
        assert!(!is_known_script_name(Some("unknown")));
        assert!(!is_known_script_name(Some(" Unknown ")));
        assert!(!is_known_script_name(Some(" ")));
        assert!(is_known_script_name(Some("Secp256k1")));
    }

    #[test]
    fn test_script_labels_from_semantic_tags() {
        use ckbadger_store::types::semantic_tags as st;

        // PLAIN (0) returns empty labels.
        assert!(script_labels_from_semantic_tags(st::PLAIN).is_empty());

        // Single bit.
        assert_eq!(script_labels_from_semantic_tags(st::DAO), vec!["NervosDAO"]);
        assert_eq!(script_labels_from_semantic_tags(st::SPORE), vec!["Spore"]);

        // Multiple bits.
        let labels = script_labels_from_semantic_tags(st::DAO | st::XUDT | st::CLUSTER);
        assert_eq!(labels, vec!["NervosDAO", "xUDT", "Spore Cluster"]);

        // did:ckb is an independent identity protocol with its own tag/label.
        assert_eq!(
            script_labels_from_semantic_tags(st::DID_CKB),
            vec!["did:ckb"]
        );
        assert_ne!(st::DID_CKB, st::SPORE);
        assert_ne!(st::DID_CKB, st::BIT_CELL);

        // All bits set.
        let all = st::DAO
            | st::SUDT
            | st::XUDT
            | st::DOTBIT
            | st::MNFT
            | st::SPORE
            | st::CLUSTER
            | st::DID_CKB;
        let labels = script_labels_from_semantic_tags(all);
        assert_eq!(labels.len(), 8);
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
            tx_hash: [0u8; 32],
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        };
        let mut domain_batch = StoreBatch::new(&domain);
        domain_batch.put_addr_tx(
            &lock_hash,
            30,
            0,
            &stale_tx,
            &ckbadger_store::types::AddrTxValue::new(0, false, true, 0),
        );
        domain_batch.put_addr_tx(
            &lock_hash,
            20,
            0,
            &canonical_tx_new,
            &ckbadger_store::types::AddrTxValue::new(0, false, true, 0),
        );
        domain_batch.put_addr_tx(
            &lock_hash,
            10,
            0,
            &canonical_tx_old,
            &ckbadger_store::types::AddrTxValue::new(0, false, true, 0),
        );
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
        // AddrTxValue is propagated through from the store.
        assert_eq!(rows[0].3.tx_type_str(), "received");
        assert_eq!(rows[1].3.tx_type_str(), "received");
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
    fn active_address_window_uses_effective_chain_time_not_assumed_block_rate() {
        let root = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let mut batch = StoreBatch::new(&store);
        for (number, chain_time_ms) in [(0, 0), (1, 86_400_000), (2, 172_800_000)] {
            batch.put_block_header(
                number,
                &ckbadger_store::types::CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0; 32],
                    timestamp: chain_time_ms,
                    chain_time_ms,
                    epoch_number: 0,
                    epoch_index: 0,
                    epoch_length: 1,
                    dao: vec![0; 32],
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                },
            );
        }
        batch.commit().unwrap();
        assert_eq!(active_addresses_min_block(&store, 2, 1).unwrap(), 2);
    }

    #[test]
    fn test_parse_address_token_cursor_valid() {
        let cursor = format!("200:{}", "bb".repeat(32));
        let (balance, type_hash) = super::parse_address_token_cursor(&cursor).unwrap();
        assert_eq!(balance, 200);
        assert_eq!(type_hash, vec![0xBB; 32]);
    }

    #[test]
    fn test_parse_address_token_cursor_accepts_balance_above_u128() {
        let cursor = format!(
            "531691198313966349161522824112137830400:{}",
            "bb".repeat(32)
        );
        let (balance, _) = super::parse_address_token_cursor(&cursor).unwrap();
        assert_eq!(
            balance.to_string(),
            "531691198313966349161522824112137830400"
        );
    }

    #[test]
    fn test_parse_address_token_cursor_rejects_wrong_length_hash() {
        assert!(super::parse_address_token_cursor("200:aabbcc").is_err());
    }
}
