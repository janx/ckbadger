'use client';

import { useState, useRef, useCallback, useMemo } from 'react';
import { ChartDataPoint } from '@/lib/api';

interface LineChartProps {
  data: ChartDataPoint[];
  yAxisLabel: string;
  y2AxisLabel?: string;
  primaryColor?: string;
  secondaryColor?: string;
  height?: number;
  interactive?: boolean;
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
  primaryColor = '#8b5cf6',
  secondaryColor = '#00c389',
  height: chartHeightProp = 240,
  interactive = true,
}: LineChartProps) {
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

    let max2 = -Infinity;
    for (const v of values2) {
      if (v > max2) max2 = v;
    }
    if (!isFinite(max2) || max2 === 0) max2 = 1;

    return { minVal: min * 0.9, maxVal: max * 1.1, minVal2: 0, maxVal2: max2 * 1.1 };
  }, [values, values2]);

  const xScale = useCallback(
    (i: number) => padding.left + (i / (data.length - 1 || 1)) * chartWidth,
    [data.length, padding.left, chartWidth]
  );

  const yScale = useCallback(
    (v: number) =>
      padding.top + chartHeight - ((v - minVal) / (maxVal - minVal || 1)) * chartHeight,
    [padding.top, chartHeight, minVal, maxVal]
  );

  const y2Scale = useCallback(
    (v: number) =>
      padding.top + chartHeight - ((v - minVal2) / (maxVal2 - minVal2 || 1)) * chartHeight,
    [padding.top, chartHeight, minVal2, maxVal2]
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

  const pathD = useMemo(() => {
    if (!data.length) return '';
    const parts: string[] = new Array(data.length);
    for (let i = 0; i < data.length; i++) {
      parts[i] = `${i === 0 ? 'M' : 'L'} ${xScale(i)} ${yScale(values[i])}`;
    }
    return parts.join(' ');
  }, [data.length, xScale, yScale, values]);

  const path2D = useMemo(() => {
    if (!values2.length) return '';
    const parts: string[] = new Array(data.length);
    for (let i = 0; i < data.length; i++) {
      parts[i] = `${i === 0 ? 'M' : 'L'} ${xScale(i)} ${y2Scale(values2[i])}`;
    }
    return parts.join(' ');
  }, [data.length, xScale, y2Scale, values2]);

  if (!fullData.length) {
    return (
      <div className="flex h-64 items-center justify-center text-slate-500">No data available</div>
    );
  }

  const yTicks = Array.from({ length: 5 }, (_, i) => minVal + ((maxVal - minVal) / 4) * i);
  const y2Ticks = values2.length
    ? Array.from({ length: 5 }, (_, i) => minVal2 + ((maxVal2 - minVal2) / 4) * i)
    : [];

  const xTickCount = Math.min(5, data.length);
  const xTicks = Array.from({ length: xTickCount }, (_, i) =>
    Math.floor((i / (xTickCount - 1 || 1)) * (data.length - 1))
  );

  const isPercent =
    yAxisLabel.includes('%') || yAxisLabel === 'APC' || yAxisLabel.includes('Ratio');

  const selectionStart =
    dragStart !== null && dragEnd !== null ? Math.min(dragStart, dragEnd) : null;
  const selectionEnd = dragStart !== null && dragEnd !== null ? Math.max(dragStart, dragEnd) : null;

  return (
    <div className="relative">
      {interactive && isZoomed && (
        <button
          onClick={handleReset}
          className="hover:border-terminal-green hover:text-terminal-green absolute right-2 top-2 z-10 rounded border border-slate-700 bg-slate-800 px-2 py-1 font-mono text-xs text-slate-300 transition-colors"
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

        <path d={pathD} fill="none" stroke={primaryColor} strokeWidth="2" />
        {path2D && <path d={path2D} fill="none" stroke={secondaryColor} strokeWidth="2" />}

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
            <circle
              cx={xScale(hoverIndex)}
              cy={yScale(values[hoverIndex])}
              r={4}
              fill={primaryColor}
              stroke="#fff"
              strokeWidth={2}
            />
            {values2.length > 0 && values2[hoverIndex] !== undefined && (
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
