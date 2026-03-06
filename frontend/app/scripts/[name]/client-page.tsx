'use client';

import { useEffect, useMemo, useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { useSearchParams } from '@/src/navigation';
import Link from '@/components/ui/link';
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
import {
  getScriptRefBadgeLabel,
  getScriptRefQueryHashType,
  normalizeScriptRefHashType,
  type ScriptRefHashType,
} from '@/lib/script-ref';
import { formatCkbCompact } from '@/lib/utils';
import type { KnownScript, ScriptLookupInfo } from '@/lib/api';

interface SelectedDeployment {
  codeHash: string;
  hashType: ScriptRefHashType;
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

function normalizeHash(value: string | null | undefined): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function isHexScriptHash(value: string): boolean {
  return /^0x[0-9a-fA-F]{64}$/.test(value);
}

function deploymentReferenceHashes(
  deployment: KnownScript,
  lookupInfo?: ScriptLookupInfo
): {
  typeRef: string | null;
  dataRef: string | null;
  dataRefType: ScriptRefHashType;
} {
  const normalizedHashType = normalizeScriptRefHashType(deployment.hashType);
  const lookupTypeRef = normalizeHash(lookupInfo?.deploymentTypeHash);
  const lookupDataRef = normalizeHash(lookupInfo?.deploymentDataHash);
  const typeRef =
    lookupTypeRef ??
    deployment.typeHash ??
    (normalizedHashType === 'type' ? deployment.codeHash : null);
  const dataRef =
    lookupDataRef ??
    deployment.dataHash ??
    (normalizedHashType !== 'type' ? deployment.codeHash : null);
  const baseDataRefType =
    normalizedHashType !== 'type' ? getScriptRefQueryHashType(deployment.hashType, 'data') : 'data';
  const dataRefType =
    lookupInfo?.hashType && lookupInfo.hashType !== 'type'
      ? getScriptRefQueryHashType(lookupInfo.hashType, baseDataRefType)
      : baseDataRefType;

  return { typeRef, dataRef, dataRefType };
}

export interface ScriptDetailPageProps {
  name: string;
}

export default function ScriptDetailPage({ name: routeName }: ScriptDetailPageProps) {
  const searchParams = useSearchParams();
  const name = decodeURIComponent(routeName);
  const selectedRefParam = searchParams.get('ref');
  const selectedRef =
    selectedRefParam && isHexScriptHash(selectedRefParam.trim())
      ? `0x${selectedRefParam.trim().slice(2).toLowerCase()}`
      : null;
  const selectedRefHashType = normalizeScriptRefHashType(searchParams.get('hashType'));
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
  const lookupCodeHashes = useMemo(() => {
    const refs = new Set<string>((deployments ?? []).map((deployment) => deployment.codeHash));
    if (selectedRef) {
      refs.add(selectedRef);
    }
    return Array.from(refs);
  }, [deployments, selectedRef]);
  const { data: deploymentLookup } = useQuery({
    queryKey: ['script-deployments-lookup', lookupCodeHashes],
    queryFn: () => api.lookupScripts(lookupCodeHashes),
    enabled: lookupCodeHashes.length > 0,
    staleTime: Infinity,
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
      const selectedByRef = selectedRef
        ? sortedDeployments.find((deployment) => {
            const normalizedHashType = normalizeScriptRefHashType(deployment.hashType);
            if (
              deployment.codeHash === selectedRef &&
              (!selectedRefHashType || selectedRefHashType === normalizedHashType)
            ) {
              return true;
            }
            const refs = deploymentReferenceHashes(
              deployment,
              deploymentLookup?.[deployment.codeHash]
            );
            const matchesTypeRef =
              refs.typeRef === selectedRef &&
              (!selectedRefHashType || selectedRefHashType === 'type');
            const matchesDataRef =
              refs.dataRef === selectedRef &&
              (!selectedRefHashType || selectedRefHashType === refs.dataRefType);
            return matchesTypeRef || matchesDataRef;
          })
        : null;
      const firstWithCells = sortedDeployments.find((d) => {
        const normalizedHashType = normalizeScriptRefHashType(d.hashType);
        const stats = usageByCodeHash.get(d.codeHash);
        return normalizedHashType && stats && stats.liveCellsCount > 0;
      });
      const target = selectedByRef || firstWithCells || sortedDeployments[0];
      const stats = usageByCodeHash.get(target.codeHash);
      let hashType = normalizeScriptRefHashType(target.hashType);
      if (selectedByRef && selectedRef && selectedRefHashType) {
        const selectedRefs = deploymentReferenceHashes(target, deploymentLookup?.[target.codeHash]);
        if (selectedRefHashType === 'type' && selectedRefs.typeRef === selectedRef) {
          hashType = 'type';
        } else if (
          selectedRefHashType !== 'type' &&
          selectedRefs.dataRef === selectedRef &&
          selectedRefs.dataRefType === selectedRefHashType
        ) {
          hashType = selectedRefHashType;
        }
      }
      if (hashType) {
        setSelectedDeployment({
          codeHash: target.codeHash,
          hashType,
          scriptKind: stats?.scriptKind as 'lock' | 'type' | undefined,
        });
      }
    }
  }, [deploymentLookup, deployments, selectedDeployment, selectedRef, selectedRefHashType, usage]);

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
  const selectedDeploymentInfo =
    selectedDeployment &&
    sortedDeployments.find((d) => {
      const hashType = normalizeScriptRefHashType(d.hashType);
      return d.codeHash === selectedDeployment.codeHash && hashType === selectedDeployment.hashType;
    });
  const selectedDeploymentRefs = selectedDeploymentInfo
    ? deploymentReferenceHashes(
        selectedDeploymentInfo,
        deploymentLookup?.[selectedDeploymentInfo.codeHash]
      )
    : {
        typeRef: selectedDeployment?.hashType === 'type' ? selectedDeployment.codeHash : null,
        dataRef: selectedDeployment?.hashType !== 'type' ? selectedDeployment?.codeHash : null,
        dataRefType:
          selectedDeployment?.hashType && selectedDeployment.hashType !== 'type'
            ? selectedDeployment.hashType
            : 'data',
      };

  const handleDeploymentClick = (deployment: KnownScript) => {
    const hashType = normalizeScriptRefHashType(deployment.hashType);
    if (!hashType) return;
    const stats = usageByCodeHash.get(deployment.codeHash);
    const newSelected = {
      codeHash: deployment.codeHash,
      hashType,
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
    selectedDeployment?.hashType === normalizeScriptRefHashType(deployment.hashType);

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
                <Badge variant="neutral">{inferredScriptKind.toUpperCase()}</Badge>
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
                  className="text-terminal-green rounded border border-slate-700 px-3 py-1 font-mono text-sm transition-colors hover:border-slate-600 hover:bg-slate-900"
                >
                  RFC
                </a>
              )}
              {scriptInfo.website && (
                <a
                  href={scriptInfo.website}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-terminal-green rounded border border-slate-700 px-3 py-1 font-mono text-sm transition-colors hover:border-slate-600 hover:bg-slate-900"
                >
                  Website
                </a>
              )}
              {scriptInfo.sourceUrl && (
                <a
                  href={scriptInfo.sourceUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-terminal-green rounded border border-slate-700 px-3 py-1 font-mono text-sm transition-colors hover:border-slate-600 hover:bg-slate-900"
                >
                  Source
                </a>
              )}
            </div>
          }
        />

        <TerminalPanel className="mb-6 border-slate-700/80">
          <TerminalPanelHeader indicator="active">Deployments</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div
              data-testid="script-ref-semantics"
              className="grid gap-3 border-b border-slate-800 px-4 py-4 md:grid-cols-3"
            >
              <div className="rounded-md border border-slate-800 bg-slate-900/60 p-3">
                <div className="mb-1 font-mono text-[11px] uppercase tracking-wider text-slate-500">
                  Script Ref
                </div>
                <div className="font-mono text-xs text-slate-300">type ref</div>
                <div className="mt-1 text-xs text-slate-400">
                  Resolves by type script hash. Upgradeable flow, executes on latest CKB-VM.
                </div>
              </div>
              <div className="rounded-md border border-slate-800 bg-slate-900/60 p-3">
                <div className="mb-1 font-mono text-[11px] uppercase tracking-wider text-slate-500">
                  Script Ref
                </div>
                <div className="font-mono text-xs text-slate-300">data/data1/data2</div>
                <div className="mt-1 text-xs text-slate-400">
                  Resolves by bytecode hash. Immutable binary, VM version fixed to v0/v1/v2.
                </div>
              </div>
              <div className="rounded-md border border-slate-800 bg-slate-900/60 p-3">
                <div className="mb-1 font-mono text-[11px] uppercase tracking-wider text-slate-500">
                  Tradeoff
                </div>
                <div className="text-xs text-slate-400">
                  `type` favors upgradability; `data` family favors deterministic, reproducible
                  execution.
                </div>
                <a
                  href="https://docs.nervos.org/docs/tech-explanation/data-type-diff"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-terminal-green mt-2 inline-block text-xs hover:underline"
                >
                  Reference doc: data vs type hash semantics
                </a>
              </div>
            </div>
            <div className="overflow-x-auto">
              <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                <div className="flex-1">Deployment</div>
                <div className="w-56">Deployed At</div>
                <div className="w-24">Kind</div>
                <div className="w-24 text-right">Cells</div>
                <div className="w-32 text-right">Capacity</div>
              </div>
              {sortedDeployments.map((deployment, idx) => {
                const stats = usageByCodeHash.get(deployment.codeHash);
                const selected = isSelected(deployment);
                const refs = deploymentReferenceHashes(
                  deployment,
                  deploymentLookup?.[deployment.codeHash]
                );
                return (
                  <TerminalRow
                    key={idx}
                    className={`cursor-pointer ${selected ? 'bg-terminal-green/10 ring-terminal-green/30 ring-1 ring-inset' : ''}`}
                  >
                    <div
                      className="flex w-full items-center gap-3"
                      onClick={() => handleDeploymentClick(deployment)}
                    >
                      <div className="min-w-0 flex-1 py-0.5">
                        <div className="mb-2">
                          {deployment.codeCellTxHash && deployment.codeCellOutputIndex !== null ? (
                            <Link
                              href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                              onClick={(e) => e.stopPropagation()}
                              className="text-terminal-green text-xs hover:underline"
                            >
                              <HexDisplay
                                value={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                                color="accent"
                                startChars={8}
                                endChars={8}
                              />
                            </Link>
                          ) : (
                            <span className="text-slate-500">-</span>
                          )}
                        </div>
                        <div className="space-y-1 text-xs">
                          <div className="flex items-center gap-2">
                            <span className="inline-flex rounded border border-slate-700/80 bg-slate-800/70 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-slate-400">
                              type
                            </span>
                            {refs.typeRef ? (
                              <HexDisplay
                                value={refs.typeRef}
                                color="accent"
                                size="sm"
                                startChars={10}
                                endChars={8}
                              />
                            ) : (
                              <span className="font-mono text-slate-500">Unavailable</span>
                            )}
                          </div>
                          <div className="flex items-center gap-2">
                            <span className="inline-flex rounded border border-slate-700/80 bg-slate-800/70 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-slate-400">
                              {getScriptRefBadgeLabel(refs.dataRefType)}
                            </span>
                            {refs.dataRef ? (
                              <HexDisplay
                                value={refs.dataRef}
                                color="accent"
                                size="sm"
                                startChars={10}
                                endChars={8}
                              />
                            ) : (
                              <span className="font-mono text-slate-500">Unavailable</span>
                            )}
                          </div>
                        </div>
                      </div>
                      <div
                        className="w-56 font-mono text-xs text-slate-400"
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
                      <div className="w-24 text-slate-400">
                        {stats?.scriptKind ? (
                          <Badge variant="neutral" className="px-1.5 py-0.5 text-[10px]">
                            {stats.scriptKind}
                          </Badge>
                        ) : (
                          <span className="text-slate-500">-</span>
                        )}
                      </div>
                      <div className="w-24 text-right font-mono tabular-nums text-slate-300">
                        {stats ? (
                          <span title={`Total: ${formatNumber(stats.cellsCount)}`}>
                            {formatNumber(stats.liveCellsCount)}
                          </span>
                        ) : (
                          '-'
                        )}
                      </div>
                      <div className="w-32 text-right font-mono tabular-nums text-slate-300">
                        {stats ? (
                          <span title={`${formatCkbCompact(stats.liveCapacitySum).full} CKB`}>
                            {formatCkbCompact(stats.liveCapacitySum).value} CKB
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
                  <div className="sticky bottom-0 z-10 flex border-t border-slate-700 bg-slate-950/95 px-4 py-3 font-medium backdrop-blur">
                    <div className="flex-1 text-slate-400">Total</div>
                    <div className="w-56" />
                    <div className="w-24" />
                    <div className="text-terminal-green w-24 text-right font-mono tabular-nums">
                      <span title={`Total: ${formatNumber(usage.cellsCount)}`}>
                        {formatNumber(usage.liveCellsCount)}
                      </span>
                    </div>
                    <div className="text-terminal-green w-32 text-right font-mono tabular-nums">
                      <span title={`${formatCkbCompact(usage.liveCapacitySum).full} CKB`}>
                        {formatCkbCompact(usage.liveCapacitySum).value} CKB
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
                  <span className="text-slate-500">|</span>
                  <div
                    data-testid="capacity-selected-refs"
                    className="flex flex-wrap items-center gap-2 text-xs"
                  >
                    <Badge variant="gray">type</Badge>
                    {selectedDeploymentRefs.typeRef ? (
                      <HexDisplay
                        value={selectedDeploymentRefs.typeRef}
                        color="accent"
                        size="sm"
                        startChars={10}
                        endChars={8}
                      />
                    ) : (
                      <span className="font-mono text-slate-500">Unavailable</span>
                    )}
                    <Badge variant="gray">
                      {getScriptRefBadgeLabel(selectedDeploymentRefs.dataRefType)}
                    </Badge>
                    {selectedDeploymentRefs.dataRef ? (
                      <HexDisplay
                        value={selectedDeploymentRefs.dataRef}
                        color="accent"
                        size="sm"
                        startChars={10}
                        endChars={8}
                      />
                    ) : (
                      <span className="font-mono text-slate-500">Unavailable</span>
                    )}
                  </div>
                </div>
              </TerminalPanelHeader>
              <TerminalPanelContent padding="none">
                {selectedDeploymentUsage && (
                  <div className="border-b border-slate-800 px-4 py-4">
                    <CapacityUtilization
                      totalCapacity={selectedDeploymentUsage.liveCapacitySum}
                      occupiedCapacity={selectedDeploymentUsage.liveOccupiedCapacitySum}
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
                      valueUnit="shannon"
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
                  <span className="text-slate-500">|</span>
                  <div
                    data-testid="cells-selected-refs"
                    className="flex flex-wrap items-center gap-2 text-xs"
                  >
                    <Badge variant="gray">type</Badge>
                    {selectedDeploymentRefs.typeRef ? (
                      <HexDisplay
                        value={selectedDeploymentRefs.typeRef}
                        color="accent"
                        size="sm"
                        startChars={10}
                        endChars={8}
                      />
                    ) : (
                      <span className="font-mono text-slate-500">Unavailable</span>
                    )}
                    <Badge variant="gray">
                      {getScriptRefBadgeLabel(selectedDeploymentRefs.dataRefType)}
                    </Badge>
                    {selectedDeploymentRefs.dataRef ? (
                      <HexDisplay
                        value={selectedDeploymentRefs.dataRef}
                        color="accent"
                        size="sm"
                        startChars={10}
                        endChars={8}
                      />
                    ) : (
                      <span className="font-mono text-slate-500">Unavailable</span>
                    )}
                  </div>
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
                              <HexDisplay
                                value={`${cell.txHash}:${cell.outputIndex}`}
                                color="accent"
                              />
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
                              className="text-terminal-green hover:underline"
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
