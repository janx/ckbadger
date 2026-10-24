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
  for (let d = 0; d <= 12; d++) {
    const scaled = step * 10 ** d;
    if (Math.abs(scaled - Math.round(scaled)) < 1e-6) return d;
  }
  throw new Error(`stepDecimals: ${step} has no short decimal form`);
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
    values.push(Number((anchor + i * step).toFixed(decimals)));
  }
  return { values, step };
}

function trimZeros(s: string): string {
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s;
}

/**
 * A tick label: thousands as k / M / B (trailing zeros trimmed: 500, 1k, 2.5k, 10k), smaller
 * values with the decimals their step needs (2.4, 2.6 …), negatives with a real minus sign.
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
  return `${sign}${abs.toFixed(stepDecimals(step))}`;
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
