'use client';

import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';
import { ActivityRow } from '@/components/activity/activity-row';
import { ClassMark } from '@/components/ui/class-mark';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { totalActivities } from '@/lib/activity-stats';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { startTransition, useEffect, useMemo, useRef, useState } from 'react';
import {
  api,
  type DailyActivityStats,
  type GlobalActivity,
  type GlobalActivityFilter,
} from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { cn } from '@/lib/utils';

const PAGE_SIZE = DEFAULT_PAGE_SIZE;
const POLL_INTERVAL_MS = 10_000;
const NEAR_TOP_THRESHOLD_PX = 120;
const ROW_HIGHLIGHT_DURATION_MS = 2_000;

const FILTER_OPTIONS: Array<{ label: string; value: GlobalActivityFilter }> = [
  { label: 'All', value: 'all' },
  { label: 'CKB', value: 'ckb' },
  { label: 'Token', value: 'token' },
  { label: 'Object', value: 'object' },
  { label: 'Identity', value: 'identity' },
  { label: 'DAO', value: 'dao' },
  { label: 'Script', value: 'script' },
  { label: 'Protocol', value: 'protocol' },
];

const STREAM_KEYFRAMES = `
@keyframes activity-stream-row-enter {
  from { transform: translateY(-6px); opacity: 0; }
  to { transform: translateY(0); opacity: 1; }
}
@keyframes activity-stream-row-glow {
  from { background-color: rgba(46, 219, 163, 0.10); }
  to { background-color: transparent; }
}
`;

function ensureKeyframes() {
  if (typeof document === 'undefined') return;
  if (document.getElementById('activity-stream-explorer-keyframes')) return;
  const style = document.createElement('style');
  style.id = 'activity-stream-explorer-keyframes';
  style.textContent = STREAM_KEYFRAMES;
  document.head.appendChild(style);
}

function safeWindowScrollTo(options: ScrollToOptions) {
  if (
    typeof window === 'undefined' ||
    typeof window.scrollTo !== 'function' ||
    /jsdom/i.test(window.navigator.userAgent)
  ) {
    return;
  }

  try {
    window.scrollTo(options);
  } catch {
    // jsdom does not implement scrolling.
  }
}

function safeScrollIntoView(element: HTMLDivElement | null, options?: ScrollIntoViewOptions) {
  if (
    !element ||
    typeof element.scrollIntoView !== 'function' ||
    /jsdom/i.test(navigator.userAgent)
  ) {
    return;
  }

  try {
    element.scrollIntoView(options);
  } catch {
    // jsdom does not implement scrolling.
  }
}

function itemKey(activity: GlobalActivity): string {
  return activity.txHash;
}

function toActivityDate(timestamp: string): Date {
  const numeric = Number(timestamp);
  if (Number.isNaN(numeric)) {
    return new Date(timestamp);
  }
  return new Date(numeric < 1_000_000_000_000 ? numeric * 1000 : numeric);
}

function getActivityDayLabel(timestamp: string): string {
  const target = toActivityDate(timestamp);
  const targetKey = `${target.getFullYear()}-${target.getMonth()}-${target.getDate()}`;
  const now = new Date();
  const todayKey = `${now.getFullYear()}-${now.getMonth()}-${now.getDate()}`;

  if (targetKey === todayKey) {
    return 'Today';
  }

  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const yesterdayKey = `${yesterday.getFullYear()}-${yesterday.getMonth()}-${yesterday.getDate()}`;
  if (targetKey === yesterdayKey) {
    return 'Yesterday';
  }

  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
  }).format(target);
}

function mergeUniqueActivities(
  incoming: GlobalActivity[],
  existing: GlobalActivity[]
): GlobalActivity[] {
  const merged: GlobalActivity[] = [];
  const seen = new Set<string>();

  for (const activity of [...incoming, ...existing]) {
    const key = itemKey(activity);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    merged.push(activity);
  }

  return merged;
}

async function fetchFreshHeadActivities(
  filter: GlobalActivityFilter,
  knownKeys: Set<string>
): Promise<GlobalActivity[]> {
  let cursor: string | undefined;
  const fresh: GlobalActivity[] = [];

  while (true) {
    const page = await api.getGlobalActivities({
      limit: PAGE_SIZE,
      cursor,
      filter,
    });
    if (page.data.length === 0) {
      return fresh;
    }

    let reachedKnownItem = false;
    for (const activity of page.data) {
      if (knownKeys.has(itemKey(activity))) {
        reachedKnownItem = true;
        break;
      }
      fresh.push(activity);
    }

    if (reachedKnownItem || !page.hasMore) {
      return fresh;
    }

    if (!page.nextCursor) {
      throw new Error('global activities head refresh returned hasMore without nextCursor');
    }

    cursor = page.nextCursor;
  }
}

function getDayDividerTestId(label: string): string {
  return `activity-day-divider-${label.toLowerCase().replace(/\s+/g, '-')}`;
}

function activityFilterCounts(
  summary: DailyActivityStats
): Partial<Record<GlobalActivityFilter, number>> {
  return {
    all: totalActivities(summary),
    ckb: summary.transferCount,
    token: summary.tokenCount,
    object: summary.objectCount,
    identity: summary.identityCount,
    dao:
      summary.daoDepositCount + summary.daoWithdrawRequestCount + summary.daoWithdrawCompleteCount,
    script: summary.scriptCallCount,
  };
}
function ActivityStreamToolbar({
  selectedFilter,
  visibleItemsCount,
  statusLabel,
  onFilterChange,
  summary,
}: {
  selectedFilter: GlobalActivityFilter;
  visibleItemsCount: number;
  statusLabel: 'active' | 'inactive';
  onFilterChange: (value: string | number | undefined) => void;
  summary?: DailyActivityStats;
}) {
  const counts = summary ? activityFilterCounts(summary) : null;
  const classes = {
    token: 'tokens',
    object: 'objects',
    identity: 'identities',
    dao: 'dao',
  } as const;
  return (
    <div
      data-testid="activities-stream-toolbar"
      className="bg-base-bg border-base-border-subtle border-b py-3"
    >
      <div role="group" aria-label="Filter by kind" className="flex flex-wrap items-center gap-2">
        {FILTER_OPTIONS.map((option) => (
          <button
            type="button"
            key={option.value}
            aria-label={option.label}
            aria-pressed={selectedFilter === option.value}
            onClick={() => onFilterChange(option.value)}
            className={cn(
              'flex items-center gap-2 rounded-md border px-3 py-2 text-xs transition-colors',
              selectedFilter === option.value
                ? 'border-base-border text-text-bright bg-base-surface'
                : 'text-text-dim hover:text-text border-transparent'
            )}
          >
            {option.value in classes && (
              <ClassMark kind={classes[option.value as keyof typeof classes]} />
            )}
            {option.label}
            {counts && counts[option.value] !== undefined && (
              <span className="text-text-dim text-[11px] tabular-nums">
                {counts[option.value]!.toLocaleString('en-US')}
              </span>
            )}
          </button>
        ))}
        <span className="text-text-dim ml-auto text-[11px]" aria-live="polite">
          {statusLabel === 'active'
            ? 'Refreshing'
            : `${visibleItemsCount.toLocaleString('en-US')} loaded`}
        </span>
      </div>
      {counts && counts.all !== undefined && counts.all > 0 && (
        <div
          role="img"
          aria-label={`Activity kinds on ${summary!.date}`}
          className="mt-3 flex h-2 overflow-hidden rounded-sm"
        >
          {FILTER_OPTIONS.filter(
            (o) => o.value !== 'all' && counts[o.value] !== undefined && counts[o.value]! > 0
          ).map((o) => (
            <span
              key={o.value}
              title={`${o.label}: ${counts[o.value]}`}
              style={{
                width: `${(counts[o.value]! / counts.all!) * 100}%`,
                backgroundColor:
                  o.value in classes
                    ? CLASS_COLORS[classes[o.value as keyof typeof classes]]
                    : o.value === 'ckb'
                      ? '#2edba3'
                      : CLASS_COLORS.other,
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ActivityDayDivider({ label }: { label: string }) {
  return (
    <div
      className="text-text-dim border-base-border-subtle mt-5 border-b py-2 text-[11px]"
      data-testid={getDayDividerTestId(label)}
    >
      {label}
    </div>
  );
}

function LoadingRows() {
  return Array.from({ length: 8 }).map((_, index) => (
    <div
      key={index}
      className="border-base-border/50 border-b px-3 py-3 last:border-b-0"
      data-testid="activities-stream-skeleton"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="bg-base-elevated h-4 w-28 animate-pulse rounded" />
        <div className="bg-base-elevated h-3 w-24 animate-pulse rounded" />
      </div>
      <div className="mt-2 flex items-center justify-between gap-2">
        <div className="bg-base-elevated h-3 w-32 animate-pulse rounded" />
        <div className="bg-base-elevated h-3 w-20 animate-pulse rounded" />
      </div>
    </div>
  ));
}

export function ActivitiesStreamExplorer({ summary }: { summary?: DailyActivityStats }) {
  useEffect(() => ensureKeyframes(), []);

  const [selectedFilter, setSelectedFilter] = useState<GlobalActivityFilter>('all');
  const [prependedItems, setPrependedItems] = useState<GlobalActivity[]>([]);
  const [pendingNewItems, setPendingNewItems] = useState<GlobalActivity[]>([]);
  const [highlightedKeys, setHighlightedKeys] = useState<Set<string>>(new Set());
  const [isNearTop, setIsNearTop] = useState(() => {
    if (typeof window === 'undefined') {
      return true;
    }
    return window.scrollY <= NEAR_TOP_THRESHOLD_PX;
  });
  const topAnchorRef = useRef<HTMLDivElement | null>(null);
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const highlightTimerRef = useRef<number | null>(null);

  const clearHighlightTimer = () => {
    if (highlightTimerRef.current !== null) {
      window.clearTimeout(highlightTimerRef.current);
      highlightTimerRef.current = null;
    }
  };

  const scheduleHighlightReset = () => {
    clearHighlightTimer();
    highlightTimerRef.current = window.setTimeout(() => {
      setHighlightedKeys(new Set());
      highlightTimerRef.current = null;
    }, ROW_HIGHLIGHT_DURATION_MS);
  };

  const {
    data,
    error,
    isLoading,
    hasNextPage,
    fetchNextPage,
    isFetchingNextPage,
    isFetchNextPageError,
  } = useInfiniteQuery({
    queryKey: ['global-activities-stream', selectedFilter],
    queryFn: ({ pageParam }) =>
      api.getGlobalActivities({
        limit: PAGE_SIZE,
        cursor: pageParam,
        filter: selectedFilter,
      }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) =>
      lastPage.hasMore ? (lastPage.nextCursor ?? undefined) : undefined,
  });

  const baseItems = useMemo(() => data?.pages.flatMap((page) => page.data) ?? [], [data?.pages]);

  const knownKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const activity of [...prependedItems, ...baseItems, ...pendingNewItems]) {
      keys.add(itemKey(activity));
    }
    return keys;
  }, [prependedItems, baseItems, pendingNewItems]);

  const visibleItems = useMemo(
    () => mergeUniqueActivities(prependedItems, baseItems),
    [prependedItems, baseItems]
  );

  const headQuery = useQuery({
    queryKey: ['global-activities-stream-head', selectedFilter],
    queryFn: () => fetchFreshHeadActivities(selectedFilter, knownKeys),
    enabled: data !== undefined && !error,
    refetchInterval: POLL_INTERVAL_MS,
  });

  useEffect(() => {
    const onScroll = () => {
      setIsNearTop(window.scrollY <= NEAR_TOP_THRESHOLD_PX);
    };

    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  useEffect(() => () => clearHighlightTimer(), []);

  useEffect(() => {
    setPrependedItems([]);
    setPendingNewItems([]);
    setHighlightedKeys(new Set());
    clearHighlightTimer();
    safeWindowScrollTo({ top: 0 });
  }, [selectedFilter]);

  useEffect(() => {
    if (!headQuery.data) {
      return;
    }

    const fresh = headQuery.data.filter((activity) => !knownKeys.has(itemKey(activity)));
    if (fresh.length === 0) {
      return;
    }

    const freshKeys = new Set(fresh.map(itemKey));
    if (isNearTop) {
      setPrependedItems((current) => mergeUniqueActivities(fresh, current));
      setHighlightedKeys((current) => new Set([...current, ...freshKeys]));
      scheduleHighlightReset();
      return;
    }

    setPendingNewItems((current) => mergeUniqueActivities(fresh, current));
  }, [headQuery.data, isNearTop, knownKeys]);

  useEffect(() => {
    if (!sentinelRef.current || !hasNextPage || isFetchingNextPage) {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          void fetchNextPage();
        }
      },
      { rootMargin: '600px 0px' }
    );

    observer.observe(sentinelRef.current);
    return () => observer.disconnect();
  }, [fetchNextPage, hasNextPage, isFetchingNextPage, visibleItems.length]);

  const handleFilterChange = (value: string | number | undefined) => {
    const nextFilter = (value as GlobalActivityFilter | undefined) ?? 'all';
    startTransition(() => {
      setSelectedFilter(nextFilter);
    });
  };

  const handleMergePending = () => {
    if (pendingNewItems.length === 0) {
      return;
    }

    const freshKeys = new Set(pendingNewItems.map(itemKey));
    setPrependedItems((current) => mergeUniqueActivities(pendingNewItems, current));
    setPendingNewItems([]);
    setHighlightedKeys((current) => new Set([...current, ...freshKeys]));
    scheduleHighlightReset();
    safeScrollIntoView(topAnchorRef.current, { behavior: 'smooth', block: 'start' });
  };

  const statusLabel = isFetchingNextPage || headQuery.isFetching ? 'active' : 'inactive';
  const initialError = !isLoading && !!error && visibleItems.length === 0;
  const emptyState = !isLoading && !error && visibleItems.length === 0;
  const hasPending = pendingNewItems.length > 0;

  return (
    <div className="space-y-4">
      <div ref={topAnchorRef} />

      {headQuery.isError && visibleItems.length > 0 && (
        <div className="border-base-border bg-base-surface/90 text-text-dim rounded-lg border px-3 py-2 font-mono text-xs">
          Could not refresh:{' '}
          {headQuery.error instanceof Error
            ? headQuery.error.message
            : 'unable to refresh head stream right now'}
        </div>
      )}

      <section
        className="min-h-[44rem] min-w-0"
        data-testid="activities-stream-panel"
        aria-label="Activity feed"
      >
        <div
          data-testid="activities-stream-sticky-stack"
          className={cn(
            'sticky z-30',
            BELOW_STICKY_HEADER,
            hasPending && 'shadow-[0_10px_24px_rgba(0,0,0,0.28)]'
          )}
        >
          {hasPending && (
            <button
              type="button"
              onClick={handleMergePending}
              aria-label={`${pendingNewItems.length} new activit${pendingNewItems.length === 1 ? 'y' : 'ies'}`}
              className="border-jade/10 w-full rounded-none border-y bg-[#04070d] px-4 py-1.5 text-left transition-colors hover:bg-[#060a11] sm:py-2"
            >
              <span className="text-text text-xs">
                {pendingNewItems.length} new activit{pendingNewItems.length === 1 ? 'y' : 'ies'} ·
                Show at top
              </span>
            </button>
          )}

          <ActivityStreamToolbar
            selectedFilter={selectedFilter}
            visibleItemsCount={visibleItems.length}
            statusLabel={statusLabel}
            summary={summary}
            onFilterChange={handleFilterChange}
          />
        </div>
        {isLoading ? (
          <LoadingRows />
        ) : initialError ? (
          <div className="flex min-h-[20rem] flex-col items-center justify-center gap-3 px-6 py-12 text-center">
            <div className="text-text font-mono text-sm uppercase">Failed to load activities</div>
            <div className="text-text-dim max-w-lg text-sm">
              {error instanceof Error ? error.message : 'Unknown error'}
            </div>
          </div>
        ) : emptyState ? (
          <div className="flex min-h-[20rem] flex-col items-center justify-center gap-3 px-6 py-12 text-center">
            <div className="text-text font-mono text-sm uppercase">No activities yet</div>
            <div className="text-text-dim text-sm">No activity matches this filter.</div>
          </div>
        ) : (
          <div>
            {visibleItems.map((activity, index) => {
              const key = itemKey(activity);
              const previousLabel =
                index > 0 ? getActivityDayLabel(visibleItems[index - 1].timestamp) : null;
              const currentLabel = getActivityDayLabel(activity.timestamp);
              const showDayDivider = currentLabel !== previousLabel;

              return (
                <div key={key}>
                  {showDayDivider && <ActivityDayDivider label={currentLabel} />}
                  <ActivityRow activity={activity} fresh={highlightedKeys.has(key)} />
                </div>
              );
            })}

            <div ref={sentinelRef} data-testid="activities-stream-sentinel" className="h-4" />
            {hasNextPage && !isFetchingNextPage && (
              <div className="flex justify-center py-4">
                <button
                  type="button"
                  onClick={() => void fetchNextPage()}
                  className="border-base-border text-text hover:text-text-bright rounded-md border px-5 py-2 text-xs"
                >
                  Older
                </button>
              </div>
            )}

            {isFetchingNextPage && (
              <div className="text-text-dim border-base-border/50 border-t px-3 py-3 text-center font-mono text-xs uppercase">
                Loading older activities
              </div>
            )}

            {isFetchNextPageError && (
              <div className="border-base-border/50 border-t px-3 py-3 text-center">
                <div className="text-negative font-mono text-xs uppercase">
                  Failed to load older activities
                </div>
                <button
                  type="button"
                  className="text-jade hover:text-jade/80 mt-1 cursor-pointer font-mono text-xs uppercase underline transition-colors"
                  onClick={() => void fetchNextPage()}
                >
                  Retry
                </button>
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
