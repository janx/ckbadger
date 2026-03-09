'use client';

import { useQuery } from '@tanstack/react-query';
import { api, ChartDataPoint, NetworkStats } from '@/lib/api';
import { useMemo } from 'react';
import Link from '@/components/ui/link';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR, CHART_GRID_COLOR } from '@/lib/chart-colors';

interface MiniChartProps {
  data: ChartDataPoint[];
  color?: string;
}

function MiniLineChart({ data, color = CHART_PRIMARY_COLOR }: MiniChartProps) {
  const width = 500;
  const height = 120;
  const padding = { top: 10, right: 10, bottom: 25, left: 45 };
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;

  const { minVal, maxVal, pathD, yTicks, xTicks } = useMemo(() => {
    if (!data.length) return { minVal: 0, maxVal: 1, pathD: '', yTicks: [], xTicks: [] };

    const vals = data.map((d) => parseFloat(d.value) || 0);
    let min = Math.min(...vals);
    let max = Math.max(...vals);

    const range = max - min;
    min = min - range * 0.1;
    max = max + range * 0.1;

    if (min === max) {
      min = min * 0.9;
      max = max * 1.1;
    }

    const xScale = (i: number) => padding.left + (i / (data.length - 1 || 1)) * chartWidth;
    const yScale = (v: number) =>
      padding.top + chartHeight - ((v - min) / (max - min || 1)) * chartHeight;

    const parts: string[] = [];
    for (let i = 0; i < data.length; i++) {
      parts.push(`${i === 0 ? 'M' : 'L'} ${xScale(i)} ${yScale(vals[i])}`);
    }

    const ticks = Array.from({ length: 4 }, (_, i) => min + ((max - min) / 3) * i);
    const xTickCount = Math.min(6, data.length);
    const xTickIndices = Array.from({ length: xTickCount }, (_, i) =>
      Math.floor((i / (xTickCount - 1 || 1)) * (data.length - 1))
    );

    return {
      minVal: min,
      maxVal: max,
      pathD: parts.join(' '),
      yTicks: ticks,
      xTicks: xTickIndices.map((idx) => ({ idx, x: xScale(idx), label: data[idx]?.date || '' })),
    };
  }, [data, chartWidth, chartHeight, padding.left, padding.top]);

  const yScale = (v: number) =>
    padding.top + chartHeight - ((v - minVal) / (maxVal - minVal || 1)) * chartHeight;

  const formatYTick = (val: number) => {
    if (val >= 1_000_000_000_000_000) return `${(val / 1_000_000_000_000_000).toFixed(0)}P`;
    if (val >= 1_000_000_000_000) return `${(val / 1_000_000_000_000).toFixed(0)}T`;
    if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(0)}G`;
    if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(0)}M`;
    if (val >= 1_000) return `${(val / 1_000).toFixed(0)}K`;
    return val.toFixed(1);
  };

  const formatXLabel = (label: string) => {
    const parts = label.split('/');
    if (parts.length >= 3) {
      return `${parts[1]}/${parts[2]}`;
    }
    return label;
  };

  if (!data.length) {
    return <div className="text-text-muted flex h-32 items-center justify-center">No data</div>;
  }

  return (
    <svg viewBox={`0 0 ${width} ${height}`} className="w-full">
      {yTicks.map((tick, i) => (
        <g key={`y-${i}`}>
          <line
            x1={padding.left}
            x2={width - padding.right}
            y1={yScale(tick)}
            y2={yScale(tick)}
            stroke={CHART_GRID_COLOR}
            strokeDasharray="2,2"
          />
          <text
            x={padding.left - 5}
            y={yScale(tick)}
            textAnchor="end"
            dominantBaseline="middle"
            className="fill-text-muted font-mono tabular-nums"
            fontSize="10"
          >
            {formatYTick(tick)}
          </text>
        </g>
      ))}

      {xTicks.map(({ idx, x, label }) => (
        <text
          key={`x-${idx}`}
          x={x}
          y={height - 5}
          textAnchor="middle"
          className="fill-text-muted font-mono tabular-nums"
          fontSize="10"
        >
          {formatXLabel(label)}
        </text>
      ))}

      <path d={pathD} fill="none" stroke={color} strokeWidth="1.5" />
    </svg>
  );
}

interface ChartCardProps {
  leftLabel: string;
  leftValue: string;
  rightLabel: string;
  rightValue: string;
  chartTitle: string;
  data: ChartDataPoint[];
  isLoading?: boolean;
  href?: string;
  chartColor?: string;
}

function ChartCard({
  leftLabel,
  leftValue,
  rightLabel,
  rightValue,
  chartTitle,
  data,
  isLoading,
  href,
  chartColor = CHART_PRIMARY_COLOR,
}: ChartCardProps) {
  const content = (
    <div
      className={`border-base-border bg-base-surface rounded-lg border p-4 ${href ? 'hover:border-base-border cursor-pointer transition-colors' : ''}`}
    >
      <div className="mb-3 flex items-start justify-between">
        <div>
          <div className="text-text-muted font-mono text-xs uppercase tracking-wider">
            {leftLabel}
          </div>
          <div
            className={`text-emphasis font-mono text-xl font-bold tabular-nums ${isLoading ? 'animate-pulse' : ''}`}
          >
            {leftValue}
          </div>
        </div>
        <div className="text-right">
          <div className="text-text-muted font-mono text-xs uppercase tracking-wider">
            {rightLabel}
          </div>
          <div
            className={`text-warning font-mono text-xl font-bold tabular-nums ${isLoading ? 'animate-pulse' : ''}`}
          >
            {rightValue}
          </div>
        </div>
      </div>
      <div className="text-text-muted mb-1 font-mono text-xs">{chartTitle}</div>
      <MiniLineChart data={data} color={chartColor} />
    </div>
  );

  if (href) {
    return <Link href={href}>{content}</Link>;
  }

  return content;
}

interface HomeChartsProps {
  stats?: NetworkStats;
  isLoading?: boolean;
  initialBlockTimeChart?: ChartResponse | null;
  initialHashRateChart?: ChartResponse | null;
}

interface ChartResponse {
  data: ChartDataPoint[];
  title: string;
  yAxisLabel: string;
  y2AxisLabel?: string;
}

export function HomeCharts({
  stats,
  isLoading: statsLoading,
  initialBlockTimeChart,
  initialHashRateChart,
}: HomeChartsProps) {
  const { data: blockTimeChart, isLoading: blockTimeLoading } = useQuery({
    queryKey: ['chart-average-block-time-home'],
    queryFn: () => api.getAverageBlockTimeChart(),
    initialData: initialBlockTimeChart ?? undefined,
    staleTime: 60000,
    refetchInterval: 300000,
  });

  const { data: hashRateChart, isLoading: hashRateLoading } = useQuery({
    queryKey: ['chart-hash-rate-home'],
    queryFn: () => api.getHashRateChart(),
    initialData: initialHashRateChart ?? undefined,
    staleTime: 60000,
    refetchInterval: 300000,
  });

  const recentBlockTimeData = useMemo(() => {
    if (!blockTimeChart?.data) return [];
    return blockTimeChart.data.slice(-14);
  }, [blockTimeChart]);

  const recentHashRateData = useMemo(() => {
    if (!hashRateChart?.data) return [];
    return hashRateChart.data.slice(-14);
  }, [hashRateChart]);

  return (
    <div className="grid gap-3 lg:grid-cols-2">
      <ChartCard
        leftLabel="Latest Block"
        leftValue={stats?.latestBlock?.toLocaleString() ?? '-'}
        rightLabel="Average Block Time"
        rightValue={stats?.avgBlockTime ?? '-'}
        chartTitle="Average Block Time (s)"
        data={recentBlockTimeData}
        isLoading={statsLoading || blockTimeLoading}
        href="/charts/average-block-time"
        chartColor={CHART_PRIMARY_COLOR}
      />
      <ChartCard
        leftLabel="Mining Hash Rate"
        leftValue={stats?.hashRate ?? '-'}
        rightLabel="Mining Difficulty"
        rightValue={stats?.difficulty ?? '-'}
        chartTitle="Hash Rate (H/s)"
        data={recentHashRateData}
        isLoading={statsLoading || hashRateLoading}
        href="/charts/hash-rate"
        chartColor={CHART_SECONDARY_COLOR}
      />
    </div>
  );
}
