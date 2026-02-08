'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { useState, useMemo } from 'react';
import { api } from '@/lib/api';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { formatTimeAgo } from '@/lib/utils';

export default function TransactionsPage() {
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  const [cursorHistory, setCursorHistory] = useState<string[]>([]);
  const limit = 25;

  const { data, isLoading } = useQuery({
    queryKey: ['transactions', cursor, limit],
    queryFn: () => api.getTransactions({ cursor, limit }),
    staleTime: 30_000,
    gcTime: 5 * 60_000,
  });

  const handleNextPage = () => {
    if (data?.nextCursor) {
      setCursorHistory((prev) => [...prev, cursor || '']);
      setCursor(data.nextCursor);
    }
  };

  const handlePrevPage = () => {
    if (cursorHistory.length > 0) {
      const prev = cursorHistory[cursorHistory.length - 1];
      setCursorHistory((h) => h.slice(0, -1));
      setCursor(prev || undefined);
    }
  };

  const formattedNumbers = useMemo(() => {
    if (!data?.data) return new Map<string, string>();
    return new Map(data.data.map((tx) => [tx.hash, tx.blockNumber.toLocaleString()]));
  }, [data?.data]);

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader title="Transactions" subtitle="Browse all transactions on the CKB network" />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Transaction List</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
              <div className="flex-1">Tx Hash</div>
              <div className="w-32">Block</div>
              <div className="w-24 text-center">In/Out</div>
              <div className="w-32 text-right">Time</div>
            </div>

            {isLoading
              ? Array.from({ length: 10 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="flex animate-pulse items-center">
                      <div className="flex-1">
                        <div className="h-4 w-48 rounded bg-slate-800" />
                      </div>
                      <div className="w-32">
                        <div className="h-4 w-20 rounded bg-slate-800" />
                      </div>
                      <div className="w-24 text-center">
                        <div className="mx-auto h-4 w-16 rounded bg-slate-800" />
                      </div>
                      <div className="w-32 text-right">
                        <div className="ml-auto h-4 w-20 rounded bg-slate-800" />
                      </div>
                    </div>
                  </TerminalRow>
                ))
              : data?.data?.map((tx) => (
                  <TerminalRow key={tx.hash}>
                    <div className="flex items-center">
                      <div className="flex-1">
                        <Link href={`/tx/${tx.hash}`} className="hover:underline">
                          <HexDisplay value={tx.hash} color="green" startChars={12} endChars={8} />
                        </Link>
                      </div>
                      <div className="w-32">
                        <Link
                          href={`/blocks/${tx.blockNumber}`}
                          className="text-amber font-mono hover:underline"
                        >
                          #{formattedNumbers.get(tx.hash)}
                        </Link>
                      </div>
                      <div className="w-24 text-center font-mono text-slate-400">
                        <span className="text-terminal-dim">{tx.inputsCount}</span>
                        <span className="mx-1 text-slate-600">→</span>
                        <span className="text-terminal-dim">{tx.outputsCount}</span>
                      </div>
                      <div className="w-32 text-right text-slate-500">
                        {formatTimeAgo(tx.timestamp)}
                      </div>
                    </div>
                  </TerminalRow>
                ))}
          </TerminalPanelContent>

          {data && (
            <TerminalPanelFooter className="flex items-center justify-between">
              <span className="text-sm text-slate-500">
                {data.total != null && `Total: ${data.total.toLocaleString()} transactions`}
              </span>
              <div className="flex gap-2">
                <button
                  onClick={handlePrevPage}
                  disabled={cursorHistory.length === 0}
                  className="hover:border-terminal-dark hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-1.5 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                >
                  Previous
                </button>
                <button
                  onClick={handleNextPage}
                  disabled={!data.hasMore}
                  className="hover:border-terminal-dark hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-1.5 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                >
                  Next
                </button>
              </div>
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
