// Lane B4 second fix round (review-rules-fixes-tests SHOULD-4, the coordinator's item 6): the
// producer fixture is what the API could send. Each row's owner address spells the lock the row
// carries: a cell's `lock` script (code hash, hash type, args), or a gallery row's named lock
// (`lockCodeHash`), as the API writes both from one stored lock. A row that carries only its
// owner's lock hash (a spore's or a cluster's detail) has the address every row of that lock hash
// has, one of which carries the lock.
import { describe, expect, it } from 'vitest';
import * as producer from '@/__tests__/msw/fixtures/item-producer';
import { addressScript } from '@/lib/lock-script';
import { scriptHash } from '@/__tests__/utils/ckb-hash';

interface Owned {
  path: string;
  ownerAddress: string;
  lock?: { codeHash: string; hashType: string; args: string };
  lockCodeHash?: string | null;
  ownerLockHash?: string;
}

/** Every object under `value` that has an owner address, with where it is. */
function owned(value: unknown, path: string, out: Owned[] = []): Owned[] {
  if (Array.isArray(value)) value.forEach((v, i) => owned(v, `${path}[${i}]`, out));
  else if (value !== null && typeof value === 'object') {
    const row = value as Record<string, unknown>;
    if (typeof row.ownerAddress === 'string') out.push({ ...(row as unknown as Owned), path });
    for (const [key, v] of Object.entries(row)) owned(v, `${path}.${key}`, out);
  }
  return out;
}

describe('the producer fixture’s owners', () => {
  const rows = Object.entries(producer).flatMap(([name, value]) => owned(value, name));

  const carries = (row: Owned) => row.lock !== undefined || row.lockCodeHash !== undefined;

  it('has owners to check, each beside its lock or its lock hash', () => {
    expect(rows.length).toBeGreaterThan(20);
    for (const row of rows)
      expect([row.path, carries(row) || row.ownerLockHash !== undefined]).toEqual([row.path, true]);
  });

  it('gives one lock hash one address, and every lock hash a row that carries its lock', () => {
    const byHash = new Map<string, Owned[]>();
    for (const row of rows)
      if (row.ownerLockHash !== undefined)
        byHash.set(row.ownerLockHash, [...(byHash.get(row.ownerLockHash) ?? []), row]);
    for (const [hash, same] of byHash) {
      expect([hash, [...new Set(same.map((r) => r.ownerAddress))].length]).toEqual([hash, 1]);
      expect([hash, same.some(carries)]).toEqual([hash, true]);
    }
  });

  // B4 third round (the coordinator's item 8): an owner's lock hash is the hash of the lock its
  // address spells, as the API derives both from one stored lock
  it('carries, as each owner’s lock hash, the hash of the lock its address spells', () => {
    const disagree = rows.flatMap((row) =>
      row.ownerLockHash !== undefined &&
      row.ownerLockHash !== scriptHash(addressScript(row.ownerAddress))
        ? [
            `${row.path}: ${row.ownerLockHash} is not ${scriptHash(addressScript(row.ownerAddress))}`,
          ]
        : []
    );
    expect(rows.filter((row) => row.ownerLockHash !== undefined).length).toBeGreaterThan(5);
    expect(disagree).toEqual([]);
  });

  it('spells, in each owner address, the lock its row carries', () => {
    const disagree = rows.filter(carries).flatMap((row) => {
      const spelled = addressScript(row.ownerAddress);
      const agrees =
        row.lock !== undefined
          ? spelled.codeHash === row.lock.codeHash &&
            spelled.hashType === row.lock.hashType &&
            spelled.args === row.lock.args
          : spelled.codeHash === row.lockCodeHash;
      return agrees ? [] : [row.path];
    });
    expect(disagree).toEqual([]);
  });
});
