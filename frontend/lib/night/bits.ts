/**
 * The hash as what it is: 256 bits, most significant first. The hash seal reads its strokes from
 * these (`lib/talisman/bits.ts`); nothing here is derived, it is the hash itself.
 */

const HASH32 = /^0x[0-9a-fA-F]{64}$/;

/** The 256 bits of a 32-byte hash, most significant first. */
export function bitsOf(hash: string): Uint8Array {
  if (!HASH32.test(hash)) throw new Error(`night: not a 32-byte hash: ${hash}`);
  const out = new Uint8Array(256);
  const hex = hash.slice(2);
  for (let i = 0; i < 64; i++) {
    const nibble = parseInt(hex[i], 16);
    for (let b = 0; b < 4; b++) out[i * 4 + b] = (nibble >> (3 - b)) & 1;
  }
  return out;
}
