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
import { classifyActivity, type ActivityBody } from '@/lib/activity-classify';
export type { ActivityBody } from '@/lib/activity-classify';
import { MINUS } from '@/lib/chart-ticks';
import { splitCkbAmount } from '@/lib/ckb-decimal';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
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
import { formatBlockNumber } from '@/lib/format-block';
import { ClassMark } from '@/components/ui/class-mark';
import { CompositionBadge } from '@/components/ui/composition-mark';
import type { AssetClass } from '@/lib/design-tokens';

/** A row that is not a pool row sits in a block; an absent number is a contract violation. */
function committedActivityBlock(activity: { blockNumber: number | null; txHash: string }): number {
  if (activity.blockNumber === null)
    throw new Error(`Activity ${activity.txHash} has neither a block nor a pool status`);
  return activity.blockNumber;
}

// ---------------------------------------------------------------------------
// Shared helpers (extracted from latest-activities.tsx)
// ---------------------------------------------------------------------------

export function formatStandard(standard: string): string {
  if (standard === 'xudt') return 'xUDT';
  if (standard === 'xudt_compatible') return 'xUDT-compatible';
  if (standard === 'sudt') return 'sUDT';
  if (standard === 'spore') return 'Spore';
  if (standard === 'm-nft') return 'M-NFT';
  if (standard === 'dotbit') return '.bit';
  if (standard === 'bit_cell') return '.bit Cell';
  if (standard === 'dotcell') return '.cell';
  if (standard === 'did_ckb') return 'did:ckb';
  return standard.charAt(0).toUpperCase() + standard.slice(1);
}

export function capitalizeAction(action: string): string {
  if (!action) return '';
  return action.charAt(0).toUpperCase() + action.slice(1);
}

export const TYPE_SCRIPT_CALL_LABEL = 'Script Call (type)';
export const LOCK_SCRIPT_CALL_LABEL = 'Script Call (lock)';

/**
 * A call's script as a reader names it: the registry name, else its code hash (what kind of
 * script it is), never the script hash, which reads as `code(args)` (F2-tx, TX-31).
 */
export function formatScriptRef(sc: { codeHash: string; scriptName?: string }): string {
  if (sc.scriptName?.trim()) return sc.scriptName!.trim();
  return truncateHash(sc.codeHash, 8, 4);
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

/** A call's name and args; `quiet` draws the name as an ink link and the args dim (no gold). */
function CallExpr({
  name,
  href,
  args,
  quiet,
}: {
  name: string;
  href: string;
  /** Null when the script has empty args: the boards print the name alone (F2-tx). */
  args: string | null;
  quiet: boolean;
}) {
  return (
    <>
      <Link
        href={href}
        className={cn(
          'transition-colors',
          quiet ? 'text-text hover:text-jade' : 'text-gold hover:text-gold/80'
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {name}
      </Link>
      {args !== null && (
        <>
          <span className="text-text-dim">(</span>
          <span className={quiet ? 'text-text-dim' : 'text-aqua/70'}>{args}</span>
          <span className="text-text-dim">)</span>
        </>
      )}
    </>
  );
}

export function TypeCallExpr({ sc, quiet = false }: { sc: ActivityTypeCall; quiet?: boolean }) {
  return (
    <CallExpr
      name={formatScriptRef({ codeHash: sc.typeCodeHash, scriptName: sc.scriptName })}
      href={getScriptDetailHref({
        name: sc.scriptName,
        codeHash: sc.typeCodeHash,
        hashType: sc.typeHashType,
        scriptKind: 'type',
      })}
      args={sc.typeArgs === '0x' ? null : truncateHash(sc.typeArgs, 6, 4)}
      quiet={quiet}
    />
  );
}

// ---------------------------------------------------------------------------
// LockCallExpr — lock script name link with args
// ---------------------------------------------------------------------------

export function LockCallExpr({ lc, quiet = false }: { lc: ActivityLockCall; quiet?: boolean }) {
  return (
    <CallExpr
      name={formatScriptRef({ codeHash: lc.lockCodeHash, scriptName: lc.scriptName })}
      href={getScriptDetailHref({
        name: lc.scriptName,
        codeHash: lc.lockCodeHash,
        hashType: lc.lockHashType,
        scriptKind: 'lock',
      })}
      args={lc.lockArgs === '0x' ? null : truncateHash(lc.lockArgs, 6, 4)}
      quiet={quiet}
    />
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
  // protocolActionMeta throws when a deposit / withdraw step lacks its capacity (or a completed
  // withdraw its compensation), so below those are present; nothing is ever shown as 0 instead.
  const { capacity, compensation } = protocolActionMeta(pa);
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
            +{formatCkbAmount(capacity as bigint).full} CKB locked
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
            {formatCkbAmount(capacity as bigint).full} CKB
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
              +{formatCkbAmount(capacity as bigint).full} CKB
            </span>
            {compensation !== null && (
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
        value:
          capacity !== null ? (
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

/** The label of a protocol action (`DAO Deposit`, `Fiber · Channel Open`, `UTXOSwap · swap`). */
/**
 * A `.cell` action in words. `ring_relink` is the indexer's own record of a ring pointer rewrite
 * nothing else in the tx explains; it names the neighbour whose pointer changed when known.
 */
export function dotcellActionLabel(action: string, name: string | null = null): string {
  if (action === 'ring_relink')
    return name ? `ring pointer changed for ${name}` : 'ring pointer changed';
  return formatProtocolAction(action);
}

export function protocolActionLabel(pa: ActivityProtocolAction): string {
  if (pa.protocol === 'dao') return daoActionLabel(pa.action);
  if (pa.protocol === 'dotcell') {
    const name = pa.metadata?.name;
    return `${pa.protocolName} \u00B7 ${dotcellActionLabel(pa.action, typeof name === 'string' ? name : null)}`;
  }
  if (pa.protocol === 'rgbpp') {
    const labels: Record<string, string> = {
      leap_to_btc: 'Leap to Bitcoin',
      btc_time_locked: 'Bitcoin time lock',
      leap_to_ckb: 'Leap to CKB',
      transfer: 'Transfer',
      receive: 'Receive',
    };
    return `${pa.protocolName} · ${labels[pa.action] ?? formatProtocolAction(pa.action)}`;
  }
  const actionLabel =
    pa.protocol === 'fiber' ? getFiberActionLabel(pa.action) : formatProtocolAction(pa.action);
  return `${pa.protocolName} \u00B7 ${actionLabel}`;
}

/** An activity part that cannot be read, said in its row with the tx it belongs to. */
export function ActivityReadError({ txHash, error }: { txHash: string; error: unknown }) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <span
      role="alert"
      data-testid="activity-read-error"
      className="text-rouge block whitespace-normal break-all font-mono text-xs"
    >
      {`Cannot read tx ${txHash}: ${message}`}
    </span>
  );
}

/** A protocol action's row parts, or — when its metadata cannot be read — an error in their place. */
function readProtocolActionEventParts(pa: ActivityProtocolAction, txHash: string): EventParts {
  try {
    return getProtocolActionEventParts(pa);
  } catch (e) {
    return { badge: <ActivityReadError txHash={txHash} error={e} />, value: null };
  }
}

function getProtocolActionEventParts(pa: ActivityProtocolAction): EventParts {
  // DAO is handled separately via getDaoEventParts
  if (pa.protocol === 'dao') {
    return getDaoEventParts(pa);
  }

  const isFiber = pa.protocol === 'fiber';
  const label = protocolActionLabel(pa);
  const { btcTxid, capacity } = protocolActionMeta(pa);

  return {
    badge: (
      <span className={cn('font-mono text-xs', isFiber ? 'text-violet' : 'text-orange')}>
        {isFiber ? '\u26A1' : '\u2B21'} {label}
      </span>
    ),
    value: btcTxid ? (
      <span className="text-text-dim font-mono text-xs">btc:{truncateHash(btcTxid, 8, 6)}</span>
    ) : isFiber && capacity !== null ? (
      <span className="text-text-dim font-mono text-xs">{formatCkbAmount(capacity).full} CKB</span>
    ) : null,
  };
}

/** `Cellbase` for a cellbase (the site's word), `CKB Transfer` when CKB is all the tx moves, else `CKB`. */
export function ckbEventLabel(isCellbase: boolean, isPrimaryAction: boolean): string {
  return isCellbase ? 'Cellbase' : isPrimaryAction ? 'CKB Transfer' : 'CKB';
}

/**
 * CKB event parts.
 * - "Cellbase" for cellbase transactions
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
    events.push(readProtocolActionEventParts(pa, activity.txHash));
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
        {formatBlockNumber(committedActivityBlock(activity))}
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
export function ParticipantLine({ participant }: { participant: ParticipantInfo }) {
  const showCkb = participant.ckbDelta !== '0';

  return (
    <div className="flex items-center justify-between gap-2">
      <ParticipantRefView participant={participant} compact />
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
export function globalTxEvents(activity: ActivityBody): TxEvent[] {
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
      ? readProtocolActionEventParts(e.action, activity.txHash)
      : e.kind === 'type'
        ? getTypeEventParts(e.call)
        : getLockEventParts(e.call)
  );
}

/** Net change of the same item across every participant decides mint/burn/transfer. */
function itemMovement(activity: ActivityBody, primary: ItemDelta | null): string {
  if (!primary)
    throw new Error(
      `transaction ${activity.txHash ?? 'uncommitted'}: item headline has no item delta`
    );
  const sameItem = (item: ItemDelta) => {
    if (primary.kind === 'token')
      return item.kind === 'token' && item.typeScriptHash === primary.typeScriptHash;
    if (primary.kind === 'object')
      return item.kind === 'object' && item.objectId === primary.objectId;
    return (
      item.kind === 'identity' &&
      item.identityId === primary.identityId &&
      item.standard === primary.standard
    );
  };
  let net = BigInt(0);
  for (const participant of activity.participants)
    for (const item of participant.itemDeltas) {
      if (!sameItem(item)) continue;
      const delta = String(item.delta);
      if (!/^-?\d+$/.test(delta))
        throw new Error(
          `transaction ${activity.txHash ?? 'uncommitted'}: invalid ${item.kind} delta ${delta}`
        );
      net += BigInt(delta);
    }
  return net > BigInt(0) ? 'mint' : net < BigInt(0) ? 'burn' : 'transfer';
}

/** The asset class a feed headline is about, or null for CKB transfers and script calls. */
export function activityClassMark(activity: ActivityBody): AssetClass | null {
  const displayType = classifyActivity(activity).displayType;
  return displayType.startsWith('dao')
    ? 'dao'
    : displayType === 'token'
      ? 'tokens'
      : displayType === 'object'
        ? 'objects'
        : displayType === 'identity'
          ? 'identities'
          : null;
}

/**
 * The Layer-3 narrative of a global activity, as the feed heads it: the classifier picks the
 * highest non-empty layer, and the existing labels name it.
 */
export function activityHeadline(activity: ActivityBody): string {
  const c = classifyActivity(activity);
  switch (c.displayType) {
    case 'daoDeposit':
    case 'daoWithdrawRequest':
    case 'daoWithdrawComplete':
    case 'protocolAction':
      return protocolActionLabel(c.primaryProtocolAction as ActivityProtocolAction);
    case 'token':
      return `Token ${itemMovement(activity, c.primaryItemDelta)}`;
    case 'object':
      return `Object ${itemMovement(activity, c.primaryItemDelta)}`;
    case 'identity':
      return `Identity ${itemMovement(activity, c.primaryItemDelta)}`;
    case 'typeCall':
      return 'Script call';
    case 'ckbTransfer':
      return ckbEventLabel(activity.isCellbase, true);
  }
}

/**
 * Exact CKB, all 8 decimals: integer, the significant decimals, then the trailing zeros ghosted,
 * so right-aligned amounts line up on the decimal point. `tone="sign"` colours by sign.
 */
export function ExactCkb({
  shannons,
  signed = true,
  tone = 'sign',
  wrap = false,
  unit = true,
}: {
  shannons: bigint;
  signed?: boolean;
  tone?: 'sign' | 'plain';
  /** Allow a line break before the decimal or unit, preserving every digit. */
  wrap?: boolean;
  /** F2 blocks: false when a column header already names the unit ("Reward, CKB", BA-46). */
  unit?: boolean;
}) {
  const parts = splitCkbAmount(shannons);
  const color =
    tone === 'plain'
      ? 'text-text'
      : parts.sign === '+'
        ? 'text-positive'
        : parts.sign === MINUS
          ? 'text-negative'
          : 'text-text';
  return (
    <span
      data-ckb-amount
      className={cn(
        wrap
          ? 'inline-flex max-w-full flex-wrap justify-end tabular-nums'
          : 'whitespace-nowrap tabular-nums',
        color
      )}
    >
      <span className="whitespace-nowrap">
        {signed ? parts.sign : ''}
        {parts.integer}
      </span>
      <span className="whitespace-nowrap">
        <span className="opacity-[0.78]">{parts.fraction}</span>
        <span data-ghost className="text-text-ghost">
          {parts.ghost}
        </span>
      </span>
      {unit && <span className="text-text-dim whitespace-pre text-[11px]"> CKB</span>}
    </span>
  );
}

/**
 * A participant's Layer-2 change as a fact: `±amount SYMBOL`, or `<id> arrived / departed`.
 * `tone="sign"` (F2-tx, for the Address board) colours a token amount by its sign; the default
 * draws it bright, as the Activities, Token and Transaction boards do.
 */
export function ItemDeltaFact({
  item,
  tone = 'plain',
}: {
  item: ItemDelta;
  tone?: 'plain' | 'sign';
}) {
  switch (item.kind) {
    case 'token': {
      const delta = BigInt(item.delta);
      const sign = delta > BigInt(0) ? '+' : delta < BigInt(0) ? MINUS : '';
      const abs = item.delta.startsWith('-') ? item.delta.slice(1) : item.delta;
      const label = item.symbol?.trim() || truncateHash(item.typeScriptHash, 8, 6);
      return (
        <span className="inline-flex items-baseline gap-1.5">
          <ClassMark kind="tokens" size={12} className="self-center" />
          <Link
            href={getTokenDetailHref(item.typeScriptHash)}
            className={cn(
              'hover:text-jade tabular-nums transition-colors',
              tone === 'plain'
                ? 'text-text-bright'
                : delta > BigInt(0)
                  ? 'text-positive'
                  : delta < BigInt(0)
                    ? 'text-negative'
                    : 'text-text-bright'
            )}
            onClick={(e) => e.stopPropagation()}
          >
            {sign}
            {formatTokenBalanceWithRawMarker(abs, item.decimals ?? null)} {label}
          </Link>
        </span>
      );
    }
    case 'object': {
      const collectionName = item.collection?.name?.trim();
      return (
        <span className="text-text-bright">
          <ClassMark kind="objects" size={12} className="mr-1.5" />
          <span className="whitespace-nowrap">
            {collectionName ? `${collectionName} ` : 'object '}
            <Link
              href={getObjectDetailHref(item.objectId)}
              className="hover:text-jade transition-colors"
              onClick={(e) => e.stopPropagation()}
            >
              {truncateHash(item.objectId, 8, 4)}
            </Link>
          </span>{' '}
          <span className="text-text-dim">{item.delta > 0 ? 'arrived' : 'departed'}</span>
          {item.composition && (
            <CompositionBadge tier={item.composition} size="sm" className="ml-1.5 align-middle" />
          )}
        </span>
      );
    }
    case 'identity':
      return (
        <span className="text-text-bright">
          <ClassMark kind="identities" size={12} className="mr-1.5" />
          <span className="whitespace-nowrap">
            {`${formatStandard(item.standard)} `}
            <Link
              href={getIdentityItemDetailHref(item.standard, item.identityId)}
              className="hover:text-jade transition-colors"
              onClick={(e) => e.stopPropagation()}
            >
              {truncateHash(item.identityId, 8, 4)}
            </Link>
          </span>{' '}
          <span className="text-text-dim">{item.delta > 0 ? 'arrived' : 'departed'}</span>
        </span>
      );
  }
}

/**
 * What a protocol action's metadata says, as quiet parts of one line: DAO amounts and the deposit
 * block, a Fiber channel and its capacity, an RGB++ BTC txid. Empty when the action carries none.
 */
export function protocolActionNoteParts(pa: ActivityProtocolAction): React.ReactNode[] {
  const meta = protocolActionMeta(pa);
  const parts: React.ReactNode[] = [];
  if (meta.channelOutpoint) {
    // `0x<funding tx hash>:<index>`: shorten the hash, keep the index whole.
    const [hash, index] = meta.channelOutpoint.split(':');
    parts.push(`channel ${truncateHash(hash, 10, 4)}${index === undefined ? '' : `:${index}`}`);
  }
  if (meta.capacity !== null) {
    parts.push(
      <>
        <ExactCkb shannons={meta.capacity} signed={false} tone="plain" />
        {pa.protocol === 'dao' && pa.action === 'deposit' ? ' locked' : ''}
      </>
    );
  }
  if (meta.depositBlock !== null) {
    parts.push(`deposited at ${formatBlockNumber(meta.depositBlock)}`);
  }
  if (meta.compensation !== null) {
    parts.push(
      <>
        +<ExactCkb shannons={meta.compensation} signed={false} tone="plain" /> compensation
      </>
    );
  }
  if (pa.protocol === 'rgbpp') {
    const transitions: Record<string, string> = {
      leap_to_btc: 'CKB lock → Bitcoin-controlled lock',
      btc_time_locked: 'RGB++ lock → Bitcoin time lock',
      leap_to_ckb: 'Bitcoin-linked lock → CKB lock',
      transfer: 'Bitcoin-controlled cells change ownership',
      receive: 'New Bitcoin-controlled cells',
    };
    if (transitions[pa.action]) parts.push(transitions[pa.action]);
  }
  if (meta.btcTxid)
    parts.push(
      <span className="text-[#dc8a5a]" title={meta.btcTxid}>
        Bitcoin tx {truncateHash(meta.btcTxid, 8, 6)}
        {meta.btcOutputIndex !== null ? `:${meta.btcOutputIndex}` : ''}
      </span>
    );
  return parts;
}

/** Parts joined with a dim middle dot. */
export function DotJoined({ parts }: { parts: React.ReactNode[] }) {
  return (
    <>
      {parts.map((part, i) => (
        <Fragment key={i}>
          {i > 0 && <span className="text-text-dim">{' \u00B7 '}</span>}
          {part}
        </Fragment>
      ))}
    </>
  );
}

/** A protocol action's metadata as one quiet line, or nothing when it carries none. */
export function ProtocolActionNote({ pa }: { pa: ActivityProtocolAction }) {
  const parts = protocolActionNoteParts(pa);
  return parts.length === 0 ? null : <DotJoined parts={parts} />;
}
