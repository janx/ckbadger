# Latest Activities Stream Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the grouped-by-tx activity display with a mixed event stream where each activity type (DAO, token, object, identity, script call, CKB transfer) gets a distinct visual layout.

**Architecture:** Pure frontend rewrite. The existing `GET /activities/latest?limit=32` API returns `GlobalActivity[]` (one per owner-action, cellbase excluded). A new `classifyActivity()` pure function classifies each item into one of 8 visual types by priority. The stream component renders each item with a type-specific layout, using smooth slide-in animation for new items.

**Tech Stack:** React 19, TanStack Query v5, Tailwind CSS 3.4, Vitest, @testing-library/react

**Design doc:** `docs/plans/2026-03-13-latest-activities-redesign-design.md`

---

### Task 1: Create `classifyActivity()` with Tests

**Files:**

- Create: `frontend/lib/activity-classify.ts`
- Create: `frontend/__tests__/lib/activity-classify.test.ts`

**Step 1: Write the failing tests**

Create `frontend/__tests__/lib/activity-classify.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import type { GlobalActivity } from '@/lib/api';
import { classifyActivity, type ActivityType } from '@/lib/activity-classify';

function makeActivity(overrides: Partial<GlobalActivity> = {}): GlobalActivity {
  return {
    address: overrides.address ?? 'ckb1qtest',
    txHash: overrides.txHash ?? '0xtx',
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    assetChanges: overrides.assetChanges ?? [],
    scriptCalls: overrides.scriptCalls ?? [],
    peers: overrides.peers ?? [],
  };
}

describe('classifyActivity', () => {
  it('classifies DAO deposit', () => {
    const result = classifyActivity(
      makeActivity({ assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }] })
    );
    expect(result.type).toBe('daoDeposit');
  });

  it('classifies DAO withdraw request', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [{ type: 'daoWithdrawRequest', capacity: '10200000000', depositBlock: 100 }],
      })
    );
    expect(result.type).toBe('daoWithdrawRequest');
  });

  it('classifies DAO withdraw complete', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'daoWithdrawComplete', capacity: '10200000000', compensation: '42000000' },
        ],
      })
    );
    expect(result.type).toBe('daoWithdrawComplete');
  });

  it('classifies token transfer', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xtoken', delta: '500', symbol: 'SEAL', decimals: 8 },
        ],
      })
    );
    expect(result.type).toBe('token');
  });

  it('classifies object action', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [{ type: 'object', objectId: '0xspore', standard: 'spore', action: 'mint' }],
      })
    );
    expect(result.type).toBe('object');
  });

  it('classifies identity action', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'identity', identityId: '0xdotbit', standard: 'dotbit', action: 'update' },
        ],
      })
    );
    expect(result.type).toBe('identity');
  });

  it('classifies script call', () => {
    const result = classifyActivity(
      makeActivity({
        scriptCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Omnilock',
          },
        ],
      })
    );
    expect(result.type).toBe('scriptCall');
  });

  it('classifies CKB transfer as fallback', () => {
    const result = classifyActivity(makeActivity({ ckbDelta: '-50000000000' }));
    expect(result.type).toBe('ckbTransfer');
  });

  it('DAO deposit takes priority over token in same activity', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
          { type: 'daoDeposit', capacity: '10200000000' },
        ],
      })
    );
    expect(result.type).toBe('daoDeposit');
  });

  it('token takes priority over script call', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [
          { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
        ],
        scriptCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Omnilock',
          },
        ],
      })
    );
    expect(result.type).toBe('token');
  });

  it('returns the first matching asset change for the classified type', () => {
    const result = classifyActivity(
      makeActivity({
        assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
      })
    );
    expect(result.type).toBe('daoDeposit');
    expect(result.primaryAssetChange).toEqual({ type: 'daoDeposit', capacity: '10200000000' });
  });
});
```

**Step 2: Run tests to verify they fail**

Run: `cd frontend && npx vitest run __tests__/lib/activity-classify.test.ts`
Expected: FAIL — module `@/lib/activity-classify` does not exist

**Step 3: Write the implementation**

Create `frontend/lib/activity-classify.ts`:

```ts
import type { ActivityAssetChange, ActivityScriptCall, GlobalActivity } from '@/lib/api';

export type ActivityType =
  | 'daoDeposit'
  | 'daoWithdrawRequest'
  | 'daoWithdrawComplete'
  | 'token'
  | 'object'
  | 'identity'
  | 'scriptCall'
  | 'ckbTransfer';

export interface ClassifiedActivity {
  type: ActivityType;
  activity: GlobalActivity;
  primaryAssetChange: ActivityAssetChange | null;
  primaryScriptCall: ActivityScriptCall | null;
}

const ASSET_TYPE_PRIORITY: Array<{ assetType: string; activityType: ActivityType }> = [
  { assetType: 'daoDeposit', activityType: 'daoDeposit' },
  { assetType: 'daoWithdrawRequest', activityType: 'daoWithdrawRequest' },
  { assetType: 'daoWithdrawComplete', activityType: 'daoWithdrawComplete' },
  { assetType: 'token', activityType: 'token' },
  { assetType: 'object', activityType: 'object' },
  { assetType: 'identity', activityType: 'identity' },
];

export function classifyActivity(activity: GlobalActivity): ClassifiedActivity {
  for (const { assetType, activityType } of ASSET_TYPE_PRIORITY) {
    const match = activity.assetChanges.find((c) => c.type === assetType);
    if (match) {
      return {
        type: activityType,
        activity,
        primaryAssetChange: match,
        primaryScriptCall: activity.scriptCalls[0] ?? null,
      };
    }
  }

  if (activity.scriptCalls.length > 0) {
    return {
      type: 'scriptCall',
      activity,
      primaryAssetChange: null,
      primaryScriptCall: activity.scriptCalls[0],
    };
  }

  return {
    type: 'ckbTransfer',
    activity,
    primaryAssetChange: null,
    primaryScriptCall: null,
  };
}
```

**Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run __tests__/lib/activity-classify.test.ts`
Expected: All 10 tests PASS

**Step 5: Commit**

```bash
git add frontend/lib/activity-classify.ts frontend/__tests__/lib/activity-classify.test.ts
git commit -m "feat: add classifyActivity() for activity stream type detection"
```

---

### Task 2: Rewrite `latest-activities.tsx` — Stream Component

**Files:**

- Rewrite: `frontend/components/latest-activities.tsx`
- Reference: `frontend/lib/activity-classify.ts` (from Task 1)
- Reference: `frontend/lib/detail-routes.ts` (existing, for `getScriptDetailHref`, `getObjectDetailHref`, `getIdentityItemDetailHref`, `getTokenDetailHref`)
- Reference: `frontend/lib/utils.ts` (existing, for `formatTimeAgo`, `formatCkbAmount`, `truncateHash`, `cn`)
- Reference: `frontend/components/ui/terminal-panel.tsx` (existing, for `TerminalPanel`, `TerminalPanelHeader`, `TerminalPanelContent`)
- Reference: `frontend/components/ui/hex-display.tsx` (existing)

**Step 1: Write the new component**

Rewrite `frontend/components/latest-activities.tsx` with the following structure. The component:

- Fetches `api.getLatestActivities(32)` via TanStack Query (10s poll)
- Classifies each activity using `classifyActivity()`
- Renders each item with a type-specific sub-renderer
- Tracks previous items by `txHash+address` key for new-item detection
- Applies `animate-slide-in` CSS class + jade glow to new items

Key implementation details:

**Type badge colors and icons** (use Unicode characters for icons, Tailwind color classes for accents):

| Type                | Icon | Label                                     | Color class      |
| ------------------- | ---- | ----------------------------------------- | ---------------- |
| daoDeposit          | `◆`  | DAO Deposit                               | `text-gold`      |
| daoWithdrawRequest  | `◆`  | DAO Withdraw Request                      | `text-gold`      |
| daoWithdrawComplete | `◆`  | DAO Withdraw Complete                     | `text-positive`  |
| token               | `◎`  | `{symbol} Transfer`                       | `text-[#ff66aa]` |
| object              | `⬡`  | `{standard} {action}` (e.g. "Spore Mint") | `text-lavender`  |
| identity            | `✦`  | `{standard} {action}` (e.g. ".bit Renew") | `text-aqua`      |
| scriptCall          | `⚙`  | `Script: {name}`                          | `text-amber`     |
| ckbTransfer         | `↗`  | CKB Transfer                              | `text-jade`      |

**Each type-specific renderer** is a small function component receiving `ClassifiedActivity`. All share the same outer structure:

- Line 1: icon + label (left) | time ago (right)
- Line 2+: type-specific content

**Address display**: Use existing `truncateHash` for hex addresses. For CKB addresses (`ckb1...`/`ckt1...`), truncate as `{first 8}...{last 6}`.

**CKB delta display**: Use existing `formatCkbAmount()`. Color: `text-positive` for positive, `text-negative` for negative, `text-text-dim` for zero.

**Animation**: New items get a CSS class `animate-slide-in` that does `translateY(-100%) -> translateY(0)` over 300ms, and a `bg-jade/10` glow that fades over 2s. Use `useRef` to track previous item keys and `useEffect` to detect new ones on data change.

**Item limit**: Show up to 20 items. No "VIEW ALL" link change needed (keep existing `/activities` link).

**Step 2: Run type-check**

Run: `cd frontend && pnpm type-check`
Expected: PASS (no type errors)

**Step 3: Run lint**

Run: `cd frontend && pnpm lint`
Expected: PASS

**Step 4: Commit**

```bash
git add frontend/components/latest-activities.tsx
git commit -m "feat: rewrite latest-activities as mixed event stream with type-specific layouts"
```

---

### Task 3: Rewrite `latest-activities.test.tsx`

**Files:**

- Rewrite: `frontend/__tests__/components/latest-activities.test.tsx`

**Step 1: Write new tests**

The old tests validated grouping behavior (grouped by tx, max 3 participants, "+N more"). The new tests validate stream rendering (individual items, type-specific rendering).

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestActivities } from '@/components/latest-activities';
import type { GlobalActivity } from '@/lib/api';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getLatestActivities: vi.fn(),
  },
}));

function makeActivity(
  overrides: Partial<GlobalActivity> & Pick<GlobalActivity, 'address' | 'txHash'>
): GlobalActivity {
  return {
    address: overrides.address,
    txHash: overrides.txHash,
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    assetChanges: overrides.assetChanges ?? [],
    scriptCalls: overrides.scriptCalls ?? [],
    peers: overrides.peers ?? [],
  };
}

describe('LatestActivities stream', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders each activity as a separate stream item (no tx grouping)', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qsender11111111111111111111111111111111111111111',
        txHash: '0xtx-shared',
        ckbDelta: '-10000000000',
      }),
      makeActivity({
        address: 'ckb1qreceiver111111111111111111111111111111111111111',
        txHash: '0xtx-shared',
        ckbDelta: '10000000000',
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      const addressLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/address/'));
      expect(addressLinks).toHaveLength(2);
    });
  });

  it('renders a DAO deposit with the DAO Deposit label', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qdao111111111111111111111111111111111111111111111',
        txHash: '0xtx-dao',
        ckbDelta: '-10200000000',
        assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('DAO Deposit')).toBeInTheDocument();
    });
  });

  it('renders a token transfer with the token symbol', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qtoken1111111111111111111111111111111111111111111',
        txHash: '0xtx-token',
        assetChanges: [
          { type: 'token', typeScriptHash: '0xtoken', delta: '1200', symbol: 'SEAL', decimals: 8 },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('SEAL Transfer')).toBeInTheDocument();
    });
  });

  it('renders a script call with the script name', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qscript1111111111111111111111111111111111111111111',
        txHash: '0xtx-script',
        scriptCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Omnilock',
          },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/Script: Omnilock/)).toBeInTheDocument();
    });
  });

  it('renders CKB transfer for activities with no assets and no script calls', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qtransfer11111111111111111111111111111111111111111',
        txHash: '0xtx-ckb',
        ckbDelta: '-50000000000',
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('CKB Transfer')).toBeInTheDocument();
    });
  });

  it('limits visible items to 20', async () => {
    const activities = Array.from({ length: 25 }, (_, i) =>
      makeActivity({
        address: `ckb1qaddr${String(i).padStart(40, '0')}`,
        txHash: `0xtx-${i}`,
        blockNumber: 11_000 - i,
        ckbDelta: '100000000',
      })
    );

    vi.mocked(api.getLatestActivities).mockResolvedValue(activities);

    render(<LatestActivities />);

    await waitFor(() => {
      const addressLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/address/'));
      expect(addressLinks).toHaveLength(20);
    });
  });

  it('shows skeleton while loading', () => {
    vi.mocked(api.getLatestActivities).mockReturnValue(new Promise(() => {}));

    render(<LatestActivities />);

    expect(screen.getByTestId('latest-activities-content')).toBeInTheDocument();
  });
});
```

**Step 2: Run tests**

Run: `cd frontend && npx vitest run __tests__/components/latest-activities.test.tsx`
Expected: All 7 tests PASS

**Step 3: Commit**

```bash
git add frontend/__tests__/components/latest-activities.test.tsx
git commit -m "test: rewrite latest-activities tests for stream rendering"
```

---

### Task 4: Delete `latest-activity-groups.ts` and Its Tests

**Files:**

- Delete: `frontend/lib/latest-activity-groups.ts`
- Delete: `frontend/__tests__/lib/latest-activity-groups.test.ts`

**Step 1: Verify no other imports remain**

Run: `cd frontend && grep -r "latest-activity-groups" --include='*.ts' --include='*.tsx' .`
Expected: Only the two files being deleted (the import in `latest-activities.tsx` was already removed in Task 2)

**Step 2: Delete the files**

```bash
rm frontend/lib/latest-activity-groups.ts frontend/__tests__/lib/latest-activity-groups.test.ts
```

**Step 3: Run full test suite to verify nothing breaks**

Run: `cd frontend && npx vitest run`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add -u frontend/lib/latest-activity-groups.ts frontend/__tests__/lib/latest-activity-groups.test.ts
git commit -m "chore: remove latest-activity-groups (replaced by activity-classify)"
```

---

### Task 5: Final Validation

**Step 1: Type-check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 2: Lint**

Run: `cd frontend && pnpm lint`
Expected: PASS

**Step 3: Full test suite**

Run: `cd frontend && npx vitest run`
Expected: All tests PASS

**Step 4: Format**

Run: `pnpm format`
Expected: Files formatted

**Step 5: Commit any formatting changes**

```bash
git add -A && git diff --cached --quiet || git commit -m "style: format"
```

**Step 6: Visual check (optional if dev server available)**

Run: `cd frontend && pnpm dev`
Verify: Homepage shows mixed event stream with type-specific layouts, new items slide in.
