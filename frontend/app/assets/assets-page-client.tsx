'use client';

import { useEffect, useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from 'next/link';
import Image from 'next/image';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { api, Asset } from '@/lib/api';
import { toNftDetailSlug } from '@/lib/nft-collections';
import { formatCkbCompact } from '@/lib/utils';

type AssetTab = 'token' | 'nft';
type SortDirection = 'asc' | 'desc';
type StorageTierFilter = 'all' | 'fully_onchain' | 'offchain_dependent' | 'unknown';
type AssetSortKey =
  | 'name'
  | 'type'
  | 'supply'
  | 'transfers24h'
  | 'holders'
  | 'transfers'
  | 'occupied'
  | 'capacity'
  | 'onchainRatio';

const TOKEN_STANDARD_OPTIONS = ['xudt', 'sudt'];
const NFT_STANDARD_OPTIONS = ['spore', 'm-nft', 'dotbit', 'did:ckb'];
const STORAGE_TIER_OPTIONS: StorageTierFilter[] = [
  'all',
  'fully_onchain',
  'offchain_dependent',
  'unknown',
];

function normalizeAssetTab(value: string | null): AssetTab {
  if (value === 'dob') {
    // Backward compatibility: old assets links used ?type=dob.
    return 'nft';
  }
  if (value === 'nft' || value === 'token') {
    return value;
  }
  return 'token';
}

function normalizeStandardFilter(value: string | null): string | undefined {
  if (!value) {
    return undefined;
  }
  const normalized = value.trim().toLowerCase();
  return normalized.length > 0 ? normalized : undefined;
}

function normalizeStorageTier(value: string | null): StorageTierFilter {
  if (!value) {
    return 'all';
  }
  const normalized = value.trim().toLowerCase();
  if (normalized === 'decentralized_external' || normalized === 'centralized_dependent') {
    return 'offchain_dependent';
  }
  switch (normalized) {
    case 'all':
      return 'all';
    case 'fully_onchain':
      return 'fully_onchain';
    case 'offchain_dependent':
      return 'offchain_dependent';
    case 'unknown':
      return 'unknown';
    default:
      return 'all';
  }
}

function formatStorageTierLabel(value: StorageTierFilter): string {
  switch (value) {
    case 'fully_onchain':
      return 'Fully On-chain';
    case 'offchain_dependent':
      return 'Offchain Dependent';
    case 'unknown':
      return 'Unknown';
    default:
      return 'All Storage';
  }
}

function formatStandardLabel(standard: string): string {
  switch (standard) {
    case 'xudt':
      return 'xUDT';
    case 'sudt':
      return 'sUDT';
    case 'm-nft':
      return 'm-NFT';
    case 'did_ckb':
    case 'did:ckb':
      return 'did:ckb';
    case 'dotbit':
      return 'DOTBIT';
    default:
      return standard.toUpperCase();
  }
}

function getStandardOptions(assetType: AssetTab, selectedStandard?: string) {
  const options = assetType === 'token' ? TOKEN_STANDARD_OPTIONS : NFT_STANDARD_OPTIONS;
  if (selectedStandard && !options.includes(selectedStandard)) {
    return [...options, selectedStandard];
  }
  return options;
}

function AssetTable({
  assetType,
  search,
  standard,
  storageTier,
}: {
  assetType: AssetTab;
  search: string | undefined;
  standard: string | undefined;
  storageTier: StorageTierFilter;
}) {
  const pagination = useCursorPagination();
  const { reset } = pagination;
  const [sortKey, setSortKey] = useState<AssetSortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');

  useEffect(() => {
    setSortKey('capacity');
    setSortDirection('desc');
    reset();
  }, [assetType, standard, storageTier, reset]);

  const { data, isLoading } = useQuery({
    queryKey: [
      'assets',
      assetType,
      pagination.cursor,
      search,
      standard,
      sortKey,
      sortDirection,
      storageTier,
    ],
    queryFn: () =>
      api.getAssets({
        limit: 20,
        type: assetType,
        cursor: pagination.cursor,
        search,
        standard,
        sortKey,
        sortDirection,
        storageTier: assetType === 'nft' && storageTier !== 'all' ? storageTier : undefined,
      }),
    placeholderData: keepPreviousData,
  });

  const assets = data?.data ?? [];
  const tableMinWidthClass = assetType === 'token' ? 'min-w-[1040px]' : 'min-w-[1120px]';
  const nameColumnClass = 'min-w-[17rem] flex-[1.8_0_17rem] pr-4';
  const typeColumnClass = 'w-24 shrink-0';
  const smallNumberColumnClass = 'w-24 shrink-0 whitespace-nowrap text-right';
  const mediumNumberColumnClass = 'w-28 shrink-0 whitespace-nowrap text-right';
  const capacityColumnClass = 'w-32 shrink-0 whitespace-nowrap text-right';

  const formatNumber = (num: number | string) => {
    return new Intl.NumberFormat().format(Number(num));
  };

  const shortHash = (value: string) => {
    if (value.length <= 20) return value;
    return `${value.slice(0, 10)}...${value.slice(-8)}`;
  };

  const getAssetLink = (asset: Asset) => {
    if (asset.assetType === 'token') return `/tokens/${asset.id}`;
    if (asset.standard === 'spore') {
      return `/clusters/${asset.clusterId || asset.id}`;
    }
    return `/nfts/${toNftDetailSlug(asset.id, asset.standard)}`;
  };

  const getAssetName = (asset: Asset) => {
    if (asset.assetType === 'token') {
      return asset.symbol || asset.name || shortHash(asset.id);
    }
    return asset.name || 'Unnamed Collection';
  };

  const getTypeBadgeLabel = (asset: Asset) => formatStandardLabel(asset.standard);
  const getStorageBadgeLabel = (asset: Asset) => {
    const tier = asset.storageTier;
    if (!tier) return null;
    if (tier === 'fully_onchain') return 'FULLY ON-CHAIN';
    if (
      tier === 'offchain_dependent' ||
      tier === 'decentralized_external' ||
      tier === 'centralized_dependent'
    ) {
      return 'OFFCHAIN DEPENDENT';
    }
    return 'UNKNOWN';
  };

  const toggleSort = (nextKey: AssetSortKey) => {
    if (nextKey === sortKey) {
      setSortDirection((prev) => (prev === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortKey(nextKey);
      setSortDirection(nextKey === 'name' || nextKey === 'type' ? 'asc' : 'desc');
    }
    pagination.reset();
  };

  const renderSortHeader = (
    key: AssetSortKey,
    label: string,
    className: string,
    align: 'left' | 'right' = 'left'
  ) => (
    <button
      type="button"
      className={`${className} flex items-center gap-1 whitespace-nowrap ${align === 'right' ? 'justify-end text-right' : ''}`}
      onClick={() => toggleSort(key)}
      aria-label={`Sort by ${label}`}
    >
      <span>{label}</span>
      <span className={sortKey === key ? 'text-terminal-green' : 'text-slate-700'}>
        {sortKey === key ? (sortDirection === 'asc' ? '↑' : '↓') : '↕'}
      </span>
    </button>
  );

  if (isLoading) {
    return (
      <div className="space-y-2 py-4">
        <div className="px-4 pb-1 text-xs text-slate-500 md:hidden">
          Swipe horizontally to view all columns.
        </div>
        <div className="overflow-x-auto" data-testid="asset-table-scroll">
          <div className={tableMinWidthClass} data-testid="asset-table-inner">
            {Array.from({ length: 5 }).map((_, i) => (
              <TerminalRow key={i} hoverable={false}>
                <div className="flex animate-pulse items-center">
                  <div className={nameColumnClass}>
                    <div className="h-4 w-48 rounded bg-slate-800" />
                  </div>
                  <div className={typeColumnClass}>
                    <div className="h-4 w-12 rounded bg-slate-800" />
                  </div>
                  {assetType !== 'token' && (
                    <div className={smallNumberColumnClass}>
                      <div className="ml-auto h-4 w-10 rounded bg-slate-800" />
                    </div>
                  )}
                  <div className={smallNumberColumnClass}>
                    <div className="ml-auto h-4 w-12 rounded bg-slate-800" />
                  </div>
                  <div className={mediumNumberColumnClass}>
                    <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
                  </div>
                  <div className={capacityColumnClass}>
                    <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
                  </div>
                  <div className={capacityColumnClass}>
                    <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
                  </div>
                </div>
              </TerminalRow>
            ))}
          </div>
        </div>
      </div>
    );
  }

  if (!data?.data?.length) {
    return <div className="py-8 text-center text-slate-500">No assets found</div>;
  }

  return (
    <>
      <div className="px-4 pb-1 pt-3 text-xs text-slate-500 md:hidden">
        Swipe horizontally to view all columns.
      </div>
      <div className="overflow-x-auto" data-testid="asset-table-scroll">
        <div className={tableMinWidthClass} data-testid="asset-table-inner">
          <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
            {renderSortHeader(
              'name',
              assetType === 'token' ? 'Token' : 'Collection',
              nameColumnClass
            )}
            {renderSortHeader('type', 'Standard', typeColumnClass)}
            {assetType !== 'token' &&
              renderSortHeader('supply', 'Items', smallNumberColumnClass, 'right')}
            {renderSortHeader('transfers24h', '24h Txns', smallNumberColumnClass, 'right')}
            {renderSortHeader('holders', 'Holders', mediumNumberColumnClass, 'right')}
            {renderSortHeader('occupied', 'Occupied (CKB)', capacityColumnClass, 'right')}
            {renderSortHeader('capacity', 'Capacity (CKB)', capacityColumnClass, 'right')}
          </div>
          {assets.map((asset: Asset) => (
            <TerminalRow key={asset.id}>
              <div className="flex items-center">
                <div className={nameColumnClass}>
                  <Link href={getAssetLink(asset)} className="block">
                    <div className="flex items-center gap-2">
                      <span
                        data-testid="asset-icon-slot"
                        className="flex h-6 w-6 shrink-0 items-center justify-center"
                      >
                        {asset.assetType === 'token' && asset.iconUrl && (
                          <Image
                            src={asset.iconUrl}
                            alt=""
                            className="h-6 w-6 rounded-full"
                            width={24}
                            height={24}
                            unoptimized
                            onError={(event) => {
                              event.currentTarget.style.visibility = 'hidden';
                            }}
                          />
                        )}
                        {asset.assetType === 'nft' && asset.standard === 'spore' && (
                          <span className="text-sm leading-none">🗂️</span>
                        )}
                      </span>
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5">
                          <span
                            className="text-terminal-green max-w-full truncate font-medium hover:underline"
                            title={getAssetName(asset)}
                          >
                            {getAssetName(asset)}
                          </span>
                          {asset.assetType === 'nft' && getStorageBadgeLabel(asset) && (
                            <span className="rounded border border-slate-700 px-1.5 py-0.5 font-mono text-[10px] text-slate-300">
                              {getStorageBadgeLabel(asset)}
                            </span>
                          )}
                          {asset.published && (
                            <span className="text-terminal-green" title="Verified">
                              <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 20 20">
                                <path
                                  fillRule="evenodd"
                                  d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                                  clipRule="evenodd"
                                />
                              </svg>
                            </span>
                          )}
                          {asset.famous && (
                            <span className="text-amber" title="Famous">
                              <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 20 20">
                                <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                              </svg>
                            </span>
                          )}
                        </div>
                        <HexDisplay
                          value={asset.id}
                          color="accent"
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                    </div>
                  </Link>
                </div>
                <div className={typeColumnClass}>
                  <div className="flex flex-wrap gap-1">
                    <Badge variant="neutral">{getTypeBadgeLabel(asset)}</Badge>
                  </div>
                </div>
                {assetType !== 'token' && (
                  <div className={`${smallNumberColumnClass} font-mono tabular-nums text-white`}>
                    {formatNumber(asset.totalSupply || 0)}
                  </div>
                )}
                <div className={`${smallNumberColumnClass} text-amber font-mono tabular-nums`}>
                  {formatNumber(asset.transfers24h)}
                </div>
                <div className={`${mediumNumberColumnClass} font-mono tabular-nums text-slate-400`}>
                  {formatNumber(asset.holdersCount)}
                </div>
                <div className={`${capacityColumnClass} font-mono tabular-nums text-slate-300`}>
                  {(() => {
                    const occupied = asset.liveOccupiedCapacity;
                    if (!occupied) {
                      return <span className="text-slate-500">-</span>;
                    }
                    const compact = formatCkbCompact(occupied);
                    return <span title={`${compact.full} CKB`}>{compact.value}</span>;
                  })()}
                </div>
                <div className={`${capacityColumnClass} font-mono tabular-nums text-slate-300`}>
                  {(() => {
                    const capacity = asset.liveCapacity;
                    if (!capacity) {
                      return <span className="text-slate-500">-</span>;
                    }
                    const compact = formatCkbCompact(capacity);
                    return <span title={`${compact.full} CKB`}>{compact.value}</span>;
                  })()}
                </div>
              </div>
            </TerminalRow>
          ))}
        </div>
      </div>
      <TerminalPanelFooter>
        <CursorPagination
          total={data.total ?? undefined}
          totalLabel="items"
          pageSize={20}
          page={pagination.page}
          hasMore={data.hasMore}
          hasPrevious={pagination.hasPrevious}
          onNext={() => pagination.goToNext(data.nextCursor)}
          onPrevious={pagination.goToPrevious}
        />
      </TerminalPanelFooter>
    </>
  );
}

export function AssetsPageClient() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const [activeTab, setActiveTab] = useState<AssetTab>(() =>
    normalizeAssetTab(searchParams.get('type'))
  );
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>(undefined);
  const [standard, setStandard] = useState<string | undefined>(() =>
    normalizeStandardFilter(searchParams.get('standard'))
  );
  const [storageTier, setStorageTier] = useState<StorageTierFilter>(() =>
    normalizeStorageTier(searchParams.get('storageTier'))
  );

  useEffect(() => {
    const tabFromUrl = normalizeAssetTab(searchParams.get('type'));
    setActiveTab((prev) => (prev === tabFromUrl ? prev : tabFromUrl));
    const standardFromUrl = normalizeStandardFilter(searchParams.get('standard'));
    setStandard((prev) => (prev === standardFromUrl ? prev : standardFromUrl));
    const storageTierFromUrl = normalizeStorageTier(searchParams.get('storageTier'));
    setStorageTier((prev) => (prev === storageTierFromUrl ? prev : storageTierFromUrl));
  }, [searchParams]);

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    setSearch(searchInput.trim() || undefined);
  };

  const clearSearch = () => {
    setSearchInput('');
    setSearch(undefined);
  };

  const handleTabChange = (value: string) => {
    const nextTab = normalizeAssetTab(value);
    setActiveTab(nextTab);
    setSearch(undefined);
    setSearchInput('');
    setStandard(undefined);
    setStorageTier('all');
    const params = new URLSearchParams(searchParams.toString());
    params.set('type', nextTab);
    params.delete('standard');
    params.delete('storageTier');
    const query = params.toString();
    router.replace(query ? `${pathname}?${query}` : pathname, { scroll: false });
  };

  const handleStandardChange = (value: string) => {
    const nextStandard = normalizeStandardFilter(value);
    setStandard(nextStandard);
    const params = new URLSearchParams(searchParams.toString());
    if (nextStandard) {
      params.set('standard', nextStandard);
    } else {
      params.delete('standard');
    }
    const query = params.toString();
    router.replace(query ? `${pathname}?${query}` : pathname, { scroll: false });
  };

  const handleStorageTierChange = (value: string) => {
    const nextStorageTier = normalizeStorageTier(value);
    setStorageTier(nextStorageTier);
    const params = new URLSearchParams(searchParams.toString());
    if (nextStorageTier === 'all') {
      params.delete('storageTier');
    } else {
      params.set('storageTier', nextStorageTier);
    }
    const query = params.toString();
    router.replace(query ? `${pathname}?${query}` : pathname, { scroll: false });
  };

  const standardOptions = getStandardOptions(activeTab, standard);

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Assets"
          subtitle="Browse tokens and NFTs on the CKB network"
          actions={
            <form
              onSubmit={handleSearch}
              className="flex w-full flex-wrap justify-end gap-2 sm:w-auto"
            >
              <div className="relative w-full sm:w-auto">
                <input
                  type="text"
                  value={searchInput}
                  onChange={(e) => setSearchInput(e.target.value)}
                  placeholder="Search by name..."
                  className="focus:border-terminal-dark focus:ring-terminal-dark w-full rounded border border-slate-700 bg-slate-900 px-3 py-1.5 font-mono text-sm text-white placeholder-slate-600 transition-colors focus:outline-none focus:ring-1 sm:w-64"
                />
                {search && (
                  <button
                    type="button"
                    onClick={clearSearch}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300"
                  >
                    ×
                  </button>
                )}
              </div>
              <button
                type="submit"
                className="text-terminal-green rounded border border-slate-700 bg-slate-900/40 px-4 py-1.5 font-mono text-sm transition-colors hover:bg-slate-900/70"
              >
                Search
              </button>
            </form>
          }
        />

        <TerminalPanel>
          <Tabs value={activeTab} onValueChange={handleTabChange}>
            <TerminalPanelHeader
              indicator="active"
              actions={
                <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto">
                  <select
                    value={standard ?? ''}
                    onChange={(event) => handleStandardChange(event.target.value)}
                    aria-label="Filter by standard"
                    className="focus:border-terminal-dark focus:ring-terminal-dark min-w-[10.5rem] rounded border border-slate-700 bg-slate-900 px-3 py-1.5 font-mono text-sm text-white transition-colors focus:outline-none focus:ring-1"
                  >
                    <option value="">All standards</option>
                    {standardOptions.map((item) => (
                      <option key={item} value={item}>
                        {formatStandardLabel(item)}
                      </option>
                    ))}
                  </select>
                  {activeTab === 'nft' && (
                    <select
                      value={storageTier}
                      onChange={(event) => handleStorageTierChange(event.target.value)}
                      aria-label="Filter by storage tier"
                      className="focus:border-terminal-dark focus:ring-terminal-dark min-w-[12rem] rounded border border-slate-700 bg-slate-900 px-3 py-1.5 font-mono text-sm text-white transition-colors focus:outline-none focus:ring-1"
                    >
                      {STORAGE_TIER_OPTIONS.map((item) => (
                        <option key={item} value={item}>
                          {formatStorageTierLabel(item)}
                        </option>
                      ))}
                    </select>
                  )}
                  <TabsList className="ml-auto">
                    <TabsTrigger value="token">Tokens</TabsTrigger>
                    <TabsTrigger value="nft">NFTs</TabsTrigger>
                  </TabsList>
                </div>
              }
            >
              Asset List
            </TerminalPanelHeader>

            <TerminalPanelContent padding="none">
              <TabsContent value="token">
                <AssetTable
                  assetType="token"
                  search={search}
                  standard={standard}
                  storageTier="all"
                />
              </TabsContent>

              <TabsContent value="nft">
                <AssetTable
                  assetType="nft"
                  search={search}
                  standard={standard}
                  storageTier={storageTier}
                />
              </TabsContent>
            </TerminalPanelContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}

export function AssetsPageFallback() {
  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6 h-10 w-48 animate-pulse rounded bg-slate-800" />
        <div className="h-80 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
      </main>
    </div>
  );
}
