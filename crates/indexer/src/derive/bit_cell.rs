//! L3 (J3): `bit_cell_by_account` — the live `.bit Cell` items that name
//! each `.bit` account, so a `.bit` identity can link to them.
//!
//! Source rows: `identity_data` entries with standard `.bit Cell`. A live one
//! contributes `[1]` to `account_id(20) + identity_id(32)`; a consumed one
//! contributes nothing. The live writer (`insert_bit_cell`, the identity branch
//! of `consume_spore`) reports each entry transition with its block; the
//! delta `f(new) - f(old)` removes the old account's row and adds the new
//! one's, so a `.bit Cell` re-created with data naming another account moves
//! between accounts exactly.

use anyhow::{anyhow, ensure, Result};
use ckbadger_store::derive_ops::{
    bit_cell_by_account, DerivedAggregateId, ReadSnapshot, BIT_CELL_BY_ACCOUNT,
};
use ckbadger_store::types::{IdentityEntry, IdentityExtra, IdentityStandard};
use ckbadger_store::CF_IDENTITY_DATA;

use super::{
    add_delta, resume_key, BlockDeltas, ChunkBudget, DeltaMap, DerivedAggregate, LiveDeriveCtx,
};

pub struct BitCellByAccount;

/// What one identity entry contributes: a live `.bit Cell` names its account.
pub(crate) fn bit_cell_contribution(
    identity_id: &[u8],
    entry: &IdentityEntry,
) -> Result<Option<Vec<u8>>> {
    if entry.standard != IdentityStandard::BitCell || !entry.is_live {
        return Ok(None);
    }
    let IdentityExtra::BitCell { account_id, .. } = &entry.extra else {
        return Err(anyhow!(
            ".bit Cell identity 0x{} carries no .bit Cell metadata",
            hex::encode(identity_id)
        ));
    };
    Ok(Some(bit_cell_by_account::key(account_id, identity_id)?))
}

impl DerivedAggregate for BitCellByAccount {
    fn id(&self) -> DerivedAggregateId {
        BIT_CELL_BY_ACCOUNT
    }

    fn rebuild_chunk(
        &self,
        snapshot: &ReadSnapshot<'_>,
        cursor: Option<&[u8]>,
        budget: &mut ChunkBudget,
        out: &mut DeltaMap,
    ) -> Result<Option<Vec<u8>>> {
        let start = resume_key(cursor);
        for item in snapshot.iter_from(CF_IDENTITY_DATA, &start)? {
            let (identity_id, value) = item.map_err(|e| anyhow!("iterating identity_data: {e}"))?;
            let entry: IdentityEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to decode identity 0x{}: {e}",
                    hex::encode(&identity_id)
                )
            })?;
            if let Some(key) = bit_cell_contribution(&identity_id, &entry)? {
                add_delta(out, key, &[1])?;
            }
            if budget.tick() {
                return Ok(Some(identity_id.to_vec()));
            }
        }
        Ok(None)
    }

    fn live_deltas(&self, ctx: &LiveDeriveCtx<'_>, out: &mut BlockDeltas) -> Result<()> {
        for transition in ctx.identity_transitions {
            let block = out.entry(transition.block).or_default();
            if let Some(old) = &transition.old {
                if let Some(key) = bit_cell_contribution(&transition.identity_id, old)? {
                    add_delta(block, key, &[-1])?;
                }
            }
            if let Some(key) = bit_cell_contribution(&transition.identity_id, &transition.new)? {
                add_delta(block, key, &[1])?;
            }
        }
        Ok(())
    }

    fn check_row(&self, key: &[u8], value: &[i128]) -> Result<()> {
        ensure!(
            key.len() == bit_cell_by_account::KEY_LEN,
            "bit_cell_by_account key has {} bytes",
            key.len()
        );
        ensure!(
            value == [1],
            "a live .bit Cell names its account exactly once, got {value:?}"
        );
        Ok(())
    }
}
