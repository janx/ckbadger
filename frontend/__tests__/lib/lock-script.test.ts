import { describe, expect, it } from 'vitest';
import { ckbAddress, seededAddress } from '@/__tests__/utils/ckb-address';
import { addressScript, DEFAULT_LOCK_CODE_HASH, isDefaultLock } from '@/lib/lock-script';

// RFC 0021's own example: the default lock (SECP256K1/blake160 by type) over these args.
const ARGS = '0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64';
const RFC_FULL =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
const JOYID = '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323';

describe('addressScript (RFC 0021, full format)', () => {
  it('reads the default lock and its args off the RFC example', () => {
    expect(addressScript(RFC_FULL)).toEqual({
      codeHash: DEFAULT_LOCK_CODE_HASH,
      hashType: 'type',
      args: ARGS,
    });
  });

  it('reads a testnet address the same way', () => {
    expect(
      addressScript(
        'ckt1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqgutnjd'
      )
    ).toEqual({ codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'type', args: ARGS });
  });

  it('keeps the hash type: the default code hash by data is another lock', () => {
    const data =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl';
    expect(addressScript(data).hashType).toBe('data');
    const data2 =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsp9nnw7qkdnnclfkg59uzn8umtfd2kwxceqzcxpsa';
    expect(addressScript(data2).hashType).toBe('data2');
  });

  it('reads another lock’s code hash and its longer args', () => {
    expect(
      addressScript(
        'ckb1qrgqep8saj8agswr30pls73hra28ry8jlnlc3ejzh3dl2ju7xxpjxqgqqxeeh0qtxeeu05my2z7pfn7d45k4t8rvvsfavygk'
      )
    ).toEqual({ codeHash: JOYID, hashType: 'type', args: `0x0001${ARGS.slice(2)}` });
  });

  it('refuses what is not a full-format CKB address, naming it', () => {
    // a changed character breaks the checksum
    expect(() => addressScript(RFC_FULL.replace('dnnw7', 'dnnw8'))).toThrow(
      /checksum does not match/
    );
    // another network's prefix
    expect(() =>
      addressScript(
        'bc1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceq7lh8rz'
      )
    ).toThrow(/not a CKB address/);
    // the deprecated short format (format 0x01), even under a bech32m checksum
    expect(() => addressScript('ckb1qyqt8xaupvm8837nv3gtc9x0ekkj64vud3jquj5z3w')).toThrow(
      /format 0x01 is not the full format/
    );
    // a hash type the chain does not have
    expect(() =>
      addressScript(
        'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqannw7qkdnnclfkg59uzn8umtfd2kwxceqjz3ghp'
      )
    ).toThrow(/hash type byte 3/);
    // a payload too short to hold a code hash
    expect(() => addressScript('ckb1qzda0cr08m85hc8jlsfcdtqu')).toThrow(/too short/);
    // a character outside bech32, and a string too short for its checksum
    expect(() => addressScript('ckb1qzda0b')).toThrow(/"b" is not bech32/);
    expect(() => addressScript('ckb1qq')).toThrow(/shorter than its checksum/);
    expect(() => addressScript('ckb1qzda0')).toThrow('address ckb1qzda0');
  });
});

describe('the fixtures’ encoder (__tests__/utils/ckb-address.ts) against the decoder', () => {
  it('writes the RFC example, and every address it writes reads back', () => {
    expect(ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, args: ARGS })).toBe(RFC_FULL);
    expect(addressScript(seededAddress('sendr1'))).toEqual({
      codeHash: DEFAULT_LOCK_CODE_HASH,
      hashType: 'type',
      args: `0x${'00'.repeat(14)}73656e647231`,
    });
    expect(addressScript(seededAddress('q3q7ue', JOYID)).codeHash).toBe(JOYID);
  });
});

describe('isDefaultLock', () => {
  it('is the default lock’s code hash, in either case, and no other', () => {
    expect(isDefaultLock(DEFAULT_LOCK_CODE_HASH)).toBe(true);
    expect(isDefaultLock(DEFAULT_LOCK_CODE_HASH.toUpperCase().replace('0X', '0x'))).toBe(true);
    expect(isDefaultLock(JOYID)).toBe(false);
  });
});
