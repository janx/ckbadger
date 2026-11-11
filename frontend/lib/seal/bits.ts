/**
 * The hash a seal is drawn from, read as the prototype reads it: 256 bits, each hex digit most
 * significant bit first (seal2.js `bits`, the same order as `lib/night/bits.ts`, which this
 * reuses). A seal reads the bits in order through a `BitReader`; every slice it takes is kept,
 * so a glyph can say which bits chose it and its variant.
 */

import { bitsOf } from '@/lib/night/bits';

/** The only hash a seal is drawn from: a 32-byte hash in lowercase hex. */
export const SEAL_HASH = /^0x[0-9a-f]{64}$/;

export function assertSealHash(hash: string): void {
  if (typeof hash !== 'string' || !SEAL_HASH.test(hash))
    throw new Error(`hash seal: not a lowercase 32-byte hash: ${JSON.stringify(hash)}`);
}

/** A run of the hash's bits: where it starts (0–255) and the bits, 0 or 1. */
export interface BitSlice {
  readonly at: number;
  readonly bits: readonly number[];
}

/** The number a slice spells, most significant bit first. */
export function valueOf(slice: BitSlice): number {
  let v = 0;
  for (const b of slice.bits) v = (v << 1) | b;
  return v;
}

/**
 * The hash's bits read front to back. The prototype wraps round past bit 256; no seal here
 * reads that far (the bound is pinned by a test), so reading past the end is an error.
 */
export class BitReader {
  private readonly all: Uint8Array;
  private cur = 0;

  constructor(hash: string) {
    assertSealHash(hash);
    this.all = bitsOf(hash);
  }

  /** How many bits have been read. */
  get at(): number {
    return this.cur;
  }

  read(k: number): BitSlice {
    if (!Number.isInteger(k) || k < 0) throw new Error(`hash seal: cannot read ${k} bits`);
    if (this.cur + k > this.all.length)
      throw new Error(
        `hash seal: the seal reads past bit ${this.all.length} (at ${this.cur}, wants ${k} more)`
      );
    const slice = { at: this.cur, bits: Array.from(this.all.subarray(this.cur, this.cur + k)) };
    this.cur += k;
    return slice;
  }
}
