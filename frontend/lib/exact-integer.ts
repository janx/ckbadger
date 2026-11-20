/** Parse an API integer without passing it through IEEE-754 numbers. */
export function exactUnsignedInteger(value: string, context: string): bigint {
  if (!/^(0|[1-9]\d*)$/.test(value)) {
    throw new Error(
      `${context}: expected a non-negative decimal integer, received ${JSON.stringify(value)}`
    );
  }
  return BigInt(value);
}

/**
 * value ÷ divisor CUT at `decimals`, never rounded (the house rule, readability R5: a rounded
 * figure can show a digit that is not on chain; 1.005 reads 1.00): the one form of a figure that
 * shortens an exact ratio, the H-multiplier first (lane D4 follow-up).
 */
export function cutScaledInteger(value: bigint, divisor: bigint, decimals: number): string {
  if (value < BigInt(0) || divisor <= BigInt(0) || !Number.isInteger(decimals) || decimals < 0) {
    throw new Error('invalid exact integer display scale');
  }
  const scale = BigInt(10) ** BigInt(decimals);
  const shown = (value * scale) / divisor;
  if (decimals === 0) return shown.toString();
  return `${shown / scale}.${(shown % scale).toString().padStart(decimals, '0')}`;
}

/**
 * The same cut under its first name, which its callers read (the coordinator's ruling of
 * 2026-11-19: a shortened figure is never rounded; it rounded half-up, 1.005 read 1.01). No figure
 * on the site is rounded through it any more, so there is no half-up form.
 */
export const formatScaledInteger = cutScaledInteger;

export function formatCompactInteger(value: string, context: string): string {
  const integer = exactUnsignedInteger(value, context);
  for (const [suffix, divisor] of [
    ['E', BigInt(10) ** BigInt(18)],
    ['P', BigInt(10) ** BigInt(15)],
    ['T', BigInt(10) ** BigInt(12)],
    ['B', BigInt(10) ** BigInt(9)],
    ['M', BigInt(10) ** BigInt(6)],
    ['k', BigInt(10) ** BigInt(3)],
  ] as const) {
    // a unit from where the value reaches it: cut, 999,999 is 999.99k, never a rounded 1.00M
    if (integer >= divisor) return `${formatScaledInteger(integer, divisor, 2)}${suffix}`;
  }
  return integer.toString();
}

/**
 * A non-negative number a route sends as a float (a rate, a progress), cut at `decimals` (over
 * `divisor`) from the shortest decimal that reads back as it, never rounded (lane exactness, part
 * B3). A float written in exponent form is refused, named by `context`.
 */
export function cutFloat(
  value: number,
  decimals: number,
  context: string,
  divisor: bigint = BigInt(1)
): string {
  const [numerator, denominator] = exactDecimalRatio(String(value), context);
  return cutScaledInteger(numerator, denominator * divisor, decimals);
}

/** Exact decimal ratio/percentage text from an API string. Geometry may still use Number. */
export function formatDecimal(value: string, decimals: number, multiplier = 1): string {
  if (!Number.isSafeInteger(multiplier) || multiplier < 0) {
    throw new Error(`invalid decimal multiplier ${multiplier}`);
  }
  const [numerator, denominator] = exactDecimalRatio(value, 'decimal');
  return formatScaledInteger(numerator * BigInt(multiplier), denominator, decimals);
}

/** Non-negative decimal API value as a rational, before any chart geometry conversion. */
export function exactDecimalRatio(value: string, context: string): [bigint, bigint] {
  const match = /^(0|[1-9]\d*)(?:\.(\d+))?$/.exec(value);
  if (!match) throw new Error(`${context}: invalid decimal ${JSON.stringify(value)}`);
  const fraction = match[2] ?? '';
  return [BigInt(match[1] + fraction), BigInt(10) ** BigInt(fraction.length)];
}

/** Signed chart decimal; preserve the unsigned API contract above. */
export function exactSignedDecimalRatio(value: string, context: string): [bigint, bigint] {
  const negative = value.startsWith('-');
  const [numerator, denominator] = exactDecimalRatio(negative ? value.slice(1) : value, context);
  return [negative ? -numerator : numerator, denominator];
}

/** Compact amount cut, never rounded, from an exact decimal rational. */
export function formatCompactRatio(numerator: bigint, denominator: bigint): string {
  if (numerator < BigInt(0) || denominator <= BigInt(0)) {
    throw new Error('invalid compact decimal ratio');
  }
  for (const [suffix, unit] of [
    ['E', BigInt(10) ** BigInt(18)],
    ['P', BigInt(10) ** BigInt(15)],
    ['T', BigInt(10) ** BigInt(12)],
    ['B', BigInt(10) ** BigInt(9)],
    ['M', BigInt(10) ** BigInt(6)],
    ['k', BigInt(10) ** BigInt(3)],
  ] as const) {
    // a unit from where the value reaches it (cut: 999,999.99 is 999.99k, never 1.00M)
    if (numerator >= denominator * unit) {
      return `${formatScaledInteger(numerator, denominator * unit, 2)}${suffix}`;
    }
  }
  return formatScaledInteger(numerator, denominator, 2);
}

/**
 * An exact (optionally signed) decimal string with its whole part grouped in thousands. Every
 * fractional digit is kept: this never rounds, so it is the path for values shown exactly.
 */
export function formatExactDecimal(value: string, context: string): string {
  const match = /^([+-])?(0|[1-9]\d*)(?:\.(\d+))?$/.exec(value);
  if (!match) throw new Error(`${context}: invalid decimal ${JSON.stringify(value)}`);
  const [, sign = '', whole, fraction] = match;
  return `${sign}${BigInt(whole).toLocaleString('en-US')}${fraction ? `.${fraction}` : ''}`;
}
