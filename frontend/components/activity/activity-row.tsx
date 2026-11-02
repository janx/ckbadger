'use client';
import { useState, type ReactNode } from 'react';
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
import type {
  ActivityProtocolAction,
  GlobalActivity,
  ParticipantInfo,
  PoolStatus,
  PoolInterpretation,
} from '@/lib/api';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
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
      <span data-role="headline" className="text-text-bright text-body font-medium">
        {title}
      </span>
      {showEvidence && evidence !== undefined && (
        <span data-role="evidence" className="text-text-dim text-meta shrink-0">
          {evidence}
        </span>
      )}
      {showEvidence && evidence === undefined && (
        <span data-role="evidence" className="text-text-dim text-meta shrink-0">
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
    <div data-role="detail" className="text-text text-small flex items-baseline gap-2 pl-3.5">
      <span className="text-text-dim text-meta w-[30px] shrink-0">{label}</span>
      <span className="min-w-0 [overflow-wrap:anywhere]">{children}</span>
    </div>
  );
}

/**
 * What a protocol-action line draws, as a key: the label and every metadata field the line reads
 * (`protocolActionNoteParts` reads `protocolActionMeta`, the protocol and the action). Two actions
 * with one key read the same, whatever else their metadata carries.
 */
function protocolLineKey(pa: ActivityProtocolAction, isPrimary: boolean): string {
  return JSON.stringify(
    [isPrimary, protocolActionLabel(pa), pa.protocol, pa.action, protocolActionMeta(pa)],
    (_, v: unknown) => (typeof v === 'bigint' ? v.toString() : v)
  );
}

/**
 * A run of calls, dot-joined. Over PARTICIPANT_LIMIT, the first five and an explicit
 * "+N more locks" that opens the rest in place (B#26: 29 lock calls, one per line at 390).
 */
function FoldedCalls({ parts, noun }: { parts: ReactNode[]; noun: 'types' | 'locks' }) {
  const [open, setOpen] = useState(false);
  const folded = !open && parts.length > PARTICIPANT_LIMIT;
  const shown = folded ? parts.slice(0, PARTICIPANT_LIMIT - 1) : parts;
  return (
    <>
      <DotJoined parts={shown} />
      {folded && (
        <>
          {' '}
          <button
            type="button"
            aria-expanded={false}
            onClick={() => setOpen(true)}
            className="text-interactive hover:text-jade whitespace-nowrap bg-transparent transition-colors"
          >
            +{parts.length - shown.length} more {noun}
          </button>
        </>
      )}
    </>
  );
}

/**
 * The detail lines of an activity, from the one event classification: each protocol action's
 * metadata (the headline's own action needs no name; another one leads with its label), then
 * every type call and every lock call in call syntax, args kept. Action lines that read the same
 * are said once with their count ("×18"); a long call run folds behind "+N more".
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
  type ProtocolLine =
    | { key: string; parts: ReactNode[]; count: number }
    | { key: string; error: unknown };
  const protocolLines: ProtocolLine[] = [];
  events.forEach((e, i) => {
    if (e.kind !== 'protocol') return;
    let parts: ReactNode[];
    try {
      parts = protocolActionNoteParts(e.action);
    } catch (error) {
      protocolLines.push({ key: `p${i}`, error });
      return;
    }
    const named = e.action === primary ? parts : [protocolActionLabel(e.action), ...parts];
    if (named.length === 0) return;
    const key = protocolLineKey(e.action, e.action === primary);
    const same = protocolLines.find((l) => l.key === key);
    if (same && 'count' in same) same.count += 1;
    else protocolLines.push({ key, parts: named, count: 1 });
  });
  const lines: ReactNode[] = protocolLines.map((l) => (
    <DetailLine key={l.key} label="">
      {'error' in l ? (
        <ActivityReadError txHash={activity.txHash} error={l.error} />
      ) : (
        <>
          <DotJoined parts={l.parts} />
          {l.count > 1 && <span className="text-text-dim"> ×{l.count}</span>}
        </>
      )}
    </DetailLine>
  ));
  if (showScriptDetails && types.length > 0) {
    lines.push(
      <DetailLine key="type" label="type">
        <FoldedCalls
          noun="types"
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
        <FoldedCalls
          noun="locks"
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

// Below md each participant is a wrapping line (see `Participants`): the 640–767 band has one 608 px
// content width, too narrow for the 4/5-track rows (their item track started at x≈605). The tracks
// start at md.
const PARTICIPANT_GRID =
  'flex flex-col gap-y-0.5 pl-3.5 text-small md:grid md:grid-cols-[minmax(0,136px)_minmax(150px,max-content)_minmax(0,104px)_minmax(0,1fr)] md:items-baseline md:gap-x-3';

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
  'flex flex-col gap-y-0.5 pl-3.5 text-small md:grid md:grid-cols-[minmax(0,136px)_minmax(0,150px)_minmax(150px,max-content)_max-content_minmax(0,1fr)] md:items-baseline md:gap-x-3';
function FoldedItemDeltas({
  items,
  focusItemId,
  itemTone,
}: {
  items: ParticipantInfo['itemDeltas'];
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
}) {
  const [open, setOpen] = useState(false);
  const objects = items.filter((i) => i.kind === 'object');
  if (open || objects.length < 2)
    return (
      <DotJoined
        parts={items.map((item, i) => (
          <ItemDeltaFact key={i} item={item} tone={itemTone} />
        ))}
      />
    );
  const lead = objects.find((o) => o.kind === 'object' && o.objectId === focusItemId) ?? objects[0];
  const shown = items.filter((i) => i.kind !== 'object' || i === lead);
  return (
    <span className="inline-flex flex-wrap items-center gap-x-3 gap-y-0.5">
      <DotJoined
        parts={shown.map((item, i) => (
          <ItemDeltaFact key={i} item={item} tone={itemTone} />
        ))}
      />
      <button
        type="button"
        aria-expanded={false}
        onClick={() => setOpen(true)}
        className="border-base-border text-text hover:text-text-bright text-meta h-6 rounded-md border px-2"
      >
        +{objects.length - 1} more object{objects.length - 1 === 1 ? '' : 's'}
      </button>
    </span>
  );
}

/** "+2 objects", "−1 object", "+1 −1 objects": a participant's object changes as one signed count. */
export function objectCountLabel(items: ParticipantInfo['itemDeltas']): string {
  const objects = items.filter((i) => i.kind === 'object');
  const arrived = objects.filter((o) => o.delta > 0).length;
  const departed = objects.length - arrived;
  const counts = [arrived > 0 && `+${arrived}`, departed > 0 && `−${departed}`].filter(Boolean);
  return `${counts.join(' ')} object${objects.length === 1 ? '' : 's'}`;
}

/**
 * One participant. Below md one wrapping line: the party unit, the amount at the right end (it
 * drops under the party when the line cannot hold both), then occupied and the item changes. From
 * md the wrappers dissolve (`md:contents`) and every cell takes its track of the shared grid.
 *
 * The asset boards' phone form (`foldObjects`, B#12): occupied joins the line, and the objects are
 * one count chip ("+2 objects") beside the party that opens them in place; tokens and identities
 * stay as facts. From md the objects fold as before ("+N more objects").
 */
function ParticipantLine({
  p,
  lockColumn,
  foldObjects,
  focusItemId,
  itemTone,
  notes,
}: {
  p: ActivityParticipant & { key: string };
  lockColumn: boolean;
  foldObjects: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  notes?: ReadonlyMap<string, string>;
}) {
  const [objectsOpen, setObjectsOpen] = useState(false);
  const phoneObjects = foldObjects && p.itemDeltas.some((i) => i.kind === 'object');
  const otherItems = p.itemDeltas.filter((i) => i.kind !== 'object');
  return (
    <div
      data-role="participant"
      className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 md:contents"
    >
      {/* The party as one unit below md: it takes the line before the amount does, and its
          lock kind never splits; only when address, roles and lock kind cannot share one line
          does the lock kind start the next line, whole, under the address. */}
      <div
        data-role="party-unit"
        className="flex max-w-full flex-wrap items-baseline gap-x-2 gap-y-0.5 md:contents"
      >
        <div
          data-party={p.key}
          className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 md:col-start-1 md:block md:min-w-0 md:overflow-hidden"
        >
          <ParticipantRefView participant={p} compact quiet />
          {!lockColumn && p.lockKind && (
            <span className="text-text-dim text-meta shrink-0 whitespace-nowrap md:ml-2">
              {p.lockKind}
            </span>
          )}
          {p.lockHash !== null && notes?.has(p.lockHash) && (
            <span data-role="participant-note" className="text-text-dim text-meta md:ml-2">
              {notes.get(p.lockHash)}
            </span>
          )}
        </div>
        {/* Below md the tail of the party unit (the narrow form never drops it, C#8); from md
            its own column. */}
        {lockColumn && (
          <div
            data-role="lock-kind"
            className="text-text-dim text-meta shrink-0 whitespace-nowrap md:col-start-2 md:block md:min-w-0 md:truncate"
            title={p.lockKind ?? undefined}
          >
            {p.lockKind ?? ''}
          </div>
        )}
      </div>
      {phoneObjects && (
        <span
          data-role="phone-items"
          className="inline-flex flex-wrap items-baseline gap-x-2 md:hidden"
        >
          {otherItems.length > 0 && (
            <DotJoined
              parts={otherItems.map((item, i) => (
                <ItemDeltaFact key={i} item={item} tone={itemTone} />
              ))}
            />
          )}
          <button
            type="button"
            data-role="object-count"
            aria-expanded={objectsOpen}
            onClick={() => setObjectsOpen((open) => !open)}
            className="border-base-border text-text hover:text-text-bright text-meta h-6 whitespace-nowrap rounded-md border px-2"
          >
            {objectCountLabel(p.itemDeltas)}
          </button>
        </span>
      )}
      <div className={cn('col-start-2 ml-auto text-right md:ml-0', lockColumn && 'md:col-start-3')}>
        <ExactCkb shannons={BigInt(p.ckbDelta)} />
      </div>
      {p.usedDelta !== '0' && (
        <div
          data-role="occupied"
          className={cn(
            'text-text-dim text-meta col-start-2 whitespace-nowrap text-right md:basis-auto',
            // The asset boards' phone form keeps occupied on the participant's line.
            !foldObjects && 'basis-full',
            lockColumn ? 'md:col-start-4' : 'md:col-start-3'
          )}
        >
          occupied {formatOccupiedDelta(p.usedDelta)}
        </div>
      )}
      {p.itemDeltas.length > 0 && (
        <div
          className={cn(
            'col-span-2 col-start-1 basis-full md:col-span-1 md:basis-auto',
            phoneObjects && 'hidden md:block',
            lockColumn ? 'md:col-start-5' : 'md:col-start-4'
          )}
        >
          {foldObjects ? (
            <FoldedItemDeltas items={p.itemDeltas} focusItemId={focusItemId} itemTone={itemTone} />
          ) : (
            <DotJoined
              parts={p.itemDeltas.map((item, i) => (
                <ItemDeltaFact key={i} item={item} tone={itemTone} />
              ))}
            />
          )}
        </div>
      )}
      {phoneObjects && objectsOpen && (
        <div data-role="phone-objects" className="basis-full pl-2 md:hidden">
          <DotJoined
            parts={p.itemDeltas
              .filter((i) => i.kind === 'object')
              .map((item, i) => (
                <ItemDeltaFact key={i} item={item} tone={itemTone} />
              ))}
          />
        </div>
      )}
    </div>
  );
}

function Participants({
  participants,
  lockColumn = false,
  foldObjects = false,
  focusItemId,
  itemTone,
  notes,
}: {
  participants: ActivityParticipant[];
  lockColumn?: boolean;
  foldObjects?: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  /** A fact the API attaches to one participant, by its lock hash (R-4 "holds the class cell"). */
  notes?: ReadonlyMap<string, string>;
}) {
  const [expanded, setExpanded] = useState(false);
  const indexed = participants.map((p, i) => ({ ...p, key: partyKey(p, i) }));
  const { shown, hidden } = participantsToShow(indexed, expanded);
  const foldable = participants.length > PARTICIPANT_LIMIT;
  if (participants.length === 0) return null;
  return (
    <div data-role="participants" className={lockColumn ? PARTICIPANT_GRID_LOCK : PARTICIPANT_GRID}>
      {shown.map((p) => (
        <ParticipantLine
          key={p.key}
          p={p}
          lockColumn={lockColumn}
          foldObjects={foldObjects}
          focusItemId={focusItemId}
          itemTone={itemTone}
          notes={notes}
        />
      ))}
      {foldable && (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((e) => !e)}
          className="text-interactive hover:text-jade text-small col-span-full self-start bg-transparent py-1 transition-colors md:justify-self-start"
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
  /** Token amounts by sign (jade +, rouge −): only the Address board draws them so (BA-35). */
  itemTone?: 'plain' | 'sign';
  /** Round 3 (R-4): a note the API attaches to one participant, keyed by its lock hash. */
  participantNotes?: ReadonlyMap<string, string>;
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
  itemTone = 'plain',
  participantNotes,
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
          itemTone={itemTone}
          notes={participantNotes}
        />
      )}
    </article>
  );
}
