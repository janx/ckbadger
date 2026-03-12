'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api, type ActivityAssetChange } from '@/lib/api';
import {
  buildLatestActivityGroupSummary,
  groupLatestActivitiesByTx,
} from '@/lib/latest-activity-groups';
import { formatTimeAgo, cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { HexDisplay } from '@/components/ui/hex-display';
import { formatCkbAmount } from '@/lib/utils';

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
    queryFn: () => api.getLatestActivities(16),
    refetchInterval: 10000,
  });

  const itemCount = activities?.length ?? 0;
  const showSkeleton = isLoading || (itemCount === 0 && isFetching);
  const groups = activities ? groupLatestActivitiesByTx(activities).slice(0, 4) : [];

  useEffect(() => {
    if (groups.length > 0) {
      const currentKeys = groups.map((group) => group.txHash);
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
  }, [groups]);

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
          : groups.map((group) => {
              const visibleParticipants = group.participants.slice(0, 3);
              const hiddenParticipantCount = Math.max(
                group.participantCount - visibleParticipants.length,
                0
              );
              const summary = buildLatestActivityGroupSummary(group);
              return (
                <TerminalRow
                  key={group.txHash}
                  className={cn(
                    'transition-all duration-500',
                    newActivityKey === group.txHash && 'bg-jade/10 shadow-glow-jade'
                  )}
                >
                  <div className="space-y-2">
                    <div className="flex items-start justify-between gap-2">
                      <Link href={`/tx/${group.txHash}`} className="group min-w-0 flex-1">
                        <HexDisplay
                          value={group.txHash}
                          truncate
                          startChars={8}
                          endChars={6}
                          color="aqua"
                          size="sm"
                          showGroupHighlight={false}
                        />
                      </Link>
                      <div className="flex shrink-0 flex-col items-end gap-0.5">
                        <Link
                          href={`/blocks/${group.blockNumber}`}
                          className="text-text-dim hover:text-aqua font-mono text-[10px] transition-colors"
                        >
                          #{group.blockNumber.toLocaleString()}
                        </Link>
                        <span className="text-text-dim font-mono text-[10px]">
                          {formatTimeAgo(group.timestamp)}
                        </span>
                      </div>
                    </div>
                    <div className="text-text font-mono text-[11px] leading-tight">{summary}</div>
                    <div className="space-y-1.5">
                      {visibleParticipants.map((participant) => {
                        const delta = BigInt(participant.ckbDelta);
                        const usedDelta = BigInt(participant.usedDelta);
                        const isCkbAddress =
                          participant.address.startsWith('ckb1') ||
                          participant.address.startsWith('ckt1');
                        const hiddenAssetCount = Math.max(participant.assetChanges.length - 2, 0);

                        return (
                          <div
                            key={`${group.txHash}:${participant.address}`}
                            className="border-base-border/40 bg-base-elevated/30 rounded border px-2 py-1.5"
                          >
                            <div className="flex items-start justify-between gap-2">
                              <div className="min-w-0 flex-1">
                                <Link
                                  href={`/address/${participant.address}`}
                                  className="text-text font-mono text-xs transition-opacity hover:opacity-80"
                                >
                                  {isCkbAddress ? (
                                    truncateAddress(participant.address)
                                  ) : (
                                    <HexDisplay
                                      value={participant.address}
                                      truncate
                                      startChars={8}
                                      endChars={6}
                                      size="sm"
                                      showGroupHighlight={false}
                                    />
                                  )}
                                </Link>
                                {participant.assetChanges.length > 0 && (
                                  <div className="mt-1 flex flex-wrap items-center gap-1">
                                    {participant.assetChanges.slice(0, 2).map((change, idx) => (
                                      <AssetBadge key={idx} change={change} />
                                    ))}
                                    {hiddenAssetCount > 0 && (
                                      <span className="text-text-dim font-mono text-[10px]">
                                        +{hiddenAssetCount} assets
                                      </span>
                                    )}
                                  </div>
                                )}
                              </div>
                              <div className="flex shrink-0 flex-col items-end gap-0.5 text-right">
                                <span
                                  className={cn(
                                    'font-mono text-xs tabular-nums',
                                    delta > BigInt(0) && 'text-positive',
                                    delta < BigInt(0) && 'text-negative',
                                    delta === BigInt(0) && 'text-text-dim'
                                  )}
                                >
                                  {delta > BigInt(0) ? '+' : ''}
                                  {formatCkbAmount(participant.ckbDelta).full} CKB
                                </span>
                                {usedDelta !== BigInt(0) && (
                                  <span className="text-jade/60 font-mono text-[10px] tabular-nums">
                                    {usedDelta > BigInt(0) ? '+' : ''}
                                    {formatCkbAmount(participant.usedDelta).integer} KB
                                  </span>
                                )}
                              </div>
                            </div>
                          </div>
                        );
                      })}
                      {hiddenParticipantCount > 0 && (
                        <div className="text-text-dim font-mono text-[10px]">
                          +{hiddenParticipantCount} more
                        </div>
                      )}
                    </div>
                  </div>
                </TerminalRow>
              );
            })}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
