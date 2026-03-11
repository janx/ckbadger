'use client';
import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import dynamic from '@/lib/dynamic-client';
import { useParams, usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { UsageBar } from '@/components/ui/progress-bar';
import { HexDisplay } from '@/components/ui/hex-display';
import { Capacity } from '@/components/ui/capacity';
import { Address } from '@/components/ui/address';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { api, type CellDep, type GraphNode, type ScriptLookupResponse } from '@/lib/api';
import { getScriptRefBadgeLabel, getScriptRefQueryHashType } from '@/lib/script-ref';
import { formatTimeAgo, formatCkbAmount } from '@/lib/utils';
import { analyzeWitness, buildScriptGroupLens } from '@/lib/witness-analysis';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';
type TxGraphView = 'flow' | 'graph';
const SECTION_TAB_VALUES = ['io', 'scripts', 'celldeps', 'graph'] as const;
type SectionTab = (typeof SECTION_TAB_VALUES)[number];
const SECTION_TAB_TITLES: Record<SectionTab, string> = {
  io: 'Inputs/Outputs',
  scripts: 'Scripts',
  celldeps: 'Cell Deps',
  graph: 'Graph',
};
const DeferredCellGraph = dynamic(() => import('@/components/cell-graph'), {
  loading: () => (
    <div className="border-base-border/70 bg-base-surface/70 flex h-[240px] items-center justify-center rounded border">
      <p className="text-text-dim text-sm">Loading graph section...</p>
    </div>
  ),
});
const WITNESS_BYTES_PER_ROW = 24;
const EMPTY_WITNESSES: string[] = [];
const WITNESS_SEGMENT_TONES = [
  {
    dot: 'bg-emphasis',
    activePill: 'border-emphasis/70 bg-emphasis/15 text-emphasis',
    valueText: 'text-emphasis',
    byte: 'rounded bg-emphasis/15 text-emphasis/70',
    byteActive: 'rounded bg-emphasis/25 text-emphasis ring-1 ring-emphasis/70',
    byteHover: 'byte-hover-breathe ring-1 ring-emphasis/80 shadow-[0_0_10px_rgba(0,255,65,0.35)]',
    asciiActive: 'rounded-sm bg-emphasis/20 text-emphasis',
    asciiHover:
      'rounded-sm bg-emphasis/30 text-emphasis shadow-[inset_0_0_0_1px_rgba(0,255,65,0.45)]',
  },
  {
    dot: 'bg-info',
    activePill: 'border-info/70 bg-info/15 text-info',
    valueText: 'text-info',
    byte: 'rounded bg-info/15 text-info-dim',
    byteActive: 'rounded bg-info/25 text-info ring-1 ring-info/70',
    byteHover: 'byte-hover-breathe ring-1 ring-info/80 shadow-[0_0_10px_rgba(58,110,160,0.35)]',
    asciiActive: 'rounded-sm bg-info/20 text-info',
    asciiHover: 'rounded-sm bg-info/30 text-info shadow-[inset_0_0_0_1px_rgba(58,110,160,0.5)]',
  },
  {
    dot: 'bg-warning-400',
    activePill: 'border-warning-400/70 bg-warning/15 text-warning',
    valueText: 'text-warning',
    byte: 'rounded bg-warning/15 text-warning',
    byteActive: 'rounded bg-warning/25 text-warning ring-1 ring-warning/70',
    byteHover: 'byte-hover-breathe ring-1 ring-warning/80 shadow-[0_0_10px_rgba(251,191,36,0.35)]',
    asciiActive: 'rounded-sm bg-warning/20 text-warning',
    asciiHover:
      'rounded-sm bg-warning/30 text-warning-50 shadow-[inset_0_0_0_1px_rgba(251,191,36,0.5)]',
  },
  {
    dot: 'bg-[#9a5090]',
    activePill: 'border-[#9a5090]/70 bg-[#9a5090]/15 text-[#7a4070]',
    valueText: 'text-[#7a4070]',
    byte: 'rounded bg-[#9a5090]/15 text-[#7a4070]',
    byteActive: 'rounded bg-[#9a5090]/25 text-[#6a3060] ring-1 ring-[#9a5090]/70',
    byteHover:
      'byte-hover-breathe ring-1 ring-[#9a5090]/80 shadow-[0_0_10px_rgba(154,80,144,0.35)]',
    asciiActive: 'rounded-sm bg-[#9a5090]/20 text-[#6a3060]',
    asciiHover:
      'rounded-sm bg-[#9a5090]/30 text-[#6a3060] shadow-[inset_0_0_0_1px_rgba(154,80,144,0.5)]',
  },
] as const;
function getWitnessSegmentTone(segmentIndex: number) {
  return WITNESS_SEGMENT_TONES[Math.abs(segmentIndex) % WITNESS_SEGMENT_TONES.length];
}
function getPreferredSegmentLabelsByScriptGroupKind(kind: 'lock' | 'type'): string[] {
  return kind === 'lock' ? ['lock'] : ['inputType', 'outputType'];
}
function findPreferredSegmentIndex(
  kind: 'lock' | 'type',
  segments: Array<{ label: string }>
): number | null {
  const labels = getPreferredSegmentLabelsByScriptGroupKind(kind);
  const index = segments.findIndex((segment) =>
    labels.some((label) => segment.label === label || segment.label.startsWith(`${label}.`))
  );
  return index >= 0 ? index : null;
}
function isSectionTabValue(value: string | null): value is SectionTab {
  return value !== null && (SECTION_TAB_VALUES as readonly string[]).includes(value);
}
function parseNonNegativeInt(value: string | null): number | null {
  if (value === null || !/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) return null;
  return parsed;
}
function normalizeScriptHashType(hashType: string | undefined): string {
  return hashType ?? 'unknown';
}
function normalizeScriptArgs(args: string | undefined): string {
  return args ?? '0x';
}
function getErrorMessage(error: unknown): string | null {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message.trim();
  }
  if (typeof error === 'string' && error.trim().length > 0) {
    return error.trim();
  }
  return null;
}
export default function TransactionDetailPage() {
  const params = useParams();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const router = useRouter();
  const hash = params.hash as string;
  const tabFromQuery = searchParams.get('tab');
  const [activeSectionTab, setActiveSectionTab] = useState<SectionTab>(() =>
    isSectionTabValue(tabFromQuery) ? tabFromQuery : 'io'
  );
  const {
    data: tx,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['transaction', hash],
    queryFn: () => api.getTransactionDetail(hash),
  });
  const errorMessage = getErrorMessage(error);
  const isNotFoundError = errorMessage?.startsWith('API error: 404') ?? false;
  const { cycles, hasCycles, isCalculating, hasFailed } = useCyclesCalculation(
    hash,
    tx?.cycles,
    tx?.isCellbase ?? false
  );
  const { data: graphData } = useQuery({
    queryKey: ['txGraph', hash],
    queryFn: () => api.getTransactionGraph(hash),
    enabled: !!hash,
  });
  const [txGraphView, setTxGraphView] = useState<TxGraphView>('flow');
  const { data: cellDeps, isLoading: cellDepsLoading } = useQuery({
    queryKey: ['txCellDeps', hash],
    queryFn: () => api.getTransactionCellDeps(hash),
    enabled: !!hash,
  });
  const { data: lifecycle } = useQuery({
    queryKey: ['txLifecycle', hash],
    queryFn: () => api.getTransactionLifecycle(hash),
    enabled: !!hash && !!tx && !tx.isCellbase,
  });
  const codeHashes = useMemo(() => {
    if (!tx) return [];
    const hashes = new Set<string>();
    tx.inputs?.forEach((input) => {
      if (input.lock?.codeHash) hashes.add(input.lock.codeHash);
      if (input.type?.codeHash) hashes.add(input.type.codeHash);
    });
    tx.outputs?.forEach((output) => {
      if (output.lock?.codeHash) hashes.add(output.lock.codeHash);
      if (output.type?.codeHash) hashes.add(output.type.codeHash);
    });
    return Array.from(hashes);
  }, [tx]);
  const { data: scriptLookup } = useQuery({
    queryKey: ['scriptLookup', codeHashes],
    queryFn: () => api.lookupScripts(codeHashes),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const graphInsights = useMemo(() => {
    if (!graphData) {
      return {
        nodeCount: 0,
        linkCount: 0,
        inputLinkCount: 0,
        outputLinkCount: 0,
        outputNodes: [] as Array<{
          outputIndex: number;
          status: string | null;
          capacity: string | null;
        }>,
        graphHeight: 240,
      };
    }
    const inputLinkCount = graphData.links.filter(
      (link) => link.linkType === 'input' || link.linkType === 'consumed_by'
    ).length;
    const outputLinkCount = graphData.links.filter(
      (link) => link.linkType === 'output' || link.linkType === 'creates'
    ).length;
    const outputNodes = graphData.nodes
      .filter(
        (node) =>
          node.nodeType === 'cell' &&
          node.data?.txHash === hash &&
          node.data?.outputIndex !== undefined
      )
      .map((node) => ({
        outputIndex: node.data?.outputIndex ?? 0,
        status: node.data?.status ?? null,
        capacity: node.data?.capacity ?? null,
      }))
      .sort((a, b) => a.outputIndex - b.outputIndex);
    const graphHeight = Math.min(
      340,
      Math.max(220, 200 + graphData.nodes.length * 10 + graphData.links.length * 6)
    );
    return {
      nodeCount: graphData.nodes.length,
      linkCount: graphData.links.length,
      inputLinkCount,
      outputLinkCount,
      outputNodes,
      graphHeight,
    };
  }, [graphData, hash]);
  const handleGraphNodeClick = (node: GraphNode) => {
    if (node.nodeType === 'transaction' && node.data?.hash) {
      router.push(`/tx/${node.data.hash}`);
    } else if (
      node.nodeType === 'cell' &&
      node.data?.txHash !== undefined &&
      node.data?.outputIndex !== undefined
    ) {
      router.push(`/cell/${node.data.txHash}-${node.data.outputIndex}`);
    }
  };
  useEffect(() => {
    const nextTab = isSectionTabValue(tabFromQuery) ? tabFromQuery : 'io';
    if (nextTab !== activeSectionTab) {
      setActiveSectionTab(nextTab);
    }
  }, [activeSectionTab, tabFromQuery]);
  const updateSearchParams = (mutator: (nextParams: URLSearchParams) => void) => {
    const nextParams = new URLSearchParams(searchParams.toString());
    mutator(nextParams);
    const nextQuery = nextParams.toString();
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  };
  const handleSectionTabChange = (nextValue: string) => {
    if (!isSectionTabValue(nextValue)) return;
    setActiveSectionTab(nextValue);
    updateSearchParams((nextParams) => {
      if (nextValue === 'io') {
        nextParams.delete('tab');
      } else {
        nextParams.set('tab', nextValue);
      }
    });
  };
  const clearLinkedWitnessSelection = () => {
    setLinkedWitnessIndex(null);
    setLinkedScriptGroupKey(null);
    updateSearchParams((nextParams) => {
      nextParams.delete('witness');
      nextParams.delete('wg');
    });
  };
  const selectedWitnessFromQuery = parseNonNegativeInt(searchParams.get('witness'));
  const selectedScriptGroupFromQuery = searchParams.get('wg');
  const [linkedWitnessIndex, setLinkedWitnessIndex] = useState<number | null>(
    selectedWitnessFromQuery
  );
  const [linkedScriptGroupKey, setLinkedScriptGroupKey] = useState<string | null>(
    selectedScriptGroupFromQuery
  );
  useEffect(() => {
    setLinkedWitnessIndex(selectedWitnessFromQuery);
    setLinkedScriptGroupKey(selectedScriptGroupFromQuery);
  }, [hash, selectedScriptGroupFromQuery, selectedWitnessFromQuery]);
  const ioHighlightState = useMemo(() => {
    const highlightedInputIndices = new Set<number>();
    const highlightedOutputIndices = new Set<number>();
    if (!tx) return { highlightedInputIndices, highlightedOutputIndices };
    const groups = buildScriptGroupLens(tx);
    const focusedGroup =
      linkedScriptGroupKey !== null
        ? (groups.find((group) => group.key === linkedScriptGroupKey) ?? null)
        : null;
    const associatedGroups =
      focusedGroup !== null
        ? [focusedGroup]
        : linkedWitnessIndex !== null
          ? groups.filter((group) => group.witnessIndex === linkedWitnessIndex)
          : [];
    associatedGroups.forEach((group) => {
      group.inputIndices.forEach((inputIndex) => highlightedInputIndices.add(inputIndex));
    });
    if (associatedGroups.length === 0 && linkedWitnessIndex !== null && tx.inputs) {
      if (linkedWitnessIndex >= 0 && linkedWitnessIndex < tx.inputs.length) {
        highlightedInputIndices.add(linkedWitnessIndex);
      }
    }
    if (associatedGroups.length > 0 && tx.outputs) {
      tx.outputs.forEach((output, outputIndex) => {
        const isLinked = associatedGroups.some((group) => {
          const script = group.kind === 'lock' ? output.lock : output.type;
          if (!script?.codeHash) return false;
          return (
            script.codeHash === group.codeHash &&
            normalizeScriptHashType(script.hashType) === group.hashType &&
            normalizeScriptArgs(script.args) === group.args
          );
        });
        if (isLinked) highlightedOutputIndices.add(outputIndex);
      });
    }
    return { highlightedInputIndices, highlightedOutputIndices };
  }, [linkedScriptGroupKey, linkedWitnessIndex, tx]);
  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <div className="animate-pulse space-y-8">
            <div className="bg-base-surface h-20 w-full rounded" />
            <div className="bg-base-surface h-64 w-full rounded" />
            <div className="bg-base-surface h-96 w-full rounded" />
          </div>
        </main>
      </div>
    );
  }
  if (error || !tx) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">
                {isNotFoundError ? 'Transaction not found' : 'Failed to load transaction'}
              </h2>
              {!isNotFoundError && errorMessage && (
                <p className="text-text-dim mt-3 break-all text-sm">{errorMessage}</p>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader
          title="Transaction"
          hash={tx.hash}
          badge={<Badge variant="green">{tx.confirmations.toLocaleString()} Confirmations</Badge>}
        />
        <TerminalPanel className="mb-8" glow>
          <TerminalPanelHeader indicator="active">Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            <DataGrid>
              <DataField label="Block">
                {tx.isCellbase ? (
                  <Link
                    href={`/blocks/${tx.blockNumber}`}
                    className="text-interactive hover:underline"
                  >
                    #{tx.blockNumber.toLocaleString()}
                  </Link>
                ) : lifecycle?.proposedIn ? (
                  <div className="flex items-center gap-3">
                    <div className="flex items-center gap-1.5">
                      <span className="text-text-dim text-xs">Proposed</span>
                      <Link
                        href={`/blocks/${lifecycle.proposedIn.blockNumber}`}
                        className="text-text hover:text-text-bright hover:underline"
                      >
                        #{lifecycle.proposedIn.blockNumber.toLocaleString()}
                      </Link>
                    </div>
                    <span className="text-text-dim">→</span>
                    <div className="flex items-center gap-1.5">
                      <span className="text-text-dim text-xs">Committed</span>
                      <Link
                        href={`/blocks/${tx.blockNumber}`}
                        className="text-interactive font-medium hover:underline"
                      >
                        #{tx.blockNumber.toLocaleString()}
                      </Link>
                    </div>
                    {lifecycle.commitmentDistance !== null && (
                      <Badge
                        variant={
                          lifecycle.commitmentDistance <= 4
                            ? 'green'
                            : lifecycle.commitmentDistance <= 7
                              ? 'gold'
                              : 'red'
                        }
                      >
                        +{lifecycle.commitmentDistance}
                      </Badge>
                    )}
                  </div>
                ) : (
                  <Link
                    href={`/blocks/${tx.blockNumber}`}
                    className="text-interactive hover:underline"
                  >
                    #{tx.blockNumber.toLocaleString()}
                  </Link>
                )}
              </DataField>
              <DataField label="Timestamp" copyValue={new Date(tx.timestamp).toISOString()}>
                {new Date(tx.timestamp).toLocaleString()} ({formatTimeAgo(tx.timestamp)})
              </DataField>
              <DataField label="Type">
                {tx.isCellbase ? (
                  <Badge variant="neutral">Cellbase (Mining Reward)</Badge>
                ) : (
                  <Badge variant="neutral">Normal Transaction</Badge>
                )}
              </DataField>
              <DataField label="Fee">
                <div className="flex flex-col items-end gap-1">
                  <span className="text-text-bright">
                    <Capacity value={tx.fee} className="text-text-bright" />
                    <span className="text-text ml-2 font-mono text-sm tabular-nums">
                      ({BigInt(tx.fee).toLocaleString()} shannon)
                    </span>
                  </span>
                  {tx.feeRate && (
                    <span className="text-text font-mono text-xs tabular-nums">
                      {Number(tx.feeRate).toLocaleString()} shannon/KB
                    </span>
                  )}
                </div>
              </DataField>
              {tx.txSize && (
                <DataField label="Size">
                  <UsageBar value={tx.txSize} max={512000} unit="Bytes" />
                </DataField>
              )}
              {!tx.isCellbase && (
                <DataField label="Cycles">
                  {hasCycles && cycles !== null ? (
                    <UsageBar value={cycles} max={3_500_000_000} />
                  ) : isCalculating ? (
                    <span className="text-text inline-flex items-center gap-2 italic">
                      <span className="border-base-border inline-block h-3 w-3 animate-spin rounded-full border-2 border-t-transparent" />
                      <span className="cycles-calculating-marquee">Calculating ...</span>
                    </span>
                  ) : hasFailed ? (
                    <span className="text-negative italic">Calculation failed</span>
                  ) : (
                    <span className="text-text-dim italic">Not available</span>
                  )}
                </DataField>
              )}
              <DataField label="Carried Capacity">
                <Capacity
                  value={(BigInt(tx.outputsCapacity || '0') + BigInt(tx.fee)).toString()}
                  className="text-text-bright"
                />
              </DataField>
              <DataField label="Used Capacity Change">
                {(() => {
                  const inputUsed = BigInt(tx.inputsUsedCapacity || '0');
                  const outputUsed = BigInt(tx.outputsUsedCapacity || '0');
                  const change = outputUsed - inputUsed;
                  const zero = BigInt(0);
                  const isIncrease = change > zero;
                  const isDecrease = change < zero;
                  const absChange = change < zero ? -change : change;
                  const f = formatCkbAmount(absChange);
                  return (
                    <div className="flex items-center justify-end gap-2">
                      {isIncrease && (
                        <span className="border-positive/30 bg-positive/10 text-positive inline-flex items-center rounded border px-2 py-1 font-mono text-sm tabular-nums">
                          +{f.integer}
                          <span className="text-positive/60 text-[0.85em]">.{f.decimal}</span>
                          <span className="ml-1 text-[0.85em]">CKB</span>
                        </span>
                      )}
                      {isDecrease && (
                        <span className="border-negative/30 bg-negative/10 text-negative inline-flex items-center rounded border px-2 py-1 font-mono text-sm tabular-nums">
                          -{f.integer}
                          <span className="text-negative/60 text-[0.85em]">.{f.decimal}</span>
                          <span className="ml-1 text-[0.85em]">CKB</span>
                        </span>
                      )}
                      {!isIncrease && !isDecrease && (
                        <span className="border-base-border bg-base-elevated text-text inline-flex items-center rounded border px-2 py-1 text-sm">
                          No change
                        </span>
                      )}
                    </div>
                  );
                })()}
              </DataField>
            </DataGrid>
          </TerminalPanelContent>
        </TerminalPanel>
        <TerminalPanel className="mb-8">
          <Tabs value={activeSectionTab} onValueChange={handleSectionTabChange}>
            <TerminalPanelHeader
              indicator="active"
              actions={
                <TabsList>
                  <TabsTrigger value="io">
                    Inputs/Outputs ({tx.inputsCount} {'->'} {tx.outputsCount})
                  </TabsTrigger>
                  <TabsTrigger value="scripts">Scripts</TabsTrigger>
                  <TabsTrigger value="celldeps">Cell Deps</TabsTrigger>
                  <TabsTrigger value="graph">Graph</TabsTrigger>
                </TabsList>
              }
            >
              {SECTION_TAB_TITLES[activeSectionTab]}
            </TerminalPanelHeader>
            <TerminalPanelContent className="p-0">
              <TabsContent value="io" className="mt-0 p-0">
                <InputsOutputsTab
                  tx={tx}
                  scriptLookup={scriptLookup}
                  highlightedInputIndices={ioHighlightState.highlightedInputIndices}
                  highlightedOutputIndices={ioHighlightState.highlightedOutputIndices}
                  onHighlightedItemClick={clearLinkedWitnessSelection}
                />
              </TabsContent>
              <TabsContent value="scripts" className="mt-0 p-0">
                <ScriptsSummaryTab tx={tx} scriptLookup={scriptLookup} />
              </TabsContent>
              <TabsContent value="celldeps" className="mt-0 p-0">
                <CellDepsTab cellDeps={cellDeps} isLoading={cellDepsLoading} />
              </TabsContent>
              <TabsContent
                value="graph"
                className="border-base-border/80 bg-base-surface/40 mt-0 rounded border p-4"
              >
                <div className="space-y-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="border-base-border/70 bg-base-surface/60 inline-flex rounded border p-1">
                      <button
                        type="button"
                        className={`rounded px-2.5 py-1 text-xs transition-colors ${
                          txGraphView === 'flow'
                            ? 'bg-emphasis/15 text-emphasis'
                            : 'text-text hover:text-text-bright'
                        }`}
                        onClick={() => setTxGraphView('flow')}
                      >
                        Flow View
                      </button>
                      <button
                        type="button"
                        className={`rounded px-2.5 py-1 text-xs transition-colors ${
                          txGraphView === 'graph'
                            ? 'bg-emphasis/15 text-emphasis'
                            : 'text-text hover:text-text-bright'
                        }`}
                        onClick={() => setTxGraphView('graph')}
                      >
                        Graph View
                      </button>
                    </div>
                    {graphInsights.nodeCount > 0 && (
                      <span className="text-text-dim text-xs">
                        {graphInsights.nodeCount} nodes / {graphInsights.linkCount} links
                      </span>
                    )}
                  </div>
                  {txGraphView === 'flow' ? (
                    <div data-testid="tx-relationship-flow" className="space-y-3">
                      <div className="grid gap-3 sm:grid-cols-2">
                        <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                          <div className="text-text text-xs uppercase tracking-wide">
                            Inputs in Graph
                          </div>
                          <div className="text-text-bright mt-1 font-mono text-lg">
                            {graphInsights.inputLinkCount}
                          </div>
                        </div>
                        <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                          <div className="text-text text-xs uppercase tracking-wide">
                            Outputs in Graph
                          </div>
                          <div className="text-text-bright mt-1 font-mono text-lg">
                            {graphInsights.outputNodes.length}
                          </div>
                        </div>
                        <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                          <div className="text-text text-xs uppercase tracking-wide">
                            Live Outputs
                          </div>
                          <div className="text-emphasis mt-1 font-mono text-lg">
                            {
                              graphInsights.outputNodes.filter((node) => node.status === 'live')
                                .length
                            }
                          </div>
                        </div>
                        <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                          <div className="text-text text-xs uppercase tracking-wide">
                            Dead Outputs
                          </div>
                          <div className="text-negative mt-1 font-mono text-lg">
                            {
                              graphInsights.outputNodes.filter((node) => node.status === 'dead')
                                .length
                            }
                          </div>
                        </div>
                      </div>
                      <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                        <div className="text-text text-xs uppercase tracking-wide">
                          Transaction Flow Snapshot
                        </div>
                        <div className="text-text mt-2 text-sm">
                          Inputs:{' '}
                          <span className="text-text-bright font-mono">{tx.inputsCount}</span>{' '}
                          {'->'} Outputs:{' '}
                          <span className="text-text-bright font-mono">{tx.outputsCount}</span> |
                          Graph Edges:{' '}
                          <span className="text-text-bright font-mono">
                            {graphInsights.outputLinkCount}
                          </span>
                        </div>
                      </div>
                    </div>
                  ) : graphData && graphData.nodes.length > 0 ? (
                    <DeferredCellGraph
                      nodes={graphData.nodes}
                      links={graphData.links}
                      onNodeClick={handleGraphNodeClick}
                      width={undefined}
                      height={graphInsights.graphHeight}
                    />
                  ) : (
                    <p className="text-text-dim py-8 text-center">Loading graph...</p>
                  )}
                </div>
              </TabsContent>
            </TerminalPanelContent>
          </Tabs>
        </TerminalPanel>
        <TerminalPanel>
          <TerminalPanelHeader indicator="active">
            Witness ({tx.witnesses?.length ?? 0})
          </TerminalPanelHeader>
          <TerminalPanelContent className="p-0">
            <WitnessTab
              tx={tx}
              scriptLookup={scriptLookup}
              onSelectionChange={(witnessIndex, groupKey) => {
                setLinkedWitnessIndex(witnessIndex);
                setLinkedScriptGroupKey(groupKey);
              }}
            />
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
interface TabProps {
  tx: NonNullable<Awaited<ReturnType<typeof api.getTransactionDetail>>>;
  scriptLookup?: ScriptLookupResponse;
}
interface InputsOutputsTabProps extends TabProps {
  highlightedInputIndices?: Set<number>;
  highlightedOutputIndices?: Set<number>;
  onHighlightedItemClick?: () => void;
}
interface WitnessTabProps extends TabProps {
  onSelectionChange?: (witnessIndex: number | null, groupKey: string | null) => void;
}
const UNKNOWN_SCRIPT_NAME = 'unknown';
function hasKnownScriptName(name: string | null | undefined): boolean {
  return Boolean(name && name.trim() && name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
}
function getScriptHref({
  codeHash,
  hashType,
  scriptKind,
  scriptName,
}: {
  codeHash: string;
  hashType: string | null | undefined;
  scriptKind: 'lock' | 'type';
  scriptName: string | null | undefined;
}): string {
  if (hasKnownScriptName(scriptName)) {
    return `/scripts/${encodeURIComponent(scriptName!.trim())}`;
  }
  return `/script/${codeHash}?hashType=${encodeURIComponent(getScriptRefQueryHashType(hashType))}&kind=${scriptKind}`;
}
function ScriptLabel({
  script,
  scriptLookup,
  type,
}: {
  script: { codeHash: string; hashType?: string } | undefined;
  scriptLookup?: ScriptLookupResponse;
  type: 'lock' | 'type';
}) {
  if (!script) return null;
  const info = scriptLookup?.[script.codeHash];
  const trimmedScriptName = info?.name?.trim();
  if (!trimmedScriptName || trimmedScriptName.toLowerCase() === UNKNOWN_SCRIPT_NAME) return null;
  const label = trimmedScriptName;
  const href = getScriptHref({
    codeHash: script.codeHash,
    hashType: info?.hashType ?? script.hashType,
    scriptKind: type,
    scriptName: trimmedScriptName,
  });
  return (
    <Link href={href}>
      <Badge variant="neutral" className="hover:opacity-80">
        {label}
      </Badge>
    </Link>
  );
}
function InputsOutputsTab({
  tx,
  scriptLookup,
  highlightedInputIndices,
  highlightedOutputIndices,
  onHighlightedItemClick,
}: InputsOutputsTabProps) {
  return (
    <div className="grid gap-6 p-4 lg:grid-cols-2">
      <div>
        <h4 className="border-base-border text-text-dim mb-3 border-b pb-2 font-mono text-xs uppercase tracking-wider">
          Inputs ({tx.inputsCount})
        </h4>
        {tx.inputs && tx.inputs.length > 0 ? (
          <div className="border-base-border bg-base-surface/50 rounded-lg border">
            {tx.inputs.map((input, index) => {
              const isHighlighted = highlightedInputIndices?.has(index) ?? false;
              return (
                <TerminalRow
                  key={index}
                  data-testid={`tx-io-input-${index}`}
                  onClick={(event) => {
                    if (!isHighlighted) return;
                    const clickedElement = event.target as HTMLElement | null;
                    if (clickedElement?.closest('a')) return;
                    onHighlightedItemClick?.();
                  }}
                  className={`flex flex-col gap-2 ${
                    isHighlighted
                      ? 'io-linked-highlight border-emphasis/70 bg-emphasis/10 ring-emphasis/30 cursor-pointer ring-1'
                      : ''
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim font-mono text-xs">#{index}</span>
                      <ScriptLabel script={input.lock} scriptLookup={scriptLookup} type="lock" />
                      <ScriptLabel script={input.type} scriptLookup={scriptLookup} type="type" />
                    </div>
                    {input.previousOutput && (
                      <Link
                        href={`/cell/${input.previousOutput.txHash}-${input.previousOutput.index}`}
                        className="hover:text-interactive text-text group flex items-center gap-1 font-mono text-xs"
                      >
                        <HexDisplay
                          value={input.previousOutput.txHash}
                          startChars={8}
                          endChars={6}
                          size="sm"
                          copyable={false}
                        />
                        <span>:{input.previousOutput.index}</span>
                      </Link>
                    )}
                  </div>
                  {(input.address || input.capacity) && (
                    <div className="flex items-center justify-between">
                      {input.address ? (
                        <Address address={input.address} />
                      ) : (
                        <span className="text-negative text-sm">Address error</span>
                      )}
                      {input.capacity && (
                        <Capacity value={input.capacity} className="text-text text-sm" />
                      )}
                    </div>
                  )}
                </TerminalRow>
              );
            })}
          </div>
        ) : tx.isCellbase ? (
          <p className="text-text-dim text-sm">Cellbase has no inputs</p>
        ) : (
          <p className="text-text-dim text-sm">Loading inputs...</p>
        )}
      </div>
      <div>
        <h4 className="border-base-border text-text-dim mb-3 border-b pb-2 font-mono text-xs uppercase tracking-wider">
          Outputs ({tx.outputsCount})
        </h4>
        {tx.outputs && tx.outputs.length > 0 ? (
          <div className="border-base-border bg-base-surface/50 rounded-lg border">
            {tx.outputs.map((output, index) => {
              const isHighlighted = highlightedOutputIndices?.has(index) ?? false;
              return (
                <TerminalRow
                  key={index}
                  data-testid={`tx-io-output-${index}`}
                  onClick={(event) => {
                    if (!isHighlighted) return;
                    const clickedElement = event.target as HTMLElement | null;
                    if (clickedElement?.closest('a')) return;
                    onHighlightedItemClick?.();
                  }}
                  className={`flex flex-col gap-2 ${
                    isHighlighted
                      ? 'io-linked-highlight border-emphasis/70 bg-emphasis/10 ring-emphasis/30 cursor-pointer ring-1'
                      : ''
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="text-text-dim font-mono text-xs">#{index}</span>
                      <ScriptLabel script={output.lock} scriptLookup={scriptLookup} type="lock" />
                      <ScriptLabel script={output.type} scriptLookup={scriptLookup} type="type" />
                    </div>
                    <Link
                      href={`/cell/${tx.hash}-${index}`}
                      className="text-interactive font-mono text-xs hover:underline"
                    >
                      View Cell
                    </Link>
                  </div>
                  <div className="flex items-center justify-between">
                    {output.address ? (
                      <Address address={output.address} />
                    ) : (
                      <span className="text-negative text-sm">Address error</span>
                    )}
                    <Capacity value={output.capacity} className="text-text" />
                  </div>
                </TerminalRow>
              );
            })}
          </div>
        ) : (
          <p className="text-text-dim text-sm">Loading outputs...</p>
        )}
      </div>
    </div>
  );
}
function WitnessTab({ tx, scriptLookup, onSelectionChange }: WitnessTabProps) {
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const router = useRouter();
  const witnesses = tx.witnesses ?? EMPTY_WITNESSES;
  const witnessesAvailable = tx.witnessesAvailable ?? witnesses.length > 0;
  const witnessFromQuery = parseNonNegativeInt(searchParams.get('witness'));
  const scriptGroupFromQuery = searchParams.get('wg');
  const witnessAnalyses = useMemo(
    () => witnesses.map((witness, index) => analyzeWitness(witness, index, tx.inputsCount)),
    [tx.inputsCount, witnesses]
  );
  const scriptGroupLens = useMemo(() => buildScriptGroupLens(tx), [tx]);
  const [activeWitnessIndex, setActiveWitnessIndex] = useState<number | null>(
    () => witnessFromQuery
  );
  const [hoveredSegmentIndex, setHoveredSegmentIndex] = useState<number | null>(null);
  const [pinnedSegmentIndex, setPinnedSegmentIndex] = useState<number | null>(null);
  const [hoveredByteOffset, setHoveredByteOffset] = useState<number | null>(null);
  const [expandedHeuristicIndex, setExpandedHeuristicIndex] = useState<number | null>(null);
  const [activeScriptGroupKey, setActiveScriptGroupKey] = useState<string | null>(
    () => scriptGroupFromQuery
  );
  useEffect(() => {
    setActiveWitnessIndex(witnessFromQuery);
    setHoveredSegmentIndex(null);
    setPinnedSegmentIndex(null);
    setHoveredByteOffset(null);
    setExpandedHeuristicIndex(null);
    setActiveScriptGroupKey(scriptGroupFromQuery);
  }, [scriptGroupFromQuery, tx.hash, witnessFromQuery]);
  useEffect(() => {
    setHoveredSegmentIndex(null);
    setPinnedSegmentIndex(null);
    setHoveredByteOffset(null);
    setExpandedHeuristicIndex(null);
  }, [activeWitnessIndex]);
  useEffect(() => {
    if (activeWitnessIndex === null) return;
    if (witnessAnalyses.length === 0) {
      setActiveWitnessIndex(null);
      return;
    }
    if (activeWitnessIndex < witnessAnalyses.length) return;
    setActiveWitnessIndex(witnessAnalyses.length - 1);
  }, [activeWitnessIndex, witnessAnalyses.length]);
  useEffect(() => {
    const normalizedGroupKey =
      scriptGroupFromQuery && scriptGroupLens.some((group) => group.key === scriptGroupFromQuery)
        ? scriptGroupFromQuery
        : null;
    if (normalizedGroupKey !== activeScriptGroupKey) {
      setActiveScriptGroupKey(normalizedGroupKey);
    }
  }, [activeScriptGroupKey, scriptGroupFromQuery, scriptGroupLens]);
  useEffect(() => {
    if (activeScriptGroupKey === null) return;
    const linkedGroup = scriptGroupLens.find((group) => group.key === activeScriptGroupKey);
    if (!linkedGroup) return;
    if (linkedGroup.witnessIndex !== activeWitnessIndex) {
      setActiveWitnessIndex(linkedGroup.witnessIndex);
    }
  }, [activeScriptGroupKey, activeWitnessIndex, scriptGroupLens]);
  const activeWitness =
    activeWitnessIndex !== null ? (witnessAnalyses[activeWitnessIndex] ?? null) : null;
  const activeScriptGroup =
    activeScriptGroupKey !== null
      ? (scriptGroupLens.find((group) => group.key === activeScriptGroupKey) ?? null)
      : null;
  const activeDeterministic = activeWitness?.deterministic ?? null;
  const syncWitnessQuery = (nextWitnessIndex: number | null, nextGroupKey: string | null) => {
    const nextParams = new URLSearchParams(searchParams.toString());
    if (nextWitnessIndex === null) {
      nextParams.delete('witness');
      nextParams.delete('wg');
    } else {
      nextParams.set('witness', String(nextWitnessIndex));
      if (nextGroupKey) {
        nextParams.set('wg', nextGroupKey);
      } else {
        nextParams.delete('wg');
      }
    }
    const nextQuery = nextParams.toString();
    router.replace(nextQuery ? `${pathname}?${nextQuery}` : pathname, { scroll: false });
  };
  useEffect(() => {
    if (!activeScriptGroup || !activeDeterministic) return;
    if (activeWitnessIndex === null || activeScriptGroup.witnessIndex !== activeWitnessIndex)
      return;
    const preferredSegmentIndex = findPreferredSegmentIndex(
      activeScriptGroup.kind,
      activeDeterministic.segments
    );
    if (preferredSegmentIndex === null) return;
    setPinnedSegmentIndex(preferredSegmentIndex);
    setHoveredSegmentIndex(null);
  }, [activeScriptGroup, activeDeterministic, activeWitnessIndex]);
  if (!witnessesAvailable && witnessAnalyses.length === 0) {
    return (
      <div className="p-4" data-testid="tx-witness-tab">
        <div className="border-base-border bg-base-surface/60 text-text rounded border p-4 text-sm">
          Witness bytes are unavailable in current API mode. Set `[ckb].data_path` in
          `ckbadger.toml` to enable witness inspection.
        </div>
      </div>
    );
  }
  if (witnessAnalyses.length === 0) {
    return (
      <div className="p-4" data-testid="tx-witness-tab">
        <div className="border-base-border bg-base-surface/60 text-text rounded border p-4 text-sm">
          This transaction has no witness entries.
        </div>
      </div>
    );
  }
  const deterministicAnalysis = activeDeterministic;
  const heuristicGuesses = activeWitness?.heuristicGuesses ?? [];
  const dataSegments = deterministicAnalysis?.segments ?? [];
  const segmentOffsetMap = new Array<number>(activeWitness?.previewBytes ?? 0).fill(-1);
  dataSegments.forEach((segment, segmentIndex) => {
    const start = Math.max(0, segment.start);
    const end = Math.min(activeWitness?.previewBytes ?? 0, segment.end);
    for (let offset = start; offset < end; offset += 1) {
      segmentOffsetMap[offset] = segmentIndex;
    }
  });
  const focusedSegmentIndex =
    pinnedSegmentIndex !== null ? pinnedSegmentIndex : hoveredSegmentIndex;
  const activeSegment =
    focusedSegmentIndex !== null &&
    focusedSegmentIndex >= 0 &&
    focusedSegmentIndex < dataSegments.length
      ? dataSegments[focusedSegmentIndex]
      : null;
  const activeSegmentTone =
    focusedSegmentIndex !== null ? getWitnessSegmentTone(focusedSegmentIndex) : null;
  const activeSegmentHex = (() => {
    if (!activeSegment || !activeWitness) return null;
    const start = Math.max(0, Math.min(activeSegment.start, activeWitness.previewBytes));
    const end = Math.max(start, Math.min(activeSegment.end, activeWitness.previewBytes));
    const hexSlice = activeWitness.previewHex.slice(start * 2, end * 2);
    if (!hexSlice) return null;
    const maxChars = 256;
    if (hexSlice.length <= maxChars) {
      return {
        value: `0x${hexSlice}`,
        truncated: false,
      };
    }
    return {
      value: `0x${hexSlice.slice(0, maxChars)}...`,
      truncated: true,
    };
  })();
  const rows = [];
  if (activeWitness) {
    for (let i = 0; i < activeWitness.previewHex.length; i += WITNESS_BYTES_PER_ROW * 2) {
      rows.push(activeWitness.previewHex.slice(i, i + WITNESS_BYTES_PER_ROW * 2));
    }
  }
  const clearSelection = () => {
    setActiveScriptGroupKey(null);
    setActiveWitnessIndex(null);
    setHoveredSegmentIndex(null);
    setPinnedSegmentIndex(null);
    setHoveredByteOffset(null);
    setExpandedHeuristicIndex(null);
    onSelectionChange?.(null, null);
    syncWitnessQuery(null, null);
  };
  const selectWitness = (witnessIndex: number) => {
    if (activeWitnessIndex === witnessIndex) {
      clearSelection();
      return;
    }
    setActiveScriptGroupKey(null);
    setActiveWitnessIndex(witnessIndex);
    onSelectionChange?.(witnessIndex, null);
    syncWitnessQuery(witnessIndex, null);
  };
  const toggleScriptGroupFocus = (groupKey: string, witnessIndex: number) => {
    if (activeScriptGroupKey === groupKey) {
      clearSelection();
      return;
    }
    setActiveScriptGroupKey(groupKey);
    setActiveWitnessIndex(witnessIndex);
    onSelectionChange?.(witnessIndex, groupKey);
    syncWitnessQuery(witnessIndex, groupKey);
  };
  const inputWitnessCount = witnessAnalyses.filter((witness) => witness.role === 'input').length;
  const extraWitnessCount = witnessAnalyses.filter((witness) => witness.role === 'extra').length;
  return (
    <div className="space-y-4 p-4" data-testid="tx-witness-tab">
      <div className={`grid gap-3 ${scriptGroupLens.length > 0 ? 'md:grid-cols-2' : ''}`}>
        <div className="border-base-border bg-base-surface/50 rounded border p-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <div className="text-text-dim text-[11px] uppercase tracking-wider">
              Witness Entries
            </div>
            <div className="flex flex-wrap items-center gap-1">
              <button
                type="button"
                data-testid="tx-witness-clear-selection"
                onClick={clearSelection}
                disabled={activeWitnessIndex === null && activeScriptGroupKey === null}
                className={`rounded border px-1.5 py-0.5 font-mono text-[11px] transition ${
                  activeWitnessIndex === null && activeScriptGroupKey === null
                    ? 'border-base-border text-text-dim cursor-not-allowed'
                    : 'border-base-border/70 text-text hover:text-text-bright hover:border-base-border/80'
                }`}
              >
                clear
              </button>
              <span className="border-base-border/70 bg-base-surface/80 text-text rounded border px-1.5 py-0.5 font-mono text-[11px]">
                total {witnessAnalyses.length}
              </span>
              <span className="border-emphasis/30 bg-emphasis/10 text-emphasis rounded border px-1.5 py-0.5 font-mono text-[11px]">
                input {inputWitnessCount}
              </span>
              <span className="border-info/30 bg-info/10 text-info rounded border px-1.5 py-0.5 font-mono text-[11px]">
                extra {extraWitnessCount}
              </span>
            </div>
          </div>
          <div className="grid gap-1 sm:grid-cols-2">
            {witnessAnalyses.map((witness) => (
              <button
                key={witness.index}
                type="button"
                data-testid={`tx-witness-item-${witness.index}`}
                onClick={() => selectWitness(witness.index)}
                className={`rounded border px-2 py-1.5 text-left transition ${
                  witness.index === activeWitnessIndex
                    ? 'border-emphasis/70 bg-emphasis/10'
                    : 'border-base-border/70 bg-base-surface/70 hover:border-base-border/70'
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="text-text-bright font-mono text-[11px]">#{witness.index}</div>
                  <Badge variant={witness.role === 'input' ? 'green' : 'gray'}>
                    {witness.role}
                  </Badge>
                </div>
                <div className="text-text mt-1 font-mono text-[11px]">
                  {witness.byteLength.toLocaleString()} bytes
                </div>
                <div className="text-text-dim mt-0.5 truncate font-mono text-[11px]">
                  {witness.previewHex ? `0x${witness.previewHex.slice(0, 40)}` : '0x'}
                </div>
              </button>
            ))}
          </div>
        </div>
        {scriptGroupLens.length > 0 && (
          <div className="border-base-border bg-base-surface/50 rounded border p-3">
            <div className="text-text-dim mb-2 text-[11px] uppercase tracking-wider">
              Script Groups
            </div>
            <div className="grid gap-1.5">
              {scriptGroupLens.map((group) => {
                const groupScriptName = scriptLookup?.[group.codeHash]?.name ?? null;
                const isFocused = activeScriptGroup?.key === group.key;
                return (
                  <div
                    key={group.key}
                    role="button"
                    tabIndex={0}
                    data-testid={`tx-script-group-focus-${group.witnessIndex}-${group.kind}`}
                    onClick={() => toggleScriptGroupFocus(group.key, group.witnessIndex)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault();
                        toggleScriptGroupFocus(group.key, group.witnessIndex);
                      }
                    }}
                    className={`rounded border px-2 py-1.5 ${
                      isFocused
                        ? 'border-info/70 bg-info/10'
                        : group.witnessIndex === activeWitnessIndex
                          ? 'border-emphasis/70 bg-emphasis/10'
                          : 'border-base-border/70 bg-base-surface/70'
                    } hover:border-base-border/80 cursor-pointer transition`}
                  >
                    <div className="mb-1 flex flex-wrap items-center gap-1.5">
                      <Badge variant="gray">{group.kind}</Badge>
                      <Badge variant="gray">{getScriptRefBadgeLabel(group.hashType)}</Badge>
                      <span className="text-emphasis font-mono text-xs">
                        {isFocused ? 'focused' : `witness #${group.witnessIndex}`}
                      </span>
                    </div>
                    {hasKnownScriptName(groupScriptName) ? (
                      <Link
                        href={getScriptHref({
                          codeHash: group.codeHash,
                          hashType: group.hashType,
                          scriptKind: group.kind,
                          scriptName: groupScriptName,
                        })}
                        onClick={(event) => event.stopPropagation()}
                        className="text-interactive text-sm hover:underline"
                      >
                        {groupScriptName}
                      </Link>
                    ) : (
                      <Link
                        href={getScriptHref({
                          codeHash: group.codeHash,
                          hashType: group.hashType,
                          scriptKind: group.kind,
                          scriptName: groupScriptName,
                        })}
                        onClick={(event) => event.stopPropagation()}
                        className="group"
                      >
                        <HexDisplay
                          value={group.codeHash}
                          size="sm"
                          className="group-hover:underline"
                        />
                      </Link>
                    )}
                    <div className="text-text-dim mt-1 font-mono text-[11px]">
                      inputs: [{group.inputIndices.join(', ')}]
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
      {activeScriptGroup && (
        <div
          data-testid="tx-witness-focused-group"
          className="border-info/30 bg-info/10 flex flex-wrap items-center justify-between gap-2 rounded border px-3 py-2"
        >
          <div className="text-info text-xs">
            Focused script group: <span className="font-mono">{activeScriptGroup.kind}</span> {'->'}
            witness #{activeScriptGroup.witnessIndex}
          </div>
          <button
            type="button"
            onClick={() => {
              setActiveScriptGroupKey(null);
              const nextWitnessIndex = activeWitnessIndex ?? activeScriptGroup.witnessIndex;
              onSelectionChange?.(nextWitnessIndex, null);
              syncWitnessQuery(nextWitnessIndex, null);
            }}
            className="border-info/30 text-info hover:bg-info/20 rounded border px-2 py-1 font-mono text-xs"
          >
            Clear focus
          </button>
        </div>
      )}
      {activeWitness ? (
        <>
          <div className="mb-3 flex flex-wrap items-center gap-2 text-xs">
            <div className="border-base-border/70 bg-base-surface/70 inline-flex items-center gap-2 rounded border px-2.5 py-1.5">
              <span className="text-text uppercase tracking-wide">Active</span>
              <span className="text-text-bright font-mono">#{activeWitness.index}</span>
              <Badge variant={activeWitness.role === 'input' ? 'green' : 'gray'}>
                {activeWitness.role}
              </Badge>
            </div>
            <div className="border-base-border/70 bg-base-surface/70 inline-flex items-center gap-2 rounded border px-2.5 py-1.5">
              <span className="text-text uppercase tracking-wide">Size</span>
              <span className="text-text-bright font-mono">
                {activeWitness.byteLength.toLocaleString()}B
              </span>
            </div>
            <div
              className={`inline-flex items-center gap-2 rounded border px-2.5 py-1.5 ${
                activeWitness.isPreviewTruncated
                  ? 'border-warning/30 bg-warning/10'
                  : 'border-emphasis/25 bg-emphasis/5'
              }`}
            >
              <span className="text-text uppercase tracking-wide">Preview</span>
              {activeWitness.isPreviewTruncated ? (
                <span className="text-warning font-mono">
                  Truncated at {activeWitness.previewBytes.toLocaleString()}B
                </span>
              ) : (
                <span className="text-emphasis">Full witness shown</span>
              )}
            </div>
          </div>
          {deterministicAnalysis && (
            <div
              data-testid="tx-witness-deterministic-section"
              className="border-base-border bg-base-bg/70 rounded border p-2"
            >
              <div className="mb-1.5 flex flex-wrap items-center gap-1.5">
                <span className="text-text-dim text-[10px] uppercase tracking-[0.12em]">
                  Deterministic Decode
                </span>
                <Badge variant="neutral">{deterministicAnalysis.kind}</Badge>
                <span className="border-base-border/80 bg-base-surface/70 text-text rounded border px-1.5 py-0.5 font-mono text-[10px]">
                  {deterministicAnalysis.segments.length} segments
                </span>
                {pinnedSegmentIndex !== null && (
                  <span data-testid="tx-witness-segment-pinned">
                    <Badge variant="gold">Pinned</Badge>
                  </span>
                )}
              </div>
              <div className="text-text mb-1.5 text-[11px] leading-4">
                {deterministicAnalysis.summary}
              </div>
              <div className="grid gap-2 md:grid-cols-2">
                <div className="border-base-border bg-base-bg/60 rounded border p-1.5">
                  <div className="text-text-dim mb-1 text-[10px] uppercase tracking-[0.12em]">
                    Parsed Segments
                  </div>
                  <div
                    className="flex flex-wrap gap-1"
                    onMouseLeave={() => setHoveredSegmentIndex(null)}
                  >
                    {deterministicAnalysis.segments.map((segment, idx) => {
                      const inPreview =
                        segment.start < activeWitness.previewBytes && segment.end > 0;
                      const isActive = idx === focusedSegmentIndex;
                      const segmentTone = getWitnessSegmentTone(idx);
                      return (
                        <button
                          key={`${segment.label}-${segment.start}-${segment.end}`}
                          type="button"
                          data-testid={`tx-witness-segment-item-${idx}`}
                          onMouseEnter={() => setHoveredSegmentIndex(idx)}
                          onClick={() =>
                            setPinnedSegmentIndex((prev) => (prev === idx ? null : idx))
                          }
                          title={segment.meaning}
                          className={`inline-flex max-w-full items-center gap-1.5 rounded border px-1.5 py-0.5 font-mono text-[11px] transition ${
                            isActive
                              ? segmentTone.activePill
                              : inPreview
                                ? 'border-base-border/70 bg-base-surface/60 text-text hover:border-base-border/70'
                                : 'border-base-border/70 bg-base-surface/40 text-text-dim'
                          }`}
                        >
                          <span
                            className={`h-1.5 w-1.5 shrink-0 rounded-full ${segmentTone.dot}`}
                          />
                          <span className="truncate">{segment.label}</span>
                          <span className="text-text-dim shrink-0 text-[10px]">
                            [{segment.start}..{segment.end})
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </div>
                <div
                  data-testid="tx-witness-active-segment"
                  className="border-base-border bg-base-bg/70 h-[132px] overflow-y-auto rounded border p-2 sm:h-[144px]"
                >
                  {activeSegment ? (
                    <>
                      <div className="text-text-dim text-[10px] uppercase tracking-[0.12em]">
                        Segment Detail
                      </div>
                      <div className="text-text mt-1 font-mono text-[11px]">
                        {activeSegment.label}
                      </div>
                      <div className="text-text mt-0.5 text-[10px] leading-4">
                        {activeSegment.meaning}
                      </div>
                      <div
                        data-testid="tx-witness-active-segment-value"
                        className={`mt-1 break-all font-mono text-sm ${activeSegmentTone?.valueText ?? 'text-emphasis'}`}
                      >
                        {activeSegment.humanValue}
                      </div>
                      <div className="text-text mt-1.5 font-mono text-[11px]">
                        [{activeSegment.start}..{activeSegment.end})
                      </div>
                      {activeSegmentHex && (
                        <div
                          data-testid="tx-witness-active-segment-hex"
                          className={`mt-1 break-all font-mono text-[11px] ${activeSegmentTone?.valueText ?? 'text-emphasis'}`}
                        >
                          {activeSegmentHex.value}
                        </div>
                      )}
                      {activeSegmentHex?.truncated && (
                        <div className="text-text-dim mt-1 text-[11px]">
                          Hex preview truncated for readability.
                        </div>
                      )}
                    </>
                  ) : (
                    <>
                      <div className="text-text-dim text-[10px] uppercase tracking-[0.12em]">
                        Segment Detail
                      </div>
                      <div className="text-text-dim mt-1 text-xs">
                        Hover a segment/byte to preview it, or click a segment to pin it.
                      </div>
                    </>
                  )}
                </div>
              </div>
            </div>
          )}
          {heuristicGuesses.length > 0 && (
            <div
              data-testid="tx-witness-heuristics-list"
              className="border-base-border bg-base-bg/70 rounded border p-2"
            >
              <div className="mb-1 flex items-center justify-between gap-2">
                <div className="text-text-dim text-[10px] uppercase tracking-[0.12em]">
                  Heuristic Guesses
                </div>
                <span className="border-base-border/80 bg-base-surface/70 text-text rounded border px-1.5 py-0.5 font-mono text-[10px]">
                  {heuristicGuesses.length}
                </span>
              </div>
              <div className="grid gap-1 sm:grid-cols-2 xl:grid-cols-3">
                {heuristicGuesses.map((guess, idx) => {
                  const guessTone = getWitnessSegmentTone(idx);
                  const isExpanded = expandedHeuristicIndex === idx;
                  return (
                    <button
                      key={`${guess.kind}-${idx}`}
                      type="button"
                      data-testid={`tx-witness-heuristic-item-${idx}`}
                      onClick={() =>
                        setExpandedHeuristicIndex((prev) => (prev === idx ? null : idx))
                      }
                      className={`rounded border p-1 text-left transition ${
                        isExpanded
                          ? `${guessTone.activePill} bg-opacity-100`
                          : 'border-base-border/80 bg-base-surface/70 hover:border-base-border/80'
                      }`}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-1">
                            <span
                              className={`h-1.5 w-1.5 shrink-0 rounded-full ${guessTone.dot}`}
                            />
                            <span className="text-text font-mono text-[11px]">{guess.kind}</span>
                            <Badge
                              variant={
                                guess.confidence === 'high'
                                  ? 'green'
                                  : guess.confidence === 'medium'
                                    ? 'gold'
                                    : 'gray'
                              }
                            >
                              {guess.confidence}
                            </Badge>
                          </div>
                        </div>
                        <span className="text-text-dim font-mono text-[10px]">
                          {isExpanded ? '[-]' : '[+]'}
                        </span>
                      </div>
                      {isExpanded && (
                        <div
                          data-testid={`tx-witness-heuristic-detail-${idx}`}
                          className="border-base-border/80 mt-1 border-t pt-1"
                        >
                          <div className="text-text text-[10px] leading-4">{guess.reason}</div>
                          {guess.humanValue && (
                            <div
                              className={`mt-0.5 break-all font-mono text-[11px] ${guessTone.valueText}`}
                            >
                              {guess.humanValue}
                            </div>
                          )}
                        </div>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          )}
          <div className="border-base-border bg-base-bg overflow-x-auto rounded-md border p-4 font-mono text-xs">
            {activeWitness.previewHex.length === 0 ? (
              <div className="text-text-dim">No bytes to render for this witness.</div>
            ) : (
              <div
                data-testid="tx-witness-bytes-grid"
                className="min-w-max"
                onMouseLeave={() => {
                  setHoveredSegmentIndex(null);
                  setHoveredByteOffset(null);
                }}
              >
                {rows.map((rowHex, rowIndex) => {
                  const offset = (rowIndex * WITNESS_BYTES_PER_ROW).toString(16).padStart(4, '0');
                  const bytes = [];
                  for (let i = 0; i < rowHex.length; i += 2) {
                    bytes.push(rowHex.slice(i, i + 2));
                  }
                  const byteEntries = bytes.map((byteHex, colIndex) => {
                    const absoluteOffset = rowIndex * WITNESS_BYTES_PER_ROW + colIndex;
                    const segmentIndex =
                      absoluteOffset < segmentOffsetMap.length
                        ? segmentOffsetMap[absoluteOffset]
                        : -1;
                    const segmentTone =
                      segmentIndex >= 0 ? getWitnessSegmentTone(segmentIndex) : null;
                    const isActiveSegment =
                      segmentIndex >= 0 && segmentIndex === focusedSegmentIndex;
                    const isHoveredByte = absoluteOffset === hoveredByteOffset;
                    const hasActiveSegment = focusedSegmentIndex !== null;
                    const byteClass =
                      segmentIndex < 0
                        ? hasActiveSegment
                          ? 'text-text-dim'
                          : 'rounded bg-base-elevated/70 text-text'
                        : isActiveSegment
                          ? (segmentTone?.byteActive ??
                            'rounded bg-emphasis/25 text-emphasis ring-1 ring-emphasis/70')
                          : hasActiveSegment
                            ? 'text-text-dim opacity-40'
                            : (segmentTone?.byte ?? 'rounded bg-emphasis/15 text-emphasis/70');
                    const asciiClass =
                      segmentIndex < 0
                        ? hasActiveSegment
                          ? 'text-text-dim'
                          : 'text-text-dim'
                        : isActiveSegment
                          ? (segmentTone?.asciiActive ?? 'rounded-sm bg-emphasis/20 text-emphasis')
                          : hasActiveSegment
                            ? 'text-text-dim opacity-40'
                            : 'text-text-dim';
                    const asciiHoverClass = isHoveredByte
                      ? segmentIndex >= 0
                        ? (segmentTone?.asciiHover ??
                          'rounded-sm bg-emphasis/30 text-emphasis shadow-[inset_0_0_0_1px_rgba(0,255,65,0.45)]')
                        : 'rounded-sm bg-base-border/50 text-text'
                      : '';
                    const hoverBreatheClass = isHoveredByte
                      ? segmentIndex >= 0
                        ? (segmentTone?.byteHover ??
                          'byte-hover-breathe ring-1 ring-emphasis/80 shadow-[0_0_10px_rgba(0,255,65,0.35)]')
                        : 'byte-hover-breathe ring-1 ring-base-border/70 shadow-[0_0_8px_rgba(148,163,184,0.35)]'
                      : '';
                    const title =
                      segmentIndex >= 0 && segmentIndex < dataSegments.length
                        ? dataSegments[segmentIndex].label
                        : undefined;
                    const code = parseInt(byteHex, 16);
                    const asciiChar = code >= 32 && code <= 126 ? String.fromCharCode(code) : '.';
                    return {
                      byteHex,
                      asciiChar,
                      absoluteOffset,
                      segmentIndex,
                      title,
                      byteClass,
                      asciiClass,
                      asciiHoverClass,
                      hoverBreatheClass,
                    };
                  });
                  const padCount = WITNESS_BYTES_PER_ROW - bytes.length;
                  return (
                    <div
                      key={rowIndex}
                      data-row-index={rowIndex}
                      className="hover:bg-base-elevated/50 flex py-0.5"
                    >
                      <span className="text-text-dim mr-4 select-none">0x{offset}:</span>
                      <div className="text-emphasis/70 mr-6 flex gap-1.5">
                        {byteEntries.map((entry) => (
                          <span
                            key={entry.absoluteOffset}
                            data-testid={`tx-witness-byte-${entry.absoluteOffset}`}
                            className={`${entry.byteClass} ${
                              entry.segmentIndex >= 0 ? 'cursor-pointer' : 'cursor-default'
                            } ${entry.hoverBreatheClass}`}
                            title={entry.title}
                            onMouseEnter={() => {
                              setHoveredByteOffset(entry.absoluteOffset);
                              setHoveredSegmentIndex(
                                entry.segmentIndex >= 0 ? entry.segmentIndex : null
                              );
                            }}
                            onClick={() => {
                              if (entry.segmentIndex < 0) return;
                              setPinnedSegmentIndex((prev) =>
                                prev === entry.segmentIndex ? null : entry.segmentIndex
                              );
                            }}
                          >
                            {entry.byteHex}
                          </span>
                        ))}
                        {Array.from({ length: padCount }).map((_, i) => (
                          <span key={`pad-${i}`} className="opacity-0">
                            00
                          </span>
                        ))}
                      </div>
                      <div className="border-base-border border-l pl-4">
                        {byteEntries.map((entry) => (
                          <span
                            key={`ascii-${entry.absoluteOffset}`}
                            data-testid={`tx-witness-ascii-byte-${entry.absoluteOffset}`}
                            className={`inline-flex w-2.5 justify-center rounded-sm transition-colors duration-100 ${
                              entry.segmentIndex >= 0 ? 'cursor-pointer' : 'cursor-default'
                            } ${entry.asciiClass} ${entry.asciiHoverClass}`}
                            title={entry.title}
                            onMouseEnter={() => {
                              setHoveredByteOffset(entry.absoluteOffset);
                              setHoveredSegmentIndex(
                                entry.segmentIndex >= 0 ? entry.segmentIndex : null
                              );
                            }}
                            onClick={() => {
                              if (entry.segmentIndex < 0) return;
                              setPinnedSegmentIndex((prev) =>
                                prev === entry.segmentIndex ? null : entry.segmentIndex
                              );
                            }}
                          >
                            {entry.asciiChar}
                          </span>
                        ))}
                      </div>
                    </div>
                  );
                })}
                {activeWitness.remainingBytes > 0 && (
                  <div className="text-text-dim mt-2 select-none italic">
                    ... {activeWitness.remainingBytes.toLocaleString()} more bytes
                  </div>
                )}
              </div>
            )}
          </div>
        </>
      ) : (
        <div
          data-testid="tx-witness-selection-empty"
          className="border-base-border bg-base-surface/60 text-text rounded border p-4 text-sm"
        >
          Select a witness entry or script group to inspect deterministic decode and bytes.
        </div>
      )}
    </div>
  );
}
interface ScriptInfo {
  codeHash: string;
  hashType: string;
  name: string | null;
  count: number;
}
function ScriptsSummaryTab({ tx, scriptLookup }: TabProps) {
  const scriptSummary = useMemo(() => {
    const lockScripts = new Map<string, ScriptInfo>();
    const typeScripts = new Map<string, ScriptInfo>();
    const addScript = (
      map: Map<string, ScriptInfo>,
      codeHash: string,
      hashType: string,
      name: string | null
    ) => {
      const key = `${codeHash}:${hashType}`;
      const existing = map.get(key);
      map.set(key, {
        codeHash,
        hashType,
        name,
        count: (existing?.count ?? 0) + 1,
      });
    };
    tx.inputs?.forEach((input) => {
      if (input.lock?.codeHash && input.lock?.hashType) {
        const name = scriptLookup?.[input.lock.codeHash]?.name ?? null;
        addScript(lockScripts, input.lock.codeHash, input.lock.hashType, name);
      }
    });
    tx.outputs?.forEach((output) => {
      if (output.lock?.codeHash && output.lock?.hashType) {
        const name = scriptLookup?.[output.lock.codeHash]?.name ?? null;
        addScript(lockScripts, output.lock.codeHash, output.lock.hashType, name);
      }
      if (output.type?.codeHash && output.type?.hashType) {
        const name = scriptLookup?.[output.type.codeHash]?.name ?? null;
        addScript(typeScripts, output.type.codeHash, output.type.hashType, name);
      }
    });
    return { lockScripts, typeScripts };
  }, [tx, scriptLookup]);
  return (
    <div className="grid gap-6 p-4 lg:grid-cols-2">
      <div>
        <h4 className="border-base-border text-text-dim mb-3 border-b pb-2 font-mono text-xs uppercase tracking-wider">
          Lock Scripts
        </h4>
        <div className="border-base-border bg-base-surface/50 rounded-lg border">
          {Array.from(scriptSummary.lockScripts.values()).map((script) => (
            <TerminalRow
              key={`${script.codeHash}:${script.hashType}`}
              className="flex items-center justify-between"
            >
              <div className="min-w-0 flex-1">
                {hasKnownScriptName(script.name) ? (
                  <Link
                    href={getScriptHref({
                      codeHash: script.codeHash,
                      hashType: script.hashType,
                      scriptKind: 'lock',
                      scriptName: script.name,
                    })}
                    className="text-interactive hover:underline"
                  >
                    {script.name!.trim()}
                  </Link>
                ) : (
                  <Link
                    href={getScriptHref({
                      codeHash: script.codeHash,
                      hashType: script.hashType,
                      scriptKind: 'lock',
                      scriptName: script.name,
                    })}
                    className="group"
                  >
                    <HexDisplay
                      value={script.codeHash}
                      truncate
                      className="group-hover:underline"
                    />
                  </Link>
                )}
              </div>
              <div className="flex items-center gap-2">
                <Badge variant="gray">{getScriptRefBadgeLabel(script.hashType)}</Badge>
                <Badge variant="gray">
                  {script.count} cell{script.count > 1 ? 's' : ''}
                </Badge>
              </div>
            </TerminalRow>
          ))}
          {scriptSummary.lockScripts.size === 0 && (
            <div className="text-text-dim p-4 text-center text-sm">No lock scripts</div>
          )}
        </div>
      </div>
      <div>
        <h4 className="border-base-border text-text-dim mb-3 border-b pb-2 font-mono text-xs uppercase tracking-wider">
          Type Scripts
        </h4>
        <div className="border-base-border bg-base-surface/50 rounded-lg border">
          {Array.from(scriptSummary.typeScripts.values()).map((script) => (
            <TerminalRow
              key={`${script.codeHash}:${script.hashType}`}
              className="flex items-center justify-between"
            >
              <div className="min-w-0 flex-1">
                {hasKnownScriptName(script.name) ? (
                  <Link
                    href={getScriptHref({
                      codeHash: script.codeHash,
                      hashType: script.hashType,
                      scriptKind: 'type',
                      scriptName: script.name,
                    })}
                    className="text-interactive hover:underline"
                  >
                    {script.name!.trim()}
                  </Link>
                ) : (
                  <Link
                    href={getScriptHref({
                      codeHash: script.codeHash,
                      hashType: script.hashType,
                      scriptKind: 'type',
                      scriptName: script.name,
                    })}
                    className="group"
                  >
                    <HexDisplay
                      value={script.codeHash}
                      truncate
                      className="group-hover:underline"
                    />
                  </Link>
                )}
              </div>
              <div className="flex items-center gap-2">
                <Badge variant="gray">{getScriptRefBadgeLabel(script.hashType)}</Badge>
                <Badge variant="gray">
                  {script.count} cell{script.count > 1 ? 's' : ''}
                </Badge>
              </div>
            </TerminalRow>
          ))}
          {scriptSummary.typeScripts.size === 0 && (
            <div className="text-text-dim p-4 text-center text-sm">No type scripts</div>
          )}
        </div>
      </div>
    </div>
  );
}
interface CellDepsTabProps {
  cellDeps?: CellDep[];
  isLoading: boolean;
}
function CellDepsTab({ cellDeps, isLoading }: CellDepsTabProps) {
  if (isLoading) {
    return <p className="text-text-dim py-8 text-center">Loading cell deps...</p>;
  }
  if (!cellDeps || cellDeps.length === 0) {
    return <p className="text-text-dim py-8 text-center">No cell dependencies</p>;
  }
  return (
    <div className="border-base-border bg-base-surface/50 m-4 rounded-lg border">
      {cellDeps.map((cellDep, index) => (
        <TerminalRow
          key={`${cellDep.outPointTxHash}-${cellDep.outPointIndex}`}
          className="flex items-center justify-between"
        >
          <div className="flex items-center gap-3">
            <span className="text-text-dim font-mono text-xs">#{index}</span>
            <Link
              href={`/cell/${cellDep.outPointTxHash}-${cellDep.outPointIndex}`}
              className="hover:text-interactive group flex items-center gap-1"
            >
              <HexDisplay
                value={cellDep.outPointTxHash}
                startChars={10}
                endChars={8}
                copyable={false}
              />
              <span className="group-hover:text-interactive text-text-dim">:</span>
              <span className="group-hover:text-interactive text-text font-mono text-sm">
                {cellDep.outPointIndex}
              </span>
            </Link>
          </div>
          <Badge variant="neutral">{cellDep.depType}</Badge>
        </TerminalRow>
      ))}
    </div>
  );
}
