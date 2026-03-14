'use client';

import Link from '@/components/ui/link';
import { FilterButtonGroup } from '@/components/ui/chart-card';
import {
  TerminalDivider,
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import {
  CkbDelta,
  LockCallBadge,
  LockCallExpr,
  TypeCallExpr,
  TYPE_SCRIPT_CALL_LABEL,
  capitalizeAction,
  formatStandard,
} from '@/components/activity-event-row';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { startTransition, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { api, type GlobalActivity, type GlobalActivityFilter } from '@/lib/api';
import { classifyActivity, type ClassifiedActivity } from '@/lib/activity-classify';
import {
  getIdentityItemDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatTokenBalance } from '@/lib/format-asset';
import { cn, formatCkbAmount, formatTimeAgo, truncateHash } from '@/lib/utils';

const PAGE_SIZE = 20;
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
  return `${activity.txHash}:${activity.address}`;
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

function truncateAddress(addr: string): string {
  return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
}

function formatAddress(address: string): string {
  if (address.startsWith('ckb1') || address.startsWith('ckt1')) {
    return truncateAddress(address);
  }
  return truncateHash(address);
}

interface TypeBadgeInfo {
  icon: string;
  label: string;
  colorClass: string;
}

function getTypeBadge(classified: ClassifiedActivity): TypeBadgeInfo {
  const { displayType, primaryAssetChange } = classified;

  switch (displayType) {
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
        return {
          icon: '\u2B21',
          label: `${formatStandard(change.standard)} ${capitalizeAction(change.action)}`,
          colorClass: 'text-lavender',
        };
      }
      return { icon: '\u2B21', label: 'Object', colorClass: 'text-lavender' };
    }
    case 'identity': {
      const change = primaryAssetChange;
      if (change && change.type === 'identity') {
        return {
          icon: '\u2726',
          label: `${formatStandard(change.standard)} ${capitalizeAction(change.action)}`,
          colorClass: 'text-aqua',
        };
      }
      return { icon: '\u2726', label: 'Identity', colorClass: 'text-aqua' };
    }
    case 'protocolAction': {
      const pa = classified.primaryProtocolAction;
      return {
        icon: '\u26A1',
        label: pa ? pa.protocol.toUpperCase() : 'Protocol',
        colorClass: 'text-violet',
      };
    }
    case 'typeCall':
      return { icon: '\u2699', label: TYPE_SCRIPT_CALL_LABEL, colorClass: 'text-amber' };
    case 'ckbTransfer':
      return { icon: '\u2197', label: 'CKB Transfer', colorClass: 'text-jade' };
    default:
      return { icon: '\u2197', label: 'Transfer', colorClass: 'text-jade' };
  }
}

function AddressLink({ address }: { address: string }) {
  return (
    <Link href={`/address/${address}`} className="text-text hover:text-aqua font-mono text-xs">
      {formatAddress(address)}
    </Link>
  );
}

function renderPrimaryValue(classified: ClassifiedActivity) {
  const { activity, primaryAssetChange, primaryProtocolAction, primaryTypeCall } = classified;

  switch (classified.displayType) {
    case 'daoDeposit': {
      const capacity =
        primaryAssetChange && primaryAssetChange.type === 'daoDeposit'
          ? primaryAssetChange.capacity
          : '0';
      return (
        <span className="text-positive font-mono text-xs tabular-nums">
          +{formatCkbAmount(capacity).full} CKB locked
        </span>
      );
    }
    case 'daoWithdrawRequest': {
      const capacity =
        primaryAssetChange && primaryAssetChange.type === 'daoWithdrawRequest'
          ? primaryAssetChange.capacity
          : '0';
      return (
        <span className="text-gold font-mono text-xs tabular-nums">
          {formatCkbAmount(capacity).full} CKB
        </span>
      );
    }
    case 'daoWithdrawComplete': {
      const capacity =
        primaryAssetChange && primaryAssetChange.type === 'daoWithdrawComplete'
          ? primaryAssetChange.capacity
          : '0';
      return (
        <span className="text-positive font-mono text-xs tabular-nums">
          +{formatCkbAmount(capacity).full} CKB
        </span>
      );
    }
    case 'token': {
      if (primaryAssetChange?.type !== 'token') {
        return <CkbDelta delta={activity.ckbDelta} />;
      }
      const delta = BigInt(primaryAssetChange.delta);
      const prefix = delta > BigInt(0) ? '+' : delta < BigInt(0) ? '-' : '';
      const balance = formatTokenBalance(
        primaryAssetChange.delta.startsWith('-')
          ? primaryAssetChange.delta.slice(1)
          : primaryAssetChange.delta,
        primaryAssetChange.decimals ?? 0
      );
      const label =
        primaryAssetChange.symbol ?? truncateHash(primaryAssetChange.typeScriptHash, 8, 6);
      const colorClass =
        delta > BigInt(0) ? 'text-positive' : delta < BigInt(0) ? 'text-negative' : 'text-text-dim';
      return (
        <Link
          href={getTokenDetailHref(primaryAssetChange.typeScriptHash)}
          className={cn('font-mono text-xs tabular-nums hover:underline', colorClass)}
        >
          {prefix}
          {balance} {label}
        </Link>
      );
    }
    case 'object': {
      if (primaryAssetChange?.type !== 'object') {
        return <CkbDelta delta={activity.ckbDelta} />;
      }
      return (
        <Link
          href={getObjectDetailHref(primaryAssetChange.objectId)}
          className="text-lavender/80 hover:text-lavender font-mono text-xs"
        >
          {truncateHash(primaryAssetChange.objectId, 8, 6)}
        </Link>
      );
    }
    case 'identity': {
      if (primaryAssetChange?.type !== 'identity') {
        return <CkbDelta delta={activity.ckbDelta} />;
      }
      return (
        <Link
          href={getIdentityItemDetailHref(
            primaryAssetChange.standard,
            primaryAssetChange.identityId
          )}
          className="text-aqua/80 hover:text-aqua font-mono text-xs"
        >
          {truncateHash(primaryAssetChange.identityId, 8, 6)}
        </Link>
      );
    }
    case 'protocolAction':
      return (
        <span className="text-violet font-mono text-xs uppercase">
          {primaryProtocolAction?.action ?? 'action'}
        </span>
      );
    case 'typeCall':
      return primaryTypeCall ? (
        <span className="font-mono text-xs">
          <TypeCallExpr sc={primaryTypeCall} />
        </span>
      ) : (
        <span className="text-amber font-mono text-xs">{TYPE_SCRIPT_CALL_LABEL}</span>
      );
    case 'ckbTransfer':
    default:
      return <CkbDelta delta={activity.ckbDelta} />;
  }
}

function renderDetailLine(classified: ClassifiedActivity) {
  const { activity, primaryProtocolAction, primaryAssetChange, primaryTypeCall, primaryLockCall } =
    classified;
  const pieces: ReactNode[] = [];

  if (
    !['ckbTransfer', 'daoDeposit', 'daoWithdrawRequest', 'daoWithdrawComplete'].includes(
      classified.displayType
    ) &&
    activity.ckbDelta !== '0'
  ) {
    pieces.push(<CkbDelta key="ckb-delta" delta={activity.ckbDelta} />);
  }

  if (primaryProtocolAction) {
    const btcTxid = primaryProtocolAction.metadata?.btcTxid;
    pieces.push(
      <span key="protocol-action" className="text-violet font-mono text-[11px] uppercase">
        {primaryProtocolAction.protocol}:{primaryProtocolAction.action}
      </span>
    );
    if (typeof btcTxid === 'string') {
      pieces.push(
        <span key="btc-txid" className="text-text-dim font-mono text-[11px]">
          btc:{truncateHash(btcTxid, 8, 6)}
        </span>
      );
    }
    if (primaryAssetChange?.type === 'token') {
      pieces.push(
        <span key="protocol-token" className="text-text-dim font-mono text-[11px]">
          token {primaryAssetChange.symbol ?? truncateHash(primaryAssetChange.typeScriptHash, 8, 6)}
        </span>
      );
    }
  } else if (primaryTypeCall) {
    pieces.push(
      <span key="type-call" className="text-text-dim font-mono text-[11px]">
        <TypeCallExpr sc={primaryTypeCall} />
      </span>
    );
  } else if (primaryLockCall) {
    pieces.push(
      <span key="lock-call" className="text-text-dim font-mono text-[11px]">
        <LockCallExpr lc={primaryLockCall} />
      </span>
    );
  } else if (primaryAssetChange?.type === 'daoWithdrawComplete') {
    pieces.push(
      <span key="dao-compensation" className="text-positive font-mono text-[11px] tabular-nums">
        +{formatCkbAmount(primaryAssetChange.compensation).full} CKB compensation
      </span>
    );
  } else if (activity.lockCalls.length > 0 && !primaryLockCall) {
    pieces.push(
      <span key="lock-call-fallback" className="text-text-dim font-mono text-[11px]">
        <LockCallExpr lc={activity.lockCalls[0]} />
      </span>
    );
  }

  if (pieces.length === 0) {
    return null;
  }

  return <div className="flex flex-wrap items-center gap-x-3 gap-y-1">{pieces}</div>;
}

interface ActivityStreamRowProps {
  activity: GlobalActivity;
  isNew?: boolean;
}

function ActivityStreamRow({ activity, isNew = false }: ActivityStreamRowProps) {
  const classified = classifyActivity(activity);
  const badge = getTypeBadge(classified);
  const txHref = `/tx/${activity.txHash}`;
  const blockHref = `/blocks/${activity.blockNumber}`;
  const detailLine = renderDetailLine(classified);

  return (
    <TerminalRow
      className="space-y-2"
      style={
        isNew
          ? {
              animation:
                'activity-stream-row-enter 280ms ease-out, activity-stream-row-glow 2s ease-out forwards',
            }
          : undefined
      }
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className={cn('font-mono text-xs', badge.colorClass)}>
            {badge.icon} {badge.label}
          </span>
          {classified.primaryLockCall && <LockCallBadge lc={classified.primaryLockCall} />}
        </div>
        <div className="flex flex-wrap items-center justify-end gap-x-3 gap-y-1 font-mono text-[10px]">
          <Link href={txHref} className="text-text-dim hover:text-aqua">
            tx {truncateHash(activity.txHash, 8, 6)}
          </Link>
          <Link href={blockHref} className="text-text-dim hover:text-text">
            #{activity.blockNumber.toLocaleString()}
          </Link>
          <span className="text-text-dim">{formatActivityTimeAgo(activity.timestamp)}</span>
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2">
        <AddressLink address={activity.address} />
        {renderPrimaryValue(classified)}
      </div>

      {detailLine && <div>{detailLine}</div>}
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

  useEffect(() => {
    setPrependedItems([]);
    setPendingNewItems([]);
    setHighlightedKeys(new Set());
    if (highlightTimerRef.current) {
      window.clearTimeout(highlightTimerRef.current);
      highlightTimerRef.current = null;
    }
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
      if (highlightTimerRef.current) {
        window.clearTimeout(highlightTimerRef.current);
      }
      highlightTimerRef.current = window.setTimeout(() => {
        setHighlightedKeys(new Set());
        highlightTimerRef.current = null;
      }, ROW_HIGHLIGHT_DURATION_MS);
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
    if (highlightTimerRef.current) {
      window.clearTimeout(highlightTimerRef.current);
    }
    highlightTimerRef.current = window.setTimeout(() => {
      setHighlightedKeys(new Set());
      highlightTimerRef.current = null;
    }, ROW_HIGHLIGHT_DURATION_MS);
    safeScrollIntoView(topAnchorRef.current, { behavior: 'smooth', block: 'start' });
  };

  const statusLabel = isFetchingNextPage || headQuery.isFetching ? 'active' : 'inactive';
  const initialError = !isLoading && !!error && visibleItems.length === 0;
  const emptyState = !isLoading && !error && visibleItems.length === 0;

  return (
    <div className="space-y-4">
      <div ref={topAnchorRef} />

      <div className="bg-base-bg/95 border-base-border sticky top-[4.5rem] z-20 rounded-lg border backdrop-blur-sm">
        <div className="flex flex-wrap items-center justify-between gap-3 px-3 py-3">
          <div className="text-text-dim font-mono text-[11px] uppercase tracking-widest">
            Stream Filters
          </div>
          <div className="text-text-dim font-mono text-[11px]">
            {visibleItems.length.toLocaleString()} loaded
          </div>
        </div>
        <div className="px-3 pb-3">
          <FilterButtonGroup
            options={FILTER_OPTIONS}
            selected={selectedFilter}
            onChange={handleFilterChange}
            className="flex-wrap"
          />
        </div>
      </div>

      {pendingNewItems.length > 0 && (
        <button
          type="button"
          onClick={handleMergePending}
          className="border-jade/40 bg-jade/10 text-jade hover:bg-jade/15 sticky top-[8.75rem] z-10 w-full rounded-lg border px-3 py-2 font-mono text-xs uppercase tracking-wider transition-colors"
        >
          {pendingNewItems.length} new activit{pendingNewItems.length === 1 ? 'y' : 'ies'}
        </button>
      )}

      {headQuery.isError && visibleItems.length > 0 && (
        <div className="border-base-border bg-base-surface/90 text-text-dim rounded-lg border px-3 py-2 font-mono text-xs">
          Live refresh paused:{' '}
          {headQuery.error instanceof Error
            ? headQuery.error.message
            : 'unable to refresh head stream right now'}
        </div>
      )}

      <TerminalPanel className="min-h-[44rem]">
        <TerminalPanelHeader indicator={statusLabel}>Global Activity Stream</TerminalPanelHeader>
        <TerminalPanelContent padding="none">
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
                    {showDayDivider && (
                      <TerminalDivider className="px-3 py-2" label={currentLabel} />
                    )}
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
                </div>
              )}

              {!hasNextPage && !isFetchingNextPage && visibleItems.length > 0 && (
                <div className="text-text-dim border-base-border/50 border-t px-3 py-3 text-center font-mono text-xs uppercase">
                  End of stream
                </div>
              )}
            </div>
          )}
        </TerminalPanelContent>
      </TerminalPanel>
    </div>
  );
}
