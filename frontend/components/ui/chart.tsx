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
}
export interface ChartMarker {
  index: number;
  label: string;
  /** Short plot glyph; the full label remains in the accessible legend. */
  plotLabel?: string;
  href: string;
}
export interface ChartProps {
  title: string;
  subtitle: string;
  data: readonly ChartPoint[];
  series: readonly ChartSeries[];
  /** Physical display unit, e.g. PH/s, seconds, CKB. Scale values before passing them. */
  unit: string;
  kind?: 'line' | 'area' | 'stacked' | 'bars' | 'ranked';
  height?: number;
  markers?: readonly ChartMarker[];
  /** Receives the original exact decimal, never geometry converted through Number. */
  formatValue?: (value: string) => ReactNode;
  formatAxis?: (value: number, step: number) => string;
  activeIndex?: number | null;
  onActiveIndexChange?: (index: number | null) => void;
  xDomain?: readonly [number, number];
  plotLeft?: number;
  hideCaption?: boolean;
  hideXAxis?: boolean;
  cursorX?: number | null;
}
export function Chart({
  title,
  subtitle,
  data,
  series,
  unit,
  kind = 'line',
  height = 220,
  markers = [],
  formatValue = (v) => v,
  formatAxis = formatTick,
  activeIndex: controlledActive,
  onActiveIndexChange,
  xDomain,
  plotLeft,
  hideCaption = false,
  hideXAxis = false,
  cursorX,
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
  for (const marker of markers)
    if (!Number.isSafeInteger(marker.index) || marker.index < 0 || marker.index >= data.length)
      throw new Error(`${title}: hardfork ${marker.label} has invalid index ${marker.index}`);
  const color = (i: number) =>
    series[i].color ??
    (series[i].assetClass ? CLASS_COLORS[series[i].assetClass!] : HUE_RAMP[i % HUE_RAMP.length]);
  const activeIndex = active !== null && active < data.length ? active : null;
  const selected = data[activeIndex === null ? data.length - 1 : activeIndex];
  const plotValues =
    kind === 'stacked' ? values.map((row) => row.reduce((s, v) => s + v, 0)) : values.flat();
  const ticks = plotValues.length
    ? niceTicks(
        kind === 'line' ? Math.min(...plotValues) : Math.min(0, ...plotValues),
        kind === 'line' ? Math.max(...plotValues) : Math.max(0, ...plotValues),
        4
      )
    : null;
  const labels = ticks?.values.map((v) => `${formatAxis(v, ticks.step)} ${unit}`) ?? [];
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
      : data.length === 1
        ? 0.5
        : i / (data.length - 1)) *
      ((right as number) - left);
  const y = (v: number) =>
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
            {activeIndex === null ? subtitle : selected?.label}
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
                  : Math.round(((px - left) / (right - left)) * (data.length - 1));
                setActive(index < 0 ? 0 : index >= data.length ? data.length - 1 : index);
              }}
              onPointerLeave={() => setActive(null)}
            >
              {ticks.values.map((v) => (
                <g key={v}>
                  <line x1={left} x2={right} y1={y(v)} y2={y(v)} stroke="#1a1f30" />
                  <text x={left - 8} y={y(v) + 3} textAnchor="end" fill="#7c849c" fontSize="10">
                    {formatAxis(v, ticks.step)} {unit}
                  </text>
                </g>
              ))}
              {series.map((s, si) => {
                const upper = values.map((row) =>
                  kind === 'stacked' ? row.slice(0, si + 1).reduce((sum, v) => sum + v, 0) : row[si]
                );
                const lower = values.map((row) =>
                  kind === 'stacked' ? row.slice(0, si).reduce((sum, v) => sum + v, 0) : 0
                );
                const line = upper.map((v, i) => `${i ? 'L' : 'M'}${x(i)},${y(v)}`).join(' ');
                if (kind === 'bars')
                  return (
                    <g key={s.key}>
                      {upper.map((v, i) => (
                        <rect
                          key={i}
                          x={x(i) - 2 + si * 4}
                          y={y(Math.max(0, v))}
                          width="3"
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
                        d={`${line} ${lower
                          .map((_, index) => {
                            const i = lower.length - 1 - index;
                            return `L${x(i)},${y(lower[i])}`;
                          })
                          .join(' ')} Z`}
                        fill={color(si)}
                        fillOpacity={kind === 'stacked' ? 0.6 : 0.12}
                      />
                    )}
                    <path d={line} fill="none" stroke={color(si)} strokeWidth="1.5" />
                    {data.length === 1 && (
                      <circle cx={x(0)} cy={y(upper[0])} r="2" fill={color(si)} />
                    )}
                  </g>
                );
              })}
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
              <text
                visibility={hideXAxis ? 'hidden' : undefined}
                x={left}
                y={height - 5}
                fill="#7c849c"
                fontSize="10"
              >
                {data[0].label}
              </text>
              {data.length > 1 && (
                <text
                  visibility={hideXAxis ? 'hidden' : undefined}
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
      {markers.length > 0 && (
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
