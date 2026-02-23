'use client';

import Link from 'next/link';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { useMemo } from 'react';
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
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { formatTimeAgo } from '@/lib/utils';

export default function TransactionsPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = 25;

  const { data, isLoading } = useQuery({
    queryKey: ['transactions', cursor, limit],
    queryFn: () => api.getTransactions({ cursor, limit }),
    staleTime: 30_000,
    gcTime: 5 * 60_000,
    placeholderData: keepPreviousData,
  });

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
              <div className="flex-1">Transaction</div>
              <div className="w-24 text-center">In/Out</div>
              <div className="w-32 text-right">Time</div>
            </div>

            {isLoading
              ? Array.from({ length: 10 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="flex animate-pulse items-center">
                      <div className="flex-1">
                        <div className="h-4 w-48 rounded bg-slate-800" />
                        <div className="mt-1 h-3 w-20 rounded bg-slate-800" />
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
                          <HexDisplay value={tx.hash} color="accent" startChars={12} endChars={8} />
                        </Link>
                        <Link
                          href={`/blocks/${tx.blockNumber}`}
                          className="text-terminal-green block font-mono text-xs hover:underline"
                        >
                          #{formattedNumbers.get(tx.hash)}
                        </Link>
                      </div>
                      <div className="w-24 text-center font-mono text-slate-400">
                        <span className="text-terminal-dim">{tx.inputsCount}</span>
                        <span className="mx-1 text-slate-500">→</span>
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
            <TerminalPanelFooter>
              <CursorPagination
                total={data.total ?? undefined}
                totalLabel="transactions"
                pageSize={limit}
                page={page}
                hasMore={data.hasMore}
                hasPrevious={hasPrevious}
                onNext={() => goToNext(data.nextCursor)}
                onPrevious={goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
