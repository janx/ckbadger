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
import { api, type Asset, type InventorySummary } from '@/lib/api';
import type { CompositionTier } from '@/lib/asset-utils';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';

export type AssetType = Asset['assetType'];
type AssetSortKey = NonNullable<Parameters<typeof api.getAssets>[0]>['sortKey'];
// F2 objects (IV-03, IV-12): the board offers standard chips on Tokens only; Objects filter by
// composition, Identities list their four systems plainly. Round 3 (contracts R-A · R-3): the
// chips are the summary's `tokenStandards` (each count equals that filter's total), in the
// board's order; a standard the summary does not list has no catalog token and no chip.
const TOKEN_CHIP_ORDER = ['xudt', 'sudt', 'xudt_compatible'];
function tokenStandardChips(summary: InventorySummary): InventorySummary['tokenStandards'] {
  const sum = summary.tokenStandards.reduce((total, s) => total + s.count, 0);
  if (sum !== summary.counts.token)
    throw new Error(
      `Inventory summary: token standards sum to ${sum}, not the ${summary.counts.token} catalog tokens`
    );
  const place = (standard: string) => {
    const i = TOKEN_CHIP_ORDER.indexOf(standard);
    return i < 0 ? TOKEN_CHIP_ORDER.length : i;
  };
  return [...summary.tokenStandards].sort(
    (a, b) => place(a.standard) - place(b.standard) || a.standard.localeCompare(b.standard)
  );
}
interface InventoryPageClientProps {
  assetType: AssetType;
  title?: string;
  subtitle?: string;
  panelTitle?: string;
}
/** C1: the tab names the inventory view it shows. */
const INVENTORY_VIEW_TITLE: Record<AssetType, string> = {
  token: 'Tokens',
  object: 'Objects',
  identity: 'Identities',
};

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
  const pageSize = usePageSize(DEFAULT_PAGE_SIZE);
  const assets = useQuery({
    queryKey: [
      'assets',
      assetType,
      pagination.cursor,
      pageSize,
      search,
      standard,
      compositionTier,
      sortKey,
      sortDirection,
    ],
    queryFn: () =>
      api.getAssets({
        type: assetType,
        limit: pageSize,
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
          documentTitle={[INVENTORY_VIEW_TITLE[assetType], 'Inventory']}
          identity="Everything people hold on CKB besides plain CKB"
          actions={
            <div className="flex max-w-full flex-wrap gap-2">
              <label className="sr-only" htmlFor="inventory-search">
                Search the inventory
              </label>
              <input
                id="inventory-search"
                type="search"
                className="border-base-border bg-base-surface text-small min-w-0 rounded border px-3 py-2"
                placeholder="Search by name..."
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
              />
            </div>
          }
        >
          <nav
            aria-label="Inventory classes"
            // Below sm each count sits under its label: the three tabs in one line are ~414 px,
            // stacked ~330 px, so they share a 358 px line at full type size (B#25).
            className="border-base-border flex flex-wrap gap-x-5 gap-y-2 border-b sm:gap-x-6"
          >
            {(['token', 'object', 'identity'] as const).map((type) => {
              const plural =
                type === 'identity' ? 'identities' : (`${type}s` as 'tokens' | 'objects');
              return (
                <AppLink
                  key={type}
                  href={`/inventory/${plural}`}
                  aria-current={type === assetType ? 'page' : undefined}
                  className={`text-body grid grid-cols-[16px_auto] items-center gap-x-2 border-b-2 pb-3 sm:flex sm:items-center sm:gap-2 ${type === assetType ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
                >
                  <ClassMark kind={plural} size={16} />
                  <span data-role="tab-label">{plural[0].toUpperCase() + plural.slice(1)}</span>
                  {summary.data && (
                    <span data-role="tab-count" className="text-text-dim text-meta col-start-2">
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
          {assetType === 'token' && (
            <div
              className="hidden flex-wrap gap-2 sm:flex"
              role="group"
              aria-label="Standard filters"
            >
              {[
                { standard: '', count: null as number | null },
                ...(summary.data ? tokenStandardChips(summary.data) : []),
              ].map(({ standard: s, count }) => (
                <button
                  key={s}
                  type="button"
                  aria-pressed={(standard ?? '') === s}
                  className={`text-small rounded border px-3 py-1.5 ${standard === s || (!standard && !s) ? 'border-jade text-text-bright' : 'border-base-border text-text-dim'}`}
                  onClick={() => filter('standard', s)}
                >
                  {s ? (
                    <>
                      {formatStandard(s)} <AssetCount value={count!} />
                    </>
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
          {/* F2 objects (IV-07): the board's quiet "sorted by" line carries the sort controls.
              Board L3: below sm it also carries the standard filter as one select, so the tabs,
              the filter and the sort take two lines at 390 px. */}
          <div
            data-testid="inventory-controls"
            className="text-text-dim text-meta sm:text-small flex flex-wrap items-center gap-x-1.5 gap-y-1 sm:gap-x-2"
          >
            {assetType === 'token' && (
              <>
                <label htmlFor="inventory-standard" className="sr-only">
                  Standard filter
                </label>
                <select
                  id="inventory-standard"
                  value={standard ?? ''}
                  onChange={(e) => filter('standard', e.target.value)}
                  // Sized to the chosen standard, not the longest one (Chromium).
                  className="border-base-border bg-base-surface text-text h-7 min-w-0 rounded border px-1.5 [field-sizing:content] sm:hidden"
                >
                  {[
                    { standard: '', count: summary.data?.counts[assetType] ?? null },
                    ...(summary.data ? tokenStandardChips(summary.data) : []),
                  ].map(({ standard: s, count }) => (
                    <option key={s} value={s}>
                      {s ? formatStandard(s) : 'All'}
                      {count !== null && ` ${count.toLocaleString('en-US')}`}
                    </option>
                  ))}
                </select>
              </>
            )}
            {/* Below sm the words go to the screen reader so the filter, the sort and its
                direction share one line; the select names itself. */}
            <span className="sr-only sm:not-sr-only">sorted by</span>
            <select
              aria-label="Sort inventory"
              value={sortKey}
              onChange={(e) => {
                setSortKey(e.target.value as AssetSortKey);
                pagination.reset();
              }}
              className="text-text min-w-0 bg-transparent py-1"
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
              className="text-text hover:text-text-bright whitespace-nowrap"
              onClick={() => {
                setSortDirection(sortDirection === 'desc' ? 'asc' : 'desc');
                pagination.reset();
              }}
            >
              {sortDirection === 'desc' ? '↓' : '↑'}
              <span className="hidden sm:inline">
                {sortDirection === 'desc' ? ' largest first' : ' smallest first'}
              </span>
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
                pageSize={pageSize}
                page={pagination.page}
                currentCount={assets.data.data.length}
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
        <p role="status" className="text-text-dim text-small">
          Loading inventory…
        </p>
      </main>
    </div>
  );
}
