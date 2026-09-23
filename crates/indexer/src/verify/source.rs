//! History-source qualification for the chain-derived verify checks.
//!
//! A chain-derived `Pass` is only worth as much as the history it was computed
//! from. A reachable node proves nothing on its own: its indexer may have been
//! built from a snapshot, started at block N, or run with block/cell filters,
//! any of which silently removes records the verifier would then never see.
//!
//! So qualification is a precondition, not a diagnostic:
//!
//! * the operator's declaration (`verify-source.toml`) states what the index
//!   is — genesis, node version, index start block, filters, who declared it;
//! * the live node is checked against that declaration, and against the case's
//!   anchor: same genesis, same version, `get_indexer_tip >= H`, and `H` still
//!   hashes to the anchor;
//! * anything missing or contradictory is `Inconclusive`. It never becomes a
//!   `Fail` (that would blame ckbadger for the source) and never a `Pass`.
//!
//! This module also owns the cursor walk over `get_transactions`, because
//! "did the enumeration actually finish" is a property of the source, not of
//! the entity being verified. A cursor that does not advance is an `Error`; a
//! budget that runs out is an incomplete page, never a short history.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;

use super::report::SourceProfileReport;
use crate::rpc::{CkbRpcClient, IndexerSearchKey, IndexerTxRecord};

/// The block the case is pinned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAnchor {
    pub block_number: u64,
    pub block_hash: String,
}

/// The operator's declaration of what the node's index actually covers.
///
/// Read from `<network workdir>/verify-source.toml`. Nothing here is inferred:
/// the runtime can confirm or contradict a declaration, but it cannot discover
/// that history was continuously indexed from genesis.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SourceDeclaration {
    pub genesis_hash: String,
    pub node_version: String,
    /// First block the index covers. Anything above 0 leaves a hole the
    /// verifier cannot see into.
    pub index_start_block: u64,
    pub built_from_genesis: bool,
    pub declared_by: String,
    pub declared_at: String,
    #[serde(default)]
    pub block_filter: Option<String>,
    #[serde(default)]
    pub cell_filter: Option<String>,
}

impl SourceDeclaration {
    /// `Ok(None)` when the file is absent — missing evidence, not a failure.
    /// An unparseable file is an error: it was meant to say something.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        let body = match std::fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let declaration = toml::from_str(&body)
            .with_context(|| format!("parsing the source declaration {}", path.display()))?;
        Ok(Some(declaration))
    }
}

/// What a qualified source is, for the manifest and the report.
#[derive(Debug, Clone)]
pub struct SourceProfile {
    pub genesis_hash: String,
    pub node_version: String,
    pub indexer_tip: u64,
    pub index_start_block: u64,
    pub declaration_path: String,
    pub declared_by: String,
    pub declared_at: String,
    pub block_filter: Option<String>,
    pub cell_filter: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SourceQualification {
    Qualified(SourceProfile),
    Inconclusive(String),
}

impl SourceQualification {
    pub fn to_report(&self) -> SourceProfileReport {
        match self {
            SourceQualification::Qualified(profile) => SourceProfileReport {
                status: "qualified".to_string(),
                reason: None,
                genesis_hash: Some(profile.genesis_hash.clone()),
                node_version: Some(profile.node_version.clone()),
                indexer_tip: Some(profile.indexer_tip),
                declaration_path: Some(profile.declaration_path.clone()),
                declared_by: Some(profile.declared_by.clone()),
                declared_at: Some(profile.declared_at.clone()),
                block_filter: profile.block_filter.clone(),
                cell_filter: profile.cell_filter.clone(),
            },
            SourceQualification::Inconclusive(reason) => SourceProfileReport {
                status: "inconclusive".to_string(),
                reason: Some(reason.clone()),
                genesis_hash: None,
                node_version: None,
                indexer_tip: None,
                declaration_path: None,
                declared_by: None,
                declared_at: None,
                block_filter: None,
                cell_filter: None,
            },
        }
    }
}

/// Check the declaration against the live node and the case's anchor.
///
/// Returns `Err` only for transport/protocol failures; every substantive
/// shortfall is `Inconclusive` with the reason spelled out.
pub async fn qualify_source(
    client: &CkbRpcClient,
    declaration: Option<&SourceDeclaration>,
    declaration_path: &Path,
    anchor: &SourceAnchor,
) -> anyhow::Result<SourceQualification> {
    let Some(declaration) = declaration else {
        return Ok(SourceQualification::Inconclusive(format!(
            "no source declaration at {}: a reachable node is not evidence that its index \
             covers [0, {}]",
            declaration_path.display(),
            anchor.block_number
        )));
    };

    if !declaration.built_from_genesis {
        return Ok(SourceQualification::Inconclusive(format!(
            "{} declares built_from_genesis = false; history below the index start is not covered",
            declaration_path.display()
        )));
    }
    if declaration.index_start_block != 0 {
        return Ok(SourceQualification::Inconclusive(format!(
            "{} declares index_start_block = {}; blocks below it are not enumerable",
            declaration_path.display(),
            declaration.index_start_block
        )));
    }
    if let Some(filter) = declaration
        .block_filter
        .as_deref()
        .filter(|f| !f.is_empty())
    {
        return Ok(SourceQualification::Inconclusive(format!(
            "{} declares block_filter = '{filter}'; a filtered index can omit records",
            declaration_path.display()
        )));
    }
    if let Some(filter) = declaration.cell_filter.as_deref().filter(|f| !f.is_empty()) {
        return Ok(SourceQualification::Inconclusive(format!(
            "{} declares cell_filter = '{filter}'; a filtered index can omit records",
            declaration_path.display()
        )));
    }

    let node_version = client.local_node_info().await?.version;
    if node_version != declaration.node_version {
        return Ok(SourceQualification::Inconclusive(format!(
            "declared node_version '{}' but the node answering is '{}': the declaration \
             describes a different binary",
            declaration.node_version, node_version
        )));
    }

    let genesis_hash = client
        .get_block_hash(0)
        .await?
        .ok_or_else(|| anyhow::anyhow!("node returned no genesis block hash"))?;
    if !hash_eq(&genesis_hash, &declaration.genesis_hash) {
        return Ok(SourceQualification::Inconclusive(format!(
            "declared genesis {} but the node's genesis is {}: this is a different chain",
            declaration.genesis_hash, genesis_hash
        )));
    }

    let indexer_tip = client.get_indexer_tip().await?;
    if indexer_tip.block_number < anchor.block_number {
        return Ok(SourceQualification::Inconclusive(format!(
            "node indexer tip {} is behind the anchor {}: it cannot enumerate [0, {}]",
            indexer_tip.block_number, anchor.block_number, anchor.block_number
        )));
    }

    let anchor_hash = client
        .get_block_hash(anchor.block_number)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "node returned no block hash for the anchor height {}",
                anchor.block_number
            )
        })?;
    if !hash_eq(&anchor_hash, &anchor.block_hash) {
        return Ok(SourceQualification::Inconclusive(format!(
            "anchor {} hashes to {} on the node but the export pinned {}: the chain moved \
             under this case",
            anchor.block_number, anchor_hash, anchor.block_hash
        )));
    }

    Ok(SourceQualification::Qualified(SourceProfile {
        genesis_hash,
        node_version,
        indexer_tip: indexer_tip.block_number,
        index_start_block: declaration.index_start_block,
        declaration_path: declaration_path.to_string_lossy().into_owned(),
        declared_by: declaration.declared_by.clone(),
        declared_at: declaration.declared_at.clone(),
        block_filter: declaration.block_filter.clone(),
        cell_filter: declaration.cell_filter.clone(),
    }))
}

/// Re-verify the anchor after the history walk has finished.
///
/// `qualify_source` only proves the anchor was canonical when the walk
/// *started*. A reorg landing underneath it while the walk runs makes
/// `get_transactions` enumerate one chain while the export describes another,
/// and comparing those would produce a confident `Fail` with exact numbers —
/// blaming ckbadger for a chain that moved. So the canonical hash at `H` and
/// the node indexer's coverage of `H` are checked again at the end.
///
/// The tip growing is expected and fine; only the hash at `H` and an index
/// that no longer reaches `H` invalidate the case.
///
/// `Ok(None)` means the anchor still holds. `Ok(Some(reason))` means it moved.
pub async fn reverify_anchor(
    client: &CkbRpcClient,
    anchor: &SourceAnchor,
) -> anyhow::Result<Option<String>> {
    let anchor_hash = client
        .get_block_hash(anchor.block_number)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "node returned no block hash for the anchor height {} after the walk",
                anchor.block_number
            )
        })?;
    if !hash_eq(&anchor_hash, &anchor.block_hash) {
        return Ok(Some(format!(
            "anchor {} moved during the walk: it hashed to {} at the start and {} at the end",
            anchor.block_number, anchor.block_hash, anchor_hash
        )));
    }

    let indexer_tip = client.get_indexer_tip().await?;
    if indexer_tip.block_number < anchor.block_number {
        return Ok(Some(format!(
            "node indexer tip fell to {} during the walk, below the anchor {}: part of the \
             enumeration ran against an index that no longer covers [0, {}]",
            indexer_tip.block_number, anchor.block_number, anchor.block_number
        )));
    }

    Ok(None)
}

/// Case-insensitive hash comparison that tolerates a missing `0x`.
fn hash_eq(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X")
            .to_ascii_lowercase()
    };
    normalize(left) == normalize(right)
}

/// One run-wide allowance, shared by every entity and every phase.
///
/// V3 defines the budget per *run*, not per entity: sixteen entities that each
/// got a fresh 10,000-request allowance would be a 160,000-request run. Every
/// RPC call — pagination included — spends the same counters, so what the
/// manifest reports as spent is what was actually spent.
#[derive(Debug)]
pub struct RunBudget {
    max_records: usize,
    max_rpc_requests: usize,
    deadline: Instant,
    /// RPC requests made so far, across pagination and point lookups.
    pub rpc_requests: usize,
    /// History records folded in so far, across entities.
    pub records: usize,
}

impl RunBudget {
    pub fn new(max_records: usize, max_rpc_requests: usize, wall: Duration) -> Self {
        Self {
            max_records,
            max_rpc_requests,
            deadline: Instant::now() + wall,
            rpc_requests: 0,
            records: 0,
        }
    }

    /// Why the budget is spent, if it is. `None` means there is room left.
    pub fn exhausted(&self) -> Option<String> {
        if self.rpc_requests >= self.max_rpc_requests {
            return Some(format!(
                "RPC budget exhausted after {} request(s)",
                self.rpc_requests
            ));
        }
        if self.records >= self.max_records {
            return Some(format!(
                "record budget exhausted after {} record(s)",
                self.records
            ));
        }
        if Instant::now() >= self.deadline {
            return Some(format!(
                "time budget exhausted after {} RPC request(s) and {} record(s)",
                self.rpc_requests, self.records
            ));
        }
        None
    }

    pub fn charge_request(&mut self) {
        self.rpc_requests += 1;
    }

    pub fn charge_records(&mut self, count: usize) {
        self.records += count;
    }

    pub fn max_records(&self) -> usize {
        self.max_records
    }
}

/// One entity's history as the node indexer enumerated it.
#[derive(Debug, Clone)]
pub struct TransactionHistoryPage {
    pub records: Vec<IndexerTxRecord>,
    pub pages: usize,
    /// True only when the walk reached the end of the history inside budget.
    pub complete: bool,
}

/// Walk `get_transactions` to the end of the history, or to the budget.
///
/// Termination is the node's own empty page. A `last_cursor` that repeats
/// while records are still coming back is an error: continuing would either
/// loop forever or append the same records again, and stopping quietly would
/// report a truncated history as complete.
pub async fn collect_transactions(
    client: &CkbRpcClient,
    search_key: &IndexerSearchKey,
    page_limit: u32,
    max_pages: usize,
    budget: &mut RunBudget,
) -> anyhow::Result<TransactionHistoryPage> {
    let mut records: Vec<IndexerTxRecord> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut pages = 0usize;

    loop {
        // Pagination is part of the run, not free of it: a wide entity whose
        // enumeration alone spends the allowance must stop here rather than
        // hand the caller a short history that looks complete.
        if pages >= max_pages || budget.exhausted().is_some() {
            return Ok(TransactionHistoryPage {
                records,
                pages,
                complete: false,
            });
        }

        budget.charge_request();
        let page = client
            .get_transactions(search_key, "asc", page_limit, cursor.as_deref())
            .await?;
        pages += 1;

        if page.objects.is_empty() {
            return Ok(TransactionHistoryPage {
                records,
                pages,
                complete: true,
            });
        }

        // Any repeat, not just a consecutive one: a cursor that cycles through
        // a set of values would otherwise loop forever, re-appending the same
        // records and inventing capacity out of one page.
        if !seen_cursors.insert(page.last_cursor.clone()) {
            anyhow::bail!(
                "node indexer cursor '{}' repeated after {} page(s) and {} record(s): the \
                 enumeration is cycling rather than advancing",
                page.last_cursor,
                pages,
                records.len()
            );
        }

        budget.charge_records(page.objects.len());
        records.extend(page.objects);
        cursor = Some(page.last_cursor);

        if budget.records >= budget.max_records() {
            return Ok(TransactionHistoryPage {
                records,
                pages,
                complete: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{CkbRpcClient, IndexerSearchKey, Script};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    const GENESIS: &str = "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5";
    const NODE_VERSION: &str = "0.119.0 (abcdef1 2026-01-01)";

    fn declaration_toml(genesis: &str, node_version: &str, index_start_block: u64) -> String {
        format!(
            r#"genesis_hash = "{genesis}"
node_version = "{node_version}"
index_start_block = {index_start_block}
built_from_genesis = true
declared_by = "operator"
declared_at = "2026-09-22T00:00:00Z"
"#
        )
    }

    fn write_declaration(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("verify-source.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    fn anchor(block_number: u64, block_hash: &str) -> SourceAnchor {
        SourceAnchor {
            block_number,
            block_hash: block_hash.to_string(),
        }
    }

    /// Mount the three identity RPCs a qualification needs.
    async fn mount_node(
        server: &MockServer,
        genesis: &str,
        node_version: &str,
        indexer_tip: u64,
        anchor_hash: &str,
    ) {
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method":"local_node_info"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc":"2.0","id":1,"result":{"version": node_version}
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method":"get_indexer_tip"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc":"2.0","id":1,
                "result":{"block_hash":"0xtip","block_number": format!("0x{indexer_tip:x}")}
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({"method":"get_block_hash","params":["0x0"]}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc":"2.0","id":1,"result": genesis
            })))
            .with_priority(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method":"get_block_hash"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc":"2.0","id":1,"result": anchor_hash
            })))
            .with_priority(2)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_matching_declaration_qualifies_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), &declaration_toml(GENESIS, NODE_VERSION, 0));
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xanchor").await;

        let declaration = SourceDeclaration::load(&path).unwrap().unwrap();
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            Some(&declaration),
            &path,
            &anchor(900, "0xanchor"),
        )
        .await
        .unwrap();

        let report = qualification.to_report();
        let SourceQualification::Qualified(profile) = qualification else {
            panic!("a declaration matching the live node must qualify");
        };
        assert_eq!(profile.genesis_hash, GENESIS);
        assert_eq!(profile.node_version, NODE_VERSION);
        assert_eq!(profile.indexer_tip, 1_000);
        assert_eq!(profile.index_start_block, 0);
        assert_eq!(profile.declaration_path, path.to_string_lossy());

        // The operator's statement is the evidence for coverage the runtime
        // cannot re-derive, so the report must carry it, not just the file.
        assert_eq!(report.status, "qualified");
        assert_eq!(report.declared_by.as_deref(), Some("operator"));
        assert_eq!(report.declared_at.as_deref(), Some("2026-09-22T00:00:00Z"));
        assert_eq!(report.block_filter, None);
        assert_eq!(report.cell_filter, None);
        assert_eq!(report.indexer_tip, Some(1_000));
    }

    #[tokio::test]
    async fn a_missing_declaration_is_inconclusive_not_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("verify-source.toml");
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xanchor").await;

        assert!(
            SourceDeclaration::load(&path).unwrap().is_none(),
            "an absent declaration is not an error, it is missing evidence"
        );
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            None,
            &path,
            &anchor(900, "0xanchor"),
        )
        .await
        .unwrap();

        let SourceQualification::Inconclusive(reason) = qualification else {
            panic!("a reachable node is not evidence that its index covers [0, H]");
        };
        assert!(reason.contains("verify-source.toml"), "{reason}");
    }

    #[tokio::test]
    async fn a_node_version_that_does_not_match_the_declaration_is_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), &declaration_toml(GENESIS, "0.118.0", 0));
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xanchor").await;

        let declaration = SourceDeclaration::load(&path).unwrap().unwrap();
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            Some(&declaration),
            &path,
            &anchor(900, "0xanchor"),
        )
        .await
        .unwrap();

        let SourceQualification::Inconclusive(reason) = qualification else {
            panic!("the declaration describes a different binary than the one answering");
        };
        assert!(reason.contains("0.118.0"), "{reason}");
        assert!(reason.contains(NODE_VERSION), "{reason}");
    }

    #[tokio::test]
    async fn an_indexer_tip_behind_the_anchor_is_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), &declaration_toml(GENESIS, NODE_VERSION, 0));
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 899, "0xanchor").await;

        let declaration = SourceDeclaration::load(&path).unwrap().unwrap();
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            Some(&declaration),
            &path,
            &anchor(900, "0xanchor"),
        )
        .await
        .unwrap();

        let SourceQualification::Inconclusive(reason) = qualification else {
            panic!("an index that has not reached H cannot enumerate [0, H]");
        };
        assert!(reason.contains("899"), "{reason}");
        assert!(reason.contains("900"), "{reason}");
    }

    #[tokio::test]
    async fn a_genesis_hash_mismatch_is_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), &declaration_toml("0xdeadbeef", NODE_VERSION, 0));
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xanchor").await;

        let declaration = SourceDeclaration::load(&path).unwrap().unwrap();
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            Some(&declaration),
            &path,
            &anchor(900, "0xanchor"),
        )
        .await
        .unwrap();
        assert!(matches!(
            qualification,
            SourceQualification::Inconclusive(_)
        ));
    }

    #[tokio::test]
    async fn an_index_that_does_not_start_at_genesis_is_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), &declaration_toml(GENESIS, NODE_VERSION, 5_000));
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000_000, "0xanchor").await;

        let declaration = SourceDeclaration::load(&path).unwrap().unwrap();
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            Some(&declaration),
            &path,
            &anchor(900_000, "0xanchor"),
        )
        .await
        .unwrap();

        let SourceQualification::Inconclusive(reason) = qualification else {
            panic!("a tip past H says nothing about the blocks below the index start");
        };
        assert!(reason.contains("5000"), "{reason}");
    }

    #[tokio::test]
    async fn an_anchor_the_node_no_longer_has_is_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), &declaration_toml(GENESIS, NODE_VERSION, 0));
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xreorged").await;

        let declaration = SourceDeclaration::load(&path).unwrap().unwrap();
        let qualification = qualify_source(
            &CkbRpcClient::new(server.uri()),
            Some(&declaration),
            &path,
            &anchor(900, "0xanchor"),
        )
        .await
        .unwrap();

        let SourceQualification::Inconclusive(reason) = qualification else {
            panic!("the chain moved under the anchor; nothing can be concluded");
        };
        assert!(reason.contains("0xreorged"), "{reason}");
    }

    #[tokio::test]
    async fn an_unchanged_anchor_passes_reverification() {
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xanchor").await;
        let moved = reverify_anchor(&CkbRpcClient::new(server.uri()), &anchor(900, "0xanchor"))
            .await
            .unwrap();
        assert_eq!(moved, None, "a growing tip is not a moved anchor");
    }

    #[tokio::test]
    async fn an_anchor_hash_that_changed_during_the_walk_is_detected() {
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 1_000, "0xreorged").await;
        let moved = reverify_anchor(&CkbRpcClient::new(server.uri()), &anchor(900, "0xanchor"))
            .await
            .unwrap()
            .expect("a different hash at H is a moved anchor");
        assert!(moved.contains("0xreorged"), "{moved}");
        assert!(moved.contains("during the walk"), "{moved}");
    }

    #[tokio::test]
    async fn an_index_that_fell_below_the_anchor_during_the_walk_is_detected() {
        let server = MockServer::start().await;
        mount_node(&server, GENESIS, NODE_VERSION, 899, "0xanchor").await;
        let moved = reverify_anchor(&CkbRpcClient::new(server.uri()), &anchor(900, "0xanchor"))
            .await
            .unwrap()
            .expect("an index that no longer reaches H did not enumerate [0, H]");
        assert!(moved.contains("899"), "{moved}");
    }

    #[tokio::test]
    async fn a_malformed_declaration_is_an_error_not_missing_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_declaration(dir.path(), "genesis_hash = ");
        let error = SourceDeclaration::load(&path)
            .expect_err("an unparseable declaration is a local failure, not an absent one");
        assert!(error.to_string().contains("verify-source.toml"), "{error}");
    }

    struct SequentialPages {
        calls: Arc<AtomicUsize>,
        pages: Vec<serde_json::Value>,
    }

    impl Respond for SequentialPages {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let result = self
                .pages
                .get(n)
                .cloned()
                .unwrap_or_else(|| json!({"objects": [], "last_cursor": "0x"}));
            ResponseTemplate::new(200)
                .set_body_json(json!({"jsonrpc":"2.0","id":1,"result": result}))
        }
    }

    fn record(block: u64, io_index: u32) -> serde_json::Value {
        json!({
            "block_number": format!("0x{block:x}"),
            "io_index": format!("0x{io_index:x}"),
            "io_type": "output",
            "tx_hash": format!("0x{block:064x}"),
            "tx_index": "0x0"
        })
    }

    fn search_key() -> IndexerSearchKey {
        IndexerSearchKey::exact_type(
            Script {
                code_hash: format!("0x{}", "11".repeat(32)),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            },
            Some((0, 1_000)),
        )
    }

    async fn mount_pages(server: &MockServer, pages: Vec<serde_json::Value>) -> Arc<AtomicUsize> {
        let calls = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method":"get_transactions"})))
            .respond_with(SequentialPages {
                calls: calls.clone(),
                pages,
            })
            .mount(server)
            .await;
        calls
    }

    #[tokio::test]
    async fn pagination_walks_every_page_and_forwards_the_cursor() {
        let server = MockServer::start().await;
        mount_pages(
            &server,
            vec![
                json!({"objects":[record(1,0), record(2,0)], "last_cursor":"0xc1"}),
                json!({"objects":[record(3,0)], "last_cursor":"0xc2"}),
                json!({"objects":[], "last_cursor":"0x"}),
            ],
        )
        .await;

        let page = collect_transactions(
            &CkbRpcClient::new(server.uri()),
            &search_key(),
            100,
            10,
            &mut RunBudget::new(100, 10_000, Duration::from_secs(600)),
        )
        .await
        .unwrap();

        assert!(page.complete);
        assert_eq!(page.records.len(), 3);
        assert_eq!(page.pages, 3);

        let bodies: Vec<serde_json::Value> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect();
        assert_eq!(bodies[0]["params"][3], serde_json::Value::Null);
        assert_eq!(bodies[1]["params"][3], "0xc1");
        assert_eq!(bodies[2]["params"][3], "0xc2");
    }

    #[tokio::test]
    async fn a_cursor_that_does_not_advance_is_an_error() {
        let server = MockServer::start().await;
        mount_pages(
            &server,
            vec![
                json!({"objects":[record(1,0)], "last_cursor":"0xc1"}),
                json!({"objects":[record(1,0)], "last_cursor":"0xc1"}),
            ],
        )
        .await;

        let error = collect_transactions(
            &CkbRpcClient::new(server.uri()),
            &search_key(),
            100,
            10,
            &mut RunBudget::new(100, 10_000, Duration::from_secs(600)),
        )
        .await
        .expect_err("a repeating cursor would loop forever or silently duplicate history");
        assert!(error.to_string().contains("cursor"), "{error}");
        assert!(error.to_string().contains("0xc1"), "{error}");
    }

    /// A cursor that cycles through several values never repeats consecutively,
    /// so comparing only neighbours would loop until the budget ran out and
    /// report the same records many times over.
    #[tokio::test]
    async fn a_cursor_that_cycles_is_an_error_even_when_it_never_repeats_consecutively() {
        let server = MockServer::start().await;
        mount_pages(
            &server,
            vec![
                json!({"objects":[record(1,0)], "last_cursor":"0xc1"}),
                json!({"objects":[record(2,0)], "last_cursor":"0xc2"}),
                json!({"objects":[record(3,0)], "last_cursor":"0xc1"}),
            ],
        )
        .await;

        let error = collect_transactions(
            &CkbRpcClient::new(server.uri()),
            &search_key(),
            100,
            10,
            &mut RunBudget::new(100, 10_000, Duration::from_secs(600)),
        )
        .await
        .expect_err("a cycling cursor must not be mistaken for progress");
        assert!(error.to_string().contains("0xc1"), "{error}");
        assert!(error.to_string().contains("cycling"), "{error}");
    }

    /// Every page request spends the run's RPC allowance, so a walk that could
    /// not even finish paginating is reported incomplete rather than short.
    #[tokio::test]
    async fn an_rpc_budget_spent_on_pagination_stops_the_walk() {
        let server = MockServer::start().await;
        mount_pages(
            &server,
            vec![
                json!({"objects":[record(1,0)], "last_cursor":"0xc1"}),
                json!({"objects":[record(2,0)], "last_cursor":"0xc2"}),
                json!({"objects":[record(3,0)], "last_cursor":"0xc3"}),
            ],
        )
        .await;

        let mut budget = RunBudget::new(1_000, 2, Duration::from_secs(600));
        let page = collect_transactions(
            &CkbRpcClient::new(server.uri()),
            &search_key(),
            100,
            10,
            &mut budget,
        )
        .await
        .unwrap();

        assert!(!page.complete);
        assert_eq!(page.pages, 2);
        assert_eq!(
            budget.rpc_requests, 2,
            "page requests are charged to the same allowance as point lookups"
        );
    }

    #[tokio::test]
    async fn a_record_budget_that_runs_out_reports_incomplete_rather_than_a_short_history() {
        let server = MockServer::start().await;
        mount_pages(
            &server,
            vec![
                json!({"objects":[record(1,0), record(2,0)], "last_cursor":"0xc1"}),
                json!({"objects":[record(3,0), record(4,0)], "last_cursor":"0xc2"}),
            ],
        )
        .await;

        let page = collect_transactions(
            &CkbRpcClient::new(server.uri()),
            &search_key(),
            100,
            10,
            &mut RunBudget::new(3, 10_000, Duration::from_secs(600)),
        )
        .await
        .unwrap();

        assert!(
            !page.complete,
            "an exhausted budget must never look like the end of the history"
        );
        assert!(page.records.len() <= 4);
    }

    #[tokio::test]
    async fn a_page_budget_that_runs_out_reports_incomplete() {
        let server = MockServer::start().await;
        mount_pages(
            &server,
            vec![
                json!({"objects":[record(1,0)], "last_cursor":"0xc1"}),
                json!({"objects":[record(2,0)], "last_cursor":"0xc2"}),
                json!({"objects":[record(3,0)], "last_cursor":"0xc3"}),
            ],
        )
        .await;

        let page = collect_transactions(
            &CkbRpcClient::new(server.uri()),
            &search_key(),
            100,
            2,
            &mut RunBudget::new(100, 10_000, Duration::from_secs(600)),
        )
        .await
        .unwrap();

        assert!(!page.complete);
        assert_eq!(page.pages, 2);
    }
}
