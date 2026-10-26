'use client';

import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { ActivityRow, ActivityRowHead } from '@/components/activity/activity-row';
export { formatOccupiedDelta } from '@/components/activity/activity-row';
import { useFreshKeys } from '@/hooks/useFreshKeys';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { api, type Transaction } from '@/lib/api';
import { cn } from '@/lib/utils';

/** The feed always shows this many rows: no inner scroll, no row cut mid-way. */
export const FEED_ROWS = 8;

type FeedTab = 'activity' | 'transactions';

const TABS: { id: FeedTab; label: string; viewAll: string }[] = [
  { id: 'activity', label: 'Latest activity', viewAll: '/activities' },
  { id: 'transactions', label: 'Transactions', viewAll: '/transactions' },
];

/**
 * The homepage feed: latest activity (what each tx did and who it moved CKB for) or the latest
 * transactions, one tab at a time, in compact two-line rows. Both lists stay live while hidden:
 * activity polls, and transactions also take the realtime socket's new_transaction pushes.
 */
export function ActivityFeed() {
  const [tab, setTab] = useState<FeedTab>('activity');
  const activities = useQuery({
    queryKey: ['latest-activities', FEED_ROWS],
    queryFn: () => api.getLatestActivities(FEED_ROWS),
    refetchInterval: 10000,
  });
  // The key the realtime socket writes new transactions into (useRealtimeData).
  const transactions = useQuery({
    queryKey: ['latest-transactions'],
    queryFn: () => api.getTransactions({ limit: 10 }),
    refetchInterval: 10000,
  });
  const active = TABS.find((t) => t.id === tab) as (typeof TABS)[number];

  return (
    <div data-testid="home-feed" className="flex min-w-0 flex-col">
      <div className="border-base-border-subtle flex items-center justify-between gap-4 border-b">
        <div role="group" aria-label="Feed" className="flex gap-4 sm:gap-6">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              aria-pressed={tab === t.id}
              onClick={() => setTab(t.id)}
              className={cn(
                '-mb-px h-11 whitespace-nowrap border-b-2 bg-transparent p-0 text-sm transition-colors sm:text-[15px]',
                tab === t.id
                  ? 'border-jade text-text-bright font-medium'
                  : 'text-text-dim hover:text-text border-transparent'
              )}
            >
              {t.label}
            </button>
          ))}
        </div>
        <Link
          href={active.viewAll}
          className="text-interactive hover:text-jade whitespace-nowrap text-xs transition-colors"
        >
          View all →
        </Link>
      </div>
      {/* Keyed by tab: a tab switch starts a fresh "new rows" baseline. */}
      {tab === 'activity' ? (
        <FeedList
          key="activity"
          query={activities}
          what="activity"
          keyOf={(a) => a.txHash}
          row={(a, fresh) => <ActivityRow activity={a} fresh={fresh} />}
          items={activities.data}
        />
      ) : (
        <FeedList
          key="transactions"
          query={transactions}
          what="transactions"
          keyOf={(t) => t.hash}
          row={(t, fresh) => <TxRow tx={t} fresh={fresh} />}
          items={transactions.data?.data}
        />
      )}
    </div>
  );
}

const ROW_CLASS = 'flex flex-col gap-[3px] border-b border-base-elevated py-3 transition-colors';

function FeedList<T>({
  query,
  items,
  what,
  keyOf,
  row,
}: {
  query: { error: Error | null };
  items: T[] | undefined;
  what: string;
  keyOf: (item: T) => string;
  row: (item: T, fresh: boolean) => ReactNode;
}) {
  const visible = items ? items.slice(0, FEED_ROWS) : null;
  const fresh = useFreshKeys(visible ? visible.map(keyOf) : null);
  if (query.error && !visible) {
    return (
      <div role="alert" className="text-rouge py-3 text-xs">
        {`Failed to load the latest ${what}: ${query.error.message}`}
      </div>
    );
  }
  if (!visible) {
    return (
      <div aria-hidden="true" data-testid="feed-loading">
        {Array.from({ length: FEED_ROWS }, (_, i) => (
          <div key={i} className={ROW_CLASS}>
            <div className="flex justify-between">
              <span className="bg-base-elevated h-4 w-44 animate-pulse rounded" />
              <span className="bg-base-elevated h-3 w-12 animate-pulse rounded" />
            </div>
            <div className="flex justify-between">
              <span className="bg-base-elevated h-3 w-56 animate-pulse rounded" />
              <span className="bg-base-elevated h-3 w-32 animate-pulse rounded" />
            </div>
          </div>
        ))}
      </div>
    );
  }
  if (visible.length === 0) {
    return <div className="text-text-dim py-3 text-xs">{`No ${what} yet.`}</div>;
  }
  return (
    <ul className="m-0 list-none p-0" data-testid="feed-rows">
      {visible.map((item) => {
        const key = keyOf(item);
        return <li key={key}>{row(item, fresh.has(key))}</li>;
      })}
    </ul>
  );
}

const plural = (n: number, noun: string) => `${noun}${n === 1 ? '' : 's'}`;

function TxRow({ tx, fresh }: { tx: Transaction; fresh: boolean }) {
  // Cross-highlight with the transaction pipeline (hovered or pinned there, or hovered here).
  const hoverTx = usePipelineHoverStore((s) => s.hoverTx);
  const pinned = usePipelineHoverStore((s) => s.pinned);
  const setHover = usePipelineHoverStore((s) => s.setHover);
  const clearHover = usePipelineHoverStore((s) => s.clearHover);
  const highlighted = hoverTx === tx.hash || pinned === tx.hash;
  const count = 'inline-block min-w-[3ch] text-right tabular-nums';
  return (
    <article
      data-feed-row={tx.hash}
      data-tx-row={tx.hash}
      data-highlighted={highlighted}
      // A committed tx anchors the proposal window on the block that committed it.
      onPointerEnter={() =>
        setHover({ block: tx.blockNumber, tx: tx.hash, window: tx.blockNumber })
      }
      onPointerLeave={clearHover}
      className={cn(ROW_CLASS, fresh && 'bg-jade/5', highlighted && 'bg-base-elevated/70')}
    >
      <ActivityRowHead
        title={
          <>
            <span className={count}>{tx.inputsCount}</span>
            <span className="text-text font-normal"> {plural(tx.inputsCount, 'input')} → </span>
            <span className={count}>{tx.outputsCount}</span>
            <span className="text-text font-normal"> {plural(tx.outputsCount, 'output')}</span>
          </>
        }
        hash={tx.hash}
        blockNumber={tx.blockNumber}
        timestamp={tx.timestamp}
      />
    </article>
  );
}
