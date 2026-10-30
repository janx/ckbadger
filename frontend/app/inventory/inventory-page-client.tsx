'use client';
import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ClassMark } from '@/components/ui/class-mark';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { HMulHelpPopover } from '@/components/ui/hmul-info';
import { InventorySummaryNotice, ObjectComposition } from '@/components/asset/inventory-summary';
import { InventoryRows, AssetCount } from '@/components/asset/inventory-table';
import { AddressLoadState, AddressStaleNotice } from '@/components/address/address-holdings';
import { formatStandard } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type Asset } from '@/lib/api';
import type { CompositionTier } from '@/lib/asset-utils';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';

export type AssetType = Asset['assetType'];
type AssetSortKey = NonNullable<Parameters<typeof api.getAssets>[0]>['sortKey'];
// F2 objects (IV-03, IV-12): the board offers standard chips on Tokens only (xUDT-compatible
// included); Objects filter by composition, Identities list their four systems plainly.
const standards: Record<AssetType, string[]> = {
  token: ['xudt', 'sudt', 'xudt_compatible'],
  object: [],
  identity: [],
};
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
  const resetPage = pagination.reset;
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string>();
  // F2 objects (IV-07): the board's search box filters as you type (debounced), no button.
  useEffect(() => {
    const next = searchInput.trim() || undefined;
    if (next === search) return;
    const timer = setTimeout(() => {
      setSearch(next);
      resetPage();
    }, 250);
    return () => clearTimeout(timer);
  }, [searchInput, search, resetPage]);
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
          assetType === 'object' ? (compositionTier as CompositionTier | undefined) : undefined,
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
    ['capacity', 'CKB held in their cells'],
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
            <div className="flex max-w-full flex-wrap gap-2">
              <label className="sr-only" htmlFor="inventory-search">
                Search the inventory
              </label>
              <input
                id="inventory-search"
                type="search"
                className="border-base-border bg-base-surface min-w-0 rounded border px-3 py-2 text-xs"
                placeholder="Search by name..."
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
              />
            </div>
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
          {assetType === 'object' && summary.data && (
            <ObjectComposition
              summary={summary.data}
              selected={compositionTier}
              onSelect={(tier) => filter('compositionTier', tier)}
            />
          )}
          {standards[assetType].length > 0 && (
            <div className="flex flex-wrap gap-2" role="group" aria-label="Standard filters">
              {['', ...standards[assetType]].map((s) => (
                <button
                  key={s}
                  type="button"
                  aria-pressed={(standard ?? '') === s}
                  className={`rounded border px-3 py-1.5 text-xs ${standard === s || (!standard && !s) ? 'border-jade text-text-bright' : 'border-base-border text-text-dim'}`}
                  onClick={() => filter('standard', s)}
                >
                  {s ? (
                    formatStandard(s)
                  ) : (
                    <>
                      All
                      {summary.data && (
                        <>
                          {' '}
                          <AssetCount value={summary.data.counts[assetType]} />
                        </>
                      )}
                    </>
                  )}
                </button>
              ))}
            </div>
          )}
          {/* F2 objects (IV-07): the board's quiet "sorted by" line carries the sort controls. */}
          <div className="text-text-dim flex flex-wrap items-center gap-2 text-[12px]">
            <span>sorted by</span>
            <select
              aria-label="Sort inventory"
              value={sortKey}
              onChange={(e) => {
                setSortKey(e.target.value as AssetSortKey);
                pagination.reset();
              }}
              className="text-text bg-transparent py-1"
            >
              {sortOptions.map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
            <button
              type="button"
              aria-label="Reverse sort direction"
              className="text-text hover:text-text-bright"
              onClick={() => {
                setSortDirection(sortDirection === 'desc' ? 'asc' : 'desc');
                pagination.reset();
              }}
            >
              {sortDirection === 'desc' ? '↓ largest first' : '↑ smallest first'}
            </button>
            {assetType !== 'identity' && <HMulHelpPopover />}
          </div>
          {summary.data ? (
            <InventorySummaryNotice summary={summary.data} assetType={assetType} />
          ) : (
            <AddressLoadState
              error={summary.error}
              loading={summary.isLoading}
              label="inventory totals"
            />
          )}
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
