'use client';
import { useEffect, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  CursorPaginatedResponse,
  GlobalActivity,
  ItemActivity,
  ItemRingRelink,
  Transaction,
} from '@/lib/api';
import { ActivityRow } from '@/components/activity/activity-row';
import { ActivityReadError, capitalizeAction } from '@/components/activity-event-row';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import { ResourceBar, resourceScale } from '@/components/blocks/block-visual';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { AssetReadState } from '@/components/asset/detail-shell';
import { AssetAmount } from '@/components/asset/inventory-table';
import { formatTimeAgo } from '@/lib/utils';
const NEIGHBOUR_HEADLINES: Record<ItemRingRelink['by'], string> = {
  register: 'Neighbour registered',
  register_subname: 'Neighbour registered a sub-name',
  recycle: 'Neighbour recycled',
  unattributed: 'Ring pointer changed',
};

export interface ItemFeedHeadline {
  text: string;
  /** Someone else's action that only rewrote this item's cell (a ring neighbour): dimmed. */
  neighbour: boolean;
}

/** One item's own feed: its kind, and the protocol name for standards with protocol actions. */
export interface ItemFeedSubject {
  kind: 'identity' | 'object';
  /** `.cell`: its rows are named by the item's own protocol action. */
  protocolName?: string;
}

/**
 * Headline of a row in one item's own feed. The API states what the tx meant for this item
 * (`itemContext.protocolAction` / `ringRelink`); this only displays it (ID-C1, #5). A ring
 * neighbour's rewrite is dimmed; a `.cell` row is its own protocol action; any other standard is
 * named by its exact item action ("Identity Renew").
 */
export function itemFeedHeadline(activity: ItemActivity, item: ItemFeedSubject): ItemFeedHeadline {
  const context = activity.itemContext;
  if (!context?.action)
    throw new Error(`${activity.txHash}: item feed row has no item action for ${item.kind}`);
  if (context.ringRelink) {
    const text = NEIGHBOUR_HEADLINES[context.ringRelink.by];
    if (!text)
      throw new Error(
        `${activity.txHash}: unknown ring relink ${JSON.stringify(context.ringRelink.by)}`
      );
    return { text, neighbour: true };
  }
  if (item.protocolName && context.protocolAction)
    return {
      text: `${item.protocolName} · ${context.protocolAction.replaceAll('_', ' ')}`,
      neighbour: false,
    };
  return {
    text: `${item.kind === 'identity' ? 'Identity' : 'Object'} ${capitalizeAction(context.action)}`,
    neighbour: false,
  };
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
  context?: { label: string; render: (tx: Transaction) => ReactNode };
}
export function AssetTransactions({ rows, token, context }: AssetTransactionsProps) {
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
          <Hash hash={r.hash} copyable={false} startChars={10} endChars={6} className="text-xs" />
        </AppLink>
      ),
    },
    {
      key: 'block',
      label: 'Block · age',
      minWidth: 160,
      render: (r) => (
        <>
          <AppLink href={`/blocks/${r.blockNumber}`}>
            #{r.blockNumber.toLocaleString('en-US')}
          </AppLink>
          <span className="text-text-dim"> · {formatTimeAgo(r.timestamp)}</span>
          <span className="text-text-dim block text-[11px]">tx {r.index} in block</span>
        </>
      ),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 70,
      numeric: true,
      render: (r) => `${r.inputsCount} → ${r.outputsCount}`,
    },
  ];
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
      minWidth: 112,
      render: (r) => (
        <ResourceBar value={r.txSize!} scale={sizeScale} label="Serialized bytes" stacked />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
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
  if (context)
    columns.push({ key: 'context', label: context.label, minWidth: 180, render: context.render });
  return (
    <ListTable
      label="Asset transactions"
      rows={rows}
      columns={columns}
      rowKey={(r) => r.hash}
      empty="No transactions."
    />
  );
}
export interface AssetFeedProps {
  queryKey: string;
  scope: string;
  initialActivityCursor?: string;
  onActivityCursorChange?: (cursor: string | undefined) => void;
  loadActivities: (params: {
    limit: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<GlobalActivity>>;
  loadTransactions: (params: {
    limit: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<Transaction>>;
  token?: AssetTransactionsProps['token'];
  context?: AssetTransactionsProps['context'];
  /** One item's own feed: every row carries its item context (see itemFeedHeadline). */
  item?: ItemFeedSubject;
}
export function AssetFeed({
  queryKey,
  scope,
  loadActivities,
  loadTransactions,
  token,
  context,
  initialActivityCursor,
  onActivityCursorChange,
  item,
}: AssetFeedProps) {
  const [tab, setTab] = useState<'activities' | 'transactions'>('activities');
  const actsPages = useCursorPagination(),
    txPages = useCursorPagination();
  const activityCursor = actsPages.cursor ?? initialActivityCursor;
  useEffect(() => {
    onActivityCursorChange?.(activityCursor);
  }, [activityCursor, onActivityCursorChange]);
  const acts = useQuery({
    queryKey: ['asset-feed', queryKey, 'activities', activityCursor],
    queryFn: () => loadActivities({ limit: 50, cursor: activityCursor }),
    enabled: tab === 'activities',
  });
  const txs = useQuery({
    queryKey: ['asset-feed', queryKey, 'transactions', txPages.cursor],
    queryFn: () => loadTransactions({ limit: 50, cursor: txPages.cursor }),
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
            className={`border-b-2 py-3 text-[15px] ${tab === t ? 'border-jade text-text-bright' : 'text-text-dim border-transparent'}`}
          >
            {t === 'activities' ? 'Activities' : 'Transactions'}
          </button>
        ))}
        <span className="text-text-dim pb-2 text-[11px] sm:ml-auto">{scope} · newest first</span>
      </div>
      {current.data ? (
        <>
          <AssetReadState error={current.error} label={tab} retained />
          {tab === 'activities' ? (
            acts.data!.data.length ? (
              acts.data!.data.map((a) => {
                let headline: ItemFeedHeadline | undefined;
                try {
                  headline = item && itemFeedHeadline(a as ItemActivity, item);
                } catch (error) {
                  // A row that breaks the feed contract is reported in place; the page stays up.
                  return (
                    <div key={a.txHash} className="border-base-elevated border-b py-3">
                      <ActivityReadError txHash={a.txHash} error={error} />
                    </div>
                  );
                }
                return (
                  <ActivityRow
                    key={a.txHash}
                    activity={a}
                    headline={headline?.text}
                    className={headline?.neighbour ? 'opacity-[0.45]' : undefined}
                  />
                );
              })
            ) : (
              <p className="text-text-dim py-6 text-xs">No activities.</p>
            )
          ) : (
            <AssetTransactions rows={txs.data!.data} token={token} context={context} />
          )}
          <CursorPagination
            total={current.data.total}
            totalLabel="transactions"
            pageSize={50}
            page={pages.page}
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
