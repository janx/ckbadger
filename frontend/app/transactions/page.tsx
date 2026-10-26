'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { HexDisplay } from '@/components/ui/hex-display';
import { ExactCkb, activityHeadline } from '@/components/activity-event-row';
import { TransactionTable } from '@/components/transaction/transaction-table';
import { PoolInterpretationNotice, poolStatusLabel } from '@/components/ui/pool-status';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo, cn } from '@/lib/utils';
import { ClassMark } from '@/components/ui/class-mark';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import {
  activityFilterCounts,
  completedYesterdayActivities,
  completedYesterdayTransactions,
  previousChainDay,
  totalActivities,
} from '@/lib/activity-stats';

type TransactionKind =
  | 'all'
  | 'ckb'
  | 'script'
  | 'cellbase'
  | 'dao'
  | 'token'
  | 'object'
  | 'identity';
const KINDS: { value: TransactionKind; label: string; mark?: AssetClass; color: string }[] = [
  { value: 'all', label: 'All', color: '#7c849c' },
  { value: 'ckb', label: 'CKB transfers', color: '#2edba3' },
  { value: 'script', label: 'Script calls', color: CLASS_COLORS.ckb },
  { value: 'cellbase', label: 'Cellbase', color: CLASS_COLORS.other },
  { value: 'dao', label: 'DAO', mark: 'dao', color: CLASS_COLORS.dao },
  { value: 'token', label: 'Tokens', mark: 'tokens', color: CLASS_COLORS.tokens },
  { value: 'object', label: 'Objects', mark: 'objects', color: CLASS_COLORS.objects },
  { value: 'identity', label: 'Identities', mark: 'identities', color: CLASS_COLORS.identities },
];
type PoolRow = Awaited<ReturnType<typeof api.getPoolTransactions>>['data'][number];

export default function TransactionsPage() {
  const [kind, setKind] = useState<TransactionKind>('all');
  const activityType = kind === 'all' ? undefined : kind;
  const [tab, setTab] = useState<'committed' | 'pool'>('committed');
  const committedPage = useCursorPagination();
  const poolPage = useCursorPagination();
  const limit = DEFAULT_PAGE_SIZE;
  const committed = useQuery({
    queryKey: ['transactions', committedPage.cursor, limit, activityType],
    queryFn: () =>
      api.getTransactions({
        cursor: committedPage.cursor,
        limit,
        ...(activityType ? { activityType } : {}),
      }),
    staleTime: 30_000,
  });
  const pool = useQuery({
    queryKey: ['pool-transactions', poolPage.cursor, limit, activityType],
    queryFn: () =>
      api.getPoolTransactions({
        cursor: poolPage.cursor,
        limit,
        ...(activityType ? { activityType } : {}),
      }),
    enabled: tab === 'pool',
    refetchInterval: 5000,
  });
  const poolHead = useQuery({
    queryKey: ['pool-transactions', undefined, limit, undefined],
    queryFn: () => api.getPoolTransactions({ cursor: undefined, limit }),
    refetchInterval: 5000,
  });
  const { data: daily, error: dailyError } = useQuery({
    queryKey: ['daily-activity-stats', 2],
    queryFn: () => api.getDailyActivityStats(2),
    refetchInterval: 60000,
  });
  const { data: txStats, error: txStatsError } = useQuery({
    queryKey: ['tx-stats'],
    queryFn: () => api.getTxStats(),
    refetchInterval: 60000,
  });
  const yesterday = previousChainDay();
  const day = daily ? completedYesterdayActivities(daily) : undefined;
  const total = txStats ? completedYesterdayTransactions(txStats) : undefined;
  const counts: Partial<Record<TransactionKind, number>> = day
    ? { ...activityFilterCounts(day, total), cellbase: day.coinbaseCount }
    : total === undefined
      ? {}
      : { all: total };
  const occurrences = day ? totalActivities(day) + day.coinbaseCount : 0;
  const largestPoolSize = poolHead.data?.data.length
    ? Math.max(...poolHead.data.data.map((row) => row.txSize))
    : undefined;
  const chooseKind = (value: TransactionKind) => {
    setKind(value);
    committedPage.reset();
    poolPage.reset();
  };
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
      key: 'activity',
      label: 'What it does',
      minWidth: 200,
      render: (tx) =>
        tx.activity ? (
          activityHeadline(tx.activity)
        ) : (
          <span className="text-text-dim">Interpretation unavailable</span>
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
        <DetailLayout
          title="Transactions"
          kind="index"
          actions={
            <Link href="/activities" className="text-text-dim hover:text-jade text-xs">
              Activities →
            </Link>
          }
        >
          <section className="space-y-4" aria-label="Yesterday and the pool">
            <p className="text-text text-[15px]">
              {total === undefined ? (
                <span className="text-text-dim">Yesterday’s total unavailable</span>
              ) : (
                <>
                  <span className="text-text-bright">{total.toLocaleString('en-US')}</span>{' '}
                  yesterday (UTC+8)
                </>
              )}
              {poolHead.data && (
                <>
                  {' '}
                  ·{' '}
                  <span className="text-text-bright">
                    {(poolHead.data.totals.pending + poolHead.data.totals.proposed).toLocaleString(
                      'en-US'
                    )}
                  </span>{' '}
                  waiting in the pool now
                </>
              )}
            </p>
            {(dailyError || txStatsError) && (
              <p role="alert" className="text-text-dim text-xs">
                Could not read yesterday’s statistics: {(dailyError ?? txStatsError)!.message}
              </p>
            )}
            {daily && !day && (
              <p className="text-text-dim text-xs">
                No complete activity statistics for {yesterday} (UTC+8).
              </p>
            )}
            {txStats && total === undefined && (
              <p className="text-text-dim text-xs">
                No complete transaction count for {yesterday} (UTC+8).
              </p>
            )}
            <div
              role="group"
              aria-label="Filter by kind, with yesterday's counts"
              className="flex flex-wrap gap-2"
            >
              {KINDS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-label={option.label}
                  aria-pressed={kind === option.value}
                  onClick={() => chooseKind(option.value)}
                  className={cn(
                    'flex min-h-[34px] items-center gap-2 rounded-md border px-3 text-xs',
                    kind === option.value
                      ? 'border-base-border bg-base-surface text-text-bright'
                      : 'text-text-dim hover:text-text border-transparent'
                  )}
                >
                  {option.mark ? (
                    <ClassMark kind={option.mark} />
                  ) : option.value !== 'all' ? (
                    <span
                      aria-hidden="true"
                      className="h-2 w-2 rounded-sm"
                      style={{ backgroundColor: option.color }}
                    />
                  ) : null}
                  {option.label}
                  {counts[option.value] !== undefined && (
                    <span className="text-text-dim text-[11px]">
                      {counts[option.value]!.toLocaleString('en-US')}
                      {option.value === 'dao' ? ' actions' : ''}
                    </span>
                  )}
                </button>
              ))}
            </div>
            {day && (
              <p className="text-text-dim text-[11px]">
                Kind occurrences may overlap; DAO counts actions. “All” counts unique transactions.
              </p>
            )}
            {occurrences > 0 && (
              <div
                role="img"
                aria-label={`Yesterday's kind occurrences; categories overlap`}
                className="flex h-2 overflow-hidden rounded-sm"
              >
                {KINDS.filter(
                  (option) =>
                    option.value !== 'all' &&
                    counts[option.value] !== undefined &&
                    counts[option.value]! > 0
                ).map((option) => (
                  <span
                    key={option.value}
                    title={`${option.label}: ${counts[option.value]}`}
                    style={{
                      width: `${(counts[option.value]! / occurrences) * 100}%`,
                      backgroundColor: option.color,
                    }}
                  />
                ))}
              </div>
            )}
            {poolHead.error ? (
              <p role="alert" className="text-text-dim text-xs">
                Could not read the pool: {poolHead.error.message}
              </p>
            ) : poolHead.data ? (
              <div
                data-testid="pool-banner"
                className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs"
              >
                <span className="text-text-dim">In the pool</span>
                <span
                  aria-label="Tracked pool transactions, squares sized by bytes"
                  className="flex max-w-full flex-wrap items-center gap-1"
                >
                  {poolHead.data.data.map((tx) => (
                    <Link
                      key={tx.hash}
                      href={`/tx/${tx.hash}`}
                      title={`${tx.hash}: ${tx.txSize} bytes, ${tx.feeRate} shannon/KB`}
                      style={{
                        display: 'block',
                        width: 18 * Math.sqrt(tx.txSize / largestPoolSize!),
                        height: 18 * Math.sqrt(tx.txSize / largestPoolSize!),
                        background: tx.status === 'proposed' ? '#f2c55c' : '#68ccf0',
                      }}
                      aria-label={`Pool transaction ${tx.hash}`}
                    />
                  ))}
                  {poolHead.data.total > poolHead.data.data.length && (
                    <span className="text-text-dim">
                      +{(poolHead.data.total - poolHead.data.data.length).toLocaleString('en-US')}
                    </span>
                  )}
                </span>
                <span>
                  {poolHead.data.totals.pending.toLocaleString('en-US')} pending ·{' '}
                  {poolHead.data.totals.proposed.toLocaleString('en-US')} proposed ·{' '}
                  {poolHead.data.totals.awaitingIndex.toLocaleString('en-US')} awaiting index
                  {poolHead.data.summary.newestSeenAt !== null && (
                    <> · newest {formatTimeAgo(poolHead.data.summary.newestSeenAt)}</>
                  )}
                </span>
                <button
                  type="button"
                  onClick={() => setTab('pool')}
                  className="border-base-border text-text hover:text-text-bright rounded-md border px-3 py-2"
                >
                  Open the pool →
                </button>
              </div>
            ) : (
              <p className="text-text-dim text-xs">Reading the pool…</p>
            )}
          </section>
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
                className="flex-col items-start gap-3 sm:flex-row sm:items-center"
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
              <dl className="flex flex-wrap gap-x-8 gap-y-3 text-xs" aria-label="Pool summary">
                <div>
                  <dt className="text-text-dim">Size</dt>
                  <dd>{BigInt(pool.data.summary.size).toLocaleString('en-US')} bytes</dd>
                </div>
                <div>
                  <dt className="text-text-dim">Cycles</dt>
                  <dd>{BigInt(pool.data.summary.cycles).toLocaleString('en-US')}</dd>
                </div>
                <div>
                  <dt className="text-text-dim">Fee rate range</dt>
                  <dd>
                    {pool.data.summary.minFeeRate === null || pool.data.summary.maxFeeRate === null
                      ? 'No fee rates'
                      : `${BigInt(pool.data.summary.minFeeRate).toLocaleString('en-US')}–${BigInt(pool.data.summary.maxFeeRate).toLocaleString('en-US')} shannon/KB`}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim">Newest seen</dt>
                  <dd>
                    {pool.data.summary.newestSeenAt === null
                      ? 'No transactions'
                      : formatTimeAgo(pool.data.summary.newestSeenAt)}
                  </dd>
                </div>
              </dl>
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
                className="flex-col items-start gap-3 sm:flex-row sm:items-center"
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
