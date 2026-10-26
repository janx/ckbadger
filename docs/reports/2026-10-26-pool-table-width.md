## Goal

- Keep every pool transaction column, including cycles, visible within the 1248px desktop content area.

## Principle Alignment

- CKB Native: exact Value, Fee, fee rate, bytes, and cycles remain visible together with transaction interpretation and state.
- Local First: presentation only; no query, endpoint, or persistence changes.
- Agent Friendly: reuse the shared exact bigint amount formatter and its decimal/unit wrap points.

## Result

- Reduced the pool table’s column budget from 1350px to 1212px. Value/Fee retain eight decimals and wrap before their decimal or unit when necessary. Partial interpretation remains explicit and can wrap inside its column.
- Re-sync required: **no**.
- Next: merge into integration, refresh the root fixture screenshots, and rerun the final frontend gates together with the incoming API serializer follow-up.

### Validation

- Added a regression that failed against the previous 1350px budget, then passed after the fix; it also preserves `184,467,440,737.09551615 CKB` and `3,500,000,000` cycles.
- All 50 focused tests across the pool page, transaction table, and exact activity amount component passed.
- Fixture Chromium at 1440px: pool table clientWidth and scrollWidth both 1248, every cell fits, and the cycles column ends at the container boundary. At 390px, document scrollWidth is 390. Both runs had zero page errors.
- Logs: `.worktrees/site-redesign-perf-results/frontend-pool-width-{red,green,visual}.log`. Screenshots: `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/pool-width-lane-{1440,390}.png`.
