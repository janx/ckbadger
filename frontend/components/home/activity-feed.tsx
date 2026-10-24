'use client';

import { Fragment, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import {
  DotJoined,
  ExactCkb,
  ItemDeltaFact,
  LockCallExpr,
  TypeCallExpr,
  activityHeadline,
  globalTxEvents,
  protocolActionLabel,
  protocolActionNoteParts,
} from '@/components/activity-event-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { classifyActivity } from '@/lib/activity-classify';
import { formatSignedCkbExact, formatSignedCkbInteger, SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import { PARTICIPANT_LIMIT, participantsToShow } from '@/lib/participant-order';
import { useFreshKeys } from '@/hooks/useFreshKeys';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { api, type GlobalActivity, type ParticipantInfo, type Transaction } from '@/lib/api';
import { cn, formatTimeAgo, truncateHash } from '@/lib/utils';

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
                '-mb-px h-11 whitespace-nowrap border-b-2 bg-transparent p-0 text-[15px] transition-colors',
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

/**
 * Line 1 of a row: the primary fact on the left (the Layer-3 narrative for an activity), the raw
 * evidence quiet on the right — block, tx hash, age — always there to verify it against.
 */
function RowHead({
  title,
  hash,
  blockNumber,
  timestamp,
}: {
  title: ReactNode;
  hash: string;
  blockNumber: number;
  timestamp: string;
}) {
  return (
    <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-0.5">
      <span data-role="headline" className="text-text-bright text-[13.5px] font-medium">
        {title}
      </span>
      <span data-role="evidence" className="text-text-dim shrink-0 text-[11.5px]">
        <Link href={`/blocks/${blockNumber}`} className="hover:text-text transition-colors">
          #{blockNumber.toLocaleString('en-US')}
        </Link>
        {' · '}
        <Link href={`/tx/${hash}`} className="hover:text-jade text-[#6aa9c4] transition-colors">
          {truncateHash(hash, 10, 6)}
        </Link>
        {' · '}
        {formatTimeAgo(timestamp)}
      </span>
    </div>
  );
}

/** A detail line under the headline, indented: a short dim label column, then the text. */
function DetailLine({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div data-role="detail" className="flex items-baseline gap-2 pl-3.5 text-xs">
      <span className="text-text-dim w-[30px] shrink-0 text-[11px]">{label}</span>
      <span className="min-w-0 [overflow-wrap:anywhere]">{children}</span>
    </div>
  );
}

/**
 * The detail lines of an activity, from the one event classification: each protocol action's
 * metadata (the headline's own action needs no name; another one leads with its label), then
 * every type call and every lock call in call syntax, args kept.
 */
function ActivityDetails({ activity }: { activity: GlobalActivity }) {
  const primary = classifyActivity(activity).primaryProtocolAction;
  const events = globalTxEvents(activity);
  const types = events.flatMap((e) => (e.kind === 'type' ? [e.call] : []));
  const locks = events.flatMap((e) => (e.kind === 'lock' ? [e.call] : []));
  const lines: ReactNode[] = [];
  events.forEach((e, i) => {
    if (e.kind !== 'protocol') return;
    const parts = protocolActionNoteParts(e.action);
    const named = e.action === primary ? parts : [protocolActionLabel(e.action), ...parts];
    if (named.length > 0) {
      lines.push(
        <DetailLine key={`p${i}`} label="">
          <DotJoined parts={named} />
        </DetailLine>
      );
    }
  });
  if (types.length > 0) {
    lines.push(
      <DetailLine key="type" label="type">
        <DotJoined
          parts={types.map((sc, i) => (
            <span key={i} className="whitespace-nowrap">
              <TypeCallExpr sc={sc} quiet />
            </span>
          ))}
        />
      </DetailLine>
    );
  }
  if (locks.length > 0) {
    lines.push(
      <DetailLine key="lock" label="lock">
        <DotJoined
          parts={locks.map((lc, i) => (
            <span key={i} className="whitespace-nowrap">
              <LockCallExpr lc={lc} quiet />
            </span>
          ))}
        />
      </DetailLine>
    );
  }
  return <>{lines}</>;
}

/** The change in occupied capacity, whole CKB (it is bytes), exact otherwise. */
export function formatOccupiedDelta(usedDelta: string): string {
  const shannons = BigInt(usedDelta);
  return shannons % SHANNONS_PER_CKB === BigInt(0)
    ? formatSignedCkbInteger(shannons)
    : formatSignedCkbExact(shannons);
}

const PARTICIPANT_GRID =
  'grid grid-cols-[minmax(0,1fr)_max-content] items-baseline gap-x-3 gap-y-0.5 pl-3.5 text-xs sm:grid-cols-[minmax(0,136px)_minmax(150px,max-content)_minmax(0,104px)_minmax(0,1fr)]';

function partyKey(p: ParticipantInfo, i: number): string {
  return `${p.lockHash ?? p.lockHashPrefix ?? p.address ?? 'party'}:${i}`;
}

/**
 * Every participant, Layer 1 and Layer 2: the party, its exact CKB change (decimal points lined
 * up), its occupied change when there is one, and its item changes. Named parties with no change
 * are listed — their zero is the fact. Over six, the five largest and an explicit "+N" to expand.
 */
function Participants({ participants }: { participants: ParticipantInfo[] }) {
  const [expanded, setExpanded] = useState(false);
  const indexed = participants.map((p, i) => ({ ...p, key: partyKey(p, i) }));
  const { shown, hidden } = participantsToShow(indexed, expanded);
  const foldable = participants.length > PARTICIPANT_LIMIT;
  if (participants.length === 0) return null;
  return (
    <div data-role="participants" className={PARTICIPANT_GRID}>
      {shown.map((p) => (
        <Fragment key={p.key}>
          <div data-party={p.key} className="col-start-1 min-w-0 overflow-hidden">
            <ParticipantRefView participant={p} compact quiet />
          </div>
          <div className="col-start-2 text-right">
            <ExactCkb shannons={BigInt(p.ckbDelta)} />
          </div>
          {p.usedDelta !== '0' && (
            <div
              data-role="occupied"
              className="text-text-dim col-start-2 text-right text-[11.5px] sm:col-start-3"
            >
              occupied {formatOccupiedDelta(p.usedDelta)}
            </div>
          )}
          {p.itemDeltas.length > 0 && (
            <div className="col-span-2 col-start-1 sm:col-span-1 sm:col-start-4">
              <DotJoined
                parts={p.itemDeltas.map((item, i) => (
                  <ItemDeltaFact key={i} item={item} />
                ))}
              />
            </div>
          )}
        </Fragment>
      ))}
      {foldable && (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((e) => !e)}
          className="text-interactive hover:text-jade col-span-full justify-self-start bg-transparent py-1 text-xs transition-colors"
        >
          {expanded ? 'Show fewer participants' : `+${hidden} participants`}
        </button>
      )}
    </div>
  );
}

function ActivityRow({ activity, fresh }: { activity: GlobalActivity; fresh: boolean }) {
  return (
    <article data-feed-row={activity.txHash} className={cn(ROW_CLASS, fresh && 'bg-jade/5')}>
      <RowHead
        title={activityHeadline(activity)}
        hash={activity.txHash}
        blockNumber={activity.blockNumber}
        timestamp={activity.timestamp}
      />
      <ActivityDetails activity={activity} />
      <Participants participants={activity.participants} />
    </article>
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
      <RowHead
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
