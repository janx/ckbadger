'use client';
import { Fragment, useId, useState, type CSSProperties, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ReadoutRow } from '@/components/ui/readout-row';
import { useScanFill } from '@/components/ui/scan-fill';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout, type PinnedReadout } from '@/hooks/usePinnedReadout';
import {
  AXIS_CHAR_PX,
  niceTicks,
  formatTick,
  placeXTicks,
  tickGutter,
  type Ticks,
} from '@/lib/chart-ticks';
import { exactSignedDecimalRatio } from '@/lib/exact-integer';
import { CLASS_COLORS, jadeRamp, type AssetClass } from '@/lib/design-tokens';
import { CHART_GRID_COLOR } from '@/lib/chart-colors';
import { unitAfter, withUnit } from '@/lib/format-unit';
import { SVG_TEXT, centreShift, wrapWords } from '@/lib/chart-type';
import { textPx, tightLinePx } from '@/lib/type-metrics';

export interface ChartPoint {
  label: string;
  values: Readonly<Record<string, string>>;
  /** Optional exact time coordinate for plots sharing one time axis. */
  x?: number;
  /**
   * A stack's whole at this point as the API states it, which every share a chart page states of
   * the point divides by (components/charts/chart-model.ts). Not drawn.
   */
  whole?: string;
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
  /**
   * X-axis labels at these point indexes, replacing the first / last label pair. `edge`: a
   * histogram's open-ended outlier label, always kept (placeXTicks).
   */
  xTicks?: readonly { index: number; label: string; edge?: boolean }[];
  /**
   * The x axis names categories (a few labelled buckets, one slot bar each): `xTicks` names every
   * point, and every label is drawn, on extra rows under the plot when one row cannot hold them.
   */
  categoricalX?: boolean;
  /** A dot at the end of each line, and at the hovered point with an in-plot reading. */
  pointDots?: boolean;
  /** Bars fill their slot (a few labelled buckets) instead of 3 px ticks. */
  slotBars?: boolean;
  /** Keep the markers' in-plot labels but drop the link row under the plot. */
  hideMarkerLegend?: boolean;
  /** How a point label reads wherever the chart prints it (hover caption and reading). */
  formatLabel?: (label: string) => string;
  /**
   * Key the series under the plot — swatch · name · value at the reading, one per line, in band
   * order — instead of reading them out as headline numbers in the caption (a chart of many
   * bands, responsive review C#5).
   */
  seriesLegend?: boolean;
  /**
   * Tap to pin (plan R6): the readout line under the plot, by this accessible name. A tap (click)
   * on the plot pins the nearest point's reading there; a second tap on that point, or Escape,
   * clears it. `idle` is what the line says while nothing is hovered or pinned.
   */
  pinnedReadout?: { label: string; idle?: ReactNode };
  /**
   * The tap-to-pin state, for a caller that draws its own readout line from it (`usePinnedReadout`
   * over point indexes): the chart hovers and pins through it and shows its `active` point.
   */
  pin?: PinnedReadout<number>;
  // --- hierarchy v3 (DAO page, R13): every one opt-in ---
  /** The chart's name written inside the plot, top left (meta, dim); no caption is drawn then. */
  plotName?: string;
  /**
   * Round values written inside the plot from its left edge (ghost ink), with no axis gutter.
   * `'over'` (lane P, additive): the same, the labels written over the plotted series — for a
   * filled chart (a stacked area is 60 % opaque) whose fill would bury a label drawn under it; the
   * grid lines stay under the series either way.
   */
  insideYTicks?: boolean | 'over';
  /**
   * Lane A5 (UI review 2026-11-14, U-19): the y ticks are bare values ("1.5k"); the unit is said
   * once, in `plotName`, which is then required. Off (the default), every tick carries its unit.
   */
  bareTicks?: boolean;
  /** A count axis: whole-number ticks only, the step at least 1 (never "0.5 transfers"). */
  integerTicks?: boolean;
  /** The first series' latest value written at the line's end, from its exact decimal. */
  endLabel?: (value: string) => string;
  // --- lane D1, the chart pages on the chart-hero board: every one opt-in ---
  /**
   * A stack's bands named at their right ends (rule 5): each series' name and its stated figure at
   * the last point (by series key), in a column right of the plot where the plot keeps three fifths
   * of the width, else a key under the plot in the stack's order, top band first. No word on a band.
   */
  bandEnds?: Readonly<Record<string, string>>;
  /** A stack drawn in solid fills (rule 5: a dark band clears the night ground by 1.5 : 1), not at 60 %. */
  solidBands?: boolean;
  /** A chart under the main one (rule 7): its line without the halo, so it is never the heaviest mark. */
  quiet?: boolean;
  /**
   * Rule 6: the value axis stops at `value`, the largest value of the points that set it; a mark
   * above is cut at the plot's top under a gold tick, and `note` (its exact value, in words) is
   * written over the plot under its name, which is then required. No figure changes.
   */
  axisCap?: { value: number; note: string };
}
/**
 * Midnight, continued (move 5): the attributes that give a chart's primary data line its halo —
 * `.chart-line-halo`, in the line's own colour when that is not jade (`--halo-color`, 55 %). Only
 * the first series of a line chart (`kind` line) is primary; every other mark is bare.
 */
export function lineHalo(
  kind: NonNullable<ChartProps['kind']>,
  seriesIndex: number,
  color: string
): { className: string; style?: CSSProperties } {
  if (kind !== 'line' || seriesIndex !== 0) return { className: '' };
  return color === JADE
    ? { className: 'chart-line-halo' }
    : {
        className: 'chart-line-halo',
        style: { '--halo-color': `color-mix(in srgb, ${color} 55%, transparent)` } as CSSProperties,
      };
}
/** The theme's jade, the halo's own colour; `jadeRamp(1)[0]` is the chart default. */
const JADE = 'var(--c-jade)';
/*
 * The chart's text (lib/chart-type.ts): its ticks, x labels, gap notes, markers and target-line keys
 * at the tick size, an in-plot name and the hover reading's head at the label size, an end label and
 * the hover reading's figures at the reading size. The bands and baselines below are computed from
 * those sizes, so they move with the scale.
 */
/** hierarchy v3: an in-plot name's baseline, one tight line of it under the svg's top (15 px). */
export const PLOT_NAME_Y = tightLinePx(SVG_TEXT.label);
/**
 * hierarchy v3: the band above the data that holds an in-plot name of one line: its baseline and
 * as much again (30 px). A name too long for its plot is broken between words (an SVG text does not
 * wrap) and the band grows by a tight line for each line after the first.
 */
export const PLOT_NAME_BAND = 2 * PLOT_NAME_Y;
const PLOT_NAME_LINE = tightLinePx(SVG_TEXT.label);
/** How far a marker's label stands over the plot. */
const MARKER_LABEL_GAP = 6;
/** The top of a plot with no name: a marker's label at the tick size over it, its cap inside the svg (18 px). */
export const PLOT_TOP = SVG_TEXT.tick + MARKER_LABEL_GAP;
/** The x labels' baseline over the svg's foot, room for their descent. */
const X_AXIS_FOOT = 5;
/** The band under the plot that holds the x labels: their foot, a tick line and 11 px clear of the plot (28 px). */
export const X_AXIS_BAND = X_AXIS_FOOT + SVG_TEXT.tick + 11;
/** The height of each extra row of x labels a categorical axis stacks under the plot: a tight tick line (15 px). */
export const X_LABEL_ROW = tightLinePx(SVG_TEXT.tick);
/**
 * The band above the plot that keys its target lines: its label's baseline at the label's own
 * size (its cap at the band's top) and 4 px under it for the descent; the swatch's length.
 */
export const REFERENCE_KEY_Y = SVG_TEXT.tick;
export const REFERENCE_KEY_BAND = REFERENCE_KEY_Y + 4;
const REFERENCE_SWATCH = 18;
/** An in-plot tick is written where its line lies at least a label's size and a pixel under the band. */
export const INSIDE_TICK_ROOM = SVG_TEXT.label + 1;
/**
 * Lane D1 (the chart-hero board, rule 4): an in-plot tick's places, tried in order — over its grid
 * line at the plot's left end, under it, then the same two at the right end — and the first whose
 * box clears every mark of the plot by `TICK_CLEAR` is taken; where all four are crossed the line
 * keeps no number. Above: its baseline 5 px over the line; under: 14 px below it (a 12 px label's
 * cap clears the line by 3 px). Its box: 11 px over its baseline and 3 under (the board's).
 */
const TICK_OVER = -5;
const TICK_UNDER = 14;
const TICK_BOX_UP = 11;
const TICK_BOX_DOWN = 3;
const TICK_CLEAR = 2;
/** Lane D1 (rule 5): a band's leader line and the gap after it, the gap between name and figure. */
const BAND_LEADER = 12;
const BAND_GAP = 16;
/** The share of the width a plot keeps when its bands are named at their right ends. */
const BAND_PLOT_SHARE = 0.6;
/** One line of band names at the head step: the closest two names stand apart. */
const BAND_LINE = tightLinePx(SVG_TEXT.head);
/** Rule 6: the gold mark before an axis cap's note, and its room. */
const CAP_MARK = 16;
/**
 * The hover reading (F2 charts group, CH-22): its head's baseline (the label size and 5 px of
 * padding over its cap) and the line of each figure under it, set tight at the reading size.
 */
export const HOVER_HEAD_Y = SVG_TEXT.label + 5;
export const HOVER_LINE = tightLinePx(SVG_TEXT.reading);
/** The hover reading's padding, left and right of its widest line. */
export const HOVER_PAD = 10;
/** The box's foot under its head's baseline when it has no figure: each figure adds a line. */
const HOVER_FOOT = 6;
/** The hover reading's distance from its point. */
const HOVER_GAP = 14;
/** The box's top under the plot's top. */
const HOVER_DROP = 4;
/** Half the box's 1 px border: the border stays inside the svg too. */
const HOVER_EDGE = 0.5;

/**
 * Where the hover reading's box (`box`, its width and height) stands for a point at `px` in an svg
 * of `svg`'s size whose plot starts at `top`, decided in both dimensions at once: its top a few
 * pixels under the plot's, and right of the point where the svg holds it there, else left of it,
 * else against the svg's nearer edge (over the crosshair). `null` where the svg cannot hold the
 * box and its border whole, in either dimension (a history of 21 bands in a 260 px chart): the
 * reading is drawn whole or not at all, and the readout line and the key under the plot read the
 * point.
 */
function hoverBoxPlace(
  px: number,
  top: number,
  box: { width: number; height: number },
  svg: { width: number; height: number }
): { x: number; y: number } | null {
  const y = top + HOVER_DROP;
  if (
    box.width + 2 * HOVER_EDGE > svg.width ||
    y - HOVER_EDGE < 0 ||
    y + box.height + HOVER_EDGE > svg.height
  )
    return null;
  const first = HOVER_EDGE;
  const last = svg.width - HOVER_EDGE - box.width;
  if (px + HOVER_GAP <= last) return { x: px + HOVER_GAP, y };
  if (px - HOVER_GAP - box.width >= first) return { x: px - HOVER_GAP - box.width, y };
  return { x: px < svg.width / 2 ? first : last, y };
}
/**
 * A target line, drawn alike in the plot and as its key's sample (CH-21): a dash in the bright ink
 * over a knock-out of the same dash in the page's ground, drawn after the data, so it reads where
 * a series runs along it; the sample is that line, not a "---" that reads as text.
 */
const REFERENCE_DASH = { stroke: 'var(--c-text-bright)', strokeWidth: 1, strokeDasharray: '4 3' };
const REFERENCE_KNOCKOUT = { ...REFERENCE_DASH, stroke: 'var(--c-base-bg)', strokeWidth: 3 };

/** The target line from `x1` to `x2` at `y`: the knock-out, then the dash. */
function ReferenceStroke({ x1, x2, y }: { x1: number; x2: number; y: number }) {
  return (
    <>
      <line x1={x1} x2={x2} y1={y} y2={y} {...REFERENCE_KNOCKOUT} />
      <line x1={x1} x2={x2} y1={y} y2={y} {...REFERENCE_DASH} />
    </>
  );
}

/**
 * Where a stack's names stand at its right end (lane D1, rule 5): each at the middle of its band at
 * the last point, the closest two a line apart — pushed down from the top, then up from the plot's
 * foot where the last ran past it — with `mid` kept for the leader line to its band.
 */
function bandPlaces(
  bands: readonly { key: string; lower: number; upper: number }[],
  y: (value: number) => number,
  top: number,
  bottom: number
): { key: string; index: number; mid: number; at: number }[] {
  const placed = bands
    .map((band, index) => ({ key: band.key, index, mid: y((band.lower + band.upper) / 2) }))
    .sort((a, b) => a.mid - b.mid)
    .map((band) => ({ ...band, at: band.mid }));
  const half = BAND_LINE / 2;
  placed.forEach((band, i) => {
    band.at = Math.max(band.at, i ? placed[i - 1].at + BAND_LINE : top + half);
  });
  for (let i = placed.length - 1; i >= 0; i--)
    placed[i].at = Math.min(
      placed[i].at,
      i === placed.length - 1 ? bottom - half : placed[i + 1].at - BAND_LINE
    );
  return placed;
}

/** Target-line keys laid right to left from the plot's right edge: dashed swatch, then label. */
function referenceKeys(
  lines: readonly { label: string }[],
  right: number
): { label: string; swatch: number; textEnd: number }[] {
  let end = right;
  return lines.map(({ label }) => {
    const textEnd = end;
    const swatch = textEnd - label.length * AXIS_CHAR_PX - 4 - REFERENCE_SWATCH;
    end = swatch - 12;
    return { label, swatch, textEnd };
  });
}

/** What decides a chart's value axis: its points, how they stack, and how a tick reads. */
export type ChartValueAxisInput = Pick<
  ChartProps,
  | 'title'
  | 'data'
  | 'series'
  | 'unit'
  | 'kind'
  | 'yScale'
  | 'yDomain'
  | 'referenceLines'
  | 'formatAxis'
  | 'bareTicks'
  | 'integerTicks'
  | 'axisCap'
>;

/**
 * A chart's values as plot geometry, its round value ticks and their labels. The Chart draws from
 * it, and charts stacked on one time axis size their shared gutter from it (`alignedPlotLeft`).
 */
export function chartValueAxis({
  title,
  data,
  series,
  unit,
  kind = 'line',
  yScale = 'linear',
  yDomain,
  referenceLines = [],
  formatAxis = formatTick,
  bareTicks = false,
  integerTicks = false,
  axisCap,
}: ChartValueAxisInput): {
  values: number[][];
  plotValues: number[];
  ticks: Ticks | null;
  tickLabel: (value: number, step: number) => string;
  labels: string[];
} {
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
  if (integerTicks && yScale === 'log')
    throw new Error(`${title}: a count axis is linear, not logarithmic`);
  // a log axis ticks whole decades only: a tick between them (10^0.5) has no exact label, and a
  // tick's label is its value (the lead's ruling of 2026-11-19)
  const counted = { integer: integerTicks || yScale === 'log' };
  if (axisCap && (yScale === 'log' || yDomain || !Number.isFinite(axisCap.value)))
    throw new Error(`${title}: an axis cap is a linear axis's own top`);
  // Lane D1 (rule 6): a value over the cap does not set the axis; it is drawn cut at the top.
  const geometryValues = [
    ...plotValues.map((value) => (axisCap && value > axisCap.value ? axisCap.value : value)),
    ...referenceLines.map((line) => line.value),
  ].map((value) => (yScale === 'log' ? Math.log10(value) : value));
  const ticks = yDomain
    ? niceTicks(yDomain[0], yDomain[1], 4, 0, counted)
    : plotValues.length
      ? niceTicks(
          kind === 'line' ? Math.min(...geometryValues) : Math.min(0, ...geometryValues),
          kind === 'line' ? Math.max(...geometryValues) : Math.max(0, ...geometryValues),
          4,
          0,
          counted
        )
      : null;
  // U-19 (lane A5): a bare tick is the value alone; its plot's name says the unit once.
  const tickUnit = bareTicks ? '' : unit;
  const tickLabel = (value: number, step: number) =>
    yScale === 'log'
      ? withUnit(formatAxis(10 ** value, 10 ** step), tickUnit)
      : withUnit(formatAxis(value, step), tickUnit);
  const labels = ticks?.values.map((v) => tickLabel(v, ticks.step)) ?? [];
  return { values, plotValues, ticks, tickLabel, labels };
}

/** The one `plotLeft` for charts stacked on a shared time axis: the gutter of their longest tick. */
export function alignedPlotLeft(charts: readonly ChartValueAxisInput[]): number {
  return Math.max(...charts.map((chart) => tickGutter(chartValueAxis(chart).labels)));
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
  categoricalX = false,
  pointDots = false,
  slotBars = false,
  hideMarkerLegend = false,
  formatLabel = (label: string) => label,
  seriesLegend = false,
  pinnedReadout,
  pin,
  plotName,
  insideYTicks = false,
  bareTicks = false,
  integerTicks = false,
  endLabel,
  bandEnds,
  solidBands = false,
  quiet = false,
  axisCap,
}: ChartProps) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const own = usePinnedReadout<number>();
  // Visual finish V13: an area's fill gradient, one per chart instance and series.
  const areaFillId = `chart-area-${useId().replace(/:/g, '')}`;
  const clipId = `${areaFillId}-cap`;
  // Readability R1: by night the scan is cut into the fills (areas, bands, bars), never a line or
  // a word; null by day. The svg is drawn 1:1 (its viewBox is its size), so the rhythm holds.
  const scan = useScanFill();
  const drawsFill =
    kind === 'bars' || kind === 'stacked' || (kind === 'area' && series.some((s) => !s.lineOnly));
  if (pin && controlledActive !== undefined)
    throw new Error(
      `${title}: a chart takes its active point from its pin or its caller, not both`
    );
  // A pin names a point of these points: new ones (another range, a refetched day) let it go.
  const points = `${data.length}|${data[0]?.label}|${data.at(-1)?.label}`;
  const [pinnedPoints, setPinnedPoints] = useState(points);
  if (pinnedPoints !== points) {
    setPinnedPoints(points);
    own.clear();
  }
  // Hover lives in the pin state too; only a tap-to-pin chart ever pins.
  const tap = pin ?? own;
  const tappable = pin !== undefined || pinnedReadout !== undefined;
  const active = controlledActive === undefined ? tap.active : controlledActive;
  const setActive = (index: number | null) => {
    tap.hover(index);
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
  const { values, ticks, tickLabel, labels } = chartValueAxis({
    title,
    data,
    series,
    unit,
    kind,
    yScale,
    yDomain,
    referenceLines,
    formatAxis,
    bareTicks,
    integerTicks,
    axisCap,
  });
  if (bareTicks && plotName === undefined)
    throw new Error(`${title}: bare ticks need the plot's name to say the unit`);
  if (axisCap && plotName === undefined)
    throw new Error(`${title}: an axis cap's note stands under the plot's name`);
  if (bandEnds && (kind !== 'stacked' || series.some((s) => bandEnds[s.key] === undefined)))
    throw new Error(`${title}: band ends name every band of a stack`);
  for (const marker of markers)
    if (!Number.isSafeInteger(marker.index) || marker.index < 0 || marker.index >= data.length)
      throw new Error(`${title}: hardfork ${marker.label} has invalid index ${marker.index}`);
  const ramp = jadeRamp(series.length);
  const color = (i: number) =>
    series[i].color ?? (series[i].assetClass ? CLASS_COLORS[series[i].assetClass!] : ramp[i]);
  // Midnight, continued (move 5): the primary data line of a line chart carries a 2.5 px halo
  // in its own colour; bars and areas (and a model's dashed projection) have none, nor a quiet
  // chart under the main one (lane D1, rule 7).
  const halo = (i: number) => (quiet ? { className: '' } : lineHalo(kind, i, color(i)));
  const activeIndex = active !== null && active < data.length ? active : null;
  const selected =
    activeIndex !== null
      ? data[activeIndex]
      : reading !== undefined
        ? reading
        : data[data.length - 1];
  for (const tick of xTicks ?? [])
    if (!Number.isSafeInteger(tick.index) || tick.index < 0 || tick.index >= data.length)
      throw new Error(`${title}: x tick ${tick.label} has invalid index ${tick.index}`);
  if (
    categoricalX &&
    (!slotBars ||
      !xTicks ||
      xTicks.length !== data.length ||
      xTicks.some((tick, i) => tick.index !== i))
  )
    throw new Error(`${title}: a categorical axis names every slot bar, in order`);
  const gutter = insideYTicks ? 0 : tickGutter(labels);
  if (plotLeft !== undefined && plotLeft < gutter)
    throw new Error(
      `${title}: plotLeft ${plotLeft} cannot hold its longest tick: "${labels.reduce((a, b) => (b.length > a.length ? b : a), '')}" needs ${gutter}`
    );
  const left = plotLeft ?? gutter;
  // Lane D1 (rule 5): a stack's names at its bands' right ends take a column the plot gives up
  // where it keeps three fifths of the width; else they are a key under the plot.
  const bandColumn =
    bandEnds && width !== null
      ? (() => {
          const names = Math.max(...series.map((s) => textPx(s.label.length, SVG_TEXT.head)));
          const figures = Math.max(
            ...series.map((s) => textPx(bandEnds[s.key].length, SVG_TEXT.head))
          );
          const column = BAND_LEADER + names + BAND_GAP + figures;
          return width - column >= BAND_PLOT_SHARE * width ? column : null;
        })()
      : null;
  const right = width === null ? null : bandColumn !== null ? width - bandColumn : width - 10;
  // hierarchy v3: an in-plot name keeps its own band above the data, a line more for each line
  // its words take where the plot cannot hold them on one (from left + 8 to the plot's right).
  const nameLines =
    plotName === undefined
      ? []
      : right === null
        ? [plotName]
        : wrapWords(plotName, SVG_TEXT.label, right - left - 8);
  // Rule 6: the cap's note, after its gold mark, broken between words where the plot is narrow;
  // its first line stands a line under the name's last.
  const capY =
    PLOT_NAME_Y +
    nameLines.length * PLOT_NAME_LINE +
    (referenceLines.length ? REFERENCE_KEY_BAND : 0);
  const capLines =
    axisCap === undefined
      ? []
      : right === null
        ? [axisCap.note]
        : wrapWords(axisCap.note, SVG_TEXT.label, right - left - 8 - CAP_MARK);
  // A target line is keyed in its own band above the plot, never on the plot (C#16).
  const keyBand = referenceLines.length ? REFERENCE_KEY_BAND : 0,
    top =
      (plotName === undefined
        ? PLOT_TOP
        : PLOT_NAME_BAND + (nameLines.length - 1) * PLOT_NAME_LINE) +
      keyBand +
      // the cap's note takes its own lines under the name (rule 6)
      capLines.length * PLOT_NAME_LINE +
      // the fork markers' labels stand a tight line under the name's last line and the cap's,
      // as many lines as they are measured to take, at every width (the D1 hand-back, item 4)
      (plotName !== undefined && markers.length ? MARKER_LABEL_GAP : 0),
    bottom = height - X_AXIS_BAND + keyBand,
    axisY = height + keyBand - X_AXIS_FOOT;
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
  /**
   * The span of y the plot's marks cover between `x0` and `x1` (lane D1): a line where it runs
   * (its points inside and the line at both ends); a filled mark (a stack, a bar, an area) from its
   * top down to the baseline. Null where no mark crosses.
   */
  const markSpan = (x0: number, x1: number): [number, number] | null => {
    const ys: number[] = [];
    const filled = kind === 'stacked' || kind === 'bars';
    const shown = kind === 'stacked' ? [series.length - 1] : series.map((_, si) => si);
    for (const si of shown) {
      const tops = values.map((row) =>
        kind === 'stacked' ? row.slice(0, si + 1).reduce((sum, v) => sum + v, 0) : row[si]
      );
      const yAt = (i: number) => Math.min(bottom, Math.max(top, y(tops[i])));
      for (let i = 0; i < data.length; i++) {
        const xi = x(i);
        if (xi >= x0 && xi <= x1) ys.push(yAt(i));
        if (!filled && i + 1 < data.length) {
          const xn = x(i + 1);
          for (const edge of [x0, x1])
            if (xi < edge && edge < xn)
              ys.push(yAt(i) + ((yAt(i + 1) - yAt(i)) * (edge - xi)) / (xn - xi));
        }
      }
    }
    if (!ys.length) return null;
    return [Math.min(...ys), filled ? bottom : Math.max(...ys)];
  };
  /**
   * Where each in-plot value stands (hierarchy v3; lane D1, rule 4): its number at its own line,
   * over it at the plot's left end, else under it, else at the right end over and under it: the
   * first of those places whose box clears the plot's marks AND every number already placed, by
   * `TICK_CLEAR`. (Each number was placed against the marks alone, from b285bbda, so the number
   * under one line and the number over the next stood on one spot where two lines are under 30 px
   * apart: "4" on "2" in the Nominal APC card, "5" on "10" in the Inflation card, at 53d9b56f.)
   *
   * The rule when they do not all fit: the axis keeps the least a reader needs to tell its scale,
   * the highest line that holds a number (from the top down) and the lowest (from the bottom up,
   * clear of the first): two numbers, or one on a linear axis that stands on 0 (the baseline is
   * not written, hierarchy v3, but where the labelled lines hold no second number the axis needs,
   * the baseline's is it; the plot reads from 0 as its x axis stands on it). Where the marks
   * leave that least no clear place, it stands at its first place clear of the other numbers, on
   * the marks, on the page's ground (`grounded`). The lines between follow from the top down, and
   * a line with no clear place keeps no number, its grid line standing. A plot that cannot keep
   * that least clear of its own numbers is too short for an in-plot scale: it is refused, named,
   * never drawn with too few numbers to read (the chart tests draw every index card at 278 and
   * 358 px). A number left off loses no figure: the readout line and the head state every value
   * exactly.
   */
  const insidePlaces = new Map<
    number,
    { x: number; y: number; anchor: 'start' | 'end'; grounded: boolean }
  >();
  if (insideYTicks && ticks && right !== null) {
    // the labelled lines, from the top down: every round value above the baseline, out of the band
    const lines = ticks.values
      .filter((v, i) => i > 0 && yTick(v) >= top + INSIDE_TICK_ROOM)
      .sort((a, b) => b - a);
    const boxes: { x0: number; x1: number; y0: number; y1: number }[] = [];
    // `onMarks`: the place may cross the plot's marks (the least the axis keeps, drawn on a ground)
    const stand = (v: number, onMarks = false): boolean => {
      const label = tickLabel(v, ticks.step);
      // at the left end it starts where the plot's name does (left + 8); at the right it ends 2 px in
      const wide = textPx(label.length, SVG_TEXT.label) + 10;
      const line = yTick(v);
      const places = [
        { x: left + 8, anchor: 'start' as const, x0: left, x1: left + wide, dy: TICK_OVER },
        { x: left + 8, anchor: 'start' as const, x0: left, x1: left + wide, dy: TICK_UNDER },
        { x: right - 2, anchor: 'end' as const, x0: right - wide, x1: right, dy: TICK_OVER },
        { x: right - 2, anchor: 'end' as const, x0: right - wide, x1: right, dy: TICK_UNDER },
      ];
      for (const spot of places) {
        const box = {
          x0: spot.x0,
          x1: spot.x1,
          y0: line + spot.dy - TICK_BOX_UP,
          y1: line + spot.dy + TICK_BOX_DOWN,
        };
        if (box.y0 < top - TICK_CLEAR || box.y1 > bottom) continue;
        const span = markSpan(spot.x0, spot.x1);
        const onAMark =
          span !== null && !(box.y1 + TICK_CLEAR < span[0] || box.y0 - TICK_CLEAR > span[1]);
        if (onAMark && !onMarks) continue;
        const crosses = boxes.some(
          (other) =>
            box.x0 < other.x1 + TICK_CLEAR &&
            other.x0 < box.x1 + TICK_CLEAR &&
            box.y0 < other.y1 + TICK_CLEAR &&
            other.y0 < box.y1 + TICK_CLEAR
        );
        if (crosses) continue;
        insidePlaces.set(v, {
          x: spot.x,
          y: line + spot.dy,
          anchor: spot.anchor,
          grounded: onAMark,
        });
        boxes.push(box);
        return true;
      }
      return false;
    };
    // the least that tells the scale: two numbers; one on a linear axis that stands on 0, which
    // the plot reads from as its x axis stands on it
    const baseline = ticks.values[0];
    const needed = yScale !== 'log' && baseline === 0 ? 1 : 2;
    // the numbers the axis keeps: the highest line that holds one, then the lowest; first clear
    // of the marks, then, for the least the axis keeps, on them on a ground
    let highest: number | undefined;
    let lowest: number | undefined;
    for (const onMarks of [false, true]) {
      if (highest === undefined) highest = lines.find((v) => stand(v, onMarks));
      const kept = (highest === undefined ? 0 : 1) + (lowest === undefined ? 0 : 1);
      if (highest !== undefined && lowest === undefined && (!onMarks || kept < needed))
        lowest = [...lines].reverse().find((v) => v !== highest && stand(v, onMarks));
      // where the labelled lines hold no second number the axis needs, the baseline's is it
      if (highest !== undefined && lowest === undefined && needed === 2 && stand(baseline, onMarks))
        lowest = baseline;
      if ((highest === undefined ? 0 : 1) + (lowest === undefined ? 0 : 1) >= needed) break;
    }
    const kept = (highest === undefined ? 0 : 1) + (lowest === undefined ? 0 : 1);
    if (kept < needed)
      throw new Error(
        `${title}: the plot is too short for an in-plot scale: ${needed === 1 ? 'a number' : 'two numbers'} must stand, and of its lines ${lines.map((v) => tickLabel(v, ticks.step)).join(', ') || '(none above its baseline)'} ${kept === 0 ? 'none holds' : 'only one holds'} a number clear of the other numbers`
      );
    // the lines between, from the top down, each where a place is free
    const [high, low] = [highest, lowest];
    if (high !== undefined && low !== undefined)
      for (const v of lines) if (v < high && v > low && !insidePlaces.has(v)) stand(v);
  }
  /**
   * An in-plot value (hierarchy v3): a figure, the dim ink (R4), bare, at its place if it has one;
   * one the axis keeps on the plot's marks stands on the page's ground (a stroke under its glyphs,
   * as the end label's), so the mark does not cut it.
   */
  const insideTickLabel = (v: number) => {
    const place = insidePlaces.get(v);
    if (!place || !ticks) return null;
    return (
      <text
        key={`y-tick:${v}`}
        data-role="y-tick"
        data-grounded={place.grounded || undefined}
        x={place.x}
        y={place.y}
        textAnchor={place.anchor}
        fill="var(--c-text-dim)"
        fontSize={SVG_TEXT.label}
        {...(place.grounded
          ? { stroke: 'var(--c-base-bg)', strokeWidth: 4, paintOrder: 'stroke' }
          : {})}
      >
        {tickLabel(v, ticks.step)}
      </text>
    );
  };
  const xLabels =
    xTicks && right !== null && right > left && data.length
      ? placeXTicks(
          xTicks.map((tick) => ({ ...tick, at: x(tick.index) })),
          { left, right, width: width as number, categorical: categoricalX }
        )
      : null;
  /** The point nearest a pointer at `clientX` over the plot (`el`, the chart's svg). */
  const pointAt = (clientX: number, el: Element): number => {
    const px = clientX - el.getBoundingClientRect().left;
    const index = xDomain
      ? data.reduce(
          (closest, _, i) => (Math.abs(x(i) - px) < Math.abs(x(closest) - px) ? i : closest),
          0
        )
      : slotBars
        ? Math.floor(((px - left) / ((right as number) - left)) * data.length)
        : Math.round(((px - left) / ((right as number) - left)) * (data.length - 1));
    return index < 0 ? 0 : index >= data.length ? data.length - 1 : index;
  };
  // Extra label rows go under the plot; the plot keeps its height.
  const svgHeight = height + keyBand + ((xLabels?.rows ?? 1) - 1) * X_LABEL_ROW;
  return (
    <figure className="m-0 min-w-0">
      {plotName === undefined && (
        <figcaption
          className={
            hideCaption ? 'sr-only' : 'mb-3 flex flex-wrap items-baseline justify-between gap-3'
          }
        >
          <span>
            <span className="text-text-bright text-body block font-medium">{title}</span>
            <span className="text-text-dim text-meta">
              {activeIndex === null ? subtitle : selected && formatLabel(selected.label)}
            </span>
          </span>
          {!seriesLegend && (
            <span className="text-text-bright text-hero flex flex-wrap justify-end gap-3 tabular-nums">
              {selected &&
                series.map((s, i) => (
                  <span key={s.key}>
                    {/* A legend's label is text, in the text ink; the series colour is on its
                        swatch only (U-08: "Other scripts" in its slate read 1.4:1). */}
                    {series.length > 1 && (
                      <span className="text-text text-meta mr-2">
                        <span
                          aria-hidden="true"
                          data-swatch
                          className="mr-1.5 inline-block h-2 w-2 align-middle"
                          style={{ backgroundColor: color(i) }}
                        />
                        {s.label}
                      </span>
                    )}
                    {formatValue(selected.values[s.key])}
                    <span className="text-text-dim text-meta">{unitAfter(unit)}</span>
                  </span>
                ))}
            </span>
          )}
        </figcaption>
      )}
      <div ref={ref} className="min-w-0" style={{ minHeight: height }}>
        {!data.length ? (
          <p className="text-text text-small py-8">No data in this range.</p>
        ) : kind === 'ranked' ? (
          <ol className="space-y-3">
            {data.map((p, i) => (
              <li key={`${p.label}:${i}`} className="text-small">
                <div className="flex flex-wrap justify-between gap-2">
                  <span>{p.label}</span>
                  <span className="tabular-nums">
                    {formatValue(p.values[series[0].key])}
                    {unitAfter(unit)}
                  </span>
                </div>
                <div
                  aria-hidden="true"
                  className="scan-fill mt-1 h-1"
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
              height={svgHeight}
              viewBox={`0 0 ${width} ${svgHeight}`}
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
                  e.preventDefault();
                  const next =
                    (active === null ? data.length - 1 : active) + (e.key === 'ArrowLeft' ? -1 : 1);
                  setActive(next < 0 ? 0 : next >= data.length ? data.length - 1 : next);
                }
                if (e.key === 'Escape') {
                  tap.clear();
                  onActiveIndexChange?.(null);
                }
              }}
              onPointerMove={(e) => setActive(pointAt(e.clientX, e.currentTarget))}
              onPointerLeave={() => setActive(null)}
              onClick={
                tappable ? (e) => tap.toggle(pointAt(e.clientX, e.currentTarget)) : undefined
              }
            >
              {drawsFill && scan?.defs}
              {axisCap && (
                <defs>
                  <clipPath id={clipId}>
                    <rect x={left} y={top} width={right - left} height={bottom - top + 1} />
                  </clipPath>
                </defs>
              )}
              {ticks.values.map((v, i) =>
                insideYTicks ? (
                  // hierarchy v3: the baseline and a value in the name's band are not written.
                  i > 0 &&
                  yTick(v) >= top + INSIDE_TICK_ROOM && (
                    <g key={v}>
                      {/* Visual finish V13: the grid is solid; a dash means a reference. */}
                      <line
                        x1={left}
                        x2={right}
                        y1={yTick(v)}
                        y2={yTick(v)}
                        stroke={CHART_GRID_COLOR}
                      />
                      {insideYTicks !== 'over' && insideTickLabel(v)}
                    </g>
                  )
                ) : (
                  <g key={v}>
                    <line
                      x1={left}
                      x2={right}
                      y1={yTick(v)}
                      y2={yTick(v)}
                      stroke={CHART_GRID_COLOR}
                    />
                    <text
                      x={left - 8}
                      y={yTick(v) + centreShift(SVG_TEXT.tick)}
                      textAnchor="end"
                      fill="var(--c-text-dim)"
                      fontSize={SVG_TEXT.tick}
                    >
                      {tickLabel(v, ticks.step)}
                    </text>
                  </g>
                )
              )}
              {/* the baseline's number, where the axis keeps it to tell its scale */}
              {insideYTicks !== 'over' && insideTickLabel(ticks.values[0])}
              {plotName !== undefined && (
                <text
                  data-role="chart-name"
                  x={left + 8}
                  y={PLOT_NAME_Y + keyBand}
                  fill="var(--c-text-dim)"
                  fontSize={SVG_TEXT.label}
                >
                  {nameLines.length === 1
                    ? plotName
                    : nameLines.map((line, i) => (
                        <tspan
                          key={`${i}:${line}`}
                          x={i ? left + 8 : undefined}
                          dy={i ? PLOT_NAME_LINE : undefined}
                        >
                          {line}
                        </tspan>
                      ))}
                </text>
              )}
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
                      fill="var(--c-base-surface)"
                    />
                    {gapRight - gapLeft > 120 && (
                      <text
                        x={(gapLeft + gapRight) / 2}
                        y={(top + bottom) / 2}
                        textAnchor="middle"
                        fill="var(--c-text-dim)"
                        fontSize={SVG_TEXT.tick}
                      >
                        {gap.label}
                      </text>
                    )}
                  </g>
                );
              })}
              {/* Lane D1 (rule 6): a mark above an axis cap is cut at the plot's top. */}
              <g clipPath={axisCap ? `url(#${clipId})` : undefined}>
                {series.map((s, si) => {
                  const upper = values.map((row) =>
                    kind === 'stacked'
                      ? row.slice(0, si + 1).reduce((sum, v) => sum + v, 0)
                      : row[si]
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
                        {/* one mask over the series' bars, not a mask (a layer) per bar */}
                        <g mask={scan?.mask}>
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
                        {/* F2 charts group: a bar's hovered top, like a line's hovered point. */}
                        {pointDots && activeIndex !== null && (
                          <circle
                            data-hover-dot={s.key}
                            cx={x(activeIndex)}
                            cy={y(upper[activeIndex])}
                            r="4"
                            fill="var(--c-base-bg)"
                            stroke={color(si)}
                            strokeWidth="2"
                          />
                        )}
                      </g>
                    );
                  // Visual finish V13: a single (not stacked) area is light, not paint — its colour
                  // at .22 at the top of the fill fading to 0 at its foot; a stacked area keeps its
                  // flat 60 %, since its bands must read as the parts of one whole.
                  const lit = kind === 'area' && !s.lineOnly;
                  const fillId = `${areaFillId}-${si}`;
                  const areaFill =
                    kind === 'stacked' ? color(si) : lit ? `url(#${fillId})` : 'none';
                  return (
                    <g key={s.key}>
                      {lit && (
                        <defs>
                          <linearGradient id={fillId} x1="0" y1="0" x2="0" y2="1">
                            <stop offset="0" stopColor={color(si)} stopOpacity="0.22" />
                            <stop offset="1" stopColor={color(si)} stopOpacity="0" />
                          </linearGradient>
                        </defs>
                      )}
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
                          fill={areaFill}
                          fillOpacity={kind === 'stacked' ? (solidBands ? 1 : 0.6) : undefined}
                          mask={areaFill === 'none' ? undefined : scan?.mask}
                        />
                      )}
                      {projectedAfter === undefined ? (
                        <path
                          d={line}
                          fill="none"
                          stroke={color(si)}
                          strokeWidth="1.5"
                          strokeDasharray={s.dashed ? '4 3' : undefined}
                          {...(s.dashed ? {} : halo(si))}
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
                            {...halo(si)}
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
                          fill="var(--c-base-bg)"
                          stroke={color(si)}
                          strokeWidth="2"
                        />
                      )}
                    </g>
                  );
                })}
              </g>
              {insideYTicks === 'over' && ticks.values.map((v) => insideTickLabel(v))}
              {axisCap && (
                <g data-role="axis-cap">
                  {/* the gold mark and the note under the plot's name (the board's outlier) */}
                  <path d={`M${left + 8} ${capY} l5 -10 l5 10 Z`} fill="var(--c-gold)" />
                  <text
                    x={left + 8 + CAP_MARK}
                    y={capY}
                    fill="var(--c-text)"
                    fontSize={SVG_TEXT.label}
                  >
                    {capLines.map((line, i) => (
                      <tspan
                        key={`${i}:${line}`}
                        x={i ? left + 8 + CAP_MARK : undefined}
                        dy={i ? PLOT_NAME_LINE : undefined}
                      >
                        {line}
                      </tspan>
                    ))}
                  </text>
                  {/* a gold tick over each mark the cap cuts */}
                  {values.flatMap((row, i) =>
                    (kind === 'stacked' ? row.reduce((sum, v) => sum + v, 0) : Math.max(...row)) >
                    axisCap.value
                      ? [
                          <line
                            key={`cap:${i}`}
                            data-cut={data[i].label}
                            x1={x(i)}
                            x2={x(i)}
                            y1={top - 4}
                            y2={top}
                            stroke="var(--c-gold)"
                            strokeWidth={2}
                          />,
                        ]
                      : []
                  )}
                </g>
              )}
              {bandColumn !== null &&
                bandEnds &&
                bandPlaces(
                  series.map((s, si) => ({
                    key: s.key,
                    lower: values[data.length - 1].slice(0, si).reduce((sum, v) => sum + v, 0),
                    upper: values[data.length - 1].slice(0, si + 1).reduce((sum, v) => sum + v, 0),
                  })),
                  y,
                  top,
                  bottom
                ).map((band) => (
                  <g key={band.key} data-band-end={band.key}>
                    <line
                      x1={right}
                      x2={right + BAND_LEADER - 4}
                      y1={band.mid}
                      y2={band.at}
                      stroke={color(band.index)}
                    />
                    <text
                      x={right + BAND_LEADER}
                      y={band.at + centreShift(SVG_TEXT.head)}
                      fill="var(--c-text)"
                      fontSize={SVG_TEXT.head}
                    >
                      {series[band.index].label}
                    </text>
                    <text
                      x={width}
                      y={band.at + centreShift(SVG_TEXT.head)}
                      textAnchor="end"
                      fill="var(--c-text-bright)"
                      fontSize={SVG_TEXT.head}
                    >
                      {bandEnds[band.key]}
                    </text>
                  </g>
                ))}
              {endLabel && (
                <text
                  data-role="end-label"
                  x={right}
                  y={Math.max(SVG_TEXT.reading, y(values[data.length - 1][0]) - 8)}
                  textAnchor="end"
                  fill="var(--c-text-bright)"
                  fontSize={SVG_TEXT.reading}
                  stroke="var(--c-base-bg)"
                  strokeWidth="4"
                  paintOrder="stroke"
                >
                  {endLabel(data[data.length - 1].values[series[0].key])}
                </text>
              )}
              {referenceLines.map((line) => (
                <g key={line.label} data-reference-line={line.label}>
                  <ReferenceStroke x1={left} x2={right} y={y(line.value)} />
                </g>
              ))}
              {referenceKeys(referenceLines, right).map(({ label, swatch, textEnd }) => (
                <g key={label} data-reference-key={label}>
                  <ReferenceStroke
                    x1={swatch}
                    x2={swatch + REFERENCE_SWATCH}
                    y={REFERENCE_KEY_Y - centreShift(SVG_TEXT.tick)}
                  />
                  <text
                    x={textEnd}
                    y={REFERENCE_KEY_Y}
                    textAnchor="end"
                    fill="var(--c-text)"
                    fontSize={SVG_TEXT.tick}
                  >
                    {label}
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
                    stroke="var(--c-text-dim)"
                    strokeDasharray="3 3"
                  />
                  <text
                    x={x(m.index)}
                    y={top - MARKER_LABEL_GAP}
                    textAnchor="middle"
                    fill="var(--c-text-dim)"
                    fontSize={SVG_TEXT.tick}
                  >
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
                  stroke="var(--c-text)"
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
                    return `${series.length > 1 ? `${s.label} ` : ''}${withUnit(String(shown), unit)}`;
                  });
                  const head = formatLabel(data[activeIndex].label);
                  // Drawn whole or not at all: as wide as its widest line, each line measured at
                  // its own size (the head at the label size, the figures at the reading size),
                  // and its padding; as tall as its head and a line a figure; placed by
                  // hoverBoxPlace, or not drawn where the svg cannot hold it.
                  const boxWidth =
                    Math.max(
                      textPx(head.length, SVG_TEXT.label),
                      ...rows.map((r) => textPx(r.length, SVG_TEXT.reading))
                    ) +
                    2 * HOVER_PAD;
                  const boxHeight = HOVER_HEAD_Y + HOVER_FOOT + rows.length * HOVER_LINE;
                  const place = hoverBoxPlace(
                    x(activeIndex),
                    top,
                    { width: boxWidth, height: boxHeight },
                    { width, height: svgHeight }
                  );
                  if (place === null) return null;
                  const { x: bx, y: by } = place;
                  return (
                    <g data-hover-reading pointerEvents="none">
                      <rect
                        x={bx}
                        y={by}
                        width={boxWidth}
                        height={boxHeight}
                        fill="var(--c-base-surface)"
                        stroke="var(--c-base-border)"
                      />
                      <text
                        x={bx + HOVER_PAD}
                        y={by + HOVER_HEAD_Y}
                        fill="var(--c-text-dim)"
                        fontSize={SVG_TEXT.label}
                      >
                        {head}
                      </text>
                      {rows.map((row, i) => (
                        <text
                          key={row}
                          x={bx + HOVER_PAD}
                          y={by + HOVER_HEAD_Y + 1 + (i + 1) * HOVER_LINE}
                          fill="var(--c-text-bright)"
                          fontSize={SVG_TEXT.reading}
                        >
                          {row}
                        </text>
                      ))}
                    </g>
                  );
                })()}
              {xLabels?.ticks.map((tick) => (
                <text
                  key={`${tick.index}:${tick.label}`}
                  data-x-tick
                  visibility={hideXAxis ? 'hidden' : undefined}
                  x={tick.x}
                  y={axisY + tick.row * X_LABEL_ROW}
                  textAnchor={tick.anchor}
                  fill="var(--c-text-dim)"
                  fontSize={SVG_TEXT.tick}
                >
                  {tick.label}
                </text>
              ))}
              <text
                visibility={hideXAxis || xTicks ? 'hidden' : undefined}
                x={left}
                y={axisY}
                fill="var(--c-text-dim)"
                fontSize={SVG_TEXT.tick}
              >
                {data[0].label}
              </text>
              {data.length > 1 && (
                <text
                  visibility={hideXAxis || xTicks ? 'hidden' : undefined}
                  x={right}
                  y={axisY}
                  textAnchor="end"
                  fill="var(--c-text-dim)"
                  fontSize={SVG_TEXT.tick}
                >
                  {data[data.length - 1].label}
                </text>
              )}
            </svg>
          )
        )}
      </div>
      {/* Lane D1 (rule 5): where the column does not fit, the key under the plot, top band first. */}
      {bandEnds && width !== null && bandColumn === null && (
        <ul aria-label={`${title} key`} className="text-small mt-3 flex flex-col gap-1">
          {[...series.keys()].reverse().map((i) => (
            <li key={series[i].key} className="flex min-w-0 items-center gap-2">
              <span
                aria-hidden="true"
                data-swatch
                className="inline-block h-2.5 w-2.5 shrink-0"
                style={{ backgroundColor: color(i) }}
              />
              <span className="text-text min-w-0">{series[i].label}</span>
              <span className="text-text-bright ml-auto whitespace-nowrap tabular-nums">
                {bandEnds[series[i].key]}
              </span>
            </li>
          ))}
        </ul>
      )}
      {pinnedReadout && (
        <div className="mt-2">
          <ReadoutRow label={pinnedReadout.label} pinned={tap.showingPin} idle={pinnedReadout.idle}>
            {activeIndex !== null && selected ? (
              <>
                {formatLabel(selected.label)}
                {seriesLegend
                  ? ' · values in the key below'
                  : series.map((s) => (
                      <Fragment key={s.key}>
                        {' · '}
                        {series.length > 1 && `${s.label} `}
                        {formatValue(selected.values[s.key])}
                        {unitAfter(unit)}
                      </Fragment>
                    ))}
              </>
            ) : null}
          </ReadoutRow>
        </div>
      )}
      {seriesLegend && (
        <ul aria-label={`${title} legend`} className="text-body mt-3 flex flex-col gap-1">
          {series.map((s, i) => (
            <li key={s.key} className="flex min-w-0 flex-wrap items-baseline justify-start gap-x-2">
              <span className="flex min-w-0 items-baseline gap-2">
                <span
                  aria-hidden="true"
                  data-swatch
                  className="inline-block h-2.5 w-2.5 shrink-0 self-center"
                  style={{ backgroundColor: color(i) }}
                />
                <span className="text-text">{s.label}</span>
              </span>
              {selected && (
                <span className="text-text-bright whitespace-nowrap tabular-nums">
                  {formatValue(selected.values[s.key])}
                  <span className="text-text-dim text-meta">{unitAfter(unit)}</span>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
      {/* A tap-to-pin chart announces through its readout line instead. */}
      {!pinnedReadout && (
        <span className="sr-only" aria-live="polite">
          {activeIndex !== null && selected
            ? `${selected.label}: ${series.map((s) => `${s.label} ${withUnit(selected.values[s.key], unit)}`).join(', ')}`
            : ''}
        </span>
      )}
      {markers.length > 0 && !hideMarkerLegend && (
        <div className="text-meta mt-2 flex flex-wrap gap-4">
          {markers.map((m) => (
            <Link key={`${m.index}:${m.label}`} href={m.href} look="inline">
              {m.label} · {data[m.index].label}
            </Link>
          ))}
        </div>
      )}
    </figure>
  );
}
