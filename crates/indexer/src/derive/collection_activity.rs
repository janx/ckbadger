//! L2 (J5): `object_collection_activity_count` — the number of
//! `object_collection_activities` rows per padded collection id, so a Spore
//! cluster's `activitiesCount` is one point read instead of a walk of every
//! activity row.
//!
//! Source rows: `object_collection_activities` (key = collection_id(32,
//! padded) + block_desc + tx_desc + block_hash + tx_hash). Each row
//! contributes `[1]` to its collection's key. The rows are written by the
//! live writer's `ObjectCollectionActivityAccumulator::flush` (one row per
//! collection and transaction) and deleted only by rollback, which the undo
//! log of this aggregate mirrors. The cluster aggregate row (`cluster_agg`)
//! is bulk-written and stays byte-identical; this count lives beside it.

use anyhow::{anyhow, ensure, Result};
use ckbadger_store::derive_ops::{
    DerivedAggregateId, ReadSnapshot, OBJECT_COLLECTION_ACTIVITY_COUNT,
};
use ckbadger_store::{keys, CF_OBJECT_COLLECTION_ACTIVITIES};

use super::{
    add_delta, resume_key, BlockDeltas, ChunkBudget, DeltaMap, DerivedAggregate, LiveDeriveCtx,
};

pub struct ObjectCollectionActivityCount;

impl DerivedAggregate for ObjectCollectionActivityCount {
    fn id(&self) -> DerivedAggregateId {
        OBJECT_COLLECTION_ACTIVITY_COUNT
    }

    fn rebuild_chunk(
        &self,
        snapshot: &ReadSnapshot<'_>,
        cursor: Option<&[u8]>,
        budget: &mut ChunkBudget,
        out: &mut DeltaMap,
    ) -> Result<Option<Vec<u8>>> {
        let start = resume_key(cursor);
        for item in snapshot.iter_from(CF_OBJECT_COLLECTION_ACTIVITIES, &start)? {
            let (key, _) =
                item.map_err(|e| anyhow!("iterating object_collection_activities: {e}"))?;
            ensure!(
                key.len() == keys::OBJECT_COLLECTION_ACTIVITY_KEY_SIZE,
                "object_collection_activities key has {} bytes: 0x{}",
                key.len(),
                hex::encode(&key)
            );
            add_delta(out, key[..32].to_vec(), &[1])?;
            if budget.tick() {
                return Ok(Some(key.to_vec()));
            }
        }
        Ok(None)
    }

    fn live_deltas(&self, ctx: &LiveDeriveCtx<'_>, out: &mut BlockDeltas) -> Result<()> {
        for (collection_id, block) in ctx.object_collection_activity_rows {
            add_delta(
                out.entry(*block).or_default(),
                keys::encode_object_collection_activity_prefix(collection_id).to_vec(),
                &[1],
            )?;
        }
        Ok(())
    }

    fn check_row(&self, key: &[u8], value: &[i128]) -> Result<()> {
        ensure!(key.len() == 32, "collection key has {} bytes", key.len());
        ensure!(
            value.len() == 1 && value[0] > 0,
            "expected [activity rows > 0], got {value:?}"
        );
        Ok(())
    }
}
