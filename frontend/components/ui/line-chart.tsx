'use client';

import { useState, useRef, useCallback, useMemo } from 'react';
import { ChartDataPoint } from '@/lib/api';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR } from '@/lib/chart-colors';

export type LineChartType = 'line' | 'bar';
export interface LineChartMarker {
  x: string;
  label: string;
  color?: string;
}

interface LineChartProps {
  data: ChartDataPoint[];
  yAxisLabel: string;
  y2AxisLabel?: string;
  primaryColor?: string;
  secondaryColor?: string;
  height?: number;
  interactive?: boolean;
  defaultLogScale?: boolean;
  chartType?: LineChartType;
  markers?: LineChartMarker[];
}

function formatValue(val: number | undefined, isPercent = false): string {
  if (val === undefined || val === null || isNaN(val)) return '-';
  if (isPercent) return `${val.toFixed(2)}%`;
  if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(2)}B`;
  if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(2)}M`;
  if (val >= 1_000) return `${(val / 1_000).toFixed(2)}K`;
  if (val < 1 && val > 0) return val.toFixed(4);
  return val.toFixed(2);
}

function formatAxisValue(val: number, isPercent = false): string {
  if (isPercent) return `${val.toFixed(1)}%`;
  if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(1)}B`;
  if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(0)}M`;
  if (val >= 1_000) return `${(val / 1_000).toFixed(0)}K`;
  if (val < 1 && val > 0) return val.toFixed(2);
  return val.toFixed(0);
}

export function LineChart({
  data: fullData,
  yAxisLabel,
  y2AxisLabel,
  primaryColor = CHART_PRIMARY_COLOR,
  secondaryColor = CHART_SECONDARY_COLOR,
  height: chartHeightProp = 240,
  interactive = true,
  defaultLogScale = false,
  chartType = 'line',
  markers = [],
}: LineChartProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState<number | null>(null);
  const [dragEnd, setDragEnd] = useState<number | null>(null);
  const [zoomRange, setZoomRange] = useState<[number, number] | null>(null);
  const [panStart, setPanStart] = useState<{ x: number; range: [number, number] } | null>(null);
  const [useLogScale, setUseLogScale] = useState(defaultLogScale);

  const width = 600;
  const height = chartHeightProp;
  const padding = useMemo(
    () => ({ top: 20, right: y2AxisLabel ? 60 : 20, bottom: 40, left: 60 }),
    [y2AxisLabel]
  );
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;

  const data = useMemo(() => {
    if (!zoomRange) return fullData;
    return fullData.slice(zoomRange[0], zoomRange[1] + 1);
  }, [fullData, zoomRange]);

  const isZoomed = zoomRange !== null;

  const values = useMemo(() => data.map((d) => parseFloat(d.value) || 0), [data]);
  const values2 = useMemo(
    () => (y2AxisLabel ? data.map((d) => parseFloat(d.value2 || '0') || 0) : []),
    [data, y2AxisLabel]
  );

  const { minVal, maxVal, minVal2, maxVal2 } = useMemo(() => {
    let min = Infinity;
    let max = -Infinity;
    for (const v of values) {
      if (v < min) min = v;
      if (v > max) max = v;
    }
    if (!isFinite(min)) min = 0;
    if (!isFinite(max) || max === 0) max = 1;

    if (chartType === 'bar') {
      min = 0;
    }

    let max2 = -Infinity;
    for (const v of values2) {
      if (v > max2) max2 = v;
    }
    if (!isFinite(max2) || max2 === 0) max2 = 1;

    // For log scale, ensure min is at least 1 (log(0) is undefined)
    if (useLogScale) {
      min = Math.max(1, min);
      return { minVal: min, maxVal: max * 1.1, minVal2: 0, maxVal2: max2 * 1.1 };
    }

    return { minVal: min * 0.9, maxVal: max * 1.1, minVal2: 0, maxVal2: max2 * 1.1 };
  }, [values, values2, useLogScale, chartType]);

  const xScale = useCallback(
    (i: number) => {
      if (chartType === 'bar') {
        const step = chartWidth / (data.length || 1);
        return padding.left + step * (i + 0.5);
      }
      return padding.left + (i / (data.length - 1 || 1)) * chartWidth;
    },
    [chartType, chartWidth, data.length, padding.left]
  );

  const yScale = useCallback(
    (v: number) => {
      if (useLogScale) {
        const logMin = Math.log10(Math.max(1, minVal));
        const logMax = Math.log10(maxVal);
        const logV = Math.log10(Math.max(1, v));
        return padding.top + chartHeight - ((logV - logMin) / (logMax - logMin || 1)) * chartHeight;
      }
      return padding.top + chartHeight - ((v - minVal) / (maxVal - minVal || 1)) * chartHeight;
    },
    [padding.top, chartHeight, minVal, maxVal, useLogScale]
  );

  const y2Scale = useCallback(
    (v: number) =>
      padding.top + chartHeight - ((v - minVal2) / (maxVal2 - minVal2 || 1)) * chartHeight,
    [padding.top, chartHeight, minVal2, maxVal2]
  );

  const xScaleInverse = useCallback(
    (x: number) => {
      if (chartType === 'bar') {
        const step = chartWidth / (data.length || 1);
        return Math.floor((x - padding.left) / step);
      }
      const ratio = (x - padding.left) / chartWidth;
      return Math.round(ratio * (data.length - 1));
    },
    [chartType, chartWidth, data.length, padding.left]
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
      if (!interactive || e.button !== 1) return;
      e.preventDefault();

      if (!zoomRange) return;

      const rect = svgRef.current?.getBoundingClientRect();
      if (!rect) return;

      setPanStart({ x: e.clientX, range: zoomRange });
    },
    [interactive, zoomRange]
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
    if (e.button === 1) {
      setPanStart(null);
    }
  }, []);

  const handleMouseMove = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      const idx = getMouseIndex(e);
      setHoverIndex(idx);
      if (interactive && isDragging && idx !== null) {
        setDragEnd(idx);
      }
      if (panStart) {
        handleMiddleMouseMove(e);
      }
    },
    [getMouseIndex, isDragging, interactive, panStart, handleMiddleMouseMove]
  );

  const handleMouseDown = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (e.button === 1) {
        handleMiddleMouseDown(e);
        return;
      }
      if (!interactive) return;
      const idx = getMouseIndex(e);
      if (idx !== null) {
        setIsDragging(true);
        setDragStart(idx);
        setDragEnd(idx);
      }
    },
    [getMouseIndex, interactive, handleMiddleMouseDown]
  );

  const handleMouseUp = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (e.button === 1) {
        handleMiddleMouseUp(e);
        return;
      }
      if (!interactive) return;
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
    [isDragging, dragStart, dragEnd, zoomRange, interactive, handleMiddleMouseUp]
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

  const handleReset = useCallback(() => {
    setZoomRange(null);
  }, []);

  const handleWheel = useCallback(
    (e: React.WheelEvent<SVGSVGElement>) => {
      if (!interactive) return;
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
    [interactive, getMouseIndex, zoomRange, fullData.length, data.length]
  );

  const pathD = useMemo(() => {
    if (chartType === 'bar' || !data.length) return '';
    const parts: string[] = new Array(data.length);
    for (let i = 0; i < data.length; i++) {
      parts[i] = `${i === 0 ? 'M' : 'L'} ${xScale(i)} ${yScale(values[i])}`;
    }
    return parts.join(' ');
  }, [chartType, data.length, xScale, yScale, values]);

  const path2D = useMemo(() => {
    if (chartType === 'bar' || !values2.length) return '';
    const parts: string[] = new Array(data.length);
    for (let i = 0; i < data.length; i++) {
      parts[i] = `${i === 0 ? 'M' : 'L'} ${xScale(i)} ${y2Scale(values2[i])}`;
    }
    return parts.join(' ');
  }, [chartType, data.length, xScale, y2Scale, values2]);

  const barWidth = useMemo(() => {
    if (chartType !== 'bar') return 0;
    const step = chartWidth / (data.length || 1);
    return Math.max(2, Math.min(40, step * 0.7));
  }, [chartType, chartWidth, data.length]);

  const barBaseline = useMemo(
    () => (chartType === 'bar' ? yScale(useLogScale ? 1 : 0) : 0),
    [chartType, yScale, useLogScale]
  );

  const yTicks = useMemo(() => {
    if (useLogScale) {
      const logMin = Math.floor(Math.log10(Math.max(1, minVal)));
      const logMax = Math.ceil(Math.log10(maxVal));
      const ticks: number[] = [];
      for (let exp = logMin; exp <= logMax && ticks.length < 6; exp++) {
        ticks.push(Math.pow(10, exp));
      }
      return ticks.length >= 2 ? ticks : [minVal, maxVal];
    }
    return Array.from({ length: 5 }, (_, i) => minVal + ((maxVal - minVal) / 4) * i);
  }, [minVal, maxVal, useLogScale]);

  const handleToggleLogScale = useCallback(() => {
    setUseLogScale((prev) => !prev);
  }, []);

  if (!fullData.length) {
    return (
      <div className="flex h-64 items-center justify-center text-slate-500">No data available</div>
    );
  }

  const y2Ticks = values2.length
    ? Array.from({ length: 5 }, (_, i) => minVal2 + ((maxVal2 - minVal2) / 4) * i)
    : [];

  const xTickCount = Math.min(5, data.length);
  const xTicks = Array.from({ length: xTickCount }, (_, i) =>
    Math.floor((i / (xTickCount - 1 || 1)) * (data.length - 1))
  );

  const markerPositions = !markers.length
    ? []
    : markers
        .map((marker) => {
          const markerX = parseFloat(marker.x);
          const idx = data.findIndex((point) => {
            if (point.date === marker.x) return true;
            const pointX = parseFloat(point.date);
            return Number.isFinite(markerX) && Number.isFinite(pointX) && pointX === markerX;
          });
          return idx >= 0 ? { ...marker, idx } : null;
        })
        .filter((marker): marker is LineChartMarker & { idx: number } => marker !== null);

  const isPercent =
    yAxisLabel.includes('%') || yAxisLabel === 'APC' || yAxisLabel.includes('Ratio');

  const selectionStart =
    dragStart !== null && dragEnd !== null ? Math.min(dragStart, dragEnd) : null;
  const selectionEnd = dragStart !== null && dragEnd !== null ? Math.max(dragStart, dragEnd) : null;

  return (
    <div className="relative">
      <div className="absolute right-2 top-2 z-10 flex gap-2">
        <button
          onClick={handleToggleLogScale}
          className={`rounded border px-2 py-1 font-mono text-xs transition-colors ${
            useLogScale
              ? 'border-terminal-green text-terminal-green bg-terminal-green/10'
              : 'hover:border-terminal-green hover:text-terminal-green border-slate-700 bg-slate-800 text-slate-300'
          }`}
        >
          Log Scale
        </button>
        {interactive && isZoomed && (
          <button
            onClick={handleReset}
            className="hover:border-terminal-green hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-2 py-1 font-mono text-xs text-slate-300 transition-colors"
          >
            Reset Zoom
          </button>
        )}
      </div>
      <svg
        ref={svgRef}
        viewBox={`0 0 ${width} ${height}`}
        className={`w-full select-none ${interactive ? 'cursor-crosshair' : 'cursor-default'}`}
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
              stroke="#334155"
              strokeDasharray="2,2"
            />
            <text
              x={padding.left - 8}
              y={yScale(tick)}
              textAnchor="end"
              className="fill-slate-400 font-mono tabular-nums"
              dominantBaseline="middle"
              fontSize={10}
            >
              {formatAxisValue(tick, isPercent)}
            </text>
          </g>
        ))}

        {y2AxisLabel &&
          y2Ticks.map((tick, i) => (
            <text
              key={`y2-${i}`}
              x={width - padding.right + 8}
              y={y2Scale(tick)}
              textAnchor="start"
              className="fill-terminal-green font-mono tabular-nums"
              dominantBaseline="middle"
              fontSize={10}
            >
              {formatAxisValue(tick)}
            </text>
          ))}

        {xTicks.map((idx) => (
          <text
            key={`x-${idx}`}
            x={xScale(idx)}
            y={height - padding.bottom + 20}
            textAnchor="middle"
            className="fill-slate-500 font-mono tabular-nums"
            fontSize={10}
          >
            {data[idx]?.date || ''}
          </text>
        ))}

        {markerPositions.map((marker) => {
          const markerX = xScale(marker.idx);
          const labelOnRight = markerX > width - padding.right - 80;
          return (
            <g key={`marker-${marker.label}-${marker.idx}`}>
              <line
                x1={markerX}
                x2={markerX}
                y1={padding.top}
                y2={padding.top + chartHeight}
                stroke={marker.color || '#f59e0b'}
                strokeDasharray="4,3"
                strokeWidth={1.5}
                data-testid="line-chart-marker-line"
              />
              <text
                x={labelOnRight ? markerX - 6 : markerX + 6}
                y={padding.top + 12}
                textAnchor={labelOnRight ? 'end' : 'start'}
                className="fill-slate-300 font-mono"
                fontSize={9}
                data-testid="line-chart-marker-label"
              >
                {marker.label}
              </text>
            </g>
          );
        })}

        {chartType === 'line' && (
          <path
            d={pathD}
            fill="none"
            stroke={primaryColor}
            strokeWidth="2"
            data-testid="line-series-primary"
          />
        )}
        {chartType === 'line' && path2D && (
          <path
            d={path2D}
            fill="none"
            stroke={secondaryColor}
            strokeWidth="2"
            data-testid="line-series-secondary"
          />
        )}
        {chartType === 'bar' &&
          data.map((_, i) => {
            const value = Math.max(values[i] ?? 0, useLogScale ? 1 : 0);
            const barTop = yScale(value);
            const y = Math.min(barTop, barBaseline);
            const barHeight = Math.max(1, Math.abs(barBaseline - barTop));
            return (
              <rect
                key={`bar-${data[i]?.date ?? i}`}
                data-testid="bar-series-primary"
                x={xScale(i) - barWidth / 2}
                y={y}
                width={barWidth}
                height={barHeight}
                fill={primaryColor}
                fillOpacity={0.85}
              />
            );
          })}

        {interactive && selectionStart !== null && selectionEnd !== null && (
          <rect
            x={xScale(selectionStart)}
            y={padding.top}
            width={Math.max(0, xScale(selectionEnd) - xScale(selectionStart))}
            height={chartHeight}
            fill={primaryColor}
            fillOpacity={0.22}
            stroke={primaryColor}
            strokeWidth={1}
          />
        )}

        <rect
          x={padding.left}
          y={padding.top}
          width={chartWidth}
          height={chartHeight}
          fill="transparent"
          className="cursor-crosshair"
        />

        {hoverIndex !== null && hoverIndex < values.length && values[hoverIndex] !== undefined && (
          <>
            <line
              x1={xScale(hoverIndex)}
              x2={xScale(hoverIndex)}
              y1={padding.top}
              y2={padding.top + chartHeight}
              stroke="#475569"
              strokeDasharray="3,3"
            />
            {chartType === 'line' && (
              <circle
                cx={xScale(hoverIndex)}
                cy={yScale(values[hoverIndex])}
                r={4}
                fill={primaryColor}
                stroke="#fff"
                strokeWidth={2}
              />
            )}
            {chartType === 'bar' && (
              <rect
                x={xScale(hoverIndex) - barWidth / 2}
                y={Math.min(yScale(Math.max(values[hoverIndex], useLogScale ? 1 : 0)), barBaseline)}
                width={barWidth}
                height={Math.max(
                  1,
                  Math.abs(barBaseline - yScale(Math.max(values[hoverIndex], useLogScale ? 1 : 0)))
                )}
                fill="transparent"
                stroke="#ffffff"
                strokeWidth={1}
              />
            )}
            {chartType === 'line' && values2.length > 0 && values2[hoverIndex] !== undefined && (
              <circle
                cx={xScale(hoverIndex)}
                cy={y2Scale(values2[hoverIndex])}
                r={4}
                fill={secondaryColor}
                stroke="#fff"
                strokeWidth={2}
              />
            )}
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
              height={y2AxisLabel ? 60 : 44}
              rx={4}
              fill="#0f172a"
              fillOpacity={0.95}
              stroke="#334155"
            />
            <text
              x={8}
              y={16}
              className="fill-slate-300 font-mono font-medium tabular-nums"
              fontSize={10}
            >
              {data[hoverIndex]?.date}
            </text>
            <text x={8} y={32} className="fill-slate-400 font-mono tabular-nums" fontSize={10}>
              <tspan fill={primaryColor}>{yAxisLabel}: </tspan>
              <tspan className="fill-white">{formatValue(values[hoverIndex], isPercent)}</tspan>
            </text>
            {y2AxisLabel && values2.length > 0 && (
              <text x={8} y={48} className="fill-slate-400 font-mono tabular-nums" fontSize={10}>
                <tspan fill={secondaryColor}>{y2AxisLabel}: </tspan>
                <tspan className="fill-white">{formatValue(values2[hoverIndex])}</tspan>
              </text>
            )}
          </g>
        )}
      </svg>
      {interactive && isZoomed && (
        <div className="mt-1 text-center font-mono text-xs tabular-nums text-slate-500">
          Showing {data[0]?.date} - {data[data.length - 1]?.date} ({data.length} points)
        </div>
      )}
    </div>
  );
}
