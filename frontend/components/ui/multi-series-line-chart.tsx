'use client';

import { useState, useRef, useCallback, useMemo } from 'react';
import { StackedAreaDataPoint, StackedAreaSeries } from '@/lib/api';
import {
  CHART_PRIMARY_COLOR,
  CHART_GRID_COLOR,
  CHART_HOVER_COLOR,
  CHART_TOOLTIP_BG,
  CHART_TOOLTIP_BORDER,
} from '@/lib/chart-colors';

interface MultiSeriesLineChartProps {
  data: StackedAreaDataPoint[];
  series: StackedAreaSeries[];
  height?: number;
  defaultVisibleSeries?: string[];
}

function formatValue(val: number | undefined): string {
  if (val === undefined || val === null || isNaN(val)) return '-';
  if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(2)}B`;
  if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(2)}M`;
  if (val >= 1_000) return `${(val / 1_000).toFixed(2)}K`;
  return val.toFixed(0);
}

function formatAxisValue(val: number): string {
  if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(1)}B`;
  if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(0)}M`;
  if (val >= 1_000) return `${(val / 1_000).toFixed(0)}K`;
  return val.toFixed(0);
}

export function MultiSeriesLineChart({
  data: fullData,
  series,
  height: chartHeightProp = 400,
  defaultVisibleSeries,
}: MultiSeriesLineChartProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState<number | null>(null);
  const [dragEnd, setDragEnd] = useState<number | null>(null);
  const [zoomRange, setZoomRange] = useState<[number, number] | null>(null);
  const [panStart, setPanStart] = useState<{ x: number; range: [number, number] } | null>(null);
  const [visibleSeries, setVisibleSeries] = useState<Set<string>>(
    new Set(defaultVisibleSeries || series.map((s) => s.key))
  );

  const width = 600;
  const height = chartHeightProp;
  const padding = useMemo(() => ({ top: 20, right: 20, bottom: 40, left: 60 }), []);
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;

  const data = useMemo(() => {
    if (!zoomRange) return fullData;
    return fullData.slice(zoomRange[0], zoomRange[1] + 1);
  }, [fullData, zoomRange]);

  const isZoomed = zoomRange !== null;

  const seriesValues = useMemo(() => {
    const result: Record<string, number[]> = {};
    for (const s of series) {
      result[s.key] = data.map((d) => parseFloat(d.values[s.key] || '0') || 0);
    }
    return result;
  }, [data, series]);

  const { minVal, maxVal } = useMemo(() => {
    let min = Infinity;
    let max = -Infinity;
    for (const s of series) {
      if (!visibleSeries.has(s.key)) continue;
      const vals = seriesValues[s.key];
      for (const v of vals) {
        if (v < min) min = v;
        if (v > max) max = v;
      }
    }
    if (!isFinite(min)) min = 0;
    if (!isFinite(max) || max === 0) max = 1;
    return { minVal: min * 0.9, maxVal: max * 1.1 };
  }, [series, seriesValues, visibleSeries]);

  const xScale = useCallback(
    (i: number) => padding.left + (i / (data.length - 1 || 1)) * chartWidth,
    [data.length, padding.left, chartWidth]
  );

  const yScale = useCallback(
    (v: number) =>
      padding.top + chartHeight - ((v - minVal) / (maxVal - minVal || 1)) * chartHeight,
    [padding.top, chartHeight, minVal, maxVal]
  );

  const xScaleInverse = useCallback(
    (x: number) => {
      const ratio = (x - padding.left) / chartWidth;
      return Math.round(ratio * (data.length - 1));
    },
    [data.length, padding.left, chartWidth]
  );

  const getMouseIndex = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (!svgRef.current) return null;
      const rect = svgRef.current.getBoundingClientRect();
      const scaleX = width / rect.width;
      const x = (e.clientX - rect.left) * scaleX;
      if (x < padding.left) return 0;
      if (x > width - padding.right) return data.length - 1;
      const idx = xScaleInverse(x);
      return Math.max(0, Math.min(data.length - 1, idx));
    },
    [data.length, padding.left, padding.right, xScaleInverse]
  );

  const handleMiddleMouseDown = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (e.button !== 1) return;
      e.preventDefault();
      if (!zoomRange) return;
      const rect = svgRef.current?.getBoundingClientRect();
      if (!rect) return;
      setPanStart({ x: e.clientX, range: zoomRange });
    },
    [zoomRange]
  );

  const handleMiddleMouseMove = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (!panStart || !svgRef.current) return;
      const rect = svgRef.current.getBoundingClientRect();
      const deltaX = e.clientX - panStart.x;
      const scaleX = width / rect.width;
      const pixelDelta = deltaX * scaleX;
      const rangeLength = panStart.range[1] - panStart.range[0];
      const pointsPerPixel = rangeLength / chartWidth;
      const pointsDelta = Math.round(-pixelDelta * pointsPerPixel);
      let newStart = panStart.range[0] + pointsDelta;
      let newEnd = panStart.range[1] + pointsDelta;
      if (newStart < 0) {
        newStart = 0;
        newEnd = rangeLength;
      }
      if (newEnd > fullData.length - 1) {
        newEnd = fullData.length - 1;
        newStart = Math.max(0, newEnd - rangeLength);
      }
      setZoomRange([newStart, newEnd]);
    },
    [panStart, fullData.length, chartWidth]
  );

  const handleMiddleMouseUp = useCallback((e: React.MouseEvent<SVGSVGElement>) => {
    if (e.button === 1) setPanStart(null);
  }, []);

  const handleMouseMove = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      const idx = getMouseIndex(e);
      setHoverIndex(idx);
      if (isDragging && idx !== null) setDragEnd(idx);
      if (panStart) handleMiddleMouseMove(e);
    },
    [getMouseIndex, isDragging, panStart, handleMiddleMouseMove]
  );

  const handleMouseDown = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (e.button === 1) {
        handleMiddleMouseDown(e);
        return;
      }
      const idx = getMouseIndex(e);
      if (idx !== null) {
        setIsDragging(true);
        setDragStart(idx);
        setDragEnd(idx);
      }
    },
    [getMouseIndex, handleMiddleMouseDown]
  );

  const handleMouseUp = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (e.button === 1) {
        handleMiddleMouseUp(e);
        return;
      }
      if (isDragging && dragStart !== null && dragEnd !== null) {
        const start = Math.min(dragStart, dragEnd);
        const end = Math.max(dragStart, dragEnd);
        if (end - start >= 2) {
          const baseStart = zoomRange ? zoomRange[0] : 0;
          setZoomRange([baseStart + start, baseStart + end]);
        }
      }
      setIsDragging(false);
      setDragStart(null);
      setDragEnd(null);
    },
    [isDragging, dragStart, dragEnd, zoomRange, handleMiddleMouseUp]
  );

  const handleMouseLeave = useCallback(() => {
    setHoverIndex(null);
    setPanStart(null);
    if (isDragging) {
      setIsDragging(false);
      setDragStart(null);
      setDragEnd(null);
    }
  }, [isDragging]);

  const handleReset = useCallback(() => setZoomRange(null), []);

  const handleWheel = useCallback(
    (e: React.WheelEvent<SVGSVGElement>) => {
      e.preventDefault();
      const idx = getMouseIndex(e as unknown as React.MouseEvent<SVGSVGElement>);
      if (idx === null) return;
      const currentStart = zoomRange ? zoomRange[0] : 0;
      const currentEnd = zoomRange ? zoomRange[1] : fullData.length - 1;
      const currentLength = currentEnd - currentStart;
      const zoomFactor = e.deltaY > 0 ? 1.2 : 0.8;
      const newLength = Math.max(
        10,
        Math.min(fullData.length, Math.round(currentLength * zoomFactor))
      );
      if (newLength >= fullData.length) {
        setZoomRange(null);
        return;
      }
      const globalIdx = currentStart + idx;
      const ratio = idx / (data.length - 1 || 1);
      let newStart = Math.round(globalIdx - newLength * ratio);
      let newEnd = newStart + newLength;
      if (newStart < 0) {
        newStart = 0;
        newEnd = newLength;
      }
      if (newEnd > fullData.length - 1) {
        newEnd = fullData.length - 1;
        newStart = Math.max(0, newEnd - newLength);
      }
      setZoomRange([newStart, newEnd]);
    },
    [getMouseIndex, zoomRange, fullData.length, data.length]
  );

  const toggleSeries = useCallback((key: string) => {
    setVisibleSeries((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        if (next.size > 1) next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }, []);

  const paths = useMemo(() => {
    const result: Record<string, string> = {};
    for (const s of series) {
      if (!visibleSeries.has(s.key)) continue;
      const vals = seriesValues[s.key];
      if (!vals.length) continue;
      const parts: string[] = [];
      for (let i = 0; i < data.length; i++) {
        parts.push(`${i === 0 ? 'M' : 'L'} ${xScale(i)} ${yScale(vals[i])}`);
      }
      result[s.key] = parts.join(' ');
    }
    return result;
  }, [series, seriesValues, visibleSeries, data.length, xScale, yScale]);

  const yTicks = useMemo(
    () => Array.from({ length: 5 }, (_, i) => minVal + ((maxVal - minVal) / 4) * i),
    [minVal, maxVal]
  );

  if (!fullData.length) {
    return (
      <div className="text-text-muted flex h-64 items-center justify-center">No data available</div>
    );
  }

  const xTickCount = Math.min(5, data.length);
  const xTicks = Array.from({ length: xTickCount }, (_, i) =>
    Math.floor((i / (xTickCount - 1 || 1)) * (data.length - 1))
  );

  const selectionStart =
    dragStart !== null && dragEnd !== null ? Math.min(dragStart, dragEnd) : null;
  const selectionEnd = dragStart !== null && dragEnd !== null ? Math.max(dragStart, dragEnd) : null;

  return (
    <div className="relative">
      <div className="mb-4 flex flex-wrap items-center justify-center gap-4">
        {series.map((s) => (
          <button
            key={s.key}
            onClick={() => toggleSeries(s.key)}
            className={`flex items-center gap-2 rounded border px-3 py-1.5 font-mono text-xs transition-colors ${
              visibleSeries.has(s.key)
                ? 'border-base-border bg-base-elevated text-text-primary'
                : 'border-base-border bg-base-surface text-text-muted'
            }`}
          >
            <span
              className="h-3 w-3 rounded"
              style={{ backgroundColor: visibleSeries.has(s.key) ? s.color : CHART_HOVER_COLOR }}
            />
            {s.label}
          </button>
        ))}
      </div>

      <div className="absolute right-2 top-12 z-10 flex gap-2">
        {isZoomed && (
          <button
            onClick={handleReset}
            className="hover:border-interactive hover:text-interactive border-base-border bg-base-elevated text-text-secondary rounded border px-2 py-1 font-mono text-xs transition-colors"
          >
            Reset Zoom
          </button>
        )}
      </div>

      <svg
        ref={svgRef}
        viewBox={`0 0 ${width} ${height}`}
        className="w-full cursor-crosshair select-none"
        onMouseMove={handleMouseMove}
        onMouseDown={handleMouseDown}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseLeave}
        onWheel={handleWheel}
        onContextMenu={(e) => e.preventDefault()}
      >
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
              x={padding.left - 8}
              y={yScale(tick)}
              textAnchor="end"
              className="fill-text-muted font-mono tabular-nums"
              dominantBaseline="middle"
              fontSize={10}
            >
              {formatAxisValue(tick)}
            </text>
          </g>
        ))}

        {xTicks.map((idx) => (
          <text
            key={`x-${idx}`}
            x={xScale(idx)}
            y={height - padding.bottom + 20}
            textAnchor="middle"
            className="fill-text-muted font-mono tabular-nums"
            fontSize={10}
          >
            {data[idx]?.date || ''}
          </text>
        ))}

        {series.map(
          (s) =>
            paths[s.key] && (
              <path key={s.key} d={paths[s.key]} fill="none" stroke={s.color} strokeWidth="2" />
            )
        )}

        {selectionStart !== null && selectionEnd !== null && (
          <rect
            x={xScale(selectionStart)}
            y={padding.top}
            width={Math.max(0, xScale(selectionEnd) - xScale(selectionStart))}
            height={chartHeight}
            fill={CHART_PRIMARY_COLOR}
            fillOpacity={0.3}
            stroke={CHART_PRIMARY_COLOR}
            strokeWidth={1}
          />
        )}

        <rect
          x={padding.left}
          y={padding.top}
          width={chartWidth}
          height={chartHeight}
          fill="transparent"
        />

        {hoverIndex !== null && hoverIndex < data.length && (
          <>
            <line
              x1={xScale(hoverIndex)}
              x2={xScale(hoverIndex)}
              y1={padding.top}
              y2={padding.top + chartHeight}
              stroke={CHART_HOVER_COLOR}
              strokeDasharray="3,3"
            />
            {series.map((s) => {
              if (!visibleSeries.has(s.key)) return null;
              const val = seriesValues[s.key][hoverIndex];
              return (
                <circle
                  key={s.key}
                  cx={xScale(hoverIndex)}
                  cy={yScale(val)}
                  r={4}
                  fill={s.color}
                  stroke="#fff"
                  strokeWidth={2}
                />
              );
            })}
          </>
        )}

        {hoverIndex !== null && hoverIndex < data.length && data[hoverIndex] && (
          <g
            transform={`translate(${Math.min(xScale(hoverIndex) + 10, width - 200)}, ${padding.top + 10})`}
          >
            <rect
              x={0}
              y={0}
              width={180}
              height={20 + series.filter((s) => visibleSeries.has(s.key)).length * 16}
              rx={4}
              fill={CHART_TOOLTIP_BG}
              fillOpacity={0.95}
              stroke={CHART_TOOLTIP_BORDER}
            />
            <text
              x={8}
              y={14}
              className="fill-text-secondary font-mono font-medium tabular-nums"
              fontSize={10}
            >
              {data[hoverIndex]?.date}
            </text>
            {series
              .filter((s) => visibleSeries.has(s.key))
              .map((s, i) => (
                <text
                  key={s.key}
                  x={8}
                  y={30 + i * 16}
                  className="fill-text-muted font-mono tabular-nums"
                  fontSize={10}
                >
                  <tspan fill={s.color}>{s.label}: </tspan>
                  <tspan className="fill-text-primary">
                    {formatValue(seriesValues[s.key][hoverIndex])}
                  </tspan>
                </text>
              ))}
          </g>
        )}
      </svg>

      {isZoomed && (
        <div className="text-text-muted mt-1 text-center font-mono text-xs tabular-nums">
          Showing {data[0]?.date} - {data[data.length - 1]?.date} ({data.length} points)
        </div>
      )}

      <div className="text-text-muted mt-4 text-center font-mono text-xs">
        Drag to select range | Scroll to zoom | Middle-click drag to pan
      </div>
    </div>
  );
}
