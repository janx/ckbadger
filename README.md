# ckbadger

> A high-performance, feature-rich blockchain explorer for Nervos CKB, optimized for the Cell Model.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CKB](https://img.shields.io/badge/CKB-Nervos-green.svg)](https://www.nervos.org/)

## Overview

**ckbadger** is a next-generation CKB blockchain explorer designed with four core principles:

- **CKB Native** - Make CKB concepts tangible instead of just-another-explorer
- **Unrivaled Speed** - Lightning-fast database rebuilds and ultra-low-latency request processing
- **Local First** - Optimized for decentralized deployment on localhosts
- **Agent Friendly** - Designed for AI-assisted development with clear structure and automation-friendly workflows

## Performance Targets

To keep the `Unrivaled Speed` principle concrete, performance work should report against these targets
on localhost deployments:

| Metric                              | Target                                                        | Measurement                                                                                                          |
| ----------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Database rebuild speed              | Maximize sustained throughput without correctness regressions | Record total sync duration and progress EMA (`blocks/sec`) from indexer logs                                         |
| API latency (common read endpoints) | `p50 <= 10ms`, `p95 <= 50ms`, `p99 <= 100ms` on warm cache    | Benchmark `/api/v1` list/detail endpoints and report p50/p95/p99                                                     |
| Correctness guardrail               | `0` verification failures after speed optimizations           | Run `cargo run -p ckbadger-indexer -- verify --depth fast` (and `--depth sampling` for aggregate/DAO/supply changes) |

- Performance-affecting PRs should include before/after numbers.
- Keep benchmark snapshots up to date in `docs/PERFORMANCE_RESULTS.md`.
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

### Data & Analytics

- **Network Dashboard** - Hash rate, difficulty, epoch progress, TPS metrics
- **Historical Charts** - Block time, transaction volume, active addresses
- **Real-time Updates** - WebSocket subscriptions for new blocks and transactions
- **System Status Page** - Monitor sync progress, index rebuild status, and integrity checks
- **Data Integrity Verification** - 28 built-in checks for acceptance testing via API
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
                    ┌───────────┴───────────┐
                    ▼                       ▼
              ┌───────────┐           ┌──────────┐
              │  RocksDB  │           │  Redis   │
              │  (Store)  │           │  (Cache) │
              └───────────┘           └──────────┘
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

| Layer             | Technology                          | Purpose                         |
| ----------------- | ----------------------------------- | ------------------------------- |
| **Frontend**      | Next.js 15, TanStack Query, Zustand | SSR + real-time data            |
| **UI**            | Tailwind CSS, Custom Components     | Responsive design               |
| **Visualization** | react-force-graph-2d, D3.js         | Cell relationship graphs        |
| **API**           | Rust (Axum)                         | High-performance REST/WebSocket |
| **Indexer**       | Rust (3-stage pipeline)             | Block parsing, cell tracking    |
| **Storage**       | RocksDB (ckbadger-store)            | Embedded data store             |
| **Cache**         | Redis                               | API cache + sync progress       |

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
# Start local dependencies (always redis; ckb-node only in internal mode)
make local-up

# Reset local RocksDB + redis cache data (keeps ckb-data)
make local-reset CONFIRM=1

# Run verification against local API
make local-verify

# Sampling checks + RPC spot-checks
make local-verify VERIFY_DEPTH=sampling VERIFY_RPC_URL=http://localhost:8114
```

`make local-up` mode resolution:

- If `.env` contains `COMPOSE_PROFILES=internal`, it starts `redis + ckb-node`
- Otherwise it starts `redis` only (external CKB mode)
- You can override per command:
  - `make local-up CKB_NODE_MODE=internal`
  - `make local-up CKB_NODE_MODE=external`

`make local-reset CONFIRM=1` cleanup scope:

- Deletes local RocksDB path (`CKBADGER_DATA_PATH`) and api secondary path
- Deletes compose volumes `ckbadger-data` and `redis-data` (if present)
- Keeps `ckb-data` volume (CKB chain data is not removed)

## Configuration

### Environment Variables

```bash
# .env.example

# CKB Node Configuration
# For Makefile local-up mode selection:
#   COMPOSE_PROFILES=internal  -> internal CKB node in Docker
#   COMPOSE_PROFILES unset     -> external CKB node (host)
#
# For built-in node (--profile internal): uses http://ckb-node:8114 automatically
# For external node: set your host's CKB RPC URL
CKB_RPC_URL=http://host.docker.internal:8114  # macOS/Windows
# CKB_RPC_URL=http://172.17.0.1:8114          # Linux
CKB_NETWORK=mainnet  # mainnet | testnet | devnet

# ckbadger-store RocksDB data path
CKBADGER_DATA_PATH=./data/ckbadger-store

# Redis (optional)
REDIS_URL=redis://localhost:6379

# API Server
API_PORT=3001
API_RATE_LIMIT=100  # requests per minute

# Frontend
NEXT_PUBLIC_API_URL=http://localhost:3001
NEXT_PUBLIC_WS_URL=ws://localhost:3001/ws

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
CKBADGER_DATA_PATH=./data/ckbadger-store
CKB_RPC_URL=http://localhost:8114
REDIS_URL=redis://localhost:6379
TOKEN_LABELS_PATH=docs/token-labels
```

## API Reference

### REST Endpoints

```
GET  /api/v1/blocks                              # List blocks (paginated)
GET  /api/v1/blocks/{id}                         # Block details
GET  /api/v1/transactions                        # List transactions (paginated)
GET  /api/v1/transactions/{hash}                 # Transaction details
GET  /api/v1/transactions/{hash}/detail          # Transaction with inputs/outputs
GET  /api/v1/addresses/{addr}                    # Address info & balance
GET  /api/v1/cells/live                          # Query live cells
GET  /api/v1/cells/{tx_hash}/{output_index}      # Cell details
GET  /api/v1/statistics/network                  # Network + sync status
GET  /api/v1/forks/recent                        # Deep fork / recent reorg status

# Mempool API (Real-time transaction pool)
GET  /api/v1/mempool/pending-proposals           # Pending proposals with fee/size metadata

# Graph API (Cell Relationship Visualization)
GET  /api/v1/graph/cell/{tx_hash}/{output_index}?depth=2  # Cell relationship graph
GET  /api/v1/graph/transaction/{hash}?depth=2             # Transaction I/O graph
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
//     syncStatus: { isSyncing, syncedBlock, tipBlock, progress, estimatedTime },
//     indexRebuildStatus: { isRebuilding, total, completed, currentIndex, progress } // optional
//   }
// }
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
│   │       ├── parser/     # Block, cell, script parsers
│   │       ├── db/         # RocksDB write operations
│   │       ├── sync/       # Synchronization logic
│   │       └── verify/     # Data integrity verification (28 checks via API)
│   ├── api/                # REST API server
│   │   └── src/
│   │       ├── routes/     # HTTP handlers (blocks, tx, cells, graph)
│   │       └── ws/         # WebSocket handlers
│   ├── ckbadger-store/     # Embedded RocksDB storage engine
│   └── ckb-store-reader/   # Read-only CKB RocksDB reader (optional direct read mode)
├── frontend/               # Next.js application
│   ├── app/                # App router pages
│   ├── components/         # React components
│   │   ├── ui/             # Reusable UI (Hash, Capacity, etc.)
│   │   └── cell-graph.tsx  # Force-directed graph visualization
│   ├── hooks/              # Custom hooks (WebSocket, etc.)
│   └── lib/                # API client, utilities
├── docs/                   # Documentation & references
│   ├── rfcs/               # [submodule] CKB RFCs - protocol specs
│   ├── docs.nervos.org/    # [submodule] Official Nervos docs
│   ├── token-labels/       # [submodule] Known token metadata
│   ├── POSTMORTEM.md       # Historical bugs & lessons learned
│   └── DAO_CALCULATIONS.md # DAO formula documentation
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

# List all 28 available checks
cargo run -p ckbadger-indexer -- verify --list-checks
```

| Tier         | Checks | What it validates                                                     |
| ------------ | ------ | --------------------------------------------------------------------- |
| **Fast**     | 6      | API reachable, sync complete, genesis block, tip block, DAO, forks    |
| **Sampling** | 7      | Block hash roundtrips, parent chain, balances, charts, RPC spot-check |
| **Explorer** | 15     | Last 30 days vs official CKB explorer (cached, 24h freshness)         |

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

| Feature               | Priority | Description                            |
| --------------------- | -------- | -------------------------------------- |
| RGB++ Support         | P1       | RGB++ protocol parsing and display     |
| Multi-language (i18n) | P2       | Chinese, English, Japanese, Korean     |
| Address Labels        | P2       | Exchange, contract, whale address tags |
| Address Monitoring    | P3       | Email/webhook notifications            |
| Transaction Broadcast | P3       | Submit transactions from browser       |
| GraphQL API           | P3       | Alternative query interface            |

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
