'use client';
import { useEffect, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import {
  api,
  type CollectionActivity,
  type CollectionHolder,
  type ItemStatusFilter,
} from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CapacityStatisticsSection } from '@/components/ui/capacity-statistics-section';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { ObjectActivityCard } from '@/components/object/object-activity-card';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
const DOTBIT_COLLECTION_ID = '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f';

function isDotbitAlias(assetId: string): boolean {
  const normalized = assetId.toLowerCase();
  return normalized === 'dotbit' || normalized === '.bit' || normalized === DOTBIT_COLLECTION_ID;
}
import { formatNumber } from '@/lib/utils';
import { formatActivityTimestamp } from '@/lib/asset-utils';
type IdentityTab = 'activities' | 'identities' | 'holders';
function isIdentityTab(value: string | null): value is IdentityTab {
  return value === 'activities' || value === 'identities' || value === 'holders';
}
export interface IdentityCollectionPageProps {
  collectionId: string;
}
export default function IdentityCollectionPage({ collectionId }: IdentityCollectionPageProps) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const tabFromQuery = searchParams.get('tab');
  const [capacityRange, setCapacityRange] = useState<CapacityRangeKey>('all');
  const [searchInput, setSearchInput] = useState('');
  const [searchKeyword, setSearchKeyword] = useState('');
  const [statusFilter, setStatusFilter] = useState<ItemStatusFilter>('all');
  const [activeTab, setActiveTab] = useState<IdentityTab>(() =>
    isIdentityTab(tabFromQuery) ? tabFromQuery : 'activities'
  );
  const itemsPagination = useCursorPagination();
  const holdersPagination = useCursorPagination();
  const activitiesPagination = useCursorPagination();
  const { reset: resetItemsPagination } = itemsPagination;
  const { reset: resetHoldersPagination } = holdersPagination;
  const { reset: resetActivitiesPagination } = activitiesPagination;
  const capacityRangeParams = getCapacityRangeParams(capacityRange);
  // Debounced search (250ms)
  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setSearchKeyword(searchInput.trim());
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [searchInput]);
  // Fetch identity collection detail
  const collectionQuery = useQuery({
    queryKey: ['identity-collection', collectionId],
    queryFn: () => api.getIdentityCollection(collectionId),
    retry: false,
  });
  const collection = collectionQuery.data;
  const isDotbit = collection
    ? isDotbitAlias(collection.collectionId) || collection.standard.toLowerCase() === 'dotbit'
    : isDotbitAlias(collectionId);
  const searchLabel = isDotbit ? 'Search .bit' : 'Search did:ckb';
  const itemDetailPrefix = isDotbit ? '/identities/dotbit' : '/identities/did';
  // Fetch collection items
  const {
    data: collectionItems,
    isLoading: isItemsLoading,
    isFetching: isItemsFetching,
    isError: isItemsError,
  } = useQuery({
    queryKey: [
      'identity-collection-items',
      collectionId,
      itemsPagination.cursor,
      searchKeyword,
      statusFilter,
    ],
    queryFn: () =>
      api.getIdentityCollectionItems(collectionId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: itemsPagination.cursor,
        search: searchKeyword || undefined,
        status: statusFilter,
      }),
    enabled: !!collection && activeTab === 'identities',
    placeholderData: keepPreviousData,
  });
  // Fetch collection holders
  const {
    data: collectionHolders,
    isLoading: isHoldersLoading,
    isError: isHoldersError,
  } = useQuery({
    queryKey: ['identity-collection-holders', collectionId, holdersPagination.cursor],
    queryFn: () =>
      api.getIdentityCollectionHolders(collectionId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: holdersPagination.cursor,
      }),
    enabled: !!collection && activeTab === 'holders',
    placeholderData: keepPreviousData,
  });
  // Fetch collection activities
  const {
    data: collectionActivities,
    isLoading: isActivitiesLoading,
    isError: isActivitiesError,
  } = useQuery({
    queryKey: ['identity-collection-activities', collectionId, activitiesPagination.cursor],
    queryFn: () =>
      api.getIdentityCollectionActivities(collectionId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: activitiesPagination.cursor,
      }),
    enabled: !!collection && activeTab === 'activities',
    placeholderData: keepPreviousData,
  });
  const { data: capacityChart, isLoading: isCapacityChartLoading } = useQuery({
    queryKey: ['identity-collection-capacity-chart', collectionId, capacityRange],
    queryFn: () =>
      capacityRangeParams
        ? api.getIdentityCollectionCapacityChart(collectionId, capacityRangeParams)
        : api.getIdentityCollectionCapacityChart(collectionId),
    enabled: !!collection,
  });
  // Reset items pagination when search/filter changes
  useEffect(() => {
    resetItemsPagination();
  }, [collectionId, searchKeyword, statusFilter, resetItemsPagination]);
  // Reset holders/activities pagination when collection changes
  useEffect(() => {
    resetHoldersPagination();
    resetActivitiesPagination();
  }, [collectionId, resetActivitiesPagination, resetHoldersPagination]);
  const updateSearchParams = (mutator: (nextParams: URLSearchParams) => void) => {
    const nextParams = new URLSearchParams(searchParams.toString());
    mutator(nextParams);
    const nextQuery = nextParams.toString();
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  };
  const handleTabChange = (nextValue: string) => {
    if (!isIdentityTab(nextValue)) return;
    setActiveTab(nextValue);
    updateSearchParams((nextParams) => {
      if (nextValue === 'activities') {
        nextParams.delete('tab');
      } else {
        nextParams.set('tab', nextValue);
      }
    });
  };
  // Loading state
  if (collectionQuery.isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-48 animate-pulse rounded" />
          <div className="border-base-border bg-base-surface/50 mb-6 h-48 animate-pulse rounded border" />
        </main>
      </div>
    );
  }
  // Error state
  if (collectionQuery.isError || !collection) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">Identity collection not found</h2>
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
            href="/inventory/identities"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            &larr; Back to Identities
          </Link>
        </div>
        {/* Unified Collection Overview */}
        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Collection Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            {/* Name + badge */}
            <div className="flex flex-wrap items-center gap-3">
              <h1 className="text-text-bright font-mono text-2xl font-bold">
                {collection.name || 'Identity Collection'}
              </h1>
              <Badge variant="neutral">{collection.standard.toUpperCase()}</Badge>
            </div>

            {/* Collection ID */}
            <div className="mt-3 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">collection id</span>
              <HexDisplay value={collection.collectionId} truncate={false} size="sm" />
            </div>

            {/* Stat cards row */}
            <div className="border-base-border mt-4 grid grid-cols-2 gap-3 border-t pt-4 sm:grid-cols-3">
              {/* Total identities card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Total Identities
                </div>
                <div className="text-warning font-mono text-sm font-semibold tabular-nums">
                  {formatNumber(collection.totalCount)}
                </div>
                {collection.liveCount !== collection.totalCount && (
                  <div className="text-text-dim font-mono text-xs">
                    {formatNumber(collection.liveCount)} live
                  </div>
                )}
              </div>

              {/* Holders card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Holders
                </div>
                <div className="text-text-bright font-mono text-sm font-semibold tabular-nums">
                  {formatNumber(collection.holdersCount)}
                </div>
              </div>

              {/* Activities card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Activities
                </div>
                <div className="text-text-bright font-mono text-sm font-semibold tabular-nums">
                  {formatNumber(collection.activitiesCount)}
                </div>
              </div>
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        <CapacityStatisticsSection
          className="mb-6"
          capacityRange={capacityRange}
          onCapacityRangeChange={setCapacityRange}
          capacityChart={capacityChart}
          isCapacityChartLoading={isCapacityChartLoading}
          totalCapacity={collection.ownedCapacity}
          commonKnowledgeSize={collection.ownedKnowledge}
          totalCapacityLabel="Owned Capacity"
        />

        <TerminalPanel>
          <Tabs value={activeTab} onValueChange={handleTabChange}>
            <TerminalPanelHeader
              indicator="active"
              actions={
                <div className="flex flex-wrap items-center gap-3">
                  <TabsList className="border-b-0">
                    <TabsTrigger value="activities">
                      Activities ({formatNumber(collection.activitiesCount)})
                    </TabsTrigger>
                    <TabsTrigger value="identities">
                      Identities ({formatNumber(collection.totalCount)})
                    </TabsTrigger>
                    <TabsTrigger value="holders">
                      Holders ({formatNumber(collection.holdersCount)})
                    </TabsTrigger>
                  </TabsList>
                  {activeTab === 'identities' && (
                    <div className="flex items-center gap-2">
                      <select
                        value={statusFilter}
                        onChange={(event) =>
                          setStatusFilter(event.target.value as ItemStatusFilter)
                        }
                        aria-label="Status Filter"
                        className="focus:border-emphasis border-base-border bg-base-surface text-text-bright rounded border px-2.5 py-1.5 font-mono text-xs outline-none transition-colors"
                      >
                        <option value="all">All</option>
                        <option value="live">Live</option>
                        <option value="recycled">Recycled</option>
                      </select>
                      <input
                        type="text"
                        value={searchInput}
                        onChange={(event) => setSearchInput(event.target.value)}
                        placeholder={searchLabel}
                        aria-label={searchLabel}
                        className="focus:border-emphasis border-base-border bg-base-surface text-text-bright placeholder:text-text-dim w-44 rounded border px-2.5 py-1.5 font-mono text-xs outline-none transition-colors"
                      />
                      {isItemsFetching && (
                        <span className="text-text-dim font-mono text-xs">Searching...</span>
                      )}
                    </div>
                  )}
                </div>
              }
            >
              {activeTab === 'activities'
                ? 'Activities'
                : activeTab === 'holders'
                  ? 'Holders'
                  : 'Identities'}
            </TerminalPanelHeader>
            {/* Activities tab */}
            <TabsContent value="activities" className="py-0">
              <TerminalPanelContent>
                {isActivitiesLoading ? (
                  <div className="text-text-dim py-8 text-center">Loading activities...</div>
                ) : isActivitiesError ? (
                  <div className="text-rouge py-8 text-center">
                    Failed to load activities. Please refresh and try again.
                  </div>
                ) : !collectionActivities?.data?.length ? (
                  <div className="text-text-dim py-8 text-center">
                    No activities in this collection
                  </div>
                ) : (
                  <div className="space-y-2">
                    {collectionActivities.data.map((activity: CollectionActivity) => (
                      <ObjectActivityCard
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
                  total={collectionActivities?.total ?? undefined}
                  totalLabel="Activities"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={activitiesPagination.page}
                  currentCount={collectionActivities?.data?.length ?? 0}
                  hasMore={collectionActivities?.hasMore ?? false}
                  hasPrevious={activitiesPagination.hasPrevious}
                  onNext={() => activitiesPagination.goToNext(collectionActivities?.nextCursor)}
                  onPrevious={activitiesPagination.goToPrevious}
                />
              </TerminalPanelFooter>
            </TabsContent>
            {/* Identities tab */}
            <TabsContent value="identities" className="py-0">
              <TerminalPanelContent>
                {isItemsLoading ? (
                  <div className="text-text-dim py-8 text-center">Loading identities...</div>
                ) : isItemsError ? (
                  <div className="text-rouge py-8 text-center">
                    Failed to load identities. Please refresh and try again.
                  </div>
                ) : !collectionItems?.data?.length ? (
                  <div className="text-text-dim py-8 text-center">
                    No identities in this collection
                  </div>
                ) : (
                  <div className="border-base-border bg-base-surface/30 overflow-hidden rounded border">
                    {collectionItems.data.map((item) => (
                      <div
                        key={item.nftId}
                        className="row-scan hover:bg-base-elevated/40 border-base-border border-b px-3 py-2.5 transition-colors last:border-b-0"
                      >
                        <div className="mb-1 flex items-center justify-between gap-3">
                          <Link
                            href={`${itemDetailPrefix}/${encodeURIComponent(item.nftId)}`}
                            className="hover:text-emphasis text-text-bright font-mono text-sm hover:underline"
                          >
                            {item.name || item.nftId}
                          </Link>
                          {item.isLive ? (
                            <Badge variant="green">Live</Badge>
                          ) : (
                            <Badge variant="red">Recycled</Badge>
                          )}
                        </div>
                        <div className="text-text-dim flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-xs">
                          <span>
                            ID:{' '}
                            <span className="text-text">
                              <HexDisplay
                                value={item.nftId}
                                size="sm"
                                startChars={10}
                                endChars={8}
                              />
                            </span>
                          </span>
                          <span>Block #{formatNumber(item.createdAtBlock)}</span>
                          {item.isLive && (
                            <span>
                              Cell:{' '}
                              {item.txHash &&
                              item.outputIndex !== null &&
                              item.outputIndex !== undefined ? (
                                <Link
                                  href={`/cell/${item.txHash}-${item.outputIndex}`}
                                  className="text-emphasis hover:underline"
                                >
                                  <HexDisplay
                                    value={item.txHash}
                                    size="sm"
                                    startChars={10}
                                    endChars={8}
                                  />
                                  -{item.outputIndex}
                                </Link>
                              ) : (
                                <span className="text-text-dim">Unavailable</span>
                              )}
                            </span>
                          )}
                          {item.ownerLockHash && (
                            <span>
                              Owner:{' '}
                              <Link
                                href={`/address/${item.ownerLockHash}`}
                                className="hover:underline"
                              >
                                <HexDisplay
                                  value={item.ownerLockHash}
                                  size="sm"
                                  startChars={10}
                                  endChars={8}
                                />
                              </Link>
                            </span>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </TerminalPanelContent>
              <TerminalPanelFooter>
                <CursorPagination
                  total={collectionItems?.total ?? undefined}
                  totalLabel="Identities"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={itemsPagination.page}
                  currentCount={collectionItems?.data?.length ?? 0}
                  hasMore={collectionItems?.hasMore ?? false}
                  hasPrevious={itemsPagination.hasPrevious}
                  onNext={() => itemsPagination.goToNext(collectionItems?.nextCursor)}
                  onPrevious={itemsPagination.goToPrevious}
                />
              </TerminalPanelFooter>
            </TabsContent>
            {/* Holders tab */}
            <TabsContent value="holders" className="py-0">
              <TerminalPanelContent>
                {isHoldersLoading ? (
                  <div className="text-text-dim py-8 text-center">Loading holders...</div>
                ) : isHoldersError ? (
                  <div className="text-rouge py-8 text-center">
                    Failed to load holders. Please refresh and try again.
                  </div>
                ) : !collectionHolders?.data?.length ? (
                  <div className="text-text-dim py-8 text-center">
                    No holders in this collection
                  </div>
                ) : (
                  <div className="border-base-border bg-base-surface/30 overflow-hidden rounded border">
                    {collectionHolders.data.map((holder: CollectionHolder) => (
                      <div
                        key={holder.lockScriptHash}
                        className="row-scan hover:bg-base-elevated/40 border-base-border flex items-center justify-between gap-3 border-b px-3 py-2.5 transition-colors last:border-b-0"
                      >
                        <div className="min-w-0">
                          <Link
                            href={`/address/${holder.address ?? holder.lockScriptHash}`}
                            className="text-text font-mono text-xs hover:underline"
                          >
                            {holder.address ? (
                              holder.address
                            ) : (
                              <HexDisplay
                                value={holder.lockScriptHash}
                                size="sm"
                                startChars={12}
                                endChars={10}
                              />
                            )}
                          </Link>
                        </div>
                        <div className="text-text-bright shrink-0 font-mono text-sm">
                          {formatNumber(holder.itemCount)}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </TerminalPanelContent>
              <TerminalPanelFooter>
                <CursorPagination
                  total={collectionHolders?.total}
                  totalLabel="Holders"
                  pageSize={DEFAULT_PAGE_SIZE}
                  page={holdersPagination.page}
                  currentCount={collectionHolders?.data?.length ?? 0}
                  hasMore={collectionHolders?.hasMore ?? false}
                  hasPrevious={holdersPagination.hasPrevious}
                  onNext={() => holdersPagination.goToNext(collectionHolders?.nextCursor)}
                  onPrevious={holdersPagination.goToPrevious}
                />
              </TerminalPanelFooter>
            </TabsContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}
