# Agent-Friendly Page Output

Frontend pages support two machine-oriented formats alongside the default HTML:

- `md` — human+agent readable summaries
- `raw` — structured payloads for tooling/automation
- Both carry `buildVersion` in structured metadata (`.md` frontmatter, `.raw` meta)

## Format Negotiation

Priority order (strict):

1. `query.format`
2. URL suffix (`.md` / `.raw`)
3. `Accept` header

### Markdown Output

1. URL suffix `.md` (e.g. `/blocks/123.md`)
2. Query parameter `?format=md`
3. Header `Accept: text/markdown`

### Raw Output

1. URL suffix `.raw` (e.g. `/blocks/123.raw`)
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

## Examples

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

# Capabilities
curl http://localhost:8100/capabilities
```

## End-to-End Debugger Workflow

```bash
TX_HASH=0x...replace_with_real_tx_hash...
curl "http://localhost:8100/tx/${TX_HASH}.raw?profile=debugger" \
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
scripts/run_tx_debugger_matrix.sh 0x...tx_hash...

# Focused iteration
SCRIPT_GROUP_TYPES="lock" CELL_TYPES="input" \
  scripts/run_tx_debugger_matrix.sh 0x...tx_hash...

# Keep running after a failing combination
CONTINUE_ON_ERROR=1 scripts/run_tx_debugger_matrix.sh 0x...tx_hash...
```

## Implementation Boundary

- Markdown/raw output is handled in frontend only (Vite SPA middleware)
- API JSON endpoints under `/api/v1` are not rewritten to markdown/raw
- Static files are not rewritten
- Raw responses include `x-ckbadger-format`, `x-ckbadger-profile`, and `x-ckbadger-schema`

## Checklist: Adding/Changing Routes or Formats (MANDATORY)

1. Update markdown route parsing in `frontend/lib/ai/markdown-route.ts` if markdown coverage changes
2. Update raw route parsing in `frontend/lib/ai/raw-route.ts` if raw coverage changes
3. Update renderer(s): `frontend/lib/ai/markdown-renderer.ts` and/or `frontend/lib/ai/raw-renderer.ts`
4. Update rewrite negotiation in `frontend/lib/ai/markdown-request.ts` if format rules change
5. Update capability/discovery files: `frontend/lib/ai/capabilities.ts`, `frontend/public/llms.txt`, and `frontend/public/llms-full.txt`
6. Add/adjust tests in `frontend/__tests__/lib/markdown-*.test.ts`, `frontend/__tests__/lib/raw-*.test.ts`, and `frontend/__tests__/lib/capabilities.test.ts`
