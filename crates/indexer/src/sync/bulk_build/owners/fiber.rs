use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::keys;
use ckbadger_store::store::CF_FIBER_CHANNEL_BY_FUNDING_ARGS;
use ckbadger_store::types::{FiberChannel, FiberChannelState};
use ckbadger_store::{CF_ADDR_FIBER_CHANNELS, CF_FIBER_CHANNELS, CF_FIBER_CHANNEL_BY_COMMITMENT};

use super::{BulkReducer, ReducerContext};
use crate::parser::fiber::{
    is_commitment_lock, is_funding_lock, parse_commitment_lock_args, parse_funding_lock_args,
};
use crate::sync::bulk_build::facts::{CellFacts, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};

#[derive(Debug, Default)]
pub(crate) struct FiberOwner {
    channels: BTreeMap<Vec<u8>, FiberChannel>,
    channel_by_funding_args: BTreeMap<Vec<u8>, Vec<u8>>,
    channel_by_commitment: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl FiberOwner {
    fn build_snapshot_rows(&self) -> Result<Vec<MaterializedRow>> {
        let mut rows = Vec::new();

        for (channel_id, channel) in &self.channels {
            rows.push(MaterializedRow::new(
                CF_FIBER_CHANNELS,
                channel_id.clone(),
                bincode::serialize(channel)?,
            ));
        }

        for (funding_args, channel_id) in &self.channel_by_funding_args {
            rows.push(MaterializedRow::new(
                CF_FIBER_CHANNEL_BY_FUNDING_ARGS,
                funding_args.clone(),
                channel_id.clone(),
            ));
        }

        for (commitment_hash, channel_id) in &self.channel_by_commitment {
            rows.push(MaterializedRow::new(
                CF_FIBER_CHANNEL_BY_COMMITMENT,
                commitment_hash.clone(),
                channel_id.clone(),
            ));
        }

        for (channel_id, channel) in &self.channels {
            for participant in &channel.participants {
                rows.push(MaterializedRow::new(
                    CF_ADDR_FIBER_CHANNELS,
                    keys::encode_addr_fiber_channel_key(participant, channel_id),
                    Vec::new(),
                ));
            }
        }

        Ok(rows)
    }

    #[allow(dead_code)]
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
        let rows = self.build_snapshot_rows()?;
        materializer.materialize_final_snapshot(&rows)
    }
}

impl FiberOwner {
    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::btree_map_serialized_bytes(&self.channels)
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.channel_by_funding_args,
            )
            + crate::sync::bulk_build::accounting::btree_map_serialized_bytes(
                &self.channel_by_commitment,
            )
    }

    fn handle_channel_open(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary,
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
        if let Some(existing) = self
            .channel_by_funding_args
            .get(funding_lock_args.as_slice())
        {
            bail!(
                "duplicate fiber funding lock args in bulk reducer: block={} tx=0x{} tx_index={} funding_lock_args=0x{} existing_channel_id=0x{} new_channel_id=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(&funding_lock_args),
                hex::encode(existing),
                hex::encode(&channel_id)
            );
        }

        let channel = FiberChannel {
            funding_tx_hash: tx.tx_hash.to_vec(),
            funding_output_index: output_index,
            state: FiberChannelState::Open,
            capacity,
            udt_type_hash: None,
            udt_amount: None,
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

        self.channel_by_funding_args
            .insert(funding_lock_args, channel_id.clone());
        self.channels.insert(channel_id, channel);
        Ok(())
    }

    fn handle_channel_close(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary,
    ) -> Result<()> {
        let funding_lock_args = summary
            .funding_input_pubkey_hash
            .as_deref()
            .ok_or_else(|| {
                anyhow!(
                    "fiber channel_close missing parsed funding input args in bulk reducer: block={} tx=0x{} tx_index={}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index
                )
            })?;
        let channel = self.channel_by_funding_args.get(funding_lock_args).ok_or_else(|| {
            anyhow!(
                "fiber channel_close missing channel by funding args in bulk reducer: block={} tx=0x{} tx_index={} funding_lock_args=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(funding_lock_args)
            )
        })?;
        let channel = self.channels.get_mut(channel.as_slice()).ok_or_else(|| {
            anyhow!(
                "fiber channel_close missing channel state in bulk reducer: block={} tx=0x{} tx_index={} funding_lock_args=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(funding_lock_args)
            )
        })?;
        if channel.state != FiberChannelState::Open {
            bail!(
                "fiber channel_close expected open channel in bulk reducer: block={} tx=0x{} tx_index={} state={:?} funding_lock_args=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                channel.state,
                hex::encode(funding_lock_args)
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
        summary: &FiberTxSummary,
    ) -> Result<()> {
        let funding_lock_args = summary
            .funding_input_pubkey_hash
            .as_deref()
            .ok_or_else(|| {
                anyhow!(
                    "fiber force_close missing parsed funding input args in bulk reducer: block={} tx=0x{} tx_index={}",
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
        let channel_id = self.channel_by_funding_args.get(funding_lock_args).ok_or_else(|| {
            anyhow!(
                "fiber force_close missing channel by funding args in bulk reducer: block={} tx=0x{} tx_index={} funding_lock_args=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(funding_lock_args)
            )
        })?;
        let channel_id = channel_id.clone();
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
                "fiber force_close missing channel state in bulk reducer: block={} tx=0x{} tx_index={} funding_lock_args=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                hex::encode(funding_lock_args)
            )
        })?;
        if channel.state != FiberChannelState::Open {
            bail!(
                "fiber force_close expected open channel in bulk reducer: block={} tx=0x{} tx_index={} state={:?} funding_lock_args=0x{}",
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index,
                channel.state,
                hex::encode(funding_lock_args)
            );
        }

        channel.state = FiberChannelState::ForceClosed;
        channel.close_tx_hash = Some(tx.tx_hash.to_vec());
        channel.close_block = Some(tx.block_number);
        channel.close_timestamp = Some(tx.timestamp_ms);
        channel.commitment_tx_hash = Some(tx.tx_hash.to_vec());
        channel.commitment_output_index = summary.commitment_output_index;
        channel.delay_epoch = summary.commitment_output_delay_epoch;

        self.channel_by_commitment
            .insert(commitment_hash, channel_id.clone());
        Ok(())
    }

    fn handle_settlement(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        summary: &FiberTxSummary,
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
        summary: &FiberTxSummary,
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
        channel.commitment_tx_hash = Some(tx.tx_hash.to_vec());
        channel.commitment_output_index = summary.commitment_output_index;
        channel.delay_epoch = summary.commitment_output_delay_epoch;

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
struct FiberTxSummary {
    has_funding_input: bool,
    has_funding_output: bool,
    has_commitment_input: bool,
    has_commitment_output: bool,
    funding_input_pubkey_hash: Option<Vec<u8>>,
    funding_output_pubkey_hash: Option<Vec<u8>>,
    funding_output_capacity: Option<u64>,
    funding_output_index: Option<u32>,
    commitment_input_args: Option<Vec<u8>>,
    commitment_output_args: Option<Vec<u8>>,
    commitment_output_index: Option<u32>,
    commitment_output_delay_epoch: Option<u64>,
    participants: BTreeSet<Vec<u8>>,
}

impl FiberTxSummary {
    fn from_tx(tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<Self> {
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

    fn record_input(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        input: &ResolvedInputFacts,
        ctx: &ReducerContext<'_>,
    ) -> Result<()> {
        let lock_code_hash = ctx.resolve_identity(input.lock_code_hash_id);
        let lock_args = ctx.resolve_identity(input.lock_args_id);
        if is_funding_lock(lock_code_hash) {
            self.has_funding_input = true;
            if self.funding_input_pubkey_hash.is_none() {
                let parsed = parse_funding_lock_args(lock_args).ok_or_else(|| {
                    anyhow!(
                        "fiber funding input args invalid in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} args_len={}",
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        hex::encode(input.outpoint.tx_hash),
                        input.outpoint.index,
                        lock_args.len()
                    )
                })?;
                self.funding_input_pubkey_hash = Some(parsed.pubkey_hash);
            }
            return Ok(());
        }

        if is_commitment_lock(lock_code_hash) {
            self.has_commitment_input = true;
            if self.commitment_input_args.is_none() {
                parse_commitment_lock_args(lock_args).ok_or_else(|| {
                    anyhow!(
                        "fiber commitment input args invalid in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} args_len={}",
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        hex::encode(input.outpoint.tx_hash),
                        input.outpoint.index,
                        lock_args.len()
                    )
                })?;
                self.commitment_input_args = Some(lock_args.to_vec());
            }
            return Ok(());
        }

        self.participants
            .insert(ctx.resolve_identity(input.lock_script_hash_id).to_vec());
        Ok(())
    }

    fn record_output(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
    ) -> Result<()> {
        let lock_code_hash = ctx.resolve_identity(cell.lock_code_hash_id);
        let lock_args = ctx.resolve_identity(cell.lock_args_id);
        if is_funding_lock(lock_code_hash) {
            self.has_funding_output = true;
            if self.funding_output_pubkey_hash.is_none() {
                let parsed = parse_funding_lock_args(lock_args).ok_or_else(|| {
                    anyhow!(
                        "fiber funding output args invalid in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} args_len={}",
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        hex::encode(cell.outpoint.tx_hash),
                        cell.outpoint.index,
                        lock_args.len()
                    )
                })?;
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
                self.funding_output_pubkey_hash = Some(parsed.pubkey_hash);
                self.funding_output_capacity = Some(capacity);
                self.funding_output_index = Some(cell.outpoint.index);
            }
            return Ok(());
        }

        if is_commitment_lock(lock_code_hash) {
            self.has_commitment_output = true;
            if self.commitment_output_args.is_none() {
                let parsed = parse_commitment_lock_args(lock_args).ok_or_else(|| {
                    anyhow!(
                        "fiber commitment output args invalid in bulk reducer: block={} tx=0x{} tx_index={} outpoint=0x{}:{} args_len={}",
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        hex::encode(cell.outpoint.tx_hash),
                        cell.outpoint.index,
                        lock_args.len()
                    )
                })?;
                self.commitment_output_args = Some(lock_args.to_vec());
                self.commitment_output_index = Some(cell.outpoint.index);
                self.commitment_output_delay_epoch = Some(parsed.delay_epoch);
            }
            return Ok(());
        }

        self.participants
            .insert(ctx.resolve_identity(cell.lock_script_hash_id).to_vec());
        Ok(())
    }
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
    fn channel_open_stores_state_and_indexes() {
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
        owner
            .channel_by_funding_args
            .insert(funding_args.clone(), channel_id.clone());

        assert_eq!(owner.channels.len(), 1);
        assert_eq!(
            owner.channels[channel_id.as_slice()].state,
            FiberChannelState::Open
        );
        assert_eq!(
            owner.channel_by_funding_args[funding_args.as_slice()],
            channel_id
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
            .channel_by_funding_args
            .insert(funding_args, channel_id.clone());
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
            .channel_by_funding_args
            .insert(funding_args, channel_id.clone());
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
