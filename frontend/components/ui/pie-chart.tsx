'use client';

import { useState, useMemo } from 'react';
import { getChartPaletteColor, CHART_TOOLTIP_BG } from '@/lib/chart-colors';

interface PieChartDataPoint {
  label: string;
  value: number;
  color?: string;
}

interface PieChartProps {
  data: PieChartDataPoint[];
  size?: number;
  fullWidth?: boolean;
  showLegend?: boolean;
  formatValue?: (value: number) => string;
  highlightIndex?: number | null;
  onHighlightChange?: (index: number | null) => void;
}

export function PieChart({
  data,
  size = 300,
  fullWidth = false,
  showLegend = true,
  formatValue = (v) => v.toFixed(2) + '%',
  highlightIndex,
  onHighlightChange,
}: PieChartProps) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const activeIndex =
    highlightIndex !== undefined && highlightIndex !== null ? highlightIndex : hoverIndex;

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
        color: d.color || getChartPaletteColor(i),
        midAngle: startAngle + angle / 2,
      };
    });
  }, [data, total]);

  if (!data.length || total === 0) {
    return (
      <div
        className="text-text-dim flex items-center justify-center"
        style={{ width: size, height: size }}
      >
        No data available
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-6 lg:flex-row lg:items-start">
      <div
        className={fullWidth ? 'relative aspect-square w-full' : 'relative'}
        style={fullWidth ? undefined : { width: size, height: size }}
      >
        <svg viewBox="-1.2 -1.2 2.4 2.4" className="h-full w-full">
          {slices.map((slice, i) => (
            <path
              key={i}
              d={slice.pathD}
              fill={slice.color}
              stroke={CHART_TOOLTIP_BG}
              strokeWidth={0.02}
              className="cursor-pointer transition-opacity"
              opacity={activeIndex === null || activeIndex === i ? 1 : 0.5}
              onMouseEnter={() => {
                setHoverIndex(i);
                onHighlightChange?.(i);
              }}
              onMouseLeave={() => {
                setHoverIndex(null);
                onHighlightChange?.(null);
              }}
              transform={activeIndex === i ? `scale(1.05)` : undefined}
              style={{ transformOrigin: 'center' }}
            />
          ))}
          <circle cx={0} cy={0} r={0.6} fill={CHART_TOOLTIP_BG} />
          {activeIndex !== null && slices[activeIndex] && (
            <>
              <text
                x={0}
                y={-0.1}
                textAnchor="middle"
                className="fill-text-bright text-[0.12px] font-medium"
              >
                {slices[activeIndex].label.length > 12
                  ? slices[activeIndex].label.slice(0, 12) + '...'
                  : slices[activeIndex].label}
              </text>
              <text
                x={0}
                y={0.15}
                textAnchor="middle"
                className="fill-text font-mono text-[0.14px] tabular-nums"
              >
                {formatValue(slices[activeIndex].percentage)}
              </text>
            </>
          )}
          {activeIndex === null && (
            <text x={0} y={0.05} textAnchor="middle" className="fill-text-dim text-[0.1px]">
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
                activeIndex === i ? 'bg-base-elevated' : 'hover:bg-base-elevated/50'
              }`}
              onMouseEnter={() => {
                setHoverIndex(i);
                onHighlightChange?.(i);
              }}
              onMouseLeave={() => {
                setHoverIndex(null);
                onHighlightChange?.(null);
              }}
            >
              <div
                className="h-3 w-3 flex-shrink-0 rounded"
                style={{ backgroundColor: slice.color }}
              />
              <span className="text-text min-w-0 flex-1 truncate" title={slice.label}>
                {slice.label.length > 20
                  ? slice.label.slice(0, 8) + '...' + slice.label.slice(-6)
                  : slice.label}
              </span>
              <span className="text-text-bright flex-shrink-0 font-mono tabular-nums">
                {formatValue(slice.percentage)}
              </span>
            </div>
          ))}
          {slices.length > 15 && (
            <div className="text-text-dim px-2 py-1 text-xs">+{slices.length - 15} more</div>
          )}
        </div>
      )}
    </div>
  );
}
