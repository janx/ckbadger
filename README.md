# CKBadger> An Opinionated Local-first CKB-native Explorer

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![CKB](https://img.shields.io/badge/CKB-Nervos-green.svg)](https://www.nervos.org/)

## Principles

- **CKB Native** — Make CKB concepts tangible. Let CKB be felt. The chain is the single source of truth.
- **Local First** — The only requirement is a CKB node and the willingness to run your own stack.
- **Agent Friendly** — Clear guides, specs, and tailored API responses for agents. Built by agents.

## Web5: The Local-First Web

Who says the web must consist of centralized services that every user connects to?

Web5 is the local-first web — a network of equally connected nodes, each running its own stack: CKB node, Fiber node, CKBadger, and whatever else the owner needs. No single point of failure. No single point of control. CKBadger is one piece of that stack.

### Unix Aesthetics Through Radical Simplicity

Take local-first to its logical extreme and you arrive at a design with Unix aesthetics. No RPC middleware. No SQL databases. No container orchestration. Everything is files. Everything is disk I/O.

CKBadger is built around files and executable binaries. It optimizes for writes — building data indexes — not reads, the opposite of typical web services. It's a website that runs locally. A piece of software that shows you web pages. A local web application serving you, not others.

It runs side-by-side with your CKB node. Index building is extremely fast, so local experiments stay cheap: if the DB breaks, rebuild it instead of nursing a 24-hour sync artifact. As long as you have a CKB node, you never fear data loss or inconsistency.

CKB + local-first cuts away accidental complexity and preserves only the essential.

### From Isolated Nodes to a Local-First Network

Local-first software is nothing new. But local-first applications have always been solitary programs running on isolated personal machines. CKB changes this. By providing a trustless consensus layer and a peer-to-peer network, CKB connects isolated local-first software into a local-first _network_ — enabling local-first social networks, local-first payments, local-first identity, and everything else that used to require a centralized intermediary.

### The AI Era Is the Local-First Era

Web 2.0 centralized services won because they required zero client-side setup — just open a browser and go. The tradeoff was giving up ownership, privacy, and performance.

In the AI era, agents handle setup for you. The friction that kept local-first impractical is gone. Suddenly we can have everything: ownership, privacy, performance, _and_ ease of use.

This is the era local-first architecture was waiting for. This is the era Fiber and Web5 were waiting for.

### Design Starting Point

Documents under `docs/prompts/` are manually marinated texts capturing the ideas and principles behind CKBadger. Start there for all design reasoning — data model, API, indexer, UI/UX.

## Features

Just try it and feel.

## Quick Start

### Prerequisites

- A running CKB node with RPC accessible (default: `http://127.0.0.1:8114`)
- If you can run a CKB node, you can run CKBadger.
- If you don't know how to run CKB — no worries, agents can do that for you.

### Build and Run

1. Clone this repository
2. `make release`
3. `./target/release/ckbadger -h`

TODO: build release binaries for download-and-run.

### Usage

```bash
# Initialize work directory, without -C it will use the current dir
ckbadger init -C workdir

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

`ckbadger tui` is pretty fun — watching the stats while CKBadger bulk-syncs is one of my favourite entertainments.

### Work Directory Structure

```
./
├── ckbadger.toml              # Sole configuration file
├── metadata/              # Optional: local metadata overrides
├── data/
│   ├── domain/                # Mutable canonical state (RocksDB)
│   └── append-only/           # Immutable history (RocksDB)
├── media/                     # Content-addressed decoded media blobs (DOB artwork)
├── run/                       # Runtime state (gitignored)
│   ├── supervisor.pid
│   ├── indexer.sock            # Indexer IPC socket
│   └── logs/                   # Process logs
└── perf/
    └── bulk-sync/             # Auto-generated bulk-sync perf artifacts + latest baseline
```

### Configuration

All configuration lives in a single `ckbadger.toml` file. Priority: **CLI args > ckbadger.toml > defaults**. No `.env` files. No environment variables. If you don't know what a config key means, ask Claude Code or Codex.

### Agent-Friendly Page Output (Markdown + Raw)

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

Agent discovery:

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

### Architecture

```
                    ckbadger run (supervisor)
                    ┌──────────────────────────────────────────┐
                    │  ┌──────────┐ ┌─────┐ ┌───────────────┐  │
                    │  │ Indexer  │ │ API │ │Frontend Server│  │
                    │  └────┬─────┘ └──┬──┘ └───────┬───────┘  │
                    │       │          │            │          │
                    │       │   Unix Socket IPC     │          │
                    │       │          │            │          │
                    │  ┌────┴──────────┴────┐   ┌───┴───────┐  │
                    │  │      RocksDB       │   │   SPA     │  │
                    │  │  Domain + Append   │   │  Assets   │  │
                    │  └────────────────────┘   └───────────┘  │
                    └──────────────────────────────────────────┘
                                │
                                ▼
                    ┌──────────────────────┐
                    │      CKB Node        │
                    │    (external RPC)    │
                    └──────────────────────┘
```

### Services

| Service    | Description                   | Port |
| ---------- | ----------------------------- | ---- |
| `indexer`  | Blockchain sync daemon        | -    |
| `api`      | REST/WebSocket API server     | 8101 |
| `frontend` | Static file HTTP server (SPA) | 8100 |

### Tech Stack

| Layer             | Technology                                              | Purpose                         |
| ----------------- | ------------------------------------------------------- | ------------------------------- |
| **CLI**           | Rust (Clap), single `ckbadger` binary                   | All subcommands, supervisor     |
| **Frontend**      | Vite, React 19, React Router, TanStack Query            | Local-first SPA shell           |
| **UI**            | Tailwind CSS, Custom Components                         | Responsive design               |
| **Visualization** | react-force-graph-2d, D3.js                             | Cell relationship graphs        |
| **API**           | Rust (Axum)                                             | High-performance REST/WebSocket |
| **Indexer**       | Rust (3-stage pipeline)                                 | Block parsing, cell tracking    |
| **Storage**       | RocksDB (60 domain + 1 append-only CFs, ckbadger-store) | Embedded dual-store data engine |
| **Cache**         | In-memory LRU                                           | API response cache              |
| **IPC**           | Unix domain sockets                                     | Inter-process communication     |

### Data Integrity Verification

The `verify` subcommand validates data by calling the CKBadger REST API — no direct store access needed. Runs from anywhere the API is reachable.

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
| **Sampling** | 23     | Block hash roundtrips, parent chain, balances, charts, supply invariants, tokens, spores, NFTs |
| **Explorer** | 27     | Last 30 days vs official CKB explorer (cached, 24h freshness)                                  |

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

## License

This project is licensed under the GNU General Public License v3.0 — see [LICENSE](./LICENSE).
