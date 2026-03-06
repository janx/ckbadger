# ckbadger

> A high-performance, feature-rich blockchain explorer for Nervos CKB, optimized for the Cell Model.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CKB](https://img.shields.io/badge/CKB-Nervos-green.svg)](https://www.nervos.org/)

## Overview

**ckbadger** is a next-generation CKB blockchain explorer designed with three core principles:

- **CKB Native** - Make CKB concepts tangible instead of just-another-explorer. CKB chain data is the only source of truth, all other data are derived from it.
- **Local First** - Optimized for decentralized deployment on localhosts
- **Agent Friendly** - Prefer clear, automation-friendly structure and workflows

### Local First (Expanded)

- Local-first aligns with Web5 and Unix philosophy. Files and executable binaries are the foundation of composability, and ckbadger is designed around files and executable binaries.
- Local-first means ckbadger optimizes for writes (building data indexes), not reads (serving API and web page requests), unlike typical blockchain explorers. This enables extremely fast database sync, so local experiments remain cheap: if the DB is broken, rebuild it instead of protecting a 60-hour sync artifact. DB reads remain very fast, just not the top optimization target.

### Design Starting Point

- Documents under `docs/prompts/` capture the deep understanding and thinking principles behind ckbadger.
- Treat `docs/prompts/` as the starting point for all design reasoning across data model, API, indexer, and UI/UX decisions.

## Coding Principles

- **Fail Fast, Fail Early** - Never hide invariant violations with silent fallbacks, lower-bound clamps, or default-zero repairs; fail immediately with actionable context
- **Refactor First When It Helps** - Before implementing new code, evaluate whether a focused refactor will reduce complexity or risk; if yes, refactor first and then implement.
- **Single Calculation Path for Read Data** - For any data that must be read/derived, keep exactly one computation path and make that single path correct.
- **No Fallback Calculation Chains** - Reject defensive multi-path computation such as "if path A is wrong, fallback to B, then fallback to C"; do not add path B/C, fix path A.

Correctness issues must be fixed at the source path. Do not add silent guards (for example
`max(0)`, `saturating_sub`, or `unwrap_or(0)` on invariant-critical paths) to mask inconsistent
state transitions.

### Debug & Fix Principles

- **Trace Root Cause** - Do not stop at shallow/near-surface symptoms; track the true upstream root cause.
- **Fix Root Cause, Not With Fallbacks** - If you find some data incorrect, don't be satisfy, don't use recalculation code to correct it, instead you should check why it's incorrect in the first place, fix the bug there. Do not patch incorrect pre-computation with extra fallback paths; fix the original computation logic that produced the wrong state.

## Responsibility Boundary

- **Indexer owns all RocksDB writes**: any task that persists or mutates database state must be executed by `ckbadger-indexer`.
- **API is read-only for RocksDB**: `ckbadger-api` must only read from store and must not perform persistent DB writes.
- If API encounters missing derived data (for example missing transaction cycles), API should trigger indexer to compute/write it, then return the result after waiting/polling.

## Performance Targets

To keep the `Unrivaled Speed` principle concrete, performance work should report against these targets
on localhost deployments:

| Metric                              | Target                                                        | Measurement                                                                                  |
| ----------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Database rebuild speed              | Maximize sustained throughput without correctness regressions | Record total sync duration and progress EMA (`blocks/sec`) from indexer logs                 |
| API latency (common read endpoints) | `p50 <= 10ms`, `p95 <= 50ms`, `p99 <= 100ms` on warm cache    | Benchmark `/api/v1` list/detail endpoints and report p50/p95/p99                             |
| Correctness guardrail               | `0` verification failures after speed optimizations           | Run `ckbadger verify --depth fast` (and `--depth sampling` for aggregate/DAO/supply changes) |

- Performance-affecting PRs should include before/after numbers.
- Benchmark snapshots are generated on demand; no committed `docs/PERFORMANCE_RESULTS.md` baseline is required.
- For module ownership and entry points, see `docs/ARCHITECTURE_MAP.md`.

## Features

### Core Explorer

- **Block & Transaction Explorer** - Full block details, transaction decoding, witness parsing
- **Cell Browser** - Live/dead cell tracking, capacity analysis, data preview
- **Address Pages** - Balance, transaction history, owned cells, token holdings
- **Cell Relationship Graph** - Interactive visualization of cell dependencies and transaction flows

### CKB-Specific

- **Nervos DAO Tracker** - Deposit/withdrawal lifecycle, compensation calculator
- **sUDT/xUDT Support** - Token listings, holder rankings, transfer history
- **Spore NFT Support** - NFT collections, metadata rendering, ownership tracking
- **.bit / did:ckb NFT Support** - .bit domain NFTs and did:ckb identity NFTs
- **Script Browser** - Script listings, usage stats, occupation charts

### Data & Analytics

- **Network Dashboard** - Hash rate, difficulty, epoch progress, TPS metrics
- **Historical Charts** - Block time, transaction volume, active addresses, HODL wave, inflation rate, nominal APC
- **Activity System** - Unified activity feed for addresses, tokens, NFTs, and clusters
- **Real-time Updates** - WebSocket subscriptions for new blocks and transactions
- **AI-Friendly Multi-Format Pages** - Use `.md` for summaries, `.raw` for tool-oriented payloads, plus `/capabilities` for machine discovery
- **System Status Page** - Monitor sync progress, pipeline health, and integrity checks
- **Data Integrity Verification** - 54 built-in checks for acceptance testing via API
- **Developer API** - REST endpoints with rate limiting

## Architecture

```
                    ckbadger run (supervisor)
                    ┌──────────────────────────────────────────┐
                    │  ┌──────────┐ ┌─────┐ ┌───────────────┐ │
                    │  │ Indexer  │ │ API │ │Frontend Server│ │
                    │  └────┬─────┘ └──┬──┘ └───────┬───────┘ │
                    │       │          │             │          │
                    │       │   Unix Socket IPC      │          │
                    │       │          │             │          │
                    │  ┌────┴──────────┴────┐   ┌───┴───────┐ │
                    │  │      RocksDB       │   │   SPA     │ │
                    │  │  Domain + Append   │   │  Assets   │ │
                    │  └────────────────────┘   └───────────┘ │
                    └──────────────────────────────────────────┘
                                │
                                ▼
                    ┌──────────────────────┐
                    │      CKB Node        │
                    │    (external RPC)     │
                    └──────────────────────┘
```

## Tech Stack

| Layer             | Technology                                     | Purpose                         |
| ----------------- | ---------------------------------------------- | ------------------------------- |
| **CLI**           | Rust (Clap), single `ckbadger` binary          | All subcommands, supervisor     |
| **Frontend**      | Vite, React 19, React Router, TanStack Query   | Local-first SPA shell           |
| **UI**            | Tailwind CSS, Custom Components                | Responsive design               |
| **Visualization** | react-force-graph-2d, D3.js                    | Cell relationship graphs        |
| **API**           | Rust (Axum)                                    | High-performance REST/WebSocket |
| **Indexer**       | Rust (3-stage pipeline)                        | Block parsing, cell tracking    |
| **Storage**       | RocksDB (domain + append-only, ckbadger-store) | Embedded dual-store data engine |
| **Cache**         | In-memory LRU                                  | API response cache              |
| **IPC**           | Unix domain sockets                            | Inter-process communication     |

## Quick Start

### Prerequisites

- A running CKB node with RPC accessible (default: `http://127.0.0.1:8114`)

### Installation

Download the ckbadger package and add `bin/` to your PATH:

```
ckbadger-v0.1.0-linux-x86_64/
├── bin/
│   └── ckbadger               # Single binary
└── share/
    ├── frontend/              # Static web assets
    └── token-labels/          # Default token label data
```

### Usage

```bash
# Initialize work directory
ckbadger init

# Start all services (indexer + API + frontend server)
ckbadger run

# Access the explorer
open http://localhost:8100
```

### Subcommands

```bash
ckbadger init             # Initialize work directory (ckbadger.toml, data/, run/, perf/)
ckbadger run              # Supervisor: start indexer + api + frontend-server
ckbadger run --only X     # Start specific services (indexer, api, frontend)
ckbadger tui              # Terminal monitoring UI
ckbadger status           # Lightweight sync/service status query
ckbadger verify           # Data integrity checks
ckbadger label-import     # Import token/script labels
ckbadger purge --confirm  # Delete derived data, keep config + perf history
```

All subcommands accept `-C <path>` to specify work directory (default: current directory).

## Configuration

All configuration lives in a single `ckbadger.toml` file. Priority: **CLI args > ckbadger.toml > defaults**. No .env files. No environment variables.

```toml
[ckb]
rpc_url = "http://127.0.0.1:8114"
network = "mainnet"               # mainnet | testnet

[api]
host = "127.0.0.1"
port = 8101

[frontend]
port = 8100                       # Static file HTTP server

[indexer]
batch_size = 10000
parallel_fetch_size = 64
pipeline_buffer = 8
bulk_sync_threshold = 1000

[log]
level = "info"
```

### Work Directory Structure

```
./
├── ckbadger.toml              # Sole configuration file
├── token-labels/              # Optional: overrides share/token-labels
├── labels.toml                # Optional: overrides imported label info
├── data/
│   ├── domain/                # Mutable canonical state (RocksDB)
│   └── append-only/           # Immutable history (RocksDB)
├── run/                       # Runtime state (gitignored)
│   ├── supervisor.pid
│   ├── indexer.sock            # Indexer IPC socket
│   └── logs/                   # Process logs
└── perf/
    └── bulk-sync/             # Auto-generated bulk-sync perf artifacts + latest baseline
```

## API Reference

### REST Endpoints

```
# Blocks
GET  /api/v1/blocks                              # List blocks (paginated)
GET  /api/v1/blocks/{id}                         # Block details
GET  /api/v1/blocks/{id}/fee-stats               # Block fee statistics
GET  /api/v1/blocks/{id}/proposals               # Block proposals

# Transactions
GET  /api/v1/transactions                        # List transactions (paginated)
GET  /api/v1/transactions/{hash}                 # Transaction details
GET  /api/v1/transactions/{hash}/detail          # Transaction with inputs/outputs
GET  /api/v1/transactions/{hash}/cell-deps       # Transaction cell dependencies
GET  /api/v1/transactions/{hash}/cycles          # Transaction cycles status
GET  /api/v1/transactions/{hash}/lifecycle       # Transaction lifecycle
POST /api/v1/transactions/{hash}/calculate-cycles  # Trigger cycles calculation

# Addresses & Cells
GET  /api/v1/addresses/{addr}                    # Address info & balance
GET  /api/v1/addresses/{addr}/transactions       # Address transaction history
GET  /api/v1/addresses/{addr}/tokens             # Address token holdings
GET  /api/v1/addresses/{addr}/activities         # Address activity feed
GET  /api/v1/addresses/top                       # Top addresses by balance
GET  /api/v1/addresses/active                    # Recently active addresses
GET  /api/v1/cells/live                          # Query live cells
GET  /api/v1/cells/by-script                     # Query cells by script
GET  /api/v1/cells/{tx_hash}/{output_index}      # Cell details

# Tokens (sUDT/xUDT)
GET  /api/v1/tokens                              # List tokens
GET  /api/v1/tokens/{type_hash}                  # Token details
GET  /api/v1/tokens/{type_hash}/holders          # Token holder rankings
GET  /api/v1/tokens/{type_hash}/transfers        # Token transfers
GET  /api/v1/tokens/{type_hash}/activities       # Token activities
GET  /api/v1/tokens/{type_hash}/charts/occupation  # Token occupation chart

# Spore / NFT
GET  /api/v1/spore/clusters                      # List Spore clusters
GET  /api/v1/spore/clusters/{id}                 # Cluster details
GET  /api/v1/spore/clusters/{id}/spores          # Spores in cluster
GET  /api/v1/spore/clusters/{id}/holders         # Cluster holders
GET  /api/v1/spore/clusters/{id}/activities      # Cluster activities
GET  /api/v1/spore/nfts                          # List Spore NFTs
GET  /api/v1/assets                              # Unified asset listing
GET  /api/v1/assets/nfts/items/{nft_id}          # NFT item detail
GET  /api/v1/assets/nfts/dotbit/items/{nft_id}   # .bit NFT detail
GET  /api/v1/assets/nfts/did/items/{nft_id}      # did:ckb NFT detail

# DAO
GET  /api/v1/dao/deposits                        # List DAO deposits
GET  /api/v1/dao/deposits/{lock_hash}            # Address DAO deposits
GET  /api/v1/dao/summary/{lock_hash}             # Address DAO summary
GET  /api/v1/dao/statistics                      # DAO statistics
GET  /api/v1/dao/calculator                      # Compensation calculator
GET  /api/v1/dao/charts/total-deposit            # Total deposit chart
GET  /api/v1/dao/charts/daily-deposit            # Daily deposit chart
GET  /api/v1/dao/charts/circulation-ratio        # Circulation ratio chart

# Scripts
GET  /api/v1/scripts                             # List known scripts
GET  /api/v1/scripts/{name}                      # Script details
GET  /api/v1/scripts/{name}/usage                # Script usage stats
POST /api/v1/scripts/lookup                      # Batch script lookup
GET  /api/v1/scripts/code-cell                   # Script code cell
GET  /api/v1/scripts/charts/occupation           # Script occupation chart

# Network & Statistics
GET  /api/v1/statistics/network                  # Network + sync status
GET  /api/v1/statistics/tx-stats                 # Transaction statistics
GET  /api/v1/statistics/recent-blocks            # Recent blocks summary
GET  /api/v1/charts/{chart-name}                 # Various chart endpoints
GET  /api/v1/hardforks                           # Hardfork timeline
GET  /api/v1/forks/recent                        # Deep fork / recent reorg status
GET  /api/v1/search?q=...                        # Universal search

# Mempool
GET  /api/v1/mempool/info                        # Mempool overview
GET  /api/v1/mempool/transactions                # Pending transactions
GET  /api/v1/mempool/blocks                      # Mempool blocks
GET  /api/v1/mempool/pending-proposals           # Pending proposals

# Graph API (Cell Relationship Visualization)
GET  /api/v1/graph/cell/{tx_hash}/{output_index}?depth=2  # Cell relationship graph
GET  /api/v1/graph/transaction/{hash}?depth=2             # Transaction I/O graph
GET  /api/v1/graph/proposals/{block_number}               # Proposal relationship graph
```

### Graph API Response

```json
{
  "nodes": [
    {
      "id": "cell-0x123...-0",
      "nodeType": "cell",
      "label": "100.5K CKB",
      "data": {
        "txHash": "0x123...",
        "outputIndex": 0,
        "capacity": "10050000000",
        "status": "live"
      }
    },
    {
      "id": "tx-0x456...",
      "nodeType": "transaction",
      "label": "TX ...456abc",
      "data": {
        "hash": "0x456...",
        "blockNumber": 12345
      }
    }
  ],
  "links": [
    {
      "source": "tx-0x456...",
      "target": "cell-0x123...-0",
      "linkType": "creates"
    }
  ]
}
```

### WebSocket Subscriptions

```javascript
const ws = new WebSocket('ws://localhost:8101/ws');

// Subscribe to new blocks
ws.send(
  JSON.stringify({
    action: 'subscribe',
    channel: 'new_block',
  })
);

// Subscribe to new transactions
ws.send(
  JSON.stringify({
    action: 'subscribe',
    channel: 'new_transaction',
  })
);

// Ping to keep connection alive
ws.send(JSON.stringify({ action: 'ping' }));

// Receive updates
ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('New event:', data);
};

// new_block message format:
// {
//   type: "new_block",
//   data: {
//     number: 12345,
//     hash: "0x...",
//     timestamp: "2024-01-01T00:00:00Z",
//     transactionsCount: 5,
//     epochNumber: 100,
//     epochIndex: 450,
//     epochLength: 1800,
//     avgBlockTime: "10.50s",
//     estimatedEpochTime: "3h 45m",
//     syncStatus: { isSyncing, syncedBlock, tipBlock, progress, estimatedTime }
//   }
// }
```

### AI-Friendly Page Output (Markdown + Raw)

The frontend supports two machine-oriented page formats:

- `md`: human+agent readable summaries
- `raw`: structured payloads for tooling/automation

Format negotiation priority (strict):

1. `query.format`
2. path suffix (`.md` / `.raw`)
3. `Accept` header

Markdown modes:

- Suffix mode: append `.md` to a page URL
- Query mode: add `?format=md`
- Header mode: send `Accept: text/markdown`

Raw modes:

- Suffix mode: append `.raw` to a page URL
- Query mode: add `?format=raw`
- Header mode: send `Accept: application/vnd.ckbadger.raw+json`

Raw profile:

- `profile` query selects a raw variant (default: `default`)
- Example: `?profile=debugger` for transaction debugger payloads

Examples:

```bash
# Markdown
curl http://localhost:8100/blocks.md
curl "http://localhost:8100/blocks?format=md&limit=20"
curl -H "Accept: text/markdown" http://localhost:8100/charts/hash-rate

# Raw (default profile)
curl http://localhost:8100/blocks/123.raw
curl "http://localhost:8100/cell/0x...txhash...-0?format=raw"
curl -H "Accept: application/vnd.ckbadger.raw+json" http://localhost:8100/tx/0x...hash...

# Raw debugger profile (tx only)
curl "http://localhost:8100/tx/0x...hash....raw?profile=debugger" \
  | jq '.data.txDebugger.mockTransaction'
```

Response details:

- Markdown content type: `text/markdown; charset=utf-8`
- Raw content type: `application/json; charset=utf-8`
- Raw response headers:
  - `x-ckbadger-format`
  - `x-ckbadger-profile`
  - `x-ckbadger-schema`
- Raw profile errors fail fast (`invalid_profile`, `profile_not_supported`)

AI discovery:

- `frontend/public/llms.txt`
- `frontend/public/llms-full.txt`
- `http://localhost:8100/capabilities` (machine-readable format/profile/route matrix)

Debugger workflow (`.raw?profile=debugger`):

```bash
# 1) Fetch debugger payload and extract mock transaction
TX_HASH=0x...replace_with_real_tx_hash...
curl "http://localhost:8100/tx/${TX_HASH}.raw?profile=debugger" \
  | jq '.data.txDebugger.mockTransaction' > /tmp/mock_tx.json

# 2) Run ckb-debugger with extracted tx-file
ckb-debugger \
  --tx-file /tmp/mock_tx.json \
  --cell-index 0 \
  --cell-type input \
  --script-group-type lock
```

Troubleshooting:

- `invalid_profile` / `profile_not_supported`: check `profile` and route support via `/capabilities`
- RPC errors (`rpc_http_error`, `rpc_error`): ensure CKB node RPC is reachable (`ckb.rpc_url` in ckbadger.toml, default `http://127.0.0.1:8114`)
- `tx_not_found`: verify transaction hash on the same network as the connected node

Matrix run helper (lock/type + input/output):

```bash
# Fail-fast matrix run (all combinations)
scripts/run_tx_debugger_matrix.sh 0x...tx_hash...

# Limit scope for faster iteration
SCRIPT_GROUP_TYPES="lock" CELL_TYPES="input" \
  scripts/run_tx_debugger_matrix.sh 0x...tx_hash...

# Keep running after failures to collect all failing combinations
CONTINUE_ON_ERROR=1 scripts/run_tx_debugger_matrix.sh 0x...tx_hash...
```

## Deployment

Download the ckbadger package for your platform, extract it, and add `bin/` to your PATH. Then:

```bash
mkdir my-explorer && cd my-explorer
ckbadger init
# Edit ckbadger.toml to point to your CKB node
ckbadger run
```

No Docker, no Redis, no Node.js — just the single binary and static assets.

## Troubleshooting

### Indexer Exits With `Too many open files`

Symptoms:

- Indexer logs show `IO error ... Too many open files`
- Supervisor repeatedly restarts indexer (`service exited, restarting`)
- Sync height stops advancing

Root cause:

- Process file descriptor soft limit (`ulimit -n`) is too low (commonly `1024`)
- RocksDB opens many SST files during compaction/rollback cleanup, which can exceed that limit

Fix (current shell/session):

```bash
# Stop current supervisor (if running from this work dir)
kill "$(cat run/supervisor.pid)" 2>/dev/null || true

# Raise fd limit for this shell, then restart
ulimit -n 65535
ckbadger run
```

Verify:

```bash
pid=$(cat run/supervisor.pid)
awk '/Max open files/ {print}' /proc/$pid/limits
rg -n "Too many open files" data/domain/LOG
```

Persistent fix:

- If managed by `systemd`, set `LimitNOFILE=65535` in the service unit.
- If started manually, run `ulimit -n 65535` in the same shell before `ckbadger run`.

## Development

### Project Structure

```
ckbadger/
├── crates/
│   ├── cli/                # Single binary, clap subcommands, wires everything
│   ├── config/             # TOML configuration, WorkDir, labels.toml
│   ├── ipc/                # Unix socket IPC (JSON-over-socket protocol)
│   ├── common/             # Shared types and utilities
│   ├── indexer/            # Blockchain indexer (library)
│   │   └── src/
│   │       ├── rpc/        # CKB RPC client
│   │       ├── parser/     # Block, cell, script, spore, .bit, mNFT, RGB++ parsers
│   │       ├── db/         # RocksDB write operations
│   │       ├── sync/       # Synchronization logic
│   │       └── verify/     # Data integrity verification (54 checks via API)
│   ├── api/                # REST API server (library)
│   │   └── src/
│   │       ├── routes/     # HTTP handlers (blocks, tx, cells, tokens, spore, assets, DAO, scripts, graph, etc.)
│   │       └── ws/         # WebSocket handlers
│   ├── ckbadger-store/     # Embedded RocksDB storage engine (40 column families)
│   ├── ckb-store-reader/   # Read-only CKB RocksDB reader (optional direct read mode)
│   └── tui/                # Terminal monitoring UI (library)
├── frontend/               # Vite + React SPA
│   ├── app/                # App router pages (client components)
│   ├── components/         # React components
│   │   ├── ui/             # Reusable UI (Hash, Capacity, etc.)
│   │   └── cell-graph.tsx  # Force-directed graph visualization
│   ├── hooks/              # Custom hooks (WebSocket, etc.)
│   ├── lib/                # API client, utilities
│   └── public/             # Static assets + LLM discovery files
├── docs/                   # Documentation & references
│   ├── rfcs/               # [submodule] CKB RFCs - protocol specs
│   ├── docs.nervos.org/    # [submodule] Official Nervos docs
│   ├── token-labels/       # [submodule] Known token metadata
│   └── plans/              # Design documents and implementation plans
├── .github/workflows/      # CI/CD pipelines
└── Makefile                # Dev shortcuts
```

### Running Locally (Development)

```bash
# Clone with submodules (for CKB reference docs)
git clone --recurse-submodules https://github.com/nervosnetwork/ckbadger.git
cd ckbadger

# Or initialize submodules after clone
git submodule update --init --recursive

# Build and run via CLI
cargo run -p ckbadger-cli -- init
cargo run -p ckbadger-cli -- run

# Or run individual services for development
cargo run -p ckbadger-cli -- run --only indexer
cargo run -p ckbadger-cli -- run --only api

# Run monitoring TUI
cargo run -p ckbadger-cli -- tui

# Run frontend dev server (for frontend development)
cd frontend && pnpm install && pnpm dev
```

### Services

| Service    | Description                   | Port |
| ---------- | ----------------------------- | ---- |
| `indexer`  | Blockchain sync daemon        | -    |
| `api`      | REST/WebSocket API server     | 8101 |
| `frontend` | Static file HTTP server (SPA) | 8100 |

### Label Import

```bash
# Manual trigger (imports UDT + script labels)
ckbadger label-import

# With custom token-labels path
ckbadger label-import --token-labels-path /path/to/token-labels
```

Token labels lookup order:

1. `<work_dir>/token-labels/` (if exists)
2. `<install_dir>/share/token-labels/` (default)

Optional `labels.toml` in work directory can override imported label info (script name overrides, NFT storage tiers, deprecated entries).

`label_import` also auto-runs when the indexer starts.

### Data Integrity Verification

The `verify` subcommand validates data by calling the ckbadger REST API — no direct store access needed. It can run from anywhere the API is reachable.

```bash
# Quick sanity checks (seconds)
ckbadger verify --depth fast

# Sampling + explorer comparison (minutes)
ckbadger verify --depth sampling

# Skip explorer HTTP calls
ckbadger verify --depth sampling --no-explorer

# Custom API URL
ckbadger verify --api-url http://localhost:8101/api/v1

# Add CKB RPC spot-checks
ckbadger verify --rpc-url http://localhost:8114

# List all available checks
ckbadger verify --list-checks
```

| Tier         | Checks | What it validates                                                                              |
| ------------ | ------ | ---------------------------------------------------------------------------------------------- |
| **Fast**     | 6      | API reachable, sync complete, genesis block, tip block, DAO, forks                             |
| **Sampling** | 21     | Block hash roundtrips, parent chain, balances, charts, supply invariants, tokens, spores, NFTs |
| **Explorer** | 16     | Last 30 days vs official CKB explorer (cached, 24h freshness)                                  |

Explorer API responses are cached to `.verify-cache/` with 24-hour freshness. On HTTP failure, stale cache is used as fallback.

### Running Tests

```bash
# Rust tests
cargo test                               # All tests
cargo test --lib                         # Unit tests only
cargo test -p ckbadger-cli               # CLI crate
cargo test -p ckbadger-config            # Config crate
cargo test test_parse_epoch              # Single test (partial match)

# Frontend tests
cd frontend && pnpm test                 # Run Vitest
cd frontend && pnpm test:coverage        # With coverage

# Type check & lint
cd frontend && pnpm type-check           # TypeScript (tsc --noEmit)
cd frontend && pnpm lint                 # ESLint
```

### Test Coverage

Coverage is verified in CI across Rust crates and frontend unit tests.

### CI/CD

GitHub Actions workflow runs on every push:

- Rust: fmt check, clippy, unit tests, coverage (Codecov)
- Frontend: type-check, lint, Vitest, coverage

## Comparison

| Feature                 | ckbadger               | CKB Explorer | Etherscan |
| ----------------------- | ---------------------- | ------------ | --------- |
| Cell Relationship Graph | Interactive            | N/A          | N/A       |
| Local Deployment        | Single binary, no deps | Complex      | Closed    |
| Real-time Updates       | WebSocket              | Polling      | WebSocket |
| DAO Tracking            | Yes                    | Basic        | N/A       |
| sUDT/xUDT               | Yes                    | Partial      | N/A       |
| Spore NFT               | Yes                    | Partial      | N/A       |
| Self-hosted             | Yes                    | Limited      | N/A       |
| External Dependencies   | None (no Docker/Redis) | Docker       | N/A       |

---

## Roadmap

### Future Enhancements

| Feature               | Priority | Status      | Description                            |
| --------------------- | -------- | ----------- | -------------------------------------- |
| RGB++ Support         | P1       | In Progress | RGB++ protocol parsing and display     |
| Multi-language (i18n) | P2       | Planned     | Chinese, English, Japanese, Korean     |
| Address Labels        | P2       | Planned     | Exchange, contract, whale address tags |
| Address Monitoring    | P3       | Planned     | Email/webhook notifications            |
| Transaction Broadcast | P3       | Planned     | Submit transactions from browser       |
| GraphQL API           | P3       | Planned     | Alternative query interface            |

---

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](./CONTRIBUTING.md) for guidelines.

```bash
# Fork the repository
# Create your feature branch
git checkout -b feature/amazing-feature

# Commit your changes
git commit -m 'Add amazing feature'

# Push to the branch
git push origin feature/amazing-feature

# Open a Pull Request
```

## License

This project is licensed under the MIT License - see the [LICENSE](./LICENSE) file for details.

## Acknowledgments

- [Nervos Network](https://www.nervos.org/) - CKB blockchain
- [mempool.space](https://mempool.space/) - Inspiration for real-time architecture
- [Blockscout](https://www.blockscout.com/) - Reference for explorer features
- [Otterscan](https://github.com/otterscan/otterscan) - Inspiration for local-first design

---

<p align="center">
  Built for the Nervos ecosystem
</p>
