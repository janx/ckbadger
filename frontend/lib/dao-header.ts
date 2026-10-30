/** The header's four little-endian u64 fields, without economic reinterpretation. */
export interface DaoHeader {
  C: bigint;
  AR: bigint;
  S: bigint;
  U: bigint;
}

export function decodeDaoHeader(hex: string, context: string): DaoHeader {
  if (!/^0x[0-9a-fA-F]{64}$/.test(hex)) {
    throw new Error(`${context}: DAO header must contain exactly 32 hexadecimal bytes`);
  }
  const words = [0, 1, 2, 3].map((word) => {
    const bytes = hex.slice(2 + word * 16, 18 + word * 16).match(/../g)!;
    return BigInt(`0x${bytes.reverse().join('')}`);
  });
  return { C: words[0], AR: words[1], S: words[2], U: words[3] };
}

/** AR is stored ×10¹⁶ (RFC-0023); 1.0 at genesis. */
export const AR_SCALE = BigInt('10000000000000000');

/** The accumulated rate as its exact 16-decimal value, e.g. `1.2065094518945692`. */
export function formatAccumulatedRate(ar: bigint): string {
  if (ar < BigInt(0)) throw new Error(`DAO AR ${ar} is negative`);
  const fraction = (ar % AR_SCALE).toString().padStart(16, '0');
  return `${ar / AR_SCALE}.${fraction}`;
}
