/**
 * Series helpers for the homepage charts: day labels, chart numbers, day-over-day changes of a
 * cumulative CKB series, and the in-progress last bucket of `/statistics/tx-stats`.
 */
import type { ChartDataPoint } from '@/lib/api';
import { parseCkbDecimal } from '@/lib/ckb-decimal';

const ISO_DAY_RE = /^(\d{4})-(\d{2})-(\d{2})$/;
const COMPACT_DAY_RE = /^(\d{4})(\d{2})(\d{2})$/;

function dayParts(date: string): [string, string, string] {
  const m = ISO_DAY_RE.exec(date) ?? COMPACT_DAY_RE.exec(date);
  if (!m) throw new Error(`"${date}" is not a YYYY-MM-DD or YYYYMMDD day`);
  return [m[1], m[2], m[3]];
}

/** `2026-10-23` or `20261023` as the axis label `10/23`. */
export function dayLabel(date: string): string {
  const [, month, day] = dayParts(date);
  return `${month}/${day}`;
}

function dayNumber(date: string): number {
  const [y, m, d] = dayParts(date);
  return Date.UTC(Number(y), Number(m) - 1, Number(d)) / 86_400_000;
}

/** A chart point's value as a number; a value that is not one is a contract violation. */
export function chartNumber(point: ChartDataPoint): number {
  const value = Number(point.value);
  if (point.value.trim() === '' || !Number.isFinite(value)) {
    throw new Error(`chart value "${point.value}" on ${point.date} is not a number`);
  }
  return value;
}

export interface DailyDelta {
  date: string;
  /** The change since the previous day, in exact shannons. */
  delta: bigint;
}

/**
 * The day-over-day change of a cumulative daily CKB series (knowledge size, DAO deposits) over its
 * last `days` days, exact in shannons. The one derivation both homepage change charts use. The
 * series must be one point per consecutive day; a missing day would fold two days into one bar,
 * so it throws instead.
 */
export function dailyDeltas(points: ChartDataPoint[], days: number): DailyDelta[] {
  if (!Number.isInteger(days) || days < 1) {
    throw new Error(`dailyDeltas: days must be a positive integer, got ${days}`);
  }
  const window = points.slice(-(days + 1));
  const out: DailyDelta[] = [];
  for (let i = 1; i < window.length; i++) {
    const prev = window[i - 1];
    const cur = window[i];
    if (dayNumber(cur.date) !== dayNumber(prev.date) + 1) {
      throw new Error(`daily series skips from ${prev.date} to ${cur.date}`);
    }
    out.push({ date: cur.date, delta: parseCkbDecimal(cur.value) - parseCkbDecimal(prev.value) });
  }
  return out;
}

export interface InProgressSplit<T> {
  /** Every bucket that is over, oldest first. */
  complete: T[];
  /** The last bucket: the hour or day still in progress. */
  inProgress: T;
}

/**
 * Split a `/statistics/tx-stats` series into its complete buckets and the in-progress one. The
 * endpoint's contract is that the LAST bucket of `hourlyData` and of `dailyData` is the hour / day
 * still in progress (it also reports it as `currentHour` / `currentDay`); the split never guesses
 * from labels. An empty series has no current bucket, which the contract does not allow.
 */
export function splitInProgress<T>(buckets: T[], series: string): InProgressSplit<T> {
  if (buckets.length === 0) {
    throw new Error(`${series} has no buckets, not even the one in progress`);
  }
  return { complete: buckets.slice(0, -1), inProgress: buckets[buckets.length - 1] };
}
