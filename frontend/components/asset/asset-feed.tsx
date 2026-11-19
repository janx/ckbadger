'use client';
import { FoldToggle } from '@/components/ui/fold-toggle';
import { useEffect, useId, useState, type ReactNode } from 'react';
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
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { NoBreak } from '@/components/ui/no-break';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import { ResourceBar, ScaleHeader, resourceScale } from '@/components/blocks/block-visual';
import { Pager } from '@/components/ui/pager';
import { Tabs, type Tab } from '@/components/ui/tabs';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { ENTITY_PAGE_SIZE, timePageAt } from '@/lib/pagination';
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
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { COUNT_TRACK_PX } from '@/components/ui/aligned-bar';
import {
  MAX_BLOCK_BYTES,
  MAX_TX_INPUTS,
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
/**
 * Line 1 of an asset's transaction on a phone (the phone-rows board's head(), as the address page's
 * transactions): the tight hash at the key's 14 px (92.4; no stone, as the table's), the 12 px
 * between the parts and, at 13 px, the block with the oldest age a live list writes, "#99,999,999 ·
 * 10y 364d ago" (26 characters, 202.8), wider than the identity boards' date ("· 2026-11-02",
 * 187.2): 307.2 of a 360 px phone's 328 px line.
 */
const ASSET_TX_PHONE_HEAD_PX = phoneHead(
  'asset transactions',
  textPx(shortChars(SHORT_FORMS.tight.hash), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(`${WIDEST_BLOCK_NUMBER} · ${WIDEST_AGE}`.length, fontPx('small'))
);

/**
 * An asset's transaction on a phone (the phone-rows board, as the address page's transaction row):
 * the hash and the block with its age (its date on an identity's board, as the table writes it);
 * what the row did on this asset where the feed says it (the identity's action, the feed's own
 * column under its label); quiet, the cells, the token's amount moved with its symbol and the fee,
 * each named. The output capacity, the bytes, the cycles and the index in the block are the table's
 * and the transaction page's; the cards from 560 px (`wide`) keep them at the end of the quiet line.
 */
function AssetTxPhoneRow({
  r,
  wide,
  token,
  context,
  identity,
}: {
  r: Transaction;
  wide: boolean;
  token: AssetTransactionsProps['token'];
  context: AssetTransactionsProps['context'];
  identity: AssetTransactionsProps['identity'];
}) {
  const did = [
    identity ? identity.action(r) : null,
    context ? (
      context.placement === 'after-cells' ? (
        context.render(r)
      ) : (
        <Labelled label={context.label} words>
          {context.render(r)}
        </Labelled>
      )
    ) : null,
  ];
  const figures = [
    <span key="cells" className="whitespace-nowrap">
      {r.inputsCount} → {formatCount(r.outputsCount, 'cell')}
    </span>,
    token ? (
      <Labelled key="moved" label={`${token.symbol ?? 'token'} moved`} after>
        <TokenAmount
          form="summary"
          amount={tokenMoved(r, token.typeHash)}
          decimals={token.decimals}
        />
      </Labelled>
    ) : null,
    <Labelled key="fee" label="fee">
      <CkbAmount form="summary" shannons={BigInt(r.fee)} signed={false} tone="plain" />
    </Labelled>,
    ...(wide
      ? [
          // ID-C8: the identity boards carry no output capacity and no cycles.
          identity ? null : (
            <Labelled key="output" label="output">
              <CkbAmount
                form="summary"
                shannons={BigInt(r.outputCapacity!)}
                signed={false}
                tone="plain"
              />
            </Labelled>
          ),
          <span key="size" className="whitespace-nowrap">
            {r.txSize!.toLocaleString('en-US')} B
          </span>,
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
        ]
      : []),
  ];
  return (
    <PhoneRow
      testId="asset-tx-row-mobile"
      head={ASSET_TX_PHONE_HEAD_PX}
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left: (
            <AppLink href={`/tx/${r.hash}`} look="key">
              <Hash hash={r.hash} size="tight" className="text-body" />
            </AppLink>
          ),
          // the block and its age (or date) are each a unit; "this device's clock is behind" in
          // the age's place is words and breaks between them (Utc8Age; L2)
          right: (
            <>
              <NoBreak>
                <AppLink href={`/blocks/${r.blockNumber}`} look="inline">
                  {formatBlockNumber(r.blockNumber)}
                </AppLink>
              </NoBreak>
              {' · '}
              {identity ? (
                <NoBreak>{formatUtc8Date(r.timestamp)}</NoBreak>
              ) : (
                <Utc8Age timestamp={r.timestamp} />
              )}
            </>
          ),
        },
        did.some((part) => part !== null) && {
          key: 'what',
          tone: 'body',
          left: <Dotted parts={did} />,
        },
        { key: 'figures', tone: 'quiet', left: <Dotted parts={figures} /> },
      ]}
    />
  );
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
        <AppLink look="key" href={`/tx/${r.hash}`}>
          <Hash hash={r.hash} size="row" className="text-small" />
        </AppLink>
      ),
    },
    {
      key: 'block',
      // ID-C8: the identity boards date the block (UTC+8); the other asset boards age it.
      label: identity ? 'Block · date' : 'Block · age',
      // "#99,999,999 · 10y 364d ago" on its line (a date, "2026-10-22", on the identity boards),
      // "tx 9999 in block" under it, both values at the cell's 13 px (ink and size roles: the age
      // stood quiet and the place at 12 px; 16 glyphs × 7.8 = 124.8 under the first line's 26 ×
      // 7.8 = 202.8, so the column's width does not move); it was 160
      minWidth: columnPx(
        Math.max(
          cell(`${WIDEST_BLOCK_NUMBER} · ${identity ? '2026-10-22' : WIDEST_AGE}`.length),
          cell(`tx ${WIDEST_COUNT_DIGITS} in block`.length)
        ) + CELL_GAP_PX
      ),
      render: (r) => (
        <>
          <AppLink look="inline" href={`/blocks/${r.blockNumber}`}>
            {formatBlockNumber(r.blockNumber)}
          </AppLink>
          <span className="text-text-dim"> · </span>
          {unitNode(identity ? formatUtc8Date(r.timestamp) : <Utc8Age timestamp={r.timestamp} />)}
          <span className="block">tx {r.index} in block</span>
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
      // a transaction's inputs, up to MAX_TX_INPUTS (13,568: five digits, a consensus bound), and
      // its outputs, four digits (WIDEST_COUNT), ungrouped as written: "13568 → 9999" (93.6 at 13
      // px; it budgeted four digits of inputs, "9999 → 9999", 85.8: the page lane notes)
      minWidth: columnPx(cell(`${MAX_TX_INPUTS} → ${WIDEST_COUNT_DIGITS}`.length) + CELL_GAP_PX),
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
          <span className="text-text">
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
      mobileRow={(r, wide) => (
        <AssetTxPhoneRow r={r} wide={wide} {...{ token, context, identity }} />
      )}
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
  const [opened, setOpened] = useState(false);
  const feedId = useId();
  if (!props.recent) return <FullAssetFeed {...props} />;
  const count = props.recent.transactions;
  // Lane P2's follow-up (item 10): the newest rows open into the whole feed in place, a fold, so
  // its control is the one disclosure control (FoldToggle, lane P1): "all 184 transactions ↓",
  // the collection's own count, opens the whole feed on its transactions (the tabs switch to the
  // activities), and the same button, still focused, "fewer ↑", folds it back to the newest
  // rows. Where the count is not derived (L2) the fold asks for the feed by name, "all activity
  // ↓", no number (lane P2's third round, item 9), and opens it on its activities.
  return (
    <>
      <div id={feedId} className="min-w-0">
        {opened ? (
          // the counted fold opens on the transactions it counts, the uncounted on the activities
          <FullAssetFeed {...props} initialTab={count === null ? 'activities' : 'transactions'} />
        ) : (
          <RecentAssetFeed {...props} recent={props.recent} />
        )}
      </div>
      <div className="mt-3">
        {count === null ? (
          <FoldToggle open={opened} uncounted="activity" controls={feedId} onToggle={setOpened} />
        ) : (
          <FoldToggle
            open={opened}
            count={count}
            singular="transaction"
            controls={feedId}
            onToggle={setOpened}
          />
        )}
      </div>
    </>
  );
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
}: AssetFeedProps & {
  recent: NonNullable<AssetFeedProps['recent']>;
}) {
  if (!Number.isSafeInteger(recent.rows) || recent.rows <= 0)
    throw new Error(`recent activity needs a positive row count, got ${recent.rows}`);
  const acts = useQuery({
    queryKey: ['asset-feed', queryKey, 'recent', recent.rows],
    queryFn: () => loadActivities({ limit: recent.rows }),
  });
  return (
    <section aria-label="Recent activity" className="min-w-0">
      {/* Rubbing × Song print, phase 5 (A10): the activity, run on the rows it shows; the fold
          that opens the whole feed stands under the rows (AssetFeed). */}
      <PromptHeading command="Activity" args={`--last ${recent.rows}`} />
      <div className="mt-3.5">
        {acts.data ? (
          <>
            <AssetReadState error={acts.error} label="recent activity" retained />
            {acts.data.data.length ? (
              <ActivityRuns
                showScriptDetails={!plain}
                // rows past the teaser's, or a next page, are unread: its last run is not final
                more={acts.data.data.length > recent.rows || acts.data.hasMore}
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
              <p className="text-text text-small py-6">No activities.</p>
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
  // a neighbour's row is set back (R4): the quiet variant, every word dim, nothing faded; its run
  // with it (B4)
  const setBack = quiet ? 'quiet' : undefined;
  const words = headline && [headline.text, ...(headline.raw ?? [])].filter(Boolean).join(' · ');
  return {
    key: a.txHash,
    subject: runSubject(a, `${quiet ? 'neighbour: ' : ''}${activityTitleText(a, words, rowMark)}`),
    activity: a,
    title: <ActivityTitle activity={a} headline={headlineNode} mark={rowMark} />,
    className: setBack,
    row: (
      <ActivityRow
        activity={a}
        headline={headlineNode}
        className={setBack}
        mark={rowMark}
        showScriptDetails={!plain}
        foldObjects={plain}
        focusItemId={focusItemId}
        focusTokenTypeHash={focusTokenTypeHash}
        participantNotes={notes}
      />
    ),
  };
}

/** The feed's two layers, as the tab row names them (a command line's names: lowercase). */
type FeedLayer = 'activities' | 'transactions';
const FEED_TABS: readonly Tab<FeedLayer>[] = [
  { key: 'activities', label: 'activities' },
  { key: 'transactions', label: 'transactions' },
];

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
}: AssetFeedProps & { initialTab?: FeedLayer }) {
  const [tab, setTab] = useState<FeedLayer>(initialTab);
  const pageSize = usePageSize(ENTITY_PAGE_SIZE);
  // The URL's cursor, until the reader goes newer from it: the API pages one way (older), so the
  // way newer from a cursor the walk has no history of is the newest end (the page lane notes:
  // a reader who landed on page 3 could not go newer at all)
  const [urlCursor, setUrlCursor] = useState(initialActivityCursor);
  // a change of page size at md starts each walk again (lane c2-finish, 1d); the activities' walk
  // starts from the URL's cursor, so its first page is stamped there (lane cd-fixes-controls)
  const actsPages = useCursorPagination(pageSize, { from: urlCursor }),
    txPages = useCursorPagination(pageSize);
  const activityCursor = actsPages.cursor;
  useEffect(() => {
    onActivityCursorChange?.(activityCursor);
  }, [activityCursor, onActivityCursorChange]);
  const acts = useQuery({
    queryKey: ['asset-feed', queryKey, 'activities', activityCursor, pageSize],
    queryFn: actsPages.ask((cursor) => loadActivities({ limit: pageSize, cursor })),
    enabled: tab === 'activities',
  });
  const txs = useQuery({
    queryKey: ['asset-feed', queryKey, 'transactions', txPages.cursor, pageSize],
    queryFn: txPages.ask((cursor) => loadTransactions({ limit: pageSize, cursor })),
    enabled: tab === 'transactions',
  });
  const current = tab === 'activities' ? acts : txs,
    pages = tab === 'activities' ? actsPages : txPages;
  const at = current.data ? timePageAt(tab, current.data, pages) : null;
  // A page counts from where its cursor started: an activity page opened at a cursor from the
  // URL (an identity page's link) is not counted from the newest end, so it names no page.
  const fromNewest = tab === 'transactions' || urlCursor === undefined;
  const newer = pages.hasPrevious
    ? pages.goToPrevious
    : tab === 'activities' && urlCursor !== undefined
      ? () => setUrlCursor(undefined)
      : undefined;
  return (
    <section aria-label="What happened" className="min-w-0">
      {/* the tab row is the section's command line (the controls board); the outline keeps the
          section's name */}
      <h2 className="sr-only">What happened</h2>
      <Tabs label="What happened" command tabs={FEED_TABS} value={tab} onChange={setTab}>
        {/* the feed's scope, a note (it stood at the row's right end): not a paragraph */}
        <div className="text-text-dim text-meta pb-1 pt-3">{scope} · newest first</div>
        {current.data ? (
          <>
            <AssetReadState error={current.error} label={tab} retained />
            {tab === 'activities' ? (
              acts.data!.data.length ? (
                // B4: a run folds within this page; one that goes on is a second run on the next
                <ActivityRuns
                  showScriptDetails={!plain}
                  more={acts.data!.hasMore}
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
                <p className="text-text text-small py-6">No activities.</p>
              )
            ) : (
              <AssetTransactions
                rows={txs.data!.data}
                token={token}
                context={context}
                identity={identity}
              />
            )}
            {(at || newer) && (
              // the controls board's pager: a list in time, newest first. A feed opened at a URL
              // cursor on its oldest page is one page of the walk, but it keeps its way to the
              // newest end (lane P2's fifth round, NOTE 8: no pager was drawn, and no way newer)
              <div className="mt-3">
                <Pager
                  mode="time"
                  label={tab}
                  order="newest first"
                  page={fromNewest && at ? at.page : undefined}
                  pages={fromNewest && at ? at.pages : undefined}
                  onNewer={newer}
                  onOlder={at?.onOlder}
                />
              </div>
            )}
          </>
        ) : (
          <AssetReadState label={tab} error={current.error} read={current} />
        )}
      </Tabs>
    </section>
  );
}
