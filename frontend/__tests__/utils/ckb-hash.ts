/**
 * CKB's hash for fixtures (lane B4 third round, the coordinator's item 8): BLAKE2b-256 with the
 * personalisation "ckb-default-hash" (RFC 0022), and a script's hash over its molecule encoding, so
 * a fixture's lock hash is checked against the lock it names instead of trusted. Written apart from
 * any code under test; `__tests__/lib/ckb-hash.test.ts` holds it to published hashes.
 */

const MASK = (BigInt(1) << BigInt(64)) - BigInt(1);
const IV = [
  '6a09e667f3bcc908',
  'bb67ae8584caa73b',
  '3c6ef372fe94f82b',
  'a54ff53a5f1d36f1',
  '510e527fade682d1',
  '9b05688c2b3e6c1f',
  '1f83d9abfb41bd6b',
  '5be0cd19137e2179',
].map((h) => BigInt(`0x${h}`));
const SIGMA = [
  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
  [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
  [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
  [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
  [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
  [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
  [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
  [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
  [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
  [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];
const PERSONAL = new TextEncoder().encode('ckb-default-hash');
const HASH_TYPE_BYTE: Record<string, number> = { data: 0, type: 1, data1: 2, data2: 4 };

const rotr = (x: bigint, n: number) => ((x >> BigInt(n)) | (x << BigInt(64 - n))) & MASK;

function word(bytes: Uint8Array, at: number): bigint {
  let w = BigInt(0);
  for (let i = 7; i >= 0; i--) w = (w << BigInt(8)) | BigInt(bytes[at + i]);
  return w;
}

function compress(h: bigint[], block: Uint8Array, counter: bigint, last: boolean) {
  const m = Array.from({ length: 16 }, (_, i) => word(block, 8 * i));
  const v = [...h, ...IV];
  v[12] ^= counter & MASK;
  v[13] ^= counter >> BigInt(64);
  if (last) v[14] ^= MASK;
  const g = (a: number, b: number, c: number, d: number, x: bigint, y: bigint) => {
    v[a] = (v[a] + v[b] + x) & MASK;
    v[d] = rotr(v[d] ^ v[a], 32);
    v[c] = (v[c] + v[d]) & MASK;
    v[b] = rotr(v[b] ^ v[c], 24);
    v[a] = (v[a] + v[b] + y) & MASK;
    v[d] = rotr(v[d] ^ v[a], 16);
    v[c] = (v[c] + v[d]) & MASK;
    v[b] = rotr(v[b] ^ v[c], 63);
  };
  for (let round = 0; round < 12; round++) {
    const s = SIGMA[round % 10];
    g(0, 4, 8, 12, m[s[0]], m[s[1]]);
    g(1, 5, 9, 13, m[s[2]], m[s[3]]);
    g(2, 6, 10, 14, m[s[4]], m[s[5]]);
    g(3, 7, 11, 15, m[s[6]], m[s[7]]);
    g(0, 5, 10, 15, m[s[8]], m[s[9]]);
    g(1, 6, 11, 12, m[s[10]], m[s[11]]);
    g(2, 7, 8, 13, m[s[12]], m[s[13]]);
    g(3, 4, 9, 14, m[s[14]], m[s[15]]);
  }
  for (let i = 0; i < 8; i++) h[i] ^= v[i] ^ v[i + 8];
}

function hex(hexString: string): Uint8Array {
  const h = hexString.replace(/^0x/, '');
  if (h.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(h)) throw new Error(`not hex: ${hexString}`);
  return Uint8Array.from({ length: h.length / 2 }, (_, i) =>
    parseInt(h.slice(2 * i, 2 * i + 2), 16)
  );
}

/** CKB's hash of some bytes: BLAKE2b, a 32-byte digest, personalised "ckb-default-hash". */
export function ckbHash(data: Uint8Array): string {
  const h = IV.slice();
  // the parameter block: a 32-byte digest, no key, fanout 1, depth 1; the personalisation
  h[0] ^= BigInt(0x01010020);
  h[6] ^= word(PERSONAL, 0);
  h[7] ^= word(PERSONAL, 8);
  const blocks = Math.max(1, Math.ceil(data.length / 128));
  let counter = BigInt(0);
  for (let i = 0; i < blocks; i++) {
    const chunk = data.subarray(128 * i, 128 * i + 128);
    const block = new Uint8Array(128);
    block.set(chunk);
    counter += BigInt(chunk.length);
    compress(h, block, counter, i === blocks - 1);
  }
  const out: string[] = [];
  for (let i = 0; i < 4; i++)
    for (let b = 0; b < 8; b++)
      out.push(
        Number((h[i] >> BigInt(8 * b)) & BigInt(0xff))
          .toString(16)
          .padStart(2, '0')
      );
  return `0x${out.join('')}`;
}

/** A script's hash: CKB's hash of its molecule table (code hash, hash type byte, args as Bytes). */
export function scriptHash(script: { codeHash: string; hashType: string; args: string }): string {
  const code = hex(script.codeHash);
  if (code.length !== 32) throw new Error(`a code hash is 32 bytes: ${script.codeHash}`);
  const hashType = HASH_TYPE_BYTE[script.hashType];
  if (hashType === undefined) throw new Error(`not a CKB hash type: ${script.hashType}`);
  const args = hex(script.args);
  const u32 = (n: number) => [n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff];
  const total = 16 + 32 + 1 + 4 + args.length;
  return ckbHash(
    Uint8Array.from([
      ...u32(total),
      ...u32(16),
      ...u32(48),
      ...u32(49),
      ...code,
      hashType,
      ...u32(args.length),
      ...args,
    ])
  );
}
