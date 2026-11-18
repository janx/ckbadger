/**
 * Real CKB addresses for fixtures (RFC 0021 full format, bech32m). A row reads a party's lock off
 * its address (`lib/lock-script.ts`, readability lane B4), so a fixture party carries an address
 * the chain could have written, not a string that only looks like one. This encoder is written
 * apart from the decoder it feeds, so the two check each other.
 */
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';

const CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const GENERATOR = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
const HASH_TYPE_BYTE = { data: 0, type: 1, data1: 2, data2: 4 } as const;

/** JoyID's lock code hash (mainnet), for a party on a lock that is not the default. */
export const JOYID_CODE_HASH = '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323';

function polymod(values: number[]): number {
  let chk = 1;
  for (const v of values) {
    const top = chk >>> 25;
    chk = ((chk & 0x1ffffff) << 5) ^ v;
    GENERATOR.forEach((g, i) => {
      if ((top >>> i) & 1) chk ^= g;
    });
  }
  return chk;
}

function bytes(hex: string): number[] {
  const h = hex.replace(/^0x/, '');
  if (h.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(h)) throw new Error(`not hex: ${hex}`);
  return Array.from({ length: h.length / 2 }, (_, i) => parseInt(h.slice(2 * i, 2 * i + 2), 16));
}

/** The full-format address of a lock script. */
export function ckbAddress({
  codeHash,
  hashType = 'type',
  args,
  network = 'ckb',
}: {
  codeHash: string;
  hashType?: keyof typeof HASH_TYPE_BYTE;
  args: string;
  network?: 'ckb' | 'ckt';
}): string {
  if (bytes(codeHash).length !== 32) throw new Error(`a code hash is 32 bytes: ${codeHash}`);
  const payload = [0x00, ...bytes(codeHash), HASH_TYPE_BYTE[hashType], ...bytes(args)];
  const words: number[] = [];
  let acc = 0;
  let bits = 0;
  for (const b of payload) {
    acc = (acc << 8) | b;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      words.push((acc >> bits) & 31);
    }
    acc &= (1 << bits) - 1;
  }
  if (bits > 0) words.push((acc << (5 - bits)) & 31);
  const hrp = [...network].map((c) => c.charCodeAt(0));
  const expanded = [...hrp.map((c) => c >> 5), 0, ...hrp.map((c) => c & 31)];
  const mod = polymod([...expanded, ...words, 0, 0, 0, 0, 0, 0]) ^ 0x2bc830a3;
  const checksum = Array.from({ length: 6 }, (_, i) => (mod >>> (5 * (5 - i))) & 31);
  return `${network}1${[...words, ...checksum].map((w) => CHARSET[w]).join('')}`;
}

/** A made-up code hash for a fixture lock with this name: its characters' codes, padded to 32 bytes. */
export function namedLockCodeHash(name: string): string {
  const spelled = [...name].map((c) => c.charCodeAt(0).toString(16).padStart(2, '0')).join('');
  return `0x${spelled.slice(0, 64).padEnd(64, '0')}`;
}

/**
 * A fixture party's address, told apart by a short seed (a test's "tail"): the lock with `codeHash`
 * (the default lock unless named) over 20 bytes of args spelled from the seed's characters.
 */
export function seededAddress(seed: string, codeHash: string = DEFAULT_LOCK_CODE_HASH): string {
  const args = [...seed]
    .map((c) => c.charCodeAt(0).toString(16).padStart(2, '0'))
    .join('')
    .slice(0, 40)
    .padStart(40, '0');
  return ckbAddress({ codeHash, args: `0x${args}` });
}
