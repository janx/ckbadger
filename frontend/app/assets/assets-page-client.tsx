'use client';
import { useEffect, useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Image from '@/components/ui/image';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { AppLink } from '@/components/ui/app-link';
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
import {
  getClusterDetailHref,
  getIdentityCollectionHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatCkbCompact } from '@/lib/utils';
type AssetTab = 'token' | 'object' | 'identity';
type SortDirection = 'asc' | 'desc';
type StorageTierFilter = 'all' | 'fully_onchain' | 'offchain_dependent' | 'unknown';
type AssetSortKey =
  | 'name'
  | 'type'
  | 'supply'
  | 'transfers24h'
  | 'holders'
  | 'transfers'
  | 'used'
  | 'capacity'
  | 'onchainRatio'
  | 'hMultiplier';
const TOKEN_STANDARD_OPTIONS = ['xudt', 'sudt'];
const OBJECT_STANDARD_OPTIONS = ['spore', 'm-nft'];
const IDENTITY_STANDARD_OPTIONS = ['dotbit', 'did:ckb'];
const STORAGE_TIER_OPTIONS: StorageTierFilter[] = [
  'all',
  'fully_onchain',
  'offchain_dependent',
  'unknown',
];
function normalizeAssetTab(value: string | null): AssetTab {
  if (value === 'dob' || value === 'nft') {
    // Backward compatibility: old assets links used ?type=dob or ?type=nft.
    return 'object';
  }
  if (value === 'object' || value === 'identity' || value === 'token') {
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
function formatTokenSupply(totalSupply: string | null, decimals: number | null): string | null {
  if (!totalSupply) return null;
  if (decimals == null || decimals === 0) {
    return new Intl.NumberFormat().format(BigInt(totalSupply));
  }
  const num = BigInt(totalSupply);
  const divisor = BigInt(10 ** decimals);
  const integer = (num / divisor).toString();
  const remainder = num % divisor;
  const formatted = new Intl.NumberFormat().format(BigInt(integer));
  if (remainder === BigInt(0)) return formatted;
  const decimal = remainder.toString().padStart(decimals, '0').replace(/0+$/, '');
  return `${formatted}.${decimal}`;
}
function getStandardOptions(assetType: AssetTab, selectedStandard?: string) {
  const options =
    assetType === 'token'
      ? TOKEN_STANDARD_OPTIONS
      : assetType === 'object'
        ? OBJECT_STANDARD_OPTIONS
        : IDENTITY_STANDARD_OPTIONS;
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
        storageTier: assetType === 'object' && storageTier !== 'all' ? storageTier : undefined,
      }),
    placeholderData: keepPreviousData,
  });
  const assets = data?.data ?? [];
  const nameColumnClass = 'min-w-0 flex-[2_0_10rem] pr-4';
  const typeColumnClass = 'w-20 shrink-0';
  const smallNumberColumnClass = 'w-20 shrink-0 whitespace-nowrap text-right';
  const mediumNumberColumnClass = 'w-28 shrink-0 whitespace-nowrap text-right';
  const capacityColumnClass = 'w-28 shrink-0 whitespace-nowrap text-right';
  const formatNumber = (num: number | string) => {
    return new Intl.NumberFormat().format(Number(num));
  };
  const shortHash = (value: string) => {
    if (value.length <= 20) return value;
    return `${value.slice(0, 10)}...${value.slice(-8)}`;
  };
  const getAssetLink = (asset: Asset) => {
    if (asset.assetType === 'token') return getTokenDetailHref(asset.id);
    if (asset.standard === 'spore') {
      return getClusterDetailHref(asset.clusterId || asset.id);
    }
    if (asset.assetType === 'identity') return getIdentityCollectionHref(asset.standard, asset.id);
    return getObjectDetailHref(asset.id);
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
      <span className={sortKey === key ? 'text-emphasis' : 'text-text-dim'}>
        {sortKey === key ? (sortDirection === 'asc' ? '↑' : '↓') : '↕'}
      </span>
    </button>
  );
  if (isLoading) {
    return (
      <div className="space-y-2 py-4">
        {/* Table skeleton (lg+) */}
        {Array.from({ length: 5 }).map((_, i) => (
          <TerminalRow key={i} hoverable={false}>
            <div className="hidden animate-pulse items-center lg:flex">
              <div className={nameColumnClass}>
                <div className="bg-base-elevated h-4 w-48 rounded" />
              </div>
              <div className={typeColumnClass}>
                <div className="bg-base-elevated h-4 w-12 rounded" />
              </div>
              {assetType !== 'token' && (
                <div className={smallNumberColumnClass}>
                  <div className="bg-base-elevated ml-auto h-4 w-10 rounded" />
                </div>
              )}
              <div className={smallNumberColumnClass}>
                <div className="bg-base-elevated ml-auto h-4 w-12 rounded" />
              </div>
              <div className={mediumNumberColumnClass}>
                <div className="bg-base-elevated ml-auto h-4 w-16 rounded" />
              </div>
              {assetType === 'token' && (
                <div className={`${capacityColumnClass} hidden xl:block`}>
                  <div className="bg-base-elevated ml-auto h-4 w-16 rounded" />
                </div>
              )}
              <div className={`${capacityColumnClass} hidden xl:block`}>
                <div className="bg-base-elevated ml-auto h-4 w-12 rounded" />
              </div>
              <div className={`${capacityColumnClass} hidden xl:block`}>
                <div className="bg-base-elevated ml-auto h-4 w-16 rounded" />
              </div>
              <div className={capacityColumnClass}>
                <div className="bg-base-elevated ml-auto h-4 w-16 rounded" />
              </div>
            </div>
            {/* Card skeleton (<lg) */}
            <div className="animate-pulse space-y-2 lg:hidden">
              <div className="flex items-center gap-2">
                <div className="bg-base-elevated h-6 w-6 shrink-0 rounded-full" />
                <div className="bg-base-elevated h-4 w-40 rounded" />
                <div className="bg-base-elevated ml-auto h-4 w-12 rounded" />
              </div>
              <div className="flex gap-4">
                <div className="bg-base-elevated h-3 w-16 rounded" />
                <div className="bg-base-elevated h-3 w-20 rounded" />
                <div className="bg-base-elevated h-3 w-16 rounded" />
              </div>
            </div>
          </TerminalRow>
        ))}
      </div>
    );
  }
  if (!data?.data?.length) {
    return <div className="text-text-dim py-8 text-center">No assets found</div>;
  }
  return (
    <>
      <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-3 py-2 font-mono text-xs uppercase tracking-wider lg:flex">
        {renderSortHeader('name', assetType === 'token' ? 'Token' : 'Collection', nameColumnClass)}
        {renderSortHeader('type', 'Standard', typeColumnClass)}
        {assetType !== 'token' &&
          renderSortHeader('supply', 'Items', smallNumberColumnClass, 'right')}
        {renderSortHeader('transfers24h', '24h Txns', smallNumberColumnClass, 'right')}
        {renderSortHeader('holders', 'Holders', mediumNumberColumnClass, 'right')}
        <div className="hidden xl:contents">
          {assetType === 'token' &&
            renderSortHeader('supply', 'Circulation', capacityColumnClass, 'right')}
          {renderSortHeader('hMultiplier', 'HM', capacityColumnClass, 'right')}
          {renderSortHeader('used', 'Used', capacityColumnClass, 'right')}
        </div>
        {renderSortHeader('capacity', 'Capacity', capacityColumnClass, 'right')}
      </div>
      {assets.map((asset: Asset) => (
        <TerminalRow key={asset.id}>
          {/* Table row (lg+) */}
          <div className="hidden items-center lg:flex">
            <div className={nameColumnClass}>
              <AppLink href={getAssetLink(asset)} className="block">
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
                    {asset.assetType === 'object' && asset.standard === 'spore' && (
                      <span className="text-sm leading-none">🗂️</span>
                    )}
                  </span>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span
                        className="text-emphasis max-w-full truncate font-medium hover:underline"
                        title={getAssetName(asset)}
                      >
                        {getAssetName(asset)}
                      </span>
                      {asset.assetType === 'object' && getStorageBadgeLabel(asset) && (
                        <span className="border-base-border text-text rounded border px-1.5 py-0.5 font-mono text-[10px]">
                          {getStorageBadgeLabel(asset)}
                        </span>
                      )}
                      {asset.published && (
                        <span className="text-emphasis" title="Verified">
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
                        <span className="text-warning" title="Famous">
                          <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 20 20">
                            <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                          </svg>
                        </span>
                      )}
                    </div>
                    <HexDisplay value={asset.id} size="sm" startChars={8} endChars={6} />
                  </div>
                </div>
              </AppLink>
            </div>
            <div className={typeColumnClass}>
              <div className="flex flex-wrap gap-1">
                <Badge variant="neutral">{getTypeBadgeLabel(asset)}</Badge>
              </div>
            </div>
            {assetType !== 'token' && (
              <div className={`${smallNumberColumnClass} text-text-bright font-mono tabular-nums`}>
                {formatNumber(asset.totalSupply || 0)}
              </div>
            )}
            <div className={`${smallNumberColumnClass} text-warning font-mono tabular-nums`}>
              {formatNumber(asset.transfers24h)}
            </div>
            <div className={`${mediumNumberColumnClass} text-text-dim font-mono tabular-nums`}>
              {formatNumber(asset.holdersCount)}
            </div>
            {assetType === 'token' && (
              <div
                className={`${capacityColumnClass} text-text-bright hidden font-mono tabular-nums xl:block`}
              >
                {(() => {
                  const formatted = formatTokenSupply(asset.totalSupply, asset.decimals);
                  return formatted ? (
                    <span className="block truncate" title={`Total Circulation: ${formatted}`}>
                      {formatted}
                    </span>
                  ) : (
                    <span className="text-text-dim">-</span>
                  );
                })()}
              </div>
            )}
            <div
              className={`${capacityColumnClass} text-text hidden font-mono tabular-nums xl:block`}
            >
              {asset.hMultiplier != null ? (
                <span title={`H-Multiplier: capacity / used = ×${asset.hMultiplier.toFixed(2)}`}>
                  ×{asset.hMultiplier.toFixed(2)}
                </span>
              ) : (
                <span className="text-text-dim">-</span>
              )}
            </div>
            <div
              className={`${capacityColumnClass} text-text hidden font-mono tabular-nums xl:block`}
            >
              {(() => {
                const occupied = asset.liveUsedCapacity;
                if (!occupied) {
                  return <span className="text-text-dim">-</span>;
                }
                const compact = formatCkbCompact(occupied);
                return <span title={`${compact.full} CKB`}>{compact.value}</span>;
              })()}
            </div>
            <div className={`${capacityColumnClass} text-text font-mono tabular-nums`}>
              {(() => {
                const capacity = asset.liveCapacity;
                if (!capacity) {
                  return <span className="text-text-dim">-</span>;
                }
                const compact = formatCkbCompact(capacity);
                return <span title={`${compact.full} CKB`}>{compact.value}</span>;
              })()}
            </div>
          </div>
          {/* Card layout (<lg) */}
          <div className="space-y-1.5 lg:hidden">
            <div className="flex items-center gap-2">
              <AppLink href={getAssetLink(asset)} className="min-w-0 flex-1">
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
                    {asset.assetType === 'object' && asset.standard === 'spore' && (
                      <span className="text-sm leading-none">🗂️</span>
                    )}
                  </span>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span
                        className="text-emphasis max-w-full truncate font-medium hover:underline"
                        title={getAssetName(asset)}
                      >
                        {getAssetName(asset)}
                      </span>
                      {asset.published && (
                        <span className="text-emphasis" title="Verified">
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
                        <span className="text-warning" title="Famous">
                          <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 20 20">
                            <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                          </svg>
                        </span>
                      )}
                    </div>
                    <HexDisplay value={asset.id} size="sm" startChars={8} endChars={6} />
                  </div>
                </div>
              </AppLink>
              <Badge variant="neutral">{getTypeBadgeLabel(asset)}</Badge>
            </div>
            {assetType === 'object' && getStorageBadgeLabel(asset) && (
              <span className="border-base-border text-text rounded border px-1.5 py-0.5 font-mono text-[10px]">
                {getStorageBadgeLabel(asset)}
              </span>
            )}
            <div className="text-text-dim flex flex-wrap items-center gap-x-4 gap-y-1 font-mono text-xs tabular-nums">
              <span>
                24h: <span className="text-warning">{formatNumber(asset.transfers24h)}</span>
              </span>
              <span>Holders: {formatNumber(asset.holdersCount)}</span>
              {assetType !== 'token' && <span>Items: {formatNumber(asset.totalSupply || 0)}</span>}
              <span>
                Cap:{' '}
                {(() => {
                  const c = asset.liveCapacity;
                  return c ? formatCkbCompact(c).value : '-';
                })()}
              </span>
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
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Assets"
          subtitle="Browse tokens, objects, and identities on the CKB network"
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
                  className="focus:border-emphasis-dim focus:ring-emphasis-dim border-base-border bg-base-surface placeholder-text-dim text-text-bright w-full rounded border px-3 py-1.5 font-mono text-sm transition-colors focus:outline-none focus:ring-1 sm:w-64"
                />
                {search && (
                  <button
                    type="button"
                    onClick={clearSearch}
                    className="text-text-dim hover:text-text absolute right-2 top-1/2 -translate-y-1/2"
                  >
                    ×
                  </button>
                )}
              </div>
              <button
                type="submit"
                className="text-emphasis border-base-border bg-base-surface/40 hover:bg-base-surface/70 rounded border px-4 py-1.5 font-mono text-sm transition-colors"
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
                    className="focus:border-emphasis-dim focus:ring-emphasis-dim border-base-border bg-base-surface text-text-bright min-w-[10.5rem] rounded border px-3 py-1.5 font-mono text-sm transition-colors focus:outline-none focus:ring-1"
                  >
                    <option value="">All standards</option>
                    {standardOptions.map((item) => (
                      <option key={item} value={item}>
                        {formatStandardLabel(item)}
                      </option>
                    ))}
                  </select>
                  {activeTab === 'object' && (
                    <select
                      value={storageTier}
                      onChange={(event) => handleStorageTierChange(event.target.value)}
                      aria-label="Filter by storage tier"
                      className="focus:border-emphasis-dim focus:ring-emphasis-dim border-base-border bg-base-surface text-text-bright min-w-[12rem] rounded border px-3 py-1.5 font-mono text-sm transition-colors focus:outline-none focus:ring-1"
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
                    <TabsTrigger value="object">Objects</TabsTrigger>
                    <TabsTrigger value="identity">Identities</TabsTrigger>
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
              <TabsContent value="object">
                <AssetTable
                  assetType="object"
                  search={search}
                  standard={standard}
                  storageTier={storageTier}
                />
              </TabsContent>
              <TabsContent value="identity">
                <AssetTable
                  assetType="identity"
                  search={search}
                  standard={standard}
                  storageTier="all"
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
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="bg-base-elevated mb-6 h-10 w-48 animate-pulse rounded" />
        <div className="border-base-border bg-base-surface/50 h-80 animate-pulse rounded border" />
      </main>
    </div>
  );
}
