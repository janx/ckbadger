'use client';

import { useState, useRef, useCallback, useMemo } from 'react';

export interface StackedAreaDataPoint {
  date: string;
  values: Record<string, string>;
}

interface OverlayLine {
  key: string;
  label: string;
  color: string;
}

interface StackedAreaChartProps {
  data: StackedAreaDataPoint[];
  series: {
    key: string;
    label: string;
    color: string;
  }[];
  height?: number;
  interactive?: boolean;
  isPercentage?: boolean;
  overlayLine?: OverlayLine;
  valueUnit?: 'raw' | 'shannon';
}

function formatValue(val: number | undefined, isPercentage = false): string {
  if (val === undefined || val === null || isNaN(val)) return '-';
  if (isPercentage) return `${val.toFixed(2)}%`;
  if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(2)}B`;
  if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(2)}M`;
  if (val >= 1_000) return `${(val / 1_000).toFixed(2)}K`;
  return val.toFixed(2);
}

function formatAxisValue(val: number, isPercentage = false): string {
  if (isPercentage) return `${val.toFixed(0)}%`;
  if (val >= 1_000_000_000) return `${(val / 1_000_000_000).toFixed(1)}B`;
  if (val >= 1_000_000) return `${(val / 1_000_000).toFixed(0)}M`;
  if (val >= 1_000) return `${(val / 1_000).toFixed(0)}K`;
  return val.toFixed(0);
}

function parseShannonToCkb(value: string): number {
  const SHANNON = BigInt(100_000_000);
  const ZERO = BigInt(0);
  try {
    const shannon = BigInt(value);
    const isNegative = shannon < ZERO;
    const abs = isNegative ? -shannon : shannon;
    const whole = Number(abs / SHANNON);
    const fraction = Number(abs % SHANNON) / 100_000_000;
    if (!Number.isFinite(whole)) return 0;
    const ckb = whole + fraction;
    return isNegative ? -ckb : ckb;
  } catch {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed / 100_000_000 : 0;
  }
}

function parseChartValue(value: string | undefined, valueUnit: 'raw' | 'shannon'): number {
  if (!value) return 0;
  if (valueUnit === 'shannon') return parseShannonToCkb(value);
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function StackedAreaChart({
  data: fullData,
  series,
  height: chartHeightProp = 240,
  interactive = true,
  isPercentage = false,
  overlayLine,
  valueUnit = 'raw',
}: StackedAreaChartProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState<number | null>(null);
  const [dragEnd, setDragEnd] = useState<number | null>(null);
  const [zoomRange, setZoomRange] = useState<[number, number] | null>(null);
  const [panStart, setPanStart] = useState<{ x: number; range: [number, number] } | null>(null);

  const width = 600;
  const height = chartHeightProp;
  const padding = useMemo(
    () => ({ top: 20, right: overlayLine ? 70 : 20, bottom: 40, left: 70 }),
    [overlayLine]
  );
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;

  const data = useMemo(() => {
    if (!zoomRange) return fullData;
    return fullData.slice(zoomRange[0], zoomRange[1] + 1);
  }, [fullData, zoomRange]);

  const isZoomed = zoomRange !== null;

  const stackedValues = useMemo(() => {
    return data.map((d) => {
      const result: Record<string, number> = {};
      const rawValues = series.map((s) => parseChartValue(d.values[s.key], valueUnit));
      const rawTotal = rawValues.reduce((sum, value) => sum + value, 0);
      const values =
        isPercentage && rawTotal > 0
          ? rawValues.map((value) => (value / rawTotal) * 100)
          : rawValues;

      let cumulative = 0;
      for (const [index, s] of series.entries()) {
        const val = values[index];
        result[`${s.key}_start`] = cumulative;
        cumulative += val;
        result[`${s.key}_end`] = cumulative;
        result[s.key] = val;
      }
      result.total = cumulative;
      return result;
    });
  }, [data, series, isPercentage, valueUnit]);

  const maxVal = useMemo(() => {
    if (isPercentage) return 100;
    const max = Math.max(...stackedValues.map((v) => v.total)) * 1.1;
    return max || 1;
  }, [stackedValues, isPercentage]);

  const overlayValues = useMemo(() => {
    if (!overlayLine) return null;
    return data.map((d) => parseChartValue(d.values[overlayLine.key], valueUnit));
  }, [data, overlayLine, valueUnit]);

  const overlayMaxVal = useMemo(() => {
    if (!overlayValues) return 1;
    const max = Math.max(...overlayValues) * 1.1;
    return max || 1;
  }, [overlayValues]);

  const xScale = useCallback(
    (i: number) => padding.left + (i / (data.length - 1 || 1)) * chartWidth,
    [data.length, padding.left, chartWidth]
  );

  const yScale = useCallback(
    (v: number) => padding.top + chartHeight - (v / maxVal) * chartHeight,
    [padding.top, chartHeight, maxVal]
  );

  const yScaleOverlay = useCallback(
    (v: number) => padding.top + chartHeight - (v / overlayMaxVal) * chartHeight,
    [padding.top, chartHeight, overlayMaxVal]
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

  if (!fullData.length) {
    return (
      <div className="flex h-64 items-center justify-center text-slate-500">No data available</div>
    );
  }

  const areaPaths = series.map((s) => {
    const topPath = data
      .map(
        (_, i) => `${i === 0 ? 'M' : 'L'} ${xScale(i)} ${yScale(stackedValues[i][`${s.key}_end`])}`
      )
      .join(' ');
    const bottomPath = data
      .map(
        (_, i) =>
          `L ${xScale(data.length - 1 - i)} ${yScale(stackedValues[data.length - 1 - i][`${s.key}_start`])}`
      )
      .join(' ');
    return `${topPath} ${bottomPath} Z`;
  });

  const overlayLinePath =
    overlayValues && overlayValues.length > 0
      ? overlayValues
          .map((v, i) => `${i === 0 ? 'M' : 'L'} ${xScale(i)} ${yScaleOverlay(v)}`)
          .join(' ')
      : null;

  const yTicks = Array.from({ length: 5 }, (_, i) => (maxVal / 4) * i);
  const overlayYTicks = overlayLine
    ? Array.from({ length: 5 }, (_, i) => (overlayMaxVal / 4) * i)
    : [];

  const xTickCount = Math.min(5, data.length);
  const xTicks = Array.from({ length: xTickCount }, (_, i) =>
    Math.floor((i / (xTickCount - 1 || 1)) * (data.length - 1))
  );

  const selectionStart =
    dragStart !== null && dragEnd !== null ? Math.min(dragStart, dragEnd) : null;
  const selectionEnd = dragStart !== null && dragEnd !== null ? Math.max(dragStart, dragEnd) : null;

  return (
    <div className="relative">
      {interactive && isZoomed && (
        <button
          onClick={handleReset}
          className="absolute right-2 top-2 z-10 rounded bg-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-600"
        >
          Reset Zoom
        </button>
      )}
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
              stroke="#374151"
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
              {formatAxisValue(tick, isPercentage)}
            </text>
          </g>
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

        {series.map((s, i) => (
          <path
            key={s.key}
            d={areaPaths[i]}
            fill={s.color}
            fillOpacity={0.8}
            stroke={s.color}
            strokeWidth="1"
          />
        ))}

        {overlayLine && overlayLinePath && (
          <path
            d={overlayLinePath}
            fill="none"
            stroke={overlayLine.color}
            strokeWidth="2"
            strokeLinejoin="round"
          />
        )}

        {overlayLine &&
          overlayYTicks.map((tick, i) => (
            <text
              key={`oy-${i}`}
              x={width - padding.right + 8}
              y={yScaleOverlay(tick)}
              textAnchor="start"
              className="fill-slate-400 font-mono tabular-nums"
              dominantBaseline="middle"
              fontSize={10}
            >
              {formatAxisValue(tick)}
            </text>
          ))}

        {interactive && selectionStart !== null && selectionEnd !== null && (
          <rect
            x={xScale(selectionStart)}
            y={padding.top}
            width={Math.max(0, xScale(selectionEnd) - xScale(selectionStart))}
            height={chartHeight}
            fill="#8b5cf6"
            fillOpacity={0.3}
            stroke="#8b5cf6"
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

        {hoverIndex !== null && hoverIndex < data.length && (
          <>
            <line
              x1={xScale(hoverIndex)}
              x2={xScale(hoverIndex)}
              y1={padding.top}
              y2={padding.top + chartHeight}
              stroke="#6b7280"
              strokeDasharray="3,3"
            />
          </>
        )}

        {hoverIndex !== null && hoverIndex < data.length && data[hoverIndex] && (
          <g
            transform={`translate(${Math.min(xScale(hoverIndex) + 10, width - 160)}, ${padding.top + 10})`}
          >
            <rect
              x={0}
              y={0}
              width={150}
              height={20 + series.length * 14 + (overlayLine ? 14 : 0)}
              rx={4}
              fill="#0f172a"
              fillOpacity={0.95}
              stroke="#334155"
            />
            <text
              x={8}
              y={14}
              className="fill-slate-300 font-mono font-medium tabular-nums"
              fontSize={10}
            >
              {data[hoverIndex]?.date}
            </text>
            {series.map((s, i) => (
              <text
                key={s.key}
                x={8}
                y={28 + i * 14}
                className="fill-slate-400 font-mono tabular-nums"
                fontSize={10}
              >
                <tspan fill={s.color}>● </tspan>
                <tspan>{s.label}: </tspan>
                <tspan className="fill-white">
                  {formatValue(stackedValues[hoverIndex][s.key], isPercentage)}
                </tspan>
              </text>
            ))}
            {overlayLine && overlayValues && (
              <text
                x={8}
                y={28 + series.length * 14}
                className="fill-slate-400 font-mono tabular-nums"
                fontSize={10}
              >
                <tspan fill={overlayLine.color}>━ </tspan>
                <tspan>{overlayLine.label}: </tspan>
                <tspan className="fill-white">{formatValue(overlayValues[hoverIndex])}</tspan>
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
