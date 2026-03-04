# ckbadger

> A high-performance, feature-rich blockchain explorer for Nervos CKB, optimized for the Cell Model.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CKB](https://img.shields.io/badge/CKB-Nervos-green.svg)](https://www.nervos.org/)

## Overview

**ckbadger** is a next-generation CKB blockchain explorer designed with four core principles:

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

| Metric                              | Target                                                        | Measurement                                                                                                          |
| ----------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Database rebuild speed              | Maximize sustained throughput without correctness regressions | Record total sync duration and progress EMA (`blocks/sec`) from indexer logs                                         |
| API latency (common read endpoints) | `p50 <= 10ms`, `p95 <= 50ms`, `p99 <= 100ms` on warm cache    | Benchmark `/api/v1` list/detail endpoints and report p50/p95/p99                                                     |
| Correctness guardrail               | `0` verification failures after speed optimizations           | Run `cargo run -p ckbadger-indexer -- verify --depth fast` (and `--depth sampling` for aggregate/DAO/supply changes) |

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
- **Data Integrity Verification** - 43 built-in checks for acceptance testing via API
- **Developer API** - REST endpoints with rate limiting

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Frontend                                 │
│         Next.js 15 + TanStack Query + react-force-graph         │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                         API Layer                                │
│              Rust (Axum) - REST / WebSocket                      │
└─────────────────────────────────────────────────────────────────┘
                                │
                ┌───────────────┼───────────────┐
                ▼               ▼               ▼
           ┌───────────┐   ┌───────────┐   ┌──────────┐
           │  RocksDB  │   │  RocksDB  │   │  Redis   │
           │  (Domain) │   │(Append-Only)│  │  (Cache) │
           └───────────┘   └───────────┘   └──────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Rust Indexer                               │
│     Block Fetcher → Cell Parser → Script Decoder → DB Writer     │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        CKB Node                                  │
│               RPC (+ optional direct RocksDB read)               │
└─────────────────────────────────────────────────────────────────┘
```

## Tech Stack

| Layer             | Technology                                     | Purpose                         |
| ----------------- | ---------------------------------------------- | ------------------------------- |
| **Frontend**      | Next.js 15, TanStack Query, Zustand            | SSR + real-time data            |
| **UI**            | Tailwind CSS, Custom Components                | Responsive design               |
| **Visualization** | react-force-graph-2d, D3.js                    | Cell relationship graphs        |
| **API**           | Rust (Axum)                                    | High-performance REST/WebSocket |
| **Indexer**       | Rust (3-stage pipeline)                        | Block parsing, cell tracking    |
| **Storage**       | RocksDB (domain + append-only, ckbadger-store) | Embedded dual-store data engine |
| **Cache**         | Redis                                          | API cache + sync progress       |

## Quick Start

### Prerequisites

- Docker & Docker Compose

### Option 1: With Built-in CKB Node (Recommended for quick start)

```bash
# Clone the repository
git clone https://github.com/nervosnetwork/ckbadger.git
cd ckbadger

# Start all services including CKB node
docker compose --profile internal up -d

# View logs
docker compose logs -f

# Access the explorer
open http://localhost:3000
```

### Option 2: Use Existing CKB Node on Host

If you already have a CKB node running on your host machine:

```bash
# Clone the repository
git clone https://github.com/nervosnetwork/ckbadger.git
cd ckbadger

# Configure to use host CKB node
# Edit .env file:
#   CKB_RPC_URL=http://host.docker.internal:8114  (macOS/Windows)
#   CKB_RPC_URL=http://172.17.0.1:8114            (Linux)

# Start services without CKB node
docker compose up -d

# Access the explorer
open http://localhost:3000
```

### Minimal Setup

For resource-constrained environments (4GB RAM):

```bash
docker compose -f docker-compose.minimal.yml up -d
```

### Local-First Shortcuts

Use repository `Makefile` targets for the most common local workflow:

```bash
# Start local stack (always redis/indexer/api/frontend; ckb-node only in internal mode)
make up

# Rebuild + restart one or multiple compose services
make rebuild SERVICES=api
make rebuild SERVICES="api frontend"

# Run monitoring TUI
make tui

# Reset local RocksDB + redis cache data (keeps ckb-data)
make reset CONFIRM=1

# Run verification against local API
make verify

# Sampling checks + RPC spot-checks
make verify VERIFY_DEPTH=sampling VERIFY_RPC_URL=http://localhost:8114
```

`make up` mode resolution:

- If `.env` contains `COMPOSE_PROFILES=internal`, it starts `redis + ckb-node + indexer + api + frontend`
- Otherwise it starts `redis + indexer + api + frontend` (external CKB mode)
- You can override per command:
  - `make up CKB_NODE_MODE=internal`
  - `make up CKB_NODE_MODE=external`

`make rebuild SERVICES="<name> [name ...]"`:

- Allowed services: `redis`, `ckb-node`, `indexer`, `api`, `frontend`
- Uses `--no-deps` for non-`ckb-node` services, so only listed target services are recreated
- Including `ckb-node` requires internal mode (`COMPOSE_PROFILES=internal` or `CKB_NODE_MODE=internal`)

`make tui`:

- Runs `ckbadger-tui` for sync/memory/throughput monitoring
- Host-only execution (no Docker fallback)
- No RocksDB local path pre-check; TUI reads sync/memory data from Redis + API
- Pass extra args with `TUI_ARGS`, for example:
  - `make tui TUI_ARGS="--refresh-ms 500 --api-url http://localhost:3001/api/v1"`

`make reset CONFIRM=1` cleanup scope:

- Deletes local RocksDB paths (`CKBADGER_DOMAIN_DATA_PATH`, `CKBADGER_APPEND_ONLY_DATA_PATH`) and both api secondary paths
- Deletes compose volumes `ckbadger-data` and `redis-data` (if present)
- Keeps `ckb-data` volume (CKB chain data is not removed)

## Configuration

### Environment Variables

```bash
# .env.example

# CKB Node Configuration
# For Makefile up mode selection:
#   COMPOSE_PROFILES=internal  -> internal CKB node in Docker
#   COMPOSE_PROFILES unset     -> external CKB node (host)
#
# For built-in node (--profile internal): uses http://ckb-node:8114 automatically
# For external node: set your host's CKB RPC URL
CKB_RPC_URL=http://host.docker.internal:8114  # macOS/Windows
# CKB_RPC_URL=http://172.17.0.1:8114          # Linux
CKB_NETWORK=mainnet  # mainnet | testnet | devnet

# ckbadger domain RocksDB data path
CKBADGER_DOMAIN_DATA_PATH=./data/ckbadger-store
# ckbadger append-only RocksDB data path
CKBADGER_APPEND_ONLY_DATA_PATH=./data/ckbadger-store-append-only

# Redis (optional)
REDIS_URL=redis://localhost:6379

# API Server
API_PORT=3001
API_RATE_LIMIT=100  # requests per minute

# Frontend
NEXT_PUBLIC_API_URL=http://localhost:3001/api/v1
NEXT_PUBLIC_WS_URL=ws://localhost:3001/ws
# Optional server-side API base for Next.js route handlers (.md/.raw)
# In docker-compose frontend container, set this to http://api:3001/api/v1
CKBADGER_SERVER_API_URL=http://localhost:3001/api/v1

# Verify subcommand (runs outside Docker, calls the ckbadger API)
CKBADGER_API_URL=http://localhost:3001/api/v1
# VERIFY_CKB_RPC_URL=http://localhost:8114
```

### Indexer Configuration

`ckbadger-indexer` is configured via CLI flags and environment variables.

```bash
# CLI
cargo run -p ckbadger-indexer -- \
  --batch-size 10000 \
  --parallel-fetch-size 64 \
  --pipeline-enabled \
  --pipeline-buffer 8 \
  --bulk-sync-threshold 1000

# Environment variables
CKBADGER_DOMAIN_DATA_PATH=./data/ckbadger-store
CKBADGER_APPEND_ONLY_DATA_PATH=./data/ckbadger-store-append-only
CKB_RPC_URL=http://localhost:8114
REDIS_URL=redis://localhost:6379
TOKEN_LABELS_PATH=docs/token-labels
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
const ws = new WebSocket('ws://localhost:3001/ws');

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
curl http://localhost:3000/blocks.md
curl "http://localhost:3000/blocks?format=md&limit=20"
curl -H "Accept: text/markdown" http://localhost:3000/charts/hash-rate

# Raw (default profile)
curl http://localhost:3000/blocks/123.raw
curl "http://localhost:3000/cell/0x...txhash...-0?format=raw"
curl -H "Accept: application/vnd.ckbadger.raw+json" http://localhost:3000/tx/0x...hash...

# Raw debugger profile (tx only)
curl "http://localhost:3000/tx/0x...hash....raw?profile=debugger" \
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
- `http://localhost:3000/capabilities` (machine-readable format/profile/route matrix)

Debugger workflow (`.raw?profile=debugger`):

```bash
# 1) Fetch debugger payload and extract mock transaction
TX_HASH=0x...replace_with_real_tx_hash...
curl "http://localhost:3000/tx/${TX_HASH}.raw?profile=debugger" \
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
- RPC errors (`rpc_http_error`, `rpc_error`): ensure CKB node RPC is reachable (`CKB_RPC_URL`, default `http://127.0.0.1:8114`)
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

### Docker Compose (Recommended for small deployments)

```bash
# Built-in CKB node
docker compose --profile internal up -d

# External CKB node on host (set CKB_RPC_URL in .env first)
docker compose up -d
```

### Kubernetes (Production scale)

```bash
# Install local Helm chart
helm install ckbadger ./deploy/helm/ckbadger \
  --namespace ckbadger \
  --create-namespace \
  -f ./deploy/helm/ckbadger/values.yaml
```

Helm chart templates and values are under `deploy/helm/ckbadger/`.

## Development

### Project Structure

```
ckbadger/
├── crates/
│   ├── common/             # Shared types and utilities
│   ├── indexer/            # Blockchain indexer
│   │   └── src/
│   │       ├── rpc/        # CKB RPC client
│   │       ├── parser/     # Block, cell, script, spore, .bit, mNFT, RGB++ parsers
│   │       ├── db/         # RocksDB write operations
│   │       ├── sync/       # Synchronization logic
│   │       └── verify/     # Data integrity verification (43 checks via API)
│   ├── api/                # REST API server
│   │   └── src/
│   │       ├── routes/     # HTTP handlers (blocks, tx, cells, tokens, spore, assets, DAO, scripts, graph, etc.)
│   │       └── ws/         # WebSocket handlers
│   ├── ckbadger-store/     # Embedded RocksDB storage engine (37 column families; split stats CFs)
│   ├── ckb-store-reader/   # Read-only CKB RocksDB reader (optional direct read mode)
│   └── tui/                # Terminal monitoring UI (sync/memory/throughput)
├── frontend/               # Next.js application
│   ├── app/                # App router pages
│   │   ├── ai-md/          # Markdown route handlers for AI-friendly page output
│   │   └── ai-raw/         # Raw route handlers for tool-oriented payload output
│   ├── components/         # React components
│   │   ├── ui/             # Reusable UI (Hash, Capacity, etc.)
│   │   └── cell-graph.tsx  # Force-directed graph visualization
│   ├── hooks/              # Custom hooks (WebSocket, etc.)
│   ├── lib/                # API client, utilities
│   │   └── ai/             # Markdown/raw parsing/rendering/capabilities helpers
│   ├── middleware.ts       # Format rewrite for .md/.raw, ?format, and Accept negotiation
│   └── public/             # Static assets + LLM discovery files
├── docs/                   # Documentation & references
│   ├── rfcs/               # [submodule] CKB RFCs - protocol specs
│   ├── docs.nervos.org/    # [submodule] Official Nervos docs
│   ├── token-labels/       # [submodule] Known token metadata
│   ├── ACTIVITY_SYSTEM.md  # Activity system design documentation
│   ├── ARCHITECTURE_MAP.md # Module ownership and entry points
│   ├── DAO_CALCULATIONS.md # DAO formula documentation
│   ├── INDEXER_PIPELINE.md # Pipeline architecture documentation
│   ├── POSTMORTEM.md       # Historical bugs & lessons learned
│   ├── REORG_HANDLING.md   # Chain reorganization handling
│   └── WORLD_VIEW.md       # CKB worldview and design philosophy
├── docker/                 # Dockerfiles (indexer, api, frontend)
├── .github/workflows/      # CI/CD pipelines
└── docker-compose.yml      # Development setup
```

### Running Locally

```bash
# Clone with submodules (for CKB reference docs)
git clone --recurse-submodules https://github.com/nervosnetwork/ckbadger.git
cd ckbadger

# Or initialize submodules after clone
git submodule update --init --recursive

# Start dependencies
docker compose up -d redis ckb-node

# Run indexer (from project root)
cargo run -p ckbadger-indexer --release

# Run API server (from project root)
cargo run -p ckbadger-api --release

# Run monitoring TUI (sync/memory/throughput, no task controls)
cargo run -p ckbadger-tui

# Run frontend
cd frontend && pnpm install && pnpm dev
```

### Docker Services

The `docker-compose.yml` includes the following services:

| Service    | Description                  | Port |
| ---------- | ---------------------------- | ---- |
| `redis`    | Redis cache for sync status  | 6379 |
| `ckb-node` | CKB node (profile: internal) | 8114 |
| `indexer`  | Blockchain sync daemon       | -    |
| `api`      | REST/WebSocket API server    | 3001 |
| `frontend` | Next.js web application      | 3000 |

```bash
# View logs for specific service
docker compose logs -f indexer

# Restart a service
docker compose restart indexer
```

### Label Import

```bash
# Manual trigger (imports UDT + script labels once)
cargo run -p ckbadger-indexer -- label-import

# Custom source path / network
cargo run -p ckbadger-indexer -- label-import \
  --token-labels-path docs/token-labels \
  --network mainnet
```

`label_import` also auto-runs in the background when the indexer starts and
`$TOKEN_LABELS_PATH/information` exists.

### Data Integrity Verification

The indexer includes a `verify` subcommand that validates data by calling the ckbadger REST API — no direct store access needed. It can run from anywhere the API is reachable (host, CI, another machine).

```bash
# Quick sanity checks (seconds)
cargo run -p ckbadger-indexer -- verify --depth fast

# Sampling + explorer comparison (minutes)
cargo run -p ckbadger-indexer -- verify --depth sampling

# Skip explorer HTTP calls
cargo run -p ckbadger-indexer -- verify --depth sampling --no-explorer

# Custom API URL
cargo run -p ckbadger-indexer -- verify --api-url http://localhost:3001/api/v1

# Add CKB RPC spot-checks
cargo run -p ckbadger-indexer -- verify --rpc-url http://localhost:8114

# List all 43 available checks
cargo run -p ckbadger-indexer -- verify --list-checks
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
cargo test -p ckbadger-indexer           # Specific crate
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

| Feature                 | ckbadger       | CKB Explorer | Etherscan |
| ----------------------- | -------------- | ------------ | --------- |
| Cell Relationship Graph | Interactive    | N/A          | N/A       |
| Local Deployment        | Single command | Complex      | Closed    |
| Real-time Updates       | WebSocket      | Polling      | WebSocket |
| DAO Tracking            | Yes            | Basic        | N/A       |
| sUDT/xUDT               | Yes            | Partial      | N/A       |
| Spore NFT               | Yes            | Partial      | N/A       |
| Self-hosted             | Yes            | Limited      | N/A       |
| Min Resources           | 4GB RAM        | 16GB+        | N/A       |

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
