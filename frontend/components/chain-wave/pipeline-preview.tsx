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
    <section className="overflow-visible rounded-2xl bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 p-4 ring-1 ring-inset ring-slate-700/70">
      <div className="mb-2">
        <h2 className="text-base font-semibold text-white sm:text-lg">Transaction Pipeline</h2>
        <div
          data-testid="pipeline-preview-summary-row"
          className="mt-1 flex flex-col gap-1.5 sm:flex-row sm:items-center sm:justify-between"
        >
          <p className="text-xs sm:text-sm">
            <span className="text-amber-300">Mempool ({formatCount(mempoolCount)})</span>
            <span className="text-slate-500"> {'->'} </span>
            <span className="text-terminal-dim">Proposals ({formatCount(proposalsCount)})</span>
            <span className="text-slate-500"> {'->'} </span>
            <span className="text-terminal-green">
              New Committed ({formatCount(committedCount)})
            </span>
          </p>
          <p className="rounded-md border border-slate-700/60 bg-slate-900/70 px-2 py-1 text-[11px] text-slate-300 sm:text-right">
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
