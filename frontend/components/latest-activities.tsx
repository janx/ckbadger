'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api, type GlobalActivity } from '@/lib/api';
import { classifyActivity, type ClassifiedActivity } from '@/lib/activity-classify';
import {
  getObjectDetailHref,
  getIdentityItemDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatTimeAgo, formatCkbAmount, truncateHash, cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import {
  CkbDelta,
  TypeCallExpr,
  LockCallExpr,
  LockCallBadge,
  TYPE_SCRIPT_CALL_LABEL,
} from '@/components/activity-event-row';

const MAX_STREAM_ITEMS = 20;

const STREAM_KEYFRAMES = `
@keyframes stream-slide-in {
  from { transform: translateY(-100%); opacity: 0; }
  to { transform: translateY(0); opacity: 1; }
}
@keyframes stream-glow-fade {
  from { background-color: rgba(46, 219, 163, 0.1); }
  to { background-color: transparent; }
}
`;

function ensureKeyframes() {
  if (typeof document === 'undefined') return;
  if (document.getElementById('stream-keyframes')) return;
  const style = document.createElement('style');
  style.id = 'stream-keyframes';
  style.textContent = STREAM_KEYFRAMES;
  document.head.appendChild(style);
}

function truncateAddress(addr: string): string {
  return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
}

function formatAddress(address: string): string {
  if (address.startsWith('ckb1') || address.startsWith('ckt1')) {
    return truncateAddress(address);
  }
  return truncateHash(address);
}

/** Get a unique key for a global activity. TX-level now, so txHash is sufficient. */
function itemKey(activity: GlobalActivity): string {
  return activity.txHash;
}

/** Get the primary address from the first participant. */
function primaryAddress(activity: GlobalActivity): string {
  return activity.participants[0]?.address ?? '';
}

/** Get the primary CKB delta from the first participant. */
function primaryCkbDelta(activity: GlobalActivity): string {
  return activity.participants[0]?.ckbDelta ?? '0';
}

interface TypeBadgeInfo {
  icon: string;
  label: string;
  colorClass: string;
}

function getTypeBadge(classified: ClassifiedActivity): TypeBadgeInfo {
  const { displayType, primaryItemDelta } = classified;

  switch (displayType) {
    case 'daoDeposit':
      return { icon: '\u25C6', label: 'DAO Deposit', colorClass: 'text-gold' };
    case 'daoWithdrawRequest':
      return { icon: '\u25C6', label: 'DAO Withdraw Request', colorClass: 'text-gold' };
    case 'daoWithdrawComplete':
      return { icon: '\u25C6', label: 'DAO Withdraw Complete', colorClass: 'text-positive' };
    case 'token': {
      if (primaryItemDelta && primaryItemDelta.kind === 'token') {
        const label = primaryItemDelta.symbol ?? truncateHash(primaryItemDelta.typeScriptHash, 8, 6);
        return { icon: '\u25CE', label: `${label} Transfer`, colorClass: 'text-token' };
      }
      return { icon: '\u25CE', label: 'Token Transfer', colorClass: 'text-token' };
    }
    case 'object': {
      if (primaryItemDelta && primaryItemDelta.kind === 'object') {
        const actionLabel = primaryItemDelta.delta > 0 ? 'Received' : 'Sent';
        return { icon: '\u2B21', label: `Object ${actionLabel}`, colorClass: 'text-lavender' };
      }
      return { icon: '\u2B21', label: 'Object', colorClass: 'text-lavender' };
    }
    case 'identity': {
      if (primaryItemDelta && primaryItemDelta.kind === 'identity') {
        const actionLabel = primaryItemDelta.delta > 0 ? 'Registered' : 'Released';
        return { icon: '\u2726', label: `Identity ${actionLabel}`, colorClass: 'text-aqua' };
      }
      return { icon: '\u2726', label: 'Identity', colorClass: 'text-aqua' };
    }
    case 'protocolAction': {
      const pa = classified.primaryProtocolAction;
      const label = pa ? pa.protocol.toUpperCase() : 'Protocol';
      return { icon: '\u26A1', label, colorClass: 'text-violet' };
    }
    case 'typeCall':
      return { icon: '\u2699', label: TYPE_SCRIPT_CALL_LABEL, colorClass: 'text-amber' };
    case 'ckbTransfer':
      return { icon: '\u2197', label: 'CKB Transfer', colorClass: 'text-jade' };
    default:
      return { icon: '\u2197', label: 'Transfer', colorClass: 'text-jade' };
  }
}

function AddressLink({ address }: { address: string }) {
  return (
    <Link
      href={`/address/${address}`}
      className="text-text hover:text-aqua font-mono text-xs transition-colors"
      onClick={(e) => e.stopPropagation()}
    >
      {formatAddress(address)}
    </Link>
  );
}

function StreamItemCkbTransfer({ classified }: { classified: ClassifiedActivity }) {
  const { activity } = classified;
  const badge = getTypeBadge(classified);
  const addr = primaryAddress(activity);
  const ckbDelta = primaryCkbDelta(activity);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span className={cn('font-mono text-xs', badge.colorClass)}>
            {badge.icon} {badge.label}
          </span>
          {classified.primaryLockCall && <LockCallBadge lc={classified.primaryLockCall} />}
        </div>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        <CkbDelta delta={ckbDelta} />
      </div>
    </>
  );
}

function StreamItemDaoDeposit({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryProtocolAction } = classified;
  const badge = getTypeBadge(classified);
  const capacity = (primaryProtocolAction?.metadata?.capacity as string) ?? '0';
  const addr = primaryAddress(activity);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('font-mono text-xs', badge.colorClass)}>
          {badge.icon} {badge.label}
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        <span className="text-positive font-mono text-xs tabular-nums">
          +{formatCkbAmount(capacity).full} CKB locked
        </span>
      </div>
    </>
  );
}

function StreamItemDaoWithdrawRequest({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryProtocolAction } = classified;
  const badge = getTypeBadge(classified);
  const capacity = (primaryProtocolAction?.metadata?.capacity as string) ?? '0';
  const addr = primaryAddress(activity);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('font-mono text-xs', badge.colorClass)}>
          {badge.icon} {badge.label}
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        <span className="text-gold font-mono text-xs tabular-nums">
          {formatCkbAmount(capacity).full} CKB
        </span>
      </div>
    </>
  );
}

function StreamItemDaoWithdrawComplete({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryProtocolAction } = classified;
  const badge = getTypeBadge(classified);
  const capacity = (primaryProtocolAction?.metadata?.capacity as string) ?? '0';
  const compensation = (primaryProtocolAction?.metadata?.compensation as string) ?? '0';
  const addr = primaryAddress(activity);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('font-mono text-xs', badge.colorClass)}>
          {badge.icon} {badge.label}
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        <span className="text-positive font-mono text-xs tabular-nums">
          +{formatCkbAmount(capacity).full} CKB
        </span>
      </div>
      <div className="flex justify-end">
        <span className="text-positive font-mono text-[10px] tabular-nums">
          +{formatCkbAmount(compensation).full} CKB compensation
        </span>
      </div>
    </>
  );
}

function StreamItemToken({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryItemDelta } = classified;
  const badge = getTypeBadge(classified);
  const addr = primaryAddress(activity);
  const ckbDelta = primaryCkbDelta(activity);

  let tokenDelta = '';
  let typeScriptHash = '';
  if (primaryItemDelta && primaryItemDelta.kind === 'token') {
    const delta = BigInt(primaryItemDelta.delta);
    const sign = delta > BigInt(0) ? '+' : '';
    const symbol = primaryItemDelta.symbol;
    typeScriptHash = primaryItemDelta.typeScriptHash;
    tokenDelta = `${sign}${primaryItemDelta.delta}${symbol ? ` ${symbol}` : ''}`;
  }

  const ckbValue = BigInt(ckbDelta);
  const showCkbDelta = ckbValue !== BigInt(0);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span className={cn('font-mono text-xs', badge.colorClass)}>
            {badge.icon} {badge.label}
          </span>
          {classified.primaryLockCall && <LockCallBadge lc={classified.primaryLockCall} />}
        </div>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        {typeScriptHash ? (
          <Link
            href={getTokenDetailHref(typeScriptHash)}
            className="text-token hover:text-token-bright font-mono text-xs tabular-nums transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {tokenDelta}
          </Link>
        ) : (
          <span className="text-token font-mono text-xs tabular-nums">{tokenDelta}</span>
        )}
      </div>
      {showCkbDelta && (
        <div className="flex justify-end">
          <CkbDelta delta={ckbDelta} />
        </div>
      )}
    </>
  );
}

function StreamItemObject({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryItemDelta } = classified;
  const badge = getTypeBadge(classified);
  const addr = primaryAddress(activity);

  let objectId = '';
  if (primaryItemDelta && primaryItemDelta.kind === 'object') {
    objectId = primaryItemDelta.objectId;
  }

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('font-mono text-xs', badge.colorClass)}>
          {badge.icon} {badge.label}
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        {objectId ? (
          <Link
            href={getObjectDetailHref(objectId)}
            className="text-lavender/80 hover:text-lavender font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(objectId, 8, 6)}
          </Link>
        ) : (
          <span className="text-text-dim font-mono text-xs">--</span>
        )}
      </div>
    </>
  );
}

function StreamItemIdentity({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryItemDelta } = classified;
  const badge = getTypeBadge(classified);
  const addr = primaryAddress(activity);

  let identityId = '';
  if (primaryItemDelta && primaryItemDelta.kind === 'identity') {
    identityId = primaryItemDelta.identityId;
  }

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('font-mono text-xs', badge.colorClass)}>
          {badge.icon} {badge.label}
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        {identityId ? (
          <Link
            href={getIdentityItemDetailHref('identity', identityId)}
            className="text-aqua/80 hover:text-aqua font-mono text-xs transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            {truncateHash(identityId, 8, 6)}
          </Link>
        ) : (
          <span className="text-text-dim font-mono text-xs">--</span>
        )}
      </div>
    </>
  );
}

function StreamItemTypeCall({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryTypeCall } = classified;
  const badge = getTypeBadge(classified);
  const addr = primaryAddress(activity);
  const ckbDelta = primaryCkbDelta(activity);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('min-w-0 truncate font-mono text-xs', badge.colorClass)}>
          {badge.icon} {TYPE_SCRIPT_CALL_LABEL}{' '}
          {primaryTypeCall ? <TypeCallExpr sc={primaryTypeCall} /> : null}
        </span>
        <span className="text-text-dim shrink-0 font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        <CkbDelta delta={ckbDelta} />
      </div>
    </>
  );
}

const FIBER_ACTION_LABELS: Record<string, string> = {
  channel_open: 'Channel Open',
  channel_close: 'Channel Close',
  force_close: 'Force Close',
  settlement: 'Settlement',
};

const STABLEPP_ACTION_LABELS: Record<string, string> = {
  open_vault: 'Open Vault',
  borrow: 'Borrow',
  repay: 'Repay',
  close_vault: 'Close Vault',
  deposit: 'Deposit',
  adjust: 'Adjust Vault',
  liquidation: 'Liquidation',
  redemption: 'Redemption',
  interaction: 'Interaction',
};

const PROTOCOL_ACTION_LABELS: Record<string, Record<string, string>> = {
  fiber: FIBER_ACTION_LABELS,
  stablepp: STABLEPP_ACTION_LABELS,
};

function StreamItemProtocolAction({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryProtocolAction, primaryLockCall, primaryItemDelta } = classified;
  const addr = primaryAddress(activity);
  const ckbDelta = primaryCkbDelta(activity);

  // Layer 3: prefer ProtocolAction, fall back to legacy lock-call-only path
  const protocolName = primaryProtocolAction
    ? primaryProtocolAction.protocol
    : (primaryLockCall?.decoded?.protocol as string) ||
      primaryLockCall?.scriptName?.trim() ||
      'Protocol';
  const action = primaryProtocolAction
    ? (PROTOCOL_ACTION_LABELS[primaryProtocolAction.protocol]?.[primaryProtocolAction.action] ??
      primaryProtocolAction.action.replace(/_/g, ' '))
    : (primaryLockCall?.decoded?.intentType as string) ||
      (primaryLockCall?.decoded?.action as string) ||
      '';

  // Layer 2: carried asset summary (e.g., "+1,000 XUDT")
  let assetDetail: React.ReactNode = null;
  if (primaryItemDelta && primaryItemDelta.kind === 'token') {
    const delta = BigInt(primaryItemDelta.delta);
    const sign = delta > BigInt(0) ? '+' : '';
    const symbol = primaryItemDelta.symbol;
    assetDetail = (
      <span className="text-token font-mono text-[10px] tabular-nums">
        {sign}
        {primaryItemDelta.delta}
        {symbol ? ` ${symbol}` : ''}
      </span>
    );
  } else if (
    primaryProtocolAction?.protocol === 'fiber' &&
    primaryProtocolAction?.metadata?.capacity
  ) {
    const cap = primaryProtocolAction.metadata.capacity as string;
    assetDetail = (
      <span className="text-text-dim font-mono text-[10px] tabular-nums">
        {formatCkbAmount(cap).full} CKB
      </span>
    );
  }

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className="text-violet min-w-0 truncate font-mono text-xs">
          {'\u26A1'} <span className="text-violet">{protocolName}</span>
          {action ? (
            <>
              <span className="text-text-dim"> · </span>
              <span className="text-violet/70">{action}</span>
            </>
          ) : primaryLockCall ? (
            <>
              <span className="text-text-dim"> · </span>
              <LockCallExpr lc={primaryLockCall} />
            </>
          ) : null}
        </span>
        <span className="text-text-dim shrink-0 font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        {addr && <AddressLink address={addr} />}
        <CkbDelta delta={ckbDelta} />
      </div>
      {assetDetail && <div className="flex justify-end">{assetDetail}</div>}
    </>
  );
}

function StreamItem({ classified }: { classified: ClassifiedActivity }) {
  switch (classified.displayType) {
    case 'ckbTransfer':
      return <StreamItemCkbTransfer classified={classified} />;
    case 'daoDeposit':
      return <StreamItemDaoDeposit classified={classified} />;
    case 'daoWithdrawRequest':
      return <StreamItemDaoWithdrawRequest classified={classified} />;
    case 'daoWithdrawComplete':
      return <StreamItemDaoWithdrawComplete classified={classified} />;
    case 'token':
      return <StreamItemToken classified={classified} />;
    case 'object':
      return <StreamItemObject classified={classified} />;
    case 'identity':
      return <StreamItemIdentity classified={classified} />;
    case 'protocolAction':
      return <StreamItemProtocolAction classified={classified} />;
    case 'typeCall':
      return <StreamItemTypeCall classified={classified} />;
    default:
      return <StreamItemCkbTransfer classified={classified} />;
  }
}

interface LatestActivitiesProps {
  isRealtime?: boolean;
  queryLimit?: number;
  maxItems?: number;
  showViewAllLink?: boolean;
  scrollable?: boolean;
  panelClassName?: string;
}

export function LatestActivities({
  isRealtime = false,
  queryLimit = 32,
  maxItems = MAX_STREAM_ITEMS,
  showViewAllLink = true,
  scrollable = false,
  panelClassName,
}: LatestActivitiesProps) {
  useEffect(() => ensureKeyframes(), []);
  const [newItemKeys, setNewItemKeys] = useState<Set<string>>(new Set());
  const prevKeysRef = useRef<Set<string>>(new Set());

  const {
    data: activities,
    isLoading,
    isFetching,
  } = useQuery({
    queryKey: ['latest-activities', queryLimit],
    queryFn: () => api.getLatestActivities(queryLimit),
    refetchInterval: 10000,
  });

  const itemCount = activities?.length ?? 0;
  const showSkeleton = isLoading || (itemCount === 0 && isFetching);

  const classifiedItems = useMemo<ClassifiedActivity[]>(
    () => (activities ? activities.slice(0, maxItems).map((a) => classifyActivity(a)) : []),
    [activities, maxItems]
  );

  useEffect(() => {
    if (classifiedItems.length > 0) {
      const currentKeys = new Set(classifiedItems.map((c) => itemKey(c.activity)));
      const prevKeys = prevKeysRef.current;
      prevKeysRef.current = currentKeys;

      if (prevKeys.size > 0) {
        const freshKeys = new Set<string>();
        for (const key of currentKeys) {
          if (!prevKeys.has(key)) {
            freshKeys.add(key);
          }
        }
        if (freshKeys.size > 0) {
          setNewItemKeys(freshKeys);
          const timer = setTimeout(() => setNewItemKeys(new Set()), 2000);
          return () => clearTimeout(timer);
        }
      }
    }
  }, [classifiedItems]);

  const headerActions = showViewAllLink ? (
    <Link
      href="/activities"
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
    >
      VIEW ALL &rarr;
    </Link>
  ) : null;

  return (
    <TerminalPanel
      variant="default"
      glow={isRealtime}
      className={cn('flex h-[38rem] flex-col', panelClassName)}
    >
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        Latest Activities
      </TerminalPanelHeader>
      <TerminalPanelContent padding="none" className="min-h-0 flex-1">
        <div
          data-testid="latest-activities-content"
          className={cn('h-full', scrollable ? 'overflow-y-auto' : 'overflow-hidden')}
        >
          {showSkeleton
            ? Array.from({ length: 8 }).map((_, i) => (
                <div
                  key={i}
                  className="border-base-border/50 animate-pulse border-b px-3 py-2 last:border-b-0"
                >
                  <div className="flex items-center justify-between">
                    <div className="bg-base-elevated h-4 w-28 rounded" />
                    <div className="bg-base-elevated h-3 w-14 rounded" />
                  </div>
                  <div className="mt-1.5 flex items-center justify-between">
                    <div className="bg-base-elevated h-3 w-24 rounded" />
                    <div className="bg-base-elevated h-3 w-20 rounded" />
                  </div>
                </div>
              ))
            : classifiedItems.map((classified) => {
                const key = itemKey(classified.activity);
                const isNew = newItemKeys.has(key);
                const txHref = `/tx/${classified.activity.txHash}`;
                const blockHref = `/blocks/${classified.activity.blockNumber}`;

                return (
                  <div
                    key={key}
                    className={cn(
                      'border-base-border/50 border-b px-3 py-2 last:border-b-0',
                      'hover:bg-base-elevated/50 transition-all duration-300'
                    )}
                    style={
                      isNew
                        ? {
                            animation:
                              'stream-slide-in 300ms ease-out, stream-glow-fade 2s ease-out forwards',
                          }
                        : undefined
                    }
                  >
                    <div className="space-y-2">
                      <StreamItem classified={classified} />
                      <div className="flex flex-wrap items-center justify-end gap-x-3 gap-y-1 font-mono text-[10px]">
                        <Link
                          href={txHref}
                          className="text-text-dim hover:text-aqua transition-colors"
                        >
                          tx {truncateHash(classified.activity.txHash, 8, 6)}
                        </Link>
                        <Link
                          href={blockHref}
                          className="text-text-dim hover:text-text transition-colors"
                        >
                          #{classified.activity.blockNumber.toLocaleString()}
                        </Link>
                      </div>
                    </div>
                  </div>
                );
              })}
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
