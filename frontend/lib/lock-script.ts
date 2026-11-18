/**
 * What a party's lock is, from what a row knows of it (readability 2026-11-18, R9 and lane B4).
 *
 * An address is shortened to `ckb1…` and its tail (`lib/shorten.ts`): the characters after `ckb1`
 * spell the lock's code hash, the same for every address of a lock, so they tell no two rows apart
 * and a shortened address no longer says which lock it is. The lock is named beside the address
 * instead, and only when it is not the default lock: no name beside an address means the default
 * lock. A row of a feed knows its party's address and the registry's name of its lock (the API's
 * `lockKind`), not the lock's code hash; the address holds it, so it is read off the address here.
 *
 * Nothing is guessed: an address is decoded exactly (RFC 0021's full format, the only one the API
 * writes: bech32m under `ckb` or `ckt`, its payload `0x00`, the 32-byte code hash, the hash type,
 * the args), its checksum checked, and anything else is an error that names the address.
 */
import type { ScriptRefHashType } from '@/lib/script-ref';

/**
 * The default lock's code hash: SECP256K1/blake160 by type (RFC 0024's system script, the same on
 * mainnet and testnet; `docs/metadata/scripts/default-lock.toml`, its `canonical_ref_hash`). The
 * registry names a lock by its code hash, so this is the code hash it names "Default Lock" by.
 */
export const DEFAULT_LOCK_CODE_HASH =
  '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';

/** Whether a lock's code hash is the default lock's: then a row writes no lock name. */
export function isDefaultLock(codeHash: string): boolean {
  return codeHash.toLowerCase() === DEFAULT_LOCK_CODE_HASH;
}

/** The lock script an address encodes. */
export interface AddressScript {
  codeHash: string;
  hashType: ScriptRefHashType;
  args: string;
}

const CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
/** BIP-350: the constant a bech32m checksum leaves. */
const BECH32M = 0x2bc830a3;
const GENERATOR = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
/** RFC 0021: the full format's payload starts with this byte. */
const FULL_FORMAT = 0x00;
const HASH_TYPES: Readonly<Record<number, ScriptRefHashType>> = {
  0: 'data',
  1: 'type',
  2: 'data1',
  4: 'data2',
};

function polymod(values: readonly number[]): number {
  let chk = 1;
  for (const value of values) {
    const top = chk >>> 25;
    chk = ((chk & 0x1ffffff) << 5) ^ value;
    for (let i = 0; i < 5; i++) if ((top >>> i) & 1) chk ^= GENERATOR[i];
  }
  return chk;
}

function hrpExpand(hrp: string): number[] {
  const codes = [...hrp].map((c) => c.charCodeAt(0));
  return [...codes.map((c) => c >> 5), 0, ...codes.map((c) => c & 31)];
}

/** Five-bit groups to bytes; what is left over must be fewer than five bits, all zero. */
function bytesOf(words: readonly number[], address: string): number[] {
  const bytes: number[] = [];
  let acc = 0;
  let bits = 0;
  for (const word of words) {
    acc = ((acc << 5) | word) & 0xfff;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((acc >> bits) & 0xff);
    }
  }
  if (bits >= 5 || (acc & ((1 << bits) - 1)) !== 0)
    throw new Error(`address ${address}: its payload does not end on a whole byte`);
  return bytes;
}

const hex = (bytes: readonly number[]) =>
  `0x${bytes.map((b) => b.toString(16).padStart(2, '0')).join('')}`;

/** The lock script an address encodes, read off the address (RFC 0021, full format). */
export function addressScript(address: string): AddressScript {
  const at = address.lastIndexOf('1');
  const hrp = address.slice(0, at);
  if (hrp !== 'ckb' && hrp !== 'ckt')
    throw new Error(`address ${address}: not a CKB address (its prefix is not ckb1 or ckt1)`);
  const words = [...address.slice(at + 1)].map((c) => {
    const word = CHARSET.indexOf(c);
    if (word < 0) throw new Error(`address ${address}: ${JSON.stringify(c)} is not bech32`);
    return word;
  });
  if (words.length < 6) throw new Error(`address ${address}: shorter than its checksum`);
  if (polymod([...hrpExpand(hrp), ...words]) !== BECH32M)
    throw new Error(`address ${address}: its bech32m checksum does not match`);
  const payload = bytesOf(words.slice(0, -6), address);
  if (payload[0] !== FULL_FORMAT)
    throw new Error(
      `address ${address}: format 0x${(payload[0] ?? 0).toString(16).padStart(2, '0')} is not the full format`
    );
  if (payload.length < 34)
    throw new Error(`address ${address}: its payload is too short for a code hash and hash type`);
  const hashType = HASH_TYPES[payload[33]];
  if (hashType === undefined)
    throw new Error(`address ${address}: hash type byte ${payload[33]} is not a CKB hash type`);
  return {
    codeHash: hex(payload.slice(1, 33)),
    hashType,
    args: hex(payload.slice(34)),
  };
}
