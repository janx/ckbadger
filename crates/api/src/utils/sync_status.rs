use anyhow::{bail, Result};
use ckbadger_store::types::{DeepForkInfo, SyncStatus};

pub(crate) fn deep_fork_info_if_consistent(status: &SyncStatus) -> Result<Option<&DeepForkInfo>> {
    match (status.deep_fork_detected, status.deep_fork_info.as_ref()) {
        (false, None) => Ok(None),
        (true, Some(info)) => Ok(Some(info)),
        (true, None) => bail!(
            "sync_status invariant violated: deep_fork_detected=true but deep_fork_info is missing (tip_block={})",
            status.tip_block_number
        ),
        (false, Some(_)) => bail!(
            "sync_status invariant violated: deep_fork_detected=false but deep_fork_info exists (tip_block={})",
            status.tip_block_number
        ),
    }
}
