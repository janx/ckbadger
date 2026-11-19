import { sentField } from '@/lib/api-fields';
import { exactUnsignedInteger } from '@/lib/exact-integer';

/**
 * One day of a "CKB held" history as the routes state it (lane exactness, part B, the third read's S4): the whole its cells hold
 * (`capacity`), the part they occupy (`occupiedCapacity`) and the free rest (`freeCapacity`), the
 * names every "CKB held" figure of the API takes (the token history's days, the cell versions).
 * The page reads the three and sums or subtracts nothing. An API older than this page named the
 * parts `used` and `unused`: it is refused by name, never read as missing figures.
 */
export interface HeldPoint {
  capacity: string;
  occupiedCapacity: string;
  freeCapacity: string;
}

/** The three figures of a day of a "CKB held" history, each an exact integer, or a refusal. */
export function heldPoint(
  values: Readonly<Record<string, string | undefined>>,
  context: string
): HeldPoint {
  if (values.occupiedCapacity === undefined && ('used' in values || 'unused' in values))
    throw new Error(
      `${context}: the API sends used and unused, not occupiedCapacity and freeCapacity: it is older than this page`
    );
  // a figure the route sends that is missing is from an API older than this page, refused by name
  // (lib/api-fields.ts `sentField`, the other readers' words)
  const read = (key: keyof HeldPoint, word: string) =>
    exactUnsignedInteger(sentField(values[key], context, key), `${context}: ${word}`).toString();
  return {
    capacity: read('capacity', 'capacity'),
    occupiedCapacity: read('occupiedCapacity', 'occupied'),
    freeCapacity: read('freeCapacity', 'free'),
  };
}

/** A "CKB held" history's two bands, as the routes send them: the occupied and the free part. */
export const HELD_SERIES_KEYS = ['occupiedCapacity', 'freeCapacity'] as const;

/** Whether a history's series are the held bands, refusing an older API's `used` and `unused`. */
export function assertHeldSeries(series: readonly { key: string }[], context: string): void {
  if (series.some((s) => s.key === 'used' || s.key === 'unused'))
    throw new Error(
      `${context}: the API sends the bands used and unused, not occupiedCapacity and freeCapacity: it is older than this page`
    );
  for (const key of HELD_SERIES_KEYS)
    if (!series.some((s) => s.key === key)) throw new Error(`${context}: no ${key} series`);
}

/**
 * A cell version's free part, the route's `freeCapacity` (lane exactness, part B5): an API older than
 * this page sends none, and is refused by name, never read as `BigInt(undefined)`.
 */
export function versionFree(version: { freeCapacity?: string }, context: string): bigint {
  return exactUnsignedInteger(
    sentField(version.freeCapacity, context, 'freeCapacity'),
    `${context}: free`
  );
}
