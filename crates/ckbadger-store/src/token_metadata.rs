//! Token metadata provenance (IV-26): the rules that say a token's
//! `name` / `symbol` / `decimals` — what its own Unique Cell says, and a
//! curated label joined over it — and whether the two disagree.
//!
//! A stored token row is the chain's (resync fix round 2, B): the live writer
//! and the bulk snapshot writer both call [`TokenInfo::merge_chain_metadata`]
//! (through `udt::apply_onchain_token_info`) and nothing else. The API joins
//! a token's curated label when it reads the token, by
//! [`TokenInfo::attach_label`] (its one `token_identity` function), and reads
//! [`TokenInfo::checked_metadata_source`],
//! [`TokenInfo::chain_metadata_conflict`] and [`TokenInfo::claimed_symbols`]
//! on what that gives.
use crate::types::{ChainTokenMetadata, OnchainInfoBinding, TokenInfo, TokenMetadataSource};

impl TokenInfo {
    /// THE label rule: a curated label joined to the token's row gives its
    /// name, symbol and decimals ([`Self::apply_label`], the label wins where
    /// it states a field), its icon and its description; the chain's own
    /// values stay beside them (`chain_metadata`). The API applies it when it
    /// reads a token (resync fix round 2, B); no stored row holds a label. The
    /// standard stays the row's: a chain fact, what the pair of the token's
    /// type script runs, which a label never states (resync fix round 3, item
    /// 4, the lead's ruling).
    pub fn attach_label(&mut self, label: &crate::types::TokenLabel) {
        self.apply_label(label.name.clone(), label.symbol.clone(), label.decimals);
        self.icon_url = label.icon_url.clone();
        self.description = label.description.clone();
    }

    /// A curated label writes `name`, `symbol` and `decimals`; the stored
    /// fields are now the label's. The bound chain values are kept.
    pub fn apply_label(&mut self, name: String, symbol: String, decimals: i32) {
        self.name = Some(name);
        self.symbol = Some(symbol);
        self.decimals = Some(decimals);
        self.metadata_source = Some(TokenMetadataSource::Label);
    }

    /// Merge a bound Unique Cell observation into the stored row, by its
    /// binding, and return how the stored row disagreed with it before the
    /// merge (`stored -> chain`), if it did.
    ///
    /// * `XudtExtension` — the xUDT's own args name the Unique Cell, so the
    ///   association is cryptographic: every field the chain asserts (a
    ///   non-empty name / symbol, the decimals) replaces the stored one, and
    ///   the chain values become the bound ones. The source is chain: a stored
    ///   row holds what the chain says and never a label (resync fix round 2,
    ///   B; the branch that kept a label-written field the chain left empty
    ///   went in round 3, item 3, unreachable since).
    /// * `IssuanceCooccurrence` — a heuristic: it fills gaps only and never
    ///   displaces already-known metadata or an earlier binding. A row it
    ///   fills from nothing is chain-sourced.
    pub fn merge_chain_metadata(&mut self, chain: &ChainTokenMetadata) -> Option<String> {
        let divergence = self.metadata_divergence(
            chain.name.as_deref(),
            chain.symbol.as_deref(),
            Some(i32::from(chain.decimals)),
        );
        match chain.binding {
            OnchainInfoBinding::XudtExtension => {
                if let Some(name) = &chain.name {
                    self.name = Some(name.clone());
                }
                if let Some(symbol) = &chain.symbol {
                    self.symbol = Some(symbol.clone());
                }
                self.decimals = Some(i32::from(chain.decimals));
                self.metadata_source = Some(TokenMetadataSource::Chain);
                self.chain_metadata = Some(chain.clone());
            }
            OnchainInfoBinding::IssuanceCooccurrence => {
                let mut filled = false;
                if self.name.is_none() && chain.name.is_some() {
                    self.name = chain.name.clone();
                    filled = true;
                }
                if self.symbol.is_none() && chain.symbol.is_some() {
                    self.symbol = chain.symbol.clone();
                    filled = true;
                }
                if self.decimals.is_none() {
                    self.decimals = Some(i32::from(chain.decimals));
                    filled = true;
                }
                if filled && self.metadata_source.is_none() {
                    self.metadata_source = Some(TokenMetadataSource::Chain);
                }
                if self.chain_metadata.is_none() {
                    self.chain_metadata = Some(chain.clone());
                }
            }
        }
        divergence
    }

    /// The fields where the stored row contradicts what another source
    /// asserts (`None` arguments assert nothing; a field the store does not
    /// hold yet is a gap, not a contradiction). The one comparison used by
    /// label import (label vs. store), both sync write paths (chain vs.
    /// store) and the API's `conflict`.
    pub fn metadata_divergence(
        &self,
        asserted_name: Option<&str>,
        asserted_symbol: Option<&str>,
        asserted_decimals: Option<i32>,
    ) -> Option<String> {
        let mut parts = Vec::new();
        if let (Some(name), Some(asserted)) = (self.name.as_deref(), asserted_name) {
            if name != asserted {
                parts.push(format!("name {name:?} -> {asserted:?}"));
            }
        }
        if let (Some(symbol), Some(asserted)) = (self.symbol.as_deref(), asserted_symbol) {
            if symbol != asserted {
                parts.push(format!("symbol {symbol:?} -> {asserted:?}"));
            }
        }
        if let (Some(decimals), Some(asserted)) = (self.decimals, asserted_decimals) {
            if decimals != asserted {
                parts.push(format!("decimals {decimals} -> {asserted}"));
            }
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// Whether the stored fields disagree with the bound Unique Cell.
    pub fn chain_metadata_conflict(&self) -> bool {
        self.chain_metadata.as_ref().is_some_and(|chain| {
            self.metadata_divergence(
                chain.name.as_deref(),
                chain.symbol.as_deref(),
                Some(i32::from(chain.decimals)),
            )
            .is_some()
        })
    }

    /// The stored source, checked against the stored fields: a row with a
    /// name, symbol or decimals has a source, and a row with none has none.
    /// A disagreement is a corrupt row (an error naming it), never a guess.
    pub fn checked_metadata_source(&self) -> anyhow::Result<Option<TokenMetadataSource>> {
        let has_fields = self.name.is_some() || self.symbol.is_some() || self.decimals.is_some();
        match (self.metadata_source, has_fields) {
            (Some(source), true) => Ok(Some(source)),
            (None, false) => Ok(None),
            (None, true) => anyhow::bail!(
                "token row (type args 0x{}) has name/symbol/decimals but no metadata source",
                hex::encode(&self.type_args)
            ),
            (Some(source), false) => anyhow::bail!(
                "token row (type args 0x{}) has metadata source {source:?} but no name, symbol \
                 or decimals",
                hex::encode(&self.type_args)
            ),
        }
    }

    /// Every symbol the token answers to: the stored one and its Unique
    /// Cell's, without repeats (stored first).
    pub fn claimed_symbols(&self) -> Vec<&str> {
        let mut symbols: Vec<&str> = self.symbol.as_deref().into_iter().collect();
        if let Some(chain) = self
            .chain_metadata
            .as_ref()
            .and_then(|chain| chain.symbol.as_deref())
        {
            if !symbols.contains(&chain) {
                symbols.push(chain);
            }
        }
        symbols
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unnamed_row() -> TokenInfo {
        TokenInfo {
            type_code_hash: vec![0x50; 32],
            hash_type: 2,
            type_args: vec![0x24; 32],
            standard: "xudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: None,
            chain_metadata: None,
        }
    }

    fn labelled_rgbpp_jump() -> TokenInfo {
        let mut row = unnamed_row();
        row.apply_label("RGB++_Jump".to_string(), "RGB++_Jump".to_string(), 18);
        row
    }

    fn chain(
        name: Option<&str>,
        symbol: Option<&str>,
        decimals: u8,
        binding: OnchainInfoBinding,
    ) -> ChainTokenMetadata {
        ChainTokenMetadata {
            name: name.map(str::to_string),
            symbol: symbol.map(str::to_string),
            decimals,
            binding,
        }
    }

    /// The RGB++_Jump shape: a curated label renames a token whose own Unique
    /// Cell (bound by issuance co-occurrence) calls itself "RGB++". The label
    /// keeps the stored fields, the source says so, the chain's values are
    /// kept beside it, and the disagreement is a conflict.
    #[test]
    fn cooccurrence_keeps_the_label_and_records_the_chain_values() {
        let mut row = labelled_rgbpp_jump();
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Label));
        let observed = chain(
            Some("RGB++"),
            Some("RGB++"),
            18,
            OnchainInfoBinding::IssuanceCooccurrence,
        );
        let divergence = row.merge_chain_metadata(&observed);
        assert_eq!(
            divergence.as_deref(),
            Some(r#"name "RGB++_Jump" -> "RGB++", symbol "RGB++_Jump" -> "RGB++""#)
        );
        assert_eq!(
            (row.name.as_deref(), row.symbol.as_deref(), row.decimals),
            (Some("RGB++_Jump"), Some("RGB++_Jump"), Some(18))
        );
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Label));
        assert_eq!(row.chain_metadata, Some(observed));
        assert!(row.chain_metadata_conflict());
        assert_eq!(row.claimed_symbols(), ["RGB++_Jump", "RGB++"]);
        assert_eq!(
            row.checked_metadata_source().unwrap(),
            Some(TokenMetadataSource::Label)
        );
    }

    /// A cryptographic (extension) binding gives the stored row the chain's
    /// fields, the source chain; a label is joined after it when the token is
    /// read, and wins where it states a field, the chain's values beside it.
    /// (Re-pinned in resync fix round 3, item 3: a stored row never holds a
    /// label, so the extension never meets a label-written field; it pinned
    /// the branch that kept one the chain left empty, unreachable since round
    /// 2's B and removed.)
    #[test]
    fn extension_binding_gives_the_row_the_chains_fields_and_a_label_joined_after_wins() {
        let mut row = unnamed_row();
        let full = chain(Some("On"), Some("ON"), 8, OnchainInfoBinding::XudtExtension);
        assert_eq!(row.merge_chain_metadata(&full), None);
        assert_eq!(
            (row.name.as_deref(), row.symbol.as_deref(), row.decimals),
            (Some("On"), Some("ON"), Some(8))
        );
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Chain));
        assert!(!row.chain_metadata_conflict());
        assert_eq!(row.claimed_symbols(), ["ON"]);

        let mut row = unnamed_row();
        let nameless = chain(None, Some("ON"), 8, OnchainInfoBinding::XudtExtension);
        row.merge_chain_metadata(&nameless);
        assert_eq!(row.name.as_deref(), None);
        assert_eq!(row.symbol.as_deref(), Some("ON"));
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Chain));

        row.apply_label("RGB++_Jump".to_string(), "RGB++_Jump".to_string(), 8);
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Label));
        assert_eq!(row.chain_metadata, Some(nameless));
        assert!(
            row.chain_metadata_conflict(),
            "the label's symbol is not the chain's"
        );
        assert_eq!(row.claimed_symbols(), ["RGB++_Jump", "ON"]);
    }

    /// With no label, the Unique Cell fills the row under either binding and
    /// the source is chain; a later co-occurrence never displaces the bound
    /// values (fill gaps only), while a later extension binding does.
    #[test]
    fn unlabelled_tokens_take_the_chain_values_by_binding_strength() {
        let mut row = unnamed_row();
        assert_eq!(row.checked_metadata_source().unwrap(), None);
        let first = chain(
            Some("First"),
            Some("F"),
            6,
            OnchainInfoBinding::IssuanceCooccurrence,
        );
        assert_eq!(row.merge_chain_metadata(&first), None);
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Chain));
        assert_eq!(row.chain_metadata.as_ref(), Some(&first));

        let second = chain(
            Some("Second"),
            Some("S"),
            6,
            OnchainInfoBinding::IssuanceCooccurrence,
        );
        row.merge_chain_metadata(&second);
        assert_eq!(row.symbol.as_deref(), Some("F"));
        assert_eq!(row.chain_metadata.as_ref(), Some(&first));
        assert!(!row.chain_metadata_conflict());

        let extension = chain(Some("Ext"), Some("E"), 2, OnchainInfoBinding::XudtExtension);
        row.merge_chain_metadata(&extension);
        assert_eq!(row.symbol.as_deref(), Some("E"));
        assert_eq!(row.chain_metadata.as_ref(), Some(&extension));
        // A later co-occurrence never displaces an extension binding.
        row.merge_chain_metadata(&second);
        assert_eq!(row.chain_metadata.as_ref(), Some(&extension));
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Chain));
    }

    /// Label import after a chain observation: the label writes the fields
    /// and the source, the bound chain values stay.
    #[test]
    fn a_label_applied_after_the_chain_keeps_the_chain_values() {
        let mut row = unnamed_row();
        let observed = chain(
            Some("RGB++"),
            Some("RGB++"),
            18,
            OnchainInfoBinding::IssuanceCooccurrence,
        );
        row.merge_chain_metadata(&observed);
        row.apply_label("RGB++_Jump".to_string(), "RGB++_Jump".to_string(), 18);
        assert_eq!(row.metadata_source, Some(TokenMetadataSource::Label));
        assert_eq!(row.chain_metadata, Some(observed));
        assert!(row.chain_metadata_conflict());
    }

    /// A stored row whose fields and source disagree is corrupt: an error
    /// naming what is wrong, never a guessed source.
    #[test]
    fn a_source_that_disagrees_with_the_fields_is_an_error() {
        let mut fields_without_source = unnamed_row();
        fields_without_source.symbol = Some("X".to_string());
        let error = fields_without_source
            .checked_metadata_source()
            .unwrap_err()
            .to_string();
        assert!(error.contains("no metadata source"), "{error}");

        let mut source_without_fields = unnamed_row();
        source_without_fields.metadata_source = Some(TokenMetadataSource::Chain);
        let error = source_without_fields
            .checked_metadata_source()
            .unwrap_err()
            .to_string();
        assert!(error.contains("no name, symbol or decimals"), "{error}");
    }

    /// Provenance lives on the token row in the domain store; the
    /// append-only store has no token CF and refuses the row.
    #[test]
    fn token_provenance_is_domain_state_never_append_only() {
        use crate::store::{APPEND_CFS, CF_TOKENS, DOMAIN_CFS};
        assert!(DOMAIN_CFS.contains(&CF_TOKENS));
        assert!(!APPEND_CFS.contains(&CF_TOKENS));
        assert!(!crate::store::is_append_only_cf_name(CF_TOKENS));
        let dir = tempfile::TempDir::new().unwrap();
        let append = crate::CkbadgerStore::open_append_only(dir.path()).unwrap();
        assert!(!append.has_cf(CF_TOKENS));
        // The append-only store refuses a domain CF outright (it panics
        // naming the CF), so the row can never land there.
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            append.put_token_direct(&[0xec; 32], &labelled_rgbpp_jump())
        }));
        assert!(matches!(refused, Err(_) | Ok(Err(_))), "{refused:?}");
    }

    /// The new fields survive the store's codec.
    #[test]
    fn token_info_with_provenance_round_trips_through_the_store() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = crate::CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut row = labelled_rgbpp_jump();
        row.merge_chain_metadata(&chain(
            Some("RGB++"),
            Some("RGB++"),
            18,
            OnchainInfoBinding::IssuanceCooccurrence,
        ));
        store.put_token_direct(&[0xec; 32], &row).unwrap();
        assert_eq!(store.get_token(&[0xec; 32]).unwrap(), Some(row));
    }
}
