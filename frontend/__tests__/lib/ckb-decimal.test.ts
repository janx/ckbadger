import { describe, expect, it } from 'vitest';
import { MINUS } from '@/lib/chart-ticks';
import {
  exactCkbText,
  formatDecimalBytes,
  formatPercent,
  formatSignedCkbExact,
  formatSignedCkbInteger,
  formatSignedCount,
  parseCkbDecimal,
  parseShannons,
} from '@/lib/ckb-decimal';

describe('parseCkbDecimal', () => {
  it('reads CKB decimals exactly as shannons', () => {
    expect(parseCkbDecimal('8131306075.44505907')).toBe(BigInt('813130607544505907'));
    expect(parseCkbDecimal('8128895616.437997')).toBe(BigInt('812889561643799700'));
    expect(parseCkbDecimal('151104471')).toBe(BigInt('15110447100000000'));
    expect(parseCkbDecimal('-2578825.48689593')).toBe(BigInt('-257882548689593'));
    expect(parseCkbDecimal('0')).toBe(BigInt(0));
  });

  it('keeps the exact difference two floats would round away', () => {
    const a = parseCkbDecimal('8131474441.92489293');
    const b = parseCkbDecimal('8131306075.44505907');
    expect(a - b).toBe(BigInt('16836647983386'));
  });

  it('rejects anything that is not a CKB decimal with at most 8 places', () => {
    expect(() => parseCkbDecimal('1.123456789')).toThrow(/at most 8 decimals/);
    expect(() => parseCkbDecimal('1e9')).toThrow(/not a CKB amount/);
    expect(() => parseCkbDecimal('')).toThrow(/not a CKB amount/);
    expect(() => parseCkbDecimal('12,000')).toThrow(/not a CKB amount/);
  });
});

describe('parseShannons', () => {
  it('reads an integer shannon string and names the field when it is not one', () => {
    expect(parseShannons('812889561643799700', 'daoLocked')).toBe(BigInt('812889561643799700'));
    expect(() => parseShannons('8.1', 'daoLocked')).toThrow('daoLocked "8.1" is not an integer');
  });
});

describe('formatSignedCkbInteger', () => {
  it('prints whole CKB with an explicit sign, truncated toward zero', () => {
    expect(formatSignedCkbInteger(BigInt('8299100000000'))).toBe('+82,991');
    expect(formatSignedCkbInteger(BigInt('16836647983386'))).toBe('+168,366');
    expect(formatSignedCkbInteger(BigInt('-257882548689593'))).toBe(`${MINUS}2,578,825`);
    expect(formatSignedCkbInteger(BigInt(0))).toBe('0');
    expect(formatSignedCkbInteger(BigInt(5))).toBe('+0');
  });
});

describe('formatSignedCkbExact', () => {
  it('prints every shannon with an explicit sign', () => {
    expect(formatSignedCkbExact(BigInt('16836647983386'))).toBe('+168,366.47983386');
    expect(formatSignedCkbExact(BigInt('-257882548689593'))).toBe(`${MINUS}2,578,825.48689593`);
    expect(formatSignedCkbExact(BigInt(0))).toBe('0.00000000');
    expect(formatSignedCkbExact(BigInt(1))).toBe('+0.00000001');
  });
});

describe('formatSignedCount', () => {
  it('signs a count', () => {
    expect(formatSignedCount(-3)).toBe(`${MINUS}3`);
    expect(formatSignedCount(1200)).toBe('+1,200');
    expect(formatSignedCount(0)).toBe('0');
    expect(() => formatSignedCount(1.5)).toThrow(/integer/);
  });
});

describe('formatPercent', () => {
  // re-pinned for lane exactness, part B2: one percentage, cut by the one cut function, never rounded (it was half
  // up: 2 of 3 read 66.67, 5 of 8 at none 63)
  it('cuts an exact share, never rounds it', () => {
    const total = BigInt('4986878303450933125');
    // 0.30306…% and 16.30051…%: the same digits cut as rounded
    expect(formatPercent(BigInt('15113424500000000'), total)).toBe('0.30');
    expect(formatPercent(BigInt('812889561643799700'), total)).toBe('16.30');
    expect(formatPercent(BigInt(1), BigInt(8), 1)).toBe('12.5');
    expect(formatPercent(BigInt(1), BigInt(3), 0)).toBe('33');
    // 66.666…% is 66.66 (half up 66.67); 62.5 % at none is 62 (half up 63)
    expect(formatPercent(BigInt(2), BigInt(3), 2)).toBe('66.66');
    expect(formatPercent(BigInt(5), BigInt(8), 0)).toBe('62');
  });

  it('rejects a non-positive total or a negative part', () => {
    expect(() => formatPercent(BigInt(1), BigInt(0))).toThrow(/total must be positive/);
    expect(() => formatPercent(BigInt(-1), BigInt(10))).toThrow(/must not be negative/);
  });
});

describe('formatDecimalBytes', () => {
  // Re-pinned on purpose (lane exactness, part B3): the unit is the one the exact value reaches and
  // the figure is cut in it, as the hash rate's (part B, S3). It chose the unit at
  // unit − unit / 20,000 and rounded half up, so 999,950 bytes read 1.0 MB where they are 999.9 kB.
  it('chooses the unit the exact value reaches and cuts in it', () => {
    for (const [unit, previous, next] of [
      [BigInt(1e6), 'kB', 'MB'],
      [BigInt(1e9), 'MB', 'GB'],
      [BigInt(1e12), 'GB', 'TB'],
    ] as const) {
      expect(formatDecimalBytes(unit - unit / BigInt(20000))).toBe(`999.9 ${previous}`);
      expect(formatDecimalBytes(unit - BigInt(1))).toBe(`999.9 ${previous}`);
      expect(formatDecimalBytes(unit)).toBe(`1.0 ${next}`);
    }
  });

  it('prints decimal units with one decimal, cut', () => {
    expect(formatDecimalBytes(BigInt(151_134_245))).toBe('151.1 MB');
    // 41,590,000,000 bytes are 41.59 GB: 41.5 (half up read 41.6)
    expect(formatDecimalBytes(BigInt(41_590_000_000))).toBe('41.5 GB');
    expect(formatDecimalBytes(BigInt(999))).toBe('999 B');
  });
});

describe('exactCkbText', () => {
  it('writes the digits CkbAmount prints in its column form: every decimal, the sign only when signed, zero as 0', () => {
    expect(exactCkbText(BigInt(1647), false)).toBe('0.00001647');
    // a negative where no sign is written is refused, as CkbAmount refuses it (re-pinned, review
    // of the fixes M2: this wrote its magnitude, '0.00030000')
    expect(() => exactCkbText(BigInt(-30000), false)).toThrow(
      'CKB amount -30000 shannons is negative where a sign is not written'
    );
    expect(exactCkbText(BigInt(-30000), true)).toBe(`${MINUS}0.00030000`);
    expect(exactCkbText(BigInt('75000000000'), true)).toBe('+750.00000000');
    expect(exactCkbText(BigInt('193912572831'), false)).toBe('1,939.12572831');
    expect(exactCkbText(BigInt(0), false)).toBe('0');
    expect(exactCkbText(BigInt(0), true)).toBe('0');
  });
});
