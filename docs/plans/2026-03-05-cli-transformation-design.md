# ckbadger CLI Transformation Design

**Date**: 2026-03-05
**Status**: Approved

## Goal

Transform ckbadger from a Docker-orchestrated multi-service deployment into a standalone CLI tool (`ckbadger` binary + static assets), following local-first principles. Zero external dependencies — no Docker, no Redis, no Node.js.

## Principle Alignment

- **CKB Native**: Unchanged — CKB chain data remains sole source of truth
- **Local First**: Elevated — single binary + work directory, no containers, no external services
- **Agent Friendly**: Improved — `ckbadger init && ckbadger run` replaces Docker Compose

## Key Decisions

| Decision         | Choice                           | Rationale                              |
| ---------------- | -------------------------------- | -------------------------------------- |
| Frontend serving | Static export + Axum ServeDir    | No Node.js dependency                  |
| Redis            | Fully removed                    | RocksDB + Unix socket IPC              |
| Process model    | Multi-subprocess + supervisor    | Process isolation, independent restart |
| Config           | TOML only (no .env, no env vars) | CLI args > ckbadger.toml > defaults    |
| CKB node         | External only                    | User runs ckb separately               |
| Platforms        | Linux/macOS (UNIX/POSIX)         | Unix sockets for IPC                   |

## Subcommands

```
ckbadger init           # Initialize work directory (ckbadger.toml, data/, run/)
ckbadger run            # Supervisor: fork indexer + api + frontend-server
ckbadger run --only X   # Start specific services (indexer, api, frontend)
ckbadger tui            # Terminal monitoring UI
ckbadger status         # Lightweight sync/service status query
ckbadger verify         # Data integrity checks (fast/sampling)
ckbadger label-import   # Import token/script labels
ckbadger purge          # Delete derived data, keep config
```

All subcommands accept `-C <path>` to specify work directory (default: current directory).

## Work Directory Structure

```
./
├── ckbadger.toml              # Sole configuration file
├── token-labels/              # Optional: overrides share/token-labels
├── labels.toml                # Optional: overrides imported label info
├── data/
│   ├── domain/                # Mutable canonical state (RocksDB)
│   └── append-only/           # Immutable history (RocksDB)
└── run/                       # Runtime state (gitignored)
    ├── supervisor.pid
    ├── indexer.sock            # Indexer IPC socket
    └── logs/                   # Process logs
```

## Installation Package Structure

```
ckbadger-v0.1.0-linux-x86_64/
├── bin/
│   └── ckbadger               # Single binary
└── share/
    ├── frontend/              # Next.js static export output
    └── token-labels/          # Default label data
```

## Configuration (ckbadger.toml)

Priority: **CLI args > ckbadger.toml > defaults**. No .env files. No environment variables.

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

## Process Model (Supervisor)

`ckbadger run` operates as a supervisor process:

```
ckbadger run (supervisor)
  ├── ckbadger _internal indexer          (fork)
  ├── ckbadger _internal api              (fork)
  └── ckbadger _internal frontend-server  (fork)
```

- Supervisor forks itself with internal subcommands
- Monitors child health, auto-restarts on crash
- Unix socket IPC (`run/indexer.sock`) for sync progress queries
- `--only indexer,api` for selective startup
- SIGTERM/SIGINT: graceful shutdown of all children
- PID file at `run/supervisor.pid` for status/tui to discover

## Redis Replacement

| Current (Redis)         | New (No Redis)                                               |
| ----------------------- | ------------------------------------------------------------ |
| `sync:status` (60s TTL) | RocksDB `get_sync_status()` (already exists)                 |
| `sync:progress` (30s)   | Unix socket IPC query to indexer process                     |
| `memory:stats` (30s)    | Unix socket IPC query to indexer process                     |
| Cache invalidation      | IPC notification: indexer -> api to refresh secondary reader |

## Frontend Migration

### Static Export

- Change `next.config.ts`: `output: 'export'`
- Convert home page from async server component to client-side fetch
- Remove middleware.ts (content negotiation moves to Axum)

### Routes Migration to Rust API

- `/ai-md/[...slug]` → Rust API endpoint
- `/ai-raw/[...slug]` → Rust API endpoint
- `/capabilities` → Rust API endpoint
- Content negotiation (Accept header, query.format, URL suffix) → Axum middleware

### Serving

- `ckbadger _internal frontend-server` uses Axum + `tower-http::ServeDir`
- SPA fallback: unmatched routes serve `index.html`
- API proxy: `/api/v1/*` requests proxied to api process (port 8101)

## Crate Reorganization

```
crates/
├── common/            # Unchanged
├── ckb-store-reader/  # Unchanged
├── ckbadger-store/    # Unchanged
├── indexer/           # Remove main.rs → lib only (indexer logic)
├── api/               # Remove main.rs → lib only (api + frontend serving)
├── tui/               # Remove main.rs → lib only (tui logic)
├── supervisor/        # NEW: process management, IPC, health checks
└── cli/               # NEW: single binary, clap subcommands, wires everything
```

The `cli` crate is the only `[[bin]]` target. It imports all other crates as libraries.

## Token Labels & Overrides

### Lookup Order (token-labels directory)

1. `<work_dir>/token-labels/` (if exists)
2. `<install_dir>/share/token-labels/` (default)

### labels.toml (override imported labels)

```toml
[script_name_overrides]
"DAS Lock" = ".bit Lock"
"DID Account" = ".bit Account"
"DID Cell" = ".bit Cell"
"Web5 DID" = "did:ckb"
"SECP256K1/blake160" = "Default Lock"
"SECP256k1/Multisig" = "Default Multisig"

[nft_storage_tier_overrides]
".bit" = "fully_onchain"
"dotbit" = "fully_onchain"
"did:ckb" = "fully_onchain"
"did_ckb" = "fully_onchain"

deprecated = [
    "0x24b04faf80ded836efc05247778eec4ec02548dab6e2012c0107374aa3f68b81",
    "0xd51e6eaf48124c601f41abe173f1da550b4cbca9c6a166781906a287abbb3d9a",
    "0x2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9",
    "0x1122a4fb54697cf2e6e3a96c9d80fd398a936559b90954c6e88eb7ba0cf652df",
    "0x90ca618be6c15f5857d3cbd09f9f24ca6770af047ba9ee70989ec3b229419ac7",
]
```

Applied after token-labels import, overriding any conflicting values.

## Removed Artifacts

- `docker-compose.yml` — no longer needed
- `docker/Dockerfile.*` — no longer needed
- `.env` — replaced by ckbadger.toml
- `Makefile` — replaced by ckbadger subcommands
- Redis feature flag (`--features redis-cache`) — removed entirely

## What Changes for Users

| Before                          | After                                  |
| ------------------------------- | -------------------------------------- |
| `docker compose up` / `make up` | `ckbadger init && ckbadger run`        |
| `make tui`                      | `ckbadger tui`                         |
| `make reset CONFIRM=1`          | `ckbadger purge`                       |
| `make verify`                   | `ckbadger verify`                      |
| Edit `.env`                     | Edit `ckbadger.toml`                   |
| Install Docker + Node.js        | Download ckbadger package, add to PATH |
