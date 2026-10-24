'use client';

import { Fragment } from 'react';
import Link from '@/components/ui/link';
import {
  getScriptDetailHref,
  getObjectDetailHref,
  getIdentityItemDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatCkbAmount, truncateHash, cn } from '@/lib/utils';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { PoolInterpretationNotice, PoolStatusBadge, TimeInPool } from '@/components/ui/pool-status';
import { ParticipantRefView } from '@/components/participant-ref';
import type {
  Activity,
  GlobalActivity,
  ParticipantInfo,
  ItemDelta,
  ActivityTypeCall,
  ActivityLockCall,
  ActivityProtocolAction,
} from '@/lib/api';

// ---------------------------------------------------------------------------
// Shared helpers (extracted from latest-activities.tsx)
// ---------------------------------------------------------------------------

export function formatStandard(standard: string): string {
  if (standard === 'spore') return 'Spore';
  if (standard === 'm-nft') return 'M-NFT';
  if (standard === 'dotbit') return '.bit';
  if (standard === 'did_ckb') return 'did:ckb';
  return standard.charAt(0).toUpperCase() + standard.slice(1);
}

export function capitalizeAction(action: string): string {
  if (!action) return '';
  return action.charAt(0).toUpperCase() + action.slice(1);
}

export const TYPE_SCRIPT_CALL_LABEL = 'Script Call (type)';
export const LOCK_SCRIPT_CALL_LABEL = 'Script Call (lock)';

export function formatScriptRef(sc: { scriptHash: string; scriptName?: string }): string {
  if (sc.scriptName?.trim()) return sc.scriptName!.trim();
  return sc.scriptHash.slice(0, 10);
}

// ---------------------------------------------------------------------------
// CkbDelta — color-coded CKB amount
// ---------------------------------------------------------------------------

export function CkbDelta({ delta }: { delta: string }) {
  const value = BigInt(delta);
  const formatted = formatCkbAmount(delta);
  const isPositive = value > BigInt(0);
  const isNegative = value < BigInt(0);

  return (
    <span
      className={cn(
        'font-mono text-xs tabular-nums',
        isPositive && 'text-positive',
        isNegative && 'text-negative',
        !isPositive && !isNegative && 'text-text-dim'
      )}
    >
      {isPositive ? '+' : ''}
      {formatted.full} CKB
    </span>
  );
}

// ---------------------------------------------------------------------------
// TypeCallExpr — script name link with args
// ---------------------------------------------------------------------------

export function TypeCallExpr({ sc }: { sc: ActivityTypeCall }) {
  const fnName = formatScriptRef(sc);
  const args = truncateHash(sc.typeArgs, 6, 4);

  return (
    <>
      <Link
        href={getScriptDetailHref({
          name: sc.scriptName,
          codeHash: sc.typeCodeHash,
          hashType: sc.typeHashType,
          scriptKind: 'type',
        })}
        className="text-gold hover:text-gold/80 transition-colors"
        onClick={(e) => e.stopPropagation()}
      >
        {fnName}
      </Link>
      <span className="text-text-dim">(</span>
      <span className="text-aqua/70">{args}</span>
      <span className="text-text-dim">)</span>
    </>
  );
}

// ---------------------------------------------------------------------------
// LockCallExpr — lock script name link with args
// ---------------------------------------------------------------------------

export function LockCallExpr({ lc }: { lc: ActivityLockCall }) {
  const fnName = formatScriptRef({ scriptHash: lc.scriptHash, scriptName: lc.scriptName });
  const args = truncateHash(lc.lockArgs, 6, 4);

  return (
    <>
      <Link
        href={getScriptDetailHref({
          name: lc.scriptName,
          codeHash: lc.lockCodeHash,
          hashType: lc.lockHashType,
          scriptKind: 'lock',
        })}
        className="text-gold hover:text-gold/80 transition-colors"
        onClick={(e) => e.stopPropagation()}
      >
        {fnName}
      </Link>
      <span className="text-text-dim">(</span>
      <span className="text-aqua/70">{args}</span>
      <span className="text-text-dim">)</span>
    </>
  );
}

// ---------------------------------------------------------------------------
// LockCallBadge — compact pill label
// ---------------------------------------------------------------------------

export function LockCallBadge({ lc }: { lc: ActivityLockCall }) {
  const name =
    (lc.decoded?.protocol as string) || lc.scriptName?.trim() || lc.lockCodeHash.slice(0, 10);

  return (
    <span className="text-text-dim bg-base-elevated/80 rounded px-1 py-0.5 font-mono text-[9px] uppercase">
      {name}
    </span>
  );
}

// ---------------------------------------------------------------------------
// EventParts — badge (left) and value (right) as separate elements
// ---------------------------------------------------------------------------

export interface EventParts {
  badge: React.ReactNode;
  value: React.ReactNode;
}

function getItemDeltaEventParts(item: ItemDelta): EventParts {
  switch (item.kind) {
    case 'token': {
      const delta = BigInt(item.delta);
      const isPositive = delta > BigInt(0);
      const isZero = delta === BigInt(0);
      const prefix = isZero ? '' : isPositive ? '+' : '-';
      const absDelta = item.delta.startsWith('-') ? item.delta.slice(1) : item.delta;
      const formatted = formatTokenBalanceWithRawMarker(absDelta, item.decimals ?? null);
      const color = isZero ? 'text-text-dim' : isPositive ? 'text-positive' : 'text-negative';
      const symbol = item.symbol?.trim();
      const label = symbol || truncateHash(item.typeScriptHash, 8, 6);
      return {
        badge: (
          <span className="text-token font-mono text-xs">
            {'\u25CF'} {label} Transfer
          </span>
        ),
        value: (
          <Link
            href={getTokenDetailHref(item.typeScriptHash)}
            className={cn(
              'font-mono text-xs tabular-nums transition-colors hover:underline',
              color
            )}
            onClick={(e) => e.stopPropagation()}
          >
            {prefix}
            {formatted} {label}
          </Link>
        ),
      };
    }
    case 'object': {
      const isAdd = item.delta > 0;
      const actionLabel = isAdd ? 'Received' : 'Sent';
      return {
        badge: (
          <span className="text-lavender font-mono text-xs">
            {'\u2B21'} Object {actionLabel}
          </span>
        ),
        value: (
          <Link
            href={getObjectDetailHref(item.objectId)}
            className="text-lavender/80 hover:text-lavender font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(item.objectId, 8, 6)}
          </Link>
        ),
      };
    }
    case 'identity': {
      const isAdd = item.delta > 0;
      const actionLabel = isAdd ? 'Registered' : 'Released';
      return {
        badge: (
          <span className="text-aqua font-mono text-xs">
            {'\u2736'} Identity {actionLabel}
          </span>
        ),
        value: (
          <Link
            href={getIdentityItemDetailHref(item.standard, item.identityId)}
            className="text-aqua/80 hover:text-aqua font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(item.identityId, 8, 6)}
          </Link>
        ),
      };
    }
  }
}

/** The label of a DAO step, as its badge and the compact summary both print it. */
function daoActionLabel(action: string): string {
  switch (action) {
    case 'deposit':
      return 'DAO Deposit';
    case 'withdraw_request':
      return 'DAO Withdraw Request';
    case 'withdraw_complete':
      return 'DAO Withdraw Complete';
    default:
      return 'DAO';
  }
}

function getDaoEventParts(pa: ActivityProtocolAction): EventParts {
  const capacity = pa.metadata?.capacity as string | undefined;
  const compensation = pa.metadata?.compensation as string | undefined;
  const label = daoActionLabel(pa.action);

  switch (pa.action) {
    case 'deposit':
      return {
        badge: (
          <span className="text-gold font-mono text-xs">
            {'\u25C6'} {label}
          </span>
        ),
        value: (
          <span className="text-positive font-mono text-xs tabular-nums">
            +{capacity ? formatCkbAmount(capacity).full : '0'} CKB locked
          </span>
        ),
      };
    case 'withdraw_request':
      return {
        badge: (
          <span className="text-gold font-mono text-xs">
            {'\u25C6'} {label}
          </span>
        ),
        value: (
          <span className="text-gold font-mono text-xs tabular-nums">
            {capacity ? formatCkbAmount(capacity).full : '0'} CKB
          </span>
        ),
      };
    case 'withdraw_complete':
      return {
        badge: (
          <span className="text-positive font-mono text-xs">
            {'\u25C6'} {label}
          </span>
        ),
        value: (
          <div className="flex flex-col items-end gap-0.5">
            <span className="text-positive font-mono text-xs tabular-nums">
              +{capacity ? formatCkbAmount(capacity).full : '0'} CKB
            </span>
            {compensation && (
              <span className="text-positive font-mono text-[10px] tabular-nums">
                +{formatCkbAmount(compensation).full} CKB compensation
              </span>
            )}
          </div>
        ),
      };
    default:
      return {
        badge: (
          <span className="text-gold font-mono text-xs">
            {'\u25C6'} {label}
          </span>
        ),
        value: capacity ? (
          <span className="text-gold font-mono text-xs tabular-nums">
            {formatCkbAmount(capacity).full} CKB
          </span>
        ) : null,
      };
  }
}

function getTypeEventParts(sc: ActivityTypeCall): EventParts {
  return {
    badge: (
      <span className="text-amber font-mono text-xs">
        {'\u2699'} {TYPE_SCRIPT_CALL_LABEL}
      </span>
    ),
    value: (
      <span className="font-mono text-xs">
        <TypeCallExpr sc={sc} />
      </span>
    ),
  };
}

function getLockEventParts(lc: ActivityLockCall): EventParts {
  return {
    badge: (
      <span className="text-violet font-mono text-xs">
        {'\u26A1'} {LOCK_SCRIPT_CALL_LABEL}
      </span>
    ),
    value: (
      <span className="font-mono text-xs">
        <LockCallExpr lc={lc} />
      </span>
    ),
  };
}

function formatProtocolAction(action: string): string {
  return action.replace(/_/g, ' ');
}

const FIBER_ACTION_LABELS: Record<string, string> = {
  channel_open: 'Channel Open',
  channel_close: 'Channel Close',
  force_close: 'Force Close',
  settlement: 'Settlement',
};

function getFiberActionLabel(action: string): string {
  return FIBER_ACTION_LABELS[action] ?? formatProtocolAction(action);
}

/** The label of a protocol action (`DAO Deposit`, `fiber · Channel Open`, `utxoswap · swap`). */
export function protocolActionLabel(pa: ActivityProtocolAction): string {
  if (pa.protocol === 'dao') return daoActionLabel(pa.action);
  const actionLabel =
    pa.protocol === 'fiber' ? getFiberActionLabel(pa.action) : formatProtocolAction(pa.action);
  return `${pa.protocol} \u00B7 ${actionLabel}`;
}

function getProtocolActionEventParts(pa: ActivityProtocolAction): EventParts {
  // DAO is handled separately via getDaoEventParts
  if (pa.protocol === 'dao') {
    return getDaoEventParts(pa);
  }

  const isFiber = pa.protocol === 'fiber';
  const label = protocolActionLabel(pa);
  const btcTxid = pa.metadata?.btcTxid as string | undefined;
  const capacity = pa.metadata?.capacity as string | undefined;

  return {
    badge: (
      <span className={cn('font-mono text-xs', isFiber ? 'text-violet' : 'text-orange')}>
        {isFiber ? '\u26A1' : '\u2B21'} {label}
      </span>
    ),
    value: btcTxid ? (
      <span className="text-text-dim font-mono text-xs">btc:{truncateHash(btcTxid, 8, 6)}</span>
    ) : isFiber && capacity ? (
      <span className="text-text-dim font-mono text-xs">{formatCkbAmount(capacity).full} CKB</span>
    ) : null,
  };
}

/** `Coinbase` for a cellbase, `CKB Transfer` when CKB is all the tx moves, else `CKB`. */
export function ckbEventLabel(isCellbase: boolean, isPrimaryAction: boolean): string {
  return isCellbase ? 'Coinbase' : isPrimaryAction ? 'CKB Transfer' : 'CKB';
}

/**
 * CKB event parts.
 * - "Coinbase" for cellbase transactions
 * - "CKB Transfer" when this is the primary action (no L2/L3 events)
 * - "CKB" when the CKB change is just a position side-effect of L2/L3 (fee + capacity)
 */
function getCkbEventParts(
  delta: string,
  isCellbase: boolean,
  isPrimaryAction: boolean
): EventParts {
  const icon = isCellbase ? '\u2605' : '\u2197';
  const label = ckbEventLabel(isCellbase, isPrimaryAction);
  const colorClass = isCellbase ? 'text-gold' : 'text-jade';

  return {
    badge: (
      <span className={cn('font-mono text-xs', colorClass)}>
        {icon} {label}
      </span>
    ),
    value: <CkbDelta delta={delta} />,
  };
}

// ---------------------------------------------------------------------------
// ActivityEventGroup — full TX group: tx hash + all event sub-rows
// ---------------------------------------------------------------------------

export interface ActivityEventGroupProps {
  activity: Activity;
  formatTimeAgo: (timestamp: string | number) => string;
  /** Whether this is the first group — controls the top separator line. */
  isFirst?: boolean;
}

/**
 * Renders a TX group as both:
 *  - Narrow (< md): a self-contained stacked card
 *  - Wide (>= md): bare grid cells — the **parent** must be the grid container
 *    (`md:grid` with `gridTemplateColumns: '13rem 1fr auto 5rem'`)
 */
export function ActivityEventGroup({
  activity,
  formatTimeAgo,
  isFirst = true,
}: ActivityEventGroupProps) {
  const events: EventParts[] = [];

  activity.protocolActions.forEach((pa) => {
    events.push(getProtocolActionEventParts(pa));
  });
  activity.itemDeltas.forEach((item) => {
    events.push(getItemDeltaEventParts(item));
  });
  activity.typeCalls.forEach((sc) => {
    events.push(getTypeEventParts(sc));
  });
  const protocolNames = new Set(
    activity.protocolActions.map((pa: ActivityProtocolAction) => pa.protocol)
  );
  activity.lockCalls.forEach((lc) => {
    const decodedProtocol = lc.decoded?.protocol as string | undefined;
    if (decodedProtocol && protocolNames.has(decodedProtocol)) return;
    events.push(getLockEventParts(lc));
  });
  // CKB is "Transfer" only when there are no L2/L3 events — otherwise it's
  // just the position side-effect (capacity change + fee).
  const isPrimaryCkb = events.length === 0;
  events.push(getCkbEventParts(activity.ckbDelta, activity.isCellbase, isPrimaryCkb));

  const txLink = `/tx/${activity.txHash}`;
  // A transaction still in the node's pool has no block and no block time. It
  // reports where it stands and how long it has waited instead — never a
  // block link, and never a timestamp it does not have.
  const isInPool = activity.blockNumber === null;
  const blockLink = `/blocks/${activity.blockNumber}`;
  const time = activity.timestamp === null ? null : formatTimeAgo(Number(activity.timestamp));

  const position =
    isInPool && activity.poolStatus ? (
      <PoolStatusBadge status={activity.poolStatus} />
    ) : (
      <Link href={blockLink} className="text-text-dim hover:text-text transition-colors">
        #{activity.blockNumber?.toLocaleString()}
      </Link>
    );

  const when =
    isInPool && activity.timeAddedToPool ? (
      <TimeInPool since={activity.timeAddedToPool} className="text-warning font-mono text-[10px]" />
    ) : (
      <span className="text-text-dim font-mono text-[10px]">{time}</span>
    );

  // Vertical padding helper — first/last rows of a group get more padding
  const cellPy = (i: number): string => {
    const first = i === 0;
    const last = i === events.length - 1;
    if (first && last) return 'py-2';
    if (first) return 'pt-2 pb-0.5';
    if (last) return 'pt-0.5 pb-2';
    return 'py-0.5';
  };

  return (
    <>
      {/* === Narrow viewport (< md): self-contained stacked card === */}
      <div className="border-base-border/50 border-b px-4 py-2 last:border-b-0 md:hidden">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-1.5 font-mono text-xs">
            <Link href={txLink} className="text-text hover:text-aqua transition-colors">
              {truncateHash(activity.txHash, 8, 6)}
            </Link>
            <span className="text-text-dim">{'\u00B7'}</span>
            {position}
          </div>
          <span className="shrink-0">{when}</span>
        </div>
        <PoolInterpretationNotice
          interpretation={activity.interpretation}
          className="text-warning mt-1 block font-mono text-[10px]"
        />
        <div className="mt-1 space-y-0.5 pl-2">
          {events.map((event, i) => (
            <div key={i} className="flex items-center justify-between gap-2">
              <div className="min-w-0 truncate">{event.badge}</div>
              <div className="shrink-0">{event.value}</div>
            </div>
          ))}
        </div>
      </div>

      {/* === Wide viewport (>= md): bare grid cells — parent provides the grid === */}
      {/* Group separator — full-width border between TX groups */}
      {!isFirst && (
        <div
          className="border-base-border/50 hidden border-t md:block"
          style={{ gridColumn: '1 / -1' }}
        />
      )}
      {events.map((event, i) => (
        <Fragment key={i}>
          {/* Col 1: TX hash — first row only */}
          <div className={cn('hidden items-center pl-4 md:flex', cellPy(i))}>
            {i === 0 ? (
              <div className="flex items-center gap-1.5 font-mono text-xs">
                <Link href={txLink} className="text-text hover:text-aqua transition-colors">
                  {truncateHash(activity.txHash, 8, 6)}
                </Link>
                <span className="text-text-dim">{'\u00B7'}</span>
                {position}
              </div>
            ) : null}
          </div>

          {/* Col 2: Badge — right-aligned to sit close to value column */}
          <div className={cn('hidden min-w-0 truncate text-right md:block', cellPy(i))}>
            {event.badge}
          </div>

          {/* Col 3: Value — right-aligned, auto-sized */}
          <div className={cn('hidden text-right md:block', cellPy(i))}>{event.value}</div>

          {/* Col 4: Time — first row only */}
          <div className={cn('hidden pr-4 text-right md:block', cellPy(i))}>
            {i === 0 ? when : null}
          </div>
        </Fragment>
      ))}
      {activity.interpretation?.status === 'partial' && (
        <div className="hidden px-4 pb-2 text-right md:block" style={{ gridColumn: '1 / -1' }}>
          <PoolInterpretationNotice interpretation={activity.interpretation} />
        </div>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Global activity helpers — tx-centric layered view for multi-participant
// ---------------------------------------------------------------------------

/** Inline item delta: compact signed amount + label, for participant summary lines. */
function InlineItemDelta({ item }: { item: ItemDelta }) {
  switch (item.kind) {
    case 'token': {
      const delta = BigInt(item.delta);
      const prefix = delta > BigInt(0) ? '+' : delta < BigInt(0) ? '-' : '';
      const absDelta = item.delta.startsWith('-') ? item.delta.slice(1) : item.delta;
      const formatted = formatTokenBalanceWithRawMarker(absDelta, item.decimals ?? null);
      const label = item.symbol?.trim() || truncateHash(item.typeScriptHash, 8, 6);
      const color =
        delta > BigInt(0) ? 'text-positive' : delta < BigInt(0) ? 'text-negative' : 'text-text-dim';
      return (
        <Link
          href={getTokenDetailHref(item.typeScriptHash)}
          className={cn('font-mono text-xs tabular-nums hover:underline', color)}
          onClick={(e) => e.stopPropagation()}
        >
          {prefix}
          {formatted} {label}
        </Link>
      );
    }
    case 'object': {
      const prefix = item.delta > 0 ? '+' : '';
      return (
        <Link
          href={getObjectDetailHref(item.objectId)}
          className="text-lavender/80 hover:text-lavender font-mono text-xs transition-colors"
          onClick={(e) => e.stopPropagation()}
        >
          {prefix}
          {item.delta} {truncateHash(item.objectId, 8, 6)}
        </Link>
      );
    }
    case 'identity': {
      const prefix = item.delta > 0 ? '+' : '';
      return (
        <Link
          href={getIdentityItemDetailHref(item.standard, item.identityId)}
          className="text-aqua/80 hover:text-aqua font-mono text-xs transition-colors"
          onClick={(e) => e.stopPropagation()}
        >
          {prefix}
          {item.delta} {truncateHash(item.identityId, 8, 6)}
        </Link>
      );
    }
  }
}

/** Single participant line: party + CKB delta + item deltas (L1 + L2). */
export function ParticipantLine({
  participant,
  muted = false,
}: {
  participant: ParticipantInfo;
  /** Draw the party in the neutral ink (see ParticipantRefView). */
  muted?: boolean;
}) {
  const showCkb = participant.ckbDelta !== '0';

  return (
    <div className="flex items-center justify-between gap-2">
      <ParticipantRefView participant={participant} compact muted={muted} />
      <div className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5">
        {showCkb && <CkbDelta delta={participant.ckbDelta} />}
        {participant.itemDeltas.map((item, i) => (
          <InlineItemDelta key={i} item={item} />
        ))}
      </div>
    </div>
  );
}

/** One tx-level event of a global activity: an L3 protocol action or an L2 type / lock call. */
export type TxEvent =
  | { kind: 'protocol'; action: ActivityProtocolAction }
  | { kind: 'type'; call: ActivityTypeCall }
  | { kind: 'lock'; call: ActivityLockCall };

/**
 * The tx-level events of a GlobalActivity, in display order: L3 protocol actions, then the L2
 * catch-all type calls, then the lock calls a protocol action does not already explain. The one
 * classification both the full event rows and the compact summary render.
 */
export function globalTxEvents(activity: GlobalActivity): TxEvent[] {
  const events: TxEvent[] = activity.protocolActions.map((action) => ({
    kind: 'protocol' as const,
    action,
  }));
  for (const call of activity.typeCalls) events.push({ kind: 'type', call });
  const protocolNames = new Set(
    activity.protocolActions.map((pa: ActivityProtocolAction) => pa.protocol)
  );
  for (const call of activity.lockCalls) {
    const decoded = call.decoded?.protocol as string | undefined;
    if (decoded && protocolNames.has(decoded)) continue;
    events.push({ kind: 'lock', call });
  }
  return events;
}

/**
 * Build tx-level event rows for a GlobalActivity.
 * Returns L3 protocol actions + L2 catch-all type/lock calls.
 * Per-participant data should be rendered separately via ParticipantLine.
 */
export function buildGlobalTxEvents(activity: GlobalActivity): EventParts[] {
  return globalTxEvents(activity).map((e) =>
    e.kind === 'protocol'
      ? getProtocolActionEventParts(e.action)
      : e.kind === 'type'
        ? getTypeEventParts(e.call)
        : getLockEventParts(e.call)
  );
}

/** One run of the compact summary: an optional dim count (`2 type calls`) and the names. */
export interface TxSummaryGroup {
  count: string | null;
  names: string[];
}

function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}

/**
 * The one-line summary of a tx's events for a compact feed row: its protocol actions by name, then
 * `N type calls` and `N lock calls` with the distinct script names. A tx without events reads as
 * its CKB movement (`CKB Transfer`, or `Coinbase` for a cellbase), as the full rows label it.
 */
export function summarizeTxEvents(events: TxEvent[], isCellbase: boolean): TxSummaryGroup[] {
  const protocols = events.flatMap((e) => (e.kind === 'protocol' ? [e.action] : []));
  const types = events.flatMap((e) => (e.kind === 'type' ? [e.call] : []));
  const locks = events.flatMap((e) => (e.kind === 'lock' ? [e.call] : []));
  const groups: TxSummaryGroup[] = [];
  if (protocols.length > 0) {
    groups.push({ count: null, names: protocols.map(protocolActionLabel) });
  }
  if (types.length > 0) {
    groups.push({
      count: plural(types.length, 'type call'),
      names: [...new Set(types.map((sc) => formatScriptRef(sc)))],
    });
  }
  if (locks.length > 0) {
    groups.push({
      count: plural(locks.length, 'lock call'),
      names: [
        ...new Set(
          locks.map((lc) =>
            formatScriptRef({ scriptHash: lc.scriptHash, scriptName: lc.scriptName })
          )
        ),
      ],
    });
  }
  if (groups.length === 0) {
    groups.push({ count: null, names: [ckbEventLabel(isCellbase, true)] });
  }
  return groups;
}

/** The compact summary of a global activity, as one line of text (truncated by its container). */
export function TxEventSummary({
  activity,
  className,
}: {
  activity: GlobalActivity;
  className?: string;
}) {
  const groups = summarizeTxEvents(globalTxEvents(activity), activity.isCellbase);
  return (
    <span data-testid="tx-event-summary" className={cn('min-w-0 truncate', className)}>
      {groups.map((g, i) => (
        <Fragment key={i}>
          {i > 0 && <span className="text-text-dim">{' \u00B7 '}</span>}
          {g.count && <span className="text-text-dim">{`${g.count}  `}</span>}
          <span className="text-text">{g.names.join(' \u00B7 ')}</span>
        </Fragment>
      ))}
    </span>
  );
}
