import { describe, expect, it } from 'vitest';
import { decodedLockArgsText, isFieldLayoutLock } from '@/lib/lock-args';

describe('decodedLockArgsText (R2b-a 39 + R-8)', () => {
  it('reads a Nervape Shadow Lock as its prefix byte plus the CKBFS type hash', () => {
    const decoded = {
      lock: 'shadow-lock',
      prefix: '0x01',
      ckbfsTypeHash: `0x31da${'00'.repeat(28)}5d74`,
    };
    expect(isFieldLayoutLock(decoded)).toBe(true);
    expect(decodedLockArgsText(decoded)).toBe('args 0x01 + CKBFS type hash 0x31da00...5d74');
  });

  it('shows the JoyID sub type as raw bytes, in layout order', () => {
    expect(
      decodedLockArgsText({ lock: 'joyid', subType: '0x0001', pubkeyHash: `0x${'55'.repeat(20)}` })
    ).toBe('args sub type 0x0001 + pubkey hash 0x555555...5555');
  });

  it('keeps the R2b-a families as field words joined by a dot', () => {
    const decoded = { lock: 'secp256k1_blake160', pubkeyHash: `0x${'11'.repeat(20)}` };
    expect(isFieldLayoutLock(decoded)).toBe(false);
    expect(decodedLockArgsText(decoded)).toBe('pubkey hash 0x111111...1111');
  });
});
