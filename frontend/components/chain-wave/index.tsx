'use client';

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { PackedContainer, TxItem, TxCategory } from './packed-container';

function FlowArrow() {
  return (
    <div className="flex flex-col items-center justify-center px-1 sm:px-2">
      <div className="flex items-center text-slate-500">
        <div className="hidden h-0.5 w-3 bg-gradient-to-r from-slate-600 to-slate-500 sm:block sm:w-4" />
        <svg width="10" height="14" viewBox="0 0 10 14" fill="none" className="text-slate-500">
          <path
            d="M2 2L7 7L2 12"
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

export function ChainWave() {
  const { data: summary } = useQuery({
    queryKey: ['mempool-summary'],
    queryFn: () => api.getMempoolSummary(),
    refetchInterval: 5000,
    staleTime: 3000,
  });

  const pendingItems = useMemo((): TxItem[] => {
    if (!summary?.pending) return [];
    return summary.pending
      .filter((tx) => tx.status === 'pending')
      .map((tx) => ({
        id: tx.txHash,
        size: tx.size,
        fee: tx.fee,
        feeRate: tx.feeRate,
        category: 'normal' as TxCategory,
      }));
  }, [summary?.pending]);

  const proposalItems = useMemo((): TxItem[] => {
    if (!summary?.proposals) return [];
    return summary.proposals.map((proposal) => ({
      id: proposal.fullTxHash || proposal.proposalId,
      size: proposal.size ?? 500,
      fee: proposal.fee ?? undefined,
      feeRate: proposal.feeRate ?? undefined,
      category: 'normal' as TxCategory,
    }));
  }, [summary?.proposals]);

  const tipBlockItems = useMemo((): TxItem[] => {
    if (!summary?.tipBlockTxs) return [];
    return summary.tipBlockTxs.map((tx) => {
      const feeRate = tx.txSize > 0 ? (tx.fee / tx.txSize) * 1000 : undefined;
      return {
        id: tx.hash,
        size: tx.txSize || 500,
        fee: tx.fee,
        feeRate,
        category: tx.isCellbase ? 'cellbase' : ('normal' as TxCategory),
      };
    });
  }, [summary?.tipBlockTxs]);

  const globalMaxSize = useMemo(() => {
    const allSizes = [
      ...pendingItems.map((item) => item.size),
      ...proposalItems.map((item) => item.size),
      ...tipBlockItems.map((item) => item.size),
    ];
    if (allSizes.length === 0) return 10000;
    return Math.max(...allSizes, 2000);
  }, [pendingItems, proposalItems, tipBlockItems]);

  const tipBlock = summary?.tipBlock;

  return (
    <div className="rounded-2xl border border-slate-700/50 bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 p-4 shadow-xl sm:p-6">
      <div className="mb-4 flex items-center justify-between">
        <h2 className="text-lg font-bold tracking-tight text-white sm:text-xl">Transaction Flow</h2>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] sm:text-xs">
          <div className="flex items-center gap-1.5">
            <div className="h-2.5 w-2.5 rounded-sm bg-slate-500/80" />
            <span className="text-slate-400">Pending</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="h-2.5 w-2.5 rounded-sm bg-amber-600/80" />
            <span className="text-slate-400">Proposed</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="h-2.5 w-2.5 rounded-sm bg-purple-600/80" />
            <span className="text-slate-400">Committed</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="h-2.5 w-2.5 rounded-sm bg-emerald-600/80" />
            <span className="text-slate-400">Cellbase</span>
          </div>
        </div>
      </div>

      <div className="flex items-stretch gap-0">
        <PackedContainer
          title="Mempool"
          subtitle="Pending transactions"
          type="mempool"
          items={pendingItems}
          totalCount={pendingItems.length}
          emptyText="No pending transactions"
          globalMaxSize={globalMaxSize}
        />

        <FlowArrow />

        <PackedContainer
          title="Proposed"
          subtitle="Awaiting commit"
          type="proposals"
          items={proposalItems}
          totalCount={proposalItems.length}
          emptyText="No proposed txs"
          globalMaxSize={globalMaxSize}
        />

        <FlowArrow />

        <PackedContainer
          title={`Block #${(tipBlock?.number ?? 0).toLocaleString()}`}
          subtitle="Latest Committed"
          type="tip"
          items={tipBlockItems}
          totalCount={summary?.tipBlockTxs?.length ?? tipBlockItems.length}
          blockNumber={tipBlock?.number}
          emptyText="No transactions"
          globalMaxSize={globalMaxSize}
        />
      </div>
    </div>
  );
}
