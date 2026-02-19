'use client';

import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams } from 'next/navigation';
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
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

export default function ClusterDetailPage() {
  const params = useParams();
  const clusterId = params.clusterId as string;

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
    queryKey: ['cluster-occupation-chart', clusterId],
    queryFn: () => api.getSporeClusterOccupationChart(clusterId),
    enabled: !!clusterId,
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
            href="/assets?type=dob"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← Back to DOBs
          </Link>
        </div>

        <PageHeader
          title={cluster.name || 'Unnamed Collection'}
          badge={<Badge variant="purple">Spore Cluster</Badge>}
        />

        <div className="grid gap-6 lg:grid-cols-3">
          <div className="lg:col-span-1">
            <TerminalPanel>
              <TerminalPanelHeader indicator="active">Cluster Info</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Cluster ID">
                    <HexDisplay
                      value={cluster.clusterId}
                      truncate={false}
                      color="white"
                      size="sm"
                    />
                  </DataField>
                  {cluster.description && (
                    <DataField label="Description">
                      <span className="text-slate-300">{cluster.description}</span>
                    </DataField>
                  )}
                  <DataField label="Total DOBs">
                    <span className="text-amber text-xl font-semibold tabular-nums">
                      {formatNumber(cluster.sporesCount)}
                    </span>
                  </DataField>
                  {cluster.liveCapacity &&
                    cluster.liveOccupiedCapacity &&
                    (() => {
                      const totalBig = BigInt(cluster.liveCapacity);
                      const occupiedBig = BigInt(cluster.liveOccupiedCapacity);
                      if (totalBig <= BigInt(0)) return null;
                      const freeBig = totalBig - occupiedBig;
                      const ratio = Number((occupiedBig * BigInt(10000)) / totalBig) / 100;
                      return (
                        <DataField label="Capacity Utilization">
                          <div className="w-full">
                            <div className="mb-1 flex items-center justify-between">
                              <span className="font-mono text-xs text-slate-400">
                                {ratio.toFixed(1)}% occupied
                              </span>
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
                                title={formatCkbAmount(cluster.liveOccupiedCapacity).full + ' CKB'}
                              >
                                Occupied: {formatCkbCompact(cluster.liveOccupiedCapacity).value} CKB
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
                    })()}
                  <DataField label="Creator">
                    {cluster.ownerAddress ? (
                      <Address address={cluster.ownerAddress} truncate={false} />
                    ) : (
                      <Link href={`/address/${cluster.ownerLockHash}`} className="hover:underline">
                        <HexDisplay value={cluster.ownerLockHash} color="green" size="sm" />
                      </Link>
                    )}
                  </DataField>
                  <DataField label="Created at Block">
                    <Link
                      href={`/blocks/${cluster.createdAtBlock}`}
                      className="text-terminal-green font-mono hover:underline"
                    >
                      #{formatNumber(cluster.createdAtBlock)}
                    </Link>
                  </DataField>
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>
          </div>

          <div className="space-y-6 lg:col-span-2">
            <TerminalPanel>
              <TerminalPanelHeader indicator="none">Occupation History</TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="mb-3 text-sm text-slate-400">
                  Daily cumulative live CKB occupation for this DOB collection.
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

            <TerminalPanel>
              <TerminalPanelHeader indicator="active">
                DOBs in this Collection ({formatNumber(sporesData?.total || 0)})
              </TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {sporesLoading ? (
                  <div className="py-8 text-center text-slate-400">Loading DOBs...</div>
                ) : sporesData?.data?.length ? (
                  <>
                    <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                      <div className="flex-1">DOB</div>
                      <div className="w-32">Content</div>
                      <div className="w-24 text-right">Size</div>
                      <div className="w-36 text-right">Owner</div>
                    </div>

                    {sporesData.data.map((spore) => (
                      <TerminalRow key={spore.sporeId}>
                        <div className="flex items-center">
                          <div className="flex flex-1 items-center gap-2">
                            <span className="text-lg">{getContentTypeIcon(spore.contentType)}</span>
                            <Link href={`/nfts/${spore.sporeId}`} className="hover:underline">
                              <HexDisplay value={spore.sporeId} color="green" size="sm" />
                            </Link>
                          </div>
                          <div className="w-32 font-mono text-sm text-slate-400">
                            {spore.contentType}
                          </div>
                          <div className="w-24 text-right font-mono text-sm text-slate-400">
                            {formatNumber(spore.contentSize)} B
                          </div>
                          <div className="w-36 text-right">
                            {spore.ownerAddress ? (
                              <Link
                                href={`/address/${spore.ownerLockHash}`}
                                className="hover:underline"
                              >
                                <Address address={spore.ownerAddress} truncate />
                              </Link>
                            ) : (
                              <Link
                                href={`/address/${spore.ownerLockHash}`}
                                className="hover:underline"
                              >
                                <HexDisplay
                                  value={spore.ownerLockHash}
                                  color="green"
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
                ) : (
                  <div className="py-8 text-center text-slate-500">No DOBs in this collection</div>
                )}
              </TerminalPanelContent>

              {sporesData && sporesData.data.length > 0 && (
                <TerminalPanelFooter>
                  <CursorPagination
                    total={sporesData.total}
                    totalLabel="DOBs"
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
