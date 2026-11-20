/**
 * Axis helpers for the homepage small multiples: round ticks, their labels, and the symmetric cap
 * of a signed (daily-change) chart. One helper per job, so every chart reads the same numbers.
 */

import { SVG_TEXT } from '@/lib/chart-type';
import { advancePx } from '@/lib/type-metrics';
import { formatCompactRatio } from '@/lib/exact-integer';

/** The typographic minus, used for every negative number the charts print. */
export const MINUS = '−';

export interface Ticks {
  /** Ascending, evenly spaced round values; the first and last are the axis domain. */
  values: number[];
  step: number;
}

function assertFinite(name: string, value: number): void {
  if (!Number.isFinite(value)) {
    throw new Error(`${name} must be a finite number, got ${value}`);
  }
}

/** The smallest 1 / 2 / 2.5 / 5 × 10^k step that is at least `raw`. */
export function niceStep(raw: number): number {
  assertFinite('niceStep: raw step', raw);
  if (raw <= 0) throw new Error(`niceStep: raw step must be positive, got ${raw}`);
  const mag = 10 ** Math.floor(Math.log10(raw));
  const norm = raw / mag;
  // A hair of tolerance: float division puts an exact 2.0 at 2.0000000000000004.
  const eps = 1e-9;
  const m =
    norm <= 1 + eps ? 1 : norm <= 2 + eps ? 2 : norm <= 2.5 + eps ? 2.5 : norm <= 5 + eps ? 5 : 10;
  return m * mag;
}

/** Decimals needed to print multiples of `step` exactly (0.25 → 2, 2.5 → 1, 500 → 0). */
export function stepDecimals(step: number): number {
  assertFinite('stepDecimals: step', step);
  if (step === 0) return 0;
  const [mantissa, exponentText] = step.toExponential().split('e');
  const exponent = Number(exponentText);
  // Shift the decimal exponent rather than multiply by 10 ** d: token decimals
  // can reach 255, and representable numbers can need up to 324 decimal places.
  for (let d = Math.max(0, -exponent); d <= 324; d++) {
    const scaled = Number(`${mantissa}e${exponent + d}`);
    const integer = Math.round(scaled);
    // Float tolerance may remove arithmetic noise around a nonzero integer, but a
    // nonzero step rounding to zero has not reached its required decimal precision.
    if (integer !== 0 && Math.abs(scaled - integer) < 1e-6) return d;
  }
  throw new Error(`stepDecimals: ${step} has no short decimal form`);
}

/** Round a plotted number to the tick's decimal grid without toFixed's 100-digit limit. */
function tickDecimal(value: number, decimals: number): string {
  assertFinite('tickDecimal: value', value);
  const [mantissa, exponentText = '0'] = Math.abs(value).toString().split('e');
  const [whole, fraction = ''] = mantissa.split('.');
  const coefficient = BigInt(whole + fraction);
  const shift = decimals + Number(exponentText) - fraction.length;
  let rounded: bigint;
  if (shift >= 0) {
    rounded = coefficient * BigInt(10) ** BigInt(shift);
  } else {
    const divisor = BigInt(10) ** BigInt(-shift);
    rounded = (coefficient + divisor / BigInt(2)) / divisor;
  }
  const digits = rounded.toString().padStart(decimals + 1, '0');
  const fixed =
    decimals === 0 ? digits : `${digits.slice(0, -decimals)}.${digits.slice(-decimals)}`;
  return value < 0 && rounded !== BigInt(0) ? `-${fixed}` : fixed;
}

/**
 * The step of a count axis: the nice step, but at least 1 and never fractional (a 2.5 step under
 * 10 becomes 5), so every tick is a whole number of things (UI review 2026-11-14, U-19).
 */
function wholeStep(raw: number): number {
  const step = niceStep(raw);
  if (step <= 1) return 1;
  return Number.isInteger(step) ? step : niceStep(step * 1.5);
}

/**
 * Round ticks covering [min, max]: about `target` of them, evenly spaced by a 1/2/2.5/5 × 10^k
 * step. They are multiples of the step, or — with `anchor` — the anchor plus multiples of it, so a
 * reference value (a consensus target) is itself a tick. A flat series gets a domain around its
 * value (or [0, 1] for zero) so the axis is never degenerate. `integer`: a count axis, whose step
 * is a whole number of at least 1 (never "0.5 transfers").
 */
export function niceTicks(
  min: number,
  max: number,
  target = 4,
  anchor = 0,
  { integer = false }: { integer?: boolean } = {}
): Ticks {
  assertFinite('niceTicks: min', min);
  assertFinite('niceTicks: max', max);
  assertFinite('niceTicks: anchor', anchor);
  if (integer && !Number.isInteger(anchor))
    throw new Error(`niceTicks: a count axis cannot be anchored at ${anchor}`);
  if (min > max) throw new Error(`niceTicks: min ${min} is above max ${max}`);
  if (!Number.isInteger(target) || target < 2) {
    throw new Error(`niceTicks: target must be an integer of at least 2, got ${target}`);
  }
  let lo = min;
  let hi = max;
  if (lo === hi) {
    if (lo === 0) {
      hi = 1;
    } else {
      const pad = Math.abs(lo) * 0.1;
      lo -= pad;
      hi += pad;
    }
  }
  const step = (integer ? wholeStep : niceStep)((hi - lo) / (target - 1));
  const decimals = Math.max(stepDecimals(step), anchor === 0 ? 0 : stepDecimals(anchor));
  const first = Math.floor((lo - anchor) / step + 1e-9);
  const last = Math.ceil((hi - anchor) / step - 1e-9);
  const values: number[] = [];
  for (let i = first; i <= last; i++) {
    values.push(Number(tickDecimal(anchor + i * step, decimals)));
  }
  return { values, step };
}

/**
 * A tick's exact digits, its sign aside, as `[whole, fraction]`, the decimal point moved `shift`
 * places left: the scale chose the tick on its step's grid (`niceTicks`), so the shortest form of
 * the number is the value itself, and nothing here rounds or cuts (the lead's ruling of
 * 2026-11-19: a tick's label is its value).
 */
export function exactTickDigits(value: number, shift = 0): [string, string] {
  assertFinite('exactTickDigits: value', value);
  const [mantissa, exponentText = '0'] = Math.abs(value).toString().split('e');
  const [whole, fraction = ''] = mantissa.split('.');
  const digits = `${whole}${fraction}`;
  const point = whole.length + Number(exponentText) - shift;
  const [w, f] =
    point <= 0
      ? ['0', `${'0'.repeat(-point)}${digits}`]
      : point >= digits.length
        ? [`${digits}${'0'.repeat(point - digits.length)}`, '']
        : [digits.slice(0, point), digits.slice(point)];
  return [w.replace(/^0+(?=\d)/, ''), f.replace(/0+$/, '')];
}

/**
 * A tick label, exactly its value (the lead's ruling of 2026-11-19: a rounded or a cut label puts
 * the line at another number than it says): thousands as k / M / B with every digit the value has
 * (500, 1k, 2.5k, 1.00001M), smaller values with at least the decimals their step needs (2.40,
 * 2.60 …), micro-token values in scientific notation, negatives with a real minus sign. Exact data
 * readouts remain separate from axis labels.
 */
export function formatTick(value: number, step: number): string {
  assertFinite('formatTick: value', value);
  if (value === 0) return '0';
  const abs = Math.abs(value);
  const sign = value < 0 ? MINUS : '';
  const units: [number, number, string][] = [
    [1e9, 9, 'B'],
    [1e6, 6, 'M'],
    [1e3, 3, 'k'],
  ];
  for (const [div, shift, suffix] of units) {
    if (abs >= div) {
      const [whole, fraction] = exactTickDigits(abs, shift);
      return `${sign}${fraction ? `${whole}.${fraction}` : whole}${suffix}`;
    }
  }
  if (abs < 1e-8) return `${sign}${abs.toExponential()}`;
  const [whole, fraction] = exactTickDigits(abs);
  const decimals = Math.max(stepDecimals(step), fraction.length);
  return `${sign}${decimals ? `${whole}.${fraction.padEnd(decimals, '0')}` : whole}`;
}

/** The smallest 1 / 1.5 / 2 / 2.5 / 3 / 4 / 5 / 6 / 8 × 10^k value that is at least `x`. */
export function niceCeil(x: number): number {
  assertFinite('niceCeil: x', x);
  if (x <= 0) throw new Error(`niceCeil: x must be positive, got ${x}`);
  const mag = 10 ** Math.floor(Math.log10(x));
  const norm = x / mag;
  for (const m of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) {
    if (norm <= m + 1e-9) return m * mag;
  }
  throw new Error(`niceCeil: no mantissa covers ${x}`);
}

/**
 * The symmetric axis cap of a signed bar chart: the round ceiling of the 2nd-largest |value| × 1.25,
 * so one outlier day does not flatten the other bars (it is drawn clipped instead). With a single
 * non-zero value the cap is built from it, so nothing clips; an all-zero series gets ±1.
 */
export function signedAxisCap(values: number[]): number {
  values.forEach((v, i) => assertFinite(`signedAxisCap: value ${i}`, v));
  const mags = values.map((v) => Math.abs(v)).sort((a, b) => b - a);
  const largest = mags[0] ?? 0;
  const second = mags[1] ?? 0;
  const basis = second > 0 ? second : largest;
  if (basis === 0) return 1;
  return niceCeil(basis * 1.25);
}

export interface SignedBar {
  value: number;
  /** The drawn value: `value`, or ±cap when it overshoots the axis. */
  drawn: number;
  clipped: boolean;
}

/** Each value against the axis cap: what is drawn, and whether it was clipped at the cap. */
export function clipToCap(values: number[], cap: number): SignedBar[] {
  assertFinite('clipToCap: cap', cap);
  if (cap <= 0) throw new Error(`clipToCap: cap must be positive, got ${cap}`);
  return values.map((value, i) => {
    assertFinite(`clipToCap: value ${i}`, value);
    const clipped = Math.abs(value) > cap;
    return { value, drawn: clipped ? Math.sign(value) * cap : value, clipped };
  });
}

function trimZeros(s: string): string {
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s;
}

/**
 * A signed CKB change, compact, for a clipped bar's label and a chart's range: +28.12M, −8.94M,
 * +950k, +12.5k, −12. Read from the exact shannons and cut in the unit the value reaches, its
 * trailing zeros dropped; under 1,000 CKB the whole CKB, cut (lane exactness, part B3: it rounded
 * the float the bar is drawn from, 28,129,999.99999999 CKB read +28.13M).
 */
export function formatCompactSigned(shannons: bigint): string {
  const sign = shannons > BigInt(0) ? '+' : shannons < BigInt(0) ? MINUS : '';
  const abs = shannons < BigInt(0) ? -shannons : shannons;
  const perCkb = BigInt(100_000_000);
  if (abs < perCkb * BigInt(1000)) return `${sign}${abs / perCkb}`;
  const compact = formatCompactRatio(abs, perCkb);
  const suffix = compact.replace(/^[\d.]+/, '');
  return `${sign}${trimZeros(compact.slice(0, compact.length - suffix.length))}${suffix}`;
}

/**
 * The advance an x label (set at the tick size) is measured at: the face's 0.6 em (7.2 px at
 * 12 px). It was 0.2 px a glyph wider, a margin that kept labels apart; that margin is the gap
 * below, in whole pixels between two labels, not a share of each glyph.
 */
export const AXIS_CHAR_PX = advancePx(SVG_TEXT.tick);
/** The clear space kept between two x labels on one row. */
const X_LABEL_GAP = 6;
/** A time-axis label this close to a plot edge anchors to that edge instead of centring. */
const X_EDGE_ANCHOR = 20;

export interface XTickInput {
  index: number;
  label: string;
  /** The x of the point (or slot centre) the label names. */
  at: number;
  /** A histogram's open-ended outlier label (`≤ 2.05`, `≥ 7.67 h`): an edge of the regular axis. */
  edge?: boolean;
}

export interface PlacedXTick {
  index: number;
  label: string;
  x: number;
  anchor: 'start' | 'middle' | 'end';
  /** 0 for the first label row under the plot, 1 for the one below it, and so on. */
  row: number;
  edge?: boolean;
}

const xSpan = (t: { x: number; anchor: PlacedXTick['anchor']; label: string }) => {
  const w = t.label.length * AXIS_CHAR_PX;
  const from = t.anchor === 'start' ? t.x : t.anchor === 'end' ? t.x - w : t.x - w / 2;
  return { from, to: from + w };
};

/**
 * Where the x labels go. A time axis keeps every k-th label, the fewest thinnings at which no
 * two touch, so the kept labels stay evenly spaced. A histogram's open-ended outlier labels
 * (`edge`) stand outside that sequence: each is always kept, since it says where the regular axis
 * stops (and the last carries the unit), and a regular label that would touch one gives way
 * (readability 2026-11-18: thinning from "≤ 2.05" kept 3.5 / 4.5 / 5.5 and dropped the 4 h peak
 * and the unit). A categorical axis (a few named buckets, one bar each) keeps every label: centred
 * under its bar, held inside the chart, and stacked onto as many rows as it takes for no two on a
 * row to touch.
 */
export function placeXTicks(
  ticks: readonly XTickInput[],
  frame: { left: number; right: number; width: number; categorical: boolean }
): { ticks: PlacedXTick[]; rows: number } {
  const { left, right, width, categorical } = frame;
  if (!categorical) {
    const anchored = ticks.map((tick) => ({
      index: tick.index,
      label: tick.label,
      x: tick.at,
      anchor: (tick.at - left < X_EDGE_ANCHOR
        ? 'start'
        : right - tick.at < X_EDGE_ANCHOR
          ? 'end'
          : 'middle') as PlacedXTick['anchor'],
      row: 0,
      ...(tick.edge ? { edge: true } : {}),
    }));
    const edges = anchored.filter((tick) => tick.edge);
    const regular = anchored.filter((tick) => !tick.edge);
    const touch = (a: PlacedXTick, b: PlacedXTick) => {
      const [s, t] = [xSpan(a), xSpan(b)];
      return s.from < t.to + X_LABEL_GAP && t.from < s.to + X_LABEL_GAP;
    };
    // Every k-th regular tick from the first, for the smallest k at which no two kept labels
    // touch: the kept ticks stay evenly spaced (C#17: dropping only the touching one printed
    // "#0 #2,000", then jumped to "#6,000").
    // A single kept label always fits, so the search ends by k = the tick count.
    const everyKth = (k: number) => regular.filter((_, i) => i % k === 0);
    const apart = (kept: readonly PlacedXTick[]) =>
      kept.every((tick, i) => i === 0 || !touch(kept[i - 1], tick));
    let k = 1;
    while (!apart(everyKth(k))) k++;
    const kept = [
      ...everyKth(k).filter((tick) => edges.every((edge) => !touch(edge, tick))),
      ...edges,
    ].sort((a, b) => a.index - b.index);
    return { ticks: kept, rows: 1 };
  }
  const centred = ticks.map((tick) => {
    const w = tick.label.length * AXIS_CHAR_PX;
    if (w > width)
      throw new Error(`categorical label "${tick.label}" is wider than its ${width} px chart`);
    const x = Math.min(width - w / 2, Math.max(w / 2, tick.at));
    return { index: tick.index, label: tick.label, x, anchor: 'middle' as const };
  });
  const spans = centred.map(xSpan);
  // Rows of every r-th label: the first r for which no label touches the next one on its row.
  let rows = 1;
  while (spans.some((s, i) => i + rows < spans.length && spans[i + rows].from < s.to + X_LABEL_GAP))
    rows++;
  return { ticks: centred.map((t, i) => ({ ...t, row: i % rows })), rows };
}

/** The advance of one glyph of a y-axis tick label, set at the tick size (6 px at 10 px). */
export const TICK_CHAR_PX = advancePx(SVG_TEXT.tick);
/** Clear space between a y tick label's end and the plot. */
const TICK_TO_PLOT = 8;

/**
 * The y-axis gutter that holds every tick label whole: the longest label plus the space between it
 * and the plot, and never narrower than 48 px.
 */
export function tickGutter(labels: readonly string[]): number {
  return Math.max(48, ...labels.map((label) => label.length * TICK_CHAR_PX + TICK_TO_PLOT));
}
