/** The epoch the API reports as `number(index/length)`, e.g. `15168(451/1590)`. */
export interface EpochInfo {
  number: number;
  index: number;
  length: number;
}

const EPOCH_RE = /^(\d+)\((\d+)\/(\d+)\)$/;

/**
 * Parse `/statistics/network`'s `epoch` field. A string that is not `number(index/length)`, or
 * whose index is not inside a non-empty epoch, is an API contract violation: it throws with the
 * raw value rather than inventing an epoch.
 */
export function parseEpoch(raw: string): EpochInfo {
  const match = EPOCH_RE.exec(raw);
  if (!match) {
    throw new Error(`epoch "${raw}" is not number(index/length)`);
  }
  const info = {
    number: Number(match[1]),
    index: Number(match[2]),
    length: Number(match[3]),
  };
  if (info.length === 0) {
    throw new Error(`epoch "${raw}" has length 0`);
  }
  if (info.index >= info.length) {
    throw new Error(`epoch "${raw}" index ${info.index} is outside its length ${info.length}`);
  }
  return info;
}

/** One written form of an epoch position: `15,173 · 1,001 / 1,472` (BA-41). */
export function formatEpochPosition({ number, index, length }: EpochInfo): string {
  for (const [name, value] of [
    ['number', number],
    ['index', index],
    ['length', length],
  ] as const)
    if (!Number.isSafeInteger(value) || value < 0)
      throw new Error(`epoch ${name} ${value} is not a non-negative integer`);
  if (index >= length) throw new Error(`epoch index ${index} is outside its length ${length}`);
  const n = (v: number) => v.toLocaleString('en-US');
  return `${n(number)} · ${n(index)} / ${n(length)}`;
}
