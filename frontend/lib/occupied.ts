/**
 * The one writer of an occupied figure (readability lane B4 and its fix round): occupied capacity
 * is bytes, 1 CKB (10^8 shannons) of capacity for each, and a figure writes its symbol
 * (`docs/RESPONSIVE_LAYOUT.md`, "Formats"), so "occupied" always carries its unit: "61 B", a pair
 * "563 → 589 B" (the unit once, after the pair), a change "−61 B". Grouped, BigInt, never rounded.
 */
import { SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import { withUnit } from '@/lib/format-unit';
import { MINUS } from '@/lib/chart-ticks';

const grouped = (bytes: bigint) => bytes.toLocaleString('en-US');

function nonNegative(bytes: bigint, what: string): bigint {
  if (bytes < BigInt(0)) throw new Error(`${what} ${bytes} bytes is negative`);
  return bytes;
}

/** A count of occupied bytes: "61 B". */
export function occupiedBytesText(bytes: bigint): string {
  return withUnit(grouped(nonNegative(bytes, 'occupied')), 'B');
}

/** Occupied bytes before and after: "563 → 589 B", the unit once, after the pair. */
export function occupiedPairText(before: bigint, after: bigint): string {
  return withUnit(
    `${grouped(nonNegative(before, 'occupied before'))} → ${grouped(nonNegative(after, 'occupied after'))}`,
    'B'
  );
}

/** A change in occupied bytes, signed: "+26 B", "−61 B", "0 B". */
export function occupiedBytesDelta(bytes: bigint): string {
  const sign = bytes > BigInt(0) ? '+' : bytes < BigInt(0) ? MINUS : '';
  return withUnit(`${sign}${grouped(bytes < BigInt(0) ? -bytes : bytes)}`, 'B');
}

/**
 * A change in occupied capacity as the API sends it (shannons, `usedDelta`) as bytes with their
 * unit ("−61 B"). A change that is not a whole number of bytes is not one the chain can make, and
 * is refused with its value.
 */
export function formatOccupiedDelta(usedDelta: string): string {
  const shannons = BigInt(usedDelta);
  if (shannons % SHANNONS_PER_CKB !== BigInt(0))
    throw new Error(
      `occupied change ${usedDelta} shannons is not a whole number of bytes (10^8 shannons each)`
    );
  return occupiedBytesDelta(shannons / SHANNONS_PER_CKB);
}
