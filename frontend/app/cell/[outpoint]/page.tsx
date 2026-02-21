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
import { DataField } from '@/components/ui/data-field';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { Capacity } from '@/components/ui/capacity';
import { ScriptView } from '@/components/ui/script-view';
import { CellGraph } from '@/components/cell-graph';
import { api, type GraphNode } from '@/lib/api';

type RelationshipView = 'lifecycle' | 'graph';

function shortenHash(hash: string, leading: number = 10, trailing: number = 8): string {
  if (hash.length <= leading + trailing + 3) {
    return hash;
  }
  return `${hash.slice(0, leading)}...${hash.slice(-trailing)}`;
}

export default function CellDetailPage() {
  const params = useParams();
  const router = useRouter();
  const outpoint = params.outpoint as string;

  const [txHash, indexStr] = outpoint.split('-');
  const outputIndex = parseInt(indexStr || '0', 10);

  const {
    data: cell,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['cell', txHash, outputIndex],
    queryFn: () => api.getCell(txHash, outputIndex),
    enabled: !!txHash,
  });

  const { data: graphData } = useQuery({
    queryKey: ['cellGraph', txHash, outputIndex],
    queryFn: () => api.getCellGraph(txHash, outputIndex, 2),
    enabled: !!txHash,
  });

  const codeHashes = useMemo(() => {
    if (!cell) return [];
    const hashes = new Set<string>();
    if (cell.lock?.codeHash) hashes.add(cell.lock.codeHash);
    if (cell.type?.codeHash) hashes.add(cell.type.codeHash);
    return Array.from(hashes);
  }, [cell]);

  const { data: scriptLookup } = useQuery({
    queryKey: ['scriptLookup', codeHashes],
    queryFn: () => api.lookupScripts(codeHashes),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const [hoveredSegmentKey, setHoveredSegmentKey] = useState<string | null>(null);
  const [relationshipView, setRelationshipView] = useState<RelationshipView>('lifecycle');

  const capacityView = useMemo(() => {
    if (!cell) {
      return null;
    }

    const SHANNONS_PER_CKB = BigInt(100000000);
    const totalCapacity = BigInt(cell.capacity);
    const occupied = cell.occupiedCapacity !== undefined ? BigInt(cell.occupiedCapacity) : null;
    const ZERO = BigInt(0);
    const BASIS_POINTS = BigInt(10000);
    const occupiedBytes =
      occupied !== null && occupied >= ZERO ? Number(occupied / SHANNONS_PER_CKB) : null;
    const occupiedRatioPercent =
      occupied !== null && totalCapacity > ZERO
        ? Number((occupied * BASIS_POINTS) / totalCapacity) / 100
        : null;

    const breakdown = cell.occupiedCapacityBreakdown;
    const segments = breakdown && [
      {
        key: 'capacityFieldBytes',
        label: 'Capacity Field',
        bytes: Math.max(0, breakdown.capacityFieldBytes),
        colorClass: 'bg-cyan-500',
      },
      {
        key: 'lockScriptBytes',
        label: 'Lock Script',
        bytes: Math.max(0, breakdown.lockScriptBytes),
        colorClass: 'bg-blue-500',
      },
      {
        key: 'typeScriptBytes',
        label: 'Type Script',
        bytes: Math.max(0, breakdown.typeScriptBytes),
        colorClass: 'bg-violet-500',
      },
      {
        key: 'dataBytes',
        label: 'Cell Data',
        bytes: Math.max(0, breakdown.dataBytes),
        colorClass: 'bg-emerald-500',
      },
    ];

    const knownBytes = segments?.reduce((acc, seg) => acc + seg.bytes, 0) ?? 0;
    const breakdownTotalBytes = Math.max(0, breakdown?.totalBytes ?? 0);
    const canonicalTotalBytes =
      occupiedBytes !== null
        ? Math.max(occupiedBytes, knownBytes)
        : Math.max(knownBytes, breakdownTotalBytes);
    const inferredBytes = Math.max(0, canonicalTotalBytes - knownBytes);

    const segmentsWithInference =
      inferredBytes > 0
        ? [
            ...(segments ?? []),
            {
              key: 'inferredBytes',
              label: 'Unindexed Script Args',
              bytes: inferredBytes,
              colorClass: 'bg-amber-500',
            },
          ]
        : (segments ?? []);

    return {
      occupied,
      totalCapacity,
      totalBytes: canonicalTotalBytes,
      occupiedRatioPercent,
      segments: segmentsWithInference.map((seg) => ({
        ...seg,
        percent: canonicalTotalBytes > 0 ? (seg.bytes / canonicalTotalBytes) * 100 : 0,
      })),
      formulaText: breakdown
        ? `${segmentsWithInference.map((seg) => seg.bytes).join(' + ')} = ${canonicalTotalBytes} bytes`
        : null,
      hasBreakdown: Boolean(breakdown),
    };
  }, [cell]);

  const relationshipStats = useMemo(() => {
    if (!graphData || !cell) {
      return {
        nodeCount: 0,
        linkCount: 0,
        upstreamInputs: [] as Array<{
          txHash: string;
          outputIndex: number;
          capacity: string | null;
          status: string | null;
        }>,
        graphHeight: 240,
      };
    }

    const upstreamInputs = graphData.nodes
      .filter((node) => {
        if (node.nodeType !== 'cell') return false;
        const nodeTxHash = node.data?.txHash;
        const nodeOutputIndex = node.data?.outputIndex;
        if (!nodeTxHash || nodeOutputIndex === undefined) return false;
        return !(nodeTxHash === cell.txHash && nodeOutputIndex === cell.outputIndex);
      })
      .map((node) => ({
        txHash: node.data?.txHash ?? '',
        outputIndex: node.data?.outputIndex ?? 0,
        capacity: node.data?.capacity ?? null,
        status: node.data?.status ?? null,
      }))
      .slice(0, 6);

    const graphHeight = Math.min(
      320,
      Math.max(220, 200 + graphData.nodes.length * 12 + graphData.links.length * 6)
    );

    return {
      nodeCount: graphData.nodes.length,
      linkCount: graphData.links.length,
      upstreamInputs,
      graphHeight,
    };
  }, [graphData, cell]);

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
            <div className="grid gap-6 lg:grid-cols-2">
              <div className="h-80 rounded bg-slate-900" />
              <div className="space-y-6">
                <div className="h-36 rounded bg-slate-900" />
                <div className="h-36 rounded bg-slate-900" />
              </div>
            </div>
          </div>
        </main>
      </div>
    );
  }

  if (error || !cell) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Cell not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const isLive = cell.status === 'live';
  const lockScriptInfo = cell.lock?.codeHash ? scriptLookup?.[cell.lock.codeHash] : undefined;
  const typeScriptInfo = cell.type?.codeHash ? scriptLookup?.[cell.type.codeHash] : undefined;

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Cell"
          hash={`${txHash}:${outputIndex}`}
          badge={
            <div className="flex items-center gap-2">
              <Badge variant={isLive ? 'green' : 'red'}>{isLive ? 'Live' : 'Dead'}</Badge>
              {cell.isDepGroup && <Badge variant="amber">Dep Group</Badge>}
              {cell.daoInfo && <Badge variant="purple">Nervos DAO</Badge>}
            </div>
          }
        />

        {capacityView && (
          <TerminalPanel className="mb-6" glow>
            <TerminalPanelHeader indicator="active">Capacity</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="grid gap-4 md:grid-cols-3">
                <div className="border-terminal-green/20 bg-terminal-green/5 rounded border p-3">
                  <div className="mb-1 text-xs uppercase tracking-wide text-slate-400">
                    Total Capacity
                  </div>
                  <Capacity
                    value={capacityView.totalCapacity}
                    className="text-terminal-green text-lg"
                    animate={false}
                  />
                </div>
                <div className="rounded border border-cyan-500/20 bg-cyan-500/5 p-3">
                  <div className="mb-1 text-xs uppercase tracking-wide text-slate-400">
                    Occupied Capacity
                  </div>
                  {capacityView.occupied !== null ? (
                    <Capacity
                      value={capacityView.occupied}
                      className="text-lg text-cyan-300"
                      animate={false}
                    />
                  ) : (
                    <div className="font-mono text-lg text-slate-500">N/A</div>
                  )}
                </div>
                <div className="rounded border border-slate-700/60 bg-slate-900/60 p-3">
                  <div className="mb-1 text-xs uppercase tracking-wide text-slate-400">
                    Utilization Ratio
                  </div>
                  <div className="font-mono text-xl text-white">
                    {capacityView.occupiedRatioPercent !== null
                      ? `${Math.max(0, capacityView.occupiedRatioPercent).toFixed(2)}%`
                      : 'N/A'}
                  </div>
                </div>
              </div>

              {capacityView.hasBreakdown ? (
                <>
                  <div className="mt-3">
                    <div className="mb-1 flex flex-wrap items-baseline justify-between gap-2">
                      <div className="text-xs uppercase tracking-wide text-slate-400">
                        Byte Composition ({capacityView.totalBytes.toLocaleString()} bytes)
                      </div>
                      {capacityView.formulaText && (
                        <div className="text-xs text-slate-500">
                          Formula: {capacityView.formulaText}
                        </div>
                      )}
                    </div>
                    <div className="overflow-hidden rounded border border-slate-700 bg-slate-900">
                      <div className="flex h-3.5 w-full">
                        {capacityView.segments.map((segment) =>
                          segment.percent > 0 ? (
                            <div
                              key={segment.key}
                              className={`${segment.colorClass} transition-all ${
                                hoveredSegmentKey === null
                                  ? ''
                                  : hoveredSegmentKey === segment.key
                                    ? 'brightness-125'
                                    : 'opacity-35'
                              }`}
                              style={{ width: `${segment.percent}%` }}
                              title={`${segment.label}: ${segment.bytes.toLocaleString()} bytes (${segment.percent.toFixed(2)}%)`}
                              onMouseEnter={() => setHoveredSegmentKey(segment.key)}
                              onMouseLeave={() => setHoveredSegmentKey(null)}
                            />
                          ) : null
                        )}
                      </div>
                    </div>

                    <div
                      data-testid="byte-composition-guides"
                      className="relative z-10 mt-1 flex h-3 w-full"
                    >
                      {capacityView.segments.map((segment) => (
                        <div
                          key={`${segment.key}-guide`}
                          className="relative"
                          style={{ width: `${segment.percent}%` }}
                        >
                          {segment.percent > 0 ? (
                            <span
                              className={`absolute left-1/2 top-0 h-3 w-px -translate-x-1/2 ${
                                segment.colorClass
                              } transition-opacity ${
                                hoveredSegmentKey === null || hoveredSegmentKey === segment.key
                                  ? 'opacity-60'
                                  : 'opacity-20'
                              }`}
                            />
                          ) : null}
                        </div>
                      ))}
                    </div>
                  </div>

                  <div
                    data-testid="byte-composition-legend"
                    className="relative z-0 mt-1 overflow-x-auto pb-0.5"
                  >
                    <div className="inline-flex min-w-full flex-nowrap gap-2">
                      {capacityView.segments.map((segment) => (
                        <button
                          key={segment.key}
                          type="button"
                          className={`inline-flex items-center gap-2 whitespace-nowrap rounded border px-2.5 py-1 text-xs transition-all ${
                            hoveredSegmentKey === null
                              ? 'border-slate-700/60 bg-slate-900/50 text-slate-300'
                              : hoveredSegmentKey === segment.key
                                ? 'border-slate-500 bg-slate-800 text-white'
                                : 'border-slate-800/60 bg-slate-900/30 text-slate-500'
                          }`}
                          onMouseEnter={() => setHoveredSegmentKey(segment.key)}
                          onMouseLeave={() => setHoveredSegmentKey(null)}
                        >
                          <span className={`h-2.5 w-2.5 rounded-full ${segment.colorClass}`} />
                          <span>{segment.label}</span>
                          <span className="font-mono text-slate-400">
                            {segment.bytes.toLocaleString()}B
                          </span>
                          <span className="font-mono text-slate-500">
                            {segment.percent.toFixed(2)}%
                          </span>
                        </button>
                      ))}
                    </div>
                  </div>
                </>
              ) : (
                <div className="mt-4 text-sm text-slate-500">
                  Occupied capacity breakdown is unavailable for this cell.
                </div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        <div className="grid gap-6 lg:grid-cols-2 lg:items-start">
          <TerminalPanel glow>
            <TerminalPanelHeader indicator={isLive ? 'active' : 'inactive'}>
              Overview
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-1">
                <DataField label="Address">
                  <div className="flex items-center gap-2">
                    {cell.address ? (
                      <Address address={cell.address} />
                    ) : (
                      <Link
                        href={`/address/${cell.lockScriptHash}`}
                        className="text-terminal-green hover:underline"
                      >
                        <HexDisplay value={cell.lockScriptHash} />
                      </Link>
                    )}
                    {lockScriptInfo && (
                      <Link
                        href={`/scripts/${encodeURIComponent(lockScriptInfo.name)}`}
                        className="text-blue-400 hover:underline"
                      >
                        <Badge variant="blue">{lockScriptInfo.name}</Badge>
                      </Link>
                    )}
                  </div>
                </DataField>
                <DataField label="Created at Block">
                  <Link
                    href={`/blocks/${cell.createdAtBlock}`}
                    className="text-terminal-green hover:underline"
                  >
                    #{cell.createdAtBlock.toLocaleString()}
                  </Link>
                </DataField>
                <DataField label="Transaction">
                  <Link href={`/tx/${cell.txHash}`} className="text-amber hover:underline">
                    <HexDisplay value={cell.txHash} color="amber" />
                  </Link>
                </DataField>
                {!isLive && cell.consumedAtBlock && (
                  <>
                    <DataField label="Consumed at Block">
                      <Link
                        href={`/blocks/${cell.consumedAtBlock}`}
                        className="text-red-400 hover:underline"
                      >
                        #{cell.consumedAtBlock.toLocaleString()}
                      </Link>
                    </DataField>
                    {cell.consumedByTx && (
                      <DataField label="Consumed by TX">
                        <Link
                          href={`/tx/${cell.consumedByTx}`}
                          className="text-red-400 hover:underline"
                        >
                          <HexDisplay value={cell.consumedByTx} color="amber" />
                        </Link>
                      </DataField>
                    )}
                  </>
                )}
                <DataField label="Data Size">
                  <span className="text-white">
                    {cell.dataSize > 0 ? `${cell.dataSize.toLocaleString()} bytes` : 'Empty'}
                  </span>
                </DataField>
              </div>

              {cell.cellType === 'genesis_special_burn' && (
                <div className="border-amber/30 bg-amber/10 mt-4 rounded-lg border p-4">
                  <div className="text-amber text-sm font-medium">Genesis Special Burn Cell</div>
                  <div className="mt-2 text-sm text-slate-300">
                    <p>
                      This cell contains 8.4B CKB burnt at genesis (25% of 33.6B initial issuance).
                      For secondary issuance calculation,{' '}
                      <strong className="text-amber">5.04B CKB (60%)</strong> is treated as
                      &ldquo;occupied&rdquo; capacity, ensuring miners receive secondary rewards.
                    </p>
                    <p className="mt-2">
                      <span className="text-slate-400">Virtual Occupied Capacity: </span>
                      <span className="text-amber font-mono">5,040,000,000 CKB</span>
                    </p>
                  </div>
                </div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span>Cell Relationship</span>
                {relationshipStats.nodeCount > 0 && (
                  <span className="text-xs text-slate-500">
                    {relationshipStats.nodeCount} nodes / {relationshipStats.linkCount} links
                  </span>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-3">
                <div className="inline-flex rounded border border-slate-700/70 bg-slate-900/60 p-1">
                  <button
                    type="button"
                    className={`rounded px-2.5 py-1 text-xs transition-colors ${
                      relationshipView === 'lifecycle'
                        ? 'bg-terminal-green/15 text-terminal-green'
                        : 'text-slate-400 hover:text-slate-200'
                    }`}
                    onClick={() => setRelationshipView('lifecycle')}
                  >
                    Lifecycle
                  </button>
                  <button
                    type="button"
                    className={`rounded px-2.5 py-1 text-xs transition-colors ${
                      relationshipView === 'graph'
                        ? 'bg-cyan-500/15 text-cyan-300'
                        : 'text-slate-400 hover:text-slate-200'
                    }`}
                    onClick={() => setRelationshipView('graph')}
                  >
                    Graph
                  </button>
                </div>

                {relationshipView === 'lifecycle' ? (
                  <div data-testid="cell-relationship-lifecycle" className="space-y-2.5">
                    <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                      <div className="text-xs uppercase tracking-wide text-slate-400">Created</div>
                      <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-slate-200">
                        <span className="text-slate-500">TX</span>
                        <Link
                          href={`/tx/${cell.txHash}`}
                          className="text-amber font-mono hover:underline"
                        >
                          {shortenHash(cell.txHash)}
                        </Link>
                        <span className="text-slate-500">Output #{cell.outputIndex}</span>
                        <span className="text-slate-500">at</span>
                        <Link
                          href={`/blocks/${cell.createdAtBlock}`}
                          className="text-terminal-green hover:underline"
                        >
                          #{cell.createdAtBlock.toLocaleString()}
                        </Link>
                      </div>
                    </div>

                    <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                      <div className="text-xs uppercase tracking-wide text-slate-400">
                        Current Status
                      </div>
                      <div className="mt-1 flex flex-wrap items-center gap-2 text-sm">
                        <Badge variant={isLive ? 'green' : 'red'}>{isLive ? 'Live' : 'Dead'}</Badge>
                        {isLive ? (
                          <span className="text-slate-300">
                            Unspent cell available in current state.
                          </span>
                        ) : (
                          <span className="text-slate-300">
                            Cell was consumed by a later transaction.
                          </span>
                        )}
                      </div>
                    </div>

                    <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                      <div className="text-xs uppercase tracking-wide text-slate-400">
                        Upstream Inputs ({relationshipStats.upstreamInputs.length})
                      </div>
                      {relationshipStats.upstreamInputs.length > 0 ? (
                        <div className="mt-2 space-y-1.5">
                          {relationshipStats.upstreamInputs.map((input) => (
                            <div
                              key={`${input.txHash}-${input.outputIndex}`}
                              className="flex flex-wrap items-center gap-2 text-sm"
                            >
                              <Link
                                href={`/cell/${input.txHash}-${input.outputIndex}`}
                                className="font-mono text-cyan-300 hover:underline"
                              >
                                {shortenHash(input.txHash)}:{input.outputIndex}
                              </Link>
                              {input.capacity && (
                                <span className="font-mono text-slate-400">
                                  {BigInt(input.capacity).toLocaleString()} shannons
                                </span>
                              )}
                              {input.status && (
                                <span className="rounded bg-slate-800 px-1.5 py-0.5 text-xs text-slate-400">
                                  {input.status}
                                </span>
                              )}
                            </div>
                          ))}
                        </div>
                      ) : (
                        <div className="mt-1 text-sm text-slate-500">
                          No upstream input cells found.
                        </div>
                      )}
                    </div>

                    {!isLive && (
                      <div className="rounded border border-slate-700/70 bg-slate-900/70 p-3">
                        <div className="text-xs uppercase tracking-wide text-slate-400">
                          Consumed
                        </div>
                        {cell.consumedByTx ? (
                          <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-slate-200">
                            <span className="text-slate-500">TX</span>
                            <Link
                              href={`/tx/${cell.consumedByTx}`}
                              className="font-mono text-red-400 hover:underline"
                            >
                              {shortenHash(cell.consumedByTx)}
                            </Link>
                            {cell.consumedAtBlock && (
                              <>
                                <span className="text-slate-500">at</span>
                                <Link
                                  href={`/blocks/${cell.consumedAtBlock}`}
                                  className="text-red-400 hover:underline"
                                >
                                  #{cell.consumedAtBlock.toLocaleString()}
                                </Link>
                              </>
                            )}
                          </div>
                        ) : (
                          <div className="mt-1 text-sm text-slate-500">
                            Consuming transaction not indexed in this graph view.
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                ) : graphData && graphData.nodes.length > 0 ? (
                  <CellGraph
                    nodes={graphData.nodes}
                    links={graphData.links}
                    onNodeClick={handleGraphNodeClick}
                    width={undefined}
                    height={relationshipStats.graphHeight}
                  />
                ) : (
                  <div className="flex h-[220px] items-center justify-center text-sm text-slate-500">
                    Graph data unavailable
                  </div>
                )}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        </div>

        <div className="mt-6 grid gap-6 lg:grid-cols-2">
          <TerminalPanel>
            <TerminalPanelHeader indicator="none">
              <div className="flex items-center gap-2">
                <span>Lock Script</span>
                {lockScriptInfo && (
                  <Link href={`/scripts/${encodeURIComponent(lockScriptInfo.name)}`}>
                    <Badge variant="blue">{lockScriptInfo.name}</Badge>
                  </Link>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <ScriptView script={cell.lock ?? null} collapsible={false} />
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="none">
              <div className="flex items-center gap-2">
                <span>Type Script</span>
                {typeScriptInfo && (
                  <Link href={`/scripts/${encodeURIComponent(typeScriptInfo.name)}`}>
                    <Badge variant="purple">{typeScriptInfo.name}</Badge>
                  </Link>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <ScriptView script={cell.type ?? null} collapsible={false} />
            </TerminalPanelContent>
          </TerminalPanel>
        </div>

        {cell.daoInfo && (
          <TerminalPanel className="mt-6" glow>
            <TerminalPanelHeader indicator="active">
              <div className="flex items-center gap-2">
                <span>Nervos DAO</span>
                <Badge
                  variant={
                    cell.daoInfo.daoStatus === 'deposited'
                      ? 'green'
                      : cell.daoInfo.daoStatus === 'withdrawing'
                        ? 'amber'
                        : 'gray'
                  }
                >
                  {cell.daoInfo.daoStatus === 'deposited'
                    ? 'Active'
                    : cell.daoInfo.daoStatus === 'withdrawing'
                      ? 'Withdrawing'
                      : 'Withdrawn'}
                </Badge>
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
                <div>
                  <div className="text-xs text-slate-500">Deposit Block</div>
                  <Link
                    href={`/blocks/${cell.daoInfo.depositBlockNumber}`}
                    className="text-terminal-green hover:underline"
                  >
                    #{cell.daoInfo.depositBlockNumber.toLocaleString()}
                  </Link>
                </div>
                <div>
                  <div className="text-xs text-slate-500">Deposit Time</div>
                  <span className="text-white">
                    {new Date(cell.daoInfo.depositTimestamp).toLocaleString()}
                  </span>
                </div>
                {cell.daoInfo.withdrawRequestBlock && (
                  <div>
                    <div className="text-xs text-slate-500">Withdraw Request</div>
                    <Link
                      href={`/blocks/${cell.daoInfo.withdrawRequestBlock}`}
                      className="text-amber hover:underline"
                    >
                      #{cell.daoInfo.withdrawRequestBlock.toLocaleString()}
                    </Link>
                  </div>
                )}
                {cell.daoInfo.withdrawRequestTimestamp && (
                  <div>
                    <div className="text-xs text-slate-500">Request Time</div>
                    <span className="text-white">
                      {new Date(cell.daoInfo.withdrawRequestTimestamp).toLocaleString()}
                    </span>
                  </div>
                )}
                {cell.daoInfo.withdrawBlock && (
                  <div>
                    <div className="text-xs text-slate-500">Withdrawn Block</div>
                    <Link
                      href={`/blocks/${cell.daoInfo.withdrawBlock}`}
                      className="text-red-400 hover:underline"
                    >
                      #{cell.daoInfo.withdrawBlock.toLocaleString()}
                    </Link>
                  </div>
                )}
                {cell.daoInfo.withdrawTimestamp && (
                  <div>
                    <div className="text-xs text-slate-500">Withdrawn Time</div>
                    <span className="text-white">
                      {new Date(cell.daoInfo.withdrawTimestamp).toLocaleString()}
                    </span>
                  </div>
                )}
                {cell.daoInfo.compensation && (
                  <div>
                    <div className="text-xs text-slate-500">Compensation Earned</div>
                    <span className="text-terminal-green font-mono">
                      {cell.daoInfo.compensationCkb
                        ? `${Number(cell.daoInfo.compensationCkb).toLocaleString()} CKB`
                        : `${Number(cell.daoInfo.compensation).toLocaleString()} Shannon`}
                    </span>
                  </div>
                )}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {cell.codeCellOf && cell.codeCellOf.length > 0 && (
          <TerminalPanel className="mt-6">
            <TerminalPanelHeader indicator="active">
              <div className="flex items-center gap-2">
                <span>Script Deployments</span>
                <Badge variant="green">Code Cell</Badge>
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <p className="mb-4 text-sm text-slate-400">
                This cell stores script code used by the following scripts:
              </p>
              <div className="space-y-2">
                {cell.codeCellOf.map((script, idx) => (
                  <TerminalRow key={idx}>
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <Link
                          href={`/scripts/${encodeURIComponent(script.name)}`}
                          className="text-terminal-green text-lg font-medium hover:underline"
                        >
                          {script.name}
                        </Link>
                        <Badge variant="gray">{script.hashType}</Badge>
                      </div>
                      <HexDisplay value={script.codeHash} size="sm" color="white" />
                    </div>
                  </TerminalRow>
                ))}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {cell.isDepGroup && (
          <TerminalPanel className="mt-6">
            <TerminalPanelHeader indicator="warning">
              <div className="flex items-center gap-2">
                <span>Dep Group Contents</span>
                {cell.depGroupItems && (
                  <Badge variant="amber">
                    {cell.depGroupItems.length} cell{cell.depGroupItems.length !== 1 ? 's' : ''}
                  </Badge>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              {cell.depGroupItems ? (
                <div className="space-y-1">
                  {cell.depGroupItems.map((item, idx) => (
                    <TerminalRow key={idx} className="flex items-center gap-3">
                      <span className="w-8 text-right font-mono text-sm text-slate-500">
                        #{idx}
                      </span>
                      <Link
                        href={`/cell/${item.txHash}-${item.outputIndex}`}
                        className="text-terminal-green hover:underline"
                      >
                        <HexDisplay value={`${item.txHash}:${item.outputIndex}`} />
                      </Link>
                    </TerminalRow>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-slate-500">
                  Cell data contains {Math.floor((cell.dataSize - 4) / 36)} OutPoints (data
                  truncated in database)
                </p>
              )}
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {cell.dataSize > 0 && (
          <TerminalPanel className="mt-6" variant="inset">
            <TerminalPanelHeader indicator="none">
              <div className="flex items-center gap-2">
                <span>Data</span>
                {cell.data && (cell.data.length - 2) / 2 < cell.dataSize && (
                  <Badge variant="amber">
                    Truncated ({cell.dataSize.toLocaleString()} bytes total)
                  </Badge>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="overflow-x-auto rounded-md border border-slate-800 bg-slate-950 p-4 font-mono text-xs">
                {(() => {
                  const rawData = cell.data ? cell.data.replace(/^0x/, '') : '';
                  if (!rawData) return <div className="text-slate-500">0x</div>;

                  const BYTES_PER_ROW = 24;
                  const MAX_ROWS = 10;
                  const MAX_DISPLAY_BYTES = BYTES_PER_ROW * MAX_ROWS;
                  const receivedBytes = rawData.length / 2;
                  const actualTotalBytes = cell.dataSize;
                  const displayBytes = Math.min(receivedBytes, MAX_DISPLAY_BYTES);
                  const displayHex = rawData.slice(0, displayBytes * 2);

                  const rows = [];
                  for (let i = 0; i < displayHex.length; i += BYTES_PER_ROW * 2) {
                    rows.push(displayHex.slice(i, i + BYTES_PER_ROW * 2));
                  }

                  const remainingBytes = actualTotalBytes - displayBytes;

                  return (
                    <div className="min-w-max">
                      {rows.map((rowHex, idx) => {
                        const offset = (idx * BYTES_PER_ROW).toString(16).padStart(4, '0');
                        const bytes = [];
                        const ascii = [];

                        for (let i = 0; i < rowHex.length; i += 2) {
                          const hex = rowHex.slice(i, i + 2);
                          bytes.push(hex);
                          const code = parseInt(hex, 16);
                          ascii.push(code >= 32 && code <= 126 ? String.fromCharCode(code) : '.');
                        }

                        const padCount = BYTES_PER_ROW - bytes.length;

                        return (
                          <div key={idx} className="flex py-0.5 hover:bg-slate-800/50">
                            <span className="mr-4 select-none text-slate-600">0x{offset}:</span>
                            <div className="text-terminal-dim mr-6 flex gap-1.5">
                              {bytes.map((b, i) => (
                                <span key={i} className="hover:text-terminal-green">
                                  {b}
                                </span>
                              ))}
                              {Array.from({ length: padCount }).map((_, i) => (
                                <span key={`pad-${i}`} className="opacity-0">
                                  00
                                </span>
                              ))}
                            </div>
                            <div className="border-l border-slate-800 pl-4 tracking-widest text-slate-500">
                              {ascii.join('')}
                            </div>
                          </div>
                        );
                      })}
                      {remainingBytes > 0 && (
                        <div className="mt-2 select-none italic text-slate-600">
                          ... {remainingBytes.toLocaleString()} more bytes
                        </div>
                      )}
                    </div>
                  );
                })()}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}
      </main>
    </div>
  );
}
