'use client';

import { useMemo } from 'react';
import { cn } from '@/lib/utils';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';

interface SparkChartProps {
  data: number[];
  color?: string;
  height?: number;
  className?: string;
}

export function SparkChart({
  data,
  color = CHART_PRIMARY_COLOR,
  height = 40,
  className,
}: SparkChartProps) {
  const pathD = useMemo(() => {
    if (!data.length) return '';

    const min = Math.min(...data);
    const max = Math.max(...data);
    const range = max - min || 1;

    const width = 100;
    const padding = 2;
    const chartHeight = height - padding * 2;
    const stepX = width / (data.length - 1 || 1);

    const points = data.map((val, i) => {
      const x = i * stepX;
      const y = padding + chartHeight - ((val - min) / range) * chartHeight;
      return `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${y.toFixed(1)}`;
    });

    return points.join(' ');
  }, [data, height]);

  const areaPathD = useMemo(() => {
    if (!pathD) return '';
    return `${pathD} L 100 ${height - 2} L 0 ${height - 2} Z`;
  }, [pathD, height]);

  if (!data.length) {
    return (
      <div
        className={cn('text-text-dim flex items-center justify-center', className)}
        style={{ height }}
      >
        No data
      </div>
    );
  }

  return (
    <svg
      viewBox={`0 0 100 ${height}`}
      className={cn('w-full', className)}
      preserveAspectRatio="none"
    >
      <defs>
        <linearGradient id={`spark-gradient-${color.replace('#', '')}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.3" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={areaPathD} fill={`url(#spark-gradient-${color.replace('#', '')})`} />
      <path d={pathD} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}
