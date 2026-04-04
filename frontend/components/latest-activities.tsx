'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api, type GlobalActivity } from '@/lib/api';
import { formatTimeAgo, truncateHash, cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { buildGlobalTxEvents, ParticipantLine } from '@/components/activity-event-row';

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

/** Get a unique key for a global activity. TX-level now, so txHash is sufficient. */
function itemKey(activity: GlobalActivity): string {
  return activity.txHash;
}

/** Max participants to show inline on homepage before collapsing. */
const MAX_VISIBLE_PARTICIPANTS = 3;

function StreamItem({ activity }: { activity: GlobalActivity }) {
  const txEvents = buildGlobalTxEvents(activity);
  const participants = activity.participants;
  const visibleParticipants = participants.slice(0, MAX_VISIBLE_PARTICIPANTS);
  const hiddenCount = participants.length - visibleParticipants.length;

  return (
    <>
      {/* TX header: hash · block     time */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 font-mono text-xs">
          <Link
            href={`/tx/${activity.txHash}`}
            className="text-text-dim hover:text-aqua transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            <span className="text-text-dim/60 mr-0.5">tx</span>
            {truncateHash(activity.txHash, 8, 6)}
          </Link>
          <span className="text-text-dim">{'\u00B7'}</span>
          <Link
            href={`/blocks/${activity.blockNumber}`}
            className="text-text-dim hover:text-text transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            #{activity.blockNumber.toLocaleString()}
          </Link>
        </div>
        <span className="text-text-dim shrink-0 font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>

      {/* Event rows: L3 protocol actions + L2 catch-all type/lock calls */}
      {txEvents.length > 0 && (
        <div className="mt-1 space-y-0.5 pl-2">
          {txEvents.map((event, i) => (
            <div key={i} className="flex items-center justify-between gap-2">
              <div className="min-w-0 truncate">{event.badge}</div>
              <div className="shrink-0">{event.value}</div>
            </div>
          ))}
        </div>
      )}

      {/* Per-participant lines: L1 CKB + L2 item deltas */}
      <div className={cn('space-y-0.5', txEvents.length > 0 ? 'mt-0.5 pl-2' : 'mt-1 pl-2')}>
        {visibleParticipants.map((p) => (
          <ParticipantLine key={p.address} participant={p} />
        ))}
        {hiddenCount > 0 && (
          <span className="text-text-dim font-mono text-[10px]">
            +{hiddenCount} more participant{hiddenCount > 1 ? 's' : ''}
          </span>
        )}
      </div>
    </>
  );
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

  const visibleItems = useMemo<GlobalActivity[]>(
    () => (activities ? activities.slice(0, maxItems) : []),
    [activities, maxItems]
  );

  useEffect(() => {
    if (visibleItems.length > 0) {
      const currentKeys = new Set(visibleItems.map((a) => itemKey(a)));
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
  }, [visibleItems]);

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
            : visibleItems.map((activity) => {
                const key = itemKey(activity);
                const isNew = newItemKeys.has(key);

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
                    <StreamItem activity={activity} />
                  </div>
                );
              })}
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
