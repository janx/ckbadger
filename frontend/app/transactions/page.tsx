'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useEffect, useState } from 'react';
import { useInRouterContext } from 'react-router-dom';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { DetailLayout } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { RowSeal } from '@/components/ui/row-seal';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { ExactCkb, activityHeadline } from '@/components/activity-event-row';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import { PoolInterpretationNotice, poolStatusLabel } from '@/components/ui/pool-status';
import type { PoolStatus } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo, formatUtc8DateTime, cn } from '@/lib/utils';
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
  { value: 'all', label: 'All', color: 'var(--c-text-dim)' },
  { value: 'ckb', label: 'CKB transfers', color: 'var(--c-jade)' },
  { value: 'script', label: 'Script calls', color: CLASS_COLORS.ckb },
  { value: 'cellbase', label: 'Cellbase', color: CLASS_COLORS.other },
  { value: 'dao', label: 'Nervos DAO', mark: 'dao', color: CLASS_COLORS.dao },
  { value: 'token', label: 'Tokens', mark: 'tokens', color: CLASS_COLORS.tokens },
  { value: 'object', label: 'Objects', mark: 'objects', color: CLASS_COLORS.objects },
  { value: 'identity', label: 'Identities', mark: 'identities', color: CLASS_COLORS.identities },
];
/**
 * The kind a URL names (`/transactions?kind=dao`, the DAO page's "DAO transactions →"): a known
 * kind, else All (hierarchy D11).
 */
function kindFromUrl(raw: string | null): TransactionKind {
  const known = KINDS.find((option) => option.value === raw);
  return known ? known.value : 'all';
}
type PoolRow = Awaited<ReturnType<typeof api.getPoolTransactions>>['data'][number];
/**
 * Pool squares (TX-38): pending cyan, proposed gold (gold's "proposed" meaning), and a committed
 * transaction this store has not indexed yet as a hollow grey outline — it is no longer waiting.
 */
const POOL_SQUARE: Record<PoolStatus, string> = {
  pending: 'bg-info',
  proposed: 'bg-gold',
  committed_awaiting_index: 'border border-text-dim',
};
function PoolDot({ status }: { status: PoolStatus }) {
  return (
    <span aria-hidden="true" className={cn('h-[7px] w-[7px] rounded-full', POOL_SQUARE[status])} />
  );
}

/** The cells a pool transaction consumes: a cellbase none (the API counts its one null input). */
function poolConsumed(tx: PoolRow): number {
  return tx.activity?.isCellbase ? 0 : tx.inputsCount;
}

/** A pool transaction's stone: its hash and its cells; not in a block yet, so pressed unpressed. */
function poolSealId(tx: PoolRow): SealId {
  return { kind: 'tx', hash: tx.hash, consumed: poolConsumed(tx), created: tx.outputsCount };
}

/**
 * The hash in the pool table's Transaction column: "0x" and 10 digits, the ellipsis, 6 digits (21
 * characters at 13 px, 163.8 px), as before the transaction's row stone (old issue C). The phone
 * row's identity line, which shares its line with the status and the wait, keeps 10 + 6.
 */
const POOL_HASH_CHARS = { start: 12, end: 6 } as const;

/**
 * A pool transaction as a phone row (no board; built from board L2's rules): the hash with its
 * status and wait, then what it does and its value, then fee, rate, cells, size and cycles quiet.
 */
function PoolPhoneRow({ tx, wide }: { tx: PoolRow; wide: boolean }) {
  const what = (
    <>
      {tx.activity ? (
        <span className="text-text-bright">{activityHeadline(tx.activity)}</span>
      ) : (
        <span className="text-text-dim">Interpretation unavailable</span>
      )}
      <PoolInterpretationNotice
        interpretation={tx.interpretation}
        className="text-warning text-meta ml-2"
      />
    </>
  );
  const value =
    tx.value === null ? (
      <span className="text-text-dim">Interpretation partial</span>
    ) : (
      <ExactCkb
        shannons={parseShannons(tx.value, `pool transaction ${tx.hash} value`)}
        signed={false}
        tone="plain"
      />
    );
  const fee = (
    <span className="whitespace-nowrap">
      fee{' '}
      <ExactCkb
        shannons={parseShannons(tx.fee, `pool transaction ${tx.hash} fee`)}
        signed={false}
        tone="plain"
      />
    </span>
  );
  const figures = [
    <span key="rate" className="whitespace-nowrap">
      {BigInt(tx.feeRate).toLocaleString('en-US')} shannon/KB
    </span>,
    <span key="cells" className="whitespace-nowrap">
      {poolConsumed(tx)} → {tx.outputsCount} cells
    </span>,
    <span key="size" className="whitespace-nowrap">
      {tx.txSize.toLocaleString('en-US')} B
    </span>,
    <span key="cycles" className="whitespace-nowrap">
      {BigInt(tx.cycles).toLocaleString('en-US')} cycles
    </span>,
  ];
  return (
    <PhoneRow
      testId="pool-row-mobile"
      mark={<RowSeal id={poolSealId(tx)} state="unpressed" />}
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <Link href={`/tx/${tx.hash}`} className="hover:underline">
              <Hash
                hash={tx.hash}
                startChars={10}
                endChars={6}
                copyable={false}
                className="text-body"
              />
            </Link>
          ),
          right: (
            <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
              <PoolDot status={tx.status} />
              <span className={tx.status === 'proposed' ? 'text-gold' : 'text-text'}>
                {poolStatusLabel(tx.status)}
              </span>
              <span className="text-text-dim">
                · {formatTimeAgo(new Date(Number(tx.timestamp)))}
              </span>
            </span>
          ),
        },
        wide
          ? {
              key: 'what',
              tone: 'body',
              left: <Dotted parts={[what, value, fee]} />,
              right: (
                <span className="text-text-dim text-meta">
                  <Dotted parts={figures} />
                </span>
              ),
            }
          : { key: 'what', tone: 'body', left: what, right: value },
        !wide && {
          key: 'figures',
          tone: 'quiet',
          left: <span className="text-text">{fee}</span>,
          right: <Dotted parts={figures} />,
        },
      ]}
    />
  );
}

export default function TransactionsPage() {
  const searchParams = useSearchParams();
  const pathname = usePathname();
  const router = useRouter();
  const inRouter = useInRouterContext();
  const urlKind = kindFromUrl(searchParams.get('kind'));
  const [kind, setKind] = useState<TransactionKind>(urlKind);
  const activityType = kind === 'all' ? undefined : kind;
  const [tab, setTab] = useState<'committed' | 'pool'>('committed');
  const committedPage = useCursorPagination();
  const poolPage = useCursorPagination();
  const { reset: resetCommitted } = committedPage;
  const { reset: resetPool } = poolPage;
  // D11: the page follows its URL, also when a link or history entry changes it under this page.
  useEffect(() => {
    setKind(urlKind);
    resetCommitted();
    resetPool();
  }, [urlKind, resetCommitted, resetPool]);
  const limit = usePageSize(DEFAULT_PAGE_SIZE);
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
    // D11: the URL names the chosen kind (replace: a filter is not a step of history). Outside a
    // router there is no address to keep in step, and leaving the page would reload it.
    if (inRouter) {
      const query = new URLSearchParams(searchParams);
      if (value === 'all') query.delete('kind');
      else query.set('kind', value);
      router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
    }
  };
  const poolColumns: ListColumn<PoolRow>[] = [
    {
      key: 'tx',
      label: 'Transaction',
      // The transaction's row stone (ROW_SEAL.tx.footprint, 56.5 px), the hash's 21 characters at
      // 13 px (163.8 px) and the 16 px gap: 236.3 of its 237 px. The 30 px it gained over 207 are
      // the widest container's own (1,232 px from 1280; the columns took 1,200 of it), so no
      // other column moves; the table is 1,230 px, the committed table's width, and both of the
      // page's lists change form at the same width (old issue C: the stone had taken the 30 from
      // the hash, 12 + 6 → 10 + 4).
      minWidth: 237,
      render: (tx) => (
        <div className="flex items-start">
          <RowSeal id={poolSealId(tx)} state="unpressed" overhang />
          <div className="min-w-0 space-y-1">
            {/* Table-rows T5: the pool row's key, 13 px bright on an 18 px line. */}
            <Link href={`/tx/${tx.hash}`}>
              <Hash
                hash={tx.hash}
                startChars={POOL_HASH_CHARS.start}
                endChars={POOL_HASH_CHARS.end}
                copyable={false}
                className="text-body text-text-bright leading-[18px]"
              />
            </Link>
            <PoolInterpretationNotice interpretation={tx.interpretation} />
          </div>
        </div>
      ),
    },
    {
      key: 'activity',
      label: 'What it does',
      minWidth: 180,
      render: (tx) =>
        tx.activity ? (
          activityHeadline(tx.activity)
        ) : (
          <span className="text-text-dim">Interpretation unavailable</span>
        ),
    },
    {
      key: 'status',
      label: 'Status · waiting',
      minWidth: 150,
      render: (tx) => (
        <span className="text-small flex items-center gap-1.5">
          <PoolDot status={tx.status} />
          <span className={tx.status === 'proposed' ? 'text-gold' : 'text-text'}>
            {poolStatusLabel(tx.status)}
          </span>
          <span className="text-text-dim">· {formatTimeAgo(new Date(Number(tx.timestamp)))}</span>
        </span>
      ),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 64,
      numeric: true,
      render: (tx) => `${poolConsumed(tx)} → ${tx.outputsCount}`,
    },
    {
      key: 'value',
      label: 'Value',
      minWidth: 164,
      numeric: true,
      render: (tx) =>
        tx.value === null ? (
          <span className="text-text-dim whitespace-normal">Interpretation partial</span>
        ) : (
          <ExactCkb
            shannons={parseShannons(tx.value, `pool transaction ${tx.hash} value`)}
            signed={false}
            tone="plain"
            wrap
          />
        ),
    },
    {
      key: 'fee',
      label: 'Fee',
      minWidth: 128,
      numeric: true,
      render: (tx) => (
        <ExactCkb
          shannons={parseShannons(tx.fee, `pool transaction ${tx.hash} fee`)}
          signed={false}
          tone="plain"
          wrap
        />
      ),
    },
    {
      key: 'feeRate',
      label: 'Fee rate, shannon/KB',
      // The head on its two lines ("shannon/KB", 10 characters at 11 px, 66 px) or a rate of
      // 1,000,000,000 (13 at 12 px, 93.6 px), and the 16 px gap; 7 px went to the row seal.
      minWidth: 123,
      numeric: true,
      render: (tx) => BigInt(tx.feeRate).toLocaleString('en-US'),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      minWidth: 90,
      numeric: true,
      render: (tx) => tx.txSize.toLocaleString('en-US'),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      // The most a transaction may take, "3,500,000,000" (13 characters at 12 px, 93.6 px), and no
      // gap (the last column); 16 px went to the row seal.
      minWidth: 94,
      numeric: true,
      render: (tx) => BigInt(tx.cycles).toLocaleString('en-US'),
    },
  ];
  const active = tab === 'committed' ? committed : pool;
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        <DetailLayout
          title="Transactions"
          kind="index"
          actions={
            <Link href="/activities" className="text-text-dim hover:text-jade text-small">
              Activities →
            </Link>
          }
        >
          <section className="space-y-4" aria-label="Yesterday and the pool">
            <p className="text-text text-body">
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
              <p role="alert" className="text-text-dim text-small">
                Could not read yesterday’s statistics: {(dailyError ?? txStatsError)!.message}
              </p>
            )}
            {daily && !day && (
              <p className="text-text-dim text-small">
                No complete activity statistics for {yesterday} (UTC+8).
              </p>
            )}
            {txStats && total === undefined && (
              <p className="text-text-dim text-small">
                No complete transaction count for {yesterday} (UTC+8).
              </p>
            )}
            {/* Board L2: below sm the kind chips and the pool banner fold into one control line
                — the kinds, each with yesterday's count, as one select, and the pool as a chip
                that opens it — so the first transaction is on the first screen. */}
            <div data-testid="kind-pool-line" className="flex items-center gap-2 sm:hidden">
              <label htmlFor="transactions-kind" className="sr-only">
                Filter by kind, with yesterday's counts
              </label>
              <select
                id="transactions-kind"
                value={kind}
                onChange={(e) => chooseKind(e.target.value as TransactionKind)}
                className="border-base-border bg-base-surface text-text-bright text-small h-8 min-w-0 rounded border px-2"
              >
                {KINDS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.value === 'all' ? 'All kinds' : option.label}
                    {counts[option.value] !== undefined &&
                      ` ${counts[option.value]!.toLocaleString('en-US')}`}
                  </option>
                ))}
              </select>
              {poolHead.data && (
                <button
                  type="button"
                  aria-pressed={tab === 'pool'}
                  onClick={() => setTab(tab === 'pool' ? 'committed' : 'pool')}
                  className={cn(
                    'text-small flex h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded border px-2.5',
                    tab === 'pool'
                      ? 'border-base-border bg-base-surface text-text-bright'
                      : 'border-base-border-subtle text-text'
                  )}
                >
                  <span aria-hidden="true" className="bg-info h-1.5 w-1.5 rounded-full" />
                  pool · {poolHead.data.totals.pending.toLocaleString('en-US')} pending
                </button>
              )}
            </div>
            <div
              role="group"
              aria-label="Filter by kind, with yesterday's counts"
              className="hidden flex-wrap gap-2 sm:flex"
            >
              {KINDS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-label={option.label}
                  aria-pressed={kind === option.value}
                  onClick={() => chooseKind(option.value)}
                  className={cn(
                    'text-small flex min-h-[34px] items-center gap-2 rounded border px-3',
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
                    <span className="text-text-dim text-meta">
                      {counts[option.value]!.toLocaleString('en-US')}
                    </span>
                  )}
                </button>
              ))}
            </div>
            {day && (
              <p className="text-text-dim text-meta">
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
              <p role="alert" className="text-text-dim text-small">
                Could not read the pool: {poolHead.error.message}
              </p>
            ) : poolHead.data ? (
              <div
                data-testid="pool-banner"
                className="text-small hidden flex-wrap items-center gap-x-4 gap-y-2 sm:flex"
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
                      data-pool-status={tx.status}
                      className={cn('block', POOL_SQUARE[tx.status])}
                      style={{
                        width: 18 * Math.sqrt(tx.txSize / largestPoolSize!),
                        height: 18 * Math.sqrt(tx.txSize / largestPoolSize!),
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
                    // Lane P: on the UTC+8 clock; how long ago is its hover.
                    <>
                      {' '}
                      · newest seen{' '}
                      <time title={formatTimeAgo(poolHead.data.summary.newestSeenAt)}>
                        {formatUtc8DateTime(poolHead.data.summary.newestSeenAt)}
                      </time>
                    </>
                  )}
                </span>
                <button
                  type="button"
                  onClick={() => setTab('pool')}
                  className="border-base-border text-text hover:text-text-bright rounded border px-3 py-2"
                >
                  Open the pool →
                </button>
              </div>
            ) : (
              <p className="text-text-dim text-small">Reading the pool…</p>
            )}
          </section>
          <div className="border-base-border-subtle flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-b">
            <div
              role="group"
              aria-label="Transaction views"
              className="hidden flex-wrap gap-6 sm:flex"
            >
              {(['committed', 'pool'] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={tab === value}
                  onClick={() => setTab(value)}
                  className={cn(
                    'text-section -mb-px flex min-h-11 items-center gap-2 border-b-2 font-medium',
                    tab === value
                      ? 'border-jade text-text-bright'
                      : 'text-text-dim border-transparent'
                  )}
                >
                  {value === 'committed' ? 'Committed' : 'In the pool'}
                  {value === 'pool' && poolHead.data && (
                    <span className="text-text-dim text-small font-normal">
                      {(
                        poolHead.data.totals.pending + poolHead.data.totals.proposed
                      ).toLocaleString('en-US')}
                    </span>
                  )}
                </button>
              ))}
            </div>
            <span className="text-text-dim text-small">
              {tab === 'committed'
                ? 'newest first · cellbase rows set back'
                : 'not yet in a block · newest first'}
            </span>
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
              <TransactionListTable transactions={committed.data.data} />
              <p className="text-text-dim text-small">
                Value is what the participants received. A cellbase row shows, grey, the reward it
                pays; no total counts it.
              </p>
              <CursorPagination
                className="flex-col items-start gap-3 sm:flex-row sm:items-center"
                total={committed.data.total ?? undefined}
                totalLabel="transactions"
                pageSize={limit}
                page={committedPage.page}
                currentCount={committed.data.data.length}
                hasMore={committed.data.hasMore}
                hasPrevious={committedPage.hasPrevious}
                onNext={() => committedPage.goToNext(committed.data.nextCursor)}
                onPrevious={committedPage.goToPrevious}
                previousLabel="Newer"
                nextLabel="Older →"
              />
            </>
          )}
          {tab === 'pool' && pool.data && (
            <>
              <dl className="text-small flex flex-wrap gap-x-9 gap-y-2.5" aria-label="Pool summary">
                <div>
                  <dt className="text-text-dim">Pending · proposed</dt>
                  <dd className="text-text-bright text-body mt-0.5">
                    {pool.data.totals.pending.toLocaleString('en-US')} ·{' '}
                    {pool.data.totals.proposed.toLocaleString('en-US')}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim">Total size</dt>
                  <dd className="text-text-bright text-body mt-0.5">
                    {BigInt(pool.data.summary.size).toLocaleString('en-US')} B
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim">Total cycles</dt>
                  <dd className="text-text-bright text-body mt-0.5">
                    {BigInt(pool.data.summary.cycles).toLocaleString('en-US')}
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim">Fee rate, shannon/KB</dt>
                  <dd className="text-text-bright text-body mt-0.5">
                    {pool.data.summary.minFeeRate === null || pool.data.summary.maxFeeRate === null
                      ? 'No fee rates'
                      : `${BigInt(pool.data.summary.minFeeRate).toLocaleString('en-US')} – ${BigInt(pool.data.summary.maxFeeRate).toLocaleString('en-US')}`}
                  </dd>
                </div>
              </dl>
              <ListTable
                label="Transactions in the pool"
                columns={poolColumns}
                rows={pool.data.data}
                rowKey={(tx) => tx.hash}
                empty="The node's pool is empty."
                mobileRow={(tx, wide) => <PoolPhoneRow tx={tx} wide={wide} />}
              />
              <p className="text-text-dim text-small">
                Newest first, {limit} a page. A proposed transaction enters a block 2–10 blocks
                after its proposal.
              </p>
              {pool.data.truncated && (
                <p className="text-text-dim text-small">
                  This node's tracked pool is truncated; more transactions may be present.
                </p>
              )}
              <CursorPagination
                className="flex-col items-start gap-3 sm:flex-row sm:items-center"
                total={pool.data.total}
                totalLabel="pool transactions"
                pageSize={limit}
                page={poolPage.page}
                currentCount={pool.data.data.length}
                hasMore={pool.data.nextCursor !== null}
                hasPrevious={poolPage.hasPrevious}
                onNext={() => poolPage.goToNext(pool.data.nextCursor)}
                onPrevious={poolPage.goToPrevious}
                previousLabel="Newer"
                nextLabel="Older →"
              />
            </>
          )}
        </DetailLayout>
        <TopicCharts topic="/transactions" />
      </main>
    </div>
  );
}
