# Adaptive Table/List Layout Design

## Goal

Make all tables and lists responsive within the existing container width (~1280px max), adapting layout from mobile to desktop with card-to-table transitions and progressive column disclosure.

## Principle Alignment

- CKB Native: N/A (pure frontend layout concern)
- Local First: N/A
- Agent Friendly: Clean responsive patterns make pages parseable at any viewport

## Core Strategy

Each table has a **card breakpoint** below which rows render as stacked cards, and above which they render as a traditional table row. Breakpoint is chosen per-table based on column count. Above the breakpoint, columns use flexible widths with bonus columns shown progressively at wider breakpoints.

## Breakpoint Assignments

| Table                | Cols | Card below    | Bonus cols at wider bp       |
| -------------------- | ---- | ------------- | ---------------------------- |
| Blocks               | 4    | `sm` (640px)  | Longer hash truncation at lg |
| Transactions         | 3    | `sm`          | Block number inline at lg    |
| Scripts              | 5    | `md` (768px)  | Wider description at lg      |
| Address holdings     | 3    | `sm`          | --                           |
| Address activities   | 5    | `md`          | Wider assets col at lg       |
| Address transactions | 6    | `lg` (1024px) | Size/Cycles at xl (existing) |
| Assets               | 7    | `lg`          | Occupied col hidden below xl |
| Forks                | 7    | `lg`          | Full tip hashes at xl        |
| DAO deposits         | 6    | `md`          | Wider compensation at lg     |

## Card Layout Pattern

Below the card breakpoint, each TerminalRow renders a stacked card:

```
+-------------------------------+
| #14,230,567            2m ago |  <- primary + time
| 0x3a4f...8b2c                |  <- hash/identifier
| 5 txs                        |  <- secondary info
+-------------------------------+
```

Rules:

- Row 1: Primary identifier (left) + time (right-aligned)
- Row 2+: Secondary fields as inline values or badges
- Same TerminalRow component, different inner layout via responsive Tailwind classes
- Table header row hidden in card mode

## Table Mode (Above Breakpoint)

Replace fixed `w-*` with flex-proportional widths:

```tsx
// Before (rigid):
<div className="w-32">Block</div>
<div className="flex-1">Hash</div>
<div className="w-20">Txs</div>
<div className="w-32">Time</div>

// After (flexible):
<div className="w-24 shrink-0">Block</div>
<div className="min-w-0 flex-1">Hash</div>
<div className="w-16 shrink-0">Txs</div>
<div className="w-24 shrink-0">Time</div>
```

## Progressive Column Disclosure

Extra columns shown only at wider breakpoints:

```tsx
<div className="hidden w-28 shrink-0 lg:block">Size/Cycles</div>
```

- Assets: Hide Occupied (CKB) below xl
- Forks: Show only block numbers for Old/New Tip below xl, full hashes at xl+
- Address transactions: Keep existing hidden xl:block on Size/Cycles
- Scripts: Description truncates more aggressively below lg

## Implementation Pattern

No new components. CSS-only changes using dual-render with responsive visibility:

```tsx
<TerminalRow>
  {/* Table layout (md+) */}
  <div className="hidden items-center md:flex">
    <div className="w-24 shrink-0">...</div>
    <div className="min-w-0 flex-1">...</div>
  </div>
  {/* Card layout (<md) */}
  <div className="space-y-1 md:hidden">
    <div className="flex justify-between">...</div>
  </div>
</TerminalRow>
```

## Pages NOT Changed

- Home page (LatestBlocks, LatestTransactions, LatestActivities): Already card-like
- Block detail (Inputs/Outputs, Scripts, Cell Deps): Already vertical detail views
- Address cells grid: Already responsive with grid-cols-1/2/3

## Scope

~10 files. No new components, no new dependencies. Remove horizontal scroll + min-width patterns (e.g., Assets `min-w-[1040px]`) in favor of card/table hybrid. Interaction behavior stays identical to desktop (links navigate to same detail pages).
