'use client';
import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithRef,
  type ReactNode,
} from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import {
  ActivityReadError,
  ItemDeltaFact,
  activityClassMark,
  formatScriptRef,
  activityHeadline,
  globalTxEvents,
  protocolActionLabel,
  protocolActionNoteFacts,
  type ProtocolNoteFact,
} from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { ParticipantRefView } from '@/components/participant-ref';
import { LockWordsView, lockNoun, namedLockWords, partyLock } from '@/components/ui/lock-kind';
import { normalizeScriptRefHashType } from '@/lib/script-ref';
import { Dotted, dottedGroup, type DottedGroup } from '@/components/ui/phone-row';
import { formatCount, countNoun } from '@/lib/format-count';
import { classifyActivity } from '@/lib/activity-classify';
import { formatOccupiedDelta } from '@/lib/occupied';
import { PARTICIPANT_LIMIT, participantsToShow } from '@/lib/participant-order';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { lockArgsLine } from '@/lib/lock-args';
import type {
  ActivityItemType,
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
import type { ActivityBody } from '@/lib/activity-classify';
import type { CompositionTier } from '@/lib/asset-utils';
import type { AssetClass } from '@/lib/design-tokens';
import { cn, formatCkbAmount, formatTimeAgo } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { useClockText, Utc8Age, Utc8Date } from '@/components/ui/utc8-date';
import { FoldToggle } from '@/components/ui/fold-toggle';
import { ShortId } from '@/components/ui/short-id';
import { PHONE_PART_GAP_PX, ROW_LINE_PX, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { WIDEST_AGE, WIDEST_BLOCK_NUMBER } from '@/lib/list-bounds';
import { useFromSm } from '@/hooks/useFromMd';
export type ActivityParticipant = ParticipantInfo & { lockKind?: string | null };
// T7: 8 px padding, 2 px between the headline line and the participant lines.
const ROW_CLASS = 'flex flex-col gap-0.5 border-b border-row-rule py-2 transition-colors';

/**
 * A row's hover (consistency C3, T7): the tx hash, and the block's age beside it. On a list that is
 * historical by nature the row shows the date and the age is here alone; a live list's row says
 * its age (PA-22) and the hover repeats it. A pool row has no block time, so its hover is the hash.
 */
export function activityRowTitle(hash: string, timestamp: string | null, now: number): string {
  return timestamp === null ? hash : `${hash} · ${formatTimeAgo(timestamp, now)}`;
}

/**
 * A row's `<article>` with that hover, its age read from the page's one clock (lane E4), so it
 * moves with no data change. Only this element renders again as the age moves: the row inside it
 * is the same element and is not rendered again.
 */
export function RowArticle({
  hash,
  timestamp,
  ...article
}: ComponentPropsWithRef<'article'> & { hash: string; timestamp: string | null }) {
  const title = useClockText((now) => activityRowTitle(hash, timestamp, now));
  return <article {...article} title={title} />;
}

/**
 * When the row's block was made: its age on a row of a live feed (PA-22: the activities, the home
 * feed, the DAO's recent activity), the full date-time its hover; the date on the UTC+8 clock on a
 * list that is historical by nature (consistency C3), the age its hover.
 */
function RowDate({ timestamp, age }: { timestamp: string; age: boolean }) {
  return age ? <Utc8Age timestamp={timestamp} /> : <Utc8Date timestamp={timestamp} />;
}

/**
 * Line 1 of an activity row on a phone (the phone-rows board's head(): 262 of 328 px there for "CKB
 * Transfer" beside "#21,009,129 · 50s ago"), at its widest: the class mark and its 8 px (24), the
 * headline's first 12 characters at the key's 14 px (100.8), the 12 px between the parts and the
 * evidence at the meta step, the widest block with the oldest age a live list writes, "#99,999,999
 * · 10y 364d ago" (26 characters, 187.2): 324 of a 360 px phone's 328 px line (16 characters of
 * headline at 390). A longer headline wraps between its words beside the evidence, which keeps its
 * place on the line; so the headline's longest word and the evidence must fit together, "Withdraw"
 * (67.2) the longest of the site's own headlines' words.
 */
const ACTIVITY_PHONE_HEAD_PX = phoneHead(
  'activities',
  16 +
    8 +
    textPx(12, fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(`${WIDEST_BLOCK_NUMBER} · ${WIDEST_AGE}`.length, fontPx('meta'))
);

/**
 * Line 1 of a row: the primary fact on the left (the Layer-3 narrative for an activity), the raw
 * evidence quiet on the right — block, tx hash, date — always there to verify it against.
 *
 * `layout="line"` (tables round 2026-11-02, T7 · option A) is the activity row's one 18 px line:
 * the title, then `details` inline (the tx-level facts, dim), the evidence on the right. The title
 * and the details are one separated list (PA-25): a "·" never ends a line or opens one. With
 * `href` the title is the link to the transaction and the default evidence is block · date — the
 * hash is the row's hover (`title`), not text. The default `layout="wrap"` keeps the hash link
 * (TxRow). Both end with the block's time: on a live list its age, the full date-time its hover
 * (PA-22); on a list historical by nature the UTC+8 date, the age its hover (consistency C3).
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
  detailsId,
  age = false,
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
  /** T7 (line layout): the tx-level facts after the title, as parts of the head's one list. */
  details?: readonly (ReactNode | DottedGroup)[];
  /** The id of the element that holds `details`, which the head's names' fold controls. */
  detailsId?: string;
  /** PA-22: a row of a live list says its age, the date-time its hover; else the date. */
  age?: boolean;
}) {
  const line = layout === 'line';
  const linked = line && href !== undefined;
  const titleClass = cn('text-text-bright text-body font-medium', line && 'leading-row');
  // Below sm (the phone-rows board, PA-10): the headline and the evidence share line 1 and the line
  // does not wrap its parts — the headline takes the room the evidence leaves it (basis 0) and
  // wraps between its words; the evidence shrinks only where it alone is wider than the line (this
  // device's clock behind), its list then breaking between its parts. What was called stands on a
  // line of its own under it (`head-details`).
  const fromSm = useFromSm();
  const phone = line && !fromSm;
  const headline = linked ? (
    <Link
      data-role="headline"
      href={href}
      // the row's key: the key look's bright ink (a link's ink is its look's)
      look="key"
      className={cn('text-body font-medium', line && 'leading-row')}
    >
      {title}
    </Link>
  ) : (
    <span data-role="headline" className={titleClass}>
      {title}
    </span>
  );
  const head = (
    <div
      data-role="row-head"
      data-head-px={phone ? ACTIVITY_PHONE_HEAD_PX : undefined}
      className={cn(
        'flex items-baseline justify-between',
        phone ? 'flex-nowrap gap-x-3' : 'flex-wrap gap-x-4',
        line ? 'leading-row gap-y-0' : 'gap-y-0.5'
      )}
    >
      {phone ? (
        <span data-role="head-main" className="min-w-0 flex-1 basis-0">
          {headline}
        </span>
      ) : line ? (
        <span id={detailsId} data-role="head-main" className="min-w-0">
          <Dotted parts={[headline, ...(details ?? [])]} />
        </span>
      ) : (
        <span data-role="headline" className={titleClass}>
          {title}
        </span>
      )}
      {showEvidence && evidence !== undefined && (
        <span
          data-role="evidence"
          className={cn(
            'text-text-dim text-meta',
            line && 'leading-row ml-auto',
            phone && 'min-w-0'
          )}
        >
          {evidence}
        </span>
      )}
      {showEvidence && evidence === undefined && (
        <span
          data-role="evidence"
          // An item of the head's wrapping row that may shrink (lane B4 fix round: it was
          // shrink-0, so it stood at its whole width, one line past a phone's): beside the headline
          // while both fit, on its own line when not, where it takes the line's room. Its parts are
          // a separated list (second fix round: the separators have one owner, and a line never
          // starts or ends with one): it breaks between the parts, each separator hung before the
          // part after it, and inside a part only where the part lets it (the block number has no
          // space to break at, the status is one badge, `Utc8Age` keeps an age whole and lets "this
          // device's clock is behind" break between its words).
          className={cn(
            'text-text-dim text-meta',
            line && 'leading-row ml-auto',
            phone && 'min-w-0'
          )}
        >
          <Dotted
            parts={[
              blockNumber !== null ? (
                // its ink said on itself, the evidence's quiet ink (a part handed to the list)
                <Link href={`/blocks/${blockNumber}`} look="inline">
                  {formatBlockNumber(blockNumber)}
                </Link>
              ) : poolStatus ? (
                <PoolStatusBadge status={poolStatus} />
              ) : (
                <span>Unconfirmed</span>
              ),
              !linked && (
                <Link href={`/tx/${hash}`} look="inline">
                  <ShortId value={hash} kind="hash" size="row" />
                </Link>
              ),
              timestamp !== null ? (
                <RowDate timestamp={timestamp} age={age} />
              ) : timeAddedToPool ? (
                <TimeInPool since={timeAddedToPool} />
              ) : (
                'Time unavailable'
              ),
            ]}
          />
        </span>
      )}
    </div>
  );
  if (!phone || !details || details.length === 0) return head;
  return (
    <>
      {head}
      <div id={detailsId} data-role="head-details" className="text-small leading-row min-w-0">
        <Dotted parts={details} />
      </div>
    </>
  );
}

/**
 * What a protocol-action line draws, as a key: the label and every metadata field the line reads
 * (`protocolActionNoteFacts` reads `protocolActionMeta`, the protocol and the action). Two actions
 * with one key read the same, whatever else their metadata carries.
 */
function protocolLineKey(pa: ActivityProtocolAction, isPrimary: boolean): string {
  return JSON.stringify(
    [isPrimary, protocolActionLabel(pa), pa.protocol, pa.action, protocolActionMeta(pa)],
    (_, v: unknown) => (typeof v === 'bigint' ? v.toString() : v)
  );
}

/** A run of a head's script names: type scripts (a run's item types, the type calls) or locks. */
interface ScriptRun {
  kind: 'type' | 'lock';
  parts: ReactNode[];
}

/** The fold of a head's script names: open or not, the way to turn it, the names' element. */
export interface HeadFold {
  open: boolean;
  onToggle: (open: boolean) => void;
  controls: string;
}

/**
 * The scripts a head names, by ONE rule for a row's head and a folded run's (the lead's final
 * round, item 2: "the same list of the same things"): one list, in its runs' order (a run's item
 * types first, then the type calls, then the lock calls), of at most PARTICIPANT_LIMIT names, six;
 * over six, the first five and "+N more locks" (types, scripts: what the fold holds), the inline
 * fold that opens the rest in place and folds it back (B#26: 29 lock calls, one per line at 390).
 * The fold is written after the last name shown, so it never stands alone after a separator.
 */
function headScripts(runs: readonly ScriptRun[], fold: HeadFold): DottedGroup[] {
  const total = runs.reduce((n, r) => n + r.parts.length, 0);
  const foldable = total > PARTICIPANT_LIMIT;
  const keep = foldable && !fold.open ? PARTICIPANT_LIMIT - 1 : total;
  const held = new Set<ScriptRun['kind']>();
  let before = 0;
  // one group a run in both states (an empty one draws nothing), so the fold's group keeps its
  // place in the head's list and stays the same element as the names open and fold
  const groups = runs.map((r) => {
    const take = Math.max(0, Math.min(r.parts.length, keep - before));
    before += r.parts.length;
    if (take < r.parts.length) held.add(r.kind);
    return detailRun(r.parts.slice(0, take));
  });
  if (!foldable) return groups;
  const noun = held.size === 1 ? [...held][0] : 'script';
  return [
    ...groups,
    // after the last name shown, joined to it: never alone after a separator
    dottedGroup(
      [
        <FoldToggle
          key="fold"
          form="inline"
          open={fold.open}
          count={total - (PARTICIPANT_LIMIT - 1)}
          singular={noun}
          controls={fold.controls}
          onToggle={fold.onToggle}
        />,
      ],
      { role: 'names-fold', className: DETAIL, joined: true }
    ),
  ];
}

/** A called script as the row names it (T7): its name, its page, and the call with its full args. */
interface CallRef {
  name: string;
  href: string;
  call: string;
}

function typeCallRef(sc: ActivityTypeCall): CallRef {
  const name = formatScriptRef({ codeHash: sc.typeCodeHash, scriptName: sc.scriptName }, 'row');
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
  // a lock's words are the one decision's (components/ui/lock-kind.tsx), as a list names locks
  const name = lockNoun(
    namedLockWords(
      {
        name: lc.scriptName ?? null,
        codeHash: lc.lockCodeHash,
        hashType: normalizeScriptRefHashType(lc.lockHashType),
      },
      'column'
    ),
    'row'
  );
  const call = lc.lockArgs === '0x' ? name : `${name}(${lc.lockArgs})`;
  // Revised R-8: args that do not fit the lock's registry layout carry the API's reason.
  const notDecoded = lc.decodeError ? lockArgsLine(lc.decoded, lc.decodeError, 'row') : null;
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
 * The code a run's items and its DAO step carry (`itemTypes`), as its folded row names it: each code
 * once, by the registry's name (else its short code hash), a link to its page.
 */
function itemTypeRef(t: ActivityItemType): CallRef {
  const name = formatScriptRef(
    { codeHash: t.typeCodeHash, scriptName: t.scriptName ?? undefined },
    'row'
  );
  return {
    name,
    call: name,
    href: getScriptDetailHref({
      name: t.scriptName ?? undefined,
      codeHash: t.typeCodeHash,
      hashType: t.typeHashType,
      scriptKind: 'type',
    }),
  };
}

/**
 * The calls by name, once per script (name and page), in first-call order. On a row (`args`) each
 * has its count and every call it stands for in its hover: without args on the row, two calls of
 * one script would read ".bit Lock · .bit Lock"; one name, "×2" and the calls in its hover say the
 * same. A folded run's head (`args` false) names the code its rows called, each once: its rows
 * differ in how many cells carried a code (the fold compares the set, the lead's final round, item
 * 1) and in their args, so no row's count or args are the run's.
 */
function callNames(refs: CallRef[], args: boolean): ReactNode[] {
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
        title={args ? g.calls.join('\n') : undefined}
        look="inline"
        onClick={(e) => e.stopPropagation()}
      >
        {g.name}
      </Link>
      {args && g.calls.length > 1 && <span className="text-text-dim"> ×{g.calls.length}</span>}
    </span>
  ));
}

/** The voice of the tx-level facts after the headline: one run of them, named `detail`. */
const DETAIL = 'text-text-dim text-small font-normal';

function detailRun(parts: readonly ReactNode[]): DottedGroup {
  return dottedGroup(parts, { role: 'detail', className: DETAIL });
}

/** A part with "×k" after it, when a line stands for k alike. */
function times(part: ReactNode, count: number): ReactNode {
  return count > 1 ? (
    <>
      {part}
      <span className="text-text-dim"> ×{count}</span>
    </>
  ) : (
    part
  );
}

/** The last part of a run with "×k" after it. */
function withCount(parts: readonly ReactNode[], count: number): ReactNode[] {
  return parts.map((part, i) => (i === parts.length - 1 ? times(part, count) : part));
}

/** One protocol action as a row reads it: its name, whether the headline names it, its facts. */
interface ProtocolActionView {
  /** What reads alike inline (`protocolLineKey`): identical lines are said once with "×k". */
  lineKey: string;
  /** `protocol:action`, what "the same action" compares. */
  action: string;
  label: string;
  isPrimary: boolean;
  facts: ProtocolNoteFact[];
}

/** What a run of one action counts: a DAO action is one per deposit cell; any other, an action. */
function actionNoun(action: string): string {
  const [protocol, name] = action.split(':');
  return protocol === 'dao' ? (name === 'withdraw_complete' ? 'withdrawal' : 'deposit') : 'action';
}

/** "51 deposits" for a run of one action, "7 actions" for a mixed one. */
function runCount(actions: ProtocolActionView[]): string {
  const one = actions.every((a) => a.action === actions[0].action);
  return formatCount(actions.length, one ? actionNoun(actions[0].action) : 'action');
}

/**
 * Whether the rendered head holds the inline run on its one line (T7: the row line, `leading-row`):
 * the headline and its facts in one line box, and the evidence beside them, not fallen to a line
 * of its own. Read from layout,
 * not a character count — a head mixes 13, 12 and 11 px text (a ~145-character head of two DAO
 * withdraw requests fits a 1440 line; a count of 130 had listed it).
 */
function headIsOneLine(article: HTMLElement, txHash: string): boolean {
  const main = article.querySelector('[data-role="head-main"]');
  if (!main) throw new Error(`transaction ${txHash}: the activity head has no main line`);
  const m = main.getBoundingClientRect();
  if (m.height > ROW_LINE_PX * 1.5) return false;
  const evidence = article.querySelector('[data-role="evidence"]');
  return !evidence || evidence.getBoundingClientRect().top - m.top < ROW_LINE_PX / 2;
}

/**
 * The head's protocol facts inline (T7, unchanged for one action and for a short run): each
 * action's facts after the headline, the headline's own action unnamed, every other one led by its
 * label; lines that read the same are said once with their count ("×18").
 */
/** Actions that read alike, in first-seen order: one line with its count ("×18"), inline or listed. */
interface ProtocolLineGroup {
  key: string;
  first: ProtocolActionView;
  count: number;
}

function groupProtocolLines(actions: ProtocolActionView[]): ProtocolLineGroup[] {
  const groups: ProtocolLineGroup[] = [];
  for (const a of actions) {
    const same = groups.find((g) => g.key === a.lineKey);
    if (same) same.count += 1;
    else groups.push({ key: a.lineKey, first: a, count: 1 });
  }
  return groups;
}

/** A group's inline parts: its facts, led by its label unless it is the headline's own action. */
function inlineParts(g: ProtocolLineGroup): ReactNode[] {
  const facts = g.first.facts.map((f) => f.node);
  return g.first.isPrimary ? facts : [g.first.label, ...facts];
}

/** Each line of the head's protocol facts as one run of its list. */
function inlineProtocolRuns(groups: ProtocolLineGroup[]): DottedGroup[] {
  return groups.map((g) => detailRun(withCount(inlineParts(g), g.count)));
}

/**
 * The head of a listed run (lane F follow-up 2, T7 "one thing per line"): the action named once
 * — the headline names its own — with the count, and for a run of one action whose every member
 * carries an amount, the exact total (its title says what it sums). Nothing of the list inline.
 */
function protocolRunSummary(
  actions: ProtocolActionView[],
  primaryAction: string | null
): DottedGroup[] {
  const one = actions.every((a) => a.action === actions[0].action);
  const amounts = actions.map((a) => a.facts.find((f) => f.kind === 'capacity'));
  const total =
    one && amounts.every((f) => f !== undefined)
      ? amounts.reduce((sum, f) => sum + (f as { shannons: bigint }).shannons, BigInt(0))
      : null;
  const count = runCount(actions);
  return [
    detailRun([one && actions[0].action !== primaryAction && actions[0].label, count]),
    detailRun([
      total !== null && (
        <span
          data-role="protocol-total"
          title={`${formatCkbAmount(total).full} CKB, the sum of ${count}`}
        >
          <CkbAmount form="column" shannons={total} signed={false} tone="plain" />
        </span>
      ),
    ]),
  ];
}

/** Characters an exact amount takes ("5,100.00000000 CKB"): the width of the amount column. */
function amountChars(shannons: bigint): number {
  return `${formatCkbAmount(shannons).full} CKB`.length;
}

/**
 * One action per 18 px line, as participant lines are drawn: its name when the run is mixed (a
 * column as wide as the longest name), its amount in one right-aligned tabular column (the
 * decimals line up), "deposited at #block" as a link to the block, then every other fact it carries.
 */
function ProtocolActionLine({
  group,
  named,
  labelChars,
  amountWidth,
}: {
  group: ProtocolLineGroup;
  named: boolean;
  labelChars: number;
  amountWidth: number;
}) {
  const action = group.first;
  const parts: ReactNode[] = [];
  if (named)
    parts.push(
      <span className="text-text inline-block" style={{ minWidth: `${labelChars}ch` }}>
        {action.label}
      </span>
    );
  for (const f of action.facts) {
    if (f.kind === 'capacity')
      parts.push(
        <>
          <span
            data-role="protocol-amount"
            className="inline-block text-right tabular-nums"
            style={{ minWidth: `${amountWidth}ch` }}
          >
            <CkbAmount form="column" shannons={f.shannons} signed={false} tone="plain" />
          </span>
          {f.suffix}
        </>
      );
    // The deposit block's fact carries its own link (one rendering, in protocolActionNoteFacts).
    else parts.push(f.node);
  }
  if (parts.length === 0) parts.push(action.label);
  return (
    <div data-role="protocol-line" className="text-text-dim text-small leading-row pl-[18px]">
      <Dotted parts={withCount(parts, group.count)} />
    </div>
  );
}

/**
 * A listed run's actions under the head (lane F follow-up 2): folded by default behind
 * "51 deposits ↓", opened to one action per line, folded back by "fewer ↑". Every action of the
 * transaction is here — actions that read alike are one line with their count ("×18"), as inline.
 */
function ProtocolActionList({
  actions,
  groups,
}: {
  actions: ProtocolActionView[];
  groups: ProtocolLineGroup[];
}) {
  const [open, setOpen] = useState(false);
  const one = actions.every((a) => a.action === actions[0].action);
  const widths = actions.flatMap((a) =>
    a.facts.flatMap((f) => (f.kind === 'capacity' ? [amountChars(f.shannons)] : []))
  );
  const amountWidth = widths.length ? Math.max(...widths) : 0;
  const labelChars = Math.max(...actions.map((a) => a.label.length));
  const linesId = useId();
  return (
    <div data-role="protocol-list" className="flex min-w-0 flex-col">
      {/* the one fold control (FoldToggle): "all 51 deposits ↓", "fewer ↑" */}
      <div data-role="protocol-list-toggle" className="self-start pl-[18px]">
        <FoldToggle
          open={open}
          count={actions.length}
          singular={one ? actionNoun(actions[0].action) : 'action'}
          controls={linesId}
          onToggle={setOpen}
        />
      </div>
      <div id={linesId} data-role="protocol-list-lines" className="flex min-w-0 flex-col">
        {open &&
          groups.map((g) => (
            <ProtocolActionLine
              key={g.key}
              group={g}
              named={!one}
              labelChars={labelChars}
              amountWidth={amountWidth}
            />
          ))}
      </div>
    </div>
  );
}

/** A protocol action read for its row's note, or the error it cannot be read with. */
type ProtocolRead =
  | { facts: ProtocolNoteFact[]; error?: undefined }
  | { facts?: undefined; error: unknown };

/** The one read of a protocol action for a row (its note's facts). */
function readProtocolAction(action: Parameters<typeof protocolActionNoteFacts>[0]): ProtocolRead {
  try {
    return { facts: protocolActionNoteFacts(action, 'row', 'summary') };
  } catch (error) {
    return { error };
  }
}

/** Whether the one lock decision refuses a party's lock (its line then fails in its place). */
function partyLockRefused(party: ParticipantInfo): boolean {
  try {
    partyLock(party);
    return false;
  } catch {
    return true;
  }
}

/**
 * Whether a row cannot read one of its protocol actions or one of its parties' locks: its row
 * reports the error in its place (`activityDetails`; its party line, ruling D7), so a feed never
 * folds it (lane B4 fix round, review NOTE 1). A folded run names no party (Gate B), so a refused
 * lock inside a run would show only once the run is opened.
 */
export function rowReadFails(activity: ActivityBody): boolean {
  return (
    globalTxEvents(activity).some(
      (e) => e.kind === 'protocol' && readProtocolAction(e.action).error !== undefined
    ) || activity.participants.some(partyLockRefused)
  );
}

/**
 * The tx-level facts of an activity, from the one event classification (T7): the protocol actions
 * (`actions`, drawn inline or as a list by `ActivityRow`), then the type calls and the lock calls by
 * name ("lock …"), their args in each name's hover, a long call run folded behind "+N more"
 * (`calls`). A protocol action that cannot be read is an error line under the headline (`errors`).
 */
function activityDetails(
  activity: ActivityBody & { txHash: string },
  showScriptDetails: boolean,
  fold: HeadFold,
  args = true,
  /** A folded run's head: the code its rows' items carry, named first (`itemTypes`). */
  itemTypes: readonly ActivityItemType[] = []
): {
  actions: ProtocolActionView[];
  primaryAction: string | null;
  calls: DottedGroup[];
  errors: ReactNode;
} {
  const primary = classifyActivity(activity).primaryProtocolAction;
  const events = globalTxEvents(activity);
  const types = events.flatMap((e) => (e.kind === 'type' ? [e.call] : []));
  const locks = events.flatMap((e) => (e.kind === 'lock' ? [e.call] : []));
  const actions: ProtocolActionView[] = [];
  const errors: ReactNode[] = [];
  events.forEach((e, i) => {
    if (e.kind !== 'protocol') return;
    const read = readProtocolAction(e.action);
    if (read.facts === undefined) {
      errors.push(
        <div key={`p${i}`} data-role="detail-error" className="text-small pl-[18px]">
          <ActivityReadError txHash={activity.txHash} error={read.error} />
        </div>
      );
      return;
    }
    const { facts } = read;
    actions.push({
      lineKey: protocolLineKey(e.action, e.action === primary),
      action: `${e.action.protocol}:${e.action.action}`,
      label: protocolActionLabel(e.action),
      isPrimary: e.action === primary,
      facts,
    });
  });
  const runs: ScriptRun[] = [
    { kind: 'type', parts: callNames(itemTypes.map(itemTypeRef), args) },
    ...(showScriptDetails
      ? [
          { kind: 'type' as const, parts: callNames(types.map(typeCallRef), args) },
          // each name says it is a lock (`lockNoun`): "JoyID lock", "lock 0x9b9b…9b9b"
          { kind: 'lock' as const, parts: callNames(locks.map(lockCallRef), args) },
        ]
      : []),
  ];
  const calls = headScripts(
    runs.filter((r) => r.parts.length > 0),
    fold
  );
  return {
    actions,
    primaryAction: primary ? `${primary.protocol}:${primary.action}` : null,
    calls,
    errors: <>{errors}</>,
  };
}

/**
 * The party whose line's only movement is the fee (readability lane B4): its index, or null. Given
 * a row that lists every party of the transaction with its change, CKB is conserved but for what
 * enters (a cellbase's reward, a Nervos DAO withdrawal's compensation) and what leaves (the fee).
 * When nothing entered, no item moved, and exactly one party's CKB changed and went down, that
 * change is the fee exactly: its line says so, in the reading ink. Nothing is summed to find it.
 */
function feeOnlyParty(activity: ActivityBody): number | null {
  if (activity.isCellbase) return null;
  if (
    activity.protocolActions.some(
      (pa) => pa.protocol === 'dao' && pa.action === 'withdraw_complete'
    )
  )
    return null;
  let payer: number | null = null;
  for (const [i, p] of activity.participants.entries()) {
    if (p.itemDeltas.length > 0) return null;
    const delta = BigInt(p.ckbDelta);
    if (delta === BigInt(0)) continue;
    if (delta > BigInt(0) || payer !== null) return null;
    payer = i;
  }
  return payer;
}

function partyKey(p: ParticipantInfo, i: number): string {
  return `${p.lockHash ?? p.lockHashPrefix ?? p.address ?? 'party'}:${i}`;
}

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
      <ItemDeltaFact item={item} size="row" tone={tone} showComposition={showComposition} />
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
  controls,
}: {
  items: ItemDelta[];
  foldObjects: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  showComposition: boolean;
  className?: string;
  /** The id of the participant's line, which the objects' fold opens and folds. */
  controls: string;
}) {
  const [open, setOpen] = useState(false);
  const objects = items.filter((i) => i.kind === 'object');
  const foldable = foldObjects && objects.length >= 2;
  const folded = foldable && !open;
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
      {/* the rest of the batch inside the line: FoldToggle's inline form, which stays and folds
          back (the lead's final round, item 3) */}
      {foldable && (
        <span data-role="more-objects" className={cn('inline-flex', className)}>
          <FoldToggle
            form="inline"
            open={open}
            count={objects.length - 1}
            singular="object"
            controls={controls}
            onToggle={setOpen}
          />
        </span>
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
  return `${counts.join(' ')} ${countNoun(objects.length, 'object')}`;
}

/**
 * One participant, one 18 px line (T7 · option A, Feed.dc.html column A): the address (and any
 * roles), its lock's name in plain words when it is not the default lock (`partyLock`, R9), a note
 * the API attaches, the signed CKB, each item change with its mark (signed, by sign; symbol dim),
 * and the occupied change last at the meta step in the dim ink (a figure: never the ghost, never
 * the dim thinned; R4). The line wraps between those items, in that order, when it cannot hold
 * them — never inside one.
 *
 * Consistency C3: the participant a page is about (`focus`) writes its address as text, not a link
 * to the page it is on, underlined jade (`.this-address`, material pass v2 move 5 — the words
 * "this address" stay for a screen reader, the dotted tag is gone). When every item on it is an
 * object of one composition tier, the line says it once
 * after its items (`lineCompositionTier`); otherwise each object keeps its own badge.
 *
 * The asset boards' phone form (`foldObjects`, B#12) stays: below md the objects are one count
 * chip ("+2 objects") that opens them in place; tokens and identities stay as facts.
 */
function ParticipantLine({
  p,
  txHash,
  focus,
  fee,
  foldObjects,
  focusItemId,
  focusTokenTypeHash,
  itemTone,
  notes,
}: {
  p: ActivityParticipant & { key: string };
  /** The line's transaction, named where its figures are refused. */
  txHash: string;
  focus: boolean;
  /** Its only movement is the fee (`feeOnlyParty`): "fee" and the fee, in the reading ink. */
  fee: boolean;
  foldObjects: boolean;
  focusItemId?: string;
  /** The token the page is about: its amount leads the line, before the CKB (B4). */
  focusTokenTypeHash?: string;
  itemTone: 'plain' | 'sign';
  notes?: ReadonlyMap<string, string>;
}) {
  const [objectsOpen, setObjectsOpen] = useState(false);
  const lineId = useId();
  const leads = (i: ItemDelta) =>
    focusTokenTypeHash !== undefined &&
    i.kind === 'token' &&
    i.typeScriptHash === focusTokenTypeHash;
  const lead = p.itemDeltas.filter(leads);
  const rest = p.itemDeltas.filter((i) => !leads(i));
  const phoneObjects = foldObjects && p.itemDeltas.some((i) => i.kind === 'object');
  const otherItems = rest.filter((i) => i.kind !== 'object');
  const lineTier = lineCompositionTier(p.itemDeltas);
  const showComposition = lineTier === null;
  const lock = partyLock(p);
  const fromSm = useFromSm();
  const party = (
    <span
      data-role="party"
      data-party={p.key}
      data-this-address={focus || undefined}
      className={fromSm ? '-ml-3 max-w-full' : undefined}
    >
      <ParticipantRefView participant={p} size="row" compact quiet linked={!focus} />
      {focus && <span className="sr-only">this address</span>}
    </span>
  );
  const lockPart = lock && (
    // LockKind's default form, never a chip (material pass v2, move 5: boxes stay on
    // controls): plain words at the line's own size, only for a lock that is not the default
    // (R9, lane B4)
    <span data-role="lock-kind">
      <LockWordsView words={lock} size="row" />
    </span>
  );
  const notePart = p.lockHash !== null && notes?.has(p.lockHash) && (
    <span data-role="participant-note" className="text-text-dim text-meta">
      {notes.get(p.lockHash)}
    </span>
  );
  const ckb = (
    <span
      data-role="ckb"
      data-fee={fee || undefined}
      className={fromSm ? undefined : 'ml-auto shrink-0 whitespace-nowrap'}
    >
      {/* A feed line: the summary form (R5), the exact change in its title. A fee is no loss to
          mark in the cinnabar: the word says where it went, its amount is unsigned (B4). */}
      {fee ? (
        <>
          <span data-role="fee-label" className="text-text-dim">
            fee
          </span>{' '}
          <CkbAmount form="summary" shannons={-BigInt(p.ckbDelta)} signed={false} tone="plain" />
        </>
      ) : (
        <CkbAmount form="summary" shannons={BigInt(p.ckbDelta)} />
      )}
    </span>
  );
  const occupied = p.usedDelta !== '0' && (
    <span
      data-role="occupied"
      className={cn('text-meta text-text-dim whitespace-nowrap tabular-nums', !fromSm && 'ml-auto')}
    >
      occupied {formatOccupiedDelta(p.usedDelta, `transaction ${txHash}, party ${p.key}`)}
    </span>
  );
  const leadItems = lead.map((item, i) => (
    // the page's own token, before the CKB that holds it (B4)
    <ItemOnLine key={`lead${i}`} item={item} tone={itemTone} showComposition={showComposition} />
  ));
  const items = (
    <>
      {phoneObjects && (
        <span
          data-role="phone-items"
          className="inline-flex flex-wrap items-center gap-x-2.5 md:hidden"
        >
          {otherItems.map((item, i) => (
            <ItemOnLine key={i} item={item} tone={itemTone} showComposition={showComposition} />
          ))}
          {/* the objects as one signed count, the figure that stands for them folded: FoldToggle's
              inline form in its words (the lead's final round, item 3) */}
          <span data-role="object-count">
            <FoldToggle
              form="inline"
              open={objectsOpen}
              count={p.itemDeltas.filter((i) => i.kind === 'object').length}
              singular="object"
              words={objectCountLabel(p.itemDeltas)}
              controls={lineId}
              onToggle={setObjectsOpen}
            />
          </span>
        </span>
      )}
      <ItemsOnLine
        items={rest}
        foldObjects={foldObjects}
        focusItemId={focusItemId}
        itemTone={itemTone}
        showComposition={showComposition}
        className={phoneObjects ? 'hidden md:inline-flex' : undefined}
        controls={lineId}
      />
      {lineTier !== null && (
        <span data-role="composition" className="inline-flex">
          <CompositionBadge tier={lineTier} size="sm" />
        </span>
      )}
    </>
  );
  const openObjects = phoneObjects && objectsOpen && (
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
  );
  // Below sm (the phone-rows board, PA-10): the party on one line, the address and its lock
  // wrapping beside the CKB, which ends the line on the row's one right edge; what else it moved and
  // its occupied bytes on the line under it, the bytes quiet at that line's right.
  if (!fromSm) {
    const moved = lead.length > 0 || rest.length > 0 || lineTier !== null;
    return (
      <div id={lineId} data-role="participant" className="text-small leading-row pl-[18px]">
        <div data-role="party-line" className="flex flex-nowrap items-baseline gap-x-3">
          <span className="min-w-0 flex-1 basis-0 [overflow-wrap:anywhere]">
            {party}
            {lockPart && <> {lockPart}</>}
            {notePart && <> {notePart}</>}
          </span>
          {ckb}
        </div>
        {(moved || occupied) && (
          <div
            data-role="party-figures"
            className="flex flex-wrap items-center gap-x-2.5 gap-y-0 pl-3"
          >
            {leadItems}
            {items}
            {occupied}
          </div>
        )}
        {openObjects}
      </div>
    );
  }
  return (
    <div
      id={lineId}
      data-role="participant"
      // The address sits 18 px in; a line that wraps (phone widths) continues 30 px in, a hanging
      // indent, so the continuation never reads as the next participant.
      className="text-small leading-row flex flex-wrap items-center gap-x-2.5 gap-y-0 pl-[30px]"
    >
      {party}
      {lockPart}
      {notePart}
      {leadItems}
      {ckb}
      {items}
      {occupied}
      {openObjects}
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
 * The parties a row's head shows, the one rule for an ordinary row and a folded run (lane B4 fix
 * round, review NOTE 2): the focus first, always shown (consistency C3), then the others by
 * `participantsToShow` (over six, the five largest, in the feed's order), and how many are folded.
 */
function partiesShown<T extends ActivityParticipant>(
  participants: T[],
  focusLockHash: string | undefined,
  txHash: string,
  expanded: boolean
): { focus: (T & { key: string }) | null; shown: (T & { key: string })[]; hidden: number } {
  const indexed = participants.map((p, i) => ({ ...p, key: partyKey(p, i) }));
  const { focus, others } = splitFocus(indexed, focusLockHash, txHash);
  const { shown, hidden } = participantsToShow(others, expanded);
  return { focus, shown: focus === null ? shown : [focus, ...shown], hidden };
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
  feePayer,
  focusTokenTypeHash,
}: {
  participants: ActivityParticipant[];
  txHash: string;
  focusLockHash?: string;
  /** The index of the party whose only movement is the fee (`feeOnlyParty`), or null. */
  feePayer: number | null;
  focusTokenTypeHash?: string;
  foldObjects?: boolean;
  focusItemId?: string;
  itemTone: 'plain' | 'sign';
  /** A fact the API attaches to one participant, by its lock hash (R-4 "holds the class cell"). */
  notes?: ReadonlyMap<string, string>;
}) {
  const [expanded, setExpanded] = useState(false);
  const listId = useId();
  const { focus, shown, hidden } = partiesShown(participants, focusLockHash, txHash, expanded);
  const foldable = expanded || hidden > 0;
  if (participants.length === 0) return null;
  return (
    <div id={listId} data-role="participants" className="flex min-w-0 flex-col">
      {shown.map((p) => (
        <ParticipantLine
          key={p.key}
          p={p}
          txHash={txHash}
          focus={p === focus}
          fee={feePayer !== null && p.key === partyKey(participants[feePayer], feePayer)}
          foldObjects={foldObjects}
          focusItemId={focusItemId}
          focusTokenTypeHash={focusTokenTypeHash}
          itemTone={itemTone}
          notes={notes}
        />
      ))}
      {/* the participants' way to go under their lines: FoldToggle's box, "all 8 participants ↓"
          and "fewer ↑" on one button (the lead's final round, item 3) */}
      {foldable && (
        <div className="self-start pl-[18px]">
          <FoldToggle
            open={expanded}
            count={participants.length}
            singular="participant"
            controls={listId}
            onToggle={setExpanded}
          />
        </div>
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
  /** F2 objects: fold a batch of objects behind "+N more objects", leading with `focusItemId` (OB-M3). */
  foldObjects?: boolean;
  focusItemId?: string;
  /**
   * Readability B4: the token the page is about (a token page's feed). Its amount leads each
   * participant line that moved it, the CKB after it; any other item keeps its place.
   */
  focusTokenTypeHash?: string;
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
  /** PA-22: a row of a live feed says its age, the date-time its hover; else the date. */
  age?: boolean;
  /**
   * How many of the transaction's parties the row does not list with their changes (the address
   * page lists its own party alone): a line is said to be the fee (`feeOnlyParty`) only on a row
   * that lists every party, since an unlisted party's change is not known to it.
   */
  unlistedParties?: number;
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
  focusTokenTypeHash,
  itemTone = 'sign',
  participantNotes,
  focusLockHash,
  age = false,
  unlistedParties = 0,
}: ActivityRowProps) {
  const shown =
    mode === 'participant' && participant ? { ...activity, participants: [participant] } : activity;
  const [namesOpen, setNamesOpen] = useState(false);
  const namesId = useId();
  const { actions, primaryAction, calls, errors } = activityDetails(activity, showScriptDetails, {
    open: namesOpen,
    onToggle: setNamesOpen,
    controls: namesId,
  });
  // Lane F follow-up 2: over PARTICIPANT_LIMIT lines the run is a list under the head; two to six
  // stay inline while the rendered head stays one line (`headIsOneLine`, measured once per
  // transaction, before paint), else the same list. One line is always inline. Lines are the
  // head's distinct readings: actions that read alike are one line ("×18", A12c).
  const groups = groupProtocolLines(actions);
  const lines = groups.filter((g) => inlineParts(g).length > 0).length;
  const articleRef = useRef<HTMLElement>(null);
  const [fit, setFit] = useState<{ tx: string; fits: boolean } | null>(null);
  const measured = lines >= 2 && lines <= PARTICIPANT_LIMIT;
  const listed =
    lines > PARTICIPANT_LIMIT || (measured && fit?.tx === activity.txHash && !fit.fits);
  useLayoutEffect(() => {
    if (!measured || fit?.tx === activity.txHash) return;
    const article = articleRef.current;
    if (!article)
      throw new Error(`transaction ${activity.txHash}: the activity row is not mounted`);
    setFit({ tx: activity.txHash, fits: headIsOneLine(article, activity.txHash) });
  }, [measured, fit, activity.txHash]);
  const runs: DottedGroup[] = [
    ...(actions.length === 0
      ? []
      : listed
        ? protocolRunSummary(actions, primaryAction)
        : inlineProtocolRuns(groups)),
    ...calls,
  ];
  return (
    <RowArticle
      ref={articleRef}
      data-feed-row={activity.txHash}
      // T7: the hash leaves the visible row; it is the row's hover (and the headline's link).
      // and the age beside it (consistency C3); the row shows the date on a historical list, the
      // age on a live one (PA-22)
      hash={activity.txHash}
      timestamp={activity.timestamp}
      className={cn(ROW_CLASS, fresh && 'bg-jade/5', className)}
    >
      <ActivityRowHead
        layout="line"
        title={<ActivityTitle activity={activity} headline={headline} mark={markOverride} />}
        // A caller's own evidence (block, address pages) carries its own tx link; then the
        // headline stays text, which also keeps a headline with links of its own unnested.
        href={evidence === undefined ? `/tx/${activity.txHash}` : undefined}
        details={runs}
        detailsId={namesId}
        hash={activity.txHash}
        blockNumber={activity.blockNumber}
        timestamp={activity.timestamp}
        showEvidence={showEvidence}
        evidence={evidence}
        poolStatus={activity.poolStatus}
        timeAddedToPool={activity.timeAddedToPool}
        age={age}
      />
      {listed && <ProtocolActionList actions={actions} groups={groups} />}
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
          feePayer={mode === 'cross-user' && unlistedParties === 0 ? feeOnlyParty(activity) : null}
          focusTokenTypeHash={focusTokenTypeHash}
        />
      )}
    </RowArticle>
  );
}

/** The mark a row's title carries: the caller's (null for none), else the activity's class. */
function titleMark(activity: ActivityBody, mark: AssetClass | null | undefined): AssetClass | null {
  return mark === undefined ? activityClassMark(activity) : mark;
}

/**
 * A row's title as its head draws it: its class mark, then its headline (the feed's own words,
 * else the activity's Layer-3 narrative). The one place a title is drawn, for a row and a run.
 */
export function ActivityTitle({
  activity,
  headline,
  mark,
}: {
  activity: ActivityBody;
  headline?: ReactNode;
  mark?: AssetClass | null;
}) {
  const shownMark = titleMark(activity, mark);
  return (
    <>
      {shownMark && <ClassMark kind={shownMark} className="mr-2" />}
      {headline ?? activityHeadline(activity)}
    </>
  );
}

/**
 * The same title in words, what a feed's runs compare (`lib/activity-runs.ts`): the class mark's
 * name and the headline's words. `headlineText` is a feed's own headline in words, as it draws it.
 */
export function activityTitleText(
  activity: ActivityBody,
  headlineText?: string,
  mark?: AssetClass | null
): string {
  const shownMark = titleMark(activity, mark);
  return `${shownMark === null ? '' : `${shownMark}: `}${headlineText ?? activityHeadline(activity)}`;
}

/**
 * A run of rows that did the same thing, folded (readability lane B4; the rule is
 * `lib/activity-runs.ts`, the owner's of Gate B): one row that says what its rows did and called —
 * the title, the scripts by the code they share (no row's args: those differ from row to row) —
 * how many transactions it holds and how many participants they name, and the newest time (the
 * first row's: a feed is newest first). It counts and sums nothing: no amount of any row is drawn,
 * and no total; it names no party, since its rows' parties differ. Its toggle is the site's one
 * fold control (`FoldToggle`: "all N transactions ↓", "fewer ↑"): it opens the ordinary rows in
 * place, under it, and folds them again, stays where it is in both states and keeps the focus.
 */
export function FoldedActivityRow({
  rows,
  participants,
  title,
  showScriptDetails,
  age,
  className,
  fresh = false,
  open,
  controls,
  onToggle,
}: {
  /** The run, newest first: at least two committed rows. */
  rows: readonly ActivityRowData[];
  /** How many parties its rows name, each once (`runParties`). */
  participants: number;
  /** The title its rows draw (`ActivityTitle`). */
  title: ReactNode;
  showScriptDetails: boolean;
  /** As the feed's rows: the age on a live feed, else the date (PA-22). */
  age: boolean;
  /**
   * The class its feed gives its rows, given to the run too: a neighbour's rows on an identity's
   * feed are set back, and so is their run (the feed's one decision, R4).
   */
  className?: string;
  fresh?: boolean;
  open: boolean;
  /**
   * The id of the list its rows are drawn in (`ActivityRuns`'s): opened, the run's rows stand in
   * it under this row, siblings of the feed's other rows, so a row never remounts as it joins or
   * leaves a run.
   */
  controls: string;
  onToggle: () => void;
}) {
  const first = rows[0];
  if (rows.length < 2)
    throw new Error(`transaction ${first?.txHash}: a folded run holds at least two rows`);
  if (first.timestamp === null || first.blockNumber === null)
    throw new Error(`transaction ${first.txHash}: a folded run's rows are committed`);
  if (participants < 1)
    throw new Error(`transaction ${first.txHash}: a folded run names no participant`);
  if (first.itemTypes === null)
    throw new Error(`transaction ${first.txHash}: a folded run's rows state their item types`);
  const [namesOpen, setNamesOpen] = useState(false);
  const namesId = useId();
  // the script code is the same on every row of a run: the first row's item types and calls, by
  // name, are the run's (with no row's count or args), by a row's head's one rule (`headScripts`):
  // "Token transfer · Simple UDT"
  const { calls: details } = activityDetails(
    first,
    showScriptDetails,
    { open: namesOpen, onToggle: setNamesOpen, controls: namesId },
    false,
    first.itemTypes
  );
  const transactions = formatCount(rows.length, 'transaction');
  const toggle = (
    <FoldToggle
      open={open}
      count={rows.length}
      singular="transaction"
      controls={controls}
      onToggle={onToggle}
    />
  );
  return (
    <article
      data-role="activity-run"
      data-run-count={rows.length}
      className={cn(ROW_CLASS, fresh && 'bg-jade/5', className)}
    >
      <ActivityRowHead
        layout="line"
        title={title}
        details={details}
        detailsId={namesId}
        hash={first.txHash}
        blockNumber={first.blockNumber}
        timestamp={first.timestamp}
        evidence={
          <>
            newest <RowDate timestamp={first.timestamp} age={age} />
          </>
        }
      />
      {/* what the run holds, counted: the toggle first, so it stands in one place open or not */}
      <div data-role="run-count" className="text-small leading-row text-text pl-[18px]">
        {/* "participants": every party its rows name, the count the stores can state today. The
            owner's word is "senders" (a party one of whose cells the transaction consumed): it
            becomes that count here once the indexer records, per participant, that its lock holds
            an input (the per-participant input flag, a re-sync; the route then states it and
            `runParties` counts the parties that sent). This is the one place that writes it. */}
        <Dotted parts={[toggle, open && transactions, formatCount(participants, 'participant')]} />
      </div>
    </article>
  );
}
