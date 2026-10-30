//! The API crate's integration tests: each `tests/*.rs` file is a module of
//! this one binary (`autotests = false`), because every test binary links the
//! whole dependency graph. Declare a new test file below.
//! `api_pipeline` stays its own binary — see its `[[test]]` in Cargo.toml.

mod common;

mod api_activities;
mod api_address_holdings;
mod api_asset_history;
mod api_assets;
mod api_blocks;
mod api_cells;
mod api_charts;
mod api_dao;
mod api_fiber;
mod api_forks;
mod api_frontend_formats;
mod api_graph;
mod api_hash_validation;
mod api_inventory_summary;
mod api_live_cell_filters;
mod api_mempool;
mod api_network;
mod api_numeric_validation;
mod api_scripts;
mod api_search;
mod api_spore;
mod api_statistics;
mod api_token_redesign;
mod api_tokens;
mod api_transaction_cells;
mod api_transactions;
mod api_verify;
mod api_verify_asset_contract;
mod api_verify_calendar_contract;
mod api_vessels;
mod api_warmup;
mod frontend_server_spa;
mod metadata_object_tiers;

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
