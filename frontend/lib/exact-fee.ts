/** Exact shannons-per-byte fee rates for readouts; Number is used only for chart geometry. */
export interface ExactFeeRate {
  fee: bigint;
  size: bigint;
}

export function exactFeeRate(fee: string, size: number, context: string): ExactFeeRate {
  if (!/^(0|[1-9]\d*)$/.test(fee)) {
    throw new Error(`${context}: invalid fee ${fee}`);
  }
  if (!Number.isSafeInteger(size) || size <= 0) {
    throw new Error(`${context}: invalid transaction size ${size}`);
  }
  return { fee: BigInt(fee), size: BigInt(size) };
}

function formatRatioHalfUp(numerator: bigint, denominator: bigint): string {
  if (numerator < BigInt(0) || denominator <= BigInt(0)) {
    throw new Error(`invalid fee-rate ratio ${numerator}/${denominator}`);
  }
  const cents = (numerator * BigInt(200) + denominator) / (denominator * BigInt(2));
  return `${cents / BigInt(100)}.${(cents % BigInt(100)).toString().padStart(2, '0')}`;
}

export function formatExactFeeRate(rate: ExactFeeRate | null | undefined): string {
  if (rate == null) return 'N/A';
  return `${formatRatioHalfUp(rate.fee, rate.size)} sh/B`;
}

export function medianExactFeeRate(rates: Array<ExactFeeRate | null | undefined>): string {
  const valid = rates
    .filter((rate): rate is ExactFeeRate => rate != null)
    .sort((a, b) => {
      const left = a.fee * b.size;
      const right = b.fee * a.size;
      return left < right ? -1 : left > right ? 1 : 0;
    });
  if (valid.length === 0) return 'N/A';
  const middle = Math.floor(valid.length / 2);
  if (valid.length % 2 === 1) return formatExactFeeRate(valid[middle]);
  const lower = valid[middle - 1];
  const upper = valid[middle];
  const numerator = lower.fee * upper.size + upper.fee * lower.size;
  const denominator = BigInt(2) * lower.size * upper.size;
  return `${formatRatioHalfUp(numerator, denominator)} sh/B`;
}
