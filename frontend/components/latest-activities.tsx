'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api, type GlobalActivity } from '@/lib/api';
import { classifyActivity, type ClassifiedActivity } from '@/lib/activity-classify';
import {
  getScriptDetailHref,
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

let keyframesInjected = false;
function ensureKeyframes() {
  if (keyframesInjected || typeof document === 'undefined') return;
  const style = document.createElement('style');
  style.textContent = STREAM_KEYFRAMES;
  document.head.appendChild(style);
  keyframesInjected = true;
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

function formatStandard(standard: string): string {
  if (standard === 'spore') return 'Spore';
  if (standard === 'm-nft') return 'M-NFT';
  if (standard === 'dotbit') return '.bit';
  if (standard === 'did_ckb') return 'did:ckb';
  return standard.charAt(0).toUpperCase() + standard.slice(1);
}

function capitalizeAction(action: string): string {
  if (!action) return '';
  return action.charAt(0).toUpperCase() + action.slice(1);
}

function itemKey(activity: GlobalActivity): string {
  return `${activity.txHash}:${activity.address}`;
}

interface TypeBadgeInfo {
  icon: string;
  label: string;
  colorClass: string;
}

function getTypeBadge(classified: ClassifiedActivity): TypeBadgeInfo {
  const { type, primaryAssetChange, primaryScriptCall } = classified;

  switch (type) {
    case 'daoDeposit':
      return { icon: '\u25C6', label: 'DAO Deposit', colorClass: 'text-gold' };
    case 'daoWithdrawRequest':
      return { icon: '\u25C6', label: 'DAO Withdraw Request', colorClass: 'text-gold' };
    case 'daoWithdrawComplete':
      return { icon: '\u25C6', label: 'DAO Withdraw Complete', colorClass: 'text-positive' };
    case 'token': {
      const change = primaryAssetChange;
      if (change && change.type === 'token') {
        const label = change.symbol ?? truncateHash(change.typeScriptHash, 8, 6);
        return { icon: '\u25CE', label: `${label} Transfer`, colorClass: 'text-[#ff66aa]' };
      }
      return { icon: '\u25CE', label: 'Token Transfer', colorClass: 'text-[#ff66aa]' };
    }
    case 'object': {
      const change = primaryAssetChange;
      if (change && change.type === 'object') {
        const std = formatStandard(change.standard);
        const action = capitalizeAction(change.action);
        return { icon: '\u2B21', label: `${std} ${action}`, colorClass: 'text-lavender' };
      }
      return { icon: '\u2B21', label: 'Object', colorClass: 'text-lavender' };
    }
    case 'identity': {
      const change = primaryAssetChange;
      if (change && change.type === 'identity') {
        const std = formatStandard(change.standard);
        const action = capitalizeAction(change.action);
        return { icon: '\u2726', label: `${std} ${action}`, colorClass: 'text-aqua' };
      }
      return { icon: '\u2726', label: 'Identity', colorClass: 'text-aqua' };
    }
    case 'scriptCall': {
      const sc = primaryScriptCall;
      if (sc) {
        const name = sc.scriptName?.trim() || truncateHash(sc.typeCodeHash, 8, 6);
        return { icon: '\u2699', label: `Script: ${name}`, colorClass: 'text-amber' };
      }
      return { icon: '\u2699', label: 'Script Call', colorClass: 'text-amber' };
    }
    case 'ckbTransfer':
      return { icon: '\u2197', label: 'CKB Transfer', colorClass: 'text-jade' };
    default:
      return { icon: '\u2197', label: 'Transfer', colorClass: 'text-jade' };
  }
}

function CkbDelta({ delta }: { delta: string }) {
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
        <AddressLink address={activity.address} />
        <CkbDelta delta={activity.ckbDelta} />
      </div>
    </>
  );
}

function StreamItemDaoDeposit({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryAssetChange } = classified;
  const badge = getTypeBadge(classified);
  const capacity =
    primaryAssetChange && primaryAssetChange.type === 'daoDeposit'
      ? primaryAssetChange.capacity
      : '0';

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
        <AddressLink address={activity.address} />
        <span className="text-positive font-mono text-xs tabular-nums">
          +{formatCkbAmount(capacity).full} CKB locked
        </span>
      </div>
    </>
  );
}

function StreamItemDaoWithdrawRequest({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryAssetChange } = classified;
  const badge = getTypeBadge(classified);
  const capacity =
    primaryAssetChange && primaryAssetChange.type === 'daoWithdrawRequest'
      ? primaryAssetChange.capacity
      : '0';

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
        <AddressLink address={activity.address} />
        <span className="text-gold font-mono text-xs tabular-nums">
          {formatCkbAmount(capacity).full} CKB
        </span>
      </div>
    </>
  );
}

function StreamItemDaoWithdrawComplete({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryAssetChange } = classified;
  const badge = getTypeBadge(classified);
  const capacity =
    primaryAssetChange && primaryAssetChange.type === 'daoWithdrawComplete'
      ? primaryAssetChange.capacity
      : '0';
  const compensation =
    primaryAssetChange && primaryAssetChange.type === 'daoWithdrawComplete'
      ? primaryAssetChange.compensation
      : '0';

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
        <AddressLink address={activity.address} />
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
  const { activity, primaryAssetChange } = classified;
  const badge = getTypeBadge(classified);

  let tokenDelta = '';
  let tokenSymbol = '';
  let typeScriptHash = '';
  if (primaryAssetChange && primaryAssetChange.type === 'token') {
    const delta = BigInt(primaryAssetChange.delta);
    const sign = delta > BigInt(0) ? '+' : '';
    tokenSymbol = primaryAssetChange.symbol ?? '';
    typeScriptHash = primaryAssetChange.typeScriptHash;
    tokenDelta = `${sign}${primaryAssetChange.delta}${tokenSymbol ? ` ${tokenSymbol}` : ''}`;
  }

  const ckbValue = BigInt(activity.ckbDelta);
  const showCkbDelta = ckbValue !== BigInt(0);

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
        <AddressLink address={activity.address} />
        {typeScriptHash ? (
          <Link
            href={getTokenDetailHref(typeScriptHash)}
            className="font-mono text-xs tabular-nums text-[#ff66aa] transition-colors hover:text-[#ff88bb]"
            onClick={(e) => e.stopPropagation()}
          >
            {tokenDelta}
          </Link>
        ) : (
          <span className="font-mono text-xs tabular-nums text-[#ff66aa]">{tokenDelta}</span>
        )}
      </div>
      {showCkbDelta && (
        <div className="flex justify-end">
          <CkbDelta delta={activity.ckbDelta} />
        </div>
      )}
    </>
  );
}

function StreamItemObject({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryAssetChange } = classified;
  const badge = getTypeBadge(classified);

  let objectId = '';
  if (primaryAssetChange && primaryAssetChange.type === 'object') {
    objectId = primaryAssetChange.objectId;
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
        <AddressLink address={activity.address} />
        {objectId ? (
          <Link
            href={getObjectDetailHref(objectId)}
            className="text-lavender hover:text-lavender-bright font-mono text-xs transition-colors"
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
  const { activity, primaryAssetChange } = classified;
  const badge = getTypeBadge(classified);

  let identityId = '';
  let standard = '';
  if (primaryAssetChange && primaryAssetChange.type === 'identity') {
    identityId = primaryAssetChange.identityId;
    standard = primaryAssetChange.standard;
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
        <AddressLink address={activity.address} />
        {identityId ? (
          <Link
            href={getIdentityItemDetailHref(standard, identityId)}
            className="text-aqua hover:text-aqua-bright font-mono text-xs transition-colors"
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

function StreamItemScriptCall({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryScriptCall } = classified;
  const badge = getTypeBadge(classified);

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('font-mono text-xs', badge.colorClass)}>
          {badge.icon}{' '}
          {primaryScriptCall ? (
            <Link
              href={getScriptDetailHref({
                name: primaryScriptCall.scriptName,
                codeHash: primaryScriptCall.typeCodeHash,
                hashType: primaryScriptCall.typeHashType,
                scriptKind: 'type',
              })}
              className="text-amber hover:text-amber-bright transition-colors"
              onClick={(e) => e.stopPropagation()}
            >
              {badge.label}
            </Link>
          ) : (
            badge.label
          )}
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        <AddressLink address={activity.address} />
        <CkbDelta delta={activity.ckbDelta} />
      </div>
      {primaryScriptCall && (
        <div className="flex justify-end">
          <span className="text-text-dim font-mono text-[10px]">
            args {truncateHash(primaryScriptCall.typeArgs, 8, 6)}
          </span>
        </div>
      )}
    </>
  );
}

function StreamItem({ classified }: { classified: ClassifiedActivity }) {
  switch (classified.type) {
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
    case 'scriptCall':
      return <StreamItemScriptCall classified={classified} />;
    default:
      return <StreamItemCkbTransfer classified={classified} />;
  }
}

interface LatestActivitiesProps {
  isRealtime?: boolean;
}

export function LatestActivities({ isRealtime = false }: LatestActivitiesProps) {
  useEffect(() => ensureKeyframes(), []);
  const [newItemKeys, setNewItemKeys] = useState<Set<string>>(new Set());
  const prevKeysRef = useRef<Set<string>>(new Set());

  const {
    data: activities,
    isLoading,
    isFetching,
  } = useQuery({
    queryKey: ['latest-activities'],
    queryFn: () => api.getLatestActivities(32),
    refetchInterval: 10000,
  });

  const itemCount = activities?.length ?? 0;
  const showSkeleton = isLoading || (itemCount === 0 && isFetching);

  const classifiedItems = useMemo<ClassifiedActivity[]>(
    () => (activities ? activities.slice(0, MAX_STREAM_ITEMS).map((a) => classifyActivity(a)) : []),
    [activities]
  );

  useEffect(() => {
    if (classifiedItems.length > 0) {
      const currentKeys = new Set(classifiedItems.map((c) => itemKey(c.activity)));
      const prevKeys = prevKeysRef.current;

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

      prevKeysRef.current = currentKeys;
    }
  }, [classifiedItems]);

  const headerActions = (
    <Link
      href="/activities"
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
    >
      VIEW ALL &rarr;
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime} className="flex h-[44rem] flex-col">
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        Latest Activities
      </TerminalPanelHeader>
      <TerminalPanelContent padding="none" className="min-h-0 flex-1">
        <div data-testid="latest-activities-content" className="h-full overflow-hidden">
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

                return (
                  <Link
                    key={key}
                    href={`/tx/${classified.activity.txHash}`}
                    className={cn(
                      'border-base-border/50 block border-b px-3 py-2 no-underline last:border-b-0',
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
                    <div className="space-y-1">
                      <StreamItem classified={classified} />
                    </div>
                  </Link>
                );
              })}
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
