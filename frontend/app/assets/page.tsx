'use client';

import { useEffect, useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from 'next/link';
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

type AssetTab = 'token' | 'nft' | 'dob';
type SortDirection = 'asc' | 'desc';
type AssetSortKey =
  | 'name'
  | 'type'
  | 'supply'
  | 'transfers24h'
  | 'holders'
  | 'transfers'
  | 'occupied'
  | 'capacity';

function normalizeAssetTab(value: string | null): AssetTab {
  if (value === 'nft' || value === 'dob' || value === 'token') {
    return value;
  }
  return 'token';
}

function AssetTable({ assetType, search }: { assetType: AssetTab; search: string | undefined }) {
  const pagination = useCursorPagination();
  const { reset } = pagination;
  const [sortKey, setSortKey] = useState<AssetSortKey>('capacity');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');

  useEffect(() => {
    setSortKey('capacity');
    setSortDirection('desc');
    reset();
  }, [assetType, reset]);

  const { data, isLoading } = useQuery({
    queryKey: ['assets', assetType, pagination.cursor, search, sortKey, sortDirection],
    queryFn: () =>
      api.getAssets({
        limit: 20,
        type: assetType,
        cursor: pagination.cursor,
        search,
        sortKey,
        sortDirection,
      }),
    placeholderData: keepPreviousData,
  });

  const assets = data?.data ?? [];

  const formatNumber = (num: number | string) => {
    return new Intl.NumberFormat().format(Number(num));
  };

  const shortHash = (value: string) => {
    if (value.length <= 20) return value;
    return `${value.slice(0, 10)}...${value.slice(-8)}`;
  };

  const getAssetLink = (asset: Asset) => {
    if (asset.assetType === 'token') return `/tokens/${asset.id}`;
    if (asset.assetType === 'dob') return `/clusters/${asset.id}`;
    return `/nfts/${toNftDetailSlug(asset.id, asset.standard)}`;
  };

  const getAssetName = (asset: Asset) => {
    if (asset.assetType === 'token') {
      return asset.symbol || asset.name || shortHash(asset.id);
    }
    return asset.name || 'Unnamed Collection';
  };

  const getTypeBadgeVariant = (asset: Asset): 'green' | 'purple' | 'blue' | 'amber' => {
    if (asset.assetType === 'dob') return 'green';
    if (asset.assetType === 'nft') return 'purple';
    if (asset.standard === 'xudt') return 'purple';
    return 'blue';
  };

  const getTypeBadgeLabel = (asset: Asset) => {
    if (asset.assetType === 'dob') return asset.standard.toUpperCase();
    if (asset.assetType === 'nft') return asset.standard.toUpperCase();
    return asset.standard.toUpperCase();
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
      className={`${className} flex items-center gap-1 ${align === 'right' ? 'justify-end text-right' : ''}`}
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
        {Array.from({ length: 5 }).map((_, i) => (
          <TerminalRow key={i} hoverable={false}>
            <div className="flex animate-pulse items-center">
              <div className="flex-1">
                <div className="h-4 w-48 rounded bg-slate-800" />
              </div>
              <div className="w-20 shrink-0">
                <div className="h-4 w-12 rounded bg-slate-800" />
              </div>
              <div className="w-24 shrink-0 text-right">
                <div className="ml-auto h-4 w-10 rounded bg-slate-800" />
              </div>
              <div className="w-28 shrink-0 text-right">
                <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
              </div>
              <div className="w-28 shrink-0 text-right">
                <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
              </div>
              <div className="w-32 shrink-0 text-right">
                <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
              </div>
              <div className="w-32 shrink-0 text-right">
                <div className="ml-auto h-4 w-16 rounded bg-slate-800" />
              </div>
            </div>
          </TerminalRow>
        ))}
      </div>
    );
  }

  if (!data?.data?.length) {
    return <div className="py-8 text-center text-slate-500">No assets found</div>;
  }

  return (
    <>
      <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
        {renderSortHeader(
          'name',
          assetType === 'dob' ? 'Collection' : assetType === 'token' ? 'Token' : 'NFT',
          'flex-1'
        )}
        {renderSortHeader('type', 'Type', 'w-20 shrink-0')}
        {assetType === 'dob' && renderSortHeader('supply', 'DOBs', 'w-24 shrink-0', 'right')}
        {renderSortHeader('transfers24h', '24h Txns', 'w-24 shrink-0', 'right')}
        {renderSortHeader('holders', 'Holders', 'w-28 shrink-0', 'right')}
        {renderSortHeader('transfers', 'Transfers', 'w-28 shrink-0', 'right')}
        {renderSortHeader('occupied', 'Occupied', 'w-32 shrink-0', 'right')}
        {renderSortHeader('capacity', 'Capacity', 'w-32 shrink-0', 'right')}
      </div>
      {assets.map((asset: Asset) => (
        <TerminalRow key={asset.id}>
          <div className="flex items-center">
            <div className="flex-1">
              <Link href={getAssetLink(asset)} className="block">
                <div className="flex items-center gap-2">
                  {asset.assetType === 'token' && asset.iconUrl && (
                    <img
                      src={asset.iconUrl}
                      alt=""
                      className="h-6 w-6 rounded-full"
                      onError={(e) => {
                        (e.target as HTMLImageElement).style.display = 'none';
                      }}
                    />
                  )}
                  {asset.assetType === 'dob' && (
                    <span className="flex h-6 w-6 items-center justify-center text-sm">🗂️</span>
                  )}
                  <div>
                    <div className="flex items-center gap-1.5">
                      <span className="text-terminal-green font-medium hover:underline">
                        {getAssetName(asset)}
                      </span>
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
                      color="white"
                      size="sm"
                      startChars={8}
                      endChars={6}
                    />
                  </div>
                </div>
              </Link>
            </div>
            <div className="w-20 shrink-0">
              <div className="flex flex-wrap gap-1">
                <Badge variant={getTypeBadgeVariant(asset)}>{getTypeBadgeLabel(asset)}</Badge>
              </div>
            </div>
            {assetType === 'dob' && (
              <div className="w-24 shrink-0 text-right font-mono text-white">
                {formatNumber(asset.totalSupply || 0)}
              </div>
            )}
            <div className="text-amber w-24 shrink-0 text-right font-mono">
              {formatNumber(asset.transfers24h)}
            </div>
            <div className="w-28 shrink-0 text-right font-mono text-slate-400">
              {formatNumber(asset.holdersCount)}
            </div>
            <div className="w-28 shrink-0 text-right font-mono text-slate-400">
              {formatNumber(asset.transfersCount)}
            </div>
            <div className="w-32 shrink-0 text-right font-mono text-slate-300">
              {(() => {
                const occupied = asset.liveOccupiedCapacity;
                if (!occupied) {
                  return <span className="text-slate-600">-</span>;
                }
                const compact = formatCkbCompact(occupied);
                return <span title={`${compact.full} CKB`}>{compact.value}</span>;
              })()}
            </div>
            <div className="w-32 shrink-0 text-right font-mono text-slate-300">
              {(() => {
                const capacity = asset.liveCapacity;
                if (!capacity) {
                  return <span className="text-slate-600">-</span>;
                }
                const compact = formatCkbCompact(capacity);
                return <span title={`${compact.full} CKB`}>{compact.value}</span>;
              })()}
            </div>
          </div>
        </TerminalRow>
      ))}
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

export default function AssetsPage() {
  const [activeTab, setActiveTab] = useState<AssetTab>(() => {
    if (typeof window === 'undefined') return 'token';
    const params = new URLSearchParams(window.location.search);
    return normalizeAssetTab(params.get('type'));
  });
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>(undefined);

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

    if (typeof window !== 'undefined') {
      const params = new URLSearchParams(window.location.search);
      params.set('type', nextTab);
      const query = params.toString();
      const nextUrl = query ? `${window.location.pathname}?${query}` : window.location.pathname;
      window.history.replaceState(null, '', nextUrl);
    }
  };

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Assets"
          subtitle="Browse tokens, NFTs, and DOBs on the CKB network"
          actions={
            <form onSubmit={handleSearch} className="flex gap-2">
              <div className="relative">
                <input
                  type="text"
                  value={searchInput}
                  onChange={(e) => setSearchInput(e.target.value)}
                  placeholder="Search by name..."
                  className="focus:border-terminal-dark focus:ring-terminal-dark w-64 rounded border border-slate-700 bg-slate-900 px-3 py-1.5 font-mono text-sm text-white placeholder-slate-600 transition-colors focus:outline-none focus:ring-1"
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
                className="border-terminal-dark bg-terminal-dark/20 text-terminal-green hover:bg-terminal-dark/40 rounded border px-4 py-1.5 font-mono text-sm transition-colors"
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
                <TabsList>
                  <TabsTrigger value="token">Tokens</TabsTrigger>
                  <TabsTrigger value="nft">NFTs</TabsTrigger>
                  <TabsTrigger value="dob">
                    <span className="flex items-center gap-1.5">
                      DOBs
                      <Badge variant="green">ON-CHAIN</Badge>
                    </span>
                  </TabsTrigger>
                </TabsList>
              }
            >
              Asset List
            </TerminalPanelHeader>

            <TerminalPanelContent padding="none">
              <TabsContent value="token">
                <AssetTable assetType="token" search={search} />
              </TabsContent>

              <TabsContent value="nft">
                <div className="border-b border-slate-800 bg-purple-950/20 p-4">
                  <div className="flex items-start gap-3">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-purple-900/50 text-xl">
                      🖼️
                    </div>
                    <div>
                      <h3 className="font-semibold text-purple-400">NFT Collections</h3>
                      <p className="mt-1 text-sm text-slate-400">
                        NFTs (Non-Fungible Tokens) are unique digital assets. Unlike DOBs, NFTs
                        often rely on external services for metadata storage.
                      </p>
                    </div>
                  </div>
                </div>
                <AssetTable assetType="nft" search={search} />
              </TabsContent>

              <TabsContent value="dob">
                <div className="bg-terminal-dark/10 border-b border-slate-800 p-4">
                  <div className="flex items-start gap-3">
                    <div className="bg-terminal-dark/30 flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-xl">
                      🗂️
                    </div>
                    <div>
                      <h3 className="text-terminal-green font-semibold">DOB Collections</h3>
                      <p className="mt-1 text-sm text-slate-400">
                        DOBs (Digital Objects) are fully on-chain assets with all data stored
                        directly in CKB cells. They require no external services and can exist
                        permanently.
                      </p>
                    </div>
                  </div>
                </div>
                <AssetTable assetType="dob" search={search} />
              </TabsContent>
            </TerminalPanelContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}
