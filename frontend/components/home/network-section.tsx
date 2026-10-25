'use client';

import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  BarChart,
  ChartFigure,
  LineChart,
  type Bar,
  type SeriesPoint,
} from '@/components/home/mini-chart';
import {
  SECTION_HEADING_CLASS,
  SMALL_MULTIPLES_CLASS,
  bodyOf,
  derive,
  viewOf,
} from '@/components/home/section';
import { api, type ChartResponse, type TxStatsDataPoint } from '@/lib/api';
import { chartNumber, dayLabel, splitByCompleteness } from '@/lib/daily-series';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import {
  exactHashRateUnit,
  formatPercentShort,
  orphanRateTargetPercent,
} from '@/lib/network-metrics';
import { cn } from '@/lib/utils';

/** Daily charts show their last 14 points: complete days, by the endpoints' contract. */
const DAYS = 14;

/** The Network section: four same-size small multiples, one data hue. */
export function NetworkSection() {
  return (
    <section aria-labelledby="home-network-heading" className="flex flex-col gap-[18px]">
      <h2 id="home-network-heading" className={SECTION_HEADING_CLASS}>
        Network
      </h2>
      <div className={SMALL_MULTIPLES_CLASS}>
        <BlockTimeFigure />
        <HashRateFigure />
        <UncleRateFigure />
        <TransactionsFigure />
      </div>
    </section>
  );
}

interface LineView {
  points: SeriesPoint[];
  min: number;
  max: number;
  last: SeriesPoint;
}

/** The last 14 days of a daily chart, scaled for display. */
function lineView(chart: ChartResponse, scale: (v: number) => number): LineView | null {
  const points = chart.data.slice(-DAYS).map((p) => ({
    label: dayLabel(p.date),
    value: scale(chartNumber(p)),
  }));
  if (points.length === 0) return null;
  const values = points.map((p) => p.value);
  return {
    points,
    min: Math.min(...values),
    max: Math.max(...values),
    last: points[points.length - 1],
  };
}

const range = (v: LineView) => `${v.points[0].label} to ${v.last.label}`;

function BlockTimeFigure() {
  const { data, error } = useQuery({
    queryKey: ['chart-average-block-time-home'],
    queryFn: () => api.getAverageBlockTimeChart(),
    staleTime: 60_000,
    refetchInterval: 300_000,
  });
  const d = useMemo(
    () =>
      derive({ data, error }, (chart) => lineView(chart, (v) => v), 'No block-time history yet'),
    [data, error]
  );
  const v = viewOf(d);
  const s = (n: number) => n.toFixed(2);
  return (
    <ChartFigure
      testId="home-block-time"
      title="Block time"
      href="/charts/average-block-time"
      subtitle="daily average, s"
      headline={v ? s(v.last.value) : undefined}
      body={bodyOf(d, (view) => (
        <LineChart
          points={view.points}
          formatValue={s}
          ariaLabel={`Average block time per day, ${range(view)}, between ${s(view.min)} and ${s(view.max)} seconds, ${s(view.last.value)} s on ${view.last.label}`}
        />
      ))}
    />
  );
}

function HashRateFigure() {
  const { data, error } = useQuery({
    queryKey: ['chart-hash-rate-home'],
    queryFn: () => api.getHashRateChart(),
    staleTime: 60_000,
    refetchInterval: 300_000,
  });
  // The values are H/s; the unit only divides them for display.
  const d = useMemo(
    () =>
      derive(
        { data, error },
        (chart) => {
          const raw = chart.data
            .slice(-DAYS)
            .map((p) => exactUnsignedInteger(p.value, `hash rate ${p.date}`));
          if (raw.length === 0) return null;
          const unit = exactHashRateUnit(
            raw.reduce((max, value) => (value > max ? value : max), BigInt(0))
          );
          const view = lineView(chart, (hps) => hps / Number(unit.divisor));
          return (
            view && {
              ...view,
              points: view.points.map((point, index) => ({
                ...point,
                display: formatScaledInteger(raw[index], unit.divisor, 2),
              })),
              unit: unit.suffix,
              lastExact: formatScaledInteger(raw[raw.length - 1], unit.divisor, 2),
              minExact: formatScaledInteger(
                raw.reduce((min, value) => (value < min ? value : min)),
                unit.divisor,
                2
              ),
              maxExact: formatScaledInteger(
                raw.reduce((max, value) => (value > max ? value : max)),
                unit.divisor,
                2
              ),
            }
          );
        },
        'No hash-rate history yet'
      ),
    [data, error]
  );
  const v = viewOf(d);
  const s = (n: number) => n.toFixed(2);
  return (
    <ChartFigure
      testId="home-hash-rate"
      title="Hash rate"
      href="/charts/hash-rate"
      subtitle={v ? `daily, ${v.unit}` : 'daily'}
      headline={v ? v.lastExact : undefined}
      body={bodyOf(d, (view) => (
        <LineChart
          points={view.points}
          formatValue={s}
          ariaLabel={`Hash rate per day, ${range(view)}, between ${view.minExact} and ${view.maxExact} ${view.unit}, ${view.lastExact} ${view.unit} on ${view.last.label}`}
        />
      ))}
    />
  );
}

function UncleRateFigure() {
  const { data, error } = useQuery({
    queryKey: ['chart-uncle-rate-home'],
    queryFn: () => api.getUncleRateChart(),
    staleTime: 60_000,
    refetchInterval: 300_000,
  });
  const d = useMemo(
    () =>
      derive(
        { data, error },
        (chart) => {
          // The target is a consensus parameter the API reports; without it there is no chart.
          const target = orphanRateTargetPercent(chart.orphanRateTarget);
          const view = lineView(chart, (rate) => rate * 100);
          return view && { ...view, target };
        },
        'No uncle-rate history yet'
      ),
    [data, error]
  );
  const v = viewOf(d);
  const s = (n: number) => n.toFixed(2);
  return (
    <ChartFigure
      testId="home-uncle-rate"
      title="Uncle rate"
      href="/charts/uncle-rate"
      subtitle={v ? `daily, NC-Max target ${formatPercentShort(v.target)}%` : 'daily'}
      headline={v ? `${s(v.last.value)}%` : undefined}
      body={bodyOf(d, (view) => (
        <LineChart
          points={view.points}
          reference={view.target}
          formatValue={(n) => `${s(n)}%`}
          ariaLabel={`Uncle rate per day, ${range(view)}, between ${s(view.min)} and ${s(view.max)} percent around the NC-Max target of ${formatPercentShort(view.target)} percent, ${s(view.last.value)} percent on ${view.last.label}`}
        />
      ))}
    />
  );
}

type TxRange = '24h' | '14d';

// The toggle names the range, so the sub-label names only the bucket.
const TX_RANGES: Record<TxRange, { button: string; subtitle: string; series: string }> = {
  '24h': { button: '24 h', subtitle: 'per hour', series: 'hourlyData' },
  '14d': { button: '14 d', subtitle: 'per day', series: 'dailyData' },
};

interface TxView {
  bars: Bar[];
  /** The last complete bucket: the headline. */
  last: TxStatsDataPoint | null;
  min: number;
  max: number;
}

/** The bars of a tx-stats series, from its `complete` flags; null for an empty series. */
function txView(buckets: TxStatsDataPoint[], series: string): TxView | null {
  buckets.forEach((b) => {
    if (!Number.isInteger(b.value) || b.value < 0) {
      throw new Error(`${series} bucket ${b.label} has ${b.value} transactions`);
    }
  });
  const { complete, inProgress } = splitByCompleteness(buckets, series);
  if (complete.length === 0 && inProgress.length === 0) return null;
  const bars: Bar[] = [
    ...complete.map((b, i) => ({
      label: b.label,
      value: b.value,
      state: i === complete.length - 1 ? ('highlight' as const) : ('complete' as const),
    })),
    ...inProgress.map((b) => ({ label: b.label, value: b.value, state: 'inProgress' as const })),
  ];
  const values = complete.map((b) => b.value);
  return {
    bars,
    last: complete.length > 0 ? complete[complete.length - 1] : null,
    min: values.length > 0 ? Math.min(...values) : 0,
    max: values.length > 0 ? Math.max(...values) : 0,
  };
}

/** Transactions: one chart, one time scale at a time — hourly over 24 h or daily over 14 d. */
function TransactionsFigure() {
  const [txRange, setTxRange] = useState<TxRange>('24h');
  const { data, error } = useQuery({
    queryKey: ['tx-stats'],
    queryFn: () => api.getTxStats(),
    staleTime: 10_000,
    refetchInterval: 30_000,
  });
  const meta = TX_RANGES[txRange];
  const d = useMemo(
    () =>
      derive(
        { data, error },
        (stats) => txView(txRange === '24h' ? stats.hourlyData : stats.dailyData, meta.series),
        'No transactions yet'
      ),
    [data, error, txRange, meta.series]
  );
  const v = viewOf(d);
  const n = (x: number) => x.toLocaleString('en-US');
  const ariaLabel = (view: TxView) => {
    const last = view.last ? n(view.last.value) : 'none';
    const open = view.bars.filter((b) => b.state === 'inProgress').length;
    const unit = txRange === '24h' ? 'hour' : 'day';
    const hollow =
      open === 1
        ? `the current ${unit} drawn hollow`
        : `the ${open} ${unit}s still in progress drawn hollow`;
    return txRange === '24h'
      ? `Transactions per hour over the last 24 hours, ${n(view.min)} to ${n(view.max)} per hour, ${last} in the last full hour, ${hollow}`
      : `Transactions per day, ${view.bars[0].label} to ${view.bars[view.bars.length - 1].label}, ${n(view.min)} to ${n(view.max)} per day, ${last} on the last full day, ${hollow}`;
  };
  return (
    <ChartFigure
      testId="home-transactions"
      title="Transactions"
      href="/charts/transaction-count"
      subtitle={meta.subtitle}
      headline={v ? (v.last ? n(v.last.value) : '—') : undefined}
      controls={
        <span
          role="group"
          aria-label="Transactions range"
          className="border-base-border flex overflow-hidden rounded-md border"
        >
          {(Object.keys(TX_RANGES) as TxRange[]).map((r) => (
            <button
              key={r}
              type="button"
              aria-pressed={txRange === r}
              onClick={() => setTxRange(r)}
              className={cn(
                'h-8 whitespace-nowrap px-[9px] text-xs transition-colors',
                txRange === r
                  ? 'bg-base-border-subtle text-text-bright'
                  : 'text-text-dim hover:text-text bg-transparent'
              )}
            >
              {TX_RANGES[r].button}
            </button>
          ))}
        </span>
      }
      body={bodyOf(d, (view) => (
        <BarChart bars={view.bars} formatValue={n} ariaLabel={ariaLabel(view)} />
      ))}
    />
  );
}
