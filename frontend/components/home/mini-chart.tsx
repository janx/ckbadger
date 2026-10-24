'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { useElementWidth } from '@/hooks/useElementWidth';
import {
  clipToCap,
  formatCompactSigned,
  formatTick,
  niceTicks,
  signedAxisCap,
} from '@/lib/chart-ticks';
import { cn } from '@/lib/utils';

/**
 * The homepage small multiples: one figure frame (caption + headline + chart) and three SVG chart
 * kinds — a line, bars from zero, and signed bars around zero. They share one geometry, one
 * colour set (one data hue), and draw 1:1 so their 10 px axis text never scales.
 */

export const CHART_H = 104;
const TOP = 10;
const BOTTOM = 84;
const LEFT = 34;
const RIGHT_PAD = 8;
const TICK_X = 28;
const X_LABEL_Y = 101;
const PLOT_H = BOTTOM - TOP;

const C = {
  grid: '#1d2233',
  base: '#343c50',
  tick: '#7c849c',
  reference: '#a0a8be',
  data: '#2edba3',
  down: '#e8555a',
  background: '#0c0e15',
} as const;

// ---------------------------------------------------------------------------
// Figure frame
// ---------------------------------------------------------------------------

export type ChartBody =
  | { state: 'loading' }
  | { state: 'error'; message: string }
  | { state: 'empty'; message: string }
  | { state: 'ready'; chart: ReactNode };

interface ChartFigureProps {
  title: string;
  /** Where the title links to: the full chart. */
  href?: string;
  subtitle: string;
  /** The headline value, right-aligned in the caption. */
  headline?: string;
  /** A control beside the headline, e.g. a range toggle. */
  controls?: ReactNode;
  body: ChartBody;
  className?: string;
  testId?: string;
}

export function ChartFigure({
  title,
  href,
  subtitle,
  headline,
  controls,
  body,
  className,
  testId,
}: ChartFigureProps) {
  return (
    <figure data-testid={testId} className={cn('m-0 flex min-w-0 flex-col gap-1.5', className)}>
      <figcaption className="flex items-end justify-between gap-3">
        <span className="flex min-w-0 flex-col">
          {href ? (
            <Link
              href={href}
              className="text-text-bright hover:text-jade w-fit text-[13px] transition-colors"
            >
              {title}
            </Link>
          ) : (
            <span className="text-text-bright text-[13px]">{title}</span>
          )}
          <span className="text-text-dim text-xs">{subtitle}</span>
        </span>
        <span className="flex shrink-0 items-center gap-2.5">
          {headline !== undefined && (
            <span
              data-testid={testId ? `${testId}-headline` : undefined}
              className="text-text-bright text-lg font-medium tabular-nums leading-tight"
            >
              {headline}
            </span>
          )}
          {controls}
        </span>
      </figcaption>
      <FigureBody body={body} />
    </figure>
  );
}

function FigureBody({ body }: { body: ChartBody }) {
  switch (body.state) {
    case 'loading':
      return (
        <div
          aria-hidden="true"
          className="bg-base-surface animate-pulse rounded"
          style={{ height: CHART_H }}
        />
      );
    case 'error':
      return (
        <div
          role="alert"
          className="text-rouge flex items-center text-xs"
          style={{ minHeight: CHART_H }}
        >
          {body.message}
        </div>
      );
    case 'empty':
      return (
        <div className="text-text-dim flex items-center text-xs" style={{ height: CHART_H }}>
          {body.message}
        </div>
      );
    case 'ready':
      return <>{body.chart}</>;
  }
}

// ---------------------------------------------------------------------------
// Shared geometry
// ---------------------------------------------------------------------------

function ChartCanvas({
  ariaLabel,
  children,
}: {
  ariaLabel: string;
  children: (width: number) => ReactNode;
}) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  return (
    <div ref={ref} className="w-full" style={{ height: CHART_H }}>
      {width !== null && (
        <svg
          role="img"
          aria-label={ariaLabel}
          width={width}
          height={CHART_H}
          viewBox={`0 0 ${width} ${CHART_H}`}
          className="block overflow-visible font-mono tabular-nums"
        >
          {children(width)}
        </svg>
      )}
    </div>
  );
}

const f1 = (n: number) => n.toFixed(1);

function yScale(domain: [number, number]) {
  const [d0, d1] = domain;
  return (v: number) => BOTTOM - ((v - d0) / (d1 - d0)) * PLOT_H;
}

interface XLabel {
  x: number;
  text: string;
  anchor: 'start' | 'middle' | 'end';
}

/** First, middle and last labels, anchored so none is clipped at an edge. */
function xLabels(labels: string[], midIndex: number, midX: number, right: number): XLabel[] {
  const n = labels.length;
  if (n === 0) return [];
  if (n === 1) return [{ x: right, text: labels[0], anchor: 'end' }];
  const out: XLabel[] = [{ x: LEFT, text: labels[0], anchor: 'start' }];
  if (n >= 3) out.push({ x: midX, text: labels[midIndex], anchor: 'middle' });
  out.push({ x: right, text: labels[n - 1], anchor: 'end' });
  return out;
}

function XAxisLabels({ labels }: { labels: XLabel[] }) {
  return (
    <g fill={C.tick} fontSize={10} data-axis="x">
      {labels.map((l, i) => (
        <text key={i} x={f1(l.x)} y={X_LABEL_Y} textAnchor={l.anchor}>
          {l.text}
        </text>
      ))}
    </g>
  );
}

interface YTick {
  y: number;
  text: string;
  emphasis?: boolean;
}

function YAxisLabels({ ticks }: { ticks: YTick[] }) {
  return (
    <g fontSize={10} textAnchor="end" data-axis="y">
      {ticks.map((t, i) => (
        <text key={i} x={TICK_X} y={f1(t.y + 3)} fill={t.emphasis ? C.reference : C.tick}>
          {t.text}
        </text>
      ))}
    </g>
  );
}

// ---------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------

export interface SeriesPoint {
  label: string;
  value: number;
}

interface LineChartProps {
  points: SeriesPoint[];
  ariaLabel: string;
  /** A dashed reference line (a consensus target); its value is placed on a tick. */
  reference?: number;
  formatValue: (v: number) => string;
}

export function LineChart({ points, ariaLabel, reference, formatValue }: LineChartProps) {
  if (points.length === 0) throw new Error('LineChart needs at least one point');
  const values = points.map((p) => p.value);
  const lo = Math.min(...values, ...(reference === undefined ? [] : [reference]));
  const hi = Math.max(...values, ...(reference === undefined ? [] : [reference]));
  const ticks = niceTicks(lo, hi, 4, reference ?? 0);
  const domain: [number, number] = [ticks.values[0], ticks.values[ticks.values.length - 1]];
  const y = yScale(domain);
  return (
    <ChartCanvas ariaLabel={ariaLabel}>
      {(width) => {
        const right = width - RIGHT_PAD;
        const n = points.length;
        const x = (i: number) => (n === 1 ? right : LEFT + (i * (right - LEFT)) / (n - 1));
        const mid = Math.floor((n - 1) / 2);
        const isRef = (v: number) => reference !== undefined && Math.abs(v - reference) < 1e-9;
        const last = points[n - 1];
        return (
          <>
            <path
              data-role="grid"
              d={ticks.values
                .filter((v) => !isRef(v))
                .map((v) => `M${LEFT} ${f1(y(v))}H${f1(right)}`)
                .join('')}
              stroke={C.grid}
            />
            {reference !== undefined && (
              <path
                data-role="reference"
                d={`M${LEFT} ${f1(y(reference))}H${f1(right)}`}
                stroke={C.reference}
                strokeOpacity={0.7}
                strokeDasharray="4 3"
              />
            )}
            <YAxisLabels
              ticks={ticks.values.map((v) => ({
                y: y(v),
                text: formatTick(v, ticks.step),
                emphasis: isRef(v),
              }))}
            />
            <polyline
              data-role="series"
              points={points.map((p, i) => `${f1(x(i))},${f1(y(p.value))}`).join(' ')}
              fill="none"
              stroke={C.data}
              strokeWidth={1.5}
              strokeLinejoin="round"
            />
            <circle data-role="last" cx={f1(x(n - 1))} cy={f1(y(last.value))} r={2.6} fill={C.data}>
              <title>{`${last.label}: ${formatValue(last.value)}`}</title>
            </circle>
            <XAxisLabels
              labels={xLabels(
                points.map((p) => p.label),
                mid,
                x(mid),
                right
              )}
            />
          </>
        );
      }}
    </ChartCanvas>
  );
}

// ---------------------------------------------------------------------------
// Bars from zero
// ---------------------------------------------------------------------------

/** `highlight` is the bar the headline reads; `inProgress` an hour / day not over yet (hollow). */
export type BarState = 'complete' | 'highlight' | 'inProgress';

export interface Bar {
  label: string;
  value: number;
  state: BarState;
}

interface BarChartProps {
  bars: Bar[];
  ariaLabel: string;
  formatValue: (v: number) => string;
}

export function BarChart({ bars, ariaLabel, formatValue }: BarChartProps) {
  if (bars.length === 0) throw new Error('BarChart needs at least one bar');
  bars.forEach((b) => {
    if (!Number.isFinite(b.value) || b.value < 0) {
      throw new Error(`bar ${b.label} has value ${b.value}; bars start at zero`);
    }
  });
  // Three ticks, as the mockup draws bars: 0 / 500 / 1k.
  const ticks = niceTicks(0, Math.max(...bars.map((b) => b.value)), 3);
  const domain: [number, number] = [0, ticks.values[ticks.values.length - 1]];
  const y = yScale(domain);
  return (
    <ChartCanvas ariaLabel={ariaLabel}>
      {(width) => {
        const right = width - RIGHT_PAD;
        const n = bars.length;
        const slot = (right - LEFT) / n;
        const bw = slot * 0.72;
        const bx = (i: number) => LEFT + slot * i + (slot - bw) / 2;
        const mid = Math.floor(n / 2);
        return (
          <>
            <path
              data-role="grid"
              d={ticks.values
                .filter((v) => v !== 0)
                .map((v) => `M${LEFT} ${f1(y(v))}H${f1(right)}`)
                .join('')}
              stroke={C.grid}
            />
            <path data-role="base" d={`M${LEFT} ${BOTTOM}H${f1(right)}`} stroke={C.base} />
            <YAxisLabels
              ticks={ticks.values.map((v) => ({ y: y(v), text: formatTick(v, ticks.step) }))}
            />
            {bars.map((b, i) => {
              const top = y(b.value);
              const title = <title>{`${b.label}: ${formatValue(b.value)}`}</title>;
              if (b.state === 'inProgress') {
                return (
                  <rect
                    key={i}
                    data-bar={b.state}
                    x={f1(bx(i) + 0.5)}
                    y={f1(Math.min(top, BOTTOM - 1) + 0.5)}
                    width={f1(Math.max(bw - 1, 1))}
                    height={f1(Math.max(BOTTOM - top - 1, 0.5))}
                    fill={C.data}
                    fillOpacity={0.1}
                    stroke={C.data}
                    strokeOpacity={0.7}
                    strokeDasharray="2 1.5"
                  >
                    {title}
                  </rect>
                );
              }
              return (
                <rect
                  key={i}
                  data-bar={b.state}
                  x={f1(bx(i))}
                  y={f1(top)}
                  width={f1(bw)}
                  height={f1(BOTTOM - top)}
                  fill={C.data}
                  fillOpacity={b.state === 'highlight' ? 1 : 0.45}
                >
                  {title}
                </rect>
              );
            })}
            <XAxisLabels
              labels={xLabels(
                bars.map((b) => b.label),
                mid,
                bx(mid) + bw / 2,
                right
              )}
            />
          </>
        );
      }}
    </ChartCanvas>
  );
}

// ---------------------------------------------------------------------------
// Signed bars around zero
// ---------------------------------------------------------------------------

export interface SignedPoint {
  label: string;
  /** The change, in display units (CKB) — geometry only. */
  value: number;
  /** The exact change, for the bar's tooltip. */
  exact: string;
}

interface SignedBarChartProps {
  points: SignedPoint[];
  ariaLabel: string;
}

/**
 * Up bars jade, down bars red, on a symmetric axis; a bar beyond the cap is clipped and labelled,
 * and the aria-label names the clipped days.
 */
export function SignedBarChart({ points, ariaLabel }: SignedBarChartProps) {
  if (points.length === 0) throw new Error('SignedBarChart needs at least one bar');
  const cap = signedAxisCap(points.map((p) => p.value));
  const bars = clipToCap(
    points.map((p) => p.value),
    cap
  );
  const capStep = cap;
  const zero = (TOP + BOTTOM) / 2;
  const half = PLOT_H / 2;
  const yOf = (v: number) => zero - (v / cap) * half;
  const clipped = bars.flatMap((b, i) =>
    b.clipped ? [`${formatCompactSigned(b.value)} on ${points[i].label}`] : []
  );
  const label =
    clipped.length === 0
      ? ariaLabel
      : `${ariaLabel}; beyond the ±${formatTick(cap, capStep)} axis, drawn clipped: ${clipped.join(', ')}`;
  return (
    <ChartCanvas ariaLabel={label}>
      {(width) => {
        const right = width - RIGHT_PAD;
        const n = points.length;
        const slot = (right - LEFT) / n;
        const bw = slot * 0.7;
        const bx = (i: number) => LEFT + slot * i + (slot - bw) / 2;
        const mid = Math.floor(n / 2);
        return (
          <>
            <path
              data-role="grid"
              d={`M${LEFT} ${TOP}H${f1(right)}M${LEFT} ${BOTTOM}H${f1(right)}`}
              stroke={C.grid}
              strokeDasharray="2 3"
            />
            <path data-role="base" d={`M${LEFT} ${zero}H${f1(right)}`} stroke={C.base} />
            <YAxisLabels
              ticks={[
                { y: TOP, text: formatTick(cap, capStep) },
                { y: zero, text: '0' },
                { y: BOTTOM, text: formatTick(-cap, capStep) },
              ]}
            />
            {bars.map((b, i) => {
              const up = b.value >= 0;
              const end = yOf(b.drawn);
              const x0 = bx(i);
              const color = up ? C.data : C.down;
              return (
                <g key={i} data-bar={up ? 'up' : 'down'} data-clipped={b.clipped || undefined}>
                  <rect
                    x={f1(x0)}
                    y={f1(Math.min(zero, end))}
                    width={f1(bw)}
                    height={f1(Math.abs(end - zero))}
                    fill={color}
                    fillOpacity={0.75}
                  >
                    <title>{`${points[i].label}: ${points[i].exact}`}</title>
                  </rect>
                  {b.clipped && (
                    <ClipMark
                      x0={x0}
                      bw={bw}
                      up={up}
                      color={color}
                      text={formatCompactSigned(b.value)}
                      right={right}
                    />
                  )}
                </g>
              );
            })}
            <XAxisLabels
              labels={xLabels(
                points.map((p) => p.label),
                mid,
                bx(mid) + bw / 2,
                right
              )}
            />
          </>
        );
      }}
    </ChartCanvas>
  );
}

/** Two background-coloured slashes across a clipped bar's end, and its value beside it. */
function ClipMark({
  x0,
  bw,
  up,
  color,
  text,
  right,
}: {
  x0: number;
  bw: number;
  up: boolean;
  color: string;
  text: string;
  right: number;
}) {
  const ys = up ? [TOP + 6.5, TOP + 9.5] : [BOTTOM - 6.5, BOTTOM - 3.5];
  const sx = x0 - 1;
  const len = bw + 2;
  // 10 px JetBrains Mono is about 6 px a character.
  const labelW = text.length * 6;
  const after = x0 + bw + 3.5;
  const fits = after + labelW <= right;
  return (
    <>
      <path
        data-role="break"
        d={ys.map((yy) => `M${f1(sx)} ${f1(yy)}l${f1(len)}-2`).join('')}
        stroke={C.background}
        strokeWidth={1.6}
      />
      <text
        data-role="clip-label"
        x={f1(fits ? after : x0 - 3.5)}
        y={up ? TOP + 8 : BOTTOM - 1}
        textAnchor={fits ? 'start' : 'end'}
        fill={color}
        fontSize={10}
      >
        {text}
      </text>
    </>
  );
}
