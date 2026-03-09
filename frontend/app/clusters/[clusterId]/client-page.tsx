'use client';

import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueries, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
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
import { NftActivityCard } from '@/components/nft/nft-activity-card';
import { NftCollectionStatCards } from '@/components/nft/nft-collection-stat-cards';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { getOccupationRangeParams, OccupationRangeKey } from '@/lib/occupation-range';
import { ClusterDescription } from '@/components/spore/cluster-description';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { formatNumber } from '@/lib/utils';
import { formatActivityTimestamp } from '@/lib/nft-utils';

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

function getSortIndicator(direction: 'asc' | 'desc' | null): string {
  if (direction === 'asc') return '↑';
  if (direction === 'desc') return '↓';
  return '↕';
}

export interface ClusterDetailPageProps {
  clusterId: string;
}

export default function ClusterDetailPage({ clusterId }: ClusterDetailPageProps) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const tabFromQuery = searchParams.get('tab');
  const [occupationRange, setOccupationRange] = useState<OccupationRangeKey>('all');
  const [activeCollectionTab, setActiveCollectionTab] = useState<CollectionSectionTab>(() =>
    isCollectionSectionTab(tabFromQuery) ? tabFromQuery : 'activities'
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

    if (activeCollectionTab === 'activities') {
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
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-64 animate-pulse rounded" />
          <div className="grid gap-6 lg:grid-cols-3">
            <div className="border-base-border bg-base-surface/50 h-64 animate-pulse rounded border" />
            <div className="border-base-border bg-base-surface/50 h-96 animate-pulse rounded border lg:col-span-2" />
          </div>
        </main>
      </div>
    );
  }

  if (clusterError || !cluster) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-muted text-xl">Spore Cluster not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/assets?type=nft"
            className="hover:text-emphasis text-text-muted text-sm transition-colors"
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

        <NftCollectionStatCards
          totalCount={cluster.sporesCount}
          totalLabel="Total Spores"
          liveCapacity={cluster.liveCapacity}
          liveOccupiedCapacity={cluster.liveOccupiedCapacity}
          createdAtBlock={cluster.createdAtBlock}
          storageTier={cluster.storageProfile?.tier}
          storageOnchainRatio={cluster.storageProfile?.fullyOnchainRatio}
        />

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
                    <span className="text-warning text-xl font-semibold tabular-nums">
                      {formatNumber(cluster.sporesCount)}
                    </span>
                  </DataField>
                  <DataField label="Creator" layout="vertical" valueClassName="w-full">
                    {creatorAddress ? (
                      <Address address={creatorAddress} truncate={false} />
                    ) : (
                      <span className="text-text-muted font-mono">Address unavailable</span>
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
                  <div className="text-text-muted py-4 text-sm">Preparing content snapshot...</div>
                ) : !filteredAndSortedSpores.length ? (
                  <div className="text-text-muted py-4 text-sm">
                    No spores to summarize for current filters.
                  </div>
                ) : (
                  <div className="space-y-3">
                    {pageContentBreakdown.map((item) => (
                      <div
                        key={item.type}
                        className="border-base-border bg-base-surface/40 rounded border p-2.5"
                      >
                        <div className="mb-1 flex items-center justify-between gap-3">
                          <span className="text-text-muted font-mono text-xs uppercase tracking-wider">
                            {item.type}
                          </span>
                          <span className="text-text-secondary font-mono text-xs">
                            {item.count} ({item.percent}%)
                          </span>
                        </div>
                        <div className="bg-base-elevated h-1.5 overflow-hidden rounded">
                          <div
                            className="bg-emphasis h-full"
                            style={{ width: `${item.percent}%` }}
                          />
                        </div>
                      </div>
                    ))}
                    <div className="border-base-border bg-base-surface/40 rounded border px-3 py-2">
                      <div className="text-text-muted font-mono text-xs uppercase tracking-wider">
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
                            className="border-base-border bg-base-surface text-text-primary placeholder:text-text-muted w-full rounded border px-2 py-1 font-mono text-xs sm:w-48"
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
                            className="border-base-border bg-base-surface text-text-primary rounded border px-2 py-1 font-mono text-xs"
                          >
                            <option value="all">All Types</option>
                            <option value="image">Image</option>
                            <option value="video">Video</option>
                            <option value="audio">Audio</option>
                            <option value="text">Text</option>
                            <option value="other">Other</option>
                          </select>

                          <div className="text-text-muted font-mono text-xs">
                            {filteredAndSortedSpores.length} shown /{' '}
                            {formatNumber(cluster.sporesCount)} total
                          </div>
                        </div>
                      )}
                      <TabsList className="border-b-0">
                        <TabsTrigger value="activities">
                          Activities ({formatNumber(cluster.activitiesCount)})
                        </TabsTrigger>
                        <TabsTrigger value="nfts">
                          NFTs ({formatNumber(cluster.sporesCount)})
                        </TabsTrigger>
                        <TabsTrigger value="holders">
                          Holders ({formatNumber(cluster.holdersCount)})
                        </TabsTrigger>
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
                    {isClusterActivitiesLoading && !clusterActivities ? (
                      <div className="text-text-muted py-8 text-center">Loading activities...</div>
                    ) : isClusterActivitiesError ? (
                      <div className="py-8 text-center text-rose-400">
                        Failed to load activities. Please refresh and try again.
                      </div>
                    ) : !clusterActivities?.data?.length ? (
                      <div className="text-text-muted py-8 text-center">
                        No activities in this collection
                      </div>
                    ) : (
                      <div className="space-y-2">
                        {clusterActivities.data.map((activity) => (
                          <NftActivityCard
                            key={`${activity.txHash}-${activity.txIndex}`}
                            txHash={activity.txHash}
                            blockNumber={activity.blockNumber}
                            txIndex={activity.txIndex}
                            timestamp={formatActivityTimestamp(activity.timestamp)}
                            actions={activity.actions}
                            badgeActions
                          />
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
                      <div className="text-text-muted py-8 text-center">Loading spores...</div>
                    ) : filteredAndSortedSpores.length ? (
                      <>
                        <div className="border-base-border bg-base-surface/50 text-text-muted hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:block">
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
                                className="text-text-muted hover:text-text-secondary ml-auto inline-flex items-center gap-1 text-right font-mono text-xs uppercase tracking-wider transition"
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
                                className="text-text-muted hover:text-text-secondary ml-auto inline-flex items-center gap-1 text-right font-mono text-xs uppercase tracking-wider transition"
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
                                    <span className="text-text-muted font-mono text-xs">
                                      Unknown spore ID
                                    </span>
                                  )}
                                </div>
                                <div className="text-text-secondary font-mono text-xs">
                                  {contentType}
                                </div>
                                <div className="text-text-secondary text-right font-mono text-xs">
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
                                    <span className="text-text-muted font-mono text-xs">
                                      Address unavailable
                                    </span>
                                  )}
                                </div>
                                <div className="text-right">
                                  <Link
                                    href={`/blocks/${createdAtBlock}`}
                                    className="text-emphasis font-mono text-xs hover:underline"
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
                                      <span className="text-text-muted font-mono text-xs">
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
                                  <span className="text-text-muted font-mono">{contentType}</span>
                                  <span className="text-text-secondary font-mono">
                                    {formatNumber(contentSize)} B
                                  </span>
                                </div>
                                <div className="flex items-center justify-between gap-3 text-xs">
                                  <span className="text-text-muted font-mono">
                                    Block #{formatNumber(createdAtBlock)}
                                  </span>
                                  {resolvedOwnerAddress ? (
                                    <Address address={resolvedOwnerAddress} truncate />
                                  ) : (
                                    <span className="text-text-muted font-mono text-xs">
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
                      <div className="text-text-muted py-8 text-center">
                        No spores match current filters
                      </div>
                    ) : (
                      <div className="text-text-muted py-8 text-center">
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
                    {isClusterHoldersLoading && !clusterHolders ? (
                      <div className="text-text-muted py-8 text-center">Loading holders...</div>
                    ) : isClusterHoldersError ? (
                      <div className="py-8 text-center text-rose-400">
                        Failed to load holders. Please refresh and try again.
                      </div>
                    ) : !clusterHolders?.data?.length ? (
                      <div className="text-text-muted py-8 text-center">
                        No holders in this collection
                      </div>
                    ) : (
                      <div className="border-base-border bg-base-surface/30 overflow-hidden rounded border">
                        {clusterHolders.data.map((holder) => (
                          <div
                            key={holder.lockScriptHash}
                            className="row-scan hover:bg-base-elevated/40 border-base-border flex items-center justify-between gap-3 border-b px-3 py-2.5 transition-colors last:border-b-0"
                          >
                            <div className="min-w-0">
                              <Link
                                href={`/address/${holder.address ?? holder.lockScriptHash}`}
                                className="text-text-secondary font-mono text-xs hover:underline"
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
