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
    if (integer >= divisor - divisor / BigInt(200000))
      return `${formatScaledInteger(integer, divisor, 2)}${suffix}`;
  }
  return integer.toString();
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

/** Compact amount rounded half-up from an exact decimal rational. */
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
    ['K', BigInt(10) ** BigInt(3)],
  ] as const) {
    if (numerator * BigInt(200000) >= denominator * unit * BigInt(199999)) {
      return `${formatScaledInteger(numerator, denominator * unit, 2)}${suffix}`;
    }
  }
  return formatScaledInteger(numerator, denominator, 2);
}

/** Share of exact decimal values. Zero total has no defined percentage. */
export function formatExactShare(values: readonly [bigint, bigint][], index: number): string {
  const gcd = (a: bigint, b: bigint): bigint => {
    while (b !== BigInt(0)) [a, b] = [b, a % b];
    return a;
  };
  if (
    values.some(([numerator, denominator]) => numerator < BigInt(0) || denominator <= BigInt(0))
  ) {
    throw new Error('invalid chart share ratio');
  }
  const commonDenominator = values.reduce(
    (lcm, [, denominator]) => (lcm / gcd(lcm, denominator)) * denominator,
    BigInt(1)
  );
  const scaled = values.map(
    ([numerator, denominator]) => numerator * (commonDenominator / denominator)
  );
  const part = scaled[index];
  if (part === undefined) throw new Error(`missing chart share index ${index}`);
  const total = scaled.reduce((sum, part) => sum + part, BigInt(0));
  if (total === BigInt(0)) return '—';
  return `${formatScaledInteger(part * BigInt(100), total, 2)}%`;
}
