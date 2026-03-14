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
import { formatTokenBalance } from '@/lib/format-asset';
import type {
  Activity,
  ActivityAssetChange,
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

interface EventParts {
  badge: React.ReactNode;
  value: React.ReactNode;
}

function getAssetEventParts(change: ActivityAssetChange): EventParts {
  switch (change.type) {
    case 'daoDeposit':
      return {
        badge: <span className="text-gold font-mono text-xs">{'\u25C6'} DAO Deposit</span>,
        value: (
          <span className="text-positive font-mono text-xs tabular-nums">
            +{formatCkbAmount(change.capacity).full} CKB locked
          </span>
        ),
      };
    case 'daoWithdrawRequest':
      return {
        badge: <span className="text-gold font-mono text-xs">{'\u25C6'} DAO Withdraw Request</span>,
        value: (
          <span className="text-gold font-mono text-xs tabular-nums">
            {formatCkbAmount(change.capacity).full} CKB
          </span>
        ),
      };
    case 'daoWithdrawComplete':
      return {
        badge: (
          <span className="text-positive font-mono text-xs">{'\u25C6'} DAO Withdraw Complete</span>
        ),
        value: (
          <div className="flex flex-col items-end gap-0.5">
            <span className="text-positive font-mono text-xs tabular-nums">
              +{formatCkbAmount(change.capacity).full} CKB
            </span>
            <span className="text-positive font-mono text-[10px] tabular-nums">
              +{formatCkbAmount(change.compensation).full} CKB compensation
            </span>
          </div>
        ),
      };
    case 'token': {
      const delta = BigInt(change.delta);
      const isPositive = delta > BigInt(0);
      const isZero = delta === BigInt(0);
      const prefix = isZero ? '' : isPositive ? '+' : '-';
      const absDelta = change.delta.startsWith('-') ? change.delta.slice(1) : change.delta;
      const formatted = formatTokenBalance(absDelta, change.decimals ?? 0);
      const color = isZero ? 'text-text-dim' : isPositive ? 'text-positive' : 'text-negative';
      const symbol = change.symbol?.trim();
      const label = symbol || truncateHash(change.typeScriptHash, 8, 6);
      return {
        badge: (
          <span className="font-mono text-xs text-[#ff66aa]">
            {'\u25CF'} {label} Transfer
          </span>
        ),
        value: (
          <Link
            href={getTokenDetailHref(change.typeScriptHash)}
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
      const std = formatStandard(change.standard);
      const action = capitalizeAction(change.action);
      return {
        badge: (
          <span className="text-lavender font-mono text-xs">
            {'\u2B21'} {std} {action}
          </span>
        ),
        value: (
          <Link
            href={getObjectDetailHref(change.objectId)}
            className="text-lavender/80 hover:text-lavender font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(change.objectId, 8, 6)}
          </Link>
        ),
      };
    }
    case 'identity': {
      const std = formatStandard(change.standard);
      const action = capitalizeAction(change.action);
      return {
        badge: (
          <span className="text-aqua font-mono text-xs">
            {'\u2736'} {std} {action}
          </span>
        ),
        value: (
          <Link
            href={getIdentityItemDetailHref(change.standard, change.identityId)}
            className="text-aqua/80 hover:text-aqua font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(change.identityId, 8, 6)}
          </Link>
        ),
      };
    }
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

function getProtocolActionEventParts(pa: ActivityProtocolAction): EventParts {
  const isFiber = pa.protocol === 'fiber';
  const actionLabel = isFiber ? getFiberActionLabel(pa.action) : formatProtocolAction(pa.action);
  const label = `${pa.protocol} \u00B7 ${actionLabel}`;
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

function getCkbEventParts(delta: string, isCellbase: boolean): EventParts {
  const icon = isCellbase ? '\u2605' : '\u2197';
  const label = isCellbase ? 'Coinbase' : 'CKB Transfer';
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
 *  - Wide (≥ md): bare grid cells — the **parent** must be the grid container
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
  activity.assetChanges.forEach((change) => {
    events.push(getAssetEventParts(change));
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
  events.push(getCkbEventParts(activity.ckbDelta, activity.isCellbase));

  const txLink = `/tx/${activity.txHash}`;
  const blockLink = `/blocks/${activity.blockNumber}`;
  const time = formatTimeAgo(Number(activity.timestamp));

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
            <Link href={blockLink} className="text-text-dim hover:text-text transition-colors">
              #{activity.blockNumber.toLocaleString()}
            </Link>
          </div>
          <span className="text-text-dim shrink-0 font-mono text-[10px]">{time}</span>
        </div>
        <div className="mt-1 space-y-0.5 pl-2">
          {events.map((event, i) => (
            <div key={i} className="flex items-center justify-between gap-2">
              {event.badge}
              {event.value}
            </div>
          ))}
        </div>
      </div>

      {/* === Wide viewport (≥ md): bare grid cells — parent provides the grid === */}
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
                <Link href={blockLink} className="text-text-dim hover:text-text transition-colors">
                  #{activity.blockNumber.toLocaleString()}
                </Link>
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
            {i === 0 ? <span className="text-text-dim font-mono text-[10px]">{time}</span> : null}
          </div>
        </Fragment>
      ))}
    </>
  );
}
