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
      className: 'bg-lavender/10 text-lavender border border-lavender-dim/50',
    };
  }
  const delta = BigInt(activity.ckbDelta);
  if (delta > BigInt(0)) {
    return {
      label: 'Received',
      className: 'bg-positive/10 text-positive border border-positive/30',
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
    className: 'bg-base-elevated text-text-dim border border-base-border/50',
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
        <span className="border-base-border/60 bg-base-elevated/80 text-text rounded border px-1.5 py-0.5 text-[10px]">
          {change.standard === 'm-nft' ? 'M-NFT' : 'Spore'} {change.action}
        </span>
      );
    case 'identity':
      return (
        <span className="border-base-border/60 bg-base-elevated/80 text-text rounded border px-1.5 py-0.5 text-[10px]">
          {change.standard === 'did_ckb' ? 'did:ckb' : '.bit'} {change.action}
        </span>
      );
    case 'daoDeposit':
      return (
        <span className="border-base-border/60 bg-base-elevated/80 text-text rounded border px-1.5 py-0.5 text-[10px]">
          DAO Deposit {formatCkbAmount(change.capacity).integer} CKB
        </span>
      );
    case 'daoWithdrawRequest':
      return (
        <span className="border-gold-dim/50 bg-gold/10 text-gold rounded border px-1.5 py-0.5 text-[10px]">
          DAO Withdraw Request
        </span>
      );
    case 'daoWithdrawComplete':
      return (
        <span className="text-positive border-positive/30 bg-positive/10 rounded border px-1.5 py-0.5 text-[10px]">
          DAO Withdraw +{formatCkbAmount(change.compensation).integer} CKB
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
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
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
              const usedDelta = BigInt(activity.usedDelta);
              const isCkbAddress =
                activity.address.startsWith('ckb1') || activity.address.startsWith('ckt1');
              const hasFinancial = delta !== BigInt(0) || usedDelta !== BigInt(0);

              return (
                <TerminalRow
                  key={activityKey}
                  className={cn(
                    'transition-all duration-500',
                    newActivityKey === activityKey && 'bg-jade/10 shadow-glow-jade'
                  )}
                >
                  {/* Line 1: Address + Peers | Badge + Time */}
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      <Link
                        href={`/address/${activity.address}`}
                        className="text-text font-mono text-sm transition-opacity hover:opacity-80"
                      >
                        {isCkbAddress ? (
                          truncateAddress(activity.address)
                        ) : (
                          <HexDisplay
                            value={activity.address}
                            truncate
                            startChars={8}
                            endChars={6}
                            size="sm"
                            showGroupHighlight={false}
                          />
                        )}
                      </Link>
                      {activity.peers.length > 0 && (
                        <div className="mt-0.5 flex items-center gap-1 font-mono text-[10px]">
                          <span className="text-text-dim">
                            {delta < BigInt(0) ? '→' : delta > BigInt(0) ? '←' : '↔'}
                          </span>
                          {activity.peers.slice(0, 3).map((peer, idx) => (
                            <span key={peer} className="flex items-center">
                              {idx > 0 && <span className="text-text-dim mr-1">,</span>}
                              <Link
                                href={`/address/${peer}`}
                                className="text-text-dim hover:text-text transition-colors"
                              >
                                {truncateAddress(peer)}
                              </Link>
                            </span>
                          ))}
                          {activity.peers.length > 3 && (
                            <span className="text-text-dim">+{activity.peers.length - 3}</span>
                          )}
                        </div>
                      )}
                    </div>
                    <div className="flex shrink-0 flex-col items-end gap-0.5">
                      <span
                        className={cn(
                          'rounded px-1.5 py-0.5 font-mono text-[10px]',
                          badge.className
                        )}
                      >
                        {badge.label}
                      </span>
                      <span className="text-text-dim text-[10px]">
                        {formatTimeAgo(activity.timestamp)}
                      </span>
                    </div>
                  </div>

                  {/* Line 2: CKB delta + Used delta + Asset badges */}
                  {(hasFinancial || activity.assetChanges.length > 0) && (
                    <div className="mt-1 flex flex-wrap items-center gap-1.5">
                      {delta !== BigInt(0) && (
                        <span
                          className={cn(
                            'font-mono text-xs font-bold tabular-nums',
                            delta > BigInt(0) ? 'text-positive' : 'text-negative'
                          )}
                        >
                          {delta > BigInt(0) ? '+' : ''}
                          {formatCkbAmount(activity.ckbDelta).full} CKB
                        </span>
                      )}
                      {usedDelta !== BigInt(0) && (
                        <span className="text-jade/60 font-mono text-[10px] tabular-nums">
                          {usedDelta > BigInt(0) ? '+' : ''}
                          {formatCkbAmount(activity.usedDelta).integer} KB
                        </span>
                      )}
                      {activity.assetChanges.map((change, idx) => (
                        <AssetBadge key={idx} change={change} />
                      ))}
                    </div>
                  )}

                  {/* Line 3: TX + Block (compact metadata) */}
                  <div className="mt-1 flex items-center justify-between gap-2">
                    <Link href={`/tx/${activity.txHash}`} className="group block">
                      <HexDisplay
                        value={activity.txHash}
                        truncate
                        startChars={8}
                        endChars={6}
                        color="aqua"
                        size="sm"
                        showGroupHighlight={false}
                      />
                    </Link>
                    <Link
                      href={`/blocks/${activity.blockNumber}`}
                      className="text-text-dim hover:text-aqua shrink-0 font-mono text-[10px] transition-colors"
                    >
                      #{activity.blockNumber.toLocaleString()}
                    </Link>
                  </div>
                </TerminalRow>
              );
            })}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
