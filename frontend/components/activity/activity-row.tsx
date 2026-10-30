'use client';
import { Fragment, useState, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import {
  ActivityReadError,
  DotJoined,
  ExactCkb,
  ItemDeltaFact,
  LockCallExpr,
  TypeCallExpr,
  activityClassMark,
  activityHeadline,
  globalTxEvents,
  protocolActionLabel,
  protocolActionNoteParts,
} from '@/components/activity-event-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { classifyActivity } from '@/lib/activity-classify';
import { formatSignedCkbExact, formatSignedCkbInteger, SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import { PARTICIPANT_LIMIT, participantsToShow } from '@/lib/participant-order';
import type { GlobalActivity, ParticipantInfo, PoolStatus, PoolInterpretation } from '@/lib/api';
import { PoolStatusBadge, TimeInPool, PoolInterpretationNotice } from '@/components/ui/pool-status';
import type { ActivityBody } from '@/lib/activity-classify';
import type { AssetClass } from '@/lib/design-tokens';
import { cn, formatTimeAgo, truncateHash } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
export type ActivityParticipant = ParticipantInfo & { lockKind?: string | null };
const ROW_CLASS = 'flex flex-col gap-[3px] border-b border-base-elevated py-3 transition-colors';
/**
 * Line 1 of a row: the primary fact on the left (the Layer-3 narrative for an activity), the raw
 * evidence quiet on the right — block, tx hash, age — always there to verify it against.
 */
export function ActivityRowHead({
  title,
  hash,
  blockNumber,
  timestamp,
  showEvidence = true,
  poolStatus,
  timeAddedToPool,
  evidence,
}: {
  /** F2 blocks: replaces the default "#block · hash · age" (a block page names the tx index). */
  evidence?: ReactNode;
  title: ReactNode;
  hash: string;
  blockNumber: number | null;
  timestamp: string | null;
  poolStatus?: PoolStatus;
  timeAddedToPool?: string;
  showEvidence?: boolean;
}) {
  return (
    <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-0.5">
      <span data-role="headline" className="text-text-bright text-[13px] font-medium">
        {title}
      </span>
      {showEvidence && evidence !== undefined && (
        <span data-role="evidence" className="text-text-dim shrink-0 text-[11px]">
          {evidence}
        </span>
      )}
      {showEvidence && evidence === undefined && (
        <span data-role="evidence" className="text-text-dim shrink-0 text-[11px]">
          {blockNumber !== null ? (
            <Link href={`/blocks/${blockNumber}`} className="hover:text-text transition-colors">
              {formatBlockNumber(blockNumber)}
            </Link>
          ) : poolStatus ? (
            <PoolStatusBadge status={poolStatus} />
          ) : (
            <span>Unconfirmed</span>
          )}
          {' · '}
          <Link href={`/tx/${hash}`} className="hover:text-jade text-text-dim transition-colors">
            {truncateHash(hash, 10, 6)}
          </Link>
          {' · '}
          {timestamp !== null ? (
            formatTimeAgo(timestamp)
          ) : timeAddedToPool ? (
            <TimeInPool since={timeAddedToPool} />
          ) : (
            'Time unavailable'
          )}
        </span>
      )}
    </div>
  );
}

/** A detail line under the headline, indented: a short dim label column, then the text. */
function DetailLine({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div data-role="detail" className="text-text flex items-baseline gap-2 pl-3.5 text-xs">
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
function ActivityDetails({
  activity,
  showScriptDetails = true,
}: {
  activity: ActivityBody & { txHash: string };
  showScriptDetails?: boolean;
}) {
  const primary = classifyActivity(activity).primaryProtocolAction;
  const events = globalTxEvents(activity);
  const types = events.flatMap((e) => (e.kind === 'type' ? [e.call] : []));
  const locks = events.flatMap((e) => (e.kind === 'lock' ? [e.call] : []));
  const lines: ReactNode[] = [];
  events.forEach((e, i) => {
    if (e.kind !== 'protocol') return;
    let parts: ReactNode[];
    try {
      parts = protocolActionNoteParts(e.action);
    } catch (error) {
      lines.push(
        <DetailLine key={`p${i}`} label="">
          <ActivityReadError txHash={activity.txHash} error={error} />
        </DetailLine>
      );
      return;
    }
    const named = e.action === primary ? parts : [protocolActionLabel(e.action), ...parts];
    if (named.length > 0) {
      lines.push(
        <DetailLine key={`p${i}`} label="">
          <DotJoined parts={named} />
        </DetailLine>
      );
    }
  });
  if (showScriptDetails && types.length > 0) {
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
  if (showScriptDetails && locks.length > 0) {
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
// ── F2 objects (round 2): the asset boards' participant line — lock kind as its own unwrapped
// column (IV-14, OB-12), and a batch of objects folded behind "+N more objects" (OB-M3). ──
const PARTICIPANT_GRID_LOCK =
  'grid grid-cols-[minmax(0,1fr)_max-content] items-baseline gap-x-3 gap-y-0.5 pl-3.5 text-xs sm:grid-cols-[minmax(0,136px)_minmax(0,150px)_minmax(150px,max-content)_minmax(0,104px)_minmax(0,1fr)]';
function FoldedItemDeltas({
  items,
  focusItemId,
}: {
  items: ParticipantInfo['itemDeltas'];
  focusItemId?: string;
}) {
  const [open, setOpen] = useState(false);
  const objects = items.filter((i) => i.kind === 'object');
  if (open || objects.length < 2)
    return (
      <DotJoined
        parts={items.map((item, i) => (
          <ItemDeltaFact key={i} item={item} />
        ))}
      />
    );
  const lead = objects.find((o) => o.kind === 'object' && o.objectId === focusItemId) ?? objects[0];
  const shown = items.filter((i) => i.kind !== 'object' || i === lead);
  return (
    <span className="inline-flex flex-wrap items-center gap-x-3 gap-y-0.5">
      <DotJoined
        parts={shown.map((item, i) => (
          <ItemDeltaFact key={i} item={item} />
        ))}
      />
      <button
        type="button"
        aria-expanded={false}
        onClick={() => setOpen(true)}
        className="border-base-border text-text hover:text-text-bright h-6 rounded-md border px-2 text-[11.5px]"
      >
        +{objects.length - 1} more object{objects.length - 1 === 1 ? '' : 's'}
      </button>
    </span>
  );
}

function Participants({
  participants,
  lockColumn = false,
  foldObjects = false,
  focusItemId,
}: {
  participants: ActivityParticipant[];
  lockColumn?: boolean;
  foldObjects?: boolean;
  focusItemId?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const indexed = participants.map((p, i) => ({ ...p, key: partyKey(p, i) }));
  const { shown, hidden } = participantsToShow(indexed, expanded);
  const foldable = participants.length > PARTICIPANT_LIMIT;
  if (participants.length === 0) return null;
  return (
    <div data-role="participants" className={lockColumn ? PARTICIPANT_GRID_LOCK : PARTICIPANT_GRID}>
      {shown.map((p) => (
        <Fragment key={p.key}>
          <div data-party={p.key} className="col-start-1 min-w-0 overflow-hidden">
            <ParticipantRefView participant={p} compact quiet />
            {!lockColumn && p.lockKind && (
              <span className="text-text-dim ml-2 text-[11px]">{p.lockKind}</span>
            )}
          </div>
          {lockColumn && (
            <div
              data-role="lock-kind"
              className="text-text-dim hidden min-w-0 truncate whitespace-nowrap text-[11.5px] sm:col-start-2 sm:block"
              title={p.lockKind ?? undefined}
            >
              {p.lockKind ?? ''}
            </div>
          )}
          <div className={cn('col-start-2 text-right', lockColumn && 'sm:col-start-3')}>
            <ExactCkb shannons={BigInt(p.ckbDelta)} />
          </div>
          {p.usedDelta !== '0' && (
            <div
              data-role="occupied"
              className={cn(
                'text-text-dim col-start-2 text-right text-[11px]',
                lockColumn ? 'sm:col-start-4' : 'sm:col-start-3'
              )}
            >
              occupied {formatOccupiedDelta(p.usedDelta)}
            </div>
          )}
          {p.itemDeltas.length > 0 && (
            <div
              className={cn(
                'col-span-2 col-start-1 sm:col-span-1',
                lockColumn ? 'sm:col-start-5' : 'sm:col-start-4'
              )}
            >
              {foldObjects ? (
                <FoldedItemDeltas items={p.itemDeltas} focusItemId={focusItemId} />
              ) : (
                <DotJoined
                  parts={p.itemDeltas.map((item, i) => (
                    <ItemDeltaFact key={i} item={item} />
                  ))}
                />
              )}
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

export type ActivityRowData = Omit<GlobalActivity, 'blockNumber' | 'timestamp' | 'txIndex'> & {
  blockNumber: number | null;
  timestamp: string | null;
  poolStatus?: PoolStatus;
  timeAddedToPool?: string;
  interpretation?: PoolInterpretation;
};
export type ActivityRowProps = {
  activity: ActivityRowData;
  fresh?: boolean;
  /** Participant endpoints cannot infer whole-transaction mint/burn from a single delta. */
  headline?: ReactNode;
  className?: string;
  /** F2 identities (round 2, ID-C13): the row's class mark when the feed knows it (an item feed). */
  mark?: AssetClass | null;
  showParticipants?: boolean;
  showScriptDetails?: boolean;
  showEvidence?: boolean;
  /** F2 blocks: the row's own evidence line (see `ActivityRowHead.evidence`). */
  evidence?: ReactNode;
  /** F2 objects: lock kind in its own column (IV-14, OB-12). */
  lockColumn?: boolean;
  /** F2 objects: fold a batch of objects behind "+N more objects", leading with `focusItemId` (OB-M3). */
  foldObjects?: boolean;
  focusItemId?: string;
} & (
  | { mode?: 'cross-user'; participant?: never }
  | { mode: 'participant'; participant: ParticipantInfo }
);

export function ActivityRow({
  activity,
  fresh = false,
  headline,
  className,
  mark: markOverride,
  mode = 'cross-user',
  participant,
  showParticipants = true,
  showScriptDetails = true,
  showEvidence = true,
  evidence,
  lockColumn = false,
  foldObjects = false,
  focusItemId,
}: ActivityRowProps) {
  const shown =
    mode === 'participant' && participant ? { ...activity, participants: [participant] } : activity;
  const mark = markOverride === undefined ? activityClassMark(activity) : markOverride;
  return (
    <article
      data-feed-row={activity.txHash}
      className={cn(ROW_CLASS, fresh && 'bg-jade/5', className)}
    >
      <ActivityRowHead
        title={
          <>
            {mark && <ClassMark kind={mark} className="mr-2" />}
            {headline ?? activityHeadline(activity)}
          </>
        }
        hash={activity.txHash}
        blockNumber={activity.blockNumber}
        timestamp={activity.timestamp}
        showEvidence={showEvidence}
        evidence={evidence}
        poolStatus={activity.poolStatus}
        timeAddedToPool={activity.timeAddedToPool}
      />
      <PoolInterpretationNotice interpretation={activity.interpretation} />
      <ActivityDetails activity={activity} showScriptDetails={showScriptDetails} />
      {showParticipants && (
        <Participants
          participants={shown.participants}
          lockColumn={lockColumn}
          foldObjects={foldObjects}
          focusItemId={focusItemId}
        />
      )}
    </article>
  );
}
