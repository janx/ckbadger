import type { VesselCell, VesselSnapshot } from '@/lib/api';

const SHANNONS_PER_BYTE = BigInt(100_000_000);

export interface DrawVessel {
  cell: VesselCell;
  fullness: number;
  areaWeight: number;
  entrance: 'created' | 'sampled';
}

/** Keep chain quantities exact until they become diagram coordinates. */
export function readVessel(cell: VesselCell, previousAnchor: number | null): DrawVessel {
  const capacity = BigInt(cell.capacityShannons);
  const occupied = BigInt(cell.occupiedShannons);
  const parts = cell.anatomy;
  const total =
    parts.capacityFieldBytes + parts.lockScriptBytes + parts.typeScriptBytes + parts.dataBytes;
  if (
    !Number.isSafeInteger(total) ||
    parts.capacityFieldBytes !== 8 ||
    parts.lockScriptBytes < 33 ||
    parts.typeScriptBytes < 0 ||
    parts.dataBytes < 0 ||
    parts.totalBytes !== total ||
    capacity <= BigInt(0) ||
    occupied !== BigInt(total) * SHANNONS_PER_BYTE ||
    occupied > capacity
  ) {
    throw new Error(`Invalid canonical vessel anatomy: ${cell.outpoint}`);
  }
  return {
    cell,
    // The fixed-point conversion is only a canvas coordinate, never a reported chain amount.
    fullness: Number((occupied * BigInt(1_000_000)) / capacity) / 1_000_000,
    areaWeight: Math.exp(Math.log(Number(capacity)) * 0.42),
    entrance:
      previousAnchor !== null && cell.createdAtBlock > previousAnchor ? 'created' : 'sampled',
  };
}

export interface VesselChange {
  entered: DrawVessel[];
  stayed: DrawVessel[];
  consumed: string[];
  rotatedOrReorganized: string[];
}

export function compareVesselSnapshots(
  previous: VesselSnapshot | null,
  next: VesselSnapshot
): VesselChange {
  const old = new Set(previous?.cells.map((cell) => cell.outpoint) ?? []);
  const fresh = new Set<string>();
  const entered: DrawVessel[] = [];
  const stayed: DrawVessel[] = [];
  for (const cell of next.cells) {
    if (fresh.has(cell.outpoint)) throw new Error(`Duplicate vessel outpoint: ${cell.outpoint}`);
    fresh.add(cell.outpoint);
    const drawn = readVessel(cell, previous?.anchorBlock ?? null);
    (old.has(cell.outpoint) ? stayed : entered).push(drawn);
  }
  const tracked = new Map(next.tracked.map((fate) => [fate.outpoint, fate]));
  const consumed: string[] = [];
  const rotatedOrReorganized: string[] = [];
  for (const outpoint of old) {
    if (fresh.has(outpoint)) continue;
    const fate = tracked.get(outpoint);
    if (!fate) throw new Error(`Missing tracked vessel fate: ${outpoint}`);
    if (fate.status === 'consumed') consumed.push(outpoint);
    else rotatedOrReorganized.push(outpoint);
  }
  return { entered, stayed, consumed, rotatedOrReorganized };
}

export interface VesselRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Capacity-weighted diagram layout; its geometry is intentionally not an aggregate. */
export function layoutVessels(
  vessels: DrawVessel[],
  width: number,
  height: number
): Map<string, VesselRect> {
  const result = new Map<string, VesselRect>();
  if (width <= 0 || height <= 0 || vessels.length === 0) return result;
  const sorted = [...vessels].sort(
    (a, b) => b.areaWeight - a.areaWeight || a.cell.outpoint.localeCompare(b.cell.outpoint)
  );
  const place = (items: DrawVessel[], x: number, y: number, w: number, h: number) => {
    if (items.length === 1) {
      result.set(items[0].cell.outpoint, { x, y, width: w, height: h });
      return;
    }
    const total = items.reduce((sum, item) => sum + item.areaWeight, 0);
    let sum = 0;
    let split = 0;
    while (split < items.length - 1 && (sum < total / 2 || split === 0)) {
      sum += items[split].areaWeight;
      split++;
    }
    const proportion = sum / total;
    if (w >= h) {
      place(items.slice(0, split), x, y, w * proportion, h);
      place(items.slice(split), x + w * proportion, y, w * (1 - proportion), h);
    } else {
      place(items.slice(0, split), x, y, w, h * proportion);
      place(items.slice(split), x, y + h * proportion, w, h * (1 - proportion));
    }
  };
  place(sorted, 0, 0, width, height);
  return result;
}
