'use client';

import { useQuery } from '@tanstack/react-query';
import { api, Block } from '@/lib/api';
import { MempoolBlocks } from '@/components/mempool-blocks';

interface PipelinePreviewProps {
  initialBlocks?: Block[];
}

export function PipelinePreview({ initialBlocks = [] }: PipelinePreviewProps) {
  const { data: mempoolData } = useQuery({
    queryKey: ['mempool-blocks'],
    queryFn: () => api.getMempoolBlocks(),
    refetchInterval: 5000,
  });
  const { data: pendingProposalsData } = useQuery({
    queryKey: ['mempool-blocks-lens-pending-proposals'],
    queryFn: () => api.getPendingProposals(),
    refetchInterval: 10000,
  });
  const { data: mempoolTransactions } = useQuery({
    queryKey: ['mempool-blocks-lens-mempool-transactions'],
    queryFn: () => api.getMempoolTransactions(),
    refetchInterval: 10000,
  });

  const { data: blocksData } = useQuery({
    queryKey: ['latest-blocks'],
    queryFn: () => api.getBlocks({ limit: 10 }),
    initialData: initialBlocks.length
      ? {
          data: initialBlocks,
          total: initialBlocks.length,
          limit: 10,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    refetchInterval: 10000,
  });
  const proposalHashSet = new Set(
    (pendingProposalsData?.proposals ?? [])
      .map((proposal) => proposal.fullTxHash)
      .filter((hash): hash is string => Boolean(hash))
  );
  const mempoolOnlyCount =
    mempoolTransactions?.filter((tx) => tx.status !== 'proposed' && !proposalHashSet.has(tx.txHash))
      .length ?? null;

  const mempoolCount = mempoolOnlyCount ?? mempoolData?.totalPendingCount;
  const proposalsCount = pendingProposalsData?.totalCount ?? mempoolData?.totalProposedCount;
  const committedCount =
    blocksData?.data?.[0] && blocksData.data[0].transactionsCount > 0
      ? blocksData.data[0].transactionsCount - 1
      : 0;
  const formatCount = (value: number | undefined) =>
    typeof value === 'number' ? value.toLocaleString() : '--';

  return (
    <section className="from-base-surface via-base-surface to-base-elevated ring-base-border/70 overflow-visible rounded-2xl bg-gradient-to-br p-4 ring-1 ring-inset">
      <div className="mb-2">
        <h2 className="text-base font-semibold text-white sm:text-lg">Transaction Pipeline</h2>
        <div
          data-testid="pipeline-preview-summary-row"
          className="mt-1 flex flex-col gap-1.5 sm:flex-row sm:items-center sm:justify-between"
        >
          <p className="text-xs sm:text-sm">
            <span className="text-warning-300">Mempool ({formatCount(mempoolCount)})</span>
            <span className="text-text-muted"> {'->'} </span>
            <span className="text-emphasis-dim">Proposals ({formatCount(proposalsCount)})</span>
            <span className="text-text-muted"> {'->'} </span>
            <span className="text-emphasis">New Committed ({formatCount(committedCount)})</span>
          </p>
          <p className="border-base-border/60 bg-base-surface/70 text-text-secondary rounded-md border px-2 py-1 text-[11px] sm:text-right">
            w {'->'} size | h {'->'} cycles | x {'->'} fee | y {'->'} fee rate
          </p>
        </div>
      </div>

      <MempoolBlocks
        latestBlocks={initialBlocks}
        chrome="flat"
        showHeader={false}
        showTxnLens
        legendMode="none"
      />
    </section>
  );
}
