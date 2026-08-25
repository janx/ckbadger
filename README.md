# CKBadger> A Local-first CKB-native Explorer

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![CKB](https://img.shields.io/badge/CKB-Nervos-green.svg)](https://www.nervos.org/)
![Linux](https://img.shields.io/badge/Linux-compatible-brightgreen?logo=linux&logoColor=white)
![macOS](https://img.shields.io/badge/macOS-compatible-brightgreen?logo=apple&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-incompatible-red?logo=windows&logoColor=white)

## Principles

- **CKB Native** — Make CKB concepts tangible. Let CKB be felt. The chain is the single source of truth.
- **Local First** — All you need is a CKB node and the will to run your own stack.
- **Agent Friendly** — Agents are first-class users. Built by agents, for agents.

## Local-First: A Web5 Pillar

Web5 inherits from Web2 and Web3 to overthrow both. It wields Web2 technologies but rejects the Web2 paradigm. It stands on blockchains but builds off-chain, local-first. Web5 is [web2+web3](https://www.nervos.org/knowledge-base/web5-extra-decentralized), web5 is not [web2+web3](https://talk.nervos.org/t/my-web5-your-web5/9506).

Web5 has multiple pillars — local-first software, peer-to-peer networking, a proof-of-work objective anchor, and more. CKBadger is local-first software.

The web does not have to be centralized services that every user connects to. Local-first is the alternative: a network of equally connected nodes, each running its own stack — CKB node, Fiber node, CKBadger, and whatever else you choose to run. No single point of failure. No single point of control. CKBadger is the eyes of that stack.

### Self-Custody, Extended

Local-first extends self-custody. Self-custody tells us to hold our own keys and assets; local-first tells us to hold our identity and data too.

### Simplicity

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

Whether or not you share the [Web5 vision](https://web5.info), I hope CKBadger inspires you to build great things.

## Features

Just try it and feel.

## Quick Start

### Prerequisites

- A running CKB node with RPC accessible (default: `http://127.0.0.1:8114`)
- ~60GB disk space per fully indexed network (actual usage depends on chain and enabled data)

If you can run a CKB node, you can run CKBadger. If you don't know how to run CKB — no worries, agents can do that for you.

### Build and Run

1. Clone this repository
2. `make release`
3. `./target/release/ckbadger -h`

### Usage

```bash
# Initialize a work directory (creates an orchestrator + a mainnet stack).
# Without -C it uses the current dir; add --with-testnet to run testnet too.
ckbadger init -C workdir

# Start all services (one indexer + API per network, plus a shared frontend)
ckbadger -C workdir run

# Access the explorer (switch networks from the header)
open http://localhost:8100
```

### Subcommands

```bash
ckbadger init                 # Initialize orchestrator (ckbadger.toml) + a mainnet work dir
ckbadger init --with-testnet  # ... plus a testnet stack alongside mainnet
ckbadger run                  # Supervisor: start indexer + api per network + shared frontend
ckbadger tui                  # Terminal monitoring UI
ckbadger status               # Lightweight sync/service status query
ckbadger verify               # Data integrity checks
ckbadger label-import         # Import token/script labels
ckbadger purge --confirm      # Delete chain/cache/run/bench data; keep config, metadata, perf, network observations
```

All subcommands accept `-C <path>` to specify work directory (default: current directory).

`run --only` is a single-network command. Point `-C` at a network subdirectory and select
`indexer`, `api`, `frontend-server`, or `crawler`, for example:

```bash
ckbadger -C workdir/testnet run --only indexer,api
```

An orchestrator root rejects `--only` because the target network would be ambiguous.

`ckbadger tui` is pretty fun — watching the stats while CKBadger bulk-syncs is one of my favourite entertainments, see if you can identify the bottlenecks on your machine.

For a fresh db, `ckbadger run` will kick off bulk-sync mode, read data from the local ckb node and build indexes. The time a full sync will take depends:

- ~20mins on my dev machines, 24 cores, 64-96GB mem, NVMe SSD.
- ~5hrs on an aws ec2 instance, 8 vCPU @ 2.5GHz, 30GB mem, XFS on 640 NVMe EBS.
- probably requires tweaking on <=16GB mem servers.

### Agent-Friendly Page Output

Every supported explorer page exposes `.md` (markdown summary) and `.raw` (structured JSON)
formats for agent consumption. In orchestrator mode, page paths start with the network
(`/<network>/...`). See [docs/AI_FORMATS.md](docs/AI_FORMATS.md) for format negotiation, raw
profiles, debugger workflow, and examples.

## Dive Deeper

### Design Starting Point

Documents under `docs/prompts/` are manually marinated texts capturing the ideas and principles behind CKBadger. Start there for all design reasoning.

### Architecture

```
ckbadger run (orchestrator supervisor)
├── Shared frontend + network-aware reverse proxy (:8100)
│   └── /api/{network}/v1 + /ws/{network} ──▶ matching per-network API
└── Per-network stack (mainnet, testnet, …)
    ├── CKB node / RPC ──▶ indexer ──▶ domain + append-only stores ──────────────┐
    ├── configured CKB RPC ── local_node_info/get_peers ──┐                     │
    ├── CKB p2p network ── Identify/Discovery ─────────────┴─▶ crawler (opt-in)  │
    │                                                         └─▶ network store ┤
    └── API (read-only) ◀───────────────────────────────────────────────────────┘
```

### Services

| Service           | Description                                                                      | Default port               |
| ----------------- | -------------------------------------------------------------------------------- | -------------------------- |
| `indexer`         | Per-network blockchain sync daemon                                               | -                          |
| `api`             | Per-network REST/WebSocket API server                                            | mainnet 8101, testnet 8102 |
| `frontend-server` | Shared SPA server and network-aware reverse proxy                                | 8100                       |
| `crawler`         | Opt-in CKB L1 dial + configured-node session observer; sole network-store writer | outbound p2p + local RPC   |

### Tech Stack

| Layer             | Technology                                                          | Purpose                          |
| ----------------- | ------------------------------------------------------------------- | -------------------------------- |
| **CLI**           | Rust (Clap), single `ckbadger` binary                               | All subcommands, supervisor      |
| **Frontend**      | Vite, React 19, React Router, TanStack Query                        | Local-first SPA shell            |
| **UI**            | Tailwind CSS, Custom Components                                     | Responsive design                |
| **Visualization** | react-force-graph-2d, D3.js                                         | Cell relationship graphs         |
| **API**           | Rust (Axum)                                                         | High-performance REST/WebSocket  |
| **Indexer**       | Rust (3-stage pipeline)                                             | Block parsing, cell tracking     |
| **Storage**       | RocksDB (59 domain + 1 append-only + 3 network CFs, ckbadger-store) | Embedded three-store data engine |
| **Cache**         | In-memory LRU                                                       | API response cache               |
| **IPC**           | Unix domain sockets                                                 | Inter-process communication      |

The crawler keeps three independent kinds of evidence: Discovery advertisements, direct sessions
seen by the configured local CKB node, and this crawler's outbound dial results. RPC
`get_peers.is_outbound` is interpreted from that local node's vantage (`true` means the local node
initiated the session; `false` means the remote peer did). A direct-only peer remains visible even
without a reusable address, and RPC session addresses are never turned into crawler dial aliases.
These facts do not prove NAT/firewall status, hosting environment, “home node”, or global
reachability. The configured RPC node's genesis hash must match the selected network before its
session observations are accepted.

All of this remains inside the existing 3-CF network store; ckbadger-store still has 63 CFs total
(59 domain + 1 append-only + 3 network). The crawler is its sole writer and the API opens it only
as a read-only secondary.

### Deployment

CKBadger is designed local-first, but the architecture doesn't lock you in. The services are stateless beyond RocksDB, so with the right deployment setup the same binary can sit behind a reverse proxy and serve a public audience. Run it for yourself, or run it for others.

### Work Directory Structure

`ckbadger init` sets up an **orchestrator root**: a top-level `ckbadger.toml` plus one work directory per network. Each network subdirectory is a self-contained single-network work dir.

```
./                             # Orchestrator root
├── ckbadger.toml              # Orchestrator config: [[network]] list + shared [frontend]/[log]
├── mainnet/                   # A standard single-network work directory
│   ├── config.toml            #   this network's config ([ckb] node, [api] port, [store], …)
│   ├── metadata/              #   Optional: local metadata overrides
│   ├── data/
│   │   ├── domain/            #   Mutable canonical state (RocksDB)
│   │   ├── append-only/       #   Immutable cell payloads (RocksDB)
│   │   └── network/           #   Opt-in p2p/session observations (exempt from chain replay)
│   ├── media/                 #   Content-addressed decoded media blobs (DOB artwork)
│   ├── run/                   #   Runtime state — supervisor.pid, indexer.sock, logs/ (gitignored)
│   └── perf/                  #   Auto-generated bulk-sync perf artifacts + latest baseline
└── testnet/                   # With --with-testnet: a second network, same layout
    └── …
```

A bare work directory containing only `config.toml` (no `ckbadger.toml`) runs as a single standalone network — the orchestrator is just one-stack-per-network on top of that.

### Configuration

`ckbadger init` writes two kinds of config:

- **Orchestrator** — `ckbadger.toml` at the root: a `[[network]]` list plus the shared `[frontend]` (default `127.0.0.1:8100`) and `[log]`. `ckbadger run` here launches one indexer + API per network and one shared frontend.
- **Per-network** — `<network>/config.toml`: a standard single-network config (`[ckb]` node +
  `[api]` port + `[store]` …). Each network gets its own API port (mainnet `8101`, testnet `8102`)
  and its own data directory. Before `ckbadger run`, set the required `[ckb].workdir` and the
  matching `[ckb].rpc_url` in every network config. Generated configs use port `8114` as a
  placeholder for both networks; co-resident CKB nodes normally need distinct RPC ports.
  Startup validates the configured network against the node rather than indexing the wrong chain.

Priority: **CLI args > config.toml > defaults**. No `.env` files. No environment variables. If you don't know what a config key means, ask Claude Code or Codex.

### Multiple networks (mainnet + testnet)

`ckbadger init --with-testnet` runs mainnet and testnet side by side behind **one** explorer at `http://localhost:8100`:

- The frontend reverse-proxies `/api/<network>/v1/*` and `/ws/<network>` to each network's API port — single origin, no CORS; the per-network API servers stay unaware.
- The active network lives in the URL path (`/mainnet/…`, `/testnet/…`); a header switcher flips between the live networks, and deep links like `/testnet/tx/0x…` preserve the network when shared.
- Each network reads its own CKB node, configured by `[ckb].workdir` and `[ckb].rpc_url` in that
  network's `config.toml`.
- APIs, enabled crawlers, and the shared frontend start immediately. Indexers are admitted in
  `[[network]]` order so only one network performs fresh-store bulk sync at a time. Once an
  indexer reaches the near-tip threshold, the next network starts; live sync then proceeds
  independently.

### Testing

Three testing systems: data integrity verification, per-endpoint benchmarking, and concurrent load stress testing. See [docs/TESTING.md](docs/TESTING.md) for full details.

```bash
ckbadger verify --depth fast                       # Data integrity (7 checks, seconds)
make bench                                         # Per-endpoint latency baseline
make stress STRESS_ARGS="--scenario api --auto-ramp"  # Find API breaking point
```

## License

This project is licensed under the GNU General Public License v3.0 — see [LICENSE](./LICENSE).
