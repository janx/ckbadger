'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useEffect, useState, type ReactNode } from 'react';
import { useInRouterContext } from 'react-router-dom';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { api, dayUserTransactions } from '@/lib/api';
import { DetailLayout } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable, minimumWidth, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { RowSeal } from '@/components/ui/row-seal';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { activityHeadline } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import {
  CELLBASE_LIST_MIN_PX,
  CellbaseListTable,
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
  previousChainDay,
} from '@/lib/activity-stats';
import { countNoun, formatCount } from '@/lib/format-count';
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

type TransactionKind = 'all' | 'ckb' | 'script' | 'dao' | 'token' | 'object' | 'identity';
/**
 * The user transactions' kinds, in the board's order: the kind filter's values (lib/kind-filter.ts,
 * the words and marks the activities stream's filter shares, PA-15). "all" is every user
 * transaction; the cellbases are a tab of their own (lists lane P3, plan §7.6), not a kind.
 */
const TRANSACTION_KINDS: readonly TransactionKind[] = [
  'all',
  'ckb',
  'script',
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
              <Hash hash={tx.hash} size="tight" variant="inherit" className="text-body" />
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
            {/* Table-rows T5: the pool row's key, the key look's bright, on the 20 px row line. */}
            <Link href={`/tx/${tx.hash}`} look="key">
              <Hash hash={tx.hash} size="row" variant="inherit" className="text-body leading-row" />
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
 * The width the page's three lists change form at (they are one page): the largest of their sums,
 * the pool's 1,212 px (the user transactions' own is 898, the cellbases' 804). Each list's flexible
 * column keeps what that width is over its own sum: the pool's What it does none, the user
 * transactions' What it did 314 px, the cellbases' Pays 408 (their `formWidth`). Exported as the
 * one source of that width (review rules-tests NOTE-1): the tests draw the lists at it and compare
 * it with the pool's budgets.
 */
export const TRANSACTIONS_FORM_PX = Math.max(
  TRANSACTION_LIST_MIN_PX,
  CELLBASE_LIST_MIN_PX,
  POOL_MIN_PX
);

/** The pool list's columns as drawn, changing form at TRANSACTIONS_FORM_PX. */
export const POOL_COLUMNS = poolColumns(TRANSACTIONS_FORM_PX - POOL_MIN_PX);

export default function TransactionsPage() {
  const searchParams = useSearchParams();
  const pathname = usePathname();
  const router = useRouter();
  const inRouter = useInRouterContext();
  const urlKind = kindFromUrl(searchParams.get('kind'));
  const [kind, setKind] = useState<TransactionKind>(urlKind);
  // the pool's kind (no pool transaction is a cellbase: "all" asks for every one)
  const activityType = kind === 'all' ? undefined : kind;
  // the page opens on the user transactions (plan §7.6): every committed one but the cellbase
  const committedType = kind === 'all' ? ('user' as const) : kind;
  const [tab, setTab] = useState<'committed' | 'cellbases' | 'pool'>('committed');
  const limit = usePageSize(DEFAULT_PAGE_SIZE);
  // a change of page size at md starts each walk again (lane c2-finish, 1d)
  // and so does a change of the list it pages (lane cd-fixes-controls): the kind
  const committedPage = useCursorPagination(limit, { list: [kind] });
  const cellbasePage = useCursorPagination(limit);
  const poolPage = useCursorPagination(limit, { list: [kind] });
  // D11: the page follows its URL, also when a link or history entry changes it under this page.
  useEffect(() => setKind(urlKind), [urlKind]);
  const committed = useQuery({
    queryKey: ['transactions', committedPage.cursor, limit, committedType],
    queryFn: committedPage.ask((cursor: string | undefined) =>
      api.getTransactions({ cursor, limit, activityType: committedType })
    ),
    staleTime: 30_000,
  });
  const cellbases = useQuery({
    queryKey: ['transactions', cellbasePage.cursor, limit, 'cellbase'],
    queryFn: cellbasePage.ask((cursor: string | undefined) =>
      api.getTransactions({ cursor, limit, activityType: 'cellbase' })
    ),
    enabled: tab === 'cellbases',
    staleTime: 30_000,
  });
  const pool = useQuery({
    // asked by its walk (lane cd-fixes-controls, item 1), so its cache entry is its own beside the
    // head's first page
    queryKey: ['pool-transactions', poolPage.cursor, limit, activityType, 'page'],
    queryFn: poolPage.ask((cursor: string | undefined) =>
      api.getPoolTransactions({
        cursor,
        limit,
        ...(activityType ? { activityType } : {}),
      })
    ),
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
  const yesterday = previousChainDay();
  const day = daily ? completedYesterdayActivities(daily) : undefined;
  // yesterday's user transactions: the route's count, the day's committed transactions less its
  // cellbases (the activities page states the same figure; no page subtracts)
  const users = day === undefined ? undefined : dayUserTransactions(day);
  const counts: Partial<Record<TransactionKind, number>> = day
    ? activityFilterCounts(day, users)
    : {};
  const occurrences = day ? day.kindOccurrences : 0;
  const largestPoolSize = poolHead.data?.data.length
    ? Math.max(...poolHead.data.data.map((row) => row.txSize))
    : undefined;
  const setKindAndUrl = (value: TransactionKind) => {
    setKind(value);
    // D11: the URL names the chosen kind (replace: a filter is not a step of history). Outside a
    // router there is no address to keep in step, and leaving the page would reload it.
    if (inRouter) {
      const query = new URLSearchParams(searchParams);
      if (value === 'all') query.delete('kind');
      else query.set('kind', value);
      router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
    }
  };
  const chooseKind = (value: TransactionKind) => {
    setKindAndUrl(value);
    // a kind is of the user transactions: chosen over the cellbases, it shows them
    if (tab === 'cellbases') setTab('committed');
  };
  // no kind filters the cellbases (no cellbase is a DAO or a token transaction): opening them sets
  // the kind back to "all", in the URL too, so no chip stands pressed over a list it does not
  // filter (the page-phase review's SHOULD 6)
  const chooseTab = (value: 'committed' | 'cellbases' | 'pool') => {
    if (value === 'cellbases' && kind !== 'all') setKindAndUrl('all');
    setTab(value);
  };
  const active = tab === 'committed' ? committed : tab === 'cellbases' ? cellbases : pool;
  // where the committed list stands in time (lib/pagination.ts): its page, and the pages its
  // served total makes where it states one (a kind's list states none); nothing for one page
  const committedAt = committed.data
    ? timePageAt('committed transactions', committed.data, committedPage)
    : null;
  const cellbasesAt = cellbases.data ? timePageAt('cellbases', cellbases.data, cellbasePage) : null;
  // the pool's pages, a list in time with no count of pages: the pool moves under the reader
  // (every five seconds), so its pages are no fixed number; its "more" is its next cursor
  const poolAt = pool.data
    ? timePageAt(
        'pool transactions',
        { ...pool.data, hasMore: pool.data.nextCursor !== null, total: null },
        poolPage
      )
    : null;
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        <DetailLayout
          title="Transactions"
          kind="index"
          actions={
            <Link href="/activities" look="onward" className="text-small">
              Activities
            </Link>
          }
        >
          <section className="space-y-4" aria-label="Yesterday and the pool">
            {/* What the page says first (lists lane P3): yesterday's transactions, the users' and the
                cellbases' apart, as the lists below are; what waits now is the pool's line. */}
            <p data-role="lede" className="text-text text-body max-w-measure">
              {users === undefined || !day ? (
                'Yesterday’s counts are unavailable.'
              ) : (
                <>
                  <span className="text-text-bright">{users.toLocaleString('en-US')}</span> user{' '}
                  {countNoun(users, 'transaction')} and{' '}
                  <span className="text-text-bright">
                    {day.coinbaseCount.toLocaleString('en-US')}
                  </span>{' '}
                  {countNoun(day.coinbaseCount, 'cellbase')} yesterday (UTC+8).
                </>
              )}
            </p>
            {dailyError && (
              <p role="alert" className="text-text text-small max-w-measure">
                Could not read yesterday’s statistics: {dailyError.message}
              </p>
            )}
            {daily && !day && (
              <p className="text-text-dim text-small max-w-measure">
                No complete activity statistics for {yesterday} (UTC+8).
              </p>
            )}
            {/* The controls board's kind filter, with yesterday's counts (PA-15: the activities
                stream's, the same control; on a phone a select after "kind"). */}
            <FilterChips
              label={KIND_FILTER_LABEL}
              values={kindFilterValues(TRANSACTION_KINDS, counts)}
              value={kind}
              onChange={chooseKind}
              counted
            />
            {day && (
              <p className="text-text-dim text-meta max-w-measure">
                Yesterday’s counts (UTC+8): kinds may overlap, DAO counts actions, “all” counts each
                user transaction once.
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
              <p role="alert" className="text-text text-small max-w-measure">
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
                      look="block"
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
                {/* its way into the pool from sm; below it the views' tab row is that way. It opens the
                    pool's tab below, on this page: "↓", not a link's "→" (the placement lane's
                    note) */}
                <button
                  type="button"
                  onClick={() => setTab('pool')}
                  className="border-base-border text-text hover:text-text-bright hidden border px-3 py-2 sm:inline-flex"
                >
                  Open the pool ↓
                </button>
              </div>
            ) : (
              <p className="text-text-dim text-small">Reading the pool…</p>
            )}
          </section>
          {/* The list's command line (the controls board's tab row): the user transactions, the
              cellbases, or the pool. The pool's counts are its line above, each said once (no
              tab sums pending and proposed in the page). */}
          <Tabs
            label="Transaction views"
            command
            tabs={[
              { key: 'committed', label: 'transactions' },
              { key: 'cellbases', label: 'cellbases' },
              { key: 'pool', label: 'in the pool' },
            ]}
            value={tab}
            onChange={chooseTab}
          >
            <div className="mt-3 space-y-8">
              <div className="text-text-dim text-small">
                {tab === 'committed'
                  ? 'committed · newest first'
                  : tab === 'cellbases'
                    ? 'one in every block · newest first'
                    : 'not yet in a block · newest first'}
              </div>
              {isAwaited(active) && (
                <p role="status" className="text-text-dim">
                  Loading transactions…
                </p>
              )}
              {active.error && (
                <p role="alert" className="text-text max-w-measure">
                  Unable to read transactions: {active.error.message}
                </p>
              )}
              {tab === 'committed' && committed.data && (
                <>
                  {/* B5: how to read the Value column, above the table, not after its last row; an
                      explanation, in the reading ink (R3) */}
                  <p className="text-text text-small max-w-measure">
                    Value is what the participants received.
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
                    />
                  )}
                </>
              )}
              {tab === 'cellbases' && cellbases.data && (
                <>
                  {/* how to read the list, above it: an explanation, in the reading ink (R3) */}
                  <p className="text-text text-small max-w-measure">
                    A block’s cellbase pays the miner of an earlier block its reward; block 0’s
                    issued the first CKB.
                  </p>
                  <CellbaseListTable
                    transactions={cellbases.data.data}
                    formWidth={TRANSACTIONS_FORM_PX}
                  />
                  {cellbasesAt && (
                    <Pager mode="time" label="cellbases" order="newest first" {...cellbasesAt} />
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
                  {/* B5: how to read the list, above it, not after its last row; an explanation,
                      in the reading ink (R3) */}
                  <p className="text-text text-small max-w-measure">
                    Newest first, {limit} a page. A proposed transaction enters a block 2–10 blocks
                    after its proposal.
                  </p>
                  {pool.data.truncated && (
                    <p className="text-text text-small">
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
                  {poolAt && (
                    <Pager mode="time" label="pool transactions" order="newest first" {...poolAt} />
                  )}
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
