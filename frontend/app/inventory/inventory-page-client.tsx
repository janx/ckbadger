'use client';
import { TopicCharts } from '@/components/charts/topic-charts';
import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { DetailLayout } from '@/components/ui/detail-layout';
import { FilterChips, type FilterValue } from '@/components/ui/filter-chips';
import { HMulHelpPopover } from '@/components/ui/hmul-info';
import { Pager } from '@/components/ui/pager';
import { Select } from '@/components/ui/select';
import { TabLinks } from '@/components/ui/tabs';
import { TextField } from '@/components/ui/text-field';
import { InventorySummaryNotice, ObjectComposition } from '@/components/asset/inventory-summary';
import { InventoryRows } from '@/components/asset/inventory-table';
import { AddressLoadState, AddressStaleNotice } from '@/components/address/address-holdings';
import { formatStandard } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type Asset, type InventorySummary } from '@/lib/api';
import type { CompositionTier } from '@/lib/asset-utils';
import { DEFAULT_PAGE_SIZE, rankedPageAt } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';

export type AssetType = Asset['assetType'];
type AssetSortKey = NonNullable<NonNullable<Parameters<typeof api.getAssets>[0]>['sortKey']>;
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
/** The filter's "all": no standard in the request or the URL. */
const ALL_STANDARDS = '';
/**
 * The token standard filter's values (the controls board's chips; a select after "standard" where
 * they do not fit a phone's line): all, then the summary's standards with their counts, in the
 * board's words. A standard the URL chose that the summary does not list has no catalog token, by
 * the summary's own count: it stands as the chosen value with none (0), so the filter says what the
 * list is filtered by.
 */
function standardValues(
  summary: InventorySummary,
  chosen: string | undefined
): FilterValue<string>[] {
  const listed = tokenStandardChips(summary);
  const values: FilterValue<string>[] = [
    { key: ALL_STANDARDS, label: 'all', count: summary.counts.token },
    ...listed.map((s) => ({
      key: s.standard,
      label: formatStandard(s.standard).toLowerCase(),
      count: s.count,
    })),
  ];
  if (chosen !== undefined && !listed.some((s) => s.standard === chosen))
    values.push({ key: chosen, label: formatStandard(chosen).toLowerCase(), count: 0 });
  return values;
}
/** The sort select's choices, in the controls board's words. */
const SORTS: readonly { value: AssetSortKey; label: string }[] = [
  { value: 'capacity', label: 'ckb held in its cells' },
  { value: 'name', label: 'name' },
  { value: 'type', label: 'standard' },
  { value: 'holders', label: 'holders' },
  { value: 'used', label: 'occupied' },
  { value: 'supply', label: 'supply / minted' },
  { value: 'transfers24h', label: '24 h activity' },
  { value: 'hMultiplier', label: 'h-mult.' },
];
/** The three classes: each a tab of the command line, its name the last part of its address. */
const INVENTORY_CLASSES: readonly { key: AssetType; label: string }[] = [
  { key: 'token', label: 'tokens' },
  { key: 'object', label: 'objects' },
  { key: 'identity', label: 'identities' },
];
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
  const pageSize = usePageSize(DEFAULT_PAGE_SIZE);
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string>();
  // F2 objects (IV-07): the board's search box filters as you type (debounced), no button.
  useEffect(() => {
    const next = searchInput.trim() || undefined;
    if (next === search) return;
    const timer = setTimeout(() => {
      setSearch(next);
    }, 250);
    return () => clearTimeout(timer);
  }, [searchInput, search]);
  const [standard, setStandard] = useState<string | undefined>(
    params.get('standard')?.trim().toLowerCase() || undefined
  );
  const [compositionTier, setCompositionTier] = useState<string | undefined>(
    params.get('compositionTier') || undefined
  );
  const [sortKey, setSortKey] = useState<AssetSortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');
  // a change of page size at md starts the walk again (lane c2-finish, 1d), and so does a change of the list it pages (lane cd-fixes-controls): its class,
  // search, standard, tier and sort
  const pagination = useCursorPagination(pageSize, {
    list: [assetType, search, standard, compositionTier, sortKey, sortDirection],
  });
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
      pageSize,
      search,
      standard,
      compositionTier,
      sortKey,
      sortDirection,
    ],
    queryFn: pagination.ask((cursor) =>
      api.getAssets({
        type: assetType,
        limit: pageSize,
        cursor,
        search,
        standard,
        compositionTier:
          assetType === 'object' ? (compositionTier as CompositionTier | undefined) : undefined,
        sortKey,
        sortDirection,
      })
    ),
  });
  const classLabel = INVENTORY_CLASSES.find((c) => c.key === assetType)!.label;
  // the shown page's figures (lib/pagination.ts); nothing for one page, an empty page past the
  // end its place and a way back (lane c2-finish, 1b)
  const assetsAt = assets.data ? rankedPageAt(classLabel, assets.data, pagination) : null;
  const filter = (key: 'standard' | 'compositionTier', value: string) => {
    const next = value || undefined;
    if (key === 'standard') setStandard(next);
    else setCompositionTier(next);
    const query = new URLSearchParams(params.toString());
    if (next) query.set(key, next);
    else query.delete(key);
    router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
  };
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        <DetailLayout
          kind="index"
          title="Inventory"
          documentTitle={[INVENTORY_VIEW_TITLE[assetType], 'Inventory']}
          identity="Everything people hold on CKB besides plain CKB"
        >
          {/* The controls board's inventory head: the classes are the command line, then the
              filter with the search at its right, then the sort after "sorted by". Each class is
              its own address, so the row is the tab row's links (a step of history, a class opens
              in a new tab; lane c2-finish, item 3). */}
          <TabLinks
            label="Inventory classes"
            command
            tabs={INVENTORY_CLASSES.map(({ key, label }) => ({
              key,
              label,
              count: summary.data?.counts[key],
              href: `/inventory/${label}`,
            }))}
            current={assetType}
          />
          <div className="mt-8 space-y-8">
            <AddressStaleNotice error={summary.error} label="inventory totals" />
            {assetType === 'object' && summary.data && (
              <ObjectComposition
                summary={summary.data}
                selected={compositionTier}
                onSelect={(tier) => filter('compositionTier', tier)}
              />
            )}
            <div className="flex flex-wrap items-center gap-2.5">
              {assetType === 'token' && summary.data && (
                <FilterChips
                  label="standard"
                  values={standardValues(summary.data, standard)}
                  value={standard ?? ALL_STANDARDS}
                  onChange={(next) => filter('standard', next)}
                  counted
                />
              )}
              <TextField
                label="Search the inventory"
                search
                id="inventory-search"
                placeholder="search by name"
                value={searchInput}
                onChange={setSearchInput}
                className="min-w-[160px] flex-1 sm:ml-auto sm:w-40 sm:flex-none"
              />
            </div>
            <div
              data-testid="inventory-controls"
              className="text-text-dim text-meta sm:text-small flex flex-wrap items-center gap-x-1.5 gap-y-1 sm:gap-x-2"
            >
              <Select label="sorted by" value={sortKey} options={SORTS} onChange={setSortKey} />
              <button
                type="button"
                aria-label="Reverse sort direction"
                className="text-text hover:text-text-bright whitespace-nowrap"
                onClick={() => setSortDirection(sortDirection === 'desc' ? 'asc' : 'desc')}
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
              <AddressLoadState error={summary.error} read={summary} label="inventory totals" />
            )}
            {assets.data ? (
              <>
                <AddressStaleNotice error={assets.error} label="inventory" />
                <InventoryRows rows={assets.data.data} assetType={assetType} />
                {assetsAt && <Pager mode="pages" label={classLabel} {...assetsAt} />}
              </>
            ) : (
              <AddressLoadState error={assets.error} read={assets} label="inventory" />
            )}
          </div>
        </DetailLayout>
        <TopicCharts topic="/inventory" />
      </main>
    </div>
  );
}
export function InventoryPageFallback() {
  return (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">
        <p role="status" className="text-text text-small">
          Loading inventory…
        </p>
      </main>
    </div>
  );
}
