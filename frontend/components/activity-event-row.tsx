'use client';

import Link from '@/components/ui/link';
import {
  getScriptDetailHref,
  getObjectDetailHref,
  getIdentityItemDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatCkbAmount, truncateHash, cn } from '@/lib/utils';
import { formatTokenBalance } from '@/lib/format-asset';
import type { Activity, ActivityAssetChange, ActivityScriptCall } from '@/lib/api';

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

export function formatScriptRef(sc: {
  scriptHash: string;
  typeHashType: string;
  scriptName?: string;
}): string {
  if (sc.scriptName?.trim()) return sc.scriptName!.trim();
  // script_kind:<first 4 bytes of script_hash>
  const hashPrefix = sc.scriptHash.slice(0, 10); // "0x" + 8 hex chars = 4 bytes
  return `${sc.typeHashType}:${hashPrefix}`;
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
// ScriptCallExpr — script name link with args
// ---------------------------------------------------------------------------

function ScriptCallExpr({ sc }: { sc: ActivityScriptCall }) {
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
// Event sub-row rendering — one row per asset change / script call / CKB delta
// ---------------------------------------------------------------------------

interface EventRowProps {
  className?: string;
}

function AssetEventRow({ change, className }: { change: ActivityAssetChange } & EventRowProps) {
  switch (change.type) {
    case 'daoDeposit':
      return (
        <div className={cn('flex items-center justify-between gap-2', className)}>
          <span className="text-gold font-mono text-xs">{'\u25C6'} DAO Deposit</span>
          <span className="text-positive font-mono text-xs tabular-nums">
            +{formatCkbAmount(change.capacity).full} CKB locked
          </span>
        </div>
      );
    case 'daoWithdrawRequest':
      return (
        <div className={cn('flex items-center justify-between gap-2', className)}>
          <span className="text-gold font-mono text-xs">{'\u25C6'} DAO Withdraw Request</span>
          <span className="text-gold font-mono text-xs tabular-nums">
            {formatCkbAmount(change.capacity).full} CKB
          </span>
        </div>
      );
    case 'daoWithdrawComplete':
      return (
        <div className={cn('flex flex-col gap-0.5', className)}>
          <div className="flex items-center justify-between gap-2">
            <span className="text-positive font-mono text-xs">
              {'\u25C6'} DAO Withdraw Complete
            </span>
            <span className="text-positive font-mono text-xs tabular-nums">
              +{formatCkbAmount(change.capacity).full} CKB
            </span>
          </div>
          <div className="flex justify-end">
            <span className="text-positive font-mono text-[10px] tabular-nums">
              +{formatCkbAmount(change.compensation).full} CKB compensation
            </span>
          </div>
        </div>
      );
    case 'token': {
      const delta = BigInt(change.delta);
      const isPositive = delta > BigInt(0);
      const isZero = delta === BigInt(0);
      const sign = isZero ? '' : isPositive ? '+' : '';
      const absDelta = change.delta.startsWith('-') ? change.delta.slice(1) : change.delta;
      const formatted = formatTokenBalance(absDelta, change.decimals ?? 0);
      const color = isZero ? 'text-text-dim' : isPositive ? 'text-positive' : 'text-negative';
      const symbol = change.symbol?.trim();
      const label = symbol || truncateHash(change.typeScriptHash, 8, 6);
      return (
        <div className={cn('flex items-center justify-between gap-2', className)}>
          <span className="font-mono text-xs text-[#ff66aa]">
            {'\u25CF'} {label} Transfer
          </span>
          <Link
            href={getTokenDetailHref(change.typeScriptHash)}
            className={cn(
              'font-mono text-xs tabular-nums transition-colors hover:underline',
              color
            )}
            onClick={(e) => e.stopPropagation()}
          >
            {sign}
            {change.delta.startsWith('-') ? '-' : ''}
            {formatted} {label}
          </Link>
        </div>
      );
    }
    case 'object': {
      const std = formatStandard(change.standard);
      const action = capitalizeAction(change.action);
      return (
        <div className={cn('flex items-center justify-between gap-2', className)}>
          <span className="text-lavender font-mono text-xs">
            {'\u2B21'} {std} {action}
          </span>
          <Link
            href={getObjectDetailHref(change.objectId)}
            className="text-lavender/80 hover:text-lavender font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(change.objectId, 8, 6)}
          </Link>
        </div>
      );
    }
    case 'identity': {
      const std = formatStandard(change.standard);
      const action = capitalizeAction(change.action);
      return (
        <div className={cn('flex items-center justify-between gap-2', className)}>
          <span className="text-aqua font-mono text-xs">
            {'\u2736'} {std} {action}
          </span>
          <Link
            href={getIdentityItemDetailHref(change.standard, change.identityId)}
            className="text-aqua/80 hover:text-aqua font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(change.identityId, 8, 6)}
          </Link>
        </div>
      );
    }
  }
}

function ScriptEventRow({ sc, className }: { sc: ActivityScriptCall } & EventRowProps) {
  return (
    <div className={cn('flex items-center justify-between gap-2', className)}>
      <span className="text-amber min-w-0 truncate font-mono text-xs">
        {'\u2699'} Script call <ScriptCallExpr sc={sc} />
      </span>
    </div>
  );
}

function CkbEventRow({
  delta,
  isCellbase,
  className,
}: {
  delta: string;
  isCellbase: boolean;
} & EventRowProps) {
  const icon = isCellbase ? '\u2605' : '\u2197';
  const label = isCellbase ? 'Coinbase' : 'CKB Transfer';
  const colorClass = isCellbase ? 'text-gold' : 'text-jade';

  return (
    <div className={cn('flex items-center justify-between gap-2', className)}>
      <span className={cn('font-mono text-xs', colorClass)}>
        {icon} {label}
      </span>
      <CkbDelta delta={delta} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// ActivityEventGroup — full TX group: tx hash + all event sub-rows
// ---------------------------------------------------------------------------

export interface ActivityEventGroupProps {
  activity: Activity;
  formatTimeAgo: (timestamp: string | number) => string;
}

export function ActivityEventGroup({ activity, formatTimeAgo }: ActivityEventGroupProps) {
  const eventRows: React.ReactNode[] = [];

  // 1. Asset changes
  activity.assetChanges.forEach((change, i) => {
    eventRows.push(<AssetEventRow key={`asset-${i}`} change={change} />);
  });

  // 2. Script calls
  activity.scriptCalls.forEach((sc, i) => {
    eventRows.push(<ScriptEventRow key={`script-${i}`} sc={sc} />);
  });

  // 3. CKB delta — always shown
  eventRows.push(
    <CkbEventRow key="ckb" delta={activity.ckbDelta} isCellbase={activity.isCellbase} />
  );

  const txLink = `/tx/${activity.txHash}`;
  const blockLink = `/blocks/${activity.blockNumber}`;
  const time = formatTimeAgo(Number(activity.timestamp));

  return (
    <div className="border-base-border/50 border-b px-4 py-2 last:border-b-0">
      {/* Narrow viewport: stacked layout */}
      <div className="md:hidden">
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
        <div className="mt-1 space-y-0.5 pl-2">{eventRows}</div>
      </div>

      {/* Wide viewport: TX hash column + event rows inline */}
      <div className="hidden md:block">
        {eventRows.map((row, i) => (
          <div key={i} className="flex items-center gap-4">
            {/* TX hash column — only shown on first row */}
            <div className="w-56 shrink-0 lg:w-64">
              {i === 0 ? (
                <div className="flex items-center gap-1.5 font-mono text-xs">
                  <Link href={txLink} className="text-text hover:text-aqua transition-colors">
                    {truncateHash(activity.txHash, 8, 6)}
                  </Link>
                  <span className="text-text-dim">{'\u00B7'}</span>
                  <Link
                    href={blockLink}
                    className="text-text-dim hover:text-text transition-colors"
                  >
                    #{activity.blockNumber.toLocaleString()}
                  </Link>
                </div>
              ) : null}
            </div>

            {/* Event row — fills remaining space */}
            <div className="min-w-0 flex-1">{row}</div>

            {/* Time — only shown on first row */}
            <div className="w-20 shrink-0 text-right">
              {i === 0 ? <span className="text-text-dim font-mono text-[10px]">{time}</span> : null}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
