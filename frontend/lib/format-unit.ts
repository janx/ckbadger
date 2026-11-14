/**
 * How a number meets its unit (UI review 2026-11-14, CH-03, CH-12, CH-14; `docs/RESPONSIVE_LAYOUT.md`,
 * "Formats"): the one home for unit forms, lifted from the chart template (lane B3). A percentage
 * takes `%` with no space, any other unit follows its number after a space (a no-break space in a
 * display figure); a physical quantity keeps its SI prefix with its unit symbol (`PH/s`), a count
 * or a CKB amount the compact k / M / B / T of `formatCompactInteger` and `formatTick`. Counts are
 * grouped by `formatCount`, exact values by their own formatters; this module only joins and
 * spells.
 */
import { MINUS, formatTick, stepDecimals } from '@/lib/chart-ticks';

/**
 * What follows a number to name its unit (CH-14): a percentage takes its sign with no space
 * (`24.79%`), any other unit follows `space` (`10.43 s`); a display figure passes a no-break
 * space so its unit never wraps away from it. No unit, nothing.
 */
export function unitAfter(unit: string, space = ' '): string {
  if (unit === '') return '';
  return unit === '%' ? '%' : `${space}${unit}`;
}

/** A value with its unit, in the one form: `24.79%`, `10.43 s`, `16,826 transactions` (CH-14). */
export function withUnit(value: string, unit: string): string {
  return `${value}${unitAfter(unit)}`;
}

/** A window between two dates: `2026-11-07 → 2026-11-13` (CH-03). */
export function formatDateWindow(from: string, to: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(from) || !/^\d{4}-\d{2}-\d{2}$/.test(to))
    throw new Error(`invalid date window ${from}, ${to}`);
  return `${from} → ${to}`;
}

/** Units that carry an SI prefix already (peta): their axis groups digits, no k / M (CH-12). */
const PREFIXED_UNITS: ReadonlySet<string> = new Set(['P', 'PH/s']);

/**
 * The y-axis number for a chart in `unit`: one prefix per value (CH-12), so a unit that is already
 * prefixed reads `6,000 P`, never `6k P`; every other unit takes the axis' k / M / B.
 */
export function chartAxisFormat(unit: string): (value: number, step: number) => string {
  if (!PREFIXED_UNITS.has(unit)) return formatTick;
  return (value, step) => {
    const decimals = stepDecimals(step);
    const text = Math.abs(value).toLocaleString('en-US', {
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    });
    return value < 0 && Number(text.replace(/,/g, '')) !== 0 ? `${MINUS}${text}` : text;
  };
}

const MAGNITUDE: Readonly<Record<string, string>> = { '': '', k: 'k', m: 'M' };

/**
 * A Cell size bucket in the axis' words (CH-12, CH-14): `100k-1m CKB` → `100k – 1M CKB`,
 * `<100 CKB` → `< 100 CKB`, `>=1m CKB` → `≥ 1M CKB`: one prefix form (M, as `1M cells` on the
 * axis beside it) and a range with an en dash and spaces. A label in no known form fails.
 */
export function cellSizeBucketLabel(label: string): string {
  const match = /^(<|>=)?(\d+)([km]?)(?:-(\d+)([km]?))? CKB$/.exec(label);
  if (!match) throw new Error(`cell size: unknown bucket ${JSON.stringify(label)}`);
  const [, bound, from, fromUnit, to, toUnit] = match;
  const first = `${from}${MAGNITUDE[fromUnit]}`;
  if (bound !== undefined && to === undefined)
    return bound === '<' ? `< ${first} CKB` : `≥ ${first} CKB`;
  if (bound === undefined && to !== undefined) return `${first} – ${to}${MAGNITUDE[toUnit]} CKB`;
  throw new Error(`cell size: unknown bucket ${JSON.stringify(label)}`);
}
