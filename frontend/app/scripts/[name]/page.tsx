'use client';

import { useState, useEffect } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Capacity } from '@/components/ui/capacity';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { OccupationRangeSelector } from '@/components/ui/occupation-range-selector';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api } from '@/lib/api';
import { getOccupationRangeParams, OccupationRangeKey } from '@/lib/occupation-range';
import { formatCkbCompact } from '@/lib/utils';
import type { KnownScript } from '@/lib/api';

interface SelectedDeployment {
  codeHash: string;
  hashType: string;
  scriptKind?: 'lock' | 'type';
}

function compareDeploymentsByDeployedAt(a: KnownScript, b: KnownScript): number {
  const aTs = a.deployedAt ?? null;
  const bTs = b.deployedAt ?? null;
  if (aTs != null && bTs != null) {
    return bTs - aTs;
  }
  if (aTs != null) return -1;
  if (bTs != null) return 1;
  return a.codeHash.localeCompare(b.codeHash);
}

export default function ScriptDetailPage() {
  const params = useParams();
  const name = decodeURIComponent(params.name as string);
  const [occupationRange, setOccupationRange] = useState<OccupationRangeKey>('all');
  const [selectedDeployment, setSelectedDeployment] = useState<SelectedDeployment | null>(null);
  const cellsPagination = useCursorPagination();
  const occupationRangeParams = getOccupationRangeParams(occupationRange);

  const {
    data: deployments,
    isLoading: isDeploymentsLoading,
    error: deploymentsError,
  } = useQuery({
    queryKey: ['script', name],
    queryFn: () => api.getScript(name),
  });

  const { data: usage, isLoading: isUsageLoading } = useQuery({
    queryKey: ['script-usage', name],
    queryFn: () => api.getScriptUsage(name),
  });

  const selectedScriptKindForChart =
    selectedDeployment?.scriptKind === 'lock' || selectedDeployment?.scriptKind === 'type'
      ? selectedDeployment.scriptKind
      : undefined;
  const { data: selectedOccupationChart, isLoading: isSelectedOccupationChartLoading } = useQuery({
    queryKey: [
      'script-occupation-chart',
      'deployment',
      selectedDeployment?.codeHash,
      selectedScriptKindForChart,
      occupationRange,
    ],
    queryFn: () =>
      occupationRangeParams
        ? api.getScriptOccupationChartByCodeHash(
            selectedDeployment!.codeHash,
            selectedScriptKindForChart,
            occupationRangeParams
          )
        : api.getScriptOccupationChartByCodeHash(
            selectedDeployment!.codeHash,
            selectedScriptKindForChart
          ),
    enabled: !!selectedDeployment,
  });

  useEffect(() => {
    if (deployments && deployments.length > 0 && usage && !selectedDeployment) {
      const sortedDeployments = [...deployments].sort(compareDeploymentsByDeployedAt);
      const usageByCodeHash = new Map(usage.byDeployment.map((d) => [d.codeHash, d]));
      const firstWithCells = sortedDeployments.find((d) => {
        const stats = usageByCodeHash.get(d.codeHash);
        return d.hashType && stats && stats.liveCellsCount > 0;
      });
      const target = firstWithCells || sortedDeployments[0];
      const stats = usageByCodeHash.get(target.codeHash);
      if (target.hashType) {
        setSelectedDeployment({
          codeHash: target.codeHash,
          hashType: target.hashType,
          scriptKind: stats?.scriptKind as 'lock' | 'type' | undefined,
        });
      }
    }
  }, [deployments, usage, selectedDeployment]);

  const { data: cellsData, isLoading: isCellsLoading } = useQuery({
    queryKey: [
      'script-cells',
      selectedDeployment?.codeHash,
      selectedDeployment?.hashType,
      selectedDeployment?.scriptKind,
      cellsPagination.cursor,
    ],
    queryFn: () =>
      api.getCellsByScriptRef({
        codeHash: selectedDeployment!.codeHash,
        hashType: selectedDeployment!.hashType,
        scriptKind: selectedDeployment!.scriptKind,
        limit: 20,
        cursor: cellsPagination.cursor,
      }),
    enabled: !!selectedDeployment,
    placeholderData: keepPreviousData,
  });

  const isLoading = isDeploymentsLoading || isUsageLoading;

  if (isLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="h-20 w-full rounded bg-slate-900" />
            <div className="h-64 rounded bg-slate-900" />
            <div className="h-96 rounded bg-slate-900" />
          </div>
        </main>
      </div>
    );
  }

  if (deploymentsError || !deployments || deployments.length === 0) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Script not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const scriptInfo = deployments[0];

  const formatNumber = (num: number) => {
    return new Intl.NumberFormat().format(num);
  };

  const usageByCodeHash = new Map(usage?.byDeployment.map((d) => [d.codeHash, d]) ?? []);
  const inferredScriptKind = usage?.byDeployment.find((d) => d.scriptKind)?.scriptKind;
  const sortedDeployments = [...deployments].sort(compareDeploymentsByDeployedAt);
  const selectedDeploymentUsage = selectedDeployment
    ? usageByCodeHash.get(selectedDeployment.codeHash)
    : undefined;

  const handleDeploymentClick = (deployment: KnownScript) => {
    if (!deployment.hashType) return;
    const stats = usageByCodeHash.get(deployment.codeHash);
    const newSelected = {
      codeHash: deployment.codeHash,
      hashType: deployment.hashType,
      scriptKind: stats?.scriptKind as 'lock' | 'type' | undefined,
    };
    if (
      selectedDeployment?.codeHash !== newSelected.codeHash ||
      selectedDeployment?.hashType !== newSelected.hashType
    ) {
      setSelectedDeployment(newSelected);
      cellsPagination.reset();
    }
  };

  const isSelected = (deployment: KnownScript) =>
    selectedDeployment?.codeHash === deployment.codeHash &&
    selectedDeployment?.hashType === deployment.hashType;

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title={scriptInfo.name}
          subtitle={scriptInfo.description}
          badge={
            <div className="flex items-center gap-2">
              {inferredScriptKind && (
                <Badge variant={inferredScriptKind === 'lock' ? 'blue' : 'purple'}>
                  {inferredScriptKind.toUpperCase()}
                </Badge>
              )}
              {scriptInfo.decoderType && (
                <Badge variant="gray">{scriptInfo.decoderType.toUpperCase()}</Badge>
              )}
            </div>
          }
          actions={
            <div className="flex gap-3">
              {scriptInfo.rfc && (
                <a
                  href={scriptInfo.rfc}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-terminal-green hover:border-terminal-dark rounded border border-slate-700 px-3 py-1 font-mono text-sm transition-colors hover:bg-slate-900"
                >
                  RFC
                </a>
              )}
              {scriptInfo.website && (
                <a
                  href={scriptInfo.website}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-terminal-green hover:border-terminal-dark rounded border border-slate-700 px-3 py-1 font-mono text-sm transition-colors hover:bg-slate-900"
                >
                  Website
                </a>
              )}
              {scriptInfo.sourceUrl && (
                <a
                  href={scriptInfo.sourceUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-terminal-green hover:border-terminal-dark rounded border border-slate-700 px-3 py-1 font-mono text-sm transition-colors hover:bg-slate-900"
                >
                  Source
                </a>
              )}
            </div>
          }
        />

        <TerminalPanel className="mb-6" glow>
          <TerminalPanelHeader indicator="active">Deployments</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="overflow-x-auto">
              <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                <div className="flex-1">Deployment</div>
                <div className="w-48">Deployed At</div>
                <div className="w-20">Tag</div>
                <div className="w-20">Kind</div>
                <div className="w-20">Status</div>
                <div className="w-24 text-right">Cells</div>
                <div className="w-28 text-right">Capacity</div>
              </div>
              {sortedDeployments.map((deployment, idx) => {
                const stats = usageByCodeHash.get(deployment.codeHash);
                const selected = isSelected(deployment);
                return (
                  <TerminalRow
                    key={idx}
                    className={`cursor-pointer ${selected ? 'bg-terminal-dark/20' : ''}`}
                  >
                    <div
                      className="flex w-full items-center"
                      onClick={() => handleDeploymentClick(deployment)}
                    >
                      <div className="min-w-0 flex-1">
                        <div className="mb-1">
                          {deployment.codeCellTxHash && deployment.codeCellOutputIndex !== null ? (
                            <Link
                              href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                              onClick={(e) => e.stopPropagation()}
                              className="text-amber hover:underline"
                            >
                              <HexDisplay
                                value={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                                color="amber"
                                startChars={8}
                                endChars={8}
                              />
                            </Link>
                          ) : (
                            <span className="text-slate-600">-</span>
                          )}
                        </div>
                        <HexDisplay
                          value={`${deployment.hashType}:${deployment.codeHash}`}
                          color={selected ? 'green' : 'white'}
                          startChars={15}
                          endChars={6}
                        />
                      </div>
                      <div
                        className="w-48 font-mono text-xs text-slate-400"
                        title={
                          deployment.deployedAt
                            ? new Date(deployment.deployedAt).toISOString()
                            : undefined
                        }
                      >
                        {deployment.deployedAt
                          ? new Date(deployment.deployedAt).toLocaleString()
                          : '-'}
                      </div>
                      <div className="w-20 text-slate-400">{deployment.tag || '-'}</div>
                      <div className="w-20">
                        {stats?.scriptKind ? (
                          <Badge variant={stats.scriptKind === 'lock' ? 'blue' : 'purple'}>
                            {stats.scriptKind}
                          </Badge>
                        ) : (
                          <span className="text-slate-600">-</span>
                        )}
                      </div>
                      <div className="w-20">
                        {deployment.deprecated ? (
                          <Badge variant="red">Deprecated</Badge>
                        ) : (
                          <Badge variant="green">Active</Badge>
                        )}
                      </div>
                      <div className="w-24 text-right font-mono text-slate-400">
                        {stats ? (
                          <span title={`Total: ${formatNumber(stats.cellsCount)}`}>
                            {formatNumber(stats.liveCellsCount)}
                          </span>
                        ) : (
                          '-'
                        )}
                      </div>
                      <div className="w-28 text-right font-mono text-slate-400">
                        {stats ? (
                          <span title={`${formatCkbCompact(stats.liveCapacitySum).full} CKB`}>
                            {formatCkbCompact(stats.liveCapacitySum).value}
                          </span>
                        ) : (
                          '-'
                        )}
                      </div>
                    </div>
                  </TerminalRow>
                );
              })}
              {usage && (
                <>
                  <div className="flex border-t border-slate-700 bg-slate-900/50 px-4 py-3 font-medium">
                    <div className="flex-1 text-slate-400">Total</div>
                    <div className="w-48" />
                    <div className="w-20" />
                    <div className="w-20" />
                    <div className="w-20" />
                    <div className="text-terminal-green w-24 text-right font-mono">
                      <span title={`Total: ${formatNumber(usage.cellsCount)}`}>
                        {formatNumber(usage.liveCellsCount)}
                      </span>
                    </div>
                    <div className="text-terminal-green w-28 text-right font-mono">
                      <span title={`${formatCkbCompact(usage.liveCapacitySum).full} CKB`}>
                        {formatCkbCompact(usage.liveCapacitySum).value}
                      </span>
                    </div>
                  </div>
                </>
              )}
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        {selectedDeployment && (
          <>
            <TerminalPanel className="mb-6">
              <TerminalPanelHeader indicator="none">
                <div className="flex items-center gap-2">
                  <span>Capacity &amp; Occupation</span>
                  <span className="text-slate-600">|</span>
                  <HexDisplay
                    value={`${selectedDeployment.hashType}:${selectedDeployment.codeHash}`}
                    color="white"
                    size="sm"
                    startChars={15}
                    endChars={6}
                  />
                </div>
              </TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {selectedDeploymentUsage && (
                  <div className="border-b border-slate-800 px-4 py-4">
                    <CapacityUtilization
                      totalCapacity={selectedDeploymentUsage.liveCapacitySum}
                      occupiedCapacity={selectedDeploymentUsage.liveOccupiedCapacitySum}
                      label="Capacity & Occupation"
                    />
                  </div>
                )}
                <div className="px-4 py-4">
                  <div className="mb-3 text-xs text-slate-500">
                    Historical occupied/unoccupied live capacity for the selected deployment.
                  </div>
                  <OccupationRangeSelector value={occupationRange} onChange={setOccupationRange} />
                  {isSelectedOccupationChartLoading ? (
                    <div className="py-6 text-center text-slate-500">
                      Loading deployment history...
                    </div>
                  ) : selectedOccupationChart && selectedOccupationChart.data.length > 0 ? (
                    <StackedAreaChart
                      data={selectedOccupationChart.data}
                      series={selectedOccupationChart.series}
                      height={220}
                    />
                  ) : (
                    <div className="py-6 text-center text-slate-500">No deployment history yet</div>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>

            <TerminalPanel>
              <TerminalPanelHeader indicator="none">
                <div className="flex items-center gap-2">
                  <span>Cells</span>
                  <span className="text-slate-600">|</span>
                  <HexDisplay
                    value={`${selectedDeployment.hashType}:${selectedDeployment.codeHash}`}
                    color="white"
                    size="sm"
                    startChars={15}
                    endChars={6}
                  />
                </div>
              </TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {isCellsLoading ? (
                  <div className="py-8 text-center text-slate-500">Loading cells...</div>
                ) : cellsData && cellsData.data.length > 0 ? (
                  <>
                    <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                      <div className="flex-1">Cell</div>
                      <div className="w-52 shrink-0 text-right">Capacity</div>
                      <div className="w-24 shrink-0 text-right">Data Size</div>
                      <div className="w-28 shrink-0 text-right">Created At</div>
                    </div>
                    {cellsData.data.map((cell) => (
                      <TerminalRow key={`${cell.txHash}-${cell.outputIndex}`}>
                        <div className="flex items-center">
                          <div className="flex-1">
                            <Link
                              href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                              className="text-terminal-green hover:underline"
                            >
                              <HexDisplay value={`${cell.txHash}:${cell.outputIndex}`} />
                            </Link>
                          </div>
                          <div className="w-52 shrink-0 text-right text-white">
                            <Capacity value={cell.capacity} />
                          </div>
                          <div className="w-24 shrink-0 text-right font-mono text-slate-400">
                            {cell.cellType === 'genesis_special_burn' ? (
                              <span
                                className="cursor-help border-b border-dashed border-slate-600"
                                title="Virtual occupied capacity: 5.04B CKB"
                              >
                                <Capacity value={cell.virtualOccupiedCapacity || '0'} />
                              </span>
                            ) : (
                              <>{cell.dataSize.toLocaleString()} bytes</>
                            )}
                          </div>
                          <div className="w-28 shrink-0 text-right">
                            <Link
                              href={`/blocks/${cell.createdAtBlock}`}
                              className="text-amber hover:underline"
                            >
                              #{cell.createdAtBlock.toLocaleString()}
                            </Link>
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                    <div className="border-t border-slate-800 p-4">
                      <CursorPagination
                        total={cellsData.total}
                        totalLabel="cells"
                        pageSize={20}
                        page={cellsPagination.page}
                        hasMore={cellsData.hasMore}
                        hasPrevious={cellsPagination.hasPrevious}
                        onNext={() => cellsPagination.goToNext(cellsData.nextCursor)}
                        onPrevious={cellsPagination.goToPrevious}
                      />
                    </div>
                  </>
                ) : (
                  <div className="py-8 text-center text-slate-500">
                    No cells found for this script
                  </div>
                )}
              </TerminalPanelContent>
            </TerminalPanel>
          </>
        )}
      </main>
    </div>
  );
}
