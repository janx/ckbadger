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
 */
export function ageSeconds(timestamp: string | number | Date, now = Date.now()): Age {
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
 * A fact rail's relative age in its two largest units (Transaction board: "2d 20h ago"); feed
 * rows keep `formatTimeAgo`'s one unit. Truncates, never rounds up.
 */
export function formatAgeTwoUnits(timestamp: string | number | Date, now = Date.now()): string {
  const age = ageSeconds(timestamp, now);
  if (age.kind === 'clock-behind') return CLOCK_BEHIND;
  const { seconds } = age;
  const units: [number, string][] = [
    [86400, 'd'],
    [3600, 'h'],
    [60, 'm'],
    [1, 's'],
  ];
  const first = units.findIndex(([size]) => seconds >= size);
  if (first === -1) return '0s ago';
  const [size, unit] = units[first];
  const lead = `${Math.floor(seconds / size)}${unit}`;
  const next = units[first + 1];
  if (!next) return `${lead} ago`;
  const rest = Math.floor((seconds % size) / next[0]);
  return rest === 0 ? `${lead} ago` : `${lead} ${rest}${next[1]} ago`;
}
