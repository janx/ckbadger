//! A token as the API names it (resync fix round 2, B): its stored row, which
//! holds what the chain says, with its curated label joined.
//!
//! The indexer stores no label in a `tokens` row: label import keeps the
//! labels in `token_labels`, and a row is the chain's on both sync paths,
//! whatever labels exist. The join happens here, when a route reads the
//! token, by THE label rule ([`TokenInfo::attach_label`]: the label wins
//! where it states a field, the chain's own values stay beside it in
//! `chain_metadata`, and `metadata_source` says `label`). Every route that
//! reads a token row reads it through this module, so which source wins is
//! one rule that can change without a re-sync.
//!
//! Cost: the labels are few (one per curated token) and read with the row
//! they name, in the same pinned view: one point read beside a token's row
//! ([`get_token`]), one `multi_get` beside a batch of rows
//! ([`get_tokens_batch`]), one scan of the labels beside the list of every
//! row ([`list_tokens`]); a label never lists a token the chain does not
//! hold.

use ckbadger_store::types::{TokenInfo, TokenLabel};
use ckbadger_store::CkbadgerStore;

/// THE join: the token's row with its label, if it has one.
pub fn token_with_label(mut row: TokenInfo, label: Option<&TokenLabel>) -> TokenInfo {
    if let Some(label) = label {
        row.attach_label(label);
    }
    row
}

/// The token `type_hash` as the API names it; `None` while the chain does not
/// hold it (a label alone names nothing).
pub fn get_token(store: &CkbadgerStore, type_hash: &[u8]) -> anyhow::Result<Option<TokenInfo>> {
    let Some(row) = store.get_token(type_hash)? else {
        return Ok(None);
    };
    Ok(Some(token_with_label(
        row,
        store.get_token_label(type_hash)?.as_ref(),
    )))
}

/// Whether the chain holds the token `type_hash` (a row exists); no label is
/// read.
pub fn token_exists(store: &CkbadgerStore, type_hash: &[u8]) -> anyhow::Result<bool> {
    Ok(store.get_token(type_hash)?.is_some())
}

/// The tokens `type_hashes` as the API names them, in order, `None` where the
/// chain does not hold one.
pub fn get_tokens_batch(
    store: &CkbadgerStore,
    type_hashes: &[Vec<u8>],
) -> anyhow::Result<Vec<(Vec<u8>, Option<TokenInfo>)>> {
    let rows = store.get_tokens_batch(type_hashes)?;
    let labels = store.get_token_labels_batch(type_hashes)?;
    anyhow::ensure!(
        rows.len() == labels.len(),
        "token batch read {} rows but {} labels for {} type hashes",
        rows.len(),
        labels.len(),
        type_hashes.len()
    );
    Ok(rows
        .into_iter()
        .zip(labels)
        .map(|((hash, row), label)| (hash, row.map(|row| token_with_label(row, label.as_ref()))))
        .collect())
}

/// Every token the chain holds, as the API names it.
pub fn list_tokens(store: &CkbadgerStore) -> anyhow::Result<Vec<(Vec<u8>, TokenInfo)>> {
    let labels: std::collections::HashMap<Vec<u8>, TokenLabel> =
        store.list_token_labels()?.into_iter().collect();
    Ok(store
        .list_tokens()?
        .into_iter()
        .map(|(hash, row)| {
            let row = token_with_label(row, labels.get(&hash));
            (hash, row)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::TokenMetadataSource;

    fn chain_row() -> TokenInfo {
        TokenInfo {
            type_code_hash: vec![0x50; 32],
            hash_type: 2,
            type_args: vec![0x24; 32],
            standard: "xudt".to_string(),
            name: Some("RGB++".to_string()),
            symbol: Some("RGB++".to_string()),
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 101,
            icon_url: None,
            description: None,
            transfers_count: 3,
            metadata_source: Some(TokenMetadataSource::Chain),
            chain_metadata: None,
        }
    }

    fn label() -> TokenLabel {
        TokenLabel {
            name: "Renamed".to_string(),
            symbol: "RNM".to_string(),
            decimals: 18,
            icon_url: Some("logo.png".to_string()),
            description: Some("curated".to_string()),
        }
    }

    /// The label wins where it states a field; the chain's facts stay; no
    /// label leaves the row as the chain made it.
    #[test]
    fn the_label_is_joined_over_the_chains_row() {
        assert_eq!(token_with_label(chain_row(), None), chain_row());
        let joined = token_with_label(chain_row(), Some(&label()));
        assert_eq!(
            (
                joined.name.as_deref(),
                joined.symbol.as_deref(),
                joined.decimals,
                joined.icon_url.as_deref(),
                joined.metadata_source
            ),
            (
                Some("Renamed"),
                Some("RNM"),
                Some(18),
                Some("logo.png"),
                Some(TokenMetadataSource::Label)
            )
        );
        assert_eq!((joined.first_seen_block, joined.transfers_count), (101, 3));
    }

    /// The readers join the label of each row they read, and list no label
    /// without a row.
    #[test]
    fn every_reader_joins_the_rows_label() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let labelled = vec![0x01; 32];
        let plain = vec![0x02; 32];
        let unheld = vec![0x03; 32];
        store.put_token_direct(&labelled, &chain_row()).unwrap();
        store.put_token_direct(&plain, &chain_row()).unwrap();
        store.put_token_label_direct(&labelled, &label()).unwrap();
        store.put_token_label_direct(&unheld, &label()).unwrap();

        let expected = token_with_label(chain_row(), Some(&label()));
        assert_eq!(
            get_token(&store, &labelled).unwrap(),
            Some(expected.clone())
        );
        assert_eq!(get_token(&store, &plain).unwrap(), Some(chain_row()));
        assert_eq!(get_token(&store, &unheld).unwrap(), None);
        assert!(!token_exists(&store, &unheld).unwrap());
        assert_eq!(
            get_tokens_batch(&store, &[labelled.clone(), unheld.clone(), plain.clone()]).unwrap(),
            vec![
                (labelled.clone(), Some(expected.clone())),
                (unheld, None),
                (plain.clone(), Some(chain_row())),
            ]
        );
        assert_eq!(
            list_tokens(&store).unwrap(),
            vec![(labelled, expected), (plain, chain_row())]
        );
    }
}
