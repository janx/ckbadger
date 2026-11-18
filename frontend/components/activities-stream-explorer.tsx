'use client';

import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { ClassMark } from '@/components/ui/class-mark';
import {
  CLASS_COLORS,
  cssVars,
  DATA_COLOR,
  STREAM_COLORS,
  type AssetClass,
} from '@/lib/design-tokens';
import { totalActivities, activityFilterCounts, previousChainDay } from '@/lib/activity-stats';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import { startTransition, useEffect, useMemo, useRef, useState } from 'react';
import { useInRouterContext } from 'react-router-dom';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import {
  api,
  type DailyActivityStats,
  type GlobalActivity,
  type GlobalActivityFilter,
} from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { usePageSize } from '@/hooks/usePageSize';
import { cn, formatUtc8Date } from '@/lib/utils';
import { useClockTime } from '@/components/ui/utc8-date';
import { countNoun } from '@/lib/format-count';

const PAGE_SIZE = DEFAULT_PAGE_SIZE;
const POLL_INTERVAL_MS = 10_000;
const NEAR_TOP_THRESHOLD_PX = 120;
const ROW_HIGHLIGHT_DURATION_MS = 2_000;

/** The Activities board's chips, in its order and words (TX-30), shared with Transactions. */
const FILTER_OPTIONS: Array<{
  label: string;
  value: GlobalActivityFilter;
  swatch?: string;
  mark?: AssetClass;
}> = [
  { label: 'All', value: 'all' },
  { label: 'CKB transfers', value: 'ckb', swatch: DATA_COLOR },
  { label: 'Script calls', value: 'script', swatch: CLASS_COLORS.ckb },
  { label: 'Nervos DAO', value: 'dao', mark: 'dao' },
  { label: 'Tokens', value: 'token', mark: 'tokens' },
  { label: 'Objects', value: 'object', mark: 'objects' },
  { label: 'Identities', value: 'identity', mark: 'identities' },
  { label: 'Protocols', value: 'protocol' },
];

/**
 * The filter a URL names (`/activities?filter=dao`, the DAO page's "All DAO activity →"): a known
 * chip value, else All (hierarchy D11).
 */
function filterFromUrl(raw: string | null): GlobalActivityFilter {
  const known = FILTER_OPTIONS.find((option) => option.value === raw);
  return known ? known.value : 'all';
}

const STREAM_KEYFRAMES = `
@keyframes activity-stream-row-enter {
  from { transform: translateY(-6px); opacity: 0; }
  to { transform: translateY(0); opacity: 1; }
}
@keyframes activity-stream-row-glow {
  from { background-color: rgb(var(--rgb-jade) / 0.1); }
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

/**
 * The divider a row falls under, on the site's one UTC+8 clock (D2), never the browser's zone:
 * "Today" and "Yesterday" are the reader's, at `now` from the page's one clock (lane E4).
 */
function getActivityDayLabel(timestamp: string, now: number): string {
  const day = formatUtc8Date(toActivityDate(timestamp).getTime());
  if (day === formatUtc8Date(now)) return 'Today';
  if (day === previousChainDay(now)) return 'Yesterday';
  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  }).format(new Date(`${day}T00:00:00Z`));
}

/**
 * The stream's rows by day, in order: consecutive rows of one UTC+8 date are one group under one
 * divider. A group is keyed by what it is, that date and which time the date comes in the stream
 * (a block's time may be earlier than its parent's, so near midnight a day can come again after
 * the next); its label ("Today", "Yesterday", "Nov 13") is the reader's, relative to `now`, and
 * is no part of its key. A row arriving at the top, an older page at the bottom and the turn of
 * the day leave every key as it was, so no group is remounted under the reader (lane B4 fix
 * rounds).
 */
function dayGroups(
  items: readonly GlobalActivity[],
  now: number
): { key: string; label: string; items: GlobalActivity[] }[] {
  const groups: { key: string; date: string; label: string; items: GlobalActivity[] }[] = [];
  const seen = new Map<string, number>();
  for (const activity of items) {
    const date = formatUtc8Date(toActivityDate(activity.timestamp).getTime());
    const last = groups[groups.length - 1];
    if (last && last.date === date) {
      last.items.push(activity);
      continue;
    }
    const time = seen.get(date) ?? 0;
    seen.set(date, time + 1);
    groups.push({
      key: `${date}#${time}`,
      date,
      label: getActivityDayLabel(activity.timestamp, now),
      items: [activity],
    });
  }
  return groups;
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

function ActivityStreamToolbar({
  selectedFilter,
  onFilterChange,
  summary,
  totalCount,
}: {
  selectedFilter: GlobalActivityFilter;
  onFilterChange: (value: string | number | undefined) => void;
  summary?: DailyActivityStats;
  totalCount?: number;
}) {
  const counts = summary ? activityFilterCounts(summary, totalCount) : null;
  const occurrences = summary ? totalActivities(summary) : 0;
  return (
    <div
      data-testid="activities-stream-toolbar"
      className="bg-base-bg border-row-rule border-b py-3"
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
              'text-small flex items-center gap-2 border px-3 py-2 transition-colors',
              selectedFilter === option.value
                ? 'border-base-border text-text-bright bg-base-surface'
                : 'text-text-dim hover:text-text border-transparent'
            )}
          >
            {option.mark && <ClassMark kind={option.mark} />}
            {option.swatch && (
              <span
                aria-hidden="true"
                data-swatch
                className="h-[9px] w-[9px]"
                style={{ backgroundColor: option.swatch }}
              />
            )}
            {option.label}
            {counts && counts[option.value] !== undefined && (
              <span className="text-text-dim text-meta tabular-nums">
                {counts[option.value]!.toLocaleString('en-US')}
              </span>
            )}
          </button>
        ))}
      </div>
      {summary && (
        <p className="text-text-dim text-meta mt-2">
          Yesterday, UTC+8 · kind occurrences may overlap; DAO counts actions.
        </p>
      )}
      {counts && occurrences > 0 && (
        <div
          role="img"
          aria-label={`Activity kind occurrences on ${summary!.date}; categories overlap`}
          className="mt-3 flex h-2 overflow-hidden"
        >
          {FILTER_OPTIONS.filter(
            (o) => o.value !== 'all' && counts[o.value] !== undefined && counts[o.value]! > 0
          ).map((o) => (
            <span
              key={o.value}
              title={`${o.label}: ${counts[o.value]}`}
              style={{
                width: `${(counts[o.value]! / occurrences) * 100}%`,
                backgroundColor: o.mark ? CLASS_COLORS[o.mark] : (o.swatch ?? CLASS_COLORS.other),
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
      className="text-text-dim border-row-rule text-meta mt-5 border-b py-2"
      data-testid={getDayDividerTestId(label)}
    >
      {label}
    </div>
  );
}

function LoadingRows() {
  return (
    <>
      <p role="status" className="text-text-dim text-small py-3">
        Reading activities…
      </p>
      <SkeletonRows />
    </>
  );
}

function SkeletonRows() {
  return Array.from({ length: 8 }).map((_, index) => (
    <div
      key={index}
      className="border-row-rule border-b px-3 py-3 last:border-b-0"
      data-testid="activities-stream-skeleton"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="bg-base-elevated h-4 w-28 animate-pulse" />
        <div className="bg-base-elevated h-3 w-24 animate-pulse" />
      </div>
      <div className="mt-2 flex items-center justify-between gap-2">
        <div className="bg-base-elevated h-3 w-32 animate-pulse" />
        <div className="bg-base-elevated h-3 w-20 animate-pulse" />
      </div>
    </div>
  ));
}

export function ActivitiesStreamExplorer({
  summary,
  totalCount,
}: {
  summary?: DailyActivityStats;
  totalCount?: number;
}) {
  useEffect(() => ensureKeyframes(), []);
  // "Today" and "Yesterday" move on at UTC+8 midnight: the stream renders again when the date does
  const now = useClockTime((t) => formatUtc8Date(t));

  const searchParams = useSearchParams();
  const pathname = usePathname();
  const router = useRouter();
  const inRouter = useInRouterContext();
  const urlFilter = filterFromUrl(searchParams.get('filter'));
  const [selectedFilter, setSelectedFilter] = useState<GlobalActivityFilter>(urlFilter);
  // D11: the stream follows its URL, also when a link or history entry changes it under this page.
  useEffect(() => setSelectedFilter(urlFilter), [urlFilter]);
  const pageSize = usePageSize(PAGE_SIZE);
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

  const stream = useInfiniteQuery({
    queryKey: ['global-activities-stream', selectedFilter, pageSize],
    queryFn: ({ pageParam }) =>
      api.getGlobalActivities({
        limit: pageSize,
        cursor: pageParam,
        filter: selectedFilter,
      }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) =>
      lastPage.hasMore ? (lastPage.nextCursor ?? undefined) : undefined,
  });
  const { data, error, hasNextPage, fetchNextPage, isFetchingNextPage, isFetchNextPageError } =
    stream;
  // awaited, not isLoading: a first page paused offline or between retries has not answered
  // "nothing of this kind"
  const awaited = isAwaited(stream);

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
    // D11: the URL names the chosen filter (replace: a filter is not a step of history). Outside a
    // router there is no address to keep in step, and leaving the page would reload it.
    if (inRouter) {
      const query = new URLSearchParams(searchParams);
      if (nextFilter === 'all') query.delete('filter');
      else query.set('filter', nextFilter);
      router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
    }
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

  const initialError = !awaited && !!error && visibleItems.length === 0;
  const emptyState = !awaited && !error && visibleItems.length === 0;
  const hasPending = pendingNewItems.length > 0;

  return (
    <div className="space-y-4">
      <div ref={topAnchorRef} />

      {headQuery.isError && visibleItems.length > 0 && (
        <div className="border-base-border bg-base-surface/90 text-text-dim text-small border px-3 py-2 font-mono">
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
            hasPending && 'shadow-[var(--m-raised-shadow)]'
          )}
        >
          {hasPending && (
            <button
              type="button"
              onClick={handleMergePending}
              aria-label={`${pendingNewItems.length} new ${countNoun(pendingNewItems.length, 'activity', 'activities')}`}
              className="border-jade/10 w-full border-y bg-[color:var(--bar)] px-4 py-1.5 text-left transition-colors hover:bg-[color:var(--bar-hover)] sm:py-2"
              style={cssVars({
                '--bar': STREAM_COLORS.pending,
                '--bar-hover': STREAM_COLORS.pendingHover,
              })}
            >
              <span className="text-text text-small">
                {pendingNewItems.length} new{' '}
                {countNoun(pendingNewItems.length, 'activity', 'activities')} · Show at top
              </span>
            </button>
          )}

          <ActivityStreamToolbar
            selectedFilter={selectedFilter}
            summary={summary}
            totalCount={totalCount}
            onFilterChange={handleFilterChange}
          />
        </div>
        {awaited ? (
          <LoadingRows />
        ) : initialError ? (
          <div role="alert" className="text-small flex flex-col gap-1 py-4">
            <p className="text-text">Could not read activities</p>
            <p className="text-text-dim max-w-measure break-words">
              {error instanceof Error ? error.message : String(error)}
            </p>
          </div>
        ) : emptyState ? (
          <p className="text-text-dim py-4">Nothing of this kind in the latest activities.</p>
        ) : (
          <div>
            {/* Readability B4: each day's rows, under its divider, with their runs folded; a run
                folds within its day and across the older pages appended to it. */}
            {dayGroups(visibleItems, now).map(({ key, label, items }) => (
              <div key={key}>
                <ActivityDayDivider label={label} />
                <ActivityRuns
                  age
                  rows={items.map((activity) => {
                    const key = itemKey(activity);
                    return {
                      key,
                      subject: runSubject(activity, activityTitleText(activity)),
                      activity,
                      title: <ActivityTitle activity={activity} />,
                      fresh: highlightedKeys.has(key),
                      row: <ActivityRow activity={activity} fresh={highlightedKeys.has(key)} age />,
                    };
                  })}
                />
              </div>
            ))}

            <div ref={sentinelRef} data-testid="activities-stream-sentinel" className="h-4" />
            {hasNextPage && !isFetchingNextPage && (
              <div className="flex justify-center py-4">
                <button
                  type="button"
                  onClick={() => void fetchNextPage()}
                  className="border-base-border text-text hover:text-text-bright text-small border px-5 py-2"
                >
                  Older
                </button>
              </div>
            )}

            {isFetchingNextPage && (
              <p role="status" className="text-text-dim text-small py-3 text-center">
                Reading older activities…
              </p>
            )}

            {isFetchNextPageError && (
              <div className="border-row-rule border-t px-3 py-3 text-center">
                <p className="text-text text-small">Could not read older activities</p>
                <button
                  type="button"
                  className="text-jade hover:text-jade-dim text-small mt-1 cursor-pointer underline transition-colors"
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
