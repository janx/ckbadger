'use client';

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react';
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
 * colour set (one data hue), draw 1:1 so their 10 px axis text never scales, and share one hover:
 * the pointer (or a tap, or ←/→ on the focused chart) picks the nearest bucket, a thin guide marks
 * it, the other marks step back, and the caption's headline and sub-label read it out.
 */

export const CHART_H = 104;
const TOP = 10;
const BOTTOM = 84;
const LEFT = 34;
const RIGHT_PAD = 8;
const TICK_X = 28;
const X_LABEL_Y = 101;
const PLOT_H = BOTTOM - TOP;
/** Marks other than the hovered one step back to this opacity. */
const DIMMED = 0.35;

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
// Readout: what the hovered bucket shows in the caption
// ---------------------------------------------------------------------------

/** A bucket's value for the caption (`value`, may carry dim decimals) and its plain text. */
export interface ReadoutValue {
  value: ReactNode;
  text: string;
}

interface Readout extends ReadoutValue {
  /** The bucket's date or hour, shown in place of the sub-label. */
  label: string;
}

const ReadoutContext = createContext<(r: Readout | null) => void>(() => {});

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
  const [readout, setReadout] = useState<Readout | null>(null);
  const tid = (part: string) => (testId ? `${testId}-${part}` : undefined);
  return (
    <figure data-testid={testId} className={cn('m-0 flex min-w-0 flex-col gap-1.5', className)}>
      <figcaption className="flex items-end justify-between gap-2">
        <span className="flex min-w-0 flex-col">
          {href ? (
            <Link
              href={href}
              className="text-text-bright hover:text-jade block w-fit max-w-full truncate text-[13px] transition-colors"
            >
              {title}
            </Link>
          ) : (
            <span className="text-text-bright block max-w-full truncate text-[13px]">{title}</span>
          )}
          <span data-testid={tid('subtitle')} className="text-text-dim text-xs">
            {readout ? readout.label : subtitle}
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-2.5">
          {(readout || headline !== undefined) && (
            <span
              data-testid={tid('headline')}
              className="text-text-bright text-lg font-medium tabular-nums leading-tight"
            >
              {readout ? readout.value : headline}
            </span>
          )}
          {controls}
        </span>
      </figcaption>
      <span data-testid={tid('readout')} aria-live="polite" className="sr-only">
        {readout ? `${readout.label}: ${readout.text}` : ''}
      </span>
      <ReadoutContext.Provider value={setReadout}>
        <FigureBody body={body} />
      </ReadoutContext.Provider>
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
// Shared canvas: geometry, hover, touch, keyboard
// ---------------------------------------------------------------------------

interface CanvasProps {
  ariaLabel: string;
  /** Each bucket's x centre for a given width; the pointer snaps to the nearest. */
  xsFor: (width: number) => number[];
  readoutOf: (index: number) => Readout;
  /** Changes whenever the data does, so a held bucket's readout follows a refetch. */
  signature: string;
  children: (width: number, hovered: number | null) => ReactNode;
}

function nearest(xs: number[], x: number): number {
  let best = 0;
  for (let i = 1; i < xs.length; i++) {
    if (Math.abs(xs[i] - x) < Math.abs(xs[best] - x)) best = i;
  }
  return best;
}

function ChartCanvas({ ariaLabel, xsFor, readoutOf, signature, children }: CanvasProps) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const svgRef = useRef<SVGSVGElement>(null);
  const report = useContext(ReadoutContext);
  const [hovered, setHovered] = useState<number | null>(null);
  // Handlers read the latest state through refs: a pointerdown and the focus it causes arrive in
  // one task, before a re-render.
  const hoveredRef = useRef<number | null>(null);
  const byTouch = useRef(false);
  const xs = width === null ? [] : xsFor(width);
  const latest = useRef({ xs, readoutOf, report });
  latest.current = { xs, readoutOf, report };

  const select = useCallback((i: number | null) => {
    const { xs: cur, readoutOf: read, report: send } = latest.current;
    const next = i === null || cur.length === 0 ? null : Math.max(0, Math.min(cur.length - 1, i));
    hoveredRef.current = next;
    setHovered(next);
    send(next === null ? null : read(next));
  }, []);

  // New data under a held bucket: re-read it (or let go when the bucket is gone).
  useEffect(() => {
    const i = hoveredRef.current;
    if (i === null) return;
    if (i >= latest.current.xs.length) select(null);
    else latest.current.report(latest.current.readoutOf(i));
  }, [signature, select]);

  // The caption must not keep a readout for a chart that is gone.
  useEffect(() => () => latest.current.report(null), []);

  // A tapped bucket stays until the next tap outside the chart.
  useEffect(() => {
    if (hovered === null || !byTouch.current) return;
    const away = (e: Event) => {
      if (!svgRef.current?.contains(e.target as Node)) select(null);
    };
    document.addEventListener('pointerdown', away);
    return () => document.removeEventListener('pointerdown', away);
  }, [hovered, select]);

  const pick = (e: ReactPointerEvent<SVGSVGElement>) => {
    byTouch.current = e.pointerType === 'touch';
    const left = (svgRef.current as SVGSVGElement).getBoundingClientRect().left;
    select(nearest(latest.current.xs, e.clientX - left));
  };
  const onKeyDown = (e: ReactKeyboardEvent<SVGSVGElement>) => {
    const last = latest.current.xs.length - 1;
    const h = hoveredRef.current;
    const moves: Record<string, number | null> = {
      ArrowLeft: h === null ? last : h - 1,
      ArrowRight: h === null ? last : h + 1,
      Home: 0,
      End: last,
      Escape: null,
    };
    if (!(e.key in moves)) return;
    e.preventDefault();
    byTouch.current = false;
    select(moves[e.key]);
  };

  const shown = hovered !== null && hovered < xs.length ? hovered : null;
  return (
    <div ref={ref} className="w-full" style={{ height: CHART_H }}>
      {width !== null && (
        <svg
          ref={svgRef}
          role="img"
          aria-label={ariaLabel}
          tabIndex={0}
          width={width}
          height={CHART_H}
          viewBox={`0 0 ${width} ${CHART_H}`}
          className="focus-visible:ring-base-border block touch-pan-y overflow-visible rounded-sm font-mono tabular-nums outline-none focus-visible:ring-1"
          onPointerMove={pick}
          onPointerDown={pick}
          onPointerLeave={(e) => {
            if (e.pointerType !== 'touch') select(null);
          }}
          onFocus={() => {
            if (hoveredRef.current === null) select(latest.current.xs.length - 1);
          }}
          onBlur={() => select(null)}
          onKeyDown={onKeyDown}
        >
          {shown !== null && (
            <line
              data-role="guide"
              x1={f1(xs[shown])}
              x2={f1(xs[shown])}
              y1={TOP}
              y2={BOTTOM}
              stroke={C.tick}
              strokeOpacity={0.6}
            />
          )}
          {children(width, shown)}
        </svg>
      )}
    </div>
  );
}

/** Opacity of bucket `i` while `hovered` is: full when it is the one (or none is), else dimmed. */
const bucketOpacity = (i: number, hovered: number | null) =>
  hovered === null || hovered === i ? 1 : DIMMED;

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

/** Slot geometry shared by both bar kinds: `n` equal slots, bars `fill` of a slot wide. */
function slots(width: number, n: number, fill: number) {
  const right = width - RIGHT_PAD;
  const slot = (right - LEFT) / n;
  const bw = slot * fill;
  const bx = (i: number) => LEFT + slot * i + (slot - bw) / 2;
  return { right, bw, bx, centre: (i: number) => bx(i) + bw / 2 };
}

const signatureOf = (items: { label: string; value: number }[]) =>
  items.map((p) => `${p.label}=${p.value}`).join('|');

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
  /** A value as the caption shows it. */
  formatValue: (v: number) => string;
}

function lineXs(width: number, n: number): number[] {
  const right = width - RIGHT_PAD;
  return Array.from({ length: n }, (_, i) =>
    n === 1 ? right : LEFT + (i * (right - LEFT)) / (n - 1)
  );
}

export function LineChart({ points, ariaLabel, reference, formatValue }: LineChartProps) {
  if (points.length === 0) throw new Error('LineChart needs at least one point');
  const values = points.map((p) => p.value);
  const lo = Math.min(...values, ...(reference === undefined ? [] : [reference]));
  const hi = Math.max(...values, ...(reference === undefined ? [] : [reference]));
  const ticks = niceTicks(lo, hi, 4, reference ?? 0);
  const domain: [number, number] = [ticks.values[0], ticks.values[ticks.values.length - 1]];
  const y = yScale(domain);
  const n = points.length;
  return (
    <ChartCanvas
      ariaLabel={ariaLabel}
      xsFor={(w) => lineXs(w, n)}
      readoutOf={(i) => {
        const text = formatValue(points[i].value);
        return { value: text, text, label: points[i].label };
      }}
      signature={signatureOf(points)}
    >
      {(width, hovered) => {
        const right = width - RIGHT_PAD;
        const xs = lineXs(width, n);
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
              points={points.map((p, i) => `${f1(xs[i])},${f1(y(p.value))}`).join(' ')}
              fill="none"
              stroke={C.data}
              strokeWidth={1.5}
              strokeLinejoin="round"
            />
            <circle
              data-role="last"
              cx={f1(xs[n - 1])}
              cy={f1(y(last.value))}
              r={2.6}
              fill={C.data}
            />
            {hovered !== null && (
              <circle
                data-role="hover-dot"
                cx={f1(xs[hovered])}
                cy={f1(y(points[hovered].value))}
                r={3.8}
                fill={C.data}
                stroke={C.background}
                strokeWidth={1.5}
              />
            )}
            <XAxisLabels
              labels={xLabels(
                points.map((p) => p.label),
                mid,
                xs[mid],
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
  /** A value as the caption shows it. */
  formatValue: (v: number) => string;
}

const BAR_FILL = 0.72;

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
  const n = bars.length;
  return (
    <ChartCanvas
      ariaLabel={ariaLabel}
      xsFor={(w) => {
        const g = slots(w, n, BAR_FILL);
        return bars.map((_, i) => g.centre(i));
      }}
      readoutOf={(i) => {
        const b = bars[i];
        const text = formatValue(b.value);
        return {
          value: text,
          text,
          label: b.state === 'inProgress' ? `${b.label} · in progress` : b.label,
        };
      }}
      signature={signatureOf(bars)}
    >
      {(width, hovered) => {
        const { right, bw, bx, centre } = slots(width, n, BAR_FILL);
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
              const on = hovered === i;
              return (
                <g
                  key={i}
                  data-bucket={i}
                  data-hovered={on || undefined}
                  opacity={bucketOpacity(i, hovered)}
                >
                  {b.state === 'inProgress' ? (
                    <rect
                      data-bar={b.state}
                      x={f1(bx(i) + 0.5)}
                      y={f1(Math.min(top, BOTTOM - 1) + 0.5)}
                      width={f1(Math.max(bw - 1, 1))}
                      height={f1(Math.max(BOTTOM - top - 1, 0.5))}
                      fill={C.data}
                      fillOpacity={on ? 0.25 : 0.1}
                      stroke={C.data}
                      strokeOpacity={on ? 1 : 0.7}
                      strokeDasharray="2 1.5"
                    />
                  ) : (
                    <rect
                      data-bar={b.state}
                      x={f1(bx(i))}
                      y={f1(top)}
                      width={f1(bw)}
                      height={f1(BOTTOM - top)}
                      fill={C.data}
                      fillOpacity={on || b.state === 'highlight' ? 1 : 0.45}
                    />
                  )}
                </g>
              );
            })}
            <XAxisLabels
              labels={xLabels(
                bars.map((b) => b.label),
                mid,
                centre(mid),
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
  /** The exact change, as the caption reads it out. */
  readout: ReadoutValue;
}

interface SignedBarChartProps {
  points: SignedPoint[];
  ariaLabel: string;
}

const SIGNED_FILL = 0.7;

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
  const n = points.length;
  return (
    <ChartCanvas
      ariaLabel={label}
      xsFor={(w) => {
        const g = slots(w, n, SIGNED_FILL);
        return points.map((_, i) => g.centre(i));
      }}
      readoutOf={(i) => ({ ...points[i].readout, label: points[i].label })}
      signature={signatureOf(points)}
    >
      {(width, hovered) => {
        const { right, bw, bx, centre } = slots(width, n, SIGNED_FILL);
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
              const on = hovered === i;
              return (
                <g
                  key={i}
                  data-bucket={i}
                  data-hovered={on || undefined}
                  data-bar={up ? 'up' : 'down'}
                  data-clipped={b.clipped || undefined}
                  opacity={bucketOpacity(i, hovered)}
                >
                  <rect
                    x={f1(x0)}
                    y={f1(Math.min(zero, end))}
                    width={f1(bw)}
                    height={f1(Math.abs(end - zero))}
                    fill={color}
                    fillOpacity={on ? 1 : 0.75}
                  />
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
                centre(mid),
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
