'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api, Transaction } from '@/lib/api';
import { formatTimeAgo, cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { HexDisplay } from '@/components/ui/hex-display';

interface LatestTransactionsProps {
  isRealtime?: boolean;
  initialTransactions?: Transaction[];
}

export function LatestTransactions({
  isRealtime = false,
  initialTransactions,
}: LatestTransactionsProps) {
  const [newTxHash, setNewTxHash] = useState<string | null>(null);
  const prevTxsRef = useRef<string[]>([]);

  const {
    data: txs,
    isLoading,
    isFetching,
  } = useQuery({
    queryKey: ['latest-transactions'],
    queryFn: () => api.getTransactions({ limit: 10 }),
    initialData: initialTransactions?.length
      ? {
          data: initialTransactions,
          total: initialTransactions.length,
          limit: 10,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    initialDataUpdatedAt: 0,
    refetchInterval: 10000,
  });

  const itemCount = txs?.data?.length ?? 0;
  const showSkeleton = isLoading || (itemCount === 0 && isFetching);

  useEffect(() => {
    if (txs?.data) {
      const currentHashes = txs.data.map((t) => t.hash);
      const prevHashes = prevTxsRef.current;

      if (prevHashes.length > 0) {
        const newTx = currentHashes.find((h) => !prevHashes.includes(h));
        if (newTx) {
          setNewTxHash(newTx);
          setTimeout(() => setNewTxHash(null), 2000);
        }
      }

      prevTxsRef.current = currentHashes;
    }
  }, [txs?.data]);

  const headerActions = (
    <Link
      href="/transactions"
      className="text-text-dim hover:text-interactive font-mono text-xs transition-colors"
    >
      VIEW ALL →
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        Latest Transactions
      </TerminalPanelHeader>
      <TerminalPanelContent padding="none">
        {showSkeleton
          ? Array.from({ length: 8 }).map((_, i) => (
              <TerminalRow key={i} hoverable={false}>
                <div className="flex animate-pulse items-center justify-between">
                  <div className="space-y-2">
                    <div className="bg-base-elevated h-4 w-32 rounded" />
                    <div className="bg-base-elevated h-3 w-20 rounded" />
                  </div>
                  <div className="space-y-2 text-right">
                    <div className="bg-base-elevated h-3 w-16 rounded" />
                    <div className="bg-base-elevated h-3 w-12 rounded" />
                  </div>
                </div>
              </TerminalRow>
            ))
          : txs?.data?.slice(0, 8).map((tx) => (
              <TerminalRow
                key={tx.hash}
                className={cn(
                  'transition-all duration-500',
                  newTxHash === tx.hash && 'bg-interactive/10 shadow-interactive-glow'
                )}
              >
                <div className="flex items-center justify-between gap-4">
                  <div className="min-w-0 flex-1">
                    <Link href={`/tx/${tx.hash}`} className="group block">
                      <HexDisplay
                        value={tx.hash}
                        truncate
                        startChars={8}
                        endChars={6}
                        color="aqua"
                        size="sm"
                        showGroupHighlight={false}
                      />
                    </Link>
                    <div className="mt-1.5 flex items-center gap-2 text-xs">
                      <span className="text-text-dim">Block</span>
                      <Link
                        href={`/blocks/${tx.blockNumber}`}
                        className="text-text-dim hover:text-interactive font-mono"
                      >
                        #{tx.blockNumber.toLocaleString()}
                      </Link>
                    </div>
                  </div>

                  <div className="shrink-0 text-right">
                    <div className="flex items-center gap-1.5 font-mono text-sm">
                      <span className="text-gold-dim">{tx.inputsCount}</span>
                      <span className="text-text-dim">→</span>
                      <span className="text-warning-dim">{tx.outputsCount}</span>
                    </div>
                    <div className="text-text-dim mt-1.5 text-xs">
                      {formatTimeAgo(tx.timestamp)}
                    </div>
                  </div>
                </div>
              </TerminalRow>
            ))}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
