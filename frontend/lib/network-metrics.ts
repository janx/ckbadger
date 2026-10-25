/** Units and consensus targets for the homepage network charts. */

export interface HashRateUnit {
  suffix: string;
  divisor: number;
}

export function exactHashRateUnit(maxHashesPerSecond: bigint): { suffix: string; divisor: bigint } {
  if (maxHashesPerSecond < BigInt(0)) throw new Error('hash rate is negative');
  for (const [suffix, divisor] of [
    ['EH/s', BigInt(10) ** BigInt(18)],
    ['PH/s', BigInt(10) ** BigInt(15)],
    ['TH/s', BigInt(10) ** BigInt(12)],
    ['GH/s', BigInt(10) ** BigInt(9)],
    ['MH/s', BigInt(10) ** BigInt(6)],
    ['KH/s', BigInt(10) ** BigInt(3)],
  ] as const) {
    if (maxHashesPerSecond >= divisor) return { suffix, divisor };
  }
  return { suffix: 'H/s', divisor: BigInt(1) };
}

const HASH_RATE_UNITS: HashRateUnit[] = [
  { suffix: 'EH/s', divisor: 1e18 },
  { suffix: 'PH/s', divisor: 1e15 },
  { suffix: 'TH/s', divisor: 1e12 },
  { suffix: 'GH/s', divisor: 1e9 },
  { suffix: 'MH/s', divisor: 1e6 },
  { suffix: 'KH/s', divisor: 1e3 },
];

/**
 * The unit a series of hash rates in H/s reads best in: the largest unit its maximum is at least
 * one of (mainnet PH/s, a testnet far less). `/charts/hash-rate` reports H/s; no conversion of
 * the values happens here beyond dividing by the chosen unit.
 */
export function hashRateUnit(maxHashesPerSecond: number): HashRateUnit {
  if (!Number.isFinite(maxHashesPerSecond) || maxHashesPerSecond < 0) {
    throw new Error(`hash rate ${maxHashesPerSecond} is not a non-negative number of H/s`);
  }
  return (
    HASH_RATE_UNITS.find((u) => maxHashesPerSecond >= u.divisor) ?? { suffix: 'H/s', divisor: 1 }
  );
}

export interface OrphanRateTarget {
  numerator: number;
  denominator: number;
}

/**
 * The NC-Max orphan-rate target (`orphanRateTarget` on `/charts/uncle-rate`, read from the node's
 * consensus) as a percentage. Missing or malformed is an API contract violation and throws: the
 * target is a consensus parameter, never a number the frontend assumes.
 */
export function orphanRateTargetPercent(target: OrphanRateTarget | undefined | null): number {
  if (target === undefined || target === null) {
    throw new Error('the uncle-rate response has no orphanRateTarget');
  }
  const { numerator, denominator } = target;
  if (!Number.isInteger(numerator) || numerator < 0) {
    throw new Error(`orphanRateTarget numerator ${numerator} is not a non-negative integer`);
  }
  if (!Number.isInteger(denominator) || denominator <= 0) {
    throw new Error(`orphanRateTarget denominator ${denominator} is not a positive integer`);
  }
  return (numerator / denominator) * 100;
}

/** A percentage with at most two decimals and no trailing zeros: `2.5`, `2.64`, `3`. */
export function formatPercentShort(percent: number): string {
  if (!Number.isFinite(percent)) throw new Error(`percentage ${percent} is not finite`);
  return percent.toFixed(2).replace(/\.?0+$/, '');
}
