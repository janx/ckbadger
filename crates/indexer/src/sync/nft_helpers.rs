use anyhow::{anyhow, Result};

/// Compute a deterministic event order for a DotBit input (consume) within a
/// block.  Consumes get even slots (`tx_global_index * 2`) so that a same-tx
/// output (create) always sorts *after* its paired input.
pub(crate) fn dotbit_consume_event_order(tx_global_index: usize) -> Result<u64> {
    let tx_index = u64::try_from(tx_global_index).map_err(|_| {
        anyhow!(
            "dotbit tx index exceeds u64 while building consume order: {}",
            tx_global_index
        )
    })?;
    tx_index.checked_mul(2).ok_or_else(|| {
        anyhow!(
            "dotbit consume event order overflow: tx_global_index={}",
            tx_global_index
        )
    })
}

/// Compute a deterministic event order for a DotBit output (create) within a
/// block.  Creates get odd slots (`tx_global_index * 2 + 1`).
pub(crate) fn dotbit_create_event_order(tx_global_index: usize) -> Result<u64> {
    dotbit_consume_event_order(tx_global_index)?
        .checked_add(1)
        .ok_or_else(|| {
            anyhow!(
                "dotbit create event order overflow: tx_global_index={}",
                tx_global_index
            )
        })
}

/// Decide whether a consumed DotBit account cell should be treated as truly
/// consumed (deleted).  If a later output re-creates the same account within
/// the same batch, the account is still live.
pub(crate) fn should_consume_dotbit_account(
    latest_create_order: Option<u64>,
    consume_order: u64,
) -> bool {
    match latest_create_order {
        Some(order) => order <= consume_order,
        None => true,
    }
}

/// Returns true if the spore should be consumed in this batch.
/// If the spore was re-created later in the same batch (transfer), skip consumption.
pub(crate) fn should_consume_spore(
    latest_create_tx_index: Option<usize>,
    consume_tx_index: usize,
) -> bool {
    match latest_create_tx_index {
        Some(last_create) => last_create < consume_tx_index,
        None => true,
    }
}

/// Returns true if the mNFT token should be consumed in this batch.
/// If the token was re-created later in the same batch (transfer), skip consumption.
pub(crate) fn should_consume_mnft_token(
    latest_create_tx_index: Option<usize>,
    consume_tx_index: usize,
) -> bool {
    match latest_create_tx_index {
        Some(last_create) => last_create < consume_tx_index,
        None => true,
    }
}

/// Look up the DotBit account-id for a given outpoint.  Prefers an already-
/// persisted store mapping (`db_account_id`) and falls back to the in-flight
/// batch mapping when the outpoint was created within the same batch.
#[allow(dead_code)] // currently only used in tests
pub(crate) fn resolve_dotbit_account_id_for_outpoint(
    db_account_id: Option<Vec<u8>>,
    prev_tx_hash: &[u8],
    prev_index: i16,
    batch_dotbit_outpoints: &std::collections::HashMap<(Vec<u8>, i16), Vec<u8>>,
) -> Option<Vec<u8>> {
    db_account_id.or_else(|| {
        batch_dotbit_outpoints
            .get(&(prev_tx_hash.to_vec(), prev_index))
            .cloned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_dotbit_event_order_marks_output_after_input_in_same_tx() {
        let consume_order = dotbit_consume_event_order(42).unwrap();
        let create_order = dotbit_create_event_order(42).unwrap();
        assert!(create_order > consume_order);
    }

    #[test]
    fn test_should_consume_dotbit_account_when_no_later_output_exists() {
        let consume_order = dotbit_consume_event_order(10).unwrap();
        assert!(should_consume_dotbit_account(None, consume_order));
        assert!(should_consume_dotbit_account(
            Some(consume_order),
            consume_order
        ));
        assert!(!should_consume_dotbit_account(
            Some(consume_order + 1),
            consume_order
        ));
    }

    #[test]
    fn test_should_consume_dotbit_account_with_cross_tx_recreate() {
        let consume_t1 = dotbit_consume_event_order(1).unwrap();
        let create_t2 = dotbit_create_event_order(2).unwrap();
        assert!(
            !should_consume_dotbit_account(Some(create_t2), consume_t1),
            "later output should keep account live"
        );

        let consume_t3 = dotbit_consume_event_order(3).unwrap();
        assert!(
            should_consume_dotbit_account(Some(create_t2), consume_t3),
            "consume after latest output should mark account consumed"
        );
    }

    #[test]
    fn test_should_consume_spore_no_recreate() {
        assert!(should_consume_spore(None, 10));
    }

    #[test]
    fn test_should_consume_spore_recreated_after() {
        assert!(!should_consume_spore(Some(12), 10));
    }

    #[test]
    fn test_should_consume_spore_recreated_before() {
        assert!(should_consume_spore(Some(8), 10));
    }

    #[test]
    fn test_should_consume_mnft_no_recreate() {
        assert!(should_consume_mnft_token(None, 10));
    }

    #[test]
    fn test_should_consume_mnft_recreated_after() {
        assert!(!should_consume_mnft_token(Some(12), 10));
    }

    #[test]
    fn test_should_consume_mnft_recreated_before() {
        assert!(should_consume_mnft_token(Some(8), 10));
    }

    #[test]
    fn test_resolve_dotbit_account_id_for_outpoint_prefers_store_mapping() {
        let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        let tx_hash = vec![0x11; 32];
        let store_account = vec![0x22; 20];
        let batch_account = vec![0x33; 20];
        batch_dotbit_outpoints.insert((tx_hash.clone(), 7), batch_account);

        let resolved = resolve_dotbit_account_id_for_outpoint(
            Some(store_account.clone()),
            &tx_hash,
            7,
            &batch_dotbit_outpoints,
        );
        assert_eq!(resolved, Some(store_account));
    }

    #[test]
    fn test_resolve_dotbit_account_id_for_outpoint_falls_back_to_batch_mapping() {
        let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        let tx_hash = vec![0x44; 32];
        let batch_account = vec![0x55; 20];
        batch_dotbit_outpoints.insert((tx_hash.clone(), 3), batch_account.clone());

        let resolved =
            resolve_dotbit_account_id_for_outpoint(None, &tx_hash, 3, &batch_dotbit_outpoints);
        assert_eq!(resolved, Some(batch_account));
    }
}
