'use client';
import { useState, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { useElementWidth } from '@/hooks/useElementWidth';
import { niceTicks, formatTick } from '@/lib/chart-ticks';
import { exactSignedDecimalRatio } from '@/lib/exact-integer';
import { CLASS_COLORS, HUE_RAMP, type AssetClass } from '@/lib/design-tokens';

export interface ChartPoint {
  label: string;
  values: Readonly<Record<string, string>>;
  /** Optional exact time coordinate for plots sharing one time axis. */
  x?: number;
}
export interface ChartSeries {
  key: string;
  label: string;
  assetClass?: AssetClass;
  /** Explicit semantic color for non-class quantities such as occupied/free capacity. */
  color?: string;
  // --- F2 charts group (CH-12, CH-58): per-series stroke, both off by default ---
  /** Draw this series' line dashed (a derived or companion reading, e.g. real inflation). */
  dashed?: boolean;
  /** In an `area` chart, draw this series as a bare line over the filled ones. */
  lineOnly?: boolean;
}
export interface ChartMarker {
  index: number;
  label: string;
  /** Short plot glyph; the full label remains in the accessible legend. */
  plotLabel?: string;
  href: string;
}
/** A stretch of the shared time axis with no observation: shaded, labelled, never bridged. */
export interface ChartGap {
  from: number;
  to: number;
  label: string;
}
export interface ChartProps {
  title: string;
  subtitle: string;
  data: readonly ChartPoint[];
  series: readonly ChartSeries[];
  /** Physical display unit, e.g. PH/s, seconds, CKB. Scale values before passing them. */
  unit: string;
  kind?: 'line' | 'area' | 'stacked' | 'bars' | 'ranked';
  yScale?: 'linear' | 'log';
  height?: number;
  markers?: readonly ChartMarker[];
  /**
   * Receives the original exact decimal, never geometry converted through Number. Required: every
   * chart states how its unit is read (S2), there is no raw pass-through default.
   */
  formatValue: (value: string) => ReactNode;
  formatAxis?: (value: number, step: number) => string;
  activeIndex?: number | null;
  onActiveIndexChange?: (index: number | null) => void;
  xDomain?: readonly [number, number];
  /** Index of the last observed point; a line is dashed after it (a model projected ahead). */
  projectedAfter?: number;
  /**
   * The reading shown while nothing is hovered. Omitted: the last point. `null`: no reading,
   * because the source has not provided one (a model's "today", CH-03) — never a stand-in sample.
   */
  reading?: ChartPoint | null;
  /** Requires `xDomain`; series lines and areas break across each gap instead of joining it. */
  gaps?: readonly ChartGap[];
  plotLeft?: number;
  hideCaption?: boolean;
  hideXAxis?: boolean;
  cursorX?: number | null;
  // --- F2 charts group (CH-09, CH-12, CH-15, CH-22, CH-23, CH-24, CH-35): every one opt-in ---
  /** A fixed value domain, e.g. [0, 100] for a share chart, instead of rounding to nice ticks. */
  yDomain?: readonly [number, number];
  /** Dashed horizontal reference lines (a consensus target); the axis widens to hold them. */
  referenceLines?: readonly { value: number; label: string }[];
  /** X-axis labels at these point indexes, replacing the first / last label pair. */
  xTicks?: readonly { index: number; label: string }[];
  /** A dot at the end of each line, and at the hovered point with an in-plot reading. */
  pointDots?: boolean;
  /** Bars fill their slot (a few labelled buckets) instead of 3 px ticks. */
  slotBars?: boolean;
  /** Keep the markers' in-plot labels but drop the link row under the plot. */
  hideMarkerLegend?: boolean;
  /** How a point label reads wherever the chart prints it (hover caption and reading). */
  formatLabel?: (label: string) => string;
}
export function Chart({
  title,
  subtitle,
  data,
  series,
  unit,
  kind = 'line',
  yScale = 'linear',
  height = 220,
  markers = [],
  formatValue,
  formatAxis = formatTick,
  activeIndex: controlledActive,
  onActiveIndexChange,
  xDomain,
  gaps = [],
  projectedAfter,
  reading,
  plotLeft,
  hideCaption = false,
  hideXAxis = false,
  cursorX,
  yDomain,
  referenceLines = [],
  xTicks,
  pointDots = false,
  slotBars = false,
  hideMarkerLegend = false,
  formatLabel = (label: string) => label,
}: ChartProps) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const [localActive, setLocalActive] = useState<number | null>(null);
  const active = controlledActive === undefined ? localActive : controlledActive;
  const setActive = (index: number | null) => {
    setLocalActive(index);
    onActiveIndexChange?.(index);
  };
  if (
    xDomain &&
    (!Number.isFinite(xDomain[0]) ||
      !Number.isFinite(xDomain[1]) ||
      xDomain[0] >= xDomain[1] ||
      data.some(
        (p) => p.x === undefined || !Number.isFinite(p.x) || p.x < xDomain[0] || p.x > xDomain[1]
      ))
  )
    throw new Error(`${title}: invalid shared time axis`);
  if (
    gaps.length &&
    (!xDomain ||
      gaps.some(
        (gap) =>
          !(gap.from < gap.to) ||
          gap.from < xDomain[0] ||
          gap.to > xDomain[1] ||
          data.some((p) => p.x! > gap.from && p.x! < gap.to)
      ))
  )
    throw new Error(`${title}: gaps must lie on the shared time axis between observations`);
  if (
    projectedAfter !== undefined &&
    (kind !== 'line' ||
      !Number.isSafeInteger(projectedAfter) ||
      projectedAfter < 0 ||
      projectedAfter >= data.length)
  )
    throw new Error(`${title}: projection must start at a returned point of a line chart`);
  if (!series.length) throw new Error(`${title}: chart needs at least one series`);
  if (new Set(series.map((s) => s.key)).size !== series.length)
    throw new Error(`${title}: duplicate chart series`);
  if (kind === 'ranked' && (series.length !== 1 || data.length > 6))
    throw new Error(
      `${title}: ranked composition requires one series and at most five categories plus others`
    );
  const values = data.map((p, i) =>
    series.map((s) => {
      const raw = p.values[s.key];
      if (raw === undefined) throw new Error(`${title}: missing ${s.key} at ${p.label}`);
      const [n, d] = exactSignedDecimalRatio(raw, `${title}: ${s.key} at ${p.label}`);
      const value = Number(n) / Number(d);
      if (!Number.isFinite(value))
        throw new Error(`${title}: ${s.key} at ${i} exceeds chart geometry range`);
      if (kind === 'stacked' && value < 0)
        throw new Error(`${title}: stacked series ${s.key} at ${p.label} is negative`);
      return value;
    })
  );
  if (
    yScale === 'log' &&
    (kind !== 'line' || values.some((row) => row.some((value) => value <= 0)))
  )
    throw new Error(`${title}: logarithmic scale requires a positive line series`);
  for (const marker of markers)
    if (!Number.isSafeInteger(marker.index) || marker.index < 0 || marker.index >= data.length)
      throw new Error(`${title}: hardfork ${marker.label} has invalid index ${marker.index}`);
  const color = (i: number) =>
    series[i].color ??
    (series[i].assetClass ? CLASS_COLORS[series[i].assetClass!] : HUE_RAMP[i % HUE_RAMP.length]);
  const activeIndex = active !== null && active < data.length ? active : null;
  const selected =
    activeIndex !== null
      ? data[activeIndex]
      : reading !== undefined
        ? reading
        : data[data.length - 1];
  const plotValues =
    kind === 'stacked' ? values.map((row) => row.reduce((s, v) => s + v, 0)) : values.flat();
  // F2 charts group: reference values are plotted, so the axis must hold them (CH-12).
  for (const line of referenceLines)
    if (!Number.isFinite(line.value) || (yScale === 'log' && line.value <= 0))
      throw new Error(`${title}: reference line ${line.label} has no plottable value`);
  if (
    yDomain &&
    (yScale === 'log' ||
      !Number.isFinite(yDomain[0]) ||
      !Number.isFinite(yDomain[1]) ||
      yDomain[0] >= yDomain[1])
  )
    throw new Error(`${title}: invalid fixed value domain`);
  for (const tick of xTicks ?? [])
    if (!Number.isSafeInteger(tick.index) || tick.index < 0 || tick.index >= data.length)
      throw new Error(`${title}: x tick ${tick.label} has invalid index ${tick.index}`);
  const geometryValues = [...plotValues, ...referenceLines.map((line) => line.value)].map(
    (value) => (yScale === 'log' ? Math.log10(value) : value)
  );
  const ticks = yDomain
    ? niceTicks(yDomain[0], yDomain[1], 4)
    : plotValues.length
      ? niceTicks(
          kind === 'line' ? Math.min(...geometryValues) : Math.min(0, ...geometryValues),
          kind === 'line' ? Math.max(...geometryValues) : Math.max(0, ...geometryValues),
          4
        )
      : null;
  const tickLabel = (value: number, step: number) =>
    yScale === 'log'
      ? `${formatAxis(10 ** value, 10 ** step)} ${unit}`
      : `${formatAxis(value, step)} ${unit}`;
  const labels = ticks?.values.map((v) => tickLabel(v, ticks.step)) ?? [];
  const left = plotLeft ?? Math.max(48, ...labels.map((s) => s.length * 6 + 8));
  const right = width === null ? null : width - 10,
    top = 16,
    bottom = height - 26;
  const lo = ticks?.values[0],
    hi = ticks?.values[ticks.values.length - 1];
  const x = (i: number) =>
    left +
    (xDomain
      ? (data[i].x! - xDomain[0]) / (xDomain[1] - xDomain[0])
      : slotBars
        ? (i + 0.5) / data.length // F2 charts group: slot centres (CH-09)
        : data.length === 1
          ? 0.5
          : i / (data.length - 1)) *
      ((right as number) - left);
  const y = (v: number) =>
    bottom -
    (((yScale === 'log' ? Math.log10(v) : v) - (lo as number)) /
      ((hi as number) - (lo as number))) *
      (bottom - top);
  // Index runs between gaps: a series is never drawn across a stretch with no observation.
  const runs = data.reduce<number[][]>((all, point, i) => {
    const broken = i > 0 && gaps.some((gap) => data[i - 1].x! <= gap.from && point.x! >= gap.to);
    if (i === 0 || broken) all.push([i]);
    else all[all.length - 1].push(i);
    return all;
  }, []);
  const yTick = (v: number) =>
    bottom - ((v - (lo as number)) / ((hi as number) - (lo as number))) * (bottom - top);
  return (
    <figure className="m-0 min-w-0">
      <figcaption
        className={
          hideCaption ? 'sr-only' : 'mb-3 flex flex-wrap items-baseline justify-between gap-3'
        }
      >
        <span>
          <span className="text-text-bright block text-[15px] font-medium">{title}</span>
          <span className="text-text-dim text-[11px]">
            {activeIndex === null ? subtitle : selected && formatLabel(selected.label)}
          </span>
        </span>
        <span className="text-text-bright flex flex-wrap justify-end gap-3 text-[18px] tabular-nums">
          {selected &&
            series.map((s, i) => (
              <span key={s.key}>
                {series.length > 1 && (
                  <span className="mr-2 text-[11px]" style={{ color: color(i) }}>
                    {s.label}
                  </span>
                )}
                {formatValue(selected.values[s.key])}
                <span className="text-text-dim text-[11px]"> {unit}</span>
              </span>
            ))}
        </span>
      </figcaption>
      <div ref={ref} className="min-w-0" style={{ minHeight: height }}>
        {!data.length ? (
          <p className="text-text-dim py-8 text-xs">No data in this range.</p>
        ) : kind === 'ranked' ? (
          <ol className="space-y-3">
            {data.map((p, i) => (
              <li key={`${p.label}:${i}`} className="text-xs">
                <div className="flex flex-wrap justify-between gap-2">
                  <span>{p.label}</span>
                  <span className="tabular-nums">
                    {formatValue(p.values[series[0].key])} {unit}
                  </span>
                </div>
                <div
                  aria-hidden="true"
                  className="mt-1 h-1"
                  style={{
                    backgroundColor: color(0),
                    width: `${(Math.abs(values[i][0]) / (Math.max(...values.map((row) => Math.abs(row[0]))) || 1)) * 100}%`,
                  }}
                />
              </li>
            ))}
          </ol>
        ) : (
          width !== null &&
          right !== null &&
          right > left &&
          ticks &&
          lo !== undefined &&
          hi !== undefined && (
            <svg
              role="img"
              aria-label={`${title}, ${unit}`}
              width={width}
              height={height}
              viewBox={`0 0 ${width} ${height}`}
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
                  e.preventDefault();
                  const next =
                    (active === null ? data.length - 1 : active) + (e.key === 'ArrowLeft' ? -1 : 1);
                  setActive(next < 0 ? 0 : next >= data.length ? data.length - 1 : next);
                }
                if (e.key === 'Escape') setActive(null);
              }}
              onPointerMove={(e) => {
                const px = e.clientX - e.currentTarget.getBoundingClientRect().left;
                const index = xDomain
                  ? data.reduce(
                      (closest, _, i) =>
                        Math.abs(x(i) - px) < Math.abs(x(closest) - px) ? i : closest,
                      0
                    )
                  : slotBars
                    ? Math.floor(((px - left) / (right - left)) * data.length)
                    : Math.round(((px - left) / (right - left)) * (data.length - 1));
                setActive(index < 0 ? 0 : index >= data.length ? data.length - 1 : index);
              }}
              onPointerLeave={() => setActive(null)}
            >
              {ticks.values.map((v) => (
                <g key={v}>
                  <line x1={left} x2={right} y1={yTick(v)} y2={yTick(v)} stroke="#1a1f30" />
                  <text x={left - 8} y={yTick(v) + 3} textAnchor="end" fill="#7c849c" fontSize="10">
                    {tickLabel(v, ticks.step)}
                  </text>
                </g>
              ))}
              {gaps.map((gap) => {
                const toX = (t: number) =>
                  left + ((t - xDomain![0]) / (xDomain![1] - xDomain![0])) * (right - left);
                const gapLeft = toX(gap.from),
                  gapRight = toX(gap.to);
                return (
                  <g key={`${gap.from}:${gap.to}`} data-chart-gap={gap.label}>
                    <rect
                      x={gapLeft}
                      y={top}
                      width={Math.max(0, gapRight - gapLeft)}
                      height={bottom - top}
                      fill="#10131c"
                    />
                    {gapRight - gapLeft > 120 && (
                      <text
                        x={(gapLeft + gapRight) / 2}
                        y={(top + bottom) / 2}
                        textAnchor="middle"
                        fill="#7c849c"
                        fontSize="10"
                      >
                        {gap.label}
                      </text>
                    )}
                  </g>
                );
              })}
              {series.map((s, si) => {
                const upper = values.map((row) =>
                  kind === 'stacked' ? row.slice(0, si + 1).reduce((sum, v) => sum + v, 0) : row[si]
                );
                const lower = values.map((row) =>
                  kind === 'stacked' ? row.slice(0, si).reduce((sum, v) => sum + v, 0) : 0
                );
                const line = runs
                  .map((run) =>
                    run.map((i, k) => `${k ? 'L' : 'M'}${x(i)},${y(upper[i])}`).join(' ')
                  )
                  .join(' ');
                // F2 charts group: a slot-wide bar for a few labelled buckets (CH-09).
                const slot = (right - left) / Math.max(1, data.length);
                const barWidth = slotBars ? (slot * 0.7) / series.length : 3;
                if (kind === 'bars')
                  return (
                    <g key={s.key}>
                      {upper.map((v, i) => (
                        <rect
                          key={i}
                          x={slotBars ? x(i) - slot * 0.35 + si * barWidth : x(i) - 2 + si * 4}
                          y={y(Math.max(0, v))}
                          width={barWidth}
                          height={Math.abs(y(v) - y(0))}
                          fill={color(si)}
                        />
                      ))}
                    </g>
                  );
                return (
                  <g key={s.key}>
                    {(kind === 'area' || kind === 'stacked') && (
                      <path
                        d={runs
                          .map(
                            (run) =>
                              `${run.map((i, k) => `${k ? 'L' : 'M'}${x(i)},${y(upper[i])}`).join(' ')} ${[
                                ...run,
                              ]
                                .reverse()
                                .map((i) => `L${x(i)},${y(lower[i])}`)
                                .join(' ')} Z`
                          )
                          .join(' ')}
                        fill={kind === 'area' && s.lineOnly ? 'none' : color(si)}
                        fillOpacity={kind === 'stacked' ? 0.6 : 0.12}
                      />
                    )}
                    {projectedAfter === undefined ? (
                      <path
                        d={line}
                        fill="none"
                        stroke={color(si)}
                        strokeWidth="1.5"
                        strokeDasharray={s.dashed ? '4 3' : undefined}
                      />
                    ) : (
                      <>
                        <path
                          d={upper
                            .slice(0, projectedAfter + 1)
                            .map((v, i) => `${i ? 'L' : 'M'}${x(i)},${y(v)}`)
                            .join(' ')}
                          fill="none"
                          stroke={color(si)}
                          strokeWidth="1.5"
                        />
                        <path
                          data-projected
                          d={upper
                            .slice(projectedAfter)
                            .map((v, k) => `${k ? 'L' : 'M'}${x(projectedAfter + k)},${y(v)}`)
                            .join(' ')}
                          fill="none"
                          stroke={color(si)}
                          strokeWidth="1.5"
                          strokeDasharray="4 3"
                        />
                        <circle
                          cx={x(projectedAfter)}
                          cy={y(upper[projectedAfter])}
                          r="2.5"
                          fill={color(si)}
                        />
                      </>
                    )}
                    {runs
                      .filter((run) => run.length === 1)
                      .map(([i]) => (
                        <circle key={i} cx={x(i)} cy={y(upper[i])} r="2" fill={color(si)} />
                      ))}
                    {/* F2 charts group: the line's end and the hovered point (CH-12, CH-22). */}
                    {pointDots && projectedAfter === undefined && kind !== 'stacked' && (
                      <circle
                        data-end-dot={s.key}
                        cx={x(data.length - 1)}
                        cy={y(upper[data.length - 1])}
                        r="3.4"
                        fill={color(si)}
                      />
                    )}
                    {pointDots && activeIndex !== null && (
                      <circle
                        data-hover-dot={s.key}
                        cx={x(activeIndex)}
                        cy={y(upper[activeIndex])}
                        r="4"
                        fill="#0c0e15"
                        stroke={color(si)}
                        strokeWidth="2"
                      />
                    )}
                  </g>
                );
              })}
              {referenceLines.map((line) => (
                <g key={line.label} data-reference-line={line.label}>
                  <line
                    x1={left}
                    x2={right}
                    y1={y(line.value)}
                    y2={y(line.value)}
                    stroke="#a0a8be"
                    strokeOpacity="0.6"
                    strokeDasharray="3 3"
                  />
                  <text
                    x={right - 4}
                    y={y(line.value) - 4}
                    textAnchor="end"
                    fill="#a0a8be"
                    fontSize="10"
                  >
                    {line.label}
                  </text>
                </g>
              ))}
              {markers.map((m) => (
                <g key={`${m.index}:${m.label}`}>
                  <title>{m.label}</title>
                  <line
                    x1={x(m.index)}
                    x2={x(m.index)}
                    y1={top}
                    y2={bottom}
                    stroke="#7c849c"
                    strokeDasharray="3 3"
                  />
                  <text x={x(m.index)} y="10" textAnchor="middle" fill="#7c849c" fontSize="10">
                    {m.plotLabel ?? m.label}
                  </text>
                </g>
              ))}
              {activeIndex !== null && (
                <line
                  x1={
                    cursorX !== undefined && cursorX !== null && xDomain
                      ? left + ((cursorX - xDomain[0]) / (xDomain[1] - xDomain[0])) * (right - left)
                      : x(activeIndex!)
                  }
                  x2={
                    cursorX !== undefined && cursorX !== null && xDomain
                      ? left + ((cursorX - xDomain[0]) / (xDomain[1] - xDomain[0])) * (right - left)
                      : x(activeIndex!)
                  }
                  y1={top}
                  y2={bottom}
                  stroke="#a0a8be"
                />
              )}
              {pointDots &&
                activeIndex !== null &&
                (() => {
                  // F2 charts group: the hover reading sits beside the point, not in the caption (CH-22).
                  const rows = series.map((s) => {
                    const shown = formatValue(data[activeIndex].values[s.key]);
                    if (typeof shown !== 'string' && typeof shown !== 'number')
                      throw new Error(`${title}: an in-plot reading needs a text value`);
                    return `${series.length > 1 ? `${s.label} ` : ''}${shown} ${unit}`;
                  });
                  const head = formatLabel(data[activeIndex].label);
                  const boxWidth = Math.max(head.length, ...rows.map((r) => r.length)) * 6.6 + 20;
                  const boxHeight = 22 + rows.length * 16;
                  const px = x(activeIndex);
                  const bx = px + 14 + boxWidth > right ? px - 14 - boxWidth : px + 14;
                  const by = top + 4;
                  return (
                    <g data-hover-reading pointerEvents="none">
                      <rect
                        x={bx}
                        y={by}
                        width={boxWidth}
                        height={boxHeight}
                        rx="6"
                        fill="#10131c"
                        stroke="#222840"
                      />
                      <text x={bx + 10} y={by + 16} fill="#7c849c" fontSize="11">
                        {head}
                      </text>
                      {rows.map((row, i) => (
                        <text
                          key={row}
                          x={bx + 10}
                          y={by + 33 + i * 16}
                          fill="#dee2ec"
                          fontSize="13"
                        >
                          {row}
                        </text>
                      ))}
                    </g>
                  );
                })()}
              {xTicks?.map((tick) => (
                <text
                  key={`${tick.index}:${tick.label}`}
                  data-x-tick
                  visibility={hideXAxis ? 'hidden' : undefined}
                  x={x(tick.index)}
                  y={height - 5}
                  textAnchor={
                    x(tick.index) - left < 20
                      ? 'start'
                      : right - x(tick.index) < 20
                        ? 'end'
                        : 'middle'
                  }
                  fill="#7c849c"
                  fontSize="10"
                >
                  {tick.label}
                </text>
              ))}
              <text
                visibility={hideXAxis || xTicks ? 'hidden' : undefined}
                x={left}
                y={height - 5}
                fill="#7c849c"
                fontSize="10"
              >
                {data[0].label}
              </text>
              {data.length > 1 && (
                <text
                  visibility={hideXAxis || xTicks ? 'hidden' : undefined}
                  x={right}
                  y={height - 5}
                  textAnchor="end"
                  fill="#7c849c"
                  fontSize="10"
                >
                  {data[data.length - 1].label}
                </text>
              )}
            </svg>
          )
        )}
      </div>
      <span className="sr-only" aria-live="polite">
        {activeIndex !== null && selected
          ? `${selected.label}: ${series.map((s) => `${s.label} ${selected.values[s.key]} ${unit}`).join(', ')}`
          : ''}
      </span>
      {markers.length > 0 && !hideMarkerLegend && (
        <div className="text-text-dim mt-2 flex flex-wrap gap-4 text-[11px]">
          {markers.map((m) => (
            <Link key={`${m.index}:${m.label}`} href={m.href} className="hover:text-jade">
              {m.label} · {data[m.index].label}
            </Link>
          ))}
        </div>
      )}
    </figure>
  );
}
