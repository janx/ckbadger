'use client';

import { useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { HexDisplay } from '@/components/ui/hex-display';
import { ExactCkb } from '@/components/activity-event-row';
import { TransactionTable } from '@/components/transaction/transaction-table';
import { PoolInterpretationNotice, poolStatusLabel } from '@/components/ui/pool-status';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';

type PoolRow = Awaited<ReturnType<typeof api.getPoolTransactions>>['data'][number];

export default function TransactionsPage() {
  const [tab, setTab] = useState<'committed' | 'pool'>('committed');
  const committedPage = useCursorPagination();
  const poolPage = useCursorPagination();
  const limit = DEFAULT_PAGE_SIZE;
  const committed = useQuery({
    queryKey: ['transactions', committedPage.cursor, limit],
    queryFn: () => api.getTransactions({ cursor: committedPage.cursor, limit }),
    staleTime: 30_000,
    placeholderData: keepPreviousData,
  });
  const pool = useQuery({
    queryKey: ['pool-transactions', poolPage.cursor, limit],
    queryFn: () => api.getPoolTransactions({ cursor: poolPage.cursor, limit }),
    enabled: tab === 'pool',
    refetchInterval: 5000,
    placeholderData: keepPreviousData,
  });
  const poolColumns: ListColumn<PoolRow>[] = [
    {
      key: 'tx',
      label: 'Transaction',
      minWidth: 190,
      render: (tx) => (
        <div className="space-y-1">
          <Link href={`/tx/${tx.hash}`}>
            <HexDisplay value={tx.hash} startChars={10} endChars={6} />
          </Link>
          <PoolInterpretationNotice interpretation={tx.interpretation} />
        </div>
      ),
    },
    {
      key: 'status',
      label: 'State',
      minWidth: 100,
      render: (tx) => (
        <span className={tx.status === 'proposed' ? 'text-gold' : 'text-text-dim'}>
          {poolStatusLabel(tx.status)}
        </span>
      ),
    },
    {
      key: 'time',
      label: 'Seen',
      minWidth: 90,
      render: (tx) => formatTimeAgo(new Date(Number(tx.timestamp))),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 75,
      numeric: true,
      render: (tx) => `${tx.inputsCount} → ${tx.outputsCount}`,
    },
    {
      key: 'value',
      label: 'Value',
      minWidth: 180,
      numeric: true,
      render: (tx) =>
        tx.value === null ? (
          <span className="text-text-dim">Interpretation partial</span>
        ) : (
          <ExactCkb
            shannons={parseShannons(tx.value, `pool transaction ${tx.hash} value`)}
            signed={false}
            tone="plain"
          />
        ),
    },
    {
      key: 'fee',
      label: 'Fee',
      minWidth: 150,
      numeric: true,
      render: (tx) => (
        <ExactCkb
          shannons={parseShannons(tx.fee, `pool transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
        />
      ),
    },
    {
      key: 'feeRate',
      label: 'Fee rate, shannon/KB',
      minWidth: 155,
      numeric: true,
      render: (tx) => BigInt(tx.feeRate).toLocaleString('en-US'),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      minWidth: 100,
      numeric: true,
      render: (tx) => tx.txSize.toLocaleString('en-US'),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      minWidth: 110,
      numeric: true,
      render: (tx) => BigInt(tx.cycles).toLocaleString('en-US'),
    },
  ];
  const active = tab === 'committed' ? committed : pool;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        <DetailLayout title="Transactions" kind="index">
          <div
            role="group"
            aria-label="Transaction views"
            className="border-base-border-subtle flex flex-wrap gap-6 border-b"
          >
            {(['committed', 'pool'] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={tab === value}
                onClick={() => setTab(value)}
                className={`min-h-11 border-b-2 text-[13px] ${tab === value ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
              >
                {value === 'committed' ? 'Committed' : 'In the pool'}
              </button>
            ))}
          </div>
          {active.isLoading && (
            <p role="status" className="text-text-dim">
              Loading transactions…
            </p>
          )}
          {active.error && (
            <p role="alert" className="text-text-dim">
              Unable to read transactions: {active.error.message}
            </p>
          )}
          {tab === 'committed' && committed.data && (
            <>
              <TransactionTable transactions={committed.data.data} />
              <p className="text-text-dim text-xs">
                Value is the sum of positive participant CKB changes. Cellbase issuance is excluded.
              </p>
              <CursorPagination
                total={committed.data.total ?? undefined}
                totalLabel="transactions"
                pageSize={limit}
                page={committedPage.page}
                hasMore={committed.data.hasMore}
                hasPrevious={committedPage.hasPrevious}
                onNext={() => committedPage.goToNext(committed.data.nextCursor)}
                onPrevious={committedPage.goToPrevious}
              />
            </>
          )}
          {tab === 'pool' && pool.data && (
            <>
              <p className="text-text-dim text-xs">
                {pool.data.totals.pending.toLocaleString('en-US')} pending ·{' '}
                {pool.data.totals.proposed.toLocaleString('en-US')} proposed ·{' '}
                {pool.data.totals.awaitingIndex.toLocaleString('en-US')} awaiting index
              </p>
              <ListTable
                label="Transactions in the pool"
                columns={poolColumns}
                rows={pool.data.data}
                rowKey={(tx) => tx.hash}
                empty="The node's pool is empty."
              />
              {pool.data.truncated && (
                <p className="text-text-dim text-xs">
                  This node's tracked pool is truncated; more transactions may be present.
                </p>
              )}
              <CursorPagination
                total={pool.data.total}
                totalLabel="pool transactions"
                pageSize={limit}
                page={poolPage.page}
                hasMore={pool.data.nextCursor !== null}
                hasPrevious={poolPage.hasPrevious}
                onNext={() => poolPage.goToNext(pool.data.nextCursor)}
                onPrevious={poolPage.goToPrevious}
              />
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
