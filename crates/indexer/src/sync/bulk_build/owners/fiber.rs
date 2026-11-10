use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::types::{FiberChannel, FiberChannelState};
use ckbadger_store::{CF_ADDR_FIBER_CHANNELS, CF_FIBER_CHANNELS, CF_FIBER_CHANNEL_BY_COMMITMENT};

use super::{BulkReducer, ReducerContext};
use crate::db::writer::fiber::apply_commitment_enrichment;
use crate::parser::fiber::{
    is_commitment_lock, is_funding_lock, parse_commitment_lock_args, parse_funding_cell,
    parse_funding_udt_amount, CommitmentLockArgs,
};
use crate::sync::bulk_build::facts::{CellFacts, OutPointKey, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};

#[derive(Debug, Default)]
pub(crate) struct FiberOwner {
    channels: BTreeMap<Vec<u8>, FiberChannel>,
    channel_by_commitment: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl FiberOwner {
    pub(crate) fn emit_snapshot_rows<F>(&self, mut emit: F) -> Result<()>
    where
        F: FnMut(MaterializedRow) -> Result<()>,
    {
        for (channel_id, channel) in &self.channels {
            emit(MaterializedRow::new(
                CF_FIBER_CHANNELS,
                channel_id.clone(),
                bincode::serialize(channel)?,
            ))?;
        }

        for (commitment_hash, channel_id) in &self.channel_by_commitment {
            emit(MaterializedRow::new(
                CF_FIBER_CHANNEL_BY_COMMITMENT,
                commitment_hash.clone(),
                channel_id.clone(),
            ))?;
        }

        for (channel_id, channel) in &self.channels {
            for participant in &channel.participants {
                emit(MaterializedRow::new(
                    CF_ADDR_FIBER_CHANNELS,
                    keys::encode_addr_fiber_channel_key(participant, channel_id),
                    Vec::new(),
                ))?;
            }
        }

        Ok(())
    }

    #[cfg(test)]
    fn build_snapshot_rows(&self) -> Result<Vec<MaterializedRow>> {
        let mut rows = Vec::new();
        self.emit_snapshot_rows(|row| {
            rows.push(row);
            Ok(())
        })?;
        Ok(rows)
    }

    #[cfg(test)]
    pub(crate) fn build_final_rows(&self) -> Result<super::super::materialize::OwnerFinalRows> {
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: Vec::new(),
            snapshot_rows: self.build_snapshot_rows()?,
        })
    }
}

impl BulkReducer for FiberOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let summary = FiberTxSummary::from_tx(tx, ctx)?;
        let Some(event) = summary.classify_event() else {
            return Ok(());
        };

        match event {
            FiberEvent::ChannelOpen => self.handle_channel_open(tx, &summary),
            FiberEvent::ChannelClose => self.handle_channel_close(tx, &summary),
            FiberEvent::ForceClose => self.handle_force_close(tx, &summary),
            FiberEvent::Settlement => self.handle_settlement(tx, &summary),
            FiberEvent::CommitmentRevocation => self.handle_commitment_revocation(tx, &summary),
        }
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        materializer.materialize_final_snapshot_bounded(|sink| {
            self.emit_snapshot_rows(|row| sink.push(row))
        })
    }
}

impl FiberOwner {
    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::btree_map_serialized_bytes(&self.channels)
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.channel_by_commitment,
            )
    }

    fn handle_channel_open(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary<'_>,
    ) -> Result<()> {
        let funding_lock_args = summary
            .funding_output_pubkey_hash
            .clone()
            .ok_or_else(|| {
                anyhow!(
                    "fiber channel_open missing parsed funding lock args in bulk reducer: block={} tx=0x{} tx_index={}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index
                )
            })?;
        let output_index = summary.funding_output_index.ok_or_else(|| {
            anyhow!(
                "fiber channel_open missing funding output index in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let capacity = summary.funding_output_capacity.ok_or_else(|| {
            anyhow!(
                "fiber channel_open missing funding output capacity in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        if summary.participants.is_empty() {
            bail!(
                "fiber channel_open has no non-fiber participants in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            );
        }

        let channel_id = keys::encode_fiber_channel_id(&tx.tx_hash, output_index);
        if self.channels.contains_key(channel_id.as_slice()) {
            bail!(
                "duplicate fiber channel open in bulk reducer: block={} tx=0x{} tx_index={} channel_id=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(&channel_id)
            );
        }
        let (udt_type_hash, udt_amount) = match &summary.funding_output_udt {
            Some((type_hash, amount)) => (Some(type_hash.clone()), Some(*amount)),
            None => (None, None),
        };
        let channel = FiberChannel {
            funding_tx_hash: tx.tx_hash.to_vec(),
            funding_output_index: output_index,
            state: FiberChannelState::Open,
            capacity,
            udt_type_hash,
            udt_amount,
            open_block: tx.block_number,
            open_timestamp: tx.timestamp_ms,
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: summary.participants.iter().cloned().collect(),
            funding_lock_args: funding_lock_args.clone(),
        };

        self.channels.insert(channel_id, channel);
        Ok(())
    }

    fn handle_channel_close(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary<'_>,
    ) -> Result<()> {
        let funding_outpoint = summary.funding_input_outpoint.ok_or_else(|| {
            anyhow!(
                "fiber channel_close missing funding input outpoint in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let channel_id =
            keys::encode_fiber_channel_id(&funding_outpoint.tx_hash, funding_outpoint.index);
        let channel = self.channels.get_mut(channel_id.as_slice()).ok_or_else(|| {
            anyhow!(
                "fiber channel_close missing channel state in bulk reducer: block={} tx=0x{} tx_index={} funding_outpoint=0x{}:{} channel_id=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(funding_outpoint.tx_hash),
                funding_outpoint.index,
                hex::encode(&channel_id)
            )
        })?;
        if channel.state != FiberChannelState::Open {
            bail!(
                "fiber channel_close expected open channel in bulk reducer: block={} tx=0x{} tx_index={} state={:?} funding_outpoint=0x{}:{} channel_id=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                channel.state,
                hex::encode(funding_outpoint.tx_hash),
                funding_outpoint.index,
                hex::encode(&channel_id)
            );
        }

        channel.state = FiberChannelState::CooperativelyClosed;
        channel.close_tx_hash = Some(tx.tx_hash.to_vec());
        channel.close_block = Some(tx.block_number);
        channel.close_timestamp = Some(tx.timestamp_ms);
        Ok(())
    }

    fn handle_force_close(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary<'_>,
    ) -> Result<()> {
        let funding_outpoint = summary.funding_input_outpoint.ok_or_else(|| {
            anyhow!(
                "fiber force_close missing funding input outpoint in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let commitment_args = summary.commitment_output_args.as_deref().ok_or_else(|| {
            anyhow!(
                "fiber force_close missing commitment output args in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let commitment_parsed = summary.commitment_output_parsed_args.ok_or_else(|| {
            anyhow!(
                "fiber force_close missing parsed commitment output args in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let commitment_output_index = summary.commitment_output_index.ok_or_else(|| {
            anyhow!(
                "fiber force_close missing commitment output index in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let channel_id =
            keys::encode_fiber_channel_id(&funding_outpoint.tx_hash, funding_outpoint.index);
        let commitment_hash = blake2b_hash(commitment_args);
        if let Some(existing) = self.channel_by_commitment.get(commitment_hash.as_slice()) {
            if existing != &channel_id {
                bail!(
                    "fiber force_close duplicate commitment mapping in bulk reducer: block={} tx=0x{} tx_index={} commitment_hash=0x{} existing_channel_id=0x{} new_channel_id=0x{}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    hex::encode(&commitment_hash),
                    hex::encode(existing),
                    hex::encode(&channel_id)
                );
            }
        }
        let channel = self.channels.get_mut(channel_id.as_slice()).ok_or_else(|| {
            anyhow!(
                "fiber force_close missing channel state in bulk reducer: block={} tx=0x{} tx_index={} funding_outpoint=0x{}:{} channel_id=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(funding_outpoint.tx_hash),
                funding_outpoint.index,
                hex::encode(&channel_id)
            )
        })?;
        if channel.state != FiberChannelState::Open {
            bail!(
                "fiber force_close expected open channel in bulk reducer: block={} tx=0x{} tx_index={} state={:?} funding_outpoint=0x{}:{} channel_id=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                channel.state,
                hex::encode(funding_outpoint.tx_hash),
                funding_outpoint.index,
                hex::encode(&channel_id)
            );
        }

        channel.state = FiberChannelState::ForceClosed;
        channel.close_tx_hash = Some(tx.tx_hash.to_vec());
        channel.close_block = Some(tx.block_number);
        channel.close_timestamp = Some(tx.timestamp_ms);
        apply_commitment_enrichment(
            channel,
            tx.tx_hash.as_slice(),
            &commitment_parsed,
            commitment_output_index,
        );

        self.channel_by_commitment
            .insert(commitment_hash, channel_id.clone());
        Ok(())
    }

    fn handle_settlement(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary<'_>,
    ) -> Result<()> {
        let commitment_args = summary.commitment_input_args.as_deref().ok_or_else(|| {
            anyhow!(
                "fiber settlement missing commitment input args in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let commitment_hash = blake2b_hash(commitment_args);
        let channel_id = self
            .channel_by_commitment
            .get(commitment_hash.as_slice())
            .ok_or_else(|| {
                anyhow!(
                    "fiber settlement missing channel by commitment in bulk reducer: block={} tx=0x{} tx_index={} commitment_hash=0x{}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    hex::encode(&commitment_hash)
                )
            })?
            .clone();
        let channel = self.channels.get_mut(channel_id.as_slice()).ok_or_else(|| {
            anyhow!(
                "fiber settlement missing channel state in bulk reducer: block={} tx=0x{} tx_index={} commitment_hash=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(&commitment_hash)
            )
        })?;
        if channel.state != FiberChannelState::ForceClosed {
            bail!(
                "fiber settlement expected force-closed channel in bulk reducer: block={} tx=0x{} tx_index={} state={:?} commitment_hash=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                channel.state,
                hex::encode(&commitment_hash)
            );
        }

        channel.state = FiberChannelState::Settled;
        channel.settlement_tx_hash = Some(tx.tx_hash.to_vec());
        channel.settlement_block = Some(tx.block_number);
        channel.settlement_timestamp = Some(tx.timestamp_ms);
        Ok(())
    }

    fn handle_commitment_revocation(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary<'_>,
    ) -> Result<()> {
        let old_commitment_args =
            summary.commitment_input_args.as_deref().ok_or_else(|| {
                anyhow!(
                    "fiber commitment_revocation missing commitment input args in bulk reducer: block={} tx=0x{} tx_index={}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index
                )
            })?;
        let new_commitment_args =
            summary.commitment_output_args.as_deref().ok_or_else(|| {
                anyhow!(
                    "fiber commitment_revocation missing commitment output args in bulk reducer: block={} tx=0x{} tx_index={}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index
                )
            })?;

        let old_hash = blake2b_hash(old_commitment_args);
        let new_hash = blake2b_hash(new_commitment_args);

        let channel_id = self
            .channel_by_commitment
            .get(old_hash.as_slice())
            .ok_or_else(|| {
                anyhow!(
                    "fiber commitment_revocation missing channel by commitment in bulk reducer: block={} tx=0x{} tx_index={} commitment_hash=0x{}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    hex::encode(&old_hash)
                )
            })?
            .clone();

        let channel = self.channels.get_mut(channel_id.as_slice()).ok_or_else(|| {
            anyhow!(
                "fiber commitment_revocation missing channel state in bulk reducer: block={} tx=0x{} tx_index={} commitment_hash=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(&old_hash)
            )
        })?;
        if channel.state != FiberChannelState::ForceClosed {
            bail!(
                "fiber commitment_revocation expected force-closed channel in bulk reducer: block={} tx=0x{} tx_index={} state={:?} commitment_hash=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                channel.state,
                hex::encode(&old_hash)
            );
        }

        // Rotate commitment: update channel metadata, swap hash mapping
        let new_commitment_parsed = summary.commitment_output_parsed_args.ok_or_else(|| {
            anyhow!(
                "fiber commitment_revocation missing parsed commitment output args in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        let new_commitment_output_index = summary.commitment_output_index.ok_or_else(|| {
            anyhow!(
                "fiber commitment_revocation missing commitment output index in bulk reducer: block={} tx=0x{} tx_index={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        apply_commitment_enrichment(
            channel,
            tx.tx_hash.as_slice(),
            &new_commitment_parsed,
            new_commitment_output_index,
        );

        self.channel_by_commitment.remove(old_hash.as_slice());
        self.channel_by_commitment.insert(new_hash, channel_id);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FiberEvent {
    ChannelOpen,
    ChannelClose,
    ForceClose,
    Settlement,
    CommitmentRevocation,
}

#[derive(Debug, Default)]
struct FiberTxSummary<'a> {
    has_funding_input: bool,
    has_funding_output: bool,
    has_commitment_input: bool,
    has_commitment_output: bool,
    funding_input_outpoint: Option<OutPointKey>,
    funding_output_pubkey_hash: Option<Vec<u8>>,
    funding_output_capacity: Option<u64>,
    funding_output_index: Option<u32>,
    commitment_input_args: Option<Vec<u8>>,
    commitment_output_args: Option<Vec<u8>>,
    commitment_output_index: Option<u32>,
    commitment_output_parsed_args: Option<CommitmentLockArgs<'a>>,
    /// UDT identity of the funding output, when it carries a type script.
    funding_output_udt: Option<(Vec<u8>, u128)>,
    participants: BTreeSet<Vec<u8>>,
}

impl<'a> FiberTxSummary<'a> {
    fn from_tx(tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'a>) -> Result<Self> {
        let mut summary = Self::default();

        for input in &tx.resolved_inputs {
            summary.record_input(tx, input, ctx)?;
        }
        for cell in tx.cells.iter() {
            summary.record_output(tx, cell, ctx)?;
        }

        Ok(summary)
    }

    fn classify_event(&self) -> Option<FiberEvent> {
        if self.has_funding_output && !self.has_funding_input {
            Some(FiberEvent::ChannelOpen)
        } else if self.has_funding_input && !self.has_commitment_output {
            Some(FiberEvent::ChannelClose)
        } else if self.has_funding_input && self.has_commitment_output {
            Some(FiberEvent::ForceClose)
        } else if self.has_commitment_input && self.has_commitment_output {
            Some(FiberEvent::CommitmentRevocation)
        } else if self.has_commitment_input {
            Some(FiberEvent::Settlement)
        } else {
            None
        }
    }

    /// Record one input. Whether it is a channel cell is decided by the
    /// parser's arbiters alone; a cell they reject is an ordinary cell and its
    /// lock an ordinary participant. A cell whose lock code hash is not a
    /// Fiber lock costs exactly the two code-hash checks and the insert.
    fn record_input(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        input: &ResolvedInputFacts,
        ctx: &ReducerContext<'a>,
    ) -> Result<()> {
        let lock_code_hash = ctx.resolve_identity(input.lock_code_hash_id);
        let lock_args = ctx.resolve_identity(input.lock_args_id);
        if is_funding_lock(lock_code_hash) {
            let data_len = fiber_cell_data_len(input.data_size, tx, &input.outpoint)?;
            if parse_funding_cell(lock_args, input.type_code_hash_id.is_some(), data_len).is_some()
            {
                self.has_funding_input = true;
                if self.funding_input_outpoint.is_none() {
                    self.funding_input_outpoint = Some(input.outpoint);
                }
                return Ok(());
            }
        } else if is_commitment_lock(lock_code_hash)
            && parse_commitment_lock_args(lock_args).is_some()
        {
            self.has_commitment_input = true;
            if self.commitment_input_args.is_none() {
                self.commitment_input_args = Some(lock_args.to_vec());
            }
            return Ok(());
        }

        self.participants
            .insert(ctx.resolve_identity(input.lock_script_hash_id).to_vec());
        Ok(())
    }

    /// Record one output; classified exactly as [`Self::record_input`].
    fn record_output(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        cell: &CellFacts,
        ctx: &ReducerContext<'a>,
    ) -> Result<()> {
        let lock_code_hash = ctx.resolve_identity(cell.lock_code_hash_id);
        let lock_args = ctx.resolve_identity(cell.lock_args_id);
        if is_funding_lock(lock_code_hash) {
            let data_len = fiber_cell_data_len(cell.data_size, tx, &cell.outpoint)?;
            let typed = cell.type_code_hash_id.is_some();
            if let Some(parsed) = parse_funding_cell(lock_args, typed, data_len) {
                self.has_funding_output = true;
                if self.funding_output_pubkey_hash.is_none() {
                    self.record_funding_output(tx, cell, ctx, parsed.pubkey_hash)?;
                }
                return Ok(());
            }
        } else if is_commitment_lock(lock_code_hash) {
            if let Some(parsed) = parse_commitment_lock_args(lock_args) {
                self.has_commitment_output = true;
                if self.commitment_output_args.is_none() {
                    self.commitment_output_args = Some(lock_args.to_vec());
                    self.commitment_output_index = Some(cell.outpoint.index);
                    self.commitment_output_parsed_args = Some(parsed);
                }
                return Ok(());
            }
        }

        self.participants
            .insert(ctx.resolve_identity(cell.lock_script_hash_id).to_vec());
        Ok(())
    }

    /// The first funding output the arbiter accepted: its pubkey hash,
    /// capacity, index and, when typed, UDT identity. Every error here is the
    /// indexer's own facts contradicting themselves, never the chain's bytes.
    fn record_funding_output(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        cell: &CellFacts,
        ctx: &ReducerContext<'a>,
        pubkey_hash: &[u8; 20],
    ) -> Result<()> {
        let capacity = u64::try_from(cell.capacity).map_err(|_| {
            anyhow!(
                "fiber funding output capacity is negative in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} capacity={}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(cell.outpoint.tx_hash),
                cell.outpoint.index,
                cell.capacity
            )
        })?;
        // UDT identity of the funding cell: its type script hash plus the
        // amount held in data, which the arbiter checked fits.
        self.funding_output_udt = match cell.type_code_hash_id {
            None => None,
            Some(_) => {
                let type_script_hash_id = cell.type_script_hash_id.ok_or_else(|| {
                    anyhow!(
                        "fiber funding output carries a type script with no type_script_hash in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{}",
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        hex::encode(cell.outpoint.tx_hash),
                        cell.outpoint.index
                    )
                })?;
                let amount = parse_funding_udt_amount(&cell.data).ok_or_else(|| {
                    anyhow!(
                        "fiber funding output accepted by the arbiter (data_size={}) but its data cannot hold a {}-byte UDT amount in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} data_len={}",
                        cell.data_size,
                        crate::parser::fiber::FUNDING_UDT_AMOUNT_LEN,
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        hex::encode(cell.outpoint.tx_hash),
                        cell.outpoint.index,
                        cell.data.len()
                    )
                })?;
                Some((ctx.resolve_identity(type_script_hash_id).to_vec(), amount))
            }
        };
        self.funding_output_pubkey_hash = Some(pubkey_hash.to_vec());
        self.funding_output_capacity = Some(capacity);
        self.funding_output_index = Some(cell.outpoint.index);
        Ok(())
    }
}

/// A funding-lock cell's data length for the arbiter. A negative stored size
/// is the indexer's own facts contradicting themselves.
fn fiber_cell_data_len(
    data_size: i32,
    tx: &ResolvedTxFacts<'_>,
    outpoint: &OutPointKey,
) -> Result<usize> {
    usize::try_from(data_size).map_err(|_| {
        anyhow!(
            "fiber funding-lock cell has a negative data_size in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} data_size={}",
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index,
            hex::encode(outpoint.tx_hash),
            outpoint.index,
            data_size
        )
    })
}

fn blake2b_hash(data: &[u8]) -> Vec<u8> {
    use ckb_hash::new_blake2b;

    let mut hasher = new_blake2b();
    hasher.update(data);
    let mut hash = vec![0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::CellSemanticTag;
    use crate::sync::bulk_build::interner::IdentityInterner;
    use crate::sync::types::InternId;

    // ═══════════════════════════════════════════════════════════════════
    // Real captured chain vectors (fetched from local mainnet/testnet CKB
    // nodes on 2026-08-03; hermetic constants, no network in tests).
    // ═══════════════════════════════════════════════════════════════════

    const SECP_CODE_HASH: &str =
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
    const USDI_CODE_HASH: &str =
        "0xcc9dc33ef234e14bc788c43a4848556a5fb16401a04662fc55db9bb201987037";
    const USDI_ARGS: &str = "0x71fd1985b2971a9903e4d8ed0d59e6710166985217ca0681437883837b86162f";
    const USDI_TYPE_SCRIPT_HASH: &str =
        "0x07ac97b5ff3df4b49f59a59f4d80d33d22c1263a57467c512c93b9c29b7a0de3";
    const USDI_OPEN_TX_HASH: &str =
        "0x4d49307f8d0572947e53bfbf35b06ce9c56a4affa43eef1a3ded311b67e28e4c";

    fn hex32(s: &str) -> [u8; 32] {
        let bytes = crate::rpc::parse_hex_to_bytes(s);
        bytes.as_slice().try_into().expect("32-byte hex")
    }

    fn hexv(s: &str) -> Vec<u8> {
        crate::rpc::parse_hex_to_bytes(s)
    }

    struct VectorBuilder {
        interner: IdentityInterner,
    }

    impl VectorBuilder {
        fn new() -> Self {
            Self {
                interner: IdentityInterner::default(),
            }
        }

        fn id(&self, bytes: Vec<u8>) -> InternId {
            self.interner.intern_bytes(bytes)
        }

        /// Build a CellFacts from a real captured cell. Script hashes are
        /// computed exactly as the parser does.
        #[allow(clippy::too_many_arguments)]
        fn cell(
            &self,
            outpoint_tx: [u8; 32],
            outpoint_index: u32,
            capacity: i64,
            lock_code_hash: &str,
            lock_args: &str,
            type_script: Option<(&str, &str)>,
            data: &[u8],
        ) -> CellFacts {
            let lock_code_hash = hexv(lock_code_hash);
            let lock_args = hexv(lock_args);
            let lock_script_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
                &lock_code_hash,
                1,
                &lock_args,
            );
            let (type_script_hash_id, type_code_hash_id, type_hash_type, type_args_id) =
                match type_script {
                    Some((code_hash, args)) => {
                        let code_hash = hexv(code_hash);
                        let args = hexv(args);
                        let hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
                            &code_hash, 1, &args,
                        );
                        (
                            Some(self.id(hash)),
                            Some(self.id(code_hash)),
                            Some(1),
                            Some(self.id(args)),
                        )
                    }
                    None => (None, None, None, None),
                };
            CellFacts {
                outpoint: OutPointKey::new(outpoint_tx, outpoint_index),
                created_at_block: 0,
                created_by_block_dao_ar: 1,
                capacity,
                lock_script_hash_id: self.id(lock_script_hash),
                lock_code_hash_id: self.id(lock_code_hash),
                lock_hash_type: 1,
                lock_args_id: self.id(lock_args),
                type_script_hash_id,
                type_code_hash_id,
                type_hash_type,
                type_args_id,
                occupied_capacity: 61_00000000,
                data_size: data.len() as i32,
                data: data.to_vec(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }
        }

        /// Build a ResolvedInputFacts from a real captured cell.
        fn input(
            &self,
            outpoint_tx: [u8; 32],
            outpoint_index: u32,
            capacity: i64,
            lock_code_hash: &str,
            lock_args: &str,
        ) -> ResolvedInputFacts {
            let lock_code_hash = hexv(lock_code_hash);
            let lock_args = hexv(lock_args);
            let lock_script_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
                &lock_code_hash,
                1,
                &lock_args,
            );
            ResolvedInputFacts {
                outpoint: OutPointKey::new(outpoint_tx, outpoint_index),
                created_at_block: 0,
                created_by_block_dao_ar: 1,
                capacity,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                lock_script_hash_id: self.id(lock_script_hash),
                lock_code_hash_id: self.id(lock_code_hash),
                lock_hash_type: 1,
                lock_args_id: self.id(lock_args),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                data_size: 0,
                data_hash: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }
        }
    }

    fn tx_facts<'a>(
        tx_hash: [u8; 32],
        block_number: i64,
        timestamp_ms: i64,
        tx_index: i32,
        resolved_inputs: Vec<ResolvedInputFacts>,
        cells: Vec<CellFacts>,
    ) -> ResolvedTxFacts<'a> {
        ResolvedTxFacts {
            tx_hash,
            block_number,
            block_hash: [0x99; 32],
            timestamp_ms,
            chain_time_ms: timestamp_ms,
            block_dao_ar: 1,
            tx_index,
            dotbit_action: None,
            resolved_inputs,
            cells: cells.into(),
        }
    }

    /// Bug 1 red test (audit B2), bulk path: applying the real USDI-funded
    /// open tx must record the funding output's UDT type script hash and
    /// 16-byte LE amount on the channel row.
    #[test]
    fn bulk_channel_open_udt_funded_real_vector_sets_udt_fields() {
        let b = VectorBuilder::new();
        let usdi = Some((USDI_CODE_HASH, USDI_ARGS));
        let open_tx = hex32(USDI_OPEN_TX_HASH);

        let inputs = vec![
            b.input(
                hex32("0x7f76488c606aadf8f9cae974336c9c81a97829618b7fccb14d4097bf4ca4a3c5"),
                2,
                9_981_999_999_031,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
            ),
            b.input(
                hex32("0x6ac8f0ad3c2408d63e2bbe756bd058c21f725f4970a6350a3d191ffb1215b6e1"),
                0,
                10_000_000_000_000,
                SECP_CODE_HASH,
                "0x2c71fb9e1c558782f6ca013fd7e8612d98990177",
            ),
        ];
        let cells = vec![
            b.cell(
                open_tx,
                0,
                36_000_000_000,
                crate::parser::fiber::FUNDING_LOCK_CODE_HASH_TESTNET,
                "0x00510ea5249c2b102ab35607ee04418ae47cb83b",
                usdi,
                &hexv("0x32ca9a3b000000000000000000000000"),
            ),
            // USDI change output — must NOT be mistaken for the funding UDT.
            b.cell(
                open_tx,
                1,
                14_200_000_000,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                usdi,
                &hexv("0x9cc99a3b000000000000000000000000"),
            ),
            b.cell(
                open_tx,
                2,
                9_963_999_998_062,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                None,
                &[],
            ),
        ];

        let frozen = b.interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = FiberOwner::default();
        owner
            .apply_tx(
                &tx_facts(open_tx, 20565583, 1_774_580_273_479, 1, inputs, cells),
                &ctx,
            )
            .unwrap();

        let channel_id = keys::encode_fiber_channel_id(&open_tx, 0);
        let channel = &owner.channels[channel_id.as_slice()];
        assert_eq!(channel.state, FiberChannelState::Open);
        assert_eq!(channel.capacity, 36_000_000_000);
        assert_eq!(channel.udt_type_hash, Some(hexv(USDI_TYPE_SCRIPT_HASH)));
        assert_eq!(channel.udt_amount, Some(1_000_000_050u128));
    }

    /// Live/bulk single-path proof for the USDI open vector: the channel row
    /// produced by the live chain (FiberDetector metadata → live writer) must
    /// equal the row produced by the bulk reducer, field for field.
    #[test]
    fn live_and_bulk_channel_open_rows_identical_for_udt_vector() {
        use crate::db::writer::activities::{
            build_tx_actions_for_block, InputCellView, OutputCellView, ProtocolDetector, TxView,
        };
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;

        let open_tx = hex32(USDI_OPEN_TX_HASH);
        let block_number = 20565583i64;
        let timestamp_ms = 1_774_580_273_479i64;

        // ── Bulk side ──────────────────────────────────────────────────
        let b = VectorBuilder::new();
        let usdi = Some((USDI_CODE_HASH, USDI_ARGS));
        let inputs = vec![
            b.input(
                hex32("0x7f76488c606aadf8f9cae974336c9c81a97829618b7fccb14d4097bf4ca4a3c5"),
                2,
                9_981_999_999_031,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
            ),
            b.input(
                hex32("0x6ac8f0ad3c2408d63e2bbe756bd058c21f725f4970a6350a3d191ffb1215b6e1"),
                0,
                10_000_000_000_000,
                SECP_CODE_HASH,
                "0x2c71fb9e1c558782f6ca013fd7e8612d98990177",
            ),
        ];
        let cells = vec![
            b.cell(
                open_tx,
                0,
                36_000_000_000,
                crate::parser::fiber::FUNDING_LOCK_CODE_HASH_TESTNET,
                "0x00510ea5249c2b102ab35607ee04418ae47cb83b",
                usdi,
                &hexv("0x32ca9a3b000000000000000000000000"),
            ),
            b.cell(
                open_tx,
                2,
                9_963_999_998_062,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                None,
                &[],
            ),
            b.cell(
                open_tx,
                3,
                9_996_199_999_787,
                SECP_CODE_HASH,
                "0x2c71fb9e1c558782f6ca013fd7e8612d98990177",
                None,
                &[],
            ),
        ];
        let frozen = b.interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = FiberOwner::default();
        owner
            .apply_tx(
                &tx_facts(open_tx, block_number, timestamp_ms, 1, inputs, cells),
                &ctx,
            )
            .unwrap();
        let channel_id = keys::encode_fiber_channel_id(&open_tx, 0);
        let bulk_channel = owner.channels[channel_id.as_slice()].clone();

        // ── Live side: detector → TxActions → live writer ──────────────
        let secp_code_hash = hexv(SECP_CODE_HASH);
        let funding_code_hash = hexv(crate::parser::fiber::FUNDING_LOCK_CODE_HASH_TESTNET);
        let usdi_code_hash = hexv(USDI_CODE_HASH);
        let usdi_args = hexv(USDI_ARGS);
        let usdi_type_hash = hexv(USDI_TYPE_SCRIPT_HASH);
        let funding_lock_args = hexv("0x00510ea5249c2b102ab35607ee04418ae47cb83b");
        let funding_lock_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &funding_code_hash,
            1,
            &funding_lock_args,
        );
        let secp_args_a = hexv("0xd2c9b058568578c884e108e3a82ee111af6a9f4b");
        let secp_args_b = hexv("0x2c71fb9e1c558782f6ca013fd7e8612d98990177");
        let secp_hash_a = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &secp_code_hash,
            1,
            &secp_args_a,
        );
        let secp_hash_b = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &secp_code_hash,
            1,
            &secp_args_b,
        );
        let prev_a = hexv("0x7f76488c606aadf8f9cae974336c9c81a97829618b7fccb14d4097bf4ca4a3c5");
        let prev_b = hexv("0x6ac8f0ad3c2408d63e2bbe756bd058c21f725f4970a6350a3d191ffb1215b6e1");
        let funding_data = hexv("0x32ca9a3b000000000000000000000000");

        let live_inputs = vec![
            InputCellView {
                previous_tx_hash: &prev_a,
                previous_output_index: 2,
                lock_script_hash: &secp_hash_a,
                lock_code_hash: &secp_code_hash,
                lock_hash_type: 1,
                lock_args: &secp_args_a,
                capacity: 9_981_999_999_031,
                occupied_capacity: 61_00000000,
                type_code_hash: None,
                type_hash_type: None,
                type_script_hash: None,
                type_args: None,
                udt_amount: None,
                bit_cell_identity_id: None,
                dotcell: None,
                data_size: 0,
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            },
            InputCellView {
                previous_tx_hash: &prev_b,
                previous_output_index: 0,
                lock_script_hash: &secp_hash_b,
                lock_code_hash: &secp_code_hash,
                lock_hash_type: 1,
                lock_args: &secp_args_b,
                capacity: 10_000_000_000_000,
                occupied_capacity: 61_00000000,
                type_code_hash: None,
                type_hash_type: None,
                type_script_hash: None,
                type_args: None,
                udt_amount: None,
                bit_cell_identity_id: None,
                dotcell: None,
                data_size: 0,
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            },
        ];
        let live_outputs = vec![
            OutputCellView {
                capacity: 36_000_000_000,
                lock_code_hash: &funding_code_hash,
                lock_hash_type: 1,
                lock_args: &funding_lock_args,
                lock_script_hash: &funding_lock_hash,
                type_code_hash: Some(&usdi_code_hash),
                type_hash_type: Some(1),
                type_args: Some(&usdi_args),
                type_script_hash: Some(&usdi_type_hash),
                data_hash: &[],
                data_size: 16,
                data: &funding_data,
            },
            OutputCellView {
                capacity: 9_963_999_998_062,
                lock_code_hash: &secp_code_hash,
                lock_hash_type: 1,
                lock_args: &secp_args_a,
                lock_script_hash: &secp_hash_a,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: &[],
                data_size: 0,
                data: &[],
            },
            OutputCellView {
                capacity: 9_996_199_999_787,
                lock_code_hash: &secp_code_hash,
                lock_hash_type: 1,
                lock_args: &secp_args_b,
                lock_script_hash: &secp_hash_b,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: &[],
                data_size: 0,
                data: &[],
            },
        ];
        let tx_view = TxView {
            tx_hash: &open_tx,
            block_hash: &[0x99; 32],
            tx_index: 1,
            block_number,
            timestamp: timestamp_ms,
            is_cellbase: false,
            dotbit_action: None,
            inputs: live_inputs,
            outputs: live_outputs,
        };
        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(
            crate::db::writer::fiber_detector::FiberDetector::new(false),
        )];
        let tx_actions_list = build_tx_actions_for_block(&[tx_view], &detectors).unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        for tx_actions in &tx_actions_list {
            crate::db::writer::fiber::process_fiber_channel_events(&mut batch, tx_actions).unwrap();
        }
        batch.commit().unwrap();
        let live_channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();

        assert_eq!(
            live_channel, bulk_channel,
            "live-path channel row must equal the bulk-path row"
        );
    }

    /// Bug 3 red test (audit B3): live/bulk force-close parity on the real
    /// mainnet vector. The chain: open 0x18e21924… (block 15676252) →
    /// force close 0x2bce4f4c… (block 15676315, all owners fiber locks).
    /// The live rows must equal the bulk rows after each step; the bulk-built
    /// DB row for this channel carries delayEpoch 11529216145580097542 and
    /// commitment output index 0.
    #[test]
    fn live_and_bulk_force_close_rows_identical_for_mainnet_vector() {
        use crate::db::writer::activities::{
            build_tx_actions_for_block, InputCellView, OutputCellView, ProtocolDetector, TxView,
        };
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;

        const FUNDING_ARGS: &str = "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef";
        const COMMITMENT_ARGS: &str =
            "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef06000000000100a00000000000000000";
        let open_tx = hex32("0x18e21924d3590e865473aa6e16be7a1d45c2b990d5519c03b82d371467783789");
        let fc_tx = hex32("0x2bce4f4cdd42a23386325ae204bbebfb94dba267dedbe80569e1553c2fedcc7f");

        // ── Bulk side: open then force close ───────────────────────────
        let b = VectorBuilder::new();
        let open_inputs = vec![
            b.input(
                hex32("0x9345967025b585df70efda45f325564988d1ddbb3b20d30d60aef08efa2f187c"),
                2,
                49_999_998_619,
                SECP_CODE_HASH,
                "0xa9631ecab784e46aba801e4871c57a092929a6fa",
            ),
            b.input(
                hex32("0x9345967025b585df70efda45f325564988d1ddbb3b20d30d60aef08efa2f187c"),
                1,
                89_999_999_545,
                SECP_CODE_HASH,
                "0xa9631ecab784e46aba801e4871c57a092929a6fa",
            ),
            b.input(
                hex32("0xbcba65190b578218b5f8d738b0f7b6a8aff9ea780b9b3d4a923da485161e4fc6"),
                2,
                143_799_999_649,
                SECP_CODE_HASH,
                "0x9c636e7c3f711fc3b6784b073eac7777742c3d8f",
            ),
        ];
        let open_cells = vec![
            b.cell(
                open_tx,
                0,
                106_200_000_000,
                crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET,
                FUNDING_ARGS,
                None,
                &[],
            ),
            b.cell(
                open_tx,
                1,
                39_999_997_543,
                SECP_CODE_HASH,
                "0xa9631ecab784e46aba801e4871c57a092929a6fa",
                None,
                &[],
            ),
            b.cell(
                open_tx,
                2,
                137_599_999_298,
                SECP_CODE_HASH,
                "0x9c636e7c3f711fc3b6784b073eac7777742c3d8f",
                None,
                &[],
            ),
        ];
        let fc_inputs = vec![b.input(
            open_tx,
            0,
            106_200_000_000,
            crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET,
            FUNDING_ARGS,
        )];
        let fc_cells = vec![b.cell(
            fc_tx,
            0,
            106_199_999_545,
            crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
            COMMITMENT_ARGS,
            None,
            &[],
        )];
        let frozen = b.interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = FiberOwner::default();
        owner
            .apply_tx(
                &tx_facts(
                    open_tx,
                    15676252,
                    1_742_364_647_196,
                    5,
                    open_inputs,
                    open_cells,
                ),
                &ctx,
            )
            .unwrap();
        owner
            .apply_tx(
                &tx_facts(fc_tx, 15676315, 1_742_365_295_267, 14, fc_inputs, fc_cells),
                &ctx,
            )
            .unwrap();
        let channel_id = keys::encode_fiber_channel_id(&open_tx, 0);
        let bulk_channel = owner.channels[channel_id.as_slice()].clone();
        // Bulk parity with the audited DB row for this channel:
        assert_eq!(bulk_channel.delay_epoch, Some(11529216145580097542u64));
        assert_eq!(bulk_channel.commitment_output_index, Some(0));
        // Real DB participants (audit evidence, channel 0x3cce5863…):
        assert_eq!(
            bulk_channel.participants,
            vec![
                hexv("0x5a76ef0a255f2703c47c697332bc0e8fefdc02ef58a7f874143ba7d79bd31548"),
                hexv("0x724f15b7e78b82b068f69d442a0993b549ef59d2f7a035066a6cc54847396bf6"),
            ]
        );

        // ── Live side: detector → TxActions → live writer, both txs ────
        let secp_code_hash = hexv(SECP_CODE_HASH);
        let funding_code_hash = hexv(crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET);
        let commitment_code_hash = hexv(crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET);
        let funding_args = hexv(FUNDING_ARGS);
        let commitment_args = hexv(COMMITMENT_ARGS);
        let funding_lock_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &funding_code_hash,
            1,
            &funding_args,
        );
        let commitment_lock_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &commitment_code_hash,
            1,
            &commitment_args,
        );
        let secp_args_a = hexv("0xa9631ecab784e46aba801e4871c57a092929a6fa");
        let secp_args_b = hexv("0x9c636e7c3f711fc3b6784b073eac7777742c3d8f");
        let secp_hash_a = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &secp_code_hash,
            1,
            &secp_args_a,
        );
        let secp_hash_b = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &secp_code_hash,
            1,
            &secp_args_b,
        );
        let prev_a = hexv("0x9345967025b585df70efda45f325564988d1ddbb3b20d30d60aef08efa2f187c");
        let prev_b = hexv("0xbcba65190b578218b5f8d738b0f7b6a8aff9ea780b9b3d4a923da485161e4fc6");

        fn secp_input_view<'a>(
            code_hash: &'a [u8],
            prev: &'a [u8],
            idx: u32,
            capacity: i64,
            hash: &'a [u8],
            args: &'a [u8],
        ) -> InputCellView<'a> {
            InputCellView {
                previous_tx_hash: prev,
                previous_output_index: idx,
                lock_script_hash: hash,
                lock_code_hash: code_hash,
                lock_hash_type: 1,
                lock_args: args,
                capacity,
                occupied_capacity: 61_00000000,
                type_code_hash: None,
                type_hash_type: None,
                type_script_hash: None,
                type_args: None,
                udt_amount: None,
                bit_cell_identity_id: None,
                dotcell: None,
                data_size: 0,
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            }
        }
        fn secp_output_view<'a>(
            code_hash: &'a [u8],
            capacity: i64,
            hash: &'a [u8],
            args: &'a [u8],
        ) -> OutputCellView<'a> {
            OutputCellView {
                capacity,
                lock_code_hash: code_hash,
                lock_hash_type: 1,
                lock_args: args,
                lock_script_hash: hash,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: &[],
                data_size: 0,
                data: &[],
            }
        }

        let open_view = TxView {
            tx_hash: &open_tx,
            block_hash: &[0x98; 32],
            tx_index: 5,
            block_number: 15676252,
            timestamp: 1_742_364_647_196,
            is_cellbase: false,
            dotbit_action: None,
            inputs: vec![
                secp_input_view(
                    &secp_code_hash,
                    &prev_a,
                    2,
                    49_999_998_619,
                    &secp_hash_a,
                    &secp_args_a,
                ),
                secp_input_view(
                    &secp_code_hash,
                    &prev_a,
                    1,
                    89_999_999_545,
                    &secp_hash_a,
                    &secp_args_a,
                ),
                secp_input_view(
                    &secp_code_hash,
                    &prev_b,
                    2,
                    143_799_999_649,
                    &secp_hash_b,
                    &secp_args_b,
                ),
            ],
            outputs: vec![
                OutputCellView {
                    capacity: 106_200_000_000,
                    lock_code_hash: &funding_code_hash,
                    lock_hash_type: 1,
                    lock_args: &funding_args,
                    lock_script_hash: &funding_lock_hash,
                    type_code_hash: None,
                    type_hash_type: None,
                    type_args: None,
                    type_script_hash: None,
                    data_hash: &[],
                    data_size: 0,
                    data: &[],
                },
                secp_output_view(&secp_code_hash, 39_999_997_543, &secp_hash_a, &secp_args_a),
                secp_output_view(&secp_code_hash, 137_599_999_298, &secp_hash_b, &secp_args_b),
            ],
        };
        let fc_view = TxView {
            tx_hash: &fc_tx,
            block_hash: &[0x97; 32],
            tx_index: 14,
            block_number: 15676315,
            timestamp: 1_742_365_295_267,
            is_cellbase: false,
            dotbit_action: None,
            inputs: vec![InputCellView {
                previous_tx_hash: &open_tx,
                previous_output_index: 0,
                lock_script_hash: &funding_lock_hash,
                lock_code_hash: &funding_code_hash,
                lock_hash_type: 1,
                lock_args: &funding_args,
                capacity: 106_200_000_000,
                occupied_capacity: 61_00000000,
                type_code_hash: None,
                type_hash_type: None,
                type_script_hash: None,
                type_args: None,
                udt_amount: None,
                bit_cell_identity_id: None,
                dotcell: None,
                data_size: 0,
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            }],
            outputs: vec![OutputCellView {
                capacity: 106_199_999_545,
                lock_code_hash: &commitment_code_hash,
                lock_hash_type: 1,
                lock_args: &commitment_args,
                lock_script_hash: &commitment_lock_hash,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: &[],
                data_size: 0,
                data: &[],
            }],
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(
            crate::db::writer::fiber_detector::FiberDetector::new(true),
        )];
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        for view in [open_view, fc_view] {
            let tx_actions_list = build_tx_actions_for_block(&[view], &detectors).unwrap();
            let mut batch = StoreBatch::new(&store);
            for tx_actions in &tx_actions_list {
                crate::db::writer::fiber::process_fiber_channel_events(&mut batch, tx_actions)
                    .unwrap();
            }
            batch.commit().unwrap();
        }
        let live_channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();

        assert_eq!(
            live_channel, bulk_channel,
            "live-path force-closed channel row must equal the bulk-path row"
        );
        // The commitment index must resolve identically too.
        let commitment_hash = blake2b_hash(&commitment_args);
        assert_eq!(
            store
                .get_fiber_channel_id_by_commitment(&commitment_hash)
                .unwrap()
                .unwrap(),
            channel_id
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Scenario harness: the same transactions through the bulk reducer and
    // through the live detector → writer, with every Fiber row both write.
    // ═══════════════════════════════════════════════════════════════════

    /// One cell of a scenario, as both sync paths see it.
    #[derive(Clone)]
    struct ScenarioCell {
        tx_hash: [u8; 32],
        index: u32,
        capacity: i64,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        /// `(code_hash, args)`, hash type `type`.
        type_script: Option<(Vec<u8>, Vec<u8>)>,
        data: Vec<u8>,
    }

    impl ScenarioCell {
        fn new(tx_hash: [u8; 32], index: u32, capacity: i64, code: &str, args: Vec<u8>) -> Self {
            Self {
                tx_hash,
                index,
                capacity,
                lock_code_hash: hexv(code),
                lock_args: args,
                type_script: None,
                data: Vec::new(),
            }
        }

        fn typed(mut self, code: &str, args: &str, data: Vec<u8>) -> Self {
            self.type_script = Some((hexv(code), hexv(args)));
            self.data = data;
            self
        }

        fn lock_script_hash(&self) -> Vec<u8> {
            crate::parser::script::ScriptParser::compute_script_hash_raw(
                &self.lock_code_hash,
                1,
                &self.lock_args,
            )
        }

        fn type_script_hash(&self) -> Option<Vec<u8>> {
            self.type_script.as_ref().map(|(code, args)| {
                crate::parser::script::ScriptParser::compute_script_hash_raw(code, 1, args)
            })
        }
    }

    struct ScenarioTx {
        hash: [u8; 32],
        block_number: i64,
        timestamp_ms: i64,
        tx_index: i32,
        inputs: Vec<ScenarioCell>,
        outputs: Vec<ScenarioCell>,
    }

    impl ScenarioTx {
        /// `outputs` given as (capacity, lock code hash, lock args); their
        /// out points are this transaction's.
        fn new(
            hash: [u8; 32],
            block_number: i64,
            inputs: Vec<ScenarioCell>,
            outputs: Vec<ScenarioCell>,
        ) -> Self {
            let outputs = outputs
                .into_iter()
                .enumerate()
                .map(|(index, mut cell)| {
                    cell.tx_hash = hash;
                    cell.index = index as u32;
                    cell
                })
                .collect();
            Self {
                hash,
                block_number,
                timestamp_ms: 1_700_000_000_000 + block_number * 1_000,
                tx_index: 1,
                inputs,
                outputs,
            }
        }

        fn output(&self, index: usize) -> ScenarioCell {
            self.outputs[index].clone()
        }
    }

    /// An output-to-be: its out point is set by `ScenarioTx::new`.
    fn out(capacity: i64, code: &str, args: Vec<u8>) -> ScenarioCell {
        ScenarioCell::new([0; 32], 0, capacity, code, args)
    }

    /// Every Fiber row of one sync path, decoded where the value is a struct.
    #[derive(Debug, PartialEq)]
    struct FiberRows {
        channels: Vec<(Vec<u8>, FiberChannel)>,
        by_commitment: Vec<(Vec<u8>, Vec<u8>)>,
        addr_channel_keys: Vec<Vec<u8>>,
    }

    impl VectorBuilder {
        fn scenario_cell_facts(&self, cell: &ScenarioCell) -> CellFacts {
            let (type_script_hash_id, type_code_hash_id, type_hash_type, type_args_id) =
                match &cell.type_script {
                    Some((code, args)) => (
                        Some(self.id(cell.type_script_hash().unwrap())),
                        Some(self.id(code.clone())),
                        Some(1),
                        Some(self.id(args.clone())),
                    ),
                    None => (None, None, None, None),
                };
            CellFacts {
                outpoint: OutPointKey::new(cell.tx_hash, cell.index),
                created_at_block: 0,
                created_by_block_dao_ar: 1,
                capacity: cell.capacity,
                lock_script_hash_id: self.id(cell.lock_script_hash()),
                lock_code_hash_id: self.id(cell.lock_code_hash.clone()),
                lock_hash_type: 1,
                lock_args_id: self.id(cell.lock_args.clone()),
                type_script_hash_id,
                type_code_hash_id,
                type_hash_type,
                type_args_id,
                occupied_capacity: 61_00000000,
                data_size: cell.data.len() as i32,
                data: cell.data.clone(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }
        }

        fn scenario_input_facts(&self, cell: &ScenarioCell) -> ResolvedInputFacts {
            let facts = self.scenario_cell_facts(cell);
            ResolvedInputFacts {
                outpoint: facts.outpoint,
                created_at_block: 0,
                created_by_block_dao_ar: 1,
                capacity: facts.capacity,
                occupied_capacity: facts.occupied_capacity,
                udt_amount: None,
                lock_script_hash_id: facts.lock_script_hash_id,
                lock_code_hash_id: facts.lock_code_hash_id,
                lock_hash_type: 1,
                lock_args_id: facts.lock_args_id,
                type_script_hash_id: facts.type_script_hash_id,
                type_code_hash_id: facts.type_code_hash_id,
                type_hash_type: facts.type_hash_type,
                type_args_id: facts.type_args_id,
                data_size: facts.data_size,
                data_hash: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }
        }
    }

    /// The scenario through the bulk reducer, one `apply_tx` per transaction.
    fn bulk_rows(txs: &[ScenarioTx]) -> Result<FiberRows> {
        let b = VectorBuilder::new();
        let facts: Vec<_> = txs
            .iter()
            .map(|tx| {
                (
                    tx,
                    tx.inputs
                        .iter()
                        .map(|c| b.scenario_input_facts(c))
                        .collect::<Vec<_>>(),
                    tx.outputs
                        .iter()
                        .map(|c| b.scenario_cell_facts(c))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let frozen = b.interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = FiberOwner::default();
        for (tx, inputs, cells) in facts {
            owner.apply_tx(
                &tx_facts(
                    tx.hash,
                    tx.block_number,
                    tx.timestamp_ms,
                    tx.tx_index,
                    inputs,
                    cells,
                ),
                &ctx,
            )?;
        }
        let mut addr_channel_keys: Vec<Vec<u8>> = owner
            .channels
            .iter()
            .flat_map(|(id, channel)| {
                channel
                    .participants
                    .iter()
                    .map(|participant| keys::encode_addr_fiber_channel_key(participant, id))
            })
            .collect();
        addr_channel_keys.sort();
        Ok(FiberRows {
            channels: owner.channels.into_iter().collect(),
            by_commitment: owner.channel_by_commitment.into_iter().collect(),
            addr_channel_keys,
        })
    }

    /// The scenario through the live detector and writer, one committed batch
    /// per transaction.
    fn live_rows(txs: &[ScenarioTx]) -> Result<FiberRows> {
        use crate::db::writer::activities::{
            build_tx_actions_for_block, InputCellView, OutputCellView, ProtocolDetector, TxView,
        };
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;

        struct Owned {
            lock_hash: Vec<u8>,
            type_hash: Option<Vec<u8>>,
        }
        let owned = |cells: &[ScenarioCell]| -> Vec<Owned> {
            cells
                .iter()
                .map(|c| Owned {
                    lock_hash: c.lock_script_hash(),
                    type_hash: c.type_script_hash(),
                })
                .collect()
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(
            crate::db::writer::fiber_detector::FiberDetector::new(true),
        )];
        let dir = tempfile::TempDir::new()?;
        let store = CkbadgerStore::open_domain(dir.path())?;
        for tx in txs {
            let input_owned = owned(&tx.inputs);
            let output_owned = owned(&tx.outputs);
            let view = TxView {
                tx_hash: &tx.hash,
                block_hash: &[0x99; 32],
                tx_index: tx.tx_index,
                block_number: tx.block_number,
                timestamp: tx.timestamp_ms,
                is_cellbase: false,
                dotbit_action: None,
                inputs: tx
                    .inputs
                    .iter()
                    .zip(&input_owned)
                    .map(|(c, o)| InputCellView {
                        previous_tx_hash: &c.tx_hash,
                        previous_output_index: c.index,
                        lock_script_hash: &o.lock_hash,
                        lock_code_hash: &c.lock_code_hash,
                        lock_hash_type: 1,
                        lock_args: &c.lock_args,
                        capacity: c.capacity,
                        occupied_capacity: 61_00000000,
                        type_code_hash: c.type_script.as_ref().map(|(code, _)| code.as_slice()),
                        type_hash_type: c.type_script.as_ref().map(|_| 1),
                        type_script_hash: o.type_hash.as_deref(),
                        type_args: c.type_script.as_ref().map(|(_, args)| args.as_slice()),
                        udt_amount: None,
                        bit_cell_identity_id: None,
                        dotcell: None,
                        data_size: c.data.len() as i32,
                        data: &c.data,
                        is_dao_withdraw_request: false,
                        dao_compensation: None,
                    })
                    .collect(),
                outputs: tx
                    .outputs
                    .iter()
                    .zip(&output_owned)
                    .map(|(c, o)| OutputCellView {
                        capacity: c.capacity,
                        lock_code_hash: &c.lock_code_hash,
                        lock_hash_type: 1,
                        lock_args: &c.lock_args,
                        lock_script_hash: &o.lock_hash,
                        type_code_hash: c.type_script.as_ref().map(|(code, _)| code.as_slice()),
                        type_hash_type: c.type_script.as_ref().map(|_| 1),
                        type_args: c.type_script.as_ref().map(|(_, args)| args.as_slice()),
                        type_script_hash: o.type_hash.as_deref(),
                        data_hash: &[],
                        data_size: c.data.len() as i32,
                        data: &c.data,
                    })
                    .collect(),
            };
            let tx_actions_list = build_tx_actions_for_block(&[view], &detectors)?;
            let mut batch = StoreBatch::new(&store);
            for tx_actions in &tx_actions_list {
                crate::db::writer::fiber::process_fiber_channel_events(&mut batch, tx_actions)?;
            }
            batch.commit()?;
        }
        let rows = |cf: &str| -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
            store
                .iterator_cf(store.cf(cf), rocksdb::IteratorMode::Start)
                .map(|row| {
                    let (key, value) = row?;
                    Ok((key.to_vec(), value.to_vec()))
                })
                .collect()
        };
        Ok(FiberRows {
            channels: store.list_fiber_channels(10_000, None, None)?,
            by_commitment: rows(CF_FIBER_CHANNEL_BY_COMMITMENT)?,
            addr_channel_keys: rows(CF_ADDR_FIBER_CHANNELS)?
                .into_iter()
                .map(|(key, _)| key)
                .collect(),
        })
    }

    /// Both paths must succeed and write identical Fiber rows.
    fn identical_rows(txs: &[ScenarioTx]) -> FiberRows {
        let bulk = bulk_rows(txs).expect("bulk reducer must index the scenario");
        let live = live_rows(txs).expect("live detector and writer must index the scenario");
        assert_eq!(bulk, live, "bulk and live Fiber rows must be identical");
        bulk
    }

    const ALICE_ARGS: &str = "0xd2c9b058568578c884e108e3a82ee111af6a9f4b";

    fn alice_cell(tx_hash: [u8; 32], index: u32, capacity: i64) -> ScenarioCell {
        ScenarioCell::new(tx_hash, index, capacity, SECP_CODE_HASH, hexv(ALICE_ARGS))
    }

    fn commitment_args(len: usize, features: Option<u8>) -> Vec<u8> {
        let mut args = vec![0x55; 20];
        args.extend_from_slice(&0xa000_0100_0000_0001u64.to_le_bytes());
        args.extend_from_slice(&1u64.to_be_bytes());
        args.resize(len.max(36), 0x66);
        args.truncate(len);
        if let Some(features) = features {
            args.push(features);
        }
        args
    }

    fn valid_open(hash: [u8; 32], block_number: i64) -> ScenarioTx {
        ScenarioTx::new(
            hash,
            block_number,
            vec![alice_cell([0x50; 32], 0, 1000_00000000)],
            vec![
                out(
                    500_00000000,
                    crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET,
                    vec![0x44; 20],
                ),
                out(499_00000000, SECP_CODE_HASH, hexv(ALICE_ARGS)),
            ],
        )
    }

    /// Review 45fd N1 (plan D9): a cell carrying the commitment-lock code hash
    /// with 10-byte args is an ordinary cell: no channel rows, the build goes
    /// on, and its lock is an ordinary participant of the transaction.
    #[test]
    fn commitment_output_with_10_byte_args_is_an_ordinary_cell() {
        let tx = ScenarioTx::new(
            [0x61; 32],
            100,
            vec![alice_cell([0x60; 32], 0, 100_00000000)],
            vec![
                out(
                    61_00000000,
                    crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
                    vec![0x0a; 10],
                ),
                out(38_00000000, SECP_CODE_HASH, hexv(ALICE_ARGS)),
            ],
        );
        let not_a_commitment = tx.output(0).lock_script_hash();
        let rows = identical_rows(std::slice::from_ref(&tx));
        assert!(rows.channels.is_empty() && rows.by_commitment.is_empty());

        let b = VectorBuilder::new();
        let inputs = tx
            .inputs
            .iter()
            .map(|c| b.scenario_input_facts(c))
            .collect();
        let cells = tx
            .outputs
            .iter()
            .map(|c| b.scenario_cell_facts(c))
            .collect();
        let frozen = b.interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let facts = tx_facts(tx.hash, tx.block_number, tx.timestamp_ms, 1, inputs, cells);
        let summary = FiberTxSummary::from_tx(&facts, &ctx).unwrap();
        assert!(!summary.has_commitment_output);
        assert!(summary.participants.contains(&not_a_commitment));
        assert_eq!(summary.classify_event(), None);
    }

    /// Review 45fd N1 (plan D9): a funding-lock cell with 5-byte args opens no
    /// channel; its lock is an ordinary participant.
    #[test]
    fn funding_output_with_5_byte_args_is_an_ordinary_cell() {
        let tx = ScenarioTx::new(
            [0x63; 32],
            100,
            vec![alice_cell([0x62; 32], 0, 100_00000000)],
            vec![
                out(
                    61_00000000,
                    crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET,
                    vec![0x05; 5],
                ),
                out(38_00000000, SECP_CODE_HASH, hexv(ALICE_ARGS)),
            ],
        );
        let not_a_funding = tx.output(0).lock_script_hash();
        let rows = identical_rows(std::slice::from_ref(&tx));
        assert!(rows.channels.is_empty());

        let b = VectorBuilder::new();
        let inputs = tx
            .inputs
            .iter()
            .map(|c| b.scenario_input_facts(c))
            .collect();
        let cells = tx
            .outputs
            .iter()
            .map(|c| b.scenario_cell_facts(c))
            .collect();
        let frozen = b.interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let facts = tx_facts(tx.hash, tx.block_number, tx.timestamp_ms, 1, inputs, cells);
        let summary = FiberTxSummary::from_tx(&facts, &ctx).unwrap();
        assert!(!summary.has_funding_output);
        assert!(summary.participants.contains(&not_a_funding));
        assert_eq!(summary.classify_event(), None);
    }

    /// Review 45fd N1 (plan D9; replaces `bulk_channel_open_udt_funding_short_data_bails`):
    /// a typed funding-lock cell whose data cannot hold the 16-byte amount is
    /// an ordinary cell, when created and when spent.
    #[test]
    fn typed_funding_output_with_8_byte_data_is_an_ordinary_cell() {
        let create = ScenarioTx::new(
            [0x65; 32],
            100,
            vec![alice_cell([0x64; 32], 0, 1000_00000000)],
            vec![
                out(
                    360_00000000,
                    crate::parser::fiber::FUNDING_LOCK_CODE_HASH_TESTNET,
                    hexv("0x00510ea5249c2b102ab35607ee04418ae47cb83b"),
                )
                .typed(USDI_CODE_HASH, USDI_ARGS, vec![0x08; 8]),
                out(639_00000000, SECP_CODE_HASH, hexv(ALICE_ARGS)),
            ],
        );
        let spend = ScenarioTx::new(
            [0x66; 32],
            101,
            vec![create.output(0)],
            vec![out(359_00000000, SECP_CODE_HASH, hexv(ALICE_ARGS)).typed(
                USDI_CODE_HASH,
                USDI_ARGS,
                vec![0x08; 8],
            )],
        );
        let rows = identical_rows(&[create, spend]);
        assert!(rows.channels.is_empty());
    }

    /// Review 45fd finding 8 + N1 (plan D9): a tracked channel's funding spent
    /// into a commitment-code-hash output whose args fit no deployed layout has
    /// no commitment output: it is a plain (cooperative) close, never a force
    /// close tracking that cell.
    #[test]
    fn funding_spent_into_malformed_commitment_output_is_a_cooperative_close() {
        let open_hash = [0x67; 32];
        for args in [
            commitment_args(10, None),
            commitment_args(40, None),
            commitment_args(57, Some(0x00)),
            commitment_args(57, Some(0x01))
                .into_iter()
                .chain([0x00])
                .collect(),
        ] {
            let open = valid_open(open_hash, 100);
            let close = ScenarioTx::new(
                [0x68; 32],
                101,
                vec![open.output(0)],
                vec![out(
                    499_99999000,
                    crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
                    args.clone(),
                )],
            );
            let rows = identical_rows(&[open, close]);
            assert_eq!(rows.channels.len(), 1, "args_len={}", args.len());
            let (id, channel) = &rows.channels[0];
            assert_eq!(id, &keys::encode_fiber_channel_id(&open_hash, 0));
            assert_eq!(
                channel.state,
                FiberChannelState::CooperativelyClosed,
                "args_len={}",
                args.len()
            );
            assert_eq!(channel.close_tx_hash, Some(vec![0x68; 32]));
            assert_eq!(channel.commitment_tx_hash, None);
            assert_eq!(channel.delay_epoch, None);
            assert!(rows.by_commitment.is_empty());
        }
    }

    /// The daric 56-byte layout (36-byte head + pending-HTLC hash) is a
    /// deployed commitment layout: a force close into it is tracked.
    #[test]
    fn force_close_into_56_byte_commitment_is_tracked() {
        let open_hash = [0x69; 32];
        let open = valid_open(open_hash, 100);
        let args = commitment_args(56, None);
        let close = ScenarioTx::new(
            [0x6a; 32],
            101,
            vec![open.output(0)],
            vec![out(
                499_99999000,
                crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
                args.clone(),
            )],
        );
        let rows = identical_rows(&[open, close]);
        let (id, channel) = &rows.channels[0];
        assert_eq!(channel.state, FiberChannelState::ForceClosed);
        assert_eq!(channel.commitment_tx_hash, Some(vec![0x6a; 32]));
        assert_eq!(channel.delay_epoch, Some(0xa000_0100_0000_0001));
        assert_eq!(rows.by_commitment, vec![(blake2b_hash(&args), id.clone())]);
    }

    /// A tracked commitment spent into payouts plus a commitment-code-hash
    /// output whose args fit no layout: that output is an ordinary cell, so
    /// the spend is the channel's settlement (the old reducer halted on it).
    #[test]
    fn settlement_with_an_extra_malformed_commitment_output_is_a_settlement() {
        let open_hash = [0x6b; 32];
        let open = valid_open(open_hash, 100);
        // 36-byte head + settlement hash + first-settlement flag 0x00.
        let mut tracked = commitment_args(56, None);
        tracked.push(0x00);
        let close = ScenarioTx::new(
            [0x6c; 32],
            101,
            vec![open.output(0)],
            vec![out(
                499_99999000,
                crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
                tracked.clone(),
            )],
        );
        assert_eq!(tracked.len(), 57);
        let settle = ScenarioTx::new(
            [0x6d; 32],
            102,
            vec![close.output(0)],
            vec![
                out(438_99998000, SECP_CODE_HASH, hexv(ALICE_ARGS)),
                out(
                    61_00000000,
                    crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
                    vec![0x0a; 10],
                ),
            ],
        );
        let rows = identical_rows(&[open, close, settle]);
        let (_, channel) = &rows.channels[0];
        assert_eq!(channel.state, FiberChannelState::Settled);
        assert_eq!(channel.settlement_tx_hash, Some(vec![0x6d; 32]));
        assert_eq!(channel.commitment_tx_hash, Some(vec![0x6c; 32]));
    }

    /// Real mainnet channel 0xe1a939e4… on the 0.7.0 commitment lock (58-byte
    /// v1 args, features byte 0x01), from node `get_transaction` responses
    /// captured 2026-11-10: open 0xc4693771… (block 20967540) → force close
    /// 0x263875f6… (20967551) → partial settlements deriving new commitment
    /// cells 0xed1fbf13… (20967931) and 0x255f83e7… (20967938) → final
    /// settlement 0x37bcfbd7… (20967943). Both paths must write the row the
    /// mainnet API serves for this channel.
    #[test]
    fn live_and_bulk_rows_identical_for_58_byte_v1_mainnet_vector() {
        const FUNDING_ARGS: &str = "0x5c494edc86cce3a4d5692364453193e6ed9c6954";
        const SECP_A: &str = "0x7f0a30a2da9ee266d6f901daa52605d855c04449";
        const SECP_B: &str = "0xe2d5f53fcb8fde1a17cc8f81bde5ef9a92bcf7fc";
        const COMMITMENT_FORCE_CLOSE: &str = "0x5c494edc86cce3a4d5692364453193e6ed9c695401000000000100a000000000000000023a019230a18033cd86431f5a8bd0188974a094710001";
        const COMMITMENT_ED1F: &str = "0x5c494edc86cce3a4d5692364453193e6ed9c695401000000000100a000000000000000023430dc58c3d22031b35419c4ec79e5996f2e24db0101";
        const COMMITMENT_255F: &str = "0x5c494edc86cce3a4d5692364453193e6ed9c695401000000000100a00000000000000002f4bb16e4d4b49bb983db6d5ec191cd860c06712b0101";
        let commitment = crate::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET;
        let open_hash = hex32("0xc46937718fdb92ad66feb99c64b676aa220ea896477d1a3a5dc14ddf17b6bfb9");
        let close_hash =
            hex32("0x263875f6184c3de6a09bd1457f4a40c123fb768c576a5fcd1d82dc0c8488a4b6");
        let ed1f_hash = hex32("0xed1fbf13dc59cf255e9b6728e45a96dce3aaeacab98d893c7b9ba2437d527abd");
        let c255f_hash =
            hex32("0x255f83e718cd6ab33be6a7a2581a82632853fc2fe89b75283815042d85f37fe9");
        let settle_hash =
            hex32("0x37bcfbd70d84f39b5c8f0bfe0744cb91477f5a75317119b9d00e590459def47f");
        for args in [COMMITMENT_FORCE_CLOSE, COMMITMENT_ED1F, COMMITMENT_255F] {
            assert_eq!(hexv(args).len(), 58);
        }

        let mut open = ScenarioTx::new(
            open_hash,
            20967540,
            vec![
                ScenarioCell::new(
                    hex32("0xee1d07e6a6e1e7aa4b1c5e2d2e1a6a12c030f3e2f35e26c9921d03b1741c17b3"),
                    0,
                    49_999_999_456,
                    SECP_CODE_HASH,
                    hexv(SECP_A),
                ),
                ScenarioCell::new(
                    hex32("0xfd1ff084b679a6dcf71b115c5744d62c2419c5ae9d4cb8a4c54855fad81d362d"),
                    1,
                    30_300_001_849,
                    SECP_CODE_HASH,
                    hexv(SECP_B),
                ),
            ],
            vec![
                out(
                    35_000_000_000,
                    crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET,
                    hexv(FUNDING_ARGS),
                ),
                out(24_999_998_992, SECP_CODE_HASH, hexv(SECP_A)),
                out(20_300_001_603, SECP_CODE_HASH, hexv(SECP_B)),
            ],
        );
        open.timestamp_ms = 1_794_219_736_030;
        let mut close = ScenarioTx::new(
            close_hash,
            20967551,
            vec![open.output(0)],
            vec![out(
                34_999_999_543,
                commitment,
                hexv(COMMITMENT_FORCE_CLOSE),
            )],
        );
        close.timestamp_ms = 1_794_219_855_039;
        let ed1f = ScenarioTx::new(
            ed1f_hash,
            20967931,
            vec![
                close.output(0),
                ScenarioCell::new(
                    hex32("0x84a6a8dd2e7cb5d76c7f9061951fadfb336a5de07e1d9f2fa56c34f2dbb39d5f"),
                    2,
                    49_999_967_954,
                    SECP_CODE_HASH,
                    hexv(SECP_B),
                ),
            ],
            vec![
                out(33_999_999_543, commitment, hexv(COMMITMENT_ED1F)),
                out(50_999_967_040, SECP_CODE_HASH, hexv(SECP_B)),
            ],
        );
        let c255f = ScenarioTx::new(
            c255f_hash,
            20967938,
            vec![
                ed1f.output(0),
                ScenarioCell::new(
                    hex32("0x00e9735c26998020df1e0a8272f2801c8b446880e329f744f550c10c874e6f6d"),
                    0,
                    49_999_999_456,
                    SECP_CODE_HASH,
                    hexv(SECP_A),
                ),
            ],
            vec![
                out(9_999_999_543, commitment, hexv(COMMITMENT_255F)),
                out(73_999_998_671, SECP_CODE_HASH, hexv(SECP_A)),
            ],
        );
        let mut settle = ScenarioTx::new(
            settle_hash,
            20967943,
            vec![c255f.output(0), open.output(2)],
            vec![out(30_300_000_508, SECP_CODE_HASH, hexv(SECP_B))],
        );
        settle.timestamp_ms = 1_794_225_002_832;

        let rows = identical_rows(&[open, close, ed1f, c255f, settle]);
        let channel_id = keys::encode_fiber_channel_id(&open_hash, 0);
        // The mainnet API's record of this channel (captured 2026-11-10).
        let expected = FiberChannel {
            funding_tx_hash: open_hash.to_vec(),
            funding_output_index: 0,
            state: FiberChannelState::Settled,
            capacity: 35_000_000_000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 20967540,
            open_timestamp: 1_794_219_736_030,
            close_tx_hash: Some(close_hash.to_vec()),
            close_block: Some(20967551),
            close_timestamp: Some(1_794_219_855_039),
            commitment_tx_hash: Some(c255f_hash.to_vec()),
            commitment_output_index: Some(0),
            delay_epoch: Some(11_529_216_145_580_097_537),
            settlement_tx_hash: Some(settle_hash.to_vec()),
            settlement_block: Some(20967943),
            settlement_timestamp: Some(1_794_225_002_832),
            participants: vec![
                hexv("0x9a040b1fa0257fb0abe427dcec79441e4dc277a6eed602a290b75acaa2d059e2"),
                hexv("0xc281bed43192de71ffdc38397919cfc4623463bb3e75e3a6a10aa96e90d1502a"),
            ],
            funding_lock_args: hexv(FUNDING_ARGS),
        };
        assert_eq!(rows.channels, vec![(channel_id.clone(), expected)]);
        assert_eq!(
            rows.by_commitment,
            vec![(blake2b_hash(&hexv(COMMITMENT_255F)), channel_id)]
        );
    }

    #[test]
    fn classify_event_channel_open() {
        let summary = FiberTxSummary {
            has_funding_output: true,
            ..Default::default()
        };
        assert_eq!(summary.classify_event(), Some(FiberEvent::ChannelOpen));
    }

    #[test]
    fn classify_event_channel_close() {
        let summary = FiberTxSummary {
            has_funding_input: true,
            ..Default::default()
        };
        assert_eq!(summary.classify_event(), Some(FiberEvent::ChannelClose));
    }

    #[test]
    fn classify_event_force_close() {
        let summary = FiberTxSummary {
            has_funding_input: true,
            has_commitment_output: true,
            ..Default::default()
        };
        assert_eq!(summary.classify_event(), Some(FiberEvent::ForceClose));
    }

    #[test]
    fn classify_event_settlement() {
        let summary = FiberTxSummary {
            has_commitment_input: true,
            ..Default::default()
        };
        assert_eq!(summary.classify_event(), Some(FiberEvent::Settlement));
    }

    #[test]
    fn classify_event_commitment_revocation() {
        let summary = FiberTxSummary {
            has_commitment_input: true,
            has_commitment_output: true,
            ..Default::default()
        };
        assert_eq!(
            summary.classify_event(),
            Some(FiberEvent::CommitmentRevocation)
        );
    }

    #[test]
    fn classify_event_force_close_takes_priority_over_revocation() {
        // If funding_input + commitment_input + commitment_output, it's ForceClose
        let summary = FiberTxSummary {
            has_funding_input: true,
            has_commitment_input: true,
            has_commitment_output: true,
            ..Default::default()
        };
        assert_eq!(summary.classify_event(), Some(FiberEvent::ForceClose));
    }

    #[test]
    fn classify_event_none_when_no_fiber_cells() {
        let summary = FiberTxSummary::default();
        assert_eq!(summary.classify_event(), None);
    }

    #[test]
    fn channel_open_stores_state() {
        let mut owner = FiberOwner::default();
        let funding_args = vec![0xbb; 20];
        let tx_hash = [0xaa; 32];
        let channel_id = keys::encode_fiber_channel_id(&tx_hash, 0);

        let channel = FiberChannel {
            funding_tx_hash: tx_hash.to_vec(),
            funding_output_index: 0,
            state: FiberChannelState::Open,
            capacity: 100_00000000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 100,
            open_timestamp: 1000,
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![vec![0x11; 32]],
            funding_lock_args: funding_args.clone(),
        };

        owner.channels.insert(channel_id.clone(), channel);

        assert_eq!(owner.channels.len(), 1);
        assert_eq!(
            owner.channels[channel_id.as_slice()].state,
            FiberChannelState::Open
        );
    }

    #[test]
    fn reused_funding_args_close_by_consumed_outpoint() {
        fn tx(tx_hash: [u8; 32], block_number: i64) -> ResolvedTxFacts<'static> {
            ResolvedTxFacts {
                tx_hash,
                block_number,
                block_hash: [0x99; 32],
                timestamp_ms: block_number * 1_000,
                chain_time_ms: block_number * 1_000,
                block_dao_ar: 0,
                tx_index: 1,
                dotbit_action: None,
                resolved_inputs: Vec::new(),
                cells: std::borrow::Cow::Borrowed(&[]),
            }
        }

        fn open_summary(funding_args: Vec<u8>) -> FiberTxSummary<'static> {
            FiberTxSummary {
                funding_output_pubkey_hash: Some(funding_args),
                funding_output_capacity: Some(500_00000000),
                funding_output_index: Some(0),
                participants: BTreeSet::from([vec![0xAA; 32]]),
                ..Default::default()
            }
        }

        let mut owner = FiberOwner::default();
        let first_funding_tx_hash = [0x10; 32];
        let second_funding_tx_hash = [0x11; 32];
        let reused_funding_args = vec![0xCC; 20];

        owner
            .handle_channel_open(
                &tx(first_funding_tx_hash, 100),
                &open_summary(reused_funding_args.clone()),
            )
            .unwrap();
        owner
            .handle_channel_open(
                &tx(second_funding_tx_hash, 101),
                &open_summary(reused_funding_args),
            )
            .unwrap();

        let close_summary = FiberTxSummary {
            funding_input_outpoint: Some(OutPointKey::new(first_funding_tx_hash, 0)),
            ..Default::default()
        };
        owner
            .handle_channel_close(&tx([0x20; 32], 102), &close_summary)
            .unwrap();

        let first_channel_id = keys::encode_fiber_channel_id(&first_funding_tx_hash, 0);
        let second_channel_id = keys::encode_fiber_channel_id(&second_funding_tx_hash, 0);
        assert_eq!(
            owner.channels[first_channel_id.as_slice()].state,
            FiberChannelState::CooperativelyClosed
        );
        assert_eq!(
            owner.channels[second_channel_id.as_slice()].state,
            FiberChannelState::Open
        );
    }

    #[test]
    fn estimated_bytes_increases_with_channels() {
        let mut owner = FiberOwner::default();
        let empty_bytes = owner.estimated_bytes();

        let channel_id = vec![0xaa; 36];
        let channel = FiberChannel {
            funding_tx_hash: vec![0xaa; 32],
            funding_output_index: 0,
            state: FiberChannelState::Open,
            capacity: 100_00000000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 100,
            open_timestamp: 1000,
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![vec![0x11; 32]],
            funding_lock_args: vec![0xbb; 20],
        };
        owner.channels.insert(channel_id, channel);

        assert!(owner.estimated_bytes() > empty_bytes);
    }

    #[test]
    fn commitment_revocation_rotates_hash() {
        let mut owner = FiberOwner::default();
        let funding_args = vec![0xbb; 20];
        let tx_hash = [0xaa; 32];
        let channel_id = keys::encode_fiber_channel_id(&tx_hash, 0);

        // Set up a force-closed channel with a commitment hash
        let old_commitment_args = vec![0xcc; 57];
        let old_hash = blake2b_hash(&old_commitment_args);

        let channel = FiberChannel {
            funding_tx_hash: tx_hash.to_vec(),
            funding_output_index: 0,
            state: FiberChannelState::ForceClosed,
            capacity: 100_00000000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 100,
            open_timestamp: 1000,
            close_tx_hash: Some(vec![0xdd; 32]),
            close_block: Some(200),
            close_timestamp: Some(2000),
            commitment_tx_hash: Some(vec![0xdd; 32]),
            commitment_output_index: Some(0),
            delay_epoch: Some(100),
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![vec![0x11; 32]],
            funding_lock_args: funding_args.clone(),
        };
        owner.channels.insert(channel_id.clone(), channel);
        owner
            .channel_by_commitment
            .insert(old_hash.clone(), channel_id.clone());

        // New commitment args (different settlement_hash field)
        let mut new_commitment_args = vec![0xcc; 36]; // same pubkey+delay+version
        new_commitment_args.extend_from_slice(&[0xee; 20]); // different settlement_hash
        new_commitment_args.push(0x01);
        let new_hash = blake2b_hash(&new_commitment_args);

        // Verify old hash maps to channel
        assert!(owner
            .channel_by_commitment
            .contains_key(old_hash.as_slice()));
        assert!(!owner
            .channel_by_commitment
            .contains_key(new_hash.as_slice()));

        // Simulate revocation: old_hash removed, new_hash inserted
        owner.channel_by_commitment.remove(old_hash.as_slice());
        owner
            .channel_by_commitment
            .insert(new_hash.clone(), channel_id.clone());

        // Channel state stays ForceClosed
        let ch = owner.channels.get(channel_id.as_slice()).unwrap();
        assert_eq!(ch.state, FiberChannelState::ForceClosed);

        // Old hash gone, new hash present
        assert!(!owner
            .channel_by_commitment
            .contains_key(old_hash.as_slice()));
        assert_eq!(
            owner
                .channel_by_commitment
                .get(new_hash.as_slice())
                .unwrap(),
            &channel_id
        );
    }

    #[test]
    fn commitment_revocation_chain_then_settlement() {
        // Verifies that multiple revocations followed by a settlement work correctly
        let mut owner = FiberOwner::default();
        let funding_args = vec![0xbb; 20];
        let tx_hash = [0xaa; 32];
        let channel_id = keys::encode_fiber_channel_id(&tx_hash, 0);

        // Force-closed channel
        let commitment_args_v1 = vec![0xc1; 57];
        let hash_v1 = blake2b_hash(&commitment_args_v1);

        let channel = FiberChannel {
            funding_tx_hash: tx_hash.to_vec(),
            funding_output_index: 0,
            state: FiberChannelState::ForceClosed,
            capacity: 100_00000000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 100,
            open_timestamp: 1000,
            close_tx_hash: Some(vec![0xdd; 32]),
            close_block: Some(200),
            close_timestamp: Some(2000),
            commitment_tx_hash: Some(vec![0xdd; 32]),
            commitment_output_index: Some(0),
            delay_epoch: Some(100),
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![vec![0x11; 32]],
            funding_lock_args: funding_args.clone(),
        };
        owner.channels.insert(channel_id.clone(), channel);
        owner
            .channel_by_commitment
            .insert(hash_v1.clone(), channel_id.clone());

        // Revocation 1: v1 → v2
        let commitment_args_v2 = vec![0xc2; 57];
        let hash_v2 = blake2b_hash(&commitment_args_v2);
        owner.channel_by_commitment.remove(hash_v1.as_slice());
        owner
            .channel_by_commitment
            .insert(hash_v2.clone(), channel_id.clone());

        // Revocation 2: v2 → v3
        let commitment_args_v3 = vec![0xc3; 57];
        let hash_v3 = blake2b_hash(&commitment_args_v3);
        owner.channel_by_commitment.remove(hash_v2.as_slice());
        owner
            .channel_by_commitment
            .insert(hash_v3.clone(), channel_id.clone());

        // Channel should still be ForceClosed
        assert_eq!(
            owner.channels[channel_id.as_slice()].state,
            FiberChannelState::ForceClosed
        );

        // Only v3 hash should exist
        assert!(!owner.channel_by_commitment.contains_key(hash_v1.as_slice()));
        assert!(!owner.channel_by_commitment.contains_key(hash_v2.as_slice()));
        assert_eq!(
            owner.channel_by_commitment.get(hash_v3.as_slice()).unwrap(),
            &channel_id
        );

        // Settlement using v3 hash should find the channel
        let settled_channel = owner.channels.get_mut(channel_id.as_slice()).unwrap();
        settled_channel.state = FiberChannelState::Settled;
        assert_eq!(
            owner.channels[channel_id.as_slice()].state,
            FiberChannelState::Settled
        );
    }
}
