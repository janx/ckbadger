//! Read-only, node-backed evidence for an indexed Fiber channel. The current
//! commitment is the writer's tracked outpoint, which may have been replaced
//! by a partial settlement after the original force-close transaction.
use anyhow::{anyhow, ensure, Context, Result};
use axum::extract::{Path, State};
use ckbadger_indexer::{parser::fiber as protocol, rpc::TransactionView};
use ckbadger_store::types::{CachedBlockHeader, FiberChannel, FiberChannelState};
use serde::Serialize;
use std::sync::Arc;

use super::{canonical_header, decode_epoch_delay, format_hex};
use crate::{
    response::{ok, ApiError, ApiResult},
    utils::parse_hash32,
    AppState,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberChannelEvidenceResponse {
    pub channel_id: String,
    pub funding: FiberCellEvidence,
    pub current_commitment: Option<FiberCommitmentEvidence>,
    pub opening_party_flows: Vec<FiberPartyFlow>,
    pub stages: Vec<FiberStageEvidence>,
    pub settlement_outputs: Vec<FiberSettlementOutput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberCellEvidence {
    pub tx_hash: String,
    pub output_index: u32,
    pub block_number: i64,
    pub capacity: String,
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub is_live: bool,
    pub consumed_by_tx: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberCommitmentEvidence {
    #[serde(flatten)]
    pub cell: FiberCellEvidence,
    pub pubkey_hash: String,
    pub delay_epoch: String,
    pub version: String,
    pub settlement_hash: Option<String>,
    pub settlement_flag: Option<u8>,
    pub lock_format: String,
    pub epoch_lock: String,
    pub ready_epoch: Option<String>,
    pub epoch_lock_met_at_tip: Option<bool>,
    pub tip_epoch: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberPartyFlow {
    pub lock_script_hash: String,
    /// All opening inputs controlled by this lock; not a signed channel allocation.
    pub opening_input_capacity: String,
    /// All opening outputs to this lock, including change and unrelated outputs.
    pub opening_output_capacity: String,
    /// Input minus output, includes transaction fee and any other activity.
    pub observed_net_input: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberStageEvidence {
    pub stage: String,
    pub tx_hash: String,
    pub block_number: i64,
    pub timestamp: String,
    pub inputs: usize,
    pub outputs: usize,
    pub witness_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberSettlementOutput {
    pub tx_hash: String,
    pub output_index: u32,
    pub owner_lock_hash: String,
    pub observed_party: bool,
    pub capacity: String,
}

fn parse_hex(value: &str) -> Result<Vec<u8>> {
    hex::decode(
        value
            .strip_prefix("0x")
            .ok_or_else(|| anyhow!("missing 0x prefix: {value}"))?,
    )
    .with_context(|| format!("invalid hex {value}"))
}

fn parse_u64(value: &str) -> Result<u64> {
    let hex = value
        .strip_prefix("0x")
        .ok_or_else(|| anyhow!("missing 0x prefix: {value}"))?;
    u64::from_str_radix(hex, 16).with_context(|| format!("invalid u64 {value}"))
}

fn hash_type_byte(value: &str) -> Result<u8> {
    match value {
        "data" => Ok(0),
        "type" => Ok(1),
        "data1" => Ok(2),
        "data2" => Ok(4),
        _ => Err(anyhow!("invalid Fiber output hash_type {value}")),
    }
}

fn output_lock_hash(output: &ckbadger_indexer::rpc::CellOutput) -> Result<Vec<u8>> {
    let code = parse_hex(&output.lock.code_hash)?;
    ensure!(
        code.len() == 32,
        "Fiber output lock code hash width {}",
        code.len()
    );
    Ok(crate::utils::address::compute_script_hash(
        &code,
        hash_type_byte(&output.lock.hash_type)?,
        &parse_hex(&output.lock.args)?,
    ))
}

fn canonical_raw_tx(state: &AppState, hash: &[u8], expected_block: i64) -> Result<TransactionView> {
    use ckb_types::prelude::*;
    ensure!(
        hash.len() == 32,
        "Fiber transaction hash width {}",
        hash.len()
    );
    let (block, _, indexed) = state.store.get_tx_by_hash(hash)?.ok_or_else(|| {
        anyhow!(
            "Fiber transaction 0x{} missing canonical index",
            hex::encode(hash)
        )
    })?;
    ensure!(
        block == expected_block,
        "Fiber transaction 0x{} indexed at {block}, expected {expected_block}",
        hex::encode(hash)
    );
    let reader = state.ckb_store.as_ref().ok_or_else(|| {
        anyhow!(
            "Fiber on-chain evidence requires local CKB RocksDB reader: tx=0x{}",
            hex::encode(hash)
        )
    })?;
    let id: [u8; 32] = hash.try_into()?;
    let (tx, node_block) = reader
        .get_transaction_with_block_number(&id)
        .ok_or_else(|| {
            anyhow!(
                "Fiber transaction 0x{} missing from local CKB store",
                hex::encode(hash)
            )
        })?;
    ensure!(
        node_block as i128 == i128::from(block) && tx.hash().as_slice() == hash,
        "Fiber transaction 0x{} local node location/hash differs from canonical index",
        hex::encode(hash)
    );
    ensure!(
        usize::try_from(indexed.outputs_count)? == tx.outputs().len(),
        "Fiber transaction 0x{} output count differs from canonical index",
        hex::encode(hash)
    );
    Ok(super::super::transactions::indexer_tx_view(
        &ckb_store_reader::convert_transaction_view(&tx),
    ))
}

fn cell_evidence(version: &super::super::asset_history::CanonicalCellVersion) -> FiberCellEvidence {
    FiberCellEvidence {
        tx_hash: version.response.tx_hash.clone(),
        output_index: version.response.output_index as u32,
        block_number: version.response.block_number,
        capacity: version.response.capacity.clone(),
        lock_code_hash: version.response.lock.code_hash.clone(),
        lock_hash_type: version.response.lock.hash_type.clone(),
        lock_args: version.response.lock.args.clone(),
        is_live: version.response.is_live,
        consumed_by_tx: version.response.consumed_by_tx.clone(),
    }
}

fn stage(
    stage: &str,
    hash: &[u8],
    header: &CachedBlockHeader,
    block: i64,
    tx: &TransactionView,
) -> FiberStageEvidence {
    FiberStageEvidence {
        stage: stage.into(),
        tx_hash: format_hex(hash),
        block_number: block,
        timestamp: header.timestamp.to_string(),
        inputs: tx.inputs.len(),
        outputs: tx.outputs.len(),
        witness_count: tx.witnesses.len(),
    }
}

/// Add a relative epoch fraction to the tracked cell's creation epoch, using
/// exact integer arithmetic. Neither calendar days nor wall time enter it.
fn ready_epoch(header: &CachedBlockHeader, delay: u64) -> Result<String> {
    let part = decode_epoch_delay(delay).map_err(anyhow::Error::msg)?;
    ensure!(part.relative, "Fiber commitment delay must be relative");
    ensure!(
        header.epoch_length > 0
            && header.epoch_index >= 0
            && header.epoch_index < header.epoch_length,
        "Fiber commitment creation header invalid epoch fraction"
    );
    let den = i128::from(header.epoch_length)
        .checked_mul(i128::from(part.length))
        .ok_or_else(|| anyhow!("Fiber epoch denominator overflow"))?;
    let num = i128::from(header.epoch_index)
        .checked_mul(i128::from(part.length))
        .and_then(|v| v.checked_add(i128::from(part.index) * i128::from(header.epoch_length)))
        .ok_or_else(|| anyhow!("Fiber epoch numerator overflow"))?;
    let whole = i128::from(header.epoch_number)
        .checked_add(i128::from(part.epochs))
        .and_then(|v| v.checked_add(num / den))
        .ok_or_else(|| anyhow!("Fiber epoch overflow"))?;
    let remainder = num % den;
    let divisor = gcd(remainder, den);
    Ok(format!(
        "{}+{}/{}",
        whole,
        remainder / divisor,
        den / divisor
    ))
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn epoch_at_or_after(header: &CachedBlockHeader, target: &str) -> Result<bool> {
    let (whole, fraction) = target
        .split_once('+')
        .ok_or_else(|| anyhow!("invalid target epoch"))?;
    let (num, den) = fraction
        .split_once('/')
        .ok_or_else(|| anyhow!("invalid target fraction"))?;
    let (whole, num, den) = (
        whole.parse::<i128>()?,
        num.parse::<i128>()?,
        den.parse::<i128>()?,
    );
    ensure!(
        den > 0 && num >= 0 && num < den,
        "invalid target epoch fraction {target}"
    );
    ensure!(
        header.epoch_length > 0
            && header.epoch_index >= 0
            && header.epoch_index < header.epoch_length,
        "Fiber tip header invalid epoch fraction"
    );
    let header_whole = i128::from(header.epoch_number);
    if header_whole != whole {
        return Ok(header_whole > whole);
    }
    let left = i128::from(header.epoch_index)
        .checked_mul(den)
        .ok_or_else(|| anyhow!("Fiber tip epoch comparison overflow"))?;
    let right = num
        .checked_mul(i128::from(header.epoch_length))
        .ok_or_else(|| anyhow!("Fiber target epoch comparison overflow"))?;
    Ok(left >= right)
}

fn commitment_epoch_rule(
    flag: Option<u8>,
    header: &CachedBlockHeader,
    tip: &CachedBlockHeader,
    delay: u64,
) -> Result<(String, Option<String>, Option<bool>)> {
    match flag {
        Some(0) => {
            let ready = ready_epoch(header, delay)?;
            let met = epoch_at_or_after(tip, &ready)?;
            Ok(("firstSettlementDelay".into(), Some(ready), Some(met)))
        }
        // fiber-scripts commitment-lock main.rs: party-settlement since is
        // guarded by `is_first_settlement` (args[56] == 0). A subsequent
        // commitment can still have other HTLC and tx validity conditions.
        Some(1) => Ok(("subsequentSettlementNoReappliedDelay".into(), None, None)),
        Some(value) => Err(anyhow!("invalid Fiber settlement flag {value}")),
        // Historic 36-byte locks do not expose a flag. Their deployed rule
        // cannot be inferred from the modern 57-byte contract.
        None => Ok(("legacyFormatRuleUnverified".into(), None, None)),
    }
}

fn is_updated_commitment(close_tx_hash: Option<&[u8]>, tracked_tx_hash: &[u8]) -> bool {
    close_tx_hash != Some(tracked_tx_hash)
}

fn build_evidence(
    state: &AppState,
    id: &[u8],
    ch: &FiberChannel,
) -> Result<FiberChannelEvidenceResponse> {
    use super::super::{asset_history::canonical_version_at, asset_protocol::raw_transaction};
    let index = i16::try_from(ch.funding_output_index)
        .context("Fiber funding output index out of range")?;
    let funding = canonical_version_at(state, id, &ch.funding_tx_hash, index)?;
    ensure!(
        funding.response.block_number == ch.open_block
            && funding.cell.capacity == i64::try_from(ch.capacity)?,
        "Fiber channel 0x{} funding location/capacity mismatch",
        hex::encode(id)
    );
    ensure!(
        protocol::is_funding_lock(&funding.cell.lock_code_hash)
            && funding.cell.lock_args == ch.funding_lock_args,
        "Fiber channel 0x{} stored funding lock differs from canonical output",
        hex::encode(id)
    );
    let opening_tx = raw_transaction(state, &funding)?;
    let opening_header = canonical_header(&state.store, ch.open_block)?;
    let mut stages = vec![stage(
        "opening",
        &ch.funding_tx_hash,
        &opening_header,
        ch.open_block,
        &opening_tx,
    )];
    let mut flows = Vec::new();
    for party in &ch.participants {
        let mut inputs = 0u128;
        for input in &opening_tx.inputs {
            let prev_hash = parse_hex(&input.previous_output.tx_hash)?;
            let prev_index = i16::try_from(parse_u64(&input.previous_output.index)?)?;
            let previous = canonical_version_at(state, id, &prev_hash, prev_index)?;
            if &previous.cell.lock_script_hash == party {
                inputs = inputs
                    .checked_add(u128::try_from(previous.cell.capacity)?)
                    .ok_or_else(|| anyhow!("Fiber opening input capacity overflow"))?;
            }
        }
        let mut outputs = 0u128;
        for output in &opening_tx.outputs {
            if &output_lock_hash(output)? == party {
                outputs = outputs
                    .checked_add(u128::from(parse_u64(&output.capacity)?))
                    .ok_or_else(|| anyhow!("Fiber opening output capacity overflow"))?;
            }
        }
        flows.push(FiberPartyFlow {
            lock_script_hash: format_hex(party),
            opening_input_capacity: inputs.to_string(),
            opening_output_capacity: outputs.to_string(),
            observed_net_input: (i128::try_from(inputs)? - i128::try_from(outputs)?).to_string(),
        });
    }
    let mut settlement_outputs = Vec::new();
    if ch.state != FiberChannelState::Open {
        let close_hash = ch
            .close_tx_hash
            .as_ref()
            .ok_or_else(|| anyhow!("closed Fiber channel missing close transaction"))?;
        let close_block = ch
            .close_block
            .ok_or_else(|| anyhow!("closed Fiber channel missing close block"))?;
        let tx = canonical_raw_tx(state, close_hash, close_block)?;
        ensure!(
            tx.inputs.iter().any(
                |input| parse_hex(&input.previous_output.tx_hash).ok().as_deref()
                    == Some(ch.funding_tx_hash.as_slice())
                    && parse_u64(&input.previous_output.index).ok()
                        == Some(u64::from(ch.funding_output_index))
            ),
            "Fiber channel 0x{} close transaction does not consume funding outpoint",
            hex::encode(id)
        );
        ensure!(
            funding.response.consumed_by_tx.as_deref() == Some(format_hex(close_hash).as_str()),
            "Fiber channel 0x{} funding consumer differs from close transaction",
            hex::encode(id)
        );
        stages.push(stage(
            "close",
            close_hash,
            &canonical_header(&state.store, close_block)?,
            close_block,
            &tx,
        ));
    }
    let current_commitment = if let Some(hash) = &ch.commitment_tx_hash {
        let output_index = ch
            .commitment_output_index
            .ok_or_else(|| anyhow!("Fiber tracked commitment missing output index"))?;
        let version = canonical_version_at(state, id, hash, i16::try_from(output_index)?)?;
        let tx = raw_transaction(state, &version)?;
        ensure!(
            protocol::is_commitment_lock(&version.cell.lock_code_hash),
            "Fiber tracked output is not a commitment lock: 0x{}:{output_index}",
            hex::encode(hash)
        );
        let args = &version.cell.lock_args;
        ensure!(
            args.len() == 36 || args.len() == 57,
            "Fiber commitment 0x{}:{output_index} has unsupported args length {}",
            hex::encode(hash),
            args.len()
        );
        let parsed = protocol::parse_commitment_lock_args(args)
            .ok_or_else(|| anyhow!("Fiber commitment args invalid"))?;
        ensure!(
            Some(parsed.delay_epoch) == ch.delay_epoch,
            "Fiber commitment 0x{}:{output_index} delay differs from indexed channel",
            hex::encode(hash)
        );
        let header = canonical_header(&state.store, version.response.block_number)?;
        let (_, tip) = state
            .store
            .get_sync_tip_block()?
            .ok_or_else(|| anyhow!("Fiber evidence missing indexed tip"))?;
        let (epoch_lock, ready, met) =
            commitment_epoch_rule(parsed.settlement_flag, &header, &tip, parsed.delay_epoch)
                .with_context(|| {
                    format!("Fiber commitment 0x{}:{output_index}", hex::encode(hash))
                })?;
        if is_updated_commitment(ch.close_tx_hash.as_deref(), hash) {
            stages.push(stage(
                "currentCommitment",
                hash,
                &header,
                version.response.block_number,
                &tx,
            ));
        }
        if ch.state == FiberChannelState::ForceClosed {
            ensure!(
                version.response.is_live,
                "Fiber channel 0x{} force-close tracked commitment is not live",
                hex::encode(id)
            );
        }
        if ch.state == FiberChannelState::Settled {
            ensure!(
                version.response.consumed_by_tx.as_deref()
                    == ch
                        .settlement_tx_hash
                        .as_ref()
                        .map(|h| format_hex(h))
                        .as_deref(),
                "Fiber channel 0x{} tracked commitment consumer differs from settlement",
                hex::encode(id)
            );
        }
        Some(FiberCommitmentEvidence {
            cell: cell_evidence(&version),
            pubkey_hash: format_hex(&parsed.pubkey_hash),
            delay_epoch: parsed.delay_epoch.to_string(),
            version: parsed.version.to_string(),
            settlement_hash: parsed.settlement_hash.as_ref().map(|v| format_hex(v)),
            settlement_flag: parsed.settlement_flag,
            lock_format: if args.len() == 36 {
                "legacy36"
            } else {
                "full57"
            }
            .into(),
            epoch_lock,
            ready_epoch: ready,
            epoch_lock_met_at_tip: met,
            tip_epoch: format!(
                "{}+{}/{}",
                tip.epoch_number, tip.epoch_index, tip.epoch_length
            ),
        })
    } else {
        ensure!(
            ch.commitment_output_index.is_none()
                && ch.delay_epoch.is_none()
                && ch.state != FiberChannelState::ForceClosed
                && ch.state != FiberChannelState::Settled,
            "Fiber channel 0x{} missing tracked commitment",
            hex::encode(id)
        );
        None
    };
    if ch.state == FiberChannelState::Settled {
        let hash = ch
            .settlement_tx_hash
            .as_ref()
            .ok_or_else(|| anyhow!("settled Fiber channel missing settlement transaction"))?;
        let block = ch
            .settlement_block
            .ok_or_else(|| anyhow!("settled Fiber channel missing settlement block"))?;
        let tx = canonical_raw_tx(state, hash, block)?;
        stages.push(stage(
            "settlement",
            hash,
            &canonical_header(&state.store, block)?,
            block,
            &tx,
        ));
        for (index, output) in tx.outputs.iter().enumerate() {
            let owner = output_lock_hash(output)?;
            settlement_outputs.push(FiberSettlementOutput {
                tx_hash: format_hex(hash),
                output_index: u32::try_from(index)?,
                observed_party: ch.participants.contains(&owner),
                owner_lock_hash: format_hex(&owner),
                capacity: u128::from(parse_u64(&output.capacity)?).to_string(),
            });
        }
    }
    Ok(FiberChannelEvidenceResponse {
        channel_id: format_hex(id),
        funding: cell_evidence(&funding),
        current_commitment,
        opening_party_flows: flows,
        stages,
        settlement_outputs,
    })
}

pub(super) async fn get_channel_evidence(
    State(state): State<Arc<AppState>>,
    Path(channel_id): Path<String>,
) -> ApiResult<FiberChannelEvidenceResponse> {
    let id = parse_hash32(&channel_id, "channel_id")?;
    let ch = state
        .store
        .get_fiber_channel(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Fiber channel not found"))?;
    if state.ckb_store.is_none() {
        return Err(ApiError::internal(format!(
            "Fiber on-chain evidence requires local CKB RocksDB reader: channel={channel_id}"
        )));
    }
    let result = tokio::task::spawn_blocking(move || build_evidence(&state, &id, &ch))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_zero_fraction_normalizes_and_ready_epoch_is_exact() {
        let mut header = CachedBlockHeader {
            hash: vec![],
            parent_hash: vec![],
            timestamp: 0,
            epoch_number: 100,
            epoch_index: 2,
            epoch_length: 3,
            dao: vec![],
            transactions_count: 0,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 0,
        };
        assert_eq!(
            ready_epoch(&header, 0xa000_0000_0000_0001).unwrap(),
            "101+2/3"
        );
        assert!(epoch_at_or_after(&header, "100+2/3").unwrap());
        header.epoch_index = 1;
        assert!(!epoch_at_or_after(&header, "100+2/3").unwrap());
        assert_eq!(
            ready_epoch(&header, 0xa000_0200_0000_0001).unwrap(),
            "101+1/3"
        );
        header.epoch_number = i64::MAX;
        header.epoch_index = i32::MAX - 1;
        header.epoch_length = i32::MAX;
        assert!(epoch_at_or_after(&header, "100+0/1").unwrap());
        assert!(!epoch_at_or_after(&header, "9223372036854775808+0/1").unwrap());
    }

    #[test]
    fn subsequent_partial_payout_has_no_reapplied_delay_and_legacy_is_unknown() {
        // Actual Fiber force-close 0xa4c598… later produced the tracked
        // commitment in 0x11ab778…; the modern contract's flag 1 represents
        // that later cell. Source: nervosnetwork/fiber-scripts,
        // contracts/commitment-lock/src/main.rs, main branch, lines 1732-38,
        // 2151-55, 2197-2201, 2263-69 (reviewed 2026-10-28).
        // https://github.com/nervosnetwork/fiber-scripts/blob/main/contracts/commitment-lock/src/main.rs
        let mut header = CachedBlockHeader {
            hash: vec![],
            parent_hash: vec![],
            timestamp: 0,
            epoch_number: 100,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![],
            transactions_count: 0,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 0,
        };
        let (_, ready, met) =
            commitment_epoch_rule(Some(1), &header, &header, 0xa001_0000_0000_0006).unwrap();
        assert!(ready.is_none() && met.is_none());
        assert_eq!(
            commitment_epoch_rule(None, &header, &header, 0xa001_0000_0000_0006)
                .unwrap()
                .0,
            "legacyFormatRuleUnverified"
        );
        let (_, ready, met) =
            commitment_epoch_rule(Some(0), &header, &header, 0xa001_0000_0000_0006).unwrap();
        assert_eq!(ready.as_deref(), Some("106+0/1"));
        assert_eq!(met, Some(false));
        header.epoch_number = 106;
        assert_eq!(
            commitment_epoch_rule(Some(0), &header, &header, 0xa001_0000_0000_0006)
                .unwrap()
                .1
                .as_deref(),
            Some("112+0/1")
        );
        let close = hex::decode("a4c598859bfcb32180bf58893f150388ecf5e2eaf876de00aba539b9cb0a4396")
            .unwrap();
        let tracked =
            hex::decode("11ab7787b5f36a2513db3a0ab83595b716ec1316a7ae5b05ef542347389ac9ed")
                .unwrap();
        assert!(is_updated_commitment(Some(&close), &tracked));
        assert!(!is_updated_commitment(Some(&tracked), &tracked));
    }
}
