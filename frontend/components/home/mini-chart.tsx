'use client';

import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import Link from '@/components/ui/link';
import { ReadoutRow } from '@/components/ui/readout-row';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import {
  clipToCap,
  formatCompactSigned,
  formatTick,
  niceTicks,
  signedAxisCap,
} from '@/lib/chart-ticks';
import { usePhosphor } from '@/hooks/usePhosphor';
import { sparkline } from '@/lib/blocks-text';
import { cn } from '@/lib/utils';

/**
 * The homepage small multiples: one figure frame (caption + headline + chart) and four SVG chart
 * kinds — a line, bars from zero, a sparkline of eighth blocks, and signed bars around zero. They share one geometry, one
 * colour set (one data hue), draw 1:1 so their 10 px axis text never scales, and share one hover:
 * the pointer (or ←/→ on the focused chart) picks the nearest bucket, a thin guide marks it, the
 * other marks step back, and the caption's headline and sub-label read it out, as does the readout
 * line under the chart. A tap pins the nearest bucket there until a second tap on it or Escape (R6).
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
  // the row-rule ink: every chart's grid is 1 px solid #1a1f30 (visual finish V13 b)
  grid: 'var(--c-base-border-subtle)',
  base: 'var(--c-text-ghost)',
  tick: 'var(--c-text-dim)',
  reference: 'var(--c-text)',
  data: 'var(--c-jade)',
  down: 'var(--c-rouge)',
  background: 'var(--c-base-bg)',
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

/** What a chart reports to its figure: the bucket it reads, and whether a tap pinned it. */
interface FigureReadout extends Readout {
  pinned: boolean;
}

const ReadoutContext = createContext<(r: FigureReadout | null) => void>(() => {});

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
  /**
   * Whether the figure's readout row carries "tap or hover" (consistency C10). Default true; a
   * section of several figures says it once by its heading and passes false.
   */
  hint?: boolean;
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
  hint = true,
}: ChartFigureProps) {
  const [readout, setReadout] = useState<FigureReadout | null>(null);
  // The headline flares when its served value changes (material pass v2, move 4); a hover's
  // readout, which replaces it, is not a change.
  const flare = usePhosphor(headline);
  const tid = (part: string) => (testId ? `${testId}-${part}` : undefined);
  return (
    <figure data-testid={testId} className={cn('m-0 flex min-w-0 flex-col gap-1.5', className)}>
      <figcaption className="flex items-end justify-between gap-2">
        <span className="flex min-w-0 flex-col">
          {href ? (
            <Link
              href={href}
              className="text-text-bright hover:text-jade text-body block w-fit max-w-full truncate transition-colors"
            >
              {title}
            </Link>
          ) : (
            <span className="text-text-bright text-body block max-w-full truncate">{title}</span>
          )}
          <span data-testid={tid('subtitle')} className="text-text-dim text-small">
            {readout ? readout.label : subtitle}
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-2.5">
          {(readout || headline !== undefined) && (
            <span
              data-testid={tid('headline')}
              className={cn(
                'text-text-bright text-hero font-medium tabular-nums leading-tight',
                !readout && flare
              )}
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
      {body.state === 'ready' && (
        <ReadoutRow
          label={`${title} readout`}
          pinned={readout !== null && readout.pinned}
          hint={hint}
        >
          {readout ? `${readout.label} · ${readout.text}` : null}
        </ReadoutRow>
      )}
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
          className="text-rouge text-small flex items-center"
          style={{ minHeight: CHART_H }}
        >
          {body.message}
        </div>
      );
    case 'empty':
      return (
        <div className="text-text-dim text-small flex items-center" style={{ height: CHART_H }}>
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
  const readout = usePinnedReadout<number>();
  // A press focuses the chart too; only a keyboard focus starts the reading at the last bucket.
  const pressed = useRef(false);
  const xs = width === null ? [] : xsFor(width);
  const latest = useRef({ readoutOf, report });
  latest.current = { readoutOf, report };
  // New data without the held bucket lets it go.
  if (readout.active !== null && readout.active >= xs.length) readout.clear();
  const shown = readout.active !== null && readout.active < xs.length ? readout.active : null;
  const pinned = readout.showingPin;

  // The caption and the readout line read the held bucket, again whenever the data changes.
  useEffect(() => {
    const { readoutOf: read, report: send } = latest.current;
    send(shown === null ? null : { ...read(shown), pinned });
  }, [shown, pinned, signature]);

  // The caption must not keep a readout for a chart that is gone.
  useEffect(() => () => latest.current.report(null), []);

  const bucketAt = (clientX: number) =>
    nearest(xs, clientX - (svgRef.current as SVGSVGElement).getBoundingClientRect().left);
  const onKeyDown = (e: ReactKeyboardEvent<SVGSVGElement>) => {
    const last = xs.length - 1;
    const h = readout.active;
    const moves: Record<string, number> = {
      ArrowLeft: h === null ? last : h - 1,
      ArrowRight: h === null ? last : h + 1,
      Home: 0,
      End: last,
    };
    if (e.key === 'Escape') {
      e.preventDefault();
      readout.clear();
      return;
    }
    if (!(e.key in moves)) return;
    e.preventDefault();
    readout.hover(Math.max(0, Math.min(last, moves[e.key])));
  };

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
          onPointerMove={(e) => readout.hover(bucketAt(e.clientX))}
          onPointerDown={() => {
            pressed.current = true;
          }}
          onPointerCancel={() => {
            pressed.current = false;
          }}
          onPointerLeave={() => readout.hover(null)}
          onClick={(e) => {
            pressed.current = false;
            readout.toggle(bucketAt(e.clientX));
          }}
          onFocus={() => {
            if (!pressed.current && readout.active === null) readout.hover(xs.length - 1);
          }}
          onBlur={() => {
            pressed.current = false;
            readout.hover(null);
          }}
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
  /** Exact formatted value for the caption; value remains SVG geometry only. */
  display?: string;
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
        const text = points[i].display ?? formatValue(points[i].value);
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

/** A bar's readout: its value, and its label — marked when its hour or day is not over yet. */
function barReadout(bars: Bar[], formatValue: (v: number) => string) {
  return (i: number): Readout => {
    const b = bars[i];
    const text = formatValue(b.value);
    return {
      value: text,
      text,
      label: b.state === 'inProgress' ? `${b.label} · in progress` : b.label,
    };
  };
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
  const n = bars.length;
  return (
    <ChartCanvas
      ariaLabel={ariaLabel}
      xsFor={(w) => {
        const g = slots(w, n, BAR_FILL);
        return bars.map((_, i) => g.centre(i));
      }}
      readoutOf={barReadout(bars, formatValue)}
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
// A sparkline of eighth blocks
// ---------------------------------------------------------------------------

/** JetBrains Mono's advance: a glyph is 0.6 em wide. */
const GLYPH_ADVANCE = 0.6;
/** Its block elements stand on the descender, 0.3 em under the baseline, and are 1.32 em tall. */
const GLYPH_DESCENT = 0.3;
const GLYPH_HEIGHT = 1.32;

/** The ink of a spark glyph: the headline's bucket bright, an unfinished one faint. */
const SPARK_INK: Record<BarState, string> = {
  complete: 'var(--c-jade)',
  highlight: 'var(--c-jade-bright)',
  inProgress: 'var(--c-text-ghost)',
};

/**
 * Counts as a sparkline (rubbing × Song print, phase 2): one eighth block per bucket
 * (lib/blocks-text `sparkline`, from zero to the largest), set in the mono at the size that makes
 * each glyph one slot wide, standing on the plot's floor; the newest complete bucket bright, the
 * unfinished ones faint. It sits on the same canvas as the bars it replaces, so the pointer, the
 * keys and a tap read a bucket out the same way.
 */
export function SparkChart({ bars, ariaLabel, formatValue }: BarChartProps) {
  if (bars.length === 0) throw new Error('SparkChart needs at least one bar');
  const glyphs = [...sparkline(bars.map((b) => b.value))];
  const n = bars.length;
  return (
    <ChartCanvas
      ariaLabel={ariaLabel}
      xsFor={(w) => {
        const g = slots(w, n, 1);
        return bars.map((_, i) => g.centre(i));
      }}
      readoutOf={barReadout(bars, formatValue)}
      signature={signatureOf(bars)}
    >
      {(width, hovered) => {
        const { right, bw, centre } = slots(width, n, 1);
        // one glyph a slot wide, never taller than the plot
        const size = Math.min(bw / GLYPH_ADVANCE, PLOT_H / GLYPH_HEIGHT);
        const mid = Math.floor(n / 2);
        return (
          <>
            <g data-spark="" fontSize={f1(size)} textAnchor="middle">
              {bars.map((b, i) => (
                <text
                  key={i}
                  data-bucket={i}
                  data-bar={b.state}
                  data-hovered={hovered === i || undefined}
                  x={f1(centre(i))}
                  y={f1(BOTTOM - GLYPH_DESCENT * size)}
                  fill={SPARK_INK[b.state]}
                  opacity={bucketOpacity(i, hovered)}
                >
                  {glyphs[i]}
                </text>
              ))}
            </g>
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
