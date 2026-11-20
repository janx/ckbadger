'use client';

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { BarChart, ChartFigure, type Bar } from '@/components/home/mini-chart';
import { RankedBars, type RankedRow } from '@/components/home/ranked-bars';
import {
  ACTIVITY_HEAD_CLASS,
  SectionHeading,
  bodyOf,
  derive,
  viewOf,
} from '@/components/home/section';
import { activityTypeShares, scriptShares } from '@/lib/activity-stats';
import {
  api,
  dayUserTransactions,
  type ActivitySummary24h,
  type DailyActivityStats,
} from '@/lib/api';
import { dayLabel } from '@/lib/daily-series';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { usePhosphor } from '@/hooks/usePhosphor';
import { cn } from '@/lib/utils';
import { ckbAmountText, ckbShortenedTitle } from '@/lib/amount-form';
import { parseShannons } from '@/lib/ckb-decimal';
import { countNoun, formatCount } from '@/lib/format-count';

const TYPE_BREAKDOWN = '/charts/activity-type-breakdown';

/** `activity --24 h`: three KPIs, ranked types and scripts, and the daily activity chart. */
export function ActivityStats() {
  const { data, error } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
  });
  const d = useMemo(() => derive({ data, error }, summaryView, 'No activity.'), [data, error]);

  return (
    <div data-testid="home-activity-stats" className="flex min-w-0 flex-col gap-[22px]">
      {/* Rubbing × Song print, phase 5 (H5): a command on its window, the charts its note; the
          row is the feed's heading row, so the two command lines sit on one line. */}
      <div className={ACTIVITY_HEAD_CLASS}>
        <SectionHeading
          command="activity"
          args="--24 h"
          note={
            <Link href="/charts" look="onward" className="text-small">
              charts
            </Link>
          }
        />
      </div>
      {d.state === 'loading' && (
        <div aria-hidden="true" className="bg-base-surface h-40 animate-pulse" />
      )}
      {d.state === 'error' && (
        <div role="alert" className="text-rouge text-small">
          {d.message}
        </div>
      )}
      {/* an empty-state line, the reading ink (BRIEF-ink-size-roles item 3) */}
      {d.state === 'empty' && <div className="text-small">{d.message}</div>}
      {d.state === 'ready' && <Summary view={d.view} />}
      <DailyActivityFigure />
    </div>
  );
}

interface SummaryView {
  activities: number;
  occurrences: number;
  addresses: number;
  /** CKB moved, compact (a tile, R5), and the exact value its title names when it was cut. */
  ckbMoved: { text: string; title: string | undefined };
  types: RankedRow[];
  scripts: RankedRow[];
  scriptCalls: number;
}

function summaryView(s: ActivitySummary24h): SummaryView {
  const scripts = scriptShares(s);
  const rows: RankedRow[] = scripts.top.map((r) => ({
    key: r.key,
    label: r.label,
    count: r.count,
    total: scripts.total,
    href: getScriptDetailHref({ name: r.name, codeHash: r.codeHash }),
  }));
  if (scripts.others) {
    rows.push({
      key: 'others',
      label: `${scripts.others.scripts} ${countNoun(scripts.others.scripts, 'other')}`,
      count: scripts.others.count,
      total: scripts.total,
    });
  }
  return {
    activities: s.activityCount,
    occurrences: s.kindOccurrences,
    addresses: s.uniqueAddressCount,
    ckbMoved: {
      text: ckbAmountText(parseShannons(s.totalCkbMoved, 'CKB moved'), 'compact'),
      title: ckbShortenedTitle(parseShannons(s.totalCkbMoved, 'CKB moved'), 'compact'),
    },
    types: activityTypeShares(s).map((r) => ({
      key: r.key,
      label: r.label,
      count: r.count,
      total: s.kindOccurrences,
      href: TYPE_BREAKDOWN,
    })),
    scripts: rows,
    scriptCalls: scripts.total,
  };
}

/** A 24 h count; it flares when the served figure changes (material pass v2, move 4). */
function Kpi({ label, title, children }: { label: string; title?: string; children: string }) {
  const flare = usePhosphor(children);
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-text-dim text-small">{label}</dt>
      <dd
        title={title}
        className={cn('text-text-bright text-hero m-0 font-medium tabular-nums', flare)}
      >
        {children}
      </dd>
    </div>
  );
}

function Summary({ view }: { view: SummaryView }) {
  const n = (x: number) => x.toLocaleString('en-US');
  return (
    <>
      <dl className="m-0 flex flex-wrap gap-x-11 gap-y-3">
        <Kpi label="Activities">{n(view.activities)}</Kpi>
        <Kpi label="Addresses">{n(view.addresses)}</Kpi>
        <Kpi label="CKB moved" title={view.ckbMoved.title}>
          {view.ckbMoved.text}
        </Kpi>
      </dl>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-x-10 gap-y-[22px]">
        <RankedBars
          testId="home-activity-types"
          title={`By type, share of ${n(view.occurrences)} kind occurrences`}
          titleHref={TYPE_BREAKDOWN}
          rows={view.types}
        />
        <RankedBars
          testId="home-activity-scripts"
          title={`By script, share of ${formatCount(view.scriptCalls, 'call')}`}
          titleHref="/charts/most-utilized-scripts"
          rows={view.scripts}
          wideLabels
        />
      </div>
    </>
  );
}

interface DailyView {
  bars: Bar[];
  first: string;
  last: { label: string; value: number };
  min: number;
  max: number;
}

/**
 * Activities per day, oldest first; the last day is the headline. A day's bar is the route's count
 * of its user transactions (`userTransactionCount`). Days must be in order.
 */
export function dailyActivityView(days: DailyActivityStats[]): DailyView | null {
  if (days.length === 0) return null;
  for (let i = 1; i < days.length; i++) {
    if (days[i].date <= days[i - 1].date) {
      throw new Error(`daily activity days out of order: ${days[i - 1].date} then ${days[i].date}`);
    }
  }
  const totals = days.map(dayUserTransactions);
  const bars = days.map((day, i) => ({
    label: dayLabel(day.date),
    value: totals[i],
    state: i === days.length - 1 ? ('highlight' as const) : ('complete' as const),
  }));
  const last = bars[bars.length - 1];
  return {
    bars,
    first: bars[0].label,
    last: { label: last.label, value: last.value },
    min: Math.min(...totals),
    max: Math.max(...totals),
  };
}

function DailyActivityFigure() {
  const { data, error } = useQuery({
    queryKey: ['home-daily-activity', 30],
    queryFn: () => api.getDailyActivityStats(30),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const d = useMemo(
    () => derive({ data, error }, (days) => dailyActivityView(days), 'No daily activity.'),
    [data, error]
  );
  const v = viewOf(d);
  const n = (x: number) => x.toLocaleString('en-US');
  return (
    <ChartFigure
      testId="home-daily-activity"
      title="Daily activity"
      href="/charts/activity-volume"
      subtitle={v ? `activities per day, ${v.first} to ${v.last.label}` : 'activities per day'}
      headline={v ? n(v.last.value) : undefined}
      body={bodyOf(d, (view) => (
        <BarChart
          bars={view.bars}
          formatValue={n}
          ariaLabel={`Activities per day, ${view.first} to ${view.last.label}, between ${n(view.min)} and ${n(view.max)}, ${n(view.last.value)} on ${view.last.label}`}
        />
      ))}
    />
  );
}
