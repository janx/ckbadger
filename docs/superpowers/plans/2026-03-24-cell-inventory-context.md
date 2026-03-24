# Cell Inventory Context Card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show an inline summary card of the inventory item a cell represents on the cell detail page.

**Architecture:** A `useInventoryContext(cell)` hook detects item type from `dataAnalysis.deterministic.kind` (with code_hash fallback for DID CKB), extracts the item ID, and a dependent `useQuery` fetches the item detail from existing API endpoints. Per-type card components render inside a `TerminalPanel` between the type script and data sections.

**Tech Stack:** React 19, TanStack Query v5, TypeScript, Vitest, MSW

**Spec:** `docs/superpowers/specs/2026-03-24-cell-inventory-context-design.md`

---

## File Structure

| File | Responsibility |
|---|---|
| `frontend/components/cell/inventory-context.tsx` | New: `useInventoryContext` hook, `InventoryContextSection` entry point, 8 per-type card components |
| `frontend/app/cell/[outpoint]/client-page.tsx` | Modify: import and render `InventoryContextSection` at line ~827 |
| `frontend/__tests__/components/cell/inventory-context.test.tsx` | New: 10 test cases |
| `frontend/__tests__/msw/handlers.ts` | Modify: add mock handlers for inventory API endpoints |

---

### Task 1: Add MSW Mock Handlers for Inventory Endpoints

**Files:**
- Modify: `frontend/__tests__/msw/handlers.ts`

- [ ] **Step 1: Add mock response data and handlers**

Add these handlers to the `handlers` array in `frontend/__tests__/msw/handlers.ts`. Place them after the existing handlers. Use `DEFAULT_API_BASE` which is already imported.

```typescript
// --- Inventory context mock handlers ---

http.get(`${API_BASE}/spore/objects/:sporeId`, () => {
  return HttpResponse.json({
    sporeId: '0xspore123',
    txHash: '0xabc123',
    outputIndex: 0,
    clusterId: '0xcluster456',
    contentType: 'image/png',
    contentSize: 1024,
    ownerLockHash: '0xownerhash789',
    ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xws',
    isLive: true,
    createdAtBlock: 100000,
    ownedCapacity: '14500000000',
    ownedKnowledge: null,
    mediaProfile: {
      tier: 'pure_ckb',
      sources: [],
      hasRenderableImage: true,
      issues: [],
    },
  });
}),

http.get(`${API_BASE}/spore/clusters/:clusterId`, () => {
  return HttpResponse.json({
    clusterId: '0xcluster456',
    name: 'Test Cluster',
    description: 'A test cluster',
    ownerLockHash: '0xownerhash789',
    ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xws',
    sporesCount: 42,
    holdersCount: 15,
    activitiesCount: 100,
    createdAtBlock: 90000,
  });
}),

http.get(`${API_BASE}/tokens/:typeHash`, () => {
  return HttpResponse.json({
    typeScriptHash: '0xtokenhash123',
    typeCodeHash: '0xcode123',
    typeHashType: 'type',
    typeArgs: '0xargs123',
    standard: 'xudt',
    name: 'Test Token',
    symbol: 'TT',
    decimals: 8,
    description: 'A test token',
    iconUrl: null,
    published: true,
    famous: false,
    tags: null,
    udtType: null,
    manager: null,
    email: null,
    operatorWebsite: null,
    totalSupply: '1000000000000000000',
    maximumSupply: null,
    maximumSupplyStatus: 'unlimited',
    holdersCount: 500,
    transfersCount: 10000,
    transfers24h: 50,
    cellsCount: 200,
    totalCapacity: null,
    totalCommonKnowledgeSize: null,
  });
}),

http.get(`${API_BASE}/assets/objects/items/:nftId`, () => {
  return HttpResponse.json({
    nftId: '0xmnft_token_id',
    standard: 'mnft',
    isLive: true,
    ownerLockHash: '0xownerhash789',
    createdAtBlock: 100000,
    tokenIndex: 0,
    characteristicHex: '0x0102030405060708',
    configure: 0,
    state: 0,
    txHash: '0xabc123',
    outputIndex: 0,
    class: {
      classId: '0xclass123',
      issuerId: '0xissuer123',
      name: 'Test NFT Class',
      description: 'A test class',
      renderer: null,
      total: 100,
      issued: 42,
      configure: 0,
    },
    issuer: {
      issuerId: '0xissuer123',
      name: 'Test Issuer',
      classCount: 5,
      setCount: 2,
      infoHex: null,
    },
    lifecycle: [],
  });
}),

http.get(`${API_BASE}/assets/objects/:collectionId`, () => {
  return HttpResponse.json({
    collectionId: '0xclass123',
    standard: 'mnft',
    name: 'Test Collection',
    totalCount: 100,
    liveCount: 90,
    holdersCount: 30,
    activitiesCount: 200,
    ownedCapacity: '50000000000',
    ownedKnowledge: '40000000000',
    classDetail: {
      classId: '0xclass123',
      issuerId: '0xissuer123',
      name: 'Test NFT Class',
      description: 'A test class',
      renderer: null,
      total: 100,
      issued: 42,
      configure: 0,
    },
    issuerDetail: {
      issuerId: '0xissuer123',
      name: 'Test Issuer',
      classCount: 5,
      setCount: 2,
      infoHex: null,
    },
  });
}),

http.get(`${API_BASE}/assets/identities/dotbit/items/:nftId`, () => {
  return HttpResponse.json({
    nftId: '0xdotbit_account_id',
    name: 'alice.bit',
    standard: 'dotbit',
    ownerLockHash: '0xownerhash789',
    isLive: true,
    createdAtBlock: 100000,
    expiredAt: 1750000000,
  });
}),

http.get(`${API_BASE}/assets/identities/did/items/:nftId`, () => {
  return HttpResponse.json({
    nftId: '0xdid_ckb_id',
    name: 'did:ckb:alice',
    standard: 'did_ckb',
    ownerLockHash: '0xownerhash789',
    isLive: true,
    createdAtBlock: 100000,
    expiredAt: null,
  });
}),

// Error handler for inventory context error test — must be overridden per-test
// using server.use() to return 404 for specific IDs
```

**Note for error test:** The wildcard MSW handlers above match any ID. To test error behavior, use `server.use()` in the test to temporarily override with a 404 response:

```typescript
import { server } from '@/tests/msw/server'; // or wherever the MSW server is set up

// In the error test:
server.use(
  http.get(`${API_BASE}/spore/objects/:sporeId`, () => {
    return new HttpResponse(null, { status: 404 });
  })
);
```

- [ ] **Step 2: Verify handlers file still valid**

Run: `cd frontend && npx vitest run --reporter=verbose __tests__/pages/cell.test.tsx 2>&1 | head -30`
Expected: existing cell tests still pass (new handlers don't break anything)

- [ ] **Step 3: Commit**

```bash
git add frontend/__tests__/msw/handlers.ts
git commit -m "test: add MSW mock handlers for inventory context endpoints"
```

---

### Task 2: Write Tests for `useInventoryContext` Hook and `InventoryContextSection`

**Files:**
- Create: `frontend/__tests__/components/cell/inventory-context.test.tsx`

- [ ] **Step 1: Create the test file with all 10 test cases**

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { DEFAULT_API_BASE } from '@/lib/runtime-config';
import type { Cell } from '@/lib/api';

// The component under test — will be created in Task 3
import { InventoryContextSection } from '@/components/cell/inventory-context';

function renderWithProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>
  );
}

// --- Base cell factory ---
function makeCell(overrides: Partial<Cell> = {}): Cell {
  return {
    txHash: '0xabc123def456789012345678901234567890123456789012345678901234abcd',
    outputIndex: 0,
    capacity: '14500000000',
    dataSize: 100,
    createdAtBlock: 100000,
    lockScriptHash: '0xlockhash',
    status: 'live' as const,
    isDepGroup: false,
    ...overrides,
  };
}

describe('InventoryContextSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders nothing when cell has no type script', () => {
    const cell = makeCell({ type: undefined, typeScriptHash: undefined });
    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders nothing when deterministic kind is unrecognized', () => {
    const cell = makeCell({
      type: { codeHash: '0xunknown', hashType: 'type', args: '0x1234' },
      typeScriptHash: '0xunknownhash',
      dataAnalysis: {
        deterministic: { kind: 'unknown_type', summary: 'test', segments: [] },
        heuristicGuesses: [],
      },
    });
    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders Spore card with cluster link and content type', async () => {
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      typeScriptHash: '0xsporehash',
      dataAnalysis: {
        deterministic: { kind: 'spore_cell', summary: 'Spore NFT', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    await waitFor(() => {
      expect(screen.getByText('Spore NFT')).toBeInTheDocument();
    });
    expect(screen.getByText(/image\/png/)).toBeInTheDocument();
    // Cluster is shown as a linked HexDisplay (truncated ID), not the cluster name
    const clusterLink = screen.getByRole('link', { name: /0xcluster/ });
    expect(clusterLink).toHaveAttribute('href', '/clusters/0xcluster456');
  });

  it('renders UDT card with formatted amount', async () => {
    const cell = makeCell({
      type: { codeHash: '0xudt_code', hashType: 'type', args: '0xargs123' },
      typeScriptHash: '0xtokenhash123',
      udtAmount: '12345678900000000',
      dataAnalysis: {
        deterministic: { kind: 'udt_amount', summary: 'UDT', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    await waitFor(() => {
      expect(screen.getByText('Test Token')).toBeInTheDocument();
    });
    // 12345678900000000 / 10^8 = 123,456,789
    expect(screen.getByText(/123,456,789/)).toBeInTheDocument();
    expect(screen.getByText('TT')).toBeInTheDocument();
  });

  it('renders m-NFT token card with class and issuer info', async () => {
    const cell = makeCell({
      type: { codeHash: '0xmnft_code', hashType: 'type', args: '0xmnft_token_id' },
      typeScriptHash: '0xmnfthash',
      dataAnalysis: {
        deterministic: { kind: 'mnft_token_cell', summary: 'mNFT Token', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    await waitFor(() => {
      expect(screen.getByText('Test NFT Class')).toBeInTheDocument();
    });
    expect(screen.getByText('Test Issuer')).toBeInTheDocument();
  });

  it('renders Cluster card with item count', async () => {
    const cell = makeCell({
      type: { codeHash: '0xcluster_code', hashType: 'type', args: '0xcluster456' },
      typeScriptHash: '0xclusterhash',
      dataAnalysis: {
        deterministic: { kind: 'spore_cluster_cell', summary: 'Spore Cluster', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    await waitFor(() => {
      expect(screen.getByText('Test Cluster')).toBeInTheDocument();
    });
    expect(screen.getByText('42')).toBeInTheDocument();
  });

  it('renders .bit card with account name', async () => {
    const cell = makeCell({
      type: { codeHash: '0xdotbit_code', hashType: 'type', args: '0xdotbit_account_id' },
      typeScriptHash: '0xdotbithash',
      dataAnalysis: {
        deterministic: { kind: 'dotbit_account', summary: '.bit account', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    await waitFor(() => {
      expect(screen.getByText('alice.bit')).toBeInTheDocument();
    });
  });

  it('shows loading state while inventory fetch is pending', () => {
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore_slow' },
      typeScriptHash: '0xsporehash',
      dataAnalysis: {
        deterministic: { kind: 'spore_cell', summary: 'Spore NFT', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    expect(screen.getByTestId('inventory-context-loading')).toBeInTheDocument();
  });

  it('hides section silently on fetch error', async () => {
    // Override MSW handler to return 404 for this test
    server.use(
      http.get(`${DEFAULT_API_BASE}/spore/objects/:sporeId`, () => {
        return new HttpResponse(null, { status: 404 });
      })
    );
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore_error' },
      typeScriptHash: '0xsporehash',
      dataAnalysis: {
        deterministic: { kind: 'spore_cell', summary: 'Spore NFT', segments: [] },
        heuristicGuesses: [],
      },
    });
    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);
    // Wait for the query to settle
    await waitFor(
      () => {
        expect(container.querySelector('[data-testid="inventory-context-loading"]')).toBeNull();
      },
      { timeout: 3000 }
    );
    // Should render nothing on error
    expect(container.querySelector('[data-testid="inventory-context-section"]')).toBeNull();
  });

  it('renders View details link pointing to correct route for each type', async () => {
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      typeScriptHash: '0xsporehash',
      dataAnalysis: {
        deterministic: { kind: 'spore_cell', summary: 'Spore NFT', segments: [] },
        heuristicGuesses: [],
      },
    });
    renderWithProviders(<InventoryContextSection cell={cell} />);
    await waitFor(() => {
      expect(screen.getByText('Spore NFT')).toBeInTheDocument();
    });
    const detailLink = screen.getByRole('link', { name: /View details/ });
    expect(detailLink).toHaveAttribute('href', '/objects/0xspore123');
  });
});
```

- [ ] **Step 2: Run tests to verify they fail (component doesn't exist yet)**

Run: `cd frontend && npx vitest run --reporter=verbose __tests__/components/cell/inventory-context.test.tsx 2>&1 | tail -20`
Expected: FAIL — cannot resolve `@/components/cell/inventory-context`

- [ ] **Step 3: Commit**

```bash
git add frontend/__tests__/components/cell/inventory-context.test.tsx
git commit -m "test: add failing tests for cell inventory context section"
```

---

### Task 3: Implement `useInventoryContext` Hook and `InventoryContextSection`

**Files:**
- Create: `frontend/components/cell/inventory-context.tsx`

- [ ] **Step 1: Create the component file with hook, entry point, and all card components**

Create `frontend/components/cell/inventory-context.tsx`:

```typescript
'use client';

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { api } from '@/lib/api';
import type {
  Cell,
  SporeNft,
  SporeCluster,
  Token,
  MnftItemDetail,
  ObjectCollection,
  CollectionItem,
} from '@/lib/api';
import { formatTokenBalance } from '@/lib/format-asset';
import { formatNumber } from '@/lib/utils';

// DID CKB code_hash (hash_type = type) — no deterministic decode exists for this type
const DID_CKB_CODE_HASH =
  '0x079bb8c1dfb249f60d932f4b1a60fa5cb2a36af3653ac09464f262e2f3f682a9';

type InventoryItemType =
  | 'spore'
  | 'cluster'
  | 'mnft_token'
  | 'mnft_class'
  | 'mnft_issuer'
  | 'udt'
  | 'dotbit'
  | 'did_ckb';

interface InventoryContext {
  itemType: InventoryItemType;
  itemId: string;
}

const KIND_MAP: Record<string, { itemType: InventoryItemType; idSource: 'args' | 'typeScriptHash' }> = {
  spore_cell: { itemType: 'spore', idSource: 'args' },
  spore_cluster_cell: { itemType: 'cluster', idSource: 'args' },
  mnft_token_cell: { itemType: 'mnft_token', idSource: 'args' },
  mnft_class_cell: { itemType: 'mnft_class', idSource: 'args' },
  mnft_issuer_cell: { itemType: 'mnft_issuer', idSource: 'args' },
  udt_amount: { itemType: 'udt', idSource: 'typeScriptHash' },
  dotbit_account: { itemType: 'dotbit', idSource: 'args' },
};

function useInventoryContext(cell: Cell | undefined): InventoryContext | null {
  return useMemo(() => {
    if (!cell) return null;

    // Primary path: deterministic.kind
    const kind = cell.dataAnalysis?.deterministic?.kind;
    if (kind) {
      // Exclude DAO cells — they have their own daoInfo section
      if (kind === 'dao_deposit_cell' || kind === 'dao_withdraw_request_cell') return null;

      const mapping = KIND_MAP[kind];
      if (!mapping) return null;

      const itemId =
        mapping.idSource === 'args' ? cell.type?.args : cell.typeScriptHash;
      if (!itemId) return null;

      return { itemType: mapping.itemType, itemId };
    }

    // Fallback: DID CKB code_hash match
    if (cell.type?.codeHash === DID_CKB_CODE_HASH && cell.type?.args) {
      return { itemType: 'did_ckb', itemId: cell.type.args };
    }

    return null;
  }, [cell]);
}

function fetchInventoryItem(ctx: InventoryContext) {
  switch (ctx.itemType) {
    case 'spore':
      return api.getSporeObject(ctx.itemId);
    case 'cluster':
      return api.getSporeCluster(ctx.itemId);
    case 'mnft_token':
      return api.getMnftItemDetail(ctx.itemId);
    case 'mnft_class':
    case 'mnft_issuer':
      return api.getObjectCollection(ctx.itemId);
    case 'udt':
      return api.getToken(ctx.itemId);
    case 'dotbit':
      return api.getDotbitItemDetail(ctx.itemId);
    case 'did_ckb':
      return api.getDidCkbItemDetail(ctx.itemId);
  }
}

function getDetailHref(ctx: InventoryContext): string | null {
  switch (ctx.itemType) {
    case 'spore':
      return `/objects/${ctx.itemId}`;
    case 'cluster':
      return `/clusters/${ctx.itemId}`;
    case 'mnft_token':
      return `/objects/mnft/${ctx.itemId}`;
    case 'mnft_class':
      return `/classes/${ctx.itemId}`;
    case 'mnft_issuer':
      return null; // no issuer detail page
    case 'udt':
      return `/tokens/${ctx.itemId}`;
    case 'dotbit':
      return `/identities/dotbit/${ctx.itemId}`;
    case 'did_ckb':
      return `/identities/did/${ctx.itemId}`;
  }
}

function getItemTypeLabel(itemType: InventoryItemType): string {
  switch (itemType) {
    case 'spore':
      return 'Spore NFT';
    case 'cluster':
      return 'Spore Cluster';
    case 'mnft_token':
      return 'm-NFT Token';
    case 'mnft_class':
      return 'm-NFT Class';
    case 'mnft_issuer':
      return 'm-NFT Issuer';
    case 'udt':
      return 'Token';
    case 'dotbit':
      return '.bit Account';
    case 'did_ckb':
      return 'DID:CKB';
  }
}

// --- Per-type card components ---

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-text-dim text-xs uppercase tracking-wide">{label}</div>
      <div className="text-text-bright mt-0.5 text-sm">{children}</div>
    </div>
  );
}

function SporeItemCard({ data }: { data: SporeNft }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      <InfoRow label="Content Type">{data.contentType}</InfoRow>
      <InfoRow label="Content Size">{data.contentSize.toLocaleString()} bytes</InfoRow>
      {data.clusterId && (
        <InfoRow label="Cluster">
          <Link href={`/clusters/${data.clusterId}`} className="text-aqua hover:underline">
            <HexDisplay value={data.clusterId} size="sm" />
          </Link>
        </InfoRow>
      )}
      {data.ownerAddress && (
        <InfoRow label="Owner">
          <Address address={data.ownerAddress} />
        </InfoRow>
      )}
      {data.mediaProfile?.tier && (
        <InfoRow label="Composition">
          <Badge variant={data.mediaProfile.tier === 'pure_ckb' ? 'green' : 'gray'}>
            {data.mediaProfile.tier.replace(/_/g, ' ')}
          </Badge>
        </InfoRow>
      )}
    </div>
  );
}

function ClusterItemCard({ data }: { data: SporeCluster }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Name">{data.name}</InfoRow>}
      {data.description && (
        <InfoRow label="Description">
          <span className="line-clamp-2">{data.description}</span>
        </InfoRow>
      )}
      <InfoRow label="Items">{formatNumber(data.sporesCount)}</InfoRow>
      <InfoRow label="Holders">{formatNumber(data.holdersCount)}</InfoRow>
    </div>
  );
}

function MnftTokenItemCard({ data }: { data: MnftItemDetail }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      <InfoRow label="Class">
        <Link href={`/classes/${data.class.classId}`} className="text-aqua hover:underline">
          {data.class.name || <HexDisplay value={data.class.classId} size="sm" />}
        </Link>
      </InfoRow>
      <InfoRow label="Issuer">{data.issuer.name || 'Unknown'}</InfoRow>
      <InfoRow label="Characteristics">
        <HexDisplay value={data.characteristicHex} size="sm" />
      </InfoRow>
      {data.ownerLockHash && (
        <InfoRow label="Owner">
          <HexDisplay value={data.ownerLockHash} size="sm" />
        </InfoRow>
      )}
    </div>
  );
}

function MnftClassItemCard({ data }: { data: ObjectCollection }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Name">{data.name}</InfoRow>}
      {data.issuerDetail?.name && (
        <InfoRow label="Issuer">{data.issuerDetail.name}</InfoRow>
      )}
      {data.classDetail && (
        <InfoRow label="Supply">
          {data.classDetail.issued} / {data.classDetail.total}
        </InfoRow>
      )}
      <InfoRow label="Total Items">{formatNumber(data.totalCount)}</InfoRow>
    </div>
  );
}

function MnftIssuerItemCard({ data }: { data: ObjectCollection }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
      {data.issuerDetail?.name && <InfoRow label="Name">{data.issuerDetail.name}</InfoRow>}
      {data.issuerDetail?.classCount != null && (
        <InfoRow label="Classes">{formatNumber(data.issuerDetail.classCount)}</InfoRow>
      )}
      {data.issuerDetail?.setCount != null && (
        <InfoRow label="Sets">{formatNumber(data.issuerDetail.setCount)}</InfoRow>
      )}
    </div>
  );
}

function UdtItemCard({ data, cell }: { data: Token; cell: Cell }) {
  const formattedAmount = cell.udtAmount
    ? formatTokenBalance(cell.udtAmount, data.decimals)
    : null;

  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      <InfoRow label="Token">
        <span className="flex items-center gap-1.5">
          {data.name || 'Unknown'}{' '}
          {data.symbol && (
            <Badge variant="neutral">{data.symbol}</Badge>
          )}
        </span>
      </InfoRow>
      {formattedAmount && (
        <InfoRow label="Amount in Cell">
          <span className="font-mono">{formattedAmount}</span>
          {data.symbol && <span className="text-text-dim ml-1">{data.symbol}</span>}
        </InfoRow>
      )}
      <InfoRow label="Total Supply">
        {formatTokenBalance(data.totalSupply, data.decimals)}
      </InfoRow>
      <InfoRow label="Holders">{formatNumber(data.holdersCount)}</InfoRow>
    </div>
  );
}

function DotbitItemCard({ data }: { data: CollectionItem }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
      <InfoRow label="Account">{data.name || 'Unknown'}</InfoRow>
      {data.ownerLockHash && (
        <InfoRow label="Owner">
          <HexDisplay value={data.ownerLockHash} size="sm" />
        </InfoRow>
      )}
      {data.expiredAt && (
        <InfoRow label="Expires">
          {new Date(data.expiredAt * 1000).toLocaleDateString()}
        </InfoRow>
      )}
    </div>
  );
}

function DidCkbItemCard({ data }: { data: CollectionItem }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
      <InfoRow label="Identity">{data.name || 'Unknown'}</InfoRow>
      {data.ownerLockHash && (
        <InfoRow label="Owner">
          <HexDisplay value={data.ownerLockHash} size="sm" />
        </InfoRow>
      )}
    </div>
  );
}

// --- Loading skeleton ---

function InventoryContextSkeleton() {
  return (
    <div data-testid="inventory-context-loading" className="mt-6">
      <TerminalPanel>
        <TerminalPanelHeader indicator="none">
          <div className="bg-base-border/50 h-4 w-32 animate-pulse rounded" />
        </TerminalPanelHeader>
        <TerminalPanelContent>
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
            {[...Array(4)].map((_, i) => (
              <div key={i}>
                <div className="bg-base-border/30 mb-1 h-3 w-16 animate-pulse rounded" />
                <div className="bg-base-border/50 h-4 w-24 animate-pulse rounded" />
              </div>
            ))}
          </div>
        </TerminalPanelContent>
      </TerminalPanel>
    </div>
  );
}

// --- Entry point ---

function renderCard(
  ctx: InventoryContext,
  data: unknown,
  cell: Cell
): React.ReactNode {
  switch (ctx.itemType) {
    case 'spore':
      return <SporeItemCard data={data as SporeNft} />;
    case 'cluster':
      return <ClusterItemCard data={data as SporeCluster} />;
    case 'mnft_token':
      return <MnftTokenItemCard data={data as MnftItemDetail} />;
    case 'mnft_class':
      return <MnftClassItemCard data={data as ObjectCollection} />;
    case 'mnft_issuer':
      return <MnftIssuerItemCard data={data as ObjectCollection} />;
    case 'udt':
      return <UdtItemCard data={data as Token} cell={cell} />;
    case 'dotbit':
      return <DotbitItemCard data={data as CollectionItem} />;
    case 'did_ckb':
      return <DidCkbItemCard data={data as CollectionItem} />;
  }
}

export function InventoryContextSection({ cell }: { cell: Cell | undefined }) {
  const ctx = useInventoryContext(cell);

  const { data, isLoading, isError } = useQuery({
    queryKey: ['inventory', ctx?.itemType, ctx?.itemId],
    queryFn: () => fetchInventoryItem(ctx!),
    enabled: !!ctx,
  });

  if (!ctx) return null;
  if (isLoading) return <InventoryContextSkeleton />;
  if (isError || !data) return null;

  const detailHref = getDetailHref(ctx);
  const label = getItemTypeLabel(ctx.itemType);

  return (
    <div className="mt-6" data-testid="inventory-context-section">
      <TerminalPanel>
        <TerminalPanelHeader indicator="none">
          <div className="flex items-center gap-2">
            <span>{label}</span>
            {detailHref && (
              <Link
                href={detailHref}
                className="text-aqua text-xs hover:underline"
              >
                View details &rarr;
              </Link>
            )}
          </div>
        </TerminalPanelHeader>
        <TerminalPanelContent>
          {renderCard(ctx, data, cell!)}
        </TerminalPanelContent>
      </TerminalPanel>
    </div>
  );
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cd frontend && npx vitest run --reporter=verbose __tests__/components/cell/inventory-context.test.tsx 2>&1 | tail -30`
Expected: all 10 tests PASS

- [ ] **Step 3: Fix any test failures and re-run**

Iterate until all tests pass. Common adjustments:
- MSW handler response shapes may need tweaking to match what tests query for
- `Badge` variant prop values may need adjusting
- `formatNumber` import path may differ — check `frontend/lib/utils.ts`

- [ ] **Step 4: Run full test suite to check for regressions**

Run: `cd frontend && npx vitest run --reporter=verbose 2>&1 | tail -20`
Expected: all existing tests still pass

- [ ] **Step 5: Commit**

```bash
git add frontend/components/cell/inventory-context.tsx
git commit -m "feat: add inventory context section component with per-type cards"
```

---

### Task 4: Integrate `InventoryContextSection` into Cell Detail Page

**Files:**
- Modify: `frontend/app/cell/[outpoint]/client-page.tsx` (insert at ~line 827)

- [ ] **Step 1: Add import at top of file**

Add to the imports section (after the existing component imports around line 18):

```typescript
import { InventoryContextSection } from '@/components/cell/inventory-context';
```

- [ ] **Step 2: Insert the component between type script section and DAO/data sections**

After line 827 (after `</div>` closing the side-panels container, before `{cell.daoInfo && (`):

```tsx
        <InventoryContextSection cell={cell} />
```

The insertion point is between these two existing blocks:

```tsx
          </div>   {/* closes the side-panels div, line ~826-827 */}
        </div>

        {/* === INSERT HERE === */}
        <InventoryContextSection cell={cell} />

        {cell.daoInfo && (  {/* existing DAO section, line ~828 */}
```

- [ ] **Step 3: Run the cell page test to verify no regressions**

Run: `cd frontend && npx vitest run --reporter=verbose __tests__/pages/cell.test.tsx 2>&1 | tail -20`
Expected: all existing cell page tests still pass

- [ ] **Step 4: Run full test suite**

Run: `cd frontend && npx vitest run --reporter=verbose 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 5: Run type check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
git add frontend/app/cell/[outpoint]/client-page.tsx
git commit -m "feat: integrate inventory context section into cell detail page"
```

---

### Task 5: Manual Verification and Cleanup

**Files:**
- Review all changed files

- [ ] **Step 1: Verify type exports**

Check that `Cell`, `SporeNft`, `SporeCluster`, `Token`, `MnftItemDetail`, `ObjectCollection`, `CollectionItem` are all exported from `frontend/lib/api.ts`. If any are not exported, add them to the export list.

Run: `cd frontend && grep -n 'export.*\(Cell\|SporeNft\|SporeCluster\|Token\|MnftItemDetail\|ObjectCollection\|CollectionItem\)' lib/api.ts | head -20`

- [ ] **Step 2: Run full pre-commit check**

Run: `cd frontend && pnpm type-check && pnpm lint && npx vitest run`
Expected: all pass

- [ ] **Step 3: Commit any final adjustments**

If any fixes were needed:
```bash
git add -A
git commit -m "fix: address type export and lint issues in inventory context"
```
