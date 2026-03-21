'use client';
import { useEffect, useMemo, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import Image from '@/components/ui/image';
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
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CapacityStatisticsSection } from '@/components/ui/capacity-statistics-section';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { ObjectActivityCard } from '@/components/object/object-activity-card';
import { ObjectCollectionStatCards } from '@/components/object/object-collection-stat-cards';
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
import { formatActivityTimestamp, formatStorageTier } from '@/lib/asset-utils';
import { getCapacityRangeParams, CapacityRangeKey } from '@/lib/capacity-range';
import { decodeDobContent, extractSporePayload } from '@/lib/dob-render';
import { ClusterDescription } from '@/components/spore/cluster-description';
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
  const [searchInput, setSearchInput] = useState('');
  const [searchKeyword, setSearchKeyword] = useState('');
  const [collectionStatusSelection, setCollectionStatusSelection] =
    useState<ItemStatusFilter>('all');
  const [activeCollectionTab, setActiveCollectionTab] = useState<CollectionSectionTab>(() =>
    isCollectionSectionTab(tabFromQuery) ? tabFromQuery : 'activities'
  );
  const [externalPreviewFailed, setExternalPreviewFailed] = useState(false);
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
  const { data: sporeCell, isLoading: isSporeCellLoading } = useQuery({
    queryKey: ['spore-cell-preview', spore?.txHash, resolvedSporeOutputIndex],
    queryFn: () => api.getCell(spore!.txHash, resolvedSporeOutputIndex!),
    enabled: !!spore?.txHash && resolvedSporeOutputIndex !== null && resolvedSporeOutputIndex >= 0,
    retry: false,
  });
  const { data: capacityChart, isLoading: isCapacityChartLoading } = useQuery({
    queryKey: ['spore-capacity-chart', assetId, capacityRange],
    queryFn: () =>
      capacityRangeParams
        ? api.getSporeObjectCapacityChart(assetId, capacityRangeParams)
        : api.getSporeObjectCapacityChart(assetId),
    enabled: !!spore,
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
  const getContentTypeIcon = (contentType: string) => {
    if (contentType.startsWith('image/') || contentType.startsWith('ipfs/image')) return '🖼️';
    if (contentType.startsWith('video/') || contentType.startsWith('ipfs/video')) return '🎬';
    if (contentType.startsWith('audio/') || contentType.startsWith('ipfs/audio')) return '🎵';
    if (contentType.startsWith('text/')) return '📄';
    return '📦';
  };
  const shortenHex = (value: string, start: number = 16, end: number = 12) => {
    const normalized = value.startsWith('0x') ? value : `0x${value}`;
    if (normalized.length <= start + end + 3) {
      return normalized;
    }
    return `${normalized.slice(0, start)}...${normalized.slice(-end)}`;
  };
  const sporePayload = useMemo(() => extractSporePayload(sporeCell), [sporeCell]);
  const dobContent = useMemo(() => {
    if (decodedDobByApi) {
      return {
        dnaHex: decodedDobByApi.dnaHex,
        traits: decodedDobByApi.traits,
        svgMarkup: decodedDobByApi.svgMarkup,
        issues: decodedDobByApi.issues,
      };
    }
    if (!spore) {
      return null;
    }
    return decodeDobContent({
      sporeContentType: spore.contentType,
      contentText: sporePayload?.textContent,
      clusterDescription: cluster?.description,
    });
  }, [cluster?.description, decodedDobByApi, spore, sporePayload?.textContent]);
  const mediaPreviewUrl = useMemo(() => {
    if (!sporePayload?.contentType || !sporePayload.contentHex) {
      return null;
    }
    const normalized = sporePayload.contentType.toLowerCase();
    if (
      !normalized.startsWith('image/') &&
      !normalized.startsWith('video/') &&
      !normalized.startsWith('audio/')
    ) {
      return null;
    }
    const safeBytes = Uint8Array.from(sporePayload.contentBytes);
    const blob = new Blob([safeBytes.buffer], { type: sporePayload.contentType });
    return URL.createObjectURL(blob);
  }, [sporePayload?.contentHex, sporePayload?.contentType, sporePayload?.contentBytes]);
  useEffect(() => {
    return () => {
      if (mediaPreviewUrl) {
        URL.revokeObjectURL(mediaPreviewUrl);
      }
    };
  }, [mediaPreviewUrl]);
  const dobSvgDataUrl = useMemo(() => {
    if (!dobContent?.svgMarkup) {
      return null;
    }
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(dobContent.svgMarkup)}`;
  }, [dobContent?.svgMarkup]);
  const externalImagePreviewUrl = useMemo(() => {
    const sources = spore?.mediaProfile?.sources ?? [];
    const firstImageSource = sources.find((source) => {
      const uri = source.uri.toLowerCase();
      return (
        uri.startsWith('https://') ||
        uri.startsWith('http://') ||
        uri.startsWith('data:image/') ||
        uri.startsWith('ipfs://') ||
        uri.startsWith('ar://') ||
        uri.endsWith('.png') ||
        uri.endsWith('.jpg') ||
        uri.endsWith('.jpeg') ||
        uri.endsWith('.gif') ||
        uri.endsWith('.webp') ||
        uri.endsWith('.svg') ||
        uri.endsWith('.avif')
      );
    });
    if (!firstImageSource) {
      return null;
    }
    const uri = firstImageSource.uri;
    if (uri.startsWith('ipfs://')) {
      const cidPath = uri.replace('ipfs://', '');
      return `https://ipfs.io/ipfs/${cidPath}`;
    }
    if (uri.startsWith('ar://')) {
      const id = uri.replace('ar://', '');
      return `https://arweave.net/${id}`;
    }
    return uri;
  }, [spore?.mediaProfile?.sources]);
  useEffect(() => {
    setExternalPreviewFailed(false);
  }, [externalImagePreviewUrl]);
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
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="mb-6">
            <Link
              href="/assets?type=object"
              className="hover:text-emphasis text-text-dim text-sm transition-colors"
            >
              ← Back to Objects
            </Link>
          </div>
          <PageHeader
            title={collection.name || 'Object Collection'}
            badge={<Badge variant="neutral">{collection.standard.toUpperCase()}</Badge>}
          />
          <ObjectCollectionStatCards
            totalCount={collection.totalCount}
            liveCount={collection.liveCount}
            storageTier={collection.storageProfile?.tier}
            storageOnchainRatio={collection.storageProfile?.fullyOnchainRatio}
          />
          <div className="space-y-6">
            <CapacityStatisticsSection
              capacityRange={capacityRange}
              onCapacityRangeChange={setCapacityRange}
              capacityChart={collectionCapacityChart}
              isCapacityChartLoading={isCollectionCapacityChartLoading}
              totalCapacity={collection.ownedCapacity}
              commonKnowledgeSize={collection.ownedKnowledge}
              totalCapacityLabel="Owned Capacity"
            />
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Collection ID</TerminalPanelHeader>
              <TerminalPanelContent>
                <HexDisplay value={collection.collectionId} truncate={false} />
              </TerminalPanelContent>
            </TerminalPanel>
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
                              <Link
                                href={`/objects/mnft/${item.nftId}`}
                                className="hover:underline"
                              >
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
          </div>
        </main>
      </div>
    );
  }
  if (!spore) {
    return null;
  }
  const resolvedOwnerAddress = spore.ownerAddress || ownerAddressRecord?.address || null;
  const previewContentType = sporePayload?.contentType || spore.contentType;
  const normalizedPreviewContentType = previewContentType.toLowerCase();
  const previewBytes = sporePayload?.contentBytes.length ?? spore.contentSize;
  const previewText = sporePayload?.textContent?.trim() ?? '';
  const previewTextTruncated = previewText.length > 600;
  const previewTextSnippet = previewTextTruncated ? `${previewText.slice(0, 600)}...` : previewText;
  const hasDecodedTraits = (dobContent?.traits.length ?? 0) > 0;
  const shouldShowPayloadTextPanel =
    !!previewTextSnippet &&
    (normalizedPreviewContentType.startsWith('text/') ||
      normalizedPreviewContentType.includes('json') ||
      normalizedPreviewContentType.startsWith('dob/'));
  const sporeOutputIndex =
    resolvedSporeOutputIndex !== null && resolvedSporeOutputIndex >= 0
      ? resolvedSporeOutputIndex
      : spore.outputIndex;
  const hasCellLink = Number.isInteger(sporeOutputIndex);
  const renderPipeline = dobSvgDataUrl
    ? 'DOB decoder generated SVG preview from cluster metadata and DNA bytes.'
    : mediaPreviewUrl
      ? 'Bytes were decoded into a media blob using the on-chain contentType.'
      : externalImagePreviewUrl && !externalPreviewFailed
        ? 'Resolved an image URI from media metadata and rendered via direct fetch.'
        : previewTextSnippet
          ? 'Bytes were decoded as UTF-8 text for direct inspection.'
          : 'Payload is shown as a generic binary asset because no richer decoder matched.';
  const renderSporePreview = () => {
    if (isSporeCellLoading) {
      return (
        <div className="text-text-dim flex h-64 items-center justify-center px-4 text-center font-mono text-xs">
          Loading on-chain payload...
        </div>
      );
    }
    if (dobSvgDataUrl) {
      return (
        <div className="bg-base-bg/60 h-64 p-3">
          <div className="relative h-full w-full">
            <Image
              src={dobSvgDataUrl}
              alt="DOB rendered preview"
              fill
              unoptimized
              sizes="(max-width: 768px) 100vw, 50vw"
              className="border-base-border rounded border object-contain"
            />
          </div>
        </div>
      );
    }
    if (mediaPreviewUrl && previewContentType.startsWith('image/')) {
      return (
        <div className="bg-base-bg/60 h-64 p-3">
          <div className="relative h-full w-full">
            <Image
              src={mediaPreviewUrl}
              alt="Spore content preview"
              fill
              unoptimized
              sizes="(max-width: 768px) 100vw, 50vw"
              className="border-base-border rounded border object-contain"
            />
          </div>
        </div>
      );
    }
    if (externalImagePreviewUrl && !externalPreviewFailed) {
      return (
        <div className="bg-base-bg/60 h-64 p-3">
          <div className="relative h-full w-full">
            <Image
              src={externalImagePreviewUrl}
              alt="Spore external media preview"
              fill
              unoptimized
              sizes="(max-width: 768px) 100vw, 50vw"
              className="border-base-border rounded border object-contain"
              onError={() => setExternalPreviewFailed(true)}
            />
          </div>
        </div>
      );
    }
    if (mediaPreviewUrl && previewContentType.startsWith('video/')) {
      return (
        <div className="bg-base-bg/60 h-64 p-3">
          <video
            src={mediaPreviewUrl}
            controls
            className="border-base-border h-full w-full rounded border"
          />
        </div>
      );
    }
    if (mediaPreviewUrl && previewContentType.startsWith('audio/')) {
      return (
        <div className="bg-base-bg/60 flex h-64 flex-col items-center justify-center gap-3 p-3">
          <div className="text-text-dim font-mono text-xs tracking-[0.2em]">AUDIO</div>
          <audio src={mediaPreviewUrl} controls className="w-full max-w-xs" />
        </div>
      );
    }
    if (dobContent?.traits.length) {
      return (
        <div className="bg-base-bg/60 h-64 overflow-y-auto p-3">
          <div className="space-y-2 font-mono text-xs">
            {dobContent.traits.map((trait) => (
              <div
                key={`${trait.name}-${trait.value}`}
                className="border-base-border bg-base-surface/70 rounded border p-2"
              >
                <div className="text-text-dim">{trait.name}</div>
                <div className="text-text-bright break-all">{trait.value}</div>
              </div>
            ))}
          </div>
        </div>
      );
    }
    if (previewTextSnippet) {
      return (
        <div className="bg-base-bg/60 h-64 overflow-y-auto p-3">
          <pre className="text-text-bright max-w-full whitespace-pre-wrap break-all font-mono text-xs">
            {previewTextSnippet}
          </pre>
        </div>
      );
    }
    return (
      <div className="bg-base-bg/60 flex h-64 items-center justify-center text-6xl">
        {getContentTypeIcon(previewContentType)}
      </div>
    );
  };
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/assets?type=object"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            ← Back to Objects
          </Link>
        </div>
        <PageHeader
          title={
            cluster?.name
              ? `${cluster.name} (${truncateHash(spore.sporeId, 6, 4)})`
              : `Spore Asset (${truncateHash(spore.sporeId, 6, 4)})`
          }
          badge={
            spore.isLive ? <Badge variant="green">Live</Badge> : <Badge variant="red">Burned</Badge>
          }
        />
        <div className="grid gap-6 xl:grid-cols-5">
          <div className="space-y-6 xl:col-span-2">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Spore Content Preview</TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="border-base-border mb-4 overflow-hidden rounded border">
                  {renderSporePreview()}
                </div>
                <div className="grid gap-2 sm:grid-cols-2">
                  <div className="border-base-border bg-base-surface/50 rounded border px-3 py-2">
                    <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                      Content Type
                    </div>
                    <div className="text-text-bright mt-1 break-all font-mono text-xs">
                      {previewContentType}
                    </div>
                  </div>
                  <div className="border-base-border bg-base-surface/50 rounded border px-3 py-2">
                    <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                      Payload Size
                    </div>
                    <div className="text-text-bright mt-1 font-mono text-xs">
                      {formatNumber(previewBytes)} bytes
                    </div>
                  </div>
                  <div className="border-base-border bg-base-surface/50 rounded border px-3 py-2 sm:col-span-2">
                    <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                      Rendering Pipeline
                    </div>
                    <div className="text-text mt-1 text-xs">{renderPipeline}</div>
                  </div>
                  {spore.mediaProfile && (
                    <div className="border-base-border bg-base-surface/50 rounded border px-3 py-2 sm:col-span-2">
                      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                        Storage Tier
                      </div>
                      <div className="mt-1 flex items-center gap-2 text-xs">
                        <Badge
                          variant={
                            spore.mediaProfile.tier === 'fully_on_ckb_and_btc'
                              ? 'green'
                              : spore.mediaProfile.tier === 'centralized_dependent'
                                ? 'red'
                                : spore.mediaProfile.tier === 'decentralized_external'
                                  ? 'neutral'
                                  : 'neutral'
                          }
                        >
                          {formatStorageTier(spore.mediaProfile.tier)}
                        </Badge>
                        <span className="text-text-dim">
                          {spore.mediaProfile.hasRenderableImage
                            ? 'Renderable image detected'
                            : 'No renderable image detected'}
                        </span>
                      </div>
                    </div>
                  )}
                  {dobContent?.dnaHex && (
                    <div className="border-info/30 bg-info/10 rounded border px-3 py-2 sm:col-span-2">
                      <div className="text-info font-mono text-[10px] uppercase tracking-wider">
                        DOB DNA
                      </div>
                      <div className="text-info-dim mt-1 font-mono text-xs">
                        {shortenHex(dobContent.dnaHex, 18, 14)}
                      </div>
                    </div>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
            {spore.mediaProfile && (
              <TerminalPanel>
                <TerminalPanelHeader indicator="active">Media Sources</TerminalPanelHeader>
                <TerminalPanelContent>
                  {!spore.mediaProfile.sources.length ? (
                    <div className="text-text-dim text-xs">No explicit media URI dependencies.</div>
                  ) : (
                    <div className="space-y-2">
                      {spore.mediaProfile.sources.map((source, index) => (
                        <div
                          key={`${source.uri}-${index}`}
                          className="border-base-border bg-base-surface/40 rounded border p-2"
                        >
                          <div className="mb-1 flex items-center gap-2">
                            <Badge
                              variant={
                                source.dependencyTier === 'fully_on_ckb_and_btc'
                                  ? 'green'
                                  : source.dependencyTier === 'centralized_dependent'
                                    ? 'red'
                                    : 'neutral'
                              }
                            >
                              {formatStorageTier(source.dependencyTier)}
                            </Badge>
                            <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
                              {source.sourceLocation}
                            </span>
                          </div>
                          <div className="text-text-bright break-all font-mono text-xs">
                            {source.uri}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
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
          </div>
          <div className="space-y-6 xl:col-span-3">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Spore Details</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Spore ID" layout="vertical" valueClassName="w-full">
                    <HexDisplay value={spore.sporeId} truncate={false} />
                  </DataField>
                  <DataField label="Status">
                    {spore.isLive ? (
                      <Badge variant="green">Live</Badge>
                    ) : (
                      <Badge variant="red">Burned</Badge>
                    )}
                  </DataField>
                  <DataField label="Content Type">
                    <span className="text-text-bright font-mono">{previewContentType}</span>
                  </DataField>
                  <DataField label="Payload Size">
                    <span className="text-text-bright font-mono">
                      {formatNumber(previewBytes)} bytes
                    </span>
                  </DataField>
                  <DataField label="Interpreted As">
                    <span className="text-text-bright font-mono">
                      {normalizedPreviewContentType.startsWith('image/')
                        ? 'Image'
                        : normalizedPreviewContentType.startsWith('video/')
                          ? 'Video'
                          : normalizedPreviewContentType.startsWith('audio/')
                            ? 'Audio'
                            : normalizedPreviewContentType.startsWith('text/')
                              ? 'Text'
                              : normalizedPreviewContentType.startsWith('dob/')
                                ? 'DOB Metadata'
                                : 'Binary'}
                    </span>
                  </DataField>
                  <DataField label="Owner" layout="vertical" valueClassName="w-full">
                    {resolvedOwnerAddress ? (
                      <Address address={resolvedOwnerAddress} truncate={false} />
                    ) : (
                      <span className="text-text-dim font-mono">Address unavailable</span>
                    )}
                  </DataField>
                  <DataField label="Owner Lock Hash" layout="vertical" valueClassName="w-full">
                    <Link href={`/address/${spore.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={spore.ownerLockHash} truncate={false} />
                    </Link>
                  </DataField>
                  <DataField label="Origin Cell">
                    {hasCellLink ? (
                      <Link
                        href={`/cell/${spore.txHash}-${sporeOutputIndex}`}
                        className="text-emphasis font-mono hover:underline"
                      >
                        <HexDisplay value={spore.txHash} size="sm" />-{sporeOutputIndex}
                      </Link>
                    ) : (
                      <span className="text-text-dim font-mono">Unavailable</span>
                    )}
                  </DataField>
                  <DataField label="Created at Block">
                    <Link
                      href={`/blocks/${spore.createdAtBlock}`}
                      className="text-emphasis font-mono hover:underline"
                    >
                      #{formatNumber(spore.createdAtBlock)}
                    </Link>
                  </DataField>
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>
            {hasDecodedTraits && (
              <TerminalPanel>
                <TerminalPanelHeader indicator="active">Decoded Traits</TerminalPanelHeader>
                <TerminalPanelContent>
                  <div className="text-text-dim mb-3 text-sm">
                    Traits derived from DOB metadata and on-chain DNA bytes.
                  </div>
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
            {shouldShowPayloadTextPanel && (
              <TerminalPanel>
                <TerminalPanelHeader indicator="active">Payload Text View</TerminalPanelHeader>
                <TerminalPanelContent>
                  <pre className="border-base-border bg-base-bg/40 text-text-bright max-h-80 max-w-full overflow-x-auto overflow-y-auto whitespace-pre-wrap break-all rounded border p-3 font-mono text-xs">
                    {previewTextSnippet}
                  </pre>
                  {previewTextTruncated && (
                    <div className="text-text-dim mt-2 text-xs">
                      Showing first 600 characters from on-chain payload text.
                    </div>
                  )}
                </TerminalPanelContent>
              </TerminalPanel>
            )}
            <CapacityStatisticsSection
              capacityRange={capacityRange}
              onCapacityRangeChange={setCapacityRange}
              capacityChart={capacityChart}
              isCapacityChartLoading={isCapacityChartLoading}
              totalCapacity={spore.ownedCapacity}
              commonKnowledgeSize={spore.ownedKnowledge}
            />
            {cluster && (
              <TerminalPanel>
                <TerminalPanelHeader indicator="active">Cluster Context</TerminalPanelHeader>
                <TerminalPanelContent>
                  <DataGrid columns={1}>
                    <DataField label="Name" layout="vertical" valueClassName="w-full">
                      <Link
                        href={`/clusters/${cluster.clusterId}`}
                        className="text-emphasis hover:underline"
                      >
                        {cluster.name || 'Unnamed Collection'}
                      </Link>
                    </DataField>
                    {cluster.description && (
                      <DataField label="Description" layout="vertical" valueClassName="w-full">
                        <ClusterDescription description={cluster.description} />
                      </DataField>
                    )}
                    <DataField label="Cluster ID" layout="vertical" valueClassName="w-full">
                      <Link href={`/clusters/${cluster.clusterId}`} className="hover:underline">
                        <HexDisplay value={cluster.clusterId} truncate={false} />
                      </Link>
                    </DataField>
                    <DataField label="Total Spores">
                      <span className="text-warning font-mono">
                        {formatNumber(cluster.sporesCount)}
                      </span>
                    </DataField>
                  </DataGrid>
                </TerminalPanelContent>
              </TerminalPanel>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}
