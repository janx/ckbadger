'use client';

import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import dynamic from '@/lib/dynamic-client';
import { useParams, useRouter } from '@/src/navigation';
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
const DATA_SEGMENT_TONES = [
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

type CapacitySegmentTone = {
  dot: string;
  legendActivePill: string;
  legendValueText: string;
};

const CAPACITY_SEGMENT_TONES: Record<string, CapacitySegmentTone> = {
  capacityFieldBytes: {
    dot: 'bg-slate-500',
    legendActivePill: 'border-slate-400/70 bg-slate-500/15 text-slate-100',
    legendValueText: 'text-slate-300',
  },
  lockScriptBytes: {
    dot: 'bg-terminal-green',
    legendActivePill: 'border-terminal-green/70 bg-terminal-green/15 text-terminal-green',
    legendValueText: 'text-terminal-green',
  },
  typeScriptBytes: {
    dot: 'bg-cyan-400',
    legendActivePill: 'border-cyan-400/70 bg-cyan-500/15 text-cyan-300',
    legendValueText: 'text-cyan-300',
  },
  dataBytes: {
    dot: 'bg-amber-400',
    legendActivePill: 'border-amber-400/70 bg-amber-500/15 text-amber-200',
    legendValueText: 'text-amber-200',
  },
  inferredBytes: {
    dot: 'bg-fuchsia-400',
    legendActivePill: 'border-fuchsia-400/70 bg-fuchsia-500/15 text-fuchsia-200',
    legendValueText: 'text-fuchsia-200',
  },
};

const DeferredCellGraph = dynamic(() => import('@/components/cell-graph'), {
  loading: () => (
    <div className="flex h-[240px] items-center justify-center rounded border border-slate-700/70 bg-slate-900/70">
      <p className="text-sm text-slate-500">Loading graph section...</p>
    </div>
  ),
});

function getDataSegmentTone(segmentIndex: number) {
  return DATA_SEGMENT_TONES[Math.abs(segmentIndex) % DATA_SEGMENT_TONES.length];
}

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
  const [hoveredDataByteOffset, setHoveredDataByteOffset] = useState<number | null>(null);
  const [pinnedDataSegmentIndex, setPinnedDataSegmentIndex] = useState<number | null>(null);
  const [expandedHeuristicIndex, setExpandedHeuristicIndex] = useState<number | null>(null);
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
        colorClass: CAPACITY_SEGMENT_TONES.capacityFieldBytes.dot,
        legendActivePill: CAPACITY_SEGMENT_TONES.capacityFieldBytes.legendActivePill,
        legendValueText: CAPACITY_SEGMENT_TONES.capacityFieldBytes.legendValueText,
      },
      {
        key: 'lockScriptBytes',
        label: 'Lock Script',
        bytes: Math.max(0, breakdown.lockScriptBytes),
        colorClass: CAPACITY_SEGMENT_TONES.lockScriptBytes.dot,
        legendActivePill: CAPACITY_SEGMENT_TONES.lockScriptBytes.legendActivePill,
        legendValueText: CAPACITY_SEGMENT_TONES.lockScriptBytes.legendValueText,
      },
      {
        key: 'typeScriptBytes',
        label: 'Type Script',
        bytes: Math.max(0, breakdown.typeScriptBytes),
        colorClass: CAPACITY_SEGMENT_TONES.typeScriptBytes.dot,
        legendActivePill: CAPACITY_SEGMENT_TONES.typeScriptBytes.legendActivePill,
        legendValueText: CAPACITY_SEGMENT_TONES.typeScriptBytes.legendValueText,
      },
      {
        key: 'dataBytes',
        label: 'Cell Data',
        bytes: Math.max(0, breakdown.dataBytes),
        colorClass: CAPACITY_SEGMENT_TONES.dataBytes.dot,
        legendActivePill: CAPACITY_SEGMENT_TONES.dataBytes.legendActivePill,
        legendValueText: CAPACITY_SEGMENT_TONES.dataBytes.legendValueText,
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
              colorClass: CAPACITY_SEGMENT_TONES.inferredBytes.dot,
              legendActivePill: CAPACITY_SEGMENT_TONES.inferredBytes.legendActivePill,
              legendValueText: CAPACITY_SEGMENT_TONES.inferredBytes.legendValueText,
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

  const deterministicAnalysis = cell?.dataAnalysis?.deterministic ?? null;
  const heuristicGuesses = cell?.dataAnalysis?.heuristicGuesses ?? [];
  const dataSegments = useMemo(
    () => deterministicAnalysis?.segments ?? [],
    [deterministicAnalysis]
  );
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
  const activeDataSegmentTone =
    focusedDataSegmentIndex !== null ? getDataSegmentTone(focusedDataSegmentIndex) : null;

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

  useEffect(() => {
    setHoveredDataSegmentIndex(null);
    setHoveredDataByteOffset(null);
    setPinnedDataSegmentIndex(null);
    setExpandedHeuristicIndex(null);
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
              {cell.isDepGroup && <Badge variant="neutral">Dep Group</Badge>}
              {cell.daoInfo && <Badge variant="neutral">Nervos DAO</Badge>}
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
                    className="text-lg text-slate-200"
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
                      className="text-terminal-green text-lg"
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
                                ? segment.legendActivePill
                                : 'border-slate-800/60 bg-slate-900/40 text-slate-500'
                          }`}
                          onMouseEnter={() => setHoveredSegmentKey(segment.key)}
                          onMouseLeave={() => setHoveredSegmentKey(null)}
                        >
                          <span className="flex min-w-0 items-center gap-1.5 overflow-hidden">
                            <span
                              className={`h-2 w-2 shrink-0 rounded-full ring-1 ring-black/25 ${segment.colorClass}`}
                            />
                            <span className="truncate">{segment.label}</span>
                          </span>
                          <span
                            className={`shrink-0 font-mono text-[11px] ${
                              hoveredSegmentKey === null
                                ? 'text-slate-500'
                                : hoveredSegmentKey === segment.key
                                  ? segment.legendValueText
                                  : 'text-slate-500'
                            }`}
                          >
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
                          className="text-terminal-green font-mono hover:underline"
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
                        <div className="mt-2 max-h-56 space-y-1.5 overflow-y-auto pr-1 [scrollbar-color:rgb(71_85_105)_transparent] [scrollbar-width:thin]">
                          {relationshipStats.upstreamInputs.map((input) => (
                            <div
                              key={`${input.txHash}-${input.outputIndex}`}
                              className="flex flex-wrap items-center gap-2 text-sm"
                            >
                              <Link
                                href={`/cell/${input.txHash}-${input.outputIndex}`}
                                className="text-terminal-green font-mono hover:underline"
                              >
                                {shortenHash(input.txHash)}:{input.outputIndex}
                              </Link>
                              {input.capacity && (
                                <span className="font-mono text-slate-400">
                                  {BigInt(input.capacity).toLocaleString()} shannons
                                </span>
                              )}
                              {input.status && (
                                <Badge
                                  variant={
                                    input.status.toLowerCase() === 'live'
                                      ? 'green'
                                      : input.status.toLowerCase() === 'dead' ||
                                          input.status.toLowerCase() === 'consumed'
                                        ? 'red'
                                        : input.status.toLowerCase() === 'withdrawing'
                                          ? 'amber'
                                          : 'gray'
                                  }
                                >
                                  {input.status.toLowerCase() === 'withdrawing'
                                    ? 'Withdraw Request'
                                    : input.status}
                                </Badge>
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
                              className="text-terminal-green font-mono hover:underline"
                            >
                              {shortenHash(cell.consumedByTx)}
                            </Link>
                            {cell.consumedAtBlock && (
                              <>
                                <span className="text-slate-500">at</span>
                                <Link
                                  href={`/blocks/${cell.consumedAtBlock}`}
                                  className="text-terminal-green hover:underline"
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
                    <DeferredCellGraph
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
                        className="text-terminal-green hover:underline"
                      >
                        <HexDisplay value={cell.lockScriptHash} color="accent" />
                      </Link>
                    )}
                    {lockScriptInfo && (
                      <Link
                        href={`/scripts/${encodeURIComponent(lockScriptInfo.name)}`}
                        className="text-terminal-green hover:underline"
                      >
                        <Badge variant="neutral">{lockScriptInfo.name}</Badge>
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
                      <Badge variant="neutral">{lockScriptInfo.name}</Badge>
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
                      <Badge variant="neutral">{typeScriptInfo.name}</Badge>
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
                      ? 'Withdraw Request'
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
                      className="text-terminal-green hover:underline"
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
                      className="text-terminal-green hover:underline"
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
                    <span className="font-mono text-slate-200">
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
                <Badge variant="neutral">Code Cell</Badge>
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
                          className="text-terminal-green text-lg font-medium hover:underline"
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
                              className="hover:text-terminal-green font-mono text-slate-300 hover:underline"
                            >
                              <HexDisplay
                                value={refs.typeHash}
                                size="sm"
                                color="accent"
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
                              className="hover:text-terminal-green font-mono text-slate-300 hover:underline"
                            >
                              <HexDisplay
                                value={refs.dataHash}
                                size="sm"
                                color="accent"
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
                  <Badge variant="neutral">
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
                        <HexDisplay value={`${item.txHash}:${item.outputIndex}`} color="accent" />
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
                      : 'border-terminal-green/25 bg-terminal-green/5'
                  }`}
                >
                  <span className="uppercase tracking-wide text-slate-400">Preview</span>
                  {isDataPreviewTruncated ? (
                    <span className="text-amber font-mono">
                      Truncated at the {dataPreviewBytes.toLocaleString()}-th byte
                    </span>
                  ) : (
                    <span className="text-terminal-green">Full data shown</span>
                  )}
                </div>
              </div>

              {deterministicAnalysis && (
                <div
                  data-testid="data-deterministic-section"
                  className="mb-3 rounded border border-slate-800 bg-slate-950/70 p-2"
                >
                  <div className="mb-1.5 flex flex-wrap items-center gap-1.5">
                    <span className="text-[10px] uppercase tracking-[0.12em] text-slate-500">
                      Deterministic Decode
                    </span>
                    <Badge variant="neutral">{deterministicAnalysis.kind}</Badge>
                    <span className="rounded border border-slate-700/80 bg-slate-900/70 px-1.5 py-0.5 font-mono text-[10px] text-slate-400">
                      {deterministicAnalysis.segments.length} segments
                    </span>
                    {pinnedDataSegmentIndex !== null && (
                      <span data-testid="data-segment-pinned">
                        <Badge variant="amber">Pinned</Badge>
                      </span>
                    )}
                  </div>
                  <div className="mb-1.5 text-[11px] leading-4 text-slate-300">
                    {deterministicAnalysis.summary}
                  </div>
                  <div
                    data-testid="data-deterministic-columns"
                    className="grid gap-2 md:grid-cols-2"
                  >
                    <div className="rounded border border-slate-800 bg-slate-950/60 p-1.5">
                      <div className="mb-1 text-[10px] uppercase tracking-[0.12em] text-slate-500">
                        Parsed Segments
                      </div>
                      <div
                        className="flex flex-wrap gap-1"
                        onMouseLeave={() => setHoveredDataSegmentIndex(null)}
                      >
                        {deterministicAnalysis.segments.map((segment, idx) => {
                          const inPreview = segment.start < dataPreviewBytes && segment.end > 0;
                          const isActive = idx === focusedDataSegmentIndex;
                          const segmentTone = getDataSegmentTone(idx);
                          return (
                            <button
                              key={`${segment.label}-${segment.start}-${segment.end}`}
                              type="button"
                              data-testid={`data-segment-item-${idx}`}
                              onMouseEnter={() => setHoveredDataSegmentIndex(idx)}
                              onClick={() =>
                                setPinnedDataSegmentIndex((prev) => (prev === idx ? null : idx))
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
                      data-testid="data-active-segment"
                      className="h-[132px] overflow-y-auto rounded border border-slate-800 bg-slate-950/70 p-2 sm:h-[144px]"
                    >
                      {activeDataSegment ? (
                        <>
                          <div className="text-[10px] uppercase tracking-[0.12em] text-slate-500">
                            Segment Detail
                          </div>
                          <div className="mt-1 font-mono text-[11px] text-slate-300">
                            {activeDataSegment.label}
                          </div>
                          <div className="mt-0.5 text-[10px] leading-4 text-slate-400">
                            {activeDataSegment.meaning}
                          </div>
                          <div
                            data-testid="data-active-segment-value"
                            className={`mt-1 break-all font-mono text-sm ${activeDataSegmentTone?.valueText ?? 'text-terminal-green'}`}
                          >
                            {activeDataSegment.humanValue}
                          </div>
                          <div className="mt-1.5 font-mono text-[11px] text-slate-300">
                            [{activeDataSegment.start}..{activeDataSegment.end})
                          </div>
                          {activeDataSegmentHex && (
                            <div
                              data-testid="data-active-segment-hex"
                              className={`mt-1 break-all font-mono text-[11px] ${activeDataSegmentTone?.valueText ?? 'text-terminal-green'}`}
                            >
                              {activeDataSegmentHex.value}
                            </div>
                          )}
                          {activeDataSegmentHex?.truncated && (
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
                  data-testid="data-heuristics-list"
                  className="mb-3 rounded border border-slate-800 bg-slate-950/70 p-2"
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
                      const guessTone = getDataSegmentTone(idx);
                      const isExpanded = expandedHeuristicIndex === idx;
                      return (
                        <button
                          key={`${guess.kind}-${idx}`}
                          type="button"
                          data-testid={`data-heuristic-item-${idx}`}
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
                                {guess.mimeType && <Badge variant="gray">{guess.mimeType}</Badge>}
                              </div>
                            </div>
                            <span className="font-mono text-[10px] text-slate-500">
                              {isExpanded ? '[-]' : '[+]'}
                            </span>
                          </div>
                          {isExpanded && (
                            <div
                              data-testid={`data-heuristic-detail-${idx}`}
                              className="mt-1 border-t border-slate-800/80 pt-1"
                            >
                              <div className="text-[10px] leading-4 text-slate-400">
                                {guess.reason}
                              </div>
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
                {(() => {
                  const rawData = dataPreview.rawData;
                  if (!rawData) {
                    return (
                      <div className="text-slate-500">
                        Raw bytes unavailable from node store. Set `[ckb].data_path` in
                        `ckbadger.toml` to enable payload preview.
                      </div>
                    );
                  }

                  const displayHex = dataPreview.displayHex;

                  const rows = [];
                  for (let i = 0; i < displayHex.length; i += DATA_BYTES_PER_ROW * 2) {
                    rows.push(displayHex.slice(i, i + DATA_BYTES_PER_ROW * 2));
                  }

                  const remainingBytes = dataPreview.remainingBytes;

                  return (
                    <div
                      data-testid="data-bytes-grid"
                      className="min-w-max"
                      onMouseLeave={() => {
                        setHoveredDataSegmentIndex(null);
                        setHoveredDataByteOffset(null);
                      }}
                    >
                      {rows.map((rowHex, idx) => {
                        const offset = (idx * DATA_BYTES_PER_ROW).toString(16).padStart(4, '0');
                        const bytes = [];

                        for (let i = 0; i < rowHex.length; i += 2) {
                          const hex = rowHex.slice(i, i + 2);
                          bytes.push(hex);
                        }

                        const byteEntries = bytes.map((b, i) => {
                          const absoluteOffset = idx * DATA_BYTES_PER_ROW + i;
                          const segmentIndex =
                            absoluteOffset < segmentOffsetMap.length
                              ? segmentOffsetMap[absoluteOffset]
                              : -1;
                          const segmentTone =
                            segmentIndex >= 0 ? getDataSegmentTone(segmentIndex) : null;
                          const isActiveSegment =
                            segmentIndex >= 0 && segmentIndex === focusedDataSegmentIndex;
                          const isHoveredByte = absoluteOffset === hoveredDataByteOffset;
                          const hasActiveSegment = focusedDataSegmentIndex !== null;
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
                          const code = parseInt(b, 16);
                          const asciiChar =
                            code >= 32 && code <= 126 ? String.fromCharCode(code) : '.';

                          return {
                            byteHex: b,
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

                        const padCount = DATA_BYTES_PER_ROW - bytes.length;

                        return (
                          <div
                            key={idx}
                            data-row-index={idx}
                            className="flex py-0.5 hover:bg-slate-800/50"
                          >
                            <span className="mr-4 select-none text-slate-500">0x{offset}:</span>
                            <div className="text-terminal-dim mr-6 flex gap-1.5">
                              {byteEntries.map((entry) => {
                                return (
                                  <span
                                    key={entry.absoluteOffset}
                                    data-testid={`data-byte-${entry.absoluteOffset}`}
                                    className={`${entry.byteClass} ${
                                      entry.segmentIndex >= 0 ? 'cursor-pointer' : 'cursor-default'
                                    } ${entry.hoverBreatheClass}`}
                                    title={entry.title}
                                    onMouseEnter={() => {
                                      setHoveredDataByteOffset(entry.absoluteOffset);
                                      setHoveredDataSegmentIndex(
                                        entry.segmentIndex >= 0 ? entry.segmentIndex : null
                                      );
                                    }}
                                    onClick={() => {
                                      if (entry.segmentIndex < 0) return;
                                      setPinnedDataSegmentIndex((prev) =>
                                        prev === entry.segmentIndex ? null : entry.segmentIndex
                                      );
                                    }}
                                  >
                                    {entry.byteHex}
                                  </span>
                                );
                              })}
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
                                  data-testid={`data-ascii-byte-${entry.absoluteOffset}`}
                                  className={`inline-flex w-2.5 justify-center rounded-sm transition-colors duration-100 ${
                                    entry.segmentIndex >= 0 ? 'cursor-pointer' : 'cursor-default'
                                  } ${entry.asciiClass} ${entry.asciiHoverClass}`}
                                  title={entry.title}
                                  onMouseEnter={() => {
                                    setHoveredDataByteOffset(entry.absoluteOffset);
                                    setHoveredDataSegmentIndex(
                                      entry.segmentIndex >= 0 ? entry.segmentIndex : null
                                    );
                                  }}
                                  onClick={() => {
                                    if (entry.segmentIndex < 0) return;
                                    setPinnedDataSegmentIndex((prev) =>
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
                      {remainingBytes > 0 && (
                        <div className="mt-2 select-none italic text-slate-500">
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
