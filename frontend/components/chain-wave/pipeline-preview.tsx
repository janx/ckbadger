'use client';

import Link from 'next/link';
import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, Block, CursorPaginatedResponse, PendingProposal, Transaction } from '@/lib/api';
import { buildMetricDomain, mapTxToScatterPoint, type MetricDomain } from './flow-metrics';

interface PipelinePreviewProps {
  initialBlocks?: Block[];
}

type StageKey = 'mempool' | 'proposed' | 'committed';

interface PreviewTx {
  id: string;
  size: number;
  feeRate?: number | null;
  cycles?: number | null;
}

const MAX_STAGE_POINTS = 24;

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

function formatFeeRate(value: number | null | undefined): string {
  if (!value || value <= 0) return 'N/A';
  return `${value.toFixed(2)} sh/B`;
}

function formatCycles(value: number | null | undefined): string {
  if (!value || value <= 0) return 'N/A';
  return Math.round(value).toLocaleString();
}

function formatDataSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function median(values: Array<number | null | undefined>): number | null {
  const valid = values
    .filter((value): value is number => value !== null && value !== undefined && value > 0)
    .sort((a, b) => a - b);

  if (valid.length === 0) return null;
  const mid = Math.floor(valid.length / 2);
  if (valid.length % 2 === 0) return (valid[mid - 1] + valid[mid]) / 2;
  return valid[mid];
}

function stageClass(stage: StageKey): string {
  if (stage === 'mempool') return 'ring-cyan-500/35 bg-cyan-500/8';
  if (stage === 'proposed') return 'ring-amber-500/35 bg-amber-500/8';
  return 'ring-emerald-500/35 bg-emerald-500/8';
}

function bubbleColor(stage: StageKey, feeScore: number, missingFeeRate: boolean): string {
  if (missingFeeRate) return 'rgba(148, 163, 184, 0.6)';

  const hueShift = stage === 'mempool' ? 0 : stage === 'proposed' ? -28 : 24;
  const hue = 192 - feeScore * 130 + hueShift;
  const saturation = 80;
  const lightness = 42 + feeScore * 20;
  return `hsl(${hue}, ${saturation}%, ${lightness}%)`;
}

function mempoolTxToPreview(tx: {
  txHash: string;
  size: number;
  feeRate: number;
  cycles: number;
}): PreviewTx {
  return {
    id: tx.txHash,
    size: toSafePositive(tx.size, 220),
    feeRate: tx.feeRate,
    cycles: tx.cycles,
  };
}

function proposalToPreview(proposal: PendingProposal): PreviewTx {
  return {
    id: proposal.fullTxHash || proposal.proposalId,
    size: toSafePositive(proposal.size, 220),
    feeRate: proposal.feeRate,
    cycles: proposal.cycles,
  };
}

function blockTxToPreview(tx: Transaction): PreviewTx {
  const fee = parseFloat(tx.fee) || 0;
  const feeRate = tx.txSize && tx.txSize > 0 ? fee / tx.txSize : null;

  return {
    id: tx.hash,
    size: toSafePositive(tx.txSize, 220),
    feeRate,
    cycles: tx.cycles ?? null,
  };
}

function StageMiniScatter({
  title,
  subtitle,
  stage,
  total,
  items,
  domain,
}: {
  title: string;
  subtitle: string;
  stage: StageKey;
  total: number;
  items: PreviewTx[];
  domain: MetricDomain;
}) {
  const points = useMemo(
    () =>
      items
        .map((item) => ({ item, point: mapTxToScatterPoint(item, domain) }))
        .sort((a, b) => a.point.radius - b.point.radius),
    [domain, items]
  );

  const medianFeeRate = useMemo(() => median(items.map((item) => item.feeRate)), [items]);
  const medianCycles = useMemo(() => median(items.map((item) => item.cycles)), [items]);

  return (
    <article
      className={`min-w-[78%] rounded-xl p-2 ring-1 ring-inset sm:min-w-[48%] lg:min-w-0 ${stageClass(stage)}`}
    >
      <div className="mb-2 flex items-start justify-between gap-2">
        <div>
          <div className="text-[11px] uppercase tracking-widest text-slate-400">{title}</div>
          <div className="text-[11px] text-slate-500">{subtitle}</div>
        </div>
        <div className="text-right">
          <div className="text-base font-semibold text-white">{total.toLocaleString()}</div>
          <div className="text-[10px] uppercase tracking-widest text-slate-500">txns</div>
        </div>
      </div>

      <div className="relative h-24 overflow-hidden rounded-lg bg-slate-950/60 ring-1 ring-inset ring-slate-800/80 sm:h-28">
        <div className="pointer-events-none absolute inset-0 grid grid-cols-4 grid-rows-4">
          {Array.from({ length: 16 }, (_, idx) => (
            <div key={idx} className="border border-slate-800/40" />
          ))}
        </div>
        {points.length === 0 ? (
          <div className="flex h-full items-center justify-center px-3 text-center text-xs text-slate-500">
            No tx samples
          </div>
        ) : (
          <div className="absolute inset-0">
            {points.map(({ item, point }) => {
              const titleText = [
                `TX: ${item.id.slice(0, 10)}...${item.id.slice(-6)}`,
                `Size: ${item.size.toLocaleString()} B`,
                `Fee Rate: ${formatFeeRate(item.feeRate)}`,
                `Cycles: ${formatCycles(item.cycles)}`,
              ].join('\n');

              return (
                <div
                  key={item.id}
                  className="absolute rounded-full"
                  style={{
                    width: point.radius * 1.8,
                    height: point.radius * 1.8,
                    left: `calc(${point.x * 100}% - ${point.radius * 0.9}px)`,
                    top: `calc(${point.y * 100}% - ${point.radius * 0.9}px)`,
                    backgroundColor: bubbleColor(stage, point.feeScore, point.missingFeeRate),
                    border: point.missingCycles
                      ? '1px dashed rgba(148, 163, 184, 0.8)'
                      : '1px solid rgba(226, 232, 240, 0.45)',
                    opacity: point.missingCycles ? 0.58 : 0.9,
                  }}
                  title={titleText}
                />
              );
            })}
          </div>
        )}
        <div className="pointer-events-none absolute bottom-1 left-2 text-[10px] uppercase tracking-wider text-slate-500">
          low fee
        </div>
        <div className="pointer-events-none absolute bottom-1 right-2 text-[10px] uppercase tracking-wider text-slate-500">
          high fee
        </div>
        <div className="pointer-events-none absolute left-2 top-1 text-[10px] uppercase tracking-wider text-slate-500">
          high cycles
        </div>
      </div>

      <div className="mt-2 grid grid-cols-2 gap-1 text-[10px] sm:text-[11px]">
        <div className="truncate rounded-md bg-slate-950/55 px-2 py-1 text-slate-400">
          median fee:{' '}
          <span className="font-medium text-slate-200">{formatFeeRate(medianFeeRate)}</span>
        </div>
        <div className="truncate rounded-md bg-slate-950/55 px-2 py-1 text-slate-400">
          median cycles:{' '}
          <span className="font-medium text-slate-200">{formatCycles(medianCycles)}</span>
        </div>
      </div>
    </article>
  );
}

function StageConnector({ label }: { label: string }) {
  return (
    <div className="hidden items-center justify-center lg:flex">
      <div className="flex w-14 items-center gap-1 text-slate-500">
        <div className="h-px flex-1 bg-gradient-to-r from-slate-700 to-slate-500" />
        <div className="h-1.5 w-1.5 rounded-full bg-amber-400/90 shadow-[0_0_12px_rgba(251,191,36,0.45)]" />
      </div>
      <div className="ml-1 text-[10px] uppercase tracking-widest text-slate-500">{label}</div>
    </div>
  );
}

export function PipelinePreview({ initialBlocks = [] }: PipelinePreviewProps) {
  const { data: mempoolInfo } = useQuery({
    queryKey: ['pipeline-preview-mempool-info'],
    queryFn: () => api.getMempoolInfo(),
    refetchInterval: 10000,
  });

  const { data: mempoolTxs } = useQuery({
    queryKey: ['pipeline-preview-mempool-transactions'],
    queryFn: () => api.getMempoolTransactions(),
    refetchInterval: 10000,
  });

  const { data: pendingProposals } = useQuery({
    queryKey: ['pipeline-preview-pending-proposals'],
    queryFn: () => api.getPendingProposals(),
    refetchInterval: 10000,
  });

  const { data: blocksData } = useQuery<CursorPaginatedResponse<Block>>({
    queryKey: ['pipeline-preview-blocks'],
    queryFn: () => api.getBlocks({ limit: 3 }),
    initialData: initialBlocks.length
      ? {
          data: initialBlocks.slice(0, 3),
          total: initialBlocks.slice(0, 3).length,
          limit: 3,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    refetchInterval: 10000,
  });

  const recentBlocks = useMemo(() => (blocksData?.data ?? []).slice(0, 3), [blocksData]);
  const latestBlock = recentBlocks[0];

  const { data: latestBlockTxs } = useQuery<CursorPaginatedResponse<Transaction>>({
    queryKey: ['pipeline-preview-latest-block-transactions', latestBlock?.number ?? null],
    queryFn: () => api.getTransactions({ blockNumber: latestBlock!.number, limit: 40 }),
    enabled: !!latestBlock,
    refetchInterval: 10000,
  });

  const mempoolItems = useMemo(
    () =>
      sampleItems(
        (mempoolTxs ?? []).filter((tx) => tx.status === 'pending').map(mempoolTxToPreview),
        MAX_STAGE_POINTS
      ),
    [mempoolTxs]
  );

  const proposalItems = useMemo(
    () => sampleItems((pendingProposals?.proposals ?? []).map(proposalToPreview), MAX_STAGE_POINTS),
    [pendingProposals]
  );

  const committedItems = useMemo(
    () =>
      sampleItems(
        (latestBlockTxs?.data ?? []).filter((tx) => !tx.isCellbase).map(blockTxToPreview),
        MAX_STAGE_POINTS
      ),
    [latestBlockTxs]
  );

  const metricDomain = useMemo(
    () => buildMetricDomain([...mempoolItems, ...proposalItems, ...committedItems]),
    [committedItems, mempoolItems, proposalItems]
  );

  return (
    <section className="overflow-hidden rounded-2xl bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 p-4 ring-1 ring-inset ring-slate-700/70">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-white sm:text-lg">Pipeline Snapshot</h2>
          <p className="text-xs text-slate-400">
            Compact tri-metric flow for mempool, proposal, and commit stages.
          </p>
        </div>
        <Link
          href="/pipeline"
          className="bg-terminal-green/10 text-terminal-green ring-terminal-green/35 hover:bg-terminal-green/20 rounded-lg px-3 py-1.5 text-xs font-medium ring-1 ring-inset transition-colors"
        >
          View full pipeline
        </Link>
      </div>

      <div className="mb-2 rounded-lg bg-slate-950/50 px-3 py-2 ring-1 ring-inset ring-slate-800/80">
        <div className="grid grid-cols-[1fr_auto_1fr_auto_1fr] items-center gap-2 text-[10px] uppercase tracking-widest text-slate-500">
          <div className="truncate text-cyan-300/90">Mempool</div>
          <div className="text-center">→</div>
          <div className="truncate text-amber-300/90">Proposed</div>
          <div className="text-center">→</div>
          <div className="truncate text-emerald-300/90">Committed</div>
        </div>
      </div>

      <div className="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1 lg:mx-0 lg:grid lg:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto_minmax(0,1fr)] lg:overflow-visible lg:px-0">
        <StageMiniScatter
          title="Mempool"
          subtitle="awaiting proposal"
          stage="mempool"
          total={mempoolInfo?.pendingCount ?? 0}
          items={mempoolItems}
          domain={metricDomain}
        />
        <StageConnector label="propose" />
        <StageMiniScatter
          title="Proposed"
          subtitle="queued for pack"
          stage="proposed"
          total={mempoolInfo?.proposedCount ?? 0}
          items={proposalItems}
          domain={metricDomain}
        />
        <StageConnector label="pack" />
        <StageMiniScatter
          title="Committed"
          subtitle={`latest block #${(latestBlock?.number ?? 0).toLocaleString()}`}
          stage="committed"
          total={latestBlock?.transactionsCount ?? 0}
          items={committedItems}
          domain={metricDomain}
        />
      </div>

      <div className="mt-2.5 flex flex-wrap items-center gap-2 text-[11px] text-slate-400">
        <span className="rounded-md bg-slate-950/60 px-2 py-1 ring-1 ring-inset ring-slate-800/80">
          Bubble size = txn size
        </span>
        <span className="rounded-md bg-slate-950/60 px-2 py-1 ring-1 ring-inset ring-slate-800/80">
          X = fee rate
        </span>
        <span className="rounded-md bg-slate-950/60 px-2 py-1 ring-1 ring-inset ring-slate-800/80">
          Y = cycles
        </span>
        <span className="rounded-md bg-slate-950/60 px-2 py-1 ring-1 ring-inset ring-slate-800/80">
          Mempool size: {formatDataSize(mempoolInfo?.totalSize ?? 0)}
        </span>
      </div>
    </section>
  );
}
