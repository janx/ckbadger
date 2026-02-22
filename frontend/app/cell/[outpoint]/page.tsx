'use client';

import { useEffect, useMemo, useState } from 'react';
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
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { Capacity } from '@/components/ui/capacity';
import { ScriptView } from '@/components/ui/script-view';
import { CellGraph } from '@/components/cell-graph';
import { api, type GraphNode } from '@/lib/api';
import {
  getScriptRefBadgeLabel,
  getScriptRefQueryHashType,
  normalizeScriptRefHashType,
  type ScriptRefHashType,
} from '@/lib/script-ref';

type RelationshipView = 'lifecycle' | 'graph';
const DATA_PREVIEW_LIMIT_BYTES = 1024;
const DATA_BYTES_PER_ROW = 24;
const UNKNOWN_SCRIPT_NAME = 'unknown';

function hasKnownScriptName(name: string | null | undefined): boolean {
  return Boolean(name && name.trim() && name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
}

function normalizeHash(value: string | null | undefined): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function getScriptRefHref(referenceHash: string, hashType: ScriptRefHashType): string {
  return `/script/${referenceHash}?hashType=${hashType}&kind=both`;
}

function getDeploymentReferenceHashes(script: {
  codeHash: string;
  hashType: string;
  deploymentTypeHash?: string | null;
  deploymentDataHash?: string | null;
}): { typeHash: string | null; dataHash: string | null; dataHashType: ScriptRefHashType } {
  const typeHash =
    normalizeHash(script.deploymentTypeHash) ??
    (script.hashType === 'type' ? normalizeHash(script.codeHash) : null);

  const dataHash =
    normalizeHash(script.deploymentDataHash) ??
    (script.hashType !== 'type' ? normalizeHash(script.codeHash) : null);

  const dataHashType: ScriptRefHashType =
    script.hashType !== 'type' ? getScriptRefQueryHashType(script.hashType, 'data') : 'data';

  return { typeHash, dataHash, dataHashType };
}

function getCodeCellScriptHref(script: {
  name: string;
  codeHash: string;
  hashType: string;
  deploymentTypeHash?: string | null;
  deploymentDataHash?: string | null;
}): string {
  if (hasKnownScriptName(script.name)) {
    return `/scripts/${encodeURIComponent(script.name.trim())}`;
  }

  const refs = getDeploymentReferenceHashes(script);
  if (refs.typeHash) {
    return getScriptRefHref(refs.typeHash, 'type');
  }
  if (refs.dataHash) {
    return getScriptRefHref(refs.dataHash, refs.dataHashType);
  }

  return getScriptRefHref(script.codeHash, normalizeScriptRefHashType(script.hashType) ?? 'data');
}

function shortenHash(hash: string, leading: number = 10, trailing: number = 8): string {
  if (hash.length <= leading + trailing + 3) {
    return hash;
  }
  return `${hash.slice(0, leading)}...${hash.slice(-trailing)}`;
}

function scriptFallbackLabel(codeHash: string): string {
  return `script: ${shortenHash(codeHash, 10, 8)}`;
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
  const [hoveredDataSegmentIndex, setHoveredDataSegmentIndex] = useState<number | null>(null);
  const [pinnedDataSegmentIndex, setPinnedDataSegmentIndex] = useState<number | null>(null);
  const [dataByteFilter, setDataByteFilter] = useState<'all' | 'parsed' | 'unparsed'>('all');
  const [selectedUnparsedRangeIndex, setSelectedUnparsedRangeIndex] = useState<number | null>(null);
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
        colorClass: 'bg-cyan-400',
      },
      {
        key: 'lockScriptBytes',
        label: 'Lock Script',
        bytes: Math.max(0, breakdown.lockScriptBytes),
        colorClass: 'bg-blue-400',
      },
      {
        key: 'typeScriptBytes',
        label: 'Type Script',
        bytes: Math.max(0, breakdown.typeScriptBytes),
        colorClass: 'bg-violet-400',
      },
      {
        key: 'dataBytes',
        label: 'Cell Data',
        bytes: Math.max(0, breakdown.dataBytes),
        colorClass: 'bg-emerald-400',
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

  const dataPreview = useMemo(() => {
    const rawData = cell?.data ? cell.data.replace(/^0x/, '') : '';
    const receivedDataBytes = Math.max(0, rawData.length / 2);
    const dataPreviewBytes = Math.min(receivedDataBytes, DATA_PREVIEW_LIMIT_BYTES);
    const isDataPreviewTruncated = (cell?.dataSize ?? 0) > dataPreviewBytes;
    const displayHex = rawData.slice(0, dataPreviewBytes * 2);
    const remainingBytes = Math.max(0, (cell?.dataSize ?? 0) - dataPreviewBytes);

    return {
      rawData,
      receivedDataBytes,
      dataPreviewBytes,
      isDataPreviewTruncated,
      displayHex,
      remainingBytes,
    };
  }, [cell]);

  const dataSegments = cell?.dataAnalysis?.deterministic?.segments ?? [];
  const segmentOffsetMap = useMemo(() => {
    const map = new Array<number>(dataPreview.dataPreviewBytes).fill(-1);
    dataSegments.forEach((segment, segmentIndex) => {
      const start = Math.max(0, segment.start);
      const end = Math.min(dataPreview.dataPreviewBytes, segment.end);
      for (let offset = start; offset < end; offset++) {
        map[offset] = segmentIndex;
      }
    });
    return map;
  }, [dataPreview.dataPreviewBytes, dataSegments]);

  const focusedDataSegmentIndex =
    pinnedDataSegmentIndex !== null ? pinnedDataSegmentIndex : hoveredDataSegmentIndex;

  const activeDataSegment =
    focusedDataSegmentIndex !== null &&
    focusedDataSegmentIndex >= 0 &&
    focusedDataSegmentIndex < dataSegments.length
      ? dataSegments[focusedDataSegmentIndex]
      : null;

  const activeDataSegmentHex = useMemo(() => {
    if (!activeDataSegment) return null;
    if (!dataPreview.rawData) return null;

    const totalBytes = Math.floor(dataPreview.rawData.length / 2);
    const start = Math.max(0, Math.min(activeDataSegment.start, totalBytes));
    const end = Math.max(start, Math.min(activeDataSegment.end, totalBytes));
    const hexSlice = dataPreview.rawData.slice(start * 2, end * 2);

    if (!hexSlice) return null;

    const maxChars = 256; // 128 bytes preview
    if (hexSlice.length <= maxChars) {
      return {
        value: `0x${hexSlice}`,
        truncated: false,
        byteLength: end - start,
      };
    }
    return {
      value: `0x${hexSlice.slice(0, maxChars)}...`,
      truncated: true,
      byteLength: end - start,
    };
  }, [activeDataSegment, dataPreview.rawData]);

  const dataParseCoverage = useMemo(() => {
    const calcCoverage = (totalBytes: number) => {
      if (totalBytes <= 0 || dataSegments.length === 0) {
        return { coveredBytes: 0, uncoveredBytes: Math.max(0, totalBytes), coveragePercent: 0 };
      }

      const ranges = dataSegments
        .map((segment) => ({
          start: Math.max(0, Math.min(totalBytes, segment.start)),
          end: Math.max(0, Math.min(totalBytes, segment.end)),
        }))
        .filter((range) => range.end > range.start)
        .sort((a, b) => a.start - b.start);

      if (ranges.length === 0) {
        return { coveredBytes: 0, uncoveredBytes: totalBytes, coveragePercent: 0 };
      }

      const merged: Array<{ start: number; end: number }> = [ranges[0]];
      for (let i = 1; i < ranges.length; i++) {
        const current = ranges[i];
        const last = merged[merged.length - 1];
        if (current.start <= last.end) {
          last.end = Math.max(last.end, current.end);
        } else {
          merged.push(current);
        }
      }

      const coveredBytes = merged.reduce((sum, range) => sum + (range.end - range.start), 0);
      const uncoveredBytes = Math.max(0, totalBytes - coveredBytes);
      const coveragePercent = totalBytes > 0 ? (coveredBytes / totalBytes) * 100 : 0;
      return { coveredBytes, uncoveredBytes, coveragePercent };
    };

    return {
      full: calcCoverage(cell?.dataSize ?? 0),
      preview: calcCoverage(dataPreview.dataPreviewBytes),
    };
  }, [cell?.dataSize, dataPreview.dataPreviewBytes, dataSegments]);

  const unparsedPreviewRanges = useMemo(() => {
    const total = dataPreview.dataPreviewBytes;
    if (total <= 0) return [] as Array<{ start: number; end: number; length: number }>;
    if (dataSegments.length === 0) {
      return [{ start: 0, end: total, length: total }];
    }

    const ranges = dataSegments
      .map((segment) => ({
        start: Math.max(0, Math.min(total, segment.start)),
        end: Math.max(0, Math.min(total, segment.end)),
      }))
      .filter((range) => range.end > range.start)
      .sort((a, b) => a.start - b.start);

    if (ranges.length === 0) {
      return [{ start: 0, end: total, length: total }];
    }

    const merged: Array<{ start: number; end: number }> = [ranges[0]];
    for (let i = 1; i < ranges.length; i++) {
      const current = ranges[i];
      const last = merged[merged.length - 1];
      if (current.start <= last.end) {
        last.end = Math.max(last.end, current.end);
      } else {
        merged.push(current);
      }
    }

    const gaps: Array<{ start: number; end: number; length: number }> = [];
    let cursor = 0;
    for (const range of merged) {
      if (cursor < range.start) {
        gaps.push({
          start: cursor,
          end: range.start,
          length: range.start - cursor,
        });
      }
      cursor = Math.max(cursor, range.end);
    }
    if (cursor < total) {
      gaps.push({
        start: cursor,
        end: total,
        length: total - cursor,
      });
    }
    return gaps;
  }, [dataPreview.dataPreviewBytes, dataSegments]);

  const selectedUnparsedRange =
    selectedUnparsedRangeIndex !== null &&
    selectedUnparsedRangeIndex >= 0 &&
    selectedUnparsedRangeIndex < unparsedPreviewRanges.length
      ? unparsedPreviewRanges[selectedUnparsedRangeIndex]
      : null;

  useEffect(() => {
    setHoveredDataSegmentIndex(null);
    setPinnedDataSegmentIndex(null);
    setDataByteFilter('all');
    setSelectedUnparsedRangeIndex(null);
  }, [txHash, outputIndex]);

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
  const dataPreviewBytes = dataPreview.dataPreviewBytes;
  const isDataPreviewTruncated = dataPreview.isDataPreviewTruncated;

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
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">Capacity</TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="grid gap-4 md:grid-cols-3">
                <div className="rounded border border-slate-700/70 bg-slate-900/60 p-3">
                  <div className="mb-1 text-xs uppercase tracking-wide text-slate-400">
                    Total Capacity
                  </div>
                  <Capacity
                    value={capacityView.totalCapacity}
                    className="text-lg text-emerald-300"
                    animate={false}
                  />
                </div>
                <div className="rounded border border-slate-700/70 bg-slate-900/60 p-3">
                  <div className="mb-1 text-xs uppercase tracking-wide text-slate-400">
                    Occupied Capacity
                  </div>
                  {capacityView.occupied !== null ? (
                    <Capacity
                      value={capacityView.occupied}
                      className="text-lg text-sky-300"
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
                    <div className="overflow-hidden rounded border border-slate-700/80 bg-slate-900/80">
                      <div className="flex h-3 w-full">
                        {capacityView.segments.map((segment) =>
                          segment.percent > 0 ? (
                            <div
                              key={segment.key}
                              className={`${segment.colorClass} transition-all ${
                                hoveredSegmentKey === null
                                  ? ''
                                  : hoveredSegmentKey === segment.key
                                    ? 'brightness-110'
                                    : 'opacity-45'
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
                  </div>

                  <div data-testid="byte-composition-legend" className="relative z-0 mt-2">
                    <div
                      className="grid gap-1.5"
                      style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))' }}
                    >
                      {capacityView.segments.map((segment) => (
                        <button
                          key={segment.key}
                          type="button"
                          className={`inline-flex min-w-0 items-center justify-between gap-1 rounded border px-2 py-1 text-xs transition-all ${
                            hoveredSegmentKey === null
                              ? 'border-slate-700/50 bg-slate-900/60 text-slate-300'
                              : hoveredSegmentKey === segment.key
                                ? 'border-slate-500/80 bg-slate-800/80 text-white'
                                : 'border-slate-800/60 bg-slate-900/40 text-slate-500'
                          }`}
                          onMouseEnter={() => setHoveredSegmentKey(segment.key)}
                          onMouseLeave={() => setHoveredSegmentKey(null)}
                        >
                          <span className="flex min-w-0 items-center gap-1.5 overflow-hidden">
                            <span
                              className={`h-2 w-2 shrink-0 rounded-full ${segment.colorClass}`}
                            />
                            <span className="truncate">{segment.label}</span>
                          </span>
                          <span className="shrink-0 font-mono text-[11px] text-slate-500">
                            {segment.bytes.toLocaleString()}B · {segment.percent.toFixed(2)}%
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

        <div className="grid gap-6 lg:grid-cols-2 lg:items-stretch">
          <TerminalPanel className="h-full">
            <TerminalPanelHeader indicator="active">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span>Overview</span>
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
                        ? 'bg-slate-800 text-slate-100 ring-1 ring-slate-700'
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
                        ? 'bg-slate-800 text-slate-100 ring-1 ring-slate-700'
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
                          className="font-mono text-amber-300 hover:underline"
                        >
                          {shortenHash(cell.txHash)}
                        </Link>
                        <span className="text-slate-500">Output #{cell.outputIndex}</span>
                        <span className="text-slate-500">at</span>
                        <Link
                          href={`/blocks/${cell.createdAtBlock}`}
                          className="text-emerald-300 hover:underline"
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
                        <div className="mt-2 max-h-56 space-y-1.5 overflow-y-auto pr-1 [scrollbar-color:rgb(71_85_105)_transparent] [scrollbar-width:thin]">
                          {relationshipStats.upstreamInputs.map((input) => (
                            <div
                              key={`${input.txHash}-${input.outputIndex}`}
                              className="flex flex-wrap items-center gap-2 text-sm"
                            >
                              <Link
                                href={`/cell/${input.txHash}-${input.outputIndex}`}
                                className="font-mono text-sky-300 hover:underline"
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
                              className="font-mono text-rose-300 hover:underline"
                            >
                              {shortenHash(cell.consumedByTx)}
                            </Link>
                            {cell.consumedAtBlock && (
                              <>
                                <span className="text-slate-500">at</span>
                                <Link
                                  href={`/blocks/${cell.consumedAtBlock}`}
                                  className="text-rose-300 hover:underline"
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
                  <div className="space-y-2">
                    <div className="rounded border border-slate-700/70 bg-slate-900/70 px-3 py-2 text-xs text-slate-400">
                      <span className="inline-flex items-center gap-2">
                        <span className="h-2 w-2 rounded-full bg-amber-300" />
                        Current cell node is highlighted.
                      </span>
                    </div>
                    <CellGraph
                      nodes={graphData.nodes}
                      links={graphData.links}
                      onNodeClick={handleGraphNodeClick}
                      focusCell={{ txHash: cell.txHash, outputIndex: cell.outputIndex }}
                      width={undefined}
                      height={relationshipStats.graphHeight}
                    />
                  </div>
                ) : (
                  <div className="flex h-[220px] items-center justify-center text-sm text-slate-500">
                    Graph data unavailable
                  </div>
                )}
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

          <div data-testid="cell-side-panels" className="space-y-6">
            <div data-testid="cell-address-panel">
              <TerminalPanel>
                <TerminalPanelHeader indicator="none">Address</TerminalPanelHeader>
                <TerminalPanelContent>
                  <div className="flex flex-wrap items-center gap-2 text-sm text-slate-200">
                    {cell.address ? (
                      <Address address={cell.address} />
                    ) : (
                      <Link
                        href={`/address/${cell.lockScriptHash}`}
                        className="text-emerald-300 hover:underline"
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
                </TerminalPanelContent>
              </TerminalPanel>
            </div>

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
        </div>

        {cell.daoInfo && (
          <TerminalPanel className="mt-6">
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
                    className="text-emerald-300 hover:underline"
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
                      className="text-amber-300 hover:underline"
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
                      className="text-rose-300 hover:underline"
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
                    <span className="font-mono text-emerald-300">
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
                {cell.codeCellOf.map((script, idx) => {
                  const refs = getDeploymentReferenceHashes(script);

                  return (
                    <TerminalRow key={`${script.codeHash}-${script.hashType}-${idx}`}>
                      <div className="min-w-0 space-y-1.5">
                        <Link
                          href={getCodeCellScriptHref(script)}
                          className="text-lg font-medium text-emerald-300 hover:underline"
                        >
                          {hasKnownScriptName(script.name)
                            ? script.name.trim()
                            : scriptFallbackLabel(script.codeHash)}
                        </Link>
                        <div className="flex flex-wrap items-center gap-2 text-xs">
                          <span className="uppercase tracking-wide text-slate-500">Refs</span>
                          <Badge variant="gray">type</Badge>
                          {refs.typeHash ? (
                            <Link
                              href={getScriptRefHref(refs.typeHash, 'type')}
                              className="font-mono text-slate-300 hover:text-emerald-300 hover:underline"
                            >
                              <HexDisplay
                                value={refs.typeHash}
                                size="sm"
                                color="green"
                                startChars={10}
                                endChars={8}
                              />
                            </Link>
                          ) : (
                            <span className="font-mono text-slate-500">Unavailable</span>
                          )}
                          <Badge variant="gray">{getScriptRefBadgeLabel(refs.dataHashType)}</Badge>
                          {refs.dataHash ? (
                            <Link
                              href={getScriptRefHref(refs.dataHash, refs.dataHashType)}
                              className="font-mono text-slate-300 hover:text-emerald-300 hover:underline"
                            >
                              <HexDisplay
                                value={refs.dataHash}
                                size="sm"
                                color="green"
                                startChars={10}
                                endChars={8}
                              />
                            </Link>
                          ) : (
                            <span className="font-mono text-slate-500">Unavailable</span>
                          )}
                        </div>
                      </div>
                    </TerminalRow>
                  );
                })}
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
                        className="text-emerald-300 hover:underline"
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
              <span>DATA</span>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="mb-3 flex flex-wrap items-center gap-2 text-xs">
                <div className="inline-flex items-center gap-2 rounded border border-slate-700/70 bg-slate-900/70 px-2.5 py-1.5">
                  <span className="uppercase tracking-wide text-slate-400">Total</span>
                  <span className="font-mono text-white">
                    {cell.dataSize.toLocaleString()} bytes
                  </span>
                </div>
                <div
                  className={`inline-flex items-center gap-2 rounded border px-2.5 py-1.5 ${
                    isDataPreviewTruncated
                      ? 'border-amber/30 bg-amber/10'
                      : 'border-emerald-500/20 bg-emerald-500/5'
                  }`}
                >
                  <span className="uppercase tracking-wide text-slate-400">Preview</span>
                  {isDataPreviewTruncated ? (
                    <span className="text-amber font-mono">
                      Truncated at the {dataPreviewBytes.toLocaleString()}-th byte
                    </span>
                  ) : (
                    <span className="text-emerald-300">Full data shown</span>
                  )}
                </div>
              </div>

              {cell.dataAnalysis?.deterministic && (
                <div
                  data-testid="data-deterministic-panel"
                  className="mb-3 rounded border border-slate-700/70 bg-slate-900/60 p-3"
                >
                  <div className="mb-1 flex flex-wrap items-center gap-2">
                    <span className="text-xs uppercase tracking-wide text-slate-400">
                      Deterministic Decode
                    </span>
                    <Badge variant="blue">{cell.dataAnalysis.deterministic.kind}</Badge>
                    {pinnedDataSegmentIndex !== null && (
                      <span data-testid="data-segment-pinned">
                        <Badge variant="amber">Pinned</Badge>
                      </span>
                    )}
                  </div>
                  <p className="mb-3 text-sm text-slate-300">
                    {cell.dataAnalysis.deterministic.summary}
                  </p>

                  <div className="mb-3 grid gap-2 md:grid-cols-2">
                    <div className="rounded border border-slate-800 bg-slate-950/70 px-2.5 py-2">
                      <div className="text-[11px] uppercase tracking-wide text-slate-500">
                        Parsed Coverage (Full Payload)
                      </div>
                      <div className="mt-1 font-mono text-sm text-slate-200">
                        {dataParseCoverage.full.coveredBytes.toLocaleString()} /{' '}
                        {(cell.dataSize ?? 0).toLocaleString()} bytes
                      </div>
                      <div className="text-xs text-slate-400">
                        {dataParseCoverage.full.coveragePercent.toFixed(2)}% parsed
                      </div>
                    </div>
                    <div className="rounded border border-slate-800 bg-slate-950/70 px-2.5 py-2">
                      <div className="text-[11px] uppercase tracking-wide text-slate-500">
                        Parsed Coverage (Preview Window)
                      </div>
                      <div className="mt-1 font-mono text-sm text-slate-200">
                        {dataParseCoverage.preview.coveredBytes.toLocaleString()} /{' '}
                        {dataPreviewBytes.toLocaleString()} bytes
                      </div>
                      <div className="text-xs text-slate-400">
                        {dataParseCoverage.preview.coveragePercent.toFixed(2)}% parsed
                      </div>
                    </div>
                  </div>

                  <div
                    data-testid="data-unparsed-ranges"
                    className="mb-3 rounded border border-slate-800 bg-slate-950/70 p-2.5"
                  >
                    <div className="mb-2 text-[11px] uppercase tracking-wide text-slate-500">
                      Unparsed Preview Ranges
                    </div>
                    {unparsedPreviewRanges.length > 0 ? (
                      <div className="space-y-1.5">
                        {unparsedPreviewRanges.map((range, idx) => {
                          const isSelected = selectedUnparsedRangeIndex === idx;
                          return (
                            <button
                              key={`${range.start}-${range.end}`}
                              type="button"
                              data-testid={`unparsed-range-item-${idx}`}
                              className={`flex w-full items-center justify-between rounded border px-2 py-1 text-left text-xs transition ${
                                isSelected
                                  ? 'border-amber-400/70 bg-amber-500/10 text-amber-200'
                                  : 'border-slate-700/70 bg-slate-900/60 text-slate-300 hover:border-slate-500/70'
                              }`}
                              onClick={() => {
                                setSelectedUnparsedRangeIndex((prev) =>
                                  prev === idx ? null : idx
                                );
                                setDataByteFilter('all');
                                const rowIndex = Math.floor(range.start / DATA_BYTES_PER_ROW);
                                const rowNode = document.querySelector<HTMLElement>(
                                  `[data-row-index="${rowIndex}"]`
                                );
                                rowNode?.scrollIntoView?.({
                                  block: 'center',
                                  behavior: 'smooth',
                                });
                              }}
                            >
                              <span className="font-mono">
                                [{range.start}..{range.end})
                              </span>
                              <span className="text-slate-500">{range.length} bytes</span>
                            </button>
                          );
                        })}
                      </div>
                    ) : (
                      <div className="text-xs text-slate-500">
                        Preview window is fully covered by deterministic segments.
                      </div>
                    )}
                  </div>

                  <div className="space-y-2">
                    {cell.dataAnalysis.deterministic.segments.map((segment, idx) => {
                      const inPreview = segment.start < dataPreviewBytes && segment.end > 0;
                      const isActive = idx === focusedDataSegmentIndex;
                      return (
                        <button
                          key={`${segment.label}-${segment.start}-${segment.end}`}
                          type="button"
                          data-testid={`data-segment-item-${idx}`}
                          onMouseEnter={() => setHoveredDataSegmentIndex(idx)}
                          onMouseLeave={() => setHoveredDataSegmentIndex(null)}
                          onClick={() =>
                            setPinnedDataSegmentIndex((prev) => (prev === idx ? null : idx))
                          }
                          className={`flex w-full items-center justify-between rounded border px-2.5 py-1.5 text-left transition ${
                            isActive
                              ? 'border-emerald-400/70 bg-emerald-500/10'
                              : inPreview
                                ? 'border-slate-700/70 bg-slate-900/60 hover:border-slate-500/70'
                                : 'border-slate-800/70 bg-slate-900/40 text-slate-500'
                          }`}
                        >
                          <div className="min-w-0">
                            <div className="truncate font-mono text-xs text-slate-200">
                              {segment.label}
                            </div>
                            <div className="truncate text-xs text-slate-400">{segment.meaning}</div>
                          </div>
                          <div className="ml-2 shrink-0 text-right font-mono text-[11px] text-slate-500">
                            [{segment.start}..{segment.end})
                          </div>
                        </button>
                      );
                    })}
                  </div>

                  <div
                    data-testid="data-active-segment"
                    className="mt-3 rounded border border-slate-800 bg-slate-950/70 p-2.5"
                  >
                    {activeDataSegment ? (
                      <>
                        <div className="text-xs uppercase tracking-wide text-slate-500">
                          Human Value
                        </div>
                        <div
                          data-testid="data-active-segment-value"
                          className="mt-1 break-all font-mono text-sm text-emerald-200"
                        >
                          {activeDataSegment.humanValue}
                        </div>
                        <div className="mt-2 text-xs uppercase tracking-wide text-slate-500">
                          Byte Range
                        </div>
                        <div className="mt-1 font-mono text-xs text-slate-300">
                          [{activeDataSegment.start}..{activeDataSegment.end})
                        </div>
                        {activeDataSegmentHex && (
                          <>
                            <div className="mt-2 text-xs uppercase tracking-wide text-slate-500">
                              Hex Slice ({activeDataSegmentHex.byteLength} bytes)
                            </div>
                            <div
                              data-testid="data-active-segment-hex"
                              className="mt-1 break-all font-mono text-xs text-sky-300"
                            >
                              {activeDataSegmentHex.value}
                            </div>
                            {activeDataSegmentHex.truncated && (
                              <div className="mt-1 text-xs text-slate-500">
                                Hex preview truncated for readability.
                              </div>
                            )}
                          </>
                        )}
                      </>
                    ) : (
                      <div className="text-xs text-slate-500">
                        Hover a segment/byte to preview it, or click a segment to pin it.
                      </div>
                    )}
                  </div>
                </div>
              )}

              {cell.dataAnalysis?.heuristicGuesses &&
                cell.dataAnalysis.heuristicGuesses.length > 0 && (
                  <div
                    data-testid="data-heuristic-panel"
                    className="mb-3 rounded border border-slate-700/70 bg-slate-900/60 p-3"
                  >
                    <div className="mb-2 text-xs uppercase tracking-wide text-slate-400">
                      Heuristic Guesses
                    </div>
                    <div className="space-y-2">
                      {cell.dataAnalysis.heuristicGuesses.map((guess, idx) => (
                        <div
                          key={`${guess.kind}-${idx}`}
                          className="rounded border border-slate-800 bg-slate-950/70 p-2"
                        >
                          <div className="mb-1 flex flex-wrap items-center gap-2">
                            <span className="font-mono text-xs text-slate-200">{guess.kind}</span>
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
                            {guess.mimeType && <Badge variant="gray">{guess.mimeType}</Badge>}
                          </div>
                          <div className="text-xs text-slate-400">{guess.reason}</div>
                          {guess.humanValue && (
                            <div className="mt-1 break-all font-mono text-xs text-slate-300">
                              {guess.humanValue}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}

              <div className="overflow-x-auto rounded-md border border-slate-800 bg-slate-950 p-4 font-mono text-xs">
                {(() => {
                  const rawData = dataPreview.rawData;
                  if (!rawData) {
                    return (
                      <div className="text-slate-500">
                        Raw bytes unavailable from node store. Configure `CKB_DATA_PATH` on API to
                        enable payload preview.
                      </div>
                    );
                  }

                  const displayBytes = dataPreviewBytes;
                  const displayHex = dataPreview.displayHex;

                  const rows = [];
                  for (let i = 0; i < displayHex.length; i += DATA_BYTES_PER_ROW * 2) {
                    rows.push(displayHex.slice(i, i + DATA_BYTES_PER_ROW * 2));
                  }

                  const remainingBytes = dataPreview.remainingBytes;

                  return (
                    <div className="min-w-max">
                      {dataSegments.length > 0 && (
                        <div
                          data-testid="data-byte-filter"
                          className="mb-2 inline-flex rounded border border-slate-700/70 bg-slate-900/60 p-1"
                        >
                          {(['all', 'parsed', 'unparsed'] as const).map((mode) => (
                            <button
                              key={mode}
                              type="button"
                              onClick={() => setDataByteFilter(mode)}
                              className={`rounded px-2.5 py-1 text-xs transition-colors ${
                                dataByteFilter === mode
                                  ? 'bg-slate-800 text-slate-100 ring-1 ring-slate-700'
                                  : 'text-slate-400 hover:text-slate-200'
                              }`}
                            >
                              {mode === 'all'
                                ? 'All Bytes'
                                : mode === 'parsed'
                                  ? 'Parsed Bytes'
                                  : 'Unparsed Bytes'}
                            </button>
                          ))}
                        </div>
                      )}
                      {rows.map((rowHex, idx) => {
                        const offset = (idx * DATA_BYTES_PER_ROW).toString(16).padStart(4, '0');
                        const bytes = [];
                        const ascii = [];

                        for (let i = 0; i < rowHex.length; i += 2) {
                          const hex = rowHex.slice(i, i + 2);
                          bytes.push(hex);
                          const code = parseInt(hex, 16);
                          ascii.push(code >= 32 && code <= 126 ? String.fromCharCode(code) : '.');
                        }

                        const padCount = DATA_BYTES_PER_ROW - bytes.length;

                        return (
                          <div
                            key={idx}
                            data-row-index={idx}
                            className="flex py-0.5 hover:bg-slate-800/50"
                          >
                            <span className="mr-4 select-none text-slate-600">0x{offset}:</span>
                            <div className="text-terminal-dim mr-6 flex gap-1.5">
                              {bytes.map((b, i) => {
                                const absoluteOffset = idx * DATA_BYTES_PER_ROW + i;
                                const segmentIndex =
                                  absoluteOffset < segmentOffsetMap.length
                                    ? segmentOffsetMap[absoluteOffset]
                                    : -1;
                                const isActiveSegment =
                                  segmentIndex >= 0 && segmentIndex === focusedDataSegmentIndex;
                                const hasActiveSegment = focusedDataSegmentIndex !== null;
                                const filteredOut =
                                  (dataByteFilter === 'parsed' && segmentIndex < 0) ||
                                  (dataByteFilter === 'unparsed' && segmentIndex >= 0);
                                const inSelectedUnparsedRange =
                                  selectedUnparsedRange !== null &&
                                  absoluteOffset >= selectedUnparsedRange.start &&
                                  absoluteOffset < selectedUnparsedRange.end;
                                const byteClass =
                                  segmentIndex < 0
                                    ? hasActiveSegment
                                      ? 'text-slate-600'
                                      : 'rounded bg-amber-500/10 text-amber-200/80'
                                    : isActiveSegment
                                      ? 'rounded bg-emerald-500/30 text-emerald-100 ring-1 ring-emerald-400/70'
                                      : hasActiveSegment
                                        ? 'text-slate-500 opacity-40'
                                        : 'rounded bg-sky-500/15 text-sky-200';
                                const title =
                                  segmentIndex >= 0 && segmentIndex < dataSegments.length
                                    ? dataSegments[segmentIndex].label
                                    : undefined;

                                return (
                                  <span
                                    key={i}
                                    data-testid={`data-byte-${absoluteOffset}`}
                                    className={`${byteClass} ${
                                      segmentIndex >= 0 ? 'cursor-pointer' : 'cursor-default'
                                    } ${filteredOut ? 'opacity-20' : ''} ${
                                      inSelectedUnparsedRange
                                        ? 'ring-1 ring-amber-400/70 brightness-125'
                                        : ''
                                    }`}
                                    title={title}
                                    onMouseEnter={() =>
                                      setHoveredDataSegmentIndex(
                                        segmentIndex >= 0 ? segmentIndex : null
                                      )
                                    }
                                    onMouseLeave={() => setHoveredDataSegmentIndex(null)}
                                    onClick={() => {
                                      if (segmentIndex < 0) return;
                                      setPinnedDataSegmentIndex((prev) =>
                                        prev === segmentIndex ? null : segmentIndex
                                      );
                                    }}
                                  >
                                    {b}
                                  </span>
                                );
                              })}
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
