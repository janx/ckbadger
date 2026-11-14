import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';
import { formatScaledInteger } from '@/lib/exact-integer';
import { TYPE_STEPS } from '@/lib/type-scale';
import { formatAgeOneUnit, formatSpan } from '@/lib/format-age';

// tailwind-merge reads an unknown `text-<word>` as a colour; the scale's steps are sizes, so
// `text-text-dim text-meta` keeps both classes and `text-body text-section` keeps the last.
const twMerge = extendTailwindMerge({
  extend: { classGroups: { 'font-size': [{ text: [...TYPE_STEPS] }] } },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Raw block timestamp displayed in the indexer's fixed UTC+8 calendar timezone. */
export function formatUtc8Date(timestamp: string | number): string {
  return utc8Iso(timestampMs(timestamp, 'block timestamp')).slice(0, 10);
}

/** A feed row's relative age in one unit ("5m ago"): format-age's one form (U-15). */
export function formatTimeAgo(timestamp: string | number | Date, now = Date.now()): string {
  return formatAgeOneUnit(timestamp, now);
}

const UTC8_OFFSET_MS = 8 * 60 * 60 * 1000;

/** Milliseconds since the epoch from a ms number, a decimal ms string, or an ISO string. */
function timestampMs(timestamp: string | number | Date, what = 'timestamp'): number {
  const value =
    timestamp instanceof Date
      ? timestamp.getTime()
      : typeof timestamp === 'number'
        ? timestamp
        : /^\d+$/.test(timestamp)
          ? Number(timestamp)
          : Date.parse(timestamp);
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`Invalid ${what} ${JSON.stringify(timestamp)}`);
  return value;
}

function utc8Iso(ms: number): string {
  return new Date(ms + UTC8_OFFSET_MS).toISOString();
}

/** The site's one clock (D2): `2026-10-22 22:43:55 UTC+8`, for chain and observer times alike. */
export function formatUtc8DateTime(timestamp: string | number | Date): string {
  const shifted = utc8Iso(timestampMs(timestamp));
  return `${shifted.slice(0, 10)} ${shifted.slice(11, 19)} UTC+8`;
}

/** The time-of-day part of the same clock: `22:43:55 UTC+8`. */
export function formatUtc8Time(timestamp: string | number | Date): string {
  return formatUtc8DateTime(timestamp).slice(11);
}

/** Protocol/cache Unix-second timestamps on the same UTC+8 clock (OB-1). */
export function formatUnixSecondsUtc8(seconds: string): string {
  if (!/^(0|[1-9]\d*)$/.test(seconds)) {
    throw new Error(`Invalid Unix seconds ${JSON.stringify(seconds)}`);
  }
  const ms = BigInt(seconds) * BigInt(1000);
  if (ms > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error(`Unix seconds ${seconds} exceeds safe Date range`);
  }
  return formatUtc8DateTime(Number(ms));
}

/**
 * Human duration from an exact millisecond count (SC-C1: not raw "… ms"), in format-age's one
 * form (U-15): "26m 14s", "52s"; under a second "450ms".
 */
export function formatDurationMs(ms: number): string {
  if (!Number.isSafeInteger(ms) || ms < 0) {
    throw new Error(`invalid duration ms ${ms}`);
  }
  if (ms < 1000) return `${ms}ms`;
  return formatSpan(Math.floor(ms / 1000));
}

export function formatCkbAmount(shannons: string | bigint): {
  integer: string;
  decimal: string;
  full: string;
  isNegative: boolean;
} {
  const value = BigInt(shannons);
  const isNegative = value < BigInt(0);
  const absValue = isNegative ? -value : value;

  const ckbWhole = absValue / BigInt(100_000_000);
  const ckbFraction = absValue % BigInt(100_000_000);

  const integer = ckbWhole.toLocaleString('en-US');
  const decimal = ckbFraction.toString().padStart(8, '0');
  const full = `${isNegative ? '-' : ''}${integer}.${decimal}`;

  return { integer, decimal, full, isNegative };
}

export function formatCapacity(capacity: string | bigint): string {
  const { full } = formatCkbAmount(capacity);
  return `${full} CKB`;
}

/** The compact CKB units in shannons, largest first (CH-14). */
const CKB_COMPACT_UNITS = [
  ['T', BigInt('100000000000000000000')],
  ['B', BigInt('100000000000000000')],
  ['M', BigInt('100000000000000')],
  ['k', BigInt('100000000000')],
] as const;
const ONE_CKB = BigInt(100000000);

/**
 * A non-negative shannon amount in the largest compact unit it REACHES once rounded at two
 * decimals of the unit below (the review's item J, as formatCompactInteger chooses): an amount from
 * 999.995 of the unit below is 1.00 of this one, so 999,995 CKB is 1.00M, never 1000.00k. The
 * threshold is the divisor less 1/200,000 of it. No unit reached, null.
 */
function compactCkbUnits(abs: bigint): string | null {
  for (const [suffix, divisor] of CKB_COMPACT_UNITS) {
    if (abs >= divisor - divisor / BigInt(200000))
      return `${formatScaledInteger(abs, divisor, 2)}${suffix}`;
  }
  return null;
}

export function formatCkbCompact(shannons: string | bigint): {
  value: string;
  full: string;
} {
  const amount = BigInt(shannons);
  if (amount < BigInt(0)) throw new Error(`negative capacity ${amount} shannons`);
  const full = formatCkbAmount(shannons).full;
  return { value: compactCkbUnits(amount) ?? formatScaledInteger(amount, ONE_CKB, 2), full };
}

export function formatNumber(num: number | bigint): string {
  return num.toLocaleString();
}
