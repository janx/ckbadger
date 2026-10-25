/** Parse an API integer without passing it through IEEE-754 numbers. */
export function exactUnsignedInteger(value: string, context: string): bigint {
  if (!/^(0|[1-9]\d*)$/.test(value)) {
    throw new Error(
      `${context}: expected a non-negative decimal integer, received ${JSON.stringify(value)}`
    );
  }
  return BigInt(value);
}

/** Round half-up only at the requested display precision. */
export function formatScaledInteger(value: bigint, divisor: bigint, decimals: number): string {
  if (value < BigInt(0) || divisor <= BigInt(0) || !Number.isInteger(decimals) || decimals < 0) {
    throw new Error('invalid exact integer display scale');
  }
  const scale = BigInt(10) ** BigInt(decimals);
  const rounded = (value * scale * BigInt(2) + divisor) / (divisor * BigInt(2));
  if (decimals === 0) return rounded.toString();
  return `${rounded / scale}.${(rounded % scale).toString().padStart(decimals, '0')}`;
}

export function formatCompactInteger(value: string, context: string): string {
  const integer = exactUnsignedInteger(value, context);
  for (const [suffix, divisor] of [
    ['E', BigInt(10) ** BigInt(18)],
    ['P', BigInt(10) ** BigInt(15)],
    ['T', BigInt(10) ** BigInt(12)],
    ['B', BigInt(10) ** BigInt(9)],
    ['M', BigInt(10) ** BigInt(6)],
    ['K', BigInt(10) ** BigInt(3)],
  ] as const) {
    if (integer >= divisor) return `${formatScaledInteger(integer, divisor, 2)}${suffix}`;
  }
  return integer.toString();
}

/** Exact decimal ratio/percentage text from an API string. Geometry may still use Number. */
export function formatDecimal(value: string, decimals: number, multiplier = 1): string {
  const match = /^(0|[1-9]\d*)(?:\.(\d+))?$/.exec(value);
  if (!match) throw new Error(`invalid decimal ${JSON.stringify(value)}`);
  const fraction = match[2] ?? '';
  const numerator = BigInt(match[1] + fraction) * BigInt(multiplier);
  return formatScaledInteger(numerator, BigInt(10) ** BigInt(fraction.length), decimals);
}
