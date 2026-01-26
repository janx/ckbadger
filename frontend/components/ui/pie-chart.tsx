'use client';

import { useState, useMemo } from 'react';

interface PieChartDataPoint {
  label: string;
  value: number;
  color?: string;
}

interface PieChartProps {
  data: PieChartDataPoint[];
  size?: number;
  showLegend?: boolean;
  formatValue?: (value: number) => string;
}

const COLORS = [
  '#8b5cf6',
  '#00c389',
  '#f59e0b',
  '#ef4444',
  '#3b82f6',
  '#ec4899',
  '#14b8a6',
  '#f97316',
  '#6366f1',
  '#84cc16',
  '#a855f7',
  '#22d3ee',
];

export function PieChart({
  data,
  size = 300,
  showLegend = true,
  formatValue = (v) => v.toFixed(2) + '%',
}: PieChartProps) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const total = useMemo(() => data.reduce((sum, d) => sum + d.value, 0), [data]);

  const slices = useMemo(() => {
    let currentAngle = -90;
    return data.map((d, i) => {
      const percentage = total > 0 ? (d.value / total) * 100 : 0;
      const angle = (percentage / 100) * 360;
      const startAngle = currentAngle;
      const endAngle = currentAngle + angle;
      currentAngle = endAngle;

      const startRad = (startAngle * Math.PI) / 180;
      const endRad = (endAngle * Math.PI) / 180;

      const x1 = Math.cos(startRad);
      const y1 = Math.sin(startRad);
      const x2 = Math.cos(endRad);
      const y2 = Math.sin(endRad);

      const largeArc = angle > 180 ? 1 : 0;

      const pathD =
        percentage >= 100
          ? `M 0 -1 A 1 1 0 1 1 0 1 A 1 1 0 1 1 0 -1`
          : `M 0 0 L ${x1} ${y1} A 1 1 0 ${largeArc} 1 ${x2} ${y2} Z`;

      return {
        ...d,
        percentage,
        pathD,
        color: d.color || COLORS[i % COLORS.length],
        midAngle: startAngle + angle / 2,
      };
    });
  }, [data, total]);

  if (!data.length || total === 0) {
    return (
      <div
        className="flex items-center justify-center text-slate-500"
        style={{ width: size, height: size }}
      >
        No data available
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-6 lg:flex-row lg:items-start">
      <div className="relative" style={{ width: size, height: size }}>
        <svg viewBox="-1.2 -1.2 2.4 2.4" className="h-full w-full">
          {slices.map((slice, i) => (
            <path
              key={i}
              d={slice.pathD}
              fill={slice.color}
              stroke="#111827"
              strokeWidth={0.02}
              className="cursor-pointer transition-opacity"
              opacity={hoverIndex === null || hoverIndex === i ? 1 : 0.5}
              onMouseEnter={() => setHoverIndex(i)}
              onMouseLeave={() => setHoverIndex(null)}
              transform={hoverIndex === i ? `scale(1.05)` : undefined}
              style={{ transformOrigin: 'center' }}
            />
          ))}
          <circle cx={0} cy={0} r={0.6} fill="#111827" />
          {hoverIndex !== null && slices[hoverIndex] && (
            <>
              <text
                x={0}
                y={-0.1}
                textAnchor="middle"
                className="fill-white text-[0.12px] font-medium"
              >
                {slices[hoverIndex].label.length > 12
                  ? slices[hoverIndex].label.slice(0, 12) + '...'
                  : slices[hoverIndex].label}
              </text>
              <text
                x={0}
                y={0.15}
                textAnchor="middle"
                className="fill-slate-300 font-mono text-[0.14px] tabular-nums"
              >
                {formatValue(slices[hoverIndex].percentage)}
              </text>
            </>
          )}
          {hoverIndex === null && (
            <text x={0} y={0.05} textAnchor="middle" className="fill-slate-400 text-[0.1px]">
              Hover for details
            </text>
          )}
        </svg>
      </div>

      {showLegend && (
        <div className="flex max-h-80 flex-col gap-1 overflow-y-auto">
          {slices.slice(0, 15).map((slice, i) => (
            <div
              key={i}
              className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm transition-colors ${
                hoverIndex === i ? 'bg-slate-800' : 'hover:bg-slate-800/50'
              }`}
              onMouseEnter={() => setHoverIndex(i)}
              onMouseLeave={() => setHoverIndex(null)}
            >
              <div
                className="h-3 w-3 flex-shrink-0 rounded"
                style={{ backgroundColor: slice.color }}
              />
              <span className="min-w-0 flex-1 truncate text-slate-300" title={slice.label}>
                {slice.label.length > 20
                  ? slice.label.slice(0, 8) + '...' + slice.label.slice(-6)
                  : slice.label}
              </span>
              <span className="flex-shrink-0 font-mono tabular-nums text-white">
                {formatValue(slice.percentage)}
              </span>
            </div>
          ))}
          {slices.length > 15 && (
            <div className="px-2 py-1 text-xs text-slate-500">+{slices.length - 15} more</div>
          )}
        </div>
      )}
    </div>
  );
}
