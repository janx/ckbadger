'use client';

import Link from '@/components/ui/link';
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
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatTimeAgo } from '@/lib/utils';

export default function TransactionsPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = DEFAULT_PAGE_SIZE;

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
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader title="Transactions" subtitle="Browse all transactions on the CKB network" />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Transaction List</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider sm:flex">
              <div className="min-w-0 flex-1">Transaction</div>
              <div className="w-20 shrink-0 text-center">In/Out</div>
              <div className="w-24 shrink-0 text-right">Time</div>
            </div>

            {isLoading
              ? Array.from({ length: 10 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="hidden animate-pulse items-center sm:flex">
                      <div className="min-w-0 flex-1">
                        <div className="bg-base-elevated h-4 w-48 rounded" />
                        <div className="bg-base-elevated mt-1 h-3 w-20 rounded" />
                      </div>
                      <div className="w-20 shrink-0 text-center">
                        <div className="bg-base-elevated mx-auto h-4 w-16 rounded" />
                      </div>
                      <div className="w-24 shrink-0 text-right">
                        <div className="bg-base-elevated ml-auto h-4 w-20 rounded" />
                      </div>
                    </div>
                    <div className="animate-pulse space-y-1 sm:hidden">
                      <div className="flex items-center justify-between gap-2">
                        <div className="bg-base-elevated h-4 w-40 rounded" />
                        <div className="bg-base-elevated h-4 w-16 shrink-0 rounded" />
                      </div>
                      <div className="flex items-center justify-between">
                        <div className="bg-base-elevated h-3 w-20 rounded" />
                        <div className="bg-base-elevated h-3 w-16 rounded" />
                      </div>
                    </div>
                  </TerminalRow>
                ))
              : data?.data?.map((tx) => (
                  <TerminalRow key={tx.hash}>
                    <div className="hidden items-center sm:flex">
                      <div className="min-w-0 flex-1">
                        <Link href={`/tx/${tx.hash}`} className="hover:underline">
                          <HexDisplay value={tx.hash} startChars={12} endChars={8} />
                        </Link>
                        <Link
                          href={`/blocks/${tx.blockNumber}`}
                          className="text-emphasis block font-mono text-xs hover:underline"
                        >
                          #{formattedNumbers.get(tx.hash)}
                        </Link>
                      </div>
                      <div className="text-text-dim w-20 shrink-0 text-center font-mono">
                        <span className="text-emphasis-dim">{tx.inputsCount}</span>
                        <span className="text-text-dim mx-1">→</span>
                        <span className="text-emphasis-dim">{tx.outputsCount}</span>
                      </div>
                      <div className="text-text-dim w-24 shrink-0 text-right">
                        {formatTimeAgo(tx.timestamp)}
                      </div>
                    </div>
                    <div className="space-y-1 sm:hidden">
                      <div className="flex items-center justify-between gap-2">
                        <Link href={`/tx/${tx.hash}`} className="hover:underline">
                          <HexDisplay value={tx.hash} startChars={10} endChars={6} />
                        </Link>
                        <span className="text-text-dim shrink-0 text-xs">
                          {formatTimeAgo(tx.timestamp)}
                        </span>
                      </div>
                      <div className="flex items-center justify-between">
                        <Link
                          href={`/blocks/${tx.blockNumber}`}
                          className="text-emphasis font-mono text-xs hover:underline"
                        >
                          #{formattedNumbers.get(tx.hash)}
                        </Link>
                        <span className="text-text-dim font-mono text-xs">
                          <span className="text-emphasis-dim">{tx.inputsCount}</span>
                          <span className="mx-1">→</span>
                          <span className="text-emphasis-dim">{tx.outputsCount}</span>
                        </span>
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
