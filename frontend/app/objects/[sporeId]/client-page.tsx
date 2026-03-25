'use client';
import { useEffect, useMemo, useState } from 'react';
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
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CapacityStatisticsSection } from '@/components/ui/capacity-statistics-section';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { ObjectActivityCard } from '@/components/object/object-activity-card';
import {
  compositionTierCardStyle,
  CompositionTierTooltip,
  previewPanelStyle,
} from '@/components/object/storage-tier';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
const DOTBIT_COLLECTION_ID = '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f';
const DID_CKB_COLLECTION_ID = '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f';

function isDotbitAlias(assetId: string): boolean {
  const normalized = assetId.toLowerCase();
  return normalized === 'dotbit' || normalized === '.bit' || normalized === DOTBIT_COLLECTION_ID;
}

function isDidCkbAlias(assetId: string): boolean {
  const normalized = assetId.toLowerCase();
  return (
    normalized === 'did:ckb' || normalized === 'did_ckb' || normalized === DID_CKB_COLLECTION_ID
  );
}

function normalizeObjectAssetId(assetId: string): string {
  if (isDotbitAlias(assetId)) return DOTBIT_COLLECTION_ID;
  if (isDidCkbAlias(assetId)) return DID_CKB_COLLECTION_ID;
  return assetId;
}
import { formatNumber, truncateHash } from '@/lib/utils';
import { formatActivityTimestamp, formatCompositionTier } from '@/lib/asset-utils';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
import { decodeDobContent, extractSporePayload, type SporePayload } from '@/lib/dob-render';
import { detectPreview } from '@/lib/preview-utils';
import { SporePreview, type PreviewPhysicality } from '@/components/object/spore-preview';
type CollectionSectionTab = 'activities' | 'objects' | 'holders';
function isCollectionSectionTab(value: string | null): value is CollectionSectionTab {
  return value === 'activities' || value === 'objects' || value === 'holders';
}
function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && error.message.includes('404');
}
export interface SporeDetailPageProps {
  sporeId: string;
}
export default function SporeDetailPage({ sporeId }: SporeDetailPageProps) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const rawAssetId = sporeId;
  const tabFromQuery = searchParams.get('tab');
  const [capacityRange, setCapacityRange] = useState<CapacityRangeKey>('all');
  const [hoveredByteOffset, setHoveredByteOffset] = useState<number | null>(null);
  const [searchInput, setSearchInput] = useState('');
  const [searchKeyword, setSearchKeyword] = useState('');
  const [collectionStatusSelection, setCollectionStatusSelection] =
    useState<ItemStatusFilter>('all');
  const [activeCollectionTab, setActiveCollectionTab] = useState<CollectionSectionTab>(() =>
    isCollectionSectionTab(tabFromQuery) ? tabFromQuery : 'activities'
  );
  const collectionItemsPagination = useCursorPagination();
  const collectionHoldersPagination = useCursorPagination();
  const collectionActivitiesPagination = useCursorPagination();
  const { reset: resetCollectionItemsPagination } = collectionItemsPagination;
  const { reset: resetCollectionHoldersPagination } = collectionHoldersPagination;
  const { reset: resetCollectionActivitiesPagination } = collectionActivitiesPagination;
  const capacityRangeParams = getCapacityRangeParams(capacityRange);
  const isDotbitCollection = isDotbitAlias(rawAssetId);
  const isDidCkbCollection = isDidCkbAlias(rawAssetId);
  const assetId = normalizeObjectAssetId(rawAssetId);
  // Redirect identity collection aliases to /identities/ routes
  useEffect(() => {
    if (isDotbitCollection) {
      router.replace('/identities/dotbit');
    } else if (isDidCkbCollection) {
      router.replace('/identities/did:ckb');
    }
  }, [isDotbitCollection, isDidCkbCollection, router]);
  if (isDotbitCollection || isDidCkbCollection) {
    return null; // redirecting
  }
  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setSearchKeyword(searchInput.trim());
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [searchInput]);
  const sporeQuery = useQuery({
    queryKey: ['spore', rawAssetId],
    queryFn: () => api.getSporeObject(assetId),
    enabled: !isDotbitCollection && !isDidCkbCollection,
    retry: false,
  });
  const spore = sporeQuery.data;
  const shouldQueryCollection =
    isDotbitCollection || isDidCkbCollection || (!spore && isNotFoundError(sporeQuery.error));
  const collectionQuery = useQuery({
    queryKey: ['object-collection', assetId],
    queryFn: () => api.getObjectCollection(assetId),
    enabled: shouldQueryCollection,
    retry: false,
  });
  const collection = collectionQuery.data;
  const isMnftCollection = !!collection && collection.standard.toLowerCase() === 'm-nft';
  const collectionAssetId = collection?.collectionId ?? assetId;
  const isDotbitCollectionView =
    isDotbitCollection ||
    (!!collection &&
      (isDotbitAlias(collection.collectionId) || collection.standard.toLowerCase() === 'dotbit'));
  const isDidCkbCollectionView =
    isDidCkbCollection ||
    (!!collection &&
      (isDidCkbAlias(collection.collectionId) ||
        collection.standard.toLowerCase() === 'did_ckb' ||
        collection.standard.toLowerCase() === 'did:ckb'));
  const supportsCollectionFilters = isDotbitCollectionView || isDidCkbCollectionView;
  const collectionSearchKeyword = supportsCollectionFilters ? searchKeyword : '';
  const collectionStatusFilter = supportsCollectionFilters ? collectionStatusSelection : 'all';
  const collectionSearchLabel = isDotbitCollectionView ? 'Search .bit' : 'Search did:ckb';
  const collectionInactiveStatusLabel =
    isDotbitCollectionView || isDidCkbCollectionView ? 'Recycled' : 'Burned';
  const { data: cluster } = useQuery({
    queryKey: ['cluster', spore?.clusterId],
    queryFn: () => api.getSporeCluster(spore!.clusterId!),
    enabled: !!spore?.clusterId,
  });
  const { data: ownerAddressRecord } = useQuery({
    queryKey: ['address-by-lock-hash', spore?.ownerLockHash],
    queryFn: () => api.getAddress(spore!.ownerLockHash),
    enabled: !!spore?.ownerLockHash && !spore?.ownerAddress,
    retry: false,
  });
  const { data: decodedDobByApi } = useQuery({
    queryKey: ['spore-dob-decoded', assetId],
    queryFn: () => api.getSporeObjectDecoded(assetId),
    enabled: !!spore && spore.contentType.toLowerCase().startsWith('dob/'),
    retry: false,
  });
  const { data: sporeTxDetail } = useQuery({
    queryKey: ['spore-tx-detail', spore?.txHash],
    queryFn: () => api.getTransactionDetail(spore!.txHash),
    enabled: !!spore?.txHash,
  });
  const resolvedSporeOutputIndex = useMemo(() => {
    if (!spore) {
      return null;
    }
    const fallback = Number.isInteger(spore.outputIndex) ? spore.outputIndex : null;
    const outputs = sporeTxDetail?.outputs;
    if (!outputs || outputs.length === 0) {
      return fallback;
    }
    const normalizedSporeId = spore.sporeId.toLowerCase();
    const exactIndex = outputs.findIndex(
      (output) => output.type?.args?.toLowerCase() === normalizedSporeId
    );
    if (exactIndex >= 0) {
      return exactIndex;
    }
    return fallback;
  }, [spore, sporeTxDetail]);
  const { data: sporeCell } = useQuery({
    queryKey: ['spore-cell-preview', spore?.txHash, resolvedSporeOutputIndex],
    queryFn: () => api.getCell(spore!.txHash, resolvedSporeOutputIndex!),
    enabled: !!spore?.txHash && resolvedSporeOutputIndex !== null && resolvedSporeOutputIndex >= 0,
    retry: false,
  });
  const { data: collectionCapacityChart, isLoading: isCollectionCapacityChartLoading } = useQuery({
    queryKey: ['object-collection-capacity-chart', collectionAssetId, capacityRange],
    queryFn: () =>
      capacityRangeParams
        ? api.getObjectCollectionCapacityChart(collectionAssetId, capacityRangeParams)
        : api.getObjectCollectionCapacityChart(collectionAssetId),
    enabled: !!collection,
  });
  const {
    data: collectionItems,
    isLoading: isCollectionItemsLoading,
    isFetching: isCollectionItemsFetching,
    isError: isCollectionItemsError,
  } = useQuery({
    queryKey: [
      'object-collection-items',
      collectionAssetId,
      collectionItemsPagination.cursor,
      collectionSearchKeyword,
      collectionStatusFilter,
    ],
    queryFn: () =>
      api.getObjectCollectionItems(collectionAssetId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: collectionItemsPagination.cursor,
        search: collectionSearchKeyword || undefined,
        status: supportsCollectionFilters ? collectionStatusFilter : undefined,
      }),
    enabled: !!collection,
    placeholderData: keepPreviousData,
  });
  const {
    data: collectionHolders,
    isLoading: isCollectionHoldersLoading,
    isError: isCollectionHoldersError,
  } = useQuery({
    queryKey: ['object-collection-holders', collectionAssetId, collectionHoldersPagination.cursor],
    queryFn: () =>
      api.getObjectCollectionHolders(collectionAssetId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: collectionHoldersPagination.cursor,
      }),
    enabled: !!collection && activeCollectionTab === 'holders',
    placeholderData: keepPreviousData,
  });
  const {
    data: collectionActivities,
    isLoading: isCollectionActivitiesLoading,
    isError: isCollectionActivitiesError,
  } = useQuery({
    queryKey: [
      'object-collection-activities',
      collectionAssetId,
      collectionActivitiesPagination.cursor,
    ],
    queryFn: () =>
      api.getObjectCollectionActivities(collectionAssetId, {
        limit: DEFAULT_PAGE_SIZE,
        cursor: collectionActivitiesPagination.cursor,
      }),
    enabled: !!collection && activeCollectionTab === 'activities',
    placeholderData: keepPreviousData,
  });
  useEffect(() => {
    resetCollectionItemsPagination();
  }, [
    collectionAssetId,
    collectionSearchKeyword,
    collectionStatusFilter,
    resetCollectionItemsPagination,
  ]);
  useEffect(() => {
    resetCollectionHoldersPagination();
    resetCollectionActivitiesPagination();
  }, [collectionAssetId, resetCollectionActivitiesPagination, resetCollectionHoldersPagination]);
  const updateSearchParams = (mutator: (nextParams: URLSearchParams) => void) => {
    const nextParams = new URLSearchParams(searchParams.toString());
    mutator(nextParams);
    const nextQuery = nextParams.toString();
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  };
  const handleCollectionTabChange = (nextValue: string) => {
    if (!isCollectionSectionTab(nextValue)) return;
    setActiveCollectionTab(nextValue);
    updateSearchParams((nextParams) => {
      if (nextValue === 'activities') {
        nextParams.delete('tab');
      } else {
        nextParams.set('tab', nextValue);
      }
    });
  };
  const sporePayload = useMemo(() => extractSporePayload(sporeCell), [sporeCell]);
  const dobContent = useMemo(() => {
    if (decodedDobByApi) {
      return {
        dnaHex: decodedDobByApi.dnaHex,
        traits: decodedDobByApi.traits,
        media: decodedDobByApi.media ?? [],
        issues: decodedDobByApi.issues,
      };
    }
    if (!spore) {
      return null;
    }
    const local = decodeDobContent({
      sporeContentType: spore.contentType,
      contentText: sporePayload?.textContent,
      clusterDescription: cluster?.description,
    });
    return local ? { ...local, media: [] } : null;
  }, [cluster?.description, decodedDobByApi, spore, sporePayload?.textContent]);
  const preview = useMemo(
    () =>
      detectPreview(
        spore?.contentType ?? '',
        sporePayload?.contentBytes,
        dobContent?.media?.map((m) => ({ mediaType: m.mediaType, url: m.url, step: m.step }))
      ),
    [spore?.contentType, sporePayload?.contentBytes, dobContent?.media]
  );
  useEffect(() => {
    if (isMnftCollection && collection) {
      router.replace(`/classes/${collection.collectionId}`);
    }
  }, [isMnftCollection, collection, router]);
  const isPageLoading =
    sporeQuery.isLoading || (shouldQueryCollection && collectionQuery.isLoading);
  const hasTerminalError =
    (!spore && !collection && !isPageLoading && !shouldQueryCollection) ||
    (!spore && shouldQueryCollection && collectionQuery.isError);
  if (isPageLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-48 animate-pulse rounded" />
          <div className="grid gap-6 lg:grid-cols-3">
            <div className="border-base-border bg-base-surface/50 h-64 animate-pulse rounded border" />
            <div className="border-base-border bg-base-surface/50 h-64 animate-pulse rounded border lg:col-span-2" />
          </div>
        </main>
      </div>
    );
  }
  if (hasTerminalError) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">Asset not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  if (collection) {
    if (isMnftCollection) {
      return null; // redirecting to /classes/[classId]
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
              ← Back to Objects
            </Link>
          </div>

          {/* Unified Collection Overview */}
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">Collection Overview</TerminalPanelHeader>
            <TerminalPanelContent>
              {/* Name + badge */}
              <div className="flex flex-wrap items-center gap-3">
                <h1 className="text-text-bright font-mono text-2xl font-bold">
                  {collection.name || 'Object Collection'}
                </h1>
                <Badge variant="neutral">{collection.standard.toUpperCase()}</Badge>
              </div>

              {/* Collection ID */}
              <div className="mt-3 flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">
                  collection id
                </span>
                <HexDisplay value={collection.collectionId} truncate={false} size="sm" />
              </div>

              {/* Stat cards row */}
              <div className="border-base-border mt-4 grid grid-cols-2 gap-3 border-t pt-4 sm:grid-cols-4">
                {/* Composition card (color-coded) */}
                {collection.composition?.tier &&
                  (() => {
                    const style = compositionTierCardStyle(collection.composition.tier);
                    return (
                      <div className={style.card}>
                        <div
                          className={`mb-1.5 font-mono text-[10px] uppercase tracking-wider ${style.label}`}
                        >
                          Composition
                        </div>
                        <div className="flex items-center gap-1">
                          <span
                            className={`font-mono text-sm font-semibold leading-tight ${style.text}`}
                          >
                            {formatCompositionTier(collection.composition.tier)}
                          </span>
                          <CompositionTierTooltip
                            tier={collection.composition.tier}
                            buttonClassName={style.tooltipButton}
                          />
                        </div>
                        {collection.composition.fullyOnchainRatio && (
                          <div className="text-text-dim mt-1 font-mono text-xs">
                            On-chain:{' '}
                            {(Number(collection.composition.fullyOnchainRatio) * 100).toFixed(1)}%
                          </div>
                        )}
                      </div>
                    );
                  })()}

                {/* Supply card */}
                <div className="border-base-border rounded border p-3">
                  <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                    Supply
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
              </div>
            </TerminalPanelContent>
          </TerminalPanel>

          <CapacityStatisticsSection
            className="mb-6"
            capacityRange={capacityRange}
            onCapacityRangeChange={setCapacityRange}
            capacityChart={collectionCapacityChart}
            isCapacityChartLoading={isCollectionCapacityChartLoading}
            totalCapacity={collection.ownedCapacity}
            commonKnowledgeSize={collection.ownedKnowledge}
            totalCapacityLabel="Owned Capacity"
          />

          <TerminalPanel>
            <Tabs value={activeCollectionTab} onValueChange={handleCollectionTabChange}>
              <TerminalPanelHeader
                indicator="active"
                actions={
                  <div className="flex flex-wrap items-center gap-3">
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
                    {activeCollectionTab === 'objects' && supportsCollectionFilters && (
                      <div className="flex items-center gap-2">
                        <select
                          value={collectionStatusFilter}
                          onChange={(event) =>
                            setCollectionStatusSelection(event.target.value as ItemStatusFilter)
                          }
                          aria-label="Status Filter"
                          className="focus:border-emphasis border-base-border bg-base-surface text-text-bright rounded border px-2.5 py-1.5 font-mono text-xs outline-none transition-colors"
                        >
                          <option value="all">All</option>
                          <option value="live">Live</option>
                          <option value="recycled">{collectionInactiveStatusLabel}</option>
                        </select>
                        <input
                          type="text"
                          value={searchInput}
                          onChange={(event) => setSearchInput(event.target.value)}
                          placeholder={collectionSearchLabel}
                          aria-label={collectionSearchLabel}
                          className="focus:border-emphasis border-base-border bg-base-surface text-text-bright placeholder:text-text-dim w-44 rounded border px-2.5 py-1.5 font-mono text-xs outline-none transition-colors"
                        />
                        {isCollectionItemsFetching && (
                          <span className="text-text-dim font-mono text-xs">Searching...</span>
                        )}
                      </div>
                    )}
                  </div>
                }
              >
                {activeCollectionTab === 'activities'
                  ? 'Activities'
                  : activeCollectionTab === 'holders'
                    ? 'Holders'
                    : 'Objects'}
              </TerminalPanelHeader>
              <TabsContent value="activities" className="py-0">
                <TerminalPanelContent>
                  {isCollectionActivitiesLoading ? (
                    <div className="text-text-dim py-8 text-center">Loading activities...</div>
                  ) : isCollectionActivitiesError ? (
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
                    page={collectionActivitiesPagination.page}
                    currentCount={collectionActivities?.data?.length ?? 0}
                    hasMore={collectionActivities?.hasMore ?? false}
                    hasPrevious={collectionActivitiesPagination.hasPrevious}
                    onNext={() =>
                      collectionActivitiesPagination.goToNext(collectionActivities?.nextCursor)
                    }
                    onPrevious={collectionActivitiesPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              </TabsContent>
              <TabsContent value="objects" className="py-0">
                <TerminalPanelContent>
                  {isDotbitCollectionView ? (
                    isCollectionItemsLoading ? (
                      <div className="text-text-dim py-8 text-center">Loading Objects...</div>
                    ) : isCollectionItemsError ? (
                      <div className="text-rouge py-8 text-center">
                        Failed to load Objects. Please refresh and try again.
                      </div>
                    ) : !collectionItems?.data?.length ? (
                      <div className="text-text-dim py-8 text-center">
                        No Objects in this collection
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
                                href={`/identities/dotbit/${encodeURIComponent(item.nftId)}`}
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
                    )
                  ) : isCollectionItemsLoading || isCollectionItemsFetching ? (
                    <div className="text-text-dim py-8 text-center">Loading Objects...</div>
                  ) : isCollectionItemsError ? (
                    <div className="text-rouge py-8 text-center">
                      Failed to load Objects. Please refresh and try again.
                    </div>
                  ) : !collectionItems?.data?.length ? (
                    <div className="text-text-dim py-8 text-center">
                      No Objects in this collection
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {collectionItems.data.map((item) => (
                        <div
                          key={item.nftId}
                          className="border-base-border bg-base-surface/40 flex flex-col gap-2 rounded border p-3"
                        >
                          <div className="flex items-center justify-between gap-3">
                            {item.standard.toLowerCase() === 'm-nft' ? (
                              <Link
                                href={`/objects/mnft/${item.nftId}`}
                                className="hover:text-emphasis text-text-bright font-mono text-sm hover:underline"
                              >
                                {item.name || item.nftId}
                              </Link>
                            ) : item.standard.toLowerCase() === 'did_ckb' ||
                              item.standard.toLowerCase() === 'did:ckb' ? (
                              <Link
                                href={`/identities/did/${encodeURIComponent(item.nftId)}`}
                                className="hover:text-emphasis text-text-bright font-mono text-sm hover:underline"
                              >
                                {item.name || item.nftId}
                              </Link>
                            ) : (
                              <div className="text-text-bright font-mono text-sm">
                                {item.name || item.nftId}
                              </div>
                            )}
                            {item.isLive ? (
                              <Badge variant="green">Live</Badge>
                            ) : (
                              <Badge variant="red">
                                {item.standard.toLowerCase() === 'did_ckb' ||
                                item.standard.toLowerCase() === 'did:ckb'
                                  ? 'Recycled'
                                  : 'Burned'}
                              </Badge>
                            )}
                          </div>
                          {item.standard.toLowerCase() === 'm-nft' ? (
                            <Link href={`/objects/mnft/${item.nftId}`} className="hover:underline">
                              <HexDisplay value={item.nftId} size="sm" />
                            </Link>
                          ) : item.standard.toLowerCase() === 'did_ckb' ||
                            item.standard.toLowerCase() === 'did:ckb' ? (
                            <Link
                              href={`/identities/did/${encodeURIComponent(item.nftId)}`}
                              className="hover:underline"
                            >
                              <HexDisplay value={item.nftId} size="sm" />
                            </Link>
                          ) : (
                            <HexDisplay value={item.nftId} size="sm" />
                          )}
                          <div className="text-text-dim font-mono text-xs">
                            Created at block #{formatNumber(item.createdAtBlock)}
                          </div>
                          {item.ownerLockHash && (
                            <div className="text-text-dim font-mono text-xs">
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
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                </TerminalPanelContent>
                <TerminalPanelFooter>
                  <CursorPagination
                    total={collectionItems?.total ?? undefined}
                    totalLabel="Objects"
                    pageSize={DEFAULT_PAGE_SIZE}
                    page={collectionItemsPagination.page}
                    hasMore={collectionItems?.hasMore ?? false}
                    hasPrevious={collectionItemsPagination.hasPrevious}
                    onNext={() => collectionItemsPagination.goToNext(collectionItems?.nextCursor)}
                    onPrevious={collectionItemsPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              </TabsContent>
              <TabsContent value="holders" className="py-0">
                <TerminalPanelContent>
                  {isCollectionHoldersLoading ? (
                    <div className="text-text-dim py-8 text-center">Loading holders...</div>
                  ) : isCollectionHoldersError ? (
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
                    page={collectionHoldersPagination.page}
                    currentCount={collectionHolders?.data?.length ?? 0}
                    hasMore={collectionHolders?.hasMore ?? false}
                    hasPrevious={collectionHoldersPagination.hasPrevious}
                    onNext={() =>
                      collectionHoldersPagination.goToNext(collectionHolders?.nextCursor)
                    }
                    onPrevious={collectionHoldersPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              </TabsContent>
            </Tabs>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  if (!spore) {
    return null;
  }
  const resolvedOwnerAddress = spore.ownerAddress || ownerAddressRecord?.address || null;
  const previewContentType = sporePayload?.contentType || spore.contentType;
  const previewBytes = sporePayload?.contentBytes.length ?? spore.contentSize;
  const hasDecodedTraits = (dobContent?.traits.length ?? 0) > 0;
  const shouldShowPayloadHexPanel = !!sporePayload?.contentHex;
  const sporeOutputIndex =
    resolvedSporeOutputIndex !== null && resolvedSporeOutputIndex >= 0
      ? resolvedSporeOutputIndex
      : spore.outputIndex;
  const hasCellLink = Number.isInteger(sporeOutputIndex);
  const previewStyle = previewPanelStyle(spore.mediaProfile?.tier);
  const previewPhysicality: PreviewPhysicality = (() => {
    const tier = spore.mediaProfile?.tier;
    if (tier === 'pure_ckb') return 'onchain';
    if (tier === 'btc_ckb') return 'onchain-btc';
    return 'default';
  })();

  const SporeContentPanel = ({
    sporePayload: payload,
    clusterId,
    className,
  }: {
    sporePayload: SporePayload;
    clusterId?: string | null;
    className?: string;
  }) => (
    <TerminalPanel className={className}>
      <TerminalPanelHeader indicator="active">Spore Content</TerminalPanelHeader>
      <TerminalPanelContent>
        <div className="space-y-2">
          <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
              Content Type
            </div>
            <div className="text-text-bright mt-1 break-all font-mono text-xs">
              {payload.contentType}
            </div>
          </div>
          <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
              Content Size
            </div>
            <div className="text-text-bright mt-1 font-mono text-xs">
              {formatNumber(payload.contentBytes.length)} bytes
            </div>
          </div>
          {clusterId && (
            <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
              <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                Cluster ID
              </div>
              <div className="mt-1">
                <Link
                  href={`/clusters/${clusterId}`}
                  className="text-text-bright font-mono text-xs hover:underline"
                >
                  <HexDisplay value={clusterId} truncate={false} size="sm" />
                </Link>
              </div>
            </div>
          )}
          {payload.textContent && (
            <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
              <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                Content (Text)
              </div>
              <pre className="text-text-bright mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all font-mono text-xs">
                {payload.textContent.length > 600
                  ? `${payload.textContent.slice(0, 600)}...`
                  : payload.textContent}
              </pre>
            </div>
          )}
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/inventory/objects"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            ← Back to Objects
          </Link>
        </div>

        {/* Unified Spore Overview */}
        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Spore Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            {/* Identity section */}
            <div>
              {/* Name + badge */}
              <div className="flex flex-wrap items-center gap-3">
                <h1 className="text-text-bright font-mono text-2xl font-bold">
                  {cluster?.name
                    ? `${cluster.name} (${truncateHash(spore.sporeId, 6, 4)})`
                    : `Spore Asset (${truncateHash(spore.sporeId, 6, 4)})`}
                </h1>
                {spore.isLive ? (
                  <Badge variant="green">Live</Badge>
                ) : (
                  <Badge variant="red">Burned</Badge>
                )}
              </div>

              {/* Spore ID */}
              <div className="mt-3 flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">spore id</span>
                <HexDisplay value={spore.sporeId} truncate={false} size="sm" />
              </div>

              {/* Cell */}
              {hasCellLink && (
                <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
                  <span className="text-text-dim text-xs uppercase tracking-wider">cell</span>
                  <Link
                    href={`/cell/${spore.txHash}-${sporeOutputIndex}`}
                    className="hover:underline"
                  >
                    <HexDisplay value={spore.txHash} size="sm" startChars={14} endChars={10} />
                    <span className="text-text-dim">-{sporeOutputIndex}</span>
                  </Link>
                </div>
              )}

              {/* Owner */}
              <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">owner</span>
                {resolvedOwnerAddress ? (
                  <Address address={resolvedOwnerAddress} truncate={false} />
                ) : (
                  <span className="text-text-dim">unavailable</span>
                )}
              </div>

              {/* Owner Lock Hash */}
              <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">lock hash</span>
                <Link href={`/address/${spore.ownerLockHash}`} className="hover:underline">
                  <HexDisplay value={spore.ownerLockHash} size="sm" startChars={14} endChars={10} />
                </Link>
              </div>
            </div>

            {/* Stat cards row */}
            <div className="border-base-border mt-4 grid grid-cols-2 gap-3 border-t pt-4 sm:grid-cols-4">
              {/* Object composition card (color-coded) */}
              {spore.mediaProfile?.tier &&
                (() => {
                  const style = compositionTierCardStyle(spore.mediaProfile.tier);
                  return (
                    <div className={style.card}>
                      <div
                        className={`mb-1.5 font-mono text-[10px] uppercase tracking-wider ${style.label}`}
                      >
                        Object Composition
                      </div>
                      <div className="flex items-center gap-1">
                        <span
                          className={`font-mono text-sm font-semibold leading-tight ${style.text}`}
                        >
                          {formatCompositionTier(spore.mediaProfile.tier)}
                        </span>
                        <CompositionTierTooltip
                          tier={spore.mediaProfile.tier}
                          buttonClassName={style.tooltipButton}
                        />
                      </div>
                    </div>
                  );
                })()}

              {/* Cluster card */}
              {cluster && (
                <div className="border-base-border rounded border p-3">
                  <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                    Cluster
                  </div>
                  <Link
                    href={`/clusters/${cluster.clusterId}`}
                    className="text-text-bright font-mono text-sm font-semibold hover:underline"
                  >
                    {cluster.name || 'Unnamed'}
                  </Link>
                  <div className="text-text-dim font-mono text-xs">
                    {formatNumber(cluster.sporesCount)} spores
                  </div>
                </div>
              )}

              {/* Content card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Content
                </div>
                <div className="text-text-bright font-mono text-sm font-semibold">
                  {previewContentType}
                </div>
                <div className="text-text-dim font-mono text-xs">
                  {formatNumber(previewBytes)} bytes
                </div>
              </div>

              {/* Created card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Created
                </div>
                <Link
                  href={`/blocks/${spore.createdAtBlock}`}
                  className="text-text-bright font-mono text-sm font-semibold tabular-nums hover:underline"
                >
                  #{formatNumber(spore.createdAtBlock)}
                </Link>
              </div>
            </div>

            {/* Owned Capacity bar */}
            <CapacityUtilization
              totalCapacity={spore.ownedCapacity ?? '0'}
              commonKnowledgeSize={spore.ownedKnowledge ?? '0'}
              totalLabel="Owned Capacity"
              className="mt-4"
            />
          </TerminalPanelContent>
        </TerminalPanel>

        {/* Side-by-side row: On-Chain Media paired with DOB Details or Spore Content */}
        {preview && (hasDecodedTraits || sporePayload) && (
          <div className="mb-6 grid gap-6 lg:grid-cols-2">
            <TerminalPanel className={previewStyle.panel}>
              <TerminalPanelHeader indicator="active" className={previewStyle.header}>
                <span className={previewStyle.headerText || undefined}>On-Chain Media</span>
              </TerminalPanelHeader>
              <TerminalPanelContent>
                <SporePreview preview={preview} physicality={previewPhysicality} />
                <div className="mt-4 space-y-2">
                  <div className="grid gap-2 sm:grid-cols-2">
                    <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
                      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                        Content Type
                      </div>
                      <div className="text-text-bright mt-1 break-all font-mono text-xs">
                        {previewContentType}
                      </div>
                    </div>
                    <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
                      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                        Size
                      </div>
                      <div className="text-text-bright mt-1 font-mono text-xs">
                        {formatNumber(previewBytes)} bytes
                      </div>
                    </div>
                  </div>
                  {dobContent?.media && dobContent.media.length > 0 && (
                    <div className="space-y-2">
                      {dobContent.media.map((m) => (
                        <div
                          key={m.hash}
                          className="border-base-border bg-base-surface/50 rounded border p-2.5"
                        >
                          <div className="flex items-baseline gap-2">
                            <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                              {m.role ?? `Step ${m.step ?? 0}`}
                            </span>
                            <span className="text-text font-mono text-[10px]">
                              {m.mediaType} · {formatNumber(m.size)} bytes
                            </span>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                  {(!dobContent?.media || dobContent.media.length === 0) &&
                    sporePayload?.textContent && (
                      <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
                        <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                          Payload
                        </div>
                        <pre className="text-text-bright mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-[10px] leading-relaxed">
                          {sporePayload.textContent.length > 500
                            ? `${sporePayload.textContent.slice(0, 500)}...`
                            : sporePayload.textContent}
                        </pre>
                      </div>
                    )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
            {hasDecodedTraits ? (
              <TerminalPanel>
                <TerminalPanelHeader indicator="active">
                  {previewContentType.toUpperCase()} Details
                </TerminalPanelHeader>
                <TerminalPanelContent>
                  {dobContent?.dnaHex && (
                    <div className="border-info/30 bg-info/10 mb-3 rounded border px-3 py-2">
                      <div className="text-info font-mono text-[10px] uppercase tracking-wider">
                        DNA
                      </div>
                      <div className="text-info-dim mt-1 break-all font-mono text-xs">
                        {dobContent.dnaHex}
                      </div>
                    </div>
                  )}
                  <div className="grid gap-2 sm:grid-cols-2">
                    {dobContent!.traits.map((trait) => (
                      <div
                        key={`${trait.name}-${trait.value}`}
                        className="border-base-border bg-base-surface/50 rounded border p-2.5"
                      >
                        <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                          {trait.name}
                        </div>
                        <div className="text-text-bright mt-1 break-all font-mono text-xs">
                          {trait.value}
                        </div>
                      </div>
                    ))}
                  </div>
                </TerminalPanelContent>
              </TerminalPanel>
            ) : (
              <SporeContentPanel sporePayload={sporePayload!} clusterId={spore.clusterId} />
            )}
          </div>
        )}

        {/* Preview only (no companion panel) */}
        {preview && !hasDecodedTraits && !sporePayload && (
          <TerminalPanel className={`mb-6 ${previewStyle.panel}`}>
            <TerminalPanelHeader indicator="active" className={previewStyle.header}>
              <span className={previewStyle.headerText || undefined}>On-Chain Media</span>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <SporePreview preview={preview} physicality={previewPhysicality} />
              <div className="mt-4 grid gap-2 sm:grid-cols-2">
                <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
                  <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                    Content Type
                  </div>
                  <div className="text-text-bright mt-1 break-all font-mono text-xs">
                    {previewContentType}
                  </div>
                </div>
                <div className="border-base-border bg-base-surface/50 rounded border p-2.5">
                  <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                    Size
                  </div>
                  <div className="text-text-bright mt-1 font-mono text-xs">
                    {formatNumber(previewBytes)} bytes
                  </div>
                </div>
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {/* DOB Details standalone (no preview available) */}
        {!preview && hasDecodedTraits && (
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">
              {previewContentType.toUpperCase()} Details
            </TerminalPanelHeader>
            <TerminalPanelContent>
              {dobContent?.dnaHex && (
                <div className="border-info/30 bg-info/10 mb-3 rounded border px-3 py-2">
                  <div className="text-info font-mono text-[10px] uppercase tracking-wider">
                    DNA
                  </div>
                  <div className="text-info-dim mt-1 break-all font-mono text-xs">
                    {dobContent.dnaHex}
                  </div>
                </div>
              )}
              <div className="grid gap-2 sm:grid-cols-2">
                {dobContent!.traits.map((trait) => (
                  <div
                    key={`${trait.name}-${trait.value}`}
                    className="border-base-border bg-base-surface/50 rounded border p-2.5"
                  >
                    <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                      {trait.name}
                    </div>
                    <div className="text-text-bright mt-1 break-all font-mono text-xs">
                      {trait.value}
                    </div>
                  </div>
                ))}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {/* Spore Content standalone (when not already paired in side-by-side with preview) */}
        {sporePayload && !(preview && !hasDecodedTraits) && (
          <SporeContentPanel
            sporePayload={sporePayload}
            clusterId={spore.clusterId}
            className="mb-6"
          />
        )}

        {/* Payload Hex View */}
        {shouldShowPayloadHexPanel && (
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">
              Payload Data ({formatNumber(sporePayload!.contentBytes.length)} bytes)
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="border-base-border bg-base-bg overflow-x-auto rounded-md border p-4 font-mono text-xs">
                <div className="min-w-max" onMouseLeave={() => setHoveredByteOffset(null)}>
                  {(() => {
                    const hex = sporePayload!.contentHex;
                    const bytes = sporePayload!.contentBytes;
                    const BYTES_PER_ROW = 16;
                    const MAX_BYTES = 512;
                    const totalBytes = bytes.length;
                    const displayBytes = Math.min(totalBytes, MAX_BYTES);
                    const rows = [];
                    for (let r = 0; r < displayBytes; r += BYTES_PER_ROW) {
                      const end = Math.min(r + BYTES_PER_ROW, displayBytes);
                      const rowBytes: { hex: string; ascii: string; offset: number }[] = [];
                      for (let i = r; i < end; i++) {
                        const h = hex.slice(i * 2, i * 2 + 2);
                        const code = bytes[i];
                        const ch = code >= 32 && code <= 126 ? String.fromCharCode(code) : '.';
                        rowBytes.push({ hex: h, ascii: ch, offset: i });
                      }
                      rows.push({ offset: r, bytes: rowBytes });
                    }
                    return (
                      <>
                        {rows.map((row) => {
                          const padCount = BYTES_PER_ROW - row.bytes.length;
                          return (
                            <div key={row.offset} className="hover:bg-base-elevated/50 flex py-0.5">
                              <span className="text-text-dim mr-4 select-none">
                                0x{row.offset.toString(16).padStart(4, '0')}:
                              </span>
                              <div className="text-emphasis-dim mr-6 flex gap-1.5">
                                {row.bytes.map((b) => (
                                  <span
                                    key={b.offset}
                                    className={
                                      hoveredByteOffset === b.offset
                                        ? 'bg-emphasis/25 text-emphasis ring-emphasis/70 rounded ring-1'
                                        : 'bg-base-elevated/70 text-text rounded'
                                    }
                                    onMouseEnter={() => setHoveredByteOffset(b.offset)}
                                  >
                                    {b.hex}
                                  </span>
                                ))}
                                {Array.from({ length: padCount }).map((_, i) => (
                                  <span key={`pad-${i}`} className="opacity-0">
                                    00
                                  </span>
                                ))}
                              </div>
                              <div className="border-base-border text-text-dim border-l pl-4">
                                {row.bytes.map((b) => (
                                  <span
                                    key={`a-${b.offset}`}
                                    className={`inline-flex w-2.5 justify-center ${
                                      hoveredByteOffset === b.offset
                                        ? 'bg-emphasis/20 text-emphasis rounded-sm'
                                        : ''
                                    }`}
                                    onMouseEnter={() => setHoveredByteOffset(b.offset)}
                                  >
                                    {b.ascii}
                                  </span>
                                ))}
                              </div>
                            </div>
                          );
                        })}
                        {totalBytes > MAX_BYTES && (
                          <div className="text-text-dim mt-2 select-none italic">
                            ... {(totalBytes - MAX_BYTES).toLocaleString()} more bytes
                          </div>
                        )}
                      </>
                    );
                  })()}
                </div>
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {/* Media Compositions */}
        {spore.mediaProfile && (
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">Media Compositions</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-4">
                {/* On-chain composition */}
                {(preview || spore.mediaProfile.tier !== 'unknown') && (
                  <div>
                    <div className="text-text-dim mb-2 font-mono text-[10px] uppercase tracking-wider">
                      On-Chain
                    </div>
                    <div className="border-base-border bg-base-surface/40 space-y-2 rounded border p-2.5">
                      <div className="grid gap-2 sm:grid-cols-3">
                        <div>
                          <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                            Type
                          </div>
                          <div className="text-text-bright mt-0.5 font-mono text-xs">
                            {previewContentType}
                          </div>
                        </div>
                        <div>
                          <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                            Size
                          </div>
                          <div className="text-text-bright mt-0.5 font-mono text-xs">
                            {formatNumber(previewBytes)} bytes
                          </div>
                        </div>
                        <div>
                          <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                            Tier
                          </div>
                          <div className="text-text-bright mt-0.5 font-mono text-xs">
                            {formatCompositionTier(spore.mediaProfile.tier)}
                          </div>
                        </div>
                      </div>
                      {dobContent?.media && dobContent.media.length > 0 && (
                        <div className="space-y-2">
                          {dobContent.media.map((m) => (
                            <div
                              key={m.hash}
                              className="border-base-border bg-base-surface/50 rounded border p-2.5"
                            >
                              <div className="flex items-baseline gap-2">
                                <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                                  {m.role ?? `Step ${m.step ?? 0}`}
                                </span>
                                <span className="text-text font-mono text-[10px]">
                                  {m.mediaType} · {formatNumber(m.size)} bytes
                                </span>
                              </div>
                            </div>
                          ))}
                        </div>
                      )}
                      {(!dobContent?.media || dobContent.media.length === 0) &&
                        sporePayload?.textContent && (
                          <div>
                            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                              Payload
                            </div>
                            <pre className="text-text mt-1 max-h-24 overflow-auto whitespace-pre-wrap break-all font-mono text-[10px] leading-relaxed">
                              {sporePayload.textContent.length > 400
                                ? `${sporePayload.textContent.slice(0, 400)}...`
                                : sporePayload.textContent}
                            </pre>
                          </div>
                        )}
                    </div>
                  </div>
                )}
                {/* Off-chain sources */}
                {spore.mediaProfile.sources.length > 0 && (
                  <div>
                    <div className="text-text-dim mb-2 font-mono text-[10px] uppercase tracking-wider">
                      Off-Chain
                    </div>
                    <div className="space-y-2">
                      {spore.mediaProfile.sources.map((source, index) => (
                        <div
                          key={`${source.uri}-${index}`}
                          className="border-base-border bg-base-surface/40 rounded border p-2.5"
                        >
                          <div className="flex items-baseline gap-2">
                            <span className="bg-base-elevated text-text-dim inline-block rounded px-1.5 py-0.5 font-mono text-[10px] uppercase">
                              {source.scheme}
                            </span>
                            <span className="text-text-dim font-mono text-[10px]">
                              {source.sourceLocation}
                            </span>
                          </div>
                          <div className="text-text-bright mt-1 break-all font-mono text-xs">
                            {source.uri}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
                {/* No media at all */}
                {!preview &&
                  spore.mediaProfile.tier === 'unknown' &&
                  !spore.mediaProfile.sources.length && (
                    <div className="text-text-dim text-xs">No media compositions detected.</div>
                  )}
              </div>
              {!!spore.mediaProfile.issues.length && (
                <div className="border-rouge-dim/40 bg-rouge/10 text-rouge-dim mt-3 space-y-1 rounded border p-2 font-mono text-xs">
                  {spore.mediaProfile.issues.map((issue) => (
                    <div key={issue}>- {issue}</div>
                  ))}
                </div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        )}
      </main>
    </div>
  );
}
