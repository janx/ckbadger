'use client';
import { useState, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import {
  ActivityReadError,
  DotJoined,
  ExactCkb,
  ItemDeltaFact,
  activityClassMark,
  formatScriptRef,
  activityHeadline,
  globalTxEvents,
  protocolActionLabel,
  protocolActionNoteParts,
} from '@/components/activity-event-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { classifyActivity } from '@/lib/activity-classify';
import { formatSignedCkbExact, formatSignedCkbInteger, SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import { PARTICIPANT_LIMIT, participantsToShow } from '@/lib/participant-order';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { lockArgsLine } from '@/lib/lock-args';
import type {
  ActivityLockCall,
  ActivityProtocolAction,
  ActivityTypeCall,
  GlobalActivity,
  ItemDelta,
  ParticipantInfo,
  PoolStatus,
  PoolInterpretation,
} from '@/lib/api';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import { PoolStatusBadge, TimeInPool, PoolInterpretationNotice } from '@/components/ui/pool-status';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { rowTagClass } from '@/components/ui/row-tag';
import type { ActivityBody } from '@/lib/activity-classify';
import type { CompositionTier } from '@/lib/asset-utils';
import type { AssetClass } from '@/lib/design-tokens';
import { cn, formatTimeAgo, truncateHash } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { Utc8Date } from '@/components/ui/utc8-date';
export type ActivityParticipant = ParticipantInfo & { lockKind?: string | null };
// T7: 8 px padding, 2 px between the headline line and the participant lines.
const ROW_CLASS = 'flex flex-col gap-0.5 border-b border-base-elevated py-2 transition-colors';

/**
 * A row's hover (consistency C3, T7): the tx hash, and the block's age beside it — the age left
 * the visible row for the absolute date. A pool row has no block time, so its hover is the hash.
 */
export function activityRowTitle(hash: string, timestamp: string | null): string {
  return timestamp === null ? hash : `${hash} · ${formatTimeAgo(timestamp)}`;
}

/**
 * The block's date on the UTC+8 clock every dated row uses (consistency C3: one date form on every
 * feed; "ago" drifts as the page is read). The age is the date's own hover.
 */
function RowDate({ timestamp }: { timestamp: string }) {
  return <Utc8Date timestamp={timestamp} />;
}

/**
 * Line 1 of a row: the primary fact on the left (the Layer-3 narrative for an activity), the raw
 * evidence quiet on the right — block, tx hash, date — always there to verify it against.
 *
 * `layout="line"` (tables round 2026-11-02, T7 · option A) is the activity row's one 18 px line:
 * the title, then `details` inline (the tx-level facts, dim), the evidence on the right. With
 * `href` the title is the link to the transaction and the default evidence is block · date — the
 * hash is the row's hover (`title`), not text. The default `layout="wrap"` keeps the hash link
 * (TxRow). Consistency C3: both end with the absolute UTC+8 date, the age in its hover (was the age).
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
  layout = 'wrap',
  href,
  details,
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
  /** T7: `line` is the activity row's one 18 px headline line; `wrap` (default) is unchanged. */
  layout?: 'wrap' | 'line';
  /** T7 (line layout): the title links here and the default evidence leaves the hash out. */
  href?: string;
  /** T7 (line layout): the tx-level facts drawn inline after the title. */
  details?: ReactNode;
}) {
  const line = layout === 'line';
  const linked = line && href !== undefined;
  const titleClass = cn('text-text-bright text-body font-medium', line && 'leading-[18px]');
  return (
    <div
      data-role="row-head"
      className={cn(
        'flex flex-wrap items-baseline justify-between gap-x-4',
        line ? 'gap-y-0 leading-[18px]' : 'gap-y-0.5'
      )}
    >
      {line ? (
        <span className="min-w-0">
          {linked ? (
            <Link
              data-role="headline"
              href={href}
              className={cn(titleClass, 'hover:text-jade transition-colors')}
            >
              {title}
            </Link>
          ) : (
            <span data-role="headline" className={titleClass}>
              {title}
            </span>
          )}
          {details}
        </span>
      ) : (
        <span data-role="headline" className={titleClass}>
          {title}
        </span>
      )}
      {showEvidence && evidence !== undefined && (
        <span
          data-role="evidence"
          className={cn('text-text-dim text-meta shrink-0', line && 'ml-auto leading-[18px]')}
        >
          {evidence}
        </span>
      )}
      {showEvidence && evidence === undefined && (
        <span
          data-role="evidence"
          className={cn(
            'text-text-dim text-meta shrink-0',
            line && 'ml-auto whitespace-nowrap leading-[18px]'
          )}
        >
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
          {!linked && (
            <>
              <Link
                href={`/tx/${hash}`}
                className="hover:text-jade text-text-dim transition-colors"
              >
                {truncateHash(hash, 10, 6)}
              </Link>
              {' · '}
            </>
          )}
          {timestamp !== null ? (
            <RowDate timestamp={timestamp} />
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

/** A called script as the row names it (T7): its name, its page, and the call with its full args. */
interface CallRef {
  name: string;
  href: string;
  call: string;
}

function typeCallRef(sc: ActivityTypeCall): CallRef {
  const name = formatScriptRef({ codeHash: sc.typeCodeHash, scriptName: sc.scriptName });
  return {
    name,
    call: sc.typeArgs === '0x' ? name : `${name}(${sc.typeArgs})`,
    href: getScriptDetailHref({
      name: sc.scriptName,
      codeHash: sc.typeCodeHash,
      hashType: sc.typeHashType,
      scriptKind: 'type',
    }),
  };
}

function lockCallRef(lc: ActivityLockCall): CallRef {
  const name = formatScriptRef({ codeHash: lc.lockCodeHash, scriptName: lc.scriptName });
  const call = lc.lockArgs === '0x' ? name : `${name}(${lc.lockArgs})`;
  // Revised R-8: args that do not fit the lock's registry layout carry the API's reason.
  const notDecoded = lc.decodeError ? lockArgsLine(lc.decoded, lc.decodeError) : null;
  return {
    name,
    call: notDecoded ? `${call} ${notDecoded}` : call,
    href: getScriptDetailHref({
      name: lc.scriptName,
      codeHash: lc.lockCodeHash,
      hashType: lc.lockHashType,
      scriptKind: 'lock',
    }),
  };
}

/**
 * The calls by name, once per script (name and page), in first-call order, each with its count
 * and every call it stands for. Without args on the row, two calls of one script would read
 * ".bit Lock · .bit Lock"; one name, "×2" and the calls in its hover say the same.
 */
function callNames(refs: CallRef[]): ReactNode[] {
  const groups: { name: string; href: string; calls: string[] }[] = [];
  for (const r of refs) {
    const same = groups.find((g) => g.name === r.name && g.href === r.href);
    if (same) same.calls.push(r.call);
    else groups.push({ name: r.name, href: r.href, calls: [r.call] });
  }
  return groups.map((g) => (
    <span key={`${g.href}:${g.name}`} className="whitespace-nowrap">
      <Link
        href={g.href}
        title={g.calls.join('\n')}
        className="text-text-dim hover:text-jade transition-colors"
        onClick={(e) => e.stopPropagation()}
      >
        {g.name}
      </Link>
      {g.calls.length > 1 && <span className="text-text-dim"> ×{g.calls.length}</span>}
    </span>
  ));
}

/** One inline group of tx-level facts after the headline, led by a dim middle dot. */
function DetailRun({ children }: { children: ReactNode }) {
  return (
    <>
      <span className="text-text-dim text-small">{' \u00B7 '}</span>
      <span data-role="detail" className="text-text-dim text-small font-normal">
        {children}
      </span>
    </>
  );
}

/** One protocol action's line: its label (null for the headline's own action) and its facts. */
interface ProtocolLineView {
  key: string;
  /** `protocol:action`, what "the same action" compares. */
  action: string;
  label: string | null;
  parts: ReactNode[];
  /** Actions that read exactly this line ("×18"). */
  count: number;
}

/** What a run of one action counts: a DAO action is one per deposit cell; any other, an action. */
function actionNoun(action: string, count: number): string {
  const [protocol, name] = action.split(':');
  const noun =
    protocol === 'dao' ? (name === 'withdraw_complete' ? 'withdrawal' : 'deposit') : 'action';
  return count === 1 ? noun : `${noun}s`;
}

/**
 * The protocol-action lines of a head (lane F follow-up, the PARTICIPANT_LIMIT pattern of
 * `FoldedCalls`): up to PARTICIPANT_LIMIT lines are all said as before. Over it, the first five
 * and "+N more" that opens the rest in place; when every line is one action (a dCKB batch withdraw:
 * 51 DAO withdraw requests), the head names it once with its count ("51 deposits") and the lines
 * carry only their facts. Nothing is dropped — the expand shows every line.
 */
function FoldedProtocolLines({
  lines,
  primaryAction,
}: {
  lines: ProtocolLineView[];
  /** The headline's action (`protocol:action`): a run of it needs no name. */
  primaryAction: string | null;
}) {
  const [open, setOpen] = useState(false);
  const over = lines.length > PARTICIPANT_LIMIT;
  const one = over && lines.every((l) => l.action === lines[0].action);
  const shown = over && !open ? lines.slice(0, PARTICIPANT_LIMIT - 1) : lines;
  const actions = (ls: ProtocolLineView[]) => ls.reduce((n, l) => n + l.count, 0);
  const hidden = actions(lines) - actions(shown);
  const nounOf = (count: number) =>
    one ? actionNoun(lines[0].action, count) : count === 1 ? 'action' : 'actions';
  const label = lines.find((l) => l.label !== null)?.label ?? null;
  return (
    <>
      {one && (
        <DetailRun>
          {lines[0].action !== primaryAction && label !== null ? `${label} · ` : ''}
          {actions(lines)} {nounOf(actions(lines))}
        </DetailRun>
      )}
      {shown.map((l) => {
        const parts = one || l.label === null ? l.parts : [l.label, ...l.parts];
        if (parts.length === 0) return null;
        return (
          <DetailRun key={l.key}>
            <DotJoined parts={parts} />
            {l.count > 1 && <span className="text-text-dim"> ×{l.count}</span>}
          </DetailRun>
        );
      })}
      {over && !open && (
        <>
          {' '}
          <button
            type="button"
            aria-expanded={false}
            onClick={() => setOpen(true)}
            className="text-interactive hover:text-jade text-small whitespace-nowrap bg-transparent font-normal transition-colors"
          >
            +{hidden} more {nounOf(hidden)}
          </button>
        </>
      )}
    </>
  );
}

/**
 * The tx-level facts of an activity, from the one event classification, as inline runs after the
 * headline (T7): each protocol action's metadata (the headline's own action needs no name; another
 * one leads with its label), then the type calls and the lock calls by name ("lock …"), their args
 * in each name's hover. Action runs that read the same are said once with their count ("×18"); a
 * long call run, and a long run of protocol lines (`FoldedProtocolLines`), folds behind "+N more".
 * A protocol action that cannot be read is an error line under the headline (`errors`).
 */
function activityDetails(
  activity: ActivityBody & { txHash: string },
  showScriptDetails: boolean
): { runs: ReactNode; errors: ReactNode } {
  const primary = classifyActivity(activity).primaryProtocolAction;
  const events = globalTxEvents(activity);
  const types = events.flatMap((e) => (e.kind === 'type' ? [e.call] : []));
  const locks = events.flatMap((e) => (e.kind === 'lock' ? [e.call] : []));
  const lines: ProtocolLineView[] = [];
  const errors: ReactNode[] = [];
  events.forEach((e, i) => {
    if (e.kind !== 'protocol') return;
    let parts: ReactNode[];
    try {
      parts = protocolActionNoteParts(e.action);
    } catch (error) {
      errors.push(
        <div key={`p${i}`} data-role="detail-error" className="text-small pl-[18px]">
          <ActivityReadError txHash={activity.txHash} error={error} />
        </div>
      );
      return;
    }
    const label = e.action === primary ? null : protocolActionLabel(e.action);
    if (label === null && parts.length === 0) return;
    const key = protocolLineKey(e.action, e.action === primary);
    const same = lines.find((l) => l.key === key);
    if (same) same.count += 1;
    else
      lines.push({
        key,
        action: `${e.action.protocol}:${e.action.action}`,
        label,
        parts,
        count: 1,
      });
  });
  const runs: ReactNode[] = [];
  if (lines.length > 0)
    runs.push(
      <FoldedProtocolLines
        key="protocol"
        lines={lines}
        primaryAction={primary ? `${primary.protocol}:${primary.action}` : null}
      />
    );
  if (showScriptDetails && types.length > 0)
    runs.push(
      <DetailRun key="type">
        <FoldedCalls noun="types" parts={callNames(types.map(typeCallRef))} />
      </DetailRun>
    );
  if (showScriptDetails && locks.length > 0)
    runs.push(
      <DetailRun key="lock">
        {'lock '}
        <FoldedCalls noun="locks" parts={callNames(locks.map(lockCallRef))} />
      </DetailRun>
    );
  return { runs: <>{runs}</>, errors: <>{errors}</> };
}

/** The change in occupied capacity, whole CKB (it is bytes), exact otherwise. */
export function formatOccupiedDelta(usedDelta: string): string {
  const shannons = BigInt(usedDelta);
  return shannons % SHANNONS_PER_CKB === BigInt(0)
    ? formatSignedCkbInteger(shannons)
    : formatSignedCkbExact(shannons);
}

function partyKey(p: ParticipantInfo, i: number): string {
  return `${p.lockHash ?? p.lockHashPrefix ?? p.address ?? 'party'}:${i}`;
}

/**
 * The lock kind as an 11 px bordered tag (T5/T7; the classes of lane R's `RowTag`, built here
 * until the row primitives land — swapping it for `RowTag` is lane M's).
 */
const LOCK_TAG =
  'text-meta text-text-dim border border-base-border rounded-[3px] px-[5px] leading-4 whitespace-nowrap';
/** A control on a participant line, as tall as its tag (16 px line + border = the 18 px line). */
const LINE_BUTTON =
  'border-base-border text-text hover:text-text-bright text-meta whitespace-nowrap rounded-[3px] border bg-transparent px-[5px] leading-4';

/** One Layer-2 fact as one item of the participant line: it wraps whole, never inside. */
function ItemOnLine({
  item,
  tone,
  showComposition,
  className,
}: {
  item: ItemDelta;
  tone: 'plain' | 'sign';
  /** False when the line says its objects' one shared tier once (`lineCompositionTier`). */
  showComposition: boolean;
  className?: string;
}) {
  return (
    <span data-role="item" className={cn('min-w-0 max-w-full', className)}>
      <ItemDeltaFact item={item} tone={tone} symbol="dim" showComposition={showComposition} />
    </span>
  );
}

/**
 * Consistency C3: the one composition tier every item of a participant line shares, said once at
 * the end of its items. Only objects carry a tier, so a line that also moves a token or an
 * identity has none to share (a badge at its end would read as theirs); null too when an object
 * carries no tier or the tiers differ — then each object keeps its own badge.
 */
export function lineCompositionTier(items: ItemDelta[]): CompositionTier | null {
  const first = items[0];
  if (first === undefined || first.kind !== 'object' || !first.composition) return null;
  const tier = first.composition;
  return items.every((i) => i.kind === 'object' && i.composition === tier) ? tier : null;
}

/**
 * The item facts of one participant line. `foldObjects` (F2 objects, OB-M3): a batch of objects
 * shows its lead (this page's object, else the first) and "+N more objects", which opens the rest
 * in place; the participant's tokens and identities always show. `className` hides the run below
 * md where the phone form draws the objects as one count chip instead.
 */
function ItemsOnLine({
  items,
  foldObjects,
  focusItemId,
  itemTone,
  showComposition,
  className,
}: {
  items: ItemDelta[];
  foldObjects: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  showComposition: boolean;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const objects = items.filter((i) => i.kind === 'object');
  const folded = foldObjects && !open && objects.length >= 2;
  const lead = folded
    ? (objects.find((o) => o.kind === 'object' && o.objectId === focusItemId) ?? objects[0])
    : null;
  const shown = folded ? items.filter((i) => i.kind !== 'object' || i === lead) : items;
  return (
    <>
      {shown.map((item, i) => (
        <ItemOnLine
          key={i}
          item={item}
          tone={itemTone}
          showComposition={showComposition}
          className={className}
        />
      ))}
      {folded && (
        <button
          type="button"
          data-role="more-objects"
          aria-expanded={false}
          onClick={() => setOpen(true)}
          className={cn(LINE_BUTTON, className)}
        >
          +{objects.length - 1} more object{objects.length - 1 === 1 ? '' : 's'}
        </button>
      )}
    </>
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
 * One participant, one 18 px line (T7 · option A, Feed.dc.html column A): the address (and any
 * roles), the lock kind as a tag, a note the API attaches, the signed exact CKB, each item change
 * with its mark (signed, by sign; symbol dim), and the occupied change last in the board's 11 px
 * ghost tone (≈ #4a5268). The line wraps between those items, in that order, when it cannot hold
 * them — never inside one.
 *
 * Consistency C3: the participant a page is about (`focus`) writes its address as text, not a link
 * to the page it is on, and carries a "this address" tag after its lock tag (Address.dc.html →
 * History). When every item on it is an object of one composition tier, the line says it once
 * after its items (`lineCompositionTier`); otherwise each object keeps its own badge.
 *
 * The asset boards' phone form (`foldObjects`, B#12) stays: below md the objects are one count
 * chip ("+2 objects") that opens them in place; tokens and identities stay as facts.
 */
function ParticipantLine({
  p,
  focus,
  foldObjects,
  focusItemId,
  itemTone,
  notes,
}: {
  p: ActivityParticipant & { key: string };
  focus: boolean;
  foldObjects: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  notes?: ReadonlyMap<string, string>;
}) {
  const [objectsOpen, setObjectsOpen] = useState(false);
  const phoneObjects = foldObjects && p.itemDeltas.some((i) => i.kind === 'object');
  const otherItems = p.itemDeltas.filter((i) => i.kind !== 'object');
  const lineTier = lineCompositionTier(p.itemDeltas);
  const showComposition = lineTier === null;
  return (
    <div
      data-role="participant"
      // The address sits 18 px in; a line that wraps (phone widths) continues 30 px in, a hanging
      // indent, so the continuation never reads as the next participant.
      className="text-small flex flex-wrap items-center gap-x-2.5 gap-y-0 pl-[30px] leading-[18px]"
    >
      <span data-role="party" data-party={p.key} className="-ml-3 max-w-full">
        <ParticipantRefView participant={p} compact quiet linked={!focus} />
      </span>
      {p.lockKind && (
        <span data-role="lock-kind" className={LOCK_TAG}>
          {p.lockKind}
        </span>
      )}
      {focus && (
        // The board's ghost tag, as the row tag's ghost variant: dashed, in the readable dim ink.
        <span data-role="this-address" data-row-tag="ghost" className={rowTagClass('ghost')}>
          this address
        </span>
      )}
      {p.lockHash !== null && notes?.has(p.lockHash) && (
        <span data-role="participant-note" className="text-text-dim text-meta">
          {notes.get(p.lockHash)}
        </span>
      )}
      <span data-role="ckb">
        <ExactCkb shannons={BigInt(p.ckbDelta)} />
      </span>
      {phoneObjects && (
        <span
          data-role="phone-items"
          className="inline-flex flex-wrap items-center gap-x-2.5 md:hidden"
        >
          {otherItems.map((item, i) => (
            <ItemOnLine key={i} item={item} tone={itemTone} showComposition={showComposition} />
          ))}
          <button
            type="button"
            data-role="object-count"
            aria-expanded={objectsOpen}
            onClick={() => setObjectsOpen((open) => !open)}
            className={LINE_BUTTON}
          >
            {objectCountLabel(p.itemDeltas)}
          </button>
        </span>
      )}
      <ItemsOnLine
        items={p.itemDeltas}
        foldObjects={foldObjects}
        focusItemId={focusItemId}
        itemTone={itemTone}
        showComposition={showComposition}
        className={phoneObjects ? 'hidden md:inline-flex' : undefined}
      />
      {lineTier !== null && (
        <span data-role="composition" className="inline-flex">
          <CompositionBadge tier={lineTier} size="sm" />
        </span>
      )}
      {p.usedDelta !== '0' && (
        <span
          data-role="occupied"
          className="text-meta text-text-dim/60 whitespace-nowrap tabular-nums"
        >
          occupied {formatOccupiedDelta(p.usedDelta)}
        </span>
      )}
      {phoneObjects && objectsOpen && (
        <div
          data-role="phone-objects"
          className="flex basis-full flex-wrap items-center gap-x-2.5 pl-2 md:hidden"
        >
          {p.itemDeltas
            .filter((i) => i.kind === 'object')
            .map((item, i) => (
              <ItemOnLine key={i} item={item} tone={itemTone} showComposition={showComposition} />
            ))}
        </div>
      )}
    </div>
  );
}

/**
 * The focused participant (consistency C3) and the others. The focus is matched by its full lock
 * hash; a focus that is not a participant of the transaction is a caller's contract violation.
 */
function splitFocus<T extends ActivityParticipant>(
  participants: T[],
  focusLockHash: string | undefined,
  txHash: string
): { focus: T | null; others: T[] } {
  if (focusLockHash === undefined) return { focus: null, others: participants };
  const at = participants.findIndex((p) => p.lockHash === focusLockHash);
  if (at < 0)
    throw new Error(
      `transaction ${txHash}: focus lock hash ${focusLockHash} is not one of its ${participants.length} participants`
    );
  return { focus: participants[at], others: participants.filter((_, i) => i !== at) };
}

/**
 * Every participant, Layer 1 and Layer 2, one line each (D1: none is dropped). Named parties with
 * no change are listed — their zero is the fact. Over six, the five largest and an explicit "+N"
 * to expand (the homepage rule, unchanged). Consistency C3: with `focusLockHash` that participant
 * comes first, always shown; the others keep their order and that rule among themselves.
 */
function Participants({
  participants,
  txHash,
  focusLockHash,
  foldObjects = false,
  focusItemId,
  itemTone,
  notes,
}: {
  participants: ActivityParticipant[];
  txHash: string;
  focusLockHash?: string;
  foldObjects?: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  /** A fact the API attaches to one participant, by its lock hash (R-4 "holds the class cell"). */
  notes?: ReadonlyMap<string, string>;
}) {
  const [expanded, setExpanded] = useState(false);
  const indexed = participants.map((p, i) => ({ ...p, key: partyKey(p, i) }));
  const { focus, others } = splitFocus(indexed, focusLockHash, txHash);
  const { shown: shownOthers, hidden } = participantsToShow(others, expanded);
  const shown = focus === null ? shownOthers : [focus, ...shownOthers];
  const foldable = others.length > PARTICIPANT_LIMIT;
  if (participants.length === 0) return null;
  return (
    <div data-role="participants" className="flex min-w-0 flex-col">
      {shown.map((p) => (
        <ParticipantLine
          key={p.key}
          p={p}
          focus={p === focus}
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
          className="text-interactive hover:text-jade text-small self-start bg-transparent pl-[18px] leading-[18px] transition-colors"
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
  /**
   * F2 objects (IV-14, OB-12): was the lock kind's own grid column. Option A (T7) draws the lock
   * kind as a tag on every participant line, so this no longer changes the row; kept so the
   * callers that pass it need no change in this lane.
   */
  lockColumn?: boolean;
  /** F2 objects: fold a batch of objects behind "+N more objects", leading with `focusItemId` (OB-M3). */
  foldObjects?: boolean;
  focusItemId?: string;
  /**
   * Token amounts: by sign (jade +, rouge −) by default — an asset delta is a signed delta, like
   * the CKB beside it (T7 ruling, 2026-11-02; was the Address board only, BA-35). `plain` draws
   * them bright.
   */
  itemTone?: 'plain' | 'sign';
  /** Round 3 (R-4): a note the API attaches to one participant, keyed by its lock hash. */
  participantNotes?: ReadonlyMap<string, string>;
  /**
   * Consistency C3 (the address page): the participant with this full lock hash leads, its
   * address as text with a "this address" tag; it must be a participant of the transaction.
   */
  focusLockHash?: string;
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
  foldObjects = false,
  focusItemId,
  itemTone = 'sign',
  participantNotes,
  focusLockHash,
}: ActivityRowProps) {
  const shown =
    mode === 'participant' && participant ? { ...activity, participants: [participant] } : activity;
  const mark = markOverride === undefined ? activityClassMark(activity) : markOverride;
  const { runs, errors } = activityDetails(activity, showScriptDetails);
  return (
    <article
      data-feed-row={activity.txHash}
      // T7: the hash leaves the visible row; it is the row's hover (and the headline's link).
      // Consistency C3: so is the age, beside it; the row shows the date.
      title={activityRowTitle(activity.txHash, activity.timestamp)}
      className={cn(ROW_CLASS, fresh && 'bg-jade/5', className)}
    >
      <ActivityRowHead
        layout="line"
        title={
          <>
            {mark && <ClassMark kind={mark} className="mr-2" />}
            {headline ?? activityHeadline(activity)}
          </>
        }
        // A caller's own evidence (block, address pages) carries its own tx link; then the
        // headline stays text, which also keeps a headline with links of its own unnested.
        href={evidence === undefined ? `/tx/${activity.txHash}` : undefined}
        details={runs}
        hash={activity.txHash}
        blockNumber={activity.blockNumber}
        timestamp={activity.timestamp}
        showEvidence={showEvidence}
        evidence={evidence}
        poolStatus={activity.poolStatus}
        timeAddedToPool={activity.timeAddedToPool}
      />
      {errors}
      <PoolInterpretationNotice interpretation={activity.interpretation} />
      {showParticipants && (
        <Participants
          participants={shown.participants}
          txHash={activity.txHash}
          focusLockHash={focusLockHash}
          foldObjects={foldObjects}
          focusItemId={focusItemId}
          itemTone={itemTone}
          notes={participantNotes}
        />
      )}
    </article>
  );
}
