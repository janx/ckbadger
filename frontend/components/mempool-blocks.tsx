'use client';

import Link from 'next/link';
import { useQuery, useQueries } from '@tanstack/react-query';
import { api, MempoolBlock, Block, BlockFeeStats } from '@/lib/api';
import { cn } from '@/lib/utils';

interface MempoolBlocksProps {
  latestBlocks?: Block[];
}

function formatFeeRate(rate: number): string {
  if (rate < 0.01) return '<0.01';
  if (rate >= 1000) return `${(rate / 1000).toFixed(1)}K`;
  return rate.toFixed(2);
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

function getBlockColors(index: number, isPending: boolean): string {
  if (!isPending) {
    return 'from-purple-500 via-purple-600 to-purple-700';
  }
  const colorSets = [
    'from-emerald-400 via-emerald-500 to-emerald-600',
    'from-green-400 via-green-500 to-green-600',
    'from-lime-400 via-yellow-500 to-yellow-600',
    'from-yellow-400 via-amber-500 to-amber-600',
    'from-amber-400 via-orange-500 to-orange-600',
    'from-orange-400 via-orange-500 to-red-500',
    'from-red-400 via-red-500 to-red-600',
    'from-red-500 via-red-600 to-rose-700',
  ];
  return colorSets[Math.min(index, colorSets.length - 1)];
}

function Block2D({
  gradient,
  isEmpty,
  children,
}: {
  gradient: string;
  isEmpty?: boolean;
  children: React.ReactNode;
}) {
  if (isEmpty) {
    return (
      <div className="h-[80px] w-[100px] sm:h-[96px] sm:w-[116px]">
        <div className="flex h-full w-full flex-col items-center justify-center rounded-xl border-2 border-dashed border-slate-600/50 bg-slate-800/40 backdrop-blur-sm">
          {children}
        </div>
      </div>
    );
  }

  return (
    <div className="h-[80px] w-[100px] sm:h-[96px] sm:w-[116px]">
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

function PendingBlock({
  block,
  predictedNumber,
  isNextBlock,
}: {
  block: MempoolBlock;
  predictedNumber: number;
  isNextBlock: boolean;
}) {
  const isEmpty = block.transactionCount === 0;
  const gradient = isEmpty ? '' : getBlockColors(block.index, true);

  return (
    <div className="flex flex-col items-center">
      <div
        className={cn(
          'mb-1.5 font-mono text-xs font-semibold tabular-nums sm:text-sm',
          isEmpty ? 'text-slate-500' : 'text-amber-400'
        )}
      >
        {predictedNumber.toLocaleString()}
      </div>
      <Block2D gradient={gradient} isEmpty={isEmpty}>
        {isEmpty ? (
          <>
            <div className="text-[11px] font-medium text-slate-400">Empty</div>
            <div className="text-[10px] text-slate-500">0 txs</div>
          </>
        ) : (
          <div className="flex h-full w-full flex-col justify-between">
            <div className="flex flex-col items-center">
              <div className="font-mono text-[10px] font-bold tabular-nums text-white drop-shadow-sm sm:text-[11px]">
                ~{formatFeeRate(block.medianFeeRate)}
              </div>
              <div className="text-[8px] font-medium text-white/70">shannons/B</div>
              <div className="font-mono text-[8px] tabular-nums text-white/60">
                {formatFeeRate(block.feeRateRange.min)}-{formatFeeRate(block.feeRateRange.max)}
              </div>
            </div>
            <div className="flex flex-col items-center">
              <div className="font-mono text-[11px] font-bold tabular-nums text-white drop-shadow-sm sm:text-xs">
                {formatCapacity(block.totalFee)} CKB
              </div>
              <div className="text-[9px] font-medium text-white/70">
                {block.transactionCount} txs
              </div>
            </div>
          </div>
        )}
      </Block2D>
      {isNextBlock && (
        <div className="mt-1.5 rounded-full bg-amber-500/20 px-2 py-0.5 text-[10px] font-medium text-amber-400 sm:text-xs">
          Next Block
        </div>
      )}
    </div>
  );
}

function MinedBlock({ block, feeStats }: { block: Block; feeStats?: BlockFeeStats | 'loading' }) {
  const isLoading = feeStats === 'loading';
  const stats = feeStats && feeStats !== 'loading' ? feeStats : null;
  const gradient = getBlockColors(0, false);

  return (
    <Link href={`/blocks/${block.number}`} className="group/block flex flex-col items-center">
      <div className="mb-1.5 font-mono text-xs font-semibold tabular-nums text-purple-400 transition-colors group-hover/block:text-purple-300 sm:text-sm">
        {block.number.toLocaleString()}
      </div>
      <div className="relative">
        <Block2D gradient={gradient}>
          {isLoading ? (
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
            <div className="flex h-full w-full flex-col justify-between">
              <div className="flex flex-col items-center">
                <div className="font-mono text-[11px] font-bold tabular-nums text-white drop-shadow-sm sm:text-xs">
                  ~{formatFeeRate(stats.avgFeeRate)}
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
            <div className="flex h-full w-full flex-col items-center justify-center">
              <div className="text-xl font-bold text-white drop-shadow-sm sm:text-2xl">
                {block.transactionsCount}
              </div>
              <div className="text-[10px] font-medium text-white/80">transactions</div>
              <div className="text-[9px] text-white/50">{formatTimeAgo(block.timestamp)}</div>
            </div>
          )}
        </Block2D>
        <div className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-3 -translate-x-1/2 whitespace-nowrap rounded-xl border border-slate-700/50 bg-slate-900/95 px-4 py-3 text-xs text-white opacity-0 shadow-2xl backdrop-blur-sm transition-opacity group-hover/block:opacity-100">
          <div className="flex min-w-[260px] flex-col gap-1.5">
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
          <div className="absolute left-1/2 top-full -translate-x-1/2 border-[6px] border-transparent border-t-slate-900/95" />
        </div>
      </div>
    </Link>
  );
}

function ChainArrow({ isPending = false }: { isPending?: boolean }) {
  return (
    <div className="flex items-center justify-center px-1 sm:px-2">
      <div
        className={cn('flex items-center gap-0.5', isPending ? 'text-slate-500' : 'text-slate-400')}
      >
        <div
          className={cn('h-0.5 w-3 rounded-full', isPending ? 'bg-slate-600' : 'bg-slate-500')}
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

function MempoolDivider() {
  return (
    <div className="flex flex-col items-center justify-center px-3 sm:px-4">
      <div className="flex h-[80px] items-center sm:h-[96px]">
        <div className="flex items-center gap-2">
          <div className="h-px w-4 bg-gradient-to-r from-transparent to-slate-500" />
          <div className="rounded-md border border-slate-600/50 bg-slate-800/50 px-2 py-1 text-[10px] font-medium tracking-wider text-slate-400">
            MINED
          </div>
          <div className="h-px w-4 bg-gradient-to-l from-transparent to-slate-500" />
        </div>
      </div>
    </div>
  );
}

export function MempoolBlocks({ latestBlocks }: MempoolBlocksProps) {
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
  const displayedMinedBlocks = minedBlocks.slice(0, 3);

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

  if (mempoolLoading && pendingBlocks.length === 0) {
    return (
      <div className="rounded-2xl border border-slate-700/50 bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 p-5 shadow-xl">
        <h2 className="mb-5 text-lg font-bold tracking-tight text-white sm:text-xl">
          Chain Tip Intelligence
        </h2>
        <div className="flex items-center justify-center gap-3 py-4 sm:gap-4">
          {Array.from({ length: 6 }).map((_, i) => (
            <div
              key={i}
              className="h-[80px] w-[100px] animate-pulse rounded-xl bg-slate-700/50 sm:h-[96px] sm:w-[116px]"
            />
          ))}
        </div>
      </div>
    );
  }

  const totalPending = mempoolData?.totalPendingCount ?? 0;
  const totalProposed = mempoolData?.totalProposedCount ?? 0;
  const latestBlockNumber = minedBlocks[0]?.number ?? 0;
  const displayPendingBlocks = pendingBlocks.slice(0, 4).reverse();

  return (
    <div className="rounded-2xl border border-slate-700/50 bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 p-5 shadow-xl">
      <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <h2 className="text-lg font-bold tracking-tight text-white sm:text-xl">
          Chain Tip Intelligence
        </h2>
        <div className="flex items-center gap-4 text-xs sm:text-sm">
          <div className="flex items-center gap-1.5">
            <div className="h-2 w-2 rounded-full bg-emerald-500 shadow-sm shadow-emerald-500/50" />
            <span className="text-slate-400">Proposed:</span>
            <span className="font-semibold tabular-nums text-emerald-400">{totalProposed}</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="h-2 w-2 rounded-full bg-amber-500 shadow-sm shadow-amber-500/50" />
            <span className="text-slate-400">Mempool:</span>
            <span className="font-semibold tabular-nums text-amber-400">{totalPending}</span>
          </div>
        </div>
      </div>

      <div className="flex items-center justify-center gap-1 sm:gap-2">
        {displayPendingBlocks.map((block, index) => (
          <div key={block.index} className="flex items-center">
            <PendingBlock
              block={block}
              predictedNumber={latestBlockNumber + block.index + 1}
              isNextBlock={block.index === 0}
            />
            {index < displayPendingBlocks.length - 1 && <ChainArrow isPending />}
          </div>
        ))}

        {pendingBlocks.length > 0 && minedBlocks.length > 0 && <MempoolDivider />}

        {displayedMinedBlocks.map((block, index) => (
          <div key={block.hash} className="flex items-center">
            <MinedBlock block={block} feeStats={feeStatsMap.get(block.number)} />
            {index < displayedMinedBlocks.length - 1 && <ChainArrow />}
          </div>
        ))}
      </div>
    </div>
  );
}
