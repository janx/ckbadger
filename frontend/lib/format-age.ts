/**
 * How far a time may run ahead of this device's clock and still read as "now". A block's timestamp
 * and a pool entry's `pendingSince` come from the node's clock, which is not this device's: CKB
 * consensus itself accepts a block whose timestamp is up to 15 s after the validating node's clock
 * (ckb `ALLOWED_FUTURE_BLOCKTIME`, 15 * 1000 ms, checked by its header timestamp verifier).
 */
export const ALLOWED_FUTURE_MS = 15_000;

/** What a relative age says instead when the time is further ahead than consensus allows. */
export const CLOCK_BEHIND = "this device's clock is behind";

export type Age = { kind: 'age'; seconds: number } | { kind: 'clock-behind' };

/**
 * The one rule for a relative age (review 45fd ruling D4): whole seconds since the time,
 * truncated; a time at most ALLOWED_FUTURE_MS ahead of `now` is age zero; a time further ahead has
 * no age, as this device's clock is behind the chain's. A timestamp that is not a time throws.
 * `now` has no default (lane E4): an age on screen takes it from the page's one clock
 * (`useClockText` in components/ui/utc8-date.tsx), so it moves with no data change; a read of
 * the wall clock here wrote it once and left it standing.
 */
export function ageSeconds(timestamp: string | number | Date, now: number): Age {
  const then =
    timestamp instanceof Date
      ? timestamp.getTime()
      : typeof timestamp === 'number'
        ? timestamp
        : /^\d+$/.test(timestamp)
          ? Number(timestamp)
          : Date.parse(timestamp);
  if (!Number.isFinite(then)) throw new Error(`Invalid timestamp ${JSON.stringify(timestamp)}`);
  if (then > now)
    return then - now > ALLOWED_FUTURE_MS ? { kind: 'clock-behind' } : { kind: 'age', seconds: 0 };
  return { kind: 'age', seconds: Math.floor((now - then) / 1000) };
}

/**
 * The one form of a duration and an age (U-15): a number and its unit letter, `y d h m s`, the
 * two largest units, a zero second unit left out — "4m 32s", "26m 14s", "52s", "3d 4h", "2d".
 * Truncated, never rounded up. A span has no calendar, so its largest unit is the day ("2555d");
 * an age is a span back from now, so from a year on it reads calendar years and days on the
 * site's UTC+8 clock ("6y 363d ago"), where a day count stops being readable.
 */
const SPAN_UNITS: readonly [number, string][] = [
  [86400, 'd'],
  [3600, 'h'],
  [60, 'm'],
  [1, 's'],
];

/** A span of whole seconds in its two largest units: "26m 14s", "52s", "3d 4h", "0s". */
export function formatSpan(seconds: number): string {
  if (!Number.isSafeInteger(seconds) || seconds < 0) throw new Error(`invalid span ${seconds}`);
  const first = SPAN_UNITS.findIndex(([size]) => seconds >= size);
  if (first === -1) return '0s';
  const [size, unit] = SPAN_UNITS[first];
  const lead = `${Math.floor(seconds / size)}${unit}`;
  const next = SPAN_UNITS[first + 1];
  if (!next) return lead;
  const rest = Math.floor((seconds % size) / next[0]);
  return rest === 0 ? lead : `${lead} ${rest}${next[1]}`;
}

const UTC8_MS = 8 * 3600 * 1000;

/**
 * Whole calendar years from `then` to `now` on the UTC+8 clock (the anniversary of `then` at its
 * UTC+8 date and time), and the whole days after the last anniversary; null under a year.
 */
function calendarYears(then: number, now: number): string | null {
  const a = new Date(then + UTC8_MS);
  const b = new Date(now + UTC8_MS);
  const anniversary = (years: number) =>
    Date.UTC(
      a.getUTCFullYear() + years,
      a.getUTCMonth(),
      a.getUTCDate(),
      a.getUTCHours(),
      a.getUTCMinutes(),
      a.getUTCSeconds(),
      a.getUTCMilliseconds()
    );
  let years = b.getUTCFullYear() - a.getUTCFullYear();
  if (anniversary(years) > b.getTime()) years -= 1;
  if (years < 1) return null;
  const days = Math.floor((b.getTime() - anniversary(years)) / 86_400_000);
  return days === 0 ? `${years}y` : `${years}y ${days}d`;
}

function timeMs(timestamp: string | number | Date): number {
  return timestamp instanceof Date
    ? timestamp.getTime()
    : typeof timestamp === 'number'
      ? timestamp
      : /^\d+$/.test(timestamp)
        ? Number(timestamp)
        : Date.parse(timestamp);
}

/**
 * A time's age in its largest unit, "5m", "3h" (from a year on, years and days: "6y 363d"), the
 * notation of the one form; null when the time is further ahead than consensus allows, as this
 * device's clock is behind the chain's. A pool entry's wait reads it ("in pool for 5m").
 */
export function ageInOneUnit(timestamp: string | number | Date, now: number): string | null {
  const age = ageSeconds(timestamp, now);
  if (age.kind === 'clock-behind') return null;
  const years = calendarYears(timeMs(timestamp), now);
  if (years !== null) return years;
  const unit =
    SPAN_UNITS.find(([size]) => age.seconds >= size) ?? SPAN_UNITS[SPAN_UNITS.length - 1];
  return `${Math.floor(age.seconds / unit[0])}${unit[1]}`;
}

/** A feed row's relative age in its largest unit ("5m ago"); from a year on, "6y 363d ago". */
export function formatAgeOneUnit(timestamp: string | number | Date, now: number): string {
  const age = ageInOneUnit(timestamp, now);
  return age === null ? CLOCK_BEHIND : `${age} ago`;
}
