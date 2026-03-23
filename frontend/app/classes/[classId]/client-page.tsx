'use client';
import { useEffect, useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { api, type CollectionActivity, type CollectionHolder } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CapacityStatisticsSection } from '@/components/ui/capacity-statistics-section';
import { ObjectActivityCard } from '@/components/object/object-activity-card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatNumber } from '@/lib/utils';
import { formatActivityTimestamp, normalizeAssetId } from '@/lib/asset-utils';

type CollectionSectionTab = 'activities' | 'objects' | 'holders';

function isCollectionSectionTab(value: string | null): value is CollectionSectionTab {
  return value === 'activities' || value === 'objects' || value === 'holders';
}

function decodeClassConfigure(configure: number): string {
  const flags: string[] = [];
  if ((configure & 0b00000001) !== 0) flags.push('transferable');
  if ((configure & 0b00000010) !== 0) flags.push('burnable');
  if ((configure & 0b00000100) !== 0) flags.push('mutable');
  return flags.length > 0 ? flags.join(', ') : 'none';
}

export interface MnftClassDetailPageProps {
  classId: string;
}

export default function MnftClassDetailPage({ classId: routeClassId }: MnftClassDetailPageProps) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const classId = normalizeAssetId(routeClassId);
  const tabFromQuery = searchParams.get('tab');
  const [capacityRange, setCapacityRange] = useState<CapacityRangeKey>('all');
  const [activeCollectionTab, setActiveCollectionTab] = useState<CollectionSectionTab>(() =>
    isCollectionSectionTab(tabFromQuery) ? tabFromQuery : 'activities'
  );
  const capacityRangeParams = getCapacityRangeParams(capacityRange);

  const itemsPagination = useCursorPagination();
  const holdersPagination = useCursorPagination();
  const activitiesPagination = useCursorPagination();
  const { reset: resetHoldersPagination } = holdersPagination;
  const { reset: resetActivitiesPagination } = activitiesPagination;

  const {
    data: collection,
    isLoading: collectionLoading,
    error: collectionError,
  } = useQuery({
    queryKey: ['mnft-class', classId],
    queryFn: () => api.getObjectCollection(classId),
    retry: false,
  });

  const { data: creatorAddressRecord } = useQuery({
    queryKey: ['address-by-lock-hash', collection?.ownerLockHash],
    queryFn: () => api.getAddress(collection!.ownerLockHash!),
    enabled: !!collection?.ownerLockHash,
    retry: false,
  });

  const { data: collectionItems, isLoading: isItemsLoading } = useQuery({
    queryKey: ['mnft-class-items', classId, itemsPagination.cursor],
    queryFn: () =>
      api.getObjectCollectionItems(classId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: itemsPagination.cursor,
      }),
    enabled: !!collection && activeCollectionTab === 'objects',
    placeholderData: keepPreviousData,
  });

  const {
    data: collectionHolders,
    isLoading: isHoldersLoading,
    isError: isHoldersError,
  } = useQuery({
    queryKey: ['mnft-class-holders', classId, holdersPagination.cursor],
    queryFn: () =>
      api.getObjectCollectionHolders(classId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: holdersPagination.cursor,
      }),
    enabled: !!collection && activeCollectionTab === 'holders',
    placeholderData: keepPreviousData,
  });

  const {
    data: collectionActivities,
    isLoading: isActivitiesLoading,
    isError: isActivitiesError,
  } = useQuery({
    queryKey: ['mnft-class-activities', classId, activitiesPagination.cursor],
    queryFn: () =>
      api.getObjectCollectionActivities(classId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: activitiesPagination.cursor,
      }),
    enabled: !!collection && activeCollectionTab === 'activities',
    placeholderData: keepPreviousData,
  });

  const { data: capacityChart, isLoading: isCapacityChartLoading } = useQuery({
    queryKey: ['mnft-class-capacity-chart', classId, capacityRange],
    queryFn: () =>
      capacityRangeParams
        ? api.getObjectCollectionCapacityChart(classId, capacityRangeParams)
        : api.getObjectCollectionCapacityChart(classId),
    enabled: !!collection,
  });

  useEffect(() => {
    const currentQuery = searchParams.toString();
    const nextParams = new URLSearchParams(currentQuery);
    if (activeCollectionTab === 'activities') {
      nextParams.delete('tab');
    } else {
      nextParams.set('tab', activeCollectionTab);
    }
    const nextQuery = nextParams.toString();
    if (nextQuery === currentQuery) return;
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  }, [activeCollectionTab, pathname, router, searchParams]);

  useEffect(() => {
    resetHoldersPagination();
    resetActivitiesPagination();
  }, [classId, resetActivitiesPagination, resetHoldersPagination]);

  const creatorAddress = creatorAddressRecord?.address || null;
  const classDetail = collection?.classDetail;
  const issuerDetail = collection?.issuerDetail;

  if (collectionLoading) {
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

  if (collectionError || !collection) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">mNFT Class not found</h2>
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
            href="/inventory/objects"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            &larr; Back to Objects
          </Link>
        </div>

        {/* Collection Overview — mirrors cluster page layout */}
        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Collection Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            <div className="flex flex-wrap items-center gap-3">
              <h1 className="text-text-bright font-mono text-2xl font-bold">
                {collection.name || 'Unnamed Collection'}
              </h1>
              <Badge variant="neutral">mNFT Class</Badge>
            </div>

            <div className="mt-3 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">class id</span>
              <HexDisplay value={collection.collectionId} truncate={false} size="sm" />
            </div>

            {(creatorAddress || collection.ownerLockHash) && (
              <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">creator</span>
                {creatorAddress ? (
                  <Address address={creatorAddress} truncate={false} />
                ) : (
                  <Link href={`/address/${collection.ownerLockHash}`} className="hover:underline">
                    <HexDisplay value={collection.ownerLockHash!} truncate={false} size="sm" />
                  </Link>
                )}
              </div>
            )}

            {/* Stat cards row */}
            <div className="border-base-border mt-4 grid grid-cols-2 gap-3 border-t pt-4 sm:grid-cols-4">
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Supply
                </div>
                <div className="text-warning font-mono text-sm font-semibold tabular-nums">
                  {formatNumber(collection.totalCount)}
                </div>
              </div>

              {collection.liveCount !== collection.totalCount && (
                <div className="border-base-border rounded border p-3">
                  <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                    Live
                  </div>
                  <div className="text-text-bright font-mono text-sm font-semibold tabular-nums">
                    {formatNumber(collection.liveCount)}
                  </div>
                </div>
              )}

              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Holders
                </div>
                <div className="text-text-bright font-mono text-sm font-semibold tabular-nums">
                  {formatNumber(collection.holdersCount)}
                </div>
              </div>

              {collection.createdAtBlock !== undefined && (
                <div className="border-base-border rounded border p-3">
                  <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                    Created
                  </div>
                  <Link
                    href={`/blocks/${collection.createdAtBlock}`}
                    className="text-text-bright font-mono text-sm font-semibold tabular-nums hover:underline"
                  >
                    #{formatNumber(collection.createdAtBlock)}
                  </Link>
                </div>
              )}
            </div>

            {/* Class description */}
            {classDetail?.description && (
              <div className="border-base-border mt-3 border-t pt-3">
                <div className="text-text font-mono text-sm">{classDetail.description}</div>
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>

        {/* Capacity Statistics */}
        <CapacityStatisticsSection
          className="mb-6"
          capacityRange={capacityRange}
          onCapacityRangeChange={setCapacityRange}
          capacityChart={capacityChart}
          isCapacityChartLoading={isCapacityChartLoading}
          totalCapacity={collection.ownedCapacity}
          commonKnowledgeSize={collection.ownedKnowledge}
        />

        {/* Class Context — analogous to DOB Blueprint in cluster page */}
        {(classDetail || issuerDetail) && (
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">Class Context</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="flex flex-wrap gap-x-6 gap-y-2 font-mono text-sm">
                {classDetail && (
                  <>
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim text-xs uppercase tracking-wider">
                        issued / total
                      </span>
                      <span className="text-text-bright tabular-nums">
                        {formatNumber(classDetail.issued)} / {formatNumber(classDetail.total)}
                      </span>
                    </div>
                    {classDetail.renderer && (
                      <div className="flex items-center gap-2">
                        <span className="text-text-dim text-xs uppercase tracking-wider">
                          renderer
                        </span>
                        <span className="text-text-bright">{classDetail.renderer}</span>
                      </div>
                    )}
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim text-xs uppercase tracking-wider">
                        configure
                      </span>
                      <span className="text-text-bright">
                        {decodeClassConfigure(classDetail.configure)}
                      </span>
                    </div>
                  </>
                )}
              </div>

              {issuerDetail && (
                <div className="border-base-border mt-4 border-t pt-4">
                  <div className="text-text-dim mb-2 font-mono text-xs uppercase tracking-wider">
                    Issuer
                  </div>
                  <div className="flex flex-wrap gap-x-6 gap-y-2 font-mono text-sm">
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim text-xs uppercase tracking-wider">name</span>
                      <span className="text-text-bright">
                        {issuerDetail.name || 'Unnamed Issuer'}
                      </span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim text-xs uppercase tracking-wider">
                        classes
                      </span>
                      <span className="text-text-bright tabular-nums">
                        {formatNumber(issuerDetail.classCount)}
                      </span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim text-xs uppercase tracking-wider">sets</span>
                      <span className="text-text-bright tabular-nums">
                        {formatNumber(issuerDetail.setCount)}
                      </span>
                    </div>
                  </div>
                  <div className="mt-2">
                    <span className="text-text-dim font-mono text-xs uppercase tracking-wider">
                      issuer id
                    </span>
                    <div className="mt-1">
                      <HexDisplay value={issuerDetail.issuerId} truncate={false} size="sm" />
                    </div>
                  </div>
                </div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {/* Tabs: Activities / Objects / Holders */}
        <TerminalPanel>
          <Tabs
            value={activeCollectionTab}
            onValueChange={(v) => {
              if (isCollectionSectionTab(v)) setActiveCollectionTab(v);
            }}
          >
            <TerminalPanelHeader
              indicator="active"
              actions={
                <TabsList className="border-b-0">
                  <TabsTrigger value="activities">
                    Activities ({formatNumber(collection.activitiesCount)})
                  </TabsTrigger>
                  <TabsTrigger value="objects">
                    Objects ({formatNumber(collection.totalCount)})
                  </TabsTrigger>
                  <TabsTrigger value="holders">
                    Holders ({formatNumber(collection.holdersCount)})
                  </TabsTrigger>
                </TabsList>
              }
            >
              {activeCollectionTab === 'activities'
                ? 'Activities'
                : activeCollectionTab === 'holders'
                  ? 'Holders'
                  : 'Objects'}
            </TerminalPanelHeader>

            {/* Activities Tab */}
            <TabsContent value="activities" className="py-0">
              <TerminalPanelContent>
                {isActivitiesLoading && !collectionActivities ? (
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

            {/* Objects Tab — TABLE layout matching cluster page */}
            <TabsContent value="objects" className="py-0">
              <TerminalPanelContent padding="none">
                {isItemsLoading ? (
                  <div className="text-text-dim py-8 text-center">Loading objects...</div>
                ) : !collectionItems?.data?.length ? (
                  <div className="text-text-dim py-8 text-center">
                    No objects in this collection
                  </div>
                ) : (
                  <>
                    <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:block">
                      <div className="grid grid-cols-[minmax(0,2fr)_minmax(0,1.5fr)_80px_minmax(0,1.2fr)_110px] items-center gap-3">
                        <div>Token ID</div>
                        <div>Name</div>
                        <div className="text-center">Status</div>
                        <div className="text-right">Owner</div>
                        <div className="text-right">Block</div>
                      </div>
                    </div>
                    {collectionItems.data.map((item) => (
                      <TerminalRow key={item.nftId}>
                        {/* Desktop layout */}
                        <div className="hidden md:grid md:grid-cols-[minmax(0,2fr)_minmax(0,1.5fr)_80px_minmax(0,1.2fr)_110px] md:items-center md:gap-3">
                          <div>
                            <Link href={`/objects/mnft/${item.nftId}`} className="hover:underline">
                              <HexDisplay value={item.nftId} size="sm" />
                            </Link>
                          </div>
                          <div className="text-text truncate font-mono text-xs">
                            {item.name || '\u2014'}
                          </div>
                          <div className="text-center">
                            {item.isLive ? (
                              <Badge variant="green">Live</Badge>
                            ) : (
                              <Badge variant="red">Burned</Badge>
                            )}
                          </div>
                          <div className="text-right">
                            {item.ownerLockHash ? (
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
                            ) : (
                              <span className="text-text-dim font-mono text-xs">{'\u2014'}</span>
                            )}
                          </div>
                          <div className="text-right">
                            <Link
                              href={`/blocks/${item.createdAtBlock}`}
                              className="text-emphasis font-mono text-xs hover:underline"
                            >
                              #{formatNumber(item.createdAtBlock)}
                            </Link>
                          </div>
                        </div>
                        {/* Mobile layout */}
                        <div className="space-y-2 md:hidden">
                          <div className="flex items-start justify-between gap-3">
                            <Link href={`/objects/mnft/${item.nftId}`} className="hover:underline">
                              <HexDisplay value={item.nftId} size="sm" />
                            </Link>
                            {item.isLive ? (
                              <Badge variant="green">Live</Badge>
                            ) : (
                              <Badge variant="red">Burned</Badge>
                            )}
                          </div>
                          {item.name && (
                            <div className="text-text font-mono text-xs">{item.name}</div>
                          )}
                          <div className="flex items-center justify-between gap-3 text-xs">
                            <span className="text-text-dim font-mono">
                              Block #{formatNumber(item.createdAtBlock)}
                            </span>
                            {item.ownerLockHash && (
                              <Link
                                href={`/address/${item.ownerLockHash}`}
                                className="hover:underline"
                              >
                                <HexDisplay
                                  value={item.ownerLockHash}
                                  size="sm"
                                  startChars={8}
                                  endChars={6}
                                />
                              </Link>
                            )}
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                  </>
                )}
              </TerminalPanelContent>
              {collectionItems && (collectionItems.data?.length ?? 0) > 0 && (
                <TerminalPanelFooter>
                  <CursorPagination
                    total={collection.totalCount}
                    totalLabel="Objects"
                    pageSize={DEFAULT_PAGE_SIZE}
                    page={itemsPagination.page}
                    currentCount={collectionItems.data?.length ?? 0}
                    hasMore={collectionItems.hasMore}
                    hasPrevious={itemsPagination.hasPrevious}
                    onNext={() => itemsPagination.goToNext(collectionItems.nextCursor)}
                    onPrevious={itemsPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              )}
            </TabsContent>

            {/* Holders Tab */}
            <TabsContent value="holders" className="py-0">
              <TerminalPanelContent>
                {isHoldersLoading && !collectionHolders ? (
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
