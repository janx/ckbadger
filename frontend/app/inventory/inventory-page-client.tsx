'use client';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ClassMark } from '@/components/ui/class-mark';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { HMulHelpPopover } from '@/components/ui/hmul-info';
import { InventorySummaryNotice } from '@/components/asset/inventory-summary';
import { InventoryRows, AssetCount } from '@/components/asset/inventory-table';
import { AddressLoadState, AddressStaleNotice } from '@/components/address/address-holdings';
import { formatStandard } from '@/components/activity-event-row';
import { formatCompositionTier } from '@/lib/asset-utils';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type Asset } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';

export type AssetType = Asset['assetType'];
type AssetSortKey = NonNullable<Parameters<typeof api.getAssets>[0]>['sortKey'];
const standards = {
  token: ['xudt', 'sudt'],
  object: ['spore', 'm-nft'],
  identity: ['dotbit', 'bit_cell', 'did_ckb', 'dotcell'],
};
const tiers = [
  'pure_ckb',
  'btc_ckb',
  'decentralized_mixture',
  'centralized_mixture',
  'unknown',
  'decode_pending',
] as const;
interface InventoryPageClientProps {
  assetType: AssetType;
  title?: string;
  subtitle?: string;
  panelTitle?: string;
}
export function InventoryPageClient({ assetType }: InventoryPageClientProps) {
  const router = useRouter(),
    pathname = usePathname(),
    params = useSearchParams();
  const pagination = useCursorPagination();
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string>();
  const [standard, setStandard] = useState<string | undefined>(
    params.get('standard')?.trim().toLowerCase() || undefined
  );
  const [compositionTier, setCompositionTier] = useState<string | undefined>(
    params.get('compositionTier') || undefined
  );
  const [sortKey, setSortKey] = useState<AssetSortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');
  const summary = useQuery({
    queryKey: ['inventory-summary'],
    queryFn: api.getInventorySummary,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const assets = useQuery({
    queryKey: [
      'assets',
      assetType,
      pagination.cursor,
      search,
      standard,
      compositionTier,
      sortKey,
      sortDirection,
    ],
    queryFn: () =>
      api.getAssets({
        type: assetType,
        limit: DEFAULT_PAGE_SIZE,
        cursor: pagination.cursor,
        search,
        standard,
        compositionTier:
          assetType === 'object' ? (compositionTier as (typeof tiers)[number]) : undefined,
        sortKey,
        sortDirection,
      }),
  });
  const filter = (key: 'standard' | 'compositionTier', value: string) => {
    const next = value || undefined;
    if (key === 'standard') setStandard(next);
    else setCompositionTier(next);
    pagination.reset();
    const query = new URLSearchParams(params.toString());
    if (next) query.set(key, next);
    else query.delete(key);
    router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
  };
  const sortOptions: [AssetSortKey, string][] = [
    ['capacity', 'CKB held'],
    ['name', 'Name'],
    ['type', 'Standard'],
    ['holders', 'Holders'],
    ['used', 'Occupied'],
    ['supply', 'Supply / minted'],
    ['transfers24h', '24 h activity'],
    ['hMultiplier', 'H-mult.'],
  ];
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        <DetailLayout
          kind="index"
          title="Inventory"
          identity="Everything people hold on CKB besides plain CKB"
          actions={
            <form
              onSubmit={(event) => {
                event.preventDefault();
                setSearch(searchInput.trim() || undefined);
                pagination.reset();
              }}
              className="flex max-w-full flex-wrap gap-2"
            >
              <label className="sr-only" htmlFor="inventory-search">
                Search the inventory
              </label>
              <input
                id="inventory-search"
                className="border-base-border bg-base-surface min-w-0 rounded border px-3 py-2 text-xs"
                placeholder="Search by name..."
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
              />
              <button className="border-base-border rounded border px-3 text-xs">Search</button>
            </form>
          }
        >
          <nav
            aria-label="Inventory classes"
            className="border-base-border flex flex-wrap gap-6 border-b"
          >
            {(['token', 'object', 'identity'] as const).map((type) => {
              const plural =
                type === 'identity' ? 'identities' : (`${type}s` as 'tokens' | 'objects');
              return (
                <AppLink
                  key={type}
                  href={`/inventory/${plural}`}
                  aria-current={type === assetType ? 'page' : undefined}
                  className={`flex items-center gap-2 border-b-2 pb-3 text-[15px] ${type === assetType ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
                >
                  <ClassMark kind={plural} size={16} />
                  {plural[0].toUpperCase() + plural.slice(1)}
                  {summary.data && (
                    <span className="text-text-dim text-[11px]">
                      <AssetCount value={summary.data.counts[type]} />
                    </span>
                  )}
                </AppLink>
              );
            })}
          </nav>
          <AddressStaleNotice error={summary.error} label="inventory totals" />
          {summary.data ? (
            <InventorySummaryNotice summary={summary.data} assetType={assetType} />
          ) : (
            <AddressLoadState
              error={summary.error}
              loading={summary.isLoading}
              label="inventory totals"
            />
          )}
          <div
            className="hidden flex-wrap gap-2 sm:flex"
            role="group"
            aria-label="Standard filters"
          >
            {['', ...standards[assetType]].map((s) => (
              <button
                key={s}
                type="button"
                aria-pressed={(standard ?? '') === s}
                className={`rounded border px-3 py-1.5 text-xs ${standard === s || (!standard && !s) ? 'border-jade text-text-bright' : 'border-base-border text-text-dim'}`}
                onClick={() => filter('standard', s)}
              >
                {s ? formatStandard(s) : 'All standards'}
              </button>
            ))}
          </div>
          {assetType === 'object' && (
            <div
              className="hidden flex-wrap gap-2 sm:flex"
              role="group"
              aria-label="Composition filters"
            >
              {['', ...tiers].map((t) => (
                <button
                  key={t}
                  type="button"
                  aria-pressed={(compositionTier ?? '') === t}
                  className={`rounded border px-3 py-1.5 text-xs ${compositionTier === t || (!compositionTier && !t) ? 'border-jade text-text-bright' : 'border-base-border text-text-dim'}`}
                  onClick={() => filter('compositionTier', t)}
                >
                  {t ? formatCompositionTier(t) : 'All compositions'}
                </button>
              ))}
            </div>
          )}
          <div className="flex flex-wrap items-center gap-3 text-xs">
            <label className="text-text-dim sm:hidden">
              Standard{' '}
              <select
                aria-label="Filter by standard"
                value={standard ?? ''}
                onChange={(e) => filter('standard', e.target.value)}
                className="border-base-border bg-base-surface ml-2 rounded border p-2"
              >
                <option value="">All standards</option>
                {[...new Set([...standards[assetType], ...(standard ? [standard] : [])])].map(
                  (s) => (
                    <option key={s} value={s}>
                      {formatStandard(s)}
                    </option>
                  )
                )}
              </select>
            </label>
            {assetType === 'object' && (
              <label className="text-text-dim sm:hidden">
                Composition{' '}
                <select
                  aria-label="Filter by composition tier"
                  value={compositionTier ?? ''}
                  onChange={(e) => filter('compositionTier', e.target.value)}
                  className="border-base-border bg-base-surface ml-2 max-w-full rounded border p-2"
                >
                  <option value="">All compositions</option>
                  {tiers.map((t) => (
                    <option key={t} value={t}>
                      {formatCompositionTier(t)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="text-text-dim sm:ml-auto">
              Sort by{' '}
              <select
                aria-label="Sort inventory"
                value={sortKey}
                onChange={(e) => {
                  setSortKey(e.target.value as AssetSortKey);
                  pagination.reset();
                }}
                className="border-base-border bg-base-surface ml-2 rounded border p-2"
              >
                {sortOptions.map(([key, label]) => (
                  <option key={key} value={key}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <button
              aria-label="Reverse sort direction"
              className="border-base-border rounded border p-2"
              onClick={() => {
                setSortDirection(sortDirection === 'desc' ? 'asc' : 'desc');
                pagination.reset();
              }}
            >
              {sortDirection === 'desc' ? 'Descending' : 'Ascending'}
            </button>
            <HMulHelpPopover />
          </div>
          {assets.data ? (
            <>
              <AddressStaleNotice error={assets.error} label="inventory" />
              <InventoryRows rows={assets.data.data} assetType={assetType} />
              <CursorPagination
                total={assets.data.total ?? undefined}
                totalLabel="items"
                pageSize={DEFAULT_PAGE_SIZE}
                page={pagination.page}
                hasMore={assets.data.hasMore}
                hasPrevious={pagination.hasPrevious}
                onNext={() => pagination.goToNext(assets.data.nextCursor)}
                onPrevious={pagination.goToPrevious}
              />
            </>
          ) : (
            <AddressLoadState error={assets.error} loading={assets.isLoading} label="inventory" />
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
export function InventoryPageFallback() {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <p role="status" className="text-text-dim text-xs">
          Loading inventory…
        </p>
      </main>
    </div>
  );
}
