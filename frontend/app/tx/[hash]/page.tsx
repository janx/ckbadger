'use client';

import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
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
import { CellGraph } from '@/components/cell-graph';
import { api, type CellDep, type GraphNode, type ScriptLookupResponse } from '@/lib/api';
import { getScriptRefBadgeLabel, getScriptRefQueryHashType } from '@/lib/script-ref';
import { formatTimeAgo, formatCkbAmount } from '@/lib/utils';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';

type TxGraphView = 'flow' | 'graph';

export default function TransactionDetailPage() {
  const params = useParams();
  const router = useRouter();
  const hash = params.hash as string;

  const {
    data: tx,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['transaction', hash],
    queryFn: () => api.getTransactionDetail(hash),
  });

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
              <h2 className="text-xl text-slate-400">Transaction not found</h2>
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
                    <span className="text-slate-600">→</span>
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

        <TerminalPanel>
          <Tabs defaultValue="io">
            <TerminalPanelHeader>
              <TabsList>
                <TabsTrigger value="io">
                  Inputs/Outputs ({tx.inputsCount} → {tx.outputsCount})
                </TabsTrigger>
                <TabsTrigger value="scripts">Scripts</TabsTrigger>
                <TabsTrigger value="celldeps">Cell Deps</TabsTrigger>
                <TabsTrigger value="graph">Graph</TabsTrigger>
              </TabsList>
            </TerminalPanelHeader>

            <TabsContent value="io" className="p-0">
              <InputsOutputsTab tx={tx} scriptLookup={scriptLookup} />
            </TabsContent>

            <TabsContent value="scripts" className="p-0">
              <ScriptsSummaryTab tx={tx} scriptLookup={scriptLookup} />
            </TabsContent>

            <TabsContent value="celldeps" className="p-0">
              <CellDepsTab cellDeps={cellDeps} isLoading={cellDepsLoading} />
            </TabsContent>

            <TabsContent value="graph" className="p-4">
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
                        <span className="font-mono text-slate-100">{tx.outputsCount}</span> | Graph
                        Edges:{' '}
                        <span className="font-mono text-slate-100">
                          {graphInsights.outputLinkCount}
                        </span>
                      </div>
                    </div>
                  </div>
                ) : graphData && graphData.nodes.length > 0 ? (
                  <CellGraph
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
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}

interface TabProps {
  tx: NonNullable<Awaited<ReturnType<typeof api.getTransactionDetail>>>;
  scriptLookup?: ScriptLookupResponse;
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

function InputsOutputsTab({ tx, scriptLookup }: TabProps) {
  return (
    <div className="grid gap-6 p-4 lg:grid-cols-2">
      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Inputs ({tx.inputsCount})
        </h4>
        {tx.inputs && tx.inputs.length > 0 ? (
          <div className="rounded-lg border border-slate-800 bg-slate-900/50">
            {tx.inputs.map((input, index) => (
              <TerminalRow key={index} className="flex flex-col gap-2">
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
            ))}
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
            {tx.outputs.map((output, index) => (
              <TerminalRow key={index} className="flex flex-col gap-2">
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
            ))}
          </div>
        ) : (
          <p className="text-sm text-slate-500">Loading outputs...</p>
        )}
      </div>
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
