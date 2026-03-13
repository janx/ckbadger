//! Pending proposal operations for the transaction pipeline visualization.

use crate::store::CkbadgerStore;
use ckbadger_common::CachedProposal;
use rocksdb::IteratorMode;

impl CkbadgerStore {
    /// Write a pending proposal to the store. Overwrites if proposal_id already exists.
    pub fn put_pending_proposal(&self, proposal: &CachedProposal) -> anyhow::Result<()> {
        let key = proposal.proposal_id.as_bytes();
        let value = serde_json::to_vec(proposal)?;
        self.put_cf(self.cf_pending_proposals(), key, &value)
    }

    /// Delete a pending proposal by its hex proposal ID (e.g., when committed).
    pub fn delete_pending_proposal(&self, proposal_id: &str) -> anyhow::Result<()> {
        self.delete_cf(self.cf_pending_proposals(), proposal_id.as_bytes())
    }

    /// Get all pending proposals from the store.
    pub fn get_all_pending_proposals(&self) -> anyhow::Result<Vec<CachedProposal>> {
        let iter = self.iterator_cf(self.cf_pending_proposals(), IteratorMode::Start);
        let mut proposals = Vec::new();
        for item in iter {
            let (_, value) = item?;
            let p = serde_json::from_slice::<CachedProposal>(&value)
                .map_err(|e| anyhow::anyhow!("failed to deserialize pending proposal: {}", e))?;
            proposals.push(p);
        }
        Ok(proposals)
    }

    /// Delete all proposals that have expired (tip > proposed_at + WINDOW_FARTHEST).
    /// Returns the number of deleted entries.
    pub fn delete_expired_proposals(&self, current_tip: i64) -> anyhow::Result<usize> {
        let iter = self.iterator_cf(self.cf_pending_proposals(), IteratorMode::Start);
        let mut to_delete = Vec::new();
        for item in iter {
            let (key, value) = item?;
            let p = serde_json::from_slice::<CachedProposal>(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize pending proposal during cleanup: {}",
                    e
                )
            })?;
            if p.is_expired(current_tip) {
                to_delete.push(key.to_vec());
            }
        }
        let count = to_delete.len();
        for key in &to_delete {
            self.delete_cf(self.cf_pending_proposals(), key)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use ckbadger_common::CachedProposal;

    use crate::store::CkbadgerStore;

    fn open_test_store() -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_put_and_get_all_pending_proposals() {
        let (_dir, store) = open_test_store();
        let p1 = CachedProposal::new_minimal("abcdef1234abcdef1234".to_string(), 100, 0);
        let p2 = CachedProposal::new_minimal("1234567890abcdef1234".to_string(), 100, 1);
        store.put_pending_proposal(&p1).unwrap();
        store.put_pending_proposal(&p2).unwrap();
        let all = store.get_all_pending_proposals().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_delete_pending_proposal() {
        let (_dir, store) = open_test_store();
        let p1 = CachedProposal::new_minimal("abcdef1234abcdef1234".to_string(), 100, 0);
        let p2 = CachedProposal::new_minimal("1234567890abcdef1234".to_string(), 101, 1);
        store.put_pending_proposal(&p1).unwrap();
        store.put_pending_proposal(&p2).unwrap();
        store
            .delete_pending_proposal("abcdef1234abcdef1234")
            .unwrap();
        let all = store.get_all_pending_proposals().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].proposal_id, "1234567890abcdef1234");
    }

    #[test]
    fn test_delete_expired_proposals() {
        let (_dir, store) = open_test_store();
        let old = CachedProposal::new_minimal("oldproposal123456789".to_string(), 50, 0);
        let recent = CachedProposal::new_minimal("newproposal123456789".to_string(), 1000, 0);
        store.put_pending_proposal(&old).unwrap();
        store.put_pending_proposal(&recent).unwrap();
        let deleted = store.delete_expired_proposals(100).unwrap();
        assert_eq!(deleted, 1);
        let all = store.get_all_pending_proposals().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].proposal_id, "newproposal123456789");
    }

    #[test]
    fn test_put_overwrites_existing_proposal() {
        let (_dir, store) = open_test_store();
        let p1 = CachedProposal::new_minimal("abcdef1234abcdef1234".to_string(), 100, 0);
        store.put_pending_proposal(&p1).unwrap();
        let p1_enriched = CachedProposal::new_with_details(
            "abcdef1234abcdef1234".to_string(),
            "0xfullhash".to_string(),
            100,
            0,
            5000,
            200,
            10000,
        );
        store.put_pending_proposal(&p1_enriched).unwrap();
        let all = store.get_all_pending_proposals().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].fee, Some(5000));
    }

    #[test]
    fn test_get_all_returns_empty_when_no_proposals() {
        let (_dir, store) = open_test_store();
        let all = store.get_all_pending_proposals().unwrap();
        assert!(all.is_empty());
    }
}
