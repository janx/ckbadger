'use client';

import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueries, keepPreviousData } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams, usePathname, useRouter, useSearchParams } from 'next/navigation';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CapacityOccupationSection } from '@/components/ui/capacity-occupation-section';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { getOccupationRangeParams, OccupationRangeKey } from '@/lib/occupation-range';
import { ClusterDescription } from '@/components/spore/cluster-description';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { formatCkbCompact } from '@/lib/utils';

type ListContentFilter = 'all' | 'image' | 'video' | 'audio' | 'text' | 'other';
type ListSort = 'createdDesc' | 'createdAsc' | 'sizeDesc' | 'sizeAsc';
type CollectionSectionTab = 'activities' | 'nfts' | 'holders';

const LIST_FILTER_VALUES: ListContentFilter[] = ['all', 'image', 'video', 'audio', 'text', 'other'];
const LIST_SORT_VALUES: ListSort[] = ['createdDesc', 'createdAsc', 'sizeDesc', 'sizeAsc'];

function isListContentFilter(value: string | null): value is ListContentFilter {
  return !!value && LIST_FILTER_VALUES.includes(value as ListContentFilter);
}

function isListSort(value: string | null): value is ListSort {
  return !!value && LIST_SORT_VALUES.includes(value as ListSort);
}

function isCollectionSectionTab(value: string | null): value is CollectionSectionTab {
  return value === 'activities' || value === 'nfts' || value === 'holders';
}

function safeString(value: unknown, fallback = ''): string {
  if (typeof value !== 'string') {
    return fallback;
  }
  return value;
}

function safeNumber(value: unknown, fallback = 0): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return fallback;
  }
  return value;
}

function formatStorageTierLabel(tier: string): string {
  if (tier === 'fully_onchain') return 'Fully On-chain';
  if (tier === 'decentralized_external') return 'Decentralized External';
  if (tier === 'centralized_dependent') return 'Centralized Dependency';
  return 'Unknown';
}

function getSortIndicator(direction: 'asc' | 'desc' | null): string {
  if (direction === 'asc') return '↑';
  if (direction === 'desc') return '↓';
  return '↕';
}

export default function ClusterDetailPage() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const params = useParams();
  const clusterId = params.clusterId as string;
  const tabFromQuery = searchParams.get('tab');
  const [occupationRange, setOccupationRange] = useState<OccupationRangeKey>('all');
  const [activeCollectionTab, setActiveCollectionTab] = useState<CollectionSectionTab>(() =>
    isCollectionSectionTab(tabFromQuery) ? tabFromQuery : 'nfts'
  );
  const [listContentFilter, setListContentFilter] = useState<ListContentFilter>(() => {
    const value = searchParams.get('content');
    return isListContentFilter(value) ? value : 'all';
  });
  const [listSort, setListSort] = useState<ListSort>(() => {
    const value = searchParams.get('sort');
    return isListSort(value) ? value : 'createdDesc';
  });
  const [listQuery, setListQuery] = useState(() => searchParams.get('q') ?? '');
  const occupationRangeParams = getOccupationRangeParams(occupationRange);

  const sporesPagination = useCursorPagination();
  const clusterHoldersPagination = useCursorPagination();
  const clusterActivitiesPagination = useCursorPagination();
  const { reset: resetClusterHoldersPagination } = clusterHoldersPagination;
  const { reset: resetClusterActivitiesPagination } = clusterActivitiesPagination;

  const {
    data: cluster,
    isLoading: clusterLoading,
    error: clusterError,
  } = useQuery({
    queryKey: ['cluster', clusterId],
    queryFn: () => api.getSporeCluster(clusterId),
  });

  const { data: sporesData, isLoading: sporesLoading } = useQuery({
    queryKey: ['cluster-spores', clusterId, sporesPagination.cursor],
    queryFn: () =>
      api.getSporesByCluster(clusterId, { limit: 20, cursor: sporesPagination.cursor }),
    enabled: !!clusterId,
    placeholderData: keepPreviousData,
  });

  const {
    data: clusterHolders,
    isLoading: isClusterHoldersLoading,
    isFetching: isClusterHoldersFetching,
    isError: isClusterHoldersError,
  } = useQuery({
    queryKey: ['cluster-holders', clusterId, clusterHoldersPagination.cursor],
    queryFn: () =>
      api.getSporeClusterHolders(clusterId, {
        limit: 20,
        cursor: clusterHoldersPagination.cursor,
      }),
    enabled: !!clusterId && activeCollectionTab === 'holders',
    placeholderData: keepPreviousData,
  });

  const {
    data: clusterActivities,
    isLoading: isClusterActivitiesLoading,
    isFetching: isClusterActivitiesFetching,
    isError: isClusterActivitiesError,
  } = useQuery({
    queryKey: ['cluster-activities', clusterId, clusterActivitiesPagination.cursor],
    queryFn: () =>
      api.getSporeClusterActivities(clusterId, {
        limit: 20,
        cursor: clusterActivitiesPagination.cursor,
      }),
    enabled: !!clusterId && activeCollectionTab === 'activities',
    placeholderData: keepPreviousData,
  });

  const { data: occupationChart, isLoading: isOccupationChartLoading } = useQuery({
    queryKey: ['cluster-occupation-chart', clusterId, occupationRange],
    queryFn: () =>
      occupationRangeParams
        ? api.getSporeClusterOccupationChart(clusterId, occupationRangeParams)
        : api.getSporeClusterOccupationChart(clusterId),
    enabled: !!clusterId,
  });

  const { data: creatorAddressRecord } = useQuery({
    queryKey: ['cluster-creator-address', cluster?.ownerLockHash],
    queryFn: () => api.getAddress(cluster!.ownerLockHash),
    enabled: !!cluster?.ownerLockHash && !cluster?.ownerAddress,
    retry: false,
  });

  const formatNumber = (num: number) => {
    return new Intl.NumberFormat().format(num);
  };

  const parseShannons = (value: string | null | undefined): bigint | null => {
    if (!value) {
      return null;
    }

    try {
      return BigInt(value);
    } catch {
      return null;
    }
  };

  const getContentTypeIcon = (contentType: string | null | undefined) => {
    const normalized = safeString(contentType);
    if (!normalized) return '📦';
    if (normalized.startsWith('image/')) return '🖼️';
    if (normalized.startsWith('video/')) return '🎬';
    if (normalized.startsWith('audio/')) return '🎵';
    if (normalized.startsWith('text/')) return '📄';
    return '📦';
  };

  const summarizeContentType = (contentType: string | null | undefined): string => {
    const normalized = safeString(contentType);
    if (!normalized) {
      return 'unknown';
    }
    const [primary] = normalized.toLowerCase().split('/');
    if (!primary) {
      return 'unknown';
    }
    return primary;
  };

  const isKnownPrimaryType = (primary: string): boolean => {
    return primary === 'image' || primary === 'video' || primary === 'audio' || primary === 'text';
  };

  const liveCapacity = parseShannons(cluster?.liveCapacity ?? null);
  const occupiedCapacity = parseShannons(cluster?.liveOccupiedCapacity ?? null);
  const occupationPercent =
    liveCapacity && occupiedCapacity && liveCapacity > BigInt(0)
      ? (Number((occupiedCapacity * BigInt(10000)) / liveCapacity) / 100).toFixed(2)
      : null;
  const compactLiveCapacity = liveCapacity ? `${formatCkbCompact(liveCapacity).value} CKB` : '--';
  const compactOccupiedCapacity = occupiedCapacity
    ? `${formatCkbCompact(occupiedCapacity).value} CKB`
    : '--';

  const normalizedQuery = listQuery.trim().toLowerCase();
  const sizeSortDirection =
    listSort === 'sizeAsc' ? 'asc' : listSort === 'sizeDesc' ? 'desc' : null;
  const blockSortDirection =
    listSort === 'createdAsc' ? 'asc' : listSort === 'createdDesc' ? 'desc' : null;

  useEffect(() => {
    const currentQuery = searchParams.toString();
    const nextParams = new URLSearchParams(currentQuery);

    if (listContentFilter === 'all') {
      nextParams.delete('content');
    } else {
      nextParams.set('content', listContentFilter);
    }

    if (listSort === 'createdDesc') {
      nextParams.delete('sort');
    } else {
      nextParams.set('sort', listSort);
    }

    if (!normalizedQuery) {
      nextParams.delete('q');
    } else {
      nextParams.set('q', normalizedQuery);
    }

    if (activeCollectionTab === 'nfts') {
      nextParams.delete('tab');
    } else {
      nextParams.set('tab', activeCollectionTab);
    }

    const nextQuery = nextParams.toString();
    if (nextQuery === currentQuery) {
      return;
    }
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  }, [
    activeCollectionTab,
    listContentFilter,
    listSort,
    normalizedQuery,
    pathname,
    router,
    searchParams,
  ]);

  useEffect(() => {
    resetClusterHoldersPagination();
    resetClusterActivitiesPagination();
  }, [clusterId, resetClusterActivitiesPagination, resetClusterHoldersPagination]);

  const filteredAndSortedSpores = useMemo(() => {
    if (!sporesData?.data?.length) {
      return [];
    }

    const filtered = sporesData.data.filter((spore) => {
      if (listContentFilter === 'all') {
        return true;
      }
      const primary = summarizeContentType(spore.contentType);
      if (listContentFilter === 'other') {
        return !isKnownPrimaryType(primary);
      }
      if (primary !== listContentFilter) {
        return false;
      }
      return true;
    });

    const queryFiltered = normalizedQuery
      ? filtered.filter((spore) => {
          const sporeId = safeString(spore.sporeId);
          const contentType = safeString(spore.contentType);
          const ownerLockHash = safeString(spore.ownerLockHash);
          const ownerAddress = safeString(spore.ownerAddress);
          return (
            sporeId.toLowerCase().includes(normalizedQuery) ||
            contentType.toLowerCase().includes(normalizedQuery) ||
            ownerLockHash.toLowerCase().includes(normalizedQuery) ||
            ownerAddress.toLowerCase().includes(normalizedQuery)
          );
        })
      : filtered;

    const sorted = [...queryFiltered];
    sorted.sort((a, b) => {
      const createdAtA = safeNumber(a.createdAtBlock);
      const createdAtB = safeNumber(b.createdAtBlock);
      const contentSizeA = safeNumber(a.contentSize);
      const contentSizeB = safeNumber(b.contentSize);
      if (listSort === 'createdDesc') {
        return createdAtB - createdAtA;
      }
      if (listSort === 'createdAsc') {
        return createdAtA - createdAtB;
      }
      if (listSort === 'sizeDesc') {
        return contentSizeB - contentSizeA;
      }
      if (listSort === 'sizeAsc') {
        return contentSizeA - contentSizeB;
      }
      return 0;
    });
    return sorted;
  }, [sporesData?.data, listContentFilter, listSort, normalizedQuery]);

  const missingSporeOwnerLockHashes = useMemo(() => {
    if (!filteredAndSortedSpores.length) {
      return [];
    }

    const unique = new Map<string, string>();
    for (const spore of filteredAndSortedSpores) {
      const ownerAddress = safeString(spore.ownerAddress);
      const ownerLockHash = safeString(spore.ownerLockHash);
      if (ownerAddress || !ownerLockHash) {
        continue;
      }
      const normalized = ownerLockHash.toLowerCase();
      if (!unique.has(normalized)) {
        unique.set(normalized, ownerLockHash);
      }
    }
    return Array.from(unique.values());
  }, [filteredAndSortedSpores]);

  const sporeOwnerAddressQueries = useQueries({
    queries: missingSporeOwnerLockHashes.map((ownerLockHash) => ({
      queryKey: ['spore-owner-address', ownerLockHash],
      queryFn: () => api.getAddress(ownerLockHash),
      retry: false,
    })),
  });

  const sporeOwnerAddressByLockHash = useMemo(() => {
    const map = new Map<string, string>();
    missingSporeOwnerLockHashes.forEach((ownerLockHash, index) => {
      const address = sporeOwnerAddressQueries[index]?.data?.address;
      if (address) {
        map.set(ownerLockHash.toLowerCase(), address);
      }
    });
    return map;
  }, [missingSporeOwnerLockHashes, sporeOwnerAddressQueries]);

  const resolveSporeOwnerAddress = (
    ownerLockHash: string | null | undefined,
    ownerAddress?: string | null
  ) => {
    const normalizedOwnerAddress = safeString(ownerAddress);
    if (normalizedOwnerAddress) {
      return normalizedOwnerAddress;
    }
    const normalizedOwnerLockHash = safeString(ownerLockHash);
    if (!normalizedOwnerLockHash) {
      return null;
    }
    return sporeOwnerAddressByLockHash.get(normalizedOwnerLockHash.toLowerCase()) ?? null;
  };

  const pageContentBreakdown = useMemo(() => {
    if (!filteredAndSortedSpores.length) {
      return [];
    }

    const map = new Map<string, number>();
    for (const spore of filteredAndSortedSpores) {
      const key = summarizeContentType(spore.contentType);
      map.set(key, (map.get(key) ?? 0) + 1);
    }

    return Array.from(map.entries())
      .map(([type, count]) => ({
        type,
        count,
        percent: ((count / filteredAndSortedSpores.length) * 100).toFixed(1),
      }))
      .sort((a, b) => b.count - a.count);
  }, [filteredAndSortedSpores]);

  const avgPayloadBytes = useMemo(() => {
    if (!filteredAndSortedSpores.length) {
      return null;
    }
    const sum = filteredAndSortedSpores.reduce(
      (acc, item) => acc + safeNumber(item.contentSize),
      0
    );
    return Math.round(sum / filteredAndSortedSpores.length);
  }, [filteredAndSortedSpores]);

  const creatorAddress = cluster?.ownerAddress || creatorAddressRecord?.address || null;

  if (clusterLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="mb-6 h-10 w-64 animate-pulse rounded bg-slate-800" />
          <div className="grid gap-6 lg:grid-cols-3">
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
            <div className="h-96 animate-pulse rounded border border-slate-800 bg-slate-900/50 lg:col-span-2" />
          </div>
        </main>
      </div>
    );
  }

  if (clusterError || !cluster) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Spore Cluster not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/assets?type=nft"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← Back to NFTs
          </Link>
        </div>

        <PageHeader
          title={cluster.name || 'Unnamed Collection'}
          badge={
            <Badge
              variant={
                cluster.storageProfile?.tier === 'fully_onchain'
                  ? 'green'
                  : cluster.storageProfile?.tier === 'centralized_dependent'
                    ? 'red'
                    : 'neutral'
              }
            >
              Spore Cluster
            </Badge>
          }
          subtitle="On-chain cluster metadata, capacity footprint, and spore composition."
        />

        <div className="mb-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <TerminalPanel variant="inset">
            <TerminalPanelContent className="space-y-2">
              <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                Total Spores
              </div>
              <div className="text-amber text-2xl font-semibold tabular-nums">
                {formatNumber(cluster.sporesCount)}
              </div>
              <div className="font-mono text-xs text-slate-500">Full collection supply</div>
            </TerminalPanelContent>
          </TerminalPanel>

          {cluster.storageProfile && (
            <TerminalPanel variant="inset">
              <TerminalPanelContent className="space-y-2">
                <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                  Storage Integrity
                </div>
                <div className="text-terminal-green text-base font-semibold">
                  {formatStorageTierLabel(cluster.storageProfile.tier)}
                </div>
                <div className="font-mono text-xs text-slate-400">
                  On-chain ratio:{' '}
                  {(Number(cluster.storageProfile.fullyOnchainRatio) * 100).toFixed(2)}%
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
          )}

          <TerminalPanel variant="inset">
            <TerminalPanelContent className="space-y-2">
              <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                Live Capacity
              </div>
              <div className="font-mono text-lg text-white">{compactLiveCapacity}</div>
              <div className="font-mono text-xs text-slate-500">Total live CKB in this cluster</div>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel variant="inset">
            <TerminalPanelContent className="space-y-2">
              <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                Occupied Capacity
              </div>
              <div className="font-mono text-lg text-white">{compactOccupiedCapacity}</div>
              <div className="font-mono text-xs text-slate-500">
                Occupied Ratio: {occupationPercent ? `${occupationPercent}%` : '--'}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel variant="inset">
            <TerminalPanelContent className="space-y-2">
              <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                Created At
              </div>
              <Link
                href={`/blocks/${cluster.createdAtBlock}`}
                className="text-terminal-green font-mono text-lg hover:underline"
              >
                #{formatNumber(cluster.createdAtBlock)}
              </Link>
              <div className="font-mono text-xs text-slate-500">Genesis block of this cluster</div>
            </TerminalPanelContent>
          </TerminalPanel>
        </div>

        <div className="grid gap-6 xl:grid-cols-5">
          <div className="space-y-6 xl:col-span-2">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Cluster Info</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Cluster ID" layout="vertical" valueClassName="w-full">
                    <HexDisplay
                      value={cluster.clusterId}
                      truncate={false}
                      color="accent"
                      size="sm"
                    />
                  </DataField>
                  {cluster.description && (
                    <DataField label="Description" layout="vertical" valueClassName="w-full">
                      <ClusterDescription description={cluster.description} />
                    </DataField>
                  )}
                  <DataField label="Total Spores">
                    <span className="text-amber text-xl font-semibold tabular-nums">
                      {formatNumber(cluster.sporesCount)}
                    </span>
                  </DataField>
                  <DataField label="Creator" layout="vertical" valueClassName="w-full">
                    {creatorAddress ? (
                      <Address address={creatorAddress} truncate={false} />
                    ) : (
                      <span className="font-mono text-slate-500">Address unavailable</span>
                    )}
                  </DataField>
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>

            <TerminalPanel>
              <TerminalPanelHeader indicator="active">
                Content Snapshot (Filtered View)
              </TerminalPanelHeader>
              <TerminalPanelContent>
                {sporesLoading ? (
                  <div className="py-4 text-sm text-slate-500">Preparing content snapshot...</div>
                ) : !filteredAndSortedSpores.length ? (
                  <div className="py-4 text-sm text-slate-500">
                    No spores to summarize for current filters.
                  </div>
                ) : (
                  <div className="space-y-3">
                    {pageContentBreakdown.map((item) => (
                      <div
                        key={item.type}
                        className="rounded border border-slate-800 bg-slate-900/40 p-2.5"
                      >
                        <div className="mb-1 flex items-center justify-between gap-3">
                          <span className="font-mono text-xs uppercase tracking-wider text-slate-400">
                            {item.type}
                          </span>
                          <span className="font-mono text-xs text-slate-300">
                            {item.count} ({item.percent}%)
                          </span>
                        </div>
                        <div className="h-1.5 overflow-hidden rounded bg-slate-800">
                          <div
                            className="bg-terminal-green h-full"
                            style={{ width: `${item.percent}%` }}
                          />
                        </div>
                      </div>
                    ))}
                    <div className="rounded border border-slate-800 bg-slate-900/40 px-3 py-2">
                      <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                        Average Payload Size
                      </div>
                      <div className="mt-1 font-mono text-sm text-white">
                        {avgPayloadBytes !== null ? `${formatNumber(avgPayloadBytes)} B` : '--'}
                      </div>
                    </div>
                  </div>
                )}
              </TerminalPanelContent>
            </TerminalPanel>
          </div>

          <div className="space-y-6 xl:col-span-3">
            <CapacityOccupationSection
              description="Daily cumulative live CKB occupation for this Spore collection."
              occupationRange={occupationRange}
              onOccupationRangeChange={setOccupationRange}
              occupationChart={occupationChart}
              isOccupationChartLoading={isOccupationChartLoading}
              totalCapacity={cluster.liveCapacity}
              occupiedCapacity={cluster.liveOccupiedCapacity}
            />

            <TerminalPanel>
              <Tabs
                value={activeCollectionTab}
                onValueChange={(nextValue) => {
                  if (isCollectionSectionTab(nextValue)) {
                    setActiveCollectionTab(nextValue);
                  }
                }}
              >
                <TerminalPanelHeader
                  indicator="active"
                  actions={
                    <div className="flex w-full flex-wrap items-center justify-between gap-3">
                      {activeCollectionTab === 'nfts' && (
                        <div
                          data-testid="spore-list-controls"
                          className="flex flex-1 flex-wrap items-center gap-2"
                        >
                          <label className="sr-only" htmlFor="spore-list-query">
                            Search spores
                          </label>
                          <input
                            id="spore-list-query"
                            aria-label="Search spores"
                            type="text"
                            value={listQuery}
                            onChange={(event) => setListQuery(event.target.value)}
                            placeholder="Search hash / owner / type"
                            className="w-full rounded border border-slate-700 bg-slate-900 px-2 py-1 font-mono text-xs text-slate-200 placeholder:text-slate-500 sm:w-48"
                          />

                          <label className="sr-only" htmlFor="spore-content-filter">
                            Filter spores by content type
                          </label>
                          <select
                            id="spore-content-filter"
                            aria-label="Filter spores by content type"
                            value={listContentFilter}
                            onChange={(event) =>
                              setListContentFilter(
                                event.target.value as
                                  | 'all'
                                  | 'image'
                                  | 'video'
                                  | 'audio'
                                  | 'text'
                                  | 'other'
                              )
                            }
                            className="rounded border border-slate-700 bg-slate-900 px-2 py-1 font-mono text-xs text-slate-200"
                          >
                            <option value="all">All Types</option>
                            <option value="image">Image</option>
                            <option value="video">Video</option>
                            <option value="audio">Audio</option>
                            <option value="text">Text</option>
                            <option value="other">Other</option>
                          </select>

                          <div className="font-mono text-xs text-slate-500">
                            {filteredAndSortedSpores.length} shown /{' '}
                            {formatNumber(cluster.sporesCount)} total
                          </div>
                        </div>
                      )}
                      <TabsList className="border-b-0">
                        <TabsTrigger value="activities">Activities</TabsTrigger>
                        <TabsTrigger value="nfts">NFTs</TabsTrigger>
                        <TabsTrigger value="holders">Holders</TabsTrigger>
                      </TabsList>
                    </div>
                  }
                >
                  {activeCollectionTab === 'activities'
                    ? 'Activities'
                    : activeCollectionTab === 'holders'
                      ? 'Holders'
                      : 'NFTs'}
                </TerminalPanelHeader>

                <TabsContent value="activities" className="py-0">
                  <TerminalPanelContent>
                    {isClusterActivitiesLoading || isClusterActivitiesFetching ? (
                      <div className="py-8 text-center text-slate-500">Loading activities...</div>
                    ) : isClusterActivitiesError ? (
                      <div className="py-8 text-center text-rose-400">
                        Failed to load activities. Please refresh and try again.
                      </div>
                    ) : !clusterActivities?.data?.length ? (
                      <div className="py-8 text-center text-slate-500">
                        No activities in this collection
                      </div>
                    ) : (
                      <div className="space-y-2">
                        {clusterActivities.data.map((activity) => (
                          <div
                            key={`${activity.txHash}-${activity.txIndex}`}
                            className="space-y-2 rounded border border-slate-800 bg-slate-900/40 p-3"
                          >
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div className="font-mono text-xs text-slate-400">
                                Block{' '}
                                <Link
                                  href={`/blocks/${activity.blockNumber}`}
                                  className="text-terminal-green hover:underline"
                                >
                                  #{formatNumber(activity.blockNumber)}
                                </Link>
                                <span className="mx-1 text-slate-600">•</span>
                                Tx Index {activity.txIndex}
                              </div>
                              <div className="flex flex-wrap gap-1.5">
                                {activity.actions.map((action) => (
                                  <Badge
                                    key={`${activity.txHash}-${activity.txIndex}-${action}`}
                                    variant={
                                      action === 'mint'
                                        ? 'green'
                                        : action === 'burn'
                                          ? 'red'
                                          : 'neutral'
                                    }
                                  >
                                    {action}
                                  </Badge>
                                ))}
                              </div>
                            </div>
                            <Link
                              href={`/tx/${activity.txHash}`}
                              className="block font-mono text-xs text-slate-300 hover:underline"
                            >
                              <HexDisplay
                                value={activity.txHash}
                                color="accent"
                                size="sm"
                                startChars={14}
                                endChars={10}
                              />
                            </Link>
                            <div className="font-mono text-xs text-slate-500">
                              Timestamp: {activity.timestamp}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </TerminalPanelContent>
                  <TerminalPanelFooter>
                    <CursorPagination
                      totalLabel="Activities"
                      pageSize={20}
                      page={clusterActivitiesPagination.page}
                      currentCount={clusterActivities?.data?.length ?? 0}
                      hasMore={clusterActivities?.hasMore ?? false}
                      hasPrevious={clusterActivitiesPagination.hasPrevious}
                      onNext={() =>
                        clusterActivitiesPagination.goToNext(clusterActivities?.nextCursor)
                      }
                      onPrevious={clusterActivitiesPagination.goToPrevious}
                    />
                  </TerminalPanelFooter>
                </TabsContent>

                <TabsContent value="nfts" className="py-0">
                  <TerminalPanelContent padding="none">
                    {sporesLoading ? (
                      <div className="py-8 text-center text-slate-400">Loading spores...</div>
                    ) : filteredAndSortedSpores.length ? (
                      <>
                        <div className="hidden border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500 md:block">
                          <div className="grid grid-cols-[minmax(0,1.8fr)_minmax(0,1fr)_90px_80px_minmax(0,1.2fr)_110px] items-center gap-3">
                            <div>Spore ID</div>
                            <div>Content</div>
                            <div className="text-right">
                              <button
                                type="button"
                                onClick={() =>
                                  setListSort((current) =>
                                    current === 'sizeDesc' ? 'sizeAsc' : 'sizeDesc'
                                  )
                                }
                                aria-label="Sort spores by size"
                                className="ml-auto inline-flex items-center gap-1 text-right font-mono text-xs uppercase tracking-wider text-slate-500 transition hover:text-slate-300"
                              >
                                <span>Size</span>
                                <span aria-hidden>{getSortIndicator(sizeSortDirection)}</span>
                              </button>
                            </div>
                            <div className="text-center">Status</div>
                            <div className="text-right">Owner</div>
                            <div className="text-right">
                              <button
                                type="button"
                                onClick={() =>
                                  setListSort((current) =>
                                    current === 'createdDesc' ? 'createdAsc' : 'createdDesc'
                                  )
                                }
                                aria-label="Sort spores by block"
                                className="ml-auto inline-flex items-center gap-1 text-right font-mono text-xs uppercase tracking-wider text-slate-500 transition hover:text-slate-300"
                              >
                                <span>Block</span>
                                <span aria-hidden>{getSortIndicator(blockSortDirection)}</span>
                              </button>
                            </div>
                          </div>
                        </div>

                        {filteredAndSortedSpores.map((spore) => {
                          const sporeId = safeString(spore.sporeId);
                          const contentType = safeString(spore.contentType, 'unknown');
                          const contentSize = safeNumber(spore.contentSize);
                          const createdAtBlock = safeNumber(spore.createdAtBlock);
                          const ownerLockHash = safeString(spore.ownerLockHash);
                          const ownerAddress = safeString(spore.ownerAddress);
                          const resolvedOwnerAddress = resolveSporeOwnerAddress(
                            ownerLockHash,
                            ownerAddress
                          );
                          const rowKey =
                            sporeId ||
                            `${safeString(spore.txHash, 'unknown-tx')}:${safeNumber(spore.outputIndex)}`;
                          const isLive = spore.isLive !== false;
                          return (
                            <TerminalRow key={rowKey}>
                              <div className="hidden md:grid md:grid-cols-[minmax(0,1.8fr)_minmax(0,1fr)_90px_80px_minmax(0,1.2fr)_110px] md:items-center md:gap-3">
                                <div className="flex items-center gap-2">
                                  <span className="text-base">
                                    {getContentTypeIcon(contentType)}
                                  </span>
                                  {sporeId ? (
                                    <Link href={`/nfts/${sporeId}`} className="hover:underline">
                                      <HexDisplay value={sporeId} color="accent" size="sm" />
                                    </Link>
                                  ) : (
                                    <span className="font-mono text-xs text-slate-500">
                                      Unknown spore ID
                                    </span>
                                  )}
                                </div>
                                <div className="font-mono text-xs text-slate-300">
                                  {contentType}
                                </div>
                                <div className="text-right font-mono text-xs text-slate-300">
                                  {formatNumber(contentSize)} B
                                </div>
                                <div className="text-center">
                                  {isLive ? (
                                    <Badge variant="green">Live</Badge>
                                  ) : (
                                    <Badge variant="red">Burned</Badge>
                                  )}
                                </div>
                                <div className="text-right">
                                  {resolvedOwnerAddress ? (
                                    <Address address={resolvedOwnerAddress} truncate />
                                  ) : (
                                    <span className="font-mono text-xs text-slate-500">
                                      Address unavailable
                                    </span>
                                  )}
                                </div>
                                <div className="text-right">
                                  <Link
                                    href={`/blocks/${createdAtBlock}`}
                                    className="text-terminal-green font-mono text-xs hover:underline"
                                  >
                                    #{formatNumber(createdAtBlock)}
                                  </Link>
                                </div>
                              </div>

                              <div className="space-y-2 md:hidden">
                                <div className="flex items-start justify-between gap-3">
                                  <div className="flex items-center gap-2">
                                    <span className="text-base">
                                      {getContentTypeIcon(contentType)}
                                    </span>
                                    {sporeId ? (
                                      <Link href={`/nfts/${sporeId}`} className="hover:underline">
                                        <HexDisplay value={sporeId} color="accent" size="sm" />
                                      </Link>
                                    ) : (
                                      <span className="font-mono text-xs text-slate-500">
                                        Unknown spore ID
                                      </span>
                                    )}
                                  </div>
                                  {isLive ? (
                                    <Badge variant="green">Live</Badge>
                                  ) : (
                                    <Badge variant="red">Burned</Badge>
                                  )}
                                </div>
                                <div className="flex items-center justify-between gap-3 text-xs">
                                  <span className="font-mono text-slate-500">{contentType}</span>
                                  <span className="font-mono text-slate-300">
                                    {formatNumber(contentSize)} B
                                  </span>
                                </div>
                                <div className="flex items-center justify-between gap-3 text-xs">
                                  <span className="font-mono text-slate-500">
                                    Block #{formatNumber(createdAtBlock)}
                                  </span>
                                  {resolvedOwnerAddress ? (
                                    <Address address={resolvedOwnerAddress} truncate />
                                  ) : (
                                    <span className="font-mono text-xs text-slate-500">
                                      Address unavailable
                                    </span>
                                  )}
                                </div>
                              </div>
                            </TerminalRow>
                          );
                        })}
                      </>
                    ) : (sporesData?.data?.length ?? 0) > 0 ? (
                      <div className="py-8 text-center text-slate-500">
                        No spores match current filters
                      </div>
                    ) : (
                      <div className="py-8 text-center text-slate-500">
                        No spores in this collection
                      </div>
                    )}
                  </TerminalPanelContent>

                  {sporesData && filteredAndSortedSpores.length > 0 && (
                    <TerminalPanelFooter>
                      <CursorPagination
                        total={cluster.sporesCount}
                        totalLabel="Spores"
                        pageSize={20}
                        page={sporesPagination.page}
                        currentCount={filteredAndSortedSpores.length}
                        hasMore={sporesData.hasMore}
                        hasPrevious={sporesPagination.hasPrevious}
                        onNext={() => sporesPagination.goToNext(sporesData.nextCursor)}
                        onPrevious={sporesPagination.goToPrevious}
                      />
                    </TerminalPanelFooter>
                  )}
                </TabsContent>

                <TabsContent value="holders" className="py-0">
                  <TerminalPanelContent>
                    {isClusterHoldersLoading || isClusterHoldersFetching ? (
                      <div className="py-8 text-center text-slate-500">Loading holders...</div>
                    ) : isClusterHoldersError ? (
                      <div className="py-8 text-center text-rose-400">
                        Failed to load holders. Please refresh and try again.
                      </div>
                    ) : !clusterHolders?.data?.length ? (
                      <div className="py-8 text-center text-slate-500">
                        No holders in this collection
                      </div>
                    ) : (
                      <div className="overflow-hidden rounded border border-slate-800 bg-slate-900/30">
                        {clusterHolders.data.map((holder) => (
                          <div
                            key={holder.lockScriptHash}
                            className="row-scan hover:bg-slate-850/40 flex items-center justify-between gap-3 border-b border-slate-800 px-3 py-2.5 transition-colors last:border-b-0"
                          >
                            <div className="min-w-0">
                              <Link
                                href={`/address/${holder.address ?? holder.lockScriptHash}`}
                                className="font-mono text-xs text-slate-300 hover:underline"
                              >
                                {holder.address ? (
                                  holder.address
                                ) : (
                                  <HexDisplay
                                    value={holder.lockScriptHash}
                                    color="accent"
                                    size="sm"
                                    startChars={12}
                                    endChars={10}
                                  />
                                )}
                              </Link>
                            </div>
                            <div className="shrink-0 font-mono text-sm text-white">
                              {formatNumber(holder.itemCount)}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </TerminalPanelContent>
                  <TerminalPanelFooter>
                    <CursorPagination
                      total={clusterHolders?.total}
                      totalLabel="Holders"
                      pageSize={20}
                      page={clusterHoldersPagination.page}
                      currentCount={clusterHolders?.data?.length ?? 0}
                      hasMore={clusterHolders?.hasMore ?? false}
                      hasPrevious={clusterHoldersPagination.hasPrevious}
                      onNext={() => clusterHoldersPagination.goToNext(clusterHolders?.nextCursor)}
                      onPrevious={clusterHoldersPagination.goToPrevious}
                    />
                  </TerminalPanelFooter>
                </TabsContent>
              </Tabs>
            </TerminalPanel>
          </div>
        </div>
      </main>
    </div>
  );
}
