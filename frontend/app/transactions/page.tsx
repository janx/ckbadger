'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useEffect, useState, type ReactNode } from 'react';
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
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { activityHeadline } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import {
  TRANSACTION_LIST_MIN_PX,
  TransactionListTable,
} from '@/components/transaction/transaction-list-table';
import { PoolInterpretationNotice, poolStatusLabel } from '@/components/ui/pool-status';
import type { PoolStatus } from '@/lib/api';
import type { SealId } from '@/lib/seal/stones';
import { FilterChips } from '@/components/ui/filter-chips';
import { Pager } from '@/components/ui/pager';
import { Tabs } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { DEFAULT_PAGE_SIZE, timePageAt } from '@/lib/pagination';
import { KIND_FILTER_LABEL, kindColor, kindFilterValues, kindWords } from '@/lib/kind-filter';
import { usePageSize } from '@/hooks/usePageSize';
import { parseShannons } from '@/lib/ckb-decimal';
import { cn } from '@/lib/utils';
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
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
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
/**
 * The list's kinds, in the board's order: the kind filter's values (lib/kind-filter.ts, the words
 * and marks the activities stream's filter shares, PA-15).
 */
const TRANSACTION_KINDS: readonly TransactionKind[] = [
  'all',
  'ckb',
  'script',
  'cellbase',
  'dao',
  'token',
  'object',
  'identity',
];
/**
 * The kind a URL names (`/transactions?kind=dao`, the DAO page's "DAO transactions →"): a known
 * kind, else All (hierarchy D11).
 */
function kindFromUrl(raw: string | null): TransactionKind {
  const known = TRANSACTION_KINDS.find((kind) => kind === raw);
  return known ?? 'all';
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

/** The pool transactions' statuses, each a word in the Status column. */
const POOL_STATUSES: readonly PoolStatus[] = ['pending', 'proposed', 'committed_awaiting_index'];
/** Their words, as the Status column and the phone row write them. */
const POOL_STATUS_WORDS = POOL_STATUSES.map(poolStatusLabel);

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
 * Line 1 of a pool transaction's phone row (the phone-rows board's head(), as the committed list's):
 * the unpressed stone and its gap (ROW_SEAL.tx.footprint, 40.3), the tight hash at the key's 14 px
 * (92.4), the 12 px between the parts and, at 13 px, the status and its wait as the Status column
 * holds them: the 7 px dot, 6 px, the widest status ("Confirming", 78), 6 px and the longest wait
 * under 100 days ("· 99d ago", 70.2): 311.9 of a 360 px phone's 328 px line.
 */
const POOL_PHONE_HEAD_PX = phoneHead(
  'pool transactions',
  ROW_SEAL.tx.footprint +
    textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('body')) +
    PHONE_PART_GAP_PX +
    7 +
    6 +
    textPx(Math.max(...POOL_STATUS_WORDS.map((w) => w.length)), fontPx('small')) +
    6 +
    textPx('· 99d ago'.length, fontPx('small'))
);

/**
 * A pool transaction as a phone row (the phone-rows board, as the committed list's row): the hash
 * with its status and wait; what it does; then, quiet, the cells, the value, the fee and its rate,
 * each figure after its name. Bytes and cycles are on the transaction's page and in the table; the
 * cards from 560 px (`wide`) keep them at the end of the quiet line.
 */
function PoolPhoneRow({ tx, wide }: { tx: PoolRow; wide: boolean }) {
  const what = (
    <>
      {tx.activity ? (
        activityHeadline(tx.activity)
      ) : (
        <span className="text-text-dim">Interpretation unavailable</span>
      )}
      <PoolInterpretationNotice
        interpretation={tx.interpretation}
        className="text-warning text-meta ml-2"
      />
    </>
  );
  const figures: ReactNode[] = [
    <span key="cells" className="whitespace-nowrap">
      {poolConsumed(tx)} → {formatCount(tx.outputsCount, 'cell')}
    </span>,
    tx.value === null ? (
      <span key="value" className="text-text-dim">
        Interpretation partial
      </span>
    ) : (
      <Labelled key="value" label="value">
        <CkbAmount
          form="summary"
          shannons={parseShannons(tx.value, `pool transaction ${tx.hash} value`)}
          signed={false}
          tone="plain"
        />
      </Labelled>
    ),
    <Labelled key="fee" label="fee">
      <CkbAmount
        form="summary"
        shannons={parseShannons(tx.fee, `pool transaction ${tx.hash} fee`)}
        signed={false}
        tone="plain"
      />
    </Labelled>,
    <Labelled key="rate" label="fee rate">
      {BigInt(tx.feeRate).toLocaleString('en-US')} shannon/KB
    </Labelled>,
    ...(wide
      ? [
          <span key="size" className="whitespace-nowrap">
            {tx.txSize.toLocaleString('en-US')} B
          </span>,
          <span key="cycles" className="whitespace-nowrap">
            {BigInt(tx.cycles).toLocaleString('en-US')} cycles
          </span>,
        ]
      : []),
  ];
  return (
    <PhoneRow
      testId="pool-row-mobile"
      head={POOL_PHONE_HEAD_PX}
      mark={<RowSeal id={poolSealId(tx)} state="unpressed" />}
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <Link href={`/tx/${tx.hash}`} look="key">
              <Hash hash={tx.hash} size="tight" copyable={false} className="text-body" />
            </Link>
          ),
          // the dot, the status word and the wait stand in one row; "this device's clock is
          // behind" in the wait's place is words and breaks between them in its item (Utc8Age; L2)
          right: (
            <span className="inline-flex items-center gap-1.5">
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
        { key: 'what', tone: 'body', left: what },
        { key: 'figures', tone: 'quiet', left: <Dotted parts={figures} /> },
      ]}
    />
  );
}

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
      // The transaction's row stone (ROW_SEAL.tx.footprint, 40.3 px), the hash at the row size, 17
      // characters at the key's 14 px (142.8), and the gap: 199.1. Its 31 px spare, kept so the
      // two lists changed form at one width, went (readability F3-e): What it does keeps that.
      minWidth: columnPx(
        ROW_SEAL.tx.footprint +
          textPx(shortChars(SHORT_FORMS.row.hash), fontPx('body')) +
          CELL_GAP_PX
      ),
      render: (tx) => (
        <div className="flex items-start">
          {/* the stone stands centred on the key's line (RowSeal) */}
          <RowSeal id={poolSealId(tx)} state="unpressed" />
          <div className="min-w-0 space-y-1">
            {/* Table-rows T5: the pool row's key, bright on the 20 px row line. */}
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
      // (none today: the pool's 1,212 px is the larger, the committed list's 1,211). It was 180
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
          textPx(Math.max(...POOL_STATUS_WORDS.map((w) => w.length)), fontPx('small')) +
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
 * sums, the pool's 1,212 px today (the committed list's own is 1,211). Each list's flexible column
 * keeps what that width is over its own sum: the pool's What it does none, the committed list's
 * What it did 1 px (its `formWidth`). Exported as the one source of that width (review rules-tests
 * NOTE-1): the tests draw both lists at it and compare it with the pool's budgets.
 */
export const TRANSACTIONS_FORM_PX = Math.max(TRANSACTION_LIST_MIN_PX, POOL_MIN_PX);

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
  const limit = usePageSize(DEFAULT_PAGE_SIZE);
  // a change of page size at md starts each walk again (lane c2-finish, 1d)
  const committedPage = useCursorPagination(limit);
  const poolPage = useCursorPagination(limit);
  const { reset: resetCommitted } = committedPage;
  const { reset: resetPool } = poolPage;
  // D11: the page follows its URL, also when a link or history entry changes it under this page.
  useEffect(() => {
    setKind(urlKind);
    resetCommitted();
    resetPool();
  }, [urlKind, resetCommitted, resetPool]);
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
  // where the committed list stands in time (lib/pagination.ts): its page, and the pages its
  // served total makes where it states one (a kind's list states none); nothing for one page
  const committedAt = committed.data
    ? timePageAt('committed transactions', committedPage.page, committed.data)
    : null;
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
            {/* The controls board's kind filter, with yesterday's counts (PA-15: the activities
                stream's, the same control; on a phone a select after "kind"). */}
            <FilterChips
              label={KIND_FILTER_LABEL}
              values={kindFilterValues(TRANSACTION_KINDS, counts)}
              value={kind}
              onChange={chooseKind}
            />
            {day && (
              <p className="text-text-dim text-meta max-w-measure">
                Kind occurrences may overlap; DAO counts actions. “all” counts unique transactions.
              </p>
            )}
            {occurrences > 0 && (
              <div
                role="img"
                aria-label={`Yesterday's kind occurrences; categories overlap`}
                className="flex h-2 overflow-hidden"
              >
                {TRANSACTION_KINDS.filter(
                  (k) => k !== 'all' && counts[k] !== undefined && counts[k]! > 0
                ).map((k) => (
                  <span
                    key={k}
                    title={`${kindWords(k)}: ${counts[k]}`}
                    style={{
                      width: `${(counts[k]! / occurrences) * 100}%`,
                      backgroundColor: kindColor(k),
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
                className="text-small flex flex-wrap items-center gap-x-4 gap-y-2"
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
                {/* its way into the pool from sm; below it the views' tab row is that way */}
                <button
                  type="button"
                  onClick={() => setTab('pool')}
                  className="border-base-border text-text hover:text-text-bright hidden border px-3 py-2 sm:inline-flex"
                >
                  Open the pool →
                </button>
              </div>
            ) : (
              <p className="text-text-dim text-small">Reading the pool…</p>
            )}
          </section>
          {/* The list's command line (the controls board's tab row): committed or in the pool. */}
          <Tabs
            label="Transaction views"
            command
            tabs={[
              { key: 'committed', label: 'committed' },
              {
                key: 'pool',
                label: 'in the pool',
                count: poolHead.data
                  ? poolHead.data.totals.pending + poolHead.data.totals.proposed
                  : undefined,
              },
            ]}
            value={tab}
            onChange={setTab}
          >
            <div className="mt-3 space-y-8">
              <div className="text-text-dim text-small">
                {tab === 'committed'
                  ? 'newest first · cellbase rows set back'
                  : 'not yet in a block · newest first'}
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
                    Value is what the participants received. A cellbase row shows, grey, the reward
                    it pays; no total counts it.
                  </p>
                  <TransactionListTable
                    transactions={committed.data.data}
                    formWidth={TRANSACTIONS_FORM_PX}
                  />
                  {/* The controls board's cursor in time. */}
                  {committedAt && (
                    <Pager
                      mode="time"
                      label="committed transactions"
                      order="newest first"
                      {...committedAt}
                      onNewer={committedPage.hasPrevious ? committedPage.goToPrevious : undefined}
                      onOlder={
                        committed.data.hasMore
                          ? () => committedPage.goToNext(committed.data.nextCursor)
                          : undefined
                      }
                    />
                  )}
                </>
              )}
              {tab === 'pool' && pool.data && (
                <>
                  <dl
                    className="text-small flex flex-wrap gap-x-9 gap-y-2.5"
                    aria-label="Pool summary"
                  >
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
                        {pool.data.summary.minFeeRate === null ||
                        pool.data.summary.maxFeeRate === null
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
                  {/* The controls board's cursor in time, without a count of pages: the pool moves
                      under the reader (every five seconds), so its pages are no fixed number. */}
                  <Pager
                    mode="time"
                    label="pool transactions"
                    order="newest first"
                    page={poolPage.page}
                    onNewer={poolPage.hasPrevious ? poolPage.goToPrevious : undefined}
                    onOlder={
                      pool.data.nextCursor !== null
                        ? () => poolPage.goToNext(pool.data.nextCursor)
                        : undefined
                    }
                  />
                </>
              )}
            </div>
          </Tabs>
        </DetailLayout>
        <TopicCharts topic="/transactions" />
      </main>
    </div>
  );
}
