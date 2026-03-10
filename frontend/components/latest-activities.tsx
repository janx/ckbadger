'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api, type GlobalActivity, type ActivityAssetChange } from '@/lib/api';
import { formatTimeAgo, cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { HexDisplay } from '@/components/ui/hex-display';
import { formatCkbAmount } from '@/lib/utils';

function getTypeBadge(activity: GlobalActivity): { label: string; className: string } {
  if (activity.isCellbase) {
    return {
      label: 'Coinbase',
      className: 'bg-purple-900/50 text-purple-300 border border-purple-700/50',
    };
  }
  const delta = BigInt(activity.ckbDelta);
  if (delta > BigInt(0)) {
    return {
      label: 'Received',
      className: 'bg-emerald-900/50 text-positive border border-emerald-700/50',
    };
  }
  if (delta < BigInt(0)) {
    return {
      label: 'Sent',
      className: 'bg-negative/10 text-negative border border-negative/30',
    };
  }
  return {
    label: 'Self',
    className: 'bg-base-elevated text-text-muted border border-base-border/50',
  };
}

function AssetBadge({ change }: { change: ActivityAssetChange }) {
  switch (change.type) {
    case 'token': {
      const delta = BigInt(change.delta);
      const sign = delta > BigInt(0) ? '+' : '';
      const color = delta > BigInt(0) ? 'text-positive' : 'text-negative';
      const label = change.symbol ?? `${change.typeScriptHash.slice(0, 10)}...`;
      return (
        <span
          className={cn(
            'border-base-border/60 bg-base-elevated/80 rounded border px-1.5 py-0.5 font-mono text-[10px]',
            color
          )}
        >
          {label} {sign}
          {change.delta}
        </span>
      );
    }
    case 'object':
      return (
        <span className="border-base-border/60 bg-base-elevated/80 text-text-secondary rounded border px-1.5 py-0.5 text-[10px]">
          {change.standard === 'm-nft' ? 'M-NFT' : 'Spore'} {change.action}
        </span>
      );
    case 'identity':
      return (
        <span className="border-base-border/60 bg-base-elevated/80 text-text-secondary rounded border px-1.5 py-0.5 text-[10px]">
          {change.standard === 'did_ckb' ? 'did:ckb' : '.bit'} {change.action}
        </span>
      );
    case 'daoDeposit':
      return (
        <span className="border-base-border/60 bg-base-elevated/80 text-text-secondary rounded border px-1.5 py-0.5 text-[10px]">
          DAO Deposit
        </span>
      );
    case 'daoWithdrawRequest':
      return (
        <span className="border-warning-700/50 bg-warning-900/30 text-warning-300 rounded border px-1.5 py-0.5 text-[10px]">
          DAO Withdraw Request
        </span>
      );
    case 'daoWithdrawComplete':
      return (
        <span className="text-positive rounded border border-emerald-700/50 bg-emerald-900/30 px-1.5 py-0.5 text-[10px]">
          DAO Withdraw Complete
        </span>
      );
    default:
      return null;
  }
}

function truncateAddress(addr: string): string {
  return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
}

interface LatestActivitiesProps {
  isRealtime?: boolean;
}

export function LatestActivities({ isRealtime = false }: LatestActivitiesProps) {
  const [newActivityKey, setNewActivityKey] = useState<string | null>(null);
  const prevKeysRef = useRef<string[]>([]);

  const {
    data: activities,
    isLoading,
    isFetching,
  } = useQuery({
    queryKey: ['latest-activities'],
    queryFn: () => api.getLatestActivities(6),
    refetchInterval: 10000,
  });

  const itemCount = activities?.length ?? 0;
  const showSkeleton = isLoading || (itemCount === 0 && isFetching);

  useEffect(() => {
    if (activities) {
      const currentKeys = activities.map((a) => `${a.blockNumber}:${a.txIndex}:${a.address}`);
      const prevKeys = prevKeysRef.current;

      if (prevKeys.length > 0) {
        const newKey = currentKeys.find((k) => !prevKeys.includes(k));
        if (newKey) {
          setNewActivityKey(newKey);
          setTimeout(() => setNewActivityKey(null), 2000);
        }
      }

      prevKeysRef.current = currentKeys;
    }
  }, [activities]);

  const headerActions = (
    <Link
      href="/activities"
      className="text-text-muted hover:text-emphasis font-mono text-xs transition-colors"
    >
      VIEW ALL →
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        Latest Activities
      </TerminalPanelHeader>
      <TerminalPanelContent padding="none">
        {showSkeleton
          ? Array.from({ length: 4 }).map((_, i) => (
              <TerminalRow key={i} hoverable={false}>
                <div className="animate-pulse space-y-2">
                  <div className="flex items-center justify-between">
                    <div className="bg-base-elevated h-4 w-28 rounded" />
                    <div className="flex items-center gap-2">
                      <div className="bg-base-elevated h-4 w-16 rounded" />
                      <div className="bg-base-elevated h-3 w-14 rounded" />
                    </div>
                  </div>
                  <div className="flex items-center justify-between">
                    <div className="bg-base-elevated h-3 w-24 rounded" />
                    <div className="bg-base-elevated h-3 w-20 rounded" />
                  </div>
                </div>
              </TerminalRow>
            ))
          : activities?.slice(0, 6).map((activity) => {
              const activityKey = `${activity.blockNumber}:${activity.txIndex}:${activity.address}`;
              const badge = getTypeBadge(activity);
              const delta = BigInt(activity.ckbDelta);
              const isCkbAddress =
                activity.address.startsWith('ckb1') || activity.address.startsWith('ckt1');

              return (
                <TerminalRow
                  key={activityKey}
                  className={cn(
                    'transition-all duration-500',
                    newActivityKey === activityKey &&
                      'bg-cyan-500/10 shadow-[0_0_8px_rgba(6,182,212,0.15)]'
                  )}
                >
                  {/* Row 1: Address | Type badge | Time */}
                  <div className="flex items-center justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      <Link
                        href={`/address/${activity.address}`}
                        className="text-text-secondary font-mono text-sm transition-opacity hover:opacity-80"
                      >
                        {isCkbAddress ? (
                          truncateAddress(activity.address)
                        ) : (
                          <HexDisplay
                            value={activity.address}
                            truncate
                            startChars={8}
                            endChars={6}
                            color="white"
                            size="sm"
                            showGroupHighlight={false}
                          />
                        )}
                      </Link>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                      <span
                        className={cn(
                          'rounded px-1.5 py-0.5 font-mono text-[10px]',
                          badge.className
                        )}
                      >
                        {badge.label}
                      </span>
                      <span className="text-text-muted text-xs">
                        {formatTimeAgo(activity.timestamp)}
                      </span>
                    </div>
                  </div>

                  {/* Row 2: Tx hash | Block number */}
                  <div className="mt-1.5 flex items-center justify-between gap-2">
                    <Link href={`/tx/${activity.txHash}`} className="group block">
                      <HexDisplay
                        value={activity.txHash}
                        truncate
                        startChars={8}
                        endChars={6}
                        color="amber"
                        size="sm"
                        showGroupHighlight={false}
                      />
                    </Link>
                    <Link
                      href={`/blocks/${activity.blockNumber}`}
                      className="hover:text-emphasis text-text-muted shrink-0 font-mono text-xs transition-colors"
                    >
                      Block{' '}
                      <span className="text-emphasis">
                        #{activity.blockNumber.toLocaleString()}
                      </span>
                    </Link>
                  </div>

                  {/* Row 3: CKB delta (only if non-zero) */}
                  {delta !== BigInt(0) && (
                    <div className="mt-1">
                      <span
                        className={cn(
                          'font-mono text-xs',
                          delta > BigInt(0) ? 'text-positive' : 'text-negative'
                        )}
                      >
                        {delta > BigInt(0) ? '+' : ''}
                        {formatCkbAmount(activity.ckbDelta).full} CKB
                      </span>
                    </div>
                  )}

                  {/* Row 4: Asset change badges */}
                  {activity.assetChanges.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap gap-1">
                      {activity.assetChanges.map((change, idx) => (
                        <AssetBadge key={idx} change={change} />
                      ))}
                    </div>
                  )}
                </TerminalRow>
              );
            })}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
