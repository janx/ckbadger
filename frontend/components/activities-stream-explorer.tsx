'use client';

import Link from '@/components/ui/link';
import { TerminalPanel, TerminalPanelContent, TerminalRow } from '@/components/ui/terminal-panel';
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
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { cn, formatCkbAmount, formatTimeAgo, truncateHash } from '@/lib/utils';

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

function getDayDividerTestId(label: string): string {
  return `activity-day-divider-${label.toLowerCase().replace(/\s+/g, '-')}`;
}

interface TypeBadgeInfo {
  icon: string;
  label: string;
  colorClass: string;
}

function getTypeBadge(classified: ClassifiedActivity): TypeBadgeInfo {
  const { displayType } = classified;

  switch (displayType) {
    case 'daoDeposit':
    case 'daoWithdrawRequest':
    case 'daoWithdrawComplete':
      return { icon: '\u25C6', label: 'DAO', colorClass: 'text-gold' };
    case 'token':
      return { icon: '\u25CE', label: 'Token', colorClass: 'text-token' };
    case 'object':
      return { icon: '\u2B21', label: 'Object', colorClass: 'text-lavender' };
    case 'identity':
      return { icon: '\u2726', label: 'Identity', colorClass: 'text-aqua' };
    case 'protocolAction':
      return { icon: '\u26A1', label: 'Protocol', colorClass: 'text-violet' };
    case 'typeCall':
      return { icon: '\u2699', label: 'Script', colorClass: 'text-amber' };
    case 'ckbTransfer':
      return { icon: '\u2197', label: 'CKB', colorClass: 'text-jade' };
    default:
      return { icon: '\u2197', label: 'Activity', colorClass: 'text-jade' };
  }
}

function formatProtocolName(protocol: string): string {
  if (protocol === 'rgbpp') return 'RGB++';
  if (protocol === 'utxoswap') return 'UTXOSwap';
  return protocol.charAt(0).toUpperCase() + protocol.slice(1);
}

function titleize(value: string): string {
  return value
    .split(/[_-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function getActivityHeadline(classified: ClassifiedActivity): string {
  const { primaryAssetChange, primaryProtocolAction } = classified;

  switch (classified.displayType) {
    case 'daoDeposit':
      return 'DAO Deposit';
    case 'daoWithdrawRequest':
      return 'DAO Withdraw Request';
    case 'daoWithdrawComplete':
      return 'DAO Withdraw Complete';
    case 'token': {
      const change = primaryAssetChange;
      if (change && change.type === 'token') {
        return `${change.symbol ?? truncateHash(change.typeScriptHash, 8, 6)} Transfer`;
      }
      return 'Token Transfer';
    }
    case 'object': {
      const change = primaryAssetChange;
      if (change && change.type === 'object') {
        return `${formatStandard(change.standard)} ${capitalizeAction(change.action)}`;
      }
      return 'Object Activity';
    }
    case 'identity': {
      const change = primaryAssetChange;
      if (change && change.type === 'identity') {
        return `${formatStandard(change.standard)} ${capitalizeAction(change.action)}`;
      }
      return 'Identity Activity';
    }
    case 'protocolAction': {
      if (!primaryProtocolAction) {
        return 'Protocol Action';
      }
      return `${formatProtocolName(primaryProtocolAction.protocol)} \u00B7 ${titleize(primaryProtocolAction.action)}`;
    }
    case 'typeCall':
      return TYPE_SCRIPT_CALL_LABEL;
    case 'ckbTransfer':
      return 'CKB Transfer';
    default:
      return 'Activity';
  }
}

function AddressLink({ address, className }: { address: string; className?: string }) {
  return (
    <Link
      href={`/address/${address}`}
      className={cn('text-text hover:text-aqua font-mono text-xs transition-colors', className)}
    >
      {formatAddress(address)}
    </Link>
  );
}

function renderPrimaryValue(classified: ClassifiedActivity) {
  const { activity, primaryAssetChange, primaryProtocolAction } = classified;

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
      return <CkbDelta delta={activity.ckbDelta} />;
    }
    case 'identity': {
      return <CkbDelta delta={activity.ckbDelta} />;
    }
    case 'protocolAction': {
      if (primaryAssetChange?.type === 'token') {
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
          delta > BigInt(0)
            ? 'text-positive'
            : delta < BigInt(0)
              ? 'text-negative'
              : 'text-text-dim';
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

      const capacity = primaryProtocolAction?.metadata?.capacity;
      if (typeof capacity === 'string') {
        return (
          <span className="text-text font-mono text-xs tabular-nums">
            {formatCkbAmount(capacity).full} CKB
          </span>
        );
      }

      return <CkbDelta delta={activity.ckbDelta} />;
    }
    case 'typeCall':
      return <CkbDelta delta={activity.ckbDelta} />;
    case 'ckbTransfer':
    default:
      return <CkbDelta delta={activity.ckbDelta} />;
  }
}

function MetaChip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        'border-base-border/50 bg-base-elevated/70 text-text-dim inline-flex items-center rounded-md border px-2 py-1 font-mono text-[10px] leading-none',
        className
      )}
    >
      {children}
    </span>
  );
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

function renderSubjectLine(classified: ClassifiedActivity) {
  const { activity, primaryProtocolAction, primaryAssetChange, primaryTypeCall, primaryLockCall } =
    classified;

  switch (classified.displayType) {
    case 'daoDeposit':
      return <span className="text-text-dim">NervosDAO position created</span>;
    case 'daoWithdrawRequest':
      return primaryAssetChange?.type === 'daoWithdrawRequest' ? (
        <>
          <span className="text-text-dim">deposit block</span>
          <MetaChip>#{primaryAssetChange.depositBlock.toLocaleString()}</MetaChip>
        </>
      ) : (
        <span className="text-text-dim">NervosDAO withdraw request</span>
      );
    case 'daoWithdrawComplete':
      return primaryAssetChange?.type === 'daoWithdrawComplete' ? (
        <>
          <span className="text-text-dim">compensation</span>
          <span className="text-positive font-mono text-xs tabular-nums">
            +{formatCkbAmount(primaryAssetChange.compensation).full} CKB
          </span>
        </>
      ) : (
        <span className="text-text-dim">NervosDAO withdrawal completed</span>
      );
    case 'token':
      return primaryAssetChange?.type === 'token' ? (
        <>
          <Link
            href={getTokenDetailHref(primaryAssetChange.typeScriptHash)}
            className="text-token/85 hover:text-token font-mono text-xs transition-colors"
          >
            {primaryAssetChange.symbol ?? truncateHash(primaryAssetChange.typeScriptHash, 8, 6)}
          </Link>
          {activity.ckbDelta !== '0' && (
            <>
              <span className="text-text-dim">ckb</span>
              <CkbDelta delta={activity.ckbDelta} />
            </>
          )}
        </>
      ) : null;
    case 'object':
      return primaryAssetChange?.type === 'object' ? (
        <>
          <Link
            href={getObjectDetailHref(primaryAssetChange.objectId)}
            className="text-lavender/80 hover:text-lavender font-mono text-xs transition-colors"
          >
            {truncateHash(primaryAssetChange.objectId, 8, 6)}
          </Link>
          {activity.ckbDelta !== '0' && (
            <>
              <span className="text-text-dim">ckb</span>
              <CkbDelta delta={activity.ckbDelta} />
            </>
          )}
        </>
      ) : null;
    case 'identity':
      return primaryAssetChange?.type === 'identity' ? (
        <>
          <Link
            href={getIdentityItemDetailHref(
              primaryAssetChange.standard,
              primaryAssetChange.identityId
            )}
            className="text-aqua/80 hover:text-aqua font-mono text-xs transition-colors"
          >
            {truncateHash(primaryAssetChange.identityId, 8, 6)}
          </Link>
          {activity.ckbDelta !== '0' && (
            <>
              <span className="text-text-dim">ckb</span>
              <CkbDelta delta={activity.ckbDelta} />
            </>
          )}
        </>
      ) : null;
    case 'protocolAction': {
      const pieces: ReactNode[] = [];
      const btcTxid = primaryProtocolAction?.metadata?.btcTxid;
      const capacity = primaryProtocolAction?.metadata?.capacity;

      if (primaryAssetChange?.type === 'token') {
        const label =
          primaryAssetChange.symbol ?? truncateHash(primaryAssetChange.typeScriptHash, 8, 6);
        pieces.push(
          <Link
            key="token"
            href={getTokenDetailHref(primaryAssetChange.typeScriptHash)}
            className="text-token/85 hover:text-token font-mono text-xs transition-colors"
          >
            {label}
          </Link>
        );
      }

      if (typeof capacity === 'string') {
        pieces.push(
          <span key="capacity" className="text-text-dim font-mono text-xs tabular-nums">
            capacity {formatCkbAmount(capacity).full} CKB
          </span>
        );
      }

      if (typeof btcTxid === 'string') {
        pieces.push(
          <span key="btc" className="text-text-dim font-mono text-xs">
            btc {truncateHash(btcTxid, 8, 6)}
          </span>
        );
      }

      if (primaryTypeCall) {
        pieces.push(
          <span key="type-call" className="font-mono text-xs">
            <TypeCallExpr sc={primaryTypeCall} />
          </span>
        );
      } else if (primaryLockCall) {
        pieces.push(
          <span key="lock-call" className="font-mono text-xs">
            <LockCallExpr lc={primaryLockCall} />
          </span>
        );
      }

      return pieces.length > 0 ? pieces : <span className="text-text-dim">Protocol activity</span>;
    }
    case 'typeCall':
      return primaryTypeCall ? (
        <span className="font-mono text-xs">
          <TypeCallExpr sc={primaryTypeCall} />
        </span>
      ) : primaryLockCall ? (
        <span className="font-mono text-xs">
          <LockCallExpr lc={primaryLockCall} />
        </span>
      ) : (
        <span className="text-text-dim">Type script activity</span>
      );
    case 'ckbTransfer':
    default:
      if (activity.peers.length === 0) {
        return <span className="text-text-dim">Owner balance change</span>;
      }
      if (activity.peers.length === 1) {
        return (
          <>
            <span className="text-text-dim">with</span>
            <AddressLink address={activity.peers[0]} className="text-xs" />
          </>
        );
      }
      return (
        <span className="text-text-dim font-mono text-xs">
          {activity.peers.length} counterparties
        </span>
      );
  }
}

interface ActivityStreamRowProps {
  activity: GlobalActivity;
  isNew?: boolean;
}

function ActivityStreamRow({ activity, isNew = false }: ActivityStreamRowProps) {
  const classified = classifyActivity(activity);
  const badge = getTypeBadge(classified);
  const headline = getActivityHeadline(classified);
  const txHref = `/tx/${activity.txHash}`;
  const blockHref = `/blocks/${activity.blockNumber}`;
  const subjectLine = renderSubjectLine(classified);
  const metaChipClass =
    'border-base-border/45 bg-base-elevated/55 text-text-dim hover:text-aqua inline-flex items-center rounded-md border px-2 py-1 font-mono text-[10px] leading-none transition-colors';
  const rowAnimation = isNew
    ? {
        animation:
          'activity-stream-row-enter 280ms ease-out, activity-stream-row-glow 2s ease-out forwards',
      }
    : undefined;

  return (
    <TerminalRow
      role="article"
      aria-label={headline}
      className="px-4 py-4 sm:px-5"
      style={rowAnimation}
    >
      <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-start lg:gap-4">
        <div className="min-w-0 space-y-2.5">
          <div className="flex flex-wrap items-center gap-2">
            <span
              className={cn(
                'border-base-border/50 bg-base-elevated/70 inline-flex items-center rounded-md border px-2 py-1 font-mono text-[10px] uppercase tracking-[0.18em]',
                badge.colorClass
              )}
            >
              {badge.icon} {badge.label}
            </span>
            {classified.primaryLockCall && <LockCallBadge lc={classified.primaryLockCall} />}
          </div>

          <div className="flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
            <h2 className="text-text-bright font-mono text-base font-semibold tracking-tight sm:text-[17px]">
              {headline}
            </h2>
          </div>

          {subjectLine && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">{subjectLine}</div>
          )}

          <div className="flex flex-wrap items-center gap-2 pt-0.5">
            <span className="text-text-dim font-mono text-[10px] uppercase tracking-[0.18em]">
              Owner
            </span>
            <AddressLink address={activity.address} className="text-[11px]" />
            <Link href={txHref} className={metaChipClass}>
              tx {truncateHash(activity.txHash, 8, 6)}
            </Link>
            <Link href={blockHref} className={metaChipClass}>
              #{activity.blockNumber.toLocaleString()}
            </Link>
            <MetaChip>{formatActivityTimeAgo(activity.timestamp)}</MetaChip>
          </div>
        </div>

        <div className="flex items-start justify-start pt-0.5 lg:justify-end">
          <div className="border-base-border/40 bg-base-bg/50 min-w-[10.5rem] rounded-lg border px-3 py-2">
            {renderPrimaryValue(classified)}
          </div>
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
