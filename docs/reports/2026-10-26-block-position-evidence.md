## Goal

- Complete the Block board’s proposal/commitment and uncle diagram, with exact decoded DAO fields next to the raw header.

## Principle Alignment

- CKB Native: proposals link to their canonical commitment blocks; uncle parentage and inclusion remain distinct relationships. The current block reuses the pipeline’s cellbase, transaction-size, and proposal glyphs.
- Local First: the diagram consumes existing block, paginated transaction, and proposal responses. It introduces no endpoint, persistent state, store writes, or service changes.
- Agent Friendly: one `decodeDaoHeader` utility decodes the four little-endian u64 words as bigint; inconsistent proposal distances and missing committed transaction origins fail with block/transaction context.

## Result

- `BlockPosition` shows exact incoming/outgoing proposal arcs, deduplicating committed transaction hashes when an uncle repeats a proposal. Incoming scope is explicitly limited to the current transaction page when paginated.
- Current and parent timestamps are shown only when supplied. Other tiles contain known heights without invented timestamps or transaction contents. Each uncle links its supplied parent hash, its own hash, and a dashed inclusion edge; its parent is not assumed to match an unqueried canonical block hash.
- Desktop retains the SVG diagram. Below md, the same grouped relationships become a vertical reading with current block, proposal endpoints/counts, and uncle parent/inclusion links; no mobile horizontal scrolling is required.
- Raw DAO bytes now show exact C/S/U as CKB and AR with its full 16-decimal scale. U is explicitly protocol occupied capacity, including genesis virtual occupied capacity; it is not relabelled as real Common Knowledge Size.
- Re-sync required: **no**.
- Next: parent integration runs the new tests and final frontend gates after the isolated indexer performance window. This commit was requested before that window ends so integration review can proceed.

### Validation

- Added decoder tests for the approved board’s exact values, maximum u64 precision, invalid hex, and wrong byte lengths.
- Added diagram tests for exact proposal endpoints, uncle links, repeated proposal deduplication, pagination scope, tip/cellbase behavior, mobile relation parity, missing origin, and inconsistent distance.
- Added page coverage for the four decoded fields and the U qualifier.
- New automated tests and type/lint gates are **pending**, deliberately not run during the indexer lane’s quiet performance window. Commit hooks are disabled under the parent’s explicit quiet-window instruction. The parent owns the final integrated gate.
- Fixture-backed Chromium acceptance at 1440 and 390 px passed: two proposal arcs, one uncle branch, correct exact DAO display, zero page errors, and document scroll width equal to viewport width. The mobile relationship region also has scrollWidth equal to clientWidth. Local artifacts: `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/block-position-{1440,390}.png`.
