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
import { formatCkbCompact } from '@/lib/utils';

type ListContentFilter = 'all' | 'image' | 'video' | 'audio' | 'text' | 'other';
type ListSort = 'createdDesc' | 'createdAsc' | 'sizeDesc' | 'sizeAsc';

const LIST_FILTER_VALUES: ListContentFilter[] = ['all', 'image', 'video', 'audio', 'text', 'other'];
const LIST_SORT_VALUES: ListSort[] = ['createdDesc', 'createdAsc', 'sizeDesc', 'sizeAsc'];

function isListContentFilter(value: string | null): value is ListContentFilter {
  return !!value && LIST_FILTER_VALUES.includes(value as ListContentFilter);
}

function isListSort(value: string | null): value is ListSort {
  return !!value && LIST_SORT_VALUES.includes(value as ListSort);
}

export default function ClusterDetailPage() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const params = useParams();
  const clusterId = params.clusterId as string;
  const [occupationRange, setOccupationRange] = useState<OccupationRangeKey>('all');
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

  const getContentTypeIcon = (contentType: string) => {
    if (contentType.startsWith('image/')) return '🖼️';
    if (contentType.startsWith('video/')) return '🎬';
    if (contentType.startsWith('audio/')) return '🎵';
    if (contentType.startsWith('text/')) return '📄';
    return '📦';
  };

  const summarizeContentType = (contentType: string): string => {
    if (!contentType) {
      return 'unknown';
    }
    const [primary] = contentType.toLowerCase().split('/');
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

    const nextQuery = nextParams.toString();
    if (nextQuery === currentQuery) {
      return;
    }
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  }, [listContentFilter, listSort, normalizedQuery, pathname, router, searchParams]);

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
          return (
            spore.sporeId.toLowerCase().includes(normalizedQuery) ||
            spore.contentType.toLowerCase().includes(normalizedQuery) ||
            spore.ownerLockHash.toLowerCase().includes(normalizedQuery) ||
            (spore.ownerAddress?.toLowerCase().includes(normalizedQuery) ?? false)
          );
        })
      : filtered;

    const sorted = [...queryFiltered];
    sorted.sort((a, b) => {
      if (listSort === 'createdDesc') {
        return b.createdAtBlock - a.createdAtBlock;
      }
      if (listSort === 'createdAsc') {
        return a.createdAtBlock - b.createdAtBlock;
      }
      if (listSort === 'sizeDesc') {
        return b.contentSize - a.contentSize;
      }
      if (listSort === 'sizeAsc') {
        return a.contentSize - b.contentSize;
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
      if (spore.ownerAddress || !spore.ownerLockHash) {
        continue;
      }
      const normalized = spore.ownerLockHash.toLowerCase();
      if (!unique.has(normalized)) {
        unique.set(normalized, spore.ownerLockHash);
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

  const resolveSporeOwnerAddress = (ownerLockHash: string, ownerAddress?: string) => {
    if (ownerAddress) {
      return ownerAddress;
    }
    return sporeOwnerAddressByLockHash.get(ownerLockHash.toLowerCase()) ?? null;
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
    const sum = filteredAndSortedSpores.reduce((acc, item) => acc + item.contentSize, 0);
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
          badge={<Badge variant="neutral">Spore Cluster</Badge>}
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
              <TerminalPanelHeader
                indicator="active"
                actions={
                  <div className="flex flex-wrap items-center gap-2">
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

                    <label className="sr-only" htmlFor="spore-list-sort">
                      Sort spores
                    </label>
                    <select
                      id="spore-list-sort"
                      aria-label="Sort spores"
                      value={listSort}
                      onChange={(event) =>
                        setListSort(
                          event.target.value as
                            | 'createdDesc'
                            | 'createdAsc'
                            | 'sizeDesc'
                            | 'sizeAsc'
                        )
                      }
                      className="rounded border border-slate-700 bg-slate-900 px-2 py-1 font-mono text-xs text-slate-200"
                    >
                      <option value="createdDesc">Latest Block</option>
                      <option value="createdAsc">Earliest Block</option>
                      <option value="sizeDesc">Largest Payload</option>
                      <option value="sizeAsc">Smallest Payload</option>
                    </select>

                    <div className="font-mono text-xs text-slate-500">
                      {filteredAndSortedSpores.length} shown /{' '}
                      {formatNumber(sporesData?.total || 0)} total
                    </div>
                  </div>
                }
              >
                Spores in this collection ({formatNumber(sporesData?.total || 0)})
              </TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {sporesLoading ? (
                  <div className="py-8 text-center text-slate-400">Loading spores...</div>
                ) : filteredAndSortedSpores.length ? (
                  <>
                    <div className="hidden border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500 md:block">
                      <div className="grid grid-cols-[minmax(0,1.8fr)_minmax(0,1fr)_90px_80px_minmax(0,1.2fr)_110px] items-center gap-3">
                        <div>Spore ID</div>
                        <div>Content</div>
                        <div className="text-right">Size</div>
                        <div className="text-center">Status</div>
                        <div className="text-right">Owner</div>
                        <div className="text-right">Block</div>
                      </div>
                    </div>

                    {filteredAndSortedSpores.map((spore) => {
                      const resolvedOwnerAddress = resolveSporeOwnerAddress(
                        spore.ownerLockHash,
                        spore.ownerAddress
                      );
                      return (
                        <TerminalRow key={spore.sporeId}>
                          <div className="hidden md:grid md:grid-cols-[minmax(0,1.8fr)_minmax(0,1fr)_90px_80px_minmax(0,1.2fr)_110px] md:items-center md:gap-3">
                            <div className="flex items-center gap-2">
                              <span className="text-base">
                                {getContentTypeIcon(spore.contentType)}
                              </span>
                              <Link href={`/nfts/${spore.sporeId}`} className="hover:underline">
                                <HexDisplay value={spore.sporeId} color="accent" size="sm" />
                              </Link>
                            </div>
                            <div className="font-mono text-xs text-slate-300">
                              {spore.contentType}
                            </div>
                            <div className="text-right font-mono text-xs text-slate-300">
                              {formatNumber(spore.contentSize)} B
                            </div>
                            <div className="text-center">
                              {spore.isLive ? (
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
                                href={`/blocks/${spore.createdAtBlock}`}
                                className="text-terminal-green font-mono text-xs hover:underline"
                              >
                                #{formatNumber(spore.createdAtBlock)}
                              </Link>
                            </div>
                          </div>

                          <div className="space-y-2 md:hidden">
                            <div className="flex items-start justify-between gap-3">
                              <div className="flex items-center gap-2">
                                <span className="text-base">
                                  {getContentTypeIcon(spore.contentType)}
                                </span>
                                <Link href={`/nfts/${spore.sporeId}`} className="hover:underline">
                                  <HexDisplay value={spore.sporeId} color="accent" size="sm" />
                                </Link>
                              </div>
                              {spore.isLive ? (
                                <Badge variant="green">Live</Badge>
                              ) : (
                                <Badge variant="red">Burned</Badge>
                              )}
                            </div>
                            <div className="flex items-center justify-between gap-3 text-xs">
                              <span className="font-mono text-slate-500">{spore.contentType}</span>
                              <span className="font-mono text-slate-300">
                                {formatNumber(spore.contentSize)} B
                              </span>
                            </div>
                            <div className="flex items-center justify-between gap-3 text-xs">
                              <span className="font-mono text-slate-500">
                                Block #{formatNumber(spore.createdAtBlock)}
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
                    total={sporesData.total}
                    totalLabel="Spores"
                    pageSize={20}
                    page={sporesPagination.page}
                    hasMore={sporesData.hasMore}
                    hasPrevious={sporesPagination.hasPrevious}
                    onNext={() => sporesPagination.goToNext(sporesData.nextCursor)}
                    onPrevious={sporesPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              )}
            </TerminalPanel>
          </div>
        </div>
      </main>
    </div>
  );
}
