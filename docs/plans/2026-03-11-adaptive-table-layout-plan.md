# Adaptive Table/List Layout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make all list-page tables responsive with card layout on narrow screens, flexible columns on desktop, and progressive column disclosure at wider breakpoints.

**Architecture:** Each table gets a dual-render pattern — a `hidden {bp}:flex` table layout and a `{bp}:hidden` card layout inside each `TerminalRow`. Header rows are hidden below the card breakpoint. No new components, pure Tailwind responsive classes. Remove existing horizontal-scroll patterns.

**Tech Stack:** Tailwind CSS responsive classes, existing TerminalRow/TerminalPanel components.

**Design doc:** `docs/plans/2026-03-11-adaptive-table-layout-design.md`

---

### Task 1: Blocks Page — Card + Flexible Table

**Files:**

- Modify: `frontend/app/blocks/page.tsx`

The Blocks page has 4 columns (Block, Hash, Txs, Time). Card breakpoint: `sm` (640px).

**Step 1: Modify the header row to hide below sm**

In `frontend/app/blocks/page.tsx`, find the header div (the one with `text-text-muted flex border-b`) and add `hidden sm:flex` to hide it on mobile:

```tsx
<div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider sm:flex">
  <div className="w-24 shrink-0">Block</div>
  <div className="min-w-0 flex-1">Hash</div>
  <div className="w-16 shrink-0 text-center">Txs</div>
  <div className="w-24 shrink-0 text-right">Time</div>
</div>
```

Note: Column widths change from `w-32/flex-1/w-20/w-32` to `w-24/flex-1/w-16/w-24` (more flexible).

**Step 2: Add dual-render to each data row**

Replace the inner `<div className="flex items-center">` inside each `TerminalRow` with:

```tsx
<TerminalRow key={block.number}>
  {/* Table layout (sm+) */}
  <div className="hidden items-center sm:flex">
    <div className="w-24 shrink-0">
      <Link href={`/blocks/${block.number}`} className="text-emphasis font-mono hover:underline">
        #{block.number.toLocaleString()}
      </Link>
      {block.hardforkActivation && (
        <div className="mt-1">
          <Badge variant="amber" className="text-[10px]">
            HF · {block.hardforkActivation.shortName.toUpperCase()}
          </Badge>
        </div>
      )}
    </div>
    <div className="min-w-0 flex-1">
      <Link href={`/blocks/${block.hash}`} className="hover:underline">
        <HexDisplay value={block.hash} startChars={12} endChars={8} />
      </Link>
    </div>
    <div className="text-warning w-16 shrink-0 text-center font-mono">
      {block.transactionsCount}
    </div>
    <div className="text-text-muted w-24 shrink-0 text-right">{formatTimeAgo(block.timestamp)}</div>
  </div>
  {/* Card layout (<sm) */}
  <div className="space-y-1 sm:hidden">
    <div className="flex items-center justify-between gap-2">
      <Link href={`/blocks/${block.number}`} className="text-emphasis font-mono hover:underline">
        #{block.number.toLocaleString()}
      </Link>
      <span className="text-text-muted text-xs">{formatTimeAgo(block.timestamp)}</span>
    </div>
    <Link href={`/blocks/${block.hash}`} className="block hover:underline">
      <HexDisplay value={block.hash} startChars={10} endChars={6} />
    </Link>
    <div className="flex items-center gap-2">
      <span className="text-warning font-mono text-xs">{block.transactionsCount} txs</span>
      {block.hardforkActivation && (
        <Badge variant="amber" className="text-[10px]">
          HF · {block.hardforkActivation.shortName.toUpperCase()}
        </Badge>
      )}
    </div>
  </div>
</TerminalRow>
```

**Step 3: Update loading skeleton to match dual-render**

Replace the loading skeleton with responsive versions:

```tsx
<TerminalRow key={i} hoverable={false}>
  {/* Skeleton: table (sm+) */}
  <div className="hidden animate-pulse items-center sm:flex">
    <div className="w-24 shrink-0">
      <div className="bg-base-elevated h-4 w-16 rounded" />
    </div>
    <div className="min-w-0 flex-1">
      <div className="bg-base-elevated h-4 w-48 rounded" />
    </div>
    <div className="w-16 shrink-0 text-center">
      <div className="bg-base-elevated mx-auto h-4 w-8 rounded" />
    </div>
    <div className="w-24 shrink-0 text-right">
      <div className="bg-base-elevated ml-auto h-4 w-16 rounded" />
    </div>
  </div>
  {/* Skeleton: card (<sm) */}
  <div className="animate-pulse space-y-2 sm:hidden">
    <div className="flex justify-between">
      <div className="bg-base-elevated h-4 w-20 rounded" />
      <div className="bg-base-elevated h-3 w-14 rounded" />
    </div>
    <div className="bg-base-elevated h-3 w-40 rounded" />
  </div>
</TerminalRow>
```

**Step 4: Verify visually**

Run: `cd frontend && pnpm dev`
Check `/blocks` at 500px, 700px, 1200px widths in browser devtools.

**Step 5: Commit**

```bash
git add frontend/app/blocks/page.tsx
git commit -m "feat(ui): adaptive layout for blocks list — card below sm, flexible columns"
```

---

### Task 2: Transactions Page — Card + Flexible Table

**Files:**

- Modify: `frontend/app/transactions/page.tsx`

3 columns (Transaction, In/Out, Time). Card breakpoint: `sm` (640px).

**Step 1: Hide header below sm, adjust column widths**

```tsx
<div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider sm:flex">
  <div className="min-w-0 flex-1">Transaction</div>
  <div className="w-20 shrink-0 text-center">In/Out</div>
  <div className="w-24 shrink-0 text-right">Time</div>
</div>
```

**Step 2: Dual-render for data rows**

```tsx
<TerminalRow key={tx.hash}>
  {/* Table (sm+) */}
  <div className="hidden items-center sm:flex">
    <div className="min-w-0 flex-1">
      <Link href={`/tx/${tx.hash}`} className="hover:underline">
        <HexDisplay value={tx.hash} startChars={12} endChars={8} />
      </Link>
      <Link
        href={`/blocks/${tx.blockNumber}`}
        className="text-emphasis block font-mono text-xs hover:underline"
      >
        #{formattedNumbers.get(tx.hash)}
      </Link>
    </div>
    <div className="text-text-muted w-20 shrink-0 text-center font-mono">
      <span className="text-emphasis-dim">{tx.inputsCount}</span>
      <span className="text-text-muted mx-1">→</span>
      <span className="text-emphasis-dim">{tx.outputsCount}</span>
    </div>
    <div className="text-text-muted w-24 shrink-0 text-right">{formatTimeAgo(tx.timestamp)}</div>
  </div>
  {/* Card (<sm) */}
  <div className="space-y-1 sm:hidden">
    <div className="flex items-center justify-between gap-2">
      <Link href={`/tx/${tx.hash}`} className="hover:underline">
        <HexDisplay value={tx.hash} startChars={10} endChars={6} />
      </Link>
      <span className="text-text-muted shrink-0 text-xs">{formatTimeAgo(tx.timestamp)}</span>
    </div>
    <div className="flex items-center justify-between">
      <Link
        href={`/blocks/${tx.blockNumber}`}
        className="text-emphasis font-mono text-xs hover:underline"
      >
        #{formattedNumbers.get(tx.hash)}
      </Link>
      <span className="text-text-muted font-mono text-xs">
        <span className="text-emphasis-dim">{tx.inputsCount}</span> →{' '}
        <span className="text-emphasis-dim">{tx.outputsCount}</span>
      </span>
    </div>
  </div>
</TerminalRow>
```

**Step 3: Update skeleton similarly, verify at 500px/700px/1200px**

**Step 4: Commit**

```bash
git add frontend/app/transactions/page.tsx
git commit -m "feat(ui): adaptive layout for transactions list — card below sm, flexible columns"
```

---

### Task 3: Scripts Page — Card below md, progressive description

**Files:**

- Modify: `frontend/app/scripts/page.tsx`

5 columns (Script, Kind, Description, Occupied, Capacity). Card breakpoint: `md` (768px).

**Step 1: Hide header below md, use flexible widths**

```tsx
<div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:flex">
  {renderSortHeader('name', 'Script', 'w-40 shrink-0')}
  {renderSortHeader('kind', 'Kind', 'w-16 shrink-0')}
  {renderSortHeader('description', 'Description', 'min-w-0 flex-1 px-4')}
  {renderSortHeader('occupied', 'Occupied (CKB)', 'w-28 shrink-0', 'right')}
  {renderSortHeader('capacity', 'Capacity (CKB)', 'w-28 shrink-0', 'right')}
</div>
```

**Step 2: Dual-render for rows**

Table mode (md+): Keep current layout with `hidden md:flex`. Card mode (<md):

```tsx
{
  /* Card (<md) */
}
<div className="space-y-1.5 md:hidden">
  <div className="flex items-center justify-between gap-2">
    {/* Script name link */}
    <AppLink href={getScriptHref(script)} className="text-emphasis font-medium hover:underline">
      {hasKnownScriptName(script.name) ? script.name!.trim() : UNLABELED_SCRIPT_LABEL}
    </AppLink>
    {script.scriptKind && (
      <Badge variant={script.scriptKind === 'lock' ? 'blue' : 'purple'}>{script.scriptKind}</Badge>
    )}
  </div>
  {hasKnownScriptName(script.name) && script.description && (
    <div className="text-text-muted line-clamp-2 text-xs">{script.description}</div>
  )}
  {!hasKnownScriptName(script.name) && (
    <div className="text-text-muted font-mono text-xs">{getScriptRefDisplay(script)}</div>
  )}
  <div className="text-text-secondary flex items-center gap-4 font-mono text-xs">
    <span>Occupied: {formatCkbCompact(script.liveOccupiedCapacitySum || '0').value}</span>
    <span>Capacity: {formatCkbCompact(script.liveCapacitySum || '0').value}</span>
  </div>
</div>;
```

**Step 3: Update skeleton, verify at 600px/800px/1200px**

**Step 4: Commit**

```bash
git add frontend/app/scripts/page.tsx
git commit -m "feat(ui): adaptive layout for scripts list — card below md"
```

---

### Task 4: Forks Page — Card below lg, progressive tip hashes

**Files:**

- Modify: `frontend/app/forks/page.tsx`

7 columns (Event, Depth, Fork Point, Old Tip, New Tip, Orphaned, Time). Card breakpoint: `lg` (1024px).

**Step 1: Hide header below lg**

```tsx
<div className="... hidden lg:flex">
```

**Step 2: Table mode (lg+) — flexible columns**

Keep the existing layout but wrap in `hidden lg:flex`. Old/New Tip hash `HexDisplay` shows more chars at xl:

```tsx
<HexDisplay value={event.oldTipHash} size="sm" startChars={8} endChars={6} />
```

**Step 3: Card mode (<lg)**

```tsx
<div className="space-y-1.5 lg:hidden">
  <div className="flex items-center justify-between gap-2">
    <Link href={`/forks/${event.id}`}>
      <Badge variant={getBadgeVariant(event.eventType)}>{event.eventType.toUpperCase()}</Badge>
    </Link>
    <span className="text-text-muted text-xs">{formatTimeAgo(event.detectedAt)}</span>
  </div>
  <div className="flex items-center gap-4 text-xs">
    <span className="text-text-secondary">
      Depth: <span className="text-warning font-mono">{event.depth}</span>
    </span>
    <Link
      href={`/blocks/${event.forkPointNumber}`}
      className="text-emphasis font-mono hover:underline"
    >
      Fork #{event.forkPointNumber.toLocaleString()}
    </Link>
  </div>
  <div className="flex items-center gap-4 text-xs">
    <span className="text-text-muted">
      Old:{' '}
      <span className="text-text-secondary font-mono">#{event.oldTipNumber.toLocaleString()}</span>
    </span>
    <span className="text-text-muted">
      New:{' '}
      <span className="text-text-secondary font-mono">#{event.newTipNumber.toLocaleString()}</span>
    </span>
  </div>
  <div className="text-negative text-xs">
    {event.orphanedBlocksCount} blocks, {event.orphanedTxsCount} txs orphaned
  </div>
</div>
```

**Step 4: Update skeleton, verify at 800px/1100px/1280px**

**Step 5: Commit**

```bash
git add frontend/app/forks/page.tsx
git commit -m "feat(ui): adaptive layout for forks list — card below lg, progressive tip hashes"
```

---

### Task 5: Assets Page — Card below lg, remove horizontal scroll

**Files:**

- Modify: `frontend/app/assets/assets-page-client.tsx`

7 columns (Name, Standard, Items?, 24h Txns, Holders, Occupied, Capacity). Card breakpoint: `lg` (1024px). This is the most complex table.

**Step 1: Remove horizontal scroll infrastructure**

Delete:

- `tableMinWidthClass` variable and its `min-w-[1040px]`/`min-w-[1120px]` usage
- The `overflow-x-auto` wrapper div and the inner `tableMinWidthClass` div
- The "Swipe horizontally" hint divs (both in loading and data states)

Replace with direct rendering inside `TerminalPanelContent`.

**Step 2: Hide header below lg**

```tsx
<div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider lg:flex">
  {renderSortHeader(
    'name',
    assetType === 'token' ? 'Token' : 'Collection',
    'min-w-0 flex-[2_0_10rem] pr-4'
  )}
  {renderSortHeader('type', 'Standard', 'w-20 shrink-0')}
  {assetType !== 'token' && renderSortHeader('supply', 'Items', 'w-20 shrink-0', 'right')}
  {renderSortHeader('transfers24h', '24h Txns', 'w-20 shrink-0', 'right')}
  {renderSortHeader('holders', 'Holders', 'w-24 shrink-0', 'right')}
  <div className="hidden xl:block">
    {renderSortHeader('occupied', 'Occupied', 'w-28 shrink-0', 'right')}
  </div>
  {renderSortHeader('capacity', 'Capacity', 'w-28 shrink-0', 'right')}
</div>
```

Note: `Occupied` column hidden below xl (progressive disclosure).

**Step 3: Dual-render for rows**

Table (lg+): Similar to current but with `hidden lg:flex` and the Occupied column wrapped in `hidden xl:block`.

Card (<lg):

```tsx
<div className="space-y-1.5 lg:hidden">
  <div className="flex items-center gap-2">
    <AppLink href={getAssetLink(asset)} className="min-w-0 flex-1">
      <div className="flex items-center gap-2">
        {/* icon */}
        <span className="text-emphasis truncate font-medium hover:underline">{getAssetName(asset)}</span>
        {asset.published && /* verified icon */}
      </div>
      <HexDisplay value={asset.id} size="sm" startChars={8} endChars={6} />
    </AppLink>
    <Badge variant="neutral">{getTypeBadgeLabel(asset)}</Badge>
  </div>
  <div className="text-text-muted flex flex-wrap items-center gap-x-4 gap-y-1 font-mono text-xs tabular-nums">
    <span>24h: <span className="text-warning">{formatNumber(asset.transfers24h)}</span></span>
    <span>Holders: {formatNumber(asset.holdersCount)}</span>
    {assetType !== 'token' && <span>Items: {formatNumber(asset.totalSupply || 0)}</span>}
    <span>Cap: {formatCkbCompact(asset.liveCapacity || '0').value}</span>
  </div>
</div>
```

**Step 4: Update loading skeleton to dual-render, remove swipe hint from skeleton too**

**Step 5: Verify at 800px/1100px/1280px**

**Step 6: Commit**

```bash
git add frontend/app/assets/assets-page-client.tsx
git commit -m "feat(ui): adaptive layout for assets list — card below lg, remove horizontal scroll"
```

---

### Task 6: Address Holdings — Card below sm

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx` (holdings section only)

3 columns (Asset, Standard, Balance). Card breakpoint: `sm` (640px).

**Step 1: Hide header below sm**

Find the holdings header div and add `hidden sm:flex`:

```tsx
<div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider sm:flex">
  <div className="min-w-0 flex-1">Asset</div>
  <div className="w-28 shrink-0">Standard</div>
  <div className="w-44 shrink-0 text-right">Balance</div>
</div>
```

**Step 2: Dual-render for token rows**

Table (sm+): Current layout with `hidden sm:flex`.

Card (<sm):

```tsx
<div className="space-y-1 sm:hidden" onClick={() => handleTokenSelect(isSelected ? null : token)}>
  <div className="flex items-center justify-between gap-2">
    <div className="flex min-w-0 items-center gap-2">
      {/* token icon */}
      <Link
        href={`/tokens/${token.typeScriptHash}`}
        onClick={(e) => e.stopPropagation()}
        className="text-emphasis truncate font-medium hover:underline"
      >
        {tokenDisplayName(token)}
      </Link>
      <Badge variant="gray">{token.standard}</Badge>
    </div>
  </div>
  <div className="text-text-primary text-right font-mono text-sm">
    {formatTokenBalance(token.balance, token.decimals)}
  </div>
</div>
```

**Step 3: Commit**

```bash
git add frontend/app/address/[addr]/client-page.tsx
git commit -m "feat(ui): adaptive layout for address holdings — card below sm"
```

---

### Task 7: Address Activities — Card below md

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx` (activities tab only)

5 columns (Transaction, Type, CKB Change, Assets, Time). Card breakpoint: `md` (768px).

**Step 1: Remove `overflow-x-auto` and `min-w-[640px]` wrappers**

**Step 2: Hide header below md**

```tsx
<div className="border-base-border bg-base-surface/50 text-text-muted hidden items-center gap-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:flex">
```

**Step 3: Dual-render for rows**

Table (md+): Wrap existing layout in `hidden md:flex`.

Card (<md):

```tsx
<div className="space-y-1.5 md:hidden">
  <div className="flex items-center justify-between gap-2">
    <Link href={`/tx/${activity.txHash}`}>
      <HexDisplay value={activity.txHash} truncate startChars={8} endChars={6} />
    </Link>
    <span className="text-text-muted shrink-0 text-xs">
      {formatTimeAgo(Number(activity.timestamp))}
    </span>
  </div>
  <div className="flex items-center justify-between gap-2">
    <div className="flex items-center gap-2">
      {/* Type badge (Coinbase/Received/Sent/Self) */}
      <Link
        href={`/blocks/${activity.blockNumber}`}
        className="text-text-muted font-mono text-xs hover:underline"
      >
        #{activity.blockNumber.toLocaleString()}
      </Link>
    </div>
    <span className={`font-mono text-sm ${deltaColor}`}>
      {isPositive && '+'}
      {formatCkbAmount(activity.ckbDelta).full} CKB
    </span>
  </div>
  {activity.assetChanges.length > 0 && (
    <div className="flex flex-wrap gap-1">
      {activity.assetChanges.map((change, i) => (
        <AssetChangeBadge key={i} change={change} />
      ))}
    </div>
  )}
</div>
```

**Step 4: Commit**

```bash
git add frontend/app/address/[addr]/client-page.tsx
git commit -m "feat(ui): adaptive layout for address activities — card below md"
```

---

### Task 8: Address Transactions — Card below lg

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx` (transactions tab only)

6 columns (Transaction, In/Out, Fee, Size/Cycles, CKB Change, Time). Card breakpoint: `lg` (1024px). Size/Cycles already `hidden xl:block`.

**Step 1: Remove `overflow-x-auto` and `min-w-[700px]` wrappers**

**Step 2: Hide header below lg**

```tsx
<div className="... hidden lg:flex">
```

**Step 3: Table mode (lg+) — wrap existing in `hidden lg:flex`**

**Step 4: Card mode (<lg)**

```tsx
<div className="space-y-1.5 lg:hidden">
  <div className="flex items-center justify-between gap-2">
    <Link href={`/tx/${tx.txHash}`}>
      <HexDisplay value={tx.txHash} truncate startChars={8} endChars={6} />
    </Link>
    <span className="text-text-muted shrink-0 text-xs">{formatTimeAgo(Number(tx.timestamp))}</span>
  </div>
  <div className="flex items-center justify-between gap-2">
    <div className="text-text-muted flex items-center gap-3 font-mono text-xs">
      <span>
        <span className="text-emphasis-dim">{tx.inputsCount}</span> →{' '}
        <span className="text-emphasis-dim">{tx.outputsCount}</span>
      </span>
      <span>Fee: {formatCkbAmount(tx.fee).full}</span>
    </div>
    <span
      className={`font-mono text-sm ${tx.capacityChange.startsWith('-') ? 'text-negative' : 'text-positive'}`}
    >
      {!tx.capacityChange.startsWith('-') && '+'}
      {formatCkbAmount(tx.capacityChange).full} CKB
    </span>
  </div>
</div>
```

**Step 5: Commit**

```bash
git add frontend/app/address/[addr]/client-page.tsx
git commit -m "feat(ui): adaptive layout for address transactions — card below lg"
```

---

### Task 9: DAO Deposits — Card below md

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx` (DAO deposits section only)

6 columns (Deposit, Status, Capacity, Compensation, Duration, Time). Card breakpoint: `md` (768px).

**Step 1: Hide CSS Grid header below md**

```tsx
<div className="... hidden md:grid" style={{ gridTemplateColumns: '10rem 6rem 1fr 1fr 6rem 5.5rem' }}>
```

**Step 2: Table row (md+) — wrap existing grid in `hidden md:grid`**

**Step 3: Card mode (<md)**

```tsx
<div className="space-y-1.5 md:hidden">
  <div className="flex items-center justify-between gap-2">
    <Link href={`/tx/${deposit.txHash}`}>
      <HexDisplay value={deposit.txHash} truncate startChars={6} endChars={6} />
    </Link>
    {getDaoStatusBadge(deposit.status)}
  </div>
  <div className="flex items-center justify-between gap-2 text-sm">
    <Capacity value={deposit.capacity} className="text-text-primary" />
    {deposit.compensation ? (
      <span className="text-positive font-mono">
        +{formatCkbAmount(deposit.compensation).full} CKB
      </span>
    ) : deposit.status === 'deposited' ? (
      <span className="text-text-muted">Accruing...</span>
    ) : null}
  </div>
  <div className="text-text-muted flex items-center gap-3 text-xs">
    <span>
      {formatDaoDuration(
        deposit.depositTimestamp,
        deposit.withdrawTimestamp || deposit.withdrawRequestTimestamp
      )}
    </span>
    <span>{formatTimeAgo(deposit.depositTimestamp)}</span>
    <Link
      href={`/blocks/${deposit.depositBlockNumber}`}
      className="text-text-muted font-mono hover:underline"
    >
      #{deposit.depositBlockNumber.toLocaleString()}
    </Link>
  </div>
</div>
```

**Step 4: Commit**

```bash
git add frontend/app/address/[addr]/client-page.tsx
git commit -m "feat(ui): adaptive layout for DAO deposits — card below md"
```

---

### Task 10: Visual Verification & Final Commit

**Step 1: Run frontend type-check**

Run: `cd frontend && pnpm type-check`
Expected: No errors.

**Step 2: Run frontend lint**

Run: `cd frontend && pnpm lint`
Expected: No errors.

**Step 3: Run frontend tests**

Run: `cd frontend && npx vitest run`
Expected: All tests pass. If any snapshot tests fail due to class changes, update them.

**Step 4: Manual visual check (all pages at 3 widths)**

Run: `cd frontend && pnpm dev`

Check these URLs at 500px, 800px, 1280px:

- `/blocks` — card below 640px, table above
- `/transactions` — card below 640px, table above
- `/scripts` — card below 768px, table above
- `/forks` — card below 1024px, table above
- `/assets` — card below 1024px, no horizontal scroll
- `/address/<any>` — holdings card below 640px, activities card below 768px, transactions card below 1024px, DAO deposits card below 768px

**Step 5: Format**

Run: `pnpm format`

**Step 6: Final commit if formatting changed anything**

```bash
git add -A
git commit -m "style: format adaptive table layout changes"
```
