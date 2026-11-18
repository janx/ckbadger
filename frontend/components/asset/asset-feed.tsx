'use client';
import { useEffect, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  CollectionFeedActivity,
  CollectionFeedPage,
  CursorPaginatedResponse,
  GlobalActivity,
  ItemActivity,
  Transaction,
} from '@/lib/api';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject, type FeedRow } from '@/components/activity/activity-runs';
import { ActivityReadError } from '@/components/activity-event-row';
import {
  collectionFeedHeadline,
  itemFeedHeadline as itemHeadlineFromContext,
  type ItemFeedHeadline,
  type ItemFeedSubject,
} from '@/lib/feed-headlines';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { NoBreak } from '@/components/ui/no-break';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import { ResourceBar, ScaleHeader, resourceScale } from '@/components/blocks/block-visual';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { ENTITY_PAGE_SIZE } from '@/lib/pagination';
import { AssetReadState } from '@/components/asset/detail-shell';
import { formatUtc8Date } from '@/lib/utils';
import { Utc8Age } from '@/components/ui/utc8-date';
import { formatBlockNumber } from '@/lib/format-block';
import { FeedHeadlineText } from '@/components/asset/feed-headline-text';
import type { AssetClass } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';
import { unitNode } from '@/components/ui/units';
import { tokenAmountText } from '@/lib/amount-form';
import { SHORT_FORMS } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { COUNT_TRACK_PX } from '@/components/ui/aligned-bar';
import {
  MAX_BLOCK_BYTES,
  TX_CYCLES_BOUND,
  WIDEST_AGE,
  WIDEST_BLOCK_NUMBER,
  WIDEST_COUNT_DIGITS,
  WIDEST_POOL_VALUE_LINE,
  WIDEST_TX_FEE,
  shortChars,
} from '@/lib/list-bounds';

/** A run of characters at the cells' 13 px (readability F3-e: every column budgets what it holds). */
const cell = (chars: number) => textPx(chars, fontPx('small'));

/**
 * A feed's own words in a column of their own (an identity's action, a context the feed adds): 12
 * characters at their minimum, and the gap; they wrap.
 */
const FEED_WORDS_PX = columnPx(cell(12) + CELL_GAP_PX);

/**
 * The Size and Cycles bars' track, their heads' and their stacked bars' alike (the coordinator's
 * ruling on F3-e: a head's tick row is in the tick form and stands whole over its track): 48 px.
 * The heads drew 120 px of exact ticks over 80 px bars.
 */
const TRACK_PX = COUNT_TRACK_PX;

/**
 * The widest token amount a list column holds on one line (the moved column here, a supply
 * event's amounts), as TokenAmount's column form writes it: under a billion whole tokens at the
 * token's decimals ("999,999,999.99999999" at 8), the raw integer and " (raw)" when they are
 * unknown. A larger amount breaks after a thousands separator.
 */
export function widestTokenAmount(decimals: number | null): string {
  if (decimals === null) return `${tokenAmountText(BigInt(999_999_999), 0, 'exact')} (raw)`;
  return tokenAmountText(BigInt(10) ** BigInt(9 + decimals) - BigInt(1), decimals, 'column');
}

/** One item-feed row's headline from its API `itemContext` (see lib/feed-headlines). */
export function itemFeedHeadline(activity: ItemActivity, item: ItemFeedSubject): ItemFeedHeadline {
  return itemHeadlineFromContext(
    activity.itemContext,
    activity.protocolActions,
    activity.txHash,
    item
  );
}

/**
 * Round 3 (contracts R-A · R-4, OB-M10): an m-NFT mint row's "holds the class cell", on the
 * participant whose lock holds the class cell the mint outputs. Nothing when `classCell` is null
 * or absent (every other row); a lock that is no participant of the row is a contract error.
 */
function classCellNote(activity: ItemActivity): ReadonlyMap<string, string> | undefined {
  const cell = activity.itemContext.classCell;
  if (!cell) return undefined;
  if (!activity.participants.some((p) => p.lockHash === cell.lockHash))
    throw new Error(`the class cell's lock ${cell.lockHash} is no participant of this row`);
  return new Map([[cell.lockHash, 'holds the class cell']]);
}

export function tokenMoved(transaction: Transaction, typeHash: string): string {
  if (!transaction.activity)
    throw new Error(`${transaction.hash}: token transaction is missing its full activity`);
  let value = BigInt(0);
  for (const p of transaction.activity.participants)
    for (const item of p.itemDeltas) {
      if (item.kind !== 'token' || item.typeScriptHash !== typeHash) continue;
      if (!/^-?(0|[1-9]\d*)$/.test(item.delta))
        throw new Error(`${transaction.hash}: invalid token delta ${item.delta}`);
      const delta = BigInt(item.delta);
      if (delta > BigInt(0)) value += delta;
    }
  return value.toString();
}
export interface AssetTransactionsProps {
  rows: readonly Transaction[];
  token?: { typeHash: string; symbol: string | null; decimals: number | null };
  /**
   * A feed-specific column: an item's "where it went" at the row's end (OB-13), or a collection's
   * own column right after Cells, titled by the collection (OB-C9, `placement: 'after-cells'`).
   */
  context?: {
    label: string;
    render: (tx: Transaction) => ReactNode;
    placement?: 'end' | 'after-cells';
  };
  /**
   * ID-C8 (identity boards): the row's action in its own column after Block, the block dated
   * (UTC+8) instead of aged, and no Output capacity / Cycles columns.
   */
  identity?: { actionLabel: string; action: (tx: Transaction) => ReactNode };
}
export function AssetTransactions({ rows, token, context, identity }: AssetTransactionsProps) {
  for (const tx of rows) {
    if (tx.outputCapacity === undefined || !Number.isSafeInteger(tx.txSize) || tx.txSize! <= 0)
      throw new Error(`${tx.hash}: missing transaction capacity or size`);
    if (tx.cyclesStatus === 'complete' ? tx.cycles === null : tx.cycles !== null)
      throw new Error(`${tx.hash}: inconsistent cycles status`);
    if (tx.cycles !== null && (!Number.isSafeInteger(tx.cycles) || tx.cycles < 0))
      throw new Error(`${tx.hash}: invalid cycles`);
  }
  const sizeScale = resourceScale(rows.map((r) => r.txSize!)),
    cycleScale = resourceScale(rows.flatMap((r) => (r.cycles === null ? [] : [r.cycles])));
  // Readability F3-e: every column budgeted from the widest value it holds (lib/list-bounds.ts),
  // the 13 px cells and 12 px notes and units at 0.6 em a glyph, and the 16 px gap after every
  // column; the gap after the last one shown is taken off below (`shown`).
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'tx',
      label: 'Transaction',
      // the hash at the row size (17 characters, 132.6); it was 178
      minWidth: columnPx(cell(shortChars(SHORT_FORMS.row.hash)) + CELL_GAP_PX),
      render: (r) => (
        <AppLink href={`/tx/${r.hash}`}>
          <Hash hash={r.hash} size="row" copyable={false} className="text-small" />
        </AppLink>
      ),
    },
    {
      key: 'block',
      // ID-C8: the identity boards date the block (UTC+8); the other asset boards age it.
      label: identity ? 'Block · date' : 'Block · age',
      // "#99,999,999 · 10y 364d ago" on its line (a date, "2026-10-22", on the identity boards),
      // "tx 9999 in block" at 12 px under it; it was 160
      minWidth: columnPx(
        Math.max(
          cell(`${WIDEST_BLOCK_NUMBER} · ${identity ? '2026-10-22' : WIDEST_AGE}`.length),
          textPx(`tx ${WIDEST_COUNT_DIGITS} in block`.length, fontPx('meta'))
        ) + CELL_GAP_PX
      ),
      render: (r) => (
        <>
          <AppLink href={`/blocks/${r.blockNumber}`}>{formatBlockNumber(r.blockNumber)}</AppLink>
          <span className="text-text-dim">
            {' '}
            ·{' '}
            {unitNode(identity ? formatUtc8Date(r.timestamp) : <Utc8Age timestamp={r.timestamp} />)}
          </span>
          <span className="text-text-dim text-meta block">tx {r.index} in block</span>
        </>
      ),
    },
    ...(identity
      ? [
          {
            key: 'action',
            label: identity.actionLabel,
            // the board's own words, 12 characters at the minimum; they wrap (it was 150)
            minWidth: FEED_WORDS_PX,
            className: '!whitespace-normal',
            render: identity.action,
          } satisfies ListColumn<Transaction>,
        ]
      : []),
    {
      key: 'cells',
      label: 'Cells',
      // four digits a side as written, "9999 → 9999" (85.8); it was 70
      minWidth: columnPx(
        cell(`${WIDEST_COUNT_DIGITS} → ${WIDEST_COUNT_DIGITS}`.length) + CELL_GAP_PX
      ),
      numeric: true,
      render: (r) => `${r.inputsCount} → ${r.outputsCount}`,
    },
  ];
  const contextColumn: ListColumn<Transaction> | null = context
    ? {
        key: 'context',
        label: context.label,
        // the feed's own words, 12 characters at the minimum; they wrap (it was 110 after the
        // cells, 180 at the end)
        minWidth: FEED_WORDS_PX,
        numeric: context.placement === 'after-cells',
        render: context.render,
      }
    : null;
  if (contextColumn && context?.placement === 'after-cells') columns.push(contextColumn);
  if (token)
    columns.push({
      key: 'moved',
      label: `${token.symbol ?? 'Token'} moved`,
      // under a billion whole tokens at the token's decimals on one line; more breaks after a
      // thousands separator (it was 160)
      minWidth: columnPx(cell(widestTokenAmount(token.decimals).length) + CELL_GAP_PX),
      numeric: true,
      className: '!whitespace-normal',
      render: (r) => (
        <TokenAmount
          form="column"
          amount={tokenMoved(r, token.typeHash)}
          decimals={token.decimals}
        />
      ),
    });
  columns.push(
    {
      key: 'output',
      label: 'Output capacity',
      // under a million CKB on one line with its unit (124.8 and 28.8); more breaks before the
      // decimals (it was 180)
      minWidth: columnPx(
        cell(WIDEST_POOL_VALUE_LINE.length) + textPx(' CKB'.length, fontPx('meta')) + CELL_GAP_PX
      ),
      numeric: true,
      className: '!whitespace-normal',
      render: (r) => (
        <CkbAmount
          form="column"
          shannons={BigInt(r.outputCapacity!)}
          signed={false}
          tone="plain"
          wrap
        />
      ),
    },
    {
      key: 'fee',
      label: 'Fee',
      // a fee under 100 CKB with its unit (85.8 and 28.8); it was 140
      minWidth: columnPx(
        cell(WIDEST_TX_FEE.length) + textPx(' CKB'.length, fontPx('meta')) + CELL_GAP_PX
      ),
      numeric: true,
      className: '!whitespace-normal',
      render: (r) => (
        <CkbAmount form="column" shannons={BigInt(r.fee)} signed={false} tone="plain" wrap />
      ),
    },
    {
      key: 'size',
      label: 'Size',
      // OB-13: the header carries the scale every bar in the column shares.
      header:
        sizeScale > 0 ? (
          <ScaleHeader label="Size, bytes" scale={sizeScale} trackWidth={TRACK_PX} />
        ) : undefined,
      // the head's name, "Size, bytes" (11 characters at 12 px, 79.2), wider than the figure over
      // each bar ("597,000", a transaction no larger than a block, 54.6) and the 48 px track; it was
      // 136 for a 120 px row of exact ticks
      minWidth: columnPx(
        Math.max(
          textPx('Size, bytes'.length, fontPx('meta')),
          cell(MAX_BLOCK_BYTES.toLocaleString('en-US').length),
          TRACK_PX
        ) + CELL_GAP_PX
      ),
      render: (r) => (
        <ResourceBar
          value={r.txSize!}
          scale={sizeScale}
          label="Serialized bytes"
          stacked
          trackWidth={TRACK_PX}
        />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      header:
        cycleScale > 0 ? (
          <ScaleHeader label="Cycles" scale={cycleScale} trackWidth={TRACK_PX} />
        ) : undefined,
      // the figure over each bar, a transaction's cycles under a billion as the committed list's
      // ("750,000,000", 85.8), wider than the head's name and the 48 px track; it was 120 for a
      // 120 px row of exact ticks
      minWidth: columnPx(
        Math.max(
          textPx('Cycles'.length, fontPx('meta')),
          cell(TX_CYCLES_BOUND.toLocaleString('en-US').length),
          TRACK_PX
        ) + CELL_GAP_PX
      ),
      render: (r) =>
        r.cycles === null ? (
          <span className="text-text-dim">
            {r.cyclesStatus === 'failed' ? 'Calculation failed' : 'Not computed'}
          </span>
        ) : (
          <ResourceBar
            label="Cycles"
            value={r.cycles}
            scale={cycleScale}
            stacked
            trackWidth={TRACK_PX}
          />
        ),
    }
  );
  // ID-C8: the identity boards carry no Output capacity or Cycles column.
  const kept = identity
    ? columns.filter((column) => column.key !== 'output' && column.key !== 'cycles')
    : columns;
  if (contextColumn && context?.placement !== 'after-cells') kept.push(contextColumn);
  // the last column keeps no gap after it (its cells' last:pr-0)
  const shown = kept.map((column, i) =>
    i === kept.length - 1 ? { ...column, minWidth: column.minWidth - CELL_GAP_PX } : column
  );
  return (
    <ListTable
      label="Asset transactions"
      rows={rows}
      columns={shown}
      rowKey={(r) => r.hash}
      empty="No transactions."
      mobileRow={(r, wide) => {
        // What the row did on this asset: the identity action, the feed's own context (an item's
        // cell after it, a collection's headline) and the tokens moved; then the CKB it output.
        const did = [
          identity ? identity.action(r) : null,
          context ? (
            context.placement === 'after-cells' ? (
              context.render(r)
            ) : (
              <span>
                <span className="text-text-dim">{context.label}: </span>
                {context.render(r)}
              </span>
            )
          ) : null,
          token ? (
            <span className="whitespace-nowrap">
              <TokenAmount
                form="summary"
                amount={tokenMoved(r, token.typeHash)}
                decimals={token.decimals}
              />{' '}
              {token.symbol ?? 'token'} moved
            </span>
          ) : null,
        ];
        const output = identity ? null : (
          <CkbAmount
            form="summary"
            shannons={BigInt(r.outputCapacity!)}
            signed={false}
            tone="plain"
          />
        );
        const figures = [
          <span key="cells" className="whitespace-nowrap">
            {r.inputsCount} → {formatCount(r.outputsCount, 'cell')}
          </span>,
          <span key="fee" className="whitespace-nowrap">
            fee <CkbAmount form="summary" shannons={BigInt(r.fee)} signed={false} tone="plain" />
          </span>,
          <span key="size" className="whitespace-nowrap">
            {r.txSize!.toLocaleString('en-US')} B
          </span>,
          // ID-C8: the identity boards carry no cycles.
          identity ? null : (
            <span key="cycles" className="whitespace-nowrap">
              {r.cycles === null
                ? r.cyclesStatus === 'failed'
                  ? 'cycles calculation failed'
                  : 'cycles not computed'
                : `${r.cycles.toLocaleString('en-US')} cycles`}
            </span>
          ),
          <span key="index" className="whitespace-nowrap">
            tx {r.index} in block
          </span>,
        ];
        const hasDid = did.some((part) => part !== null);
        return (
          <PhoneRow
            testId="asset-tx-row-mobile"
            lines={[
              {
                key: 'identity',
                tone: 'head',
                left: (
                  <AppLink href={`/tx/${r.hash}`}>
                    <Hash hash={r.hash} size="tight" copyable={false} className="text-body" />
                  </AppLink>
                ),
                // the block, its date and its age are each a unit; "this device's clock is
                // behind" in the age's place is words and breaks between them (Utc8Age; L2)
                right: (
                  <>
                    <NoBreak>
                      <AppLink href={`/blocks/${r.blockNumber}`} className="text-text-bright">
                        {formatBlockNumber(r.blockNumber)}
                      </AppLink>
                    </NoBreak>
                    <span className="text-text">
                      {' '}
                      ·{' '}
                      {identity ? (
                        <NoBreak>{formatUtc8Date(r.timestamp)}</NoBreak>
                      ) : (
                        <Utc8Age timestamp={r.timestamp} />
                      )}
                    </span>
                  </>
                ),
              },
              wide
                ? {
                    key: 'what',
                    tone: 'body',
                    left: <Dotted parts={[...did, output]} />,
                    right: (
                      <span className="text-text-dim text-meta">
                        <Dotted parts={figures} />
                      </span>
                    ),
                  }
                : hasDid
                  ? { key: 'what', tone: 'body', left: <Dotted parts={did} />, right: output }
                  : { key: 'what', tone: 'body', left: 'output', right: output },
              !wide && { key: 'figures', tone: 'quiet', left: <Dotted parts={figures} /> },
            ]}
          />
        );
      }}
    />
  );
}
export interface AssetFeedProps {
  queryKey: string;
  scope: string;
  initialActivityCursor?: string;
  onActivityCursorChange?: (cursor: string | undefined) => void;
  /** A collection feed's `total` is null while its count is not derived (L2): say nothing. */
  loadActivities: (params: {
    limit: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<GlobalActivity> | CollectionFeedPage<GlobalActivity>>;
  loadTransactions: (params: {
    limit: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<Transaction> | CollectionFeedPage<Transaction>>;
  token?: AssetTransactionsProps['token'];
  context?: AssetTransactionsProps['context'];
  identity?: AssetTransactionsProps['identity'];
  /** One item's own feed: every row carries its item context (see itemFeedHeadline). */
  item?: ItemFeedSubject;
  /** A collection's feed: every row carries its collection context (OB-C13). */
  collection?: boolean;
  /**
   * F2 objects (OB-12, IV-14, OB-M3): the asset boards' plain rows — no script-call lines, the
   * lock kind as its own column, and a batch of objects folded behind "+N more objects".
   */
  plain?: boolean;
  /** F2 objects: the object a folded batch leads with (this page's item). */
  focusItemId?: string;
  /** F2 objects (IV-14): every row of this feed is about one class (the token board's mark). */
  mark?: AssetClass;
  /**
   * Consistency round (Main.dc.html, "Recent activity"): open as the newest `rows` activities
   * under an h2, with two links — all activity, and the transactions (`transactions` names their
   * count) — that open the full feed (tabs and pages) in place. Omitted: the full feed (old
   * default).
   */
  recent?: { rows: number; transactions: number | null };
}
export function AssetFeed(props: AssetFeedProps) {
  const [opened, setOpened] = useState<'activities' | 'transactions' | null>(null);
  if (!props.recent || opened) return <FullAssetFeed {...props} initialTab={opened ?? undefined} />;
  return <RecentAssetFeed {...props} recent={props.recent} onOpen={setOpened} />;
}

/** The newest few activities of an asset, as the board's "Recent activity" (see `recent`). */
function RecentAssetFeed({
  queryKey,
  loadActivities,
  item,
  collection = false,
  plain = false,
  focusItemId,
  mark,
  token,
  recent,
  onOpen,
}: AssetFeedProps & {
  recent: NonNullable<AssetFeedProps['recent']>;
  onOpen: (tab: 'activities' | 'transactions') => void;
}) {
  if (!Number.isSafeInteger(recent.rows) || recent.rows <= 0)
    throw new Error(`recent activity needs a positive row count, got ${recent.rows}`);
  const acts = useQuery({
    queryKey: ['asset-feed', queryKey, 'recent', recent.rows],
    queryFn: () => loadActivities({ limit: recent.rows }),
  });
  return (
    <section aria-label="Recent activity" className="min-w-0">
      {/* Rubbing × Song print, phase 5 (A10): the activity, run on the rows it shows; the two
          links that open the full feed are the right-prompt note. */}
      <PromptHeading
        command="Activity"
        args={`--last ${recent.rows}`}
        note={
          <>
            <button
              type="button"
              onClick={() => onOpen('activities')}
              className="text-text hover:text-text-bright"
            >
              All activity →
            </button>
            <span className="text-text-ghost"> · </span>
            <button
              type="button"
              onClick={() => onOpen('transactions')}
              className="text-text hover:text-text-bright tabular-nums"
            >
              {unitNode(
                recent.transactions === null
                  ? 'All transactions →'
                  : `${formatCount(recent.transactions, 'transaction')} →`
              )}
            </button>
          </>
        }
      />
      <div className="mt-3.5">
        {acts.data ? (
          <>
            <AssetReadState error={acts.error} label="recent activity" retained />
            {acts.data.data.length ? (
              <ActivityRuns
                showScriptDetails={!plain}
                rows={acts.data.data.slice(0, recent.rows).map((a) =>
                  feedActivityRow(a, {
                    item,
                    collection,
                    plain,
                    focusItemId,
                    mark,
                    focusTokenTypeHash: token?.typeHash,
                  })
                )}
              />
            ) : (
              <p className="text-text-dim text-small py-6">No activities.</p>
            )}
          </>
        ) : (
          <AssetReadState label="recent activity" error={acts.error} read={acts} />
        )}
      </div>
    </section>
  );
}

/** What an asset feed's rows are drawn with (the feed's props). */
interface FeedRowOptions {
  item?: ItemFeedSubject;
  collection: boolean;
  plain: boolean;
  focusItemId?: string;
  mark?: AssetClass;
  /** A token page's token (`AssetFeedProps.token`): its amount leads each line (B4). */
  focusTokenTypeHash?: string;
}

/**
 * One activity row of an asset feed, its headline from the item or collection context, with what
 * its run is compared on (readability B4: the title in words, a neighbour's set-back rows apart
 * from the item's own). A row the feed cannot read reports itself in place and never folds.
 */
function feedActivityRow(
  a: GlobalActivity,
  { item, collection, plain, focusItemId, mark, focusTokenTypeHash }: FeedRowOptions
): FeedRow {
  let headline: ItemFeedHeadline | undefined;
  let notes: ReadonlyMap<string, string> | undefined;
  try {
    notes = item ? classCellNote(a as ItemActivity) : undefined;
    headline = item
      ? itemFeedHeadline(a as ItemActivity, item)
      : collection
        ? {
            text: collectionFeedHeadline((a as CollectionFeedActivity).collectionContext, a.txHash),
            neighbour: false,
          }
        : undefined;
  } catch (error) {
    // A row that breaks the feed contract is reported in place; the page stays up.
    return {
      key: a.txHash,
      subject: null,
      activity: a,
      title: null,
      row: (
        <div className="border-row-rule border-b py-3">
          <ActivityReadError txHash={a.txHash} error={error} />
        </div>
      ),
    };
  }
  // F2 identities (round 2, ID-C13): every row of an identity's feed is about it.
  const rowMark = mark ?? (item?.kind === 'identity' ? 'identities' : undefined);
  const headlineNode = headline && <FeedHeadlineText headline={headline} />;
  const quiet = headline?.neighbour === true;
  const words = headline && [headline.text, ...(headline.raw ?? [])].filter(Boolean).join(' · ');
  return {
    key: a.txHash,
    subject: runSubject(a, `${quiet ? 'neighbour: ' : ''}${activityTitleText(a, words, rowMark)}`),
    activity: a,
    title: <ActivityTitle activity={a} headline={headlineNode} mark={rowMark} />,
    quiet,
    row: (
      <ActivityRow
        activity={a}
        headline={headlineNode}
        // a neighbour's row is set back (R4): the quiet variant, every word dim, nothing faded
        className={quiet ? 'quiet' : undefined}
        mark={rowMark}
        showScriptDetails={!plain}
        lockColumn={plain}
        foldObjects={plain}
        focusItemId={focusItemId}
        focusTokenTypeHash={focusTokenTypeHash}
        participantNotes={notes}
      />
    ),
  };
}

function FullAssetFeed({
  queryKey,
  scope,
  loadActivities,
  loadTransactions,
  token,
  context,
  identity,
  initialActivityCursor,
  onActivityCursorChange,
  item,
  collection = false,
  plain = false,
  focusItemId,
  mark,
  initialTab = 'activities',
}: AssetFeedProps & { initialTab?: 'activities' | 'transactions' }) {
  const [tab, setTab] = useState<'activities' | 'transactions'>(initialTab);
  const actsPages = useCursorPagination(),
    txPages = useCursorPagination();
  const pageSize = usePageSize(ENTITY_PAGE_SIZE);
  const activityCursor = actsPages.cursor ?? initialActivityCursor;
  useEffect(() => {
    onActivityCursorChange?.(activityCursor);
  }, [activityCursor, onActivityCursorChange]);
  const acts = useQuery({
    queryKey: ['asset-feed', queryKey, 'activities', activityCursor, pageSize],
    queryFn: () => loadActivities({ limit: pageSize, cursor: activityCursor }),
    enabled: tab === 'activities',
  });
  const txs = useQuery({
    queryKey: ['asset-feed', queryKey, 'transactions', txPages.cursor, pageSize],
    queryFn: () => loadTransactions({ limit: pageSize, cursor: txPages.cursor }),
    enabled: tab === 'transactions',
  });
  const current = tab === 'activities' ? acts : txs,
    pages = tab === 'activities' ? actsPages : txPages;
  return (
    <section aria-label="What happened" className="min-w-0">
      <div
        role="tablist"
        aria-label="What happened"
        className="border-base-border mb-3 flex flex-wrap items-center gap-6 border-b max-md:gap-x-4"
      >
        {(['activities', 'transactions'] as const).map((t) => (
          <button
            key={t}
            role="tab"
            aria-selected={tab === t}
            onClick={() => setTab(t)}
            className={`text-section max-md:text-body border-b-2 py-3 ${tab === t ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
          >
            {t === 'activities' ? 'Activities' : 'Transactions'}
          </button>
        ))}
        <span className="text-text-dim text-meta pb-2 sm:ml-auto">{scope} · newest first</span>
      </div>
      {current.data ? (
        <>
          <AssetReadState error={current.error} label={tab} retained />
          {tab === 'activities' ? (
            acts.data!.data.length ? (
              // B4: a run folds within this page; one that goes on is a second run on the next
              <ActivityRuns
                showScriptDetails={!plain}
                rows={acts.data!.data.map((a) =>
                  feedActivityRow(a, {
                    item,
                    collection,
                    plain,
                    focusItemId,
                    mark,
                    focusTokenTypeHash: token?.typeHash,
                  })
                )}
              />
            ) : (
              <p className="text-text-dim text-small py-6">No activities.</p>
            )
          ) : (
            <AssetTransactions
              rows={txs.data!.data}
              token={token}
              context={context}
              identity={identity}
            />
          )}
          <CursorPagination
            total={current.data.total}
            totalLabel="transactions"
            pageSize={pageSize}
            page={pages.page}
            currentCount={current.data.data.length}
            hasMore={current.data.hasMore}
            hasPrevious={pages.hasPrevious}
            onNext={() => pages.goToNext(current.data!.nextCursor)}
            onPrevious={pages.goToPrevious}
          />
        </>
      ) : (
        <AssetReadState label={tab} error={current.error} read={current} />
      )}
    </section>
  );
}
