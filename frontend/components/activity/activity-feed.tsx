'use client';

import { cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { ActivityItem } from './activity-item';
import type { Activity } from '@/types/activity';

interface ActivityFeedProps {
  activities: Activity[];
  isLoading?: boolean;
  hasMore?: boolean;
  onLoadMore?: () => void;
  loadingMore?: boolean;
  title?: string;
  emptyMessage?: string;
  className?: string;
  highlightedActivityId?: string;
  showHeader?: boolean;
  headerActions?: React.ReactNode;
}

function LoadingSkeleton() {
  return (
    <TerminalRow hoverable={false}>
      <div className="flex animate-pulse items-center justify-between gap-4">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <div className="flex items-center gap-2">
            <div className="h-4 w-4 rounded bg-slate-800" />
            <div className="h-4 w-12 rounded bg-slate-800" />
          </div>
          <div className="flex-1 space-y-2">
            <div className="h-4 w-32 rounded bg-slate-800" />
            <div className="h-3 w-48 rounded bg-slate-800" />
          </div>
        </div>
        <div className="space-y-2 text-right">
          <div className="h-3 w-20 rounded bg-slate-800" />
          <div className="h-3 w-12 rounded bg-slate-800" />
        </div>
      </div>
    </TerminalRow>
  );
}

function EmptyState({ message }: { message: string }) {
  return (
    <div className="flex flex-col items-center justify-center py-12 text-center">
      <div className="font-mono text-sm text-slate-500">{message}</div>
    </div>
  );
}

export function ActivityFeed({
  activities,
  isLoading = false,
  hasMore = false,
  onLoadMore,
  loadingMore = false,
  title = 'Activity',
  emptyMessage = 'No activities found',
  className,
  highlightedActivityId,
  showHeader = true,
  headerActions,
}: ActivityFeedProps) {
  const showSkeleton = isLoading && activities.length === 0;
  const showEmpty = !isLoading && activities.length === 0;

  return (
    <TerminalPanel variant="default" className={className}>
      {showHeader && (
        <TerminalPanelHeader indicator="inactive" actions={headerActions}>
          {title}
        </TerminalPanelHeader>
      )}

      <TerminalPanelContent padding="none">
        {showSkeleton ? (
          Array.from({ length: 5 }).map((_, i) => <LoadingSkeleton key={i} />)
        ) : showEmpty ? (
          <EmptyState message={emptyMessage} />
        ) : (
          activities.map((activity) => (
            <ActivityItem
              key={activity.activityId}
              activity={activity}
              highlighted={activity.activityId === highlightedActivityId}
            />
          ))
        )}
      </TerminalPanelContent>

      {hasMore && onLoadMore && (
        <TerminalPanelFooter>
          <button
            onClick={onLoadMore}
            disabled={loadingMore}
            className={cn(
              'w-full py-2 font-mono text-xs uppercase tracking-wider',
              'text-slate-500 transition-colors hover:text-amber-400',
              'disabled:cursor-not-allowed disabled:opacity-50'
            )}
          >
            {loadingMore ? 'Loading...' : 'Load More'}
          </button>
        </TerminalPanelFooter>
      )}
    </TerminalPanel>
  );
}
