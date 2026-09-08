# Agent-Friendly Page Output

Frontend pages support two machine-oriented formats alongside the default HTML:

- `md` — human+agent readable summaries
- `raw` — structured payloads for tooling/automation
- Both carry `buildVersion` in structured metadata (`.md` frontmatter, `.raw` meta)

Explorer page URLs are network-scoped: `/{network}/...` (for example `/mainnet/blocks`). Route
patterns in capabilities/discovery documents may be written relative to that network prefix.

## Format Negotiation

Priority order (strict):

1. `query.format`
2. URL suffix (`.md` / `.raw`)
3. `Accept` header

`format=html` explicitly selects HTML, including on a suffixed URL. Accept quality weights
are respected; `q=0` does not select a representation and browsers prefer HTML on ties.
Duplicate/unknown `format` values return `400 invalid_format`.
Negotiated pages and HTML page responses use `Vary: Accept` and `Cache-Control: no-store`.
Static hashed assets retain their immutable cache policy. `HEAD` returns the same headers
as `GET` without a body. Errors are JSON with `error.code` and `error.message`, including
`unknown_page`, `unknown_network`, `invalid_profile`, and `profile_not_supported`.

### Markdown Output

1. URL suffix `.md` (e.g. `/mainnet/blocks/123.md`)
2. Query parameter `?format=md`
3. Header `Accept: text/markdown`

### Raw Output

1. URL suffix `.raw` (e.g. `/mainnet/blocks/123.raw`)
2. Query parameter `?format=raw`
3. Header `Accept: application/vnd.ckbadger.raw+json`

## Raw Profiles

- `profile` query selects a raw variant (`default` when absent)
- `profile=debugger` is supported on `/tx/{hash}` and includes `data.txDebugger.mockTransaction`
- Unknown/unsupported profiles fail fast with `invalid_profile` / `profile_not_supported`

## Agent Discovery

- `frontend/public/llms.txt` — short discovery doc
- `frontend/public/llms-full.txt` — full discovery doc
- `http://localhost:8100/capabilities` — machine-readable format/profile/route matrix

The production router and built static discovery files generate their route/profile tables
from `frontend/lib/ai/page-registry.ts`. `/capabilities.chartSlugs` enumerates registered charts.

`/capabilities.site` advertises `pageBasePattern`, the shared `apiBasePattern`/`wsUrlPattern`
(`{network}` placeholder), and the concrete `networks` list plus `defaultNetwork` that make the
placeholder actionable. It no longer lists direct single-network paths — they are not routes on
the origin that serves the document. Its page route matrices are relative to `pageBasePattern`.

## Examples

```bash
NETWORK=mainnet

# Markdown
curl "http://localhost:8100/${NETWORK}/blocks.md"
curl "http://localhost:8100/${NETWORK}/blocks?format=md&limit=20"
curl -H "Accept: text/markdown" "http://localhost:8100/${NETWORK}/charts/hash-rate"

# Raw (default profile)
curl "http://localhost:8100/${NETWORK}/blocks/123.raw"
curl "http://localhost:8100/${NETWORK}/cell/0x...txhash...-0?format=raw"
curl -H "Accept: application/vnd.ckbadger.raw+json" \
  "http://localhost:8100/${NETWORK}/tx/0x...hash..."

# Raw debugger profile (tx only)
curl "http://localhost:8100/${NETWORK}/tx/0x...hash....raw?profile=debugger" \
  | jq '.data.txDebugger.mockTransaction'

# Structured API through the shared network-aware proxy
curl "http://localhost:8100/api/${NETWORK}/v1/statistics/network"

# Capabilities
curl http://localhost:8100/capabilities
```

## End-to-End Debugger Workflow

```bash
TX_HASH=0x...replace_with_real_tx_hash...
NETWORK=mainnet
curl "http://localhost:8100/${NETWORK}/tx/${TX_HASH}.raw?profile=debugger" \
  | jq '.data.txDebugger.mockTransaction' > /tmp/mock_tx.json

ckb-debugger \
  --tx-file /tmp/mock_tx.json \
  --cell-index 0 \
  --cell-type input \
  --script-group-type lock
```

### Troubleshooting

- `invalid_profile` / `profile_not_supported`: check route support via `/capabilities`
- `rpc_http_error` / `rpc_error`: verify CKB RPC URL (default `http://127.0.0.1:8114`)
- `tx_not_found`: confirm tx hash and network alignment

### Matrix Run Helper

```bash
# Full matrix: script-group-type (lock/type) x cell-type (input/output) x all indices
scripts/run_tx_debugger_matrix.sh 0x...tx_hash... http://localhost:8100/mainnet

# Focused iteration
SCRIPT_GROUP_TYPES="lock" CELL_TYPES="input" \
  scripts/run_tx_debugger_matrix.sh 0x...tx_hash... http://localhost:8100/mainnet

# Keep running after a failing combination
CONTINUE_ON_ERROR=1 \
  scripts/run_tx_debugger_matrix.sh 0x...tx_hash... http://localhost:8100/mainnet
```

## Implementation Boundary

- Axum negotiates page requests before both embedded-asset and filesystem SPA fallbacks.
  The TypeScript renderers are bundled by `pnpm --dir frontend build` and embedded in the
  CLI with QuickJS. No Node process or external renderer is required at runtime. Vite dev
  and preview forward format requests to this same Axum server at `127.0.0.1:8100`.
- Each render has a fresh context, its own selected network, a 64 MiB JS heap limit and a
  30-second deadline. Four requests can render concurrently; excess requests receive
  `503 renderer_busy`. Rust limits upstream reads to eight at a time, 256 requests, 16 MiB
  per response and 64 MiB total per render, and allows
  only read-only API requests and the debugger's read-only RPC methods, without redirects.
- Renderers read each network's configured API/RPC; no RocksDB write paths change.
- Direct API JSON under `/api/v1` and shared-proxy JSON under `/api/{network}/v1` are not
  rewritten to markdown/raw
- Static files are not rewritten
- Raw responses include `x-ckbadger-format`, `x-ckbadger-profile`, and `x-ckbadger-schema`

## Public Origin

Set this in the shared orchestrator `ckbadger.toml` (or single-network `config.toml`):

```toml
[frontend]
public_origin = "https://explorer.example.org"
```

This HTTP(S) origin must have no path, query, fragment or credentials. It takes priority
for `/capabilities.origin` and canonical Markdown/raw metadata. Without it, the server
uses `Host` and HTTP, accepting `X-Forwarded-Host`/`X-Forwarded-Proto` only from a loopback
socket peer. A local reverse proxy must overwrite those headers. Configure `public_origin`
when the reverse proxy is on another host. Forwarded headers from remote peers are ignored.

## Production Verification

```bash
pnpm --dir frontend build
cargo test -p ckbadger-api --test api_frontend_formats
cargo build --release -p ckbadger
python3 scripts/test_frontend_release.py --binary target/release/ckbadger
```

The binary test starts the actual CLI frontend in an empty workdir with two fixture API/RPC
servers. It checks success responses, format priority, HTML, HEAD, profile errors, network
isolation, HTTPS canonical metadata and debugger dep-group expansion. Every advertised route
and profile also runs against a pre-sync API to verify that its error reaches the client.
CI tests embedded assets in debug builds too; release packaging requires the release-binary test.

## Checklist: Adding/Changing Routes or Formats (MANDATORY)

1. Register page patterns, kinds and raw profiles in `frontend/lib/ai/page-registry.ts`
2. Register chart slugs and API methods in that same registry if chart coverage changes
3. Update renderer(s): `frontend/lib/ai/markdown-renderer.ts` and/or `frontend/lib/ai/raw-renderer.ts`
4. Update rewrite negotiation in `frontend/lib/ai/markdown-request.ts` if format rules change
5. Rebuild the frontend to regenerate the embedded renderer and discovery route tables; update discovery prose only when the contract changes
6. Add/adjust tests in `frontend/__tests__/lib/markdown-*.test.ts`, `frontend/__tests__/lib/raw-*.test.ts`, and `frontend/__tests__/lib/capabilities.test.ts`
