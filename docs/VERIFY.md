# Data Integrity Verification (`verify` subcommand)

The indexer includes a `verify` subcommand for acceptance testing data integrity. It calls the ckbadger REST API (no direct store access needed) and can run from anywhere the API is reachable.

## Quick Start

```bash
cargo run -p ckbadger-indexer -- verify --depth fast        # Quick checks (seconds)
cargo run -p ckbadger-indexer -- verify --depth sampling    # Sampling + explorer (minutes)
cargo run -p ckbadger-indexer -- verify --list-checks       # List all 55 checks
cargo run -p ckbadger-indexer -- verify --no-explorer       # Skip explorer HTTP checks
cargo run -p ckbadger-indexer -- verify --api-url http://localhost:3001/api/v1  # Custom API URL
cargo run -p ckbadger-indexer -- verify --rpc-url http://localhost:8114         # Add RPC spot-checks
cargo run -p ckbadger-indexer -- verify --checks genesis_block,dao_statistics_sane  # Specific checks
```

## Check Tiers

| Tier                  | Checks | Runtime | What it validates                                                                                                                                                                                                                                                         |
| --------------------- | ------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Fast** (F1-F6)      | 6      | seconds | API reachable, sync complete, genesis block, tip block, deep fork clear, DAO statistics sane                                                                                                                                                                              |
| **Sampling** (S1-S22) | 22     | minutes | Block hash roundtrip, parent chain, address balance, chart validations (tx count, cells, supply, block time, epoch, HODL wave, knowledge composition, APC, inflation), supply invariants, RPC compare, tokens, spores, NFTs, top-asset holder/address consistency         |
| **Explorer** (X1-X27) | 27     | minutes | Compare last 30 days against official CKB explorer API (tx count, DAO deposit, hash rate, difficulty, knowledge size, uncle rate, cell counts, daily deposit, circulation ratio, supply, burnt, mining reward, treasury, NervosDAO point-in-time stats, depositors count) |

## Explorer Response Cache

Explorer checks cache HTTP responses to `.verify-cache/{indicator}.json` (override with `--cache-dir`).

- **Fresh cache** (< 24h): used immediately, no HTTP request
- **Stale cache**: re-fetched from API; on HTTP failure, stale data used with warning
- **Cache format**: JSON with `fetched_at` timestamp, `indicator` name, `data` map (date->value)

## Adding a New Verify Check

1. Choose tier: `api_checks.rs` (fast or sampling), `explorer.rs` (external API comparison)
2. Create struct implementing `Check` trait (name, description, tier, run)
3. Register in the module's `*_checks()` function
4. Convention: `F{N}` / `S{N}` / `X{N}` prefix in doc comment

## File Locations

| What                | Where                                     |
| ------------------- | ----------------------------------------- |
| CLI args & runner   | `crates/indexer/src/verify/mod.rs`        |
| Check trait & types | `crates/indexer/src/verify/checks.rs`     |
| API checks (F+S)    | `crates/indexer/src/verify/api_checks.rs` |
| Explorer checks     | `crates/indexer/src/verify/explorer.rs`   |
| Report rendering    | `crates/indexer/src/verify/report.rs`     |
| LCG sampler         | `crates/indexer/src/verify/sampling.rs`   |
