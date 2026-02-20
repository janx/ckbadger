'use client';

import Link from 'next/link';
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
      <div className="mb-2 flex items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-white sm:text-lg">Transaction Pipeline</h2>
          <p className="mt-1 text-xs sm:text-sm">
            <span className="text-cyan-300">Mempool ({formatCount(mempoolCount)})</span>
            <span className="text-slate-500"> {'->'} </span>
            <span className="text-emerald-300">Proposals ({formatCount(proposalsCount)})</span>
            <span className="text-slate-500"> {'->'} </span>
            <span className="text-violet-300">New Committed ({formatCount(committedCount)})</span>
          </p>
        </div>
        <div className="flex flex-col items-end gap-1">
          <Link
            href="/pipeline"
            className="bg-terminal-green/10 text-terminal-green ring-terminal-green/35 hover:bg-terminal-green/20 rounded-lg px-3 py-1.5 text-xs font-medium ring-1 ring-inset transition-colors"
          >
            View full pipeline
          </Link>
          <p className="text-right text-[10px] text-slate-400 sm:text-[11px]">
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
