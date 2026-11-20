/**
 * Exact CKB arithmetic for the homepage: values arrive as shannon integers or as CKB decimal
 * strings, and everything shown as an exact amount is computed in BigInt shannons.
 */
import { MINUS } from '@/lib/chart-ticks';
import { cutScaledInteger } from '@/lib/exact-integer';
import { ckbAmountText } from '@/lib/amount-form';

export const SHANNONS_PER_CKB = BigInt(100_000_000);

const CKB_DECIMAL_RE = /^(-)?(\d+)(?:\.(\d{1,8}))?$/;

/** A CKB decimal string (`8131306075.44505907`, `-2578825.4868`, `151104471`) as exact shannons. */
export function parseCkbDecimal(raw: string): bigint {
  const match = CKB_DECIMAL_RE.exec(raw);
  if (!match) {
    throw new Error(`"${raw}" is not a CKB amount with at most 8 decimals`);
  }
  const [, minus, whole, fraction = ''] = match;
  const shannons = BigInt(whole) * SHANNONS_PER_CKB + BigInt(fraction.padEnd(8, '0'));
  return minus ? -shannons : shannons;
}

/** A shannon string from the API (`812889561643799700`) as a BigInt; anything else throws. */
export function parseShannons(raw: string, field: string): bigint {
  if (!/^-?\d+$/.test(raw)) {
    throw new Error(`${field} "${raw}" is not an integer shannon amount`);
  }
  return BigInt(raw);
}

/** Shannons as a float CKB number — for drawing geometry only, never for a printed amount. */
export function shannonsToCkbNumber(shannons: bigint): number {
  return Number(shannons) / 1e8;
}

function groupThousands(n: bigint): string {
  return n.toLocaleString('en-US');
}

/** Whole CKB (truncated toward zero) with an explicit sign: `+82,991`, `−2,578,825`, `0`. */
export function formatSignedCkbInteger(shannons: bigint): string {
  const whole = shannons / SHANNONS_PER_CKB; // BigInt division truncates toward zero
  if (whole === BigInt(0)) return shannons === BigInt(0) ? '0' : shannons > 0 ? '+0' : `${MINUS}0`;
  return whole > 0 ? `+${groupThousands(whole)}` : `${MINUS}${groupThousands(-whole)}`;
}

/** Exact CKB with an explicit sign and all 8 decimals: `+168,366.47983386`, `−2,578,825.48689593`. */
export function formatSignedCkbExact(shannons: bigint): string {
  const abs = shannons < BigInt(0) ? -shannons : shannons;
  const whole = groupThousands(abs / SHANNONS_PER_CKB);
  const frac = (abs % SHANNONS_PER_CKB).toString().padStart(8, '0');
  const sign = shannons > BigInt(0) ? '+' : shannons < BigInt(0) ? MINUS : '';
  return `${sign}${whole}.${frac}`;
}

/** A signed count: `+12`, `−3`, `0`. */
export function formatSignedCount(n: number): string {
  if (!Number.isInteger(n)) throw new Error(`formatSignedCount: ${n} is not an integer`);
  if (n === 0) return '0';
  return n > 0 ? `+${n.toLocaleString('en-US')}` : `${MINUS}${(-n).toLocaleString('en-US')}`;
}

/**
 * `part / total` as a percentage, exact and cut at `decimals` places by the one cut function
 * (`cutScaledInteger`), never rounded (`0.30`; lane exactness, part B2: it rounded half up, 2 of 3 read 66.67).
 */
export function formatPercent(part: bigint, total: bigint, decimals = 2): string {
  if (total <= BigInt(0)) throw new Error(`formatPercent: total must be positive, got ${total}`);
  if (part < BigInt(0)) throw new Error(`formatPercent: part must not be negative, got ${part}`);
  return cutScaledInteger(part * BigInt(100), total, decimals);
}

/**
 * Whole bytes (1 CKB of capacity = 1 byte) in decimal units with one decimal: `151.1 MB`, in the
 * unit the exact value reaches and cut in it (lane exactness, part B3: it chose the unit at
 * unit − unit / 20,000 and rounded half up, so 999,950 bytes read 1.0 MB).
 */
export function formatDecimalBytes(bytes: bigint): string {
  if (bytes < BigInt(0)) throw new Error(`formatDecimalBytes: ${bytes} is negative`);
  const units: [bigint, string][] = [
    [BigInt(1e12), 'TB'],
    [BigInt(1e9), 'GB'],
    [BigInt(1e6), 'MB'],
    [BigInt(1e3), 'kB'],
  ];
  for (const [div, suffix] of units) {
    if (bytes >= div) return `${cutScaledInteger(bytes, div, 1)} ${suffix}`;
  }
  return `${bytes} B`;
}

/**
 * The digits `CkbAmount` prints for an amount in its column form, as plain text without the unit
 * (`0.00001647`, `750.00000000`): the column form of `lib/amount-form.ts`, the one formatter, with
 * the sign only when `signed` and zero as `0` (consistency plan C4). A sentence that names an
 * amount the page also draws (the transaction comment's fee, a cell seal's label) takes it from
 * here, so the two never disagree (pinned against `CkbAmount` in its test).
 */
export function exactCkbText(shannons: bigint, signed: boolean): string {
  return ckbAmountText(shannons, 'column', signed);
}
