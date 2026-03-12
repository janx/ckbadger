# Homepage Latest Activities Locked Height Design

## Goal

- Make the homepage `Latest Activities` card render exactly 5 activity groups inside a fixed-height card with no scrollbar, clipping any overflow beyond the locked height.

## Principle Alignment

- CKB Native: keep the transaction-grouped activity presentation introduced previously, without changing activity semantics or synthesizing new transaction interpretations.
- Local First: keep the change fully in the frontend card layout; no API, store, or schema changes.
- Agent Friendly: constrain the behavior through simple constants and component tests rather than dynamic measurement or layout heuristics.

## Context

- The current grouped implementation in `frontend/components/latest-activities.tsx` renders at most 4 activity groups.
- The card height is currently content-driven, so its visual footprint changes with the activity mix.
- The requested behavior is stricter:
  - show 5 activity groups
  - lock the card height
  - hide overflow
  - never show a scrollbar

## Decision

- Keep the existing grouped transaction layout.
- Increase the displayed group cap from 4 to 5.
- Apply the fixed-height constraint only in `LatestActivities`, not in shared `TerminalPanel`.
- Use `overflow-hidden` on the card content region so anything beyond the fixed height is clipped.

## Rejected Alternatives

- Dynamic content measurement:
  - rejected as unnecessary complexity for a simple, deterministic homepage card
- Modifying shared `TerminalPanel`:
  - rejected because the behavior is specific to this card and should not leak into other homepage panels
- Per-row equal-height slots:
  - rejected because it would force tighter compression inside every group and degrade readability

## UI Behavior

- The card renders only the first 5 grouped transactions.
- The card gets a fixed total height, tuned for the homepage row layout.
- The inner content area is non-scrollable and clips anything beyond the available vertical space.
- No scrollbar is shown on desktop or mobile.

## Scope

- Expected implementation files:
  - `frontend/components/latest-activities.tsx`
  - `frontend/__tests__/components/latest-activities.test.tsx`

- No changes to:
  - `frontend/components/ui/terminal-panel.tsx`
  - API routes
  - store or schema

## Testing

- Update component tests to assert:
  - 5 activity groups can render
  - a 6th group is not rendered
  - the component keeps using the grouped transaction model

- Add a regression check that the card content container uses `overflow-hidden`.

## Risks

- A tall fifth group may be partially clipped at the bottom; this is acceptable because the user explicitly requested locked height with clipped overflow.
- The ideal fixed height may need one round of visual tuning if the adjacent homepage card changes later.

## Result

- The homepage `Latest Activities` card becomes a stable-height panel with a predictable 5-group limit and no scrollbars.
