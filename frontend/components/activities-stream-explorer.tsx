'use client';

import { RetryAction } from '@/components/ui/retry-action';
import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { FilterChips } from '@/components/ui/filter-chips';
import { Pager } from '@/components/ui/pager';
import { cssVars, STREAM_COLORS } from '@/lib/design-tokens';
import { KIND_FILTER_LABEL, kindColor, kindFilterValues, kindWords } from '@/lib/kind-filter';
import { activityFilterCounts, previousChainDay } from '@/lib/activity-stats';
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

/**
 * The Activities board's kinds, in its order (TX-30): the kind filter's values (lib/kind-filter.ts,
 * the words and marks the transactions list's filter shares, PA-15).
 */
const ACTIVITY_KINDS: readonly GlobalActivityFilter[] = [
  'all',
  'ckb',
  'script',
  'dao',
  'token',
  'object',
  'identity',
  'protocol',
];

/**
 * The filter a URL names (`/activities?filter=dao`, the DAO page's "All DAO activity →"): a known
 * kind, else All (hierarchy D11).
 */
function filterFromUrl(raw: string | null): GlobalActivityFilter {
  const known = ACTIVITY_KINDS.find((kind) => kind === raw);
  return known ?? 'all';
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
 * divider. A group is keyed by the rows it was first drawn with (lane B4 fix rounds): a row keeps
 * the key of the group it was first drawn in (`anchors`, the stream remembers them), and a group
 * takes the key of the first of its rows that has one; a group none of whose rows was drawn yet is
 * keyed by its date and its first row. Its label ("Today", "Yesterday", "Nov 13") is the
 * reader's, relative to `now`, and is no part of its key. So a row arriving at the top, an older
 * page at the bottom, the turn of the day, and a new row whose date already stands lower (a
 * block's time may be earlier than its parent's, so near midnight a day can come again after the
 * next) leave every drawn group's key as it was, and no group is remounted under the reader.
 */
export function dayGroups(
  items: readonly GlobalActivity[],
  now: number,
  anchors: ReadonlyMap<string, string>
): { key: string; label: string; items: GlobalActivity[] }[] {
  const groups: { date: string; label: string; items: GlobalActivity[] }[] = [];
  for (const activity of items) {
    const date = formatUtc8Date(toActivityDate(activity.timestamp).getTime());
    const last = groups[groups.length - 1];
    if (last && last.date === date) last.items.push(activity);
    else
      groups.push({ date, label: getActivityDayLabel(activity.timestamp, now), items: [activity] });
  }
  const taken = new Set<string>();
  return groups.map(({ date, label, items: rows }) => {
    const key =
      rows
        .map((row) => anchors.get(itemKey(row)))
        .find((anchor) => anchor !== undefined && !taken.has(anchor)) ??
      `${date}:${itemKey(rows[0])}`;
    taken.add(key);
    return { key, label, items: rows };
  });
}

/**
 * The anchors the next render's `dayGroups` reads: each row drawn now and the group key it was first
 * drawn in (a row new to the page takes its group's). A row the feed no longer draws is not kept, so
 * a live feed holds anchors for the rows on the page, never for every row it ever drew.
 */
export function drawnAnchors(
  groups: readonly { key: string; items: readonly GlobalActivity[] }[],
  previous: ReadonlyMap<string, string>
): Map<string, string> {
  const next = new Map<string, string>();
  for (const group of groups)
    for (const row of group.items) next.set(itemKey(row), previous.get(itemKey(row)) ?? group.key);
  return next;
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
  onFilterChange: (value: GlobalActivityFilter) => void;
  summary?: DailyActivityStats;
  totalCount?: number;
}) {
  const counts = summary ? activityFilterCounts(summary, totalCount) : null;
  const occurrences = summary ? summary.kindOccurrences : 0;
  return (
    <div
      data-testid="activities-stream-toolbar"
      className="bg-base-bg border-row-rule border-b py-3"
    >
      {/* The controls board's kind filter (PA-15: the transactions list's, the same control) */}
      <FilterChips
        label={KIND_FILTER_LABEL}
        values={kindFilterValues(ACTIVITY_KINDS, counts ?? {})}
        value={selectedFilter}
        onChange={onFilterChange}
        counted
      />
      {/* the counts' period, said with them (lists lane P3: they are yesterday's, the list below
          is the latest, which its own line says) */}
      {summary && (
        <p className="text-text-dim text-meta mt-2">
          Yesterday’s counts (UTC+8): kinds may overlap, DAO counts actions.
        </p>
      )}
      {counts && occurrences > 0 && (
        <div
          role="img"
          aria-label={`Activity kind occurrences on ${summary!.date}; categories overlap`}
          className="mt-3 flex h-2 overflow-hidden"
        >
          {ACTIVITY_KINDS.filter(
            (kind) => kind !== 'all' && counts[kind] !== undefined && counts[kind]! > 0
          ).map((kind) => (
            <span
              key={kind}
              title={`${kindWords(kind)}: ${counts[kind]}`}
              style={{
                width: `${(counts[kind]! / occurrences) * 100}%`,
                backgroundColor: kindColor(kind),
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

  const handleFilterChange = (nextFilter: GlobalActivityFilter) => {
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

  // each drawn row's day group key, as first drawn (`dayGroups`): a prepend or an older page never
  // re-keys a group that already holds a drawn row; only the rows drawn now are kept (`drawnAnchors`)
  const groupAnchors = useRef<ReadonlyMap<string, string>>(new Map());
  const groups = dayGroups(visibleItems, now, groupAnchors.current);
  useEffect(() => {
    groupAnchors.current = drawnAnchors(groups, groupAnchors.current);
  });

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
            <p className="text-text max-w-measure break-words">
              {error instanceof Error ? error.message : String(error)}
            </p>
          </div>
        ) : emptyState ? (
          <p className="text-text-dim py-4">Nothing of this kind in the latest activities.</p>
        ) : (
          <div>
            {/* the list's period, as the counts' above (lists lane P3): the latest activities, as
                they arrive, under each day's divider */}
            <div data-role="list-period" className="text-text-dim text-small pt-3">
              the latest · newest first · by day (UTC+8)
            </div>
            {/* Readability B4: each day's rows, under its divider, with their runs folded; a run
                folds within its day and across the older pages appended to it. */}
            {groups.map(({ key, label, items }, at) => (
              // under the filter too: another filter shows another set of rows, and its runs'
              // state starts anew (a run the reader never opened under it is never drawn open)
              <div key={`${selectedFilter}:${key}`}>
                <ActivityDayDivider label={label} />
                <ActivityRuns
                  age
                  // the last day's last run, older pages unread, is not final
                  more={hasNextPage === true && at === groups.length - 1}
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
            {/* The controls board's pager for a stream that grows downward: the next page's rows,
                as many as the stream's pages hold (the size the API answered with). */}
            {hasNextPage && !isFetchingNextPage && (
              <div className="py-4">
                <Pager
                  mode="more"
                  next={{
                    count: data!.pages[data!.pages.length - 1].limit,
                    onShow: () => void fetchNextPage(),
                  }}
                />
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
                <RetryAction
                  words="Retry"
                  onRetry={() => void fetchNextPage()}
                  className="text-small mt-1"
                />
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
