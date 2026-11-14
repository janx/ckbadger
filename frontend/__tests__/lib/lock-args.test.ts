import { describe, expect, it } from 'vitest';
import { decodedLockArgsText, isFieldLayoutLock, lockArgsLine } from '@/lib/lock-args';

describe('decodedLockArgsText (R2b-a 39 + R-8)', () => {
  // Pass 3 (revised R-8): the Shadow Lock's fields are a raw prefix byte and a raw 32-byte hash;
  // nothing names what the hash is.
  it('reads a Nervape Shadow Lock as its raw prefix byte plus its raw hash', () => {
    const decoded = { lock: 'shadow-lock', prefix: '0x07', hash: `0x31da${'00'.repeat(28)}5d74` };
    expect(isFieldLayoutLock(decoded)).toBe(true);
    // U-09 (re-pinned): a field's hex at the size of the line it stands in
    expect(decodedLockArgsText(decoded, 'row')).toBe('args 0x07 + hash 0x31da0000…005d74');
    expect(decodedLockArgsText(decoded, 'tight')).toBe('args 0x07 + hash 0x31da…5d74');
    expect(decodedLockArgsText(decoded, 'row')).not.toMatch(/CKBFS/);
  });

  it('shows the JoyID sub type as raw bytes, in layout order', () => {
    expect(
      decodedLockArgsText(
        { lock: 'joyid', subType: '0x0001', pubkeyHash: `0x${'55'.repeat(20)}` },
        'row'
      )
    ).toBe('args sub type 0x0001 + pubkey hash 0x55555555…555555');
  });

  it('keeps the R2b-a families as field words joined by a dot', () => {
    const decoded = { lock: 'secp256k1_blake160', pubkeyHash: `0x${'11'.repeat(20)}` };
    expect(isFieldLayoutLock(decoded)).toBe(false);
    expect(decodedLockArgsText(decoded, 'row')).toBe('pubkey hash 0x11111111…111111');
  });
});

// Pass 3 (revised R-8): args that do not fit the registry layout carry the API's reason.
describe('lockArgsLine (R-8 decode error)', () => {
  const reason = 'JoyID: args are 23 bytes, layout subType(2) + pubkeyHash(20) expects 22';
  it('says why the args were not decoded, in one quiet line', () => {
    expect(lockArgsLine(null, reason, 'row')).toBe(`args not decoded: ${reason}`);
  });
  it('reads decoded fields, and says nothing for a lock with no registry layout', () => {
    expect(
      lockArgsLine(
        { lock: 'joyid', subType: '0x0001', pubkeyHash: `0x${'55'.repeat(20)}` },
        null,
        'tight'
      )
    ).toBe('args sub type 0x0001 + pubkey hash 0x5555…5555');
    expect(lockArgsLine(null, null, 'row')).toBeNull();
    expect(lockArgsLine(undefined, undefined, 'row')).toBeNull();
  });
  it('refuses decoded fields and a decode error together (the contract never sends both)', () => {
    expect(() => lockArgsLine({ lock: 'joyid' }, reason, 'row')).toThrow(
      /both decoded and not decoded/
    );
  });
});
