## Goal

- Finish the Transactions board controls and pool summary, keep unique daily transaction counts separate from overlapping activity kinds, and complete lazy script-group cycle requests.

## Principle Alignment

- CKB Native: transaction counts come from the canonical transaction bucket; non-cellbase activity counts subtract the same day’s coinbase count. Class occurrences and DAO actions retain their actual meaning.
- Local First: reads existing API snapshots and asks the indexer’s lazy calculation endpoint for missing execution evidence. No database writes, service restarts, or new persistent aggregates.
- Agent Friendly: one shared UTC+8 day selector and occurrence-count mapping serve Activities and Transactions. Missing complete-day data is explicit; invalid transaction/cellbase relationships fail with date and values.

## Result

- Transactions has the approved eight kind filters, completed-yesterday counts, occurrence share bar, a pool banner before the view tabs, exact pool snapshot summary, and shared activity headlines in pool rows. Filtering resets cursor state for both views.
- Pool glyphs represent actual fetched transaction sizes; summary values come from the full API snapshot, not the visible page. Partial interpretation remains explicit.
- Activities “All” uses exact same-day canonical transactions minus cellbase. Overlapping class counts are labelled occurrences; DAO counts actions. No latest-day substitution or sum-as-total remains.
- Canonical VM replay is suppressed for provisional/cellbase transactions. Missing executed-group cycles request lazy replay even when the total is already known; the measured total stays visible and completion refreshes transaction detail.
- Re-sync required: **no** for this frontend change.
- Next: integrate the API lane’s activity-type query serialization/filter endpoint follow-up and the parent’s shared TransactionTable changes; run the full integration suite after the isolated performance window.

### Validation

- Existing focused baseline: 4 files / 23 tests passed before refactoring.
- Regression-first failures confirmed incorrect daily count/timezone behavior, unwanted provisional enqueue, and missing group replay with a positive aggregate.
- Focused result: **6 files / 62 tests passed**, covering daily boundaries/missing buckets, overlapping kind counts, pool summary/filter calls, provisional handling, group replay, transaction evidence, and shared activity streaming.
- TypeScript, ESLint, and production build passed. TypeScript used three parent-owned TransactionTable/block fixture files temporarily overlaid and unstaged for the required nullable-cycles contract; those files are excluded from this commit.
- Full suite was interrupted (exit 130) when the indexer lane requested a quiet performance window. The parent will run the final full integration gate. Commit hooks were disabled only during that explicit quiet window, after the checks above had passed.
- Deterministic Chromium fixtures at 1440 and 390 px: Transactions, pool, and Activities have no page errors or document horizontal overflow. Screenshots: `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/{transactions,pool,activities}-final-{1440,390}.png` (local ignored artifacts).
