'use client';

import { useEffect, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
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
import { CapacityOccupationSection } from '@/components/ui/capacity-occupation-section';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { isDotbitAlias, normalizeNftAssetId } from '@/lib/nft-collections';
import { getOccupationRangeParams, OccupationRangeKey } from '@/lib/occupation-range';

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && error.message.includes('404');
}

export default function SporeDetailPage() {
  const params = useParams();
  const rawAssetId = params.sporeId as string;
  const [occupationRange, setOccupationRange] = useState<OccupationRangeKey>('all');
  const [searchInput, setSearchInput] = useState('');
  const [searchKeyword, setSearchKeyword] = useState('');
  const collectionItemsPagination = useCursorPagination();
  const occupationRangeParams = getOccupationRangeParams(occupationRange);
  const isDotbitCollection = isDotbitAlias(rawAssetId);
  const assetId = normalizeNftAssetId(rawAssetId);

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setSearchKeyword(searchInput.trim());
    }, 250);

    return () => window.clearTimeout(timeout);
  }, [searchInput]);

  const sporeQuery = useQuery({
    queryKey: ['spore', rawAssetId],
    queryFn: () => api.getSporeNft(assetId),
    enabled: !isDotbitCollection,
    retry: false,
  });
  const spore = sporeQuery.data;
  const shouldQueryCollection = isDotbitCollection || (!spore && isNotFoundError(sporeQuery.error));

  const collectionQuery = useQuery({
    queryKey: ['nft-collection', assetId],
    queryFn: () => api.getNftCollection(assetId),
    enabled: shouldQueryCollection,
    retry: false,
  });
  const collection = collectionQuery.data;
  const collectionAssetId = collection?.collectionId ?? assetId;

  const { data: cluster } = useQuery({
    queryKey: ['cluster', spore?.clusterId],
    queryFn: () => api.getSporeCluster(spore!.clusterId!),
    enabled: !!spore?.clusterId,
  });

  const { data: occupationChart, isLoading: isOccupationChartLoading } = useQuery({
    queryKey: ['spore-occupation-chart', assetId, occupationRange],
    queryFn: () =>
      occupationRangeParams
        ? api.getSporeNftOccupationChart(assetId, occupationRangeParams)
        : api.getSporeNftOccupationChart(assetId),
    enabled: !!spore,
  });

  const { data: collectionOccupationChart, isLoading: isCollectionOccupationChartLoading } =
    useQuery({
      queryKey: ['nft-collection-occupation-chart', collectionAssetId, occupationRange],
      queryFn: () =>
        occupationRangeParams
          ? api.getNftCollectionOccupationChart(collectionAssetId, occupationRangeParams)
          : api.getNftCollectionOccupationChart(collectionAssetId),
      enabled: !!collection,
    });

  const {
    data: collectionItems,
    isLoading: isCollectionItemsLoading,
    isFetching: isCollectionItemsFetching,
    isError: isCollectionItemsError,
  } = useQuery({
    queryKey: [
      'nft-collection-items',
      collectionAssetId,
      collectionItemsPagination.cursor,
      searchKeyword,
    ],
    queryFn: () =>
      api.getNftCollectionItems(collectionAssetId, {
        limit: 20,
        cursor: collectionItemsPagination.cursor,
        search: searchKeyword || undefined,
      }),
    enabled: !!collection,
    placeholderData: keepPreviousData,
  });

  useEffect(() => {
    collectionItemsPagination.reset();
  }, [collectionAssetId, searchKeyword, collectionItemsPagination.reset]);

  const formatNumber = (num: number) => {
    return new Intl.NumberFormat().format(num);
  };

  const getContentTypeIcon = (contentType: string) => {
    if (contentType.startsWith('image/')) return '🖼️';
    if (contentType.startsWith('video/')) return '🎬';
    if (contentType.startsWith('audio/')) return '🎵';
    if (contentType.startsWith('text/')) return '📄';
    return '📦';
  };

  const isPageLoading =
    sporeQuery.isLoading || (shouldQueryCollection && collectionQuery.isLoading);

  const hasTerminalError =
    (!spore && !collection && !isPageLoading && !shouldQueryCollection) ||
    (!spore && shouldQueryCollection && collectionQuery.isError);

  if (isPageLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="mb-6 h-10 w-48 animate-pulse rounded bg-slate-800" />
          <div className="grid gap-6 lg:grid-cols-3">
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
            <div className="h-64 animate-pulse rounded border border-slate-800 bg-slate-900/50 lg:col-span-2" />
          </div>
        </main>
      </div>
    );
  }

  if (hasTerminalError) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Asset not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  if (collection) {
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
            title={collection.name || 'NFT Collection'}
            badge={<Badge variant="neutral">{collection.standard.toUpperCase()}</Badge>}
          />

          <div className="space-y-6">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Collection Details</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Collection ID">
                    <HexDisplay value={collection.collectionId} truncate={false} color="accent" />
                  </DataField>
                  <DataField label="Standard">
                    <span className="font-mono text-slate-300">
                      {collection.standard.toUpperCase()}
                    </span>
                  </DataField>
                  <DataField label="Live NFTs">
                    <span className="text-amber font-mono">
                      {formatNumber(collection.liveCount)}
                    </span>
                  </DataField>
                  <DataField label="Total NFTs">
                    <span className="text-amber font-mono">
                      {formatNumber(collection.totalCount)}
                    </span>
                  </DataField>
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>

            <CapacityOccupationSection
              description="Daily cumulative live CKB occupation for this NFT collection."
              occupationRange={occupationRange}
              onOccupationRangeChange={setOccupationRange}
              occupationChart={collectionOccupationChart}
              isOccupationChartLoading={isCollectionOccupationChartLoading}
              totalCapacity={collection.liveCapacity}
              occupiedCapacity={collection.liveOccupiedCapacity}
            />

            <TerminalPanel>
              <TerminalPanelHeader
                indicator="active"
                actions={
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      value={searchInput}
                      onChange={(event) => setSearchInput(event.target.value)}
                      placeholder={isDotbitCollection ? 'Search .bit' : 'Search NFT'}
                      aria-label={isDotbitCollection ? 'Search .bit' : 'Search NFT'}
                      className="focus:border-terminal-green w-44 rounded border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs text-slate-200 outline-none transition-colors placeholder:text-slate-500"
                    />
                    {isCollectionItemsFetching && (
                      <span className="font-mono text-xs text-slate-500">Searching...</span>
                    )}
                  </div>
                }
              >
                Collection NFTs
              </TerminalPanelHeader>
              <TerminalPanelContent>
                {isCollectionItemsLoading ? (
                  <div className="py-8 text-center text-slate-500">Loading NFTs...</div>
                ) : isCollectionItemsError ? (
                  <div className="py-8 text-center text-rose-400">
                    Failed to load NFTs. Please refresh and try again.
                  </div>
                ) : !collectionItems?.data?.length ? (
                  <div className="py-8 text-center text-slate-500">No NFTs in this collection</div>
                ) : (
                  <div className="overflow-hidden rounded border border-slate-800 bg-slate-900/30">
                    {collectionItems.data.map((item) => (
                      <div
                        key={item.nftId}
                        className="row-scan hover:bg-slate-850/40 border-b border-slate-800 px-3 py-2.5 transition-colors last:border-b-0"
                      >
                        <div className="mb-1 flex items-center justify-between gap-3">
                          {item.txHash &&
                          item.outputIndex !== null &&
                          item.outputIndex !== undefined ? (
                            <Link
                              href={`/cell/${item.txHash}-${item.outputIndex}`}
                              className="hover:text-terminal-green font-mono text-sm text-white hover:underline"
                            >
                              {item.name || item.nftId}
                            </Link>
                          ) : (
                            <span className="font-mono text-sm text-white">
                              {item.name || item.nftId}
                            </span>
                          )}
                          {item.isLive ? (
                            <Badge variant="green">Live</Badge>
                          ) : (
                            <Badge variant="red">Burned</Badge>
                          )}
                        </div>
                        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-xs text-slate-400">
                          <span>
                            ID:{' '}
                            <span className="text-slate-300">
                              <HexDisplay
                                value={item.nftId}
                                color="accent"
                                size="sm"
                                startChars={10}
                                endChars={8}
                              />
                            </span>
                          </span>
                          <span>Block #{formatNumber(item.createdAtBlock)}</span>
                          <span>
                            Cell:{' '}
                            {item.txHash &&
                            item.outputIndex !== null &&
                            item.outputIndex !== undefined ? (
                              <Link
                                href={`/cell/${item.txHash}-${item.outputIndex}`}
                                className="text-terminal-green hover:underline"
                              >
                                <HexDisplay
                                  value={item.txHash}
                                  color="accent"
                                  size="sm"
                                  startChars={10}
                                  endChars={8}
                                />
                                -{item.outputIndex}
                              </Link>
                            ) : (
                              <span className="text-slate-500">Unavailable</span>
                            )}
                          </span>
                          {item.ownerLockHash && (
                            <span>
                              Owner:{' '}
                              <Link
                                href={`/address/${item.ownerLockHash}`}
                                className="hover:underline"
                              >
                                <HexDisplay
                                  value={item.ownerLockHash}
                                  color="accent"
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
                  totalLabel="NFTs"
                  pageSize={20}
                  page={collectionItemsPagination.page}
                  hasMore={collectionItems?.hasMore ?? false}
                  hasPrevious={collectionItemsPagination.hasPrevious}
                  onNext={() => collectionItemsPagination.goToNext(collectionItems?.nextCursor)}
                  onPrevious={collectionItemsPagination.goToPrevious}
                />
              </TerminalPanelFooter>
            </TerminalPanel>
          </div>
        </main>
      </div>
    );
  }

  if (!spore) {
    return null;
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
          title="Spore NFT"
          badge={
            spore.isLive ? <Badge variant="green">Live</Badge> : <Badge variant="red">Burned</Badge>
          }
        />

        <div className="grid gap-6 lg:grid-cols-3">
          <div className="lg:col-span-1">
            <TerminalPanel>
              <TerminalPanelContent>
                <div className="mb-4 flex h-48 items-center justify-center rounded border border-slate-800 bg-slate-900/50 text-6xl">
                  {getContentTypeIcon(spore.contentType)}
                </div>
                <div className="space-y-2 text-center">
                  <div className="font-mono text-lg text-white">{spore.contentType}</div>
                  <div className="font-mono text-sm text-slate-400">
                    {formatNumber(spore.contentSize)} bytes
                  </div>
                  {!spore.isLive && <Badge variant="red">Burned</Badge>}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
          </div>

          <div className="space-y-6 lg:col-span-2">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">NFT Details</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Spore ID">
                    <HexDisplay value={spore.sporeId} truncate={false} color="accent" />
                  </DataField>
                  <DataField label="Owner">
                    {spore.ownerAddress ? (
                      <Address address={spore.ownerAddress} truncate={false} />
                    ) : (
                      <Link href={`/address/${spore.ownerLockHash}`} className="hover:underline">
                        <HexDisplay value={spore.ownerLockHash} truncate={false} color="accent" />
                      </Link>
                    )}
                  </DataField>
                  <DataField label="Owner Lock Hash">
                    <Link href={`/address/${spore.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={spore.ownerLockHash} truncate={false} color="accent" />
                    </Link>
                  </DataField>
                  <DataField label="Created at Block">
                    <Link
                      href={`/blocks/${spore.createdAtBlock}`}
                      className="text-terminal-green font-mono hover:underline"
                    >
                      #{formatNumber(spore.createdAtBlock)}
                    </Link>
                  </DataField>
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>

            <CapacityOccupationSection
              description="Daily cumulative live CKB occupation for this NFT."
              occupationRange={occupationRange}
              onOccupationRangeChange={setOccupationRange}
              occupationChart={occupationChart}
              isOccupationChartLoading={isOccupationChartLoading}
              totalCapacity={spore.liveCapacity}
              occupiedCapacity={spore.liveOccupiedCapacity}
            />

            {cluster && (
              <TerminalPanel>
                <TerminalPanelHeader indicator="active">Collection</TerminalPanelHeader>
                <TerminalPanelContent>
                  <DataGrid columns={1}>
                    <DataField label="Name">
                      <Link
                        href={`/clusters/${cluster.clusterId}`}
                        className="text-terminal-green hover:underline"
                      >
                        {cluster.name || 'Unnamed Collection'}
                      </Link>
                    </DataField>
                    {cluster.description && (
                      <DataField label="Description">
                        <span className="text-slate-300">{cluster.description}</span>
                      </DataField>
                    )}
                    <DataField label="Cluster ID">
                      <Link href={`/clusters/${cluster.clusterId}`} className="hover:underline">
                        <HexDisplay value={cluster.clusterId} truncate={false} color="accent" />
                      </Link>
                    </DataField>
                    <DataField label="Total NFTs">
                      <span className="text-amber font-mono">
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
