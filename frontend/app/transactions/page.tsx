'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useEffect, useState } from 'react';
import { useInRouterContext } from 'react-router-dom';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { DetailLayout } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable, minimumWidth, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { RowSeal } from '@/components/ui/row-seal';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { activityHeadline } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import {
  TRANSACTION_LIST_MIN_PX,
  TransactionListTable,
} from '@/components/transaction/transaction-list-table';
import { PoolInterpretationNotice, poolStatusLabel } from '@/components/ui/pool-status';
import type { PoolStatus } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { parseShannons } from '@/lib/ckb-decimal';
import { cn } from '@/lib/utils';
import { ClassMark } from '@/components/ui/class-mark';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import {
  activityFilterCounts,
  completedYesterdayActivities,
  completedYesterdayTransactions,
  previousChainDay,
  totalActivities,
} from '@/lib/activity-stats';
import { formatCount } from '@/lib/format-count';
import { useAge, Utc8Age, Utc8DateTime } from '@/components/ui/utc8-date';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { SHORT_FORMS } from '@/lib/shorten';
import {
  MAX_BLOCK_BYTES,
  MAX_BLOCK_CYCLES,
  MAX_TX_INPUTS,
  WIDEST_COUNT_DIGITS,
  WIDEST_FEE_RATE,
  WIDEST_POOL_VALUE_LINE,
  WIDEST_TX_FEE,
  shortChars,
} from '@/lib/list-bounds';

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
 * When the newest pool transaction was seen, on the UTC+8 clock (lane P); how long ago is its
 * hover, read from the page's one clock, so it moves with no data change (lane E4).
 */
function NewestSeen({ at }: { at: string }) {
  const age = useAge(at);
  return <Utc8DateTime timestamp={at} title={age} />;
}

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
      <CkbAmount
        form="summary"
        shannons={parseShannons(tx.value, `pool transaction ${tx.hash} value`)}
        signed={false}
        tone="plain"
      />
    );
  const fee = (
    <span className="whitespace-nowrap">
      fee{' '}
      <CkbAmount
        form="summary"
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
      {poolConsumed(tx)} → {formatCount(tx.outputsCount, 'cell')}
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
              <Hash hash={tx.hash} size="tight" copyable={false} className="text-body" />
            </Link>
          ),
          right: (
            <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
              <PoolDot status={tx.status} />
              <span className={tx.status === 'proposed' ? 'text-gold' : 'text-text'}>
                {poolStatusLabel(tx.status)}
              </span>
              <span className="text-text-dim">
                · <Utc8Age timestamp={tx.timestamp} />
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

/** The pool transactions' statuses, each a word in the Status column. */
const POOL_STATUSES: readonly PoolStatus[] = ['pending', 'proposed', 'committed_awaiting_index'];

/**
 * The pool list's columns (readability F3-e: each budgeted from the widest value it holds).
 * `sameWidth` is what What it does keeps beyond its own minimum so the pool changes form where
 * the committed list does (both lists are one page).
 */
function poolColumns(sameWidth: number): ListColumn<PoolRow>[] {
  return [
    {
      key: 'tx',
      label: 'Transaction',
      // The transaction's row stone (ROW_SEAL.tx.footprint, 56.5 px), the hash at the row size, 17
      // characters at the key's 14 px (142.8), and the gap: 215.3. Its 31 px spare, kept so the
      // two lists changed form at one width, went (readability F3-e): What it does keeps that.
      minWidth: columnPx(
        ROW_SEAL.tx.footprint +
          textPx(shortChars(SHORT_FORMS.row.hash), fontPx('body')) +
          CELL_GAP_PX
      ),
      render: (tx) => (
        <div className="flex items-start">
          <RowSeal id={poolSealId(tx)} state="unpressed" overhang />
          <div className="min-w-0 space-y-1">
            {/* Table-rows T5: the pool row's key, 13 px bright on an 18 px line. */}
            <Link href={`/tx/${tx.hash}`}>
              <Hash
                hash={tx.hash}
                size="row"
                copyable={false}
                className="text-body text-text-bright leading-row"
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
      // The table's flexible column (readability F3-e): a headline, or "Interpretation
      // unavailable", breaks between its words, so at its minimum it holds the longest word,
      // "Interpretation" (14 characters at 13 px, 109.2), and the gap, and it takes what the
      // container gives beyond the sum. Both of the page's lists change form at one width, the
      // larger of their sums: `sameWidth` is what the committed list's sum is over the pool's
      // (none today: the pool's 1,228 px is the larger, the committed list's 1,227). It was 180
      // and named no content.
      minWidth:
        columnPx(textPx('Interpretation'.length, fontPx('small')) + CELL_GAP_PX) + sameWidth,
      width: 'auto',
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
      // Its widest content on one line: the 7 px dot, the 6 px gap, the widest status word
      // ("Confirming", 10 characters at 13 px, 78), the 6 px gap and the longest wait
      // formatTimeAgo writes for any age under 100 days ("· 59s ago" / "· 59m ago" / "· 23h ago" /
      // "· 99d ago", 9 characters, 70.2), and the gap: 183.2. At 150 it lived on the container's
      // spread slack and "● Pending · 26s ago" (150.2) broke when that went to the hash (old issue
      // C). A wait of 100 days or more wraps inside the cell before its age, which stays one unit;
      // "this device's clock is behind" is words (not a NoBreak, Utc8Age) and breaks between them in
      // the wait's flex item, which shrinks to the 71 px the dot and "Confirming" leave it.
      minWidth: columnPx(
        7 +
          6 +
          textPx(
            Math.max(...POOL_STATUSES.map((s) => poolStatusLabel(s).length)),
            fontPx('small')
          ) +
          6 +
          textPx('· 99d ago'.length, fontPx('small')) +
          CELL_GAP_PX
      ),
      render: (tx) => (
        <span className="text-small flex items-center gap-1.5">
          <PoolDot status={tx.status} />
          <span className={tx.status === 'proposed' ? 'text-gold' : 'text-text'}>
            {poolStatusLabel(tx.status)}
          </span>
          <span className="text-text-dim">
            · <Utc8Age timestamp={tx.timestamp} />
          </span>
        </span>
      ),
    },
    {
      key: 'cells',
      label: 'Cells',
      // The most inputs a transaction holds and four digits of outputs, as the pool writes them
      // (ungrouped), "13568 → 9999" (12 characters at 13 px, 93.6), and the gap (review fb-tests
      // 6: it counted four digits of inputs while 13,568 44-byte inputs fit a block). It was 64,
      // which held "10 → 2".
      minWidth: columnPx(
        textPx(`${MAX_TX_INPUTS} → ${WIDEST_COUNT_DIGITS}`.length, fontPx('small')) + CELL_GAP_PX
      ),
      numeric: true,
      render: (tx) => `${poolConsumed(tx)} → ${tx.outputsCount}`,
    },
    {
      key: 'value',
      label: 'Value',
      // A value under a million CKB on one line, "999,999.99999999" (16 characters at 13 px,
      // 124.8), its unit " CKB" at 12 px (28.8) and the gap; a larger one breaks before its
      // decimals (wrap), each of its lines narrower. It was 164, which broke one from 100,000 CKB.
      minWidth: columnPx(
        textPx(WIDEST_POOL_VALUE_LINE.length, fontPx('small')) +
          textPx(' CKB'.length, fontPx('meta')) +
          CELL_GAP_PX
      ),
      numeric: true,
      render: (tx) =>
        tx.value === null ? (
          <span className="text-text-dim whitespace-normal">Interpretation partial</span>
        ) : (
          <CkbAmount
            form="column"
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
      // A fee under 100 CKB, "99.99999999" (11 characters at 13 px, 85.8), its unit (28.8) and
      // the gap. It was 128, which broke a fee from 10 CKB.
      minWidth: columnPx(
        textPx(WIDEST_TX_FEE.length, fontPx('small')) +
          textPx(' CKB'.length, fontPx('meta')) +
          CELL_GAP_PX
      ),
      numeric: true,
      render: (tx) => (
        <CkbAmount
          form="column"
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
      // A rate up to 1,000,000,000 (13 characters at 13 px, 101.4), wider than the head's longest
      // word on its two lines ("shannon/KB", 10 at 12 px, 72), and the gap. Its 13 px spare went
      // (readability F3-e).
      minWidth: columnPx(
        Math.max(
          textPx(WIDEST_FEE_RATE.length, fontPx('small')),
          textPx('shannon/KB'.length, fontPx('meta'))
        ) + CELL_GAP_PX
      ),
      numeric: true,
      render: (tx) => BigInt(tx.feeRate).toLocaleString('en-US'),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      // The largest a pool transaction can be, a block's "597,000" (7 characters at 13 px, 54.6:
      // a node takes no transaction too large for a block, so genesis's cellbase is never in the
      // pool), and the gap; its 1 px spare went (readability F3-e). Its head takes two lines ("Size," /
      // "bytes"), as "Fee rate, shannon/KB" beside it already does, so the head row keeps its
      // height; the 22 px its one-line head held went to Status (old issue C).
      minWidth: columnPx(
        textPx(MAX_BLOCK_BYTES.toLocaleString('en-US').length, fontPx('small')) + CELL_GAP_PX
      ),
      numeric: true,
      render: (tx) => tx.txSize.toLocaleString('en-US'),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      // The most a transaction may take, "3,500,000,000" (13 characters at 13 px, 101.4), and no
      // gap (the last column); 16 px went to the row seal.
      minWidth: columnPx(textPx(MAX_BLOCK_CYCLES.toLocaleString('en-US').length, fontPx('small'))),
      numeric: true,
      render: (tx) => BigInt(tx.cycles).toLocaleString('en-US'),
    },
  ];
}

/** The pool list's columns at their own minimums: their sum is the pool's side of the one width. */
const POOL_MIN_PX = minimumWidth('Transactions in the pool', poolColumns(0));

/**
 * The width both of the page's lists change form at (they are one page): the larger of their
 * sums, the pool's 1,228 px today (the committed list's own is 1,227). Each list's flexible column
 * keeps what that width is over its own sum: the pool's What it does none, the committed list's
 * What it did 1 px (its `formWidth`).
 */
const TRANSACTIONS_FORM_PX = Math.max(TRANSACTION_LIST_MIN_PX, POOL_MIN_PX);

/** The pool list's columns as drawn, changing form at TRANSACTIONS_FORM_PX. */
export const POOL_COLUMNS = poolColumns(TRANSACTIONS_FORM_PX - POOL_MIN_PX);

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
            <p className="text-text text-body max-w-measure">
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
              <p role="alert" className="text-text-dim text-small max-w-measure">
                Could not read yesterday’s statistics: {(dailyError ?? txStatsError)!.message}
              </p>
            )}
            {daily && !day && (
              <p className="text-text-dim text-small max-w-measure">
                No complete activity statistics for {yesterday} (UTC+8).
              </p>
            )}
            {txStats && total === undefined && (
              <p className="text-text-dim text-small max-w-measure">
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
                className="border-base-border bg-base-surface text-text-bright text-small h-8 min-w-0 border px-2"
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
                    'text-small flex h-8 shrink-0 items-center gap-1.5 whitespace-nowrap border px-2.5',
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
                    'text-small flex min-h-[34px] items-center gap-2 border px-3',
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
                      className="h-2 w-2"
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
              <p className="text-text-dim text-meta max-w-measure">
                Kind occurrences may overlap; DAO counts actions. “All” counts unique transactions.
              </p>
            )}
            {occurrences > 0 && (
              <div
                role="img"
                aria-label={`Yesterday's kind occurrences; categories overlap`}
                className="flex h-2 overflow-hidden"
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
              <p role="alert" className="text-text-dim text-small max-w-measure">
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
                      title={`${tx.hash}: ${formatCount(tx.txSize, 'byte')}, ${tx.feeRate} shannon/KB`}
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
                      · newest seen <NewestSeen at={poolHead.data.summary.newestSeenAt} />
                    </>
                  )}
                </span>
                <button
                  type="button"
                  onClick={() => setTab('pool')}
                  className="border-base-border text-text hover:text-text-bright border px-3 py-2"
                >
                  Open the pool →
                </button>
              </div>
            ) : (
              <p className="text-text-dim text-small">Reading the pool…</p>
            )}
          </section>
          <div className="border-row-rule flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-b">
            <div
              role="group"
              aria-label="Transaction views"
              className="hidden flex-wrap gap-6 max-md:gap-x-4 sm:flex"
            >
              {(['committed', 'pool'] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={tab === value}
                  onClick={() => setTab(value)}
                  className={cn(
                    'text-section max-md:text-body -mb-px flex min-h-11 items-center gap-2 border-b-2 font-medium max-md:gap-1.5',
                    tab === value
                      ? 'border-jade text-text-bright'
                      : 'text-text-dim border-transparent'
                  )}
                >
                  {value === 'committed' ? 'Committed' : 'In the pool'}
                  {value === 'pool' && poolHead.data && (
                    <span className="text-text-dim text-small max-md:text-meta font-normal">
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
          {isAwaited(active) && (
            <p role="status" className="text-text-dim">
              Loading transactions…
            </p>
          )}
          {active.error && (
            <p role="alert" className="text-text-dim max-w-measure">
              Unable to read transactions: {active.error.message}
            </p>
          )}
          {tab === 'committed' && committed.data && (
            <>
              {/* B5: how to read the Value column, above the table, not after its last row. */}
              <p className="text-text-dim text-small max-w-measure">
                Value is what the participants received. A cellbase row shows, grey, the reward it
                pays; no total counts it.
              </p>
              <TransactionListTable
                transactions={committed.data.data}
                formWidth={TRANSACTIONS_FORM_PX}
              />
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
              {/* B5: how to read the list, above it, not after its last row. */}
              <p className="text-text-dim text-small max-w-measure">
                Newest first, {limit} a page. A proposed transaction enters a block 2–10 blocks
                after its proposal.
              </p>
              {pool.data.truncated && (
                <p className="text-text-dim text-small">
                  This node's tracked pool is truncated; more transactions may be present.
                </p>
              )}
              <ListTable
                label="Transactions in the pool"
                columns={POOL_COLUMNS}
                rows={pool.data.data}
                rowKey={(tx) => tx.hash}
                empty="The node's pool is empty."
                mobileRow={(tx, wide) => <PoolPhoneRow tx={tx} wide={wide} />}
              />
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
