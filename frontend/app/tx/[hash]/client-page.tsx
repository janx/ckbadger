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
    <div className="flex h-[240px] items-center justify-center rounded border border-slate-700/70 bg-slate-900/70">
      <p className="text-sm text-slate-500">Loading graph section...</p>
    </div>
  ),
});

const WITNESS_BYTES_PER_ROW = 24;
const EMPTY_WITNESSES: string[] = [];
const WITNESS_SEGMENT_TONES = [
  {
    dot: 'bg-terminal-green',
    activePill: 'border-terminal-green/70 bg-terminal-green/15 text-terminal-green',
    valueText: 'text-terminal-green',
    byte: 'rounded bg-terminal-green/15 text-terminal-dim',
    byteActive: 'rounded bg-terminal-green/25 text-terminal-green ring-1 ring-terminal-green/70',
    byteHover:
      'byte-hover-breathe ring-1 ring-terminal-green/80 shadow-[0_0_10px_rgba(0,255,65,0.35)]',
    asciiActive: 'rounded-sm bg-terminal-green/20 text-terminal-green',
    asciiHover:
      'rounded-sm bg-terminal-green/30 text-terminal-green shadow-[inset_0_0_0_1px_rgba(0,255,65,0.45)]',
  },
  {
    dot: 'bg-cyan-400',
    activePill: 'border-cyan-400/70 bg-cyan-500/15 text-cyan-300',
    valueText: 'text-cyan-300',
    byte: 'rounded bg-cyan-500/15 text-cyan-300',
    byteActive: 'rounded bg-cyan-500/25 text-cyan-200 ring-1 ring-cyan-400/70',
    byteHover: 'byte-hover-breathe ring-1 ring-cyan-400/80 shadow-[0_0_10px_rgba(34,211,238,0.35)]',
    asciiActive: 'rounded-sm bg-cyan-500/20 text-cyan-200',
    asciiHover:
      'rounded-sm bg-cyan-500/30 text-cyan-100 shadow-[inset_0_0_0_1px_rgba(34,211,238,0.5)]',
  },
  {
    dot: 'bg-amber-400',
    activePill: 'border-amber-400/70 bg-amber-500/15 text-amber-200',
    valueText: 'text-amber-200',
    byte: 'rounded bg-amber-500/15 text-amber-200',
    byteActive: 'rounded bg-amber-500/25 text-amber-100 ring-1 ring-amber-400/70',
    byteHover:
      'byte-hover-breathe ring-1 ring-amber-400/80 shadow-[0_0_10px_rgba(251,191,36,0.35)]',
    asciiActive: 'rounded-sm bg-amber-500/20 text-amber-100',
    asciiHover:
      'rounded-sm bg-amber-500/30 text-amber-50 shadow-[inset_0_0_0_1px_rgba(251,191,36,0.5)]',
  },
  {
    dot: 'bg-fuchsia-400',
    activePill: 'border-fuchsia-400/70 bg-fuchsia-500/15 text-fuchsia-200',
    valueText: 'text-fuchsia-200',
    byte: 'rounded bg-fuchsia-500/15 text-fuchsia-200',
    byteActive: 'rounded bg-fuchsia-500/25 text-fuchsia-100 ring-1 ring-fuchsia-400/70',
    byteHover:
      'byte-hover-breathe ring-1 ring-fuchsia-400/80 shadow-[0_0_10px_rgba(232,121,249,0.35)]',
    asciiActive: 'rounded-sm bg-fuchsia-500/20 text-fuchsia-100',
    asciiHover:
      'rounded-sm bg-fuchsia-500/30 text-fuchsia-50 shadow-[inset_0_0_0_1px_rgba(232,121,249,0.5)]',
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
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="h-20 w-full rounded bg-slate-900" />
            <div className="h-64 w-full rounded bg-slate-900" />
            <div className="h-96 w-full rounded bg-slate-900" />
          </div>
        </main>
      </div>
    );
  }

  if (error || !tx) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">
                {isNotFoundError ? 'Transaction not found' : 'Failed to load transaction'}
              </h2>
              {!isNotFoundError && errorMessage && (
                <p className="mt-3 break-all text-sm text-slate-500">{errorMessage}</p>
              )}
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
                    className="text-terminal-green hover:underline"
                  >
                    #{tx.blockNumber.toLocaleString()}
                  </Link>
                ) : lifecycle?.proposedIn ? (
                  <div className="flex items-center gap-3">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs text-slate-500">Proposed</span>
                      <Link
                        href={`/blocks/${lifecycle.proposedIn.blockNumber}`}
                        className="text-slate-400 hover:text-slate-300 hover:underline"
                      >
                        #{lifecycle.proposedIn.blockNumber.toLocaleString()}
                      </Link>
                    </div>
                    <span className="text-slate-500">→</span>
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs text-slate-500">Committed</span>
                      <Link
                        href={`/blocks/${tx.blockNumber}`}
                        className="text-terminal-green font-medium hover:underline"
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
                              ? 'amber'
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
                    className="text-terminal-green hover:underline"
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
                  <span className="text-white">
                    <Capacity value={tx.fee} className="text-white" />
                    <span className="ml-2 font-mono text-sm tabular-nums text-slate-400">
                      ({BigInt(tx.fee).toLocaleString()} shannon)
                    </span>
                  </span>
                  {tx.feeRate && (
                    <span className="font-mono text-xs tabular-nums text-slate-400">
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
                    <span className="inline-flex items-center gap-2 italic text-slate-400">
                      <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-slate-400 border-t-transparent" />
                      <span className="cycles-calculating-marquee">Calculating ...</span>
                    </span>
                  ) : hasFailed ? (
                    <span className="italic text-red-400">Calculation failed</span>
                  ) : (
                    <span className="italic text-slate-500">Not available</span>
                  )}
                </DataField>
              )}

              <DataField label="Carried Capacity">
                <Capacity
                  value={(BigInt(tx.outputsCapacity || '0') + BigInt(tx.fee)).toString()}
                  className="text-white"
                />
              </DataField>

              <DataField label="Occupied Capacity Change">
                {(() => {
                  const inputOccupied = BigInt(tx.inputsOccupiedCapacity || '0');
                  const outputOccupied = BigInt(tx.outputsOccupiedCapacity || '0');
                  const change = outputOccupied - inputOccupied;
                  const zero = BigInt(0);
                  const isIncrease = change > zero;
                  const isDecrease = change < zero;
                  const absChange = change < zero ? -change : change;
                  const f = formatCkbAmount(absChange);
                  return (
                    <div className="flex items-center justify-end gap-2">
                      {isIncrease && (
                        <span className="inline-flex items-center rounded border border-green-900/50 bg-green-900/50 px-2 py-1 font-mono text-sm tabular-nums text-green-400">
                          +{f.integer}
                          <span className="text-[0.85em] text-green-400/60">.{f.decimal}</span>
                          <span className="ml-1 text-[0.85em]">CKB</span>
                        </span>
                      )}
                      {isDecrease && (
                        <span className="inline-flex items-center rounded border border-red-900/50 bg-red-900/50 px-2 py-1 font-mono text-sm tabular-nums text-red-400">
                          -{f.integer}
                          <span className="text-[0.85em] text-red-400/60">.{f.decimal}</span>
                          <span className="ml-1 text-[0.85em]">CKB</span>
                        </span>
                      )}
                      {!isIncrease && !isDecrease && (
                        <span className="inline-flex items-center rounded border border-slate-700 bg-slate-800 px-2 py-1 text-sm text-slate-400">
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
                className="mt-0 rounded border border-slate-800/80 bg-slate-900/40 p-4"
              >
                <div className="space-y-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="inline-flex rounded border border-slate-700/70 bg-slate-900/60 p-1">
                      <button
                        type="button"
                        className={`rounded px-2.5 py-1 text-xs transition-colors ${
                          txGraphView === 'flow'
                            ? 'bg-terminal-green/15 text-terminal-green'
                            : 'text-slate-400 hover:text-slate-200'
                        }`}
                        onClick={() => setTxGraphView('flow')}
                      >
                        Flow View
                      </button>
                      <button
                        type="button"
                        className={`rounded px-2.5 py-1 text-xs transition-colors ${
                          txGraphView === 'graph'
                            ? 'bg-terminal-green/15 text-terminal-green'
                            : 'text-slate-400 hover:text-slate-200'
                        }`}
                        onClick={() => setTxGraphView('graph')}
                      >
                        Graph View
                      </button>
                    </div>
                    {graphInsights.nodeCount > 0 && (
                      <span className="text-xs text-slate-500">
                        {graphInsights.nodeCount} nodes / {graphInsights.linkCount} links
                      </span>
                    )}
                  </div>

                  {txGraphView === 'flow' ? (
                    <div data-testid="tx-relationship-flow" className="space-y-3">
                      <div className="grid gap-3 sm:grid-cols-2">
                        <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                          <div className="text-xs uppercase tracking-wide text-slate-400">
                            Inputs in Graph
                          </div>
                          <div className="mt-1 font-mono text-lg text-slate-100">
                            {graphInsights.inputLinkCount}
                          </div>
                        </div>
                        <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                          <div className="text-xs uppercase tracking-wide text-slate-400">
                            Outputs in Graph
                          </div>
                          <div className="mt-1 font-mono text-lg text-slate-100">
                            {graphInsights.outputNodes.length}
                          </div>
                        </div>
                        <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                          <div className="text-xs uppercase tracking-wide text-slate-400">
                            Live Outputs
                          </div>
                          <div className="text-terminal-green mt-1 font-mono text-lg">
                            {
                              graphInsights.outputNodes.filter((node) => node.status === 'live')
                                .length
                            }
                          </div>
                        </div>
                        <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                          <div className="text-xs uppercase tracking-wide text-slate-400">
                            Dead Outputs
                          </div>
                          <div className="mt-1 font-mono text-lg text-red-400">
                            {
                              graphInsights.outputNodes.filter((node) => node.status === 'dead')
                                .length
                            }
                          </div>
                        </div>
                      </div>

                      <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                        <div className="text-xs uppercase tracking-wide text-slate-400">
                          Transaction Flow Snapshot
                        </div>
                        <div className="mt-2 text-sm text-slate-300">
                          Inputs: <span className="font-mono text-slate-100">{tx.inputsCount}</span>{' '}
                          {'->'} Outputs:{' '}
                          <span className="font-mono text-slate-100">{tx.outputsCount}</span> |
                          Graph Edges:{' '}
                          <span className="font-mono text-slate-100">
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
                    <p className="py-8 text-center text-slate-500">Loading graph...</p>
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
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Inputs ({tx.inputsCount})
        </h4>
        {tx.inputs && tx.inputs.length > 0 ? (
          <div className="rounded-lg border border-slate-800 bg-slate-900/50">
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
                      ? 'io-linked-highlight border-terminal-green/70 bg-terminal-green/10 ring-terminal-green/30 cursor-pointer ring-1'
                      : ''
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-slate-500">#{index}</span>
                      <ScriptLabel script={input.lock} scriptLookup={scriptLookup} type="lock" />
                      <ScriptLabel script={input.type} scriptLookup={scriptLookup} type="type" />
                    </div>
                    {input.previousOutput && (
                      <Link
                        href={`/cell/${input.previousOutput.txHash}-${input.previousOutput.index}`}
                        className="hover:text-terminal-green group flex items-center gap-1 font-mono text-xs text-slate-400"
                      >
                        <HexDisplay
                          value={input.previousOutput.txHash}
                          startChars={8}
                          endChars={6}
                          color="accent"
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
                        <span className="text-sm text-red-400">Address error</span>
                      )}
                      {input.capacity && (
                        <Capacity value={input.capacity} className="text-sm text-slate-300" />
                      )}
                    </div>
                  )}
                </TerminalRow>
              );
            })}
          </div>
        ) : tx.isCellbase ? (
          <p className="text-sm text-slate-500">Cellbase has no inputs</p>
        ) : (
          <p className="text-sm text-slate-500">Loading inputs...</p>
        )}
      </div>

      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Outputs ({tx.outputsCount})
        </h4>
        {tx.outputs && tx.outputs.length > 0 ? (
          <div className="rounded-lg border border-slate-800 bg-slate-900/50">
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
                      ? 'io-linked-highlight border-terminal-green/70 bg-terminal-green/10 ring-terminal-green/30 cursor-pointer ring-1'
                      : ''
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-slate-500">#{index}</span>
                      <ScriptLabel script={output.lock} scriptLookup={scriptLookup} type="lock" />
                      <ScriptLabel script={output.type} scriptLookup={scriptLookup} type="type" />
                    </div>
                    <Link
                      href={`/cell/${tx.hash}-${index}`}
                      className="text-terminal-green font-mono text-xs hover:underline"
                    >
                      View Cell
                    </Link>
                  </div>
                  <div className="flex items-center justify-between">
                    {output.address ? (
                      <Address address={output.address} />
                    ) : (
                      <span className="text-sm text-red-400">Address error</span>
                    )}
                    <Capacity value={output.capacity} className="text-slate-300" />
                  </div>
                </TerminalRow>
              );
            })}
          </div>
        ) : (
          <p className="text-sm text-slate-500">Loading outputs...</p>
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
        <div className="rounded border border-slate-800 bg-slate-900/60 p-4 text-sm text-slate-400">
          Witness bytes are unavailable in current API mode. Set `[ckb].data_path` in
          `ckbadger.toml` to enable witness inspection.
        </div>
      </div>
    );
  }

  if (witnessAnalyses.length === 0) {
    return (
      <div className="p-4" data-testid="tx-witness-tab">
        <div className="rounded border border-slate-800 bg-slate-900/60 p-4 text-sm text-slate-400">
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
        <div className="rounded border border-slate-800 bg-slate-900/50 p-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <div className="text-[11px] uppercase tracking-wider text-slate-500">
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
                    ? 'cursor-not-allowed border-slate-800 text-slate-500'
                    : 'border-slate-700/70 text-slate-300 hover:border-slate-500/80 hover:text-slate-200'
                }`}
              >
                clear
              </button>
              <span className="rounded border border-slate-700/70 bg-slate-900/80 px-1.5 py-0.5 font-mono text-[11px] text-slate-300">
                total {witnessAnalyses.length}
              </span>
              <span className="border-terminal-green/30 bg-terminal-green/10 text-terminal-green rounded border px-1.5 py-0.5 font-mono text-[11px]">
                input {inputWitnessCount}
              </span>
              <span className="rounded border border-cyan-400/30 bg-cyan-500/10 px-1.5 py-0.5 font-mono text-[11px] text-cyan-300">
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
                    ? 'border-terminal-green/70 bg-terminal-green/10'
                    : 'border-slate-700/70 bg-slate-900/70 hover:border-slate-500/70'
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="font-mono text-[11px] text-slate-100">#{witness.index}</div>
                  <Badge variant={witness.role === 'input' ? 'green' : 'gray'}>
                    {witness.role}
                  </Badge>
                </div>
                <div className="mt-1 font-mono text-[11px] text-slate-400">
                  {witness.byteLength.toLocaleString()} bytes
                </div>
                <div className="mt-0.5 truncate font-mono text-[11px] text-slate-500">
                  {witness.previewHex ? `0x${witness.previewHex.slice(0, 40)}` : '0x'}
                </div>
              </button>
            ))}
          </div>
        </div>
        {scriptGroupLens.length > 0 && (
          <div className="rounded border border-slate-800 bg-slate-900/50 p-3">
            <div className="mb-2 text-[11px] uppercase tracking-wider text-slate-500">
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
                        ? 'border-cyan-400/70 bg-cyan-500/10'
                        : group.witnessIndex === activeWitnessIndex
                          ? 'border-terminal-green/70 bg-terminal-green/10'
                          : 'border-slate-700/70 bg-slate-900/70'
                    } cursor-pointer transition hover:border-slate-500/80`}
                  >
                    <div className="mb-1 flex flex-wrap items-center gap-1.5">
                      <Badge variant="gray">{group.kind}</Badge>
                      <Badge variant="gray">{getScriptRefBadgeLabel(group.hashType)}</Badge>
                      <span className="text-terminal-green font-mono text-xs">
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
                        className="text-terminal-green text-sm hover:underline"
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
                          color="accent"
                          className="group-hover:underline"
                        />
                      </Link>
                    )}
                    <div className="mt-1 font-mono text-[11px] text-slate-500">
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
          className="flex flex-wrap items-center justify-between gap-2 rounded border border-cyan-400/50 bg-cyan-500/10 px-3 py-2"
        >
          <div className="text-xs text-cyan-100">
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
            className="rounded border border-cyan-400/50 px-2 py-1 font-mono text-xs text-cyan-200 hover:bg-cyan-500/20"
          >
            Clear focus
          </button>
        </div>
      )}

      {activeWitness ? (
        <>
          <div className="mb-3 flex flex-wrap items-center gap-2 text-xs">
            <div className="inline-flex items-center gap-2 rounded border border-slate-700/70 bg-slate-900/70 px-2.5 py-1.5">
              <span className="uppercase tracking-wide text-slate-400">Active</span>
              <span className="font-mono text-white">#{activeWitness.index}</span>
              <Badge variant={activeWitness.role === 'input' ? 'green' : 'gray'}>
                {activeWitness.role}
              </Badge>
            </div>
            <div className="inline-flex items-center gap-2 rounded border border-slate-700/70 bg-slate-900/70 px-2.5 py-1.5">
              <span className="uppercase tracking-wide text-slate-400">Size</span>
              <span className="font-mono text-white">
                {activeWitness.byteLength.toLocaleString()}B
              </span>
            </div>
            <div
              className={`inline-flex items-center gap-2 rounded border px-2.5 py-1.5 ${
                activeWitness.isPreviewTruncated
                  ? 'border-amber/30 bg-amber/10'
                  : 'border-terminal-green/25 bg-terminal-green/5'
              }`}
            >
              <span className="uppercase tracking-wide text-slate-400">Preview</span>
              {activeWitness.isPreviewTruncated ? (
                <span className="text-amber font-mono">
                  Truncated at {activeWitness.previewBytes.toLocaleString()}B
                </span>
              ) : (
                <span className="text-terminal-green">Full witness shown</span>
              )}
            </div>
          </div>

          {deterministicAnalysis && (
            <div
              data-testid="tx-witness-deterministic-section"
              className="rounded border border-slate-800 bg-slate-950/70 p-2"
            >
              <div className="mb-1.5 flex flex-wrap items-center gap-1.5">
                <span className="text-[10px] uppercase tracking-[0.12em] text-slate-500">
                  Deterministic Decode
                </span>
                <Badge variant="neutral">{deterministicAnalysis.kind}</Badge>
                <span className="rounded border border-slate-700/80 bg-slate-900/70 px-1.5 py-0.5 font-mono text-[10px] text-slate-400">
                  {deterministicAnalysis.segments.length} segments
                </span>
                {pinnedSegmentIndex !== null && (
                  <span data-testid="tx-witness-segment-pinned">
                    <Badge variant="amber">Pinned</Badge>
                  </span>
                )}
              </div>
              <div className="mb-1.5 text-[11px] leading-4 text-slate-300">
                {deterministicAnalysis.summary}
              </div>
              <div className="grid gap-2 md:grid-cols-2">
                <div className="rounded border border-slate-800 bg-slate-950/60 p-1.5">
                  <div className="mb-1 text-[10px] uppercase tracking-[0.12em] text-slate-500">
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
                                ? 'border-slate-700/70 bg-slate-900/60 text-slate-200 hover:border-slate-500/70'
                                : 'border-slate-800/70 bg-slate-900/40 text-slate-500'
                          }`}
                        >
                          <span
                            className={`h-1.5 w-1.5 shrink-0 rounded-full ${segmentTone.dot}`}
                          />
                          <span className="truncate">{segment.label}</span>
                          <span className="shrink-0 text-[10px] text-slate-500">
                            [{segment.start}..{segment.end})
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div
                  data-testid="tx-witness-active-segment"
                  className="h-[132px] overflow-y-auto rounded border border-slate-800 bg-slate-950/70 p-2 sm:h-[144px]"
                >
                  {activeSegment ? (
                    <>
                      <div className="text-[10px] uppercase tracking-[0.12em] text-slate-500">
                        Segment Detail
                      </div>
                      <div className="mt-1 font-mono text-[11px] text-slate-300">
                        {activeSegment.label}
                      </div>
                      <div className="mt-0.5 text-[10px] leading-4 text-slate-400">
                        {activeSegment.meaning}
                      </div>
                      <div
                        data-testid="tx-witness-active-segment-value"
                        className={`mt-1 break-all font-mono text-sm ${activeSegmentTone?.valueText ?? 'text-terminal-green'}`}
                      >
                        {activeSegment.humanValue}
                      </div>
                      <div className="mt-1.5 font-mono text-[11px] text-slate-300">
                        [{activeSegment.start}..{activeSegment.end})
                      </div>
                      {activeSegmentHex && (
                        <div
                          data-testid="tx-witness-active-segment-hex"
                          className={`mt-1 break-all font-mono text-[11px] ${activeSegmentTone?.valueText ?? 'text-terminal-green'}`}
                        >
                          {activeSegmentHex.value}
                        </div>
                      )}
                      {activeSegmentHex?.truncated && (
                        <div className="mt-1 text-[11px] text-slate-500">
                          Hex preview truncated for readability.
                        </div>
                      )}
                    </>
                  ) : (
                    <>
                      <div className="text-[10px] uppercase tracking-[0.12em] text-slate-500">
                        Segment Detail
                      </div>
                      <div className="mt-1 text-xs text-slate-500">
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
              className="rounded border border-slate-800 bg-slate-950/70 p-2"
            >
              <div className="mb-1 flex items-center justify-between gap-2">
                <div className="text-[10px] uppercase tracking-[0.12em] text-slate-500">
                  Heuristic Guesses
                </div>
                <span className="rounded border border-slate-700/80 bg-slate-900/70 px-1.5 py-0.5 font-mono text-[10px] text-slate-400">
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
                          : 'border-slate-800/80 bg-slate-900/70 hover:border-slate-600/80'
                      }`}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-1">
                            <span
                              className={`h-1.5 w-1.5 shrink-0 rounded-full ${guessTone.dot}`}
                            />
                            <span className="font-mono text-[11px] text-slate-200">
                              {guess.kind}
                            </span>
                            <Badge
                              variant={
                                guess.confidence === 'high'
                                  ? 'green'
                                  : guess.confidence === 'medium'
                                    ? 'amber'
                                    : 'gray'
                              }
                            >
                              {guess.confidence}
                            </Badge>
                          </div>
                        </div>
                        <span className="font-mono text-[10px] text-slate-500">
                          {isExpanded ? '[-]' : '[+]'}
                        </span>
                      </div>
                      {isExpanded && (
                        <div
                          data-testid={`tx-witness-heuristic-detail-${idx}`}
                          className="mt-1 border-t border-slate-800/80 pt-1"
                        >
                          <div className="text-[10px] leading-4 text-slate-400">{guess.reason}</div>
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

          <div className="overflow-x-auto rounded-md border border-slate-800 bg-slate-950 p-4 font-mono text-xs">
            {activeWitness.previewHex.length === 0 ? (
              <div className="text-slate-500">No bytes to render for this witness.</div>
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
                          ? 'text-slate-500'
                          : 'rounded bg-slate-800/70 text-slate-300'
                        : isActiveSegment
                          ? (segmentTone?.byteActive ??
                            'rounded bg-terminal-green/25 text-terminal-green ring-1 ring-terminal-green/70')
                          : hasActiveSegment
                            ? 'text-slate-500 opacity-40'
                            : (segmentTone?.byte ??
                              'rounded bg-terminal-green/15 text-terminal-dim');
                    const asciiClass =
                      segmentIndex < 0
                        ? hasActiveSegment
                          ? 'text-slate-500'
                          : 'text-slate-500'
                        : isActiveSegment
                          ? (segmentTone?.asciiActive ??
                            'rounded-sm bg-terminal-green/20 text-terminal-green')
                          : hasActiveSegment
                            ? 'text-slate-500 opacity-40'
                            : 'text-slate-500';
                    const asciiHoverClass = isHoveredByte
                      ? segmentIndex >= 0
                        ? (segmentTone?.asciiHover ??
                          'rounded-sm bg-terminal-green/30 text-terminal-green shadow-[inset_0_0_0_1px_rgba(0,255,65,0.45)]')
                        : 'rounded-sm bg-slate-700/50 text-slate-200'
                      : '';
                    const hoverBreatheClass = isHoveredByte
                      ? segmentIndex >= 0
                        ? (segmentTone?.byteHover ??
                          'byte-hover-breathe ring-1 ring-terminal-green/80 shadow-[0_0_10px_rgba(0,255,65,0.35)]')
                        : 'byte-hover-breathe ring-1 ring-slate-400/70 shadow-[0_0_8px_rgba(148,163,184,0.35)]'
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
                      className="flex py-0.5 hover:bg-slate-800/50"
                    >
                      <span className="mr-4 select-none text-slate-500">0x{offset}:</span>
                      <div className="text-terminal-dim mr-6 flex gap-1.5">
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
                      <div className="border-l border-slate-800 pl-4">
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
                  <div className="mt-2 select-none italic text-slate-500">
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
          className="rounded border border-slate-800 bg-slate-900/60 p-4 text-sm text-slate-400"
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
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Lock Scripts
        </h4>
        <div className="rounded-lg border border-slate-800 bg-slate-900/50">
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
                    className="text-terminal-green hover:underline"
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
                      color="accent"
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
            <div className="p-4 text-center text-sm text-slate-500">No lock scripts</div>
          )}
        </div>
      </div>

      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Type Scripts
        </h4>
        <div className="rounded-lg border border-slate-800 bg-slate-900/50">
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
                    className="text-terminal-green hover:underline"
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
                      color="accent"
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
            <div className="p-4 text-center text-sm text-slate-500">No type scripts</div>
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
    return <p className="py-8 text-center text-slate-500">Loading cell deps...</p>;
  }

  if (!cellDeps || cellDeps.length === 0) {
    return <p className="py-8 text-center text-slate-500">No cell dependencies</p>;
  }

  return (
    <div className="m-4 rounded-lg border border-slate-800 bg-slate-900/50">
      {cellDeps.map((cellDep, index) => (
        <TerminalRow
          key={`${cellDep.outPointTxHash}-${cellDep.outPointIndex}`}
          className="flex items-center justify-between"
        >
          <div className="flex items-center gap-3">
            <span className="font-mono text-xs text-slate-500">#{index}</span>
            <Link
              href={`/cell/${cellDep.outPointTxHash}-${cellDep.outPointIndex}`}
              className="hover:text-terminal-green group flex items-center gap-1"
            >
              <HexDisplay
                value={cellDep.outPointTxHash}
                startChars={10}
                endChars={8}
                color="accent"
                copyable={false}
              />
              <span className="group-hover:text-terminal-green text-slate-500">:</span>
              <span className="group-hover:text-terminal-green font-mono text-sm text-slate-300">
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
