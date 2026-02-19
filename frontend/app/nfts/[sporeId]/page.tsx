'use client';

import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { Address } from '@/components/ui/address';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && error.message.includes('404');
}

function renderCapacityUtilization(
  liveCapacity: string | null | undefined,
  liveOccupiedCapacity: string | null | undefined
) {
  if (!liveCapacity || !liveOccupiedCapacity) return null;
  const totalBig = BigInt(liveCapacity);
  const occupiedBig = BigInt(liveOccupiedCapacity);
  if (totalBig <= BigInt(0)) return null;

  const freeBig = totalBig - occupiedBig;
  const ratio = Number((occupiedBig * BigInt(10000)) / totalBig) / 100;

  return (
    <DataField label="Capacity Utilization">
      <div className="w-full">
        <div className="mb-1 flex items-center justify-between">
          <span className="font-mono text-xs text-slate-400">{ratio.toFixed(1)}% occupied</span>
        </div>
        <div className="flex h-2.5 w-full overflow-hidden rounded-sm bg-slate-800">
          <div
            className="bg-amber transition-all duration-300"
            style={{ width: `${Math.max(ratio, 0.5)}%` }}
          />
          <div className="bg-terminal-green/30 flex-1" />
        </div>
        <div className="mt-1.5 flex items-center justify-between">
          <span
            className="text-amber font-mono text-xs"
            title={formatCkbAmount(liveOccupiedCapacity).full + ' CKB'}
          >
            Occupied: {formatCkbCompact(liveOccupiedCapacity).value} CKB
          </span>
          <span
            className="text-terminal-green font-mono text-xs"
            title={formatCkbAmount(freeBig.toString()).full + ' CKB'}
          >
            Unoccupied: {formatCkbCompact(freeBig.toString()).value} CKB
          </span>
        </div>
      </div>
    </DataField>
  );
}

export default function SporeDetailPage() {
  const params = useParams();
  const assetId = params.sporeId as string;

  const sporeQuery = useQuery({
    queryKey: ['spore', assetId],
    queryFn: () => api.getSporeNft(assetId),
    retry: false,
  });
  const spore = sporeQuery.data;
  const shouldQueryCollection = !spore && isNotFoundError(sporeQuery.error);

  const collectionQuery = useQuery({
    queryKey: ['nft-collection', assetId],
    queryFn: () => api.getNftCollection(assetId),
    enabled: shouldQueryCollection,
    retry: false,
  });
  const collection = collectionQuery.data;

  const { data: cluster } = useQuery({
    queryKey: ['cluster', spore?.clusterId],
    queryFn: () => api.getSporeCluster(spore!.clusterId!),
    enabled: !!spore?.clusterId,
  });

  const { data: occupationChart, isLoading: isOccupationChartLoading } = useQuery({
    queryKey: ['spore-occupation-chart', assetId],
    queryFn: () => api.getSporeNftOccupationChart(assetId),
    enabled: !!spore,
  });

  const { data: collectionOccupationChart, isLoading: isCollectionOccupationChartLoading } =
    useQuery({
      queryKey: ['nft-collection-occupation-chart', assetId],
      queryFn: () => api.getNftCollectionOccupationChart(assetId),
      enabled: !!collection,
    });

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
            badge={<Badge variant="purple">{collection.standard.toUpperCase()}</Badge>}
          />

          <div className="space-y-6">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Collection Details</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Collection ID">
                    <HexDisplay value={collection.collectionId} truncate={false} color="white" />
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
                  {renderCapacityUtilization(
                    collection.liveCapacity,
                    collection.liveOccupiedCapacity
                  )}
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>

            <TerminalPanel>
              <TerminalPanelHeader indicator="none">Occupation History</TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="mb-3 text-sm text-slate-400">
                  Daily cumulative live CKB occupation for this NFT collection.
                </div>
                {isCollectionOccupationChartLoading ? (
                  <div className="py-8 text-center text-slate-500">
                    Loading occupation history...
                  </div>
                ) : collectionOccupationChart && collectionOccupationChart.data.length > 0 ? (
                  <StackedAreaChart
                    data={collectionOccupationChart.data}
                    series={collectionOccupationChart.series}
                  />
                ) : (
                  <div className="py-8 text-center text-slate-500">No occupation history yet</div>
                )}
              </TerminalPanelContent>
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
                    <HexDisplay value={spore.sporeId} truncate={false} color="white" />
                  </DataField>
                  <DataField label="Owner">
                    {spore.ownerAddress ? (
                      <Address address={spore.ownerAddress} truncate={false} />
                    ) : (
                      <Link href={`/address/${spore.ownerLockHash}`} className="hover:underline">
                        <HexDisplay value={spore.ownerLockHash} truncate={false} color="green" />
                      </Link>
                    )}
                  </DataField>
                  <DataField label="Owner Lock Hash">
                    <Link href={`/address/${spore.ownerLockHash}`} className="hover:underline">
                      <HexDisplay value={spore.ownerLockHash} truncate={false} color="green" />
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
                  {renderCapacityUtilization(spore.liveCapacity, spore.liveOccupiedCapacity)}
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>

            <TerminalPanel>
              <TerminalPanelHeader indicator="none">Occupation History</TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="mb-3 text-sm text-slate-400">
                  Daily cumulative live CKB occupation for this NFT.
                </div>
                {isOccupationChartLoading ? (
                  <div className="py-8 text-center text-slate-500">
                    Loading occupation history...
                  </div>
                ) : occupationChart && occupationChart.data.length > 0 ? (
                  <StackedAreaChart data={occupationChart.data} series={occupationChart.series} />
                ) : (
                  <div className="py-8 text-center text-slate-500">No occupation history yet</div>
                )}
              </TerminalPanelContent>
            </TerminalPanel>

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
                        <HexDisplay value={cluster.clusterId} truncate={false} color="green" />
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
