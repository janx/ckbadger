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
import { ActivityRow } from '@/components/activity/activity-row';
import { ActivityReadError } from '@/components/activity-event-row';
import {
  collectionFeedHeadline,
  itemFeedHeadline as itemHeadlineFromContext,
  type ItemFeedHeadline,
  type ItemFeedSubject,
} from '@/lib/feed-headlines';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import { ResourceBar, ScaleHeader, resourceScale } from '@/components/blocks/block-visual';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { AssetReadState } from '@/components/asset/detail-shell';
import { AssetAmount } from '@/components/asset/inventory-table';
import { formatTimeAgo, formatUtc8Date } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { FeedHeadlineText } from '@/components/asset/feed-headline-text';
import type { AssetClass } from '@/lib/design-tokens';
export type { ItemFeedHeadline, ItemFeedSubject } from '@/lib/feed-headlines';

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
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'tx',
      label: 'Transaction',
      minWidth: 178,
      render: (r) => (
        <AppLink href={`/tx/${r.hash}`}>
          <Hash
            hash={r.hash}
            copyable={false}
            startChars={10}
            endChars={6}
            className="text-small"
          />
        </AppLink>
      ),
    },
    {
      key: 'block',
      // ID-C8: the identity boards date the block (UTC+8); the other asset boards age it.
      label: identity ? 'Block · date' : 'Block · age',
      minWidth: 160,
      render: (r) => (
        <>
          <AppLink href={`/blocks/${r.blockNumber}`}>{formatBlockNumber(r.blockNumber)}</AppLink>
          <span className="text-text-dim">
            {' '}
            · {identity ? formatUtc8Date(r.timestamp) : formatTimeAgo(r.timestamp)}
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
            minWidth: 150,
            className: '!whitespace-normal',
            render: identity.action,
          } satisfies ListColumn<Transaction>,
        ]
      : []),
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 70,
      numeric: true,
      render: (r) => `${r.inputsCount} → ${r.outputsCount}`,
    },
  ];
  const contextColumn: ListColumn<Transaction> | null = context
    ? {
        key: 'context',
        label: context.label,
        minWidth: context.placement === 'after-cells' ? 110 : 180,
        numeric: context.placement === 'after-cells',
        render: context.render,
      }
    : null;
  if (contextColumn && context?.placement === 'after-cells') columns.push(contextColumn);
  if (token)
    columns.push({
      key: 'moved',
      label: `${token.symbol ?? 'Token'} moved`,
      minWidth: 160,
      numeric: true,
      className: '!whitespace-normal',
      render: (r) => (
        <AssetAmount amount={tokenMoved(r, token.typeHash)} decimals={token.decimals} />
      ),
    });
  columns.push(
    {
      key: 'output',
      label: 'Output capacity',
      minWidth: 180,
      numeric: true,
      className: '!whitespace-normal',
      render: (r) => (
        <ExactCkb shannons={BigInt(r.outputCapacity!)} signed={false} tone="plain" wrap />
      ),
    },
    {
      key: 'fee',
      label: 'Fee',
      minWidth: 140,
      numeric: true,
      className: '!whitespace-normal',
      render: (r) => <ExactCkb shannons={BigInt(r.fee)} signed={false} tone="plain" wrap />,
    },
    {
      key: 'size',
      label: 'Size',
      // OB-13: the header carries the scale every bar in the column shares.
      header: sizeScale > 0 ? <ScaleHeader label="Size, bytes" scale={sizeScale} /> : undefined,
      minWidth: 112,
      render: (r) => (
        <ResourceBar value={r.txSize!} scale={sizeScale} label="Serialized bytes" stacked />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      header: cycleScale > 0 ? <ScaleHeader label="Cycles" scale={cycleScale} /> : undefined,
      minWidth: 120,
      render: (r) =>
        r.cycles === null ? (
          <span className="text-text-dim">
            {r.cyclesStatus === 'failed' ? 'Calculation failed' : 'Not computed'}
          </span>
        ) : (
          <ResourceBar label="Cycles" value={r.cycles} scale={cycleScale} stacked />
        ),
    }
  );
  // ID-C8: the identity boards carry no Output capacity or Cycles column.
  const shown = identity
    ? columns.filter((column) => column.key !== 'output' && column.key !== 'cycles')
    : columns;
  if (contextColumn && context?.placement !== 'after-cells') shown.push(contextColumn);
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
              <AssetAmount amount={tokenMoved(r, token.typeHash)} decimals={token.decimals} />{' '}
              {token.symbol ?? 'token'} moved
            </span>
          ) : null,
        ];
        const output = identity ? null : (
          <ExactCkb shannons={BigInt(r.outputCapacity!)} signed={false} tone="plain" />
        );
        const figures = [
          <span key="cells" className="whitespace-nowrap">
            {r.inputsCount} → {r.outputsCount} cells
          </span>,
          <span key="fee" className="whitespace-nowrap">
            fee <ExactCkb shannons={BigInt(r.fee)} signed={false} tone="plain" />
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
                    <Hash
                      hash={r.hash}
                      copyable={false}
                      startChars={10}
                      endChars={6}
                      className="text-body"
                    />
                  </AppLink>
                ),
                right: (
                  <span className="whitespace-nowrap">
                    <AppLink href={`/blocks/${r.blockNumber}`} className="text-text-bright">
                      {formatBlockNumber(r.blockNumber)}
                    </AppLink>
                    <span className="text-text">
                      {' '}
                      · {identity ? formatUtc8Date(r.timestamp) : formatTimeAgo(r.timestamp)}
                    </span>
                  </span>
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
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
        <h2 className="text-text-bright text-section font-medium">Recent activity</h2>
        <span className="text-small">
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
            {recent.transactions === null
              ? 'All transactions →'
              : `${recent.transactions.toLocaleString('en-US')} transactions →`}
          </button>
        </span>
      </div>
      <div className="mt-3.5">
        {acts.data ? (
          <>
            <AssetReadState error={acts.error} label="recent activity" retained />
            {acts.data.data.length ? (
              acts.data.data
                .slice(0, recent.rows)
                .map((a) => (
                  <FeedActivityRow
                    key={a.txHash}
                    activity={a}
                    item={item}
                    collection={collection}
                    plain={plain}
                    focusItemId={focusItemId}
                    mark={mark}
                  />
                ))
            ) : (
              <p className="text-text-dim text-small py-6">No activities.</p>
            )}
          </>
        ) : (
          <AssetReadState label="recent activity" error={acts.error} loading={acts.isLoading} />
        )}
      </div>
    </section>
  );
}

/** One activity row of an asset feed, its headline from the item or collection context. */
function FeedActivityRow({
  activity: a,
  item,
  collection,
  plain,
  focusItemId,
  mark,
}: {
  activity: GlobalActivity;
  item?: ItemFeedSubject;
  collection: boolean;
  plain: boolean;
  focusItemId?: string;
  mark?: AssetClass;
}) {
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
    return (
      <div className="border-base-elevated border-b py-3">
        <ActivityReadError txHash={a.txHash} error={error} />
      </div>
    );
  }
  return (
    <ActivityRow
      activity={a}
      headline={headline && <FeedHeadlineText headline={headline} />}
      className={headline?.neighbour ? 'opacity-[0.45]' : undefined}
      // F2 identities (round 2, ID-C13): every row of an identity's feed is about it.
      mark={mark ?? (item?.kind === 'identity' ? 'identities' : undefined)}
      showScriptDetails={!plain}
      lockColumn={plain}
      foldObjects={plain}
      focusItemId={focusItemId}
      participantNotes={notes}
    />
  );
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
  const pageSize = usePageSize(50);
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
        className="border-base-border mb-3 flex flex-wrap items-center gap-6 border-b"
      >
        {(['activities', 'transactions'] as const).map((t) => (
          <button
            key={t}
            role="tab"
            aria-selected={tab === t}
            onClick={() => setTab(t)}
            className={`text-section border-b-2 py-3 ${tab === t ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
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
              acts.data!.data.map((a) => (
                <FeedActivityRow
                  key={a.txHash}
                  activity={a}
                  item={item}
                  collection={collection}
                  plain={plain}
                  focusItemId={focusItemId}
                  mark={mark}
                />
              ))
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
        <AssetReadState label={tab} error={current.error} loading={current.isLoading} />
      )}
    </section>
  );
}
