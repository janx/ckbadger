// Lane B4 third round (the coordinator's item 8): the fixtures' hash, held to hashes published or
// computed apart from it (Python's hashlib.blake2b, digest_size 32, person b'ckb-default-hash').
import { describe, expect, it } from 'vitest';
import { ckbHash, scriptHash } from '@/__tests__/utils/ckb-hash';

describe('ckbHash', () => {
  it('hashes no bytes to CKB’s empty hash', () => {
    expect(ckbHash(new Uint8Array())).toBe(
      '0x44f4c69744d5f8c55d642062949dcae49bc4e7ef43d388c5a12f42b5633d163e'
    );
  });

  it('hashes one whole block and several blocks', () => {
    const bytes = (n: number) => Uint8Array.from({ length: n }, (_, i) => i % 256);
    expect(ckbHash(bytes(128))).toBe(
      '0xbd884467b7c169be49cdf82907f46ddc25c9bf36cf495eea856439e11a248b4d'
    );
    expect(ckbHash(bytes(768))).toBe(
      '0x855010ff18b2aaba857a7187532090c935f58b3391fff54cb16cf813b5b79d21'
    );
  });
});

describe('scriptHash', () => {
  it('hashes the Nervos DAO type script to its published hash', () => {
    expect(
      scriptHash({
        codeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
        hashType: 'type',
        args: '0x',
      })
    ).toBe('0xcc77c4deac05d68ab5b26828f0bf4565a8d73113d7bb7e92b8362b8a74e58e58');
  });

  it('hashes a lock with args', () => {
    expect(
      scriptHash({
        codeHash: `0x${'22'.repeat(32)}`,
        hashType: 'type',
        args: `0x${'33'.repeat(20)}`,
      })
    ).toBe('0xaa368106113229017f9e5ef36fdd33cc71e28cad8e738d8fbf9bd79a5cdad8ab');
  });

  it('refuses what is no script', () => {
    expect(() => scriptHash({ codeHash: '0x22', hashType: 'type', args: '0x' })).toThrow(
      'a code hash is 32 bytes: 0x22'
    );
    expect(() =>
      scriptHash({ codeHash: `0x${'22'.repeat(32)}`, hashType: 'data3', args: '0x' })
    ).toThrow('not a CKB hash type: data3');
  });
});
