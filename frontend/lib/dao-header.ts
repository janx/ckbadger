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
