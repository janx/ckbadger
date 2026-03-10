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
    dot: 'bg-emphasis',
    activePill: 'border-emphasis/70 bg-emphasis/15 text-emphasis',
    valueText: 'text-emphasis',
    byte: 'rounded bg-emphasis/15 text-emphasis-dim',
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
type CapacitySegmentTone = {
  dot: string;
  legendActivePill: string;
  legendValueText: string;
};
const CAPACITY_SEGMENT_TONES: Record<string, CapacitySegmentTone> = {
  capacityFieldBytes: {
    dot: 'bg-base-border',
    legendActivePill: 'border-base-border/70 bg-base-border/15 text-text-primary',
    legendValueText: 'text-text-secondary',
  },
  lockScriptBytes: {
    dot: 'bg-emphasis',
    legendActivePill: 'border-emphasis/70 bg-emphasis/15 text-emphasis',
    legendValueText: 'text-emphasis',
  },
  typeScriptBytes: {
    dot: 'bg-info',
    legendActivePill: 'border-info/70 bg-info/15 text-info',
    legendValueText: 'text-info',
  },
  dataBytes: {
    dot: 'bg-warning-400',
    legendActivePill: 'border-warning-400/70 bg-warning/15 text-warning',
    legendValueText: 'text-warning',
  },
  inferredBytes: {
    dot: 'bg-[#9a5090]',
    legendActivePill: 'border-[#9a5090]/70 bg-[#9a5090]/15 text-[#7a4070]',
    legendValueText: 'text-[#7a4070]',
  },
};
const DeferredCellGraph = dynamic(() => import('@/components/cell-graph'), {
  loading: () => (
    <div className="border-base-border/70 bg-base-surface/70 flex h-[240px] items-center justify-center rounded border">
      <p className="text-text-muted text-sm">Loading graph section...</p>
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
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="bg-base-surface h-20 w-full rounded" />
            <div className="grid gap-6 lg:grid-cols-2">
              <div className="bg-base-surface h-80 rounded" />
              <div className="space-y-6">
                <div className="bg-base-surface h-36 rounded" />
                <div className="bg-base-surface h-36 rounded" />
              </div>
            </div>
          </div>
        </main>
      </div>
    );
  }
  if (error || !cell) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-muted text-xl">Cell not found</h2>
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
    <div className="bg-base-bg min-h-screen">
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
                <div className="border-base-border/70 bg-base-surface/60 rounded border p-3">
                  <div className="text-text-muted mb-1 text-xs uppercase tracking-wide">
                    Total Capacity
                  </div>
                  <Capacity
                    value={capacityView.totalCapacity}
                    className="text-text-primary text-lg"
                    animate={false}
                  />
                </div>
                <div className="border-base-border/70 bg-base-surface/60 rounded border p-3">
                  <div className="text-text-muted mb-1 text-xs uppercase tracking-wide">
                    Occupied Capacity
                  </div>
                  {capacityView.occupied !== null ? (
                    <Capacity
                      value={capacityView.occupied}
                      className="text-emphasis text-lg"
                      animate={false}
                    />
                  ) : (
                    <div className="text-text-muted font-mono text-lg">N/A</div>
                  )}
                </div>
                <div className="border-base-border/60 bg-base-surface/60 rounded border p-3">
                  <div className="text-text-muted mb-1 text-xs uppercase tracking-wide">
                    Utilization Ratio
                  </div>
                  <div className="text-text-primary font-mono text-xl">
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
                      <div className="text-text-muted text-xs uppercase tracking-wide">
                        Byte Composition ({capacityView.totalBytes.toLocaleString()} bytes)
                      </div>
                      {capacityView.formulaText && (
                        <div className="text-text-muted text-xs">
                          Formula: {capacityView.formulaText}
                        </div>
                      )}
                    </div>
                    <div className="border-base-border/80 bg-base-surface/80 overflow-hidden rounded border">
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
                              ? 'border-base-border/50 bg-base-surface/60 text-text-secondary'
                              : hoveredSegmentKey === segment.key
                                ? segment.legendActivePill
                                : 'border-base-border/60 bg-base-surface/40 text-text-muted'
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
                                ? 'text-text-muted'
                                : hoveredSegmentKey === segment.key
                                  ? segment.legendValueText
                                  : 'text-text-muted'
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
                <div className="text-text-muted mt-4 text-sm">
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
                  <span className="text-text-muted text-xs">
                    {relationshipStats.nodeCount} nodes / {relationshipStats.linkCount} links
                  </span>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="space-y-3">
                <div className="border-base-border/70 bg-base-surface/60 inline-flex rounded border p-1">
                  <button
                    type="button"
                    className={`rounded px-2.5 py-1 text-xs transition-colors ${
                      relationshipView === 'lifecycle'
                        ? 'bg-base-elevated text-text-primary ring-base-border ring-1'
                        : 'text-text-muted hover:text-text-primary'
                    }`}
                    onClick={() => setRelationshipView('lifecycle')}
                  >
                    Lifecycle
                  </button>
                  <button
                    type="button"
                    className={`rounded px-2.5 py-1 text-xs transition-colors ${
                      relationshipView === 'graph'
                        ? 'bg-base-elevated text-text-primary ring-base-border ring-1'
                        : 'text-text-muted hover:text-text-primary'
                    }`}
                    onClick={() => setRelationshipView('graph')}
                  >
                    Graph
                  </button>
                </div>
                {relationshipView === 'lifecycle' ? (
                  <div data-testid="cell-relationship-lifecycle" className="space-y-2.5">
                    <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                      <div className="text-text-muted text-xs uppercase tracking-wide">Created</div>
                      <div className="text-text-primary mt-1 flex flex-wrap items-center gap-2 text-sm">
                        <span className="text-text-muted">TX</span>
                        <Link
                          href={`/tx/${cell.txHash}`}
                          className="text-emphasis font-mono hover:underline"
                        >
                          {shortenHash(cell.txHash)}
                        </Link>
                        <span className="text-text-muted">Output #{cell.outputIndex}</span>
                        <span className="text-text-muted">at</span>
                        <Link
                          href={`/blocks/${cell.createdAtBlock}`}
                          className="text-emphasis hover:underline"
                        >
                          #{cell.createdAtBlock.toLocaleString()}
                        </Link>
                      </div>
                    </div>
                    <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                      <div className="text-text-muted text-xs uppercase tracking-wide">
                        Current Status
                      </div>
                      <div className="mt-1 flex flex-wrap items-center gap-2 text-sm">
                        <Badge variant={isLive ? 'green' : 'red'}>{isLive ? 'Live' : 'Dead'}</Badge>
                        {isLive ? (
                          <span className="text-text-secondary">
                            Unspent cell available in current state.
                          </span>
                        ) : (
                          <span className="text-text-secondary">
                            Cell was consumed by a later transaction.
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                      <div className="text-text-muted text-xs uppercase tracking-wide">
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
                                className="text-emphasis font-mono hover:underline"
                              >
                                {shortenHash(input.txHash)}:{input.outputIndex}
                              </Link>
                              {input.capacity && (
                                <span className="text-text-muted font-mono">
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
                        <div className="text-text-muted mt-1 text-sm">
                          No upstream input cells found.
                        </div>
                      )}
                    </div>
                    {!isLive && (
                      <div className="border-base-border/70 bg-base-surface/70 rounded border p-3">
                        <div className="text-text-muted text-xs uppercase tracking-wide">
                          Consumed
                        </div>
                        {cell.consumedByTx ? (
                          <div className="text-text-primary mt-1 flex flex-wrap items-center gap-2 text-sm">
                            <span className="text-text-muted">TX</span>
                            <Link
                              href={`/tx/${cell.consumedByTx}`}
                              className="text-emphasis font-mono hover:underline"
                            >
                              {shortenHash(cell.consumedByTx)}
                            </Link>
                            {cell.consumedAtBlock && (
                              <>
                                <span className="text-text-muted">at</span>
                                <Link
                                  href={`/blocks/${cell.consumedAtBlock}`}
                                  className="text-emphasis hover:underline"
                                >
                                  #{cell.consumedAtBlock.toLocaleString()}
                                </Link>
                              </>
                            )}
                          </div>
                        ) : (
                          <div className="text-text-muted mt-1 text-sm">
                            Consuming transaction not indexed in this graph view.
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                ) : graphData && graphData.nodes.length > 0 ? (
                  <div className="space-y-2">
                    <div className="border-base-border/70 bg-base-surface/70 text-text-muted rounded border px-3 py-2 text-xs">
                      <span className="inline-flex items-center gap-2">
                        <span className="bg-warning-300 h-2 w-2 rounded-full" />
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
                  <div className="text-text-muted flex h-[220px] items-center justify-center text-sm">
                    Graph data unavailable
                  </div>
                )}
              </div>
              {cell.cellType === 'genesis_special_burn' && (
                <div className="border-warning/30 bg-warning/10 mt-4 rounded-lg border p-4">
                  <div className="text-warning text-sm font-medium">Genesis Special Burn Cell</div>
                  <div className="text-text-secondary mt-2 text-sm">
                    <p>
                      This cell contains 8.4B CKB burnt at genesis (25% of 33.6B initial issuance).
                      For secondary issuance calculation,{' '}
                      <strong className="text-warning">5.04B CKB (60%)</strong> is treated as
                      &ldquo;occupied&rdquo; capacity, ensuring miners receive secondary rewards.
                    </p>
                    <p className="mt-2">
                      <span className="text-text-muted">Virtual Occupied Capacity: </span>
                      <span className="text-warning font-mono">5,040,000,000 CKB</span>
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
                  <div className="text-text-primary flex flex-wrap items-center gap-2 text-sm">
                    {cell.address ? (
                      <Address address={cell.address} />
                    ) : (
                      <Link
                        href={`/address/${cell.lockScriptHash}`}
                        className="text-emphasis hover:underline"
                      >
                        <HexDisplay value={cell.lockScriptHash} />
                      </Link>
                    )}
                    {lockScriptInfo && (
                      <Link
                        href={`/scripts/${encodeURIComponent(lockScriptInfo.name)}`}
                        className="text-emphasis hover:underline"
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
                  <div className="text-text-muted text-xs">Deposit Block</div>
                  <Link
                    href={`/blocks/${cell.daoInfo.depositBlockNumber}`}
                    className="text-emphasis hover:underline"
                  >
                    #{cell.daoInfo.depositBlockNumber.toLocaleString()}
                  </Link>
                </div>
                <div>
                  <div className="text-text-muted text-xs">Deposit Time</div>
                  <span className="text-text-primary">
                    {new Date(cell.daoInfo.depositTimestamp).toLocaleString()}
                  </span>
                </div>
                {cell.daoInfo.withdrawRequestBlock && (
                  <div>
                    <div className="text-text-muted text-xs">Withdraw Request</div>
                    <Link
                      href={`/blocks/${cell.daoInfo.withdrawRequestBlock}`}
                      className="text-emphasis hover:underline"
                    >
                      #{cell.daoInfo.withdrawRequestBlock.toLocaleString()}
                    </Link>
                  </div>
                )}
                {cell.daoInfo.withdrawRequestTimestamp && (
                  <div>
                    <div className="text-text-muted text-xs">Request Time</div>
                    <span className="text-text-primary">
                      {new Date(cell.daoInfo.withdrawRequestTimestamp).toLocaleString()}
                    </span>
                  </div>
                )}
                {cell.daoInfo.withdrawBlock && (
                  <div>
                    <div className="text-text-muted text-xs">Withdrawn Block</div>
                    <Link
                      href={`/blocks/${cell.daoInfo.withdrawBlock}`}
                      className="text-emphasis hover:underline"
                    >
                      #{cell.daoInfo.withdrawBlock.toLocaleString()}
                    </Link>
                  </div>
                )}
                {cell.daoInfo.withdrawTimestamp && (
                  <div>
                    <div className="text-text-muted text-xs">Withdrawn Time</div>
                    <span className="text-text-primary">
                      {new Date(cell.daoInfo.withdrawTimestamp).toLocaleString()}
                    </span>
                  </div>
                )}
                {cell.daoInfo.compensation && (
                  <div>
                    <div className="text-text-muted text-xs">Compensation Earned</div>
                    <span className="text-text-primary font-mono">
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
              <p className="text-text-muted mb-4 text-sm">
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
                          className="text-emphasis text-lg font-medium hover:underline"
                        >
                          {hasKnownScriptName(script.name)
                            ? script.name.trim()
                            : scriptFallbackLabel(script.codeHash)}
                        </Link>
                        <div className="flex flex-wrap items-center gap-2 text-xs">
                          <span className="text-text-muted uppercase tracking-wide">Refs</span>
                          <Badge variant="gray">type</Badge>
                          {refs.typeHash ? (
                            <Link
                              href={getScriptRefHref(refs.typeHash, 'type')}
                              className="hover:text-emphasis text-text-secondary font-mono hover:underline"
                            >
                              <HexDisplay
                                value={refs.typeHash}
                                size="sm"
                                startChars={10}
                                endChars={8}
                              />
                            </Link>
                          ) : (
                            <span className="text-text-muted font-mono">Unavailable</span>
                          )}
                          <Badge variant="gray">{getScriptRefBadgeLabel(refs.dataHashType)}</Badge>
                          {refs.dataHash ? (
                            <Link
                              href={getScriptRefHref(refs.dataHash, refs.dataHashType)}
                              className="hover:text-emphasis text-text-secondary font-mono hover:underline"
                            >
                              <HexDisplay
                                value={refs.dataHash}
                                size="sm"
                                startChars={10}
                                endChars={8}
                              />
                            </Link>
                          ) : (
                            <span className="text-text-muted font-mono">Unavailable</span>
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
                      <span className="text-text-muted w-8 text-right font-mono text-sm">
                        #{idx}
                      </span>
                      <Link
                        href={`/cell/${item.txHash}-${item.outputIndex}`}
                        className="text-emphasis hover:underline"
                      >
                        <HexDisplay value={`${item.txHash}:${item.outputIndex}`} />
                      </Link>
                    </TerminalRow>
                  ))}
                </div>
              ) : (
                <p className="text-text-muted text-sm">
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
                <div className="border-base-border/70 bg-base-surface/70 inline-flex items-center gap-2 rounded border px-2.5 py-1.5">
                  <span className="text-text-muted uppercase tracking-wide">Total</span>
                  <span className="text-text-primary font-mono">
                    {cell.dataSize.toLocaleString()} bytes
                  </span>
                </div>
                <div
                  className={`inline-flex items-center gap-2 rounded border px-2.5 py-1.5 ${
                    isDataPreviewTruncated
                      ? 'border-warning/30 bg-warning/10'
                      : 'border-emphasis/25 bg-emphasis/5'
                  }`}
                >
                  <span className="text-text-muted uppercase tracking-wide">Preview</span>
                  {isDataPreviewTruncated ? (
                    <span className="text-warning font-mono">
                      Truncated at the {dataPreviewBytes.toLocaleString()}-th byte
                    </span>
                  ) : (
                    <span className="text-emphasis">Full data shown</span>
                  )}
                </div>
              </div>
              {deterministicAnalysis && (
                <div
                  data-testid="data-deterministic-section"
                  className="border-base-border bg-base-bg/70 mb-3 rounded border p-2"
                >
                  <div className="mb-1.5 flex flex-wrap items-center gap-1.5">
                    <span className="text-text-muted text-[10px] uppercase tracking-[0.12em]">
                      Deterministic Decode
                    </span>
                    <Badge variant="neutral">{deterministicAnalysis.kind}</Badge>
                    <span className="border-base-border/80 bg-base-surface/70 text-text-muted rounded border px-1.5 py-0.5 font-mono text-[10px]">
                      {deterministicAnalysis.segments.length} segments
                    </span>
                    {pinnedDataSegmentIndex !== null && (
                      <span data-testid="data-segment-pinned">
                        <Badge variant="amber">Pinned</Badge>
                      </span>
                    )}
                  </div>
                  <div className="text-text-secondary mb-1.5 text-[11px] leading-4">
                    {deterministicAnalysis.summary}
                  </div>
                  <div
                    data-testid="data-deterministic-columns"
                    className="grid gap-2 md:grid-cols-2"
                  >
                    <div className="border-base-border bg-base-bg/60 rounded border p-1.5">
                      <div className="text-text-muted mb-1 text-[10px] uppercase tracking-[0.12em]">
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
                                    ? 'border-base-border/70 bg-base-surface/60 text-text-primary hover:border-base-border/70'
                                    : 'border-base-border/70 bg-base-surface/40 text-text-muted'
                              }`}
                            >
                              <span
                                className={`h-1.5 w-1.5 shrink-0 rounded-full ${segmentTone.dot}`}
                              />
                              <span className="truncate">{segment.label}</span>
                              <span className="text-text-muted shrink-0 text-[10px]">
                                [{segment.start}..{segment.end})
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                    <div
                      data-testid="data-active-segment"
                      className="border-base-border bg-base-bg/70 h-[132px] overflow-y-auto rounded border p-2 sm:h-[144px]"
                    >
                      {activeDataSegment ? (
                        <>
                          <div className="text-text-muted text-[10px] uppercase tracking-[0.12em]">
                            Segment Detail
                          </div>
                          <div className="text-text-secondary mt-1 font-mono text-[11px]">
                            {activeDataSegment.label}
                          </div>
                          <div className="text-text-muted mt-0.5 text-[10px] leading-4">
                            {activeDataSegment.meaning}
                          </div>
                          <div
                            data-testid="data-active-segment-value"
                            className={`mt-1 break-all font-mono text-sm ${activeDataSegmentTone?.valueText ?? 'text-emphasis'}`}
                          >
                            {activeDataSegment.humanValue}
                          </div>
                          <div className="text-text-secondary mt-1.5 font-mono text-[11px]">
                            [{activeDataSegment.start}..{activeDataSegment.end})
                          </div>
                          {activeDataSegmentHex && (
                            <div
                              data-testid="data-active-segment-hex"
                              className={`mt-1 break-all font-mono text-[11px] ${activeDataSegmentTone?.valueText ?? 'text-emphasis'}`}
                            >
                              {activeDataSegmentHex.value}
                            </div>
                          )}
                          {activeDataSegmentHex?.truncated && (
                            <div className="text-text-muted mt-1 text-[11px]">
                              Hex preview truncated for readability.
                            </div>
                          )}
                        </>
                      ) : (
                        <>
                          <div className="text-text-muted text-[10px] uppercase tracking-[0.12em]">
                            Segment Detail
                          </div>
                          <div className="text-text-muted mt-1 text-xs">
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
                  className="border-base-border bg-base-bg/70 mb-3 rounded border p-2"
                >
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <div className="text-text-muted text-[10px] uppercase tracking-[0.12em]">
                      Heuristic Guesses
                    </div>
                    <span className="border-base-border/80 bg-base-surface/70 text-text-muted rounded border px-1.5 py-0.5 font-mono text-[10px]">
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
                              : 'border-base-border/80 bg-base-surface/70 hover:border-base-border/80'
                          }`}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <div className="min-w-0">
                              <div className="flex flex-wrap items-center gap-1">
                                <span
                                  className={`h-1.5 w-1.5 shrink-0 rounded-full ${guessTone.dot}`}
                                />
                                <span className="text-text-primary font-mono text-[11px]">
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
                            <span className="text-text-muted font-mono text-[10px]">
                              {isExpanded ? '[-]' : '[+]'}
                            </span>
                          </div>
                          {isExpanded && (
                            <div
                              data-testid={`data-heuristic-detail-${idx}`}
                              className="border-base-border/80 mt-1 border-t pt-1"
                            >
                              <div className="text-text-muted text-[10px] leading-4">
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
              <div className="border-base-border bg-base-bg overflow-x-auto rounded-md border p-4 font-mono text-xs">
                {(() => {
                  const rawData = dataPreview.rawData;
                  if (!rawData) {
                    return (
                      <div className="text-text-muted">
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
                                ? 'text-text-muted'
                                : 'rounded bg-base-elevated/70 text-text-secondary'
                              : isActiveSegment
                                ? (segmentTone?.byteActive ??
                                  'rounded bg-emphasis/25 text-emphasis ring-1 ring-emphasis/70')
                                : hasActiveSegment
                                  ? 'text-text-muted opacity-40'
                                  : (segmentTone?.byte ??
                                    'rounded bg-emphasis/15 text-emphasis-dim');
                          const asciiClass =
                            segmentIndex < 0
                              ? hasActiveSegment
                                ? 'text-text-muted'
                                : 'text-text-muted'
                              : isActiveSegment
                                ? (segmentTone?.asciiActive ??
                                  'rounded-sm bg-emphasis/20 text-emphasis')
                                : hasActiveSegment
                                  ? 'text-text-muted opacity-40'
                                  : 'text-text-muted';
                          const asciiHoverClass = isHoveredByte
                            ? segmentIndex >= 0
                              ? (segmentTone?.asciiHover ??
                                'rounded-sm bg-emphasis/30 text-emphasis shadow-[inset_0_0_0_1px_rgba(0,255,65,0.45)]')
                              : 'rounded-sm bg-base-border/50 text-text-primary'
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
                            className="hover:bg-base-elevated/50 flex py-0.5"
                          >
                            <span className="text-text-muted mr-4 select-none">0x{offset}:</span>
                            <div className="text-emphasis-dim mr-6 flex gap-1.5">
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
                            <div className="border-base-border border-l pl-4">
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
                        <div className="text-text-muted mt-2 select-none italic">
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
