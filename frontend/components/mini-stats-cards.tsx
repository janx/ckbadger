'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, TxStatsDataPoint } from '@/lib/api';
import { cn } from '@/lib/utils';

interface MiniStatsCardsProps {
  className?: string;
}

function formatNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toLocaleString();
}

interface BarChartProps {
  data: TxStatsDataPoint[];
  color: string;
  height?: number;
}

function BarChart({ data, color, height = 48 }: BarChartProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);

  if (!data.length) {
    return (
      <div className="flex items-center justify-center text-slate-600" style={{ height }}>
        No data
      </div>
    );
  }

  const maxValue = Math.max(...data.map((d) => d.value), 1);
  const barColor = color === 'emerald' ? 'bg-emerald-500' : 'bg-amber-500';
  const barColorHover = color === 'emerald' ? 'bg-emerald-400' : 'bg-amber-400';

  return (
    <div className="relative" style={{ height }}>
      <div className="flex h-full items-end gap-[2px]">
        {data.map((point, i) => {
          const barHeight = (point.value / maxValue) * 100;
          const isHovered = hoveredIndex === i;

          return (
            <div
              key={i}
              className="relative h-full flex-1"
              onMouseEnter={() => setHoveredIndex(i)}
              onMouseLeave={() => setHoveredIndex(null)}
            >
              <div
                className={cn(
                  'absolute bottom-0 w-full rounded-t-sm transition-all duration-150',
                  isHovered ? barColorHover : barColor,
                  isHovered ? 'opacity-100' : 'opacity-70'
                )}
                style={{ height: `${Math.max(barHeight, 2)}%` }}
              />
              {isHovered && (
                <div className="absolute bottom-full left-1/2 z-10 mb-2 -translate-x-1/2 whitespace-nowrap rounded bg-slate-800 px-2 py-1 text-xs shadow-lg">
                  <div className="font-mono text-slate-300">{point.label}</div>
                  <div
                    className={cn(
                      'font-mono font-bold',
                      color === 'emerald' ? 'text-emerald-400' : 'text-amber-400'
                    )}
                  >
                    {formatNumber(point.value)}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

interface TxStatWidgetProps {
  label: string;
  value: number;
  data: TxStatsDataPoint[];
  color: string;
}

function TxStatWidget({ label, value, data, color }: TxStatWidgetProps) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between">
        <span className="font-mono text-xs uppercase tracking-wider text-slate-500">{label}</span>
        <span
          className={cn(
            'font-mono text-xl font-bold tabular-nums',
            color === 'emerald' ? 'text-terminal-green' : 'text-amber'
          )}
        >
          {formatNumber(value)}
        </span>
      </div>
      <BarChart data={data} color={color} height={48} />
      <div className="flex justify-between font-mono text-[10px] text-slate-600">
        <span>{data.length > 0 ? data[0].label : ''}</span>
        <span>{data.length > 0 ? data[data.length - 1].label : ''}</span>
      </div>
    </div>
  );
}

export function MiniStatsCards({ className }: MiniStatsCardsProps) {
  const { data: txStats } = useQuery({
    queryKey: ['tx-stats'],
    queryFn: () => api.getTxStats(),
    staleTime: 10000,
    refetchInterval: 30000,
  });

  const hourlyData = txStats?.hourlyData ?? [];
  const dailyData = txStats?.dailyData ?? [];
  const txsLastHour = txStats?.currentHour ?? 0;
  const txsLast24Hours = txStats?.currentDay ?? 0;

  return (
    <div className={cn('h-full rounded-lg border border-slate-800 bg-slate-900 p-4', className)}>
      <div className="grid h-full grid-cols-2 gap-6">
        <TxStatWidget
          label="TXs Last 60 Mins"
          value={txsLastHour}
          data={hourlyData}
          color="emerald"
        />
        <TxStatWidget
          label="TXs Last 24 Hours"
          value={txsLast24Hours}
          data={dailyData}
          color="amber"
        />
      </div>
    </div>
  );
}
