# CKBadger> A Local-first CKB-native Explorer

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![CKB](https://img.shields.io/badge/CKB-Nervos-green.svg)](https://www.nervos.org/)

Opinionated software for web5 believers - carbon and silicon alike.

## Principles

- **CKB Native** — Make CKB concepts tangible. Let CKB be felt. The chain is the single source of truth.
- **Local First** — All you need is a CKB node and the will to run your own stack.
- **Agent Friendly** — Agents are first-class users. Built by agents, for agents.

## Web5: The Local-First Web

Web5 inherits from Web2 and Web3 to overthrow both. It wields Web2 technologies but rejects the Web2 paradigm. It stands on blockchains but builds off-chain, local-first. Web5 is [web2+web3](https://www.nervos.org/knowledge-base/web5-extra-decentralized), web5 is not [web2+web3](https://talk.nervos.org/t/my-web5-your-web5/9506).

The web does not have to be centralized services that every user connects to.

Web5 is the local-first web. It is a network of equally connected nodes, each running its own stack: CKB node, Fiber node, CKBadger, and whatever else you choose to run. No single point of failure. No single point of control. CKBadger is the eyes of that stack.

### Radical Simplicity

Take local-first to its logical extreme and you arrive at a design with Unix aesthetics. No RPC middleware. No SQL databases. No container orchestration. Everything is files. Everything is disk I/O.

CKBadger is built around files and executable binaries. It optimizes for writes — building data indexes — not reads, the opposite of typical web services. It's a website that runs locally. A piece of software that shows you web pages. A local web application serving you, not others.

It runs side-by-side with your CKB node. Index building is extremely fast, so local experiments stay cheap: if the DB breaks, rebuild it instead of nursing a 24-hour sync artifact. As long as you have a CKB node, you can always rebuild.

CKB + local-first cuts away accidental complexity and preserves only the essential.

### From Isolated Nodes to a Local-First Network

Local-first software is nothing new. But local-first applications have always been solitary - programs running on isolated personal machines.

CKB changes this. A trustless common knowledge base and a peer-to-peer network connects isolated local-first software into a local-first network. Local-first social networks. Local-first payments. Local-first identity. Everything that used to require a centralized intermediary - without one.

### The AI Era Is the Local-First Era

Web 2.0 centralized services won because they required zero client-side setup — just open a browser and go. The tradeoff was giving up ownership, privacy, and performance.

In the AI era, agents handle setup for you. The friction that kept local-first impractical is gone. [We can have everything](https://talk.nervos.org/t/web5-own-data-not-tokens/9505): ownership, privacy, performance, _and_ ease of use.

This is the era local-first architecture was waiting for. This is the era Fiber and Web5 were waiting for.

### [CKBadger Don't Care](https://www.youtube.com/watch?v=4r7wHMg5Yjg)

Whether or not you share the [Web5 vision](https://web5.tech), I hope CKBadger inspires you to build great things.

## Design Starting Point

Documents under `docs/prompts/` are manually marinated texts capturing the ideas and principles behind CKBadger. Start there for all design reasoning.

## Features

Just try it and feel.

## Quick Start

### Prerequisites

- A running CKB node with RPC accessible (default: `http://127.0.0.1:8114`)
- ~60GB harddisk space

If you can run a CKB node, you can run CKBadger. If you don't know how to run CKB — no worries, agents can do that for you.

Note. only tested on Linux so far, probabaly will run on Macos, not compatible with Windows.

### Build and Run

1. Clone this repository
2. `make release`
3. `./target/release/ckbadger -h`

TODO: build release binaries for download-and-run.

TODO: test compatibility with macos and windows.

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

`ckbadger tui` is pretty fun — watching the stats while CKBadger bulk-syncs is one of my favourite entertainments, see if you can identify the bottlenecks on your machine.

For a fresh db, `ckbadger run` will kick off bulk-sync mode, read data from the local ckb node and build indexes. The time a full sync will take depends:

- ~20mins on a dev machine, AMD Ryzen AI 9 HX 370, 96GB mem, btrfs+LUKS on NVMe SSD.
- ~5hrs on an aws ec2 instance, 8 vCPU @ 2.5GHz, 30GB mem, XFS on 640 NVMe EBS.

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
                    │   (File read / RPC)  │
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

### Testing

Three testing systems: data integrity verification, per-endpoint benchmarking, and concurrent load stress testing. See [docs/TESTING.md](docs/TESTING.md) for full details.

```bash
ckbadger verify --depth fast                       # Data integrity (6 checks, seconds)
make bench                                         # Per-endpoint latency baseline
make stress STRESS_ARGS="--scenario api --auto-ramp"  # Find API breaking point
```

## License

This project is licensed under the GNU General Public License v3.0 — see [LICENSE](./LICENSE).
