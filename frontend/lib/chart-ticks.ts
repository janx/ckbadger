/**
 * Axis helpers for the homepage small multiples: round ticks, their labels, and the symmetric cap
 * of a signed (daily-change) chart. One helper per job, so every chart reads the same numbers.
 */

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
 * Round ticks covering [min, max]: about `target` of them, evenly spaced by a 1/2/2.5/5 × 10^k
 * step. They are multiples of the step, or — with `anchor` — the anchor plus multiples of it, so a
 * reference value (a consensus target) is itself a tick. A flat series gets a domain around its
 * value (or [0, 1] for zero) so the axis is never degenerate.
 */
export function niceTicks(min: number, max: number, target = 4, anchor = 0): Ticks {
  assertFinite('niceTicks: min', min);
  assertFinite('niceTicks: max', max);
  assertFinite('niceTicks: anchor', anchor);
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
  const step = niceStep((hi - lo) / (target - 1));
  const decimals = Math.max(stepDecimals(step), anchor === 0 ? 0 : stepDecimals(anchor));
  const first = Math.floor((lo - anchor) / step + 1e-9);
  const last = Math.ceil((hi - anchor) / step - 1e-9);
  const values: number[] = [];
  for (let i = first; i <= last; i++) {
    values.push(Number(tickDecimal(anchor + i * step, decimals)));
  }
  return { values, step };
}

function trimZeros(s: string): string {
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s;
}

/**
 * A tick label: thousands as k / M / B (trailing zeros trimmed: 500, 1k, 2.5k, 10k), smaller
 * values with the decimals their step needs (2.4, 2.6 …), micro-token values in scientific
 * notation, negatives with a real minus sign. Exact data readouts remain separate from axis labels.
 */
export function formatTick(value: number, step: number): string {
  assertFinite('formatTick: value', value);
  if (value === 0) return '0';
  const abs = Math.abs(value);
  const sign = value < 0 ? MINUS : '';
  const units: [number, string][] = [
    [1e9, 'B'],
    [1e6, 'M'],
    [1e3, 'k'],
  ];
  for (const [div, suffix] of units) {
    if (abs >= div) return `${sign}${trimZeros((abs / div).toFixed(3))}${suffix}`;
  }
  const decimal = tickDecimal(abs, stepDecimals(step));
  return `${sign}${abs < 1e-8 ? Number(decimal).toExponential() : decimal}`;
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

/** A compact signed amount for a clipped bar's label: +28.12M, −8.94M, +950k, +12.5k, −12. */
export function formatCompactSigned(value: number): string {
  assertFinite('formatCompactSigned: value', value);
  const sign = value > 0 ? '+' : value < 0 ? MINUS : '';
  const abs = Math.abs(value);
  if (abs >= 1e9) return `${sign}${(abs / 1e9).toFixed(2)}B`;
  if (abs >= 1e6) return `${sign}${(abs / 1e6).toFixed(2)}M`;
  if (abs >= 1e3) return `${sign}${trimZeros((abs / 1e3).toFixed(1))}k`;
  return `${sign}${abs.toFixed(0)}`;
}

/** The advance of one 10 px JetBrains Mono glyph, for keeping axis labels apart. */
export const AXIS_CHAR_PX = 6.2;
/** The clear space kept between two x labels on one row. */
const X_LABEL_GAP = 6;
/** A time-axis label this close to a plot edge anchors to that edge instead of centring. */
const X_EDGE_ANCHOR = 20;

export interface XTickInput {
  index: number;
  label: string;
  /** The x of the point (or slot centre) the label names. */
  at: number;
}

export interface PlacedXTick {
  index: number;
  label: string;
  x: number;
  anchor: 'start' | 'middle' | 'end';
  /** 0 for the first label row under the plot, 1 for the one below it, and so on. */
  row: number;
}

const xSpan = (t: { x: number; anchor: PlacedXTick['anchor']; label: string }) => {
  const w = t.label.length * AXIS_CHAR_PX;
  const from = t.anchor === 'start' ? t.x : t.anchor === 'end' ? t.x - w : t.x - w / 2;
  return { from, to: from + w };
};

/**
 * Where the x labels go. A time axis drops a label that would touch the one before it. A
 * categorical axis (a few named buckets, one bar each) keeps every label: centred under its bar,
 * held inside the chart, and stacked onto as many rows as it takes for no two on a row to touch.
 */
export function placeXTicks(
  ticks: readonly XTickInput[],
  frame: { left: number; right: number; width: number; categorical: boolean }
): { ticks: PlacedXTick[]; rows: number } {
  const { left, right, width, categorical } = frame;
  if (!categorical) {
    const kept: PlacedXTick[] = [];
    for (const tick of ticks) {
      const anchor: PlacedXTick['anchor'] =
        tick.at - left < X_EDGE_ANCHOR
          ? 'start'
          : right - tick.at < X_EDGE_ANCHOR
            ? 'end'
            : 'middle';
      const placed = { index: tick.index, label: tick.label, x: tick.at, anchor, row: 0 };
      const previous = kept[kept.length - 1];
      if (!previous || xSpan(placed).from >= xSpan(previous).to + X_LABEL_GAP) kept.push(placed);
    }
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
