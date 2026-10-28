//! Bounded, read-only canonical cells for the error-page artwork.
//! The window is a sample, never a claim to enumerate all live cells.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use ckbadger_store::PositionedCellInfo;
use serde::{Deserialize, Serialize};

use super::cells::{estimated_occupied_capacity_breakdown, OccupiedCapacityBreakdown};
use crate::{
    response::{ok, ApiError, ApiResult},
    utils::parse_hash32,
    AppState,
};

const BLOCK_WINDOW: i64 = 12;
const TXS_PER_BLOCK: i32 = 6;
const OUTPUTS_PER_TX: i16 = 4;
const MAX_VISIBLE: usize = 64;
const MAX_TRACKED: usize = 64;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/vessels/snapshot", get(snapshot))
}

#[derive(Debug, Deserialize)]
struct Params {
    tracked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VesselCell {
    outpoint: String,
    capacity_shannons: String,
    occupied_shannons: String,
    anatomy: OccupiedCapacityBreakdown,
    created_at_block: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackedFate {
    outpoint: String,
    status: &'static str,
    consumed_at_block: Option<i64>,
    consuming_tx_hash: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    anchor_block: i64,
    anchor_hash: String,
    first_sampled_block: i64,
    blocks_in_window: i64,
    transactions_per_block: i32,
    outputs_per_transaction: i16,
    max_visible: usize,
    examined_outputs: usize,
    cells: Vec<VesselCell>,
    tracked: Vec<TrackedFate>,
}

fn outpoint(tx_hash: &[u8], index: i16) -> String {
    format!("0x{}-{index}", hex::encode(tx_hash))
}

fn parse_tracked(raw: Option<&str>) -> Result<Vec<(Vec<u8>, i16)>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<_> = raw.split(',').collect();
    if parts.len() > MAX_TRACKED {
        return Err(format!(
            "tracked vessel count {} exceeds {MAX_TRACKED}",
            parts.len()
        ));
    }
    let mut result = Vec::with_capacity(parts.len());
    let mut seen = std::collections::HashSet::new();
    for part in parts {
        let (hash, index) = part
            .rsplit_once('-')
            .ok_or_else(|| format!("invalid tracked outpoint {part}"))?;
        let hash = parse_hash32(hash, "tracked outpoint hash")
            .map_err(|_| format!("invalid tracked outpoint hash {part}"))?;
        let index: i16 = index
            .parse()
            .map_err(|_| format!("invalid tracked output index {part}"))?;
        if index < 0 || !seen.insert((hash.clone(), index)) {
            return Err(format!("invalid or duplicate tracked outpoint {part}"));
        }
        result.push((hash, index));
    }
    Ok(result)
}

fn exact_cell(cell: &PositionedCellInfo, tx_hash: &[u8], index: i16) -> anyhow::Result<VesselCell> {
    let has_type = cell.type_code_hash.is_some();
    anyhow::ensure!(
        cell.data_size >= 0,
        "invalid vessel cell source sizes: outpoint={}, data_size={}",
        outpoint(tx_hash, index),
        cell.data_size
    );
    anyhow::ensure!(
        cell.type_script_hash.is_some() == has_type
            && cell.type_hash_type.is_some() == has_type
            && cell.type_args.is_some() == has_type,
        "incomplete vessel type script evidence: outpoint={}",
        outpoint(tx_hash, index)
    );
    let anatomy = estimated_occupied_capacity_breakdown(cell);
    let expected = anatomy
        .total_bytes
        .checked_mul(100_000_000)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "vessel occupied capacity overflow: {}",
                outpoint(tx_hash, index)
            )
        })?;
    anyhow::ensure!(anatomy.total_bytes > 0 && cell.occupied_capacity == expected && cell.capacity >= expected,
        "invalid vessel cell anatomy: outpoint={}, indexed_occupied={}, computed_occupied={}, capacity={}",
        outpoint(tx_hash, index), cell.occupied_capacity, expected, cell.capacity);
    Ok(VesselCell {
        outpoint: outpoint(tx_hash, index),
        capacity_shannons: cell.capacity.to_string(),
        occupied_shannons: cell.occupied_capacity.to_string(),
        anatomy,
        created_at_block: cell.created_at_block,
    })
}

async fn snapshot(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Params>,
) -> ApiResult<Snapshot> {
    let tracked = parse_tracked(params.tracked.as_deref()).map_err(ApiError::bad_request)?;
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Snapshot> {
        // Keep the same secondary view even if the awaiting HTTP task is cancelled.
        let _view = ckbadger_store::read_view::acquire_read();
        let store = &state.store;
        let cells_store = &state.append_only_store;
        let (anchor_block, anchor) = store.get_sync_tip_block()?
            .ok_or_else(|| anyhow::anyhow!("canonical block tip is not indexed"))?;
        anyhow::ensure!(anchor_block >= 0, "negative canonical vessel anchor block {anchor_block}");
        anyhow::ensure!(anchor.hash.len() == 32, "invalid canonical anchor hash at block {anchor_block}");
        let first_sampled_block = (anchor_block - BLOCK_WINDOW + 1).max(0);
        let mut cells = Vec::new();
        let mut examined_outputs = 0;
        for block in (first_sampled_block..=anchor_block).rev() {
            let header = store.get_block_header(block)?
                .ok_or_else(|| anyhow::anyhow!("vessel sample block {block} missing canonical header"))?;
            anyhow::ensure!(header.transactions_count >= 0, "vessel sample block {block} has negative transaction count");
            for tx_index in 0..header.transactions_count.min(TXS_PER_BLOCK) {
                let tx = store.get_tx_index(block, tx_index)?
                    .ok_or_else(|| anyhow::anyhow!("vessel sample missing canonical transaction: block={block}, index={tx_index}"))?;
                let (location_block, location_index, _) = store.get_tx_by_hash(&tx.tx_hash)?
                    .ok_or_else(|| anyhow::anyhow!("vessel sample transaction lacks canonical hash mapping: block={block}, index={tx_index}, hash=0x{}", hex::encode(tx.tx_hash)))?;
                anyhow::ensure!(location_block == block && location_index == tx_index,
                    "vessel sample transaction location mismatch: block={block}, index={tx_index}, hash=0x{}, mapped=({location_block},{location_index})",
                    hex::encode(tx.tx_hash));
                anyhow::ensure!(tx.outputs_count >= 0, "vessel sample negative output count: block={block}, index={tx_index}");
                for output_index in 0..tx.outputs_count.min(OUTPUTS_PER_TX) {
                    examined_outputs += 1;
                    let live = store.get_cell(&tx.tx_hash, output_index, cells_store)?;
                    let consumed = store.get_consumed_cell_info(&tx.tx_hash, output_index, cells_store)?;
                    anyhow::ensure!(live.is_some() != consumed.is_some(),
                        "vessel sample output lacks exactly one canonical state marker: outpoint={}, block={block}, tx_index={tx_index}",
                        outpoint(&tx.tx_hash, output_index));
                    if let Some(cell) = live {
                        anyhow::ensure!(cell.created_at_block == block,
                            "vessel sample creation block mismatch: outpoint={}, indexed={}, transaction_block={block}",
                            outpoint(&tx.tx_hash, output_index), cell.created_at_block);
                        if cells.len() < MAX_VISIBLE {
                            cells.push(exact_cell(&cell, &tx.tx_hash, output_index)?);
                        }
                    } else if let Some(consumed) = consumed {
                        anyhow::ensure!(consumed.created_at_block == block,
                            "vessel sample consumed cell creation block mismatch: outpoint={}, indexed={}, transaction_block={block}",
                            outpoint(&tx.tx_hash, output_index), consumed.created_at_block);
                        anyhow::ensure!(consumed.consumed_at_block >= block && consumed.consumed_at_block <= anchor_block && consumed.consumed_by_tx.as_ref().is_some_and(|hash| hash.len() == 32),
                            "vessel sample invalid consumer evidence: outpoint={}, consumed_at_block={}, anchor={anchor_block}",
                            outpoint(&tx.tx_hash, output_index), consumed.consumed_at_block);
                    }
                }
            }
        }
        let mut tracked_fates = Vec::with_capacity(tracked.len());
        for (tx_hash, index) in tracked {
            let live = store.get_cell(&tx_hash, index, cells_store)?;
            let consumed = store.get_consumed_cell_info(&tx_hash, index, cells_store)?;
            anyhow::ensure!(!(live.is_some() && consumed.is_some()),
                "vessel tracked outpoint has both live and consumed markers: {}", outpoint(&tx_hash, index));
            let (status, consumed_at_block, consuming_tx_hash) = if live.is_some() {
                ("live", None, None)
            } else if let Some(consumed) = consumed {
                anyhow::ensure!(consumed.consumed_at_block >= consumed.created_at_block && consumed.consumed_at_block <= anchor_block,
                    "vessel tracked outpoint has invalid consumption block: {}", outpoint(&tx_hash, index));
                let consumer = consumed.consumed_by_tx.ok_or_else(|| anyhow::anyhow!(
                    "vessel tracked outpoint lacks consuming transaction: {}", outpoint(&tx_hash, index)))?;
                anyhow::ensure!(consumer.len() == 32, "vessel tracked outpoint has invalid consuming hash: {}", outpoint(&tx_hash, index));
                ("consumed", Some(consumed.consumed_at_block), Some(format!("0x{}", hex::encode(consumer))))
            } else {
                ("notCanonical", None, None)
            };
            tracked_fates.push(TrackedFate { outpoint: outpoint(&tx_hash, index), status, consumed_at_block, consuming_tx_hash });
        }
        Ok(Snapshot {
            anchor_block,
            anchor_hash: format!("0x{}", hex::encode(anchor.hash)),
            first_sampled_block,
            blocks_in_window: anchor_block - first_sampled_block + 1,
            transactions_per_block: TXS_PER_BLOCK,
            outputs_per_transaction: OUTPUTS_PER_TX,
            max_visible: MAX_VISIBLE,
            examined_outputs,
            cells,
            tracked: tracked_fates,
        })
    }).await.map_err(|error| ApiError::internal(error.to_string()))?
      .map_err(|error| ApiError::internal(error.to_string()))?;
    ok(result)
}

#[cfg(test)]
mod tests {
    use super::parse_tracked;

    #[test]
    fn tracked_parser_rejects_duplicates_and_unbounded_input() {
        let one = format!("0x{}-0", "11".repeat(32));
        assert_eq!(parse_tracked(Some(&one)).unwrap().len(), 1);
        assert!(parse_tracked(Some(&format!("{one},{one}"))).is_err());
        assert!(parse_tracked(Some(&vec![one; 65].join(","))).is_err());
    }
}
