'use client';

import Link from '@/components/ui/link';
import { TerminalPanel, TerminalPanelContent, TerminalRow } from '@/components/ui/terminal-panel';
import { buildGlobalTxEvents, ParticipantLine } from '@/components/activity-event-row';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { startTransition, useEffect, useMemo, useRef, useState } from 'react';
import { api, type GlobalActivity, type GlobalActivityFilter } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { cn, formatTimeAgo, truncateHash } from '@/lib/utils';

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

function formatActivityTimeAgo(timestamp: string): string {
  return formatTimeAgo(toActivityDate(timestamp));
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

function getFilterLabel(filter: GlobalActivityFilter): string {
  return FILTER_OPTIONS.find((option) => option.value === filter)?.label ?? 'All';
}

function ActivityStreamToolbar({
  selectedFilter,
  visibleItemsCount,
  statusLabel,
  stacked = false,
  onFilterChange,
}: {
  selectedFilter: GlobalActivityFilter;
  visibleItemsCount: number;
  statusLabel: 'active' | 'inactive';
  stacked?: boolean;
  onFilterChange: (value: string | number | undefined) => void;
}) {
  const activeLabel = getFilterLabel(selectedFilter).toUpperCase();
  const indicatorClass =
    statusLabel === 'active' ? 'bg-jade shadow-[0_0_8px_rgba(46,219,163,0.3)]' : 'bg-jade/35';

  return (
    <div
      data-testid="activities-stream-toolbar"
      className={cn(
        'bg-[#060810]',
        stacked ? 'border-jade/10 border-b' : 'border-jade/10 border-y'
      )}
    >
      <div className="container mx-auto flex min-h-12 flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2.5">
        <div className="flex min-w-0 flex-1 items-center gap-0 overflow-x-auto font-mono text-[11px] tabular-nums leading-none">
          <span className={cn('mr-2 inline-block h-1.5 w-1.5 rounded-full', indicatorClass)} />
          <span className="text-jade/50 uppercase tracking-wider">filter</span>
          <span className="text-jade ml-1.5 font-bold transition-colors">{activeLabel}</span>
          <span className="text-jade/20 mx-2.5 select-none">|</span>
          <span className="text-jade/50 uppercase tracking-wider">loaded</span>
          <span className="text-jade ml-1.5 font-bold transition-colors">
            {visibleItemsCount.toLocaleString()}
          </span>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {FILTER_OPTIONS.map((option) => {
            const isActive = selectedFilter === option.value;

            return (
              <button
                key={option.value}
                type="button"
                onClick={() => onFilterChange(option.value)}
                className={cn(
                  'rounded-sm border px-2 py-1 font-mono text-[11px] uppercase tracking-[0.16em] transition-colors',
                  isActive
                    ? 'border-jade/20 bg-jade/8 text-jade'
                    : 'text-jade/45 hover:bg-jade/[0.04] hover:text-jade/80 border-transparent bg-transparent'
                )}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function ActivityDayDivider({ label }: { label: string }) {
  return (
    <div
      className="flex items-center gap-2 px-4 pb-1.5 pt-3.5"
      data-testid={getDayDividerTestId(label)}
    >
      <span className="bg-jade/55 block h-1 w-1 rounded-full" />
      <span className="text-text-dim font-mono text-[10px] uppercase tracking-[0.22em]">
        {label}
      </span>
      <div className="from-jade/12 via-base-border/55 h-px flex-1 bg-gradient-to-r to-transparent" />
    </div>
  );
}

// ---------------------------------------------------------------------------
// ActivityStreamRow — tx-centric layered view
// ---------------------------------------------------------------------------

interface ActivityStreamRowProps {
  activity: GlobalActivity;
  isNew?: boolean;
}

function ActivityStreamRow({ activity, isNew = false }: ActivityStreamRowProps) {
  const txEvents = buildGlobalTxEvents(activity);
  const txHref = `/tx/${activity.txHash}`;
  const blockHref = `/blocks/${activity.blockNumber}`;

  const rowAnimation = isNew
    ? {
        animation:
          'activity-stream-row-enter 280ms ease-out, activity-stream-row-glow 2s ease-out forwards',
      }
    : undefined;

  return (
    <TerminalRow
      role="article"
      aria-label={`Transaction ${activity.txHash.slice(0, 10)}`}
      className="px-4 py-4 sm:px-5"
      style={rowAnimation}
    >
      <div className="space-y-2.5">
        {/* TX header: hash · block#   time */}
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-1.5 font-mono text-sm">
            <Link href={txHref} className="text-text-dim hover:text-aqua transition-colors">
              <span className="text-text-dim/60 mr-0.5 text-xs">tx</span>
              {truncateHash(activity.txHash, 10, 8)}
            </Link>
            <span className="text-text-dim">{'\u00B7'}</span>
            <Link
              href={blockHref}
              className="text-text-dim hover:text-text text-xs transition-colors"
            >
              #{activity.blockNumber.toLocaleString()}
            </Link>
          </div>
          <span className="text-text-dim shrink-0 font-mono text-[10px]">
            {formatActivityTimeAgo(activity.timestamp)}
          </span>
        </div>

        {/* TX-level events: L3 protocol actions + L2 catch-all type/lock calls */}
        {txEvents.length > 0 && (
          <div className="space-y-1 pl-3">
            {txEvents.map((event, i) => (
              <div key={i} className="flex items-center justify-between gap-2">
                <div className="min-w-0 truncate">{event.badge}</div>
                <div className="shrink-0">{event.value}</div>
              </div>
            ))}
          </div>
        )}

        {/* Per-participant lines: L1 CKB + L2 item deltas */}
        <div className="space-y-1 pl-3">
          {activity.participants.map((p) => (
            <ParticipantLine key={p.address} participant={p} />
          ))}
        </div>
      </div>
    </TerminalRow>
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

export function ActivitiesStreamExplorer() {
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
          Live refresh paused:{' '}
          {headQuery.error instanceof Error
            ? headQuery.error.message
            : 'unable to refresh head stream right now'}
        </div>
      )}

      <TerminalPanel
        className="min-h-[44rem] overflow-visible"
        data-testid="activities-stream-panel"
      >
        <TerminalPanelContent padding="none">
          <div
            data-testid="activities-stream-sticky-stack"
            className={cn(
              'border-jade/10 sticky top-[5.25rem] z-30 border-x',
              hasPending && 'shadow-[0_10px_24px_rgba(0,0,0,0.28)]'
            )}
          >
            {hasPending && (
              <button
                type="button"
                onClick={handleMergePending}
                aria-label={`${pendingNewItems.length} new activit${pendingNewItems.length === 1 ? 'y' : 'ies'}`}
                className="border-jade/10 w-full rounded-none border-y bg-[#04070d] px-4 py-2 text-left transition-colors hover:bg-[#060a11]"
              >
                <div className="flex flex-wrap items-center gap-0 font-mono text-[11px] tabular-nums leading-none">
                  <span className="bg-jade/80 mr-2 block h-2 w-2 rounded-full shadow-[0_0_10px_rgba(46,219,163,0.35)]" />
                  <span className="text-jade/55 uppercase tracking-wider">LIVE BUFFER</span>
                  <span className="text-jade/20 mx-2.5 select-none">|</span>
                  <span className="text-jade font-bold uppercase tracking-[0.14em]">
                    {pendingNewItems.length} new activit
                    {pendingNewItems.length === 1 ? 'y' : 'ies'}
                  </span>
                  <span className="text-jade/20 mx-2.5 select-none">|</span>
                  <span className="text-text-dim uppercase tracking-[0.18em]">merge at top</span>
                </div>
              </button>
            )}

            <ActivityStreamToolbar
              selectedFilter={selectedFilter}
              visibleItemsCount={visibleItems.length}
              statusLabel={statusLabel}
              stacked={hasPending}
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
              <div className="text-text-dim text-sm">
                This filter has no canonical activity rows in the current window.
              </div>
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
                    <ActivityStreamRow activity={activity} isNew={highlightedKeys.has(key)} />
                  </div>
                );
              })}

              <div ref={sentinelRef} data-testid="activities-stream-sentinel" className="h-4" />

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
        </TerminalPanelContent>
      </TerminalPanel>
    </div>
  );
}
