# Homepage Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Redesign homepage to follow Information Design principles — domain knowledge first, aggregations second, raw data reachable at bottom.

**Architecture:** Extend backend NetworkStats with 3 hero metrics (knowledge size, circulating supply, DAO locked). Add one new backend endpoint for asset ecosystem summary. Restructure frontend homepage from 6 sections to a layered layout: hero stat row → Layer 1 domain (activities, DAO, assets, activity trend) → Layer 2 aggregations (knowledge size trend, network health, script utilization) → Layer 0 raw data (compact blocks + transactions).

**Tech Stack:** Rust/Axum backend, React/TypeScript/TanStack Query frontend, Tailwind CSS, existing UI components (TerminalPanel, SparkChart, LineChart).

**Design doc:** `docs/plans/2026-03-11-homepage-redesign-design.md`

---

## Task 1: Backend — Extend NetworkStats with Hero Metrics

Add `knowledgeSize`, `circulatingSupply`, `daoLocked` to the NetworkStats API response.

**Files:**

- Modify: `crates/api/src/routes/statistics.rs` (struct ~line 141, handler ~line 2219)
- Test: `crates/api/tests/api_integration.rs`

**Step 1: Add fields to NetworkStats struct**

In `crates/api/src/routes/statistics.rs`, add 3 optional fields to the `NetworkStats` struct (around line 141):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    pub latest_block: i64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub estimated_epoch_time: String,
    pub transactions_per_minute: String,
    pub transactions_per_day: String,
    pub sync_status: SyncStatus,
    pub deep_fork_status: DeepForkStatus,
    // NEW: Hero metrics from latest DAO daily snapshot
    pub knowledge_size: Option<String>,       // occupied capacity in shannons
    pub circulating_supply: Option<String>,   // total issuance - burnt, in shannons
    pub dao_locked: Option<String>,           // total deposited in DAO, in shannons
}
```

**Step 2: Populate fields in fetch_network_stats_from_db**

In the `fetch_network_stats_from_db` function (~line 2219), after existing store reads, add:

```rust
// Hero metrics from latest DAO snapshot
let dao_snapshot = store.get_latest_dao_daily_snapshot();
let knowledge_size = dao_snapshot.as_ref().map(|s| s.occupied_capacity.to_string());
let circulating_supply = dao_snapshot.as_ref().map(|s| {
    (s.total_issuance - GENESIS_BURNT as i128).to_string()
});
let dao_locked = dao_snapshot.as_ref().map(|s| s.total_deposited.to_string());
```

Then include these in the `NetworkStats` construction:

```rust
NetworkStats {
    // ... existing fields ...
    knowledge_size,
    circulating_supply,
    dao_locked,
}
```

Note: `GENESIS_BURNT` is already imported from `ckbadger_common::dao`. Verify its type (likely `u64` = `8_400_000_000_00000000`). Cast to `i128` if needed.

**Step 3: Run cargo check**

```bash
cargo check -p ckbadger-api
```

Expected: Compiles with no errors. If there are type mismatches with GENESIS_BURNT, adjust the cast.

**Step 4: Run existing tests**

```bash
cargo test -p ckbadger-api
```

Expected: All existing tests pass. The new fields are `Option<String>` so they're backward-compatible (will be `null` in tests using mock data without DAO snapshots).

**Step 5: Commit**

```bash
git add crates/api/src/routes/statistics.rs
git commit -m "feat(api): add hero metrics to NetworkStats (knowledge size, circulating, DAO locked)"
```

---

## Task 2: Backend — Asset Ecosystem Summary Endpoint

New endpoint `GET /statistics/asset-ecosystem` returning top tokens and capacity breakdown by category.

**Files:**

- Modify: `crates/api/src/routes/statistics.rs` (add handler + response types + route)
- Test: `crates/api/tests/api_integration.rs`

**Step 1: Define response types**

Add to `crates/api/src/routes/statistics.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEcosystemResponse {
    pub top_tokens: Vec<TopTokenEntry>,
    pub capacity_breakdown: Vec<CapacityCategory>,
    pub total_knowledge_size_ckb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopTokenEntry {
    pub type_script_hash: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub holders_count: i64,
    pub total_capacity_ckb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityCategory {
    pub category: String,
    pub capacity_ckb: String,
    pub percentage: String,
}
```

**Step 2: Implement handler**

```rust
async fn get_asset_ecosystem(
    State(state): State<Arc<AppState>>,
) -> ApiResult<AssetEcosystemResponse> {
    if let Some(cached) = state.mem_cache.get::<AssetEcosystemResponse>(&CacheKeys::ASSET_ECOSYSTEM) {
        return ok(cached);
    }

    let store = &state.store;

    // Top 5 tokens from warmup cache (already sorted by holders_count DESC)
    let token_entries: Vec<CachedAssetEntry> = state
        .mem_cache
        .get(CACHE_KEY_ASSETS_TOKEN)
        .unwrap_or_default();

    let top_tokens: Vec<TopTokenEntry> = token_entries
        .iter()
        .take(5)
        .map(|t| TopTokenEntry {
            type_script_hash: hex::encode(&t.type_script_hash),
            name: t.name.clone(),
            symbol: t.symbol.clone(),
            holders_count: t.holders_count,
            total_capacity_ckb: shannon_to_ckb(t.total_capacity).to_string(),
        })
        .collect();

    // Capacity by category
    let total_token_cap: i128 = token_entries.iter().map(|t| t.total_capacity).sum();

    let nft_entries: Vec<CachedAssetEntry> = state
        .mem_cache
        .get(CACHE_KEY_ASSETS_NFT)
        .unwrap_or_default();
    let total_object_cap: i128 = nft_entries.iter().map(|n| n.total_capacity).sum();

    // DAO locked + knowledge size from latest snapshot
    let dao_snapshot = store.get_latest_dao_daily_snapshot();
    let dao_locked: i128 = dao_snapshot.as_ref().map(|s| s.total_deposited).unwrap_or(0);
    let knowledge_size: i128 = dao_snapshot.as_ref().map(|s| s.occupied_capacity).unwrap_or(0);

    // "Other" = knowledge_size - known categories
    let known = total_token_cap + total_object_cap + dao_locked;
    let other = (knowledge_size - known).max(0);

    let total_ckb = if knowledge_size > 0 { knowledge_size } else { 1 }; // avoid div-by-zero
    let pct = |v: i128| format!("{:.1}", v as f64 / total_ckb as f64 * 100.0);

    let capacity_breakdown = vec![
        CapacityCategory { category: "dao".into(), capacity_ckb: shannon_to_ckb(dao_locked).to_string(), percentage: pct(dao_locked) },
        CapacityCategory { category: "tokens".into(), capacity_ckb: shannon_to_ckb(total_token_cap).to_string(), percentage: pct(total_token_cap) },
        CapacityCategory { category: "objects".into(), capacity_ckb: shannon_to_ckb(total_object_cap).to_string(), percentage: pct(total_object_cap) },
        CapacityCategory { category: "other".into(), capacity_ckb: shannon_to_ckb(other).to_string(), percentage: pct(other) },
    ];

    let result = AssetEcosystemResponse {
        top_tokens,
        capacity_breakdown,
        total_knowledge_size_ckb: shannon_to_ckb(knowledge_size).to_string(),
    };

    state.mem_cache.insert(CacheKeys::ASSET_ECOSYSTEM, result.clone(), CacheTtl::ASSET_ECOSYSTEM);
    ok(result)
}
```

Note: Adapt to actual `CachedAssetEntry` field names — check `crates/api/src/warmup.rs` for exact struct. Add `ASSET_ECOSYSTEM` to `CacheKeys` and `CacheTtl` (30s TTL). The `shannon_to_ckb` is already imported from `crate::utils`.

**Step 3: Register route**

In the `routes()` function of statistics.rs, add:

```rust
.route("/statistics/asset-ecosystem", get(get_asset_ecosystem))
```

**Step 4: Build and test**

```bash
cargo check -p ckbadger-api && cargo test -p ckbadger-api
```

**Step 5: Commit**

```bash
git add crates/api/src/routes/statistics.rs crates/api/src/cache.rs
git commit -m "feat(api): add asset ecosystem summary endpoint"
```

---

## Task 3: Frontend — Update API Types and Add Methods

**Files:**

- Modify: `frontend/lib/api.ts`
- Test: `frontend/__tests__/lib/` (if API tests exist)

**Step 1: Extend NetworkStats type**

In `frontend/lib/api.ts`, add 3 fields to `NetworkStats` interface (~line 86):

```typescript
interface NetworkStats {
  // ... existing fields ...
  knowledgeSize: string | null; // shannons
  circulatingSupply: string | null; // shannons
  daoLocked: string | null; // shannons
}
```

**Step 2: Add AssetEcosystem types**

```typescript
interface TopTokenEntry {
  typeScriptHash: string;
  name: string | null;
  symbol: string | null;
  holdersCount: number;
  totalCapacityCkb: string;
}

interface CapacityCategory {
  category: string;
  capacityCkb: string;
  percentage: string;
}

interface AssetEcosystemResponse {
  topTokens: TopTokenEntry[];
  capacityBreakdown: CapacityCategory[];
  totalKnowledgeSizeCkb: string;
}
```

**Step 3: Add API method**

In the `api` object:

```typescript
getAssetEcosystem: (): Promise<AssetEcosystemResponse> => {
  return fetchApi('/statistics/asset-ecosystem');
},
```

**Step 4: Run type check**

```bash
cd frontend && pnpm type-check
```

Expected: Pass (new types are additive, new optional fields on NetworkStats won't break existing usage).

**Step 5: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "feat(frontend): add API types for hero metrics and asset ecosystem"
```

---

## Task 4: Frontend — HeroStatRow Component

5 metrics in a clean horizontal row: Knowledge Size, Circulating, DAO Locked, Block Height, Epoch.

**Files:**

- Create: `frontend/components/hero-stat-row.tsx`
- Test: `frontend/__tests__/components/hero-stat-row.test.tsx`

**Step 1: Write the test**

```typescript
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { HeroStatRow } from '@/components/hero-stat-row';

const mockStats = {
  knowledgeSize: '19800000000000000000',  // ~198 GB in shannons
  circulatingSupply: '4380000000000000000', // ~43.8B CKB
  daoLocked: '1120000000000000000',        // ~11.2B CKB
  latestBlock: 14235678,
  epoch: '8234(150/1800)',
};

describe('HeroStatRow', () => {
  it('renders all 5 metrics', () => {
    render(<HeroStatRow stats={mockStats as any} />);
    expect(screen.getByText(/knowledge size/i)).toBeInTheDocument();
    expect(screen.getByText(/circulating/i)).toBeInTheDocument();
    expect(screen.getByText(/dao locked/i)).toBeInTheDocument();
    expect(screen.getByText(/block height/i)).toBeInTheDocument();
    expect(screen.getByText(/epoch/i)).toBeInTheDocument();
  });

  it('renders loading state when stats is null', () => {
    render(<HeroStatRow stats={null} />);
    // Should show placeholder/skeleton
    const skeletons = document.querySelectorAll('.animate-pulse');
    expect(skeletons.length).toBeGreaterThan(0);
  });
});
```

**Step 2: Run test to verify it fails**

```bash
cd frontend && npx vitest run __tests__/components/hero-stat-row.test.tsx
```

Expected: FAIL — module not found.

**Step 3: Implement the component**

Create `frontend/components/hero-stat-row.tsx`:

```typescript
'use client';

import Link from 'next/link';
import type { NetworkStats } from '@/lib/api';

interface HeroStatRowProps {
  stats: NetworkStats | null;
}

function formatShannonsToDisplay(shannons: string | null, unit: 'bytes' | 'ckb'): string {
  if (!shannons) return '—';
  const val = Number(BigInt(shannons)) / 1e8; // shannons to CKB
  if (unit === 'bytes') {
    // Knowledge size: occupied capacity in CKB, but display as bytes
    // 1 CKB = 1 byte of storage
    const bytes = val;
    if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
    if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
    return `${bytes.toFixed(0)} KB`;
  }
  // CKB display
  if (val >= 1e9) return `${(val / 1e9).toFixed(2)}B`;
  if (val >= 1e6) return `${(val / 1e6).toFixed(2)}M`;
  return val.toLocaleString();
}

function formatNumber(n: number): string {
  return n.toLocaleString();
}

function parseEpochNumber(epoch: string): string {
  const match = epoch.match(/^(\d+)/);
  return match ? formatNumber(parseInt(match[1], 10)) : epoch;
}

const metrics = [
  {
    key: 'knowledgeSize',
    label: 'Knowledge Size',
    href: '/charts/knowledge-size',
    format: (stats: NetworkStats) => formatShannonsToDisplay(stats.knowledgeSize, 'bytes'),
  },
  {
    key: 'circulating',
    label: 'Circulating',
    href: '/charts/total-supply',
    format: (stats: NetworkStats) => formatShannonsToDisplay(stats.circulatingSupply, 'ckb'),
  },
  {
    key: 'daoLocked',
    label: 'DAO Locked',
    href: '/nervos-dao',
    format: (stats: NetworkStats) => formatShannonsToDisplay(stats.daoLocked, 'ckb'),
  },
  {
    key: 'blockHeight',
    label: 'Block Height',
    href: (stats: NetworkStats) => `/blocks/${stats.latestBlock}`,
    format: (stats: NetworkStats) => `#${formatNumber(stats.latestBlock)}`,
  },
  {
    key: 'epoch',
    label: 'Epoch',
    href: '/charts/epoch-time-length',
    format: (stats: NetworkStats) => `#${parseEpochNumber(stats.epoch)}`,
  },
] as const;

export function HeroStatRow({ stats }: HeroStatRowProps) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-4 sm:gap-6">
      {metrics.map((m) => (
        <Link
          key={m.key}
          href={typeof m.href === 'function' && stats ? m.href(stats) : (m.href as string)}
          className="group flex min-w-0 flex-1 flex-col items-center gap-1 transition-opacity hover:opacity-80"
        >
          <span className="text-emphasis font-mono text-xl font-bold tabular-nums sm:text-2xl">
            {stats ? m.format(stats) : <span className="bg-base-elevated inline-block h-7 w-20 animate-pulse rounded" />}
          </span>
          <span className="text-text-dim text-xs uppercase tracking-wider">
            {m.label}
          </span>
        </Link>
      ))}
    </div>
  );
}
```

Note: Adjust `formatShannonsToDisplay` based on actual data values — verify that `knowledgeSize` from the API represents occupied capacity in shannons (1 CKB = 10^8 shannons, 1 CKB = 1 byte of storage). Check `docs/DAO_CALCULATIONS.md` for exact semantics. The `Link` component import may need to be from `@/components/ui/link` instead of `next/link` — check existing components for the pattern used.

**Step 4: Run test to verify it passes**

```bash
cd frontend && npx vitest run __tests__/components/hero-stat-row.test.tsx
```

Expected: PASS.

**Step 5: Commit**

```bash
git add frontend/components/hero-stat-row.tsx frontend/__tests__/components/hero-stat-row.test.tsx
git commit -m "feat(frontend): add HeroStatRow component for homepage hero metrics"
```

---

## Task 5: Frontend — DaoOverview Component

DAO summary panel: Total Deposited, APC, Depositor Count, 30-day trend spark.

**Files:**

- Create: `frontend/components/dao-overview.tsx`
- Test: `frontend/__tests__/components/dao-overview.test.tsx`

**Step 1: Write the test**

```typescript
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { DaoOverview } from '@/components/dao-overview';

// Mock API
vi.mock('@/lib/api', () => ({
  api: {
    getDaoStatistics: vi.fn().mockResolvedValue({
      totalDepositedCkb: '11200000000.50',
      totalDepositors: 4521,
      estimatedApc: '2.45',
    }),
    getDaoTotalDepositChart: vi.fn().mockResolvedValue({
      data: [
        { date: '2026-02-10', value: '10000000000' },
        { date: '2026-02-11', value: '10100000000' },
      ],
      title: 'Total DAO Deposit',
      yAxisLabel: 'CKB',
    }),
  },
}));

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe('DaoOverview', () => {
  it('renders DAO statistics', async () => {
    render(<DaoOverview />, { wrapper });
    expect(await screen.findByText(/deposited/i)).toBeInTheDocument();
    expect(await screen.findByText(/apc/i)).toBeInTheDocument();
    expect(await screen.findByText(/depositors/i)).toBeInTheDocument();
  });
});
```

**Step 2: Run test to verify it fails**

```bash
cd frontend && npx vitest run __tests__/components/dao-overview.test.tsx
```

**Step 3: Implement the component**

Create `frontend/components/dao-overview.tsx`:

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { SparkChart } from '@/components/ui/spark-chart';
import { TerminalPanel, TerminalPanelHeader, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { Link } from '@/components/ui/link';

export function DaoOverview() {
  const { data: daoStats, isLoading: statsLoading } = useQuery({
    queryKey: ['dao-statistics'],
    queryFn: () => api.getDaoStatistics(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const { data: depositChart } = useQuery({
    queryKey: ['dao-total-deposit-chart'],
    queryFn: () => api.getDaoTotalDepositChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const sparkData = depositChart?.data
    ?.slice(-30)
    .map((d) => parseFloat(d.value)) ?? [];

  return (
    <TerminalPanel className="h-full">
      <TerminalPanelHeader>
        <Link href="/nervos-dao" className="transition-opacity hover:opacity-80">
          Nervos DAO
        </Link>
      </TerminalPanelHeader>
      <TerminalPanelContent className="flex flex-col gap-3">
        {/* Total Deposited */}
        <div>
          <div className="text-text-dim text-xs uppercase tracking-wider">Total Deposited</div>
          <div className="text-emphasis font-mono text-lg font-bold tabular-nums">
            {statsLoading ? (
              <span className="bg-base-elevated inline-block h-6 w-32 animate-pulse rounded" />
            ) : (
              `${formatCkb(daoStats?.totalDepositedCkb)} CKB`
            )}
          </div>
        </div>

        {/* APC + Depositors row */}
        <div className="flex gap-6">
          <div>
            <div className="text-text-dim text-xs uppercase tracking-wider">APC</div>
            <div className="text-jade font-mono text-base font-bold tabular-nums">
              {daoStats?.estimatedApc ? `${daoStats.estimatedApc}%` : '—'}
            </div>
          </div>
          <div>
            <div className="text-text-dim text-xs uppercase tracking-wider">Depositors</div>
            <div className="text-text-bright font-mono text-base font-bold tabular-nums">
              {daoStats?.totalDepositors?.toLocaleString() ?? '—'}
            </div>
          </div>
        </div>

        {/* 30-day trend spark */}
        {sparkData.length > 0 && (
          <div className="mt-auto">
            <div className="text-text-dim mb-1 text-[10px] uppercase tracking-wider">30-Day Trend</div>
            <SparkChart data={sparkData} height={32} />
          </div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function formatCkb(value?: string): string {
  if (!value) return '—';
  const num = parseFloat(value);
  if (num >= 1e9) return `${(num / 1e9).toFixed(2)}B`;
  if (num >= 1e6) return `${(num / 1e6).toFixed(2)}M`;
  return num.toLocaleString();
}
```

Note: Check if `TerminalPanel` is imported from `@/components/ui/terminal-panel` or another path. Adapt imports to match existing component patterns. The `totalDepositedCkb` field from DaoStatistics is already in CKB (not shannons), so no conversion needed.

**Step 4: Run test**

```bash
cd frontend && npx vitest run __tests__/components/dao-overview.test.tsx
```

**Step 5: Commit**

```bash
git add frontend/components/dao-overview.tsx frontend/__tests__/components/dao-overview.test.tsx
git commit -m "feat(frontend): add DaoOverview component for homepage"
```

---

## Task 6: Frontend — AssetEcosystem Component

Top tokens by holders + horizontal capacity breakdown bar.

**Files:**

- Create: `frontend/components/asset-ecosystem.tsx`
- Test: `frontend/__tests__/components/asset-ecosystem.test.tsx`

**Step 1: Write the test**

```typescript
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { AssetEcosystem } from '@/components/asset-ecosystem';

vi.mock('@/lib/api', () => ({
  api: {
    getAssetEcosystem: vi.fn().mockResolvedValue({
      topTokens: [
        { typeScriptHash: 'abc123', name: 'USDT', symbol: 'USDT', holdersCount: 1500, totalCapacityCkb: '50000000' },
        { typeScriptHash: 'def456', name: 'SEAL', symbol: 'SEAL', holdersCount: 800, totalCapacityCkb: '30000000' },
      ],
      capacityBreakdown: [
        { category: 'dao', capacityCkb: '11200000000', percentage: '57.1' },
        { category: 'tokens', capacityCkb: '1500000000', percentage: '7.6' },
        { category: 'objects', capacityCkb: '500000000', percentage: '2.5' },
        { category: 'other', capacityCkb: '6400000000', percentage: '32.8' },
      ],
      totalKnowledgeSizeCkb: '19600000000',
    }),
  },
}));

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe('AssetEcosystem', () => {
  it('renders top tokens', async () => {
    render(<AssetEcosystem />, { wrapper });
    expect(await screen.findByText('USDT')).toBeInTheDocument();
    expect(await screen.findByText('SEAL')).toBeInTheDocument();
  });

  it('renders capacity breakdown', async () => {
    render(<AssetEcosystem />, { wrapper });
    expect(await screen.findByText(/dao/i)).toBeInTheDocument();
    expect(await screen.findByText(/tokens/i)).toBeInTheDocument();
  });
});
```

**Step 2: Run test to verify it fails**

```bash
cd frontend && npx vitest run __tests__/components/asset-ecosystem.test.tsx
```

**Step 3: Implement the component**

Create `frontend/components/asset-ecosystem.tsx`:

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { TerminalPanel, TerminalPanelHeader, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { Link } from '@/components/ui/link';

const CATEGORY_COLORS: Record<string, string> = {
  dao: '#44ee77',
  tokens: '#ff66aa',
  objects: '#bb88ff',
  identities: '#44bbff',
  other: '#666677',
};

const CATEGORY_LABELS: Record<string, string> = {
  dao: 'DAO',
  tokens: 'Tokens',
  objects: 'Objects',
  identities: 'Identities',
  other: 'Other',
};

export function AssetEcosystem() {
  const { data, isLoading } = useQuery({
    queryKey: ['asset-ecosystem'],
    queryFn: () => api.getAssetEcosystem(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  return (
    <TerminalPanel className="h-full">
      <TerminalPanelHeader>Asset Ecosystem</TerminalPanelHeader>
      <TerminalPanelContent className="flex flex-col gap-3">
        {/* Top Tokens */}
        <div>
          <div className="text-text-dim mb-1.5 text-[10px] uppercase tracking-wider">Top Tokens</div>
          {isLoading ? (
            <div className="space-y-2">
              {[1, 2, 3].map((i) => (
                <div key={i} className="bg-base-elevated h-5 animate-pulse rounded" />
              ))}
            </div>
          ) : (
            <div className="space-y-1">
              {data?.topTokens.map((t) => (
                <Link
                  key={t.typeScriptHash}
                  href={`/tokens/${t.typeScriptHash}`}
                  className="flex items-baseline justify-between font-mono text-xs transition-opacity hover:opacity-80"
                >
                  <span className="text-text-bright">{t.name ?? t.symbol ?? 'Unknown'}</span>
                  <span className="text-text-dim tabular-nums">{t.holdersCount.toLocaleString()} holders</span>
                </Link>
              ))}
            </div>
          )}
        </div>

        {/* Capacity Breakdown Bar */}
        {data?.capacityBreakdown && (
          <div className="mt-auto">
            <div className="text-text-dim mb-1.5 text-[10px] uppercase tracking-wider">
              Capacity Breakdown
            </div>
            <div className="flex h-3 w-full overflow-hidden rounded-full">
              {data.capacityBreakdown.map((cat) => (
                <div
                  key={cat.category}
                  style={{
                    width: `${Math.max(parseFloat(cat.percentage), 1)}%`,
                    backgroundColor: CATEGORY_COLORS[cat.category] ?? '#666',
                  }}
                  title={`${CATEGORY_LABELS[cat.category]}: ${parseFloat(cat.percentage).toFixed(1)}%`}
                />
              ))}
            </div>
            {/* Legend */}
            <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1">
              {data.capacityBreakdown.map((cat) => (
                <div key={cat.category} className="flex items-center gap-1 text-[10px]">
                  <span
                    className="inline-block h-2 w-2 rounded-full"
                    style={{ backgroundColor: CATEGORY_COLORS[cat.category] ?? '#666' }}
                  />
                  <span className="text-text-dim">
                    {CATEGORY_LABELS[cat.category]} {parseFloat(cat.percentage).toFixed(1)}%
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
```

**Step 4: Run test**

```bash
cd frontend && npx vitest run __tests__/components/asset-ecosystem.test.tsx
```

**Step 5: Commit**

```bash
git add frontend/components/asset-ecosystem.tsx frontend/__tests__/components/asset-ecosystem.test.tsx
git commit -m "feat(frontend): add AssetEcosystem component for homepage"
```

---

## Task 7: Frontend — ActivityTrend Component

Replaces ActivityBreakdown. Shows 14-day volume bar chart + type breakdown text + 24h stats.

**Files:**

- Create: `frontend/components/activity-trend.tsx`
- Test: `frontend/__tests__/components/activity-trend.test.tsx`

**Step 1: Write the test**

```typescript
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ActivityTrend } from '@/components/activity-trend';

vi.mock('@/lib/api', () => ({
  api: {
    getDailyActivityStats: vi.fn().mockResolvedValue([
      { date: '2026-03-10', transferCount: 1200, daoDepositCount: 50, tokenCount: 80, objectCount: 10, uniqueAddressCount: 450, totalCkbMoved: '500000000000' },
      { date: '2026-03-09', transferCount: 1100, daoDepositCount: 45, tokenCount: 75, objectCount: 8, uniqueAddressCount: 420, totalCkbMoved: '480000000000' },
    ]),
    getActivitySummary24h: vi.fn().mockResolvedValue({
      transferCount: 1200,
      daoDepositCount: 50,
      daoWithdrawRequestCount: 20,
      daoWithdrawCompleteCount: 10,
      tokenCount: 80,
      objectCount: 10,
      identityCount: 5,
      scriptCallCount: 30,
      uniqueAddressCount: 450,
      totalCkbMoved: '500000000000',
      hoursCovered: 24,
    }),
  },
}));

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe('ActivityTrend', () => {
  it('renders activity summary stats', async () => {
    render(<ActivityTrend />, { wrapper });
    expect(await screen.findByText(/unique addr/i)).toBeInTheDocument();
  });
});
```

**Step 2: Run test to verify it fails**

```bash
cd frontend && npx vitest run __tests__/components/activity-trend.test.tsx
```

**Step 3: Implement the component**

Create `frontend/components/activity-trend.tsx`:

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { TerminalPanel, TerminalPanelHeader, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { Link } from '@/components/ui/link';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';

function formatCompact(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return n.toString();
}

function formatCkbCompact(shannons: string): string {
  const ckb = Number(BigInt(shannons)) / 1e8;
  if (ckb >= 1e9) return `${(ckb / 1e9).toFixed(1)}B`;
  if (ckb >= 1e6) return `${(ckb / 1e6).toFixed(1)}M`;
  return ckb.toLocaleString();
}

export function ActivityTrend() {
  const { data: dailyStats } = useQuery({
    queryKey: ['daily-activity-stats', 14],
    queryFn: () => api.getDailyActivityStats(14),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const { data: summary } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  // Compute daily totals for bar chart
  const barData = (dailyStats ?? []).map((d) => ({
    date: d.date,
    total:
      d.transferCount +
      d.daoDepositCount +
      (d.daoWithdrawRequestCount ?? 0) +
      (d.daoWithdrawCompleteCount ?? 0) +
      d.tokenCount +
      d.objectCount +
      (d.identityCount ?? 0) +
      (d.scriptCallCount ?? 0),
  }));

  const maxVal = Math.max(...barData.map((d) => d.total), 1);

  return (
    <TerminalPanel className="h-full">
      <TerminalPanelHeader>
        <Link href="/charts" className="transition-opacity hover:opacity-80">
          Activity Trend
        </Link>
      </TerminalPanelHeader>
      <TerminalPanelContent className="flex flex-col gap-3">
        {/* 14-day bar chart */}
        <div className="flex h-16 items-end gap-[2px]">
          {barData.map((d) => (
            <div
              key={d.date}
              className="flex-1 rounded-t-sm transition-all"
              style={{
                height: `${Math.max((d.total / maxVal) * 100, 2)}%`,
                backgroundColor: CHART_PRIMARY_COLOR,
                opacity: 0.8,
              }}
              title={`${d.date}: ${d.total.toLocaleString()} activities`}
            />
          ))}
        </div>

        {/* Type breakdown text */}
        {summary && (
          <>
            <div className="text-text-dim flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px]">
              <span>Transfers: {formatCompact(summary.transferCount)}</span>
              <span>DAO: {formatCompact(summary.daoDepositCount + summary.daoWithdrawRequestCount + summary.daoWithdrawCompleteCount)}</span>
              <span>Tokens: {formatCompact(summary.tokenCount)}</span>
              <span>Objects: {formatCompact(summary.objectCount)}</span>
            </div>

            {/* 24h stats */}
            <div className="mt-auto flex gap-6">
              <div>
                <div className="text-text-dim text-[10px] uppercase tracking-wider">Unique Addr (24h)</div>
                <div className="text-text-bright font-mono text-sm font-bold tabular-nums">
                  {summary.uniqueAddressCount.toLocaleString()}
                </div>
              </div>
              <div>
                <div className="text-text-dim text-[10px] uppercase tracking-wider">CKB Moved (24h)</div>
                <div className="text-text-bright font-mono text-sm font-bold tabular-nums">
                  {formatCkbCompact(summary.totalCkbMoved)}
                </div>
              </div>
            </div>
          </>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
```

Note: Check if `getDailyActivityStats` takes days param or uses a different signature. Verify import path for `CHART_PRIMARY_COLOR`.

**Step 4: Run test**

```bash
cd frontend && npx vitest run __tests__/components/activity-trend.test.tsx
```

**Step 5: Commit**

```bash
git add frontend/components/activity-trend.tsx frontend/__tests__/components/activity-trend.test.tsx
git commit -m "feat(frontend): add ActivityTrend component for homepage"
```

---

## Task 8: Frontend — Layer 2 Section Components

Three compact components for the aggregation section: Knowledge Size Trend, Network Health, Script Utilization.

**Files:**

- Create: `frontend/components/home-layer2.tsx` (all 3 in one file — they're small)
- Test: `frontend/__tests__/components/home-layer2.test.tsx`

**Step 1: Write the test**

```typescript
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { KnowledgeSizeTrend, NetworkHealth, ScriptUtilization } from '@/components/home-layer2';

vi.mock('@/lib/api', () => ({
  api: {
    getKnowledgeSizeChart: vi.fn().mockResolvedValue({
      data: [{ date: '2026-03-10', value: '19600000000' }],
      title: 'Knowledge Size',
      yAxisLabel: 'CKB',
    }),
    getAverageBlockTimeChart: vi.fn().mockResolvedValue({
      data: [{ date: '2026-03-10', value: '8.2' }],
      title: 'Average Block Time',
      yAxisLabel: 'seconds',
    }),
    getHashRateChart: vi.fn().mockResolvedValue({
      data: [{ date: '2026-03-10', value: '650000000000000' }],
      title: 'Hash Rate',
      yAxisLabel: 'H/s',
    }),
    getScripts: vi.fn().mockResolvedValue({
      data: [
        { name: 'secp256k1', liveCapacitySum: '5000000000', liveUsedCapacitySum: '3000000000' },
        { name: 'dao', liveCapacitySum: '2000000000', liveUsedCapacitySum: '1000000000' },
      ],
      hasMore: false,
    }),
  },
}));

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe('KnowledgeSizeTrend', () => {
  it('renders', async () => {
    render(<KnowledgeSizeTrend />, { wrapper });
    expect(await screen.findByText(/knowledge size/i)).toBeInTheDocument();
  });
});

describe('NetworkHealth', () => {
  it('renders block time and hash rate', async () => {
    render(<NetworkHealth stats={{ avgBlockTime: '8.2', hashRate: '650 TH/s' } as any} />, { wrapper });
    expect(await screen.findByText(/block time/i)).toBeInTheDocument();
    expect(await screen.findByText(/hash rate/i)).toBeInTheDocument();
  });
});

describe('ScriptUtilization', () => {
  it('renders top scripts', async () => {
    render(<ScriptUtilization />, { wrapper });
    expect(await screen.findByText(/script/i)).toBeInTheDocument();
  });
});
```

**Step 2: Run test to verify it fails**

```bash
cd frontend && npx vitest run __tests__/components/home-layer2.test.tsx
```

**Step 3: Implement the components**

Create `frontend/components/home-layer2.tsx`:

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
import { api, type NetworkStats } from '@/lib/api';
import { SparkChart } from '@/components/ui/spark-chart';
import { ChartCard } from '@/components/ui/chart-card';
import { Link } from '@/components/ui/link';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR } from '@/lib/chart-colors';

// ─── Knowledge Size Trend ───────────────────────────────────────

export function KnowledgeSizeTrend() {
  const { data: chart, isLoading } = useQuery({
    queryKey: ['knowledge-size-chart'],
    queryFn: () => api.getKnowledgeSizeChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const sparkData = chart?.data?.slice(-30).map((d) => parseFloat(d.value)) ?? [];

  return (
    <ChartCard title="Knowledge Size" href="/charts/knowledge-size" isLoading={isLoading} height={100}>
      {sparkData.length > 0 && <SparkChart data={sparkData} height={60} />}
    </ChartCard>
  );
}

// ─── Network Health ─────────────────────────────────────────────

interface NetworkHealthProps {
  stats: NetworkStats | null;
}

export function NetworkHealth({ stats }: NetworkHealthProps) {
  const { data: blockTimeChart } = useQuery({
    queryKey: ['block-time-chart'],
    queryFn: () => api.getAverageBlockTimeChart(),
    staleTime: 60_000,
    refetchInterval: 300_000,
  });

  const { data: hashRateChart } = useQuery({
    queryKey: ['hash-rate-chart'],
    queryFn: () => api.getHashRateChart(),
    staleTime: 60_000,
    refetchInterval: 300_000,
  });

  const blockTimeSpark = blockTimeChart?.data?.slice(-14).map((d) => parseFloat(d.value)) ?? [];
  const hashRateSpark = hashRateChart?.data?.slice(-14).map((d) => parseFloat(d.value)) ?? [];

  return (
    <ChartCard title="Network Health" href="/charts/average-block-time" height={100}>
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-text-dim text-[10px] uppercase tracking-wider">Block Time</div>
            <div className="text-emphasis font-mono text-sm font-bold tabular-nums">
              {stats?.avgBlockTime ?? '—'}s
            </div>
          </div>
          <div className="w-24">
            {blockTimeSpark.length > 0 && (
              <SparkChart data={blockTimeSpark} height={20} color={CHART_PRIMARY_COLOR} />
            )}
          </div>
        </div>
        <div className="flex items-center justify-between">
          <div>
            <div className="text-text-dim text-[10px] uppercase tracking-wider">Hash Rate</div>
            <div className="text-emphasis font-mono text-sm font-bold tabular-nums">
              {stats?.hashRate ?? '—'}
            </div>
          </div>
          <div className="w-24">
            {hashRateSpark.length > 0 && (
              <SparkChart data={hashRateSpark} height={20} color={CHART_SECONDARY_COLOR} />
            )}
          </div>
        </div>
      </div>
    </ChartCard>
  );
}

// ─── Script Utilization ─────────────────────────────────────────

const BAR_COLORS = ['#8ce00a', '#00d7eb', '#ff66aa', '#bb88ff', '#ff8800'];

export function ScriptUtilization() {
  const { data, isLoading } = useQuery({
    queryKey: ['scripts-top5'],
    queryFn: () => api.getScripts({ limit: 5, sortKey: 'used', sortDirection: 'desc' }),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const scripts = data?.data ?? [];
  const maxCap = Math.max(
    ...scripts.map((s) => parseFloat(s.liveUsedCapacitySum ?? '0')),
    1
  );

  return (
    <ChartCard title="Script Utilization" href="/charts/most-utilized-scripts" isLoading={isLoading} height={100}>
      <div className="flex flex-col gap-1.5">
        {scripts.map((s, i) => {
          const cap = parseFloat(s.liveUsedCapacitySum ?? '0');
          const pct = (cap / maxCap) * 100;
          return (
            <div key={s.codeHash} className="flex items-center gap-2">
              <span className="text-text-dim w-20 truncate font-mono text-[10px]">{s.name}</span>
              <div className="bg-base-elevated h-2 flex-1 overflow-hidden rounded-full">
                <div
                  className="h-full rounded-full transition-all"
                  style={{ width: `${pct}%`, backgroundColor: BAR_COLORS[i % BAR_COLORS.length] }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </ChartCard>
  );
}
```

Note: Verify `getScripts` sortKey values. The backend accepts `sort_key=used` (maps to used capacity). Check if `liveUsedCapacitySum` is the correct field name on `KnownScript` (it might be `liveCapacitySum` or similar). The `ChartCard` height may need adjustment to fit the compact format.

**Step 4: Run test**

```bash
cd frontend && npx vitest run __tests__/components/home-layer2.test.tsx
```

**Step 5: Commit**

```bash
git add frontend/components/home-layer2.tsx frontend/__tests__/components/home-layer2.test.tsx
git commit -m "feat(frontend): add L2 section components (knowledge size, network health, scripts)"
```

---

## Task 9: Frontend — Restructure Homepage Layout

Replace `home-content.tsx` with the new layered layout. This is the main integration task.

**Files:**

- Modify: `frontend/components/home-content.tsx`
- Modify: `frontend/app/page.tsx` (simplify InitialData)

**Step 1: Rewrite home-content.tsx**

Replace the entire layout in `frontend/components/home-content.tsx` with:

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
import { api, type NetworkStats } from '@/lib/api';
import { SyncBanner } from '@/components/stats-cards';
import { HeroStatRow } from '@/components/hero-stat-row';
import { LatestActivities } from '@/components/latest-activities';
import { DaoOverview } from '@/components/dao-overview';
import { AssetEcosystem } from '@/components/asset-ecosystem';
import { ActivityTrend } from '@/components/activity-trend';
import { KnowledgeSizeTrend, NetworkHealth, ScriptUtilization } from '@/components/home-layer2';
import { LatestBlocks } from '@/components/latest-blocks';
import { LatestTransactions } from '@/components/latest-transactions';
import { useRealtimeData } from '@/hooks/use-realtime-data';
import { Link } from '@/components/ui/link';

export function HomeContent() {
  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10_000,
  });

  const { isConnected } = useRealtimeData();

  const showSyncBanner =
    stats?.syncStatus?.isSyncing && stats?.syncStatus?.syncMode === 'bulk';

  return (
    <div className="container mx-auto px-4 py-4 sm:py-6">
      {/* Sync Banner — only during bulk sync */}
      {showSyncBanner && stats && (
        <div className="mt-2">
          <SyncBanner stats={stats} />
        </div>
      )}

      {/* Hero Stat Row */}
      <div className="mt-4">
        <HeroStatRow stats={stats ?? null} />
      </div>

      {/* ═══ LAYER 1: DOMAIN KNOWLEDGE ═══ */}
      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <LatestActivities isRealtime={isConnected} />
        <DaoOverview />
      </div>

      <div className="mt-4 grid gap-4 lg:grid-cols-2">
        <AssetEcosystem />
        <ActivityTrend />
      </div>

      {/* ═══ LAYER 2: AGGREGATIONS ═══ */}
      <div className="mt-6 grid gap-4 lg:grid-cols-3">
        <KnowledgeSizeTrend />
        <NetworkHealth stats={stats ?? null} />
        <ScriptUtilization />
      </div>

      {/* Link cards */}
      <div className="mt-3 flex gap-4">
        <Link
          href="/charts/total-supply"
          className="text-text-dim hover:text-text-bright font-mono text-xs transition-colors"
        >
          Supply &amp; Economics →
        </Link>
        <Link
          href="/charts"
          className="text-text-dim hover:text-text-bright font-mono text-xs transition-colors"
        >
          All Charts →
        </Link>
      </div>

      {/* ═══ LAYER 0: RAW DATA ═══ */}
      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <LatestBlocks isRealtime={isConnected} compact />
        <LatestTransactions isRealtime={isConnected} compact />
      </div>

      {/* Live indicator */}
      {isConnected && (
        <div className="fixed bottom-4 right-4 z-50">
          <div className="terminal-card border-jade/30 bg-base-surface/80 flex items-center gap-1.5 border px-2 py-1 backdrop-blur-sm">
            <span className="bg-jade h-1.5 w-1.5 animate-pulse rounded-full" />
            <span className="text-jade font-mono text-[10px] uppercase tracking-wider">Live</span>
          </div>
        </div>
      )}
    </div>
  );
}
```

Note: The `compact` prop on LatestBlocks/LatestTransactions is new — see next step. Also remove the `InitialData` interface and `HomeContentProps` since we're now doing all data fetching inside each component. Update `page.tsx` to just render `<HomeContent />` without passing props.

**Step 2: Update page.tsx**

Simplify `frontend/app/page.tsx` — remove the `initialData` pattern since each component now fetches its own data:

```typescript
'use client';

import { HomeContent } from '@/components/home-content';
import { Header } from '@/components/layout/header';

export default function Home() {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <HomeContent />
    </div>
  );
}
```

Note: Check if `Header` is rendered in `page.tsx` or in the root `layout.tsx`. If it's in layout, don't include it here. The point is to simplify page.tsx by removing the query + initialData pattern — components now own their own data fetching.

**Step 3: Add `compact` prop to LatestBlocks**

In `frontend/components/latest-blocks.tsx`, add a `compact` prop that limits to 4 rows:

```typescript
interface LatestBlocksProps {
  isRealtime?: boolean;
  initialBlocks?: Block[];
  compact?: boolean; // NEW
}

export function LatestBlocks({ isRealtime, initialBlocks, compact }: LatestBlocksProps) {
  const limit = compact ? 4 : 10;
  // ... in the useQuery, use { limit } instead of { limit: 10 }
  // ... in the render, slice data to `limit` items
```

**Step 4: Add `compact` prop to LatestTransactions**

Same pattern in `frontend/components/latest-transactions.tsx`:

```typescript
interface LatestTransactionsProps {
  isRealtime?: boolean;
  initialTransactions?: Transaction[];
  compact?: boolean; // NEW
}
```

**Step 5: Run type check and build**

```bash
cd frontend && pnpm type-check && pnpm build
```

Fix any type errors from the refactoring.

**Step 6: Commit**

```bash
git add frontend/components/home-content.tsx frontend/app/page.tsx frontend/components/latest-blocks.tsx frontend/components/latest-transactions.tsx
git commit -m "feat(frontend): restructure homepage layout with domain-layered design"
```

---

## Task 10: Tests and Cleanup

Update existing homepage tests, remove unused imports, run full test suite.

**Files:**

- Modify: `frontend/__tests__/` (update homepage tests)
- Potentially remove unused component references

**Step 1: Update homepage snapshot/integration tests**

Check `frontend/__tests__/` for existing homepage tests. Update them to reflect the new component structure:

- Remove references to `HomeCharts`, `EpochProgress`, `MiniStatsCards`, `PipelinePreview`, `ActivityBreakdown`
- Add references to `HeroStatRow`, `DaoOverview`, `AssetEcosystem`, `ActivityTrend`, `KnowledgeSizeTrend`, `NetworkHealth`, `ScriptUtilization`

**Step 2: Run full frontend test suite**

```bash
cd frontend && npx vitest run
```

Fix any failures.

**Step 3: Run linting and formatting**

```bash
cd frontend && pnpm lint && pnpm type-check && pnpm format
```

**Step 4: Run full Rust test suite**

```bash
cargo test
```

**Step 5: Verify the app builds**

```bash
cd frontend && pnpm build
```

**Step 6: Visual check**

Start the dev server and verify the homepage looks correct:

```bash
pnpm dev
```

Open http://localhost:3000 and verify:

- Hero stat row shows 5 metrics (may show "—" if no local data)
- Layer 1 sections appear (activities, DAO, assets, activity trend)
- Layer 2 sections appear (knowledge size, network health, scripts)
- Layer 0 shows compact blocks + transactions
- Sync banner only shows during bulk sync
- No console errors

**Step 7: Clean up unused components**

The following components are no longer imported from the homepage. Check if they're used elsewhere before removing:

- `home-charts.tsx` — if only used on homepage, mark for removal or keep for other pages
- `chain-wave/epoch-progress.tsx` — may be used on other pages
- `chain-wave/pipeline-preview.tsx` — may be used on mempool page
- `mini-stats-cards.tsx` — if only used on homepage, mark for removal
- `activity-breakdown.tsx` — if only used on homepage, mark for removal

Do NOT delete components that are imported elsewhere. Only remove dead code.

**Step 8: Final commit**

```bash
git add -A
git commit -m "chore(frontend): update tests and clean up unused homepage imports"
```

---

## Summary of Changes

### Backend (2 tasks)

1. Extend `NetworkStats` with `knowledgeSize`, `circulatingSupply`, `daoLocked`
2. New `GET /statistics/asset-ecosystem` endpoint

### Frontend — New Components (5 tasks)

3. API types for new endpoints
4. `HeroStatRow` — 5 hero metrics
5. `DaoOverview` — DAO summary panel + spark chart
6. `AssetEcosystem` — top tokens + capacity bar
7. `ActivityTrend` — daily volume chart + type breakdown
8. `KnowledgeSizeTrend`, `NetworkHealth`, `ScriptUtilization` — L2 mini charts

### Frontend — Integration (2 tasks)

9. Restructure `home-content.tsx` layout + simplify `page.tsx` + add `compact` prop
10. Tests, cleanup, visual verification

### Removed from Homepage

- `HomeCharts` (block time/hash rate hero charts)
- `EpochProgress` (standalone epoch bar)
- `MiniStatsCards` (TX count widgets)
- `PipelinePreview` (mempool pipeline)
- `ActivityBreakdown` (pie charts)

### Data Flow

- Hero stat row: `getNetworkStats()` (extended with 3 DAO-derived fields)
- DAO Overview: `getDaoStatistics()` + `getDaoTotalDepositChart()`
- Asset Ecosystem: `getAssetEcosystem()` (new endpoint)
- Activity Trend: `getDailyActivityStats(14)` + `getActivitySummary24h()`
- Knowledge Size Trend: `getKnowledgeSizeChart()`
- Network Health: `getAverageBlockTimeChart()` + `getHashRateChart()` + NetworkStats
- Script Utilization: `getScripts({ limit: 5, sortKey: 'used', sortDirection: 'desc' })`
- Raw data: `getBlocks({ limit: 4 })` + `getTransactions({ limit: 4 })`
