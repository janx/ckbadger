import { describe, expect, it } from 'vitest';
import { MINUS } from '@/lib/chart-ticks';
import {
  formatDecimalBytes,
  formatPercent,
  formatSignedCkbExact,
  formatSignedCkbInteger,
  formatSignedCount,
  parseCkbDecimal,
  parseShannons,
  splitCkbAmount,
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
  it('rounds an exact share half-up', () => {
    const total = BigInt('4986878303450933125');
    expect(formatPercent(BigInt('15113424500000000'), total)).toBe('0.30');
    expect(formatPercent(BigInt('812889561643799700'), total)).toBe('16.30');
    expect(formatPercent(BigInt(1), BigInt(8), 1)).toBe('12.5');
    expect(formatPercent(BigInt(1), BigInt(3), 0)).toBe('33');
    expect(formatPercent(BigInt(2), BigInt(3), 2)).toBe('66.67');
  });

  it('rejects a non-positive total or a negative part', () => {
    expect(() => formatPercent(BigInt(1), BigInt(0))).toThrow(/total must be positive/);
    expect(() => formatPercent(BigInt(-1), BigInt(10))).toThrow(/must not be negative/);
  });
});

describe('formatDecimalBytes', () => {
  it('promotes a rounded 1000.0 to the next decimal unit', () => {
    for (const [unit, previous, next] of [
      [BigInt(1e6), 'kB', 'MB'],
      [BigInt(1e9), 'MB', 'GB'],
      [BigInt(1e12), 'GB', 'TB'],
    ] as const) {
      const threshold = unit - unit / BigInt(20000);
      expect(formatDecimalBytes(threshold - BigInt(1))).toBe(`999.9 ${previous}`);
      expect(formatDecimalBytes(threshold)).toBe(`1.0 ${next}`);
    }
  });

  it('prints decimal units with one decimal', () => {
    expect(formatDecimalBytes(BigInt(151_134_245))).toBe('151.1 MB');
    expect(formatDecimalBytes(BigInt(41_590_000_000))).toBe('41.6 GB');
    expect(formatDecimalBytes(BigInt(999))).toBe('999 B');
  });
});

describe('splitCkbAmount', () => {
  it('splits exact CKB into sign, integer, significant decimals and ghosted trailing zeros', () => {
    expect(splitCkbAmount(BigInt(-30000))).toEqual({
      sign: MINUS,
      integer: '0',
      fraction: '.0003',
      ghost: '0000',
    });
    expect(splitCkbAmount(BigInt('41232486750'))).toEqual({
      sign: '+',
      integer: '412',
      fraction: '.3248675',
      ghost: '0',
    });
    expect(splitCkbAmount(BigInt('-75000000000'))).toEqual({
      sign: MINUS,
      integer: '750',
      fraction: '',
      ghost: '.00000000',
    });
    expect(splitCkbAmount(BigInt('193912572831'))).toEqual({
      sign: '+',
      integer: '1,939',
      fraction: '.12572831',
      ghost: '',
    });
  });

  it('gives zero no sign, all eight decimals ghosted', () => {
    expect(splitCkbAmount(BigInt(0))).toEqual({
      sign: '',
      integer: '0',
      fraction: '',
      ghost: '.00000000',
    });
  });

  it('always keeps 8 decimals in total, so right-aligned amounts line up', () => {
    for (const s of ['1', '-123456789', '100000000', '-1234567890123456']) {
      const p = splitCkbAmount(BigInt(s));
      expect((p.fraction + p.ghost).length).toBe(9);
    }
  });
});
