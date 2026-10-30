//! The indexer crate's integration tests: each `tests/*.rs` file is a module
//! of this one binary (`autotests = false`), because every test binary links
//! the whole dependency graph. Declare a new test file below.

mod address_balances;
mod batch_nft_lookups;
mod bulk_build_engine;
mod bulk_build_materialization;
mod crash_recovery;
mod daily_statistics;
mod dao_batch_operations;
mod dao_daily_snapshot;
mod dao_deferred;
mod entity_stats_rollback;
mod live_cells;
mod object_protocol_state;
mod pipeline_consistency;
mod reorg_handling;
mod script_usage_stats;
mod udt_cells;
mod verify_entity_statistics;

/// With `autotests = false`, a `tests/*.rs` file that neither this list nor a
/// `[[test]]` target declares would never compile or run.
#[test]
fn every_test_file_is_declared() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = include_str!("integration.rs");
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let mut undeclared = Vec::new();
    for entry in std::fs::read_dir(manifest_dir.join("tests")).unwrap() {
        let path = entry.unwrap().path();
        let module = if path.extension().is_some_and(|extension| extension == "rs") {
            path.file_stem()
        } else if path.join("mod.rs").is_file() {
            path.file_name()
        } else {
            continue;
        };
        let module = module.unwrap().to_str().unwrap();
        if module == "integration" {
            continue;
        }
        let declared = root.lines().any(|line| line == format!("mod {module};"))
            || manifest.contains(&format!("path = \"tests/{module}.rs\""));
        if !declared {
            undeclared.push(module.to_owned());
        }
    }
    undeclared.sort();
    assert!(
        undeclared.is_empty(),
        "tests/ files declared neither as a module in tests/integration.rs nor as a \
         [[test]] target in Cargo.toml never run: {undeclared:?}"
    );
}
