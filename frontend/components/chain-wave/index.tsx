'use client';

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, Block, NetworkStats, MempoolTransaction, Transaction } from '@/lib/api';
import { PackedContainer, TxItem, TxCategory } from './packed-container';
import { ProposalsContainer } from './proposals-container';

interface ChainWaveProps {
  initialBlocks?: Block[];
  stats?: NetworkStats | null;
}

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

function mempoolTxToItem(tx: MempoolTransaction): TxItem {
  return {
    id: tx.txHash,
    size: tx.size,
    fee: tx.fee,
    feeRate: tx.feeRate,
    category: 'normal' as TxCategory,
  };
}

function blockTxToItem(tx: Transaction): TxItem {
  const feeNum = parseFloat(tx.fee) || 0;
  const feeRate = tx.txSize && tx.txSize > 0 ? feeNum / tx.txSize : undefined;
  return {
    id: tx.hash,
    size: tx.txSize ?? 500,
    fee: feeNum,
    feeRate,
    category: tx.isCellbase ? 'cellbase' : 'normal',
  };
}

export function ChainWave({ initialBlocks }: ChainWaveProps) {
  const { data: mempoolTxs } = useQuery({
    queryKey: ['mempool-transactions'],
    queryFn: () => api.getMempoolTransactions(),
    refetchInterval: 5000,
  });

  const { data: blocksData } = useQuery({
    queryKey: ['chain-wave-tip-block'],
    queryFn: () => api.getBlocks({ limit: 1 }),
    initialData: initialBlocks?.length
      ? {
          data: initialBlocks.slice(0, 1),
          total: 1,
          limit: 1,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    refetchInterval: 10000,
  });

  const tipBlock = blocksData?.data?.[0];

  const { data: tipBlockTxs } = useQuery({
    queryKey: ['block-transactions', tipBlock?.number],
    queryFn: () =>
      tipBlock
        ? api.getTransactions({ blockNumber: tipBlock.number, limit: 200 })
        : Promise.resolve({ data: [], total: 0, limit: 200, hasMore: false, nextCursor: null }),
    enabled: !!tipBlock,
    refetchInterval: 10000,
  });

  const pendingItems = useMemo(() => {
    if (!mempoolTxs) return [];
    return mempoolTxs.filter((tx) => tx.status === 'pending').map(mempoolTxToItem);
  }, [mempoolTxs]);

  const proposedTxHashes = useMemo(() => {
    if (!mempoolTxs) return [];
    return mempoolTxs.filter((tx) => tx.status === 'proposed').map((tx) => tx.txHash);
  }, [mempoolTxs]);

  const tipBlockItems = useMemo(() => {
    if (!tipBlockTxs?.data) return [];
    return tipBlockTxs.data.map(blockTxToItem);
  }, [tipBlockTxs]);

  const globalMaxSize = useMemo(() => {
    const allSizes = [
      ...pendingItems.map((item) => item.size),
      ...tipBlockItems.map((item) => item.size),
    ];
    if (allSizes.length === 0) return 10000;
    return Math.max(...allSizes, 2000);
  }, [pendingItems, tipBlockItems]);

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

        <ProposalsContainer shortIds={proposedTxHashes} totalCount={proposedTxHashes.length} />

        <FlowArrow />

        <PackedContainer
          title={`Block #${(tipBlock?.number ?? 0).toLocaleString()}`}
          subtitle="Latest Committed"
          type="tip"
          items={tipBlockItems}
          totalCount={tipBlockTxs?.total ?? tipBlockItems.length}
          blockNumber={tipBlock?.number}
          emptyText="No transactions"
          globalMaxSize={globalMaxSize}
        />
      </div>
    </div>
  );
}
