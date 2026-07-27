'use client';

import { type MouseEvent, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useRouter } from '@/src/navigation';
import { api, type ActivitySummary24h } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { formatCkbCompact, cn } from '@/lib/utils';
import { PieChart } from '@/components/ui/pie-chart';
import {
  ACTIVITY_TYPE_COLORS,
  CHART_PRIMARY_COLOR,
  getChartPaletteColor,
} from '@/lib/chart-colors';
import Link from '@/components/ui/link';
import { getScriptDetailHref } from '@/lib/detail-routes';

function formatCompact(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return n.toString();
}

function buildPieData(stats: ActivitySummary24h) {
  return [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_TYPE_COLORS.Transfer },
    {
      label: 'DAO Deposit',
      value: stats.daoDepositCount,
      color: ACTIVITY_TYPE_COLORS['DAO Deposit'],
    },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_TYPE_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_TYPE_COLORS.Token },
    { label: 'Object', value: stats.objectCount, color: ACTIVITY_TYPE_COLORS.Object },
    { label: 'Identity', value: stats.identityCount, color: ACTIVITY_TYPE_COLORS.Identity },
    {
      label: 'Script Call',
      value: stats.scriptCallCount,
      color: ACTIVITY_TYPE_COLORS['Script Call'],
    },
  ]
    .filter((s) => s.value > 0)
    .sort((a, b) => b.value - a.value);
}

// ---------------------------------------------------------------------------
// ActivityBarChartCard — standalone 14-day bar chart
// ---------------------------------------------------------------------------

export function ActivityBarChartCard() {
  const [hoveredBarIdx, setHoveredBarIdx] = useState<number | null>(null);

  const { data: dailyStats, isLoading } = useQuery({
    queryKey: ['daily-activity-stats', 30],
    queryFn: () => api.getDailyActivityStats(30),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const barData =
    dailyStats?.map((d) => ({
      date: d.date,
      total:
        d.transferCount +
        d.daoDepositCount +
        d.daoWithdrawRequestCount +
        d.daoWithdrawCompleteCount +
        d.tokenCount +
        d.objectCount +
        d.identityCount +
        d.scriptCallCount,
    })) ?? [];

  const maxVal = Math.max(...barData.map((d) => d.total), 1);

  return (
    <div className="border-base-border bg-base-surface rounded-lg border px-4 py-3">
      <div className="mb-1.5 flex items-baseline justify-between">
        <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
          Activity — 30 Day Trend
        </div>
        {hoveredBarIdx !== null && barData[hoveredBarIdx] && (
          <div className="font-mono text-[10px] tabular-nums">
            <span className="text-text-dim">{barData[hoveredBarIdx].date}</span>{' '}
            <span className="text-jade font-bold">
              {barData[hoveredBarIdx].total.toLocaleString()}
            </span>
          </div>
        )}
      </div>
      {isLoading || barData.length === 0 ? (
        <div className="bg-base-elevated h-14 w-full animate-pulse rounded" />
      ) : (
        <div className="flex h-14 items-end gap-[1px]" onMouseLeave={() => setHoveredBarIdx(null)}>
          {barData.map((d, i) => (
            <div
              key={d.date}
              className="flex-1 cursor-crosshair rounded-t-sm transition-opacity duration-100"
              style={{
                height: `${Math.max((d.total / maxVal) * 100, 4)}%`,
                backgroundColor: CHART_PRIMARY_COLOR,
                opacity: hoveredBarIdx !== null && hoveredBarIdx !== i ? 0.3 : 0.8,
              }}
              onMouseEnter={() => setHoveredBarIdx(i)}
              title={`${d.date}: ${d.total.toLocaleString()}`}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// PieSection — reusable pie chart + side legend
// ---------------------------------------------------------------------------

function PieSection<T extends { label: string; value: number; color?: string }>({
  title,
  data,
  highlightIndex,
  onHighlightChange,
  useExplicitColors,
  sectionHref,
  getItemHref,
  testIdPrefix,
}: {
  title: string;
  data: T[];
  highlightIndex: number | null;
  onHighlightChange: (idx: number | null) => void;
  useExplicitColors?: boolean;
  sectionHref?: string;
  getItemHref?: (item: T, index: number) => string | null | undefined;
  testIdPrefix?: string;
}) {
  // Network-aware router: raw `useNavigate` would push un-prefixed hrefs, which
  // the route guard resolves against the DEFAULT network — dropping the user out
  // of the network they are browsing.
  const router = useRouter();
  const total = data.reduce((s, x) => s + x.value, 0);
  const isSectionClickable = Boolean(sectionHref);

  function handleSectionClick() {
    if (!sectionHref) return;
    router.push(sectionHref);
  }

  function handleItemClick(event: MouseEvent<Element>, item: T, index: number) {
    const href = getItemHref?.(item, index);
    if (!href) return;
    event.stopPropagation();
    router.push(href);
  }

  return (
    <div
      data-testid={testIdPrefix ? `${testIdPrefix}-section` : undefined}
      className={cn(
        'flex min-h-0 flex-1 items-center gap-3 lg:gap-4',
        isSectionClickable ? 'cursor-pointer' : ''
      )}
      onClick={handleSectionClick}
    >
      <div
        data-testid={testIdPrefix ? `${testIdPrefix}-chart-rail` : undefined}
        className="flex shrink-0 basis-[clamp(13rem,42%,17rem)] justify-center xl:basis-[clamp(12rem,40%,16rem)] 2xl:basis-[clamp(11.5rem,38%,15rem)]"
      >
        <PieChart
          data={data}
          fullWidth
          chartClassName="w-full max-w-[17rem] xl:max-w-[16rem] 2xl:max-w-[15rem]"
          showLegend={false}
          highlightIndex={highlightIndex}
          onHighlightChange={onHighlightChange}
          onSliceClick={(index, event) => handleItemClick(event, data[index], index)}
          testIdPrefix={testIdPrefix}
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col justify-center self-stretch overflow-hidden">
        <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider lg:text-[11px]">
          {title}
        </div>
        {data.map((d, i) => {
          const pct = total > 0 ? ((d.value / total) * 100).toFixed(1) : '0';
          return (
            <div
              key={d.label}
              data-testid={testIdPrefix ? `${testIdPrefix}-legend-item-${i}` : undefined}
              className={cn(
                'flex cursor-pointer items-center gap-1.5 py-0.5 font-mono text-[10px] leading-tight transition-opacity lg:gap-2 lg:py-1 lg:text-[11px]',
                highlightIndex !== null && highlightIndex !== i ? 'opacity-40' : ''
              )}
              onMouseEnter={() => onHighlightChange(i)}
              onMouseLeave={() => onHighlightChange(null)}
              onClick={(event) => handleItemClick(event, d, i)}
            >
              <div
                className="h-1.5 w-1.5 shrink-0 rounded-sm lg:h-2 lg:w-2"
                style={{
                  backgroundColor: useExplicitColors ? d.color : getChartPaletteColor(i),
                }}
              />
              <span className="text-text-dim min-w-0 truncate">{d.label}</span>
              <span className="text-text-bright shrink-0 tabular-nums">{pct}%</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// ActivityCard — activity types pie + script usage pie + 24h stats
// ---------------------------------------------------------------------------

interface ActivityCardProps {
  isRealtime?: boolean;
}

export function ActivityCard({ isRealtime = false }: ActivityCardProps) {
  const [hoveredActivityIdx, setHoveredActivityIdx] = useState<number | null>(null);
  const [hoveredScriptIdx, setHoveredScriptIdx] = useState<number | null>(null);

  const { data: summary, isLoading } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
  });

  const activityPieData = summary ? buildPieData(summary) : [];

  const scriptPieData = useMemo(() => {
    if (!summary?.scriptCounts) return [];
    return summary.scriptCounts
      .filter((s) => s.count > 0)
      .sort((a, b) => b.count - a.count)
      .slice(0, 8)
      .map((s) => ({
        label: s.name || `${s.codeHash.slice(0, 10)}...`,
        value: s.count,
        codeHash: s.codeHash,
        name: s.name,
      }));
  }, [summary]);

  const headerActions = (
    <Link
      href="/charts"
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
    >
      VIEW ALL &rarr;
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime} className="flex flex-col lg:h-[38rem]">
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        <Link href="/charts" className="hover:text-jade transition-colors">
          Activity Stats (24h)
        </Link>
      </TerminalPanelHeader>
      <TerminalPanelContent
        padding="md"
        className="flex min-h-0 flex-1 flex-col items-center justify-center"
      >
        {isLoading ? (
          <div className="space-y-4">
            <div className="bg-base-elevated h-6 w-full animate-pulse rounded" />
            <div className="bg-base-elevated h-36 w-full animate-pulse rounded" />
            <div className="bg-base-elevated h-36 w-full animate-pulse rounded" />
          </div>
        ) : (
          <div className="flex flex-col gap-4">
            {/* 24h stats — top */}
            <div className="border-base-border/40 divide-base-border/40 flex items-stretch divide-x border-b pb-3">
              <StatItem
                label="Activities"
                value={
                  summary
                    ? formatCompact(
                        summary.transferCount +
                          summary.daoDepositCount +
                          summary.daoWithdrawRequestCount +
                          summary.daoWithdrawCompleteCount +
                          summary.tokenCount +
                          summary.objectCount +
                          summary.identityCount +
                          summary.scriptCallCount
                      )
                    : '\u2014'
                }
              />
              <StatItem
                label="Addresses"
                value={summary ? summary.uniqueAddressCount.toLocaleString() : '\u2014'}
              />
              <StatItem
                label="Volume"
                value={summary ? formatCkbCompact(summary.totalCkbMoved).value + ' CKB' : '\u2014'}
              />
            </div>

            <div className="flex flex-col gap-4">
              {/* Activity types pie */}
              {activityPieData.length > 0 && (
                <PieSection
                  title="Activity Types"
                  data={activityPieData}
                  highlightIndex={hoveredActivityIdx}
                  onHighlightChange={setHoveredActivityIdx}
                  useExplicitColors
                  sectionHref="/charts/activity-type-breakdown"
                  getItemHref={() => '/charts/activity-type-breakdown'}
                  testIdPrefix="activity-types"
                />
              )}

              {/* Script usage pie */}
              {scriptPieData.length > 0 && (
                <PieSection
                  title="Script Usage"
                  data={scriptPieData}
                  highlightIndex={hoveredScriptIdx}
                  onHighlightChange={setHoveredScriptIdx}
                  sectionHref="/charts/most-utilized-scripts"
                  getItemHref={(item) =>
                    getScriptDetailHref({
                      name: item.name,
                      codeHash: item.codeHash,
                    })
                  }
                  testIdPrefix="script-usage"
                />
              )}
            </div>
          </div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function StatItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex-1 text-center">
      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">{label}</div>
      <div className="text-jade mt-1 font-mono text-base font-bold tabular-nums">{value}</div>
    </div>
  );
}
