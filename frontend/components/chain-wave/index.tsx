'use client';

import { useMemo } from 'react';
import Link from '@/components/ui/link';
import { useQueries, useQuery } from '@tanstack/react-query';
import {
  api,
  Block,
  CursorPaginatedResponse,
  MempoolTransaction,
  PendingProposal,
  Transaction,
} from '@/lib/api';
import { buildMetricDomain, mapTxToScatterPoint } from './flow-metrics';

interface ChainWaveProps {
  initialBlocks?: Block[];
  showHeader?: boolean;
  chrome?: 'card' | 'flat';
}

type FlowStage = 'mempool' | 'proposed' | 'committed';

interface FlowTxItem {
  id: string;
  size: number;
  fee?: number | null;
  feeRate?: number | null;
  cycles?: number | null;
  category: 'normal' | 'cellbase';
}

interface CommittedBlock {
  block: Block;
  items: FlowTxItem[];
  totalCount: number;
}

const MAX_STAGE_ITEMS = 180;
const MAX_BLOCK_ITEMS = 80;
const MAX_COMMITTED_BLOCKS = 4;

function toSafePositive(value: number | null | undefined, fallback: number): number {
  if (typeof value !== 'number' || Number.isNaN(value) || !Number.isFinite(value) || value <= 0) {
    return fallback;
  }

  return value;
}

function formatFeeRate(value: number | null | undefined): string {
  if (!value || value <= 0) return 'N/A';
  return `${value.toFixed(2)} sh/B`;
}

function formatCycles(value: number | null | undefined): string {
  if (!value || value <= 0) return 'N/A';
  return Math.round(value).toLocaleString();
}

function median(values: Array<number | null | undefined>): number | null {
  const valid = values
    .filter((value): value is number => value !== null && value !== undefined && value > 0)
    .sort((a, b) => a - b);

  if (valid.length === 0) return null;

  const mid = Math.floor(valid.length / 2);
  if (valid.length % 2 === 0) {
    return (valid[mid - 1] + valid[mid]) / 2;
  }

  return valid[mid];
}

function stagePillClass(stage: FlowStage): string {
  if (stage === 'mempool') return 'border-warning-500/30 bg-warning/10 text-warning';
  if (stage === 'proposed') return 'border-green-500/30 bg-green-500/10 text-green-200';
  return 'border-emphasis/40 bg-emphasis/10 text-emphasis';
}

function bubbleColor(feeScore: number, stage: FlowStage, missing: boolean): string {
  if (missing) return 'rgba(148, 163, 184, 0.6)';

  const alpha = 0.4 + feeScore * 0.4;
  if (stage === 'mempool') return `rgba(255, 176, 0, ${alpha})`;
  if (stage === 'proposed') return `rgba(0, 204, 51, ${alpha})`;
  return `rgba(140, 224, 10, ${alpha})`;
}

function mempoolTxToItem(tx: MempoolTransaction): FlowTxItem {
  return {
    id: tx.txHash,
    size: toSafePositive(tx.size, 200),
    fee: tx.fee,
    feeRate: tx.feeRate,
    cycles: tx.cycles,
    category: 'normal',
  };
}

function proposalToItem(proposal: PendingProposal): FlowTxItem {
  return {
    id: proposal.fullTxHash || proposal.proposalId,
    size: toSafePositive(proposal.size, 200),
    fee: proposal.fee,
    feeRate: proposal.feeRate,
    cycles: proposal.cycles,
    category: 'normal',
  };
}

function blockTxToItem(tx: Transaction): FlowTxItem {
  const feeNum = parseFloat(tx.fee) || 0;
  const feeRate = tx.txSize && tx.txSize > 0 ? feeNum / tx.txSize : null;

  return {
    id: tx.hash,
    size: toSafePositive(tx.txSize, 200),
    fee: feeNum,
    feeRate,
    cycles: tx.cycles ?? null,
    category: tx.isCellbase ? 'cellbase' : 'normal',
  };
}

function StageFlowPill({
  title,
  subtitle,
  value,
  stage,
}: {
  title: string;
  subtitle: string;
  value: number;
  stage: FlowStage;
}) {
  return (
    <div className={`rounded-xl border px-3 py-2 ${stagePillClass(stage)}`}>
      <div className="text-text-secondary/70 text-[11px] uppercase tracking-widest">{title}</div>
      <div className="mt-1 flex items-center justify-between gap-3">
        <div className="text-lg font-semibold text-white">{value.toLocaleString()}</div>
        <div className="text-text-secondary/70 text-[11px]">{subtitle}</div>
      </div>
    </div>
  );
}

function StageConnector({ label }: { label: string }) {
  return (
    <div className="hidden items-center gap-1 px-2 lg:flex">
      <div className="bg-base-border h-px w-6" />
      <div className="relative">
        <div className="bg-emphasis/80 h-2 w-2 rounded-full" />
        <div className="bg-emphasis/35 absolute inset-0 animate-ping rounded-full" />
      </div>
      <div className="text-text-muted text-[10px] uppercase tracking-widest">{label}</div>
      <div className="bg-base-border h-px w-6" />
    </div>
  );
}

function TxMetricScatter({
  items,
  stage,
  emptyText,
  compact = false,
}: {
  items: FlowTxItem[];
  stage: FlowStage;
  emptyText: string;
  compact?: boolean;
}) {
  const domain = useMemo(() => buildMetricDomain(items), [items]);
  const points = useMemo(
    () =>
      items
        .map((item) => ({ item, point: mapTxToScatterPoint(item, domain) }))
        .sort((a, b) => a.point.radius - b.point.radius),
    [domain, items]
  );

  return (
    <div
      className={`border-base-border/60 bg-base-bg/50 relative overflow-hidden rounded-xl border ${
        compact ? 'h-28' : 'h-52'
      }`}
    >
      <div className="pointer-events-none absolute inset-0 grid grid-cols-4 grid-rows-4">
        {Array.from({ length: 16 }, (_, idx) => (
          <div key={idx} className="border-base-border/60 border" />
        ))}
      </div>

      {points.length === 0 ? (
        <div className="text-text-muted flex h-full items-center justify-center px-4 text-center text-xs">
          {emptyText}
        </div>
      ) : (
        <div className="absolute inset-0">
          {points.map(({ item, point }) => {
            const title = [
              `TX: ${item.id.slice(0, 10)}...${item.id.slice(-6)}`,
              `Size: ${item.size.toLocaleString()} B`,
              `Fee Rate: ${formatFeeRate(item.feeRate)}`,
              `Cycles: ${formatCycles(item.cycles)}`,
            ].join('\n');

            return (
              <div
                key={item.id}
                className={`absolute rounded-full transition-transform duration-150 hover:scale-110 ${
                  item.category === 'cellbase' ? 'ring-1 ring-emerald-300/90' : ''
                }`}
                style={{
                  width: point.radius * 2,
                  height: point.radius * 2,
                  left: `calc(${point.x * 100}% - ${point.radius}px)`,
                  top: `calc(${point.y * 100}% - ${point.radius}px)`,
                  backgroundColor: bubbleColor(point.feeScore, stage, point.missingFeeRate),
                  border: point.missingCycles
                    ? '1px dashed rgba(148, 163, 184, 0.9)'
                    : '1px solid rgba(226, 232, 240, 0.5)',
                  opacity: point.missingCycles ? 0.55 : 0.86,
                }}
                title={title}
              />
            );
          })}
        </div>
      )}

      {!compact && (
        <>
          <div className="text-text-muted absolute bottom-1 left-2 text-[10px] uppercase tracking-widest">
            Low fee rate
          </div>
          <div className="text-text-muted absolute bottom-1 right-2 text-[10px] uppercase tracking-widest">
            High fee rate
          </div>
          <div className="text-text-muted absolute left-1 top-2 -rotate-90 text-[10px] uppercase tracking-widest">
            High cycles
          </div>
          <div className="text-text-muted absolute bottom-2 left-1 -rotate-90 text-[10px] uppercase tracking-widest">
            Low cycles
          </div>
        </>
      )}
    </div>
  );
}

function StageScatterCard({
  title,
  subtitle,
  stage,
  items,
  totalCount,
  emptyText,
}: {
  title: string;
  subtitle: string;
  stage: FlowStage;
  items: FlowTxItem[];
  totalCount: number;
  emptyText: string;
}) {
  const medianFeeRate = useMemo(() => median(items.map((item) => item.feeRate)), [items]);
  const medianCycles = useMemo(() => median(items.map((item) => item.cycles)), [items]);

  return (
    <div className="border-base-border/50 bg-base-surface/60 rounded-2xl border p-4">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-white sm:text-base">{title}</h3>
          <div className="text-text-muted text-xs">{subtitle}</div>
        </div>
        <div className="text-right">
          <div className="text-lg font-bold text-white">{totalCount.toLocaleString()}</div>
          <div className="text-text-muted text-[11px] uppercase tracking-widest">txns</div>
        </div>
      </div>

      <div className="mb-3 grid grid-cols-2 gap-2 text-xs">
        <div className="border-base-border/50 bg-base-bg/60 rounded-lg border px-2 py-1.5">
          <div className="text-text-muted">Median fee rate</div>
          <div className="text-text-primary font-medium">{formatFeeRate(medianFeeRate)}</div>
        </div>
        <div className="border-base-border/50 bg-base-bg/60 rounded-lg border px-2 py-1.5">
          <div className="text-text-muted">Median cycles</div>
          <div className="text-text-primary font-medium">{formatCycles(medianCycles)}</div>
        </div>
      </div>

      <TxMetricScatter items={items} stage={stage} emptyText={emptyText} />
    </div>
  );
}

function CommittedBlocksStrip({ blocks }: { blocks: CommittedBlock[] }) {
  if (blocks.length === 0) {
    return (
      <div className="border-base-border/50 bg-base-surface/60 text-text-muted rounded-2xl border p-4 text-sm">
        No committed blocks yet
      </div>
    );
  }

  return (
    <div className="border-base-border/50 bg-base-surface/60 rounded-2xl border p-4">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold text-white sm:text-base">Recent Committed Blocks</h3>
          <div className="text-text-muted text-xs">New blocks stream in as txns get packed</div>
        </div>
        <div className="border-emphasis/40 bg-emphasis/10 text-emphasis rounded-lg border px-2 py-1 text-xs">
          head #{blocks[0].block.number.toLocaleString()}
        </div>
      </div>

      <div className="grid gap-3 md:grid-cols-2">
        {blocks.map((entry, index) => (
          <Link
            key={entry.block.number}
            href={`/blocks/${entry.block.number}`}
            className="hover:border-emphasis/50 border-base-border/60 bg-base-bg/60 block rounded-xl border p-3 transition-colors"
          >
            <div className="mb-2 flex items-center justify-between text-xs">
              <div className="text-text-primary font-medium">
                #{entry.block.number.toLocaleString()}
              </div>
              {index === 0 ? (
                <span className="border-emphasis/40 bg-emphasis/20 text-emphasis rounded-md border px-1.5 py-0.5 text-[10px] uppercase tracking-widest">
                  New
                </span>
              ) : (
                <span className="text-text-muted">{entry.totalCount.toLocaleString()} txns</span>
              )}
            </div>
            <TxMetricScatter
              items={entry.items}
              stage="committed"
              emptyText="No txns in this block"
              compact
            />
          </Link>
        ))}
      </div>
    </div>
  );
}

function TriMetricLegend() {
  return (
    <div className="border-base-border/60 bg-base-bg/40 mt-4 rounded-xl border p-3">
      <div className="text-text-muted text-xs uppercase tracking-widest">Tri-metric encoding</div>
      <div className="text-text-secondary mt-2 grid gap-2 text-xs sm:grid-cols-3">
        <div className="border-base-border/50 bg-base-surface/60 rounded-lg border px-2 py-1.5">
          Bubble size = txn size (bytes)
        </div>
        <div className="border-base-border/50 bg-base-surface/60 rounded-lg border px-2 py-1.5">
          X-axis = fee rate (shannons / byte)
        </div>
        <div className="border-base-border/50 bg-base-surface/60 rounded-lg border px-2 py-1.5">
          Y-axis = cycles (higher at top)
        </div>
      </div>
      <div className="text-text-muted mt-2 text-[11px]">
        Dashed bubble means cycles data is not available yet; green ring marks cellbase.
      </div>
    </div>
  );
}

export function ChainWave({ initialBlocks, showHeader = true, chrome = 'card' }: ChainWaveProps) {
  const { data: mempoolTxs } = useQuery({
    queryKey: ['mempool-transactions'],
    queryFn: () => api.getMempoolTransactions(),
    refetchInterval: 5000,
  });

  const { data: pendingProposalsData } = useQuery({
    queryKey: ['pending-proposals'],
    queryFn: () => api.getPendingProposals(),
    refetchInterval: 5000,
  });

  const { data: blocksData } = useQuery<CursorPaginatedResponse<Block>>({
    queryKey: ['chain-wave-block-stream'],
    queryFn: () => api.getBlocks({ limit: MAX_COMMITTED_BLOCKS }),
    initialData: initialBlocks?.length
      ? {
          data: initialBlocks.slice(0, MAX_COMMITTED_BLOCKS),
          total: initialBlocks.slice(0, MAX_COMMITTED_BLOCKS).length,
          limit: MAX_COMMITTED_BLOCKS,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    refetchInterval: 10000,
  });

  const committedBlocks = useMemo(
    () => (blocksData?.data ?? []).slice(0, MAX_COMMITTED_BLOCKS),
    [blocksData]
  );

  const blockTxQueries = useQueries({
    queries: committedBlocks.map((block) => ({
      queryKey: ['chain-wave-block-transactions', block.number],
      queryFn: () => api.getTransactions({ blockNumber: block.number, limit: MAX_BLOCK_ITEMS }),
      refetchInterval: 10000,
    })),
  });

  const pendingItems = useMemo(() => {
    if (!mempoolTxs) return [];
    return mempoolTxs
      .filter((tx) => tx.status === 'pending')
      .map(mempoolTxToItem)
      .slice(0, MAX_STAGE_ITEMS);
  }, [mempoolTxs]);

  const proposalItems = useMemo(() => {
    const proposals = pendingProposalsData?.proposals ?? [];
    return proposals.map(proposalToItem).slice(0, MAX_STAGE_ITEMS);
  }, [pendingProposalsData]);

  const committedItemsByBlock = useMemo<CommittedBlock[]>(
    () =>
      committedBlocks.map((block, idx) => {
        const query = blockTxQueries[idx];
        const txs = query?.data?.data ?? [];
        const items = txs.map(blockTxToItem).slice(0, MAX_BLOCK_ITEMS);
        const totalCount = query?.data?.total ?? block.transactionsCount;
        return { block, items, totalCount };
      }),
    [blockTxQueries, committedBlocks]
  );

  const committedPreviewItems = useMemo(
    () => committedItemsByBlock[0]?.items ?? [],
    [committedItemsByBlock]
  );

  const containerClassName =
    chrome === 'flat'
      ? 'rounded-2xl bg-gradient-to-br from-base-surface via-base-surface to-base-elevated/80 p-4 sm:p-6'
      : 'rounded-2xl border border-base-border/50 bg-gradient-to-br from-base-surface via-base-surface to-base-elevated p-4 shadow-xl sm:p-6';

  return (
    <div className={containerClassName}>
      {showHeader && (
        <div className="mb-4">
          <h2 className="text-lg font-bold tracking-tight text-white sm:text-xl">
            Transaction Flow Pipeline
          </h2>
          <p className="text-text-muted mt-1 text-sm">
            Mempool txns move through proposal queue and are continuously committed into new blocks.
          </p>
        </div>
      )}

      <div className="mb-4 flex flex-col gap-2 lg:flex-row lg:items-center">
        <StageFlowPill
          title="Mempool"
          subtitle="pending"
          value={pendingItems.length}
          stage="mempool"
        />
        <StageConnector label="propose" />
        <StageFlowPill
          title="Proposed"
          subtitle="waiting commit"
          value={proposalItems.length}
          stage="proposed"
        />
        <StageConnector label="pack" />
        <StageFlowPill
          title="Committed"
          subtitle="latest block txns"
          value={committedPreviewItems.length}
          stage="committed"
        />
      </div>

      <div className="space-y-4">
        <div className="grid gap-4 md:grid-cols-2">
          <StageScatterCard
            title="Mempool"
            subtitle="Candidate txns waiting for proposal"
            stage="mempool"
            items={pendingItems}
            totalCount={pendingItems.length}
            emptyText="No pending transactions"
          />

          <StageScatterCard
            title="Proposed Pool"
            subtitle="Proposed txns waiting for commitment"
            stage="proposed"
            items={proposalItems}
            totalCount={proposalItems.length}
            emptyText="No proposed transactions"
          />
        </div>

        <CommittedBlocksStrip blocks={committedItemsByBlock} />
      </div>

      <TriMetricLegend />
    </div>
  );
}
