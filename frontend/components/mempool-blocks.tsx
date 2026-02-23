'use client';

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import Link from 'next/link';
import { useQuery, useQueries, useQueryClient } from '@tanstack/react-query';
import { api, MempoolBlock, Block, BlockFeeStats, Transaction, PendingProposal } from '@/lib/api';
import { resolveBubbleOverlaps } from '@/lib/pipeline-bubble-layout';
import { computeUniformShiftDeltaX } from '@/lib/pipeline-animation';
import { cn } from '@/lib/utils';

interface MempoolBlocksProps {
  latestBlocks?: Block[];
  chrome?: 'card' | 'flat';
  showHeader?: boolean;
  showTxnLens?: boolean;
  legendMode?: 'row' | 'none';
}

type LensStage = 'mempool' | 'proposed' | 'committed';

interface LensTxItem {
  id: string;
  stage: LensStage;
  isCellbase: boolean;
  proposalId?: string | null;
  size: number;
  fee?: number | null;
  feeRate?: number | null;
  cycles?: number | null;
}

interface TxBubble {
  id: string;
  left: number;
  top: number;
  widthPx: number;
  heightPx: number;
  color: string;
  border: string;
  opacity: number;
  title: string;
  txLabel: string;
  proposalLabel?: string;
  stageLabel: string;
  sizeLabel: string;
  feeLabel: string;
  feeRateLabel: string;
  cyclesLabel: string;
}

const MAX_LENS_STAGE_ITEMS = 24;

function toSafePositive(value: number | null | undefined, fallback: number): number {
  if (typeof value !== 'number' || Number.isNaN(value) || !Number.isFinite(value) || value <= 0) {
    return fallback;
  }
  return value;
}

function sampleItems<T>(items: T[], maxCount: number): T[] {
  if (items.length <= maxCount) return items;
  if (maxCount <= 1) return items.slice(0, 1);

  const sampled: T[] = [];
  const step = (items.length - 1) / (maxCount - 1);

  for (let i = 0; i < maxCount; i += 1) {
    sampled.push(items[Math.round(i * step)]);
  }

  return sampled;
}

function clamp01(value: number): number {
  if (Number.isNaN(value) || !Number.isFinite(value)) return 0;
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

function seededUnit(seed: string): number {
  let hash = 0;
  for (let i = 0; i < seed.length; i += 1) {
    hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  }
  return (hash % 1000) / 999;
}

interface RectDomain {
  sizeMin: number;
  sizeMax: number;
  cyclesMin: number;
  cyclesMax: number;
  feeMin: number;
  feeMax: number;
  feeRateMin: number;
  feeRateMax: number;
}

const DEFAULT_RECT_DOMAIN: RectDomain = {
  sizeMin: 200,
  sizeMax: 10_000,
  cyclesMin: 10_000,
  cyclesMax: 5_000_000,
  feeMin: 500,
  feeMax: 5_000_000,
  feeRateMin: 1,
  feeRateMax: 5_000,
};

function normalizeLog(value: number, min: number, max: number): number {
  const safeValue = Math.max(value, 0.000001);
  const safeMin = Math.max(min, 0.000001);
  const safeMax = Math.max(max, safeMin + 0.000001);
  const logMin = Math.log(safeMin);
  const logMax = Math.log(safeMax);
  if (logMax <= logMin) return 0.5;
  return clamp01((Math.log(safeValue) - logMin) / (logMax - logMin));
}

function minMax(values: number[], fallbackMin: number, fallbackMax: number): [number, number] {
  if (values.length === 0) return [fallbackMin, fallbackMax];
  const minValue = Math.min(...values);
  const maxValue = Math.max(...values);

  if (minValue === maxValue) {
    if (minValue <= 1) return [0.000001, minValue + 1];
    return [minValue * 0.8, minValue * 1.2];
  }

  return [minValue, maxValue];
}

function buildRectDomain(items: LensTxItem[]): RectDomain {
  if (items.length === 0) return DEFAULT_RECT_DOMAIN;

  const sizes = items.map((item) => item.size).filter((value) => value > 0);
  const cycles = items
    .map((item) => item.cycles)
    .filter((value): value is number => value !== null && value !== undefined && value > 0);
  const fees = items
    .map((item) => item.fee)
    .filter((value): value is number => value !== null && value !== undefined && value > 0);
  const feeRates = items
    .map((item) => item.feeRate)
    .filter((value): value is number => value !== null && value !== undefined && value > 0);

  const [sizeMin, sizeMax] = minMax(
    sizes,
    DEFAULT_RECT_DOMAIN.sizeMin,
    DEFAULT_RECT_DOMAIN.sizeMax
  );
  const [cyclesMin, cyclesMax] = minMax(
    cycles,
    DEFAULT_RECT_DOMAIN.cyclesMin,
    DEFAULT_RECT_DOMAIN.cyclesMax
  );
  const [feeMin, feeMax] = minMax(fees, DEFAULT_RECT_DOMAIN.feeMin, DEFAULT_RECT_DOMAIN.feeMax);
  const [feeRateMin, feeRateMax] = minMax(
    feeRates,
    DEFAULT_RECT_DOMAIN.feeRateMin,
    DEFAULT_RECT_DOMAIN.feeRateMax
  );

  return {
    sizeMin,
    sizeMax,
    cyclesMin,
    cyclesMax,
    feeMin,
    feeMax,
    feeRateMin,
    feeRateMax,
  };
}

interface RectMetrics {
  sizeScore: number;
  cyclesScore: number;
  feeScore: number;
  feeRateScore: number;
  missingCycles: boolean;
  missingFee: boolean;
  missingFeeRate: boolean;
}

function mapTxToRectMetrics(tx: LensTxItem, domain: RectDomain): RectMetrics {
  const hasCycles = tx.cycles !== null && tx.cycles !== undefined && tx.cycles > 0;
  const hasFee = tx.fee !== null && tx.fee !== undefined && tx.fee > 0;
  const hasFeeRate = tx.feeRate !== null && tx.feeRate !== undefined && tx.feeRate > 0;

  return {
    sizeScore: normalizeLog(Math.max(tx.size, 0.000001), domain.sizeMin, domain.sizeMax),
    cyclesScore: hasCycles
      ? normalizeLog(Math.max(tx.cycles ?? 0.000001, 0.000001), domain.cyclesMin, domain.cyclesMax)
      : 0,
    feeScore: hasFee
      ? normalizeLog(Math.max(tx.fee ?? 0.000001, 0.000001), domain.feeMin, domain.feeMax)
      : 0,
    feeRateScore: hasFeeRate
      ? normalizeLog(
          Math.max(tx.feeRate ?? 0.000001, 0.000001),
          domain.feeRateMin,
          domain.feeRateMax
        )
      : 0,
    missingCycles: !hasCycles,
    missingFee: !hasFee,
    missingFeeRate: !hasFeeRate,
  };
}

function median(values: number[]): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  if (sorted.length % 2 === 0) {
    return (sorted[mid - 1] + sorted[mid]) / 2;
  }
  return sorted[mid];
}

function syntheticBlock(index: number, items: LensTxItem[]): MempoolBlock {
  if (items.length === 0) {
    return {
      index,
      transactionCount: 0,
      totalSize: 0,
      totalFee: 0,
      totalCycles: 0,
      feeRateRange: { min: 0, max: 0 },
      medianFeeRate: 0,
      estimatedTimeMinutes: index + 1,
    };
  }

  const feeRates = items
    .map((item) => item.feeRate)
    .filter((value): value is number => value !== null && value !== undefined && value > 0);
  const totalSize = items.reduce((sum, item) => sum + Math.max(0, Math.round(item.size)), 0);
  const totalFee = items.reduce((sum, item) => sum + Math.max(0, Math.round(item.fee ?? 0)), 0);
  const totalCycles = items.reduce(
    (sum, item) => sum + Math.max(0, Math.round(item.cycles ?? 0)),
    0
  );

  return {
    index,
    transactionCount: items.length,
    totalSize,
    totalFee,
    totalCycles,
    feeRateRange:
      feeRates.length > 0
        ? { min: Math.min(...feeRates), max: Math.max(...feeRates) }
        : { min: 0, max: 0 },
    medianFeeRate: feeRates.length > 0 ? median(feeRates) : 0,
    estimatedTimeMinutes: index + 1,
  };
}

function shortProposalLabel(proposalId: string | null | undefined): string | undefined {
  if (!proposalId) return undefined;
  if (proposalId.length <= 16) return proposalId;
  return `${proposalId.slice(0, 10)}...${proposalId.slice(-4)}`;
}

function proposalPriorityScore(proposal: PendingProposal): number {
  const feeRate = Math.max(0, proposal.feeRate ?? 0);
  const urgency = 1 / Math.max(1, proposal.blocksUntilExpiry + 1);
  return Math.log10(feeRate + 1) * 6 + urgency * 14;
}

function splitProposalBuckets(proposals: PendingProposal[]): {
  nextBlockProposals: PendingProposal[];
  backlogProposals: PendingProposal[];
} {
  if (proposals.length === 0) {
    return { nextBlockProposals: [], backlogProposals: [] };
  }

  const ranked = [...proposals].sort((a, b) => proposalPriorityScore(b) - proposalPriorityScore(a));
  const urgentIds = new Set(
    ranked
      .filter((proposal) => proposal.blocksUntilExpiry <= 2)
      .map((proposal) => proposal.proposalId)
  );
  const baseCount = Math.max(1, Math.min(MAX_LENS_STAGE_ITEMS, Math.round(ranked.length * 0.4)));
  const selectedIds = new Set<string>();

  ranked.forEach((proposal) => {
    if (selectedIds.size >= baseCount) return;
    selectedIds.add(proposal.proposalId);
  });

  ranked.forEach((proposal) => {
    if (selectedIds.size >= MAX_LENS_STAGE_ITEMS) return;
    if (urgentIds.has(proposal.proposalId)) {
      selectedIds.add(proposal.proposalId);
    }
  });

  const nextBlockProposals = ranked.filter((proposal) => selectedIds.has(proposal.proposalId));
  const backlogProposals = ranked.filter((proposal) => !selectedIds.has(proposal.proposalId));

  return { nextBlockProposals, backlogProposals };
}

function lensColor(
  stage: LensStage,
  feeScore: number,
  missing: boolean,
  isCellbase: boolean
): string {
  if (isCellbase) return 'rgba(74, 222, 128, 0.24)';
  if (missing) return 'rgba(148, 163, 184, 0.55)';

  const alpha = 0.35 + feeScore * 0.5;
  if (stage === 'mempool') return `rgba(255, 176, 0, ${alpha})`;
  if (stage === 'proposed') return `rgba(0, 204, 51, ${alpha})`;
  return `rgba(0, 255, 65, ${alpha})`;
}

function mempoolTxToLensItem(tx: {
  txHash: string;
  fee: number;
  size: number;
  feeRate: number;
  cycles: number;
}): LensTxItem {
  return {
    id: tx.txHash,
    stage: 'mempool',
    isCellbase: false,
    size: toSafePositive(tx.size, 220),
    fee: tx.fee,
    feeRate: tx.feeRate,
    cycles: tx.cycles,
  };
}

function proposalToLensItem(
  proposal: {
    proposalId: string;
    fullTxHash: string | null;
    fee: number | null;
    size: number | null;
    feeRate: number | null;
    cycles: number | null;
  },
  tx?: Transaction
): LensTxItem {
  const txFee = tx ? parseFloat(tx.fee) || null : null;
  const mergedFee = proposal.fee ?? txFee;
  const mergedSize = proposal.size ?? tx?.txSize ?? null;
  const mergedCycles = proposal.cycles ?? tx?.cycles ?? null;
  const mergedFeeRate =
    proposal.feeRate ?? (mergedFee && mergedSize && mergedSize > 0 ? mergedFee / mergedSize : null);

  return {
    id: proposal.fullTxHash || proposal.proposalId,
    stage: 'proposed',
    isCellbase: false,
    proposalId: proposal.proposalId,
    size: toSafePositive(mergedSize, 220),
    fee: mergedFee,
    feeRate: mergedFeeRate,
    cycles: mergedCycles,
  };
}

function committedTxToLensItem(tx: Transaction): LensTxItem {
  const fee = parseFloat(tx.fee) || 0;
  const feeRate = tx.txSize && tx.txSize > 0 ? fee / tx.txSize : null;

  return {
    id: tx.hash,
    stage: 'committed',
    isCellbase: tx.isCellbase,
    size: toSafePositive(tx.txSize, 220),
    fee,
    feeRate,
    cycles: tx.cycles ?? null,
  };
}

function formatFeeRate(rate: number): string {
  if (rate < 0.01) return '<0.01';
  if (rate >= 1000) return `${(rate / 1000).toFixed(1)}K`;
  return rate.toFixed(2);
}

function formatLensFeeRate(rate: number | null | undefined): string {
  if (!rate || rate <= 0) return 'N/A';
  return `${rate.toFixed(2)} sh/B`;
}

function formatLensFee(fee: number | null | undefined): string {
  if (!fee || fee <= 0) return 'N/A';
  if (fee >= 1_000_000) return `${(fee / 1_000_000).toFixed(2)}M sh`;
  if (fee >= 1_000) return `${(fee / 1_000).toFixed(2)}K sh`;
  return `${fee.toFixed(0)} sh`;
}

function formatLensCycles(cycles: number | null | undefined): string {
  if (!cycles || cycles <= 0) return 'N/A';
  return Math.round(cycles).toLocaleString();
}

function txBubbleTitle(item: LensTxItem): string {
  const proposal = shortProposalLabel(item.proposalId);
  return [
    `TX: ${item.id.slice(0, 10)}...${item.id.slice(-6)}`,
    item.isCellbase ? 'Type: Cellbase' : null,
    proposal ? `Proposal: ${proposal}` : null,
    `Stage: ${item.stage}`,
    `Size: ${Math.round(item.size).toLocaleString()} B`,
    `Fee: ${formatLensFee(item.fee)}`,
    `Fee rate: ${formatLensFeeRate(item.feeRate)}`,
    `Cycles: ${formatLensCycles(item.cycles)}`,
  ]
    .filter((line): line is string => Boolean(line))
    .join('\n');
}

function toTxBubble(item: LensTxItem, metrics: RectMetrics, jitterSeed: string): TxBubble {
  const jitterX = (seededUnit(`${jitterSeed}:x`) - 0.5) * 0.18;
  const jitterY = (seededUnit(`${jitterSeed}:y`) - 0.5) * 0.16;
  const rawLeft = clamp01(metrics.feeScore + jitterX);
  const rawTop = clamp01(1 - metrics.feeRateScore + jitterY);
  const plotPadding = 0.08;
  const left = plotPadding + rawLeft * (1 - plotPadding * 2);
  const top = plotPadding + rawTop * (1 - plotPadding * 2);
  const widthPx = 8 + metrics.sizeScore * 18;
  const heightPx = 6 + metrics.cyclesScore * 14;
  const hasMissingMetrics = metrics.missingCycles || metrics.missingFee || metrics.missingFeeRate;
  const isCellbase = item.isCellbase;

  return {
    id: item.id,
    left,
    top,
    widthPx,
    heightPx,
    color: lensColor(item.stage, metrics.feeRateScore, hasMissingMetrics, isCellbase),
    border: isCellbase
      ? '1px solid rgba(167, 243, 208, 0.72)'
      : hasMissingMetrics
        ? '1px dashed rgba(148, 163, 184, 0.82)'
        : '1px solid rgba(226, 232, 240, 0.55)',
    opacity: isCellbase ? 0.42 : hasMissingMetrics ? 0.56 : 0.9,
    title: txBubbleTitle(item),
    txLabel: `${item.id.slice(0, 10)}...${item.id.slice(-6)}`,
    proposalLabel: shortProposalLabel(item.proposalId),
    stageLabel: isCellbase ? `${item.stage} (cellbase)` : item.stage,
    sizeLabel: `${Math.round(item.size).toLocaleString()} B`,
    feeLabel: formatLensFee(item.fee),
    feeRateLabel: formatLensFeeRate(item.feeRate),
    cyclesLabel: formatLensCycles(item.cycles),
  };
}

function formatCapacity(shannons: number): string {
  const ckb = shannons / 100_000_000;
  if (ckb >= 1_000_000) return `${(ckb / 1_000_000).toFixed(2)}M`;
  if (ckb >= 1_000) return `${(ckb / 1_000).toFixed(2)}K`;
  return ckb.toFixed(2);
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)}MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(1)}KB`;
  return `${bytes}B`;
}

function formatTimeAgo(timestamp: string): string {
  const now = Date.now();
  const time = new Date(timestamp).getTime();
  const diff = Math.floor((now - time) / 1000);

  if (diff < 60) return `${diff}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

function formatCount(value: number): string {
  return value.toLocaleString();
}

function edgeFadeOpacity(distanceToEdge: number): number {
  if (distanceToEdge <= 0) return 0.38;
  if (distanceToEdge === 1) return 0.68;
  return 1;
}

function getBlockColors(index: number, isPending: boolean): string {
  if (!isPending) {
    return 'from-terminal-dark via-terminal-dim to-terminal-green';
  }
  const colorSets = [
    'from-amber-bright via-amber to-amber-dim',
    'from-amber via-amber-dim to-amber-dark',
    'from-amber-dim via-amber-dark to-slate-700',
    'from-slate-500 via-slate-600 to-slate-700',
    'from-slate-500 via-slate-600 to-slate-700',
  ];
  return colorSets[Math.min(index, colorSets.length - 1)];
}

function Block2D({
  gradient,
  isEmpty,
  large = false,
  transparent = false,
  borderClassName,
  children,
}: {
  gradient: string;
  isEmpty?: boolean;
  large?: boolean;
  transparent?: boolean;
  borderClassName?: string;
  children: React.ReactNode;
}) {
  const sizeClass = large
    ? 'h-[98px] w-[126px] sm:h-[114px] sm:w-[148px]'
    : 'h-[80px] w-[100px] sm:h-[96px] sm:w-[116px]';

  if (transparent) {
    return (
      <div className={sizeClass}>
        <div
          className={cn(
            'relative flex h-full w-full cursor-pointer flex-col items-center justify-center rounded-xl border-2 p-2 text-center transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-black/30',
            borderClassName || 'border-slate-500/70'
          )}
          style={{ backgroundColor: 'transparent' }}
        >
          <div className="relative z-10 flex h-full w-full flex-col">{children}</div>
        </div>
      </div>
    );
  }

  if (isEmpty) {
    return (
      <div className={sizeClass}>
        <div className="flex h-full w-full flex-col items-center justify-center rounded-xl border-2 border-dashed border-slate-600/50 bg-slate-800/40 backdrop-blur-sm">
          {children}
        </div>
      </div>
    );
  }

  return (
    <div className={sizeClass}>
      <div
        className={cn(
          'relative flex h-full w-full cursor-pointer flex-col items-center justify-center rounded-xl bg-gradient-to-br p-2 text-center transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-black/30',
          gradient
        )}
        style={{
          boxShadow:
            'inset 0 1px 1px rgba(255,255,255,0.15), inset 0 -1px 1px rgba(0,0,0,0.15), 0 4px 12px rgba(0,0,0,0.3)',
        }}
      >
        <div className="absolute inset-0 rounded-xl bg-gradient-to-b from-white/10 to-transparent" />
        <div className="relative z-10 flex h-full w-full flex-col">{children}</div>
      </div>
    </div>
  );
}

function TxBubbleLayer({
  bubbles,
  showAxes = false,
  glowClassName = 'to-terminal-green/10',
}: {
  bubbles: TxBubble[];
  showAxes?: boolean;
  glowClassName?: string;
}) {
  const [hovered, setHovered] = useState<{ bubble: TxBubble; x: number; y: number } | null>(null);
  if (bubbles.length === 0 && !showAxes) return null;

  const viewportWidth = typeof window === 'undefined' ? 1024 : window.innerWidth;
  const viewportHeight = typeof window === 'undefined' ? 768 : window.innerHeight;
  const tooltipWidth = 190;
  const tooltipHeight = 126;

  const tooltipX = (() => {
    if (!hovered) return 8;
    return Math.max(8, Math.min(hovered.x + 10, viewportWidth - tooltipWidth - 8));
  })();

  const tooltipY = (() => {
    if (!hovered) return 8;
    return Math.max(8, Math.min(hovered.y + 10, viewportHeight - tooltipHeight - 8));
  })();

  const tooltip = hovered ? (
    <div
      data-testid="tx-bubble-tooltip"
      className="pointer-events-none fixed z-[9999] min-w-[196px] rounded-xl border border-slate-700/55 bg-slate-900/95 px-3 py-2.5 text-[10px] text-slate-100 shadow-2xl shadow-black/45 backdrop-blur-md"
      style={{
        left: tooltipX,
        top: tooltipY,
      }}
    >
      <div className="border-b border-slate-700/55 pb-1.5 font-mono text-[10px] tracking-wide text-slate-200">
        {hovered.bubble.txLabel}
      </div>
      <div className="mt-1.5 text-slate-400">
        Stage: <span className="font-medium text-slate-100">{hovered.bubble.stageLabel}</span>
      </div>
      {hovered.bubble.proposalLabel && (
        <div className="text-slate-400">
          Proposal: <span className="font-mono text-slate-100">{hovered.bubble.proposalLabel}</span>
        </div>
      )}
      <div className="text-slate-400">
        Size: <span className="font-mono text-slate-100">{hovered.bubble.sizeLabel}</span>
      </div>
      <div className="text-slate-400">
        Fee: <span className="font-mono text-slate-100">{hovered.bubble.feeLabel}</span>
      </div>
      <div className="text-slate-400">
        Fee rate: <span className="font-mono text-slate-100">{hovered.bubble.feeRateLabel}</span>
      </div>
      <div className="text-slate-400">
        Cycles: <span className="font-mono text-slate-100">{hovered.bubble.cyclesLabel}</span>
      </div>
    </div>
  ) : null;

  return (
    <div className="absolute inset-0 z-10 overflow-visible">
      <div className="absolute inset-0 overflow-hidden rounded-[inherit]">
        {showAxes && (
          <>
            <div
              data-testid="tx-bubble-layer-glow"
              className={cn(
                'pointer-events-none absolute inset-1 rounded-[inherit] bg-gradient-to-br from-white/[0.02] via-transparent',
                glowClassName
              )}
            />
            {[0.33, 0.66].map((ratio) => (
              <div
                key={`vertical-guide-${ratio}`}
                className="pointer-events-none absolute bottom-2 top-2 border-l border-dashed border-slate-300/15"
                style={{ left: `calc(${ratio * 100}% - 0.5px)` }}
              />
            ))}
            {[0.33, 0.66].map((ratio) => (
              <div
                key={`horizontal-guide-${ratio}`}
                className="pointer-events-none absolute left-2 right-2 border-t border-dashed border-slate-300/15"
                style={{ top: `calc(${ratio * 100}% - 0.5px)` }}
              />
            ))}
            <div className="pointer-events-none absolute bottom-2 left-2 right-2 h-px bg-slate-200/25" />
            <div className="pointer-events-none absolute bottom-2 left-2 top-2 w-px bg-slate-200/25" />
            <div className="pointer-events-none absolute inset-1.5 grid grid-cols-8 grid-rows-7 opacity-20">
              {Array.from({ length: 56 }, (_, idx) => (
                <div key={idx} className="border border-slate-200/10" />
              ))}
            </div>
          </>
        )}
        {bubbles.map((bubble) => (
          <div
            key={bubble.id}
            className="absolute transition-transform duration-150 hover:z-20 hover:scale-105"
            style={{
              width: bubble.widthPx,
              height: bubble.heightPx,
              left: `calc(${bubble.left * 100}% - ${bubble.widthPx / 2}px)`,
              top: `calc(${bubble.top * 100}% - ${bubble.heightPx / 2}px)`,
              backgroundColor: bubble.color,
              border: bubble.border,
              opacity: bubble.opacity,
              borderRadius: '2px',
            }}
            data-tx-tooltip={bubble.title}
            onMouseEnter={(event) => {
              setHovered({ bubble, x: event.clientX, y: event.clientY });
            }}
            onMouseMove={(event) => {
              setHovered({ bubble, x: event.clientX, y: event.clientY });
            }}
            onMouseLeave={() => setHovered(null)}
          />
        ))}
      </div>
      {tooltip && typeof document !== 'undefined' ? createPortal(tooltip, document.body) : null}
    </div>
  );
}

type PendingTone = 'mempool' | 'proposals' | 'next';

function PendingBlock({
  block,
  predictedNumber,
  topLabel,
  tone = 'next',
  isNextBlock,
  large = false,
  bubbles = [],
}: {
  block: MempoolBlock;
  predictedNumber?: number;
  topLabel?: string;
  tone?: PendingTone;
  isNextBlock: boolean;
  large?: boolean;
  bubbles?: TxBubble[];
}) {
  const isEmpty = block.transactionCount === 0;
  const gradient = isEmpty
    ? ''
    : tone === 'mempool'
      ? 'from-amber-bright via-amber to-amber-dim'
      : tone === 'proposals'
        ? 'from-terminal-dark via-terminal-dim to-terminal-green'
        : large
          ? 'from-terminal-dark via-terminal-dim to-terminal-green'
          : getBlockColors(block.index, true);
  const effectiveGradient =
    isEmpty && large ? 'from-slate-700 via-slate-800 to-slate-900' : gradient;
  const borderClassName = isEmpty
    ? 'border-slate-500/70'
    : tone === 'mempool'
      ? 'border-amber/70'
      : tone === 'proposals' || tone === 'next'
        ? 'border-terminal-dim/70'
        : 'border-terminal-green/70';
  const topLabelClass = isEmpty
    ? 'text-slate-500'
    : tone === 'mempool'
      ? 'text-amber'
      : tone === 'proposals' || tone === 'next'
        ? 'text-terminal-dim'
        : 'text-terminal-green';
  const glowClassName = isEmpty
    ? 'to-slate-500/[0.20]'
    : tone === 'mempool'
      ? 'to-amber/[0.12]'
      : tone === 'proposals' || tone === 'next'
        ? 'to-terminal-dim/[0.12]'
        : 'to-terminal-green/10';

  return (
    <div className="flex flex-col items-center">
      <div
        className={cn(
          'mb-1 font-mono text-xs font-semibold tabular-nums sm:text-sm',
          topLabelClass
        )}
      >
        {topLabel ?? predictedNumber?.toLocaleString()}
      </div>
      <Block2D
        gradient={effectiveGradient}
        isEmpty={isEmpty && !large}
        large={large}
        transparent={large}
        borderClassName={borderClassName}
      >
        {large ? (
          <div className="relative h-full w-full overflow-visible rounded-lg border border-white/25 bg-transparent">
            <TxBubbleLayer bubbles={bubbles} showAxes glowClassName={glowClassName} />
          </div>
        ) : isEmpty ? (
          <>
            <div className="text-[11px] font-medium text-slate-400">Empty</div>
            <div className="text-[10px] text-slate-500">0 txs</div>
          </>
        ) : (
          <div className="flex h-full w-full flex-col gap-1">
            <div className="relative h-10 overflow-visible rounded-md border border-white/20 bg-black/25">
              <TxBubbleLayer bubbles={bubbles} />
            </div>
            <div className="flex flex-col items-center">
              <div className="font-mono text-[10px] font-bold tabular-nums text-white drop-shadow-sm sm:text-[11px]">
                ~{formatFeeRate(block.medianFeeRate)} sh/B
              </div>
              <div className="font-mono text-[8px] tabular-nums text-white/60">
                {formatFeeRate(block.feeRateRange.min)}-{formatFeeRate(block.feeRateRange.max)}
              </div>
            </div>
            <div className="flex flex-col items-center">
              <div className="font-mono text-[10px] font-bold tabular-nums text-white drop-shadow-sm sm:text-xs">
                {formatCapacity(block.totalFee)} CKB
              </div>
              <div className="text-[9px] font-medium text-white/70">
                {block.transactionCount} txs
              </div>
            </div>
          </div>
        )}
      </Block2D>
      {large && (
        <div className="mt-1.5 text-center text-[10px] sm:text-[11px]">
          <div
            className={cn('font-mono tabular-nums', isEmpty ? 'text-slate-500' : 'text-white/90')}
          >
            ~{formatFeeRate(block.medianFeeRate)} sh/B
          </div>
          <div className="font-mono tabular-nums text-white/55">
            {formatFeeRate(block.feeRateRange.min)}-{formatFeeRate(block.feeRateRange.max)}
          </div>
          <div className="text-white/65">
            {formatCapacity(block.totalFee)} CKB · {block.transactionCount} txs
          </div>
        </div>
      )}
      {isNextBlock && (
        <div className="bg-terminal-dim/15 text-terminal-dim mt-1.5 rounded-full px-2 py-0.5 text-[10px] font-medium sm:text-xs">
          Next Block
        </div>
      )}
    </div>
  );
}

function MinedBlock({
  block,
  feeStats,
  large = false,
  bubbles = [],
}: {
  block: Block;
  feeStats?: BlockFeeStats | 'loading';
  large?: boolean;
  bubbles?: TxBubble[];
}) {
  const isLoading = feeStats === 'loading';
  const stats = feeStats && feeStats !== 'loading' ? feeStats : null;
  const gradient = getBlockColors(0, false);
  const borderClassName = 'border-terminal-green/70';
  const glowClassName = 'to-terminal-green/10';
  const anchorRef = useRef<HTMLAnchorElement | null>(null);
  const [isHovered, setIsHovered] = useState(false);
  const [anchorRect, setAnchorRect] = useState<DOMRect | null>(null);

  useEffect(() => {
    if (!isHovered) return;

    const updateAnchorRect = () => {
      if (!anchorRef.current) return;
      setAnchorRect(anchorRef.current.getBoundingClientRect());
    };

    updateAnchorRect();
    window.addEventListener('resize', updateAnchorRect);
    window.addEventListener('scroll', updateAnchorRect, true);

    return () => {
      window.removeEventListener('resize', updateAnchorRect);
      window.removeEventListener('scroll', updateAnchorRect, true);
    };
  }, [isHovered]);

  const tooltipViewportWidth = typeof window === 'undefined' ? 1024 : window.innerWidth;
  const tooltipWidth = 296;
  const tooltipHeight = stats ? 206 : 146;

  const tooltipLeft = anchorRect
    ? Math.max(
        8,
        Math.min(
          anchorRect.left + anchorRect.width / 2 - tooltipWidth / 2,
          tooltipViewportWidth - tooltipWidth - 8
        )
      )
    : 8;
  const tooltipTop = anchorRect ? Math.max(8, anchorRect.top - tooltipHeight - 12) : 8;

  const minedBlockTooltip = isHovered && anchorRect && typeof document !== 'undefined' && (
    <div
      data-testid={`mined-block-tooltip-${block.number}`}
      className="pointer-events-none fixed z-[10000] w-[296px] rounded-xl border border-slate-700/50 bg-slate-900/95 px-4 py-3 text-xs text-white shadow-2xl backdrop-blur-sm"
      style={{
        left: tooltipLeft,
        top: tooltipTop,
      }}
    >
      <div className="flex flex-col gap-1.5">
        <div className="border-b border-slate-700/50 pb-2 text-sm font-semibold">
          Block #{block.number.toLocaleString()}
        </div>
        {stats && (
          <>
            <div className="flex justify-between gap-4">
              <span className="text-slate-400">Avg Fee Rate:</span>
              <span className="font-mono tabular-nums">
                ~{formatFeeRate(stats.avgFeeRate)} shannons/B
              </span>
            </div>
            <div className="flex justify-between gap-4">
              <span className="text-slate-400">Fee Range:</span>
              <span className="font-mono tabular-nums">
                {formatFeeRate(stats.minFeeRate)} - {formatFeeRate(stats.maxFeeRate)}
              </span>
            </div>
            <div className="flex justify-between gap-4">
              <span className="text-slate-400">Size:</span>
              <span>{formatBytes(stats.totalSize)}</span>
            </div>
          </>
        )}
        <div className="flex justify-between gap-4">
          <span className="text-slate-400">Transactions:</span>
          <span>{block.transactionsCount}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-slate-400">Proposals:</span>
          <span>{block.proposalsCount ?? 0}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-slate-400">Time:</span>
          <span>{formatTimeAgo(block.timestamp)} ago</span>
        </div>
      </div>
    </div>
  );

  return (
    <Link
      ref={anchorRef}
      href={`/blocks/${block.number}`}
      className="group/block flex flex-col items-center"
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      onFocus={() => setIsHovered(true)}
      onBlur={() => setIsHovered(false)}
    >
      <div className="text-terminal-green group-hover/block:text-terminal-dim mb-1 font-mono text-xs font-semibold tabular-nums transition-colors sm:text-sm">
        {block.number.toLocaleString()}
      </div>
      {block.hardforkActivation && (
        <div
          className="mb-1 rounded border border-amber-900/60 bg-amber-900/30 px-1.5 py-0.5 font-mono text-[9px] text-amber-300"
          data-testid={`mempool-mined-hardfork-${block.number}`}
        >
          HF {block.hardforkActivation.shortName.toUpperCase()}
        </div>
      )}
      <div className="relative">
        <Block2D
          gradient={gradient}
          large={large}
          transparent={large}
          borderClassName={borderClassName}
        >
          {large ? (
            <div className="relative h-full w-full overflow-visible rounded-lg border border-white/25 bg-transparent">
              <TxBubbleLayer bubbles={bubbles} showAxes glowClassName={glowClassName} />
            </div>
          ) : isLoading ? (
            <div className="flex h-full w-full flex-col justify-between">
              <div className="flex flex-col items-center gap-1">
                <div className="h-3 w-14 animate-pulse rounded-md bg-white/20" />
                <div className="h-2 w-12 animate-pulse rounded-md bg-white/20" />
              </div>
              <div className="flex flex-col items-center gap-1">
                <div className="h-2 w-10 animate-pulse rounded-md bg-white/20" />
                <div className="text-[9px] font-medium text-white/60">
                  {block.transactionsCount} txs
                </div>
              </div>
            </div>
          ) : stats ? (
            <div className="flex h-full w-full flex-col gap-1">
              <div className="relative h-10 overflow-visible rounded-md border border-white/20 bg-black/25">
                <TxBubbleLayer bubbles={bubbles} />
              </div>
              <div className="flex flex-col items-center">
                <div className="font-mono text-[11px] font-bold tabular-nums text-white drop-shadow-sm sm:text-xs">
                  ~{formatFeeRate(stats.avgFeeRate)} sh/B
                </div>
                <div className="font-mono text-[8px] tabular-nums text-white/60 sm:text-[9px]">
                  {formatFeeRate(stats.minFeeRate)}-{formatFeeRate(stats.maxFeeRate)}
                </div>
              </div>
              <div className="flex flex-col items-center">
                <div className="text-[9px] font-medium text-white/90 sm:text-[10px]">
                  {formatBytes(stats.totalSize)}
                </div>
                <div className="text-[9px] font-medium text-white/70">
                  {block.transactionsCount} txs · {block.proposalsCount ?? 0} props
                </div>
                <div className="text-[8px] text-white/50">{formatTimeAgo(block.timestamp)}</div>
              </div>
            </div>
          ) : (
            <div className="flex h-full w-full flex-col items-center justify-center gap-1">
              <div className="relative h-10 w-full overflow-visible rounded-md border border-white/20 bg-black/25">
                <TxBubbleLayer bubbles={bubbles} />
              </div>
              <div className="text-xl font-bold text-white drop-shadow-sm sm:text-2xl">
                {block.transactionsCount}
              </div>
              <div className="text-[10px] font-medium text-white/80">transactions</div>
              <div className="text-[9px] text-white/50">{formatTimeAgo(block.timestamp)}</div>
            </div>
          )}
        </Block2D>
      </div>
      {minedBlockTooltip ? createPortal(minedBlockTooltip, document.body) : null}
      {large && (
        <div className="mt-1.5 text-center text-[10px] sm:text-[11px]">
          {isLoading ? (
            <>
              <div className="mx-auto h-3 w-20 animate-pulse rounded bg-white/20" />
              <div className="mx-auto mt-1 h-2 w-24 animate-pulse rounded bg-white/20" />
            </>
          ) : stats ? (
            <>
              <div className="font-mono tabular-nums text-white/90">
                ~{formatFeeRate(stats.avgFeeRate)} sh/B
              </div>
              <div className="font-mono tabular-nums text-white/55">
                {formatFeeRate(stats.minFeeRate)}-{formatFeeRate(stats.maxFeeRate)}
              </div>
              <div className="text-white/65">
                {formatBytes(stats.totalSize)} · {block.transactionsCount} txs
              </div>
              <div className="text-white/45">{formatTimeAgo(block.timestamp)} ago</div>
            </>
          ) : (
            <>
              <div className="font-mono tabular-nums text-white/90">
                {block.transactionsCount} txs
              </div>
              <div className="text-white/45">{formatTimeAgo(block.timestamp)} ago</div>
            </>
          )}
        </div>
      )}
    </Link>
  );
}

function ChainArrow({ isPending = false }: { isPending?: boolean }) {
  return (
    <div className="flex items-center justify-center px-1 sm:px-2">
      <div
        className={cn(
          'flex items-center gap-0.5',
          isPending ? 'text-terminal-dim/70' : 'text-terminal-green/70'
        )}
      >
        <div
          className={cn(
            'h-0.5 w-3 rounded-full',
            isPending ? 'bg-terminal-dim/50' : 'bg-terminal-green/50'
          )}
        />
        <svg width="8" height="12" viewBox="0 0 8 12" fill="none" className="opacity-60">
          <path
            d="M1 1L6 6L1 11"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </div>
    </div>
  );
}

function MempoolDivider({ large = false }: { large?: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center px-3 sm:px-4">
      <div
        className={cn(
          'flex items-center',
          large ? 'h-[98px] sm:h-[114px]' : 'h-[80px] sm:h-[96px]'
        )}
      >
        <div className="flex items-center gap-2">
          <div className="h-px w-4 bg-gradient-to-r from-transparent to-slate-500" />
          <div className="border-terminal-dark/50 text-terminal-green/80 rounded-md border bg-slate-900/70 px-2 py-1 text-[10px] font-medium tracking-wider">
            MINED
          </div>
          <div className="h-px w-4 bg-gradient-to-l from-transparent to-slate-500" />
        </div>
      </div>
    </div>
  );
}

export function MempoolBlocks({
  latestBlocks,
  chrome = 'card',
  showHeader = true,
  showTxnLens = false,
  legendMode = 'row',
}: MempoolBlocksProps) {
  const queryClient = useQueryClient();
  const { data: mempoolData, isLoading: mempoolLoading } = useQuery({
    queryKey: ['mempool-blocks'],
    queryFn: () => api.getMempoolBlocks(),
    refetchInterval: 5000,
  });

  const { data: blocksData } = useQuery({
    queryKey: ['latest-blocks'],
    queryFn: () => api.getBlocks({ limit: 10 }),
    initialData: latestBlocks?.length
      ? {
          data: latestBlocks,
          total: latestBlocks.length,
          limit: 10,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    refetchInterval: 10000,
  });

  const pendingBlocks = mempoolData?.pendingBlocks ?? [];
  const minedBlocks = blocksData?.data ?? [];
  const displayedMinedBlocks = minedBlocks.slice(0, showTxnLens ? 10 : 3);

  const feeStatsQueries = useQueries({
    queries: displayedMinedBlocks.map((block) => ({
      queryKey: ['block-fee-stats', block.number],
      queryFn: () => api.getBlockFeeStats(block.number),
      staleTime: Infinity,
      gcTime: 1000 * 60 * 10,
    })),
  });

  const feeStatsMap = new Map<number, BlockFeeStats | 'loading'>();
  displayedMinedBlocks.forEach((block, index) => {
    const result = feeStatsQueries[index];
    if (result?.data) {
      feeStatsMap.set(block.number, result.data);
    } else if (result?.isLoading) {
      feeStatsMap.set(block.number, 'loading');
    }
  });

  const { data: mempoolTransactions } = useQuery({
    queryKey: ['mempool-blocks-lens-mempool-transactions'],
    queryFn: () => api.getMempoolTransactions(),
    enabled: showTxnLens,
    refetchInterval: 10000,
  });

  const { data: pendingProposalsData } = useQuery({
    queryKey: ['mempool-blocks-lens-pending-proposals'],
    queryFn: () => api.getPendingProposals(),
    enabled: showTxnLens,
    refetchInterval: 10000,
  });

  const committedTxQueries = useQueries({
    queries: showTxnLens
      ? displayedMinedBlocks.map((block) => ({
          queryKey: ['mempool-blocks-lens-block-transactions', block.number],
          queryFn: () => api.getTransactions({ blockNumber: block.number, limit: 80 }),
          staleTime: 5000,
          gcTime: 1000 * 60 * 5,
        }))
      : [],
  });

  const pendingProposals = useMemo(
    () => pendingProposalsData?.proposals ?? [],
    [pendingProposalsData?.proposals]
  );
  const proposalBuckets = useMemo(() => splitProposalBuckets(pendingProposals), [pendingProposals]);

  const proposalLookupHashes = useMemo(
    () =>
      showTxnLens
        ? Array.from(
            new Set(
              [...proposalBuckets.nextBlockProposals, ...proposalBuckets.backlogProposals]
                .map((proposal) => proposal.fullTxHash)
                .filter((hash): hash is string => Boolean(hash))
            )
          ).slice(0, MAX_LENS_STAGE_ITEMS * 3)
        : [],
    [proposalBuckets.backlogProposals, proposalBuckets.nextBlockProposals, showTxnLens]
  );

  const proposalTxQueries = useQueries({
    queries: showTxnLens
      ? proposalLookupHashes.map((hash) => ({
          queryKey: ['mempool-blocks-lens-proposal-transaction', hash],
          queryFn: () => api.getTransaction(hash),
          staleTime: 5000,
          gcTime: 1000 * 60 * 5,
        }))
      : [],
  });

  const proposalTxByHash = useMemo(() => {
    const mapped = new Map<string, Transaction>();
    if (!showTxnLens) return mapped;

    proposalLookupHashes.forEach((hash, index) => {
      const tx = proposalTxQueries[index]?.data;
      if (tx) mapped.set(hash, tx);
    });

    return mapped;
  }, [proposalLookupHashes, proposalTxQueries, showTxnLens]);

  const proposalHashSet = useMemo(
    () =>
      new Set(
        pendingProposals
          .map((proposal) => proposal.fullTxHash)
          .filter((hash): hash is string => Boolean(hash))
      ),
    [pendingProposals]
  );

  const mempoolOnlyTransactions = useMemo(
    () =>
      showTxnLens
        ? (mempoolTransactions ?? []).filter(
            (tx) => tx.status !== 'proposed' && !proposalHashSet.has(tx.txHash)
          )
        : [],
    [mempoolTransactions, proposalHashSet, showTxnLens]
  );

  const mempoolAllLensItems = useMemo(
    () => (showTxnLens ? mempoolOnlyTransactions.map(mempoolTxToLensItem) : []),
    [mempoolOnlyTransactions, showTxnLens]
  );

  const nextProposalAllLensItems = useMemo(
    () =>
      showTxnLens
        ? proposalBuckets.nextBlockProposals.map((proposal) =>
            proposalToLensItem(
              proposal,
              proposal.fullTxHash ? proposalTxByHash.get(proposal.fullTxHash) : undefined
            )
          )
        : [],
    [proposalBuckets.nextBlockProposals, proposalTxByHash, showTxnLens]
  );

  const backlogProposalAllLensItems = useMemo(
    () =>
      showTxnLens
        ? proposalBuckets.backlogProposals.map((proposal) =>
            proposalToLensItem(
              proposal,
              proposal.fullTxHash ? proposalTxByHash.get(proposal.fullTxHash) : undefined
            )
          )
        : [],
    [proposalBuckets.backlogProposals, proposalTxByHash, showTxnLens]
  );

  const mempoolLensItems = useMemo(
    () => sampleItems(mempoolAllLensItems, MAX_LENS_STAGE_ITEMS),
    [mempoolAllLensItems]
  );
  const nextProposalLensItems = useMemo(
    () => sampleItems(nextProposalAllLensItems, MAX_LENS_STAGE_ITEMS),
    [nextProposalAllLensItems]
  );
  const backlogProposalLensItems = useMemo(
    () => sampleItems(backlogProposalAllLensItems, MAX_LENS_STAGE_ITEMS),
    [backlogProposalAllLensItems]
  );

  const committedLensItems = useMemo(() => {
    if (!showTxnLens) return [];

    const committedTxs = committedTxQueries.flatMap((query) => query.data?.data ?? []);
    return sampleItems(
      committedTxs.map((tx) => committedTxToLensItem(tx)),
      MAX_LENS_STAGE_ITEMS
    );
  }, [committedTxQueries, showTxnLens]);

  const lensItems = useMemo(
    () => [
      ...mempoolLensItems,
      ...nextProposalLensItems,
      ...backlogProposalLensItems,
      ...committedLensItems,
    ],
    [backlogProposalLensItems, committedLensItems, mempoolLensItems, nextProposalLensItems]
  );

  const lensDomain = useMemo(() => buildRectDomain(lensItems), [lensItems]);

  const legacyPendingBlocks = pendingBlocks.slice(0, 4).reverse();
  const nextPendingBlock = pendingBlocks[0];
  const mempoolStageBlock = useMemo(
    () => syntheticBlock(2, mempoolAllLensItems),
    [mempoolAllLensItems]
  );
  const proposalsStageBlock = useMemo(
    () => syntheticBlock(1, backlogProposalAllLensItems),
    [backlogProposalAllLensItems]
  );
  const nextStageBlock = useMemo(
    () => syntheticBlock(0, nextProposalAllLensItems),
    [nextProposalAllLensItems]
  );

  const committedLensByBlock = useMemo(() => {
    const mapped = new Map<number, LensTxItem[]>();
    if (!showTxnLens) return mapped;

    displayedMinedBlocks.forEach((block, index) => {
      const queryData = committedTxQueries[index]?.data?.data ?? [];
      const sampled = sampleItems(
        queryData.map((tx) => committedTxToLensItem(tx)),
        14
      );
      mapped.set(block.number, sampled);
    });

    return mapped;
  }, [committedTxQueries, displayedMinedBlocks, showTxnLens]);

  const mempoolBubbles = useMemo(() => {
    if (!showTxnLens) return [];

    const bubbles = sampleItems(mempoolLensItems, 12)
      .map((item) => {
        const metrics = mapTxToRectMetrics(item, lensDomain);
        return toTxBubble(item, metrics, `mempool-${item.id}`);
      })
      .sort((a, b) => a.widthPx * a.heightPx - b.widthPx * b.heightPx);

    return resolveBubbleOverlaps(bubbles);
  }, [lensDomain, mempoolLensItems, showTxnLens]);

  const proposalBacklogBubbles = useMemo(() => {
    if (!showTxnLens) return [];

    const bubbles = sampleItems(backlogProposalLensItems, 12)
      .map((item) => {
        const metrics = mapTxToRectMetrics(item, lensDomain);
        return toTxBubble(item, metrics, `proposal-backlog-${item.id}`);
      })
      .sort((a, b) => a.widthPx * a.heightPx - b.widthPx * b.heightPx);

    return resolveBubbleOverlaps(bubbles);
  }, [backlogProposalLensItems, lensDomain, showTxnLens]);

  const nextProposalBubbles = useMemo(() => {
    if (!showTxnLens) return [];

    const bubbles = sampleItems(nextProposalLensItems, 12)
      .map((item) => {
        const metrics = mapTxToRectMetrics(item, lensDomain);
        return toTxBubble(item, metrics, `proposal-next-${item.id}`);
      })
      .sort((a, b) => a.widthPx * a.heightPx - b.widthPx * b.heightPx);

    return resolveBubbleOverlaps(bubbles);
  }, [lensDomain, nextProposalLensItems, showTxnLens]);

  const committedBubblesByBlock = useMemo(() => {
    const mapped = new Map<number, TxBubble[]>();
    if (!showTxnLens) return mapped;

    displayedMinedBlocks.forEach((block) => {
      const bubbles = resolveBubbleOverlaps(
        (committedLensByBlock.get(block.number) ?? [])
          .map((item) => {
            const metrics = mapTxToRectMetrics(item, lensDomain);
            return toTxBubble(item, metrics, `${block.number}-${item.id}`);
          })
          .sort((a, b) => a.widthPx * a.heightPx - b.widthPx * b.heightPx)
      );
      mapped.set(block.number, bubbles);
    });

    return mapped;
  }, [committedLensByBlock, displayedMinedBlocks, lensDomain, showTxnLens]);

  const totalPending =
    showTxnLens && mempoolTransactions
      ? mempoolOnlyTransactions.length
      : (mempoolData?.totalPendingCount ?? 0);
  const totalProposed =
    showTxnLens && pendingProposalsData
      ? pendingProposalsData.totalCount
      : (mempoolData?.totalProposedCount ?? 0);
  const latestCommittedCount = minedBlocks[0]?.transactionsCount ?? 0;
  const totalCommitted = latestCommittedCount > 0 ? latestCommittedCount - 1 : 0;
  const latestBlockNumber = minedBlocks[0]?.number ?? 0;
  const nextBlockNumber = latestBlockNumber + 1;
  const mempoolNodeId = `block-${nextBlockNumber + 2}`;
  const proposalsNodeId = `block-${nextBlockNumber + 1}`;
  const nextNodeId = `block-${nextBlockNumber}`;
  const blockNodeRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const previousTipRef = useRef<number | null>(null);
  const previousTipForRefreshRef = useRef<number | null>(null);
  const animationFramesRef = useRef<number[]>([]);
  const resetTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      animationFramesRef.current.forEach((frameId) => cancelAnimationFrame(frameId));
      animationFramesRef.current = [];
      if (resetTimeoutRef.current) {
        clearTimeout(resetTimeoutRef.current);
        resetTimeoutRef.current = null;
      }
    };
  }, []);

  const minedNodeIds = useMemo(
    () => displayedMinedBlocks.map((block) => `block-${block.number}`),
    [displayedMinedBlocks]
  );
  const visibleNodeIds = useMemo(
    () =>
      showTxnLens
        ? [mempoolNodeId, proposalsNodeId, nextNodeId, ...minedNodeIds]
        : [...minedNodeIds],
    [mempoolNodeId, minedNodeIds, nextNodeId, proposalsNodeId, showTxnLens]
  );

  useEffect(() => {
    if (latestBlockNumber <= 0) return;

    if (
      previousTipForRefreshRef.current !== null &&
      latestBlockNumber !== previousTipForRefreshRef.current
    ) {
      // Force-refresh txn-related card data on tip change so card internals update
      // immediately instead of waiting for interval-based polling.
      void queryClient.refetchQueries({
        queryKey: ['mempool-blocks'],
        exact: true,
        type: 'active',
      });
      void queryClient.refetchQueries({ queryKey: ['block-fee-stats'], type: 'active' });

      if (showTxnLens) {
        void queryClient.refetchQueries({
          queryKey: ['mempool-blocks-lens-mempool-transactions'],
          exact: true,
          type: 'active',
        });
        void queryClient.refetchQueries({
          queryKey: ['mempool-blocks-lens-pending-proposals'],
          exact: true,
          type: 'active',
        });
        void queryClient.refetchQueries({
          queryKey: ['mempool-blocks-lens-block-transactions'],
          type: 'active',
        });
        void queryClient.refetchQueries({
          queryKey: ['mempool-blocks-lens-proposal-transaction'],
          type: 'active',
        });
      }
    }

    previousTipForRefreshRef.current = latestBlockNumber;
  }, [latestBlockNumber, queryClient, showTxnLens]);

  function setBlockNodeRef(nodeId: string, node: HTMLDivElement | null): void {
    if (!node) {
      blockNodeRefs.current.delete(nodeId);
      return;
    }
    blockNodeRefs.current.set(nodeId, node);
  }

  useLayoutEffect(() => {
    const currentRects = new Map<string, DOMRect>();
    visibleNodeIds.forEach((nodeId) => {
      const node = blockNodeRefs.current.get(nodeId);
      if (node) {
        currentRects.set(nodeId, node.getBoundingClientRect());
      }
    });

    if (
      showTxnLens &&
      latestBlockNumber > 0 &&
      previousTipRef.current !== null &&
      latestBlockNumber !== previousTipRef.current
    ) {
      animationFramesRef.current.forEach((frameId) => cancelAnimationFrame(frameId));
      animationFramesRef.current = [];
      if (resetTimeoutRef.current) {
        clearTimeout(resetTimeoutRef.current);
        resetTimeoutRef.current = null;
      }

      const orderedRects = visibleNodeIds
        .map((nodeId) => currentRects.get(nodeId))
        .filter((rect): rect is DOMRect => Boolean(rect));
      const shiftDeltaX = computeUniformShiftDeltaX(orderedRects);

      visibleNodeIds.forEach((nodeId) => {
        const node = blockNodeRefs.current.get(nodeId);
        const nextRect = currentRects.get(nodeId);
        if (!node || !nextRect) return;

        const shouldAnimate = Math.abs(shiftDeltaX) > 0.5;
        if (!shouldAnimate) return;

        node.style.willChange = 'transform';
        node.style.transition = 'none';
        node.style.transform = `translate3d(${shiftDeltaX}px, 0, 0)`;
      });

      const frame1 = requestAnimationFrame(() => {
        const frame2 = requestAnimationFrame(() => {
          visibleNodeIds.forEach((nodeId) => {
            const node = blockNodeRefs.current.get(nodeId);
            if (!node) return;

            node.style.transition = 'transform 1180ms cubic-bezier(0.22, 0.61, 0.36, 1)';
            node.style.transform = 'translate3d(0, 0, 0)';
          });

          resetTimeoutRef.current = setTimeout(() => {
            visibleNodeIds.forEach((nodeId) => {
              const node = blockNodeRefs.current.get(nodeId);
              if (!node) return;
              node.style.transition = '';
              node.style.transform = '';
              node.style.willChange = '';
            });
            resetTimeoutRef.current = null;
          }, 1220);
        });
        animationFramesRef.current.push(frame2);
      });
      animationFramesRef.current.push(frame1);
    }

    if (latestBlockNumber > 0) {
      previousTipRef.current = latestBlockNumber;
    }
  }, [latestBlockNumber, showTxnLens, visibleNodeIds]);

  const containerClassName =
    chrome === 'flat'
      ? cn(
          'rounded-2xl bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800/80',
          showHeader ? 'p-5' : 'px-3 py-2 sm:px-4 sm:py-2.5'
        )
      : 'rounded-2xl border border-slate-700/50 bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 p-5 shadow-xl';

  if (mempoolLoading && pendingBlocks.length === 0) {
    return (
      <div className={containerClassName}>
        {showHeader && (
          <h2 className="mb-5 text-lg font-bold tracking-tight text-white sm:text-xl">
            Chain Tip Intelligence
          </h2>
        )}
        <div className="flex items-center justify-center gap-3 py-4 sm:gap-4">
          {Array.from({ length: 6 }).map((_, i) => (
            <div
              key={i}
              className={cn(
                'animate-pulse rounded-xl bg-slate-700/50',
                showTxnLens
                  ? 'h-[98px] w-[126px] sm:h-[114px] sm:w-[148px]'
                  : 'h-[80px] w-[100px] sm:h-[96px] sm:w-[116px]'
              )}
            />
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className={containerClassName}>
      {showHeader && (
        <div className="mb-5">
          <h2 className="text-lg font-bold tracking-tight text-white sm:text-xl">
            Chain Tip Intelligence
          </h2>
          <div
            data-testid="pipeline-summary-row"
            className="mt-1 flex flex-col gap-1.5 sm:flex-row sm:items-center sm:justify-between"
          >
            <p className="text-xs sm:text-sm">
              <span className="text-amber-300">Mempool ({formatCount(totalPending)})</span>
              <span className="text-slate-500"> {'->'} </span>
              <span className="text-terminal-dim">Proposals ({formatCount(totalProposed)})</span>
              <span className="text-slate-500"> {'->'} </span>
              <span className="text-terminal-green">
                New Committed ({formatCount(totalCommitted)})
              </span>
            </p>
            {legendMode === 'row' && (
              <p className="rounded-md border border-slate-700/60 bg-slate-900/70 px-2 py-1 text-[11px] text-slate-300 sm:text-right">
                w {'->'} size | h {'->'} cycles | x {'->'} fee | y {'->'} fee rate
              </p>
            )}
          </div>
        </div>
      )}

      {legendMode === 'row' && !showHeader && (
        <div className="mb-1 mt-1 flex items-center">
          <span className="rounded-md border border-slate-700/60 bg-slate-900/70 px-2 py-1 text-[11px] text-slate-300">
            w {'->'} size | h {'->'} cycles | x {'->'} fee | y {'->'} fee rate
          </span>
        </div>
      )}

      <div className={cn('relative', showHeader ? 'pb-6' : 'pb-2')}>
        <div className="overflow-x-auto overscroll-x-contain pb-1 [scrollbar-width:thin]">
          <div
            className={cn(
              'flex min-w-full items-center gap-1 sm:gap-2',
              showTxnLens ? 'w-max justify-start pl-4 pr-2 sm:pl-7 sm:pr-4' : 'justify-center'
            )}
          >
            {showTxnLens ? (
              <>
                <div
                  ref={(node) => setBlockNodeRef(mempoolNodeId, node)}
                  className="flex items-center transition-opacity duration-500"
                >
                  <PendingBlock
                    block={mempoolStageBlock}
                    topLabel="Mempool"
                    tone="mempool"
                    isNextBlock={false}
                    large
                    bubbles={mempoolBubbles}
                  />
                </div>
                <ChainArrow isPending />

                <div
                  ref={(node) => setBlockNodeRef(proposalsNodeId, node)}
                  className="flex items-center transition-opacity duration-500"
                >
                  <PendingBlock
                    block={proposalsStageBlock}
                    topLabel="Proposals"
                    tone="proposals"
                    isNextBlock={false}
                    large
                    bubbles={proposalBacklogBubbles}
                  />
                </div>
                <ChainArrow isPending />

                <div
                  ref={(node) => setBlockNodeRef(nextNodeId, node)}
                  className="flex items-center transition-opacity duration-500"
                >
                  <PendingBlock
                    block={
                      nextProposalLensItems.length > 0
                        ? nextStageBlock
                        : (nextPendingBlock ?? nextStageBlock)
                    }
                    predictedNumber={nextBlockNumber}
                    tone="next"
                    isNextBlock
                    large
                    bubbles={nextProposalBubbles}
                  />
                </div>
              </>
            ) : (
              <>
                {legacyPendingBlocks.map((block, index) => (
                  <div
                    key={`pending-${latestBlockNumber + block.index + 1}`}
                    className="flex items-center transition-opacity duration-500"
                    style={{ opacity: edgeFadeOpacity(index) }}
                  >
                    <PendingBlock
                      block={block}
                      predictedNumber={latestBlockNumber + block.index + 1}
                      isNextBlock={block.index === 0}
                      large={false}
                      bubbles={[]}
                    />
                    {index < legacyPendingBlocks.length - 1 && <ChainArrow isPending />}
                  </div>
                ))}
              </>
            )}

            {(showTxnLens || pendingBlocks.length > 0) && minedBlocks.length > 0 && (
              <MempoolDivider large={showTxnLens} />
            )}

            {displayedMinedBlocks.map((block, index) => (
              <div
                key={`mined-${block.number}`}
                ref={(node) => setBlockNodeRef(`block-${block.number}`, node)}
                className="flex items-center transition-opacity duration-500"
                style={{ opacity: edgeFadeOpacity(displayedMinedBlocks.length - 1 - index) }}
              >
                <MinedBlock
                  block={block}
                  feeStats={feeStatsMap.get(block.number)}
                  large={showTxnLens}
                  bubbles={committedBubblesByBlock.get(block.number) ?? []}
                />
                {index < displayedMinedBlocks.length - 1 && <ChainArrow />}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
